//! Workouts (stage 4a): built on the web, saved here, written as a FIT workout file and sent to
//! the watch by the phone.
//!
//! Owner side (session cookie, `DataUser`; writes need `X-Nimrag: 1`; an admin gets 403):
//!
//! ```text
//! GET    /api/workouts               the user's workouts, the limits, whether sending is possible
//! POST   /api/workouts               create (body: WorkoutBody)
//! GET    /api/workouts/{id}          one workout with its send state
//! PUT    /api/workouts/{id}          replace it (revision + 1)
//! DELETE /api/workouts/{id}
//! POST   /api/workouts/{id}/send     queue it for the watch ("Send to watch")
//! DELETE /api/workouts/{id}/send     take it out of the queue again
//! ```
//!
//! Phone side (a device token, `Authorization: Bearer`, like `/api/files`; no CSRF header because
//! no cookie is involved; the token must belong to a user):
//!
//! ```text
//! GET  /api/device/workouts                the queued workouts: id, name, revision
//! GET  /api/device/workouts/{id}/fit       the FIT file (application/octet-stream)
//! POST /api/device/workouts/{id}/result    {revision, ok, error?}: sent or failed
//! ```
//!
//! The FIT file is made from the stored definition when it is asked for (`nimrag_core::fit::workout`),
//! never stored. A server built without the FIT profile can keep workouts but cannot send them:
//! "send" and the fit download answer 503 with a message that says so, and the list reports
//! `watch_send.available = false` so the page can say it before the user tries.

use super::{ApiJson, Ctx};
use crate::api::{ApiError, AppState, Device};
use crate::auth::DataUser;
use axum::{
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use nimrag_core::client::GARMIN_EPOCH_UNIX_S;
use nimrag_core::fit::encode::EncodeError;
use nimrag_core::fit::workout as fit;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

/// Workouts one user may keep.
pub const MAX_WORKOUTS: i64 = 200;
/// Longest error text kept from the phone's report.
const MAX_ERROR_CHARS: usize = 200;

/// What a 503 says when the server has no FIT profile.
pub const UNAVAILABLE: &str = "this server was built without the FIT profile, so it cannot write \
    workout files for the watch; workouts can still be saved here";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/workouts", get(list).post(create))
        .route("/api/workouts/{id}", get(read).put(update).delete(remove))
        .route("/api/workouts/{id}/send", post(send).delete(cancel))
        .route("/api/device/workouts", get(device_queue))
        .route("/api/device/workouts/{id}/fit", get(device_fit))
        .route("/api/device/workouts/{id}/result", post(device_result))
}

// ---- the workout as the API shows it ----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SportDto {
    Running,
    Cycling,
    Walking,
    Hiking,
    Strength,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum IntensityDto {
    Warmup,
    Active,
    Rest,
    Cooldown,
    Recovery,
}

/// How a step ends.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DurationDto {
    /// Until the lap button is pressed.
    Open,
    Time {
        seconds: u32,
    },
    Distance {
        meters: u32,
    },
    HeartRateBelow {
        bpm: u16,
    },
    HeartRateAbove {
        bpm: u16,
    },
}

/// What the wearer aims for during a step.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetDto {
    None,
    /// Zone 1 to 5 of the watch's heart-rate zones.
    HeartRateZone {
        zone: u8,
    },
    HeartRateRange {
        low_bpm: u16,
        high_bpm: u16,
    },
    /// Seconds per kilometre; `fast_s_per_km` is the smaller number.
    Pace {
        fast_s_per_km: f64,
        slow_s_per_km: f64,
    },
    Speed {
        low_kmh: f64,
        high_kmh: f64,
    },
    /// Steps per minute when running, revolutions per minute otherwise.
    Cadence {
        low: u16,
        high: u16,
    },
}

/// One step inside a repeat block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct StepDto {
    pub intensity: IntensityDto,
    pub duration: DurationDto,
    pub target: TargetDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NodeDto {
    Step {
        intensity: IntensityDto,
        duration: DurationDto,
        target: TargetDto,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        note: Option<String>,
    },
    /// The steps run `count` times in all.
    Repeat { count: u16, steps: Vec<StepDto> },
}

/// What the owner edits: the body of create and update, and the content of a stored workout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct WorkoutBody {
    pub name: String,
    pub sport: SportDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
    pub nodes: Vec<NodeDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SendState {
    /// Never sent, or edited since.
    Idle,
    /// Waiting for the phone to take it to the watch.
    Queued,
    /// The phone handed the file to the watch and the watch took it. (That the watch then shows
    /// the workout is for the first test on a real watch to confirm.)
    Sent,
    /// The phone tried and failed; `error` says why.
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SendInfo {
    pub state: SendState,
    pub requested_at: Option<String>,
    pub finished_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct WorkoutSummary {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub sport: SportDto,
    /// Steps the user sees: every step, those inside repeat blocks included.
    pub steps: u32,
    /// Total time when every step has a time (repeats counted); otherwise `null`.
    pub duration_s: Option<u32>,
    pub updated_at: String,
    pub send: SendInfo,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct WorkoutDetail {
    #[ts(type = "number")]
    pub id: i64,
    /// Counts the edits; the phone reports the revision it sent.
    #[ts(type = "number")]
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
    pub workout: WorkoutBody,
    pub send: SendInfo,
}

/// Whether this server can write the FIT file the watch needs.
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct WatchSend {
    pub available: bool,
    /// Why not, when `available` is false.
    pub reason: Option<String>,
}

/// The limits the builder keeps to (the server checks them again).
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct WorkoutLimits {
    pub max_workouts: u32,
    pub max_steps: u32,
    pub max_block_steps: u32,
    pub repeat_min: u32,
    pub repeat_max: u32,
    pub name_max_bytes: u32,
    pub note_max_bytes: u32,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct WorkoutList {
    pub watch_send: WatchSend,
    pub limits: WorkoutLimits,
    /// Newest change first.
    pub workouts: Vec<WorkoutSummary>,
}

// ---- the phone's side ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct DeviceWorkout {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    #[ts(type = "number")]
    pub revision: i64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct DeviceQueue {
    /// Oldest request first.
    pub workouts: Vec<DeviceWorkout>,
}

/// The phone's report on one upload.
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DeviceResult {
    /// The revision that was uploaded (the `X-Nimrag-Revision` of the fit download).
    #[ts(type = "number")]
    pub revision: i64,
    pub ok: bool,
    /// Why it failed (shown to the owner); ignored when `ok`.
    #[serde(default)]
    #[ts(optional)]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct DeviceResultResponse {
    /// False when the workout was edited, taken out of the queue or deleted since the phone
    /// fetched it: the report was not applied, and the queue (if any) holds the newer version.
    pub applied: bool,
}

// ---- conversion and checks ------------------------------------------------------------------

impl From<SportDto> for fit::Sport {
    fn from(s: SportDto) -> Self {
        match s {
            SportDto::Running => fit::Sport::Running,
            SportDto::Cycling => fit::Sport::Cycling,
            SportDto::Walking => fit::Sport::Walking,
            SportDto::Hiking => fit::Sport::Hiking,
            SportDto::Strength => fit::Sport::Strength,
            SportDto::Other => fit::Sport::Other,
        }
    }
}

impl From<IntensityDto> for fit::Intensity {
    fn from(i: IntensityDto) -> Self {
        match i {
            IntensityDto::Warmup => fit::Intensity::Warmup,
            IntensityDto::Active => fit::Intensity::Active,
            IntensityDto::Rest => fit::Intensity::Rest,
            IntensityDto::Cooldown => fit::Intensity::Cooldown,
            IntensityDto::Recovery => fit::Intensity::Recovery,
        }
    }
}

impl From<DurationDto> for fit::Duration {
    fn from(d: DurationDto) -> Self {
        match d {
            DurationDto::Open => fit::Duration::Open,
            DurationDto::Time { seconds } => fit::Duration::Time { seconds },
            DurationDto::Distance { meters } => fit::Duration::Distance { meters },
            DurationDto::HeartRateBelow { bpm } => fit::Duration::HeartRateBelow { bpm },
            DurationDto::HeartRateAbove { bpm } => fit::Duration::HeartRateAbove { bpm },
        }
    }
}

impl From<TargetDto> for fit::Target {
    fn from(t: TargetDto) -> Self {
        match t {
            TargetDto::None => fit::Target::None,
            TargetDto::HeartRateZone { zone } => fit::Target::HeartRateZone { zone },
            TargetDto::HeartRateRange { low_bpm, high_bpm } => {
                fit::Target::HeartRateRange { low_bpm, high_bpm }
            }
            TargetDto::Pace {
                fast_s_per_km,
                slow_s_per_km,
            } => fit::Target::Pace {
                fast_s_per_km,
                slow_s_per_km,
            },
            TargetDto::Speed { low_kmh, high_kmh } => fit::Target::Speed { low_kmh, high_kmh },
            TargetDto::Cadence { low, high } => fit::Target::Cadence { low, high },
        }
    }
}

impl From<&StepDto> for fit::Step {
    fn from(s: &StepDto) -> Self {
        fit::Step {
            intensity: s.intensity.into(),
            duration: s.duration.into(),
            target: s.target.into(),
            note: s.note.clone().filter(|n| !n.trim().is_empty()),
        }
    }
}

impl From<&NodeDto> for fit::Node {
    fn from(n: &NodeDto) -> Self {
        match n {
            NodeDto::Step {
                intensity,
                duration,
                target,
                note,
            } => fit::Node::Step(fit::Step {
                intensity: (*intensity).into(),
                duration: (*duration).into(),
                target: (*target).into(),
                note: note.clone().filter(|n| !n.trim().is_empty()),
            }),
            NodeDto::Repeat { count, steps } => fit::Node::Repeat {
                count: *count,
                steps: steps.iter().map(Into::into).collect(),
            },
        }
    }
}

impl From<&WorkoutBody> for fit::Workout {
    fn from(b: &WorkoutBody) -> Self {
        fit::Workout {
            name: b.name.trim().to_owned(),
            sport: b.sport.into(),
            note: b.note.clone().filter(|n| !n.trim().is_empty()),
            nodes: b.nodes.iter().map(Into::into).collect(),
        }
    }
}

/// The limits of `nimrag_core::fit::workout`, plus the name length (the encoder would cut a long
/// name; the owner is told instead).
fn check(body: &WorkoutBody) -> Res<fit::Workout> {
    let w = fit::Workout::from(body);
    if w.name.len() > fit::NAME_MAX_BYTES {
        return Err(ApiError::bad_request(format!(
            "the name is at most {} bytes",
            fit::NAME_MAX_BYTES
        )));
    }
    w.validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(w)
}

fn sport_name(s: SportDto) -> &'static str {
    match s {
        SportDto::Running => "running",
        SportDto::Cycling => "cycling",
        SportDto::Walking => "walking",
        SportDto::Hiking => "hiking",
        SportDto::Strength => "strength",
        SportDto::Other => "other",
    }
}

fn unavailable() -> ApiError {
    ApiError::new(StatusCode::SERVICE_UNAVAILABLE, UNAVAILABLE)
}

/// The FIT file for a stored workout.
fn encode(state: &AppState, body: &WorkoutBody) -> Res<Vec<u8>> {
    if !fit::available() {
        return Err(unavailable());
    }
    let w = check(body)?;
    let created = (state.now() - GARMIN_EPOCH_UNIX_S).clamp(0, i64::from(u32::MAX)) as u32;
    fit::encode(&w, created).map_err(|e| match e {
        EncodeError::Unavailable => unavailable(),
        EncodeError::Invalid(m) => ApiError::bad_request(m),
        other => ApiError::from(anyhow::anyhow!("workout encoding failed: {other}")),
    })
}

fn watch_send() -> WatchSend {
    let available = fit::available();
    WatchSend {
        available,
        reason: (!available).then(|| UNAVAILABLE.to_owned()),
    }
}

fn limits() -> WorkoutLimits {
    WorkoutLimits {
        max_workouts: MAX_WORKOUTS as u32,
        max_steps: fit::MAX_STEPS as u32,
        max_block_steps: fit::MAX_BLOCK_STEPS as u32,
        repeat_min: u32::from(fit::REPEAT_MIN),
        repeat_max: u32::from(fit::REPEAT_MAX),
        name_max_bytes: fit::NAME_MAX_BYTES as u32,
        note_max_bytes: fit::NOTE_MAX_BYTES as u32,
    }
}

fn steps_of(body: &WorkoutBody) -> u32 {
    body.nodes
        .iter()
        .map(|n| match n {
            NodeDto::Step { .. } => 1,
            NodeDto::Repeat { steps, .. } => steps.len() as u32,
        })
        .sum()
}

/// Total seconds when every step ends by time.
fn duration_of(body: &WorkoutBody) -> Option<u32> {
    let secs = |d: &DurationDto| match d {
        DurationDto::Time { seconds } => Some(u64::from(*seconds)),
        _ => None,
    };
    let mut total = 0u64;
    for n in &body.nodes {
        total += match n {
            NodeDto::Step { duration, .. } => secs(duration)?,
            NodeDto::Repeat { count, steps } => {
                let one: Option<u64> = steps.iter().map(|s| secs(&s.duration)).sum();
                one? * u64::from(*count)
            }
        };
    }
    u32::try_from(total).ok()
}

// ---- rows -----------------------------------------------------------------------------------

macro_rules! cols {
    () => {
        "id, name, definition, revision, created_at, updated_at, send_state, \
         send_requested_at, send_finished_at, send_error"
    };
}

#[derive(Debug, sqlx::FromRow)]
struct Row {
    id: i64,
    name: String,
    definition: String,
    revision: i64,
    created_at: i64,
    updated_at: i64,
    send_state: String,
    send_requested_at: Option<i64>,
    send_finished_at: Option<i64>,
    send_error: Option<String>,
}

impl Row {
    fn body(&self) -> Res<WorkoutBody> {
        serde_json::from_str(&self.definition).map_err(|e| {
            ApiError::from(anyhow::anyhow!(
                "stored workout {} is unreadable: {e}",
                self.id
            ))
        })
    }

    fn send(&self, ctx: &Ctx) -> SendInfo {
        SendInfo {
            state: match self.send_state.as_str() {
                "queued" => SendState::Queued,
                "sent" => SendState::Sent,
                "failed" => SendState::Failed,
                _ => SendState::Idle,
            },
            requested_at: ctx.opt_ts(self.send_requested_at),
            finished_at: ctx.opt_ts(self.send_finished_at),
            error: self.send_error.clone(),
        }
    }

    fn detail(&self, ctx: &Ctx) -> Res<WorkoutDetail> {
        Ok(WorkoutDetail {
            id: self.id,
            revision: self.revision,
            created_at: ctx.ts(self.created_at),
            updated_at: ctx.ts(self.updated_at),
            workout: self.body()?,
            send: self.send(ctx),
        })
    }

    fn summary(&self, ctx: &Ctx) -> WorkoutSummary {
        let body = self.body().ok();
        WorkoutSummary {
            id: self.id,
            name: self.name.clone(),
            sport: body.as_ref().map_or(SportDto::Other, |b| b.sport),
            steps: body.as_ref().map_or(0, steps_of),
            duration_s: body.as_ref().and_then(duration_of),
            updated_at: ctx.ts(self.updated_at),
            send: self.send(ctx),
        }
    }
}

async fn load(state: &AppState, uid: i64, id: i64) -> Res<Row> {
    sqlx::query_as::<_, Row>(concat!(
        "SELECT ",
        cols!(),
        " FROM workouts WHERE id = ? AND user_id = ?"
    ))
    .bind(id)
    .bind(uid)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("no such workout"))
}

fn definition_of(body: &WorkoutBody) -> Res<String> {
    serde_json::to_string(body).map_err(|e| ApiError::from(anyhow::Error::from(e)))
}

// ---- the owner's handlers -------------------------------------------------------------------------

async fn list(user: DataUser, State(state): State<AppState>) -> Res<Json<WorkoutList>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let rows = sqlx::query_as::<_, Row>(concat!(
        "SELECT ",
        cols!(),
        " FROM workouts WHERE user_id = ? ORDER BY updated_at DESC, id DESC"
    ))
    .bind(ctx.uid)
    .fetch_all(&ctx.pool)
    .await?;
    Ok(Json(WorkoutList {
        watch_send: watch_send(),
        limits: limits(),
        workouts: rows.iter().map(|r| r.summary(&ctx)).collect(),
    }))
}

async fn read(
    user: DataUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Res<Json<WorkoutDetail>> {
    let ctx = Ctx::new(&state, user.id).await?;
    Ok(Json(load(&state, user.id, id).await?.detail(&ctx)?))
}

async fn create(
    user: DataUser,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<WorkoutBody>,
) -> Res<(StatusCode, Json<WorkoutDetail>)> {
    let ctx = Ctx::new(&state, user.id).await?;
    check(&body)?;
    let have: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workouts WHERE user_id = ?")
        .bind(ctx.uid)
        .fetch_one(&ctx.pool)
        .await?;
    if have >= MAX_WORKOUTS {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("a user keeps at most {MAX_WORKOUTS} workouts; delete one first"),
        ));
    }
    let id = sqlx::query(
        "INSERT INTO workouts (user_id, name, sport, definition, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(ctx.uid)
    .bind(body.name.trim())
    .bind(sport_name(body.sport))
    .bind(definition_of(&body)?)
    .bind(ctx.now)
    .bind(ctx.now)
    .execute(&ctx.pool)
    .await?
    .last_insert_rowid();
    let row = load(&state, user.id, id).await?;
    Ok((StatusCode::CREATED, Json(row.detail(&ctx)?)))
}

async fn update(
    user: DataUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ApiJson(body): ApiJson<WorkoutBody>,
) -> Res<Json<WorkoutDetail>> {
    let ctx = Ctx::new(&state, user.id).await?;
    check(&body)?;
    // A queued workout stays queued (the phone fetches the newest revision); a sent or failed one
    // goes back to idle, because the watch has the old version.
    let n = sqlx::query(
        "UPDATE workouts SET name = ?, sport = ?, definition = ?, revision = revision + 1,
                updated_at = ?,
                send_state = CASE WHEN send_state = 'queued' THEN 'queued' ELSE 'idle' END,
                send_finished_at = CASE WHEN send_state = 'queued' THEN send_finished_at ELSE NULL END,
                send_error = CASE WHEN send_state = 'queued' THEN send_error ELSE NULL END
         WHERE id = ? AND user_id = ?",
    )
    .bind(body.name.trim())
    .bind(sport_name(body.sport))
    .bind(definition_of(&body)?)
    .bind(ctx.now)
    .bind(id)
    .bind(ctx.uid)
    .execute(&ctx.pool)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(ApiError::not_found("no such workout"));
    }
    Ok(Json(load(&state, user.id, id).await?.detail(&ctx)?))
}

async fn remove(
    user: DataUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Res<StatusCode> {
    let n = sqlx::query("DELETE FROM workouts WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user.id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if n == 1 {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("no such workout"))
    }
}

/// "Send to watch": the workout must be writable as a FIT file now (so a server without the
/// profile says so here, not later on the phone); then it waits in the queue for the phone.
async fn send(
    user: DataUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Res<Json<WorkoutDetail>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let row = load(&state, user.id, id).await?;
    encode(&state, &row.body()?)?;
    sqlx::query(
        "UPDATE workouts SET send_state = 'queued', send_requested_at = ?, send_finished_at = NULL,
                send_error = NULL
         WHERE id = ? AND user_id = ? AND send_state <> 'queued'",
    )
    .bind(ctx.now)
    .bind(id)
    .bind(ctx.uid)
    .execute(&ctx.pool)
    .await?;
    Ok(Json(load(&state, user.id, id).await?.detail(&ctx)?))
}

async fn cancel(
    user: DataUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Res<Json<WorkoutDetail>> {
    let ctx = Ctx::new(&state, user.id).await?;
    load(&state, user.id, id).await?;
    sqlx::query(
        "UPDATE workouts SET send_state = 'idle', send_requested_at = NULL
         WHERE id = ? AND user_id = ? AND send_state = 'queued'",
    )
    .bind(id)
    .bind(ctx.uid)
    .execute(&ctx.pool)
    .await?;
    Ok(Json(load(&state, user.id, id).await?.detail(&ctx)?))
}

// ---- the phone's handlers ----------------------------------------------------------------------------

fn device_user(device: &Device) -> Res<i64> {
    device.user_id.ok_or_else(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "this device token is not assigned to a user",
        )
    })
}

async fn device_queue(device: Device, State(state): State<AppState>) -> Res<Json<DeviceQueue>> {
    let uid = device_user(&device)?;
    let rows: Vec<(i64, String, i64)> = sqlx::query_as(
        "SELECT id, name, revision FROM workouts WHERE user_id = ? AND send_state = 'queued'
         ORDER BY send_requested_at, id",
    )
    .bind(uid)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(DeviceQueue {
        workouts: rows
            .into_iter()
            .map(|(id, name, revision)| DeviceWorkout { id, name, revision })
            .collect(),
    }))
}

async fn device_fit(
    device: Device,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Res<Response> {
    let uid = device_user(&device)?;
    let row = load(&state, uid, id).await?;
    if row.send_state != "queued" {
        return Err(ApiError::not_found("this workout is not queued"));
    }
    let bytes = encode(&state, &row.body()?)?;
    let mut resp = Response::new(axum::body::Body::from(bytes));
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    h.insert(
        "x-nimrag-revision",
        HeaderValue::from_str(&row.revision.to_string()).map_err(anyhow::Error::from)?,
    );
    Ok(resp)
}

async fn device_result(
    device: Device,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ApiJson(r): ApiJson<DeviceResult>,
) -> Res<Json<DeviceResultResponse>> {
    let uid = device_user(&device)?;
    load(&state, uid, id).await?;
    let (new_state, error) = if r.ok {
        ("sent", None)
    } else {
        let text: String = r
            .error
            .as_deref()
            .unwrap_or("the phone could not send it")
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_ERROR_CHARS)
            .collect();
        ("failed", Some(text))
    };
    let n = sqlx::query(
        "UPDATE workouts SET send_state = ?, send_finished_at = ?, send_error = ?
         WHERE id = ? AND user_id = ? AND revision = ? AND send_state = 'queued'",
    )
    .bind(new_state)
    .bind(state.now())
    .bind(error)
    .bind(id)
    .bind(uid)
    .bind(r.revision)
    .execute(&state.pool)
    .await?
    .rows_affected();
    Ok(Json(DeviceResultResponse { applied: n == 1 }))
}
