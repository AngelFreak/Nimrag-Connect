//! nimrag ingest server. Files are the source of truth (content-addressed on
//! disk); the SQLite database is a derived view that `reparse` can rebuild.

pub mod api;
pub mod auth;
pub mod backup;
pub mod connect;
pub mod db;
pub mod derive;
pub mod derived;
pub mod embedded;
pub mod export;
pub mod garmin;
pub mod gps_assist;
pub mod import;
pub mod ingest;
pub mod ledger;
pub mod seed;
pub mod settings;
pub mod store;
pub mod tiles;
pub mod tokens;
pub mod totp;
pub mod users;
pub mod weather;
pub mod web;
pub mod web_api;

use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
