//! The admin area: accounts, and the data nobody owns yet. Every route needs an admin session
//! (users get 403).
//!
//! * Users: list, create (the server makes a temporary password, shown once, which must be
//!   replaced at the first login), reset the password (a new temporary one), reset 2FA (the next
//!   login enrols again), disable and enable, delete. The last active admin can be neither
//!   disabled nor deleted. Disabling, resetting a password and resetting 2FA all sign the user
//!   out and end their trusted devices' unlock credentials.
//! * Delete: refused with 409 while the user has data (tokens, files, body entries, set edits,
//!   a Garmin account) unless `?confirm=true`. Even then the original files are never deleted:
//!   they, and the user's device tokens, go back to "unassigned" (the Unassigned card), and only
//!   the user's own records (preferences, body entries, set edits, Garmin Forward) are removed.
//! * Unassigned: device tokens without a user (every token that existed before accounts) and the
//!   files they uploaded. Assigning moves them to a user and rebuilds the affected derived rows.

use super::account::{token_info, TokenInfo};
use super::ApiJson;
use crate::api::{ApiError, AppState};
use crate::auth::AdminUser;
use crate::users::{self, Role, User};
use crate::{settings, tokens};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use ts_rs::TS;

type Res<T> = Result<T, ApiError>;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/users", get(list_users).post(create_user))
        .route("/api/admin/users/{id}", delete(delete_user))
        .route("/api/admin/users/{id}/reset-password", post(reset_password))
        .route("/api/admin/users/{id}/reset-2fa", post(reset_2fa))
        .route("/api/admin/users/{id}/disable", post(disable))
        .route("/api/admin/users/{id}/enable", post(enable))
        .route("/api/admin/unassigned", get(unassigned))
        .route("/api/admin/unassigned/assign", post(assign))
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct UserInfo {
    #[ts(type = "number")]
    pub id: i64,
    pub username: String,
    pub role: Role,
    pub totp_enabled: bool,
    pub must_change_password: bool,
    pub disabled: bool,
    pub created_at: String,
    /// Active device tokens.
    pub active_tokens: u32,
    /// Files that belong to the user.
    pub files: u32,
    /// Whether deleting the user needs `confirm` (tokens, files, body entries, set edits or a
    /// Garmin account exist).
    pub has_data: bool,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct UserList {
    pub users: Vec<UserInfo>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct NewUser {
    pub username: String,
    /// `user` (default) or `admin`.
    #[serde(default)]
    #[ts(optional)]
    pub role: Option<Role>,
}

/// The temporary password is returned this once and never stored in the clear.
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct NewUserResponse {
    pub user: UserInfo,
    pub temporary_password: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TemporaryPassword {
    pub temporary_password: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct UnassignedToken {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    /// Files this token uploaded that nobody owns yet.
    pub files: u32,
}

/// Records from before accounts (user 0) that the admin can hand to a user.
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct LegacyData {
    /// Preferences (time zone, goals, card layout) exist.
    pub settings: bool,
    pub body_entries: u32,
    pub strength_edits: u32,
    /// A Garmin Forward account exists.
    pub garmin_account: bool,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct UnassignedResponse {
    /// Active device tokens without a user.
    pub tokens: Vec<UnassignedToken>,
    /// Unowned files no listed token uploaded (CLI imports, revoked or unknown devices).
    pub unattributed_files: u32,
    pub legacy: LegacyData,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AssignRequest {
    #[ts(type = "number[]")]
    pub token_ids: Vec<i64>,
    #[ts(type = "number")]
    pub user_id: i64,
    /// Also take the unowned files that no listed token uploaded.
    #[serde(default)]
    #[ts(optional)]
    pub include_unattributed: Option<bool>,
    /// Also hand the pre-accounts records (preferences, body entries, set edits, Garmin Forward)
    /// to the user. Refused when the user already has some of their own.
    #[serde(default)]
    #[ts(optional)]
    pub adopt_legacy: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct AssignResponse {
    pub tokens_moved: u32,
    pub files_moved: u32,
    pub legacy_adopted: bool,
}

// ---- users ----------------------------------------------------------------------

struct Counts {
    active_tokens: i64,
    files: i64,
    body: i64,
    edits: i64,
    garmin: bool,
}

async fn counts(pool: &SqlitePool, uid: i64) -> Res<Counts> {
    let one = |sql: &'static str| async move {
        let n: i64 = sqlx::query_scalar(sql).bind(uid).fetch_one(pool).await?;
        Ok::<i64, ApiError>(n)
    };
    Ok(Counts {
        active_tokens: one("SELECT COUNT(*) FROM tokens WHERE user_id = ? AND revoked_at IS NULL")
            .await?,
        files: one("SELECT COUNT(*) FROM file_owners WHERE user_id = ?").await?,
        body: one("SELECT COUNT(*) FROM body_measurements WHERE user_id = ?").await?,
        edits: one("SELECT COUNT(*) FROM strength_set_overrides WHERE user_id = ?").await?,
        garmin: one("SELECT COUNT(*) FROM garmin_account WHERE user_id = ?").await? > 0,
    })
}

async fn user_info(state: &AppState, tz: chrono_tz::Tz, u: &User) -> Res<UserInfo> {
    let c = counts(&state.pool, u.id).await?;
    Ok(UserInfo {
        id: u.id,
        username: u.username.clone(),
        role: u.role,
        totp_enabled: u.totp_enabled,
        must_change_password: u.must_change_password,
        disabled: u.disabled_at.is_some(),
        created_at: settings::rfc3339(tz, u.created_at),
        active_tokens: c.active_tokens as u32,
        files: c.files as u32,
        has_data: c.active_tokens + c.files + c.body + c.edits > 0 || c.garmin,
    })
}

async fn admin_tz(state: &AppState, admin: &AdminUser) -> Res<chrono_tz::Tz> {
    Ok(settings::load(&state.pool, admin.id).await?.tz)
}

async fn list_users(admin: AdminUser, State(state): State<AppState>) -> Res<Json<UserList>> {
    let tz = admin_tz(&state, &admin).await?;
    let mut out = Vec::new();
    for u in users::list(&state.pool).await? {
        out.push(user_info(&state, tz, &u).await?);
    }
    Ok(Json(UserList { users: out }))
}

async fn create_user(
    admin: AdminUser,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<NewUser>,
) -> Res<(StatusCode, Json<NewUserResponse>)> {
    let username = users::normalise_username(&body.username).map_err(ApiError::bad_request)?;
    let temp = users::temporary_password()?;
    let hash = super::account::hash_under_gate(&state, temp.clone()).await?;
    let role = body.role.unwrap_or(Role::User);
    let Some(id) = users::create(&state.pool, &username, &hash, role, true, state.now()).await?
    else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "that username is taken",
        ));
    };
    tracing::info!(user_id = id, role = role.as_str(), "user created");
    let user = users::by_id(&state.pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("user vanished"))?;
    let tz = admin_tz(&state, &admin).await?;
    Ok((
        StatusCode::CREATED,
        Json(NewUserResponse {
            user: user_info(&state, tz, &user).await?,
            temporary_password: temp,
        }),
    ))
}

async fn target(state: &AppState, id: i64) -> Res<User> {
    users::by_id(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("no such user"))
}

fn not_self(admin: &AdminUser, id: i64) -> Res<()> {
    if admin.id == id {
        return Err(ApiError::bad_request(
            "use Settings for your own password and two-factor",
        ));
    }
    Ok(())
}

async fn reset_password(
    admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Res<Json<TemporaryPassword>> {
    not_self(&admin, id)?;
    let user = target(&state, id).await?;
    let temp = users::temporary_password()?;
    let hash = super::account::hash_under_gate(&state, temp.clone()).await?;
    users::set_password(&state.pool, user.id, &hash, true).await?;
    tracing::info!(user_id = user.id, "password reset by an admin");
    Ok(Json(TemporaryPassword {
        temporary_password: temp,
    }))
}

async fn reset_2fa(
    admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Res<StatusCode> {
    not_self(&admin, id)?;
    let user = target(&state, id).await?;
    users::reset_totp(&state.pool, user.id).await?;
    tracing::info!(user_id = user.id, "2FA reset by an admin");
    Ok(StatusCode::NO_CONTENT)
}

/// The last active admin cannot be taken away.
async fn keep_one_admin(state: &AppState, user: &User) -> Res<()> {
    if user.role == Role::Admin
        && user.disabled_at.is_none()
        && users::other_active_admins(&state.pool, user.id).await? == 0
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "this is the last active admin",
        ));
    }
    Ok(())
}

async fn disable(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Res<StatusCode> {
    let user = target(&state, id).await?;
    keep_one_admin(&state, &user).await?;
    users::set_disabled(&state.pool, user.id, true, state.now()).await?;
    tracing::info!(user_id = user.id, "user disabled");
    Ok(StatusCode::NO_CONTENT)
}

async fn enable(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Res<StatusCode> {
    let user = target(&state, id).await?;
    users::set_disabled(&state.pool, user.id, false, state.now()).await?;
    tracing::info!(user_id = user.id, "user enabled");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct DeleteQuery {
    #[serde(default)]
    confirm: bool,
}

async fn delete_user(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<DeleteQuery>,
) -> Res<StatusCode> {
    let user = target(&state, id).await?;
    keep_one_admin(&state, &user).await?;
    let info = user_info(&state, chrono_tz::Tz::UTC, &user).await?;
    if info.has_data && !q.confirm {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "this user has data; delete with confirm=true to unassign their files and device \
             tokens and remove their records",
        ));
    }
    // The user's files and tokens go back to "unassigned" (nothing on disk is deleted), their
    // records are removed, and the derived rows of both partitions are rebuilt.
    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
    for sql in [
        "UPDATE tokens SET user_id = NULL, device_secret_hash = NULL WHERE user_id = ?",
        "DELETE FROM sessions WHERE user_id = ?",
        "DELETE FROM file_owners WHERE user_id = ?",
        "DELETE FROM user_settings WHERE user_id = ?",
        "DELETE FROM body_measurements WHERE user_id = ?",
        "DELETE FROM strength_set_overrides WHERE user_id = ?",
        "DELETE FROM garmin_uploads WHERE user_id = ?",
        "DELETE FROM garmin_account WHERE user_id = ?",
        "DELETE FROM users WHERE id = ?",
    ] {
        sqlx::query(sql).bind(user.id).execute(&mut *tx).await?;
    }
    if info.files > 0 {
        crate::derive::reparse_partitions(&mut tx, &state.data_dir, &[user.id, 0]).await?;
    } else {
        crate::derived::clear_samples(&mut tx, Some(&[user.id])).await?;
    }
    tx.commit().await?;
    tracing::info!(user_id = user.id, "user deleted");
    Ok(StatusCode::NO_CONTENT)
}

// ---- unassigned data ---------------------------------------------------------------

/// Files nobody owns that `token_id` uploaded.
const TOKEN_FILES_SQL: &str = "SELECT COUNT(DISTINCT u.sha256) FROM uploads u
    WHERE u.device_id = ? AND NOT EXISTS (SELECT 1 FROM file_owners o WHERE o.sha256 = u.sha256)";

/// Unowned files that none of the unassigned active tokens uploaded.
const UNATTRIBUTED_SQL: &str = "SELECT f.sha256 FROM files f
    WHERE NOT EXISTS (SELECT 1 FROM file_owners o WHERE o.sha256 = f.sha256)
      AND NOT EXISTS (SELECT 1 FROM uploads u JOIN tokens t ON t.id = u.device_id
                      WHERE u.sha256 = f.sha256 AND t.user_id IS NULL AND t.revoked_at IS NULL)";

async fn legacy(pool: &SqlitePool) -> Res<LegacyData> {
    let n = |sql: &'static str| async move {
        let v: i64 = sqlx::query_scalar(sql).fetch_one(pool).await?;
        Ok::<i64, ApiError>(v)
    };
    Ok(LegacyData {
        settings: n("SELECT COUNT(*) FROM user_settings WHERE user_id = 0").await? > 0,
        body_entries: n("SELECT COUNT(*) FROM body_measurements WHERE user_id = 0").await? as u32,
        strength_edits: n("SELECT COUNT(*) FROM strength_set_overrides WHERE user_id = 0").await?
            as u32,
        garmin_account: n("SELECT COUNT(*) FROM garmin_account WHERE user_id = 0").await? > 0,
    })
}

async fn unassigned(
    admin: AdminUser,
    State(state): State<AppState>,
) -> Res<Json<UnassignedResponse>> {
    let tz = admin_tz(&state, &admin).await?;
    let mut out = Vec::new();
    for t in tokens::list(&state.pool, tokens::Whose::Unassigned).await? {
        if t.revoked_at.is_some() {
            continue;
        }
        let files: i64 = sqlx::query_scalar(TOKEN_FILES_SQL)
            .bind(t.id)
            .fetch_one(&state.pool)
            .await?;
        let TokenInfo {
            id,
            name,
            created_at,
            last_used_at,
            ..
        } = token_info(tz, t);
        out.push(UnassignedToken {
            id,
            name,
            created_at,
            last_used_at,
            files: files as u32,
        });
    }
    let unattributed: Vec<String> = sqlx::query_scalar(UNATTRIBUTED_SQL)
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(UnassignedResponse {
        tokens: out,
        unattributed_files: unattributed.len() as u32,
        legacy: legacy(&state.pool).await?,
    }))
}

async fn assign(
    _: AdminUser,
    State(state): State<AppState>,
    ApiJson(body): ApiJson<AssignRequest>,
) -> Res<Json<AssignResponse>> {
    let user = target(&state, body.user_id).await?;
    if user.role != Role::User || user.disabled_at.is_some() {
        return Err(ApiError::bad_request(
            "data can only be assigned to an enabled user account",
        ));
    }
    let mut ids = body.token_ids.clone();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() && body.include_unattributed != Some(true) && body.adopt_legacy != Some(true)
    {
        return Err(ApiError::bad_request("nothing to assign"));
    }
    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
    // Every token must exist, be active and unassigned.
    for id in &ids {
        let ok: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM tokens WHERE id = ? AND user_id IS NULL AND revoked_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        if ok.is_none() {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                format!("token {id} is not an active unassigned token"),
            ));
        }
    }
    let mut files = 0u32;
    for id in &ids {
        sqlx::query("UPDATE tokens SET user_id = ? WHERE id = ?")
            .bind(user.id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        files += sqlx::query(
            "INSERT OR IGNORE INTO file_owners (sha256, user_id)
             SELECT DISTINCT sha256, ? FROM uploads WHERE device_id = ?",
        )
        .bind(user.id)
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected() as u32;
    }
    if body.include_unattributed == Some(true) {
        files += sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT OR IGNORE INTO file_owners (sha256, user_id) SELECT sha256, ? FROM ({UNATTRIBUTED_SQL})"
        )))
        .bind(user.id)
        .execute(&mut *tx)
        .await?
        .rows_affected() as u32;
    }
    let mut adopted = false;
    if body.adopt_legacy == Some(true) {
        // The user must not have records of their own that the old ones would collide with.
        let own: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM user_settings WHERE user_id = ?1)
                  + (SELECT COUNT(*) FROM body_measurements WHERE user_id = ?1)
                  + (SELECT COUNT(*) FROM strength_set_overrides WHERE user_id = ?1)
                  + (SELECT COUNT(*) FROM garmin_account WHERE user_id = ?1)",
        )
        .bind(user.id)
        .fetch_one(&mut *tx)
        .await?;
        if own > 0 {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "this user already has records of their own; the old ones cannot be merged",
            ));
        }
        for sql in [
            "UPDATE user_settings SET user_id = ? WHERE user_id = 0",
            "UPDATE body_measurements SET user_id = ? WHERE user_id = 0",
            "UPDATE strength_set_overrides SET user_id = ? WHERE user_id = 0",
            "UPDATE garmin_account SET user_id = ? WHERE user_id = 0",
            "UPDATE garmin_uploads SET user_id = ? WHERE user_id = 0",
        ] {
            adopted |= sqlx::query(sql)
                .bind(user.id)
                .execute(&mut *tx)
                .await?
                .rows_affected()
                > 0;
        }
    }
    // The files changed owner: rebuild the derived rows of both partitions.
    if files > 0 {
        crate::derive::reparse_partitions(&mut tx, &state.data_dir, &[0, user.id]).await?;
    }
    tx.commit().await?;
    tracing::info!(
        user_id = user.id,
        tokens = ids.len(),
        files,
        "unassigned data assigned"
    );
    Ok(Json(AssignResponse {
        tokens_moved: ids.len() as u32,
        files_moved: files,
        legacy_adopted: adopted,
    }))
}
