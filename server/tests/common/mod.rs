#![allow(dead_code)]
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use nimrag_server::users::Role;
use nimrag_server::{api, db, tokens, totp};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, OnceLock};
use tower::ServiceExt;

/// The data user every environment starts with, and the device token's owner.
pub const USER: &str = "tester";
/// RFC 6238's example secret ("12345678901234567890"): every test user's 2FA secret.
pub const TOTP_SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

pub struct Env {
    pub dir: tempfile::TempDir,
    pub pool: SqlitePool,
    pub app: Router,
    /// A device token of the data user [`USER`].
    pub token: String,
    /// Id of [`USER`].
    pub user_id: i64,
    /// The server's clock when it is pinned (unix seconds, movable); `None` is the real time.
    pub clock: Option<Arc<AtomicI64>>,
    /// Hand rows a test inserted without a `user_id` (and their files) to [`USER`] before every
    /// request, so a test that seeds with plain INSERTs sees its data. Isolation tests that
    /// seed per user explicitly switch it off.
    pub adopt: AtomicBool,
}

/// One argon2id hash of [`PASSWORD`], shared by every test user (hashing is slow in debug builds).
fn password_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| nimrag_server::auth::hash_password(PASSWORD).unwrap())
}

/// A user with the test password and 2FA enrolled (or not).
pub async fn make_user(pool: &SqlitePool, name: &str, role: Role, enrolled: bool) -> i64 {
    let id = nimrag_server::users::create(pool, name, password_hash(), role, false, 0)
        .await
        .unwrap()
        .expect("username free");
    if enrolled {
        nimrag_server::users::set_totp(pool, id, TOTP_SECRET, 0)
            .await
            .unwrap();
    }
    id
}

pub async fn env_with_limit(max_body: usize) -> Env {
    build(None, max_body).await
}

async fn build(clock: Option<i64>, max_body: usize) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::open(dir.path()).await.unwrap();
    let user_id = make_user(&pool, USER, Role::User, true).await;
    let (_, token) = tokens::create(&pool, "test-device", Some(user_id))
        .await
        .unwrap();
    let mut state = api::AppState::new(pool.clone(), dir.path().to_path_buf(), max_body);
    let clock = clock.map(|now| Arc::new(AtomicI64::new(now)));
    if let Some(c) = &clock {
        let c = c.clone();
        state = state.with_clock(move || c.load(Ordering::SeqCst));
    }
    Env {
        dir,
        pool,
        app: api::router(state),
        token,
        user_id,
        clock,
        adopt: AtomicBool::new(true),
    }
}

/// A database with no accounts at all, clock pinned to `now` (for the seed and setup tests).
pub async fn bare_env_at(now: i64) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::open(dir.path()).await.unwrap();
    let clock = Arc::new(AtomicI64::new(now));
    let c = clock.clone();
    let state = api::AppState::new(pool.clone(), dir.path().to_path_buf(), api::MAX_BODY_BYTES)
        .with_clock(move || c.load(Ordering::SeqCst));
    Env::from_parts(dir, pool, api::router(state), String::new(), Some(clock))
}

pub async fn env() -> Env {
    env_with_limit(api::MAX_BODY_BYTES).await
}

impl Env {
    pub async fn request(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Vec<u8>,
    ) -> (StatusCode, serde_json::Value) {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let resp = self
            .app
            .clone()
            .oneshot(req.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    /// Upload with `source=import` unless the query names a source.
    pub async fn upload(&self, query: &str, body: &[u8]) -> (StatusCode, serde_json::Value) {
        let query = if query.contains("source=") {
            query.to_string()
        } else {
            format!("{query}&source=import")
        };
        self.request(
            Method::POST,
            &format!("/api/files?{query}"),
            Some(&self.token),
            body.to_vec(),
        )
        .await
    }
}

/// Minimal valid FIT file: 12-byte header, file_id definition + one data
/// message (type=4 activity, time_created=`time`), CRC-16/ARC trailer.
pub fn tiny_fit(time: u32) -> Vec<u8> {
    let mut records = vec![
        0x40, 0x00, 0x00, 0x00, 0x00,
        0x02, // definition, local 0, little endian, global 0, 2 fields
        0x00, 0x01, 0x00, // field 0 (type), 1 byte, enum
        0x04, 0x04, 0x86, // field 4 (time_created), 4 bytes, uint32
        0x00, 0x04, // data record, local 0, type = 4
    ];
    records.extend_from_slice(&time.to_le_bytes());
    let mut file = vec![12, 0x10, 0x34, 0x08]; // size, protocol 1.0, profile 2100 LE
    file.extend_from_slice(&(records.len() as u32).to_le_bytes());
    file.extend_from_slice(b".FIT");
    file.extend_from_slice(&records);
    let crc = nimrag_core::crc::checksum(&file);
    file.extend_from_slice(&crc.to_le_bytes());
    file
}

async fn dump(pool: &SqlitePool, queries: &[(&str, &'static str)]) -> String {
    let mut out = String::new();
    for (name, q) in queries {
        for row in sqlx::query(*q).fetch_all(pool).await.unwrap() {
            out.push_str(name);
            for i in 0..sqlx::Row::columns(&row).len() {
                let v: String = row
                    .try_get::<Option<String>, _>(i)
                    .map(|v| format!("{v:?}"))
                    .or_else(|_| row.try_get::<Option<i64>, _>(i).map(|v| format!("{v:?}")))
                    .unwrap();
                out.push('|');
                out.push_str(&v);
            }
            out.push('\n');
        }
    }
    out
}

/// Full dump of the derived tables in deterministic order.
pub async fn dump_derived(pool: &SqlitePool) -> String {
    dump(
        pool,
        &[
            ("file_parse", "SELECT * FROM file_parse ORDER BY sha256"),
            (
                "fit_files",
                "SELECT * FROM fit_files ORDER BY sha256, chain_idx",
            ),
            (
                "fit_messages",
                "SELECT * FROM fit_messages ORDER BY sha256, chain_idx, msg_idx",
            ),
        ],
    )
    .await
}

/// Ledger dump without autoincrement ids.
pub async fn dump_ledger(pool: &SqlitePool) -> String {
    dump(
        pool,
        &[
            ("files", "SELECT * FROM files ORDER BY sha256"),
            ("uploads", "SELECT sha256, device_id, device_name, at, source, data_type, sub_type, file_index, file_number, watch_timestamp, created, repeat_count, last_at FROM uploads ORDER BY sha256, at"),
        ],
    )
    .await
}

pub fn stored_path(dir: &Path, sha: &str) -> std::path::PathBuf {
    dir.join("files")
        .join(&sha[0..2])
        .join(&sha[2..4])
        .join(sha)
}

// ---- owner API helpers -------------------------------------------------------------

pub const PASSWORD: &str = "correct horse battery";

/// An environment whose clock is pinned to `now` (unix seconds).
pub async fn env_at(now: i64) -> Env {
    build(Some(now), api::MAX_BODY_BYTES).await
}

/// Tables with a `user_id` column that is part of the derived rows.
pub const OWNED_TABLES: &[&str] = &[
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
    "body_measurements",
    "daily_totals",
];

/// Unix seconds of a local wall-clock time in `Europe/Copenhagen`.
pub fn cph(y: i32, m: u32, d: u32, h: u32, mi: u32) -> i64 {
    use chrono::TimeZone;
    chrono_tz::Europe::Copenhagen
        .with_ymd_and_hms(y, m, d, h, mi, 0)
        .earliest()
        .unwrap()
        .timestamp()
}

impl Env {
    /// The server's idea of now.
    pub fn now(&self) -> i64 {
        self.clock.as_ref().map_or_else(
            || {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
            },
            |c| c.load(Ordering::SeqCst),
        )
    }

    /// An environment around an app the test built itself: no data user, nothing adopted.
    pub fn from_parts(
        dir: tempfile::TempDir,
        pool: SqlitePool,
        app: Router,
        token: String,
        clock: Option<Arc<AtomicI64>>,
    ) -> Env {
        Env {
            dir,
            pool,
            app,
            token,
            user_id: 0,
            clock,
            adopt: AtomicBool::new(false),
        }
    }

    /// Give every row that has no owner yet to [`USER`] (see [`Env::adopt`]).
    pub async fn adopt_unowned(&self) {
        let uid = self.user_id;
        for t in OWNED_TABLES {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE {t} SET user_id = {uid} WHERE user_id = 0"
            )))
            .execute(&self.pool)
            .await
            .unwrap();
        }
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT OR IGNORE INTO file_owners (sha256, user_id) SELECT sha256, {uid} FROM files"
        )))
        .execute(&self.pool)
        .await
        .unwrap();
    }

    /// Any request: returns status, headers and the JSON body (Null when empty).
    pub async fn call(
        &self,
        method: Method,
        uri: &str,
        cookie: Option<&str>,
        csrf: bool,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, axum::http::HeaderMap, serde_json::Value) {
        if self.adopt.load(Ordering::Relaxed) {
            self.adopt_unowned().await;
        }
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, c);
        }
        if csrf {
            req = req.header("x-nimrag", "1");
        }
        let body = match body {
            Some(b) => {
                req = req.header(header::CONTENT_TYPE, "application/json");
                Body::from(b.to_string())
            }
            None => Body::empty(),
        };
        let resp = self
            .app
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let (status, headers) = (resp.status(), resp.headers().clone());
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            headers,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    /// Sign in as the data user; returns the `Cookie` header value.
    pub async fn login(&self) -> String {
        self.login_as(USER).await
    }

    /// Sign in as `username` (password [`PASSWORD`], 2FA secret [`TOTP_SECRET`]) with a fresh
    /// code. The replay guard is reset first so a test may sign in as often as it likes.
    pub async fn login_as(&self, username: &str) -> String {
        sqlx::query("UPDATE users SET totp_last_step = 0 WHERE username = ?")
            .bind(username)
            .execute(&self.pool)
            .await
            .unwrap();
        let code = totp::code_at(TOTP_SECRET, self.now()).unwrap();
        let (s, h, j) = self
            .call(
                Method::POST,
                "/api/login",
                None,
                false,
                Some(
                    serde_json::json!({ "username": username, "password": PASSWORD, "code": code }),
                ),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "login as {username}: {j}");
        let set = h.get(header::SET_COOKIE).unwrap().to_str().unwrap();
        set.split(';').next().unwrap().to_owned()
    }

    /// Another account; returns its id.
    pub async fn add_user(&self, name: &str, role: Role) -> i64 {
        make_user(&self.pool, name, role, true).await
    }

    /// A device token for `user_id`.
    pub async fn token_for(&self, user_id: i64) -> String {
        tokens::create(&self.pool, "phone", Some(user_id))
            .await
            .unwrap()
            .1
    }

    pub async fn get(&self, cookie: &str, uri: &str) -> serde_json::Value {
        let (s, _, j) = self.call(Method::GET, uri, Some(cookie), false, None).await;
        assert_eq!(s, StatusCode::OK, "GET {uri}: {j}");
        j
    }

    /// A state-changing request with the CSRF header.
    pub async fn send(
        &self,
        cookie: &str,
        method: Method,
        uri: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let (s, _, j) = self.call(method, uri, Some(cookie), true, Some(body)).await;
        (s, j)
    }

    pub async fn exec(&self, sql: &str) {
        sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
            .execute(&self.pool)
            .await
            .unwrap();
    }
}
