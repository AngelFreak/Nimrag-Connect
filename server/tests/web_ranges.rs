//! The range endpoints behind the Training status, HRV status and Sleep score pages:
//! `GET /api/training/status`, `GET /api/hrv` and `GET /api/sleep/history`, against hand-made rows
//! (exact values, gaps, rows that could come from an import) and an empty database.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use nimrag_server::derived::{rank_key, src_id};
use serde_json::json;

fn at(day: u32, h: u32, m: u32) -> i64 {
    cph(2026, 11, day, h, m)
}

/// Pinned to Thursday 2026-11-19 20:00 in Copenhagen (UTC+1), owner signed in, zone set.
async fn setup() -> (Env, String) {
    let e = env_at(cph(2026, 11, 19, 20, 0)).await;
    let cookie = e.login().await;
    let (s, _) = e
        .send(
            &cookie,
            Method::PATCH,
            "/api/settings",
            json!({ "timezone": "Europe/Copenhagen" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    (e, cookie)
}

async fn status_row(e: &Env, ts: i64, status: &str) {
    e.exec(&format!(
        "INSERT INTO training_status (ts, status, weekly_load_sum, load_min, load_max, fitness_trend, load_trend, src)
         VALUES ({ts}, {status}, 300, 240, 420, 1, 1, 1)"
    ))
    .await;
}

async fn status_of(e: &Env, c: &str, uri: &str) -> StatusCode {
    e.call(Method::GET, uri, Some(c), false, None).await.0
}

// ---- training status ---------------------------------------------------------------------------

#[tokio::test]
async fn training_status_days_runs_legend_and_current() {
    let (e, c) = setup().await;
    // 14 Recovery, 15 and 16 Strained, 17 no reading, 18 Recovery, 19 Recovery then (later that
    // day) Maintaining: the last reading of a day is the day's status.
    status_row(&e, at(14, 6, 30), "5").await;
    status_row(&e, at(15, 6, 30), "8").await;
    status_row(&e, at(16, 6, 30), "8").await;
    status_row(&e, at(18, 6, 30), "5").await;
    status_row(&e, at(19, 6, 30), "5").await;
    status_row(&e, at(19, 18, 0), "4").await;
    e.exec(&format!(
        "INSERT INTO training_load (ts, acute, chronic, acwr, acwr_percent, status, src) VALUES ({}, 310, 280, 1.1, 55, 2, 1)",
        at(19, 6, 30)
    ))
    .await;

    let t = e
        .get(&c, "/api/training/status?from=2026-11-14&to=2026-11-19")
        .await;
    assert_eq!(t["sample"], false);
    assert_eq!(
        (t["from"].as_str(), t["to"].as_str(), t["today"].as_str()),
        (Some("2026-11-14"), Some("2026-11-19"), Some("2026-11-19"))
    );
    let days = t["days"].as_array().unwrap();
    assert_eq!(days.len(), 6);
    let labels: Vec<Option<&str>> = days.iter().map(|d| d["status"]["label"].as_str()).collect();
    assert_eq!(
        labels,
        vec![
            Some("Recovery"),
            Some("Strained"),
            Some("Strained"),
            None,
            Some("Recovery"),
            Some("Maintaining")
        ]
    );
    assert_eq!(days[0]["status"]["kind"], "recovery");
    assert_eq!(days[1]["status"]["code"], 8);
    assert!(
        days[3]["status"].is_null(),
        "no reading is null, not a status"
    );

    // Newest first; a day without a reading ends a run.
    let runs: Vec<(String, String, String, u64, bool, bool)> = t["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["status"]["label"].as_str().unwrap().to_owned(),
                r["from"].as_str().unwrap().to_owned(),
                r["to"].as_str().unwrap().to_owned(),
                r["days"].as_u64().unwrap(),
                r["starts_before_range"].as_bool().unwrap(),
                r["ongoing"].as_bool().unwrap(),
            )
        })
        .collect();
    let run = |l: &str, a: &str, b: &str, n: u64, early: bool, ongoing: bool| {
        (l.to_owned(), a.to_owned(), b.to_owned(), n, early, ongoing)
    };
    assert_eq!(
        runs,
        vec![
            run("Maintaining", "2026-11-19", "2026-11-19", 1, false, true),
            run("Recovery", "2026-11-18", "2026-11-18", 1, false, false),
            run("Strained", "2026-11-15", "2026-11-16", 2, false, false),
            run("Recovery", "2026-11-14", "2026-11-14", 1, false, false),
        ]
    );

    let legend: Vec<(&str, &str)> = t["legend"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| (l["label"].as_str().unwrap(), l["kind"].as_str().unwrap()))
        .collect();
    assert_eq!(
        legend,
        vec![
            ("Recovery", "recovery"),
            ("Strained", "strained"),
            ("Maintaining", "maintaining"),
            ("Productive", "productive"),
            ("Detraining", "detraining"),
            ("No status", "none"),
        ]
    );
    // The current card carries the name; the load figures come with it.
    assert_eq!(t["current"]["status_code"], 4);
    assert_eq!(t["current"]["label"], "Maintaining");
    assert_eq!(t["current"]["kind"], "maintaining");
    assert_eq!(t["current"]["since"], "2026-11-19");
    assert_eq!(t["load"]["acute"], 310.0);
    assert_eq!(t["load"]["ratio"], 1.1);
}

#[tokio::test]
async fn a_run_that_continues_from_before_the_range_says_so() {
    let (e, c) = setup().await;
    status_row(&e, at(12, 6, 30), "5").await;
    status_row(&e, at(13, 6, 30), "5").await;
    status_row(&e, at(14, 6, 30), "5").await;
    status_row(&e, at(15, 6, 30), "8").await;
    let t = e
        .get(&c, "/api/training/status?from=2026-11-14&to=2026-11-15")
        .await;
    assert_eq!(t["runs"][1]["status"]["label"], "Recovery");
    assert_eq!(t["runs"][1]["starts_before_range"], true);
    assert_eq!(t["runs"][0]["starts_before_range"], false);
    // The status the day before the range is another one: the run starts where the range does.
    let t = e
        .get(&c, "/api/training/status?from=2026-11-15&to=2026-11-15")
        .await;
    assert_eq!(t["runs"][0]["starts_before_range"], false);
    // A day without a reading before the range ends the run, as it does inside the range.
    status_row(&e, at(10, 6, 30), "8").await;
    let t = e
        .get(&c, "/api/training/status?from=2026-11-11&to=2026-11-13")
        .await;
    assert_eq!(t["runs"][0]["from"], "2026-11-12");
    assert_eq!(t["runs"][0]["starts_before_range"], false);
}

#[tokio::test]
async fn training_status_names_only_the_verified_codes() {
    let (e, c) = setup().await;
    // 1 Detraining, 7 Productive and 0 no status are verified or defined; 2, 3, 6 and 9 are not.
    for (d, code) in [
        (13, "1"),
        (14, "7"),
        (15, "0"),
        (16, "2"),
        (17, "3"),
        (18, "6"),
        (19, "9"),
    ] {
        status_row(&e, at(d, 6, 30), code).await;
    }
    let t = e
        .get(&c, "/api/training/status?from=2026-11-13&to=2026-11-19")
        .await;
    let got: Vec<(&str, &str)> = t["days"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["status"]["label"].as_str().unwrap(),
                d["status"]["kind"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            ("Detraining", "detraining"),
            ("Productive", "productive"),
            ("No status", "none"),
            ("Status 2", "unknown"),
            ("Status 3", "unknown"),
            ("Status 6", "unknown"),
            ("Status 9", "unknown"),
        ]
    );
    assert_eq!(t["current"]["label"], "Status 9");
    // The number of a code that has no name is kept.
    assert_eq!(t["days"][3]["status"]["code"], 2);
}

#[tokio::test]
async fn training_status_range_rules() {
    let (e, c) = setup().await;
    // Defaults: the week ending today. Nothing recorded: every day empty, no runs.
    let t = e.get(&c, "/api/training/status").await;
    assert_eq!(
        (t["from"].as_str(), t["to"].as_str()),
        (Some("2026-11-13"), Some("2026-11-19"))
    );
    let days = t["days"].as_array().unwrap();
    assert_eq!(days.len(), 7);
    assert!(days.iter().all(|d| d["status"].is_null()));
    assert_eq!(t["runs"], json!([]));
    assert!(t["current"]["status_code"].is_null() && t["current"]["label"].is_null());
    // A `to` in the future is cut at today.
    let t = e
        .get(&c, "/api/training/status?from=2026-11-17&to=2026-12-25")
        .await;
    assert_eq!(t["to"], "2026-11-19");
    for bad in [
        "/api/training/status?from=2026-11-19&to=2026-11-18",
        "/api/training/status?from=2026-11-20",
        "/api/training/status?from=nope",
        "/api/training/status?from=2025-10-15&to=2026-11-19",
    ] {
        assert_eq!(
            status_of(&e, &c, bad).await,
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
    // 400 days is the limit.
    assert_eq!(
        status_of(&e, &c, "/api/training/status?from=2025-10-16&to=2026-11-19").await,
        StatusCode::OK
    );
}

// ---- HRV ---------------------------------------------------------------------------------------

#[tokio::test]
async fn hrv_days_baseline_overnight_average_and_the_night() {
    let (e, c) = setup().await;
    let cols = "ts, weekly_avg, last_night_avg, last_night_5min_high, baseline_balanced_lower, baseline_balanced_upper, status, src";
    for row in [
        format!("{}, 57, 40, 60, 53, 70, 9, 1", at(15, 6, 5)), // a code nobody has named
        format!("{}, 60, 58, 80, 52, 68, 4, 1", at(16, 6, 5)), // balanced
        format!("{}, 59, 50, 70, 53, 69, 2, 1", at(17, 6, 5)), // low
        format!("{}, 58, 54, 72, 53, 70, 3, 1", at(19, 6, 5)), // unbalanced
    ] {
        e.exec(&format!("INSERT INTO hrv_summary ({cols}) VALUES ({row})"))
            .await;
    }
    // The night of 17 to 18 Nov has readings only (no summary): 22:30 to 05:30, three values.
    for (ts, v) in [
        (at(17, 22, 30), 40.0),
        (at(18, 2, 0), 50.0),
        (at(18, 5, 30), 60.0),
    ] {
        e.exec(&format!(
            "INSERT INTO hrv_values (ts, rmssd_ms, src) VALUES ({ts}, {v}, 1)"
        ))
        .await;
    }
    let h = e.get(&c, "/api/hrv?from=2026-11-15&to=2026-11-19").await;
    assert_eq!(h["today"], "2026-11-19");
    let days = h["days"].as_array().unwrap();
    assert_eq!(days.len(), 5);
    assert_eq!(days[0]["status_code"], 9);
    assert!(days[0]["status"].is_null() && days[0]["status_label"].is_null());
    assert_eq!(
        (
            days[1]["status"].as_str(),
            days[1]["status_label"].as_str(),
            days[1]["weekly_avg_ms"].as_f64()
        ),
        (Some("balanced"), Some("Balanced"), Some(60.0))
    );
    assert_eq!(days[2]["status"], "low");
    // 18 Nov: no summary, the overnight value is the mean of the night's readings.
    assert_eq!(days[3]["date"], "2026-11-18");
    assert_eq!(days[3]["last_night_ms"], 50.0);
    assert!(days[3]["status_code"].is_null() && days[3]["weekly_avg_ms"].is_null());
    assert_eq!(days[4]["status"], "unbalanced");
    assert_eq!(days[4]["last_night_5min_high_ms"], 72.0);
    // Overnight average over the days that have one: 40, 58, 50, 50 (readings) and 54.
    assert_eq!(h["nights"], 5);
    assert_eq!(h["overnight_avg_ms"], 50.4);
    // The balanced range of the latest day that has one.
    assert_eq!(
        (
            h["baseline_low_ms"].as_f64(),
            h["baseline_high_ms"].as_f64()
        ),
        (Some(53.0), Some(70.0))
    );
    assert!(h["night"].is_null(), "readings only for a one-day range");

    // One day: the readings of that night come with it.
    let d = e.get(&c, "/api/hrv?from=2026-11-18&to=2026-11-18").await;
    let n = &d["night"];
    assert_eq!(n["readings"].as_array().unwrap().len(), 3);
    assert_eq!(n["readings"][0]["time"], "2026-11-17T22:30:00+01:00");
    assert_eq!(
        (
            n["avg_ms"].as_f64(),
            n["low_ms"].as_f64(),
            n["high_ms"].as_f64()
        ),
        (Some(50.0), Some(40.0), Some(60.0))
    );
    assert_eq!(d["days"][0]["last_night_ms"], 50.0);
}

#[tokio::test]
async fn hrv_poor_status_and_an_empty_range() {
    let (e, c) = setup().await;
    e.exec(&format!(
        "INSERT INTO hrv_summary (ts, last_night_avg, status, src) VALUES ({}, 30, 1, 1)",
        at(19, 6, 5)
    ))
    .await;
    let h = e.get(&c, "/api/hrv?from=2026-11-19&to=2026-11-19").await;
    assert_eq!(h["days"][0]["status"], "poor");
    assert_eq!(h["days"][0]["status_label"], "Poor");
    assert!(h["baseline_low_ms"].is_null() && h["night"].is_null());
    let h = e.get(&c, "/api/hrv?from=2026-11-01&to=2026-11-10").await;
    assert_eq!(h["nights"], 0);
    assert!(h["overnight_avg_ms"].is_null());
    assert_eq!(h["days"].as_array().unwrap().len(), 10);
    assert_eq!(
        status_of(&e, &c, "/api/hrv?from=2026-11-19&to=2026-11-01").await,
        StatusCode::BAD_REQUEST
    );
}

// ---- sleep history -----------------------------------------------------------------------------

fn sha(c: char) -> String {
    c.to_string().repeat(64)
}

/// A night starting at `start` and lasting `hours`, without stage rows (so time asleep is the span).
async fn night(e: &Env, id: char, start: i64, hours: f64, score: Option<f64>) {
    let sha = sha(id);
    let end = start + (hours * 3600.0) as i64;
    e.exec(&format!(
        "INSERT INTO files (sha256, size, received_at, source) VALUES ('{sha}', 1, '2026-11-19T00:00:00.000Z', 'watch')"
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO sources (sha256, src_id, ok, last_ts, msg_count, rank_key) VALUES ('{sha}', {}, 1, {end}, 1, '{}')",
        src_id(&sha),
        rank_key(true, Some(end), 1)
    ))
    .await;
    let score = score.map_or("NULL".to_owned(), |s| s.to_string());
    e.exec(&format!(
        "INSERT INTO sleep_sessions (sha256, idx, start_ts, end_ts, overall_score) VALUES ('{sha}', 0, {start}, {end}, {score})"
    ))
    .await;
}

async fn six_nights(e: &Env) {
    // Nights by the date they begin on: Oct 30 (8 h, 90), Nov 2 (6 h, 60), Nov 12 (8 h 30, 80),
    // Nov 13 (7 h, 70), Nov 15 (7 h 30, no score), Nov 18 (9 h, 88).
    night(e, 'a', cph(2026, 10, 30, 22, 0), 8.0, Some(90.0)).await;
    night(e, 'b', at(2, 23, 0), 6.0, Some(60.0)).await;
    night(e, 'c', at(12, 22, 0), 8.5, Some(80.0)).await;
    night(e, 'd', at(13, 23, 0), 7.0, Some(70.0)).await;
    night(e, 'e', at(15, 23, 30), 7.5, None).await;
    night(e, 'f', at(18, 22, 0), 9.0, Some(88.0)).await;
}

#[tokio::test]
async fn sleep_history_by_day_keeps_gaps_visible() {
    let (e, c) = setup().await;
    six_nights(&e).await;
    let h = e
        .get(&c, "/api/sleep/history?from=2026-11-12&to=2026-11-19")
        .await;
    assert_eq!(h["group"], "day");
    assert_eq!(h["long_night_s"], 28800.0);
    assert_eq!(h["today"], "2026-11-19");
    let b = h["buckets"].as_array().unwrap();
    assert_eq!(b.len(), 8);
    assert_eq!(
        (b[0]["from"].as_str(), b[0]["to"].as_str()),
        (Some("2026-11-12"), Some("2026-11-12"))
    );
    assert_eq!(
        (b[0]["avg_score"].as_f64(), b[0]["avg_duration_s"].as_f64()),
        (Some(80.0), Some(30600.0))
    );
    assert_eq!(b[1]["avg_duration_s"], 25200.0);
    assert_eq!(b[2]["nights"], 0, "a missing night is an empty bucket");
    assert!(b[2]["avg_score"].is_null() && b[2]["avg_duration_s"].is_null());
    assert_eq!(b[3]["nights"], 1);
    assert!(b[3]["avg_score"].is_null(), "recorded but without a score");
    assert_eq!(b[3]["avg_duration_s"], 27000.0);
    assert_eq!(b[6]["avg_score"], 88.0);
    assert_eq!(b[7]["nights"], 0);
    let s = &h["stats"];
    assert_eq!(
        (
            s["nights"].as_u64(),
            s["scored_nights"].as_u64(),
            s["long_nights"].as_u64()
        ),
        (Some(4), Some(3), Some(2))
    );
    assert_eq!(
        (
            s["avg_score"].as_f64(),
            s["highest_score"].as_f64(),
            s["lowest_score"].as_f64()
        ),
        (Some(79.3), Some(88.0), Some(70.0))
    );
    assert_eq!(s["avg_duration_s"], 28800.0);
}

#[tokio::test]
async fn sleep_history_by_week_and_month() {
    let (e, c) = setup().await;
    six_nights(&e).await;
    // Weeks start on Monday and are clipped to the range: 2 to 8, 9 to 15, 16 to 19 Nov.
    let w = e
        .get(
            &c,
            "/api/sleep/history?from=2026-11-02&to=2026-11-19&group=week",
        )
        .await;
    let b = w["buckets"].as_array().unwrap();
    assert_eq!(b.len(), 3);
    assert_eq!(
        (b[2]["from"].as_str(), b[2]["to"].as_str()),
        (Some("2026-11-16"), Some("2026-11-19"))
    );
    assert_eq!(
        (
            b[0]["nights"].as_u64(),
            b[0]["avg_score"].as_f64(),
            b[0]["avg_duration_s"].as_f64()
        ),
        (Some(1), Some(60.0), Some(21600.0))
    );
    assert_eq!(
        (
            b[1]["nights"].as_u64(),
            b[1]["avg_score"].as_f64(),
            b[1]["high_score"].as_f64(),
            b[1]["low_score"].as_f64(),
            b[1]["avg_duration_s"].as_f64()
        ),
        (Some(3), Some(75.0), Some(80.0), Some(70.0), Some(27600.0))
    );
    let s = &w["stats"];
    assert_eq!(
        (
            s["avg_score"].as_f64(),
            s["highest_score"].as_f64(),
            s["lowest_score"].as_f64()
        ),
        (Some(74.5), Some(88.0), Some(60.0))
    );
    // Mean of the weekly highest (60, 80, 88) and lowest (60, 70, 88) scores.
    assert_eq!(
        (s["avg_highest"].as_f64(), s["avg_lowest"].as_f64()),
        (Some(76.0), Some(72.7))
    );

    // Calendar months, clipped: 30 and 31 Oct, then 1 to 19 Nov.
    let m = e
        .get(
            &c,
            "/api/sleep/history?from=2026-10-30&to=2026-11-19&group=month",
        )
        .await;
    let b = m["buckets"].as_array().unwrap();
    assert_eq!(b.len(), 2);
    assert_eq!(
        (b[0]["from"].as_str(), b[0]["to"].as_str()),
        (Some("2026-10-30"), Some("2026-10-31"))
    );
    assert_eq!(
        (b[0]["nights"].as_u64(), b[0]["avg_score"].as_f64()),
        (Some(1), Some(90.0))
    );
    assert_eq!(
        (b[1]["nights"].as_u64(), b[1]["avg_score"].as_f64()),
        (Some(5), Some(74.5))
    );
    assert_eq!(m["stats"]["nights"], 6);

    assert_eq!(
        status_of(&e, &c, "/api/sleep/history?group=year").await,
        StatusCode::BAD_REQUEST
    );
    // The night endpoint next to it still takes a date.
    assert_eq!(
        status_of(&e, &c, "/api/sleep/2026-11-18").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn range_endpoints_on_an_empty_server_are_empty_not_zero() {
    let (e, c) = setup().await;
    let h = e
        .get(
            &c,
            "/api/sleep/history?from=2026-11-01&to=2026-11-19&group=month",
        )
        .await;
    assert_eq!(h["stats"]["nights"], 0);
    assert!(h["stats"]["avg_score"].is_null() && h["stats"]["avg_duration_s"].is_null());
    assert!(h["buckets"][0]["avg_score"].is_null());
    assert_eq!(h["buckets"].as_array().unwrap().len(), 1);
}
