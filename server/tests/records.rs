//! Personal records: the best-effort search on synthetic tracks (exact values), and
//! `/api/records` plus the activity page's `records` over hand-made rows.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use nimrag_server::derived::{rank_key, src_id};
use nimrag_server::web_api::records::{best_effort, efforts_of_points};
use serde_json::{json, Value};

fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} is not within {tol} of {b}");
}

/// Samples every `step` seconds at a constant speed (m/s) for `secs` seconds.
fn steady(speed: f64, secs: i64, step: i64) -> Vec<(i64, f64)> {
    (0..=secs / step)
        .map(|i| (i * step, speed * (i * step) as f64))
        .collect()
}

// ---- the search --------------------------------------------------------------------------------

#[test]
fn constant_pace_interpolates_exactly() {
    // Samples 25 s apart; 400 m at 3 m/s is 133.33 s, between samples at both ends.
    let p = steady(3.0, 1000, 25);
    near(best_effort(&p, 400.0).unwrap(), 400.0 / 3.0, 1e-9);
    near(best_effort(&p, 1609.344).unwrap(), 1609.344 / 3.0, 1e-9);
    // The whole track is 3000 m: exactly its length still counts, one metre more does not.
    near(best_effort(&p, 3000.0).unwrap(), 1000.0, 1e-9);
    assert!(best_effort(&p, 3000.5).is_none());
}

#[test]
fn finds_the_fast_stretch_inside_a_run() {
    // 4 m/s for 100 s, then 5 m/s for 400 s, then 3 m/s for 100 s; samples every 10 s.
    let mut p = steady(4.0, 100, 10);
    let mut d = 400.0;
    for i in 1..=40 {
        d += 50.0;
        p.push((100 + i * 10, d));
    }
    for i in 1..=10 {
        d += 30.0;
        p.push((500 + i * 10, d));
    }
    near(best_effort(&p, 1000.0).unwrap(), 200.0, 1e-9);
    near(best_effort(&p, 2000.0).unwrap(), 400.0, 1e-9);
    // 2400 m do not fit in the fast part: the 2000 m at 5 m/s plus 400 m at 4 m/s, 400 s + 100 s.
    near(best_effort(&p, 2400.0).unwrap(), 500.0, 1e-9);
}

#[test]
fn a_pause_is_never_inside_an_effort() {
    // 600 m, a minute without records (the watch was paused), 600 m more at the same pace.
    let mut p = steady(3.0, 200, 10);
    let end = *p.last().unwrap();
    for i in 0..=20 {
        p.push((end.0 + 60 + i * 10, end.1 + 3.0 * (i * 10) as f64));
    }
    assert!(best_effort(&p, 1000.0).is_none());
    near(best_effort(&p, 500.0).unwrap(), 500.0 / 3.0, 1e-9);
    // Without the pause the same distance is found.
    assert!(best_effort(&steady(3.0, 400, 10), 1000.0).is_some());
    // A gap of exactly 30 s is still continuous; 31 s is not.
    let join = |gap: i64| {
        let mut q = steady(3.0, 200, 10);
        let e = *q.last().unwrap();
        q.extend((0..=20).map(|i| (e.0 + gap + i * 10, e.1 + 90.0 + 3.0 * (i * 10) as f64)));
        q
    };
    assert!(best_effort(&join(30), 1000.0).is_some());
    assert!(best_effort(&join(31), 1000.0).is_none());
}

#[test]
fn a_distance_longer_than_the_activity_gives_nothing() {
    let p = steady(3.0, 900, 10); // 2700 m
    assert!(best_effort(&p, 5000.0).is_none());
    assert!(best_effort(&[], 400.0).is_none());
    assert!(best_effort(&[(0, 0.0)], 400.0).is_none());
}

#[test]
fn matches_a_brute_force_search_on_an_uneven_track() {
    // Speeds between 0 and 6 m/s from a small generator, samples 1 to 7 s apart, with stops.
    let mut seed = 12345u64;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as f64 / (1u64 << 31) as f64
    };
    let (mut t, mut d) = (0i64, 0.0f64);
    let mut p = vec![(t, d)];
    for _ in 0..600 {
        let dt = 1 + (next() * 7.0) as i64;
        let v = if next() < 0.15 {
            0.0
        } else {
            1.5 + next() * 4.5
        };
        t += dt;
        d += v * dt as f64;
        p.push((t, d));
    }
    // Reference: try starts on a fine grid, bisect the end on the piecewise linear track.
    let at = |s: f64| -> f64 {
        let i = p
            .partition_point(|q| (q.0 as f64) <= s)
            .min(p.len() - 1)
            .max(1);
        let ((t0, d0), (t1, d1)) = (p[i - 1], p[i]);
        d0 + (d1 - d0) * (s - t0 as f64) / (t1 - t0) as f64
    };
    for target in [400.0, 1000.0, 1609.344] {
        let mut best = f64::MAX;
        let last = p.last().unwrap().0 as f64;
        let mut s = 0.0;
        while s < last {
            let want = at(s) + target;
            if want <= p.last().unwrap().1 {
                let (mut lo, mut hi) = (s, last);
                for _ in 0..50 {
                    let mid = (lo + hi) / 2.0;
                    if at(mid) < want {
                        lo = mid
                    } else {
                        hi = mid
                    }
                }
                best = best.min(hi - s);
            }
            s += 0.02;
        }
        near(best_effort(&p, target).unwrap(), best, 0.05);
    }
}

#[test]
fn efforts_of_points_drops_impossible_speeds_and_other_sports() {
    let p = steady(4.0, 600, 10); // 2400 m
    let e = efforts_of_points("run", &p);
    let keys: Vec<_> = e.iter().map(|x| x.0).collect();
    assert_eq!(keys, ["400m", "1k", "1mi"]);
    near(e[0].1, 100.0, 1e-9);
    // A jump of 500 m in one second is a bad reading, not a 400 m effort.
    let glitch = [(0, 0.0), (10, 10.0), (11, 510.0), (20, 520.0)];
    assert!(efforts_of_points("run", &glitch).is_empty());
    assert!(efforts_of_points("walk", &p).is_empty());
    assert_eq!(efforts_of_points("cycle", &steady(8.0, 5200, 10)).len(), 2);
}

// ---- the API -----------------------------------------------------------------------------------

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

fn rec<'a>(j: &'a Value, group: &str, kind: &str) -> &'a Value {
    let g = j["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["key"] == group)
        .unwrap_or_else(|| panic!("no group {group}"));
    g["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == kind)
        .unwrap_or_else(|| panic!("no record {kind}"))
}

async fn count(e: &Env, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
        .fetch_one(&e.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn records_with_previous_bests_and_the_activity_badge() {
    let (e, cookie) = setup().await;
    // Mon 2 Nov: 5 km at 4 m/s. Wed 11 Nov: 10 km at 4.5 m/s. Sat 14 Nov: 3 km at 5 m/s.
    let a = insert(&e, &run('a', 2, 4.0, 1250, 30.0)).await;
    let b = insert(&e, &run('b', 11, 4.5, 2222, 20.0)).await;
    let c = insert(&e, &run('c', 14, 5.0, 600, 5.0)).await;
    // Fri 13 Nov: 80 minutes of strength training, no distance.
    let strength = Spec {
        c: 'd',
        sport: 10,
        sub: 20,
        start: at(13, 18, 0),
        speed: 0.0,
        secs: 4800,
        ascent: 0.0,
        owner: None,
    };
    insert(&e, &strength).await;
    // A 25 km ride at 6 m/s on Sun 15 Nov.
    let ride = Spec {
        c: 'e',
        sport: 2,
        sub: 0,
        start: at(15, 9, 0),
        speed: 6.0,
        secs: 4170,
        ascent: 120.0,
        owner: None,
    };
    let r = insert(&e, &ride).await;

    let j = e.get(&cookie, "/api/records").await;
    let keys: Vec<_> = j["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["run", "walk", "cycle", "all", "steps"]);

    // 1 km: the 5 m/s run of Sat 14 Nov, 200 s; the best before it was Wed's 222.22 s.
    let k1 = rec(&j, "run", "run_1k");
    assert_eq!(
        (k1["unit"].as_str(), k1["value"].as_f64()),
        (Some("s"), Some(200.0))
    );
    assert_eq!(k1["activity"]["id"], c);
    assert_eq!(k1["date"], "2026-11-14");
    near(
        k1["previous"]["value"].as_f64().unwrap(),
        1000.0 / 4.5,
        1e-6,
    );
    assert_eq!(k1["previous"]["date"], "2026-11-11");
    // 400 m and a mile: Saturday as well.
    assert_eq!(rec(&j, "run", "run_400m")["value"].as_f64(), Some(80.0));
    near(
        rec(&j, "run", "run_1mi")["value"].as_f64().unwrap(),
        1609.344 / 5.0,
        1e-6,
    );
    // 5 km: only Mon and Wed ran that far; Wed is the best, Mon the previous.
    let k5 = rec(&j, "run", "run_5k");
    assert_eq!(k5["activity"]["id"], b);
    near(k5["value"].as_f64().unwrap(), 5000.0 / 4.5, 1e-6);
    assert_eq!(k5["previous"]["value"].as_f64(), Some(1250.0));
    assert_eq!(k5["previous"]["date"], "2026-11-02");
    // 10 km is 2222 s * 4.5 = 9999 m: just short, so nothing yet. No half marathon either.
    assert!(rec(&j, "run", "run_10k")["value"].is_null());
    assert!(rec(&j, "run", "run_half")["value"].is_null());
    assert!(rec(&j, "run", "run_marathon")["value"].is_null());
    // Longest run and most ascent.
    let long = rec(&j, "run", "run_longest");
    assert_eq!(long["activity"]["id"], b);
    assert_eq!(long["value"].as_f64(), Some(9999.0));
    assert_eq!(long["previous"]["value"].as_f64(), Some(5000.0));
    let asc = rec(&j, "run", "run_ascent");
    assert_eq!(
        (asc["activity"]["id"].as_str(), asc["value"].as_f64()),
        (Some(a.as_str()), Some(30.0))
    );
    assert!(asc["previous"].is_null());
    // Walks: none yet.
    assert!(rec(&j, "walk", "walk_longest")["value"].is_null());
    // The ride: 20 km in 3333.33 s, no 40 km.
    let c20 = rec(&j, "cycle", "cycle_20k");
    near(c20["value"].as_f64().unwrap(), 20000.0 / 6.0, 1e-6);
    assert_eq!(c20["activity"]["id"], r);
    assert!(rec(&j, "cycle", "cycle_40k")["value"].is_null());
    assert_eq!(
        rec(&j, "cycle", "cycle_longest")["value"].as_f64(),
        Some(25020.0)
    );
    // Longest by time: the strength session; the run of Wed was the best before.
    let t = rec(&j, "all", "longest_time");
    assert_eq!(t["value"].as_f64(), Some(4800.0));
    assert_eq!(t["activity"]["kind"], "strength");
    assert_eq!(t["previous"]["value"].as_f64(), Some(2222.0));

    // The activity page lists what it holds.
    let d = e.get(&cookie, &format!("/api/activities/{b}")).await;
    assert_eq!(d["records"], json!(["run_5k", "run_longest"]));
    let d = e.get(&cookie, &format!("/api/activities/{c}")).await;
    assert_eq!(d["records"], json!(["run_400m", "run_1k", "run_1mi"]));
    let d = e.get(&cookie, &format!("/api/activities/{a}")).await;
    assert_eq!(d["records"], json!(["run_ascent"]));
}

#[tokio::test]
async fn an_empty_account_has_every_record_empty() {
    let (e, cookie) = setup().await;
    let j = e.get(&cookie, "/api/records").await;
    for g in j["groups"].as_array().unwrap() {
        assert!(!g["records"].as_array().unwrap().is_empty(), "{g}");
        for r in g["records"].as_array().unwrap() {
            assert!(r["value"].is_null() && r["activity"].is_null() && r["previous"].is_null());
        }
    }
}

#[tokio::test]
async fn step_records_per_day_and_per_week() {
    let (e, cookie) = setup().await;
    // Whole-day totals (Garmin's) for the weeks of Mon 2 and Mon 9 Nov.
    for (day, steps) in [(2, 8000), (3, 12000), (9, 6000), (10, 5000)] {
        e.exec(&format!(
            "INSERT INTO daily_totals (ts, steps, src) VALUES ({}, {steps}, 1)",
            at(day, 0, 0)
        ))
        .await;
    }
    // A day with watch counters wins over a total: Mon 16 Nov, walking 9000 and running 4000
    // by the evening (largest reading per type, summed), after a smaller one earlier.
    for (h, ty, steps) in [(10, 0, 3000), (20, 0, 9000), (20, 1, 4000)] {
        e.exec(&format!(
            "INSERT INTO mon_activity (ts, activity_type, steps, src) VALUES ({}, {ty}, {steps}, 1)",
            at(16, h, 0)
        ))
        .await;
    }
    e.exec(&format!(
        "INSERT INTO daily_totals (ts, steps, src) VALUES ({}, 99999, 1)",
        at(16, 0, 0)
    ))
    .await;

    let j = e.get(&cookie, "/api/records").await;
    let day = rec(&j, "steps", "steps_day");
    assert_eq!(day["value"].as_f64(), Some(13000.0));
    assert_eq!(day["date"], "2026-11-16");
    assert!(day["activity"].is_null());
    assert_eq!(day["previous"]["value"].as_f64(), Some(12000.0));
    assert_eq!(day["previous"]["date"], "2026-11-03");
    // Weeks run Monday to Sunday: 20000, 11000, 13000.
    let week = rec(&j, "steps", "steps_week");
    assert_eq!(week["value"].as_f64(), Some(20000.0));
    assert_eq!(week["date"], "2026-11-02");
    assert!(week["previous"].is_null());
}

#[tokio::test]
async fn efforts_are_cached_and_recomputed_after_a_rederive() {
    let (e, cookie) = setup().await;
    let a = insert(&e, &run('a', 2, 4.0, 1250, 0.0)).await;
    assert_eq!(count(&e, "activity_efforts").await, 0);
    let j = e.get(&cookie, "/api/records").await;
    assert_eq!(rec(&j, "run", "run_1k")["value"].as_f64(), Some(250.0));
    // 400 m, 1 km, a mile and 5 km, and the file is marked as done.
    assert_eq!(count(&e, "activity_efforts").await, 4);
    assert_eq!(count(&e, "activity_efforts_done").await, 1);

    // The cache answers: change a stored effort and the API shows the stored one.
    e.exec("UPDATE activity_efforts SET seconds = 240 WHERE effort = '1k'")
        .await;
    let j = e.get(&cookie, "/api/records").await;
    assert_eq!(rec(&j, "run", "run_1k")["value"].as_f64(), Some(240.0));

    // Re-deriving the file clears its cache (and its records), as `persist` does first.
    let mut tx = e.pool.begin().await.unwrap();
    nimrag_server::derived::clear_file(&mut tx, &a)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(count(&e, "activity_efforts").await, 0);
    assert_eq!(count(&e, "activity_efforts_done").await, 0);
}

#[tokio::test]
async fn another_users_activities_do_not_count() {
    let (e, cookie) = setup().await;
    let other = e.add_user("other", nimrag_server::users::Role::User).await;
    // The other user's run is faster and longer than mine.
    let mut theirs = run('f', 5, 6.0, 1800, 400.0);
    theirs.owner = Some(other);
    insert(&e, &theirs).await;
    let mine = insert(&e, &run('a', 2, 4.0, 1250, 30.0)).await;

    let j = e.get(&cookie, "/api/records").await;
    assert_eq!(rec(&j, "run", "run_5k")["value"].as_f64(), Some(1250.0));
    assert_eq!(rec(&j, "run", "run_5k")["activity"]["id"], mine);
    assert_eq!(
        rec(&j, "run", "run_longest")["value"].as_f64(),
        Some(5000.0)
    );
    assert_eq!(rec(&j, "run", "run_ascent")["value"].as_f64(), Some(30.0));
    assert_eq!(
        rec(&j, "all", "longest_time")["value"].as_f64(),
        Some(1250.0)
    );
    // Their file is not computed for me, and I cannot open it.
    let done: Vec<String> = sqlx::query_scalar("SELECT sha256 FROM activity_efforts_done")
        .fetch_all(&e.pool)
        .await
        .unwrap();
    assert_eq!(done, [mine]);
    let (s, _, _) = e
        .call(
            Method::GET,
            &format!("/api/activities/{}", sha('f')),
            Some(&cookie),
            false,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // And the other way round: they see their own numbers only.
    let theirs_cookie = e.login_as("other").await;
    let (s, _, j) = e
        .call(
            Method::GET,
            "/api/records",
            Some(&theirs_cookie),
            false,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    near(
        rec(&j, "run", "run_5k")["value"].as_f64().unwrap(),
        5000.0 / 6.0,
        1e-9,
    );
}
