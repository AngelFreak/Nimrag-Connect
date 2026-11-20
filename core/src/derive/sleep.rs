//! Wellness parser: sleep (275 `sleep_level`, 346 `sleep_assessment`, 411
//! `sleep_summary`).
//!
//! UNVERIFIED ON REAL FILES: no sleep file has been captured yet; this is built
//! from the local profile and synthetic tests only.
//!
//! - `sleep_levels`: `sleep_level` value and enum label, at the message time.
//! - `sleep_sessions`: one `SleepSession` per `sleep_summary` (explicit
//!   `sleep_start_timestamp_utc` / `sleep_end_timestamp_utc`; no other source of
//!   start/end is used, so they stay NULL without a start). A file such as the
//!   watch's backup holds a history of nights, so every summary becomes a row;
//!   a second summary with the same start time within a file is dropped.
//!   `sleep_assessment` (scores, no time of its own) is attached only when it
//!   is unambiguous: a file with exactly one summary (the assessment beats the
//!   summary's overall score) or with no summary (a session without times).
//!   With several summaries the assessments are not attributed to any night
//!   and are counted in `Derived::skipped`.
//! - Without a summary, the session's start/end come from the file's sleep
//!   events (`event` 21 with event 74, unnamed in the public FIT profile: type
//!   start, then stop), as the Instinct 2's nightly sleep file (128/49) has them.
use super::{fit_utc, Derived, SleepLevel, SleepSession};
use crate::fit::interpret::Interpreted;

const SLEEP_LEVEL: u16 = 275;
const SLEEP_ASSESSMENT: u16 = 346;
const SLEEP_SUMMARY: u16 = 411;
const EVENT: u16 = 21;
/// `event.event` value the watch writes at falling asleep (start) and waking (stop).
const EVENT_SLEEP: f64 = 74.0;

pub fn run(msgs: &[Interpreted], out: &mut Derived) {
    let mut assessment: Option<SleepSession> = None;
    let mut assessments = 0u64;
    let mut summaries: Vec<SleepSession> = Vec::new();
    // First sleep start event and the first stop after it.
    let (mut slept, mut woke): (Option<i64>, Option<i64>) = (None, None);
    for m in msgs {
        match m.global {
            SLEEP_LEVEL => level(m, out),
            EVENT if m.number("event") == Some(EVENT_SLEEP) => {
                let ts = super::MsgExt::unix_ts(m);
                match m.number("event_type") {
                    Some(0.0) if slept.is_none() => slept = ts,
                    Some(1.0) if slept.is_some() && woke.is_none() => woke = ts,
                    _ => {}
                }
            }
            SLEEP_ASSESSMENT => {
                assessments += 1;
                if assessment.is_none() {
                    assessment = Some(SleepSession {
                        overall_score: m.number("overall_sleep_score"),
                        quality_score: m.number("sleep_quality_score"),
                        duration_score: m.number("sleep_duration_score"),
                        recovery_score: m.number("sleep_recovery_score"),
                        deep_score: m.number("deep_sleep_score"),
                        light_score: m.number("light_sleep_score"),
                        rem_score: m.number("rem_sleep_score"),
                        awakenings_count: m.number("awakenings_count"),
                        avg_stress: m.number("average_stress_during_sleep"),
                        ..Default::default()
                    });
                }
            }
            SLEEP_SUMMARY => {
                let start_ts = m.number("sleep_start_timestamp_utc").and_then(fit_utc);
                if start_ts.is_some() && summaries.iter().any(|s| s.start_ts == start_ts) {
                    continue; // first summary per start time wins
                }
                summaries.push(SleepSession {
                    start_ts,
                    end_ts: m.number("sleep_end_timestamp_utc").and_then(fit_utc),
                    overall_score: m.number("overall_sleep_score"),
                    ..Default::default()
                });
            }
            _ => {}
        }
    }
    match summaries.len() {
        0 => out
            .sleep_sessions
            .extend(assessment.map(|a| match (slept, woke) {
                (Some(start), Some(end)) if start < end => SleepSession {
                    start_ts: Some(start),
                    end_ts: Some(end),
                    ..a
                },
                _ => a,
            })),
        1 => {
            let mut s = summaries.remove(0);
            if let Some(a) = assessment {
                s = SleepSession {
                    start_ts: s.start_ts,
                    end_ts: s.end_ts,
                    overall_score: a.overall_score.or(s.overall_score),
                    ..a
                };
            }
            out.sleep_sessions.push(s);
        }
        _ => {
            out.skipped += assessments;
            out.sleep_sessions.extend(summaries);
        }
    }
    out.sleep_sessions.retain(|s| *s != SleepSession::default());
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
