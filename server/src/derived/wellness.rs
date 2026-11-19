//! Wellness persistence (scaffold stub; filled by the wellness builder).
use anyhow::Result;
use nimrag_core::derive::Derived;
use sqlx::{Sqlite, Transaction};

pub const SAMPLE_TABLES: &[&str] = &[];
pub const ENTITY_TABLES: &[&str] = &[];

pub async fn write(
    _tx: &mut Transaction<'_, Sqlite>,
    _sha256: &str,
    _src: i64,
    _d: &Derived,
) -> Result<()> {
    Ok(())
}
