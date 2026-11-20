//! Garmin Connect history (`import-connect`) through the real binary and router, on SYNTHETIC
//! JSON (made-up numbers, the shapes the Garmin Connect connector returns). Row-level checks that
//! need a FIT file (watch rows, activities) pass vacuously without the generated FIT profile.
mod common;

use axum::http::{Method, StatusCode};
use common::*;
use nimrag_core::derive::test_support::{FitBuilder, Fv};
use nimrag_core::fit::profile;
use nimrag_server::users::Role;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::process::{Command, Output};

const TZ: &str = "Europe/Copenhagen";
/// FIT time 0 in unix seconds.
const FIT_EPOCH: i64 = 631_065_600;

fn now() -> i64 {
    cph(2026, 3, 20, 12, 0)
}

fn write_json(dir: &Path, name: &str, v: &Value) {
    let p = dir.join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, serde_json::to_string_pretty(v).unwrap()).unwrap();
}

fn night(date: &str, start: i64, end: i64, score: i64, stages: [i64; 4]) -> Value {
    json!({
        "date": date,
        "sleep_start": start * 1000,
        "sleep_end": end * 1000,
        "sleep_score": score,
        "deep_sleep_seconds": stages[0],
        "light_sleep_seconds": stages[1],
        "rem_sleep_seconds": stages[2],
        "awake_seconds": stages[3],
        "awake_count": 2,
        "avg_sleep_stress": 18.5,
        "resting_heart_rate_bpm": 51,
    })
}

fn msg(kind: &str, fields: &[(&str, Value)]) -> Value {
    json!({
        "type": kind,
        "fields": fields.iter().map(|(n, v)| json!({"name": n, "value": v})).collect::<Vec<_>>(),
    })
}

/// Everything the importer knows, with made-up numbers: two nights (local 22:30 -> 06:30 and
/// 23:00 -> 07:00 in Copenhagen), HRV with baselines, training status, VO2 max, whole-day
/// totals, an activity list with one run and one detailed strength session.
fn fixtures() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write_json(
        d,
        "sleep/nights.json",
        &json!({"nights": [
            night("2026-03-11", cph(2026, 3, 10, 22, 30), cph(2026, 3, 11, 6, 30), 80, [5400, 16800, 6000, 600]),
            night("2026-03-12", cph(2026, 3, 11, 23, 0), cph(2026, 3, 12, 7, 0), 70, [3600, 18000, 5400, 1800]),
        ]}),
    );
    write_json(
        d,
        "hrv_daily.json",
        &json!({"2026-03-11": {
            "date": "2026-03-11", "weekly_avg_hrv_ms": 41, "last_night_avg_hrv_ms": 44,
            "last_night_5min_high_hrv_ms": 70, "baseline_low_upper_ms": 30,
            "baseline_balanced_low_ms": 35, "baseline_balanced_upper_ms": 48, "status": "BALANCED"
        }}),
    );
    write_json(
        d,
        "training_status.json",
        &json!({"2026-03-11": {
            "date": "2026-03-11", "training_status": 7, "fitness_trend": 1, "acute_load": 300,
            "chronic_load": 280, "load_ratio": 1.1, "acwr_percent": 40,
            "optimal_chronic_load_min": 224, "optimal_chronic_load_max": 420,
            "monthly_load_aerobic_low": 500, "monthly_load_aerobic_high": 600,
            "monthly_load_anaerobic": 120
        }}),
    );
    write_json(
        d,
        "load_trend.json",
        &json!({"trend": [
            {"date": "2026-03-09", "atl": 250, "ctl": 240, "acwr": 1.0, "acwr_percent": 35,
             "optimal_chronic_load_min": 190, "optimal_chronic_load_max": 360,
             "training_status_code": 4, "fitness_trend": 1},
        ]}),
    );
    write_json(
        d,
        "vo2max.json",
        &json!({"sport": "running", "trend": [
            {"date": "2026-03-11", "vo2_max": 52.5, "carried_forward": false},
            {"date": "2026-03-12", "vo2_max": 52.5, "carried_forward": true},
        ]}),
    );
    write_json(
        d,
        "stats.json",
        &json!({"days": [
            {"date": "2026-03-11", "steps": 9000, "total_calories": 2500, "active_calories": 500,
             "resting_calories": 2000, "has_data": true, "is_partial": false},
            {"date": "2026-03-12", "steps": 100, "has_data": true, "is_partial": true},
            {"date": "2026-03-13", "has_data": false},
        ]}),
    );
    write_json(
        d,
        "activities.json",
        &json!({"count": 1, "activities": [{
            "id": 1001, "type": "running", "start_time": "2026-03-11 17:30:00",
            "duration_seconds": 1800, "distance_meters": 5000, "calories": 350,
            "avg_hr_bpm": 150, "max_hr_bpm": 170, "elevation_gain_meters": 20
        }]}),
    );
    write_json(
        d,
        "activity_1002.json",
        &json!({
            "summary": {
                "id": 1002, "type": "strength_training",
                "start_time_gmt": "2026-03-12 16:00:00", "start_time_local": "2026-03-12 17:00:00",
                "duration_seconds": 2400, "calories": 250, "avg_hr_bpm": 110, "max_hr_bpm": 150,
                "training_effect": 2.5, "training_load": 55
            },
            "fit": {"messages": [
                msg("session", &[
                    ("start_time", json!("2026-03-12T16:00:00")),
                    ("timestamp", json!("2026-03-12T16:40:00")),
                    ("sport", json!("training")), ("sub_sport", json!("strength_training")),
                    ("total_timer_time", json!(2400)), ("total_elapsed_time", json!(2400)),
                ]),
                msg("lap", &[
                    ("start_time", json!("2026-03-12T16:00:00")),
                    ("total_timer_time", json!(2400)),
                ]),
                msg("set", &[
                    ("timestamp", json!("2026-03-12T16:10:00")),
                    ("start_time", json!("2026-03-12T16:09:00")),
                    ("duration", json!(40)), ("set_type", json!("active")),
                    ("repetitions", json!(8)), ("weight", json!(40.0)),
                    ("category", json!(["bench_press"])),
                ]),
                msg("set", &[
                    ("timestamp", json!("2026-03-12T16:11:00")),
                    ("duration", json!(60)), ("set_type", json!("rest")),
                ]),
            ]}
        }),
    );
    dir
}

fn run_connect(data: &Path, input: &Path, user: &str, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .arg("import-connect")
        .args(["--user", user])
        .args(extra)
        .arg("--data-dir")
        .arg(data)
        .arg(input)
        .output()
        .unwrap()
}

fn import(e: &Env, input: &Path) -> String {
    let out = run_connect(e.dir.path(), input, USER, &["--timezone", TZ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn reparse(dir: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .args(["reparse", "--data-dir"])
        .arg(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The pages show local days: give a user the zone the fixtures were made in.
async fn set_zone(e: &Env, uid: i64) {
    nimrag_server::settings::set_user(&e.pool, uid, "timezone", TZ)
        .await
        .unwrap();
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Every table a Connect file fills, plus ledger and source rows, in deterministic order.
const TABLES: &[&str] = &[
    "files",
    "file_owners",
    "file_derive",
    "sources",
    "sleep_sessions",
    "resting_hr",
    "hrv_summary",
    "training_status",
    "training_load",
    "training_balance",
    "vo2max",
    "daily_totals",
    "activities",
    "activity_sessions",
    "activity_laps",
    "activity_records",
    "activity_time_in_zone",
    "strength_sets",
    "exercise_titles",
    "activity_metrics",
];

async fn dump(pool: &SqlitePool) -> String {
    let mut out = String::new();
    for t in TABLES {
        let cols: Vec<String> = sqlx::query(sqlx::AssertSqlSafe(format!("PRAGMA table_info({t})")))
            .fetch_all(pool)
            .await
            .unwrap()
            .iter()
            .map(|r| r.get::<String, _>("name"))
            .collect();
        let expr = cols
            .iter()
            .map(|c| format!("quote({c})"))
            .collect::<Vec<_>>()
            .join(" || '|' || ");
        let sql = format!("SELECT {expr} FROM {t} ORDER BY {expr}");
        for r in sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(pool)
            .await
            .unwrap()
        {
            out.push_str(&format!("{t}|{}\n", r.get::<String, _>(0)));
        }
    }
    out
}

fn day<'a>(j: &'a Value, date: &str) -> &'a Value {
    j["days"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["date"] == date)
        .unwrap_or_else(|| panic!("no day {date}"))
}

// ---- the import ---------------------------------------------------------------------------

#[tokio::test]
async fn import_fills_the_history_tables_and_the_pages_read_them() {
    let e = env_at(now()).await;
    let input = fixtures();
    let out = import(&e, input.path());
    assert!(out.contains("JSON files read:      8"), "{out}");
    // The activity list is split into one blob per activity: 8 files -> 8 blobs.
    assert!(out.contains("blobs stored new:     8"), "{out}");
    assert!(out.contains("errors:               0"), "{out}");

    // Ledger: stored as 'connect' blobs, owned by the user.
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM files WHERE source = 'connect'"
        )
        .await,
        8
    );
    assert_eq!(
        count(
            &e.pool,
            &format!(
                "SELECT COUNT(*) FROM file_owners WHERE user_id = {}",
                e.user_id
            )
        )
        .await,
        8
    );
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM sources WHERE origin = 'connect'"
        )
        .await,
        8
    );

    // Sleep: instants are UTC milliseconds, the stage totals are kept, and time asleep is
    // deep + light + REM.
    type Night = (i64, i64, f64, f64, f64, f64, f64, f64);
    let rows: Vec<Night> = sqlx::query_as(
        "SELECT start_ts, end_ts, overall_score, asleep_s, deep_s, light_s, rem_s, awake_s
         FROM sleep_sessions_current ORDER BY start_ts",
    )
    .fetch_all(&e.pool)
    .await
    .unwrap();
    assert_eq!(
        rows[0],
        (
            cph(2026, 3, 10, 22, 30),
            cph(2026, 3, 11, 6, 30),
            80.0,
            28_200.0,
            5400.0,
            16_800.0,
            6000.0,
            600.0
        )
    );
    assert_eq!(rows.len(), 2);

    // Date-only rows sit at the start of the local day.
    let day_start = cph(2026, 3, 11, 0, 0);
    let (weekly, low, bal_low, bal_up, status): (f64, f64, f64, f64, i64) = sqlx::query_as(
        "SELECT weekly_avg, baseline_low_upper, baseline_balanced_lower, baseline_balanced_upper, status
         FROM hrv_summary WHERE ts = ?",
    )
    .bind(day_start)
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(
        (weekly, low, bal_low, bal_up, status),
        (41.0, 30.0, 35.0, 48.0, 4)
    );
    assert_eq!(
        count(
            &e.pool,
            &format!("SELECT COUNT(*) FROM resting_hr WHERE ts = {day_start}")
        )
        .await,
        1
    );
    let met: f64 = sqlx::query_scalar("SELECT maximal_met FROM vo2max")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(
        met, 15.0,
        "52.5 ml/kg/min is 15 MET; the carried-forward day is not a measurement"
    );
    assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM vo2max").await, 1);
    // A partial day and an empty day are not whole-day totals.
    assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM daily_totals").await, 1);
    // The status file and the load trend both give a status for their day.
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM training_status").await,
        2
    );

    // Activities: the list entry's local start, the detailed one's UTC start.
    let starts: Vec<(String, i64, Option<i64>, String)> = sqlx::query_as(
        "SELECT activity_key, start_ts, local_offset_s, sport_name FROM activities_current ORDER BY start_ts",
    )
    .fetch_all(&e.pool)
    .await
    .unwrap();
    assert_eq!(
        starts,
        vec![
            (
                "connect:1001".into(),
                cph(2026, 3, 11, 17, 30),
                None,
                "running".into()
            ),
            (
                "connect:1002".into(),
                cph(2026, 3, 12, 17, 0),
                Some(3600),
                "training".into()
            ),
        ]
    );
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM activity_laps").await,
        1
    );
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM strength_sets").await,
        2
    );

    // The pages.
    set_zone(&e, e.user_id).await;
    let user = e.login().await;
    let s = e
        .get(&user, "/api/sleep/history?from=2026-03-01&to=2026-03-19")
        .await;
    assert_eq!(s["stats"]["nights"], 2);
    assert_eq!(s["stats"]["avg_score"], 75.0);
    assert_eq!(
        s["stats"]["avg_duration_s"], 27_600.0,
        "time asleep, not the time in bed"
    );
    let d = e.get(&user, "/api/days/2026-03-10").await;
    let n = &d["sleep"];
    assert_eq!(
        (
            &n["duration_s"],
            &n["deep_s"],
            &n["light_s"],
            &n["rem_s"],
            &n["awake_s"],
            &n["score"]
        ),
        (
            &json!(28_200.0),
            &json!(5400.0),
            &json!(16_800.0),
            &json!(6000.0),
            &json!(600.0),
            &json!(80.0)
        )
    );
    assert_eq!(n["start"], "2026-03-10T22:30:00+01:00");
    assert_eq!(n["end"], "2026-03-11T06:30:00+01:00");
    // No minute data that day: steps come from the whole-day totals.
    assert_eq!(e.get(&user, "/api/days/2026-03-11").await["steps"], 9000.0);
    let hrv = e.get(&user, "/api/hrv?from=2026-03-05&to=2026-03-15").await;
    let h = day(&hrv, "2026-03-11");
    assert_eq!(h["last_night_ms"], 44.0);
    assert_eq!(h["baseline_low_ms"], 35.0);
    assert_eq!(h["baseline_high_ms"], 48.0);
    let ts = e
        .get(&user, "/api/training/status?from=2026-03-09&to=2026-03-12")
        .await;
    assert_eq!(day(&ts, "2026-03-11")["status"]["label"], "Productive");
    let acts = e.get(&user, "/api/activities").await;
    let kinds: Vec<&str> = acts["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["strength", "run"]);
}

#[tokio::test]
async fn a_rerun_changes_nothing_and_unknown_json_is_not_stored() {
    let e = env_at(now()).await;
    let input = fixtures();
    write_json(input.path(), "notes.json", &json!({"hello": "world"}));
    std::fs::write(input.path().join("broken.json"), "{ not json").unwrap();
    let first = import(&e, input.path());
    assert!(
        first.contains("not Connect JSON (not stored): 2"),
        "{first}"
    );
    let before = dump(&e.pool).await;
    assert!(!before.is_empty());

    let second = import(&e, input.path());
    assert!(second.contains("blobs stored new:     0"), "{second}");
    assert!(second.contains("blobs already there:  8"), "{second}");
    assert_eq!(
        dump(&e.pool).await,
        before,
        "nothing is duplicated or rewritten"
    );
    assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM files").await, 8);
}

#[tokio::test]
async fn local_dates_are_read_in_the_users_zone_unless_one_is_given() {
    let e = env_at(now()).await;
    nimrag_server::settings::set_user(&e.pool, e.user_id, "timezone", "Asia/Tokyo")
        .await
        .unwrap();
    let input = fixtures();
    let out = run_connect(e.dir.path(), input.path(), USER, &[]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("time zone:            Asia/Tokyo"));
    use chrono::TimeZone;
    let tokyo = chrono_tz::Asia::Tokyo
        .with_ymd_and_hms(2026, 3, 11, 0, 0, 0)
        .unwrap()
        .timestamp();
    assert_eq!(
        count(
            &e.pool,
            &format!("SELECT COUNT(*) FROM hrv_summary WHERE ts = {tokyo}")
        )
        .await,
        1
    );
    // Sleep instants are absolute: the zone does not move them.
    assert_eq!(
        count(
            &e.pool,
            &format!(
                "SELECT COUNT(*) FROM sleep_sessions WHERE start_ts = {}",
                cph(2026, 3, 10, 22, 30)
            )
        )
        .await,
        1
    );
}

#[tokio::test]
async fn the_command_needs_a_normal_user() {
    let e = env_at(now()).await;
    e.add_user("boss", Role::Admin).await;
    let input = fixtures();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
            .arg("import-connect")
            .args(args)
            .arg("--data-dir")
            .arg(e.dir.path())
            .arg(input.path())
            .output()
            .unwrap();
        assert!(!out.status.success(), "{args:?} must fail");
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    assert!(run(&[]).contains("--user"));
    assert!(run(&["--user", "nobody"]).contains("nobody"));
    assert!(run(&["--user", "boss"]).contains("admin"));
    assert!(run(&["--user", USER, "--timezone", "Mars/Olympus"]).contains("time zone"));
    assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM files").await, 0);
}

// ---- survival -------------------------------------------------------------------------------

#[tokio::test]
async fn reparse_gives_back_the_same_rows() {
    let e = env_at(now()).await;
    import(&e, fixtures().path());
    let before = dump(&e.pool).await;
    let out = reparse(e.dir.path());
    assert!(out.contains("8 Garmin Connect inputs"), "{out}");
    assert_eq!(dump(&e.pool).await, before);
}

#[tokio::test]
async fn deleting_the_user_and_assigning_the_files_moves_the_history() {
    let e = env_at(now()).await;
    e.adopt.store(false, std::sync::atomic::Ordering::Relaxed);
    let admin = {
        e.add_user("boss", Role::Admin).await;
        e.login_as("boss").await
    };
    let other = e.add_user("other", Role::User).await;
    import(&e, fixtures().path());
    let sleep_rows = "SELECT COUNT(*) FROM sleep_sessions_current";
    assert_eq!(
        count(
            &e.pool,
            &format!("{sleep_rows} WHERE user_id = {}", e.user_id)
        )
        .await,
        2
    );

    let (s, _) = e
        .send(
            &admin,
            Method::DELETE,
            &format!("/api/admin/users/{}?confirm=true", e.user_id),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    // The files stay, owned by nobody; so do the rows derived from them.
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM files WHERE source = 'connect'"
        )
        .await,
        8
    );
    assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM file_owners").await, 0);
    assert_eq!(
        count(&e.pool, &format!("{sleep_rows} WHERE user_id = 0")).await,
        2
    );
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM daily_totals WHERE user_id = 0"
        )
        .await,
        1
    );

    let (s, j) = e
        .send(
            &admin,
            Method::POST,
            "/api/admin/unassigned/assign",
            json!({ "token_ids": [], "user_id": other, "include_unattributed": true }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["files_moved"], 8);
    for t in [
        "sleep_sessions_current",
        "daily_totals",
        "hrv_summary",
        "training_status",
        "vo2max",
        "activities_current",
    ] {
        assert_eq!(
            count(
                &e.pool,
                &format!("SELECT COUNT(*) FROM {t} WHERE user_id = {other}")
            )
            .await,
            count(&e.pool, &format!("SELECT COUNT(*) FROM {t}")).await,
            "{t}"
        );
        assert!(
            count(&e.pool, &format!("SELECT COUNT(*) FROM {t}")).await > 0,
            "{t}"
        );
    }
    set_zone(&e, other).await;
    let cookie = e.login_as("other").await;
    let s = e
        .get(&cookie, "/api/sleep/history?from=2026-03-01&to=2026-03-19")
        .await;
    assert_eq!(s["stats"]["nights"], 2);
    assert_eq!(
        e.get(&cookie, "/api/days/2026-03-11").await["steps"],
        9000.0
    );
}

#[tokio::test]
async fn another_user_sees_none_of_it() {
    let e = env_at(now()).await;
    e.adopt.store(false, std::sync::atomic::Ordering::Relaxed);
    let other = e.add_user("other", Role::User).await;
    import(&e, fixtures().path());
    set_zone(&e, other).await;
    let cookie = e.login_as("other").await;
    let s = e
        .get(&cookie, "/api/sleep/history?from=2026-03-01&to=2026-03-19")
        .await;
    assert_eq!(s["stats"]["nights"], 0);
    let ts = e
        .get(
            &cookie,
            "/api/training/status?from=2026-03-09&to=2026-03-12",
        )
        .await;
    assert!(ts["days"]
        .as_array()
        .unwrap()
        .iter()
        .all(|d| d["status"].is_null()));
    assert_eq!(
        e.get(&cookie, "/api/activities").await["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_ne!(
        e.get(&cookie, "/api/days/2026-03-11").await["steps"],
        9000.0
    );
    let hrv = e
        .get(&cookie, "/api/hrv?from=2026-03-05&to=2026-03-15")
        .await;
    assert!(day(&hrv, "2026-03-11")["last_night_ms"].is_null());
    for t in [
        "sleep_sessions_current",
        "daily_totals",
        "hrv_summary",
        "resting_hr",
        "vo2max",
        "activities_current",
    ] {
        assert_eq!(
            count(
                &e.pool,
                &format!("SELECT COUNT(*) FROM {t} WHERE user_id = {other}")
            )
            .await,
            0,
            "{t}"
        );
    }
}

// ---- the watch wins ---------------------------------------------------------------------------

/// A watch HRV summary at the very instant the Connect one for 2026-03-11 sits at.
fn fit_hrv() -> Vec<u8> {
    FitBuilder::new()
        .message(
            370,
            &[
                (253, Fv::U32((cph(2026, 3, 11, 0, 0) - FIT_EPOCH) as u32)),
                (0, Fv::U16(6400)),
            ],
        )
        .build()
}

/// A watch night overlapping the first Connect night.
fn fit_night() -> Vec<u8> {
    FitBuilder::new()
        .message(
            411,
            &[
                (10, Fv::U32((cph(2026, 3, 10, 22, 45) - FIT_EPOCH) as u32)),
                (8, Fv::U32((cph(2026, 3, 11, 6, 15) - FIT_EPOCH) as u32)),
            ],
        )
        .build()
}

/// A watch run starting `offset_s` after the Connect run of the list (17:30 local).
fn fit_run(offset_s: i64, serial: u32) -> Vec<u8> {
    let t = (cph(2026, 3, 11, 17, 30) + offset_s - FIT_EPOCH) as u32;
    FitBuilder::new()
        .message(0, &[(3, Fv::U32(serial)), (4, Fv::U32(t))])
        .message(20, &[(253, Fv::U32(t)), (3, Fv::U8(120)), (5, Fv::U32(0))])
        .message(
            18,
            &[
                (253, Fv::U32(t + 1800)),
                (2, Fv::U32(t)),
                (5, Fv::U8(1)),
                (7, Fv::U32(1_800_000)),
                (9, Fv::U32(500_000)),
            ],
        )
        .message(
            34,
            &[
                (253, Fv::U32(t + 1800)),
                (5, Fv::U32(t + 5400)),
                (1, Fv::U16(1)),
            ],
        )
        .build()
}

async fn upload(e: &Env, token: &str, data_type: u32, bytes: Vec<u8>) {
    let (s, _) = e
        .request(
            Method::POST,
            &format!("/api/files?data_type={data_type}&source=watch"),
            Some(token),
            bytes,
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
}

#[tokio::test]
async fn a_watch_row_wins_whichever_came_first() {
    if !profile::available() {
        return;
    }
    let mut dumps = Vec::new();
    for connect_first in [true, false] {
        let e = env_at(now()).await;
        let input = fixtures();
        if connect_first {
            import(&e, input.path());
        }
        upload(&e, &e.token.clone(), 128, fit_hrv()).await;
        upload(&e, &e.token.clone(), 128, fit_night()).await;
        if !connect_first {
            import(&e, input.path());
        }
        let ts = cph(2026, 3, 11, 0, 0);
        let (weekly, last): (f64, Option<f64>) =
            sqlx::query_as("SELECT weekly_avg, last_night_avg FROM hrv_summary WHERE ts = ?")
                .bind(ts)
                .fetch_one(&e.pool)
                .await
                .unwrap();
        assert_eq!(weekly, 50.0, "connect_first={connect_first}");
        assert_eq!(last, None, "the watch row replaces the whole Connect row");
        let src: i64 = sqlx::query_scalar("SELECT src FROM hrv_summary WHERE ts = ?")
            .bind(ts)
            .fetch_one(&e.pool)
            .await
            .unwrap();
        let origin: String = sqlx::query_scalar("SELECT origin FROM sources WHERE src_id = ?")
            .bind(src)
            .fetch_one(&e.pool)
            .await
            .unwrap();
        assert_eq!(origin, "fit", "connect_first={connect_first}");
        // The Connect night is hidden by the overlapping watch night; the other one stays.
        let nights: Vec<(i64, Option<f64>)> = sqlx::query_as(
            "SELECT start_ts, asleep_s FROM sleep_sessions_current ORDER BY start_ts",
        )
        .fetch_all(&e.pool)
        .await
        .unwrap();
        assert_eq!(
            nights,
            vec![
                (cph(2026, 3, 10, 22, 45), None),
                (cph(2026, 3, 11, 23, 0), Some(27_000.0))
            ],
            "connect_first={connect_first}"
        );
        // The other Connect rows are untouched.
        assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM daily_totals").await, 1);
        dumps.push(
            sqlx::query_scalar::<_, String>(
                "SELECT group_concat(weekly_avg || ':' || ts, ',') FROM hrv_summary",
            )
            .fetch_one(&e.pool)
            .await
            .unwrap(),
        );
    }
    assert_eq!(dumps[0], dumps[1], "import order does not matter");
}

#[tokio::test]
async fn a_watch_activity_within_two_minutes_supersedes_the_connect_one() {
    if !profile::available() {
        return;
    }
    for (offset, hidden) in [(60, true), (-119, true), (300, false)] {
        for connect_first in [true, false] {
            let e = env_at(now()).await;
            let input = fixtures();
            if connect_first {
                import(&e, input.path());
            }
            upload(&e, &e.token.clone(), 4, fit_run(offset, 7)).await;
            if !connect_first {
                import(&e, input.path());
            }
            let keys: Vec<Option<String>> =
                sqlx::query_scalar("SELECT activity_key FROM activities_current ORDER BY start_ts")
                    .fetch_all(&e.pool)
                    .await
                    .unwrap();
            let connect_run = keys.iter().any(|k| k.as_deref() == Some("connect:1001"));
            assert_eq!(
                connect_run, !hidden,
                "offset {offset}s, connect_first={connect_first}: {keys:?}"
            );
            // The strength session is elsewhere in time and stays; the watch run is always there.
            assert!(keys.iter().any(|k| k.as_deref() == Some("connect:1002")));
            assert_eq!(keys.len(), if hidden { 2 } else { 3 });
            // Hidden, not deleted: the Connect rows are still derived.
            assert_eq!(
                count(
                    &e.pool,
                    "SELECT COUNT(*) FROM activities WHERE activity_key = 'connect:1001'"
                )
                .await,
                1
            );
            let user = e.login().await;
            let list = e.get(&user, "/api/activities").await;
            assert_eq!(list["items"].as_array().unwrap().len(), keys.len());
        }
    }
}

#[tokio::test]
async fn another_users_watch_activity_hides_nothing() {
    if !profile::available() {
        return;
    }
    let e = env_at(now()).await;
    e.adopt.store(false, std::sync::atomic::Ordering::Relaxed);
    let other = e.add_user("other", Role::User).await;
    import(&e, fixtures().path());
    let token = e.token_for(other).await;
    upload(&e, &token, 4, fit_run(0, 9)).await;
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM activities_current WHERE activity_key = 'connect:1001'"
        )
        .await,
        1
    );
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM activities_current").await,
        3
    );
}

// ---- records ----------------------------------------------------------------------------------

/// Made-up position: 10.0 degrees north, 20.0 east, plus `step` semicircle units (nowhere real).
const LAT0: i64 = 119_304_647;
const LON0: i64 = 238_609_294;

fn fit_secs(unix: i64) -> i64 {
    unix - FIT_EPOCH
}

fn fld(name: &str, def: i64, base: &str, raw: Value, value: Value) -> Value {
    json!({"name": name, "definition_number": def, "base_type": base, "raw_value": raw, "value": value})
}

/// One `record` message the way the connector returns it: some fields named, some `unknown_N`.
/// `named` false = the connector's decoder lacked the profile for speed and altitude.
fn record(idx: i64, unix: i64, step: i64, hr: i64, named: bool) -> Value {
    let mut f = vec![
        fld(
            "timestamp",
            253,
            "uint32",
            json!(fit_secs(unix)),
            json!("2026-03-12T16:00:00"),
        ),
        fld(
            "position_lat",
            0,
            "sint32",
            json!(LAT0 + step),
            json!(LAT0 + step),
        ),
        fld(
            "position_long",
            1,
            "sint32",
            json!(LON0 + step),
            json!(LON0 + step),
        ),
        fld(
            "distance",
            5,
            "uint32",
            json!(100 * step),
            json!(step as f64),
        ),
        fld("heart_rate", 3, "uint8", json!(hr), json!(hr)),
        fld("unknown_134", 134, "uint8", json!(9), json!(9)),
    ];
    if named {
        f.push(fld("enhanced_speed", 73, "uint32", json!(2500), json!(2.5)));
        f.push(fld(
            "enhanced_altitude",
            78,
            "uint32",
            json!(2464),
            json!(-7.2),
        ));
    } else {
        f.push(fld("unknown_73", 73, "uint32", json!(2500), json!(2500)));
        f.push(fld("unknown_78", 78, "uint32", json!(2464), json!(2464)));
    }
    json!({"message_index": idx, "type": "record", "global_message_number": 20, "fields": f})
}

/// A `time_in_zone` message (the connector names it `unknown_216`): the raw array is in
/// milliseconds.
fn zone_msg(idx: i64, ref_mesg: i64, ref_index: i64, ms: Value) -> Value {
    json!({"message_index": idx, "type": "unknown_216", "global_message_number": 216, "fields": [
        fld("unknown_253", 253, "uint32", json!(1), json!(1)),
        fld("unknown_0", 0, "uint16", json!(ref_mesg), json!(ref_mesg)),
        fld("unknown_1", 1, "uint16", json!(ref_index), json!(ref_index)),
        fld("unknown_2", 2, "uint32", ms.clone(), ms),
        fld("unknown_6", 6, "uint8", json!([101, 121, 141, 162, 182, 202]), json!([101, 121, 141, 162, 182, 202])),
    ]})
}

fn page(id: i64, offset: i64, messages: Vec<Value>) -> Value {
    json!({"activity_id": id, "source": "garmin_original_fit",
           "pagination": {"offset": offset, "limit": 5000, "returned_count": messages.len()},
           "messages": messages})
}

/// Records for the detailed strength session 1002 (starts 17:00 local = 16:00 UTC), in two pages
/// that overlap on message 2, as the connector pages when the limit is small.
fn write_records(dir: &Path) {
    let t0 = cph(2026, 3, 12, 17, 0);
    write_json(
        dir,
        "records/1002_0.json",
        &page(
            1002,
            0,
            vec![
                record(0, t0, 0, 100, true),
                record(1, t0 + 1, 10, 110, false),
                record(2, t0 + 2, 20, 120, true),
                // a record without a timestamp or any value is not a row
                json!({"message_index": 3, "type": "record", "global_message_number": 20,
                       "fields": [fld("unknown_134", 134, "uint8", json!(1), json!(1))]}),
            ],
        ),
    );
    write_json(
        dir,
        "records/1002_4.json",
        &page(
            1002,
            4,
            vec![
                record(2, t0 + 2, 20, 120, true),
                zone_msg(4, 216, 0, json!([6002, 26147, 17000, 0, 0, 0, 0])),
                record(5, t0 + 3, 30, 130, false),
                zone_msg(6, 18, 0, json!([10_000, 60_000, 300_000, null, 0, 0, 0])),
                {
                    // a heart rate of 255 is the FIT "invalid" value, not a reading
                    let mut r = record(7, t0 + 4, 40, 255, false);
                    r["fields"][4]["raw_value"] = json!(255);
                    r
                },
            ],
        ),
    );
}

fn q(s: &str) -> sqlx::AssertSqlSafe<String> {
    sqlx::AssertSqlSafe(s.to_owned())
}

async fn sha_of(pool: &SqlitePool, key: &str) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT sha256 FROM activities_current WHERE activity_key = '{key}'"
    )))
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn records_fill_the_track_the_charts_and_the_zones() {
    let e = env_at(now()).await;
    let input = fixtures();
    write_records(input.path());
    let out = import(&e, input.path());
    // The records directory is not a pile of unrecognised files.
    assert!(out.contains("JSON files read:      8"), "{out}");
    assert!(out.contains("not Connect JSON (not stored): 0"), "{out}");
    assert!(out.contains("activities with records: 1"), "{out}");
    assert!(out.contains("blobs stored new:     8"), "{out}");

    let sha = sha_of(&e.pool, "connect:1002").await;
    let n = count(
        &e.pool,
        &format!("SELECT COUNT(*) FROM activity_records WHERE sha256 = '{sha}'"),
    )
    .await;
    assert_eq!(
        n, 5,
        "messages 0, 1, 2, 5, 7 (2 is on both pages, 3 has no value)"
    );
    let first: (i64, f64, f64, f64, f64, f64, f64) = sqlx::query_as(q(&format!(
        "SELECT ts, lat, lon, hr, speed, altitude_m, distance_m FROM activity_records
         WHERE sha256 = '{sha}' ORDER BY idx LIMIT 1"
    )))
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(first.0, cph(2026, 3, 12, 17, 0));
    assert!(
        (first.1 - 10.0).abs() < 1e-6 && (first.2 - 20.0).abs() < 1e-6,
        "degrees"
    );
    assert_eq!((first.3, first.4, first.6), (100.0, 2.5, 0.0));
    assert!((first.5 + 7.2).abs() < 1e-9, "metres");
    // Unnamed speed and altitude are scaled here: the same numbers.
    let second: (f64, f64, f64) = sqlx::query_as(q(&format!(
        "SELECT speed, altitude_m, distance_m FROM activity_records WHERE sha256 = '{sha}' AND idx = 1"
    )))
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert!((second.0 - 2.5).abs() < 1e-9 && (second.1 + 7.2).abs() < 1e-9);
    assert_eq!(second.2, 10.0, "distance in metres");
    let last_hr: Option<f64> = sqlx::query_scalar(q(&format!(
        "SELECT hr FROM activity_records WHERE sha256 = '{sha}' ORDER BY idx DESC LIMIT 1"
    )))
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(last_hr, None, "255 is invalid");

    let zones: Vec<(Option<i64>, Option<i64>, String)> = sqlx::query_as(q(&format!(
        "SELECT ref_mesg, ref_index, hr_zone_s FROM activity_time_in_zone WHERE sha256 = '{sha}' ORDER BY idx"
    )))
    .fetch_all(&e.pool)
    .await
    .unwrap();
    assert_eq!(
        zones,
        vec![
            (Some(216), Some(0), "[6.002,26.147,17,0,0,0,0]".into()),
            (Some(18), Some(0), "[10,60,300,null,0,0,0]".into()),
        ]
    );
    // Only the activity with records has rows; the list entry and the others have none.
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM activity_records").await,
        5
    );

    // The activity page reads them: track, series and the zones of the session message.
    set_zone(&e, e.user_id).await;
    let user = e.login().await;
    let d = e.get(&user, &format!("/api/activities/{sha}")).await;
    assert_eq!(d["has_track"], true);
    let series = &d["series"];
    assert_eq!(
        series["heart_rate_bpm"],
        json!([100.0, 110.0, 120.0, 130.0, null])
    );
    assert_eq!(series["elapsed_s"], json!([0.0, 1.0, 2.0, 3.0, 4.0]));
    assert_eq!(d["hr_below_zone1_s"], 10.0);
    assert_eq!(d["hr_zones"][0]["duration_s"], 60.0);
    assert_eq!(d["hr_zones"][1]["duration_s"], 300.0);
    assert_eq!(d["hr_zones"][2]["duration_s"], Value::Null);
    let t = e.get(&user, &format!("/api/activities/{sha}/track")).await;
    assert_eq!(t["available"], true);
    assert_eq!(t["points"].as_array().unwrap().len(), 5);
}

#[tokio::test]
async fn a_rerun_with_records_changes_nothing_and_reparse_agrees() {
    let e = env_at(now()).await;
    let input = fixtures();
    write_records(input.path());
    import(&e, input.path());
    let before = dump(&e.pool).await;
    assert!(before.contains("activity_records|"));
    let second = import(&e, input.path());
    assert!(second.contains("blobs stored new:     0"), "{second}");
    assert!(second.contains("activities with records: 1"), "{second}");
    assert_eq!(dump(&e.pool).await, before);
    assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM files").await, 8);
    reparse(e.dir.path());
    assert_eq!(dump(&e.pool).await, before);
}

#[tokio::test]
async fn records_pulled_later_are_added_to_an_activity_imported_without() {
    let e = env_at(now()).await;
    let input = fixtures();
    import(&e, input.path());
    let old = sha_of(&e.pool, "connect:1002").await;
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM activity_records").await,
        0
    );

    write_records(input.path());
    let out = import(&e, input.path());
    assert!(out.contains("blobs stored new:     1"), "{out}");
    assert!(out.contains("blobs already there:  7"), "{out}");
    // The activity with records is the current one; the first import stays, hidden.
    let now_sha = sha_of(&e.pool, "connect:1002").await;
    assert_ne!(now_sha, old);
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM activities WHERE activity_key = 'connect:1002'"
        )
        .await,
        2
    );
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM activities_current WHERE activity_key = 'connect:1002'"
        )
        .await,
        1
    );
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM activity_records").await,
        5
    );
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM activity_time_in_zone").await,
        2
    );
    // The page lists it once, with its track.
    set_zone(&e, e.user_id).await;
    let user = e.login().await;
    let list = e.get(&user, "/api/activities").await;
    assert_eq!(list["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        e.get(&user, &format!("/api/activities/{now_sha}")).await["has_track"],
        true
    );
    // And once more changes nothing.
    let before = dump(&e.pool).await;
    let third = import(&e, input.path());
    assert!(third.contains("blobs stored new:     0"), "{third}");
    assert_eq!(dump(&e.pool).await, before);
}

#[tokio::test]
async fn records_of_an_unknown_activity_or_for_another_one_are_not_used() {
    let e = env_at(now()).await;
    let input = fixtures();
    // Pages for an activity that has no file here, and a stray name.
    write_json(
        input.path(),
        "records/777_0.json",
        &page(777, 0, vec![record(0, now(), 0, 90, true)]),
    );
    std::fs::write(input.path().join("records/notes.txt"), "x").unwrap();
    let out = import(&e, input.path());
    assert!(out.contains("activities with records: 0"), "{out}");
    assert!(
        out.contains("records without an activity file (not stored): 1"),
        "{out}"
    );
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM activity_records").await,
        0
    );

    // A page whose activity_id is another one: reported, the activity is stored without records.
    write_json(
        input.path(),
        "records/1002_0.json",
        &page(555, 0, vec![record(0, now(), 0, 90, true)]),
    );
    let bad = run_connect(e.dir.path(), input.path(), USER, &["--timezone", TZ]);
    assert!(!bad.status.success());
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("imported without records"),
        "{}",
        String::from_utf8_lossy(&bad.stderr)
    );
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM activity_records").await,
        0
    );
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM activities_current WHERE activity_key = 'connect:1002'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn a_watch_activity_still_hides_a_connect_activity_that_has_records() {
    if !profile::available() {
        return;
    }
    for connect_first in [true, false] {
        let e = env_at(now()).await;
        let input = fixtures();
        write_records(input.path());
        if connect_first {
            import(&e, input.path());
        }
        // A watch strength session starting 45 s after the Connect one (17:00 local).
        let t = (cph(2026, 3, 12, 17, 0) + 45 - FIT_EPOCH) as u32;
        let fit = FitBuilder::new()
            .message(0, &[(3, Fv::U32(9)), (4, Fv::U32(t))])
            .message(20, &[(253, Fv::U32(t)), (3, Fv::U8(120)), (5, Fv::U32(0))])
            .message(
                18,
                &[
                    (253, Fv::U32(t + 1800)),
                    (2, Fv::U32(t)),
                    (5, Fv::U8(1)),
                    (7, Fv::U32(1_800_000)),
                ],
            )
            .message(
                34,
                &[
                    (253, Fv::U32(t + 1800)),
                    (5, Fv::U32(t + 5400)),
                    (1, Fv::U16(1)),
                ],
            )
            .build();
        upload(&e, &e.token.clone(), 4, fit).await;
        if !connect_first {
            import(&e, input.path());
        }
        let keys: Vec<Option<String>> =
            sqlx::query_scalar("SELECT activity_key FROM activities_current ORDER BY start_ts")
                .fetch_all(&e.pool)
                .await
                .unwrap();
        assert!(
            !keys.iter().any(|k| k.as_deref() == Some("connect:1002")),
            "connect_first={connect_first}: {keys:?}"
        );
        // Hidden, not deleted: its records are still stored.
        assert_eq!(
            count(
                &e.pool,
                "SELECT COUNT(*) FROM activity_records r JOIN sources s ON s.sha256 = r.sha256 WHERE s.origin = 'connect'"
            )
            .await,
            5
        );
    }
}
