//! Scheduled backups, set by the admin (see `backup.rs` and `docs/design/backup-settings.md`).
//!
//! * `GET /api/admin/backups`: the settings in force and where they come from, the status, and
//!   the backups in the folder (newest first).
//! * `PUT /api/admin/backups`: validate, save in the global settings, apply at once.
//! * `DELETE /api/admin/backups`: forget the saved settings; the start-up options apply again.
//! * `POST /api/admin/backups/run`: one backup now (202; 409 while one runs).
//!
//! Every route needs an admin session; writes need the CSRF header like all writes. Texts that go
//! to the browser never contain the server's data dir path.

use super::ApiJson;
use crate::api::{ApiError, AppState};
use crate::auth::AdminUser;
use crate::backup::{self, Config, Saved};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/admin/backups",
            get(show).put(save).delete(use_startup),
        )
        .route("/api/admin/backups/run", post(run_now))
}

/// What the admin saved (or, in a request, wants to save).
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct BackupSettings {
    pub enabled: bool,
    /// Absolute folder on the server. May be empty only while backups are off.
    pub dir: String,
    /// Seconds between backups: 3600 to 30 days.
    #[ts(type = "number")]
    pub every_s: i64,
    /// How many backups to keep: 1 to 365.
    #[ts(type = "number")]
    pub keep: i64,
}

/// The options the server was started with.
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct BackupStartup {
    pub dir: String,
    #[ts(type = "number")]
    pub every_s: u64,
    #[ts(type = "number")]
    pub keep: u64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum BackupSource {
    /// Saved in the web app.
    Saved,
    /// The start-up options.
    Startup,
    /// Neither: backups are off.
    None,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct BackupOk {
    pub name: String,
    /// Unix seconds when it finished.
    #[ts(type = "number")]
    pub at: i64,
    #[ts(type = "number")]
    pub duration_ms: u64,
    /// Bytes written (files that were hard-linked from the last backup are not counted).
    #[ts(type = "number")]
    pub copied_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct BackupFailure {
    pub text: String,
    #[ts(type = "number")]
    pub at: i64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct BackupStatus {
    pub running: bool,
    /// Since the server started.
    pub last_ok: Option<BackupOk>,
    /// Since the server started, and cleared by the next success.
    pub last_error: Option<BackupFailure>,
    /// Unix seconds; `null` when backups are off.
    #[ts(type = "number | null")]
    pub next_at: Option<i64>,
}

/// One backup folder, from its `backup.json` (sizes are `null` when that cannot be read).
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct BackupEntry {
    pub name: String,
    /// Unix seconds (from the name).
    #[ts(type = "number")]
    pub at: i64,
    #[ts(type = "number | null")]
    pub files: Option<u64>,
    #[ts(type = "number | null")]
    pub files_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub db_bytes: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct BackupsResponse {
    /// The saved settings; `null` when none were saved.
    pub settings: Option<BackupSettings>,
    pub source: BackupSource,
    pub startup: Option<BackupStartup>,
    pub status: BackupStatus,
    /// In the folder in force, newest first.
    pub backups: Vec<BackupEntry>,
}

// ---- errors --------------------------------------------------------------------------------

enum Fail {
    Api(ApiError),
    /// A value that was refused: 422 with the field it belongs to.
    Invalid(&'static str, String),
}

impl From<ApiError> for Fail {
    fn from(e: ApiError) -> Self {
        Self::Api(e)
    }
}
impl From<anyhow::Error> for Fail {
    fn from(e: anyhow::Error) -> Self {
        Self::Api(e.into())
    }
}
impl From<tokio::task::JoinError> for Fail {
    fn from(e: tokio::task::JoinError) -> Self {
        anyhow::Error::from(e).into()
    }
}
impl From<sqlx::Error> for Fail {
    fn from(e: sqlx::Error) -> Self {
        Self::Api(e.into())
    }
}

impl IntoResponse for Fail {
    fn into_response(self) -> Response {
        match self {
            Self::Api(e) => e.into_response(),
            Self::Invalid(field, text) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "error": text, "field": field })),
            )
                .into_response(),
        }
    }
}

type Res<T> = Result<T, Fail>;

// ---- reading -------------------------------------------------------------------------------

/// The folder and keep count a backup by hand uses: the saved ones (also while backups are off),
/// else the start-up options.
async fn active(state: &AppState) -> Res<Option<(PathBuf, usize)>> {
    Ok(match backup::load_saved(&state.pool).await? {
        Some(s) => (!s.dir.is_empty()).then(|| (PathBuf::from(&s.dir), s.keep.max(1) as usize)),
        None => state.backups.startup().map(|c| (c.dir.clone(), c.keep)),
    })
}

fn to_startup(c: &Config) -> BackupStartup {
    BackupStartup {
        dir: c.dir.display().to_string(),
        every_s: c.every.as_secs(),
        keep: c.keep as u64,
    }
}

/// Newest first; sizes from each backup's `backup.json`. `Err` when the folder cannot be read.
fn read_folder(dir: &std::path::Path) -> anyhow::Result<Vec<BackupEntry>> {
    let mut found = backup::list(dir)?;
    found.reverse();
    Ok(found
        .into_iter()
        .map(|f| {
            let m: Option<backup::Manifest> = std::fs::read(f.path.join("backup.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
            BackupEntry {
                name: f.name,
                at: f.at,
                files: m.as_ref().map(|m| m.files),
                files_bytes: m.as_ref().map(|m| m.files_bytes),
                db_bytes: m.as_ref().map(|m| m.db_bytes),
            }
        })
        .collect())
}

async fn response(state: &AppState) -> Res<Json<BackupsResponse>> {
    let saved = backup::load_saved(&state.pool).await?;
    let startup = state.backups.startup().map(to_startup);
    let source = match (&saved, &startup) {
        (Some(_), _) => BackupSource::Saved,
        (None, Some(_)) => BackupSource::Startup,
        (None, None) => BackupSource::None,
    };
    let mut st = state.backups.status();
    let mut failure = st.last_error.take().map(|e| BackupFailure {
        text: e.text,
        at: e.at,
    });
    let mut backups = vec![];
    if let Some((dir, _)) = active(state).await? {
        match tokio::task::spawn_blocking(move || read_folder(&dir)).await? {
            Ok(list) => backups = list,
            Err(e) => {
                tracing::warn!(error = format!("{e:#}"), "listing the backup folder failed");
                if failure.is_none() {
                    failure = Some(BackupFailure {
                        text: "The backup folder cannot be read.".into(),
                        at: state.now(),
                    });
                }
            }
        }
    }
    Ok(Json(BackupsResponse {
        settings: saved.map(|s| BackupSettings {
            enabled: s.enabled,
            dir: s.dir,
            every_s: s.every_s as i64,
            keep: s.keep as i64,
        }),
        source,
        startup,
        status: BackupStatus {
            running: st.running,
            last_ok: st.last_ok.map(|o| BackupOk {
                name: o.name,
                at: o.at,
                duration_ms: o.duration_ms,
                copied_bytes: o.copied_bytes,
            }),
            last_error: failure,
            next_at: st.next_at,
        },
        backups,
    }))
}

async fn show(_: AdminUser, State(state): State<AppState>) -> Res<Json<BackupsResponse>> {
    response(&state).await
}

// ---- writing -------------------------------------------------------------------------------

async fn save(
    _: AdminUser,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<BackupSettings>,
) -> Res<Json<BackupsResponse>> {
    let invalid = |field, text: &str| Fail::Invalid(field, text.to_owned());
    if body.dir.len() > backup::MAX_DIR_BYTES {
        return Err(invalid("dir", "This folder name is too long."));
    }
    if !(backup::MIN_EVERY_S..=backup::MAX_EVERY_S).contains(&body.every_s) {
        return Err(invalid(
            "every_s",
            "Choose between every hour and every 30 days.",
        ));
    }
    if !(1..=backup::MAX_KEEP).contains(&body.keep) {
        return Err(invalid("keep", "Keep between 1 and 365 backups."));
    }
    // A folder that went away must not stop the admin from turning backups off.
    let dir = PathBuf::from(&body.dir);
    if body.enabled {
        let data = state.data_dir.as_ref().clone();
        tokio::task::spawn_blocking(move || backup::check_folder(&data, &dir))
            .await?
            .map_err(|text| Fail::Invalid("dir", text))?;
    } else if !dir.as_os_str().is_empty() && !dir.is_absolute() {
        return Err(invalid(
            "dir",
            "The folder must be a full path, starting with /.",
        ));
    }
    backup::store_saved(
        &state.pool,
        &Saved {
            enabled: body.enabled,
            dir: body.dir,
            every_s: body.every_s as u32,
            keep: body.keep as u32,
        },
    )
    .await?;
    state.backups.reload(&state.pool).await?;
    tracing::info!(enabled = body.enabled, "backup settings saved by an admin");
    response(&state).await
}

async fn use_startup(_: AdminUser, State(state): State<AppState>) -> Res<Json<BackupsResponse>> {
    backup::clear_saved(&state.pool).await?;
    state.backups.reload(&state.pool).await?;
    tracing::info!("saved backup settings removed by an admin");
    response(&state).await
}

async fn run_now(
    _: AdminUser,
    State(state): State<AppState>,
) -> Res<(StatusCode, Json<BackupsResponse>)> {
    let Some((dir, keep)) = active(&state).await? else {
        return Err(Fail::Invalid(
            "dir",
            "Set a backup folder first.".to_owned(),
        ));
    };
    let Some(guard) = state.backups.try_begin() else {
        return Err(ApiError::new(StatusCode::CONFLICT, "A backup is already running.").into());
    };
    let (pool, data) = (state.pool.clone(), state.data_dir.as_ref().clone());
    tokio::spawn(async move {
        backup::run_tracked(guard, &pool, &data, &dir, keep).await;
    });
    Ok((StatusCode::ACCEPTED, response(&state).await?))
}
