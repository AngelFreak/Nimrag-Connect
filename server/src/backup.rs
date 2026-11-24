//! Backups and restore.
//!
//! One backup is a folder `nimrag-backup-<UTC time>` inside the backup directory:
//!
//! ```text
//! nimrag-backup-20261121T020000Z/
//!   nimrag.db      consistent snapshot, taken with `VACUUM INTO` (safe while `serve` runs)
//!   files/         the file store: blobs and their sidecars, same layout as the data dir
//!   backup.json    what was copied, for people and for `restore`
//! ```
//!
//! The database goes first, then the files. Files are never deleted, so every file the snapshot
//! knows is in the copy. A file that arrived in between is in `files/` without a database row;
//! `rebuild-ledger` adds it (unowned).
//!
//! Blobs are content-addressed and never change. Sidecars are rewritten atomically (a new
//! inode), never in place. So a file whose size and modification time match the copy in the
//! previous backup is hard-linked from it instead of copied again. If linking fails (another
//! filesystem, or one without hard links) the file is copied. A file in a backup is never
//! rewritten.
//!
//! A backup is written under a temporary name and renamed when complete. If anything fails the
//! temporary folder is removed and the older backups stay. Old backups are pruned only after a
//! new one succeeded. `backup` does not take the data-dir lock: it only reads.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Backup folders are named `nimrag-backup-20261121T020000Z` (a `-N` suffix when two start in
/// the same second).
pub const PREFIX: &str = "nimrag-backup-";
const TMP_PREFIX: &str = ".tmp-nimrag-backup-";
const MANIFEST: &str = "backup.json";
const BACKUP_LOCK: &str = ".backup.lock";
const FORMAT: u32 = 1;
/// A file modified more recently than this is not yet trusted to keep its modification time.
const SETTLE: Duration = Duration::from_millis(100);
/// After a failed scheduled backup, try again after this long at the latest.
const RETRY_AFTER: Duration = Duration::from_secs(3600);

/// What `serve` is asked to do: `--backup-dir`, `--backup-every`, `--backup-keep`.
#[derive(Debug, Clone)]
pub struct Config {
    pub dir: PathBuf,
    pub every: Duration,
    /// How many backups to keep (at least 1).
    pub keep: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub created_at: String,
    /// Newest migration in the snapshot.
    pub schema_version: Option<i64>,
    /// Blobs and sidecars in `files/`.
    pub files: u64,
    /// Their total size.
    pub files_bytes: u64,
    pub linked: u64,
    pub copied: u64,
    pub db_bytes: u64,
}

#[derive(Debug)]
pub struct Report {
    pub name: String,
    pub path: PathBuf,
    pub files: u64,
    pub files_bytes: u64,
    pub linked: u64,
    pub copied: u64,
    pub copied_bytes: u64,
    pub db_bytes: u64,
    pub duration: Duration,
    /// Old backups removed after this one succeeded.
    pub pruned: Vec<String>,
}

impl Report {
    /// The one log line (and CLI line) per backup.
    pub fn summary(&self) -> String {
        format!(
            "{}: {} files ({} bytes; {} hard-linked, {} copied, {} bytes written) + database {} bytes, {} ms",
            self.name,
            self.files,
            self.files_bytes,
            self.linked,
            self.copied,
            self.copied_bytes,
            self.db_bytes,
            self.duration.as_millis()
        )
    }
}

/// `6h`, `1d`, `30m`, `90s`: a whole number and a unit. A bare number is refused, it would be
/// a guess between seconds and hours.
pub fn parse_interval(s: &str) -> Result<Duration> {
    let s = s.trim();
    let (digits, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: u64 = digits
        .parse()
        .map_err(|_| anyhow::anyhow!("{s:?} is not an interval like 6h, 1d, 30m or 90s"))?;
    let unit = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => bail!("{s:?} needs a unit: s, m, h or d (for example 6h or 1d)"),
    };
    if n == 0 {
        bail!("an interval must be longer than zero");
    }
    n.checked_mul(unit)
        .map(Duration::from_secs)
        .with_context(|| format!("{s:?} is too long"))
}

// ---- finding backups ----------------------------------------------------------------------

/// A finished backup folder in the backup directory.
#[derive(Debug, Clone)]
pub struct Found {
    pub name: String,
    pub path: PathBuf,
    /// Unix seconds from the name.
    pub at: i64,
}

/// `(unix seconds, suffix)` of a backup folder name; `None` for any other name.
fn parse_name(name: &str) -> Option<(i64, u32)> {
    let rest = name.strip_prefix(PREFIX)?;
    let (stamp, n) = match rest.split_once('-') {
        Some((stamp, n)) => (stamp, n.parse().ok()?),
        None => (rest, 0),
    };
    let t = chrono::NaiveDateTime::parse_from_str(stamp, "%Y%m%dT%H%M%SZ").ok()?;
    Some((t.and_utc().timestamp(), n))
}

/// Finished backups in `dir`, oldest first. Anything else in `dir` is ignored.
pub fn list(dir: &Path) -> Result<Vec<Found>> {
    let mut out: Vec<(i64, u32, Found)> = vec![];
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    for e in rd {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some((at, n)) = parse_name(&name) {
            if e.path().is_dir() {
                out.push((
                    at,
                    n,
                    Found {
                        name,
                        path: e.path(),
                        at,
                    },
                ));
            }
        }
    }
    out.sort_by_key(|(at, n, _)| (*at, *n));
    Ok(out.into_iter().map(|(_, _, f)| f).collect())
}

// ---- one backup -----------------------------------------------------------------------------

/// Take one backup of `data_dir` into `out_dir` (created if missing) and, when `keep` is given,
/// prune all but the newest `keep` backups afterwards. `pool` can be the server's own pool or a
/// read-only one ([`crate::db::open_read_only`]).
pub async fn run_once(
    pool: &SqlitePool,
    data_dir: &Path,
    out_dir: &Path,
    keep: Option<usize>,
) -> Result<Report> {
    let started = Instant::now();
    fs::create_dir_all(out_dir)
        .with_context(|| format!("creating backup folder {}", out_dir.display()))?;
    refuse_inside(data_dir, out_dir)?;
    // One backup at a time per backup folder (the CLI may run while `serve` backs up).
    let _lock = lock_folder(out_dir)?;
    remove_leftovers(out_dir);

    let previous = list(out_dir)?.pop();
    let name = unique_name(out_dir);
    let tmp = out_dir.join(format!("{TMP_PREFIX}{}", &name[PREFIX.len()..]));
    let done = async {
        fs::create_dir(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        let db = tmp.join(crate::db::DB_FILE);
        snapshot(pool, &db).await?;
        let db_bytes = fs::metadata(&db)?.len();
        let schema_version: Option<i64> =
            sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations WHERE success = 1")
                .fetch_one(pool)
                .await
                .ok()
                .flatten();
        let src = data_dir.join("files");
        let dst = tmp.join("files");
        let prev = previous.as_ref().map(|p| p.path.join("files"));
        let stats = tokio::task::spawn_blocking(move || copy_store(&src, &dst, prev.as_deref()))
            .await
            .context("the copy task died")??;
        let manifest = Manifest {
            format: FORMAT,
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            schema_version,
            files: stats.files,
            files_bytes: stats.bytes,
            linked: stats.linked,
            copied: stats.copied,
            db_bytes,
        };
        let mut f = File::create(tmp.join(MANIFEST))?;
        serde_json::to_writer_pretty(&mut f, &manifest)?;
        f.sync_all()?;
        sync_dir(&tmp);
        let path = out_dir.join(&name);
        fs::rename(&tmp, &path).with_context(|| format!("renaming into {}", path.display()))?;
        sync_dir(out_dir);
        anyhow::Ok((path, stats, db_bytes))
    }
    .await;
    let (path, stats, db_bytes) = match done {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };

    let mut pruned = vec![];
    if let Some(keep) = keep {
        match prune(out_dir, keep) {
            Ok(p) => pruned = p,
            // The new backup is good; a folder that cannot be removed is not a failure of it.
            Err(e) => tracing::warn!(error = format!("{e:#}"), "pruning old backups failed"),
        }
    }
    Ok(Report {
        name,
        path,
        files: stats.files,
        files_bytes: stats.bytes,
        linked: stats.linked,
        copied: stats.copied,
        copied_bytes: stats.copied_bytes,
        db_bytes,
        duration: started.elapsed(),
        pruned,
    })
}

/// `VACUUM INTO`: a consistent copy of the live database in one read transaction. Writers keep
/// going while it runs.
async fn snapshot(pool: &SqlitePool, dest: &Path) -> Result<()> {
    let dest = dest
        .to_str()
        .context("the backup path is not valid UTF-8")?;
    sqlx::query("VACUUM INTO ?")
        .bind(dest)
        .execute(pool)
        .await
        .context("copying the database (VACUUM INTO)")?;
    File::open(dest)?.sync_all()?;
    Ok(())
}

/// A backup inside the data dir would be copied into itself next time.
fn refuse_inside(data_dir: &Path, out_dir: &Path) -> Result<()> {
    let data = data_dir.canonicalize()?;
    let out = out_dir.canonicalize()?;
    if out.starts_with(&data) {
        bail!(
            "the backup folder {} is inside the data dir {}; use a folder outside it, ideally on another disk",
            out.display(),
            data.display()
        );
    }
    Ok(())
}

fn lock_folder(out_dir: &Path) -> Result<File> {
    use std::fs::TryLockError;
    let f = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(out_dir.join(BACKUP_LOCK))?;
    match f.try_lock() {
        Ok(()) => Ok(f),
        Err(TryLockError::WouldBlock) => {
            bail!("another backup into {} is running", out_dir.display())
        }
        Err(TryLockError::Error(e)) => Err(e).context("locking the backup folder"),
    }
}

/// Temporary folders of a backup that died. Only called under the folder lock.
fn remove_leftovers(out_dir: &Path) {
    if let Ok(rd) = fs::read_dir(out_dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(TMP_PREFIX) {
                let _ = fs::remove_dir_all(e.path());
            }
        }
    }
}

/// `nimrag-backup-<UTC time>`, with `-N` appended when that second is taken. A name always
/// sorts after every existing backup, also when the folders of the same second were pruned.
fn unique_name(out_dir: &Path) -> String {
    let now = chrono::Utc::now();
    let mut stamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    let mut n = 0;
    if let Some(newest) = list(out_dir).ok().and_then(|l| l.into_iter().last()) {
        let (at, last_n) = parse_name(&newest.name).unwrap_or((0, 0));
        if at >= now.timestamp() {
            // The newest backup is from this second (or the clock went back): follow it.
            stamp = newest.name[PREFIX.len()..PREFIX.len() + 16].to_owned();
            n = last_n + 1;
        }
    }
    loop {
        let name = if n == 0 {
            format!("{PREFIX}{stamp}")
        } else {
            format!("{PREFIX}{stamp}-{n}")
        };
        if !out_dir.join(&name).exists() {
            return name;
        }
        n += 1;
    }
}

/// Remove all but the newest `keep` backups (never fewer than one is kept).
pub fn prune(dir: &Path, keep: usize) -> Result<Vec<String>> {
    let all = list(dir)?;
    let excess = all.len().saturating_sub(keep.max(1));
    let mut gone = vec![];
    for old in all.into_iter().take(excess) {
        fs::remove_dir_all(&old.path)
            .with_context(|| format!("removing {}", old.path.display()))?;
        gone.push(old.name);
    }
    Ok(gone)
}

fn sync_dir(dir: &Path) {
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
}

// ---- copying the file store -----------------------------------------------------------------

#[derive(Debug, Default)]
struct Stats {
    files: u64,
    bytes: u64,
    linked: u64,
    copied: u64,
    copied_bytes: u64,
}

struct Copier<'a> {
    stats: Stats,
    /// False once a hard link failed for a reason other than "no such file": stop trying.
    link: bool,
    prev: Option<&'a Path>,
}

fn copy_store(src: &Path, dst: &Path, prev: Option<&Path>) -> Result<Stats> {
    fs::create_dir_all(dst)?;
    let mut c = Copier {
        stats: Stats::default(),
        link: prev.is_some(),
        prev,
    };
    if src.is_dir() {
        c.walk(src, dst, Path::new(""))?;
    }
    Ok(c.stats)
}

impl Copier<'_> {
    /// Walk `src/rel`; skip every name that starts with a dot (`.incoming`, `.tmp-*`).
    fn walk(&mut self, src: &Path, dst: &Path, rel: &Path) -> Result<()> {
        let mut entries = fs::read_dir(src.join(rel))
            .with_context(|| format!("reading {}", src.join(rel).display()))?
            .collect::<io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let rel = rel.join(&name);
            let from = src.join(&rel);
            // `metadata` follows a symlink, and fails on a dangling one: a backup that cannot
            // read a file must fail, not skip it.
            let meta =
                fs::metadata(&from).with_context(|| format!("reading {}", from.display()))?;
            if meta.is_dir() {
                fs::create_dir_all(dst.join(&rel))?;
                self.walk(src, dst, &rel)?;
            } else if meta.is_file() {
                self.file(&from, &dst.join(&rel), &rel, &meta)?;
            }
        }
        Ok(())
    }

    fn file(&mut self, from: &Path, to: &Path, rel: &Path, meta: &fs::Metadata) -> Result<()> {
        self.stats.files += 1;
        self.stats.bytes += meta.len();
        if self.link {
            if let Some(prev) = self.prev.map(|p| p.join(rel)) {
                if same_file(&prev, meta) {
                    match fs::hard_link(&prev, to) {
                        Ok(()) => {
                            self.stats.linked += 1;
                            return Ok(());
                        }
                        // Another filesystem or none that links: copy from now on.
                        Err(e) if e.kind() != io::ErrorKind::NotFound => self.link = false,
                        Err(_) => {}
                    }
                }
            }
        }
        copy_file(from, to, meta)?;
        self.stats.copied += 1;
        self.stats.copied_bytes += meta.len();
        Ok(())
    }
}

/// The previous backup's copy has the size and modification time of the source file.
fn same_file(prev: &Path, src: &fs::Metadata) -> bool {
    let Ok(p) = fs::metadata(prev) else {
        return false;
    };
    p.is_file()
        && p.len() == src.len()
        && matches!((p.modified(), src.modified()), (Ok(a), Ok(b)) if a == b)
}

/// Copy `from` to a new file `to` (never over an existing one), keep the modification time (the
/// next backup compares it) and fsync.
fn copy_file(from: &Path, to: &Path, meta: &fs::Metadata) -> Result<()> {
    let mut r = File::open(from).with_context(|| format!("reading {}", from.display()))?;
    let mut w = File::create_new(to).with_context(|| format!("creating {}", to.display()))?;
    let n = io::copy(&mut r, &mut w).with_context(|| format!("copying {}", from.display()))?;
    if n != meta.len() {
        bail!(
            "{} changed while it was copied ({n} bytes read, {} expected)",
            from.display(),
            meta.len()
        );
    }
    if let Ok(m) = meta.modified() {
        // A file changed in the last instant could change again within the same clock tick and
        // keep its modification time. Stamp such a copy with the epoch so that the next backup
        // never takes it for unchanged and copies the file again.
        let fresh = SystemTime::now()
            .duration_since(m)
            .map_or(true, |age| age < SETTLE);
        let _ = w.set_modified(if fresh { SystemTime::UNIX_EPOCH } else { m });
    }
    w.sync_all()?;
    Ok(())
}

// ---- settings saved in the web app ------------------------------------------------------------

const KEY_ENABLED: &str = "backup_enabled";
const KEY_DIR: &str = "backup_dir";
const KEY_EVERY: &str = "backup_every_s";
const KEY_KEEP: &str = "backup_keep";
const KEYS: [&str; 4] = [KEY_ENABLED, KEY_DIR, KEY_EVERY, KEY_KEEP];

/// Bounds the web app enforces (the start-up options allow shorter intervals).
pub const MIN_EVERY_S: i64 = 3600;
pub const MAX_EVERY_S: i64 = 30 * 86_400;
pub const MAX_KEEP: i64 = 365;
pub const MAX_DIR_BYTES: usize = 1024;
const PROBE: &str = ".nimrag-write-test";

/// What the admin saved in the web app (the global `settings` table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub enabled: bool,
    /// Empty only while backups are off and no folder was ever given.
    pub dir: String,
    pub every_s: u32,
    pub keep: u32,
}

impl Saved {
    /// The scheduler's config; `None` when off.
    pub fn config(&self) -> Option<Config> {
        (self.enabled && !self.dir.is_empty()).then(|| Config {
            dir: PathBuf::from(&self.dir),
            every: Duration::from_secs(self.every_s as u64),
            keep: self.keep.max(1) as usize,
        })
    }
}

/// The saved settings; `None` when none were saved (or the stored values are unusable, which is
/// logged: the start-up options apply then).
pub async fn load_saved(pool: &SqlitePool) -> Result<Option<Saved>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, value FROM settings WHERE key IN ('backup_enabled', 'backup_dir', 'backup_every_s', 'backup_keep')",
    )
    .fetch_all(pool)
    .await?;
    let get = |k: &str| rows.iter().find(|r| r.0 == k).map(|r| r.1.as_str());
    if rows.is_empty() {
        return Ok(None);
    }
    let parsed = (|| {
        Some(Saved {
            enabled: match get(KEY_ENABLED)? {
                "1" => true,
                "0" => false,
                _ => return None,
            },
            dir: get(KEY_DIR)?.to_owned(),
            every_s: get(KEY_EVERY)?.parse().ok()?,
            keep: get(KEY_KEEP)?.parse().ok()?,
        })
    })();
    if parsed.is_none() {
        tracing::warn!("the saved backup settings are incomplete or unreadable; ignoring them");
    }
    Ok(parsed)
}

pub async fn store_saved(pool: &SqlitePool, s: &Saved) -> Result<()> {
    let mut tx = pool.begin().await?;
    for (k, v) in [
        (KEY_ENABLED, if s.enabled { "1" } else { "0" }.to_owned()),
        (KEY_DIR, s.dir.clone()),
        (KEY_EVERY, s.every_s.to_string()),
        (KEY_KEEP, s.keep.to_string()),
    ] {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        )
        .bind(k)
        .bind(v)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Forget the saved settings: the start-up options apply again.
pub async fn clear_saved(pool: &SqlitePool) -> Result<()> {
    let mut tx = pool.begin().await?;
    for k in KEYS {
        sqlx::query("DELETE FROM settings WHERE key = ?")
            .bind(k)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Check a folder the admin typed. The text is for the browser: plain, and without the data dir's
/// path. Blocking (file system).
pub fn check_folder(data_dir: &Path, dir: &Path) -> std::result::Result<(), String> {
    if !dir.is_absolute() {
        return Err("The folder must be a full path, starting with /.".into());
    }
    if !dir.is_dir() {
        return Err("This folder does not exist on the server. With Docker, mount a folder into the container first, for example /backups.".into());
    }
    let probe = dir.join(PROBE);
    if File::create(&probe).is_err() {
        return Err("The server cannot write to this folder.".into());
    }
    let _ = fs::remove_file(&probe);
    let (Ok(data), Ok(out)) = (data_dir.canonicalize(), dir.canonicalize()) else {
        return Err("This folder cannot be read.".into());
    };
    if out.starts_with(&data) || data.starts_with(&out) {
        return Err("This folder overlaps the server's data folder. Use a folder outside it, ideally on another disk.".into());
    }
    Ok(())
}

// ---- the scheduler ----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LastOk {
    pub name: String,
    /// Unix seconds when it finished.
    pub at: i64,
    pub duration_ms: u64,
    pub copied_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct LastError {
    /// Without the data dir's path.
    pub text: String,
    pub at: i64,
}

/// What the admin page shows. Kept in memory only.
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub running: bool,
    pub last_ok: Option<LastOk>,
    pub last_error: Option<LastError>,
    /// Unix seconds of the next scheduled backup; `None` when off.
    pub next_at: Option<i64>,
}

/// Where the scheduler's config comes from, live: the start-up options, or (when the admin saved
/// some) the settings of the web app. The scheduler follows [`Control::reload`] at once.
pub struct Control {
    startup: Option<Config>,
    tx: tokio::sync::watch::Sender<Option<Config>>,
    status: std::sync::Mutex<Status>,
    /// One backup at a time, scheduled or by hand.
    gate: Arc<tokio::sync::Mutex<()>>,
}

/// The right to run one backup. Dropping it ends "running".
pub struct RunGuard {
    ctl: Arc<Control>,
    _gate: tokio::sync::OwnedMutexGuard<()>,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.ctl.with_status(|s| s.running = false);
    }
}

impl Control {
    pub fn new(startup: Option<Config>) -> Self {
        Self {
            tx: tokio::sync::watch::channel(startup.clone()).0,
            startup,
            status: Default::default(),
            gate: Default::default(),
        }
    }

    /// The start-up options (`--backup-dir` and friends), if given.
    pub fn startup(&self) -> Option<&Config> {
        self.startup.as_ref()
    }

    /// The config the scheduler runs on now.
    pub fn current(&self) -> Option<Config> {
        self.tx.borrow().clone()
    }

    pub fn status(&self) -> Status {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn with_status(&self, f: impl FnOnce(&mut Status)) {
        f(&mut self.status.lock().unwrap_or_else(|e| e.into_inner()));
    }

    /// Re-read the saved settings and hand the result to the scheduler.
    pub async fn reload(&self, pool: &SqlitePool) -> Result<()> {
        let cfg = match load_saved(pool).await? {
            Some(saved) => saved.config(),
            None => self.startup.clone(),
        };
        self.tx.send_replace(cfg);
        Ok(())
    }

    /// Take the right to run a backup at once; `None` while one is running.
    pub fn try_begin(self: &Arc<Self>) -> Option<RunGuard> {
        let gate = self.gate.clone().try_lock_owned().ok()?;
        self.with_status(|s| s.running = true);
        Some(RunGuard {
            ctl: self.clone(),
            _gate: gate,
        })
    }

    /// Wait for a running backup to end, then take the right to run the next one.
    async fn begin(self: &Arc<Self>) -> RunGuard {
        let gate = self.gate.clone().lock_owned().await;
        self.with_status(|s| s.running = true);
        RunGuard {
            ctl: self.clone(),
            _gate: gate,
        }
    }
}

/// Run one backup under `guard`, record the outcome in the status and log it. Returns whether it
/// worked. Errors are shown to the browser, so the data dir's path is taken out of them.
pub async fn run_tracked(
    guard: RunGuard,
    pool: &SqlitePool,
    data_dir: &Path,
    dir: &Path,
    keep: usize,
) -> bool {
    let (pool2, data2, dir2) = (pool.clone(), data_dir.to_path_buf(), dir.to_path_buf());
    // Its own task, so even a panic ends one backup and not the caller.
    let result =
        tokio::spawn(async move { run_once(&pool2, &data2, &dir2, Some(keep)).await }).await;
    let now = chrono::Utc::now().timestamp();
    let ok = match result {
        Ok(Ok(r)) => {
            tracing::info!(
                backup = %r.name,
                files = r.files,
                files_bytes = r.files_bytes,
                linked = r.linked,
                copied = r.copied,
                db_bytes = r.db_bytes,
                pruned = r.pruned.len(),
                duration_ms = r.duration.as_millis() as u64,
                "backup done"
            );
            guard.ctl.with_status(|s| {
                s.last_error = None;
                s.last_ok = Some(LastOk {
                    name: r.name.clone(),
                    at: now,
                    duration_ms: r.duration.as_millis() as u64,
                    copied_bytes: r.copied_bytes,
                });
            });
            true
        }
        failed => {
            let text = match failed {
                Ok(Err(e)) => format!("{e:#}"),
                Ok(Ok(_)) => unreachable!(),
                Err(e) => format!("the backup task died: {e}"),
            };
            tracing::warn!(
                dir = %dir.display(),
                error = %text,
                "backup failed; the server keeps running"
            );
            let text = scrub(&text, data_dir);
            guard
                .ctl
                .with_status(|s| s.last_error = Some(LastError { text, at: now }));
            false
        }
    };
    drop(guard);
    ok
}

/// Take the data dir's path out of an error text (it is shown in the browser).
fn scrub(text: &str, data_dir: &Path) -> String {
    let mut out = text.to_owned();
    let mut paths = vec![data_dir.to_path_buf()];
    paths.extend(data_dir.canonicalize());
    for p in paths {
        out = out.replace(&p.display().to_string(), "(data folder)");
    }
    out
}

/// Back up every `cfg.every`, for as long as the server runs. See [`spawn_scheduler_with`].
pub fn spawn_scheduler(
    pool: SqlitePool,
    data_dir: PathBuf,
    cfg: Config,
) -> tokio::task::JoinHandle<()> {
    spawn_scheduler_with(pool, data_dir, Arc::new(Control::new(Some(cfg))))
}

/// Back up on the schedule of `ctl`, for as long as the server runs. The first backup is due when
/// the newest existing one is older than `every` (at once when there is none), so a restart does
/// not make an extra backup. A failed backup is logged as a warning and tried again after
/// `min(every, 1 h)`. A new config (a changed interval or folder, or off) is picked up at once;
/// off waits for the next change. Nothing here can stop the server.
pub fn spawn_scheduler_with(
    pool: SqlitePool,
    data_dir: PathBuf,
    ctl: Arc<Control>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut rx = ctl.tx.subscribe();
        let mut retry = false;
        loop {
            let cfg = rx.borrow_and_update().clone();
            let Some(cfg) = cfg else {
                ctl.with_status(|s| s.next_at = None);
                retry = false;
                if rx.changed().await.is_err() {
                    return;
                }
                continue;
            };
            let wait = if retry {
                cfg.every.min(RETRY_AFTER)
            } else {
                due_in(&cfg)
            };
            let next = chrono::Utc::now().timestamp() + wait.as_secs() as i64;
            ctl.with_status(|s| s.next_at = Some(next));
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                changed = rx.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    retry = false;
                    continue;
                }
            }
            let (guard, waited) = match ctl.try_begin() {
                Some(g) => (g, false),
                None => (ctl.begin().await, true),
            };
            // A backup by hand ran meanwhile: it may have made this one unnecessary.
            if waited && !due_in(&cfg).is_zero() {
                drop(guard);
                continue;
            }
            retry = !run_tracked(guard, &pool, &data_dir, &cfg.dir, cfg.keep).await;
        }
    })
}

/// How long until the next scheduled backup: `every` after the newest one, now if none.
pub fn due_in(cfg: &Config) -> Duration {
    let newest = list(&cfg.dir).ok().and_then(|l| l.last().map(|f| f.at));
    let Some(at) = newest else {
        return Duration::ZERO;
    };
    let age = (chrono::Utc::now().timestamp() - at).max(0) as u64;
    cfg.every.saturating_sub(Duration::from_secs(age))
}

// ---- restore ----------------------------------------------------------------------------------

#[derive(Debug)]
pub struct RestoreReport {
    pub files: u64,
    pub files_bytes: u64,
    pub db_bytes: u64,
    pub manifest: Option<Manifest>,
}

/// Copy the backup folder `from` into the new, empty data dir `data_dir`. A target that has
/// anything in it is refused. Afterwards the database is checked (`PRAGMA quick_check`). On any
/// failure the target is emptied again.
pub async fn restore(from: &Path, data_dir: &Path) -> Result<RestoreReport> {
    let db = from.join(crate::db::DB_FILE);
    if !db.is_file() {
        let hint = match list(from)?.pop() {
            Some(newest) => format!(
                "; {} holds backups, the newest is {}",
                from.display(),
                newest.path.display()
            ),
            None => String::new(),
        };
        bail!(
            "{} is not a backup folder (no nimrag.db){hint}",
            from.display()
        );
    }
    match fs::read_dir(data_dir) {
        Ok(mut rd) => {
            if rd.next().is_some() {
                bail!(
                    "{} is not empty; restore into a new or empty data dir",
                    data_dir.display()
                );
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("reading {}", data_dir.display())),
    }
    let existed = data_dir.exists();
    fs::create_dir_all(data_dir)
        .with_context(|| format!("creating data dir {}", data_dir.display()))?;
    // Holds `.lock` for the copy (a `serve` started on this dir meanwhile is refused).
    let lock = crate::db::lock_data_dir(data_dir, "restore")?;
    let result = restore_into(from, data_dir).await;
    drop(lock);
    if result.is_err() {
        if existed {
            if let Ok(rd) = fs::read_dir(data_dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    let _ = if p.is_dir() {
                        fs::remove_dir_all(&p)
                    } else {
                        fs::remove_file(&p)
                    };
                }
            }
        } else {
            let _ = fs::remove_dir_all(data_dir);
        }
    }
    result
}

async fn restore_into(from: &Path, data_dir: &Path) -> Result<RestoreReport> {
    let manifest: Option<Manifest> = fs::read(from.join(MANIFEST))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    if let Some(m) = &manifest {
        if m.format > FORMAT {
            bail!(
                "this backup has format {}, this build understands up to {FORMAT}",
                m.format
            );
        }
    }
    let (from2, to2) = (from.to_path_buf(), data_dir.to_path_buf());
    let (files, files_bytes) = tokio::task::spawn_blocking(move || {
        let mut c = Copier {
            stats: Stats::default(),
            link: false,
            prev: None,
        };
        let src = from2.join("files");
        let dst = to2.join("files");
        fs::create_dir_all(&dst)?;
        if src.is_dir() {
            c.walk(&src, &dst, Path::new(""))?;
        }
        let db_src = from2.join(crate::db::DB_FILE);
        let db_dst = to2.join(crate::db::DB_FILE);
        copy_file(&db_src, &db_dst, &fs::metadata(&db_src)?)?;
        sync_dir(&to2);
        anyhow::Ok((c.stats.files, c.stats.bytes))
    })
    .await
    .context("the copy task died")??;

    // Immutable: nothing else has the new database open, and no -wal or -shm file is left behind.
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(data_dir.join(crate::db::DB_FILE))
                .read_only(true)
                .immutable(true),
        )
        .await
        .context("opening the restored database")?;
    let check: String = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_one(&pool)
        .await
        .context("checking the restored database")?;
    pool.close().await;
    if check != "ok" {
        bail!("the restored database failed its check: {check}");
    }
    Ok(RestoreReport {
        files,
        files_bytes,
        db_bytes: fs::metadata(data_dir.join(crate::db::DB_FILE))?.len(),
        manifest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals() {
        assert_eq!(parse_interval("6h").unwrap(), Duration::from_secs(21_600));
        assert_eq!(parse_interval("1d").unwrap(), Duration::from_secs(86_400));
        assert_eq!(parse_interval("30m").unwrap(), Duration::from_secs(1800));
        assert_eq!(parse_interval(" 90s ").unwrap(), Duration::from_secs(90));
        for bad in [
            "",
            "6",
            "h",
            "0h",
            "-1h",
            "1.5h",
            "6 hours",
            "1w",
            "99999999999999999999d",
        ] {
            assert!(parse_interval(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn backup_names_sort_by_time_then_suffix() {
        let a = parse_name("nimrag-backup-20261121T020000Z").unwrap();
        let b = parse_name("nimrag-backup-20261121T020000Z-1").unwrap();
        let c = parse_name("nimrag-backup-20261121T020001Z").unwrap();
        assert!(a < b && b < c);
        assert_eq!(a.0, 1_795_226_400);
        for other in [
            ".tmp-nimrag-backup-20261121T020000Z",
            "nimrag-backup-notes",
            "nimrag-backup-20261121T020000Z-x",
            "backup.json",
        ] {
            assert!(parse_name(other).is_none(), "{other}");
        }
    }
}
