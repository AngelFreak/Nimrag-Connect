mod common;

use axum::http::{header, Method, StatusCode};
use common::*;
use serde_json::json;

fn login_body(pw: &str) -> Option<serde_json::Value> {
    Some(json!({ "password": pw }))
}

#[tokio::test]
async fn login_sets_a_hardened_cookie_and_me_works() {
    let e = env_at(1_800_000_000).await;
    nimrag_server::auth::set_password(&e.pool, PASSWORD, 0)
        .await
        .unwrap();
    let (s, h, j) = e
        .call(
            Method::POST,
            "/api/login",
            None,
            false,
            login_body(PASSWORD),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(j["authenticated"], true);
    let set = h
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    for flag in [
        "nimrag_session=",
        "HttpOnly",
        "SameSite=Strict",
        "Path=/",
        "Max-Age=2592000",
        "Secure",
    ] {
        assert!(set.contains(flag), "{set} lacks {flag}");
    }
    let cookie = set.split(';').next().unwrap();
    let (s, _, me) = e
        .call(Method::GET, "/api/me", Some(cookie), false, None)
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(me["timezone"].is_string());
    assert_eq!(me["sample"], false);

    // Only the SHA-256 of the session id is stored.
    let id = cookie.trim_start_matches("nimrag_session=");
    let stored: String = sqlx::query_scalar("SELECT id_hash FROM sessions")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_ne!(stored, id);
    assert_eq!(stored, nimrag_server::sha256_hex(id.as_bytes()));
    assert_eq!(id.len(), 64);
}

#[tokio::test]
async fn insecure_cookies_flag_drops_secure_only() {
    let dir = tempfile::tempdir().unwrap();
    let pool = nimrag_server::db::open(dir.path()).await.unwrap();
    nimrag_server::auth::set_password(&pool, PASSWORD, 0)
        .await
        .unwrap();
    let app = nimrag_server::api::router(
        nimrag_server::api::AppState::new(pool, dir.path().to_path_buf(), 1 << 20)
            .with_insecure_cookies(true),
    );
    let e = Env {
        dir,
        pool: sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap(),
        app,
        token: String::new(),
    };
    let (s, h, _) = e
        .call(
            Method::POST,
            "/api/login",
            None,
            false,
            login_body(PASSWORD),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let set = h.get(header::SET_COOKIE).unwrap().to_str().unwrap();
    assert!(!set.contains("Secure"));
    assert!(set.contains("HttpOnly") && set.contains("SameSite=Strict"));
}

#[tokio::test]
async fn wrong_password_and_missing_owner() {
    let e = env_at(1_800_000_000).await;
    // No owner yet.
    let (s, _, _) = e
        .call(
            Method::POST,
            "/api/login",
            None,
            false,
            login_body("anything at all"),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    nimrag_server::auth::set_password(&e.pool, PASSWORD, 0)
        .await
        .unwrap();
    let (s, h, _) = e
        .call(
            Method::POST,
            "/api/login",
            None,
            false,
            login_body("not the password"),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(h.get(header::SET_COOKIE).is_none());
    let (s, _, _) = e
        .call(
            Method::POST,
            "/api/login",
            None,
            false,
            Some(json!({ "nope": 1 })),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Short passwords are refused at set time.
    assert!(nimrag_server::auth::set_password(&e.pool, "short", 0)
        .await
        .is_err());
}

#[tokio::test]
async fn login_is_rate_limited_and_recovers() {
    let e = env_at(1_800_000_000).await;
    nimrag_server::auth::set_password(&e.pool, PASSWORD, 0)
        .await
        .unwrap();
    for _ in 0..4 {
        let (s, _, _) = e
            .call(
                Method::POST,
                "/api/login",
                None,
                false,
                login_body("wrong wrong wrong"),
            )
            .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    // The 5th failure starts the lock-out; even the right password is refused now.
    let (s, _, _) = e
        .call(
            Method::POST,
            "/api/login",
            None,
            false,
            login_body("wrong wrong wrong"),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, h, _) = e
        .call(
            Method::POST,
            "/api/login",
            None,
            false,
            login_body(PASSWORD),
        )
        .await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert!(
        h.get(header::RETRY_AFTER)
            .unwrap()
            .to_str()
            .unwrap()
            .parse::<i64>()
            .unwrap()
            >= 1
    );
}

#[tokio::test]
async fn every_read_needs_a_session_and_device_tokens_cannot_read() {
    let e = env_at(cph(2026, 11, 19, 20, 0)).await;
    let cookie = e.login().await;
    for path in [
        "/api/me",
        "/api/tokens",
        "/api/today",
        "/api/days/2026-11-19",
        "/api/activities",
        "/api/sleep/2026-11-18",
        "/api/sleep?nights=7",
        "/api/trends?range=7d",
        "/api/movement/2026-11-19",
        "/api/training",
        "/api/body",
        "/api/calendar/2026-11",
        "/api/devices",
        "/api/storage",
        "/api/settings",
        "/api/activities/0000000000000000000000000000000000000000000000000000000000000000",
        "/api/activities/0000000000000000000000000000000000000000000000000000000000000000/track",
        "/api/activities/0000000000000000000000000000000000000000000000000000000000000000/strength",
    ] {
        let (s, _, _) = e.call(Method::GET, path, None, false, None).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{path} without a session");
        // A device token (Bearer) is not a session.
        let req = axum::http::Request::builder()
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {}", e.token))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = tower::ServiceExt::oneshot(e.app.clone(), req)
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "{path} with a device token"
        );
        // With the session it is not 401 (404 for the unknown activity id).
        let (s, _, _) = e.call(Method::GET, path, Some(&cookie), false, None).await;
        assert!(
            s == StatusCode::OK || s == StatusCode::NOT_FOUND,
            "{path}: {s}"
        );
    }
    // A forged cookie is no session either.
    let (s, _, _) = e
        .call(
            Method::GET,
            "/api/me",
            Some("nimrag_session=deadbeef"),
            false,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn csrf_header_is_required_for_changes_except_upload_and_login() {
    let e = env_at(cph(2026, 11, 19, 20, 0)).await;
    let cookie = e.login().await;
    // Signed in but without X-Nimrag: refused, nothing changes.
    let (s, _, _) = e
        .call(
            Method::POST,
            "/api/tokens",
            Some(&cookie),
            false,
            Some(json!({ "name": "x" })),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, _) = e
        .call(
            Method::PATCH,
            "/api/settings",
            Some(&cookie),
            false,
            Some(json!({ "step_goal": 9000 })),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, _) = e
        .call(Method::DELETE, "/api/tokens/1", Some(&cookie), false, None)
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, _) = e
        .call(Method::POST, "/api/logout", Some(&cookie), false, None)
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tokens")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    // A wrong header value does not count.
    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/api/tokens")
        .header(header::COOKIE, &cookie)
        .header("x-nimrag", "0")
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(r#"{"name":"x"}"#))
        .unwrap();
    let resp = tower::ServiceExt::oneshot(e.app.clone(), req)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    // Device upload and login are exempt (login is covered above); upload needs only the Bearer token.
    let (s, _) = e.upload("data_type=4", b"abc").await;
    assert_eq!(s, StatusCode::CREATED);
    // With the header it works.
    let (s, _) = e
        .send(
            &cookie,
            Method::PATCH,
            "/api/settings",
            json!({ "step_goal": 9000 }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn logout_ends_the_session() {
    let e = env_at(cph(2026, 11, 19, 20, 0)).await;
    let cookie = e.login().await;
    let (s, h, _) = e
        .call(Method::POST, "/api/logout", Some(&cookie), true, None)
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(h
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .contains("Max-Age=0"));
    let (s, _, _) = e
        .call(Method::GET, "/api/me", Some(&cookie), false, None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sessions_expire_after_30_days() {
    let e = env_at(1_800_000_000).await;
    let cookie = e.login().await;
    e.exec("UPDATE sessions SET expires_at = 1799999999").await;
    let (s, _, _) = e
        .call(Method::GET, "/api/me", Some(&cookie), false, None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn token_management_never_leaks_secrets_and_revokes() {
    let e = env_at(cph(2026, 11, 19, 20, 0)).await;
    let cookie = e.login().await;
    let (s, created) = e
        .send(
            &cookie,
            Method::POST,
            "/api/tokens",
            json!({ "name": "phone" }),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    let secret = created["token"].as_str().unwrap().to_owned();
    assert!(secret.starts_with("nrg_"));
    let id = created["id"].as_i64().unwrap();

    let list = e.get(&cookie, "/api/tokens").await;
    let text = list.to_string();
    assert!(!text.contains(&secret) && !text.contains("hash") && !text.contains("\"token\""));
    let me = list["tokens"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == id)
        .unwrap();
    assert_eq!(me["name"], "phone");
    assert_eq!(me["active"], true);

    // The new token uploads (no CSRF header, no cookie) but cannot read.
    let up = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/api/files?source=watch")
        .header(header::AUTHORIZATION, format!("Bearer {secret}"))
        .body(axum::body::Body::from("xyz"))
        .unwrap();
    let resp = tower::ServiceExt::oneshot(e.app.clone(), up).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let (s, _, _) = e.call(Method::GET, "/api/tokens", None, false, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let (s, _) = e
        .send(
            &cookie,
            Method::POST,
            "/api/tokens",
            json!({ "name": "  " }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, _) = e
        .send(
            &cookie,
            Method::DELETE,
            &format!("/api/tokens/{id}"),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let up = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/api/files?source=watch")
        .header(header::AUTHORIZATION, format!("Bearer {secret}"))
        .body(axum::body::Body::from("xyz2"))
        .unwrap();
    let resp = tower::ServiceExt::oneshot(e.app.clone(), up).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let list = e.get(&cookie, "/api/tokens").await;
    let me = list["tokens"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == id)
        .unwrap();
    assert_eq!(me["active"], false);
    assert!(me["revoked_at"].is_string());
    let (s, _) = e
        .send(
            &cookie,
            Method::DELETE,
            &format!("/api/tokens/{id}"),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
