//! Weather for the watch (stage 2f): `POST /api/device/weather`.
//!
//! The phone sends the watch's position (already rounded to 0.1 degree, about
//! 10 km, by `nimrag-core`; rounded again here as defence), the server gets the
//! forecast from Open-Meteo (https://open-meteo.com, free, no key, CC BY 4.0)
//! and answers with the FIT `weather_conditions` records the watch's 5011 and
//! 5012 messages carry. The phone only talks to this server; Open-Meteo only
//! ever sees a rounded cell and this server's address.
//!
//! ```text
//! POST /api/device/weather    (device token, Authorization: Bearer; no CSRF header)
//!   {"lat": 55.7, "lon": 12.6, "hours": 12, "format": 1}
//!   -> 200 {"definitions": "<base64>", "records": ["<base64>", ...], "fetched_at": <unix s>}
//!      400 bad position, 401 no token, 403 token without a user,
//!      502 Open-Meteo did not answer,
//!      503 switched off (`NIMRAG_WEATHER=off`) or no FIT profile in this build
//! ```
//!
//! Privacy: coordinates are never stored and never logged (not even rounded);
//! log lines say what happened, not where. Upstream errors are mapped to fixed
//! words because the error text of an HTTP client carries the URL, and the URL
//! carries the cell. Results are cached in memory for 30 minutes per rounded
//! cell, so one cell is fetched at most twice an hour however often a watch asks.
//!
//! The FIT bytes are made by `nimrag_core::fit::weather` from profile lookups by
//! name: a server built without the FIT profile answers 503, as the workout
//! endpoint does, and the phone then tells the watch there is no weather.
//!
//! **WMO weather code to Garmin condition.** Open-Meteo reports WMO code table
//! 4677 values; the watch wants FIT `weather_status` values. The mapping is our
//! own (Gadgetbridge maps OpenWeatherMap codes, not WMO ones; its choices
//! guided the rain and snow intensities) and is listed in [`condition_from_wmo`]:
//!
//! | WMO code | meaning | Garmin condition |
//! | --- | --- | --- |
//! | 0, 1 | clear, mainly clear | clear |
//! | 2 | partly cloudy | partly cloudy |
//! | 3 | overcast | cloudy |
//! | 45, 48 | fog, rime fog | fog |
//! | 51, 53 | light and moderate drizzle | light rain |
//! | 55 | dense drizzle | rain |
//! | 56, 57 | freezing drizzle | wintry mix |
//! | 61 | slight rain | light rain |
//! | 63 | moderate rain | rain |
//! | 65 | heavy rain | heavy rain |
//! | 66, 67 | freezing rain | wintry mix |
//! | 71, 77 | slight snow, snow grains | light snow |
//! | 73 | moderate snow | snow |
//! | 75 | heavy snow | heavy snow |
//! | 80 | slight rain showers | scattered showers |
//! | 81 | moderate rain showers | rain |
//! | 82 | violent rain showers | heavy rain |
//! | 85 | slight snow showers | light snow |
//! | 86 | heavy snow showers | heavy snow |
//! | 95, 96, 99 | thunderstorm, with hail | thunderstorms |
//! | anything else | unknown | no value (the watch shows none) |
//!
//! Ported ideas from Gadgetbridge commit 7257742b (AGPL-3.0-or-later):
//! `service/devices/garmin/http/interceptors/WeatherInterceptor.java` (which
//! values a Garmin weather answer holds) and
//! `service/devices/garmin/fit/fieldDefinitions/FieldDefinitionWeatherStatus.java`
//! (the intensity choices of the condition mapping; the code itself is not
//! ported). Copyright (C) 2024-2026 Daniele Gobbetti, José Rebelo, Thomas
//! Kuehne and the other Gadgetbridge contributors. See docs/PORTED.md.

use crate::api::{ApiError, AppState, Device};
use crate::web_api::ApiJson;
use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use base64::Engine;
use nimrag_core::client::WeatherCell;
use nimrag_core::fit::encode::EncodeError;
use nimrag_core::fit::weather::{self as fit, Condition, Forecast, Point};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Where Open-Meteo lives; `NIMRAG_WEATHER_URL` / `--weather-url` replace it (tests
/// point it at a local fake; no test reaches the real service).
pub const DEFAULT_BASE_URL: &str = "https://api.open-meteo.com";
/// Longest we wait for Open-Meteo.
pub const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a fetched forecast serves later requests for the same cell.
pub const CACHE_TTL_S: i64 = 30 * 60;
/// Cells kept in memory. A single owner's watch asks for a handful.
const MAX_CELLS: usize = 256;
/// The most hourly records the watch gets (Gadgetbridge sends 12).
const MAX_HOURS: u8 = 12;
/// Days asked from Open-Meteo: today and four more, as the watch shows.
const FORECAST_DAYS: u8 = 5;

/// What the weather endpoint says when it is switched off.
pub const OFF: &str = "weather is switched off on this server (NIMRAG_WEATHER=off)";
/// What it says when the FIT profile is missing.
pub const UNAVAILABLE: &str = "this server was built without the FIT profile, so it cannot write \
    the weather messages for the watch";

#[derive(Debug, Clone)]
pub struct WeatherConfig {
    /// The kill switch: false answers 503 and never contacts Open-Meteo.
    pub enabled: bool,
    pub base_url: String,
    pub timeout: Duration,
    pub ttl_s: i64,
}

impl Default for WeatherConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            base_url: DEFAULT_BASE_URL.into(),
            timeout: UPSTREAM_TIMEOUT,
            ttl_s: CACHE_TTL_S,
        }
    }
}

/// Why no forecast could be made. Carries no position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherError {
    /// Open-Meteo did not answer in time.
    Timeout,
    /// Open-Meteo could not be reached.
    Unreachable,
    /// Open-Meteo answered with this HTTP status.
    Status(u16),
    /// Open-Meteo's answer was not what we expect.
    BadAnswer,
}

impl WeatherError {
    /// A fixed word for the log.
    pub fn word(self) -> &'static str {
        match self {
            WeatherError::Timeout => "timeout",
            WeatherError::Unreachable => "unreachable",
            WeatherError::Status(_) => "status",
            WeatherError::BadAnswer => "bad answer",
        }
    }
}

#[derive(Debug)]
struct Cached {
    fetched_at: i64,
    raw: Arc<Raw>,
}

/// The weather service of one server: configuration, HTTP client and cache.
pub struct Weather {
    cfg: WeatherConfig,
    client: OnceLock<reqwest::Client>,
    cache: Mutex<HashMap<(i16, i16), Cached>>,
}

impl Default for Weather {
    fn default() -> Self {
        Self::new(WeatherConfig::default())
    }
}

impl Weather {
    pub fn new(cfg: WeatherConfig) -> Self {
        Self {
            cfg,
            client: OnceLock::new(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn enabled(&self) -> bool {
        self.cfg.enabled
    }

    fn client(&self) -> &reqwest::Client {
        self.client.get_or_init(|| {
            reqwest::Client::builder()
                .timeout(self.cfg.timeout)
                .user_agent(concat!("nimrag-server/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        })
    }

    /// The forecast for a cell: from the cache when it is younger than the TTL,
    /// else fetched. The flag says "served from the cache". `now` is unix seconds.
    pub async fn forecast(
        &self,
        cell: WeatherCell,
        hours: u8,
        now: i64,
    ) -> Result<(Forecast, bool, i64), WeatherError> {
        let key = (cell.lat_tenths, cell.lon_tenths);
        let hit = {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            cache
                .get(&key)
                .filter(|c| (0..self.cfg.ttl_s).contains(&(now - c.fetched_at)))
                .map(|c| (c.raw.clone(), c.fetched_at))
        };
        let (raw, fetched_at, cached) = match hit {
            Some((raw, at)) => (raw, at, true),
            None => {
                let raw = Arc::new(self.fetch(cell).await?);
                self.store(key, raw.clone(), now);
                (raw, now, false)
            }
        };
        let forecast = build_forecast(&raw, cell, hours).ok_or(WeatherError::BadAnswer)?;
        Ok((forecast, cached, fetched_at))
    }

    fn store(&self, key: (i16, i16), raw: Arc<Raw>, now: i64) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= MAX_CELLS && !cache.contains_key(&key) {
            let ttl = self.cfg.ttl_s;
            cache.retain(|_, c| (0..ttl).contains(&(now - c.fetched_at)));
            if cache.len() >= MAX_CELLS {
                if let Some(oldest) = cache
                    .iter()
                    .min_by_key(|(_, c)| c.fetched_at)
                    .map(|(k, _)| *k)
                {
                    cache.remove(&oldest);
                }
            }
        }
        cache.insert(
            key,
            Cached {
                fetched_at: now,
                raw,
            },
        );
    }

    async fn fetch(&self, cell: WeatherCell) -> Result<Raw, WeatherError> {
        // Every value is plain ASCII (digits, letters, commas, underscores): no encoding needed.
        let url = format!(
            "{}/v1/forecast?latitude={:.1}&longitude={:.1}&current={CURRENT_VARS}&hourly={HOURLY_VARS}\
             &daily={DAILY_VARS}&wind_speed_unit=ms&temperature_unit=celsius&timeformat=unixtime\
             &timezone=auto&forecast_days={FORECAST_DAYS}",
            self.cfg.base_url.trim_end_matches('/'),
            cell.lat_deg(),
            cell.lon_deg(),
        );
        let resp = self.client().get(url).send().await.map_err(classify)?;
        let status = resp.status();
        if !status.is_success() {
            return Err(WeatherError::Status(status.as_u16()));
        }
        let bytes = resp.bytes().await.map_err(classify)?;
        serde_json::from_slice::<Raw>(&bytes).map_err(|_| WeatherError::BadAnswer)
    }
}

/// An HTTP client error as a fixed word: never its text, which holds the URL.
fn classify(e: reqwest::Error) -> WeatherError {
    if e.is_timeout() {
        WeatherError::Timeout
    } else if e.is_decode() || e.is_body() {
        WeatherError::BadAnswer
    } else {
        WeatherError::Unreachable
    }
}

const CURRENT_VARS: &str = "temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,\
    pressure_msl,wind_speed_10m,wind_direction_10m,dew_point_2m";
const HOURLY_VARS: &str = "temperature_2m,relative_humidity_2m,dew_point_2m,apparent_temperature,\
    precipitation_probability,weather_code,pressure_msl,wind_speed_10m,wind_direction_10m,uv_index";
const DAILY_VARS: &str = "weather_code,temperature_2m_max,temperature_2m_min,\
    precipitation_probability_max,wind_speed_10m_max,wind_direction_10m_dominant,uv_index_max";

// ---- Open-Meteo's answer ----------------------------------------------------------------------

/// The parts of Open-Meteo's `/v1/forecast` answer we read (units are asked for
/// in the request: Celsius, m/s, hPa, Unix time). Every value may be null.
#[derive(Debug, Default, Deserialize)]
struct Raw {
    #[serde(default)]
    utc_offset_seconds: i32,
    current: Option<RawCurrent>,
    #[serde(default)]
    hourly: RawHourly,
    #[serde(default)]
    daily: RawDaily,
}

#[derive(Debug, Default, Deserialize)]
struct RawCurrent {
    time: i64,
    temperature_2m: Option<f64>,
    relative_humidity_2m: Option<f64>,
    apparent_temperature: Option<f64>,
    weather_code: Option<f64>,
    pressure_msl: Option<f64>,
    wind_speed_10m: Option<f64>,
    wind_direction_10m: Option<f64>,
    dew_point_2m: Option<f64>,
}

type Series = Vec<Option<f64>>;

#[derive(Debug, Default, Deserialize)]
struct RawHourly {
    #[serde(default)]
    time: Vec<i64>,
    #[serde(default)]
    temperature_2m: Series,
    #[serde(default)]
    relative_humidity_2m: Series,
    #[serde(default)]
    dew_point_2m: Series,
    #[serde(default)]
    apparent_temperature: Series,
    #[serde(default)]
    precipitation_probability: Series,
    #[serde(default)]
    weather_code: Series,
    #[serde(default)]
    pressure_msl: Series,
    #[serde(default)]
    wind_speed_10m: Series,
    #[serde(default)]
    wind_direction_10m: Series,
    #[serde(default)]
    uv_index: Series,
}

#[derive(Debug, Default, Deserialize)]
struct RawDaily {
    #[serde(default)]
    time: Vec<i64>,
    #[serde(default)]
    weather_code: Series,
    #[serde(default)]
    temperature_2m_max: Series,
    #[serde(default)]
    temperature_2m_min: Series,
    #[serde(default)]
    precipitation_probability_max: Series,
    #[serde(default)]
    wind_speed_10m_max: Series,
    #[serde(default)]
    wind_direction_10m_dominant: Series,
    #[serde(default)]
    uv_index_max: Series,
}

fn at(s: &Series, i: usize) -> Option<f64> {
    s.get(i).copied().flatten().filter(|v| v.is_finite())
}

/// WMO weather interpretation code (table 4677, as Open-Meteo reports it) to a
/// Garmin condition; the table is in the module documentation. `None` for a
/// code we do not know.
pub fn condition_from_wmo(code: i64) -> Option<Condition> {
    Some(match code {
        0 | 1 => Condition::Clear,
        2 => Condition::PartlyCloudy,
        3 => Condition::Cloudy,
        45 | 48 => Condition::Fog,
        51 | 53 => Condition::LightRain,
        55 => Condition::Rain,
        56 | 57 => Condition::WintryMix,
        61 => Condition::LightRain,
        63 => Condition::Rain,
        65 => Condition::HeavyRain,
        66 | 67 => Condition::WintryMix,
        71 | 77 => Condition::LightSnow,
        73 => Condition::Snow,
        75 => Condition::HeavySnow,
        80 => Condition::ScatteredShowers,
        81 => Condition::Rain,
        82 => Condition::HeavyRain,
        85 => Condition::LightSnow,
        86 => Condition::HeavySnow,
        95 | 96 | 99 => Condition::Thunderstorms,
        _ => return None,
    })
}

fn condition_at(s: &Series, i: usize) -> Option<Condition> {
    at(s, i).and_then(|c| condition_from_wmo(c.round() as i64))
}

/// Day of the week at the place for a local-midnight Unix time, 0 = Sunday.
fn weekday(local_midnight_unix_s: i64, utc_offset_s: i32) -> u8 {
    let days = (local_midnight_unix_s + i64::from(utc_offset_s)).div_euclid(86_400);
    // 1970-01-01 was a Thursday (4)
    (days + 4).rem_euclid(7) as u8
}

/// Make the forecast the encoder takes from Open-Meteo's answer: now (with
/// today's high and low from the first day), the next `hours` full hours and the
/// days. `None` when the answer has no current block.
fn build_forecast(raw: &Raw, cell: WeatherCell, hours: u8) -> Option<Forecast> {
    let cur = raw.current.as_ref()?;
    let h = &raw.hourly;
    let d = &raw.daily;
    let hours = usize::from(if hours == 0 {
        MAX_HOURS
    } else {
        hours.min(MAX_HOURS)
    });

    // the hour that contains "now" gives the probability and the UV index
    let now_hour = h.time.iter().rposition(|t| *t <= cur.time);
    let current = Point {
        time_unix_s: cur.time,
        temperature_c: cur.temperature_2m,
        feels_like_c: cur.apparent_temperature,
        high_c: at(&d.temperature_2m_max, 0),
        low_c: at(&d.temperature_2m_min, 0),
        condition: cur
            .weather_code
            .and_then(|c| condition_from_wmo(c.round() as i64)),
        wind_speed_ms: cur.wind_speed_10m,
        wind_direction_deg: cur.wind_direction_10m,
        precipitation_probability: now_hour.and_then(|i| at(&h.precipitation_probability, i)),
        humidity_percent: cur.relative_humidity_2m,
        dew_point_c: cur.dew_point_2m,
        uv_index: now_hour.and_then(|i| at(&h.uv_index, i)),
        pressure_hpa: cur.pressure_msl,
        weekday: None,
    };

    let hourly = h
        .time
        .iter()
        .enumerate()
        .filter(|(_, t)| **t > cur.time)
        .take(hours)
        .map(|(i, t)| Point {
            time_unix_s: *t,
            temperature_c: at(&h.temperature_2m, i),
            feels_like_c: at(&h.apparent_temperature, i),
            high_c: None,
            low_c: None,
            condition: condition_at(&h.weather_code, i),
            wind_speed_ms: at(&h.wind_speed_10m, i),
            wind_direction_deg: at(&h.wind_direction_10m, i),
            precipitation_probability: at(&h.precipitation_probability, i),
            humidity_percent: at(&h.relative_humidity_2m, i),
            dew_point_c: at(&h.dew_point_2m, i),
            uv_index: at(&h.uv_index, i),
            pressure_hpa: at(&h.pressure_msl, i),
            weekday: None,
        })
        .collect();

    let daily = d
        .time
        .iter()
        .enumerate()
        .take(usize::from(FORECAST_DAYS))
        .map(|(i, t)| Point {
            time_unix_s: *t,
            temperature_c: None,
            feels_like_c: None,
            high_c: at(&d.temperature_2m_max, i),
            low_c: at(&d.temperature_2m_min, i),
            condition: condition_at(&d.weather_code, i),
            wind_speed_ms: at(&d.wind_speed_10m_max, i),
            wind_direction_deg: at(&d.wind_direction_10m_dominant, i),
            precipitation_probability: at(&d.precipitation_probability_max, i),
            humidity_percent: None,
            dew_point_c: None,
            uv_index: at(&d.uv_index_max, i),
            pressure_hpa: None,
            weekday: Some(weekday(*t, raw.utc_offset_seconds)),
        })
        .collect();

    Some(Forecast {
        lat_deg: cell.lat_deg(),
        lon_deg: cell.lon_deg(),
        location: None,
        current,
        hourly,
        daily,
    })
}

// ---- the endpoint ------------------------------------------------------------------------------

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/device/weather", post(device_weather))
}

#[derive(Deserialize)]
struct Request {
    lat: f64,
    lon: f64,
    /// Hours of forecast the watch asked for; 0 or missing means 12; at most 12 are sent.
    #[serde(default)]
    hours: u8,
    /// The watch's format byte; reserved, accepted and ignored.
    #[serde(default)]
    #[allow(dead_code)]
    format: u8,
}

#[derive(Serialize)]
struct Response {
    /// Base64 of the FIT definition records (the payload of one 5011).
    definitions: String,
    /// Base64 of each FIT data record (to be cut into 5012 messages).
    records: Vec<String>,
    /// When the forecast was fetched from Open-Meteo, Unix seconds.
    fetched_at: i64,
}

/// Round a position to the 0.1 degree grid; `None` when it is not on the globe.
/// The server's own defence: the phone already rounds.
pub fn round_to_cell(lat: f64, lon: f64) -> Option<WeatherCell> {
    if !lat.is_finite() || !lon.is_finite() || lat.abs() > 90.0 || lon.abs() > 180.0 {
        return None;
    }
    let (la, lo) = ((lat * 10.0).round() as i32, (lon * 10.0).round() as i32);
    Some(WeatherCell {
        lat_tenths: i16::try_from(la).ok()?,
        lon_tenths: i16::try_from(lo).ok()?,
    })
}

async fn device_weather(
    device: Device,
    State(state): State<AppState>,
    ApiJson(req): ApiJson<Request>,
) -> Result<Json<Response>, ApiError> {
    if device.user_id.is_none() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "this device token is not assigned to a user",
        ));
    }
    if !state.weather.enabled() {
        return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, OFF));
    }
    let cell = round_to_cell(req.lat, req.lon)
        .ok_or_else(|| ApiError::bad_request("lat must be -90..90 and lon -180..180"))?;
    if !fit::available() {
        return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, UNAVAILABLE));
    }
    let (forecast, cached, fetched_at) = state
        .weather
        .forecast(cell, req.hours, state.now())
        .await
        .map_err(|e| {
            tracing::warn!(reason = e.word(), "weather: no forecast");
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "the weather service did not give a forecast",
            )
        })?;
    let fit = fit::encode(&forecast).map_err(|e| match e {
        EncodeError::Unavailable => ApiError::new(StatusCode::SERVICE_UNAVAILABLE, UNAVAILABLE),
        other => anyhow::Error::new(other).into(),
    })?;
    tracing::info!(cached, records = fit.records.len(), "weather answered");
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    Ok(Json(Response {
        definitions: b64(&fit.definitions),
        records: fit.records.iter().map(|r| b64(r)).collect(),
        fetched_at,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_wmo_code_open_meteo_documents_has_a_condition() {
        let documented = [
            0, 1, 2, 3, 45, 48, 51, 53, 55, 56, 57, 61, 63, 65, 66, 67, 71, 73, 75, 77, 80, 81, 82,
            85, 86, 95, 96, 99,
        ];
        for code in documented {
            assert!(condition_from_wmo(code).is_some(), "code {code}");
        }
    }

    #[test]
    fn the_documented_table_is_the_mapping() {
        use Condition::*;
        let table = [
            (0, Clear),
            (1, Clear),
            (2, PartlyCloudy),
            (3, Cloudy),
            (45, Fog),
            (48, Fog),
            (51, LightRain),
            (53, LightRain),
            (55, Rain),
            (56, WintryMix),
            (57, WintryMix),
            (61, LightRain),
            (63, Rain),
            (65, HeavyRain),
            (66, WintryMix),
            (67, WintryMix),
            (71, LightSnow),
            (73, Snow),
            (75, HeavySnow),
            (77, LightSnow),
            (80, ScatteredShowers),
            (81, Rain),
            (82, HeavyRain),
            (85, LightSnow),
            (86, HeavySnow),
            (95, Thunderstorms),
            (96, Thunderstorms),
            (99, Thunderstorms),
        ];
        for (code, want) in table {
            assert_eq!(condition_from_wmo(code), Some(want), "code {code}");
        }
    }

    #[test]
    fn unknown_codes_are_no_value_not_a_guess() {
        for code in [-1, 4, 10, 44, 50, 58, 70, 100, 1000] {
            assert_eq!(condition_from_wmo(code), None, "code {code}");
        }
    }

    #[test]
    fn intensity_is_kept_for_rain_and_snow() {
        use Condition::*;
        assert_eq!(condition_from_wmo(61), Some(LightRain));
        assert_eq!(condition_from_wmo(65), Some(HeavyRain));
        assert_eq!(condition_from_wmo(71), Some(LightSnow));
        assert_eq!(condition_from_wmo(75), Some(HeavySnow));
    }

    #[test]
    fn weekdays_follow_the_local_day() {
        // 2026-11-23 00:00 UTC is a Monday
        let monday_utc = 1_795_392_000;
        assert_eq!(weekday(monday_utc, 0), 1);
        // local midnight at UTC+1 is 23:00 UTC the day before: still Monday there
        assert_eq!(weekday(monday_utc - 3600, 3600), 1);
        assert_eq!(weekday(monday_utc + 6 * 86_400, 0), 0); // Sunday
        assert_eq!(weekday(0, 0), 4); // 1970-01-01, a Thursday
        assert_eq!(weekday(-86_400, 0), 3); // before the epoch
    }

    #[test]
    fn positions_round_to_the_grid_and_off_globe_ones_are_refused() {
        let c = round_to_cell(55.6761, 12.5683).unwrap();
        assert_eq!((c.lat_tenths, c.lon_tenths), (557, 126));
        let c = round_to_cell(-33.8688, -151.2093).unwrap();
        assert_eq!((c.lat_tenths, c.lon_tenths), (-339, -1512));
        // an already rounded value stays put
        let c = round_to_cell(55.7, 12.6).unwrap();
        assert_eq!((c.lat_tenths, c.lon_tenths), (557, 126));
        assert!(round_to_cell(90.0, 180.0).is_some());
        for (la, lo) in [
            (91.0, 0.0),
            (0.0, 181.0),
            (f64::NAN, 0.0),
            (0.0, f64::INFINITY),
        ] {
            assert!(round_to_cell(la, lo).is_none(), "{la} {lo}");
        }
    }

    /// A trimmed answer in Open-Meteo's real shape (checked once by hand against
    /// the live API for a city centre).
    const ANSWER: &str = r#"{
      "latitude": 55.69701, "longitude": 12.600891, "utc_offset_seconds": 3600,
      "timezone": "Europe/Copenhagen",
      "current": {"time": 1000, "interval": 900, "temperature_2m": 2.4, "relative_humidity_2m": 87,
        "apparent_temperature": -1.7, "weather_code": 61, "pressure_msl": 1007.5,
        "wind_speed_10m": 3.8, "wind_direction_10m": 280, "dew_point_2m": 0.4},
      "hourly": {"time": [0, 3600, 7200, 10800],
        "temperature_2m": [3.0, 2.0, null, 1.0],
        "relative_humidity_2m": [80, 81, 82, 83], "dew_point_2m": [1, 1, 1, 1],
        "apparent_temperature": [0, 0, 0, 0],
        "precipitation_probability": [10, 20, 30, 40],
        "weather_code": [3, 61, 63, 999], "pressure_msl": [1000, 1001, 1002, 1003],
        "wind_speed_10m": [4, 5, 6, 7], "wind_direction_10m": [10, 20, 30, 40],
        "uv_index": [0.0, 0.5, 1.0, 1.5]},
      "daily": {"time": [0, 86400], "weather_code": [61, 3],
        "temperature_2m_max": [6.2, 4.2], "temperature_2m_min": [2.4, null],
        "precipitation_probability_max": [55, 9], "wind_speed_10m_max": [5.8, 8.4],
        "wind_direction_10m_dominant": [275, 345], "uv_index_max": [0.95, 0.9]}
    }"#;

    fn cell() -> WeatherCell {
        WeatherCell {
            lat_tenths: 557,
            lon_tenths: 126,
        }
    }

    #[test]
    fn current_hourly_and_daily_are_taken_from_the_answer() {
        let raw: Raw = serde_json::from_str(ANSWER).unwrap();
        let f = build_forecast(&raw, cell(), 12).unwrap();
        assert!((f.lat_deg - 55.7).abs() < 1e-9 && (f.lon_deg - 12.6).abs() < 1e-9);

        let c = &f.current;
        assert_eq!(c.time_unix_s, 1000);
        assert_eq!(c.temperature_c, Some(2.4));
        assert_eq!(c.feels_like_c, Some(-1.7));
        assert_eq!(c.condition, Some(Condition::LightRain));
        assert_eq!(c.wind_speed_ms, Some(3.8));
        assert_eq!(
            c.high_c,
            Some(6.2),
            "today's high and low come from the first day"
        );
        assert_eq!(c.low_c, Some(2.4));
        // the hour that contains now (starting at 0) gives the probability
        assert_eq!(c.precipitation_probability, Some(10.0));

        // only hours after now: 3600, 7200, 10800
        let times: Vec<i64> = f.hourly.iter().map(|p| p.time_unix_s).collect();
        assert_eq!(times, vec![3600, 7200, 10800]);
        assert_eq!(f.hourly[0].condition, Some(Condition::LightRain));
        assert_eq!(f.hourly[1].temperature_c, None, "a null stays no value");
        assert_eq!(f.hourly[2].condition, None, "an unknown code is no value");

        assert_eq!(f.daily.len(), 2);
        assert_eq!(f.daily[0].high_c, Some(6.2));
        assert_eq!(f.daily[1].low_c, None);
        assert_eq!(
            f.daily[0].weekday,
            Some(4),
            "1970-01-01 local was a Thursday"
        );
        assert_eq!(f.daily[1].weekday, Some(5));
        assert_eq!(f.daily[0].precipitation_probability, Some(55.0));
    }

    #[test]
    fn the_hour_count_is_capped_and_zero_means_twelve() {
        let mut raw: Raw = serde_json::from_str(ANSWER).unwrap();
        raw.hourly.time = (1..=40).map(|i| 1000 + i * 3600).collect();
        assert_eq!(build_forecast(&raw, cell(), 0).unwrap().hourly.len(), 12);
        assert_eq!(build_forecast(&raw, cell(), 99).unwrap().hourly.len(), 12);
        assert_eq!(build_forecast(&raw, cell(), 5).unwrap().hourly.len(), 5);
        assert_eq!(build_forecast(&raw, cell(), 1).unwrap().hourly.len(), 1);
    }

    #[test]
    fn an_answer_without_current_conditions_is_no_forecast() {
        let raw: Raw = serde_json::from_str(r#"{"hourly": {"time": [1]}}"#).unwrap();
        assert!(build_forecast(&raw, cell(), 12).is_none());
    }

    #[test]
    fn missing_series_are_no_values_not_errors() {
        let raw: Raw =
            serde_json::from_str(r#"{"current": {"time": 5}, "hourly": {"time": [10, 20]}}"#)
                .unwrap();
        let f = build_forecast(&raw, cell(), 12).unwrap();
        assert_eq!(f.current.temperature_c, None);
        assert_eq!(f.hourly.len(), 2);
        assert_eq!(f.hourly[0].temperature_c, None);
        assert!(f.daily.is_empty());
    }
}
