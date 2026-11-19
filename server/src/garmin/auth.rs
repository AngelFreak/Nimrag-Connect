//! Garmin login (mobile SSO JSON flow), CAS ticket to DI OAuth2 exchange,
//! token refresh and token checks.
//!
//! Ported from python-garminconnect **0.3.20** (MIT, (c) 2020-2026 Ron
//! Klinkien, https://github.com/cyberjunky/python-garminconnect, release commit
//! 3eee7a4d): `garminconnect/client/sso.py` (mobile strategy: sign-in page GET,
//! `/mobile/api/login`, `/mobile/api/mfa/verifyCode`), `client/session.py`
//! (DI token exchange and refresh), `client/transport.py` (`_native_headers`),
//! `client/tokens.py` (Basic auth, JWT `exp`). Field names and headers are
//! copied from that source, never from memory; see
//! `docs/research/garmin-forward.md`. NOT tested against real Garmin.
//!
//! Secrets: the password is only ever a parameter of [`begin_login`]; it is put
//! into the request body in memory and not stored, logged or put into an error.
//! Tokens have a redacting `Debug` and are never logged.

use super::http::{self, GarminHttp, Request};
use super::{GarminConfig, GarminUrls};
use base64::Engine;
use serde_json::{json, Value};
use std::sync::Arc;

const CLIENT_ID: &str = "GCM_IOS_DARK";
const IOS_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148";
const DI_GRANT_TYPE: &str =
    "https://connectapi.garmin.com/di-oauth2-service/oauth/grant/service_ticket";
/// Tried in this order until one exchange succeeds (constants.py `DI_CLIENT_IDS`).
const DI_CLIENT_IDS: [&str; 4] = [
    "GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2",
    "GARMIN_CONNECT_MOBILE_ANDROID_DI_2024Q4",
    "GARMIN_CONNECT_MOBILE_ANDROID_DI",
    "GARMIN_CONNECT_MOBILE_IOS_DI",
];
/// Refresh when the access token expires within this many seconds (tokens.py).
const REFRESH_MARGIN_S: i64 = 900;

/// DI OAuth2 tokens: the whole persisted Garmin session.
#[derive(Clone, PartialEq, Eq)]
pub struct Tokens {
    pub di_token: String,
    pub di_refresh_token: String,
    pub di_client_id: String,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("di_token", &"<redacted>")
            .field("di_refresh_token", &"<redacted>")
            .field("di_client_id", &self.di_client_id)
            .finish()
    }
}

impl Tokens {
    /// Structural check of what the token file / a login result holds.
    pub fn validate(&self) -> Result<(), String> {
        if self.di_token.is_empty() || self.di_refresh_token.is_empty() {
            return Err("di_token and di_refresh_token must not be empty".into());
        }
        if self.di_client_id.is_empty() || self.di_client_id.len() > 100 {
            return Err("di_client_id is missing".into());
        }
        if jwt_claims(&self.di_token).is_none() {
            return Err("di_token is not a JWT".into());
        }
        Ok(())
    }

    /// True when the access token expires within 15 minutes (or has no readable `exp`: then
    /// the token is used as is, as the reference client does).
    pub fn expires_soon(&self, now: i64) -> bool {
        jwt_claims(&self.di_token)
            .and_then(|c| c.get("exp").and_then(Value::as_f64))
            .is_some_and(|exp| exp.is_finite() && (now as f64) > exp - REFRESH_MARGIN_S as f64)
    }
}

/// Payload of a JWT, unverified (Garmin signs it; we hold no key). `alg: none` is refused.
pub fn jwt_claims(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let (h, p) = (parts.next()?, parts.next()?);
    let dec = |s: &str| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(s.trim_end_matches('='))
            .ok()
    };
    let header: Value = serde_json::from_slice(&dec(h)?).ok()?;
    if header.get("alg").and_then(Value::as_str) == Some("none") {
        return None;
    }
    serde_json::from_slice(&dec(p)?).ok()
}

#[derive(Debug, PartialEq, Eq)]
pub enum AuthError {
    /// Garmin said the email or password is wrong.
    InvalidCredentials,
    /// Garmin refused the MFA code (the pending login stays usable).
    InvalidMfaCode,
    /// 429 / 427 / a rate-limit body; seconds from `Retry-After` when sent.
    RateLimited(Option<u64>),
    /// 403 / captcha: Garmin treats this client as a bot (TLS fingerprint).
    Blocked(String),
    /// The refresh token (or ticket) was refused: a new login is needed.
    Rejected,
    /// Network error or 5xx: try again later.
    Unavailable(String),
    /// Anything else that does not match the expected protocol.
    Unexpected(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCredentials => write!(f, "Garmin rejected the email or password"),
            Self::InvalidMfaCode => write!(f, "Garmin rejected the verification code"),
            Self::RateLimited(_) => write!(f, "Garmin is rate limiting this server"),
            Self::Blocked(why) => write!(f, "Garmin blocked the login ({why})"),
            Self::Rejected => write!(f, "Garmin rejected the stored login"),
            Self::Unavailable(why) => write!(f, "Garmin is unreachable ({why})"),
            Self::Unexpected(why) => write!(f, "unexpected answer from Garmin ({why})"),
        }
    }
}

impl From<http::HttpError> for AuthError {
    fn from(e: http::HttpError) -> Self {
        Self::Unavailable(e.0)
    }
}

/// The headers every authenticated request and the DI endpoints carry (transport.py `_native_headers`).
pub fn native_headers() -> Vec<(String, String)> {
    [
        ("User-Agent", "GCM-Android-5.23"),
        (
            "X-Garmin-User-Agent",
            "com.garmin.android.apps.connectmobile/5.23; ; Google/sdk_gphone64_arm64/google; Android/33; Dalvik/2.1.0",
        ),
        ("X-Garmin-Paired-App-Version", "10861"),
        ("X-Garmin-Client-Platform", "Android"),
        ("X-App-Ver", "10861"),
        ("X-Lang", "en"),
        ("X-GCExperience", "GC5"),
        ("Accept-Language", "en-US,en;q=0.9"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}

fn basic(client_id: &str) -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{client_id}:"))
    )
}

fn retry_after(r: &http::Response) -> Option<u64> {
    r.header("retry-after").and_then(|v| v.trim().parse().ok())
}

// ---- login ----------------------------------------------------------------------

/// A login waiting for its MFA code: the cookie-jar session of the credential
/// POST. Held in memory only; not `Debug`.
pub struct PendingMfa {
    http: Arc<dyn GarminHttp>,
    method: String,
}

pub enum LoginStep {
    Done(Tokens),
    Mfa(PendingMfa),
}

struct SsoUrls {
    page: String,
    login: String,
    mfa: String,
    referer: String,
}

fn sso_urls(u: &GarminUrls) -> SsoUrls {
    let svc = http::urlencode(&u.ios_service);
    let q = format!("clientId={CLIENT_ID}&locale=en-US&service={svc}");
    let referer = format!(
        "{}/mobile/sso/en_US/sign-in?clientId={CLIENT_ID}&service={svc}",
        u.sso
    );
    SsoUrls {
        page: referer.clone(),
        login: format!("{}/mobile/api/login?{q}", u.sso),
        mfa: format!("{}/mobile/api/mfa/verifyCode?{q}", u.sso),
        referer,
    }
}

fn json_headers(sso: &str, referer: &str) -> Vec<(String, String)> {
    vec![
        ("User-Agent".into(), IOS_UA.into()),
        ("Accept".into(), "application/json, text/plain, */*".into()),
        ("Accept-Language".into(), "en-US,en;q=0.9".into()),
        ("Origin".into(), sso.into()),
        ("Referer".into(), referer.into()),
    ]
}

async fn pause(cfg: &GarminConfig) {
    let (lo, hi) = cfg.login_delay_ms;
    if hi == 0 {
        return;
    }
    let mut b = [0u8; 4];
    let _ = getrandom::fill(&mut b);
    let span = hi.saturating_sub(lo) + 1;
    let ms = lo + u64::from(u32::from_le_bytes(b)) % span;
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

/// Steps 1 and 2: prime the session (cookies), then POST the credentials.
/// On `MFA_REQUIRED` the cookie-jar session is returned for [`verify_mfa`].
pub async fn begin_login(
    cfg: &GarminConfig,
    email: &str,
    password: &str,
) -> Result<LoginStep, AuthError> {
    let urls = sso_urls(&cfg.urls);
    let session = (cfg.http)(true);

    // Step 1: the sign-in page. Only its cookies matter; skipping it makes the
    // credential POST answer 429 (python-garminconnect PR #479).
    let page = session
        .send(
            Request::get(&urls.page)
                .header("User-Agent", IOS_UA)
                .header(
                    "Accept",
                    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                )
                .header("Accept-Language", "en-US,en;q=0.9"),
        )
        .await?;
    match page.status {
        429 | 427 => return Err(AuthError::RateLimited(retry_after(&page))),
        403 => {
            return Err(AuthError::Blocked(
                "bot challenge on the sign-in page".into(),
            ))
        }
        s if s >= 500 => return Err(AuthError::Unavailable(format!("sign-in page status {s}"))),
        _ => {}
    }
    pause(cfg).await;

    // Step 2: the credential POST.
    let body = http::json_body(&json!({
        "username": email,
        "password": password,
        "rememberMe": true,
        "captchaToken": "",
    }));
    let resp = session
        .send(Request::post(&urls.login, body).headers(json_headers(&cfg.urls.sso, &urls.referer)))
        .await?;
    match interpret(&resp)? {
        Interpreted::Ticket(t) => Ok(LoginStep::Done(exchange(cfg, &t).await?)),
        Interpreted::Mfa(method) => Ok(LoginStep::Mfa(PendingMfa {
            http: session,
            method,
        })),
        Interpreted::Other(kind) => {
            Err(AuthError::Unexpected(format!("login status type {kind:?}")))
        }
    }
}

/// Step 3: submit the MFA code on the same cookie-jar session.
pub async fn verify_mfa(
    cfg: &GarminConfig,
    pending: &PendingMfa,
    code: &str,
) -> Result<Tokens, AuthError> {
    let urls = sso_urls(&cfg.urls);
    let body = http::json_body(&json!({
        "mfaMethod": pending.method,
        "mfaVerificationCode": code,
        "rememberMyBrowser": true,
        "reconsentList": [],
        "mfaSetup": false,
    }));
    let resp = pending
        .http
        .send(Request::post(&urls.mfa, body).headers(json_headers(&cfg.urls.sso, &urls.referer)))
        .await?;
    match interpret(&resp) {
        Ok(Interpreted::Ticket(t)) => exchange(cfg, &t).await,
        // Any other JSON answer to a code is a failed check (the reference client treats it so);
        // the pending login stays usable for another try.
        Ok(Interpreted::Mfa(_) | Interpreted::Other(_)) | Err(AuthError::InvalidCredentials) => {
            Err(AuthError::InvalidMfaCode)
        }
        Err(e) => Err(e),
    }
}

enum Interpreted {
    Ticket(String),
    Mfa(String),
    /// A well-formed answer with another `responseStatus.type` (truncated).
    Other(String),
}

/// Read `responseStatus.type` of a login / verifyCode answer (sso.py:464-527).
fn interpret(resp: &http::Response) -> Result<Interpreted, AuthError> {
    match resp.status {
        429 | 427 => return Err(AuthError::RateLimited(retry_after(resp))),
        403 => return Err(AuthError::Blocked("bot challenge on the login".into())),
        s if s >= 500 => return Err(AuthError::Unavailable(format!("login status {s}"))),
        _ => {}
    }
    let Some(json) = resp.json() else {
        return Err(AuthError::Blocked("the login answer was not JSON".into()));
    };
    if json
        .pointer("/error/status-code")
        .and_then(Value::as_str)
        .is_some_and(|c| c == "429")
    {
        return Err(AuthError::RateLimited(None));
    }
    match json
        .pointer("/responseStatus/type")
        .and_then(Value::as_str)
        .unwrap_or("")
    {
        "SUCCESSFUL" => json
            .get("serviceTicketId")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(|t| Interpreted::Ticket(t.to_owned()))
            .ok_or_else(|| AuthError::Unexpected("success without a service ticket".into())),
        "MFA_REQUIRED" => Ok(Interpreted::Mfa(
            json.pointer("/customerMfaInfo/mfaLastMethodUsed")
                .and_then(Value::as_str)
                .filter(|m| !m.is_empty() && m.len() < 32)
                .unwrap_or("email")
                .to_owned(),
        )),
        "INVALID_USERNAME_PASSWORD" => Err(AuthError::InvalidCredentials),
        "CAPTCHA_REQUIRED" => Err(AuthError::Blocked("captcha required".into())),
        other => Ok(Interpreted::Other(other.chars().take(40).collect())),
    }
}

/// Step 4: CAS service ticket to DI OAuth2 tokens (session.py:189-244).
async fn exchange(cfg: &GarminConfig, ticket: &str) -> Result<Tokens, AuthError> {
    let api = (cfg.http)(false);
    let mut last = 0u16;
    for client_id in DI_CLIENT_IDS {
        let body = http::form(&[
            ("client_id", client_id),
            ("service_ticket", ticket),
            ("grant_type", DI_GRANT_TYPE),
            ("service_url", &cfg.urls.ios_service),
        ]);
        let req = Request::post(cfg.urls.di_token_url(), body)
            .headers(native_headers())
            .header("Authorization", basic(client_id))
            .header("Accept", "application/json,text/html;q=0.9,*/*;q=0.8")
            .header("Cache-Control", "no-cache");
        let resp = api.send(req).await?;
        last = resp.status;
        if resp.status == 429 {
            return Err(AuthError::RateLimited(retry_after(&resp)));
        }
        if !(200..300).contains(&resp.status) {
            continue;
        }
        let Some(j) = resp.json() else { continue };
        let (Some(access), Some(refresh)) = (
            j.get("access_token").and_then(Value::as_str),
            j.get("refresh_token").and_then(Value::as_str),
        ) else {
            continue;
        };
        // The client id is read from the JWT claim, falling back to the id that worked.
        let id = jwt_claims(access)
            .and_then(|c| {
                c.get("client_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| client_id.to_owned());
        return Ok(Tokens {
            di_token: access.to_owned(),
            di_refresh_token: refresh.to_owned(),
            di_client_id: id,
        });
    }
    if last >= 500 {
        return Err(AuthError::Unavailable(format!(
            "token exchange status {last}"
        )));
    }
    Err(AuthError::Unexpected(format!(
        "token exchange refused (status {last})"
    )))
}

// ---- refresh and token check -------------------------------------------------------

/// Refresh the access token (session.py:246-279). The refresh token may rotate:
/// the caller must store the returned tokens. No cookies, no SSO.
pub async fn refresh(
    cfg: &GarminConfig,
    api: &dyn GarminHttp,
    tokens: &Tokens,
) -> Result<Tokens, AuthError> {
    let body = http::form(&[
        ("grant_type", "refresh_token"),
        ("client_id", &tokens.di_client_id),
        ("refresh_token", &tokens.di_refresh_token),
    ]);
    let req = Request::post(cfg.urls.di_token_url(), body)
        .headers(native_headers())
        .header("Authorization", basic(&tokens.di_client_id))
        .header("Accept", "application/json")
        .header("Cache-Control", "no-cache");
    let resp = api.send(req).await?;
    match resp.status {
        200..=299 => {
            let j = resp
                .json()
                .ok_or_else(|| AuthError::Unexpected("refresh answer was not JSON".into()))?;
            let access = j
                .get("access_token")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
                .ok_or_else(|| AuthError::Unexpected("refresh without access_token".into()))?;
            let refresh = j
                .get("refresh_token")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
                .unwrap_or(&tokens.di_refresh_token);
            Ok(Tokens {
                di_token: access.to_owned(),
                di_refresh_token: refresh.to_owned(),
                di_client_id: tokens.di_client_id.clone(),
            })
        }
        429 => Err(AuthError::RateLimited(retry_after(&resp))),
        400 | 401 | 403 => Err(AuthError::Rejected),
        s => Err(AuthError::Unavailable(format!("refresh status {s}"))),
    }
}

/// Headers of an authenticated connectapi request.
pub fn api_headers(tokens: &Tokens) -> Vec<(String, String)> {
    let mut h = native_headers();
    h.push((
        "Authorization".into(),
        format!("Bearer {}", tokens.di_token),
    ));
    h.push(("Accept".into(), "application/json".into()));
    h
}

#[derive(Debug, PartialEq, Eq)]
pub enum TokenCheck {
    Valid,
    Unauthorized,
}

/// `GET /userprofile-service/socialProfile`: proves a token is accepted (sso.py:207-218).
pub async fn check_token(
    cfg: &GarminConfig,
    api: &dyn GarminHttp,
    tokens: &Tokens,
) -> Result<TokenCheck, AuthError> {
    let url = format!("{}/userprofile-service/socialProfile", cfg.urls.connectapi);
    let resp = api
        .send(Request::get(url).headers(api_headers(tokens)))
        .await?;
    match resp.status {
        200..=299 => Ok(TokenCheck::Valid),
        401 | 403 => Ok(TokenCheck::Unauthorized),
        429 => Err(AuthError::RateLimited(retry_after(&resp))),
        s => Err(AuthError::Unavailable(format!("profile check status {s}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(claims: &Value) -> String {
        let e = |v: &Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
        format!("{}.{}.sig", e(&json!({"alg": "RS256"})), e(claims))
    }

    #[test]
    fn jwt_expiry_and_margin() {
        let t = Tokens {
            di_token: jwt(&json!({"exp": 10_000, "client_id": "X"})),
            di_refresh_token: "r".into(),
            di_client_id: "X".into(),
        };
        assert!(t.validate().is_ok());
        assert!(!t.expires_soon(10_000 - 901));
        assert!(t.expires_soon(10_000 - 899));
        assert!(t.expires_soon(20_000));
    }

    #[test]
    fn validation_rejects_junk() {
        let bad = |t: &str, r: &str, c: &str| Tokens {
            di_token: t.into(),
            di_refresh_token: r.into(),
            di_client_id: c.into(),
        };
        assert!(bad("", "r", "c").validate().is_err());
        assert!(bad("not-a-jwt", "r", "c").validate().is_err());
        let ok = jwt(&json!({"exp": 1}));
        assert!(bad(&ok, "", "c").validate().is_err());
        assert!(bad(&ok, "r", "").validate().is_err());
        let none = format!(
            "{}.{}.",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{}")
        );
        assert!(bad(&none, "r", "c").validate().is_err());
    }

    #[test]
    fn debug_hides_tokens() {
        let t = Tokens {
            di_token: "SECRETACCESS".into(),
            di_refresh_token: "SECRETREFRESH".into(),
            di_client_id: "CID".into(),
        };
        let d = format!("{t:?}");
        assert!(!d.contains("SECRET") && d.contains("CID"));
    }

    #[test]
    fn basic_auth_has_empty_secret() {
        // base64("GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2:")
        let b = basic("A");
        assert_eq!(b, "Basic QTo=");
    }
}
