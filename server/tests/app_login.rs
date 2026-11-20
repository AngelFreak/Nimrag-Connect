//! The Android app's sign-in: first login (username, password, 2FA code), later unlock with the
//! password and the device secret, and resume with the device secret alone (the fingerprint gate
//! is on the phone). Revoking the device token kills every credential it carries.

mod common;

use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, Method, StatusCode};
use common::*;
use nimrag_server::tokens;
use nimrag_server::users::Role;
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower::ServiceExt;

const NOW: i64 = 1_800_000_000;

/// The app sends `X-Nimrag: 1` on every call except file uploads, like the web UI.
async fn post_from(e: &Env, peer: &str, uri: &str, body: Value) -> (StatusCode, HeaderMap, Value) {
    let mut req = axum::http::Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-nimrag", "1")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(peer.parse().unwrap(), 40000)));
    let resp = e.app.clone().oneshot(req).await.unwrap();
    let (status, headers) = (resp.status(), resp.headers().clone());
    let bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A POST from a client address nobody used before, so the per-IP limiter never interferes.
async fn app(e: &Env, uri: &str, body: Value) -> (StatusCode, HeaderMap, Value) {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    post_from(e, &format!("198.19.{}.{}", n / 250, n % 250 + 1), uri, body).await
}

/// A fresh, valid 2FA code for `user` (the replay guard is rewound first).
async fn code(e: &Env, user: &str) -> String {
    sqlx::query("UPDATE users SET totp_last_step = 0 WHERE username = ?")
        .bind(user)
        .execute(&e.pool)
        .await
        .unwrap();
    nimrag_server::totp::code_at(TOTP_SECRET, e.now()).unwrap()
}

fn cookie_of(h: &HeaderMap) -> String {
    h.get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

struct Phone {
    device_id: i64,
    token: String,
    secret: String,
    cookie: String,
}

async fn register(e: &Env, user: &str, name: &str) -> Phone {
    let c = code(e, user).await;
    let (s, h, j) = app(
        e,
        "/api/app/login",
        json!({ "username": user, "password": PASSWORD, "code": c, "device_name": name }),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{j}");
    Phone {
        device_id: j["device_id"].as_i64().unwrap(),
        token: j["token"].as_str().unwrap().to_owned(),
        secret: j["device_secret"].as_str().unwrap().to_owned(),
        cookie: cookie_of(&h),
    }
}

async fn alive(e: &Env, cookie: &str) -> bool {
    let (s, _, _) = e
        .call(Method::GET, "/api/me", Some(cookie), false, None)
        .await;
    s == StatusCode::OK
}

async fn count(e: &Env, sql: &str, bind: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .bind(bind)
        .fetch_one(&e.pool)
        .await
        .unwrap()
}

async fn setup() -> Env {
    let e = env_at(NOW).await;
    e.adopt.store(false, std::sync::atomic::Ordering::Relaxed);
    e
}

// ---- first login --------------------------------------------------------------------

#[tokio::test]
async fn first_app_login_registers_the_phone_and_hands_out_credentials_once() {
    let e = setup().await;
    let c = code(&e, USER).await;
    let (s, h, j) = app(
        &e,
        "/api/app/login",
        json!({ "username": "Tester", "password": PASSWORD, "code": c, "device_name": "  Pixel 9a " }),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{j}");
    assert_eq!(j["me"]["username"], USER);
    assert_eq!(j["me"]["role"], "user");
    let token = j["token"].as_str().unwrap();
    let secret = j["device_secret"].as_str().unwrap();
    let id = j["device_id"].as_i64().unwrap();
    assert!(secret.starts_with("nrd_") && secret.len() >= 36, "{secret}");
    assert_ne!(token, secret);
    let cookie = cookie_of(&h);
    assert!(
        alive(&e, &cookie).await,
        "the response carries a working session"
    );

    // Only hashes are stored, and the session is tied to the device token.
    let (name, user, th, sh): (String, i64, String, String) =
        sqlx::query_as("SELECT name, user_id, hash, device_secret_hash FROM tokens WHERE id = ?")
            .bind(id)
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert_eq!((name.as_str(), user), ("Pixel 9a", e.user_id));
    assert_eq!(th, tokens::hash_token(token));
    assert_eq!(sh, tokens::hash_token(secret));
    assert!(th != token && sh != secret);
    for plain in [token, secret] {
        assert_eq!(
            count(
                &e,
                "SELECT COUNT(*) FROM tokens WHERE hash = ? OR device_secret_hash = ?",
                plain
            )
            .await,
            0
        );
    }
    let on_token: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE token_id = ?")
        .bind(id)
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(on_token, 1);

    // The token uploads as that user; the web list shows the phone but no secret.
    let (s, up) = e
        .request(
            Method::POST,
            "/api/files?data_type=4&source=watch",
            Some(token),
            b"from the phone".to_vec(),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{up}");
    let owner: i64 = sqlx::query_scalar("SELECT user_id FROM file_owners")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(owner, e.user_id);
    let web = e.login().await;
    let list = e.get(&web, "/api/tokens").await.to_string();
    assert!(list.contains("Pixel 9a") && !list.contains(secret) && !list.contains(token));
}

#[tokio::test]
async fn app_login_rejects_bad_credentials_alike_and_bad_accounts_for_their_reason() {
    let e = setup().await;
    let good = code(&e, USER).await;
    let wrong_code = if good == "000000" { "000001" } else { "000000" };
    let mut answers = Vec::new();
    for body in [
        json!({ "username": "nobody", "password": PASSWORD, "code": good, "device_name": "p" }),
        json!({ "username": USER, "password": "not the password", "code": good, "device_name": "p" }),
        json!({ "username": USER, "password": PASSWORD, "code": wrong_code, "device_name": "p" }),
        json!({ "username": USER, "password": PASSWORD, "code": "", "device_name": "p" }),
    ] {
        let (s, h, j) = app(&e, "/api/app/login", body).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert!(h.get(header::SET_COOKIE).is_none());
        answers.push(j);
    }
    assert!(answers.windows(2).all(|w| w[0] == w[1]), "{answers:?}");
    assert_eq!(
        count(&e, "SELECT COUNT(*) FROM tokens WHERE name = ?", "p").await,
        0
    );

    // A code works once: a replay is refused.
    let c = code(&e, USER).await;
    let body = json!({ "username": USER, "password": PASSWORD, "code": c, "device_name": "first" });
    assert_eq!(
        app(&e, "/api/app/login", body.clone()).await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        app(&e, "/api/app/login", body).await.0,
        StatusCode::UNAUTHORIZED
    );

    // Device names: 1 to 64 characters.
    for name in ["", "   ", &"x".repeat(65)] {
        let c = code(&e, USER).await;
        let (s, _, _) = app(
            &e,
            "/api/app/login",
            json!({ "username": USER, "password": PASSWORD, "code": c, "device_name": name }),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{name:?}");
    }

    // The app cannot enrol 2FA: an account without it counts as a wrong code.
    make_user(&e.pool, "fresh", Role::User, false).await;
    let (s, _, _) = app(
        &e,
        "/api/app/login",
        json!({ "username": "fresh", "password": PASSWORD, "code": good, "device_name": "p" }),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Admins have no data; users with a temporary password change it on the web first.
    e.add_user("boss", Role::Admin).await;
    let c = code(&e, "boss").await;
    let (s, h, _) = app(
        &e,
        "/api/app/login",
        json!({ "username": "boss", "password": PASSWORD, "code": c, "device_name": "p" }),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(h.get(header::SET_COOKIE).is_none());
    e.exec("UPDATE users SET must_change_password = 1 WHERE username = 'tester'")
        .await;
    let c = code(&e, USER).await;
    let (s, _, _) = app(
        &e,
        "/api/app/login",
        json!({ "username": USER, "password": PASSWORD, "code": c, "device_name": "p" }),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(
        count(&e, "SELECT COUNT(*) FROM tokens WHERE name = ?", "p").await,
        0
    );
}

#[tokio::test]
async fn the_active_token_cap_applies_to_phones_too() {
    let e = setup().await;
    for _ in 1..tokens::MAX_ACTIVE {
        tokens::create(&e.pool, "filler", Some(e.user_id))
            .await
            .unwrap();
    }
    let c = code(&e, USER).await;
    let (s, _, j) = app(
        &e,
        "/api/app/login",
        json!({ "username": USER, "password": PASSWORD, "code": c, "device_name": "one too many" }),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT, "{j}");
    assert_eq!(
        count(
            &e,
            "SELECT COUNT(*) FROM tokens WHERE name = ?",
            "one too many"
        )
        .await,
        0
    );
}

// ---- unlock ---------------------------------------------------------------------------

#[tokio::test]
async fn unlock_needs_the_password_and_this_phones_secret_but_no_code() {
    let e = setup().await;
    let phone = register(&e, USER, "Pixel").await;
    let (s, h, j) = app(
        &e,
        "/api/app/unlock",
        json!({ "username": USER, "password": PASSWORD, "device_secret": phone.secret }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["username"], USER);
    let cookie = cookie_of(&h);
    assert!(alive(&e, &cookie).await);
    let on_token: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE token_id = ?")
        .bind(phone.device_id)
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(on_token, 2, "both app sessions hang on the device token");

    let bea_id = e.add_user("bea", Role::User).await;
    let bea = register(&e, "bea", "Bea's phone").await;
    let wrong: Vec<Value> = vec![
        json!({ "username": USER, "password": "not the password", "device_secret": phone.secret }),
        json!({ "username": USER, "password": PASSWORD, "device_secret": format!("nrd_{}", "a".repeat(64)) }),
        // Another user's phone cannot unlock this account, and the other way round.
        json!({ "username": USER, "password": PASSWORD, "device_secret": bea.secret }),
        json!({ "username": "bea", "password": PASSWORD, "device_secret": phone.secret }),
        // A device token is an upload credential, not a device secret.
        json!({ "username": USER, "password": PASSWORD, "device_secret": phone.token }),
        json!({ "username": "nobody", "password": PASSWORD, "device_secret": phone.secret }),
    ];
    let mut answers = Vec::new();
    for body in wrong {
        let (s, h, j) = app(&e, "/api/app/unlock", body.clone()).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{body}");
        assert!(h.get(header::SET_COOKIE).is_none());
        answers.push(j);
    }
    assert!(answers.windows(2).all(|w| w[0] == w[1]), "{answers:?}");
    // A missing secret is not a login at all.
    let (s, h, _) = app(
        &e,
        "/api/app/unlock",
        json!({ "username": USER, "password": PASSWORD }),
    )
    .await;
    assert!(s.is_client_error() && s != StatusCode::OK);
    assert!(h.get(header::SET_COOKIE).is_none());
    // Bea's own phone still works for her.
    let (s, _, _) = app(
        &e,
        "/api/app/unlock",
        json!({ "username": "bea", "password": PASSWORD, "device_secret": bea.secret }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let _ = bea_id;
}

// ---- resume (fingerprint) ---------------------------------------------------------------

#[tokio::test]
async fn resume_takes_the_device_secret_alone_and_never_a_bare_biometric_claim() {
    let e = setup().await;
    let phone = register(&e, USER, "Pixel").await;
    let (s, h, j) = app(
        &e,
        "/api/app/resume",
        json!({ "device_secret": phone.secret }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["username"], USER);
    assert!(alive(&e, &cookie_of(&h)).await);

    // "The fingerprint matched" is not something the server can check, so it is not a credential.
    for body in [
        json!({ "biometric": true }),
        json!({}),
        json!({ "username": USER, "biometric": true }),
        json!({ "username": USER, "password": PASSWORD }),
    ] {
        let (s, h, _) = app(&e, "/api/app/resume", body.clone()).await;
        assert!(s.is_client_error(), "{body} -> {s}");
        assert!(h.get(header::SET_COOKIE).is_none(), "{body}");
    }
    for secret in [
        "",
        "nrd_",
        &format!("nrd_{}", "0".repeat(64)),
        phone.token.as_str(),
        PASSWORD,
    ] {
        let (s, h, _) = app(
            &e,
            "/api/app/resume",
            json!({ "device_secret": secret, "biometric": true }),
        )
        .await;
        assert!(s.is_client_error(), "{secret:?} -> {s}");
        assert!(h.get(header::SET_COOKIE).is_none());
    }
}

#[tokio::test]
async fn guessing_device_secrets_is_rate_limited_per_address() {
    let e = setup().await;
    let phone = register(&e, USER, "Pixel").await;
    let peer = "203.0.113.50";
    let mut limited_after = None;
    for i in 0..12 {
        let (s, _, _) = post_from(
            &e,
            peer,
            "/api/app/resume",
            json!({ "device_secret": format!("nrd_{i:064}") }),
        )
        .await;
        if s == StatusCode::TOO_MANY_REQUESTS {
            limited_after = Some(i);
            break;
        }
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    let n = limited_after.expect("the limiter must engage");
    assert!(n <= 8, "locked after {n} failures");
    // The guesser's address is locked out even with the right secret ...
    let (s, h, _) = post_from(
        &e,
        peer,
        "/api/app/resume",
        json!({ "device_secret": phone.secret }),
    )
    .await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert!(h.get(header::RETRY_AFTER).is_some());
    // ... while the phone, somewhere else, is not affected.
    let (s, _, _) = post_from(
        &e,
        "203.0.113.51",
        "/api/app/resume",
        json!({ "device_secret": phone.secret }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

// ---- revoking ---------------------------------------------------------------------------

#[tokio::test]
async fn revoking_the_device_token_kills_its_secret_sessions_and_uploads() {
    let e = setup().await;
    let phone = register(&e, USER, "Pixel").await;
    let (s, h, _) = app(
        &e,
        "/api/app/unlock",
        json!({ "username": USER, "password": PASSWORD, "device_secret": phone.secret }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let second = cookie_of(&h);
    let other = register(&e, USER, "Second phone").await;
    assert!(alive(&e, &phone.cookie).await && alive(&e, &second).await);
    let (s, _) = e
        .request(
            Method::POST,
            "/api/files?data_type=4&source=watch",
            Some(&phone.token),
            b"one".to_vec(),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);

    let web = e.login().await;
    let (s, _) = e
        .send(
            &web,
            Method::DELETE,
            &format!("/api/tokens/{}", phone.device_id),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);

    assert!(
        !alive(&e, &phone.cookie).await,
        "the first app session ended"
    );
    assert!(!alive(&e, &second).await, "so did the one made by unlock");
    let (s, _, _) = app(
        &e,
        "/api/app/unlock",
        json!({ "username": USER, "password": PASSWORD, "device_secret": phone.secret }),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = app(
        &e,
        "/api/app/resume",
        json!({ "device_secret": phone.secret }),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = e
        .request(
            Method::POST,
            "/api/files?data_type=4&source=watch",
            Some(&phone.token),
            b"two".to_vec(),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let kept: Option<String> =
        sqlx::query_scalar("SELECT device_secret_hash FROM tokens WHERE id = ?")
            .bind(phone.device_id)
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert!(
        kept.is_none(),
        "the secret's hash is gone, not just ignored"
    );

    // Nothing else was touched: the web session and the other phone work.
    assert!(alive(&e, &web).await);
    assert!(alive(&e, &other.cookie).await);
    let (s, _, _) = app(
        &e,
        "/api/app/resume",
        json!({ "device_secret": other.secret }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn admin_actions_and_own_password_changes_end_every_trusted_phone() {
    let e = setup().await;
    e.add_user("boss", Role::Admin).await;
    let admin = e.login_as("boss").await;
    let uid = e.user_id;

    // Reset 2FA.
    let phone = register(&e, USER, "a").await;
    let (s, _) = e
        .send(
            &admin,
            Method::POST,
            &format!("/api/admin/users/{uid}/reset-2fa"),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(!alive(&e, &phone.cookie).await);
    assert_eq!(
        app(
            &e,
            "/api/app/resume",
            json!({ "device_secret": phone.secret })
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );

    // Re-enrol through the web login so the account is whole again, then reset the password.
    e.exec(&format!(
        "UPDATE users SET totp_secret = '{TOTP_SECRET}', totp_enabled = 1 WHERE id = {uid}"
    ))
    .await;
    let phone = register(&e, USER, "b").await;
    let (s, j) = e
        .send(
            &admin,
            Method::POST,
            &format!("/api/admin/users/{uid}/reset-password"),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(
        app(
            &e,
            "/api/app/resume",
            json!({ "device_secret": phone.secret })
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );

    // Disable (and enable again: the phone must sign in afresh). Put the known password back first.
    e.exec(&format!("UPDATE users SET must_change_password = 0, password_hash = (SELECT password_hash FROM users WHERE username = 'boss') WHERE id = {uid}")).await;
    let phone = register(&e, USER, "c").await;
    let (s, _) = e
        .send(
            &admin,
            Method::POST,
            &format!("/api/admin/users/{uid}/disable"),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(
        app(
            &e,
            "/api/app/resume",
            json!({ "device_secret": phone.secret })
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (s, _) = e
        .send(
            &admin,
            Method::POST,
            &format!("/api/admin/users/{uid}/enable"),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(
        app(
            &e,
            "/api/app/resume",
            json!({ "device_secret": phone.secret })
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (s, _, _) = app(
        &e,
        "/api/app/unlock",
        json!({ "username": USER, "password": PASSWORD, "device_secret": phone.secret }),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // The user changing their own password.
    let phone = register(&e, USER, "d").await;
    let (s, _, j) = e
        .call(
            Method::POST,
            "/api/me/password",
            Some(&phone.cookie),
            true,
            Some(json!({ "current_password": PASSWORD, "new_password": "a brand new passphrase" })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(
        app(
            &e,
            "/api/app/resume",
            json!({ "device_secret": phone.secret })
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn deleting_a_user_unassigns_their_phones_and_ends_their_secrets() {
    let e = setup().await;
    e.add_user("boss", Role::Admin).await;
    let admin = e.login_as("boss").await;
    let phone = register(&e, USER, "Pixel").await;
    let (s, _) = e
        .send(
            &admin,
            Method::DELETE,
            &format!("/api/admin/users/{}?confirm=true", e.user_id),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(
        app(
            &e,
            "/api/app/resume",
            json!({ "device_secret": phone.secret })
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert!(!alive(&e, &phone.cookie).await);
    // The token itself stays, unassigned, for the admin to hand to someone else.
    let (s, _) = e
        .request(
            Method::POST,
            "/api/files?data_type=4&source=watch",
            Some(&phone.token),
            b"x".to_vec(),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    let owners: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM file_owners")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(owners, 0);
}

#[tokio::test]
async fn each_user_sees_and_signs_out_only_their_own_phones() {
    let e = setup().await;
    e.add_user("bea", Role::User).await;
    let mine = register(&e, USER, "Pixel 9a").await;
    let hers = register(&e, "bea", "Galaxy").await;
    let (me, her) = (e.login().await, e.login_as("bea").await);

    // The list names the phone and carries no secret, and only the user's own phones are in it.
    let list = e.get(&me, "/api/tokens").await;
    let text = list.to_string();
    assert!(
        text.contains("Pixel 9a") && !text.contains("Galaxy"),
        "{text}"
    );
    for secret in [&mine.token, &mine.secret, &hers.token, &hers.secret] {
        assert!(!text.contains(secret.as_str()));
    }
    let her_list = e.get(&her, "/api/tokens").await.to_string();
    assert!(her_list.contains("Galaxy") && !her_list.contains("Pixel 9a"));

    // Signing out someone else's phone is "not found" and changes nothing.
    let (s, _) = e
        .send(
            &me,
            Method::DELETE,
            &format!("/api/tokens/{}", hers.device_id),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = app(
        &e,
        "/api/app/resume",
        json!({ "device_secret": hers.secret }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    // Signing out one's own phone ends it.
    let (s, _) = e
        .send(
            &me,
            Method::DELETE,
            &format!("/api/tokens/{}", mine.device_id),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _, _) = app(
        &e,
        "/api/app/resume",
        json!({ "device_secret": mine.secret }),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_phone_list_marks_the_phone_this_session_came_from() {
    let e = setup().await;
    let first = register(&e, USER, "Pixel 9a").await;
    let second = register(&e, USER, "Pixel 9a").await;
    let current = |list: &Value| -> Vec<i64> {
        list["tokens"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["current"] == true)
            .map(|t| t["id"].as_i64().unwrap())
            .collect()
    };

    // Seen from a phone's web view, exactly that phone is "this phone".
    assert_eq!(
        current(&e.get(&first.cookie, "/api/tokens").await),
        vec![first.device_id]
    );
    assert_eq!(
        current(&e.get(&second.cookie, "/api/tokens").await),
        vec![second.device_id]
    );

    // A browser session has no phone behind it: nothing is marked, but every phone is listed.
    let web = e.login().await;
    let list = e.get(&web, "/api/tokens").await;
    let ids: Vec<i64> = list["tokens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_i64().unwrap())
        .collect();
    assert!(ids.contains(&first.device_id) && ids.contains(&second.device_id));
    assert!(current(&list).is_empty(), "{list}");
}
