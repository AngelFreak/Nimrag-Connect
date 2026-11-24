//! `GET/PATCH /api/activities/{id}/strength`.
//!
//! Edits (reps, weight, exercise name) are stored in `strength_set_overrides`,
//! keyed by the activity's stable identity (`activity_key`, else the file hash)
//! and the set's ordinal, never in the derived tables, so `reparse` keeps them.
//! Reads merge the overrides over the derived rows.

use super::activities::{hr_points, records, summary_of, valid_id};
use super::{Ctx, TimedValue};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/activities/{id}/strength", get(read).patch(patch))
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct StrengthSet {
    /// Ordinal of the set in the watch's file; stable across `reparse`.
    pub idx: u32,
    pub reps: Option<f64>,
    pub weight_kg: Option<f64>,
    pub duration_s: Option<f64>,
    pub exercise: Option<String>,
    /// True when the owner changed any value of this set.
    pub edited: bool,
    pub original_reps: Option<f64>,
    pub original_weight_kg: Option<f64>,
    pub original_exercise: Option<String>,
}

/// Consecutive sets of the same exercise.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ExerciseGroup {
    pub name: Option<String>,
    pub sets: Vec<StrengthSet>,
    pub volume_kg: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct StrengthPrevious {
    pub id: String,
    pub start: String,
    pub duration_s: Option<f64>,
    pub sets: u32,
    pub volume_kg: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct StrengthResponse {
    pub sample: bool,
    pub id: String,
    pub start: String,
    pub duration_s: Option<f64>,
    pub avg_hr_bpm: Option<f64>,
    pub max_hr_bpm: Option<f64>,
    /// Working sets (rest rows excluded).
    pub sets: u32,
    /// Sum of reps x weight over sets that have both; `null` when none do.
    pub volume_kg: Option<f64>,
    pub exercises: Vec<ExerciseGroup>,
    /// Heart rate during the session, thinned to at most 120 points.
    pub heart_rate: Vec<TimedValue>,
    /// The strength session before this one, to compare against.
    pub previous: Option<StrengthPrevious>,
}

// ---- reading -----------------------------------------------------------------------------

struct Raw {
    idx: i64,
    duration_s: Option<f64>,
    reps: Option<f64>,
    weight_kg: Option<f64>,
    category: Option<i64>,
    subtype: Option<i64>,
    name: Option<String>,
}

fn humanise(category_name: &str) -> String {
    let s = category_name.replace('_', " ");
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| {
        f.to_uppercase().collect::<String>() + c.as_str()
    })
}

async fn overrides(ctx: &Ctx, ident: &str) -> Res<HashMap<i64, Map<String, Value>>> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT set_idx, fields FROM strength_set_overrides WHERE user_id = ? AND activity_ident = ?",
    )
    .bind(ctx.uid)
    .bind(ident)
    .fetch_all(&ctx.pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(i, f)| match serde_json::from_str::<Value>(&f) {
            Ok(Value::Object(m)) => Some((i, m)),
            _ => None,
        })
        .collect())
}

async fn raw_sets(ctx: &Ctx, sha: &str) -> Res<Vec<Raw>> {
    type Row = (
        i64,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<i64>,
        Option<String>,
        Option<i64>,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT idx, duration_s, reps, weight_kg, category, category_name, category_subtype
         FROM strength_sets WHERE sha256 = ? AND COALESCE(set_type, 1) <> 0 ORDER BY idx",
    )
    .bind(sha)
    .fetch_all(&ctx.pool)
    .await?;
    let titles: Vec<(Option<i64>, Option<i64>, Option<String>)> =
        sqlx::query_as("SELECT category, subtype, name FROM exercise_titles WHERE sha256 = ?")
            .bind(sha)
            .fetch_all(&ctx.pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(
            |(idx, duration_s, reps, weight_kg, category, category_name, subtype)| {
                let titled = |sub: Option<i64>| {
                    titles
                        .iter()
                        .find(|t| {
                            t.0 == category
                                && t.1 == sub
                                && t.2.as_deref().is_some_and(|n| !n.is_empty())
                        })
                        .and_then(|t| t.2.clone())
                };
                let name = titled(subtype)
                    .or_else(|| titled(None))
                    .or_else(|| category_name.as_deref().map(humanise));
                Raw {
                    idx,
                    duration_s,
                    reps,
                    weight_kg,
                    category,
                    subtype,
                    name,
                }
            },
        )
        .collect())
}

/// The working sets of an activity with the owner's edits applied.
pub async fn load_sets(ctx: &Ctx, sha: &str, ident: &str) -> Res<Vec<StrengthSet>> {
    let ov = overrides(ctx, ident).await?;
    Ok(raw_sets(ctx, sha)
        .await?
        .into_iter()
        .map(|r| {
            let o = ov.get(&r.idx);
            let num = |key: &str, orig: Option<f64>| match o.and_then(|m| m.get(key)) {
                Some(Value::Null) => None,
                Some(v) => v.as_f64().or(orig),
                None => orig,
            };
            let exercise = match o.and_then(|m| m.get("exercise")) {
                Some(Value::String(s)) => Some(s.clone()),
                _ => r.name.clone(),
            };
            // Category/subtype only group sets whose name was not edited.
            let _ = (r.category, r.subtype);
            StrengthSet {
                idx: r.idx as u32,
                reps: num("reps", r.reps),
                weight_kg: num("weight_kg", r.weight_kg),
                duration_s: r.duration_s,
                exercise,
                edited: o.is_some_and(|m| !m.is_empty()),
                original_reps: r.reps,
                original_weight_kg: r.weight_kg,
                original_exercise: r.name,
            }
        })
        .collect())
}

pub fn volume(sets: &[StrengthSet]) -> Option<f64> {
    let v: Vec<f64> = sets
        .iter()
        .filter_map(|s| s.reps.zip(s.weight_kg).map(|(r, w)| r * w))
        .collect();
    (!v.is_empty()).then(|| (v.iter().sum::<f64>() * 10.0).round() / 10.0)
}

pub fn group(sets: &[StrengthSet]) -> Vec<ExerciseGroup> {
    let mut out: Vec<ExerciseGroup> = Vec::new();
    for s in sets {
        match out.last_mut() {
            Some(g) if g.name == s.exercise => g.sets.push(s.clone()),
            _ => out.push(ExerciseGroup {
                name: s.exercise.clone(),
                sets: vec![s.clone()],
                volume_kg: None,
            }),
        }
    }
    for g in &mut out {
        g.volume_kg = volume(&g.sets);
    }
    out
}

pub async fn ident_of(ctx: &Ctx, id: &str) -> Res<String> {
    if !valid_id(id) {
        return Err(ApiError::not_found("no such activity"));
    }
    // Joining file_owners makes someone else's activity a 404 on every by-id route.
    sqlx::query_scalar::<_, String>(
        "SELECT COALESCE(a.activity_key, a.sha256) FROM activities a
         JOIN file_owners o ON o.sha256 = a.sha256 AND o.user_id = ? WHERE a.sha256 = ?",
    )
    .bind(ctx.uid)
    .bind(id)
    .fetch_optional(&ctx.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("no such activity"))
}

async fn build(ctx: &Ctx, id: &str) -> Res<StrengthResponse> {
    let ident = ident_of(ctx, id).await?;
    let me = summary_of(ctx, id).await?;
    let sets = load_sets(ctx, id, &ident).await?;
    let recs = records(ctx, id).await?;
    // The session before this one that is a strength session.
    let before =
        chrono::DateTime::parse_from_rfc3339(&me.start).map_or(i64::MAX, |d| d.timestamp());
    let prev = super::activities::list_items(ctx, Some("strength"), None, Some(before), 1).await?;
    Ok(StrengthResponse {
        sample: ctx.sample(),
        id: id.to_owned(),
        start: me.start,
        duration_s: me.duration_s,
        avg_hr_bpm: me.avg_hr_bpm,
        max_hr_bpm: me.max_hr_bpm,
        sets: sets.len() as u32,
        volume_kg: volume(&sets),
        exercises: group(&sets),
        heart_rate: hr_points(ctx, &recs, 120),
        previous: prev.into_iter().next().map(|p| StrengthPrevious {
            id: p.id,
            start: p.start,
            duration_s: p.duration_s,
            sets: p.sets.unwrap_or(0),
            volume_kg: p.volume_kg,
        }),
    })
}

async fn read(
    user: DataUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Res<Json<StrengthResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    Ok(Json(build(&ctx, &id).await?))
}

// ---- editing -------------------------------------------------------------------------------

fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// One set's edit. A field that is absent stays as it is, `null` clears the value
/// (bodyweight, unknown reps), a value replaces it. `reset: true` drops every
/// edit of the set and returns it to what the watch recorded.
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SetPatch {
    pub idx: u32,
    #[serde(default)]
    #[ts(optional)]
    pub reset: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional, type = "number | null")]
    pub reps: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional, type = "number | null")]
    pub weight_kg: Option<Option<f64>>,
    #[serde(default)]
    #[ts(optional)]
    pub exercise: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct StrengthPatch {
    pub sets: Vec<SetPatch>,
}

fn check_number(name: &str, v: Option<f64>, max: f64, whole: bool) -> Result<(), ApiError> {
    match v {
        Some(x) if !x.is_finite() || x < 0.0 || x > max || (whole && x.fract() != 0.0) => {
            Err(ApiError::bad_request(format!(
                "{name} must be {}0 to {max}",
                if whole { "a whole number " } else { "" }
            )))
        }
        _ => Ok(()),
    }
}

async fn patch(
    user: DataUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    super::ApiJson(body): super::ApiJson<StrengthPatch>,
) -> Res<Json<StrengthResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let ident = ident_of(&ctx, &id).await?;
    if body.sets.is_empty() || body.sets.len() > 500 {
        return Err(ApiError::bad_request("sets must hold 1 to 500 edits"));
    }
    let known: Vec<i64> = raw_sets(&ctx, &id).await?.iter().map(|r| r.idx).collect();
    for p in &body.sets {
        if !known.contains(&i64::from(p.idx)) {
            return Err(ApiError::bad_request(format!(
                "activity has no working set {}",
                p.idx
            )));
        }
        check_number("reps", p.reps.flatten(), 10_000.0, true)?;
        check_number("weight_kg", p.weight_kg.flatten(), 2_000.0, false)?;
        if let Some(e) = &p.exercise {
            if e.trim().is_empty() || e.chars().count() > 80 {
                return Err(ApiError::bad_request(
                    "exercise must have 1 to 80 characters",
                ));
            }
        }
    }
    let mut tx = state.pool.begin().await.map_err(anyhow::Error::from)?;
    for p in &body.sets {
        let idx = i64::from(p.idx);
        if p.reset == Some(true) {
            sqlx::query(
                "DELETE FROM strength_set_overrides WHERE user_id = ? AND activity_ident = ? AND set_idx = ?",
            )
            .bind(ctx.uid)
            .bind(&ident)
            .bind(idx)
            .execute(&mut *tx)
            .await?;
            continue;
        }
        let current: Option<String> = sqlx::query_scalar(
            "SELECT fields FROM strength_set_overrides WHERE user_id = ? AND activity_ident = ? AND set_idx = ?",
        )
        .bind(ctx.uid)
        .bind(&ident)
        .bind(idx)
        .fetch_optional(&mut *tx)
        .await?;
        let mut fields = match current.and_then(|c| serde_json::from_str::<Value>(&c).ok()) {
            Some(Value::Object(m)) => m,
            _ => Map::new(),
        };
        if let Some(r) = p.reps {
            fields.insert("reps".into(), serde_json::json!(r));
        }
        if let Some(w) = p.weight_kg {
            fields.insert("weight_kg".into(), serde_json::json!(w));
        }
        if let Some(e) = &p.exercise {
            fields.insert("exercise".into(), Value::String(e.trim().to_owned()));
        }
        if fields.is_empty() {
            continue;
        }
        sqlx::query(
            "INSERT INTO strength_set_overrides (user_id, activity_ident, set_idx, fields, updated_at) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (user_id, activity_ident, set_idx) DO UPDATE SET fields = excluded.fields,
                                                                  updated_at = excluded.updated_at",
        )
        .bind(ctx.uid)
        .bind(&ident)
        .bind(idx)
        .bind(Value::Object(fields).to_string())
        .bind(ctx.now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await.map_err(anyhow::Error::from)?;
    Ok(Json(build(&ctx, &id).await?))
}
