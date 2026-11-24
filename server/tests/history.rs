//! The server's stored files for a phone (docs/design/history-on-phone.md): `GET
//! /api/device/files` pages the token user's files newest first, `.../content` gives the bytes.

mod common;

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use common::*;
use http_body_util::BodyExt;
use nimrag_server::derived::{rank_key, src_id};
use nimrag_server::users::Role;
use nimrag_server::{store, tokens};
use serde_json::Value;
use std::sync::atomic::Ordering;
use tower::ServiceExt;

const NOW: i64 = 1_800_000_000;

fn sha(n: u32) -> String {
    format!("{n:x}{}", "a".repeat(63))
}

/// A file row of `uid` received at `second` of 2026-11-20, with a data time when `last_ts` is set.
async fn add(e: &Env, uid: i64, sha: &str, second: u32, last_ts: Option<i64>) {
    e.exec(&format!(
        "INSERT INTO files (sha256, size, received_at, source) VALUES ('{sha}', 7, '2026-11-20T10:00:{second:02}.000Z', 'connect')"
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO file_owners (sha256, user_id) VALUES ('{sha}', {uid})"
    ))
    .await;
    if let Some(ts) = last_ts {
        e.exec(&format!(
            "INSERT INTO sources (sha256, src_id, ok, last_ts, msg_count, rank_key, origin) VALUES ('{sha}', {}, 1, {ts}, 1, '{}', 'connect')",
            src_id(sha),
            rank_key(true, Some(ts), 1)
        ))
        .await;
    }
}

async fn files(e: &Env, token: &str, query: &str) -> (StatusCode, Value) {
    e.request(
        Method::GET,
        &format!("/api/device/files{query}"),
        Some(token),
        vec![],
    )
    .await
}

fn shas(j: &Value) -> Vec<String> {
    j["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["sha256"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn the_list_is_paged_newest_first_with_a_stable_order_for_equal_times() {
    let e = env_at(NOW).await;
    e.adopt.store(false, Ordering::Relaxed);
    // Seconds 1..=4; two arrived in the same instant, so the hash decides.
    add(&e, e.user_id, &sha(1), 1, Some(100)).await;
    add(&e, e.user_id, &sha(2), 2, Some(200)).await;
    add(&e, e.user_id, &sha(3), 3, None).await;
    add(&e, e.user_id, &sha(6), 4, Some(400)).await;
    add(&e, e.user_id, &sha(7), 4, Some(400)).await;

    let (s, all) = files(&e, &e.token, "").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(shas(&all), [sha(6), sha(7), sha(3), sha(2), sha(1)]);
    assert!(all["next_cursor"].is_null());
    let first = &all["files"][0];
    assert_eq!(first["size"], 7);
    assert_eq!(first["source"], "connect");
    assert_eq!(first["last_ts"], 400);
    assert!(first["received_at"]
        .as_str()
        .unwrap()
        .starts_with("2026-11-20T10:00:04"));
    assert!(all["files"][2]["last_ts"].is_null());

    // Pages of two walk the same list once, with no overlap and no gap.
    let mut seen = vec![];
    let mut cursor = String::new();
    for _ in 0..10 {
        let (s, page) = files(&e, &e.token, &format!("?limit=2&cursor={cursor}")).await;
        assert_eq!(s, StatusCode::OK);
        seen.extend(shas(&page));
        match page["next_cursor"].as_str() {
            Some(c) => cursor = c.to_owned(),
            None => break,
        }
    }
    assert_eq!(seen, shas(&all));

    // A bad cursor is a 400; a limit out of range is clamped.
    assert_eq!(
        files(&e, &e.token, "?cursor=zz").await.0,
        StatusCode::BAD_REQUEST
    );
    let (s, j) = files(&e, &e.token, "?limit=0").await;
    assert_eq!((s, shas(&j).len()), (StatusCode::OK, 1));
}

#[tokio::test]
async fn since_keeps_files_with_later_data_and_those_with_none_known() {
    let e = env_at(NOW).await;
    e.adopt.store(false, Ordering::Relaxed);
    add(&e, e.user_id, &sha(1), 1, Some(100)).await;
    add(&e, e.user_id, &sha(2), 2, Some(200)).await;
    add(&e, e.user_id, &sha(3), 3, None).await;
    let (_, j) = files(&e, &e.token, "?since=200").await;
    assert_eq!(shas(&j), [sha(3), sha(2)]);
    let (_, j) = files(&e, &e.token, "?since=201").await;
    assert_eq!(shas(&j), [sha(3)]);
    // A window and a cursor together.
    let (_, j) = files(&e, &e.token, "?since=100&limit=1").await;
    let cursor = j["next_cursor"].as_str().unwrap().to_owned();
    let (_, j) = files(&e, &e.token, &format!("?since=100&limit=5&cursor={cursor}")).await;
    assert_eq!(shas(&j), [sha(2), sha(1)]);
    assert_eq!(
        files(&e, &e.token, "?since=abc").await.0,
        StatusCode::BAD_REQUEST
    );
}

async fn content(e: &Env, token: Option<&str>, sha: &str) -> (StatusCode, Vec<u8>, Option<String>) {
    let mut req = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/device/files/{sha}/content"));
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let resp = e
        .app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let ctype = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap().to_owned());
    (
        status,
        resp.into_body().collect().await.unwrap().to_bytes().to_vec(),
        ctype,
    )
}

#[tokio::test]
async fn the_content_is_the_stored_blob_and_only_for_the_owner() {
    let e = env_at(NOW).await;
    e.adopt.store(false, Ordering::Relaxed);
    let bytes = b"\x00\x01 some stored bytes \xff".to_vec();
    let r = store::receive_bytes(e.dir.path(), &bytes).unwrap();
    store::commit(e.dir.path(), &r.tmp, &r.sha256).unwrap();
    e.exec(&format!(
        "INSERT INTO files (sha256, size, received_at, source) VALUES ('{}', {}, '2026-11-20T10:00:01.000Z', 'import')",
        r.sha256,
        bytes.len()
    ))
    .await;
    let bea = e.add_user("bea", Role::User).await;
    e.exec(&format!(
        "INSERT INTO file_owners (sha256, user_id) VALUES ('{}', {bea})",
        r.sha256
    ))
    .await;
    let bea_token = e.token_for(bea).await;

    // Bea's file is not ours (404, not listed); for her it is.
    let (s, _, _) = content(&e, Some(&e.token), &r.sha256).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(shas(&files(&e, &e.token, "").await.1).is_empty());
    let (s, got, ctype) = content(&e, Some(&bea_token), &r.sha256).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(got, bytes);
    assert_eq!(ctype.as_deref(), Some("application/octet-stream"));
    assert_eq!(shas(&files(&e, &bea_token, "").await.1), [r.sha256.clone()]);

    // Unknown and malformed hashes.
    assert_eq!(
        content(&e, Some(&e.token), &sha(5)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        content(&e, Some(&e.token), "nope").await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn a_token_needs_a_user_and_there_is_no_other_way_in() {
    let e = env_at(NOW).await;
    e.adopt.store(false, Ordering::Relaxed);
    add(&e, e.user_id, &sha(1), 1, None).await;
    let (_, loose) = tokens::create(&e.pool, "loose", None).await.unwrap();
    assert_eq!(files(&e, &loose, "").await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        content(&e, Some(&loose), &sha(1)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(files(&e, "nrg_nope", "").await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        content(&e, Some("nrg_nope"), &sha(1)).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (s, _) = e
        .request(Method::GET, "/api/device/files", None, vec![])
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(content(&e, None, &sha(1)).await.0, StatusCode::UNAUTHORIZED);
    // A signed-in browser session is no way in either: this is the phone's door.
    let cookie = e.login().await;
    let req = Request::builder()
        .uri("/api/device/files")
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        e.app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}
