//! First-run setup, login (username, password, 2FA code), logout, `GET /api/me`, the signed-in
//! user's own password and 2FA, and device-token management.
//!
//! Login in one request: `POST /api/login {username, password, code}`. A wrong username, a wrong
//! password, a wrong or missing code and a disabled account all answer the same 401 and take
//! about the same time (a dummy argon2 verify stands in for an unknown username). Two steps can
//! sit between a right password and a session, each a short-lived single-use pending token (not
//! a session): enrolling 2FA (a new or reset user: `POST /api/login/totp-enrol`) and changing a
//! temporary password (`POST /api/login/change-password`).
//!
//! First-run setup creates the admin: `POST /api/setup` takes the username and password and
//! returns a 2FA secret to enrol; `POST /api/setup/confirm` takes a code from it and only then
//! creates the account (and so closes setup). Nothing is stored before the confirmation, and the
//! account is inserted only if no user exists, so of two racing setups exactly one wins.

use super::{ApiJson, Ctx};
use crate::api::{ApiError, AppState};
use crate::auth::{self, AuthUser, ClientIp, Cred, DataUser, PendingKind};
use crate::users::{self, Role, User};
use crate::{settings, tokens, totp};
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub fn routes() -> Router<AppState> {
    let small = || DefaultBodyLimit::max(crate::api::MAX_LOGIN_BYTES);
    Router::new()
        .route("/api/login", post(login).layer(small()))
        .route("/api/login/totp-enrol", post(totp_enrol).layer(small()))
        .route(
            "/api/login/change-password",
            post(change_password).layer(small()),
        )
        .route(
            "/api/setup",
            get(setup_status).post(setup_start).layer(small()),
        )
        .route("/api/setup/confirm", post(setup_confirm).layer(small()))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/me/password", post(change_own_password).layer(small()))
        .route("/api/me/2fa", post(two_factor_start).layer(small()))
        .route(
            "/api/me/2fa/confirm",
            post(two_factor_confirm).layer(small()),
        )
        .route("/api/tokens", get(list_tokens))
        .route("/api/tokens/{id}", delete(revoke_token))
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct Me {
    /// Always `true`: a signed-out request gets 401 instead.
    pub authenticated: bool,
    pub username: String,
    /// `admin` sets the server up and manages accounts (and has no data); `user` has the data.
    pub role: Role,
    /// IANA name of the user's time zone.
    pub timezone: String,
    pub sample: bool,
}

#[derive(Serialize, Deserialize, TS)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    /// The 6-digit code from the authenticator app. Not needed (and ignored) while the user has
    /// not enrolled 2FA yet.
    #[serde(default)]
    #[ts(optional)]
    pub code: Option<String>,
}

/// What a right password (and code) leads to.
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum LoginResponse {
    /// Signed in; the session cookie is set.
    Done(Me),
    /// The user has no 2FA yet: show the QR code for `otpauth_uri` (or the `secret`), then send
    /// a code to `POST /api/login/totp-enrol`. With `must_change_password` the new password goes
    /// with it.
    Enrol2fa {
        pending_token: String,
        otpauth_uri: String,
        secret: String,
        must_change_password: bool,
    },
    /// The temporary password must be replaced: `POST /api/login/change-password`.
    ChangePassword { pending_token: String },
}

#[derive(Serialize, Deserialize, TS)]
pub struct EnrolRequest {
    pub pending_token: String,
    pub code: String,
    /// Required when the login response said `must_change_password`.
    #[serde(default)]
    #[ts(optional)]
    pub new_password: Option<String>,
}

#[derive(Serialize, Deserialize, TS)]
pub struct ChangePasswordRequest {
    pub pending_token: String,
    pub new_password: String,
}

/// `GET /api/setup`: whether the server still waits for its first admin.
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct SetupStatus {
    pub needed: bool,
}

/// `POST /api/setup`: the first admin's username and password (only while no user exists).
#[derive(Serialize, Deserialize, TS)]
pub struct SetupStartRequest {
    pub username: String,
    pub password: String,
}

/// The 2FA secret to enrol before the admin exists.
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct SetupStartResponse {
    pub pending_token: String,
    pub otpauth_uri: String,
    pub secret: String,
}

#[derive(Serialize, Deserialize, TS)]
pub struct SetupConfirmRequest {
    pub pending_token: String,
    pub code: String,
}

/// `POST /api/me/2fa`: re-enrolling needs the password again.
#[derive(Serialize, Deserialize, TS)]
pub struct TwoFactorStart {
    pub password: String,
}

#[derive(Serialize, Deserialize, TS)]
pub struct TwoFactorConfirm {
    pub pending_token: String,
    pub code: String,
}

#[derive(Serialize, Deserialize, TS)]
pub struct ChangeOwnPassword {
    pub current_password: String,
    pub new_password: String,
}

// ---- helpers --------------------------------------------------------------------

fn with_cookie(mut resp: Response, cookie: &str) -> Response {
    if let Ok(v) = HeaderValue::from_str(cookie) {
        resp.headers_mut().append(header::SET_COOKIE, v);
    }
    resp
}

pub(crate) fn too_many_attempts(wait: i64) -> Response {
    let mut resp =
        ApiError::new(StatusCode::TOO_MANY_REQUESTS, "too many attempts").into_response();
    if let Ok(v) = HeaderValue::from_str(&wait.to_string()) {
        resp.headers_mut().insert(header::RETRY_AFTER, v);
    }
    resp
}

/// The one answer for every way a login can be wrong.
pub(crate) fn wrong_credentials() -> Response {
    ApiError::new(StatusCode::UNAUTHORIZED, "wrong username, password or code").into_response()
}

pub(crate) async fn me_of(state: &AppState, user: &AuthUser) -> Result<Me, ApiError> {
    let ctx = Ctx::new(state, user.id).await?;
    Ok(Me {
        authenticated: true,
        username: user.username.clone(),
        role: user.role,
        timezone: ctx.tz().name().to_owned(),
        sample: ctx.sample(),
    })
}

/// Start a session for `user` and answer with `body` and the cookie.
pub(crate) async fn start_session(
    state: &AppState,
    user: &User,
    token_id: Option<i64>,
    status: StatusCode,
) -> Result<Response, ApiError> {
    let id = auth::create_session(&state.pool, user.id, token_id, state.now()).await?;
    let me = me_of(
        state,
        &AuthUser {
            id: user.id,
            username: user.username.clone(),
            role: user.role,
        },
    )
    .await?;
    let body = if status == StatusCode::OK {
        Json(LoginResponse::Done(me)).into_response()
    } else {
        (status, Json(LoginResponse::Done(me))).into_response()
    };
    Ok(with_cookie(
        body,
        &auth::set_cookie_header(Some(&id), state.insecure_cookies),
    ))
}

/// Check a 2FA code against the user's stored secret and burn the step (replay-proof).
pub(crate) async fn check_user_code(
    state: &AppState,
    user: &User,
    code: Option<&str>,
) -> Result<bool, ApiError> {
    let (Some(secret), Some(code)) = (user.totp_secret.as_deref(), code) else {
        return Ok(false);
    };
    let Some(step) = totp::check(secret, code, state.now(), user.totp_last_step) else {
        return Ok(false);
    };
    Ok(users::accept_totp_step(&state.pool, user.id, step).await?)
}

fn check_new_password(password: &str) -> Result<(), ApiError> {
    if password.chars().count() < auth::MIN_PASSWORD_LEN {
        return Err(ApiError::bad_request(format!(
            "password must have at least {} characters",
            auth::MIN_PASSWORD_LEN
        )));
    }
    Ok(())
}

/// Hash a new password, one argon2 job at a time like the login checks.
pub(crate) async fn hash_under_gate(
    state: &AppState,
    password: String,
) -> Result<String, ApiError> {
    let _permit = state
        .login_gate
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("login gate closed: {e}"))?;
    Ok(
        tokio::task::spawn_blocking(move || auth::hash_password(&password))
            .await
            .map_err(anyhow::Error::from)??,
    )
}

/// Whether `password` is the user's current password (limiter and gate as for a login).
async fn same_as_current(
    state: &AppState,
    user: &User,
    password: String,
) -> Result<bool, ApiError> {
    let hash = user.password_hash.clone();
    let _permit = state
        .login_gate
        .acquire()
        .await
        .map_err(|e| anyhow::anyhow!("login gate closed: {e}"))?;
    Ok(
        tokio::task::spawn_blocking(move || auth::verify_password(&password, &hash))
            .await
            .map_err(anyhow::Error::from)?,
    )
}

/// Re-check the password of a signed-in user (token creation, password and 2FA changes).
/// Wrong gives 403, not 401: the session is fine, so the page must not be sent to /login.
pub(crate) async fn confirm_password(
    state: &AppState,
    ip: std::net::IpAddr,
    user: &AuthUser,
    password: String,
) -> Result<Option<Response>, ApiError> {
    match auth::verify_login(state, ip, &user.username, password).await? {
        Cred::Ok(_) => {
            auth::record_success(state, ip, &user.username);
            Ok(None)
        }
        Cred::Wrong => Ok(Some(
            ApiError::new(StatusCode::FORBIDDEN, "wrong password").into_response(),
        )),
        Cred::Locked(wait) => Ok(Some(too_many_attempts(wait))),
    }
}

/// A pending-token request is refused while its user is locked out.
fn locked(state: &AppState, ip: std::net::IpAddr, username: &str) -> Option<Response> {
    let now = state.now();
    let a = state.limiter.check(&ip, now).err();
    let b = state
        .username_limiter
        .check(&username.to_ascii_lowercase(), now)
        .err();
    a.max(b).map(too_many_attempts)
}

fn expired() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        "this step has expired; log in again",
    )
}

// ---- login -----------------------------------------------------------------------

async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<LoginRequest>,
) -> Result<Response, ApiError> {
    let user = match auth::verify_login(&state, ip, &body.username, body.password).await? {
        Cred::Ok(u) => *u,
        Cred::Wrong => return Ok(wrong_credentials()),
        Cred::Locked(wait) => return Ok(too_many_attempts(wait)),
    };
    let now = state.now();
    if !user.totp_enabled {
        // Right password, no 2FA yet: enrol one before anything else.
        auth::record_success(&state, ip, &user.username);
        let secret = totp::new_secret()?;
        let token = state.pending.insert(
            PendingKind::Enrol {
                user_id: user.id,
                username: user.username.clone(),
            },
            secret.clone(),
            now,
        )?;
        return Ok(Json(LoginResponse::Enrol2fa {
            pending_token: token,
            otpauth_uri: totp::otpauth_uri(&secret, &user.username),
            secret,
            must_change_password: user.must_change_password,
        })
        .into_response());
    }
    // A missing code counts as a failure like a wrong one: otherwise "no lock-out after many
    // tries without a code" would tell an attacker the password was right.
    if !check_user_code(&state, &user, body.code.as_deref()).await? {
        auth::record_failure(&state, ip, &user.username);
        return Ok(wrong_credentials());
    }
    auth::record_success(&state, ip, &user.username);
    if user.must_change_password {
        let token = state.pending.insert(
            PendingKind::ChangePassword {
                user_id: user.id,
                username: user.username.clone(),
            },
            String::new(),
            now,
        )?;
        return Ok(Json(LoginResponse::ChangePassword {
            pending_token: token,
        })
        .into_response());
    }
    start_session(&state, &user, None, StatusCode::OK).await
}

async fn totp_enrol(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<EnrolRequest>,
) -> Result<Response, ApiError> {
    let now = state.now();
    let pending = state
        .pending
        .get(&body.pending_token, now)
        .ok_or_else(expired)?;
    let PendingKind::Enrol { user_id, username } = pending.kind else {
        return Err(expired());
    };
    if let Some(r) = locked(&state, ip, &username) {
        return Ok(r);
    }
    let user = users::by_id(&state.pool, user_id)
        .await?
        .filter(|u| u.disabled_at.is_none() && !u.totp_enabled)
        .ok_or_else(expired)?;
    let new_hash = if user.must_change_password {
        let pw = body
            .new_password
            .clone()
            .ok_or_else(|| ApiError::bad_request("choose a new password"))?;
        check_new_password(&pw)?;
        if same_as_current(&state, &user, pw.clone()).await? {
            return Err(ApiError::bad_request(
                "choose a password different from the temporary one",
            ));
        }
        Some(hash_under_gate(&state, pw).await?)
    } else {
        None
    };
    let Some(step) = totp::check(&pending.secret, &body.code, now, 0) else {
        auth::record_failure(&state, ip, &username);
        let alive = state.pending.failed_attempt(&body.pending_token);
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            if alive {
                "wrong code"
            } else {
                "too many wrong codes; log in again"
            },
        ));
    };
    if !state.pending.take(&body.pending_token) {
        return Err(expired());
    }
    users::set_totp(&state.pool, user.id, &pending.secret, step).await?;
    if let Some(hash) = new_hash {
        users::set_password(&state.pool, user.id, &hash, false).await?;
    }
    auth::record_success(&state, ip, &username);
    tracing::info!(user_id = user.id, "2FA enrolled");
    start_session(&state, &user, None, StatusCode::OK).await
}

async fn change_password(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<ChangePasswordRequest>,
) -> Result<Response, ApiError> {
    let now = state.now();
    let pending = state
        .pending
        .get(&body.pending_token, now)
        .ok_or_else(expired)?;
    let PendingKind::ChangePassword { user_id, username } = pending.kind else {
        return Err(expired());
    };
    if let Some(r) = locked(&state, ip, &username) {
        return Ok(r);
    }
    let user = users::by_id(&state.pool, user_id)
        .await?
        .filter(|u| u.disabled_at.is_none() && u.must_change_password)
        .ok_or_else(expired)?;
    check_new_password(&body.new_password)?;
    if same_as_current(&state, &user, body.new_password.clone()).await? {
        return Err(ApiError::bad_request(
            "choose a password different from the temporary one",
        ));
    }
    let hash = hash_under_gate(&state, body.new_password).await?;
    if !state.pending.take(&body.pending_token) {
        return Err(expired());
    }
    users::set_password(&state.pool, user.id, &hash, false).await?;
    tracing::info!(user_id = user.id, "temporary password replaced");
    start_session(&state, &user, None, StatusCode::OK).await
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(raw) = auth::session_cookie(&headers, state.insecure_cookies) {
        auth::delete_session(&state.pool, &raw).await?;
    }
    Ok(with_cookie(
        StatusCode::NO_CONTENT.into_response(),
        &auth::set_cookie_header(None, state.insecure_cookies),
    ))
}

async fn me(user: AuthUser, State(state): State<AppState>) -> Result<Json<Me>, ApiError> {
    Ok(Json(me_of(&state, &user).await?))
}

// ---- first-run setup -------------------------------------------------------------

async fn setup_status(State(state): State<AppState>) -> Result<Json<SetupStatus>, ApiError> {
    Ok(Json(SetupStatus {
        needed: users::count(&state.pool).await? == 0,
    }))
}

fn already_set_up() -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "this server is already set up")
}

/// Step one of the first-run setup: validate, hash the password, hand out a 2FA secret. Open to
/// anyone who can reach the server, but only while no user exists. Creates nothing yet.
async fn setup_start(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<SetupStartRequest>,
) -> Result<Json<SetupStartResponse>, ApiError> {
    if users::count(&state.pool).await? > 0 {
        return Err(already_set_up());
    }
    let username = users::normalise_username(&body.username).map_err(ApiError::bad_request)?;
    check_new_password(&body.password)?;
    let hash = hash_under_gate(&state, body.password).await?;
    let secret = totp::new_secret()?;
    let token = state.pending.insert(
        PendingKind::Setup {
            username: username.clone(),
            password_hash: hash,
        },
        secret.clone(),
        state.now(),
    )?;
    Ok(Json(SetupStartResponse {
        pending_token: token,
        otpauth_uri: totp::otpauth_uri(&secret, &username),
        secret,
    }))
}

/// Step two: a code from the new secret proves the authenticator works; only now is the admin
/// created (if still nobody exists) and signed in.
async fn setup_confirm(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<SetupConfirmRequest>,
) -> Result<Response, ApiError> {
    let now = state.now();
    if users::count(&state.pool).await? > 0 {
        return Err(already_set_up());
    }
    let pending = state
        .pending
        .get(&body.pending_token, now)
        .ok_or_else(expired)?;
    let PendingKind::Setup {
        username,
        password_hash,
    } = pending.kind
    else {
        return Err(expired());
    };
    if let Some(wait) = state.limiter.check(&ip, now).err() {
        return Ok(too_many_attempts(wait));
    }
    let Some(step) = totp::check(&pending.secret, &body.code, now, 0) else {
        state.limiter.failure(&ip, now);
        let alive = state.pending.failed_attempt(&body.pending_token);
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            if alive {
                "wrong code"
            } else {
                "too many wrong codes; start the setup again"
            },
        ));
    };
    if !state.pending.take(&body.pending_token) {
        return Err(expired());
    }
    if !users::claim_first_admin(
        &state.pool,
        &username,
        &password_hash,
        &pending.secret,
        step,
        now,
    )
    .await?
    {
        return Err(already_set_up());
    }
    tracing::info!("admin created through first-run setup");
    let user = users::by_username(&state.pool, &username)
        .await?
        .ok_or_else(|| anyhow::anyhow!("admin vanished after setup"))?;
    start_session(&state, &user, None, StatusCode::CREATED).await
}

// ---- the signed-in user's own password and 2FA -------------------------------------

async fn change_own_password(
    user: AuthUser,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<ChangeOwnPassword>,
) -> Result<Response, ApiError> {
    check_new_password(&body.new_password)?;
    if let Some(refusal) = confirm_password(&state, ip, &user, body.current_password).await? {
        return Ok(refusal);
    }
    let hash = hash_under_gate(&state, body.new_password).await?;
    users::set_password(&state.pool, user.id, &hash, false).await?;
    tracing::info!(user_id = user.id, "password changed");
    // Every session ended with the change, this one included: start a fresh one.
    let row = users::by_id(&state.pool, user.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("user vanished"))?;
    let resp = start_session(&state, &row, None, StatusCode::OK).await?;
    Ok(resp)
}

async fn two_factor_start(
    user: AuthUser,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<TwoFactorStart>,
) -> Result<Response, ApiError> {
    if let Some(refusal) = confirm_password(&state, ip, &user, body.password).await? {
        return Ok(refusal);
    }
    let secret = totp::new_secret()?;
    let token = state.pending.insert(
        PendingKind::Reenrol {
            user_id: user.id,
            username: user.username.clone(),
        },
        secret.clone(),
        state.now(),
    )?;
    Ok(Json(SetupStartResponse {
        pending_token: token,
        otpauth_uri: totp::otpauth_uri(&secret, &user.username),
        secret,
    })
    .into_response())
}

async fn two_factor_confirm(
    user: AuthUser,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<TwoFactorConfirm>,
) -> Result<Response, ApiError> {
    let now = state.now();
    let pending = state
        .pending
        .get(&body.pending_token, now)
        .ok_or_else(expired)?;
    let PendingKind::Reenrol { user_id, .. } = pending.kind else {
        return Err(expired());
    };
    if user_id != user.id {
        return Err(expired());
    }
    if let Some(r) = locked(&state, ip, &user.username) {
        return Ok(r);
    }
    let Some(step) = totp::check(&pending.secret, &body.code, now, 0) else {
        auth::record_failure(&state, ip, &user.username);
        let alive = state.pending.failed_attempt(&body.pending_token);
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            if alive {
                "wrong code"
            } else {
                "too many wrong codes; start again"
            },
        ));
    };
    if !state.pending.take(&body.pending_token) {
        return Err(expired());
    }
    users::set_totp(&state.pool, user.id, &pending.secret, step).await?;
    auth::record_success(&state, ip, &user.username);
    tracing::info!(user_id = user.id, "2FA re-enrolled");
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---- device tokens ------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TokenInfo {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
    pub active: bool,
    /// `true` for the phone this request's session was started by (the Android app's web view);
    /// always `false` in a browser, which has no phone behind it.
    pub current: bool,
}

/// The signed-in user's phones and tokens. Never holds a secret: a token value is shown only to
/// the app that logs in, once.
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TokenList {
    pub tokens: Vec<TokenInfo>,
}

pub(crate) fn token_info(tz: chrono_tz::Tz, t: tokens::TokenRow) -> TokenInfo {
    TokenInfo {
        id: t.id,
        name: t.name,
        created_at: settings::stored_rfc3339(tz, &t.created_at),
        last_used_at: t.last_used_at.map(|s| settings::stored_rfc3339(tz, &s)),
        active: t.revoked_at.is_none(),
        revoked_at: t.revoked_at.map(|s| settings::stored_rfc3339(tz, &s)),
        current: false,
    }
}

async fn list_tokens(
    user: DataUser,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<TokenList>, ApiError> {
    let tz = settings::load(&state.pool, user.id).await?.tz;
    let current = match auth::session_cookie(&headers, state.insecure_cookies) {
        Some(raw) => auth::session_token_id(&state.pool, &raw, state.now()).await?,
        None => None,
    };
    let tokens = tokens::list(&state.pool, tokens::Whose::User(user.id))
        .await?
        .into_iter()
        .map(|t| {
            let is_current = current == Some(t.id);
            TokenInfo {
                current: is_current,
                ..token_info(tz, t)
            }
        })
        .collect();
    Ok(Json(TokenList { tokens }))
}

async fn revoke_token(
    user: DataUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    if tokens::revoke(&state.pool, id, Some(user.id)).await? {
        tracing::info!(device_id = id, "device token revoked");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("no active token with that id"))
    }
}
