//! The one ingest path: a received blob in the store's incoming dir becomes a
//! stored file, a sidecar event, ledger rows and (for FIT bytes) derived rows.
//! Used by the HTTP upload and by `import-export`.

use crate::api::AppState;
use crate::ledger::{self, Meta, Sidecar, UploadEvent};
use crate::{derive, store};
use anyhow::{Context, Result};

/// Commit `blob`, record the upload event (sidecar first, then DB) and parse
/// FIT bytes that have no parse result yet. Returns true if the file is new.
/// `origin` (import only) is the archive-relative path; identical re-imports of
/// the same origin fold into the existing event.
pub async fn ingest(
    state: &AppState,
    blob: &store::Received,
    device: Option<(i64, String)>,
    meta: Meta,
    origin: Option<String>,
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
    let head = {
        let (dir, sha) = (state.data_dir.clone(), sha256.clone());
        tokio::task::spawn_blocking(move || store::read_head(&dir, &sha, derive::SNIFF_LEN))
            .await
            .context("sniff task")??
    };
    if derive::looks_like_fit(&head) {
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
    if let Some(parsed) = &parsed {
        derive::write(&mut tx, sha256, parsed).await?;
    }
    tx.commit().await?;
    drop(_guard);
    let created = event.created && !repeat;
    let is_fit = derive::looks_like_fit(&head);
    crate::garmin::on_stored(state, sha256, &event.meta, is_fit, created).await;
    Ok(created)
}
