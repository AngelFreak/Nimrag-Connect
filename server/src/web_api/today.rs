//! `GET /api/today`: everything the Today page shows in one response: the "In
//! focus" carousel (energy, last night's sleep, HRV), every "At a glance" card,
//! and the "Yesterday" and "Last 7 days" lists. Anything without data is `null`.

use super::activities::{list_items, ActivityListItem};
use super::daily::{
    connect_day, day_stats, energy_block, heart_rate_block, hr_window, hrv_block, intensity_total,
    night_detail, session_last_night, stress_block, EnergyBlock, HeartRateBlock, HrWindow,
    HrvBlock, SleepNight, StressBlock,
};
use super::days::{intensity_week, IntensityWeek};
use super::system::{last_data_ts, last_upload};
use super::training::{
    recovery, status_and_load, vo2max, RecoveryCard, TrainingLoadCard, TrainingReadinessCard,
    TrainingStatusCard, Vo2Card,
};
use super::{mean, round1, Ctx};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::settings::date_string;
use axum::{extract::State, routing::get, Json, Router};
use chrono::Duration;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/today", get(today))
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct StepsCard {
    pub steps: Option<f64>,
    pub goal: u32,
    pub percent_of_goal: Option<f64>,
    pub distance_m: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct RespirationCard {
    pub latest_brpm: Option<f64>,
    pub latest_at: Option<String>,
    /// Mean of today's readings (without any, Garmin Connect's average for the day if imported).
    pub day_avg_brpm: Option<f64>,
    /// Mean during last night's sleep.
    pub sleep_avg_brpm: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct Spo2Card {
    pub latest_percent: Option<f64>,
    pub latest_at: Option<String>,
    pub sleep_avg_percent: Option<f64>,
    pub sleep_low_percent: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct WeightCard {
    pub weight_kg: Option<f64>,
    pub measured_at: Option<String>,
    /// Latest minus the first weigh-in of the last 30 days; `null` with fewer than two.
    pub change_30d_kg: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct WeekTraining {
    pub sessions: u32,
    pub duration_s: f64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct YesterdaySummary {
    pub date: String,
    pub energy_charged: Option<f64>,
    pub energy_drained: Option<f64>,
    pub steps: Option<f64>,
    /// The night that ended yesterday morning (yesterday's sleep, as on the Day view).
    pub sleep_score: Option<f64>,
    pub sleep_duration_s: Option<f64>,
    pub resting_hr_bpm: Option<f64>,
    pub hr_high_bpm: Option<f64>,
    pub stress_avg: Option<f64>,
    /// The watch's 7-day HRV average as of yesterday.
    pub hrv_7d_avg_ms: Option<f64>,
    /// Moderate + 2 x vigorous minutes of the day.
    pub intensity_minutes: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct KindCount {
    pub kind: String,
    pub count: u32,
}

/// The 7 complete days before today.
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct Last7Days {
    pub from: String,
    pub to: String,
    pub activity_count: u32,
    pub activities_by_kind: Vec<KindCount>,
    pub avg_steps: Option<f64>,
    pub avg_sleep_score: Option<f64>,
    pub avg_sleep_duration_s: Option<f64>,
    pub avg_resting_hr_bpm: Option<f64>,
    pub avg_stress: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct Today {
    pub sample: bool,
    pub date: String,
    pub generated_at: String,
    pub timezone: String,
    /// When the newest file arrived on the server.
    pub last_sync_at: Option<String>,
    /// Time of the newest watch sample we hold.
    pub last_data_at: Option<String>,
    // In focus
    pub energy: EnergyBlock,
    /// Last night (ended within the last 30 hours); `null` if the watch recorded none.
    pub sleep: Option<SleepNight>,
    pub hrv: HrvBlock,
    // At a glance
    /// Resting heart rate and today's low and high (midnight to now).
    pub heart_rate: HeartRateBlock,
    /// The last 24 hours (rolling) for the heart-rate chart.
    pub heart_rate_24h: HrWindow,
    pub stress: StressBlock,
    pub steps: StepsCard,
    pub intensity_minutes: IntensityWeek,
    pub training_status: TrainingStatusCard,
    /// No data source yet: always `null`.
    pub training_readiness: Option<TrainingReadinessCard>,
    pub training_load: TrainingLoadCard,
    pub respiration: RespirationCard,
    pub spo2: Spo2Card,
    pub recovery_time: RecoveryCard,
    pub vo2max: Vo2Card,
    pub weight: WeightCard,
    // Lists
    pub last_activity: Option<ActivityListItem>,
    pub training_this_week: WeekTraining,
    pub yesterday: YesterdaySummary,
    pub last_7_days: Last7Days,
}

async fn latest(ctx: &Ctx, sql: &'static str) -> Res<Option<(i64, f64)>> {
    Ok(sqlx::query_as(sql)
        .bind(ctx.uid)
        .bind(ctx.now)
        .fetch_optional(&ctx.pool)
        .await?)
}

async fn today(user: DataUser, State(state): State<AppState>) -> Res<Json<Today>> {
    let ctx = Ctx::new(&state, user.id).await?;
    let d = ctx.today();
    let (day_from, day_to) = ctx.day_bounds(d);
    let yesterday = d - Duration::days(1);
    let stats = day_stats(&ctx, d - Duration::days(7), d).await?;
    let me = stats.get(&d).cloned().unwrap_or_default();

    let night = match session_last_night(&ctx).await? {
        Some(s) => Some(night_detail(&ctx, &s).await?),
        None => None,
    };
    let (training_status, training_load) = status_and_load(&ctx).await?;

    let resp = latest(
        &ctx,
        "SELECT ts, brpm FROM respiration WHERE user_id = ? AND brpm IS NOT NULL AND ts <= ? ORDER BY ts DESC LIMIT 1",
    )
    .await?;
    let day_resp: Option<f64> = sqlx::query_scalar(
        "SELECT AVG(brpm) FROM respiration WHERE user_id = ? AND ts >= ? AND ts < ?",
    )
    .bind(ctx.uid)
    .bind(day_from)
    .bind(day_to)
    .fetch_one(&ctx.pool)
    .await?;
    let spo2 = latest(
        &ctx,
        "SELECT ts, percent FROM spo2 WHERE user_id = ? AND percent IS NOT NULL AND ts <= ? ORDER BY ts DESC LIMIT 1",
    )
    .await?;

    let weights: Vec<(i64, f64)> = sqlx::query_as(
        "SELECT measured_at, weight_kg FROM body_measurements WHERE user_id = ? AND weight_kg IS NOT NULL AND measured_at <= ?
         ORDER BY measured_at",
    )
    .bind(ctx.uid)
    .bind(ctx.now)
    .fetch_all(&ctx.pool)
    .await?;
    let recent: Vec<&(i64, f64)> = weights
        .iter()
        .filter(|w| w.0 >= ctx.now - 30 * 86_400)
        .collect();

    let last_activity = list_items(&ctx, None, None, Some(ctx.now + 1), 1)
        .await?
        .into_iter()
        .next();
    let week_from = ctx.day_bounds(super::daily::week_start(d)).0;
    let week_acts = list_items(&ctx, None, Some(week_from), Some(day_to), 200).await?;

    // Yesterday and the 7 complete days before today.
    let y = stats.get(&yesterday).cloned().unwrap_or_default();
    let y_night = super::daily::nights(&ctx, yesterday, yesterday)
        .await?
        .into_iter()
        .next();
    let y_hrv = hrv_block(&ctx, yesterday).await?;
    let from7 = d - Duration::days(7);
    let week7: Vec<&super::daily::DayStats> = stats.range(from7..d).map(|(_, s)| s).collect();
    let nights7 = super::daily::nights(&ctx, from7, yesterday).await?;
    let acts7 = list_items(
        &ctx,
        None,
        Some(ctx.day_bounds(from7).0),
        Some(day_from),
        500,
    )
    .await?;
    let mut by_kind: BTreeMap<String, u32> = BTreeMap::new();
    for a in &acts7 {
        *by_kind.entry(a.kind.clone()).or_default() += 1;
    }

    let goal = f64::from(ctx.prefs.step_goal);
    Ok(Json(Today {
        sample: ctx.sample(),
        date: date_string(d),
        generated_at: ctx.ts(ctx.now),
        timezone: ctx.tz().name().to_owned(),
        last_sync_at: last_upload(&ctx).await?,
        last_data_at: ctx.opt_ts(last_data_ts(&ctx).await?),
        energy: energy_block(&ctx, d).await?,
        hrv: hrv_block(&ctx, d).await?,
        heart_rate: heart_rate_block(&ctx, d).await?,
        heart_rate_24h: hr_window(&ctx, ctx.now - 86_400, ctx.now + 1).await?,
        stress: stress_block(&ctx, day_from, day_to).await?,
        steps: StepsCard {
            steps: me.steps,
            goal: ctx.prefs.step_goal,
            percent_of_goal: me.steps.map(|s| (s / goal * 1000.0).round() / 10.0),
            distance_m: me.distance_m,
        },
        intensity_minutes: intensity_week(&ctx, d).await?,
        training_status,
        training_readiness: None,
        training_load,
        respiration: RespirationCard {
            latest_brpm: resp.map(|r| r.1),
            latest_at: ctx.opt_ts(resp.map(|r| r.0)),
            // The watch's mean of today's readings, else Connect's day average.
            day_avg_brpm: day_resp.map(round1).or(connect_day(&ctx, d)
                .await?
                .and_then(|c| c.resp_avg.map(round1))),
            sleep_avg_brpm: night.as_ref().and_then(|n| n.breathing_brpm),
        },
        spo2: Spo2Card {
            latest_percent: spo2.map(|r| r.1),
            latest_at: ctx.opt_ts(spo2.map(|r| r.0)),
            sleep_avg_percent: night.as_ref().and_then(|n| n.spo2_avg_percent),
            sleep_low_percent: night.as_ref().and_then(|n| n.spo2_low_percent),
        },
        recovery_time: recovery(&ctx).await?,
        vo2max: vo2max(&ctx).await?,
        weight: WeightCard {
            weight_kg: weights.last().map(|w| w.1),
            measured_at: ctx.opt_ts(weights.last().map(|w| w.0)),
            change_30d_kg: match (recent.first(), recent.last()) {
                (Some(a), Some(b)) if recent.len() > 1 => Some(round1(b.1 - a.1)),
                _ => None,
            },
        },
        last_activity,
        training_this_week: WeekTraining {
            sessions: week_acts.len() as u32,
            duration_s: week_acts.iter().filter_map(|a| a.duration_s).sum(),
        },
        yesterday: YesterdaySummary {
            date: date_string(yesterday),
            energy_charged: y.energy.charged,
            energy_drained: y.energy.drained,
            steps: y.steps,
            sleep_score: y_night.as_ref().and_then(|n| n.score),
            sleep_duration_s: y_night.as_ref().map(|n| n.duration_s),
            resting_hr_bpm: y.resting_hr,
            hr_high_bpm: y.hr_high,
            stress_avg: y.stress_avg,
            hrv_7d_avg_ms: y_hrv.weekly_avg_ms,
            intensity_minutes: intensity_total(y.moderate_min, y.vigorous_min),
        },
        last_7_days: Last7Days {
            from: date_string(from7),
            to: date_string(yesterday),
            activity_count: acts7.len() as u32,
            activities_by_kind: by_kind
                .into_iter()
                .map(|(kind, count)| KindCount { kind, count })
                .collect(),
            avg_steps: mean(week7.iter().filter_map(|s| s.steps)).map(f64::round),
            avg_sleep_score: mean(nights7.iter().filter_map(|n| n.score)),
            avg_sleep_duration_s: mean(nights7.iter().map(|n| n.duration_s)).map(f64::round),
            avg_resting_hr_bpm: mean(week7.iter().filter_map(|s| s.resting_hr)),
            avg_stress: mean(week7.iter().filter_map(|s| s.stress_avg)),
        },
        sleep: night,
    }))
}
