//! The Garmin account row, the upload queue and the background worker.
//!
//! Queue contract (IMPLEMENTATION_PLAN.md, Stage 2g): table `garmin_uploads`;
//! every activity file stored while Forward is on is queued; transient errors
//! back off 1 min doubling to 6 h; 429 honours Retry-After; 401 gets one
//! refresh and then pauses the queue; Garmin's duplicate answer counts as
//! done; no file is tried more than 10 times without an owner action.
//!
//! Runtime queries (not `query!` macros), so `.sqlx` needs no update.

use super::auth::{self, AuthError, Tokens};
use super::upload::{self, UploadOutcome};
use crate::api::AppState;
use anyhow::{Context, Result};
use sqlx::{Row, SqlitePool};
use std::time::Duration;

pub const MAX_TRIES: i64 = 10;
const BACKOFF_FIRST_S: i64 = 60;
const BACKOFF_MAX_S: i64 = 6 * 3600;
const RETRY_AFTER_MAX_S: i64 = 24 * 3600;

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

/// The one account row. `Debug` shows no token (see [`Tokens`]).
#[derive(Debug, Clone)]
pub struct Account {
    pub tokens: Option<Tokens>,
    pub status: AccountStatus,
    pub last_error: Option<String>,
}

pub async fn load_account(pool: &SqlitePool) -> Result<Option<Account>> {
    let row = sqlx::query(
        "SELECT di_token, di_refresh_token, di_client_id, status, last_error
         FROM garmin_account WHERE id = 1",
    )
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
        }
    }))
}

/// Store the tokens of a fresh login or import: Forward is now on and connected.
pub async fn save_login(pool: &SqlitePool, t: &Tokens, now: i64) -> Result<()> {
    sqlx::query(
        "INSERT INTO garmin_account
             (id, di_token, di_refresh_token, di_client_id, status, last_error, enabled_at, updated_at)
         VALUES (1, ?, ?, ?, 'connected', NULL, ?, ?)
         ON CONFLICT (id) DO UPDATE SET
             di_token = excluded.di_token, di_refresh_token = excluded.di_refresh_token,
             di_client_id = excluded.di_client_id, status = 'connected', last_error = NULL,
             updated_at = excluded.updated_at",
    )
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
pub async fn save_tokens(pool: &SqlitePool, t: &Tokens, now: i64) -> Result<()> {
    sqlx::query(
        "UPDATE garmin_account SET di_token = ?, di_refresh_token = ?, di_client_id = ?, updated_at = ?
         WHERE id = 1",
    )
    .bind(&t.di_token)
    .bind(&t.di_refresh_token)
    .bind(&t.di_client_id)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_status(
    pool: &SqlitePool,
    status: AccountStatus,
    error: Option<&str>,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE garmin_account SET status = ?, last_error = ?, updated_at = ? WHERE id = 1",
    )
    .bind(status.as_str())
    .bind(error)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// The refresh token is dead: forget the tokens (they are useless) and ask for a new login.
pub async fn expire_login(pool: &SqlitePool, error: &str, now: i64) -> Result<()> {
    sqlx::query(
        "UPDATE garmin_account SET di_token = '', di_refresh_token = '', status = 'needs_login',
                last_error = ?, updated_at = ? WHERE id = 1",
    )
    .bind(error)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Forward off: the tokens and every file not yet sent are forgotten; done rows stay.
pub async fn delete_account(pool: &SqlitePool) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM garmin_account")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM garmin_uploads WHERE status != 'done'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Queue one file if Forward is on. True when a row was added.
pub async fn enqueue(pool: &SqlitePool, sha256: &str, now: i64) -> Result<bool> {
    let r = sqlx::query(
        "INSERT INTO garmin_uploads (file_sha256, status, attempts, next_attempt_at, created_at)
         SELECT ?, 'pending', 0, ?, ? WHERE EXISTS (SELECT 1 FROM garmin_account)
         ON CONFLICT (file_sha256) DO NOTHING",
    )
    .bind(sha256)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

/// Queue the best file of every activity that started at or after `since_ts`.
pub async fn backfill(pool: &SqlitePool, since_ts: i64, now: i64) -> Result<u64> {
    let r = sqlx::query(
        "INSERT INTO garmin_uploads (file_sha256, status, attempts, next_attempt_at, created_at)
         SELECT sha256, 'pending', 0, ?, ? FROM activities_current
         WHERE COALESCE(start_ts, ts, time_created) >= ?
         ON CONFLICT (file_sha256) DO NOTHING",
    )
    .bind(now)
    .bind(now)
    .bind(since_ts)
    .execute(pool)
    .await?;
    Ok(r.rows_affected())
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub pending: i64,
    pub done: i64,
    pub failed: i64,
}

pub async fn counts(pool: &SqlitePool) -> Result<Counts> {
    let rows = sqlx::query("SELECT status, COUNT(*) FROM garmin_uploads GROUP BY status")
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
pub async fn last_item_error(pool: &SqlitePool) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT last_error FROM garmin_uploads WHERE last_error IS NOT NULL AND status != 'done'
         ORDER BY (status = 'failed') DESC, created_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn next_attempt_at(pool: &SqlitePool) -> Result<Option<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT MIN(next_attempt_at) FROM garmin_uploads WHERE status = 'pending'",
    )
    .fetch_one(pool)
    .await?)
}

/// Owner action: failed files get a fresh set of tries; a paused queue resumes.
pub async fn retry(pool: &SqlitePool, now: i64) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let r = sqlx::query(
        "UPDATE garmin_uploads SET status = 'pending', attempts = 0, next_attempt_at = ?, last_error = NULL
         WHERE status = 'failed'",
    )
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE garmin_uploads SET next_attempt_at = MIN(next_attempt_at, ?) WHERE status = 'pending'",
    )
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE garmin_account SET status = 'connected', last_error = NULL, updated_at = ?
         WHERE id = 1 AND status = 'paused'",
    )
    .bind(now)
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
                    let connected = matches!(
                        load_account(&state.pool).await,
                        Ok(Some(Account {
                            status: AccountStatus::Connected,
                            ..
                        }))
                    );
                    if connected {
                        if let Ok(Some(at)) = next_attempt_at(&state.pool).await {
                            wait = (at - state.now()).clamp(1, 3600);
                        }
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

/// Process every file that is due now. Returns how many files were handled.
/// Stops early when the account is not connected or Garmin pushes back.
pub async fn run_due(state: &AppState) -> Result<usize> {
    let mut handled = 0;
    loop {
        let now = state.now();
        let Some(account) = load_account(&state.pool).await? else {
            break;
        };
        if account.status != AccountStatus::Connected {
            break;
        }
        let Some(tokens) = account.tokens else {
            expire_login(&state.pool, "no stored login", now).await?;
            break;
        };
        let Some((sha, attempts)) = next_due(&state.pool, now).await? else {
            break;
        };
        handled += 1;
        if let Flow::Stop = process(state, tokens, &sha, attempts, now).await? {
            break;
        }
    }
    Ok(handled)
}

async fn next_due(pool: &SqlitePool, now: i64) -> Result<Option<(String, i64)>> {
    let row = sqlx::query(
        "SELECT file_sha256, attempts FROM garmin_uploads
         WHERE status = 'pending' AND next_attempt_at <= ?
         ORDER BY next_attempt_at, created_at, file_sha256 LIMIT 1",
    )
    .bind(now)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get(0), r.get(1))))
}

enum Flow {
    Continue,
    Stop,
}

enum Refreshed {
    Fresh(Tokens),
    /// Refresh token refused: needs_login is stored.
    LoginExpired,
    /// Garmin unreachable or rate limiting the refresh: the queue waits this long.
    Later(i64),
}

/// Refresh under the token lock and store the (possibly rotated) result.
async fn refresh_tokens(state: &AppState, current: &Tokens) -> Result<Refreshed> {
    let g = &state.garmin;
    let _lock = g.token_lock.lock().await;
    // Someone may have refreshed while we waited for the lock.
    let stored = load_account(&state.pool)
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
            save_tokens(&state.pool, &new, now).await?;
            Ok(Refreshed::Fresh(new))
        }
        Err(AuthError::Rejected) => {
            tracing::warn!("garmin refresh token was refused; login needed");
            expire_login(&state.pool, "Garmin login expired; log in again", now).await?;
            Ok(Refreshed::LoginExpired)
        }
        Err(AuthError::RateLimited(ra)) => {
            let wait = ra.map_or(BACKOFF_FIRST_S, |s| (s as i64).clamp(1, RETRY_AFTER_MAX_S));
            note_error(&state.pool, "Garmin rate limited the token refresh", now).await?;
            Ok(Refreshed::Later(wait))
        }
        Err(e) => {
            tracing::warn!(error = %e, "garmin refresh failed");
            note_error(&state.pool, &format!("token refresh failed: {e}"), now).await?;
            Ok(Refreshed::Later(BACKOFF_FIRST_S))
        }
    }
}

async fn note_error(pool: &SqlitePool, msg: &str, now: i64) -> Result<()> {
    sqlx::query("UPDATE garmin_account SET last_error = ?, updated_at = ? WHERE id = 1")
        .bind(msg)
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

async fn defer_all(pool: &SqlitePool, until: i64) -> Result<()> {
    sqlx::query(
        "UPDATE garmin_uploads SET next_attempt_at = MAX(next_attempt_at, ?) WHERE status = 'pending'",
    )
    .bind(until)
    .execute(pool)
    .await?;
    Ok(())
}

async fn process(
    state: &AppState,
    mut tokens: Tokens,
    sha: &str,
    attempts: i64,
    now: i64,
) -> Result<Flow> {
    let g = &state.garmin;
    let pool = &state.pool;

    if tokens.expires_soon(now) {
        match refresh_tokens(state, &tokens).await? {
            Refreshed::Fresh(t) => tokens = t,
            Refreshed::LoginExpired => return Ok(Flow::Stop),
            Refreshed::Later(s) => {
                defer_all(pool, now + s).await?;
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

    let mut outcome =
        upload::upload_activity(&g.cfg, g.api().as_ref(), &tokens, &name, &bytes).await;
    if outcome == UploadOutcome::AuthExpired {
        // One refresh, one more try; a second 401 pauses the queue.
        match refresh_tokens(state, &tokens).await? {
            Refreshed::Fresh(t) => {
                tokens = t;
                outcome =
                    upload::upload_activity(&g.cfg, g.api().as_ref(), &tokens, &name, &bytes).await;
            }
            Refreshed::LoginExpired => return Ok(Flow::Stop),
            Refreshed::Later(s) => {
                defer_all(pool, now + s).await?;
                return Ok(Flow::Stop);
            }
        }
        if outcome == UploadOutcome::AuthExpired {
            tracing::warn!("garmin kept answering 401 after a refresh; queue paused");
            set_status(
                pool,
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
            clear_account_error(pool).await?;
        }
        UploadOutcome::Duplicate => {
            tracing::info!(sha = short, "garmin already has this activity");
            finish(
                pool,
                sha,
                tries,
                "done",
                None,
                Some("Garmin already had this activity"),
                now,
            )
            .await?;
            clear_account_error(pool).await?;
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
            defer_all(pool, now + wait).await?;
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

async fn clear_account_error(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "UPDATE garmin_account SET last_error = NULL WHERE id = 1 AND last_error IS NOT NULL",
    )
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
