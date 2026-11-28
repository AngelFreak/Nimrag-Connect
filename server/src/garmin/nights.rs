//! The morning job: fetch the nights the watch never delivered from the user's Garmin account.
//!
//! The watch hands each file to whichever phone app connects first, once. When the Garmin Connect
//! app wins, Nimrag never gets that night's stages and HRV, but Garmin has them. Every 30 minutes
//! (first pass 2 minutes after start) this job looks, for every user with a connected Garmin
//! account and `fill_nights` on, at the last 7 local dates and asks Garmin for the ones the watch
//! did not give us. The answer is stored as a `sleep_detail` Connect file, exactly as
//! `import-connect` would store it (source `connect`: never sent back to Garmin, never above the
//! watch's own rows).
//!
//! A date is asked at most every 3 hours (`no_night`, `error`) and never again once `filled`;
//! today is asked from 09:00 local time only. The state is `garmin_night_checks`.
//!
//! Hard rules: tokens, the profile name and response bodies are never logged or stored (the blob
//! is the Connect JSON of the night, with its account ids stripped by the importer); the log has
//! counts only.

use super::auth::{self, Tokens};
use super::http::{self, Request};
use super::queue::{self, AccountStatus, Refreshed};
use crate::api::AppState;
use crate::connect::{import, Kind};
use crate::settings;
use anyhow::Result;
use chrono::{Duration as Days, NaiveDate, TimeZone};
use chrono_tz::Tz;
use serde_json::Value;
use sqlx::Row;
use std::collections::HashMap;
use std::time::Duration;

/// How many local dates back (today included) a pass looks.
const WINDOW_DAYS: i64 = 7;
/// At most this many night requests per user and pass.
const MAX_REQUESTS: usize = 7;
/// A date that came back empty or failed is not asked again for this long.
const RECHECK_S: i64 = 3 * 3600;
/// Today is asked from this local hour (the watch usually syncs by then).
const TODAY_FROM_HOUR: u32 = 9;
/// First pass after start, and the gap between passes.
const FIRST_PASS_S: u64 = 120;
const PASS_EVERY_S: u64 = 1800;

/// What the job keeps in memory only.
#[derive(Default)]
pub struct Memory {
    /// Garmin's profile name by user: asked once per process, never stored or logged.
    display_names: HashMap<i64, String>,
    /// A 429 with `Retry-After`: the user is left alone until this unix second.
    hold_until: HashMap<i64, i64>,
}

/// Counts of one pass (the only thing logged).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub users: u32,
    pub asked: u32,
    pub filled: u32,
    pub no_night: u32,
    pub errors: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Filled,
    NoNight,
    Error,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Filled => "filled",
            Self::NoNight => "no_night",
            Self::Error => "error",
        }
    }
}

/// Background task: a pass 2 minutes after start, then every 30 minutes. Errors are logged and
/// the next pass tries again.
pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(FIRST_PASS_S)).await;
        loop {
            match run_pass(&state).await {
                Ok(c) => tracing::info!(
                    users = c.users,
                    asked = c.asked,
                    filled = c.filled,
                    no_night = c.no_night,
                    errors = c.errors,
                    "garmin nights pass"
                ),
                Err(e) => tracing::warn!(error = format!("{e:#}"), "garmin nights pass failed"),
            }
            tokio::time::sleep(Duration::from_secs(PASS_EVERY_S)).await;
        }
    })
}

/// One pass over every user with a connected account and `fill_nights` on.
pub async fn run_pass(state: &AppState) -> Result<Counts> {
    let uids: Vec<i64> = sqlx::query_scalar(
        "SELECT user_id FROM garmin_account
         WHERE status = 'connected' AND fill_nights = 1 AND user_id > 0 ORDER BY user_id",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut counts = Counts::default();
    for uid in uids {
        if let Err(e) = run_user(state, uid, &mut counts).await {
            counts.errors += 1;
            tracing::warn!(
                error = format!("{e:#}"),
                "garmin nights: a user's pass failed"
            );
        }
    }
    Ok(counts)
}

/// Unix seconds of `hour`:00 local time on `date`.
fn local_at(tz: Tz, date: NaiveDate, hour: u32) -> i64 {
    date.and_hms_opt(hour, 0, 0)
        .and_then(|t| tz.from_local_datetime(&t).earliest())
        .map_or_else(
            || settings::day_start(tz, date) + i64::from(hour) * 3600,
            |d| d.timestamp(),
        )
}

/// Does the watch (a FIT-source file, the same notion as migration 0020's views) already have
/// this night's stages? The night that ends on `date` lies in [date-1 18:00, date 14:00) local;
/// stage rows end their period, so their `ts` is tested.
async fn watch_has_night(state: &AppState, uid: i64, tz: Tz, date: NaiveDate) -> Result<bool> {
    let lo = local_at(tz, date - Days::days(1), 18);
    let hi = local_at(tz, date, 14);
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM sleep_levels l
         JOIN sources s ON s.src_id = l.src AND s.origin = 'fit'
         WHERE l.user_id = ? AND l.ts >= ? AND l.ts < ? LIMIT 1",
    )
    .bind(uid)
    .bind(lo)
    .bind(hi)
    .fetch_optional(&state.pool)
    .await?;
    Ok(found.is_some())
}

async fn checks(state: &AppState, uid: i64) -> Result<HashMap<String, (i64, String)>> {
    let rows =
        sqlx::query("SELECT date, checked_at, outcome FROM garmin_night_checks WHERE user_id = ?")
            .bind(uid)
            .fetch_all(&state.pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get::<String, _>(0), (r.get(1), r.get::<String, _>(2))))
        .collect())
}

async fn record(state: &AppState, uid: i64, date: &str, outcome: Outcome) -> Result<()> {
    sqlx::query(
        "INSERT INTO garmin_night_checks (user_id, date, checked_at, outcome) VALUES (?, ?, ?, ?)
         ON CONFLICT (user_id, date) DO UPDATE SET
             checked_at = excluded.checked_at, outcome = excluded.outcome",
    )
    .bind(uid)
    .bind(date)
    .bind(state.now())
    .bind(outcome.as_str())
    .execute(&state.pool)
    .await?;
    Ok(())
}

/// The newest `filled` date of a user, for the API.
pub async fn last_filled_night(pool: &sqlx::SqlitePool, uid: i64) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT MAX(date) FROM garmin_night_checks WHERE user_id = ? AND outcome = 'filled'",
    )
    .bind(uid)
    .fetch_one(pool)
    .await?)
}

/// What one GET to Garmin came to.
enum Got {
    /// 2xx: the body text.
    Body(String),
    /// Another status (not auth, not 429): the date is recorded as `error`.
    Failed,
    /// 429: the pass for this user stops.
    RateLimited(Option<u64>),
    /// The refresh token was refused (`needs_login` is stored), or the refresh is to wait, or the
    /// token is refused again after a refresh: the pass for this user stops.
    Stop,
}

/// GET `url` with the user's tokens. A 401 or 403 gets one refresh (under the token lock, the
/// queue's own refresh) and one more try.
async fn get(state: &AppState, uid: i64, tokens: &mut Tokens, url: &str) -> Result<Got> {
    let g = &state.garmin;
    let mut refreshed = false;
    loop {
        let resp = match g
            .api()
            .send(Request::get(url.to_owned()).headers(auth::api_headers(tokens)))
            .await
        {
            Ok(r) => r,
            Err(_) => return Ok(Got::Failed),
        };
        match resp.status {
            200..=299 => return Ok(Got::Body(String::from_utf8_lossy(&resp.body).into_owned())),
            401 | 403 if !refreshed => {
                refreshed = true;
                match queue::refresh_tokens(state, uid, tokens).await? {
                    Refreshed::Fresh(t) => *tokens = t,
                    Refreshed::LoginExpired => return Ok(Got::Stop),
                    Refreshed::Later(s) => {
                        hold(state, uid, state.now() + s);
                        return Ok(Got::Stop);
                    }
                }
            }
            401 | 403 => return Ok(Got::Stop),
            429 => {
                return Ok(Got::RateLimited(
                    resp.header("retry-after")
                        .and_then(|v| v.trim().parse::<u64>().ok()),
                ))
            }
            _ => return Ok(Got::Failed),
        }
    }
}

fn hold(state: &AppState, uid: i64, until: i64) {
    let mut m = state
        .garmin
        .nights
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let e = m.hold_until.entry(uid).or_insert(0);
    *e = (*e).max(until);
}

/// Garmin's profile name of the user (the path part of the sleep endpoint). Memory only.
async fn display_name(state: &AppState, uid: i64, tokens: &mut Tokens) -> Result<Option<String>> {
    if let Some(n) = state
        .garmin
        .nights
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .display_names
        .get(&uid)
    {
        return Ok(Some(n.clone()));
    }
    let url = format!(
        "{}/userprofile-service/socialProfile",
        state.garmin.cfg.urls.connectapi
    );
    let body = match get(state, uid, tokens, &url).await? {
        Got::Body(b) => b,
        Got::RateLimited(ra) => {
            hold_after_429(state, uid, ra);
            return Ok(None);
        }
        Got::Failed | Got::Stop => return Ok(None),
    };
    let name = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v.get("displayName")?.as_str().map(str::to_owned))
        .filter(|n| !n.is_empty());
    if let Some(n) = &name {
        state
            .garmin
            .nights
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .display_names
            .insert(uid, n.clone());
    }
    Ok(name)
}

fn hold_after_429(state: &AppState, uid: i64, retry_after: Option<u64>) {
    if let Some(s) = retry_after {
        let s = (s as i64).clamp(1, queue::RETRY_AFTER_MAX_S);
        hold(state, uid, state.now() + s);
    }
}

/// Does this answer carry a night we can store: it classifies as `sleep_detail`, has stage
/// periods and a numeric start instant.
fn is_night(v: &Value) -> bool {
    import::classify(v) == Some(Kind::SleepDetail)
        && v.get("sleepLevels")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
        && v.pointer("/dailySleepDTO/sleepStartTimestampGMT")
            .is_some_and(Value::is_number)
}

async fn run_user(state: &AppState, uid: i64, counts: &mut Counts) -> Result<()> {
    let now = state.now();
    {
        let m = state
            .garmin
            .nights
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if m.hold_until.get(&uid).is_some_and(|&t| now < t) {
            return Ok(());
        }
    }
    let Some(account) = queue::load_account(&state.pool, uid).await? else {
        return Ok(());
    };
    if account.status != AccountStatus::Connected || !account.fill_nights {
        return Ok(());
    }
    let Some(mut tokens) = account.tokens else {
        return Ok(());
    };
    counts.users += 1;
    let tz = settings::load(&state.pool, uid).await?.tz;
    let today = settings::date_of(tz, now);
    let seen = checks(state, uid).await?;

    // Newest first: if Garmin pushes back, the freshest nights are the ones that mattered.
    let mut asked = 0usize;
    for back in 0..WINDOW_DAYS {
        let date = today - Days::days(back);
        let key = settings::date_string(date);
        if date == today && now < local_at(tz, today, TODAY_FROM_HOUR) {
            continue;
        }
        if let Some((at, outcome)) = seen.get(&key) {
            if outcome == "filled" || now - at < RECHECK_S {
                continue;
            }
        }
        if watch_has_night(state, uid, tz, date).await? {
            continue;
        }
        if asked >= MAX_REQUESTS {
            break;
        }
        // Gentle with Garmin, and the watch gets one more look after the pause.
        if asked > 0 && state.garmin.cfg.nights_pause_ms > 0 {
            tokio::time::sleep(Duration::from_millis(state.garmin.cfg.nights_pause_ms)).await;
            if watch_has_night(state, uid, tz, date).await? {
                continue;
            }
        }
        if tokens.expires_soon(state.now()) {
            match queue::refresh_tokens(state, uid, &tokens).await? {
                Refreshed::Fresh(t) => tokens = t,
                Refreshed::LoginExpired => return Ok(()),
                Refreshed::Later(s) => {
                    hold(state, uid, state.now() + s);
                    return Ok(());
                }
            }
        }
        let Some(name) = display_name(state, uid, &mut tokens).await? else {
            // Not asked yet (no name): nothing is recorded, the next pass tries again.
            return Ok(());
        };
        asked += 1;
        counts.asked += 1;
        let url = format!(
            "{}/wellness-service/wellness/dailySleepData/{}?date={key}&nonSleepBufferMinutes=60",
            state.garmin.cfg.urls.connectapi,
            http::urlencode(&name)
        );
        let outcome = match get(state, uid, &mut tokens, &url).await? {
            Got::Body(body) => store(state, uid, tz, &key, &body).await?,
            Got::Failed => Outcome::Error,
            Got::RateLimited(ra) => {
                record(state, uid, &key, Outcome::Error).await?;
                counts.errors += 1;
                hold_after_429(state, uid, ra);
                return Ok(());
            }
            Got::Stop => return Ok(()),
        };
        record(state, uid, &key, outcome).await?;
        match outcome {
            Outcome::Filled => counts.filled += 1,
            Outcome::NoNight => counts.no_night += 1,
            Outcome::Error => counts.errors += 1,
        }
    }
    Ok(())
}

/// Judge and store one answer; the importer strips the account ids and ingest derives as usual.
async fn store(state: &AppState, uid: i64, tz: Tz, date: &str, body: &str) -> Result<Outcome> {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return Ok(Outcome::Error);
    };
    if !is_night(&v) {
        return Ok(Outcome::NoNight);
    }
    let origin = format!("garmin-fetch/sleep_detail/{date}.json");
    let report = import::import_one(state, &origin, body, uid, tz).await?;
    let stored = report
        .by_kind
        .get(Kind::SleepDetail.as_str())
        .is_some_and(|(new, present)| new + present > 0);
    Ok(if stored && report.errors.is_empty() {
        Outcome::Filled
    } else {
        Outcome::Error
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_night_needs_levels_and_a_start() {
        let ok = json!({"dailySleepDTO": {"calendarDate": "2026-11-27", "sleepStartTimestampGMT": 1}, "sleepLevels": [{}]});
        assert!(is_night(&ok));
        let mut no_levels = ok.clone();
        no_levels["sleepLevels"] = json!([]);
        assert!(!is_night(&no_levels));
        let mut no_start = ok.clone();
        no_start["dailySleepDTO"]["sleepStartTimestampGMT"] = json!(null);
        assert!(!is_night(&no_start));
        assert!(!is_night(&json!({"dailySleepDTO": {}})));
    }

    #[test]
    fn local_times_follow_the_zone() {
        let tz: Tz = "Europe/Copenhagen".parse().unwrap();
        let d = NaiveDate::from_ymd_opt(2026, 11, 28).unwrap();
        assert_eq!(
            settings::rfc3339(tz, local_at(tz, d, 9)),
            "2026-11-28T09:00:00+01:00"
        );
        assert_eq!(
            settings::rfc3339(tz, local_at(tz, d - Days::days(1), 18)),
            "2026-11-27T18:00:00+01:00"
        );
    }
}
