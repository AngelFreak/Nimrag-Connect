//! `GET /api/training`: recovery time, 7-day load against its target range, load
//! by intensity, training status and VO2 max. The cards are shared with Today.
//! `GET /api/training/status?from&to`: the training status day by day over a range, as runs.

use super::daily::dates;
use super::{round1, Ctx, DateValue, RangeParams};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::settings::date_string;
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
    Router::new()
        .route("/api/training", get(training))
        .route("/api/training/status", get(status_range))
}

// ---- status names ------------------------------------------------------------------------------

/// How the UI colours a training status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    Detraining,
    Maintaining,
    Recovery,
    Productive,
    Strained,
    /// Code 0, or a row without a status: the watch gave none.
    None,
    /// A code nobody has verified yet; shown as "Status N" in grey.
    Unknown,
}

/// A training status as sent to the UI: the watch's number, its name and how to colour it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct StatusInfo {
    pub code: Option<i32>,
    pub label: String,
    pub kind: StatusKind,
}

/// The ONE place that names training status codes (FIT `training_status.status`, the same numbers
/// as Garmin's API). Only these are verified against the owner's own history: 1 Detraining,
/// 4 Maintaining, 5 Recovery, 7 Productive, 8 Strained; 0 is "No status". The others (2, 3, 6, 9)
/// and anything else are NOT verified: they come out as "Status N" with no name. Do not guess names
/// for them; add a code here only after checking it against a real history.
pub fn status_info(code: i64) -> StatusInfo {
    let (label, kind) = match code {
        0 => ("No status".to_owned(), StatusKind::None),
        1 => ("Detraining".to_owned(), StatusKind::Detraining),
        4 => ("Maintaining".to_owned(), StatusKind::Maintaining),
        5 => ("Recovery".to_owned(), StatusKind::Recovery),
        7 => ("Productive".to_owned(), StatusKind::Productive),
        8 => ("Strained".to_owned(), StatusKind::Strained),
        n => (format!("Status {n}"), StatusKind::Unknown),
    };
    StatusInfo {
        code: i32::try_from(code).ok(),
        label,
        kind,
    }
}

/// Colour legend of the status timeline, in the order the UI shows it.
fn status_legend() -> Vec<StatusInfo> {
    [5, 8, 4, 7, 1, 0].into_iter().map(status_info).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct TrainingStatusCard {
    /// Raw `training_status` code from the watch.
    pub status_code: Option<i32>,
    /// Name of the status (see [`status_info`]; `Status N` for a code that is not verified).
    pub label: Option<String>,
    pub kind: Option<StatusKind>,
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

pub type StatusRow = (
    i64,
    Option<i64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<i64>,
    Option<i64>,
);

/// Training status rows with `from <= ts <= to`, oldest first.
pub async fn status_rows(ctx: &Ctx, from: i64, to: i64) -> Res<Vec<StatusRow>> {
    Ok(sqlx::query_as(
        "SELECT ts, status, weekly_load_sum, load_min, load_max, fitness_trend, load_trend
         FROM training_status WHERE user_id = ? AND ts >= ? AND ts <= ? ORDER BY ts",
    )
    .bind(ctx.uid)
    .bind(from)
    .bind(to)
    .fetch_all(&ctx.pool)
    .await?)
}

/// A training status row older than this is not the current 7-day load.
const LOAD_MAX_AGE_S: i64 = 7 * 86_400;
/// The target range of an acute load, as factors of the chronic load.
const LOAD_RANGE: (f64, f64) = (0.8, 1.5);

pub async fn status_and_load(ctx: &Ctx) -> Res<(TrainingStatusCard, TrainingLoadCard)> {
    let today = ctx.today();
    let rows = status_rows(ctx, ctx.day_bounds(today - Duration::days(120)).0, ctx.now).await?;
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
    let info = latest.and_then(|l| l.1).map(status_info);
    let status = TrainingStatusCard {
        status_code: latest.and_then(|l| l.1).map(|v| v as i32),
        label: info.as_ref().map(|i| i.label.clone()),
        kind: info.as_ref().map(|i| i.kind),
        since,
        since_is_lower_bound: lower,
        fitness_trend_code: latest.and_then(|l| l.5).map(|v| v as i32),
        load_trend_code: latest.and_then(|l| l.6).map(|v| v as i32),
        as_of: ctx.opt_ts(latest.map(|l| l.0)),
    };
    let mut daily: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    for r in &rows {
        if let Some(v) = r.2 {
            daily.insert(ctx.date(r.0), v);
        }
    }
    // The watch's own acute load (its 7-day load), with the chronic load its range comes from.
    let acute_rows: Vec<(i64, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT ts, acute, chronic FROM training_load
         WHERE user_id = ? AND ts >= ? AND ts <= ? AND acute IS NOT NULL ORDER BY ts",
    )
    .bind(ctx.uid)
    .bind(ctx.day_bounds(today - Duration::days(27)).0)
    .bind(ctx.now)
    .fetch_all(&ctx.pool)
    .await?;
    // A day without a status load shows that day's last acute load.
    let mut acute_daily: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    for r in &acute_rows {
        if let Some(v) = r.1 {
            acute_daily.insert(ctx.date(r.0), v);
        }
    }
    for (d, v) in acute_daily {
        daily.entry(d).or_insert(v);
    }
    let from = today - Duration::days(27);
    // The load tile reads the newest row that HAS a load, with that row's target range: the
    // watch writes a row for today with only a status, and Garmin Connect's row of the day
    // before carries the load. The watch's status rows never carry a load, but its
    // `training_load` rows carry the acute load, which is the same figure; their target range
    // is the chronic load times 0.8 to 1.5 (the factors Connect's rows show). The newer of the
    // two wins. Rows older than a week are no longer "the 7-day load".
    let fresh = ctx.now - LOAD_MAX_AGE_S;
    let from_status = rows
        .iter()
        .rev()
        .find(|r| r.2.is_some() && r.0 >= fresh)
        .map(|l| (l.0, (l.2, l.3, l.4)));
    let from_acute = acute_rows.iter().rev().find(|r| r.0 >= fresh).map(|r| {
        let range =
            r.2.filter(|c| *c > 0.0)
                .map(|c| (c * LOAD_RANGE.0, c * LOAD_RANGE.1));
        (
            r.0,
            (r.1, range.map(|x| round1(x.0)), range.map(|x| round1(x.1))),
        )
    });
    let (load, tmin, tmax) = match (from_status, from_acute) {
        (Some(s), Some(a)) if a.0 > s.0 => a.1,
        (Some(s), _) => s.1,
        (None, Some(a)) => a.1,
        (None, None) => (None, None, None),
    };
    let vs = match (load, tmin, tmax) {
        (Some(v), Some(lo), _) if v < lo => Some("below"),
        (Some(v), _, Some(hi)) if v > hi => Some("above"),
        (Some(_), Some(_), Some(_)) => Some("within"),
        _ => None,
    };
    let acute: Option<(Option<f64>, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT acute, chronic, acwr FROM training_load WHERE user_id = ? AND ts <= ? ORDER BY ts DESC LIMIT 1",
    )
    .bind(ctx.uid)
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
        "SELECT start_ts, minutes FROM recovery_time WHERE user_id = ? AND start_ts <= ? ORDER BY start_ts DESC LIMIT 1",
    )
    .bind(ctx.uid)
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

/// ml/kg/min from the watch's maximal MET (derived: MET x 3.5).
pub fn vo2_from_met(met: f64) -> f64 {
    round1(met * 3.5)
}

/// `(ts, maximal_met)` readings with `ts <= to`, oldest first: the watch-level readings plus the
/// maximal MET stored with each activity file. A maximal MET of 0 means "no estimate" (some
/// activity files carry it) and is left out, or it would draw as a VO2 max of 0.
pub async fn vo2_readings(ctx: &Ctx, to: i64) -> Res<Vec<(i64, f64)>> {
    Ok(sqlx::query_as(
        "SELECT ts, maximal_met FROM (
             SELECT ts, maximal_met FROM vo2max WHERE user_id = ?
             UNION ALL
             SELECT a.start_ts, m.maximal_met FROM activity_metrics m
               JOIN activities_current a ON a.sha256 = m.sha256
               WHERE a.user_id = ? AND m.maximal_met IS NOT NULL AND a.start_ts IS NOT NULL)
         WHERE ts <= ? AND maximal_met > 0 ORDER BY ts",
    )
    .bind(ctx.uid)
    .bind(ctx.uid)
    .bind(to)
    .fetch_all(&ctx.pool)
    .await?)
}

pub async fn vo2max(ctx: &Ctx) -> Res<Vo2Card> {
    let rows = vo2_readings(ctx, ctx.now).await?;
    let recent: Vec<&(i64, f64)> = rows
        .iter()
        .filter(|r| r.0 >= ctx.now - 90 * 86_400)
        .collect();
    let conv = vo2_from_met;
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

pub async fn balance(ctx: &Ctx) -> Res<Option<LoadBalance>> {
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
         FROM training_balance WHERE user_id = ? AND ts <= ? ORDER BY ts DESC LIMIT 1",
    )
    .bind(ctx.uid)
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

async fn training(user: DataUser, State(state): State<AppState>) -> Res<Json<TrainingResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
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

// ---- GET /api/training/status ---------------------------------------------------------------

/// One local day of the status timeline; `status` is `null` when no reading exists for the day.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct StatusDay {
    pub date: String,
    pub status: Option<StatusInfo>,
}

/// Consecutive days with the same status code (a day without a reading ends a run).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct StatusRun {
    pub status: StatusInfo,
    /// First and last local date of the run inside the range.
    pub from: String,
    pub to: String,
    pub days: u32,
    /// The run starts on the first day of the range and the day before had the same status, so it
    /// began earlier than the range shows.
    pub starts_before_range: bool,
    /// The run reaches today: the status the watch reports now.
    pub ongoing: bool,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TrainingStatusRange {
    pub sample: bool,
    pub from: String,
    pub to: String,
    /// Today's local date (the range never goes past it).
    pub today: String,
    /// Every day of the range, oldest first; days without a reading have `status: null`.
    pub days: Vec<StatusDay>,
    /// Runs of equal status, newest first.
    pub runs: Vec<StatusRun>,
    /// The colour key of the timeline.
    pub legend: Vec<StatusInfo>,
    /// The latest status the watch reported (not limited to the range) and its load figures.
    pub current: TrainingStatusCard,
    pub load: TrainingLoadCard,
}

async fn status_range(
    user: DataUser,
    State(state): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Res<Json<TrainingStatusRange>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let (from, to) = ctx.range(&p)?;
    let (lo, hi) = (ctx.day_bounds(from).0, ctx.day_bounds(to).1);
    // Rows come from the table whatever their source (watch or an imported history). The last
    // reading of a local day is the day's status.
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT ts, status FROM training_status WHERE user_id = ? AND ts >= ? AND ts < ? AND status IS NOT NULL ORDER BY ts",
    )
    .bind(ctx.uid)
    .bind(lo)
    .bind(hi)
    .fetch_all(&ctx.pool)
    .await?;
    let by_day: BTreeMap<NaiveDate, i64> =
        rows.into_iter().map(|(ts, s)| (ctx.date(ts), s)).collect();
    let today = ctx.today();
    let days: Vec<StatusDay> = dates(from, to)
        .map(|d| StatusDay {
            date: date_string(d),
            status: by_day.get(&d).copied().map(status_info),
        })
        .collect();
    let mut runs: Vec<StatusRun> = Vec::new();
    // The run being built: (code, first day, last day, days).
    let mut open: Option<(i64, NaiveDate, NaiveDate, u32)> = None;
    for d in dates(from, to) {
        let code = by_day.get(&d).copied();
        match (&mut open, code) {
            (Some((c, _, last, n)), Some(now)) if *c == now => {
                *last = d;
                *n += 1;
            }
            (slot, code) => {
                if let Some(o) = slot.take() {
                    runs.push(run_of(o, today));
                }
                open = code.map(|c| (c, d, d, 1));
            }
        }
    }
    if let Some(o) = open.take() {
        runs.push(run_of(o, today));
    }
    // A run that starts on the first day may have begun earlier: it did when the last reading
    // before the range, on the day before it, has the same status.
    if let Some(first) = runs.first_mut().filter(|r| r.from == date_string(from)) {
        let before: Option<(i64, i64)> = sqlx::query_as(
            "SELECT ts, status FROM training_status WHERE user_id = ? AND ts < ? AND status IS NOT NULL ORDER BY ts DESC LIMIT 1",
        )
        .bind(ctx.uid)
        .bind(lo)
        .fetch_optional(&ctx.pool)
        .await?;
        first.starts_before_range = before.is_some_and(|(ts, code)| {
            ctx.date(ts) == from - Duration::days(1)
                && Some(code) == first.status.code.map(i64::from)
        });
    }
    runs.reverse();
    let (current, load) = status_and_load(&ctx).await?;
    Ok(Json(TrainingStatusRange {
        sample: ctx.sample(),
        from: date_string(from),
        to: date_string(to),
        today: date_string(today),
        days,
        runs,
        legend: status_legend(),
        current,
        load,
    }))
}

fn run_of((code, a, b, days): (i64, NaiveDate, NaiveDate, u32), today: NaiveDate) -> StatusRun {
    StatusRun {
        status: status_info(code),
        from: date_string(a),
        to: date_string(b),
        days,
        starts_before_range: false,
        ongoing: b == today,
    }
}
