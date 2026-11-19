//! Wellness parser: monitoring files (messages 55 `monitoring`, 103
//! `monitoring_info`, 211 `monitoring_hr_data`, 227 `stress_level`, 297
//! `respiration_rate`, 269 `spo2_data`).
//!
//! Verified on real monitoring files (Instinct 2) for 55, 103, 211, 227, 297.
//! `spo2_data` has not been seen in a real file yet (synthetic tests only).
//!
//! Field mapping (profile names, read through `Interpreted::number`):
//! - `mon_hr`: `heart_rate`.
//! - `mon_activity`: `steps` (subfield of `cycles`, only when the activity type
//!   says steps), `distance`, `active_calories`, `active_time`, `duration_min`,
//!   `intensity`; keyed by `activity_type` (field 5 or the component of 24).
//!   Cumulative counters are stored as reported, never differenced.
//! - `mon_extra`: fields 35-38 are `ascent_total`, `descent_total`,
//!   `moderate_activity_minutes_total`, `vigorous_activity_minutes_total`
//!   (running totals as reported). Floors are not derived: no watch-written
//!   floors field exists, and ascent is never divided by 3.048.
//! - `stress`: `stress_level_value` (negative = status code: stress NULL,
//!   `stress_raw` keeps it), `body_battery`, `body_battery_status`; the time is
//!   `stress_level_time`.
//! - `respiration`: `respiration_rate` (non-positive: brpm NULL, raw kept).
//! - `spo2`: `reading_spo2`, `reading_confidence`, `mode`.
//! - `resting_hr`: `resting_heart_rate`, `current_day_resting_heart_rate`.
//! - `mon_info`: `local_timestamp - timestamp`, `resting_metabolic_rate`.
//!   `steps_goal` is left NULL: `cycles_goal` is a per-activity-type array whose
//!   profile scale (2) does not obviously apply to steps; unresolved until
//!   checked against the Garmin Connect goal.
//!
//! A message with a usable value but no UTC time (device-relative, or no full
//! timestamp to anchor `timestamp_16` on) is skipped and counted.
use super::{fit_utc, Derived, MonActivity, MonExtra, MonHr, MonInfo};
use super::{Respiration, RestingHr, Spo2, Stress};
use crate::fit::interpret::Interpreted;

const MONITORING: u16 = 55;
const MONITORING_INFO: u16 = 103;
const MONITORING_HR_DATA: u16 = 211;
const STRESS_LEVEL: u16 = 227;
const SPO2_DATA: u16 = 269;
const RESPIRATION_RATE: u16 = 297;

pub fn run(msgs: &[Interpreted], out: &mut Derived) {
    for m in msgs {
        match m.global {
            MONITORING => monitoring(m, out),
            MONITORING_INFO => info(m, out),
            MONITORING_HR_DATA => resting_hr(m, out),
            STRESS_LEVEL => stress(m, out),
            SPO2_DATA => spo2(m, out),
            RESPIRATION_RATE => respiration(m, out),
            _ => {}
        }
    }
}

/// UTC time of a message, or count it as skipped.
fn ts_or_skip(m: &Interpreted, out: &mut Derived) -> Option<i64> {
    let ts = super::MsgExt::unix_ts(m);
    if ts.is_none() {
        out.skipped += 1;
    }
    ts
}

fn monitoring(m: &Interpreted, out: &mut Derived) {
    let hr = m.number("heart_rate");
    let (steps, distance_m, active_calories, active_time_s) = (
        m.number("steps"),
        m.number("distance"),
        m.number("active_calories"),
        m.number("active_time"),
    );
    let (duration_min, intensity) = (m.number("duration_min"), m.number("intensity"));
    let activity_values = [
        steps,
        distance_m,
        active_calories,
        active_time_s,
        duration_min,
        intensity,
    ];
    let extra = [
        m.number("ascent_total"),
        m.number("descent_total"),
        m.number("moderate_activity_minutes_total"),
        m.number("vigorous_activity_minutes_total"),
    ];
    let has_activity = activity_values.iter().any(Option::is_some);
    let activity_type = m.number("activity_type").map(|t| t as i64);
    // Counted once per message even when several row kinds are lost.
    let dropped_cycles = m.number("cycles").or(m.number("strokes")).is_some();
    let has_any =
        hr.is_some() || dropped_cycles || has_activity || extra.iter().any(Option::is_some);
    if !has_any {
        return;
    }
    let Some(ts) = ts_or_skip(m, out) else {
        return;
    };
    // `cycles`/`strokes` (non-step activity types) are not stored: count them.
    if dropped_cycles {
        out.skipped += 1;
    }
    if let Some(bpm) = hr {
        out.mon_hr.push(MonHr { ts, bpm });
    }
    if has_activity {
        match activity_type {
            Some(activity_type) => out.mon_activity.push(MonActivity {
                ts,
                activity_type,
                steps,
                distance_m,
                active_calories,
                active_time_s,
                duration_min,
                intensity,
            }),
            // No activity type: the row has no key.
            None => out.skipped += 1,
        }
    }
    if extra.iter().any(Option::is_some) {
        out.mon_extra.push(MonExtra {
            ts,
            ascent_total_m: extra[0],
            descent_total_m: extra[1],
            moderate_total_min: extra[2],
            vigorous_total_min: extra[3],
        });
    }
}

fn info(m: &Interpreted, out: &mut Derived) {
    if out.mon_info.is_some() {
        return; // one per file; the first wins
    }
    // Both times must be real (non-device-relative); subtract in unix seconds.
    let local_offset_s = match (
        m.number("local_timestamp").and_then(fit_utc),
        super::MsgExt::unix_ts(m),
    ) {
        (Some(local), Some(utc)) => Some((local - utc) as f64),
        _ => None,
    };
    let info = MonInfo {
        local_offset_s,
        rmr_kcal_day: m.number("resting_metabolic_rate"),
        steps_goal: None,
    };
    if info.local_offset_s.is_some() || info.rmr_kcal_day.is_some() {
        out.mon_info = Some(info);
    }
}

fn resting_hr(m: &Interpreted, out: &mut Derived) {
    let (rhr, current_day_rhr) = (
        m.number("resting_heart_rate"),
        m.number("current_day_resting_heart_rate"),
    );
    if rhr.is_none() && current_day_rhr.is_none() {
        return;
    }
    if let Some(ts) = ts_or_skip(m, out) {
        out.resting_hr.push(RestingHr {
            ts,
            rhr,
            current_day_rhr,
        });
    }
}

fn stress(m: &Interpreted, out: &mut Derived) {
    let raw = m.number("stress_level_value");
    let body_battery = m.number("body_battery");
    let status = m.number("body_battery_status").map(|s| s as i64);
    if raw.is_none() && body_battery.is_none() && status.is_none() {
        return;
    }
    let Some(ts) = m.number("stress_level_time").and_then(fit_utc) else {
        out.skipped += 1;
        return;
    };
    out.stress.push(Stress {
        ts,
        stress: raw.filter(|v| *v >= 0.0),
        stress_raw: raw,
        body_battery,
        body_battery_status: status,
    });
}

fn respiration(m: &Interpreted, out: &mut Derived) {
    let Some(raw) = m.number("respiration_rate") else {
        return;
    };
    if let Some(ts) = ts_or_skip(m, out) {
        out.respiration.push(Respiration {
            ts,
            brpm: Some(raw).filter(|v| *v > 0.0),
            brpm_raw: Some(raw),
        });
    }
}

fn spo2(m: &Interpreted, out: &mut Derived) {
    let (percent, confidence) = (m.number("reading_spo2"), m.number("reading_confidence"));
    if percent.is_none() && confidence.is_none() {
        return;
    }
    if let Some(ts) = ts_or_skip(m, out) {
        out.spo2.push(Spo2 {
            ts,
            percent,
            confidence,
            mode: m.number("mode").map(|v| v as i64),
        });
    }
}
