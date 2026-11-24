//! `POST /api/import/nimrag` ("Restore from a Nimrag export") through the real router, on SYNTHETIC
//! data: the zip of `GET /api/export` of one server goes into a user of another, and into the
//! same user again. Also what the restore refuses and what the export's exact part holds.
mod common;

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use common::*;
use http_body_util::BodyExt;
use nimrag_core::derive::test_support::{FitBuilder, Fv};
use nimrag_core::fit::profile;
use nimrag_server::connect::{wrap, Kind};
use nimrag_server::users::Role;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use tower::ServiceExt;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const T0: u32 = 0x3000_0000;
const NOW: i64 = 1_800_000_000;

/// An activity file; `serial` makes its bytes (and so its activity) unique.
fn activity(serial: u32) -> Vec<u8> {
    FitBuilder::new()
        .message(0, &[(3, Fv::U32(serial)), (4, Fv::U32(T0 - 100))])
        .message(20, &[(253, Fv::U32(T0)), (3, Fv::U8(120))])
        .message(
            18,
            &[(253, Fv::U32(T0 + 10)), (2, Fv::U32(T0)), (5, Fv::U8(1))],
        )
        .build()
}

fn sleep_json() -> String {
    json!({"nights": [{
        "date": "2026-03-11",
        "sleep_start": cph(2026, 3, 10, 22, 30) * 1000,
        "sleep_end": cph(2026, 3, 11, 6, 30) * 1000,
        "sleep_score": 80,
        "deep_sleep_seconds": 5400,
        "light_sleep_seconds": 16800,
        "rem_sleep_seconds": 6000,
        "awake_seconds": 600,
        "awake_count": 2,
        "avg_sleep_stress": 18.5,
        "resting_heart_rate_bpm": 51,
    }]})
    .to_string()
}

fn zip_of(entries: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in entries {
        w.start_file(name.as_str(), opts).unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn unzip(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("a valid zip");
    let mut out = BTreeMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut b = vec![];
        f.read_to_end(&mut b).unwrap();
        out.insert(f.name().unwrap().into_owned(), b);
    }
    out
}

fn json_of(z: &BTreeMap<String, Vec<u8>>, name: &str) -> Value {
    serde_json::from_slice(z.get(name).unwrap_or_else(|| panic!("{name} missing"))).unwrap()
}

async fn upload(e: &Env, token: &str, query: &str, body: &[u8]) -> String {
    let (s, j) = e
        .request(
            Method::POST,
            &format!("/api/files?{query}"),
            Some(token),
            body.to_vec(),
        )
        .await;
    assert!(s == StatusCode::CREATED || s == StatusCode::OK, "{s} {j}");
    j["sha256"].as_str().unwrap().to_owned()
}

async fn download(e: &Env, cookie: &str) -> Vec<u8> {
    let resp = e
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/export")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    resp.into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

/// `POST` with whatever the test wants to get wrong.
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

async fn restore_raw(e: &Env, cookie: &str, body: Vec<u8>) -> (StatusCode, Value) {
    post_raw(
        e,
        Some(cookie),
        true,
        "/api/import/nimrag?name=export.zip",
        body,
    )
    .await
}

async fn restore(e: &Env, cookie: &str, body: &[u8]) -> Value {
    let (s, j) = restore_raw(e, cookie, body.to_vec()).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    j
}

async fn rows(e: &Env, sql: &str, uid: i64) -> Vec<String> {
    use sqlx::Row;
    sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
        .bind(uid)
        .fetch_all(&e.pool)
        .await
        .unwrap()
        .iter()
        .map(|r| {
            (0..r.columns().len())
                .map(|i| {
                    r.try_get::<Option<String>, _>(i)
                        .map(|v| format!("{v:?}"))
                        .or_else(|_| r.try_get::<Option<i64>, _>(i).map(|v| format!("{v:?}")))
                        .or_else(|_| r.try_get::<Option<f64>, _>(i).map(|v| format!("{v:?}")))
                        .unwrap()
                })
                .collect::<Vec<_>>()
                .join("|")
        })
        .collect()
}

async fn count(e: &Env, table: &str, uid: i64) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) FROM {table} WHERE user_id = ?"
    )))
    .bind(uid)
    .fetch_one(&e.pool)
    .await
    .unwrap()
}

/// Nothing may stay behind in the incoming directory.
fn assert_no_leftovers(e: &Env) {
    let incoming = e.dir.path().join("files").join(".incoming");
    let left: Vec<_> = std::fs::read_dir(&incoming)
        .map(|d| d.flatten().map(|f| f.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "left in .incoming: {left:?}");
}

struct Source {
    e: Env,
    cookie: String,
    other: i64,
    zip: Vec<u8>,
    shas: Vec<String>,
}

/// User A (the environment's own user) with a watch FIT, a Garmin Connect envelope, an `import`
/// FIT, settings, two workouts, two body entries and a strength edit; and a second user with a
/// file and a workout of their own. The export is A's.
async fn source() -> Source {
    let e = env_at(NOW).await;
    e.adopt.store(false, std::sync::atomic::Ordering::Relaxed);
    let a = e.user_id;
    let other = e.add_user("second", Role::User).await;
    let tok_other = e.token_for(other).await;

    let watch = upload(
        &e,
        &e.token,
        "source=watch&data_type=128&sub_type=4&file_index=3&file_number=7&watch_timestamp=1234",
        &activity(11),
    )
    .await;
    let tz: chrono_tz::Tz = "Europe/Copenhagen".parse().unwrap();
    let connect = upload(
        &e,
        &e.token,
        "source=import",
        &wrap(Kind::Sleep, tz, &sleep_json()),
    )
    .await;
    // The upload endpoint takes no `connect` source; the importer records it so.
    e.exec(&format!(
        "UPDATE files SET source = 'connect' WHERE sha256 = '{connect}'"
    ))
    .await;
    let imported = upload(&e, &e.token, "source=import", &activity(12)).await;
    upload(&e, &tok_other, "source=import", &activity(99)).await;

    let cards = serde_json::to_string(&nimrag_server::settings::HomeCards::default()).unwrap();
    for (key, value) in [
        ("timezone", "Europe/Copenhagen".to_owned()),
        ("step_goal", "9000".to_owned()),
        ("intensity_goal_min", "200".to_owned()),
        ("home_cards", cards),
        ("junk", "not a setting".to_owned()),
    ] {
        sqlx::query("INSERT INTO user_settings (user_id, key, value) VALUES (?, ?, ?)")
            .bind(a)
            .bind(key)
            .bind(value)
            .execute(&e.pool)
            .await
            .unwrap();
    }
    for (uid, name, sport, def, state, rev) in [
        (
            a,
            "Easy run",
            "running",
            r#"{"note":"easy","nodes":[1,2]}"#,
            "sent",
            3,
        ),
        (
            a,
            "Long \"quoted\" run",
            "running",
            r#"{"nodes":[]}"#,
            "queued",
            1,
        ),
        (
            other,
            "Other person's workout",
            "walking",
            r#"{"nodes":[7]}"#,
            "idle",
            1,
        ),
    ] {
        sqlx::query(
            "INSERT INTO workouts (user_id, name, sport, definition, revision, created_at, updated_at, send_state)
             VALUES (?, ?, ?, ?, ?, 1700000000, 1700000500, ?)",
        )
        .bind(uid)
        .bind(name)
        .bind(sport)
        .bind(def)
        .bind(rev)
        .bind(state)
        .execute(&e.pool)
        .await
        .unwrap();
    }
    for (uid, at, weight, fat, waist) in [
        (a, 1_790_000_000, Some(80.25), Some(18.5), None),
        (a, 1_790_100_000, Some(79.75), None, Some(88.5)),
        (other, 1_790_000_000, Some(120.5), None, None),
    ] {
        sqlx::query(
            "INSERT INTO body_measurements (user_id, measured_at, weight_kg, body_fat_pct, waist_cm, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(uid)
        .bind(at)
        .bind(weight)
        .bind(fat)
        .bind(waist)
        .bind(at + 5)
        .execute(&e.pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO strength_set_overrides (user_id, activity_ident, set_idx, fields, updated_at)
         VALUES (?, 'x:1', 2, ?, 1790000300)",
    )
    .bind(a)
    .bind(json!({"reps": 10, "exercise": "Goblet squat"}).to_string())
    .execute(&e.pool)
    .await
    .unwrap();

    let cookie = e.login().await;
    let zip = download(&e, &cookie).await;
    Source {
        e,
        cookie,
        other,
        zip,
        shas: vec![watch, connect, imported],
    }
}

/// A fresh server with its own user (the environment's `tester`), nothing else.
async fn fresh() -> (Env, String) {
    let e = env_at(NOW).await;
    e.adopt.store(false, std::sync::atomic::Ordering::Relaxed);
    let cookie = e.login().await;
    (e, cookie)
}

const FILE_ROWS: &str =
    "SELECT f.sha256, f.size, f.source, f.data_type, f.sub_type, f.file_index, f.file_number,
        f.watch_timestamp FROM files f JOIN file_owners o ON o.sha256 = f.sha256
        WHERE o.user_id = ? ORDER BY f.sha256";

#[tokio::test]
async fn the_export_has_an_exact_part_with_only_the_users_own_rows() {
    let s = source().await;
    let z = unzip(&s.zip);
    let m = json_of(&z, "nimrag/manifest.json");
    assert_eq!(m["format"], "nimrag-export");
    assert_eq!(m["version"], 1);
    assert_eq!(m["timezone"], "Europe/Copenhagen");
    assert_eq!(
        m["counts"],
        json!({"files": 3, "workouts": 2, "body_measurements": 2, "strength_set_edits": 1, "settings": 5})
    );
    assert!(m["schema_version"].as_i64().unwrap() >= 16);

    let files = json_of(&z, "nimrag/files.json");
    let files = files.as_array().unwrap();
    assert_eq!(files.len(), 3);
    let watch = files.iter().find(|f| f["sha256"] == s.shas[0]).unwrap();
    assert_eq!(watch["name"], format!("originals/{}.fit", s.shas[0]));
    assert_eq!(watch["source"], "watch");
    assert_eq!(watch["data_type"], 128);
    assert_eq!(watch["sub_type"], 4);
    assert_eq!(watch["file_index"], 3);
    assert_eq!(watch["file_number"], 7);
    assert_eq!(watch["watch_timestamp"], 1234);
    assert!(watch["received_at"].is_string() && watch["size"].as_i64().unwrap() > 0);
    let connect = files.iter().find(|f| f["sha256"] == s.shas[1]).unwrap();
    assert_eq!(connect["source"], "connect");
    assert_eq!(connect["name"], format!("originals/{}.json", s.shas[1]));

    let settings = json_of(&z, "nimrag/settings.json");
    assert_eq!(settings["step_goal"], "9000");
    let workouts = json_of(&z, "nimrag/workouts.json");
    assert_eq!(workouts[0]["name"], "Easy run");
    assert_eq!(
        workouts[0]["definition"],
        json!({"note": "easy", "nodes": [1, 2]})
    );
    assert_eq!(workouts[0]["created_at"], 1_700_000_000);
    assert_eq!(workouts[0]["updated_at"], 1_700_000_500);
    let body = json_of(&z, "nimrag/body_measurements.json");
    assert_eq!(body[0]["weight_kg"], 80.25);
    assert_eq!(body[0]["waist_cm"], Value::Null);
    let edits = json_of(&z, "nimrag/strength_set_edits.json");
    assert_eq!(edits[0]["activity_ident"], "x:1");
    assert_eq!(
        edits[0]["fields"],
        json!({"reps": 10, "exercise": "Goblet squat"})
    );

    // Nothing of the other user, and nothing secret, in any of it.
    let all: String = z
        .iter()
        .filter(|(k, _)| k.starts_with("nimrag/"))
        .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
        .collect();
    for needle in [
        "Other person",
        "120.5",
        "second",
        "password",
        "totp",
        "token",
    ] {
        assert!(!all.contains(needle), "the exact part holds {needle}");
    }
    let _ = s.other;
    // The README tells how to restore.
    let readme = String::from_utf8(z["README.txt"].clone()).unwrap();
    assert!(readme.contains("nimrag/") && readme.contains("Restore from a Nimrag export"));
}

#[tokio::test]
async fn a_restore_into_another_server_gives_the_same_files_counts_and_entries() {
    let s = source().await;
    let a = s.e.user_id;
    let (e2, cookie2) = fresh().await;
    let b = e2.user_id;

    let r = restore(&e2, &cookie2, &s.zip).await;
    assert_eq!(r["name"], "export.zip");
    assert_eq!(
        r["files"],
        json!({"added": 3, "already_here": 0, "missing_in_export": 0, "refused": 0}),
        "{r}"
    );
    assert_eq!(r["settings_applied"], 4);
    assert_eq!(r["settings_skipped"], 1);
    assert_eq!(r["workouts"], json!({"added": 2, "already_here": 0}));
    assert_eq!(
        r["body_measurements"],
        json!({"added": 2, "already_here": 0})
    );
    assert_eq!(r["strength_set_edits"], json!({"applied": 1, "older": 0}));
    assert_no_leftovers(&e2);

    // The same files with the same ledger details, owned by the restoring user.
    let (fa, fb) = (
        rows(&s.e, FILE_ROWS, a).await,
        rows(&e2, FILE_ROWS, b).await,
    );
    assert_eq!(fa.len(), 3);
    assert_eq!(fa, fb);
    for sha in &s.shas {
        assert_eq!(
            std::fs::read(stored_path(s.e.dir.path(), sha)).unwrap(),
            std::fs::read(stored_path(e2.dir.path(), sha)).unwrap()
        );
    }
    let sc = nimrag_server::ledger::read_sidecar(e2.dir.path(), &s.shas[0])
        .unwrap()
        .unwrap();
    assert_eq!(sc.meta.source, "watch");
    assert_eq!(sc.meta.file_index, Some(3));

    // Derived: the same counts.
    for table in [
        "activities_current",
        "sleep_sessions_current",
        "daily_totals",
        "mon_hr",
        "connect_daily",
        "sleep_levels",
    ] {
        assert_eq!(
            count(&s.e, table, a).await,
            count(&e2, table, b).await,
            "{table}"
        );
    }
    assert!(count(&e2, "sleep_sessions_current", b).await > 0);
    if profile::available() {
        assert_eq!(count(&e2, "activities_current", b).await, 2);
    }

    // The person's own entries, equal; workouts start idle at revision 1.
    let w = "SELECT name, sport, definition, created_at, updated_at FROM workouts WHERE user_id = ? ORDER BY name";
    assert_eq!(rows(&s.e, w, a).await, rows(&e2, w, b).await);
    let states = "SELECT send_state, revision, send_requested_at, send_error FROM workouts WHERE user_id = ?";
    assert_eq!(
        rows(&e2, states, b).await,
        vec!["Some(\"idle\")|Some(1)|None|None"; 2]
    );
    let body = "SELECT measured_at, weight_kg, body_fat_pct, waist_cm, created_at FROM body_measurements WHERE user_id = ? ORDER BY measured_at";
    assert_eq!(rows(&s.e, body, a).await, rows(&e2, body, b).await);
    let edits = "SELECT activity_ident, set_idx, fields, updated_at FROM strength_set_overrides WHERE user_id = ?";
    assert_eq!(rows(&s.e, edits, a).await, rows(&e2, edits, b).await);
    let settings = "SELECT key, value FROM user_settings WHERE user_id = ? ORDER BY key";
    let mut want = rows(&s.e, settings, a).await;
    want.retain(|r| !r.contains("junk"));
    assert_eq!(want.len(), 4);
    assert_eq!(rows(&e2, settings, b).await, want);

    // Again: everything is already here, nothing changes.
    let before = (
        count(&e2, "workouts", b).await,
        count(&e2, "body_measurements", b).await,
    );
    let r = restore(&e2, &cookie2, &s.zip).await;
    assert_eq!(
        r["files"],
        json!({"added": 0, "already_here": 3, "missing_in_export": 0, "refused": 0}),
        "{r}"
    );
    assert_eq!(r["workouts"], json!({"added": 0, "already_here": 2}));
    assert_eq!(
        r["body_measurements"],
        json!({"added": 0, "already_here": 2})
    );
    assert_eq!(r["strength_set_edits"], json!({"applied": 0, "older": 1}));
    assert_eq!(
        before,
        (
            count(&e2, "workouts", b).await,
            count(&e2, "body_measurements", b).await
        )
    );
    assert_eq!(rows(&e2, FILE_ROWS, b).await, fb);
    assert_no_leftovers(&e2);

    // The restored user's own export holds the same exact part again.
    let again = unzip(&download(&e2, &cookie2).await);
    let first = unzip(&s.zip);
    for name in [
        "nimrag/workouts.json",
        "nimrag/body_measurements.json",
        "nimrag/strength_set_edits.json",
    ] {
        assert_eq!(json_of(&again, name), json_of(&first, name), "{name}");
    }
}

#[tokio::test]
async fn a_restore_into_the_same_user_adds_nothing() {
    let s = source().await;
    let a = s.e.user_id;
    let before = rows(&s.e, FILE_ROWS, a).await;
    let r = restore(&s.e, &s.cookie, &s.zip).await;
    assert_eq!(
        r["files"],
        json!({"added": 0, "already_here": 3, "missing_in_export": 0, "refused": 0}),
        "{r}"
    );
    assert_eq!(r["workouts"], json!({"added": 0, "already_here": 2}));
    assert_eq!(
        r["body_measurements"],
        json!({"added": 0, "already_here": 2})
    );
    assert_eq!(r["strength_set_edits"], json!({"applied": 0, "older": 1}));
    assert_eq!(count(&s.e, "workouts", a).await, 2);
    assert_eq!(count(&s.e, "body_measurements", a).await, 2);
    assert_eq!(rows(&s.e, FILE_ROWS, a).await, before);
    // The other user's rows were not touched.
    assert_eq!(count(&s.e, "workouts", s.other).await, 1);
    assert_no_leftovers(&s.e);
}

#[tokio::test]
async fn a_restore_merges_with_what_the_user_has_and_the_later_edit_wins() {
    let s = source().await;
    let (e2, cookie2) = fresh().await;
    let b = e2.user_id;
    // B already has a newer edit of the same set, a body entry of their own and a workout.
    sqlx::query(
        "INSERT INTO strength_set_overrides (user_id, activity_ident, set_idx, fields, updated_at)
         VALUES (?, 'x:1', 2, '{\"reps\":99}', 1790009999)",
    )
    .bind(b)
    .execute(&e2.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO body_measurements (user_id, measured_at, weight_kg, created_at) VALUES (?, 1700000000, 70.5, 1700000000)")
        .bind(b)
        .execute(&e2.pool)
        .await
        .unwrap();
    let r = restore(&e2, &cookie2, &s.zip).await;
    assert_eq!(r["strength_set_edits"], json!({"applied": 0, "older": 1}));
    assert_eq!(r["body_measurements"]["added"], 2);
    assert_eq!(
        count(&e2, "body_measurements", b).await,
        3,
        "nothing of theirs deleted"
    );
    let fields: String =
        sqlx::query_scalar("SELECT fields FROM strength_set_overrides WHERE user_id = ?")
            .bind(b)
            .fetch_one(&e2.pool)
            .await
            .unwrap();
    assert_eq!(fields, "{\"reps\":99}");
}

#[tokio::test]
async fn a_file_whose_bytes_do_not_match_its_hash_is_refused_and_not_stored() {
    let s = source().await;
    let mut z = unzip(&s.zip);
    let victim = format!("originals/{}.fit", s.shas[0]);
    z.get_mut(&victim).unwrap().push(0);
    let (e2, cookie2) = fresh().await;
    let r = restore(&e2, &cookie2, &zip_of(&z)).await;
    assert_eq!(r["files"]["refused"], 1, "{r}");
    assert_eq!(r["files"]["added"], 2);
    assert!(r["errors"][0].as_str().unwrap().contains("SHA-256"));
    assert!(!stored_path(e2.dir.path(), &s.shas[0]).exists());
    assert_eq!(count(&e2, "file_owners", e2.user_id).await, 2);
    assert_no_leftovers(&e2);
}

#[tokio::test]
async fn names_that_leave_originals_are_refused() {
    let s = source().await;
    let mut z = unzip(&s.zip);
    let mut files = json_of(&z, "nimrag/files.json");
    let bytes = z[&format!("originals/{}.fit", s.shas[0])].clone();
    // The right bytes under names that climb out of the folder (and one absolute path).
    z.insert("evil.fit".into(), bytes.clone());
    let list = files.as_array_mut().unwrap();
    list.clear();
    for name in [
        "originals/../evil.fit",
        "../evil.fit",
        "/evil.fit",
        "originals//x",
        "originals\\evil.fit",
    ] {
        list.push(
            json!({"name": name, "sha256": s.shas[0], "size": bytes.len(), "source": "watch",
            "data_type": 128, "sub_type": 4, "file_index": null, "file_number": null,
            "watch_timestamp": null, "received_at": "2026-01-01T00:00:00.000Z"}),
        );
    }
    z.insert("nimrag/files.json".into(), files.to_string().into_bytes());
    let (e2, cookie2) = fresh().await;
    let r = restore(&e2, &cookie2, &zip_of(&z)).await;
    assert_eq!(
        r["files"],
        json!({"added": 0, "already_here": 0, "missing_in_export": 0, "refused": 5}),
        "{r}"
    );
    assert_eq!(count(&e2, "file_owners", e2.user_id).await, 0);
    assert!(!stored_path(e2.dir.path(), &s.shas[0]).exists());
    assert_no_leftovers(&e2);
}

#[tokio::test]
async fn a_file_the_export_lacks_is_counted_as_missing() {
    let s = source().await;
    let mut z = unzip(&s.zip);
    let mut files = json_of(&z, "nimrag/files.json");
    files[0]["name"] = Value::Null;
    z.insert("nimrag/files.json".into(), files.to_string().into_bytes());
    let (e2, cookie2) = fresh().await;
    let r = restore(&e2, &cookie2, &zip_of(&z)).await;
    assert_eq!(r["files"]["missing_in_export"], 1, "{r}");
    assert_eq!(r["files"]["added"], 2);
}

#[tokio::test]
async fn zips_that_are_not_a_usable_nimrag_export_get_422() {
    let s = source().await;
    let (e2, cookie2) = fresh().await;
    let z = unzip(&s.zip);

    // Not a zip at all.
    let (st, j) = restore_raw(&e2, &cookie2, b"just some text".to_vec()).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(j["error"], "This is not a Nimrag export");

    // A zip without a manifest.
    let other: BTreeMap<String, Vec<u8>> = [("a.txt".to_owned(), b"hello".to_vec())].into();
    let (st, j) = restore_raw(&e2, &cookie2, zip_of(&other)).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(j["error"], "This is not a Nimrag export");

    // An export from before the exact part existed.
    let mut old = z.clone();
    old.retain(|k, _| !k.starts_with("nimrag/"));
    let (st, j) = restore_raw(&e2, &cookie2, zip_of(&old)).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(j["error"].as_str().unwrap().contains("older Nimrag"), "{j}");

    // Another format, and a newer version.
    for (mutate, want) in [
        (
            json!({"format": "something-else"}),
            "This is not a Nimrag export",
        ),
        (
            json!({"version": 2}),
            "newer Nimrag; update this server first",
        ),
    ] {
        let mut m = json_of(&z, "nimrag/manifest.json");
        for (k, v) in mutate.as_object().unwrap() {
            m[k] = v.clone();
        }
        let mut bad = z.clone();
        bad.insert("nimrag/manifest.json".into(), m.to_string().into_bytes());
        let (st, j) = restore_raw(&e2, &cookie2, zip_of(&bad)).await;
        assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(j["error"].as_str().unwrap().contains(want), "{j}");
    }

    // A missing JSON part.
    let mut damaged = z.clone();
    damaged.remove("nimrag/workouts.json");
    let (st, j) = restore_raw(&e2, &cookie2, zip_of(&damaged)).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(j["error"].as_str().unwrap().contains("damaged"), "{j}");

    // None of it stored anything.
    assert_eq!(count(&e2, "file_owners", e2.user_id).await, 0);
    assert_no_leftovers(&e2);
}

#[tokio::test]
async fn invalid_settings_are_skipped_and_counted() {
    let s = source().await;
    let mut z = unzip(&s.zip);
    z.insert(
        "nimrag/settings.json".into(),
        json!({"timezone": "Mars/Olympus", "step_goal": "0", "intensity_goal_min": "abc",
               "home_cards": "{}", "password_hash": "x"})
        .to_string()
        .into_bytes(),
    );
    let (e2, cookie2) = fresh().await;
    let r = restore(&e2, &cookie2, &zip_of(&z)).await;
    assert_eq!(r["settings_applied"], 0, "{r}");
    assert_eq!(r["settings_skipped"], 5);
    assert_eq!(count(&e2, "user_settings", e2.user_id).await, 0);
}

#[tokio::test]
async fn only_a_signed_in_user_with_the_csrf_header_may_restore() {
    let s = source().await;
    let (e2, cookie2) = fresh().await;
    let uri = "/api/import/nimrag?name=export.zip";
    let (st, _) = post_raw(&e2, None, true, uri, s.zip.clone()).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "signed out");
    let (st, _) = post_raw(&e2, Some(&cookie2), false, uri, s.zip.clone()).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "no X-Nimrag header");
    // A device token is not a session.
    let resp = e2
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {}", e2.token))
                .header("x-nimrag", "1")
                .body(Body::from(s.zip.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "device token");
    e2.add_user("boss", Role::Admin).await;
    let admin = e2.login_as("boss").await;
    let (st, _) = post_raw(&e2, Some(&admin), true, uri, s.zip.clone()).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "an admin has no data");
    let (st, _) = post_raw(
        &e2,
        Some(&cookie2),
        true,
        "/api/import/nimrag",
        s.zip.clone(),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "the name is required");
    assert_eq!(count(&e2, "file_owners", e2.user_id).await, 0);
}

#[tokio::test]
async fn the_garmin_import_sends_a_nimrag_export_to_the_restore() {
    let s = source().await;
    let (e2, cookie2) = fresh().await;
    let (st, j) = post_raw(
        &e2,
        Some(&cookie2),
        true,
        "/api/import/garmin?name=export.zip",
        s.zip.clone(),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        j["error"],
        "This is a Nimrag export: use Restore from a Nimrag export"
    );
    assert_eq!(count(&e2, "file_owners", e2.user_id).await, 0);
    assert_no_leftovers(&e2);
}
