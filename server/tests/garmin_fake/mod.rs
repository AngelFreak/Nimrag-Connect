//! A fake Garmin for the Forward tests: one axum server on 127.0.0.1:0 that
//! plays the SSO, the DI token service and connectapi. No test touches the real
//! Garmin; every URL of the server under test points here.
#![allow(dead_code)]

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    Router,
};
use base64::Engine;
use nimrag_server::{api, db, garmin::GarminConfig, tokens, users::Role};
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

pub const START: i64 = 1_800_000_000;
pub const EMAIL: &str = "owner@example.test";
pub const GARMIN_PASSWORD: &str = "garmin-pw-Zx81-secret";
pub const MFA_CODE: &str = "123456";
pub const TOKEN_TTL: i64 = 3600;

/// What the fake answers to the next upload.
#[derive(Clone, Debug)]
pub enum Up {
    Ok(i64),
    OkNoId,
    Dup,
    Status(u16),
    Retry(u16, u64),
    /// 409 that is not the duplicate answer.
    OtherConflict,
}

pub struct FakeState {
    pub clock: Arc<AtomicI64>,
    pub mfa: bool,
    /// Answer the sign-in page with this status instead of 200.
    pub page_status: Option<u16>,
    /// Answer the credential POST with this status instead of the normal flow.
    pub login_status: Option<u16>,
    /// Reject this many DI client ids before accepting one.
    pub di_reject_first: usize,
    pub rotate_refresh: bool,
    /// Tokens issued from now on are not accepted by connectapi.
    pub refuse_new_tokens: bool,
    pub refresh_status: Option<u16>,
    pub counter: u32,
    pub tickets: HashSet<String>,
    pub access_valid: HashSet<String>,
    pub refresh_valid: HashSet<String>,
    pub script: VecDeque<Up>,
    pub default_upload: Up,
    pub next_activity_id: i64,
    /// Bodies of the uploads that reached the upload endpoint, with the bearer used.
    pub uploads: Vec<(String, Vec<u8>)>,
    pub requests: Vec<String>,
    pub refresh_calls: u32,
    pub profile_calls: u32,
    /// Cookies that arrived on the credential POST.
    pub cookies_seen_on_login: Vec<String>,
}

#[derive(Clone)]
pub struct FakeGarmin {
    pub url: String,
    pub st: Arc<Mutex<FakeState>>,
}

fn b64(v: &Value) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string())
}

pub fn make_jwt(exp: i64, client_id: &str, jti: u32) -> String {
    format!(
        "{}.{}.fakesig",
        b64(&json!({"alg": "RS256"})),
        b64(&json!({"exp": exp, "client_id": client_id, "jti": jti}))
    )
}

impl FakeState {
    fn now(&self) -> i64 {
        self.clock.load(Ordering::SeqCst)
    }

    fn issue(&mut self, client_id: &str) -> (String, Option<String>) {
        self.counter += 1;
        let access = make_jwt(self.now() + TOKEN_TTL, client_id, self.counter);
        if !self.refuse_new_tokens {
            self.access_valid.insert(access.clone());
        }
        (access, None)
    }
}

impl FakeGarmin {
    pub async fn start(clock: Arc<AtomicI64>) -> Self {
        let st = Arc::new(Mutex::new(FakeState {
            clock,
            mfa: false,
            page_status: None,
            login_status: None,
            di_reject_first: 0,
            rotate_refresh: true,
            refuse_new_tokens: false,
            refresh_status: None,
            counter: 0,
            tickets: HashSet::new(),
            access_valid: HashSet::new(),
            refresh_valid: HashSet::new(),
            script: VecDeque::new(),
            default_upload: Up::Ok(1000),
            next_activity_id: 1000,
            uploads: vec![],
            requests: vec![],
            refresh_calls: 0,
            profile_calls: 0,
            cookies_seen_on_login: vec![],
        }));
        let app = Router::new().fallback(handler).with_state(st.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { url, st }
    }

    pub fn config(&self) -> GarminConfig {
        GarminConfig::for_fake(&self.url)
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut FakeState) -> R) -> R {
        f(&mut self.st.lock().unwrap())
    }

    pub fn script(&self, ups: impl IntoIterator<Item = Up>) {
        self.with(|s| s.script.extend(ups));
    }

    /// Every access token stops working (401 on connectapi); refresh tokens still do.
    pub fn revoke_access(&self) {
        self.with(|s| s.access_valid.clear());
    }

    /// Mint a token set as if the login had happened elsewhere (for imports).
    pub fn mint(&self, exp_in: i64) -> (String, String) {
        self.with(|s| {
            s.counter += 1;
            let access = make_jwt(
                s.now() + exp_in,
                "GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2",
                s.counter,
            );
            s.access_valid.insert(access.clone());
            let refresh = format!("refresh-minted-{}", s.counter);
            s.refresh_valid.insert(refresh.clone());
            (access, refresh)
        })
    }

    pub fn upload_count(&self) -> usize {
        self.with(|s| s.uploads.len())
    }
}

fn json_resp(status: u16, v: Value) -> Response {
    (
        StatusCode::from_u16(status).unwrap(),
        [("content-type", "application/json")],
        v.to_string(),
    )
        .into_response()
}

fn cookie_ok(h: &HeaderMap) -> bool {
    h.get("cookie")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|c| c.contains("SESSION=fake-sso-session"))
}

fn form_field(body: &[u8], key: &str) -> Option<String> {
    let s = std::str::from_utf8(body).ok()?;
    s.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| {
            // minimal percent-decoding
            let b = v.as_bytes();
            let mut out = Vec::new();
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'%' && i + 2 < b.len() {
                    if let Ok(x) = u8::from_str_radix(&v[i + 1..i + 3], 16) {
                        out.push(x);
                        i += 3;
                        continue;
                    }
                }
                out.push(b[i]);
                i += 1;
            }
            String::from_utf8_lossy(&out).into_owned()
        })
    })
}

async fn handler(
    State(st): State<Arc<Mutex<FakeState>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = uri.path().to_owned();
    let mut s = st.lock().unwrap();
    s.requests.push(format!("{method} {path}"));
    match (method.as_str(), path.as_str()) {
        ("GET", "/mobile/sso/en_US/sign-in") => {
            if let Some(code) = s.page_status {
                return json_resp(code, json!({}));
            }
            let mut r = (StatusCode::OK, "<html>sign in</html>").into_response();
            r.headers_mut().insert(
                "set-cookie",
                HeaderValue::from_static("SESSION=fake-sso-session; Path=/; HttpOnly"),
            );
            r
        }
        ("POST", "/mobile/api/login") => {
            let cookie = headers
                .get("cookie")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_owned();
            s.cookies_seen_on_login.push(cookie);
            if let Some(code) = s.login_status {
                return json_resp(code, json!({"error": {"status-code": code.to_string()}}));
            }
            // Without the sign-in page's cookies Garmin answers 429.
            if !cookie_ok(&headers) {
                return json_resp(429, json!({"error": {"status-code": "429"}}));
            }
            let v: Value = serde_json::from_slice(&body).unwrap_or_default();
            let ok = v["username"] == EMAIL && v["password"] == GARMIN_PASSWORD;
            if !ok {
                return json_resp(
                    401,
                    json!({"responseStatus": {"type": "INVALID_USERNAME_PASSWORD"}}),
                );
            }
            if s.mfa {
                return json_resp(
                    200,
                    json!({"responseStatus": {"type": "MFA_REQUIRED"},
                           "customerMfaInfo": {"mfaLastMethodUsed": "email"}}),
                );
            }
            s.counter += 1;
            let ticket = format!("ST-{}", s.counter);
            s.tickets.insert(ticket.clone());
            json_resp(
                200,
                json!({"responseStatus": {"type": "SUCCESSFUL"}, "serviceTicketId": ticket}),
            )
        }
        ("POST", "/mobile/api/mfa/verifyCode") => {
            if !cookie_ok(&headers) {
                return json_resp(429, json!({"error": {"status-code": "429"}}));
            }
            let v: Value = serde_json::from_slice(&body).unwrap_or_default();
            if v["mfaVerificationCode"] != MFA_CODE || v["mfaMethod"] != "email" {
                return json_resp(200, json!({"responseStatus": {"type": "MFA_CODE_INVALID"}}));
            }
            s.counter += 1;
            let ticket = format!("ST-{}", s.counter);
            s.tickets.insert(ticket.clone());
            json_resp(
                200,
                json!({"responseStatus": {"type": "SUCCESSFUL"}, "serviceTicketId": ticket}),
            )
        }
        ("POST", "/di-oauth2-service/oauth/token") => {
            let auth = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_owned();
            let Some(basic) = auth.strip_prefix("Basic ") else {
                return json_resp(401, json!({"error": "no basic auth"}));
            };
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(basic)
                .unwrap_or_default();
            let decoded = String::from_utf8_lossy(&decoded).into_owned();
            let Some(client_id) = decoded.strip_suffix(':') else {
                return json_resp(401, json!({"error": "secret must be empty"}));
            };
            let client_id = client_id.to_owned();
            match form_field(&body, "grant_type").as_deref() {
                Some("refresh_token") => {
                    s.refresh_calls += 1;
                    if let Some(code) = s.refresh_status {
                        return json_resp(code, json!({"error": "forced"}));
                    }
                    let rt = form_field(&body, "refresh_token").unwrap_or_default();
                    if form_field(&body, "client_id").as_deref() != Some(client_id.as_str())
                        || !s.refresh_valid.contains(&rt)
                    {
                        return json_resp(400, json!({"error": "invalid_grant"}));
                    }
                    let (access, _) = s.issue(&client_id);
                    let mut out = json!({"access_token": access, "token_type": "Bearer", "expires_in": TOKEN_TTL});
                    if s.rotate_refresh {
                        s.refresh_valid.remove(&rt);
                        let new = format!("refresh-{}", s.counter);
                        s.refresh_valid.insert(new.clone());
                        out["refresh_token"] = json!(new);
                    }
                    json_resp(200, out)
                }
                Some(g) if g.ends_with("/grant/service_ticket") => {
                    if s.di_reject_first > 0 {
                        s.di_reject_first -= 1;
                        return json_resp(401, json!({"error": "unknown client"}));
                    }
                    let t = form_field(&body, "service_ticket").unwrap_or_default();
                    if !s.tickets.remove(&t) {
                        return json_resp(400, json!({"error": "bad ticket"}));
                    }
                    let (access, _) = s.issue(&client_id);
                    let refresh = format!("refresh-{}", s.counter);
                    s.refresh_valid.insert(refresh.clone());
                    json_resp(
                        200,
                        json!({"access_token": access, "refresh_token": refresh, "token_type": "Bearer"}),
                    )
                }
                _ => json_resp(400, json!({"error": "unsupported_grant_type"})),
            }
        }
        ("GET", "/userprofile-service/socialProfile") => {
            s.profile_calls += 1;
            if bearer_ok(&s, &headers) {
                json_resp(200, json!({"displayName": "fake"}))
            } else {
                json_resp(401, json!({"error": "invalid_token"}))
            }
        }
        ("POST", "/upload-service/upload") => {
            let bearer = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_owned();
            if !bearer_ok(&s, &headers) {
                return json_resp(401, json!({"error": "invalid_token"}));
            }
            let ct = headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if !ct.starts_with("multipart/form-data; boundary=") {
                return json_resp(400, json!({"message": "not multipart"}));
            }
            s.uploads.push((bearer, body.to_vec()));
            let up = s
                .script
                .pop_front()
                .unwrap_or_else(|| s.default_upload.clone());
            match up {
                Up::Ok(_) | Up::OkNoId => {
                    s.next_activity_id += 1;
                    let id = match up {
                        Up::Ok(_) => json!(s.next_activity_id),
                        _ => Value::Null,
                    };
                    let successes = if id.is_null() {
                        json!([])
                    } else {
                        json!([{"internalId": id, "externalId": "x", "messages": []}])
                    };
                    json_resp(
                        201,
                        json!({"detailedImportResult": {"uploadId": 1, "successes": successes, "failures": []}}),
                    )
                }
                Up::Dup => json_resp(
                    409,
                    json!({"detailedImportResult": {"failures": [{"messages": [{"code": 202, "content": "Duplicate Activity."}]}]}}),
                ),
                Up::OtherConflict => json_resp(
                    409,
                    json!({"detailedImportResult": {"failures": [{"messages": [{"code": 5, "content": "Odd conflict."}]}]}}),
                ),
                Up::Status(c) => json_resp(c, json!({"message": format!("scripted {c}")})),
                Up::Retry(c, secs) => {
                    let mut r = json_resp(c, json!({"message": "slow down"}));
                    r.headers_mut().insert(
                        "retry-after",
                        HeaderValue::from_str(&secs.to_string()).unwrap(),
                    );
                    r
                }
            }
        }
        _ => json_resp(404, json!({})),
    }
}

fn bearer_ok(s: &FakeState, h: &HeaderMap) -> bool {
    h.get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| s.access_valid.contains(t))
}

// ---- the server under test ------------------------------------------------------------

pub struct Rig {
    pub env: super::common::Env,
    pub state: api::AppState,
    pub fake: FakeGarmin,
    pub clock: Arc<AtomicI64>,
    pub cookie: String,
}

/// Server + fake Garmin on a shared, movable clock; the owner is signed in.
pub async fn rig() -> Rig {
    let clock = Arc::new(AtomicI64::new(START));
    let fake = FakeGarmin::start(clock.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let pool = db::open(dir.path()).await.unwrap();
    let user_id = super::common::make_user(&pool, super::common::USER, Role::User, true).await;
    let (_, token) = tokens::create(&pool, "test-device", Some(user_id))
        .await
        .unwrap();
    let c = clock.clone();
    let state = api::AppState::new(pool.clone(), dir.path().to_path_buf(), api::MAX_BODY_BYTES)
        .with_garmin(fake.config())
        .with_clock(move || c.load(Ordering::SeqCst));
    let app = api::router(state.clone());
    let mut env =
        super::common::Env::from_parts(dir, pool, app, token, Some(clock.clone()));
    env.user_id = user_id;
    env.adopt = std::sync::atomic::AtomicBool::new(true);
    let cookie = env.login().await;
    Rig {
        env,
        state,
        fake,
        clock,
        cookie,
    }
}

impl Rig {
    pub fn advance(&self, secs: i64) {
        self.clock.fetch_add(secs, Ordering::SeqCst);
    }

    pub fn now(&self) -> i64 {
        self.clock.load(Ordering::SeqCst)
    }

    pub async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.env.send(&self.cookie, Method::POST, uri, body).await
    }

    pub async fn status(&self) -> Value {
        self.env.get(&self.cookie, "/api/garmin").await
    }

    /// Log in through the API (no MFA); panics unless it works.
    pub async fn connect(&self) {
        let (s, j) = self
            .post(
                "/api/garmin/login",
                json!({"email": EMAIL, "password": GARMIN_PASSWORD}),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{j}");
        assert_eq!(j["mfa_required"], false);
    }

    /// Store an activity file the way a watch upload does; returns its sha256.
    pub async fn activity(&self, tag: u32) -> String {
        self.file("data_type=128&sub_type=4&source=watch", tag)
            .await
    }

    pub async fn file(&self, query: &str, tag: u32) -> String {
        let (s, j) = self.env.upload(query, &super::common::tiny_fit(tag)).await;
        assert_eq!(s, StatusCode::CREATED, "{j}");
        j["sha256"].as_str().unwrap().to_owned()
    }

    pub async fn run(&self) -> usize {
        nimrag_server::garmin::queue::run_due(&self.state)
            .await
            .unwrap()
    }

    pub async fn item(&self, sha: &str) -> (String, i64, i64, Option<i64>, Option<String>) {
        use sqlx::Row;
        let r = sqlx::query(
            "SELECT status, attempts, next_attempt_at, garmin_activity_id, last_error FROM garmin_uploads WHERE file_sha256 = ?",
        )
        .bind(sha)
        .fetch_one(&self.env.pool)
        .await
        .unwrap();
        (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4))
    }

    pub async fn queue_len(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM garmin_uploads")
            .fetch_one(&self.env.pool)
            .await
            .unwrap()
    }

    pub async fn stored_tokens(&self) -> (String, String, String) {
        use sqlx::Row;
        let r = sqlx::query("SELECT di_token, di_refresh_token, di_client_id FROM garmin_account")
            .fetch_one(&self.env.pool)
            .await
            .unwrap();
        (r.get(0), r.get(1), r.get(2))
    }
}

/// Everything in the data directory (database, WAL, stored files) as bytes.
pub fn all_bytes(dir: &std::path::Path) -> Vec<Vec<u8>> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(b) = std::fs::read(&p) {
                out.push(b);
            }
        }
    }
    out
}

pub fn contains(hay: &[u8], needle: &str) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}
