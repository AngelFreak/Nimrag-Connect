//! The owner's JSON API (everything under `/api` except the device upload).
//!
//! Conventions: every handler takes the [`Owner`](crate::auth::Owner) extractor
//! (session cookie; a device token never passes); metric units are in the field
//! names (`distance_m`, `resting_hr_bpm`, `duration_s`); a metric that does not
//! exist is `null`, never 0; timestamps are RFC 3339 in the owner's zone, dates
//! `YYYY-MM-DD`; data responses carry `sample: true` for `dev-seed` data.
//! Request and response structs derive `ts_rs::TS`; [`export_types`] writes them
//! to `web/src/api/generated/`.

pub mod account;
pub mod activities;
pub mod body;
pub mod daily;
pub mod days;
pub mod prefs;
pub mod sleep;
pub mod strength;
pub mod system;
pub mod today;
pub mod training;
pub mod trends;

use crate::api::{ApiError, AppState};
use crate::settings::{self, Prefs};
use axum::{
    extract::{FromRequest, Request},
    Json, Router,
};
use chrono::NaiveDate;
use chrono_tz::Tz;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sqlx::SqlitePool;
use std::path::Path;
use ts_rs::{Config, TS};

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(account::routes())
        .merge(prefs::routes())
        .merge(today::routes())
        .merge(days::routes())
        .merge(activities::routes())
        .merge(strength::routes())
        .merge(sleep::routes())
        .merge(trends::routes())
        .merge(training::routes())
        .merge(body::routes())
        .merge(system::routes())
        .merge(crate::garmin::api::routes())
}

// ---- request context ----------------------------------------------------------

/// Preferences plus the pool and the current time, loaded once per request.
pub struct Ctx {
    pub pool: SqlitePool,
    pub prefs: Prefs,
    pub now: i64,
}

impl Ctx {
    pub async fn new(state: &AppState) -> Result<Self, ApiError> {
        Ok(Self {
            pool: state.pool.clone(),
            prefs: settings::load(&state.pool).await?,
            now: state.now(),
        })
    }
    pub fn tz(&self) -> Tz {
        self.prefs.tz
    }
    pub fn sample(&self) -> bool {
        self.prefs.sample
    }
    pub fn today(&self) -> NaiveDate {
        settings::date_of(self.tz(), self.now)
    }
    pub fn ts(&self, ts: i64) -> String {
        settings::rfc3339(self.tz(), ts)
    }
    pub fn opt_ts(&self, ts: Option<i64>) -> Option<String> {
        ts.map(|t| self.ts(t))
    }
    pub fn date(&self, ts: i64) -> NaiveDate {
        settings::date_of(self.tz(), ts)
    }
    pub fn day_bounds(&self, d: NaiveDate) -> (i64, i64) {
        settings::day_bounds(self.tz(), d)
    }
    /// Parse a `{date}` path segment.
    pub fn parse_date(s: &str) -> Result<NaiveDate, ApiError> {
        settings::parse_date(s).ok_or_else(|| ApiError::bad_request("date must be YYYY-MM-DD"))
    }
}

/// JSON body extractor whose rejections are the API's own JSON errors (400).
pub struct ApiJson<T>(pub T);

impl<T, S> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;
    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        Json::<T>::from_request(req, state)
            .await
            .map(|Json(v)| Self(v))
            .map_err(|e| match e.status() {
                axum::http::StatusCode::PAYLOAD_TOO_LARGE => {
                    ApiError::new(axum::http::StatusCode::PAYLOAD_TOO_LARGE, "body too large")
                }
                _ => ApiError::bad_request(e.body_text()),
            })
    }
}

/// A value at a time (RFC 3339).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct TimedValue {
    pub time: String,
    pub value: Option<f64>,
}

/// A value on a local date (`YYYY-MM-DD`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct DateValue {
    pub date: String,
    pub value: Option<f64>,
}

pub fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

pub fn mean(v: impl IntoIterator<Item = f64>) -> Option<f64> {
    let (s, n) = v.into_iter().fold((0.0, 0u32), |(s, n), x| (s + x, n + 1));
    (n > 0).then(|| round1(s / f64::from(n)))
}

/// `(ts, value)` rows from a static query taking `(from, to)`.
pub async fn series(
    pool: &SqlitePool,
    sql: &'static str,
    from: i64,
    to: i64,
) -> Result<Vec<(i64, f64)>, ApiError> {
    Ok(sqlx::query_as::<_, (i64, f64)>(sql)
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await?)
}

/// Activity kinds the UI filters and labels by.
pub fn kind_of(sport: Option<i64>, sub_sport: Option<i64>) -> &'static str {
    match (sport, sub_sport) {
        (Some(10), Some(20)) | (Some(4), Some(20)) => "strength",
        (Some(1), _) => "run",
        (Some(11), _) => "walk",
        (Some(17), _) => "hike",
        (Some(2), _) => "cycle",
        (Some(5), _) => "swim",
        (Some(10), _) => "training",
        _ => "other",
    }
}

// ---- generated TypeScript ----------------------------------------------------------

macro_rules! export_all_types {
    ($cfg:expr, $($t:ty),+ $(,)?) => {{
        let mut names: Vec<String> = Vec::new();
        $(
            <$t as TS>::export_all($cfg)?;
            names.push(<$t as TS>::ident($cfg));
        )+
        names
    }};
}

/// Write the TypeScript for every request and response type (and what they
/// reference) into `dir`, plus an `index.ts` re-exporting the top-level ones.
/// Files are rewritten only when their content differs.
pub fn export_types(dir: &Path) -> anyhow::Result<()> {
    use crate::garmin::api as garmin;
    use crate::web_api::*;
    std::fs::create_dir_all(dir)?;
    let cfg = Config::new()
        .with_large_int("number")
        .with_out_dir(dir.to_path_buf());
    let mut names = export_all_types!(
        &cfg,
        account::Me,
        account::LoginRequest,
        account::TokenList,
        account::NewToken,
        account::NewTokenResponse,
        prefs::SettingsResponse,
        prefs::SettingsPatch,
        today::Today,
        days::DayResponse,
        days::MovementResponse,
        days::CalendarResponse,
        activities::ActivityList,
        activities::ActivityDetail,
        activities::TrackResponse,
        strength::StrengthResponse,
        strength::StrengthPatch,
        sleep::SleepNightResponse,
        sleep::SleepNightsResponse,
        trends::TrendsResponse,
        training::TrainingResponse,
        body::BodyResponse,
        body::NewBodyEntry,
        body::BodyEntry,
        system::DevicesResponse,
        system::StorageResponse,
        garmin::GarminStatus,
        garmin::GarminLoginRequest,
        garmin::GarminLoginResponse,
        garmin::GarminMfaRequest,
        garmin::GarminBackfillRequest,
        garmin::GarminBackfillResponse,
        garmin::GarminImportTokens,
        crate::settings::HomeCard,
        crate::settings::HomeCards,
    );
    names.sort();
    names.dedup();
    let mut index = String::from(
        "// Generated by `nimrag-server export-types`; do not edit.\n// Every request and response type of the JSON API.\n",
    );
    for n in &names {
        index.push_str(&format!("export type {{ {n} }} from \"./{n}\";\n"));
    }
    std::fs::write(dir.join("index.ts"), index)?;
    Ok(())
}
