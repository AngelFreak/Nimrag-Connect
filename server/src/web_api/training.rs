//! `GET /api/training`: recovery time, 7-day load against its target range, load
//! by intensity, training status and VO2 max. The cards are shared with Today.

use super::daily::dates;
use super::{round1, Ctx, DateValue};
use crate::api::{ApiError, AppState};
use crate::auth::Owner;
use crate::settings::date_string;
use axum::{extract::State, routing::get, Json, Router};
use chrono::Duration;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/training", get(training))
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct TrainingStatusCard {
    /// Raw `training_status` code from the watch; the label mapping is not known yet.
    pub status_code: Option<i32>,
    /// Always `null` for now: the code-to-name table has not been verified.
    pub label: Option<String>,
    /// Local date of the first reading in the current run of the same status.
    pub since: Option<String>,
    /// True when the run reaches back to the oldest reading we hold (the real start may be earlier).
    pub since_is_lower_bound: bool,
    pub fitness_trend_code: Option<i32>,
    pub load_trend_code: Option<i32>,
    pub as_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct TrainingLoadCard {
    /// The watch's weekly (7-day) training load sum.
    pub seven_day_load: Option<f64>,
    pub target_min: Option<f64>,
    pub target_max: Option<f64>,
    /// Where the load sits against the target range: `below`, `within` or `above`.
    pub vs_target: Option<String>,
    pub acute: Option<f64>,
    pub chronic: Option<f64>,
    /// Acute to chronic load ratio.
    pub ratio: Option<f64>,
    /// Daily value of the 7-day load for the last 4 weeks.
    pub series_4w: Vec<DateValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct LoadBalance {
    pub aerobic_low: Option<f64>,
    pub aerobic_high: Option<f64>,
    pub anaerobic: Option<f64>,
    pub aerobic_low_target_min: Option<f64>,
    pub aerobic_low_target_max: Option<f64>,
    pub aerobic_high_target_min: Option<f64>,
    pub aerobic_high_target_max: Option<f64>,
    pub anaerobic_target_min: Option<f64>,
    pub anaerobic_target_max: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RecoveryCard {
    pub started_at: Option<String>,
    pub total_s: Option<f64>,
    pub remaining_s: Option<f64>,
    /// 0 to 100.
    pub percent_complete: Option<f64>,
    pub ready_at: Option<String>,
}

/// VO2 max shown as the watch's maximal MET x 3.5. Derived, not Garmin's published value.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Vo2Card {
    /// ml/kg/min = maximal_met x 3.5; `null` without a reading.
    pub value: Option<f64>,
    pub maximal_met: Option<f64>,
    /// Always true: the number is derived from `maximal_met`.
    pub derived: bool,
    pub measured_at: Option<String>,
    /// Latest minus the first reading of the last 3 months; `null` with fewer than two.
    pub change_3m: Option<f64>,
    pub series_3m: Vec<DateValue>,
}

/// No table holds the watch's readiness score yet.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct TrainingReadinessCard {
    pub score: Option<f64>,
    pub label: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TrainingResponse {
    pub sample: bool,
    pub as_of: String,
    pub recovery: RecoveryCard,
    pub load: TrainingLoadCard,
    pub balance: Option<LoadBalance>,
    pub status: TrainingStatusCard,
    pub vo2max: Vo2Card,
    /// Always `null` for now (no data source).
    pub readiness: Option<TrainingReadinessCard>,
}

type StatusRow = (
    i64,
    Option<i64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<i64>,
    Option<i64>,
);

async fn status_rows(ctx: &Ctx, from: i64) -> Res<Vec<StatusRow>> {
    Ok(sqlx::query_as(
        "SELECT ts, status, weekly_load_sum, load_min, load_max, fitness_trend, load_trend
         FROM training_status WHERE ts >= ? AND ts <= ? ORDER BY ts",
    )
    .bind(from)
    .bind(ctx.now)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn status_and_load(ctx: &Ctx) -> Res<(TrainingStatusCard, TrainingLoadCard)> {
    let today = ctx.today();
    let rows = status_rows(ctx, ctx.day_bounds(today - Duration::days(120)).0).await?;
    let latest = rows.last();
    let (since, lower) = match latest.and_then(|l| l.1) {
        Some(code) => {
            let run: Vec<&StatusRow> = rows
                .iter()
                .rev()
                .take_while(|r| r.1 == Some(code))
                .collect();
            (
                run.last().map(|r| date_string(ctx.date(r.0))),
                run.len() == rows.len(),
            )
        }
        None => (None, false),
    };
    let status = TrainingStatusCard {
        status_code: latest.and_then(|l| l.1).map(|v| v as i32),
        label: None,
        since,
        since_is_lower_bound: lower,
        fitness_trend_code: latest.and_then(|l| l.5).map(|v| v as i32),
        load_trend_code: latest.and_then(|l| l.6).map(|v| v as i32),
        as_of: ctx.opt_ts(latest.map(|l| l.0)),
    };
    let mut daily: BTreeMap<chrono::NaiveDate, f64> = BTreeMap::new();
    for r in &rows {
        if let Some(v) = r.2 {
            daily.insert(ctx.date(r.0), v);
        }
    }
    let from = today - Duration::days(27);
    let (load, tmin, tmax) = latest.map_or((None, None, None), |l| (l.2, l.3, l.4));
    let vs = match (load, tmin, tmax) {
        (Some(v), Some(lo), _) if v < lo => Some("below"),
        (Some(v), _, Some(hi)) if v > hi => Some("above"),
        (Some(_), Some(_), Some(_)) => Some("within"),
        _ => None,
    };
    let acute: Option<(Option<f64>, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT acute, chronic, acwr FROM training_load WHERE ts <= ? ORDER BY ts DESC LIMIT 1",
    )
    .bind(ctx.now)
    .fetch_optional(&ctx.pool)
    .await?;
    let acute = acute.unwrap_or_default();
    let load_card = TrainingLoadCard {
        seven_day_load: load,
        target_min: tmin,
        target_max: tmax,
        vs_target: vs.map(str::to_owned),
        acute: acute.0,
        chronic: acute.1,
        ratio: acute.2,
        series_4w: dates(from, today)
            .filter_map(|d| {
                daily.get(&d).map(|v| DateValue {
                    date: date_string(d),
                    value: Some(*v),
                })
            })
            .collect(),
    };
    Ok((status, load_card))
}

pub async fn recovery(ctx: &Ctx) -> Res<RecoveryCard> {
    let row: Option<(i64, f64)> = sqlx::query_as(
        "SELECT start_ts, minutes FROM recovery_time WHERE start_ts <= ? ORDER BY start_ts DESC LIMIT 1",
    )
    .bind(ctx.now)
    .fetch_optional(&ctx.pool)
    .await?;
    Ok(match row {
        None => RecoveryCard {
            started_at: None,
            total_s: None,
            remaining_s: None,
            percent_complete: None,
            ready_at: None,
        },
        Some((start, minutes)) => {
            let total = minutes * 60.0;
            let elapsed = (ctx.now - start) as f64;
            RecoveryCard {
                started_at: Some(ctx.ts(start)),
                total_s: Some(total),
                remaining_s: Some((total - elapsed).max(0.0)),
                percent_complete: (total > 0.0)
                    .then(|| round1((elapsed / total * 100.0).clamp(0.0, 100.0))),
                ready_at: Some(ctx.ts(start + (minutes * 60.0) as i64)),
            }
        }
    })
}

pub async fn vo2max(ctx: &Ctx) -> Res<Vo2Card> {
    // Watch-level readings plus the maximal MET stored with each activity file.
    let rows: Vec<(i64, f64)> = sqlx::query_as(
        "SELECT ts, maximal_met FROM (
             SELECT ts, maximal_met FROM vo2max
             UNION ALL
             SELECT a.start_ts, m.maximal_met FROM activity_metrics m
               JOIN activities_current a ON a.sha256 = m.sha256
               WHERE m.maximal_met IS NOT NULL AND a.start_ts IS NOT NULL)
         WHERE ts <= ? ORDER BY ts",
    )
    .bind(ctx.now)
    .fetch_all(&ctx.pool)
    .await?;
    let recent: Vec<&(i64, f64)> = rows
        .iter()
        .filter(|r| r.0 >= ctx.now - 90 * 86_400)
        .collect();
    let conv = |met: f64| round1(met * 3.5);
    let last = rows.last();
    Ok(Vo2Card {
        value: last.map(|l| conv(l.1)),
        maximal_met: last.map(|l| l.1),
        derived: true,
        measured_at: ctx.opt_ts(last.map(|l| l.0)),
        change_3m: match (recent.first(), recent.last()) {
            (Some(a), Some(b)) if recent.len() > 1 => Some(round1(conv(b.1) - conv(a.1))),
            _ => None,
        },
        series_3m: recent
            .iter()
            .map(|r| DateValue {
                date: date_string(ctx.date(r.0)),
                value: Some(conv(r.1)),
            })
            .collect(),
    })
}

async fn balance(ctx: &Ctx) -> Res<Option<LoadBalance>> {
    type Row = (
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
    );
    let r: Option<Row> = sqlx::query_as(
        "SELECT aerobic_low, aerobic_high, anaerobic, aerobic_low_target_min, aerobic_low_target_max,
                aerobic_high_target_min, aerobic_high_target_max, anaerobic_target_min, anaerobic_target_max
         FROM training_balance WHERE ts <= ? ORDER BY ts DESC LIMIT 1",
    )
    .bind(ctx.now)
    .fetch_optional(&ctx.pool)
    .await?;
    Ok(r.map(|r| LoadBalance {
        aerobic_low: r.0,
        aerobic_high: r.1,
        anaerobic: r.2,
        aerobic_low_target_min: r.3,
        aerobic_low_target_max: r.4,
        aerobic_high_target_min: r.5,
        aerobic_high_target_max: r.6,
        anaerobic_target_min: r.7,
        anaerobic_target_max: r.8,
    }))
}

async fn training(_: Owner, State(state): State<AppState>) -> Res<Json<TrainingResponse>> {
    let ctx = Ctx::new(&state).await?;
    let (status, load) = status_and_load(&ctx).await?;
    Ok(Json(TrainingResponse {
        sample: ctx.sample(),
        as_of: ctx.ts(ctx.now),
        recovery: recovery(&ctx).await?,
        load,
        balance: balance(&ctx).await?,
        status,
        vo2max: vo2max(&ctx).await?,
        readiness: None,
    }))
}
