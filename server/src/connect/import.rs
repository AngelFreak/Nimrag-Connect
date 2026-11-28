//! `import-connect`: store a directory of Garmin Connect JSON as blobs (one envelope per input
//! file, one per activity of an activity list) through the one ingest path, owned by one user.
//!
//! `DIR/records/<activity id>_<offset>.json` (pages of the connector's `get_activity_fit_messages`
//! with the record, time in zone and zones target messages, see [`super::records`]) is optional:
//! an activity file with pages gets its records folded into its envelope, one without is stored
//! as before. An activity imported earlier without records becomes a second blob (the envelope
//! has the records now) that outranks it by row count, so a re-run after the pull adds the track
//! and the charts; the older blob stays stored and is hidden by `activities_current`.
//!
//! `DIR/daily_stats/YYYY-MM.json` (a month of Garmin's per-day summaries, read by
//! `connect/rows.rs`) is optional too: it is found like any other JSON (by its shape, a list of
//! days) and a directory without it imports exactly as before.
//!
//! Re-running is idempotent: identical input gives identical envelopes, so every blob is already
//! stored and ledgered (the origin, the path inside the directory, folds repeats into the
//! existing event), and nothing is derived twice. Files that are not recognised Connect JSON
//! are listed and not stored.

use super::{records, wrap, wrap_with_records, Kind};
use crate::api::AppState;
use crate::ledger::Meta;
use crate::{ingest, store};
use anyhow::{Context, Result};
use chrono_tz::Tz;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Largest input file read.
const MAX_FILE_BYTES: u64 = 256 << 20;

/// Where the record pages are, below the input directory.
const RECORDS_DIR: &str = "records";

/// Record pages by activity id: (offset, path) in offset order. Names that are not
/// `<id>_<offset>.json` are ignored. A missing directory is no pages.
fn record_pages(dir: &Path) -> BTreeMap<i64, Vec<(i64, PathBuf)>> {
    let mut out: BTreeMap<i64, Vec<(i64, PathBuf)>> = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir.join(RECORDS_DIR)) else {
        return out;
    };
    for e in entries.flatten() {
        if !e.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        let Some((id, offset)) = stem.split_once('_') else {
            continue;
        };
        if let (Ok(id), Ok(offset)) = (id.parse::<i64>(), offset.parse::<i64>()) {
            out.entry(id).or_default().push((offset, e.path()));
        }
    }
    for pages in out.values_mut() {
        pages.sort();
    }
    out
}

/// The `records` member of one activity from its pages on disk.
fn read_records(pages: &[(i64, PathBuf)], id: i64) -> Result<Option<Value>, String> {
    let mut parsed = Vec::new();
    for (_, path) in pages {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let len = std::fs::metadata(path)
            .map_err(|e| format!("records/{name}: {e}"))?
            .len();
        if len > MAX_FILE_BYTES {
            return Err(format!("records/{name}: larger than the size limit"));
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("records/{name}: {e}"))?;
        parsed.push(
            serde_json::from_str::<Value>(&text).map_err(|e| format!("records/{name}: {e}"))?,
        );
    }
    records::extract(id, &parsed)
}

#[derive(Debug, Default)]
pub struct Report {
    /// Input files looked at (`*.json`).
    pub files: u64,
    /// Blobs stored for the first time and blobs that were already there, by kind.
    pub by_kind: BTreeMap<&'static str, (u64, u64)>,
    /// Activity blobs (new or already there) that carry records.
    pub with_records: u64,
    /// Activities that have `records/` pages and no activity file in the directory.
    pub records_without_activity: Vec<i64>,
    /// Files that are not Connect JSON this importer knows (path inside the directory).
    pub unrecognised: Vec<String>,
    pub errors: Vec<String>,
}

impl Report {
    pub fn new_blobs(&self) -> u64 {
        self.by_kind.values().map(|v| v.0).sum()
    }
    pub fn present_blobs(&self) -> u64 {
        self.by_kind.values().map(|v| v.1).sum()
    }
}

/// A field that only a `daily_stats` day has (a day of `stats.json` has none of them).
const DAILY_STATS_KEYS: &[&str] = &[
    "stress_avg",
    "bb_high",
    "resp_avg",
    "hr_max",
    "moderate_min",
    "floors_up",
];

/// Decide what a parsed Connect JSON is, from its shape (never from its file name).
pub fn classify(v: &Value) -> Option<Kind> {
    // A month of per-day summaries (`daily_stats/YYYY-MM.json`): a list of days.
    if let Some(days) = v.as_array() {
        let looks = days.first().is_some_and(|d| {
            d.get("date").is_some_and(Value::is_string)
                && DAILY_STATS_KEYS.iter().any(|k| d.get(k).is_some())
        });
        return looks.then_some(Kind::DailyStats);
    }
    let obj = v.as_object()?;
    // A day of minute-level series: a descriptor list and the values array beside it.
    let has = |k: &str| obj.get(k).is_some_and(Value::is_array);
    if has("stressValueDescriptorsDTOList") && has("stressValuesArray") {
        return Some(Kind::IntradayStress);
    }
    if has("heartRateValueDescriptors") && has("heartRateValues") {
        return Some(Kind::IntradayHeartRate);
    }
    if has("respirationValueDescriptorsDTOList") && has("respirationValuesArray") {
        return Some(Kind::IntradayRespiration);
    }
    // One night of the full sleep response.
    if obj.get("dailySleepDTO").is_some_and(Value::is_object) && has("sleepLevels") {
        return Some(Kind::SleepDetail);
    }
    if obj.contains_key("nights") {
        return Some(Kind::Sleep);
    }
    if obj.contains_key("activities") {
        return None; // a list: split by the caller, see `split_list`
    }
    if obj.contains_key("summary") && obj.contains_key("fit") {
        return Some(Kind::Activity);
    }
    if let Some(days) = obj.get("days").and_then(Value::as_array) {
        let first = days.first();
        let looks =
            first.is_none_or(|d| d.get("steps").is_some() || d.get("total_calories").is_some());
        return looks.then_some(Kind::Stats);
    }
    if let Some(trend) = obj.get("trend").and_then(Value::as_array) {
        let first = trend.first()?;
        return if first.get("atl").is_some() {
            Some(Kind::TrainingLoad)
        } else if first.get("last_night_avg_hrv_ms").is_some()
            || first.get("weekly_avg_hrv_ms").is_some()
            || first.get("feedback").is_some()
        {
            Some(Kind::HrvTrend)
        } else if first.get("vo2_max").is_some() {
            Some(Kind::Vo2max)
        } else {
            None
        };
    }
    // A map keyed by date.
    if !obj.is_empty() && obj.keys().all(|k| crate::settings::parse_date(k).is_some()) {
        let first = obj.values().next()?;
        return if first.get("baseline_balanced_low_ms").is_some()
            || first.get("weekly_avg_hrv_ms").is_some()
        {
            Some(Kind::HrvDaily)
        } else if first.get("training_status").is_some() {
            Some(Kind::TrainingStatus)
        } else {
            None
        };
    }
    None
}

/// The account's profile id, which Connect's intraday responses carry.
const PROFILE_KEY: &str = "userProfilePK";

/// Remove every member called `key` (at any depth); true when something was removed.
fn strip_key(v: &mut Value, key: &str) -> bool {
    match v {
        Value::Object(m) => {
            let mut hit = m.remove(key).is_some();
            for x in m.values_mut() {
                hit |= strip_key(x, key);
            }
            hit
        }
        Value::Array(a) => a.iter_mut().fold(false, |hit, x| strip_key(x, key) | hit),
        _ => false,
    }
}

/// Remove the account identifiers of a sleep night: the top-level `userProfilePK` and the
/// `id` and `userProfilePK` of `dailySleepDTO`. True when something was removed.
fn strip_sleep_ids(v: &mut Value) -> bool {
    let mut hit = false;
    if let Some(top) = v.as_object_mut() {
        hit |= top.remove(PROFILE_KEY).is_some();
        if let Some(dto) = top.get_mut("dailySleepDTO").and_then(Value::as_object_mut) {
            hit |= dto.remove("id").is_some();
            hit |= dto.remove(PROFILE_KEY).is_some();
        }
    }
    hit
}

/// Every `*.json` below `dir`, in path order, as (path inside `dir`, full path). Symbolic links
/// are not followed. The top-level `records` directory is not walked: it holds pages of
/// activity records, read per activity (see [`record_pages`]).
fn json_files(dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
        let mut entries = std::fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let ft = e.file_type()?;
            if ft.is_dir() {
                if prefix.is_empty() && name == RECORDS_DIR {
                    continue;
                }
                walk(&e.path(), &rel, out)?;
            } else if ft.is_file() && name.to_ascii_lowercase().ends_with(".json") {
                out.push((rel, e.path()));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out)?;
    Ok(out)
}

/// One envelope to store: kind, origin (path, plus `#id` for a list entry), bytes.
struct Blob {
    kind: Kind,
    origin: String,
    bytes: Vec<u8>,
    /// The envelope carries records.
    with_records: bool,
}

/// What `blobs_of` gives for one input file: its blobs and the problems met on the way.
type Blobs = (Vec<Blob>, Vec<String>);

/// The blobs of one input file; `None` when it is not Connect JSON. `records` gives the records
/// member of an activity id (`Ok(None)`: none pulled); when it fails the activity is built
/// without records and the error is returned beside the blob.
fn blobs_of(
    rel: &str,
    raw: &str,
    tz: Tz,
    records: &dyn Fn(i64) -> Result<Option<Value>, String>,
) -> Option<Blobs> {
    let mut value: Value = serde_json::from_str(raw).ok()?;
    if let Some(items) = value.get("activities").and_then(Value::as_array) {
        let blobs = items
            .iter()
            .filter_map(|item| {
                let id = item.get("id")?.as_i64()?;
                let text = serde_json::to_string(item).ok()?;
                Some(Blob {
                    kind: Kind::ActivitySummary,
                    origin: format!("{rel}#{id}"),
                    bytes: wrap(Kind::ActivitySummary, tz, &text),
                    with_records: false,
                })
            })
            .collect();
        return Some((blobs, Vec::new()));
    }
    let kind = classify(&value)?;
    let mut errors = Vec::new();
    let mut recs = None;
    if kind == Kind::Activity {
        if let Some(id) = value.pointer("/summary/id").and_then(Value::as_i64) {
            match records(id) {
                Ok(r) => recs = r,
                Err(e) => errors.push(format!("{rel}: {e}; imported without records")),
            }
        }
    }
    // An intraday file may carry the account's profile id: never stored. A file without it keeps
    // its text byte for byte; one with it is stored as the JSON it is without that member.
    let stripped;
    let stripped_ids = if kind == Kind::SleepDetail {
        strip_sleep_ids(&mut value)
    } else {
        kind.is_intraday() && strip_key(&mut value, PROFILE_KEY)
    };
    let raw = if stripped_ids {
        stripped = serde_json::to_string(&value).ok()?;
        stripped.as_str()
    } else {
        raw
    };
    let bytes = wrap_with_records(kind, tz, raw, recs.as_ref());
    Some((
        vec![Blob {
            kind,
            origin: rel.to_owned(),
            bytes,
            with_records: recs.is_some(),
        }],
        errors,
    ))
}

/// Import every recognised JSON below `dir` for user `owner`. Local dates and times are read in
/// `tz`.
pub async fn run(state: &AppState, dir: &Path, owner: i64, tz: Tz) -> Result<Report> {
    let mut report = Report::default();
    let pages = std::sync::Arc::new(record_pages(dir));
    let mut used: BTreeSet<i64> = BTreeSet::new();
    let files = {
        let dir = dir.to_path_buf();
        tokio::task::spawn_blocking(move || json_files(&dir))
            .await
            .context("directory walk")??
    };
    for (rel, path) in files {
        report.files += 1;
        let read = {
            let path = path.clone();
            tokio::task::spawn_blocking(move || -> Result<Option<String>> {
                if std::fs::metadata(&path)?.len() > MAX_FILE_BYTES {
                    return Ok(None);
                }
                Ok(Some(std::fs::read_to_string(&path)?))
            })
            .await
            .context("read task")?
        };
        let raw = match read {
            Ok(Some(raw)) => raw,
            Ok(None) => {
                report
                    .errors
                    .push(format!("{rel}: larger than the size limit; skipped"));
                continue;
            }
            Err(e) => {
                report.errors.push(format!("{rel}: {e:#}"));
                continue;
            }
        };
        let built = {
            let (rel, pages) = (rel.clone(), pages.clone());
            tokio::task::spawn_blocking(move || {
                let ids = std::cell::RefCell::new(Vec::new());
                let lookup = |id: i64| {
                    ids.borrow_mut().push(id);
                    pages.get(&id).map_or(Ok(None), |p| read_records(p, id))
                };
                let built = blobs_of(&rel, &raw, tz, &lookup);
                (built, ids.into_inner())
            })
            .await
            .context("blob task")?
        };
        let (built, ids) = built;
        used.extend(ids);
        let Some((blobs, problems)) = built else {
            report.unrecognised.push(rel);
            continue;
        };
        report.errors.extend(problems);
        for b in blobs {
            let meta = Meta {
                source: "connect".into(),
                data_type: None,
                sub_type: None,
                file_index: None,
                file_number: None,
                watch_timestamp: None,
            };
            let received = {
                let (root, bytes) = (state.data_dir.as_ref().clone(), b.bytes);
                tokio::task::spawn_blocking(move || store::receive_bytes(&root, &bytes))
                    .await
                    .context("store task")?
            };
            let received = match received {
                Ok(r) => r,
                Err(e) => {
                    report.errors.push(format!("{}: {e:#}", b.origin));
                    continue;
                }
            };
            match ingest::ingest(
                state,
                &received,
                None,
                Some(owner),
                meta,
                Some(b.origin.clone()),
            )
            .await
            {
                Ok(created) => {
                    report.with_records += u64::from(b.with_records);
                    let e = report.by_kind.entry(b.kind.as_str()).or_default();
                    if created {
                        e.0 += 1;
                    } else {
                        e.1 += 1;
                    }
                }
                Err(e) => report
                    .errors
                    .push(format!("{}: ingest failed: {e:#}", b.origin)),
            }
        }
    }
    report.records_without_activity = pages
        .keys()
        .filter(|id| !used.contains(id))
        .copied()
        .collect();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shapes_are_told_apart() {
        let k = |v: Value| classify(&v);
        assert_eq!(k(json!({"nights": []})), Some(Kind::Sleep));
        assert_eq!(
            k(json!({"days": [{"date": "2026-01-01", "steps": 1}]})),
            Some(Kind::Stats)
        );
        assert_eq!(
            k(json!({"trend": [{"date": "2026-01-01", "atl": 1, "ctl": 2}]})),
            Some(Kind::TrainingLoad)
        );
        assert_eq!(
            k(json!({"trend": [{"date": "2026-01-01", "weekly_avg_hrv_ms": 40, "status": "LOW"}]})),
            Some(Kind::HrvTrend)
        );
        assert_eq!(
            k(json!({"trend": [{"date": "2026-01-01", "vo2_max": 50.0}]})),
            Some(Kind::Vo2max)
        );
        assert_eq!(
            k(json!({"2026-01-01": {"date": "2026-01-01", "baseline_balanced_low_ms": 30}})),
            Some(Kind::HrvDaily)
        );
        assert_eq!(
            k(json!({"2026-01-01": {"date": "2026-01-01", "training_status": 4}})),
            Some(Kind::TrainingStatus)
        );
        assert_eq!(k(json!({"summary": {}, "fit": {}})), Some(Kind::Activity));
        assert_eq!(
            k(json!([{"date": "2026-01-01", "steps": 1, "stress_avg": 20}])),
            Some(Kind::DailyStats)
        );
        assert_eq!(k(json!([{"date": "2026-01-01", "steps": 1}])), None);
        assert_eq!(k(json!([])), None);
        assert_eq!(k(json!({"something": "else"})), None);
        assert_eq!(k(json!([1, 2])), None);
    }

    #[test]
    fn a_day_of_intraday_series_is_recognised_by_its_descriptors_and_values() {
        let k = |v: Value| classify(&v);
        let desc = json!([{"key": "timestamp", "index": 0}]);
        assert_eq!(
            k(json!({"stressValueDescriptorsDTOList": desc, "stressValuesArray": []})),
            Some(Kind::IntradayStress)
        );
        assert_eq!(
            k(json!({"heartRateValueDescriptors": desc, "heartRateValues": []})),
            Some(Kind::IntradayHeartRate)
        );
        assert_eq!(
            k(json!({"respirationValueDescriptorsDTOList": desc, "respirationValuesArray": []})),
            Some(Kind::IntradayRespiration)
        );
        // Values without their descriptors, or the descriptors of another series, are not it.
        assert_eq!(k(json!({"stressValuesArray": []})), None);
        assert_eq!(
            k(json!({"heartRateValueDescriptors": desc, "respirationValuesArray": []})),
            None
        );
        assert_eq!(
            k(json!({"heartRateValueDescriptors": desc, "heartRateValues": null})),
            None
        );
    }

    #[test]
    fn the_profile_id_is_dropped_from_an_intraday_envelope_only() {
        let tz: Tz = "Europe/Copenhagen".parse().unwrap();
        let raw = r#"{"userProfilePK": 424242, "calendarDate": "2026-11-12",
            "heartRateValueDescriptors": [{"key": "timestamp", "index": 0}, {"key": "heartrate", "index": 1}],
            "heartRateValues": [[1000, 60]]}"#;
        let (blobs, _) = blobs_of("hr.json", raw, tz, &|_| Ok(None)).unwrap();
        let text = String::from_utf8(blobs[0].bytes.clone()).unwrap();
        assert!(
            !text.contains("userProfilePK") && !text.contains("424242"),
            "{text}"
        );
        assert!(text.contains("calendarDate") && text.contains("heartRateValues"));
        // Without it the original text is kept byte for byte.
        let plain = raw.replace(r#""userProfilePK": 424242, "#, "");
        let (blobs, _) = blobs_of("hr.json", &plain, tz, &|_| Ok(None)).unwrap();
        assert!(String::from_utf8(blobs[0].bytes.clone())
            .unwrap()
            .contains(&plain));
        // Another kind is stored as it came.
        let sleep = r#"{"userProfilePK": 424242, "nights": []}"#;
        let (blobs, _) = blobs_of("s.json", sleep, tz, &|_| Ok(None)).unwrap();
        assert!(String::from_utf8(blobs[0].bytes.clone())
            .unwrap()
            .contains("424242"));
        let mut v = json!({"a": [{"userProfilePK": 1, "b": 2}], "userProfilePK": 3});
        assert!(strip_key(&mut v, "userProfilePK"));
        assert_eq!(v, json!({"a": [{"b": 2}]}));
        assert!(!strip_key(&mut v, "userProfilePK"));
    }

    #[test]
    fn a_night_of_sleep_detail_is_recognised_and_loses_its_account_ids() {
        let tz: Tz = "Europe/Copenhagen".parse().unwrap();
        let raw = r#"{"userProfilePK": 424242, "dailySleepDTO": {"id": 777, "userProfilePK": 424242,
            "calendarDate": "2026-11-28", "deepSleepSeconds": 60}, "sleepLevels": [], "other": {"id": 5}}"#;
        assert_eq!(
            classify(&serde_json::from_str(raw).unwrap()),
            Some(Kind::SleepDetail)
        );
        let (blobs, _) = blobs_of("n.json", raw, tz, &|_| Ok(None)).unwrap();
        assert_eq!(blobs[0].kind, Kind::SleepDetail);
        let text = String::from_utf8(blobs[0].bytes.clone()).unwrap();
        assert!(!text.contains("userProfilePK") && !text.contains("424242"));
        assert!(!text.contains("777"), "{text}");
        assert!(text.contains("calendarDate") && text.contains(r#""other":{"id":5}"#));
        // Without the identifiers the text is kept byte for byte.
        let plain = r#"{"dailySleepDTO": {"calendarDate": "2026-11-28"},  "sleepLevels": []}"#;
        let (blobs, _) = blobs_of("n.json", plain, tz, &|_| Ok(None)).unwrap();
        assert!(String::from_utf8(blobs[0].bytes.clone())
            .unwrap()
            .contains(plain));
        // Not a night: the object is missing, or the levels are not a list.
        for other in [
            json!({"dailySleepDTO": 1, "sleepLevels": []}),
            json!({"dailySleepDTO": {}, "sleepLevels": {}}),
            json!({"sleepLevels": []}),
        ] {
            assert_eq!(classify(&other), None);
        }
    }

    #[test]
    fn an_activity_list_becomes_one_blob_per_activity() {
        let tz: Tz = "Europe/Copenhagen".parse().unwrap();
        let raw = r#"{"count": 2, "activities": [{"id": 7, "type": "running"}, {"id": 8}, {"no_id": 1}]}"#;
        let (blobs, problems) = blobs_of("activities_x.json", raw, tz, &|_| Ok(None)).unwrap();
        assert!(problems.is_empty());
        assert_eq!(blobs.len(), 2);
        assert_eq!(blobs[0].origin, "activities_x.json#7");
        assert!(blobs.iter().all(|b| b.kind == Kind::ActivitySummary));
    }
}
