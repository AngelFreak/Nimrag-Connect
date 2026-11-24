//! `GET /api/health-status` and `GET /api/health-status/history` against hand-made nights: the
//! usual range from the 28 earlier nights, the 14 night threshold, the date rules and an empty
//! account.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use nimrag_server::derived::{rank_key, src_id};
use serde_json::{json, Value};

/// Night `n` ends on 2026-11-`n` at 06:30 (it started at 22:30 the evening before).
fn end_of(n: u32) -> i64 {
    cph(2026, 11, n, 6, 30)
}

async fn setup(day: u32) -> (Env, String) {
    let e = env_at(cph(2026, 11, day, 20, 0)).await;
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

/// A night ending on the morning of `day`, with an HRV summary, a heart rate low, one breathing
/// reading and (when `spo2` is set) one blood oxygen reading inside it.
async fn night(e: &Env, day: u32, hrv: f64, low_hr: f64, brpm: f64, spo2: Option<f64>) {
    let sha = format!("{day:02x}").repeat(32);
    let end = end_of(day);
    let start = end - 8 * 3600;
    e.exec(&format!(
        "INSERT INTO files (sha256, size, received_at, source) VALUES ('{sha}', 1, '2026-11-30T00:00:00.000Z', 'watch')"
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO sources (sha256, src_id, ok, last_ts, msg_count, rank_key) VALUES ('{sha}', {}, 1, {end}, 1, '{}')",
        src_id(&sha),
        rank_key(true, Some(end), 1)
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO sleep_sessions (sha256, idx, start_ts, end_ts, overall_score) VALUES ('{sha}', 0, {start}, {end}, 80)"
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO hrv_summary (ts, weekly_avg, last_night_avg, last_night_5min_high, baseline_balanced_lower, baseline_balanced_upper, status, src)
         VALUES ({}, {hrv}, {hrv}, {hrv}, 40, 60, 4, 1)",
        end - 3600
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO mon_hr (ts, bpm, src) VALUES ({}, {low_hr}, 1), ({}, {}, 1)",
        start + 3600,
        start + 7200,
        low_hr + 10.0
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO respiration (ts, brpm, src) VALUES ({}, {brpm}, 1)",
        start + 3600
    ))
    .await;
    if let Some(p) = spo2 {
        e.exec(&format!(
            "INSERT INTO spo2 (ts, percent, src) VALUES ({}, {p}, 1)",
            start + 3600
        ))
        .await;
    }
}

fn metric<'a>(v: &'a Value, key: &str) -> &'a Value {
    v["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["key"] == key)
        .unwrap_or_else(|| panic!("metric {key} in {v}"))
}

/// Nights 1 to 20 alternate between two values, so every metric has a real spread, and night 21
/// is last night (the 21st of November at 20:00 "now").
async fn twenty_nights(e: &Env) {
    for n in 1..=20 {
        let up = n % 2 == 0;
        night(
            e,
            n,
            if up { 60.0 } else { 40.0 },
            if up { 52.0 } else { 48.0 },
            if up { 15.0 } else { 14.0 },
            if n % 4 == 0 { None } else { Some(95.0) },
        )
        .await;
    }
}

#[tokio::test]
async fn an_empty_account_has_no_night_and_nothing_to_compare() {
    let (e, c) = setup(21).await;
    let r = e.get(&c, "/api/health-status").await;
    assert_eq!(r["has_night"], false);
    assert_eq!(
        (r["outside"].as_u64(), r["compared"].as_u64()),
        (Some(0), Some(0))
    );
    assert_eq!(r["metrics"].as_array().unwrap().len(), 4);
    for m in r["metrics"].as_array().unwrap() {
        assert_eq!(m["state"], "no_reading", "{m}");
        assert!(m["value"].is_null() && m["low"].is_null());
        assert_eq!(m["nights_needed"], 14);
    }
    let h = e
        .get(
            &c,
            "/api/health-status/history?from=2026-11-15&to=2026-11-21",
        )
        .await;
    assert_eq!(h["metrics"][0]["days"].as_array().unwrap().len(), 7);
}

#[tokio::test]
async fn a_night_far_from_the_usual_is_outside_and_the_edges_are_inside() {
    let (e, c) = setup(21).await;
    twenty_nights(&e).await;
    // HRV alternates 40 and 60: mean 50, sd about 10.26, so the range is about 34.6 to 65.4.
    // Last night: 90 ms (above), lowest HR 50 (the mean of 48 and 52: inside), breathing 20 brpm
    // (above 14 to 15), blood oxygen 95 (no spread: 94 to 96, inside).
    night(&e, 21, 90.0, 50.0, 20.0, Some(95.0)).await;
    let r = e.get(&c, "/api/health-status").await;
    assert_eq!(
        (r["date"].as_str(), r["has_night"].as_bool()),
        (Some("2026-11-21"), Some(true))
    );
    let hrv = metric(&r, "hrv");
    assert_eq!(hrv["state"], "above");
    assert_eq!(hrv["value"], 90.0);
    assert_eq!(hrv["unit"], "ms");
    assert_eq!(hrv["nights"], 20);
    assert_eq!(hrv["nights_needed"], 0);
    assert!(
        hrv["low"].as_f64().unwrap() < 36.0 && hrv["high"].as_f64().unwrap() > 64.0,
        "{hrv}"
    );
    let hr = metric(&r, "resting_hr");
    assert_eq!(hr["state"], "in_range", "{hr}");
    assert_eq!(hr["value"], 50.0);
    assert_eq!(metric(&r, "breathing")["state"], "above");
    let o2 = metric(&r, "spo2");
    assert_eq!(o2["state"], "in_range");
    // Only 15 of the 20 nights had blood oxygen: still enough nights for a range.
    assert_eq!(o2["nights"], 15);
    assert_eq!(
        (r["outside"].as_u64(), r["compared"].as_u64()),
        (Some(2), Some(4))
    );
}

#[tokio::test]
async fn fewer_than_fourteen_nights_calibrates_and_says_how_many_are_missing() {
    let (e, c) = setup(21).await;
    for n in 9..=20 {
        night(&e, n, 50.0, 50.0, 14.0, Some(95.0)).await;
    }
    night(&e, 21, 80.0, 50.0, 14.0, Some(95.0)).await;
    let r = e.get(&c, "/api/health-status").await;
    let hrv = metric(&r, "hrv");
    assert_eq!(hrv["state"], "calibrating");
    assert!(hrv["low"].is_null() && hrv["high"].is_null());
    assert_eq!(
        (hrv["nights"].as_u64(), hrv["nights_needed"].as_u64()),
        (Some(12), Some(2))
    );
    assert_eq!(
        (r["outside"].as_u64(), r["compared"].as_u64()),
        (Some(0), Some(0))
    );
}

#[tokio::test]
async fn without_a_date_the_latest_recent_night_is_used_and_a_given_date_is_exact() {
    let (e, c) = setup(22).await;
    twenty_nights(&e).await;
    // Nothing for the 21st or the 22nd: the 20th is two days back, so today it is.
    let r = e.get(&c, "/api/health-status").await;
    assert_eq!(
        (r["date"].as_str(), r["has_night"].as_bool()),
        (Some("2026-11-22"), Some(false))
    );
    night(&e, 21, 50.0, 50.0, 14.5, Some(95.0)).await;
    let r = e.get(&c, "/api/health-status").await;
    assert_eq!(
        (r["date"].as_str(), r["has_night"].as_bool()),
        (Some("2026-11-21"), Some(true))
    );
    // A date asks for exactly that night.
    let r = e.get(&c, "/api/health-status?date=2026-11-20").await;
    assert_eq!(r["date"], "2026-11-20");
    assert_eq!(metric(&r, "hrv")["value"], 60.0);
    // Night 20 only has 19 earlier nights.
    assert_eq!(metric(&r, "hrv")["nights"], 19);
    let (s, _, _) = e
        .call(
            Method::GET,
            "/api/health-status?date=2026-11-23",
            Some(&c),
            false,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = e
        .call(
            Method::GET,
            "/api/health-status?date=soon",
            Some(&c),
            false,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_history_gives_each_night_its_own_range_and_limits_the_span() {
    let (e, c) = setup(22).await;
    twenty_nights(&e).await;
    night(&e, 21, 90.0, 50.0, 14.5, Some(95.0)).await;
    let h = e
        .get(
            &c,
            "/api/health-status/history?from=2026-11-14&to=2026-11-22",
        )
        .await;
    let hrv = h["metrics"][0].clone();
    assert_eq!(hrv["key"], "hrv");
    let days = hrv["days"].as_array().unwrap();
    assert_eq!(days.len(), 9);
    assert_eq!(days[0]["date"], "2026-11-14");
    // Night 14 had 13 earlier nights: still calibrating. Night 15 had 14: a range.
    assert_eq!(days[0]["state"], "calibrating");
    assert!(days[0]["low"].is_null());
    assert_eq!(days[1]["date"], "2026-11-15");
    assert_ne!(days[1]["state"], "calibrating");
    assert!(days[1]["low"].is_number());
    // The 21st is above its range, the 22nd has no night.
    assert_eq!(days[7]["state"], "above");
    assert_eq!(days[8]["state"], "no_reading");
    assert!(days[8]["value"].is_null());
    let (s, _, _) = e
        .call(
            Method::GET,
            "/api/health-status/history?from=2026-06-01&to=2026-11-22",
            Some(&c),
            false,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
