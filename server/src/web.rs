//! Serves the web interface (`web/dist`, built by `npm run build`) from the binary.
//!
//! * Release builds embed the files with `rust-embed`; debug builds read `../web/dist`
//!   from disk on each request, so `npm run build` is enough while developing.
//! * `web/dist` may be missing at compile time (`cargo test` without Node): the folder is
//!   `allow_missing`, and requests then get a small "web interface not built" page.
//! * Routing: `/api/*` and `/tiles/*` never fall back to the app (JSON / plain 404; the tile
//!   files themselves are served by `tiles.rs` when `--tiles-dir` is set). A path
//!   that names a file (has an extension) is served or is a 404. Any other path is a
//!   client-side route and gets `index.html` so React Router can take over.
//! * Caching: files under `assets/` have content hashes in their names and are cached for a
//!   year; `index.html`, the service worker and the manifest are `no-cache` so an update
//!   reaches the browser on the next load.

use crate::hex;
use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;
use std::borrow::Cow;

#[derive(RustEmbed)]
#[folder = "../web/dist"]
#[allow_missing = true]
struct Assets;

/// One file of the web app.
pub struct Asset {
    pub data: Cow<'static, [u8]>,
    /// Hex SHA-256 of the content; becomes the ETag.
    pub etag: String,
}

fn embedded(path: &str) -> Option<Asset> {
    Assets::get(path).map(|f| Asset {
        etag: hex(&f.metadata.sha256_hash()),
        data: f.data,
    })
}

/// Router fallback: everything that is not an API route.
pub async fn handler(method: Method, uri: Uri, headers: HeaderMap) -> Response {
    respond(&method, uri.path(), &headers, &embedded)
}

const INDEX: &str = "index.html";

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data: blob:; font-src 'self'; connect-src 'self'; worker-src 'self' blob:; \
child-src 'self' blob:; manifest-src 'self'; object-src 'none'; base-uri 'self'; \
form-action 'self'; frame-ancestors 'none'";

pub fn respond(
    method: &Method,
    path: &str,
    request_headers: &HeaderMap,
    lookup: &dyn Fn(&str) -> Option<Asset>,
) -> Response {
    if path == "/api" || path.starts_with("/api/") {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"error":"not found"}"#,
        )
            .into_response();
    }
    if path.starts_with("/tiles/") {
        return plain(StatusCode::NOT_FOUND, "no such tile file");
    }
    if method != Method::GET && method != Method::HEAD {
        return plain(StatusCode::METHOD_NOT_ALLOWED, "method not allowed");
    }

    let rel = path.trim_start_matches('/');
    let wants_file = rel
        .rsplit('/')
        .next()
        .is_some_and(|last| last.contains('.'));
    let (name, asset) = if rel.is_empty() {
        (INDEX, lookup(INDEX))
    } else if rel.split('/').any(|seg| seg == ".." || seg.is_empty()) {
        return plain(StatusCode::NOT_FOUND, "not found");
    } else if let Some(a) = lookup(rel) {
        (rel, Some(a))
    } else if wants_file {
        return plain(StatusCode::NOT_FOUND, "not found");
    } else {
        (INDEX, lookup(INDEX))
    };

    let Some(asset) = asset else {
        return not_built();
    };

    let etag = format!("\"{}\"", &asset.etag[..asset.etag.len().min(32)]);
    let mut res = Response::builder()
        .header(header::CONTENT_TYPE, content_type(name))
        .header(header::CACHE_CONTROL, cache_control(name))
        .header(header::ETAG, &etag)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::REFERRER_POLICY, "same-origin");
    if name == INDEX {
        res = res.header(header::CONTENT_SECURITY_POLICY, CSP);
    }

    let fresh = request_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .any(|t| t.trim().trim_start_matches("W/") == etag)
        });
    let body = if fresh {
        res = res.status(StatusCode::NOT_MODIFIED);
        Body::empty()
    } else if method == Method::HEAD {
        res = res.header(header::CONTENT_LENGTH, asset.data.len());
        Body::empty()
    } else {
        Body::from(asset.data.into_owned())
    };
    res.body(body)
        .unwrap_or_else(|_| plain(StatusCode::INTERNAL_SERVER_ERROR, "response error"))
}

fn plain(status: StatusCode, msg: &'static str) -> Response {
    (
        status,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        )],
        msg,
    )
        .into_response()
}

fn not_built() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Nimrag Connect</title>\
<body style=\"font-family:sans-serif;background:#0f1417;color:#e8edee;padding:2rem\">\
<h1>Web interface not built</h1>\
<p>This server binary was built without <code>web/dist</code>. Run <code>npm ci &amp;&amp; \
npm run build</code> in <code>web/</code> and rebuild the server (the Docker image does this).</p>\
<p>The API under <code>/api</code> works regardless.</p>",
    )
        .into_response()
}

/// Content type by file extension. Text types carry the charset.
pub fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// Hashed build output is immutable; entry points that must update promptly are not cached.
pub fn cache_control(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path);
    if path == INDEX
        || name == "sw.js"
        || name == "registerSW.js"
        || name == "manifest.webmanifest"
        || name.starts_with("workbox-")
    {
        "no-cache"
    } else if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=86400"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(path: &str) -> Option<Asset> {
        let body: &'static [u8] = match path {
            "index.html" => b"<!doctype html><div id=root></div>",
            "assets/index-abc123.js" => b"console.log(1)",
            "sw.js" => b"//sw",
            "icon.svg" => b"<svg/>",
            _ => return None,
        };
        Some(Asset {
            data: Cow::Borrowed(body),
            etag: crate::sha256_hex(body),
        })
    }

    fn get(path: &str) -> Response {
        respond(&Method::GET, path, &HeaderMap::new(), &fake)
    }

    fn h(r: &Response, name: header::HeaderName) -> &str {
        r.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    }

    #[test]
    fn root_and_spa_routes_get_index_with_no_cache() {
        for p in [
            "/",
            "/activities/12/strength",
            "/sleep/2026-11-18",
            "/__gallery",
        ] {
            let r = get(p);
            assert_eq!(r.status(), StatusCode::OK, "{p}");
            assert_eq!(h(&r, header::CONTENT_TYPE), "text/html; charset=utf-8");
            assert_eq!(h(&r, header::CACHE_CONTROL), "no-cache");
            assert!(h(&r, header::CONTENT_SECURITY_POLICY).contains("default-src 'self'"));
        }
    }

    #[test]
    fn hashed_assets_are_immutable_with_the_right_type() {
        let r = get("/assets/index-abc123.js");
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(
            h(&r, header::CONTENT_TYPE),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            h(&r, header::CACHE_CONTROL),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(h(&r, header::X_CONTENT_TYPE_OPTIONS), "nosniff");
    }

    #[test]
    fn service_worker_is_never_cached() {
        let r = get("/sw.js");
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(h(&r, header::CACHE_CONTROL), "no-cache");
        assert_eq!(cache_control("workbox-7c24d614.js"), "no-cache");
        assert_eq!(cache_control("manifest.webmanifest"), "no-cache");
    }

    #[test]
    fn missing_files_and_api_paths_do_not_fall_back_to_the_app() {
        assert_eq!(get("/assets/missing.js").status(), StatusCode::NOT_FOUND);
        assert_eq!(get("/favicon.ico").status(), StatusCode::NOT_FOUND);
        assert_eq!(get("/api/nope").status(), StatusCode::NOT_FOUND);
        assert_eq!(get("/api").status(), StatusCode::NOT_FOUND);
        assert_eq!(get("/tiles/map.pmtiles").status(), StatusCode::NOT_FOUND);
        assert_eq!(
            h(&get("/api/nope"), header::CONTENT_TYPE),
            "application/json"
        );
        assert_eq!(get("/a/../b").status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn etag_gives_304_and_head_has_no_body() {
        let r = get("/icon.svg");
        let etag = h(&r, header::ETAG).to_string();
        let mut hdr = HeaderMap::new();
        hdr.insert(header::IF_NONE_MATCH, etag.parse().unwrap());
        let r = respond(&Method::GET, "/icon.svg", &hdr, &fake);
        assert_eq!(r.status(), StatusCode::NOT_MODIFIED);
        let r = respond(&Method::HEAD, "/icon.svg", &HeaderMap::new(), &fake);
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(h(&r, header::CONTENT_LENGTH), "6");
    }

    #[test]
    fn other_methods_are_rejected() {
        let r = respond(&Method::POST, "/", &HeaderMap::new(), &fake);
        assert_eq!(r.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn without_a_build_the_app_paths_say_so() {
        let none = |_: &str| None;
        let r = respond(&Method::GET, "/", &HeaderMap::new(), &none);
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
        let r = respond(&Method::GET, "/anything/else", &HeaderMap::new(), &none);
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn content_types() {
        assert_eq!(content_type("a.css"), "text/css; charset=utf-8");
        assert_eq!(content_type("a.woff2"), "font/woff2");
        assert_eq!(
            content_type("manifest.webmanifest"),
            "application/manifest+json"
        );
        assert_eq!(content_type("icon.svg"), "image/svg+xml");
        assert_eq!(content_type("icon-192.png"), "image/png");
    }

    #[test]
    fn embedded_lookup_does_not_panic_without_a_build() {
        // Whether or not web/dist exists, an unknown file is simply absent.
        assert!(embedded("definitely-not-a-file.xyz").is_none());
    }
}
