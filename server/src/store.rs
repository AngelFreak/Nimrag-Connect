//! Content-addressed file store, the source of truth.
//!
//! Layout under the data dir:
//!   files/<aa>/<bb>/<sha256>        the uploaded bytes, untouched
//!   files/<aa>/<bb>/<sha256>.json   sidecar: metadata + every upload event
//!   files/.incoming/                uploads being streamed (same filesystem)
//! Everything is written via temp file + fsync + rename.

use anyhow::{bail, Context, Result};
use axum::body::Body;
use http_body_util::{BodyExt, LengthLimitError, Limited};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncWriteExt;

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_name() -> String {
    format!(
        ".tmp-{}-{}",
        std::process::id(),
        TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

pub fn path_for(root: &Path, sha256: &str) -> PathBuf {
    root.join("files")
        .join(&sha256[0..2])
        .join(&sha256[2..4])
        .join(sha256)
}

pub fn sidecar_path_for(root: &Path, sha256: &str) -> PathBuf {
    let mut p = path_for(root, sha256).into_os_string();
    p.push(".json");
    p.into()
}

pub struct Received {
    pub tmp: PathBuf,
    pub sha256: String,
    pub size: u64,
}

pub enum ReceiveError {
    TooLarge,
    Other(anyhow::Error),
}

impl From<anyhow::Error> for ReceiveError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other(e)
    }
}

/// Stream a request body into a temp file inside the store (hashing as it
/// goes), enforcing `max` bytes, and fsync it. The temp file is removed on error.
pub async fn receive(root: &Path, body: Body, max: usize) -> Result<Received, ReceiveError> {
    let dir = root.join("files").join(".incoming");
    tokio::fs::create_dir_all(&dir)
        .await
        .context("creating incoming dir")?;
    let tmp = dir.join(unique_name());
    match stream_to(&tmp, body, max).await {
        Ok((sha256, size)) => Ok(Received { tmp, sha256, size }),
        Err(e) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            Err(e)
        }
    }
}

async fn stream_to(tmp: &Path, body: Body, max: usize) -> Result<(String, u64), ReceiveError> {
    let mut file = tokio::fs::File::create(tmp)
        .await
        .context("creating temp file")?;
    let mut body = Limited::new(body, max);
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|e| {
            if e.downcast_ref::<LengthLimitError>().is_some() {
                ReceiveError::TooLarge
            } else {
                ReceiveError::Other(anyhow::anyhow!("reading request body: {e}"))
            }
        })?;
        if let Ok(data) = frame.into_data() {
            hasher.update(&data);
            size += data.len() as u64;
            file.write_all(&data).await.context("writing temp file")?;
        }
    }
    file.sync_all().await.context("fsync temp file")?;
    Ok((crate::hex(&hasher.finalize()), size))
}

/// Blocking counterpart of [`receive`] for bytes already in memory (importer).
pub fn receive_bytes(root: &Path, bytes: &[u8]) -> Result<Received> {
    let dir = root.join("files").join(".incoming");
    fs::create_dir_all(&dir).context("creating incoming dir")?;
    let tmp = dir.join(unique_name());
    let written = (|| -> Result<()> {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Err(e).context("writing temp file");
    }
    Ok(Received {
        tmp,
        sha256: crate::sha256_hex(bytes),
        size: bytes.len() as u64,
    })
}

/// A fresh temp path inside the incoming dir (created if needed).
pub fn incoming_tmp(root: &Path) -> Result<PathBuf> {
    let dir = root.join("files").join(".incoming");
    fs::create_dir_all(&dir).context("creating incoming dir")?;
    Ok(dir.join(unique_name()))
}

/// Move a received temp file to its hash path. Returns true if the blob is new;
/// if it already exists the temp file is discarded.
pub fn commit(root: &Path, tmp: &Path, sha256: &str) -> Result<bool> {
    let dest = path_for(root, sha256);
    if dest.exists() {
        fs::remove_file(tmp)?;
        return Ok(false);
    }
    let dir = dest.parent().expect("hash path has a parent");
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    if let Err(e) = fs::rename(tmp, &dest) {
        let _ = fs::remove_file(tmp);
        return Err(e).with_context(|| format!("renaming into {}", dest.display()));
    }
    File::open(dir)?.sync_all()?;
    Ok(true)
}

/// Atomically write `bytes` to `dest` (temp file in the same dir, fsync, rename).
pub fn write_atomic(dest: &Path, bytes: &[u8]) -> Result<()> {
    let dir = dest.parent().expect("path has a parent");
    fs::create_dir_all(dir)?;
    let tmp = dir.join(unique_name());
    let written = (|| -> Result<()> {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, dest)?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("writing {}", dest.display()));
    }
    File::open(dir)?.sync_all()?;
    Ok(())
}

/// Read a stored file and verify its hash.
pub fn read_verified(root: &Path, sha256: &str) -> Result<Vec<u8>> {
    let path = path_for(root, sha256);
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let actual = crate::sha256_hex(&bytes);
    if actual != sha256 {
        bail!("{} is corrupt: content hashes to {actual}", path.display());
    }
    Ok(bytes)
}

/// Hashes of all sidecars in the store, sorted.
pub fn list_sidecars(root: &Path) -> Result<Vec<String>> {
    let mut out = vec![];
    let files = root.join("files");
    if !files.exists() {
        return Ok(out);
    }
    for a in fs::read_dir(&files)? {
        let a = a?.path();
        if !a.is_dir()
            || a.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        for b in fs::read_dir(&a)? {
            let b = b?.path();
            if !b.is_dir() {
                continue;
            }
            for f in fs::read_dir(&b)? {
                let name = f?.file_name().to_string_lossy().into_owned();
                if let Some(stem) = name.strip_suffix(".json") {
                    if crate::is_sha256_hex(stem) {
                        out.push(stem.to_string());
                    }
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

/// First `n` bytes of a stored file (fewer if it is shorter).
pub fn read_head(root: &Path, sha256: &str, n: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let path = path_for(root, sha256);
    let mut buf = Vec::with_capacity(n);
    File::open(&path)
        .and_then(|f| f.take(n as u64).read_to_end(&mut buf))
        .with_context(|| format!("reading {}", path.display()))?;
    Ok(buf)
}

/// Remove leftovers of interrupted uploads. Call at startup, before serving.
pub fn sweep_incoming(root: &Path) -> Result<usize> {
    let dir = root.join("files").join(".incoming");
    let mut n = 0;
    if dir.exists() {
        for e in fs::read_dir(&dir)? {
            fs::remove_file(e?.path())?;
            n += 1;
        }
    }
    Ok(n)
}
