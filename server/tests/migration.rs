//! Migration 0010 (single owner -> users) on a database shaped like the live one: schema up to
//! 0009 with data in every table that changes. Nothing may be lost; the owner becomes the admin.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use nimrag_server::db;
use serde_json::json;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

/// Tables whose rows get a `user_id` column in the primary key (rebuilt by the migration).
const SAMPLE_TABLES: &[&str] = &[
    "mon_hr",
    "mon_activity",
    "mon_extra",
    "stress",
    "respiration",
    "spo2",
    "resting_hr",
    "sleep_levels",
    "hrv_values",
    "hrv_summary",
    "training_load",
    "training_status",
    "training_balance",
    "recovery_time",
    "vo2max",
    "battery",
];

async fn column_names(pool: &SqlitePool, table: &str) -> Vec<(String, String)> {
    sqlx::query(sqlx::AssertSqlSafe(format!("PRAGMA table_info({table})")))
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get::<String, _>("name"), r.get::<String, _>("type")))
        .collect()
}

/// Insert two rows with distinct, non-null values of the right type into `table`.
async fn fill(pool: &SqlitePool, table: &str) {
    let cols = column_names(pool, table).await;
    for n in 1..=2 {
        let names: Vec<&str> = cols.iter().map(|c| c.0.as_str()).collect();
        let values: Vec<String> = cols
            .iter()
            .map(|(name, ty)| match ty.to_ascii_uppercase().as_str() {
                "INTEGER" if name == "ts" => (1_000_000 + n * 60).to_string(),
                "INTEGER" => (n * 7).to_string(),
                "REAL" => format!("{}.5", n * 3),
                _ => format!("'{name}-{n}'"),
            })
            .collect();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {table} ({}) VALUES ({})",
            names.join(", "),
            values.join(", ")
        )))
        .execute(pool)
        .await
        .unwrap_or_else(|e| panic!("filling {table}: {e}"));
    }
}

async fn dump(pool: &SqlitePool, table: &str, cols: &[String]) -> Vec<String> {
    let sql = format!(
        "SELECT {} FROM {table} ORDER BY {}",
        cols.join(", "),
        cols.join(", ")
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(|r| {
            (0..cols.len())
                .map(|i| {
                    r.try_get::<Option<i64>, _>(i)
                        .map(|v| format!("{v:?}"))
                        .or_else(|_| r.try_get::<Option<f64>, _>(i).map(|v| format!("{v:?}")))
                        .or_else(|_| r.try_get::<Option<String>, _>(i).map(|v| format!("{v:?}")))
                        .unwrap()
                })
                .collect::<Vec<_>>()
                .join("|")
        })
        .collect()
}

struct Old {
    dir: tempfile::TempDir,
    hash: String,
    /// Table -> its rows before the migration (all columns that existed then).
    before: Vec<(String, Vec<String>, Vec<String>)>,
}

/// A data dir at schema version 9 with the owner, a session, tokens and data.
async fn old_database() -> Old {
    let dir = tempfile::tempdir().unwrap();
    let options = SqliteConnectOptions::new()
        .filename(dir.path().join(db::DB_FILE))
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let mut m = sqlx::migrate!("./migrations");
    m.migrations = m
        .migrations
        .iter()
        .filter(|x| x.version <= 9)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    m.run(&pool).await.unwrap();

    let hash = nimrag_server::auth::hash_password(PASSWORD).unwrap();
    sqlx::query("INSERT INTO owner (id, password_hash, updated_at) VALUES (1, ?, 1700000000)")
        .bind(&hash)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO sessions (id_hash, created_at, expires_at) VALUES ('abc', 1, 4000000000)",
    )
    .execute(&pool)
    .await
    .unwrap();
    for name in ["phone", "old tablet"] {
        sqlx::query("INSERT INTO tokens (name, hash) VALUES (?, ?)")
            .bind(name)
            .bind(format!("hash-of-{name}"))
            .execute(&pool)
            .await
            .unwrap();
    }
    for (k, v) in [
        ("timezone", "Europe/Copenhagen"),
        ("step_goal", "9000"),
        ("intensity_goal_min", "150"),
        ("home_cards", "{\"cards\":[]}"),
        ("sample_data", "1"),
    ] {
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?)")
            .bind(k)
            .bind(v)
            .execute(&pool)
            .await
            .unwrap();
    }
    // A file with its derived rows, plus the owner's own records.
    let sha = "a".repeat(64);
    sqlx::query("INSERT INTO files (sha256, size, received_at, source) VALUES (?, 1, '2026-11-19T00:00:00.000Z', 'watch')")
        .bind(&sha)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO uploads (sha256, device_id, device_name, at, source, created) VALUES (?, 1, 'phone', '2026-11-19T00:00:00.000Z', 'watch', 1)")
        .bind(&sha)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO body_measurements (measured_at, weight_kg, created_at) VALUES (1700000000, 80.5, 1700000000)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO strength_set_overrides (activity_ident, set_idx, fields, updated_at) VALUES ('act-1', 0, '{\"reps\":10}', 5)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO garmin_account (id, di_token, di_refresh_token, di_client_id, status, enabled_at, updated_at) VALUES (1, 'tok', 'ref', 'cid', 'connected', 1, 1)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO garmin_uploads (file_sha256, status, attempts, next_attempt_at, created_at) VALUES (?, 'pending', 0, 1, 1)")
        .bind(&sha)
        .execute(&pool)
        .await
        .unwrap();

    let mut before = Vec::new();
    for t in SAMPLE_TABLES {
        fill(&pool, t).await;
        let cols: Vec<String> = column_names(&pool, t)
            .await
            .into_iter()
            .map(|c| c.0)
            .collect();
        let rows = dump(&pool, t, &cols).await;
        assert_eq!(rows.len(), 2, "{t}");
        before.push((t.to_string(), cols, rows));
    }
    pool.close().await;
    Old { dir, hash, before }
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn owner_becomes_admin_and_nothing_is_lost() {
    let old = old_database().await;
    let pool = db::open(old.dir.path()).await.unwrap();

    // The owner is user `admin`: same hash byte for byte, 2FA not enrolled, no forced change.
    let admin = sqlx::query("SELECT username, password_hash, role, totp_enabled, totp_secret, must_change_password, disabled_at FROM users")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(admin.len(), 1);
    let a = &admin[0];
    assert_eq!(a.get::<String, _>("username"), "admin");
    assert_eq!(a.get::<String, _>("password_hash"), old.hash);
    assert_eq!(a.get::<String, _>("role"), "admin");
    assert_eq!(a.get::<i64, _>("totp_enabled"), 0);
    assert!(a.get::<Option<String>, _>("totp_secret").is_none());
    assert_eq!(a.get::<i64, _>("must_change_password"), 0);
    assert!(a.get::<Option<i64>, _>("disabled_at").is_none());
    let owner_gone: i64 = count(
        &pool,
        "SELECT COUNT(*) FROM sqlite_master WHERE name = 'owner'",
    )
    .await;
    assert_eq!(owner_gone, 0);

    // Sessions are cleared.
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM sessions").await, 0);

    // Device tokens survive, unassigned, still active, secrets untouched.
    let toks = sqlx::query(
        "SELECT name, hash, user_id, device_secret_hash, revoked_at FROM tokens ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(toks.len(), 2);
    assert_eq!(toks[0].get::<String, _>("hash"), "hash-of-phone");
    for t in &toks {
        assert!(t.get::<Option<i64>, _>("user_id").is_none());
        assert!(t.get::<Option<String>, _>("device_secret_hash").is_none());
        assert!(t.get::<Option<String>, _>("revoked_at").is_none());
    }

    // Files and uploads are untouched and nobody owns the file yet.
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM files").await, 1);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM uploads").await, 1);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM file_owners").await, 0);

    // Every sample row is still there, unchanged, in the unassigned partition 0.
    for (table, cols, rows) in &old.before {
        assert_eq!(&dump(&pool, table, cols).await, rows, "{table}");
        let moved = count(
            &pool,
            &format!("SELECT COUNT(*) FROM {table} WHERE user_id <> 0"),
        )
        .await;
        assert_eq!(moved, 0, "{table}");
    }

    // Owner records are kept under user 0; the server-wide flag stays global.
    let prefs: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM user_settings WHERE user_id = 0 ORDER BY key")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        prefs,
        vec![
            ("home_cards".to_string(), "{\"cards\":[]}".to_string()),
            ("intensity_goal_min".to_string(), "150".to_string()),
            ("step_goal".to_string(), "9000".to_string()),
            ("timezone".to_string(), "Europe/Copenhagen".to_string()),
        ]
    );
    let global: Vec<String> = sqlx::query_scalar("SELECT key FROM settings")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(global, vec!["sample_data".to_string()]);
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM body_measurements WHERE user_id = 0 AND weight_kg = 80.5"
        )
        .await,
        1
    );
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM strength_set_overrides WHERE user_id = 0 AND fields = '{\"reps\":10}'").await, 1);
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM garmin_account WHERE user_id = 0 AND di_token = 'tok'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM garmin_uploads WHERE user_id = 0"
        )
        .await,
        1
    );

    // Running the migrations again changes nothing (already applied).
    pool.close().await;
    let pool = db::open(old.dir.path()).await.unwrap();
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM users").await, 1);
}

#[tokio::test]
async fn migrated_admin_enrols_2fa_and_hands_the_old_data_to_a_user() {
    let old = old_database().await;
    let dir = old.dir;
    let pool = db::open(dir.path()).await.unwrap();
    let state = nimrag_server::api::AppState::new(
        pool.clone(),
        dir.path().to_path_buf(),
        nimrag_server::api::MAX_BODY_BYTES,
    );
    let e = Env::from_parts(
        dir,
        pool.clone(),
        nimrag_server::api::router(state),
        String::new(),
        None,
    );

    // Setup is closed: the owner already exists.
    let (_, _, j) = e.call(Method::GET, "/api/setup", None, false, None).await;
    assert_eq!(j["needed"], false);

    // The old password still works, but 2FA is not enrolled, so the login asks to enrol it.
    let (s, _, j) = e
        .call(
            Method::POST,
            "/api/login",
            None,
            false,
            Some(json!({ "username": "admin", "password": PASSWORD })),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(j["step"], "enrol2fa");
    let secret = j["secret"].as_str().unwrap().to_owned();
    let code = nimrag_server::totp::code_at(&secret, e.now()).unwrap();
    let (s, h, j) = e
        .call(
            Method::POST,
            "/api/login/totp-enrol",
            None,
            true,
            Some(json!({ "pending_token": j["pending_token"], "code": code })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["role"], "admin");
    let admin = h
        .get(axum::http::header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();

    // The admin has no data of their own.
    let (s, _, _) = e
        .call(Method::GET, "/api/today", Some(&admin), false, None)
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // The old phone's token and file are unassigned and offered for assignment, with the old records.
    let (s, _, j) = e
        .call(
            Method::GET,
            "/api/admin/unassigned",
            Some(&admin),
            false,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(j["tokens"].as_array().unwrap().len(), 2);
    let phone = j["tokens"][0].clone();
    assert_eq!(
        (
            phone["name"].as_str().unwrap(),
            phone["files"].as_i64().unwrap()
        ),
        ("phone", 1)
    );
    assert_eq!(j["legacy"]["body_entries"], 1);
    assert_eq!(j["legacy"]["garmin_account"], true);

    let user = make_user(&pool, "alice", nimrag_server::users::Role::User, true).await;
    let (s, _, j) = e
        .call(
            Method::POST,
            "/api/admin/unassigned/assign",
            Some(&admin),
            true,
            Some(json!({ "token_ids": [phone["id"]], "user_id": user, "adopt_legacy": true })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["tokens_moved"], 1);
    assert_eq!(j["files_moved"], 1);
    assert_eq!(j["legacy_adopted"], true);

    // The user now has the old goals, body entry and Garmin Forward account.
    let me = e.login_as("alice").await;
    let (_, _, settings) = e
        .call(Method::GET, "/api/settings", Some(&me), false, None)
        .await;
    assert_eq!(settings["step_goal"], 9000);
    assert_eq!(settings["timezone"], "Europe/Copenhagen");
    let (_, _, body) = e
        .call(Method::GET, "/api/body", Some(&me), false, None)
        .await;
    assert_eq!(body["entries"].as_array().unwrap().len(), 1);
    let (_, _, g) = e
        .call(Method::GET, "/api/garmin", Some(&me), false, None)
        .await;
    assert_eq!(g["state"], "connected");
    let (_, _, st) = e
        .call(Method::GET, "/api/storage", Some(&me), false, None)
        .await;
    assert_eq!(st["files"], 1);
}
