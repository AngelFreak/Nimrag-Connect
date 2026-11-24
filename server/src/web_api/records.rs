//! `GET /api/records`: the signed-in user's personal records, computed from their own activities
//! and daily step totals.
//!
//! Best efforts (the fastest time over a distance inside one activity) are computed from the
//! activity's records the first time they are asked for and cached per file in
//! `activity_efforts` (see migration 0017). The cache tables are entity tables, so re-deriving a
//! file clears them. The other records come straight from the activity summaries.
//!
//! A record is held by the activity with the best value, the earlier one when two are equal.
//! `previous` is the best value among the activities (or days) before the holder.

use super::{daily, kind_of, Ctx};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::settings::date_string;
use axum::{extract::State, routing::get, Json, Router};
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::{BTreeMap, HashMap, HashSet};
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/records", get(records))
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RecordActivity {
    pub id: String,
    /// run, walk, hike, cycle, swim, strength, training or other.
    pub kind: String,
    pub sport_name: Option<String>,
    pub start: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct PreviousBest {
    pub value: f64,
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct PersonalRecord {
    /// For example `run_5k`, `run_longest`, `steps_day`; the web page names them.
    pub kind: String,
    /// `s` (seconds), `m` (metres) or `steps`.
    pub unit: String,
    /// `null` when nothing counts for this record yet.
    pub value: Option<f64>,
    /// The local date it was set. For `steps_week` the Monday of the week.
    pub date: Option<String>,
    /// The activity it came from; `null` for the step records.
    pub activity: Option<RecordActivity>,
    pub previous: Option<PreviousBest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RecordGroup {
    /// run, walk (walks and hikes), cycle, all (every sport) or steps.
    pub key: String,
    pub records: Vec<PersonalRecord>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct RecordsResponse {
    pub sample: bool,
    pub groups: Vec<RecordGroup>,
}

// ---- best efforts ----------------------------------------------------------------------------

/// A gap between two records longer than this (seconds) is a pause; no effort spans it.
pub const MAX_GAP_S: i64 = 30;

/// Efforts of a run, as `(cache key, distance in metres)`.
pub const RUN_EFFORTS: &[(&str, f64)] = &[
    ("400m", 400.0),
    ("1k", 1000.0),
    ("1mi", 1609.344),
    ("5k", 5000.0),
    ("10k", 10000.0),
    ("half", 21097.5),
    ("marathon", 42195.0),
];

/// Efforts of a ride.
pub const CYCLE_EFFORTS: &[(&str, f64)] = &[("20k", 20000.0), ("40k", 40000.0)];

/// Faster than this average speed (m/s) is a bad distance reading, not an effort.
const MAX_RUN_MPS: f64 = 12.0;
const MAX_CYCLE_MPS: f64 = 25.0;

/// Split `(timestamp, cumulative distance)` samples into stretches without a pause. A sample
/// that does not move time forward is dropped; a distance that goes backwards starts a new
/// stretch (the counter was reset).
fn stretches(points: &[(i64, f64)]) -> Vec<Vec<(f64, f64)>> {
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut cur: Vec<(f64, f64)> = Vec::new();
    let mut last_ts: Option<i64> = None;
    // Times count from the first sample: a unix time is too large to subtract without losing
    // the digits after the point.
    let base = points.first().map_or(0, |p| p.0);
    for &(ts, d) in points {
        if !d.is_finite() {
            continue;
        }
        if let Some(prev) = last_ts {
            if ts <= prev {
                continue;
            }
            let reset = cur.last().is_some_and(|p| d < p.1);
            if ts - prev > MAX_GAP_S || reset {
                out.push(std::mem::take(&mut cur));
            }
        }
        last_ts = Some(ts);
        cur.push(((ts - base) as f64, d));
    }
    out.push(cur);
    out
}

/// The fastest time in seconds to cover `distance_m` within one stretch. A window can end at a
/// sample (its start is then interpolated) or start at one (its end is interpolated); the best
/// window of a piecewise linear track is always one of these, so both passes together are exact.
fn best_in_stretch(c: &[(f64, f64)], distance_m: f64) -> Option<f64> {
    let (first, last) = (c.first()?, c.last()?);
    if last.1 - first.1 < distance_m {
        return None;
    }
    let mut best: Option<f64> = None;
    let mut keep = |t: f64| best = Some(best.map_or(t, |b: f64| b.min(t)));

    // Windows ending at sample j.
    let mut k = 0;
    for &(tj, dj) in c {
        let target = dj - distance_m;
        if target < first.1 {
            continue;
        }
        // The last sample at or before the start distance.
        while k + 1 < c.len() && c[k + 1].1 <= target {
            k += 1;
        }
        let (tk, dk) = c[k];
        let start = if dk == target {
            tk
        } else {
            let (tn, dn) = c[k + 1];
            tk + (target - dk) / (dn - dk) * (tn - tk)
        };
        keep(tj - start);
    }

    // Windows starting at sample i.
    let mut m = 0;
    for &(ti, di) in c {
        let target = di + distance_m;
        if target > last.1 {
            break;
        }
        // The first sample at or beyond the end distance.
        while c[m].1 < target {
            m += 1;
        }
        let (tm, dm) = c[m];
        let end = if dm == target || m == 0 {
            tm
        } else {
            let (tp, dp) = c[m - 1];
            tp + (target - dp) / (dm - dp) * (tm - tp)
        };
        keep(end - ti);
    }
    best
}

/// The fastest time in seconds over `distance_m` anywhere in the samples (`(unix seconds,
/// cumulative metres)` in file order), skipping stretches with a pause of over [`MAX_GAP_S`]
/// seconds. `None` when no pause-free stretch covers the distance.
pub fn best_effort(points: &[(i64, f64)], distance_m: f64) -> Option<f64> {
    stretches(points)
        .iter()
        .filter_map(|s| best_in_stretch(s, distance_m))
        .reduce(f64::min)
}

/// The efforts of one activity of kind `kind`: `(key, seconds)`.
pub fn efforts_of_points(kind: &str, points: &[(i64, f64)]) -> Vec<(&'static str, f64)> {
    let (set, max_mps) = match kind {
        "run" => (RUN_EFFORTS, MAX_RUN_MPS),
        "cycle" => (CYCLE_EFFORTS, MAX_CYCLE_MPS),
        _ => return Vec::new(),
    };
    set.iter()
        .filter_map(|&(key, d)| {
            let t = best_effort(points, d)?;
            (t > 0.0 && d / t <= max_mps).then_some((key, t))
        })
        .collect()
}

/// Cached efforts of the user's runs and rides, computing (and storing) the ones missing.
async fn load_efforts(
    ctx: &Ctx,
    acts: &[Act],
) -> Res<HashMap<String, BTreeMap<&'static str, f64>>> {
    let pool: &SqlitePool = &ctx.pool;
    let done: HashSet<String> = sqlx::query_scalar(
        "SELECT d.sha256 FROM activity_efforts_done d
         JOIN activities_current a ON a.sha256 = d.sha256 WHERE a.user_id = ?",
    )
    .bind(ctx.uid)
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();
    let rows: Vec<(String, String, f64)> = sqlx::query_as(
        "SELECT e.sha256, e.effort, e.seconds FROM activity_efforts e
         JOIN activities_current a ON a.sha256 = e.sha256 WHERE a.user_id = ?",
    )
    .bind(ctx.uid)
    .fetch_all(pool)
    .await?;
    let mut out: HashMap<String, BTreeMap<&'static str, f64>> = HashMap::new();
    for (sha, effort, secs) in rows {
        if let Some(key) = RUN_EFFORTS
            .iter()
            .chain(CYCLE_EFFORTS)
            .map(|e| e.0)
            .find(|k| *k == effort)
        {
            out.entry(sha).or_default().insert(key, secs);
        }
    }
    for a in acts {
        if !matches!(a.kind, "run" | "cycle") || done.contains(&a.id) {
            continue;
        }
        let points: Vec<(i64, f64)> = sqlx::query_as(
            "SELECT ts, distance_m FROM activity_records
             WHERE sha256 = ? AND distance_m IS NOT NULL ORDER BY idx",
        )
        .bind(&a.id)
        .fetch_all(pool)
        .await?;
        let found = efforts_of_points(a.kind, &points);
        let mut tx = pool.begin().await?;
        for (key, secs) in &found {
            sqlx::query(
                "INSERT OR REPLACE INTO activity_efforts (sha256, effort, seconds) VALUES (?, ?, ?)",
            )
            .bind(&a.id)
            .bind(*key)
            .bind(*secs)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("INSERT OR IGNORE INTO activity_efforts_done (sha256) VALUES (?)")
            .bind(&a.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        out.insert(a.id.clone(), found.into_iter().collect());
    }
    Ok(out)
}

// ---- candidates and picking ------------------------------------------------------------------

/// One activity with the figures the records read.
struct Act {
    id: String,
    kind: &'static str,
    sport_name: Option<String>,
    start: i64,
    duration_s: Option<f64>,
    distance_m: Option<f64>,
    ascent_m: Option<f64>,
}

async fn load_acts(ctx: &Ctx) -> Res<Vec<Act>> {
    type Row = (
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT a.sha256, a.start_ts, a.sport, a.sport_name, a.sub_sport, a.total_timer_s,
            (SELECT SUM(timer_s) FROM activity_sessions s WHERE s.sha256 = a.sha256),
            (SELECT SUM(distance_m) FROM activity_sessions s WHERE s.sha256 = a.sha256),
            (SELECT SUM(ascent_m) FROM activity_sessions s WHERE s.sha256 = a.sha256)
         FROM activities_current a
         WHERE a.user_id = ? AND a.start_ts IS NOT NULL
         ORDER BY a.start_ts, a.sha256",
    )
    .bind(ctx.uid)
    .fetch_all(&ctx.pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, start, sport, sport_name, sub, total, timer, dist, ascent)| Act {
                id,
                kind: kind_of(sport, sub),
                sport_name,
                start,
                duration_s: timer.or(total),
                distance_m: dist,
                ascent_m: ascent,
            },
        )
        .collect())
}

/// A value and the index (into the activities, days or weeks it was picked from) that holds it.
struct Best {
    value: f64,
    at: usize,
}

/// The best entry and the best among those before it. `lower` = a smaller value is better.
/// Entries must be sorted oldest first; only a strictly better value replaces the holder.
fn pick(values: &[(usize, f64)], lower: bool) -> (Option<Best>, Option<Best>) {
    let better = |a: f64, b: f64| if lower { a < b } else { a > b };
    let mut best: Option<Best> = None;
    let mut prev_of_best: Option<Best> = None;
    for &(at, value) in values {
        if !value.is_finite() || value <= 0.0 {
            continue;
        }
        match &best {
            Some(b) if !better(value, b.value) => {}
            _ => {
                prev_of_best = best.take();
                best = Some(Best { value, at });
            }
        }
    }
    (best, prev_of_best)
}

fn record(
    ctx: &Ctx,
    acts: &[Act],
    kind: &str,
    unit: &str,
    lower: bool,
    values: Vec<(usize, f64)>,
) -> PersonalRecord {
    let (best, prev) = pick(&values, lower);
    let date = |i: usize| date_string(ctx.date(acts[i].start));
    PersonalRecord {
        kind: kind.to_owned(),
        unit: unit.to_owned(),
        value: best.as_ref().map(|b| b.value),
        date: best.as_ref().map(|b| date(b.at)),
        activity: best.as_ref().map(|b| {
            let a = &acts[b.at];
            RecordActivity {
                id: a.id.clone(),
                kind: a.kind.to_owned(),
                sport_name: a.sport_name.clone(),
                start: ctx.ts(a.start),
            }
        }),
        previous: prev.map(|p| PreviousBest {
            value: p.value,
            date: date(p.at),
        }),
    }
}

/// Every activity of one of `kinds`, as `(index, value from f)`.
fn column(acts: &[Act], kinds: &[&str], f: impl Fn(&Act) -> Option<f64>) -> Vec<(usize, f64)> {
    acts.iter()
        .enumerate()
        .filter(|(_, a)| kinds.contains(&a.kind))
        .filter_map(|(i, a)| f(a).map(|v| (i, v)))
        .collect()
}

fn effort_column(
    acts: &[Act],
    kind: &str,
    key: &str,
    efforts: &HashMap<String, BTreeMap<&'static str, f64>>,
) -> Vec<(usize, f64)> {
    column(acts, &[kind], |a| {
        efforts.get(&a.id).and_then(|e| e.get(key)).copied()
    })
}

// ---- steps -----------------------------------------------------------------------------------

/// Steps per local day, the same way the day pages read them: the watch's counters (largest
/// reading of the day per activity type, summed), else Garmin's whole-day total, else Connect's
/// daily summary.
async fn steps_by_day(ctx: &Ctx) -> Res<BTreeMap<NaiveDate, f64>> {
    // 15 minutes never straddle a local midnight, so the day of a bucket is the day of its start;
    // a counter reading belongs to the day of `ts - 1` (see `daily::counter_date`).
    let rows: Vec<(i64, i64, f64)> = sqlx::query_as(
        "SELECT activity_type, (ts - 1) / 900, MAX(steps) FROM mon_activity
         WHERE user_id = ? AND steps IS NOT NULL GROUP BY activity_type, (ts - 1) / 900",
    )
    .bind(ctx.uid)
    .fetch_all(&ctx.pool)
    .await?;
    let mut per_type: HashMap<(NaiveDate, i64), f64> = HashMap::new();
    for (ty, bucket, steps) in rows {
        let e = per_type
            .entry((ctx.date(bucket * 900), ty))
            .or_insert(steps);
        *e = e.max(steps);
    }
    let mut days: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    for ((d, _), steps) in per_type {
        *days.entry(d).or_insert(0.0) += steps;
    }
    let totals: Vec<(i64, f64)> = sqlx::query_as(
        "SELECT ts, steps FROM daily_totals WHERE user_id = ? AND steps IS NOT NULL",
    )
    .bind(ctx.uid)
    .fetch_all(&ctx.pool)
    .await?;
    for (ts, steps) in totals {
        days.entry(ctx.date(ts)).or_insert(steps);
    }
    let from = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap_or_default();
    for (d, c) in daily::connect_days(ctx, from, ctx.today() + Duration::days(1)).await? {
        if let Some(s) = c.steps {
            days.entry(d).or_insert(s);
        }
    }
    Ok(days)
}

fn step_record(kind: &str, days: &[(NaiveDate, f64)]) -> PersonalRecord {
    let values: Vec<(usize, f64)> = days.iter().enumerate().map(|(i, d)| (i, d.1)).collect();
    let (best, prev) = pick(&values, false);
    PersonalRecord {
        kind: kind.to_owned(),
        unit: "steps".to_owned(),
        value: best.as_ref().map(|b| b.value),
        date: best.as_ref().map(|b| date_string(days[b.at].0)),
        activity: None,
        previous: prev.map(|p| PreviousBest {
            value: p.value,
            date: date_string(days[p.at].0),
        }),
    }
}

async fn steps_group(ctx: &Ctx) -> Res<RecordGroup> {
    let by_day = steps_by_day(ctx).await?;
    let days: Vec<(NaiveDate, f64)> = by_day.iter().map(|(d, s)| (*d, *s)).collect();
    let mut weeks: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    for (d, s) in &days {
        let monday = *d - Duration::days(i64::from(d.weekday().num_days_from_monday()));
        *weeks.entry(monday).or_insert(0.0) += s;
    }
    let weeks: Vec<(NaiveDate, f64)> = weeks.into_iter().collect();
    Ok(RecordGroup {
        key: "steps".into(),
        records: vec![
            step_record("steps_day", &days),
            step_record("steps_week", &weeks),
        ],
    })
}

// ---- assembling ------------------------------------------------------------------------------

/// All record groups. `with_steps` is off for the activity page, which only needs its own.
pub async fn compute(ctx: &Ctx, with_steps: bool) -> Res<Vec<RecordGroup>> {
    let acts = load_acts(ctx).await?;
    let efforts = load_efforts(ctx, &acts).await?;
    let dist = |a: &Act| a.distance_m;
    let ascent = |a: &Act| a.ascent_m;
    let rec = |kind: &str, unit: &str, lower: bool, v: Vec<(usize, f64)>| {
        record(ctx, &acts, kind, unit, lower, v)
    };

    let mut run = Vec::new();
    for (key, _) in RUN_EFFORTS {
        run.push(rec(
            &format!("run_{key}"),
            "s",
            true,
            effort_column(&acts, "run", key, &efforts),
        ));
    }
    run.push(rec(
        "run_longest",
        "m",
        false,
        column(&acts, &["run"], dist),
    ));
    run.push(rec(
        "run_ascent",
        "m",
        false,
        column(&acts, &["run"], ascent),
    ));

    let walk_kinds = ["walk", "hike"];
    let walk = vec![
        rec("walk_longest", "m", false, column(&acts, &walk_kinds, dist)),
        rec(
            "walk_ascent",
            "m",
            false,
            column(&acts, &walk_kinds, ascent),
        ),
    ];

    let mut cycle = vec![rec(
        "cycle_longest",
        "m",
        false,
        column(&acts, &["cycle"], dist),
    )];
    for (key, _) in CYCLE_EFFORTS {
        cycle.push(rec(
            &format!("cycle_{key}"),
            "s",
            true,
            effort_column(&acts, "cycle", key, &efforts),
        ));
    }
    cycle.push(rec(
        "cycle_ascent",
        "m",
        false,
        column(&acts, &["cycle"], ascent),
    ));

    let any: Vec<&str> = vec![
        "run", "walk", "hike", "cycle", "swim", "strength", "training", "other",
    ];
    let all = vec![rec(
        "longest_time",
        "s",
        false,
        column(&acts, &any, |a| a.duration_s),
    )];

    let mut groups = vec![
        RecordGroup {
            key: "run".into(),
            records: run,
        },
        RecordGroup {
            key: "walk".into(),
            records: walk,
        },
        RecordGroup {
            key: "cycle".into(),
            records: cycle,
        },
        RecordGroup {
            key: "all".into(),
            records: all,
        },
    ];
    if with_steps {
        groups.push(steps_group(ctx).await?);
    }
    Ok(groups)
}

/// The kinds of the records this activity holds now.
pub async fn held_by(ctx: &Ctx, id: &str) -> Res<Vec<String>> {
    Ok(compute(ctx, false)
        .await?
        .into_iter()
        .flat_map(|g| g.records)
        .filter(|r| r.activity.as_ref().is_some_and(|a| a.id == id))
        .map(|r| r.kind)
        .collect())
}

async fn records(user: DataUser, State(state): State<AppState>) -> Res<Json<RecordsResponse>> {
    let ctx = Ctx::new(&state, user.id).await?;
    Ok(Json(RecordsResponse {
        sample: ctx.sample(),
        groups: compute(&ctx, true).await?,
    }))
}
