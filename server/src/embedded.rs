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
//! * Edits over HTTP: GET and HEAD are served; of the other methods only the changes of
//!   `edits::Edit` (settings, body entries, strength sets, workouts) are run, and each one that
//!   succeeds is written to the outbox (`outbox.rs`) for [`Embedded::next_edit`] to hand to the
//!   app, which sends it to the real server when it can. Every other change gets 403.
//! * History: [`Embedded::ingest_history`] puts in files the real server has and the phone
//!   lacks (`docs/design/history-on-phone.md`), marked in `history_copies`;
//!   [`Embedded::prune_history`] removes those again, never a file the phone's own watch gave.
//! * Files are put in with [`Embedded::ingest_bytes`] (in process, same path as the HTTP
//!   upload: store, sidecar, ledger, derive).
//!
//! The handle owns its own small tokio runtime, so callers need no async.

use crate::api::{self, ApiError, AppState};
use crate::edits::{self, DeviceState};
use crate::ledger::Meta;
use crate::users::{self, Role};
use crate::{auth, db, derive, derived, ingest, outbox, store};
use anyhow::{bail, Context, Result};
use axum::{
    body::Body,
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
pub use crate::outbox::{Dropped as DroppedEdit, Pending as PendingEdit, Verdict as EditVerdict};
/// The server side session is renewed after this many seconds (sessions last 30 days).
const SESSION_RENEW_SECS: i64 = 7 * 24 * 3600;

/// A running in-process server. Dropping it stops the server.
pub struct Embedded {
    port: u16,
    rt: Option<tokio::runtime::Runtime>,
    state: AppState,
    user_id: i64,
    /// Held while a change is made and written to the outbox, and while the state is replaced.
    edit_lock: Arc<tokio::sync::Mutex<()>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    _lock: std::fs::File,
}

struct Guard {
    secret: String,
    /// The local user's session (cookie value) and when it was made.
    session: tokio::sync::Mutex<(String, i64)>,
    user_id: i64,
    state: AppState,
    edit_lock: Arc<tokio::sync::Mutex<()>>,
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
    let edit_lock = Arc::new(tokio::sync::Mutex::new(()));
    let (state, user_id, port) = rt.block_on(async {
        let pool = db::open(&data_dir).await?;
        store::sweep_incoming(&data_dir)?;
        let state = AppState::new(pool, data_dir, api::MAX_BODY_BYTES)
            .with_insecure_cookies(true)
            .with_deferred_watch_send(true);
        outbox::recover(&state.pool).await?;
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
            edit_lock: edit_lock.clone(),
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
        edit_lock,
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
        let sha = received.sha256.clone();
        let created = self.rt().block_on(ingest::ingest(
            &self.state,
            &received,
            None,
            Some(self.user_id),
            meta,
            None,
        ))?;
        // The watch delivering a file the server copy brought earlier makes it the phone's own:
        // pruning the history must not remove it.
        self.rt().block_on(async {
            sqlx::query("DELETE FROM history_copies WHERE sha256 = ?")
                .bind(&sha)
                .execute(&self.state.pool)
                .await
        })?;
        Ok(created)
    }

    /// Of `sha256s`, those this copy does not have (order kept).
    /// Blocking; never call it from inside an async context.
    pub fn missing_files(&self, sha256s: &[String]) -> Result<Vec<String>> {
        self.rt().block_on(async {
            let mut have = std::collections::HashSet::new();
            // SQLite allows 32766 variables; stay far below.
            for chunk in sha256s.chunks(500) {
                let marks = vec!["?"; chunk.len()].join(", ");
                let sql = format!("SELECT sha256 FROM files WHERE sha256 IN ({marks})");
                let mut q = sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql));
                for sha in chunk {
                    q = q.bind(sha);
                }
                have.extend(q.fetch_all(&self.state.pool).await?);
            }
            Ok(sha256s
                .iter()
                .filter(|s| !have.contains(*s))
                .cloned()
                .collect())
        })
    }

    /// Store a file the real server has (`meta` as the server lists it) as a copy: owned by the
    /// local user, derived like any other, and marked in `history_copies` with `last_ts` (the
    /// server's last data timestamp) unless this copy already had the file. Fails, storing
    /// nothing, when the bytes do not hash to `sha256`. Returns true if the file is new here.
    /// Blocking; never call it from inside an async context.
    pub fn ingest_history(
        &self,
        bytes: &[u8],
        sha256: &str,
        meta: Meta,
        last_ts: Option<i64>,
    ) -> Result<bool> {
        let actual = crate::sha256_hex(bytes);
        if actual != sha256 {
            bail!("the bytes hash to {actual}, not {sha256}");
        }
        let had: bool = self.rt().block_on(async {
            sqlx::query_scalar::<_, i64>("SELECT 1 FROM files WHERE sha256 = ?")
                .bind(sha256)
                .fetch_optional(&self.state.pool)
                .await
                .map(|r| r.is_some())
        })?;
        if !had {
            // Marked before it is stored, so a crash in between leaves a mark with no file
            // (harmless), never a copy that looks like the phone's own.
            let now = self.state.now();
            self.rt().block_on(async {
                sqlx::query(
                    "INSERT OR REPLACE INTO history_copies (sha256, last_ts, copied_at)
                     VALUES (?, ?, ?)",
                )
                .bind(sha256)
                .bind(last_ts)
                .bind(now)
                .execute(&self.state.pool)
                .await
            })?;
        }
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

    /// Remove the copied server files whose last data timestamp is before `keep_since` (files
    /// with no known one stay), or every copy when `keep_since` is `None`; then rebuild the
    /// derived data of what is left. The phone's own files are never touched. Returns how many
    /// files were removed.
    /// Blocking; never call it from inside an async context.
    pub fn prune_history(&self, keep_since: Option<i64>) -> Result<u32> {
        self.rt().block_on(async {
            let state = &self.state;
            // Not while a file is being ingested: the sidecar and the rows must stay in step.
            let _ledger = state.ledger_lock.lock().await;
            let victims: Vec<String> = sqlx::query_scalar(
                "SELECT sha256 FROM history_copies
                 WHERE ?1 IS NULL OR (last_ts IS NOT NULL AND last_ts < ?1)",
            )
            .bind(keep_since)
            .fetch_all(&state.pool)
            .await?;
            if victims.is_empty() {
                return Ok(0);
            }
            // Read, then write: take the write lock at once, or a running write fails us with
            // "database is locked".
            let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
            sqlx::query("PRAGMA defer_foreign_keys = ON")
                .execute(&mut *tx)
                .await?;
            // Every table that points at a file by its hash, found rather than listed, so a
            // table added later is not forgotten here.
            let tables: Vec<(String, String)> = sqlx::query_as(
                "SELECT DISTINCT m.name, f.\"from\" FROM sqlite_master m, pragma_foreign_key_list(m.name) f
                 WHERE m.type = 'table' AND f.\"table\" IN ('files', 'fit_files')
                   AND f.\"to\" = 'sha256' AND m.name <> 'files'",
            )
            .fetch_all(&mut *tx)
            .await?;
            let mut removed = 0u32;
            let mut gone = Vec::new();
            for sha in &victims {
                let had: Option<i64> = sqlx::query_scalar("SELECT 1 FROM files WHERE sha256 = ?")
                    .bind(sha)
                    .fetch_optional(&mut *tx)
                    .await?;
                if had.is_some() {
                    derived::clear_file(&mut tx, sha).await?;
                    for (table, col) in &tables {
                        sqlx::query(sqlx::AssertSqlSafe(format!(
                            "DELETE FROM \"{table}\" WHERE \"{col}\" = ?"
                        )))
                        .bind(sha)
                        .execute(&mut *tx)
                        .await?;
                    }
                    sqlx::query("DELETE FROM files WHERE sha256 = ?")
                        .bind(sha)
                        .execute(&mut *tx)
                        .await?;
                    removed += 1;
                    gone.push(sha.clone());
                }
                sqlx::query("DELETE FROM history_copies WHERE sha256 = ?")
                    .bind(sha)
                    .execute(&mut *tx)
                    .await?;
            }
            if removed > 0 {
                // Rows that lost a rank contest to a removed file come back.
                derive::reparse_partitions(&mut tx, &state.data_dir, &[self.user_id]).await?;
            }
            tx.commit().await?;
            // The bytes and sidecars go after the commit: a failure here leaves stray files,
            // never rows that point at nothing.
            for sha in &gone {
                for path in [
                    store::path_for(&state.data_dir, sha),
                    store::sidecar_path_for(&state.data_dir, sha),
                ] {
                    if let Err(e) = std::fs::remove_file(&path) {
                        tracing::warn!(path = %path.display(), error = %e, "history: removing a pruned file failed");
                    }
                }
            }
            Ok(removed)
        })
    }

    /// The oldest change made on this copy that the server has not got yet, with its path in
    /// the server's terms (see `docs/design/offline-edits.md`). `None` when nothing waits.
    /// Blocking; never call it from inside an async context.
    pub fn next_edit(&self) -> Result<Option<PendingEdit>> {
        self.rt()
            .block_on(outbox::next(&self.state.pool, self.state.now()))
    }

    /// Report the server's answer to `op_id` (`status` 0: no answer). See [`outbox::done`].
    /// Blocking; never call it from inside an async context.
    pub fn edit_done(&self, op_id: &str, status: u16, response_body: &str) -> Result<EditVerdict> {
        self.rt().block_on(outbox::done(
            &self.state.pool,
            self.state.now(),
            op_id,
            status,
            response_body,
        ))
    }

    /// How many changes wait to be sent.
    /// Blocking; never call it from inside an async context.
    pub fn pending_edits(&self) -> Result<u32> {
        self.rt().block_on(outbox::count(&self.state.pool))
    }

    /// The changes the server could not apply, once.
    /// Blocking; never call it from inside an async context.
    pub fn take_dropped_edits(&self) -> Result<Vec<DroppedEdit>> {
        self.rt().block_on(outbox::take_dropped(&self.state.pool))
    }

    /// Replace the local user's settings, workouts, body entries and strength edits with
    /// `GET /api/device/state`'s answer (`json`). Returns false, changing nothing, while changes
    /// wait in the outbox.
    /// Blocking; never call it from inside an async context.
    pub fn apply_state(&self, json: &str) -> Result<bool> {
        let state: DeviceState = serde_json::from_str(json).context("reading the state")?;
        self.rt().block_on(async {
            let _no_edit_meanwhile = self.edit_lock.lock().await;
            outbox::apply_state(&self.state.pool, self.user_id, &state).await
        })
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
    // Only the changes of `edits::Edit` are taken, and each is written to the outbox so it
    // reaches the real server; anything else would never get there. Other writes into this
    // database happen only through `Embedded::ingest_bytes`.
    let reads = matches!(*req.method(), Method::GET | Method::HEAD);
    let creates = if reads {
        None
    } else {
        match edits::Edit::parse(req.method(), req.uri().path()) {
            Some(edit) => Some(edit.creates()),
            None => return edits::needs_server(),
        }
    };
    // Replace the cookie header with only the session (the secret goes no further).
    let value = format!("{}={}", auth::cookie_name(true), session_value(&g).await);
    req.headers_mut().remove(header::COOKIE);
    if let Ok(v) = HeaderValue::from_str(&value) {
        req.headers_mut().insert(header::COOKIE, v);
    }
    match creates {
        None => next.run(req).await,
        Some(creates) => make_edit(g, req, next, creates).await,
    }
}

/// Run a change and write it to the outbox. In a task of its own: a client that hangs up
/// must not cut the handler off between its change and the outbox row.
async fn make_edit(
    g: Arc<Guard>,
    req: Request,
    next: Next,
    creates: Option<&'static str>,
) -> Response {
    match tokio::spawn(async move { run_edit(&g, req, next, creates).await }).await {
        Ok(resp) => resp,
        Err(_) => {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

/// The outbox row is written first and confirmed once the handler succeeded (removed if it did
/// not), because the handlers write straight to the pool, so one transaction cannot span both.
/// That order keeps "the handler succeeded" implying "the row exists" through a crash.
async fn run_edit(g: &Guard, req: Request, next: Next, creates: Option<&'static str>) -> Response {
    // One at a time: the outbox order is the order the changes were made.
    let _one_at_a_time = g.edit_lock.lock().await;
    let (parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, api::MAX_JSON_BYTES).await else {
        return ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response();
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return ApiError::bad_request("the body must be UTF-8 JSON").into_response();
    };
    let pool = &g.state.pool;
    let recorded = async {
        let mut raw = [0u8; 16];
        getrandom::fill(&mut raw).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
        outbox::record(
            pool,
            &crate::hex(&raw),
            parts.method.as_str(),
            parts.uri.path(),
            text,
            g.state.now(),
            creates,
        )
        .await
    }
    .await;
    let seq = match recorded {
        Ok(seq) => seq,
        Err(e) => return ApiError::from(e).into_response(),
    };

    let resp = next
        .run(Request::from_parts(parts, Body::from(bytes.clone())))
        .await;
    let (rparts, rbody) = resp.into_parts();
    let answer = axum::body::to_bytes(rbody, usize::MAX).await;
    let settled = if rparts.status.is_success() {
        // A create's answer names the new row.
        let local_id = creates
            .and(answer.as_ref().ok())
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
            .and_then(|v| v.get("id").and_then(|i| i.as_i64()));
        outbox::confirm(pool, seq, local_id).await
    } else {
        outbox::discard(pool, seq).await
    };
    if let Err(e) = settled {
        return ApiError::from(e).into_response();
    }
    match answer {
        Ok(b) => Response::from_parts(rparts, Body::from(b)),
        Err(_) => {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}
