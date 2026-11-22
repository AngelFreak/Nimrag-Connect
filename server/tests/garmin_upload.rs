//! Forward mode: queueing and the upload worker (outcomes, back-off, 401, 429).
//! Every test talks to the fake Garmin in `garmin_fake`; nothing reaches Garmin.

mod common;
mod garmin_fake;

use axum::http::StatusCode;
use garmin_fake::*;
use serde_json::json;

#[tokio::test]
async fn activity_is_queued_and_uploaded() {
    let r = rig().await;
    r.connect().await;
    let sha = r.activity(1).await;
    let (status, attempts, _, id, _) = r.item(&sha).await;
    assert_eq!((status.as_str(), attempts, id), ("pending", 0, None));
    let st = r.status().await;
    assert_eq!(
        (st["pending"].as_i64(), st["done"].as_i64()),
        (Some(1), Some(0))
    );

    assert_eq!(r.run().await, 1);
    let (status, attempts, _, id, err) = r.item(&sha).await;
    assert_eq!(status, "done");
    assert_eq!(attempts, 1);
    assert!(id.is_some_and(|i| i > 1000), "the activity id is stored");
    assert!(err.is_none());

    // The request is what the research says: bearer, multipart field `file`, the exact FIT bytes.
    r.fake.with(|f| {
        assert_eq!(f.uploads.len(), 1);
        let (bearer, body) = &f.uploads[0];
        assert!(bearer.starts_with("Bearer ey"));
        assert!(contains(body, "name=\"file\"; filename=\""));
        assert!(contains(body, ".fit\""));
        let fit = common::tiny_fit(1);
        assert!(body.windows(fit.len()).any(|w| w == fit.as_slice()));
        assert_eq!(f.requests.last().unwrap(), "POST /upload-service/upload");
    });
    let st = r.status().await;
    assert_eq!(
        (st["pending"].as_i64(), st["done"].as_i64()),
        (Some(1 - 1), Some(1))
    );
    assert!(st["next_attempt_at"].is_null());

    // Nothing is sent twice.
    assert_eq!(r.run().await, 0);
    assert_eq!(r.fake.upload_count(), 1);
}

#[tokio::test]
async fn success_without_an_id_is_still_done() {
    let r = rig().await;
    r.connect().await;
    r.fake.script([Up::OkNoId]);
    let sha = r.activity(1).await;
    r.run().await;
    let (status, _, _, id, _) = r.item(&sha).await;
    assert_eq!((status.as_str(), id), ("done", None));
}

#[tokio::test]
async fn duplicate_counts_as_done() {
    let r = rig().await;
    r.connect().await;
    r.fake.script([Up::Dup]);
    let sha = r.activity(1).await;
    r.run().await;
    let (status, attempts, _, id, note) = r.item(&sha).await;
    assert_eq!((status.as_str(), attempts, id), ("done", 1, None));
    assert!(note.unwrap().contains("already"));
    let st = r.status().await;
    assert_eq!(st["done"], 1);
    assert_eq!(st["failed"], 0);
    assert!(st["last_error"].is_null(), "a duplicate is not an error");
}

#[tokio::test]
async fn expired_access_token_refreshes_once_and_retries() {
    let r = rig().await;
    r.connect().await;
    let (old_access, _, _) = r.stored_tokens().await;
    r.fake.revoke_access(); // 401 although `exp` is far away
    let sha = r.activity(1).await;
    assert_eq!(r.run().await, 1);

    let (status, attempts, _, _, _) = r.item(&sha).await;
    assert_eq!(
        (status.as_str(), attempts),
        ("done", 1),
        "the refresh retry is not a failed try"
    );
    let (new_access, _, _) = r.stored_tokens().await;
    assert_ne!(new_access, old_access);
    r.fake.with(|f| {
        assert_eq!(f.refresh_calls, 1);
        // The 401 attempt never reaches the upload handler; only the retry is recorded.
        assert_eq!(f.uploads.len(), 1);
        assert_eq!(f.uploads[0].0, format!("Bearer {new_access}"));
    });
    assert_eq!(r.status().await["state"], "connected");
}

#[tokio::test]
async fn a_second_401_pauses_the_queue_until_retry() {
    let r = rig().await;
    r.connect().await;
    let sha = r.activity(1).await;
    let other = r.activity(2).await;
    // Garmin answers 401 now, accepts the refresh, but refuses the token it just issued.
    r.fake.revoke_access();
    r.fake.with(|f| f.refuse_new_tokens = true);

    assert_eq!(r.run().await, 1, "one file tried, then the queue stops");
    let st = r.status().await;
    assert_eq!(st["state"], "paused");
    assert!(st["last_error"].as_str().unwrap().contains("log in again"));
    assert_eq!(st["pending"], 2);
    // The file keeps its tries: a paused queue is not the file's fault.
    assert_eq!(r.item(&sha).await.1, 0);
    assert_eq!(r.item(&other).await.1, 0);
    r.fake.with(|f| {
        assert_eq!(f.refresh_calls, 1, "exactly one refresh");
        assert_eq!(f.uploads.len(), 0, "a 401 never reaches the upload handler");
    });

    // Paused: the worker does nothing, however much time passes.
    r.advance(86_400);
    assert_eq!(r.run().await, 0);

    // Retry resumes (Garmin works again).
    r.fake.with(|f| f.refuse_new_tokens = false);
    let (s, j) = r.post("/api/garmin/retry", json!({})).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["state"], "connected");
    assert_eq!(r.run().await, 2);
    assert_eq!(r.status().await["done"], 2);
}

#[tokio::test]
async fn rate_limit_honours_retry_after_for_the_whole_queue() {
    let r = rig().await;
    r.connect().await;
    let a = r.activity(1).await;
    let b = r.activity(2).await;
    r.fake.script([Up::Retry(429, 1800)]);
    let t0 = r.now();

    assert_eq!(r.run().await, 1, "the queue stops at the first 429");
    let (status, attempts, next, _, err) = r.item(&a).await;
    assert_eq!((status.as_str(), attempts, next), ("pending", 1, t0 + 1800));
    assert!(err.unwrap().contains("429"));
    // The other file is held back too, so Garmin is not hammered.
    assert_eq!(r.item(&b).await.2, t0 + 1800);
    assert_eq!(r.fake.upload_count(), 1);
    let st = r.status().await;
    assert_eq!(st["state"], "connected");
    assert!(st["next_attempt_at"].as_str().unwrap().len() > 10);

    r.advance(1799);
    assert_eq!(r.run().await, 0);
    r.advance(1);
    assert_eq!(r.run().await, 2);
    assert_eq!(r.status().await["done"], 2);
}

#[tokio::test]
async fn rate_limit_without_retry_after_uses_the_back_off() {
    let r = rig().await;
    r.connect().await;
    let a = r.activity(1).await;
    r.fake.script([Up::Status(429)]);
    let t0 = r.now();
    r.run().await;
    assert_eq!(r.item(&a).await.2, t0 + 60);
}

#[tokio::test]
async fn server_errors_back_off_one_minute_doubling_and_give_up_after_ten() {
    let r = rig().await;
    r.connect().await;
    let sha = r.activity(1).await;
    r.fake.with(|f| f.default_upload = Up::Status(503));

    let mut expected = 60;
    for attempt in 1..=9 {
        let t = r.now();
        assert_eq!(r.run().await, 1, "attempt {attempt}");
        let (status, attempts, next, _, err) = r.item(&sha).await;
        assert_eq!((status.as_str(), attempts), ("pending", attempt));
        assert_eq!(
            next,
            t + expected.min(6 * 3600),
            "delay after attempt {attempt}"
        );
        assert!(err.unwrap().contains("503"));
        // Not due yet: nothing happens one second early.
        r.advance(expected.min(6 * 3600) - 1);
        assert_eq!(r.run().await, 0);
        r.advance(1);
        expected *= 2;
    }
    // The tenth failure needs the owner.
    assert_eq!(r.run().await, 1);
    let (status, attempts, _, _, err) = r.item(&sha).await;
    assert_eq!((status.as_str(), attempts), ("failed", 10));
    assert!(err.unwrap().contains("gave up after 10 tries"));
    let st = r.status().await;
    assert_eq!(
        (st["failed"].as_i64(), st["pending"].as_i64()),
        (Some(1), Some(0))
    );
    assert!(st["last_error"].as_str().unwrap().contains("gave up"));
    r.advance(10 * 86_400);
    assert_eq!(r.run().await, 0, "no more tries without an owner action");
    assert_eq!(r.fake.upload_count(), 10);

    // Retry gives it a fresh set of tries.
    r.fake.with(|f| f.default_upload = Up::Ok(77));
    let (s, j) = r.post("/api/garmin/retry", json!({})).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(
        (j["failed"].as_i64(), j["pending"].as_i64()),
        (Some(0), Some(1))
    );
    assert_eq!(r.run().await, 1);
    let (status, attempts, _, id, _) = r.item(&sha).await;
    assert_eq!((status.as_str(), attempts), ("done", 1));
    assert!(id.is_some());
}

#[tokio::test]
async fn network_failure_is_transient() {
    let r = rig().await;
    r.connect().await;
    let sha = r.activity(1).await;
    // Point the worker at a port nobody listens on by dropping the fake's accept loop:
    // simplest is a second server state with an unreachable URL.
    let dead = nimrag_server::garmin::GarminConfig::for_fake("http://127.0.0.1:9");
    let state = r.state.clone().with_garmin(dead);
    assert_eq!(
        nimrag_server::garmin::queue::run_due(&state).await.unwrap(),
        1
    );
    let (status, attempts, next, _, err) = r.item(&sha).await;
    assert_eq!(
        (status.as_str(), attempts, next),
        ("pending", 1, r.now() + 60)
    );
    assert!(err.unwrap().contains("network error"));
}

#[tokio::test]
async fn a_refused_file_fails_at_once_and_never_blocks_the_others() {
    let r = rig().await;
    r.connect().await;
    let bad = r.activity(1).await;
    let good = r.activity(2).await;
    r.fake.script([Up::Status(422), Up::OtherConflict]);
    assert_eq!(r.run().await, 2);
    let (status, attempts, _, _, err) = r.item(&bad).await;
    assert_eq!((status.as_str(), attempts), ("failed", 1));
    assert!(err.unwrap().contains("422"));
    // A 409 that is not the duplicate answer is a refusal, not a success.
    assert_eq!(r.item(&good).await.0, "failed");

    r.fake.with(|f| f.default_upload = Up::Ok(5));
    let third = r.activity(3).await;
    assert_eq!(r.run().await, 1);
    assert_eq!(r.item(&third).await.0, "done");
    assert_eq!(r.status().await["failed"], 2);
}

// ---- what is queued -----------------------------------------------------------------------

#[tokio::test]
async fn non_activity_files_are_never_queued() {
    let r = rig().await;
    r.connect().await;
    // Monitoring (128/32), sleep-ish and other FIT sub types, non-watch metadata, no metadata.
    r.file("data_type=128&sub_type=32&source=watch", 1).await;
    r.file("data_type=128&sub_type=44&source=watch", 2).await;
    r.file("data_type=128&source=watch", 3).await;
    r.file("data_type=8&sub_type=4&source=watch", 4).await;
    r.file("source=import", 5).await;
    // A 128/4 upload that is not FIT bytes.
    let (s, _) = r
        .env
        .upload(
            "data_type=128&sub_type=4&source=watch",
            b"this is not a FIT file",
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(r.queue_len().await, 0);
    assert_eq!(r.run().await, 0);
    assert_eq!(r.fake.upload_count(), 0);
}

#[tokio::test]
async fn forward_off_queues_nothing() {
    let r = rig().await;
    // Never switched on.
    r.activity(1).await;
    assert_eq!(r.queue_len().await, 0);
    assert_eq!(r.status().await["state"], "off");
    assert_eq!(r.run().await, 0);
    assert!(r.fake.with(|f| f.requests.is_empty()));

    // Switched on: the next activity is queued; the earlier one is not (backfill is explicit).
    r.connect().await;
    r.activity(2).await;
    assert_eq!(r.queue_len().await, 1);
}

#[tokio::test]
async fn a_re_upload_of_a_stored_file_is_not_queued_again() {
    let r = rig().await;
    r.connect().await;
    let sha = r.activity(1).await;
    r.run().await;
    let (s, j) = r
        .env
        .upload(
            "data_type=128&sub_type=4&source=watch",
            &common::tiny_fit(1),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(r.queue_len().await, 1);
    assert_eq!(r.item(&sha).await.0, "done");
    assert_eq!(r.run().await, 0);
}

#[tokio::test]
async fn files_stored_while_paused_wait_for_the_login() {
    let r = rig().await;
    r.connect().await;
    r.fake.with(|f| f.refresh_valid.clear());
    r.fake.revoke_access();
    let a = r.activity(1).await;
    r.run().await;
    assert_eq!(r.status().await["state"], "needs_login");
    let b = r.activity(2).await;
    assert_eq!(
        r.queue_len().await,
        2,
        "still queued while the login is expired"
    );
    assert_eq!(r.run().await, 0);
    r.connect().await;
    assert_eq!(r.run().await, 2);
    assert_eq!(r.item(&a).await.0, "done");
    assert_eq!(r.item(&b).await.0, "done");
}

// ---- backfill ------------------------------------------------------------------------------

// Upserts: with a FIT profile compiled in, the upload already derived these rows.
async fn derived_activity(r: &Rig, tag: u32, start_ts: i64) -> String {
    let sha = r.file("data_type=128&sub_type=4&source=watch", tag).await;
    let src_id = i64::from_str_radix(&sha[..15], 16).unwrap();
    for sql in [
        format!(
            "INSERT OR REPLACE INTO sources (sha256, src_id, ok, last_ts, msg_count, rank_key) VALUES ('{sha}', {src_id}, 1, {start_ts}, 1, 'k{tag}')"
        ),
        format!(
            "INSERT OR REPLACE INTO activities (sha256, ts, start_ts, total_timer_s, num_sessions, sport, sub_sport) VALUES ('{sha}', {start_ts}, {start_ts}, 1000, 1, 1, 0)"
        ),
    ] {
        r.env.exec(&sql).await;
    }
    sha
}

#[tokio::test]
async fn backfill_queues_the_last_n_days_once() {
    let r = rig().await;
    // Both activities are stored while Forward is still off, so nothing is queued.
    let recent = derived_activity(&r, 1, r.now() - 2 * 86_400).await;
    let old = derived_activity(&r, 2, r.now() - 40 * 86_400).await;
    assert_eq!(r.queue_len().await, 0);

    // Without Forward there is nothing to queue into.
    let (s, _) = r.post("/api/garmin/backfill", json!({"days": 7})).await;
    assert_eq!(s, StatusCode::CONFLICT);

    r.connect().await;
    for bad in [0, -1, 3651] {
        let (s, _) = r.post("/api/garmin/backfill", json!({"days": bad})).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "days {bad}");
    }
    let (s, j) = r.post("/api/garmin/backfill", json!({"days": 7})).await;
    assert_eq!((s, j["queued"].as_i64()), (StatusCode::OK, Some(1)));
    assert_eq!(r.item(&recent).await.0, "pending");
    let (s, j) = r.post("/api/garmin/backfill", json!({"days": 7})).await;
    assert_eq!(
        (s, j["queued"].as_i64()),
        (StatusCode::OK, Some(0)),
        "idempotent"
    );

    r.run().await;
    let (s, j) = r.post("/api/garmin/backfill", json!({"days": 60})).await;
    assert_eq!(
        (s, j["queued"].as_i64()),
        (StatusCode::OK, Some(1)),
        "the sent one is not queued again"
    );
    assert_eq!(r.item(&old).await.0, "pending");
    assert_eq!(r.item(&recent).await.0, "done");
    r.run().await;
    assert_eq!(r.status().await["done"], 2);
}

// ---- everything from the watch (send_all) ----------------------------------------------------

const MONITORING: &str = "data_type=128&sub_type=32&source=watch";
const SLEEP: &str = "data_type=128&sub_type=49&source=usb";

#[tokio::test]
async fn the_choice_defaults_to_activities_only_and_is_changed_through_the_api() {
    let r = rig().await;
    // No account: nothing to change.
    assert_eq!(r.status().await["send_all"], false);
    let (s, _) = r
        .post("/api/garmin/settings", json!({"send_all": true}))
        .await;
    assert_eq!(s, StatusCode::CONFLICT);

    r.connect().await;
    assert_eq!(
        r.status().await["send_all"],
        false,
        "a new account sends activities only"
    );
    r.set_send_all(true).await;
    assert_eq!(r.status().await["send_all"], true);
    // The choice is a setting, not a backfill: nothing was queued, and logging in again keeps it.
    assert_eq!(r.queue_len().await, 0);
    r.connect().await;
    assert_eq!(r.status().await["send_all"], true);
    r.set_send_all(false).await;
    assert_eq!(r.status().await["send_all"], false);

    // Bad bodies and a missing CSRF header are refused.
    let (s, _) = r
        .post("/api/garmin/settings", json!({"send_all": "yes"}))
        .await;
    assert!(s.is_client_error());
    let (s, _) = r.post("/api/garmin/settings", json!({})).await;
    assert!(s.is_client_error());
    let (s, _) = r
        .env
        .request(
            axum::http::Method::POST,
            "/api/garmin/settings",
            None,
            b"{}".to_vec(),
        )
        .await;
    assert!(s.is_client_error(), "no session, no CSRF header");

    // Logging out forgets the choice with the account.
    r.set_send_all(true).await;
    let (s, _) = r.post("/api/garmin/logout", json!({})).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    r.connect().await;
    assert_eq!(r.status().await["send_all"], false);
}

#[tokio::test]
async fn send_all_off_queues_only_activities() {
    let r = rig().await;
    r.connect().await;
    r.file(MONITORING, 1).await;
    r.file(SLEEP, 2).await;
    let act = r.activity(3).await;
    assert_eq!(r.queue_len().await, 1);
    assert_eq!(r.run().await, 1);
    assert_eq!(r.item(&act).await.0, "done");
    assert_eq!(r.uploaded_tags([1, 2, 3]), [3]);
}

#[tokio::test]
async fn send_all_on_queues_watch_files_and_never_the_ones_that_came_from_garmin() {
    let r = rig().await;
    r.connect().await;
    r.set_send_all(true).await;
    // Garmin sends no activity id for anything but an activity.
    r.fake.with(|f| f.default_upload = Up::OkNoId);
    let mon = r.file(MONITORING, 1).await;
    let sleep = r.file(SLEEP, 2).await;
    let act = r.activity(3).await;
    let metrics = r.file("data_type=128&sub_type=44&source=watch", 4).await;
    // From Garmin (an export or Connect history), not from the watch: never sent back.
    r.file("data_type=128&sub_type=32&source=import", 5).await;
    // Not FIT bytes, not a FIT data type, no metadata.
    let (s, _) = r.env.upload(MONITORING, b"this is not a FIT file").await;
    assert_eq!(s, StatusCode::CREATED);
    r.file("data_type=8&sub_type=32&source=watch", 7).await;
    r.file("source=watch", 8).await;

    let queued: Vec<String> = sqlx::query_scalar("SELECT file_sha256 FROM garmin_uploads")
        .fetch_all(&r.env.pool)
        .await
        .unwrap();
    for sha in [&mon, &sleep, &act, &metrics] {
        assert!(queued.contains(sha), "{sha} is queued");
    }
    assert_eq!(queued.len(), 4, "{queued:?}");

    assert_eq!(r.run().await, 4);
    let st = r.status().await;
    assert_eq!(
        (st["done"].as_i64(), st["pending"].as_i64()),
        (Some(4), Some(0))
    );
    // A file that is not an activity is done with no activity id.
    assert_eq!(r.item(&mon).await.3, None);

    // A re-upload of a stored file is not queued again.
    let (s, _) = r.env.upload(MONITORING, &common::tiny_fit(1)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r.queue_len().await, 4);

    // Back to activities only: new monitoring files are not queued any more.
    r.set_send_all(false).await;
    r.file(MONITORING, 20).await;
    r.activity(21).await;
    assert_eq!(r.queue_len().await, 5);
}

#[tokio::test]
async fn a_non_activity_file_is_sent_like_any_other_file() {
    let r = rig().await;
    r.connect().await;
    r.set_send_all(true).await;
    r.fake.script([Up::OkNoId]);
    let sha = r.file(MONITORING, 1).await;
    assert_eq!(r.run().await, 1);
    let (status, attempts, _, id, err) = r.item(&sha).await;
    assert_eq!(
        (status.as_str(), attempts, id, err),
        ("done", 1, None, None)
    );
    r.fake.with(|f| {
        let (_, body) = &f.uploads[0];
        assert!(contains(body, "name=\"file\"; filename=\""));
        assert_eq!(f.requests.last().unwrap(), "POST /upload-service/upload");
    });
}

#[tokio::test]
async fn a_duplicate_answer_for_a_non_activity_file_is_done_and_says_file() {
    let r = rig().await;
    r.connect().await;
    r.set_send_all(true).await;
    r.fake.script([Up::Dup, Up::Dup]);
    let mon = r.file(MONITORING, 1).await;
    let act = r.activity(2).await;
    assert_eq!(r.run().await, 2);

    let (status, attempts, _, id, note) = r.item(&mon).await;
    assert_eq!((status.as_str(), attempts, id), ("done", 1, None));
    let note = note.unwrap();
    assert!(note.contains("already had this file"), "{note}");
    assert!(!note.contains("activity"), "{note}");
    // An activity keeps its own wording.
    let (status, _, _, _, note) = r.item(&act).await;
    assert_eq!(status, "done");
    assert!(note.unwrap().contains("already had this activity"));

    let st = r.status().await;
    assert_eq!(
        (st["done"].as_i64(), st["failed"].as_i64()),
        (Some(2), Some(0))
    );
    assert!(st["last_error"].is_null(), "a duplicate is not an error");
}

#[tokio::test]
async fn backfill_all_queues_watch_files_since_the_time_oldest_first_and_skips_queued_ones() {
    let r = rig().await;
    let t0 = r.now();
    let day = 86_400;
    // Stored (and timed) out of order, while Forward is off, so nothing is queued yet.
    let newest = r.activity(1).await; // 1 day ago
    let oldest = r.file(MONITORING, 2).await; // 5 days ago
    let too_old = r.file(MONITORING, 3).await; // 10 days ago
    let middle = r.file(SLEEP, 4).await; // 3 days ago
    let from_garmin = r.file("data_type=128&sub_type=32&source=import", 5).await; // 2 days ago
    let same_instant_a = r.file(MONITORING, 6).await; // 4 days ago, same second as the next
    let same_instant_b = r.file(MONITORING, 7).await;
    r.received(&newest, t0 - day, 0).await;
    r.received(&oldest, t0 - 5 * day, 0).await;
    r.received(&too_old, t0 - 10 * day, 0).await;
    r.received(&middle, t0 - 3 * day, 500).await;
    r.received(&from_garmin, t0 - 2 * day, 0).await;
    r.received(&same_instant_a, t0 - 4 * day, 100).await;
    r.received(&same_instant_b, t0 - 4 * day, 200).await;
    assert_eq!(r.queue_len().await, 0);

    // Needs Forward on, and exactly one way to say how far back.
    let (s, _) = r
        .post("/api/garmin/backfill", json!({"days": 7, "mode": "all"}))
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    r.connect().await;
    for bad in [
        json!({"mode": "all"}),
        json!({"days": 7, "since": "2026-11-15", "mode": "all"}),
        json!({"days": 0, "mode": "all"}),
        json!({"since": "yesterday", "mode": "all"}),
        json!({"since": "2099-01-01", "mode": "all"}),
        json!({"since": "2001-01-01", "mode": "all"}),
        json!({"days": 7, "mode": "everything"}),
    ] {
        let (s, j) = r.post("/api/garmin/backfill", bad.clone()).await;
        assert!(s.is_client_error(), "{bad}: {s} {j}");
    }
    assert_eq!(r.queue_len().await, 0);

    let (s, j) = r
        .post("/api/garmin/backfill", json!({"days": 7, "mode": "all"}))
        .await;
    assert_eq!((s, j["queued"].as_i64()), (StatusCode::OK, Some(5)), "{j}");
    let queued: Vec<String> = sqlx::query_scalar("SELECT file_sha256 FROM garmin_uploads")
        .fetch_all(&r.env.pool)
        .await
        .unwrap();
    assert!(!queued.contains(&too_old) && !queued.contains(&from_garmin));

    // Oldest first, by the time the server received the file.
    assert_eq!(r.run().await, 5);
    assert_eq!(r.uploaded_tags(1..=7), [2, 6, 7, 4, 1]);
    assert_eq!(r.item(&newest).await.0, "done");

    // Everything is queued or sent now: the same window queues nothing, a wider one only the
    // file it adds.
    let (_, j) = r
        .post("/api/garmin/backfill", json!({"days": 7, "mode": "all"}))
        .await;
    assert_eq!(j["queued"], 0);
    let (_, j) = r
        .post("/api/garmin/backfill", json!({"days": 30, "mode": "all"}))
        .await;
    assert_eq!(j["queued"], 1);
    assert_eq!(r.item(&too_old).await.0, "pending");
    // Already-queued files are skipped even while they wait.
    let (_, j) = r
        .post("/api/garmin/backfill", json!({"days": 30, "mode": "all"}))
        .await;
    assert_eq!(j["queued"], 0);
    assert_eq!(r.run().await, 1);
    assert_eq!(r.uploaded_tags(1..=7), [2, 6, 7, 4, 1, 3]);
}

#[tokio::test]
async fn backfill_all_takes_a_start_date_and_only_the_users_own_files() {
    let r = rig().await;
    let a = r.file(MONITORING, 1).await;
    // Another user's file is not this user's to send.
    let other = r
        .env
        .add_user("someone-else", nimrag_server::users::Role::User)
        .await;
    let token = r.env.token_for(other).await;
    let (s, j) = r
        .env
        .request(
            axum::http::Method::POST,
            &format!("/api/files?{MONITORING}"),
            Some(&token),
            common::tiny_fit(2),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{j}");
    let theirs = j["sha256"].as_str().unwrap().to_owned();
    r.received(&a, r.now() - 3600, 0).await;
    r.connect().await;

    let (s, j) = r
        .post(
            "/api/garmin/backfill",
            json!({"since": "2020-01-01", "mode": "all"}),
        )
        .await;
    assert_eq!((s, j["queued"].as_i64()), (StatusCode::OK, Some(1)), "{j}");
    assert_eq!(r.item(&a).await.0, "pending");
    assert_eq!(r.queue_len().await, 1);
    let theirs_queued: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM garmin_uploads WHERE file_sha256 = ?")
            .bind(&theirs)
            .fetch_one(&r.env.pool)
            .await
            .unwrap();
    assert_eq!(theirs_queued, 0);
}

#[tokio::test]
async fn the_activities_backfill_still_works_and_leaves_other_files_alone() {
    let r = rig().await;
    let act = derived_activity(&r, 1, r.now() - 86_400).await;
    r.file(MONITORING, 2).await;
    r.connect().await;
    // No mode = activities, as before; the monitoring file is not queued.
    let (s, j) = r.post("/api/garmin/backfill", json!({"days": 7})).await;
    assert_eq!((s, j["queued"].as_i64()), (StatusCode::OK, Some(1)), "{j}");
    let (_, j) = r
        .post(
            "/api/garmin/backfill",
            json!({"days": 7, "mode": "activities"}),
        )
        .await;
    assert_eq!(j["queued"], 0);
    assert_eq!(r.queue_len().await, 1);
    assert_eq!(r.item(&act).await.0, "pending");
}

#[tokio::test]
async fn the_worker_pauses_between_uploads() {
    let r = rig().await;
    r.connect().await;
    for tag in 1..=3 {
        r.activity(tag).await;
    }
    let mut cfg = r.fake.config();
    cfg.upload_pause_ms = 150;
    let state = r.state.clone().with_garmin(cfg);
    let started = std::time::Instant::now();
    assert_eq!(
        nimrag_server::garmin::queue::run_due(&state).await.unwrap(),
        3
    );
    // A breath between uploads, none after the last one.
    let took = started.elapsed();
    assert!(took >= std::time::Duration::from_millis(300), "{took:?}");
    assert!(
        took < std::time::Duration::from_millis(450 + 2000),
        "{took:?}"
    );
    assert_eq!(r.fake.upload_count(), 3);
}

#[tokio::test]
async fn retry_needs_forward_on_and_a_valid_login() {
    let r = rig().await;
    let (s, _) = r.post("/api/garmin/retry", json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT);
    r.connect().await;
    r.fake.with(|f| f.refresh_valid.clear());
    r.fake.revoke_access();
    r.activity(1).await;
    r.run().await;
    assert_eq!(r.status().await["state"], "needs_login");
    let (s, j) = r.post("/api/garmin/retry", json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "{j}");
}
