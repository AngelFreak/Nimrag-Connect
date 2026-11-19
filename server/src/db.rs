//! Database open + migrations. WAL, foreign keys on.

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use std::path::Path;

pub const DB_FILE: &str = "nimrag.db";

pub async fn open(data_dir: &Path) -> Result<SqlitePool> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("creating data dir {}", data_dir.display()))?;
    let options = SqliteConnectOptions::new()
        .filename(data_dir.join(DB_FILE))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(10))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
        .context("opening database")?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("running migrations")?;
    Ok(pool)
}

/// Exclusive advisory lock on `<data>/.lock`, held while the returned file lives.
/// `serve`, `import-export`, `reparse` and `rebuild-ledger` each take it, so two
/// writers never run on one data dir. The holder writes its command name into
/// the file so a refused process can say who has it.
pub fn lock_data_dir(data_dir: &Path, command: &str) -> Result<std::fs::File> {
    use std::fs::{File, TryLockError};
    use std::io::{Read, Write};
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("creating data dir {}", data_dir.display()))?;
    let path = data_dir.join(".lock");
    let mut f = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    match f.try_lock() {
        Ok(()) => {
            f.set_len(0)?;
            f.write_all(command.as_bytes())?;
            Ok(f)
        }
        Err(TryLockError::WouldBlock) => {
            let mut holder = String::new();
            let _ = f.read_to_string(&mut holder);
            let holder = holder.trim();
            anyhow::bail!(
                "data dir {} in use{}",
                data_dir.display(),
                if holder.is_empty() {
                    String::new()
                } else {
                    format!(" by `{holder}`")
                }
            )
        }
        Err(TryLockError::Error(e)) => Err(e).context("locking data dir"),
    }
}
