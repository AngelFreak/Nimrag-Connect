//! The Garmin account row, the upload queue and the background worker.
//!
//! One Garmin account per user (`garmin_account.user_id`); a queued file belongs to the user who
//! owns it (`garmin_uploads.user_id`). The pre-multi-user account sits under user 0 and is not
//! worked until an admin assigns it to a user.
//!
//! Queue contract (IMPLEMENTATION_PLAN.md, Stage 2g): table `garmin_uploads`;
//! every activity file stored while Forward is on is queued, and with the account's `send_all`
//! choice every other watch/USB FIT file too; the oldest file goes first and the worker pauses
//! [`GarminConfig::upload_pause_ms`](super::GarminConfig) between uploads; transient errors
//! back off 1 min doubling to 6 h; 429 honours Retry-After; 401 gets one
//! refresh and then pauses the queue; Garmin's duplicate answer counts as
//! done (for every file type); no file is tried more than 10 times without an owner action.
//!
//! Runtime queries (not `query!` macros), so `.sqlx` needs no update.

use super::auth::{self, AuthError, Tokens};
use super::upload::{self, UploadOutcome};
use crate::api::AppState;
use anyhow::{Context, Result};
use sqlx::{Row, SqlitePool};
use std::time::Duration;

pub const MAX_TRIES: i64 = 10;
pub(crate) const BACKOFF_FIRST_S: i64 = 60;
const BACKOFF_MAX_S: i64 = 6 * 3600;
pub(crate) const RETRY_AFTER_MAX_S: i64 = 24 * 3600;

/// Delay before try number `attempts + 1`, after `attempts` failures (1 min, doubling, 6 h cap).
pub fn backoff_s(attempts: i64) -> i64 {
    let shift = (attempts - 1).clamp(0, 20);
    (BACKOFF_FIRST_S << shift).min(BACKOFF_MAX_S)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatus {
    Connected,
    /// The queue is stopped: Garmin kept answering 401 after a refresh. Retry resumes it.
    Paused,
    /// The refresh token was refused (or no tokens): the owner must log in again.
    NeedsLogin,
}

impl AccountStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Paused => "paused",
            Self::NeedsLogin => "needs_login",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "paused" => Self::Paused,
            "needs_login" => Self::NeedsLogin,
            _ => Self::Connected,
        }
    }
}

/// A user's account row. `Debug` shows no token (see [`Tokens`]).
#[derive(Debug, Clone)]
pub struct Account {
    pub tokens: Option<Tokens>,
    pub status: AccountStatus,
    pub last_error: Option<String>,
    /// False: new activities only. True: every FIT file the watch produces.
    pub send_all: bool,
    /// Fetch nights the watch never delivered from Garmin (see [`super::nights`]).
    pub fill_nights: bool,
}

pub async fn load_account(pool: &SqlitePool, uid: i64) -> Result<Option<Account>> {
    let row = sqlx::query(
        "SELECT di_token, di_refresh_token, di_client_id, status, last_error, send_all, fill_nights
         FROM garmin_account WHERE user_id = ?",
    )
    .bind(uid)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| {
        let (t, rt, cid): (String, String, String) = (r.get(0), r.get(1), r.get(2));
        Account {
            tokens: (!t.is_empty() && !rt.is_empty()).then_some(Tokens {
                di_token: t,
                di_refresh_token: rt,
                di_client_id: cid,
            }),
            status: AccountStatus::parse(&r.get::<String, _>(3)),
            last_error: r.get(4),
            send_all: r.get::<i64, _>(5) != 0,
            fill_nights: r.get::<i64, _>(6) != 0,
        }
    }))
}

/// The owner's choice: activities only, or every file from the watch. False when the user has
/// no account row. Changing it queues nothing by itself (backfill is explicit).
pub async fn set_send_all(pool: &SqlitePool, uid: i64, send_all: bool, now: i64) -> Result<bool> {
    let r = sqlx::query("UPDATE garmin_account SET send_all = ?, updated_at = ? WHERE user_id = ?")
        .bind(send_all as i64)
        .bind(now)
        .bind(uid)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// The owner's choice: fetch missed nights from Garmin or not. False when the user has no
/// account row.
pub async fn set_fill_nights(pool: &SqlitePool, uid: i64, on: bool, now: i64) -> Result<bool> {
    let r =
        sqlx::query("UPDATE garmin_account SET fill_nights = ?, updated_at = ? WHERE user_id = ?")
            .bind(on as i64)
            .bind(now)
            .bind(uid)
            .execute(pool)
            .await?;
    Ok(r.rows_affected() > 0)
}

/// Store the tokens of a fresh login or import: Forward is now on and connected.
pub async fn save_login(pool: &SqlitePool, uid: i64, t: &Tokens, now: i64) -> Result<()> {
    sqlx::query(
        "INSERT INTO garmin_account
             (user_id, di_token, di_refresh_token, di_client_id, status, last_error, enabled_at, updated_at)
         VALUES (?, ?, ?, ?, 'connected', NULL, ?, ?)
         ON CONFLICT (user_id) DO UPDATE SET
             di_token = excluded.di_token, di_refresh_token = excluded.di_refresh_token,
             di_client_id = excluded.di_client_id, status = 'connected', last_error = NULL,
             updated_at = excluded.updated_at",
    )
    .bind(uid)
    .bind(&t.di_token)
    .bind(&t.di_refresh_token)
    .bind(&t.di_client_id)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Store refreshed (possibly rotated) tokens without touching the status.
pub async fn save_tokens(pool: &SqlitePool, uid: i64, t: &Tokens, now: i64) -> Result<()> {
    sqlx::query(
        "UPDATE garmin_account SET di_token = ?, di_refresh_token = ?, di_client_id = ?, updated_at = ?
         WHERE user_id = ?",
    )
    .bind(&t.di_token)
    .bind(&t.di_refresh_token)
    .bind(&t.di_client_id)
    .bind(now)
    .bind(uid)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_status(
    pool: &SqlitePool,
    uid: i64,
    status: AccountStatus,
    error: Option<&str>,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE garmin_account SET status = ?, last_error = ?, updated_at = ? WHERE user_id = ?",
    )
    .bind(status.as_str())
    .bind(error)
    .bind(now)
    .bind(uid)
    .execute(pool)
    .await?;
    Ok(())
}

/// The refresh token is dead: forget the tokens (they are useless) and ask for a new login.
pub async fn expire_login(pool: &SqlitePool, uid: i64, error: &str, now: i64) -> Result<()> {
    sqlx::query(
        "UPDATE garmin_account SET di_token = '', di_refresh_token = '', status = 'needs_login',
                last_error = ?, updated_at = ? WHERE user_id = ?",
    )
    .bind(error)
    .bind(now)
    .bind(uid)
    .execute(pool)
    .await?;
    Ok(())
}

/// Forward off: the tokens and every file not yet sent are forgotten; done rows stay.
pub async fn delete_account(pool: &SqlitePool, uid: i64) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM garmin_account WHERE user_id = ?")
        .bind(uid)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM garmin_uploads WHERE status != 'done' AND user_id = ?")
        .bind(uid)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM garmin_night_checks WHERE user_id = ?")
        .bind(uid)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Queue one activity file of user `uid` if that user has Forward on. True when a row was added.
pub async fn enqueue(pool: &SqlitePool, uid: i64, sha256: &str, now: i64) -> Result<bool> {
    let r = sqlx::query(
        "INSERT INTO garmin_uploads (file_sha256, status, attempts, next_attempt_at, created_at, user_id, order_ts)
         SELECT ?, 'pending', 0, ?, ?, ?, ? WHERE EXISTS (SELECT 1 FROM garmin_account WHERE user_id = ?)
         ON CONFLICT (file_sha256) DO NOTHING",
    )
    .bind(sha256)
    .bind(now)
    .bind(now)
    .bind(uid)
    .bind(now * 1000)
    .bind(uid)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

/// Queue one non-activity file of user `uid` if that user has Forward on with `send_all`.
/// True when a row was added.
pub async fn enqueue_other(pool: &SqlitePool, uid: i64, sha256: &str, now: i64) -> Result<bool> {
    let r = sqlx::query(
        "INSERT INTO garmin_uploads (file_sha256, status, attempts, next_attempt_at, created_at, user_id, order_ts)
         SELECT ?, 'pending', 0, ?, ?, ?, ?
         WHERE EXISTS (SELECT 1 FROM garmin_account WHERE user_id = ? AND send_all = 1)
         ON CONFLICT (file_sha256) DO NOTHING",
    )
    .bind(sha256)
    .bind(now)
    .bind(now)
    .bind(uid)
    .bind(now * 1000)
    .bind(uid)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

/// Queue the best file of every activity of user `uid` that started at or after `since_ts`.
pub async fn backfill(pool: &SqlitePool, uid: i64, since_ts: i64, now: i64) -> Result<u64> {
    let r = sqlx::query(
        "INSERT INTO garmin_uploads (file_sha256, status, attempts, next_attempt_at, created_at, user_id, order_ts)
         SELECT sha256, 'pending', 0, ?, ?, ?, COALESCE(start_ts, ts, time_created) * 1000
         FROM activities_current
         WHERE user_id = ? AND origin = 'fit' AND COALESCE(start_ts, ts, time_created) >= ?
         ON CONFLICT (file_sha256) DO NOTHING",
    )
    .bind(now)
    .bind(now)
    .bind(uid)
    .bind(uid)
    .bind(since_ts)
    .execute(pool)
    .await?;
    Ok(r.rows_affected())
}

/// Queue every FIT file of user `uid` that came from the watch or USB and was received at or
/// after `since_ts` (unix seconds), activities included; files Garmin gave us (`import`,
/// `connect`) are left out. The worker sends them oldest first. Files already in the queue (done,
/// failed or waiting) are skipped.
pub async fn backfill_all(pool: &SqlitePool, uid: i64, since_ts: i64, now: i64) -> Result<u64> {
    // `received_at` is `2026-11-19T06:42:00.123Z`: unix seconds plus the three digits of the
    // fraction, in milliseconds, so files of one sync keep their order.
    let r = sqlx::query(
        "INSERT INTO garmin_uploads (file_sha256, status, attempts, next_attempt_at, created_at, user_id, order_ts)
         SELECT sha256, 'pending', 0, ?, ?, ?, ms FROM (
             SELECT f.sha256 AS sha256,
                    CAST(strftime('%s', f.received_at) AS INTEGER) * 1000
                        + CAST(substr(f.received_at, 21, 3) AS INTEGER) AS ms
             FROM files f JOIN file_owners o ON o.sha256 = f.sha256
             WHERE o.user_id = ? AND f.source IN ('watch', 'usb') AND f.data_type = 128
         ) WHERE ms >= ?
         ON CONFLICT (file_sha256) DO NOTHING",
    )
        .bind(now)
        .bind(now)
        .bind(uid)
        .bind(uid)
        .bind(since_ts * 1000)
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

/// Is this stored file an activity (FIT 128/4)? Only the wording of the messages depends on it.
async fn is_activity(pool: &SqlitePool, sha: &str) -> bool {
    sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM files WHERE sha256 = ? AND data_type = 128 AND sub_type = 4",
    )
    .bind(sha)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .is_some()
}

/// The note stored on a file Garmin already had.
fn duplicate_note(activity: bool) -> &'static str {
    if activity {
        "Garmin already had this activity"
    } else {
        "Garmin already had this file"
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub pending: i64,
    pub done: i64,
    pub failed: i64,
}

pub async fn counts(pool: &SqlitePool, uid: i64) -> Result<Counts> {
    let rows = sqlx::query(
        "SELECT status, COUNT(*) FROM garmin_uploads WHERE user_id = ? GROUP BY status",
    )
    .bind(uid)
    .fetch_all(pool)
    .await?;
    let mut c = Counts::default();
    for r in rows {
        let n: i64 = r.get(1);
        match r.get::<String, _>(0).as_str() {
            "pending" => c.pending = n,
            "done" => c.done = n,
            _ => c.failed = n,
        }
    }
    Ok(c)
}

/// The most telling error: a failed file first, then the newest waiting one.
pub async fn last_item_error(pool: &SqlitePool, uid: i64) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT last_error FROM garmin_uploads
         WHERE user_id = ? AND last_error IS NOT NULL AND status != 'done'
         ORDER BY (status = 'failed') DESC, created_at DESC LIMIT 1",
    )
    .bind(uid)
    .fetch_optional(pool)
    .await?)
}

pub async fn next_attempt_at(pool: &SqlitePool, uid: i64) -> Result<Option<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT MIN(next_attempt_at) FROM garmin_uploads WHERE status = 'pending' AND user_id = ?",
    )
    .bind(uid)
    .fetch_one(pool)
    .await?)
}

/// User action: failed files get a fresh set of tries; a paused queue resumes.
pub async fn retry(pool: &SqlitePool, uid: i64, now: i64) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let r = sqlx::query(
        "UPDATE garmin_uploads SET status = 'pending', attempts = 0, next_attempt_at = ?, last_error = NULL
         WHERE status = 'failed' AND user_id = ?",
    )
    .bind(now)
    .bind(uid)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE garmin_uploads SET next_attempt_at = MIN(next_attempt_at, ?)
         WHERE status = 'pending' AND user_id = ?",
    )
    .bind(now)
    .bind(uid)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE garmin_account SET status = 'connected', last_error = NULL, updated_at = ?
         WHERE user_id = ? AND status = 'paused'",
    )
    .bind(now)
    .bind(uid)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(r.rows_affected())
}

// ---- worker -------------------------------------------------------------------------

/// Background task: process due files, then sleep until the next one is due or
/// [`Garmin::wake`](super::Garmin::wake) is called. Errors are logged and retried later.
pub fn spawn_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let mut wait = 3600;
            match run_due(&state).await {
                Ok(_) => {
                    // The soonest waiting file of any connected account.
                    if let Ok(Some(at)) = next_attempt_any(&state.pool).await {
                        wait = (at - state.now()).clamp(1, 3600);
                    }
                }
                Err(e) => {
                    tracing::warn!(error = format!("{e:#}"), "garmin worker failed");
                    wait = 60;
                }
            }
            tokio::select! {
                () = state.garmin.wake_wait() => {}
                () = tokio::time::sleep(Duration::from_secs(wait as u64)) => {}
            }
        }
    })
}

/// The soonest waiting file among connected accounts of real users.
async fn next_attempt_any(pool: &SqlitePool) -> Result<Option<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT MIN(u.next_attempt_at) FROM garmin_uploads u
         JOIN garmin_account a ON a.user_id = u.user_id
         WHERE u.status = 'pending' AND a.status = 'connected' AND a.user_id > 0",
    )
    .fetch_one(pool)
    .await?)
}

/// Process every file that is due now, for every user with a connected account. Returns how
/// many files were handled. A user's pass stops early when the account is not connected or
/// Garmin pushes back; the other users go on. The pre-multi-user account (user 0) waits until
/// an admin assigns it to a user.
pub async fn run_due(state: &AppState) -> Result<usize> {
    let uids: Vec<i64> = sqlx::query_scalar(
        "SELECT user_id FROM garmin_account WHERE status = 'connected' AND user_id > 0 ORDER BY user_id",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut handled = 0;
    for uid in uids {
        handled += run_due_user(state, uid).await?;
    }
    Ok(handled)
}

/// [`run_due`] for one user.
pub async fn run_due_user(state: &AppState, uid: i64) -> Result<usize> {
    let mut handled = 0;
    let pause_ms = state.garmin.cfg.upload_pause_ms;
    loop {
        // Gentle with Garmin: a breath between two uploads (not after the last one).
        if handled > 0 && pause_ms > 0 && next_due(&state.pool, uid, state.now()).await?.is_some() {
            tokio::time::sleep(Duration::from_millis(pause_ms)).await;
        }
        let now = state.now();
        let Some(account) = load_account(&state.pool, uid).await? else {
            break;
        };
        if account.status != AccountStatus::Connected {
            break;
        }
        let Some(tokens) = account.tokens else {
            expire_login(&state.pool, uid, "no stored login", now).await?;
            break;
        };
        let Some((sha, attempts)) = next_due(&state.pool, uid, now).await? else {
            break;
        };
        handled += 1;
        if let Flow::Stop = process(state, uid, tokens, &sha, attempts, now).await? {
            break;
        }
    }
    Ok(handled)
}

async fn next_due(pool: &SqlitePool, uid: i64, now: i64) -> Result<Option<(String, i64)>> {
    let row = sqlx::query(
        "SELECT file_sha256, attempts FROM garmin_uploads
         WHERE status = 'pending' AND next_attempt_at <= ? AND user_id = ?
         ORDER BY next_attempt_at, COALESCE(order_ts, created_at * 1000), created_at, file_sha256
         LIMIT 1",
    )
    .bind(now)
    .bind(uid)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get(0), r.get(1))))
}

enum Flow {
    Continue,
    Stop,
}

pub(crate) enum Refreshed {
    Fresh(Tokens),
    /// Refresh token refused: needs_login is stored.
    LoginExpired,
    /// Garmin unreachable or rate limiting the refresh: the queue waits this long.
    Later(i64),
}

/// Refresh under the token lock and store the (possibly rotated) result.
pub(crate) async fn refresh_tokens(
    state: &AppState,
    uid: i64,
    current: &Tokens,
) -> Result<Refreshed> {
    let g = &state.garmin;
    let _lock = g.token_lock.lock().await;
    // Someone may have refreshed while we waited for the lock.
    let stored = load_account(&state.pool, uid)
        .await?
        .and_then(|a| a.tokens)
        .filter(|t| t != current);
    if let Some(t) = stored {
        if !t.expires_soon(state.now()) {
            return Ok(Refreshed::Fresh(t));
        }
    }
    let now = state.now();
    match auth::refresh(&g.cfg, g.api().as_ref(), current).await {
        Ok(new) => {
            save_tokens(&state.pool, uid, &new, now).await?;
            Ok(Refreshed::Fresh(new))
        }
        Err(AuthError::Rejected) => {
            tracing::warn!("garmin refresh token was refused; login needed");
            expire_login(&state.pool, uid, "Garmin login expired; log in again", now).await?;
            Ok(Refreshed::LoginExpired)
        }
        Err(AuthError::RateLimited(ra)) => {
            let wait = ra.map_or(BACKOFF_FIRST_S, |s| (s as i64).clamp(1, RETRY_AFTER_MAX_S));
            note_error(
                &state.pool,
                uid,
                "Garmin rate limited the token refresh",
                now,
            )
            .await?;
            Ok(Refreshed::Later(wait))
        }
        Err(e) => {
            tracing::warn!(error = %e, "garmin refresh failed");
            note_error(&state.pool, uid, &format!("token refresh failed: {e}"), now).await?;
            Ok(Refreshed::Later(BACKOFF_FIRST_S))
        }
    }
}

async fn note_error(pool: &SqlitePool, uid: i64, msg: &str, now: i64) -> Result<()> {
    sqlx::query("UPDATE garmin_account SET last_error = ?, updated_at = ? WHERE user_id = ?")
        .bind(msg)
        .bind(now)
        .bind(uid)
        .execute(pool)
        .await?;
    Ok(())
}

async fn defer_all(pool: &SqlitePool, uid: i64, until: i64) -> Result<()> {
    sqlx::query(
        "UPDATE garmin_uploads SET next_attempt_at = MAX(next_attempt_at, ?)
         WHERE status = 'pending' AND user_id = ?",
    )
    .bind(until)
    .bind(uid)
    .execute(pool)
    .await?;
    Ok(())
}

async fn process(
    state: &AppState,
    uid: i64,
    mut tokens: Tokens,
    sha: &str,
    attempts: i64,
    now: i64,
) -> Result<Flow> {
    let g = &state.garmin;
    let pool = &state.pool;

    if tokens.expires_soon(now) {
        match refresh_tokens(state, uid, &tokens).await? {
            Refreshed::Fresh(t) => tokens = t,
            Refreshed::LoginExpired => return Ok(Flow::Stop),
            Refreshed::Later(s) => {
                defer_all(pool, uid, now + s).await?;
                return Ok(Flow::Stop);
            }
        }
    }

    let bytes = {
        let (dir, sha) = (state.data_dir.clone(), sha.to_owned());
        tokio::task::spawn_blocking(move || crate::store::read_verified(&dir, &sha))
            .await
            .context("read task")?
    };
    let bytes = match bytes {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(
                error = format!("{e:#}"),
                "garmin upload: stored file unreadable"
            );
            finish(
                pool,
                sha,
                attempts + 1,
                "failed",
                None,
                Some("stored file is unreadable"),
                now,
            )
            .await?;
            return Ok(Flow::Continue);
        }
    };
    let name = format!("{}.fit", &sha[..16]);

    let mut outcome = upload::upload_file(&g.cfg, g.api().as_ref(), &tokens, &name, &bytes).await;
    if outcome == UploadOutcome::AuthExpired {
        // One refresh, one more try; a second 401 pauses the queue.
        match refresh_tokens(state, uid, &tokens).await? {
            Refreshed::Fresh(t) => {
                tokens = t;
                outcome =
                    upload::upload_file(&g.cfg, g.api().as_ref(), &tokens, &name, &bytes).await;
            }
            Refreshed::LoginExpired => return Ok(Flow::Stop),
            Refreshed::Later(s) => {
                defer_all(pool, uid, now + s).await?;
                return Ok(Flow::Stop);
            }
        }
        if outcome == UploadOutcome::AuthExpired {
            tracing::warn!("garmin kept answering 401 after a refresh; queue paused");
            set_status(
                pool,
                uid,
                AccountStatus::Paused,
                Some("Garmin rejected the login after a refresh; log in again or retry"),
                now,
            )
            .await?;
            return Ok(Flow::Stop);
        }
    }

    let tries = attempts + 1;
    let short = &sha[..12];
    match outcome {
        UploadOutcome::Uploaded { garmin_activity_id } => {
            tracing::info!(sha = short, "garmin upload done");
            finish(pool, sha, tries, "done", garmin_activity_id, None, now).await?;
            clear_account_error(pool, uid).await?;
        }
        UploadOutcome::Duplicate => {
            let note = duplicate_note(is_activity(pool, sha).await);
            tracing::info!(sha = short, "{}", note.to_lowercase());
            finish(pool, sha, tries, "done", None, Some(note), now).await?;
            clear_account_error(pool, uid).await?;
        }
        UploadOutcome::Rejected(why) => {
            tracing::warn!(sha = short, "garmin rejected the file");
            finish(pool, sha, tries, "failed", None, Some(&why), now).await?;
        }
        UploadOutcome::RateLimited { retry_after_s } => {
            let wait = retry_after_s.map_or_else(
                || backoff_s(tries),
                |s| (s as i64).clamp(1, RETRY_AFTER_MAX_S),
            );
            tracing::warn!(sha = short, wait, "garmin rate limited the upload");
            retry_later(pool, sha, tries, "Garmin rate limit (429)", now, wait).await?;
            defer_all(pool, uid, now + wait).await?;
            return Ok(Flow::Stop);
        }
        UploadOutcome::Transient(why) => {
            tracing::warn!(sha = short, tries, "garmin upload failed, will retry");
            retry_later(pool, sha, tries, &why, now, backoff_s(tries)).await?;
        }
        UploadOutcome::AuthExpired => unreachable!("handled above"),
    }
    Ok(Flow::Continue)
}

async fn clear_account_error(pool: &SqlitePool, uid: i64) -> Result<()> {
    sqlx::query(
        "UPDATE garmin_account SET last_error = NULL WHERE user_id = ? AND last_error IS NOT NULL",
    )
    .bind(uid)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record a failed try: back off, or give up after [`MAX_TRIES`].
async fn retry_later(
    pool: &SqlitePool,
    sha: &str,
    tries: i64,
    why: &str,
    now: i64,
    wait: i64,
) -> Result<()> {
    if tries >= MAX_TRIES {
        let msg = format!("gave up after {MAX_TRIES} tries: {why}");
        return finish(pool, sha, tries, "failed", None, Some(&msg), now).await;
    }
    sqlx::query(
        "UPDATE garmin_uploads SET attempts = ?, next_attempt_at = ?, last_error = ? WHERE file_sha256 = ?",
    )
    .bind(tries)
    .bind(now + wait)
    .bind(why)
    .bind(sha)
    .execute(pool)
    .await?;
    Ok(())
}

async fn finish(
    pool: &SqlitePool,
    sha: &str,
    tries: i64,
    status: &str,
    garmin_activity_id: Option<i64>,
    note: Option<&str>,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE garmin_uploads SET status = ?, attempts = ?, next_attempt_at = ?,
                garmin_activity_id = ?, last_error = ? WHERE file_sha256 = ?",
    )
    .bind(status)
    .bind(tries)
    .bind(now)
    .bind(garmin_activity_id)
    .bind(note)
    .bind(sha)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_note_names_the_kind() {
        assert_eq!(duplicate_note(true), "Garmin already had this activity");
        assert_eq!(duplicate_note(false), "Garmin already had this file");
    }

    #[test]
    fn backoff_doubles_to_six_hours() {
        assert_eq!(backoff_s(1), 60);
        assert_eq!(backoff_s(2), 120);
        assert_eq!(backoff_s(3), 240);
        assert_eq!(backoff_s(8), 7680);
        assert_eq!(backoff_s(9), 15360);
        assert_eq!(backoff_s(10), 21600);
        assert_eq!(backoff_s(30), 21600);
    }
}
