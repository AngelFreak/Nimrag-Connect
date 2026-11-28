//! `GET /api/readiness`, `GET /api/readiness/history` and `GET /api/training/explain` against
//! hand-made rows: exact scores and ratings, the missing-data rules, no look into the future, and
//! an empty database.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use nimrag_server::derived::{rank_key, src_id};
use serde_json::{json, Value};

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

/// A 64-digit hex hash that is different for every day (the source id is read from its start).
fn sha(day: u32) -> String {
    format!("{day:02x}").repeat(32)
}

/// The night that ends on the morning of `day` (22:30 to 06:30), with this score.
async fn night(e: &Env, day: u32, score: f64) {
    let sha = sha(day);
    let (start, end) = (at(day - 1, 22, 30), at(day, 6, 30));
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
    e.exec(&format!(
        "INSERT INTO sleep_sessions (sha256, idx, start_ts, end_ts, overall_score) VALUES ('{sha}', 0, {start}, {end}, {score})"
    ))
    .await;
}

/// The seven nights ending 13 to 19 Nov (average 73.86, last night 85), stress 30 on 16 to 18, an
/// HRV summary on the 19th (balanced, last night under the middle of the band), a load of 250
/// inside 200 to 400, and a recovery time that ended long ago.
async fn full_week(e: &Env) {
    for (day, score) in [
        (13, 70.0),
        (14, 74.0),
        (15, 70.0),
        (16, 74.0),
        (17, 70.0),
        (18, 74.0),
        (19, 85.0),
    ] {
        night(e, day, score).await;
    }
    for day in [16, 17, 18] {
        e.exec(&format!(
            "INSERT INTO stress (ts, stress, stress_raw, body_battery, src) VALUES ({}, 30, 30, 60, 1)",
            at(day, 12, 0)
        ))
        .await;
    }
    e.exec(&format!(
        "INSERT INTO hrv_summary (ts, weekly_avg, last_night_avg, last_night_5min_high, baseline_balanced_lower, baseline_balanced_upper, status, src)
         VALUES ({}, 60, 55, 70, 50, 70, 4, 1)",
        at(19, 6, 5)
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO training_status (ts, status, weekly_load_sum, load_min, load_max, fitness_trend, load_trend, src)
         VALUES ({}, 7, 250, 200, 400, 1, 1, 1)",
        at(19, 6, 30)
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO recovery_time (start_ts, minutes, src) VALUES ({}, 0, 1)",
        at(19, 6, 0)
    ))
    .await;
}

fn factor<'a>(v: &'a Value, key: &str) -> &'a Value {
    v["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == key)
        .unwrap_or_else(|| panic!("factor {key} in {v}"))
}

#[tokio::test]
async fn an_empty_account_has_no_score_and_says_why() {
    let (e, c) = setup().await;
    let r = e.get(&c, "/api/readiness").await;
    assert_eq!(r["date"], "2026-11-19");
    assert!(r["score"].is_null() && r["level"].is_null() && r["headline"].is_null());
    assert_eq!(r["present"], 0);
    assert_eq!(r["data_days"], 0);
    assert_eq!(r["factors"].as_array().unwrap().len(), 6);
    assert!(r["factors"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["rating"].is_null() && f["value"].is_null()));
    assert!(
        r["unavailable"]
            .as_str()
            .unwrap()
            .contains("3 days of data"),
        "{r}"
    );
    let h = e
        .get(&c, "/api/readiness/history?from=2026-11-17&to=2026-11-19")
        .await;
    assert!(h["average"].is_null());
    assert_eq!(h["days"].as_array().unwrap().len(), 3);
    assert!(h["days"][0]["score"].is_null());
}

#[tokio::test]
async fn a_full_week_gives_the_exact_score_factors_and_words() {
    let (e, c) = setup().await;
    full_week(&e).await;
    let r = e.get(&c, "/api/readiness").await;
    assert_eq!(r["sample"], false);
    // sleep 85 good 75*25, recovery 0 h very good 100*20, HRV balanced good 75*20, load good 75*15,
    // history 73.9 moderate 50*10, stress 30 good 75*10: 7750 / 100.
    assert_eq!(r["score"], 78, "{r}");
    assert_eq!(r["level"], "high");
    assert_eq!(r["present"], 6);
    assert_eq!(r["data_days"], 7);
    assert!(r["unavailable"].is_null());

    let sleep = factor(&r, "sleep");
    assert_eq!(
        (
            sleep["value"].as_f64(),
            sleep["rating"].as_str(),
            sleep["weight"].as_u64()
        ),
        (Some(85.0), Some("good"), Some(25))
    );
    let rec = factor(&r, "recovery");
    assert_eq!(
        (
            rec["value"].as_f64(),
            rec["rating"].as_str(),
            rec["unit"].as_str()
        ),
        (Some(0.0), Some("very_good"), Some("h"))
    );
    let hrv = factor(&r, "hrv");
    assert_eq!(
        (hrv["value"].as_f64(), hrv["rating"].as_str()),
        (Some(60.0), Some("good"))
    );
    assert_eq!(hrv["note"], "Balanced range 50–70 ms");
    let load = factor(&r, "load");
    assert_eq!(
        (load["value"].as_f64(), load["rating"].as_str()),
        (Some(250.0), Some("good"))
    );
    assert_eq!(load["note"], "Target range 200–400");
    let hist = factor(&r, "sleep_history");
    assert_eq!(
        (hist["value"].as_f64(), hist["rating"].as_str()),
        (Some(73.9), Some("moderate"))
    );
    let stress = factor(&r, "stress");
    assert_eq!(
        (stress["value"].as_f64(), stress["rating"].as_str()),
        (Some(30.0), Some("good"))
    );

    // The weakest factor (sleep history, moderate) is named.
    assert_eq!(r["headline_key"], "high_sleep");
    assert_eq!(r["headline"], "Ready for a solid session");
    assert!(
        r["sentence"]
            .as_str()
            .unwrap()
            .contains("your sleep over the past week"),
        "{r}"
    );
}

#[tokio::test]
async fn missing_factors_leave_the_score_to_the_rest_and_four_are_enough() {
    let (e, c) = setup().await;
    full_week(&e).await;
    e.exec("DELETE FROM hrv_summary").await;
    e.exec("DELETE FROM recovery_time").await;
    let r = e.get(&c, "/api/readiness").await;
    assert_eq!(r["present"], 4);
    assert!(factor(&r, "hrv")["rating"].is_null());
    // 75*25 + 75*15 + 50*10 + 75*10 = 4250 over 60 = 70.8.
    assert_eq!(r["score"], 71, "{r}");
    assert_eq!(r["level"], "moderate");
    // Without the load row only three are left: no score.
    e.exec("DELETE FROM training_status").await;
    let r = e.get(&c, "/api/readiness").await;
    assert_eq!(r["present"], 3);
    assert!(r["score"].is_null());
    assert!(
        r["unavailable"]
            .as_str()
            .unwrap()
            .contains("only 3 are available"),
        "{r}"
    );
}

#[tokio::test]
async fn history_does_not_look_ahead_and_gives_the_average() {
    let (e, c) = setup().await;
    full_week(&e).await;
    // The HRV, load and recovery rows (a 0 minute timer) are all from the 19th: on the 17th and 18th only the sleep,
    // history and stress factors exist, which is too few.
    let h = e
        .get(&c, "/api/readiness/history?from=2026-11-17&to=2026-11-19")
        .await;
    let days = h["days"].as_array().unwrap();
    assert_eq!(days.len(), 3);
    assert!(days[0]["score"].is_null() && days[1]["score"].is_null());
    assert_eq!(
        (
            days[2]["date"].as_str(),
            days[2]["score"].as_u64(),
            days[2]["level"].as_str()
        ),
        (Some("2026-11-19"), Some(78), Some("high"))
    );
    assert_eq!(h["average"], 78.0);
    // The same day through both endpoints agrees.
    let one = e.get(&c, "/api/readiness?date=2026-11-19").await;
    assert_eq!(one["score"], 78);
}

#[tokio::test]
async fn bad_dates_are_400() {
    let (e, c) = setup().await;
    for uri in [
        "/api/readiness?date=2026-11-20",
        "/api/readiness?date=nope",
        "/api/readiness/history?from=2026-11-20&to=2026-11-21",
        "/api/readiness/history?from=2026-11-19&to=2026-11-10",
    ] {
        let (s, _, _) = e.call(Method::GET, uri, Some(&c), false, None).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{uri}");
    }
}

// ---- training status explained ------------------------------------------------------------------

#[tokio::test]
async fn explain_without_data_has_no_sentence() {
    let (e, c) = setup().await;
    let x = e.get(&c, "/api/training/explain").await;
    assert!(x["status"].is_null() && x["sentence"].is_null() && x["meaning"].is_null());
    assert_eq!(x["factors"].as_array().unwrap().len(), 3);
    assert!(x["factors"][0]["value"].is_null());
    assert_eq!(x["focus"].as_array().unwrap().len(), 0);
    assert!(x["short_of"].is_null());
}

#[tokio::test]
async fn explain_names_why_with_the_figures_behind_it() {
    let (e, c) = setup().await;
    full_week(&e).await; // productive, load 250 in 200 to 400 (optimal), HRV balanced
                         // VO2 max: 14.0 MET (49.0) forty days ago, 14.4 MET (50.4) three days ago: +1.4, rising.
    e.exec(&format!(
        "INSERT INTO vo2max (ts, maximal_met, src) VALUES ({}, 14.0, 1), ({}, 14.4, 1)",
        at(19, 6, 0) - 40 * 86_400,
        at(16, 6, 0)
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO training_balance (ts, aerobic_low, aerobic_high, anaerobic, aerobic_low_target_min, aerobic_low_target_max,
            aerobic_high_target_min, aerobic_high_target_max, anaerobic_target_min, anaerobic_target_max, src)
         VALUES ({}, 300, 250, 20, 200, 400, 150, 300, 60, 180, 1)",
        at(19, 6, 30)
    ))
    .await;
    let x = e.get(&c, "/api/training/explain").await;
    assert_eq!(x["status"]["label"], "Productive");
    assert_eq!(x["sentence_key"], "productive_working");
    assert!(
        x["sentence"]
            .as_str()
            .unwrap()
            .contains("VO2 max is rising"),
        "{x}"
    );
    assert_eq!(x["meaning"]["title"], "Productive");

    let f = |key: &str| {
        x["factors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["key"] == key)
            .unwrap()
    };
    assert_eq!(
        (
            f("vo2max")["value"].as_f64(),
            f("vo2max")["change"].as_f64(),
            f("vo2max")["word"].as_str()
        ),
        (Some(50.4), Some(1.4), Some("Rising"))
    );
    assert_eq!(
        (
            f("load")["value"].as_f64(),
            f("load")["word"].as_str(),
            f("load")["detail"].as_str()
        ),
        (Some(250.0), Some("Optimal"), Some("Target range 200–400"))
    );
    assert_eq!(
        (f("hrv")["value"].as_f64(), f("hrv")["word"].as_str()),
        (Some(60.0), Some("Balanced"))
    );

    // Anaerobic is the only bucket short of its target.
    assert_eq!(x["short_of"], "anaerobic");
    let focus = x["focus"].as_array().unwrap();
    assert_eq!(focus.len(), 3);
    assert_eq!(focus[2]["state"], "short");
    assert_eq!(focus[0]["state"], "in_range");
}

/// A Connect night brings an HRV summary with only the night's average, and the watch gives no
/// 7-day load (only its acute load): HRV and load still have their figures, ranges and ratings.
#[tokio::test]
async fn a_night_only_hrv_row_and_an_acute_load_keep_the_hrv_and_load_factors() {
    let (e, c) = setup().await;
    full_week(&e).await;
    e.exec("DELETE FROM hrv_summary").await;
    e.exec("DELETE FROM training_status").await;
    e.exec(&format!(
        "INSERT INTO hrv_summary (ts, weekly_avg, last_night_avg, baseline_balanced_lower, baseline_balanced_upper, status, src)
         VALUES ({}, 60, 58, 50, 70, 4, 1), ({}, NULL, 55, NULL, NULL, NULL, 2)",
        at(18, 6, 5),
        at(19, 6, 30)
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO training_load (ts, acute, chronic, src) VALUES ({}, 250, 250, 1)",
        at(19, 6, 30)
    ))
    .await;

    let r = e.get(&c, "/api/readiness").await;
    let hrv = factor(&r, "hrv");
    assert_eq!(hrv["value"], 60.0, "{r}");
    assert!(!hrv["rating"].is_null(), "{r}");
    let load = factor(&r, "load");
    assert_eq!(load["value"], 250.0, "{r}");
    assert!(!load["rating"].is_null(), "{r}");
    assert_eq!(load["note"], "Target range 200–375", "{r}");

    let t = e.get(&c, "/api/today").await;
    let h = &t["hrv"];
    assert_eq!(
        (
            h["last_night_ms"].as_f64(),
            h["weekly_avg_ms"].as_f64(),
            h["baseline_low_ms"].as_f64(),
            h["baseline_high_ms"].as_f64()
        ),
        (Some(55.0), Some(60.0), Some(50.0), Some(70.0)),
        "{h}"
    );
    assert!(!h["status"].is_null(), "{h}");
}
