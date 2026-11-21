//! Export of one user's data as a zip: the original files and one CSV per kind of data.
//!
//! ```text
//! README.txt              what is in the archive
//! csv/activities.csv      one CSV per kind of data the user has (empty ones are left out)
//! csv/files.csv           every original: its name in the archive, size, when it arrived
//! originals/<sha256>.fit  the user's original files, byte for byte; the name is the stored name
//! ```
//!
//! Only rows of the given user (sample tables by `user_id`, per-file tables through
//! `file_owners`) and only files that user owns. Nothing of the admin or of other users.
//!
//! The zip is written to a non-seekable writer as it is produced: rows are read from the database
//! one at a time and files are copied in 64 KiB pieces, so memory stays flat however much data
//! there is. [`stream_response`] runs [`write_zip`] on a blocking thread and hands its output to
//! the HTTP body through a small bounded channel; a client that reads slowly slows the export
//! down, one that stops for [`IDLE_TIMEOUT`] ends it.
//!
//! Times: each time column becomes two, ISO 8601 in the user's time zone (`2026-11-19T07:42:00+01:00`)
//! and Unix seconds in UTC (`<name>_epoch_s`). A day column is a date (`YYYY-MM-DD`) and the Unix
//! seconds of that day's local midnight (`<name>_start_epoch_s`). An empty cell is "no value"
//! (never 0). Units are in the column names (`distance_m`, `weight_kg`).

use crate::settings::{date_of, date_string, rfc3339};
use anyhow::{Context, Result};
use axum::body::{Body, Bytes};
use chrono_tz::Tz;
use futures_util::StreamExt;
use sqlx::{Row, SqlitePool};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;
use tokio::runtime::Handle;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// A reader that stays away this long ends the export.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// Output is handed to the HTTP body in pieces of this size.
const CHUNK: usize = 64 * 1024;
/// Pieces waiting for a slow reader (`CHUNK` each) before the export pauses.
const QUEUE: usize = 8;

pub struct ExportUser {
    pub id: i64,
    pub username: String,
}

#[derive(Debug, Default)]
pub struct Summary {
    /// CSV files written (kinds of data the user has).
    pub csv_files: usize,
    pub rows: u64,
    /// Original files in the archive.
    pub originals: u64,
    /// Files the database lists but the store does not have (left out, listed in `files.csv`).
    pub originals_missing: u64,
}

// ---- the CSV tables ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Kind {
    Int,
    Real,
    Text,
    /// Unix seconds: ISO time in the user's zone plus `<header>_epoch_s`.
    Time,
    /// Unix seconds of a local midnight: `YYYY-MM-DD` plus `<header>_start_epoch_s`.
    Day,
}

struct Col {
    header: &'static str,
    expr: &'static str,
    kind: Kind,
}

const fn c(header: &'static str, expr: &'static str, kind: Kind) -> Col {
    Col { header, expr, kind }
}

struct Table {
    file: &'static str,
    cols: &'static [Col],
    /// Everything after the select list; `?` is the user id (exactly once).
    from: &'static str,
}

use Kind::{Day, Int, Real, Text, Time};

/// Per-file tables are joined to the user's current files (`activities_current` keeps the best
/// file per activity and the user's own), sample tables are keyed by `user_id`.
static TABLES: &[Table] = &[
    Table {
        file: "activities",
        cols: &[
            c("file", "a.sha256", Text),
            c("origin", "a.origin", Text),
            c("start", "a.start_ts", Time),
            c("sport", "a.sport", Int),
            c("sport_name", "a.sport_name", Text),
            c("sub_sport", "a.sub_sport", Int),
            c("sub_sport_name", "a.sub_sport_name", Text),
            c("sessions", "a.num_sessions", Int),
            c("total_timer_s", "a.total_timer_s", Real),
            c("local_offset_s", "a.local_offset_s", Int),
            c("aerobic_te", "m.aerobic_te", Real),
            c("anaerobic_te", "m.anaerobic_te", Real),
            c("training_load_peak", "m.training_load_peak", Real),
            c("epoc", "m.epoc", Real),
            c("maximal_met", "m.maximal_met", Real),
            c(
                "recovery_time_raw",
                "m.recovery_time_physical_activity_raw",
                Real,
            ),
        ],
        from: "FROM activities_current a LEFT JOIN activity_metrics m ON m.sha256 = a.sha256
               WHERE a.user_id = ? ORDER BY a.start_ts, a.sha256",
    },
    Table {
        file: "activity_sessions",
        cols: &[
            c("file", "s.sha256", Text),
            c("session", "s.idx", Int),
            c("start", "s.start_ts", Time),
            c("sport_name", "s.sport_name", Text),
            c("sub_sport_name", "s.sub_sport_name", Text),
            c("elapsed_s", "s.elapsed_s", Real),
            c("timer_s", "s.timer_s", Real),
            c("distance_m", "s.distance_m", Real),
            c("calories", "s.calories", Real),
            c("avg_hr", "s.avg_hr", Real),
            c("max_hr", "s.max_hr", Real),
            c("avg_speed_mps", "s.avg_speed", Real),
            c("max_speed_mps", "s.max_speed", Real),
            c("ascent_m", "s.ascent_m", Real),
            c("descent_m", "s.descent_m", Real),
            c("avg_cadence", "s.avg_cadence", Real),
            c("aerobic_te", "s.aerobic_te", Real),
            c("anaerobic_te", "s.anaerobic_te", Real),
            c("training_load", "s.training_load", Real),
        ],
        from: "FROM activity_sessions s JOIN activities_current a ON a.sha256 = s.sha256
               WHERE a.user_id = ? ORDER BY a.start_ts, s.sha256, s.idx",
    },
    Table {
        file: "activity_laps",
        cols: &[
            c("file", "l.sha256", Text),
            c("lap", "l.idx", Int),
            c("start", "l.start_ts", Time),
            c("sport_name", "l.sport_name", Text),
            c("sub_sport_name", "l.sub_sport_name", Text),
            c("elapsed_s", "l.elapsed_s", Real),
            c("timer_s", "l.timer_s", Real),
            c("distance_m", "l.distance_m", Real),
            c("calories", "l.calories", Real),
            c("avg_hr", "l.avg_hr", Real),
            c("max_hr", "l.max_hr", Real),
            c("avg_speed_mps", "l.avg_speed", Real),
            c("max_speed_mps", "l.max_speed", Real),
            c("ascent_m", "l.ascent_m", Real),
            c("descent_m", "l.descent_m", Real),
            c("avg_cadence", "l.avg_cadence", Real),
        ],
        from: "FROM activity_laps l JOIN activities_current a ON a.sha256 = l.sha256
               WHERE a.user_id = ? ORDER BY a.start_ts, l.sha256, l.idx",
    },
    Table {
        file: "activity_records",
        cols: &[
            c("file", "r.sha256", Text),
            c("time", "r.ts", Time),
            c("latitude_deg", "r.lat", Real),
            c("longitude_deg", "r.lon", Real),
            c("hr", "r.hr", Real),
            c("speed_mps", "r.speed", Real),
            c("altitude_m", "r.altitude_m", Real),
            c("distance_m", "r.distance_m", Real),
            c("cadence", "r.cadence", Real),
            c("temperature_c", "r.temperature_c", Real),
        ],
        from: "FROM activity_records r JOIN activities_current a ON a.sha256 = r.sha256
               WHERE a.user_id = ? ORDER BY a.start_ts, r.sha256, r.idx",
    },
    Table {
        file: "activity_time_in_zone",
        cols: &[
            c("file", "z.sha256", Text),
            c("idx", "z.idx", Int),
            c("hr_zone_s_json", "z.hr_zone_s", Text),
        ],
        from: "FROM activity_time_in_zone z JOIN activities_current a ON a.sha256 = z.sha256
               WHERE a.user_id = ? ORDER BY a.start_ts, z.sha256, z.idx",
    },
    Table {
        file: "strength_sets",
        cols: &[
            c("file", "t.sha256", Text),
            c("set", "t.idx", Int),
            c("time", "t.ts", Time),
            c("start", "t.start_ts", Time),
            c("duration_s", "t.duration_s", Real),
            c("set_type_name", "t.set_type_name", Text),
            c("reps", "t.reps", Real),
            c("weight_kg", "t.weight_kg", Real),
            c("category", "t.category", Int),
            c("category_name", "t.category_name", Text),
            c("category_subtype", "t.category_subtype", Int),
        ],
        from: "FROM strength_sets t JOIN activities_current a ON a.sha256 = t.sha256
               WHERE a.user_id = ? ORDER BY a.start_ts, t.sha256, t.idx",
    },
    Table {
        file: "exercise_titles",
        cols: &[
            c("file", "e.sha256", Text),
            c("idx", "e.idx", Int),
            c("category", "e.category", Int),
            c("category_name", "e.category_name", Text),
            c("subtype", "e.subtype", Int),
            c("name", "e.name", Text),
        ],
        from: "FROM exercise_titles e JOIN activities_current a ON a.sha256 = e.sha256
               WHERE a.user_id = ? ORDER BY a.start_ts, e.sha256, e.idx",
    },
    Table {
        file: "strength_set_edits",
        cols: &[
            c("activity", "activity_ident", Text),
            c("set", "set_idx", Int),
            c("changes_json", "fields", Text),
            c("updated", "updated_at", Time),
        ],
        from: "FROM strength_set_overrides WHERE user_id = ? ORDER BY activity_ident, set_idx",
    },
    Table {
        file: "workouts",
        cols: &[
            c("name", "name", Text),
            c("sport", "sport", Text),
            c("definition_json", "definition", Text),
            c("send_state", "send_state", Text),
            c("created", "created_at", Time),
            c("updated", "updated_at", Time),
        ],
        from: "FROM workouts WHERE user_id = ? ORDER BY created_at, id",
    },
    Table {
        file: "sleep_sessions",
        cols: &[
            c("file", "s.sha256", Text),
            c("start", "s.start_ts", Time),
            c("end", "s.end_ts", Time),
            c("overall_score", "s.overall_score", Real),
            c("quality_score", "s.quality_score", Real),
            c("duration_score", "s.duration_score", Real),
            c("recovery_score", "s.recovery_score", Real),
            c("deep_score", "s.deep_score", Real),
            c("light_score", "s.light_score", Real),
            c("rem_score", "s.rem_score", Real),
            c("awakenings", "s.awakenings_count", Real),
            c("avg_stress", "s.avg_stress", Real),
            c("asleep_s", "s.asleep_s", Real),
            c("deep_s", "s.deep_s", Real),
            c("light_s", "s.light_s", Real),
            c("rem_s", "s.rem_s", Real),
            c("awake_s", "s.awake_s", Real),
        ],
        from: "FROM sleep_sessions_current s WHERE s.user_id = ? ORDER BY s.start_ts, s.sha256",
    },
    Table {
        file: "sleep_stages",
        cols: &[
            c("time", "ts", Time),
            c("level", "level", Int),
            c("level_name", "level_name", Text),
        ],
        from: "FROM sleep_levels WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "heart_rate",
        cols: &[c("time", "ts", Time), c("bpm", "bpm", Real)],
        from: "FROM mon_hr WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "resting_hr",
        cols: &[
            c("time", "ts", Time),
            c("resting_hr", "rhr", Real),
            c("current_day_resting_hr", "current_day_rhr", Real),
        ],
        from: "FROM resting_hr WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "steps_activity",
        cols: &[
            c("time", "ts", Time),
            c("activity_type", "activity_type", Int),
            c("steps", "steps", Real),
            c("distance_m", "distance_m", Real),
            c("active_calories", "active_calories", Real),
            c("active_time_s", "active_time_s", Real),
            c("duration_min", "duration_min", Real),
            c("intensity", "intensity", Real),
        ],
        from: "FROM mon_activity WHERE user_id = ? ORDER BY ts, activity_type",
    },
    Table {
        file: "intensity_minutes",
        cols: &[
            c("time", "ts", Time),
            c("ascent_total_m", "ascent_total_m", Real),
            c("descent_total_m", "descent_total_m", Real),
            c("moderate_total_min", "moderate_total_min", Real),
            c("vigorous_total_min", "vigorous_total_min", Real),
        ],
        from: "FROM mon_extra WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "daily_totals",
        cols: &[
            c("date", "ts", Day),
            c("steps", "steps", Real),
            c("total_kcal", "total_kcal", Real),
            c("active_kcal", "active_kcal", Real),
            c("resting_kcal", "resting_kcal", Real),
        ],
        from: "FROM daily_totals WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "connect_daily",
        cols: &[
            c("date", "ts", Day),
            c("steps", "steps", Real),
            c("distance_m", "distance_m", Real),
            c("floors_up", "floors_up", Real),
            c("moderate_min", "moderate_min", Real),
            c("vigorous_min", "vigorous_min", Real),
            c("hr_min", "hr_min", Real),
            c("hr_max", "hr_max", Real),
            c("stress_avg", "stress_avg", Real),
            c("body_battery_charged", "bb_charged", Real),
            c("body_battery_drained", "bb_drained", Real),
            c("body_battery_high", "bb_high", Real),
            c("body_battery_low", "bb_low", Real),
            c("respiration_avg_brpm", "resp_avg", Real),
            c("respiration_high_brpm", "resp_high", Real),
            c("respiration_low_brpm", "resp_low", Real),
        ],
        from: "FROM connect_daily WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "stress_body_battery",
        cols: &[
            c("time", "ts", Time),
            c("stress", "stress", Real),
            c("stress_raw", "stress_raw", Real),
            c("body_battery", "body_battery", Real),
            c("body_battery_status", "body_battery_status", Int),
        ],
        from: "FROM stress WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "respiration",
        cols: &[
            c("time", "ts", Time),
            c("brpm", "brpm", Real),
            c("brpm_raw", "brpm_raw", Real),
        ],
        from: "FROM respiration WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "spo2",
        cols: &[
            c("time", "ts", Time),
            c("percent", "percent", Real),
            c("confidence", "confidence", Real),
            c("mode", "mode", Int),
        ],
        from: "FROM spo2 WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "hrv_values",
        cols: &[c("time", "ts", Time), c("rmssd_ms", "rmssd_ms", Real)],
        from: "FROM hrv_values WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "hrv_summary",
        cols: &[
            c("time", "ts", Time),
            c("weekly_avg_ms", "weekly_avg", Real),
            c("last_night_avg_ms", "last_night_avg", Real),
            c("last_night_5min_high_ms", "last_night_5min_high", Real),
            c("baseline_low_upper_ms", "baseline_low_upper", Real),
            c(
                "baseline_balanced_lower_ms",
                "baseline_balanced_lower",
                Real,
            ),
            c(
                "baseline_balanced_upper_ms",
                "baseline_balanced_upper",
                Real,
            ),
            c("status", "status", Int),
        ],
        from: "FROM hrv_summary WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "training_status",
        cols: &[
            c("time", "ts", Time),
            c("status", "status", Int),
            c("weekly_load_sum", "weekly_load_sum", Real),
            c("load_min", "load_min", Real),
            c("load_max", "load_max", Real),
            c("fitness_trend", "fitness_trend", Int),
            c("load_trend", "load_trend", Int),
        ],
        from: "FROM training_status WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "training_load",
        cols: &[
            c("time", "ts", Time),
            c("acute", "acute", Real),
            c("chronic", "chronic", Real),
            c("acwr", "acwr", Real),
            c("acwr_percent", "acwr_percent", Real),
            c("status", "status", Int),
        ],
        from: "FROM training_load WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "training_balance",
        cols: &[
            c("time", "ts", Time),
            c("aerobic_low", "aerobic_low", Real),
            c("aerobic_high", "aerobic_high", Real),
            c("anaerobic", "anaerobic", Real),
            c("aerobic_low_target_min", "aerobic_low_target_min", Real),
            c("aerobic_low_target_max", "aerobic_low_target_max", Real),
            c("aerobic_high_target_min", "aerobic_high_target_min", Real),
            c("aerobic_high_target_max", "aerobic_high_target_max", Real),
            c("anaerobic_target_min", "anaerobic_target_min", Real),
            c("anaerobic_target_max", "anaerobic_target_max", Real),
        ],
        from: "FROM training_balance WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "recovery_time",
        cols: &[c("start", "start_ts", Time), c("minutes", "minutes", Real)],
        from: "FROM recovery_time WHERE user_id = ? ORDER BY start_ts",
    },
    Table {
        file: "vo2max",
        cols: &[
            c("time", "ts", Time),
            c("maximal_met", "maximal_met", Real),
            c("sport", "sport", Int),
        ],
        from: "FROM vo2max WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "body_measurements",
        cols: &[
            c("time", "measured_at", Time),
            c("weight_kg", "weight_kg", Real),
            c("body_fat_pct", "body_fat_pct", Real),
            c("waist_cm", "waist_cm", Real),
            c("entered", "created_at", Time),
        ],
        from: "FROM body_measurements WHERE user_id = ? ORDER BY measured_at, id",
    },
    Table {
        file: "watch_battery",
        cols: &[c("time", "ts", Time), c("percent", "percent", Real)],
        from: "FROM battery WHERE user_id = ? ORDER BY ts",
    },
    Table {
        file: "settings",
        cols: &[c("key", "key", Text), c("value", "value", Text)],
        from: "FROM user_settings WHERE user_id = ? ORDER BY key",
    },
];

// ---- CSV cells --------------------------------------------------------------------------------

/// RFC 4180: quote a cell that holds a comma, quote or line break, or starts or ends with a space.
fn push_text(out: &mut Vec<u8>, s: &str) {
    let plain = !s.contains([',', '"', '\n', '\r']) && s.trim() == s;
    if plain {
        out.extend_from_slice(s.as_bytes());
    } else {
        out.push(b'"');
        out.extend_from_slice(s.replace('"', "\"\"").as_bytes());
        out.push(b'"');
    }
}

fn push_cell(out: &mut Vec<u8>, first: &mut bool) {
    if !std::mem::replace(first, false) {
        out.push(b',');
    }
}

fn push_time(out: &mut Vec<u8>, first: &mut bool, tz: Tz, ts: Option<i64>) {
    push_cell(out, first);
    if let Some(t) = ts {
        out.extend_from_slice(rfc3339(tz, t).as_bytes());
    }
    push_cell(out, first);
    if let Some(t) = ts {
        out.extend_from_slice(t.to_string().as_bytes());
    }
}

fn header_row(cols: &[Col]) -> Vec<u8> {
    let mut out = vec![];
    let mut first = true;
    for col in cols {
        push_cell(&mut out, &mut first);
        out.extend_from_slice(col.header.as_bytes());
        match col.kind {
            Time => {
                push_cell(&mut out, &mut first);
                out.extend_from_slice(format!("{}_epoch_s", col.header).as_bytes());
            }
            Day => {
                push_cell(&mut out, &mut first);
                out.extend_from_slice(format!("{}_start_epoch_s", col.header).as_bytes());
            }
            _ => {}
        }
    }
    out.push(b'\n');
    out
}

fn data_row(row: &sqlx::sqlite::SqliteRow, cols: &[Col], tz: Tz) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(128);
    let mut first = true;
    for (i, col) in cols.iter().enumerate() {
        match col.kind {
            Int => {
                push_cell(&mut out, &mut first);
                if let Some(v) = row.try_get_unchecked::<Option<i64>, _>(i)? {
                    out.extend_from_slice(v.to_string().as_bytes());
                }
            }
            Real => {
                push_cell(&mut out, &mut first);
                if let Some(v) = row.try_get_unchecked::<Option<f64>, _>(i)? {
                    out.extend_from_slice(v.to_string().as_bytes());
                }
            }
            Text => {
                push_cell(&mut out, &mut first);
                if let Some(v) = row.try_get_unchecked::<Option<String>, _>(i)? {
                    push_text(&mut out, &v);
                }
            }
            Time => {
                let ts = row.try_get_unchecked::<Option<i64>, _>(i)?;
                push_time(&mut out, &mut first, tz, ts);
            }
            Day => {
                let ts = row.try_get_unchecked::<Option<i64>, _>(i)?;
                push_cell(&mut out, &mut first);
                if let Some(t) = ts {
                    out.extend_from_slice(date_string(date_of(tz, t)).as_bytes());
                }
                push_cell(&mut out, &mut first);
                if let Some(t) = ts {
                    out.extend_from_slice(t.to_string().as_bytes());
                }
            }
        }
    }
    out.push(b'\n');
    Ok(out)
}

// ---- the archive --------------------------------------------------------------------------------

struct FileRow {
    sha256: String,
    size: i64,
    received_at: String,
    source: String,
    data_type: Option<i64>,
    sub_type: Option<i64>,
    file_number: Option<i64>,
    watch_timestamp: Option<i64>,
    /// Name in the archive; `None` while missing from the store.
    name: Option<String>,
}

async fn list_files(pool: &SqlitePool, uid: i64) -> Result<Vec<FileRow>> {
    let rows = sqlx::query(
        "SELECT f.sha256, f.size, f.received_at, f.source, f.data_type, f.sub_type,
                f.file_number, f.watch_timestamp
         FROM file_owners o JOIN files f ON f.sha256 = o.sha256
         WHERE o.user_id = ? ORDER BY f.received_at, f.sha256",
    )
    .bind(uid)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| FileRow {
            sha256: r.get(0),
            size: r.get(1),
            received_at: r.get(2),
            source: r.get(3),
            data_type: r.get(4),
            sub_type: r.get(5),
            file_number: r.get(6),
            watch_timestamp: r.get(7),
            name: None,
        })
        .collect())
}

struct Exporter<'a, W: Write> {
    rt: &'a Handle,
    pool: &'a SqlitePool,
    tz: Tz,
    zip: ZipWriter<zip::write::StreamWriter<W>>,
    opts: SimpleFileOptions,
    summary: Summary,
}

impl<W: Write> Exporter<'_, W> {
    fn table(&mut self, uid: i64, t: &Table) -> Result<()> {
        let select = t.cols.iter().map(|c| c.expr).collect::<Vec<_>>().join(", ");
        let sql = format!("SELECT {select} {}", t.from);
        let mut rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(uid)
            .fetch(self.pool);
        let rt = self.rt;
        // The first row decides whether the user has this kind of data at all.
        let Some(first) = rt.block_on(rows.next()).transpose()? else {
            return Ok(());
        };
        self.zip
            .start_file(format!("csv/{}.csv", t.file), self.opts)?;
        let mut out = io::BufWriter::with_capacity(CHUNK, &mut self.zip);
        out.write_all(&header_row(t.cols))?;
        out.write_all(&data_row(&first, t.cols, self.tz)?)?;
        let mut n = 1u64;
        while let Some(row) = rt.block_on(rows.next()) {
            out.write_all(&data_row(&row?, t.cols, self.tz)?)?;
            n += 1;
        }
        out.flush()?;
        drop(out);
        self.summary.csv_files += 1;
        self.summary.rows += n;
        Ok(())
    }

    /// Copy the stored file into `originals/`, named by content. `Ok(None)` if the store lacks it.
    fn original(&mut self, data_dir: &Path, sha: &str) -> Result<Option<String>> {
        let path = crate::store::path_for(data_dir, sha);
        let mut f = match File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let mut head = Vec::with_capacity(crate::derive::SNIFF_LEN);
        (&mut f)
            .take(crate::derive::SNIFF_LEN as u64)
            .read_to_end(&mut head)?;
        let ext = if crate::derive::looks_like_fit(&head) {
            ".fit"
        } else if crate::connect::looks_like_connect(&head) {
            ".json"
        } else {
            ""
        };
        let name = format!("originals/{sha}{ext}");
        self.zip.start_file(&name, self.opts)?;
        self.zip.write_all(&head)?;
        io::copy(&mut f, &mut self.zip).with_context(|| format!("copying {}", path.display()))?;
        Ok(Some(name))
    }

    fn files_csv(&mut self, files: &[FileRow]) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        self.zip.start_file("csv/files.csv", self.opts)?;
        let mut out = io::BufWriter::with_capacity(CHUNK, &mut self.zip);
        out.write_all(
            b"name,sha256,size_bytes,received,received_epoch_s,source,data_type,sub_type,file_number,watch_timestamp\n",
        )?;
        for f in files {
            let mut b = vec![];
            let mut first = true;
            push_cell(&mut b, &mut first);
            // A file the store lacks has no name: an empty cell says so.
            if let Some(n) = &f.name {
                push_text(&mut b, n);
            }
            push_cell(&mut b, &mut first);
            b.extend_from_slice(f.sha256.as_bytes());
            push_cell(&mut b, &mut first);
            b.extend_from_slice(f.size.to_string().as_bytes());
            let ts = chrono::DateTime::parse_from_rfc3339(&f.received_at)
                .ok()
                .map(|d| d.timestamp());
            push_time(&mut b, &mut first, self.tz, ts);
            push_cell(&mut b, &mut first);
            push_text(&mut b, &f.source);
            for v in [f.data_type, f.sub_type, f.file_number, f.watch_timestamp] {
                push_cell(&mut b, &mut first);
                if let Some(v) = v {
                    b.extend_from_slice(v.to_string().as_bytes());
                }
            }
            b.push(b'\n');
            out.write_all(&b)?;
        }
        out.flush()?;
        Ok(())
    }

    fn readme(&mut self, user: &ExportUser, now: i64) -> Result<()> {
        let text = format!(
            "Nimrag Connect export for {user}\n\
             Made {when} (time zone {tz}).\n\
             \n\
             originals/   your original files, byte for byte, named by the SHA-256 of their content.\n\
             \x20            .fit is a watch or Garmin export file, .json an imported Garmin Connect record.\n\
             csv/         one CSV file per kind of data you have; kinds with no rows are left out.\n\
             csv/files.csv   every original: its name in this archive (empty if the server no longer\n\
             \x20            held the file: {missing} of them), size, when it arrived and where from.\n\
             \n\
             CSV rules\n\
             - UTF-8, a header row, commas, cells in double quotes when they need it.\n\
             - Every time has two columns: ISO 8601 in your time zone ({tz}), for example\n\
             \x20 2026-11-19T07:42:00+01:00, and the same instant as Unix seconds in UTC (name_epoch_s).\n\
             - A date column (daily_totals, connect_daily) has the date and the Unix seconds of that\n\
             \x20 day's local midnight (date_start_epoch_s).\n\
             - An empty cell means there is no value, never zero. Units are in the column names.\n\
             - The file column of the activity files is the name of the original (without the extension)\n\
             \x20 in originals/. Activities, sessions, laps, records and strength sets join on it.\n\
             - activity_time_in_zone has the seconds in each heart rate zone as a JSON list.\n",
            user = user.username,
            when = rfc3339(self.tz, now),
            tz = self.tz.name(),
            missing = self.summary.originals_missing,
        );
        self.zip.start_file("README.txt", self.opts)?;
        self.zip.write_all(text.as_bytes())?;
        Ok(())
    }
}

/// Write the user's archive to `out`. Blocking: call it from a thread that is not a runtime
/// worker (`spawn_blocking`); `rt` is the runtime whose pool the queries run on.
pub fn write_zip<W: Write>(
    rt: &Handle,
    pool: &SqlitePool,
    data_dir: &Path,
    user: &ExportUser,
    tz: Tz,
    now: i64,
    out: W,
) -> Result<(Summary, W)> {
    let stamp = chrono::DateTime::from_timestamp(now, 0)
        .and_then(|t| {
            zip::DateTime::from_date_and_time(
                chrono::Datelike::year(&t) as u16,
                chrono::Datelike::month(&t) as u8,
                chrono::Datelike::day(&t) as u8,
                chrono::Timelike::hour(&t) as u8,
                chrono::Timelike::minute(&t) as u8,
                chrono::Timelike::second(&t) as u8,
            )
            .ok()
        })
        .unwrap_or_default();
    let opts = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true)
        .last_modified_time(stamp);
    let mut x = Exporter {
        rt,
        pool,
        tz,
        zip: ZipWriter::new_stream(out),
        opts,
        summary: Summary::default(),
    };
    for t in TABLES {
        x.table(user.id, t)?;
    }
    let mut files = rt.block_on(list_files(pool, user.id))?;
    for f in &mut files {
        f.name = x.original(data_dir, &f.sha256)?;
        if f.name.is_some() {
            x.summary.originals += 1;
        } else {
            x.summary.originals_missing += 1;
            tracing::warn!(sha256 = %f.sha256, "export: a file the database lists is not in the store");
        }
    }
    x.files_csv(&files)?;
    x.readme(user, now)?;
    let summary = x.summary;
    let out = x.zip.finish()?.into_inner();
    Ok((summary, out))
}

// ---- streaming it out ------------------------------------------------------------------------------

/// A `Write` that hands its bytes to the HTTP body in `CHUNK`-sized pieces through a bounded
/// channel, waiting when the reader is behind and failing when it has gone.
struct ChannelWriter {
    rt: Handle,
    tx: tokio::sync::mpsc::Sender<io::Result<Bytes>>,
    buf: Vec<u8>,
}

impl ChannelWriter {
    fn send_buf(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let piece = Bytes::from(std::mem::replace(&mut self.buf, Vec::with_capacity(CHUNK)));
        match self
            .rt
            .block_on(tokio::time::timeout(IDLE_TIMEOUT, self.tx.send(Ok(piece))))
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(io::ErrorKind::BrokenPipe.into()),
            Err(_) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the client stopped reading",
            )),
        }
    }
}

impl Write for ChannelWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        if self.buf.len() >= CHUNK {
            self.send_buf()?;
        }
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.send_buf()
    }
}

/// The body of the HTTP response: the archive, produced as it is read. A failure part-way aborts
/// the transfer (the client must not take a cut-off archive for a whole one).
pub fn stream_body(
    pool: SqlitePool,
    data_dir: std::path::PathBuf,
    user: ExportUser,
    tz: Tz,
    now: i64,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Body {
    let (tx, rx) = tokio::sync::mpsc::channel::<io::Result<Bytes>>(QUEUE);
    let rt = Handle::current();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let out = ChannelWriter {
            rt: rt.clone(),
            tx: tx.clone(),
            buf: Vec::with_capacity(CHUNK),
        };
        let result =
            write_zip(&rt, &pool, &data_dir, &user, tz, now, out).and_then(|(summary, mut out)| {
                out.flush()?;
                Ok(summary)
            });
        match result {
            Ok(s) => tracing::info!(
                user_id = user.id,
                csv_files = s.csv_files,
                rows = s.rows,
                originals = s.originals,
                missing = s.originals_missing,
                "export sent"
            ),
            Err(e) => {
                tracing::warn!(user_id = user.id, error = format!("{e:#}"), "export failed");
                let _ = tx.blocking_send(Err(io::Error::other("the export failed")));
            }
        }
    });
    Body::from_stream(futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    }))
}
