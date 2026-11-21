//! GPS assist (stage 2f): the satellite data the watch asks for, fetched by this server.
//!
//! `GET /api/device/gps-assist?path=<path?query>` (device token). The phone never talks to
//! Garmin; this endpoint is Nimrag's only contact with Garmin, and `NIMRAG_GPS_ASSIST=off`
//! (or `--no-gps-assist`) switches it off. The host is fixed here (`api.gcs.garmin.com`); only
//! a path under `/ephemeris/` with a strict character set is passed on (the same rule the core
//! applies to what the watch asks for, `nimrag_core::duties::agps::is_safe_path_and_query`).
//!
//! Each distinct path is cached in `<data dir>/gps-assist/` for 4 hours (the cache-control
//! Garmin sends; the API-wide `cache-control: no-store` stays, so the phone applies the same 4 h rule
//! itself, helped by the `Age` header). A stale copy is revalidated with `If-None-Match` before it is replaced.
//! Garmin's `ETag` is passed on as is and `If-None-Match` from the phone gives 304. Upstream
//! answers are cut at 1 MB, redirects are not followed, and the query of a path is never
//! logged or stored (only a hash of the whole path names the cache files).
//!
//! The Garmin URL and file are as fetched once by hand (a 66 KB tar of
//! `CPE_{GPS,GAL,GLO,QZSS}.BIN`, 200 without login); this code is ours, not ported.

use crate::api::{ApiError, AppState, Device};
use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use nimrag_core::duties::agps::{is_safe_path_and_query, MAX_BODY};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// Where the satellite data really comes from.
pub const GARMIN_BASE: &str = "https://api.gcs.garmin.com";
/// How long a cached copy is served without asking Garmin (what Garmin's `cache-control` says).
pub const FRESH_S: i64 = 4 * 3600;
/// Upstream request timeout.
pub const TIMEOUT: Duration = Duration::from_secs(20);
/// Cached paths kept; the oldest go first.
const MAX_ENTRIES: usize = 32;
const DIR: &str = "gps-assist";

/// Runtime configuration (`NIMRAG_GPS_ASSIST`, `--no-gps-assist`).
#[derive(Clone, Debug)]
pub struct GpsAssistConfig {
    /// `false`: the endpoint answers 404 and nothing is fetched.
    pub enabled: bool,
    /// Scheme and host of the upstream, no trailing slash. Tests point it at a local fake.
    pub upstream: String,
}

impl Default for GpsAssistConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            upstream: GARMIN_BASE.to_string(),
        }
    }
}

/// `NIMRAG_GPS_ASSIST`: `off`, `0`, `false`, `no` switch it off; `on`, `1`, `true`, `yes` or
/// nothing leave it on. Anything else is an error, so a typo cannot silently keep it on.
pub fn parse_switch(v: &str) -> Result<bool, String> {
    match v.trim().to_ascii_lowercase().as_str() {
        "" | "on" | "1" | "true" | "yes" => Ok(true),
        "off" | "0" | "false" | "no" => Ok(false),
        other => Err(format!("{other:?} is not on or off")),
    }
}

/// State of the feature inside [`AppState`].
pub struct GpsAssist {
    config: GpsAssistConfig,
    client: OnceLock<reqwest::Client>,
    /// One fetch at a time: a burst of phones makes one upstream request, not many.
    fetching: tokio::sync::Mutex<()>,
}

impl GpsAssist {
    pub fn new(config: GpsAssistConfig) -> Self {
        Self {
            config,
            client: OnceLock::new(),
            fetching: tokio::sync::Mutex::new(()),
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn client(&self) -> Result<&reqwest::Client, FetchError> {
        if let Some(c) = self.client.get() {
            return Ok(c);
        }
        let c = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .connect_timeout(Duration::from_secs(10))
            // A redirect could leave the fixed host.
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("nimrag-server")
            .build()
            .map_err(|_| FetchError::Upstream)?;
        Ok(self.client.get_or_init(|| c))
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/device/gps-assist", get(gps_assist))
}

#[derive(Deserialize)]
struct Params {
    path: String,
}

/// One cached upstream answer.
struct Entry {
    body: Vec<u8>,
    etag: String,
    fetched_at: i64,
}

#[derive(Serialize, Deserialize)]
struct Meta {
    etag: String,
    fetched_at: i64,
}

#[derive(Debug)]
enum FetchError {
    /// Upstream answered 404.
    NotFound,
    /// No answer, a bad status, too much data or nothing.
    Upstream,
}

async fn gps_assist(
    _device: Device,
    State(state): State<AppState>,
    Query(p): Query<Params>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let ga = &state.gps_assist;
    if !ga.enabled() {
        return Err(ApiError::not_found(
            "GPS assist is switched off on this server",
        ));
    }
    if !is_safe_path_and_query(&p.path) {
        return Err(ApiError::bad_request("not an /ephemeris/ path"));
    }
    let now = state.now();
    let dir = state.data_dir.join(DIR);
    let key = crate::sha256_hex(p.path.as_bytes());

    let entry = match read_cache(&dir, &key).await {
        Some(e) if now - e.fetched_at < FRESH_S && now >= e.fetched_at => e,
        stale => {
            let _one = ga.fetching.lock().await;
            // Someone may have fetched while this request waited.
            let cached = match read_cache(&dir, &key).await {
                Some(e) if now - e.fetched_at < FRESH_S && now >= e.fetched_at => {
                    return Ok(reply(e, now, &headers));
                }
                other => other.or(stale),
            };
            match fetch(ga, &p.path, cached, now).await {
                Ok(e) => {
                    if let Err(err) = write_cache(&dir, &key, &e).await {
                        tracing::warn!(error = %err, "gps-assist: cannot cache");
                    }
                    e
                }
                Err(FetchError::NotFound) => {
                    return Err(ApiError::not_found("Garmin has no such file"))
                }
                Err(FetchError::Upstream) => {
                    return Err(ApiError::new(
                        StatusCode::BAD_GATEWAY,
                        "could not get the satellite data from Garmin",
                    ))
                }
            }
        }
    };
    Ok(reply(entry, now, &headers))
}

/// 200 with the body, or 304 when the phone already has this ETag.
fn reply(e: Entry, now: i64, req: &HeaderMap) -> Response {
    let mut h = HeaderMap::new();
    if let Ok(v) = HeaderValue::from_str(&e.etag) {
        h.insert(header::ETAG, v);
    }
    h.insert(
        header::AGE,
        HeaderValue::from(((now - e.fetched_at).max(0)) as u64),
    );
    let matched = req
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|inm| etag_matches(inm, &e.etag));
    if matched {
        return (StatusCode::NOT_MODIFIED, h).into_response();
    }
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    (StatusCode::OK, h, e.body).into_response()
}

/// `If-None-Match` as a list, with weak tags and `*`.
fn etag_matches(header: &str, etag: &str) -> bool {
    let strip = |s: &str| s.trim().trim_start_matches("W/").to_string();
    let ours = strip(etag);
    header
        .split(',')
        .map(strip)
        .any(|t| t == "*" || (!ours.is_empty() && t == ours))
}

async fn fetch(
    ga: &GpsAssist,
    path: &str,
    cached: Option<Entry>,
    now: i64,
) -> Result<Entry, FetchError> {
    let client = ga.client()?;
    let url = format!("{}{}", ga.config.upstream.trim_end_matches('/'), path);
    let mut req = client.get(url);
    if let Some(c) = cached.as_ref().filter(|c| !c.etag.is_empty()) {
        req = req.header(header::IF_NONE_MATCH, c.etag.as_str());
    }
    let mut resp = req.send().await.map_err(|_| {
        tracing::warn!("gps-assist: the upstream request failed");
        FetchError::Upstream
    })?;
    let status = resp.status();
    if status == StatusCode::NOT_MODIFIED {
        if let Some(mut c) = cached {
            c.fetched_at = now;
            return Ok(c);
        }
        return Err(FetchError::Upstream);
    }
    if status == StatusCode::NOT_FOUND {
        return Err(FetchError::NotFound);
    }
    if status != StatusCode::OK {
        tracing::warn!(status = status.as_u16(), "gps-assist: upstream status");
        return Err(FetchError::Upstream);
    }
    if resp.content_length().is_some_and(|n| n > MAX_BODY as u64) {
        tracing::warn!("gps-assist: upstream body too large");
        return Err(FetchError::Upstream);
    }
    let etag = resp
        .headers()
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|_| FetchError::Upstream)? {
        if body.len() + chunk.len() > MAX_BODY {
            tracing::warn!("gps-assist: upstream body too large");
            return Err(FetchError::Upstream);
        }
        body.extend_from_slice(&chunk);
    }
    if body.is_empty() {
        return Err(FetchError::Upstream);
    }
    // No ETag from upstream: name the content, so the phone can still ask "unchanged?".
    let etag = etag
        .filter(|t| HeaderValue::from_str(t).is_ok())
        .unwrap_or_else(|| format!("\"{}\"", &crate::sha256_hex(&body)[..32]));
    tracing::info!(bytes = body.len(), "gps-assist: fetched from upstream");
    Ok(Entry {
        body,
        etag,
        fetched_at: now,
    })
}

fn files(dir: &Path, key: &str) -> (PathBuf, PathBuf) {
    (
        dir.join(format!("{key}.bin")),
        dir.join(format!("{key}.json")),
    )
}

async fn read_cache(dir: &Path, key: &str) -> Option<Entry> {
    let (bin, json) = files(dir, key);
    let meta: Meta = serde_json::from_slice(&tokio::fs::read(&json).await.ok()?).ok()?;
    let body = tokio::fs::read(&bin).await.ok()?;
    (!body.is_empty() && body.len() <= MAX_BODY).then_some(Entry {
        body,
        etag: meta.etag,
        fetched_at: meta.fetched_at,
    })
}

/// Write both files through a temporary name, body first, so a reader never sees a meta file
/// whose body is missing; then drop the oldest entries beyond [`MAX_ENTRIES`].
async fn write_cache(dir: &Path, key: &str, e: &Entry) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    let (bin, json) = files(dir, key);
    let meta = serde_json::to_vec(&Meta {
        etag: e.etag.clone(),
        fetched_at: e.fetched_at,
    })
    .map_err(std::io::Error::other)?;
    for (dest, bytes) in [(&bin, e.body.as_slice()), (&json, meta.as_slice())] {
        let tmp = dest.with_extension("tmp");
        tokio::fs::write(&tmp, bytes).await?;
        tokio::fs::rename(&tmp, dest).await?;
    }
    prune(dir).await;
    Ok(())
}

async fn prune(dir: &Path) {
    let Ok(mut rd) = tokio::fs::read_dir(dir).await else {
        return;
    };
    let mut metas = Vec::new();
    while let Ok(Some(ent)) = rd.next_entry().await {
        let p = ent.path();
        if p.extension().is_some_and(|x| x == "json") {
            let age = ent
                .metadata()
                .await
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            metas.push((age, p));
        }
    }
    if metas.len() <= MAX_ENTRIES {
        return;
    }
    metas.sort();
    for (_, json) in metas.iter().take(metas.len() - MAX_ENTRIES) {
        let _ = tokio::fs::remove_file(json.with_extension("bin")).await;
        let _ = tokio::fs::remove_file(json).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_words() {
        for on in ["", "on", "ON", "1", "true", "yes", " on "] {
            assert_eq!(parse_switch(on), Ok(true), "{on:?}");
        }
        for off in ["off", "OFF", "0", "false", "no"] {
            assert_eq!(parse_switch(off), Ok(false), "{off:?}");
        }
        assert!(parse_switch("maybe").is_err());
    }

    #[test]
    fn etag_comparison() {
        assert!(etag_matches("\"a\"", "\"a\""));
        assert!(etag_matches("W/\"a\"", "\"a\""));
        assert!(etag_matches("\"b\", \"a\"", "\"a\""));
        assert!(etag_matches("*", "\"a\""));
        assert!(!etag_matches("\"b\"", "\"a\""));
        assert!(!etag_matches("", "\"a\""));
    }
}
