//! Wellness persistence through the real router and binary. Row-level tests need
//! the locally generated FIT profile; without it derivation must record
//! `no_profile` and leave the raw tables filled.
mod common;

use nimrag_core::derive::test_support::{FitBuilder, Fv};
use nimrag_server::derived::wellness::{ENTITY_TABLES, SAMPLE_TABLES};
use sqlx::{Row, SqlitePool};
use std::process::Command;

const T: u32 = 1_000_000_000;

fn hr_file(rows: &[(u32, u8)]) -> Vec<u8> {
    let mut b = FitBuilder::new();
    for (t, bpm) in rows {
        b = b.message(55, &[(253, Fv::U32(*t)), (27, Fv::U8(*bpm))]);
    }
    b.build()
}

fn corrupt_crc(mut f: Vec<u8>) -> Vec<u8> {
    let n = f.len();
    f[n - 1] ^= 0xFF;
    f
}

async fn upload(e: &common::Env, bytes: &[u8]) {
    let (s, _) = e.upload("data_type=128", bytes).await;
    assert_eq!(s, axum::http::StatusCode::CREATED);
}

/// Every derived wellness table, sources and file_derive, in deterministic order.
async fn dump(pool: &SqlitePool) -> String {
    let mut tables: Vec<&str> = SAMPLE_TABLES.iter().chain(ENTITY_TABLES).copied().collect();
    tables.extend(["sources", "file_derive"]);
    let mut out = String::new();
    for t in tables {
        let q = format!("SELECT * FROM {t} ORDER BY 1, 2");
        for row in sqlx::query(sqlx::AssertSqlSafe(q))
            .fetch_all(pool)
            .await
            .unwrap()
        {
            out.push_str(t);
            for i in 0..row.columns().len() {
                let v = row
                    .try_get::<Option<i64>, _>(i)
                    .map(|v| format!("{v:?}"))
                    .or_else(|_| row.try_get::<Option<f64>, _>(i).map(|v| format!("{v:?}")))
                    .or_else(|_| {
                        row.try_get::<Option<String>, _>(i)
                            .map(|v| format!("{v:?}"))
                    })
                    .unwrap();
                out.push('|');
                out.push_str(&v);
            }
            out.push('\n');
        }
    }
    out
}

async fn count(pool: &SqlitePool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

fn reparse(dir: &std::path::Path) {
    let out = Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .args(["reparse", "--data-dir"])
        .arg(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[tokio::test]
async fn without_profile_status_is_no_profile_and_raw_tables_fill() {
    if nimrag_core::fit::profile::available() {
        return; // covered by the with_profile tests
    }
    let e = common::env().await;
    upload(&e, &hr_file(&[(T, 60)])).await;
    let status: String = sqlx::query_scalar("SELECT status FROM file_derive")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(status, "no_profile");
    assert_eq!(count(&e.pool, "mon_hr").await, 0);
    assert_eq!(count(&e.pool, "sources").await, 0);
    assert!(count(&e.pool, "fit_messages").await > 0);
}

mod with_profile {
    use super::*;

    /// These tests need the generated profile; they pass vacuously without it.
    macro_rules! need_profile {
        () => {
            if !nimrag_core::fit::profile::available() {
                return;
            }
        };
    }

    #[tokio::test]
    async fn upload_fills_wellness_tables_through_the_router() {
        need_profile!();
        let e = common::env().await;
        let f = FitBuilder::new()
            .message(55, &[(253, Fv::U32(T)), (27, Fv::U8(60))])
            .message(
                55,
                &[(26, Fv::U16((T & 0xFFFF) as u16 + 60)), (27, Fv::U8(61))],
            )
            .message(
                55,
                &[
                    (253, Fv::U32(T + 120)),
                    (5, Fv::U8(6)),
                    (3, Fv::U32(40)),
                    (35, Fv::U32(1500)),
                ],
            )
            .message(227, &[(1, Fv::U32(T)), (0, Fv::S16(-2)), (3, Fv::S8(70))])
            .message(297, &[(253, Fv::U32(T)), (0, Fv::S16(1400))])
            .message(
                103,
                &[
                    (253, Fv::U32(T)),
                    (0, Fv::U32(T + 3600)),
                    (5, Fv::U16(1700)),
                ],
            )
            .message(55, &[(253, Fv::U32(1000)), (27, Fv::U8(99))]) // device-relative
            .build();
        upload(&e, &f).await;
        assert_eq!(count(&e.pool, "mon_hr").await, 2);
        assert_eq!(count(&e.pool, "mon_activity").await, 1);
        assert_eq!(count(&e.pool, "mon_extra").await, 1);
        assert_eq!(count(&e.pool, "stress").await, 1);
        assert_eq!(count(&e.pool, "respiration").await, 1);
        assert_eq!(count(&e.pool, "mon_info").await, 1);
        let (status, skipped): (String, i64) =
            sqlx::query_as("SELECT status, skipped_rows FROM file_derive")
                .fetch_one(&e.pool)
                .await
                .unwrap();
        assert_eq!((status.as_str(), skipped), ("ok", 1));
        let (stress, raw): (Option<f64>, f64) =
            sqlx::query_as("SELECT stress, stress_raw FROM stress")
                .fetch_one(&e.pool)
                .await
                .unwrap();
        assert_eq!((stress, raw), (None, -2.0));
        let (bpm, src): (f64, i64) = sqlx::query_as("SELECT bpm, src FROM mon_hr WHERE ts = ?")
            .bind(i64::from(T) + 631_065_600 + 60)
            .fetch_one(&e.pool)
            .await
            .unwrap();
        let sha: String = sqlx::query_scalar("SELECT sha256 FROM sources")
            .fetch_one(&e.pool)
            .await
            .unwrap();
        assert_eq!((bpm, src), (61.0, nimrag_server::derived::src_id(&sha)));
    }

    #[tokio::test]
    async fn every_wellness_table_gets_a_row() {
        need_profile!();
        let e = common::env().await;
        let f = FitBuilder::new()
            .message(
                55,
                &[
                    (253, Fv::U32(T)),
                    (27, Fv::U8(60)),
                    (5, Fv::U8(6)),
                    (3, Fv::U32(10)),
                    (35, Fv::U32(1000)),
                ],
            )
            .message(227, &[(1, Fv::U32(T)), (0, Fv::S16(30))])
            .message(297, &[(253, Fv::U32(T)), (0, Fv::S16(1500))])
            .message(269, &[(253, Fv::U32(T)), (0, Fv::U8(97))])
            .message(211, &[(253, Fv::U32(T)), (0, Fv::U8(52))])
            .message(103, &[(253, Fv::U32(T)), (5, Fv::U16(1700))])
            .message(275, &[(253, Fv::U32(T)), (0, Fv::U8(1))])
            .message(346, &[(6, Fv::U8(80))])
            .message(371, &[(253, Fv::U32(T)), (0, Fv::U16(6400))])
            .message(370, &[(253, Fv::U32(T)), (0, Fv::U16(6400))])
            .message(378, &[(253, Fv::U32(T)), (3, Fv::U16(300))])
            .message(232, &[(253, Fv::U32(T)), (2, Fv::U8(4))])
            .message(294, &[(253, Fv::U32(T)), (0, Fv::S32(65536))])
            .message(284, &[(0, Fv::U16(60)), (1, Fv::U32(T))])
            .message(
                229,
                &[(0, Fv::U32(T)), (1, Fv::S32(65536 * 12)), (5, Fv::U8(1))],
            )
            .build();
        upload(&e, &f).await;
        for t in SAMPLE_TABLES.iter().chain(ENTITY_TABLES) {
            assert!(count(&e.pool, t).await > 0, "no row in {t}");
        }
    }

    async fn ingest(files: &[&[u8]]) -> common::Env {
        let e = common::env().await;
        for f in files {
            upload(&e, f).await;
        }
        e
    }

    #[tokio::test]
    async fn longer_file_wins_in_either_order() {
        need_profile!();
        let prefix = hr_file(&[(T, 60), (T + 60, 61)]);
        let longer = hr_file(&[(T, 60), (T + 60, 62), (T + 120, 63)]);
        let a = ingest(&[&prefix, &longer]).await;
        let b = ingest(&[&longer, &prefix]).await;
        let q = "SELECT ts, bpm FROM mon_hr ORDER BY ts";
        let ra: Vec<(i64, f64)> = sqlx::query_as(q).fetch_all(&a.pool).await.unwrap();
        let rb: Vec<(i64, f64)> = sqlx::query_as(q).fetch_all(&b.pool).await.unwrap();
        assert_eq!(ra, rb);
        assert_eq!(
            ra.iter().map(|r| r.1).collect::<Vec<_>>(),
            vec![60.0, 62.0, 63.0]
        );
        assert_eq!(dump(&a.pool).await, dump(&b.pool).await);
    }

    #[tokio::test]
    async fn overlap_composite_key_and_multi_column_rows() {
        need_profile!();
        let act = |ts: u32, steps: u32, dist: Option<u32>| {
            let mut f = vec![(253, Fv::U32(ts)), (5, Fv::U8(6)), (3, Fv::U32(steps))];
            if let Some(d) = dist {
                f.push((2, Fv::U32(d)));
            }
            f
        };
        let prefix = FitBuilder::new()
            .message(55, &act(T, 10, Some(1000)))
            .message(
                227,
                &[
                    (1, Fv::U32(T)),
                    (0, Fv::S16(30)),
                    (3, Fv::S8(50)),
                    (4, Fv::S8(1)),
                ],
            )
            .build();
        let longer = FitBuilder::new()
            .message(55, &act(T, 12, None))
            .message(55, &act(T + 60, 5, Some(300)))
            .message(227, &[(1, Fv::U32(T)), (0, Fv::S16(35))])
            .message(227, &[(1, Fv::U32(T + 180)), (0, Fv::S16(40))])
            .build();
        let a = ingest(&[&prefix, &longer]).await;
        let b = ingest(&[&longer, &prefix]).await;
        assert_eq!(dump(&a.pool).await, dump(&b.pool).await);
        let (steps, dist): (f64, Option<f64>) =
            sqlx::query_as("SELECT steps, distance_m FROM mon_activity WHERE activity_type = 6 ORDER BY ts LIMIT 1")
                .fetch_one(&a.pool)
                .await
                .unwrap();
        assert_eq!(
            (steps, dist),
            (12.0, None),
            "longer file's NULL replaces the prefix value"
        );
        let (s, bb, st): (f64, Option<f64>, Option<i64>) = sqlx::query_as(
            "SELECT stress, body_battery, body_battery_status FROM stress ORDER BY ts LIMIT 1",
        )
        .fetch_one(&a.pool)
        .await
        .unwrap();
        assert_eq!((s, bb, st), (35.0, None, None));
        assert_eq!(count(&a.pool, "mon_activity").await, 2);
        assert_eq!(count(&a.pool, "stress").await, 2);
    }

    #[tokio::test]
    async fn bad_crc_file_loses_to_good_file() {
        need_profile!();
        let good = hr_file(&[(T, 60), (T + 60, 61)]);
        let bad = corrupt_crc(hr_file(&[(T, 99), (T + 60, 98), (T + 120, 97)]));
        let a = ingest(&[&good, &bad]).await;
        let b = ingest(&[&bad, &good]).await;
        assert_eq!(dump(&a.pool).await, dump(&b.pool).await);
        let rows: Vec<f64> = sqlx::query_scalar("SELECT bpm FROM mon_hr ORDER BY ts")
            .fetch_all(&a.pool)
            .await
            .unwrap();
        assert_eq!(&rows[..2], &[60.0, 61.0], "good file keeps the overlap");
    }

    #[tokio::test]
    async fn sleep_session_current_view_picks_best_file() {
        need_profile!();
        let sess = |score: u8, extra: bool| {
            let mut b = FitBuilder::new()
                .message(346, &[(6, Fv::U8(score))])
                .message(411, &[(10, Fv::U32(T)), (8, Fv::U32(T + 100))]);
            if extra {
                b = b.message(275, &[(253, Fv::U32(T)), (0, Fv::U8(2))]);
            }
            b.build()
        };
        let e = ingest(&[&sess(70, false), &sess(75, true)]).await;
        assert_eq!(count(&e.pool, "sleep_sessions").await, 2);
        let cur: Vec<f64> = sqlx::query_scalar("SELECT overall_score FROM sleep_sessions_current")
            .fetch_all(&e.pool)
            .await
            .unwrap();
        assert_eq!(cur, vec![75.0]); // more messages, same start
    }

    fn corpus() -> Vec<Vec<u8>> {
        vec![
            hr_file(&[(T, 60), (T + 60, 61)]),
            hr_file(&[(T, 60), (T + 60, 61), (T + 120, 62)]),
            corrupt_crc(hr_file(&[(T + 120, 80), (T + 180, 81)])),
            FitBuilder::new()
                .message(227, &[(1, Fv::U32(T)), (0, Fv::S16(30)), (3, Fv::S8(50))])
                .message(297, &[(253, Fv::U32(T)), (0, Fv::S16(1500))])
                .message(
                    103,
                    &[
                        (253, Fv::U32(T)),
                        (0, Fv::U32(T + 3600)),
                        (5, Fv::U16(1650)),
                    ],
                )
                .build(),
            FitBuilder::new()
                .message(346, &[(6, Fv::U8(80))])
                .message(275, &[(253, Fv::U32(T)), (0, Fv::U8(1))])
                .message(371, &[(253, Fv::U32(T)), (0, Fv::U16(6400))])
                .build(),
        ]
    }

    #[tokio::test]
    async fn determinism_across_orders_reparse_and_drop() {
        need_profile!();
        let c = corpus();
        let refs: Vec<&[u8]> = c.iter().map(Vec::as_slice).collect();
        let rev: Vec<&[u8]> = refs.iter().rev().copied().collect();
        let mid: Vec<&[u8]> = vec![refs[2], refs[0], refs[4], refs[1], refs[3]];
        let a = ingest(&refs).await;
        let b = ingest(&rev).await;
        let c2 = ingest(&mid).await;
        let before = dump(&a.pool).await;
        assert!(before.contains("mon_hr|") && before.contains("sleep_levels|"));
        assert_eq!(before, dump(&b.pool).await);
        assert_eq!(before, dump(&c2.pool).await);

        reparse(a.dir.path());
        assert_eq!(dump(&a.pool).await, before, "reparse");

        // Drop every derived wellness table's content, then rebuild.
        for t in SAMPLE_TABLES
            .iter()
            .chain(ENTITY_TABLES)
            .chain(&["file_derive", "sources"])
        {
            sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {t}")))
                .execute(&a.pool)
                .await
                .unwrap();
        }
        assert_eq!(count(&a.pool, "mon_hr").await, 0);
        reparse(a.dir.path());
        assert_eq!(dump(&a.pool).await, before, "reparse after drop");
    }

    /// Private captures: every 128 file derives with status ok. Prints row counts
    /// per table only, never values. Skipped when the captures are absent.
    #[tokio::test]
    async fn private_replay_of_128_files() {
        need_profile!();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../private/captures");
        let Ok(sessions) = std::fs::read_dir(&root) else {
            eprintln!("private captures absent; skipping");
            return;
        };
        let mut files = Vec::new();
        for s in sessions.flatten() {
            if let Ok(rd) = std::fs::read_dir(s.path().join("files")) {
                files.extend(rd.flatten().map(|f| f.path()).filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.contains("_128_"))
                }));
            }
        }
        files.sort();
        if files.is_empty() {
            return;
        }
        let e = common::env().await;
        for p in &files {
            // Identical captures repeat across sessions: 200 (known) is fine too.
            let (st, _) = e.upload("data_type=128", &std::fs::read(p).unwrap()).await;
            assert!(st.is_success(), "{st}");
        }
        let bad: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM file_derive WHERE status != 'ok'")
            .fetch_one(&e.pool)
            .await
            .unwrap();
        assert_eq!(bad, 0, "every file derives ok");
        // Skipped rows are dropped non-step `cycles` values (see monitoring.rs);
        // reported, not asserted zero.
        let skipped: i64 =
            sqlx::query_scalar("SELECT COALESCE(SUM(skipped_rows), 0) FROM file_derive")
                .fetch_one(&e.pool)
                .await
                .unwrap();
        eprintln!("replay skipped rows total: {skipped}");
        let n: i64 = count(&e.pool, "file_derive").await;
        eprintln!("replay: {} files uploaded, {n} derived", files.len());
        for t in SAMPLE_TABLES.iter().chain(ENTITY_TABLES) {
            eprintln!("replay rows {t}: {}", count(&e.pool, t).await);
        }
    }
}
