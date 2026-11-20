//! Two users on one server: nothing one of them owns is visible or changeable by the other.
//! User `demo` owns 120 days of seeded data; user `bea` owns nothing and must see exactly what a
//! brand-new, empty server shows (`sample` aside, which is a server-wide flag).

mod common;
mod garmin_fake;

use axum::http::{Method, StatusCode};
use chrono_tz::Europe::Copenhagen;
use common::*;
use nimrag_core::derive::test_support::{FitBuilder, Fv};
use nimrag_server::seed::{self, SeedAccounts, SeedOptions};
use nimrag_server::users::Role;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

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

/// `demo` with data, `bea` without; both signed in. Returns (env, demo cookie, bea cookie).
async fn two_users() -> (Env, String, String) {
    let mut e = bare_env_at(now()).await;
    let report = seed::run(&e.pool, e.dir.path(), options()).await.unwrap();
    e.user_id = report.user_id;
    e.add_user("bea", Role::User).await;
    let demo = e.login_as("demo").await;
    let bea = e.login_as("bea").await;
    (e, demo, bea)
}

/// An empty server with one user, for what "nothing" looks like.
async fn empty() -> (Env, String) {
    let e = bare_env_at(now()).await;
    e.add_user("ref", Role::User).await;
    let c = e.login_as("ref").await;
    (e, c)
}

/// The server-wide `sample` flag is not user data.
fn strip_sample(v: &mut Value) {
    match v {
        Value::Object(m) => {
            m.remove("sample");
            // Server-wide, not the user's data.
            m.remove("database_bytes");
            m.values_mut().for_each(strip_sample);
        }
        Value::Array(a) => a.iter_mut().for_each(strip_sample),
        _ => {}
    }
}

async fn read(e: &Env, cookie: &str, uri: &str) -> (StatusCode, Value) {
    let (s, _, mut j) = e.call(Method::GET, uri, Some(cookie), false, None).await;
    strip_sample(&mut j);
    (s, j)
}

const READS: &[&str] = &[
    "/api/today",
    "/api/days/2026-11-18",
    "/api/days/2026-11-19",
    "/api/activities?limit=100",
    "/api/sleep/2026-11-18",
    "/api/sleep?nights=30",
    "/api/trends?range=7d",
    "/api/trends?range=3m",
    "/api/movement/2026-11-18",
    "/api/training",
    "/api/body",
    "/api/calendar/2026-11",
    "/api/calendar/2026-10",
    "/api/devices",
    "/api/storage",
    "/api/settings",
    "/api/tokens",
    "/api/garmin",
];

fn hr_file(t: u32, bpm: u8) -> Vec<u8> {
    FitBuilder::new()
        .message(55, &[(253, Fv::U32(t)), (27, Fv::U8(bpm))])
        .build()
}

#[tokio::test]
async fn a_user_without_data_sees_exactly_an_empty_server_on_every_data_endpoint() {
    let (e, demo, bea) = two_users().await;
    let (empty_env, ref_cookie) = empty().await;
    for uri in READS {
        let theirs = read(&e, &bea, uri).await;
        let blank = read(&empty_env, &ref_cookie, uri).await;
        assert_eq!(theirs, blank, "{uri}: bea must see an empty server");
        let mine = read(&e, &demo, uri).await;
        assert_eq!(mine.0, StatusCode::OK, "{uri}");
        if !matches!(*uri, "/api/devices" | "/api/tokens" | "/api/garmin" | "/api/settings" | "/api/storage") {
            assert_ne!(mine.1, blank.1, "{uri}: demo does have data here");
        }
    }
}

#[tokio::test]
async fn by_id_endpoints_answer_404_for_someone_elses_activities_and_changes_nothing() {
    let (e, demo, bea) = two_users().await;
    let list = e.get(&demo, "/api/activities?limit=100").await;
    let items = list["items"].as_array().expect("items array");
    assert!(items.len() >= 10, "{list}");
    let strength = items
        .iter()
        .find(|a| a["kind"] == "strength")
        .expect("the seed has a strength session");
    let strength_id = strength["id"].as_str().unwrap().to_owned();

    for a in items.iter().take(5) {
        let id = a["id"].as_str().unwrap();
        for suffix in ["", "/track", "/strength"] {
            let uri = format!("/api/activities/{id}{suffix}");
            let (s, _, _) = e.call(Method::GET, &uri, Some(&bea), false, None).await;
            assert_eq!(s, StatusCode::NOT_FOUND, "bea GET {uri}");
        }
    }
    // The owner sees the strength detail; bea cannot edit it, and the edit leaves no trace.
    let before = e.get(&demo, &format!("/api/activities/{strength_id}/strength")).await;
    assert!(before["sets"].as_u64().unwrap_or(0) > 0, "{}", before["exercises"]);
    let (s, _) = e
        .send(&bea, Method::PATCH, &format!("/api/activities/{strength_id}/strength"), json!({ "sets": [{ "idx": 0, "reps": 99.0 }] }))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM strength_set_overrides").fetch_one(&e.pool).await.unwrap();
    assert_eq!(n, 0);
    assert_eq!(e.get(&demo, &format!("/api/activities/{strength_id}/strength")).await, before);
    // Their own edit works and stays theirs.
    let (s, _) = e
        .send(&demo, Method::PATCH, &format!("/api/activities/{strength_id}/strength"), json!({ "sets": [{ "idx": 0, "reps": 11.0 }] }))
        .await;
    assert_eq!(s, StatusCode::OK);
    let owner: i64 = sqlx::query_scalar("SELECT user_id FROM strength_set_overrides LIMIT 1").fetch_one(&e.pool).await.unwrap();
    assert_eq!(owner, e.user_id);
}

#[tokio::test]
async fn body_entries_tokens_and_settings_are_separate_per_user() {
    let (e, demo, bea) = two_users().await;

    // Body measurements: bea cannot delete demo's entry; each sees only their own.
    let entries = e.get(&demo, "/api/body").await["entries"].as_array().unwrap().clone();
    assert!(!entries.is_empty());
    let id = entries[0]["id"].as_i64().unwrap();
    let (s, _) = e.send(&bea, Method::DELETE, &format!("/api/body/{id}"), json!(null)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(e.get(&demo, "/api/body").await["entries"].as_array().unwrap().len(), entries.len());
    let (s, _) = e.send(&bea, Method::POST, "/api/body", json!({ "weight_kg": 61.5 })).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(e.get(&bea, "/api/body").await["entries"].as_array().unwrap().len(), 1);
    assert_eq!(e.get(&demo, "/api/body").await["entries"].as_array().unwrap().len(), entries.len());

    // Device tokens: bea cannot see or revoke demo's.
    let demo_token = e.token_for(e.user_id).await;
    let list = e.get(&demo, "/api/tokens").await;
    let tid = list["tokens"][0]["id"].as_i64().unwrap();
    assert!(e.get(&bea, "/api/tokens").await["tokens"].as_array().unwrap().is_empty());
    let (s, _) = e.send(&bea, Method::DELETE, &format!("/api/tokens/{tid}"), json!(null)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = e.request(Method::POST, "/api/files?data_type=4&source=watch", Some(&demo_token), b"still works".to_vec()).await;
    assert_eq!(s, StatusCode::CREATED, "the token is still alive");

    // Settings: a change by one never reaches the other.
    let demo_before = e.get(&demo, "/api/settings").await;
    let bea_before = e.get(&bea, "/api/settings").await;
    let (s, _) = e.send(&bea, Method::PATCH, "/api/settings", json!({ "step_goal": 7777, "timezone": "Asia/Tokyo" })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(e.get(&demo, "/api/settings").await, demo_before);
    let after = e.get(&bea, "/api/settings").await;
    assert_eq!((after["step_goal"].as_i64(), after["timezone"].as_str()), (Some(7777), Some("Asia/Tokyo")));
    assert_ne!(after, bea_before);
    let (s, _) = e.send(&demo, Method::PATCH, "/api/settings", json!({ "step_goal": 9100 })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(e.get(&bea, "/api/settings").await, after);
    // A day boundary follows each user's own time zone.
    let t = e.get(&bea, "/api/today").await;
    assert_eq!(t["timezone"], "Asia/Tokyo");
}

#[tokio::test]
async fn files_belong_to_whoever_stored_them_first() {
    let e = env_at(now()).await;
    e.adopt.store(false, Ordering::Relaxed);
    let a_tok = e.token.clone();
    let b_id = e.add_user("bea", Role::User).await;
    let b_tok = e.token_for(b_id).await;

    let bytes = hr_file(1_000_000_000, 66);
    let (s, up) = e.request(Method::POST, "/api/files?data_type=128&source=watch", Some(&a_tok), bytes.clone()).await;
    assert_eq!(s, StatusCode::CREATED, "{up}");
    let sha = up["sha256"].as_str().unwrap().to_owned();

    let (s, _) = e.request(Method::GET, &format!("/api/files/{sha}"), Some(&a_tok), vec![]).await;
    assert_eq!(s, StatusCode::OK, "the owner's phone can ask about its file");
    let (s, _) = e.request(Method::GET, &format!("/api/files/{sha}"), Some(&b_tok), vec![]).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "another user's phone cannot");

    // The same bytes from the second user do not take the file over.
    let (s, _) = e.request(Method::POST, "/api/files?data_type=128&source=watch", Some(&b_tok), bytes).await;
    assert!(s == StatusCode::OK || s == StatusCode::CREATED, "{s}");
    let owners: Vec<i64> = sqlx::query_scalar("SELECT user_id FROM file_owners WHERE sha256 = ?").bind(&sha).fetch_all(&e.pool).await.unwrap();
    assert_eq!(owners, vec![e.user_id]);

    // Counts per user.
    let a = e.login().await;
    let b = e.login_as("bea").await;
    assert_eq!(e.get(&a, "/api/storage").await["files"], 1);
    assert_eq!(e.get(&b, "/api/storage").await["files"], 0);
}

#[tokio::test]
async fn samples_with_the_same_timestamp_from_two_users_coexist() {
    if !nimrag_core::fit::profile::available() {
        return;
    }
    let e = env_at(now()).await;
    e.adopt.store(false, Ordering::Relaxed);
    let b_id = e.add_user("bea", Role::User).await;
    let b_tok = e.token_for(b_id).await;
    let t = 1_000_000_000;
    let (s, _) = e.request(Method::POST, "/api/files?data_type=128&source=watch", Some(&e.token), hr_file(t, 61)).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = e.request(Method::POST, "/api/files?data_type=128&source=watch", Some(&b_tok), hr_file(t, 99)).await;
    assert_eq!(s, StatusCode::CREATED);
    let rows: Vec<(i64, i64)> = sqlx::query_as("SELECT user_id, bpm FROM mon_hr ORDER BY user_id").fetch_all(&e.pool).await.unwrap();
    assert_eq!(rows, vec![(e.user_id.min(b_id), if e.user_id < b_id { 61 } else { 99 }), (e.user_id.max(b_id), if e.user_id < b_id { 99 } else { 61 })]);
}

#[tokio::test]
async fn forward_to_garmin_is_per_user() {
    use garmin_fake::*;
    let r = rig().await;
    r.connect().await;
    // A second user with their own phone token; not connected to Garmin.
    let b_id = r.env.add_user("bea", Role::User).await;
    let b_tok = r.env.token_for(b_id).await;
    let b_cookie = r.env.login_as("bea").await;
    assert_eq!(r.env.get(&b_cookie, "/api/garmin").await["state"], "off");
    assert_eq!(r.status().await["state"], "connected");

    // Bea's activity is stored but not queued: she has no Garmin login.
    let (s, _) = r.env.request(Method::POST, "/api/files?data_type=128&sub_type=4&source=watch", Some(&b_tok), common::tiny_fit(7)).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(r.queue_len().await, 0);
    // The owner's file is queued and goes out; the fake sees exactly one upload.
    let sha = r.activity(1).await;
    assert_eq!(r.queue_len().await, 1);
    assert_eq!(r.run().await, 1);
    assert_eq!(r.item(&sha).await.0, "done");
    assert_eq!(r.fake.upload_count(), 1);
    // Bea cannot retry, back-fill or log out the owner's connection through her own session.
    let (s, j) = r.env.send(&b_cookie, Method::POST, "/api/garmin/retry", json!(null)).await;
    assert_ne!(s, StatusCode::OK, "{j}");
    let (s, _) = r.env.send(&b_cookie, Method::POST, "/api/garmin/logout", json!(null)).await;
    assert!(s.is_success() || s.is_client_error());
    assert_eq!(r.status().await["state"], "connected", "the owner stays connected");
    assert_eq!(r.status().await["done"], 1);
    // Bea connects her own account: her earlier file is not back-filled by magic, new ones are queued for her.
    let (s, j) = r.env.send(&b_cookie, Method::POST, "/api/garmin/login", json!({ "email": EMAIL, "password": GARMIN_PASSWORD })).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    let (s, _) = r.env.request(Method::POST, "/api/files?data_type=128&sub_type=4&source=watch", Some(&b_tok), common::tiny_fit(8)).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(r.run().await, 1);
    let queued: Vec<(i64,)> = sqlx::query_as("SELECT DISTINCT user_id FROM garmin_uploads ORDER BY user_id").fetch_all(&r.env.pool).await.unwrap();
    assert_eq!(queued.len(), 2);
    assert_eq!(r.fake.upload_count(), 2);
    assert_eq!(r.env.get(&b_cookie, "/api/garmin").await["done"], 1);
    assert_eq!(r.status().await["done"], 1);
}
