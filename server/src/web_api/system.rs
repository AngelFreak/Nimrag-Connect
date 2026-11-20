//! `GET /api/devices` (watch battery and sync times, upload devices) and
//! `GET /api/storage` (what the server holds) and `GET /api/about` (which server this is).

use super::Ctx;
use crate::api::{ApiError, AppState};
use crate::auth::{AuthUser, DataUser};
use crate::settings::stored_rfc3339;
use crate::tokens;
use axum::{extract::State, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/devices", get(devices))
        .route("/api/storage", get(storage))
        .route("/api/about", get(about))
}

/// What the profile page's About row shows.
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct AboutResponse {
    /// The server's own version (`CARGO_PKG_VERSION`), for example `0.1.0`.
    pub version: String,
}

/// `GET /api/about`: for any signed-in session, admins included (they have no data, but they do
/// have a profile page). A signed-out caller gets 401, so the version is not public.
async fn about(_user: AuthUser) -> Json<AboutResponse> {
    Json(AboutResponse {
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct WatchInfo {
    /// Not stored by any derived table yet: always `null`.
    pub model: Option<String>,
    /// Not stored by any derived table yet: always `null`.
    pub firmware: Option<String>,
    pub battery_percent: Option<f64>,
    pub battery_at: Option<String>,
    /// Time of the newest watch sample we hold.
    pub last_data_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct UploadDevice {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub active: bool,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct DevicesResponse {
    pub sample: bool,
    pub watch: WatchInfo,
    /// When the newest file arrived on the server.
    pub last_sync_at: Option<String>,
    /// Files that were new in the last 24 hours.
    pub new_files_24h: u32,
    /// Phones and other uploaders (device tokens); secrets are never returned.
    pub upload_devices: Vec<UploadDevice>,
}

pub async fn last_upload(ctx: &Ctx) -> Res<Option<String>> {
    let at: Option<String> = sqlx::query_scalar(
        "SELECT MAX(u.at) FROM uploads u JOIN file_owners o ON o.sha256 = u.sha256 AND o.user_id = ?",
    )
    .bind(ctx.uid)
    .fetch_one(&ctx.pool)
    .await?;
    Ok(at.map(|s| stored_rfc3339(ctx.tz(), &s)))
}

pub async fn last_data_ts(ctx: &Ctx) -> Res<Option<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT MAX(s.last_ts) FROM sources s JOIN file_owners o ON o.sha256 = s.sha256 AND o.user_id = ?
         WHERE s.origin = 'fit'",
    )
    .bind(ctx.uid)
    .fetch_one(&ctx.pool)
    .await?)
}

async fn devices(user: DataUser, State(state): State<AppState>) -> Res<Json<DevicesResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let battery: Option<(i64, f64)> = sqlx::query_as(
        "SELECT ts, percent FROM battery WHERE user_id = ? ORDER BY ts DESC LIMIT 1",
    )
    .bind(ctx.uid)
    .fetch_optional(&ctx.pool)
    .await?;
    let since =
        crate::settings::rfc3339(chrono_tz::Tz::UTC, ctx.now - 86_400).replace("+00:00", "Z");
    let new_files: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM uploads u JOIN file_owners o ON o.sha256 = u.sha256 AND o.user_id = ?
             WHERE u.created = 1 AND u.at >= ?",
    )
    .bind(ctx.uid)
    .bind(since)
    .fetch_one(&ctx.pool)
    .await?;
    let tz = ctx.tz();
    Ok(Json(DevicesResponse {
        sample: ctx.sample(),
        watch: WatchInfo {
            model: None,
            firmware: None,
            battery_percent: battery.map(|b| b.1),
            battery_at: ctx.opt_ts(battery.map(|b| b.0)),
            last_data_at: ctx.opt_ts(last_data_ts(&ctx).await?),
        },
        last_sync_at: last_upload(&ctx).await?,
        new_files_24h: new_files as u32,
        upload_devices: tokens::list(&state.pool, tokens::Whose::User(user.id))
            .await?
            .into_iter()
            .map(|t| UploadDevice {
                id: t.id,
                name: t.name,
                created_at: stored_rfc3339(tz, &t.created_at),
                last_used_at: t.last_used_at.map(|s| stored_rfc3339(tz, &s)),
                active: t.revoked_at.is_none(),
            })
            .collect(),
    }))
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct SourceStorage {
    pub source: String,
    pub files: u32,
    pub bytes: u64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct StorageResponse {
    pub sample: bool,
    /// Original files kept (content-addressed, never altered).
    pub files: u32,
    pub bytes: u64,
    pub by_source: Vec<SourceStorage>,
    pub parsed_ok: u32,
    pub parsed_partial: u32,
    pub parsed_error: u32,
    /// Size of the SQLite database (with its write-ahead log).
    pub database_bytes: Option<u64>,
    /// Backups are not built yet: always `null`.
    pub last_backup_at: Option<String>,
}

async fn storage(user: DataUser, State(state): State<AppState>) -> Res<Json<StorageResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let by: Vec<(String, i64, i64)> =
        sqlx::query_as(
            "SELECT f.source, COUNT(*), COALESCE(SUM(f.size), 0) FROM files f
             JOIN file_owners o ON o.sha256 = f.sha256 AND o.user_id = ? GROUP BY f.source ORDER BY f.source",
        )
        .bind(ctx.uid)
        .fetch_all(&ctx.pool)
            .await?;
    let parse: Vec<(String, i64)> = sqlx::query_as(
        "SELECT p.status, COUNT(*) FROM file_parse p
             JOIN file_owners o ON o.sha256 = p.sha256 AND o.user_id = ? GROUP BY p.status",
    )
    .bind(ctx.uid)
    .fetch_all(&ctx.pool)
    .await?;
    let n = |s: &str| parse.iter().find(|p| p.0 == s).map_or(0, |p| p.1 as u32);
    let mut db_bytes = None;
    for suffix in ["", "-wal"] {
        let path = state
            .data_dir
            .join(format!("{}{suffix}", crate::db::DB_FILE));
        if let Ok(m) = tokio::fs::metadata(path).await {
            db_bytes = Some(db_bytes.unwrap_or(0) + m.len());
        }
    }
    Ok(Json(StorageResponse {
        sample: ctx.sample(),
        files: by.iter().map(|b| b.1 as u32).sum(),
        bytes: by.iter().map(|b| b.2 as u64).sum(),
        by_source: by
            .into_iter()
            .map(|b| SourceStorage {
                source: b.0,
                files: b.1 as u32,
                bytes: b.2 as u64,
            })
            .collect(),
        parsed_ok: n("ok"),
        parsed_partial: n("partial"),
        parsed_error: n("error"),
        database_bytes: db_bytes,
        last_backup_at: None,
    }))
}
