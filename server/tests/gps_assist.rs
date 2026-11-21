//! `GET /api/device/gps-assist` (stage 2f): the phone asks this server for the satellite file and
//! the server fetches it from Garmin. The upstream here is a local fake on 127.0.0.1; no test
//! reaches Garmin. Covers the device token, the path whitelist, fetch + cache + 304 + stale
//! revalidation, the kill switch, upstream failures, the 1 MB cap and redirects.

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, Method, Request, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use http_body_util::BodyExt;
use nimrag_server::gps_assist::{GpsAssistConfig, FRESH_S};
use nimrag_server::{api, db, tokens, users::Role};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

mod common;

const NOW: i64 = 1_800_000_000;
const PATH: &str =
    "/ephemeris/cpe/sony/lle?coverage=WEEKS_1&constellations=GPS,GLONASS,GALILEO,QZSS";

/// What the fake Garmin does, and what it saw.
#[derive(Default)]
struct Fake {
    /// `(status, etag, body, location)` of the next answers.
    status: u16,
    etag: Option<String>,
    body: Vec<u8>,
    location: Option<String>,
    /// Every request: path and query, and its If-None-Match.
    seen: Vec<(String, Option<String>)>,
}

type Shared = Arc<Mutex<Fake>>;

async fn fake_handler(State(f): State<Shared>, uri: Uri, headers: HeaderMap) -> Response {
    let mut f = f.lock().unwrap();
    f.seen.push((
        uri.path_and_query().unwrap().to_string(),
        headers
            .get(header::IF_NONE_MATCH)
            .map(|v| v.to_str().unwrap().to_string()),
    ));
    // 304 when the server revalidates a tag the fake still has.
    let inm = f.seen.last().unwrap().1.clone();
    if f.status == 200 && inm.is_some() && inm == f.etag {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    let mut r = Response::new(Body::from(f.body.clone()));
    *r.status_mut() = StatusCode::from_u16(f.status).unwrap();
    if let Some(t) = &f.etag {
        r.headers_mut().insert(header::ETAG, t.parse().unwrap());
    }
    if let Some(l) = &f.location {
        r.headers_mut().insert(header::LOCATION, l.parse().unwrap());
    }
    r
}

struct T {
    _dir: tempfile::TempDir,
    app: Router,
    token: String,
    clock: Arc<AtomicI64>,
    fake: Shared,
    data_dir: std::path::PathBuf,
}

async fn setup(enabled: bool) -> T {
    let fake: Shared = Arc::new(Mutex::new(Fake {
        status: 200,
        etag: Some("\"aaaa-1111\"".into()),
        body: b"tar-bytes".to_vec(),
        ..Default::default()
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let upstream = Router::new()
        .fallback(get(fake_handler))
        .with_state(fake.clone());
    tokio::spawn(async move { axum::serve(listener, upstream).await });

    let dir = tempfile::tempdir().unwrap();
    let pool = db::open(dir.path()).await.unwrap();
    let uid = common::make_user(&pool, common::USER, Role::User, true).await;
    let (_, token) = tokens::create(&pool, "phone", Some(uid)).await.unwrap();
    let clock = Arc::new(AtomicI64::new(NOW));
    let c = clock.clone();
    let state = api::AppState::new(pool, dir.path().to_path_buf(), api::MAX_BODY_BYTES)
        .with_clock(move || c.load(Ordering::SeqCst))
        .with_gps_assist(GpsAssistConfig {
            enabled,
            upstream: format!("http://127.0.0.1:{port}"),
        });
    T {
        data_dir: dir.path().to_path_buf(),
        _dir: dir,
        app: api::router(state),
        token,
        clock,
        fake,
    }
}

fn enc(path: &str) -> String {
    // The phone sends the value percent-encoded: everything but the unreserved characters.
    path.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

async fn ask(
    t: &T,
    path: &str,
    token: Option<&str>,
    inm: Option<&str>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut req = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/device/gps-assist?path={}", enc(path)));
    if let Some(tok) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {tok}"));
    }
    if let Some(i) = inm {
        req = req.header(header::IF_NONE_MATCH, i);
    }
    let resp = t
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

fn hits(t: &T) -> usize {
    t.fake.lock().unwrap().seen.len()
}

#[tokio::test]
async fn fetches_once_then_serves_from_the_cache() {
    let t = setup(true).await;
    let (s, h, body) = ask(&t, PATH, Some(&t.token), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body, b"tar-bytes");
    assert_eq!(h[header::ETAG], "\"aaaa-1111\"");
    assert_eq!(h[header::CONTENT_TYPE], "application/octet-stream");
    assert_eq!(h[header::AGE], "0");
    // The upstream saw the exact path and query, and no If-None-Match.
    assert_eq!(t.fake.lock().unwrap().seen, vec![(PATH.to_string(), None)]);

    // Three hours later: still the cache, with an Age.
    t.clock.store(NOW + 3 * 3600, Ordering::SeqCst);
    let (s, h, body) = ask(&t, PATH, Some(&t.token), None).await;
    assert_eq!((s, body.as_slice()), (StatusCode::OK, &b"tar-bytes"[..]));
    assert_eq!(h[header::AGE], "10800");
    assert_eq!(hits(&t), 1);
}

#[tokio::test]
async fn if_none_match_gives_304_with_the_validators() {
    let t = setup(true).await;
    let (s, ..) = ask(&t, PATH, Some(&t.token), None).await;
    assert_eq!(s, StatusCode::OK);
    for inm in [
        "\"aaaa-1111\"",
        "W/\"aaaa-1111\"",
        "\"x\", \"aaaa-1111\"",
        "*",
    ] {
        let (s, h, body) = ask(&t, PATH, Some(&t.token), Some(inm)).await;
        assert_eq!(s, StatusCode::NOT_MODIFIED, "{inm}");
        assert!(body.is_empty());
        assert_eq!(h[header::ETAG], "\"aaaa-1111\"");
    }
    let (s, ..) = ask(&t, PATH, Some(&t.token), Some("\"other\"")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(hits(&t), 1);
}

#[tokio::test]
async fn a_stale_copy_is_revalidated_and_replaced() {
    let t = setup(true).await;
    ask(&t, PATH, Some(&t.token), None).await;

    // Four hours on, upstream still has the same tag: it answers 304, the copy is refreshed.
    t.clock.store(NOW + FRESH_S, Ordering::SeqCst);
    let (s, h, body) = ask(&t, PATH, Some(&t.token), None).await;
    assert_eq!((s, body.as_slice()), (StatusCode::OK, &b"tar-bytes"[..]));
    assert_eq!(h[header::AGE], "0");
    assert_eq!(
        t.fake.lock().unwrap().seen[1],
        (PATH.to_string(), Some("\"aaaa-1111\"".to_string()))
    );
    // ... so it is fresh again for the next four hours.
    t.clock.store(NOW + FRESH_S + 3600, Ordering::SeqCst);
    ask(&t, PATH, Some(&t.token), None).await;
    assert_eq!(hits(&t), 2);

    // Garmin publishes a new file: the next revalidation replaces the copy.
    {
        let mut f = t.fake.lock().unwrap();
        f.etag = Some("\"bbbb-2222\"".into());
        f.body = b"new-tar".to_vec();
    }
    t.clock.store(NOW + 2 * FRESH_S + 10, Ordering::SeqCst);
    let (s, h, body) = ask(&t, PATH, Some(&t.token), None).await;
    assert_eq!((s, body.as_slice()), (StatusCode::OK, &b"new-tar"[..]));
    assert_eq!(h[header::ETAG], "\"bbbb-2222\"");
    // A phone holding the old tag gets the new file, not a 304.
    let (s, ..) = ask(&t, PATH, Some(&t.token), Some("\"aaaa-1111\"")).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn a_missing_upstream_etag_is_replaced_by_a_content_tag() {
    let t = setup(true).await;
    t.fake.lock().unwrap().etag = None;
    let (s, h, _) = ask(&t, PATH, Some(&t.token), None).await;
    assert_eq!(s, StatusCode::OK);
    let tag = h[header::ETAG].to_str().unwrap().to_string();
    assert!(
        tag.starts_with('"') && tag.ends_with('"') && tag.len() > 8,
        "{tag}"
    );
    let (s, ..) = ask(&t, PATH, Some(&t.token), Some(&tag)).await;
    assert_eq!(s, StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn the_kill_switch_makes_no_upstream_request() {
    let t = setup(false).await;
    let (s, ..) = ask(&t, PATH, Some(&t.token), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(hits(&t), 0);
    assert!(!t.data_dir.join("gps-assist").exists());
}

#[tokio::test]
async fn only_a_device_token_passes() {
    let t = setup(true).await;
    let (s, ..) = ask(&t, PATH, None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, ..) = ask(&t, PATH, Some("not-a-token"), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(hits(&t), 0);
}

#[tokio::test]
async fn paths_outside_the_whitelist_are_refused_before_any_request() {
    let t = setup(true).await;
    for bad in [
        "/",
        "/api/health",
        "/ephemeris",
        "ephemeris/cpe",
        "/ephemeris/../admin",
        "/ephemeris//x",
        "/ephemeris/%2e%2e/x",
        "/ephemeris/a%2Fb",
        "/ephemeris/a?b?c",
        "/ephemeris/a b",
        "/ephemeris/a#frag",
        "/ephemeris/a\\b",
        "/ephemeris/ä",
        "//evil.example/ephemeris/x",
    ] {
        let (s, ..) = ask(&t, bad, Some(&t.token), None).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
    }
    let long = format!("/ephemeris/{}", "a".repeat(600));
    let (s, ..) = ask(&t, &long, Some(&t.token), None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(hits(&t), 0);

    // A missing parameter is a client error too.
    let resp = t
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/device/gps-assist")
                .header(header::AUTHORIZATION, format!("Bearer {}", t.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upstream_trouble_is_a_clean_error_and_nothing_is_cached() {
    let t = setup(true).await;
    for (status, want) in [
        (500, StatusCode::BAD_GATEWAY),
        (403, StatusCode::BAD_GATEWAY),
        (404, StatusCode::NOT_FOUND),
        // A redirect is never followed (it could leave the fixed host).
        (302, StatusCode::BAD_GATEWAY),
    ] {
        {
            let mut f = t.fake.lock().unwrap();
            f.status = status;
            f.location = (status == 302).then(|| "http://127.0.0.1:1/elsewhere".into());
        }
        let (s, ..) = ask(&t, PATH, Some(&t.token), None).await;
        assert_eq!(s, want, "upstream {status}");
    }
    // An empty body is no answer either.
    {
        let mut f = t.fake.lock().unwrap();
        f.status = 200;
        f.location = None;
        f.body = vec![];
    }
    let (s, ..) = ask(&t, PATH, Some(&t.token), None).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    // Nothing was cached: the next good answer is fetched, not skipped.
    {
        let mut f = t.fake.lock().unwrap();
        f.body = b"ok".to_vec();
    }
    let before = hits(&t);
    let (s, _, body) = ask(&t, PATH, Some(&t.token), None).await;
    assert_eq!((s, body.as_slice()), (StatusCode::OK, &b"ok"[..]));
    assert_eq!(hits(&t), before + 1);
}

#[tokio::test]
async fn an_answer_over_one_megabyte_is_refused() {
    let t = setup(true).await;
    t.fake.lock().unwrap().body = vec![7; 1024 * 1024 + 1];
    let (s, ..) = ask(&t, PATH, Some(&t.token), None).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    // Exactly 1 MiB is fine.
    t.fake.lock().unwrap().body = vec![7; 1024 * 1024];
    let (s, _, body) = ask(&t, PATH, Some(&t.token), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body.len(), 1024 * 1024);
}

#[tokio::test]
async fn the_cache_lives_in_the_data_dir_and_names_no_path() {
    let t = setup(true).await;
    ask(&t, PATH, Some(&t.token), None).await;
    ask(&t, "/ephemeris/cpe/sony/other", Some(&t.token), None).await;
    let dir = t.data_dir.join("gps-assist");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names.len(), 4, "{names:?}");
    for n in &names {
        assert!(!n.contains("ephemeris") && !n.contains("GPS"), "{n}");
        assert!(n.ends_with(".bin") || n.ends_with(".json"), "{n}");
    }
    // The metadata holds the tag and a time, and not the path or its query.
    let json = names.iter().find(|n| n.ends_with(".json")).unwrap();
    let text = std::fs::read_to_string(dir.join(json)).unwrap();
    assert!(
        !text.contains("ephemeris") && !text.contains("coverage"),
        "{text}"
    );
}

#[tokio::test]
async fn the_cache_is_kept_to_a_few_entries() {
    let t = setup(true).await;
    for i in 0..40 {
        let (s, ..) = ask(&t, &format!("/ephemeris/p{i}"), Some(&t.token), None).await;
        assert_eq!(s, StatusCode::OK);
    }
    let n = std::fs::read_dir(t.data_dir.join("gps-assist"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|x| x == "json")
        })
        .count();
    assert!(n <= 32, "{n}");
}
