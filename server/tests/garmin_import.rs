//! `POST /api/import/garmin` ("Import from Garmin" in the web app) through the real router, on
//! SYNTHETIC archives built here from `FitBuilder` and `tiny_fit`: a single activity's zip, the
//! nested layout of Garmin's account export, a plain `.fit`, and what is refused.
mod common;

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use common::*;
use http_body_util::BodyExt;
use nimrag_core::derive::test_support::{FitBuilder, Fv};
use nimrag_core::fit::profile;
use nimrag_server::users::Role;
use serde_json::{json, Value};
use std::io::{Cursor, Write};
use std::path::Path;
use std::process::Command;
use tower::ServiceExt;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// FIT time 0 in unix seconds.
const FIT_EPOCH: i64 = 631_065_600;

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in entries {
        w.start_file(*name, opts).unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap().into_inner()
}

/// A run starting `offset_s` after 17:30 local on 2026-03-11, as a watch writes it.
fn fit_run(offset_s: i64, serial: u32) -> Vec<u8> {
    let t = (cph(2026, 3, 11, 17, 30) + offset_s - FIT_EPOCH) as u32;
    FitBuilder::new()
        .message(0, &[(0, Fv::U8(4)), (3, Fv::U32(serial)), (4, Fv::U32(t))])
        .message(20, &[(253, Fv::U32(t)), (3, Fv::U8(120)), (5, Fv::U32(0))])
        .message(
            18,
            &[
                (253, Fv::U32(t + 1800)),
                (2, Fv::U32(t)),
                (5, Fv::U8(1)),
                (7, Fv::U32(1_800_000)),
                (9, Fv::U32(500_000)),
            ],
        )
        .message(
            34,
            &[
                (253, Fv::U32(t + 1800)),
                (5, Fv::U32(t + 5400)),
                (1, Fv::U16(1)),
            ],
        )
        .build()
}

/// `POST /api/import/garmin` with whatever the test wants to get wrong.
async fn post_raw(
    e: &Env,
    cookie: Option<&str>,
    csrf: bool,
    uri: &str,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(Method::POST).uri(uri);
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    if csrf {
        req = req.header("x-nimrag", "1");
    }
    let resp = e
        .app
        .clone()
        .oneshot(req.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn import(e: &Env, cookie: &str, name: &str, body: Vec<u8>) -> Value {
    let (s, j) = post_raw(
        e,
        Some(cookie),
        true,
        &format!("/api/import/garmin?name={name}"),
        body,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    j
}

async fn count(e: &Env, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(&e.pool)
        .await
        .unwrap()
}

/// Nothing may stay behind in the incoming directory (uploads, spooled nested zips).
fn assert_no_leftovers(e: &Env) {
    let incoming = e.dir.path().join("files").join(".incoming");
    let left: Vec<_> = std::fs::read_dir(&incoming)
        .map(|d| d.flatten().map(|f| f.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "left in .incoming: {left:?}");
}

/// A Garmin account export in the shape `DI_CONNECT/DI-Connect-Uploaded-Files/UploadedFiles_*.zip`.
fn full_export() -> Vec<u8> {
    let uploaded_a = zip_of(&[
        ("a_1.fit", &common::tiny_fit(1)),
        ("a_2.fit", &common::tiny_fit(2)),
        ("notes.txt", b"hello"),
        ("route.gpx", b"<gpx/>"),
    ]);
    let uploaded_b = zip_of(&[
        ("sub/b_1.fit", &common::tiny_fit(3)),
        ("fake.fit", b"this is not a FIT file at all"),
        // FIT by its bytes, whatever the name says.
        ("renamed.bin", &common::tiny_fit(4)),
    ]);
    // A zip in a zip in a zip: any depth within the limit is walked.
    let deep = zip_of(&[("level2.zip", &zip_of(&[("deep.fit", &common::tiny_fit(5))]))]);
    zip_of(&[
        (
            "DI_CONNECT/DI-Connect-Uploaded-Files/UploadedFiles_0-_Part1.zip",
            &uploaded_a,
        ),
        (
            "DI_CONNECT/DI-Connect-Uploaded-Files/UploadedFiles_0-_Part2.zip",
            &uploaded_b,
        ),
        ("DI_CONNECT/DI-Connect-Uploaded-Files/deep.zip", &deep),
        (
            "DI_CONNECT/DI-Connect-Fitness/summarizedActivities.json",
            b"[]",
        ),
        ("README.txt", b"Your data"),
    ])
}

#[tokio::test]
async fn a_single_activity_zip_is_ingested_as_the_users_own_file() {
    let e = env().await;
    let cookie = e.login().await;
    let run = fit_run(0, 7);
    let j = import(
        &e,
        &cookie,
        "12345678901.zip",
        zip_of(&[("12345678901_ACTIVITY.fit", &run)]),
    )
    .await;
    assert_eq!(
        j,
        json!({
            "name": "12345678901.zip", "fit_found": 1, "new": 1, "already_present": 0,
            "skipped": 0, "failed": 0, "errors": [],
            "kinds": [{"kind": if profile::available() { "activity" } else { "other" }, "count": 1}],
        })
    );
    let sha = nimrag_server::sha256_hex(&run);
    // Stored untouched, owned by the signed-in user, recorded as an import with its archive path.
    assert_eq!(
        std::fs::read(common::stored_path(e.dir.path(), &sha)).unwrap(),
        run
    );
    let owner: i64 = sqlx::query_scalar("SELECT user_id FROM file_owners WHERE sha256 = ?")
        .bind(&sha)
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(owner, e.user_id);
    let sc = nimrag_server::ledger::read_sidecar(e.dir.path(), &sha)
        .unwrap()
        .unwrap();
    assert_eq!(sc.meta.source, "import");
    assert_eq!(
        sc.uploads[0].origin.as_deref(),
        Some("12345678901.zip/12345678901_ACTIVITY.fit")
    );
    assert_no_leftovers(&e);
    if profile::available() {
        let list = e.get(&cookie, "/api/activities").await;
        assert_eq!(list["items"].as_array().unwrap().len(), 1, "{list}");
    }
}

#[tokio::test]
async fn the_nested_layout_of_a_full_export_is_walked_and_only_fit_is_kept() {
    let e = env().await;
    let cookie = e.login().await;
    let j = import(&e, &cookie, "export.zip", full_export()).await;
    // a_1, a_2, b_1, renamed.bin, deep.fit
    assert_eq!(j["fit_found"], 5, "{j}");
    assert_eq!(j["new"], 5);
    assert_eq!(j["already_present"], 0);
    // notes.txt, route.gpx, fake.fit (wrong bytes), summarizedActivities.json, README.txt
    assert_eq!(j["skipped"], 5, "{j}");
    assert_eq!(j["failed"], 0);
    assert_eq!(j["errors"], json!([]));
    assert_eq!(j["kinds"].as_array().unwrap().len(), 1);
    assert_eq!(j["kinds"][0]["count"], 5);
    // Only the FIT files were stored, all of them the user's, all recorded as imports.
    assert_eq!(count(&e, "SELECT COUNT(*) FROM files").await, 5);
    assert_eq!(
        count(
            &e,
            &format!(
                "SELECT COUNT(*) FROM file_owners WHERE user_id = {}",
                e.user_id
            )
        )
        .await,
        5
    );
    assert_eq!(
        count(&e, "SELECT COUNT(*) FROM files WHERE source <> 'import'").await,
        0
    );
    let sha = nimrag_server::sha256_hex(&common::tiny_fit(5));
    let sc = nimrag_server::ledger::read_sidecar(e.dir.path(), &sha)
        .unwrap()
        .unwrap();
    assert_eq!(
        sc.uploads[0].origin.as_deref(),
        Some("export.zip/DI_CONNECT/DI-Connect-Uploaded-Files/deep.zip/level2.zip/deep.fit")
    );
    // The nested zips were spooled to disk and removed again.
    assert_no_leftovers(&e);
}

#[tokio::test]
async fn a_plain_fit_file_and_a_plain_non_fit_file() {
    let e = env().await;
    let cookie = e.login().await;
    let f = common::tiny_fit(11);
    let j = import(&e, &cookie, "2026-03-11-17-30-00.fit", f.clone()).await;
    assert_eq!(
        (j["fit_found"].clone(), j["new"].clone()),
        (json!(1), json!(1))
    );
    let sc = nimrag_server::ledger::read_sidecar(e.dir.path(), &nimrag_server::sha256_hex(&f))
        .unwrap()
        .unwrap();
    assert_eq!(
        sc.uploads[0].origin.as_deref(),
        Some("2026-03-11-17-30-00.fit")
    );

    // Not a zip, not FIT: nothing stored, nothing failed.
    let j = import(&e, &cookie, "notes.txt", b"just some text".to_vec()).await;
    assert_eq!(
        (
            j["fit_found"].clone(),
            j["skipped"].clone(),
            j["failed"].clone()
        ),
        (json!(0), json!(1), json!(0)),
        "{j}"
    );
    assert_eq!(count(&e, "SELECT COUNT(*) FROM files").await, 1);

    // A directory part in the name is dropped.
    let j = import(&e, &cookie, "..%2F..%2Fother.fit", common::tiny_fit(12)).await;
    assert_eq!(j["name"], "other.fit");
    assert_no_leftovers(&e);
}

#[tokio::test]
async fn importing_the_same_export_again_changes_nothing() {
    let e = env().await;
    let cookie = e.login().await;
    let export = full_export();
    import(&e, &cookie, "export.zip", export.clone()).await;
    let ledger = dump_ledger(&e.pool).await;
    let derived = dump_derived(&e.pool).await;
    let events = count(&e, "SELECT COUNT(*) FROM uploads").await;

    let j = import(&e, &cookie, "export.zip", export.clone()).await;
    assert_eq!(
        (j["new"].clone(), j["already_present"].clone()),
        (json!(0), json!(5))
    );
    assert_eq!(j["fit_found"], 5);
    assert_eq!(count(&e, "SELECT COUNT(*) FROM files").await, 5);
    assert_eq!(count(&e, "SELECT COUNT(*) FROM uploads").await, events);
    assert_eq!(dump_derived(&e.pool).await, derived);
    // Same origins, so only the repeat counters moved.
    let after = dump_ledger(&e.pool).await;
    assert_ne!(after, ledger);
    assert_eq!(
        count(&e, "SELECT SUM(repeat_count) FROM uploads").await,
        5,
        "one repeat per file"
    );

    // Under another name the same bytes are still not stored twice.
    let j = import(&e, &cookie, "export%20(1).zip", export).await;
    assert_eq!(
        (j["new"].clone(), j["already_present"].clone()),
        (json!(0), json!(5))
    );
    assert_eq!(count(&e, "SELECT COUNT(*) FROM files").await, 5);
    assert_no_leftovers(&e);
}

#[tokio::test]
async fn a_file_the_watch_already_uploaded_is_already_present() {
    let e = env().await;
    let cookie = e.login().await;
    let run = fit_run(0, 7);
    let (s, _) = e.upload("source=watch&data_type=4", &run).await;
    assert_eq!(s, StatusCode::CREATED);
    let j = import(&e, &cookie, "1.zip", zip_of(&[("1.fit", &run)])).await;
    assert_eq!(
        (j["new"].clone(), j["already_present"].clone()),
        (json!(0), json!(1))
    );
    assert_eq!(count(&e, "SELECT COUNT(*) FROM files").await, 1);
}

#[tokio::test]
async fn a_broken_archive_and_broken_entries_are_counted_and_the_rest_imports() {
    let e = env().await;
    let cookie = e.login().await;

    // Starts like a zip, is not one.
    let mut junk = b"PK\x03\x04".to_vec();
    junk.extend_from_slice(&[0x13; 200]);
    let j = import(&e, &cookie, "broken.zip", junk).await;
    assert_eq!(
        (j["fit_found"].clone(), j["failed"].clone()),
        (json!(0), json!(1)),
        "{j}"
    );
    let text = j["errors"][0].as_str().unwrap();
    assert!(text.starts_with("broken.zip"), "{text}");
    assert!(
        !text.contains(e.dir.path().to_str().unwrap()),
        "no server paths: {text}"
    );

    // 25 entries whose bytes no longer match their checksum, and one good file.
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let bad: Vec<Vec<u8>> = (0..25).map(|i| common::tiny_fit(100 + i)).collect();
    for (i, b) in bad.iter().enumerate() {
        w.start_file(format!("bad{i}.fit"), stored).unwrap();
        w.write_all(b).unwrap();
    }
    w.start_file("good.fit", stored).unwrap();
    w.write_all(&common::tiny_fit(200)).unwrap();
    let mut z = w.finish().unwrap().into_inner();
    for b in &bad {
        let at = z.windows(b.len()).position(|w| w == b.as_slice()).unwrap();
        z[at + b.len() - 3] ^= 0xff;
    }
    let j = import(&e, &cookie, "corrupt.zip", z).await;
    assert_eq!(j["new"], 1, "{j}");
    assert_eq!(j["failed"], 25, "{j}");
    assert_eq!(j["errors"].as_array().unwrap().len(), 20, "capped");
    assert_no_leftovers(&e);
}

#[tokio::test]
async fn only_a_signed_in_user_with_data_and_the_csrf_header_may_import() {
    let e = env().await;
    let uri = "/api/import/garmin?name=a.fit";
    let body = common::tiny_fit(21);

    let (s, _) = post_raw(&e, None, true, uri, body.clone()).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "signed out");

    // A device token is not a session.
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("x-nimrag", "1")
        .header(header::AUTHORIZATION, format!("Bearer {}", e.token))
        .body(Body::from(body.clone()))
        .unwrap();
    let resp = e.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "device token");

    let cookie = e.login().await;
    let (s, _) = post_raw(&e, Some(&cookie), false, uri, body.clone()).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "no X-Nimrag header");

    e.add_user("boss", Role::Admin).await;
    let admin = e.login_as("boss").await;
    let (s, _) = post_raw(&e, Some(&admin), true, uri, body.clone()).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "an admin has no data");

    let (s, j) = post_raw(&e, Some(&cookie), true, "/api/import/garmin", body.clone()).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "name is required: {j}");
    let (s, _) = post_raw(&e, Some(&cookie), true, "/api/import/garmin?name=%2F", body).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "a name with nothing left");

    // Nothing was stored by any of those.
    assert_eq!(count(&e, "SELECT COUNT(*) FROM files").await, 0);
    assert_no_leftovers(&e);
}

#[tokio::test]
async fn each_user_gets_their_own_files() {
    let e = env().await;
    e.adopt.store(false, std::sync::atomic::Ordering::Relaxed);
    let other = e.add_user("second", Role::User).await;
    let (a, b) = (e.login().await, e.login_as("second").await);
    import(&e, &a, "a.fit", fit_run(0, 1)).await;
    import(&e, &b, "b.fit", fit_run(7200, 2)).await;
    for (uid, n) in [(e.user_id, 1), (other, 1)] {
        assert_eq!(
            count(
                &e,
                &format!("SELECT COUNT(*) FROM file_owners WHERE user_id = {uid}")
            )
            .await,
            n
        );
    }
    if profile::available() {
        for cookie in [&a, &b] {
            let list = e.get(cookie, "/api/activities").await;
            assert_eq!(list["items"].as_array().unwrap().len(), 1, "{list}");
        }
    }
}

#[tokio::test]
async fn a_body_over_the_limit_is_refused_with_413_and_leaves_nothing() {
    let e = env().await;
    let state = nimrag_server::api::AppState::new(
        e.pool.clone(),
        e.dir.path().to_path_buf(),
        nimrag_server::api::MAX_BODY_BYTES,
    )
    .with_max_import(2000);
    let e = Env {
        app: nimrag_server::api::router(state),
        ..e
    };
    let cookie = e.login().await;
    let uri = "/api/import/garmin?name=big.zip";

    // Streamed past the limit (no Content-Length to go by).
    let mut big = common::tiny_fit(31);
    big.resize(5000, 0);
    let (s, j) = post_raw(&e, Some(&cookie), true, uri, big.clone()).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE, "{j}");
    assert_no_leftovers(&e);

    // Declared too large: refused before a byte is read.
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::COOKIE, &cookie)
        .header("x-nimrag", "1")
        .header(header::CONTENT_LENGTH, "5000")
        .body(Body::from(big))
        .unwrap();
    let resp = e.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);

    // Within the limit it works, so the refusal was the size and nothing else.
    let j = import(&e, &cookie, "small.fit", common::tiny_fit(32)).await;
    assert_eq!(j["new"], 1);
    assert_eq!(count(&e, "SELECT COUNT(*) FROM files").await, 1);
    assert_no_leftovers(&e);
}

// ---- the watch wins over Garmin Connect ----------------------------------------------------

fn write_connect_run(dir: &Path) {
    std::fs::write(
        dir.join("activities.json"),
        serde_json::to_string(&json!({"count": 1, "activities": [{
            "id": 1001, "type": "running", "start_time": "2026-03-11 17:30:00",
            "duration_seconds": 1800, "distance_meters": 5000, "calories": 350,
            "avg_hr_bpm": 150, "max_hr_bpm": 170, "elevation_gain_meters": 20
        }]}))
        .unwrap(),
    )
    .unwrap();
}

fn import_connect(data: &Path, input: &Path) {
    let out = Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .args([
            "import-connect",
            "--user",
            USER,
            "--timezone",
            "Europe/Copenhagen",
        ])
        .arg("--data-dir")
        .arg(data)
        .arg(input)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[tokio::test]
async fn an_imported_activity_hides_the_same_one_from_garmin_connect_in_either_order() {
    if !profile::available() {
        return;
    }
    for connect_first in [true, false] {
        let e = env_at(cph(2026, 3, 20, 12, 0)).await;
        let cookie = e.login().await;
        nimrag_server::settings::set_user(&e.pool, e.user_id, "timezone", "Europe/Copenhagen")
            .await
            .unwrap();
        let input = tempfile::tempdir().unwrap();
        write_connect_run(input.path());
        // Garmin's original of the same run, 45 s off the Connect list's start time.
        let zip = zip_of(&[("1001_ACTIVITY.fit", &fit_run(45, 7))]);
        if connect_first {
            import_connect(e.dir.path(), input.path());
        }
        import(&e, &cookie, "1001.zip", zip).await;
        if !connect_first {
            import_connect(e.dir.path(), input.path());
        }
        let keys: Vec<Option<String>> =
            sqlx::query_scalar("SELECT activity_key FROM activities_current ORDER BY start_ts")
                .fetch_all(&e.pool)
                .await
                .unwrap();
        assert_eq!(keys.len(), 1, "connect_first={connect_first}: {keys:?}");
        assert_ne!(keys[0].as_deref(), Some("connect:1001"), "the file wins");
        // Hidden, not deleted.
        assert_eq!(
            count(
                &e,
                "SELECT COUNT(*) FROM activities WHERE activity_key = 'connect:1001'"
            )
            .await,
            1
        );
        let list = e.get(&cookie, "/api/activities").await;
        assert_eq!(
            list["items"].as_array().unwrap().len(),
            1,
            "no duplicate in the list, connect_first={connect_first}: {list}"
        );
    }
}
