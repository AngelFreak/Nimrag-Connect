//! Offline edits on the phone's copy (docs/design/offline-edits.md): the allowed changes run and
//! land in the outbox, everything else still needs the server, ids are rewritten for the server,
//! and the state the server gives replaces the copy. The round trip against a server is in
//! `edits.rs`.

use nimrag_server::db;
use nimrag_server::embedded::{self, EditVerdict};
use serde_json::{json, Value};

const SECRET: &str = "0123456789abcdef0123456789abcdef";

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn cookie() -> String {
    format!("{}={SECRET}", embedded::SECRET_COOKIE)
}

/// A request to the phone's copy as its web view makes it: the secret cookie and the CSRF header.
fn call(
    rt: &tokio::runtime::Runtime,
    port: u16,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> (u16, String) {
    rt.block_on(async {
        let mut r = reqwest::Client::new()
            .request(method, format!("http://127.0.0.1:{port}{path}"))
            .header("cookie", cookie())
            .header("x-nimrag", "1");
        if let Some(b) = body {
            r = r
                .header("content-type", "application/json")
                .body(b.to_string());
        }
        let resp = r.send().await.unwrap();
        (resp.status().as_u16(), resp.text().await.unwrap())
    })
}

fn read(rt: &tokio::runtime::Runtime, port: u16, path: &str) -> String {
    call(rt, port, reqwest::Method::GET, path, None).1
}

/// Run `f` against the data dir's database while no embedded server has it open.
fn with_db<T>(dir: &std::path::Path, f: impl AsyncFnOnce(&sqlx::SqlitePool) -> T) -> T {
    rt().block_on(async {
        let pool = db::open(dir).await.unwrap();
        let out = f(&pool).await;
        pool.close().await;
        out
    })
}

fn workout() -> Value {
    json!({
        "name": "Easy",
        "sport": "running",
        "nodes": [{ "type": "step", "intensity": "active",
                    "duration": { "kind": "time", "seconds": 600 },
                    "target": { "kind": "none" } }]
    })
}

fn id_of(body: &str) -> i64 {
    serde_json::from_str::<Value>(body).unwrap()["id"]
        .as_i64()
        .unwrap()
}

#[test]
fn an_allowed_change_runs_and_leaves_one_outbox_row() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let rt = rt();
    let p = e.port();
    assert_eq!(e.pending_edits().unwrap(), 0);
    assert!(e.next_edit().unwrap().is_none());

    let (s, body) = call(
        &rt,
        p,
        reqwest::Method::PATCH,
        "/api/settings",
        Some(json!({ "step_goal": 9000 })),
    );
    assert_eq!(s, 200, "{body}");
    // The page sees the change at once.
    assert!(read(&rt, p, "/api/settings").contains("9000"));

    assert_eq!(e.pending_edits().unwrap(), 1);
    let edit = e.next_edit().unwrap().unwrap();
    assert_eq!(
        (edit.method.as_str(), edit.path.as_str()),
        ("PATCH", "/api/settings")
    );
    assert_eq!(edit.body, r#"{"step_goal":9000}"#);
    assert!(edit.made_at > 1_700_000_000);
    assert!(edit.op_id.len() >= 16);
    // Asking again gives the same change: nothing is taken until the answer is reported.
    assert_eq!(e.next_edit().unwrap().unwrap(), edit);

    // A change the handler refuses leaves nothing behind.
    let (s, _) = call(
        &rt,
        p,
        reqwest::Method::PATCH,
        "/api/settings",
        Some(json!({ "step_goal": 0 })),
    );
    assert_eq!(s, 400);
    let (s, _) = call(&rt, p, reqwest::Method::DELETE, "/api/workouts/77", None);
    assert_eq!(s, 404);
    let (s, _) = call(&rt, p, reqwest::Method::PATCH, "/api/settings", None);
    assert_eq!(s, 400);
    assert_eq!(e.pending_edits().unwrap(), 1);

    // Oldest first, each with an op id of its own.
    call(
        &rt,
        p,
        reqwest::Method::POST,
        "/api/body",
        Some(json!({ "weight_kg": 80.0 })),
    );
    assert_eq!(e.pending_edits().unwrap(), 2);
    assert_eq!(
        e.edit_done(&edit.op_id, 200, "{}").unwrap(),
        EditVerdict::Applied
    );
    let second = e.next_edit().unwrap().unwrap();
    assert_eq!(
        (second.method.as_str(), second.path.as_str()),
        ("POST", "/api/body")
    );
    assert_ne!(second.op_id, edit.op_id);
    e.stop();
}

#[test]
fn everything_else_still_needs_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let rt = rt();
    let p = e.port();
    let refused = [
        (reqwest::Method::POST, "/api/login"),
        (reqwest::Method::POST, "/api/setup"),
        (reqwest::Method::POST, "/api/account/password"),
        (reqwest::Method::DELETE, "/api/tokens/1"),
        (reqwest::Method::PATCH, "/api/garmin"),
        (reqwest::Method::POST, "/api/import/garmin"),
        (reqwest::Method::POST, "/api/device/edits"),
        (reqwest::Method::PUT, "/api/settings"),
    ];
    for (method, path) in refused {
        let (s, body) = call(&rt, p, method.clone(), path, Some(json!({})));
        assert_eq!(s, 403, "{method} {path}");
        assert!(body.contains("needs the server"), "{body}");
    }
    assert_eq!(e.pending_edits().unwrap(), 0);

    // The secret and the host are still checked first, for an allowed change too.
    let patch = |host: Option<&str>, secret: bool| {
        rt.block_on(async {
            let mut r = reqwest::Client::new()
                .patch(format!("http://127.0.0.1:{p}/api/settings"))
                .header("x-nimrag", "1")
                .header("content-type", "application/json")
                .body(r#"{"step_goal":5000}"#);
            if let Some(h) = host {
                r = r.header("host", h);
            }
            if secret {
                r = r.header("cookie", cookie());
            }
            let resp = r.send().await.unwrap();
            (resp.status().as_u16(), resp.text().await.unwrap())
        })
    };
    assert_eq!(patch(None, false), (403, "forbidden".to_owned()));
    assert_eq!(
        patch(Some("evil.example"), true),
        (403, "forbidden".to_owned())
    );
    assert_eq!(e.pending_edits().unwrap(), 0);
    e.stop();
}

#[test]
fn a_workout_made_offline_is_renamed_for_the_changes_that_follow() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let rt = rt();
    let p = e.port();
    let (s, body) = call(
        &rt,
        p,
        reqwest::Method::POST,
        "/api/workouts",
        Some(workout()),
    );
    assert_eq!(s, 201, "{body}");
    let local = id_of(&body);
    let mut renamed = workout();
    renamed["name"] = "Easy 2".into();
    call(
        &rt,
        p,
        reqwest::Method::PUT,
        &format!("/api/workouts/{local}"),
        Some(renamed),
    );
    // "Send to watch" is only queued here: the phone has no FIT profile to check it with.
    let (s, sent) = call(
        &rt,
        p,
        reqwest::Method::POST,
        &format!("/api/workouts/{local}/send"),
        None,
    );
    assert_eq!(s, 200, "{sent}");
    assert!(sent.contains("\"queued\""), "{sent}");
    call(
        &rt,
        p,
        reqwest::Method::DELETE,
        &format!("/api/workouts/{local}/send"),
        None,
    );
    call(
        &rt,
        p,
        reqwest::Method::DELETE,
        &format!("/api/workouts/{local}"),
        None,
    );
    assert_eq!(e.pending_edits().unwrap(), 5);

    // The create goes first, with the phone's own path; the server gives it another id.
    let create = e.next_edit().unwrap().unwrap();
    assert_eq!(
        (create.method.as_str(), create.path.as_str()),
        ("POST", "/api/workouts")
    );
    let server_id = local + 1000;
    assert_eq!(
        e.edit_done(&create.op_id, 201, &format!(r#"{{"id": {server_id}}}"#))
            .unwrap(),
        EditVerdict::Applied
    );
    let expected = [
        ("PUT", format!("/api/workouts/{server_id}")),
        ("POST", format!("/api/workouts/{server_id}/send")),
        ("DELETE", format!("/api/workouts/{server_id}/send")),
        ("DELETE", format!("/api/workouts/{server_id}")),
    ];
    for (method, path) in expected {
        let next = e.next_edit().unwrap().unwrap();
        assert_eq!((next.method.as_str(), next.path), (method, path));
        assert_eq!(
            e.edit_done(&next.op_id, 200, "{}").unwrap(),
            EditVerdict::Applied
        );
    }
    assert!(e.next_edit().unwrap().is_none());
    e.stop();
}

#[test]
fn a_body_entry_made_offline_is_renamed_too() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let rt = rt();
    let p = e.port();
    let (_, made) = call(
        &rt,
        p,
        reqwest::Method::POST,
        "/api/body",
        Some(json!({ "weight_kg": 80.0 })),
    );
    let local = id_of(&made);
    call(
        &rt,
        p,
        reqwest::Method::DELETE,
        &format!("/api/body/{local}"),
        None,
    );
    let create = e.next_edit().unwrap().unwrap();
    e.edit_done(&create.op_id, 201, r#"{"id": 31}"#).unwrap();
    assert_eq!(e.next_edit().unwrap().unwrap().path, "/api/body/31");
    e.stop();
}

#[test]
fn a_row_the_server_already_had_keeps_its_id() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let rt = rt();
    let state = json!({
        "version": 1, "settings": {}, "strength": [], "body": [],
        "workouts": [{ "id": 5, "name": "Easy", "sport": "running", "definition": workout().to_string(),
                       "revision": 1, "created_at": 1, "updated_at": 1, "send_state": "idle",
                       "send_requested_at": null, "send_finished_at": null, "send_error": null }],
    });
    assert!(e.apply_state(&state.to_string()).unwrap());
    // A workout made offline gets a larger id than any pulled one, so the two never mix up.
    let (_, made) = call(
        &rt,
        e.port(),
        reqwest::Method::POST,
        "/api/workouts",
        Some(workout()),
    );
    assert!(id_of(&made) > 5);
    call(
        &rt,
        e.port(),
        reqwest::Method::DELETE,
        "/api/workouts/5",
        None,
    );
    let create = e.next_edit().unwrap().unwrap();
    e.edit_done(&create.op_id, 201, r#"{"id": 5000}"#).unwrap();
    assert_eq!(e.next_edit().unwrap().unwrap().path, "/api/workouts/5");
    e.stop();
}

#[test]
fn changes_of_a_row_whose_create_was_refused_are_dropped_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let rt = rt();
    let p = e.port();
    let (_, made) = call(
        &rt,
        p,
        reqwest::Method::POST,
        "/api/workouts",
        Some(workout()),
    );
    let local = id_of(&made);
    call(
        &rt,
        p,
        reqwest::Method::PUT,
        &format!("/api/workouts/{local}"),
        Some(workout()),
    );
    call(
        &rt,
        p,
        reqwest::Method::PATCH,
        "/api/settings",
        Some(json!({ "step_goal": 6000 })),
    );

    let create = e.next_edit().unwrap().unwrap();
    let answer = r#"{"error":"a user keeps at most 200 workouts; delete one first"}"#;
    assert_eq!(
        e.edit_done(&create.op_id, 409, answer).unwrap(),
        EditVerdict::Dropped
    );
    // The change of that workout is skipped; the settings one is next.
    let next = e.next_edit().unwrap().unwrap();
    assert_eq!(next.path, "/api/settings");
    assert_eq!(e.pending_edits().unwrap(), 1);

    let dropped = e.take_dropped_edits().unwrap();
    assert_eq!(dropped.len(), 2);
    assert_eq!(
        (dropped[0].method.as_str(), dropped[0].path.as_str()),
        ("POST", "/api/workouts")
    );
    assert!(
        dropped[0].reason.contains("at most 200"),
        "{}",
        dropped[0].reason
    );
    assert_eq!(dropped[1].path, format!("/api/workouts/{local}"));
    assert!(e.take_dropped_edits().unwrap().is_empty(), "shown once");
    e.stop();
}

#[test]
fn the_status_decides_what_becomes_of_a_change() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let rt = rt();
    let p = e.port();
    let body = json!({ "step_goal": 7000 });
    // Retryable or not decided: it stays, however often.
    for status in [0, 401, 403, 408, 429, 500, 502, 503] {
        call(
            &rt,
            p,
            reqwest::Method::PATCH,
            "/api/settings",
            Some(body.clone()),
        );
        let next = e.next_edit().unwrap().unwrap();
        assert_eq!(
            e.edit_done(&next.op_id, status, "").unwrap(),
            EditVerdict::Kept,
            "{status}"
        );
        assert_eq!(e.next_edit().unwrap().unwrap(), next, "{status}");
        assert_eq!(e.pending_edits().unwrap(), 1);
        assert_eq!(
            e.edit_done(&next.op_id, 200, "").unwrap(),
            EditVerdict::Applied
        );
    }
    // Refused for good: dropped, with the server's words or the status.
    for (status, answer, reason) in [
        (404, r#"{"error":"no such workout"}"#, "no such workout"),
        (
            400,
            r#"{"error":"step_goal must be 1 to 200000"}"#,
            "step_goal must be 1 to 200000",
        ),
        (413, "", "the server answered 413"),
    ] {
        call(
            &rt,
            p,
            reqwest::Method::PATCH,
            "/api/settings",
            Some(body.clone()),
        );
        let next = e.next_edit().unwrap().unwrap();
        assert_eq!(
            e.edit_done(&next.op_id, status, answer).unwrap(),
            EditVerdict::Dropped
        );
        assert_eq!(e.pending_edits().unwrap(), 0);
        assert_eq!(e.take_dropped_edits().unwrap()[0].reason, reason);
    }
    // An answer reported twice is not an error.
    call(&rt, p, reqwest::Method::PATCH, "/api/settings", Some(body));
    let next = e.next_edit().unwrap().unwrap();
    assert_eq!(
        e.edit_done(&next.op_id, 200, "{}").unwrap(),
        EditVerdict::Applied
    );
    assert_eq!(
        e.edit_done(&next.op_id, 200, "{}").unwrap(),
        EditVerdict::Applied
    );
    e.stop();
}

/// An activity with a strength set on the phone, as ingest would leave it.
fn insert_activity(dir: &std::path::Path, uid: i64, sha: &str, start: i64) {
    use nimrag_server::derived::{rank_key, src_id};
    with_db(dir, async |pool| {
        for sql in [
            format!("INSERT INTO files (sha256, size, received_at, source) VALUES ('{sha}', 1, '2026-11-19T00:00:00.000Z', 'watch')"),
            format!("INSERT INTO file_owners (sha256, user_id) VALUES ('{sha}', {uid})"),
            format!(
                "INSERT INTO sources (sha256, src_id, ok, last_ts, msg_count, rank_key) VALUES ('{sha}', {}, 1, {start}, 1, '{}')",
                src_id(sha),
                rank_key(true, Some(start), 1)
            ),
            format!(
                "INSERT INTO activities (sha256, activity_key, ts, start_ts, local_offset_s, total_timer_s, num_sessions, sport, sport_name, sub_sport, sub_sport_name)
                 VALUES ('{sha}', 'k:1', {}, {start}, 3600, 2000, 1, 10, 'training', 20, 'strength_training')",
                start + 2000
            ),
            format!("INSERT INTO activity_sessions (sha256, idx, start_ts, sport, sub_sport, elapsed_s, timer_s) VALUES ('{sha}', 0, {start}, 10, 20, 2000, 2000)"),
            format!(
                "INSERT INTO strength_sets (sha256, idx, ts, start_ts, duration_s, set_type, reps, weight_kg, category, category_name, category_subtype)
                 VALUES ('{sha}', 0, {start}, {start}, 40, 1, 10, 22.0, 0, 'bench_press', 0)"
            ),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(sql)).execute(pool).await.unwrap();
        }
    });
}

#[test]
fn an_activity_goes_over_by_its_hash_and_start_time() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let uid = e.user_id();
    e.stop();
    let sha = "a".repeat(64);
    insert_activity(dir.path(), uid, &sha, 1_799_000_000);

    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let rt = rt();
    let (s, body) = call(
        &rt,
        e.port(),
        reqwest::Method::PATCH,
        &format!("/api/activities/{sha}/strength"),
        Some(json!({ "sets": [{ "idx": 0, "reps": 11 }] })),
    );
    assert_eq!(s, 200, "{body}");
    let next = e.next_edit().unwrap().unwrap();
    assert_eq!(
        next.path,
        format!("/api/activities/{sha}@1799000000/strength")
    );
    assert_eq!(next.body, r#"{"sets":[{"idx":0,"reps":11}]}"#);
    e.stop();
}

#[test]
fn apply_state_replaces_the_copy_but_not_while_changes_wait() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let rt = rt();
    let p = e.port();
    let state = json!({
        "version": 1,
        "settings": { "step_goal": "15000", "timezone": "Europe/Copenhagen", "nonsense": "x" },
        "workouts": [{ "id": 40, "name": "Easy", "sport": "running", "definition": workout().to_string(),
                       "revision": 3, "created_at": 1_799_000_000, "updated_at": 1_799_000_100,
                       "send_state": "queued", "send_requested_at": 1_799_000_100,
                       "send_finished_at": null, "send_error": null }],
        "body": [{ "id": 8, "measured_at": 1_799_000_000, "weight_kg": 82.5, "body_fat_pct": null,
                   "waist_cm": null, "created_at": 1_799_000_000 }],
        "strength": [{ "activity_ident": "k:1", "set_idx": 0, "fields": "{\"reps\":4}",
                       "updated_at": 1_799_000_000 }],
    })
    .to_string();

    // Changes waiting: nothing is replaced.
    call(
        &rt,
        p,
        reqwest::Method::PATCH,
        "/api/settings",
        Some(json!({ "step_goal": 5000 })),
    );
    assert!(!e.apply_state(&state).unwrap());
    assert!(read(&rt, p, "/api/settings").contains("5000"));

    // Empty outbox: the copy becomes the server's, local rows included.
    call(
        &rt,
        p,
        reqwest::Method::POST,
        "/api/body",
        Some(json!({ "weight_kg": 60.0 })),
    );
    let first = e.next_edit().unwrap().unwrap();
    e.edit_done(&first.op_id, 200, "{}").unwrap();
    let second = e.next_edit().unwrap().unwrap();
    e.edit_done(&second.op_id, 201, r#"{"id": 9}"#).unwrap();
    assert!(e.apply_state(&state).unwrap());
    let settings = read(&rt, p, "/api/settings");
    assert!(
        settings.contains("15000") && settings.contains("Europe/Copenhagen"),
        "{settings}"
    );
    let workouts = read(&rt, p, "/api/workouts");
    assert!(
        workouts.contains("\"id\":40") && workouts.contains("queued"),
        "{workouts}"
    );
    let body = read(&rt, p, "/api/body");
    assert!(body.contains("82.5") && !body.contains("60.0"), "{body}");
    // The pull is not an edit.
    assert_eq!(e.pending_edits().unwrap(), 0);
    // A second time changes nothing more; a document that is not the state is an error.
    assert!(e.apply_state(&state).unwrap());
    assert!(e.apply_state("{}").is_err());
    assert!(e
        .apply_state(&state.replace("\"version\":1", "\"version\":2"))
        .is_err());
    e.stop();

    with_db(dir.path(), async |pool| {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM strength_set_overrides")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
        let keys: Vec<String> = sqlx::query_scalar("SELECT key FROM user_settings ORDER BY key")
            .fetch_all(pool)
            .await
            .unwrap();
        assert_eq!(keys, ["step_goal", "timezone"], "unknown keys are not kept");
        let ids: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM edit_id_map")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(ids, 0, "the pull clears the id map");
    });
}

#[test]
fn a_change_cut_off_by_a_crash_is_kept_and_sent_after_the_confirmed_one() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let rt = rt();
    call(
        &rt,
        e.port(),
        reqwest::Method::PATCH,
        "/api/settings",
        Some(json!({ "step_goal": 8500 })),
    );
    e.stop();
    // The process died after writing an outbox row and before its handler finished.
    with_db(dir.path(), async |pool| {
        sqlx::query(
            "INSERT INTO edit_outbox (op_id, method, path, body, made_at, ready)
             VALUES ('cut-off-op', 'PATCH', '/api/settings', '{\"step_goal\":9100}', 1799000000, 0)",
        )
        .execute(pool)
        .await
        .unwrap();
    });
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    assert_eq!(e.pending_edits().unwrap(), 2);
    let first = e.next_edit().unwrap().unwrap();
    assert!(first.body.contains("8500"));
    e.edit_done(&first.op_id, 200, "{}").unwrap();
    let second = e.next_edit().unwrap().unwrap();
    assert_eq!(
        (second.op_id.as_str(), second.body.as_str()),
        ("cut-off-op", r#"{"step_goal":9100}"#)
    );
    e.stop();
}

#[test]
fn a_change_still_being_made_blocks_the_ones_after_it_and_a_pull() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let uid = e.user_id();
    e.stop();
    // Written, handler not done: not offered yet, and a pull must not undo what it may change.
    with_db(dir.path(), async |pool| {
        sqlx::query(
            "INSERT INTO edit_outbox (op_id, method, path, body, made_at, ready)
             VALUES ('in-flight', 'PATCH', '/api/settings', '{}', 1799000000, 0)",
        )
        .execute(pool)
        .await
        .unwrap();
        // `start` would confirm it; keep this one in flight by checking the queries directly.
        assert_eq!(nimrag_server::outbox::next(pool, 0).await.unwrap(), None);
        assert_eq!(nimrag_server::outbox::count(pool).await.unwrap(), 1);
        let state = nimrag_server::edits::DeviceState {
            version: 1,
            settings: Default::default(),
            workouts: vec![],
            body: vec![],
            strength: vec![],
        };
        assert!(!nimrag_server::outbox::apply_state(pool, uid, &state)
            .await
            .unwrap());
    });
}
