//! Persist `nimrag_core::derive::Derived` rows. One entry point, `persist`,
//! used by ingest and reparse alike.
//!
//! Entity tables (keyed by sha256) are replaced per file. Sample tables are keyed
//! by `(user_id, natural key)` with `src` = `sources.src_id` and are upserted only when the
//! new source outranks the stored one, so ingest order never matters. `user_id` is the owner
//! of the source file (`file_owners`; 0 when it has none), so two users' samples never collide.

pub mod activity;
pub mod wellness;

use anyhow::Result;
use nimrag_core::derive::{DeriveError, Derived, FileFacts};
use nimrag_core::fit::{Crc, Decoded, FitFile};
use sqlx::{Sqlite, Transaction};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Bump when the derivation or the stored representation changes.
pub const DERIVE_VERSION: i64 = 3;

/// Sample tables (natural key + `src`): cleared together at the start of a reparse.
/// Each module owns its list (`<module>::SAMPLE_TABLES`).
fn sample_tables() -> impl Iterator<Item = &'static str> {
    activity::SAMPLE_TABLES
        .iter()
        .chain(wellness::SAMPLE_TABLES)
        .copied()
}

/// Entity tables (PK or leading key `sha256`): replaced per file.
/// Each module owns its list (`<module>::ENTITY_TABLES`).
fn entity_tables() -> impl Iterator<Item = &'static str> {
    activity::ENTITY_TABLES
        .iter()
        .chain(wellness::ENTITY_TABLES)
        .copied()
}

/// What running the core derivation over a file produced.
#[derive(Debug)]
pub enum DeriveOutcome {
    /// One `Derived` per chain file (empty when the decoder found no files).
    Derived(Vec<Derived>),
    /// No generated FIT profile is compiled in.
    NoProfile,
    /// A derive returned an error or panicked; the message is stored.
    Failed(String),
}

/// Signature of the core derivation; a seam so tests can inject a failing one.
pub type DeriveFn = fn(&FitFile) -> Result<Derived, DeriveError>;

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

/// Run `derive_fn` over every chain file without ever panicking. Call from the
/// same blocking task as the FIT decode; the result goes to [`persist`].
pub fn derive_all(decoded: &Decoded, derive_fn: DeriveFn) -> DeriveOutcome {
    let mut out = Vec::with_capacity(decoded.files.len());
    for f in &decoded.files {
        match catch_unwind(AssertUnwindSafe(|| derive_fn(f))) {
            Ok(Ok(d)) => out.push(d),
            Ok(Err(DeriveError::ProfileUnavailable)) => return DeriveOutcome::NoProfile,
            Err(p) => {
                return DeriveOutcome::Failed(format!("derive panicked: {}", panic_message(p)))
            }
        }
    }
    DeriveOutcome::Derived(out)
}

/// src_id: first 8 bytes of the sha256, as a positive i64.
pub fn src_id(sha256: &str) -> i64 {
    (u64::from_str_radix(&sha256[..16], 16).unwrap_or(0) & (i64::MAX as u64)) as i64
}

/// Sortable text for the first three rank criteria (higher is better).
pub fn rank_key(ok: bool, last_ts: Option<i64>, msg_count: u64) -> String {
    format!(
        "{}:{:012}:{:012}",
        ok as u8,
        last_ts.unwrap_or(0).max(0),
        msg_count
    )
}

/// SQL condition for an upsert: the incoming row (`excluded.src`) outranks the
/// stored row (`<table>.src`). Final tie-break compares `sources.sha256`
/// directly (asc). A stored row whose `sources` entry is gone has rank '' and
/// loses to any present source.
pub fn outranks_sql(table: &str) -> String {
    let new = "(SELECT rank_key FROM sources WHERE src_id = excluded.src)";
    let old = format!("COALESCE((SELECT rank_key FROM sources WHERE src_id = {table}.src), '')");
    let new_sha = "(SELECT sha256 FROM sources WHERE src_id = excluded.src)";
    let old_sha = format!("(SELECT sha256 FROM sources WHERE src_id = {table}.src)");
    format!("{new} > {old} OR ({new} = {old} AND {new_sha} < {old_sha})")
}

/// A bindable column value.
#[derive(Debug, Clone)]
pub enum V {
    I(Option<i64>),
    R(Option<f64>),
    T(Option<String>),
}

/// Upsert rows into a sample table of the partition `uid`. Each row holds the key values
/// followed by the value columns; `user_id` and `src` are added. Updates only when the incoming
/// source outranks.
pub async fn upsert_samples(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    keys: &[&str],
    vals: &[&str],
    src: i64,
    uid: i64,
    rows: impl IntoIterator<Item = Vec<V>>,
) -> Result<()> {
    let cols: Vec<&str> = ["user_id"]
        .into_iter()
        .chain(keys.iter().copied())
        .chain(vals.iter().copied())
        .chain(["src"])
        .collect();
    let marks = vec!["?"; cols.len()].join(", ");
    let sets: Vec<String> = vals
        .iter()
        .chain(&["src"])
        .map(|c| format!("{c} = excluded.{c}"))
        .collect();
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({marks}) ON CONFLICT (user_id, {}) DO UPDATE SET {} WHERE {}",
        cols.join(", "),
        keys.join(", "),
        sets.join(", "),
        outranks_sql(table)
    );
    for row in rows {
        debug_assert_eq!(row.len(), keys.len() + vals.len());
        let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str())).bind(uid);
        for v in row {
            q = match v {
                V::I(x) => q.bind(x),
                V::R(x) => q.bind(x),
                V::T(x) => q.bind(x),
            };
        }
        q.bind(src).execute(&mut **tx).await?;
    }
    Ok(())
}

/// Empty the sample tables: all of them, or only the partitions `uids` (0 = unassigned).
pub async fn clear_samples(tx: &mut Transaction<'_, Sqlite>, uids: Option<&[i64]>) -> Result<()> {
    for t in sample_tables() {
        match uids {
            None => {
                sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {t}")))
                    .execute(&mut **tx)
                    .await?;
            }
            Some(uids) => {
                for uid in uids {
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "DELETE FROM {t} WHERE user_id = ?"
                    )))
                    .bind(uid)
                    .execute(&mut **tx)
                    .await?;
                }
            }
        }
    }
    Ok(())
}

/// Remove this file's entity rows, `sources` and `file_derive` rows.
pub async fn clear_file(tx: &mut Transaction<'_, Sqlite>, sha256: &str) -> Result<()> {
    for t in entity_tables() {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {t} WHERE sha256 = ?"
        )))
        .bind(sha256)
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query!("DELETE FROM file_derive WHERE sha256 = ?", sha256)
        .execute(&mut **tx)
        .await?;
    sqlx::query!("DELETE FROM sources WHERE sha256 = ?", sha256)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Store the derivation of one decoded file. Clears the file's own entity rows
/// first. A failed derive is recorded as `file_derive.status = 'error'` and
/// never fails the caller's transaction.
pub async fn persist(
    tx: &mut Transaction<'_, Sqlite>,
    sha256: &str,
    decoded: &Decoded,
    parse_error: Option<&str>,
    outcome: &DeriveOutcome,
) -> Result<()> {
    clear_file(tx, sha256).await?;
    if decoded.files.is_empty() {
        return Ok(()); // file_parse already records why
    }
    let (mut status, error, mut skipped) = ("ok", parse_error.map(str::to_owned), 0u64);
    if error.is_some() || decoded.files.iter().any(|f| !f.complete) {
        status = "partial";
    }
    let derived = match outcome {
        DeriveOutcome::Derived(d) => d,
        DeriveOutcome::NoProfile => {
            tracing::warn!(%sha256, "FIT profile unavailable: derived tables not filled");
            let msg = "no generated FIT profile compiled in";
            return write_file_derive(tx, sha256, "no_profile", Some(msg), 0).await;
        }
        DeriveOutcome::Failed(msg) => {
            tracing::error!(%sha256, error = %msg, "derive failed");
            return write_file_derive(tx, sha256, "error", Some(msg), 0).await;
        }
    };

    let mut facts = FileFacts::default();
    for d in derived {
        facts.merge(&d.facts);
        skipped += d.skipped;
    }
    let ok = decoded.files.iter().all(|f| f.complete && f.crc == Crc::Ok);
    let src = src_id(sha256);
    // Whose file this is: the partition its samples go to.
    let uid: i64 = sqlx::query_scalar("SELECT user_id FROM file_owners WHERE sha256 = ?")
        .bind(sha256)
        .fetch_optional(&mut **tx)
        .await?
        .unwrap_or(0);
    let key = rank_key(ok, facts.last_ts, facts.msg_count);
    let msg_count = facts.msg_count as i64;
    sqlx::query!(
        "INSERT INTO sources (sha256, src_id, ok, last_ts, msg_count, serial, time_created,
            file_type, rank_key) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        sha256,
        src,
        ok,
        facts.last_ts,
        msg_count,
        facts.serial,
        facts.time_created,
        facts.file_type,
        key
    )
    .execute(&mut **tx)
    .await?;
    for d in derived {
        wellness::write(tx, sha256, src, uid, d).await?;
        activity::write(tx, sha256, d).await?;
    }
    write_file_derive(tx, sha256, status, error.as_deref(), skipped).await
}

async fn write_file_derive(
    tx: &mut Transaction<'_, Sqlite>,
    sha256: &str,
    status: &str,
    error: Option<&str>,
    skipped: u64,
) -> Result<()> {
    let skipped = skipped as i64;
    sqlx::query!(
        "INSERT INTO file_derive (sha256, derive_version, status, error, skipped_rows)
         VALUES (?, ?, ?, ?, ?)",
        sha256,
        DERIVE_VERSION,
        status,
        error,
        skipped
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn src_id_is_positive_and_stable() {
        let id = src_id(&"f".repeat(64));
        assert_eq!(id, i64::MAX);
        assert!(src_id(&"0".repeat(64)) == 0);
        assert!(src_id("8000000000000001aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa") >= 0);
    }

    #[test]
    fn rank_key_orders_ok_then_last_ts_then_msg_count() {
        assert!(rank_key(true, Some(1), 1) > rank_key(false, Some(9), 9));
        assert!(rank_key(true, Some(2), 1) > rank_key(true, Some(1), 9));
        assert!(rank_key(true, Some(2), 3) > rank_key(true, Some(2), 2));
    }
}
