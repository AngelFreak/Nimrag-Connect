//! nimrag core: sans-IO Garmin watch link protocol and capture decoding.
//!
//! No Bluetooth or network calls live here: callers pass bytes in and get
//! bytes and events out. Protocol reference: docs/protocol/PROTOCOL.md.
//! This crate is original code; see docs/LICENSING.md.

pub mod capture;
pub mod cobs;
pub mod crc;
pub mod describe;
pub mod gfdi;
pub mod link;
pub mod mlr;
pub mod multilink;
