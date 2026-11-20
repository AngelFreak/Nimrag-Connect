//! Garmin Connect JSON to rows. One function per [`Kind`]; each reads the shapes the Garmin
//! Connect connector returns (see `server/README.md`, "Garmin Connect history") and pushes the
//! same row structs a FIT file derives to.
//!
//! Rules (the same as the FIT derivation): an absent value is `None`, never 0; a row with no
//! usable value is not emitted; nothing is estimated.
//!
//! Time: Connect mixes UTC and local. Sleep start and end are epoch milliseconds (UTC); the
//! decoded FIT messages carry UTC ISO strings; an activity summary has both `start_time_gmt` and
//! `start_time_local`; the activity list has local `start_time`; everything else is per local
//! date. Local times and dates are resolved with the zone in the envelope; a date-only row sits
//! at the start of its local day.

use super::{DailyTotal, Kind, Parsed, SleepStages};
use crate::settings::day_start;
use chrono::{NaiveDate, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use nimrag_core::derive::activity::{
    ActivityMetrics, ActivityRow, Coded, ExerciseTitle, LapRow, SessionRow, StrengthSet, Summary,
};
use nimrag_core::derive::{
    HrvSummary, RestingHr, SleepSession, TrainingBalance, TrainingLoad, TrainingStatus, Vo2max,
};
use serde_json::Value;
use std::collections::HashMap;

type R = Result<(), String>;

pub fn fill(kind: Kind, data: &Value, tz: Tz, p: &mut Parsed) -> R {
    match kind {
        Kind::Sleep => sleep(data, tz, p),
        Kind::HrvTrend => hrv_trend(data, tz, p),
        Kind::HrvDaily => hrv_daily(data, tz, p),
        Kind::TrainingStatus => training_status(data, tz, p),
        Kind::TrainingLoad => training_load(data, tz, p),
        Kind::Vo2max => vo2max(data, tz, p),
        Kind::Stats => stats(data, tz, p),
        Kind::Activity => activity(data, tz, p),
        Kind::ActivitySummary => activity_summary(data, tz, p),
    }
}

// ---- small readers ----------------------------------------------------------------------

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key)?.as_f64().filter(|x| x.is_finite())
}

fn int(v: &Value, key: &str) -> Option<i64> {
    num(v, key).map(|x| x.round() as i64)
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

fn list<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    v.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("no \"{key}\" list"))
}

fn date_of(v: &Value, key: &str) -> Option<NaiveDate> {
    crate::settings::parse_date(text(v, key)?)
}

fn naive(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim().replace('T', " ");
    NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S"))
        .ok()
}

/// A UTC timestamp written without a zone (`2026-08-13T14:56:39`, `...:39.0`).
fn utc(s: &str) -> Option<i64> {
    naive(s).map(|n| n.and_utc().timestamp())
}

/// A local wall-clock time written without a zone, as an instant. The first of two readings of
/// an hour that happens twice.
fn local(tz: Tz, s: &str) -> Option<i64> {
    tz.from_local_datetime(&naive(s)?)
        .earliest()
        .map(|d| d.timestamp())
}

impl Parsed {
    /// Remember an instant a row refers to.
    fn seen(&mut self, ts: i64) {
        self.last_ts = self.last_ts.max(Some(ts));
    }
}

/// Some value in the row at all?
fn any(vals: &[Option<f64>]) -> bool {
    vals.iter().any(Option::is_some)
}

// ---- date-keyed wellness --------------------------------------------------------------------

/// Sleep nights: `{"nights": [{date, sleep_start, sleep_end (epoch ms UTC), sleep_score,
/// deep/light/rem/awake_sleep_seconds, ...}]}`. `date` is the day the night ended on; the
/// session keeps the real start and end instants. Time asleep is deep + light + REM (what the
/// watch shows), else Connect's own `sleep_seconds`.
fn sleep(data: &Value, tz: Tz, p: &mut Parsed) -> R {
    for n in list(data, "nights")? {
        let (Some(start), Some(end)) = (int(n, "sleep_start"), int(n, "sleep_end")) else {
            p.skipped += 1;
            continue;
        };
        let (start, end) = (start.div_euclid(1000), end.div_euclid(1000));
        if end <= start {
            p.skipped += 1;
            continue;
        }
        let (deep, light, rem) = (
            num(n, "deep_sleep_seconds"),
            num(n, "light_sleep_seconds"),
            num(n, "rem_sleep_seconds"),
        );
        let asleep = match (deep, light, rem) {
            (Some(d), Some(l), Some(r)) => Some(d + l + r),
            _ => num(n, "sleep_seconds"),
        };
        p.derived.sleep_sessions.push(SleepSession {
            start_ts: Some(start),
            end_ts: Some(end),
            overall_score: num(n, "sleep_score").filter(|s| *s > 0.0),
            awakenings_count: num(n, "awake_count"),
            avg_stress: num(n, "avg_sleep_stress"),
            ..Default::default()
        });
        p.sleep_stages.push(SleepStages {
            asleep_s: asleep,
            deep_s: deep,
            light_s: light,
            rem_s: rem,
            awake_s: num(n, "awake_seconds"),
        });
        p.rows += 1;
        p.seen(end);
        if let (Some(d), Some(rhr)) = (date_of(n, "date"), num(n, "resting_heart_rate_bpm")) {
            if rhr > 0.0 {
                let ts = day_start(tz, d);
                p.derived.resting_hr.push(RestingHr {
                    ts,
                    rhr: Some(rhr),
                    current_day_rhr: None,
                });
                p.rows += 1;
                p.seen(ts);
            }
        }
    }
    Ok(())
}

/// FIT `hrv_status` (the numbers the HRV endpoints name): 1 poor, 2 low, 3 unbalanced,
/// 4 balanced. `NONE` and anything else is no status.
fn hrv_status(s: &str) -> Option<i64> {
    match s.to_ascii_uppercase().as_str() {
        "POOR" => Some(1),
        "LOW" => Some(2),
        "UNBALANCED" => Some(3),
        "BALANCED" => Some(4),
        _ => None,
    }
}

fn push_hrv(p: &mut Parsed, tz: Tz, v: &Value, baselines: bool) {
    let Some(d) = date_of(v, "date") else {
        p.skipped += 1;
        return;
    };
    let row = HrvSummary {
        ts: day_start(tz, d),
        weekly_avg: num(v, "weekly_avg_hrv_ms"),
        last_night_avg: num(v, "last_night_avg_hrv_ms"),
        last_night_5min_high: num(v, "last_night_5min_high_hrv_ms"),
        baseline_low_upper: baselines.then(|| num(v, "baseline_low_upper_ms")).flatten(),
        baseline_balanced_lower: baselines
            .then(|| num(v, "baseline_balanced_low_ms"))
            .flatten(),
        baseline_balanced_upper: baselines
            .then(|| num(v, "baseline_balanced_upper_ms"))
            .flatten(),
        status: text(v, "status").and_then(hrv_status),
    };
    if any(&[
        row.weekly_avg,
        row.last_night_avg,
        row.last_night_5min_high,
        row.baseline_low_upper,
        row.baseline_balanced_lower,
        row.baseline_balanced_upper,
    ]) || row.status.is_some()
    {
        p.seen(row.ts);
        p.rows += 1;
        p.derived.hrv_summary.push(row);
    }
}

/// `{"trend": [{date, last_night_avg_hrv_ms, weekly_avg_hrv_ms, last_night_5min_high_hrv_ms, status}]}`
fn hrv_trend(data: &Value, tz: Tz, p: &mut Parsed) -> R {
    for v in list(data, "trend")? {
        push_hrv(p, tz, v, false);
    }
    Ok(())
}

/// `{date: {..., baseline_balanced_low_ms, baseline_balanced_upper_ms, baseline_low_upper_ms, status}}`
fn hrv_daily(data: &Value, tz: Tz, p: &mut Parsed) -> R {
    for v in data
        .as_object()
        .ok_or("not an object keyed by date")?
        .values()
    {
        push_hrv(p, tz, v, true);
    }
    Ok(())
}

/// `{date: {training_status (code), fitness_trend, acute_load, chronic_load, load_ratio,
/// acwr_percent, optimal_chronic_load_min/max, monthly_load_aerobic_low/high/anaerobic}}`.
/// The code is the FIT `training_status.status` number. The optimal range (chronic x 0.8 to
/// x 1.5) is the target range of the weekly (acute) load.
fn training_status(data: &Value, tz: Tz, p: &mut Parsed) -> R {
    for v in data
        .as_object()
        .ok_or("not an object keyed by date")?
        .values()
    {
        let Some(d) = date_of(v, "date") else {
            p.skipped += 1;
            continue;
        };
        let ts = day_start(tz, d);
        p.seen(ts);
        let status = TrainingStatus {
            ts,
            status: int(v, "training_status"),
            weekly_load_sum: num(v, "acute_load"),
            load_min: num(v, "optimal_chronic_load_min"),
            load_max: num(v, "optimal_chronic_load_max"),
            fitness_trend: int(v, "fitness_trend"),
            load_trend: None,
        };
        if status.status.is_some()
            || any(&[status.weekly_load_sum, status.load_min, status.load_max])
        {
            p.derived.training_status.push(status);
            p.rows += 1;
        }
        let load = TrainingLoad {
            ts,
            acute: num(v, "acute_load"),
            chronic: num(v, "chronic_load"),
            acwr: num(v, "load_ratio"),
            acwr_percent: num(v, "acwr_percent"),
            status: None,
        };
        if any(&[load.acute, load.chronic, load.acwr, load.acwr_percent]) {
            p.derived.training_load.push(load);
            p.rows += 1;
        }
        let balance = TrainingBalance {
            ts,
            aerobic_low: num(v, "monthly_load_aerobic_low"),
            aerobic_high: num(v, "monthly_load_aerobic_high"),
            anaerobic: num(v, "monthly_load_anaerobic"),
            ..Default::default()
        };
        if any(&[balance.aerobic_low, balance.aerobic_high, balance.anaerobic]) {
            p.derived.training_balance.push(balance);
            p.rows += 1;
        }
    }
    Ok(())
}

/// `{"trend": [{date, atl, ctl, acwr, acwr_percent, optimal_chronic_load_min/max,
/// training_status_code, fitness_trend}]}`: the load history of a whole year.
fn training_load(data: &Value, tz: Tz, p: &mut Parsed) -> R {
    for v in list(data, "trend")? {
        let Some(d) = date_of(v, "date") else {
            p.skipped += 1;
            continue;
        };
        let ts = day_start(tz, d);
        p.seen(ts);
        let load = TrainingLoad {
            ts,
            acute: num(v, "atl"),
            chronic: num(v, "ctl"),
            acwr: num(v, "acwr"),
            acwr_percent: num(v, "acwr_percent"),
            status: None,
        };
        if any(&[load.acute, load.chronic, load.acwr, load.acwr_percent]) {
            p.derived.training_load.push(load);
            p.rows += 1;
        }
        let status = TrainingStatus {
            ts,
            status: int(v, "training_status_code"),
            weekly_load_sum: num(v, "atl"),
            load_min: num(v, "optimal_chronic_load_min"),
            load_max: num(v, "optimal_chronic_load_max"),
            fitness_trend: int(v, "fitness_trend"),
            load_trend: None,
        };
        if status.status.is_some()
            || any(&[status.weekly_load_sum, status.load_min, status.load_max])
        {
            p.derived.training_status.push(status);
            p.rows += 1;
        }
    }
    Ok(())
}

/// `{"sport": "running", "trend": [{date, vo2_max (ml/kg/min), carried_forward}]}`. Only measured
/// values are kept (a carried-forward day repeats the last one). The table holds the maximal MET
/// as the watch writes it; the UI multiplies by 3.5, so the value is divided here.
fn vo2max(data: &Value, tz: Tz, p: &mut Parsed) -> R {
    let sport = text(data, "sport").and_then(sport_code);
    for v in list(data, "trend")? {
        if v.get("carried_forward").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let (Some(d), Some(vo2)) = (date_of(v, "date"), num(v, "vo2_max")) else {
            p.skipped += 1;
            continue;
        };
        if vo2 <= 0.0 {
            continue;
        }
        let ts = day_start(tz, d);
        p.derived.vo2max.push(Vo2max {
            ts,
            maximal_met: vo2 / 3.5,
            sport,
        });
        p.rows += 1;
        p.seen(ts);
    }
    Ok(())
}

/// `{"days": [{date, steps, total_calories, active_calories, resting_calories, has_data,
/// is_partial}]}`. A day still in progress is left out (the watch has it, and a half day would
/// stand as the day's total).
fn stats(data: &Value, tz: Tz, p: &mut Parsed) -> R {
    for v in list(data, "days")? {
        let Some(d) = date_of(v, "date") else {
            p.skipped += 1;
            continue;
        };
        let has_data = v.get("has_data").and_then(Value::as_bool).unwrap_or(true);
        let partial = v.get("is_partial").and_then(Value::as_bool) == Some(true);
        if !has_data || partial {
            continue;
        }
        let ts = day_start(tz, d);
        let row = DailyTotal {
            ts,
            steps: num(v, "steps"),
            total_kcal: num(v, "total_calories"),
            active_kcal: num(v, "active_calories"),
            resting_kcal: num(v, "resting_calories"),
        };
        if any(&[row.steps, row.total_kcal, row.active_kcal, row.resting_kcal]) {
            p.daily_totals.push(row);
            p.rows += 1;
            p.seen(ts);
        }
    }
    Ok(())
}

// ---- sport names ------------------------------------------------------------------------------

/// FIT `sport` codes by name (the SDK's public enum; the names the decoded messages carry).
pub fn sport_code(name: &str) -> Option<i64> {
    Some(match name {
        "generic" => 0,
        "running" => 1,
        "cycling" => 2,
        "transition" => 3,
        "fitness_equipment" => 4,
        "swimming" => 5,
        "basketball" => 6,
        "soccer" => 7,
        "tennis" => 8,
        "training" => 10,
        "walking" => 11,
        "cross_country_skiing" => 12,
        "alpine_skiing" => 13,
        "snowboarding" => 14,
        "rowing" => 15,
        "mountaineering" => 16,
        "hiking" => 17,
        "multisport" => 18,
        "paddling" => 19,
        "e_biking" => 21,
        "boating" => 23,
        "golf" => 25,
        "horseback_riding" => 27,
        "fishing" => 29,
        "inline_skating" => 30,
        "rock_climbing" => 31,
        "sailing" => 32,
        "ice_skating" => 33,
        "snowshoeing" => 35,
        "stand_up_paddleboarding" => 37,
        "surfing" => 38,
        "kayaking" => 41,
        "rafting" => 42,
        "boxing" => 47,
        "hiit" => 62,
        "dance" => 83,
        _ => return None,
    })
}

/// FIT `sub_sport` codes by name.
pub fn sub_sport_code(name: &str) -> Option<i64> {
    Some(match name {
        "generic" => 0,
        "treadmill" => 1,
        "street" => 2,
        "trail" => 3,
        "track" => 4,
        "spin" => 5,
        "indoor_cycling" => 6,
        "road" => 7,
        "mountain" => 8,
        "indoor_rowing" => 14,
        "elliptical" => 15,
        "stair_climbing" => 16,
        "lap_swimming" => 17,
        "open_water" => 18,
        "flexibility_training" => 19,
        "strength_training" => 20,
        "cardio_training" => 26,
        "indoor_walking" => 27,
        "yoga" => 43,
        "pilates" => 44,
        "indoor_running" => 45,
        "gravel_cycling" => 46,
        "virtual_activity" => 58,
        _ => return None,
    })
}

fn coded(name: Option<&str>, code: fn(&str) -> Option<i64>) -> Coded {
    Coded {
        code: name.and_then(code),
        name: name.map(str::to_owned),
    }
}

/// Sport and sub-sport of a Connect activity type key (`running`, `strength_training`,
/// `road_biking`, ...), for an activity without decoded FIT messages. An unknown key keeps its
/// name and has no code (it lists as "other").
fn type_sport(key: &str) -> (Coded, Coded) {
    let (sport, sub): (Option<&str>, Option<&str>) = match key {
        "running" => (Some("running"), Some("generic")),
        "trail_running" => (Some("running"), Some("trail")),
        "track_running" => (Some("running"), Some("track")),
        "treadmill_running" => (Some("running"), Some("treadmill")),
        "indoor_running" => (Some("running"), Some("indoor_running")),
        "cycling" => (Some("cycling"), Some("generic")),
        "road_biking" => (Some("cycling"), Some("road")),
        "mountain_biking" => (Some("cycling"), Some("mountain")),
        "gravel_cycling" => (Some("cycling"), Some("gravel_cycling")),
        "indoor_cycling" => (Some("cycling"), Some("indoor_cycling")),
        "virtual_ride" => (Some("cycling"), Some("virtual_activity")),
        "walking" | "casual_walking" | "speed_walking" => (Some("walking"), Some("generic")),
        "indoor_walking" => (Some("walking"), Some("indoor_walking")),
        "hiking" => (Some("hiking"), Some("generic")),
        "strength_training" => (Some("training"), Some("strength_training")),
        "yoga" => (Some("training"), Some("yoga")),
        "pilates" => (Some("training"), Some("pilates")),
        "hiit" => (Some("hiit"), Some("generic")),
        "lap_swimming" => (Some("swimming"), Some("lap_swimming")),
        "open_water_swimming" => (Some("swimming"), Some("open_water")),
        "swimming" => (Some("swimming"), Some("generic")),
        "indoor_cardio" => (Some("fitness_equipment"), Some("cardio_training")),
        "elliptical" => (Some("fitness_equipment"), Some("elliptical")),
        "stair_climbing" => (Some("fitness_equipment"), Some("stair_climbing")),
        "indoor_rowing" => (Some("fitness_equipment"), Some("indoor_rowing")),
        "kayaking" | "kayaking_v2" => (Some("kayaking"), Some("generic")),
        "horseback_riding" => (Some("horseback_riding"), Some("generic")),
        "other" => (Some("generic"), Some("generic")),
        _ => (None, None),
    };
    match sport {
        Some(s) => (coded(Some(s), sport_code), coded(sub, sub_sport_code)),
        None => (
            Coded {
                code: None,
                name: Some(key.to_owned()),
            },
            Coded::default(),
        ),
    }
}

// ---- activities -----------------------------------------------------------------------------------

fn key_of(id: i64) -> String {
    format!("connect:{id}")
}

fn has_value(s: &Summary) -> bool {
    s.start_ts.is_some()
        || s.sport.code.is_some()
        || any(&[
            s.elapsed_s,
            s.timer_s,
            s.distance_m,
            s.calories,
            s.avg_hr,
            s.max_hr,
            s.avg_speed,
            s.max_speed,
            s.ascent_m,
            s.descent_m,
            s.avg_cadence,
        ])
}

/// What the activity summary says, as one session's columns.
fn from_summary(sum: &Value, start_ts: Option<i64>, sport: Coded, sub: Coded) -> Summary {
    Summary {
        idx: 0,
        start_ts,
        sport,
        sub_sport: sub,
        elapsed_s: num(sum, "elapsed_duration_seconds").or_else(|| num(sum, "duration_seconds")),
        timer_s: num(sum, "duration_seconds"),
        distance_m: num(sum, "distance_meters"),
        calories: num(sum, "calories"),
        avg_hr: num(sum, "avg_hr_bpm"),
        max_hr: num(sum, "max_hr_bpm"),
        avg_speed: num(sum, "avg_speed_mps"),
        max_speed: num(sum, "max_speed_mps"),
        ascent_m: num(sum, "elevation_gain_meters"),
        descent_m: num(sum, "elevation_loss_meters"),
        avg_cadence: num(sum, "avg_cadence"),
    }
}

/// The values of a decoded FIT message by field name.
struct Msg<'a>(HashMap<&'a str, &'a Value>);

impl<'a> Msg<'a> {
    fn of(m: &'a Value) -> Self {
        let mut fields = HashMap::new();
        for f in m
            .get("fields")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let (Some(name), Some(v)) = (text(f, "name"), f.get("value")) {
                if !v.is_null() {
                    fields.entry(name).or_insert(v);
                }
            }
        }
        Msg(fields)
    }
    fn num(&self, name: &str) -> Option<f64> {
        self.0.get(name)?.as_f64().filter(|x| x.is_finite())
    }
    fn either(&self, a: &str, b: &str) -> Option<f64> {
        self.num(a).or_else(|| self.num(b))
    }
    /// A text value, or the first text of a list.
    fn text(&self, name: &str) -> Option<&'a str> {
        match self.0.get(name)? {
            Value::String(s) => Some(s),
            Value::Array(a) => a.iter().find_map(Value::as_str),
            _ => None,
        }
    }
    fn time(&self, name: &str) -> Option<i64> {
        utc(self.text(name)?)
    }
}

fn msg_summary(m: &Msg, idx: u32) -> Summary {
    Summary {
        idx,
        start_ts: m.time("start_time"),
        sport: coded(m.text("sport"), sport_code),
        sub_sport: coded(m.text("sub_sport"), sub_sport_code),
        elapsed_s: m.num("total_elapsed_time"),
        timer_s: m.num("total_timer_time"),
        distance_m: m.num("total_distance"),
        calories: m.num("total_calories"),
        avg_hr: m.num("avg_heart_rate"),
        max_hr: m.num("max_heart_rate"),
        avg_speed: m.either("enhanced_avg_speed", "avg_speed"),
        max_speed: m.either("enhanced_max_speed", "max_speed"),
        ascent_m: m.num("total_ascent"),
        descent_m: m.num("total_descent"),
        avg_cadence: m.either("avg_cadence", "avg_running_cadence"),
    }
}

/// Fill the gaps of a session read from the FIT messages with the activity summary's figures.
fn fill_gaps(s: &mut Summary, from: &Summary) {
    s.start_ts = s.start_ts.or(from.start_ts);
    s.elapsed_s = s.elapsed_s.or(from.elapsed_s);
    s.timer_s = s.timer_s.or(from.timer_s);
    s.distance_m = s.distance_m.or(from.distance_m);
    s.calories = s.calories.or(from.calories);
    s.avg_hr = s.avg_hr.or(from.avg_hr);
    s.max_hr = s.max_hr.or(from.max_hr);
    s.avg_speed = s.avg_speed.or(from.avg_speed);
    s.max_speed = s.max_speed.or(from.max_speed);
    s.ascent_m = s.ascent_m.or(from.ascent_m);
    s.descent_m = s.descent_m.or(from.descent_m);
    s.avg_cadence = s.avg_cadence.or(from.avg_cadence);
    if s.sport.name.is_none() {
        s.sport = from.sport.clone();
    }
    if s.sub_sport.name.is_none() {
        s.sub_sport = from.sub_sport.clone();
    }
}

/// `set_type`: rest 0, active 1 (the FIT enum).
fn set_type(name: &str) -> Coded {
    Coded {
        code: match name {
            "rest" => Some(0),
            "active" => Some(1),
            _ => None,
        },
        name: Some(name.to_owned()),
    }
}

/// One activity: `{"summary": get_activity, "fit": {"messages": [{"type": "session", "fields":
/// [{"name", "value"}]}]}}`. Sessions, laps, strength sets and exercise titles come from the
/// decoded FIT messages (UTC), the rest and any gap from the summary; there are no records, so
/// no track or series. HR zones are not in the data.
fn activity(data: &Value, tz: Tz, p: &mut Parsed) -> R {
    let sum = data.get("summary").ok_or("no summary")?;
    let id = sum
        .get("id")
        .and_then(Value::as_i64)
        .ok_or("the summary has no id")?;
    let gmt = text(sum, "start_time_gmt").and_then(utc);
    let loc = text(sum, "start_time_local").and_then(naive);
    let start = gmt.or_else(|| text(sum, "start_time_local").and_then(|s| local(tz, s)));
    let offset = match (gmt, loc) {
        (Some(g), Some(l)) => Some(l.and_utc().timestamp() - g),
        _ => None,
    };
    let (type_sport_c, type_sub_c) = type_sport(text(sum, "type").unwrap_or("other"));
    let summary_row = from_summary(sum, start, type_sport_c, type_sub_c);

    let messages: &[Value] = data
        .pointer("/fit/messages")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let mut counters = [0u32; 4];
    let mut next = |slot: usize| {
        let i = counters[slot];
        counters[slot] += 1;
        i
    };
    let mut sessions: Vec<SessionRow> = Vec::new();
    let mut end_ts: Option<i64> = None;
    for m in messages {
        let Some(kind) = text(m, "type") else {
            continue;
        };
        let f = Msg::of(m);
        p.rows += 1;
        match kind {
            "session" => {
                let idx = next(0);
                let s = msg_summary(&f, idx);
                end_ts = end_ts.max(f.time("timestamp"));
                p.seen(end_ts.unwrap_or(0));
                sessions.push(SessionRow {
                    s,
                    aerobic_te: f.num("total_training_effect"),
                    anaerobic_te: f.num("total_anaerobic_training_effect"),
                    training_load: None,
                });
            }
            "lap" => {
                let idx = next(1);
                let s = msg_summary(&f, idx);
                if has_value(&s) {
                    p.derived.activity.laps.push(LapRow { s });
                }
            }
            "set" => {
                let idx = next(2);
                let set = StrengthSet {
                    idx,
                    ts: f.time("timestamp"),
                    start_ts: f.time("start_time"),
                    duration_s: f.num("duration"),
                    set_type: f.text("set_type").map(set_type).unwrap_or_default(),
                    reps: f.num("repetitions"),
                    weight_kg: f.num("weight"),
                    category: Coded {
                        code: None,
                        name: f.text("category").map(str::to_owned),
                    },
                    category_subtype: f.num("category_subtype").map(|x| x.round() as i64),
                    category_all: vec![],
                    category_subtype_all: vec![],
                    wkt_step_index: f.num("wkt_step_index").map(|x| x.round() as i64),
                };
                if set.ts.is_some()
                    || set.start_ts.is_some()
                    || set.duration_s.is_some()
                    || set.set_type.name.is_some()
                    || set.reps.is_some()
                    || set.weight_kg.is_some()
                    || set.category.name.is_some()
                {
                    p.derived.activity.sets.push(set);
                }
            }
            "exercise_title" => {
                let idx = next(3);
                let title = ExerciseTitle {
                    idx,
                    category: Coded {
                        code: None,
                        name: f.text("exercise_category").map(str::to_owned),
                    },
                    subtype: f.num("exercise_name").map(|x| x.round() as i64),
                    name: f.text("wkt_step_name").map(str::to_owned),
                };
                if title.category.name.is_some() || title.subtype.is_some() || title.name.is_some()
                {
                    p.derived.activity.exercise_titles.push(title);
                }
            }
            _ => {}
        }
    }
    let one = sessions.len() <= 1;
    for s in &mut sessions {
        fill_gaps(&mut s.s, &summary_row);
        if one {
            s.training_load = num(sum, "training_load");
            s.aerobic_te = s.aerobic_te.or_else(|| num(sum, "training_effect"));
            s.anaerobic_te = s
                .anaerobic_te
                .or_else(|| num(sum, "anaerobic_training_effect"));
        }
    }
    if sessions.is_empty() {
        sessions.push(SessionRow {
            s: summary_row.clone(),
            aerobic_te: num(sum, "training_effect"),
            anaerobic_te: num(sum, "anaerobic_training_effect"),
            training_load: num(sum, "training_load"),
        });
    }
    let first = sessions[0].s.clone();
    let start_ts = first
        .start_ts
        .or(start)
        .ok_or("the activity has no start time")?;
    let duration = num(sum, "duration_seconds");
    let end = end_ts.or(duration.map(|d| start_ts + d as i64));
    p.seen(end.unwrap_or(start_ts));
    p.derived.activity.activity = Some(ActivityRow {
        activity_key: Some(key_of(id)),
        serial: None,
        time_created: Some(start_ts),
        ts: end,
        local_offset_s: offset,
        total_timer_s: duration,
        num_sessions: Some(sessions.len() as i64),
        start_ts: Some(start_ts),
        sport: first.sport.clone(),
        sub_sport: first.sub_sport.clone(),
    });
    p.derived.activity.sessions = sessions;
    let metrics = ActivityMetrics {
        maximal_met: None,
        aerobic_te: num(sum, "training_effect"),
        anaerobic_te: num(sum, "anaerobic_training_effect"),
        training_load_peak: num(sum, "training_load"),
        epoc: None,
        recovery_time_physical_activity_raw: None,
    };
    if metrics != ActivityMetrics::default() {
        p.derived.activity.metrics = Some(metrics);
    }
    p.rows += 1;
    Ok(())
}

/// One entry of the activity list: `{id, type, start_time (local), duration_seconds,
/// distance_meters, calories, avg_hr_bpm, max_hr_bpm, elevation_gain_meters, ...}`.
fn activity_summary(data: &Value, tz: Tz, p: &mut Parsed) -> R {
    let id = data
        .get("id")
        .and_then(Value::as_i64)
        .ok_or("the entry has no id")?;
    let start = text(data, "start_time")
        .and_then(|s| local(tz, s))
        .ok_or("the entry has no start time")?;
    let (sport, sub) = type_sport(text(data, "type").unwrap_or("other"));
    let row = from_summary(data, Some(start), sport.clone(), sub.clone());
    let duration = num(data, "duration_seconds");
    let end = start + duration.map_or(0, |d| d as i64);
    p.derived.activity.activity = Some(ActivityRow {
        activity_key: Some(key_of(id)),
        serial: None,
        time_created: Some(start),
        ts: Some(end),
        local_offset_s: None,
        total_timer_s: duration,
        num_sessions: Some(1),
        start_ts: Some(start),
        sport,
        sub_sport: sub,
    });
    p.derived.activity.sessions = vec![SessionRow {
        s: row,
        aerobic_te: None,
        anaerobic_te: None,
        training_load: None,
    }];
    p.seen(end);
    p.rows += 1;
    Ok(())
}
