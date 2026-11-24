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

/// One change made on the local copy that the real server has not got yet, ready to send:
/// `POST /api/device/edits` with these fields as the JSON body.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PendingEdit {
    /// Unique per change; send the same one again when retrying.
    pub op_id: String,
    pub method: String,
    /// Already in the server's terms (its ids, the activity with its start time).
    pub path: String,
    /// The request body as JSON text; empty for none. Send it as a JSON string, as it is.
    pub body: String,
    /// Unix seconds when the change was made.
    pub made_at: i64,
}

/// What became of a change once the server answered (`LocalServer::edit_done`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum EditVerdict {
    /// Done: it is off the outbox. Ask for the next one.
    Applied,
    /// Refused for good: it is off the outbox and shows up in `take_dropped_edits`.
    Dropped,
    /// Not decided (no answer, server trouble, token refused): it stays. Back off, try later.
    Kept,
}

/// A change the server could not apply.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DroppedEdit {
    pub method: String,
    /// The path as the phone made it.
    pub path: String,
    /// Why, in the server's words.
    pub reason: String,
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

    /// The oldest change waiting for the real server, or `None`. Does not remove it: report the
    /// server's answer with `edit_done`. See `docs/design/offline-edits.md`.
    pub fn next_edit(&self) -> Result<Option<PendingEdit>, LocalServerError> {
        let next = self.running()?.next_edit().map_err(failed)?;
        Ok(next.map(|e| PendingEdit {
            op_id: e.op_id,
            method: e.method,
            path: e.path,
            body: e.body,
            made_at: e.made_at,
        }))
    }

    /// Report the real server's answer to a change: its HTTP status (0: no answer) and body.
    /// A 2xx is done (a create's new id is remembered for later changes of that row); a 4xx
    /// other than 401, 403, 408 and 429 drops the change; everything else keeps it.
    pub fn edit_done(
        &self,
        op_id: String,
        status: u16,
        response_body: String,
    ) -> Result<EditVerdict, LocalServerError> {
        let verdict = self
            .running()?
            .edit_done(&op_id, status, &response_body)
            .map_err(failed)?;
        Ok(match verdict {
            embedded::EditVerdict::Applied => EditVerdict::Applied,
            embedded::EditVerdict::Dropped => EditVerdict::Dropped,
            embedded::EditVerdict::Kept => EditVerdict::Kept,
        })
    }

    /// How many changes wait for the real server.
    pub fn pending_edits(&self) -> Result<u32, LocalServerError> {
        self.running()?.pending_edits().map_err(failed)
    }

    /// The changes the real server could not apply, once (they are forgotten as returned).
    pub fn take_dropped_edits(&self) -> Result<Vec<DroppedEdit>, LocalServerError> {
        let dropped = self.running()?.take_dropped_edits().map_err(failed)?;
        Ok(dropped
            .into_iter()
            .map(|d| DroppedEdit {
                method: d.method,
                path: d.path,
                reason: d.reason,
            })
            .collect())
    }

    /// Replace the copy's settings, workouts, body entries and strength edits with the body of
    /// `GET /api/device/state`. Returns false, changing nothing, while changes are waiting.
    pub fn apply_state(&self, json: String) -> Result<bool, LocalServerError> {
        self.running()?.apply_state(&json).map_err(failed)
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
