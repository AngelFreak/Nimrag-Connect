//! `POST /api/import/garmin?name=<file name>`: the signed-in user's own Garmin export, uploaded
//! from the web app ("Import from Garmin").
//!
//! The body is the raw file: one activity's original from Garmin Connect (a zip holding a `.fit`),
//! Garmin's whole account export (one big zip with FIT originals in nested zips), or a plain
//! `.fit`. It is streamed to the store's incoming directory (hashed as it goes, never held in
//! memory) and then read from disk entry by entry by the same walker `import-export` uses
//! ([`crate::import`]): nested zips at any depth, FIT told by its header bytes, everything else
//! skipped. Every FIT goes through [`crate::ingest::ingest`] with source `import` and the signed-in
//! user as owner, so a re-import changes nothing and the watch's own rank rules apply.
//!
//! Session cookie of a user with data only (an admin gets 403, a device token never passes) and the
//! CSRF header, like every state-changing call under `/api`. The size limit is
//! `AppState::max_import` (413 above it). The request is synchronous: it answers when the last
//! file is in, which for a multi-gigabyte export is minutes. The zip work runs on a blocking thread
//! and ingest takes the ledger lock one file at a time, so other requests are not held up, and the
//! work is a detached task: a client that gives up midway leaves a finished import, not a broken
//! one. The reverse proxy must not cut a long response wait (Caddy has no such timeout by default).

use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::import::{self, Limits, Options};
use crate::store;
use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

/// Imports at once, server-wide. Each holds a thread and a file on disk while it runs.
pub const MAX_IMPORTS: usize = 2;
/// Error texts in a response.
const MAX_ERRORS: usize = 20;
/// Longest error text, in characters.
const MAX_ERROR_CHARS: usize = 200;
/// Longest file name kept (it becomes part of the upload origin).
const MAX_NAME_CHARS: usize = 200;

#[derive(Debug, Deserialize)]
pub struct ImportParams {
    /// The name the file had on the person's device.
    name: Option<String>,
}

/// FIT files of one kind in an import.
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct GarminImportKind {
    /// `activity`, `monitoring`, `sleep`, `metrics` or `other`.
    pub kind: String,
    pub count: i64,
}

/// What one uploaded file held.
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct GarminImportResponse {
    /// The file name the import was recorded under.
    pub name: String,
    /// FIT files read and stored or already stored: `new` plus `already_present`.
    pub fit_found: i64,
    /// Stored for the first time.
    pub new: i64,
    /// Already on the server (the same bytes).
    pub already_present: i64,
    /// Entries that were not FIT (JSON, GPX, images, ...): looked at by their first bytes, not stored.
    pub skipped: i64,
    /// Entries that could not be read or stored, and archives that could not be opened.
    pub failed: i64,
    /// Up to 20 short texts for the failures, with the entry's path in the archive.
    pub errors: Vec<String>,
    /// The FIT files by kind, the most numerous first.
    pub kinds: Vec<GarminImportKind>,
}

/// Removes the uploaded file when dropped, on every path out of the import.
struct Upload(PathBuf);
impl Drop for Upload {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The file name as the person's device gave it, made safe to keep: no directory part, no control
/// characters, a bounded length. `None` when nothing is left.
fn clean_name(raw: &str) -> Option<String> {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or_default();
    let name: String = last
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect();
    let name = name.trim().to_string();
    (!name.is_empty() && name != "." && name != "..").then_some(name)
}

/// A failure text for the browser: one line, bounded, without the server's directories.
fn clean_error(text: &str, data_dir: &str) -> String {
    let text = text.replace(data_dir, "(server)");
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_ERROR_CHARS)
        .collect()
}

fn too_large() -> ApiError {
    ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "file too large")
}

pub async fn import(
    user: DataUser,
    State(state): State<AppState>,
    Query(params): Query<ImportParams>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let name = params
        .name
        .as_deref()
        .and_then(clean_name)
        .ok_or_else(|| ApiError::bad_request("name is required (the file's name)"))?;
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|n| n > state.max_import as u64) {
        return Err(too_large());
    }
    let Ok(permit) = state.import_gate.clone().try_acquire_owned() else {
        let mut r = ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "other imports are running; try again in a minute",
        )
        .into_response();
        r.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        return Ok(r);
    };

    // Detached, body included: the import finishes (and the temp files go) even if the client gives
    // up, and a body that breaks off is cleaned up by `receive`, not left behind by a dropped future.
    let limits = Limits {
        // What a real export needs is a small multiple of its own size; more is a bomb.
        max_total_bytes: (state.max_import as u64).saturating_mul(4),
        ..Limits::default()
    };
    let opts = Options {
        limits,
        owner: Some(user.id),
        prefix: name.clone(),
        keep_paths: false,
        max_errors: MAX_ERRORS,
        kinds: true,
        plain_errors: true,
    };
    let task_state = state.clone();
    let (report, size) = tokio::spawn(async move {
        let _permit = permit;
        let received = store::receive(&task_state.data_dir, body, task_state.max_import)
            .await
            .map_err(|e| match e {
                store::ReceiveError::TooLarge => too_large(),
                store::ReceiveError::Other(e) => ApiError::from(e),
            })?;
        let size = received.size;
        let upload = Upload(received.tmp);
        let report = import::run_with(&task_state, &upload.0, opts).await?;
        Ok::<_, ApiError>((report, size))
    })
    .await
    .map_err(|e| ApiError::from(anyhow::anyhow!("import task: {e}")))??;

    let data_dir = state.data_dir.display().to_string();
    let mut kinds: Vec<(&str, u64)> = report.kinds.iter().map(|(k, n)| (*k, *n)).collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let n = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
    let out = GarminImportResponse {
        name,
        fit_found: n(report.fit_new + report.fit_present),
        new: n(report.fit_new),
        already_present: n(report.fit_present),
        skipped: n(report.skipped),
        failed: n(report.failed),
        errors: report
            .errors
            .iter()
            .map(|e| clean_error(e, &data_dir))
            .collect(),
        kinds: kinds
            .into_iter()
            .map(|(k, c)| GarminImportKind {
                kind: k.to_string(),
                count: n(c),
            })
            .collect(),
    };
    tracing::info!(
        user_id = user.id,
        size,
        fit = out.fit_found,
        new = out.new,
        skipped = out.skipped,
        failed = out.failed,
        "garmin import"
    );
    Ok(Json(out).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_lose_directories_and_control_characters() {
        assert_eq!(clean_name("C:\\Users\\me\\a.zip").as_deref(), Some("a.zip"));
        assert_eq!(clean_name("../../x.fit").as_deref(), Some("x.fit"));
        assert_eq!(clean_name("a\nb.zip").as_deref(), Some("ab.zip"));
        assert_eq!(clean_name("  ").as_deref(), None);
        assert_eq!(clean_name("dir/").as_deref(), None);
        assert_eq!(clean_name("..").as_deref(), None);
        assert_eq!(clean_name(&"x".repeat(500)).unwrap().chars().count(), 200);
    }

    #[test]
    fn error_texts_are_one_bounded_line_without_the_data_dir() {
        let t = clean_error("a\nb /srv/data/files/x failed", "/srv/data");
        assert_eq!(t, "a b (server)/files/x failed");
        assert_eq!(clean_error(&"y".repeat(500), "/srv").chars().count(), 200);
    }
}
