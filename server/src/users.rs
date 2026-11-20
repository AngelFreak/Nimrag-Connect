//! User accounts: the `users` table, username rules, and the lookups the login flow needs.
//!
//! There are two roles. `admin` sets the server up and manages accounts and has no data of its
//! own; `user` has the data (device tokens, files, settings). Usernames are 3 to 32 characters
//! of `[a-z0-9._-]`, case-insensitive (stored lower case).
//!
//! Nothing here logs passwords, hashes or TOTP secrets.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
        }
    }
    pub fn parse(s: &str) -> Role {
        if s == "admin" {
            Role::Admin
        } else {
            Role::User
        }
    }
}

/// A full `users` row. `Debug` is hand-written so the hash and the secret never reach a log.
#[derive(Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub role: Role,
    pub totp_secret: Option<String>,
    pub totp_enabled: bool,
    pub totp_last_step: i64,
    pub must_change_password: bool,
    pub created_at: i64,
    pub disabled_at: Option<i64>,
}

impl std::fmt::Debug for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("username", &self.username)
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

pub const USERNAME_MIN: usize = 3;
pub const USERNAME_MAX: usize = 32;

/// Lower-case a username and check the rules; the error text is shown to the admin.
pub fn normalise_username(raw: &str) -> Result<String, String> {
    let name = raw.trim().to_ascii_lowercase();
    let len = name.chars().count();
    if !(USERNAME_MIN..=USERNAME_MAX).contains(&len) {
        return Err(format!(
            "username must have {USERNAME_MIN} to {USERNAME_MAX} characters"
        ));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err("username may only contain letters a-z, digits, '.', '_' and '-'".into());
    }
    Ok(name)
}

const COLUMNS: &str = "id, username, password_hash, role, totp_secret, totp_enabled,
    totp_last_step, must_change_password, created_at, disabled_at";

fn from_row(r: &sqlx::sqlite::SqliteRow) -> User {
    User {
        id: r.get(0),
        username: r.get(1),
        password_hash: r.get(2),
        role: Role::parse(&r.get::<String, _>(3)),
        totp_secret: r.get(4),
        totp_enabled: r.get::<i64, _>(5) != 0,
        totp_last_step: r.get(6),
        must_change_password: r.get::<i64, _>(7) != 0,
        created_at: r.get(8),
        disabled_at: r.get(9),
    }
}

/// By username (any case); input that cannot be a username finds nothing.
pub async fn by_username(pool: &SqlitePool, username: &str) -> Result<Option<User>> {
    let Ok(name) = normalise_username(username) else {
        return Ok(None);
    };
    let sql = format!("SELECT {COLUMNS} FROM users WHERE username = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(name)
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(from_row))
}

pub async fn by_id(pool: &SqlitePool, id: i64) -> Result<Option<User>> {
    let sql = format!("SELECT {COLUMNS} FROM users WHERE id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(from_row))
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<User>> {
    let sql = format!("SELECT {COLUMNS} FROM users ORDER BY username");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(from_row).collect())
}

/// How many users exist at all (setup is open while this is zero).
pub async fn count(pool: &SqlitePool) -> Result<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?)
}

/// Insert a user; `None` when the username is taken (the unique index decides, so two racing
/// creations cannot both win).
pub async fn create(
    pool: &SqlitePool,
    username: &str,
    password_hash: &str,
    role: Role,
    must_change_password: bool,
    now: i64,
) -> Result<Option<i64>> {
    let res = sqlx::query(
        "INSERT INTO users (username, password_hash, role, must_change_password, created_at)
         VALUES (?, ?, ?, ?, ?) ON CONFLICT (username) DO NOTHING",
    )
    .bind(username)
    .bind(password_hash)
    .bind(role.as_str())
    .bind(must_change_password)
    .bind(now)
    .execute(pool)
    .await?;
    Ok((res.rows_affected() == 1).then(|| res.last_insert_rowid()))
}

/// First-run setup: insert the first admin, enrolled, only if no user exists yet. `true` when
/// this call created it, so of two racing setups exactly one wins.
pub async fn claim_first_admin(
    pool: &SqlitePool,
    username: &str,
    password_hash: &str,
    totp_secret: &str,
    totp_step: i64,
    now: i64,
) -> Result<bool> {
    let res = sqlx::query(
        "INSERT INTO users (username, password_hash, role, totp_secret, totp_enabled,
                            totp_last_step, must_change_password, created_at)
         SELECT ?, ?, 'admin', ?, 1, ?, 0, ? WHERE NOT EXISTS (SELECT 1 FROM users)",
    )
    .bind(username)
    .bind(password_hash)
    .bind(totp_secret)
    .bind(totp_step)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() == 1)
}

/// Store a new password hash, optionally forcing a change at the next login. Signs the user
/// out everywhere and ends their trusted devices' ability to unlock.
pub async fn set_password(
    pool: &SqlitePool,
    id: i64,
    password_hash: &str,
    must_change: bool,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE users SET password_hash = ?, must_change_password = ? WHERE id = ?")
        .bind(password_hash)
        .bind(must_change)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE tokens SET device_secret_hash = NULL WHERE user_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Enrol (or re-enrol) 2FA: store the secret, switch it on and remember the step that proved it.
pub async fn set_totp(pool: &SqlitePool, id: i64, secret: &str, step: i64) -> Result<()> {
    sqlx::query(
        "UPDATE users SET totp_secret = ?, totp_enabled = 1, totp_last_step = ? WHERE id = ?",
    )
    .bind(secret)
    .bind(step)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Forget the 2FA secret: the next login enrols again. Signs the user out and ends their
/// trusted devices' unlock credentials.
pub async fn reset_totp(pool: &SqlitePool, id: i64) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE users SET totp_secret = NULL, totp_enabled = 0, totp_last_step = 0 WHERE id = ?",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE tokens SET device_secret_hash = NULL WHERE user_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Record that `step` was used. `false` when it is not newer than the stored step (a replay,
/// or a racing request that used the same code first): the update is a compare-and-set.
pub async fn accept_totp_step(pool: &SqlitePool, id: i64, step: i64) -> Result<bool> {
    let res =
        sqlx::query("UPDATE users SET totp_last_step = ? WHERE id = ? AND totp_last_step < ?")
            .bind(step)
            .bind(id)
            .bind(step)
            .execute(pool)
            .await?;
    Ok(res.rows_affected() == 1)
}

/// Active (not disabled) admins other than `except`.
pub async fn other_active_admins(pool: &SqlitePool, except: i64) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE role = 'admin' AND disabled_at IS NULL AND id <> ?",
    )
    .bind(except)
    .fetch_one(pool)
    .await?)
}

/// Switch an account off (sessions and device secrets go with it) or on again.
pub async fn set_disabled(pool: &SqlitePool, id: i64, disabled: bool, now: i64) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE users SET disabled_at = ? WHERE id = ?")
        .bind(disabled.then_some(now))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if disabled {
        sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// A temporary password to hand to a new user (or after a reset): 16 characters from an
/// alphabet without look-alikes, in groups of four. About 80 bits.
pub fn temporary_password() -> Result<String> {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789"; // 31 symbols
    let mut raw = [0u8; 16];
    let mut out = String::with_capacity(19);
    let mut n = 0;
    while n < 16 {
        getrandom::fill(&mut raw).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
        for b in raw {
            // Rejection sampling: 248 = 31 * 8, so every symbol is equally likely.
            if b < 248 && n < 16 {
                if n > 0 && n % 4 == 0 {
                    out.push('-');
                }
                out.push(ALPHABET[usize::from(b) % ALPHABET.len()] as char);
                n += 1;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames_are_lowercased_and_checked() {
        assert_eq!(normalise_username("  Anna.K-2_x ").unwrap(), "anna.k-2_x");
        assert!(normalise_username("ab").is_err());
        assert!(normalise_username(&"a".repeat(33)).is_err());
        assert!(normalise_username(&"a".repeat(32)).is_ok());
        for bad in ["a b c", "anna@home", "ånna", "anna/k", "anna:k", "añna"] {
            assert!(normalise_username(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn temporary_passwords_are_long_enough_and_different() {
        let a = temporary_password().unwrap();
        let b = temporary_password().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.len(), 19);
        assert!(a.chars().count() >= crate::auth::MIN_PASSWORD_LEN);
        assert_eq!(a.matches('-').count(), 3);
    }
}
