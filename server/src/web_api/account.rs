//! Login, logout, `GET /api/me` and device-token management.

use super::{ApiJson, Ctx};
use crate::api::{ApiError, AppState};
use crate::auth::{self, Owner};
use crate::{settings, tokens};
use axum::{
    extract::{ConnectInfo, FromRequest, Path, Request, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use ts_rs::TS;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/login", post(login))
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

async fn login(State(state): State<AppState>, req: Request) -> Result<Response, ApiError> {
    let ip: IpAddr = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |c| c.0.ip());
    let ApiJson(body) = ApiJson::<LoginRequest>::from_request(req, &state).await?;
    let now = state.now();
    if let Err(wait) = state.limiter.check(ip, now) {
        let mut resp =
            ApiError::new(StatusCode::TOO_MANY_REQUESTS, "too many attempts").into_response();
        if let Ok(v) = HeaderValue::from_str(&wait.to_string()) {
            resp.headers_mut().insert(header::RETRY_AFTER, v);
        }
        return Ok(resp);
    }
    let Some(hash) = auth::owner_hash(&state.pool).await? else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "no password set; run `nimrag-server user set-password`",
        ));
    };
    let password = body.password;
    let ok = tokio::task::spawn_blocking(move || auth::verify_password(&password, &hash))
        .await
        .map_err(anyhow::Error::from)?;
    if !ok {
        state.limiter.failure(ip, now);
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "wrong password"));
    }
    state.limiter.success(ip);
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

async fn logout(
    State(state): State<AppState>,
    parts: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    let raw = parts
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| {
            h.split(';').find_map(|kv| {
                let (k, v) = kv.trim().split_once('=')?;
                (k == auth::COOKIE_NAME).then(|| v.to_owned())
            })
        });
    if let Some(raw) = raw {
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

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct NewToken {
    pub name: String,
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
    ApiJson(body): ApiJson<NewToken>,
) -> Result<(StatusCode, Json<NewTokenResponse>), ApiError> {
    let name = body.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(ApiError::bad_request("name must have 1 to 64 characters"));
    }
    let (id, token) = tokens::create(&state.pool, &name).await?;
    tracing::info!(device_id = id, "device token created");
    Ok((
        StatusCode::CREATED,
        Json(NewTokenResponse { id, name, token }),
    ))
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
