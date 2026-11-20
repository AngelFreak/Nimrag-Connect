//! Map tiles: `GET` and `HEAD /tiles/<name>.pmtiles` serve a PMTiles file from the directory
//! given with `serve --tiles-dir DIR` (or `NIMRAG_TILES_DIR`). The route exists only when the
//! option is set; without it `/tiles/*` is a plain 404 (see `web.rs`) and the web app draws a
//! route without a map.
//!
//! * Signed-in users only (the session cookie, either role, like the rest of the app); a device
//!   token or no session gets 401. The map is the owner's own, there is no reason to serve it to
//!   the open network.
//! * Plain file names only: `<letters, digits, - _ .>.pmtiles`, no leading dot. Anything else,
//!   a missing directory, a missing file and anything that is not a regular file is a 404; a
//!   path cannot leave the directory.
//! * A PMTiles client reads a file in many small ranges, so the files (hundreds of MB) are never
//!   loaded: the file is opened, seeked to the range's start and streamed for its length.
//!   `Range: bytes=a-b`, `bytes=a-` and `bytes=-n` give 206 with `Content-Range`; a range past the
//!   end gives 416 with `Content-Range: bytes */size`; several ranges in one request are not
//!   supported and give 416 too; a `Range` of another unit or one that is not well formed is
//!   ignored (the whole file, 200). Every response has `Accept-Ranges: bytes`.
//! * `ETag` (size and modification time) and `Last-Modified` come from the file's metadata.
//!   `If-Match` (412), `If-None-Match` and `If-Modified-Since` (304) and `If-Range` (the range
//!   only while the validator matches, else the whole file) are honoured, so a client that
//!   sends the ETag back notices a replaced file. `Cache-Control: private, no-cache`.

use crate::api::AppState;
use crate::auth::AuthUser;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::Response,
};
use std::io::SeekFrom;
use std::path::Path as FsPath;
use std::time::UNIX_EPOCH;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

const EXTENSION: &str = ".pmtiles";
const MAX_NAME_LEN: usize = 128;
/// Read size when streaming a range.
const CHUNK: usize = 64 * 1024;

/// The route handler (registered by `api::router` when a tiles directory is set).
pub async fn tile(
    _user: AuthUser,
    State(state): State<AppState>,
    method: Method,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    match state.tiles_dir.as_deref() {
        Some(dir) => serve(dir, &method, &name, &headers).await,
        None => not_found(),
    }
}

/// A file name this module serves: `<name>.pmtiles` of ASCII letters, digits, `-`, `_` and `.`,
/// not starting with a dot. (The name arrives percent-decoded, so `..%2F` is `../` here.)
pub fn valid_name(name: &str) -> bool {
    name.len() > EXTENSION.len()
        && name.len() <= MAX_NAME_LEN
        && name.ends_with(EXTENSION)
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// What a `Range` header asks for, against a file of a given size.
#[derive(Debug, PartialEq, Eq)]
pub enum RangeReq {
    /// Serve the whole file (no header, another unit, or not well formed).
    Whole,
    /// Serve bytes `start..=end` (both inside the file).
    Bytes(u64, u64),
    /// Nothing of the file is in the range, or several ranges were asked for.
    Unsatisfiable,
}

/// Interpret a `Range` header value for a file of `size` bytes (single ranges only).
pub fn parse_range(value: &str, size: u64) -> RangeReq {
    let value = value.trim();
    let Some((unit, spec)) = value.split_once('=') else {
        return RangeReq::Whole;
    };
    if !unit.trim().eq_ignore_ascii_case("bytes") {
        return RangeReq::Whole;
    }
    let spec = spec.trim();
    if spec.contains(',') {
        return RangeReq::Unsatisfiable;
    }
    let Some((first, last)) = spec.split_once('-') else {
        return RangeReq::Whole;
    };
    let (first, last) = (first.trim(), last.trim());
    if first.is_empty() {
        // The last `n` bytes.
        let Ok(n) = last.parse::<u64>() else {
            return RangeReq::Whole;
        };
        if n == 0 || size == 0 {
            return RangeReq::Unsatisfiable;
        }
        return RangeReq::Bytes(size - n.min(size), size - 1);
    }
    let Ok(start) = first.parse::<u64>() else {
        return RangeReq::Whole;
    };
    let end = if last.is_empty() {
        u64::MAX
    } else {
        match last.parse::<u64>() {
            Ok(end) if end >= start => end,
            _ => return RangeReq::Whole,
        }
    };
    if start >= size {
        return RangeReq::Unsatisfiable;
    }
    RangeReq::Bytes(start, end.min(size - 1))
}

/// Serve `name` from `dir` for a GET or HEAD request.
pub async fn serve(dir: &FsPath, method: &Method, name: &str, req: &HeaderMap) -> Response {
    if !valid_name(name) {
        return not_found();
    }
    let path = dir.join(name);
    // Look first (a directory or a pipe is not a tile file), then open, and read the size and
    // the time from the opened file so they belong to the bytes served.
    match tokio::fs::metadata(&path).await {
        Ok(m) if m.is_file() => {}
        _ => return not_found(),
    }
    let Ok(mut file) = tokio::fs::File::open(&path).await else {
        return not_found();
    };
    let Ok(meta) = file.metadata().await else {
        return not_found();
    };
    if !meta.is_file() {
        return not_found();
    }
    let size = meta.len();
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .unwrap_or_default();
    let etag = format!(
        "\"{:x}-{:x}-{:x}\"",
        size,
        modified.as_secs(),
        modified.subsec_nanos()
    );
    let last_modified = http_date(modified.as_secs());

    let base = |status: StatusCode| {
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::ETAG, &etag)
            .header(header::LAST_MODIFIED, &last_modified)
            .header(header::CACHE_CONTROL, "private, no-cache")
            .header(header::VARY, "Cookie")
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
    };
    let finish =
        |b: axum::http::response::Builder, body: Body| b.body(body).unwrap_or_else(|_| not_found());

    // Conditional requests (RFC 9110 section 13.2.2).
    let text = |name| req.get(name).and_then(|v| v.to_str().ok());
    if let Some(v) = text(header::IF_MATCH) {
        if !etag_matches(v, &etag, true) {
            return finish(base(StatusCode::PRECONDITION_FAILED), Body::empty());
        }
    }
    let not_modified = match text(header::IF_NONE_MATCH) {
        Some(v) => etag_matches(v, &etag, false),
        None => text(header::IF_MODIFIED_SINCE)
            .and_then(parse_http_date)
            .is_some_and(|since| modified.as_secs() <= since),
    };
    if not_modified {
        return finish(base(StatusCode::NOT_MODIFIED), Body::empty());
    }

    // The range counts only while `If-Range` (when sent) still names this version of the file.
    let range_applies = text(header::IF_RANGE).is_none_or(|v| {
        let v = v.trim();
        if v.starts_with('"') {
            v == etag
        } else {
            parse_http_date(v).is_some_and(|d| d == modified.as_secs())
        }
    });
    let wanted = match text(header::RANGE) {
        Some(v) if range_applies => parse_range(v, size),
        _ => RangeReq::Whole,
    };
    let (start, len, builder) = match wanted {
        RangeReq::Whole => (0, size, base(StatusCode::OK)),
        RangeReq::Bytes(a, b) => (
            a,
            b - a + 1,
            base(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_RANGE, format!("bytes {a}-{b}/{size}")),
        ),
        RangeReq::Unsatisfiable => {
            return finish(
                base(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                    .header(header::CONTENT_LENGTH, 0),
                Body::empty(),
            );
        }
    };
    let builder = builder.header(header::CONTENT_LENGTH, len);
    if *method == Method::HEAD || len == 0 {
        return finish(builder, Body::empty());
    }
    if start > 0 {
        if let Err(e) = file.seek(SeekFrom::Start(start)).await {
            tracing::warn!(error = %e, "seeking a tile file");
            return plain(
                StatusCode::INTERNAL_SERVER_ERROR,
                "cannot read the tile file",
            );
        }
    }
    finish(
        builder,
        Body::from_stream(ReaderStream::with_capacity(file.take(len), CHUNK)),
    )
}

/// `If-Match` (`strong`) and `If-None-Match` (weak comparison) against our ETag.
fn etag_matches(header: &str, etag: &str, strong: bool) -> bool {
    header.split(',').map(str::trim).any(|t| {
        if t == "*" {
            return true;
        }
        if strong {
            t == etag
        } else {
            t.strip_prefix("W/").unwrap_or(t) == etag
        }
    })
}

/// `Sun, 06 Nov 1994 08:49:37 GMT`.
fn http_date(secs: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(i64::try_from(secs).unwrap_or(0), 0)
        .unwrap_or_default()
        .format("%a, %d %b %Y %H:%M:%S GMT")
        .to_string()
}

fn parse_http_date(s: &str) -> Option<u64> {
    let t = chrono::DateTime::parse_from_rfc2822(s.trim())
        .ok()?
        .timestamp();
    u64::try_from(t).ok()
}

fn plain(status: StatusCode, msg: &'static str) -> Response {
    let mut r = Response::new(Body::from(msg));
    *r.status_mut() = status;
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    r
}

fn not_found() -> Response {
    plain(StatusCode::NOT_FOUND, "no such tile file")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_pmtiles_names_are_served() {
        for ok in [
            "map.pmtiles",
            "denmark-2026_11.pmtiles",
            "a.b.pmtiles",
            "A1.pmtiles",
        ] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in [
            "",
            ".pmtiles",
            "map",
            "map.pmtiles.bak",
            "map.PMTILES",
            "../map.pmtiles",
            "a/b.pmtiles",
            "a\\b.pmtiles",
            ".hidden.pmtiles",
            "..pmtiles",
            "map\0.pmtiles",
            "m ap.pmtiles",
            "kort%2F.pmtiles",
            "kø.pmtiles",
        ] {
            assert!(!valid_name(bad), "{bad:?}");
        }
        assert!(!valid_name(&format!("{}.pmtiles", "a".repeat(200))));
    }

    #[test]
    fn ranges_are_read_against_the_size() {
        use RangeReq::*;
        let p = |v: &str| parse_range(v, 100);
        assert_eq!(p("bytes=0-15"), Bytes(0, 15));
        assert_eq!(p("bytes=10-"), Bytes(10, 99));
        assert_eq!(p("bytes=90-500"), Bytes(90, 99), "the end is clamped");
        assert_eq!(p("bytes=-10"), Bytes(90, 99));
        assert_eq!(p("bytes=-500"), Bytes(0, 99));
        assert_eq!(p(" Bytes = 5 - 6 "), Bytes(5, 6));
        assert_eq!(p("bytes=100-"), Unsatisfiable);
        assert_eq!(p("bytes=200-300"), Unsatisfiable);
        assert_eq!(p("bytes=-0"), Unsatisfiable);
        assert_eq!(
            p("bytes=0-1,5-6"),
            Unsatisfiable,
            "several ranges are not supported"
        );
        assert_eq!(p("items=0-1"), Whole);
        assert_eq!(p("bytes=5-3"), Whole);
        assert_eq!(p("bytes=a-b"), Whole);
        assert_eq!(p("bytes=-"), Whole);
        assert_eq!(p("garbage"), Whole);
        assert_eq!(
            parse_range("bytes=0-0", 0),
            Unsatisfiable,
            "nothing in an empty file"
        );
        assert_eq!(parse_range("bytes=-5", 0), Unsatisfiable);
    }

    #[test]
    fn validators() {
        assert!(etag_matches("\"a\", \"b\"", "\"b\"", true));
        assert!(!etag_matches("W/\"b\"", "\"b\"", true));
        assert!(etag_matches("W/\"b\"", "\"b\"", false));
        assert!(etag_matches("*", "\"b\"", true));
        assert!(!etag_matches("\"c\"", "\"b\"", false));
        assert_eq!(http_date(784_111_777), "Sun, 06 Nov 1994 08:49:37 GMT");
        assert_eq!(
            parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(784_111_777)
        );
        assert_eq!(parse_http_date("yesterday"), None);
    }
}
