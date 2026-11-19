//! Persist `nimrag_core::derive::Derived` rows. One entry point, `persist`,
//! used by ingest and reparse alike.
//!
//! Entity tables (keyed by sha256) are replaced per file. Sample tables are keyed
//! by natural key with `src` = `sources.src_id` and are upserted only when the
//! new source outranks the stored one, so ingest order never matters.

pub mod activity;
pub mod wellness;

use anyhow::Result;
use nimrag_core::derive::{derive, FileFacts};
use nimrag_core::fit::{profile, Crc, Decoded};
use sqlx::{Sqlite, Transaction};

/// Bump when the derivation or the stored representation changes.
pub const DERIVE_VERSION: i64 = 1;

/// Sample tables (natural key + `src`): cleared together at the start of a reparse.
pub const SAMPLE_TABLES: &[&str] = wellness::SAMPLE_TABLES;
/// Entity tables (PK or leading key `sha256`): replaced per file.
pub const ENTITY_TABLES: &[&str] = wellness::ENTITY_TABLES;
// (activity::ENTITY_TABLES must be added here by the activity builder.)

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
/// stored row (`<table>.src`). Ties go to the smaller src_id (sha256 asc).
pub fn outranks_sql(table: &str) -> String {
    let new = "(SELECT rank_key FROM sources WHERE src_id = excluded.src)";
    let old = format!("(SELECT rank_key FROM sources WHERE src_id = {table}.src)");
    format!("{new} > {old} OR ({new} = {old} AND excluded.src < {table}.src)")
}

/// A bindable column value.
#[derive(Debug, Clone)]
pub enum V {
    I(Option<i64>),
    R(Option<f64>),
    T(Option<String>),
}

/// Upsert rows into a sample table. Each row holds the key values followed by the
/// value columns; `src` is appended. Updates only when the incoming source outranks.
pub async fn upsert_samples(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    keys: &[&str],
    vals: &[&str],
    src: i64,
    rows: impl IntoIterator<Item = Vec<V>>,
) -> Result<()> {
    let cols: Vec<&str> = keys.iter().chain(vals).copied().chain(["src"]).collect();
    let marks = vec!["?"; cols.len()].join(", ");
    let sets: Vec<String> = vals
        .iter()
        .chain(&["src"])
        .map(|c| format!("{c} = excluded.{c}"))
        .collect();
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({marks}) ON CONFLICT ({}) DO UPDATE SET {} WHERE {}",
        cols.join(", "),
        keys.join(", "),
        sets.join(", "),
        outranks_sql(table)
    );
    for row in rows {
        debug_assert_eq!(row.len(), keys.len() + vals.len());
        let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
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

pub async fn clear_samples(tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
    for t in SAMPLE_TABLES {
        sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {t}")))
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Remove this file's entity rows, `sources` and `file_derive` rows.
pub async fn clear_file(tx: &mut Transaction<'_, Sqlite>, sha256: &str) -> Result<()> {
    for t in ENTITY_TABLES {
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

/// Derive and store everything for one decoded file. Caller has cleared nothing:
/// this clears the file's own entity rows first.
pub async fn persist(
    tx: &mut Transaction<'_, Sqlite>,
    sha256: &str,
    decoded: &Decoded,
    parse_error: Option<&str>,
) -> Result<()> {
    clear_file(tx, sha256).await?;
    if decoded.files.is_empty() {
        return Ok(()); // file_parse already records why
    }
    let (mut status, mut error, mut skipped) = ("ok", parse_error.map(str::to_owned), 0u64);
    if error.is_some() || decoded.files.iter().any(|f| !f.complete) {
        status = "partial";
    }
    if !profile::available() {
        tracing::warn!(%sha256, "FIT profile unavailable: derived tables not filled");
        (status, error) = (
            "no_profile",
            Some("no generated FIT profile compiled in".into()),
        );
        return write_file_derive(tx, sha256, status, error.as_deref(), 0).await;
    }

    let mut facts = FileFacts::default();
    let mut derived = Vec::new();
    for f in &decoded.files {
        let d = derive(f).map_err(|e| anyhow::anyhow!("derive failed: {e:?}"))?;
        facts.merge(&d.facts);
        skipped += d.skipped;
        derived.push(d);
    }
    let ok = decoded.files.iter().all(|f| f.complete && f.crc == Crc::Ok);
    let src = src_id(sha256);
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
    for d in &derived {
        wellness::write(tx, sha256, src, d).await?;
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
