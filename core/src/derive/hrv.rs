//! Wellness parser: HRV (371 `hrv_value`, 370 `hrv_status_summary`).
//!
//! UNVERIFIED ON REAL FILES: no HRV file has been captured yet; this is built
//! from the local profile and synthetic tests only. `hrv_value.value` is stored
//! as `rmssd_ms` as the contract assumes; confirm against a real file.
//!
//! The raw beat-interval message `hrv` (78) is not derived.
use super::{Derived, HrvSummary, HrvValue, MsgExt};
use crate::fit::interpret::Interpreted;

const HRV_STATUS_SUMMARY: u16 = 370;
const HRV_VALUE: u16 = 371;

pub fn run(msgs: &[Interpreted], out: &mut Derived) {
    for m in msgs {
        match m.global {
            HRV_VALUE => {
                if let Some(v) = m.number("value") {
                    match m.unix_ts() {
                        Some(ts) => out.hrv_values.push(HrvValue { ts, rmssd_ms: v }),
                        None => out.skipped += 1,
                    }
                }
            }
            HRV_STATUS_SUMMARY => summary(m, out),
            _ => {}
        }
    }
}

fn summary(m: &Interpreted, out: &mut Derived) {
    let s = HrvSummary {
        ts: 0,
        weekly_avg: m.number("weekly_average"),
        last_night_avg: m.number("last_night_average"),
        last_night_5min_high: m.number("last_night_5_min_high"),
        baseline_low_upper: m.number("baseline_low_upper"),
        baseline_balanced_lower: m.number("baseline_balanced_lower"),
        baseline_balanced_upper: m.number("baseline_balanced_upper"),
        status: m.number("status").map(|v| v as i64),
    };
    if (HrvSummary { ts: 0, ..s.clone() }) == HrvSummary::default() {
        return;
    }
    match m.unix_ts() {
        Some(ts) => out.hrv_summary.push(HrvSummary { ts, ..s }),
        None => out.skipped += 1,
    }
}
