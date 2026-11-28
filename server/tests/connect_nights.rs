//! Garmin Connect nights (`import-connect` reading `sleep_detail/YYYY-MM-DD.json`, Connect's full
//! sleep response of one night) through the real binary and router, on a SYNTHETIC night made
//! here (made-up numbers): it fills the stage, HRV, heart rate and breathing rows of a night the
//! watch did not deliver, and gives way to the watch's own stage and HRV rows whatever the
//! import order.
//!
//! The night (Copenhagen, CET): Fri 27 Nov 22:00 to Sat 28 Nov 06:00 local = 21:00Z to 05:00Z.
mod common;

use common::*;
use nimrag_server::derived::{rank_key, src_id, upsert_samples, V};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::Path;
use std::process::Command;

const TZ: &str = "Europe/Copenhagen";
/// The account identifiers the response carries; they must never reach a stored blob.
const PROFILE_PK: i64 = 424_242;
const SLEEP_ID: i64 = 9_876_543_210;

/// Unix seconds of a UTC time on 27 or 28 November 2026.
fn z(day: u32, h: u32, m: u32) -> i64 {
    chrono::NaiveDate::from_ymd_opt(2026, 11, day)
        .unwrap()
        .and_hms_opt(h, m, 0)
        .unwrap()
        .and_utc()
        .timestamp()
}

fn zs(day: u32, h: u32, m: u32) -> String {
    format!("2026-11-{day:02}T{h:02}:{m:02}:00.0")
}

/// Connect's response for the night: four stage periods, HRV, heart rate and breathing with the
/// gaps Connect has (a null, a 0 and a -1).
fn night() -> Value {
    json!({
        "userProfilePK": PROFILE_PK,
        "dailySleepDTO": {
            "id": SLEEP_ID,
            "userProfilePK": PROFILE_PK,
            "calendarDate": "2026-11-28",
            "sleepStartTimestampGMT": z(27, 21, 0) * 1000,
            "sleepEndTimestampGMT": z(28, 5, 0) * 1000,
            "deepSleepSeconds": 5400,
            "lightSleepSeconds": 3600,
            "remSleepSeconds": 18000,
            "awakeSleepSeconds": 1800,
            "sleepTimeSeconds": 27000,
            "sleepScores": {"overall": {"value": 81}}
        },
        "sleepLevels": [
            {"startGMT": zs(27, 21, 0), "endGMT": zs(27, 22, 0), "activityLevel": 1.0},
            {"startGMT": zs(27, 22, 0), "endGMT": zs(27, 23, 30), "activityLevel": 0.0},
            {"startGMT": zs(27, 23, 30), "endGMT": zs(28, 4, 30), "activityLevel": 2.0},
            {"startGMT": zs(28, 4, 30), "endGMT": zs(28, 5, 0), "activityLevel": 3.0}
        ],
        "avgOvernightHrv": 48.0,
        "hrvData": [
            {"value": 44.0, "startGMT": z(27, 22, 0) * 1000},
            {"value": null, "startGMT": z(27, 22, 5) * 1000},
            {"value": 52.5, "startGMT": z(27, 22, 10) * 1000}
        ],
        "sleepHeartRate": [
            {"value": 58, "startGMT": z(27, 22, 0) * 1000},
            {"value": 0, "startGMT": z(27, 22, 2) * 1000},
            {"value": 54, "startGMT": z(27, 22, 4) * 1000},
            {"value": 51, "startGMT": z(27, 22, 6) * 1000}
        ],
        "wellnessEpochRespirationDataDTOList": [
            {"startTimeGMT": z(27, 22, 0) * 1000, "respirationValue": 14.0},
            {"startTimeGMT": z(27, 22, 1) * 1000, "respirationValue": -1.0},
            {"startTimeGMT": z(27, 22, 2) * 1000, "respirationValue": 0.0},
            {"startTimeGMT": z(27, 22, 3) * 1000, "respirationValue": 13.5}
        ]
    })
}

fn write_night(dir: &Path) {
    let d = dir.join("sleep_detail");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join("2026-11-28.json"),
        serde_json::to_string_pretty(&night()).unwrap(),
    )
    .unwrap();
}

async fn setup() -> (Env, String) {
    let e = env_at(cph(2026, 11, 28, 12, 0)).await;
    nimrag_server::settings::set_user(&e.pool, e.user_id, "timezone", TZ)
        .await
        .unwrap();
    let cookie = e.login().await;
    (e, cookie)
}

fn import(e: &Env) {
    let input = tempfile::tempdir().unwrap();
    write_night(input.path());
    let out = Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .arg("import-connect")
        .args(["--user", USER, "--timezone", TZ])
        .arg("--data-dir")
        .arg(e.dir.path())
        .arg(input.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("sleep_detail"), "{text}");
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

async fn rows(pool: &SqlitePool, sql: &str) -> Vec<String> {
    sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_all(pool)
        .await
        .unwrap()
}

/// Rows of a sample table as text with their source's origin, in key order. `table` may be a
/// `_current` view.
async fn dump(pool: &SqlitePool, table: &str, cols: &str) -> Vec<String> {
    let expr = cols
        .split(", ")
        .map(|c| format!("quote(t.{c})"))
        .collect::<Vec<_>>()
        .join(" || '|' || ");
    rows(
        pool,
        &format!(
            "SELECT t.ts || '|' || {expr} || '|' || s.origin FROM {table} t
             JOIN sources s ON s.src_id = t.src ORDER BY t.user_id, t.ts"
        ),
    )
    .await
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A watch night: its session, and optionally stage rows and HRV rows at instants of its own
/// (not Connect's), written like a FIT file's rows.
async fn watch_night(e: &Env, (start, end): (i64, i64), levels: &[(i64, i64)], hrv: &[(i64, f64)]) {
    let sha = "c".repeat(64);
    let last = end;
    e.exec(&format!(
        "INSERT INTO files (sha256, size, received_at, source) VALUES ('{sha}', 1, '2026-11-28T06:30:00.000Z', 'watch')"
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO file_owners (sha256, user_id) VALUES ('{sha}', {})",
        e.user_id
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO sources (sha256, src_id, ok, last_ts, msg_count, rank_key) VALUES ('{sha}', {}, 1, {last}, 1, '{}')",
        src_id(&sha),
        rank_key(true, Some(last), 1)
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO sleep_sessions (sha256, idx, start_ts, end_ts, overall_score) VALUES ('{sha}', 0, {start}, {end}, 70)"
    ))
    .await;
    let mut tx = e.pool.begin().await.unwrap();
    let lv = levels
        .iter()
        .map(|(ts, l)| vec![V::I(Some(*ts)), V::I(Some(*l)), V::T(None)])
        .collect::<Vec<_>>();
    upsert_samples(
        &mut tx,
        "sleep_levels",
        &["ts"],
        &["level", "level_name"],
        src_id(&sha),
        e.user_id,
        lv,
    )
    .await
    .unwrap();
    let hv = hrv
        .iter()
        .map(|(ts, v)| vec![V::I(Some(*ts)), V::R(Some(*v))])
        .collect::<Vec<_>>();
    upsert_samples(
        &mut tx,
        "hrv_values",
        &["ts"],
        &["rmssd_ms"],
        src_id(&sha),
        e.user_id,
        hv,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

/// A watch night with its own stage instants (none equal to Connect's): light, deep, REM, awake.
fn watch_levels() -> Vec<(i64, i64)> {
    vec![
        (z(27, 22, 10), 2),
        (z(27, 23, 40), 3),
        (z(28, 4, 40), 4),
        (z(28, 5, 5), 1),
    ]
}

fn sleep_api_stages(j: &Value) -> Vec<(String, String, String)> {
    j["night"]["stages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["start"].as_str().unwrap().to_owned(),
                s["end"].as_str().unwrap().to_owned(),
                s["stage"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_night_into_an_empty_database_gives_stages_hrv_heart_rate_and_breathing() {
    let (e, cookie) = setup().await;
    import(&e);
    let p = &e.pool;

    // Rows end their period; Connect 0/1/2/3 is the watch's 3/2/4/1.
    assert_eq!(
        dump(p, "sleep_levels", "level, level_name").await,
        vec![
            format!("{}|2|'light'|connect", z(27, 22, 0)),
            format!("{}|3|'deep'|connect", z(27, 23, 30)),
            format!("{}|4|'rem'|connect", z(28, 4, 30)),
            format!("{}|1|'awake'|connect", z(28, 5, 0)),
        ]
    );
    // The null HRV value is no row.
    assert_eq!(
        dump(p, "hrv_values", "rmssd_ms").await,
        vec![
            format!("{}|44.0|connect", z(27, 22, 0)),
            format!("{}|52.5|connect", z(27, 22, 10)),
        ]
    );
    // The summary is one row at the night's end, the other columns empty.
    assert_eq!(
        dump(
            p,
            "hrv_summary",
            "weekly_avg, last_night_avg, last_night_5min_high, baseline_low_upper, \
             baseline_balanced_lower, baseline_balanced_upper, status"
        )
        .await,
        vec![format!(
            "{}|NULL|48.0|NULL|NULL|NULL|NULL|NULL|connect",
            z(28, 5, 0)
        )]
    );
    // A heart rate of 0 and a breathing of -1 or 0 are gaps.
    assert_eq!(
        dump(p, "mon_hr", "bpm").await,
        vec![
            format!("{}|58.0|connect", z(27, 22, 0)),
            format!("{}|54.0|connect", z(27, 22, 4)),
            format!("{}|51.0|connect", z(27, 22, 6)),
        ]
    );
    assert_eq!(
        dump(p, "respiration", "brpm, brpm_raw").await,
        vec![
            format!("{}|14.0|NULL|connect", z(27, 22, 0)),
            format!("{}|13.5|NULL|connect", z(27, 22, 3)),
        ]
    );
    // The night itself, as the `sleep` kind writes one.
    assert_eq!(
        rows(
            p,
            "SELECT start_ts || '|' || end_ts || '|' || overall_score || '|' || asleep_s || '|' ||
                    deep_s || '|' || light_s || '|' || rem_s || '|' || awake_s
             FROM sleep_sessions_current"
        )
        .await,
        vec![format!(
            "{}|{}|81.0|27000.0|5400.0|3600.0|18000.0|1800.0",
            z(27, 21, 0),
            z(28, 5, 0)
        )]
    );

    // The account identifiers are not in the stored blob; the rest is.
    let sha: String = sqlx::query_scalar("SELECT sha256 FROM files WHERE source = 'connect'")
        .fetch_one(p)
        .await
        .unwrap();
    let blob = String::from_utf8(std::fs::read(stored_path(e.dir.path(), &sha)).unwrap()).unwrap();
    for banned in [
        "userProfilePK",
        &PROFILE_PK.to_string(),
        &SLEEP_ID.to_string(),
    ] {
        assert!(!blob.contains(banned), "{banned} is in the blob");
    }
    assert!(blob.contains("\"kind\":\"sleep_detail\"") && blob.contains("calendarDate"));
    assert!(
        blob.contains("avgOvernightHrv") && blob.contains("wellnessEpochRespirationDataDTOList")
    );

    // The night page shows the Connect stages (the totals come from the stage rows).
    let j = e.get(&cookie, "/api/sleep/2026-11-28").await;
    let n = &j["night"];
    assert_eq!(n["stages_available"], true);
    assert_eq!(sleep_api_stages(&j).len(), 4);
    assert_eq!(
        (
            n["deep_s"].as_f64(),
            n["light_s"].as_f64(),
            n["rem_s"].as_f64(),
            n["awake_s"].as_f64()
        ),
        (Some(5400.0), Some(3600.0), Some(18000.0), Some(1800.0))
    );
    assert_eq!(n["hrv_ms"].as_f64(), Some(48.0));
    assert_eq!(n["score"].as_f64(), Some(81.0));
}

#[tokio::test]
async fn a_night_the_watch_has_stage_rows_for_adds_no_connect_stage_rows() {
    let (e, cookie) = setup().await;
    // The watch's night: stage rows at its own instants, no HRV.
    watch_night(&e, (z(27, 21, 10), z(28, 5, 10)), &watch_levels(), &[]).await;
    import(&e);
    let p = &e.pool;

    assert_eq!(
        count(
            p,
            "SELECT COUNT(*) FROM sleep_levels t JOIN sources s ON s.src_id = t.src WHERE s.origin = 'connect'"
        )
        .await,
        0,
        "no Connect stage row beside the watch's"
    );
    assert_eq!(count(p, "SELECT COUNT(*) FROM sleep_levels").await, 4);
    // The watch has no HRV for the night, so Connect's fill it; heart rate and breathing too.
    assert_eq!(count(p, "SELECT COUNT(*) FROM hrv_values").await, 2);
    assert_eq!(count(p, "SELECT COUNT(*) FROM hrv_summary").await, 1);
    assert_eq!(count(p, "SELECT COUNT(*) FROM mon_hr").await, 3);
    assert_eq!(count(p, "SELECT COUNT(*) FROM respiration").await, 2);

    let j = e.get(&cookie, "/api/sleep/2026-11-28").await;
    let stages = sleep_api_stages(&j);
    assert_eq!(stages.len(), 4, "the watch's four periods only: {stages:?}");
    assert_eq!(
        j["night"]["score"].as_f64(),
        Some(70.0),
        "the watch's night"
    );
    assert_eq!(
        j["night"]["awake_s"].as_f64(),
        Some(1500.0),
        "the watch's stages"
    );
    assert_eq!(j["night"]["hrv_ms"].as_f64(), Some(48.0));
}

#[tokio::test]
async fn the_watch_wins_whatever_the_import_order() {
    // A: the watch's night first, then Connect's. B: Connect's first, then the watch's.
    let (a, ca) = setup().await;
    watch_night(&a, (z(27, 21, 10), z(28, 5, 10)), &watch_levels(), &[]).await;
    import(&a);
    let (b, cb) = setup().await;
    import(&b);
    // Before the watch file arrives, Connect's stages are what the page shows.
    let before = b.get(&cb, "/api/sleep/2026-11-28").await;
    assert_eq!(sleep_api_stages(&before).len(), 4);
    assert_eq!(before["night"]["rem_s"].as_f64(), Some(18000.0));
    watch_night(&b, (z(27, 21, 10), z(28, 5, 10)), &watch_levels(), &[]).await;
    // Both sets are in the table now (derive time cannot know), but not in what is read.
    assert_eq!(count(&b.pool, "SELECT COUNT(*) FROM sleep_levels").await, 8);

    for view in [
        "sleep_levels_current",
        "hrv_values_current",
        "hrv_summary_current",
    ] {
        let cols = match view {
            "sleep_levels_current" => "level",
            "hrv_values_current" => "rmssd_ms",
            _ => "last_night_avg",
        };
        assert_eq!(
            dump(&a.pool, view, cols).await,
            dump(&b.pool, view, cols).await,
            "{view}"
        );
    }
    let (ja, jb) = (
        a.get(&ca, "/api/sleep/2026-11-28").await,
        b.get(&cb, "/api/sleep/2026-11-28").await,
    );
    assert_eq!(ja, jb);
    assert_eq!(sleep_api_stages(&jb).len(), 4);
    // The watch's totals (awake 25 minutes), not Connect's (30 minutes).
    assert_eq!(jb["night"]["awake_s"].as_f64(), Some(1500.0));
    assert_eq!(jb["night"]["deep_s"].as_f64(), Some(5400.0));
}

#[tokio::test]
async fn a_watch_night_without_stage_rows_shows_the_connect_stages() {
    let (e, cookie) = setup().await;
    // The watch has the night (a session) but never delivered its stage file.
    watch_night(&e, (z(27, 21, 10), z(28, 5, 10)), &[], &[]).await;
    import(&e);
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM sleep_levels_current").await,
        4
    );
    let j = e.get(&cookie, "/api/sleep/2026-11-28").await;
    assert_eq!(
        j["night"]["score"].as_f64(),
        Some(70.0),
        "the watch's session"
    );
    assert_eq!(j["night"]["stages_available"], true);
    assert_eq!(sleep_api_stages(&j).len(), 4);
    assert_eq!(j["night"]["deep_s"].as_f64(), Some(5400.0));
}

#[tokio::test]
async fn watch_hrv_rows_hide_connects_hrv_and_the_import_is_the_same_after_reparse() {
    let (e, _) = setup().await;
    watch_night(
        &e,
        (z(27, 21, 10), z(28, 5, 10)),
        &[],
        &[(z(27, 23, 1), 60.0)],
    )
    .await;
    import(&e);
    // No Connect HRV row (values and summary) beside the watch's value; stages are Connect's.
    assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM hrv_values").await, 1);
    assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM sleep_levels").await, 4);

    // Connect only: a reparse rebuilds exactly the same rows.
    let (c, _) = setup().await;
    import(&c);
    let all = |pool: SqlitePool| async move {
        let mut out = Vec::new();
        for (t, cols) in [
            ("sleep_levels", "level, level_name"),
            ("hrv_values", "rmssd_ms"),
            ("hrv_summary", "last_night_avg"),
            ("mon_hr", "bpm"),
            ("respiration", "brpm"),
        ] {
            out.extend(dump(&pool, t, cols).await);
        }
        out.extend(
            rows(
                &pool,
                "SELECT start_ts || '|' || end_ts || '|' || asleep_s FROM sleep_sessions",
            )
            .await,
        );
        out
    };
    let before = all(c.pool.clone()).await;
    assert!(before.len() > 10);
    reparse(c.dir.path());
    assert_eq!(all(c.pool.clone()).await, before);
}
