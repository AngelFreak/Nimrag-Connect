//! `GET /api/export`: the signed-in user's data as a zip (see `crate::export`).
//!
//! Session cookie of a user with data only: a device token never passes and an admin gets 403,
//! like every data endpoint. A `GET` needs no CSRF header (the archive is a download link), and
//! `Cache-Control: no-store` comes from the API headers. Two exports run at a time; a third is
//! told to try again shortly.

use super::Ctx;
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::export::{stream_body, ExportUser};
use crate::settings::date_string;
use axum::{
    extract::State,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/export", get(export))
}

/// Exports at once, server-wide. Each holds a database connection and a thread while it runs.
pub const MAX_EXPORTS: usize = 2;

/// Letters, digits, `.`, `_` and `-` only, so the name is safe in a header.
fn safe(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

async fn export(user: DataUser, State(state): State<AppState>) -> Result<Response, ApiError> {
    let Ok(permit) = state.export_gate.clone().try_acquire_owned() else {
        let mut r = ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "other exports are running; try again in a minute",
        )
        .into_response();
        r.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        return Ok(r);
    };
    let ctx = Ctx::new(&state, user.id).await?;
    let filename = format!(
        "nimrag-export-{}-{}.zip",
        safe(&user.username),
        date_string(ctx.today())
    );
    let body = stream_body(
        state.pool.clone(),
        state.data_dir.as_ref().clone(),
        ExportUser {
            id: user.id,
            username: user.username.clone(),
        },
        ctx.tz(),
        ctx.now,
        permit,
    );
    let mut r = Response::new(body);
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|e| ApiError::from(anyhow::anyhow!(e)))?,
    );
    Ok(r)
}
