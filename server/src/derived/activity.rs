//! Activity persistence: writes `Derived::activity` into the entity tables
//! (keyed by sha256; replaced per file by `clear_file`). Runtime-checked
//! queries only (no `.sqlx` entries needed).
//!
//! A file with several chained FIT files appends: `idx` continues at
//! MAX(idx)+1 of the rows already stored for the sha256, and the one-row tables keep the first.
use anyhow::Result;
use nimrag_core::derive::activity::{Coded, Summary};
use nimrag_core::derive::Derived;
use sqlx::{Sqlite, Transaction};

pub const SAMPLE_TABLES: &[&str] = &[];
pub const ENTITY_TABLES: &[&str] = &[
    "activities",
    "activity_sessions",
    "activity_laps",
    "activity_records",
    "strength_sets",
    "exercise_titles",
    "activity_time_in_zone",
    "activity_metrics",
];

async fn next_idx(tx: &mut Transaction<'_, Sqlite>, table: &str, sha256: &str) -> Result<i64> {
    let sql = format!("SELECT COALESCE(MAX(idx) + 1, 0) FROM {table} WHERE sha256 = ?");
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(sha256)
        .fetch_one(&mut **tx)
        .await?)
}

/// JSON number list; None elements are `null`. Rust's shortest float form.
fn json_floats(v: &[Option<f64>]) -> String {
    let parts: Vec<String> = v
        .iter()
        .map(|x| x.map_or("null".to_string(), |x| format!("{x}")))
        .collect();
    format!("[{}]", parts.join(","))
}

fn json_ints(v: &[i64]) -> Option<String> {
    (!v.is_empty()).then(|| {
        format!(
            "[{}]",
            v.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
        )
    })
}

fn coded(c: &Coded) -> (Option<i64>, Option<&str>) {
    (c.code, c.name.as_deref())
}

async fn write_summary(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    sha256: &str,
    base: i64,
    s: &Summary,
    extra: Option<[Option<f64>; 3]>,
) -> Result<()> {
    let (sport, sport_name) = coded(&s.sport);
    let (sub, sub_name) = coded(&s.sub_sport);
    let extra_cols = if extra.is_some() {
        ", aerobic_te, anaerobic_te, training_load"
    } else {
        ""
    };
    let extra_marks = if extra.is_some() { ", ?, ?, ?" } else { "" };
    let sql = format!(
        "INSERT INTO {table} (sha256, idx, start_ts, sport, sport_name, sub_sport, sub_sport_name,
            elapsed_s, timer_s, distance_m, calories, avg_hr, max_hr, avg_speed, max_speed,
            ascent_m, descent_m, avg_cadence{extra_cols})
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?{extra_marks})"
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(sha256)
        .bind(base + i64::from(s.idx))
        .bind(s.start_ts)
        .bind(sport)
        .bind(sport_name)
        .bind(sub)
        .bind(sub_name)
        .bind(s.elapsed_s)
        .bind(s.timer_s)
        .bind(s.distance_m)
        .bind(s.calories)
        .bind(s.avg_hr)
        .bind(s.max_hr)
        .bind(s.avg_speed)
        .bind(s.max_speed)
        .bind(s.ascent_m)
        .bind(s.descent_m)
        .bind(s.avg_cadence);
    if let Some(e) = extra {
        q = q.bind(e[0]).bind(e[1]).bind(e[2]);
    }
    q.execute(&mut **tx).await?;
    Ok(())
}

pub async fn write(tx: &mut Transaction<'_, Sqlite>, sha256: &str, d: &Derived) -> Result<()> {
    let a = &d.activity;
    if let Some(x) = &a.activity {
        let (sport, sport_name) = coded(&x.sport);
        let (sub, sub_name) = coded(&x.sub_sport);
        sqlx::query(
            "INSERT OR IGNORE INTO activities (sha256, activity_key, serial, time_created, ts,
                start_ts, local_offset_s, total_timer_s, num_sessions, sport, sport_name,
                sub_sport, sub_sport_name)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(sha256)
        .bind(&x.activity_key)
        .bind(x.serial)
        .bind(x.time_created)
        .bind(x.ts)
        .bind(x.start_ts)
        .bind(x.local_offset_s)
        .bind(x.total_timer_s)
        .bind(x.num_sessions)
        .bind(sport)
        .bind(sport_name)
        .bind(sub)
        .bind(sub_name)
        .execute(&mut **tx)
        .await?;
    }
    let base = next_idx(tx, "activity_sessions", sha256).await?;
    for s in &a.sessions {
        let extra = [s.aerobic_te, s.anaerobic_te, s.training_load];
        write_summary(tx, "activity_sessions", sha256, base, &s.s, Some(extra)).await?;
    }
    let base = next_idx(tx, "activity_laps", sha256).await?;
    for l in &a.laps {
        write_summary(tx, "activity_laps", sha256, base, &l.s, None).await?;
    }
    let base = next_idx(tx, "activity_records", sha256).await?;
    for r in &a.records {
        sqlx::query(
            "INSERT INTO activity_records (sha256, idx, ts, lat, lon, hr, speed, altitude_m,
                distance_m, cadence, temperature_c) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(sha256)
        .bind(base + i64::from(r.idx))
        .bind(r.ts)
        .bind(r.lat)
        .bind(r.lon)
        .bind(r.hr)
        .bind(r.speed)
        .bind(r.altitude_m)
        .bind(r.distance_m)
        .bind(r.cadence)
        .bind(r.temperature_c)
        .execute(&mut **tx)
        .await?;
    }
    let base = next_idx(tx, "strength_sets", sha256).await?;
    for s in &a.sets {
        let (set_type, set_type_name) = coded(&s.set_type);
        sqlx::query(
            "INSERT INTO strength_sets (sha256, idx, ts, start_ts, duration_s, set_type,
                set_type_name, reps, weight_kg, category, category_name, category_subtype,
                category_all, category_subtype_all, wkt_step_index)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(sha256)
        .bind(base + i64::from(s.idx))
        .bind(s.ts)
        .bind(s.start_ts)
        .bind(s.duration_s)
        .bind(set_type)
        .bind(set_type_name)
        .bind(s.reps)
        .bind(s.weight_kg)
        .bind(s.category.code)
        .bind(s.category.name.as_deref())
        .bind(s.category_subtype)
        .bind(json_ints(&s.category_all))
        .bind(json_ints(&s.category_subtype_all))
        .bind(s.wkt_step_index)
        .execute(&mut **tx)
        .await?;
    }
    let base = next_idx(tx, "exercise_titles", sha256).await?;
    for t in &a.exercise_titles {
        sqlx::query(
            "INSERT INTO exercise_titles (sha256, idx, category, category_name, subtype, name)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(sha256)
        .bind(base + i64::from(t.idx))
        .bind(t.category.code)
        .bind(t.category.name.as_deref())
        .bind(t.subtype)
        .bind(t.name.as_deref())
        .execute(&mut **tx)
        .await?;
    }
    let base = next_idx(tx, "activity_time_in_zone", sha256).await?;
    for z in &a.time_in_zone {
        sqlx::query(
            "INSERT INTO activity_time_in_zone (sha256, idx, ref_mesg, ref_index, hr_zone_s)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(sha256)
        .bind(base + i64::from(z.idx))
        .bind(z.ref_mesg)
        .bind(z.ref_index)
        .bind(json_floats(&z.hr_zone_s))
        .execute(&mut **tx)
        .await?;
    }
    if let Some(m) = &a.metrics {
        sqlx::query(
            "INSERT OR IGNORE INTO activity_metrics (sha256, maximal_met, aerobic_te,
                anaerobic_te, training_load_peak, epoc, recovery_time_physical_activity_raw)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(sha256)
        .bind(m.maximal_met)
        .bind(m.aerobic_te)
        .bind(m.anaerobic_te)
        .bind(m.training_load_peak)
        .bind(m.epoc)
        .bind(m.recovery_time_physical_activity_raw)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
