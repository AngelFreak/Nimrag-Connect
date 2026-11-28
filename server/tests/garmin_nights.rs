//! The morning job that fetches nights the watch never delivered from the user's Garmin account.
//! Every test talks to the fake Garmin in `garmin_fake` (nothing reaches Garmin) and uses a
//! SYNTHETIC night made here (made-up numbers).
//!
//! The clock sits on Sat 28 Nov 2026 in Copenhagen (CET, UTC+1); the night that ends on the 28th
//! is Fri 22:00 to Sat 06:00 local = 21:00Z to 05:00Z.

mod common;
mod garmin_fake;

use axum::http::StatusCode;
use garmin_fake::*;
use nimrag_server::derived::{rank_key, src_id};
use nimrag_server::garmin::{nights, queue};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

const TZ: &str = "Europe/Copenhagen";
/// Account identifiers the response carries; they must never reach a stored blob.
const PROFILE_PK: i64 = 424_242_424;
const SLEEP_ID: i64 = 9_876_543_210;
const NAME: &str = "fake-profile-name";

/// Unix seconds of a UTC time in November 2026.
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

/// Connect's response for the night that ends on `day` November: four stage periods, HRV and
/// heart rate.
fn night(day: u32) -> Value {
    let p = day - 1;
    json!({
        "userProfilePK": PROFILE_PK,
        "dailySleepDTO": {
            "id": SLEEP_ID,
            "userProfilePK": PROFILE_PK,
            "calendarDate": format!("2026-11-{day:02}"),
            "sleepStartTimestampGMT": z(p, 21, 0) * 1000,
            "sleepEndTimestampGMT": z(day, 5, 0) * 1000,
            "deepSleepSeconds": 5400,
            "lightSleepSeconds": 3600,
            "remSleepSeconds": 18000,
            "awakeSleepSeconds": 1800,
            "sleepTimeSeconds": 27000,
            "sleepScores": {"overall": {"value": 81}}
        },
        "sleepLevels": [
            {"startGMT": zs(p, 21, 0), "endGMT": zs(p, 22, 0), "activityLevel": 1.0},
            {"startGMT": zs(p, 22, 0), "endGMT": zs(p, 23, 30), "activityLevel": 0.0},
            {"startGMT": zs(p, 23, 30), "endGMT": zs(day, 4, 30), "activityLevel": 2.0},
            {"startGMT": zs(day, 4, 30), "endGMT": zs(day, 5, 0), "activityLevel": 3.0}
        ],
        "avgOvernightHrv": 48.0,
        "hrvData": [
            {"value": 44.0, "startGMT": z(p, 22, 0) * 1000},
            {"value": 52.5, "startGMT": z(p, 22, 10) * 1000}
        ],
        "sleepHeartRate": [
            {"value": 58, "startGMT": z(p, 22, 0) * 1000},
            {"value": 54, "startGMT": z(p, 22, 4) * 1000}
        ]
    })
}

/// A connected rig whose clock reads `now` (set before the login, so the tokens are fresh) and
/// whose user lives in Copenhagen.
async fn rig_at(now: i64) -> Rig {
    let r = rig().await;
    r.clock.store(now, Ordering::SeqCst);
    nimrag_server::settings::set_user(&r.env.pool, r.env.user_id, "timezone", TZ)
        .await
        .unwrap();
    r.connect().await;
    r
}

fn saturday(h: u32, m: u32) -> i64 {
    common::cph(2026, 11, 28, h, m)
}

fn have(r: &Rig, day: u32) {
    r.fake.with(|f| {
        f.nights.insert(format!("2026-11-{day:02}"), night(day));
    });
}

fn asked(r: &Rig) -> Vec<String> {
    r.fake.with(|f| f.night_requests.clone())
}

async fn pass(r: &Rig) -> nights::Counts {
    nights::run_pass(&r.state).await.unwrap()
}

async fn count(r: &Rig, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(&r.env.pool)
        .await
        .unwrap()
}

async fn outcome(r: &Rig, date: &str) -> Option<String> {
    sqlx::query_scalar("SELECT outcome FROM garmin_night_checks WHERE date = ?")
        .bind(date)
        .fetch_optional(&r.env.pool)
        .await
        .unwrap()
}

/// A watch night: stage rows of a FIT-source file inside the night that ends on 28 November.
async fn watch_has_the_night(r: &Rig) {
    let e = &r.env;
    let sha = "c".repeat(64);
    e.exec(&format!(
        "INSERT INTO files (sha256, size, received_at, source) VALUES ('{sha}', 1, '2026-11-28T06:30:00.000Z', 'watch')"
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO file_owners (sha256, user_id) VALUES ('{sha}', {})",
        e.user_id
    ))
    .await;
    let last = z(28, 5, 0);
    e.exec(&format!(
        "INSERT INTO sources (sha256, src_id, ok, last_ts, msg_count, rank_key) VALUES ('{sha}', {}, 1, {last}, 1, '{}')",
        src_id(&sha),
        rank_key(true, Some(last), 1)
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO sleep_levels (user_id, ts, level, level_name, src) VALUES ({}, {}, 3, 'deep', {})",
        e.user_id,
        z(28, 4, 40),
        src_id(&sha)
    ))
    .await;
}

#[tokio::test]
async fn a_missing_night_is_fetched_once_and_filled() {
    let r = rig_at(saturday(10, 0)).await;
    have(&r, 28);

    let c = pass(&r).await;
    assert_eq!(
        (c.users, c.asked, c.filled, c.no_night, c.errors),
        (1, 7, 1, 6, 0)
    );
    // Newest first: today, then back; the endpoint is the one Garmin Connect's own app uses.
    let a = asked(&r);
    assert_eq!(a.len(), 7, "at most 7 requests per pass");
    assert_eq!(a[0], "2026-11-28");
    assert_eq!(a[6], "2026-11-22");
    assert_eq!(r.fake.with(|f| f.night_name.clone()).as_deref(), Some(NAME));
    assert!(r.fake.with(|f| f
        .requests
        .iter()
        .any(|q| q == "GET /wellness-service/wellness/dailySleepData/fake-profile-name")));

    // Stage rows end their period; HRV and the summary arrive through the `_current` views.
    let uid = r.env.user_id;
    let stages: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT ts || ':' || level FROM sleep_levels_current WHERE user_id = {uid} ORDER BY ts"
    )))
    .fetch_all(&r.env.pool)
    .await
    .unwrap();
    assert_eq!(
        stages,
        vec![
            format!("{}:2", z(27, 22, 0)),
            format!("{}:3", z(27, 23, 30)),
            format!("{}:4", z(28, 4, 30)),
            format!("{}:1", z(28, 5, 0)),
        ]
    );
    assert_eq!(
        count(
            &r,
            &format!("SELECT COUNT(*) FROM hrv_values_current WHERE user_id = {uid}")
        )
        .await,
        2
    );
    assert_eq!(
        count(
            &r,
            &format!("SELECT COUNT(*) FROM hrv_summary_current WHERE user_id = {uid}")
        )
        .await,
        1
    );
    assert_eq!(outcome(&r, "2026-11-28").await.as_deref(), Some("filled"));
    assert_eq!(outcome(&r, "2026-11-27").await.as_deref(), Some("no_night"));

    // Nothing is asked again at once; a filled date never; an empty one after 3 hours.
    assert_eq!(pass(&r).await.asked, 0);
    r.advance(3 * 3600 - 1);
    assert_eq!(pass(&r).await.asked, 0);
    r.advance(1);
    let c = pass(&r).await;
    assert_eq!((c.asked, c.filled, c.no_night), (6, 0, 6));
    assert!(!asked(&r)[7..].contains(&"2026-11-28".to_owned()));
}

#[tokio::test]
async fn the_fetched_blob_is_a_connect_blob_without_account_ids_and_is_never_sent_back() {
    let r = rig_at(saturday(10, 0)).await;
    r.set_send_all(true).await;
    have(&r, 28);
    assert_eq!(pass(&r).await.filled, 1);

    // The stored file is a `connect` file; Forward never queues it.
    assert_eq!(
        count(&r, "SELECT COUNT(*) FROM files WHERE source = 'connect'").await,
        1
    );
    assert_eq!(
        r.queue_len().await,
        0,
        "no garmin_uploads row for the fetched blob"
    );
    assert_eq!(r.run().await, 0);
    assert_eq!(r.fake.upload_count(), 0);

    // No account id, no profile name anywhere in the data directory (files and database).
    let mut seen_blob = false;
    for bytes in all_bytes(r.env.dir.path()) {
        for needle in [&PROFILE_PK.to_string(), &SLEEP_ID.to_string(), NAME] {
            assert!(!contains(&bytes, needle), "{needle} was stored");
        }
        seen_blob |= contains(&bytes, "sleepLevels");
    }
    assert!(seen_blob, "the night itself is stored");
}

#[tokio::test]
async fn a_night_the_watch_has_is_never_asked() {
    let r = rig_at(saturday(10, 0)).await;
    have(&r, 28);
    watch_has_the_night(&r).await;

    let c = pass(&r).await;
    assert_eq!((c.asked, c.filled), (6, 0));
    assert!(!asked(&r).contains(&"2026-11-28".to_owned()));
    assert_eq!(outcome(&r, "2026-11-28").await, None);
    // The watch's rows are the only ones.
    assert_eq!(
        count(&r, "SELECT COUNT(*) FROM sleep_levels_current").await,
        1
    );
}

#[tokio::test]
async fn today_is_asked_from_nine_local_time_only() {
    let r = rig_at(saturday(8, 59)).await;
    have(&r, 28);
    let c = pass(&r).await;
    assert_eq!((c.asked, c.filled), (6, 0));
    assert!(!asked(&r).contains(&"2026-11-28".to_owned()));

    r.advance(61); // 09:00:00
    let c = pass(&r).await;
    assert_eq!((c.asked, c.filled), (1, 1));
    assert_eq!(asked(&r).last().unwrap(), "2026-11-28");
}

#[tokio::test]
async fn nothing_is_asked_when_the_switch_is_off_or_the_account_is_not_connected() {
    let r = rig_at(saturday(10, 0)).await;
    have(&r, 28);

    let profile_calls = r.fake.with(|f| f.profile_calls);
    let (s, j) = r
        .post("/api/garmin/settings", json!({"fill_nights": false}))
        .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(
        (j["fill_nights"].clone(), j["send_all"].clone()),
        (json!(false), json!(false))
    );
    assert_eq!(pass(&r).await.users, 0);

    let (s, j) = r
        .post("/api/garmin/settings", json!({"fill_nights": true}))
        .await;
    assert_eq!((s, j["fill_nights"].clone()), (StatusCode::OK, json!(true)));

    let uid = r.env.user_id;
    let now = r.now();
    queue::set_status(&r.env.pool, uid, queue::AccountStatus::Paused, None, now)
        .await
        .unwrap();
    assert_eq!(pass(&r).await.users, 0);
    queue::expire_login(&r.env.pool, uid, "gone", now)
        .await
        .unwrap();
    assert_eq!(pass(&r).await.users, 0);
    assert!(asked(&r).is_empty());
    assert_eq!(
        r.fake.with(|f| f.profile_calls),
        profile_calls,
        "not even the profile is asked"
    );
}

#[tokio::test]
async fn an_expired_access_token_is_refreshed_once_and_the_request_retried() {
    let r = rig_at(saturday(10, 0)).await;
    have(&r, 28);
    r.fake.revoke_access(); // 401 although `exp` is far away

    let c = pass(&r).await;
    assert_eq!((c.filled, c.errors), (1, 0));
    r.fake
        .with(|f| assert_eq!(f.refresh_calls, 1, "one refresh for the whole pass"));
    assert_eq!(r.status().await["state"], "connected");
}

#[tokio::test]
async fn a_refused_refresh_asks_for_a_new_login() {
    let r = rig_at(saturday(10, 0)).await;
    have(&r, 28);
    r.fake.revoke_access();
    r.fake.with(|f| f.refresh_valid.clear());

    let c = pass(&r).await;
    assert_eq!((c.asked, c.filled), (0, 0));
    assert_eq!(r.status().await["state"], "needs_login");
    assert!(asked(&r).is_empty());
    assert_eq!(
        pass(&r).await.users,
        0,
        "a user who must log in again is left alone"
    );
}

#[tokio::test]
async fn rate_limiting_stops_that_users_pass_and_honours_retry_after() {
    let r = rig_at(saturday(10, 0)).await;
    have(&r, 28);
    r.fake
        .with(|f| f.night_script.push_back(NightAns::Retry(900)));

    let c = pass(&r).await;
    assert_eq!((c.asked, c.errors, c.filled), (1, 1, 0));
    assert_eq!(asked(&r).len(), 1, "the pass stopped at the 429");
    assert_eq!(outcome(&r, "2026-11-28").await.as_deref(), Some("error"));

    // Held back for the Retry-After, with no busy loop.
    r.advance(899);
    assert_eq!(pass(&r).await.users, 0);
    assert_eq!(asked(&r).len(), 1);
    // Then the other dates go on; the errored one waits out its 3 hours.
    r.advance(1);
    let c = pass(&r).await;
    assert_eq!((c.asked, c.filled), (6, 0));
    assert!(!asked(&r)[1..].contains(&"2026-11-28".to_owned()));
}

#[tokio::test]
async fn a_server_error_is_recorded_and_the_other_dates_go_on() {
    let r = rig_at(saturday(10, 0)).await;
    have(&r, 28);
    r.fake
        .with(|f| f.night_script.push_back(NightAns::Status(500)));

    let c = pass(&r).await;
    assert_eq!((c.asked, c.errors, c.no_night, c.filled), (7, 1, 6, 0));
    assert_eq!(outcome(&r, "2026-11-28").await.as_deref(), Some("error"));
    // Asked again after 3 hours, and filled then.
    r.advance(3 * 3600);
    let c = pass(&r).await;
    assert_eq!(c.filled, 1);
    assert_eq!(outcome(&r, "2026-11-28").await.as_deref(), Some("filled"));
}

#[tokio::test]
async fn the_api_reads_and_writes_the_choice_and_the_last_filled_night() {
    let r = rig_at(saturday(10, 0)).await;
    let j = r.status().await;
    assert_eq!(
        (j["fill_nights"].clone(), j["last_filled_night"].clone()),
        (json!(true), Value::Null)
    );

    have(&r, 28);
    pass(&r).await;
    let j = r.status().await;
    assert_eq!(j["last_filled_night"], "2026-11-28");

    // Writing one field leaves the other as it was.
    r.set_send_all(true).await;
    let j = r.status().await;
    assert_eq!(
        (j["send_all"].clone(), j["fill_nights"].clone()),
        (json!(true), json!(true))
    );
    let (s, j) = r
        .post("/api/garmin/settings", json!({"fill_nights": false}))
        .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(
        (j["send_all"].clone(), j["fill_nights"].clone()),
        (json!(true), json!(false))
    );

    // Forward off: the choice and the checks go with the account.
    let (s, _) = r.post("/api/garmin/logout", json!({})).await;
    assert!(s.is_success());
    let j = r.status().await;
    assert_eq!(
        (j["fill_nights"].clone(), j["last_filled_night"].clone()),
        (json!(false), Value::Null)
    );
    assert_eq!(
        count(&r, "SELECT COUNT(*) FROM garmin_night_checks").await,
        0
    );
    let (s, _) = r
        .post("/api/garmin/settings", json!({"fill_nights": true}))
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
}
