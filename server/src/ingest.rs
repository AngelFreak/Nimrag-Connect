//! The one ingest path: a received blob in the store's incoming dir becomes a
//! stored file, a sidecar event, ledger rows and (for FIT bytes) derived rows.
//! Used by the HTTP upload and by `import-export`.

use crate::api::AppState;
use crate::ledger::{self, Meta, Sidecar, UploadEvent};
use crate::{connect, derive, store};
use anyhow::{Context, Result};

/// Commit `blob`, record the upload event (sidecar first, then DB) and parse
/// FIT bytes (or a Garmin Connect JSON envelope, see [`connect`]) that have no parse result yet.
/// Returns true if the file is new.
/// `origin` (import only) is the archive-relative path; identical re-imports of
/// the same origin fold into the existing event.
///
/// `owner` is the user the file belongs to (the uploading token's user). A file has one owner:
/// the first user to hold it. A file nobody owns yet (an unassigned token's upload, or a file
/// from before multi-user) is claimed by `owner` and its derived rows move to that user; a file
/// someone else owns keeps its owner. `None` stores the file unowned.
pub async fn ingest(
    state: &AppState,
    blob: &store::Received,
    device: Option<(i64, String)>,
    owner: Option<i64>,
    meta: Meta,
    origin: Option<String>,
) -> Result<bool> {
    ingest_with(state, blob, device, owner, meta, origin, true).await
}

/// [`ingest`] with a choice on Forward: `forward == false` stores the file without queueing it
/// for Garmin Connect (a restore puts old files back; it must not upload them again).
pub async fn ingest_with(
    state: &AppState,
    blob: &store::Received,
    device: Option<(i64, String)>,
    owner: Option<i64>,
    meta: Meta,
    origin: Option<String>,
    forward: bool,
) -> Result<bool> {
    let store::Received { tmp, sha256, size } = blob;
    {
        let (dir, tmp, sha) = (state.data_dir.clone(), tmp.clone(), sha256.clone());
        tokio::task::spawn_blocking(move || store::commit(&dir, &tmp, &sha))
            .await
            .context("commit task")??;
    }

    // Parse when the bytes are FIT and the database has no parse result yet.
    // Decided from the DB, not from `created`, so a retry after a failure repairs.
    // Done outside the ledger lock.
    let mut parsed = None;
    let mut connect_parsed = None;
    let head = {
        let (dir, sha) = (state.data_dir.clone(), sha256.clone());
        let n = derive::SNIFF_LEN.max(connect::SNIFF_LEN);
        tokio::task::spawn_blocking(move || store::read_head(&dir, &sha, n))
            .await
            .context("sniff task")??
    };
    if connect::looks_like_connect(&head) {
        let done = sqlx::query_scalar::<_, i64>("SELECT 1 FROM file_derive WHERE sha256 = ?")
            .bind(sha256)
            .fetch_optional(&state.pool)
            .await?
            .is_some();
        if !done {
            let (dir, sha) = (state.data_dir.clone(), sha256.clone());
            connect_parsed = Some(
                tokio::task::spawn_blocking(move || -> anyhow::Result<connect::Parsed> {
                    Ok(connect::parse(&store::read_verified(&dir, &sha)?))
                })
                .await
                .context("connect parse task")??,
            );
        }
    } else if derive::looks_like_fit(&head) {
        let done = sqlx::query!(
            r#"SELECT 1 as "x!: i64" FROM file_parse WHERE sha256 = ?"#,
            sha256
        )
        .fetch_optional(&state.pool)
        .await?
        .is_some();
        if !done {
            let (dir, sha) = (state.data_dir.clone(), sha256.clone());
            parsed = Some(
                tokio::task::spawn_blocking(move || -> anyhow::Result<derive::Parsed> {
                    Ok(derive::parse(&store::read_verified(&dir, &sha)?))
                })
                .await
                .context("parse task")??,
            );
        }
    }

    let at = ledger::now(&state.pool).await?;
    let (device_id, device_name) = device.unzip();
    // The lock covers sidecar write and DB commit, so the sidecar and the
    // mirrored rows cannot interleave between concurrent ingests.
    let _guard = state.ledger_lock.lock().await;
    let existing = ledger::read_sidecar(&state.data_dir, sha256)?;
    let mut event = UploadEvent {
        at: at.clone(),
        device_id,
        device_name,
        meta: meta.clone(),
        created: existing.is_none(),
        repeat_count: 0,
        last_at: None,
        origin,
    };
    let mut sidecar = existing.unwrap_or_else(|| Sidecar {
        sha256: sha256.clone(),
        size: *size as i64,
        received_at: at.clone(),
        meta,
        uploads: vec![],
    });
    // An identical re-upload only bumps a counter, keeping sidecars bounded.
    // Imports match on origin anywhere in the history, so re-running an
    // import never appends events even if the file also lives elsewhere.
    let found = if event.origin.is_some() {
        sidecar.uploads.iter().rposition(|e| e.same_as(&event))
    } else {
        sidecar
            .uploads
            .len()
            .checked_sub(1)
            .filter(|&i| sidecar.uploads[i].same_as(&event))
    };
    let repeat = found.is_some();
    match found {
        Some(i) => {
            let e = &mut sidecar.uploads[i];
            e.repeat_count += 1;
            e.last_at = Some(at);
            event = e.clone();
        }
        None => sidecar.uploads.push(event.clone()),
    }
    ledger::write_sidecar(&state.data_dir, &sidecar)?;

    // Mirror the whole sidecar: a sidecar written by an earlier attempt whose
    // DB transaction failed is repaired here.
    let mut tx = state.pool.begin().await?;
    ledger::insert_file(&mut tx, &sidecar).await?;
    ledger::replace_uploads(&mut tx, &sidecar).await?;
    // The owner row goes in before the derived rows, which are partitioned by it.
    let mut claimed = false;
    if let Some(uid) = owner {
        claimed = sqlx::query("INSERT OR IGNORE INTO file_owners (sha256, user_id) VALUES (?, ?)")
            .bind(sha256)
            .bind(uid)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
    }
    if let Some(parsed) = &parsed {
        derive::write(&mut tx, sha256, parsed).await?;
    } else if let Some(parsed) = &connect_parsed {
        connect::write(&mut tx, sha256, parsed).await?;
    } else if let (true, Some(uid)) = (claimed, owner) {
        // Already parsed (it was stored before, unowned): its rows move from the unassigned
        // partition to the new owner, and the rank contests are decided again.
        derive::reparse_partitions(&mut tx, &state.data_dir, &[0, uid]).await?;
    }
    tx.commit().await?;
    drop(_guard);
    let created = event.created && !repeat;
    let is_fit = derive::looks_like_fit(&head);
    if forward {
        crate::garmin::on_stored(state, sha256, owner, &event.meta, is_fit, created).await;
    }
    Ok(created)
}
