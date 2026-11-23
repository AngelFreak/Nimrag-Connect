//! `GET /api/export` and `nimrag-server export`: what is in the zip, whose it is, who may ask.
mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use nimrag_core::derive::test_support::{FitBuilder, Fv};
use nimrag_core::fit::profile;
use nimrag_server::seed::{self, SeedAccounts, SeedOptions};
use nimrag_server::sha256_hex;
use nimrag_server::users::Role;
use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use tower::ServiceExt;

const T0: u32 = 0x3000_0000;

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

async fn upload_as(e: &Env, token: &str, query: &str, body: &[u8]) -> String {
    let (s, j) = e
        .request(
            Method::POST,
            &format!("/api/files?{query}&source=import"),
            Some(token),
            body.to_vec(),
        )
        .await;
    assert!(s == StatusCode::CREATED || s == StatusCode::OK, "{s} {j}");
    j["sha256"].as_str().unwrap().to_owned()
}

struct Download {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    bytes: Vec<u8>,
}

async fn export_with(e: &Env, headers: &[(&str, String)]) -> Download {
    let mut req = Request::builder().method(Method::GET).uri("/api/export");
    for (k, v) in headers {
        req = req.header(*k, v);
    }
    let resp = e
        .app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (status, headers) = (resp.status(), resp.headers().clone());
    let bytes = resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    Download {
        status,
        headers,
        bytes,
    }
}

async fn export_for(e: &Env, cookie: &str) -> Download {
    export_with(e, &[("cookie", cookie.to_owned())]).await
}

/// Entry name -> bytes.
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

fn text(files: &BTreeMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(
        files
            .get(name)
            .unwrap_or_else(|| panic!("{name} missing from {:?}", files.keys()))
            .clone(),
    )
    .unwrap()
}

/// RFC 4180 parser: rows of cells.
fn parse_csv(s: &str) -> Vec<Vec<String>> {
    let (mut rows, mut row, mut cell) = (vec![], vec![], String::new());
    let (mut quoted, mut chars) = (false, s.chars().peekable());
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, c) => cell.push(c),
            (false, '"') => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut cell)),
            (false, '\n') => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            (false, c) => cell.push(c),
        }
    }
    assert!(cell.is_empty() && row.is_empty(), "no final newline");
    rows
}

struct Two {
    e: Env,
    a: i64,
    b: i64,
    cookie_a: String,
    cookie_b: String,
    tok_a: String,
    tok_b: String,
}

impl Two {
    /// Rows with values only one user has. Added after the uploads: a FIT upload re-derives
    /// the sample tables and drops rows that no stored file accounts for.
    async fn add_rows(&self) {
        let (e, a, b) = (&self.e, self.a, self.b);
        let t = cph(2026, 11, 19, 7, 42);
        for (uid, bpm, weight, steps) in [
            (a, 61.5, 80.25, 4321.5),
            (b, 162.5, 99.75, 9876.25),
            (0, 77.5, 55.5, 1111.75),
        ] {
            e.exec(&format!(
                "INSERT INTO mon_hr (user_id, ts, bpm, src) VALUES ({uid}, {t}, {bpm}, 1)"
            ))
            .await;
            e.exec(&format!(
            "INSERT INTO body_measurements (user_id, measured_at, weight_kg, created_at) VALUES ({uid}, {t}, {weight}, {t})"
        ))
        .await;
            e.exec(&format!(
                "INSERT INTO daily_totals (user_id, ts, steps, src) VALUES ({uid}, {}, {steps}, 1)",
                cph(2026, 11, 19, 0, 0)
            ))
            .await;
        }
        e.exec(&format!(
            "INSERT INTO hrv_values (user_id, ts, rmssd_ms, src) VALUES ({b}, {t}, 55.25, 1)"
        ))
        .await;
        let edit = serde_json::json!({ "reps": 10, "exercise": "Goblet squat, heavy \"one\"" });
        sqlx::query(
        "INSERT INTO strength_set_overrides (user_id, activity_ident, set_idx, fields, updated_at)
         VALUES (?, 'x:1', 2, ?, ?)",
    )
    .bind(a)
    .bind(edit.to_string())
    .bind(t)
    .execute(&e.pool)
    .await
    .unwrap();
    }
}

/// Two users, each with their own files and rows, plus unassigned ones that belong to nobody.
async fn two_users() -> Two {
    let e = env_at(cph(2026, 11, 20, 12, 0)).await;
    e.adopt.store(false, std::sync::atomic::Ordering::Relaxed);
    let (a, b) = (e.user_id, e.add_user("second", Role::User).await);
    let (tok_a, tok_b) = (e.token.clone(), e.token_for(b).await);
    for (uid, tz) in [(a, "Europe/Copenhagen"), (b, "UTC")] {
        e.exec(&format!(
            "INSERT INTO user_settings (user_id, key, value) VALUES ({uid}, 'timezone', '{tz}')"
        ))
        .await;
    }
    let cookie_a = e.login_as("tester").await;
    let cookie_b = e.login_as("second").await;
    Two {
        e,
        a,
        b,
        cookie_a,
        cookie_b,
        tok_a,
        tok_b,
    }
}

#[tokio::test]
async fn a_users_export_holds_their_own_rows_and_files_and_nothing_of_anyone_else() {
    let t = two_users().await;
    let e = &t.e;
    let a_files = [
        upload_as(e, &t.tok_a, "data_type=128", &tiny_fit(1)).await,
        upload_as(e, &t.tok_a, "data_type=4", b"a note from A").await,
        upload_as(e, &t.tok_a, "data_type=128", &activity(11)).await,
    ];
    let b_files = [
        upload_as(e, &t.tok_b, "data_type=128", &tiny_fit(2)).await,
        upload_as(e, &t.tok_b, "data_type=4", b"a note from B").await,
        upload_as(e, &t.tok_b, "data_type=128", &activity(22)).await,
    ];
    // A file nobody owns (an unassigned token).
    let unassigned = {
        let (_, token) = nimrag_server::tokens::create(&e.pool, "tool", None)
            .await
            .unwrap();
        upload_as(e, &token, "data_type=4", b"unassigned bytes").await
    };

    t.add_rows().await;
    let a = export_for(e, &t.cookie_a).await;
    assert_eq!(a.status, StatusCode::OK);
    let za = unzip(&a.bytes);
    let b = export_for(e, &t.cookie_b).await;
    let zb = unzip(&b.bytes);

    let originals = |z: &BTreeMap<String, Vec<u8>>| -> Vec<String> {
        z.keys()
            .filter_map(|k| k.strip_prefix("originals/"))
            .map(|k| k.split('.').next().unwrap().to_owned())
            .collect()
    };
    let mut want_a = a_files.to_vec();
    want_a.sort();
    let mut got_a = originals(&za);
    got_a.sort();
    assert_eq!(got_a, want_a, "exactly A's files");
    let mut want_b = b_files.to_vec();
    want_b.sort();
    let mut got_b = originals(&zb);
    got_b.sort();
    assert_eq!(got_b, want_b);

    // No byte of the other user's rows or file names anywhere in an archive, CSV or original.
    let everything = |z: &BTreeMap<String, Vec<u8>>| {
        let mut s = String::new();
        for (k, v) in z {
            s.push_str(k);
            s.push('\n');
            if k.starts_with("csv/") || k == "README.txt" {
                s.push_str(&String::from_utf8_lossy(v));
            }
        }
        s
    };
    let (sa, sb) = (everything(&za), everything(&zb));
    for sha in b_files.iter().chain([&unassigned]) {
        assert!(!sa.contains(sha), "A's export names {sha}");
    }
    for sha in a_files.iter().chain([&unassigned]) {
        assert!(!sb.contains(sha), "B's export names {sha}");
    }
    for needle in [
        "162.5",
        "99.75",
        "9876.25",
        "55.25",
        "77.5",
        "55.5",
        "1111.75",
        "for second",
    ] {
        assert!(!sa.contains(needle), "A's export holds {needle}");
    }
    for needle in ["61.5", "80.25", "4321.5", "Goblet", "for tester"] {
        assert!(!sb.contains(needle), "B's export holds {needle}");
    }
    assert!(!za
        .values()
        .any(|v| v.starts_with(b"a note from B") || v.starts_with(b"unassigned")));
    assert!(sa.contains("61.5") && sa.contains("80.25") && sa.contains("4321.5"));
    assert!(
        sb.contains("162.5")
            && sb.contains("99.75")
            && sb.contains("9876.25")
            && sb.contains("55.25")
    );

    // The ledger CSV lists A's files only.
    let files = parse_csv(&text(&za, "csv/files.csv"));
    assert_eq!(files.len(), 1 + a_files.len());
    // Every sha in A's activity CSVs is one of A's files.
    if profile::available() {
        for name in ["csv/activities.csv", "csv/activity_records.csv"] {
            let rows = parse_csv(&text(&za, name));
            assert!(rows.len() > 1, "{name}");
            for r in &rows[1..] {
                assert!(a_files.contains(&r[0]), "{name}: {}", r[0]);
            }
        }
    }
    let _ = (t.a, t.b);
}

#[tokio::test]
async fn the_admin_gets_403_and_a_device_token_or_no_session_is_refused() {
    let t = two_users().await;
    let e = &t.e;
    let admin = e.add_user("boss", Role::Admin).await;
    let _ = admin;
    let cookie = e.login_as("boss").await;
    assert_eq!(export_for(e, &cookie).await.status, StatusCode::FORBIDDEN);
    // A device token does not export, nor does anyone without a session.
    let d = export_with(e, &[("authorization", format!("Bearer {}", t.tok_a))]).await;
    assert_eq!(d.status, StatusCode::UNAUTHORIZED);
    assert_eq!(export_with(e, &[]).await.status, StatusCode::UNAUTHORIZED);
    // A signed-out cookie (garbage) neither.
    let d = export_with(e, &[("cookie", "__Host-nimrag_session=nope".into())]).await;
    assert_eq!(d.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn zip_layout_headers_times_and_quoting() {
    let t = two_users().await;
    let e = &t.e;
    let sha = upload_as(e, &t.tok_a, "data_type=128", &tiny_fit(5)).await;
    upload_as(e, &t.tok_a, "data_type=4", b"plain bytes").await;
    t.add_rows().await;
    let d = export_for(e, &t.cookie_a).await;
    assert_eq!(d.status, StatusCode::OK);
    assert_eq!(d.headers[header::CONTENT_TYPE], "application/zip");
    assert_eq!(d.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        d.headers[header::CONTENT_DISPOSITION],
        "attachment; filename=\"nimrag-export-tester-2026-11-20.zip\""
    );
    let z = unzip(&d.bytes);

    // Originals: byte for byte, FIT by content gets `.fit`, other bytes keep the bare stored name.
    assert_eq!(z[&format!("originals/{sha}.fit")], tiny_fit(5));
    assert_eq!(
        z[&format!("originals/{}", sha256_hex(b"plain bytes"))],
        b"plain bytes"
    );
    assert!(text(&z, "README.txt").contains("tester"));

    // Only kinds of data the user has: heart rate, body, daily totals, edits, settings, files.
    let csvs: Vec<&str> = z
        .keys()
        .filter(|k| k.starts_with("csv/"))
        .map(String::as_str)
        .collect();
    for want in [
        "csv/heart_rate.csv",
        "csv/body_measurements.csv",
        "csv/daily_totals.csv",
        "csv/strength_set_edits.csv",
        "csv/settings.csv",
        "csv/files.csv",
    ] {
        assert!(csvs.contains(&want), "{want} in {csvs:?}");
    }
    for absent in [
        "csv/spo2.csv",
        "csv/hrv_values.csv",
        "csv/sleep_sessions.csv",
        "csv/vo2max.csv",
    ] {
        assert!(
            !csvs.contains(&absent),
            "{absent} has no rows and is left out"
        );
    }

    // Times: ISO 8601 in the user's zone (Copenhagen, +01:00 in November) and UTC epoch seconds.
    let hr = parse_csv(&text(&z, "csv/heart_rate.csv"));
    assert_eq!(hr[0], ["time", "time_epoch_s", "bpm"]);
    assert_eq!(
        hr[1],
        [
            "2026-11-19T07:42:00+01:00".to_string(),
            cph(2026, 11, 19, 7, 42).to_string(),
            "61.5".into()
        ]
    );
    assert_eq!(hr.len(), 2);
    let days = parse_csv(&text(&z, "csv/daily_totals.csv"));
    assert_eq!(
        days[0],
        [
            "date",
            "date_start_epoch_s",
            "steps",
            "total_kcal",
            "active_kcal",
            "resting_kcal"
        ]
    );
    assert_eq!(days[1][0], "2026-11-19");
    assert_eq!(days[1][1], cph(2026, 11, 19, 0, 0).to_string());
    assert_eq!(days[1][2], "4321.5");
    // A missing value is an empty cell, not 0.
    assert_eq!(days[1][3], "");
    let body = parse_csv(&text(&z, "csv/body_measurements.csv"));
    assert_eq!(
        body[0],
        [
            "time",
            "time_epoch_s",
            "weight_kg",
            "body_fat_pct",
            "waist_cm",
            "entered",
            "entered_epoch_s"
        ]
    );
    assert_eq!(body[1][2], "80.25");
    assert_eq!(body[1][3], "");

    // Quotes and commas survive: the JSON of an edit comes back as it was stored.
    let edits = parse_csv(&text(&z, "csv/strength_set_edits.csv"));
    assert_eq!(
        edits[0],
        [
            "activity",
            "set",
            "changes_json",
            "updated",
            "updated_epoch_s"
        ]
    );
    assert_eq!(edits[1][0], "x:1");
    assert_eq!(edits[1][1], "2");
    let v: serde_json::Value = serde_json::from_str(&edits[1][2]).unwrap();
    assert_eq!(v["exercise"], "Goblet squat, heavy \"one\"");

    // The file list: name in the archive, hash, size, arrival in the user's zone.
    let files = parse_csv(&text(&z, "csv/files.csv"));
    assert_eq!(
        files[0],
        [
            "name",
            "sha256",
            "size_bytes",
            "received",
            "received_epoch_s",
            "source",
            "data_type",
            "sub_type",
            "file_number",
            "watch_timestamp"
        ]
    );
    let row = files.iter().find(|r| r[1] == sha).unwrap();
    assert_eq!(row[0], format!("originals/{sha}.fit"));
    assert_eq!(row[2], tiny_fit(5).len().to_string());
    assert!(
        row[3].ends_with("+01:00") || row[3].ends_with("+02:00"),
        "{}",
        row[3]
    );
    assert_eq!(row[5], "import");
    assert_eq!(row[6], "128");
}

#[tokio::test]
async fn activity_tables_come_out_joined_by_the_file_column() {
    // Needs every activity table (strength sets among them), which Nimrag's own map lacks.
    if !profile::garmin_profile() {
        return;
    }
    let t = two_users().await;
    let e = &t.e;
    let file = FitBuilder::new()
        .message(0, &[(3, Fv::U32(42)), (4, Fv::U32(T0 - 100))])
        .message(
            20,
            &[
                (253, Fv::U32(T0)),
                (3, Fv::U8(120)),
                (0, Fv::S32(1 << 30)),
                (1, Fv::S32(1 << 29)),
            ],
        )
        .message(20, &[(253, Fv::U32(T0 + 1)), (3, Fv::U8(121))])
        .message(
            19,
            &[(253, Fv::U32(T0 + 5)), (2, Fv::U32(T0)), (9, Fv::U32(5000))],
        )
        .message(
            18,
            &[(253, Fv::U32(T0 + 10)), (2, Fv::U32(T0)), (5, Fv::U8(1))],
        )
        .message(34, &[(253, Fv::U32(T0 + 20)), (1, Fv::U16(1))])
        .message(
            225,
            &[(254, Fv::U32(T0)), (3, Fv::U16(8)), (4, Fv::U16(160))],
        )
        .build();
    let sha = upload_as(e, &t.tok_a, "data_type=4", &file).await;
    let z = unzip(&export_for(e, &t.cookie_a).await.bytes);
    for name in [
        "csv/activities.csv",
        "csv/activity_sessions.csv",
        "csv/activity_laps.csv",
        "csv/activity_records.csv",
        "csv/strength_sets.csv",
    ] {
        let rows = parse_csv(&text(&z, name));
        assert!(rows.len() >= 2, "{name} has rows");
        assert_eq!(rows[0][0], "file");
        assert!(rows[1..].iter().all(|r| r[0] == sha), "{name}");
    }
    let rec = parse_csv(&text(&z, "csv/activity_records.csv"));
    assert_eq!(rec.len(), 3, "both record messages");
    assert_eq!(rec[0][..2], ["file", "time"]);
    // Positions are the user's own data and are in.
    let lat = rec[0].iter().position(|h| h == "latitude_deg").unwrap();
    assert!(!rec[1][lat].is_empty());
    let acts = parse_csv(&text(&z, "csv/activities.csv"));
    assert_eq!(acts[1][1], "fit");
    assert_eq!(z[&format!("originals/{sha}.fit")], file);
}

#[tokio::test]
async fn a_file_the_store_lost_is_left_out_and_listed_without_a_name() {
    let t = two_users().await;
    let e = &t.e;
    let kept = upload_as(&t.e, &t.tok_a, "data_type=4", b"kept").await;
    let lost = upload_as(&t.e, &t.tok_a, "data_type=4", b"lost").await;
    std::fs::remove_file(stored_path(e.dir.path(), &lost)).unwrap();
    let d = export_for(e, &t.cookie_a).await;
    assert_eq!(d.status, StatusCode::OK);
    let z = unzip(&d.bytes);
    assert!(z.contains_key(&format!("originals/{kept}")));
    assert!(!z.contains_key(&format!("originals/{lost}")));
    let files = parse_csv(&text(&z, "csv/files.csv"));
    assert_eq!(files.iter().find(|r| r[1] == lost).unwrap()[0], "");
    assert!(text(&z, "README.txt").contains("1 of them"));
}

#[tokio::test]
async fn every_table_of_a_seeded_user_exports_with_the_database_row_counts() {
    let now = cph(2026, 11, 19, 20, 0);
    let mut e = bare_env_at(now).await;
    let accounts = SeedAccounts {
        admin_password: Some(PASSWORD.into()),
        admin_totp_secret: Some(TOTP_SECRET.into()),
        user_password: Some(PASSWORD.into()),
        user_totp_secret: Some(TOTP_SECRET.into()),
    };
    let report = seed::run(
        &e.pool,
        e.dir.path(),
        SeedOptions {
            tz: chrono_tz::Europe::Copenhagen,
            now,
            days: 30,
            accounts,
        },
    )
    .await
    .unwrap();
    e.user_id = report.user_id;
    let cookie = e.login_as("demo").await;
    let z = unzip(&export_for(&e, &cookie).await.bytes);
    let uid = report.user_id;
    let rows = |name: &str| parse_csv(&text(&z, &format!("csv/{name}.csv"))).len() - 1;
    for (csv, table) in [
        ("heart_rate", "mon_hr"),
        ("steps_activity", "mon_activity"),
        ("intensity_minutes", "mon_extra"),
        ("stress_body_battery", "stress"),
        ("respiration", "respiration"),
        ("spo2", "spo2"),
        ("resting_hr", "resting_hr"),
        ("sleep_stages", "sleep_levels"),
        ("hrv_values", "hrv_values"),
        ("hrv_summary", "hrv_summary"),
        ("training_load", "training_load"),
        ("training_status", "training_status"),
        ("training_balance", "training_balance"),
        ("recovery_time", "recovery_time"),
        ("vo2max", "vo2max"),
        ("watch_battery", "battery"),
        ("daily_totals", "daily_totals"),
        ("connect_daily", "connect_daily"),
        ("body_measurements", "body_measurements"),
    ] {
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {table} WHERE user_id = {uid}"
        )))
        .fetch_one(&e.pool)
        .await
        .unwrap();
        if n == 0 {
            assert!(!z.contains_key(&format!("csv/{csv}.csv")), "{csv}");
        } else {
            assert_eq!(rows(csv) as i64, n, "{csv} vs {table}");
        }
    }
    for (csv, view) in [
        ("activities", "activities_current"),
        ("sleep_sessions", "sleep_sessions_current"),
    ] {
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {view} WHERE user_id = {uid}"
        )))
        .fetch_one(&e.pool)
        .await
        .unwrap();
        assert!(n > 0, "the seed has {view}");
        assert_eq!(rows(csv) as i64, n, "{csv}");
    }
    for (csv, table) in [
        ("activity_sessions", "activity_sessions"),
        ("activity_laps", "activity_laps"),
        ("activity_records", "activity_records"),
        ("strength_sets", "strength_sets"),
    ] {
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {table} t JOIN activities_current a ON a.sha256 = t.sha256 WHERE a.user_id = {uid}"
        )))
        .fetch_one(&e.pool)
        .await
        .unwrap();
        assert!(n > 0, "the seed has {table}");
        assert_eq!(rows(csv) as i64, n, "{csv}");
    }
    // The seed stores no files: no originals, and so no file list.
    assert!(!z.keys().any(|k| k.starts_with("originals/")));
    assert!(!z.contains_key("csv/files.csv"));
}

#[tokio::test]
async fn only_two_exports_run_at_once_and_a_dropped_one_frees_its_place() {
    let t = two_users().await;
    let e = &t.e;
    // Random bytes that do not compress: more output than the queue towards the client holds,
    // so an export nobody reads stays running.
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut blob = vec![0u8; 2 << 20];
    for b in &mut blob {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = x as u8;
    }
    upload_as(e, &t.tok_a, "data_type=4", &blob).await;
    let start = |cookie: String| {
        let app = e.app.clone();
        async move {
            let req = Request::builder()
                .uri("/api/export")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap();
            app.oneshot(req).await.unwrap()
        }
    };
    let first = start(t.cookie_a.clone()).await;
    let second = start(t.cookie_a.clone()).await;
    assert_eq!(
        (first.status(), second.status()),
        (StatusCode::OK, StatusCode::OK)
    );
    let third = start(t.cookie_a.clone()).await;
    assert_eq!(third.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(third.headers().contains_key(header::RETRY_AFTER));
    // The clients leave: the exports end and the places come free.
    drop(first);
    drop(second);
    let mut ok = false;
    for _ in 0..100 {
        let r = start(t.cookie_a.clone()).await;
        if r.status() == StatusCode::OK {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(ok, "a place never came free");
}

// ---- the command line ---------------------------------------------------------------------------

fn bin(args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .args(args)
        .output()
        .unwrap()
}

#[tokio::test]
async fn the_command_line_export_is_the_same_zip_and_runs_beside_a_server() {
    let t = two_users().await;
    let e = &t.e;
    upload_as(e, &t.tok_a, "data_type=128", &tiny_fit(31)).await;
    upload_as(e, &t.tok_a, "data_type=4", b"for the cli").await;
    upload_as(e, &t.tok_b, "data_type=4", b"for the other").await;
    let http = unzip(&export_for(e, &t.cookie_a).await.bytes);

    // `serve` holds the data-dir lock; `export` does not need it.
    let dir = e.dir.path().to_str().unwrap().to_owned();
    let serve = std::process::Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .args(["serve", "--listen", "127.0.0.1:0", "--data-dir", &dir])
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    struct Kill(std::process::Child);
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _serve = Kill(serve);
    for _ in 0..100 {
        if std::fs::read_to_string(e.dir.path().join(".lock")).is_ok_and(|s| s == "serve") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("mine.zip");
    let o = bin(&[
        "export",
        "--user",
        "tester",
        "--out",
        out.to_str().unwrap(),
        "--data-dir",
        &dir,
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let said = String::from_utf8_lossy(&o.stdout);
    assert!(said.contains("2 original files"), "{said}");
    let cli = unzip(&std::fs::read(&out).unwrap());
    assert_eq!(
        cli.keys().collect::<Vec<_>>(),
        http.keys().collect::<Vec<_>>()
    );
    for (k, v) in &http {
        if k != "README.txt" {
            assert_eq!(&cli[k], v, "{k}");
        }
    }
    assert!(!out_dir.path().join("mine.zip.part").exists());

    // The admin, an unknown user and a database nobody migrated: refused, nothing left behind.
    e.add_user("boss", Role::Admin).await;
    for (user, why) in [("boss", "admin"), ("nobody", "no user"), ("TESTER", "")] {
        let target = out_dir.path().join(format!("{user}.zip"));
        let o = bin(&[
            "export",
            "--user",
            user,
            "--out",
            target.to_str().unwrap(),
            "--data-dir",
            &dir,
        ]);
        if user == "TESTER" {
            // Names are case-insensitive, like the login.
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        } else {
            assert!(!o.status.success(), "{why}");
            assert!(!target.exists());
            assert!(!out_dir.path().join(format!("{user}.zip.part")).exists());
        }
    }
    let empty = tempfile::tempdir().unwrap();
    let o = bin(&[
        "export",
        "--user",
        "tester",
        "--out",
        out_dir.path().join("x.zip").to_str().unwrap(),
        "--data-dir",
        empty.path().to_str().unwrap(),
    ]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("no database"));
}
