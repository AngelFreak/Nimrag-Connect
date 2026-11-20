//! The server as a library inside another process (the Android app).
//!
//! [`start`] opens the data dir, serves the same router as `nimrag-server serve` on
//! `127.0.0.1` and returns a handle. Differences from `serve`:
//!
//! * No login. Every request must carry the launch secret in the `nimrag_local` cookie
//!   (other apps on the phone can reach 127.0.0.1, the secret keeps them out: they get 403).
//!   A request that passes also gets a server-side session injected, so the owner API
//!   works unchanged. The Host header must be the loopback address (DNS rebinding).
//! * The owner password is a random value nobody knows (so the web app never offers
//!   first-run setup) and the session is created at start.
//! * No Forward worker: nothing leaves the phone from here.
//! * Read-only over HTTP: every method but GET and HEAD gets 403 (edits would never reach the
//!   real server).
//! * Files are put in with [`Embedded::ingest_bytes`] (in process, same path as the HTTP
//!   upload: store, sidecar, ledger, derive).
//!
//! The handle owns its own small tokio runtime, so callers need no async.

use crate::api::{self, ApiError, AppState};
use crate::ledger::Meta;
use crate::{auth, db, ingest, store};
use anyhow::{bail, Context, Result};
use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Name of the cookie that carries the launch secret.
pub const SECRET_COOKIE: &str = "nimrag_local";
/// Shortest secret accepted.
pub const MIN_SECRET_LEN: usize = 16;

/// A running in-process server. Dropping it stops the server.
pub struct Embedded {
    port: u16,
    rt: Option<tokio::runtime::Runtime>,
    state: AppState,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    _lock: std::fs::File,
}

struct Guard {
    secret: String,
    session: String,
    hosts: [String; 2],
}

/// Start the server on `127.0.0.1:preferred_port` (any free port if 0 or taken).
/// The data dir is created if needed and locked against a second writer.
pub fn start(data_dir: &Path, preferred_port: u16, secret: &str) -> Result<Embedded> {
    if secret.len() < MIN_SECRET_LEN {
        bail!("the secret must have at least {MIN_SECRET_LEN} characters");
    }
    let lock = db::lock_data_dir(data_dir, "embedded")?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("nimrag-local")
        .enable_all()
        .build()
        .context("starting the async runtime")?;
    let (shutdown, stop) = tokio::sync::oneshot::channel::<()>();
    let data_dir = data_dir.to_path_buf();
    let secret = secret.to_owned();
    let (state, port) = rt.block_on(async {
        let pool = db::open(&data_dir).await?;
        store::sweep_incoming(&data_dir)?;
        let state = AppState::new(pool, data_dir, api::MAX_BODY_BYTES).with_insecure_cookies(true);
        // Nobody knows this password; it only stops the web app from offering first-run setup.
        if auth::owner_hash(&state.pool).await?.is_none() {
            let mut raw = [0u8; 24];
            getrandom::fill(&mut raw).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
            auth::claim_owner(&state.pool, &crate::hex(&raw), state.now()).await?;
        }
        let session = auth::create_session(&state.pool, state.now()).await?;

        let listener = match preferred_port {
            0 => None,
            p => tokio::net::TcpListener::bind(("127.0.0.1", p)).await.ok(),
        };
        let listener = match listener {
            Some(l) => l,
            None => tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .context("binding 127.0.0.1")?,
        };
        let port = listener.local_addr()?.port();
        let guard = Arc::new(Guard {
            secret,
            session,
            hosts: [format!("127.0.0.1:{port}"), format!("localhost:{port}")],
        });
        let app = api::router(state.clone()).layer(middleware::from_fn_with_state(guard, guard_fn));
        tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = stop.await;
            })
            .await;
        });
        anyhow::Ok((state, port))
    })?;
    Ok(Embedded {
        port,
        rt: Some(rt),
        state,
        shutdown: Some(shutdown),
        _lock: lock,
    })
}

impl Embedded {
    /// The bound port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Store `bytes` as a watch file and derive from it. Returns true if the file is new.
    /// Blocking; never call it from inside an async context.
    pub fn ingest_bytes(&self, bytes: &[u8], meta: Meta) -> Result<bool> {
        let received = store::receive_bytes(&self.state.data_dir, bytes)?;
        self.rt()
            .block_on(ingest::ingest(&self.state, &received, None, meta, None))
    }

    /// JSON `{"files": n, "derive": {"ok": n, "no_profile": n, ...}}` for diagnostics.
    pub fn summary(&self) -> Result<String> {
        self.rt().block_on(async {
            let files: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
                .fetch_one(&self.state.pool)
                .await?;
            let rows: Vec<(String, i64)> =
                sqlx::query_as("SELECT status, COUNT(*) FROM file_derive GROUP BY status")
                    .fetch_all(&self.state.pool)
                    .await?;
            let derive: serde_json::Map<String, serde_json::Value> =
                rows.into_iter().map(|(s, n)| (s, n.into())).collect();
            Ok(serde_json::json!({ "files": files, "derive": derive }).to_string())
        })
    }

    /// Stop serving and close the database. Also done on drop.
    pub fn stop(mut self) {
        self.shut_down();
    }

    fn rt(&self) -> &tokio::runtime::Runtime {
        self.rt.as_ref().expect("runtime lives until shut_down")
    }

    fn shut_down(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(rt) = self.rt.take() {
            rt.block_on(self.state.pool.close());
            rt.shutdown_timeout(Duration::from_secs(2));
        }
    }
}

impl Drop for Embedded {
    fn drop(&mut self) {
        self.shut_down();
    }
}

fn forbidden() -> Response {
    (StatusCode::FORBIDDEN, "forbidden").into_response()
}

/// Constant-time equality for equally long secrets.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn cookie<'a>(headers: &'a axum::http::HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| kv.trim().split_once('=').filter(|(k, _)| *k == name))
        .map(|(_, v)| v)
}

/// Outermost layer: Host and secret checks, then the session is attached.
async fn guard_fn(State(g): State<Arc<Guard>>, mut req: Request, next: Next) -> Response {
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| g.hosts.iter().any(|x| x.eq_ignore_ascii_case(h)));
    let secret_ok = cookie(req.headers(), SECRET_COOKIE)
        .is_some_and(|v| same(v.as_bytes(), g.secret.as_bytes()));
    if !host_ok || !secret_ok {
        return forbidden();
    }
    // A read-only copy: edits made here would never reach the real server. Writes into this
    // database happen only through `Embedded::ingest_bytes`.
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "this is the phone's read-only copy; edit when the server is reachable",
        )
        .into_response();
    }
    // Replace the cookie header with only the session (the secret goes no further).
    let value = format!("{}={}", auth::cookie_name(true), g.session);
    req.headers_mut().remove(header::COOKIE);
    if let Ok(v) = HeaderValue::from_str(&value) {
        req.headers_mut().insert(header::COOKIE, v);
    }
    next.run(req).await
}
