//! The server as a library inside another process (the Android app).
//!
//! [`start`] opens the data dir, serves the same router as `nimrag-server serve` on
//! `127.0.0.1` and returns a handle. Differences from `serve`:
//!
//! * No login. Every request must carry the launch secret in the `nimrag_local` cookie
//!   (other apps on the phone can reach 127.0.0.1, the secret keeps them out: they get 403).
//!   A request that passes also gets a server-side session injected, so the owner API
//!   works unchanged. The Host header must be the loopback address (DNS rebinding).
//! * One local user. On start the data dir gets exactly one account, `local` (role `user`,
//!   a random password nobody knows, no 2FA: nobody signs in over the network), and its session
//!   is created server side. Because a user exists, the web app never offers first-run setup,
//!   and an admin cannot exist here: [`start`] refuses a data dir that holds one (a data dir
//!   from the single-owner spike, whose only account is the migrated `admin`, is converted).
//!   Every file put in with [`Embedded::ingest_bytes`] is owned by that user, and files a
//!   spike-era data dir holds without an owner are claimed by it at start.
//! * No Forward worker: nothing leaves the phone from here.
//! * Read-only over HTTP: every method but GET and HEAD gets 403 (edits would never reach the
//!   real server).
//! * Files are put in with [`Embedded::ingest_bytes`] (in process, same path as the HTTP
//!   upload: store, sidecar, ledger, derive).
//!
//! The handle owns its own small tokio runtime, so callers need no async.

use crate::api::{self, ApiError, AppState};
use crate::ledger::Meta;
use crate::users::{self, Role};
use crate::{auth, db, derive, ingest, store};
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
/// Username of the one local account.
pub const LOCAL_USERNAME: &str = "local";
/// The server side session is renewed after this many seconds (sessions last 30 days).
const SESSION_RENEW_SECS: i64 = 7 * 24 * 3600;

/// A running in-process server. Dropping it stops the server.
pub struct Embedded {
    port: u16,
    rt: Option<tokio::runtime::Runtime>,
    state: AppState,
    user_id: i64,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    _lock: std::fs::File,
}

struct Guard {
    secret: String,
    /// The local user's session (cookie value) and when it was made.
    session: tokio::sync::Mutex<(String, i64)>,
    user_id: i64,
    state: AppState,
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
    let (state, user_id, port) = rt.block_on(async {
        let pool = db::open(&data_dir).await?;
        store::sweep_incoming(&data_dir)?;
        let state = AppState::new(pool, data_dir, api::MAX_BODY_BYTES).with_insecure_cookies(true);
        let user_id = ensure_local_user(&state).await?;
        // Sessions of an earlier launch are of no use to anybody: the cookie never leaves here.
        sqlx::query("DELETE FROM sessions")
            .execute(&state.pool)
            .await?;
        let now = state.now();
        let session = auth::create_session(&state.pool, user_id, None, now).await?;

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
            session: tokio::sync::Mutex::new((session, now)),
            user_id,
            state: state.clone(),
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
        anyhow::Ok((state, user_id, port))
    })?;
    Ok(Embedded {
        port,
        rt: Some(rt),
        state,
        user_id,
        shutdown: Some(shutdown),
        _lock: lock,
    })
}

impl Embedded {
    /// The bound port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The id of the one local user (owner of every file here).
    pub fn user_id(&self) -> i64 {
        self.user_id
    }

    /// Store `bytes` as a watch file owned by the local user and derive from it. Returns true
    /// if the file is new.
    /// Blocking; never call it from inside an async context.
    pub fn ingest_bytes(&self, bytes: &[u8], meta: Meta) -> Result<bool> {
        let received = store::receive_bytes(&self.state.data_dir, bytes)?;
        self.rt().block_on(ingest::ingest(
            &self.state,
            &received,
            None,
            Some(self.user_id),
            meta,
            None,
        ))
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

/// Make sure the data dir has exactly one account, a normal user, and that it owns every file.
/// Refuses a data dir with an admin or with several users: the phone's copy has one user and
/// nobody who could manage others.
async fn ensure_local_user(state: &AppState) -> Result<i64> {
    let all = users::list(&state.pool).await?;
    let id = match all.as_slice() {
        [] => {
            // Nobody knows this password; nobody logs in over the network.
            let mut raw = [0u8; 24];
            getrandom::fill(&mut raw).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
            let hash = auth::hash_password(&crate::hex(&raw))?;
            users::create(
                &state.pool,
                LOCAL_USERNAME,
                &hash,
                Role::User,
                false,
                state.now(),
            )
            .await?
            .context("creating the local user")?
        }
        [u] if u.role == Role::User => u.id,
        // A data dir from the single-owner spike: its only account is the migrated owner
        // (`admin`, 2FA never enrolled). It becomes the local user; nothing else changes.
        [u] if u.role == Role::Admin && !u.totp_enabled && u.username == "admin" => {
            sqlx::query(
                "UPDATE users SET role = 'user', username = ?, disabled_at = NULL WHERE id = ?",
            )
            .bind(LOCAL_USERNAME)
            .bind(u.id)
            .execute(&state.pool)
            .await?;
            u.id
        }
        _ => bail!("the phone's data dir must hold exactly one normal user and no admin"),
    };
    claim_unowned_files(state, id).await?;
    Ok(id)
}

/// Files without an owner (stored before accounts existed) belong to the local user; their
/// derived rows move out of the unassigned partition.
async fn claim_unowned_files(state: &AppState, uid: i64) -> Result<()> {
    let mut tx = state.pool.begin().await?;
    let claimed = sqlx::query(
        "INSERT INTO file_owners (sha256, user_id)
         SELECT f.sha256, ? FROM files f WHERE NOT EXISTS
            (SELECT 1 FROM file_owners o WHERE o.sha256 = f.sha256)",
    )
    .bind(uid)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if claimed > 0 {
        derive::reparse_partitions(&mut tx, &state.data_dir, &[0, uid]).await?;
    }
    tx.commit().await?;
    Ok(())
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

/// The local user's session cookie value, renewed when it gets old.
async fn session_value(g: &Guard) -> String {
    let mut cur = g.session.lock().await;
    let now = g.state.now();
    if now - cur.1 > SESSION_RENEW_SECS {
        if let Ok(fresh) = auth::create_session(&g.state.pool, g.user_id, None, now).await {
            *cur = (fresh, now);
        }
    }
    cur.0.clone()
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
    let value = format!("{}={}", auth::cookie_name(true), session_value(&g).await);
    req.headers_mut().remove(header::COOKIE);
    if let Ok(v) = HeaderValue::from_str(&value) {
        req.headers_mut().insert(header::COOKIE, v);
    }
    next.run(req).await
}
