//! The HTTP seam of the Garmin client.
//!
//! Everything that talks to Garmin goes through [`GarminHttp`]. The shipped
//! implementation is plain `reqwest` + rustls ([`ReqwestHttp`]).
//!
//! **TLS fingerprinting risk (record for the owner):** Garmin's SSO sits behind
//! Cloudflare and has been seen to refuse non-browser TLS/HTTP2 fingerprints
//! at the login step with 429/427/403 (python-garminconnect rotates curl_cffi
//! Safari/Chrome profiles for exactly this reason, see
//! `docs/research/garmin-forward.md` section 4). Plain reqwest may therefore be
//! refused at login even with correct credentials. connectapi (refresh, upload)
//! is reached by plain `requests` in the reference client and is expected to
//! work. If login is blocked: (1) implement this trait with a browser-TLS
//! client (e.g. `wreq`) and pass it via [`crate::garmin::GarminConfig`], or
//! (2) use the token import fallback (`POST /api/garmin/import-tokens`) with a
//! token file made by python-garminconnect on another machine.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// A request body, already encoded: the transport adds only `Content-Type`.
#[derive(Clone)]
pub enum Body {
    None,
    Bytes { content_type: String, data: Vec<u8> },
}

/// A request. Not `Debug` on purpose: bodies and headers carry passwords and tokens.
#[derive(Clone)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

impl Request {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: Method::Get,
            url: url.into(),
            headers: vec![],
            body: Body::None,
        }
    }
    pub fn post(url: impl Into<String>, body: Body) -> Self {
        Self {
            method: Method::Post,
            url: url.into(),
            headers: vec![],
            body,
        }
    }
    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_owned(), value.into()));
        self
    }
    pub fn headers(mut self, h: impl IntoIterator<Item = (String, String)>) -> Self {
        self.headers.extend(h);
        self
    }
}

/// A response. Not `Debug`: bodies hold tokens.
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    pub fn json(&self) -> Option<serde_json::Value> {
        serde_json::from_slice(&self.body).ok()
    }
}

/// Transport failure (no HTTP response): connect, TLS, timeout. The message
/// never contains a URL query or a body.
#[derive(Debug)]
pub struct HttpError(pub String);

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "network error: {}", self.0)
    }
}
impl std::error::Error for HttpError {}

/// One HTTP session. A session made with `cookies = true` keeps the cookies
/// it receives (the SSO login needs them across its steps).
pub trait GarminHttp: Send + Sync {
    fn send(&self, req: Request) -> BoxFut<'_, Result<Response, HttpError>>;
}

/// Makes sessions: `true` = with a cookie jar (login), `false` = without (API calls).
pub type HttpFactory = Arc<dyn Fn(bool) -> Arc<dyn GarminHttp> + Send + Sync>;

/// Plain reqwest + rustls.
pub struct ReqwestHttp {
    client: reqwest::Client,
}

impl ReqwestHttp {
    /// `use_proxy = false` ignores `HTTP(S)_PROXY` (tests talk to 127.0.0.1).
    pub fn new(cookies: bool, use_proxy: bool) -> Self {
        let mut b = reqwest::Client::builder()
            .cookie_store(cookies)
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(15));
        if !use_proxy {
            b = b.no_proxy();
        }
        let client = b.build().unwrap_or_else(|_| reqwest::Client::new());
        Self { client }
    }

    pub fn factory(use_proxy: bool) -> HttpFactory {
        Arc::new(move |cookies| Arc::new(Self::new(cookies, use_proxy)))
    }
}

impl GarminHttp for ReqwestHttp {
    fn send(&self, req: Request) -> BoxFut<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let mut rb = match req.method {
                Method::Get => self.client.get(&req.url),
                Method::Post => self.client.post(&req.url),
            };
            for (k, v) in &req.headers {
                rb = rb.header(k, v);
            }
            if let Body::Bytes { content_type, data } = req.body {
                rb = rb.header("content-type", content_type).body(data);
            }
            let resp = rb.send().await.map_err(|e| HttpError(describe(e)))?;
            let status = resp.status().as_u16();
            let headers = resp
                .headers()
                .iter()
                .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
                .collect();
            let body = resp
                .bytes()
                .await
                .map_err(|e| HttpError(describe(e)))?
                .to_vec();
            Ok(Response {
                status,
                headers,
                body,
            })
        })
    }
}

fn describe(e: reqwest::Error) -> String {
    let e = e.without_url();
    let kind = if e.is_timeout() {
        "timeout"
    } else if e.is_connect() {
        "connect"
    } else if e.is_redirect() {
        "redirect"
    } else {
        "request"
    };
    format!("{kind} failed")
}

// ---- encoding helpers ---------------------------------------------------------

/// Percent-encode everything but the RFC 3986 unreserved characters.
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `application/x-www-form-urlencoded` body.
pub fn form(pairs: &[(&str, &str)]) -> Body {
    let data = pairs
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
        .into_bytes();
    Body::Bytes {
        content_type: "application/x-www-form-urlencoded".into(),
        data,
    }
}

pub fn json_body(v: &serde_json::Value) -> Body {
    Body::Bytes {
        content_type: "application/json".into(),
        data: v.to_string().into_bytes(),
    }
}

/// `multipart/form-data` with one file part.
pub fn multipart_file(field: &str, filename: &str, bytes: &[u8]) -> Body {
    let mut boundary = [0u8; 12];
    let _ = getrandom::fill(&mut boundary);
    let boundary = format!("----nimrag{}", crate::hex(&boundary));
    let mut data = Vec::with_capacity(bytes.len() + 256);
    data.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    data.extend_from_slice(bytes);
    data.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Body::Bytes {
        content_type: format!("multipart/form-data; boundary={boundary}"),
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencoding() {
        assert_eq!(urlencode("a b&c=d/é~"), "a%20b%26c%3Dd%2F%C3%A9~");
        let Body::Bytes { data, .. } = form(&[("k", "v w"), ("x", "y")]) else {
            panic!()
        };
        assert_eq!(data, b"k=v%20w&x=y");
    }

    #[test]
    fn multipart_shape() {
        let Body::Bytes { content_type, data } = multipart_file("file", "a.fit", b"BYTES") else {
            panic!()
        };
        let b = content_type.split("boundary=").nth(1).unwrap();
        let s = String::from_utf8(data).unwrap();
        assert!(s.starts_with(&format!("--{b}\r\n")));
        assert!(s.contains("name=\"file\"; filename=\"a.fit\""));
        assert!(s.contains("\r\n\r\nBYTES\r\n"));
        assert!(s.ends_with(&format!("--{b}--\r\n")));
    }
}
