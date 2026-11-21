//! Backups and restore through the real entry points: the library function the scheduler runs,
//! the router while a backup is taken, and the real binary (`serve --backup-dir`, `backup`,
//! `restore`, `reparse`).
mod common;

use common::*;
use nimrag_core::derive::test_support::{FitBuilder, Fv};
use nimrag_server::{backup, db};
use sqlx::{Row, SqlitePool};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const T0: u32 = 0x3000_0000;

fn bin(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .args(args)
        .output()
        .unwrap()
}

fn ok(out: std::process::Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// Every table (and the migrations list) of a database, one sorted line per row, all columns
/// quoted by SQLite itself: two databases with equal dumps hold the same rows.
async fn dump_all(pool: &SqlitePool) -> String {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let mut out = String::new();
    for t in tables {
        let cols: Vec<String> = sqlx::query(sqlx::AssertSqlSafe(format!("PRAGMA table_info({t})")))
            .fetch_all(pool)
            .await
            .unwrap()
            .iter()
            .map(|r| r.get::<String, _>("name"))
            // The time a migration ran is not data.
            .filter(|c| {
                !(t == "_sqlx_migrations" && (c == "installed_on" || c == "execution_time"))
            })
            .collect();
        let expr = cols
            .iter()
            .map(|c| format!("quote({c})"))
            .collect::<Vec<_>>()
            .join(" || '|' || ");
        let sql = format!("SELECT {expr} FROM {t} ORDER BY {expr}");
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(pool)
            .await
            .unwrap();
        out.push_str(&format!("{t}: {} rows\n", rows.len()));
        for r in rows {
            out.push_str(&format!("{t}|{}\n", r.get::<String, _>(0)));
        }
    }
    out
}

/// Several kinds of rows and files: ledger, derived tables (with a decoder), per-user settings,
/// body entries, a second user.
async fn fill(e: &Env) {
    for n in 1..=3u32 {
        let (s, _) = e.upload("data_type=128", &tiny_fit(n)).await;
        assert_eq!(s, axum::http::StatusCode::CREATED);
    }
    e.upload("data_type=4", b"not a fit file").await;
    // The same bytes again: a repeat on the sidecar's event.
    e.upload("data_type=128", &tiny_fit(1)).await;
    let activity = FitBuilder::new()
        .message(0, &[(3, Fv::U32(42)), (4, Fv::U32(T0 - 100))])
        .message(20, &[(253, Fv::U32(T0)), (3, Fv::U8(120))])
        .message(
            18,
            &[(253, Fv::U32(T0 + 10)), (2, Fv::U32(T0)), (5, Fv::U8(1))],
        )
        .build();
    e.upload("data_type=4", &activity).await;
    e.adopt_unowned().await;
    e.exec("INSERT INTO body_measurements (user_id, measured_at, weight_kg, created_at) VALUES (1, 1000, 80.5, 1000)")
        .await;
    e.exec("INSERT INTO user_settings (user_id, key, value) VALUES (1, 'step_goal', '12000')")
        .await;
    e.exec("INSERT INTO hrv_values (user_id, ts, rmssd_ms, src) VALUES (1, 5000, 41.0, 1)")
        .await;
    e.add_user("second", nimrag_server::users::Role::User).await;
    // Files younger than this are copied again by the next backup (see `copy_file`); let these
    // age so the tests that count links see them as settled.
    tokio::time::sleep(Duration::from_millis(300)).await;
}

fn backups(dir: &Path) -> Vec<String> {
    backup::list(dir)
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect()
}

/// Everything in `dir`, files and folders, one name per entry (a leftover temp folder shows).
fn entries(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// The blob of `sha` in a data dir or a backup folder (same layout).
fn blob_path(root: &Path, sha: &str) -> PathBuf {
    stored_path(root, sha)
}

fn sidecar_path(root: &Path, sha: &str) -> PathBuf {
    let mut p = stored_path(root, sha).into_os_string();
    p.push(".json");
    PathBuf::from(p)
}

/// A path that does not exist yet, inside a temp dir that is removed with the returned guard.
fn fresh() -> (tempfile::TempDir, PathBuf) {
    let parent = tempfile::tempdir().unwrap();
    let path = parent.path().join("new");
    (parent, path)
}

#[tokio::test]
async fn backup_while_the_router_serves_restores_to_a_working_data_dir() {
    let e = env().await;
    fill(&e).await;
    let out = tempfile::tempdir().unwrap();

    // The server keeps taking uploads while the backup runs.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let uploader = {
        let (app, token, stop) = (e.app.clone(), e.token.clone(), stop.clone());
        tokio::spawn(async move {
            use tower::ServiceExt;
            let mut n = 1000u32;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let req = axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/files?data_type=128&source=import")
                    .header("authorization", format!("Bearer {token}"))
                    .body(axum::body::Body::from(tiny_fit(n)))
                    .unwrap();
                let r = app.clone().oneshot(req).await.unwrap();
                assert_eq!(r.status(), axum::http::StatusCode::CREATED);
                n += 1;
                tokio::task::yield_now().await;
            }
            n - 1000
        })
    };
    let r = backup::run_once(&e.pool, e.dir.path(), out.path(), Some(7))
        .await
        .unwrap();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let during = uploader.await.unwrap();
    assert!(r.files >= 10, "{}", r.summary());
    assert_eq!(backups(out.path()), vec![r.name.clone()]);
    assert!(
        !entries(out.path()).iter().any(|n| n.starts_with(".tmp")),
        "{:?}",
        entries(out.path())
    );
    eprintln!("{during} uploads ran during the backup: {}", r.summary());

    // The folder is a data dir with a database snapshot that passes its own check.
    let snap = db::open_read_only(&r.path).await.unwrap();
    let check: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&snap)
        .await
        .unwrap();
    assert_eq!(check, "ok");
    // Every file the snapshot knows is in the copy, blob and sidecar.
    let shas: Vec<String> = sqlx::query_scalar("SELECT sha256 FROM files")
        .fetch_all(&snap)
        .await
        .unwrap();
    assert!(shas.len() >= 5);
    for sha in &shas {
        let blob = blob_path(&r.path, sha);
        assert_eq!(
            &nimrag_server::sha256_hex(&std::fs::read(&blob).unwrap()),
            sha
        );
        assert!(sidecar_path(&r.path, sha).exists());
    }
    snap.close().await;

    // Restore into a new dir and start on it.
    let (_guard, target) = fresh();
    let rr = backup::restore(&r.path, &target).await.unwrap();
    assert_eq!(rr.files, r.files);
    let restored = db::open(&target).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
        .fetch_one(&restored)
        .await
        .unwrap();
    assert_eq!(n as usize, shas.len());
    // The token and the users came along: the restored server accepts the old token.
    let state = nimrag_server::api::AppState::new(restored.clone(), target.clone(), 1 << 20);
    let app = nimrag_server::api::router(state);
    use tower::ServiceExt;
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/files?data_type=4&source=import")
        .header("authorization", format!("Bearer {}", e.token))
        .body(axum::body::Body::from("after the restore"))
        .unwrap();
    assert_eq!(
        app.oneshot(req).await.unwrap().status(),
        axum::http::StatusCode::CREATED
    );
}

#[tokio::test]
async fn a_quiet_backup_restores_every_row_and_file() {
    let e = env().await;
    fill(&e).await;
    let out = tempfile::tempdir().unwrap();
    let r = backup::run_once(&e.pool, e.dir.path(), out.path(), None)
        .await
        .unwrap();

    let (_guard, target) = fresh();
    backup::restore(&r.path, &target).await.unwrap();
    let restored = db::open(&target).await.unwrap();
    let (a, b) = (dump_all(&e.pool).await, dump_all(&restored).await);
    assert!(a.contains("files: 5 rows"), "{a}");
    assert_eq!(a, b);

    // The same files, byte for byte, sidecars included.
    for sha in sqlx::query_scalar::<_, String>("SELECT sha256 FROM files")
        .fetch_all(&e.pool)
        .await
        .unwrap()
    {
        for suffix in ["", ".json"] {
            let name = format!("{sha}{suffix}");
            let rel = format!("files/{}/{}/{name}", &sha[0..2], &sha[2..4]);
            assert_eq!(
                std::fs::read(e.dir.path().join(&rel)).unwrap(),
                std::fs::read(target.join(&rel)).unwrap(),
                "{rel}"
            );
        }
    }
}

#[tokio::test]
async fn the_second_backup_links_unchanged_files_and_copies_only_what_changed() {
    let e = env().await;
    fill(&e).await;
    let out = tempfile::tempdir().unwrap();
    let first = backup::run_once(&e.pool, e.dir.path(), out.path(), None)
        .await
        .unwrap();
    assert_eq!(first.linked, 0);
    assert_eq!(first.copied, first.files);

    // Nothing changed: every file is a hard link to the first backup's, nothing is written.
    let second = backup::run_once(&e.pool, e.dir.path(), out.path(), None)
        .await
        .unwrap();
    assert_eq!(second.files, first.files);
    assert_eq!(second.linked, second.files, "{}", second.summary());
    assert_eq!(second.copied, 0);
    assert_eq!(second.copied_bytes, 0);
    let sha: String = sqlx::query_scalar("SELECT sha256 FROM files ORDER BY sha256 LIMIT 1")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    let (a, b) = (blob_path(&first.path, &sha), blob_path(&second.path, &sha));
    let (ma, mb) = (
        std::fs::metadata(&a).unwrap(),
        std::fs::metadata(&b).unwrap(),
    );
    assert_eq!(ma.ino(), mb.ino(), "same inode: linked, not copied");
    assert!(ma.nlink() >= 2, "first and second backup share the file");

    // A new file and a repeated upload (which rewrites one sidecar): only those are copied.
    let (s, _) = e.upload("data_type=128", &tiny_fit(99)).await;
    assert_eq!(s, axum::http::StatusCode::CREATED);
    let (s, _) = e.upload("data_type=128", &tiny_fit(2)).await;
    assert_eq!(s, axum::http::StatusCode::OK);
    let third = backup::run_once(&e.pool, e.dir.path(), out.path(), None)
        .await
        .unwrap();
    assert_eq!(third.files, first.files + 2);
    assert_eq!(
        third.copied, 3,
        "new blob, its sidecar, the rewritten sidecar"
    );
    assert_eq!(third.linked, third.files - 3);

    // The older backups are untouched by the later ones.
    let first_sidecar = std::fs::read_to_string(first.path.join(format!(
        "files/{}/{}/{}.json",
        &sha[0..2],
        &sha[2..4],
        sha
    )))
    .unwrap();
    assert!(first_sidecar.contains(&sha));
}

#[tokio::test]
async fn a_previous_copy_that_does_not_match_is_copied_not_linked() {
    // The previous backup is there but its file has another size than the live one (as a
    // damaged copy would). Linking it would carry the damage on; the file is copied instead.
    let e = env().await;
    fill(&e).await;
    let out = tempfile::tempdir().unwrap();
    let first = backup::run_once(&e.pool, e.dir.path(), out.path(), None)
        .await
        .unwrap();
    let sha: String = sqlx::query_scalar("SELECT sha256 FROM files ORDER BY sha256 LIMIT 1")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    let damaged = blob_path(&first.path, &sha);
    // Replace the previous backup's file by a different one of another size (never in place:
    // a link to it may be shared).
    std::fs::remove_file(&damaged).unwrap();
    std::fs::write(&damaged, b"x").unwrap();
    let second = backup::run_once(&e.pool, e.dir.path(), out.path(), None)
        .await
        .unwrap();
    assert_eq!(second.copied, 1);
    assert_eq!(
        std::fs::read(blob_path(&second.path, &sha)).unwrap(),
        std::fs::read(stored_path(e.dir.path(), &sha)).unwrap()
    );
}

#[tokio::test]
async fn pruning_keeps_the_newest_n_and_nothing_else_is_touched() {
    let e = env().await;
    fill(&e).await;
    let out = tempfile::tempdir().unwrap();
    std::fs::write(out.path().join("notes.txt"), "mine").unwrap();
    std::fs::create_dir(out.path().join("other")).unwrap();
    std::fs::create_dir(out.path().join("nimrag-backup-notes")).unwrap();
    let mut made = vec![];
    for _ in 0..5 {
        let r = backup::run_once(&e.pool, e.dir.path(), out.path(), Some(3))
            .await
            .unwrap();
        made.push(r.name);
    }
    // Names within one second get -1, -2 ...: five distinct names, the newest three remain.
    made.dedup();
    assert_eq!(made.len(), 5);
    assert_eq!(backups(out.path()), made[2..].to_vec());
    let rest = entries(out.path());
    for keep in ["notes.txt", "other", "nimrag-backup-notes"] {
        assert!(rest.contains(&keep.to_string()), "{rest:?}");
    }
    // Pruning never goes below one.
    let r = backup::run_once(&e.pool, e.dir.path(), out.path(), Some(0))
        .await
        .unwrap();
    assert_eq!(backups(out.path()), vec![r.name]);
    // The surviving backup is complete although its older neighbours were hard-linked to it.
    let (_guard, target) = fresh();
    let n = backup::restore(&r.path, &target).await.unwrap().files;
    assert_eq!(n, r.files);
}

#[tokio::test]
async fn a_failed_backup_leaves_no_partial_folder_and_keeps_the_old_ones() {
    let e = env().await;
    fill(&e).await;
    let out = tempfile::tempdir().unwrap();
    let good = backup::run_once(&e.pool, e.dir.path(), out.path(), Some(1))
        .await
        .unwrap();
    let before = entries(out.path());

    // A file the backup cannot read: a dangling link where a blob should be.
    let broken = e.dir.path().join("files/ab/cd");
    std::fs::create_dir_all(&broken).unwrap();
    let name = "ab".repeat(32);
    std::os::unix::fs::symlink("/nonexistent/blob", broken.join(&name)).unwrap();
    let err = backup::run_once(&e.pool, e.dir.path(), out.path(), Some(1))
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains(&name), "{err:#}");

    // No temporary folder, no half backup; the good one is still the only one (keep = 1 did
    // not prune it, because the new backup did not succeed).
    assert_eq!(entries(out.path()), before);
    assert_eq!(backups(out.path()), vec![good.name.clone()]);
    assert!(good.path.join("nimrag.db").is_file());

    // Repaired: the next backup works and prunes the old one.
    std::fs::remove_file(broken.join(&name)).unwrap();
    let next = backup::run_once(&e.pool, e.dir.path(), out.path(), Some(1))
        .await
        .unwrap();
    assert_eq!(backups(out.path()), vec![next.name]);
}

#[tokio::test]
async fn a_dead_backups_temporary_folder_is_removed_by_the_next_one() {
    let e = env().await;
    let out = tempfile::tempdir().unwrap();
    let stale = out.path().join(".tmp-nimrag-backup-20200101T000000Z");
    std::fs::create_dir_all(stale.join("files")).unwrap();
    std::fs::write(stale.join("nimrag.db"), "half").unwrap();
    backup::run_once(&e.pool, e.dir.path(), out.path(), None)
        .await
        .unwrap();
    assert!(!stale.exists());
}

#[tokio::test]
async fn a_backup_folder_inside_the_data_dir_is_refused() {
    let e = env().await;
    let inside = e.dir.path().join("backups");
    let err = backup::run_once(&e.pool, e.dir.path(), &inside, None)
        .await
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("inside the data dir"),
        "{err:#}"
    );
}

#[tokio::test]
async fn restore_refuses_a_target_that_is_not_empty_and_explains_a_parent_folder() {
    let e = env().await;
    fill(&e).await;
    let out = tempfile::tempdir().unwrap();
    let r = backup::run_once(&e.pool, e.dir.path(), out.path(), None)
        .await
        .unwrap();

    // The target has something in it: nothing is changed.
    let busy = tempfile::tempdir().unwrap();
    std::fs::write(busy.path().join("keep.txt"), "mine").unwrap();
    let err = backup::restore(&r.path, busy.path()).await.unwrap_err();
    assert!(format!("{err:#}").contains("not empty"), "{err:#}");
    assert_eq!(entries(busy.path()), vec!["keep.txt".to_string()]);

    // A restored dir is itself no target for another restore.
    let (_guard, target) = fresh();
    backup::restore(&r.path, &target).await.unwrap();
    assert!(backup::restore(&r.path, &target).await.is_err());

    // An empty existing dir is fine.
    let empty = tempfile::tempdir().unwrap();
    backup::restore(&r.path, empty.path()).await.unwrap();

    // Pointing at the folder that holds the backups names the newest one.
    let (_g2, x) = fresh();
    let err = backup::restore(out.path(), &x).await.unwrap_err();
    assert!(format!("{err:#}").contains(&r.name), "{err:#}");
    // Not a backup at all.
    assert!(backup::restore(busy.path(), &x).await.is_err());
    assert!(!x.exists());
}

#[tokio::test]
async fn a_restore_that_fails_leaves_the_target_empty() {
    let e = env().await;
    fill(&e).await;
    let out = tempfile::tempdir().unwrap();
    let r = backup::run_once(&e.pool, e.dir.path(), out.path(), None)
        .await
        .unwrap();
    // A blob that cannot be read in the backup.
    let sha: String = sqlx::query_scalar("SELECT sha256 FROM files LIMIT 1")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    let blob = blob_path(&r.path, &sha);
    std::fs::remove_file(&blob).unwrap();
    std::os::unix::fs::symlink("/nonexistent", &blob).unwrap();

    let empty = tempfile::tempdir().unwrap();
    assert!(backup::restore(&r.path, empty.path()).await.is_err());
    assert_eq!(entries(empty.path()), Vec::<String>::new());
    let (_guard, never) = fresh();
    assert!(backup::restore(&r.path, &never).await.is_err());
    assert!(!never.exists());
}

#[tokio::test]
async fn the_scheduler_backs_up_on_its_own_prunes_and_survives_a_failure() {
    let e = env().await;
    fill(&e).await;
    let out = tempfile::tempdir().unwrap();
    let cfg = backup::Config {
        dir: out.path().to_path_buf(),
        every: Duration::from_millis(300),
        keep: 2,
    };
    let handle = backup::spawn_scheduler(e.pool.clone(), e.dir.path().to_path_buf(), cfg);
    // None yet exists, so the first one is due at once; then one per interval, never more than 2.
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let now = backups(out.path());
        assert!(now.len() <= 2, "{now:?}");
        seen.extend(now);
        if seen.len() >= 4 {
            break;
        }
    }
    assert!(seen.len() >= 4, "scheduled backups made: {seen:?}");

    // A failing backup does not stop the loop (or the server): once the cause is gone it runs again.
    let broken = e.dir.path().join("files/ef/gh");
    std::fs::create_dir_all(&broken).unwrap();
    let name = "ef".repeat(32);
    std::os::unix::fs::symlink("/nonexistent", broken.join(&name)).unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(!handle.is_finished());
    let before: std::collections::BTreeSet<_> = backups(out.path()).into_iter().collect();
    std::fs::remove_file(broken.join(&name)).unwrap();
    let mut recovered = false;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let now: std::collections::BTreeSet<_> = backups(out.path()).into_iter().collect();
        if now.difference(&before).next().is_some() {
            recovered = true;
            break;
        }
    }
    assert!(recovered, "no backup after the failure was fixed");
    handle.abort();
}

#[tokio::test]
async fn the_scheduler_waits_when_the_newest_backup_is_still_fresh() {
    let e = env().await;
    let out = tempfile::tempdir().unwrap();
    let first = backup::run_once(&e.pool, e.dir.path(), out.path(), None)
        .await
        .unwrap();
    // A restart of the server right after a backup does not make another one.
    let handle = backup::spawn_scheduler(
        e.pool.clone(),
        e.dir.path().to_path_buf(),
        backup::Config {
            dir: out.path().to_path_buf(),
            every: Duration::from_secs(3600),
            keep: 7,
        },
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(backups(out.path()), vec![first.name]);
    handle.abort();
}

// ---- the real binary -------------------------------------------------------------------------

struct Server(std::process::Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_serve(data: &Path, extra: &[&str]) -> (Server, u16) {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let child = Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .args([
            "serve",
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--data-dir",
        ])
        .arg(data)
        .args(extra)
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let server = Server(child);
    for _ in 0..200 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return (server, port);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("server did not start");
}

fn post_file(port: u16, token: &str, body: &[u8]) -> String {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        s,
        "POST /api/files?data_type=4&source=import HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    s.write_all(body).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

fn wait_for(what: &str, f: impl Fn() -> bool) {
    for _ in 0..200 {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

/// The documented test (docs/RUNBOOK.md, "Check that your backups restore"): `serve` backs up by
/// itself, `backup` works while it runs, `restore` makes a data dir the server starts on with
/// the same token, and `reparse` on it gives the derived tables the original has.
#[test]
fn real_binary_serve_backs_up_backup_runs_beside_it_and_restore_then_reparse_matches() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let out2 = tempfile::tempdir().unwrap();
    let token = ok(bin(&[
        "token",
        "create",
        "--name",
        "t",
        "--data-dir",
        data.path().to_str().unwrap(),
    ]))
    .trim()
    .to_string();

    let (server, port) = start_serve(
        data.path(),
        &[
            "--backup-dir",
            out.path().to_str().unwrap(),
            "--backup-every",
            "1h",
            "--backup-keep",
            "3",
        ],
    );
    // Upload real files over HTTP.
    for n in 1..=3u32 {
        assert!(post_file(port, &token, &tiny_fit(n)).starts_with("HTTP/1.1 201"));
    }
    let activity = FitBuilder::new()
        .message(0, &[(3, Fv::U32(7)), (4, Fv::U32(T0 - 100))])
        .message(20, &[(253, Fv::U32(T0)), (3, Fv::U8(120))])
        .message(
            18,
            &[(253, Fv::U32(T0 + 10)), (2, Fv::U32(T0)), (5, Fv::U8(1))],
        )
        .build();
    assert!(post_file(port, &token, &activity).starts_with("HTTP/1.1 201"));

    std::thread::sleep(Duration::from_millis(300)); // settle, see `copy_file`
                                                    // serve made its first backup at start (none existed). It had no files yet: that is fine,
                                                    // the next one, taken below by the CLI while serve still holds the lock, has them.
    wait_for("the scheduled backup", || !backups(out.path()).is_empty());
    let stdout = ok(bin(&[
        "backup",
        "--data-dir",
        data.path().to_str().unwrap(),
        "--out",
        out2.path().to_str().unwrap(),
    ]));
    assert!(stdout.contains("backup written to"), "{stdout}");
    assert!(stdout.contains("8 files"), "{stdout}");
    // Run it again: nothing new, everything linked.
    let stdout = ok(bin(&[
        "backup",
        "--data-dir",
        data.path().to_str().unwrap(),
        "--out",
        out2.path().to_str().unwrap(),
        "--keep",
        "1",
    ]));
    assert!(stdout.contains("0 copied"), "{stdout}");
    assert!(stdout.contains("pruned "), "{stdout}");
    let newest = backups(out2.path()).pop().unwrap();
    assert_eq!(backups(out2.path()).len(), 1);

    // Restore into a new dir and compare with the live one (nothing was written since).
    let (_g1, restored) = fresh();
    let src = out2.path().join(&newest);
    let stdout = ok(bin(&[
        "restore",
        "--from",
        src.to_str().unwrap(),
        "--data-dir",
        restored.to_str().unwrap(),
    ]));
    assert!(stdout.contains("restored 8 files"), "{stdout}");
    // A second restore into the same target is refused.
    assert!(!bin(&[
        "restore",
        "--from",
        src.to_str().unwrap(),
        "--data-dir",
        restored.to_str().unwrap(),
    ])
    .status
    .success());

    rt.block_on(async {
        let live = db::open_read_only(data.path()).await.unwrap();
        let copy = db::open_read_only(&restored).await.unwrap();
        let (a, b) = (dump_all(&live).await, dump_all(&copy).await);
        assert!(a.contains("files: 4 rows"), "{a}");
        assert_eq!(a, b, "the restored database has the live rows");
        live.close().await;
        copy.close().await;
    });

    // The restored dir starts a server that accepts the old token (tokens live in the database).
    let (_second, port2) = start_serve(&restored, &[]);
    assert!(post_file(port2, &token, b"after the restore").starts_with("HTTP/1.1 201"));
    drop(_second);

    // Restore, reparse: the derived tables are the same as before, and as the live ones.
    let (_g2, reparsed) = fresh();
    ok(bin(&[
        "restore",
        "--from",
        src.to_str().unwrap(),
        "--data-dir",
        reparsed.to_str().unwrap(),
    ]));
    let stdout = ok(bin(&["reparse", "--data-dir", reparsed.to_str().unwrap()]));
    assert!(stdout.contains("reparsed: 4 files"), "{stdout}");
    drop(server);
    ok(bin(&[
        "reparse",
        "--data-dir",
        data.path().to_str().unwrap(),
    ]));
    rt.block_on(async {
        let live = db::open_read_only(data.path()).await.unwrap();
        let copy = db::open_read_only(&reparsed).await.unwrap();
        let (a, b) = (dump_derived(&live).await, dump_derived(&copy).await);
        assert!(a.contains("fit_messages"), "needs a working decoder:\n{a}");
        assert_eq!(a, b);
        live.close().await;
        copy.close().await;
    });
}

#[test]
fn serve_refuses_bad_backup_options_and_backup_needs_its_arguments() {
    let data = tempfile::tempdir().unwrap();
    let d = data.path().to_str().unwrap();
    for args in [
        vec!["serve", "--backup-every", "6", "--data-dir", d],
        vec!["serve", "--backup-every", "0h", "--data-dir", d],
        vec![
            "serve",
            "--backup-dir",
            "/tmp/x",
            "--backup-every",
            "10s",
            "--data-dir",
            d,
        ],
        vec![
            "serve",
            "--backup-dir",
            "/tmp/x",
            "--backup-keep",
            "0",
            "--data-dir",
            d,
        ],
        vec!["backup", "--data-dir", d],
        vec!["restore", "--data-dir", d],
    ] {
        let out = bin(&args);
        assert!(!out.status.success(), "{args:?} must fail");
    }
    // `backup` of a dir with no database says so and creates nothing.
    let empty = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let out_dir = target.path().join("out");
    let out = bin(&[
        "backup",
        "--data-dir",
        empty.path().to_str().unwrap(),
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(!out_dir.exists());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no database"));
}
