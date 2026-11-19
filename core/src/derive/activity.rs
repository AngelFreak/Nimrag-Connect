//! Activity parser: activity, session, lap, record, strength set, exercise
//! title, time-in-zone and firstbeat result messages.
//!
//! Message-driven (dispatch on the global message number only; upload metadata
//! and `file_id.type` are never consulted). Fields are read by profile name via
//! `Interpreted::number`/`get` (Direct/Subfield before Component), so scaling
//! and `enhanced_*` components come from the interpret layer.
//!
//! Rules: an absent or invalid field is `None`; a row with no usable value is
//! not emitted (its ordinal `idx` is still consumed, so `idx` = ordinal among
//! messages of that type in the file); nothing is interpolated. A required
//! timestamp that is absent or device-relative skips the row and counts it in
//! `Derived::skipped`. Speed prefers `enhanced_*`. Lat/long are semicircles
//! converted to degrees (unit conversion only). `recovery_time_physical_activity_raw` is
//! `firstbeat_results.recovery_time_physical_activity` as written (no unit
//! claimed). Metrics are kept only when the file has activity rows;
//! standalone firstbeat files (128/74) belong to the wellness parser.
use super::{Derived, FIT_EPOCH_UNIX};
use crate::fit::interpret::{Interpreted, Source, Val, FIT_UTC_MIN};
use crate::fit::profile;

const G_ACTIVITY: u16 = 34;
const G_SESSION: u16 = 18;
const G_LAP: u16 = 19;
const G_RECORD: u16 = 20;
const G_SET: u16 = 225;
const G_EXERCISE_TITLE: u16 = 264;
const G_TIME_IN_ZONE: u16 = 216;
const G_FIRSTBEAT: u16 = 140;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActivityRows {
    pub activity: Option<ActivityRow>,
    pub sessions: Vec<SessionRow>,
    pub laps: Vec<LapRow>,
    pub records: Vec<RecordRow>,
    pub sets: Vec<StrengthSet>,
    pub exercise_titles: Vec<ExerciseTitle>,
    pub time_in_zone: Vec<TimeInZone>,
    pub metrics: Option<ActivityMetrics>,
}

/// An enum-typed value: raw code plus profile name when known.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Coded {
    pub code: Option<i64>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActivityRow {
    /// "<serial>:<time_created>" when both come from file_id.
    pub activity_key: Option<String>,
    pub serial: Option<i64>,
    pub time_created: Option<i64>,
    /// activity.timestamp.
    pub ts: Option<i64>,
    /// local_timestamp - timestamp.
    pub local_offset_s: Option<i64>,
    pub total_timer_s: Option<f64>,
    pub num_sessions: Option<i64>,
    /// From the first session.
    pub start_ts: Option<i64>,
    pub sport: Coded,
    pub sub_sport: Coded,
}

/// Columns shared by sessions and laps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    pub idx: u32,
    pub start_ts: Option<i64>,
    pub sport: Coded,
    pub sub_sport: Coded,
    pub elapsed_s: Option<f64>,
    pub timer_s: Option<f64>,
    pub distance_m: Option<f64>,
    pub calories: Option<f64>,
    pub avg_hr: Option<f64>,
    pub max_hr: Option<f64>,
    pub avg_speed: Option<f64>,
    pub max_speed: Option<f64>,
    pub ascent_m: Option<f64>,
    pub descent_m: Option<f64>,
    pub avg_cadence: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionRow {
    pub s: Summary,
    pub aerobic_te: Option<f64>,
    pub anaerobic_te: Option<f64>,
    pub training_load: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LapRow {
    pub s: Summary,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordRow {
    pub idx: u32,
    pub ts: i64,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub hr: Option<f64>,
    pub speed: Option<f64>,
    pub altitude_m: Option<f64>,
    pub distance_m: Option<f64>,
    pub cadence: Option<f64>,
    pub temperature_c: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StrengthSet {
    pub idx: u32,
    pub ts: Option<i64>,
    pub start_ts: Option<i64>,
    pub duration_s: Option<f64>,
    pub set_type: Coded,
    pub reps: Option<f64>,
    /// Invalid weight is None, never 0.
    pub weight_kg: Option<f64>,
    /// First valid element of `category`.
    pub category: Coded,
    pub category_subtype: Option<i64>,
    /// All valid category / subtype elements when there is more than one.
    pub category_all: Vec<i64>,
    pub category_subtype_all: Vec<i64>,
    pub wkt_step_index: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExerciseTitle {
    pub idx: u32,
    pub category: Coded,
    pub subtype: Option<i64>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimeInZone {
    pub idx: u32,
    pub ref_mesg: Option<i64>,
    pub ref_index: Option<i64>,
    /// Seconds per HR zone; invalid elements are None.
    pub hr_zone_s: Vec<Option<f64>>,
}

/// firstbeat_results (140): only fields the local profile resolves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActivityMetrics {
    pub maximal_met: Option<f64>,
    pub aerobic_te: Option<f64>,
    pub anaerobic_te: Option<f64>,
    pub training_load_peak: Option<f64>,
    pub epoc: Option<f64>,
    pub recovery_time_physical_activity_raw: Option<f64>,
}

fn int(x: Option<f64>) -> Option<i64> {
    x.map(|v| v.round() as i64)
}

/// UTC unix seconds of a date_time field; device-relative or absent values give
/// None. Only rows actually dropped are counted in `skipped` (by the callers).
fn time(m: &Interpreted, name: &str) -> Option<i64> {
    let v = m.number(name)?;
    if v < f64::from(FIT_UTC_MIN) {
        return None;
    }
    Some(v as i64 + FIT_EPOCH_UNIX)
}

fn coded(m: &Interpreted, name: &str) -> Coded {
    Coded {
        code: int(m.number(name)),
        name: m.get(name).and_then(|v| v.enum_label).map(str::to_string),
    }
}

/// First of the two names that has a value (enhanced_* first).
fn either(m: &Interpreted, a: &str, b: &str) -> Option<f64> {
    m.number(a).or_else(|| m.number(b))
}

/// All valid elements of a scalar or array field, as integers.
fn valid_ints(m: &Interpreted, name: &str) -> Vec<i64> {
    ints_of(m.get(name).and_then(|v| v.value.as_ref()))
}

fn ints_of(v: Option<&Val>) -> Vec<i64> {
    match v {
        Some(Val::Number(x)) => vec![x.round() as i64],
        Some(Val::Numbers(v)) => v.iter().flatten().map(|x| x.round() as i64).collect(),
        _ => Vec::new(),
    }
}

fn text(m: &Interpreted, name: &str) -> Option<String> {
    match m.get(name)?.value.as_ref()? {
        Val::Text(t) => t.first().cloned(),
        _ => None,
    }
}

fn degrees(semicircles: Option<f64>) -> Option<f64> {
    semicircles.map(|s| s * 180.0 / 2_147_483_648.0)
}

fn summary(m: &Interpreted, idx: u32) -> Summary {
    Summary {
        idx,
        start_ts: time(m, "start_time"),
        sport: coded(m, "sport"),
        sub_sport: coded(m, "sub_sport"),
        elapsed_s: m.number("total_elapsed_time"),
        timer_s: m.number("total_timer_time"),
        distance_m: m.number("total_distance"),
        calories: m.number("total_calories"),
        avg_hr: m.number("avg_heart_rate"),
        max_hr: m.number("max_heart_rate"),
        avg_speed: either(m, "enhanced_avg_speed", "avg_speed"),
        max_speed: either(m, "enhanced_max_speed", "max_speed"),
        ascent_m: m.number("total_ascent"),
        descent_m: m.number("total_descent"),
        avg_cadence: m.number("avg_cadence"),
    }
}

impl Summary {
    fn has_value(&self) -> bool {
        self.start_ts.is_some()
            || self.sport.code.is_some()
            || self.sub_sport.code.is_some()
            || [
                self.elapsed_s,
                self.timer_s,
                self.distance_m,
                self.calories,
                self.avg_hr,
                self.max_hr,
                self.avg_speed,
                self.max_speed,
                self.ascent_m,
                self.descent_m,
                self.avg_cadence,
            ]
            .iter()
            .any(Option::is_some)
    }
}

pub fn run(msgs: &[Interpreted], out: &mut Derived) {
    let mut skipped = 0u64;
    let mut rows = ActivityRows::default();
    let mut counters = [0u32; 8];
    let mut next = |slot: usize| {
        let i = counters[slot];
        counters[slot] += 1;
        i
    };
    let mut activity_msg: Option<&Interpreted> = None;

    for m in msgs {
        match m.global {
            G_ACTIVITY => {
                activity_msg.get_or_insert(m);
            }
            G_SESSION => {
                let idx = next(0);
                let row = SessionRow {
                    s: summary(m, idx),
                    aerobic_te: m.number("total_training_effect"),
                    anaerobic_te: m.number("total_anaerobic_training_effect"),
                    training_load: m.number("training_load_peak"),
                };
                if row.s.has_value()
                    || row.aerobic_te.is_some()
                    || row.anaerobic_te.is_some()
                    || row.training_load.is_some()
                {
                    rows.sessions.push(row);
                }
            }
            G_LAP => {
                let idx = next(1);
                let s = summary(m, idx);
                if s.has_value() {
                    rows.laps.push(LapRow { s });
                }
            }
            G_RECORD => {
                let idx = next(2);
                let row = RecordRow {
                    idx,
                    ts: 0,
                    lat: degrees(m.number("position_lat")),
                    lon: degrees(m.number("position_long")),
                    hr: m.number("heart_rate"),
                    speed: either(m, "enhanced_speed", "speed"),
                    altitude_m: either(m, "enhanced_altitude", "altitude"),
                    distance_m: m.number("distance"),
                    cadence: m.number("cadence"),
                    temperature_c: m.number("temperature"),
                };
                let any = [
                    row.lat,
                    row.lon,
                    row.hr,
                    row.speed,
                    row.altitude_m,
                    row.distance_m,
                    row.cadence,
                    row.temperature_c,
                ]
                .iter()
                .any(Option::is_some);
                if !any {
                    continue;
                }
                match m.timestamp {
                    Some(t) => rows.records.push(RecordRow {
                        ts: i64::from(t) + FIT_EPOCH_UNIX,
                        ..row
                    }),
                    None => skipped += 1,
                }
            }
            G_SET => {
                let idx = next(3);
                let categories = valid_ints(m, "category");
                let subtypes = valid_ints(m, "category_subtype");
                let category = Coded {
                    code: categories.first().copied(),
                    name: categories
                        .first()
                        .and_then(|&c| u64::try_from(c).ok())
                        .and_then(|c| profile::enum_name("exercise_category", c))
                        .map(str::to_string),
                };
                let row = StrengthSet {
                    idx,
                    ts: time(m, "timestamp"),
                    start_ts: time(m, "start_time"),
                    duration_s: m.number("duration"),
                    set_type: coded(m, "set_type"),
                    reps: m.number("repetitions"),
                    weight_kg: m.number("weight"),
                    category,
                    category_subtype: subtypes.first().copied(),
                    category_all: if categories.len() > 1 {
                        categories
                    } else {
                        vec![]
                    },
                    category_subtype_all: if subtypes.len() > 1 { subtypes } else { vec![] },
                    wkt_step_index: int(m.number("wkt_step_index")),
                };
                if row.ts.is_some()
                    || row.start_ts.is_some()
                    || row.duration_s.is_some()
                    || row.set_type.code.is_some()
                    || row.reps.is_some()
                    || row.weight_kg.is_some()
                    || row.category.code.is_some()
                    || row.category_subtype.is_some()
                    || row.wkt_step_index.is_some()
                {
                    rows.sets.push(row);
                }
            }
            G_EXERCISE_TITLE => {
                let idx = next(4);
                let cat = valid_ints(m, "exercise_category");
                let row = ExerciseTitle {
                    idx,
                    category: Coded {
                        code: cat.first().copied(),
                        name: cat
                            .first()
                            .and_then(|&c| u64::try_from(c).ok())
                            .and_then(|c| profile::enum_name("exercise_category", c))
                            .map(str::to_string),
                    },
                    // exercise_name may be a category-specific subfield with another name.
                    subtype: ints_of(
                        m.values
                            .iter()
                            .find(|v| v.num == 1 && v.source != Source::Component)
                            .and_then(|v| v.value.as_ref()),
                    )
                    .first()
                    .copied(),
                    name: text(m, "wkt_step_name"),
                };
                if row.category.code.is_some() || row.subtype.is_some() || row.name.is_some() {
                    rows.exercise_titles.push(row);
                }
            }
            G_TIME_IN_ZONE => {
                let idx = next(5);
                let zones: Vec<Option<f64>> =
                    match m.get("time_in_hr_zone").and_then(|v| v.value.as_ref()) {
                        Some(Val::Number(x)) => vec![Some(*x)],
                        Some(Val::Numbers(v)) => v.clone(),
                        _ => Vec::new(),
                    };
                if zones.iter().any(Option::is_some) {
                    rows.time_in_zone.push(TimeInZone {
                        idx,
                        ref_mesg: int(m.number("reference_mesg")),
                        ref_index: int(m.number("reference_index")),
                        hr_zone_s: zones,
                    });
                }
            }
            G_FIRSTBEAT if rows.metrics.is_none() => {
                let met = ActivityMetrics {
                    maximal_met: m.number("maximal_met"),
                    aerobic_te: m.number("training_effect"),
                    anaerobic_te: m.number("anaerobic_training_effect"),
                    training_load_peak: m.number("training_load_peak"),
                    epoc: m.number("epoc"),
                    recovery_time_physical_activity_raw: m
                        .number("recovery_time_physical_activity"),
                };
                if met != ActivityMetrics::default() {
                    rows.metrics = Some(met);
                }
            }
            _ => {}
        }
    }

    // Garmin writes the activity message last, so a truncated or in-progress file
    // has none: emit the row whenever any session/lap/record/set exists.
    let has_rows = !(rows.sessions.is_empty()
        && rows.laps.is_empty()
        && rows.records.is_empty()
        && rows.sets.is_empty());
    if activity_msg.is_some() || has_rows {
        let ts = activity_msg.and_then(|a| time(a, "timestamp"));
        let local = activity_msg.and_then(|a| a.number("local_timestamp"));
        let utc = activity_msg.and_then(|a| a.number("timestamp"));
        let first = rows.sessions.first();
        let facts = &out.facts;
        rows.activity = Some(ActivityRow {
            activity_key: match (facts.serial, facts.time_created) {
                (Some(s), Some(t)) => Some(format!("{s}:{t}")),
                _ => None,
            },
            serial: facts.serial,
            time_created: facts.time_created,
            ts,
            local_offset_s: match (local, utc) {
                (Some(l), Some(u)) if ts.is_some() => Some((l - u).round() as i64),
                _ => None,
            },
            total_timer_s: activity_msg.and_then(|a| a.number("total_timer_time")),
            num_sessions: activity_msg.and_then(|a| int(a.number("num_sessions"))),
            start_ts: first.and_then(|s| s.s.start_ts),
            sport: first.map(|s| s.s.sport.clone()).unwrap_or_default(),
            sub_sport: first.map(|s| s.s.sub_sport.clone()).unwrap_or_default(),
        });
    } else {
        rows.metrics = None;
    }
    out.skipped += skipped;
    out.activity = rows;
}
