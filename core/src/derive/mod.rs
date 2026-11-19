//! Derived, typed rows from decoded FIT files (pure, std-only, no IO).
//!
//! Message-driven: every parser runs over every file and emits rows for the
//! messages it knows; upload metadata is never consulted. Fields are read by
//! name through `fit::interpret` (scaled, subfields and components expanded);
//! parsers never re-implement scaling. Without a generated profile
//! `derive` returns `DeriveError::ProfileUnavailable`: no unscaled guesses.
//! An absent or invalid field is `None`; a row with no usable value is not
//! emitted; nothing is interpolated, differenced or estimated. Timestamps are
//! unix seconds UTC; device-relative timestamps skip the row (counted in
//! `Derived::skipped`).

pub mod activity;
pub mod hrv;
pub mod metrics;
pub mod monitoring;
pub mod sleep;
pub mod test_support;

use crate::fit::interpret::{interpret, Interpreted, Val};
use crate::fit::{profile, FitFile};

/// Seconds between the unix epoch and the FIT epoch (1989-12-31T00:00:00Z).
pub const FIT_EPOCH_UNIX: i64 = 631_065_600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeriveError {
    /// No generated FIT profile is compiled in (see tools/fit-profile/README.md).
    ProfileUnavailable,
}

/// Facts about a file computed from its bytes only (used for source ranking).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileFacts {
    pub serial: Option<i64>,
    pub time_created: Option<i64>,
    pub file_type: Option<i64>,
    /// Latest UTC timestamp of any message.
    pub last_ts: Option<i64>,
    pub msg_count: u64,
}

impl FileFacts {
    /// Combine the facts of the files of a chain: first identity wins, latest
    /// timestamp, summed message counts.
    pub fn merge(&mut self, o: &FileFacts) {
        self.serial = self.serial.or(o.serial);
        self.time_created = self.time_created.or(o.time_created);
        self.file_type = self.file_type.or(o.file_type);
        self.last_ts = self.last_ts.max(o.last_ts);
        self.msg_count += o.msg_count;
    }
}

/// Convert a FIT `date_time` value (seconds since the FIT epoch) to unix seconds
/// UTC. Values below `FIT_UTC_MIN` are device-relative (not UTC) and give `None`.
pub(crate) fn fit_utc(v: f64) -> Option<i64> {
    (v >= f64::from(crate::fit::interpret::FIT_UTC_MIN)).then(|| v as i64 + FIT_EPOCH_UNIX)
}

// ---- wellness row types (filled by monitoring/sleep/hrv/metrics) -----------

#[derive(Debug, Clone, PartialEq)]
pub struct MonHr {
    pub ts: i64,
    pub bpm: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct MonActivity {
    pub ts: i64,
    pub activity_type: i64,
    pub steps: Option<f64>,
    pub distance_m: Option<f64>,
    pub active_calories: Option<f64>,
    pub active_time_s: Option<f64>,
    pub duration_min: Option<f64>,
    pub intensity: Option<f64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct MonExtra {
    pub ts: i64,
    pub ascent_total_m: Option<f64>,
    pub descent_total_m: Option<f64>,
    pub moderate_total_min: Option<f64>,
    pub vigorous_total_min: Option<f64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Stress {
    pub ts: i64,
    pub stress: Option<f64>,
    pub stress_raw: Option<f64>,
    pub body_battery: Option<f64>,
    pub body_battery_status: Option<i64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Respiration {
    pub ts: i64,
    pub brpm: Option<f64>,
    pub brpm_raw: Option<f64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Spo2 {
    pub ts: i64,
    pub percent: Option<f64>,
    pub confidence: Option<f64>,
    pub mode: Option<i64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct RestingHr {
    pub ts: i64,
    pub rhr: Option<f64>,
    pub current_day_rhr: Option<f64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct MonInfo {
    pub local_offset_s: Option<f64>,
    pub rmr_kcal_day: Option<f64>,
    pub steps_goal: Option<f64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SleepLevel {
    pub ts: i64,
    pub level: i64,
    pub level_name: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SleepSession {
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub overall_score: Option<f64>,
    pub quality_score: Option<f64>,
    pub duration_score: Option<f64>,
    pub recovery_score: Option<f64>,
    pub deep_score: Option<f64>,
    pub light_score: Option<f64>,
    pub rem_score: Option<f64>,
    pub awakenings_count: Option<f64>,
    pub avg_stress: Option<f64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct HrvValue {
    pub ts: i64,
    pub rmssd_ms: f64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HrvSummary {
    pub ts: i64,
    pub weekly_avg: Option<f64>,
    pub last_night_avg: Option<f64>,
    pub last_night_5min_high: Option<f64>,
    pub baseline_low_upper: Option<f64>,
    pub baseline_balanced_lower: Option<f64>,
    pub baseline_balanced_upper: Option<f64>,
    pub status: Option<i64>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrainingLoad {
    pub ts: i64,
    pub acute: Option<f64>,
    pub chronic: Option<f64>,
    pub acwr: Option<f64>,
    pub acwr_percent: Option<f64>,
    pub status: Option<i64>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrainingStatus {
    pub ts: i64,
    pub status: Option<i64>,
    pub weekly_load_sum: Option<f64>,
    pub load_min: Option<f64>,
    pub load_max: Option<f64>,
    pub fitness_trend: Option<i64>,
    pub load_trend: Option<i64>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrainingBalance {
    pub ts: i64,
    pub aerobic_low: Option<f64>,
    pub aerobic_high: Option<f64>,
    pub anaerobic: Option<f64>,
    pub aerobic_low_target_min: Option<f64>,
    pub aerobic_low_target_max: Option<f64>,
    pub aerobic_high_target_min: Option<f64>,
    pub aerobic_high_target_max: Option<f64>,
    pub anaerobic_target_min: Option<f64>,
    pub anaerobic_target_max: Option<f64>,
}
/// `maximal_met` as written by the watch (no MET x 3.5 conversion).
#[derive(Debug, Clone, PartialEq)]
pub struct Vo2max {
    pub ts: i64,
    pub maximal_met: f64,
    pub sport: Option<i64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryTime {
    pub start_ts: i64,
    pub minutes: f64,
}

/// Everything derived from one `FitFile`. One Vec per row type.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Derived {
    pub facts: FileFacts,
    /// Rows skipped (for example device-relative timestamps).
    pub skipped: u64,
    pub activity: activity::ActivityRows,
    pub mon_hr: Vec<MonHr>,
    pub mon_activity: Vec<MonActivity>,
    pub mon_extra: Vec<MonExtra>,
    pub stress: Vec<Stress>,
    pub respiration: Vec<Respiration>,
    pub spo2: Vec<Spo2>,
    pub resting_hr: Vec<RestingHr>,
    pub mon_info: Option<MonInfo>,
    pub sleep_levels: Vec<SleepLevel>,
    pub sleep_sessions: Vec<SleepSession>,
    pub hrv_values: Vec<HrvValue>,
    pub hrv_summary: Vec<HrvSummary>,
    pub training_load: Vec<TrainingLoad>,
    pub training_status: Vec<TrainingStatus>,
    pub training_balance: Vec<TrainingBalance>,
    pub recovery_time: Vec<RecoveryTime>,
    pub vo2max: Vec<Vo2max>,
}

pub fn derive(file: &FitFile) -> Result<Derived, DeriveError> {
    if !profile::available() {
        return Err(DeriveError::ProfileUnavailable);
    }
    let msgs = interpret(file);
    let mut d = Derived {
        facts: facts(file, &msgs),
        ..Default::default()
    };
    activity::run(&msgs, &mut d);
    monitoring::run(&msgs, &mut d);
    sleep::run(&msgs, &mut d);
    hrv::run(&msgs, &mut d);
    metrics::run(&msgs, &mut d);
    Ok(d)
}

fn facts(file: &FitFile, msgs: &[Interpreted]) -> FileFacts {
    let id = msgs.iter().find(|m| m.global == 0);
    let get = |name: &str| id.and_then(|m| m.num(name)).map(|x| x as i64);
    FileFacts {
        serial: get("serial_number"),
        time_created: get("time_created").map(|t| t + FIT_EPOCH_UNIX),
        file_type: get("type"),
        last_ts: msgs.iter().filter_map(Interpreted::unix_ts).max(),
        msg_count: file.messages.len() as u64,
    }
}

/// Field access by profile name on interpreted messages.
pub trait MsgExt {
    /// Scalar numeric value of the first field with this name.
    fn num(&self, name: &str) -> Option<f64>;
    /// Enum label of the first field with this name.
    fn label(&self, name: &str) -> Option<&'static str>;
    /// Array value of the first field with this name.
    fn nums(&self, name: &str) -> Option<&[Option<f64>]>;
    /// UTC unix seconds of the message timestamp (None if absent or device-relative).
    fn unix_ts(&self) -> Option<i64>;
}

impl MsgExt for Interpreted {
    fn num(&self, name: &str) -> Option<f64> {
        match self
            .values
            .iter()
            .find(|v| v.name == Some(name))?
            .value
            .as_ref()?
        {
            Val::Number(x) => Some(*x),
            _ => None,
        }
    }
    fn label(&self, name: &str) -> Option<&'static str> {
        self.values
            .iter()
            .find(|v| v.name == Some(name))?
            .enum_label
    }
    fn nums(&self, name: &str) -> Option<&[Option<f64>]> {
        match self
            .values
            .iter()
            .find(|v| v.name == Some(name))?
            .value
            .as_ref()?
        {
            Val::Numbers(x) => Some(x),
            _ => None,
        }
    }
    fn unix_ts(&self) -> Option<i64> {
        self.timestamp.map(|t| i64::from(t) + FIT_EPOCH_UNIX)
    }
}
