//! Device tokens: 32 random bytes shown once; only the SHA-256 is stored.
//!
//! A token belongs to a user (`tokens.user_id`) or, for the ones that existed before accounts or
//! were made by the CLI without `--user`, to nobody until the admin assigns it. Files a token
//! uploads belong to its user. A user never creates or sees a token: the app's login
//! (`POST /api/app/login`) issues one in the background, together with a trusted-device secret
//! (`device_secret_hash`, only its SHA-256 is stored), at most [`MAX_ACTIVE`] active per user.
//! The web Profile lists the user's phones and revokes them (`GET/DELETE /api/tokens`). The
//! admin's CLI (`token create`) makes tokens for tools, which stay unassigned or belong to the
//! named user. Revoking a token revokes its device secret and ends its app sessions.

use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};

pub const TOKEN_PREFIX: &str = "nrg_";
pub const DEVICE_SECRET_PREFIX: &str = "nrd_";
/// Active tokens one user may have through the app (the CLI is not capped).
pub const MAX_ACTIVE: i64 = 20;

pub fn hash_token(token: &str) -> String {
    crate::hex(&Sha256::digest(token.as_bytes()))
}

fn random_secret(prefix: &str) -> Result<String> {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
    Ok(format!("{prefix}{}", crate::hex(&raw)))
}

/// Create a token for `user_id` (None: unassigned) and return `(id, token)`.
pub async fn create(pool: &SqlitePool, name: &str, user_id: Option<i64>) -> Result<(i64, String)> {
    let token = random_secret(TOKEN_PREFIX)?;
    let id = sqlx::query("INSERT INTO tokens (name, hash, user_id) VALUES (?, ?, ?)")
        .bind(name)
        .bind(hash_token(&token))
        .bind(user_id)
        .execute(pool)
        .await?
        .last_insert_rowid();
    Ok((id, token))
}

/// A token created by [`create_capped`].
pub struct Created {
    pub id: i64,
    pub token: String,
    /// Only for app logins.
    pub device_secret: Option<String>,
}

/// Like [`create`] for a user, but `None` (nothing inserted) when `max_active` tokens of that
/// user are already active. The check and the insert are one statement, so parallel requests
/// cannot overshoot. With `with_device_secret` the token also gets a trusted-device secret.
pub async fn create_capped(
    pool: &SqlitePool,
    name: &str,
    user_id: i64,
    max_active: i64,
    with_device_secret: bool,
) -> Result<Option<Created>> {
    let token = random_secret(TOKEN_PREFIX)?;
    let secret = with_device_secret
        .then(|| random_secret(DEVICE_SECRET_PREFIX))
        .transpose()?;
    let res = sqlx::query(
        "INSERT INTO tokens (name, hash, user_id, device_secret_hash)
         SELECT ?, ?, ?, ? WHERE (SELECT COUNT(*) FROM tokens WHERE revoked_at IS NULL AND user_id = ?) < ?",
    )
    .bind(name)
    .bind(hash_token(&token))
    .bind(user_id)
    .bind(secret.as_deref().map(hash_token))
    .bind(user_id)
    .bind(max_active)
    .execute(pool)
    .await?;
    Ok((res.rows_affected() == 1).then(|| Created {
        id: res.last_insert_rowid(),
        token,
        device_secret: secret,
    }))
}

/// Revoke an active token (and with it its device secret and app sessions). With `owner`, only
/// when the token belongs to that user.
pub async fn revoke(pool: &SqlitePool, id: i64, owner: Option<i64>) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let n = sqlx::query(
        "UPDATE tokens SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), device_secret_hash = NULL
         WHERE id = ? AND revoked_at IS NULL AND (? IS NULL OR user_id = ?)",
    )
    .bind(id)
    .bind(owner)
    .bind(owner)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n == 1 {
        sqlx::query("DELETE FROM sessions WHERE token_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(n == 1)
}

pub struct TokenRow {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
    pub user_id: Option<i64>,
    pub username: Option<String>,
}

/// Which tokens [`list`] returns.
pub enum Whose {
    /// Every token, with its user's name (CLI).
    All,
    /// One user's tokens.
    User(i64),
    /// Tokens nobody owns yet.
    Unassigned,
}

pub async fn list(pool: &SqlitePool, whose: Whose) -> Result<Vec<TokenRow>> {
    let (cond, bind) = match whose {
        Whose::All => ("1 = 1", None),
        Whose::User(id) => ("t.user_id = ?", Some(id)),
        Whose::Unassigned => ("t.user_id IS NULL", None),
    };
    let sql = format!(
        "SELECT t.id, t.name, t.created_at, t.last_used_at, t.revoked_at, t.user_id, u.username
         FROM tokens t LEFT JOIN users u ON u.id = t.user_id WHERE {cond} ORDER BY t.id"
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
    if let Some(b) = bind {
        q = q.bind(b);
    }
    Ok(q.fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| TokenRow {
            id: r.get(0),
            name: r.get(1),
            created_at: r.get(2),
            last_used_at: r.get(3),
            revoked_at: r.get(4),
            user_id: r.get(5),
            username: r.get(6),
        })
        .collect())
}

/// A presented device token that checked out.
pub struct Authenticated {
    pub id: i64,
    pub name: String,
    pub user_id: Option<i64>,
}

/// Authenticate a presented token and stamp `last_used_at`. Compares hashes against every
/// active token without early exit. A token whose user is disabled does not authenticate.
pub async fn authenticate(pool: &SqlitePool, token: &str) -> Result<Option<Authenticated>> {
    let presented = hash_token(token);
    let rows = sqlx::query(
        "SELECT t.id, t.name, t.hash, t.user_id FROM tokens t LEFT JOIN users u ON u.id = t.user_id
         WHERE t.revoked_at IS NULL AND (t.user_id IS NULL OR u.disabled_at IS NULL)",
    )
    .fetch_all(pool)
    .await?;
    let mut found = None;
    for row in rows {
        let hash: String = row.get(2);
        if ct_eq(presented.as_bytes(), hash.as_bytes()) {
            found = Some(Authenticated {
                id: row.get(0),
                name: row.get(1),
                user_id: row.get(3),
            });
        }
    }
    if let Some(a) = &found {
        sqlx::query(
            "UPDATE tokens SET last_used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
        )
        .bind(a.id)
        .execute(pool)
        .await?;
    }
    Ok(found)
}

/// The active token a trusted-device secret belongs to, with its (enabled) user's name.
pub struct TrustedDevice {
    pub token_id: i64,
    pub user_id: i64,
    pub username: String,
}

pub async fn authenticate_device_secret(
    pool: &SqlitePool,
    secret: &str,
) -> Result<Option<TrustedDevice>> {
    let presented = hash_token(secret);
    let rows = sqlx::query(
        "SELECT t.id, t.device_secret_hash, u.id, u.username
         FROM tokens t JOIN users u ON u.id = t.user_id
         WHERE t.revoked_at IS NULL AND t.device_secret_hash IS NOT NULL AND u.disabled_at IS NULL",
    )
    .fetch_all(pool)
    .await?;
    let mut found = None;
    for row in rows {
        let hash: String = row.get(1);
        if ct_eq(presented.as_bytes(), hash.as_bytes()) {
            found = Some(TrustedDevice {
                token_id: row.get(0),
                user_id: row.get(2),
                username: row.get(3),
            });
        }
    }
    if let Some(d) = &found {
        sqlx::query(
            "UPDATE tokens SET last_used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
        )
        .bind(d.token_id)
        .execute(pool)
        .await?;
    }
    Ok(found)
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
