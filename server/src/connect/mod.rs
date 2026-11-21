//! Garmin Connect history: JSON pulled from a Garmin Connect account, stored and derived like a
//! FIT file.
//!
//! Each input JSON becomes a content-addressed blob of its own kind. The blob is an envelope
//! around the original text (kept byte for byte inside it):
//!
//! ```text
//! {"nimrag_connect":1,"kind":"sleep","tz":"Europe/Copenhagen","data":<the original JSON>}
//! ```
//!
//! so it is recognised from its bytes alone (like FIT), is stored and ledgered by `ingest`, and
//! is derived again by `reparse` and by an admin's assign, with no other state. The envelope also
//! carries the IANA time zone used to turn Connect's local dates and local times into instants
//! (the importer takes it from the user's `timezone` setting); the same files imported under a
//! different zone are a different blob.
//!
//! An activity's per-second records and time in zone (`records/<id>_<offset>.json`, see
//! [`records`]) are folded into its envelope as a `records` member, so they are derived from the
//! blob's bytes like everything else.
//!
//! Garmin's per-day summaries (`daily_stats/YYYY-MM.json`, kind `daily_stats`) fill the
//! `connect_daily` table; the web API reads it only for a day, and a metric, that the watch has
//! no data for (see `web_api/daily.rs`).
//!
//! Garmin's minute-level series of a day (`intraday/{stress,heart_rate,respiration}/YYYY-MM-DD.json`,
//! kinds `intraday_*`) fill the watch's own sample tables `stress` (stress level and body battery
//! level), `mon_hr` and `respiration`, one row per sample instant, so a day imported from Connect
//! gets the Day view's charts. Where the watch has a row at the same instant its row wins (the
//! rank rule below), and the rest of the day is Connect's.
//!
//! A Connect source ranks below every FIT source (`sources.origin = 'connect'`, rank key
//! starting with `-`), so a watch row wins whatever the import order. Rows that Connect gives
//! only per date sit at the start of that local day, so any reading the watch made later the
//! same day is "the day's value" in the range endpoints. A Connect activity or night is hidden
//! by a FIT one of the same user at the same time (see `migrations/0011_connect.sql`).

pub mod import;
pub mod records;
mod rows;

use crate::derived::{self, activity, upsert_samples, wellness, V};
use anyhow::Result;
use nimrag_core::derive::Derived;
use serde_json::Value;
use sqlx::{Sqlite, Transaction};

/// Sample tables only Connect files fill (the others are the watch's own tables).
pub const SAMPLE_TABLES: &[&str] = &["daily_totals", "connect_daily"];

/// The envelope always starts with this, so a head of [`SNIFF_LEN`] bytes decides.
const MAGIC: &[u8] = br#"{"nimrag_connect":"#;
pub const SNIFF_LEN: usize = MAGIC.len();
/// Bump when the envelope's shape changes.
pub const ENVELOPE_VERSION: u32 = 1;

pub fn looks_like_connect(head: &[u8]) -> bool {
    head.starts_with(MAGIC)
}

/// What a Connect JSON holds. One kind per input file, except an activity list, which is split
/// into one `ActivitySummary` blob per activity so each can be superseded on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Sleep,
    HrvTrend,
    HrvDaily,
    TrainingStatus,
    TrainingLoad,
    Vo2max,
    Stats,
    /// A month of per-day summaries (`daily_stats/YYYY-MM.json`): stress, body battery,
    /// respiration, intensity minutes, heart rate range, floors.
    DailyStats,
    /// One activity: Connect's summary plus the decoded FIT messages, and (when its records were
    /// pulled) the track, series and time in zone.
    Activity,
    /// One entry of Connect's activity list.
    ActivitySummary,
    /// One day of Connect's stress response (`intraday/stress/YYYY-MM-DD.json`): the stress level
    /// and the body battery level every 3 minutes.
    IntradayStress,
    /// One day of Connect's heart rate response (`intraday/heart_rate/YYYY-MM-DD.json`), every
    /// 2 minutes.
    IntradayHeartRate,
    /// One day of Connect's respiration response (`intraday/respiration/YYYY-MM-DD.json`), every
    /// 2 minutes.
    IntradayRespiration,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Sleep => "sleep",
            Kind::HrvTrend => "hrv_trend",
            Kind::HrvDaily => "hrv_daily",
            Kind::TrainingStatus => "training_status",
            Kind::TrainingLoad => "training_load",
            Kind::Vo2max => "vo2max",
            Kind::Stats => "stats",
            Kind::DailyStats => "daily_stats",
            Kind::Activity => "activity",
            Kind::ActivitySummary => "activity_summary",
            Kind::IntradayStress => "intraday_stress",
            Kind::IntradayHeartRate => "intraday_heart_rate",
            Kind::IntradayRespiration => "intraday_respiration",
        }
    }

    /// A day of minute-level samples (the files may carry the account's `userProfilePK`, which
    /// is dropped before the envelope is made).
    pub fn is_intraday(self) -> bool {
        matches!(
            self,
            Kind::IntradayStress | Kind::IntradayHeartRate | Kind::IntradayRespiration
        )
    }

    pub fn parse(s: &str) -> Option<Kind> {
        [
            Kind::Sleep,
            Kind::HrvTrend,
            Kind::HrvDaily,
            Kind::TrainingStatus,
            Kind::TrainingLoad,
            Kind::Vo2max,
            Kind::Stats,
            Kind::DailyStats,
            Kind::Activity,
            Kind::ActivitySummary,
            Kind::IntradayStress,
            Kind::IntradayHeartRate,
            Kind::IntradayRespiration,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }

    /// Which of two Connect sources wins a row both produce: the richer kind (a detailed
    /// activity over its list entry, the dedicated training status over the load history, the
    /// daily HRV with its baselines over the trend).
    fn priority(self) -> u8 {
        match self {
            Kind::TrainingStatus => 3,
            Kind::TrainingLoad | Kind::HrvDaily | Kind::Activity => 2,
            _ => 1,
        }
    }
}

/// The rank key of a Connect source: below every FIT key (`ok:last_ts:msg_count`, which starts
/// with `0` or `1`; `-` sorts before both), then richer kind, later data, more rows.
pub fn rank_key(kind: Kind, last_ts: Option<i64>, rows: u64) -> String {
    format!(
        "-:{}:{:012}:{:012}",
        kind.priority(),
        last_ts.unwrap_or(0).max(0),
        rows
    )
}

/// The original JSON wrapped in its envelope.
pub fn wrap(kind: Kind, tz: chrono_tz::Tz, original: &str) -> Vec<u8> {
    wrap_with_records(kind, tz, original, None)
}

/// The original JSON wrapped in its envelope, plus (an activity only) the `records` member
/// built by [`records::extract`]: the track, the series and the time in zone. Without it the
/// bytes are exactly those of [`wrap`], so an activity imported before records existed keeps its
/// blob and an activity without records files stays the same.
pub fn wrap_with_records(
    kind: Kind,
    tz: chrono_tz::Tz,
    original: &str,
    records: Option<&Value>,
) -> Vec<u8> {
    let extra = records.map_or(String::new(), |r| format!(r#","records":{r}"#));
    format!(
        r#"{{"nimrag_connect":{ENVELOPE_VERSION},"kind":"{}","tz":"{}","data":{}{extra}}}"#,
        kind.as_str(),
        tz.name(),
        original.trim()
    )
    .into_bytes()
}

/// Stage totals of a Connect night (a watch night has stage rows in `sleep_levels` instead).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SleepStages {
    pub asleep_s: Option<f64>,
    pub deep_s: Option<f64>,
    pub light_s: Option<f64>,
    pub rem_s: Option<f64>,
    pub awake_s: Option<f64>,
}

/// Garmin's whole-day figures for one local day.
#[derive(Debug, Clone, PartialEq)]
pub struct DailyTotal {
    /// Start of the local day.
    pub ts: i64,
    pub steps: Option<f64>,
    pub total_kcal: Option<f64>,
    pub active_kcal: Option<f64>,
    pub resting_kcal: Option<f64>,
}

/// Garmin's per-day summary for one local day (`daily_stats`). Every figure is `None` when
/// Connect gave none; a day with no figure at all is not a row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DailyStat {
    /// Start of the local day.
    pub ts: i64,
    pub steps: Option<f64>,
    pub distance_m: Option<f64>,
    pub floors_up: Option<f64>,
    pub moderate_min: Option<f64>,
    pub vigorous_min: Option<f64>,
    pub hr_min: Option<f64>,
    pub hr_max: Option<f64>,
    pub stress_avg: Option<f64>,
    pub bb_charged: Option<f64>,
    pub bb_drained: Option<f64>,
    pub bb_high: Option<f64>,
    pub bb_low: Option<f64>,
    pub resp_avg: Option<f64>,
    pub resp_high: Option<f64>,
    pub resp_low: Option<f64>,
}

/// The rows one envelope derives to.
#[derive(Debug, Default)]
pub struct Parsed {
    pub kind: Option<Kind>,
    /// Rows for the watch's own tables (the same structs a FIT file derives to).
    pub derived: Derived,
    /// One entry per `derived.sleep_sessions`, in order.
    pub sleep_stages: Vec<SleepStages>,
    pub daily_totals: Vec<DailyTotal>,
    pub daily_stats: Vec<DailyStat>,
    /// Latest instant any row refers to (for the rank key).
    pub last_ts: Option<i64>,
    /// Rows produced (for the rank key).
    pub rows: u64,
    /// Entries of the JSON that could not be used.
    pub skipped: u64,
    /// Set when the envelope or its data is unusable; nothing is derived then.
    pub error: Option<String>,
}

/// Derive the rows of an envelope. Never panics on bad input: the error is in `Parsed::error`.
pub fn parse(bytes: &[u8]) -> Parsed {
    #[derive(serde::Deserialize)]
    struct Envelope {
        nimrag_connect: u32,
        kind: String,
        tz: String,
        data: Value,
        /// Only an activity has it, see [`records`].
        #[serde(default)]
        records: Option<Value>,
    }
    let fail = |e: String| Parsed {
        error: Some(e),
        ..Default::default()
    };
    let env: Envelope = match serde_json::from_slice(bytes) {
        Ok(e) => e,
        Err(e) => return fail(format!("not a Connect envelope: {e}")),
    };
    if env.nimrag_connect != ENVELOPE_VERSION {
        return fail(format!(
            "envelope version {} is not supported",
            env.nimrag_connect
        ));
    }
    let Some(kind) = Kind::parse(&env.kind) else {
        return fail(format!("unknown kind {:?}", env.kind));
    };
    let Ok(tz) = env.tz.parse::<chrono_tz::Tz>() else {
        return fail(format!("unknown time zone {:?}", env.tz));
    };
    let mut p = Parsed {
        kind: Some(kind),
        ..Default::default()
    };
    if let Err(e) = rows::fill(kind, &env.data, tz, &mut p) {
        return fail(format!("{}: {e}", kind.as_str()));
    }
    if let (Some(r), Kind::Activity) = (&env.records, kind) {
        if let Err(e) = records::apply(r, &mut p) {
            return fail(format!("{}: {e}", kind.as_str()));
        }
    }
    p.derived.facts.last_ts = p.last_ts;
    p.derived.facts.msg_count = p.rows;
    p
}

/// Replace the derived rows of one Connect file: its `sources` row (rank below any FIT file),
/// the wellness and activity rows through the same writers a FIT file uses, the sleep stage
/// totals and the daily totals.
pub async fn write(tx: &mut Transaction<'_, Sqlite>, sha256: &str, p: &Parsed) -> Result<()> {
    derived::clear_file(tx, sha256).await?;
    if let Some(e) = &p.error {
        tracing::warn!(%sha256, error = %e, "Garmin Connect file not derived");
        return derived::write_file_derive(tx, sha256, "error", Some(e), 0).await;
    }
    let kind = p.kind.unwrap_or(Kind::Stats);
    let src = derived::src_id(sha256);
    let uid: i64 = sqlx::query_scalar("SELECT user_id FROM file_owners WHERE sha256 = ?")
        .bind(sha256)
        .fetch_optional(&mut **tx)
        .await?
        .unwrap_or(0);
    sqlx::query(
        "INSERT INTO sources (sha256, src_id, ok, last_ts, msg_count, serial, time_created,
            file_type, rank_key, origin) VALUES (?, ?, 1, ?, ?, NULL, NULL, NULL, ?, 'connect')",
    )
    .bind(sha256)
    .bind(src)
    .bind(p.last_ts)
    .bind(p.rows as i64)
    .bind(rank_key(kind, p.last_ts, p.rows))
    .execute(&mut **tx)
    .await?;

    wellness::write(tx, sha256, src, uid, &p.derived).await?;
    activity::write(tx, sha256, &p.derived).await?;
    for (idx, st) in p.sleep_stages.iter().enumerate() {
        sqlx::query(
            "UPDATE sleep_sessions SET asleep_s = ?, deep_s = ?, light_s = ?, rem_s = ?, awake_s = ?
             WHERE sha256 = ? AND idx = ?",
        )
        .bind(st.asleep_s)
        .bind(st.deep_s)
        .bind(st.light_s)
        .bind(st.rem_s)
        .bind(st.awake_s)
        .bind(sha256)
        .bind(idx as i64)
        .execute(&mut **tx)
        .await?;
    }
    let totals: Vec<Vec<V>> = p
        .daily_totals
        .iter()
        .map(|d| {
            vec![
                V::I(Some(d.ts)),
                V::R(d.steps),
                V::R(d.total_kcal),
                V::R(d.active_kcal),
                V::R(d.resting_kcal),
            ]
        })
        .collect();
    upsert_samples(
        tx,
        "daily_totals",
        &["ts"],
        &["steps", "total_kcal", "active_kcal", "resting_kcal"],
        src,
        uid,
        totals,
    )
    .await?;
    let days: Vec<Vec<V>> = p
        .daily_stats
        .iter()
        .map(|d| {
            vec![
                V::I(Some(d.ts)),
                V::R(d.steps),
                V::R(d.distance_m),
                V::R(d.floors_up),
                V::R(d.moderate_min),
                V::R(d.vigorous_min),
                V::R(d.hr_min),
                V::R(d.hr_max),
                V::R(d.stress_avg),
                V::R(d.bb_charged),
                V::R(d.bb_drained),
                V::R(d.bb_high),
                V::R(d.bb_low),
                V::R(d.resp_avg),
                V::R(d.resp_high),
                V::R(d.resp_low),
            ]
        })
        .collect();
    upsert_samples(
        tx,
        "connect_daily",
        &["ts"],
        &[
            "steps",
            "distance_m",
            "floors_up",
            "moderate_min",
            "vigorous_min",
            "hr_min",
            "hr_max",
            "stress_avg",
            "bb_charged",
            "bb_drained",
            "bb_high",
            "bb_low",
            "resp_avg",
            "resp_high",
            "resp_low",
        ],
        src,
        uid,
        days,
    )
    .await?;
    let status = if p.skipped > 0 { "partial" } else { "ok" };
    derived::write_file_derive(tx, sha256, status, None, p.skipped).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connect_rank_is_below_every_fit_rank() {
        let lowest_fit = derived::rank_key(false, None, 0);
        assert!(rank_key(Kind::Activity, Some(i64::MAX / 2), 999_999) < lowest_fit);
    }

    #[test]
    fn richer_kinds_and_later_data_rank_higher() {
        assert!(
            rank_key(Kind::TrainingStatus, Some(1), 1) > rank_key(Kind::TrainingLoad, Some(9), 9)
        );
        assert!(rank_key(Kind::Sleep, Some(2), 1) > rank_key(Kind::Sleep, Some(1), 9));
        assert!(rank_key(Kind::Sleep, Some(2), 3) > rank_key(Kind::Sleep, Some(2), 2));
    }

    fn parsed(kind: Kind, original: &str) -> Parsed {
        parse(&wrap(kind, "Europe/Copenhagen".parse().unwrap(), original))
    }

    #[test]
    fn asleep_time_is_the_stage_sum_else_connects_own_total() {
        let p = parsed(
            Kind::Sleep,
            r#"{"nights": [
                {"date": "2026-03-11", "sleep_start": 1000000, "sleep_end": 9000000, "sleep_score": 0,
                 "deep_sleep_seconds": 10, "light_sleep_seconds": 20, "rem_sleep_seconds": 5, "sleep_seconds": 99},
                {"date": "2026-03-12", "sleep_start": 20000000, "sleep_end": 29000000, "sleep_seconds": 99},
                {"date": "2026-03-13", "sleep_start": 5000, "sleep_end": 4000}
            ]}"#,
        );
        assert_eq!(p.error, None);
        assert_eq!(p.skipped, 1, "an end before the start is not a night");
        let asleep: Vec<_> = p.sleep_stages.iter().map(|s| s.asleep_s).collect();
        assert_eq!(asleep, vec![Some(35.0), Some(99.0)]);
        assert_eq!(p.derived.sleep_sessions[0].start_ts, Some(1000));
        assert_eq!(
            p.derived.sleep_sessions[0].overall_score, None,
            "a score of 0 is no score"
        );
    }

    #[test]
    fn a_day_in_progress_or_without_data_is_not_a_total() {
        let p = parsed(
            Kind::Stats,
            r#"{"days": [
                {"date": "2026-03-11", "steps": 5, "has_data": true},
                {"date": "2026-03-12", "steps": 6, "is_partial": true},
                {"date": "2026-03-13", "steps": 7, "has_data": false},
                {"date": "2026-03-14"}
            ]}"#,
        );
        assert_eq!(p.daily_totals.len(), 1);
        assert_eq!(p.daily_totals[0].steps, Some(5.0));
    }

    #[test]
    fn a_daily_summary_keeps_amounts_and_drops_what_was_not_measured() {
        let p = parsed(
            Kind::DailyStats,
            r#"[
                {"date": "2026-03-11", "steps": 0, "moderate_min": 0, "bb_low": 0, "hr_min": 0,
                 "stress_avg": -1, "resp_avg": null, "resp_high": 17.5, "floors_down": 2},
                {"date": "2026-03-12", "steps": null, "hr_max": null},
                {"date": "2026-03-13"},
                {"steps": 5},
                {"date": "x", "steps": 5}
            ]"#,
        );
        assert_eq!(p.error, None);
        assert_eq!(p.skipped, 2, "a day without a valid date is not a day");
        assert_eq!(p.daily_stats.len(), 1, "a day with no figure is no row");
        let d = &p.daily_stats[0];
        let tz: chrono_tz::Tz = "Europe/Copenhagen".parse().unwrap();
        assert_eq!(
            d.ts,
            crate::settings::day_start(tz, "2026-03-11".parse().unwrap())
        );
        assert_eq!(
            (d.steps, d.moderate_min, d.bb_low, d.resp_high),
            (Some(0.0), Some(0.0), Some(0.0), Some(17.5))
        );
        assert_eq!((d.hr_min, d.stress_avg, d.resp_avg), (None, None, None));
        assert_eq!(p.rows, 1);
        let not_a_list = wrap(Kind::DailyStats, tz, "{}");
        assert!(parse(&not_a_list).error.is_some());
    }

    #[test]
    fn a_stress_day_joins_stress_and_body_battery_by_instant_and_reads_columns_by_key() {
        // Columns in another order than Connect's usual one, one more column in the stress list.
        let p = parsed(
            Kind::IntradayStress,
            r#"{"userProfilePK": 1,
            "stressValueDescriptorsDTOList": [{"key": "stressLevel", "index": 1}, {"key": "timestamp", "index": 0}],
            "stressValuesArray": [[60000, 25], [120000, -1], [180000, -2], [240000, null], [300000, 0], [360000, 40], ["x", 1], 5],
            "bodyBatteryValueDescriptorsDTOList": [
                {"bodyBatteryValueDescriptorIndex": 2, "bodyBatteryValueDescriptorKey": "timestamp"},
                {"bodyBatteryValueDescriptorIndex": 0, "bodyBatteryValueDescriptorKey": "bodyBatteryLevel"},
                {"bodyBatteryValueDescriptorIndex": 1, "bodyBatteryValueDescriptorKey": "bodyBatteryStatus"}],
            "bodyBatteryValuesArray": [[70, "MEASURED", 60000], [null, "MEASURED", 120000], [65, "MEASURED", 420000.5]]}"#,
        );
        assert_eq!(p.error, None);
        // The "x" and the 5 have no instant.
        assert_eq!(p.skipped, 2);
        let rows: Vec<_> = p
            .derived
            .stress
            .iter()
            .map(|r| {
                (
                    r.ts,
                    r.stress,
                    r.stress_raw,
                    r.body_battery,
                    r.body_battery_status,
                )
            })
            .collect();
        assert_eq!(
            rows,
            vec![
                (60, Some(25.0), None, Some(70.0), None),
                (180, None, Some(-2.0), None, None), // too much motion: the activity reading
                (300, Some(0.0), None, None, None),  // 0 is a stress value
                (360, Some(40.0), None, None, None),
                (420, None, None, Some(65.0), None),
            ],
            "-1 and null are no stress value; a null level is no body battery"
        );
        assert_eq!(p.rows, 5);
        assert_eq!(p.last_ts, Some(420));
    }

    #[test]
    fn a_stress_day_without_body_battery_and_one_without_the_stress_columns() {
        let p = parsed(
            Kind::IntradayStress,
            r#"{"stressValueDescriptorsDTOList": [{"key": "timestamp", "index": 0}, {"key": "stressLevel", "index": 1}],
                "stressValuesArray": [[1000, 10]]}"#,
        );
        assert_eq!(p.error, None);
        assert_eq!(p.derived.stress.len(), 1);
        assert_eq!(p.derived.stress[0].body_battery, None);
        // The descriptors do not name the level column: nothing is guessed from positions.
        let p = parsed(
            Kind::IntradayStress,
            r#"{"stressValueDescriptorsDTOList": [{"key": "timestamp", "index": 0}, {"key": "other", "index": 1}],
                "stressValuesArray": [[1000, 10]]}"#,
        );
        assert!(p.error.is_some() && p.derived.stress.is_empty());
        // A body battery array without its descriptors is an error too.
        let p = parsed(
            Kind::IntradayStress,
            r#"{"stressValueDescriptorsDTOList": [{"key": "timestamp", "index": 0}, {"key": "stressLevel", "index": 1}],
                "stressValuesArray": [[1000, 10]], "bodyBatteryValuesArray": [[1000, "MEASURED", 5]]}"#,
        );
        assert!(p.error.is_some());
    }

    #[test]
    fn heart_rate_and_breathing_days_keep_only_real_readings_one_per_second() {
        let p = parsed(
            Kind::IntradayHeartRate,
            r#"{"heartRateValueDescriptors": [{"key": "timestamp", "index": 0}, {"key": "heartrate", "index": 1}],
                "heartRateValues": [[120000, 61], [240000, null], [360000, 0], [480000, 70], [480001, 99], [600000, 72.5]]}"#,
        );
        assert_eq!(p.error, None);
        let rows: Vec<_> = p.derived.mon_hr.iter().map(|r| (r.ts, r.bpm)).collect();
        assert_eq!(rows, vec![(120, 61.0), (480, 70.0), (600, 72.5)]);
        assert_eq!((p.rows, p.skipped), (3, 0));

        let p = parsed(
            Kind::IntradayRespiration,
            r#"{"respirationValueDescriptorsDTOList": [{"key": "timestamp", "index": 0}, {"key": "respiration", "index": 1}],
                "respirationValuesArray": [[120000, 14.0], [240000, -1.0], [360000, 15.5]]}"#,
        );
        assert_eq!(p.error, None);
        let rows: Vec<_> = p
            .derived
            .respiration
            .iter()
            .map(|r| (r.ts, r.brpm, r.brpm_raw))
            .collect();
        assert_eq!(rows, vec![(120, Some(14.0), None), (360, Some(15.5), None)]);
        // Not a list of values.
        let p = parsed(
            Kind::IntradayRespiration,
            r#"{"respirationValueDescriptorsDTOList": [{"key": "timestamp", "index": 0}, {"key": "respiration", "index": 1}],
                "respirationValuesArray": 3}"#,
        );
        assert!(p.error.is_some());
    }

    #[test]
    fn a_broken_envelope_derives_nothing_and_never_panics() {
        for bad in [
            &b"{\"nimrag_connect\":1,\"kind\":\"nope\",\"tz\":\"UTC\",\"data\":{}}"[..],
            b"{\"nimrag_connect\":1,\"kind\":\"sleep\",\"tz\":\"Nowhere/Land\",\"data\":{}}",
            b"{\"nimrag_connect\":1,\"kind\":\"sleep\",\"tz\":\"UTC\",\"data\":[]}",
            b"{\"nimrag_connect\":1,\"kind\":\"stats\",\"tz\":\"UTC\"}",
            b"",
        ] {
            let p = parse(bad);
            assert!(p.error.is_some(), "{}", String::from_utf8_lossy(bad));
            assert!(p.derived.sleep_sessions.is_empty() && p.daily_totals.is_empty());
        }
    }

    #[test]
    fn the_envelope_keeps_the_original_text() {
        let tz: chrono_tz::Tz = "Europe/Copenhagen".parse().unwrap();
        let original = "{\n  \"nights\": [ ]\n}\n";
        let bytes = wrap(Kind::Sleep, tz, original);
        assert!(looks_like_connect(&bytes));
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains(original.trim()));
        let p = parse(&bytes);
        assert_eq!(p.error, None);
        assert_eq!(p.kind, Some(Kind::Sleep));
        assert!(parse(b"{\"nimrag_connect\":2}").error.is_some());
    }
}
