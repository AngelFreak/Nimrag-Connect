//! `GET/PATCH /api/settings`: time zone, goals and the Today page's card layout.

use super::{ApiJson, Ctx};
use crate::api::{ApiError, AppState};
use crate::auth::DataUser;
use crate::settings::{self, HomeCards};
use axum::{extract::State, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/settings", get(read).patch(patch))
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct SettingsResponse {
    /// IANA zone name; days are local days in this zone.
    pub timezone: String,
    pub step_goal: u32,
    pub intensity_goal_min: u32,
    /// Sleep need in minutes a night (360 to 600 in steps of 15); the sleep page rates a night against it.
    pub sleep_need_min: u32,
    pub home_cards: HomeCards,
    pub sample: bool,
}

/// Every field optional: only the ones present change.
#[derive(Debug, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SettingsPatch {
    #[serde(default)]
    #[ts(optional)]
    pub timezone: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub step_goal: Option<u32>,
    #[serde(default)]
    #[ts(optional)]
    pub intensity_goal_min: Option<u32>,
    #[serde(default)]
    #[ts(optional)]
    pub sleep_need_min: Option<u32>,
    #[serde(default)]
    #[ts(optional)]
    pub home_cards: Option<HomeCards>,
}

async fn current(state: &AppState, uid: i64) -> Result<SettingsResponse, ApiError> {
    let ctx = Ctx::new(state, uid).await?;
    Ok(SettingsResponse {
        timezone: ctx.tz().name().to_owned(),
        step_goal: ctx.prefs.step_goal,
        intensity_goal_min: ctx.prefs.intensity_goal_min,
        sleep_need_min: ctx.prefs.sleep_need_min,
        home_cards: ctx.prefs.home_cards.clone(),
        sample: ctx.sample(),
    })
}

async fn read(
    user: DataUser,
    State(state): State<AppState>,
) -> Result<Json<SettingsResponse>, ApiError> {
    Ok(Json(current(&state, user.id).await?))
}

async fn patch(
    user: DataUser,
    State(state): State<AppState>,
    ApiJson(p): ApiJson<SettingsPatch>,
) -> Result<Json<SettingsResponse>, ApiError> {
    // Validate everything before writing anything.
    let tz = p
        .timezone
        .as_deref()
        .map(|tz| {
            tz.parse::<chrono_tz::Tz>().map_err(|_| {
                ApiError::bad_request("timezone must be an IANA name such as Europe/Copenhagen")
            })
        })
        .transpose()?;
    if p.step_goal
        .is_some_and(|g| !settings::STEP_GOAL_RANGE.contains(&g))
    {
        return Err(ApiError::bad_request("step_goal must be 1 to 200000"));
    }
    if p.intensity_goal_min
        .is_some_and(|g| !settings::INTENSITY_GOAL_RANGE.contains(&g))
    {
        return Err(ApiError::bad_request(
            "intensity_goal_min must be 1 to 10080",
        ));
    }
    if p.sleep_need_min
        .is_some_and(|g| !settings::sleep_need_ok(g))
    {
        return Err(ApiError::bad_request(
            "sleep_need_min must be 360 to 600, in steps of 15",
        ));
    }
    let cards = p
        .home_cards
        .map(|c| c.normalised().map_err(ApiError::bad_request))
        .transpose()?;
    let pool = &state.pool;
    if let Some(tz) = tz {
        settings::set_user(pool, user.id, "timezone", tz.name()).await?;
    }
    if let Some(g) = p.step_goal {
        settings::set_user(pool, user.id, "step_goal", &g.to_string()).await?;
    }
    if let Some(g) = p.intensity_goal_min {
        settings::set_user(pool, user.id, "intensity_goal_min", &g.to_string()).await?;
    }
    if let Some(g) = p.sleep_need_min {
        settings::set_user(pool, user.id, "sleep_need_min", &g.to_string()).await?;
    }
    if let Some(c) = cards {
        settings::set_user(
            pool,
            user.id,
            "home_cards",
            &serde_json::to_string(&c).map_err(anyhow::Error::from)?,
        )
        .await?;
    }
    Ok(Json(current(&state, user.id).await?))
}
