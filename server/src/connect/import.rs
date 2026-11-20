//! `import-connect`: store a directory of Garmin Connect JSON as blobs (one envelope per input
//! file, one per activity of an activity list) through the one ingest path, owned by one user.
//!
//! Re-running is idempotent: identical input gives identical envelopes, so every blob is already
//! stored and ledgered (the origin, the path inside the directory, folds repeats into the
//! existing event), and nothing is derived twice. Files that are not recognised Connect JSON
//! are listed and not stored.

use super::{wrap, Kind};
use crate::api::AppState;
use crate::ledger::Meta;
use crate::{ingest, store};
use anyhow::{Context, Result};
use chrono_tz::Tz;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Largest input file read.
const MAX_FILE_BYTES: u64 = 256 << 20;

#[derive(Debug, Default)]
pub struct Report {
    /// Input files looked at (`*.json`).
    pub files: u64,
    /// Blobs stored for the first time and blobs that were already there, by kind.
    pub by_kind: BTreeMap<&'static str, (u64, u64)>,
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

/// Decide what a parsed Connect JSON is, from its shape (never from its file name).
pub fn classify(v: &Value) -> Option<Kind> {
    let obj = v.as_object()?;
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

/// Every `*.json` below `dir`, in path order, as (path inside `dir`, full path). Symbolic links
/// are not followed.
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
}

/// The blobs of one input file; `None` when it is not Connect JSON.
fn blobs_of(rel: &str, raw: &str, tz: Tz) -> Option<Vec<Blob>> {
    let value: Value = serde_json::from_str(raw).ok()?;
    if let Some(items) = value.get("activities").and_then(Value::as_array) {
        return Some(
            items
                .iter()
                .filter_map(|item| {
                    let id = item.get("id")?.as_i64()?;
                    let text = serde_json::to_string(item).ok()?;
                    Some(Blob {
                        kind: Kind::ActivitySummary,
                        origin: format!("{rel}#{id}"),
                        bytes: wrap(Kind::ActivitySummary, tz, &text),
                    })
                })
                .collect(),
        );
    }
    let kind = classify(&value)?;
    Some(vec![Blob {
        kind,
        origin: rel.to_owned(),
        bytes: wrap(kind, tz, raw),
    }])
}

/// Import every recognised JSON below `dir` for user `owner`. Local dates and times are read in
/// `tz`.
pub async fn run(state: &AppState, dir: &Path, owner: i64, tz: Tz) -> Result<Report> {
    let mut report = Report::default();
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
        let Some(blobs) = blobs_of(&rel, &raw, tz) else {
            report.unrecognised.push(rel);
            continue;
        };
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
        assert_eq!(k(json!({"something": "else"})), None);
        assert_eq!(k(json!([1, 2])), None);
    }

    #[test]
    fn an_activity_list_becomes_one_blob_per_activity() {
        let tz: Tz = "Europe/Copenhagen".parse().unwrap();
        let raw = r#"{"count": 2, "activities": [{"id": 7, "type": "running"}, {"id": 8}, {"no_id": 1}]}"#;
        let blobs = blobs_of("activities_x.json", raw, tz).unwrap();
        assert_eq!(blobs.len(), 2);
        assert_eq!(blobs[0].origin, "activities_x.json#7");
        assert!(blobs.iter().all(|b| b.kind == Kind::ActivitySummary));
    }
}
