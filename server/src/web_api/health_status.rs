//! `GET /api/health-status?date=`: last night's overnight values against the owner's own usual
//! range, and `GET /api/health-status/history?from&to`: the same for every night of a range (for
//! the 4-week charts).
//!
//! Four values are compared: overnight HRV, the lowest heart rate of the night, the average
//! breathing rate and the average blood oxygen (SpO2) while asleep. The usual range of a value is
//! worked out from the 28 nights before the night in question, and only when at least 14 of them
//! have the value: the mean plus and minus 1.5 times the standard deviation, and never narrower
//! than plus and minus 1 unit (bpm, ms, brpm, percentage point). This is a comparison with the
//! person's own habits, not a medical assessment. The rules are plain functions at the top of the
//! file so they can be tested without a database.

use super::daily::{night_detail, night_sessions};
use super::{round1, Ctx, RangeParams};
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
        .route("/api/health-status", get(status))
        .route("/api/health-status/history", get(history))
}

// ---- the rules ---------------------------------------------------------------------------------

/// How many nights before a night make up its usual range.
pub const WINDOW_NIGHTS: i64 = 28;
/// Fewest nights with a value that a usual range needs.
pub const MIN_NIGHTS: usize = 14;
/// How many standard deviations the range reaches to each side of the mean.
pub const SD_FACTOR: f64 = 1.5;
/// Narrowest allowed distance from the mean to each edge, in the value's own unit.
pub const MIN_HALF_WIDTH: f64 = 1.0;
/// The longest range the history answers, in days.
pub const MAX_HISTORY_DAYS: i64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HealthMetricKey {
    Hrv,
    RestingHr,
    Breathing,
    Spo2,
}

const ALL_METRICS: [HealthMetricKey; 4] = [
    HealthMetricKey::Hrv,
    HealthMetricKey::RestingHr,
    HealthMetricKey::Breathing,
    HealthMetricKey::Spo2,
];

impl HealthMetricKey {
    fn unit(self) -> &'static str {
        match self {
            HealthMetricKey::Hrv => "ms",
            HealthMetricKey::RestingHr => "bpm",
            HealthMetricKey::Breathing => "brpm",
            HealthMetricKey::Spo2 => "%",
        }
    }
}

/// Where a night's value sits against the usual range, or why that is unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    InRange,
    Above,
    Below,
    /// The night has a value, but fewer than [`MIN_NIGHTS`] earlier nights to compare it with.
    Calibrating,
    /// The night has no value for this metric (not measured).
    NoReading,
}

/// The usual range from earlier nights' values: `None` with fewer than [`MIN_NIGHTS`]. The mean
/// plus and minus [`SD_FACTOR`] times the sample standard deviation, each side at least
/// [`MIN_HALF_WIDTH`] wide; both edges rounded to one decimal, which is how they are shown and
/// how a value is compared with them.
pub fn typical_range(earlier: &[f64]) -> Option<(f64, f64)> {
    let n = earlier.len();
    if n < MIN_NIGHTS {
        return None;
    }
    let mean = earlier.iter().sum::<f64>() / n as f64;
    let var = earlier.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0);
    let half = (SD_FACTOR * var.sqrt()).max(MIN_HALF_WIDTH);
    Some((round1(mean - half), round1(mean + half)))
}

/// A value against a range; the edges count as inside.
pub fn state_of(value: f64, range: (f64, f64)) -> HealthState {
    if value < range.0 {
        HealthState::Below
    } else if value > range.1 {
        HealthState::Above
    } else {
        HealthState::InRange
    }
}

/// One night's figures for one metric.
#[derive(Debug, Clone, PartialEq)]
pub struct NightStatus {
    pub value: Option<f64>,
    pub range: Option<(f64, f64)>,
    pub state: HealthState,
    /// Earlier nights of the window that have the value.
    pub nights: usize,
}

/// The status of `date` from every known night's value of one metric.
pub fn night_status(values: &BTreeMap<NaiveDate, f64>, date: NaiveDate) -> NightStatus {
    let earlier: Vec<f64> = (1..=WINDOW_NIGHTS)
        .filter_map(|i| values.get(&(date - Duration::days(i))).copied())
        .collect();
    let value = values.get(&date).copied();
    let range = typical_range(&earlier);
    let state = match (value, range) {
        (None, _) => HealthState::NoReading,
        (Some(_), None) => HealthState::Calibrating,
        (Some(v), Some(r)) => state_of(v, r),
    };
    NightStatus {
        value,
        range,
        state,
        nights: earlier.len(),
    }
}

/// How many values are outside their range, and how many have a range to be outside of.
pub fn count_outside(states: &[HealthState]) -> (u32, u32) {
    let outside = states
        .iter()
        .filter(|s| matches!(s, HealthState::Above | HealthState::Below))
        .count() as u32;
    let compared = states
        .iter()
        .filter(|s| {
            matches!(
                s,
                HealthState::Above | HealthState::Below | HealthState::InRange
            )
        })
        .count() as u32;
    (outside, compared)
}

// ---- gathering ---------------------------------------------------------------------------------

type Values = BTreeMap<HealthMetricKey, BTreeMap<NaiveDate, f64>>;

/// Every night's four values for the local night dates `from..=to`, and the dates that have a night.
async fn gather(ctx: &Ctx, from: NaiveDate, to: NaiveDate) -> Res<(Values, Vec<NaiveDate>)> {
    let mut values: Values = ALL_METRICS.iter().map(|k| (*k, BTreeMap::new())).collect();
    let mut dates = Vec::new();
    for (date, session) in night_sessions(ctx, from, to).await? {
        dates.push(date);
        let n = night_detail(ctx, &session).await?;
        let found = [
            (HealthMetricKey::Hrv, n.hrv_ms),
            (HealthMetricKey::RestingHr, n.lowest_hr_bpm),
            (HealthMetricKey::Breathing, n.breathing_brpm),
            (HealthMetricKey::Spo2, n.spo2_avg_percent),
        ];
        for (key, v) in found {
            if let (Some(v), Some(map)) = (v.filter(|v| v.is_finite()), values.get_mut(&key)) {
                map.insert(date, round1(v));
            }
        }
    }
    Ok((values, dates))
}

// ---- GET /api/health-status --------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct HealthMetricStatus {
    pub key: HealthMetricKey,
    pub unit: String,
    /// The night's value, one decimal; `null` when it was not measured.
    pub value: Option<f64>,
    /// Edges of the usual range; `null` while calibrating.
    pub low: Option<f64>,
    pub high: Option<f64>,
    pub state: HealthState,
    /// Earlier nights (of the 28 before) that have this value.
    pub nights: u32,
    /// How many more nights are needed before a range can be drawn; 0 once there is one.
    pub nights_needed: u32,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct HealthStatusResponse {
    pub sample: bool,
    /// The night's date (the morning it ended on).
    pub date: String,
    /// Whether the watch recorded a night on this date.
    pub has_night: bool,
    pub metrics: Vec<HealthMetricStatus>,
    /// Values outside their usual range.
    pub outside: u32,
    /// Values that could be compared (they have a range).
    pub compared: u32,
}

fn metric_status(key: HealthMetricKey, s: &NightStatus) -> HealthMetricStatus {
    HealthMetricStatus {
        key,
        unit: key.unit().to_owned(),
        value: s.value,
        low: s.range.map(|r| r.0),
        high: s.range.map(|r| r.1),
        state: s.state,
        nights: s.nights as u32,
        nights_needed: MIN_NIGHTS.saturating_sub(s.nights) as u32,
    }
}

#[derive(Debug, Deserialize)]
struct DateParam {
    date: Option<String>,
}

async fn status(
    user: DataUser,
    State(state): State<AppState>,
    Query(p): Query<DateParam>,
) -> Res<Json<HealthStatusResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let today = ctx.today();
    let asked = match p.date.as_deref() {
        Some(s) => Some(Ctx::parse_date(s)?),
        None => None,
    };
    if asked.is_some_and(|d| d > today) {
        return Err(ApiError::bad_request("date must not be after today"));
    }
    let last = asked.unwrap_or(today);
    let (values, dates) = gather(&ctx, last - Duration::days(WINDOW_NIGHTS), last).await?;
    // Without a date: today's night, or last night's when today's has
    // not been recorded yet (the watch has not synced).
    let date = asked.unwrap_or_else(|| {
        dates
            .last()
            .copied()
            .filter(|d| *d >= today - Duration::days(1))
            .unwrap_or(today)
    });
    let metrics: Vec<HealthMetricStatus> = ALL_METRICS
        .iter()
        .map(|k| metric_status(*k, &night_status(&values[k], date)))
        .collect();
    let states: Vec<HealthState> = metrics.iter().map(|m| m.state).collect();
    let (outside, compared) = count_outside(&states);
    Ok(Json(HealthStatusResponse {
        sample: ctx.sample(),
        date: date_string(date),
        has_night: dates.contains(&date),
        metrics,
        outside,
        compared,
    }))
}

// ---- GET /api/health-status/history ------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct HealthStatusDay {
    pub date: String,
    pub value: Option<f64>,
    pub low: Option<f64>,
    pub high: Option<f64>,
    pub state: HealthState,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct HealthMetricHistory {
    pub key: HealthMetricKey,
    pub unit: String,
    /// Every date of the range, oldest first; a date without a night has `no_reading`.
    pub days: Vec<HealthStatusDay>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct HealthStatusHistory {
    pub sample: bool,
    pub from: String,
    pub to: String,
    pub metrics: Vec<HealthMetricHistory>,
}

async fn history(
    user: DataUser,
    State(state): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Res<Json<HealthStatusHistory>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let (from, to) = ctx.range(&p)?;
    if (to - from).num_days() >= MAX_HISTORY_DAYS {
        return Err(ApiError::bad_request(format!(
            "a range is at most {MAX_HISTORY_DAYS} days"
        )));
    }
    let (values, _) = gather(&ctx, from - Duration::days(WINDOW_NIGHTS), to).await?;
    let days: Vec<NaiveDate> = (0..=(to - from).num_days())
        .map(|i| from + Duration::days(i))
        .collect();
    let metrics = ALL_METRICS
        .iter()
        .map(|k| HealthMetricHistory {
            key: *k,
            unit: k.unit().to_owned(),
            days: days
                .iter()
                .map(|d| {
                    let s = night_status(&values[k], *d);
                    HealthStatusDay {
                        date: date_string(*d),
                        value: s.value,
                        low: s.range.map(|r| r.0),
                        high: s.range.map(|r| r.1),
                        state: s.state,
                    }
                })
                .collect(),
        })
        .collect();
    Ok(Json(HealthStatusHistory {
        sample: ctx.sample(),
        from: date_string(from),
        to: date_string(to),
        metrics,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 1).unwrap() + Duration::days(i64::from(day) - 1)
    }

    #[test]
    fn range_needs_fourteen_nights() {
        assert_eq!(typical_range(&vec![50.0; 13]), None);
        assert!(typical_range(&vec![50.0; 14]).is_some());
        assert_eq!(typical_range(&[]), None);
    }

    #[test]
    fn range_is_mean_plus_minus_one_and_a_half_sd() {
        // 7 nights of 40 and 7 of 60: mean 50, sample variance 14 * 100 / 13.
        let mut v = vec![40.0; 7];
        v.extend(vec![60.0; 7]);
        let sd = (1400.0_f64 / 13.0).sqrt();
        let (lo, hi) = typical_range(&v).unwrap();
        assert_eq!(lo, round1(50.0 - 1.5 * sd));
        assert_eq!(hi, round1(50.0 + 1.5 * sd));
    }

    #[test]
    fn range_is_never_narrower_than_one_unit_each_side() {
        // No variation at all: still 49 to 51 around 50.
        assert_eq!(typical_range(&vec![50.0; 20]), Some((49.0, 51.0)));
        // A tiny spread (1.5 sd is about 0.3) is widened to the minimum too.
        let v: Vec<f64> = (0..20)
            .map(|i| if i % 2 == 0 { 50.2 } else { 49.8 })
            .collect();
        assert_eq!(typical_range(&v), Some((49.0, 51.0)));
        // A wide spread is left alone: 10 and 30 give a half width of about 15.
        let v: Vec<f64> = (0..20)
            .map(|i| if i % 2 == 0 { 10.0 } else { 30.0 })
            .collect();
        let (lo, hi) = typical_range(&v).unwrap();
        assert!(lo < 5.0 && hi > 35.0, "{lo} {hi}");
    }

    #[test]
    fn edges_are_inside_and_beyond_them_is_outside() {
        let r = (49.0, 51.0);
        assert_eq!(state_of(49.0, r), HealthState::InRange);
        assert_eq!(state_of(51.0, r), HealthState::InRange);
        assert_eq!(state_of(48.9, r), HealthState::Below);
        assert_eq!(state_of(51.1, r), HealthState::Above);
    }

    fn series(days: std::ops::RangeInclusive<u32>, v: f64) -> BTreeMap<NaiveDate, f64> {
        days.map(|i| (d(i), v)).collect()
    }

    #[test]
    fn a_night_is_compared_with_the_28_before_it_only() {
        // Nights 1 to 28 at 50, night 29 at 52: above 49 to 51.
        let mut m = series(1..=28, 50.0);
        m.insert(d(29), 52.0);
        let s = night_status(&m, d(29));
        assert_eq!(s.state, HealthState::Above);
        assert_eq!(s.range, Some((49.0, 51.0)));
        assert_eq!(s.nights, 28);
        // Night 30 looks at nights 2 to 29, so night 1 is out of its window.
        let s = night_status(&m, d(30));
        assert_eq!(s.nights, 28);
        assert_eq!(s.state, HealthState::NoReading);
        // The night itself and later nights never count.
        m.insert(d(31), 90.0);
        assert_eq!(night_status(&m, d(29)).range, Some((49.0, 51.0)));
    }

    #[test]
    fn fewer_than_fourteen_earlier_nights_is_calibrating() {
        let mut m = series(1..=13, 50.0);
        m.insert(d(14), 50.0);
        let s = night_status(&m, d(14));
        assert_eq!(s.state, HealthState::Calibrating);
        assert_eq!(s.range, None);
        assert_eq!(s.nights, 13);
        // One more earlier night and the range exists.
        let mut m = series(1..=14, 50.0);
        m.insert(d(15), 50.0);
        let s = night_status(&m, d(15));
        assert_eq!(s.state, HealthState::InRange);
        assert_eq!(s.nights, 14);
    }

    #[test]
    fn a_night_without_a_value_has_no_reading_whatever_the_range() {
        let m = series(1..=20, 50.0);
        assert_eq!(night_status(&m, d(21)).state, HealthState::NoReading);
        assert_eq!(
            night_status(&BTreeMap::new(), d(5)).state,
            HealthState::NoReading
        );
    }

    #[test]
    fn gaps_in_the_window_count_only_the_nights_that_have_a_value() {
        // Every second night: 14 of the 28 earlier nights are enough.
        let m: BTreeMap<NaiveDate, f64> = (1..=28)
            .filter(|i| i % 2 == 0)
            .map(|i| (d(i), 50.0))
            .chain([(d(29), 50.0)])
            .collect();
        let s = night_status(&m, d(29));
        assert_eq!(s.nights, 14);
        assert_eq!(s.state, HealthState::InRange);
    }

    #[test]
    fn outside_counts_only_compared_values() {
        use HealthState::*;
        assert_eq!(count_outside(&[InRange, Above, Below, Calibrating]), (2, 3));
        assert_eq!(count_outside(&[Calibrating, NoReading]), (0, 0));
        assert_eq!(count_outside(&[InRange, InRange, InRange, InRange]), (0, 4));
    }
}
