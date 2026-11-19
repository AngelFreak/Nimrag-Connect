//! The ledger: per-file sidecars (source of truth) and the `files`/`uploads`
//! tables that mirror them. `rebuild` recreates the tables from the sidecars.

use crate::store;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::path::Path;

/// Watch/transport metadata exactly as received.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Meta {
    pub source: String,
    pub data_type: Option<i64>,
    pub sub_type: Option<i64>,
    pub file_index: Option<i64>,
    pub file_number: Option<i64>,
    pub watch_timestamp: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UploadEvent {
    pub at: String,
    pub device_id: Option<i64>,
    pub device_name: Option<String>,
    #[serde(flatten)]
    pub meta: Meta,
    pub created: bool,
    /// Identical re-uploads (same device and metadata) folded into this event.
    #[serde(default)]
    pub repeat_count: i64,
    #[serde(default)]
    pub last_at: Option<String>,
}

impl UploadEvent {
    /// True if `other` would be recorded as a repeat of this event.
    pub fn same_as(&self, other: &UploadEvent) -> bool {
        self.device_id == other.device_id && self.meta == other.meta
    }
}

/// `<sha256>.json`: the first upload's metadata plus every upload event, in order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sidecar {
    pub sha256: String,
    pub size: i64,
    pub received_at: String,
    #[serde(flatten)]
    pub meta: Meta,
    pub uploads: Vec<UploadEvent>,
}

pub fn read_sidecar(root: &Path, sha256: &str) -> Result<Option<Sidecar>> {
    let path = store::sidecar_path_for(root, sha256);
    match std::fs::read(&path) {
        Ok(b) => Ok(Some(
            serde_json::from_slice(&b).with_context(|| format!("parsing {}", path.display()))?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

pub fn write_sidecar(root: &Path, sc: &Sidecar) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(sc)?;
    bytes.push(b'\n');
    store::write_atomic(&store::sidecar_path_for(root, &sc.sha256), &bytes)
}

pub async fn now(pool: &SqlitePool) -> Result<String> {
    Ok(
        sqlx::query_scalar!(r#"SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now') as "t!: String""#)
            .fetch_one(pool)
            .await?,
    )
}

/// Insert the `files` row if absent. Returns true if inserted.
pub async fn insert_file(tx: &mut Transaction<'_, Sqlite>, sc: &Sidecar) -> Result<bool> {
    let m = &sc.meta;
    Ok(sqlx::query!(
        "INSERT OR IGNORE INTO files (sha256, size, received_at, source, data_type, sub_type,
            file_index, file_number, watch_timestamp) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        sc.sha256,
        sc.size,
        sc.received_at,
        m.source,
        m.data_type,
        m.sub_type,
        m.file_index,
        m.file_number,
        m.watch_timestamp
    )
    .execute(&mut **tx)
    .await?
    .rows_affected()
        == 1)
}

pub async fn insert_upload(
    tx: &mut Transaction<'_, Sqlite>,
    sha256: &str,
    ev: &UploadEvent,
) -> Result<()> {
    let m = &ev.meta;
    sqlx::query!(
        "INSERT INTO uploads (sha256, device_id, device_name, at, source, data_type, sub_type,
            file_index, file_number, watch_timestamp, created, repeat_count, last_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        sha256,
        ev.device_id,
        ev.device_name,
        ev.at,
        m.source,
        m.data_type,
        m.sub_type,
        m.file_index,
        m.file_number,
        m.watch_timestamp,
        ev.created,
        ev.repeat_count,
        ev.last_at
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Mirror a repeat into the last `uploads` row of this file. Returns false if
/// the file has no `uploads` row yet (an earlier ingest never committed).
pub async fn bump_last_upload(
    tx: &mut Transaction<'_, Sqlite>,
    sha256: &str,
    ev: &UploadEvent,
) -> Result<bool> {
    let n = sqlx::query!(
        "UPDATE uploads SET repeat_count = ?, last_at = ?
         WHERE id = (SELECT MAX(id) FROM uploads WHERE sha256 = ?)",
        ev.repeat_count,
        ev.last_at,
        sha256
    )
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(n > 0)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LedgerReport {
    pub sidecars: u64,
    pub files_added: u64,
    pub uploads: u64,
}

/// Recreate `files` and `uploads` rows from the sidecars (sha256 order, one
/// transaction). Existing `files` rows are kept; the `uploads` of each sidecar
/// are replaced by the sidecar's events. Never deletes anything else.
pub async fn rebuild(pool: &SqlitePool, root: &Path) -> Result<LedgerReport> {
    let mut report = LedgerReport::default();
    let mut tx = pool.begin().await?;
    for sha in store::list_sidecars(root)? {
        let sc = read_sidecar(root, &sha)?.context("sidecar vanished")?;
        if sc.sha256 != sha {
            bail!("sidecar {sha}.json claims hash {}", sc.sha256);
        }
        let blob = store::path_for(root, &sha);
        let len = std::fs::metadata(&blob)
            .with_context(|| format!("blob missing for sidecar {sha}"))?
            .len();
        if len as i64 != sc.size {
            bail!("blob {sha} has size {len}, sidecar says {}", sc.size);
        }
        report.sidecars += 1;
        report.files_added += insert_file(&mut tx, &sc).await? as u64;
        sqlx::query!("DELETE FROM uploads WHERE sha256 = ?", sha)
            .execute(&mut *tx)
            .await?;
        for ev in &sc.uploads {
            insert_upload(&mut tx, &sha, ev).await?;
            report.uploads += 1;
        }
    }
    tx.commit().await?;
    Ok(report)
}
