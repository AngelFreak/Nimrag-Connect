//! `GET /api/trends?range=7d|4w|3m|1y`: HRV against the personal range, resting
//! heart rate, breathing, blood oxygen, sleep score and duration, one point per
//! local day. Days without a reading have no point.

use super::daily::{hrv_by_day, night_sessions, nights as night_summaries, resting_by_day};
use super::metrics::breathing_by_day;
use super::{mean, round1, Ctx, DateValue};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::settings::{date_string, parse_date};
use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/trends", get(trends))
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct TrendSeries {
    pub average: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub points: Vec<DateValue>,
}

fn trend(map: BTreeMap<NaiveDate, f64>) -> TrendSeries {
    let vals = || map.values().copied();
    TrendSeries {
        average: mean(vals()),
        min: vals().reduce(f64::min),
        max: vals().reduce(f64::max),
        points: map
            .iter()
            .map(|(d, v)| DateValue {
                date: date_string(*d),
                value: Some(*v),
            })
            .collect(),
    }
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct HrvTrend {
    pub series: TrendSeries,
    /// Balanced range from the latest watch baseline in or before the range.
    pub baseline_low_ms: Option<f64>,
    pub baseline_high_ms: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct BreathingTrend {
    /// Mean breathing rate during each night's sleep.
    pub asleep: TrendSeries,
    /// Mean over the readings outside sleep, for the whole range (when the range has days the
    /// watch has no reading for, Garmin Connect's awake averages of those days count too, and
    /// the figure is the mean of the days' awake means).
    pub awake_avg_brpm: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct Spo2Trend {
    /// Mean blood oxygen during each night's sleep.
    pub overnight: TrendSeries,
    /// Lowest reading during sleep in the range.
    pub low_percent: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TrendsResponse {
    pub sample: bool,
    pub range: String,
    pub from: String,
    pub to: String,
    pub hrv: HrvTrend,
    pub resting_hr: TrendSeries,
    pub breathing: BreathingTrend,
    pub spo2: Spo2Trend,
    pub sleep_score: TrendSeries,
    pub sleep_duration_s: TrendSeries,
}

#[derive(Debug, Deserialize)]
struct Params {
    range: Option<String>,
}

async fn trends(
    user: DataUser,
    State(state): State<AppState>,
    Query(p): Query<Params>,
) -> Res<Json<TrendsResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let range = p.range.unwrap_or_else(|| "4w".into());
    let days: i64 = match range.as_str() {
        "7d" => 7,
        "4w" => 28,
        "3m" => 90,
        "1y" => 365,
        _ => return Err(ApiError::bad_request("range must be 7d, 4w, 3m or 1y")),
    };
    let to = ctx.today();
    let from = to - Duration::days(days - 1);
    let (lo, hi) = (ctx.day_bounds(from).0, ctx.day_bounds(to).1);

    let (hrv_days, hrv_rows) = hrv_by_day(&ctx, from, to).await?;
    let baseline = hrv_rows.iter().rev().find_map(|r| r.3.zip(r.4));
    let resting = resting_by_day(&ctx, from, to).await?;

    let nights = night_sessions(&ctx, from, to).await?;
    let mut score = BTreeMap::new();
    let mut duration = BTreeMap::new();
    // Time asleep (stages), the same figure the night views show.
    for n in night_summaries(&ctx, from, to).await? {
        if let Some(d) = parse_date(&n.date) {
            duration.insert(d, n.duration_s);
        }
    }
    let mut breathing = BTreeMap::new();
    let mut spo2 = BTreeMap::new();
    let (mut asleep_sum, mut asleep_n, mut spo2_low) = (0.0, 0.0, None::<f64>);
    for (d, s) in &nights {
        if let Some(v) = s.overall_score {
            score.insert(*d, v);
        }
        let w = (s.start_ts, s.end_ts);
        let (sum, n, avg): (Option<f64>, i64, Option<f64>) = sqlx::query_as(
            "SELECT SUM(brpm), COUNT(brpm), AVG(brpm) FROM respiration WHERE user_id = ? AND ts >= ? AND ts <= ?",
        )
        .bind(ctx.uid)
        .bind(w.0)
        .bind(w.1)
        .fetch_one(&ctx.pool)
        .await?;
        if let Some(a) = avg {
            breathing.insert(*d, round1(a));
            asleep_sum += sum.unwrap_or(0.0);
            asleep_n += n as f64;
        }
        let (avg, low): (Option<f64>, Option<f64>) = sqlx::query_as(
            "SELECT AVG(percent), MIN(percent) FROM spo2 WHERE user_id = ? AND ts >= ? AND ts <= ?",
        )
        .bind(ctx.uid)
        .bind(w.0)
        .bind(w.1)
        .fetch_one(&ctx.pool)
        .await?;
        if let Some(a) = avg {
            spo2.insert(*d, round1(a));
        }
        spo2_low = match (spo2_low, low) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    let (all_sum, all_n): (Option<f64>, i64) = sqlx::query_as(
        "SELECT SUM(brpm), COUNT(brpm) FROM respiration WHERE user_id = ? AND ts >= ? AND ts < ?",
    )
    .bind(ctx.uid)
    .bind(lo)
    .bind(hi)
    .fetch_one(&ctx.pool)
    .await?;
    let awake_n = all_n as f64 - asleep_n;
    let mut awake_avg =
        (awake_n > 0.0).then(|| round1((all_sum.unwrap_or(0.0) - asleep_sum) / awake_n));
    // With days the watch has no breathing reading for, Connect's daily averages count: the
    // figure is then the mean of the days' awake means, the one the Respiration page shows.
    let connect_resp: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM connect_daily WHERE user_id = ? AND ts >= ? AND ts < ? AND resp_avg IS NOT NULL",
    )
    .bind(ctx.uid)
    .bind(lo)
    .bind(hi)
    .fetch_one(&ctx.pool)
    .await?;
    if connect_resp > 0 {
        let days = breathing_by_day(&ctx, from, to).await?;
        if !days.from_connect.is_empty() {
            awake_avg = mean(days.awake.values().copied());
        }
    }

    Ok(Json(TrendsResponse {
        sample: ctx.sample(),
        range,
        from: date_string(from),
        to: date_string(to),
        hrv: HrvTrend {
            series: trend(hrv_days),
            baseline_low_ms: baseline.map(|b| b.0),
            baseline_high_ms: baseline.map(|b| b.1),
        },
        resting_hr: trend(resting),
        breathing: BreathingTrend {
            asleep: trend(breathing),
            awake_avg_brpm: awake_avg,
        },
        spo2: Spo2Trend {
            overnight: trend(spo2),
            low_percent: spo2_low,
        },
        sleep_score: trend(score),
        sleep_duration_s: trend(duration),
    }))
}
