//! `GET /api/race-predictions` over hand-made runs: the blend, the 42-day window, the weekly
//! history, the change since 4 weeks and user isolation.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use nimrag_server::derived::{rank_key, src_id};
use nimrag_server::web_api::race_predictions::{blend, daniels_seconds, effort_estimate};
use serde_json::{json, Value};

fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} is not within {tol} of {b}");
}

fn at(day: u32, h: u32, m: u32) -> i64 {
    cph(2026, 11, day, h, m)
}

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

fn sha(c: char) -> String {
    c.to_string().repeat(64)
}

struct Spec {
    c: char,
    sport: i64,
    sub: i64,
    start: i64,
    /// Constant speed in m/s over `secs` seconds, one record every 10 s (none when 0).
    speed: f64,
    secs: i64,
    ascent: f64,
    owner: Option<i64>,
}

async fn insert(e: &Env, s: &Spec) -> String {
    let sha = sha(s.c);
    let dist = s.speed * s.secs as f64;
    let last = s.start + s.secs;
    e.exec(&format!(
        "INSERT INTO files (sha256, size, received_at, source) VALUES ('{sha}', 1, '2026-11-19T00:00:00.000Z', 'watch')"
    ))
    .await;
    if let Some(o) = s.owner {
        e.exec(&format!(
            "INSERT INTO file_owners (sha256, user_id) VALUES ('{sha}', {o})"
        ))
        .await;
    }
    e.exec(&format!(
        "INSERT INTO sources (sha256, src_id, ok, last_ts, msg_count, rank_key) VALUES ('{sha}', {}, 1, {last}, 1, '{}')",
        src_id(&sha),
        rank_key(true, Some(last), 1)
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO activities (sha256, activity_key, ts, start_ts, local_offset_s, total_timer_s, num_sessions, sport, sub_sport)
         VALUES ('{sha}', 'k:{}', {last}, {}, 3600, {}, 1, {}, {})",
        s.c, s.start, s.secs, s.sport, s.sub
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO activity_sessions (sha256, idx, start_ts, sport, sub_sport, elapsed_s, timer_s, distance_m, ascent_m)
         VALUES ('{sha}', 0, {}, {}, {}, {}, {}, {dist}, {})",
        s.start, s.sport, s.sub, s.secs, s.secs, s.ascent
    ))
    .await;
    if s.speed > 0.0 {
        for i in 0..=s.secs / 10 {
            e.exec(&format!(
                "INSERT INTO activity_records (sha256, idx, ts, distance_m) VALUES ('{sha}', {i}, {}, {})",
                s.start + i * 10,
                s.speed * (i * 10) as f64
            ))
            .await;
        }
    }
    sha
}

fn run(c: char, day: u32, speed: f64, secs: i64, ascent: f64) -> Spec {
    Spec {
        c,
        sport: 1,
        sub: 0,
        start: at(day, 8, 0),
        speed,
        secs,
        ascent,
        owner: None,
    }
}

fn pred(j: &Value, metres: f64) -> &Value {
    j["predictions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["distance_m"].as_f64() == Some(metres))
        .unwrap_or_else(|| panic!("no prediction for {metres}"))
}

#[tokio::test]
async fn an_empty_account_has_no_estimate() {
    let (e, cookie) = setup().await;
    let j = e.get(&cookie, "/api/race-predictions").await;
    assert_eq!(j["basis"], Value::Null);
    assert_eq!(j["predictions"], json!([]));
    assert_eq!(j["history"], json!([]));
    assert_eq!(j["as_of"], "2026-11-19");
    // Not signed in.
    let (s, _, _) = e
        .call(Method::GET, "/api/race-predictions", None, false, None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn efforts_alone_use_riegel_from_the_three_closest() {
    let (e, cookie) = setup().await;
    // Mon 2 Nov: 5 km at 4 m/s. Wed 11 Nov: 4.5 m/s for 2222 s (9999 m: just short of 10 km).
    insert(&e, &run('a', 2, 4.0, 1250, 30.0)).await;
    insert(&e, &run('b', 11, 4.5, 2222, 20.0)).await;
    let j = e.get(&cookie, "/api/race-predictions").await;
    assert_eq!(j["basis"], "efforts");
    assert_eq!(j["vo2max"], Value::Null);
    assert_eq!(j["predictions"].as_array().unwrap().len(), 4);
    // Best efforts over both runs: 1 km, a mile and 5 km, all from run b.
    let efforts = [
        (1000.0, 1000.0 / 4.5),
        (1609.344, 1609.344 / 4.5),
        (5000.0, 5000.0 / 4.5),
    ];
    let expect = |d: f64| effort_estimate(&efforts, d).unwrap().round();
    for d in [5000.0, 10000.0, 21097.5, 42195.0] {
        let p = pred(&j, d);
        assert_eq!(p["seconds"].as_f64(), Some(expect(d)), "distance {d}");
        near(
            p["pace_s_per_km"].as_f64().unwrap(),
            expect(d) / (d / 1000.0),
            1e-9,
        );
    }
    // Four weeks ago (22 Oct) neither run had happened: no change to show.
    assert_eq!(pred(&j, 5000.0)["change_4w_s"], Value::Null);
}

#[tokio::test]
async fn vo2max_alone_and_the_blend() {
    let (e, cookie) = setup().await;
    // 14.4 MET is 50.4 ml/kg/min.
    e.exec(&format!(
        "INSERT INTO vo2max (ts, maximal_met, src) VALUES ({}, 14.4, 1)",
        at(16, 6, 0)
    ))
    .await;
    let j = e.get(&cookie, "/api/race-predictions").await;
    assert_eq!(j["basis"], "vo2max");
    near(j["vo2max"].as_f64().unwrap(), 50.4, 1e-9);
    let v = daniels_seconds(50.4, 5000.0).unwrap().round();
    assert_eq!(pred(&j, 5000.0)["seconds"].as_f64(), Some(v));

    // A 5 km run on Mon 9 Nov at 4 m/s: 70 % of its Riegel value and 30 % of the VO2 max value.
    insert(&e, &run('a', 9, 4.0, 1250, 30.0)).await;
    let j = e.get(&cookie, "/api/race-predictions").await;
    assert_eq!(j["basis"], "both");
    let efforts = [
        (1000.0, 250.0),
        (1609.344, 1609.344 / 4.0),
        (5000.0, 1250.0),
    ];
    let want = blend(
        effort_estimate(&efforts, 5000.0),
        daniels_seconds(50.4, 5000.0),
    )
    .unwrap()
    .0
    .round();
    assert_eq!(pred(&j, 5000.0)["seconds"].as_f64(), Some(want));
}

#[tokio::test]
async fn a_run_older_than_42_days_does_not_count() {
    let (e, cookie) = setup().await;
    // Thu 19 Nov is "today"; Mon 5 Oct is 45 days earlier.
    let mut old = run('a', 1, 4.0, 1250, 0.0);
    old.start = cph(2026, 10, 5, 8, 0);
    insert(&e, &old).await;
    let j = e.get(&cookie, "/api/race-predictions").await;
    assert_eq!(j["basis"], Value::Null);
    assert_eq!(j["predictions"], json!([]));
    // The run counted in the weeks it was recent: Sunday 11 Oct to Sunday 15 Nov (41 days later).
    let h = j["history"].as_array().unwrap();
    assert_eq!(h.len(), 6);
    assert_eq!(h[0]["date"], "2026-10-11");
    assert_eq!(h[5]["date"], "2026-11-15");
}

#[tokio::test]
async fn history_is_weekly_on_sundays_and_looks_back_26_weeks() {
    let (e, cookie) = setup().await;
    insert(&e, &run('a', 16, 4.0, 1250, 0.0)).await;
    e.exec(&format!(
        "INSERT INTO vo2max (ts, maximal_met, src) VALUES ({}, 14.0, 1)",
        cph(2026, 1, 3, 6, 0)
    ))
    .await;
    let j = e.get(&cookie, "/api/race-predictions").await;
    let h = j["history"].as_array().unwrap();
    assert_eq!(h.len(), 26);
    assert_eq!(h[25]["date"], "2026-11-15");
    assert_eq!(h[0]["date"], "2026-05-24");
    // The run of Mon 16 Nov is after Sunday 15 Nov: that week is the VO2 max alone, and today's
    // estimate (efforts and VO2 max) differs from it.
    let sunday = h[25]["predictions"][0]["seconds"].as_f64().unwrap();
    let today = pred(&j, 5000.0)["seconds"].as_f64().unwrap();
    assert_eq!(sunday, daniels_seconds(49.0, 5000.0).unwrap().round());
    assert_ne!(sunday, today);
    assert_eq!(j["basis"], "both");
}

#[tokio::test]
async fn change_since_four_weeks_is_the_difference_of_whole_seconds() {
    let (e, cookie) = setup().await;
    // VO2 max 49.0 on 1 Oct, 52.5 on 15 Nov.
    e.exec(&format!(
        "INSERT INTO vo2max (ts, maximal_met, src) VALUES ({}, 14.0, 1), ({}, 15.0, 1)",
        cph(2026, 10, 1, 6, 0),
        cph(2026, 11, 15, 6, 0)
    ))
    .await;
    let j = e.get(&cookie, "/api/race-predictions").await;
    let now = daniels_seconds(52.5, 5000.0).unwrap().round();
    let before = daniels_seconds(49.0, 5000.0).unwrap().round();
    let p = pred(&j, 5000.0);
    assert_eq!(p["seconds"].as_f64(), Some(now));
    assert_eq!(p["change_4w_s"].as_f64(), Some(now - before));
    assert!(now < before);
}

#[tokio::test]
async fn another_users_runs_do_not_count() {
    let (e, cookie) = setup().await;
    let other = e.add_user("other", nimrag_server::users::Role::User).await;
    let mut theirs = run('f', 10, 6.0, 1800, 400.0);
    theirs.owner = Some(other);
    insert(&e, &theirs).await;
    let j = e.get(&cookie, "/api/race-predictions").await;
    assert_eq!(j["basis"], Value::Null);
    assert_eq!(j["predictions"], json!([]));
    let theirs_cookie = e.login_as("other").await;
    let j = e.get(&theirs_cookie, "/api/race-predictions").await;
    assert_eq!(j["basis"], "efforts");
}
