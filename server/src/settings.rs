//! Settings and time-zone helpers.
//!
//! Server-wide flags (`sample_data`) live in `settings`; each user's preferences (`timezone`,
//! `step_goal`, `intensity_goal_min`, `sleep_need_min`, `home_cards`) in `user_settings`, keyed by user id (0 =
//! the pre-multi-user values, until the admin adopts them for a user).
//!
//! Days are local days in the user's time zone (`timezone`, IANA name; default
//! the server's system zone, else UTC). All timestamps leave the API as RFC 3339
//! with the zone's offset; dates are `YYYY-MM-DD`.

use anyhow::Result;
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, SecondsFormat, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use ts_rs::TS;

pub const DEFAULT_STEP_GOAL: u32 = 8000;
pub const DEFAULT_INTENSITY_GOAL_MIN: u32 = 150;
pub const DEFAULT_SLEEP_NEED_MIN: u32 = 480;

/// Cards of the Today page's "At a glance" grid. The only definition of the id
/// set: the web app imports the generated type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HomeCard {
    Energy,
    HeartRate,
    Stress,
    Sleep,
    Hrv,
    TrainingStatus,
    IntensityMinutes,
    TrainingLoad,
    Steps,
    RecoveryTime,
    Vo2Max,
    Respiration,
    Spo2,
    Weight,
    TrainingReadiness,
}

impl HomeCard {
    /// Every card, in the default order.
    pub const ALL: [HomeCard; 15] = [
        HomeCard::Energy,
        HomeCard::HeartRate,
        HomeCard::Stress,
        HomeCard::Sleep,
        HomeCard::Hrv,
        HomeCard::TrainingStatus,
        HomeCard::IntensityMinutes,
        HomeCard::TrainingLoad,
        HomeCard::Steps,
        HomeCard::RecoveryTime,
        HomeCard::Vo2Max,
        HomeCard::Respiration,
        HomeCard::Spo2,
        HomeCard::Weight,
        HomeCard::TrainingReadiness,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HomeCards {
    /// Card ids in display order (always holds every card once).
    pub order: Vec<HomeCard>,
    /// Cards the owner switched off (or that start off because they have no data yet).
    pub hidden: Vec<HomeCard>,
}

impl Default for HomeCards {
    fn default() -> Self {
        Self {
            order: HomeCard::ALL.to_vec(),
            // No data on the Instinct 2 or not entered yet: off until the owner turns them on.
            hidden: vec![
                HomeCard::Respiration,
                HomeCard::Spo2,
                HomeCard::Weight,
                HomeCard::TrainingReadiness,
            ],
        }
    }
}

impl HomeCards {
    /// Reject duplicates; cards missing from `order` are appended in default order
    /// so a card never disappears because a client sent a short list.
    pub fn normalised(mut self) -> Result<Self, String> {
        let mut seen = std::collections::HashSet::new();
        if let Some(d) = self.order.iter().find(|c| !seen.insert(**c)) {
            return Err(format!("order lists {d:?} twice"));
        }
        let mut seen = std::collections::HashSet::new();
        if let Some(d) = self.hidden.iter().find(|c| !seen.insert(**c)) {
            return Err(format!("hidden lists {d:?} twice"));
        }
        for c in HomeCard::ALL {
            if !self.order.contains(&c) {
                self.order.push(c);
            }
        }
        Ok(self)
    }
}

/// Accepted range of `step_goal` (steps a day).
pub const STEP_GOAL_RANGE: std::ops::RangeInclusive<u32> = 1..=200_000;
/// Accepted range of `intensity_goal_min` (minutes a week).
pub const INTENSITY_GOAL_RANGE: std::ops::RangeInclusive<u32> = 1..=10_080;
/// Accepted range of `sleep_need_min` (minutes a night), in steps of [`SLEEP_NEED_STEP_MIN`].
pub const SLEEP_NEED_RANGE: std::ops::RangeInclusive<u32> = 360..=600;
pub const SLEEP_NEED_STEP_MIN: u32 = 15;

/// Is this a sleep need the setting accepts (6 to 10 hours in 15 minute steps)?
pub fn sleep_need_ok(minutes: u32) -> bool {
    SLEEP_NEED_RANGE.contains(&minutes) && minutes % SLEEP_NEED_STEP_MIN == 0
}

/// A stored user setting checked the way `PATCH /api/settings` checks it, in the form it is
/// stored in. `None` for a key that is not one of the five, or a value that would be refused.
pub fn checked_user_setting(key: &str, value: &str) -> Option<String> {
    match key {
        "timezone" => value.parse::<Tz>().ok().map(|tz| tz.name().to_owned()),
        "step_goal" => value
            .parse::<u32>()
            .ok()
            .filter(|g| STEP_GOAL_RANGE.contains(g))
            .map(|g| g.to_string()),
        "intensity_goal_min" => value
            .parse::<u32>()
            .ok()
            .filter(|g| INTENSITY_GOAL_RANGE.contains(g))
            .map(|g| g.to_string()),
        "sleep_need_min" => value
            .parse::<u32>()
            .ok()
            .filter(|g| sleep_need_ok(*g))
            .map(|g| g.to_string()),
        "home_cards" => serde_json::from_str::<HomeCards>(value)
            .ok()
            .and_then(|c| c.normalised().ok())
            .and_then(|c| serde_json::to_string(&c).ok()),
        _ => None,
    }
}

pub async fn get(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn set(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

/// Store one of a user's preferences.
pub async fn set_user(pool: &SqlitePool, uid: i64, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO user_settings (user_id, key, value) VALUES (?, ?, ?)
         ON CONFLICT (user_id, key) DO UPDATE SET value = excluded.value",
    )
    .bind(uid)
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

/// The server's own zone, UTC when it cannot be determined.
pub fn system_timezone() -> Tz {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(Tz::UTC)
}

/// Everything a handler needs about a user's preferences.
#[derive(Debug, Clone)]
pub struct Prefs {
    pub tz: Tz,
    pub step_goal: u32,
    pub intensity_goal_min: u32,
    pub sleep_need_min: u32,
    pub home_cards: HomeCards,
    /// True when the database was filled by `dev-seed`.
    pub sample: bool,
}

/// The preferences of user `uid` (defaults for anything not set) and the server-wide flags.
pub async fn load(pool: &SqlitePool, uid: i64) -> Result<Prefs> {
    let mut rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM user_settings WHERE user_id = ?")
            .bind(uid)
            .fetch_all(pool)
            .await?;
    let globals: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
        .fetch_all(pool)
        .await?;
    rows.extend(globals);
    let find = |k: &str| {
        rows.iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    };
    Ok(Prefs {
        tz: find("timezone")
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(system_timezone),
        step_goal: find("step_goal")
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_STEP_GOAL),
        intensity_goal_min: find("intensity_goal_min")
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_INTENSITY_GOAL_MIN),
        sleep_need_min: find("sleep_need_min")
            .and_then(|v| v.parse().ok())
            .filter(|g| sleep_need_ok(*g))
            .unwrap_or(DEFAULT_SLEEP_NEED_MIN),
        home_cards: find("home_cards")
            .and_then(|v| serde_json::from_str::<HomeCards>(v).ok())
            .and_then(|c| c.normalised().ok())
            .unwrap_or_default(),
        sample: find("sample_data") == Some("1"),
    })
}

// ---- time helpers -----------------------------------------------------------

/// Unix seconds of local midnight starting `date` (a skipped midnight, as in
/// some zones' DST changes, resolves to the first valid instant of the day).
pub fn day_start(tz: Tz, date: NaiveDate) -> i64 {
    let naive = date.and_time(NaiveTime::MIN);
    for extra in 0..=3 {
        let t = naive + Duration::hours(extra);
        if let Some(dt) = tz.from_local_datetime(&t).earliest() {
            return dt.timestamp();
        }
    }
    naive.and_utc().timestamp()
}

/// `[start, end)` unix seconds of a local day.
pub fn day_bounds(tz: Tz, date: NaiveDate) -> (i64, i64) {
    (day_start(tz, date), day_start(tz, date + Duration::days(1)))
}

pub fn date_of(tz: Tz, ts: i64) -> NaiveDate {
    DateTime::<Utc>::from_timestamp(ts, 0)
        .unwrap_or_default()
        .with_timezone(&tz)
        .date_naive()
}

/// RFC 3339 with the owner's offset, e.g. `2026-11-19T07:42:00+01:00`.
pub fn rfc3339(tz: Tz, ts: i64) -> String {
    DateTime::<Utc>::from_timestamp(ts, 0)
        .unwrap_or_default()
        .with_timezone(&tz)
        .to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// Re-render a stored UTC text timestamp (`2026-11-19T06:42:00.000Z`) in the owner's zone.
pub fn stored_rfc3339(tz: Tz, stored: &str) -> String {
    DateTime::parse_from_rfc3339(stored)
        .map(|d| rfc3339(tz, d.timestamp()))
        .unwrap_or_else(|_| stored.to_owned())
}

/// Strict `YYYY-MM-DD` (chrono alone also accepts `2026-1-5`).
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    if s.len() != 10 {
        return None;
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// Unix seconds of 12:00 local time on `date` (DST-safe, unlike midnight + 12 h).
pub fn local_noon(tz: Tz, date: NaiveDate) -> i64 {
    let noon = date.and_time(NaiveTime::from_hms_opt(12, 0, 0).expect("valid time"));
    tz.from_local_datetime(&noon)
        .earliest()
        .map_or_else(|| day_start(tz, date) + 12 * 3600, |d| d.timestamp())
}

pub fn date_string(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_need_is_six_to_ten_hours_in_quarter_hours() {
        assert!([360, 375, 480, 585, 600].into_iter().all(sleep_need_ok));
        assert!(![0, 345, 361, 481, 605, 615].into_iter().any(sleep_need_ok));
        assert_eq!(
            checked_user_setting("sleep_need_min", "450").as_deref(),
            Some("450")
        );
        assert_eq!(checked_user_setting("sleep_need_min", "451"), None);
        assert_eq!(checked_user_setting("sleep_need_min", "eight"), None);
        assert_eq!(DEFAULT_SLEEP_NEED_MIN, 480);
    }

    #[test]
    fn default_cards_hold_every_card_once() {
        let d = HomeCards::default();
        assert_eq!(d.order.len(), HomeCard::ALL.len());
        assert_eq!(d.clone().normalised().unwrap(), d);
    }

    #[test]
    fn day_bounds_follow_dst() {
        let tz: Tz = "Europe/Copenhagen".parse().unwrap();
        let d = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap(); // clocks go back
        let (s, e) = day_bounds(tz, d);
        assert_eq!(e - s, 25 * 3600);
        assert_eq!(rfc3339(tz, s), "2026-10-25T00:00:00+02:00");
    }
}
