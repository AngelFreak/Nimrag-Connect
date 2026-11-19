//! Owner authentication: argon2id password, cookie sessions, login rate limit,
//! CSRF header. Device tokens (`tokens.rs`) can only upload; every `/api` read
//! goes through the [`Owner`] extractor, which accepts a session cookie only.
//!
//! Nothing here logs passwords, session ids or tokens.

use crate::api::{ApiError, AppState};
use anyhow::{anyhow, Result};
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use axum::{
    extract::{FromRef, FromRequestParts, Request},
    http::{header, request::Parts, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;

pub const COOKIE_NAME: &str = "nimrag_session";
pub const SESSION_SECS: i64 = 30 * 24 * 3600;
pub const MIN_PASSWORD_LEN: usize = 10;
/// Header every state-changing `/api` request must carry (with SameSite=Strict: CSRF guard).
pub const CSRF_HEADER: &str = "x-nimrag";

// ---- password ---------------------------------------------------------------

/// argon2id with the crate's default parameters (19 MiB, 2 passes, 1 lane) and a
/// random 16-byte salt; returns the PHC string.
pub fn hash_password(password: &str) -> Result<String> {
    let hash = Argon2::default()
        .hash_password(password.as_bytes())
        .map_err(|e| anyhow!("hashing failed: {e}"))?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, phc: &str) -> bool {
    Argon2::default()
        .verify_password(password.as_bytes(), phc)
        .is_ok()
}

/// Store (or replace) the owner's password. Existing sessions are dropped.
pub async fn set_password(pool: &SqlitePool, password: &str, now: i64) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        anyhow::bail!("password must have at least {MIN_PASSWORD_LEN} characters");
    }
    let hash = hash_password(password)?;
    sqlx::query(
        "INSERT INTO owner (id, password_hash, updated_at) VALUES (1, ?, ?)
         ON CONFLICT (id) DO UPDATE SET password_hash = excluded.password_hash,
                                        updated_at = excluded.updated_at",
    )
    .bind(hash)
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM sessions").execute(pool).await?;
    Ok(())
}

pub async fn owner_hash(pool: &SqlitePool) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar::<_, String>("SELECT password_hash FROM owner WHERE id = 1")
            .fetch_optional(pool)
            .await?,
    )
}

// ---- sessions ---------------------------------------------------------------

fn session_hash(raw: &str) -> String {
    crate::hex(&Sha256::digest(raw.as_bytes()))
}

/// New session: returns the cookie value (32 random bytes, hex). Only its hash is stored.
pub async fn create_session(pool: &SqlitePool, now: i64) -> Result<String> {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| anyhow!("no randomness: {e}"))?;
    let id = crate::hex(&raw);
    sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
        .bind(now)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO sessions (id_hash, created_at, expires_at) VALUES (?, ?, ?)")
        .bind(session_hash(&id))
        .bind(now)
        .bind(now + SESSION_SECS)
        .execute(pool)
        .await?;
    Ok(id)
}

pub async fn session_valid(pool: &SqlitePool, raw: &str, now: i64) -> Result<bool> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT 1 FROM sessions WHERE id_hash = ? AND expires_at > ?")
            .bind(session_hash(raw))
            .bind(now)
            .fetch_optional(pool)
            .await?
            .is_some(),
    )
}

pub async fn delete_session(pool: &SqlitePool, raw: &str) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id_hash = ?")
        .bind(session_hash(raw))
        .execute(pool)
        .await?;
    Ok(())
}

pub fn session_cookie(parts: &Parts) -> Option<String> {
    cookie_value(
        parts.headers.get(header::COOKIE)?.to_str().ok()?,
        COOKIE_NAME,
    )
}

fn cookie_value(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|kv| {
        let (k, v) = kv.trim().split_once('=')?;
        (k == name && !v.is_empty()).then(|| v.to_owned())
    })
}

/// `Set-Cookie` value for a new session (`id`) or, with `None`, one that clears it.
pub fn set_cookie_header(id: Option<&str>, insecure: bool) -> String {
    let (value, max_age) = match id {
        Some(id) => (id, SESSION_SECS),
        None => ("", 0),
    };
    let secure = if insecure { "" } else { "; Secure" };
    format!("{COOKIE_NAME}={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}{secure}")
}

// ---- login rate limit ---------------------------------------------------------

/// In-memory per-IP backoff: from the 5th consecutive failure on, further attempts
/// are refused for 2, 4, 8 ... seconds (capped at 5 minutes). A success resets the
/// counter. Behind a reverse proxy the peer is the proxy, so the limit then
/// applies to all clients together (fine for one owner).
#[derive(Default)]
pub struct LoginLimiter {
    inner: Mutex<HashMap<IpAddr, (u32, i64)>>,
}

impl LoginLimiter {
    const FREE_ATTEMPTS: u32 = 4;
    const MAX_DELAY: i64 = 300;

    /// `Err(seconds to wait)` while the address is locked out.
    pub fn check(&self, ip: IpAddr, now: i64) -> Result<(), i64> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match map.get(&ip) {
            Some((_, until)) if *until > now => Err(until - now),
            _ => Ok(()),
        }
    }

    pub fn failure(&self, ip: IpAddr, now: i64) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() > 1024 {
            map.retain(|_, (_, until)| *until > now);
        }
        let e = map.entry(ip).or_insert((0, 0));
        e.0 += 1;
        if e.0 > Self::FREE_ATTEMPTS {
            let shift = (e.0 - Self::FREE_ATTEMPTS).min(9);
            e.1 = now + (1i64 << shift).min(Self::MAX_DELAY);
        }
    }

    pub fn success(&self, ip: IpAddr) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ip);
    }
}

// ---- extractor and CSRF guard -----------------------------------------------

/// The signed-in owner (valid session cookie). Device tokens never pass.
pub struct Owner;

impl<S> FromRequestParts<S> for Owner
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        let state = AppState::from_ref(state);
        let ok = match session_cookie(parts) {
            Some(id) => session_valid(&state.pool, &id, state.now()).await?,
            None => false,
        };
        if ok {
            Ok(Owner)
        } else {
            Err(ApiError::new(StatusCode::UNAUTHORIZED, "sign in required"))
        }
    }
}

/// Reject `POST`/`PATCH`/`PUT`/`DELETE` under `/api` without `X-Nimrag: 1`, except
/// the device upload (`/api/files`, Bearer token) and `/api/login`.
pub async fn csrf_guard(req: Request, next: Next) -> Response {
    let changes = matches!(
        *req.method(),
        Method::POST | Method::PATCH | Method::PUT | Method::DELETE
    );
    let path = req.uri().path();
    let exempt = path == "/api/files" || path == "/api/login";
    if changes && path.starts_with("/api/") && !exempt {
        let ok = req
            .headers()
            .get(CSRF_HEADER)
            .is_some_and(|v| v.as_bytes() == b"1");
        if !ok {
            return ApiError::new(StatusCode::FORBIDDEN, "missing X-Nimrag: 1 header")
                .into_response();
        }
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_parsing() {
        let h = "a=b; nimrag_session=abc123; c=d";
        assert_eq!(cookie_value(h, COOKIE_NAME).as_deref(), Some("abc123"));
        assert_eq!(cookie_value("a=b", COOKIE_NAME), None);
    }

    #[test]
    fn password_roundtrip_is_argon2id() {
        let h = hash_password("correct horse battery").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery", &h));
        assert!(!verify_password("wrong", &h));
    }

    #[test]
    fn limiter_backs_off_after_four_failures() {
        let l = LoginLimiter::default();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..4 {
            l.failure(ip, 100);
            assert!(l.check(ip, 100).is_ok());
        }
        l.failure(ip, 100);
        assert_eq!(l.check(ip, 100), Err(2));
        assert!(l.check(ip, 102).is_ok());
        l.failure(ip, 102);
        assert_eq!(l.check(ip, 102), Err(4));
        l.success(ip);
        assert!(l.check(ip, 102).is_ok());
    }

    #[test]
    fn cookie_flags() {
        let c = set_cookie_header(Some("id"), false);
        assert!(c.contains("HttpOnly") && c.contains("SameSite=Strict") && c.contains("Secure"));
        assert!(c.contains("Max-Age=2592000"));
        assert!(!set_cookie_header(Some("id"), true).contains("Secure"));
        assert!(set_cookie_header(None, false).contains("Max-Age=0"));
    }
}
