//! Sign-in rules: first-run setup (admin, then 2FA), login with a code, 2FA enrolment, forced
//! password change, TOTP replay protection, per-username limiting, own password and 2FA changes.

mod common;

use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, Method, StatusCode};
use common::*;
use nimrag_server::totp;
use nimrag_server::users::Role;
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower::ServiceExt;

const NOW: i64 = 1_800_000_000;

async fn post(e: &Env, uri: &str, body: Value) -> (StatusCode, HeaderMap, Value) {
    e.call(Method::POST, uri, None, true, Some(body)).await
}

fn cookie_of(h: &HeaderMap) -> String {
    h.get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

fn code(e: &Env, secret: &str, offset_steps: i64) -> String {
    totp::code_at(secret, e.now() + 30 * offset_steps).unwrap()
}

/// A login from `peer` (the TCP peer address the limiter sees).
async fn login_at(e: &Env, peer: &str, user: &str, pw: &str, code: Option<String>) -> (StatusCode, HeaderMap, Value) {
    let mut body = json!({ "username": user, "password": pw });
    if let Some(c) = code {
        body["code"] = json!(c);
    }
    let mut req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/api/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::new(peer.parse().unwrap(), 40000)));
    let resp = e.app.clone().oneshot(req).await.unwrap();
    let (status, headers) = (resp.status(), resp.headers().clone());
    let bytes = http_body_util::BodyExt::collect(resp.into_body()).await.unwrap().to_bytes();
    (status, headers, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// A login from a client address nobody used before, so only the per-username limit can bite.
async fn login(e: &Env, user: &str, pw: &str, code: Option<String>) -> (StatusCode, HeaderMap, Value) {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    login_at(e, &format!("198.18.{}.{}", n / 250, n % 250 + 1), user, pw, code).await
}

async fn users_count(e: &Env) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&e.pool)
        .await
        .unwrap()
}

// ---- first-run setup -----------------------------------------------------------------

#[tokio::test]
async fn setup_creates_the_admin_only_once_2fa_is_confirmed() {
    let e = bare_env_at(NOW).await;
    let (_, _, j) = e.call(Method::GET, "/api/setup", None, false, None).await;
    assert_eq!(j["needed"], true);

    // CSRF header, username and password rules.
    let ok = json!({ "username": "Boss", "password": PASSWORD });
    let (s, _, _) = e.call(Method::POST, "/api/setup", None, false, Some(ok.clone())).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    for bad in [
        json!({ "username": "ab", "password": PASSWORD }),
        json!({ "username": "has space", "password": PASSWORD }),
        json!({ "username": "boss", "password": "short" }),
    ] {
        let (s, _, _) = post(&e, "/api/setup", bad).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    let (s, _, start) = post(&e, "/api/setup", ok).await;
    assert_eq!(s, StatusCode::OK);
    let secret = start["secret"].as_str().unwrap().to_owned();
    assert!(start["otpauth_uri"].as_str().unwrap().starts_with("otpauth://totp/"));
    assert!(start["otpauth_uri"].as_str().unwrap().contains(&secret));
    // Nothing exists yet: setup is not complete (and not closed) until the code is confirmed.
    assert_eq!(users_count(&e).await, 0);
    let (_, _, j) = e.call(Method::GET, "/api/setup", None, false, None).await;
    assert_eq!(j["needed"], true);

    let confirm = |c: String| json!({ "pending_token": start["pending_token"], "code": c });
    let (s, h, _) = post(&e, "/api/setup/confirm", confirm("000000".into())).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(h.get(header::SET_COOKIE).is_none());
    assert_eq!(users_count(&e).await, 0);

    let (s, h, j) = post(&e, "/api/setup/confirm", confirm(code(&e, &secret, 0))).await;
    assert_eq!(s, StatusCode::CREATED, "{j}");
    assert_eq!((j["step"].as_str(), j["role"].as_str(), j["username"].as_str()), (Some("done"), Some("admin"), Some("boss")));
    let cookie = cookie_of(&h);
    let (s, _, me) = e.call(Method::GET, "/api/me", Some(&cookie), false, None).await;
    assert_eq!((s, me["role"].as_str()), (StatusCode::OK, Some("admin")));

    // Closed for everyone from now on; the pending token is spent.
    let (_, _, j) = e.call(Method::GET, "/api/setup", None, false, None).await;
    assert_eq!(j["needed"], false);
    let (s, _, _) = post(&e, "/api/setup", json!({ "username": "other", "password": PASSWORD })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _, _) = post(&e, "/api/setup/confirm", confirm(code(&e, &secret, 1))).await;
    assert_eq!(s, StatusCode::CONFLICT);

    // The confirming code is spent too: signing in with it again is a replay.
    let (s, _, _) = login(&e, "boss", PASSWORD, Some(code(&e, &secret, 0))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = login(&e, "boss", PASSWORD, Some(code(&e, &secret, 1))).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn racing_setups_have_one_winner() {
    let e = bare_env_at(NOW).await;
    let (_, _, a) = post(&e, "/api/setup", json!({ "username": "first", "password": PASSWORD })).await;
    let (_, _, b) = post(&e, "/api/setup", json!({ "username": "second", "password": PASSWORD })).await;
    let confirm = |s: &Value| {
        json!({
            "pending_token": s["pending_token"],
            "code": totp::code_at(s["secret"].as_str().unwrap(), NOW).unwrap(),
        })
    };
    let (ra, rb) = tokio::join!(
        post(&e, "/api/setup/confirm", confirm(&a)),
        post(&e, "/api/setup/confirm", confirm(&b)),
    );
    let mut codes = [ra.0, rb.0];
    codes.sort();
    assert_eq!(codes, [StatusCode::CREATED, StatusCode::CONFLICT]);
    assert_eq!(users_count(&e).await, 1);
    // And at the storage layer: of two racing claims exactly one inserts.
    let (x, y) = tokio::join!(
        nimrag_server::users::claim_first_admin(&e.pool, "x1", "h", "S", 1, 0),
        nimrag_server::users::claim_first_admin(&e.pool, "x2", "h", "S", 1, 0),
    );
    assert!(!x.unwrap() && !y.unwrap(), "a user already exists");
}

#[tokio::test]
async fn claim_first_admin_has_one_winner_on_an_empty_database() {
    let e = bare_env_at(NOW).await;
    let (a, b) = tokio::join!(
        nimrag_server::users::claim_first_admin(&e.pool, "one", "h", "S", 1, 0),
        nimrag_server::users::claim_first_admin(&e.pool, "two", "h", "S", 1, 0),
    );
    assert_ne!(a.unwrap(), b.unwrap());
    assert_eq!(users_count(&e).await, 1);
}

#[tokio::test]
async fn a_few_wrong_setup_codes_do_not_spoil_the_step() {
    let e = bare_env_at(NOW).await;
    let (_, _, start) = post(&e, "/api/setup", json!({ "username": "boss", "password": PASSWORD })).await;
    let wrong = json!({ "pending_token": start["pending_token"], "code": "000000" });
    for _ in 0..4 {
        let (s, _, _) = post(&e, "/api/setup/confirm", wrong.clone()).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    let right = json!({ "pending_token": start["pending_token"], "code": code(&e, start["secret"].as_str().unwrap(), 0) });
    let (s, _, _) = post(&e, "/api/setup/confirm", right).await;
    assert_eq!(s, StatusCode::CREATED);
}

// ---- login with 2FA --------------------------------------------------------------------

#[tokio::test]
async fn a_user_without_2fa_is_asked_to_enrol_it_at_login() {
    let e = env_at(NOW).await;
    make_user(&e.pool, "newbie", Role::User, false).await;

    // Right password, no code needed (and none counted): the answer is the enrol step.
    let (s, h, j) = login(&e, "newbie", PASSWORD, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(h.get(header::SET_COOKIE).is_none(), "an enrol step is not a session");
    assert_eq!(j["step"], "enrol2fa");
    assert_eq!(j["must_change_password"], false);
    let secret = j["secret"].as_str().unwrap().to_owned();
    assert!(j["otpauth_uri"].as_str().unwrap().contains("newbie"));
    let token = j["pending_token"].clone();

    // The pending token is no session and a wrong code enrols nothing.
    let (s, _, _) = e.call(Method::GET, "/api/me", Some(&format!("__Host-nimrag_session={}", token.as_str().unwrap())), false, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, h, _) = post(&e, "/api/login/totp-enrol", json!({ "pending_token": token, "code": "000000" })).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(h.get(header::SET_COOKIE).is_none());
    let enabled: i64 = sqlx::query_scalar("SELECT totp_enabled FROM users WHERE username = 'newbie'").fetch_one(&e.pool).await.unwrap();
    assert_eq!(enabled, 0);

    let (s, h, j) = post(&e, "/api/login/totp-enrol", json!({ "pending_token": token, "code": code(&e, &secret, 0) })).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["step"], "done");
    let cookie = cookie_of(&h);
    e.get(&cookie, "/api/me").await;
    // The step is spent.
    let (s, _, _) = post(&e, "/api/login/totp-enrol", json!({ "pending_token": token, "code": code(&e, &secret, 1) })).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // From now on a code is required, and the enrolling code cannot be reused.
    let (s, _, _) = login(&e, "newbie", PASSWORD, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = login(&e, "newbie", PASSWORD, Some(code(&e, &secret, 0))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "replay of the enrolment code");
    let (s, _, _) = login(&e, "newbie", PASSWORD, Some(code(&e, &secret, 1))).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn totp_codes_are_single_use_and_never_go_back_in_time() {
    let e = env_at(NOW).await;
    // +-1 step is accepted; a code is accepted once; an older step than the last accepted is refused.
    let (s, _, _) = login(&e, USER, PASSWORD, Some(code(&e, TOTP_SECRET, 0))).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = login(&e, USER, PASSWORD, Some(code(&e, TOTP_SECRET, 0))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "same code twice");
    let (s, _, _) = login(&e, USER, PASSWORD, Some(code(&e, TOTP_SECRET, -1))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "an earlier step than the last accepted");
    let (s, _, _) = login(&e, USER, PASSWORD, Some(code(&e, TOTP_SECRET, 1))).await;
    assert_eq!(s, StatusCode::OK, "the next step is inside the skew");
    let (s, _, _) = login(&e, USER, PASSWORD, Some(code(&e, TOTP_SECRET, 2))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "two steps ahead is outside the skew");
    // Malformed codes are plain failures.
    for bad in ["", "12345", "1234567", "abcdef", " 12345"] {
        let (s, _, _) = login(&e, USER, PASSWORD, Some(bad.into())).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{bad:?}");
    }
}

#[tokio::test]
async fn parallel_logins_with_one_code_let_only_one_in() {
    let e = env_at(NOW).await;
    let c = code(&e, TOTP_SECRET, 0);
    let (a, b) = tokio::join!(
        login(&e, USER, PASSWORD, Some(c.clone())),
        login(&e, USER, PASSWORD, Some(c.clone())),
    );
    let mut got = [a.0, b.0];
    got.sort();
    assert_eq!(got, [StatusCode::OK, StatusCode::UNAUTHORIZED]);
}

#[tokio::test]
async fn a_temporary_password_must_be_replaced_before_a_session_starts() {
    let e = env_at(NOW).await;
    // Enrolled user with a temporary password: code first, then the new password.
    e.exec("UPDATE users SET must_change_password = 1").await;
    let (s, h, j) = login(&e, USER, PASSWORD, Some(code(&e, TOTP_SECRET, 0))).await;
    assert_eq!((s, j["step"].as_str()), (StatusCode::OK, Some("change_password")));
    assert!(h.get(header::SET_COOKIE).is_none());
    let token = j["pending_token"].clone();
    for (pw, why) in [("short", "too short"), (PASSWORD, "same as the temporary one")] {
        let (s, _, _) = post(&e, "/api/login/change-password", json!({ "pending_token": token, "new_password": pw })).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{why}");
    }
    let (s, h, j) = post(&e, "/api/login/change-password", json!({ "pending_token": token, "new_password": "a brand new password" })).await;
    assert_eq!((s, j["step"].as_str()), (StatusCode::OK, Some("done")), "{j}");
    e.get(&cookie_of(&h), "/api/me").await;
    let flag: i64 = sqlx::query_scalar("SELECT must_change_password FROM users").fetch_one(&e.pool).await.unwrap();
    assert_eq!(flag, 0);
    // Only the new password works now.
    let (s, _, _) = login(&e, USER, PASSWORD, Some(code(&e, TOTP_SECRET, 1))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = login(&e, USER, "a brand new password", Some(code(&e, TOTP_SECRET, 1))).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn a_new_user_enrols_2fa_and_picks_a_password_in_one_step() {
    let e = env_at(NOW).await;
    make_user(&e.pool, "fresh", Role::User, false).await;
    e.exec("UPDATE users SET must_change_password = 1 WHERE username = 'fresh'").await;
    let (_, _, j) = login(&e, "fresh", PASSWORD, None).await;
    assert_eq!((j["step"].as_str(), j["must_change_password"].as_bool()), (Some("enrol2fa"), Some(true)));
    let secret = j["secret"].as_str().unwrap().to_owned();
    // The password change is required with the code.
    let (s, _, _) = post(&e, "/api/login/totp-enrol", json!({ "pending_token": j["pending_token"], "code": code(&e, &secret, 0) })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, h, _) = post(
        &e,
        "/api/login/totp-enrol",
        json!({ "pending_token": j["pending_token"], "code": code(&e, &secret, 0), "new_password": "my own new password" }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    e.get(&cookie_of(&h), "/api/me").await;
    let (s, _, _) = login(&e, "fresh", PASSWORD, Some(code(&e, &secret, 1))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = login(&e, "fresh", "my own new password", Some(code(&e, &secret, 1))).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn usernames_are_case_insensitive_and_a_disabled_user_looks_like_a_wrong_password() {
    let e = env_at(NOW).await;
    let (s, _, _) = login(&e, "TESTER", PASSWORD, Some(code(&e, TOTP_SECRET, 0))).await;
    assert_eq!(s, StatusCode::OK);
    e.exec("UPDATE users SET disabled_at = 5").await;
    let (s, h, bad) = login(&e, USER, PASSWORD, Some(code(&e, TOTP_SECRET, 1))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(h.get(header::SET_COOKIE).is_none());
    let (_, _, unknown) = login(&e, "nobody", PASSWORD, Some("123456".into())).await;
    assert_eq!(bad, unknown);
}

// ---- limiter -----------------------------------------------------------------------------

async fn login_from(e: &Env, peer: &str, user: &str, pw: &str) -> StatusCode {
    let mut req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/api/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            json!({ "username": user, "password": pw, "code": "000000" }).to_string(),
        ))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::new(peer.parse().unwrap(), 40000)));
    e.app.clone().oneshot(req).await.unwrap().status()
}

#[tokio::test]
async fn failures_against_one_username_lock_it_whatever_the_ip() {
    let e = env_at(NOW).await;
    make_user(&e.pool, "other", Role::User, true).await;
    // Ten different clients, one failure each: no IP is limited, the username is.
    for i in 1..=10 {
        let s = login_from(&e, &format!("203.0.113.{i}"), USER, "wrong wrong wrong").await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "failure {i}");
    }
    // The right password from a fresh client now waits too (the account is locked, not the IP).
    let s = login_from(&e, "198.51.100.200", USER, PASSWORD).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    // Another account is untouched by it.
    let s = login_from(&e, "198.51.100.201", "other", "wrong wrong wrong").await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // A user name that does not exist is limited the same way (no oracle).
    for i in 1..=10 {
        login_from(&e, &format!("192.0.2.{i}"), "ghost", "wrong wrong wrong").await;
    }
    let s = login_from(&e, "192.0.2.200", "ghost", PASSWORD).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn a_missing_code_counts_as_a_failure() {
    let e = env_at(NOW).await;
    let peer = "203.0.113.7";
    for _ in 0..4 {
        let (s, _, _) = login_at(&e, peer, USER, PASSWORD, None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    // The 5th failure from this client starts the IP lock-out, even with everything right.
    let (s, _, _) = login_at(&e, peer, USER, PASSWORD, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = login_at(&e, peer, USER, PASSWORD, Some(code(&e, TOTP_SECRET, 0))).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
}

// ---- own password and 2FA ---------------------------------------------------------------

#[tokio::test]
async fn me_names_the_user_and_the_role() {
    let e = env_at(NOW).await;
    let cookie = e.login().await;
    let me = e.get(&cookie, "/api/me").await;
    assert_eq!((me["username"].as_str(), me["role"].as_str()), (Some(USER), Some("user")));
    e.add_user("boss", Role::Admin).await;
    let admin = e.login_as("boss").await;
    let me = e.get(&admin, "/api/me").await;
    assert_eq!((me["username"].as_str(), me["role"].as_str()), (Some("boss"), Some("admin")));
}

#[tokio::test]
async fn changing_the_password_needs_the_old_one_and_ends_other_sessions() {
    let e = env_at(NOW).await;
    let first = e.login().await;
    let second = e.login().await;
    let change = |current: &str, new: &str| json!({ "current_password": current, "new_password": new });

    let (s, _) = e.send(&first, Method::POST, "/api/me/password", change("wrong wrong wrong", "another new password")).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = e.send(&first, Method::POST, "/api/me/password", change(PASSWORD, "short")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, h, j) = e.call(Method::POST, "/api/me/password", Some(&first), true, Some(change(PASSWORD, "another new password"))).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    let fresh = cookie_of(&h);
    // Every old session ended, this one is replaced by a fresh one.
    for old in [&first, &second] {
        let (s, _, _) = e.call(Method::GET, "/api/me", Some(old), false, None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    e.get(&fresh, "/api/me").await;
    let (s, _, _) = login(&e, USER, PASSWORD, Some(code(&e, TOTP_SECRET, 1))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = login(&e, USER, "another new password", Some(code(&e, TOTP_SECRET, 1))).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn re_enrolling_2fa_needs_the_password_and_a_code_from_the_new_secret() {
    let e = env_at(NOW).await;
    let cookie = e.login().await;
    let (s, _) = e.send(&cookie, Method::POST, "/api/me/2fa", json!({ "password": "wrong wrong wrong" })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, start) = e.send(&cookie, Method::POST, "/api/me/2fa", json!({ "password": PASSWORD })).await;
    assert_eq!(s, StatusCode::OK);
    let secret = start["secret"].as_str().unwrap().to_owned();
    assert_ne!(secret, TOTP_SECRET);
    // The old secret's code does not confirm the new one.
    let (s, _) = e.send(&cookie, Method::POST, "/api/me/2fa/confirm", json!({ "pending_token": start["pending_token"], "code": code(&e, TOTP_SECRET, 0) })).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _) = e.send(&cookie, Method::POST, "/api/me/2fa/confirm", json!({ "pending_token": start["pending_token"], "code": code(&e, &secret, 0) })).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    // The old secret is dead, the new one logs in (next step: the confirming one is spent).
    let (s, _, _) = login(&e, USER, PASSWORD, Some(code(&e, TOTP_SECRET, 1))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = login(&e, USER, PASSWORD, Some(code(&e, &secret, 1))).await;
    assert_eq!(s, StatusCode::OK);
    // Without a session none of this is reachable.
    let (s, _, _) = e.call(Method::POST, "/api/me/2fa", None, true, Some(json!({ "password": PASSWORD }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn secrets_never_reach_the_logs_or_debug_output() {
    // Debug output of the account types hides hashes and 2FA secrets.
    let e = env_at(NOW).await;
    let user = nimrag_server::users::by_username(&e.pool, USER).await.unwrap().unwrap();
    let dump = format!("{user:?}");
    assert!(!dump.contains(TOTP_SECRET) && !dump.contains("argon2"), "{dump}");
}
