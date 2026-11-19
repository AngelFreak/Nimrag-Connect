//! Wellness parser: training metrics (378 `acute_load_results`, 232
//! `training_history_results`, 294 `training_balance_results`, 284
//! `recovery_time`). Verified on real 128/44 files for the messages present in
//! them (see the server replay test); values are stored as reported.
//!
//! - `training_load`: `daily_training_load_acute`, `daily_training_load_chronic`,
//!   `daily_acute_chronic_workload_ratio`, `acwr_percent`, `status`.
//! - `training_status`: `training_status`, `weekly_training_load_sum`,
//!   `load_tunnel_min`/`max`, `fitness_level_trend`, `load_level_trend`.
//! - `training_balance`: `monthly_load_*` (+ target min/max).
//! - `vo2max`: `maximal_met` as written (no MET x 3.5 conversion) from
//!   `max_met_data` (229: `max_met`, keyed by `update_time`) and from
//!   `firstbeat_results` (140: `maximal_met`, `sport`) ONLY in files without
//!   activity rows (activity files keep it in `activity_metrics`).
//! - `recovery_time`: `recovery_time_minutes`, keyed by `recovery_time_start`
//!   (the last message per start wins within a file; a FIT-epoch time; below the UTC threshold the row is skipped and counted).
use super::{
    fit_utc, Derived, MsgExt, RecoveryTime, TrainingBalance, TrainingLoad, TrainingStatus, Vo2max,
};
use crate::fit::interpret::Interpreted;

const TRAINING_HISTORY_RESULTS: u16 = 232;
const RECOVERY_TIME: u16 = 284;
const TRAINING_BALANCE_RESULTS: u16 = 294;
const ACUTE_LOAD_RESULTS: u16 = 378;
const FIRSTBEAT_RESULTS: u16 = 140;
const MAX_MET_DATA: u16 = 229;

pub fn run(msgs: &[Interpreted], out: &mut Derived) {
    let a = &out.activity;
    let has_activity = a.activity.is_some()
        || !a.sessions.is_empty()
        || !a.laps.is_empty()
        || !a.records.is_empty();
    for m in msgs {
        match m.global {
            MAX_MET_DATA => {
                if let Some(met) = m.number("max_met") {
                    match m.number("update_time").and_then(fit_utc) {
                        Some(ts) => out.vo2max.push(Vo2max {
                            ts,
                            maximal_met: met,
                            sport: m.number("sport").map(|v| v as i64),
                        }),
                        None => out.skipped += 1,
                    }
                }
            }
            FIRSTBEAT_RESULTS if !has_activity => {
                if let Some(met) = m.number("maximal_met") {
                    match m.unix_ts() {
                        Some(ts) => out.vo2max.push(Vo2max {
                            ts,
                            maximal_met: met,
                            sport: m.number("sport").map(|v| v as i64),
                        }),
                        None => out.skipped += 1,
                    }
                }
            }
            ACUTE_LOAD_RESULTS => {
                let r = TrainingLoad {
                    ts: 0,
                    acute: m.number("daily_training_load_acute"),
                    chronic: m.number("daily_training_load_chronic"),
                    acwr: m.number("daily_acute_chronic_workload_ratio"),
                    acwr_percent: m.number("acwr_percent"),
                    status: m.number("status").map(|v| v as i64),
                };
                if r != (TrainingLoad::default()) {
                    match m.unix_ts() {
                        Some(ts) => out.training_load.push(TrainingLoad { ts, ..r }),
                        None => out.skipped += 1,
                    }
                }
            }
            TRAINING_HISTORY_RESULTS => {
                let r = TrainingStatus {
                    ts: 0,
                    status: m.number("training_status").map(|v| v as i64),
                    weekly_load_sum: m.number("weekly_training_load_sum"),
                    load_min: m.number("load_tunnel_min"),
                    load_max: m.number("load_tunnel_max"),
                    fitness_trend: m.number("fitness_level_trend").map(|v| v as i64),
                    load_trend: m.number("load_level_trend").map(|v| v as i64),
                };
                if r != (TrainingStatus::default()) {
                    match m.unix_ts() {
                        Some(ts) => out.training_status.push(TrainingStatus { ts, ..r }),
                        None => out.skipped += 1,
                    }
                }
            }
            TRAINING_BALANCE_RESULTS => {
                let r = TrainingBalance {
                    ts: 0,
                    aerobic_low: m.number("monthly_load_aerobic_low"),
                    aerobic_high: m.number("monthly_load_aerobic_high"),
                    anaerobic: m.number("monthly_load_anaerobic"),
                    aerobic_low_target_min: m.number("monthly_load_aerobic_low_target_min"),
                    aerobic_low_target_max: m.number("monthly_load_aerobic_low_target_max"),
                    aerobic_high_target_min: m.number("monthly_load_aerobic_high_target_min"),
                    aerobic_high_target_max: m.number("monthly_load_aerobic_high_target_max"),
                    anaerobic_target_min: m.number("monthly_load_anaerobic_target_min"),
                    anaerobic_target_max: m.number("monthly_load_anaerobic_target_max"),
                };
                if r != (TrainingBalance::default()) {
                    match m.unix_ts() {
                        Some(ts) => out.training_balance.push(TrainingBalance { ts, ..r }),
                        None => out.skipped += 1,
                    }
                }
            }
            RECOVERY_TIME => {
                if let Some(minutes) = m.number("recovery_time_minutes") {
                    match m.number("recovery_time_start").and_then(fit_utc) {
                        Some(start_ts) => {
                            // Same start twice in a file: the last message wins.
                            out.recovery_time.retain(|r| r.start_ts != start_ts);
                            out.recovery_time.push(RecoveryTime { start_ts, minutes })
                        }
                        None => out.skipped += 1,
                    }
                }
            }
            _ => {}
        }
    }
}
