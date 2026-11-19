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
    extract::{ConnectInfo, FromRef, FromRequestParts, Request},
    http::{header, request::Parts, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Mutex;

/// Session cookie name with `--insecure-cookies` (plain http). `__Host-` cookies
/// must be `Secure`, so that prefix is only valid on the default, secure setup.
pub const COOKIE_NAME: &str = "nimrag_session";
/// Session cookie name when `Secure` is on: the `__Host-` prefix makes browsers
/// refuse a cookie set by a sibling subdomain or without `Secure`/`Path=/`.
pub const SECURE_COOKIE_NAME: &str = "__Host-nimrag_session";
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

/// The cookie name in use: only this exact name is accepted, never the other one.
pub fn cookie_name(insecure: bool) -> &'static str {
    if insecure {
        COOKIE_NAME
    } else {
        SECURE_COOKIE_NAME
    }
}

pub fn session_cookie(headers: &HeaderMap, insecure: bool) -> Option<String> {
    cookie_value(
        headers.get(header::COOKIE)?.to_str().ok()?,
        cookie_name(insecure),
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
    let name = cookie_name(insecure);
    format!("{name}={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}{secure}")
}

// ---- client address and trusted proxies ---------------------------------------

/// An IP network (`10.0.0.0/8`, `fd00::/8`; a bare address is a host route).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    addr: IpAddr,
    prefix: u8,
}

impl FromStr for Cidr {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let s = s.trim();
        let (ip, prefix) = match s.split_once('/') {
            Some((ip, p)) => (ip, Some(p)),
            None => (s, None),
        };
        let addr: IpAddr = ip
            .parse()
            .map_err(|_| anyhow!("{s:?} is not an IP address or CIDR"))?;
        let addr = addr.to_canonical();
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(p) => p
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= max)
                .ok_or_else(|| anyhow!("{s:?}: prefix length must be 0 to {max}"))?,
            None => max,
        };
        Ok(Self { addr, prefix })
    }
}

impl Cidr {
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip.to_canonical()) {
            (IpAddr::V4(n), IpAddr::V4(i)) => same_prefix(
                u128::from(u32::from(n)),
                u128::from(u32::from(i)),
                self.prefix,
                32,
            ),
            (IpAddr::V6(n), IpAddr::V6(i)) => {
                same_prefix(u128::from(n), u128::from(i), self.prefix, 128)
            }
            _ => false,
        }
    }
}

fn same_prefix(a: u128, b: u128, prefix: u8, bits: u32) -> bool {
    let shift = bits - u32::from(prefix);
    shift >= 128 || (a >> shift) == (b >> shift)
}

/// Comma-separated list of CIDRs (the `--trusted-proxy` value).
pub fn parse_cidr_list(s: &str) -> Result<Vec<Cidr>> {
    s.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::parse)
        .collect()
}

/// The address login attempts are counted against. Only when the TCP `peer` is
/// inside a trusted proxy network is `X-Forwarded-For` read, and then the
/// rightmost entry that is not itself trusted wins (everything to its left is
/// client-controlled). Otherwise, or if the header is absent, unparsable or
/// lists only trusted addresses, it is the peer.
pub fn client_ip(trusted: &[Cidr], peer: IpAddr, headers: &HeaderMap) -> IpAddr {
    let peer = peer.to_canonical();
    if !trusted.iter().any(|c| c.contains(peer)) {
        return peer;
    }
    let joined = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect::<Vec<_>>()
        .join(",");
    for entry in joined.rsplit(',') {
        let entry = entry.trim();
        let Some(ip) = entry
            .parse::<IpAddr>()
            .ok()
            .or_else(|| entry.parse::<SocketAddr>().ok().map(|a| a.ip()))
        else {
            return peer; // garbage from a trusted hop: do not guess
        };
        let ip = ip.to_canonical();
        if !trusted.iter().any(|c| c.contains(ip)) {
            return ip;
        }
    }
    peer
}

/// The client address as the login limiter sees it (see [`client_ip`]). Without
/// connection info (tests driving the router directly) it is `0.0.0.0`.
pub struct ClientIp(pub IpAddr);

impl<S> FromRequestParts<S> for ClientIp
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = AppState::from_ref(state);
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |c| c.0.ip());
        Ok(Self(client_ip(
            &state.trusted_proxies,
            peer,
            &parts.headers,
        )))
    }
}

// ---- login rate limit ---------------------------------------------------------

/// In-memory login throttle, keyed by client address ([`client_ip`]):
/// - per key: from the 5th consecutive failure on, further attempts are refused
///   for 2, 4, 8 ... seconds (capped at 5 minutes). A success resets the key, and
///   so does 15 minutes without a failure.
/// - global: more than 20 failures within a minute, across all keys, makes every
///   attempt wait 1 s after each further failure (fixed) until the rate drops
///   again, so rotating source addresses does not buy more guesses.
///
/// Without `--trusted-proxy`, behind a reverse proxy the key is the proxy's
/// address and the limit is shared by all clients.
#[derive(Default)]
pub struct LoginLimiter {
    inner: Mutex<LimiterState>,
}

#[derive(Default)]
struct LimiterState {
    /// Per key: (consecutive failures, locked until, time of last failure).
    keys: HashMap<IpAddr, (u32, i64, i64)>,
    /// Times of failures in the last minute, all keys.
    recent: VecDeque<i64>,
    global_until: i64,
}

impl LoginLimiter {
    const FREE_ATTEMPTS: u32 = 4;
    const MAX_DELAY: i64 = 300;
    /// A key's failure counter is forgotten after this long without a failure.
    pub const DECAY_SECS: i64 = 15 * 60;
    const GLOBAL_WINDOW: i64 = 60;
    const GLOBAL_MAX_PER_WINDOW: usize = 20;
    const GLOBAL_DELAY: i64 = 1;

    fn state(&self) -> std::sync::MutexGuard<'_, LimiterState> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `Err(seconds to wait)` while the address (or everyone) is locked out.
    pub fn check(&self, ip: IpAddr, now: i64) -> Result<(), i64> {
        let st = self.state();
        let key_until = st.keys.get(&ip).map_or(0, |(_, until, _)| *until);
        let until = key_until.max(st.global_until);
        if until > now {
            Err(until - now)
        } else {
            Ok(())
        }
    }

    pub fn failure(&self, ip: IpAddr, now: i64) {
        let mut st = self.state();
        if st.keys.len() > 1024 {
            st.keys
                .retain(|_, (_, _, last)| now - *last < Self::DECAY_SECS);
        }
        let e = st.keys.entry(ip).or_insert((0, 0, now));
        if now - e.2 >= Self::DECAY_SECS {
            *e = (0, 0, now);
        }
        e.0 += 1;
        e.2 = now;
        if e.0 > Self::FREE_ATTEMPTS {
            let shift = (e.0 - Self::FREE_ATTEMPTS).min(9);
            e.1 = now + (1i64 << shift).min(Self::MAX_DELAY);
        }
        st.recent.push_back(now);
        while st
            .recent
            .front()
            .is_some_and(|t| now - *t >= Self::GLOBAL_WINDOW)
        {
            st.recent.pop_front();
        }
        if st.recent.len() > Self::GLOBAL_MAX_PER_WINDOW {
            st.global_until = now + Self::GLOBAL_DELAY;
        }
    }

    pub fn success(&self, ip: IpAddr) {
        self.state().keys.remove(&ip);
    }
}

/// Outcome of [`verify_owner_password`].
pub enum Verdict {
    Correct,
    Wrong,
    /// Locked out; seconds to wait.
    Locked(i64),
    /// No owner password has been set.
    NoOwner,
}

/// Check `password` against the owner hash under the login limiter. Verification
/// is serialised (one argon2 job at a time, 19 MiB each) and the limiter is
/// checked again once the turn comes, so a burst of parallel guesses cannot all
/// pass the check before the first failure is recorded. The work runs in its own
/// task: a client that disconnects mid-request still gets its failure counted.
pub async fn verify_owner_password(
    state: &AppState,
    ip: IpAddr,
    password: String,
) -> Result<Verdict> {
    let state = state.clone();
    tokio::spawn(async move {
        // Cheap early refusal, so locked-out clients do not queue for a permit.
        if let Err(wait) = state.limiter.check(ip, state.now()) {
            return Ok(Verdict::Locked(wait));
        }
        let _permit = state
            .login_gate
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| anyhow!("login gate closed: {e}"))?;
        if let Err(wait) = state.limiter.check(ip, state.now()) {
            return Ok(Verdict::Locked(wait));
        }
        let Some(hash) = owner_hash(&state.pool).await? else {
            return Ok(Verdict::NoOwner);
        };
        let ok = tokio::task::spawn_blocking(move || verify_password(&password, &hash)).await?;
        if ok {
            state.limiter.success(ip);
            Ok(Verdict::Correct)
        } else {
            state.limiter.failure(ip, state.now());
            Ok(Verdict::Wrong)
        }
    })
    .await?
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
        let ok = match session_cookie(&parts.headers, state.insecure_cookies) {
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

/// Responses under `/api` hold personal data tied to the session cookie: never
/// stored by caches, no content sniffing, and cache keys vary by cookie.
pub async fn api_headers(req: Request, next: Next) -> Response {
    let is_api = req.uri().path().starts_with("/api/");
    let mut resp = next.run(req).await;
    if is_api {
        let h = resp.headers_mut();
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        h.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        h.append(header::VARY, HeaderValue::from_static("Cookie"));
    }
    resp
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
    fn cookie_flags_and_names() {
        let c = set_cookie_header(Some("id"), false);
        assert!(c.starts_with("__Host-nimrag_session=id;"));
        assert!(c.contains("HttpOnly") && c.contains("SameSite=Strict") && c.contains("Secure"));
        assert!(c.contains("Path=/") && !c.contains("Domain"));
        assert!(c.contains("Max-Age=2592000"));
        let plain = set_cookie_header(Some("id"), true);
        assert!(plain.starts_with("nimrag_session=id;") && !plain.contains("Secure"));
        assert!(set_cookie_header(None, false).contains("Max-Age=0"));
    }

    #[test]
    fn only_the_matching_cookie_name_is_accepted() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            "nimrag_session=plain; __Host-nimrag_session=host"
                .parse()
                .unwrap(),
        );
        assert_eq!(session_cookie(&h, false).as_deref(), Some("host"));
        assert_eq!(session_cookie(&h, true).as_deref(), Some("plain"));
        h.insert(header::COOKIE, "nimrag_session=plain".parse().unwrap());
        assert_eq!(session_cookie(&h, false), None);
        h.insert(
            header::COOKIE,
            "__Host-nimrag_session=host".parse().unwrap(),
        );
        assert_eq!(session_cookie(&h, true), None);
    }

    #[test]
    fn counter_decays_after_fifteen_minutes_without_failure() {
        let l = LoginLimiter::default();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        for i in 0..4 {
            l.failure(ip, 1000 + i);
        }
        // Slow guessing never locks out: every failure is 15 min after the last.
        let mut t = 1003;
        for _ in 0..10 {
            t += LoginLimiter::DECAY_SECS;
            l.failure(ip, t);
            assert!(l.check(ip, t).is_ok());
        }
        // Without the decay, four quick failures plus one more would lock.
        for i in 1..=3 {
            l.failure(ip, t + i);
            assert!(l.check(ip, t + i).is_ok());
        }
        l.failure(ip, t + 4);
        assert_eq!(l.check(ip, t + 4), Err(2));
        // Just under 15 minutes after the last failure the counter is still live.
        let last = t + 4;
        l.failure(ip, last + LoginLimiter::DECAY_SECS - 1);
        assert!(l.check(ip, last + LoginLimiter::DECAY_SECS - 1).is_err());
    }

    #[test]
    fn global_ceiling_delays_everyone_by_one_second() {
        let l = LoginLimiter::default();
        let other: IpAddr = "192.0.2.1".parse().unwrap();
        // 20 failures in a minute from 20 different addresses: still free.
        for i in 0..20u8 {
            l.failure(IpAddr::V4(Ipv4Addr::new(10, 0, 0, i)), 5000);
        }
        assert!(l.check(other, 5000).is_ok());
        // The 21st trips the ceiling: every address waits 1 s, fixed.
        l.failure(IpAddr::V4(Ipv4Addr::new(10, 0, 1, 0)), 5000);
        assert_eq!(l.check(other, 5000), Err(1));
        assert!(l.check(other, 5001).is_ok());
        l.failure(IpAddr::V4(Ipv4Addr::new(10, 0, 1, 1)), 5001);
        assert_eq!(l.check(other, 5001), Err(1)); // no doubling
                                                  // A minute later the window is empty again.
        assert!(l.check(other, 5100).is_ok());
        l.failure(other, 5100);
        assert!(l.check(other, 5100).is_ok());
    }

    #[test]
    fn cidr_parsing_and_matching() {
        let c: Cidr = "10.1.0.0/16".parse().unwrap();
        assert!(c.contains("10.1.200.3".parse().unwrap()));
        assert!(!c.contains("10.2.0.1".parse().unwrap()));
        assert!(!c.contains("::1".parse().unwrap()));
        assert!("10.0.0.1"
            .parse::<Cidr>()
            .unwrap()
            .contains("10.0.0.1".parse().unwrap()));
        assert!(!"10.0.0.1"
            .parse::<Cidr>()
            .unwrap()
            .contains("10.0.0.2".parse().unwrap()));
        assert!("0.0.0.0/0"
            .parse::<Cidr>()
            .unwrap()
            .contains("8.8.8.8".parse().unwrap()));
        assert!("fd00::/8"
            .parse::<Cidr>()
            .unwrap()
            .contains("fd12::1".parse().unwrap()));
        // IPv4-mapped peers match their IPv4 network.
        assert!(c.contains("::ffff:10.1.0.9".parse().unwrap()));
        assert!("10.0.0.0/33".parse::<Cidr>().is_err());
        assert!("nonsense".parse::<Cidr>().is_err());
        assert_eq!(
            parse_cidr_list("10.0.0.0/8, 192.168.0.0/16").unwrap().len(),
            2
        );
        assert!(parse_cidr_list("10.0.0.0/8,oops").is_err());
    }

    #[test]
    fn forwarded_for_is_used_only_from_trusted_peers() {
        let trusted = parse_cidr_list("10.0.0.0/8").unwrap();
        let xff = |v: &str| {
            let mut h = HeaderMap::new();
            h.insert("x-forwarded-for", v.parse().unwrap());
            h
        };
        let ip = |s: &str| -> IpAddr { s.parse().unwrap() };
        // Untrusted peer: header ignored, however it is spoofed.
        assert_eq!(
            client_ip(&trusted, ip("203.0.113.9"), &xff("198.51.100.1")),
            ip("203.0.113.9")
        );
        assert_eq!(
            client_ip(&[], ip("10.0.0.2"), &xff("198.51.100.1")),
            ip("10.0.0.2")
        );
        // Trusted peer: rightmost untrusted entry; the client-supplied left side is ignored.
        assert_eq!(
            client_ip(&trusted, ip("10.0.0.2"), &xff("198.51.100.1")),
            ip("198.51.100.1")
        );
        assert_eq!(
            client_ip(
                &trusted,
                ip("10.0.0.2"),
                &xff("1.2.3.4, 198.51.100.1, 10.0.0.7")
            ),
            ip("198.51.100.1")
        );
        // Nothing usable: the peer.
        assert_eq!(
            client_ip(&trusted, ip("10.0.0.2"), &HeaderMap::new()),
            ip("10.0.0.2")
        );
        assert_eq!(
            client_ip(&trusted, ip("10.0.0.2"), &xff("10.0.0.5")),
            ip("10.0.0.2")
        );
        assert_eq!(
            client_ip(&trusted, ip("10.0.0.2"), &xff("garbage")),
            ip("10.0.0.2")
        );
        // Several header lines count as one list.
        let mut h = xff("1.2.3.4");
        h.append("x-forwarded-for", "198.51.100.1".parse().unwrap());
        assert_eq!(client_ip(&trusted, ip("10.0.0.2"), &h), ip("198.51.100.1"));
    }
}
