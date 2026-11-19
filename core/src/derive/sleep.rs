//! Wellness parser: sleep (275 `sleep_level`, 346 `sleep_assessment`, 411
//! `sleep_summary`).
//!
//! UNVERIFIED ON REAL FILES: no sleep file has been captured yet; this is built
//! from the local profile and synthetic tests only.
//!
//! - `sleep_levels`: `sleep_level` value and enum label, at the message time.
//! - One `SleepSession` per file at most, from the first `sleep_assessment`
//!   (scores) and first `sleep_summary` (explicit `sleep_start_timestamp_utc` /
//!   `sleep_end_timestamp_utc`; no other source of start/end is used, so they
//!   stay NULL without a summary). The overall score prefers the assessment's
//!   and falls back to the summary's.
use super::{fit_utc, Derived, SleepLevel, SleepSession};
use crate::fit::interpret::Interpreted;

const SLEEP_LEVEL: u16 = 275;
const SLEEP_ASSESSMENT: u16 = 346;
const SLEEP_SUMMARY: u16 = 411;

pub fn run(msgs: &[Interpreted], out: &mut Derived) {
    let mut session = SleepSession::default();
    let mut seen_assessment = false;
    let mut seen_summary = false;
    for m in msgs {
        match m.global {
            SLEEP_LEVEL => level(m, out),
            SLEEP_ASSESSMENT if !seen_assessment => {
                seen_assessment = true;
                session.overall_score = m.number("overall_sleep_score");
                session.quality_score = m.number("sleep_quality_score");
                session.duration_score = m.number("sleep_duration_score");
                session.recovery_score = m.number("sleep_recovery_score");
                session.deep_score = m.number("deep_sleep_score");
                session.light_score = m.number("light_sleep_score");
                session.rem_score = m.number("rem_sleep_score");
                session.awakenings_count = m.number("awakenings_count");
                session.avg_stress = m.number("average_stress_during_sleep");
            }
            SLEEP_SUMMARY if !seen_summary => {
                seen_summary = true;
                session.start_ts = m.number("sleep_start_timestamp_utc").and_then(fit_utc);
                session.end_ts = m.number("sleep_end_timestamp_utc").and_then(fit_utc);
                if session.overall_score.is_none() {
                    session.overall_score = m.number("overall_sleep_score");
                }
            }
            _ => {}
        }
    }
    if session != SleepSession::default() {
        out.sleep_sessions.push(session);
    }
}

fn level(m: &Interpreted, out: &mut Derived) {
    let Some(level) = m.number("sleep_level") else {
        return;
    };
    match super::MsgExt::unix_ts(m) {
        Some(ts) => out.sleep_levels.push(SleepLevel {
            ts,
            level: level as i64,
            level_name: m
                .get("sleep_level")
                .and_then(|v| v.enum_label)
                .map(str::to_owned),
        }),
        None => out.skipped += 1,
    }
}
