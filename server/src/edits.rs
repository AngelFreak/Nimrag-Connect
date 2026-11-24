//! Offline edits (docs/design/offline-edits.md): the changes the phone's copy may make while the
//! server is away, and the two endpoints the phone uses to send them on and to pull the result.
//!
//! ```text
//! POST /api/device/edits   (device token, Authorization: Bearer; no CSRF header)
//!   {"op_id": "...", "method": "PATCH", "path": "/api/settings", "body": "{\"step_goal\":9000}",
//!    "made_at": 1800000000}
//!   -> the status and body the normal handler gave, as the token's user
//!      400 not an edit a phone may send, 401 no token, 403 token without a user
//! GET  /api/device/state   (device token)
//!   -> DeviceState: the user's settings, workouts, body entries and strength edits
//! ```
//!
//! [`Edit::parse`] is the allowlist, shared by both ends: the phone's copy accepts exactly these
//! requests and the server runs exactly these. `body` is the request body as JSON text (empty
//! for none). A request is run through the same router the web app uses, as the token's user
//! (the [`AuthUser`] rides on the request as an extension, which no client can set).
//!
//! The answer to every op id is remembered (`device_edits_applied`), so a replay after a lost
//! answer gets the first answer and changes nothing. Statuses that mean "try again later"
//! (5xx, 408, 429) are not remembered. A crash between the handler and that row would let a
//! replay run the handler twice; the window is a few microseconds and a request is small.

use crate::api::{ApiError, AppState, Device, MAX_JSON_BYTES};
use crate::auth::AuthUser;
use crate::users::{self, Role};
use crate::web_api::{self, ApiJson};
use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderValue, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tower::ServiceExt;

type Res<T> = Result<T, ApiError>;

/// The user settings a phone keeps a copy of (`PATCH /api/settings` changes exactly these).
pub const SETTING_KEYS: [&str; 5] = [
    "timezone",
    "step_goal",
    "intensity_goal_min",
    "sleep_need_min",
    "home_cards",
];
/// Version of [`DeviceState`].
pub const STATE_VERSION: u32 = 1;
/// Longest answer of a handler the server relays (a strength session with heart rate is a few KB).
const MAX_ANSWER_BYTES: usize = 1024 * 1024;
/// Remembered answers older than this are forgotten.
const KEEP_SECS: i64 = 30 * 24 * 3600;

pub fn routes() -> Router<AppState> {
    Router::new()
        // The body is a JSON text inside JSON, so escaping can make it several times larger.
        .route(
            "/api/device/edits",
            post(apply).layer(DefaultBodyLimit::max(4 * MAX_JSON_BYTES)),
        )
        .route("/api/device/state", get(state))
}

// ---- the allowlist ------------------------------------------------------------------------

/// An activity as an edit names it. On the phone this is the activity's id (the SHA-256 of its
/// file); on the wire it carries the start time too, `<sha256>@<start unix seconds>`, which the
/// server uses to find the activity if it does not hold that file under that hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityRef<'a> {
    pub sha256: &'a str,
    pub start_ts: Option<i64>,
}

impl<'a> ActivityRef<'a> {
    pub fn parse(segment: &'a str) -> Option<Self> {
        let (sha256, start) = match segment.split_once('@') {
            Some((sha, start)) => (sha, Some(number(start)?)),
            None => (segment, None),
        };
        crate::is_sha256_hex(sha256).then_some(Self {
            sha256,
            start_ts: start,
        })
    }

    pub fn segment(&self) -> String {
        match self.start_ts {
            Some(ts) => format!("{}@{ts}", self.sha256),
            None => self.sha256.to_owned(),
        }
    }
}

/// A positive decimal number (no sign, no spaces).
fn number(s: &str) -> Option<i64> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// One of the requests a phone may make offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit<'a> {
    Settings,
    BodyCreate,
    BodyDelete(i64),
    Strength(ActivityRef<'a>),
    WorkoutCreate,
    WorkoutUpdate(i64),
    WorkoutDelete(i64),
    WorkoutSend(i64),
    WorkoutUnsend(i64),
}

impl<'a> Edit<'a> {
    /// The edit a request is, or `None` when it is not on the list.
    pub fn parse(method: &Method, path: &'a str) -> Option<Self> {
        let segments: Vec<&str> = path.strip_prefix('/')?.split('/').collect();
        Some(match (method, segments.as_slice()) {
            (&Method::PATCH, ["api", "settings"]) => Self::Settings,
            (&Method::POST, ["api", "body"]) => Self::BodyCreate,
            (&Method::DELETE, ["api", "body", id]) => Self::BodyDelete(number(id)?),
            (&Method::PATCH, ["api", "activities", a, "strength"]) => {
                Self::Strength(ActivityRef::parse(a)?)
            }
            (&Method::POST, ["api", "workouts"]) => Self::WorkoutCreate,
            (&Method::PUT, ["api", "workouts", id]) => Self::WorkoutUpdate(number(id)?),
            (&Method::DELETE, ["api", "workouts", id]) => Self::WorkoutDelete(number(id)?),
            (&Method::POST, ["api", "workouts", id, "send"]) => Self::WorkoutSend(number(id)?),
            (&Method::DELETE, ["api", "workouts", id, "send"]) => Self::WorkoutUnsend(number(id)?),
            _ => return None,
        })
    }

    /// The request path of this edit.
    pub fn path(&self) -> String {
        match self {
            Self::Settings => "/api/settings".into(),
            Self::BodyCreate => "/api/body".into(),
            Self::BodyDelete(id) => format!("/api/body/{id}"),
            Self::Strength(a) => format!("/api/activities/{}/strength", a.segment()),
            Self::WorkoutCreate => "/api/workouts".into(),
            Self::WorkoutUpdate(id) | Self::WorkoutDelete(id) => format!("/api/workouts/{id}"),
            Self::WorkoutSend(id) | Self::WorkoutUnsend(id) => format!("/api/workouts/{id}/send"),
        }
    }

    /// The kind of row this edit creates (`workout`, `body`), for the id map.
    pub fn creates(&self) -> Option<&'static str> {
        match self {
            Self::WorkoutCreate => Some("workout"),
            Self::BodyCreate => Some("body"),
            _ => None,
        }
    }

    /// The row this edit names by id: its kind and id.
    pub fn row(&self) -> Option<(&'static str, i64)> {
        match *self {
            Self::BodyDelete(id) => Some(("body", id)),
            Self::WorkoutUpdate(id)
            | Self::WorkoutDelete(id)
            | Self::WorkoutSend(id)
            | Self::WorkoutUnsend(id) => Some(("workout", id)),
            _ => None,
        }
    }

    /// The same edit on another row id.
    pub fn with_row(self, new: i64) -> Self {
        match self {
            Self::BodyDelete(_) => Self::BodyDelete(new),
            Self::WorkoutUpdate(_) => Self::WorkoutUpdate(new),
            Self::WorkoutDelete(_) => Self::WorkoutDelete(new),
            Self::WorkoutSend(_) => Self::WorkoutSend(new),
            Self::WorkoutUnsend(_) => Self::WorkoutUnsend(new),
            other => other,
        }
    }
}

// ---- POST /api/device/edits ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditRequest {
    /// Unique per change; a replay carries the same one.
    op_id: String,
    method: String,
    path: String,
    /// The request body as JSON text; empty for none.
    #[serde(default)]
    body: String,
    /// Unix seconds when the change was made on the phone.
    made_at: i64,
}

fn answer(status: StatusCode, body: impl Into<Body>) -> Response {
    let mut resp = Response::new(body.into());
    *resp.status_mut() = status;
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    resp
}

fn device_user(device: &Device) -> Res<i64> {
    device.user_id.ok_or_else(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "this device token is not assigned to a user",
        )
    })
}

/// The activity id on this server for a phone's activity: the file's own hash when this user
/// has that file, else the one current activity that started at the same time.
async fn resolve_activity(state: &AppState, uid: i64, a: &ActivityRef<'_>) -> Res<String> {
    let known: Option<String> = sqlx::query_scalar(
        "SELECT a.sha256 FROM activities a
         JOIN file_owners o ON o.sha256 = a.sha256 AND o.user_id = ? WHERE a.sha256 = ?",
    )
    .bind(uid)
    .bind(a.sha256)
    .fetch_optional(&state.pool)
    .await?;
    if let Some(sha) = known {
        return Ok(sha);
    }
    if let Some(start) = a.start_ts {
        let same_start: Vec<String> = sqlx::query_scalar(
            "SELECT a.sha256 FROM activities_current a
             JOIN file_owners o ON o.sha256 = a.sha256 AND o.user_id = ?
             WHERE a.start_ts = ? LIMIT 2",
        )
        .bind(uid)
        .bind(start)
        .fetch_all(&state.pool)
        .await?;
        if let [only] = same_start.as_slice() {
            return Ok(only.clone());
        }
    }
    Err(ApiError::not_found("no such activity"))
}

/// A body entry made without a time was made when the phone saved it, not when the server hears of it.
fn dated(body: String, made_at: i64) -> String {
    let Ok(serde_json::Value::Object(mut o)) = serde_json::from_str(&body) else {
        return body;
    };
    if o.get("measured_at").is_none_or(|v| v.is_null()) {
        if let Some(at) = chrono::DateTime::from_timestamp(made_at, 0) {
            o.insert("measured_at".into(), at.to_rfc3339().into());
        }
    }
    serde_json::Value::Object(o).to_string()
}

/// Run one edit through the web API's router as `user`.
async fn dispatch(
    state: &AppState,
    user: AuthUser,
    method: Method,
    path: &str,
    body: String,
) -> Res<(StatusCode, Bytes)> {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    req.extensions_mut().insert(user);
    let app = web_api::router()
        .layer(DefaultBodyLimit::max(MAX_JSON_BYTES))
        .with_state(state.clone());
    let Ok(resp) = app.oneshot(req).await;
    let (parts, body) = resp.into_parts();
    let bytes = axum::body::to_bytes(body, MAX_ANSWER_BYTES)
        .await
        .map_err(anyhow::Error::from)?;
    Ok((parts.status, bytes))
}

async fn apply(
    device: Device,
    State(state): State<AppState>,
    ApiJson(req): ApiJson<EditRequest>,
) -> Res<Response> {
    let uid = device_user(&device)?;
    let op_ok = (8..=64).contains(&req.op_id.len())
        && req
            .op_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !op_ok {
        return Err(ApiError::bad_request(
            "op_id must have 8 to 64 letters, digits, - or _",
        ));
    }
    let method = Method::from_bytes(req.method.as_bytes())
        .map_err(|_| ApiError::bad_request("unknown method"))?;
    let edit = Edit::parse(&method, &req.path).ok_or_else(|| {
        ApiError::bad_request("this is not a change a phone can send ahead; do it on the server")
    })?;

    // One edit at a time: two copies of the same op must not both run.
    let _one_at_a_time = state.edits_lock.lock().await;
    let seen: Option<(i64, String)> = sqlx::query_as(
        "SELECT status, body FROM device_edits_applied WHERE user_id = ? AND op_id = ?",
    )
    .bind(uid)
    .bind(&req.op_id)
    .fetch_optional(&state.pool)
    .await?;
    if let Some((status, body)) = seen {
        let status = u16::try_from(status)
            .ok()
            .and_then(|s| StatusCode::from_u16(s).ok())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return Ok(answer(status, body));
    }

    let path = match edit {
        Edit::Strength(a) => Edit::Strength(ActivityRef {
            sha256: &resolve_activity(&state, uid, &a).await?,
            start_ts: None,
        })
        .path(),
        other => other.path(),
    };
    let body = if edit == Edit::BodyCreate {
        dated(req.body, req.made_at.min(state.now()))
    } else {
        req.body
    };
    let user = users::by_id(&state.pool, uid)
        .await?
        .filter(|u| u.disabled_at.is_none() && u.role == Role::User)
        .ok_or_else(|| ApiError::new(StatusCode::FORBIDDEN, "this account has no data"))?;
    let user = AuthUser {
        id: user.id,
        username: user.username,
        role: user.role,
    };

    let (status, bytes) = dispatch(&state, user, method, &path, body).await?;
    let retryable = status.is_server_error()
        || status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if !retryable {
        let now = state.now();
        sqlx::query("DELETE FROM device_edits_applied WHERE applied_at < ?")
            .bind(now - KEEP_SECS)
            .execute(&state.pool)
            .await?;
        sqlx::query(
            "INSERT OR IGNORE INTO device_edits_applied (user_id, op_id, status, body, applied_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(uid)
        .bind(&req.op_id)
        .bind(i64::from(status.as_u16()))
        .bind(&text)
        .bind(now)
        .execute(&state.pool)
        .await?;
    }
    tracing::info!(op = %req.op_id, status = status.as_u16(), made_at = req.made_at, "device edit");
    Ok(answer(status, text))
}

// ---- GET /api/device/state --------------------------------------------------------------------------

/// What the phone's copy of the owner's data is made of. Rows are the stored ones (ids and
/// all), so the phone can replace its own rows with them.
#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceState {
    pub version: u32,
    /// `user_settings` rows by key (only [`SETTING_KEYS`]); a key that is absent is at its default.
    pub settings: BTreeMap<String, String>,
    pub workouts: Vec<StateWorkout>,
    pub body: Vec<StateBody>,
    pub strength: Vec<StateStrength>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct StateWorkout {
    pub id: i64,
    pub name: String,
    pub sport: String,
    /// The `WorkoutBody` as JSON text.
    pub definition: String,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub send_state: String,
    pub send_requested_at: Option<i64>,
    pub send_finished_at: Option<i64>,
    pub send_error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct StateBody {
    pub id: i64,
    pub measured_at: i64,
    pub weight_kg: Option<f64>,
    pub body_fat_pct: Option<f64>,
    pub waist_cm: Option<f64>,
    pub created_at: i64,
}

/// One set's edit, keyed as the server keys it (`activity_key`, else the file hash).
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct StateStrength {
    pub activity_ident: String,
    pub set_idx: i64,
    /// The edited fields as JSON text.
    pub fields: String,
    pub updated_at: i64,
}

async fn state(device: Device, State(state): State<AppState>) -> Res<Json<DeviceState>> {
    let uid = device_user(&device)?;
    let pool = &state.pool;
    let settings: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM user_settings WHERE user_id = ?")
            .bind(uid)
            .fetch_all(pool)
            .await?;
    let workouts = sqlx::query_as::<_, StateWorkout>(
        "SELECT id, name, sport, definition, revision, created_at, updated_at, send_state,
                send_requested_at, send_finished_at, send_error
         FROM workouts WHERE user_id = ? ORDER BY id",
    )
    .bind(uid)
    .fetch_all(pool)
    .await?;
    let body = sqlx::query_as::<_, StateBody>(
        "SELECT id, measured_at, weight_kg, body_fat_pct, waist_cm, created_at
         FROM body_measurements WHERE user_id = ? ORDER BY id",
    )
    .bind(uid)
    .fetch_all(pool)
    .await?;
    let strength = sqlx::query_as::<_, StateStrength>(
        "SELECT activity_ident, set_idx, fields, updated_at FROM strength_set_overrides
         WHERE user_id = ? ORDER BY activity_ident, set_idx",
    )
    .bind(uid)
    .fetch_all(pool)
    .await?;
    Ok(Json(DeviceState {
        version: STATE_VERSION,
        settings: settings
            .into_iter()
            .filter(|(k, _)| SETTING_KEYS.contains(&k.as_str()))
            .collect(),
        workouts,
        body,
        strength,
    }))
}

/// A 403 for a request the phone's copy cannot take.
pub fn needs_server() -> Response {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "this needs the server: the phone's copy only takes changes to settings, body entries, \
         strength sets and workouts; do this when the server is reachable",
    )
    .into_response()
}
