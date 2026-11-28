//! The phone's copy writes from two sides at once: the engine reads the refused changes while a
//! file from the watch is being ingested. Reading them must wait for that write, not fail.

use nimrag_server::{db, outbox};
use std::time::Duration;

async fn add_dropped(conn: &mut sqlx::SqliteConnection, path: &str) {
    sqlx::query(
        "INSERT INTO edit_dropped (method, path, reason, dropped_at) VALUES ('PUT', ?, 'gone', 0)",
    )
    .bind(path)
    .execute(conn)
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn taking_the_refused_changes_waits_for_a_write_in_progress() {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::open(dir.path()).await.unwrap();
    add_dropped(&mut *pool.acquire().await.unwrap(), "/api/workouts/1").await;

    // Another writer (the ingest of a watch file) holds the write lock and commits a change.
    let mut writer = pool.acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *writer)
        .await
        .unwrap();
    add_dropped(&mut writer, "/api/workouts/2").await;

    let taker = tokio::spawn({
        let pool = pool.clone();
        async move { outbox::take_dropped(&pool).await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    sqlx::query("COMMIT").execute(&mut *writer).await.unwrap();
    drop(writer);

    let taken = taker
        .await
        .unwrap()
        .expect("waits for the writer instead of failing");
    let paths: Vec<_> = taken.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(paths, ["/api/workouts/1", "/api/workouts/2"]);
    assert!(
        outbox::take_dropped(&pool).await.unwrap().is_empty(),
        "taken once"
    );
}
