//! `GET /api/training/explain`: why the training status is what it is, in plain words, and the
//! figures behind it: one sentence from Nimrag's own rules over the status the watch reports, the
//! 7-day load against its target range, the VO2 max change over 4 weeks and the HRV status; a
//! paragraph on what the status means; and the "load focus" (which of the three intensity
//! buckets is short of its target).
//!
//! The watch decides the status; Nimrag never changes it, it only explains it. The rules are plain
//! functions ([`sentence`], [`load_state`], [`vo2_trend`], [`focus`]) tested without a database.

use super::daily::hrv_block;
use super::readiness::{hrv_state, HrvState};
use super::training::{
    balance, status_and_load, vo2_from_met, vo2_readings, LoadBalance, StatusInfo, StatusKind,
};
use super::{round1, Ctx};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use axum::{extract::State, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/training/explain", get(explain))
}

// ---- the rules ---------------------------------------------------------------------------------

/// The statuses the sentences know. The first five are the ones the watch writes and Nimrag has
/// verified; the last three have rules ready but no verified watch code yet, so nothing returns
/// them today (see `training::status_info`: no code is named without checking it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Tag {
    Productive,
    Maintaining,
    Recovery,
    Detraining,
    Strained,
    Overreaching,
    Unproductive,
    Peaking,
}

pub fn tag_of(kind: StatusKind) -> Option<Tag> {
    match kind {
        StatusKind::Productive => Some(Tag::Productive),
        StatusKind::Maintaining => Some(Tag::Maintaining),
        StatusKind::Recovery => Some(Tag::Recovery),
        StatusKind::Detraining => Some(Tag::Detraining),
        StatusKind::Strained => Some(Tag::Strained),
        StatusKind::None | StatusKind::Unknown => None,
    }
}

/// The 7-day load against its target range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    Low,
    Optimal,
    High,
    VeryHigh,
}

impl LoadState {
    pub fn word(self) -> &'static str {
        match self {
            LoadState::Low => "Low",
            LoadState::Optimal => "Optimal",
            LoadState::High => "High",
            LoadState::VeryHigh => "Very high",
        }
    }
}

/// Below the range is low, inside it optimal, above it high, and past 1.5 times the top very high.
pub fn load_state(load: f64, min: f64, max: f64) -> LoadState {
    if load < min {
        LoadState::Low
    } else if load <= max {
        LoadState::Optimal
    } else if load <= max * 1.5 {
        LoadState::High
    } else {
        LoadState::VeryHigh
    }
}

/// Where VO2 max went over 4 weeks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trend {
    Up,
    Flat,
    Down,
}

impl Trend {
    pub fn word(self) -> &'static str {
        match self {
            Trend::Up => "Rising",
            Trend::Flat => "Steady",
            Trend::Down => "Falling",
        }
    }
}

/// A change of 0.3 ml/kg/min or more counts as moving; anything smaller is steady.
pub const TREND_STEP: f64 = 0.3;

pub fn vo2_trend(change: f64) -> Trend {
    if change >= TREND_STEP {
        Trend::Up
    } else if change <= -TREND_STEP {
        Trend::Down
    } else {
        Trend::Flat
    }
}

/// The change in VO2 max (ml/kg/min) over the 4 weeks up to `now`, from `(ts, ml/kg/min)` readings,
/// oldest first: the latest reading against the last one before the window, else the first one
/// inside it. `None` when the latest reading is older than the window or there is nothing to
/// compare it with.
pub fn vo2_change_4w(readings: &[(i64, f64)], now: i64) -> Option<f64> {
    let start = now - 28 * 86_400;
    let (last_ts, last) = *readings.last()?;
    if last_ts < start {
        return None;
    }
    let base = readings
        .iter()
        .rev()
        .find(|r| r.0 < start)
        .or_else(|| readings.iter().find(|r| r.0 >= start && r.0 != last_ts))?;
    Some(round1(last - base.1))
}

/// What the sentence is made from.
#[derive(Debug, Clone, Copy, Default)]
pub struct Facts {
    pub tag: Option<Tag>,
    /// The watch gave a status Nimrag has no name for.
    pub unnamed: bool,
    pub load: Option<LoadState>,
    pub fitness: Option<Trend>,
    pub hrv: Option<HrvState>,
}

/// One sentence (and its key) naming why the status is what it is; `None` without a status. When
/// the HRV is unbalanced a caveat is added unless the sentence is already about HRV.
pub fn sentence(f: &Facts) -> Option<(&'static str, String)> {
    use LoadState::{High, Low, Optimal, VeryHigh};
    let hot = matches!(f.load, Some(High | VeryHigh));
    let (key, text): (&'static str, &str) = match f.tag {
        None if f.unnamed => (
            "unnamed",
            "The watch reported a status that Nimrag has no name for yet. The figures below still apply.",
        ),
        None => return None,
        Some(Tag::Productive) => match (f.load, f.fitness) {
            (Some(Optimal | High), Some(Trend::Up)) => (
                "productive_working",
                "Your load is in the right range and your VO2 max is rising, so this training is working.",
            ),
            (Some(Low), _) => (
                "productive_light",
                "The watch sees progress even though your 7-day load is under its target range. Keep going as you are.",
            ),
            _ => (
                "productive",
                "The watch sees your fitness improving at your current load.",
            ),
        },
        Some(Tag::Maintaining) => match (f.load, f.fitness) {
            (_, Some(Trend::Up)) => (
                "maintaining_rising",
                "Your fitness is edging up, at a load that mostly holds it.",
            ),
            (Some(Optimal | High), _) => (
                "maintaining_holding",
                "Your load is enough to hold your fitness but not to build it. Add a harder session if you want progress.",
            ),
            _ => (
                "maintaining",
                "You are training enough to hold your current fitness.",
            ),
        },
        Some(Tag::Recovery) => match f.load {
            Some(Low) => (
                "recovery_light",
                "Your load is light and your fitness is steady, a good time to recover.",
            ),
            _ => (
                "recovery",
                "The watch says you are recovering. Keep sessions easy until your recovery time is used up.",
            ),
        },
        Some(Tag::Detraining) => match f.load {
            Some(Low) => (
                "detraining_low",
                "You have trained much less than usual and your 7-day load is under its target range. Fitness slips if this goes on.",
            ),
            _ => (
                "detraining",
                "You have trained much less than usual lately, and fitness slips if this goes on.",
            ),
        },
        Some(Tag::Strained) => match (f.hrv, hot) {
            (Some(HrvState::Unbalanced | HrvState::Low | HrvState::Poor), _) => (
                "strained_hrv",
                "Your HRV shows fatigue. Keep training easy until it is back in your balanced range.",
            ),
            (_, true) => (
                "strained_load",
                "Your load is high and the watch sees strain. Ease off for a few days.",
            ),
            _ => (
                "strained",
                "The watch sees strain: your body is carrying more than it recovers from. Take it easier.",
            ),
        },
        Some(Tag::Overreaching) => (
            "overreaching",
            "Your load is too high for your recovery. Ease off before it turns into strain.",
        ),
        Some(Tag::Unproductive) => (
            "unproductive",
            "Your load is fine but your fitness is falling. Look at rest, sleep and stress.",
        ),
        Some(Tag::Peaking) => (
            "peaking",
            "Your load eased after hard training and your fitness is high. You are peaking.",
        ),
    };
    let caveat = matches!(f.hrv, Some(HrvState::Unbalanced))
        && f.tag.is_some()
        && !matches!(f.tag, Some(Tag::Strained));
    let mut out = text.to_owned();
    if caveat {
        out.push_str(" Your HRV is unbalanced, so keep an eye on your recovery.");
    }
    Some((key, out))
}

/// What a status means, in a paragraph (shown under "What this status means").
pub fn meaning(tag: Tag) -> (&'static str, &'static str) {
    match tag {
        Tag::Productive => (
            "Productive",
            "Your recent training is the right size and your fitness is going up. This is the state to aim for. Keep the mix of hard and easy days, and make sure you sleep and recover well so the gains stick.",
        ),
        Tag::Maintaining => (
            "Maintaining",
            "You train enough to keep the fitness you have, but not enough to build more. That is fine for a busy stretch or a season of upkeep. If you want to improve, add a little more load or one harder session a week.",
        ),
        Tag::Recovery => (
            "Recovery",
            "Your recent load is light and your body is catching up. Easy days now help the next hard block land better. When your recovery time reaches zero you can pick the pace up again.",
        ),
        Tag::Detraining => (
            "Detraining",
            "You have trained a lot less than your body is used to for some time. Fitness starts to slip, slowly at first. A few regular sessions in the coming week are enough to turn it around.",
        ),
        Tag::Strained => (
            "Strained",
            "Your body is under more stress than it is recovering from, which often shows in HRV, sleep and mood. Keep training light for a few days and look at sleep, food and stress before adding load again.",
        ),
        Tag::Overreaching => (
            "Overreaching",
            "Your load has been well above what you recover from. A short rest brings you back; carrying on risks a longer dip. Take easy days until your HRV and sleep settle.",
        ),
        Tag::Unproductive => (
            "Unproductive",
            "Your training load is fine but your fitness is not rising, even falling. The cause is often outside training: too little sleep, stress or illness. Rest and recovery usually fix it.",
        ),
        Tag::Peaking => (
            "Peaking",
            "You eased off after a hard block and your fitness is at its highest. This is a good window for a race or a hard test. It does not last forever, so use it.",
        ),
    }
}

/// How one intensity bucket stands against its target range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum FocusState {
    Short,
    InRange,
    Above,
    Unknown,
}

pub fn focus_state(value: Option<f64>, min: Option<f64>, max: Option<f64>) -> FocusState {
    match (value, min, max) {
        (Some(v), Some(lo), _) if v < lo => FocusState::Short,
        (Some(v), _, Some(hi)) if v > hi => FocusState::Above,
        (Some(_), Some(_), Some(_)) => FocusState::InRange,
        _ => FocusState::Unknown,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct FocusRow {
    /// `aerobic_low`, `aerobic_high` or `anaerobic`.
    pub key: String,
    pub label: String,
    pub value: Option<f64>,
    pub target_min: Option<f64>,
    pub target_max: Option<f64>,
    pub state: FocusState,
}

/// The three buckets and the key of the one furthest short of its target (as a share of the
/// bottom of its range), if any is short.
pub fn focus(b: &LoadBalance) -> (Vec<FocusRow>, Option<String>) {
    let rows: Vec<FocusRow> = [
        (
            "aerobic_low",
            "Easy aerobic",
            b.aerobic_low,
            b.aerobic_low_target_min,
            b.aerobic_low_target_max,
        ),
        (
            "aerobic_high",
            "Hard aerobic",
            b.aerobic_high,
            b.aerobic_high_target_min,
            b.aerobic_high_target_max,
        ),
        (
            "anaerobic",
            "Anaerobic",
            b.anaerobic,
            b.anaerobic_target_min,
            b.anaerobic_target_max,
        ),
    ]
    .into_iter()
    .map(|(key, label, value, lo, hi)| FocusRow {
        key: key.to_owned(),
        label: label.to_owned(),
        value,
        target_min: lo,
        target_max: hi,
        state: focus_state(value, lo, hi),
    })
    .collect();
    let mut worst: Option<(f64, &str)> = None;
    for r in &rows {
        if let (FocusState::Short, Some(v), Some(lo)) = (r.state, r.value, r.target_min) {
            let gap = if lo > 0.0 { (lo - v) / lo } else { 0.0 };
            if worst.is_none_or(|(g, _)| gap > g) {
                worst = Some((gap, &r.key));
            }
        }
    }
    let short = worst.map(|w| w.1.to_owned());
    (rows, short)
}

// ---- GET /api/training/explain -----------------------------------------------------------------

/// One figure behind the status.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ExplainFactor {
    /// `vo2max`, `load` or `hrv`.
    pub key: String,
    pub label: String,
    pub value: Option<f64>,
    pub unit: String,
    /// VO2 max: the change over 4 weeks.
    pub change: Option<f64>,
    /// "Rising", "Optimal", "Balanced" and so on.
    pub word: Option<String>,
    /// A short extra line ("Target range 240–420").
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct StatusMeaning {
    pub title: String,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct StatusExplain {
    pub sample: bool,
    /// The status the watch reports now (`null` without one).
    pub status: Option<StatusInfo>,
    pub sentence_key: Option<String>,
    pub sentence: Option<String>,
    pub meaning: Option<StatusMeaning>,
    pub factors: Vec<ExplainFactor>,
    /// Load by intensity against the targets; empty without a reading.
    pub focus: Vec<FocusRow>,
    /// Key of the bucket furthest short of its target.
    pub short_of: Option<String>,
}

async fn explain(user: DataUser, State(state): State<AppState>) -> Res<Json<StatusExplain>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let (status, load) = status_and_load(&ctx).await?;
    let balance = balance(&ctx).await?;
    let hrv = hrv_block(&ctx, ctx.today()).await?;

    let readings: Vec<(i64, f64)> = vo2_readings(&ctx, ctx.now)
        .await?
        .into_iter()
        .map(|(ts, met)| (ts, vo2_from_met(met)))
        .collect();
    let vo2 = readings.last().map(|r| r.1);
    let change = vo2_change_4w(&readings, ctx.now);
    let fitness = change.map(vo2_trend);

    let load_now = match (load.seven_day_load, load.target_min, load.target_max) {
        (Some(v), Some(lo), Some(hi)) => Some(load_state(v, lo, hi)),
        _ => None,
    };
    let band = hrv.baseline_low_ms.zip(hrv.baseline_high_ms);
    let hrv_now = hrv_state(hrv.status_code.map(i64::from), hrv.weekly_avg_ms, band);

    let info = status
        .status_code
        .map(|c| super::training::status_info(i64::from(c)));
    let tag = info.as_ref().and_then(|i| tag_of(i.kind));
    let facts = Facts {
        tag,
        unnamed: info.as_ref().is_some_and(|i| i.kind == StatusKind::Unknown),
        load: load_now,
        fitness,
        hrv: hrv_now,
    };
    let said = sentence(&facts);
    let (focus_rows, short_of) = balance.as_ref().map_or((vec![], None), focus);

    let factors = vec![
        ExplainFactor {
            key: "vo2max".to_owned(),
            label: "VO2 max".to_owned(),
            value: vo2,
            unit: "ml/kg/min".to_owned(),
            change,
            word: fitness.map(|t| t.word().to_owned()),
            detail: None,
        },
        ExplainFactor {
            key: "load".to_owned(),
            label: "7-day load".to_owned(),
            value: load.seven_day_load,
            unit: "load".to_owned(),
            change: None,
            word: load_now.map(|s| s.word().to_owned()),
            detail: load
                .target_min
                .zip(load.target_max)
                .map(|(lo, hi)| format!("Target range {}–{}", lo.round(), hi.round())),
        },
        ExplainFactor {
            key: "hrv".to_owned(),
            label: "HRV, 7-day average".to_owned(),
            value: hrv.weekly_avg_ms,
            unit: "ms".to_owned(),
            change: None,
            word: hrv.status.as_deref().map(capitalised),
            detail: band.map(|(lo, hi)| format!("Balanced range {}–{} ms", lo.round(), hi.round())),
        },
    ];
    Ok(Json(StatusExplain {
        sample: ctx.sample(),
        status: info,
        sentence_key: said.as_ref().map(|s| s.0.to_owned()),
        sentence: said.map(|s| s.1),
        meaning: tag.map(|t| {
            let (title, text) = meaning(t);
            StatusMeaning {
                title: title.to_owned(),
                text: text.to_owned(),
            }
        }),
        factors,
        focus: focus_rows,
        short_of,
    }))
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(tag: Tag) -> Facts {
        Facts {
            tag: Some(tag),
            ..Facts::default()
        }
    }

    #[test]
    fn load_states_against_the_range() {
        assert_eq!(load_state(239.0, 240.0, 420.0), LoadState::Low);
        assert_eq!(load_state(240.0, 240.0, 420.0), LoadState::Optimal);
        assert_eq!(load_state(420.0, 240.0, 420.0), LoadState::Optimal);
        assert_eq!(load_state(421.0, 240.0, 420.0), LoadState::High);
        assert_eq!(load_state(630.0, 240.0, 420.0), LoadState::High);
        assert_eq!(load_state(631.0, 240.0, 420.0), LoadState::VeryHigh);
    }

    #[test]
    fn vo2_trend_thresholds() {
        assert_eq!(vo2_trend(0.3), Trend::Up);
        assert_eq!(vo2_trend(0.2), Trend::Flat);
        assert_eq!(vo2_trend(0.0), Trend::Flat);
        assert_eq!(vo2_trend(-0.2), Trend::Flat);
        assert_eq!(vo2_trend(-0.3), Trend::Down);
    }

    #[test]
    fn vo2_change_compares_with_the_reading_before_the_window() {
        let now = 100 * 86_400;
        let day = |n: i64| now - n * 86_400;
        // Before the window (40 days ago) 50.0, inside it 51.0 and 51.4: change against 50.0.
        let r = [(day(40), 50.0), (day(20), 51.0), (day(2), 51.4)];
        assert_eq!(vo2_change_4w(&r, now), Some(1.4));
        // Nothing before the window: the first reading inside it.
        assert_eq!(vo2_change_4w(&r[1..], now), Some(0.4));
        // One reading only, inside the window: nothing to compare.
        assert_eq!(vo2_change_4w(&r[2..], now), None);
        // The latest reading is older than the window: no change observed.
        assert_eq!(vo2_change_4w(&r[..1], now), None);
        assert_eq!(vo2_change_4w(&[], now), None);
    }

    #[test]
    fn productive_sentences() {
        let mut f = facts(Tag::Productive);
        f.load = Some(LoadState::Optimal);
        f.fitness = Some(Trend::Up);
        assert_eq!(sentence(&f).unwrap().0, "productive_working");
        f.load = Some(LoadState::Low);
        assert_eq!(sentence(&f).unwrap().0, "productive_light");
        assert_eq!(sentence(&facts(Tag::Productive)).unwrap().0, "productive");
    }

    #[test]
    fn maintaining_sentences() {
        let mut f = facts(Tag::Maintaining);
        f.load = Some(LoadState::Optimal);
        f.fitness = Some(Trend::Flat);
        assert_eq!(sentence(&f).unwrap().0, "maintaining_holding");
        f.fitness = Some(Trend::Up);
        assert_eq!(sentence(&f).unwrap().0, "maintaining_rising");
        assert_eq!(sentence(&facts(Tag::Maintaining)).unwrap().0, "maintaining");
    }

    #[test]
    fn recovery_and_detraining_sentences() {
        let mut f = facts(Tag::Recovery);
        assert_eq!(sentence(&f).unwrap().0, "recovery");
        f.load = Some(LoadState::Low);
        assert_eq!(sentence(&f).unwrap().0, "recovery_light");
        let mut d = facts(Tag::Detraining);
        assert_eq!(sentence(&d).unwrap().0, "detraining");
        d.load = Some(LoadState::Low);
        assert_eq!(sentence(&d).unwrap().0, "detraining_low");
    }

    #[test]
    fn strained_names_hrv_first_then_load() {
        let mut f = facts(Tag::Strained);
        f.load = Some(LoadState::High);
        assert_eq!(sentence(&f).unwrap().0, "strained_load");
        f.hrv = Some(HrvState::Low);
        assert_eq!(sentence(&f).unwrap().0, "strained_hrv");
        f.hrv = Some(HrvState::Balanced);
        f.load = Some(LoadState::Optimal);
        assert_eq!(sentence(&f).unwrap().0, "strained");
    }

    #[test]
    fn the_other_statuses_have_a_sentence_each() {
        for (tag, key) in [
            (Tag::Overreaching, "overreaching"),
            (Tag::Unproductive, "unproductive"),
            (Tag::Peaking, "peaking"),
        ] {
            assert_eq!(sentence(&facts(tag)).unwrap().0, key);
        }
    }

    #[test]
    fn unbalanced_hrv_adds_a_caveat_except_for_strained() {
        let mut f = facts(Tag::Productive);
        f.hrv = Some(HrvState::Unbalanced);
        let s = sentence(&f).unwrap().1;
        assert!(
            s.ends_with("Your HRV is unbalanced, so keep an eye on your recovery."),
            "{s}"
        );
        f.hrv = Some(HrvState::Balanced);
        assert!(!sentence(&f).unwrap().1.contains("HRV"));
        let mut st = facts(Tag::Strained);
        st.hrv = Some(HrvState::Unbalanced);
        let s = sentence(&st).unwrap().1;
        assert_eq!(s.matches("HRV").count(), 1, "{s}");
    }

    #[test]
    fn no_status_no_sentence_and_an_unnamed_one_says_so() {
        assert!(sentence(&Facts::default()).is_none());
        let f = Facts {
            unnamed: true,
            ..Facts::default()
        };
        assert_eq!(sentence(&f).unwrap().0, "unnamed");
        assert_eq!(tag_of(StatusKind::None), None);
        assert_eq!(tag_of(StatusKind::Unknown), None);
        assert_eq!(tag_of(StatusKind::Strained), Some(Tag::Strained));
    }

    #[test]
    fn every_status_has_a_meaning() {
        for t in [
            Tag::Productive,
            Tag::Maintaining,
            Tag::Recovery,
            Tag::Detraining,
            Tag::Strained,
            Tag::Overreaching,
            Tag::Unproductive,
            Tag::Peaking,
        ] {
            let (title, text) = meaning(t);
            assert!(!title.is_empty() && text.len() > 60);
        }
    }

    fn balance_of(v: [Option<f64>; 3], t: [(f64, f64); 3]) -> LoadBalance {
        LoadBalance {
            aerobic_low: v[0],
            aerobic_high: v[1],
            anaerobic: v[2],
            aerobic_low_target_min: Some(t[0].0),
            aerobic_low_target_max: Some(t[0].1),
            aerobic_high_target_min: Some(t[1].0),
            aerobic_high_target_max: Some(t[1].1),
            anaerobic_target_min: Some(t[2].0),
            anaerobic_target_max: Some(t[2].1),
        }
    }

    #[test]
    fn focus_names_the_bucket_furthest_short() {
        let t = [(100.0, 200.0), (100.0, 200.0), (50.0, 100.0)];
        // Easy 90 (10 % short), hard 40 (60 % short), anaerobic 10 (80 % short).
        let (rows, short) = focus(&balance_of([Some(90.0), Some(40.0), Some(10.0)], t));
        assert_eq!(
            rows.iter().map(|r| r.state).collect::<Vec<_>>(),
            vec![FocusState::Short; 3]
        );
        assert_eq!(short.as_deref(), Some("anaerobic"));
        // Nothing short: in range, above.
        let (rows, short) = focus(&balance_of([Some(150.0), Some(250.0), Some(60.0)], t));
        assert_eq!(
            rows.iter().map(|r| r.state).collect::<Vec<_>>(),
            vec![FocusState::InRange, FocusState::Above, FocusState::InRange]
        );
        assert_eq!(short, None);
        // No value: unknown, never short.
        let (rows, short) = focus(&balance_of([None, Some(150.0), None], t));
        assert_eq!(rows[0].state, FocusState::Unknown);
        assert_eq!(short, None);
    }
}
