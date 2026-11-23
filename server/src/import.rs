//! Garmin account data-export importer. Walks a zip (or an extracted
//! directory), recursing into nested zips, and sends every entry whose BYTES
//! look like FIT through [`crate::ingest::ingest`] with source `import`.
//! Everything else is counted by extension and not stored.
//!
//! Safety: names must be enclosed (no `..`, no absolute paths); nesting depth,
//! entry count, per-FIT size and total materialised bytes are capped, and caps
//! are enforced on bytes actually read, never on sizes the archive declares.

use crate::api::AppState;
use crate::ledger::Meta;
use crate::{derive, ingest, store};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct Limits {
    /// Zips inside zips (the top-level archive is depth 0).
    pub max_depth: u32,
    pub max_entries: u64,
    /// Bytes materialised: FIT entries plus nested zips spooled to disk.
    pub max_total_bytes: u64,
    pub max_fit_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 4,
            max_entries: 2_000_000,
            max_total_bytes: 64 << 30,
            max_fit_bytes: 64 << 20,
        }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub scanned: u64,
    pub fit_new: u64,
    pub fit_present: u64,
    /// Lower-case extension (or "(none)") -> archive-relative paths. With
    /// [`Options::keep_paths`] off only the extension counts are kept (the paths stay empty).
    pub non_fit: BTreeMap<String, Vec<String>>,
    /// Not FIT, not stored: how many (the sum of `non_fit`, paths kept or not).
    pub skipped: u64,
    /// Entries with absolute or escaping names; imported, name kept as metadata only.
    pub unsafe_names: Vec<String>,
    /// Error texts, at most [`Options::max_errors`] of them.
    pub errors: Vec<String>,
    /// Entries (and archives) that failed, counted past the cap on `errors`.
    pub failed: u64,
    /// FIT files by kind (see [`kind_name`]); only with [`Options::kinds`].
    pub kinds: BTreeMap<&'static str, u64>,
}

/// What a run keeps and how it names things.
#[derive(Debug, Clone)]
pub struct Options {
    pub limits: Limits,
    /// The user the imported files belong to (`None`: unassigned).
    pub owner: Option<i64>,
    /// In front of every archive-relative path in the upload origin (the uploaded file's name), so
    /// re-importing the same upload folds into the same event.
    pub prefix: String,
    /// Keep the path of every skipped entry (the command line lists them).
    pub keep_paths: bool,
    /// Keep at most this many error texts.
    pub max_errors: usize,
    /// Count the FIT files by kind (one query each).
    pub kinds: bool,
    /// Error texts say what went wrong without server paths (for a browser), and the cause of an
    /// ingest failure goes to the log only.
    pub plain_errors: bool,
}

impl Options {
    /// What `import-export` does: everything kept, texts as they come.
    pub fn cli(limits: Limits, owner: Option<i64>) -> Self {
        Self {
            limits,
            owner,
            prefix: String::new(),
            keep_paths: true,
            max_errors: usize::MAX,
            kinds: false,
            plain_errors: false,
        }
    }
}

/// The FIT file type (the `type` of its `file_id`) as a short word for the import summary.
pub fn kind_name(file_type: Option<i64>) -> &'static str {
    match file_type {
        Some(4) => "activity",
        Some(15 | 28 | 32) => "monitoring",
        Some(49) => "sleep",
        Some(44) => "metrics",
        _ => "other",
    }
}

enum Item {
    Fit(String, store::Received),
    /// Not stored: (extension label, archive-relative path).
    Other(String, String),
    /// A nested zip that is being walked.
    Container,
    /// A name that is absolute or escapes the archive. The entry is still
    /// imported (blobs are stored by hash, names never touch the filesystem).
    UnsafeName(String),
    /// An entry that could not be imported.
    EntryError(String),
    /// A failure that is not one entry (unreadable archive or directory, abort).
    Error(String),
}

/// `owner` is the user the imported files belong to (`None`: unassigned).
pub async fn run(
    state: &AppState,
    input: &Path,
    limits: Limits,
    owner: Option<i64>,
) -> Result<Report> {
    run_with(state, input, Options::cli(limits, owner)).await
}

/// [`run`] with every choice spelled out. `input` is an archive, a directory, or one file (a zip is
/// told from a plain file by its first bytes, never by its name).
pub async fn run_with(state: &AppState, input: &Path, opts: Options) -> Result<Report> {
    let (tx, mut rx) = mpsc::channel::<Item>(8);
    let (root, input) = (state.data_dir.as_ref().clone(), input.to_path_buf());
    let (limits, prefix) = (opts.limits.clone(), opts.prefix.clone());
    let walker = tokio::task::spawn_blocking(move || {
        let mut w = Walker {
            root,
            limits,
            tx,
            used: 0,
            entries: 0,
            stop: false,
        };
        w.start(&input, &prefix);
    });

    let mut r = Report::default();
    let owner = opts.owner;
    let fail = |r: &mut Report, text: String| {
        r.failed += 1;
        if r.errors.len() < opts.max_errors {
            r.errors.push(text);
        }
    };
    while let Some(item) = rx.recv().await {
        match item {
            Item::Other(ext, path) => {
                r.scanned += 1;
                r.skipped += 1;
                let paths = r.non_fit.entry(ext).or_default();
                if opts.keep_paths {
                    paths.push(path);
                }
            }
            Item::Container => r.scanned += 1,
            Item::UnsafeName(n) => r.unsafe_names.push(n),
            Item::EntryError(e) => {
                r.scanned += 1;
                fail(&mut r, e);
            }
            Item::Error(e) => fail(&mut r, e),
            Item::Fit(path, blob) => {
                r.scanned += 1;
                let meta = Meta {
                    source: "import".into(),
                    data_type: None,
                    sub_type: None,
                    file_index: None,
                    file_number: None,
                    watch_timestamp: None,
                };
                match ingest::ingest(state, &blob, None, owner, meta, Some(path.clone())).await {
                    Ok(created) => {
                        if created {
                            r.fit_new += 1;
                        } else {
                            r.fit_present += 1;
                        }
                        if opts.kinds {
                            let file_type: Option<Option<i64>> = sqlx::query_scalar(
                                "SELECT file_type FROM sources WHERE sha256 = ?",
                            )
                            .bind(&blob.sha256)
                            .fetch_optional(&state.pool)
                            .await
                            .unwrap_or(None);
                            *r.kinds.entry(kind_name(file_type.flatten())).or_default() += 1;
                        }
                    }
                    Err(e) if opts.plain_errors => {
                        tracing::error!(error = format!("{e:#}"), "import: ingest failed");
                        fail(&mut r, format!("{path}: could not be stored"));
                    }
                    Err(e) => fail(&mut r, format!("{path}: ingest failed: {e:#}")),
                }
            }
        }
    }
    walker.await.context("archive walker panicked")?;
    Ok(r)
}

fn ext_of(path: &str) -> String {
    Path::new(path)
        .extension()
        .map_or_else(|| "(none)".into(), |e| e.to_string_lossy().to_lowercase())
}

struct Walker {
    root: PathBuf,
    limits: Limits,
    tx: mpsc::Sender<Item>,
    used: u64,
    entries: u64,
    /// Set by a global limit or a closed channel: unwind without reading more.
    stop: bool,
}

/// Removes a spooled nested zip when dropped.
struct Spool(PathBuf);
impl Drop for Spool {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

impl Walker {
    fn send(&mut self, item: Item) {
        if self.tx.blocking_send(item).is_err() {
            self.stop = true;
        }
    }

    /// An entry failed: counted as scanned and as an error.
    fn error(&mut self, path: &str, msg: impl std::fmt::Display) {
        self.send(Item::EntryError(format!("{path}: {msg}")));
    }

    /// A container (archive, directory) failed: an error but not an entry.
    fn container_error(&mut self, path: &str, msg: impl std::fmt::Display) {
        self.send(Item::Error(format!("{path}: {msg}")));
    }

    fn abort(&mut self, msg: &str) {
        self.send(Item::Error(format!("import aborted: {msg}")));
        self.stop = true;
    }

    /// `prefix` goes in front of every path (for a single file it is the file's own name).
    fn start(&mut self, input: &Path, prefix: &str) {
        if input.is_dir() {
            self.walk_dir(input, prefix, 0);
            return;
        }
        let mut f = match File::open(input) {
            Ok(f) => f,
            Err(e) => return self.container_error(&input.display().to_string(), e),
        };
        // A zip is told by its first bytes; anything else is one file to classify (a plain .fit).
        let mut magic = Vec::with_capacity(4);
        let sniffed = (|| {
            (&mut f).take(4).read_to_end(&mut magic)?;
            f.rewind()?;
            f.metadata()
        })();
        let md = match sniffed {
            Ok(md) => md,
            Err(e) => return self.container_error(&input.display().to_string(), e),
        };
        if magic.starts_with(b"PK") || prefix.is_empty() {
            self.walk_zip(f, prefix, 0);
        } else if self.count_entry() {
            self.classify(f, prefix, md.len(), 0);
        }
    }

    fn count_entry(&mut self) -> bool {
        self.entries += 1;
        if self.entries > self.limits.max_entries {
            self.abort("entry count limit exceeded");
        }
        !self.stop
    }

    fn walk_dir(&mut self, dir: &Path, prefix: &str, depth: u32) {
        let mut names = vec![];
        match std::fs::read_dir(dir) {
            Ok(rd) => {
                for e in rd {
                    match e {
                        Ok(e) => names.push(e.path()),
                        Err(err) => {
                            self.container_error(&join(prefix, &dir.display().to_string()), err)
                        }
                    }
                }
            }
            Err(e) => return self.container_error(&join(prefix, &dir.display().to_string()), e),
        }
        names.sort();
        for p in names {
            if self.stop {
                return;
            }
            let rel = join(prefix, &p.file_name().unwrap().to_string_lossy());
            let Ok(md) = std::fs::symlink_metadata(&p) else {
                self.error(&rel, "cannot stat");
                continue;
            };
            if md.is_dir() {
                self.walk_dir(&p, &rel, depth);
            } else if md.is_file() {
                if !self.count_entry() {
                    return;
                }
                match File::open(&p) {
                    Ok(f) => self.classify(f, &rel, md.len(), depth),
                    Err(e) => self.error(&rel, e),
                }
            } else if self.count_entry() {
                let kind = if md.file_type().is_symlink() {
                    "(symlink)"
                } else {
                    "(special file)"
                };
                self.send(Item::Other(kind.into(), rel));
            }
        }
    }

    fn walk_zip<R: Read + Seek>(&mut self, r: R, prefix: &str, depth: u32) {
        let mut zip = match zip::ZipArchive::new(r) {
            Ok(z) => z,
            Err(e) => {
                return self.error(
                    if prefix.is_empty() {
                        "(archive)"
                    } else {
                        prefix
                    },
                    e,
                )
            }
        };
        if self.entries.saturating_add(zip.len() as u64) > self.limits.max_entries {
            return self.abort("entry count limit exceeded (declared by the archive)");
        }
        for i in 0..zip.len() {
            if self.stop {
                return;
            }
            let raw = zip
                .name_for_index(i)
                .and_then(Result::ok)
                .map(std::borrow::Cow::into_owned);
            let shown = join(prefix, raw.as_deref().unwrap_or(&format!("#{i}")));
            let entry = match zip.by_index(i) {
                Ok(e) => e,
                Err(e) => {
                    self.error(&shown, e);
                    continue;
                }
            };
            if entry.is_dir() {
                continue;
            }
            if !self.count_entry() {
                return;
            }
            // enclosed_name() would quietly strip a leading slash, so check the raw name too.
            let absolute = raw
                .as_deref()
                .is_some_and(|n| n.starts_with('/') || n.starts_with('\\'));
            if absolute || entry.enclosed_name().is_none() {
                self.send(Item::UnsafeName(shown.clone()));
            }
            let size = entry.size();
            self.classify(entry, &shown, size, depth);
        }
    }

    /// Decide by content: nested zip, FIT, or other.
    fn classify<R: Read>(&mut self, mut r: R, rel: &str, declared: u64, depth: u32) {
        let mut head = Vec::with_capacity(derive::SNIFF_LEN);
        if let Err(e) = (&mut r)
            .take(derive::SNIFF_LEN as u64)
            .read_to_end(&mut head)
        {
            return self.error(rel, e);
        }
        if head.starts_with(b"PK\x03\x04") {
            return self.nested_zip(r, head, rel, declared, depth);
        }
        if !derive::looks_like_fit(&head) {
            return self.send(Item::Other(ext_of(rel), rel.to_string()));
        }
        let cap = self.limits.max_fit_bytes;
        if declared > cap {
            return self.error(
                rel,
                format!("FIT entry declares {declared} bytes, over the {cap} limit; skipped"),
            );
        }
        let mut bytes = head;
        if let Err(e) = r.take(cap + 1 - bytes.len() as u64).read_to_end(&mut bytes) {
            return self.error(rel, e);
        }
        if bytes.len() as u64 > cap {
            return self.error(
                rel,
                format!("FIT entry exceeds the {cap} byte limit; skipped"),
            );
        }
        if !self.charge(bytes.len() as u64) {
            return;
        }
        match store::receive_bytes(&self.root, &bytes) {
            Ok(blob) => self.send(Item::Fit(rel.to_string(), blob)),
            Err(e) => self.error(rel, format!("{e:#}")),
        }
    }

    fn charge(&mut self, n: u64) -> bool {
        self.used = self.used.saturating_add(n);
        if self.used > self.limits.max_total_bytes {
            self.abort("total uncompressed byte limit exceeded (zip bomb?)");
        }
        !self.stop
    }

    fn nested_zip<R: Read>(&mut self, r: R, head: Vec<u8>, rel: &str, declared: u64, depth: u32) {
        if depth + 1 > self.limits.max_depth {
            return self.error(rel, "nested zip deeper than the depth limit; skipped");
        }
        let remaining = self.limits.max_total_bytes.saturating_sub(self.used);
        if declared > remaining {
            return self.abort("nested zip larger than the remaining byte budget");
        }
        let spool = match store::incoming_tmp(&self.root) {
            Ok(p) => Spool(p),
            Err(e) => return self.error(rel, format!("{e:#}")),
        };
        let copied = (|| -> Result<u64> {
            let mut f = File::create(&spool.0)?;
            let mut src = std::io::Cursor::new(head).chain(r).take(remaining + 1);
            Ok(std::io::copy(&mut src, &mut f)?)
        })();
        let n = match copied {
            Ok(n) => n,
            Err(e) => return self.error(rel, format!("spooling nested zip: {e:#}")),
        };
        if !self.charge(n) {
            return;
        }
        match File::open(&spool.0) {
            Ok(f) => {
                self.send(Item::Container);
                self.walk_zip(f, rel, depth + 1)
            }
            Err(e) => self.error(rel, e),
        }
    }
}
