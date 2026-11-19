//! Wellness persistence: sample tables (rank-based upsert) and the two entity
//! tables `mon_info` (first row per file wins) and `sleep_sessions` (every session of a file, keyed by position).
use anyhow::Result;
use nimrag_core::derive::{
    Battery, Derived, HrvSummary, HrvValue, MonActivity, MonExtra, MonHr, RecoveryTime,
    Respiration, RestingHr, SleepLevel, Spo2, Stress, TrainingBalance, TrainingLoad,
    TrainingStatus, Vo2max,
};
use sqlx::{Sqlite, Transaction};

use super::{upsert_samples, V};

pub const SAMPLE_TABLES: &[&str] = &[
    "mon_hr",
    "mon_activity",
    "mon_extra",
    "stress",
    "respiration",
    "spo2",
    "resting_hr",
    "sleep_levels",
    "hrv_values",
    "hrv_summary",
    "training_load",
    "training_status",
    "training_balance",
    "recovery_time",
    "vo2max",
    "battery",
];
pub const ENTITY_TABLES: &[&str] = &["mon_info", "sleep_sessions"];

/// Collect the rows before awaiting: holding a borrowing iterator across the
/// await makes the handler future fail rustc's higher-ranked Send check.
macro_rules! up {
    ($tx:expr, $t:expr, $k:expr, $v:expr, $src:expr, $rows:expr $(,)?) => {{
        let rows: Vec<Vec<V>> = $rows.collect();
        upsert_samples($tx, $t, $k, $v, $src, rows).await?;
    }};
}

fn i(x: i64) -> V {
    V::I(Some(x))
}
fn oi(x: Option<i64>) -> V {
    V::I(x)
}
fn r(x: f64) -> V {
    V::R(Some(x))
}
fn or(x: Option<f64>) -> V {
    V::R(x)
}

pub async fn write(
    tx: &mut Transaction<'_, Sqlite>,
    sha256: &str,
    src: i64,
    d: &Derived,
) -> Result<()> {
    up!(
        tx,
        "mon_hr",
        &["ts"],
        &["bpm"],
        src,
        d.mon_hr.iter().map(|x: &MonHr| vec![i(x.ts), r(x.bpm)]),
    );
    up!(
        tx,
        "mon_activity",
        &["ts", "activity_type"],
        &[
            "steps",
            "distance_m",
            "active_calories",
            "active_time_s",
            "duration_min",
            "intensity",
        ],
        src,
        d.mon_activity.iter().map(|x: &MonActivity| {
            vec![
                i(x.ts),
                i(x.activity_type),
                or(x.steps),
                or(x.distance_m),
                or(x.active_calories),
                or(x.active_time_s),
                or(x.duration_min),
                or(x.intensity),
            ]
        }),
    );
    up!(
        tx,
        "mon_extra",
        &["ts"],
        &[
            "ascent_total_m",
            "descent_total_m",
            "moderate_total_min",
            "vigorous_total_min"
        ],
        src,
        d.mon_extra.iter().map(|x: &MonExtra| {
            vec![
                i(x.ts),
                or(x.ascent_total_m),
                or(x.descent_total_m),
                or(x.moderate_total_min),
                or(x.vigorous_total_min),
            ]
        }),
    );
    up!(
        tx,
        "stress",
        &["ts"],
        &[
            "stress",
            "stress_raw",
            "body_battery",
            "body_battery_status",
        ],
        src,
        d.stress.iter().map(|x: &Stress| {
            vec![
                i(x.ts),
                or(x.stress),
                or(x.stress_raw),
                or(x.body_battery),
                oi(x.body_battery_status),
            ]
        }),
    );
    up!(
        tx,
        "respiration",
        &["ts"],
        &["brpm", "brpm_raw"],
        src,
        d.respiration
            .iter()
            .map(|x: &Respiration| vec![i(x.ts), or(x.brpm), or(x.brpm_raw)]),
    );
    up!(
        tx,
        "spo2",
        &["ts"],
        &["percent", "confidence", "mode"],
        src,
        d.spo2
            .iter()
            .map(|x: &Spo2| vec![i(x.ts), or(x.percent), or(x.confidence), oi(x.mode)]),
    );
    up!(
        tx,
        "resting_hr",
        &["ts"],
        &["rhr", "current_day_rhr"],
        src,
        d.resting_hr
            .iter()
            .map(|x: &RestingHr| vec![i(x.ts), or(x.rhr), or(x.current_day_rhr)]),
    );
    up!(
        tx,
        "sleep_levels",
        &["ts"],
        &["level", "level_name"],
        src,
        d.sleep_levels.iter().map(|x: &SleepLevel| vec![
            i(x.ts),
            i(x.level),
            V::T(x.level_name.clone())
        ]),
    );
    up!(
        tx,
        "hrv_values",
        &["ts"],
        &["rmssd_ms"],
        src,
        d.hrv_values
            .iter()
            .map(|x: &HrvValue| vec![i(x.ts), r(x.rmssd_ms)]),
    );
    up!(
        tx,
        "hrv_summary",
        &["ts"],
        &[
            "weekly_avg",
            "last_night_avg",
            "last_night_5min_high",
            "baseline_low_upper",
            "baseline_balanced_lower",
            "baseline_balanced_upper",
            "status",
        ],
        src,
        d.hrv_summary.iter().map(|x: &HrvSummary| {
            vec![
                i(x.ts),
                or(x.weekly_avg),
                or(x.last_night_avg),
                or(x.last_night_5min_high),
                or(x.baseline_low_upper),
                or(x.baseline_balanced_lower),
                or(x.baseline_balanced_upper),
                oi(x.status),
            ]
        }),
    );
    up!(
        tx,
        "training_load",
        &["ts"],
        &["acute", "chronic", "acwr", "acwr_percent", "status"],
        src,
        d.training_load.iter().map(|x: &TrainingLoad| {
            vec![
                i(x.ts),
                or(x.acute),
                or(x.chronic),
                or(x.acwr),
                or(x.acwr_percent),
                oi(x.status),
            ]
        }),
    );
    up!(
        tx,
        "training_status",
        &["ts"],
        &[
            "status",
            "weekly_load_sum",
            "load_min",
            "load_max",
            "fitness_trend",
            "load_trend",
        ],
        src,
        d.training_status.iter().map(|x: &TrainingStatus| {
            vec![
                i(x.ts),
                oi(x.status),
                or(x.weekly_load_sum),
                or(x.load_min),
                or(x.load_max),
                oi(x.fitness_trend),
                oi(x.load_trend),
            ]
        }),
    );
    up!(
        tx,
        "training_balance",
        &["ts"],
        &[
            "aerobic_low",
            "aerobic_high",
            "anaerobic",
            "aerobic_low_target_min",
            "aerobic_low_target_max",
            "aerobic_high_target_min",
            "aerobic_high_target_max",
            "anaerobic_target_min",
            "anaerobic_target_max",
        ],
        src,
        d.training_balance.iter().map(|x: &TrainingBalance| {
            vec![
                i(x.ts),
                or(x.aerobic_low),
                or(x.aerobic_high),
                or(x.anaerobic),
                or(x.aerobic_low_target_min),
                or(x.aerobic_low_target_max),
                or(x.aerobic_high_target_min),
                or(x.aerobic_high_target_max),
                or(x.anaerobic_target_min),
                or(x.anaerobic_target_max),
            ]
        }),
    );
    up!(
        tx,
        "recovery_time",
        &["start_ts"],
        &["minutes"],
        src,
        d.recovery_time
            .iter()
            .map(|x: &RecoveryTime| vec![i(x.start_ts), r(x.minutes)]),
    );

    up!(
        tx,
        "vo2max",
        &["ts"],
        &["maximal_met", "sport"],
        src,
        d.vo2max
            .iter()
            .map(|x: &Vo2max| vec![i(x.ts), r(x.maximal_met), oi(x.sport)])
    );

    up!(
        tx,
        "battery",
        &["ts"],
        &["percent"],
        src,
        d.battery
            .iter()
            .map(|x: &Battery| vec![i(x.ts), r(x.percent)])
    );

    if let Some(m) = &d.mon_info {
        sqlx::query(
            "INSERT OR IGNORE INTO mon_info (sha256, local_offset_s, rmr_kcal_day, steps_goal)
             VALUES (?, ?, ?, ?)",
        )
        .bind(sha256)
        .bind(m.local_offset_s)
        .bind(m.rmr_kcal_day)
        .bind(m.steps_goal)
        .execute(&mut **tx)
        .await?;
    }
    // One row per session in the file (idx = position), replaced with the file.
    for (idx, s) in d.sleep_sessions.iter().enumerate() {
        sqlx::query(
            "INSERT OR IGNORE INTO sleep_sessions (sha256, idx, start_ts, end_ts, overall_score,
                quality_score, duration_score, recovery_score, deep_score, light_score, rem_score,
                awakenings_count, avg_stress) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(sha256)
        .bind(idx as i64)
        .bind(s.start_ts)
        .bind(s.end_ts)
        .bind(s.overall_score)
        .bind(s.quality_score)
        .bind(s.duration_score)
        .bind(s.recovery_score)
        .bind(s.deep_score)
        .bind(s.light_score)
        .bind(s.rem_score)
        .bind(s.awakenings_count)
        .bind(s.avg_stress)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
