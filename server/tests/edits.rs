//! Offline edits (docs/design/offline-edits.md), server side: `POST /api/device/edits` runs the
//! allowed changes as the token's user, once per op id; `GET /api/device/state` gives the
//! phone's copy its data. The last test runs a phone's copy (`embedded`) against a server.

mod common;

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use common::*;
use http_body_util::BodyExt;
use nimrag_server::derived::{rank_key, src_id};
use nimrag_server::embedded::{self, EditVerdict};
use nimrag_server::tokens;
use nimrag_server::users::Role;
use serde_json::{json, Value};
use tower::ServiceExt;

const NOW: i64 = 1_800_000_000;
const SECRET: &str = "0123456789abcdef0123456789abcdef";

fn session() -> Value {
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

/// What a phone sends: one change, the body as JSON text.
async fn edit(
    e: &Env,
    token: Option<&str>,
    op_id: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let payload = json!({
        "op_id": format!("test-{op_id}"),
        "method": method,
        "path": path,
        "body": body.map(|b| b.to_string()).unwrap_or_default(),
        "made_at": NOW - 600,
    });
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/device/edits")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let resp = e
        .app
        .clone()
        .oneshot(req.body(Body::from(payload.to_string())).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn device_state(e: &Env, token: &str) -> (StatusCode, Value) {
    e.request(Method::GET, "/api/device/state", Some(token), vec![])
        .await
}

fn sha(c: char) -> String {
    c.to_string().repeat(64)
}

/// A strength session of the data user: sets 0 and 1 are working sets.
async fn insert_strength(e: &Env, sha: &str, start: i64) {
    e.exec(&format!(
        "INSERT INTO files (sha256, size, received_at, source) VALUES ('{sha}', 1, '2026-11-19T00:00:00.000Z', 'watch')"
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO sources (sha256, src_id, ok, last_ts, msg_count, rank_key) VALUES ('{sha}', {}, 1, {start}, 1, '{}')",
        src_id(sha),
        rank_key(true, Some(start), 1)
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO file_owners (sha256, user_id) VALUES ('{sha}', {})",
        e.user_id
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO activities (sha256, activity_key, ts, start_ts, local_offset_s, total_timer_s, num_sessions, sport, sport_name, sub_sport, sub_sport_name)
         VALUES ('{sha}', 'k:1', {}, {start}, 3600, 2000, 1, 10, 'training', 20, 'strength_training')",
        start + 2000
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO activity_sessions (sha256, idx, start_ts, sport, sub_sport, elapsed_s, timer_s, calories, avg_hr, max_hr)
         VALUES ('{sha}', 0, {start}, 10, 20, 2000, 2000, 250, 110, 140)"
    ))
    .await;
    for idx in 0..2 {
        e.exec(&format!(
            "INSERT INTO strength_sets (sha256, idx, ts, start_ts, duration_s, set_type, reps, weight_kg, category, category_name, category_subtype)
             VALUES ('{sha}', {idx}, {start}, {start}, 40, 1, 10, 22.0, 0, 'bench_press', 0)"
        ))
        .await;
    }
}

#[tokio::test]
async fn every_allowed_change_runs_as_the_tokens_user() {
    let e = env_at(NOW).await;
    let c = e.login().await;
    let t = e.token.clone();

    // Settings.
    let (s, j) = edit(
        &e,
        Some(&t),
        "op-settings",
        "PATCH",
        "/api/settings",
        Some(json!({ "step_goal": 9000 })),
    )
    .await;
    assert_eq!(
        (s, j["step_goal"].as_u64()),
        (StatusCode::OK, Some(9000)),
        "{j}"
    );
    assert_eq!(e.get(&c, "/api/settings").await["step_goal"], 9000);

    // Body entries: create, then delete by the id the answer gave.
    let (s, j) = edit(
        &e,
        Some(&t),
        "op-body-1",
        "POST",
        "/api/body",
        Some(json!({ "weight_kg": 80.5 })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{j}");
    let entry = j["id"].as_i64().unwrap();
    assert_eq!(
        e.get(&c, "/api/body").await["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let (s, _) = edit(
        &e,
        Some(&t),
        "op-body-2",
        "DELETE",
        &format!("/api/body/{entry}"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(e.get(&c, "/api/body").await["entries"]
        .as_array()
        .unwrap()
        .is_empty());

    // Strength, with the activity named by the phone's key (hash and start time).
    let activity = sha('a');
    insert_strength(&e, &activity, NOW - 86_400).await;
    let key = format!("{activity}@{}", NOW - 86_400);
    let (s, j) = edit(
        &e,
        Some(&t),
        "op-strength",
        "PATCH",
        &format!("/api/activities/{key}/strength"),
        Some(json!({ "sets": [{ "idx": 0, "reps": 12 }] })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["id"], activity.as_str());
    assert_eq!(j["exercises"][0]["sets"][0]["reps"], 12.0);

    // Workouts: create, update, send, take back, delete.
    let (s, j) = edit(
        &e,
        Some(&t),
        "op-w-1",
        "POST",
        "/api/workouts",
        Some(session()),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{j}");
    let w = j["id"].as_i64().unwrap();
    let mut changed = session();
    changed["name"] = json!("Tempo 2");
    let (s, j) = edit(
        &e,
        Some(&t),
        "op-w-2",
        "PUT",
        &format!("/api/workouts/{w}"),
        Some(changed),
    )
    .await;
    assert_eq!(
        (s, j["revision"].as_i64()),
        (StatusCode::OK, Some(2)),
        "{j}"
    );
    // Sending needs the FIT profile on a normal server; where it is missing the server says so.
    let (s, j) = edit(
        &e,
        Some(&t),
        "op-w-3",
        "POST",
        &format!("/api/workouts/{w}/send"),
        None,
    )
    .await;
    if nimrag_core::fit::workout::available() {
        assert_eq!(
            (s, j["send"]["state"].as_str()),
            (StatusCode::OK, Some("queued")),
            "{j}"
        );
        let (s, j) = edit(
            &e,
            Some(&t),
            "op-w-4",
            "DELETE",
            &format!("/api/workouts/{w}/send"),
            None,
        )
        .await;
        assert_eq!(
            (s, j["send"]["state"].as_str()),
            (StatusCode::OK, Some("idle")),
            "{j}"
        );
    } else {
        assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{j}");
    }
    let (s, _) = edit(
        &e,
        Some(&t),
        "op-w-5",
        "DELETE",
        &format!("/api/workouts/{w}"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(e.get(&c, "/api/workouts").await["workouts"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn a_replayed_op_changes_nothing_and_gets_the_first_answer() {
    let e = env_at(NOW).await;
    let t = e.token.clone();
    let (s1, j1) = edit(
        &e,
        Some(&t),
        "op-create-1",
        "POST",
        "/api/workouts",
        Some(session()),
    )
    .await;
    let (s2, j2) = edit(
        &e,
        Some(&t),
        "op-create-1",
        "POST",
        "/api/workouts",
        Some(session()),
    )
    .await;
    assert_eq!((s1, s2), (StatusCode::CREATED, StatusCode::CREATED));
    assert_eq!(j1, j2, "the same answer, not a second workout");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workouts")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, 1);

    // A refusal is remembered too: the replay gets it even if the world changed meanwhile.
    let (s, _) = edit(&e, Some(&t), "op-del", "DELETE", "/api/workouts/9999", None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    e.exec(&format!(
        "INSERT INTO workouts (id, user_id, name, sport, definition, created_at, updated_at)
         VALUES (9999, {}, 'x', 'running', '{{}}', 0, 0)",
        e.user_id
    ))
    .await;
    let (s, _) = edit(&e, Some(&t), "op-del", "DELETE", "/api/workouts/9999", None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // The same change under a new op id is a new change.
    let (s, _) = edit(
        &e,
        Some(&t),
        "op-del-2",
        "DELETE",
        "/api/workouts/9999",
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn only_the_listed_changes_are_taken() {
    let e = env_at(NOW).await;
    let t = e.token.clone();
    let bad = [
        ("POST", "/api/login"),
        ("POST", "/api/account/password"),
        ("DELETE", "/api/tokens/1"),
        ("PATCH", "/api/garmin"),
        ("POST", "/api/import/garmin"),
        ("GET", "/api/settings"),
        ("PUT", "/api/settings"),
        ("POST", "/api/workouts/1"),
        ("PATCH", "/api/activities/not-a-hash/strength"),
        ("DELETE", "/api/body/1/extra"),
        ("DELETE", "/api/body/-1"),
        ("POST", "/api/device/edits"),
        ("POST", "/api/device/workouts/1/result"),
    ];
    for (i, (method, path)) in bad.into_iter().enumerate() {
        let (s, j) = edit(
            &e,
            Some(&t),
            &format!("op-bad-{i}"),
            method,
            path,
            Some(json!({})),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{method} {path}: {j}");
    }
    // A bad op id.
    let (s, _) = edit(&e, Some(&t), "x", "PATCH", "/api/settings", Some(json!({}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_handlers_own_answers_come_back_unchanged() {
    let e = env_at(NOW).await;
    let t = e.token.clone();
    // Validation: the same 400 and message as on the web API.
    let (s, j) = edit(
        &e,
        Some(&t),
        "op-v1",
        "PATCH",
        "/api/settings",
        Some(json!({ "step_goal": 0 })),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(j["error"], "step_goal must be 1 to 200000");
    // A row that is gone: 404.
    let (s, _) = edit(
        &e,
        Some(&t),
        "op-v2",
        "PUT",
        "/api/workouts/42",
        Some(session()),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // An activity the user does not have.
    let (s, _) = edit(
        &e,
        Some(&t),
        "op-v3",
        "PATCH",
        &format!("/api/activities/{}/strength", sha('e')),
        Some(json!({ "sets": [{ "idx": 0, "reps": 1 }] })),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Not JSON.
    let (s, _) = edit(&e, Some(&t), "op-v4", "PATCH", "/api/settings", None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_body_entry_without_a_time_is_dated_when_it_was_made() {
    let e = env_at(NOW).await;
    let c = e.login().await;
    let (s, j) = edit(
        &e,
        Some(&e.token.clone()),
        "op-time",
        "POST",
        "/api/body",
        Some(json!({ "weight_kg": 79.0 })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{j}");
    let at = chrono::DateTime::parse_from_rfc3339(j["measured_at"].as_str().unwrap())
        .unwrap()
        .timestamp();
    assert_eq!(
        at,
        NOW - 600,
        "made_at, not the time the server heard of it"
    );
    assert_eq!(
        e.get(&c, "/api/body").await["entries"][0]["weight_kg"],
        79.0
    );
}

#[tokio::test]
async fn an_activity_is_found_by_its_start_when_the_hash_is_not_known() {
    let e = env_at(NOW).await;
    let t = e.token.clone();
    let start = NOW - 86_400;
    insert_strength(&e, &sha('a'), start).await;
    let patch = Some(json!({ "sets": [{ "idx": 1, "reps": 7 }] }));
    // The phone's file hash is another one, the start time is the same.
    let key = format!("/api/activities/{}@{start}/strength", sha('b'));
    let (s, j) = edit(&e, Some(&t), "op-key-1", "PATCH", &key, patch.clone()).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["id"], sha('a').as_str());
    // Another start time finds nothing; the hash alone, unknown, finds nothing.
    let key = format!("/api/activities/{}@{}/strength", sha('b'), start + 5);
    let (s, _) = edit(&e, Some(&t), "op-key-2", "PATCH", &key, patch.clone()).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let key = format!("/api/activities/{}/strength", sha('b'));
    let (s, _) = edit(&e, Some(&t), "op-key-3", "PATCH", &key, patch).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tokens_decide_who_may_send_and_for_whom() {
    let e = env_at(NOW).await;
    let body = Some(json!({ "step_goal": 7000 }));
    // No token, a wrong one, a token without a user.
    let (s, _) = edit(
        &e,
        None,
        "op-auth-1",
        "PATCH",
        "/api/settings",
        body.clone(),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = edit(
        &e,
        Some("nrg_nope"),
        "op-auth-2",
        "PATCH",
        "/api/settings",
        body.clone(),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (_, loose) = tokens::create(&e.pool, "loose", None).await.unwrap();
    let (s, _) = edit(
        &e,
        Some(&loose),
        "op-auth-3",
        "PATCH",
        "/api/settings",
        body.clone(),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = device_state(&e, &loose).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = e
        .request(Method::GET, "/api/device/state", None, vec![])
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // A session cookie is no way in: this is the phone's door.
    let c = e.login().await;
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/device/edits")
        .header(header::COOKIE, c)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "op_id": "op-auth-4", "method": "PATCH", "path": "/api/settings",
                    "body": "{}", "made_at": 0 })
            .to_string(),
        ))
        .unwrap();
    assert_eq!(
        e.app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn users_only_reach_their_own_rows_and_state() {
    let e = env_at(NOW).await;
    let c = e.login().await;
    let bea = e.add_user("bea", Role::User).await;
    let bea_token = e.token_for(bea).await;
    let bea_cookie = e.login_as("bea").await;

    let (_, j) = edit(
        &e,
        Some(&e.token.clone()),
        "op-iso-1",
        "POST",
        "/api/workouts",
        Some(session()),
    )
    .await;
    let mine = j["id"].as_i64().unwrap();
    edit(
        &e,
        Some(&e.token.clone()),
        "op-iso-2",
        "PATCH",
        "/api/settings",
        Some(json!({ "step_goal": 4321 })),
    )
    .await;
    edit(
        &e,
        Some(&e.token.clone()),
        "op-iso-3",
        "POST",
        "/api/body",
        Some(json!({ "weight_kg": 70.0 })),
    )
    .await;

    // Bea cannot change or delete it, even by id.
    let (s, _) = edit(
        &e,
        Some(&bea_token),
        "op-iso-4",
        "DELETE",
        &format!("/api/workouts/{mine}"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = edit(
        &e,
        Some(&bea_token),
        "op-iso-5",
        "PUT",
        &format!("/api/workouts/{mine}"),
        Some(session()),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(
        e.get(&c, "/api/workouts").await["workouts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // The same op id from another user is another op.
    let (s, _) = edit(
        &e,
        Some(&bea_token),
        "op-iso-1",
        "POST",
        "/api/workouts",
        Some(session()),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(
        e.get(&bea_cookie, "/api/workouts").await["workouts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // The state is the token user's own.
    let (s, mine_state) = device_state(&e, &e.token).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(mine_state["version"], 1);
    assert_eq!(mine_state["settings"]["step_goal"], "4321");
    assert_eq!(mine_state["workouts"].as_array().unwrap().len(), 1);
    assert_eq!(mine_state["workouts"][0]["id"], mine);
    assert_eq!(mine_state["body"].as_array().unwrap().len(), 1);
    let (_, theirs) = device_state(&e, &bea_token).await;
    assert!(theirs["settings"].as_object().unwrap().is_empty());
    assert_eq!(theirs["workouts"].as_array().unwrap().len(), 1);
    assert_ne!(theirs["workouts"][0]["id"], mine);
    assert!(theirs["body"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn the_state_carries_settings_workouts_body_and_strength_edits() {
    let e = env_at(NOW).await;
    let t = e.token.clone();
    insert_strength(&e, &sha('a'), NOW - 86_400).await;
    edit(
        &e,
        Some(&t),
        "op-st-1",
        "PATCH",
        "/api/settings",
        Some(json!({ "timezone": "Europe/Copenhagen", "intensity_goal_min": 200 })),
    )
    .await;
    edit(
        &e,
        Some(&t),
        "op-st-2",
        "POST",
        "/api/workouts",
        Some(session()),
    )
    .await;
    edit(
        &e,
        Some(&t),
        "op-st-3",
        "POST",
        "/api/body",
        Some(json!({ "weight_kg": 81.0, "waist_cm": 90.0 })),
    )
    .await;
    edit(
        &e,
        Some(&t),
        "op-st-4",
        "PATCH",
        &format!("/api/activities/{}/strength", sha('a')),
        Some(json!({ "sets": [{ "idx": 0, "reps": 5, "exercise": "Row" }] })),
    )
    .await;
    // Something that is not a phone's business stays out.
    e.exec(&format!(
        "INSERT INTO user_settings (user_id, key, value) VALUES ({}, 'private_thing', 'x')",
        e.user_id
    ))
    .await;

    let (s, j) = device_state(&e, &t).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        j["settings"],
        json!({ "timezone": "Europe/Copenhagen", "intensity_goal_min": "200" })
    );
    let w = &j["workouts"][0];
    assert_eq!(
        (
            w["name"].as_str(),
            w["send_state"].as_str(),
            w["revision"].as_i64()
        ),
        (Some("Tempo"), Some("idle"), Some(1))
    );
    assert_eq!(
        serde_json::from_str::<Value>(w["definition"].as_str().unwrap()).unwrap(),
        session()
    );
    assert_eq!(
        (
            j["body"][0]["weight_kg"].as_f64(),
            j["body"][0]["waist_cm"].as_f64()
        ),
        (Some(81.0), Some(90.0))
    );
    // Keyed the way the server keys it: the activity's key, not the hash.
    assert_eq!(j["strength"][0]["activity_ident"], "k:1");
    assert_eq!(j["strength"][0]["set_idx"], 0);
    assert!(j["strength"][0]["fields"]
        .as_str()
        .unwrap()
        .contains("\"reps\":5"));
}

// ---- a phone's copy against this server -------------------------------------------------------------

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// A request to the phone's copy as its web view makes it.
fn local(
    rt: &tokio::runtime::Runtime,
    port: u16,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    rt.block_on(async {
        let mut r = reqwest::Client::new()
            .request(method, format!("http://127.0.0.1:{port}{path}"))
            .header("cookie", format!("{}={SECRET}", embedded::SECRET_COOKIE))
            .header("x-nimrag", "1");
        if let Some(b) = body {
            r = r
                .header("content-type", "application/json")
                .body(b.to_string());
        }
        let resp = r.send().await.unwrap();
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    })
}

/// Send everything the phone's copy has waiting to the server, as the app's worker does.
fn drain(
    rt: &tokio::runtime::Runtime,
    phone: &embedded::Embedded,
    server: &Env,
) -> Vec<EditVerdict> {
    let mut verdicts = Vec::new();
    while let Some(next) = phone.next_edit().unwrap() {
        let (status, answer) = rt.block_on(async {
            let payload = json!({
                "op_id": next.op_id, "method": next.method, "path": next.path,
                "body": next.body, "made_at": next.made_at,
            });
            let req = Request::builder()
                .method(Method::POST)
                .uri("/api/device/edits")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {}", server.token))
                .body(Body::from(payload.to_string()))
                .unwrap();
            let resp = server.app.clone().oneshot(req).await.unwrap();
            let status = resp.status().as_u16();
            let bytes = resp.into_body().collect().await.unwrap().to_bytes();
            (status, String::from_utf8(bytes.to_vec()).unwrap())
        });
        let verdict = phone.edit_done(&next.op_id, status, &answer).unwrap();
        assert_ne!(verdict, EditVerdict::Kept, "{status} {answer}");
        verdicts.push(verdict);
    }
    verdicts
}

#[test]
fn a_phone_works_offline_then_sends_it_all_and_pulls_the_servers_state() {
    let rt = rt();
    let server = rt.block_on(env_at(NOW));
    let web = rt.block_on(server.login());
    let dir = tempfile::tempdir().unwrap();
    let phone = embedded::start(dir.path(), 0, SECRET).unwrap();
    let p = phone.port();

    // Offline: a workout is made and changed, a goal is changed, a weight is logged.
    let (s, made) = local(
        &rt,
        p,
        reqwest::Method::POST,
        "/api/workouts",
        Some(session()),
    );
    assert_eq!(s, 201, "{made}");
    let local_id = made["id"].as_i64().unwrap();
    let mut changed = session();
    changed["name"] = json!("Tempo, longer");
    let (s, _) = local(
        &rt,
        p,
        reqwest::Method::PUT,
        &format!("/api/workouts/{local_id}"),
        Some(changed),
    );
    assert_eq!(s, 200);
    let (s, _) = local(
        &rt,
        p,
        reqwest::Method::PATCH,
        "/api/settings",
        Some(json!({ "step_goal": 12_000 })),
    );
    assert_eq!(s, 200);
    let (s, _) = local(
        &rt,
        p,
        reqwest::Method::POST,
        "/api/body",
        Some(json!({ "weight_kg": 77.7 })),
    );
    assert_eq!(s, 201);
    assert_eq!(phone.pending_edits().unwrap(), 4);

    // The server meanwhile has a workout of its own, so its ids differ from the phone's.
    rt.block_on(server.send(&web, Method::POST, "/api/workouts", session()));

    // Back online: everything goes over in order, and the server has it.
    assert_eq!(drain(&rt, &phone, &server), vec![EditVerdict::Applied; 4]);
    assert_eq!(phone.pending_edits().unwrap(), 0);
    let list = rt.block_on(server.get(&web, "/api/workouts"));
    let all = list["workouts"].as_array().unwrap();
    assert_eq!(all.len(), 2);
    let ours = all
        .iter()
        .find(|w| w["name"] == "Tempo, longer")
        .expect("the changed workout");
    assert_ne!(ours["id"], local_id, "the server gave it an id of its own");
    assert_eq!(
        rt.block_on(server.get(&web, "/api/settings"))["step_goal"],
        12_000
    );
    assert_eq!(
        rt.block_on(server.get(&web, "/api/body"))["entries"][0]["weight_kg"],
        77.7
    );

    // Pull: the phone's copy now holds the server's rows and ids.
    let (status, state) = rt.block_on(device_state(&server, &server.token));
    assert_eq!(status, StatusCode::OK);
    assert!(phone.apply_state(&state.to_string()).unwrap());
    let (s, mine) = local(&rt, p, reqwest::Method::GET, "/api/workouts", None);
    assert_eq!(s, 200);
    assert_eq!(mine["workouts"].as_array().unwrap().len(), 2);
    assert!(mine["workouts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["id"] == ours["id"]));
    assert_eq!(
        local(&rt, p, reqwest::Method::GET, "/api/settings", None).1["step_goal"],
        12_000
    );
    phone.stop();
}
