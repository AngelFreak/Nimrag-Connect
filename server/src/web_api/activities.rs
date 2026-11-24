//! `GET /api/activities`, `/api/activities/{id}` and `/api/activities/{id}/track`.
//!
//! An activity's id is the SHA-256 of the file it came from. Strength totals
//! apply the owner's set edits (see `strength.rs`).

use super::{kind_of, round1, Ctx, TimedValue};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::settings::date_string;
use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/activities", get(list))
        .route("/api/activities/{id}", get(detail))
        .route("/api/activities/{id}/track", get(track))
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ActivityListItem {
    pub id: String,
    /// run, walk, hike, cycle, swim, strength, training or other.
    pub kind: String,
    pub sport_name: Option<String>,
    pub sub_sport_name: Option<String>,
    pub start: String,
    pub date: String,
    pub duration_s: Option<f64>,
    pub distance_m: Option<f64>,
    /// Average pace for runs, walks and hikes.
    pub avg_pace_s_per_km: Option<f64>,
    pub avg_hr_bpm: Option<f64>,
    pub max_hr_bpm: Option<f64>,
    pub calories_kcal: Option<f64>,
    pub ascent_m: Option<f64>,
    /// Strength only (edits applied): working sets, volume, and exercise count.
    pub sets: Option<u32>,
    pub volume_kg: Option<f64>,
    pub exercises: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ActivityList {
    pub sample: bool,
    pub items: Vec<ActivityListItem>,
    /// Pass as `before` to get the next page; `null` at the end.
    pub next_before: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListParams {
    #[serde(rename = "type")]
    kind: Option<String>,
    before: Option<String>,
    limit: Option<usize>,
}

type ListRow = (
    String,
    String,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
);

/// Activity rows with their sessions summed (average HR weighted by timer time).
const ACTIVITY_SQL: &str =
    "SELECT a.sha256, COALESCE(a.activity_key, a.sha256), a.start_ts, a.sport, a.sport_name,
        a.sub_sport, a.sub_sport_name, a.total_timer_s,
        (SELECT SUM(timer_s) FROM activity_sessions s WHERE s.sha256 = a.sha256),
        (SELECT SUM(distance_m) FROM activity_sessions s WHERE s.sha256 = a.sha256),
        (SELECT SUM(avg_hr * timer_s) / SUM(timer_s) FROM activity_sessions s
            WHERE s.sha256 = a.sha256 AND avg_hr IS NOT NULL AND timer_s > 0),
        (SELECT MAX(max_hr) FROM activity_sessions s WHERE s.sha256 = a.sha256),
        (SELECT SUM(calories) FROM activity_sessions s WHERE s.sha256 = a.sha256),
        (SELECT SUM(ascent_m) FROM activity_sessions s WHERE s.sha256 = a.sha256)";

pub fn pace(duration_s: Option<f64>, distance_m: Option<f64>) -> Option<f64> {
    match (duration_s, distance_m) {
        (Some(t), Some(d)) if d > 0.0 && t > 0.0 => Some(round1(t / d * 1000.0)),
        _ => None,
    }
}

async fn item(ctx: &Ctx, r: ListRow) -> Res<Option<ActivityListItem>> {
    let (
        id,
        ident,
        start,
        sport,
        sport_name,
        sub,
        sub_name,
        total_timer,
        timer,
        dist,
        hr,
        max_hr,
        kcal,
        ascent,
    ) = r;
    let Some(start) = start else { return Ok(None) };
    let kind = kind_of(sport, sub);
    let duration = timer.or(total_timer);
    let (mut sets, mut volume, mut exercises) = (None, None, None);
    if kind == "strength" {
        let s = super::strength::load_sets(ctx, &id, &ident).await?;
        sets = Some(s.len() as u32);
        volume = super::strength::volume(&s);
        exercises = Some(super::strength::group(&s).len() as u32);
    }
    Ok(Some(ActivityListItem {
        id,
        kind: kind.to_owned(),
        sport_name,
        sub_sport_name: sub_name,
        start: ctx.ts(start),
        date: date_string(ctx.date(start)),
        duration_s: duration,
        distance_m: dist,
        avg_pace_s_per_km: matches!(kind, "run" | "walk" | "hike")
            .then(|| pace(duration, dist))
            .flatten(),
        avg_hr_bpm: hr.map(round1),
        max_hr_bpm: max_hr,
        calories_kcal: kcal,
        ascent_m: ascent,
        sets,
        volume_kg: volume,
        exercises,
    }))
}

/// Parse `before`: an RFC 3339 instant or a `YYYY-MM-DD` date (start of that local day).
/// An unescaped `+` in a query string arrives as a space, so a cursor pasted into a URL
/// by hand still works.
fn parse_before(ctx: &Ctx, s: &str) -> Res<i64> {
    if let Ok(d) = DateTime::parse_from_rfc3339(&s.replace(' ', "+")) {
        return Ok(d.timestamp());
    }
    crate::settings::parse_date(s)
        .map(|d| ctx.day_bounds(d).0)
        .ok_or_else(|| ApiError::bad_request("before must be RFC 3339 or YYYY-MM-DD"))
}

/// Newest first; `after` and `before` are unix seconds (start time `>= after`, `< before`).
pub async fn list_items(
    ctx: &Ctx,
    kind: Option<&str>,
    after: Option<i64>,
    before: Option<i64>,
    limit: usize,
) -> Res<Vec<ActivityListItem>> {
    let sql = format!(
        "{ACTIVITY_SQL} FROM activities_current a
         WHERE a.user_id = ? AND a.start_ts IS NOT NULL AND a.start_ts >= ? AND a.start_ts < ?
         ORDER BY a.start_ts DESC, a.sha256"
    );
    let rows: Vec<ListRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(ctx.uid)
        .bind(after.unwrap_or(i64::MIN))
        .bind(before.unwrap_or(i64::MAX))
        .fetch_all(&ctx.pool)
        .await?;
    let mut out = Vec::new();
    for r in rows {
        if out.len() >= limit {
            break;
        }
        if let Some(k) = kind {
            if kind_of(r.3, r.5) != k {
                continue;
            }
        }
        out.extend(item(ctx, r).await?);
    }
    Ok(out)
}

async fn list(
    user: DataUser,
    State(state): State<AppState>,
    Query(p): Query<ListParams>,
) -> Res<Json<ActivityList>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let limit = p.limit.unwrap_or(30).clamp(1, 200);
    let before = p
        .before
        .as_deref()
        .map(|b| parse_before(&ctx, b))
        .transpose()?;
    let kind = p.kind.as_deref().filter(|k| *k != "all");
    if let Some(k) = kind {
        if ![
            "run", "walk", "hike", "cycle", "swim", "strength", "training", "other",
        ]
        .contains(&k)
        {
            return Err(ApiError::bad_request("unknown activity type"));
        }
    }
    let mut items = list_items(&ctx, kind, None, before, limit + 1).await?;
    let more = items.len() > limit;
    items.truncate(limit);
    let next_before = if more {
        items.last().map(|i| i.start.clone())
    } else {
        None
    };
    Ok(Json(ActivityList {
        sample: ctx.sample(),
        items,
        next_before,
    }))
}

// ---- detail ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ZoneTime {
    /// 1 to 5.
    pub zone: u32,
    pub duration_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Lap {
    pub index: u32,
    pub start: Option<String>,
    pub distance_m: Option<f64>,
    pub duration_s: Option<f64>,
    pub avg_pace_s_per_km: Option<f64>,
    pub avg_hr_bpm: Option<f64>,
    pub max_hr_bpm: Option<f64>,
    pub ascent_m: Option<f64>,
}

/// Columnar samples, thinned to at most 600 points. All arrays have the same length.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ActivitySeries {
    pub elapsed_s: Vec<f64>,
    pub heart_rate_bpm: Vec<Option<f64>>,
    pub speed_mps: Vec<Option<f64>>,
    pub altitude_m: Vec<Option<f64>>,
    pub distance_m: Vec<Option<f64>>,
    pub cadence: Vec<Option<f64>>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ActivityDetail {
    pub sample: bool,
    #[serde(flatten)]
    #[ts(flatten)]
    pub summary: ActivityListItem,
    pub end: Option<String>,
    pub elapsed_s: Option<f64>,
    pub avg_speed_mps: Option<f64>,
    pub max_speed_mps: Option<f64>,
    pub descent_m: Option<f64>,
    pub avg_cadence: Option<f64>,
    pub aerobic_te: Option<f64>,
    pub anaerobic_te: Option<f64>,
    pub training_load: Option<f64>,
    /// maximal MET x 3.5 from this activity's file; derived, not Garmin's published value.
    pub vo2max_derived: Option<f64>,
    pub has_track: bool,
    pub has_strength: bool,
    pub hr_below_zone1_s: Option<f64>,
    pub hr_zones: Vec<ZoneTime>,
    pub laps: Vec<Lap>,
    pub series: Option<ActivitySeries>,
    /// The personal records this activity holds now, as `kind` values of `/api/records`.
    pub records: Vec<String>,
}

pub fn valid_id(id: &str) -> bool {
    crate::is_sha256_hex(id)
}

async fn row_for(ctx: &Ctx, id: &str) -> Res<ListRow> {
    if !valid_id(id) {
        return Err(ApiError::not_found("no such activity"));
    }
    // The ownership join is the gate for every by-id endpoint: someone else's file is a 404.
    let sql = format!(
        "{ACTIVITY_SQL} FROM activities a
         JOIN file_owners o ON o.sha256 = a.sha256 AND o.user_id = ? WHERE a.sha256 = ?"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(ctx.uid)
        .bind(id)
        .fetch_optional(&ctx.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("no such activity"))
}

pub async fn summary_of(ctx: &Ctx, id: &str) -> Res<ActivityListItem> {
    let row = row_for(ctx, id).await?;
    item(ctx, row)
        .await?
        .ok_or_else(|| ApiError::not_found("activity has no start time"))
}

fn thin<T: Clone>(v: &[T], max: usize) -> Vec<T> {
    let step = v.len().div_ceil(max).max(1);
    v.iter().step_by(step).cloned().collect()
}

type RecRow = (
    i64,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
);

pub async fn records(ctx: &Ctx, id: &str) -> Res<Vec<RecRow>> {
    Ok(sqlx::query_as(
        "SELECT ts, hr, speed, altitude_m, distance_m, cadence FROM activity_records WHERE sha256 = ? ORDER BY idx",
    )
    .bind(id)
    .fetch_all(&ctx.pool)
    .await?)
}

pub fn hr_points(ctx: &Ctx, recs: &[RecRow], max: usize) -> Vec<TimedValue> {
    thin(
        &recs.iter().filter(|r| r.1.is_some()).collect::<Vec<_>>(),
        max,
    )
    .iter()
    .map(|r| TimedValue {
        time: ctx.ts(r.0),
        value: r.1,
    })
    .collect()
}

async fn detail(
    user: DataUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Res<Json<ActivityDetail>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let summary = summary_of(&ctx, &id).await?;
    let p = &ctx.pool;
    type SessAgg = (
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
    );
    let (elapsed, max_speed, descent, cadence, aero, anaerobic, load): SessAgg = sqlx::query_as(
        "SELECT SUM(elapsed_s), MAX(max_speed), SUM(descent_m), AVG(avg_cadence),
                MAX(aerobic_te), MAX(anaerobic_te), SUM(training_load)
         FROM activity_sessions WHERE sha256 = ?",
    )
    .bind(&id)
    .fetch_one(p)
    .await?;
    let avg_speed = match (summary.distance_m, summary.duration_s) {
        (Some(d), Some(t)) if t > 0.0 => Some((d / t * 100.0).round() / 100.0),
        _ => None,
    };
    let met: Option<f64> =
        sqlx::query_scalar("SELECT maximal_met FROM activity_metrics WHERE sha256 = ?")
            .bind(&id)
            .fetch_optional(p)
            .await?
            .flatten();
    let start_ts: Option<i64> =
        sqlx::query_scalar("SELECT start_ts FROM activities WHERE sha256 = ?")
            .bind(&id)
            .fetch_one(p)
            .await?;

    let zones: Option<String> = sqlx::query_scalar(
        "SELECT hr_zone_s FROM activity_time_in_zone WHERE sha256 = ?
         ORDER BY CASE WHEN ref_mesg = 18 THEN 0 ELSE 1 END, idx LIMIT 1",
    )
    .bind(&id)
    .fetch_optional(p)
    .await?;
    let zone_vals: Vec<Option<f64>> = zones
        .and_then(|z| serde_json::from_str(&z).ok())
        .unwrap_or_default();
    // Six entries: below zone 1, then zones 1 to 5. Five: zones 1 to 5.
    let (below, zs) = if zone_vals.len() >= 6 {
        (zone_vals[0], &zone_vals[1..])
    } else {
        (None, &zone_vals[..])
    };
    let hr_zones = zs
        .iter()
        .take(5)
        .enumerate()
        .map(|(i, d)| ZoneTime {
            zone: i as u32 + 1,
            duration_s: *d,
        })
        .collect();

    type LapRow = (
        i64,
        Option<i64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
    );
    let laps: Vec<LapRow> = sqlx::query_as(
        "SELECT idx, start_ts, distance_m, timer_s, avg_hr, max_hr, ascent_m FROM activity_laps
         WHERE sha256 = ? ORDER BY idx",
    )
    .bind(&id)
    .fetch_all(p)
    .await?;
    let kind_paced = matches!(summary.kind.as_str(), "run" | "walk" | "hike");
    let laps = laps
        .into_iter()
        .enumerate()
        .map(|(i, (_, start, dist, t, hr, max_hr, asc))| Lap {
            index: i as u32 + 1,
            start: ctx.opt_ts(start),
            distance_m: dist,
            duration_s: t,
            avg_pace_s_per_km: kind_paced.then(|| pace(t, dist)).flatten(),
            avg_hr_bpm: hr.map(round1),
            max_hr_bpm: max_hr,
            ascent_m: asc,
        })
        .collect();

    let recs = records(&ctx, &id).await?;
    let first = recs.first().map(|r| r.0);
    let thinned = thin(&recs, 600);
    let series = first.map(|t0| ActivitySeries {
        elapsed_s: thinned.iter().map(|r| (r.0 - t0) as f64).collect(),
        heart_rate_bpm: thinned.iter().map(|r| r.1).collect(),
        speed_mps: thinned.iter().map(|r| r.2).collect(),
        altitude_m: thinned.iter().map(|r| r.3).collect(),
        distance_m: thinned.iter().map(|r| r.4).collect(),
        cadence: thinned.iter().map(|r| r.5).collect(),
    });
    let has_track: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM activity_records WHERE sha256 = ? AND lat IS NOT NULL AND lon IS NOT NULL)",
    )
    .bind(&id)
    .fetch_one(p)
    .await?;
    let has_strength: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM strength_sets WHERE sha256 = ?)")
            .bind(&id)
            .fetch_one(p)
            .await?;
    Ok(Json(ActivityDetail {
        sample: ctx.sample(),
        end: ctx.opt_ts(start_ts.zip(elapsed).map(|(s, e)| s + e as i64)),
        elapsed_s: elapsed,
        avg_speed_mps: avg_speed,
        max_speed_mps: max_speed,
        descent_m: descent,
        avg_cadence: cadence.map(round1),
        aerobic_te: aero,
        anaerobic_te: anaerobic,
        training_load: load,
        vo2max_derived: met.map(|m| round1(m * 3.5)),
        has_track,
        has_strength,
        hr_below_zone1_s: below,
        hr_zones,
        laps,
        series,
        records: super::records::held_by(&ctx, &id).await?,
        summary,
    }))
}

// ---- track -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Bounds {
    pub min_lat: f64,
    pub min_lon: f64,
    pub max_lat: f64,
    pub max_lon: f64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TrackResponse {
    pub sample: bool,
    pub id: String,
    pub available: bool,
    /// `[latitude, longitude]` in degrees, thinned to at most 1500 points.
    pub points: Vec<[f64; 2]>,
    pub start: Option<[f64; 2]>,
    pub end: Option<[f64; 2]>,
    pub bounds: Option<Bounds>,
}

async fn track(
    user: DataUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Res<Json<TrackResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    row_for(&ctx, &id).await?;
    let pts: Vec<(f64, f64)> = sqlx::query_as(
        "SELECT lat, lon FROM activity_records WHERE sha256 = ? AND lat IS NOT NULL AND lon IS NOT NULL ORDER BY idx",
    )
    .bind(&id)
    .fetch_all(&ctx.pool)
    .await?;
    let bounds = (!pts.is_empty()).then(|| Bounds {
        min_lat: pts.iter().map(|p| p.0).fold(f64::MAX, f64::min),
        min_lon: pts.iter().map(|p| p.1).fold(f64::MAX, f64::min),
        max_lat: pts.iter().map(|p| p.0).fold(f64::MIN, f64::max),
        max_lon: pts.iter().map(|p| p.1).fold(f64::MIN, f64::max),
    });
    let points: Vec<[f64; 2]> = thin(&pts, 1500).into_iter().map(|p| [p.0, p.1]).collect();
    Ok(Json(TrackResponse {
        sample: ctx.sample(),
        id,
        available: !pts.is_empty(),
        start: pts.first().map(|p| [p.0, p.1]),
        end: pts.last().map(|p| [p.0, p.1]),
        points,
        bounds,
    }))
}
