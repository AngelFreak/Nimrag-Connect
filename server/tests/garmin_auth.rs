//! Forward mode: login, MFA, token refresh, token import, API auth and secrets.
//! Every test talks to the fake Garmin in `garmin_fake`; nothing reaches Garmin.

mod common;
mod garmin_fake;

use axum::http::{Method, StatusCode};
use garmin_fake::*;
use serde_json::json;

fn login_body() -> serde_json::Value {
    json!({"email": EMAIL, "password": GARMIN_PASSWORD})
}

#[tokio::test]
async fn login_success_stores_tokens_and_hides_them() {
    let r = rig().await;
    assert_eq!(r.status().await["state"], "off");

    let (s, j) = r.post("/api/garmin/login", login_body()).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["mfa_required"], false);

    // The credential POST carried the cookie from the sign-in page.
    r.fake.with(|f| {
        assert_eq!(
            f.requests[..2],
            [
                "GET /mobile/sso/en_US/sign-in".to_owned(),
                "POST /mobile/api/login".to_owned()
            ]
        );
        assert!(f.cookies_seen_on_login[0].contains("SESSION=fake-sso-session"));
        assert_eq!(f.profile_calls, 1, "the token is checked once");
    });

    let st = r.status().await;
    assert_eq!(st["state"], "connected");
    assert_eq!(st["pending"], 0);
    assert!(st["last_error"].is_null());

    let (access, refresh, cid) = r.stored_tokens().await;
    assert_eq!(cid, "GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2");
    assert!(access.contains('.') && refresh.starts_with("refresh-"));
    // Neither the response nor the status carries a secret.
    let body = format!("{j} {st}");
    for secret in [access.as_str(), refresh.as_str(), GARMIN_PASSWORD] {
        assert!(!body.contains(secret));
    }
}

#[tokio::test]
async fn login_with_mfa_needs_a_code_and_survives_a_wrong_one() {
    let r = rig().await;
    r.fake.with(|f| f.mfa = true);

    let (s, j) = r.post("/api/garmin/login", login_body()).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["mfa_required"], true);
    assert_eq!(r.status().await["state"], "needs_mfa");
    assert_eq!(r.queue_len().await, 0);

    // Wrong code: 422, and the same pending login still accepts the right one.
    let (s, j) = r.post("/api/garmin/mfa", json!({"code": "000000"})).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{j}");
    assert_eq!(r.status().await["state"], "needs_mfa");

    let (s, j) = r.post("/api/garmin/mfa", json!({"code": MFA_CODE})).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["state"], "connected");
    r.fake.with(|f| {
        let n = f
            .requests
            .iter()
            .filter(|q| q.contains("verifyCode"))
            .count();
        assert_eq!(n, 2);
    });
    assert!(r.stored_tokens().await.0.contains('.'));
}

#[tokio::test]
async fn pending_mfa_expires_after_ten_minutes() {
    let r = rig().await;
    r.fake.with(|f| f.mfa = true);
    r.post("/api/garmin/login", login_body()).await;
    assert_eq!(r.status().await["state"], "needs_mfa");

    r.advance(9 * 60);
    assert_eq!(r.status().await["state"], "needs_mfa");
    r.advance(61);
    assert_eq!(r.status().await["state"], "off");
    let (s, _) = r.post("/api/garmin/mfa", json!({"code": MFA_CODE})).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn wrong_password_is_a_422_not_a_session_loss() {
    let r = rig().await;
    let (s, j) = r
        .post(
            "/api/garmin/login",
            json!({"email": EMAIL, "password": "not-the-password"}),
        )
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{j}");
    assert!(j["error"].as_str().unwrap().contains("email or password"));
    assert!(!j.to_string().contains("not-the-password"));
    let st = r.status().await;
    assert_eq!(st["state"], "off");
    // Still signed in to nimrag itself.
    r.env.get(&r.cookie, "/api/me").await;
}

#[tokio::test]
async fn blocked_and_rate_limited_logins_say_what_to_do() {
    let r = rig().await;
    r.fake.with(|f| f.page_status = Some(403));
    let (s, j) = r.post("/api/garmin/login", login_body()).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY, "{j}");
    let msg = j["error"].as_str().unwrap();
    assert!(msg.contains("import a token file"), "{msg}");

    r.fake.with(|f| {
        f.page_status = None;
        f.login_status = Some(429);
    });
    let (s, _) = r.post("/api/garmin/login", login_body()).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r.status().await["state"], "off");
}

#[tokio::test]
async fn exchange_tries_the_next_client_id() {
    let r = rig().await;
    r.fake.with(|f| f.di_reject_first = 1);
    r.connect().await;
    let (_, _, cid) = r.stored_tokens().await;
    assert_eq!(cid, "GARMIN_CONNECT_MOBILE_ANDROID_DI_2024Q4");
}

#[tokio::test]
async fn refresh_rotation_is_stored() {
    let r = rig().await;
    r.connect().await;
    let (old_access, old_refresh, _) = r.stored_tokens().await;
    r.activity(1).await;

    // The access token is within 15 minutes of expiry: the worker refreshes first.
    r.advance(garmin_fake::TOKEN_TTL - 600);
    assert_eq!(r.run().await, 1);

    let (new_access, new_refresh, _) = r.stored_tokens().await;
    assert_ne!(new_access, old_access);
    assert_ne!(new_refresh, old_refresh, "the refresh token rotated");
    r.fake.with(|f| {
        assert_eq!(f.refresh_calls, 1);
        assert_eq!(f.uploads.len(), 1);
        assert_eq!(f.uploads[0].0, format!("Bearer {new_access}"));
        assert!(!f.refresh_valid.contains(&old_refresh));
    });
    assert_eq!(r.status().await["done"], 1);

    // The new refresh token keeps working.
    r.advance(garmin_fake::TOKEN_TTL);
    r.activity(2).await;
    assert_eq!(r.run().await, 1);
    assert_eq!(r.fake.with(|f| f.refresh_calls), 2);
    assert_eq!(r.status().await["done"], 2);
}

#[tokio::test]
async fn refresh_without_rotation_keeps_the_old_refresh_token() {
    let r = rig().await;
    r.fake.with(|f| f.rotate_refresh = false);
    r.connect().await;
    let (_, old_refresh, _) = r.stored_tokens().await;
    r.activity(1).await;
    r.advance(garmin_fake::TOKEN_TTL - 100);
    r.run().await;
    let (_, refresh, _) = r.stored_tokens().await;
    assert_eq!(refresh, old_refresh);
    assert_eq!(r.status().await["done"], 1);
}

#[tokio::test]
async fn refused_refresh_token_needs_a_new_login() {
    let r = rig().await;
    r.connect().await;
    r.activity(1).await;
    r.fake.with(|f| f.refresh_valid.clear());
    r.advance(garmin_fake::TOKEN_TTL);
    assert_eq!(r.run().await, 1);
    let st = r.status().await;
    assert_eq!(st["state"], "needs_login");
    assert_eq!(st["pending"], 1, "the file waits for the next login");
    assert!(st["last_error"].as_str().unwrap().contains("log in again"));
    // The dead tokens are gone.
    let (a, rt, _) = r.stored_tokens().await;
    assert!(a.is_empty() && rt.is_empty());
    assert_eq!(r.fake.upload_count(), 0);

    // Logging in again resumes the queue.
    r.connect().await;
    assert_eq!(r.status().await["state"], "connected");
    assert_eq!(r.run().await, 1);
    assert_eq!(r.status().await["done"], 1);
}

// ---- token import ---------------------------------------------------------------------

#[tokio::test]
async fn import_tokens_validates_and_stores() {
    let r = rig().await;
    let (access, refresh) = r.fake.mint(3600);
    let (s, j) = r
        .post(
            "/api/garmin/import-tokens",
            json!({"di_token": access, "di_refresh_token": refresh,
                   "di_client_id": "GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["state"], "connected");
    let stored = r.stored_tokens().await;
    assert_eq!(stored.0, access);
    assert_eq!(stored.1, refresh);
    assert!(!j.to_string().contains(&access) && !j.to_string().contains(&refresh));

    // Uploads work with the imported tokens.
    r.activity(1).await;
    assert_eq!(r.run().await, 1);
    assert_eq!(r.status().await["done"], 1);
}

#[tokio::test]
async fn import_tokens_reads_the_client_id_and_refreshes_an_expired_access_token() {
    let r = rig().await;
    let (access, refresh) = r.fake.mint(3600);
    // The python file may lack di_client_id; the access token's claim names it.
    r.fake.revoke_access(); // the access token is no longer accepted
    let (s, j) = r
        .post(
            "/api/garmin/import-tokens",
            json!({"di_token": access, "di_refresh_token": refresh}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    let (new_access, new_refresh, cid) = r.stored_tokens().await;
    assert_eq!(cid, "GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2");
    assert_ne!(
        new_access, access,
        "the stored access token is the refreshed one"
    );
    assert_ne!(new_refresh, refresh);
    assert_eq!(r.fake.with(|f| f.refresh_calls), 1);
}

#[tokio::test]
async fn import_tokens_rejects_bad_files_and_tokens_garmin_refuses() {
    let r = rig().await;
    for (body, why) in [
        (json!({"di_token": "", "di_refresh_token": "x"}), "empty"),
        (
            json!({"di_token": "not-a-jwt", "di_refresh_token": "x", "di_client_id": "c"}),
            "not a JWT",
        ),
        (json!({"di_refresh_token": "x"}), "missing field"),
    ] {
        let (s, _) = r.post("/api/garmin/import-tokens", body).await;
        assert!(
            s == StatusCode::UNPROCESSABLE_ENTITY || s == StatusCode::BAD_REQUEST,
            "{why}: {s}"
        );
    }
    // Well-formed but unknown to Garmin: 401 on the check, refresh refused.
    let forged = make_jwt(START + 3600, "GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2", 99);
    let (s, j) = r
        .post(
            "/api/garmin/import-tokens",
            json!({"di_token": forged, "di_refresh_token": "nope"}),
        )
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{j}");
    assert_eq!(r.status().await["state"], "off");
    assert!(!j.to_string().contains(&forged));
}

// ---- API surface ------------------------------------------------------------------------

#[tokio::test]
async fn endpoints_need_the_session_cookie_and_the_csrf_header() {
    let r = rig().await;
    let writes = [
        ("/api/garmin/login", login_body()),
        ("/api/garmin/mfa", json!({"code": "1"})),
        ("/api/garmin/logout", json!({})),
        ("/api/garmin/retry", json!({})),
        ("/api/garmin/backfill", json!({"days": 3})),
        (
            "/api/garmin/import-tokens",
            json!({"di_token": "a", "di_refresh_token": "b"}),
        ),
    ];
    // No cookie: 401 (GET and every write), no Garmin traffic.
    let (s, _, _) = r
        .env
        .call(Method::GET, "/api/garmin", None, false, None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // A device token cannot read.
    let (s, _) = r
        .env
        .request(Method::GET, "/api/garmin", Some(&r.env.token), vec![])
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    for (uri, body) in &writes {
        let (s, _, _) = r
            .env
            .call(Method::POST, uri, None, true, Some(body.clone()))
            .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{uri} without a cookie");
        // Cookie but no X-Nimrag header: 403 before anything runs.
        let (s, _, _) = r
            .env
            .call(
                Method::POST,
                uri,
                Some(&r.cookie),
                false,
                Some(body.clone()),
            )
            .await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{uri} without the CSRF header");
    }
    assert!(
        r.fake.with(|f| f.requests.is_empty()),
        "nothing reached Garmin"
    );
    assert_eq!(r.status().await["state"], "off");
}

#[tokio::test]
async fn login_input_is_validated() {
    let r = rig().await;
    for body in [
        json!({"email": "", "password": "x"}),
        json!({"email": EMAIL, "password": ""}),
        json!({"email": EMAIL}),
    ] {
        let (s, _) = r.post("/api/garmin/login", body).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
    assert!(r.fake.with(|f| f.requests.is_empty()));
}

#[tokio::test]
async fn logout_forgets_tokens_and_unsent_files_but_keeps_done_rows() {
    let r = rig().await;
    r.connect().await;
    let sent = r.activity(1).await;
    r.run().await;
    let waiting = r.activity(2).await;
    assert_eq!(r.status().await["pending"], 1);

    let (s, _) = r.post("/api/garmin/logout", json!({})).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let st = r.status().await;
    assert_eq!(st["state"], "off");
    assert_eq!(
        (st["done"].as_i64(), st["pending"].as_i64()),
        (Some(1), Some(0))
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM garmin_account")
        .fetch_one(&r.env.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    assert_eq!(r.item(&sent).await.0, "done");
    let gone =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM garmin_uploads WHERE file_sha256 = ?")
            .bind(&waiting)
            .fetch_one(&r.env.pool)
            .await
            .unwrap();
    assert_eq!(gone, 0);
    // With Forward off a new activity is not queued.
    r.activity(3).await;
    assert_eq!(r.queue_len().await, 1);
}

// ---- secrets ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

#[tokio::test]
async fn secrets_never_reach_the_logs_the_database_or_debug_output() {
    let cap = Capture::default();
    let _guard = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(cap.clone())
            .finish(),
    );
    let r = rig().await;
    r.fake.with(|f| f.mfa = true);
    r.post("/api/garmin/login", login_body()).await;
    r.post("/api/garmin/mfa", json!({"code": MFA_CODE})).await;
    r.activity(1).await;
    r.fake.script([Up::Status(500), Up::Ok(5)]);
    r.run().await; // 500: a transient failure is logged
    r.advance(61);
    r.revoke_then_refresh().await;
    let (access, refresh, _) = r.stored_tokens().await;

    let logs = String::from_utf8(cap.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("garmin"), "the worker does log: {logs}");
    for secret in [
        GARMIN_PASSWORD,
        MFA_CODE,
        access.as_str(),
        refresh.as_str(),
        "refresh-",
        "Bearer",
    ] {
        assert!(!logs.contains(secret), "{secret:?} leaked into the logs");
    }

    // The password and the MFA code are never stored anywhere in the data directory.
    for bytes in all_bytes(r.env.dir.path()) {
        assert!(!contains(&bytes, GARMIN_PASSWORD));
        assert!(!contains(&bytes, MFA_CODE));
    }

    // Debug output of everything that holds a secret is redacted.
    let tokens = nimrag_server::garmin::auth::Tokens {
        di_token: access.clone(),
        di_refresh_token: refresh.clone(),
        di_client_id: "CID".into(),
    };
    let account = nimrag_server::garmin::queue::load_account(&r.env.pool)
        .await
        .unwrap();
    let req: nimrag_server::garmin::api::GarminLoginRequest =
        serde_json::from_value(login_body()).unwrap();
    let code: nimrag_server::garmin::api::GarminMfaRequest =
        serde_json::from_value(json!({"code": MFA_CODE})).unwrap();
    let imp: nimrag_server::garmin::api::GarminImportTokens =
        serde_json::from_value(json!({"di_token": access, "di_refresh_token": refresh})).unwrap();
    let dump = format!("{tokens:?} {account:?} {req:?} {code:?} {imp:?}");
    for secret in [
        GARMIN_PASSWORD,
        MFA_CODE,
        access.as_str(),
        refresh.as_str(),
        EMAIL,
    ] {
        assert!(!dump.contains(secret), "{secret:?} in Debug output: {dump}");
    }

    // The status endpoint is secret-free too.
    let st = r.status().await.to_string();
    for secret in [access.as_str(), refresh.as_str(), GARMIN_PASSWORD] {
        assert!(!st.contains(secret));
    }
}

impl Rig {
    /// Make Garmin refuse the current access token once so the worker refreshes (and logs).
    async fn revoke_then_refresh(&self) {
        self.fake.revoke_access();
        self.activity(2).await;
        self.run().await;
    }
}
