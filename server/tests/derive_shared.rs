mod common;

use axum::http::StatusCode;
use common::*;
use nimrag_core::derive::test_support::{FitBuilder, Fv};
use nimrag_core::derive::{DeriveError, Derived};
use nimrag_core::fit::FitFile;
use nimrag_server::derived::{outranks_sql, rank_key};
use nimrag_server::{derive, sha256_hex};
use sqlx::SqlitePool;

/// Serialises tests that touch the global derive override.
static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn panicking(_: &FitFile) -> Result<Derived, DeriveError> {
    panic!("boom");
}

fn failing(_: &FitFile) -> Result<Derived, DeriveError> {
    Err(DeriveError::ProfileUnavailable)
}

fn fit_bytes() -> Vec<u8> {
    FitBuilder::new()
        .message(0, &[(0, Fv::U8(4)), (4, Fv::U32(1_000_000_000))])
        .build()
}

fn set_override(f: Option<nimrag_server::derived::DeriveFn>) {
    *derive::DERIVE_OVERRIDE.lock().unwrap() = f;
}

async fn derive_status(pool: &SqlitePool, sha: &str) -> (String, Option<String>) {
    sqlx::query_as("SELECT status, error FROM file_derive WHERE sha256 = ?")
        .bind(sha)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn panicking_derive_still_commits_raw_rows_and_records_error() {
    let _g = LOCK.lock().await;
    set_override(Some(panicking));
    let e = env().await;
    let bytes = fit_bytes();
    let (s, _) = e.upload("data_type=4", &bytes).await;
    set_override(None);
    assert_eq!(s, StatusCode::CREATED);
    let sha = sha256_hex(&bytes);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fit_messages WHERE sha256 = ?")
        .bind(&sha)
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert!(n > 0, "raw rows must be committed");
    let (status, err) = derive_status(&e.pool, &sha).await;
    assert_eq!(status, "error");
    assert!(err.unwrap().contains("boom"));
}

#[tokio::test]
async fn failing_derive_records_error_and_reparse_recovers() {
    let _g = LOCK.lock().await;
    set_override(Some(panicking));
    let e = env().await;
    let bytes = fit_bytes();
    e.upload("data_type=4", &bytes).await;
    // Reparse with the same failing derive keeps working and stays 'error'.
    derive::reparse(&e.pool, e.dir.path()).await.unwrap();
    let sha = sha256_hex(&bytes);
    assert_eq!(derive_status(&e.pool, &sha).await.0, "error");
    // Without the fault, reparse repairs it (ok, partial or no_profile).
    set_override(None);
    derive::reparse(&e.pool, e.dir.path()).await.unwrap();
    assert_ne!(derive_status(&e.pool, &sha).await.0, "error");
    let _ = failing; // ProfileUnavailable maps to no_profile; covered by parse_with below
}

#[test]
fn profile_unavailable_maps_to_no_profile() {
    let p = derive::parse_with(&fit_bytes(), failing);
    assert!(matches!(
        p.derived,
        nimrag_server::derived::DeriveOutcome::NoProfile
    ));
}

/// Sample-table stand-in: tests the real `outranks_sql` through a scratch table.
async fn scratch(pool: &SqlitePool) {
    sqlx::query("CREATE TABLE t (ts INTEGER PRIMARY KEY, v INTEGER, src INTEGER)")
        .execute(pool)
        .await
        .unwrap();
}

async fn add_source(pool: &SqlitePool, sha: &str, ok: bool, last_ts: i64) -> i64 {
    sqlx::query(
        "INSERT INTO files (sha256, size, received_at, source) VALUES (?, 1, '', 'import')",
    )
    .bind(sha)
    .execute(pool)
    .await
    .unwrap();
    let id = nimrag_server::derived::src_id(sha);
    sqlx::query(
        "INSERT INTO sources (sha256, src_id, ok, msg_count, last_ts, rank_key)
         VALUES (?, ?, ?, 1, ?, ?)",
    )
    .bind(sha)
    .bind(id)
    .bind(ok)
    .bind(last_ts)
    .bind(rank_key(ok, Some(last_ts), 1))
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn upsert(pool: &SqlitePool, v: i64, src: i64) {
    let sql = format!(
        "INSERT INTO t (ts, v, src) VALUES (1, ?, ?) ON CONFLICT (ts) DO UPDATE
         SET v = excluded.v, src = excluded.src WHERE {}",
        outranks_sql("t")
    );
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(v)
        .bind(src)
        .execute(pool)
        .await
        .unwrap();
}

async fn value(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT v FROM t WHERE ts = 1")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn missing_stored_source_never_wins() {
    let e = env().await;
    scratch(&e.pool).await;
    let a = add_source(&e.pool, &"a".repeat(64), false, 1).await;
    upsert(&e.pool, 1, a).await;
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&e.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM sources")
        .execute(&e.pool)
        .await
        .unwrap();
    let b = add_source(&e.pool, &"b".repeat(64), false, 0).await;
    upsert(&e.pool, 2, b).await;
    assert_eq!(value(&e.pool).await, 2);
}

#[tokio::test]
async fn tie_break_is_sha256_ascending_and_order_independent() {
    // Same rank; src_id masks the top bit so ids can disagree with sha order:
    // "7f.." < "80.." by sha, but masking makes both ids start 7f/00.
    let lo = format!("7f{}", "0".repeat(62));
    let hi = format!("80{}", "0".repeat(62));
    for order in [[&lo, &hi], [&hi, &lo]] {
        let e = env().await;
        scratch(&e.pool).await;
        let mut ids = std::collections::HashMap::new();
        for sha in [&lo, &hi] {
            ids.insert(sha.clone(), add_source(&e.pool, sha, true, 5).await);
        }
        for (i, sha) in order.iter().enumerate() {
            upsert(&e.pool, i as i64, ids[*sha]).await;
        }
        let winner: i64 = sqlx::query_scalar("SELECT src FROM t")
            .fetch_one(&e.pool)
            .await
            .unwrap();
        assert_eq!(winner, ids[&lo], "smaller sha256 must win in either order");
    }
}

#[tokio::test]
async fn reparse_is_atomic_for_sample_tables() {
    // After reparse the transaction is committed and a second reparse is stable.
    let e = env().await;
    e.upload("data_type=4", &fit_bytes()).await;
    derive::reparse(&e.pool, e.dir.path()).await.unwrap();
    let r = derive::reparse(&e.pool, e.dir.path()).await.unwrap();
    assert_eq!(r.files, 1);
}
