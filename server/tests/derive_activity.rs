//! Activity persistence through the real router: rows, no-profile behaviour,
//! file ranking (`activities_current`), ingest-order independence and reparse.
mod common;

use axum::http::StatusCode;
use common::*;
use nimrag_core::derive::test_support::{FitBuilder, Fv};
use nimrag_core::fit::profile;
use nimrag_server::{derive, sha256_hex};
use sqlx::{Row, SqlitePool};

const T0: u32 = 0x3000_0000;
const TABLES: &[&str] = &[
    "activities",
    "activity_sessions",
    "activity_laps",
    "activity_records",
    "strength_sets",
    "exercise_titles",
    "activity_time_in_zone",
    "activity_metrics",
];

/// Activity file with `records` record messages. Same serial/time_created =
/// same activity_key.
fn activity(serial: u32, records: u32) -> Vec<u8> {
    let mut b = FitBuilder::new().message(0, &[(3, Fv::U32(serial)), (4, Fv::U32(T0 - 100))]);
    for i in 0..records {
        b = b.message(
            20,
            &[
                (253, Fv::U32(T0 + i)),
                (3, Fv::U8(100 + i as u8)),
                (5, Fv::U32(100 * i)),
            ],
        );
    }
    b.message(
        18,
        &[
            (253, Fv::U32(T0 + 20)),
            (2, Fv::U32(T0)),
            (5, Fv::U8(1)),
            (7, Fv::U32(20_000)),
            (9, Fv::U32(10_000)),
        ],
    )
    .message(
        34,
        &[
            (253, Fv::U32(T0 + 30)),
            (5, Fv::U32(T0 + 30 + 3600)),
            (1, Fv::U16(1)),
        ],
    )
    .build()
}

fn sets_file() -> Vec<u8> {
    FitBuilder::new()
        .message(0, &[(3, Fv::U32(77)), (4, Fv::U32(T0))])
        .message(
            225,
            &[
                (254, Fv::U32(T0)),
                (3, Fv::U16(8)),
                (4, Fv::U16(160)),
                (7, Fv::U16(0)),
            ],
        )
        .message(
            225,
            &[
                (254, Fv::U32(T0 + 60)),
                (3, Fv::U16(5)),
                (4, Fv::U16(0xFFFF)),
            ],
        )
        .build()
}

async fn count(pool: &SqlitePool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Deterministic dump of every activity table and view (all columns quoted).
async fn dump_activity(pool: &SqlitePool) -> String {
    let mut out = String::new();
    let mut names: Vec<&str> = TABLES.to_vec();
    names.push("activities_current");
    for t in names {
        let cols: Vec<String> = sqlx::query(sqlx::AssertSqlSafe(format!("PRAGMA table_info({t})")))
            .fetch_all(pool)
            .await
            .unwrap()
            .iter()
            .map(|r| r.get::<String, _>("name"))
            .collect();
        let expr = cols
            .iter()
            .map(|c| format!("quote({c})"))
            .collect::<Vec<_>>()
            .join(" || '|' || ");
        let sql = format!("SELECT {expr} FROM {t} ORDER BY {expr}");
        for r in sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(pool)
            .await
            .unwrap()
        {
            out.push_str(&format!("{t}|{}\n", r.get::<String, _>(0)));
        }
    }
    out
}

async fn status(pool: &SqlitePool, sha: &str) -> String {
    sqlx::query_scalar("SELECT status FROM file_derive WHERE sha256 = ?")
        .bind(sha)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn ingest_fills_activity_tables_or_reports_no_profile() {
    let e = env().await;
    let bytes = activity(1, 3);
    let (s, _) = e.upload("data_type=4", &bytes).await;
    assert_eq!(s, StatusCode::CREATED);
    let sha = sha256_hex(&bytes);
    if profile::available() {
        assert_eq!(status(&e.pool, &sha).await, "ok");
        assert_eq!(count(&e.pool, "activities").await, 1);
        assert_eq!(count(&e.pool, "activity_sessions").await, 1);
        assert_eq!(count(&e.pool, "activity_records").await, 3);
        let (key, sport, name, off): (String, i64, String, i64) = sqlx::query_as(
            "SELECT activity_key, sport, sport_name, local_offset_s FROM activities WHERE sha256 = ?",
        )
        .bind(&sha)
        .fetch_one(&e.pool)
        .await
        .unwrap();
        assert_eq!(key, format!("1:{}", i64::from(T0) - 100 + 631_065_600));
        assert_eq!((sport, name.as_str(), off), (1, "running", 3600));
        let dist: f64 =
            sqlx::query_scalar("SELECT distance_m FROM activity_sessions WHERE sha256 = ?")
                .bind(&sha)
                .fetch_one(&e.pool)
                .await
                .unwrap();
        assert_eq!(dist, 100.0);
    } else {
        assert_eq!(status(&e.pool, &sha).await, "no_profile");
        for t in TABLES {
            assert_eq!(
                count(&e.pool, t).await,
                0,
                "{t} must stay empty without a profile"
            );
        }
    }
}

#[tokio::test]
async fn strength_set_invalid_weight_is_null() {
    if !profile::available() {
        return;
    }
    let e = env().await;
    e.upload("data_type=4", &sets_file()).await;
    let rows: Vec<(i64, Option<f64>, Option<f64>)> =
        sqlx::query_as("SELECT idx, weight_kg, reps FROM strength_sets ORDER BY idx")
            .fetch_all(&e.pool)
            .await
            .unwrap();
    assert_eq!(rows, vec![(0, Some(10.0), Some(8.0)), (1, None, Some(5.0))]);
}

#[tokio::test]
async fn current_view_picks_best_file_regardless_of_ingest_order() {
    if !profile::available() {
        return;
    }
    let small = activity(5, 1);
    let big = activity(5, 4);
    let other = activity(6, 2);
    let (sha_big, sha_small) = (sha256_hex(&big), sha256_hex(&small));
    let orders: [&[&Vec<u8>]; 3] = [
        &[&small, &big, &other],
        &[&big, &other, &small],
        &[&other, &small, &big],
    ];
    let mut dumps = Vec::new();
    for order in orders {
        let e = env().await;
        for b in order {
            let (s, _) = e.upload("data_type=4", b).await;
            assert_eq!(s, StatusCode::CREATED);
        }
        let cur: Vec<String> =
            sqlx::query_scalar("SELECT sha256 FROM activities_current ORDER BY activity_key")
                .fetch_all(&e.pool)
                .await
                .unwrap();
        assert_eq!(cur.len(), 2);
        assert!(cur.contains(&sha_big), "more complete file wins");
        assert!(!cur.contains(&sha_small));
        assert_eq!(
            count(&e.pool, "activities").await,
            3,
            "every file keeps its own rows"
        );
        dumps.push(dump_activity(&e.pool).await);
    }
    assert_eq!(dumps[0], dumps[1]);
    assert_eq!(dumps[1], dumps[2]);
}

#[tokio::test]
async fn reparse_and_reparse_after_drop_give_identical_tables() {
    let e = env().await;
    e.upload("data_type=4", &activity(9, 3)).await;
    e.upload("data_type=4", &sets_file()).await;
    let before = dump_activity(&e.pool).await;
    derive::reparse(&e.pool, e.dir.path()).await.unwrap();
    assert_eq!(dump_activity(&e.pool).await, before);
    for t in TABLES {
        sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {t}")))
            .execute(&e.pool)
            .await
            .unwrap();
    }
    derive::reparse(&e.pool, e.dir.path()).await.unwrap();
    assert_eq!(dump_activity(&e.pool).await, before);
}

/// One file exercising every message type the activity parser knows.
fn everything_file() -> Vec<u8> {
    FitBuilder::new()
        .message(0, &[(3, Fv::U32(42)), (4, Fv::U32(T0 - 100))])
        .message(20, &[(253, Fv::U32(T0)), (3, Fv::U8(120))])
        .message(
            19,
            &[(253, Fv::U32(T0 + 5)), (2, Fv::U32(T0)), (9, Fv::U32(5000))],
        )
        .message(
            18,
            &[(253, Fv::U32(T0 + 10)), (2, Fv::U32(T0)), (5, Fv::U8(10))],
        )
        .message(34, &[(253, Fv::U32(T0 + 20)), (1, Fv::U16(1))])
        .message(
            225,
            &[(254, Fv::U32(T0)), (3, Fv::U16(8)), (4, Fv::U16(160))],
        )
        .message(
            264,
            &[
                (254, Fv::U16(0)),
                (0, Fv::U16(0)),
                (1, Fv::U16(3)),
                (2, Fv::Str("Bench", 6)),
            ],
        )
        .message(
            216,
            &[(253, Fv::U32(T0)), (0, Fv::U16(18)), (2, Fv::U32(1000))],
        )
        .message(
            140,
            &[(253, Fv::U32(T0)), (7, Fv::S32(50 << 16)), (4, Fv::U8(35))],
        )
        .build()
}

#[tokio::test]
async fn every_activity_table_receives_a_row() {
    if !profile::available() {
        return;
    }
    let e = env().await;
    e.upload("data_type=4", &everything_file()).await;
    for t in TABLES {
        assert!(count(&e.pool, t).await > 0, "{t} got no row");
    }
}

#[tokio::test]
async fn file_without_activity_message_is_still_listed() {
    if !profile::available() {
        return;
    }
    let e = env().await;
    let bytes = FitBuilder::new()
        .message(0, &[(3, Fv::U32(8)), (4, Fv::U32(T0))])
        .message(20, &[(253, Fv::U32(T0)), (3, Fv::U8(90))])
        .message(140, &[(253, Fv::U32(T0)), (4, Fv::U8(35))])
        .build();
    e.upload("data_type=4", &bytes).await;
    let sha = sha256_hex(&bytes);
    let cur: Vec<String> = sqlx::query_scalar("SELECT sha256 FROM activities_current")
        .fetch_all(&e.pool)
        .await
        .unwrap();
    assert_eq!(cur, vec![sha.clone()]);
    let ts: Option<i64> = sqlx::query_scalar("SELECT ts FROM activities WHERE sha256 = ?")
        .bind(&sha)
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(ts, None, "activity-message columns stay NULL");
}

#[tokio::test]
async fn standalone_firstbeat_file_gets_no_activity_rows() {
    if !profile::available() {
        return;
    }
    let e = env().await;
    let bytes = FitBuilder::new()
        .message(140, &[(253, Fv::U32(T0)), (4, Fv::U8(35))])
        .build();
    e.upload("data_type=4", &bytes).await;
    assert_eq!(count(&e.pool, "activities").await, 0);
    assert_eq!(count(&e.pool, "activity_metrics").await, 0);
}
