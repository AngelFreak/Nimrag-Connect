//! `POST /api/import/nimrag?name=<file name>`: restore a Nimrag export (the zip of "Download my
//! data") into the signed-in user ("Restore from a Nimrag export" in the web app). The work is in
//! [`crate::restore`]; this is the upload around it, the same as [`super::garmin_import`]:
//!
//! The body is the raw zip, streamed to the store's incoming directory (hashed as it goes) and
//! read from disk entry by entry. Session cookie of a user with data only (an admin gets 403, a
//! device token never passes) and the CSRF header. The size limit is `AppState::max_import` (413
//! above it) and the gate is `AppState::import_gate` (429 when other imports run). The work is a
//! detached task, so a client that gives up midway leaves a finished restore, not a broken one.
//! A zip that is not a Nimrag export (or is too new or too old) answers 422 with the reason.

use super::garmin_import::{clean_error, clean_name, too_large, ImportParams, Upload};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::import::Limits;
use crate::restore::{self, NimragImportResponse, RestoreError};
use crate::store;
use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

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

    let limits = Limits {
        max_total_bytes: (state.max_import as u64).saturating_mul(4),
        ..Limits::default()
    };
    let task_state = state.clone();
    let uid = user.id;
    let label = name.clone();
    let (mut out, size) = tokio::spawn(async move {
        let _permit = permit;
        let received = store::receive(&task_state.data_dir, body, task_state.max_import)
            .await
            .map_err(|e| match e {
                store::ReceiveError::TooLarge => too_large(),
                store::ReceiveError::Other(e) => ApiError::from(e),
            })?;
        let size = received.size;
        let upload = Upload(received.tmp);
        let out = restore::run(&task_state, &upload.0, uid, label, limits)
            .await
            .map_err(|e| match e {
                RestoreError::Unprocessable(text) => {
                    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, text)
                }
                RestoreError::Other(e) => ApiError::from(e),
            })?;
        Ok::<_, ApiError>((out, size))
    })
    .await
    .map_err(|e| ApiError::from(anyhow::anyhow!("restore task: {e}")))??;

    let data_dir = state.data_dir.display().to_string();
    out.errors = out
        .errors
        .iter()
        .map(|e| clean_error(e, &data_dir))
        .collect();
    tracing::info!(
        user_id = user.id,
        size,
        added = out.files.added,
        already_here = out.files.already_here,
        refused = out.files.refused,
        "nimrag restore"
    );
    let out: NimragImportResponse = out;
    Ok(Json(out).into_response())
}
