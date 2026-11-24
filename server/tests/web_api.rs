//! The owner's JSON API against hand-made rows (exact values) and an empty
//! database (everything null). The seeded database is covered in `web_seed.rs`.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use nimrag_server::derived::{rank_key, src_id};
use serde_json::{json, Value};

const D: (i32, u32, u32) = (2026, 11, 19);

fn at(day: u32, h: u32, m: u32) -> i64 {
    cph(2026, 11, day, h, m)
}

/// Pinned to Thursday 2026-11-19 20:00 in Copenhagen (UTC+1), owner signed in, zone set.
async fn setup() -> (Env, String) {
    let e = env_at(cph(D.0, D.1, D.2, 20, 0)).await;
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

async fn file(e: &Env, sha: &str, last_ts: i64) {
    e.exec(&format!(
        "INSERT INTO files (sha256, size, received_at, source) VALUES ('{sha}', 1, '2026-11-19T00:00:00.000Z', 'watch')"
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO sources (sha256, src_id, ok, last_ts, msg_count, rank_key) VALUES ('{sha}', {}, 1, {last_ts}, 1, '{}')",
        src_id(sha),
        rank_key(true, Some(last_ts), 1)
    ))
    .await;
}

async fn values(e: &Env, table: &str, cols: &str, rows: &[String]) {
    for r in rows {
        e.exec(&format!("INSERT INTO {table} ({cols}) VALUES ({r})"))
            .await;
    }
}

/// A run on Wed 18 Nov at 17:30 with a short GPS track, 2 laps and zone times.
async fn insert_run(e: &Env) -> String {
    let (sha, start) = (sha('b'), at(18, 17, 30));
    file(e, &sha, start + 1800).await;
    e.exec(&format!(
        "INSERT INTO activities (sha256, activity_key, ts, start_ts, local_offset_s, total_timer_s, num_sessions, sport, sport_name, sub_sport, sub_sport_name)
         VALUES ('{sha}', 'k:run', {}, {start}, 3600, 1800, 1, 1, 'running', 0, 'generic')",
        start + 1800
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO activity_sessions (sha256, idx, start_ts, sport, sub_sport, elapsed_s, timer_s, distance_m, calories, avg_hr, max_hr, ascent_m, descent_m, aerobic_te, training_load)
         VALUES ('{sha}', 0, {start}, 1, 0, 1900, 1800, 5000, 400, 150, 170, 30, 28, 3.2, 90)"
    ))
    .await;
    for (i, (d, t, hr)) in [(2500.0, 900.0, 148.0), (2500.0, 900.0, 152.0)]
        .iter()
        .enumerate()
    {
        e.exec(&format!(
            "INSERT INTO activity_laps (sha256, idx, start_ts, elapsed_s, timer_s, distance_m, avg_hr, max_hr)
             VALUES ('{sha}', {i}, {}, {t}, {t}, {d}, {hr}, 165)",
            start + i as i64 * 900
        ))
        .await;
    }
    for i in 0..4 {
        e.exec(&format!(
            "INSERT INTO activity_records (sha256, idx, ts, lat, lon, hr, speed, altitude_m, distance_m)
             VALUES ('{sha}', {i}, {}, {}, {}, {}, 2.8, 100, {})",
            start + i * 600,
            50.0 + i as f64 * 0.001,
            10.0 + i as f64 * 0.002,
            140 + i * 5,
            i * 1600
        ))
        .await;
    }
    e.exec(&format!(
        "INSERT INTO activity_time_in_zone (sha256, idx, ref_mesg, ref_index, hr_zone_s) VALUES ('{sha}', 0, 18, 0, '[10,60,300,900,500,30]')"
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO activity_metrics (sha256, maximal_met) VALUES ('{sha}', 13.0)"
    ))
    .await;
    sha
}

/// Everything the Today fixture needs: a night, the day's energy, stress, steps, heart rate,
/// intensity minutes, HRV, recovery, VO2 max and training status.
async fn fixture(e: &Env) {
    // Night of Wed 18 to Thu 19 Nov (Thu 19's sleep): 22:30 to 06:00, with stage rows.
    let (s, w) = (at(18, 22, 30), at(19, 6, 0));
    file(e, &sha('a'), w).await;
    e.exec(&format!(
        "INSERT INTO sleep_sessions (sha256, idx, start_ts, end_ts, overall_score, awakenings_count) VALUES ('{}', 0, {s}, {w}, 84, 1)",
        sha('a')
    ))
    .await;
    // Each row ends its period: light to 23:00, deep to 00:00, light to 02:00, REM to 03:00,
    // awake to 03:10, light to 06:00.
    let lv = [
        (at(18, 23, 0), 2),
        (at(19, 0, 0), 3),
        (at(19, 2, 0), 2),
        (at(19, 3, 0), 4),
        (at(19, 3, 10), 1),
        (w, 2),
    ];
    values(
        e,
        "sleep_levels",
        "ts, level, src",
        &lv.iter()
            .map(|(t, l)| format!("{t}, {l}, 1"))
            .collect::<Vec<_>>(),
    )
    .await;
    // Energy (body battery) 06:00..10:00: 40, 60, 55, 50, 70.
    let bb = [40, 60, 55, 50, 70];
    values(
        e,
        "stress",
        "ts, body_battery, src",
        &bb.iter()
            .enumerate()
            .map(|(i, v)| format!("{}, {v}, 1", at(19, 6 + i as u32, 0)))
            .collect::<Vec<_>>(),
    )
    .await;
    // Stress samples: 10, 30, movement status, 60.
    values(
        e,
        "stress",
        "ts, stress, stress_raw, src",
        &[
            format!("{}, 10, 10, 1", at(19, 12, 0)),
            format!("{}, 30, 30, 1", at(19, 12, 10)),
            format!("{}, NULL, -2, 1", at(19, 12, 15)),
            format!("{}, 60, 60, 1", at(19, 12, 20)),
        ],
    )
    .await;
    // Cumulative steps: walking 100 then 400, running 50 then 200.
    let cols = "ts, activity_type, steps, distance_m, active_calories, src";
    values(
        e,
        "mon_activity",
        cols,
        &[
            format!("{}, 6, 100, 80, 10, 1", at(19, 10, 0)),
            format!("{}, 6, 400, 280, 40, 1", at(19, 15, 0)),
            format!("{}, 1, 50, 40, 5, 1", at(19, 14, 0)),
            format!("{}, 1, 200, 150, 20, 1", at(19, 16, 0)),
        ],
    )
    .await;
    values(
        e,
        "mon_hr",
        "ts, bpm, src",
        &[
            format!("{}, 60, 1", at(19, 8, 0)),
            format!("{}, 140, 1", at(19, 9, 0)),
            format!("{}, 50, 1", at(19, 10, 0)),
        ],
    )
    .await;
    // Resting HR: 51 on the six days before, 50 today (current_day wins over rhr).
    for d in 13..=18 {
        values(
            e,
            "resting_hr",
            "ts, rhr, current_day_rhr, src",
            &[format!("{}, 52, 51, 1", at(d, 6, 10))],
        )
        .await;
    }
    values(
        e,
        "resting_hr",
        "ts, rhr, current_day_rhr, src",
        &[format!("{}, 52, 50, 1", at(19, 6, 10))],
    )
    .await;
    // Weekly running totals of intensity minutes (Mon 16 Nov starts the week).
    let ex = "ts, moderate_total_min, vigorous_total_min, src";
    values(
        e,
        "mon_extra",
        ex,
        &[
            format!("{}, 10, 5, 1", at(16, 12, 0)),
            format!("{}, 12, 5, 1", at(16, 20, 0)),
            format!("{}, 25, 6, 1", at(17, 12, 0)),
            format!("{}, 3, 1, 1", at(18, 12, 0)), // a drop: counted as a reset
            format!("{}, 10, 2, 1", at(19, 12, 0)),
        ],
    )
    .await;
    values(e, "hrv_summary", "ts, weekly_avg, last_night_avg, baseline_balanced_lower, baseline_balanced_upper, status, src",
        &[format!("{}, 66, 48, 55, 70, 3, 1", at(19, 6, 5))]).await;
    let now = cph(D.0, D.1, D.2, 20, 0);
    values(
        e,
        "recovery_time",
        "start_ts, minutes, src",
        &[format!("{}, 120, 1", now - 3600)],
    )
    .await;
    values(
        e,
        "vo2max",
        "ts, maximal_met, src",
        &[
            format!("{}, 12.8, 1", now - 60 * 86400),
            format!("{}, 13.0, 1", now - 86400),
        ],
    )
    .await;
    let ts_cols = "ts, status, weekly_load_sum, load_min, load_max, fitness_trend, load_trend, src";
    values(
        e,
        "training_status",
        ts_cols,
        &[
            format!("{}, 3, 280, 240, 420, 1, 1, 1", at(17, 6, 30)),
            format!("{}, 4, 290, 240, 420, 1, 1, 1", at(18, 6, 30)),
            format!("{}, 4, 300, 240, 420, 1, 1, 1", at(19, 6, 30)),
        ],
    )
    .await;
    values(
        e,
        "battery",
        "ts, percent, src",
        &[format!("{}, 44, 1", now - 3600)],
    )
    .await;
    insert_run(e).await;
}

#[tokio::test]
async fn today_exact_values() {
    let (e, c) = setup().await;
    fixture(&e).await;
    let t = e.get(&c, "/api/today").await;

    assert_eq!(t["sample"], false);
    assert_eq!(t["date"], "2026-11-19");
    assert_eq!(t["generated_at"], "2026-11-19T20:00:00+01:00");
    assert_eq!(t["timezone"], "Europe/Copenhagen");

    // In focus: energy.
    let en = &t["energy"];
    assert_eq!(
        (
            en["now"].as_f64(),
            en["charged"].as_f64(),
            en["drained"].as_f64()
        ),
        (Some(70.0), Some(40.0), Some(10.0))
    );
    assert_eq!(
        (en["high"].as_f64(), en["low"].as_f64()),
        (Some(70.0), Some(40.0))
    );
    assert_eq!(en["wake_time"], "2026-11-19T06:00:00+01:00");
    assert_eq!(en["series"].as_array().unwrap().len(), 5);
    assert_eq!(en["series"][0]["time"], "2026-11-19T06:00:00+01:00");

    // In focus: last night.
    let sl = &t["sleep"];
    assert_eq!(sl["date"], "2026-11-19");
    assert_eq!(sl["start"], "2026-11-18T22:30:00+01:00");
    assert_eq!(sl["duration_s"], 26400.0); // 5h20 light + 1h deep + 1h REM
    assert_eq!(sl["time_in_bed_s"], 27000.0);
    assert_eq!(
        (
            sl["deep_s"].as_f64(),
            sl["light_s"].as_f64(),
            sl["rem_s"].as_f64(),
            sl["awake_s"].as_f64()
        ),
        (Some(3600.0), Some(19200.0), Some(3600.0), Some(600.0))
    );
    assert_eq!(sl["score"], 84.0);
    assert_eq!(sl["score_label"], "good");
    assert_eq!(sl["stages"].as_array().unwrap().len(), 6);
    assert_eq!(sl["stages"][0]["stage"], "light");
    assert_eq!(sl["stages"][5]["end"], "2026-11-19T06:00:00+01:00");
    assert_eq!(sl["hrv_ms"], 48.0);
    assert!(
        sl["lowest_hr_bpm"].is_null()
            && sl["breathing_brpm"].is_null()
            && sl["spo2_avg_percent"].is_null()
    );

    // In focus: HRV against the personal range.
    let h = &t["hrv"];
    assert_eq!(
        (h["last_night_ms"].as_f64(), h["weekly_avg_ms"].as_f64()),
        (Some(48.0), Some(66.0))
    );
    assert_eq!(
        (
            h["baseline_low_ms"].as_f64(),
            h["baseline_high_ms"].as_f64()
        ),
        (Some(55.0), Some(70.0))
    );
    assert_eq!(
        (
            h["status_code"].as_i64(),
            h["status"].as_str(),
            h["vs_baseline"].as_str()
        ),
        (Some(3), Some("unbalanced"), Some("below"))
    );
    assert_eq!(h["values_4w"][0]["date"], "2026-11-19");

    // At a glance.
    let hr = &t["heart_rate"];
    assert_eq!(hr["resting_hr_bpm"], 50.0);
    assert_eq!(hr["avg_7d_resting_hr_bpm"], 50.9);
    assert_eq!(
        (
            hr["day"]["low_bpm"].as_f64(),
            hr["day"]["high_bpm"].as_f64()
        ),
        (Some(50.0), Some(140.0))
    );
    assert_eq!(hr["day"]["series"].as_array().unwrap().len(), 3);
    assert_eq!(t["heart_rate_24h"]["high_bpm"], 140.0);
    let st = &t["stress"];
    assert_eq!(st["average"], 33.3);
    assert_eq!(
        (
            st["rest_s"].as_f64(),
            st["low_s"].as_f64(),
            st["medium_s"].as_f64(),
            st["activity_s"].as_f64()
        ),
        (Some(600.0), Some(300.0), Some(180.0), Some(300.0))
    );
    assert!(st["high_s"].as_f64() == Some(0.0));
    assert_eq!(st["main_level"], "rest");
    assert_eq!(st["series"][2]["level"], "activity");
    assert!(st["series"][2]["stress"].is_null());
    assert_eq!(t["steps"]["steps"], 600.0); // max per type, summed: 400 walking + 200 running
    assert_eq!(t["steps"]["goal"], 8000);
    assert_eq!(t["steps"]["percent_of_goal"], 7.5);
    assert_eq!(t["steps"]["distance_m"], 430.0);
    let im = &t["intensity_minutes"];
    assert_eq!(im["week_start"], "2026-11-16");
    assert_eq!(
        (
            im["moderate_min"].as_f64(),
            im["vigorous_min"].as_f64(),
            im["total_min"].as_f64()
        ),
        (Some(35.0), Some(8.0), Some(51.0))
    );
    assert_eq!(im["goal_min"], 150);
    let days: Vec<Option<f64>> = im["days"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["total_min"].as_f64())
        .collect();
    assert_eq!(
        days,
        vec![
            Some(22.0),
            Some(15.0),
            Some(5.0),
            Some(9.0),
            None,
            None,
            None
        ]
    );
    assert_eq!(t["training_status"]["status_code"], 4);
    assert_eq!(t["training_status"]["since"], "2026-11-18");
    assert_eq!(t["training_status"]["since_is_lower_bound"], false);
    assert_eq!(t["training_status"]["label"], "Maintaining");
    assert_eq!(t["training_status"]["kind"], "maintaining");
    assert!(t["training_readiness"].is_null());
    let tl = &t["training_load"];
    assert_eq!(
        (
            tl["seven_day_load"].as_f64(),
            tl["target_min"].as_f64(),
            tl["target_max"].as_f64()
        ),
        (Some(300.0), Some(240.0), Some(420.0))
    );
    assert_eq!(tl["vs_target"], "within");
    assert_eq!(tl["series_4w"].as_array().unwrap().len(), 3);
    assert_eq!(t["recovery_time"]["remaining_s"], 3600.0);
    assert_eq!(t["recovery_time"]["percent_complete"], 50.0);
    assert_eq!(t["recovery_time"]["ready_at"], "2026-11-19T21:00:00+01:00");
    assert_eq!(
        (
            t["vo2max"]["value"].as_f64(),
            t["vo2max"]["change_3m"].as_f64(),
            t["vo2max"]["derived"].as_bool()
        ),
        (Some(45.5), Some(0.7), Some(true))
    );
    // No data: null, never 0.
    assert!(t["respiration"]["latest_brpm"].is_null() && t["spo2"]["latest_percent"].is_null());
    assert!(t["weight"]["weight_kg"].is_null() && t["weight"]["change_30d_kg"].is_null());

    // Lists.
    assert_eq!(t["last_activity"]["kind"], "run");
    assert_eq!(t["training_this_week"]["sessions"], 1);
    assert_eq!(t["training_this_week"]["duration_s"], 1800.0);
    let y = &t["yesterday"];
    assert_eq!(y["date"], "2026-11-18");
    // The only night ends today, so yesterday (as on its Day view) has none.
    assert_eq!(
        (
            y["sleep_score"].as_f64(),
            y["sleep_duration_s"].as_f64(),
            y["resting_hr_bpm"].as_f64()
        ),
        (None, None, Some(51.0))
    );
    assert!(y["steps"].is_null() && y["hr_high_bpm"].is_null() && y["energy_charged"].is_null());
    let l7 = &t["last_7_days"];
    assert_eq!(
        (l7["from"].as_str(), l7["to"].as_str()),
        (Some("2026-11-12"), Some("2026-11-18"))
    );
    assert_eq!(l7["activity_count"], 1);
    assert_eq!(
        l7["activities_by_kind"][0],
        json!({ "kind": "run", "count": 1 })
    );
    assert_eq!(
        (
            l7["avg_sleep_score"].as_f64(),
            l7["avg_resting_hr_bpm"].as_f64()
        ),
        (None, Some(51.0))
    );
    assert!(l7["avg_steps"].is_null() && l7["avg_stress"].is_null());
}

#[tokio::test]
async fn empty_database_is_all_null() {
    let (e, c) = setup().await;
    let t = e.get(&c, "/api/today").await;
    assert_eq!(t["sample"], false);
    for (a, b) in [
        ("energy", "now"),
        ("energy", "charged"),
        ("hrv", "last_night_ms"),
        ("hrv", "status"),
        ("heart_rate", "resting_hr_bpm"),
        ("stress", "average"),
        ("steps", "steps"),
        ("steps", "percent_of_goal"),
        ("intensity_minutes", "total_min"),
        ("training_status", "status_code"),
        ("training_load", "seven_day_load"),
        ("recovery_time", "remaining_s"),
        ("vo2max", "value"),
        ("weight", "weight_kg"),
        ("yesterday", "steps"),
        ("yesterday", "sleep_score"),
        ("last_7_days", "avg_steps"),
        ("last_7_days", "avg_sleep_duration_s"),
        ("respiration", "latest_brpm"),
        ("spo2", "latest_percent"),
    ] {
        assert!(t[a][b].is_null(), "{a}.{b} should be null, got {}", t[a][b]);
    }
    assert!(
        t["sleep"].is_null() && t["last_activity"].is_null() && t["training_readiness"].is_null()
    );
    assert_eq!(t["energy"]["series"], json!([]));
    assert_eq!(t["training_this_week"]["sessions"], 0);

    let d = e.get(&c, "/api/days/2026-11-18").await;
    assert!(
        d["steps"].is_null() && d["sleep"].is_null() && d["heart_rate"]["resting_hr_bpm"].is_null()
    );
    assert_eq!(d["activities"], json!([]));
    let n = e.get(&c, "/api/sleep/2026-11-18").await;
    assert!(n["night"].is_null() && n["average_duration_s"].is_null());
    assert_eq!(e.get(&c, "/api/sleep?nights=14").await["nights"], json!([]));
    let m = e.get(&c, "/api/movement/2026-11-19").await;
    assert!(m["steps"].is_null() && m["floors"].is_null() && m["calories"]["total_kcal"].is_null());
    assert_eq!(m["week_steps"].as_array().unwrap().len(), 7);
    let tr = e.get(&c, "/api/trends?range=1y").await;
    assert!(tr["hrv"]["series"]["average"].is_null());
    assert_eq!(tr["resting_hr"]["points"], json!([]));
    let b = e.get(&c, "/api/body").await;
    assert!(
        b["latest_weight"].is_null()
            && b["latest_body_fat"].is_null()
            && b["latest_waist"].is_null()
    );
    assert_eq!(b["entries"], json!([]));
    let tn = e.get(&c, "/api/training").await;
    assert!(
        tn["recovery"]["remaining_s"].is_null()
            && tn["balance"].is_null()
            && tn["readiness"].is_null()
            && tn["vo2max"]["value"].is_null()
    );
    assert_eq!(tn["vo2max"]["derived"], true);
    let cal = e.get(&c, "/api/calendar/2026-11").await;
    assert_eq!(cal["days"].as_array().unwrap().len(), 30);
    assert_eq!(
        cal["totals"],
        json!({ "activities": 0, "strength_sessions": 0, "cardio_sessions": 0 })
    );
    let dev = e.get(&c, "/api/devices").await;
    assert!(
        dev["watch"]["battery_percent"].is_null()
            && dev["watch"]["model"].is_null()
            && dev["last_sync_at"].is_null()
    );
    assert_eq!(e.get(&c, "/api/storage").await["files"], 0);
    assert_eq!(e.get(&c, "/api/activities").await["items"], json!([]));
}

#[tokio::test]
async fn day_movement_calendar_sleep_trends_training_exact() {
    let (e, c) = setup().await;
    fixture(&e).await;

    let d = e.get(&c, "/api/days/2026-11-19").await;
    assert_eq!(
        (d["previous_date"].as_str(), d["next_date"].is_null()),
        (Some("2026-11-18"), true)
    );
    assert_eq!(d["steps"], 600.0);
    assert_eq!(d["intensity_minutes"], 9.0);
    assert_eq!(d["energy"]["now"], 70.0);
    let y = e.get(&c, "/api/days/2026-11-18").await;
    assert_eq!(y["next_date"], "2026-11-19");
    assert_eq!(y["activities"].as_array().unwrap().len(), 1);
    assert!(y["sleep"].is_null(), "the night ends on the 19th");
    assert_eq!(d["sleep"]["duration_s"], 26400.0);
    assert_eq!(y["activities"][0]["start"], "2026-11-18T17:30:00+01:00");
    assert_eq!(
        e.call(Method::GET, "/api/days/2026-13-45", Some(&c), false, None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );

    let m = e.get(&c, "/api/movement/2026-11-19").await;
    assert_eq!(m["week_number"], 47);
    assert_eq!(
        (
            m["steps"].as_f64(),
            m["percent_of_goal"].as_f64(),
            m["distance_m"].as_f64()
        ),
        (Some(600.0), Some(7.5), Some(430.0))
    );
    assert!(m["floors"].is_null());
    let ws: Vec<Option<f64>> = m["week_steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["value"].as_f64())
        .collect();
    assert_eq!(ws, vec![None, None, None, Some(600.0), None, None, None]);
    assert_eq!(m["week_average_steps"], 600.0);
    assert_eq!(m["calories"]["active_kcal"], 60.0);
    assert!(m["calories"]["resting_kcal"].is_null());
    // The watch's resting metabolic rate fills the total.
    file(&e, &sha('d'), at(19, 6, 0)).await;
    e.exec(&format!(
        "INSERT INTO mon_info (sha256, rmr_kcal_day) VALUES ('{}', 1780)",
        sha('d')
    ))
    .await;
    let m = e.get(&c, "/api/movement/2026-11-19").await;
    assert_eq!(
        (
            m["calories"]["resting_kcal"].as_f64(),
            m["calories"]["total_kcal"].as_f64()
        ),
        (Some(1780.0), Some(1840.0))
    );

    let cal = e.get(&c, "/api/calendar/2026-11").await;
    assert_eq!(cal["month"], "2026-11");
    assert_eq!(
        cal["totals"],
        json!({ "activities": 1, "strength_sessions": 0, "cardio_sessions": 1 })
    );
    let day18 = &cal["days"][17];
    assert_eq!(day18["date"], "2026-11-18");
    assert_eq!(day18["activities"][0]["kind"], "run");
    assert_eq!(
        (
            day18["sleep_score"].as_f64(),
            day18["sleep_duration_s"].as_f64(),
            day18["resting_hr_bpm"].as_f64()
        ),
        (None, None, Some(51.0))
    );
    let day19 = &cal["days"][18];
    assert_eq!(
        (
            day19["sleep_score"].as_f64(),
            day19["sleep_duration_s"].as_f64()
        ),
        (Some(84.0), Some(26400.0))
    );
    assert_eq!(
        (
            day19["steps"].as_f64(),
            day19["energy_high"].as_f64(),
            day19["energy_low"].as_f64()
        ),
        (Some(600.0), Some(70.0), Some(40.0))
    );
    assert!(cal["days"][0]["steps"].is_null());
    assert_eq!(
        e.call(Method::GET, "/api/calendar/2026-1", Some(&c), false, None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );

    let n = e.get(&c, "/api/sleep/2026-11-19").await;
    assert_eq!(n["night"]["duration_s"], 26400.0);
    assert_eq!(n["night"]["date"], "2026-11-19");
    assert_eq!(n["last_nights"].as_array().unwrap().len(), 1);
    assert_eq!(n["average_duration_s"], 26400.0);
    assert!(e.get(&c, "/api/sleep/2026-11-18").await["night"].is_null());
    let l = e.get(&c, "/api/sleep?nights=7").await;
    assert_eq!(
        (
            l["nights"].as_array().unwrap().len(),
            l["average_score"].as_f64()
        ),
        (1, Some(84.0))
    );
    assert_eq!(
        e.call(Method::GET, "/api/sleep?nights=5", Some(&c), false, None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );

    let t = e.get(&c, "/api/trends?range=7d").await;
    assert_eq!(
        (t["from"].as_str(), t["to"].as_str()),
        (Some("2026-11-13"), Some("2026-11-19"))
    );
    assert_eq!(t["resting_hr"]["points"].as_array().unwrap().len(), 7);
    assert_eq!(
        (
            t["resting_hr"]["average"].as_f64(),
            t["resting_hr"]["min"].as_f64(),
            t["resting_hr"]["max"].as_f64()
        ),
        (Some(50.9), Some(50.0), Some(51.0))
    );
    assert_eq!(
        (
            t["hrv"]["series"]["average"].as_f64(),
            t["hrv"]["baseline_low_ms"].as_f64(),
            t["hrv"]["baseline_high_ms"].as_f64()
        ),
        (Some(48.0), Some(55.0), Some(70.0))
    );
    assert_eq!(
        (
            t["sleep_score"]["average"].as_f64(),
            t["sleep_duration_s"]["average"].as_f64()
        ),
        (Some(84.0), Some(26400.0))
    );
    assert!(t["breathing"]["asleep"]["average"].is_null() && t["spo2"]["low_percent"].is_null());
    for r in ["4w", "3m", "1y"] {
        assert_eq!(
            e.call(
                Method::GET,
                &format!("/api/trends?range={r}"),
                Some(&c),
                false,
                None
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        e.call(Method::GET, "/api/trends?range=2d", Some(&c), false, None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );

    let tn = e.get(&c, "/api/training").await;
    assert_eq!(tn["as_of"], "2026-11-19T20:00:00+01:00");
    assert_eq!(tn["recovery"]["total_s"], 7200.0);
    assert_eq!(tn["vo2max"]["maximal_met"], 13.0);
    assert_eq!(tn["vo2max"]["series_3m"].as_array().unwrap().len(), 3);
    assert!(tn["balance"].is_null() && tn["readiness"].is_null());
    e.exec(&format!(
        "INSERT INTO training_balance (ts, aerobic_low, aerobic_high, anaerobic, anaerobic_target_min, anaerobic_target_max, src) VALUES ({}, 412, 556, 128, 60, 180, 1)",
        at(19, 6, 30)
    ))
    .await;
    let tn = e.get(&c, "/api/training").await;
    assert_eq!(
        (
            tn["balance"]["aerobic_low"].as_f64(),
            tn["balance"]["anaerobic_target_max"].as_f64()
        ),
        (Some(412.0), Some(180.0))
    );

    let dv = e.get(&c, "/api/devices").await;
    assert_eq!(dv["watch"]["battery_percent"], 44.0);
    assert_eq!(dv["watch"]["battery_at"], "2026-11-19T19:00:00+01:00");
    let (s, _) = e.upload("data_type=4&source=watch", b"hello").await;
    assert_eq!(s, StatusCode::CREATED);
    let st = e.get(&c, "/api/storage").await;
    assert_eq!(
        (st["files"].as_i64(), st["bytes"].as_i64()),
        (Some(2 + 1 + 1), Some(5 + 3))
    );
    assert!(st["database_bytes"].as_u64().unwrap() > 0 && st["last_backup_at"].is_null());
}

#[tokio::test]
async fn activities_list_detail_track() {
    let (e, c) = setup().await;
    let run = insert_run(&e).await;
    let l = e.get(&c, "/api/activities").await;
    let it = &l["items"][0];
    assert_eq!(it["id"], run);
    assert_eq!(
        (
            it["kind"].as_str(),
            it["duration_s"].as_f64(),
            it["distance_m"].as_f64()
        ),
        (Some("run"), Some(1800.0), Some(5000.0))
    );
    assert_eq!(it["avg_pace_s_per_km"], 360.0);
    assert_eq!(
        (
            it["avg_hr_bpm"].as_f64(),
            it["max_hr_bpm"].as_f64(),
            it["calories_kcal"].as_f64(),
            it["ascent_m"].as_f64()
        ),
        (Some(150.0), Some(170.0), Some(400.0), Some(30.0))
    );
    assert!(it["sets"].is_null());
    assert!(l["next_before"].is_null());
    assert_eq!(
        e.get(&c, "/api/activities?type=run").await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        e.get(&c, "/api/activities?type=strength").await["items"],
        json!([])
    );
    assert_eq!(
        e.get(&c, "/api/activities?type=all&before=2026-11-18")
            .await["items"],
        json!([])
    );
    assert_eq!(
        e.get(&c, "/api/activities?before=2026-11-19").await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        e.call(
            Method::GET,
            "/api/activities?type=bogus",
            Some(&c),
            false,
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    let d = e.get(&c, &format!("/api/activities/{run}")).await;
    assert_eq!(
        (d["elapsed_s"].as_f64(), d["end"].as_str()),
        (Some(1900.0), Some("2026-11-18T18:01:40+01:00"))
    );
    assert_eq!(
        (
            d["aerobic_te"].as_f64(),
            d["training_load"].as_f64(),
            d["vo2max_derived"].as_f64()
        ),
        (Some(3.2), Some(90.0), Some(45.5))
    );
    assert_eq!(
        (d["has_track"].as_bool(), d["has_strength"].as_bool()),
        (Some(true), Some(false))
    );
    assert_eq!(d["hr_below_zone1_s"], 10.0);
    let zones: Vec<f64> = d["hr_zones"]
        .as_array()
        .unwrap()
        .iter()
        .map(|z| z["duration_s"].as_f64().unwrap())
        .collect();
    assert_eq!(zones, vec![60.0, 300.0, 900.0, 500.0, 30.0]);
    assert_eq!(d["hr_zones"][0]["zone"], 1);
    assert_eq!(d["laps"].as_array().unwrap().len(), 2);
    assert_eq!(
        (
            d["laps"][1]["index"].as_i64(),
            d["laps"][1]["avg_pace_s_per_km"].as_f64()
        ),
        (Some(2), Some(360.0))
    );
    let s = &d["series"];
    let n = s["elapsed_s"].as_array().unwrap().len();
    assert_eq!(n, 4);
    for k in [
        "heart_rate_bpm",
        "speed_mps",
        "altitude_m",
        "distance_m",
        "cadence",
    ] {
        assert_eq!(s[k].as_array().unwrap().len(), n, "{k}");
    }
    assert_eq!(s["elapsed_s"][1], 600.0);
    assert!(s["cadence"][0].is_null());

    let tr = e.get(&c, &format!("/api/activities/{run}/track")).await;
    assert_eq!(tr["available"], true);
    assert_eq!(tr["points"].as_array().unwrap().len(), 4);
    assert_eq!(tr["points"][0], json!([50.0, 10.0]));
    assert_eq!(tr["bounds"]["max_lon"], 10.006);
    assert_eq!(tr["end"], json!([50.003, 10.006]));
    // The route colours: one value per point, taken from the same records.
    assert_eq!(tr["speed_mps"], json!([2.8, 2.8, 2.8, 2.8]));
    assert_eq!(tr["heart_rate_bpm"], json!([140.0, 145.0, 150.0, 155.0]));
    assert_eq!(tr["altitude_m"], json!([100.0, 100.0, 100.0, 100.0]));
}

#[tokio::test]
async fn unknown_activity_is_404_and_bad_ids_too() {
    let (e, c) = setup().await;
    for id in [sha('9'), "short".to_owned(), "../../etc/passwd".to_owned()] {
        for suffix in ["", "/track", "/strength"] {
            let (s, _, _) = e
                .call(
                    Method::GET,
                    &format!("/api/activities/{id}{suffix}"),
                    Some(&c),
                    false,
                    None,
                )
                .await;
            assert_eq!(s, StatusCode::NOT_FOUND, "{id}{suffix}");
        }
    }
    let (s, _) = e
        .send(
            &c,
            Method::PATCH,
            &format!("/api/activities/{}/strength", sha('9')),
            json!({ "sets": [{ "idx": 0, "reps": 3 }] }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ---- strength edits ---------------------------------------------------------------------------

/// A strength session: bench x2 (idx 0, 2), squat x1 (idx 4); rest rows at 1, 3, 5.
async fn insert_strength(e: &Env, sha: &str, key: Option<&str>, start: i64) {
    file(e, sha, start + 2000).await;
    let key = key.map_or("NULL".to_owned(), |k| format!("'{k}'"));
    e.exec(&format!(
        "INSERT INTO activities (sha256, activity_key, ts, start_ts, local_offset_s, total_timer_s, num_sessions, sport, sport_name, sub_sport, sub_sport_name)
         VALUES ('{sha}', {key}, {}, {start}, 3600, 2000, 1, 10, 'training', 20, 'strength_training')",
        start + 2000
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO activity_sessions (sha256, idx, start_ts, sport, sub_sport, elapsed_s, timer_s, calories, avg_hr, max_hr)
         VALUES ('{sha}', 0, {start}, 10, 20, 2000, 2000, 250, 110, 140)"
    ))
    .await;
    let sets = [
        (0, 1, Some(10), Some(22.0), 0),
        (1, 0, None, None, 0),
        (2, 1, Some(10), Some(22.0), 0),
        (3, 0, None, None, 0),
        (4, 1, Some(12), Some(24.0), 28),
        (5, 0, None, None, 28),
    ];
    for (idx, ty, reps, w, cat) in sets {
        e.exec(&format!(
            "INSERT INTO strength_sets (sha256, idx, ts, start_ts, duration_s, set_type, reps, weight_kg, category, category_name, category_subtype)
             VALUES ('{sha}', {idx}, {start}, {start}, 40, {ty}, {}, {}, {cat}, '{}', 0)",
            reps.map_or("NULL".into(), |r: i32| r.to_string()),
            w.map_or("NULL".into(), |w: f64| w.to_string()),
            if cat == 0 { "bench_press" } else { "squat" }
        ))
        .await;
    }
    e.exec(&format!("INSERT INTO exercise_titles (sha256, idx, category, subtype, name) VALUES ('{sha}', 0, 0, 0, 'Dumbbell bench press')")).await;
    e.exec(&format!("INSERT INTO exercise_titles (sha256, idx, category, subtype, name) VALUES ('{sha}', 1, 28, 0, 'Goblet squat')")).await;
}

fn names(j: &Value) -> Vec<String> {
    j["exercises"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            format!(
                "{}x{}",
                g["name"].as_str().unwrap_or("?"),
                g["sets"].as_array().unwrap().len()
            )
        })
        .collect()
}

#[tokio::test]
async fn strength_read_and_edit() {
    let (e, c) = setup().await;
    let id = sha('c');
    insert_strength(&e, &id, Some("k:str"), at(18, 18, 0)).await;
    let url = format!("/api/activities/{id}/strength");
    let g = e.get(&c, &url).await;
    assert_eq!(g["sets"], 3); // rest rows are not sets
    assert_eq!(g["volume_kg"], 728.0); // 10*22 + 10*22 + 12*24
    assert_eq!(names(&g), vec!["Dumbbell bench pressx2", "Goblet squatx1"]);
    assert_eq!(g["exercises"][0]["sets"][1]["idx"], 2);
    assert_eq!(g["exercises"][0]["volume_kg"], 440.0);
    assert!(g["previous"].is_null());
    // The activity page's total of reps (10 + 10 + 12).
    let d = e.get(&c, &format!("/api/activities/{id}")).await;
    assert_eq!(d["reps"], 32.0);
    let list = e.get(&c, "/api/activities").await;
    assert_eq!(
        (
            list["items"][0]["sets"].as_i64(),
            list["items"][0]["volume_kg"].as_f64(),
            list["items"][0]["exercises"].as_i64()
        ),
        (Some(3), Some(728.0), Some(2))
    );

    // Edit reps and weight of set 0.
    let (s, g) = e
        .send(
            &c,
            Method::PATCH,
            &url,
            json!({ "sets": [{ "idx": 0, "reps": 12, "weight_kg": 30 }] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{g}");
    assert_eq!(g["volume_kg"], 868.0);
    let s0 = &g["exercises"][0]["sets"][0];
    assert_eq!(
        (
            s0["reps"].as_f64(),
            s0["weight_kg"].as_f64(),
            s0["edited"].as_bool()
        ),
        (Some(12.0), Some(30.0), Some(true))
    );
    assert_eq!(
        (
            s0["original_reps"].as_f64(),
            s0["original_weight_kg"].as_f64()
        ),
        (Some(10.0), Some(22.0))
    );
    assert_eq!(g["exercises"][0]["sets"][1]["edited"], false);
    // A second patch merges: only the weight is cleared (bodyweight); the reps edit stays.
    let (_, g) = e
        .send(
            &c,
            Method::PATCH,
            &url,
            json!({ "sets": [{ "idx": 0, "weight_kg": null }] }),
        )
        .await;
    let s0 = &g["exercises"][0]["sets"][0];
    assert_eq!(
        (s0["reps"].as_f64(), s0["weight_kg"].is_null()),
        (Some(12.0), true)
    );
    assert_eq!(g["volume_kg"], 728.0 - 220.0); // set 0 no longer counts
                                               // Rename the exercise of set 2: it becomes its own group.
    let (_, g) = e
        .send(
            &c,
            Method::PATCH,
            &url,
            json!({ "sets": [{ "idx": 2, "exercise": " Incline press " }] }),
        )
        .await;
    assert_eq!(
        names(&g),
        vec![
            "Dumbbell bench pressx1",
            "Incline pressx1",
            "Goblet squatx1"
        ]
    );
    assert_eq!(
        g["exercises"][1]["sets"][0]["original_exercise"],
        "Dumbbell bench press"
    );
    // Reset set 0.
    let (_, g) = e
        .send(
            &c,
            Method::PATCH,
            &url,
            json!({ "sets": [{ "idx": 0, "reset": true }] }),
        )
        .await;
    assert_eq!(g["exercises"][0]["sets"][0]["edited"], false);
    assert_eq!(g["exercises"][0]["sets"][0]["reps"], 10.0);
    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM strength_set_overrides")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(stored, 1);
    // Overrides are keyed by the activity key, not the file hash.
    let ident: String = sqlx::query_scalar("SELECT activity_ident FROM strength_set_overrides")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(ident, "k:str");
}

#[tokio::test]
async fn strength_patch_validation() {
    let (e, c) = setup().await;
    let id = sha('c');
    insert_strength(&e, &id, None, at(18, 18, 0)).await;
    let url = format!("/api/activities/{id}/strength");
    for body in [
        json!({ "sets": [] }),
        json!({ "sets": [{ "idx": 1, "reps": 3 }] }), // a rest row
        json!({ "sets": [{ "idx": 99, "reps": 3 }] }), // no such set
        json!({ "sets": [{ "idx": 0, "reps": -1 }] }),
        json!({ "sets": [{ "idx": 0, "reps": 2.5 }] }),
        json!({ "sets": [{ "idx": 0, "weight_kg": 5000 }] }),
        json!({ "sets": [{ "idx": 0, "exercise": "" }] }),
        json!({ "sets": [{ "idx": 0, "colour": "red" }] }),
        json!({ "sets": [{ "reps": 3 }] }),
        json!({}),
    ] {
        let (s, j) = e.send(&c, Method::PATCH, &url, body.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{body}: {j}");
    }
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM strength_set_overrides")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    // A bad edit in a batch applies none of it.
    let (s, _) = e
        .send(
            &c,
            Method::PATCH,
            &url,
            json!({ "sets": [{ "idx": 0, "reps": 11 }, { "idx": 99, "reps": 1 }] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM strength_set_overrides")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn strength_edits_follow_the_activity_across_files() {
    let (e, c) = setup().await;
    let (old, new) = (sha('c'), sha('e'));
    insert_strength(&e, &old, Some("k:str"), at(18, 18, 0)).await;
    let (s, _) = e
        .send(
            &c,
            Method::PATCH,
            &format!("/api/activities/{old}/strength"),
            json!({ "sets": [{ "idx": 2, "reps": 15 }] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    // A longer copy of the same activity arrives and outranks the first one.
    insert_strength(&e, &new, Some("k:str"), at(18, 18, 0)).await;
    e.exec(&format!(
        "UPDATE sources SET last_ts = last_ts + 500, rank_key = '{}' WHERE sha256 = '{new}'",
        rank_key(true, Some(at(18, 18, 0) + 2500), 1)
    ))
    .await;
    let list = e.get(&c, "/api/activities").await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["id"], new);
    let g = e.get(&c, &format!("/api/activities/{new}/strength")).await;
    assert_eq!(g["exercises"][0]["sets"][1]["reps"], 15.0);
    assert_eq!(g["exercises"][0]["sets"][1]["edited"], true);
}

#[tokio::test]
async fn strength_previous_session_for_comparison() {
    let (e, c) = setup().await;
    insert_strength(&e, &sha('c'), Some("k:1"), at(11, 18, 0)).await;
    insert_strength(&e, &sha('e'), Some("k:2"), at(18, 18, 0)).await;
    let g = e
        .get(&c, &format!("/api/activities/{}/strength", sha('e')))
        .await;
    assert_eq!(g["previous"]["id"], sha('c'));
    assert_eq!(
        (
            g["previous"]["sets"].as_i64(),
            g["previous"]["volume_kg"].as_f64()
        ),
        (Some(3), Some(728.0))
    );
    assert!(e
        .get(&c, &format!("/api/activities/{}/strength", sha('c')))
        .await["previous"]
        .is_null());
}

/// Writes the rows that deriving this file would write: one working set (10 x 22 kg).
async fn reinstate_derived(e: &Env, id: &str) {
    for t in [
        "strength_sets",
        "activities",
        "activity_sessions",
        "sources",
        "exercise_titles",
    ] {
        e.exec(&format!("DELETE FROM {t} WHERE sha256 = '{id}'"))
            .await;
    }
    let (start, last) = (at(18, 18, 0), at(18, 19, 0));
    e.exec(&format!(
        "INSERT INTO sources (sha256, src_id, ok, last_ts, msg_count, rank_key) VALUES ('{id}', {}, 1, {last}, 1, '{}')",
        src_id(id),
        rank_key(true, Some(last), 1)
    ))
    .await;
    e.exec(&format!("INSERT INTO activities (sha256, ts, start_ts, total_timer_s, num_sessions, sport, sub_sport) VALUES ('{id}', {start}, {start}, 2000, 1, 10, 20)")).await;
    e.exec(&format!("INSERT INTO activity_sessions (sha256, idx, start_ts, sport, sub_sport, timer_s) VALUES ('{id}', 0, {start}, 10, 20, 2000)")).await;
    e.exec(&format!("INSERT INTO strength_sets (sha256, idx, set_type, reps, weight_kg, category) VALUES ('{id}', 0, 1, 10, 22, 0)")).await;
}

#[tokio::test]
async fn strength_overrides_survive_reparse() {
    let (e, c) = setup().await;
    // A real file in the ledger, so that `reparse` handles its hash.
    let bytes = tiny_fit(1_000_000);
    let (s, up) = e.upload("data_type=128&source=watch", &bytes).await;
    assert_eq!(s, StatusCode::CREATED);
    let id = up["sha256"].as_str().unwrap().to_owned();
    reinstate_derived(&e, &id).await;
    let url = format!("/api/activities/{id}/strength");
    let (s, g) = e
        .send(
            &c,
            Method::PATCH,
            &url,
            json!({ "sets": [{ "idx": 0, "reps": 8, "weight_kg": 25 }] }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{g}");
    assert_eq!(g["volume_kg"], 200.0);

    // reparse rebuilds the derived tables from the files; the derived rows go away
    // (this file holds no strength sets) but the edits stay in their own table.
    nimrag_server::derive::reparse(&e.pool, e.dir.path())
        .await
        .unwrap();
    let derived: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM strength_sets WHERE sha256 = ?")
        .bind(&id)
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(derived, 0, "reparse rebuilt the derived rows");
    let kept: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM strength_set_overrides")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(kept, 1, "reparse left the edits alone");

    // The re-derivation produces the same rows again: the edit applies to them.
    reinstate_derived(&e, &id).await;
    let g = e.get(&c, &url).await;
    let s0 = &g["exercises"][0]["sets"][0];
    assert_eq!(
        (
            s0["reps"].as_f64(),
            s0["weight_kg"].as_f64(),
            s0["edited"].as_bool()
        ),
        (Some(8.0), Some(25.0), Some(true))
    );
    assert_eq!(g["volume_kg"], 200.0);
}

// ---- body measurements ---------------------------------------------------------------------------

#[tokio::test]
async fn body_measurements_crud() {
    let (e, c) = setup().await;
    let (s, w1) = e
        .send(
            &c,
            Method::POST,
            "/api/body",
            json!({ "measured_at": "2026-10-25", "weight_kg": 80.0 }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{w1}");
    assert_eq!(w1["measured_at"], "2026-10-25T12:00:00+01:00");
    assert!(w1["body_fat_pct"].is_null());
    let (_, w2) = e.send(&c, Method::POST, "/api/body", json!({ "measured_at": "2026-11-17T07:10:00+01:00", "weight_kg": 79.0, "waist_cm": 86.5 })).await;
    let (_, w3) = e
        .send(
            &c,
            Method::POST,
            "/api/body",
            json!({ "weight_kg": 78.6, "body_fat_pct": 18.2 }),
        )
        .await; // now
    assert_eq!(w3["measured_at"], "2026-11-19T20:00:00+01:00");

    let b = e.get(&c, "/api/body").await;
    assert_eq!(
        b["latest_weight"],
        json!({ "value": 78.6, "at": "2026-11-19T20:00:00+01:00" })
    );
    assert_eq!(b["latest_body_fat"]["value"], 18.2);
    assert_eq!(b["latest_waist"]["value"], 86.5);
    assert_eq!(b["latest_waist"]["at"], "2026-11-17T07:10:00+01:00");
    assert_eq!(b["weights"].as_array().unwrap().len(), 3);
    assert_eq!(b["weight_trend"].as_array().unwrap().len(), 3);
    assert_eq!(b["weight_trend"][0]["value"], 80.0);
    assert_eq!(b["weight_change_30d_kg"], -1.4); // 78.6 against the oldest entry inside 30 days (80.0 on 25 Oct)
    let ids: Vec<i64> = b["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_i64().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec![
            w3["id"].as_i64().unwrap(),
            w2["id"].as_i64().unwrap(),
            w1["id"].as_i64().unwrap()
        ]
    ); // newest first

    // Today shows the weight card.
    let t = e.get(&c, "/api/today").await;
    assert_eq!(
        (
            t["weight"]["weight_kg"].as_f64(),
            t["weight"]["change_30d_kg"].as_f64()
        ),
        (Some(78.6), Some(-1.4))
    );

    // Validation.
    for body in [
        json!({}),
        json!({ "weight_kg": 5 }),
        json!({ "weight_kg": 600 }),
        json!({ "body_fat_pct": 99 }),
        json!({ "waist_cm": 1 }),
        json!({ "weight_kg": 80, "measured_at": "yesterday" }),
        json!({ "weight_kg": 80, "measured_at": "2027-01-01" }),
        json!({ "weight_kg": "heavy" }),
        json!({ "weight_kg": 80, "extra": 1 }),
    ] {
        let (s, _) = e.send(&c, Method::POST, "/api/body", body.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{body}");
    }
    // Delete.
    let (s, _) = e
        .send(
            &c,
            Method::DELETE,
            &format!("/api/body/{}", w1["id"]),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = e
        .send(
            &c,
            Method::DELETE,
            &format!("/api/body/{}", w1["id"]),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(
        e.get(&c, "/api/body").await["entries"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // Reparse leaves owner data alone.
    nimrag_server::derive::reparse(&e.pool, e.dir.path())
        .await
        .unwrap();
    assert_eq!(
        e.get(&c, "/api/body").await["entries"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

// ---- settings and the home cards -------------------------------------------------------------------

const DEFAULT_ORDER: [&str; 15] = [
    "energy",
    "heart_rate",
    "stress",
    "sleep",
    "hrv",
    "training_status",
    "intensity_minutes",
    "training_load",
    "steps",
    "recovery_time",
    "vo2_max",
    "respiration",
    "spo2",
    "weight",
    "training_readiness",
];

#[tokio::test]
async fn sleep_night_carries_overnight_figures_and_series() {
    let (e, c) = setup().await;
    fixture(&e).await;
    // Heart rate in the night 22:30 to 06:00: 60, 50, 56 (the fixture's own readings are later).
    values(
        &e,
        "mon_hr",
        "ts, bpm, src",
        &[
            format!("{}, 60, 1", at(18, 23, 0)),
            format!("{}, 50, 1", at(19, 1, 0)),
            format!("{}, 56, 1", at(19, 2, 0)),
        ],
    )
    .await;
    // Energy 30 at 22:40, 55 at 02:00, 78 at 05:50; stress 12 and 18 (and a movement status without value).
    values(
        &e,
        "stress",
        "ts, body_battery, stress, src",
        &[
            format!("{}, 30, 12, 1", at(18, 22, 40)),
            format!("{}, 55, 18, 1", at(19, 2, 0)),
            format!("{}, 78, NULL, 1", at(19, 5, 50)),
        ],
    )
    .await;
    values(
        &e,
        "respiration",
        "ts, brpm, src",
        &[
            format!("{}, 14.04, 1", at(18, 23, 0)),
            format!("{}, 12.0, 1", at(19, 2, 0)),
        ],
    )
    .await;
    values(
        &e,
        "spo2",
        "ts, percent, src",
        &[
            format!("{}, 96, 1", at(18, 23, 0)),
            format!("{}, 92, 1", at(19, 2, 0)),
        ],
    )
    .await;
    values(
        &e,
        "hrv_values",
        "ts, rmssd_ms, src",
        &[
            format!("{}, 50, 1", at(19, 1, 0)),
            format!("{}, 60, 1", at(19, 1, 2)),
            format!("{}, 70, 1", at(19, 1, 10)),
        ],
    )
    .await;

    let n = e.get(&c, "/api/sleep/2026-11-19").await;
    let night = &n["night"];
    assert_eq!(night["sleep_need_min"], 480);
    assert_eq!(night["lowest_hr_bpm"], 50.0);
    assert_eq!(night["avg_hr_bpm"], 55.3);
    // From the first reading of the night (30) to the last, the fixture's 06:00 one (40).
    assert_eq!(night["energy_change"], 10.0);
    assert_eq!(night["breathing_low_brpm"], 12.0);
    assert_eq!(night["breathing_brpm"], 13.0);
    assert_eq!(night["spo2_low_percent"], 92.0);
    // The fixture's HRV summary (status 3) sits at 06:05, inside the window after the night.
    assert_eq!(night["hrv_status"], "unbalanced");
    assert!(night["restless_moments"].is_null());

    let keys: Vec<&str> = n["overlays"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        ["heart_rate", "energy", "stress", "breathing", "spo2", "hrv"]
    );
    let series = |k: &str| {
        n["overlays"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["key"] == k)
            .unwrap()["points"]
            .clone()
    };
    // Minutes from the start of the night (22:30): 23:00 is minute 30, 01:00 is 150, 02:00 is 210.
    assert_eq!(
        series("heart_rate"),
        json!([
            { "minute": 30.0, "value": 60.0 }, { "minute": 150.0, "value": 50.0 }, { "minute": 210.0, "value": 56.0 }
        ])
    );
    // The movement status (no stress value) is not a point of the stress line.
    assert_eq!(series("stress").as_array().unwrap().len(), 2);
    // Two HRV values within the same 5 minute bucket are averaged.
    assert_eq!(
        series("hrv"),
        json!([{ "minute": 150.0, "value": 55.0 }, { "minute": 160.0, "value": 70.0 }])
    );

    // A night without any of it has no series and null figures.
    let bare = e.get(&c, "/api/sleep/2026-11-18").await;
    assert!(bare["night"].is_null());
    assert_eq!(bare["overlays"], json!([]));
}

#[tokio::test]
async fn sleep_need_setting_is_validated_and_reaches_the_night() {
    let (e, c) = setup().await;
    fixture(&e).await;
    assert_eq!(e.get(&c, "/api/settings").await["sleep_need_min"], 480);
    let (st, s) = e
        .send(
            &c,
            Method::PATCH,
            "/api/settings",
            json!({ "sleep_need_min": 435 }),
        )
        .await;
    assert_eq!((st, &s["sleep_need_min"]), (StatusCode::OK, &json!(435)));
    assert_eq!(
        e.get(&c, "/api/sleep/2026-11-19").await["night"]["sleep_need_min"],
        435
    );
    for ok in [360, 600] {
        let (st, _) = e
            .send(
                &c,
                Method::PATCH,
                "/api/settings",
                json!({ "sleep_need_min": ok }),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "{ok}");
    }
    for bad in [
        json!(0),
        json!(345),
        json!(605),
        json!(481),
        json!(615),
        json!("8h"),
    ] {
        let (st, _) = e
            .send(
                &c,
                Method::PATCH,
                "/api/settings",
                json!({ "sleep_need_min": bad }),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad}");
    }
    assert_eq!(e.get(&c, "/api/settings").await["sleep_need_min"], 600);
}

#[tokio::test]
async fn settings_defaults_and_patch() {
    let e = env_at(cph(D.0, D.1, D.2, 20, 0)).await;
    let c = e.login().await;
    let s = e.get(&c, "/api/settings").await;
    assert_eq!(
        (s["step_goal"].as_i64(), s["intensity_goal_min"].as_i64()),
        (Some(8000), Some(150))
    );
    assert!(s["timezone"]
        .as_str()
        .unwrap()
        .parse::<chrono_tz::Tz>()
        .is_ok()); // the system zone
    assert_eq!(s["home_cards"]["order"], json!(DEFAULT_ORDER));
    assert_eq!(
        s["home_cards"]["hidden"],
        json!(["respiration", "spo2", "weight", "training_readiness"])
    );
    // The first nine are the visible, ordered ones from the brief.
    assert_eq!(
        &DEFAULT_ORDER[..9],
        [
            "energy",
            "heart_rate",
            "stress",
            "sleep",
            "hrv",
            "training_status",
            "intensity_minutes",
            "training_load",
            "steps"
        ]
    );

    let (st, s) = e.send(&c, Method::PATCH, "/api/settings", json!({ "step_goal": 10000, "intensity_goal_min": 200, "timezone": "Europe/Copenhagen" })).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        (
            s["step_goal"].as_i64(),
            s["intensity_goal_min"].as_i64(),
            s["timezone"].as_str()
        ),
        (Some(10000), Some(200), Some("Europe/Copenhagen"))
    );
    // The goals reach the pages.
    assert_eq!(e.get(&c, "/api/today").await["steps"]["goal"], 10000);
    assert_eq!(
        e.get(&c, "/api/today").await["intensity_minutes"]["goal_min"],
        200
    );
    assert_eq!(e.get(&c, "/api/settings").await["step_goal"], 10000);
    // An empty patch changes nothing.
    let (st, s2) = e.send(&c, Method::PATCH, "/api/settings", json!({})).await;
    assert_eq!((st, &s2["step_goal"]), (StatusCode::OK, &json!(10000)));

    for body in [
        json!({ "timezone": "Mars/Olympus" }),
        json!({ "timezone": "" }),
        json!({ "step_goal": 0 }),
        json!({ "step_goal": 1000000 }),
        json!({ "intensity_goal_min": 0 }),
        json!({ "step_goal": "many" }),
        json!({ "colour": "red" }),
    ] {
        let (st, _) = e
            .send(&c, Method::PATCH, "/api/settings", body.clone())
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    }
    assert_eq!(e.get(&c, "/api/settings").await["step_goal"], 10000);
}

#[tokio::test]
async fn home_cards_are_validated_and_normalised() {
    let e = env_at(cph(D.0, D.1, D.2, 20, 0)).await;
    let c = e.login().await;
    let url = "/api/settings";
    // Unknown id.
    let (s, j) = e
        .send(
            &c,
            Method::PATCH,
            url,
            json!({ "home_cards": { "order": ["energy", "sparkle"], "hidden": [] } }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{j}");
    let (s, _) = e
        .send(
            &c,
            Method::PATCH,
            url,
            json!({ "home_cards": { "order": ["energy"], "hidden": ["nope"] } }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Duplicates.
    let (s, j) = e
        .send(
            &c,
            Method::PATCH,
            url,
            json!({ "home_cards": { "order": ["energy", "steps", "energy"], "hidden": [] } }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(j["error"].as_str().unwrap().contains("twice"));
    let (s, _) = e
        .send(
            &c,
            Method::PATCH,
            url,
            json!({ "home_cards": { "order": [], "hidden": ["spo2", "spo2"] } }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Missing fields.
    let (s, _) = e
        .send(
            &c,
            Method::PATCH,
            url,
            json!({ "home_cards": { "order": ["energy"] } }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Nothing was stored by the refused requests.
    assert_eq!(
        e.get(&c, url).await["home_cards"]["order"],
        json!(DEFAULT_ORDER)
    );

    // A short list keeps its order and gets the missing cards appended in default order.
    let (s, j) = e.send(&c, Method::PATCH, url, json!({ "home_cards": { "order": ["steps", "sleep"], "hidden": ["energy", "vo2_max"] } })).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    let order = j["home_cards"]["order"].as_array().unwrap();
    assert_eq!(order.len(), 15);
    assert_eq!(
        (&order[0], &order[1], &order[2]),
        (&json!("steps"), &json!("sleep"), &json!("energy"))
    );
    assert_eq!(j["home_cards"]["hidden"], json!(["energy", "vo2_max"]));
    assert_eq!(e.get(&c, url).await["home_cards"], j["home_cards"]);
}

#[tokio::test]
async fn about_names_the_server_version_for_any_signed_in_session_only() {
    let (e, cookie) = setup().await;
    let j = e.get(&cookie, "/api/about").await;
    assert_eq!(j["version"], env!("CARGO_PKG_VERSION"));

    // An admin has no data but does have a profile page.
    e.add_user("boss", nimrag_server::users::Role::Admin).await;
    let admin_cookie = e.login_as("boss").await;
    assert_eq!(
        e.get(&admin_cookie, "/api/about").await["version"],
        j["version"]
    );

    // Signed out: not public.
    let (s, _, _) = e.call(Method::GET, "/api/about", None, false, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn training_load_tile_reads_the_newest_row_that_has_a_load() {
    let (e, c) = setup().await;
    let cols = "ts, status, weekly_load_sum, load_min, load_max, fitness_trend, load_trend, src";
    // Yesterday's Garmin Connect row has the load and its target range; today's row from the
    // watch has only a status.
    values(
        &e,
        "training_status",
        cols,
        &[
            format!("{}, 4, 2.0, 175, 328, 1, 1, 1", at(18, 0, 0)),
            format!("{}, 5, NULL, NULL, NULL, NULL, NULL, 1", at(19, 6, 30)),
        ],
    )
    .await;
    for uri in ["/api/today", "/api/training"] {
        let t = e.get(&c, uri).await;
        let (status, load) = if uri == "/api/today" {
            (&t["training_status"], &t["training_load"])
        } else {
            (&t["status"], &t["load"])
        };
        // The status is still the newest row's.
        assert_eq!(status["status_code"], 5, "{uri}");
        assert_eq!(status["since"], "2026-11-19", "{uri}");
        assert_eq!(
            (
                load["seven_day_load"].as_f64(),
                load["target_min"].as_f64(),
                load["target_max"].as_f64()
            ),
            (Some(2.0), Some(175.0), Some(328.0)),
            "{uri}"
        );
        assert_eq!(load["vs_target"], "below", "{uri}");
    }

    // A load more than a week old is no longer the 7-day load: unavailable, not stale.
    e.exec("DELETE FROM training_status").await;
    values(
        &e,
        "training_status",
        cols,
        &[
            format!("{}, 4, 250, 240, 420, 1, 1, 1", at(10, 6, 30)),
            format!("{}, 5, NULL, NULL, NULL, NULL, NULL, 1", at(19, 6, 30)),
        ],
    )
    .await;
    let t = e.get(&c, "/api/today").await;
    let load = &t["training_load"];
    assert!(load["seven_day_load"].is_null() && load["target_min"].is_null());
    assert!(load["vs_target"].is_null());
}
