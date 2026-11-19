#![allow(dead_code)]
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use nimrag_server::{api, db, tokens};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use tower::ServiceExt;

pub struct Env {
    pub dir: tempfile::TempDir,
    pub pool: SqlitePool,
    pub app: Router,
    pub token: String,
}

pub async fn env_with_limit(max_body: usize) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::open(dir.path()).await.unwrap();
    let (_, token) = tokens::create(&pool, "test-device").await.unwrap();
    let app = api::router(api::AppState::new(
        pool.clone(),
        dir.path().to_path_buf(),
        max_body,
    ));
    Env {
        dir,
        pool,
        app,
        token,
    }
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
