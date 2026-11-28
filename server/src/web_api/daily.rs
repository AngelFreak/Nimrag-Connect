//! Shared per-day building blocks: energy, stress, heart rate, sleep nights,
//! HRV, steps and intensity minutes. Used by Today, Day, Movement, Calendar,
//! Sleep and Trends so every screen shows the same number for the same thing.
//!
//! How cumulative watch counters are read (unverified on a full real week, see
//! README): daily steps, distance and active calories are the largest value of
//! the day per activity type, summed over the types; intensity minutes are
//! running totals within the local ISO week (Monday start), the per-day value is
//! the increase over the previous day's last value (a drop counts as a reset).
//! A counter reading is the total at the END of its interval, so it belongs to the day of
//! `ts - 1` ([`counter_date`]): the watch writes the finished day's total stamped exactly 00:00,
//! before it resets (seen 2026-11-21: 4490 at 00:00, then 18 at 01:04).
//!
//! Garmin Connect's daily summaries (`connect_daily`, from `import-connect`) are the fallback
//! for a day, and a metric, that has no sample rows: stress, body battery, breathing (see
//! `metrics.rs`), intensity minutes, the heart rate range, steps, distance and floors. The rule
//! is the same everywhere: if the day has any row of that metric the rows' numbers are used as
//! they are (and a figure they lack stays `null`); only a day without any row of the metric is
//! read from the summary. Whose rows they are does not matter: `stress`, `mon_hr` and
//! `respiration` hold the watch's rows and the rows of Connect's intraday series (a lower rank,
//! so the watch's row wins at an instant both have), and a day can have both (Connect's from
//! before the watch was synced, the watch's after). Every figure is computed from the merged
//! rows, one query per day, never per source. A day that has only the summary has no chart
//! points, no "now" and no stress levels.

use super::{mean, round1, series, Ctx, DateValue, TimedValue};
use crate::api::ApiError;
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

/// One value from a query with at most one row.
pub async fn one_f64(ctx: &Ctx, sql: &'static str, binds: &[i64]) -> Res<Option<f64>> {
    let mut q = sqlx::query_scalar::<_, Option<f64>>(sql).bind(ctx.uid);
    for b in binds {
        q = q.bind(*b);
    }
    Ok(q.fetch_optional(&ctx.pool).await?.flatten())
}

// ---- Garmin Connect daily summaries ----------------------------------------------------

/// One local day of Garmin Connect's summary (`connect_daily`); `None` for a figure Connect gave
/// none of.
#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub struct ConnectDay {
    pub ts: i64,
    pub steps: Option<f64>,
    pub distance_m: Option<f64>,
    pub floors_up: Option<f64>,
    pub moderate_min: Option<f64>,
    pub vigorous_min: Option<f64>,
    pub hr_min: Option<f64>,
    pub hr_max: Option<f64>,
    pub stress_avg: Option<f64>,
    pub bb_charged: Option<f64>,
    pub bb_drained: Option<f64>,
    pub bb_high: Option<f64>,
    pub bb_low: Option<f64>,
    pub resp_avg: Option<f64>,
    pub resp_high: Option<f64>,
    pub resp_low: Option<f64>,
}

const CONNECT_COLS: &str = "ts, steps, distance_m, floors_up, moderate_min, vigorous_min, hr_min,
    hr_max, stress_avg, bb_charged, bb_drained, bb_high, bb_low, resp_avg, resp_high, resp_low";

/// Connect's summaries of the local days `from..=to`, by day.
pub async fn connect_days(
    ctx: &Ctx,
    from: NaiveDate,
    to: NaiveDate,
) -> Res<BTreeMap<NaiveDate, ConnectDay>> {
    let sql = format!(
        "SELECT {CONNECT_COLS} FROM connect_daily WHERE user_id = ? AND ts >= ? AND ts < ? ORDER BY ts"
    );
    let rows: Vec<ConnectDay> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(ctx.uid)
        .bind(ctx.day_bounds(from).0)
        .bind(ctx.day_bounds(to).1)
        .fetch_all(&ctx.pool)
        .await?;
    Ok(rows.into_iter().map(|r| (ctx.date(r.ts), r)).collect())
}

/// Connect's summary of the one local day `d`.
pub async fn connect_day(ctx: &Ctx, d: NaiveDate) -> Res<Option<ConnectDay>> {
    Ok(connect_days(ctx, d, d).await?.remove(&d))
}

/// Connect's summary of the day that starts at `from` (and ends at `to`, exclusive), for the
/// blocks that take a window of timestamps.
async fn connect_in(ctx: &Ctx, from: i64, to: i64) -> Res<Option<ConnectDay>> {
    let sql = format!(
        "SELECT {CONNECT_COLS} FROM connect_daily WHERE user_id = ? AND ts >= ? AND ts < ? ORDER BY ts LIMIT 1"
    );
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(ctx.uid)
        .bind(from)
        .bind(to)
        .fetch_optional(&ctx.pool)
        .await?)
}

impl ConnectDay {
    /// The day's body battery figures; Connect has no "latest" reading.
    pub fn energy(&self) -> EnergyStats {
        EnergyStats {
            last: None,
            charged: self.bb_charged,
            drained: self.bb_drained,
            high: self.bb_high,
            low: self.bb_low,
        }
    }
}

// ---- energy ----------------------------------------------------------------------

/// The watch's body-battery-like value (0 to 100) over a local day.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct EnergyBlock {
    /// Latest sample of the day.
    pub now: Option<f64>,
    pub now_at: Option<String>,
    /// Sum of the rises between consecutive samples.
    pub charged: Option<f64>,
    /// Sum of the falls between consecutive samples (positive number).
    pub drained: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    /// End of the sleep that finished on this day.
    pub wake_time: Option<String>,
    pub series: Vec<TimedValue>,
}

#[derive(Debug, Clone, Default)]
pub struct EnergyStats {
    pub last: Option<(i64, f64)>,
    pub charged: Option<f64>,
    pub drained: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
}

pub fn energy_stats(rows: &[(i64, f64)]) -> EnergyStats {
    if rows.is_empty() {
        return EnergyStats::default();
    }
    let (mut up, mut down) = (0.0, 0.0);
    for w in rows.windows(2) {
        let d = w[1].1 - w[0].1;
        if d > 0.0 {
            up += d;
        } else {
            down -= d;
        }
    }
    let vals = rows.iter().map(|r| r.1);
    EnergyStats {
        last: rows.last().copied(),
        charged: Some(round1(up)),
        drained: Some(round1(down)),
        high: vals.clone().reduce(f64::max),
        low: vals.reduce(f64::min),
    }
}

const ENERGY_SQL: &str = "SELECT ts, body_battery FROM stress
    WHERE user_id = ? AND ts >= ? AND ts < ? AND body_battery IS NOT NULL ORDER BY ts";

pub async fn energy_block(ctx: &Ctx, date: NaiveDate) -> Res<EnergyBlock> {
    let (from, to) = ctx.day_bounds(date);
    let rows = series(ctx, ENERGY_SQL, from, to).await?;
    let s = if rows.is_empty() {
        connect_in(ctx, from, to)
            .await?
            .map(|c| c.energy())
            .unwrap_or_default()
    } else {
        energy_stats(&rows)
    };
    let wake: Option<i64> = sqlx::query_scalar(
        "SELECT end_ts FROM sleep_sessions_current WHERE user_id = ? AND end_ts >= ? AND end_ts < ?
         AND start_ts IS NOT NULL ORDER BY end_ts DESC LIMIT 1",
    )
    .bind(ctx.uid)
    .bind(from)
    .bind(to)
    .fetch_optional(&ctx.pool)
    .await?;
    Ok(EnergyBlock {
        now: s.last.map(|l| l.1),
        now_at: ctx.opt_ts(s.last.map(|l| l.0)),
        charged: s.charged,
        drained: s.drained,
        high: s.high,
        low: s.low,
        wake_time: ctx.opt_ts(wake),
        series: rows
            .iter()
            .map(|(t, v)| TimedValue {
                time: ctx.ts(*t),
                value: Some(*v),
            })
            .collect(),
    })
}

// ---- stress ----------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StressLevel {
    Rest,
    Low,
    Medium,
    High,
    /// The watch reported no stress value because of movement (negative raw value).
    Activity,
}

pub fn stress_level(stress: Option<f64>, raw: Option<f64>) -> Option<StressLevel> {
    match (stress, raw) {
        (Some(v), _) if v <= 25.0 => Some(StressLevel::Rest),
        (Some(v), _) if v <= 50.0 => Some(StressLevel::Low),
        (Some(v), _) if v <= 75.0 => Some(StressLevel::Medium),
        (Some(_), _) => Some(StressLevel::High),
        (None, Some(r)) if r < 0.0 => Some(StressLevel::Activity),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct StressSample {
    pub time: String,
    /// 0 to 100; `null` while the watch reported only a status (movement).
    pub stress: Option<f64>,
    pub level: StressLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct StressBlock {
    /// Mean of the stress values (status-only samples excluded).
    pub average: Option<f64>,
    /// The level (not counting activity) that lasted longest.
    pub main_level: Option<StressLevel>,
    pub rest_s: Option<f64>,
    pub low_s: Option<f64>,
    pub medium_s: Option<f64>,
    pub high_s: Option<f64>,
    pub activity_s: Option<f64>,
    pub series: Vec<StressSample>,
}

/// Longest time one sample is allowed to stand for.
const MAX_SAMPLE_GAP_S: i64 = 600;
const LAST_SAMPLE_S: i64 = 180;

pub async fn stress_block(ctx: &Ctx, from: i64, to: i64) -> Res<StressBlock> {
    let rows: Vec<(i64, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT ts, stress, stress_raw FROM stress
         WHERE user_id = ? AND ts >= ? AND ts < ? AND (stress IS NOT NULL OR stress_raw IS NOT NULL) ORDER BY ts",
    )
    .bind(ctx.uid)
    .bind(from)
    .bind(to)
    .fetch_all(&ctx.pool)
    .await?;
    // A day without a stress reading of any source: Connect's day average (no levels, no
    // samples).
    let connect_avg = if rows.is_empty() {
        connect_in(ctx, from, to).await?.and_then(|c| c.stress_avg)
    } else {
        None
    };
    let mut secs: HashMap<&'static str, i64> = HashMap::new();
    let mut samples = Vec::new();
    for (i, (ts, stress, raw)) in rows.iter().enumerate() {
        let Some(level) = stress_level(*stress, *raw) else {
            continue;
        };
        let span = rows
            .get(i + 1)
            .map_or(LAST_SAMPLE_S, |n| (n.0 - ts).min(MAX_SAMPLE_GAP_S));
        *secs.entry(level_name(level)).or_default() += span;
        samples.push(StressSample {
            time: ctx.ts(*ts),
            stress: *stress,
            level,
        });
    }
    let get = |l: StressLevel| {
        (!samples.is_empty()).then(|| secs.get(level_name(l)).copied().unwrap_or(0) as f64)
    };
    let main_level = [
        StressLevel::Rest,
        StressLevel::Low,
        StressLevel::Medium,
        StressLevel::High,
    ]
    .into_iter()
    .max_by_key(|l| secs.get(level_name(*l)).copied().unwrap_or(0))
    .filter(|l| secs.get(level_name(*l)).copied().unwrap_or(0) > 0);
    Ok(StressBlock {
        average: mean(rows.iter().filter_map(|r| r.1)).or(connect_avg),
        main_level,
        rest_s: get(StressLevel::Rest),
        low_s: get(StressLevel::Low),
        medium_s: get(StressLevel::Medium),
        high_s: get(StressLevel::High),
        activity_s: get(StressLevel::Activity),
        series: samples,
    })
}

fn level_name(l: StressLevel) -> &'static str {
    match l {
        StressLevel::Rest => "rest",
        StressLevel::Low => "low",
        StressLevel::Medium => "medium",
        StressLevel::High => "high",
        StressLevel::Activity => "activity",
    }
}

// ---- heart rate --------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct HrWindow {
    pub low_bpm: Option<f64>,
    pub high_bpm: Option<f64>,
    /// Mean per 5 minutes.
    pub series: Vec<TimedValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct HeartRateBlock {
    pub resting_hr_bpm: Option<f64>,
    /// Mean of the daily resting values of the 7 days up to this day.
    pub avg_7d_resting_hr_bpm: Option<f64>,
    pub day: HrWindow,
}

pub async fn hr_window(ctx: &Ctx, from: i64, to: i64) -> Res<HrWindow> {
    let rows = series(
        ctx,
        "SELECT ts, bpm FROM mon_hr WHERE user_id = ? AND ts >= ? AND ts < ? ORDER BY ts",
        from,
        to,
    )
    .await?;
    let mut buckets: BTreeMap<i64, (f64, u32)> = BTreeMap::new();
    for (ts, bpm) in &rows {
        let b = buckets.entry(ts - ts.rem_euclid(300)).or_default();
        b.0 += bpm;
        b.1 += 1;
    }
    Ok(HrWindow {
        low_bpm: rows.iter().map(|r| r.1).reduce(f64::min),
        high_bpm: rows.iter().map(|r| r.1).reduce(f64::max),
        series: buckets
            .into_iter()
            .map(|(t, (s, n))| TimedValue {
                time: ctx.ts(t),
                value: Some(round1(s / f64::from(n))),
            })
            .collect(),
    })
}

/// Last resting-HR value of each local day (the day's own value when the watch has one).
pub async fn resting_by_day(
    ctx: &Ctx,
    from: NaiveDate,
    to: NaiveDate,
) -> Res<BTreeMap<NaiveDate, f64>> {
    let rows = series(
        ctx,
        "SELECT ts, COALESCE(current_day_rhr, rhr) FROM resting_hr
         WHERE user_id = ? AND ts >= ? AND ts < ? AND COALESCE(current_day_rhr, rhr) IS NOT NULL ORDER BY ts",
        ctx.day_bounds(from).0,
        ctx.day_bounds(to).1,
    )
    .await?;
    Ok(rows.into_iter().map(|(t, v)| (ctx.date(t), v)).collect())
}

pub async fn heart_rate_block(ctx: &Ctx, date: NaiveDate) -> Res<HeartRateBlock> {
    let (from, to) = ctx.day_bounds(date);
    let rest = resting_by_day(ctx, date - Duration::days(6), date).await?;
    let mut day = hr_window(ctx, from, to).await?;
    if day.series.is_empty() {
        // No heart rate reading that day (watch or Connect intraday): the summary's lowest and
        // highest.
        if let Some(c) = connect_day(ctx, date).await? {
            day.low_bpm = c.hr_min;
            day.high_bpm = c.hr_max;
        }
    }
    Ok(HeartRateBlock {
        resting_hr_bpm: rest.get(&date).copied(),
        avg_7d_resting_hr_bpm: mean(rest.values().copied()),
        day,
    })
}

// ---- sleep ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SleepStage {
    Awake,
    Light,
    Deep,
    Rem,
    Unmeasurable,
}

fn stage_of(level: i64) -> Option<SleepStage> {
    Some(match level {
        0 => SleepStage::Unmeasurable,
        1 => SleepStage::Awake,
        2 => SleepStage::Light,
        3 => SleepStage::Deep,
        4 => SleepStage::Rem,
        _ => return None,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct StageSegment {
    pub start: String,
    pub end: String,
    pub stage: SleepStage,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SleepScores {
    pub overall: Option<f64>,
    pub quality: Option<f64>,
    pub duration: Option<f64>,
    pub recovery: Option<f64>,
    pub deep: Option<f64>,
    pub light: Option<f64>,
    pub rem: Option<f64>,
}

/// One night, in full.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SleepNight {
    /// Local date the night ends on, the morning of waking up (Garmin's convention): the night
    /// from Thursday evening to Friday morning is dated Friday.
    pub date: String,
    pub start: String,
    pub end: String,
    /// Time asleep (deep + light + REM) when stages exist, else the whole span.
    pub duration_s: f64,
    pub time_in_bed_s: f64,
    pub score: Option<f64>,
    /// excellent (90+), good (80+), fair (60+), poor: derived from the score.
    pub score_label: Option<String>,
    pub scores: SleepScores,
    pub awakenings: Option<f64>,
    pub avg_stress: Option<f64>,
    pub stages_available: bool,
    pub deep_s: Option<f64>,
    pub light_s: Option<f64>,
    pub rem_s: Option<f64>,
    pub awake_s: Option<f64>,
    pub stages: Vec<StageSegment>,
    pub hrv_ms: Option<f64>,
    pub lowest_hr_bpm: Option<f64>,
    pub breathing_brpm: Option<f64>,
    pub spo2_avg_percent: Option<f64>,
    pub spo2_low_percent: Option<f64>,
    /// Sleep need in minutes a night (the owner's setting; 480 by default).
    pub sleep_need_min: u32,
    /// Restless moments, from a night imported from Garmin Connect (a watch night has none).
    pub restless_moments: Option<f64>,
    /// Mean heart rate during the night.
    pub avg_hr_bpm: Option<f64>,
    /// Energy (Body Battery) at the end of the night minus at the start; positive when it charged.
    pub energy_change: Option<f64>,
    /// Slowest breathing during the night.
    pub breathing_low_brpm: Option<f64>,
    /// poor, low, unbalanced or balanced: the watch's 7-day HRV status for this night.
    pub hrv_status: Option<String>,
}

/// A night as one bar of the "last nights" chart.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct NightSummary {
    pub date: String,
    pub start: String,
    pub end: String,
    pub duration_s: f64,
    pub score: Option<f64>,
    pub deep_s: Option<f64>,
    pub light_s: Option<f64>,
    pub rem_s: Option<f64>,
    pub awake_s: Option<f64>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionRow {
    pub start_ts: i64,
    pub end_ts: i64,
    pub overall_score: Option<f64>,
    pub quality_score: Option<f64>,
    pub duration_score: Option<f64>,
    pub recovery_score: Option<f64>,
    pub deep_score: Option<f64>,
    pub light_score: Option<f64>,
    pub rem_score: Option<f64>,
    pub awakenings_count: Option<f64>,
    pub avg_stress: Option<f64>,
    /// Restless moments (imported nights only).
    pub restless_moments: Option<f64>,
    /// Stage totals a night imported from Garmin Connect carries (a watch night has stage rows
    /// in `sleep_levels` instead).
    pub asleep_s: Option<f64>,
    pub deep_s: Option<f64>,
    pub light_s: Option<f64>,
    pub rem_s: Option<f64>,
    pub awake_s: Option<f64>,
}

const SESSION_COLS: &str = "start_ts, end_ts, overall_score, quality_score, duration_score,
    recovery_score, deep_score, light_score, rem_score, awakenings_count, avg_stress,
    restless_moments, asleep_s, deep_s, light_s, rem_s, awake_s";

/// The local date a night belongs to: the day it ENDS on (Garmin's convention), so the night
/// from Thursday evening to Friday morning is Friday's sleep, in the account's time zone.
pub fn night_date(ctx: &Ctx, end_ts: i64) -> NaiveDate {
    ctx.date(end_ts)
}

/// Sessions of the nights whose date is in `from..=to`, i.e. that ended on those local days.
async fn sessions_of_dates(ctx: &Ctx, from: NaiveDate, to: NaiveDate) -> Res<Vec<SessionRow>> {
    let sql = format!(
        "SELECT {SESSION_COLS} FROM sleep_sessions_current
         WHERE user_id = ? AND end_ts >= ? AND end_ts < ? AND end_ts > start_ts ORDER BY end_ts"
    );
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(ctx.uid)
        .bind(ctx.day_bounds(from).0)
        .bind(ctx.day_bounds(to + Duration::days(1)).0)
        .fetch_all(&ctx.pool)
        .await?)
}

/// One session per night date in `from..=to` (the longest), oldest first.
pub async fn night_sessions(
    ctx: &Ctx,
    from: NaiveDate,
    to: NaiveDate,
) -> Res<Vec<(NaiveDate, SessionRow)>> {
    let mut by: BTreeMap<NaiveDate, SessionRow> = BTreeMap::new();
    for s in sessions_of_dates(ctx, from, to).await? {
        let d = night_date(ctx, s.end_ts);
        if by
            .get(&d)
            .is_none_or(|o| s.end_ts - s.start_ts > o.end_ts - o.start_ts)
        {
            by.insert(d, s);
        }
    }
    Ok(by.into_iter().collect())
}

/// The night for a local date (the longest session ending on it).
pub async fn session_for_night(ctx: &Ctx, date: NaiveDate) -> Res<Option<SessionRow>> {
    Ok(sessions_of_dates(ctx, date, date)
        .await?
        .into_iter()
        .max_by_key(|s| s.end_ts - s.start_ts))
}

/// The latest night that ended within the last 30 hours.
pub async fn session_last_night(ctx: &Ctx) -> Res<Option<SessionRow>> {
    let sql = format!(
        "SELECT {SESSION_COLS} FROM sleep_sessions_current
         WHERE user_id = ? AND start_ts IS NOT NULL AND end_ts > start_ts AND end_ts <= ? AND end_ts >= ?
         ORDER BY end_ts DESC LIMIT 1"
    );
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(ctx.uid)
        .bind(ctx.now)
        .bind(ctx.now - 30 * 3600)
        .fetch_optional(&ctx.pool)
        .await?)
}

struct Stages {
    segments: Vec<(i64, i64, SleepStage)>,
    deep: f64,
    light: f64,
    rem: f64,
    awake: f64,
}

/// Segments from sleep-level rows: each row lasts until the next one (or the end).
/// Stage segments of a night. Each `sleep_levels` row is the stage of the period that ENDS
/// at its time, from the previous row (or the session start); checked against an Instinct 2's
/// own sleep summary (deep/light/REM/awake all matched to the minute, 2026-11-20).
fn stages_of(levels: &[(i64, i64)], start: i64, end: i64) -> Option<Stages> {
    let inside: Vec<&(i64, i64)> = levels
        .iter()
        .filter(|l| l.0 > start && l.0 <= end)
        .collect();
    if inside.is_empty() {
        return None;
    }
    let mut st = Stages {
        segments: vec![],
        deep: 0.0,
        light: 0.0,
        rem: 0.0,
        awake: 0.0,
    };
    let mut from = start;
    for (to, level) in inside {
        let (to, from_here) = (*to, from);
        from = to;
        let Some(stage) = stage_of(*level) else {
            continue;
        };
        if to <= from_here {
            continue;
        }
        let d = (to - from_here) as f64;
        match stage {
            SleepStage::Deep => st.deep += d,
            SleepStage::Light => st.light += d,
            SleepStage::Rem => st.rem += d,
            SleepStage::Awake => st.awake += d,
            SleepStage::Unmeasurable => {}
        }
        st.segments.push((from_here, to, stage));
    }
    Some(st)
}

async fn levels(ctx: &Ctx, from: i64, to: i64) -> Res<Vec<(i64, i64)>> {
    Ok(sqlx::query_as(
        // Rows end their period, so a night (from, to] holds the rows after `from` up to `to`.
        "SELECT ts, level FROM sleep_levels WHERE user_id = ? AND ts > ? AND ts <= ? ORDER BY ts",
    )
    .bind(ctx.uid)
    .bind(from)
    .bind(to)
    .fetch_all(&ctx.pool)
    .await?)
}

fn summary(ctx: &Ctx, s: &SessionRow, lv: &[(i64, i64)]) -> NightSummary {
    let st = stages_of(lv, s.start_ts, s.end_ts);
    NightSummary {
        date: crate::settings::date_string(night_date(ctx, s.end_ts)),
        start: ctx.ts(s.start_ts),
        end: ctx.ts(s.end_ts),
        // Time asleep: from the stage rows of a watch night, else the totals an imported night
        // carries, else the whole span.
        duration_s: st
            .as_ref()
            .map_or(s.asleep_s.unwrap_or((s.end_ts - s.start_ts) as f64), |t| {
                t.deep + t.light + t.rem
            }),
        score: s.overall_score,
        deep_s: st.as_ref().map(|t| t.deep).or(s.deep_s),
        light_s: st.as_ref().map(|t| t.light).or(s.light_s),
        rem_s: st.as_ref().map(|t| t.rem).or(s.rem_s),
        awake_s: st.as_ref().map(|t| t.awake).or(s.awake_s),
    }
}

/// Nights whose local date (the day they end on) is in `from..=to`, one per date (the longest),
/// oldest first.
pub async fn nights(ctx: &Ctx, from: NaiveDate, to: NaiveDate) -> Res<Vec<NightSummary>> {
    let sessions = sessions_of_dates(ctx, from, to).await?;
    let (Some(min_start), Some(max_end)) = (
        sessions.iter().map(|s| s.start_ts).min(),
        sessions.iter().map(|s| s.end_ts).max(),
    ) else {
        return Ok(vec![]);
    };
    let lv = levels(ctx, min_start, max_end).await?;
    let mut by_date: BTreeMap<NaiveDate, (i64, NightSummary)> = BTreeMap::new();
    for s in &sessions {
        let d = night_date(ctx, s.end_ts);
        let len = s.end_ts - s.start_ts;
        if by_date.get(&d).is_none_or(|(l, _)| len > *l) {
            by_date.insert(d, (len, summary(ctx, s, &lv)));
        }
    }
    Ok(by_date.into_values().map(|v| v.1).collect())
}

pub fn score_label(score: f64) -> &'static str {
    match score {
        s if s >= 90.0 => "excellent",
        s if s >= 80.0 => "good",
        s if s >= 60.0 => "fair",
        _ => "poor",
    }
}

pub async fn night_detail(ctx: &Ctx, s: &SessionRow) -> Res<SleepNight> {
    let lv = levels(ctx, s.start_ts, s.end_ts).await?;
    let st = stages_of(&lv, s.start_ts, s.end_ts);
    let sum = summary(ctx, s, &lv);
    let hrv = match one_f64(
        ctx,
        "SELECT last_night_avg FROM hrv_summary WHERE user_id = ? AND ts >= ? AND ts <= ? AND last_night_avg IS NOT NULL
         ORDER BY ts DESC LIMIT 1",
        // From the start of the night, or of the day it ends on: a summary imported from Garmin
        // Connect sits at the start of its day.
        &[
            s.start_ts.min(ctx.day_bounds(ctx.date(s.end_ts)).0),
            s.end_ts + 6 * 3600,
        ],
    )
    .await?
    {
        Some(v) => Some(v),
        None => one_f64(ctx, "SELECT AVG(rmssd_ms) FROM hrv_values WHERE user_id = ? AND ts >= ? AND ts <= ?", &[s.start_ts, s.end_ts])
            .await?
            .map(round1),
    };
    let w = [s.start_ts, s.end_ts];
    let resp = one_f64(
        ctx,
        "SELECT AVG(brpm) FROM respiration WHERE user_id = ? AND ts >= ? AND ts <= ?",
        &w,
    )
    .await?;
    let spo2_avg = one_f64(
        ctx,
        "SELECT AVG(percent) FROM spo2 WHERE user_id = ? AND ts >= ? AND ts <= ?",
        &w,
    )
    .await?;
    let hrv_status = one_f64(
        ctx,
        "SELECT CAST(status AS REAL) FROM hrv_summary WHERE user_id = ? AND ts >= ? AND ts <= ? AND status IS NOT NULL
         ORDER BY ts DESC LIMIT 1",
        &[
            s.start_ts.min(ctx.day_bounds(ctx.date(s.end_ts)).0),
            s.end_ts + 6 * 3600,
        ],
    )
    .await?
    .and_then(|c| hrv_status_label(c as i64))
    .map(str::to_owned);
    // Energy (Body Battery) at the first and the last reading of the night.
    let battery_first = one_f64(
        ctx,
        "SELECT body_battery FROM stress WHERE user_id = ? AND ts >= ? AND ts <= ? AND body_battery IS NOT NULL
         ORDER BY ts ASC LIMIT 1",
        &w,
    )
    .await?;
    let battery_last = one_f64(
        ctx,
        "SELECT body_battery FROM stress WHERE user_id = ? AND ts >= ? AND ts <= ? AND body_battery IS NOT NULL
         ORDER BY ts DESC LIMIT 1",
        &w,
    )
    .await?;
    let energy_change = match (battery_first, battery_last) {
        (Some(a), Some(b)) => Some(b - a),
        _ => None,
    };
    Ok(SleepNight {
        sleep_need_min: ctx.prefs.sleep_need_min,
        restless_moments: s.restless_moments,
        avg_hr_bpm: one_f64(
            ctx,
            "SELECT AVG(bpm) FROM mon_hr WHERE user_id = ? AND ts >= ? AND ts <= ?",
            &w,
        )
        .await?
        .map(round1),
        energy_change,
        breathing_low_brpm: one_f64(
            ctx,
            "SELECT MIN(brpm) FROM respiration WHERE user_id = ? AND ts >= ? AND ts <= ? AND brpm > 0",
            &w,
        )
        .await?
        .map(round1),
        hrv_status,
        date: sum.date,
        start: sum.start,
        end: sum.end,
        duration_s: sum.duration_s,
        time_in_bed_s: (s.end_ts - s.start_ts) as f64,
        score: s.overall_score,
        score_label: s.overall_score.map(|v| score_label(v).to_owned()),
        scores: SleepScores {
            overall: s.overall_score,
            quality: s.quality_score,
            duration: s.duration_score,
            recovery: s.recovery_score,
            deep: s.deep_score,
            light: s.light_score,
            rem: s.rem_score,
        },
        awakenings: s.awakenings_count,
        avg_stress: s.avg_stress,
        stages_available: st.is_some(),
        deep_s: sum.deep_s,
        light_s: sum.light_s,
        rem_s: sum.rem_s,
        awake_s: sum.awake_s,
        stages: st
            .map(|t| {
                t.segments
                    .iter()
                    .map(|(a, b, stage)| StageSegment {
                        start: ctx.ts(*a),
                        end: ctx.ts(*b),
                        stage: *stage,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        hrv_ms: hrv,
        lowest_hr_bpm: one_f64(
            ctx,
            "SELECT MIN(bpm) FROM mon_hr WHERE user_id = ? AND ts >= ? AND ts <= ?",
            &w,
        )
        .await?,
        breathing_brpm: resp.map(round1),
        spo2_avg_percent: spo2_avg.map(round1),
        spo2_low_percent: one_f64(
            ctx,
            "SELECT MIN(percent) FROM spo2 WHERE user_id = ? AND ts >= ? AND ts <= ?",
            &w,
        )
        .await?,
    })
}

/// One reading of an overnight series, as minutes from the start of the night.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct NightPoint {
    pub minute: f64,
    pub value: f64,
}

/// One measurement over the night (5 minute means), for the line drawn over the stage chart.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct NightSeries {
    /// `heart_rate`, `energy`, `stress`, `breathing`, `spo2` or `hrv`.
    pub key: String,
    pub points: Vec<NightPoint>,
}

/// Means per 5 minutes since `start`; empty when there are fewer than two readings.
fn bucketed(rows: &[(i64, f64)], start: i64, decimals: bool) -> Vec<NightPoint> {
    let mut sums: BTreeMap<i64, (f64, u32)> = BTreeMap::new();
    for (t, v) in rows {
        let e = sums.entry((t - start).div_euclid(300)).or_default();
        e.0 += v;
        e.1 += 1;
    }
    let pts: Vec<NightPoint> = sums
        .into_iter()
        .map(|(b, (sum, n))| {
            let mean = sum / f64::from(n);
            NightPoint {
                minute: (b * 5) as f64,
                value: if decimals { round1(mean) } else { mean.round() },
            }
        })
        .collect();
    if pts.len() < 2 {
        Vec::new()
    } else {
        pts
    }
}

/// The overnight series of a night, only those with data.
pub async fn night_series(ctx: &Ctx, s: &SessionRow) -> Res<Vec<NightSeries>> {
    let (a, b) = (s.start_ts, s.end_ts);
    let mut out = Vec::new();
    let mut add = |key: &str, rows: Vec<(i64, f64)>, decimals: bool| {
        let points = bucketed(&rows, a, decimals);
        if !points.is_empty() {
            out.push(NightSeries {
                key: key.to_owned(),
                points,
            });
        }
    };
    add(
        "heart_rate",
        series(
            ctx,
            "SELECT ts, bpm FROM mon_hr WHERE user_id = ? AND ts >= ? AND ts <= ? ORDER BY ts",
            a,
            b,
        )
        .await?,
        false,
    );
    add(
        "energy",
        series(ctx, "SELECT ts, body_battery FROM stress WHERE user_id = ? AND ts >= ? AND ts <= ? AND body_battery IS NOT NULL ORDER BY ts", a, b).await?,
        false,
    );
    add(
        "stress",
        series(ctx, "SELECT ts, stress FROM stress WHERE user_id = ? AND ts >= ? AND ts <= ? AND stress >= 0 ORDER BY ts", a, b).await?,
        false,
    );
    add(
        "breathing",
        series(ctx, "SELECT ts, brpm FROM respiration WHERE user_id = ? AND ts >= ? AND ts <= ? AND brpm > 0 ORDER BY ts", a, b).await?,
        true,
    );
    add(
        "spo2",
        series(ctx, "SELECT ts, percent FROM spo2 WHERE user_id = ? AND ts >= ? AND ts <= ? AND percent > 0 ORDER BY ts", a, b).await?,
        false,
    );
    add(
        "hrv",
        series(ctx, "SELECT ts, rmssd_ms FROM hrv_values WHERE user_id = ? AND ts >= ? AND ts <= ? ORDER BY ts", a, b).await?,
        false,
    );
    Ok(out)
}

// ---- HRV -------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum BaselinePosition {
    Below,
    Within,
    Above,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct HrvBlock {
    pub last_night_ms: Option<f64>,
    pub weekly_avg_ms: Option<f64>,
    /// Lower and upper edge of the personal "balanced" range (from the watch's baseline).
    pub baseline_low_ms: Option<f64>,
    pub baseline_high_ms: Option<f64>,
    /// Raw status code from the watch's HRV summary.
    pub status_code: Option<i32>,
    /// poor, low, unbalanced or balanced (FIT `hrv_status`); `null` for unknown codes.
    pub status: Option<String>,
    /// Where last night sits against the balanced range.
    pub vs_baseline: Option<BaselinePosition>,
    /// One value per night for the last 4 weeks.
    pub values_4w: Vec<DateValue>,
}

pub fn hrv_status_label(code: i64) -> Option<&'static str> {
    match code {
        1 => Some("poor"),
        2 => Some("low"),
        3 => Some("unbalanced"),
        4 => Some("balanced"),
        _ => None,
    }
}

pub type SummaryRow = (
    i64,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<i64>,
);

/// Overnight HRV per local day for `from..=to` (the watch's last-night average,
/// else the mean of that day's HRV values) plus the summary rows in the range.
pub async fn hrv_by_day(
    ctx: &Ctx,
    from: NaiveDate,
    to: NaiveDate,
) -> Res<(BTreeMap<NaiveDate, f64>, Vec<SummaryRow>)> {
    let (lo, hi) = (ctx.day_bounds(from).0, ctx.day_bounds(to).1);
    let rows: Vec<SummaryRow> = sqlx::query_as(
        "SELECT ts, last_night_avg, weekly_avg, baseline_balanced_lower, baseline_balanced_upper, status
         FROM hrv_summary WHERE user_id = ? AND ts >= ? AND ts < ? ORDER BY ts",
    )
    .bind(ctx.uid)
    .bind(lo)
    .bind(hi)
    .fetch_all(&ctx.pool)
    .await?;
    let mut by_day: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    for r in &rows {
        if let Some(v) = r.1 {
            by_day.insert(ctx.date(r.0), v);
        }
    }
    // Days without a summary fall back to the mean of that day's overnight values.
    let vals = series(
        ctx,
        "SELECT ts, rmssd_ms FROM hrv_values WHERE user_id = ? AND ts >= ? AND ts < ? ORDER BY ts",
        lo,
        hi,
    )
    .await?;
    let mut sums: BTreeMap<NaiveDate, (f64, u32)> = BTreeMap::new();
    for (t, v) in vals {
        let e = sums.entry(ctx.date(t)).or_default();
        e.0 += v;
        e.1 += 1;
    }
    for (d, (s, n)) in sums {
        by_day.entry(d).or_insert_with(|| round1(s / f64::from(n)));
    }
    Ok((by_day, rows))
}

pub async fn hrv_block(ctx: &Ctx, date: NaiveDate) -> Res<HrvBlock> {
    let (by_day, rows) = hrv_by_day(ctx, date - Duration::days(27), date).await?;
    let latest = rows.iter().rev().find(|r| ctx.date(r.0) <= date);
    let last_night = by_day.get(&date).copied();
    let (low, high) = latest.map_or((None, None), |r| (r.3, r.4));
    let vs = match (last_night, low, high) {
        (Some(v), Some(l), _) if v < l => Some(BaselinePosition::Below),
        (Some(v), _, Some(h)) if v > h => Some(BaselinePosition::Above),
        (Some(_), Some(_), Some(_)) => Some(BaselinePosition::Within),
        _ => None,
    };
    let status = latest.and_then(|r| r.5);
    Ok(HrvBlock {
        last_night_ms: last_night,
        weekly_avg_ms: latest.and_then(|r| r.2),
        baseline_low_ms: low,
        baseline_high_ms: high,
        status_code: status.map(|s| s as i32),
        status: status.and_then(hrv_status_label).map(str::to_owned),
        vs_baseline: vs,
        values_4w: by_day
            .range(date - Duration::days(27)..=date)
            .map(|(d, v)| DateValue {
                date: crate::settings::date_string(*d),
                value: Some(*v),
            })
            .collect(),
    })
}

// ---- per-day numbers -----------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct DayStats {
    pub steps: Option<f64>,
    pub distance_m: Option<f64>,
    pub active_kcal: Option<f64>,
    pub resting_hr: Option<f64>,
    pub hr_low: Option<f64>,
    pub hr_high: Option<f64>,
    pub stress_avg: Option<f64>,
    pub energy: EnergyStats,
    pub moderate_min: Option<f64>,
    pub vigorous_min: Option<f64>,
}

/// The local day a cumulative counter reading counts for: the day its interval ends in, so a
/// reading stamped exactly at midnight closes the day before (see the module note).
pub fn counter_date(ctx: &Ctx, ts: i64) -> NaiveDate {
    ctx.date(ts - 1)
}

pub fn dates(from: NaiveDate, to: NaiveDate) -> impl Iterator<Item = NaiveDate> {
    std::iter::successors(Some(from), move |d| Some(*d + Duration::days(1)))
        .take_while(move |d| *d <= to)
}

/// Day-by-day figures for `from..=to` (small ranges: up to about 5 weeks).
pub async fn day_stats(
    ctx: &Ctx,
    from: NaiveDate,
    to: NaiveDate,
) -> Res<BTreeMap<NaiveDate, DayStats>> {
    let mut out: BTreeMap<NaiveDate, DayStats> =
        dates(from, to).map(|d| (d, DayStats::default())).collect();
    let (lo, hi) = (ctx.day_bounds(from).0, ctx.day_bounds(to).1);

    // Cumulative counters: largest value per (day, activity type), summed over types.
    type Row = (i64, i64, Option<f64>, Option<f64>, Option<f64>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT ts, activity_type, steps, distance_m, active_calories FROM mon_activity
         WHERE user_id = ? AND ts > ? AND ts <= ?",
    )
    .bind(ctx.uid)
    .bind(lo)
    .bind(hi)
    .fetch_all(&ctx.pool)
    .await?;
    type Max3 = (Option<f64>, Option<f64>, Option<f64>);
    let mut maxima: HashMap<(NaiveDate, i64), Max3> = HashMap::new();
    let up = |a: &mut Option<f64>, b: Option<f64>| {
        if let Some(b) = b {
            *a = Some(a.map_or(b, |x| x.max(b)));
        }
    };
    for (ts, ty, steps, dist, kcal) in rows {
        let e = maxima.entry((counter_date(ctx, ts), ty)).or_default();
        up(&mut e.0, steps);
        up(&mut e.1, dist);
        up(&mut e.2, kcal);
    }
    let add = |a: &mut Option<f64>, b: Option<f64>| {
        if let Some(b) = b {
            *a = Some(a.unwrap_or(0.0) + b);
        }
    };
    for ((d, _), (steps, dist, kcal)) in maxima {
        if let Some(s) = out.get_mut(&d) {
            add(&mut s.steps, steps);
            add(&mut s.distance_m, dist);
            add(&mut s.active_kcal, kcal);
        }
    }

    // Whole-day totals imported from Garmin Connect fill the days that have no minute data.
    let totals: Vec<(i64, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT ts, steps, active_kcal FROM daily_totals WHERE user_id = ? AND ts >= ? AND ts < ?",
    )
    .bind(ctx.uid)
    .bind(lo)
    .bind(hi)
    .fetch_all(&ctx.pool)
    .await?;
    for (ts, steps, kcal) in totals {
        if let Some(s) = out.get_mut(&ctx.date(ts)) {
            s.steps = s.steps.or(steps);
            s.active_kcal = s.active_kcal.or(kcal);
        }
    }
    // Connect's daily summaries: the same fallback for steps and distance, and the other metrics
    // below, each only where the watch has nothing for the day.
    let connect = connect_days(ctx, from, to).await?;
    for (d, c) in &connect {
        if let Some(s) = out.get_mut(d) {
            s.steps = s.steps.or(c.steps);
            s.distance_m = s.distance_m.or(c.distance_m);
        }
    }

    for (ts, bpm) in series(
        ctx,
        "SELECT ts, bpm FROM mon_hr WHERE user_id = ? AND ts >= ? AND ts < ?",
        lo,
        hi,
    )
    .await?
    {
        if let Some(s) = out.get_mut(&ctx.date(ts)) {
            s.hr_low = Some(s.hr_low.map_or(bpm, |x| x.min(bpm)));
            s.hr_high = Some(s.hr_high.map_or(bpm, |x| x.max(bpm)));
        }
    }
    for (d, c) in &connect {
        if let Some(s) = out.get_mut(d) {
            if s.hr_low.is_none() && s.hr_high.is_none() {
                s.hr_low = c.hr_min;
                s.hr_high = c.hr_max;
            }
        }
    }

    type StressRow = (i64, Option<f64>, Option<f64>, Option<f64>);
    let stress: Vec<StressRow> = sqlx::query_as(
        "SELECT ts, stress, stress_raw, body_battery FROM stress WHERE user_id = ? AND ts >= ? AND ts < ? ORDER BY ts",
    )
    .bind(ctx.uid)
    .bind(lo)
    .bind(hi)
    .fetch_all(&ctx.pool)
    .await?;
    let mut stress_by: HashMap<NaiveDate, Vec<f64>> = HashMap::new();
    let mut energy_by: HashMap<NaiveDate, Vec<(i64, f64)>> = HashMap::new();
    // Days with a watch stress reading (as `stress_block` counts them), whatever its value.
    let mut has_stress: HashSet<NaiveDate> = HashSet::new();
    for (ts, st, raw, bb) in stress {
        let d = ctx.date(ts);
        if st.is_some() || raw.is_some() {
            has_stress.insert(d);
        }
        if let Some(v) = st {
            stress_by.entry(d).or_default().push(v);
        }
        if let Some(v) = bb {
            energy_by.entry(d).or_default().push((ts, v));
        }
    }
    for (d, s) in out.iter_mut() {
        s.stress_avg = if has_stress.contains(d) {
            stress_by.get(d).and_then(|v| mean(v.iter().copied()))
        } else {
            connect.get(d).and_then(|c| c.stress_avg)
        };
        s.energy = match energy_by.get(d) {
            Some(v) => energy_stats(v),
            None => connect.get(d).map(ConnectDay::energy).unwrap_or_default(),
        };
    }

    for (d, v) in resting_by_day(ctx, from, to).await? {
        if let Some(s) = out.get_mut(&d) {
            s.resting_hr = Some(v);
        }
    }
    for (d, (m, v)) in intensity_days(ctx, from, to).await? {
        if let Some(s) = out.get_mut(&d) {
            s.moderate_min = m;
            s.vigorous_min = v;
        }
    }
    Ok(out)
}

pub fn week_start(d: NaiveDate) -> NaiveDate {
    d - Duration::days(i64::from(d.weekday().num_days_from_monday()))
}

/// Intensity minutes per local day (moderate, vigorous) for `from..=to`, from the
/// watch's running totals (see the module note). A day without a counter reading takes Connect's
/// daily minutes; Connect's minutes also count towards the week's running total, so the first
/// watch reading after them (the watch's total is the week so far) adds only what came after.
/// A day with neither but with heart-rate readings (the watch was worn) is 0; `None` otherwise.
pub async fn intensity_days(
    ctx: &Ctx,
    from: NaiveDate,
    to: NaiveDate,
) -> Res<BTreeMap<NaiveDate, (Option<f64>, Option<f64>)>> {
    let start = week_start(from);
    let rows: Vec<(i64, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT ts, moderate_total_min, vigorous_total_min FROM mon_extra
         WHERE user_id = ? AND ts > ? AND ts <= ? AND (moderate_total_min IS NOT NULL OR vigorous_total_min IS NOT NULL)
         ORDER BY ts",
    )
    .bind(ctx.uid)
    .bind(ctx.day_bounds(start).0)
    .bind(ctx.day_bounds(to).1)
    .fetch_all(&ctx.pool)
    .await?;
    let mut last: HashMap<NaiveDate, (Option<f64>, Option<f64>)> = HashMap::new();
    for (ts, m, v) in rows {
        let e = last.entry(counter_date(ctx, ts)).or_default();
        e.0 = m.or(e.0);
        e.1 = v.or(e.1);
    }
    let connect = connect_days(ctx, start, to).await?;
    // Days the watch was worn (any heart-rate reading). Such a day without a new running total
    // added no intensity minutes: it is 0, not unknown. One row per hour is enough to tell.
    let worn: HashSet<NaiveDate> = sqlx::query_scalar::<_, i64>(
        "SELECT MIN(ts) FROM mon_hr WHERE user_id = ? AND ts >= ? AND ts < ? GROUP BY ts / 3600",
    )
    .bind(ctx.uid)
    .bind(ctx.day_bounds(from).0)
    .bind(ctx.day_bounds(to).1)
    .fetch_all(&ctx.pool)
    .await?
    .into_iter()
    .map(|ts| ctx.date(ts))
    .collect();
    let (mut prev_m, mut prev_v) = (0.0, 0.0);
    // The running total so far came (in part) from Connect, not from a watch reading.
    let (mut seeded_m, mut seeded_v) = (false, false);
    let mut out = BTreeMap::new();
    for d in dates(start, to) {
        if d.weekday() == Weekday::Mon {
            (prev_m, prev_v) = (0.0, 0.0);
            (seeded_m, seeded_v) = (false, false);
        }
        let inc = |now: Option<f64>, prev: &mut f64, seeded: &mut bool| {
            now.map(|n| {
                let i = if n >= *prev {
                    n - *prev
                } else if *seeded {
                    // Below what Connect counted for the earlier days: nothing new.
                    0.0
                } else {
                    n
                };
                *prev = n;
                *seeded = false;
                i
            })
        };
        let (m, v) = match last.get(&d).copied() {
            Some(day) => (
                inc(day.0, &mut prev_m, &mut seeded_m),
                inc(day.1, &mut prev_v, &mut seeded_v),
            ),
            None => match connect
                .get(&d)
                .filter(|c| c.moderate_min.is_some() || c.vigorous_min.is_some())
            {
                Some(c) => {
                    prev_m += c.moderate_min.unwrap_or(0.0);
                    prev_v += c.vigorous_min.unwrap_or(0.0);
                    (seeded_m, seeded_v) = (true, true);
                    (c.moderate_min, c.vigorous_min)
                }
                None if worn.contains(&d) => (Some(0.0), Some(0.0)),
                None => (None, None),
            },
        };
        if d >= from {
            out.insert(d, (m, v));
        }
    }
    Ok(out)
}

/// Moderate + 2 x vigorous, `None` unless at least one is known.
pub fn intensity_total(m: Option<f64>, v: Option<f64>) -> Option<f64> {
    (m.is_some() || v.is_some()).then(|| m.unwrap_or(0.0) + 2.0 * v.unwrap_or(0.0))
}

#[cfg(test)]
mod tests {
    use super::stages_of;

    /// An Instinct 2 night (2026-11-19/20): sleep 22:10 to 07:49 and its 30 level rows, as
    /// (minutes after 22:10, level). The watch's own summary: deep 1h39, light 6h9, REM 1h22,
    /// awake 29m.
    #[test]
    fn rows_end_their_period_and_match_the_watch() {
        let rows = [
            (17, 2),
            (64, 3),
            (110, 2),
            (111, 1),
            (119, 2),
            (132, 3),
            (189, 2),
            (192, 1),
            (210, 2),
            (219, 1),
            (252, 2),
            (291, 3),
            (304, 2),
            (321, 4),
            (330, 1),
            (394, 2),
            (438, 4),
            (439, 1),
            (481, 2),
            (483, 1),
            (495, 2),
            (500, 4),
            (521, 2),
            (522, 1),
            (533, 2),
            (549, 4),
            (551, 1),
            (564, 2),
            (565, 1),
            (579, 2),
        ];
        let start = 1_000_000;
        let lv: Vec<(i64, i64)> = rows.iter().map(|&(m, l)| (start + m * 60, l)).collect();
        let st = stages_of(&lv, start, start + 579 * 60).expect("stages");
        let min = |s: f64| (s / 60.0).round() as i64;
        assert_eq!(
            (min(st.deep), min(st.light), min(st.rem), min(st.awake)),
            (99, 369, 82, 29)
        );
        assert_eq!(
            st.segments.first().map(|s| (s.0, s.1)),
            Some((start, start + 17 * 60))
        );
        // A row at the start time closes nothing and is not part of this night.
        assert!(stages_of(&[(start, 3)], start, start + 60).is_none());
    }
}
