//! Migration 0011 (Garmin Connect history) on a database at schema 0010 with data in `files` and
//! the tables that point at it. `files` is rebuilt (its `source` CHECK gains 'connect'): nothing
//! may be lost and no foreign key may break.

use nimrag_server::db;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

async fn old_pool(dir: &std::path::Path) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(dir.join(db::DB_FILE))
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let mut m = sqlx::migrate!("./migrations");
    m.migrations = m
        .migrations
        .iter()
        .filter(|x| x.version <= 10)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    m.run(&pool).await.unwrap();
    pool
}

async fn exec(pool: &SqlitePool, sql: &str) {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(pool)
        .await
        .unwrap();
}

async fn files_dump(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT quote(sha256) || '|' || quote(size) || '|' || quote(received_at) || '|' || quote(source)
            || '|' || quote(data_type) || '|' || quote(sub_type) || '|' || quote(file_index)
            || '|' || quote(file_number) || '|' || quote(watch_timestamp)
         FROM files ORDER BY sha256",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn files_are_rebuilt_without_losing_a_row_or_a_reference() {
    let dir = tempfile::tempdir().unwrap();
    let pool = old_pool(dir.path()).await;
    let shas: Vec<String> = ["a", "b", "c"].iter().map(|c| c.repeat(64)).collect();
    for (i, (sha, source)) in shas.iter().zip(["watch", "usb", "import"]).enumerate() {
        exec(
            &pool,
            &format!(
                "INSERT INTO files (sha256, size, received_at, source, data_type, sub_type, file_index, file_number, watch_timestamp)
                 VALUES ('{sha}', {}, '2026-11-19T00:00:0{i}.000Z', '{source}', 4, 1, {i}, 7, 12345)",
                100 + i
            ),
        )
        .await;
        exec(
            &pool,
            &format!(
                "INSERT INTO sources (sha256, src_id, ok, last_ts, msg_count, serial, time_created, file_type, rank_key)
                 VALUES ('{sha}', {}, 1, 1000, 5, 1, 2, 3, '1:0000001000:0000000005')",
                i + 1
            ),
        )
        .await;
        exec(
            &pool,
            &format!("INSERT INTO file_derive (sha256, derive_version, status, skipped_rows) VALUES ('{sha}', 1, 'ok', 0)"),
        )
        .await;
        exec(
            &pool,
            &format!(
                "INSERT INTO sleep_sessions (sha256, idx, start_ts, end_ts, overall_score)
                 VALUES ('{sha}', 0, {}, {}, 70)",
                1_000_000 + i * 100_000,
                1_030_000 + i * 100_000
            ),
        )
        .await;
        exec(
            &pool,
            &format!(
                "INSERT INTO activities (sha256, activity_key, start_ts, total_timer_s, sport, sport_name)
                 VALUES ('{sha}', 'k{i}', {}, 60.0, 1, 'running')",
                2_000_000 + i * 100_000
            ),
        )
        .await;
    }
    let before = files_dump(&pool).await;
    assert_eq!(before.len(), 3);
    pool.close().await;

    // `db::open` applies 0011.
    let pool = db::open(dir.path()).await.unwrap();
    let after = files_dump(&pool).await;
    assert_eq!(after, before, "every file row and column is kept");
    for t in ["sources", "file_derive", "sleep_sessions", "activities"] {
        assert_eq!(
            count(&pool, &format!("SELECT COUNT(*) FROM {t}")).await,
            3,
            "{t}"
        );
    }
    let broken = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(broken.is_empty(), "foreign keys intact: {}", broken.len());

    // Existing sources are watch files; the views still read them.
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM sources WHERE origin = 'fit'").await,
        3
    );
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM sleep_sessions_current").await,
        3
    );
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM activities_current WHERE origin = 'fit'"
        )
        .await,
        3
    );
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM sleep_sessions WHERE asleep_s IS NOT NULL"
        )
        .await,
        0
    );

    // 'connect' is accepted now, anything else still is not, and the key is still unique.
    exec(
        &pool,
        &format!("INSERT INTO files (sha256, size, received_at, source) VALUES ('{}', 1, 'x', 'connect')", "d".repeat(64)),
    )
    .await;
    assert!(sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO files (sha256, size, received_at, source) VALUES ('{}', 1, 'x', 'bogus')",
        "e".repeat(64)
    )))
    .execute(&pool)
    .await
    .is_err());
    assert!(sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO files (sha256, size, received_at, source) VALUES ('{}', 1, 'x', 'watch')",
        "d".repeat(64)
    )))
    .execute(&pool)
    .await
    .is_err());
    // A reference to a file that is not there is still refused.
    assert!(sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO file_derive (sha256, derive_version, status, skipped_rows) VALUES ('{}', 1, 'ok', 0)",
        "f".repeat(64)
    )))
    .execute(&pool)
    .await
    .is_err());
    // The temporary copy is gone.
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM sqlite_temp_master WHERE name = 'files_bak'"
        )
        .await,
        0
    );
}
