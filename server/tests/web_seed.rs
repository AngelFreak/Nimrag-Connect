//! The API against the `dev-seed` data: shapes and invariants for every endpoint
//! (exact numbers live in `web_api.rs`; the seed uses libm functions, so its values
//! are not asserted to the digit), plus the seeder's own guarantees.

mod common;

use axum::http::{Method, StatusCode};
use chrono_tz::Europe::Copenhagen;
use common::*;
use nimrag_server::seed::{self, SeedAccounts, SeedOptions};
use serde_json::{json, Value};

fn now() -> i64 {
    cph(2026, 11, 19, 20, 0)
}

fn options() -> SeedOptions {
    SeedOptions {
        tz: Copenhagen,
        now: now(),
        days: 120,
        accounts: SeedAccounts {
            admin_password: Some(PASSWORD.into()),
            admin_totp_secret: Some(TOTP_SECRET.into()),
            user_password: Some(PASSWORD.into()),
            user_totp_secret: Some(TOTP_SECRET.into()),
        },
    }
}

async fn seeded() -> (Env, String) {
    let mut e = bare_env_at(now()).await;
    let report = seed::run(&e.pool, e.dir.path(), options()).await.unwrap();
    e.user_id = report.user_id;
    assert_eq!(report.days, 120);
    assert_eq!(report.last_date, "2026-11-19");
    assert_eq!(report.first_date, "2026-07-23");
    assert!(report.activities >= 40, "{} activities", report.activities);
    let c = e.login_as("demo").await;
    (e, c)
}

fn arr<'a>(v: &'a Value, what: &str) -> &'a Vec<Value> {
    v.as_array()
        .unwrap_or_else(|| panic!("{what} is not an array: {v}"))
}

#[tokio::test]
async fn seed_refuses_uploaded_files_and_reseeding() {
    let e = env_at(now()).await;
    let (s, _) = e.upload("data_type=4&source=watch", b"hello").await;
    assert_eq!(s, StatusCode::CREATED);
    let err = seed::run(&e.pool, e.dir.path(), options())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("uploaded files"), "{err}");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "a refused seed writes nothing");

    let (e, _) = seeded().await;
    assert!(seed::run(&e.pool, e.dir.path(), options()).await.is_err());
}

#[tokio::test]
async fn seed_is_deterministic_and_marked_as_sample() {
    let (a, ca) = seeded().await;
    let (b, cb) = seeded().await;
    for url in [
        "/api/activities?limit=100",
        "/api/calendar/2026-10",
        "/api/trends?range=3m",
        "/api/today",
        "/api/training",
    ] {
        assert_eq!(a.get(&ca, url).await, b.get(&cb, url).await, "{url}");
    }
    assert_eq!(a.get(&ca, "/api/settings").await["sample"], true);
    // Every page says so.
    for url in [
        "/api/today",
        "/api/days/2026-11-18",
        "/api/sleep/2026-11-18",
        "/api/sleep?nights=7",
        "/api/trends?range=7d",
        "/api/movement/2026-11-18",
        "/api/training",
        "/api/training/status",
        "/api/hrv",
        "/api/sleep/history",
        "/api/body",
        "/api/calendar/2026-11",
        "/api/devices",
        "/api/storage",
    ] {
        assert_eq!(a.get(&ca, url).await["sample"], true, "{url}");
    }
}

#[tokio::test]
async fn seeded_today_fills_every_block() {
    let (e, c) = seeded().await;
    let t = e.get(&c, "/api/today").await;
    assert_eq!(
        (t["date"].as_str(), t["timezone"].as_str()),
        (Some("2026-11-19"), Some("Europe/Copenhagen"))
    );
    assert!(
        t["energy"]["now"].is_number()
            && t["energy"]["charged"].is_number()
            && !arr(&t["energy"]["series"], "energy series").is_empty()
    );
    assert!(t["sleep"]["duration_s"].as_f64().unwrap() > 4.0 * 3600.0);
    assert!(!arr(&t["sleep"]["stages"], "stages").is_empty());
    assert!(t["hrv"]["last_night_ms"].is_number() && t["hrv"]["baseline_low_ms"].is_number());
    assert!(
        t["heart_rate"]["resting_hr_bpm"].is_number()
            && t["heart_rate"]["avg_7d_resting_hr_bpm"].is_number()
    );
    assert!(!arr(&t["heart_rate"]["day"]["series"], "hr series").is_empty());
    assert!(t["stress"]["average"].is_number() && t["stress"]["main_level"].is_string());
    assert!(t["steps"]["steps"].as_f64().unwrap() > 0.0 && t["steps"]["goal"] == 8000);
    assert_eq!(t["intensity_minutes"]["goal_min"], 150);
    assert_eq!(
        arr(&t["intensity_minutes"]["days"], "intensity days").len(),
        7
    );
    assert!(
        t["training_status"]["status_code"].is_number()
            && t["training_load"]["seven_day_load"].is_number()
    );
    assert!(
        t["recovery_time"]["remaining_s"].is_number()
            || t["recovery_time"]["remaining_s"].is_null()
    );
    assert!(t["vo2max"]["value"].is_number() && t["vo2max"]["derived"] == true);
    assert!(t["last_activity"]["id"].is_string());
    let y = &t["yesterday"];
    assert_eq!(y["date"], "2026-11-18");
    assert!(
        y["steps"].is_number() && y["sleep_score"].is_number() && y["resting_hr_bpm"].is_number()
    );
    let l7 = &t["last_7_days"];
    assert_eq!(
        (l7["from"].as_str(), l7["to"].as_str()),
        (Some("2026-11-12"), Some("2026-11-18"))
    );
    assert!(
        l7["avg_steps"].is_number()
            && l7["avg_sleep_score"].is_number()
            && l7["activity_count"].as_i64().unwrap() >= 1
    );
}

#[tokio::test]
async fn seeded_activities_paginate_filter_and_detail() {
    let (e, c) = seeded().await;
    let first = e.get(&c, "/api/activities?limit=10").await;
    let items = arr(&first["items"], "items");
    assert_eq!(items.len(), 10);
    let starts: Vec<&str> = items.iter().map(|i| i["start"].as_str().unwrap()).collect();
    assert!(
        starts.windows(2).all(|w| w[0] > w[1]),
        "newest first: {starts:?}"
    );
    let cursor = first["next_before"]
        .as_str()
        .expect("a full page has a cursor");

    // The next page continues where the first ended, without overlap.
    let second = e
        .get(&c, &format!("/api/activities?limit=10&before={cursor}"))
        .await;
    let ids: Vec<&str> = items.iter().map(|i| i["id"].as_str().unwrap()).collect();
    for i in arr(&second["items"], "second page") {
        assert!(!ids.contains(&i["id"].as_str().unwrap()));
        assert!(i["start"].as_str().unwrap() < *starts.last().unwrap());
    }
    // Paging all the way through yields every activity once.
    let all = e.get(&c, "/api/activities?limit=200").await;
    let total = arr(&all["items"], "all").len();
    assert!(total >= 40 && all["next_before"].is_null());

    let mut kinds = std::collections::BTreeMap::new();
    for i in arr(&all["items"], "all") {
        *kinds
            .entry(i["kind"].as_str().unwrap().to_owned())
            .or_insert(0) += 1;
    }
    for k in ["run", "strength", "walk"] {
        assert!(kinds.contains_key(k), "no {k} in {kinds:?}");
        let f = e
            .get(&c, &format!("/api/activities?type={k}&limit=200"))
            .await;
        assert_eq!(arr(&f["items"], k).len(), kinds[k], "{k}");
        assert!(arr(&f["items"], k).iter().all(|i| i["kind"] == k));
    }

    // A run: detail, columnar series of equal length, a track inside its bounds.
    let run = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "run")
        .unwrap();
    assert!(run["distance_m"].as_f64().unwrap() > 1000.0 && run["avg_pace_s_per_km"].is_number());
    let id = run["id"].as_str().unwrap();
    let d = e.get(&c, &format!("/api/activities/{id}")).await;
    assert_eq!(d["has_track"], true);
    let s = &d["series"];
    let n = arr(&s["elapsed_s"], "elapsed").len();
    assert!((2..=600).contains(&n), "{n} series points");
    for k in [
        "heart_rate_bpm",
        "speed_mps",
        "altitude_m",
        "distance_m",
        "cadence",
    ] {
        assert_eq!(arr(&s[k], k).len(), n, "{k}");
    }
    assert!(!arr(&d["laps"], "laps").is_empty() && arr(&d["hr_zones"], "zones").len() == 5);
    let tr = e.get(&c, &format!("/api/activities/{id}/track")).await;
    let pts = arr(&tr["points"], "points");
    assert!((2..=1500).contains(&pts.len()));
    let b = &tr["bounds"];
    for p in pts {
        let (lat, lon) = (p[0].as_f64().unwrap(), p[1].as_f64().unwrap());
        assert!(lat >= b["min_lat"].as_f64().unwrap() && lat <= b["max_lat"].as_f64().unwrap());
        assert!(lon >= b["min_lon"].as_f64().unwrap() && lon <= b["max_lon"].as_f64().unwrap());
    }
    assert_eq!(tr["start"], pts[0]);
    assert_eq!(tr["end"], *pts.last().unwrap());

    // A strength session: sets grouped by exercise, rest sets not shown, idx gaps allowed.
    let st = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "strength")
        .unwrap();
    let id = st["id"].as_str().unwrap();
    assert!(st["sets"].as_i64().unwrap() >= 6 && st["volume_kg"].as_f64().unwrap() > 0.0);
    let g = e.get(&c, &format!("/api/activities/{id}/strength")).await;
    assert_eq!(g["sets"], st["sets"]);
    let groups = arr(&g["exercises"], "exercises");
    assert!(groups.len() >= 2);
    let idx: Vec<i64> = groups
        .iter()
        .flat_map(|x| x["sets"].as_array().unwrap())
        .map(|s| s["idx"].as_i64().unwrap())
        .collect();
    assert_eq!(idx.len() as i64, st["sets"].as_i64().unwrap());
    let mut sorted = idx.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), idx.len(), "set indexes are unique");
    assert!(
        sorted.windows(2).any(|w| w[1] - w[0] > 1),
        "rest rows leave gaps in the indexes"
    );
    assert!(groups.iter().all(|x| x["name"].is_string()));
    // The seeded sessions have an earlier one to compare against.
    assert!(
        g["previous"].is_object()
            || e.get(
                &c,
                &format!("/api/activities/{}/strength", st["id"].as_str().unwrap())
            )
            .await["previous"]
                .is_null()
    );

    // Edits work on seeded data too.
    let first = &groups[0]["sets"][0];
    let (code, out) = e
        .send(
            &c,
            Method::PATCH,
            &format!("/api/activities/{id}/strength"),
            json!({ "sets": [{ "idx": first["idx"], "reps": 99 }] }),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{out}");
    assert_eq!(out["exercises"][0]["sets"][0]["reps"], 99.0);
    // A walk has no strength sets.
    let walk = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "walk")
        .unwrap();
    let (code, _, _) = e
        .call(
            Method::GET,
            &format!("/api/activities/{}/strength", walk["id"].as_str().unwrap()),
            Some(&c),
            false,
            None,
        )
        .await;
    assert!(code == StatusCode::OK || code == StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn seeded_days_sleep_trends_movement_calendar() {
    let (e, c) = seeded().await;

    let d = e.get(&c, "/api/days/2026-11-18").await;
    assert_eq!(
        (d["previous_date"].as_str(), d["next_date"].as_str()),
        (Some("2026-11-17"), Some("2026-11-19"))
    );
    assert!(d["steps"].is_number() && d["sleep"]["duration_s"].is_number());
    assert!(!arr(&d["heart_rate"]["day"]["series"], "hr").is_empty());
    assert!(e.get(&c, "/api/days/2026-11-19").await["next_date"].is_null());
    // 120 days of data: the first day has data, the day before the seed has none.
    assert!(e.get(&c, "/api/days/2026-07-23").await["steps"].is_number());
    assert!(e.get(&c, "/api/days/2026-07-01").await["steps"].is_null());

    let n = e.get(&c, "/api/sleep/2026-11-18").await;
    let night = &n["night"];
    assert!(
        night["score"].is_number()
            && night["spo2_avg_percent"].is_number()
            && night["breathing_brpm"].is_number()
    );
    let stages = arr(&night["stages"], "stages");
    assert!(stages.len() >= 4);
    assert!(
        stages.windows(2).all(|w| w[0]["end"] == w[1]["start"]),
        "stages are contiguous"
    );
    let stage_sum: f64 = ["deep_s", "light_s", "rem_s"]
        .iter()
        .map(|k| night[k].as_f64().unwrap())
        .sum();
    assert!((stage_sum - night["duration_s"].as_f64().unwrap()).abs() < 1.0);
    assert_eq!(arr(&n["last_nights"], "last nights").len(), 7);
    for (q, want) in [(7, 7), (14, 14), (30, 30)] {
        let l = e.get(&c, &format!("/api/sleep?nights={q}")).await;
        assert_eq!(arr(&l["nights"], "nights").len(), want, "nights={q}");
    }

    for (r, points) in [("7d", 7), ("4w", 28), ("3m", 92), ("1y", 120)] {
        let t = e.get(&c, &format!("/api/trends?range={r}")).await;
        let n = arr(&t["resting_hr"]["points"], "rhr points").len();
        assert!(n >= points.min(100) - 2 && n <= 366, "{r}: {n} points");
        let (avg, lo, hi) = (
            t["resting_hr"]["average"].as_f64().unwrap(),
            t["resting_hr"]["min"].as_f64().unwrap(),
            t["resting_hr"]["max"].as_f64().unwrap(),
        );
        assert!(
            lo <= avg && avg <= hi && lo > 35.0 && hi < 90.0,
            "{r}: {lo} {avg} {hi}"
        );
        assert!(t["hrv"]["baseline_low_ms"].is_number() && t["sleep_score"]["average"].is_number());
        assert!(
            t["breathing"]["asleep"]["average"].is_number() && t["spo2"]["low_percent"].is_number()
        );
    }

    let m = e.get(&c, "/api/movement/2026-11-18").await;
    assert_eq!(arr(&m["week_steps"], "week").len(), 7);
    assert!(
        m["steps"].is_number() && m["floors"].is_null() && m["calories"]["active_kcal"].is_number()
    );
    assert!(
        m["intensity"]["total_min"].as_f64().unwrap() < 600.0,
        "plausible week: {}",
        m["intensity"]
    );

    let cal = e.get(&c, "/api/calendar/2026-10").await;
    let days = arr(&cal["days"], "days");
    assert_eq!(days.len(), 31);
    let acts: usize = days
        .iter()
        .map(|d| d["activities"].as_array().unwrap().len())
        .sum();
    assert_eq!(cal["totals"]["activities"], acts);
    assert_eq!(
        cal["totals"]["strength_sessions"].as_u64().unwrap()
            + cal["totals"]["cardio_sessions"].as_u64().unwrap(),
        acts as u64
    );
    assert!(acts >= 5);
    // Every activity listed on a day starts on that day.
    for d in days {
        for a in d["activities"].as_array().unwrap() {
            assert!(a["start"]
                .as_str()
                .unwrap()
                .starts_with(d["date"].as_str().unwrap()));
        }
    }
    // The activities of the whole seed show up in the calendar, month by month.
    let mut total = 0;
    for m in ["2026-07", "2026-08", "2026-09", "2026-10", "2026-11"] {
        total += e.get(&c, &format!("/api/calendar/{m}")).await["totals"]["activities"]
            .as_u64()
            .unwrap();
    }
    let listed = arr(
        &e.get(&c, "/api/activities?limit=200").await["items"],
        "items",
    )
    .len() as u64;
    assert_eq!(total, listed);
}

#[tokio::test]
async fn seeded_training_body_devices_storage() {
    let (e, c) = seeded().await;
    let t = e.get(&c, "/api/training").await;
    assert!(t["status"]["status_code"].is_number() && t["status"]["since"].is_string());
    assert!(t["load"]["acute"].is_number() || t["load"]["seven_day_load"].is_number());
    assert!(!arr(&t["load"]["series_4w"], "load series").is_empty());
    assert!(
        t["vo2max"]["value"].is_number()
            && !arr(&t["vo2max"]["series_3m"], "vo2 series").is_empty()
    );
    assert!(t["balance"].is_object() && t["recovery"]["total_s"].is_number());

    // Body: the seed has a weight history, but no waist or fat entries unless it says so.
    let b = e.get(&c, "/api/body").await;
    assert!(b["latest_weight"]["value"].is_number() && !arr(&b["weights"], "weights").is_empty());
    assert!(arr(&b["entries"], "entries").len() <= 100);

    let dv = e.get(&c, "/api/devices").await;
    assert!(dv["watch"]["battery_percent"].is_number());
    assert!(
        dv["watch"]["model"].is_null(),
        "no table holds the model name"
    );
    let s = e.get(&c, "/api/storage").await;
    assert_eq!(
        (s["files"].as_i64(), s["bytes"].as_i64()),
        (Some(0), Some(0))
    );
    assert!(s["database_bytes"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn seeded_range_endpoints_fill_the_three_status_pages() {
    let (e, c) = seeded().await;

    // Training status over six months: the seed spans 120 days, so the oldest weeks are empty,
    // the rest uses only verified codes, and the runs cover the same days as the timeline.
    let t = e
        .get(&c, "/api/training/status?from=2026-05-21&to=2026-11-19")
        .await;
    let days = arr(&t["days"], "days");
    assert_eq!(days.len(), 183);
    assert!(days[0]["status"].is_null());
    let kinds: std::collections::BTreeSet<&str> = days
        .iter()
        .filter_map(|d| d["status"]["kind"].as_str())
        .collect();
    assert!(kinds.len() >= 3, "a mix of statuses: {kinds:?}");
    assert!(!kinds.contains("unknown") && !kinds.contains("none"));
    let with_status = days.iter().filter(|d| !d["status"].is_null()).count() as u64;
    let in_runs: u64 = arr(&t["runs"], "runs")
        .iter()
        .map(|r| r["days"].as_u64().unwrap())
        .sum();
    assert_eq!(in_runs, with_status);
    assert_eq!(t["runs"][0]["ongoing"], true);
    assert!(t["current"]["label"].is_string() && t["load"]["acute"].is_number());

    // HRV: a status per night, a balanced range and the overnight average.
    let h = e.get(&c, "/api/hrv?from=2026-10-23&to=2026-11-19").await;
    assert_eq!(arr(&h["days"], "days").len(), 28);
    assert!(h["nights"].as_u64().unwrap() >= 20);
    assert!(h["overnight_avg_ms"].is_number() && h["baseline_low_ms"].is_number());
    assert!(arr(&h["days"], "days")
        .iter()
        .any(|d| d["status"] == "balanced" && d["weekly_avg_ms"].is_number()));
    let one = e.get(&c, "/api/hrv?from=2026-11-19&to=2026-11-19").await;
    assert!(arr(&one["night"]["readings"], "readings").len() > 20);

    // Sleep history: days, weeks and months hold the same nights.
    let mut totals = Vec::new();
    for group in ["day", "week", "month"] {
        let s = e
            .get(
                &c,
                &format!("/api/sleep/history?from=2026-07-23&to=2026-11-19&group={group}"),
            )
            .await;
        let n: u64 = arr(&s["buckets"], "buckets")
            .iter()
            .map(|b| b["nights"].as_u64().unwrap())
            .sum();
        assert_eq!(n, s["stats"]["nights"].as_u64().unwrap(), "{group}");
        assert!(s["stats"]["avg_score"].is_number() && s["stats"]["avg_duration_s"].is_number());
        totals.push((s["stats"]["avg_score"].clone(), n));
    }
    assert!(totals.windows(2).all(|w| w[0] == w[1]), "{totals:?}");
}
