//! The nimrag server inside the app process (`nimrag_server::embedded`): the app's local
//! copy of the web interface and API on 127.0.0.1, fed in process with every stored watch
//! file. Blocking calls: Kotlin calls them off the main thread.

use std::path::Path;
use std::sync::{Arc, Mutex};

use nimrag_server::embedded::{self, Embedded};
use nimrag_server::ledger::Meta;

#[derive(Debug, uniffi::Error)]
#[uniffi(flat_error)]
pub enum LocalServerError {
    /// Starting, ingesting or reading failed; the text carries no secrets.
    Failed { reason: String },
    /// The server was stopped.
    Stopped,
}

impl std::fmt::Display for LocalServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed { reason } => write!(f, "{reason}"),
            Self::Stopped => write!(f, "the local server is stopped"),
        }
    }
}

fn failed(e: impl std::fmt::Display) -> LocalServerError {
    LocalServerError::Failed {
        reason: format!("{e:#}"),
    }
}

/// Watch-file metadata as the watch directory gave it (same fields as the HTTP upload).
#[derive(Debug, Clone, uniffi::Record)]
pub struct LocalFileMeta {
    pub data_type: u8,
    pub sub_type: u8,
    pub file_index: u32,
    pub file_number: u16,
    /// Raw Garmin-epoch seconds; 0 = none.
    pub watch_timestamp: i64,
}

/// A running local server. Create it once per process.
#[derive(uniffi::Object)]
pub struct LocalServer {
    inner: Mutex<Option<Arc<Embedded>>>,
    port: u16,
}

#[uniffi::export]
impl LocalServer {
    /// Opens `data_dir`, serves on `127.0.0.1:preferred_port` (any free port when 0 or taken).
    /// `secret` (at least 16 characters) must come back in the `nimrag_local` cookie.
    #[uniffi::constructor]
    pub fn start(
        data_dir: String,
        preferred_port: u16,
        secret: String,
    ) -> Result<Arc<Self>, LocalServerError> {
        let e = embedded::start(Path::new(&data_dir), preferred_port, &secret).map_err(failed)?;
        Ok(Arc::new(Self {
            port: e.port(),
            inner: Mutex::new(Some(Arc::new(e))),
        }))
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Name of the cookie the web view must carry (value: the secret).
    pub fn secret_cookie_name(&self) -> String {
        embedded::SECRET_COOKIE.to_string()
    }

    /// Stores a watch file and derives from it. True if the file was new. Idempotent.
    pub fn ingest_file(
        &self,
        bytes: Vec<u8>,
        meta: LocalFileMeta,
    ) -> Result<bool, LocalServerError> {
        let server = self.running()?;
        let meta = Meta {
            source: "watch".into(),
            data_type: Some(meta.data_type.into()),
            sub_type: Some(meta.sub_type.into()),
            file_index: Some(meta.file_index.into()),
            file_number: Some(meta.file_number.into()),
            watch_timestamp: (meta.watch_timestamp > 0).then_some(meta.watch_timestamp),
        };
        server.ingest_bytes(&bytes, meta).map_err(failed)
    }

    /// JSON counts (`files`, `derive` by status), for diagnostics.
    pub fn summary(&self) -> Result<String, LocalServerError> {
        self.running()?.summary().map_err(failed)
    }

    /// Stops serving and closes the database. Later calls fail with `Stopped`.
    pub fn stop(&self) {
        let taken = self.inner.lock().ok().and_then(|mut g| g.take());
        // Dropping the last handle stops the server (an in-flight call keeps it alive until done).
        drop(taken);
    }
}

impl LocalServer {
    fn running(&self) -> Result<Arc<Embedded>, LocalServerError> {
        self.inner
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .ok_or(LocalServerError::Stopped)
    }
}
