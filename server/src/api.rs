//! HTTP API: device upload (POST /api/files, GET /api/files/{sha256}), GET /api/health,
//! and the owner's JSON API (`web_api`).

use crate::ingest;
use crate::ledger::Meta;
use crate::{store, tokens};
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
/// Default size limit of `POST /api/import/garmin` (a whole Garmin account export is several GB).
pub const MAX_IMPORT_BYTES: usize = 8 * 1024 * 1024 * 1024;
/// Largest JSON body on the owner API (everything but the device upload).
pub const MAX_JSON_BYTES: usize = 64 * 1024;
/// The login bodies carry a password and a code.
pub const MAX_LOGIN_BYTES: usize = 4 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub data_dir: Arc<PathBuf>,
    pub max_body: usize,
    /// Size limit of the Garmin import upload (`--max-import-bytes`).
    pub max_import: usize,
    /// Permits for `POST /api/import/garmin` (see `web_api::garmin_import::MAX_IMPORTS`).
    pub(crate) import_gate: Arc<tokio::sync::Semaphore>,
    /// Serialises sidecar read-modify-write.
    pub(crate) ledger_lock: Arc<tokio::sync::Mutex<()>>,
    /// Session cookies without `Secure` (plain-http LAN use; `--insecure-cookies`).
    pub insecure_cookies: bool,
    /// Failed logins per client address.
    pub limiter: Arc<crate::auth::LoginLimiter>,
    /// Failed logins per username.
    pub username_limiter: Arc<crate::auth::UsernameLimiter>,
    /// Pending steps of the login (2FA enrolment, forced password change, first-run setup).
    pub(crate) pending: Arc<crate::auth::PendingStore>,
    /// One permit: password checks (argon2, 19 MiB each) run one at a time.
    pub(crate) login_gate: Arc<tokio::sync::Semaphore>,
    /// Networks whose `X-Forwarded-For` is believed (`--trusted-proxy`); empty by default.
    pub(crate) trusted_proxies: Arc<Vec<crate::auth::Cidr>>,
    /// Forward mode (Garmin Connect upload): runtime state and the worker wake-up.
    pub garmin: Arc<crate::garmin::Garmin>,
    /// Where `/tiles/<name>.pmtiles` is served from (`--tiles-dir`); `None`: no tiles route.
    pub tiles_dir: Option<Arc<PathBuf>>,
    /// Permits for `GET /api/export` (see `web_api::export::MAX_EXPORTS`).
    pub(crate) export_gate: Arc<tokio::sync::Semaphore>,
    /// Weather for the watch: Open-Meteo client, kill switch and cache (see `weather.rs`).
    pub weather: Arc<crate::weather::Weather>,
    /// GPS assist: the satellite-data endpoint for the phone (`--no-gps-assist` switches it off).
    pub(crate) gps_assist: Arc<crate::gps_assist::GpsAssist>,
    /// Serialises `POST /api/device/edits`, so two copies of one op never both run.
    pub(crate) edits_lock: Arc<tokio::sync::Mutex<()>>,
    /// "Send to watch" only queues the workout, without writing its FIT file to check it (the
    /// phone's copy: the server writes the file when the phone asks for it, and this build may
    /// have no profile to write it with).
    pub(crate) defer_watch_send: bool,
    /// Scheduled backups: start-up options, saved settings, status (see `backup.rs`).
    pub backups: Arc<crate::backup::Control>,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl AppState {
    pub fn new(pool: SqlitePool, data_dir: PathBuf, max_body: usize) -> Self {
        Self {
            pool,
            data_dir: Arc::new(data_dir),
            max_body,
            max_import: MAX_IMPORT_BYTES,
            import_gate: Arc::new(tokio::sync::Semaphore::new(
                crate::web_api::garmin_import::MAX_IMPORTS,
            )),
            ledger_lock: Default::default(),
            insecure_cookies: false,
            limiter: Default::default(),
            username_limiter: Arc::new(crate::auth::username_limiter()),
            pending: Default::default(),
            login_gate: Arc::new(tokio::sync::Semaphore::new(1)),
            trusted_proxies: Default::default(),
            garmin: Default::default(),
            tiles_dir: None,
            export_gate: Arc::new(tokio::sync::Semaphore::new(
                crate::web_api::export::MAX_EXPORTS,
            )),
            weather: Default::default(),
            gps_assist: Arc::new(crate::gps_assist::GpsAssist::new(Default::default())),
            edits_lock: Default::default(),
            defer_watch_send: false,
            backups: Arc::new(crate::backup::Control::new(None)),
            clock: Arc::new(system_now),
        }
    }

    pub fn with_insecure_cookies(mut self, insecure: bool) -> Self {
        self.insecure_cookies = insecure;
        self
    }

    /// The backup options given at start-up (`--backup-dir` and friends); settings the admin saved
    /// in the web app win over them.
    pub fn with_backup_startup(mut self, cfg: Option<crate::backup::Config>) -> Self {
        self.backups = Arc::new(crate::backup::Control::new(cfg));
        self
    }

    /// Let "Send to watch" queue a workout without writing its FIT file (the phone's copy).
    pub fn with_deferred_watch_send(mut self, defer: bool) -> Self {
        self.defer_watch_send = defer;
        self
    }

    /// Believe `X-Forwarded-For` from these peers when naming the login client.
    pub fn with_trusted_proxies(mut self, nets: Vec<crate::auth::Cidr>) -> Self {
        self.trusted_proxies = Arc::new(nets);
        self
    }

    /// Largest body `POST /api/import/garmin` takes (larger gets 413).
    pub fn with_max_import(mut self, bytes: usize) -> Self {
        self.max_import = bytes;
        self
    }

    /// Serve the `*.pmtiles` files of this directory under `/tiles/` (see `tiles.rs`).
    pub fn with_tiles_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.tiles_dir = dir.map(Arc::new);
        self
    }

    /// Replace the Garmin client configuration (tests point it at a fake Garmin).
    pub fn with_garmin(mut self, cfg: crate::garmin::GarminConfig) -> Self {
        self.garmin = Arc::new(crate::garmin::Garmin::new(cfg));
        self
    }

    /// Replace the weather configuration (kill switch, Open-Meteo address; tests
    /// point it at a fake).
    pub fn with_weather(mut self, cfg: crate::weather::WeatherConfig) -> Self {
        self.weather = Arc::new(crate::weather::Weather::new(cfg));
        self
    }

    /// Configure GPS assist (the kill switch; tests point the upstream at a fake).
    pub fn with_gps_assist(mut self, cfg: crate::gps_assist::GpsAssistConfig) -> Self {
        self.gps_assist = Arc::new(crate::gps_assist::GpsAssist::new(cfg));
        self
    }

    /// Replace the clock (unix seconds); tests use it to pin "today".
    pub fn with_clock(mut self, clock: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// Current unix time in seconds.
    pub fn now(&self) -> i64 {
        (self.clock)()
    }
}

fn system_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub fn router(state: AppState) -> Router {
    let (max_body, max_import) = (state.max_body, state.max_import);
    let mut app = Router::new();
    if state.tiles_dir.is_some() {
        // Signed-in users only; without the option `/tiles/*` is a 404 (the web fallback).
        app = app.route(
            "/tiles/{name}",
            get(crate::tiles::tile).head(crate::tiles::tile),
        );
    }
    app.route("/api/health", get(health))
        // The device upload is the only large body; a route-level limit wins over the global one.
        .route(
            "/api/files",
            post(upload).layer(DefaultBodyLimit::max(max_body)),
        )
        .route("/api/files/{sha256}", get(file_info))
        // The other large body: a Garmin export the signed-in user picked in the web app.
        .route(
            "/api/import/garmin",
            post(crate::web_api::garmin_import::import).layer(DefaultBodyLimit::max(max_import)),
        )
        // The same for a Nimrag export (the zip of "Download my data").
        .route(
            "/api/import/nimrag",
            post(crate::web_api::nimrag_import::import).layer(DefaultBodyLimit::max(max_import)),
        )
        .merge(crate::web_api::router())
        .merge(crate::weather::routes())
        .merge(crate::edits::routes())
        .merge(crate::history::routes())
        .fallback(crate::web::handler) // the web app (SPA); /api/* misses stay 404
        .layer(DefaultBodyLimit::max(MAX_JSON_BYTES))
        .layer(axum::middleware::from_fn(crate::auth::csrf_guard))
        .layer(axum::middleware::from_fn(crate::auth::api_headers))
        .with_state(state)
}

// ---- errors -------------------------------------------------------------

pub struct ApiError(StatusCode, String);

impl ApiError {
    pub fn new(status: StatusCode, msg: impl Into<String>) -> Self {
        Self(status, msg.into())
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, msg.into())
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self(StatusCode::NOT_FOUND, msg.into())
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

/// Authenticated device (from `Authorization: Bearer <token>`).
pub struct Device {
    pub id: i64,
    pub name: String,
    /// The user the token belongs to; `None` for an unassigned token.
    pub user_id: Option<i64>,
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
            Some(a) => Ok(Device {
                id: a.id,
                name: a.name,
                user_id: a.user_id,
            }),
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
    let store::Received { tmp, sha256, size } = &received;
    if params
        .sha256
        .as_deref()
        .is_some_and(|claimed| claimed != sha256)
    {
        let _ = tokio::fs::remove_file(tmp).await;
        return Err(ApiError::bad_request(
            "sha256 does not match the uploaded bytes",
        ));
    }
    let meta = Meta {
        source,
        data_type: params.data_type,
        sub_type: params.sub_type,
        file_index: params.file_index,
        file_number: params.file_number,
        watch_timestamp: params.watch_timestamp,
    };
    let device_id = device.id;
    let created = ingest::ingest(
        &state,
        &received,
        Some((device.id, device.name)),
        device.user_id,
        meta,
        None,
    )
    .await?;

    tracing::info!(%sha256, size, created, device_id, "upload");
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        Json(UploadResponse {
            sha256: sha256.clone(),
            size: *size,
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
    device: Device,
    State(state): State<AppState>,
    Path(sha256): Path<String>,
) -> Result<Json<FileInfo>, ApiError> {
    if !crate::is_sha256_hex(&sha256) {
        return Err(ApiError::bad_request(
            "sha256 must be 64 lowercase hex characters",
        ));
    }
    // A device only sees files of its own user (an unassigned token, unassigned files).
    let owner: Option<i64> = sqlx::query_scalar("SELECT user_id FROM file_owners WHERE sha256 = ?")
        .bind(&sha256)
        .fetch_optional(&state.pool)
        .await?;
    if owner != device.user_id {
        return Err(ApiError(StatusCode::NOT_FOUND, "no such file".into()));
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
