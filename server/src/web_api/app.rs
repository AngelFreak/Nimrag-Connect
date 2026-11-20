//! Sign-in for the Android app (built later; the API is fixed here).
//!
//! * `POST /api/app/login {username, password, code, device_name}`: the first time on a phone,
//!   username, password and 2FA code like the web login. Answers with a session cookie and
//!   creates a device token for this phone (upload credential) plus a `device_secret`, a
//!   trusted-device credential. Both are returned once; only their SHA-256 is stored.
//! * `POST /api/app/unlock {username, password, device_secret}`: later, on the same phone, a
//!   password alone starts a session (the device secret stands in for the 2FA code).
//! * `POST /api/app/resume {device_secret}`: the fingerprint path. The app keeps `device_secret`
//!   in the Android Keystore behind a BiometricPrompt; the server never sees a fingerprint and
//!   cannot check one, so the gate is on the phone, and the secret itself (256 random bits) is
//!   the bearer credential. Anyone who holds it can resume, which is why it lives behind the
//!   Keystore and dies with the device token.
//!
//! Revoking the device token (web Settings) also revokes its device secret and ends the
//! sessions it started. Disabling the user, resetting their password or 2FA does the same for
//! every device. Admin accounts have no data and cannot use the app.

use super::account::{check_user_code, me_of, too_many_attempts, wrong_credentials, Me};
use super::ApiJson;
use crate::api::{ApiError, AppState};
use crate::auth::{self, AuthUser, ClientIp, Cred};
use crate::tokens;
use crate::users::{self, Role};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub fn routes() -> Router<AppState> {
    let small = || DefaultBodyLimit::max(crate::api::MAX_LOGIN_BYTES);
    Router::new()
        .route("/api/app/login", post(app_login).layer(small()))
        .route("/api/app/unlock", post(app_unlock).layer(small()))
        .route("/api/app/resume", post(app_resume).layer(small()))
}

#[derive(Serialize, Deserialize, TS)]
pub struct AppLoginRequest {
    pub username: String,
    pub password: String,
    pub code: String,
    /// What the phone is called in Settings, for example "Pixel 9a" (1 to 64 characters).
    pub device_name: String,
}

/// Everything the app stores after its first login. `token` and `device_secret` are shown only
/// here.
#[derive(Serialize, Deserialize, TS)]
pub struct AppLoginResponse {
    pub me: Me,
    #[ts(type = "number")]
    pub device_id: i64,
    /// Upload credential (`Authorization: Bearer`), as for a token made in the web UI.
    pub token: String,
    /// Trusted-device credential for `/api/app/unlock` and `/api/app/resume`.
    pub device_secret: String,
}

#[derive(Serialize, Deserialize, TS)]
pub struct AppUnlockRequest {
    pub username: String,
    pub password: String,
    pub device_secret: String,
}

#[derive(Serialize, Deserialize, TS)]
pub struct AppResumeRequest {
    pub device_secret: String,
}

fn with_cookie(mut resp: Response, cookie: &str) -> Response {
    if let Ok(v) = HeaderValue::from_str(cookie) {
        resp.headers_mut().append(header::SET_COOKIE, v);
    }
    resp
}

/// A session for the app: the cookie, tied to the device token.
async fn session(
    state: &AppState,
    user: &users::User,
    token_id: i64,
) -> Result<(Me, String), ApiError> {
    let id = auth::create_session(&state.pool, user.id, Some(token_id), state.now()).await?;
    let me = me_of(
        state,
        &AuthUser {
            id: user.id,
            username: user.username.clone(),
            role: user.role,
        },
    )
    .await?;
    Ok((
        me,
        auth::set_cookie_header(Some(&id), state.insecure_cookies),
    ))
}

async fn app_login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<AppLoginRequest>,
) -> Result<Response, ApiError> {
    let name = body.device_name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(ApiError::bad_request(
            "device_name must have 1 to 64 characters",
        ));
    }
    let user = match auth::verify_login(&state, ip, &body.username, body.password).await? {
        Cred::Ok(u) => *u,
        Cred::Wrong => return Ok(wrong_credentials()),
        Cred::Locked(wait) => return Ok(too_many_attempts(wait)),
    };
    if !user.totp_enabled || !check_user_code(&state, &user, Some(&body.code)).await? {
        // No 2FA yet counts as a wrong code here: the app cannot enrol, the web app does.
        auth::record_failure(&state, ip, &user.username);
        return Ok(wrong_credentials());
    }
    auth::record_success(&state, ip, &user.username);
    if user.role == Role::Admin {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "admin accounts have no data and cannot use the app",
        ));
    }
    if user.must_change_password {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "change your temporary password in the web app first",
        ));
    }
    let Some(created) =
        tokens::create_capped(&state.pool, &name, user.id, tokens::MAX_ACTIVE, true).await?
    else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!(
                "at most {} active device tokens; revoke one in Settings first",
                tokens::MAX_ACTIVE
            ),
        ));
    };
    tracing::info!(
        device_id = created.id,
        user_id = user.id,
        "app login: device registered"
    );
    let (me, cookie) = session(&state, &user, created.id).await?;
    let resp = (
        StatusCode::CREATED,
        Json(AppLoginResponse {
            me,
            device_id: created.id,
            token: created.token,
            device_secret: created.device_secret.unwrap_or_default(),
        }),
    )
        .into_response();
    Ok(with_cookie(resp, &cookie))
}

async fn app_unlock(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<AppUnlockRequest>,
) -> Result<Response, ApiError> {
    let user = match auth::verify_login(&state, ip, &body.username, body.password).await? {
        Cred::Ok(u) => *u,
        Cred::Wrong => return Ok(wrong_credentials()),
        Cred::Locked(wait) => return Ok(too_many_attempts(wait)),
    };
    let device = tokens::authenticate_device_secret(&state.pool, &body.device_secret).await?;
    let Some(device) = device.filter(|d| d.user_id == user.id && user.role == Role::User) else {
        auth::record_failure(&state, ip, &user.username);
        return Ok(wrong_credentials());
    };
    auth::record_success(&state, ip, &user.username);
    let (me, cookie) = session(&state, &user, device.token_id).await?;
    Ok(with_cookie(Json(me).into_response(), &cookie))
}

async fn app_resume(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<AppResumeRequest>,
) -> Result<Response, ApiError> {
    let now = state.now();
    if let Err(wait) = state.limiter.check(&ip, now) {
        return Ok(too_many_attempts(wait));
    }
    let device = tokens::authenticate_device_secret(&state.pool, &body.device_secret).await?;
    let user = match &device {
        Some(d) => users::by_id(&state.pool, d.user_id).await?,
        None => None,
    };
    let (Some(device), Some(user)) = (device, user) else {
        state.limiter.failure(&ip, now);
        return Ok(wrong_credentials());
    };
    if user.role != Role::User || user.disabled_at.is_some() {
        state.limiter.failure(&ip, now);
        return Ok(wrong_credentials());
    }
    state.limiter.success(&ip);
    let (me, cookie) = session(&state, &user, device.token_id).await?;
    Ok(with_cookie(Json(me).into_response(), &cookie))
}
