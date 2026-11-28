//! `GET /api/hrv?from&to`: the HRV status day by day over a range (the watch's summary rows, from
//! the watch or from an imported history), with the personal "balanced" range, the overnight
//! averages and, for a single day, the readings of that night.
//!
//! Which day a night belongs to: the summary row's own local date (the watch writes it after
//! waking), and for the individual readings the local date of `ts + 12 h`, so a night from 22:00
//! to 07:00 counts for the morning it ends on.

use super::daily::{dates, hrv_status_label};
use super::{mean, round1, Ctx, RangeParams, TimedValue};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::settings::date_string;
use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

const NIGHT_SHIFT_S: i64 = 12 * 3600;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/hrv", get(hrv))
}

/// How the UI marks an HRV status: green dot, orange square, red triangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HrvStatusKind {
    Balanced,
    Unbalanced,
    Low,
    Poor,
}

/// FIT `hrv_status` as the derive code stores it (`hrv_summary.status`): 1 poor, 2 low,
/// 3 unbalanced, 4 balanced (0 is "none"). These numbers come from the FIT profile's enum and
/// the existing Today card and tests; no real HRV file has confirmed them yet (the parser is
/// marked unverified), so they are as verified as the rest of the HRV path and no more.
fn status_kind(code: i64) -> Option<HrvStatusKind> {
    Some(match code {
        1 => HrvStatusKind::Poor,
        2 => HrvStatusKind::Low,
        3 => HrvStatusKind::Unbalanced,
        4 => HrvStatusKind::Balanced,
        _ => return None,
    })
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// One local day. A day without any reading has every field `null`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct HrvDay {
    pub date: String,
    /// Raw `hrv_summary.status`.
    pub status_code: Option<i32>,
    /// `null` for a missing status and for a code that is not one of the four names.
    pub status: Option<HrvStatusKind>,
    /// "Balanced", "Unbalanced", "Low" or "Poor"; `null` like `status`.
    pub status_label: Option<String>,
    /// The watch's 7-day average of overnight HRV.
    pub weekly_avg_ms: Option<f64>,
    /// Overnight average: the watch's last-night average, else the mean of that night's readings.
    pub last_night_ms: Option<f64>,
    pub last_night_5min_high_ms: Option<f64>,
    /// The personal "balanced" range in force that day.
    pub baseline_low_ms: Option<f64>,
    pub baseline_high_ms: Option<f64>,
}

impl HrvDay {
    fn empty(date: NaiveDate) -> Self {
        Self {
            date: date_string(date),
            status_code: None,
            status: None,
            status_label: None,
            weekly_avg_ms: None,
            last_night_ms: None,
            last_night_5min_high_ms: None,
            baseline_low_ms: None,
            baseline_high_ms: None,
        }
    }
}

/// The readings of one night (only for a one-day range).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct HrvNight {
    pub readings: Vec<TimedValue>,
    pub avg_ms: Option<f64>,
    pub low_ms: Option<f64>,
    pub high_ms: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct HrvRange {
    pub sample: bool,
    pub from: String,
    pub to: String,
    pub today: String,
    /// Every day of the range, oldest first.
    pub days: Vec<HrvDay>,
    /// Days of the range with an overnight value.
    pub nights: u32,
    /// Mean of the overnight values in the range.
    pub overnight_avg_ms: Option<f64>,
    /// The balanced range of the latest day in the range that has one.
    pub baseline_low_ms: Option<f64>,
    pub baseline_high_ms: Option<f64>,
    /// Readings of the night, when the range is a single day and readings exist.
    pub night: Option<HrvNight>,
}

type SummaryRow = (
    i64,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<i64>,
);

async fn hrv(
    user: DataUser,
    State(state): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Res<Json<HrvRange>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let (from, to) = ctx.range(&p)?;
    let (lo, hi) = (ctx.day_bounds(from).0, ctx.day_bounds(to).1);

    let rows: Vec<SummaryRow> = sqlx::query_as(
        "SELECT ts, weekly_avg, last_night_avg, last_night_5min_high, baseline_balanced_lower, baseline_balanced_upper, status
         FROM hrv_summary_current WHERE user_id = ? AND ts >= ? AND ts < ? ORDER BY ts",
    )
    .bind(ctx.uid)
    .bind(lo)
    .bind(hi)
    .fetch_all(&ctx.pool)
    .await?;
    // The latest summary of a local day is the day's.
    let mut by_day: BTreeMap<NaiveDate, HrvDay> = BTreeMap::new();
    for (ts, weekly, last, high, low_b, high_b, status) in rows {
        let d = ctx.date(ts);
        let mut day = HrvDay::empty(d);
        day.status_code = status.and_then(|s| i32::try_from(s).ok());
        day.status = status.and_then(status_kind);
        day.status_label = status.and_then(hrv_status_label).map(capitalised);
        day.weekly_avg_ms = weekly;
        day.last_night_ms = last;
        day.last_night_5min_high_ms = high;
        day.baseline_low_ms = low_b;
        day.baseline_high_ms = high_b;
        by_day.insert(d, day);
    }

    // Readings: a day's overnight average when the summary has none, and the night itself.
    let readings: Vec<(i64, f64)> = sqlx::query_as(
        "SELECT ts, rmssd_ms FROM hrv_values_current WHERE user_id = ? AND ts >= ? AND ts < ? ORDER BY ts",
    )
    .bind(ctx.uid)
    .bind(lo - NIGHT_SHIFT_S)
    .bind(hi - NIGHT_SHIFT_S)
    .fetch_all(&ctx.pool)
    .await?;
    let mut sums: BTreeMap<NaiveDate, (f64, u32)> = BTreeMap::new();
    for (ts, v) in &readings {
        let e = sums.entry(ctx.date(ts + NIGHT_SHIFT_S)).or_default();
        e.0 += v;
        e.1 += 1;
    }
    for (d, (sum, n)) in sums {
        let day = by_day.entry(d).or_insert_with(|| HrvDay::empty(d));
        if day.last_night_ms.is_none() {
            day.last_night_ms = Some(round1(sum / f64::from(n)));
        }
    }

    let days: Vec<HrvDay> = dates(from, to)
        .map(|d| by_day.remove(&d).unwrap_or_else(|| HrvDay::empty(d)))
        .collect();
    let overnight: Vec<f64> = days.iter().filter_map(|d| d.last_night_ms).collect();
    let baseline = days
        .iter()
        .rev()
        .find_map(|d| d.baseline_low_ms.zip(d.baseline_high_ms));
    let night = (from == to && !readings.is_empty()).then(|| {
        let vals = || readings.iter().map(|r| r.1);
        HrvNight {
            readings: readings
                .iter()
                .map(|(t, v)| TimedValue {
                    time: ctx.ts(*t),
                    value: Some(*v),
                })
                .collect(),
            avg_ms: mean(vals()),
            low_ms: vals().reduce(f64::min),
            high_ms: vals().reduce(f64::max),
        }
    });
    Ok(Json(HrvRange {
        sample: ctx.sample(),
        from: date_string(from),
        to: date_string(to),
        today: date_string(ctx.today()),
        nights: u32::try_from(overnight.len()).unwrap_or(u32::MAX),
        overnight_avg_ms: mean(overnight.iter().copied()),
        baseline_low_ms: baseline.map(|b| b.0),
        baseline_high_ms: baseline.map(|b| b.1),
        days,
        night,
    }))
}
