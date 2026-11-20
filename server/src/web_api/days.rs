//! `GET /api/days/{date}`, `GET /api/movement/{date}`, `GET /api/calendar/{yyyy-mm}`.

use super::activities::{list_items, ActivityListItem};
use super::daily::{
    connect_day, dates, day_stats, energy_block, heart_rate_block, intensity_days, intensity_total,
    nights, stress_block, week_start, EnergyBlock, HeartRateBlock, NightSummary, StressBlock,
};
use super::metrics::{day_breathing, day_spo2, DayBreathing, DaySpo2};
use super::{Ctx, DateValue};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::settings::{date_string, parse_date};
use axum::{
    extract::{Path, State},
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
        .route("/api/days/{date}", get(day))
        .route("/api/movement/{date}", get(movement))
        .route("/api/calendar/{month}", get(calendar))
}

// ---- day -----------------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct DayResponse {
    pub sample: bool,
    pub date: String,
    pub previous_date: String,
    /// `null` for today (and the future).
    pub next_date: Option<String>,
    pub heart_rate: HeartRateBlock,
    pub stress: StressBlock,
    pub energy: EnergyBlock,
    pub steps: Option<f64>,
    pub step_goal: u32,
    pub intensity_minutes: Option<f64>,
    pub breathing: DayBreathing,
    pub spo2: DaySpo2,
    /// The night that ended on this date (the morning of waking up).
    pub sleep: Option<NightSummary>,
    pub activities: Vec<ActivityListItem>,
}

pub async fn activities_on(ctx: &Ctx, d: NaiveDate) -> Res<Vec<ActivityListItem>> {
    let (from, to) = ctx.day_bounds(d);
    let mut v = list_items(ctx, None, Some(from), Some(to), 100).await?;
    v.reverse(); // oldest first within the day
    Ok(v)
}

async fn day(
    user: DataUser,
    State(state): State<AppState>,
    Path(date): Path<String>,
) -> Res<Json<DayResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let d = Ctx::parse_date(&date)?;
    let (from, to) = ctx.day_bounds(d);
    let stats = day_stats(&ctx, d, d).await?.remove(&d).unwrap_or_default();
    Ok(Json(DayResponse {
        sample: ctx.sample(),
        date: date_string(d),
        previous_date: date_string(d - Duration::days(1)),
        next_date: (d < ctx.today()).then(|| date_string(d + Duration::days(1))),
        heart_rate: heart_rate_block(&ctx, d).await?,
        stress: stress_block(&ctx, from, to).await?,
        energy: energy_block(&ctx, d).await?,
        steps: stats.steps,
        step_goal: ctx.prefs.step_goal,
        intensity_minutes: intensity_total(stats.moderate_min, stats.vigorous_min),
        breathing: day_breathing(&ctx, d).await?,
        spo2: day_spo2(&ctx, d).await?,
        sleep: nights(&ctx, d, d).await?.into_iter().next(),
        activities: activities_on(&ctx, d).await?,
    }))
}

// ---- movement ------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct IntensityDay {
    pub date: String,
    pub moderate_min: Option<f64>,
    pub vigorous_min: Option<f64>,
    /// Moderate + 2 x vigorous.
    pub total_min: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct IntensityWeek {
    pub week_start: String,
    pub moderate_min: Option<f64>,
    pub vigorous_min: Option<f64>,
    /// Moderate + 2 x vigorous (vigorous minutes count double).
    pub total_min: Option<f64>,
    pub goal_min: u32,
    /// Monday to Sunday.
    pub days: Vec<IntensityDay>,
}

pub async fn intensity_week(ctx: &Ctx, date: NaiveDate) -> Res<IntensityWeek> {
    let start = week_start(date);
    let end = start + Duration::days(6);
    let per_day = intensity_days(ctx, start, end).await?;
    let sum = |vals: Vec<f64>| (!vals.is_empty()).then(|| vals.iter().sum::<f64>());
    let m = sum(per_day.values().filter_map(|x| x.0).collect());
    let v = sum(per_day.values().filter_map(|x| x.1).collect());
    Ok(IntensityWeek {
        week_start: date_string(start),
        moderate_min: m,
        vigorous_min: v,
        total_min: intensity_total(m, v),
        goal_min: ctx.prefs.intensity_goal_min,
        days: per_day
            .iter()
            .map(|(d, (m, v))| IntensityDay {
                date: date_string(*d),
                moderate_min: *m,
                vigorous_min: *v,
                total_min: intensity_total(*m, *v),
            })
            .collect(),
    })
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct Calories {
    pub active_kcal: Option<f64>,
    /// The watch's resting metabolic rate for the day.
    pub resting_kcal: Option<f64>,
    pub total_kcal: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct MovementResponse {
    pub sample: bool,
    pub date: String,
    pub week_number: u32,
    pub steps: Option<f64>,
    pub step_goal: u32,
    /// Steps as a percentage of the goal (not capped).
    pub percent_of_goal: Option<f64>,
    pub distance_m: Option<f64>,
    /// Floors climbed, from Garmin Connect's daily summary (the watch has no table for them):
    /// `null` for a day without an imported summary.
    pub floors: Option<f64>,
    /// Monday to Sunday of the week containing `date`; days after `date` are `null`.
    pub week_steps: Vec<DateValue>,
    pub week_average_steps: Option<f64>,
    pub intensity: IntensityWeek,
    pub calories: Calories,
}

pub async fn resting_kcal(ctx: &Ctx, d: NaiveDate) -> Res<Option<f64>> {
    if let Some(v) = watch_resting_kcal(ctx, d).await? {
        return Ok(Some(v));
    }
    // No watch settings: the day's total imported from Garmin Connect, if any.
    super::daily::one_f64(
        ctx,
        "SELECT resting_kcal FROM daily_totals WHERE user_id = ? AND ts >= ? AND ts < ?
         AND resting_kcal IS NOT NULL LIMIT 1",
        &[ctx.day_bounds(d).0, ctx.day_bounds(d).1],
    )
    .await
}

async fn watch_resting_kcal(ctx: &Ctx, d: NaiveDate) -> Res<Option<f64>> {
    super::daily::one_f64(
        ctx,
        "SELECT m.rmr_kcal_day FROM mon_info m JOIN sources s ON s.sha256 = m.sha256
         JOIN file_owners o ON o.sha256 = m.sha256 AND o.user_id = ?
         WHERE m.rmr_kcal_day IS NOT NULL
         ORDER BY CASE WHEN s.last_ts < ? THEN 0 ELSE 1 END,
                  CASE WHEN s.last_ts < ? THEN -s.last_ts ELSE s.last_ts END LIMIT 1",
        &[ctx.day_bounds(d).1, ctx.day_bounds(d).1],
    )
    .await
}

async fn movement(
    user: DataUser,
    State(state): State<AppState>,
    Path(date): Path<String>,
) -> Res<Json<MovementResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let d = Ctx::parse_date(&date)?;
    let start = week_start(d);
    let stats = day_stats(&ctx, start, start + Duration::days(6)).await?;
    let today = stats.get(&d).cloned().unwrap_or_default();
    let resting = resting_kcal(&ctx, d).await?;
    let goal = f64::from(ctx.prefs.step_goal);
    let known: Vec<f64> = stats.range(..=d).filter_map(|(_, s)| s.steps).collect();
    Ok(Json(MovementResponse {
        sample: ctx.sample(),
        date: date_string(d),
        week_number: d.iso_week().week(),
        steps: today.steps,
        step_goal: ctx.prefs.step_goal,
        percent_of_goal: today.steps.map(|s| (s / goal * 1000.0).round() / 10.0),
        distance_m: today.distance_m,
        floors: connect_day(&ctx, d).await?.and_then(|c| c.floors_up),
        week_steps: stats
            .iter()
            .map(|(day, s)| DateValue {
                date: date_string(*day),
                value: if *day <= d { s.steps } else { None },
            })
            .collect(),
        week_average_steps: (!known.is_empty())
            .then(|| (known.iter().sum::<f64>() / known.len() as f64).round()),
        intensity: intensity_week(&ctx, d).await?,
        calories: Calories {
            active_kcal: today.active_kcal,
            resting_kcal: resting,
            total_kcal: today.active_kcal.zip(resting).map(|(a, r)| a + r),
        },
    }))
}

// ---- calendar -------------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct CalendarDay {
    pub date: String,
    pub activities: Vec<ActivityListItem>,
    pub steps: Option<f64>,
    /// The night that ended on this date (the morning of waking up).
    pub sleep_duration_s: Option<f64>,
    pub sleep_score: Option<f64>,
    pub resting_hr_bpm: Option<f64>,
    pub energy_high: Option<f64>,
    pub energy_low: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct CalendarTotals {
    pub activities: u32,
    pub strength_sessions: u32,
    /// Every activity that is not a strength session.
    pub cardio_sessions: u32,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct CalendarResponse {
    pub sample: bool,
    /// `YYYY-MM`.
    pub month: String,
    pub days: Vec<CalendarDay>,
    pub totals: CalendarTotals,
}

async fn calendar(
    user: DataUser,
    State(state): State<AppState>,
    Path(month): Path<String>,
) -> Res<Json<CalendarResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let first = parse_date(&format!("{month}-01"))
        .ok_or_else(|| ApiError::bad_request("month must be YYYY-MM"))?;
    let next_month = if first.month() == 12 {
        NaiveDate::from_ymd_opt(first.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1)
    }
    .ok_or_else(|| ApiError::bad_request("month out of range"))?;
    let last = next_month - Duration::days(1);
    let stats = day_stats(&ctx, first, last).await?;
    let night_list = nights(&ctx, first, last).await?;
    let acts = list_items(
        &ctx,
        None,
        Some(ctx.day_bounds(first).0),
        Some(ctx.day_bounds(next_month).0),
        1000,
    )
    .await?;
    let mut by_day: BTreeMap<String, Vec<ActivityListItem>> = BTreeMap::new();
    for a in acts.iter().rev() {
        by_day.entry(a.date.clone()).or_default().push(a.clone());
    }
    let days = dates(first, last)
        .map(|d| {
            let key = date_string(d);
            let s = stats.get(&d).cloned().unwrap_or_default();
            let n = night_list.iter().find(|n| n.date == key);
            CalendarDay {
                activities: by_day.remove(&key).unwrap_or_default(),
                steps: s.steps,
                sleep_duration_s: n.map(|n| n.duration_s),
                sleep_score: n.and_then(|n| n.score),
                resting_hr_bpm: s.resting_hr,
                energy_high: s.energy.high,
                energy_low: s.energy.low,
                date: key,
            }
        })
        .collect();
    let strength = acts.iter().filter(|a| a.kind == "strength").count() as u32;
    Ok(Json(CalendarResponse {
        sample: ctx.sample(),
        month: format!("{:04}-{:02}", first.year(), first.month()),
        days,
        totals: CalendarTotals {
            activities: acts.len() as u32,
            strength_sessions: strength,
            cardio_sessions: acts.len() as u32 - strength,
        },
    }))
}
