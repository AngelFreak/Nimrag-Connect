//! `GET /api/readiness?date=`: Nimrag's own estimate of how ready the owner is for training, 0 to
//! 100, from six factors; and `GET /api/readiness/history?from&to`: the score and level per day.
//!
//! This is NOT Garmin's Training Readiness (the Instinct 2 does not write one). Each factor is
//! rated on five rungs (very poor 0, poor 25, moderate 50, good 75, very good 100); the score is
//! the weighted mean of the factors that are present. The rules are plain functions at the top of
//! this file ([`rate_sleep`], [`rate_load`], [`compute`] and so on) so they can be tested without a
//! database; the handlers only gather the inputs.
//!
//! The score is computed on every request from the existing tables (a handful of range queries),
//! so a newly arrived night, HRV summary or load row shows at once and there is nothing to expire.
//! For a day before today the "now" of the day is 08:00 local: the morning the estimate is for.

use super::daily::{connect_days, dates, nights};
use super::training::status_rows;
use super::{mean, round1, Ctx, RangeParams};
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
        .route("/api/readiness", get(readiness))
        .route("/api/readiness/history", get(history))
}

// ---- the rules ---------------------------------------------------------------------------------

/// One rung of a factor's rating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessRating {
    VeryPoor,
    Poor,
    Moderate,
    Good,
    VeryGood,
}

impl ReadinessRating {
    /// 0, 25, 50, 75 or 100.
    pub fn points(self) -> f64 {
        match self {
            ReadinessRating::VeryPoor => 0.0,
            ReadinessRating::Poor => 25.0,
            ReadinessRating::Moderate => 50.0,
            ReadinessRating::Good => 75.0,
            ReadinessRating::VeryGood => 100.0,
        }
    }
}

/// The six factors, in the order the UI lists them (heaviest first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessFactorKey {
    Sleep,
    Recovery,
    Hrv,
    Load,
    SleepHistory,
    Stress,
}

const ALL_FACTORS: [ReadinessFactorKey; 6] = [
    ReadinessFactorKey::Sleep,
    ReadinessFactorKey::Recovery,
    ReadinessFactorKey::Hrv,
    ReadinessFactorKey::Load,
    ReadinessFactorKey::SleepHistory,
    ReadinessFactorKey::Stress,
];

impl ReadinessFactorKey {
    pub fn weight(self) -> u32 {
        match self {
            ReadinessFactorKey::Sleep => 25,
            ReadinessFactorKey::Recovery | ReadinessFactorKey::Hrv => 20,
            ReadinessFactorKey::Load => 15,
            ReadinessFactorKey::SleepHistory | ReadinessFactorKey::Stress => 10,
        }
    }
    fn unit(self) -> &'static str {
        match self {
            ReadinessFactorKey::Sleep | ReadinessFactorKey::SleepHistory => "score",
            ReadinessFactorKey::Recovery => "h",
            ReadinessFactorKey::Hrv => "ms",
            ReadinessFactorKey::Load => "load",
            ReadinessFactorKey::Stress => "stress",
        }
    }
    /// How the sentence names the factor.
    fn phrase(self) -> &'static str {
        match self {
            ReadinessFactorKey::Sleep => "last night's sleep",
            ReadinessFactorKey::SleepHistory => "your sleep over the past week",
            ReadinessFactorKey::Recovery => "the recovery time still left",
            ReadinessFactorKey::Hrv => "your HRV",
            ReadinessFactorKey::Load => "your recent training load",
            ReadinessFactorKey::Stress => "your stress over the last days",
        }
    }
    fn group(self) -> Group {
        match self {
            ReadinessFactorKey::Sleep | ReadinessFactorKey::SleepHistory => Group::Sleep,
            ReadinessFactorKey::Recovery | ReadinessFactorKey::Hrv => Group::Body,
            ReadinessFactorKey::Load => Group::Load,
            ReadinessFactorKey::Stress => Group::Stress,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Sleep,
    Body,
    Load,
    Stress,
}

impl Group {
    fn name(self) -> &'static str {
        match self {
            Group::Sleep => "sleep",
            Group::Body => "body",
            Group::Load => "load",
            Group::Stress => "stress",
        }
    }
}

/// How the UI colours and names a score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessLevel {
    Poor,
    Low,
    Moderate,
    High,
    Prime,
}

pub fn level_of(score: u32) -> ReadinessLevel {
    match score {
        0..=24 => ReadinessLevel::Poor,
        25..=49 => ReadinessLevel::Low,
        50..=74 => ReadinessLevel::Moderate,
        75..=94 => ReadinessLevel::High,
        _ => ReadinessLevel::Prime,
    }
}

/// Sleep score bands, for last night and for the 7-night average.
pub fn rate_sleep(score: f64) -> ReadinessRating {
    match score {
        s if s >= 90.0 => ReadinessRating::VeryGood,
        s if s >= 80.0 => ReadinessRating::Good,
        s if s >= 60.0 => ReadinessRating::Moderate,
        s if s >= 40.0 => ReadinessRating::Poor,
        _ => ReadinessRating::VeryPoor,
    }
}

/// Recovery time left, in hours.
pub fn rate_recovery(hours_left: f64) -> ReadinessRating {
    match hours_left {
        h if h <= 0.0 => ReadinessRating::VeryGood,
        h if h <= 12.0 => ReadinessRating::Good,
        h if h <= 24.0 => ReadinessRating::Moderate,
        h if h <= 48.0 => ReadinessRating::Poor,
        _ => ReadinessRating::VeryPoor,
    }
}

/// Where the 7-day HRV average sits against the person's balanced range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HrvState {
    Balanced,
    Unbalanced,
    Low,
    Poor,
}

/// `last_night` and `band` only matter for a balanced state: balanced with last night at or above
/// the middle of the band is very good.
pub fn rate_hrv(
    state: HrvState,
    last_night: Option<f64>,
    band: Option<(f64, f64)>,
) -> ReadinessRating {
    match state {
        HrvState::Balanced => match (last_night, band) {
            (Some(n), Some((lo, hi))) if n >= (lo + hi) / 2.0 => ReadinessRating::VeryGood,
            _ => ReadinessRating::Good,
        },
        HrvState::Unbalanced => ReadinessRating::Moderate,
        HrvState::Low => ReadinessRating::Poor,
        HrvState::Poor => ReadinessRating::VeryPoor,
    }
}

/// The state of an HRV summary: the watch's own status code when it has one (1 poor, 2 low,
/// 3 unbalanced, 4 balanced, as in `hrv_status_label`), else the weekly average against the band.
pub fn hrv_state(
    code: Option<i64>,
    weekly: Option<f64>,
    band: Option<(f64, f64)>,
) -> Option<HrvState> {
    match code {
        Some(1) => return Some(HrvState::Poor),
        Some(2) => return Some(HrvState::Low),
        Some(3) => return Some(HrvState::Unbalanced),
        Some(4) => return Some(HrvState::Balanced),
        _ => {}
    }
    let (w, (lo, hi)) = (weekly?, band?);
    Some(if w < lo {
        HrvState::Low
    } else if w > hi {
        HrvState::Unbalanced
    } else {
        HrvState::Balanced
    })
}

/// The 7-day load against its target range: below it very good, lower half good, upper half
/// moderate, above poor, more than 1.5 times the top very poor.
pub fn rate_load(load: f64, min: f64, max: f64) -> ReadinessRating {
    if load < min {
        ReadinessRating::VeryGood
    } else if load <= (min + max) / 2.0 {
        ReadinessRating::Good
    } else if load <= max {
        ReadinessRating::Moderate
    } else if load <= max * 1.5 {
        ReadinessRating::Poor
    } else {
        ReadinessRating::VeryPoor
    }
}

/// Average all-day stress of the last three days.
pub fn rate_stress(avg: f64) -> ReadinessRating {
    match avg {
        s if s <= 25.0 => ReadinessRating::VeryGood,
        s if s <= 35.0 => ReadinessRating::Good,
        s if s <= 45.0 => ReadinessRating::Moderate,
        s if s <= 55.0 => ReadinessRating::Poor,
        _ => ReadinessRating::VeryPoor,
    }
}

/// Fewest factors for a score.
pub const MIN_FACTORS: usize = 4;
/// Fewest days with data (a night or a stress average) in the last 14 for a score.
pub const MIN_DAYS: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ReadinessFactor {
    pub key: ReadinessFactorKey,
    /// The measured value; `null` when the factor is missing.
    pub value: Option<f64>,
    /// `score`, `h`, `ms`, `load` or `stress`.
    pub unit: String,
    /// `null` when the factor is missing (it is then left out of the score).
    pub rating: Option<ReadinessRating>,
    pub weight: u32,
    /// A short plain line about the value (the target range, the balanced range), when there is one.
    pub note: Option<String>,
}

/// The facts one day's estimate is made from; built by [`gather`] from the tables.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    /// Sleep score of the night by the date it ends on.
    pub sleep: BTreeMap<NaiveDate, f64>,
    /// Dates with a night, scored or not.
    pub nights: Vec<NaiveDate>,
    /// All-day stress average by date.
    pub stress: BTreeMap<NaiveDate, f64>,
    /// HRV summaries, oldest first.
    pub hrv: Vec<HrvRow>,
    /// Training status rows with a load, oldest first: `(ts, load, target_min, target_max)`.
    pub load: Vec<(i64, f64, Option<f64>, Option<f64>)>,
    /// Recovery time rows, oldest first: `(start_ts, minutes)`.
    pub recovery: Vec<(i64, f64)>,
}

#[derive(Debug, Clone)]
pub struct HrvRow {
    pub date: NaiveDate,
    pub weekly_avg: Option<f64>,
    pub last_night: Option<f64>,
    pub low: Option<f64>,
    pub high: Option<f64>,
    pub status: Option<i64>,
}

/// The estimate of one day.
#[derive(Debug, Clone)]
pub struct Estimate {
    pub factors: Vec<ReadinessFactor>,
    pub present: usize,
    pub data_days: usize,
    pub score: Option<u32>,
}

fn factor(
    key: ReadinessFactorKey,
    value: Option<f64>,
    rating: Option<ReadinessRating>,
    note: Option<String>,
) -> ReadinessFactor {
    ReadinessFactor {
        key,
        value,
        unit: key.unit().to_owned(),
        rating,
        weight: key.weight(),
        note,
    }
}

/// The weighted mean of the present factors, rounded to a whole number (halves up).
pub fn weighted_score(factors: &[ReadinessFactor]) -> Option<u32> {
    let (sum, w) = factors
        .iter()
        .filter_map(|f| {
            f.rating
                .map(|r| (r.points() * f64::from(f.weight), f64::from(f.weight)))
        })
        .fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
    (w > 0.0).then(|| (sum / w + 0.5).floor() as u32)
}

/// The estimate for local day `date`; `asof` is the moment the day's figures are read at.
pub fn compute(inp: &Inputs, date: NaiveDate, asof: i64) -> Estimate {
    // Sleep: the night that ended on `date`, and the average of the 7 nights ending then.
    let last = inp.sleep.get(&date).copied();
    let week: Vec<f64> = inp
        .sleep
        .range(date - Duration::days(6)..=date)
        .map(|(_, v)| *v)
        .collect();
    let week_avg = if week.len() >= 3 {
        mean(week.iter().copied())
    } else {
        None
    };

    // Recovery time left at `asof`.
    let recovery = inp
        .recovery
        .iter()
        .rev()
        .find(|r| r.0 <= asof)
        .map(|(start, minutes)| {
            let left_s = (*start as f64 + minutes * 60.0 - asof as f64).max(0.0);
            round1(left_s / 3600.0)
        });

    // HRV: the latest summary of the last week.
    let hrv_row = inp
        .hrv
        .iter()
        .rev()
        .find(|r| r.date <= date && r.date >= date - Duration::days(6));
    let band = hrv_row.and_then(|r| r.low.zip(r.high));
    let hrv_rating = hrv_row.and_then(|r| {
        hrv_state(r.status, r.weekly_avg, band).map(|s| rate_hrv(s, r.last_night, band))
    });

    // Load: the newest row with a load and a target range, at most a week old.
    let load_row = inp
        .load
        .iter()
        .rev()
        .find(|r| r.0 <= asof && r.0 >= asof - 7 * 86_400);
    let load_rating = load_row.and_then(|r| Some(rate_load(r.1, r.2?, r.3?)));

    // Stress: the three days before `date`.
    let stress_days: Vec<f64> = inp
        .stress
        .range(date - Duration::days(3)..date)
        .map(|(_, v)| *v)
        .collect();
    let stress_avg = mean(stress_days.iter().copied());

    let factors = vec![
        factor(ReadinessFactorKey::Sleep, last, last.map(rate_sleep), None),
        factor(
            ReadinessFactorKey::Recovery,
            recovery,
            recovery.map(rate_recovery),
            None,
        ),
        factor(
            ReadinessFactorKey::Hrv,
            hrv_row.and_then(|r| r.weekly_avg),
            hrv_rating,
            band.map(|(lo, hi)| format!("Balanced range {}–{} ms", lo.round(), hi.round())),
        ),
        factor(
            ReadinessFactorKey::Load,
            load_row.map(|r| r.1),
            load_rating,
            load_row
                .and_then(|r| r.2.zip(r.3))
                .map(|(lo, hi)| format!("Target range {}–{}", lo.round(), hi.round())),
        ),
        factor(
            ReadinessFactorKey::SleepHistory,
            week_avg,
            week_avg.map(rate_sleep),
            None,
        ),
        factor(
            ReadinessFactorKey::Stress,
            stress_avg,
            stress_avg.map(rate_stress),
            None,
        ),
    ];
    let present = factors.iter().filter(|f| f.rating.is_some()).count();
    let recent = |d: &&NaiveDate| **d <= date && **d >= date - Duration::days(13);
    let mut days: Vec<NaiveDate> = inp
        .nights
        .iter()
        .chain(inp.stress.keys())
        .filter(recent)
        .copied()
        .collect();
    days.sort();
    days.dedup();
    let score = if present >= MIN_FACTORS && days.len() >= MIN_DAYS {
        weighted_score(&factors)
    } else {
        None
    };
    Estimate {
        factors,
        present,
        data_days: days.len(),
        score,
    }
}

// ---- the words ---------------------------------------------------------------------------------

/// The weakest factor that is at most moderate (ties go to the heavier factor), or `None` when
/// every present factor is good or better.
pub fn weakest(factors: &[ReadinessFactor]) -> Option<ReadinessFactorKey> {
    let mut best: Option<(ReadinessRating, ReadinessFactorKey)> = None;
    for k in ALL_FACTORS {
        let Some(r) = factors.iter().find(|f| f.key == k).and_then(|f| f.rating) else {
            continue;
        };
        if best.is_none_or(|(b, _)| r < b) {
            best = Some((r, k));
        }
    }
    best.filter(|(r, _)| *r <= ReadinessRating::Moderate)
        .map(|(_, k)| k)
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// The headline key, the headline and the sentence of a level. Eighteen pairs in all: for the
/// three middle levels one per weakest-factor group (sleep, body, load, stress), for High one per group
/// and one without a weak spot, and one for Prime.
pub fn words(
    level: ReadinessLevel,
    weak: Option<ReadinessFactorKey>,
) -> (String, &'static str, String) {
    let name = weak.map(|k| capitalised(k.phrase())).unwrap_or_default();
    let group = weak.map(ReadinessFactorKey::group);
    let advice = |g: Group, level: ReadinessLevel| -> &'static str {
        match (level, g) {
            (ReadinessLevel::Poor, Group::Sleep) => "Keep today very light and get to bed early.",
            (ReadinessLevel::Poor, Group::Body) => "Skip hard training and let your body recover.",
            (ReadinessLevel::Poor, Group::Load) => "Training has piled up, so rest today.",
            (ReadinessLevel::Poor, Group::Stress) => "Rest, and keep the day as calm as you can.",
            (ReadinessLevel::Low, Group::Sleep) => "Keep it easy and go to bed early tonight.",
            (ReadinessLevel::Low, Group::Body) => {
                "Easy movement only, and let recovery run its course."
            }
            (ReadinessLevel::Low, Group::Load) => "Recent training has been heavy, so go gently.",
            (ReadinessLevel::Low, Group::Stress) => "A gentle walk or an easy session will do.",
            (_, Group::Sleep) => "A steady session is fine, and protect tonight's sleep.",
            (_, Group::Body) => "A steady session is fine, but keep it controlled.",
            (_, Group::Load) => "Stick to your usual plan and avoid adding more.",
            (_, Group::Stress) => "A steady session may clear your head, but do not push hard.",
        }
    };
    match level {
        ReadinessLevel::Prime => (
            "prime".to_owned(),
            "Ready for a hard session",
            "Everything lines up. If you have a hard session planned, today is the day.".to_owned(),
        ),
        ReadinessLevel::High => match (group, weak) {
            (Some(g), Some(k)) => (
                format!("high_{}", g.name()),
                "Ready for a solid session",
                format!(
                    "You are ready for a hard session, though {} is a little behind.",
                    k.phrase()
                ),
            ),
            _ => (
                "high".to_owned(),
                "Ready for a solid session",
                "Your numbers look good, so this is a fine day for a hard session.".to_owned(),
            ),
        },
        ReadinessLevel::Poor | ReadinessLevel::Low | ReadinessLevel::Moderate => {
            let g = group.unwrap_or(Group::Body);
            let (key, headline) = match level {
                ReadinessLevel::Poor => ("poor", "Rest today"),
                ReadinessLevel::Low => ("low", "Take it easy today"),
                _ => ("moderate", "Go by feel"),
            };
            let lead = if weak.is_some() {
                format!("{name} is the weakest part.")
            } else {
                "Nothing stands out, but the whole picture is not strong.".to_owned()
            };
            (
                format!("{key}_{}", g.name()),
                headline,
                format!("{lead} {}", advice(g, level)),
            )
        }
    }
}

// ---- gathering ---------------------------------------------------------------------------------

/// Read what [`compute`] needs for the local days `from..=to` (and the 14 days before `from`).
pub async fn gather(ctx: &Ctx, from: NaiveDate, to: NaiveDate) -> Res<Inputs> {
    let first = from - Duration::days(14);
    let (lo, hi) = (ctx.day_bounds(first).0, ctx.day_bounds(to).1);
    let mut inp = Inputs::default();

    for n in nights(ctx, first, to).await? {
        if let Some(d) = crate::settings::parse_date(&n.date) {
            inp.nights.push(d);
            if let Some(s) = n.score {
                inp.sleep.insert(d, s);
            }
        }
    }

    // Stress: the mean of the day's readings, else Garmin Connect's day average.
    let rows: Vec<(i64, f64)> = sqlx::query_as(
        "SELECT ts, stress FROM stress WHERE user_id = ? AND ts >= ? AND ts < ? AND stress IS NOT NULL",
    )
    .bind(ctx.uid)
    .bind(lo)
    .bind(hi)
    .fetch_all(&ctx.pool)
    .await?;
    let mut sums: BTreeMap<NaiveDate, (f64, u32)> = BTreeMap::new();
    for (ts, v) in rows {
        let e = sums.entry(ctx.date(ts)).or_default();
        e.0 += v;
        e.1 += 1;
    }
    for (d, (s, n)) in sums {
        inp.stress.insert(d, round1(s / f64::from(n)));
    }
    for (d, c) in connect_days(ctx, first, to).await? {
        if let (false, Some(v)) = (inp.stress.contains_key(&d), c.stress_avg) {
            inp.stress.insert(d, v);
        }
    }

    type HrvSql = (
        i64,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<i64>,
    );
    let hrv: Vec<HrvSql> = sqlx::query_as(
        "SELECT ts, weekly_avg, last_night_avg, baseline_balanced_lower, baseline_balanced_upper, status
         FROM hrv_summary_current WHERE user_id = ? AND ts >= ? AND ts < ? ORDER BY ts",
    )
    .bind(ctx.uid)
    .bind(lo)
    .bind(hi)
    .fetch_all(&ctx.pool)
    .await?;
    inp.hrv = hrv
        .into_iter()
        .map(|(ts, weekly_avg, last_night, low, high, status)| HrvRow {
            date: ctx.date(ts),
            weekly_avg,
            last_night,
            low,
            high,
            status,
        })
        .collect();

    // A load row stays valid for a week, so read a week further back.
    inp.load = status_rows(ctx, lo - 7 * 86_400, hi)
        .await?
        .into_iter()
        .filter_map(|r| Some((r.0, r.2?, r.3, r.4)))
        .collect();

    inp.recovery = sqlx::query_as(
        "SELECT start_ts, minutes FROM recovery_time WHERE user_id = ? AND start_ts >= ? AND start_ts < ? ORDER BY start_ts",
    )
    .bind(ctx.uid)
    .bind(lo - 10 * 86_400)
    .bind(hi)
    .fetch_all(&ctx.pool)
    .await?;
    Ok(inp)
}

/// The moment a day's figures are read at: now for today, 08:00 local for an earlier day.
fn asof(ctx: &Ctx, date: NaiveDate) -> i64 {
    if date >= ctx.today() {
        ctx.now
    } else {
        ctx.day_bounds(date).0 + 8 * 3600
    }
}

// ---- GET /api/readiness ------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ReadinessResponse {
    pub sample: bool,
    pub date: String,
    /// 0 to 100; `null` without enough data.
    pub score: Option<u32>,
    pub level: Option<ReadinessLevel>,
    /// Which of the headline/sentence pairs applies (`low_sleep`, `prime`, ...).
    pub headline_key: Option<String>,
    pub headline: Option<String>,
    pub sentence: Option<String>,
    /// All six factors, heaviest first; a missing one has `rating: null`.
    pub factors: Vec<ReadinessFactor>,
    /// Factors present (rated).
    pub present: u32,
    /// Days with data among the last 14.
    pub data_days: u32,
    /// Why there is no score, in plain words.
    pub unavailable: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DateParam {
    date: Option<String>,
}

fn unavailable_text(e: &Estimate) -> String {
    if e.data_days < MIN_DAYS {
        format!("Readiness needs at least {MIN_DAYS} days of data. Sync your watch for a few days.")
    } else {
        format!(
            "Readiness needs at least {MIN_FACTORS} of its 6 factors, and only {} {} available.",
            e.present,
            if e.present == 1 { "is" } else { "are" }
        )
    }
}

async fn readiness(
    user: DataUser,
    State(state): State<AppState>,
    Query(p): Query<DateParam>,
) -> Res<Json<ReadinessResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let today = ctx.today();
    let date = match p.date.as_deref() {
        Some(s) => Ctx::parse_date(s)?,
        None => today,
    };
    if date > today {
        return Err(ApiError::bad_request("date must not be after today"));
    }
    let inp = gather(&ctx, date, date).await?;
    let e = compute(&inp, date, asof(&ctx, date));
    let level = e.score.map(level_of);
    let said = level.map(|l| words(l, weakest(&e.factors)));
    Ok(Json(ReadinessResponse {
        sample: ctx.sample(),
        date: date_string(date),
        score: e.score,
        level,
        headline_key: said.as_ref().map(|w| w.0.clone()),
        headline: said.as_ref().map(|w| w.1.to_owned()),
        sentence: said.map(|w| w.2),
        unavailable: e.score.is_none().then(|| unavailable_text(&e)),
        present: e.present as u32,
        data_days: e.data_days as u32,
        factors: e.factors,
    }))
}

// ---- GET /api/readiness/history ----------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ReadinessDay {
    pub date: String,
    pub score: Option<u32>,
    pub level: Option<ReadinessLevel>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ReadinessHistory {
    pub sample: bool,
    pub from: String,
    pub to: String,
    /// Every day of the range, oldest first; a day without a score has `null`s.
    pub days: Vec<ReadinessDay>,
    /// Mean of the scores in the range, to one decimal.
    pub average: Option<f64>,
}

async fn history(
    user: DataUser,
    State(state): State<AppState>,
    Query(p): Query<RangeParams>,
) -> Res<Json<ReadinessHistory>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let (from, to) = ctx.range(&p)?;
    let inp = gather(&ctx, from, to).await?;
    let days: Vec<ReadinessDay> = dates(from, to)
        .map(|d| {
            let score = compute(&inp, d, asof(&ctx, d)).score;
            ReadinessDay {
                date: date_string(d),
                score,
                level: score.map(level_of),
            }
        })
        .collect();
    Ok(Json(ReadinessHistory {
        sample: ctx.sample(),
        from: date_string(from),
        to: date_string(to),
        average: mean(days.iter().filter_map(|d| d.score.map(f64::from))),
        days,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
    }

    #[test]
    fn sleep_bands() {
        assert_eq!(rate_sleep(90.0), ReadinessRating::VeryGood);
        assert_eq!(rate_sleep(89.9), ReadinessRating::Good);
        assert_eq!(rate_sleep(80.0), ReadinessRating::Good);
        assert_eq!(rate_sleep(79.0), ReadinessRating::Moderate);
        assert_eq!(rate_sleep(60.0), ReadinessRating::Moderate);
        assert_eq!(rate_sleep(59.0), ReadinessRating::Poor);
        assert_eq!(rate_sleep(40.0), ReadinessRating::Poor);
        assert_eq!(rate_sleep(39.0), ReadinessRating::VeryPoor);
    }

    #[test]
    fn recovery_bands() {
        assert_eq!(rate_recovery(0.0), ReadinessRating::VeryGood);
        assert_eq!(rate_recovery(0.1), ReadinessRating::Good);
        assert_eq!(rate_recovery(12.0), ReadinessRating::Good);
        assert_eq!(rate_recovery(12.1), ReadinessRating::Moderate);
        assert_eq!(rate_recovery(24.0), ReadinessRating::Moderate);
        assert_eq!(rate_recovery(48.0), ReadinessRating::Poor);
        assert_eq!(rate_recovery(48.1), ReadinessRating::VeryPoor);
    }

    #[test]
    fn hrv_states_and_ratings() {
        let band = Some((50.0, 70.0));
        assert_eq!(
            rate_hrv(HrvState::Balanced, Some(60.0), band),
            ReadinessRating::VeryGood
        );
        assert_eq!(
            rate_hrv(HrvState::Balanced, Some(59.9), band),
            ReadinessRating::Good
        );
        assert_eq!(
            rate_hrv(HrvState::Balanced, None, band),
            ReadinessRating::Good
        );
        assert_eq!(
            rate_hrv(HrvState::Unbalanced, Some(80.0), band),
            ReadinessRating::Moderate
        );
        assert_eq!(rate_hrv(HrvState::Low, None, band), ReadinessRating::Poor);
        assert_eq!(
            rate_hrv(HrvState::Poor, None, band),
            ReadinessRating::VeryPoor
        );
        assert_eq!(hrv_state(Some(4), None, None), Some(HrvState::Balanced));
        assert_eq!(hrv_state(Some(2), None, None), Some(HrvState::Low));
        // No status: the weekly average against the band; no band: no state.
        assert_eq!(hrv_state(None, Some(45.0), band), Some(HrvState::Low));
        assert_eq!(
            hrv_state(None, Some(75.0), band),
            Some(HrvState::Unbalanced)
        );
        assert_eq!(hrv_state(None, Some(60.0), band), Some(HrvState::Balanced));
        assert_eq!(hrv_state(None, Some(60.0), None), None);
        assert_eq!(hrv_state(Some(0), None, None), None);
    }

    #[test]
    fn load_against_target_range() {
        // Range 200 to 400: the middle is 300 and "far above" starts past 600.
        assert_eq!(rate_load(199.0, 200.0, 400.0), ReadinessRating::VeryGood);
        assert_eq!(rate_load(200.0, 200.0, 400.0), ReadinessRating::Good);
        assert_eq!(rate_load(300.0, 200.0, 400.0), ReadinessRating::Good);
        assert_eq!(rate_load(301.0, 200.0, 400.0), ReadinessRating::Moderate);
        assert_eq!(rate_load(400.0, 200.0, 400.0), ReadinessRating::Moderate);
        assert_eq!(rate_load(401.0, 200.0, 400.0), ReadinessRating::Poor);
        assert_eq!(rate_load(600.0, 200.0, 400.0), ReadinessRating::Poor);
        assert_eq!(rate_load(601.0, 200.0, 400.0), ReadinessRating::VeryPoor);
    }

    #[test]
    fn stress_bands() {
        assert_eq!(rate_stress(25.0), ReadinessRating::VeryGood);
        assert_eq!(rate_stress(25.1), ReadinessRating::Good);
        assert_eq!(rate_stress(35.0), ReadinessRating::Good);
        assert_eq!(rate_stress(45.0), ReadinessRating::Moderate);
        assert_eq!(rate_stress(55.0), ReadinessRating::Poor);
        assert_eq!(rate_stress(55.1), ReadinessRating::VeryPoor);
    }

    #[test]
    fn levels() {
        assert_eq!(level_of(0), ReadinessLevel::Poor);
        assert_eq!(level_of(24), ReadinessLevel::Poor);
        assert_eq!(level_of(25), ReadinessLevel::Low);
        assert_eq!(level_of(49), ReadinessLevel::Low);
        assert_eq!(level_of(50), ReadinessLevel::Moderate);
        assert_eq!(level_of(74), ReadinessLevel::Moderate);
        assert_eq!(level_of(75), ReadinessLevel::High);
        assert_eq!(level_of(94), ReadinessLevel::High);
        assert_eq!(level_of(95), ReadinessLevel::Prime);
        assert_eq!(level_of(100), ReadinessLevel::Prime);
    }

    const ASOF: i64 = 1_000_000;

    /// Everything present: last night 85 (good), the other six nights 70 or 74 (the week average
    /// is moderate), stress 30 (good), HRV balanced with last night under the middle (good), load
    /// 250 in the lower half of 200 to 400 (good), recovery done long ago (very good).
    fn full() -> Inputs {
        let mut i = Inputs::default();
        for (n, day) in (13..=19).enumerate() {
            let v = if day == 19 {
                85.0
            } else if n % 2 == 0 {
                70.0
            } else {
                74.0
            };
            i.sleep.insert(d(day), v);
            i.nights.push(d(day));
            i.stress.insert(d(day), 30.0);
        }
        i.hrv.push(HrvRow {
            date: d(19),
            weekly_avg: Some(60.0),
            last_night: Some(55.0),
            low: Some(50.0),
            high: Some(70.0),
            status: Some(4),
        });
        i.load.push((ASOF - 3600, 250.0, Some(200.0), Some(400.0)));
        i.recovery.push((ASOF - 3 * 86_400, 600.0));
        i
    }

    fn rating(e: &Estimate, k: ReadinessFactorKey) -> Option<ReadinessRating> {
        e.factors.iter().find(|f| f.key == k).unwrap().rating
    }

    #[test]
    fn score_is_the_weighted_mean_of_the_factors() {
        let e = compute(&full(), d(19), ASOF);
        assert_eq!(e.present, 6);
        assert_eq!(
            rating(&e, ReadinessFactorKey::Sleep),
            Some(ReadinessRating::Good)
        );
        assert_eq!(
            rating(&e, ReadinessFactorKey::Recovery),
            Some(ReadinessRating::VeryGood)
        );
        assert_eq!(
            rating(&e, ReadinessFactorKey::Hrv),
            Some(ReadinessRating::Good)
        );
        assert_eq!(
            rating(&e, ReadinessFactorKey::Load),
            Some(ReadinessRating::Good)
        );
        // Nights 85, 70, 74, 70, 74, 70, 74 average 73.8: moderate.
        assert_eq!(
            rating(&e, ReadinessFactorKey::SleepHistory),
            Some(ReadinessRating::Moderate)
        );
        assert_eq!(
            rating(&e, ReadinessFactorKey::Stress),
            Some(ReadinessRating::Good)
        );
        // 75*25 + 100*20 + 75*20 + 75*15 + 50*10 + 75*10 = 7750 over 100.
        assert_eq!(e.score, Some(78));
    }

    #[test]
    fn missing_factors_are_renormalised_and_four_are_enough() {
        let mut i = full();
        i.hrv.clear(); // no baseline yet
        i.recovery.clear();
        let e = compute(&i, d(19), ASOF);
        assert_eq!(e.present, 4);
        let hrv = e
            .factors
            .iter()
            .find(|f| f.key == ReadinessFactorKey::Hrv)
            .unwrap();
        assert!(hrv.value.is_none() && hrv.rating.is_none());
        // Sleep 75 (25), load 75 (15), history 50 (10), stress 75 (10): 4250 / 60 = 70.8.
        assert_eq!(e.score, Some(71));
        // A fifth one missing: no score.
        i.load.clear();
        let e = compute(&i, d(19), ASOF);
        assert_eq!(e.present, 3);
        assert_eq!(e.score, None);
    }

    #[test]
    fn fewer_than_three_days_of_data_gives_no_score() {
        let mut i = full();
        i.sleep.retain(|k, _| *k >= d(18));
        i.nights.retain(|k| *k >= d(18));
        i.stress.retain(|k, _| *k >= d(18));
        let e = compute(&i, d(19), ASOF);
        assert_eq!(e.data_days, 2);
        assert_eq!(e.score, None);
    }

    #[test]
    fn stress_uses_the_three_days_before_and_history_needs_three_nights() {
        let mut i = Inputs::default();
        i.stress.insert(d(15), 90.0); // too old
        i.stress.insert(d(16), 20.0);
        i.stress.insert(d(18), 30.0);
        i.stress.insert(d(19), 99.0); // the day itself does not count
        i.sleep.insert(d(18), 60.0);
        i.sleep.insert(d(19), 70.0);
        let e = compute(&i, d(19), ASOF);
        let f = |k: ReadinessFactorKey| e.factors.iter().find(|f| f.key == k).unwrap();
        assert_eq!(f(ReadinessFactorKey::Stress).value, Some(25.0));
        assert_eq!(
            f(ReadinessFactorKey::Stress).rating,
            Some(ReadinessRating::VeryGood)
        );
        assert_eq!(f(ReadinessFactorKey::Sleep).value, Some(70.0));
        assert!(
            f(ReadinessFactorKey::SleepHistory).value.is_none(),
            "two nights are not a history"
        );
    }

    #[test]
    fn recovery_counts_down_from_the_watch_value() {
        let mut i = Inputs::default();
        i.recovery.push((1000, 30.0 * 60.0)); // 30 h from ts 1000
        let e = compute(&i, d(19), 1000 + 6 * 3600);
        let rec = e
            .factors
            .iter()
            .find(|f| f.key == ReadinessFactorKey::Recovery)
            .unwrap();
        assert_eq!(rec.value, Some(24.0));
        assert_eq!(rec.rating, Some(ReadinessRating::Moderate));
        // A row that starts after the moment is not known yet.
        assert!(compute(&i, d(19), 500).factors[1].value.is_none());
    }

    #[test]
    fn a_load_row_older_than_a_week_does_not_count() {
        let mut i = full();
        i.load[0].0 = ASOF - 8 * 86_400;
        let e = compute(&i, d(19), ASOF);
        assert!(e
            .factors
            .iter()
            .find(|f| f.key == ReadinessFactorKey::Load)
            .unwrap()
            .value
            .is_none());
        // A row without a target range gives a value but no rating.
        i.load[0] = (ASOF - 60, 250.0, None, None);
        let e = compute(&i, d(19), ASOF);
        let load = e
            .factors
            .iter()
            .find(|f| f.key == ReadinessFactorKey::Load)
            .unwrap();
        assert_eq!((load.value, load.rating), (Some(250.0), None));
    }

    fn fs(ratings: [Option<ReadinessRating>; 6]) -> Vec<ReadinessFactor> {
        ALL_FACTORS
            .iter()
            .zip(ratings)
            .map(|(k, r)| factor(*k, None, r, None))
            .collect()
    }

    #[test]
    fn the_weakest_factor_is_named_only_when_it_is_not_good() {
        use ReadinessRating::*;
        let all = |r| [Some(r); 6];
        assert_eq!(weakest(&fs(all(Good))), None);
        assert_eq!(weakest(&fs(all(VeryGood))), None);
        let mut r = all(Good);
        r[3] = Some(Moderate);
        assert_eq!(weakest(&fs(r)), Some(ReadinessFactorKey::Load));
        // A tie goes to the heavier factor (the earlier one in the list).
        r[0] = Some(Poor);
        r[5] = Some(Poor);
        assert_eq!(weakest(&fs(r)), Some(ReadinessFactorKey::Sleep));
        // A missing factor is skipped.
        r[0] = None;
        assert_eq!(weakest(&fs(r)), Some(ReadinessFactorKey::Stress));
    }

    #[test]
    fn headline_and_sentence_rules() {
        let (k, h, s) = words(ReadinessLevel::Low, Some(ReadinessFactorKey::Sleep));
        assert_eq!((k.as_str(), h), ("low_sleep", "Take it easy today"));
        assert!(
            s.starts_with("Last night's sleep is the weakest part."),
            "{s}"
        );
        let (k, h, _) = words(ReadinessLevel::Poor, Some(ReadinessFactorKey::Hrv));
        assert_eq!((k.as_str(), h), ("poor_body", "Rest today"));
        let (k, _, s) = words(ReadinessLevel::Moderate, Some(ReadinessFactorKey::Load));
        assert_eq!(k, "moderate_load");
        assert!(
            s.starts_with("Your recent training load is the weakest part."),
            "{s}"
        );
        let (k, h, _) = words(ReadinessLevel::High, None);
        assert_eq!((k.as_str(), h), ("high", "Ready for a solid session"));
        let (k, _, s) = words(ReadinessLevel::High, Some(ReadinessFactorKey::Stress));
        assert_eq!(k, "high_stress");
        assert!(s.contains("your stress over the last days"), "{s}");
        let (k, h, _) = words(ReadinessLevel::Prime, None);
        assert_eq!((k.as_str(), h), ("prime", "Ready for a hard session"));
        // Every level and group gives a non-empty pair; the keys are distinct pairs.
        let mut keys = std::collections::BTreeSet::new();
        for l in [
            ReadinessLevel::Poor,
            ReadinessLevel::Low,
            ReadinessLevel::Moderate,
            ReadinessLevel::High,
            ReadinessLevel::Prime,
        ] {
            for w in [
                None,
                Some(ReadinessFactorKey::Sleep),
                Some(ReadinessFactorKey::Hrv),
                Some(ReadinessFactorKey::Load),
                Some(ReadinessFactorKey::Stress),
            ] {
                let (k, h, s) = words(l, w);
                assert!(!h.is_empty() && !s.is_empty());
                keys.insert(k);
            }
        }
        assert_eq!(keys.len(), 18, "{keys:?}");
    }

    #[test]
    fn weighted_score_rounds_halves_up() {
        let f = |k, r| factor(k, None, Some(r), None);
        // (75*25 + 50*20) / 45 = 63.9
        assert_eq!(
            weighted_score(&[
                f(ReadinessFactorKey::Sleep, ReadinessRating::Good),
                f(ReadinessFactorKey::Recovery, ReadinessRating::Moderate)
            ]),
            Some(64)
        );
        // (75*25 + 50*15) / 40 = 65.625; (100*25 + 25*10)/35 = 78.57.
        assert_eq!(
            weighted_score(&[
                f(ReadinessFactorKey::Sleep, ReadinessRating::Good),
                f(ReadinessFactorKey::Load, ReadinessRating::Moderate)
            ]),
            Some(66)
        );
        assert_eq!(weighted_score(&[]), None);
    }
}
