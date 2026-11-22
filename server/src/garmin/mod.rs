//! Forward mode (Stage 2g): upload a user's FIT files to their own Garmin Connect account:
//! activities (FIT 128/4) always, and with the account's `send_all` choice every other file the
//! watch produces too (monitoring, sleep, metrics). Every user has their own account row, queue
//! rows and pending MFA login.
//!
//! * [`auth`]: unofficial mobile SSO login (+ MFA), DI OAuth2 tokens, refresh.
//! * [`upload`]: POST one FIT file (activity or not), classify the answer.
//! * [`queue`]: the `garmin_account` row, the `garmin_uploads` queue, the worker.
//! * [`api`]: the signed-in user's endpoints under `/api/garmin`.
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
use std::collections::HashMap;
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
    /// Pause between two uploads of one user's queue, in ms, so a backfill of hundreds of small
    /// files does not hammer Garmin. `0` skips it.
    pub upload_pause_ms: u64,
}

impl Default for GarminConfig {
    fn default() -> Self {
        Self {
            urls: GarminUrls::default(),
            http: ReqwestHttp::factory(true),
            login_delay_ms: (2000, 5000),
            upload_pause_ms: 1000,
        }
    }
}

impl GarminConfig {
    /// Local fake: every host on `base`, no proxy, no login delay, no pause between uploads.
    pub fn for_fake(base: &str) -> Self {
        Self {
            urls: GarminUrls::all_on(base),
            http: ReqwestHttp::factory(false),
            login_delay_ms: (0, 0),
            upload_pause_ms: 0,
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
    /// Pending MFA logins by user id.
    pending: Mutex<HashMap<i64, PendingLogin>>,
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
            pending: Mutex::new(HashMap::new()),
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

    pub(crate) fn set_pending(&self, uid: i64, mfa: PendingMfa, now: i64) {
        let mut p = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        p.retain(|_, l| now - l.created_at < MFA_TTL_S);
        p.insert(
            uid,
            PendingLogin {
                mfa: Arc::new(mfa),
                created_at: now,
            },
        );
    }

    /// The user's pending MFA login, unless it expired (then it is dropped).
    pub(crate) fn pending(&self, uid: i64, now: i64) -> Option<Arc<PendingMfa>> {
        let mut p = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if p.get(&uid).is_some_and(|l| now - l.created_at >= MFA_TTL_S) {
            p.remove(&uid);
        }
        p.get(&uid).map(|l| l.mfa.clone())
    }

    pub(crate) fn pending_since(&self, uid: i64, now: i64) -> Option<i64> {
        self.pending(uid, now)?;
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&uid)
            .map(|l| l.created_at)
    }

    pub(crate) fn clear_pending(&self, uid: i64) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&uid);
    }
}

impl Default for Garmin {
    fn default() -> Self {
        Self::new(GarminConfig::default())
    }
}

/// What a stored file is to Forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wanted {
    /// A new activity file (`data_type 128`, `sub_type 4`): queued whenever Forward is on.
    Activity,
    /// Any other FIT file from the watch or USB: queued only when the account has `send_all`.
    Other,
}

/// Whether a newly stored file goes to Garmin, and under which rule. Pure: the account's
/// `send_all` choice is checked by the queue. Files that came from Garmin (`import`, `connect`)
/// are never "other", so they are not sent back.
pub fn wanted(meta: &Meta, is_fit: bool, created: bool) -> Option<Wanted> {
    if !(created && is_fit && meta.data_type == Some(128)) {
        return None;
    }
    if meta.sub_type == Some(4) {
        return Some(Wanted::Activity);
    }
    matches!(meta.source.as_str(), "watch" | "usb").then_some(Wanted::Other)
}

/// Ingest hook: a file was just stored for `owner`. Queues it when it is a new activity
/// FIT file (`data_type 128`, `sub_type 4`) or, when that user's account has `send_all` on, any
/// other new FIT file from the watch or USB; the file must have an owner with Forward on. Never
/// fails the ingest; imports (no upload metadata) are not queued, backfill covers them.
pub async fn on_stored(
    state: &AppState,
    sha256: &str,
    owner: Option<i64>,
    meta: &Meta,
    is_fit: bool,
    created: bool,
) {
    let Some(owner) = owner else { return };
    let now = state.now();
    let queued = match wanted(meta, is_fit, created) {
        None => return,
        Some(Wanted::Activity) => queue::enqueue(&state.pool, owner, sha256, now).await,
        Some(Wanted::Other) => queue::enqueue_other(&state.pool, owner, sha256, now).await,
    };
    match queued {
        Ok(true) => state.garmin.wake(),
        Ok(false) => {}
        Err(e) => tracing::warn!(error = format!("{e:#}"), "garmin: queueing a file failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(source: &str, data_type: Option<i64>, sub_type: Option<i64>) -> Meta {
        Meta {
            source: source.into(),
            data_type,
            sub_type,
            file_index: None,
            file_number: None,
            watch_timestamp: None,
        }
    }

    #[test]
    fn what_is_wanted() {
        let w = |m: Meta, fit, created| wanted(&m, fit, created);
        // Activities keep today's rule, whatever the source.
        assert_eq!(
            w(meta("watch", Some(128), Some(4)), true, true),
            Some(Wanted::Activity)
        );
        assert_eq!(
            w(meta("import", Some(128), Some(4)), true, true),
            Some(Wanted::Activity)
        );
        // Monitoring, sleep, metrics, ...: from the watch or USB only.
        for sub in [
            Some(32),
            Some(41),
            Some(44),
            Some(49),
            Some(58),
            Some(68),
            None,
        ] {
            assert_eq!(
                w(meta("watch", Some(128), sub), true, true),
                Some(Wanted::Other),
                "{sub:?}"
            );
            assert_eq!(
                w(meta("usb", Some(128), sub), true, true),
                Some(Wanted::Other),
                "{sub:?}"
            );
            assert_eq!(
                w(meta("import", Some(128), sub), true, true),
                None,
                "{sub:?}"
            );
            assert_eq!(
                w(meta("connect", Some(128), sub), true, true),
                None,
                "{sub:?}"
            );
        }
        // Not FIT, not new, not a FIT data type.
        assert_eq!(w(meta("watch", Some(128), Some(32)), false, true), None);
        assert_eq!(w(meta("watch", Some(128), Some(32)), true, false), None);
        assert_eq!(w(meta("watch", Some(8), Some(32)), true, true), None);
        assert_eq!(w(meta("watch", None, None), true, true), None);
    }
}
