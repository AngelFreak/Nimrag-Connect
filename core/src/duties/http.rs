//! The watch's HTTP-over-protobuf requests (`Smart.connect_iq_http_service` 2)
//! and the data transfer that carries large bodies (`Smart.data_transfer_service`
//! 7): decoding what the watch asks and encoding the answers, as pure functions
//! that never panic on peer data.
//!
//! Ported from Gadgetbridge commit 7257742b (AGPL-3.0-or-later):
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/http/HttpHandler.java`
//! (Copyright (C) 2024-2026 José Rebelo, kuhy, Daniele Gobbetti, Thomas
//! Kuehne), `http/GarminHttpRequest.java`, `http/GarminHttpResponse.java`,
//! `http/DataTransferHandler.java` (the protobuf shapes of the answers) and
//! `http/interceptors/FirewallInterceptor.java` (what is never forwarded;
//! Copyright (C) 2024-2026 as above). The protobufs are written against our own
//! recovered schemas `docs/protocol/proto/gdi_connect_iq_http.proto` and
//! `gdi_data_transfer.proto`, in field-number order as protobuf-java writes
//! them. *inferred (Gadgetbridge)*: nothing here has been seen with an
//! Instinct 2. Our captures only hold requests whose content is the opaque
//! `encrypted_request` (field 11), which this module reports as unreadable.
//!
//! Differences, marked [ours]: only `GET` is served; the request's
//! `max_size` is not used; an address with user information is refused; the
//! host and path of a refused request are cleaned before they can reach a log
//! (`loggable_host`, `loggable_path`: never the query, no number-like path
//! segments, which could be coordinates).

use crate::messages::{next_field, Wire};

/// `Smart` field numbers (`gdi_smart.proto`).
const SMART_CONNECT_IQ_HTTP: u64 = 2;
const SMART_DATA_TRANSFER: u64 = 7;

/// `RawResourceResponse.ResponseStatus`.
pub const RAW_OK: u64 = 100;
pub const RAW_NETWORK_REQUEST_TIMED_OUT: u64 = 200;
/// `ConnectIQHTTPResponse.ResponseStatus.NETWORK_REQUEST_TIMED_OUT`.
const WEB_NETWORK_REQUEST_TIMED_OUT: u64 = 300;

/// `FileAccessItemResponse.Status.UNKNOWN` / `INVALID_ID`.
pub const FILE_ACCESS_UNKNOWN: u64 = 0;
pub const FILE_ACCESS_INVALID_ID: u64 = 2;

/// `RawResourceRequest.HTTPMethod.GET`.
const METHOD_GET: u64 = 1;

/// What the watch asked in one protobuf message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// `raw_resource_request`: the form the ephemeris download uses.
    Raw(RawRequest),
    /// `connect_iq_http_request`: a Connect IQ app's request (its headers and
    /// body are in Garmin's binary JSON, which we do not read).
    Web { url: String },
    /// An image request, an OAuth request or "open this web page": the url
    /// when there is one.
    Other { url: Option<String> },
    /// An HTTP request with no url we can read (the encrypted form).
    Unreadable,
    /// `data_download_request`: the watch pulls a piece of a registered body.
    DataDownload {
        id: u32,
        offset: u32,
        max_chunk: Option<u32>,
    },
    /// `file_access_item_request`: the watch asks to be given the body by file
    /// access instead (a push over a multi-link file transfer).
    FileAccess { data_xfer_id: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRequest {
    pub url: String,
    /// `RawResourceRequest.HTTPMethod` number; GET when absent.
    pub method: u64,
    /// Header names in lower case, in the order sent.
    pub headers: Vec<(String, String)>,
    pub use_data_xfer: bool,
    /// The request carries multipart forms (a body we do not build).
    pub has_forms: bool,
}

impl RawRequest {
    pub fn is_get(&self) -> bool {
        self.method == METHOD_GET
    }

    /// The value of a header, by lower-case name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Decode the Smart bytes of a protobuf message from the watch. `None` when
/// they hold neither HTTP nor data-transfer content (or are malformed).
pub fn parse_smart(body: &[u8]) -> Option<Incoming> {
    let mut i = 0;
    while i < body.len() {
        match next_field(body, &mut i)? {
            (SMART_CONNECT_IQ_HTTP, Wire::Bytes(service)) => return parse_http_service(service),
            (SMART_DATA_TRANSFER, Wire::Bytes(service)) => return parse_data_transfer(service),
            _ => {}
        }
    }
    None
}

fn parse_http_service(b: &[u8]) -> Option<Incoming> {
    let mut i = 0;
    while i < b.len() {
        match next_field(b, &mut i)? {
            (5, Wire::Bytes(raw)) => {
                return Some(parse_raw(raw).map_or(Incoming::Unreadable, Incoming::Raw));
            }
            (1, Wire::Bytes(web)) => {
                return Some(match first_string(web, 1) {
                    Some(url) => Incoming::Web { url },
                    None => Incoming::Unreadable,
                });
            }
            // image request, OAuth request, OAuth complete, open web page
            (3 | 7 | 9 | 11, Wire::Bytes(other)) => {
                return Some(Incoming::Other {
                    url: first_string(other, 1),
                });
            }
            _ => {}
        }
    }
    Some(Incoming::Unreadable)
}

/// Field `number` of a message as text, the first one; `None` when absent or
/// malformed.
fn first_string(b: &[u8], number: u64) -> Option<String> {
    let mut i = 0;
    while i < b.len() {
        if let (n, Wire::Bytes(s)) = next_field(b, &mut i)? {
            if n == number {
                return String::from_utf8(s.to_vec()).ok();
            }
        }
    }
    None
}

fn parse_raw(b: &[u8]) -> Option<RawRequest> {
    let mut url = None;
    let mut r = RawRequest {
        url: String::new(),
        method: METHOD_GET,
        headers: Vec::new(),
        use_data_xfer: false,
        has_forms: false,
    };
    let mut i = 0;
    while i < b.len() {
        match next_field(b, &mut i)? {
            (1, Wire::Bytes(s)) => url = Some(String::from_utf8(s.to_vec()).ok()?),
            (3, Wire::Varint(v)) => r.method = v,
            (5, Wire::Bytes(h)) => {
                if let Some(header) = parse_header(h) {
                    r.headers.push(header);
                }
            }
            (6, Wire::Varint(v)) => r.use_data_xfer = v != 0,
            (8, Wire::Bytes(_)) => r.has_forms = true,
            _ => {}
        }
    }
    r.url = url?;
    Some(r)
}

/// One `HTTPHeader { key = 1, value = 2 }`, the name in lower case.
fn parse_header(b: &[u8]) -> Option<(String, String)> {
    let (mut key, mut value) = (None, None);
    let mut i = 0;
    while i < b.len() {
        match next_field(b, &mut i)? {
            (1, Wire::Bytes(s)) => key = Some(String::from_utf8(s.to_vec()).ok()?),
            (2, Wire::Bytes(s)) => value = Some(String::from_utf8(s.to_vec()).ok()?),
            _ => {}
        }
    }
    Some((key?.to_ascii_lowercase(), value?))
}

fn parse_data_transfer(b: &[u8]) -> Option<Incoming> {
    let mut i = 0;
    while i < b.len() {
        match next_field(b, &mut i)? {
            (1, Wire::Bytes(req)) => {
                let (mut id, mut offset, mut max) = (None, None, None);
                let mut j = 0;
                while j < req.len() {
                    match next_field(req, &mut j)? {
                        (1, Wire::Varint(v)) => id = u32::try_from(v).ok(),
                        (2, Wire::Varint(v)) => offset = u32::try_from(v).ok(),
                        (3, Wire::Varint(v)) => max = u32::try_from(v).ok(),
                        _ => {}
                    }
                }
                return Some(Incoming::DataDownload {
                    id: id?,
                    offset: offset?,
                    max_chunk: max,
                });
            }
            (8, Wire::Bytes(req)) => {
                let mut id = None;
                let mut j = 0;
                while j < req.len() {
                    if let (1, Wire::Varint(v)) = next_field(req, &mut j)? {
                        id = u32::try_from(v).ok();
                    }
                }
                return Some(Incoming::FileAccess { data_xfer_id: id? });
            }
            _ => {}
        }
    }
    None
}

// ---- URLs ----------------------------------------------------------------------------------

/// The parts of a request address the duties look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    /// Lower case, without a port.
    pub host: String,
    /// Starts with `/`; `/` when the address has none.
    pub path: String,
    /// What follows `?`, without a fragment.
    pub query: Option<String>,
}

impl Url {
    /// `path` plus `?query`, as the request line of the original address.
    pub fn path_and_query(&self) -> String {
        match &self.query {
            Some(q) => format!("{}?{}", self.path, q),
            None => self.path.clone(),
        }
    }
}

/// Split an `http://` or `https://` address. `None` for other schemes, an
/// address with user information, an empty host or a port that is not a number.
/// [ours]: Gadgetbridge takes whatever Android's `Uri.parse` gives.
pub fn parse_url(s: &str) -> Option<Url> {
    let (scheme, rest) = s.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        return None;
    }
    let rest = rest.split('#').next().unwrap_or("");
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if authority.contains('@') {
        return None;
    }
    let host = match authority.rsplit_once(':') {
        Some((h, port)) => {
            if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            h
        }
        None => authority,
    };
    if host.is_empty() {
        return None;
    }
    let (path, query) = match tail.split_once('?') {
        Some((p, q)) => (p, Some(q.to_string())),
        None => (tail, None),
    };
    Some(Url {
        host: host.to_ascii_lowercase(),
        path: if path.is_empty() {
            "/".into()
        } else {
            path.into()
        },
        query,
    })
}

/// A host cleaned for a log line: letters, digits, `.` and `-` only, at most 64
/// bytes; `?` when nothing is left.
pub fn loggable_host(host: &str) -> String {
    let h: String = host
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
        .take(64)
        .collect();
    if h.is_empty() {
        "?".into()
    } else {
        h
    }
}

/// A path cleaned for a log line: at most six segments; a segment is kept when
/// it is made of letters, `-` and `_` (or is a version such as `v2`), any other
/// segment, which could hold a number, an id or a coordinate, becomes `*`; at
/// most 24 bytes per segment; never a query. [ours]
pub fn loggable_path(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or("");
    let mut out = String::new();
    for seg in path.split('/').filter(|s| !s.is_empty()).take(6) {
        out.push('/');
        let version = seg
            .strip_prefix('v')
            .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
        let plain = seg
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || b == b'-' || b == b'_');
        if (plain || version) && seg.len() <= 24 {
            out.push_str(seg);
        } else {
            out.push('*');
        }
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

// ---- encoding ------------------------------------------------------------------------------

pub(crate) fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

pub(crate) fn put_uint(out: &mut Vec<u8>, field: u64, v: u64) {
    put_varint(out, field << 3);
    put_varint(out, v);
}

pub(crate) fn put_bytes(out: &mut Vec<u8>, field: u64, bytes: &[u8]) {
    put_varint(out, (field << 3) | 2);
    put_varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

/// How the body of a raw response travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body<'a> {
    /// In the reply itself (`resource_data`).
    Inline(&'a [u8]),
    /// Pulled by the watch (`xfer_data { id, size }`).
    Xfer { id: u32, size: u32 },
}

fn smart_http(service_field: u64, message: &[u8]) -> Vec<u8> {
    let mut service = Vec::new();
    put_bytes(&mut service, service_field, message);
    let mut smart = Vec::new();
    put_bytes(&mut smart, SMART_CONNECT_IQ_HTTP, &service);
    smart
}

/// `Smart { connect_iq_http_service { raw_resource_response { status OK,
/// http_status_code, resource_data or xfer_data, headers } } }`.
pub fn raw_response(http_status: u16, headers: &[(String, String)], body: Body) -> Vec<u8> {
    let mut m = Vec::new();
    put_uint(&mut m, 1, RAW_OK);
    put_uint(&mut m, 2, u64::from(http_status));
    match body {
        Body::Inline(b) => put_bytes(&mut m, 3, b),
        Body::Xfer { id, size } => {
            let mut item = Vec::new();
            put_uint(&mut item, 1, u64::from(id));
            put_uint(&mut item, 2, u64::from(size));
            put_bytes(&mut m, 4, &item);
        }
    }
    for (key, value) in headers {
        let mut h = Vec::new();
        put_bytes(&mut h, 1, key.as_bytes());
        put_bytes(&mut h, 2, value.as_bytes());
        put_bytes(&mut m, 5, &h);
    }
    smart_http(6, &m)
}

/// The answer that gives nothing: `raw_resource_response { status =
/// NETWORK_REQUEST_TIMED_OUT }`, what Gadgetbridge sends when no handler
/// serves a request ("the phone has no network").
pub fn raw_timed_out() -> Vec<u8> {
    let mut m = Vec::new();
    put_uint(&mut m, 1, RAW_NETWORK_REQUEST_TIMED_OUT);
    smart_http(6, &m)
}

/// Same for a Connect IQ request: `connect_iq_http_response { status =
/// NETWORK_REQUEST_TIMED_OUT, http_status_code = 0 }`.
pub fn web_timed_out() -> Vec<u8> {
    let mut m = Vec::new();
    put_uint(&mut m, 1, WEB_NETWORK_REQUEST_TIMED_OUT);
    put_uint(&mut m, 2, 0);
    smart_http(2, &m)
}

fn smart_transfer(message_field: u64, message: &[u8]) -> Vec<u8> {
    let mut service = Vec::new();
    put_bytes(&mut service, message_field, message);
    let mut smart = Vec::new();
    put_bytes(&mut smart, SMART_DATA_TRANSFER, &service);
    smart
}

/// `Smart { data_transfer_service { data_download_response { status, id,
/// offset, payload } } }`.
pub fn data_download_response(
    status: u64,
    id: u32,
    offset: u32,
    payload: Option<&[u8]>,
) -> Vec<u8> {
    let mut m = Vec::new();
    put_uint(&mut m, 1, status);
    put_uint(&mut m, 2, u64::from(id));
    put_uint(&mut m, 3, u64::from(offset));
    if let Some(p) = payload {
        put_bytes(&mut m, 4, p);
    }
    smart_transfer(2, &m)
}

/// `Smart { data_transfer_service { file_access_item_response { status,
/// data_xfer_id } } }` (no item uid: only used for refusals).
pub fn file_access_response(status: u64, data_xfer_id: u32) -> Vec<u8> {
    let mut m = Vec::new();
    put_uint(&mut m, 1, status);
    put_uint(&mut m, 2, u64::from(data_xfer_id));
    smart_transfer(9, &m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duties::xfer::{STATUS_INVALID_ID, STATUS_SUCCESS};

    fn header(k: &str, v: &str) -> Vec<u8> {
        let mut h = Vec::new();
        put_bytes(&mut h, 1, k.as_bytes());
        put_bytes(&mut h, 2, v.as_bytes());
        h
    }

    fn smart_raw(raw: &[u8]) -> Vec<u8> {
        smart_http(5, raw)
    }

    #[test]
    fn a_raw_request_is_read_with_its_headers_in_lower_case() {
        let mut raw = Vec::new();
        put_bytes(&mut raw, 1, b"https://api.gcs.garmin.com/ephemeris/x?a=b");
        put_uint(&mut raw, 6, 1);
        put_bytes(&mut raw, 5, &header("If-None-Match", "\"abc\""));
        put_bytes(&mut raw, 5, &header("Accept", "application/x-tar"));
        let got = parse_smart(&smart_raw(&raw)).unwrap();
        let Incoming::Raw(r) = got else {
            panic!("{got:?}")
        };
        assert_eq!(r.url, "https://api.gcs.garmin.com/ephemeris/x?a=b");
        assert!(r.is_get() && r.use_data_xfer && !r.has_forms);
        assert_eq!(r.header("if-none-match"), Some("\"abc\""));
        assert_eq!(r.header("accept"), Some("application/x-tar"));
        assert_eq!(r.header("accept-encoding"), None);
    }

    #[test]
    fn method_and_forms_are_read() {
        let mut raw = Vec::new();
        put_bytes(&mut raw, 1, b"https://h/p");
        put_uint(&mut raw, 3, 3);
        put_bytes(&mut raw, 8, &[]);
        let Some(Incoming::Raw(r)) = parse_smart(&smart_raw(&raw)) else {
            panic!()
        };
        assert!(!r.is_get() && r.has_forms);
    }

    #[test]
    fn an_encrypted_request_is_unreadable() {
        // Only field 11 (encrypted_request): no url.
        let mut raw = Vec::new();
        put_bytes(&mut raw, 11, &[1, 2, 3, 4]);
        assert_eq!(parse_smart(&smart_raw(&raw)), Some(Incoming::Unreadable));
        // An http service with nothing in it.
        assert_eq!(
            parse_smart(&smart_http(99, &[])),
            Some(Incoming::Unreadable)
        );
    }

    #[test]
    fn other_http_requests_give_their_url() {
        let mut web = Vec::new();
        put_bytes(&mut web, 1, b"https://example.org/x");
        assert_eq!(
            parse_smart(&smart_http(1, &web)),
            Some(Incoming::Web {
                url: "https://example.org/x".into()
            })
        );
        let mut page = Vec::new();
        put_bytes(&mut page, 1, b"https://example.org/open");
        assert_eq!(
            parse_smart(&smart_http(11, &page)),
            Some(Incoming::Other {
                url: Some("https://example.org/open".into())
            })
        );
    }

    #[test]
    fn data_transfer_requests_are_read() {
        let mut req = Vec::new();
        put_uint(&mut req, 1, 65537);
        put_uint(&mut req, 2, 4096);
        put_uint(&mut req, 3, 2000);
        let smart = smart_transfer(1, &req);
        assert_eq!(
            parse_smart(&smart),
            Some(Incoming::DataDownload {
                id: 65537,
                offset: 4096,
                max_chunk: Some(2000)
            })
        );
        let mut req = Vec::new();
        put_uint(&mut req, 1, 9);
        put_uint(&mut req, 2, 0);
        assert_eq!(
            parse_smart(&smart_transfer(1, &req)),
            Some(Incoming::DataDownload {
                id: 9,
                offset: 0,
                max_chunk: None
            })
        );
        let mut req = Vec::new();
        put_uint(&mut req, 1, 77);
        put_uint(&mut req, 2, 1);
        assert_eq!(
            parse_smart(&smart_transfer(8, &req)),
            Some(Incoming::FileAccess { data_xfer_id: 77 })
        );
        // A download request without an id is not one.
        assert_eq!(parse_smart(&smart_transfer(1, &[])), None);
    }

    #[test]
    fn other_smart_content_and_malformed_bytes_are_not_ours() {
        assert_eq!(parse_smart(&[0x42, 0x02, 0x12, 0x00]), None);
        assert_eq!(parse_smart(&[]), None);
        assert_eq!(parse_smart(&[0x12, 0x09, 0x01]), None);
        for n in 0..8 {
            let mut raw = Vec::new();
            put_bytes(&mut raw, 1, b"https://h/p");
            let smart = smart_raw(&raw);
            // A prefix never panics.
            let _ = parse_smart(&smart[..smart.len().saturating_sub(n)]);
        }
    }

    #[test]
    fn urls_are_split() {
        let u =
            parse_url("https://API.gcs.garmin.com/ephemeris/cpe/sony/lle?coverage=WEEKS_1&c=GPS")
                .unwrap();
        assert_eq!(u.host, "api.gcs.garmin.com");
        assert_eq!(u.path, "/ephemeris/cpe/sony/lle");
        assert_eq!(u.query.as_deref(), Some("coverage=WEEKS_1&c=GPS"));
        assert_eq!(
            u.path_and_query(),
            "/ephemeris/cpe/sony/lle?coverage=WEEKS_1&c=GPS"
        );
        let u = parse_url("http://h.example:8080").unwrap();
        assert_eq!((u.host.as_str(), u.path.as_str()), ("h.example", "/"));
        assert_eq!(u.query, None);
        let u = parse_url("https://h/p#frag?x").unwrap();
        assert_eq!((u.path.as_str(), u.query), ("/p", None));
        assert_eq!(parse_url("ftp://h/p"), None);
        assert_eq!(parse_url("https://user:pw@h/p"), None);
        assert_eq!(parse_url("https:///p"), None);
        assert_eq!(parse_url("https://h:80x/p"), None);
        assert_eq!(parse_url("/ephemeris/x"), None);
        assert_eq!(parse_url(""), None);
    }

    #[test]
    fn log_forms_never_hold_a_query_or_a_number() {
        assert_eq!(loggable_host("API.gcs.garmin.com"), "API.gcs.garmin.com");
        assert_eq!(loggable_host("a b\n"), "ab");
        assert_eq!(loggable_host("\u{e9}!"), "?");
        assert_eq!(
            loggable_path("/ephemeris/cpe/sony/lle?x=1"),
            "/ephemeris/cpe/sony/lle"
        );
        assert_eq!(
            loggable_path("/weather/v2/55.676,12.568/forecast"),
            "/weather/v2/*/forecast"
        );
        assert_eq!(loggable_path("/a/b/c/d/e/f/g/h"), "/a/b/c/d/e/f");
        assert_eq!(loggable_path("/12345"), "/*");
        assert_eq!(loggable_path("/averyveryveryverylongsegmentname"), "/*");
        assert_eq!(loggable_path(""), "/");
        assert_eq!(loggable_path("?only=query"), "/");
    }

    #[test]
    fn answers_are_byte_exact() {
        // Smart { 2 { 6 { 1: 200 } } }
        assert_eq!(raw_timed_out(), [0x12, 0x05, 0x32, 0x03, 0x08, 0xC8, 0x01]);
        // Smart { 2 { 2 { 1: 300, 2: 0 } } }
        assert_eq!(
            web_timed_out(),
            [0x12, 0x07, 0x12, 0x05, 0x08, 0xAC, 0x02, 0x10, 0x00]
        );
        // 200, inline body "ab", one header (etag: "1").
        let got = raw_response(200, &[("etag".into(), "\"1\"".into())], Body::Inline(b"ab"));
        assert_eq!(
            got,
            [
                0x12, 0x18, 0x32, 0x16, 0x08, 0x64, 0x10, 0xC8, 0x01, 0x1A, 0x02, b'a', b'b', 0x2A,
                0x0B, 0x0A, 0x04, b'e', b't', b'a', b'g', 0x12, 0x03, b'"', b'1', b'"'
            ]
        );
        // With a pulled body: xfer_data 4 { id 300, size 70000 }, no headers.
        let got = raw_response(
            200,
            &[],
            Body::Xfer {
                id: 300,
                size: 70_000,
            },
        );
        assert_eq!(
            got,
            [
                0x12, 0x10, 0x32, 0x0E, 0x08, 0x64, 0x10, 0xC8, 0x01, 0x22, 0x07, 0x08, 0xAC, 0x02,
                0x10, 0xF0, 0xA2, 0x04
            ]
        );
        // DataDownloadResponse SUCCESS id 5 offset 0 payload 01 02.
        assert_eq!(
            data_download_response(STATUS_SUCCESS, 5, 0, Some(&[1, 2])),
            [0x3A, 0x0C, 0x12, 0x0A, 0x08, 0x01, 0x10, 0x05, 0x18, 0x00, 0x22, 0x02, 1, 2]
        );
        assert_eq!(
            data_download_response(STATUS_INVALID_ID, 5, 7, None),
            [0x3A, 0x08, 0x12, 0x06, 0x08, 0x02, 0x10, 0x05, 0x18, 0x07]
        );
        assert_eq!(
            file_access_response(FILE_ACCESS_UNKNOWN, 5),
            [0x3A, 0x06, 0x4A, 0x04, 0x08, 0x00, 0x10, 0x05]
        );
    }
}
