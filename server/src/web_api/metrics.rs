//! `GET /api/metrics/{metric}?range=7d|4w|1y&end=YYYY-MM-DD`: the data behind a metric detail
//! page (heart rate, stress, energy, steps, intensity minutes, training load, VO2 max,
//! respiration, blood oxygen). One response holds the chart series, the summary figures and the
//! goal or target lines for a period; the 1-day view is `/api/days/{date}`.
//!
//! Every per-day figure comes from the code the Day, Today and Trends endpoints use
//! ([`day_stats`], [`stress_block`], [`intensity_days`], [`night_sessions`], the training status
//! and VO2 helpers), so a day shows the same number everywhere. Days are local days in the
//! account's zone; a night belongs to the day it ends on. A day without data is `null`, never 0.
//!
//! Periods end on `end` (default and maximum: today). `7d` is the 7 days ending on `end` (day
//! buckets), `4w` the 28 days ending on `end` (day buckets; intensity minutes use the 4 Monday
//! to Sunday weeks ending with the week of `end`, week buckets), `1y` the 12 calendar months
//! ending with the month of `end` (month buckets; the last month runs to `end`). A month bucket
//! averages the days that have a value (intensity minutes: the month's total).

use super::daily::{
    dates, day_stats, intensity_days, intensity_total, night_sessions, stress_block, week_start,
    DayStats,
};
use super::training::{status_rows, vo2_from_met, vo2_readings};
use super::{mean, round1, series, Ctx, DateValue};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::settings::{date_string, parse_date};
use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use chrono::{Datelike, Duration, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;
type Daily = BTreeMap<NaiveDate, f64>;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/metrics/{metric}", get(metric_range))
}

// ---- response ---------------------------------------------------------------------------------

/// What one point of a series stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MetricBucket {
    Day,
    Week,
    Month,
}

/// One line or bar series of the chart.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct MetricSeries {
    pub key: String,
    pub label: String,
    pub unit: String,
    /// One point per bucket of the period, oldest first, dated by the bucket's first day; a
    /// bucket without data has `value: null`.
    pub points: Vec<DateValue>,
}

/// One summary figure of the period.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct MetricStat {
    pub key: String,
    pub label: String,
    /// `null` when the period has no data for it.
    pub value: Option<f64>,
    pub unit: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct MetricRange {
    pub sample: bool,
    pub metric: String,
    pub range: String,
    /// First and last local day of the period.
    pub from: String,
    pub to: String,
    pub bucket: MetricBucket,
    pub series: Vec<MetricSeries>,
    pub stats: Vec<MetricStat>,
    /// Daily step goal (steps) or weekly intensity-minutes goal (intensity minutes), else `null`.
    pub goal: Option<f64>,
    /// Training load target range (the watch's latest), else `null`.
    pub target_low: Option<f64>,
    pub target_high: Option<f64>,
    /// Whether the metric has any data before `from`.
    pub has_prev: bool,
    /// Whether `to` is before today.
    pub has_next: bool,
}

// ---- request ----------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Metric {
    HeartRate,
    Stress,
    Energy,
    Steps,
    IntensityMinutes,
    TrainingLoad,
    Vo2Max,
    Respiration,
    Spo2,
}

impl Metric {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "heart_rate" => Self::HeartRate,
            "stress" => Self::Stress,
            "energy" => Self::Energy,
            "steps" => Self::Steps,
            "intensity_minutes" => Self::IntensityMinutes,
            "training_load" => Self::TrainingLoad,
            "vo2_max" => Self::Vo2Max,
            "respiration" => Self::Respiration,
            "spo2" => Self::Spo2,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Span {
    Week,
    FourWeeks,
    Year,
}

impl Span {
    fn parse(s: &str) -> Res<Self> {
        match s {
            "7d" => Ok(Self::Week),
            "4w" => Ok(Self::FourWeeks),
            "1y" => Ok(Self::Year),
            _ => Err(ApiError::bad_request("range must be 7d, 4w or 1y")),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Params {
    range: Option<String>,
    end: Option<String>,
}

/// First day and bucket of the period ending on `end`.
fn period(metric: Metric, span: Span, end: NaiveDate) -> (NaiveDate, MetricBucket) {
    match span {
        Span::Week => (end - Duration::days(6), MetricBucket::Day),
        Span::FourWeeks if metric == Metric::IntensityMinutes => {
            (week_start(end) - Duration::days(21), MetricBucket::Week)
        }
        Span::FourWeeks => (end - Duration::days(27), MetricBucket::Day),
        Span::Year => {
            let first = end.with_day(1).unwrap_or(end);
            (
                first.checked_sub_months(Months::new(11)).unwrap_or(first),
                MetricBucket::Month,
            )
        }
    }
}

// ---- bucketing --------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum Agg {
    /// Mean of the days with a value, one decimal.
    Mean,
    /// Mean of the days with a value, whole numbers (steps, seconds, metres).
    MeanWhole,
    /// Total of the days with a value.
    Sum,
}

fn bucket_start(d: NaiveDate, b: MetricBucket) -> NaiveDate {
    match b {
        MetricBucket::Day => d,
        MetricBucket::Week => week_start(d),
        MetricBucket::Month => d.with_day(1).unwrap_or(d),
    }
}

/// One point per bucket of `from..=to`; a bucket with no day value is `null`.
fn points(
    daily: &Daily,
    from: NaiveDate,
    to: NaiveDate,
    b: MetricBucket,
    agg: Agg,
) -> Vec<DateValue> {
    let mut groups: BTreeMap<NaiveDate, Vec<f64>> = BTreeMap::new();
    for d in dates(from, to) {
        let g = groups.entry(bucket_start(d, b)).or_default();
        if let Some(v) = daily.get(&d) {
            g.push(*v);
        }
    }
    groups
        .into_iter()
        .map(|(d, v)| DateValue {
            date: date_string(d),
            value: (!v.is_empty()).then(|| {
                let sum: f64 = v.iter().sum();
                match agg {
                    Agg::Mean => round1(sum / v.len() as f64),
                    Agg::MeanWhole => (sum / v.len() as f64).round(),
                    Agg::Sum => round1(sum),
                }
            }),
        })
        .collect()
}

fn total(v: impl IntoIterator<Item = f64>) -> Option<f64> {
    let v: Vec<f64> = v.into_iter().collect();
    (!v.is_empty()).then(|| round1(v.iter().sum()))
}

fn avg(d: &Daily) -> Option<f64> {
    mean(d.values().copied())
}

fn stat(key: &str, label: &str, unit: &str, value: Option<f64>) -> MetricStat {
    MetricStat {
        key: key.into(),
        label: label.into(),
        value,
        unit: unit.into(),
    }
}

/// What a metric builder returns; the handler adds the envelope.
#[derive(Default)]
struct Built {
    series: Vec<MetricSeries>,
    stats: Vec<MetricStat>,
    goal: Option<f64>,
    target_low: Option<f64>,
    target_high: Option<f64>,
    /// Timestamp of the metric's oldest reading.
    earliest: Option<i64>,
}

/// The period being built, so builders do not repeat their arguments.
struct Period<'a> {
    ctx: &'a Ctx,
    from: NaiveDate,
    to: NaiveDate,
    bucket: MetricBucket,
}

impl Period<'_> {
    fn series(&self, key: &str, label: &str, unit: &str, daily: &Daily, agg: Agg) -> MetricSeries {
        MetricSeries {
            key: key.into(),
            label: label.into(),
            unit: unit.into(),
            points: points(daily, self.from, self.to, self.bucket, agg),
        }
    }
    /// The first and last instant of the period: `[lo, hi)`.
    fn bounds(&self) -> (i64, i64) {
        (
            self.ctx.day_bounds(self.from).0,
            self.ctx.day_bounds(self.to).1,
        )
    }
}

/// Oldest `ts` of a query taking the user id as `?1`.
async fn earliest(ctx: &Ctx, sql: &'static str) -> Res<Option<i64>> {
    Ok(sqlx::query_scalar::<_, Option<i64>>(sql)
        .bind(ctx.uid)
        .fetch_one(&ctx.pool)
        .await?)
}

/// [`day_stats`] for any length of period, in steps small enough to keep each load bounded.
async fn stats_of(p: &Period<'_>) -> Res<BTreeMap<NaiveDate, DayStats>> {
    let mut out = BTreeMap::new();
    let mut a = p.from;
    while a <= p.to {
        let b = (a + Duration::days(34)).min(p.to);
        out.extend(day_stats(p.ctx, a, b).await?);
        a = b + Duration::days(1);
    }
    Ok(out)
}

fn pick(stats: &BTreeMap<NaiveDate, DayStats>, f: impl Fn(&DayStats) -> Option<f64>) -> Daily {
    stats
        .iter()
        .filter_map(|(d, s)| f(s).map(|v| (*d, v)))
        .collect()
}

// ---- the metrics ------------------------------------------------------------------------------

async fn heart_rate(p: &Period<'_>) -> Res<Built> {
    let st = stats_of(p).await?;
    let resting = pick(&st, |s| s.resting_hr);
    let high = pick(&st, |s| s.hr_high);
    Ok(Built {
        series: vec![
            p.series("resting", "Resting", "bpm", &resting, Agg::Mean),
            p.series("high", "High", "bpm", &high, Agg::Mean),
        ],
        stats: vec![
            stat("avg_resting", "Avg resting", "bpm", avg(&resting)),
            stat("avg_high", "Avg high", "bpm", avg(&high)),
        ],
        earliest: earliest(
            p.ctx,
            "SELECT MIN(ts) FROM (
                 SELECT ts FROM mon_hr WHERE user_id = ?1
                 UNION ALL
                 SELECT ts FROM resting_hr WHERE user_id = ?1 AND COALESCE(current_day_rhr, rhr) IS NOT NULL)",
        )
        .await?,
        ..Built::default()
    })
}

async fn stress(p: &Period<'_>) -> Res<Built> {
    let (mut average, mut rest, mut low, mut medium, mut high) =
        <(Daily, Daily, Daily, Daily, Daily)>::default();
    for d in dates(p.from, p.to) {
        let (from, to) = p.ctx.day_bounds(d);
        let b = stress_block(p.ctx, from, to).await?;
        for (map, v) in [
            (&mut average, b.average),
            (&mut rest, b.rest_s),
            (&mut low, b.low_s),
            (&mut medium, b.medium_s),
            (&mut high, b.high_s),
        ] {
            if let Some(v) = v {
                map.insert(d, v);
            }
        }
    }
    Ok(Built {
        series: vec![
            p.series("average", "Average", "", &average, Agg::Mean),
            p.series("rest_s", "Rest", "s", &rest, Agg::MeanWhole),
            p.series("low_s", "Low", "s", &low, Agg::MeanWhole),
            p.series("medium_s", "Medium", "s", &medium, Agg::MeanWhole),
            p.series("high_s", "High", "s", &high, Agg::MeanWhole),
        ],
        stats: vec![stat("avg_stress", "Avg stress", "", avg(&average))],
        earliest: earliest(
            p.ctx,
            "SELECT MIN(ts) FROM stress WHERE user_id = ?1 AND (stress IS NOT NULL OR stress_raw IS NOT NULL)",
        )
        .await?,
        ..Built::default()
    })
}

async fn energy(p: &Period<'_>) -> Res<Built> {
    let st = stats_of(p).await?;
    let high = pick(&st, |s| s.energy.high);
    let low = pick(&st, |s| s.energy.low);
    let charged = pick(&st, |s| s.energy.charged);
    let drained = pick(&st, |s| s.energy.drained);
    Ok(Built {
        series: vec![
            p.series("high", "High", "", &high, Agg::Mean),
            p.series("low", "Low", "", &low, Agg::Mean),
            p.series("charged", "Charged", "", &charged, Agg::Mean),
            p.series("drained", "Drained", "", &drained, Agg::Mean),
        ],
        stats: vec![
            stat("avg_high", "Avg high", "", avg(&high)),
            stat("avg_low", "Avg low", "", avg(&low)),
            stat("avg_charged", "Avg charged", "", avg(&charged)),
            stat("avg_drained", "Avg drained", "", avg(&drained)),
        ],
        earliest: earliest(
            p.ctx,
            "SELECT MIN(ts) FROM stress WHERE user_id = ?1 AND body_battery IS NOT NULL",
        )
        .await?,
        ..Built::default()
    })
}

async fn steps(p: &Period<'_>) -> Res<Built> {
    let st = stats_of(p).await?;
    let steps = pick(&st, |s| s.steps);
    let distance = pick(&st, |s| s.distance_m);
    Ok(Built {
        series: vec![
            p.series("steps", "Steps", "steps", &steps, Agg::MeanWhole),
            p.series("distance_m", "Distance", "m", &distance, Agg::MeanWhole),
        ],
        stats: vec![
            stat(
                "avg_steps",
                "Avg steps per day",
                "steps",
                avg(&steps).map(f64::round),
            ),
            stat(
                "total_steps",
                "Total steps",
                "steps",
                total(steps.values().copied()),
            ),
        ],
        goal: Some(f64::from(p.ctx.prefs.step_goal)),
        earliest: earliest(
            p.ctx,
            "SELECT MIN(ts) FROM (
                 SELECT ts FROM mon_activity WHERE user_id = ?1 AND steps IS NOT NULL
                 UNION ALL
                 SELECT ts FROM daily_totals WHERE user_id = ?1 AND steps IS NOT NULL)",
        )
        .await?,
        ..Built::default()
    })
}

async fn intensity(p: &Period<'_>) -> Res<Built> {
    let days = intensity_days(p.ctx, p.from, p.to).await?;
    let moderate: Daily = days
        .iter()
        .filter_map(|(d, v)| v.0.map(|x| (*d, x)))
        .collect();
    let vigorous: Daily = days
        .iter()
        .filter_map(|(d, v)| v.1.map(|x| (*d, x)))
        .collect();
    let mod_s = p.series("moderate", "Moderate", "min", &moderate, Agg::Sum);
    let vig_s = p.series("vigorous", "Vigorous", "min", &vigorous, Agg::Sum);
    // Moderate + 2 x vigorous per bucket, the figure the goal is measured in.
    let totals: Vec<f64> = mod_s
        .points
        .iter()
        .zip(&vig_s.points)
        .filter_map(|(m, v)| intensity_total(m.value, v.value))
        .collect();
    let per = match p.bucket {
        MetricBucket::Day => "day",
        MetricBucket::Week => "week",
        MetricBucket::Month => "month",
    };
    Ok(Built {
        stats: vec![
            stat("total", "Total", "min", total(totals.iter().copied())),
            stat(
                "avg",
                &format!("Avg per {per}"),
                "min",
                mean(totals.iter().copied()),
            ),
        ],
        series: vec![mod_s, vig_s],
        goal: Some(f64::from(p.ctx.prefs.intensity_goal_min)),
        earliest: earliest(
            p.ctx,
            "SELECT MIN(ts) FROM mon_extra WHERE user_id = ?1
             AND (moderate_total_min IS NOT NULL OR vigorous_total_min IS NOT NULL)",
        )
        .await?,
        ..Built::default()
    })
}

/// The watch's 7-day load (training status, falling back to the load table) and the chronic
/// load, last reading of each day; the target range is the newest the watch gave up to `to`.
async fn training_load(p: &Period<'_>) -> Res<Built> {
    let (lo, hi) = p.bounds();
    let status = status_rows(p.ctx, lo, hi - 1).await?;
    let loads: Vec<(i64, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT ts, acute, chronic FROM training_load WHERE user_id = ? AND ts >= ? AND ts < ? ORDER BY ts",
    )
    .bind(p.ctx.uid)
    .bind(lo)
    .bind(hi)
    .fetch_all(&p.ctx.pool)
    .await?;
    let mut acute = Daily::new();
    let mut chronic = Daily::new();
    for (ts, a, c) in &loads {
        let d = p.ctx.date(*ts);
        if let Some(v) = a {
            acute.insert(d, *v);
        }
        if let Some(v) = c {
            chronic.insert(d, *v);
        }
    }
    // The status table's weekly load is the figure the Today tile shows: it wins for a day
    // that has both.
    for r in &status {
        if let Some(v) = r.2 {
            acute.insert(p.ctx.date(r.0), v);
        }
    }
    let target = sqlx::query_as::<_, (f64, f64)>(
        "SELECT load_min, load_max FROM training_status
         WHERE user_id = ? AND ts < ? AND load_min IS NOT NULL AND load_max IS NOT NULL
         ORDER BY ts DESC LIMIT 1",
    )
    .bind(p.ctx.uid)
    .bind(hi)
    .fetch_optional(&p.ctx.pool)
    .await?;
    Ok(Built {
        series: vec![
            p.series("acute", "Acute load", "", &acute, Agg::Mean),
            p.series("chronic", "Chronic load", "", &chronic, Agg::Mean),
        ],
        stats: vec![
            stat("avg_acute", "Avg acute load", "", avg(&acute)),
            stat("avg_chronic", "Avg chronic load", "", avg(&chronic)),
        ],
        target_low: target.map(|t| t.0),
        target_high: target.map(|t| t.1),
        earliest: earliest(
            p.ctx,
            "SELECT MIN(ts) FROM (
                 SELECT ts FROM training_status WHERE user_id = ?1 AND weekly_load_sum IS NOT NULL
                 UNION ALL
                 SELECT ts FROM training_load WHERE user_id = ?1 AND (acute IS NOT NULL OR chronic IS NOT NULL))",
        )
        .await?,
        ..Built::default()
    })
}

async fn vo2_max(p: &Period<'_>) -> Res<Built> {
    let (lo, hi) = p.bounds();
    let readings = vo2_readings(p.ctx, hi - 1).await?;
    let mut daily = Daily::new();
    // The day's last reading.
    for (ts, met) in readings.iter().filter(|r| r.0 >= lo) {
        daily.insert(p.ctx.date(*ts), vo2_from_met(*met));
    }
    Ok(Built {
        series: vec![p.series("vo2_max", "VO2 max", "ml/kg/min", &daily, Agg::Mean)],
        stats: vec![
            stat(
                "latest",
                "Latest",
                "ml/kg/min",
                daily.values().next_back().copied(),
            ),
            stat("avg", "Avg", "ml/kg/min", avg(&daily)),
        ],
        earliest: readings.first().map(|r| r.0),
        ..Built::default()
    })
}

/// A reading of the period with the day it counts for: a reading taken during a night (its
/// start to its end) belongs to the day the night ends on, any other to its local day. `asleep`
/// says which of the two it was.
struct Reading {
    day: NaiveDate,
    asleep: bool,
    value: f64,
}

/// The readings of a `(user, from, to)` series query over the period, in order.
async fn readings(p: &Period<'_>, sql: &'static str) -> Res<Vec<Reading>> {
    // The night of `to` + 1 starts on the evening of `to`: its readings are not that day's.
    let nights = night_sessions(p.ctx, p.from, p.to + Duration::days(1)).await?;
    let (lo, hi) = p.bounds();
    // A night of the period began up to a day before it.
    let rows = series(p.ctx, sql, lo - 86_400, hi + 86_400).await?;
    let mut out = Vec::new();
    let mut i = 0;
    for (ts, value) in rows {
        while nights.get(i).is_some_and(|(_, s)| s.end_ts < ts) {
            i += 1;
        }
        let (day, asleep) = match nights.get(i) {
            Some((d, s)) if s.start_ts <= ts => (*d, true),
            _ => (p.ctx.date(ts), false),
        };
        if (p.from..=p.to).contains(&day) {
            out.push(Reading { day, asleep, value });
        }
    }
    Ok(out)
}

/// Mean of the readings that pass `keep`, per day.
fn day_means(r: &[Reading], keep: impl Fn(&Reading) -> bool) -> Daily {
    let mut sums: BTreeMap<NaiveDate, (f64, f64)> = BTreeMap::new();
    for x in r.iter().filter(|x| keep(x)) {
        let e = sums.entry(x.day).or_default();
        e.0 += x.value;
        e.1 += 1.0;
    }
    sums.into_iter()
        .map(|(d, (s, n))| (d, round1(s / n)))
        .collect()
}

async fn respiration(p: &Period<'_>) -> Res<Built> {
    let r = readings(
        p,
        "SELECT ts, brpm FROM respiration WHERE user_id = ? AND brpm IS NOT NULL AND ts >= ? AND ts < ? ORDER BY ts",
    )
    .await?;
    let asleep = day_means(&r, |x| x.asleep);
    let awake = day_means(&r, |x| !x.asleep);
    Ok(Built {
        series: vec![
            p.series("asleep", "Asleep", "brpm", &asleep, Agg::Mean),
            p.series("awake", "Awake", "brpm", &awake, Agg::Mean),
        ],
        stats: vec![
            stat("avg_asleep", "Avg asleep", "brpm", avg(&asleep)),
            stat("avg_awake", "Avg awake", "brpm", avg(&awake)),
        ],
        earliest: earliest(
            p.ctx,
            "SELECT MIN(ts) FROM respiration WHERE user_id = ?1 AND brpm IS NOT NULL",
        )
        .await?,
        ..Built::default()
    })
}

async fn spo2(p: &Period<'_>) -> Res<Built> {
    let r = readings(
        p,
        "SELECT ts, percent FROM spo2 WHERE user_id = ? AND percent IS NOT NULL AND ts >= ? AND ts < ? ORDER BY ts",
    )
    .await?;
    let average = day_means(&r, |_| true);
    let mut low = Daily::new();
    for x in &r {
        let e = low.entry(x.day).or_insert(x.value);
        *e = e.min(x.value);
    }
    Ok(Built {
        series: vec![
            p.series("average", "Average", "%", &average, Agg::Mean),
            p.series("low", "Lowest", "%", &low, Agg::Mean),
        ],
        stats: vec![
            stat("avg", "Avg", "%", avg(&average)),
            stat(
                "lowest",
                "Lowest",
                "%",
                low.values().copied().reduce(f64::min),
            ),
        ],
        earliest: earliest(
            p.ctx,
            "SELECT MIN(ts) FROM spo2 WHERE user_id = ?1 AND percent IS NOT NULL",
        )
        .await?,
        ..Built::default()
    })
}

// ---- handler ----------------------------------------------------------------------------------

async fn metric_range(
    user: DataUser,
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(q): Query<Params>,
) -> Res<Json<MetricRange>> {
    let metric = Metric::parse(&name).ok_or_else(|| ApiError::not_found("unknown metric"))?;
    let range = q.range.unwrap_or_else(|| "7d".into());
    let span = Span::parse(&range)?;
    let end = match q.end.as_deref() {
        Some(s) => {
            Some(parse_date(s).ok_or_else(|| ApiError::bad_request("end must be YYYY-MM-DD"))?)
        }
        None => None,
    };
    let ctx = Ctx::new(&state, user.id).await?;
    let today = ctx.today();
    // A period never goes past today.
    let to = end.map_or(today, |e| e.min(today));
    let (from, bucket) = period(metric, span, to);
    let p = Period {
        ctx: &ctx,
        from,
        to,
        bucket,
    };
    let b = match metric {
        Metric::HeartRate => heart_rate(&p).await?,
        Metric::Stress => stress(&p).await?,
        Metric::Energy => energy(&p).await?,
        Metric::Steps => steps(&p).await?,
        Metric::IntensityMinutes => intensity(&p).await?,
        Metric::TrainingLoad => training_load(&p).await?,
        Metric::Vo2Max => vo2_max(&p).await?,
        Metric::Respiration => respiration(&p).await?,
        Metric::Spo2 => spo2(&p).await?,
    };
    Ok(Json(MetricRange {
        sample: ctx.sample(),
        metric: name,
        range,
        from: date_string(from),
        to: date_string(to),
        bucket,
        series: b.series,
        stats: b.stats,
        goal: b.goal,
        target_low: b.target_low,
        target_high: b.target_high,
        has_prev: b.earliest.is_some_and(|t| t < ctx.day_bounds(from).0),
        has_next: to < today,
    }))
}
