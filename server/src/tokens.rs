//! Device tokens: 32 random bytes shown once; only the SHA-256 is stored.
//! Created from the CLI only; there is no bootstrap HTTP endpoint.

use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

pub const TOKEN_PREFIX: &str = "nrg_";

pub fn hash_token(token: &str) -> String {
    crate::hex(&Sha256::digest(token.as_bytes()))
}

/// Create a token and return `(id, token)`.
pub async fn create(pool: &SqlitePool, name: &str) -> Result<(i64, String)> {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
    let token = format!("{TOKEN_PREFIX}{}", crate::hex(&raw));
    let hash = hash_token(&token);
    let id = sqlx::query!("INSERT INTO tokens (name, hash) VALUES (?, ?)", name, hash)
        .execute(pool)
        .await?
        .last_insert_rowid();
    Ok((id, token))
}

pub async fn revoke(pool: &SqlitePool, id: i64) -> Result<bool> {
    let n = sqlx::query!(
        "UPDATE tokens SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND revoked_at IS NULL",
        id
    )
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n == 1)
}

pub struct TokenRow {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<TokenRow>> {
    Ok(sqlx::query_as!(
        TokenRow,
        r#"SELECT id as "id!: i64", name, created_at, last_used_at, revoked_at FROM tokens ORDER BY id"#
    )
    .fetch_all(pool)
    .await?)
}

/// Authenticate a presented token: returns `(id, name)` and stamps
/// `last_used_at`. Compares hashes against every active token without early exit.
pub async fn authenticate(pool: &SqlitePool, token: &str) -> Result<Option<(i64, String)>> {
    let presented = hash_token(token);
    let rows = sqlx::query!("SELECT id, name, hash FROM tokens WHERE revoked_at IS NULL")
        .fetch_all(pool)
        .await?;
    let mut found = None;
    for row in rows {
        if ct_eq(presented.as_bytes(), row.hash.as_bytes()) {
            found = Some((row.id, row.name));
        }
    }
    if let Some((id, _)) = &found {
        sqlx::query!(
            "UPDATE tokens SET last_used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
            id
        )
        .execute(pool)
        .await?;
    }
    Ok(found)
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
