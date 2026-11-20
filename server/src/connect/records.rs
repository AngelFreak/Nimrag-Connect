//! Per-second records and time in zone of a Connect activity.
//!
//! The Garmin Connect connector's `get_activity_fit_messages` (record, `time_in_zone` and
//! `zones_target` messages, paged by `offset`) is saved by the puller as `records/<id>_<offset>.json`.
//! The importer reads those pages and keeps only what the tables have columns for, as the
//! `records` member of the activity envelope (a compact, deterministic form, so the same files
//! give the same blob and a re-run changes nothing):
//!
//! ```text
//! "records": {"v":1,
//!             "points": [[ts, lat, lon, hr, speed, altitude_m, distance_m, cadence, temp_c], ...],
//!             "zones":  [{"ref_mesg": 18, "ref_index": 0, "s": [seconds per zone, ...]}, ...]}
//! ```
//!
//! Units are those of a watch FIT file's derived rows: unix seconds UTC, degrees, m/s with
//! `enhanced_speed` preferred, metres with `enhanced_altitude` preferred, seconds per HR zone.
//! The connector's decoder lacks parts of the FIT profile (`time_in_zone` arrives as
//! `unknown_216`, some record fields as `unknown_N`), so fields are found by their definition
//! number and, when the connector did not name them, scaled here with the profile's factors.
//! Zone boundaries are not stored (the tables have no column for them); nothing is estimated
//! from the heart rate.

use super::Parsed;
use nimrag_core::derive::activity::{RecordRow, TimeInZone};
use nimrag_core::derive::FIT_EPOCH_UNIX;
use nimrag_core::fit::interpret::FIT_UTC_MIN;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// FIT global message numbers.
const G_RECORD: i64 = 20;
const G_TIME_IN_ZONE: i64 = 216;

/// Version of the `records` member.
pub const VERSION: u64 = 1;

type Fields<'a> = &'a [Value];

fn fields(m: &Value) -> Fields<'_> {
    m.get("fields")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn field(fs: Fields<'_>, def: i64) -> Option<&Value> {
    fs.iter()
        .find(|f| f.get("definition_number").and_then(Value::as_i64) == Some(def))
}

/// The base type's "invalid" bit pattern, as a number.
fn is_invalid(base_type: Option<&str>, raw: f64) -> bool {
    let max = match base_type {
        Some("uint8" | "enum" | "byte") => 255.0,
        Some("uint16") => 65_535.0,
        Some("uint32") => 4_294_967_295.0,
        Some("sint8") => 127.0,
        Some("sint16") => 32_767.0,
        Some("sint32") => 2_147_483_647.0,
        _ => return false,
    };
    raw == max
}

/// A number field: the connector's own value when it knew the field (`known` is its profile
/// name), else the raw value scaled as the FIT profile does (`raw / scale - offset`). An
/// invalid raw value is `None`.
fn number(fs: Fields<'_>, def: i64, known: &str, scale: f64, offset: f64) -> Option<f64> {
    let f = field(fs, def)?;
    let raw = f.get("raw_value").and_then(Value::as_f64)?;
    if is_invalid(f.get("base_type").and_then(Value::as_str), raw) {
        return None;
    }
    if f.get("name").and_then(Value::as_str) == Some(known) {
        f.get("value").and_then(Value::as_f64)
    } else {
        Some(raw / scale - offset)
    }
}

fn degrees(semicircles: Option<f64>) -> Option<f64> {
    semicircles.map(|s| s * 180.0 / 2_147_483_648.0)
}

/// One `record` message as `[ts, lat, lon, hr, speed, altitude_m, distance_m, cadence, temp_c]`;
/// `None` when it has no timestamp or no value at all (as in the FIT derivation).
fn point(m: &Value) -> Option<Value> {
    let fs = fields(m);
    let ts = field(fs, 253)?.get("raw_value")?.as_f64()?;
    if ts < f64::from(FIT_UTC_MIN) {
        return None;
    }
    let lat = degrees(number(fs, 0, "position_lat", 1.0, 0.0));
    let lon = degrees(number(fs, 1, "position_long", 1.0, 0.0));
    let hr = number(fs, 3, "heart_rate", 1.0, 0.0);
    let speed = number(fs, 73, "enhanced_speed", 1000.0, 0.0)
        .or_else(|| number(fs, 6, "speed", 1000.0, 0.0));
    let altitude = number(fs, 78, "enhanced_altitude", 5.0, 500.0)
        .or_else(|| number(fs, 2, "altitude", 5.0, 500.0));
    let distance = number(fs, 5, "distance", 100.0, 0.0);
    let cadence = number(fs, 4, "cadence", 1.0, 0.0);
    let temp = number(fs, 13, "temperature", 1.0, 0.0);
    let vals = [lat, lon, hr, speed, altitude, distance, cadence, temp];
    if vals.iter().all(Option::is_none) {
        return None;
    }
    Some(json!([
        ts as i64 + FIT_EPOCH_UNIX,
        lat,
        lon,
        hr,
        speed,
        altitude,
        distance,
        cadence,
        temp
    ]))
}

/// One `time_in_zone` message: reference message and index, seconds per HR zone (an invalid
/// element is `null`). `None` when no zone has a value.
fn zones(m: &Value) -> Option<Value> {
    let fs = fields(m);
    let f = field(fs, 2)?;
    let known = f.get("name").and_then(Value::as_str) == Some("time_in_hr_zone");
    let items = f.get("raw_value").and_then(Value::as_array)?;
    let secs: Vec<Option<f64>> = items
        .iter()
        .enumerate()
        .map(|(i, raw)| {
            let raw = raw.as_f64()?;
            if raw == 4_294_967_295.0 {
                return None;
            }
            if known {
                f.get("value")
                    .and_then(Value::as_array)
                    .and_then(|v| v.get(i))
                    .and_then(Value::as_f64)
            } else {
                Some(raw / 1000.0)
            }
        })
        .collect();
    if secs.iter().all(Option::is_none) {
        return None;
    }
    let int = |def: i64| {
        field(fs, def)
            .and_then(|f| f.get("raw_value"))
            .and_then(Value::as_f64)
            .map(|v| v.round() as i64)
    };
    Some(json!({"ref_mesg": int(0), "ref_index": int(1), "s": secs}))
}

/// The `records` member for one activity from its pages (each the parsed JSON of one
/// `records/<id>_<offset>.json`, in any order). `None` when the pages hold neither a record nor
/// a zone row. An error when a page is not for this activity or has no `messages`.
pub fn extract(activity_id: i64, pages: &[Value]) -> Result<Option<Value>, String> {
    // Pages may overlap (a re-pull with another limit): a message is kept once, by its index
    // in the FIT file, and rows stay in file order.
    let mut points: BTreeMap<i64, Value> = BTreeMap::new();
    let mut zone_rows: BTreeMap<i64, Value> = BTreeMap::new();
    for (n, page) in pages.iter().enumerate() {
        if let Some(id) = page.get("activity_id").and_then(Value::as_i64) {
            if id != activity_id {
                return Err(format!("a records page is for activity {id}"));
            }
        }
        let messages = page
            .get("messages")
            .and_then(Value::as_array)
            .ok_or("a records page has no messages")?;
        for (i, m) in messages.iter().enumerate() {
            let key = m
                .get("message_index")
                .and_then(Value::as_i64)
                .unwrap_or((n as i64) << 32 | i as i64);
            match m.get("global_message_number").and_then(Value::as_i64) {
                Some(G_RECORD) => {
                    if let Some(p) = point(m) {
                        points.insert(key, p);
                    }
                }
                Some(G_TIME_IN_ZONE) => {
                    if let Some(z) = zones(m) {
                        zone_rows.insert(key, z);
                    }
                }
                _ => {}
            }
        }
    }
    if points.is_empty() && zone_rows.is_empty() {
        return Ok(None);
    }
    Ok(Some(json!({
        "v": VERSION,
        "points": points.into_values().collect::<Vec<_>>(),
        "zones": zone_rows.into_values().collect::<Vec<_>>(),
    })))
}

/// Push the rows of an envelope's `records` member into the activity rows.
pub fn apply(v: &Value, p: &mut Parsed) -> Result<(), String> {
    if v.get("v").and_then(Value::as_u64) != Some(VERSION) {
        return Err("records: unsupported version".into());
    }
    let num = |row: &Value, i: usize| row.get(i).and_then(Value::as_f64);
    for row in v
        .get("points")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
    {
        let Some(ts) = row.get(0).and_then(Value::as_i64) else {
            p.skipped += 1;
            continue;
        };
        let rec = RecordRow {
            idx: p.derived.activity.records.len() as u32,
            ts,
            lat: num(row, 1),
            lon: num(row, 2),
            hr: num(row, 3),
            speed: num(row, 4),
            altitude_m: num(row, 5),
            distance_m: num(row, 6),
            cadence: num(row, 7),
            temperature_c: num(row, 8),
        };
        p.last_ts = p.last_ts.max(Some(ts));
        p.derived.activity.records.push(rec);
        p.rows += 1;
    }
    for z in v
        .get("zones")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
    {
        let secs: Vec<Option<f64>> = z
            .get("s")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(Value::as_f64)
            .collect();
        if secs.iter().all(Option::is_none) {
            continue;
        }
        p.derived.activity.time_in_zone.push(TimeInZone {
            idx: p.derived.activity.time_in_zone.len() as u32,
            ref_mesg: z.get("ref_mesg").and_then(Value::as_i64),
            ref_index: z.get("ref_index").and_then(Value::as_i64),
            hr_zone_s: secs,
        });
        p.rows += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(name: &str, def: i64, base: &str, raw: Value, value: Value) -> Value {
        json!({"name": name, "definition_number": def, "base_type": base,
               "raw_value": raw, "value": value})
    }

    #[test]
    fn a_record_is_read_by_definition_number_and_scaled_when_unnamed() {
        let m = json!({"type": "record", "global_message_number": 20, "fields": [
            f("timestamp", 253, "uint32", json!(1_163_255_154u32), json!("x")),
            f("unknown_73", 73, "uint32", json!(2500), json!(2500)),
            f("unknown_78", 78, "uint32", json!(2464), json!(2464)),
            f("heart_rate", 3, "uint8", json!(101), json!(101)),
            f("unknown_134", 134, "uint8", json!(7), json!(7)),
            f("distance", 5, "uint32", json!(54), json!(0.54)),
            f("position_lat", 0, "sint32", json!(2_147_483_647), json!(null)),
        ]});
        let p = point(&m).unwrap();
        assert_eq!(p[0], 1_163_255_154i64 + FIT_EPOCH_UNIX);
        assert_eq!(p[1], Value::Null, "an invalid position is no position");
        assert_eq!(p[3], 101.0);
        assert_eq!(p[4], 2.5);
        assert!(
            (p[5].as_f64().unwrap() + 7.2).abs() < 1e-9,
            "2464 / 5 - 500"
        );
    }

    #[test]
    fn a_record_without_a_timestamp_or_value_is_no_row() {
        let no_ts = json!({"fields": [f("heart_rate", 3, "uint8", json!(90), json!(90))]});
        let device_time = json!({"fields": [
            f("timestamp", 253, "uint32", json!(1000), json!("x")),
            f("heart_rate", 3, "uint8", json!(90), json!(90)),
        ]});
        let empty = json!({"fields": [
            f("timestamp", 253, "uint32", json!(1_163_255_154u32), json!("x")),
            f("heart_rate", 3, "uint8", json!(null), json!(null)),
        ]});
        assert!(point(&no_ts).is_none());
        assert!(point(&device_time).is_none(), "below the UTC threshold");
        assert!(point(&empty).is_none());
    }

    #[test]
    fn zones_are_seconds_with_invalid_elements_null() {
        let raw = json!([1500, 4_294_967_295u32, 0]);
        let m = json!({"fields": [
            f("unknown_0", 0, "uint16", json!(18), json!(18)),
            f("unknown_1", 1, "uint16", json!(0), json!(0)),
            f("unknown_2", 2, "uint32", raw.clone(), raw),
        ]});
        assert_eq!(
            zones(&m).unwrap(),
            json!({"ref_mesg": 18, "ref_index": 0, "s": [1.5, null, 0.0]})
        );
        let nulls = json!([null, null]);
        let none = json!({"fields": [f("unknown_2", 2, "uint32", nulls.clone(), nulls)]});
        assert!(zones(&none).is_none());
    }

    #[test]
    fn extract_merges_overlapping_pages_and_refuses_another_activity() {
        let rec = |i: i64| {
            json!({"message_index": i, "global_message_number": 20, "fields": [
                f("timestamp", 253, "uint32", json!(1_163_255_154i64 + i), json!("x")),
                f("heart_rate", 3, "uint8", json!(100 + i), json!(100 + i)),
            ]})
        };
        let p1 = json!({"activity_id": 5, "messages": [rec(0), rec(1)]});
        let p2 = json!({"activity_id": 5, "messages": [rec(1), rec(2)]});
        // pages in any order
        let r = extract(5, &[p2, p1.clone()]).unwrap().unwrap();
        assert_eq!(r["points"].as_array().unwrap().len(), 3);
        assert_eq!(r["points"][2][3], 102.0);
        assert!(extract(6, &[p1]).is_err());
        assert_eq!(
            extract(5, &[json!({"activity_id": 5, "messages": []})]),
            Ok(None)
        );
        assert!(extract(5, &[json!({"activity_id": 5})]).is_err());
    }
}
