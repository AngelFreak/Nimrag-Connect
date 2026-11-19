//! HTTP API: POST /api/files, GET /api/files/{sha256}, GET /api/health.

use crate::ledger::{self, Meta, Sidecar, UploadEvent};
use crate::{derive, store, tokens};
use anyhow::Context;
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, FromRef, FromRequestParts, Path, Query, State},
    http::{header, request::Parts, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;

/// Default upload size limit.
pub const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub data_dir: Arc<PathBuf>,
    pub max_body: usize,
    /// Serialises sidecar read-modify-write.
    ledger_lock: Arc<tokio::sync::Mutex<()>>,
}

impl AppState {
    pub fn new(pool: SqlitePool, data_dir: PathBuf, max_body: usize) -> Self {
        Self {
            pool,
            data_dir: Arc::new(data_dir),
            max_body,
            ledger_lock: Default::default(),
        }
    }
}

pub fn router(state: AppState) -> Router {
    let max_body = state.max_body;
    Router::new()
        .route("/api/health", get(health))
        .route("/api/files", post(upload))
        .route("/api/files/{sha256}", get(file_info))
        .layer(DefaultBodyLimit::max(max_body))
        .with_state(state)
}

// ---- errors -------------------------------------------------------------

pub struct ApiError(StatusCode, String);

impl ApiError {
    fn bad_request(msg: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, msg.into())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!(error = format!("{e:#}"), "request failed");
        Self(StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        anyhow::Error::from(e).into()
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

// ---- auth ---------------------------------------------------------------

/// Authenticated device id (from `Authorization: Bearer <token>`).
pub struct Device {
    pub id: i64,
    pub name: String,
}

impl<S> FromRequestParts<S> for Device
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        let unauthorized = || ApiError(StatusCode::UNAUTHORIZED, "unauthorized".into());
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(unauthorized)?;
        let state = AppState::from_ref(state);
        match tokens::authenticate(&state.pool, token).await? {
            Some((id, name)) => Ok(Device { id, name }),
            None => Err(unauthorized()),
        }
    }
}

// ---- handlers -----------------------------------------------------------

async fn health(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    sqlx::query!("SELECT 1 as \"ok!: i64\"")
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

#[derive(Deserialize)]
pub struct UploadParams {
    data_type: Option<i64>,
    sub_type: Option<i64>,
    file_index: Option<i64>,
    file_number: Option<i64>,
    watch_timestamp: Option<i64>,
    source: Option<String>,
    /// Optional client-computed hash, verified against the received bytes.
    sha256: Option<String>,
}

#[derive(Serialize)]
struct UploadResponse {
    sha256: String,
    size: u64,
    created: bool,
}

/// Accepts any bytes: nothing is rejected because it fails to decode.
async fn upload(
    device: Device,
    State(state): State<AppState>,
    Query(params): Query<UploadParams>,
    body: Body,
) -> Result<(StatusCode, Json<UploadResponse>), ApiError> {
    let Some(source) = params.source.clone() else {
        return Err(ApiError::bad_request(
            "source is required (watch, usb or import)",
        ));
    };
    if !matches!(source.as_str(), "watch" | "usb" | "import") {
        return Err(ApiError::bad_request("source must be watch, usb or import"));
    }
    if params
        .sha256
        .as_deref()
        .is_some_and(|h| !crate::is_sha256_hex(h))
    {
        return Err(ApiError::bad_request(
            "sha256 must be 64 lowercase hex characters",
        ));
    }

    let received = match store::receive(&state.data_dir, body, state.max_body).await {
        Ok(r) => r,
        Err(store::ReceiveError::TooLarge) => {
            return Err(ApiError(
                StatusCode::PAYLOAD_TOO_LARGE,
                "body too large".into(),
            ))
        }
        Err(store::ReceiveError::Other(e)) => return Err(e.into()),
    };
    let store::Received { tmp, sha256, size } = received;
    if params
        .sha256
        .as_deref()
        .is_some_and(|claimed| claimed != sha256)
    {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(ApiError::bad_request(
            "sha256 does not match the uploaded bytes",
        ));
    }
    {
        let (dir, tmp, sha) = (state.data_dir.clone(), tmp, sha256.clone());
        tokio::task::spawn_blocking(move || store::commit(&dir, &tmp, &sha))
            .await
            .context("commit task")??;
    }

    // Record the upload in the sidecar (source of truth) first, then mirror it into the DB.
    let at = ledger::now(&state.pool).await?;
    let meta = Meta {
        source,
        data_type: params.data_type,
        sub_type: params.sub_type,
        file_index: params.file_index,
        file_number: params.file_number,
        watch_timestamp: params.watch_timestamp,
    };
    let (sidecar, event, repeat) = {
        let _guard = state.ledger_lock.lock().await;
        let existing = ledger::read_sidecar(&state.data_dir, &sha256)?;
        let created = existing.is_none();
        let mut event = UploadEvent {
            at: at.clone(),
            device_id: Some(device.id),
            device_name: Some(device.name),
            meta: meta.clone(),
            created,
            repeat_count: 0,
            last_at: None,
        };
        let mut sidecar = existing.unwrap_or_else(|| Sidecar {
            sha256: sha256.clone(),
            size: size as i64,
            received_at: at.clone(),
            meta,
            uploads: vec![],
        });
        // An identical re-upload only bumps a counter, keeping sidecars bounded.
        let repeat = match sidecar.uploads.last_mut() {
            Some(last) if last.same_as(&event) => {
                last.repeat_count += 1;
                last.last_at = Some(at);
                event = last.clone();
                true
            }
            _ => {
                sidecar.uploads.push(event.clone());
                false
            }
        };
        ledger::write_sidecar(&state.data_dir, &sidecar)?;
        (sidecar, event, repeat)
    };
    let created = event.created && !repeat;

    // Parse when the bytes are FIT and the database has no parse result yet.
    // Decided from the DB, not from `created`, so a retry after a failure repairs.
    let mut parsed = None;
    let head = {
        let (dir, sha) = (state.data_dir.clone(), sha256.clone());
        tokio::task::spawn_blocking(move || store::read_head(&dir, &sha, derive::SNIFF_LEN))
            .await
            .context("sniff task")??
    };
    if derive::looks_like_fit(&head) {
        let done = sqlx::query!(
            r#"SELECT 1 as "x!: i64" FROM file_parse WHERE sha256 = ?"#,
            sha256
        )
        .fetch_optional(&state.pool)
        .await?
        .is_some();
        if !done {
            let (dir, sha) = (state.data_dir.clone(), sha256.clone());
            parsed = Some(
                tokio::task::spawn_blocking(move || -> anyhow::Result<derive::Parsed> {
                    Ok(derive::parse(&store::read_verified(&dir, &sha)?))
                })
                .await
                .context("parse task")??,
            );
        }
    }

    let mut tx = state.pool.begin().await?;
    ledger::insert_file(&mut tx, &sidecar).await?;
    if repeat {
        ledger::bump_last_upload(&mut tx, &sha256, &event).await?;
    } else {
        ledger::insert_upload(&mut tx, &sha256, &event).await?;
    }
    if let Some(parsed) = &parsed {
        derive::write(&mut tx, &sha256, parsed).await?;
    }
    tx.commit().await?;

    tracing::info!(%sha256, size, created, device_id = device.id, "upload");
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        Json(UploadResponse {
            sha256,
            size,
            created,
        }),
    ))
}

#[derive(Serialize)]
struct FitSummary {
    fit_files: i64,
    messages: i64,
    /// True if every FIT file in the chain passed its CRC; null if none decoded.
    crc_ok: Option<bool>,
    /// null if the file was not FIT-parsed (not data_type 128).
    parse_status: Option<String>,
    parse_error: Option<String>,
}

#[derive(Serialize)]
struct FileInfo {
    sha256: String,
    size: i64,
    received_at: String,
    source: String,
    data_type: Option<i64>,
    sub_type: Option<i64>,
    file_index: Option<i64>,
    file_number: Option<i64>,
    watch_timestamp: Option<i64>,
    uploads: i64,
    fit: FitSummary,
}

async fn file_info(
    _device: Device,
    State(state): State<AppState>,
    Path(sha256): Path<String>,
) -> Result<Json<FileInfo>, ApiError> {
    if !crate::is_sha256_hex(&sha256) {
        return Err(ApiError::bad_request(
            "sha256 must be 64 lowercase hex characters",
        ));
    }
    let f = sqlx::query!(
        r#"SELECT sha256 as "sha256!", size, received_at, source, data_type, sub_type,
                  file_index, file_number, watch_timestamp
           FROM files WHERE sha256 = ?"#,
        sha256
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError(StatusCode::NOT_FOUND, "no such file".into()))?;
    let uploads = sqlx::query!(
        r#"SELECT COUNT(*) as "n!: i64" FROM uploads WHERE sha256 = ?"#,
        sha256
    )
    .fetch_one(&state.pool)
    .await?
    .n;
    let fit = sqlx::query!(
        r#"SELECT COUNT(*) as "files!: i64", COALESCE(SUM(message_count), 0) as "messages!: i64",
                  MIN(crc_ok) as "crc_ok: i64"
           FROM fit_files WHERE sha256 = ?"#,
        sha256
    )
    .fetch_one(&state.pool)
    .await?;
    let parse = sqlx::query!(
        "SELECT status, error FROM file_parse WHERE sha256 = ?",
        sha256
    )
    .fetch_optional(&state.pool)
    .await?;
    Ok(Json(FileInfo {
        sha256: f.sha256,
        size: f.size,
        received_at: f.received_at,
        source: f.source,
        data_type: f.data_type,
        sub_type: f.sub_type,
        file_index: f.file_index,
        file_number: f.file_number,
        watch_timestamp: f.watch_timestamp,
        uploads,
        fit: FitSummary {
            fit_files: fit.files,
            messages: fit.messages,
            crc_ok: fit.crc_ok.map(|v| v != 0),
            parse_status: parse.as_ref().map(|p| p.status.clone()),
            parse_error: parse.and_then(|p| p.error),
        },
    }))
}
