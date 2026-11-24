//! The phone's side of offline edits (docs/design/offline-edits.md): the outbox of changes made
//! on the phone's copy, the `local id -> server id` map, and replacing the copy with the server's
//! state. Used by `embedded.rs`; the tables exist on a server too and stay empty there.
//!
//! A change is written to `edit_outbox` before its handler runs (`ready` = 0), then confirmed
//! (`ready` = 1) when the handler succeeded or removed when it did not. So a handler that
//! succeeded always has its row, even if the process dies in between; a row whose handler never
//! got to finish (a crash) is confirmed on the next start and the server decides.

use crate::edits::{DeviceState, Edit, SETTING_KEYS, STATE_VERSION};
use anyhow::{bail, Result};
use axum::http::Method;
use sqlx::SqlitePool;

/// The next change to send, its path already rewritten for the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub op_id: String,
    pub method: String,
    pub path: String,
    /// The request body as JSON text; empty for none.
    pub body: String,
    /// Unix seconds when the change was made.
    pub made_at: i64,
}

/// What became of a change after the server answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The server applied it; it is off the outbox.
    Applied,
    /// The server refused it for good (or it can no longer apply); it is off the outbox and
    /// listed by [`take_dropped`].
    Dropped,
    /// Not decided (no answer, server trouble, or the token was refused): it stays, try later.
    Kept,
}

/// A change the server could not apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub method: String,
    /// The path as the phone made it (its own ids).
    pub path: String,
    /// Why, in the server's words.
    pub reason: String,
}

/// Write a change down before running it. `create_kind` is the kind of row the change creates.
pub async fn record(
    pool: &SqlitePool,
    op_id: &str,
    method: &str,
    path: &str,
    body: &str,
    made_at: i64,
    create_kind: Option<&str>,
) -> Result<i64> {
    Ok(sqlx::query(
        "INSERT INTO edit_outbox (op_id, method, path, body, made_at, create_kind) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(op_id)
    .bind(method)
    .bind(path)
    .bind(body)
    .bind(made_at)
    .bind(create_kind)
    .execute(pool)
    .await?
    .last_insert_rowid())
}

/// The handler succeeded: the change is ready to send. `local_id` is the id of the row it created.
pub async fn confirm(pool: &SqlitePool, seq: i64, local_id: Option<i64>) -> Result<()> {
    sqlx::query("UPDATE edit_outbox SET ready = 1, local_id = ? WHERE seq = ?")
        .bind(local_id)
        .bind(seq)
        .execute(pool)
        .await?;
    Ok(())
}

/// The handler failed: nothing changed, so nothing to send.
pub async fn discard(pool: &SqlitePool, seq: i64) -> Result<()> {
    sqlx::query("DELETE FROM edit_outbox WHERE seq = ?")
        .bind(seq)
        .execute(pool)
        .await?;
    Ok(())
}

/// At start: a change whose handler was cut off by a crash may have been applied; keep it.
pub async fn recover(pool: &SqlitePool) -> Result<()> {
    sqlx::query("UPDATE edit_outbox SET ready = 1 WHERE ready = 0")
        .execute(pool)
        .await?;
    Ok(())
}

/// Changes waiting (including one whose handler is still running).
pub async fn count(pool: &SqlitePool) -> Result<u32> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM edit_outbox")
        .fetch_one(pool)
        .await?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

enum Rewrite {
    To(String),
    Skip(&'static str),
}

/// A phone path in the server's terms: ids of rows created on the phone become the server's ids
/// and an activity gets its start time next to its hash.
async fn rewrite(pool: &SqlitePool, method: &str, path: &str) -> Result<Rewrite> {
    let Some(edit) = Method::from_bytes(method.as_bytes())
        .ok()
        .and_then(|m| Edit::parse(&m, path))
    else {
        return Ok(Rewrite::To(path.to_owned()));
    };
    if let Edit::Strength(a) = edit {
        if a.start_ts.is_some() {
            return Ok(Rewrite::To(path.to_owned()));
        }
        let start: Option<i64> =
            sqlx::query_scalar("SELECT start_ts FROM activities WHERE sha256 = ?")
                .bind(a.sha256)
                .fetch_optional(pool)
                .await?
                .flatten();
        return Ok(Rewrite::To(
            Edit::Strength(crate::edits::ActivityRef {
                sha256: a.sha256,
                start_ts: start,
            })
            .path(),
        ));
    }
    let Some((kind, id)) = edit.row() else {
        return Ok(Rewrite::To(path.to_owned()));
    };
    let mapped: Option<Option<i64>> =
        sqlx::query_scalar("SELECT server_id FROM edit_id_map WHERE kind = ? AND local_id = ?")
            .bind(kind)
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(match mapped {
        None => Rewrite::To(path.to_owned()),
        Some(Some(server_id)) => Rewrite::To(edit.with_row(server_id).path()),
        Some(None) => Rewrite::Skip("the item it changes could not be saved on the server"),
    })
}

async fn drop_row(pool: &SqlitePool, seq: i64, now: i64, why: &str) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO edit_dropped (method, path, reason, dropped_at)
         SELECT method, path, ?, ? FROM edit_outbox WHERE seq = ?",
    )
    .bind(why)
    .bind(now)
    .bind(seq)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM edit_outbox WHERE seq = ?")
        .bind(seq)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// The oldest change, ready to send. Changes that name a row whose create was refused are dropped
/// on the way. `None` when nothing waits (or the oldest one is still being made).
pub async fn next(pool: &SqlitePool, now: i64) -> Result<Option<Pending>> {
    loop {
        let row: Option<(i64, String, String, String, String, i64, i64)> = sqlx::query_as(
            "SELECT seq, op_id, method, path, body, made_at, ready FROM edit_outbox
             ORDER BY seq LIMIT 1",
        )
        .fetch_optional(pool)
        .await?;
        let Some((seq, op_id, method, path, body, made_at, ready)) = row else {
            return Ok(None);
        };
        if ready == 0 {
            return Ok(None);
        }
        match rewrite(pool, &method, &path).await? {
            Rewrite::To(path) => {
                return Ok(Some(Pending {
                    op_id,
                    method,
                    path,
                    body,
                    made_at,
                }))
            }
            Rewrite::Skip(why) => drop_row(pool, seq, now, why).await?,
        }
    }
}

/// The server's words for a refusal: the `error` of its JSON answer, else the status.
fn reason_of(status: u16, body: &str) -> String {
    let text = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_owned))
        .unwrap_or_else(|| format!("the server answered {status}"));
    text.chars().filter(|c| !c.is_control()).take(200).collect()
}

/// Record the server's answer (`status` 0: there was none). 2xx: done, and a create's new id is
/// written to the id map. Other 4xx but 401, 403, 408 and 429: refused for good, dropped.
/// Everything else stays.
pub async fn done(
    pool: &SqlitePool,
    now: i64,
    op_id: &str,
    status: u16,
    response_body: &str,
) -> Result<Verdict> {
    let row: Option<(i64, Option<String>, Option<i64>)> =
        sqlx::query_as("SELECT seq, create_kind, local_id FROM edit_outbox WHERE op_id = ?")
            .bind(op_id)
            .fetch_optional(pool)
            .await?;
    let Some((seq, create_kind, local_id)) = row else {
        // Already recorded (an answer reported twice).
        return Ok(Verdict::Applied);
    };
    let ok = (200..300).contains(&status);
    let refused = (400..500).contains(&status) && ![401, 403, 408, 429].contains(&status);
    if !ok && !refused {
        return Ok(Verdict::Kept);
    }
    let mut tx = pool.begin().await?;
    if let (Some(kind), Some(local)) = (create_kind.as_deref(), local_id) {
        let server_id = ok
            .then(|| serde_json::from_str::<serde_json::Value>(response_body).ok())
            .flatten()
            .and_then(|v| v.get("id").and_then(|i| i.as_i64()));
        // A create that went through but whose answer has no id leaves no mapping: a later
        // change of that row then fails on the server and is dropped.
        if refused || server_id.is_some() {
            sqlx::query(
                "INSERT OR REPLACE INTO edit_id_map (kind, local_id, server_id) VALUES (?, ?, ?)",
            )
            .bind(kind)
            .bind(local)
            .bind(server_id)
            .execute(&mut *tx)
            .await?;
        }
    }
    if refused {
        sqlx::query(
            "INSERT INTO edit_dropped (method, path, reason, dropped_at)
             SELECT method, path, ?, ? FROM edit_outbox WHERE seq = ?",
        )
        .bind(reason_of(status, response_body))
        .bind(now)
        .bind(seq)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("DELETE FROM edit_outbox WHERE seq = ?")
        .bind(seq)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(if ok {
        Verdict::Applied
    } else {
        Verdict::Dropped
    })
}

/// The changes that could not be applied, once: they are forgotten as they are returned.
pub async fn take_dropped(pool: &SqlitePool) -> Result<Vec<Dropped>> {
    let mut tx = pool.begin().await?;
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT method, path, reason FROM edit_dropped ORDER BY id")
            .fetch_all(&mut *tx)
            .await?;
    sqlx::query("DELETE FROM edit_dropped")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(rows
        .into_iter()
        .map(|(method, path, reason)| Dropped {
            method,
            path,
            reason,
        })
        .collect())
}

/// Replace the user's settings, workouts, body entries and strength edits with the server's.
/// Nothing changes (`false`) while changes wait in the outbox: they are not on the server yet
/// and would be undone. The id map is cleared: the copy carries the server's ids again.
pub async fn apply_state(pool: &SqlitePool, uid: i64, state: &DeviceState) -> Result<bool> {
    if state.version != STATE_VERSION {
        bail!(
            "state version {} is not understood (this build reads {STATE_VERSION})",
            state.version
        );
    }
    // Immediate: the check and the replacement must see the same database.
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let waiting: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM edit_outbox")
        .fetch_one(&mut *tx)
        .await?;
    if waiting > 0 {
        return Ok(false);
    }
    for key in SETTING_KEYS {
        sqlx::query("DELETE FROM user_settings WHERE user_id = ? AND key = ?")
            .bind(uid)
            .bind(key)
            .execute(&mut *tx)
            .await?;
    }
    for (key, value) in &state.settings {
        // Only keys this build knows: the copy never holds anything the server did not mean.
        if SETTING_KEYS.contains(&key.as_str()) {
            sqlx::query("INSERT INTO user_settings (user_id, key, value) VALUES (?, ?, ?)")
                .bind(uid)
                .bind(key)
                .bind(value)
                .execute(&mut *tx)
                .await?;
        }
    }
    for table in ["workouts", "body_measurements", "strength_set_overrides"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE user_id = ?"
        )))
        .bind(uid)
        .execute(&mut *tx)
        .await?;
    }
    for w in &state.workouts {
        sqlx::query(
            "INSERT INTO workouts (id, user_id, name, sport, definition, revision, created_at,
                updated_at, send_state, send_requested_at, send_finished_at, send_error)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(w.id)
        .bind(uid)
        .bind(&w.name)
        .bind(&w.sport)
        .bind(&w.definition)
        .bind(w.revision)
        .bind(w.created_at)
        .bind(w.updated_at)
        .bind(&w.send_state)
        .bind(w.send_requested_at)
        .bind(w.send_finished_at)
        .bind(&w.send_error)
        .execute(&mut *tx)
        .await?;
    }
    for b in &state.body {
        sqlx::query(
            "INSERT INTO body_measurements (id, user_id, measured_at, weight_kg, body_fat_pct,
                waist_cm, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(b.id)
        .bind(uid)
        .bind(b.measured_at)
        .bind(b.weight_kg)
        .bind(b.body_fat_pct)
        .bind(b.waist_cm)
        .bind(b.created_at)
        .execute(&mut *tx)
        .await?;
    }
    for s in &state.strength {
        sqlx::query(
            "INSERT INTO strength_set_overrides (user_id, activity_ident, set_idx, fields, updated_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(uid)
        .bind(&s.activity_ident)
        .bind(s.set_idx)
        .bind(&s.fields)
        .bind(s.updated_at)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("DELETE FROM edit_id_map")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}
