//! Weather (5014 from the watch; 5011 and 5012 to the watch): the message layouts.
//!
//! The watch asks for weather with 5014. The phone acknowledges it and answers
//! with FIT `weather_conditions` messages: one 5011 holding the FIT *definition*
//! records and one or more 5012 holding the FIT *data* records. The FIT bytes
//! themselves are made by the server (`crate::fit::weather`, which needs the
//! local FIT profile); this file and `client/weather.rs` only wrap opaque bytes
//! and read the watch's replies, so the public Android build needs no profile.
//!
//! Position privacy: the watch's position leaves this module only as a
//! [`WeatherCell`], rounded to 0.1 degree (about 10 km). The exact value is
//! dropped inside [`parse_weather_request`] and never stored.
//!
//! Ported from Gadgetbridge commit 7257742b (AGPL-3.0-or-later),
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/`:
//! `messages/WeatherRequestMessage.java` (the 5014 layout; no copyright line in
//! the file), `messages/FitDefinitionMessage.java` and `messages/FitDataMessage.java`
//! (what 5011 and 5012 carry: the records without a FIT file header),
//! `messages/status/FitDefinitionStatusMessage.java` and
//! `messages/status/FitDataStatusMessage.java` (the status code after the ACK
//! byte), `FitLocalMessageHandler.java` (definitions first, then the data records
//! in chunks, each chunk waiting for the watch's status) and `GarminSupport.java`
//! (`sendWeatherConditions`; Copyright (C) 2024-2026 Daniele Gobbetti, José
//! Rebelo, kuhy, Thomas Kuehne and the other Gadgetbridge contributors),
//! `devices/garmin/GarminCapability.java` (WEATHER_CONDITIONS is bit 26). All of
//! it is *inferred (Gadgetbridge)* until a watch confirms it. The units of the
//! two coordinates are not decoded by Gadgetbridge; reading them as FIT
//! semicircles is our own reading (Garmin's convention), *inferred*, see
//! docs/protocol/OPEN-QUESTIONS.md Q-COMP-3. What we do differently is marked
//! [ours].

/// 5014, watch to phone: "send me the weather".
pub const WEATHER_REQUEST: u16 = 5014;
/// 5011, phone to watch: FIT definition records.
pub const FIT_DEFINITION: u16 = 5011;
/// 5012, phone to watch: FIT data records.
pub const FIT_DATA: u16 = 5012;

/// Capability bit (in either Configuration mask) for "weather as FIT
/// conditions" (`GarminCapability.WEATHER_CONDITIONS`). Bit `i` is byte `i / 8`,
/// value `1 << (i % 8)`. Bit 27 (weather alerts) is never claimed. *inferred
/// (Gadgetbridge)*
pub const CAP_WEATHER_CONDITIONS: usize = 26;

/// What a 5014 payload must hold at least: `u8 format, s32 lat, s32 lon, u8 hours`.
const REQUEST_LEN: usize = 10;

/// FIT's invalid value for a `sint32` (also what a watch without a position
/// fix would send, if it follows FIT). [ours]
const SINT32_INVALID: i32 = i32::MAX;

/// The most FIT bytes (definitions plus data) one answer may carry. A real
/// answer is about a kilobyte; this only stops a runaway caller. [ours]
pub const MAX_WEATHER_BYTES: usize = 16 * 1024;

/// A place on a 0.1 degree grid (about 10 km): the finest position the phone
/// and the server ever get. Latitude in -900..=900, longitude in -1800..=1800,
/// both in tenths of a degree. Its `Debug` prints nothing of the position.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct WeatherCell {
    pub lat_tenths: i16,
    pub lon_tenths: i16,
}

impl std::fmt::Debug for WeatherCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WeatherCell").finish_non_exhaustive()
    }
}

impl WeatherCell {
    /// Round a position given as FIT semicircles (2^31 = 180 degrees) to the
    /// grid. `None` for the invalid value, for "0, 0" (no fix) and for
    /// anything outside the globe. [ours]
    pub fn from_semicircles(lat: i32, lon: i32) -> Option<Self> {
        if lat == SINT32_INVALID || lon == SINT32_INVALID || (lat == 0 && lon == 0) {
            return None;
        }
        let tenths = |semi: i32| (f64::from(semi) * 1800.0 / 2_147_483_648.0).round() as i32;
        let (la, lo) = (tenths(lat), tenths(lon));
        if !(-900..=900).contains(&la) || !(-1800..=1800).contains(&lo) {
            return None;
        }
        Some(Self {
            lat_tenths: la as i16,
            lon_tenths: lo as i16,
        })
    }

    /// Latitude in degrees (a multiple of 0.1).
    pub fn lat_deg(self) -> f64 {
        f64::from(self.lat_tenths) / 10.0
    }

    /// Longitude in degrees (a multiple of 0.1).
    pub fn lon_deg(self) -> f64 {
        f64::from(self.lon_tenths) / 10.0
    }
}

/// What a 5014 says, with the position already rounded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeatherRequest {
    /// The format byte as sent (its meaning is unknown; Gadgetbridge ignores it).
    pub format: u8,
    /// `None` when the watch has no usable position.
    pub cell: Option<WeatherCell>,
    /// Hours of forecast the watch asks for (Gadgetbridge sends up to 12 and
    /// ignores the number).
    pub hours: u8,
}

/// 5014 payload: `u8 format, s32 latitude, s32 longitude, u8 hours` (little
/// endian). `None` when it is shorter than 10 bytes.
pub fn parse_weather_request(p: &[u8]) -> Option<WeatherRequest> {
    if p.len() < REQUEST_LEN {
        return None;
    }
    let lat = i32::from_le_bytes([p[1], p[2], p[3], p[4]]);
    let lon = i32::from_le_bytes([p[5], p[6], p[7], p[8]]);
    Some(WeatherRequest {
        format: p[0],
        cell: WeatherCell::from_semicircles(lat, lon),
        hours: p[9],
    })
}

/// How a `Input::SendWeather` ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherResult {
    /// The watch took the definitions and every data chunk.
    Sent,
    /// Nothing was sent: the weather duty is off, the handshake is not done,
    /// or the watch does not have capability 26 (the HTTP path is not built).
    NotSupported,
    /// Nothing was sent: an earlier weather answer is still on its way.
    Busy,
    /// Nothing was sent: the bytes are not FIT definition and data records, are
    /// empty, too large, or a record does not fit one message.
    Invalid,
    /// The watch refused a message or answered with a status other than
    /// "applied" for the data. The session goes on.
    Refused,
    /// The watch did not answer within the reply deadline. The session goes on.
    TimedOut,
}

/// Longest GFDI message (length field included) whose COBS frame, with its two
/// delimiters, still fits `max_packet`; 0 when `max_packet` is unknown. [ours,
/// the same bound as `client::upload::chunk_data_size`]
pub fn max_message_len(max_packet: u16) -> usize {
    let max = usize::from(max_packet);
    let mut len = max.saturating_sub(3);
    while len > 0 && len + len / 254 + 3 > max {
        len -= 1;
    }
    len
}

/// A message is its payload plus length 2, id 2 and CRC 2.
const MESSAGE_OVERHEAD: usize = 6;

/// The most payload bytes one 5011 or 5012 may carry for `max_packet`.
pub fn payload_budget(max_packet: u16) -> usize {
    max_message_len(max_packet).saturating_sub(MESSAGE_OVERHEAD)
}

/// A FIT definition record starts with a header byte `0b01xx_xxxx`.
fn is_definition_header(b: u8) -> bool {
    b & 0xC0 == 0x40
}

/// A normal FIT data record starts with a header byte `0b00xx_xxxx`.
fn is_data_header(b: u8) -> bool {
    b & 0xC0 == 0x00
}

/// Check what the host hands over and cut the data records into 5012 payloads:
/// each holds whole records, as many as fit `budget` (Gadgetbridge's
/// `FitLocalMessageHandler.sendFollowUp` rule). `Err(())` when the bytes cannot
/// be sent: empty, too large, not records, or a record bigger than one message.
#[allow(clippy::result_unit_err)]
pub fn plan(definitions: &[u8], records: &[Vec<u8>], budget: usize) -> Result<Vec<Vec<u8>>, ()> {
    let total: usize = definitions.len() + records.iter().map(Vec::len).sum::<usize>();
    if budget == 0
        || definitions.is_empty()
        || definitions.len() > budget
        || records.is_empty()
        || total > MAX_WEATHER_BYTES
        || !is_definition_header(definitions[0])
    {
        return Err(());
    }
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    for r in records {
        if r.is_empty() || r.len() > budget || !is_data_header(r[0]) {
            return Err(());
        }
        if !current.is_empty() && current.len() + r.len() > budget {
            chunks.push(std::mem::take(&mut current));
        }
        current.extend_from_slice(r);
    }
    chunks.push(current);
    Ok(chunks)
}

/// The status code after the ACK byte in the watch's answer to 5011 (0 applied,
/// 1 not unique, 2 out of range, 3 not ready) or 5012 (0 applied, 1 no
/// definition, 2 mismatch, 3 not ready). `None` when the body is empty.
pub fn status_code(body: &[u8]) -> Option<u8> {
    body.first().copied()
}

/// What the watch's status code for 5012 means.
pub const CODE_APPLIED: u8 = 0;

#[cfg(test)]
mod tests {
    use super::*;

    fn request(format: u8, lat: i32, lon: i32, hours: u8) -> Vec<u8> {
        let mut p = vec![format];
        p.extend_from_slice(&lat.to_le_bytes());
        p.extend_from_slice(&lon.to_le_bytes());
        p.push(hours);
        p
    }

    /// Degrees to FIT semicircles, as a watch would write them.
    fn semi(deg: f64) -> i32 {
        (deg * 2_147_483_648.0 / 180.0).round() as i32
    }

    #[test]
    fn a_position_is_rounded_to_a_tenth_of_a_degree() {
        // Copenhagen city centre, about 55.6761, 12.5683
        let r = parse_weather_request(&request(1, semi(55.6761), semi(12.5683), 12)).unwrap();
        assert_eq!(r.format, 1);
        assert_eq!(r.hours, 12);
        let cell = r.cell.unwrap();
        assert_eq!((cell.lat_tenths, cell.lon_tenths), (557, 126));
        assert!((cell.lat_deg() - 55.7).abs() < 1e-9);
        assert!((cell.lon_deg() - 12.6).abs() < 1e-9);
    }

    #[test]
    fn negative_positions_round_away_from_zero_at_the_half() {
        let r = parse_weather_request(&request(0, semi(-33.8688), semi(-151.2093), 6)).unwrap();
        let cell = r.cell.unwrap();
        assert_eq!((cell.lat_tenths, cell.lon_tenths), (-339, -1512));
        // just either side of the half: 10.0504 degrees is 100.504 tenths
        let c = WeatherCell::from_semicircles(semi(10.0504), semi(20.0)).unwrap();
        assert_eq!(c.lat_tenths, 101);
        let c = WeatherCell::from_semicircles(semi(10.0496), semi(20.0)).unwrap();
        assert_eq!(c.lat_tenths, 100);
        let c = WeatherCell::from_semicircles(semi(-10.0504), semi(20.0)).unwrap();
        assert_eq!(c.lat_tenths, -101);
    }

    #[test]
    fn two_places_in_one_cell_give_the_same_cell() {
        let a = WeatherCell::from_semicircles(semi(55.651), semi(12.551)).unwrap();
        let b = WeatherCell::from_semicircles(semi(55.699), semi(12.599)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn no_fix_and_invalid_positions_are_no_cell() {
        for (lat, lon) in [
            (0, 0),
            (i32::MAX, i32::MAX),
            (i32::MAX, semi(12.0)),
            (semi(12.0), i32::MAX),
            // beyond 90 degrees of latitude
            (semi(95.0), semi(10.0)),
            (i32::MIN, 0),
        ] {
            let r = parse_weather_request(&request(1, lat, lon, 12)).unwrap();
            assert_eq!(r.cell, None, "{lat} {lon}");
        }
    }

    #[test]
    fn the_poles_and_the_date_line_stay_in_range() {
        let c = WeatherCell::from_semicircles(semi(90.0), semi(-179.99)).unwrap();
        assert_eq!((c.lat_tenths, c.lon_tenths), (900, -1800));
        let c = WeatherCell::from_semicircles(semi(-90.0), i32::MIN).unwrap();
        assert_eq!((c.lat_tenths, c.lon_tenths), (-900, -1800));
    }

    #[test]
    fn a_short_payload_is_not_a_request() {
        assert_eq!(parse_weather_request(&[0; 9]), None);
        assert_eq!(parse_weather_request(&[]), None);
        // extra bytes are tolerated (Gadgetbridge only warns)
        assert!(parse_weather_request(&[0; 12]).is_some());
    }

    #[test]
    fn debug_prints_nothing_of_the_position() {
        let c = WeatherCell {
            lat_tenths: 557,
            lon_tenths: 126,
        };
        let s = format!("{c:?}");
        assert!(!s.contains("557") && !s.contains("126"), "{s}");
    }

    #[test]
    fn message_length_bound_keeps_the_frame_inside_max_packet() {
        for max in [20u16, 100, 255, 256, 400, 1000, 4000, 5000] {
            let len = max_message_len(max);
            assert!(len + len / 254 + 3 <= usize::from(max), "{max}");
            assert!(
                (len + 1) + (len + 1) / 254 + 3 > usize::from(max),
                "{max}: not the largest"
            );
        }
        assert_eq!(max_message_len(0), 0);
        assert_eq!(payload_budget(0), 0);
        assert_eq!(payload_budget(4000), max_message_len(4000) - 6);
    }

    fn def() -> Vec<u8> {
        // a made-up definition record: header 0x40, reserved, little endian,
        // global 128, one field
        vec![0x40, 0, 0, 128, 0, 1, 0, 1, 0x02]
    }

    #[test]
    fn records_are_packed_greedily_into_whole_records() {
        let records: Vec<Vec<u8>> = (0..5).map(|i| vec![0x00, i, i, i, i]).collect(); // 5 bytes each
        let chunks = plan(&def(), &records, 12).unwrap();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 10);
        assert_eq!(chunks[1].len(), 10);
        assert_eq!(chunks[2].len(), 5);
        assert_eq!(chunks.concat(), records.concat());
        // one chunk when everything fits
        assert_eq!(plan(&def(), &records, 25).unwrap().len(), 1);
    }

    #[test]
    fn bytes_that_cannot_be_sent_are_refused_up_front() {
        let rec = vec![vec![0x00, 1, 2]];
        assert!(plan(&def(), &rec, 100).is_ok());
        assert!(plan(&[], &rec, 100).is_err(), "no definitions");
        assert!(plan(&def(), &[], 100).is_err(), "no records");
        assert!(plan(&def(), &rec, 0).is_err(), "unknown max packet");
        assert!(plan(&def(), &rec, 5).is_err(), "definitions too big");
        assert!(plan(&def(), &[vec![0x00; 9]], 8).is_err(), "record too big");
        assert!(plan(&def(), &[vec![]], 100).is_err(), "empty record");
        assert!(plan(&[0x00, 1, 2], &rec, 100).is_err(), "not a definition");
        assert!(plan(&def(), &[vec![0x40, 1]], 100).is_err(), "not data");
        assert!(
            plan(&def(), &[vec![0x80, 1]], 100).is_err(),
            "compressed header"
        );
        let big = vec![vec![0x00; 200]; 100];
        assert!(plan(&def(), &big, 4000).is_err(), "over the total cap");
    }

    #[test]
    fn the_status_code_is_the_first_body_byte() {
        assert_eq!(status_code(&[0]), Some(0));
        assert_eq!(status_code(&[2, 9]), Some(2));
        assert_eq!(status_code(&[]), None);
    }
}
