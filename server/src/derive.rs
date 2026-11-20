//! Derived tables (`file_parse`, `fit_files`, `fit_messages`): decode a stored
//! file and write the result. Used by both ingest and `reparse`.
//!
//! Determinism: rows contain only values computed from the file bytes and the
//! parser, keyed by (sha256, chain_idx, msg_idx); JSON objects keep insertion
//! (definition) order.

use crate::derived::{derive_all, DeriveFn, DeriveOutcome};
use anyhow::Result;
use nimrag_core::fit::{self, BaseType, Crc, Decoded, Value};
use serde_json::{json, Map, Value as Json};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

/// Bytes needed to recognise a FIT header.
pub const SNIFF_LEN: usize = 12;

/// FIT decoding is decided from the bytes alone (never from upload metadata):
/// header size >= 12 and ".FIT" at bytes 8..12. Shared by ingest and reparse.
pub fn looks_like_fit(head: &[u8]) -> bool {
    head.len() >= SNIFF_LEN && head[0] >= 12 && &head[8..12] == b".FIT"
}

/// Bump when the stored representation or decoder behaviour changes.
pub const PARSER_VERSION: i64 = 1;

pub struct Parsed {
    pub decoded: Decoded,
    /// Set when the decoder returned an error or panicked.
    pub error: Option<String>,
    /// Core derivation over `decoded`, computed in the same blocking task.
    pub derived: DeriveOutcome,
}

/// Decode and derive without ever panicking.
pub fn parse(bytes: &[u8]) -> Parsed {
    let f = DERIVE_OVERRIDE
        .lock()
        .ok()
        .and_then(|g| *g)
        .unwrap_or(nimrag_core::derive::derive);
    parse_with(bytes, f)
}

/// Test seam: when set, `parse` (hence ingest and reparse) uses this derivation.
pub static DERIVE_OVERRIDE: std::sync::Mutex<Option<DeriveFn>> = std::sync::Mutex::new(None);

/// [`parse`] with an injectable derivation (tests).
pub fn parse_with(bytes: &[u8], derive_fn: DeriveFn) -> Parsed {
    match catch_unwind(AssertUnwindSafe(|| fit::decode(bytes))) {
        Ok(decoded) => {
            let error = decoded.error.as_ref().map(|e| format!("{e:?}"));
            let derived = derive_all(&decoded, derive_fn);
            Parsed {
                decoded,
                error,
                derived,
            }
        }
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            Parsed {
                decoded: Decoded {
                    files: vec![],
                    error: None,
                },
                error: Some(format!("decoder panicked: {msg}")),
                derived: DeriveOutcome::Derived(vec![]),
            }
        }
    }
}

async fn clear(tx: &mut Transaction<'_, Sqlite>, sha256: &str) -> Result<()> {
    crate::derived::clear_file(tx, sha256).await?;
    sqlx::query!("DELETE FROM fit_messages WHERE sha256 = ?", sha256)
        .execute(&mut **tx)
        .await?;
    sqlx::query!("DELETE FROM fit_files WHERE sha256 = ?", sha256)
        .execute(&mut **tx)
        .await?;
    sqlx::query!("DELETE FROM file_parse WHERE sha256 = ?", sha256)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

fn crc_parts(crc: Crc) -> (&'static str, Option<i64>, Option<i64>) {
    match crc {
        Crc::Ok => ("ok", None, None),
        Crc::Mismatch { stored, computed } => (
            "mismatch",
            Some(i64::from(stored)),
            Some(i64::from(computed)),
        ),
        Crc::Missing => ("missing", None, None),
    }
}

/// Replace the derived rows of one FIT-typed file.
pub async fn write(tx: &mut Transaction<'_, Sqlite>, sha256: &str, parsed: &Parsed) -> Result<()> {
    clear(tx, sha256).await?;

    for (chain_idx, file) in parsed.decoded.files.iter().enumerate() {
        let chain_idx = chain_idx as i64;
        let h = &file.header;
        let (header_size, protocol_version, profile_version, data_size) = (
            i64::from(h.header_size),
            i64::from(h.protocol_version),
            i64::from(h.profile_version),
            i64::from(h.data_size),
        );
        let header_crc = h.header_crc.map(i64::from);
        let (crc_status, crc_stored, crc_computed) = crc_parts(file.crc);
        let crc_ok = file.crc == Crc::Ok;
        let (byte_offset, byte_len, complete) =
            (file.offset as i64, file.len as i64, file.complete);
        let message_count = file.messages.len() as i64;
        let developer_fields = serde_json::to_string(
            &file
                .developer_fields
                .iter()
                .map(dev_description_json)
                .collect::<Vec<_>>(),
        )?;
        sqlx::query!(
            "INSERT INTO fit_files (sha256, chain_idx, header_size, protocol_version,
                profile_version, data_size, header_crc, header_crc_ok, byte_offset, byte_len,
                complete, crc_status, crc_stored, crc_computed, crc_ok, message_count,
                developer_fields)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            sha256,
            chain_idx,
            header_size,
            protocol_version,
            profile_version,
            data_size,
            header_crc,
            h.header_crc_ok,
            byte_offset,
            byte_len,
            complete,
            crc_status,
            crc_stored,
            crc_computed,
            crc_ok,
            message_count,
            developer_fields
        )
        .execute(&mut **tx)
        .await?;

        for m in &file.messages {
            let msg_idx = i64::from(m.index);
            let (global, local, offset) =
                (i64::from(m.global), i64::from(m.local), m.offset as i64);
            let timestamp = m.timestamp().map(i64::from);
            let time_offset = m.time_offset.map(i64::from);
            let fields = serde_json::to_string(&fields_json(&m.fields))?;
            let developer =
                serde_json::to_string(&m.developer.iter().map(dev_field_json).collect::<Vec<_>>())?;
            sqlx::query!(
                "INSERT INTO fit_messages (sha256, chain_idx, msg_idx, global, local, timestamp,
                    time_offset, byte_offset, fields, developer)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                sha256,
                chain_idx,
                msg_idx,
                global,
                local,
                timestamp,
                time_offset,
                offset,
                fields,
                developer
            )
            .execute(&mut **tx)
            .await?;
        }
    }

    let files = &parsed.decoded.files;
    let status = match (&parsed.error, files.iter().all(|f| f.complete)) {
        (None, true) => "ok",
        (_, _) if !files.is_empty() => "partial",
        _ => "error",
    };
    let profile_version = files
        .iter()
        .map(|f| i64::from(f.header.profile_version))
        .max();
    let crc_status = if files.is_empty() {
        None
    } else {
        Some(
            files
                .iter()
                .map(|f| crc_parts(f.crc).0)
                .find(|s| *s != "ok")
                .unwrap_or("ok"),
        )
    };
    sqlx::query!(
        "INSERT INTO file_parse (sha256, parser_version, profile_version, status, error, crc_status)
         VALUES (?, ?, ?, ?, ?, ?)",
        sha256,
        PARSER_VERSION,
        profile_version,
        status,
        parsed.error,
        crc_status
    )
    .execute(&mut **tx)
    .await?;
    crate::derived::persist(
        tx,
        sha256,
        &parsed.decoded,
        parsed.error.as_deref(),
        &parsed.derived,
    )
    .await?;
    Ok(())
}

pub fn base_type_name(t: BaseType) -> String {
    match t {
        BaseType::Enum => "enum".into(),
        BaseType::Sint8 => "sint8".into(),
        BaseType::Uint8 => "uint8".into(),
        BaseType::Sint16 => "sint16".into(),
        BaseType::Uint16 => "uint16".into(),
        BaseType::Sint32 => "sint32".into(),
        BaseType::Uint32 => "uint32".into(),
        BaseType::String => "string".into(),
        BaseType::Float32 => "float32".into(),
        BaseType::Float64 => "float64".into(),
        BaseType::Uint8z => "uint8z".into(),
        BaseType::Uint16z => "uint16z".into(),
        BaseType::Uint32z => "uint32z".into(),
        BaseType::Byte => "byte".into(),
        BaseType::Sint64 => "sint64".into(),
        BaseType::Uint64 => "uint64".into(),
        BaseType::Uint64z => "uint64z".into(),
        BaseType::Unknown(b) => format!("unknown:{b}"),
    }
}

/// Raw value: integers unscaled, null for invalid (and for non-finite floats,
/// whose bytes are in `raw`), strings as text, bytes as hex. A one-element
/// array is a scalar.
fn raw_value(v: &Value) -> Json {
    fn one_or_many<T: Into<Json> + Clone>(x: &[Option<T>]) -> Json {
        let items: Vec<Json> = x
            .iter()
            .map(|e| e.clone().map_or(Json::Null, Into::into))
            .collect();
        if items.len() == 1 {
            items.into_iter().next().unwrap()
        } else {
            Json::Array(items)
        }
    }
    match v {
        Value::Uint(x) => one_or_many(x),
        Value::Sint(x) => one_or_many(x),
        Value::Float(x) => one_or_many(x), // From<f64> maps NaN/inf to null
        Value::String(x) if x.len() == 1 => json!(x[0]),
        Value::String(x) => json!(x),
        Value::Bytes(x) => json!(crate::hex(x)),
    }
}

fn fields_json(fields: &[fit::Field]) -> Json {
    let mut obj = Map::new();
    for f in fields {
        let mut e = Map::new();
        e.insert("t".into(), json!(base_type_name(f.base_type)));
        if f.big_endian {
            e.insert("be".into(), json!(true));
        }
        e.insert("v".into(), raw_value(&f.value));
        e.insert("raw".into(), json!(crate::hex(&f.raw)));
        // A repeated field number in one message must not overwrite the first.
        let mut key = f.num.to_string();
        let mut n = 1;
        while obj.contains_key(&key) {
            key = format!("{}#{n}", f.num);
            n += 1;
        }
        obj.insert(key, Json::Object(e));
    }
    Json::Object(obj)
}

fn dev_field_json(f: &fit::DeveloperField) -> Json {
    json!({
        "developer_data_index": f.developer_data_index,
        "num": f.num,
        "v": raw_value(&f.value),
        "raw": crate::hex(&f.raw),
    })
}

fn dev_description_json(d: &fit::DeveloperFieldDescription) -> Json {
    json!({
        "developer_data_index": d.developer_data_index,
        "field_definition_number": d.field_definition_number,
        "base_type": base_type_name(d.base_type),
        "name": d.name,
        "units": d.units,
        "scale": d.scale,
        "offset": d.offset,
        "native_mesg_num": d.native_mesg_num,
        "native_field_num": d.native_field_num,
    })
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReparseReport {
    pub files: u64,
    pub fit_inputs: u64,
    /// Garmin Connect JSON envelopes (see `crate::connect`).
    pub connect_inputs: u64,
    pub fit_files: u64,
    pub messages: u64,
    /// Files whose decode returned an error or panicked.
    pub parse_errors: u64,
    /// Files whose blob was missing or corrupt (recorded as file_parse 'error').
    pub unreadable: u64,
}

async fn write_unreadable(
    tx: &mut Transaction<'_, Sqlite>,
    sha256: &str,
    reason: &str,
) -> Result<()> {
    clear(tx, sha256).await?;
    sqlx::query!(
        "INSERT INTO file_parse (sha256, parser_version, profile_version, status, error, crc_status)
         VALUES (?, ?, NULL, 'error', ?, NULL)",
        sha256,
        PARSER_VERSION,
        reason
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Rebuild every derived table from the file store. Files are processed in
/// sha256 order inside ONE `BEGIN IMMEDIATE` transaction (clear sample tables,
/// delete by sha256, reinsert), so readers never see half-rebuilt sample
/// tables and a failure rolls everything back. Writers (uploads) wait for the
/// lock while it runs. A missing or corrupt
/// blob is recorded as a `file_parse` error and the run continues; callers
/// should treat `unreadable > 0` as failure. File ownership (`file_owners`) is ledger data and
/// is kept: every file is derived into its owner's partition.
pub async fn reparse(pool: &SqlitePool, data_dir: &Path) -> Result<ReparseReport> {
    // Derived rows whose file is no longer in the ledger.
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query!("DELETE FROM fit_messages WHERE sha256 NOT IN (SELECT sha256 FROM files)")
        .execute(&mut *tx)
        .await?;
    sqlx::query!("DELETE FROM fit_files WHERE sha256 NOT IN (SELECT sha256 FROM files)")
        .execute(&mut *tx)
        .await?;
    sqlx::query!("DELETE FROM file_parse WHERE sha256 NOT IN (SELECT sha256 FROM files)")
        .execute(&mut *tx)
        .await?;
    // Sample tables are rebuilt from scratch: clear, then upsert every file in sha256 order.
    crate::derived::clear_samples(&mut tx, None).await?;
    let shas: Vec<String> = sqlx::query_scalar("SELECT sha256 FROM files ORDER BY sha256")
        .fetch_all(&mut *tx)
        .await?;
    let report = reparse_files(&mut tx, data_dir, shas).await?;
    tx.commit().await?;
    Ok(report)
}

/// Rebuild the derived rows of the files owned by the partitions `uids` (0 = files without an
/// owner), inside the caller's transaction. Used when files change owner (assign, delete a user,
/// a claim at upload): the sample rows of both partitions are cleared and re-derived, so rows
/// that lost a rank contest to a file that moved away come back.
pub async fn reparse_partitions(
    tx: &mut Transaction<'_, Sqlite>,
    data_dir: &Path,
    uids: &[i64],
) -> Result<ReparseReport> {
    crate::derived::clear_samples(tx, Some(uids)).await?;
    let mut shas = Vec::new();
    for uid in uids {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT f.sha256 FROM files f LEFT JOIN file_owners o ON o.sha256 = f.sha256
             WHERE COALESCE(o.user_id, 0) = ?",
        )
        .bind(uid)
        .fetch_all(&mut **tx)
        .await?;
        shas.extend(rows);
    }
    shas.sort();
    shas.dedup();
    reparse_files(tx, data_dir, shas).await
}

/// What a stored blob is, decided from its bytes.
enum Read {
    Fit(Parsed),
    Connect(Box<crate::connect::Parsed>),
    Other,
}

async fn reparse_files(
    tx: &mut Transaction<'_, Sqlite>,
    data_dir: &Path,
    shas: Vec<String>,
) -> Result<ReparseReport> {
    let mut report = ReparseReport {
        files: shas.len() as u64,
        ..Default::default()
    };
    for sha in shas {
        let (dir, sha_owned) = (data_dir.to_path_buf(), sha.clone());
        let read = tokio::task::spawn_blocking(move || -> Result<Read> {
            let bytes = crate::store::read_verified(&dir, &sha_owned)?;
            Ok(if crate::connect::looks_like_connect(&bytes) {
                Read::Connect(Box::new(crate::connect::parse(&bytes)))
            } else if looks_like_fit(&bytes) {
                Read::Fit(parse(&bytes))
            } else {
                Read::Other
            })
        })
        .await?;
        match read {
            Err(e) => {
                report.unreadable += 1;
                write_unreadable(tx, &sha, &format!("{e:#}")).await?;
            }
            Ok(Read::Other) => clear(tx, &sha).await?,
            Ok(Read::Connect(parsed)) => {
                report.connect_inputs += 1;
                clear(tx, &sha).await?;
                crate::connect::write(tx, &sha, &parsed).await?;
            }
            Ok(Read::Fit(parsed)) => {
                report.fit_inputs += 1;
                report.fit_files += parsed.decoded.files.len() as u64;
                report.messages += parsed
                    .decoded
                    .files
                    .iter()
                    .map(|f| f.messages.len() as u64)
                    .sum::<u64>();
                report.parse_errors += parsed.error.is_some() as u64;
                write(tx, &sha, &parsed).await?;
            }
        }
    }
    Ok(report)
}
