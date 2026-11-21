//! The weather forecast as FIT `weather_conditions` records (stage 2f), for the
//! 5011 (definitions) and 5012 (data) messages of the watch's weather request.
//!
//! This is profile-dependent code: every message, field and enum value is looked
//! up **by name** in `core::fit::profile` through [`MessageBuilder`], so no
//! profile table lives here (docs/LICENSING.md). It runs on the server only; the
//! public Android app never links a profile and just moves the bytes. Without a
//! profile [`encode`] answers [`EncodeError::Unavailable`] and never wrong bytes.
//!
//! The shape of the answer is ported from Gadgetbridge commit 7257742b
//! (AGPL-3.0-or-later), `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/`
//! `GarminSupport.java` (`encodeWeather`) and `fit/messages/FitWeatherConditions.java`
//! (the unit rules of the builder), `fit/FitLocalMessageBuilder.java` (one local
//! message type per layout), Copyright (C) 2024-2026 Daniele Gobbetti, José Rebelo,
//! Thomas Kuehne and the other Gadgetbridge contributors. See docs/PORTED.md.
//! *inferred (Gadgetbridge)* until a watch shows the weather from it:
//!
//! - local type 0: one `current` record (observed time, position, temperature,
//!   today's high and low, condition, wind, precipitation probability, feels-like,
//!   humidity);
//! - local type 1: up to 12 `hourly_forecast` records;
//! - local type 2: `daily_forecast` records, today first, then up to four more
//!   days; the timestamp of every daily record is the time of the forecast (as
//!   Gadgetbridge does) and `day_of_week` says which day it is for.
//!
//! Units written (Gadgetbridge's builder rules): whole degrees Celsius,
//! wind speed in m/s (at most 65.53), direction in degrees below 360,
//! probability and humidity 0 to 100, UV index clamped to 10, pressure in
//! pascal. A value we do not have is written as FIT's invalid value, so every
//! record of a local type has the same layout. What we do differently is
//! marked [ours]: temperatures are rounded, not truncated Kelvin differences
//! (Gadgetbridge subtracts 273 from Kelvin on purpose, #4313; we start from
//! Celsius); the optional fields `dew_point`, `uv_index` and `atmospheric_pressure`
//! are written only when the local profile has them; there is no location name
//! and no air quality (Open-Meteo's forecast has neither).

use super::encode::{EncodeError, MessageBuilder, RecordSet};
use super::profile;
use crate::client::GARMIN_EPOCH_UNIX_S;

/// FIT weather condition (`weather_status`), by the profile's value name.
/// The server maps its own source's codes to these; the numbers come from the
/// profile at encoding time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Condition {
    Clear,
    PartlyCloudy,
    MostlyCloudy,
    Cloudy,
    Rain,
    LightRain,
    HeavyRain,
    ScatteredShowers,
    Snow,
    LightSnow,
    HeavySnow,
    WintryMix,
    LightRainSnow,
    HeavyRainSnow,
    Thunderstorms,
    ScatteredThunderstorms,
    Fog,
    Hazy,
    Hail,
    Windy,
    UnknownPrecipitation,
}

impl Condition {
    /// The value's name in the profile's `weather_status` type.
    pub fn profile_name(self) -> &'static str {
        match self {
            Condition::Clear => "clear",
            Condition::PartlyCloudy => "partly_cloudy",
            Condition::MostlyCloudy => "mostly_cloudy",
            Condition::Cloudy => "cloudy",
            Condition::Rain => "rain",
            Condition::LightRain => "light_rain",
            Condition::HeavyRain => "heavy_rain",
            Condition::ScatteredShowers => "scattered_showers",
            Condition::Snow => "snow",
            Condition::LightSnow => "light_snow",
            Condition::HeavySnow => "heavy_snow",
            Condition::WintryMix => "wintry_mix",
            Condition::LightRainSnow => "light_rain_snow",
            Condition::HeavyRainSnow => "heavy_rain_snow",
            Condition::Thunderstorms => "thunderstorms",
            Condition::ScatteredThunderstorms => "scattered_thunderstorms",
            Condition::Fog => "fog",
            Condition::Hazy => "hazy",
            Condition::Hail => "hail",
            Condition::Windy => "windy",
            Condition::UnknownPrecipitation => "unknown_precipitation",
        }
    }
}

/// One weather observation or forecast step. Every value is optional; a value
/// that is missing (or absurd) is written as FIT's invalid value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Point {
    /// The time the values are for, Unix seconds UTC.
    pub time_unix_s: i64,
    pub temperature_c: Option<f64>,
    pub feels_like_c: Option<f64>,
    pub high_c: Option<f64>,
    pub low_c: Option<f64>,
    pub condition: Option<Condition>,
    /// Metres per second.
    pub wind_speed_ms: Option<f64>,
    /// Degrees, 0 to 360.
    pub wind_direction_deg: Option<f64>,
    /// Percent, 0 to 100.
    pub precipitation_probability: Option<f64>,
    /// Percent, 0 to 100.
    pub humidity_percent: Option<f64>,
    pub dew_point_c: Option<f64>,
    pub uv_index: Option<f64>,
    pub pressure_hpa: Option<f64>,
    /// Day of the week at the location for a daily record, 0 = Sunday.
    pub weekday: Option<u8>,
}

/// What the watch is told: now, the next hours and the next days.
#[derive(Debug, Clone, PartialEq)]
pub struct Forecast {
    /// The place the values are for (the rounded cell), degrees.
    pub lat_deg: f64,
    pub lon_deg: f64,
    /// A place name, if one is known.
    pub location: Option<String>,
    pub current: Point,
    /// Next hours, at most 12 are written.
    pub hourly: Vec<Point>,
    /// Today first, then the following days; at most 5 are written.
    pub daily: Vec<Point>,
}

/// The records of the answer: `definitions` is the payload of one 5011,
/// `records` are the data records to cut into 5012 messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeatherFit {
    pub definitions: Vec<u8>,
    pub records: Vec<Vec<u8>>,
}

/// Hourly records written (`for (hour = 0; hour <= 11; ...)` in Gadgetbridge).
pub const MAX_HOURLY: usize = 12;
/// Daily records written: today and four more days.
pub const MAX_DAILY: usize = 5;

const LOCAL_CURRENT: u8 = 0;
const LOCAL_HOURLY: u8 = 1;
const LOCAL_DAILY: u8 = 2;

const WEEKDAYS: [&str; 7] = [
    "sunday",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
];

/// Encode a forecast. `Err(Unavailable)` without a profile.
pub fn encode(forecast: &Forecast) -> Result<WeatherFit, EncodeError> {
    let (definitions, records) = build(forecast)?.into_parts();
    Ok(WeatherFit {
        definitions,
        records,
    })
}

/// The records of a forecast, kept apart by kind of record.
fn build(forecast: &Forecast) -> Result<RecordSet, EncodeError> {
    if !profile::available() {
        return Err(EncodeError::Unavailable);
    }
    let mut set = RecordSet::new();
    set.push(LOCAL_CURRENT, &current_message(forecast)?.build())?;
    for p in forecast.hourly.iter().take(MAX_HOURLY) {
        set.push(LOCAL_HOURLY, &hourly_message(p)?.build())?;
    }
    for p in forecast.daily.iter().take(MAX_DAILY) {
        // Gadgetbridge stamps every daily record with the time of the forecast.
        set.push(
            LOCAL_DAILY,
            &daily_message(p, forecast.current.time_unix_s)?.build(),
        )?;
    }
    Ok(set)
}

// ---- the three record kinds ------------------------------------------------------------

fn current_message(f: &Forecast) -> Result<MessageBuilder, EncodeError> {
    let p = &f.current;
    let mut m = MessageBuilder::new("weather_conditions")?;
    m.enum_name("weather_report", "current")?;
    let t = garmin_time(p.time_unix_s)?;
    m.uint("timestamp", t)?;
    m.uint("observed_at_time", t)?;
    temperature(&mut m, "temperature", p.temperature_c)?;
    temperature(&mut m, "high_temperature", p.high_c)?;
    temperature(&mut m, "low_temperature", p.low_c)?;
    condition(&mut m, p.condition)?;
    wind_direction(&mut m, p.wind_direction_deg)?;
    percent(
        &mut m,
        "precipitation_probability",
        p.precipitation_probability,
    )?;
    wind_speed(&mut m, p.wind_speed_ms)?;
    temperature(&mut m, "temperature_feels_like", p.feels_like_c)?;
    percent(&mut m, "relative_humidity", p.humidity_percent)?;
    m.int("observed_location_lat", semicircles(f.lat_deg, 90.0))?;
    m.int("observed_location_long", semicircles(f.lon_deg, 180.0))?;
    if let Some(name) = f.location.as_deref().filter(|n| !n.is_empty()) {
        m.string("location", name)?;
    }
    optional_extras(&mut m, p)?;
    Ok(m)
}

fn hourly_message(p: &Point) -> Result<MessageBuilder, EncodeError> {
    let mut m = MessageBuilder::new("weather_conditions")?;
    m.enum_name("weather_report", "hourly_forecast")?;
    m.uint("timestamp", garmin_time(p.time_unix_s)?)?;
    temperature(&mut m, "temperature", p.temperature_c)?;
    condition(&mut m, p.condition)?;
    wind_direction(&mut m, p.wind_direction_deg)?;
    wind_speed(&mut m, p.wind_speed_ms)?;
    percent(
        &mut m,
        "precipitation_probability",
        p.precipitation_probability,
    )?;
    // Gadgetbridge has no separate feels-like for hours and repeats the
    // temperature; when we have one we use it.
    temperature(
        &mut m,
        "temperature_feels_like",
        p.feels_like_c.or(p.temperature_c),
    )?;
    percent(&mut m, "relative_humidity", p.humidity_percent)?;
    optional_extras(&mut m, p)?;
    Ok(m)
}

fn daily_message(p: &Point, forecast_time_unix_s: i64) -> Result<MessageBuilder, EncodeError> {
    let mut m = MessageBuilder::new("weather_conditions")?;
    m.enum_name("weather_report", "daily_forecast")?;
    m.uint("timestamp", garmin_time(forecast_time_unix_s)?)?;
    temperature(&mut m, "low_temperature", p.low_c)?;
    temperature(&mut m, "high_temperature", p.high_c)?;
    condition(&mut m, p.condition)?;
    percent(
        &mut m,
        "precipitation_probability",
        p.precipitation_probability,
    )?;
    match p.weekday.and_then(|d| WEEKDAYS.get(usize::from(d))) {
        Some(day) => {
            m.enum_name("day_of_week", day)?;
        }
        None => {
            m.invalid("day_of_week")?;
        }
    }
    percent(&mut m, "relative_humidity", p.humidity_percent)?;
    wind_speed(&mut m, p.wind_speed_ms)?;
    wind_direction(&mut m, p.wind_direction_deg)?;
    optional_extras(&mut m, p)?;
    Ok(m)
}

// ---- field rules ------------------------------------------------------------------------

/// FIT `date_time`: seconds since 1989-12-31T00:00Z.
fn garmin_time(unix_s: i64) -> Result<u64, EncodeError> {
    u64::try_from(unix_s - GARMIN_EPOCH_UNIX_S)
        .map_err(|_| EncodeError::Invalid("a weather time is before the FIT epoch".into()))
}

/// Degrees to FIT semicircles (2^31 = 180 degrees), clamped to the valid range
/// of the axis.
fn semicircles(deg: f64, limit: f64) -> i64 {
    let d = if deg.is_finite() {
        deg.clamp(-limit, limit)
    } else {
        0.0
    };
    (d * 2_147_483_648.0 / 180.0).round() as i64
}

/// Whole degrees Celsius in a `sint8`; an absurd or missing value is "no value".
fn temperature(m: &mut MessageBuilder, field: &str, c: Option<f64>) -> Result<(), EncodeError> {
    match c.filter(|v| v.is_finite() && (-100.0..=100.0).contains(v)) {
        Some(v) => m.int(field, v.round() as i64)?,
        None => m.invalid(field)?,
    };
    Ok(())
}

fn condition(m: &mut MessageBuilder, c: Option<Condition>) -> Result<(), EncodeError> {
    match c {
        Some(c) => m.enum_name("condition", c.profile_name())?,
        None => m.invalid("condition")?,
    };
    Ok(())
}

/// Metres per second, capped at 65.53 as Gadgetbridge does (the field is a
/// `uint16` in thousandths; 65.535 would be its invalid value).
fn wind_speed(m: &mut MessageBuilder, ms: Option<f64>) -> Result<(), EncodeError> {
    match ms.filter(|v| v.is_finite() && *v >= 0.0) {
        Some(v) => m.scaled("wind_speed", v.min(65.53))?,
        None => m.invalid("wind_speed")?,
    };
    Ok(())
}

fn wind_direction(m: &mut MessageBuilder, deg: Option<f64>) -> Result<(), EncodeError> {
    match deg.filter(|v| v.is_finite() && *v >= 0.0) {
        Some(v) => m.uint("wind_direction", (v.round() as u64) % 360)?,
        None => m.invalid("wind_direction")?,
    };
    Ok(())
}

/// 0 to 100 percent in a `uint8`.
fn percent(m: &mut MessageBuilder, field: &str, v: Option<f64>) -> Result<(), EncodeError> {
    match v.filter(|v| v.is_finite() && *v >= 0.0) {
        Some(v) => m.uint(field, v.round().min(100.0) as u64)?,
        None => m.invalid(field)?,
    };
    Ok(())
}

/// Fields not every profile has: written (or marked invalid) only when the
/// local profile knows them, so a layout never depends on the data.
fn optional_extras(m: &mut MessageBuilder, p: &Point) -> Result<(), EncodeError> {
    let global = m.global();
    let has = |name: &str| profile::field_by_name(global, name).is_some();
    if has("dew_point") {
        temperature(m, "dew_point", p.dew_point_c)?;
    }
    if has("uv_index") {
        // Gadgetbridge clamps to 10 (both scales call 10 and above extreme).
        match p.uv_index.filter(|v| v.is_finite() && *v >= 0.0) {
            Some(v) => m.float("uv_index", v.min(10.0))?,
            None => m.invalid("uv_index")?,
        };
    }
    if has("atmospheric_pressure") {
        match p.pressure_hpa.filter(|v| v.is_finite() && *v > 0.0) {
            Some(v) => m.uint("atmospheric_pressure", (v * 100.0).round() as u64)?,
            None => m.invalid("atmospheric_pressure")?,
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Forecast {
        let t0 = 1_800_000_000;
        let point = |i: i64| Point {
            time_unix_s: t0 + 3600 * i,
            temperature_c: Some(7.4 + i as f64),
            feels_like_c: Some(5.0),
            high_c: Some(9.0),
            low_c: Some(2.0),
            condition: Some(Condition::LightRain),
            wind_speed_ms: Some(4.5),
            wind_direction_deg: Some(250.0),
            precipitation_probability: Some(60.0),
            humidity_percent: Some(81.0),
            dew_point_c: Some(3.0),
            uv_index: Some(0.4),
            pressure_hpa: Some(1012.3),
            weekday: Some(1),
        };
        Forecast {
            lat_deg: 55.7,
            lon_deg: 12.6,
            location: None,
            current: point(0),
            hourly: (1..=14).map(point).collect(),
            daily: (0..6).map(point).collect(),
        }
    }

    #[test]
    fn condition_names_are_plain_lower_case_words() {
        for c in [
            Condition::Clear,
            Condition::PartlyCloudy,
            Condition::HeavyRainSnow,
            Condition::ScatteredThunderstorms,
            Condition::UnknownPrecipitation,
        ] {
            let n = c.profile_name();
            assert!(n.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{n}");
        }
    }

    #[test]
    fn times_and_places_convert_to_fit_units() {
        assert_eq!(garmin_time(GARMIN_EPOCH_UNIX_S).unwrap(), 0);
        assert_eq!(garmin_time(GARMIN_EPOCH_UNIX_S + 977).unwrap(), 977);
        assert!(garmin_time(0).is_err());
        assert_eq!(semicircles(90.0, 90.0), 1_073_741_824);
        assert_eq!(semicircles(-180.0, 180.0), -2_147_483_648);
        assert_eq!(semicircles(f64::NAN, 180.0), 0);
        // the clamp keeps an out-of-range latitude on the globe
        assert_eq!(semicircles(120.0, 90.0), 1_073_741_824);
    }

    /// The committed configuration has no profile: nothing is written.
    #[cfg(not(fit_profile))]
    #[test]
    fn without_a_profile_the_weather_is_unavailable_never_wrong_bytes() {
        assert_eq!(encode(&sample()).unwrap_err(), EncodeError::Unavailable);
        let mut f = sample();
        f.current.time_unix_s = 0;
        assert_eq!(encode(&f).unwrap_err(), EncodeError::Unavailable);
    }

    #[cfg(fit_profile)]
    mod with_profile {
        use super::*;
        use crate::fit::interpret::{interpret, Interpreted};
        use crate::fit::{decode, Crc};

        /// Wrap the answer as one file so the decoder can read it back.
        fn round_trip(f: &Forecast) -> (WeatherFit, Vec<Interpreted>) {
            let fit = encode(f).expect("encodes");
            // definitions first, then the records, exactly as the watch gets them
            let file = build(f).unwrap().to_file().unwrap();
            let d = decode(&file);
            assert_eq!(d.error, None);
            assert_eq!(d.files.len(), 1);
            assert_eq!(d.files[0].crc, Crc::Ok);
            let msgs = interpret(&d.files[0]);
            (fit, msgs)
        }

        #[test]
        fn the_answer_is_current_then_hours_then_days_in_three_local_types() {
            let (fit, msgs) = round_trip(&sample());
            assert_eq!(msgs.len(), 1 + 12 + 5, "hours and days are capped");
            assert!(msgs
                .iter()
                .all(|m| m.message_name == Some("weather_conditions")));
            let reports: Vec<_> = msgs
                .iter()
                .map(|m| m.get("weather_report").unwrap().enum_label.unwrap())
                .collect();
            assert_eq!(reports[0], "current");
            assert!(reports[1..13].iter().all(|r| *r == "hourly_forecast"));
            assert!(reports[13..].iter().all(|r| *r == "daily_forecast"));
            // local types 0, 1 and 2 only: the first byte of every record
            let heads: Vec<u8> = fit.records.iter().map(|r| r[0]).collect();
            assert_eq!(heads[0], 0);
            assert!(heads[1..13].iter().all(|h| *h == 1));
            assert!(heads[13..].iter().all(|h| *h == 2));
            // three definitions, each starting with a definition header
            assert_eq!(fit.definitions[0], 0x40);
        }

        #[test]
        fn values_use_the_fit_units() {
            let (_, msgs) = round_trip(&sample());
            let cur = &msgs[0];
            assert_eq!(cur.number("temperature"), Some(7.0)); // 7.4 rounds to 7
            assert_eq!(cur.number("high_temperature"), Some(9.0));
            assert_eq!(cur.number("low_temperature"), Some(2.0));
            assert_eq!(cur.number("temperature_feels_like"), Some(5.0));
            assert_eq!(cur.number("wind_speed"), Some(4.5));
            assert_eq!(cur.number("wind_direction"), Some(250.0));
            assert_eq!(cur.number("precipitation_probability"), Some(60.0));
            assert_eq!(cur.number("relative_humidity"), Some(81.0));
            assert_eq!(cur.get("condition").unwrap().enum_label, Some("light_rain"));
            // the timestamps are the same Garmin-epoch second
            assert_eq!(
                cur.number("timestamp"),
                Some((1_800_000_000 - GARMIN_EPOCH_UNIX_S) as f64)
            );
            assert_eq!(cur.number("observed_at_time"), cur.number("timestamp"));
            // semicircles back to degrees within a few metres
            let lat = cur.number("observed_location_lat").unwrap() * 180.0 / 2_147_483_648.0;
            let lon = cur.number("observed_location_long").unwrap() * 180.0 / 2_147_483_648.0;
            assert!((lat - 55.7).abs() < 1e-6 && (lon - 12.6).abs() < 1e-6);
        }

        #[test]
        fn daily_records_carry_the_forecast_time_and_the_weekday() {
            let (_, msgs) = round_trip(&sample());
            let day = &msgs[13];
            assert_eq!(day.get("day_of_week").unwrap().enum_label, Some("monday"));
            assert_eq!(
                day.number("timestamp"),
                Some((1_800_000_000 - GARMIN_EPOCH_UNIX_S) as f64),
                "every daily record is stamped with the forecast time"
            );
            assert_eq!(day.number("high_temperature"), Some(9.0));
            // hourly records have their own hour
            let hour = &msgs[2];
            assert_eq!(
                hour.number("timestamp"),
                Some((1_800_000_000 + 2 * 3600 - GARMIN_EPOCH_UNIX_S) as f64)
            );
        }

        #[test]
        fn missing_values_keep_the_layout_and_read_back_as_no_value() {
            let mut f = sample();
            f.hourly[1] = Point {
                time_unix_s: f.hourly[1].time_unix_s,
                ..Point::default()
            };
            f.daily[2].weekday = None;
            f.daily[3].condition = None;
            let (_, msgs) = round_trip(&f); // a layout mismatch would be an Err above
            assert_eq!(msgs[2].number("temperature"), None);
            assert_eq!(msgs[2].get("condition").and_then(|f| f.enum_label), None);
            assert_eq!(msgs[1].number("temperature"), Some(8.0)); // 7.4 + 1 = 8.4
        }

        #[test]
        fn absurd_values_are_no_value_not_wrapped_numbers() {
            let mut f = sample();
            f.current.temperature_c = Some(900.0);
            f.current.wind_speed_ms = Some(500.0); // capped
            f.current.humidity_percent = Some(250.0); // clamped
            f.current.wind_direction_deg = Some(725.0); // wraps once around
            let (_, msgs) = round_trip(&f);
            assert_eq!(msgs[0].number("temperature"), None);
            assert_eq!(msgs[0].number("wind_speed"), Some(65.53));
            assert_eq!(msgs[0].number("relative_humidity"), Some(100.0));
            assert_eq!(msgs[0].number("wind_direction"), Some(5.0));
        }

        #[test]
        fn a_time_before_the_fit_epoch_is_refused() {
            let mut f = sample();
            f.current.time_unix_s = 1000;
            assert!(matches!(encode(&f), Err(EncodeError::Invalid(_))));
        }

        #[test]
        fn a_location_name_is_written_when_given() {
            let mut f = sample();
            f.location = Some("Testby".into());
            let (_, msgs) = round_trip(&f);
            assert!(msgs[0].get("location").is_some());
            assert!(msgs[1].get("location").is_none());
        }
    }
}
