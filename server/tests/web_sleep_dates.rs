//! Which night belongs to a date: Garmin's convention, the night that ENDS on that local day's
//! morning. Day (`/api/days/{date}`), Calendar (`/api/calendar/{month}`), the Sleep page
//! (`/api/sleep/{date}`, `/api/sleep?nights=`) and Today must all agree, in the account's zone.

mod common;

use axum::http::{Method, StatusCode};
use chrono::TimeZone;
use common::*;
use nimrag_server::derived::{rank_key, src_id};
use serde_json::{json, Value};

async fn setup(now: i64, tz: &str) -> (Env, String) {
    let e = env_at(now).await;
    let cookie = e.login().await;
    let (s, _) = e
        .send(
            &cookie,
            Method::PATCH,
            "/api/settings",
            json!({ "timezone": tz }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    (e, cookie)
}

fn utc(d: u32, h: u32, m: u32) -> i64 {
    chrono::Utc
        .with_ymd_and_hms(2026, 11, d, h, m, 0)
        .unwrap()
        .timestamp()
}

/// A night without stage rows (so time asleep is the whole span).
async fn night(e: &Env, id: char, start: i64, end: i64, score: f64) {
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
        "INSERT INTO sleep_sessions (sha256, idx, start_ts, end_ts, overall_score) VALUES ('{sha}', 0, {start}, {end}, {score})"
    ))
    .await;
}

fn calendar_day<'a>(cal: &'a Value, date: &str) -> &'a Value {
    cal["days"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["date"] == date)
        .unwrap()
}

/// Thu 19 Nov 23:32 to Fri 20 Nov 06:36 local, and the night before it (Wed to Thu), seen on
/// Fri 20 Nov late morning: Day, Calendar, Sleep and Today all give Fri 20 the Thu-to-Fri night.
async fn check(tz: &str, at: impl Fn(u32, u32, u32) -> i64) {
    let (e, c) = setup(at(20, 10, 0), tz).await;
    night(&e, 'a', at(18, 23, 10), at(19, 6, 50), 71.0).await;
    night(&e, 'b', at(19, 23, 32), at(20, 6, 36), 84.0).await;
    let seven_h_4 = f64::from(7 * 3600 + 4 * 60);

    // Day view: Fri 20 has the night ending that morning, Thu 19 the one before, Sat none.
    let d20 = e.get(&c, "/api/days/2026-11-20").await;
    assert_eq!(
        d20["sleep"]["date"], "2026-11-20",
        "{tz}: day 20 sleep: {d20}"
    );
    assert_eq!(d20["sleep"]["duration_s"], seven_h_4);
    assert_eq!(d20["sleep"]["score"], 84.0);
    let d19 = e.get(&c, "/api/days/2026-11-19").await;
    assert_eq!(d19["sleep"]["date"], "2026-11-19");
    assert_eq!(d19["sleep"]["score"], 71.0);
    assert_eq!(
        d20["energy"]["wake_time"], d20["sleep"]["end"],
        "wake time and night agree"
    );
    let d21 = e.get(&c, "/api/days/2026-11-21").await;
    assert!(d21["sleep"].is_null());

    // Calendar: same nights on the same days.
    let cal = e.get(&c, "/api/calendar/2026-11").await;
    let (c19, c20, c21) = (
        calendar_day(&cal, "2026-11-19"),
        calendar_day(&cal, "2026-11-20"),
        calendar_day(&cal, "2026-11-21"),
    );
    assert_eq!(c20["sleep_duration_s"], seven_h_4, "{tz}: calendar 20");
    assert_eq!(c20["sleep_score"], 84.0);
    assert_eq!(c19["sleep_score"], 71.0);
    assert!(c21["sleep_duration_s"].is_null());

    // Sleep page: the date is the wake-up day, the night is the same one, and the list ends on it.
    let s20 = e.get(&c, "/api/sleep/2026-11-20").await;
    assert_eq!(s20["night"]["date"], "2026-11-20", "{tz}: sleep 20");
    assert_eq!(s20["night"]["start"], d20["sleep"]["start"]);
    assert_eq!(s20["night"]["end"], d20["sleep"]["end"]);
    assert_eq!(s20["night"]["score"], 84.0);
    let s19 = e.get(&c, "/api/sleep/2026-11-19").await;
    assert_eq!(s19["night"]["score"], 71.0);
    assert!(e.get(&c, "/api/sleep/2026-11-21").await["night"].is_null());
    let last: Vec<&str> = s20["last_nights"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["date"].as_str().unwrap())
        .collect();
    assert_eq!(&last[last.len() - 2..], ["2026-11-19", "2026-11-20"]);
    let list = e.get(&c, "/api/sleep?nights=7").await;
    assert_eq!(
        list["nights"].as_array().unwrap().last().unwrap()["date"],
        "2026-11-20"
    );

    // Sleep history buckets the night on the wake-up day too.
    let h = e
        .get(&c, "/api/sleep/history?from=2026-11-18&to=2026-11-20")
        .await;
    let b = h["buckets"].as_array().unwrap();
    assert_eq!(b.len(), 3);
    assert_eq!(b[1]["avg_score"], 71.0);
    assert_eq!(b[2]["avg_score"], 84.0);

    // Today: it is Fri 20, today's night is the newest and is last night's sleep.
    let today = e.get(&c, "/api/today").await;
    assert_eq!(today["sleep"]["start"], d20["sleep"]["start"]);
    assert_eq!(
        today["yesterday"]["sleep_score"], 71.0,
        "yesterday = night ending yesterday"
    );
}

#[tokio::test]
async fn night_belongs_to_the_day_it_ends_copenhagen() {
    check("Europe/Copenhagen", |d, h, m| cph(2026, 11, d, h, m)).await;
}

#[tokio::test]
async fn night_belongs_to_the_day_it_ends_utc() {
    check("Etc/UTC", utc).await;
}

/// Before waking up the day has no night yet; the previous one stays on its own date.
#[tokio::test]
async fn no_night_yet_for_today_before_waking() {
    let (e, c) = setup(cph(2026, 11, 20, 1, 0), "Europe/Copenhagen").await;
    night(
        &e,
        'a',
        cph(2026, 11, 18, 23, 0),
        cph(2026, 11, 19, 7, 0),
        70.0,
    )
    .await;
    assert!(e.get(&c, "/api/days/2026-11-20").await["sleep"].is_null());
    assert_eq!(
        e.get(&c, "/api/days/2026-11-19").await["sleep"]["score"],
        70.0
    );
    let list = e.get(&c, "/api/sleep?nights=7").await;
    assert_eq!(
        list["nights"].as_array().unwrap().last().unwrap()["date"],
        "2026-11-19"
    );
}
