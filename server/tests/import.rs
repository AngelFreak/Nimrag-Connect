//! `import-export` driven through the real binary on synthetic archives.
mod common;

use axum::http::StatusCode;
use std::io::{Cursor, Write};
use std::path::Path;
use std::process::{Command, Output};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in entries {
        w.start_file(*name, opts).unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn run_import(data: &Path, input: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .arg("import-export")
        .args(extra)
        .arg("--data-dir")
        .arg(data)
        .arg(input)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Garmin-export-shaped archive: nested zip of FIT/TCX/GPX plus JSON and a loose FIT.
fn export_zip() -> Vec<u8> {
    let (a, b, c) = (
        common::tiny_fit(1),
        common::tiny_fit(2),
        common::tiny_fit(3),
    );
    let inner = zip_of(&[
        ("a.fit", &a),
        ("sub/b.fit", &b),
        ("ride.tcx", b"<xml/>"),
        ("route.gpx", b"<gpx/>"),
        ("fake.fit", b"not really fit"),
    ]);
    zip_of(&[
        (
            "DI_CONNECT/DI-Connect-Uploaded-Files/UploadedFiles_0.zip",
            &inner,
        ),
        ("DI_CONNECT/DI-Connect-Wellness/2024_sleep.json", b"{}"),
        ("DI_CONNECT/loose/c.bin", &c), // FIT by bytes, not by name
        ("README", b"hi"),
    ])
}

async fn count(pool: &sqlx::SqlitePool, table: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn nested_zip_import_then_rerun_changes_nothing() {
    let (data, src) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let archive = src.path().join("export.zip");
    std::fs::write(&archive, export_zip()).unwrap();

    let out = run_import(data.path(), &archive, &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let s = stdout(&out);
    assert!(s.contains("FIT imported new:     3"), "{s}");
    assert!(s.contains("FIT already present:  0"), "{s}");
    // non-FIT: tcx, gpx, json, fake.fit (bad bytes), README (no ext)
    assert!(s.contains("non-FIT (not stored): 5"), "{s}");
    for ext in [".tcx: 1", ".gpx: 1", ".json: 1", ".fit: 1", ".(none): 1"] {
        assert!(s.contains(ext), "missing {ext}:\n{s}");
    }

    let pool = nimrag_server::db::open(data.path()).await.unwrap();
    assert_eq!(count(&pool, "files").await, 3, "non-FIT must not be stored");
    assert_eq!(
        nimrag_server::store::list_sidecars(data.path())
            .unwrap()
            .len(),
        3
    );
    let sources: Vec<String> = sqlx::query_scalar("SELECT DISTINCT source FROM files")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(sources, ["import"]);
    let sha = nimrag_server::sha256_hex(&common::tiny_fit(2));
    let sc = nimrag_server::ledger::read_sidecar(data.path(), &sha)
        .unwrap()
        .unwrap();
    assert_eq!(
        sc.uploads[0].origin.as_deref(),
        Some("DI_CONNECT/DI-Connect-Uploaded-Files/UploadedFiles_0.zip/sub/b.fit")
    );
    assert!(common::dump_derived(&pool).await.contains("fit_messages"));

    let files_before = common::dump_derived(&pool).await;
    let ledger_before = count(&pool, "uploads").await;
    let sidecar_before =
        std::fs::read_to_string(nimrag_server::store::sidecar_path_for(data.path(), &sha)).unwrap();

    let out = run_import(data.path(), &archive, &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let s = stdout(&out);
    assert!(s.contains("FIT imported new:     0"), "{s}");
    assert!(s.contains("FIT already present:  3"), "{s}");
    assert_eq!(count(&pool, "files").await, 3);
    assert_eq!(count(&pool, "uploads").await, ledger_before);
    assert_eq!(common::dump_derived(&pool).await, files_before);
    let sc = nimrag_server::ledger::read_sidecar(data.path(), &sha)
        .unwrap()
        .unwrap();
    assert_eq!((sc.uploads.len(), sc.uploads[0].repeat_count), (1, 1));
    assert_ne!(
        std::fs::read_to_string(nimrag_server::store::sidecar_path_for(data.path(), &sha)).unwrap(),
        sidecar_before,
        "only the repeat counter moves"
    );
    let rc: i64 = sqlx::query_scalar("SELECT repeat_count FROM uploads WHERE sha256 = ?")
        .bind(&sha)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rc, 1);
}

#[tokio::test]
async fn same_bytes_at_two_paths_rerun_is_still_idempotent() {
    let (data, src) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let f = common::tiny_fit(9);
    let archive = src.path().join("e.zip");
    std::fs::write(&archive, zip_of(&[("one.fit", &f), ("two.fit", &f)])).unwrap();
    assert!(run_import(data.path(), &archive, &[]).status.success());
    let pool = nimrag_server::db::open(data.path()).await.unwrap();
    assert_eq!(count(&pool, "uploads").await, 2);
    let out = run_import(data.path(), &archive, &[]);
    assert!(out.status.success());
    assert_eq!(count(&pool, "uploads").await, 2);
    assert_eq!(count(&pool, "files").await, 1);
}

#[tokio::test]
async fn extracted_directory_input() {
    let (data, src) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let d = src.path().join("DI_CONNECT/DI-Connect-Uploaded-Files");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join("UploadedFiles_0.zip"),
        zip_of(&[("x.fit", &common::tiny_fit(5))]),
    )
    .unwrap();
    std::fs::write(d.join("loose.fit"), common::tiny_fit(6)).unwrap();
    std::fs::write(d.join("notes.json"), b"{}").unwrap();

    let out = run_import(data.path(), src.path(), &["--list-skipped"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let s = stdout(&out);
    assert!(s.contains("FIT imported new:     2"), "{s}");
    assert!(
        s.contains("DI_CONNECT/DI-Connect-Uploaded-Files/notes.json"),
        "{s}"
    );
    let pool = nimrag_server::db::open(data.path()).await.unwrap();
    assert_eq!(count(&pool, "files").await, 2);
}

#[tokio::test]
async fn unsafe_names_are_imported_with_the_raw_name_and_oversized_is_rejected() {
    let (data, src) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut big = common::tiny_fit(7);
    big.resize(65 * 1024 * 1024, 0); // compresses to almost nothing
    let archive = src.path().join("evil.zip");
    std::fs::write(
        &archive,
        zip_of(&[
            ("../evil.fit", &common::tiny_fit(100)),
            ("/abs/evil2.fit", &common::tiny_fit(101)),
            ("huge.fit", &big),
            ("good.fit", &common::tiny_fit(8)),
        ]),
    )
    .unwrap();

    let out = run_import(data.path(), &archive, &[]);
    assert!(!out.status.success(), "errors must give a non-zero exit");
    let (s, e) = (stdout(&out), stderr(&out));
    assert!(s.contains("FIT imported new:     3"), "{s}\n{e}");
    assert!(
        s.contains("unsafe names (imported, name kept as metadata only): 2"),
        "{s}"
    );
    assert!(s.contains("errors:               1"), "{s}");
    assert!(s.contains("entries scanned:      4"), "{s}");
    assert!(e.contains("huge.fit"), "{e}");
    let pool = nimrag_server::db::open(data.path()).await.unwrap();
    assert_eq!(count(&pool, "files").await, 3);
    let origins: Vec<Option<String>> =
        sqlx::query_scalar("SELECT origin FROM uploads ORDER BY origin")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(origins.contains(&Some("../evil.fit".into())));
    assert!(origins.contains(&Some("/abs/evil2.fit".into())));
    // Names never touch the filesystem.
    assert!(!src.path().join("evil.fit").exists());
    assert!(!data.path().join("evil.fit").exists());
}

fn run_cmd(data: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .args(args)
        .arg("--data-dir")
        .arg(data)
        .output()
        .unwrap()
}

/// Patch the uncompressed-size fields of the only entry in `zip` (local and central header).
fn understate_size(zip: &mut [u8], to: u32) {
    let local = zip.windows(4).position(|w| w == b"PK\x03\x04").unwrap();
    zip[local + 22..local + 26].copy_from_slice(&to.to_le_bytes());
    let central = zip.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
    zip[central + 24..central + 28].copy_from_slice(&to.to_le_bytes());
}

#[tokio::test]
async fn entry_that_understates_its_size_is_stopped_by_the_bytes_read_cap() {
    let (data, src) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut big = common::tiny_fit(7);
    big.resize(65 * 1024 * 1024, 0);
    let mut z = zip_of(&[("lie.fit", &big)]);
    understate_size(&mut z, 100);
    let archive = src.path().join("lie.zip");
    std::fs::write(&archive, z).unwrap();
    let out = run_import(data.path(), &archive, &[]);
    assert!(!out.status.success());
    let e = stderr(&out);
    // The zip crate itself refuses a lying header; our bytes-read cap is the backstop.
    assert!(e.contains("lie.fit"), "{e}");
    let pool = nimrag_server::db::open(data.path()).await.unwrap();
    assert_eq!(count(&pool, "files").await, 0);
}

/// `depth` zips inside one another, with `leaf` in the innermost.
fn nested(depth: u32, leaf: &[u8]) -> Vec<u8> {
    let mut z = zip_of(&[("leaf.fit", leaf)]);
    for i in 0..depth {
        z = zip_of(&[(&format!("level{i}.zip"), &z)]);
    }
    z
}

#[tokio::test]
async fn depth_limit_is_an_error_but_other_entries_import() {
    let (data, src) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    // The nearest buildable thing to a zip that contains itself: nesting far past the limit.
    let deep = nested(8, &common::tiny_fit(50));
    let archive = src.path().join("deep.zip");
    std::fs::write(
        &archive,
        zip_of(&[("deep.zip", &deep), ("ok.fit", &common::tiny_fit(51))]),
    )
    .unwrap();
    let out = run_import(data.path(), &archive, &[]);
    assert!(!out.status.success());
    let (s, e) = (stdout(&out), stderr(&out));
    assert!(s.contains("FIT imported new:     1"), "{s}");
    assert!(e.contains("depth limit"), "{e}");
    // Within the limit the leaf is reached.
    let ok = src.path().join("ok.zip");
    std::fs::write(&ok, nested(2, &common::tiny_fit(52))).unwrap();
    assert!(run_import(data.path(), &ok, &[]).status.success());
}

#[tokio::test]
async fn total_bytes_and_entry_count_limits_abort_with_nonzero_exit() {
    let src = tempfile::tempdir().unwrap();
    let archive = src.path().join("export.zip");
    std::fs::write(&archive, export_zip()).unwrap();
    for (flag, val) in [("--max-total-bytes", "100"), ("--max-entries", "2")] {
        let data = tempfile::tempdir().unwrap();
        let out = run_import(data.path(), &archive, &[flag, val]);
        assert!(!out.status.success(), "{flag}");
        assert!(
            stderr(&out).contains("import aborted"),
            "{flag}: {}",
            stderr(&out)
        );
    }
}

#[tokio::test]
async fn limits_via_library_run() {
    use nimrag_server::{api, import};
    let (data, src) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let archive = src.path().join("export.zip");
    std::fs::write(&archive, export_zip()).unwrap();
    let pool = nimrag_server::db::open(data.path()).await.unwrap();
    let state = api::AppState::new(pool, data.path().to_path_buf(), 1 << 20);
    let limits = import::Limits {
        max_entries: 3,
        ..Default::default()
    };
    let r = import::run(&state, &archive, limits, None).await.unwrap();
    assert!(
        r.errors.iter().any(|e| e.contains("entry count")),
        "{:?}",
        r.errors
    );
}

#[tokio::test]
async fn corrupt_entry_errors_and_the_rest_imports() {
    let (data, src) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, bytes) in [
        ("a.fit", common::tiny_fit(60)),
        ("bad.fit", common::tiny_fit(61)),
        ("c.fit", common::tiny_fit(62)),
    ] {
        w.start_file(name, stored).unwrap();
        w.write_all(&bytes).unwrap();
    }
    let mut z = w.finish().unwrap().into_inner();
    let at =
        z.windows(7).position(|w| w == b"bad.fit").unwrap() + 7 + common::tiny_fit(61).len() - 3;
    z[at] ^= 0xff; // payload byte: the stored CRC no longer matches
    let archive = src.path().join("corrupt.zip");
    std::fs::write(&archive, z).unwrap();
    let out = run_import(data.path(), &archive, &[]);
    assert!(!out.status.success());
    let (s, e) = (stdout(&out), stderr(&out));
    assert!(s.contains("FIT imported new:     2"), "{s}\n{e}");
    assert!(e.contains("bad.fit"), "{e}");
}

#[tokio::test]
async fn http_upload_between_imports_keeps_event_order_and_folds_on_event_zero() {
    let e = common::env().await;
    let src = tempfile::tempdir().unwrap();
    let f = common::tiny_fit(70);
    let archive = src.path().join("a.zip");
    std::fs::write(&archive, zip_of(&[("x.fit", &f)])).unwrap();
    assert!(run_import(e.dir.path(), &archive, &[]).status.success());
    let (s, _) = e.upload("", &f).await;
    assert_eq!(s, StatusCode::OK, "second event, not a new file");
    assert!(run_import(e.dir.path(), &archive, &[]).status.success());

    let rows: Vec<(Option<String>, i64)> =
        sqlx::query_as("SELECT origin, repeat_count FROM uploads ORDER BY id")
            .fetch_all(&e.pool)
            .await
            .unwrap();
    assert_eq!(rows, [(Some("x.fit".into()), 1), (None, 0)]);
    let sc = nimrag_server::ledger::read_sidecar(e.dir.path(), &nimrag_server::sha256_hex(&f))
        .unwrap()
        .unwrap();
    assert_eq!(
        sc.uploads
            .iter()
            .map(|u| u.repeat_count)
            .collect::<Vec<_>>(),
        [1, 0]
    );
}

const UPLOADS_Q: &str = "SELECT sha256, device_id, device_name, at, source, created, repeat_count, last_at, origin FROM uploads ORDER BY sha256, at, origin";

async fn uploads_dump(pool: &sqlx::SqlitePool) -> Vec<String> {
    use sqlx::Row;
    sqlx::query(UPLOADS_Q)
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(|r| {
            (0..9)
                .map(|i| {
                    format!(
                        "{:?}",
                        r.try_get::<Option<String>, _>(i)
                            .or_else(|_| r
                                .try_get::<Option<i64>, _>(i)
                                .map(|v| v.map(|n| n.to_string())))
                            .unwrap()
                    )
                })
                .collect::<Vec<_>>()
                .join("|")
        })
        .collect()
}

fn read_sidecars(dir: &Path) -> Vec<Vec<u8>> {
    nimrag_server::store::list_sidecars(dir)
        .unwrap()
        .iter()
        .map(|s| std::fs::read(nimrag_server::store::sidecar_path_for(dir, s)).unwrap())
        .collect()
}

#[tokio::test]
async fn rebuild_ledger_after_import_reproduces_rows_and_leaves_sidecars() {
    let (data, src) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let archive = src.path().join("export.zip");
    std::fs::write(&archive, export_zip()).unwrap();
    assert!(run_import(data.path(), &archive, &[]).status.success());
    let pool = nimrag_server::db::open(data.path()).await.unwrap();
    let (rows, sidecars) = (uploads_dump(&pool).await, read_sidecars(data.path()));
    assert_eq!(rows.len(), 3);
    assert!(sidecars
        .iter()
        .all(|b| String::from_utf8_lossy(b).contains("\"origin\"")));
    let out = run_cmd(data.path(), &["rebuild-ledger"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(uploads_dump(&pool).await, rows);
    assert_eq!(read_sidecars(data.path()), sidecars);
}

#[tokio::test]
async fn rerun_repairs_a_sidecar_whose_database_rows_are_missing() {
    let (data, src) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let archive = src.path().join("export.zip");
    std::fs::write(&archive, export_zip()).unwrap();
    assert!(run_import(data.path(), &archive, &[]).status.success());
    let pool = nimrag_server::db::open(data.path()).await.unwrap();
    let derived = common::dump_derived(&pool).await;
    for q in [
        "DELETE FROM uploads",
        "DELETE FROM fit_messages",
        "DELETE FROM fit_files",
        "DELETE FROM file_parse",
    ] {
        sqlx::query(q).execute(&pool).await.unwrap();
    }
    assert!(run_import(data.path(), &archive, &[]).status.success());
    // Rows are rebuilt from the sidecars (now with the repeat counted), once per file.
    assert_eq!(count(&pool, "uploads").await, 3);
    let rc: Vec<i64> = sqlx::query_scalar("SELECT repeat_count FROM uploads")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(rc, [1, 1, 1]);
    assert_eq!(common::dump_derived(&pool).await, derived);
}

#[tokio::test]
async fn import_equals_http_upload_of_the_same_files() {
    let (data, src) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let archive = src.path().join("export.zip");
    std::fs::write(&archive, export_zip()).unwrap();
    assert!(run_import(data.path(), &archive, &[]).status.success());
    let imported = nimrag_server::db::open(data.path()).await.unwrap();

    let e = common::env().await;
    for t in 1..=3 {
        let (s, _) = e.upload("", &common::tiny_fit(t)).await;
        assert_eq!(s, StatusCode::CREATED);
    }
    let want = common::dump_derived(&e.pool).await;
    assert!(want.contains("fit_messages"), "needs a working decoder");
    assert_eq!(common::dump_derived(&imported).await, want);
    // Same `files` rows too (received_at differs, so compare the hashes and metadata).
    let q = "SELECT sha256, size, source, data_type FROM files ORDER BY sha256";
    let a: Vec<(String, i64, String, Option<i64>)> =
        sqlx::query_as(q).fetch_all(&imported).await.unwrap();
    let b: Vec<(String, i64, String, Option<i64>)> =
        sqlx::query_as(q).fetch_all(&e.pool).await.unwrap();
    assert_eq!(a, b);
}
