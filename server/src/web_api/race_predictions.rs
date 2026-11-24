//! `GET /api/race-predictions`: Nimrag's own estimate of what the owner could run over 5 km, 10 km,
//! a half marathon and a marathon today, and week by week over the last 26 weeks.
//!
//! This is NOT Garmin's Race Predictor. Two public formulas are blended:
//! - from the best efforts of the last 42 days (the cache behind the personal records), scaled to
//!   the target distance with Riegel's `T = t * (D / d)^1.06`;
//! - from the VO2 max with the Daniels and Gilbert oxygen cost and drop-off curves.
//!
//! The formulas are plain functions at the top of the file so they can be tested without a
//! database; the handler only gathers the inputs.

use super::records::{load_acts, load_efforts, RUN_EFFORTS};
use super::training::{vo2_from_met, vo2_readings};
use super::Ctx;
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::settings::date_string;
use axum::{extract::State, routing::get, Json, Router};
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/race-predictions", get(race_predictions))
}

/// The distances predicted, in metres.
pub const DISTANCES_M: [f64; 4] = [5000.0, 10000.0, 21097.5, 42195.0];

/// Efforts from this many days before the date count.
pub const WINDOW_DAYS: i64 = 42;
/// Weekly history length.
pub const HISTORY_WEEKS: i64 = 26;
/// Riegel's exponent.
const RIEGEL_EXPONENT: f64 = 1.06;
/// The shortest effort that predicts anything (metres).
const MIN_EFFORT_M: f64 = 1000.0;
/// An effort longer than this many times the target distance is not used.
const MAX_EFFORT_RATIO: f64 = 2.5;
/// Share of the blend that comes from the best efforts when both sources exist.
const EFFORTS_WEIGHT: f64 = 0.7;
const DAY: i64 = 86_400;

// ---- the formulas ------------------------------------------------------------------------------

/// Riegel: the time over `to_m` from a time `secs` over `from_m`.
pub fn riegel(secs: f64, from_m: f64, to_m: f64) -> f64 {
    secs * (to_m / from_m).powf(RIEGEL_EXPONENT)
}

/// Oxygen cost (ml/kg/min) of running at `v` metres per minute.
fn oxygen_cost(v: f64) -> f64 {
    -4.60 + 0.182258 * v + 0.000104 * v * v
}

/// The share of VO2 max a runner can hold for `t` minutes.
fn sustained_share(t: f64) -> f64 {
    0.8 + 0.1894393 * (-0.012778 * t).exp() + 0.2989558 * (-0.1932605 * t).exp()
}

/// Daniels and Gilbert: the time in seconds over `distance_m` for a VO2 max (ml/kg/min), found by
/// bisection on the duration. `None` for a VO2 max that is not a positive number.
pub fn daniels_seconds(vo2max: f64, distance_m: f64) -> Option<f64> {
    if !vo2max.is_finite() || vo2max <= 0.0 || distance_m <= 0.0 {
        return None;
    }
    // The VO2 max a race of `t` minutes over the distance asks for; it falls as `t` grows.
    let needed = |t: f64| oxygen_cost(distance_m / t) / sustained_share(t);
    let (mut lo, mut hi) = (1.0f64, 1000.0f64);
    if needed(lo) < vo2max || needed(hi) > vo2max {
        return None;
    }
    for _ in 0..100 {
        let mid = (lo + hi) / 2.0;
        if needed(mid) > vo2max {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some((lo + hi) / 2.0 * 60.0)
}

/// The estimate from best efforts `(distance in metres, seconds)` over `target_m`: Riegel from the
/// three efforts closest in distance to the target (the nearer, the better the scaling holds),
/// the median of the three. Only efforts of 1 km or more, and at most 2.5 times the target, count.
pub fn effort_estimate(efforts: &[(f64, f64)], target_m: f64) -> Option<f64> {
    let mut usable: Vec<(f64, f64)> = efforts
        .iter()
        .copied()
        .filter(|&(d, t)| {
            d >= MIN_EFFORT_M && d <= MAX_EFFORT_RATIO * target_m && t > 0.0 && t.is_finite()
        })
        .collect();
    usable.sort_by(|a, b| {
        let da = (a.0 / target_m).ln().abs();
        let db = (b.0 / target_m).ln().abs();
        da.total_cmp(&db).then(a.0.total_cmp(&b.0))
    });
    usable.truncate(3);
    let mut preds: Vec<f64> = usable
        .iter()
        .map(|&(d, t)| riegel(t, d, target_m))
        .collect();
    preds.sort_by(f64::total_cmp);
    match preds.len() {
        0 => None,
        n if n % 2 == 1 => Some(preds[n / 2]),
        n => Some((preds[n / 2 - 1] + preds[n / 2]) / 2.0),
    }
}

/// What a prediction is based on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum RaceBasis {
    Efforts,
    Vo2max,
    Both,
}

/// 70 % efforts and 30 % VO2 max when both exist, else the one that does.
pub fn blend(efforts: Option<f64>, vo2: Option<f64>) -> Option<(f64, RaceBasis)> {
    match (efforts, vo2) {
        (Some(e), Some(v)) => Some((
            EFFORTS_WEIGHT * e + (1.0 - EFFORTS_WEIGHT) * v,
            RaceBasis::Both,
        )),
        (Some(e), None) => Some((e, RaceBasis::Efforts)),
        (None, Some(v)) => Some((v, RaceBasis::Vo2max)),
        (None, None) => None,
    }
}

/// The four predictions `(distance, whole seconds)` from the best efforts and the VO2 max, with
/// what they are based on; `None` when neither source exists.
pub fn predict(efforts: &[(f64, f64)], vo2: Option<f64>) -> Option<(RaceBasis, Vec<(f64, f64)>)> {
    let mut basis: Option<RaceBasis> = None;
    let mut out = Vec::new();
    for d in DISTANCES_M {
        let e = effort_estimate(efforts, d);
        let v = vo2.and_then(|v| daniels_seconds(v, d));
        let Some((secs, b)) = blend(e, v) else {
            continue;
        };
        basis = Some(basis.map_or(b, |x| if x == b { x } else { RaceBasis::Both }));
        out.push((d, secs.round()));
    }
    basis.map(|b| (b, out))
}

// ---- the response ------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RacePrediction {
    pub distance_m: f64,
    /// Whole seconds.
    pub seconds: f64,
    pub pace_s_per_km: f64,
    /// Seconds against the estimate 4 weeks earlier (negative is faster); `null` without one.
    pub change_4w_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RaceHistoryPoint {
    /// The Sunday the estimate is computed as of.
    pub date: String,
    pub predictions: Vec<RacePrediction>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct RacePredictionsResponse {
    pub sample: bool,
    pub as_of: String,
    /// `null` when there is nothing to estimate from.
    pub basis: Option<RaceBasis>,
    /// The VO2 max (ml/kg/min) used, if any.
    pub vo2max: Option<f64>,
    pub predictions: Vec<RacePrediction>,
    /// One point per week for the last 26 weeks, oldest first; weeks without an estimate are left out.
    pub history: Vec<RaceHistoryPoint>,
}

fn to_predictions(list: &[(f64, f64)], earlier: Option<&[(f64, f64)]>) -> Vec<RacePrediction> {
    list.iter()
        .map(|&(d, secs)| RacePrediction {
            distance_m: d,
            seconds: secs,
            pace_s_per_km: secs / (d / 1000.0),
            change_4w_s: earlier
                .and_then(|b| b.iter().find(|p| p.0 == d))
                .map(|p| secs - p.1),
        })
        .collect()
}

/// Everything the estimate reads, loaded once.
struct Inputs {
    /// `(start ts, [(distance m, seconds)])` per run.
    runs: Vec<(i64, Vec<(f64, f64)>)>,
    /// `(ts, ml/kg/min)`, oldest first.
    vo2: Vec<(i64, f64)>,
}

type Estimate = Option<(RaceBasis, Vec<(f64, f64)>)>;

impl Inputs {
    /// The estimate as of `ts`: efforts of runs in the 42 days up to it, the latest VO2 max before it.
    fn at(&self, ts: i64) -> (Option<f64>, Estimate) {
        let from = ts - WINDOW_DAYS * DAY;
        let mut best: Vec<(f64, f64)> = Vec::new();
        for (start, efforts) in &self.runs {
            if *start < from || *start > ts {
                continue;
            }
            for &(d, secs) in efforts {
                match best.iter_mut().find(|b| b.0 == d) {
                    Some(b) => b.1 = b.1.min(secs),
                    None => best.push((d, secs)),
                }
            }
        }
        let vo2 = self.vo2.iter().rev().find(|r| r.0 <= ts).map(|r| r.1);
        (vo2, predict(&best, vo2))
    }

    /// The estimate 4 weeks before `ts`, as a list.
    fn earlier(&self, ts: i64) -> Option<Vec<(f64, f64)>> {
        self.at(ts - 28 * DAY).1.map(|(_, list)| list)
    }
}

async fn inputs(ctx: &Ctx) -> Res<Inputs> {
    let acts = load_acts(ctx).await?;
    let efforts = load_efforts(ctx, &acts).await?;
    let runs = acts
        .iter()
        .filter(|a| a.kind == "run")
        .filter_map(|a| {
            let e = efforts.get(&a.id)?;
            let list: Vec<(f64, f64)> = RUN_EFFORTS
                .iter()
                .filter_map(|&(key, d)| e.get(key).map(|s| (d, *s)))
                .collect();
            Some((a.start, list))
        })
        .collect();
    let vo2 = vo2_readings(ctx, ctx.now)
        .await?
        .into_iter()
        .map(|(ts, met)| (ts, vo2_from_met(met)))
        .collect();
    Ok(Inputs { runs, vo2 })
}

/// The whole response for the day of `ctx.now`.
fn build(ctx: &Ctx, inp: &Inputs) -> RacePredictionsResponse {
    let today = ctx.today();
    let earlier = inp.earlier(ctx.now);
    let (vo2, now) = inp.at(ctx.now);
    let (basis, predictions) = match now {
        Some((b, list)) => (Some(b), to_predictions(&list, earlier.as_deref())),
        None => (None, Vec::new()),
    };

    let last_sunday = today - Duration::days(i64::from(today.weekday().num_days_from_sunday()));
    let mut history = Vec::new();
    for w in (0..HISTORY_WEEKS).rev() {
        let day: NaiveDate = last_sunday - Duration::days(7 * w);
        // The end of that Sunday, but never later than now.
        let ts = (ctx.day_bounds(day).1 - 1).min(ctx.now);
        if let Some((_, list)) = inp.at(ts).1 {
            history.push(RaceHistoryPoint {
                date: date_string(day),
                predictions: to_predictions(&list, inp.earlier(ts).as_deref()),
            });
        }
    }
    RacePredictionsResponse {
        sample: ctx.sample(),
        as_of: date_string(today),
        basis,
        vo2max: vo2,
        predictions,
        history,
    }
}

async fn race_predictions(
    user: DataUser,
    State(state): State<AppState>,
) -> Res<Json<RacePredictionsResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let inp = inputs(&ctx).await?;
    Ok(Json(build(&ctx, &inp)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn within(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} is not within {tol} of {b}");
    }

    #[test]
    fn riegel_scales_with_the_exponent() {
        within(
            riegel(1200.0, 5000.0, 10000.0),
            1200.0 * 2f64.powf(1.06),
            1e-9,
        );
        within(riegel(1200.0, 5000.0, 5000.0), 1200.0, 1e-9);
    }

    #[test]
    fn daniels_matches_the_published_table_for_vo2max_50() {
        let t = |d| daniels_seconds(50.0, d).unwrap();
        within(t(5000.0), 19.0 * 60.0 + 57.0, 15.0);
        within(t(10000.0), 41.0 * 60.0 + 21.0, 15.0);
        within(t(21097.5), 3600.0 + 31.0 * 60.0 + 35.0, 15.0);
        within(t(42195.0), 3.0 * 3600.0 + 10.0 * 60.0 + 49.0, 15.0);
    }

    #[test]
    fn daniels_is_faster_for_a_higher_vo2max_and_rejects_nonsense() {
        assert!(daniels_seconds(55.0, 5000.0).unwrap() < daniels_seconds(50.0, 5000.0).unwrap());
        assert!(daniels_seconds(0.0, 5000.0).is_none());
        assert!(daniels_seconds(f64::NAN, 5000.0).is_none());
        assert!(daniels_seconds(50.0, 0.0).is_none());
    }

    #[test]
    fn effort_estimate_takes_the_median_of_the_three_closest_efforts() {
        // For a 10k the three closest efforts are the 10k, the 5k and the mile (the 1k is further).
        let e = [
            (1000.0, 200.0),
            (1609.344, 330.0),
            (5000.0, 1200.0),
            (10000.0, 2500.0),
        ];
        let mut p = [
            riegel(2500.0, 10000.0, 10000.0),
            riegel(1200.0, 5000.0, 10000.0),
            riegel(330.0, 1609.344, 10000.0),
        ];
        p.sort_by(f64::total_cmp);
        within(effort_estimate(&e, 10000.0).unwrap(), p[1], 1e-9);
    }

    #[test]
    fn effort_estimate_ignores_short_and_far_efforts() {
        // 400 m is under 1 km; a marathon is over 2.5 times 5 km; 1 km is fine.
        assert!(effort_estimate(&[(400.0, 80.0)], 5000.0).is_none());
        assert!(effort_estimate(&[(42195.0, 14400.0)], 5000.0).is_none());
        assert!(effort_estimate(&[(1000.0, 200.0)], 5000.0).is_some());
        // An even count: the mean of the middle two.
        let e = [(5000.0, 1200.0), (10000.0, 2500.0)];
        let a = riegel(1200.0, 5000.0, 5000.0);
        let b = riegel(2500.0, 10000.0, 5000.0);
        within(effort_estimate(&e, 5000.0).unwrap(), (a + b) / 2.0, 1e-9);
    }

    #[test]
    fn blend_and_basis() {
        assert_eq!(blend(None, None), None);
        assert_eq!(blend(Some(100.0), None), Some((100.0, RaceBasis::Efforts)));
        assert_eq!(blend(None, Some(200.0)), Some((200.0, RaceBasis::Vo2max)));
        let (v, b) = blend(Some(100.0), Some(200.0)).unwrap();
        within(v, 130.0, 1e-9);
        assert_eq!(b, RaceBasis::Both);
    }

    #[test]
    fn predict_gives_whole_seconds_and_a_basis() {
        assert!(predict(&[], None).is_none());
        let (b, list) = predict(&[], Some(50.0)).unwrap();
        assert_eq!(b, RaceBasis::Vo2max);
        assert_eq!(list.len(), 4);
        assert!(list.iter().all(|p| p.1 == p.1.round()));
        let (b, list) = predict(&[(5000.0, 1200.0)], Some(50.0)).unwrap();
        assert_eq!(b, RaceBasis::Both);
        // A 5 km effort is not too short for the marathon (only 1 km and under 2.5 times count).
        let want = blend(
            effort_estimate(&[(5000.0, 1200.0)], 42195.0),
            daniels_seconds(50.0, 42195.0),
        )
        .unwrap()
        .0
        .round();
        within(list[3].1, want, 0.5);
    }

    #[test]
    fn the_42_day_window_and_the_latest_vo2_before_the_date() {
        let inp = Inputs {
            runs: vec![
                (10 * DAY, vec![(5000.0, 1000.0)]),
                (60 * DAY, vec![(5000.0, 1300.0)]),
            ],
            vo2: vec![(5 * DAY, 45.0), (70 * DAY, 55.0)],
        };
        // Day 65: the day-10 run is 55 days old, so only the day-60 run counts; VO2 max is 45.
        let (vo2, p) = inp.at(65 * DAY);
        assert_eq!(vo2, Some(45.0));
        let (b, list) = p.unwrap();
        assert_eq!(b, RaceBasis::Both);
        let v = daniels_seconds(45.0, 5000.0).unwrap();
        within(list[0].1, (0.7 * 1300.0 + 0.3 * v).round(), 0.5);
        // Day 110: no run in the window, the newer VO2 max alone.
        let (vo2, p) = inp.at(110 * DAY);
        assert_eq!(vo2, Some(55.0));
        assert_eq!(p.unwrap().0, RaceBasis::Vo2max);
        // Day 30: both runs are not yet there / the day-10 run counts with the best time.
        let (_, p) = inp.at(30 * DAY);
        within(
            p.unwrap().1[0].1,
            (0.7 * 1000.0 + 0.3 * daniels_seconds(45.0, 5000.0).unwrap()).round(),
            0.5,
        );
    }
}
