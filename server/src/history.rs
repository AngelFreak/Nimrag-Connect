//! The server's stored files for a phone (docs/design/history-on-phone.md): the list of the
//! token user's files and their bytes, so the phone's copy can ingest what only the server has.
//!
//! ```text
//! GET /api/device/files?cursor=&limit=&since=   (device token, Authorization: Bearer)
//!   -> {"files": [{sha256, size, source, data_type, sub_type, file_index, file_number,
//!                  watch_timestamp, received_at, last_ts}], "next_cursor": string | null}
//!      newest received first; 403 for a token without a user
//! GET  /api/device/files/{sha256}/content
//!   -> the stored bytes; 404 unless the file is this user's
//! ```
//!
//! `since` (unix seconds) keeps the files whose last data timestamp (`sources.last_ts`) is at or
//! after it, plus the files with no known one (a file that did not parse, say): a window by
//! when the data happened, not by when it was uploaded.

use crate::api::{ApiError, AppState, Device};
use crate::store;
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;

type Res<T> = Result<T, ApiError>;

const DEFAULT_LIMIT: i64 = 500;
const MAX_LIMIT: i64 = 1000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/device/files", get(list))
        .route("/api/device/files/{sha256}/content", get(content))
}

fn device_user(device: &Device) -> Res<i64> {
    device.user_id.ok_or_else(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "this device token is not assigned to a user",
        )
    })
}

#[derive(Deserialize)]
struct ListQuery {
    cursor: Option<String>,
    limit: Option<i64>,
    since: Option<i64>,
}

#[derive(Serialize)]
struct FileRow {
    sha256: String,
    size: i64,
    source: String,
    data_type: Option<i64>,
    sub_type: Option<i64>,
    file_index: Option<i64>,
    file_number: Option<i64>,
    watch_timestamp: Option<i64>,
    received_at: String,
    last_ts: Option<i64>,
}

#[derive(Serialize)]
struct FileList {
    files: Vec<FileRow>,
    next_cursor: Option<String>,
}

/// The cursor is the last row's sort key, hex-coded so it is opaque and safe in a URL.
fn encode_cursor(received_at: &str, sha256: &str) -> String {
    crate::hex(format!("{received_at}|{sha256}").as_bytes())
}

fn decode_cursor(c: &str) -> Option<(String, String)> {
    if c.len() % 2 != 0 || !c.is_ascii() {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..c.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&c[i..i + 2], 16).ok())
        .collect();
    let text = String::from_utf8(bytes?).ok()?;
    let (at, sha) = text.split_once('|')?;
    crate::is_sha256_hex(sha).then(|| (at.to_owned(), sha.to_owned()))
}

async fn list(
    device: Device,
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Res<Json<FileList>> {
    let uid = device_user(&device)?;
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let after = match q.cursor.as_deref().filter(|c| !c.is_empty()) {
        None => None,
        Some(c) => Some(decode_cursor(c).ok_or_else(|| ApiError::bad_request("bad cursor"))?),
    };
    let (after_at, after_sha) = after.unzip();
    // One row more than asked for tells whether there is a next page.
    let rows = sqlx::query(
        "SELECT f.sha256, f.size, f.source, f.data_type, f.sub_type, f.file_index, f.file_number,
                f.watch_timestamp, f.received_at, s.last_ts
         FROM files f
         JOIN file_owners o ON o.sha256 = f.sha256 AND o.user_id = ?1
         LEFT JOIN sources s ON s.sha256 = f.sha256
         WHERE (?2 IS NULL OR s.last_ts IS NULL OR s.last_ts >= ?2)
           AND (?3 IS NULL OR f.received_at < ?3 OR (f.received_at = ?3 AND f.sha256 > ?4))
         ORDER BY f.received_at DESC, f.sha256 ASC
         LIMIT ?5",
    )
    .bind(uid)
    .bind(q.since)
    .bind(after_at)
    .bind(after_sha)
    .bind(limit + 1)
    .fetch_all(&state.pool)
    .await?;
    let mut files = Vec::with_capacity(rows.len());
    for r in &rows {
        files.push(FileRow {
            sha256: r.try_get("sha256")?,
            size: r.try_get("size")?,
            source: r.try_get("source")?,
            data_type: r.try_get("data_type")?,
            sub_type: r.try_get("sub_type")?,
            file_index: r.try_get("file_index")?,
            file_number: r.try_get("file_number")?,
            watch_timestamp: r.try_get("watch_timestamp")?,
            received_at: r.try_get("received_at")?,
            last_ts: r.try_get("last_ts")?,
        });
    }
    let next_cursor = if files.len() as i64 > limit {
        files.truncate(limit as usize);
        files
            .last()
            .map(|f| encode_cursor(&f.received_at, &f.sha256))
    } else {
        None
    };
    Ok(Json(FileList { files, next_cursor }))
}

async fn content(
    device: Device,
    State(state): State<AppState>,
    Path(sha256): Path<String>,
) -> Res<Response> {
    let uid = device_user(&device)?;
    if !crate::is_sha256_hex(&sha256) {
        return Err(ApiError::bad_request(
            "sha256 must be 64 lowercase hex characters",
        ));
    }
    // Same rule as `file_info`: only the token user's own files.
    let owner: Option<i64> = sqlx::query_scalar("SELECT user_id FROM file_owners WHERE sha256 = ?")
        .bind(&sha256)
        .fetch_optional(&state.pool)
        .await?;
    if owner != Some(uid) {
        return Err(ApiError::not_found("no such file"));
    }
    let (dir, sha) = (state.data_dir.clone(), sha256);
    let bytes = tokio::task::spawn_blocking(move || store::read_verified(&dir, &sha))
        .await
        .map_err(|e| anyhow::anyhow!("read task: {e}"))??;
    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response())
}
