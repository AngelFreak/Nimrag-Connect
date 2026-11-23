//! Weather for the watch: `POST /api/device/weather`, the Open-Meteo client and its cache.
//! Every test talks to a local fake Open-Meteo (an axum server on 127.0.0.1:0 that serves a
//! real answer saved in `fixtures/open-meteo-forecast.json`); nothing reaches the real service.
//! Writing the FIT bytes needs the FIT profile, which is not in every build: the tests that
//! need it branch on `weather::available()` and the others check the clear 503 instead.

mod common;

use axum::{
    body::Body,
    extract::State,
    http::{header, Method, Request, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use base64::Engine;
use common::*;
use http_body_util::BodyExt;
use nimrag_core::client::WeatherCell;
use nimrag_core::fit::{
    decode, encode::records_to_file, interpret::interpret, weather as fit_weather,
};
use nimrag_server::users::Role;
use nimrag_server::weather::{Weather, WeatherConfig, WeatherError, CACHE_TTL_S};
use nimrag_server::{api, db, tokens};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;

const ANSWER: &str = include_str!("fixtures/open-meteo-forecast.json");
const START: i64 = 1_800_000_000;

// ---- a fake Open-Meteo -------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Mode {
    Ok,
    Status(u16),
    Garbage,
    /// Answer only after this many milliseconds.
    Slow(u64),
}

struct FakeState {
    mode: Mode,
    /// The query string of every request that arrived.
    queries: Vec<String>,
}

#[derive(Clone)]
struct Fake {
    url: String,
    st: Arc<Mutex<FakeState>>,
}

impl Fake {
    async fn start() -> Fake {
        let st = Arc::new(Mutex::new(FakeState {
            mode: Mode::Ok,
            queries: vec![],
        }));
        let app = Router::new()
            .route("/v1/forecast", get(forecast))
            .with_state(st.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Fake { url, st }
    }

    fn set(&self, mode: Mode) {
        self.st.lock().unwrap().mode = mode;
    }

    fn calls(&self) -> usize {
        self.st.lock().unwrap().queries.len()
    }

    fn query(&self, i: usize) -> String {
        self.st.lock().unwrap().queries[i].clone()
    }
}

async fn forecast(State(st): State<Arc<Mutex<FakeState>>>, uri: Uri) -> Response {
    let mode = {
        let mut s = st.lock().unwrap();
        s.queries.push(uri.query().unwrap_or_default().to_string());
        s.mode
    };
    match mode {
        Mode::Ok => ([(header::CONTENT_TYPE, "application/json")], ANSWER).into_response(),
        Mode::Status(c) => StatusCode::from_u16(c).unwrap().into_response(),
        Mode::Garbage => "<html>not json</html>".into_response(),
        Mode::Slow(ms) => {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            ([(header::CONTENT_TYPE, "application/json")], ANSWER).into_response()
        }
    }
}

// ---- the server under test ------------------------------------------------------------------

struct Rig {
    _dir: tempfile::TempDir,
    app: Router,
    /// A device token of a user.
    token: String,
    /// A device token that no user owns.
    orphan: String,
    clock: Arc<AtomicI64>,
    fake: Fake,
    weather: Arc<Weather>,
}

async fn rig_with(cfg: impl FnOnce(&mut WeatherConfig)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::open(dir.path()).await.unwrap();
    let uid = make_user(&pool, USER, Role::User, true).await;
    let (_, token) = tokens::create(&pool, "watch phone", Some(uid))
        .await
        .unwrap();
    let (_, orphan) = tokens::create(&pool, "unassigned", None).await.unwrap();
    let fake = Fake::start().await;
    let mut wc = WeatherConfig {
        base_url: fake.url.clone(),
        timeout: Duration::from_secs(5),
        ..WeatherConfig::default()
    };
    cfg(&mut wc);
    let clock = Arc::new(AtomicI64::new(START));
    let c = clock.clone();
    let state = api::AppState::new(pool, dir.path().to_path_buf(), api::MAX_BODY_BYTES)
        .with_weather(wc)
        .with_clock(move || c.load(Ordering::SeqCst));
    let weather = state.weather.clone();
    Rig {
        _dir: dir,
        app: api::router(state),
        token,
        orphan,
        clock,
        fake,
        weather,
    }
}

async fn rig() -> Rig {
    rig_with(|_| {}).await
}

impl Rig {
    async fn post(&self, token: Option<&str>, body: Value) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(Method::POST)
            .uri("/api/device/weather")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(t) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let resp = self
            .app
            .clone()
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn ask(&self, lat: f64, lon: f64, hours: u8) -> (StatusCode, Value) {
        self.post(
            Some(&self.token),
            json!({"lat": lat, "lon": lon, "hours": hours, "format": 1}),
        )
        .await
    }
}

fn cell(lat: i16, lon: i16) -> WeatherCell {
    WeatherCell {
        lat_tenths: lat,
        lon_tenths: lon,
    }
}

fn b64(v: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(v.as_str().expect("a base64 string"))
        .expect("valid base64")
}

// ---- the endpoint: who may ask, what is checked ------------------------------------------------------

#[tokio::test]
async fn a_request_needs_a_device_token_that_belongs_to_a_user() {
    let r = rig().await;
    let (s, _) = r.post(None, json!({"lat": 55.7, "lon": 12.6})).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = r
        .post(Some("not-a-token"), json!({"lat": 55.7, "lon": 12.6}))
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = r
        .post(Some(&r.orphan), json!({"lat": 55.7, "lon": 12.6}))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(r.fake.calls(), 0);
}

#[tokio::test]
async fn positions_that_are_not_on_the_globe_are_refused_before_anything_is_fetched() {
    let r = rig().await;
    for body in [
        json!({"lat": 91.0, "lon": 12.6}),
        json!({"lat": 55.7, "lon": 181.0}),
        json!({"lat": -91.5, "lon": 0.0}),
        json!({"lat": 55.7}),
        json!({"lon": 12.6}),
        json!({"lat": "north", "lon": 12.6}),
        json!({}),
    ] {
        let (s, v) = r.post(Some(&r.token), body.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{body}");
        assert!(v["error"].is_string());
    }
    assert_eq!(r.fake.calls(), 0);
}

#[tokio::test]
async fn the_kill_switch_answers_503_and_never_contacts_open_meteo() {
    let r = rig_with(|c| c.enabled = false).await;
    let (s, v) = r.ask(55.7, 12.6, 12).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert!(v["error"].as_str().unwrap().contains("switched off"));
    assert_eq!(r.fake.calls(), 0);
}

#[tokio::test]
async fn a_server_without_the_fit_profile_says_so_and_fetches_nothing() {
    if fit_weather::available() {
        return; // the profile build answers for real, see the tests below
    }
    let r = rig().await;
    let (s, v) = r.ask(55.7, 12.6, 12).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert!(v["error"].as_str().unwrap().contains("FIT profile"));
    assert_eq!(r.fake.calls(), 0);
}

// ---- the endpoint with a profile: the FIT bytes ------------------------------------------------------

#[tokio::test]
async fn the_answer_is_fit_definitions_and_records_for_the_watch() {
    if !fit_weather::available() {
        return;
    }
    let r = rig().await;
    let (s, v) = r.ask(55.6761, 12.5683, 12).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["fetched_at"], START);
    let definitions = b64(&v["definitions"]);
    let records: Vec<Vec<u8>> = v["records"].as_array().unwrap().iter().map(b64).collect();
    // one current record, twelve hours, five days
    assert_eq!(records.len(), 1 + 12 + 5);
    assert_eq!(definitions[0] & 0xC0, 0x40, "a definition record first");

    // read the answer back through the decoder
    let file = records_to_file(&definitions, &records).unwrap();
    let d = decode(&file);
    assert_eq!(d.error, None);
    let msgs = interpret(&d.files[0]);
    assert_eq!(msgs.len(), 18);
    let cur = &msgs[0];
    assert_eq!(
        cur.get("weather_report").unwrap().enum_label,
        Some("current")
    );
    assert_eq!(cur.number("temperature"), Some(2.0)); // 2.4
    assert_eq!(cur.get("condition").unwrap().enum_label, Some("clear")); // WMO 0
    assert_eq!(cur.number("wind_speed"), Some(3.8));
    assert_eq!(cur.number("wind_direction"), Some(280.0));
    assert_eq!(cur.number("relative_humidity"), Some(87.0));
    assert_eq!(cur.number("high_temperature"), Some(6.0)); // 6.2
    assert_eq!(cur.number("low_temperature"), Some(2.0)); // 2.4
                                                          // the position the watch is told is the cell, not what the phone sent
    let lat = cur.number("observed_location_lat").unwrap() * 180.0 / 2_147_483_648.0;
    let lon = cur.number("observed_location_long").unwrap() * 180.0 / 2_147_483_648.0;
    assert!((lat - 55.7).abs() < 1e-6 && (lon - 12.6).abs() < 1e-6);
    // the hours that follow now, then the days
    assert_eq!(
        msgs[1].get("weather_report").unwrap().enum_label,
        Some("hourly_forecast")
    );
    assert_eq!(
        msgs[13].get("weather_report").unwrap().enum_label,
        Some("daily_forecast")
    );
    assert_eq!(msgs[13].number("high_temperature"), Some(6.0));
    assert_eq!(
        msgs[14].get("condition").unwrap().enum_label,
        Some("cloudy")
    ); // WMO 3
    assert_eq!(
        msgs[15].get("condition").unwrap().enum_label,
        Some("light_rain")
    ); // WMO 51
}

#[tokio::test]
async fn the_requested_hours_set_the_number_of_hourly_records() {
    if !fit_weather::available() {
        return;
    }
    let r = rig().await;
    for (hours, want) in [(3u8, 3usize), (0, 12), (200, 12)] {
        let (s, v) = r.ask(55.7, 12.6, hours).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(
            v["records"].as_array().unwrap().len(),
            1 + want + 5,
            "{hours}"
        );
    }
    assert_eq!(r.fake.calls(), 1, "the hours never cause another fetch");
}

#[tokio::test]
async fn the_server_rounds_the_position_again_before_it_asks_open_meteo() {
    if !fit_weather::available() {
        return;
    }
    let r = rig().await;
    let (s, _) = r.ask(55.6761, 12.5683, 12).await;
    assert_eq!(s, StatusCode::OK);
    let q = r.fake.query(0);
    assert!(q.contains("latitude=55.7&longitude=12.6"), "{q}");
    assert!(!q.contains("55.6761") && !q.contains("12.5683"), "{q}");
    // and asks for the units the encoder expects
    for want in [
        "wind_speed_unit=ms",
        "temperature_unit=celsius",
        "timeformat=unixtime",
    ] {
        assert!(q.contains(want), "{want}: {q}");
    }
}

#[tokio::test]
async fn a_failing_open_meteo_is_a_502_without_a_position_in_the_text() {
    if !fit_weather::available() {
        return;
    }
    for mode in [Mode::Status(500), Mode::Status(429), Mode::Garbage] {
        let r = rig().await;
        r.fake.set(mode);
        let (s, v) = r.ask(55.6761, 12.5683, 12).await;
        assert_eq!(s, StatusCode::BAD_GATEWAY);
        let text = v.to_string();
        assert!(!text.contains("55.") && !text.contains("12."), "{text}");
    }
}

#[tokio::test]
async fn a_slow_open_meteo_is_cut_off_at_the_timeout() {
    if !fit_weather::available() {
        return;
    }
    let r = rig_with(|c| c.timeout = Duration::from_millis(200)).await;
    r.fake.set(Mode::Slow(3000));
    let started = std::time::Instant::now();
    let (s, _) = r.ask(55.7, 12.6, 12).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    assert!(started.elapsed() < Duration::from_secs(2));
}

// ---- the service itself: cache, errors (no profile needed) --------------------------------------------

#[tokio::test]
async fn a_cell_is_fetched_once_per_half_hour() {
    let r = rig().await;
    let w = &r.weather;
    let (f, cached, at) = w.forecast(cell(557, 126), 12, START).await.unwrap();
    assert!(!cached);
    assert_eq!(at, START);
    assert_eq!(f.hourly.len(), 12);
    assert_eq!(r.fake.calls(), 1);

    // again, a minute later, with another hour count: from the cache
    let (f, cached, at) = w.forecast(cell(557, 126), 4, START + 60).await.unwrap();
    assert!(cached);
    assert_eq!(at, START, "the age of the data, not of the request");
    assert_eq!(f.hourly.len(), 4);
    assert_eq!(r.fake.calls(), 1);

    // just inside the TTL
    let (_, cached, _) = w
        .forecast(cell(557, 126), 12, START + CACHE_TTL_S - 1)
        .await
        .unwrap();
    assert!(cached);
    assert_eq!(r.fake.calls(), 1);

    // at the TTL it is fetched again
    let (_, cached, at) = w
        .forecast(cell(557, 126), 12, START + CACHE_TTL_S)
        .await
        .unwrap();
    assert!(!cached);
    assert_eq!(at, START + CACHE_TTL_S);
    assert_eq!(r.fake.calls(), 2);
}

#[tokio::test]
async fn every_cell_has_its_own_cache_entry() {
    let r = rig().await;
    let w = &r.weather;
    w.forecast(cell(557, 126), 12, START).await.unwrap();
    w.forecast(cell(557, 127), 12, START).await.unwrap();
    w.forecast(cell(558, 126), 12, START).await.unwrap();
    assert_eq!(r.fake.calls(), 3);
    w.forecast(cell(557, 127), 12, START + 5).await.unwrap();
    assert_eq!(r.fake.calls(), 3);
    // each upstream request names its own cell
    assert!(r.fake.query(1).contains("latitude=55.7&longitude=12.7"));
    assert!(r.fake.query(2).contains("latitude=55.8&longitude=12.6"));
}

#[tokio::test]
async fn a_clock_that_went_backwards_does_not_serve_stale_data() {
    let r = rig().await;
    r.weather.forecast(cell(557, 126), 12, START).await.unwrap();
    let (_, cached, _) = r
        .weather
        .forecast(cell(557, 126), 12, START - 10)
        .await
        .unwrap();
    assert!(!cached);
}

#[tokio::test]
async fn failures_are_not_cached_and_name_no_position() {
    let r = rig().await;
    let w = &r.weather;
    r.fake.set(Mode::Status(503));
    assert_eq!(
        w.forecast(cell(557, 126), 12, START).await.unwrap_err(),
        WeatherError::Status(503)
    );
    r.fake.set(Mode::Garbage);
    assert_eq!(
        w.forecast(cell(557, 126), 12, START).await.unwrap_err(),
        WeatherError::BadAnswer
    );
    r.fake.set(Mode::Ok);
    assert!(w.forecast(cell(557, 126), 12, START).await.is_ok());
    assert_eq!(r.fake.calls(), 3, "a failure leaves nothing in the cache");
    for e in [
        WeatherError::Timeout,
        WeatherError::Unreachable,
        WeatherError::Status(500),
        WeatherError::BadAnswer,
    ] {
        assert!(!e.word().chars().any(|c| c.is_ascii_digit()));
    }
}

#[tokio::test]
async fn an_unreachable_open_meteo_is_reported_as_such() {
    let r = rig_with(|c| c.base_url = "http://127.0.0.1:1".into()).await;
    let e = r
        .weather
        .forecast(cell(557, 126), 12, START)
        .await
        .unwrap_err();
    assert_eq!(e, WeatherError::Unreachable);
}

#[tokio::test]
async fn a_slow_open_meteo_times_out_in_the_service() {
    let r = rig_with(|c| c.timeout = Duration::from_millis(150)).await;
    r.fake.set(Mode::Slow(2000));
    let e = r
        .weather
        .forecast(cell(557, 126), 12, START)
        .await
        .unwrap_err();
    assert_eq!(e, WeatherError::Timeout);
}

#[tokio::test]
async fn the_cache_keeps_to_its_size() {
    let r = rig().await;
    // more cells than the cache holds: every one is fetched, none fails
    for i in 0..300i16 {
        r.weather
            .forecast(cell(100 + i, 5), 12, START + i64::from(i))
            .await
            .unwrap();
    }
    assert_eq!(r.fake.calls(), 300);
    // the newest cell is still there
    let (_, cached, _) = r
        .weather
        .forecast(cell(399, 5), 12, START + 300)
        .await
        .unwrap();
    assert!(cached);
    // the oldest was dropped to make room
    let (_, cached, _) = r
        .weather
        .forecast(cell(100, 5), 12, START + 300)
        .await
        .unwrap();
    assert!(!cached);
}

#[tokio::test]
async fn the_clock_the_tests_pin_is_the_clock_the_endpoint_uses() {
    if !fit_weather::available() {
        return;
    }
    let r = rig().await;
    r.ask(55.7, 12.6, 12).await;
    r.ask(55.7, 12.6, 12).await;
    assert_eq!(r.fake.calls(), 1);
    r.clock.store(START + CACHE_TTL_S + 1, Ordering::SeqCst);
    let (s, v) = r.ask(55.7, 12.6, 12).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["fetched_at"], START + CACHE_TTL_S + 1);
    assert_eq!(r.fake.calls(), 2);
}

// ---- privacy: nothing about where the watch is ends up in the log ------------------------------------------

#[derive(Clone, Default)]
struct LogBuf(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogBuf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuf {
    type Writer = LogBuf;
    fn make_writer(&'a self) -> LogBuf {
        self.clone()
    }
}

#[tokio::test]
async fn the_log_never_holds_a_position_or_a_token() {
    let buf = LogBuf::default();
    let sub = tracing_subscriber::fmt()
        .with_writer(buf.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let _guard = tracing::subscriber::set_default(sub);

    let r = rig().await;
    r.ask(55.6761, 12.5683, 12).await;
    r.ask(55.6761, 12.5683, 12).await;
    r.post(Some(&r.token), json!({"lat": 95.5123, "lon": 12.5683}))
        .await;
    let failing = rig().await;
    failing.fake.set(Mode::Status(500));
    failing.ask(55.6761, 12.5683, 12).await;

    let log = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    for secret in [
        "55.6761",
        "12.5683",
        "55.7",
        "12.6",
        "95.5123",
        r.token.as_str(),
    ] {
        assert!(!log.contains(secret), "the log shows {secret}:\n{log}");
    }
}
