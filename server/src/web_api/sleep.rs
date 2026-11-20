//! `GET /api/sleep/{date}` (one night plus the 7 nights up to it) and
//! `GET /api/sleep?nights=7|14|30`.
//!
//! A night belongs to the local date it began on (a start before 06:00 counts for
//! the day before), so "night of Wed 18 Nov" runs from Wednesday evening to Thursday.

use super::daily::{
    night_detail, nights, session_for_night, session_last_night, NightSummary, SleepNight,
};
use super::{mean, round1, Ctx};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::settings::date_string;
use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use chrono::Duration;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/sleep", get(list))
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
