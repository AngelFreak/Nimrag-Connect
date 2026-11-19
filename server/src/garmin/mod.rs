//! Forward mode (Stage 2g): upload the owner's activity files (FIT 128/4) to
//! their Garmin Connect account.
//!
//! * [`auth`]: unofficial mobile SSO login (+ MFA), DI OAuth2 tokens, refresh.
//! * [`upload`]: POST one FIT file, classify the answer.
//! * [`queue`]: the `garmin_account` row, the `garmin_uploads` queue, the worker.
//! * [`api`]: the owner's endpoints under `/api/garmin`.
//! * [`http`]: the `GarminHttp` seam (plain reqwest today).
//!
//! Hard rules: the password is used once in memory and never stored or logged;
//! tokens are stored in the database only, never logged and never returned by
//! the API; every Garmin base URL is configurable ([`GarminUrls`]) so tests run
//! against a local fake and never touch Garmin.

pub mod api;
pub mod auth;
pub mod http;
pub mod queue;
pub mod upload;

use crate::api::AppState;
use crate::ledger::Meta;
use auth::PendingMfa;
use http::{GarminHttp, HttpFactory, ReqwestHttp};
use std::sync::{Arc, Mutex, OnceLock};

/// How long a login waits for its MFA code (the cookie jar lives in memory only).
pub const MFA_TTL_S: i64 = 600;

/// Every Garmin base URL. Defaults are the real hosts; tests point them at a fake.
#[derive(Debug, Clone)]
pub struct GarminUrls {
    /// `https://sso.garmin.com`
    pub sso: String,
    /// `https://connectapi.garmin.com` (upload, profile check)
    pub connectapi: String,
    /// `https://diauth.garmin.com` (token exchange and refresh)
    pub diauth: String,
    /// The `service` the mobile login asks a ticket for (a label, never called).
    pub ios_service: String,
}

impl Default for GarminUrls {
    fn default() -> Self {
        Self {
            sso: "https://sso.garmin.com".into(),
            connectapi: "https://connectapi.garmin.com".into(),
            diauth: "https://diauth.garmin.com".into(),
            ios_service: "https://mobile.integration.garmin.com/gcm/ios".into(),
        }
    }
}

impl GarminUrls {
    pub fn di_token_url(&self) -> String {
        format!("{}/di-oauth2-service/oauth/token", self.diauth)
    }

    /// All hosts on one base URL (a local fake). The `service` label is kept.
    pub fn all_on(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self {
            sso: base.into(),
            connectapi: base.into(),
            diauth: base.into(),
            ..Self::default()
        }
    }
}

#[derive(Clone)]
pub struct GarminConfig {
    pub urls: GarminUrls,
    /// Makes HTTP sessions; swap in a browser-TLS client here (see [`http`]).
    pub http: HttpFactory,
    /// Random pause between the sign-in page and the credential POST, in ms
    /// (min, max). The reference client waits 2 to 5 s; `(0, 0)` skips it.
    pub login_delay_ms: (u64, u64),
}

impl Default for GarminConfig {
    fn default() -> Self {
        Self {
            urls: GarminUrls::default(),
            http: ReqwestHttp::factory(true),
            login_delay_ms: (2000, 5000),
        }
    }
}

impl GarminConfig {
    /// Local fake: every host on `base`, no proxy, no login delay.
    pub fn for_fake(base: &str) -> Self {
        Self {
            urls: GarminUrls::all_on(base),
            http: ReqwestHttp::factory(false),
            login_delay_ms: (0, 0),
        }
    }
}

struct PendingLogin {
    mfa: Arc<PendingMfa>,
    created_at: i64,
}

/// Runtime state of Forward that is not in the database.
pub struct Garmin {
    pub cfg: GarminConfig,
    api: OnceLock<Arc<dyn GarminHttp>>,
    wake: tokio::sync::Notify,
    pending: Mutex<Option<PendingLogin>>,
    /// One login at a time.
    pub(crate) login_lock: tokio::sync::Mutex<()>,
    /// Serialises refresh and token writes so a rotated refresh token is never lost.
    pub(crate) token_lock: tokio::sync::Mutex<()>,
}

impl Garmin {
    pub fn new(cfg: GarminConfig) -> Self {
        Self {
            cfg,
            api: OnceLock::new(),
            wake: tokio::sync::Notify::new(),
            pending: Mutex::new(None),
            login_lock: Default::default(),
            token_lock: Default::default(),
        }
    }

    /// The cookie-less session for API calls.
    pub fn api(&self) -> &Arc<dyn GarminHttp> {
        self.api.get_or_init(|| (self.cfg.http)(false))
    }

    /// Wake the worker (new file queued, login, retry).
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    pub(crate) async fn wake_wait(&self) {
        self.wake.notified().await;
    }

    pub(crate) fn set_pending(&self, mfa: PendingMfa, now: i64) {
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(PendingLogin {
            mfa: Arc::new(mfa),
            created_at: now,
        });
    }

    /// The pending MFA login, unless it expired (then it is dropped).
    pub(crate) fn pending(&self, now: i64) -> Option<Arc<PendingMfa>> {
        let mut p = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if p.as_ref().is_some_and(|l| now - l.created_at >= MFA_TTL_S) {
            *p = None;
        }
        p.as_ref().map(|l| l.mfa.clone())
    }

    pub(crate) fn pending_since(&self, now: i64) -> Option<i64> {
        self.pending(now)?;
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|l| l.created_at)
    }

    pub(crate) fn clear_pending(&self) {
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

impl Default for Garmin {
    fn default() -> Self {
        Self::new(GarminConfig::default())
    }
}

/// Ingest hook: a file was just stored. Queues it when it is a new activity
/// FIT file (`data_type 128`, `sub_type 4`) and Forward is on. Never fails the
/// ingest; imports (no upload metadata) are not queued, backfill covers them.
pub async fn on_stored(state: &AppState, sha256: &str, meta: &Meta, is_fit: bool, created: bool) {
    if !(created && is_fit && meta.data_type == Some(128) && meta.sub_type == Some(4)) {
        return;
    }
    match queue::enqueue(&state.pool, sha256, state.now()).await {
        Ok(true) => state.garmin.wake(),
        Ok(false) => {}
        Err(e) => tracing::warn!(error = format!("{e:#}"), "garmin: queueing a file failed"),
    }
}
