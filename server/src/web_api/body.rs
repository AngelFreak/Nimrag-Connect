//! `GET/POST /api/body`, `DELETE /api/body/{id}`: manual body measurements
//! (weight, body fat, waist). Owner data in its own table; `reparse` never touches it.

use super::{ApiJson, Ctx, TimedValue};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::settings::{local_noon, parse_date};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get},
    Json, Router,
};
use chrono::{DateTime, Duration};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/body", get(read).post(create))
        .route("/api/body/{id}", delete(remove))
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct BodyEntry {
    #[ts(type = "number")]
    pub id: i64,
    pub measured_at: String,
    pub weight_kg: Option<f64>,
    pub body_fat_pct: Option<f64>,
    pub waist_cm: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct NewBodyEntry {
    /// RFC 3339 or `YYYY-MM-DD` (noon that day); defaults to now.
    #[serde(default)]
    #[ts(optional)]
    pub measured_at: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub weight_kg: Option<f64>,
    #[serde(default)]
    #[ts(optional)]
    pub body_fat_pct: Option<f64>,
    #[serde(default)]
    #[ts(optional)]
    pub waist_cm: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct BodyValue {
    pub value: f64,
    pub at: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct BodyResponse {
    pub sample: bool,
    pub latest_weight: Option<BodyValue>,
    pub latest_body_fat: Option<BodyValue>,
    pub latest_waist: Option<BodyValue>,
    /// Latest weight minus the first weight of the last 30 days; `null` with fewer than two.
    pub weight_change_30d_kg: Option<f64>,
    /// Weigh-ins of the last 90 days.
    pub weights: Vec<TimedValue>,
    /// The same weigh-ins smoothed (exponential average, 7-day time constant).
    pub weight_trend: Vec<TimedValue>,
    /// Newest first, at most 100.
    pub entries: Vec<BodyEntry>,
}

type Row = (i64, i64, Option<f64>, Option<f64>, Option<f64>);

fn entry(ctx: &Ctx, r: Row) -> BodyEntry {
    BodyEntry {
        id: r.0,
        measured_at: ctx.ts(r.1),
        weight_kg: r.2,
        body_fat_pct: r.3,
        waist_cm: r.4,
    }
}

async fn read(user: DataUser, State(state): State<AppState>) -> Res<Json<BodyResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, measured_at, weight_kg, body_fat_pct, waist_cm FROM body_measurements
         WHERE user_id = ? ORDER BY measured_at DESC, id DESC",
    )
    .bind(ctx.uid)
    .fetch_all(&ctx.pool)
    .await?;
    let latest = |f: fn(&Row) -> Option<f64>| {
        rows.iter().find_map(|r| {
            f(r).map(|v| BodyValue {
                value: v,
                at: ctx.ts(r.1),
            })
        })
    };
    let weights: Vec<(i64, f64)> = rows
        .iter()
        .rev()
        .filter(|r| r.1 >= ctx.now - 90 * 86_400)
        .filter_map(|r| r.2.map(|w| (r.1, w)))
        .collect();
    let mut trend = Vec::new();
    let mut prev: Option<(i64, f64)> = None;
    for (t, w) in &weights {
        let v = match prev {
            None => *w,
            Some((pt, pv)) => {
                let a = 1.0 - (-((t - pt) as f64) / (7.0 * 86_400.0)).exp();
                pv + (w - pv) * a
            }
        };
        prev = Some((*t, v));
        trend.push(TimedValue {
            time: ctx.ts(*t),
            value: Some((v * 10.0).round() / 10.0),
        });
    }
    let recent: Vec<&(i64, f64)> = weights
        .iter()
        .filter(|w| w.0 >= ctx.now - 30 * 86_400)
        .collect();
    Ok(Json(BodyResponse {
        sample: ctx.sample(),
        latest_weight: latest(|r| r.2),
        latest_body_fat: latest(|r| r.3),
        latest_waist: latest(|r| r.4),
        weight_change_30d_kg: match (recent.first(), recent.last()) {
            (Some(a), Some(b)) if recent.len() > 1 => Some(((b.1 - a.1) * 10.0).round() / 10.0),
            _ => None,
        },
        weights: weights
            .iter()
            .map(|(t, w)| TimedValue {
                time: ctx.ts(*t),
                value: Some(*w),
            })
            .collect(),
        weight_trend: trend,
        entries: rows.into_iter().take(100).map(|r| entry(&ctx, r)).collect(),
    }))
}

fn check(name: &str, v: Option<f64>, lo: f64, hi: f64) -> Result<(), ApiError> {
    match v {
        Some(x) if !x.is_finite() || x < lo || x > hi => Err(ApiError::bad_request(format!(
            "{name} must be between {lo} and {hi}"
        ))),
        _ => Ok(()),
    }
}

async fn create(
    user: DataUser,
    State(state): State<AppState>,
    ApiJson(b): ApiJson<NewBodyEntry>,
) -> Res<(StatusCode, Json<BodyEntry>)> {
    let ctx = Ctx::new(&state, user.id).await?;
    if b.weight_kg.is_none() && b.body_fat_pct.is_none() && b.waist_cm.is_none() {
        return Err(ApiError::bad_request(
            "give at least one of weight_kg, body_fat_pct, waist_cm",
        ));
    }
    check("weight_kg", b.weight_kg, 20.0, 400.0)?;
    check("body_fat_pct", b.body_fat_pct, 2.0, 70.0)?;
    check("waist_cm", b.waist_cm, 30.0, 300.0)?;
    let at = match b.measured_at.as_deref() {
        None => ctx.now,
        Some(s) => DateTime::parse_from_rfc3339(s)
            .map(|d| d.timestamp())
            .ok()
            .or_else(|| parse_date(s).map(|d| local_noon(ctx.tz(), d)))
            .ok_or_else(|| ApiError::bad_request("measured_at must be RFC 3339 or YYYY-MM-DD"))?,
    };
    if at > ctx.now + Duration::days(1).num_seconds() {
        return Err(ApiError::bad_request("measured_at is in the future"));
    }
    let id = sqlx::query(
        "INSERT INTO body_measurements (user_id, measured_at, weight_kg, body_fat_pct, waist_cm, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(ctx.uid)
    .bind(at)
    .bind(b.weight_kg)
    .bind(b.body_fat_pct)
    .bind(b.waist_cm)
    .bind(ctx.now)
    .execute(&ctx.pool)
    .await?
    .last_insert_rowid();
    Ok((
        StatusCode::CREATED,
        Json(entry(
            &ctx,
            (id, at, b.weight_kg, b.body_fat_pct, b.waist_cm),
        )),
    ))
}

async fn remove(
    user: DataUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Res<StatusCode> {
    let n = sqlx::query("DELETE FROM body_measurements WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user.id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if n == 1 {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("no such entry"))
    }
}
