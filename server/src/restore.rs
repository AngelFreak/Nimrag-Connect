//! Restore of a Nimrag export (`docs/design/export-restore.md`): the zip [`crate::export`] writes
//! goes back into the signed-in user.
//!
//! 1. The manifest is read (`nimrag/manifest.json`, `format` and `version`), then the JSON parts.
//! 2. Every original named in `nimrag/files.json` is read from `originals/`, checked against its
//!    SHA-256 and stored through [`crate::ingest::ingest_with`] with its original ledger `Meta`.
//!    Ingest is idempotent, so a file the server has counts as "already here".
//! 3. The person's own entries (settings, workouts, body measurements, strength set edits) are
//!    merged in one transaction after the originals.
//!
//! Only adds and updates: nothing the user has is deleted, and a second restore of the same zip
//! changes nothing. A restore never queues files for Garmin Connect and never marks a workout as
//! sent to the watch.
//!
//! The zip is read on a blocking thread, one entry at a time (a file never goes above
//! `Limits::max_fit_bytes` in memory), and handed to the async side over a small channel.

use crate::api::AppState;
use crate::export::{
    BodyJson, FileJson, Manifest, StrengthEditJson, WorkoutJson, FORMAT, FORMAT_VERSION,
    MANIFEST_NAME,
};
use crate::import::Limits;
use crate::ledger::Meta;
use crate::{ingest, settings, store};
use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use tokio::sync::mpsc;
use ts_rs::TS;

pub const NOT_NIMRAG: &str = "This is not a Nimrag export";
pub const NEWER: &str = "This export was made by a newer Nimrag; update this server first";
pub const OLDER: &str = "This export was made by an older Nimrag: download a new export";
/// What the Garmin import answers when it is given a Nimrag export.
pub const USE_RESTORE: &str = "This is a Nimrag export: use Restore from a Nimrag export";

/// Longest JSON part read (the manifest and the small parts are far below it).
const MAX_JSON_BYTES: u64 = 64 << 20;
/// Longest `activity_ident`, set name or setting text kept.
const MAX_TEXT: usize = 200;
/// Error texts in a response.
pub const MAX_ERRORS: usize = 20;

// ---- the answer ---------------------------------------------------------------------------------

/// Original files of a restore.
#[derive(Debug, Default, Serialize, Deserialize, TS)]
pub struct NimragImportFiles {
    /// Stored for the first time.
    pub added: i64,
    /// The server already had the same bytes.
    pub already_here: i64,
    /// Listed in the export, but the export had no copy (the server it came from had lost it).
    pub missing_in_export: i64,
    /// Not stored: the bytes do not match their hash, the name is not allowed, or the file could
    /// not be stored.
    pub refused: i64,
}

/// Workouts or body measurements of a restore.
#[derive(Debug, Default, Serialize, Deserialize, TS)]
pub struct NimragImportAdded {
    pub added: i64,
    /// The person already had the same entry.
    pub already_here: i64,
}

/// Strength set edits of a restore.
#[derive(Debug, Default, Serialize, Deserialize, TS)]
pub struct NimragImportEdits {
    /// Written: new, or newer than the edit the person had.
    pub applied: i64,
    /// Not written: the person's own edit is as new or newer.
    pub older: i64,
}

/// What a restore did.
#[derive(Debug, Default, Serialize, Deserialize, TS)]
pub struct NimragImportResponse {
    /// The file name the restore was made from.
    pub name: String,
    pub files: NimragImportFiles,
    /// Settings written (they replace the current values).
    pub settings_applied: i64,
    /// Settings left out: not one of the person's settings, or a value that is not allowed.
    pub settings_skipped: i64,
    pub workouts: NimragImportAdded,
    pub body_measurements: NimragImportAdded,
    pub strength_set_edits: NimragImportEdits,
    /// Up to 20 short texts for what was refused or left out.
    pub errors: Vec<String>,
}

impl NimragImportResponse {
    fn error(&mut self, text: String) {
        if self.errors.len() < MAX_ERRORS {
            self.errors.push(text);
        }
    }
}

// ---- reading the archive -------------------------------------------------------------------------

/// Why a restore did not start.
#[derive(Debug)]
pub enum RestoreError {
    /// The upload is not something this server can restore (answered 422).
    Unprocessable(String),
    Other(anyhow::Error),
}

impl From<anyhow::Error> for RestoreError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other(e)
    }
}

impl From<tokio::task::JoinError> for RestoreError {
    fn from(e: tokio::task::JoinError) -> Self {
        Self::Other(anyhow::anyhow!("restore task: {e}"))
    }
}

fn refuse<T>(text: impl Into<String>) -> Result<T, RestoreError> {
    Err(RestoreError::Unprocessable(text.into()))
}

/// True when the zip at `path` has a Nimrag manifest at its top (the Garmin import refuses those).
pub fn is_nimrag_export(path: &Path) -> bool {
    File::open(path)
        .ok()
        .and_then(|f| zip::ZipArchive::new(f).ok())
        .is_some_and(|mut z| z.by_name(MANIFEST_NAME).is_ok())
}

fn read_capped<R: Read>(r: R, cap: u64) -> std::io::Result<Option<Vec<u8>>> {
    let mut bytes = vec![];
    r.take(cap + 1).read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= cap).then_some(bytes))
}

/// Everything in the JSON part, read before anything is stored.
#[derive(Debug)]
pub struct Parts {
    pub manifest: Manifest,
    pub files: Vec<FileJson>,
    pub settings: BTreeMap<String, String>,
    pub workouts: Vec<WorkoutJson>,
    pub body: Vec<BodyJson>,
    pub edits: Vec<StrengthEditJson>,
}

fn part<T: DeserializeOwned>(
    zip: &mut zip::ZipArchive<File>,
    name: &str,
) -> Result<T, RestoreError> {
    let damaged = || RestoreError::Unprocessable(format!("This export is damaged: {name}"));
    let entry = zip.by_name(name).map_err(|_| damaged())?;
    let bytes = read_capped(entry, MAX_JSON_BYTES)
        .map_err(|e| {
            RestoreError::Other(anyhow::Error::from(e).context(format!("reading {name}")))
        })?
        .ok_or_else(damaged)?;
    serde_json::from_slice(&bytes).map_err(|_| damaged())
}

/// Open the zip and read the manifest and the JSON parts. Blocking.
pub fn read_parts(path: &Path) -> Result<Parts, RestoreError> {
    let file = File::open(path).context("opening the upload")?;
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return refuse(NOT_NIMRAG);
    };
    let manifest_bytes = match zip.by_name(MANIFEST_NAME) {
        Ok(entry) => read_capped(entry, 1 << 20).context("reading the manifest")?,
        Err(_) => None,
    };
    let Some(manifest_bytes) = manifest_bytes else {
        // An export from before the exact part existed: same look, no `nimrag/` folder.
        let old = zip.by_name("csv/files.csv").is_ok() && zip.by_name("README.txt").is_ok();
        return refuse(if old { OLDER } else { NOT_NIMRAG });
    };
    let Ok(manifest) = serde_json::from_slice::<Manifest>(&manifest_bytes) else {
        return refuse(NOT_NIMRAG);
    };
    if manifest.format != FORMAT || manifest.version == 0 {
        return refuse(NOT_NIMRAG);
    }
    if manifest.version > FORMAT_VERSION {
        return refuse(NEWER);
    }
    Ok(Parts {
        files: part(&mut zip, "nimrag/files.json")?,
        settings: part(&mut zip, "nimrag/settings.json")?,
        workouts: part(&mut zip, "nimrag/workouts.json")?,
        body: part(&mut zip, "nimrag/body_measurements.json")?,
        edits: part(&mut zip, "nimrag/strength_set_edits.json")?,
        manifest,
    })
}

/// `originals/<name>` with nothing that leaves the folder: no `..`, no empty parts, no absolute
/// path, no backslashes.
fn enclosed_original(name: &str) -> bool {
    if name.contains(['\\', '\0']) {
        return false;
    }
    let mut parts = name.split('/');
    if parts.next() != Some("originals") {
        return false;
    }
    let rest: Vec<&str> = parts.collect();
    !rest.is_empty()
        && rest
            .iter()
            .all(|p| !p.is_empty() && *p != "." && *p != "..")
}

/// A source word as the ledger keeps them (`watch`, `usb`, `import`, `connect`, ...).
fn valid_source(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn meta_of(f: &FileJson) -> Meta {
    Meta {
        source: f.source.clone(),
        data_type: f.data_type,
        sub_type: f.sub_type,
        file_index: f.file_index,
        file_number: f.file_number,
        watch_timestamp: f.watch_timestamp,
    }
}

/// What the reading thread hands over for one `files.json` entry.
enum Item {
    /// The export has no copy of this one.
    Missing,
    /// Not usable; the text says why (with the entry's name).
    Refused(String),
    /// Read, hashed and written to incoming: ready for ingest.
    Blob(store::Received),
}

/// Read every original named in `files` and send it on, in order, one entry at a time. Blocking.
fn read_originals(
    root: &Path,
    zip_path: &Path,
    files: &[FileJson],
    limits: &Limits,
    tx: &mpsc::Sender<(usize, Item)>,
) -> Result<()> {
    let mut zip = zip::ZipArchive::new(File::open(zip_path)?)?;
    let mut used = 0u64;
    for (i, f) in files.iter().enumerate() {
        let item = match f.name.as_deref() {
            None => Item::Missing,
            Some(name) => match read_one(root, &mut zip, name, &f.sha256, limits, &mut used) {
                Ok(item) => item,
                Err(e) => Item::Refused(format!("{name}: {e:#}")),
            },
        };
        let blob = match tx.blocking_send((i, item)) {
            Ok(()) => continue,
            Err(mpsc::error::SendError((_, Item::Blob(b)))) => Some(b),
            Err(_) => None,
        };
        if let Some(b) = blob {
            let _ = std::fs::remove_file(&b.tmp);
        }
        break; // the receiving side is gone
    }
    Ok(())
}

fn read_one(
    root: &Path,
    zip: &mut zip::ZipArchive<File>,
    name: &str,
    sha256: &str,
    limits: &Limits,
    used: &mut u64,
) -> Result<Item> {
    if !enclosed_original(name) {
        return Ok(Item::Refused(format!("{name}: this name is not allowed")));
    }
    if !crate::is_sha256_hex(sha256) {
        return Ok(Item::Refused(format!("{name}: no valid SHA-256 is listed")));
    }
    let Ok(entry) = zip.by_name(name) else {
        return Ok(Item::Refused(format!("{name}: not in the zip")));
    };
    let cap = limits.max_fit_bytes;
    if entry.size() > cap {
        return Ok(Item::Refused(format!("{name}: over the {cap} byte limit")));
    }
    let Some(bytes) = read_capped(entry, cap)? else {
        return Ok(Item::Refused(format!("{name}: over the {cap} byte limit")));
    };
    *used = used.saturating_add(bytes.len() as u64);
    if *used > limits.max_total_bytes {
        anyhow::bail!("total byte limit exceeded");
    }
    let blob = store::receive_bytes(root, &bytes)?;
    if blob.sha256 != sha256 {
        let _ = std::fs::remove_file(&blob.tmp);
        return Ok(Item::Refused(format!(
            "{name}: the content does not match its SHA-256"
        )));
    }
    Ok(Item::Blob(blob))
}

// ---- the restore --------------------------------------------------------------------------------

/// Restore the zip at `zip_path` into `user`. `name` only labels the answer.
pub async fn run(
    state: &AppState,
    zip_path: &Path,
    user: i64,
    name: String,
    limits: Limits,
) -> Result<NimragImportResponse, RestoreError> {
    let path = zip_path.to_path_buf();
    let parts = tokio::task::spawn_blocking(move || read_parts(&path)).await??;
    let mut out = NimragImportResponse {
        name,
        ..Default::default()
    };

    let files = std::sync::Arc::new(parts.files);
    let (tx, mut rx) = mpsc::channel::<(usize, Item)>(4);
    let reader = {
        let (root, path, files, limits) = (
            state.data_dir.as_ref().clone(),
            zip_path.to_path_buf(),
            files.clone(),
            limits,
        );
        tokio::task::spawn_blocking(move || read_originals(&root, &path, &files, &limits, &tx))
    };
    while let Some((i, item)) = rx.recv().await {
        let f = &files[i];
        match item {
            Item::Missing => out.files.missing_in_export += 1,
            Item::Refused(text) => {
                out.files.refused += 1;
                out.error(text);
            }
            Item::Blob(blob) => {
                if !valid_source(&f.source) {
                    let _ = std::fs::remove_file(&blob.tmp);
                    out.files.refused += 1;
                    out.error(format!("{}: unknown source", f.sha256));
                    continue;
                }
                match ingest::ingest_with(state, &blob, None, Some(user), meta_of(f), None, false)
                    .await
                {
                    Ok(true) => out.files.added += 1,
                    Ok(false) => out.files.already_here += 1,
                    Err(e) => {
                        tracing::error!(error = format!("{e:#}"), "restore: ingest failed");
                        out.files.refused += 1;
                        out.error(format!("{}: could not be stored", f.sha256));
                    }
                }
            }
        }
    }
    reader.await.map_err(RestoreError::from)??;

    merge_entries(
        state,
        user,
        &parts.settings,
        &parts.workouts,
        &parts.body,
        &parts.edits,
        &mut out,
    )
    .await?;
    Ok(out)
}

/// The person's own entries, in one transaction: all of them or none.
async fn merge_entries(
    state: &AppState,
    uid: i64,
    settings_in: &BTreeMap<String, String>,
    workouts: &[WorkoutJson],
    body: &[BodyJson],
    edits: &[StrengthEditJson],
    out: &mut NimragImportResponse,
) -> Result<()> {
    // Reads, then writes: take the write lock up front, or a concurrent writer makes SQLite
    // answer BUSY at the first write (see `outbox::take_dropped`).
    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;

    for (key, value) in settings_in {
        match settings::checked_user_setting(key, value) {
            Some(stored) => {
                sqlx::query(
                    "INSERT INTO user_settings (user_id, key, value) VALUES (?, ?, ?)
                     ON CONFLICT (user_id, key) DO UPDATE SET value = excluded.value",
                )
                .bind(uid)
                .bind(key)
                .bind(stored)
                .execute(&mut *tx)
                .await?;
                out.settings_applied += 1;
            }
            None => {
                out.settings_skipped += 1;
                out.error(format!(
                    "setting {}: not restored",
                    key.chars().take(MAX_TEXT).collect::<String>()
                ));
            }
        }
    }

    let mut have: Vec<(String, String, serde_json::Value)> =
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT name, sport, definition FROM workouts WHERE user_id = ?",
        )
        .bind(uid)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|(n, s, d)| {
            let d = serde_json::from_str(&d).unwrap_or(serde_json::Value::String(d));
            (n, s, d)
        })
        .collect();
    for w in workouts {
        let name = w.name.trim();
        if name.is_empty() || w.sport.is_empty() {
            out.error("a workout without a name or sport was left out".into());
            continue;
        }
        if have
            .iter()
            .any(|(n, s, d)| n == name && *s == w.sport && *d == w.definition)
        {
            out.workouts.already_here += 1;
            continue;
        }
        if have.len() as i64 >= crate::web_api::workouts::MAX_WORKOUTS {
            out.error(format!(
                "workout {}: left out, a user keeps at most {} workouts",
                name.chars().take(MAX_TEXT).collect::<String>(),
                crate::web_api::workouts::MAX_WORKOUTS
            ));
            continue;
        }
        // A restore never sends anything to the watch: the workout starts as idle, revision 1.
        sqlx::query(
            "INSERT INTO workouts (user_id, name, sport, definition, revision, created_at,
                updated_at, send_state) VALUES (?, ?, ?, ?, 1, ?, ?, 'idle')",
        )
        .bind(uid)
        .bind(name)
        .bind(&w.sport)
        .bind(w.definition.to_string())
        .bind(w.created_at)
        .bind(w.updated_at)
        .execute(&mut *tx)
        .await?;
        have.push((name.to_owned(), w.sport.clone(), w.definition.clone()));
        out.workouts.added += 1;
    }

    for b in body {
        let same: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM body_measurements WHERE user_id = ? AND measured_at = ?
               AND weight_kg IS ? AND body_fat_pct IS ? AND waist_cm IS ?",
        )
        .bind(uid)
        .bind(b.measured_at)
        .bind(b.weight_kg)
        .bind(b.body_fat_pct)
        .bind(b.waist_cm)
        .fetch_optional(&mut *tx)
        .await?;
        if same.is_some() {
            out.body_measurements.already_here += 1;
            continue;
        }
        sqlx::query(
            "INSERT INTO body_measurements (user_id, measured_at, weight_kg, body_fat_pct, waist_cm,
                created_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(uid)
        .bind(b.measured_at)
        .bind(b.weight_kg)
        .bind(b.body_fat_pct)
        .bind(b.waist_cm)
        .bind(b.created_at)
        .execute(&mut *tx)
        .await?;
        out.body_measurements.added += 1;
    }

    for e in edits {
        let usable = !e.activity_ident.is_empty()
            && e.activity_ident.chars().count() <= MAX_TEXT
            && e.set_idx >= 0
            && e.fields.as_object().is_some_and(|m| !m.is_empty());
        if !usable {
            out.error("a strength set edit that cannot be read was left out".into());
            continue;
        }
        // Later wins: the archive's edit replaces the person's only when it is newer.
        let n = sqlx::query(
            "INSERT INTO strength_set_overrides (user_id, activity_ident, set_idx, fields, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (user_id, activity_ident, set_idx) DO UPDATE
               SET fields = excluded.fields, updated_at = excluded.updated_at
               WHERE excluded.updated_at > strength_set_overrides.updated_at",
        )
        .bind(uid)
        .bind(&e.activity_ident)
        .bind(e.set_idx)
        .bind(e.fields.to_string())
        .bind(e.updated_at)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if n > 0 {
            out.strength_set_edits.applied += 1;
        } else {
            out.strength_set_edits.older += 1;
        }
    }

    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_names_inside_originals_are_enclosed() {
        assert!(enclosed_original("originals/abc.fit"));
        assert!(enclosed_original("originals/sub/abc.fit"));
        for bad in [
            "originals/../x",
            "../originals/x",
            "/originals/x",
            "originals/",
            "originals",
            "originals//x",
            "originals/./x",
            "originals\\x",
            "csv/files.csv",
            "originals/a\0b",
        ] {
            assert!(!enclosed_original(bad), "{bad}");
        }
    }

    #[test]
    fn sources_are_plain_words() {
        assert!(valid_source("watch"));
        assert!(valid_source("garmin-connect_2"));
        assert!(!valid_source(""));
        assert!(!valid_source("a b"));
        assert!(!valid_source(&"x".repeat(33)));
    }
}
