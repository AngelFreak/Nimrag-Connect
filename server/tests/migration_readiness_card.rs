//! Migration 0019: a saved home layout that hid the readiness card (the old default) shows it
//! again, right after Energy. Other layouts and other settings stay as they were.

use nimrag_server::db;
use serde_json::{json, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

async fn pool_at(dir: &std::path::Path) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(dir.join(db::DB_FILE))
        .create_if_missing(true)
        .foreign_keys(true);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap()
}

async fn migrate(pool: &SqlitePool, up_to: i64) {
    let mut m = sqlx::migrate!("./migrations");
    m.migrations = m
        .migrations
        .iter()
        .filter(|x| x.version <= up_to)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    m.run(pool).await.unwrap();
}

async fn put(pool: &SqlitePool, user: i64, key: &str, value: &str) {
    sqlx::query("INSERT INTO user_settings (user_id, key, value) VALUES (?, ?, ?)")
        .bind(user)
        .bind(key)
        .bind(value)
        .execute(pool)
        .await
        .unwrap();
}

async fn get(pool: &SqlitePool, user: i64, key: &str) -> String {
    sqlx::query_scalar("SELECT value FROM user_settings WHERE user_id = ? AND key = ?")
        .bind(user)
        .bind(key)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn hidden_readiness_card_comes_back_after_energy() {
    let dir = tempfile::tempdir().unwrap();
    let pool = pool_at(dir.path()).await;
    migrate(&pool, 18).await;

    // The owner's layout as saved before the estimate existed.
    let old = json!({
        "order": ["energy", "heart_rate", "sleep", "steps", "stress", "hrv", "training_status",
                  "intensity_minutes", "training_load", "recovery_time", "vo2_max", "respiration",
                  "spo2", "weight", "training_readiness"],
        "hidden": ["respiration", "spo2", "weight", "training_readiness"]
    });
    put(&pool, 2, "home_cards", &old.to_string()).await;
    // Energy moved down, readiness hidden: readiness lands right after Energy.
    let moved = json!({
        "order": ["sleep", "training_readiness", "heart_rate", "energy", "steps"],
        "hidden": ["training_readiness"]
    });
    put(&pool, 3, "home_cards", &moved.to_string()).await;
    // A layout that shows readiness, one that is not the expected shape, another setting.
    let shown = json!({"order": ["training_readiness", "energy"], "hidden": ["weight"]});
    put(&pool, 4, "home_cards", &shown.to_string()).await;
    put(&pool, 5, "home_cards", "{\"cards\":[]}").await;
    put(&pool, 2, "timezone", "Europe/Copenhagen").await;

    migrate(&pool, i64::MAX).await;

    let owner: Value = serde_json::from_str(&get(&pool, 2, "home_cards").await).unwrap();
    assert_eq!(
        owner,
        json!({
            "order": ["energy", "training_readiness", "heart_rate", "sleep", "steps", "stress",
                      "hrv", "training_status", "intensity_minutes", "training_load",
                      "recovery_time", "vo2_max", "respiration", "spo2", "weight"],
            "hidden": ["respiration", "spo2", "weight"]
        })
    );
    let moved: Value = serde_json::from_str(&get(&pool, 3, "home_cards").await).unwrap();
    assert_eq!(
        moved,
        json!({
            "order": ["sleep", "heart_rate", "energy", "training_readiness", "steps"],
            "hidden": []
        })
    );
    let shown_after: Value = serde_json::from_str(&get(&pool, 4, "home_cards").await).unwrap();
    assert_eq!(shown_after, shown);
    assert_eq!(get(&pool, 5, "home_cards").await, "{\"cards\":[]}");
    assert_eq!(get(&pool, 2, "timezone").await, "Europe/Copenhagen");
}
