//! `GET /api/metrics/{metric}`: the range API behind the metric detail pages. Hand-made rows for
//! exact values, gaps (`null`, never 0), bucket rules, `end`, `has_prev` and `has_next`, and the
//! errors; plus every metric against the `dev-seed` data.

mod common;

use axum::http::{Method, StatusCode};
use chrono_tz::Europe::Copenhagen;
use common::*;
use nimrag_server::derived::{rank_key, src_id};
use nimrag_server::seed::{self, SeedAccounts, SeedOptions};
use serde_json::{json, Value};

const METRICS: [&str; 9] = [
    "heart_rate",
    "stress",
    "energy",
    "steps",
    "intensity_minutes",
    "training_load",
    "vo2_max",
    "respiration",
    "spo2",
];

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

async fn night(e: &Env, id: char, start: i64, end: i64) {
    let sha = id.to_string().repeat(64);
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
        "INSERT INTO sleep_sessions (sha256, idx, start_ts, end_ts, overall_score) VALUES ('{sha}', 0, {start}, {end}, 80)"
    ))
    .await;
}

/// The values of one series, in order (`null` stays `Value::Null`).
fn values(m: &Value, key: &str) -> Vec<Value> {
    let s = m["series"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == key)
        .unwrap_or_else(|| panic!("no series {key} in {m}"));
    s["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["value"].clone())
        .collect()
}

fn dates(m: &Value, key: &str) -> Vec<String> {
    let s = m["series"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == key)
        .unwrap();
    s["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["date"].as_str().unwrap().to_owned())
        .collect()
}

fn keys(m: &Value) -> Vec<&str> {
    m["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["key"].as_str().unwrap())
        .collect()
}

fn stat(m: &Value, key: &str) -> Value {
    m["stats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == key)
        .unwrap_or_else(|| panic!("no stat {key} in {m}"))["value"]
        .clone()
}

fn n(v: f64) -> Value {
    json!(v)
}

const NULL: Value = Value::Null;

async fn status(e: &Env, c: &str, uri: &str) -> StatusCode {
    e.call(Method::GET, uri, Some(c), false, None).await.0
}

// ---- heart rate ------------------------------------------------------------------------------

async fn heart_rows(e: &Env) {
    // Resting: Nov 17 to 19 (the 18th through `rhr`, the others through `current_day_rhr`), and
    // one in June. High: the day's largest reading, Nov 17 to 19.
    for (day, rhr, cur) in [(17, "NULL", "54"), (18, "55", "NULL"), (19, "NULL", "53")] {
        e.exec(&format!(
            "INSERT INTO resting_hr (ts, rhr, current_day_rhr, src) VALUES ({}, {rhr}, {cur}, 1)",
            at(day, 12, 0)
        ))
        .await;
    }
    e.exec(&format!(
        "INSERT INTO resting_hr (ts, rhr, current_day_rhr, src) VALUES ({}, NULL, 60, 1)",
        cph(2026, 6, 10, 12, 0)
    ))
    .await;
    for (day, low, high) in [(17, 52, 110), (18, 55, 121), (19, 50, 113)] {
        for (h, bpm) in [(8, low), (9, high), (10, 80)] {
            e.exec(&format!(
                "INSERT INTO mon_hr (ts, bpm, src) VALUES ({}, {bpm}, 1)",
                at(day, h, 0)
            ))
            .await;
        }
    }
}

#[tokio::test]
async fn heart_rate_resting_and_high_per_day() {
    let (e, c) = setup().await;
    heart_rows(&e).await;
    let m = e.get(&c, "/api/metrics/heart_rate?range=7d").await;
    assert_eq!(m["sample"], false);
    assert_eq!(m["metric"], "heart_rate");
    assert_eq!(m["range"], "7d");
    assert_eq!(
        (m["from"].as_str(), m["to"].as_str()),
        (Some("2026-11-13"), Some("2026-11-19"))
    );
    assert_eq!(m["bucket"], "day");
    assert_eq!(keys(&m), ["resting", "high"]);
    let s = &m["series"][0];
    assert_eq!(
        (s["label"].as_str(), s["unit"].as_str()),
        (Some("Resting"), Some("bpm"))
    );
    // One point per day, dated by the day; days without a reading are null, not 0.
    assert_eq!(
        dates(&m, "resting"),
        [
            "2026-11-13",
            "2026-11-14",
            "2026-11-15",
            "2026-11-16",
            "2026-11-17",
            "2026-11-18",
            "2026-11-19"
        ]
    );
    assert_eq!(
        values(&m, "resting"),
        [NULL, NULL, NULL, NULL, n(54.0), n(55.0), n(53.0)]
    );
    assert_eq!(
        values(&m, "high"),
        [NULL, NULL, NULL, NULL, n(110.0), n(121.0), n(113.0)]
    );
    assert_eq!(stat(&m, "avg_resting"), n(54.0));
    assert_eq!(stat(&m, "avg_high"), n(114.7));
    let st = &m["stats"][0];
    assert_eq!(
        (st["label"].as_str(), st["unit"].as_str()),
        (Some("Avg resting"), Some("bpm"))
    );
    assert_eq!(
        (
            m["goal"].clone(),
            m["target_low"].clone(),
            m["target_high"].clone()
        ),
        (NULL, NULL, NULL)
    );
}

#[tokio::test]
async fn heart_rate_4w_is_28_days_and_1y_is_12_monthly_averages() {
    let (e, c) = setup().await;
    heart_rows(&e).await;
    let m = e.get(&c, "/api/metrics/heart_rate?range=4w").await;
    assert_eq!(
        (m["from"].as_str(), m["to"].as_str()),
        (Some("2026-10-23"), Some("2026-11-19"))
    );
    assert_eq!(m["bucket"], "day");
    let d = dates(&m, "resting");
    assert_eq!(
        (d.len(), d[0].as_str(), d[27].as_str()),
        (28, "2026-10-23", "2026-11-19")
    );
    assert_eq!(values(&m, "resting")[25..], [n(54.0), n(55.0), n(53.0)]);

    let y = e.get(&c, "/api/metrics/heart_rate?range=1y").await;
    assert_eq!(
        (y["from"].as_str(), y["to"].as_str()),
        (Some("2025-12-01"), Some("2026-11-19"))
    );
    assert_eq!(y["bucket"], "month");
    let d = dates(&y, "resting");
    assert_eq!(d.len(), 12);
    assert_eq!(
        (d[0].as_str(), d[11].as_str()),
        ("2025-12-01", "2026-11-01")
    );
    let resting = values(&y, "resting");
    // June has one resting value and no readings (high is null); November averages its days.
    assert_eq!(resting[6], n(60.0), "{resting:?}");
    assert_eq!(values(&y, "high")[6], NULL);
    assert_eq!(resting[11], n(54.0));
    assert_eq!(values(&y, "high")[11], n(114.7));
    assert_eq!(
        resting.iter().filter(|v| !v.is_null()).count(),
        2,
        "months without data are null"
    );
    assert_eq!(stat(&y, "avg_resting"), n(55.5));
}

// ---- stress and energy -----------------------------------------------------------------------

async fn stress_rows(e: &Env) {
    // Nov 18: four samples three minutes apart, one in each level; battery 80 down to 50.
    // Nov 19: rest then low; battery 50 up to 90.
    for (ts, stress, battery) in [
        (at(18, 10, 0), 20, 80),
        (at(18, 10, 3), 40, 70),
        (at(18, 10, 6), 60, 60),
        (at(18, 10, 9), 80, 50),
        (at(19, 9, 0), 10, 50),
        (at(19, 9, 3), 30, 90),
    ] {
        e.exec(&format!(
            "INSERT INTO stress (ts, stress, stress_raw, body_battery, src) VALUES ({ts}, {stress}, {stress}, {battery}, 1)"
        ))
        .await;
    }
}

#[tokio::test]
async fn stress_average_and_seconds_per_level() {
    let (e, c) = setup().await;
    stress_rows(&e).await;
    let m = e.get(&c, "/api/metrics/stress?range=7d").await;
    assert_eq!(
        keys(&m),
        ["average", "rest_s", "low_s", "medium_s", "high_s"]
    );
    assert_eq!(
        values(&m, "average"),
        [NULL, NULL, NULL, NULL, NULL, n(50.0), n(20.0)]
    );
    assert_eq!(
        values(&m, "rest_s"),
        [NULL, NULL, NULL, NULL, NULL, n(180.0), n(180.0)]
    );
    assert_eq!(
        values(&m, "low_s"),
        [NULL, NULL, NULL, NULL, NULL, n(180.0), n(180.0)]
    );
    assert_eq!(values(&m, "medium_s")[5..], [n(180.0), n(0.0)]);
    assert_eq!(values(&m, "high_s")[5..], [n(180.0), n(0.0)]);
    assert_eq!(stat(&m, "avg_stress"), n(35.0));
    assert_eq!(m["series"][1]["unit"], "s");
    // The same numbers as the Day view.
    let d = e.get(&c, "/api/days/2026-11-18").await;
    assert_eq!(d["stress"]["average"], 50.0);
    assert_eq!(d["stress"]["rest_s"], 180.0);

    let y = e.get(&c, "/api/metrics/stress?range=1y").await;
    assert_eq!(y["bucket"], "month");
    assert_eq!(
        values(&y, "average")[11],
        n(35.0),
        "month = mean of the days' averages"
    );
    assert_eq!(values(&y, "rest_s")[11], n(180.0));
}

#[tokio::test]
async fn energy_high_low_charged_drained() {
    let (e, c) = setup().await;
    stress_rows(&e).await;
    let m = e.get(&c, "/api/metrics/energy?range=7d").await;
    assert_eq!(keys(&m), ["high", "low", "charged", "drained"]);
    assert_eq!(
        values(&m, "high"),
        [NULL, NULL, NULL, NULL, NULL, n(80.0), n(90.0)]
    );
    assert_eq!(
        values(&m, "low"),
        [NULL, NULL, NULL, NULL, NULL, n(50.0), n(50.0)]
    );
    assert_eq!(
        values(&m, "charged"),
        [NULL, NULL, NULL, NULL, NULL, n(0.0), n(40.0)]
    );
    assert_eq!(
        values(&m, "drained"),
        [NULL, NULL, NULL, NULL, NULL, n(30.0), n(0.0)]
    );
    assert_eq!(stat(&m, "avg_high"), n(85.0));
    assert_eq!(stat(&m, "avg_low"), n(50.0));
    assert_eq!(stat(&m, "avg_charged"), n(20.0));
    assert_eq!(stat(&m, "avg_drained"), n(15.0));
    let d = e.get(&c, "/api/days/2026-11-19").await;
    assert_eq!(
        (d["energy"]["high"].clone(), d["energy"]["charged"].clone()),
        (n(90.0), n(40.0))
    );
}

// ---- steps ---------------------------------------------------------------------------------------

async fn step_rows(e: &Env) {
    // The largest counter of the day per activity type: Nov 17 is 5000 (not 3000), Nov 18 9000.
    for (ts, steps, dist) in [
        (at(17, 12, 0), 3000, 2000),
        (at(17, 18, 0), 5000, 4000),
        (at(18, 18, 0), 9000, 7000),
    ] {
        e.exec(&format!(
            "INSERT INTO mon_activity (ts, activity_type, steps, distance_m, src) VALUES ({ts}, 6, {steps}, {dist}, 1)"
        ))
        .await;
    }
    // Whole-day totals imported from Garmin Connect.
    for (ts, steps) in [
        (cph(2026, 10, 25, 12, 0), 10_000),
        (cph(2026, 6, 10, 12, 0), 12_000),
    ] {
        e.exec(&format!(
            "INSERT INTO daily_totals (ts, steps, src) VALUES ({ts}, {steps}, 1)"
        ))
        .await;
    }
}

#[tokio::test]
async fn steps_distance_goal_and_buckets() {
    let (e, c) = setup().await;
    step_rows(&e).await;
    let m = e.get(&c, "/api/metrics/steps?range=7d").await;
    assert_eq!(keys(&m), ["steps", "distance_m"]);
    assert_eq!(
        values(&m, "steps"),
        [NULL, NULL, NULL, NULL, n(5000.0), n(9000.0), NULL]
    );
    assert_eq!(values(&m, "distance_m")[4..], [n(4000.0), n(7000.0), NULL]);
    assert_eq!(stat(&m, "avg_steps"), n(7000.0));
    assert_eq!(stat(&m, "total_steps"), n(14_000.0));
    assert_eq!(m["goal"], 8000.0);
    assert_eq!(
        (m["target_low"].clone(), m["target_high"].clone()),
        (NULL, NULL)
    );

    // 4w reaches back to Oct 23: the imported day joins the average.
    let w = e.get(&c, "/api/metrics/steps?range=4w").await;
    assert_eq!(values(&w, "steps")[2], n(10_000.0), "Oct 25");
    assert_eq!(stat(&w, "avg_steps"), n(8000.0));
    assert_eq!(stat(&w, "total_steps"), n(24_000.0));

    // 1y: average steps per day with a value, per month.
    let y = e.get(&c, "/api/metrics/steps?range=1y").await;
    let v = values(&y, "steps");
    assert_eq!(
        (v[6].clone(), v[10].clone(), v[11].clone()),
        (n(12_000.0), n(10_000.0), n(7000.0))
    );
    assert_eq!(v.iter().filter(|x| !x.is_null()).count(), 3);
    assert_eq!(stat(&y, "total_steps"), n(36_000.0));

    // The goal follows the setting.
    let (s, _) = e
        .send(
            &c,
            Method::PATCH,
            "/api/settings",
            json!({ "step_goal": 10_000 }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(e.get(&c, "/api/metrics/steps").await["goal"], 10_000.0);
}

// ---- intensity minutes ---------------------------------------------------------------------------

async fn intensity_rows(e: &Env) {
    // Running totals within the local ISO week (Monday start); see `daily.rs`.
    // Week of Oct 26: Wed 28 moderate 30. Week of Nov 9: Tue 10 moderate 20 vigorous 10, Thu 12
    // moderate 50 vigorous 10 (so Thu adds 30 and 0). Week of Nov 16: Mon 10/0, Tue 25/5,
    // Thu 40/5 (Wed has no reading).
    for (ts, m, v) in [
        (cph(2026, 10, 28, 18, 0), 30, 0),
        (at(10, 18, 0), 20, 10),
        (at(12, 18, 0), 50, 10),
        (at(16, 18, 0), 10, 0),
        (at(17, 18, 0), 25, 5),
        (at(19, 18, 0), 40, 5),
    ] {
        e.exec(&format!(
            "INSERT INTO mon_extra (ts, moderate_total_min, vigorous_total_min, src) VALUES ({ts}, {m}, {v}, 1)"
        ))
        .await;
    }
}

#[tokio::test]
async fn intensity_minutes_day_week_and_month_buckets() {
    let (e, c) = setup().await;
    intensity_rows(&e).await;

    let m = e.get(&c, "/api/metrics/intensity_minutes?range=7d").await;
    assert_eq!(m["bucket"], "day");
    assert_eq!(keys(&m), ["moderate", "vigorous"]);
    assert_eq!(
        values(&m, "moderate"),
        [NULL, NULL, NULL, n(10.0), n(15.0), NULL, n(15.0)]
    );
    // Vigorous is the raw minutes; the web counts it double.
    assert_eq!(
        values(&m, "vigorous"),
        [NULL, NULL, NULL, n(0.0), n(5.0), NULL, n(0.0)]
    );
    assert_eq!(stat(&m, "total"), n(50.0), "moderate + 2 x vigorous");
    assert_eq!(stat(&m, "avg"), n(16.7));
    assert_eq!(m["goal"], 150.0);

    // 4w: the 4 Monday-to-Sunday weeks ending with the week of `end`.
    let w = e.get(&c, "/api/metrics/intensity_minutes?range=4w").await;
    assert_eq!(w["bucket"], "week");
    assert_eq!(
        (w["from"].as_str(), w["to"].as_str()),
        (Some("2026-10-26"), Some("2026-11-19"))
    );
    assert_eq!(
        dates(&w, "moderate"),
        ["2026-10-26", "2026-11-02", "2026-11-09", "2026-11-16"]
    );
    assert_eq!(values(&w, "moderate"), [n(30.0), NULL, n(50.0), n(40.0)]);
    assert_eq!(values(&w, "vigorous"), [n(0.0), NULL, n(10.0), n(5.0)]);
    assert_eq!(stat(&w, "total"), n(150.0));
    assert_eq!(stat(&w, "avg"), n(50.0));
    assert_eq!(w["goal"], 150.0);
    // The week figures are the ones the Movement view gives.
    let mv = e.get(&c, "/api/movement/2026-11-19").await;
    assert_eq!(mv["intensity"]["moderate_min"], 40.0);
    assert_eq!(mv["intensity"]["total_min"], 50.0);

    // 1y: monthly totals.
    let y = e.get(&c, "/api/metrics/intensity_minutes?range=1y").await;
    assert_eq!(y["bucket"], "month");
    let v = values(&y, "moderate");
    assert_eq!(
        (v.len(), v[9].clone(), v[10].clone(), v[11].clone()),
        (12, NULL, n(30.0), n(90.0))
    );
    assert_eq!(values(&y, "vigorous")[11], n(15.0));
    assert_eq!(stat(&y, "total"), n(150.0));
}

// ---- training load -------------------------------------------------------------------------------

#[tokio::test]
async fn training_load_acute_chronic_and_target() {
    let (e, c) = setup().await;
    for (day, load, lo, hi) in [(17, 300, 240, 420), (19, 310, 250, 430)] {
        e.exec(&format!(
            "INSERT INTO training_status (ts, status, weekly_load_sum, load_min, load_max, src)
             VALUES ({}, 7, {load}, {lo}, {hi}, 1)",
            at(day, 6, 30)
        ))
        .await;
    }
    // The 17th also has a load row (the status figure wins); the 18th only this one.
    for (day, acute, chronic) in [(17, 290, 280), (18, 305, 282), (19, 310, 284)] {
        e.exec(&format!(
            "INSERT INTO training_load (ts, acute, chronic, src) VALUES ({}, {acute}, {chronic}, 1)",
            at(day, 7, 0)
        ))
        .await;
    }
    let m = e.get(&c, "/api/metrics/training_load?range=7d").await;
    assert_eq!(keys(&m), ["acute", "chronic"]);
    assert_eq!(
        values(&m, "acute"),
        [NULL, NULL, NULL, NULL, n(300.0), n(305.0), n(310.0)]
    );
    assert_eq!(
        values(&m, "chronic"),
        [NULL, NULL, NULL, NULL, n(280.0), n(282.0), n(284.0)]
    );
    assert_eq!(stat(&m, "avg_acute"), n(305.0));
    assert_eq!(stat(&m, "avg_chronic"), n(282.0));
    assert_eq!(
        (m["target_low"].clone(), m["target_high"].clone()),
        (n(250.0), n(430.0))
    );
    assert_eq!(m["goal"], NULL);
    // The Today tile shows the same load and range.
    let t = e.get(&c, "/api/today").await;
    assert_eq!(t["training_load"]["seven_day_load"], 310.0);
    assert_eq!(t["training_load"]["target_max"], 430.0);
    // Earlier periods keep the newest target the watch gave up to their end.
    let old = e
        .get(&c, "/api/metrics/training_load?range=7d&end=2026-11-17")
        .await;
    assert_eq!(
        (old["target_low"].clone(), old["target_high"].clone()),
        (n(240.0), n(420.0))
    );
    assert_eq!(values(&old, "acute")[6], n(300.0));

    let y = e.get(&c, "/api/metrics/training_load?range=1y").await;
    assert_eq!(values(&y, "acute")[11], n(305.0));
}

// ---- VO2 max -------------------------------------------------------------------------------------

#[tokio::test]
async fn vo2_max_only_where_a_reading_exists() {
    let (e, c) = setup().await;
    // maximal_met x 3.5; two readings on the 18th: the later one is the day's value.
    for (ts, met) in [
        (cph(2026, 6, 10, 9, 0), 13.0),
        (at(14, 9, 0), 14.0),
        (at(18, 9, 0), 14.2),
        (at(18, 17, 0), 14.4),
    ] {
        e.exec(&format!(
            "INSERT INTO vo2max (ts, maximal_met, src) VALUES ({ts}, {met}, 1)"
        ))
        .await;
    }
    let m = e.get(&c, "/api/metrics/vo2_max?range=7d").await;
    assert_eq!(keys(&m), ["vo2_max"]);
    assert_eq!(m["series"][0]["unit"], "ml/kg/min");
    assert_eq!(
        values(&m, "vo2_max"),
        [NULL, n(49.0), NULL, NULL, NULL, n(50.4), NULL]
    );
    assert_eq!(stat(&m, "latest"), n(50.4));
    assert_eq!(stat(&m, "avg"), n(49.7));
    // Same value as the Today tile.
    assert_eq!(e.get(&c, "/api/today").await["vo2max"]["value"], 50.4);
    let y = e.get(&c, "/api/metrics/vo2_max?range=1y").await;
    let v = values(&y, "vo2_max");
    assert_eq!((v[6].clone(), v[11].clone()), (n(45.5), n(49.7)));
}

// ---- respiration and blood oxygen -------------------------------------------------------------------

async fn night_rows(e: &Env) {
    // Nights dated by the day they end on: Nov 17 23:00 to Nov 18 06:00, Nov 18 23:30 to
    // Nov 19 06:30.
    night(e, 'a', at(17, 23, 0), at(18, 6, 0)).await;
    night(e, 'b', at(18, 23, 30), at(19, 6, 30)).await;
    for (ts, brpm) in [
        (at(18, 2, 0), 11),
        (at(18, 12, 0), 14),
        (at(18, 22, 0), 16),
        // After the second night starts: it counts for Nov 19, asleep.
        (at(18, 23, 45), 10),
        (at(19, 1, 0), 12),
        (at(19, 3, 0), 14),
        (at(19, 10, 0), 17),
        (at(19, 15, 0), 15),
    ] {
        e.exec(&format!(
            "INSERT INTO respiration (ts, brpm, src) VALUES ({ts}, {brpm}, 1)"
        ))
        .await;
    }
    for (ts, pct) in [
        (at(18, 2, 0), 96),
        (at(18, 15, 0), 98),
        (at(19, 3, 0), 95),
        (at(19, 4, 0), 93),
    ] {
        e.exec(&format!(
            "INSERT INTO spo2 (ts, percent, src) VALUES ({ts}, {pct}, 1)"
        ))
        .await;
    }
}

#[tokio::test]
async fn respiration_asleep_and_awake_follow_the_night_that_ends_on_the_day() {
    let (e, c) = setup().await;
    night_rows(&e).await;
    let m = e.get(&c, "/api/metrics/respiration?range=7d").await;
    assert_eq!(keys(&m), ["asleep", "awake"]);
    assert_eq!(m["series"][0]["unit"], "brpm");
    assert_eq!(
        values(&m, "asleep"),
        [NULL, NULL, NULL, NULL, NULL, n(11.0), n(12.0)]
    );
    // Nov 18 awake: 12:00 and 22:00 (the 23:45 reading is asleep on the 19th).
    assert_eq!(
        values(&m, "awake"),
        [NULL, NULL, NULL, NULL, NULL, n(15.0), n(16.0)]
    );
    assert_eq!(stat(&m, "avg_asleep"), n(11.5));
    assert_eq!(stat(&m, "avg_awake"), n(15.5));
    // The Day view's night figure is the same mean.
    let d = e.get(&c, "/api/days/2026-11-19").await;
    assert_eq!(d["sleep"]["date"], "2026-11-19");
    let sleep = e.get(&c, "/api/sleep/2026-11-19").await;
    assert_eq!(sleep["night"]["breathing_brpm"], 12.0, "{sleep}");

    let y = e.get(&c, "/api/metrics/respiration?range=1y").await;
    assert_eq!(y["bucket"], "month");
    assert_eq!(values(&y, "asleep")[11], n(11.5));
}

#[tokio::test]
async fn spo2_average_and_lowest_per_day() {
    let (e, c) = setup().await;
    night_rows(&e).await;
    let m = e.get(&c, "/api/metrics/spo2?range=7d").await;
    assert_eq!(keys(&m), ["average", "low"]);
    assert_eq!(m["series"][0]["unit"], "%");
    // Nov 18: the night's 96 and the afternoon's 98.
    assert_eq!(
        values(&m, "average"),
        [NULL, NULL, NULL, NULL, NULL, n(97.0), n(94.0)]
    );
    assert_eq!(
        values(&m, "low"),
        [NULL, NULL, NULL, NULL, NULL, n(96.0), n(93.0)]
    );
    assert_eq!(stat(&m, "avg"), n(95.5));
    assert_eq!(stat(&m, "lowest"), n(93.0));
}

// ---- shape, empty database, end and navigation ------------------------------------------------------

#[tokio::test]
async fn every_metric_has_its_series_and_buckets_and_empty_days_are_null() {
    let (e, c) = setup().await;
    let series: [(&str, &[&str]); 9] = [
        ("heart_rate", &["resting", "high"]),
        (
            "stress",
            &["average", "rest_s", "low_s", "medium_s", "high_s"],
        ),
        ("energy", &["high", "low", "charged", "drained"]),
        ("steps", &["steps", "distance_m"]),
        ("intensity_minutes", &["moderate", "vigorous"]),
        ("training_load", &["acute", "chronic"]),
        ("vo2_max", &["vo2_max"]),
        ("respiration", &["asleep", "awake"]),
        ("spo2", &["average", "low"]),
    ];
    for (metric, want) in series {
        for (range, points, bucket) in [("7d", 7, "day"), ("4w", 28, "day"), ("1y", 12, "month")] {
            let (points, bucket) = if metric == "intensity_minutes" && range == "4w" {
                (4, "week")
            } else {
                (points, bucket)
            };
            let m = e
                .get(&c, &format!("/api/metrics/{metric}?range={range}"))
                .await;
            assert_eq!(m["metric"], metric);
            assert_eq!(m["range"], range);
            assert_eq!(m["bucket"], bucket, "{metric} {range}");
            assert_eq!(keys(&m), want, "{metric} {range}");
            for s in m["series"].as_array().unwrap() {
                let p = s["points"].as_array().unwrap();
                assert_eq!(p.len(), points, "{metric} {range} {}", s["key"]);
                assert!(
                    p.iter().all(|x| x["value"].is_null()),
                    "no data: every value is null, never 0: {metric} {range}"
                );
            }
            assert!(
                m["stats"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|s| s["value"].is_null()),
                "{metric}"
            );
            assert_eq!(
                (m["has_prev"].clone(), m["has_next"].clone()),
                (json!(false), json!(false))
            );
        }
    }
}

#[tokio::test]
async fn end_has_prev_and_has_next() {
    let (e, c) = setup().await;
    step_rows(&e).await; // earliest steps: June 10
                         // Defaults: range 7d, end today (in the account's zone).
    let m = e.get(&c, "/api/metrics/steps").await;
    assert_eq!(
        (m["range"].as_str(), m["to"].as_str()),
        (Some("7d"), Some("2026-11-19"))
    );
    assert_eq!(
        (m["has_prev"].clone(), m["has_next"].clone()),
        (json!(true), json!(false))
    );

    // An earlier end: the 7 days ending on it; there are later periods.
    let m = e
        .get(&c, "/api/metrics/steps?range=7d&end=2026-11-18")
        .await;
    assert_eq!(
        (m["from"].as_str(), m["to"].as_str()),
        (Some("2026-11-12"), Some("2026-11-18"))
    );
    assert_eq!(
        (m["has_prev"].clone(), m["has_next"].clone()),
        (json!(true), json!(true))
    );
    assert_eq!(values(&m, "steps")[5..], [n(5000.0), n(9000.0)]);

    // Nothing before the first reading: June 4 to 10 is the oldest period with data.
    let m = e
        .get(&c, "/api/metrics/steps?range=7d&end=2026-06-10")
        .await;
    assert_eq!(
        (m["from"].as_str(), m["to"].as_str()),
        (Some("2026-06-04"), Some("2026-06-10"))
    );
    assert_eq!(
        (m["has_prev"].clone(), m["has_next"].clone()),
        (json!(false), json!(true))
    );
    assert_eq!(values(&m, "steps")[6], n(12_000.0));
    let m = e
        .get(&c, "/api/metrics/steps?range=7d&end=2026-06-09")
        .await;
    assert_eq!(m["has_prev"], false);
    assert!(values(&m, "steps").iter().all(Value::is_null));

    // A period before today's ends on its own last day; 4w and 1y go back by their own length.
    let m = e
        .get(&c, "/api/metrics/steps?range=4w&end=2026-06-10")
        .await;
    assert_eq!(
        (m["from"].as_str(), m["to"].as_str()),
        (Some("2026-05-14"), Some("2026-06-10"))
    );
    let m = e
        .get(&c, "/api/metrics/steps?range=1y&end=2026-06-10")
        .await;
    assert_eq!(
        (m["from"].as_str(), m["to"].as_str()),
        (Some("2025-07-01"), Some("2026-06-10"))
    );
    assert_eq!(m["bucket"], "month");
    assert_eq!(values(&m, "steps")[11], n(12_000.0));
    assert_eq!(m["has_prev"], false);

    // An end after today is today: there is no later period.
    let m = e
        .get(&c, "/api/metrics/steps?range=7d&end=2026-12-31")
        .await;
    assert_eq!(
        (m["to"].as_str(), m["has_next"].clone()),
        (Some("2026-11-19"), json!(false))
    );
    // The 4w period of intensity minutes ending on a Sunday is 4 whole weeks.
    let m = e
        .get(&c, "/api/metrics/intensity_minutes?range=4w&end=2026-11-15")
        .await;
    assert_eq!(
        (m["from"].as_str(), m["to"].as_str()),
        (Some("2026-10-19"), Some("2026-11-15"))
    );
    assert_eq!(dates(&m, "moderate").len(), 4);
}

#[tokio::test]
async fn unknown_metric_is_404_and_bad_range_or_end_is_400() {
    let (e, c) = setup().await;
    for uri in [
        "/api/metrics/blood_pressure",
        "/api/metrics/",
        "/api/metrics/Heart_Rate?range=7d",
        "/api/metrics/hrv?range=bogus",
    ] {
        assert_eq!(status(&e, &c, uri).await, StatusCode::NOT_FOUND, "{uri}");
    }
    for uri in [
        "/api/metrics/steps?range=1d",
        "/api/metrics/steps?range=3m",
        "/api/metrics/steps?range=",
        "/api/metrics/steps?range=7D",
        "/api/metrics/steps?end=2026-13-01",
        "/api/metrics/steps?end=yesterday",
        "/api/metrics/steps?end=2026-11-1",
        "/api/metrics/steps?range=7d&end=",
    ] {
        assert_eq!(status(&e, &c, uri).await, StatusCode::BAD_REQUEST, "{uri}");
    }
    let (s, _, _) = e
        .call(Method::GET, "/api/metrics/steps", None, false, None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_days_follow_the_account_zone() {
    // 23:30 UTC on the 19th is already the 20th in Copenhagen (UTC+1): today is the 20th.
    let e = env_at(
        chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 11, 19, 23, 30, 0)
            .unwrap()
            .timestamp(),
    )
    .await;
    let c = e.login().await;
    e.send(
        &c,
        Method::PATCH,
        "/api/settings",
        json!({ "timezone": "Europe/Copenhagen" }),
    )
    .await;
    // 00:30 local on the 20th belongs to the 20th.
    e.exec(&format!(
        "INSERT INTO mon_activity (ts, activity_type, steps, src) VALUES ({}, 6, 400, 1)",
        cph(2026, 11, 20, 0, 30)
    ))
    .await;
    let m = e.get(&c, "/api/metrics/steps?range=7d").await;
    assert_eq!(m["to"], "2026-11-20");
    assert_eq!(values(&m, "steps")[6], n(400.0));
    assert_eq!(values(&m, "steps")[5], NULL);
}

// ---- dev-seed data ---------------------------------------------------------------------------------

#[tokio::test]
async fn seeded_data_fills_every_metric_and_is_marked_as_sample() {
    let now = cph(2026, 11, 19, 20, 0);
    let mut e = bare_env_at(now).await;
    let report = seed::run(
        &e.pool,
        e.dir.path(),
        SeedOptions {
            tz: Copenhagen,
            now,
            days: 120,
            accounts: SeedAccounts {
                admin_password: Some(PASSWORD.into()),
                admin_totp_secret: Some(TOTP_SECRET.into()),
                user_password: Some(PASSWORD.into()),
                user_totp_secret: Some(TOTP_SECRET.into()),
            },
        },
    )
    .await
    .unwrap();
    e.user_id = report.user_id;
    let c = e.login_as("demo").await;
    for metric in METRICS {
        for range in ["7d", "4w", "1y"] {
            let m = e
                .get(&c, &format!("/api/metrics/{metric}?range={range}"))
                .await;
            assert_eq!(m["sample"], true, "{metric} {range}");
            let any = m["series"].as_array().unwrap().iter().any(|s| {
                s["points"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["value"].is_number())
            });
            // The seed has no respiration or blood oxygen on the watch it imitates.
            if !matches!(metric, "respiration" | "spo2") {
                assert!(any, "{metric} {range} has no value: {m}");
            }
        }
    }
    // Seeded history is 120 days long: the previous 4w period has data, a year back has not.
    let m = e.get(&c, "/api/metrics/steps?range=4w").await;
    assert_eq!(m["has_prev"], true);
    let m = e
        .get(&c, "/api/metrics/steps?range=4w&end=2026-08-10")
        .await;
    assert_eq!(m["has_prev"], false);
    let t = e.get(&c, "/api/metrics/training_load?range=4w").await;
    assert!(
        t["target_low"].is_number() && t["target_high"].is_number(),
        "{t}"
    );
}
