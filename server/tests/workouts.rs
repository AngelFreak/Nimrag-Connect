//! Workouts (stage 4a): the owner's JSON API, the send-to-watch queue, and the phone's endpoints.
//! Encoding needs the FIT profile, which is not in every build: the tests branch on
//! `profile::available()` and check the clear "unavailable" answer where it is missing.

mod common;

use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode},
};
use common::*;
use http_body_util::BodyExt;
use nimrag_core::fit::profile;
use nimrag_server::users::Role;
use serde_json::{json, Value};
use tower::ServiceExt;

const NOW: i64 = 1_800_000_000;

fn interval_session() -> Value {
    json!({
        "name": "4 x 400 m",
        "sport": "running",
        "note": "Easy first lap",
        "nodes": [
            { "type": "step", "intensity": "warmup",
              "duration": { "kind": "time", "seconds": 600 },
              "target": { "kind": "none" } },
            { "type": "repeat", "count": 4, "steps": [
                { "intensity": "active",
                  "duration": { "kind": "distance", "meters": 400 },
                  "target": { "kind": "pace", "fast_s_per_km": 270.0, "slow_s_per_km": 285.0 },
                  "note": "Relaxed shoulders" },
                { "intensity": "rest",
                  "duration": { "kind": "time", "seconds": 90 },
                  "target": { "kind": "none" } },
            ] },
            { "type": "step", "intensity": "cooldown",
              "duration": { "kind": "open" },
              "target": { "kind": "heart_rate_zone", "zone": 1 } },
        ]
    })
}

fn timed_session() -> Value {
    json!({
        "name": "Tempo",
        "sport": "cycling",
        "nodes": [
            { "type": "step", "intensity": "warmup",
              "duration": { "kind": "time", "seconds": 300 }, "target": { "kind": "none" } },
            { "type": "repeat", "count": 3, "steps": [
                { "intensity": "active", "duration": { "kind": "time", "seconds": 120 },
                  "target": { "kind": "cadence", "low": 85, "high": 95 } },
                { "intensity": "recovery", "duration": { "kind": "time", "seconds": 60 },
                  "target": { "kind": "none" } },
            ] },
        ]
    })
}

async fn create(e: &Env, cookie: &str, body: Value) -> Value {
    let (s, j) = e.send(cookie, Method::POST, "/api/workouts", body).await;
    assert_eq!(s, StatusCode::CREATED, "{j}");
    j
}

/// A request as the phone makes it: a Bearer token, no cookie, no CSRF header. Returns the raw body.
async fn device(
    e: &Env,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let body = match body {
        Some(b) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let resp = e
        .app
        .clone()
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let (status, headers) = (resp.status(), resp.headers().clone());
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, headers, bytes.to_vec())
}

fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

async fn state_of(e: &Env, id: i64) -> String {
    sqlx::query_scalar("SELECT send_state FROM workouts WHERE id = ?")
        .bind(id)
        .fetch_one(&e.pool)
        .await
        .unwrap()
}

// ---- the owner's API --------------------------------------------------------------------------

#[tokio::test]
async fn create_read_update_list_and_delete() {
    let e = env_at(NOW).await;
    let c = e.login().await;

    let made = create(&e, &c, interval_session()).await;
    let id = made["id"].as_i64().unwrap();
    assert_eq!(made["revision"], 1);
    assert_eq!(made["workout"], interval_session());
    assert_eq!(made["send"]["state"], "idle");

    let list = e.get(&c, "/api/workouts").await;
    assert_eq!(list["workouts"].as_array().unwrap().len(), 1);
    let w = &list["workouts"][0];
    assert_eq!(
        (w["name"].as_str(), w["sport"].as_str(), w["steps"].as_u64()),
        (Some("4 x 400 m"), Some("running"), Some(4))
    );
    // The distance and the open step mean there is no total time.
    assert!(w["duration_s"].is_null());
    assert_eq!(list["limits"]["max_steps"], 100);
    assert_eq!(list["limits"]["repeat_min"], 2);
    assert_eq!(list["watch_send"]["available"], profile::available());

    let mut edited = interval_session();
    edited["name"] = json!("  5 x 400 m  ");
    edited["nodes"][1]["count"] = json!(5);
    let (s, j) = e
        .send(&c, Method::PUT, &format!("/api/workouts/{id}"), edited)
        .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["revision"], 2);
    assert_eq!(
        j["workout"]["name"], "  5 x 400 m  ",
        "the body is kept as sent"
    );
    let list = e.get(&c, "/api/workouts").await;
    assert_eq!(
        list["workouts"][0]["name"], "5 x 400 m",
        "the name is trimmed"
    );

    let got = e.get(&c, &format!("/api/workouts/{id}")).await;
    assert_eq!(got["workout"]["nodes"][1]["count"], 5);

    let (s, _) = e
        .send(
            &c,
            Method::DELETE,
            &format!("/api/workouts/{id}"),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(e.get(&c, "/api/workouts").await["workouts"], json!([]));
    let (s, _, _) = e
        .call(
            Method::GET,
            &format!("/api/workouts/{id}"),
            Some(&c),
            false,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_timed_workout_reports_its_total_time() {
    let e = env_at(NOW).await;
    let c = e.login().await;
    create(&e, &c, timed_session()).await;
    let list = e.get(&c, "/api/workouts").await;
    // 300 s warm-up plus 3 x (120 + 60) s.
    assert_eq!(list["workouts"][0]["duration_s"], 840);
    assert_eq!(list["workouts"][0]["steps"], 3);
}

#[tokio::test]
async fn bad_workouts_are_refused_with_a_reason() {
    let e = env_at(NOW).await;
    let c = e.login().await;
    let step = |d: Value| {
        json!({ "type": "step", "intensity": "active", "duration": d,
                "target": { "kind": "none" } })
    };
    let cases: Vec<(Value, &str)> = vec![
        (
            json!({ "name": " ", "sport": "running", "nodes": [step(json!({"kind": "open"}))] }),
            "needs a name",
        ),
        (
            json!({ "name": "x", "sport": "running", "nodes": [] }),
            "no steps",
        ),
        (
            json!({ "name": "x", "sport": "running",
                    "nodes": [step(json!({"kind": "time", "seconds": 0}))] }),
            "1 second",
        ),
        (
            json!({ "name": "x", "sport": "running", "nodes": [
                { "type": "repeat", "count": 1, "steps": [
                    { "intensity": "active", "duration": { "kind": "open" },
                      "target": { "kind": "none" } }] }
            ] }),
            "repeat runs",
        ),
        (
            json!({ "name": "a".repeat(64), "sport": "running",
                    "nodes": [step(json!({"kind": "open"}))] }),
            "at most 63 bytes",
        ),
        (
            json!({ "name": "x", "sport": "swimming",
                    "nodes": [step(json!({"kind": "open"}))] }),
            "swimming",
        ),
        (
            json!({ "name": "x", "sport": "running", "colour": "red",
                    "nodes": [step(json!({"kind": "open"}))] }),
            "colour",
        ),
        (
            json!({ "name": "x", "sport": "running", "nodes": [
                { "type": "step", "intensity": "active",
                  "duration": { "kind": "heart_rate_above", "bpm": 20 },
                  "target": { "kind": "none" } }] }),
            "heart rate",
        ),
    ];
    for (body, reason) in cases {
        let (s, j) = e
            .send(&c, Method::POST, "/api/workouts", body.clone())
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{body}: {j}");
        let msg = j["error"].as_str().unwrap_or_default();
        assert!(msg.contains(reason), "{reason:?} not in {msg:?}");
    }
    assert_eq!(e.get(&c, "/api/workouts").await["workouts"], json!([]));
    // An update is checked the same way and leaves the stored workout alone.
    let id = create(&e, &c, timed_session()).await["id"]
        .as_i64()
        .unwrap();
    let (s, _) = e
        .send(
            &c,
            Method::PUT,
            &format!("/api/workouts/{id}"),
            json!({ "name": "x", "sport": "running", "nodes": [] }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(
        e.get(&c, &format!("/api/workouts/{id}")).await["revision"],
        1
    );
}

#[tokio::test]
async fn the_workout_limit_is_enforced() {
    let e = env_at(NOW).await;
    let c = e.login().await;
    for i in 0..200 {
        e.exec(&format!(
            "INSERT INTO workouts (user_id, name, sport, definition, created_at, updated_at)
             VALUES ({}, 'w{i}', 'running', '{{}}', 1, 1)",
            e.user_id
        ))
        .await;
    }
    let (s, j) = e
        .send(&c, Method::POST, "/api/workouts", timed_session())
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{j}");
}

#[tokio::test]
async fn writes_need_the_csrf_header_and_a_user_session() {
    let e = env_at(NOW).await;
    let c = e.login().await;
    let id = create(&e, &c, timed_session()).await["id"]
        .as_i64()
        .unwrap();
    let admin_id = e.add_user("boss", Role::Admin).await;
    assert!(admin_id > 0);
    let admin = e.login_as("boss").await;
    let routes = [
        (
            Method::POST,
            "/api/workouts".to_owned(),
            Some(timed_session()),
        ),
        (
            Method::PUT,
            format!("/api/workouts/{id}"),
            Some(timed_session()),
        ),
        (Method::DELETE, format!("/api/workouts/{id}"), None),
        (Method::POST, format!("/api/workouts/{id}/send"), None),
        (Method::DELETE, format!("/api/workouts/{id}/send"), None),
    ];
    for (m, uri, body) in routes {
        let (s, _, _) = e.call(m.clone(), &uri, Some(&c), false, body.clone()).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{m} {uri} without X-Nimrag");
        let (s, _, _) = e.call(m.clone(), &uri, None, true, body.clone()).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{m} {uri} without a session");
        let (s, _, _) = e.call(m.clone(), &uri, Some(&admin), true, body).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{m} {uri} as admin");
    }
    for uri in ["/api/workouts".to_owned(), format!("/api/workouts/{id}")] {
        let (s, _, _) = e.call(Method::GET, &uri, None, false, None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{uri}");
        let (s, _, _) = e.call(Method::GET, &uri, Some(&admin), false, None).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{uri} as admin");
    }
    assert_eq!(state_of(&e, id).await, "idle");
}

#[tokio::test]
async fn a_user_sees_and_changes_only_their_own_workouts() {
    let e = env_at(NOW).await;
    let mine = e.login().await;
    e.add_user("bea", Role::User).await;
    let bea = e.login_as("bea").await;
    let id = create(&e, &mine, timed_session()).await["id"]
        .as_i64()
        .unwrap();

    assert_eq!(e.get(&bea, "/api/workouts").await["workouts"], json!([]));
    for (m, uri, body) in [
        (Method::GET, format!("/api/workouts/{id}"), None),
        (
            Method::PUT,
            format!("/api/workouts/{id}"),
            Some(timed_session()),
        ),
        (Method::DELETE, format!("/api/workouts/{id}"), None),
        (Method::POST, format!("/api/workouts/{id}/send"), None),
        (Method::DELETE, format!("/api/workouts/{id}/send"), None),
    ] {
        let (s, _, _) = e.call(m.clone(), &uri, Some(&bea), true, body).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{m} {uri}");
    }
    assert_eq!(e.get(&mine, "/api/workouts").await["workouts"][0]["id"], id);
    assert_eq!(state_of(&e, id).await, "idle");
}

// ---- send to watch ----------------------------------------------------------------------------------

#[tokio::test]
async fn send_to_watch_queues_the_workout_or_says_the_profile_is_missing() {
    let e = env_at(NOW).await;
    let c = e.login().await;
    let id = create(&e, &c, interval_session()).await["id"]
        .as_i64()
        .unwrap();
    let uri = format!("/api/workouts/{id}/send");

    let (s, j) = e.send(&c, Method::POST, &uri, json!(null)).await;
    if profile::available() {
        assert_eq!(s, StatusCode::OK, "{j}");
        assert_eq!(j["send"]["state"], "queued");
        assert!(j["send"]["requested_at"].is_string());
        // Pressing it twice changes nothing.
        let (s, again) = e.send(&c, Method::POST, &uri, json!(null)).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(again["send"]["requested_at"], j["send"]["requested_at"]);
    } else {
        assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{j}");
        let msg = j["error"].as_str().unwrap();
        assert!(msg.contains("FIT profile"), "{msg}");
        assert!(msg.contains("workouts can still be saved"), "{msg}");
        assert_eq!(state_of(&e, id).await, "idle", "nothing was queued");
        let list = e.get(&c, "/api/workouts").await;
        assert_eq!(list["watch_send"]["available"], false);
        assert!(list["watch_send"]["reason"]
            .as_str()
            .unwrap()
            .contains("FIT profile"));
    }
}

#[tokio::test]
async fn a_queued_workout_can_be_taken_out_of_the_queue() {
    let e = env_at(NOW).await;
    let c = e.login().await;
    let id = create(&e, &c, timed_session()).await["id"]
        .as_i64()
        .unwrap();
    e.exec(&format!(
        "UPDATE workouts SET send_state = 'queued', send_requested_at = {NOW} WHERE id = {id}"
    ))
    .await;
    let (s, j) = e
        .send(
            &c,
            Method::DELETE,
            &format!("/api/workouts/{id}/send"),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["send"]["state"], "idle");
    assert!(j["send"]["requested_at"].is_null());
}

#[tokio::test]
async fn the_phone_fetches_the_queue_and_the_fit_file_and_reports_back() {
    let e = env_at(NOW).await;
    let c = e.login().await;
    let id = create(&e, &c, interval_session()).await["id"]
        .as_i64()
        .unwrap();
    let token = e.token_for(e.user_id).await;

    // Nothing is queued yet.
    let (s, _, b) = device(&e, Method::GET, "/api/device/workouts", Some(&token), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(json_of(&b), json!({ "workouts": [] }));
    let fit_uri = format!("/api/device/workouts/{id}/fit");
    let (s, _, _) = device(&e, Method::GET, &fit_uri, Some(&token), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "an idle workout is not fetched");

    // Queue it (directly, so this part runs in builds without the profile too).
    e.exec(&format!(
        "UPDATE workouts SET send_state = 'queued', send_requested_at = {NOW} WHERE id = {id}"
    ))
    .await;
    let (s, _, b) = device(&e, Method::GET, "/api/device/workouts", Some(&token), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        json_of(&b),
        json!({ "workouts": [{ "id": id, "name": "4 x 400 m", "revision": 1 }] })
    );

    let (s, h, bytes) = device(&e, Method::GET, &fit_uri, Some(&token), None).await;
    if profile::available() {
        assert_eq!(s, StatusCode::OK);
        assert_eq!(h[header::CONTENT_TYPE], "application/octet-stream");
        assert_eq!(h["x-nimrag-revision"], "1");
        assert_eq!(&bytes[8..12], b".FIT");
        let decoded = nimrag_core::fit::decode(&bytes);
        assert!(decoded.error.is_none(), "{:?}", decoded.error);
        assert_eq!(decoded.files.len(), 1);
    } else {
        assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
        assert!(json_of(&bytes)["error"]
            .as_str()
            .unwrap()
            .contains("FIT profile"));
    }

    // The report: applied once, and a second one finds nothing queued.
    let result = format!("/api/device/workouts/{id}/result");
    let (s, _, b) = device(
        &e,
        Method::POST,
        &result,
        Some(&token),
        Some(json!({ "revision": 1, "ok": true })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(json_of(&b), json!({ "applied": true }));
    assert_eq!(state_of(&e, id).await, "sent");
    let again = device(
        &e,
        Method::POST,
        &result,
        Some(&token),
        Some(json!({ "revision": 1, "ok": false, "error": "late" })),
    )
    .await;
    assert_eq!(json_of(&again.2), json!({ "applied": false }));
    let got = e.get(&c, &format!("/api/workouts/{id}")).await;
    assert_eq!(got["send"]["state"], "sent");
    assert!(got["send"]["finished_at"].is_string());
    assert!(got["send"]["error"].is_null());
}

#[tokio::test]
async fn a_failure_report_shows_its_reason_to_the_owner() {
    let e = env_at(NOW).await;
    let c = e.login().await;
    let id = create(&e, &c, timed_session()).await["id"]
        .as_i64()
        .unwrap();
    let token = e.token_for(e.user_id).await;
    e.exec(&format!(
        "UPDATE workouts SET send_state = 'queued', send_requested_at = {NOW} WHERE id = {id}"
    ))
    .await;
    let long = format!("the watch refused (create status 2)\n{}", "x".repeat(500));
    let (s, _, b) = device(
        &e,
        Method::POST,
        &format!("/api/device/workouts/{id}/result"),
        Some(&token),
        Some(json!({ "revision": 1, "ok": false, "error": long })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(json_of(&b), json!({ "applied": true }));
    let got = e.get(&c, &format!("/api/workouts/{id}")).await;
    assert_eq!(got["send"]["state"], "failed");
    let err = got["send"]["error"].as_str().unwrap();
    assert!(err.starts_with("the watch refused (create status 2)"));
    assert!(!err.contains('\n'));
    assert_eq!(err.chars().count(), 200);
}

#[tokio::test]
async fn editing_changes_what_the_phone_must_report() {
    let e = env_at(NOW).await;
    let c = e.login().await;
    let id = create(&e, &c, timed_session()).await["id"]
        .as_i64()
        .unwrap();
    let token = e.token_for(e.user_id).await;
    e.exec(&format!(
        "UPDATE workouts SET send_state = 'queued', send_requested_at = {NOW} WHERE id = {id}"
    ))
    .await;

    // Edited while queued: still queued, now revision 2.
    let mut edited = timed_session();
    edited["name"] = json!("Tempo v2");
    let (s, j) = e
        .send(&c, Method::PUT, &format!("/api/workouts/{id}"), edited)
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        (j["revision"].as_i64(), j["send"]["state"].as_str()),
        (Some(2), Some("queued"))
    );

    // A report for the old revision is not applied.
    let result = format!("/api/device/workouts/{id}/result");
    let (_, _, b) = device(
        &e,
        Method::POST,
        &result,
        Some(&token),
        Some(json!({ "revision": 1, "ok": true })),
    )
    .await;
    assert_eq!(json_of(&b), json!({ "applied": false }));
    assert_eq!(state_of(&e, id).await, "queued");
    let (_, _, b) = device(&e, Method::GET, "/api/device/workouts", Some(&token), None).await;
    assert_eq!(json_of(&b)["workouts"][0]["revision"], 2);

    let (_, _, b) = device(
        &e,
        Method::POST,
        &result,
        Some(&token),
        Some(json!({ "revision": 2, "ok": true })),
    )
    .await;
    assert_eq!(json_of(&b), json!({ "applied": true }));

    // Edited after it was sent: back to idle, because the watch has the old one.
    let (_, j) = e
        .send(
            &c,
            Method::PUT,
            &format!("/api/workouts/{id}"),
            timed_session(),
        )
        .await;
    assert_eq!(j["send"]["state"], "idle");
    assert!(j["send"]["finished_at"].is_null());
}

#[tokio::test]
async fn the_phones_endpoints_check_the_token_and_the_owner() {
    let e = env_at(NOW).await;
    let c = e.login().await;
    let id = create(&e, &c, timed_session()).await["id"]
        .as_i64()
        .unwrap();
    e.exec(&format!(
        "UPDATE workouts SET send_state = 'queued', send_requested_at = {NOW} WHERE id = {id}"
    ))
    .await;
    let bea = e.add_user("bea", Role::User).await;
    let bea_token = e.token_for(bea).await;
    let unassigned = nimrag_server::tokens::create(&e.pool, "usb", None)
        .await
        .unwrap()
        .1;

    let fit = format!("/api/device/workouts/{id}/fit");
    let result = format!("/api/device/workouts/{id}/result");
    let report = json!({ "revision": 1, "ok": true });
    let calls = [
        (Method::GET, "/api/device/workouts".to_owned(), None),
        (Method::GET, fit.clone(), None),
        (Method::POST, result.clone(), Some(report.clone())),
    ];
    for (m, uri, body) in calls {
        let (s, _, _) = device(&e, m.clone(), &uri, None, body.clone()).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{m} {uri} without a token");
        let (s, _, _) = device(&e, m.clone(), &uri, Some("not-a-token"), body.clone()).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{m} {uri} with a bad token");
        let (s, _, _) = device(&e, m.clone(), &uri, Some(&unassigned), body.clone()).await;
        assert_eq!(
            s,
            StatusCode::FORBIDDEN,
            "{m} {uri} with an unassigned token"
        );
        // A cookie session is not a device token.
        let (s, _, _) = e.call(m.clone(), &uri, Some(&c), true, body.clone()).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{m} {uri} with a session");
    }
    // Another user's token sees an empty queue and cannot touch this workout.
    let (_, _, b) = device(
        &e,
        Method::GET,
        "/api/device/workouts",
        Some(&bea_token),
        None,
    )
    .await;
    assert_eq!(json_of(&b), json!({ "workouts": [] }));
    let (s, _, _) = device(&e, Method::GET, &fit, Some(&bea_token), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = device(&e, Method::POST, &result, Some(&bea_token), Some(report)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(state_of(&e, id).await, "queued");
    // The phone cannot create or read workouts through the owner's API.
    let token = e.token_for(e.user_id).await;
    let (s, _, _) = device(&e, Method::GET, "/api/workouts", Some(&token), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

// ---- admin and export ---------------------------------------------------------------------------------

#[tokio::test]
async fn deleting_a_user_removes_their_workouts() {
    let e = env_at(NOW).await;
    e.adopt.store(false, std::sync::atomic::Ordering::Relaxed);
    let bea = e.add_user("bea", Role::User).await;
    e.add_user("boss", Role::Admin).await;
    let bea_cookie = e.login_as("bea").await;
    let kept = create(&e, &e.login().await, timed_session()).await["id"]
        .as_i64()
        .unwrap();
    let id = create(&e, &bea_cookie, timed_session()).await["id"]
        .as_i64()
        .unwrap();
    let admin = e.login_as("boss").await;

    // The user has data now, so the admin must confirm.
    let (s, _, _) = e
        .call(
            Method::DELETE,
            &format!("/api/admin/users/{bea}"),
            Some(&admin),
            true,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _, _) = e
        .call(
            Method::DELETE,
            &format!("/api/admin/users/{bea}?confirm=true"),
            Some(&admin),
            true,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let left: Vec<i64> = sqlx::query_scalar("SELECT id FROM workouts ORDER BY id")
        .fetch_all(&e.pool)
        .await
        .unwrap();
    assert_eq!(left, vec![kept]);
    assert_ne!(kept, id);
}

#[tokio::test]
async fn the_export_has_the_users_workouts() {
    let e = env_at(NOW).await;
    let c = e.login().await;
    create(&e, &c, timed_session()).await;
    e.add_user("bea", Role::User).await;
    let bea = e.login_as("bea").await;
    create(&e, &bea, interval_session()).await;

    let (s, _, bytes) = {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/export")
            .header(header::COOKIE, &c)
            .body(Body::empty())
            .unwrap();
        let resp = e.app.clone().oneshot(req).await.unwrap();
        let s = resp.status();
        let h = resp.headers().clone();
        (
            s,
            h,
            resp.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
    };
    assert_eq!(s, StatusCode::OK);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut csv = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("csv/workouts.csv").unwrap(), &mut csv).unwrap();
    assert!(csv.contains("Tempo"), "{csv}");
    assert!(!csv.contains("4 x 400 m"), "another user's workout: {csv}");
}
