//! Authentication: argon2id passwords, cookie sessions bound to a user, login rate limits (per
//! address and per username), the short-lived pending steps of the login (2FA enrolment, forced
//! password change, first-run setup) and the CSRF header. Device tokens (`tokens.rs`) can only
//! upload; every `/api` read goes through [`AuthUser`], [`DataUser`] or [`AdminUser`], which
//! accept a session cookie only.
//!
//! Nothing here logs passwords, session ids, tokens, secrets or codes.

use crate::api::{ApiError, AppState};
use crate::users::{self, Role, User};
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
use sqlx::{Row, SqlitePool};
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
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

/// A valid argon2id hash nobody knows the password of: verified when the username does not
/// exist (or the account is disabled), so a wrong username costs the same as a wrong password.
fn dummy_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| {
        let mut junk = [0u8; 24];
        let _ = getrandom::fill(&mut junk);
        hash_password(&crate::hex(&junk)).unwrap_or_default()
    })
}

// ---- sessions ---------------------------------------------------------------

fn session_hash(raw: &str) -> String {
    crate::hex(&Sha256::digest(raw.as_bytes()))
}

/// New session for `user_id`: returns the cookie value (32 random bytes, hex). Only its hash is
/// stored. `token_id` ties an app session to its device token (revoking the token ends it).
pub async fn create_session(
    pool: &SqlitePool,
    user_id: i64,
    token_id: Option<i64>,
    now: i64,
) -> Result<String> {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| anyhow!("no randomness: {e}"))?;
    let id = crate::hex(&raw);
    sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
        .bind(now)
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO sessions (id_hash, user_id, token_id, created_at, expires_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(session_hash(&id))
    .bind(user_id)
    .bind(token_id)
    .bind(now)
    .bind(now + SESSION_SECS)
    .execute(pool)
    .await?;
    Ok(id)
}

/// The user behind a live session (not expired, account not disabled).
pub async fn session_user(pool: &SqlitePool, raw: &str, now: i64) -> Result<Option<AuthUser>> {
    let row = sqlx::query(
        "SELECT u.id, u.username, u.role FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.id_hash = ? AND s.expires_at > ? AND u.disabled_at IS NULL",
    )
    .bind(session_hash(raw))
    .bind(now)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| AuthUser {
        id: r.get(0),
        username: r.get(1),
        role: Role::parse(&r.get::<String, _>(2)),
    }))
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

/// In-memory login throttle, keyed by client address ([`client_ip`]) or by username:
/// - per key: after `free` consecutive failures, further attempts are refused for 2, 4, 8 ...
///   seconds (capped at 5 minutes). A success resets the key, and so does 15 minutes without a
///   failure.
/// - global (the address limiter only): more than 20 failures within a minute, across all keys,
///   makes every attempt wait 1 s after each further failure (fixed) until the rate drops again,
///   so rotating source addresses does not buy more guesses.
///
/// Without `--trusted-proxy`, behind a reverse proxy the address key is the proxy's address and
/// the limit is shared by all clients.
pub struct Limiter<K> {
    inner: Mutex<LimiterState<K>>,
    free: u32,
    global: bool,
}

struct LimiterState<K> {
    /// Per key: (consecutive failures, locked until, time of last failure).
    keys: HashMap<K, (u32, i64, i64)>,
    /// Times of failures in the last minute, all keys.
    recent: VecDeque<i64>,
    global_until: i64,
}

/// The per-address limiter (four free attempts, then back-off, plus the global ceiling).
pub type LoginLimiter = Limiter<IpAddr>;

impl Default for Limiter<IpAddr> {
    fn default() -> Self {
        Self::new(4, true)
    }
}

impl<K: Hash + Eq + Clone> Limiter<K> {
    const MAX_DELAY: i64 = 300;
    /// A key's failure counter is forgotten after this long without a failure.
    pub const DECAY_SECS: i64 = 15 * 60;
    const GLOBAL_WINDOW: i64 = 60;
    const GLOBAL_MAX_PER_WINDOW: usize = 20;
    const GLOBAL_DELAY: i64 = 1;

    /// `free` failures per key before the back-off starts; `global` switches the all-keys
    /// ceiling on.
    pub fn new(free: u32, global: bool) -> Self {
        Self {
            inner: Mutex::new(LimiterState {
                keys: HashMap::new(),
                recent: VecDeque::new(),
                global_until: 0,
            }),
            free,
            global,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, LimiterState<K>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `Err(seconds to wait)` while the key (or everyone) is locked out.
    pub fn check(&self, key: &K, now: i64) -> Result<(), i64> {
        let st = self.state();
        let key_until = st.keys.get(key).map_or(0, |(_, until, _)| *until);
        let until = key_until.max(st.global_until);
        if until > now {
            Err(until - now)
        } else {
            Ok(())
        }
    }

    pub fn failure(&self, key: &K, now: i64) {
        let mut st = self.state();
        if st.keys.len() > 1024 {
            st.keys
                .retain(|_, (_, _, last)| now - *last < Self::DECAY_SECS);
        }
        let e = st.keys.entry(key.clone()).or_insert((0, 0, now));
        if now - e.2 >= Self::DECAY_SECS {
            *e = (0, 0, now);
        }
        e.0 += 1;
        e.2 = now;
        if e.0 > self.free {
            let shift = (e.0 - self.free).min(9);
            e.1 = now + (1i64 << shift).min(Self::MAX_DELAY);
        }
        if self.global {
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
    }

    pub fn success(&self, key: &K) {
        self.state().keys.remove(key);
    }
}

/// The per-username limiter: nine free attempts, so a stranger who only knows a username cannot
/// lock the owner out with a handful of guesses, but a slow guesser is still throttled.
pub type UsernameLimiter = Limiter<String>;

pub fn username_limiter() -> UsernameLimiter {
    Limiter::new(9, false)
}

/// The key the username limiter uses (lower case, bounded, so garbage cannot grow the map).
fn username_key(raw: &str) -> String {
    raw.trim().to_ascii_lowercase().chars().take(64).collect()
}

/// Outcome of [`verify_login`].
pub enum Cred {
    /// Username and password are right and the account is enabled.
    Ok(Box<User>),
    /// Wrong username, wrong password or a disabled account: not told apart.
    Wrong,
    /// Locked out; seconds to wait.
    Locked(i64),
}

fn locked_for(state: &AppState, ip: IpAddr, name: &str, now: i64) -> Option<i64> {
    let a = state.limiter.check(&ip, now).err();
    let b = state.username_limiter.check(&name.to_owned(), now).err();
    a.max(b)
}

/// Count one failed attempt against the address and the username.
pub fn record_failure(state: &AppState, ip: IpAddr, username: &str) {
    let now = state.now();
    state.limiter.failure(&ip, now);
    state.username_limiter.failure(&username_key(username), now);
}

/// A completed login: forget the failures of this address and username.
pub fn record_success(state: &AppState, ip: IpAddr, username: &str) {
    state.limiter.success(&ip);
    state.username_limiter.success(&username_key(username));
}

/// Check a username and password under the limiters. The checking is serialised (one argon2 job
/// at a time, 19 MiB each) and the limiters are checked again once the turn comes, so a burst of
/// parallel guesses cannot all pass the check before the first failure is recorded. An unknown
/// username (or a disabled account) is verified against a dummy hash, so it takes as long as a
/// wrong password. The work runs in its own task: a client that disconnects mid-request still
/// gets its failure counted.
///
/// A failure is recorded here; a success is not (the caller still has the 2FA code to check and
/// calls [`record_success`] at the end).
pub async fn verify_login(
    state: &AppState,
    ip: IpAddr,
    username: &str,
    password: String,
) -> Result<Cred> {
    let state = state.clone();
    let name = username_key(username);
    let username = username.to_owned();
    tokio::spawn(async move {
        // Cheap early refusal, so locked-out clients do not queue for a permit.
        if let Some(wait) = locked_for(&state, ip, &name, state.now()) {
            return Ok(Cred::Locked(wait));
        }
        let _permit = state
            .login_gate
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| anyhow!("login gate closed: {e}"))?;
        if let Some(wait) = locked_for(&state, ip, &name, state.now()) {
            return Ok(Cred::Locked(wait));
        }
        let user = users::by_username(&state.pool, &username)
            .await?
            .filter(|u| u.disabled_at.is_none());
        let hash = user
            .as_ref()
            .map_or_else(|| dummy_hash().to_owned(), |u| u.password_hash.clone());
        let ok = tokio::task::spawn_blocking(move || verify_password(&password, &hash)).await?;
        match user {
            Some(u) if ok => Ok(Cred::Ok(Box::new(u))),
            _ => {
                record_failure(&state, ip, &username);
                Ok(Cred::Wrong)
            }
        }
    })
    .await?
}

// ---- pending login steps --------------------------------------------------------

/// What a pending token is for.
#[derive(Debug, Clone)]
pub enum PendingKind {
    /// First-run setup: the admin to create once its 2FA is confirmed. Holds the argon2 hash,
    /// never the password.
    Setup {
        username: String,
        password_hash: String,
    },
    /// Login of a user without 2FA (new, or reset): confirm the secret with a code.
    Enrol { user_id: i64, username: String },
    /// Right password and code, but the password must be changed before a session starts.
    ChangePassword { user_id: i64, username: String },
    /// A signed-in user replacing their 2FA secret.
    Reenrol { user_id: i64, username: String },
}

#[derive(Debug, Clone)]
pub struct Pending {
    pub kind: PendingKind,
    /// Base32 secret being enrolled (empty for [`PendingKind::ChangePassword`]).
    pub secret: String,
    pub expires_at: i64,
    /// Wrong codes so far; the token is burned after [`Pending::MAX_ATTEMPTS`].
    pub attempts: u32,
}

impl Pending {
    pub const TTL_SECS: i64 = 10 * 60;
    pub const MAX_ATTEMPTS: u32 = 5;
}

/// Short-lived, single-use tokens for the steps between a right password and a session. In
/// memory only (a restart just means starting the login again); only the SHA-256 of the token is
/// the key. Bounded, so unauthenticated setup requests cannot grow it.
#[derive(Default)]
pub struct PendingStore {
    inner: Mutex<HashMap<String, Pending>>,
}

impl PendingStore {
    const MAX_SETUP: usize = 16;
    const MAX_TOTAL: usize = 256;

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, Pending>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Store a pending step and return its token. A user has one pending step of each kind
    /// (a new one replaces the old); setup tokens are capped, oldest dropped first.
    pub fn insert(&self, kind: PendingKind, secret: String, now: i64) -> Result<String> {
        let mut raw = [0u8; 32];
        getrandom::fill(&mut raw).map_err(|e| anyhow!("no randomness: {e}"))?;
        let token = crate::hex(&raw);
        let mut m = self.map();
        m.retain(|_, p| p.expires_at > now);
        let same = |a: &PendingKind, b: &PendingKind| match (a, b) {
            (PendingKind::Enrol { user_id: x, .. }, PendingKind::Enrol { user_id: y, .. })
            | (
                PendingKind::ChangePassword { user_id: x, .. },
                PendingKind::ChangePassword { user_id: y, .. },
            )
            | (PendingKind::Reenrol { user_id: x, .. }, PendingKind::Reenrol { user_id: y, .. }) => {
                x == y
            }
            _ => false,
        };
        m.retain(|_, p| !same(&p.kind, &kind));
        if matches!(kind, PendingKind::Setup { .. }) {
            let mut setups: Vec<(String, i64)> = m
                .iter()
                .filter(|(_, p)| matches!(p.kind, PendingKind::Setup { .. }))
                .map(|(k, p)| (k.clone(), p.expires_at))
                .collect();
            if setups.len() >= Self::MAX_SETUP {
                setups.sort_by_key(|(_, at)| *at);
                for (k, _) in setups.iter().take(setups.len() + 1 - Self::MAX_SETUP) {
                    m.remove(k);
                }
            }
        }
        if m.len() >= Self::MAX_TOTAL {
            anyhow::bail!("too many pending logins");
        }
        m.insert(
            session_hash(&token),
            Pending {
                kind,
                secret,
                expires_at: now + Pending::TTL_SECS,
                attempts: 0,
            },
        );
        Ok(token)
    }

    /// The pending step behind `token`, if it exists and has not expired.
    pub fn get(&self, token: &str, now: i64) -> Option<Pending> {
        let mut m = self.map();
        let key = session_hash(token);
        if m.get(&key).is_some_and(|p| p.expires_at <= now) {
            m.remove(&key);
        }
        m.get(&key).cloned()
    }

    /// A wrong code: count it, and burn the token after too many. Returns whether it survives.
    pub fn failed_attempt(&self, token: &str) -> bool {
        let mut m = self.map();
        let key = session_hash(token);
        let Some(p) = m.get_mut(&key) else {
            return false;
        };
        p.attempts += 1;
        if p.attempts >= Pending::MAX_ATTEMPTS {
            m.remove(&key);
            return false;
        }
        true
    }

    /// Use the token up. `false` when it was already used (two racing requests).
    pub fn take(&self, token: &str) -> bool {
        self.map().remove(&session_hash(token)).is_some()
    }
}

// ---- extractor and CSRF guard -----------------------------------------------

/// The signed-in user (valid session cookie, account not disabled), of either role. Device
/// tokens never pass.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
    pub role: Role,
}

impl<S> FromRequestParts<S> for AuthUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        let state = AppState::from_ref(state);
        let user = match session_cookie(&parts.headers, state.insecure_cookies) {
            Some(id) => session_user(&state.pool, &id, state.now()).await?,
            None => None,
        };
        user.ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "sign in required"))
    }
}

/// A signed-in user with data (role `user`). Admin sessions get 403: the admin sets the server
/// up and manages accounts, and has no data of its own.
#[derive(Debug, Clone)]
pub struct DataUser(pub AuthUser);

impl std::ops::Deref for DataUser {
    type Target = AuthUser;
    fn deref(&self) -> &AuthUser {
        &self.0
    }
}

impl<S> FromRequestParts<S> for DataUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if user.role == Role::Admin {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "admin accounts have no data; sign in as a user",
            ));
        }
        Ok(Self(user))
    }
}

/// A signed-in admin. Users get 403.
#[derive(Debug, Clone)]
pub struct AdminUser(pub AuthUser);

impl std::ops::Deref for AdminUser {
    type Target = AuthUser;
    fn deref(&self) -> &AuthUser {
        &self.0
    }
}

impl<S> FromRequestParts<S> for AdminUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if user.role != Role::Admin {
            return Err(ApiError::new(StatusCode::FORBIDDEN, "admin only"));
        }
        Ok(Self(user))
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
    fn the_dummy_hash_is_a_real_argon2id_hash() {
        assert!(dummy_hash().starts_with("$argon2id$"));
        assert!(!verify_password("anything", dummy_hash()));
    }

    #[test]
    fn username_limiter_allows_nine_failures_then_backs_off() {
        let l = username_limiter();
        let k = "anna".to_owned();
        for _ in 0..9 {
            l.failure(&k, 100);
            assert!(l.check(&k, 100).is_ok());
        }
        l.failure(&k, 100);
        assert_eq!(l.check(&k, 100), Err(2));
        // Another username is not affected, and neither is the global ceiling (not used here).
        assert!(l.check(&"bo".to_owned(), 100).is_ok());
        l.success(&k);
        assert!(l.check(&k, 100).is_ok());
    }

    #[test]
    fn pending_tokens_are_single_use_expire_and_burn_after_five_wrong_codes() {
        let store = PendingStore::default();
        let kind = || PendingKind::Enrol {
            user_id: 7,
            username: "anna".into(),
        };
        let t = store.insert(kind(), "SECRET".into(), 1000).unwrap();
        assert_eq!(store.get(&t, 1000).unwrap().secret, "SECRET");
        // A new pending step of the same kind for the same user replaces the old one.
        let t2 = store.insert(kind(), "OTHER".into(), 1001).unwrap();
        assert!(store.get(&t, 1001).is_none());
        assert_eq!(store.get(&t2, 1001).unwrap().secret, "OTHER");
        for _ in 0..4 {
            assert!(store.failed_attempt(&t2));
        }
        assert!(!store.failed_attempt(&t2));
        assert!(store.get(&t2, 1001).is_none());
        let t3 = store.insert(kind(), "S".into(), 2000).unwrap();
        assert!(store.get(&t3, 2000 + Pending::TTL_SECS).is_none());
        let t4 = store.insert(kind(), "S".into(), 3000).unwrap();
        assert!(store.take(&t4));
        assert!(!store.take(&t4));
    }

    #[test]
    fn setup_tokens_are_capped() {
        let store = PendingStore::default();
        let mk = |i: usize| PendingKind::Setup {
            username: format!("admin{i}"),
            password_hash: String::new(),
        };
        let tokens: Vec<String> = (0..20)
            .map(|i| store.insert(mk(i), String::new(), 1000 + i as i64).unwrap())
            .collect();
        let alive = tokens
            .iter()
            .filter(|t| store.get(t, 1100).is_some())
            .count();
        assert_eq!(alive, 16);
        assert!(store.get(&tokens[19], 1100).is_some());
        assert!(store.get(&tokens[0], 1100).is_none());
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
            l.failure(&ip, 100);
            assert!(l.check(&ip, 100).is_ok());
        }
        l.failure(&ip, 100);
        assert_eq!(l.check(&ip, 100), Err(2));
        assert!(l.check(&ip, 102).is_ok());
        l.failure(&ip, 102);
        assert_eq!(l.check(&ip, 102), Err(4));
        l.success(&ip);
        assert!(l.check(&ip, 102).is_ok());
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
            l.failure(&ip, 1000 + i);
        }
        // Slow guessing never locks out: every failure is 15 min after the last.
        let mut t = 1003;
        for _ in 0..10 {
            t += LoginLimiter::DECAY_SECS;
            l.failure(&ip, t);
            assert!(l.check(&ip, t).is_ok());
        }
        // Without the decay, four quick failures plus one more would lock.
        for i in 1..=3 {
            l.failure(&ip, t + i);
            assert!(l.check(&ip, t + i).is_ok());
        }
        l.failure(&ip, t + 4);
        assert_eq!(l.check(&ip, t + 4), Err(2));
        // Just under 15 minutes after the last failure the counter is still live.
        let last = t + 4;
        l.failure(&ip, last + LoginLimiter::DECAY_SECS - 1);
        assert!(l.check(&ip, last + LoginLimiter::DECAY_SECS - 1).is_err());
    }

    #[test]
    fn global_ceiling_delays_everyone_by_one_second() {
        let l = LoginLimiter::default();
        let other: IpAddr = "192.0.2.1".parse().unwrap();
        // 20 failures in a minute from 20 different addresses: still free.
        for i in 0..20u8 {
            l.failure(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, i)), 5000);
        }
        assert!(l.check(&other, 5000).is_ok());
        // The 21st trips the ceiling: every address waits 1 s, fixed.
        l.failure(&IpAddr::V4(Ipv4Addr::new(10, 0, 1, 0)), 5000);
        assert_eq!(l.check(&other, 5000), Err(1));
        assert!(l.check(&other, 5001).is_ok());
        l.failure(&IpAddr::V4(Ipv4Addr::new(10, 0, 1, 1)), 5001);
        assert_eq!(l.check(&other, 5001), Err(1)); // no doubling
                                                   // A minute later the window is empty again.
        assert!(l.check(&other, 5100).is_ok());
        l.failure(&other, 5100);
        assert!(l.check(&other, 5100).is_ok());
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
