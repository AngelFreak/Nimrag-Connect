//! Garmin Connect daily summaries (`import-connect` reading `daily_stats/YYYY-MM.json`) and the
//! fallback to them on every page where the watch has no data for a day: stress, energy, heart
//! rate range, breathing, intensity minutes, steps, distance and floors. The fixture is
//! `tests/fixtures/connect_daily` (made-up numbers); watch rows are plain INSERTs.
//!
//! Fixture days (Copenhagen): 10-31 and 11-12, 11-13, 11-15 to 11-19 have rows (11-14 is all
//! null: no row; 11-15 has steps only: its zeros and -1 are "not measured"; a day with a bad
//! date is skipped). 11-19's Connect numbers are deliberately wild, so a page that shows them
//! instead of the watch's would fail.
mod common;

use axum::http::{Method, StatusCode};
use common::*;
use nimrag_server::users::Role;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const TZ: &str = "Europe/Copenhagen";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/connect_daily")
}

fn at(day: u32, h: u32, m: u32) -> i64 {
    cph(2026, 11, day, h, m)
}

fn run_connect(data: &Path, input: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .arg("import-connect")
        .args(["--user", USER, "--timezone", TZ])
        .arg("--data-dir")
        .arg(data)
        .arg(input)
        .output()
        .unwrap()
}

fn import(e: &Env, input: &Path) -> String {
    let out = run_connect(e.dir.path(), input);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn reparse(dir: &Path) {
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
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// `connect_daily` as text, in key order.
async fn dump(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT quote(user_id) || '|' || quote(ts) || '|' || quote(steps) || '|' || quote(distance_m)
         || '|' || quote(floors_up) || '|' || quote(moderate_min) || '|' || quote(vigorous_min)
         || '|' || quote(hr_min) || '|' || quote(hr_max) || '|' || quote(stress_avg)
         || '|' || quote(bb_charged) || '|' || quote(bb_drained) || '|' || quote(bb_high)
         || '|' || quote(bb_low) || '|' || quote(resp_avg) || '|' || quote(resp_high)
         || '|' || quote(resp_low) FROM connect_daily ORDER BY user_id, ts",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// An environment pinned to `now`, the data user in Copenhagen, the fixture imported.
async fn setup(now: i64) -> (Env, String) {
    let e = env_at(now).await;
    nimrag_server::settings::set_user(&e.pool, e.user_id, "timezone", TZ)
        .await
        .unwrap();
    import(&e, &fixture());
    let cookie = e.login().await;
    (e, cookie)
}

/// Watch rows: Thursday 19 has everything (stress and body battery, heart rate, breathing,
/// intensity running totals 45 / 6 for the week so far); Wednesday 18 only a stress reading
/// without body battery.
async fn watch(e: &Env) {
    for sql in [
        format!("INSERT INTO mon_hr (ts, bpm, src) VALUES ({}, 70, 1), ({}, 150, 1)", at(19, 8, 0), at(19, 12, 0)),
        format!(
            "INSERT INTO stress (ts, stress, stress_raw, body_battery, src) VALUES ({}, 40, 40, 60, 1), ({}, 60, 60, 55, 1)",
            at(19, 8, 0),
            at(19, 9, 0)
        ),
        format!("INSERT INTO respiration (ts, brpm, src) VALUES ({}, 15, 1), ({}, 17, 1)", at(19, 10, 0), at(19, 11, 0)),
        format!("INSERT INTO mon_extra (ts, moderate_total_min, vigorous_total_min, src) VALUES ({}, 45, 6, 1)", at(19, 7, 0)),
        format!(
            "INSERT INTO stress (ts, stress, stress_raw, body_battery, src) VALUES ({}, 30, 30, NULL, 1)",
            at(18, 9, 0)
        ),
    ] {
        e.exec(&sql).await;
    }
}

/// The value of series `key` at `date` of a metric response.
fn point(j: &Value, key: &str, date: &str) -> Option<f64> {
    let s = j["series"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == key)
        .unwrap_or_else(|| panic!("no series {key}"));
    s["points"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["date"] == date)
        .unwrap_or_else(|| panic!("no point {date} in {key}"))["value"]
        .as_f64()
}

fn stat(j: &Value, key: &str) -> Option<f64> {
    j["stats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == key)
        .unwrap_or_else(|| panic!("no stat {key}"))["value"]
        .as_f64()
}

async fn metric(e: &Env, cookie: &str, name: &str, query: &str) -> Value {
    e.get(cookie, &format!("/api/metrics/{name}?{query}")).await
}

fn f(j: &Value) -> Option<f64> {
    j.as_f64()
}

// ---- import -----------------------------------------------------------------------------------

#[tokio::test]
async fn import_reads_daily_stats_and_a_rerun_changes_nothing() {
    let e = env_at(at(20, 12, 0)).await;
    let out = import(&e, &fixture());
    assert!(out.contains("daily_stats: 2 new, 0 already there"), "{out}");
    assert!(out.contains("not Connect JSON (not stored): 0"), "{out}");

    let rows = dump(&e.pool).await;
    // 11-14 (all null) is no row; the bad date is skipped.
    assert_eq!(rows.len(), 8, "{rows:#?}");
    let uid = e.user_id;
    assert_eq!(
        rows[1],
        format!(
            "{uid}|{}|2327.0|1669.0|1.0|0.0|0.0|58.0|101.0|28.0|5.0|33.0|65.0|34.0|13.0|17.0|10.0",
            at(12, 0, 0)
        ),
        "a day of the example file, zeros that are amounts kept"
    );
    assert_eq!(
        rows[3],
        format!("{uid}|{}|800.0|{}", at(15, 0, 0), ["NULL"; 14].join("|")),
        "0 heart rate, -1 stress and -1 breathing are not measurements"
    );
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM file_derive WHERE status = 'partial' AND skipped_rows = 1"
        )
        .await,
        1,
        "the entry with a bad date is counted as skipped"
    );

    // Again: every blob is already there, nothing changes.
    let before = dump(&e.pool).await;
    let again = import(&e, &fixture());
    assert!(
        again.contains("daily_stats: 0 new, 2 already there"),
        "{again}"
    );
    assert_eq!(dump(&e.pool).await, before);
    // And a reparse derives the same rows from the stored blobs.
    reparse(e.dir.path());
    assert_eq!(dump(&e.pool).await, before);
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM sources WHERE origin = 'connect'"
        )
        .await,
        2
    );
}

#[tokio::test]
async fn a_later_pull_of_the_month_replaces_the_days_without_duplicates() {
    let e = env_at(at(20, 12, 0)).await;
    import(&e, &fixture());
    // The same month pulled again a day later: one more day, one number corrected.
    let dir = tempfile::tempdir().unwrap();
    let mut days: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(fixture().join("daily_stats/2026-11.json")).unwrap(),
    )
    .unwrap();
    days[0]["stress_avg"] = json!(29);
    days.push(json!({"date": "2026-11-20", "steps": 100, "stress_avg": 33}));
    std::fs::create_dir_all(dir.path().join("daily_stats")).unwrap();
    std::fs::write(
        dir.path().join("daily_stats/2026-11.json"),
        serde_json::to_string(&days).unwrap(),
    )
    .unwrap();
    let out = import(&e, dir.path());
    assert!(out.contains("daily_stats: 1 new, 0 already there"), "{out}");
    let rows = dump(&e.pool).await;
    assert_eq!(rows.len(), 9, "one row per day, not per pull");
    assert!(rows[1].contains("|101.0|29.0|5.0|"), "{}", rows[1]);
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM connect_daily WHERE stress_avg = 33"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn a_directory_without_daily_stats_imports_as_before() {
    let e = env_at(at(20, 12, 0)).await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("stats.json"),
        r#"{"days": [{"date": "2026-11-11", "steps": 9000, "has_data": true}]}"#,
    )
    .unwrap();
    let out = import(&e, dir.path());
    assert!(!out.contains("daily_stats"), "{out}");
    assert!(out.contains("errors:               0"), "{out}");
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM connect_daily").await,
        0
    );
    assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM daily_totals").await, 1);
}

// ---- the Day view -----------------------------------------------------------------------------

#[tokio::test]
async fn a_day_without_watch_data_shows_connects_summary() {
    let (e, c) = setup(at(20, 12, 0)).await;
    let d = e.get(&c, "/api/days/2026-11-12").await;
    assert_eq!(f(&d["stress"]["average"]), Some(28.0));
    assert!(d["stress"]["series"].as_array().unwrap().is_empty());
    assert!(d["stress"]["main_level"].is_null() && d["stress"]["rest_s"].is_null());
    let en = &d["energy"];
    assert_eq!(
        (
            f(&en["high"]),
            f(&en["low"]),
            f(&en["charged"]),
            f(&en["drained"])
        ),
        (Some(65.0), Some(34.0), Some(5.0), Some(33.0))
    );
    assert!(en["now"].is_null() && en["series"].as_array().unwrap().is_empty());
    let hr = &d["heart_rate"]["day"];
    assert_eq!(
        (f(&hr["low_bpm"]), f(&hr["high_bpm"])),
        (Some(58.0), Some(101.0))
    );
    assert!(hr["series"].as_array().unwrap().is_empty());
    let b = &d["breathing"];
    assert_eq!(
        (f(&b["lowest"]), f(&b["highest"]), f(&b["avg_awake"])),
        (Some(10.0), Some(17.0), Some(13.0))
    );
    assert!(b["avg_asleep"].is_null(), "Connect has no asleep figure");
    assert!(b["hourly"].as_array().unwrap().is_empty());
    assert_eq!(
        f(&d["intensity_minutes"]),
        Some(0.0),
        "0 + 0 minutes is 0, not unknown"
    );
    assert_eq!(f(&d["steps"]), Some(2327.0));

    // A day with nothing of the figures: null, never 0.
    let d = e.get(&c, "/api/days/2026-11-14").await;
    assert!(d["stress"]["average"].is_null() && d["energy"]["high"].is_null());
    assert!(d["heart_rate"]["day"]["high_bpm"].is_null() && d["breathing"]["lowest"].is_null());
    assert!(d["intensity_minutes"].is_null() && d["steps"].is_null());
    // 11-15: steps only; the zeros and -1 did not become values.
    let d = e.get(&c, "/api/days/2026-11-15").await;
    assert_eq!(f(&d["steps"]), Some(800.0));
    assert!(d["stress"]["average"].is_null() && d["heart_rate"]["day"]["low_bpm"].is_null());
    assert!(d["breathing"]["avg_awake"].is_null());
}

#[tokio::test]
async fn watch_data_wins_per_metric_and_day() {
    let (e, c) = setup(at(20, 12, 0)).await;
    watch(&e).await;

    // Thursday 19: the watch has all of it; Connect's wild numbers are not shown.
    let d = e.get(&c, "/api/days/2026-11-19").await;
    assert_eq!(f(&d["stress"]["average"]), Some(50.0));
    assert_eq!(d["stress"]["series"].as_array().unwrap().len(), 2);
    let en = &d["energy"];
    assert_eq!(
        (
            f(&en["high"]),
            f(&en["low"]),
            f(&en["charged"]),
            f(&en["drained"]),
            f(&en["now"])
        ),
        (Some(60.0), Some(55.0), Some(0.0), Some(5.0), Some(55.0))
    );
    let hr = &d["heart_rate"]["day"];
    assert_eq!(
        (f(&hr["low_bpm"]), f(&hr["high_bpm"])),
        (Some(70.0), Some(150.0))
    );
    let b = &d["breathing"];
    assert_eq!(
        (f(&b["lowest"]), f(&b["highest"]), f(&b["avg_awake"])),
        (Some(15.0), Some(17.0), Some(16.0))
    );
    // 99 / 9 from Connect would be 117; the watch's running total 45 / 6 less what Connect
    // counted for Monday to Wednesday (30 / 5) is 15 / 1: 17.
    assert_eq!(f(&d["intensity_minutes"]), Some(17.0));
    // The watch has no step counters for the day: Connect's steps are the fallback.
    assert_eq!(f(&d["steps"]), Some(7000.0));

    // Wednesday 18: a watch stress reading, nothing else: stress from the watch, the rest from Connect.
    let d = e.get(&c, "/api/days/2026-11-18").await;
    assert_eq!(f(&d["stress"]["average"]), Some(30.0));
    assert_eq!(f(&d["energy"]["high"]), Some(70.0));
    assert_eq!(f(&d["heart_rate"]["day"]["high_bpm"]), Some(99.0));
    assert_eq!(f(&d["breathing"]["avg_awake"]), Some(15.0));
}

// ---- Today ------------------------------------------------------------------------------------

#[tokio::test]
async fn today_and_yesterday_fall_back_too() {
    // Friday 13 Nov: a day the fixture has and no watch row exists for.
    let (e, c) = setup(at(13, 12, 0)).await;
    let t = e.get(&c, "/api/today").await;
    assert_eq!(f(&t["stress"]["average"]), Some(31.0));
    let en = &t["energy"];
    assert_eq!(
        (
            f(&en["high"]),
            f(&en["low"]),
            f(&en["charged"]),
            f(&en["drained"])
        ),
        (Some(88.0), Some(21.0), Some(40.0), Some(52.0))
    );
    let hr = &t["heart_rate"]["day"];
    assert_eq!(
        (f(&hr["low_bpm"]), f(&hr["high_bpm"])),
        (Some(55.0), Some(120.0))
    );
    assert!(
        t["heart_rate_24h"]["series"].as_array().unwrap().is_empty(),
        "the rolling 24 hours is a watch chart"
    );
    assert_eq!(f(&t["respiration"]["day_avg_brpm"]), Some(14.5));
    assert!(t["respiration"]["latest_brpm"].is_null());
    assert_eq!(f(&t["steps"]["steps"]), Some(4100.0));
    assert_eq!(f(&t["steps"]["distance_m"]), Some(3000.0));
    // The week of Monday 9: 11-12 gave 0 / 0, today 12 / 3.
    let im = &t["intensity_minutes"];
    assert_eq!(
        (
            f(&im["moderate_min"]),
            f(&im["vigorous_min"]),
            f(&im["total_min"])
        ),
        (Some(12.0), Some(3.0), Some(18.0))
    );
    let days = im["days"].as_array().unwrap();
    assert_eq!(f(&days[3]["total_min"]), Some(0.0));
    assert_eq!(f(&days[4]["total_min"]), Some(18.0));
    let y = &t["yesterday"];
    assert_eq!(y["date"], "2026-11-12");
    assert_eq!(
        (
            f(&y["energy_charged"]),
            f(&y["energy_drained"]),
            f(&y["steps"]),
            f(&y["hr_high_bpm"]),
            f(&y["stress_avg"]),
            f(&y["intensity_minutes"])
        ),
        (
            Some(5.0),
            Some(33.0),
            Some(2327.0),
            Some(101.0),
            Some(28.0),
            Some(0.0)
        )
    );
    assert_eq!(f(&t["last_7_days"]["avg_stress"]), Some(28.0));

    // The same day on the Day, Movement and Calendar pages.
    let d = e.get(&c, "/api/days/2026-11-13").await;
    assert_eq!(f(&d["stress"]["average"]), Some(31.0));
    assert_eq!(f(&d["intensity_minutes"]), Some(18.0));
    let m = e.get(&c, "/api/movement/2026-11-13").await;
    assert_eq!(
        f(&m["floors"]),
        Some(4.0),
        "floors have a place on the Movement page"
    );
    assert_eq!(f(&m["distance_m"]), Some(3000.0));
    assert_eq!(f(&m["steps"]), Some(4100.0));
    let m = e.get(&c, "/api/movement/2026-11-14").await;
    assert!(m["floors"].is_null());
    let cal = e.get(&c, "/api/calendar/2026-11").await;
    let day = &cal["days"].as_array().unwrap()[12];
    assert_eq!(day["date"], "2026-11-13");
    assert_eq!(
        (
            f(&day["energy_high"]),
            f(&day["energy_low"]),
            f(&day["steps"])
        ),
        (Some(88.0), Some(21.0), Some(4100.0))
    );
}

// ---- metric ranges and trends -----------------------------------------------------------------

#[tokio::test]
async fn metric_ranges_trends_and_has_prev_use_the_summaries() {
    let (e, c) = setup(at(20, 12, 0)).await;
    watch(&e).await;
    let get = |m: &'static str, q: &'static str| metric(&e, &c, m, q);

    // 4 weeks to Friday 20: 10-24 .. 11-20.
    let s = get("stress", "range=4w").await;
    assert_eq!(point(&s, "average", "2026-11-12"), Some(28.0));
    assert_eq!(point(&s, "average", "2026-11-13"), Some(31.0));
    assert_eq!(point(&s, "average", "2026-11-14"), None);
    assert_eq!(
        point(&s, "average", "2026-11-18"),
        Some(30.0),
        "the watch's reading, not Connect's 35"
    );
    assert_eq!(point(&s, "average", "2026-11-19"), Some(50.0));
    assert_eq!(
        point(&s, "rest_s", "2026-11-12"),
        None,
        "no levels without a time series"
    );
    // 10-31 (40), 28, 31, 22, 26, 30 (the watch's), 50: 227 / 7
    assert_eq!(stat(&s, "avg_stress"), Some(32.4));
    assert_eq!(point(&s, "average", "2026-10-31"), Some(40.0));
    assert_eq!(
        s["has_prev"], false,
        "nothing before 10-31, the first day of the 4 weeks' data"
    );
    let s7 = get("stress", "range=7d").await;
    assert_eq!(
        s7["has_prev"], true,
        "11-12 is before the week 11-14 to 11-20"
    );

    let en = get("energy", "range=4w").await;
    assert_eq!(point(&en, "high", "2026-11-12"), Some(65.0));
    assert_eq!(point(&en, "drained", "2026-11-13"), Some(52.0));
    assert_eq!(point(&en, "high", "2026-11-19"), Some(60.0));
    assert_eq!(
        point(&en, "high", "2026-11-18"),
        Some(70.0),
        "no watch battery that day"
    );

    let hr = get("heart_rate", "range=4w").await;
    assert_eq!(point(&hr, "high", "2026-11-12"), Some(101.0));
    assert_eq!(point(&hr, "high", "2026-11-19"), Some(150.0));

    let r = get("respiration", "range=4w").await;
    assert_eq!(point(&r, "awake", "2026-11-12"), Some(13.0));
    assert_eq!(point(&r, "awake", "2026-11-19"), Some(16.0));
    assert_eq!(point(&r, "asleep", "2026-11-12"), None);
    assert_eq!(point(&r, "awake", "2026-11-15"), None);

    let st = get("steps", "range=4w").await;
    assert_eq!(point(&st, "distance_m", "2026-11-13"), Some(3000.0));
    assert_eq!(point(&st, "steps", "2026-11-12"), Some(2327.0));

    // The week of Monday 16 (calendar week): Connect 10/0, 20/5, 0/0; the watch's Thursday
    // total less those.
    let i = get("intensity_minutes", "range=7d").await;
    assert_eq!(i["from"], "2026-11-16");
    assert_eq!(point(&i, "moderate", "2026-11-16"), Some(10.0));
    assert_eq!(point(&i, "moderate", "2026-11-17"), Some(20.0));
    assert_eq!(point(&i, "vigorous", "2026-11-17"), Some(5.0));
    assert_eq!(point(&i, "moderate", "2026-11-18"), Some(0.0));
    assert_eq!(point(&i, "moderate", "2026-11-19"), Some(15.0));
    assert_eq!(point(&i, "vigorous", "2026-11-19"), Some(1.0));
    assert_eq!(point(&i, "moderate", "2026-11-20"), None);
    assert_eq!(stat(&i, "moderate"), Some(45.0));
    assert_eq!(stat(&i, "vigorous"), Some(6.0));
    assert_eq!(stat(&i, "total"), Some(57.0));
    // And the Movement page's week says the same.
    let m = e.get(&c, "/api/movement/2026-11-19").await;
    assert_eq!(f(&m["intensity"]["total_min"]), Some(57.0));

    // Weekly buckets (84 days back from 11-20, so one starts 11-07): the mean of the days with a
    // value, 11-12 and 11-13.
    let w = get("respiration", "range=12w").await;
    assert_eq!(w["bucket"], "week");
    assert_eq!(point(&w, "awake", "2026-11-07"), Some(13.8));

    // Trends: the awake average counts Connect's days: (12 + 13 + 14.5 + 13.5 + 14 + 15 + 16) / 7.
    let t = e.get(&c, "/api/trends?range=4w").await;
    assert_eq!(f(&t["breathing"]["awake_avg_brpm"]), Some(14.0));
    assert!(t["breathing"]["asleep"]["points"]
        .as_array()
        .unwrap()
        .is_empty());

    // The earliest day decides `has_prev`: nothing before 10-31.
    let s = e
        .get(&c, "/api/metrics/stress?range=7d&end=2026-10-31")
        .await;
    assert_eq!(s["has_prev"], false);
    assert_eq!(point(&s, "average", "2026-10-31"), Some(40.0));
}

#[tokio::test]
async fn another_user_sees_none_of_it() {
    let e = env_at(at(20, 12, 0)).await;
    e.adopt.store(false, std::sync::atomic::Ordering::Relaxed);
    let other = e.add_user("other", Role::User).await;
    import(&e, &fixture());
    nimrag_server::settings::set_user(&e.pool, other, "timezone", TZ)
        .await
        .unwrap();
    let cookie = e.login_as("other").await;
    let d = e.get(&cookie, "/api/days/2026-11-12").await;
    assert!(d["stress"]["average"].is_null() && d["energy"]["high"].is_null());
    assert!(d["steps"].is_null() && d["breathing"]["lowest"].is_null());
    let (s, _, _) = e
        .call(
            Method::GET,
            "/api/metrics/energy?range=4w",
            Some(&cookie),
            false,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        count(
            &e.pool,
            &format!("SELECT COUNT(*) FROM connect_daily WHERE user_id = {other}")
        )
        .await,
        0
    );
}
