//! Login, logout, `GET /api/me` and device-token management.

use super::{ApiJson, Ctx};
use crate::api::{ApiError, AppState};
use crate::auth::{self, Owner};
use crate::auth::{ClientIp, Verdict};
use crate::{settings, tokens};
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
    Router::new()
        .route(
            "/api/login",
            post(login).layer(DefaultBodyLimit::max(crate::api::MAX_LOGIN_BYTES)),
        )
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/tokens", get(list_tokens).post(create_token))
        .route("/api/tokens/{id}", delete(revoke_token))
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct Me {
    /// Always `true`: a signed-out request gets 401 instead.
    pub authenticated: bool,
    /// IANA name of the owner's time zone.
    pub timezone: String,
    pub sample: bool,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct LoginRequest {
    pub password: String,
}

fn with_cookie(mut resp: Response, cookie: &str) -> Response {
    if let Ok(v) = HeaderValue::from_str(cookie) {
        resp.headers_mut().append(header::SET_COOKIE, v);
    }
    resp
}

fn too_many_attempts(wait: i64) -> Response {
    let mut resp =
        ApiError::new(StatusCode::TOO_MANY_REQUESTS, "too many attempts").into_response();
    if let Ok(v) = HeaderValue::from_str(&wait.to_string()) {
        resp.headers_mut().insert(header::RETRY_AFTER, v);
    }
    resp
}

/// Maps a failed password check to its response; `None` when the password was right.
fn password_refusal(verdict: Verdict) -> Option<Response> {
    match verdict {
        Verdict::Correct => None,
        Verdict::Wrong => {
            Some(ApiError::new(StatusCode::UNAUTHORIZED, "wrong password").into_response())
        }
        Verdict::Locked(wait) => Some(too_many_attempts(wait)),
        Verdict::NoOwner => Some(
            ApiError::new(
                StatusCode::CONFLICT,
                "no password set; run `nimrag-server user set-password`",
            )
            .into_response(),
        ),
    }
}

async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<LoginRequest>,
) -> Result<Response, ApiError> {
    let verdict = auth::verify_owner_password(&state, ip, body.password).await?;
    if let Some(refusal) = password_refusal(verdict) {
        return Ok(refusal);
    }
    let now = state.now();
    let id = auth::create_session(&state.pool, now).await?;
    let ctx = Ctx::new(&state).await?;
    let me = Me {
        authenticated: true,
        timezone: ctx.tz().name().to_owned(),
        sample: ctx.sample(),
    };
    Ok(with_cookie(
        Json(me).into_response(),
        &auth::set_cookie_header(Some(&id), state.insecure_cookies),
    ))
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

async fn me(_: Owner, State(state): State<AppState>) -> Result<Json<Me>, ApiError> {
    let ctx = Ctx::new(&state).await?;
    Ok(Json(Me {
        authenticated: true,
        timezone: ctx.tz().name().to_owned(),
        sample: ctx.sample(),
    }))
}

// ---- device tokens ------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TokenInfo {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
    pub active: bool,
}

/// Never holds a secret: only the creation response does.
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TokenList {
    pub tokens: Vec<TokenInfo>,
}

/// Creating a token needs the owner's password again (a stolen session must not be
/// enough to mint a long-lived upload credential).
#[derive(Serialize, Deserialize, TS)]
pub struct NewToken {
    pub name: String,
    pub password: String,
}

/// The only time the token itself is returned.
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct NewTokenResponse {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub token: String,
}

async fn list_tokens(_: Owner, State(state): State<AppState>) -> Result<Json<TokenList>, ApiError> {
    let tz = settings::load(&state.pool).await?.tz;
    let tokens = tokens::list(&state.pool)
        .await?
        .into_iter()
        .map(|t| TokenInfo {
            id: t.id,
            name: t.name,
            created_at: settings::stored_rfc3339(tz, &t.created_at),
            last_used_at: t.last_used_at.map(|s| settings::stored_rfc3339(tz, &s)),
            active: t.revoked_at.is_none(),
            revoked_at: t.revoked_at.map(|s| settings::stored_rfc3339(tz, &s)),
        })
        .collect();
    Ok(Json(TokenList { tokens }))
}

async fn create_token(
    _: Owner,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<NewToken>,
) -> Result<Response, ApiError> {
    let name = body.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(ApiError::bad_request("name must have 1 to 64 characters"));
    }
    // Same limiter and one-at-a-time argon2 gate as login; a wrong password leaves the
    // session alone.
    let verdict = auth::verify_owner_password(&state, ip, body.password).await?;
    if matches!(verdict, Verdict::Wrong) {
        // 403, not 401: the session is fine, so the page must not be sent to /login.
        return Err(ApiError::new(StatusCode::FORBIDDEN, "wrong password"));
    }
    if let Some(refusal) = password_refusal(verdict) {
        return Ok(refusal);
    }
    let Some((id, token)) = tokens::create_capped(&state.pool, &name, tokens::MAX_ACTIVE).await?
    else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!(
                "at most {} active device tokens; revoke one first",
                tokens::MAX_ACTIVE
            ),
        ));
    };
    tracing::info!(device_id = id, "device token created");
    Ok((
        StatusCode::CREATED,
        Json(NewTokenResponse { id, name, token }),
    )
        .into_response())
}

async fn revoke_token(
    _: Owner,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    if tokens::revoke(&state.pool, id).await? {
        tracing::info!(device_id = id, "device token revoked");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("no active token with that id"))
    }
}
