//! The signed-in user's own Garmin endpoints (a normal user; admins have no data). Session
//! cookie on every call, `X-Nimrag: 1` on the writes (the shared CSRF guard covers everything under `/api`).
//!
//! No response and no error text ever holds a password or a token.

use super::auth::{self, AuthError, LoginStep, TokenCheck, Tokens};
use super::queue::{self, AccountStatus};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::settings;
use crate::web_api::ApiJson;
use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/garmin", get(status))
        .route("/api/garmin/login", post(login))
        .route("/api/garmin/mfa", post(mfa))
        .route("/api/garmin/logout", post(logout))
        .route("/api/garmin/retry", post(retry))
        .route("/api/garmin/backfill", post(backfill))
        .route("/api/garmin/import-tokens", post(import_tokens))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum GarminState {
    /// Forward is not set up.
    Off,
    /// Forward is on but there are no usable tokens: log in (again).
    NeedsLogin,
    /// A login waits for the MFA code (10 minutes).
    NeedsMfa,
    Connected,
    /// The queue is stopped (Garmin kept refusing the login); retry or log in again.
    Paused,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct GarminStatus {
    pub state: GarminState,
    /// Files waiting for a try (including those waiting out a back-off).
    #[ts(type = "number")]
    pub pending: i64,
    /// Files Garmin has (uploaded, or already there).
    #[ts(type = "number")]
    pub done: i64,
    /// Files that need the owner (Garmin refused them or 10 tries failed): `retry` re-queues them.
    #[ts(type = "number")]
    pub failed: i64,
    pub last_error: Option<String>,
    /// When the next waiting file is tried (RFC 3339); null when nothing waits.
    pub next_attempt_at: Option<String>,
}

#[derive(Serialize, Deserialize, TS)]
pub struct GarminLoginRequest {
    pub email: String,
    pub password: String,
}

impl std::fmt::Debug for GarminLoginRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GarminLoginRequest")
            .field("email", &"<redacted>")
            .field("password", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct GarminLoginResponse {
    /// True: Garmin sent a code; `POST /api/garmin/mfa` finishes the login.
    pub mfa_required: bool,
}

#[derive(Serialize, Deserialize, TS)]
pub struct GarminMfaRequest {
    pub code: String,
}

impl std::fmt::Debug for GarminMfaRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GarminMfaRequest")
            .field("code", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct GarminBackfillRequest {
    /// Queue activities that started within the last this many days (1 to 3650).
    #[ts(type = "number")]
    pub days: i64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct GarminBackfillResponse {
    /// Files newly queued (files already queued or sent are not counted).
    #[ts(type = "number")]
    pub queued: i64,
}

/// The token file python-garminconnect 0.3.20 writes (`garmin_tokens.json`).
#[derive(Serialize, Deserialize, TS)]
pub struct GarminImportTokens {
    pub di_token: String,
    pub di_refresh_token: String,
    /// Optional: read from the access token when missing.
    #[serde(default)]
    pub di_client_id: Option<String>,
}

impl std::fmt::Debug for GarminImportTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GarminImportTokens")
            .field("di_token", &"<redacted>")
            .field("di_refresh_token", &"<redacted>")
            .field("di_client_id", &self.di_client_id)
            .finish()
    }
}

// ---- handlers -----------------------------------------------------------------------

async fn build_status(state: &AppState, uid: i64) -> Result<GarminStatus, ApiError> {
    let now = state.now();
    let account = queue::load_account(&state.pool, uid).await?;
    let counts = queue::counts(&state.pool, uid).await?;
    let tz = settings::load(&state.pool, uid).await?.tz;
    let st = if state.garmin.pending_since(uid, now).is_some() {
        GarminState::NeedsMfa
    } else {
        match account.as_ref().map(|a| a.status) {
            None => GarminState::Off,
            Some(AccountStatus::Connected) => GarminState::Connected,
            Some(AccountStatus::Paused) => GarminState::Paused,
            Some(AccountStatus::NeedsLogin) => GarminState::NeedsLogin,
        }
    };
    let last_error = match account.and_then(|a| a.last_error) {
        Some(e) => Some(e),
        None => queue::last_item_error(&state.pool, uid).await?,
    };
    let next = if account_connected(&st) {
        queue::next_attempt_at(&state.pool, uid).await?
    } else {
        None
    };
    Ok(GarminStatus {
        state: st,
        pending: counts.pending,
        done: counts.done,
        failed: counts.failed,
        last_error,
        next_attempt_at: next.map(|t| settings::rfc3339(tz, t)),
    })
}

fn account_connected(s: &GarminState) -> bool {
    matches!(s, GarminState::Connected)
}

async fn status(
    user: DataUser,
    State(state): State<AppState>,
) -> Result<Json<GarminStatus>, ApiError> {
    Ok(Json(build_status(&state, user.id).await?))
}

fn auth_error(e: AuthError) -> ApiError {
    // Not 401: the web app treats 401 as "your own session ended".
    match e {
        AuthError::InvalidCredentials => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Garmin rejected the email or password",
        ),
        AuthError::InvalidMfaCode => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Garmin rejected the verification code",
        ),
        AuthError::RateLimited(wait) => ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            match wait {
                Some(s) => format!("Garmin is rate limiting this server; try again in {s} seconds"),
                None => "Garmin is rate limiting this server; try again later".into(),
            },
        ),
        AuthError::Blocked(why) => ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!(
                "Garmin refused the login ({why}). It may block this server's connection fingerprint; import a token file from python-garminconnect instead"
            ),
        ),
        AuthError::Rejected => ApiError::new(
            StatusCode::BAD_GATEWAY,
            "Garmin did not accept the login",
        ),
        AuthError::Unavailable(why) => ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("could not reach Garmin ({why})"),
        ),
        AuthError::Unexpected(why) => ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("unexpected answer from Garmin ({why})"),
        ),
    }
}

/// A token set just obtained: prove Garmin accepts it, store it, switch Forward on.
async fn finish_login(state: &AppState, uid: i64, tokens: Tokens) -> Result<(), ApiError> {
    let g = &state.garmin;
    if let Ok(TokenCheck::Unauthorized) = auth::check_token(&g.cfg, g.api().as_ref(), &tokens).await
    {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "Garmin accepted the login but rejected the token it issued",
        ));
    }
    // A failed check for another reason (network, 429) does not undo a login that just worked.
    let _lock = g.token_lock.lock().await;
    queue::save_login(&state.pool, uid, &tokens, state.now()).await?;
    g.clear_pending(uid);
    g.wake();
    tracing::info!(user_id = uid, "garmin forward switched on");
    Ok(())
}

async fn login(
    user: DataUser,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<GarminLoginRequest>,
) -> Result<Json<GarminLoginResponse>, ApiError> {
    let email = body.email.trim();
    if email.is_empty() || email.len() > 254 {
        return Err(ApiError::bad_request("email must have 1 to 254 characters"));
    }
    if body.password.is_empty() || body.password.len() > 256 {
        return Err(ApiError::bad_request(
            "password must have 1 to 256 characters",
        ));
    }
    let g = &state.garmin;
    let _one = g.login_lock.lock().await;
    g.clear_pending(user.id);
    match auth::begin_login(&g.cfg, email, &body.password)
        .await
        .map_err(auth_error)?
    {
        LoginStep::Done(tokens) => {
            drop(body);
            finish_login(&state, user.id, tokens).await?;
            Ok(Json(GarminLoginResponse {
                mfa_required: false,
            }))
        }
        LoginStep::Mfa(p) => {
            g.set_pending(user.id, p, state.now());
            Ok(Json(GarminLoginResponse { mfa_required: true }))
        }
    }
}

async fn mfa(
    user: DataUser,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<GarminMfaRequest>,
) -> Result<Json<GarminStatus>, ApiError> {
    let code = body.code.trim();
    if code.is_empty() || code.len() > 16 {
        return Err(ApiError::bad_request("code must have 1 to 16 characters"));
    }
    let g = &state.garmin;
    let _one = g.login_lock.lock().await;
    let Some(pending) = g.pending(user.id, state.now()) else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "no login is waiting for a code (it expires after 10 minutes); log in again",
        ));
    };
    let tokens = auth::verify_mfa(&g.cfg, &pending, code)
        .await
        .map_err(auth_error)?;
    finish_login(&state, user.id, tokens).await?;
    Ok(Json(build_status(&state, user.id).await?))
}

async fn logout(user: DataUser, State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    let g = &state.garmin;
    let _lock = g.token_lock.lock().await;
    g.clear_pending(user.id);
    queue::delete_account(&state.pool, user.id).await?;
    tracing::info!(user_id = user.id, "garmin forward switched off");
    Ok(StatusCode::NO_CONTENT)
}

async fn retry(
    user: DataUser,
    State(state): State<AppState>,
) -> Result<Json<GarminStatus>, ApiError> {
    match queue::load_account(&state.pool, user.id).await? {
        None => return Err(ApiError::new(StatusCode::CONFLICT, "Garmin is not set up")),
        Some(a) if a.status == AccountStatus::NeedsLogin => {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "the Garmin login expired; log in again",
            ))
        }
        Some(_) => {}
    }
    queue::retry(&state.pool, user.id, state.now()).await?;
    state.garmin.wake();
    Ok(Json(build_status(&state, user.id).await?))
}

async fn backfill(
    user: DataUser,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<GarminBackfillRequest>,
) -> Result<Json<GarminBackfillResponse>, ApiError> {
    if !(1..=3650).contains(&body.days) {
        return Err(ApiError::bad_request("days must be between 1 and 3650"));
    }
    if queue::load_account(&state.pool, user.id).await?.is_none() {
        return Err(ApiError::new(StatusCode::CONFLICT, "Garmin is not set up"));
    }
    let now = state.now();
    let queued = queue::backfill(&state.pool, user.id, now - body.days * 86_400, now).await?;
    state.garmin.wake();
    Ok(Json(GarminBackfillResponse {
        queued: queued as i64,
    }))
}

async fn import_tokens(
    user: DataUser,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<GarminImportTokens>,
) -> Result<Json<GarminStatus>, ApiError> {
    let client_id = body
        .di_client_id
        .clone()
        .filter(|c| !c.is_empty())
        .or_else(|| {
            auth::jwt_claims(&body.di_token)
                .and_then(|c| c.get("client_id")?.as_str().map(str::to_owned))
        })
        .unwrap_or_default();
    let tokens = Tokens {
        di_token: body.di_token.trim().to_owned(),
        di_refresh_token: body.di_refresh_token.trim().to_owned(),
        di_client_id: client_id,
    };
    tokens.validate().map_err(|why| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("not a python-garminconnect token file: {why}"),
        )
    })?;
    let g = &state.garmin;
    let _one = g.login_lock.lock().await;

    // Prove Garmin accepts them; an expired access token gets one refresh.
    let mut tokens = tokens;
    let mut check = auth::check_token(&g.cfg, g.api().as_ref(), &tokens)
        .await
        .map_err(auth_error)?;
    if check == TokenCheck::Unauthorized {
        tokens = auth::refresh(&g.cfg, g.api().as_ref(), &tokens)
            .await
            .map_err(|e| match e {
                AuthError::Rejected => ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "Garmin did not accept these tokens",
                ),
                other => auth_error(other),
            })?;
        check = auth::check_token(&g.cfg, g.api().as_ref(), &tokens)
            .await
            .map_err(auth_error)?;
    }
    if check != TokenCheck::Valid {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Garmin did not accept these tokens",
        ));
    }
    let _lock = g.token_lock.lock().await;
    queue::save_login(&state.pool, user.id, &tokens, state.now()).await?;
    g.clear_pending(user.id);
    g.wake();
    tracing::info!(
        user_id = user.id,
        "garmin forward switched on (tokens imported)"
    );
    drop(_lock);
    Ok(Json(build_status(&state, user.id).await?))
}
