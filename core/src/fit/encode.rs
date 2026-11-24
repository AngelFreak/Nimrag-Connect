//! FIT writer (stage 4a): typed values in, a complete FIT file out.
//!
//! Layers:
//! - [`FitWriter`] turns [`RawMessage`]s (global message number plus fields with
//!   a field number, a base type byte and bytes) into a file: header with its
//!   CRC, definition messages when a layout is new, data messages, file CRC.
//!   It knows nothing about the profile.
//! - [`MessageBuilder`] makes a [`RawMessage`] from **names**: the message and
//!   the field numbers, base types, scale/offset and enum values all come from
//!   `core::fit::profile` lookups by name, so no profile table lives in this
//!   file (docs/LICENSING.md, IMPLEMENTATION_PLAN.md "Profile-dependent code").
//!   Without a profile (the committed empty table), or with only Nimrag's own
//!   partial map when it lacks a name the writer asks for, every builder call
//!   answers [`EncodeError::Unavailable`]: encoding never produces wrong bytes.
//!
//! The base types (size, invalid value, little-endian bytes), the file
//! layout (14-byte header with CRC, definition then data records, trailing CRC)
//! and the rule "a definition message precedes the first data message that uses
//! a layout" are ported from Gadgetbridge commit 7257742b (AGPL-3.0-or-later):
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/fit/`
//! `FitFile.java` (`getOutgoingMessage`, `generateOutgoingDataPayload`, `Header`),
//! `RecordDefinition.java` (`generateOutgoingPayload`), `RecordHeader.java`
//! (`generateOutgoingDefinitionPayload`), `RecordData.java`
//! (`generateOutgoingDataPayload`), `FieldDefinition.java`, `FitLocalMessageBuilder.java`
//! and `baseTypes/BaseType.java`. Copyright (C) 2024-2026 Daniele Gobbetti,
//! José Rebelo, Thomas Kuehne and the other Gadgetbridge contributors. See
//! docs/PORTED.md. Behaviour is *inferred (Gadgetbridge)* until a real watch
//! accepts a file from it.
//!
//! What we do differently [ours]: values are typed per call and checked against
//! the base type's range (an out-of-range number is an error, never a wrapped
//! value); a layout is defined once per local message slot and redefined only
//! when the slot is reused (Gadgetbridge repeats the definition whenever the
//! record before used another layout, which is also valid FIT); the fields of a
//! message are written in ascending field number; local slots are handed out by
//! first use and recycled least-recently-used.
//!
//! The file CRC covers the header and the data. Gadgetbridge computes it over
//! the data only; the two are equal, because the header ends with its own CRC
//! and CRC-16/ARC over a block followed by its own CRC is zero.

use std::collections::BTreeMap;
use std::fmt;

use super::profile::{self, FieldInfo};
use super::BaseType;
use crate::crc;

/// FIT protocol version 1.0 in the header's version byte (major 1, minor 0).
/// *inferred (Gadgetbridge, `FitFile(List<RecordData>)`)*
pub const PROTOCOL_VERSION: u8 = 16;
/// Profile version in the header. A constant, as in Gadgetbridge (21117); the
/// watch only reads it as a version marker. *inferred (Gadgetbridge)*
pub const PROFILE_VERSION: u16 = 21117;
const HEADER_SIZE: u8 = 14;
/// Local message types 0 to 15.
const LOCAL_SLOTS: usize = 16;
/// The size byte of a field definition holds at most this.
const MAX_FIELD_BYTES: usize = 255;
/// The field count of a definition is one byte.
const MAX_FIELDS: usize = 255;

/// Why encoding failed. Every variant leaves no partial output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// This build cannot write the file: no FIT profile was compiled in
    /// (`NIMRAG_FIT_PROFILE=empty`), or only Nimrag's own partial map, which
    /// lacks a message, field or enum value the writer needs. Nothing is written.
    Unavailable,
    UnknownMessage(String),
    UnknownField {
        message: String,
        field: String,
    },
    UnknownSubfield {
        message: String,
        field: String,
        subfield: String,
    },
    /// The profile has no enum value of this name for the field's type.
    UnknownEnum {
        message: String,
        field: String,
        value: String,
    },
    /// The value kind does not fit the field's base type (a string for a
    /// number, a float for an enum, ...).
    WrongType {
        message: String,
        field: String,
    },
    /// The number is outside the base type's valid range (the invalid pattern
    /// counts as outside).
    OutOfRange {
        message: String,
        field: String,
    },
    /// A field would need more than 255 bytes.
    FieldTooLong {
        message: String,
        field: String,
    },
    /// The input does not describe a writable file (for example an empty
    /// workout); the text says what.
    Invalid(String),
    /// The data would not fit the header's 32-bit size.
    TooLarge,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::Unavailable => {
                f.write_str("no FIT profile is compiled in: FIT files cannot be written")
            }
            EncodeError::UnknownMessage(m) => write!(f, "the profile has no message {m}"),
            EncodeError::UnknownField { message, field } => {
                write!(f, "the profile has no field {message}.{field}")
            }
            EncodeError::UnknownSubfield {
                message,
                field,
                subfield,
            } => write!(
                f,
                "the profile has no subfield {message}.{field}.{subfield}"
            ),
            EncodeError::UnknownEnum {
                message,
                field,
                value,
            } => write!(f, "the profile has no value {value} for {message}.{field}"),
            EncodeError::WrongType { message, field } => {
                write!(f, "the value does not fit the type of {message}.{field}")
            }
            EncodeError::OutOfRange { message, field } => {
                write!(f, "the value is out of range for {message}.{field}")
            }
            EncodeError::FieldTooLong { message, field } => {
                write!(f, "{message}.{field} would be longer than 255 bytes")
            }
            EncodeError::Invalid(why) => f.write_str(why),
            EncodeError::TooLarge => f.write_str("the FIT file would be too large"),
        }
    }
}

impl std::error::Error for EncodeError {}

// ---- base types --------------------------------------------------------------

/// The FIT base type byte (type number in the low 5 bits, bit 7 set when the
/// type is wider than one byte), or `None` for `BaseType::Unknown`.
pub fn base_type_byte(bt: BaseType) -> Option<u8> {
    use BaseType::*;
    Some(match bt {
        Enum => 0x00,
        Sint8 => 0x01,
        Uint8 => 0x02,
        Sint16 => 0x83,
        Uint16 => 0x84,
        Sint32 => 0x85,
        Uint32 => 0x86,
        String => 0x07,
        Float32 => 0x88,
        Float64 => 0x89,
        Uint8z => 0x0A,
        Uint16z => 0x8B,
        Uint32z => 0x8C,
        Byte => 0x0D,
        Sint64 => 0x8E,
        Uint64 => 0x8F,
        Uint64z => 0x90,
        Unknown(_) => return None,
    })
}

/// Bytes of one element (a string counts one byte per character), or `None`
/// for `BaseType::Unknown`.
pub fn base_type_size(bt: BaseType) -> Option<usize> {
    use BaseType::*;
    Some(match bt {
        Enum | Sint8 | Uint8 | Uint8z | Byte | String => 1,
        Sint16 | Uint16 | Uint16z => 2,
        Sint32 | Uint32 | Uint32z | Float32 => 4,
        Float64 | Sint64 | Uint64 | Uint64z => 8,
        Unknown(_) => return None,
    })
}

/// The bytes of one element that means "no value" (little endian): all ones for
/// the plain integers, `enum` and `byte`, the largest positive number for signed
/// integers, zero for the `z` types, all ones for floats. A string's invalid
/// value is a single NUL.
pub fn invalid_bytes(bt: BaseType) -> Option<Vec<u8>> {
    use BaseType::*;
    let size = base_type_size(bt)?;
    Some(match bt {
        Sint8 => vec![0x7F],
        Sint16 => 0x7FFFi16.to_le_bytes().to_vec(),
        Sint32 => 0x7FFF_FFFFi32.to_le_bytes().to_vec(),
        Sint64 => i64::MAX.to_le_bytes().to_vec(),
        Uint8z | Uint16z | Uint32z | Uint64z | String => vec![0; size],
        _ => vec![0xFF; size],
    })
}

/// Little-endian bytes of an integer in a base type, `None` when the value is
/// not valid there. Valid is the whole range except the invalid pattern.
pub fn int_bytes(bt: BaseType, value: i128) -> Option<Vec<u8>> {
    use BaseType::*;
    let (lo, hi): (i128, i128) = match bt {
        Enum | Uint8 | Byte => (0, 0xFE),
        Uint16 => (0, 0xFFFE),
        Uint32 => (0, 0xFFFF_FFFE),
        Uint64 => (0, i128::from(u64::MAX) - 1),
        Uint8z => (1, 0xFF),
        Uint16z => (1, 0xFFFF),
        Uint32z => (1, 0xFFFF_FFFF),
        Uint64z => (1, i128::from(u64::MAX)),
        Sint8 => (i128::from(i8::MIN), 0x7E),
        Sint16 => (i128::from(i16::MIN), 0x7FFE),
        Sint32 => (i128::from(i32::MIN), 0x7FFF_FFFE),
        Sint64 => (i128::from(i64::MIN), i128::from(i64::MAX) - 1),
        _ => return None,
    };
    if value < lo || value > hi {
        return None;
    }
    let size = base_type_size(bt)?;
    Some(value.to_le_bytes()[..size].to_vec())
}

/// Little-endian bytes of a float in `float32` or `float64`; `None` for NaN,
/// infinity (not valid values) and other base types.
pub fn float_bytes(bt: BaseType, value: f64) -> Option<Vec<u8>> {
    if !value.is_finite() {
        return None;
    }
    match bt {
        BaseType::Float32 => {
            let v = value as f32;
            v.is_finite().then(|| v.to_le_bytes().to_vec())
        }
        BaseType::Float64 => Some(value.to_le_bytes().to_vec()),
        _ => None,
    }
}

// ---- raw messages ----------------------------------------------------------------

/// One field of a message to write: number, base type byte and the bytes of the
/// value (one or more elements, little endian).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawField {
    pub num: u8,
    pub base_type: u8,
    pub data: Vec<u8>,
}

/// One data message to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMessage {
    pub global: u16,
    pub fields: Vec<RawField>,
}

impl RawMessage {
    pub fn new(global: u16) -> Self {
        Self {
            global,
            fields: Vec::new(),
        }
    }

    /// Add (or replace) a field whose bytes are already encoded.
    pub fn with_field(mut self, num: u8, base_type: u8, data: Vec<u8>) -> Self {
        self.fields.retain(|f| f.num != num);
        self.fields.push(RawField {
            num,
            base_type,
            data,
        });
        self.fields.sort_by_key(|f| f.num);
        self
    }
}

// ---- the file writer ---------------------------------------------------------------

/// What identifies a definition: the message and its field layout.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Layout {
    global: u16,
    /// (field number, size in bytes, base type byte)
    fields: Vec<(u8, u8, u8)>,
}

/// Builds one FIT file. Push messages in file order, then [`finish`](Self::finish).
#[derive(Debug)]
pub struct FitWriter {
    data: Vec<u8>,
    slots: [Option<Layout>; LOCAL_SLOTS],
    last_used: [u64; LOCAL_SLOTS],
    tick: u64,
}

impl Default for FitWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl FitWriter {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            slots: Default::default(),
            last_used: [0; LOCAL_SLOTS],
            tick: 0,
        }
    }

    /// Append a message, preceded by its definition when no local slot holds
    /// that layout.
    pub fn push(&mut self, message: &RawMessage) -> Result<(), EncodeError> {
        let layout = layout_of(message)?;
        let (local, define) = self.slot_for(&layout);
        if define {
            self.data.extend(definition_record(local, &layout));
            self.slots[local] = Some(layout);
        }
        self.tick += 1;
        self.last_used[local] = self.tick;
        // Data record: header is the local type, then the fields in definition order.
        self.data.push(local as u8);
        for f in &message.fields {
            self.data.extend_from_slice(&f.data);
        }
        Ok(())
    }

    /// The slot holding this layout, or the one to define it in (a free one, else
    /// the least recently used). The flag says whether a definition is needed.
    fn slot_for(&self, layout: &Layout) -> (usize, bool) {
        if let Some(i) = self.slots.iter().position(|s| s.as_ref() == Some(layout)) {
            return (i, false);
        }
        let i = self
            .slots
            .iter()
            .position(Option::is_none)
            .unwrap_or_else(|| {
                (0..LOCAL_SLOTS)
                    .min_by_key(|&i| self.last_used[i])
                    .unwrap_or(0)
            });
        (i, true)
    }

    /// The finished file: header, records, CRC.
    pub fn finish(self) -> Result<Vec<u8>, EncodeError> {
        wrap_file(&self.data)
    }
}

/// The layout a message needs a definition for: its fields in ascending number
/// with their sizes and base types.
fn layout_of(message: &RawMessage) -> Result<Layout, EncodeError> {
    if message.fields.len() > MAX_FIELDS {
        return Err(EncodeError::Invalid("a message has too many fields".into()));
    }
    let mut fields = Vec::with_capacity(message.fields.len());
    for f in &message.fields {
        let size = u8::try_from(f.data.len())
            .map_err(|_| EncodeError::Invalid(format!("field {} is too long", f.num)))?;
        if f.data.is_empty() {
            return Err(EncodeError::Invalid(format!("field {} is empty", f.num)));
        }
        fields.push((f.num, size, f.base_type));
    }
    // Fields in ascending number, and no number twice, so equal layouts compare equal.
    if fields.windows(2).any(|w| w[0].0 >= w[1].0) {
        return Err(EncodeError::Invalid(
            "fields must be in ascending order, each number once".into(),
        ));
    }
    Ok(Layout {
        global: message.global,
        fields,
    })
}

/// A definition record for local slot `local`: header, reserved byte,
/// architecture (little endian), global number, field count and the fields.
fn definition_record(local: usize, layout: &Layout) -> Vec<u8> {
    let mut d = vec![0x40 | local as u8, 0, 0];
    d.extend_from_slice(&layout.global.to_le_bytes());
    d.push(layout.fields.len() as u8);
    for (num, size, base) in &layout.fields {
        d.extend_from_slice(&[*num, *size, *base]);
    }
    d
}

/// A FIT file around `data` (records): header with its CRC, the records, the
/// file CRC.
fn wrap_file(data: &[u8]) -> Result<Vec<u8>, EncodeError> {
    let size = u32::try_from(data.len()).map_err(|_| EncodeError::TooLarge)?;
    let mut out = Vec::with_capacity(HEADER_SIZE as usize + data.len() + 2);
    out.push(HEADER_SIZE);
    out.push(PROTOCOL_VERSION);
    out.extend_from_slice(&PROFILE_VERSION.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(b".FIT");
    let header_crc = crc::checksum(&out);
    out.extend_from_slice(&header_crc.to_le_bytes());
    out.extend_from_slice(data);
    let file_crc = crc::checksum(&out);
    out.extend_from_slice(&file_crc.to_le_bytes());
    Ok(out)
}

// ---- definitions and data records kept apart ---------------------------------------------

/// Definition records and data records kept apart, with a fixed local message
/// type per layout: what the weather answer needs (GFDI 5011 carries the
/// definitions, 5012 the data; neither has a FIT file header). Every message
/// pushed to one local type must have the same layout, as in Gadgetbridge's
/// `FitLocalMessageBuilder`: a different one is an error, never silently
/// re-defined. *inferred (Gadgetbridge)*
#[derive(Debug, Default)]
pub struct RecordSet {
    layouts: [Option<Layout>; LOCAL_SLOTS],
    definitions: Vec<u8>,
    records: Vec<Vec<u8>>,
}

impl RecordSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a data record of local type `local` (0 to 15). The first message of
    /// a local type adds its definition.
    pub fn push(&mut self, local: u8, message: &RawMessage) -> Result<(), EncodeError> {
        let slot = usize::from(local);
        if slot >= LOCAL_SLOTS {
            return Err(EncodeError::Invalid(
                "local message type out of range".into(),
            ));
        }
        let layout = layout_of(message)?;
        match &self.layouts[slot] {
            Some(known) if *known != layout => {
                return Err(EncodeError::Invalid(format!(
                    "local message type {local} is used with two layouts"
                )))
            }
            Some(_) => {}
            None => {
                self.definitions.extend(definition_record(slot, &layout));
                self.layouts[slot] = Some(layout);
            }
        }
        let mut record = vec![local];
        for f in &message.fields {
            record.extend_from_slice(&f.data);
        }
        self.records.push(record);
        Ok(())
    }

    /// All definition records, in the order the local types were first used.
    pub fn definitions(&self) -> &[u8] {
        &self.definitions
    }

    /// Every data record (header byte included), in push order.
    pub fn records(&self) -> &[Vec<u8>] {
        &self.records
    }

    pub fn into_parts(self) -> (Vec<u8>, Vec<Vec<u8>>) {
        (self.definitions, self.records)
    }

    /// The same records as a complete FIT file (all definitions first, then
    /// the data), for tests and tools.
    pub fn to_file(&self) -> Result<Vec<u8>, EncodeError> {
        records_to_file(&self.definitions, &self.records)
    }
}

/// Definition records and data records (as a [`RecordSet`] hands them out) as
/// one complete FIT file: header, all definitions, then the data, CRC. For
/// tests and tools; the watch gets the records without a header.
pub fn records_to_file(definitions: &[u8], records: &[Vec<u8>]) -> Result<Vec<u8>, EncodeError> {
    let mut data = definitions.to_vec();
    for r in records {
        data.extend_from_slice(r);
    }
    wrap_file(&data)
}

// ---- typed messages by profile name --------------------------------------------------

/// A name the compiled-in table lacks. With a Garmin profile that is a specific
/// error (the writer asks for something the profile should have). With only
/// Nimrag's own partial map (`cfg(fit_map)`) it means this build cannot write
/// that message: [`EncodeError::Unavailable`], the same answer as no profile.
fn miss(e: EncodeError) -> EncodeError {
    if cfg!(fit_map) {
        EncodeError::Unavailable
    } else {
        e
    }
}

/// Builds one message from names. Everything it needs is looked up in the
/// profile; the first lookup fails with [`EncodeError::Unavailable`] when there
/// is none.
#[derive(Debug)]
pub struct MessageBuilder {
    name: String,
    global: u16,
    fields: BTreeMap<u8, RawField>,
}

impl MessageBuilder {
    /// Start a message of this profile name, for example `"workout_step"`.
    pub fn new(message: &str) -> Result<Self, EncodeError> {
        if !profile::available() {
            return Err(EncodeError::Unavailable);
        }
        let global = profile::message_number(message)
            .ok_or_else(|| miss(EncodeError::UnknownMessage(message.to_owned())))?;
        Ok(Self {
            name: message.to_owned(),
            global,
            fields: BTreeMap::new(),
        })
    }

    /// The message's global number.
    pub fn global(&self) -> u16 {
        self.global
    }

    fn lookup(&self, field: &str) -> Result<(u8, &'static FieldInfo), EncodeError> {
        profile::field_by_name(self.global, field).ok_or_else(|| {
            miss(EncodeError::UnknownField {
                message: self.name.clone(),
                field: field.to_owned(),
            })
        })
    }

    fn wrong(&self, field: &str) -> EncodeError {
        EncodeError::WrongType {
            message: self.name.clone(),
            field: field.to_owned(),
        }
    }

    fn range(&self, field: &str) -> EncodeError {
        EncodeError::OutOfRange {
            message: self.name.clone(),
            field: field.to_owned(),
        }
    }

    fn store(
        &mut self,
        num: u8,
        bt: BaseType,
        data: Vec<u8>,
        field: &str,
    ) -> Result<(), EncodeError> {
        if data.len() > MAX_FIELD_BYTES {
            return Err(EncodeError::FieldTooLong {
                message: self.name.clone(),
                field: field.to_owned(),
            });
        }
        let base_type = base_type_byte(bt).ok_or_else(|| self.wrong(field))?;
        self.fields.insert(
            num,
            RawField {
                num,
                base_type,
                data,
            },
        );
        Ok(())
    }

    /// An integer in the field's own base type. Floats are not accepted here.
    pub fn int(&mut self, field: &str, value: i64) -> Result<&mut Self, EncodeError> {
        let (num, info) = self.lookup(field)?;
        let bytes = int_bytes(info.base_type, i128::from(value));
        match bytes {
            Some(b) => self.store(num, info.base_type, b, field)?,
            None if base_type_size(info.base_type).is_none() || is_non_numeric(info.base_type) => {
                return Err(self.wrong(field))
            }
            None => return Err(self.range(field)),
        }
        Ok(self)
    }

    pub fn uint(&mut self, field: &str, value: u64) -> Result<&mut Self, EncodeError> {
        let (num, info) = self.lookup(field)?;
        match int_bytes(info.base_type, i128::from(value)) {
            Some(b) => self.store(num, info.base_type, b, field)?,
            None if is_non_numeric(info.base_type) => return Err(self.wrong(field)),
            None => return Err(self.range(field)),
        }
        Ok(self)
    }

    /// A float in a `float32` or `float64` field.
    pub fn float(&mut self, field: &str, value: f64) -> Result<&mut Self, EncodeError> {
        let (num, info) = self.lookup(field)?;
        match float_bytes(info.base_type, value) {
            Some(b) => self.store(num, info.base_type, b, field)?,
            None if matches!(info.base_type, BaseType::Float32 | BaseType::Float64) => {
                return Err(self.range(field))
            }
            None => return Err(self.wrong(field)),
        }
        Ok(self)
    }

    /// A string field: UTF-8 bytes plus one NUL. An empty string leaves the
    /// field out. A longer text than fits (255 bytes with the NUL) is an error.
    pub fn string(&mut self, field: &str, value: &str) -> Result<&mut Self, EncodeError> {
        let (num, info) = self.lookup(field)?;
        if info.base_type != BaseType::String {
            return Err(self.wrong(field));
        }
        if value.is_empty() {
            self.fields.remove(&num);
            return Ok(self);
        }
        let mut data = value.as_bytes().to_vec();
        data.push(0);
        self.store(num, BaseType::String, data, field)?;
        Ok(self)
    }

    /// An array of integers (every element in the field's base type).
    pub fn ints(&mut self, field: &str, values: &[i64]) -> Result<&mut Self, EncodeError> {
        let (num, info) = self.lookup(field)?;
        let mut data = Vec::new();
        for v in values {
            data.extend(int_bytes(info.base_type, i128::from(*v)).ok_or_else(|| {
                if is_non_numeric(info.base_type) {
                    self.wrong(field)
                } else {
                    self.range(field)
                }
            })?);
        }
        if data.is_empty() {
            return Err(self.range(field));
        }
        self.store(num, info.base_type, data, field)?;
        Ok(self)
    }

    /// Raw bytes for a `byte` field.
    pub fn bytes(&mut self, field: &str, value: &[u8]) -> Result<&mut Self, EncodeError> {
        let (num, info) = self.lookup(field)?;
        if info.base_type != BaseType::Byte || value.is_empty() {
            return Err(self.wrong(field));
        }
        self.store(num, BaseType::Byte, value.to_vec(), field)?;
        Ok(self)
    }

    /// The value of an enum field by its profile name, for example
    /// (`"intensity"`, `"warmup"`). The field's profile type names the table.
    pub fn enum_name(&mut self, field: &str, value: &str) -> Result<&mut Self, EncodeError> {
        let (num, info) = self.lookup(field)?;
        let raw = profile::enum_value(info.type_name, value).ok_or_else(|| {
            miss(EncodeError::UnknownEnum {
                message: self.name.clone(),
                field: field.to_owned(),
                value: value.to_owned(),
            })
        })?;
        match int_bytes(info.base_type, i128::from(raw)) {
            Some(b) => self.store(num, info.base_type, b, field)?,
            None => return Err(self.range(field)),
        }
        Ok(self)
    }

    /// The value of a field that has a subfield with an enum type, by the
    /// subfield's profile name: (`"product"`, `"garmin_product"`, `"connect"`).
    /// The field's own base type is used.
    pub fn sub_enum_name(
        &mut self,
        field: &str,
        subfield: &str,
        value: &str,
    ) -> Result<&mut Self, EncodeError> {
        let (num, info) = self.lookup(field)?;
        let sub = info
            .subfields
            .iter()
            .find(|s| s.name == subfield)
            .ok_or_else(|| {
                miss(EncodeError::UnknownSubfield {
                    message: self.name.clone(),
                    field: field.to_owned(),
                    subfield: subfield.to_owned(),
                })
            })?;
        let raw = profile::enum_value(sub.type_name, value).ok_or_else(|| {
            miss(EncodeError::UnknownEnum {
                message: self.name.clone(),
                field: field.to_owned(),
                value: value.to_owned(),
            })
        })?;
        match int_bytes(info.base_type, i128::from(raw)) {
            Some(b) => self.store(num, info.base_type, b, field)?,
            None => return Err(self.range(field)),
        }
        Ok(self)
    }

    /// Elements already in the field's base type, little endian, exactly as they
    /// are to be stored (for arrays whose elements include the invalid value, such
    /// as a NUL-terminated text in a `uint8z` array). The length must be a
    /// multiple of the base type's size.
    pub fn raw_elements(&mut self, field: &str, data: &[u8]) -> Result<&mut Self, EncodeError> {
        let (num, info) = self.lookup(field)?;
        let size = base_type_size(info.base_type).ok_or_else(|| self.wrong(field))?;
        if data.is_empty() || data.len() % size != 0 {
            return Err(self.wrong(field));
        }
        self.store(num, info.base_type, data.to_vec(), field)?;
        Ok(self)
    }

    /// A physical value, stored as `(value + offset) * scale` with the field's
    /// own scale and offset from the profile, rounded to an integer.
    pub fn scaled(&mut self, field: &str, value: f64) -> Result<&mut Self, EncodeError> {
        let (num, info) = self.lookup(field)?;
        let raw = (value + info.offset.unwrap_or(0.0)) * info.scale.unwrap_or(1.0);
        self.store_scaled(num, info.base_type, raw, field)?;
        Ok(self)
    }

    /// Like [`scaled`](Self::scaled), with the scale and offset of one of the
    /// field's subfields (for example `workout_step.duration_value`, subfield
    /// `duration_time`: seconds, stored in milliseconds). The field's own base
    /// type is used.
    pub fn sub_scaled(
        &mut self,
        field: &str,
        subfield: &str,
        value: f64,
    ) -> Result<&mut Self, EncodeError> {
        let (num, info) = self.lookup(field)?;
        let sub = info
            .subfields
            .iter()
            .find(|s| s.name == subfield)
            .ok_or_else(|| {
                miss(EncodeError::UnknownSubfield {
                    message: self.name.clone(),
                    field: field.to_owned(),
                    subfield: subfield.to_owned(),
                })
            })?;
        let raw = (value + sub.offset.unwrap_or(0.0)) * sub.scale.unwrap_or(1.0);
        self.store_scaled(num, info.base_type, raw, field)?;
        Ok(self)
    }

    fn store_scaled(
        &mut self,
        num: u8,
        bt: BaseType,
        raw: f64,
        field: &str,
    ) -> Result<(), EncodeError> {
        if !raw.is_finite() || raw.abs() > 1.0e18 {
            return Err(self.range(field));
        }
        match int_bytes(bt, raw.round() as i128) {
            Some(b) => self.store(num, bt, b, field),
            None if is_non_numeric(bt) => Err(self.wrong(field)),
            None => Err(self.range(field)),
        }
    }

    /// Write the field's invalid value ("no value") explicitly.
    pub fn invalid(&mut self, field: &str) -> Result<&mut Self, EncodeError> {
        let (num, info) = self.lookup(field)?;
        let bytes = invalid_bytes(info.base_type).ok_or_else(|| self.wrong(field))?;
        self.store(num, info.base_type, bytes, field)?;
        Ok(self)
    }

    /// The finished message.
    pub fn build(self) -> RawMessage {
        RawMessage {
            global: self.global,
            fields: self.fields.into_values().collect(),
        }
    }
}

fn is_non_numeric(bt: BaseType) -> bool {
    matches!(
        bt,
        BaseType::String | BaseType::Float32 | BaseType::Float64 | BaseType::Unknown(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fit::{decode, Crc, Value};

    const ALL: [BaseType; 17] = [
        BaseType::Enum,
        BaseType::Sint8,
        BaseType::Uint8,
        BaseType::Sint16,
        BaseType::Uint16,
        BaseType::Sint32,
        BaseType::Uint32,
        BaseType::String,
        BaseType::Float32,
        BaseType::Float64,
        BaseType::Uint8z,
        BaseType::Uint16z,
        BaseType::Uint32z,
        BaseType::Byte,
        BaseType::Sint64,
        BaseType::Uint64,
        BaseType::Uint64z,
    ];

    #[test]
    fn base_type_bytes_sizes_and_invalid_patterns() {
        let table: [(BaseType, u8, usize, &[u8]); 17] = [
            (BaseType::Enum, 0x00, 1, &[0xFF]),
            (BaseType::Sint8, 0x01, 1, &[0x7F]),
            (BaseType::Uint8, 0x02, 1, &[0xFF]),
            (BaseType::Sint16, 0x83, 2, &[0xFF, 0x7F]),
            (BaseType::Uint16, 0x84, 2, &[0xFF, 0xFF]),
            (BaseType::Sint32, 0x85, 4, &[0xFF, 0xFF, 0xFF, 0x7F]),
            (BaseType::Uint32, 0x86, 4, &[0xFF; 4]),
            (BaseType::String, 0x07, 1, &[0]),
            (BaseType::Float32, 0x88, 4, &[0xFF; 4]),
            (BaseType::Float64, 0x89, 8, &[0xFF; 8]),
            (BaseType::Uint8z, 0x0A, 1, &[0]),
            (BaseType::Uint16z, 0x8B, 2, &[0, 0]),
            (BaseType::Uint32z, 0x8C, 4, &[0; 4]),
            (BaseType::Byte, 0x0D, 1, &[0xFF]),
            (
                BaseType::Sint64,
                0x8E,
                8,
                &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F],
            ),
            (BaseType::Uint64, 0x8F, 8, &[0xFF; 8]),
            (BaseType::Uint64z, 0x90, 8, &[0; 8]),
        ];
        for (bt, byte, size, invalid) in table {
            assert_eq!(base_type_byte(bt), Some(byte), "{bt:?}");
            assert_eq!(base_type_size(bt), Some(size), "{bt:?}");
            assert_eq!(invalid_bytes(bt).as_deref(), Some(invalid), "{bt:?}");
        }
        assert_eq!(base_type_byte(BaseType::Unknown(31)), None);
        assert_eq!(base_type_size(BaseType::Unknown(31)), None);
        assert_eq!(invalid_bytes(BaseType::Unknown(31)), None);
    }

    #[test]
    fn the_decoder_reads_back_every_base_type_byte_as_the_same_type() {
        // A one-field message per base type, written with our invalid pattern: the
        // decoder reports the value as absent for every type that has one.
        for bt in ALL {
            let invalid = invalid_bytes(bt).unwrap();
            let mut w = FitWriter::new();
            w.push(&RawMessage::new(7).with_field(0, base_type_byte(bt).unwrap(), invalid))
                .unwrap();
            let file = w.finish().unwrap();
            let decoded = decode(&file);
            assert_eq!(decoded.error, None, "{bt:?}");
            let field = &decoded.files[0].messages[0].fields[0];
            assert_eq!(field.base_type, bt, "{bt:?}");
            match &field.value {
                Value::Uint(v) => assert_eq!(v, &vec![None], "{bt:?}"),
                Value::Sint(v) => assert_eq!(v, &vec![None], "{bt:?}"),
                Value::Float(v) => assert_eq!(v, &vec![None], "{bt:?}"),
                Value::String(v) => assert!(v.is_empty(), "{bt:?}"),
                Value::Bytes(v) => assert_eq!(v, &vec![0xFF], "{bt:?}"),
            }
        }
    }

    #[test]
    fn integers_are_range_checked_and_little_endian() {
        assert_eq!(int_bytes(BaseType::Uint8, 254), Some(vec![254]));
        assert_eq!(int_bytes(BaseType::Uint8, 255), None, "the invalid pattern");
        assert_eq!(int_bytes(BaseType::Uint8, -1), None);
        assert_eq!(int_bytes(BaseType::Uint16, 0x1234), Some(vec![0x34, 0x12]));
        assert_eq!(int_bytes(BaseType::Uint32, 0xFFFF_FFFF), None);
        assert_eq!(
            int_bytes(BaseType::Uint32, 0xFFFF_FFFE),
            Some(vec![0xFE, 0xFF, 0xFF, 0xFF])
        );
        assert_eq!(int_bytes(BaseType::Sint16, -2), Some(vec![0xFE, 0xFF]));
        assert_eq!(int_bytes(BaseType::Sint8, 127), None);
        assert_eq!(
            int_bytes(BaseType::Uint8z, 0),
            None,
            "zero is invalid in z types"
        );
        assert_eq!(int_bytes(BaseType::Uint32z, 32), Some(vec![32, 0, 0, 0]));
        assert_eq!(int_bytes(BaseType::Enum, 255), None);
        assert_eq!(int_bytes(BaseType::Float32, 1), None);
        assert_eq!(int_bytes(BaseType::String, 1), None);
        assert_eq!(
            int_bytes(BaseType::Uint64, i128::from(u64::MAX) - 1),
            Some((u64::MAX - 1).to_le_bytes().to_vec())
        );
    }

    #[test]
    fn floats_are_little_endian_and_finite() {
        assert_eq!(
            float_bytes(BaseType::Float32, 1.5),
            Some(1.5f32.to_le_bytes().to_vec())
        );
        assert_eq!(
            float_bytes(BaseType::Float64, -2.25),
            Some((-2.25f64).to_le_bytes().to_vec())
        );
        assert_eq!(float_bytes(BaseType::Float32, f64::NAN), None);
        assert_eq!(float_bytes(BaseType::Float32, f64::INFINITY), None);
        assert_eq!(
            float_bytes(BaseType::Float32, 1.0e300),
            None,
            "overflows f32"
        );
        assert_eq!(float_bytes(BaseType::Uint8, 1.0), None);
    }

    /// A file made by hand, byte for byte: header, definition, data, CRC.
    #[test]
    fn a_small_file_has_exactly_these_bytes() {
        let mut w = FitWriter::new();
        let m = RawMessage::new(0)
            .with_field(4, 0x86, 1000u32.to_le_bytes().to_vec())
            .with_field(0, 0x00, vec![5]);
        w.push(&m).unwrap();
        w.push(&m).unwrap(); // same layout: no second definition
        let file = w.finish().unwrap();
        let mut expect = vec![
            14, 16, 0x7D, 0x52, // header size, protocol 1.0, profile 21117 little endian
            0x18, 0, 0, 0, // data size: 24 bytes
            b'.', b'F', b'I', b'T',
        ];
        let records: Vec<u8> = vec![
            0x40, 0, 0, 0, 0, 2, // definition: local 0, LE, global 0, 2 fields
            0, 1, 0x00, // field 0, 1 byte, enum
            4, 4, 0x86, // field 4, 4 bytes, uint32
            0x00, 5, 0xE8, 3, 0, 0, // data: local 0, type 5, 1000
            0x00, 5, 0xE8, 3, 0, 0, // the same again, without a definition
        ];
        assert_eq!(records.len(), 24);
        let hcrc = crc::checksum(&expect);
        expect.extend_from_slice(&hcrc.to_le_bytes());
        expect.extend_from_slice(&records);
        let fcrc = crc::checksum(&expect);
        expect.extend_from_slice(&fcrc.to_le_bytes());
        assert_eq!(file, expect);
        // 21117 = 0x527D
        assert_eq!(&file[2..4], &[0x7D, 0x52]);
    }

    #[test]
    fn our_own_decoder_accepts_the_file_and_both_crcs() {
        let mut w = FitWriter::new();
        w.push(
            &RawMessage::new(20)
                .with_field(3, 0x02, vec![142])
                .with_field(253, 0x86, 977_000_000u32.to_le_bytes().to_vec()),
        )
        .unwrap();
        let file = w.finish().unwrap();
        let d = decode(&file);
        assert_eq!(d.error, None);
        let f = &d.files[0];
        assert!(f.complete);
        assert_eq!(f.crc, Crc::Ok);
        assert!(f.header.header_crc_ok);
        assert!(f.header.header_crc.is_some());
        assert_eq!(f.header.protocol_version, 16);
        assert_eq!(f.header.profile_version, 21117);
        assert_eq!(f.header.data_size as usize, file.len() - 16);
        assert_eq!(f.messages[0].global, 20);
        assert_eq!(f.messages[0].timestamp(), Some(977_000_000));
    }

    #[test]
    fn the_file_crc_over_header_and_data_equals_the_crc_over_the_data_alone() {
        // Gadgetbridge computes it over the data only; the two agree because the
        // header carries its own CRC.
        let mut w = FitWriter::new();
        w.push(&RawMessage::new(1).with_field(0, 0x02, vec![9]))
            .unwrap();
        let file = w.finish().unwrap();
        let data = &file[14..file.len() - 2];
        let stored = u16::from_le_bytes([file[file.len() - 2], file[file.len() - 1]]);
        assert_eq!(crc::checksum(data), stored);
        assert_eq!(crc::checksum(&file[..file.len() - 2]), stored);
    }

    #[test]
    fn a_new_layout_gets_a_new_slot_and_a_definition() {
        let mut w = FitWriter::new();
        let a = RawMessage::new(2).with_field(0, 0x02, vec![1]);
        let b = RawMessage::new(2)
            .with_field(0, 0x02, vec![1])
            .with_field(1, 0x02, vec![2]);
        for m in [&a, &b, &a, &b] {
            w.push(m).unwrap();
        }
        let d = decode(&w.finish().unwrap());
        assert_eq!(d.error, None);
        let m = &d.files[0].messages;
        assert_eq!(m.len(), 4);
        assert_eq!(
            (m[0].local, m[1].local, m[2].local, m[3].local),
            (0, 1, 0, 1)
        );
        assert_eq!(m[1].fields.len(), 2);
    }

    #[test]
    fn more_layouts_than_slots_recycle_the_least_recently_used() {
        let mut w = FitWriter::new();
        let msgs: Vec<RawMessage> = (0..20u16)
            .map(|i| RawMessage::new(1000 + i).with_field(0, 0x02, vec![i as u8]))
            .collect();
        for m in &msgs {
            w.push(m).unwrap();
        }
        // The first layout was recycled: pushing it again redefines it.
        w.push(&msgs[0]).unwrap();
        let d = decode(&w.finish().unwrap());
        assert_eq!(d.error, None);
        let m = &d.files[0].messages;
        assert_eq!(m.len(), 21);
        for (i, msg) in m.iter().enumerate() {
            let want = if i == 20 { 0 } else { i as u16 };
            assert_eq!(msg.global, 1000 + want, "message {i}");
        }
        assert!(m.iter().all(|x| x.local < 16));
    }

    #[test]
    fn bad_raw_messages_are_refused() {
        let mut w = FitWriter::new();
        let empty = RawMessage {
            global: 1,
            fields: vec![RawField {
                num: 0,
                base_type: 2,
                data: vec![],
            }],
        };
        assert!(matches!(w.push(&empty), Err(EncodeError::Invalid(_))));
        let long = RawMessage::new(1).with_field(0, 0x07, vec![b'a'; 256]);
        assert!(matches!(w.push(&long), Err(EncodeError::Invalid(_))));
        let unsorted = RawMessage {
            global: 1,
            fields: vec![
                RawField {
                    num: 2,
                    base_type: 2,
                    data: vec![1],
                },
                RawField {
                    num: 1,
                    base_type: 2,
                    data: vec![1],
                },
            ],
        };
        assert!(matches!(w.push(&unsorted), Err(EncodeError::Invalid(_))));
        // Nothing of the refused messages was written.
        let d = decode(&w.finish().unwrap());
        assert!(d.files[0].messages.is_empty());
    }

    // ---- the profile-dependent part ---------------------------------------------

    /// The committed configuration has no profile: encoding must say so and
    /// never return bytes. (This test runs when `NIMRAG_FIT_PROFILE=none`.)
    #[cfg(not(any(fit_profile, fit_map)))]
    #[test]
    fn without_a_profile_encoding_is_unavailable() {
        assert!(!profile::available());
        assert_eq!(
            MessageBuilder::new("file_id").unwrap_err(),
            EncodeError::Unavailable
        );
        assert_eq!(
            MessageBuilder::new("no_such_message").unwrap_err(),
            EncodeError::Unavailable
        );
        assert!(EncodeError::Unavailable
            .to_string()
            .contains("no FIT profile"));
    }

    /// With only Nimrag's own partial map a name it lacks means "this build cannot
    /// write that": `Unavailable`, never a specific error and never bytes.
    #[cfg(fit_map)]
    #[test]
    fn with_the_partial_map_missing_names_are_unavailable() {
        assert!(profile::available());
        // file_id is in the map ...
        let mut id = MessageBuilder::new("file_id").unwrap();
        id.uint("serial_number", 1).unwrap();
        // ... with the `workout` file type and the connect product (observed in Garmin's
        // workout files), but not a field it does not have
        id.enum_name("type", "workout").unwrap();
        id.sub_enum_name("product", "garmin_product", "connect")
            .unwrap();
        assert_eq!(
            id.uint("no_such_field", 1).unwrap_err(),
            EncodeError::Unavailable
        );
        assert_eq!(
            id.enum_name("type", "no_such_file_type").unwrap_err(),
            EncodeError::Unavailable
        );
        // a message the map lacks
        assert_eq!(
            MessageBuilder::new("no_such_message").unwrap_err(),
            EncodeError::Unavailable
        );
    }

    #[cfg(fit_profile)]
    #[test]
    fn typed_values_round_trip_through_our_decoder_and_interpreter() {
        use crate::fit::interpret::interpret;
        let mut id = MessageBuilder::new("file_id").unwrap();
        id.enum_name("type", "workout").unwrap();
        id.enum_name("manufacturer", "garmin").unwrap();
        id.uint("serial_number", 1).unwrap();
        id.uint("time_created", 977_000_000).unwrap();
        id.string("product_name", "Nimrag").unwrap();
        let mut rec = MessageBuilder::new("record").unwrap();
        rec.uint("heart_rate", 150).unwrap();
        rec.scaled("distance", 12.5).unwrap(); // metres, stored in centimetres
        rec.int("position_lat", -123_456).unwrap();
        let mut w = FitWriter::new();
        w.push(&id.build()).unwrap();
        w.push(&rec.build()).unwrap();
        let file = w.finish().unwrap();

        let d = decode(&file);
        assert_eq!(d.error, None);
        assert_eq!(d.files[0].crc, Crc::Ok);
        let msgs = interpret(&d.files[0]);
        assert_eq!(msgs[0].message_name, Some("file_id"));
        assert_eq!(msgs[0].get("type").unwrap().enum_label, Some("workout"));
        assert_eq!(
            msgs[0].get("manufacturer").unwrap().enum_label,
            Some("garmin")
        );
        assert_eq!(msgs[0].number("serial_number"), Some(1.0));
        assert_eq!(msgs[0].number("time_created"), Some(977_000_000.0));
        assert_eq!(msgs[1].message_name, Some("record"));
        assert_eq!(msgs[1].number("heart_rate"), Some(150.0));
        assert_eq!(msgs[1].number("distance"), Some(12.5));
        assert_eq!(msgs[1].number("position_lat"), Some(-123_456.0));
    }

    #[cfg(fit_profile)]
    #[test]
    fn builder_errors_name_the_message_and_field() {
        let mut m = MessageBuilder::new("record").unwrap();
        assert_eq!(
            MessageBuilder::new("no_such_message").unwrap_err(),
            EncodeError::UnknownMessage("no_such_message".into())
        );
        assert_eq!(
            m.uint("no_such_field", 1).unwrap_err(),
            EncodeError::UnknownField {
                message: "record".into(),
                field: "no_such_field".into()
            }
        );
        // heart_rate is a uint8: 255 is the invalid pattern, 300 does not fit, a float is wrong.
        for v in [255u64, 300] {
            assert!(matches!(
                m.uint("heart_rate", v).unwrap_err(),
                EncodeError::OutOfRange { .. }
            ));
        }
        assert!(matches!(
            m.float("heart_rate", 1.5).unwrap_err(),
            EncodeError::WrongType { .. }
        ));
        assert!(matches!(
            m.string("heart_rate", "x").unwrap_err(),
            EncodeError::WrongType { .. }
        ));
        assert!(matches!(
            m.enum_name("heart_rate", "x").unwrap_err(),
            EncodeError::UnknownEnum { .. }
        ));
        assert!(matches!(
            m.sub_scaled("distance", "nope", 1.0).unwrap_err(),
            EncodeError::UnknownSubfield { .. }
        ));
        let mut id = MessageBuilder::new("file_id").unwrap();
        assert!(matches!(
            id.enum_name("type", "no_such_type_value").unwrap_err(),
            EncodeError::UnknownEnum { .. }
        ));
        // A failed call leaves the message as it was.
        assert!(m.build().fields.is_empty());
    }

    #[cfg(fit_profile)]
    #[test]
    fn setting_a_field_twice_keeps_the_last_value_and_an_empty_string_clears_it() {
        let mut m = MessageBuilder::new("workout").unwrap();
        m.string("wkt_name", "First").unwrap();
        m.string("wkt_name", "Second").unwrap();
        let raw = m.build();
        assert_eq!(raw.fields.len(), 1);
        assert_eq!(raw.fields[0].data, b"Second\0");
        let mut m = MessageBuilder::new("workout").unwrap();
        m.string("wkt_name", "x").unwrap();
        m.string("wkt_name", "").unwrap();
        assert!(m.build().fields.is_empty());
    }

    #[cfg(fit_profile)]
    #[test]
    fn a_field_over_255_bytes_is_refused() {
        let mut m = MessageBuilder::new("workout").unwrap();
        let long = "a".repeat(255); // plus the NUL: 256
        assert!(matches!(
            m.string("wkt_name", &long).unwrap_err(),
            EncodeError::FieldTooLong { .. }
        ));
        assert!(m.string("wkt_name", &"a".repeat(254)).is_ok());
    }

    #[cfg(fit_profile)]
    #[test]
    fn invalid_values_and_subfield_scales_are_written_as_the_profile_says() {
        let mut m = MessageBuilder::new("workout_step").unwrap();
        m.invalid("target_value").unwrap();
        m.sub_scaled("duration_value", "duration_time", 600.0)
            .unwrap();
        let raw = m.build();
        let by = |n: &str| {
            let (num, _) = profile::field_by_name(raw.global, n).unwrap();
            raw.fields
                .iter()
                .find(|f| f.num == num)
                .unwrap()
                .data
                .clone()
        };
        assert_eq!(by("target_value"), vec![0xFF; 4]);
        assert_eq!(by("duration_value"), 600_000u32.to_le_bytes().to_vec());
    }
}
