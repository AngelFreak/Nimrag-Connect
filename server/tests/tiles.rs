//! `/tiles/<name>.pmtiles` (`serve --tiles-dir`): whole file, single ranges (206), HEAD, 416,
//! validators, path traversal, missing files and the sign-in requirement, through the real router.
mod common;

use axum::body::Body;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use tower::ServiceExt;

const SIZE: usize = 1000;

fn content() -> Vec<u8> {
    (0..SIZE).map(|i| (i % 251) as u8).collect()
}

/// A tiles directory with `map.pmtiles` (1000 bytes of a known pattern) and some files that must
/// never be served, and `secret.pmtiles` next to the directory (outside it).
fn tiles_dir() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("tiles");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("map.pmtiles"), content()).unwrap();
    std::fs::write(dir.join("notes.txt"), "not a tile file").unwrap();
    std::fs::write(dir.join(".hidden.pmtiles"), "hidden").unwrap();
    std::fs::create_dir(dir.join("folder.pmtiles")).unwrap();
    std::fs::write(root.path().join("secret.pmtiles"), "outside the directory").unwrap();
    (root, dir)
}

async fn send(
    e: &Env,
    method: Method,
    uri: &str,
    cookie: Option<&str>,
    headers: &[(&str, &str)],
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = e
        .app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (status, headers) = (resp.status(), resp.headers().clone());
    let body = resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, headers, body)
}

async fn get(
    e: &Env,
    cookie: &str,
    uri: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, HeaderMap, Vec<u8>) {
    send(e, Method::GET, uri, Some(cookie), headers).await
}

fn h<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

async fn setup() -> (tempfile::TempDir, std::path::PathBuf, Env, String) {
    let (root, dir) = tiles_dir();
    let e = env_with_tiles(&dir).await;
    let c = e.login().await;
    (root, dir, e, c)
}

#[tokio::test]
async fn the_whole_file_with_its_validators() {
    let (_root, _dir, e, c) = setup().await;
    let (s, hd, body) = get(&e, &c, "/tiles/map.pmtiles", &[]).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body, content());
    assert_eq!(h(&hd, "content-length"), "1000");
    assert_eq!(h(&hd, "accept-ranges"), "bytes");
    assert_eq!(h(&hd, "content-type"), "application/octet-stream");
    assert!(h(&hd, "etag").starts_with('"') && h(&hd, "etag").ends_with('"'));
    assert!(
        h(&hd, "last-modified").ends_with(" GMT"),
        "{}",
        h(&hd, "last-modified")
    );
    assert_eq!(h(&hd, "cache-control"), "private, no-cache");
    assert_eq!(h(&hd, "x-content-type-options"), "nosniff");
}

#[tokio::test]
async fn single_ranges_give_206_with_content_range() {
    let (_root, _dir, e, c) = setup().await;
    let all = content();
    for (range, from, to) in [
        ("bytes=0-15", 0, 15), // what the web app probes with
        ("bytes=10-19", 10, 19),
        ("bytes=990-", 990, 999),
        ("bytes=995-5000", 995, 999), // the end is clamped
        ("bytes=-5", 995, 999),
        ("bytes=0-0", 0, 0),
        ("bytes=999-999", 999, 999),
    ] {
        let (s, hd, body) = get(&e, &c, "/tiles/map.pmtiles", &[("range", range)]).await;
        assert_eq!(s, StatusCode::PARTIAL_CONTENT, "{range}");
        assert_eq!(
            h(&hd, "content-range"),
            format!("bytes {from}-{to}/1000"),
            "{range}"
        );
        assert_eq!(
            h(&hd, "content-length"),
            (to - from + 1).to_string(),
            "{range}"
        );
        assert_eq!(h(&hd, "accept-ranges"), "bytes");
        assert_eq!(body, all[from..=to], "{range}");
    }
    // Not a range this server understands: the whole file.
    for range in ["items=0-5", "bytes=5-3", "bytes=x-y", "nonsense"] {
        let (s, hd, body) = get(&e, &c, "/tiles/map.pmtiles", &[("range", range)]).await;
        assert_eq!(s, StatusCode::OK, "{range}");
        assert!(hd.get("content-range").is_none());
        assert_eq!(body.len(), SIZE, "{range}");
    }
}

#[tokio::test]
async fn an_unsatisfiable_range_is_416() {
    let (_root, _dir, e, c) = setup().await;
    for range in [
        "bytes=1000-",
        "bytes=5000-6000",
        "bytes=-0",
        "bytes=0-1,5-6",
    ] {
        let (s, hd, body) = get(&e, &c, "/tiles/map.pmtiles", &[("range", range)]).await;
        assert_eq!(s, StatusCode::RANGE_NOT_SATISFIABLE, "{range}");
        assert_eq!(h(&hd, "content-range"), "bytes */1000", "{range}");
        assert!(body.is_empty(), "{range}");
    }
}

#[tokio::test]
async fn head_has_the_headers_and_no_body() {
    let (_root, _dir, e, c) = setup().await;
    let (s, hd, body) = send(&e, Method::HEAD, "/tiles/map.pmtiles", Some(&c), &[]).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h(&hd, "content-length"), "1000");
    assert_eq!(h(&hd, "accept-ranges"), "bytes");
    assert!(!h(&hd, "etag").is_empty());
    assert!(body.is_empty());
    let (s, hd, body) = send(
        &e,
        Method::HEAD,
        "/tiles/map.pmtiles",
        Some(&c),
        &[("range", "bytes=10-19")],
    )
    .await;
    assert_eq!(s, StatusCode::PARTIAL_CONTENT);
    assert_eq!(h(&hd, "content-range"), "bytes 10-19/1000");
    assert_eq!(h(&hd, "content-length"), "10");
    assert!(body.is_empty());
    let (s, _, _) = send(&e, Method::HEAD, "/tiles/missing.pmtiles", Some(&c), &[]).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Only GET and HEAD.
    let (s, _, _) = send(
        &e,
        Method::POST,
        "/tiles/map.pmtiles",
        Some(&c),
        &[("x-nimrag", "1")],
    )
    .await;
    assert_eq!(s, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn validators_are_honoured() {
    let (_root, dir, e, c) = setup().await;
    let (_, hd, _) = get(&e, &c, "/tiles/map.pmtiles", &[]).await;
    let etag = h(&hd, "etag").to_owned();
    let modified = h(&hd, "last-modified").to_owned();

    let (s, hd, body) = get(&e, &c, "/tiles/map.pmtiles", &[("if-none-match", &etag)]).await;
    assert_eq!(s, StatusCode::NOT_MODIFIED);
    assert_eq!(h(&hd, "etag"), etag);
    assert!(body.is_empty());
    let (s, _, _) = get(
        &e,
        &c,
        "/tiles/map.pmtiles",
        &[("if-none-match", "\"other\"")],
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = get(
        &e,
        &c,
        "/tiles/map.pmtiles",
        &[("if-modified-since", &modified)],
    )
    .await;
    assert_eq!(s, StatusCode::NOT_MODIFIED);

    // A client that pins the version it started with (the PMTiles library does) learns of a swap.
    let (s, _, _) = get(
        &e,
        &c,
        "/tiles/map.pmtiles",
        &[("if-match", &etag), ("range", "bytes=0-3")],
    )
    .await;
    assert_eq!(s, StatusCode::PARTIAL_CONTENT);
    let (s, _, _) = get(
        &e,
        &c,
        "/tiles/map.pmtiles",
        &[("if-match", "\"old\""), ("range", "bytes=0-3")],
    )
    .await;
    assert_eq!(s, StatusCode::PRECONDITION_FAILED);

    // If-Range: the range only for the same version, else the whole file.
    let (s, _, body) = get(
        &e,
        &c,
        "/tiles/map.pmtiles",
        &[("if-range", &etag), ("range", "bytes=0-3")],
    )
    .await;
    assert_eq!((s, body.len()), (StatusCode::PARTIAL_CONTENT, 4));
    let (s, _, body) = get(
        &e,
        &c,
        "/tiles/map.pmtiles",
        &[("if-range", "\"old\""), ("range", "bytes=0-3")],
    )
    .await;
    assert_eq!((s, body.len()), (StatusCode::OK, SIZE));
    let (s, _, body) = get(
        &e,
        &c,
        "/tiles/map.pmtiles",
        &[("if-range", &modified), ("range", "bytes=0-3")],
    )
    .await;
    assert_eq!((s, body.len()), (StatusCode::PARTIAL_CONTENT, 4));

    // A replaced file has another ETag.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(dir.join("map.pmtiles"), vec![7u8; 10]).unwrap();
    let (s, hd, body) = get(&e, &c, "/tiles/map.pmtiles", &[("if-none-match", &etag)]).await;
    assert_eq!((s, body.len()), (StatusCode::OK, 10));
    assert_ne!(h(&hd, "etag"), etag);
}

#[tokio::test]
async fn names_cannot_leave_the_directory() {
    let (_root, _dir, e, c) = setup().await;
    for uri in [
        "/tiles/../secret.pmtiles",
        "/tiles/..%2Fsecret.pmtiles",
        "/tiles/%2e%2e%2fsecret.pmtiles",
        "/tiles/%2E%2E/secret.pmtiles",
        "/tiles/..%5Csecret.pmtiles",
        "/tiles/sub/map.pmtiles",
        "/tiles/map.pmtiles/",
        "/tiles//map.pmtiles",
        "/tiles/.hidden.pmtiles",
        "/tiles/notes.txt",
        "/tiles/map",
        "/tiles/folder.pmtiles",
        "/tiles/%00map.pmtiles",
        "/tiles/map.pmtiles%00",
    ] {
        let (s, _, body) = get(&e, &c, uri, &[]).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{uri}");
        assert_ne!(body, content(), "{uri}");
        assert!(!String::from_utf8_lossy(&body).contains("outside"), "{uri}");
    }
}

#[tokio::test]
async fn missing_files_and_directories_are_404() {
    let (_root, _dir, e, c) = setup().await;
    let (s, _, _) = get(&e, &c, "/tiles/nothing.pmtiles", &[]).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // The directory itself is missing.
    let gone = tempfile::tempdir().unwrap().path().join("not-there");
    let e2 = env_with_tiles(&gone).await;
    let c2 = e2.login().await;
    let (s, _, _) = get(&e2, &c2, "/tiles/map.pmtiles", &[]).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // No --tiles-dir at all: the route does not exist.
    let e3 = env().await;
    let c3 = e3.login().await;
    let (s, _, _) = get(&e3, &c3, "/tiles/map.pmtiles", &[]).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn only_signed_in_users_get_tiles() {
    let (_root, _dir, e, c) = setup().await;
    let (s, _, _) = send(&e, Method::GET, "/tiles/map.pmtiles", None, &[]).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = send(&e, Method::HEAD, "/tiles/map.pmtiles", None, &[]).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // A device token is not a session.
    let bearer = format!("Bearer {}", e.token);
    let (s, _, _) = send(
        &e,
        Method::GET,
        "/tiles/map.pmtiles",
        None,
        &[("authorization", &bearer)],
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = get(&e, &c, "/tiles/map.pmtiles", &[]).await;
    assert_eq!(s, StatusCode::OK);
    // Signing in comes first: a signed-out visitor learns nothing about which files exist.
    let (s, _, _) = send(&e, Method::GET, "/tiles/missing.pmtiles", None, &[]).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

/// A range far into a file larger than memory-friendly sizes comes back from a seek, not from
/// reading the file from the start (the file is sparse: it costs no disk).
#[tokio::test]
async fn a_range_deep_in_a_big_file_is_a_seek() {
    let (_root, dir) = tiles_dir();
    let big = dir.join("big.pmtiles");
    const AT: u64 = 5 << 30; // 5 GiB
    {
        let mut f = std::fs::File::create(&big).unwrap();
        f.set_len(AT + 4096).unwrap();
        f.seek(SeekFrom::Start(AT + 100)).unwrap();
        f.write_all(b"PMTILES-MARKER").unwrap();
    }
    let e = env_with_tiles(&dir).await;
    let c = e.login().await;
    let range = format!("bytes={}-{}", AT + 100, AT + 113);
    let (s, hd, body) = get(&e, &c, "/tiles/big.pmtiles", &[("range", &range)]).await;
    assert_eq!(s, StatusCode::PARTIAL_CONTENT);
    assert_eq!(body, b"PMTILES-MARKER");
    assert_eq!(
        h(&hd, "content-range"),
        format!("bytes {}-{}/{}", AT + 100, AT + 113, AT + 4096)
    );
    let (s, hd, body) = send(&e, Method::HEAD, "/tiles/big.pmtiles", Some(&c), &[]).await;
    assert_eq!((s, body.len()), (StatusCode::OK, 0));
    assert_eq!(h(&hd, "content-length"), (AT + 4096).to_string());
    let past = format!("bytes={}-", AT + 4096);
    let (s, _, _) = get(&e, &c, "/tiles/big.pmtiles", &[("range", &past)]).await;
    assert_eq!(s, StatusCode::RANGE_NOT_SATISFIABLE);
}

/// Through a real socket and hyper (the other tests call the router directly): the stream's
/// length matches `Content-Length`, HEAD has no body, a range over the wire is a 206.
#[tokio::test]
async fn over_a_real_connection() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (_root, _dir, e, _) = setup().await;
    let session = nimrag_server::auth::create_session(&e.pool, e.user_id, None, e.now())
        .await
        .unwrap();
    let cookie = format!("{}={session}", nimrag_server::auth::cookie_name(false));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = e.app.clone();
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    let raw = |method: &'static str, range: Option<&'static str>| {
        let cookie = cookie.clone();
        async move {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            let range = range.map_or(String::new(), |r| format!("Range: {r}\r\n"));
            let req = format!(
                "{method} /tiles/map.pmtiles HTTP/1.1\r\nHost: x\r\nCookie: {cookie}\r\n{range}Connection: close\r\n\r\n"
            );
            s.write_all(req.as_bytes()).await.unwrap();
            let mut out = Vec::new();
            s.read_to_end(&mut out).await.unwrap();
            let at = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
            let head = String::from_utf8_lossy(&out[..at]).to_lowercase();
            (head, out[at + 4..].to_vec())
        }
    };
    let (head, body) = raw("GET", Some("bytes=0-15")).await;
    assert!(head.starts_with("http/1.1 206"), "{head}");
    assert!(head.contains("content-range: bytes 0-15/1000"), "{head}");
    assert_eq!(body, content()[..16]);
    let (head, body) = raw("GET", None).await;
    assert!(
        head.starts_with("http/1.1 200") && head.contains("content-length: 1000"),
        "{head}"
    );
    assert_eq!(body, content());
    let (head, body) = raw("HEAD", None).await;
    assert!(
        head.starts_with("http/1.1 200") && head.contains("content-length: 1000"),
        "{head}"
    );
    assert!(body.is_empty());
    let (head, _) = raw("GET", Some("bytes=2000-")).await;
    assert!(head.starts_with("http/1.1 416"), "{head}");
    server.abort();
}

/// The binary: `--tiles-dir` and `NIMRAG_TILES_DIR` turn the route on (a signed-out request is
/// then 401, not the plain 404 of a server without tiles).
#[test]
fn the_flag_and_the_environment_variable_turn_the_route_on() {
    use std::io::{Read, Write};
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let (_root, dir) = tiles_dir();
    let status_of = |flag: Option<&str>, env: Option<&Path>| -> String {
        let data = tempfile::tempdir().unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_nimrag-server"));
        cmd.args([
            "serve",
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--data-dir",
        ])
        .arg(data.path())
        .env_remove("NIMRAG_TILES_DIR")
        .stderr(std::process::Stdio::null());
        if let Some(f) = flag {
            cmd.args(["--tiles-dir", f]);
        }
        if let Some(e) = env {
            cmd.env("NIMRAG_TILES_DIR", e);
        }
        let _child = Child(cmd.spawn().unwrap());
        for _ in 0..100 {
            if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                s.write_all(
                    b"GET /tiles/map.pmtiles HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
                let mut out = String::new();
                s.read_to_string(&mut out).unwrap();
                return out.lines().next().unwrap_or("").to_owned();
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("server did not start");
    };
    assert_eq!(status_of(None, None), "HTTP/1.1 404 Not Found");
    assert_eq!(status_of(dir.to_str(), None), "HTTP/1.1 401 Unauthorized");
    assert_eq!(status_of(None, Some(&dir)), "HTTP/1.1 401 Unauthorized");
}
