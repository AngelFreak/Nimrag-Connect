//! `GET /api/sleep/{date}` (one night plus the 7 nights up to it),
//! `GET /api/sleep?nights=7|14|30` and `GET /api/sleep/history?from&to&group=day|week|month`
//! (scores and time asleep over a range, aggregated).
//!
//! A night belongs to the local date it began on (a start before 06:00 counts for
//! the day before), so "night of Wed 18 Nov" runs from Wednesday evening to Thursday.

use super::daily::{
    dates, night_detail, nights, session_for_night, session_last_night, week_start, NightSummary,
    SleepNight,
};
use super::{mean, round1, Ctx, RangeParams};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::settings::date_string;
use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/sleep", get(list))
        .route("/api/sleep/history", get(history))
        .route("/api/sleep/{date}", get(night))
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct SleepNightResponse {
    pub sample: bool,
    pub date: String,
    /// `null` when the watch recorded no sleep that night.
    pub night: Option<SleepNight>,
    /// The 7 nights up to and including `date` (nights without data are left out).
    pub last_nights: Vec<NightSummary>,
    pub average_duration_s: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct SleepNightsResponse {
    pub sample: bool,
    pub nights: Vec<NightSummary>,
    pub average_duration_s: Option<f64>,
    pub average_score: Option<f64>,
}

async fn night(
    user: DataUser,
    State(state): State<AppState>,
    Path(date): Path<String>,
) -> Res<Json<SleepNightResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let d = Ctx::parse_date(&date)?;
    let night = match session_for_night(&ctx, d).await? {
        Some(s) => Some(night_detail(&ctx, &s).await?),
        None => None,
    };
    let last = nights(&ctx, d - Duration::days(6), d).await?;
    Ok(Json(SleepNightResponse {
        sample: ctx.sample(),
        date: date_string(d),
        night,
        average_duration_s: mean(last.iter().map(|n| n.duration_s)).map(f64::round),
        last_nights: last,
    }))
}

#[derive(Debug, Deserialize)]
struct ListParams {
    nights: Option<u32>,
}

async fn list(
    user: DataUser,
    State(state): State<AppState>,
    Query(p): Query<ListParams>,
) -> Res<Json<SleepNightsResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let n = p.nights.unwrap_or(7);
    if ![7, 14, 30].contains(&n) {
        return Err(ApiError::bad_request("nights must be 7, 14 or 30"));
    }
    let yesterday = ctx.today() - Duration::days(1);
    let end = match session_last_night(&ctx).await? {
        Some(s) => super::daily::night_date(&ctx, s.start_ts)
            .max(yesterday)
            .min(ctx.today()),
        None => yesterday,
    };
    let list = nights(&ctx, end - Duration::days(i64::from(n) - 1), end).await?;
    Ok(Json(SleepNightsResponse {
        sample: ctx.sample(),
        average_duration_s: mean(list.iter().map(|x| x.duration_s)).map(f64::round),
        average_score: mean(list.iter().filter_map(|x| x.score)).map(round1),
        nights: list,
    }))
}

// ---- GET /api/sleep/history -----------------------------------------------------------------

/// A night this long or longer counts as a long night (green in the duration chart).
pub const LONG_NIGHT_S: f64 = 8.0 * 3600.0;

/// One bucket of the history: a night (`group=day`), a calendar week from Monday or a calendar
/// month, clipped to the range. Buckets without a night have `nights: 0` and every value `null`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SleepBucket {
    /// First and last local date of the bucket inside the range.
    pub from: String,
    pub to: String,
    pub nights: u32,
    pub avg_score: Option<f64>,
    pub high_score: Option<f64>,
    pub low_score: Option<f64>,
    /// Mean time asleep of the bucket's nights.
    pub avg_duration_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SleepHistoryStats {
    pub nights: u32,
    /// Nights with a score (a night can be recorded without one).
    pub scored_nights: u32,
    /// Mean of all scored nights.
    pub avg_score: Option<f64>,
    /// Best and worst single night.
    pub highest_score: Option<f64>,
    pub lowest_score: Option<f64>,
    /// Mean over the buckets of each bucket's highest and lowest score. With `group=day` every
    /// bucket is one night, so these equal `avg_score`: the UI shows them for week and month only.
    pub avg_highest: Option<f64>,
    pub avg_lowest: Option<f64>,
    pub avg_duration_s: Option<f64>,
    /// Nights of at least `long_night_s`.
    pub long_nights: u32,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct SleepHistory {
    pub sample: bool,
    pub from: String,
    pub to: String,
    pub today: String,
    /// `day`, `week` or `month`.
    pub group: String,
    /// Time asleep from which a night counts as long.
    pub long_night_s: f64,
    /// Every bucket of the range, oldest first.
    pub buckets: Vec<SleepBucket>,
    pub stats: SleepHistoryStats,
}

#[derive(Debug, Deserialize)]
struct HistoryParams {
    from: Option<String>,
    to: Option<String>,
    group: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Day,
    Week,
    Month,
}

impl Group {
    /// First day of the bucket `d` falls in.
    fn key(self, d: NaiveDate) -> NaiveDate {
        match self {
            Group::Day => d,
            Group::Week => week_start(d),
            Group::Month => d.with_day(1).unwrap_or(d),
        }
    }
}

async fn history(
    user: DataUser,
    State(state): State<AppState>,
    Query(p): Query<HistoryParams>,
) -> Res<Json<SleepHistory>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let group_name = p.group.unwrap_or_else(|| "day".into());
    let group = match group_name.as_str() {
        "day" => Group::Day,
        "week" => Group::Week,
        "month" => Group::Month,
        _ => return Err(ApiError::bad_request("group must be day, week or month")),
    };
    let (from, to) = ctx.range(&RangeParams {
        from: p.from,
        to: p.to,
    })?;
    // Time asleep and score come from the same night summaries as the night views.
    let list = nights(&ctx, from, to).await?;
    let mut by_date: BTreeMap<NaiveDate, &NightSummary> = BTreeMap::new();
    for n in &list {
        if let Some(d) = crate::settings::parse_date(&n.date) {
            by_date.insert(d, n);
        }
    }

    let mut buckets: Vec<SleepBucket> = Vec::new();
    let mut members: Vec<Vec<&NightSummary>> = Vec::new();
    let mut key: Option<NaiveDate> = None;
    for d in dates(from, to) {
        if key != Some(group.key(d)) {
            key = Some(group.key(d));
            buckets.push(SleepBucket {
                from: date_string(d),
                to: date_string(d),
                nights: 0,
                avg_score: None,
                high_score: None,
                low_score: None,
                avg_duration_s: None,
            });
            members.push(Vec::new());
        }
        if let (Some(b), Some(m)) = (buckets.last_mut(), members.last_mut()) {
            b.to = date_string(d);
            if let Some(n) = by_date.get(&d) {
                m.push(n);
            }
        }
    }
    for (b, m) in buckets.iter_mut().zip(&members) {
        let scores = || m.iter().filter_map(|n| n.score);
        b.nights = u32::try_from(m.len()).unwrap_or(u32::MAX);
        b.avg_score = mean(scores()).map(round1);
        b.high_score = scores().reduce(f64::max);
        b.low_score = scores().reduce(f64::min);
        b.avg_duration_s = mean(m.iter().map(|n| n.duration_s)).map(f64::round);
    }

    let scores = || list.iter().filter_map(|n| n.score);
    let stats = SleepHistoryStats {
        nights: u32::try_from(list.len()).unwrap_or(u32::MAX),
        scored_nights: u32::try_from(scores().count()).unwrap_or(u32::MAX),
        avg_score: mean(scores()).map(round1),
        highest_score: scores().reduce(f64::max),
        lowest_score: scores().reduce(f64::min),
        avg_highest: mean(buckets.iter().filter_map(|b| b.high_score)).map(round1),
        avg_lowest: mean(buckets.iter().filter_map(|b| b.low_score)).map(round1),
        avg_duration_s: mean(list.iter().map(|n| n.duration_s)).map(f64::round),
        long_nights: u32::try_from(list.iter().filter(|n| n.duration_s >= LONG_NIGHT_S).count())
            .unwrap_or(u32::MAX),
    };
    Ok(Json(SleepHistory {
        sample: ctx.sample(),
        from: date_string(from),
        to: date_string(to),
        today: date_string(ctx.today()),
        group: group_name,
        long_night_s: LONG_NIGHT_S,
        buckets,
        stats,
    }))
}
