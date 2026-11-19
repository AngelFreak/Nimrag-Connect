//! FIT file decoding (contract; implementation in `decode.rs`, names in `profile.rs`).
//!
//! Design rules (hard rule 3 "never invent data", hard rule 2 "files are the
//! source of truth"):
//! - Nothing is dropped. Unknown messages, manufacturer-specific messages,
//!   unknown fields and developer fields are all kept, with their raw bytes.
//! - Decoding is sans-IO and total: `decode` takes the whole input as bytes and
//!   never panics on any input.
//!
//! Decoder state rules (contract v2, after advisor review):
//! - Scope: local definitions, the timestamp reference and developer field
//!   descriptions reset at every (chained) file header.
//! - A new definition for a local type replaces the previous one.
//! - Timestamp reference: updated by any message with a valid uint32 field 253,
//!   and by every resolved compressed timestamp.
//! - Compressed timestamp: local type = header bits 5–6, offset = bits 0–4.
//!   With reference `last`: if off >= (last & 31) then (last & !31) + off,
//!   else (last & !31) + off + 32. No reference yet → resolved value None.
//! - The endian flag applies only to multi-byte base types.
//! - Invalid values: integers by their FIT invalid pattern; z-types at 0;
//!   floats by the all-0xFF bit pattern (not a NaN test); enum decodes to Uint
//!   with 0xFF invalid; `byte` fields stay Bytes (invalid only if all 0xFF,
//!   no per-element None); a field size of 0 or not a multiple of the base size
//!   gives Bytes, not an error. Strings may hold several NUL-separated strings.
//! - File CRC covers every byte from the file's first header byte up to the
//!   CRC. Header CRC covers header bytes 0..12; stored 0 means "not computed"
//!   (header_crc None, header_crc_ok true). header_size >= 12 is accepted and
//!   extra header bytes skipped; < 12 or no ".FIT" is NotFit.
//! - Interpretation (names, scale/offset, units, enums) is a separate, optional
//!   layer (`profile`) so a reparse with a better profile changes names, never
//!   the stored raw values.

mod decode;
pub mod profile;

pub use decode::decode;

/// A decoded FIT file. Chained files (several FIT files back to back in one
/// byte stream) are returned as several `FitFile`s by `decode`.
#[derive(Debug, Clone, PartialEq)]
pub struct FitFile {
    pub header: Header,
    /// Byte offset of this file's header within the whole input.
    pub offset: u64,
    /// Bytes this file occupies in the input (header + data + CRC as present).
    pub len: u64,
    /// False when the input ended or an error occurred inside this file; the
    /// messages decoded before that point are still returned.
    pub complete: bool,
    /// Data messages in file order (definition messages are consumed, not returned).
    pub messages: Vec<Message>,
    /// Developer field descriptions seen in this file (`field_description`, global 206).
    pub developer_fields: Vec<DeveloperFieldDescription>,
    pub crc: Crc,
}

/// File CRC check result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crc {
    Ok,
    Mismatch {
        stored: u16,
        computed: u16,
    },
    /// The two CRC bytes are missing (input ended): the file is truncated.
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub header_size: u8,
    pub protocol_version: u8,
    pub profile_version: u16,
    pub data_size: u32,
    /// Header CRC when header_size is 14 and the CRC is non-zero.
    pub header_crc: Option<u16>,
    pub header_crc_ok: bool,
}

/// One data message.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// Global message number (e.g. 20 = record). Unknown numbers are kept as-is.
    pub global: u16,
    pub local: u8,
    /// Ordinal of this data message within its file (0-based). With the file's
    /// chain index this is the stable database key: (sha256, chain_idx, index).
    pub index: u32,
    /// Byte offset of the record header from the start of the whole input.
    pub offset: u64,
    /// Raw 5-bit time offset from a compressed-timestamp header (None for normal headers).
    pub time_offset: Option<u8>,
    /// Resolved compressed timestamp (see state rules); None for normal headers
    /// or when no reference timestamp was seen yet.
    pub compressed_timestamp: Option<u32>,
    pub fields: Vec<Field>,
    pub developer: Vec<DeveloperField>,
}

/// FIT base types (low 5 bits of the base type byte; endian bit is separate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BaseType {
    Enum,
    Sint8,
    Uint8,
    Sint16,
    Uint16,
    Sint32,
    Uint32,
    String,
    Float32,
    Float64,
    Uint8z,
    Uint16z,
    Uint32z,
    Byte,
    Sint64,
    Uint64,
    Uint64z,
    /// Base type id (low 5 bits, masked) not in the FIT spec; value kept as
    /// Bytes. The unmasked byte is in `Field::base_type_raw`.
    Unknown(u8),
}

/// A decoded value. Arrays (field size a multiple of the base type size) give
/// several elements. Elements equal to the base type's invalid value are `None`.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Uint(Vec<Option<u64>>),
    Sint(Vec<Option<i64>>),
    Float(Vec<Option<f64>>),
    /// UTF-8 (lossy). A string field may hold several NUL-separated strings;
    /// empty strings are omitted, so an all-NUL field is an empty Vec.
    String(Vec<String>),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub num: u8,
    pub base_type: BaseType,
    /// Base type byte exactly as in the definition.
    pub base_type_raw: u8,
    pub big_endian: bool,
    /// Bytes exactly as stored in the file.
    pub raw: Vec<u8>,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeveloperField {
    pub developer_data_index: u8,
    pub num: u8,
    /// Raw bytes as stored; interpret with the matching DeveloperFieldDescription.
    pub raw: Vec<u8>,
    /// Decoded using the description's base type when one was seen, else Bytes.
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeveloperFieldDescription {
    pub developer_data_index: u8,
    pub field_definition_number: u8,
    pub base_type: BaseType,
    pub name: Option<String>,
    pub units: Option<String>,
    pub scale: Option<f64>,
    pub offset: Option<f64>,
    pub native_mesg_num: Option<u16>,
    pub native_field_num: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FitError {
    /// Not a FIT file at `offset` (header size < 12 or missing ".FIT").
    NotFit { offset: u64 },
    /// Bytes after the last complete file that do not start a FIT header.
    TrailingData { offset: u64, len: u64 },
    /// Data ended inside a record; `messages` decoded so far are in the partial result.
    Truncated { offset: u64 },
    /// A data record referenced a local message type with no definition.
    UndefinedLocal { offset: u64, local: u8 },
    /// A record crosses the file's declared data end although the input
    /// continues (bad data_size or corrupt record); decoding resumes after the
    /// file's CRC.
    Malformed { offset: u64 },
}

/// Result of decoding a byte stream: every file found (the last one possibly
/// `complete: false`), plus the first problem that stopped decoding.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    pub files: Vec<FitFile>,
    pub error: Option<FitError>,
}

impl Message {
    /// The message's timestamp: field 253 (uint32, valid) if present, else the
    /// resolved compressed-header timestamp. One definition shared by parsers
    /// and the database column. No field 253 is synthesised into `fields`.
    pub fn timestamp(&self) -> Option<u32> {
        self.fields
            .iter()
            .find(|f| f.num == 253 && f.base_type == BaseType::Uint32)
            .and_then(|f| match &f.value {
                Value::Uint(v) if v.len() == 1 => v[0].and_then(|t| u32::try_from(t).ok()),
                _ => None,
            })
            .or(self.compressed_timestamp)
    }
}
