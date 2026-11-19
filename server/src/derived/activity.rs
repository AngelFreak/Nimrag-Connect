//! Activity persistence (scaffold stub; filled by the activity builder).
use anyhow::Result;
use nimrag_core::derive::Derived;
use sqlx::{Sqlite, Transaction};

pub const ENTITY_TABLES: &[&str] = &[];

pub async fn write(_tx: &mut Transaction<'_, Sqlite>, _sha256: &str, _d: &Derived) -> Result<()> {
    Ok(())
}
