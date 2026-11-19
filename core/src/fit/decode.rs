//! FIT binary decoder: bytes in, `Decoded` out. Total (never panics); see `mod.rs`
//! for the contract. Nothing is dropped: unknown messages/fields and developer
//! fields keep their raw bytes.
use super::{
    BaseType, Crc, Decoded, DeveloperField, DeveloperFieldDescription, Field, FitError, FitFile,
    Header, Message, Value,
};
use crate::crc;

const GLOBAL_FIELD_DESCRIPTION: u16 = 206;
const FIELD_TIMESTAMP: u8 = 253;
const FILE_HEADER_MIN: usize = 12;

/// Decode a byte stream containing one or more chained FIT files.
///
/// A file whose extent is fully present but whose records are bad is returned
/// with `complete: false`; decoding resumes after it so chained files survive.
/// `error` is the first problem met.
pub fn decode(bytes: &[u8]) -> Decoded {
    let mut files = Vec::new();
    let mut first_error: Option<FitError> = None;
    let mut pos = 0usize;
    while pos < bytes.len() {
        let Some(header) = parse_header(bytes, pos) else {
            let error = if files.is_empty() {
                FitError::NotFit { offset: pos as u64 }
            } else {
                FitError::TrailingData {
                    offset: pos as u64,
                    len: (bytes.len() - pos) as u64,
                }
            };
            first_error.get_or_insert(error);
            break;
        };
        let (file, next, error) = decode_file(bytes, pos, header);
        let input_ended = file.crc == Crc::Missing;
        files.push(file);
        if let Some(e) = error {
            first_error.get_or_insert(e);
        }
        if input_ended {
            break;
        }
        pos = next;
    }
    if files.is_empty() {
        first_error.get_or_insert(FitError::NotFit { offset: 0 });
    }
    Decoded {
        files,
        error: first_error,
    }
}

/// Parse the file header at `start`; None if it is not a FIT header.
fn parse_header(bytes: &[u8], start: usize) -> Option<Header> {
    let b = bytes.get(start..)?;
    if b.len() < FILE_HEADER_MIN || &b[8..12] != b".FIT" || (b[0] as usize) < FILE_HEADER_MIN {
        return None;
    }
    let header_size = b[0];
    let mut header_crc = None;
    let mut header_crc_ok = true;
    if header_size >= 14 && b.len() >= 14 {
        let stored = u16::from_le_bytes([b[12], b[13]]);
        if stored != 0 {
            header_crc = Some(stored);
            header_crc_ok = crc::checksum(&b[..12]) == stored;
        }
    }
    Some(Header {
        header_size,
        protocol_version: b[1],
        profile_version: u16::from_le_bytes([b[2], b[3]]),
        data_size: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        header_crc,
        header_crc_ok,
    })
}

/// A local message definition.
struct Definition {
    global: u16,
    big_endian: bool,
    /// (field number, size, base type byte)
    fields: Vec<(u8, u8, u8)>,
    /// (field number, size, developer data index)
    dev_fields: Vec<(u8, u8, u8)>,
    /// Payload size of one data record using this definition.
    total_size: usize,
}

/// Decode one file whose header starts at `start`. Returns the file, the offset
/// after it, and the error that stopped decoding (if any).
fn decode_file(bytes: &[u8], start: usize, header: Header) -> (FitFile, usize, Option<FitError>) {
    let data_start = start.saturating_add(header.header_size as usize);
    let declared_end = data_start.saturating_add(header.data_size as usize);
    let limit = declared_end.min(bytes.len());

    let mut file = FitFile {
        header,
        offset: start as u64,
        len: 0,
        complete: false,
        messages: Vec::new(),
        developer_fields: Vec::new(),
        crc: Crc::Missing,
    };
    let mut defs: [Option<Definition>; 16] = Default::default();
    let mut last_timestamp: Option<u32> = None;
    let mut pos = data_start;
    let mut error = None;

    if data_start > bytes.len() {
        error = Some(FitError::Truncated {
            offset: bytes.len() as u64,
        });
    }
    while error.is_none() && pos < limit {
        let index = file.messages.len() as u32;
        match decode_record(
            bytes,
            pos,
            limit,
            &mut defs,
            &mut last_timestamp,
            &file.developer_fields,
            index,
        ) {
            Ok((next, Some(msg))) => {
                if msg.global == GLOBAL_FIELD_DESCRIPTION {
                    if let Some(desc) = describe(&msg) {
                        file.developer_fields.push(desc);
                    }
                }
                file.messages.push(msg);
                pos = next;
            }
            Ok((next, None)) => pos = next,
            Err(FitError::Truncated { offset })
                if declared_end.saturating_add(2) <= bytes.len() =>
            {
                // The input continues past this file, so the record is malformed.
                error = Some(FitError::Malformed { offset });
            }
            Err(e) => error = Some(e),
        }
    }
    if error.is_none() && declared_end > bytes.len() {
        error = Some(FitError::Truncated {
            offset: bytes.len() as u64,
        });
    }

    let mut end = limit;
    if declared_end <= bytes.len() {
        if let Some(t) = bytes.get(declared_end..declared_end + 2) {
            let stored = u16::from_le_bytes([t[0], t[1]]);
            let computed = crc::checksum(&bytes[start..declared_end]);
            file.crc = if stored == computed {
                Crc::Ok
            } else {
                Crc::Mismatch { stored, computed }
            };
            end = declared_end + 2;
        } else if error.is_none() {
            // Input ends exactly at, or one byte after, the data end.
            error = Some(FitError::Truncated {
                offset: declared_end as u64,
            });
        }
    }
    file.len = (end.max(start) - start) as u64;
    file.complete = error.is_none() && file.crc != Crc::Missing;
    (file, end, error)
}

/// Decode one record at `pos`. Returns the next position and the data message
/// (None for definition records).
fn decode_record(
    bytes: &[u8],
    pos: usize,
    limit: usize,
    defs: &mut [Option<Definition>; 16],
    last_timestamp: &mut Option<u32>,
    dev_descs: &[DeveloperFieldDescription],
    index: u32,
) -> Result<(usize, Option<Message>), FitError> {
    let truncated = FitError::Truncated { offset: pos as u64 };
    let h = bytes[pos];
    let mut cursor = pos + 1;

    let (local, compressed_offset) = if h & 0x80 != 0 {
        ((h >> 5) & 0x03, Some(h & 0x1F))
    } else if h & 0x40 != 0 {
        let (def, next) = parse_definition(bytes, cursor, limit, h & 0x20 != 0).ok_or(truncated)?;
        defs[(h & 0x0F) as usize] = Some(def);
        return Ok((next, None));
    } else {
        (h & 0x0F, None)
    };

    let def = defs[local as usize]
        .as_ref()
        .ok_or(FitError::UndefinedLocal {
            offset: pos as u64,
            local,
        })?;
    if cursor + def.total_size > limit {
        return Err(truncated);
    }

    let compressed_timestamp = compressed_offset.and_then(|off| {
        let last = (*last_timestamp)?;
        let mut ts = (last & !0x1F) | off as u32;
        if (off as u32) < (last & 0x1F) {
            ts = ts.wrapping_add(0x20);
        }
        Some(ts)
    });
    if compressed_timestamp.is_some() {
        *last_timestamp = compressed_timestamp;
    }

    let mut fields = Vec::with_capacity(def.fields.len());
    for &(num, size, type_byte) in &def.fields {
        let raw = bytes[cursor..cursor + size as usize].to_vec();
        cursor += size as usize;
        let base_type = base_type_from_id(type_byte & 0x1F);
        let value = decode_value(&raw, base_type, def.big_endian);
        if num == FIELD_TIMESTAMP && base_type == BaseType::Uint32 {
            if let Value::Uint(v) = &value {
                if let [Some(ts)] = v.as_slice() {
                    *last_timestamp = Some(*ts as u32);
                }
            }
        }
        fields.push(Field {
            num,
            base_type,
            base_type_raw: type_byte,
            big_endian: def.big_endian && element_size(base_type).is_some_and(|n| n > 1),
            raw,
            value,
        });
    }

    let mut developer = Vec::with_capacity(def.dev_fields.len());
    for &(num, size, idx) in &def.dev_fields {
        let raw = bytes[cursor..cursor + size as usize].to_vec();
        cursor += size as usize;
        // A developer field seen before its description stays Bytes for good;
        // descriptions are not applied retroactively.
        let described = dev_descs
            .iter()
            .rev()
            .find(|d| d.developer_data_index == idx && d.field_definition_number == num);
        let value = match described {
            Some(d) => decode_value(&raw, d.base_type, def.big_endian),
            None => Value::Bytes(raw.clone()),
        };
        developer.push(DeveloperField {
            developer_data_index: idx,
            num,
            raw,
            value,
        });
    }

    let msg = Message {
        global: def.global,
        local,
        index,
        offset: pos as u64,
        time_offset: compressed_offset,
        compressed_timestamp,
        fields,
        developer,
    };
    Ok((cursor, Some(msg)))
}

/// Parse a definition body starting after the record header. None if truncated.
fn parse_definition(
    bytes: &[u8],
    at: usize,
    limit: usize,
    has_dev: bool,
) -> Option<(Definition, usize)> {
    let body = bytes.get(at..limit)?;
    let fixed = body.get(..5)?;
    let big_endian = fixed[1] == 1;
    let global = if big_endian {
        u16::from_be_bytes([fixed[2], fixed[3]])
    } else {
        u16::from_le_bytes([fixed[2], fixed[3]])
    };
    let n_fields = fixed[4] as usize;
    let mut cur = 5usize;
    let triples = |from: usize, n: usize| -> Option<Vec<(u8, u8, u8)>> {
        let raw = body.get(from..from + 3 * n)?;
        Some(raw.chunks_exact(3).map(|c| (c[0], c[1], c[2])).collect())
    };
    let fields = triples(cur, n_fields)?;
    cur += 3 * n_fields;
    let mut dev_fields = Vec::new();
    if has_dev {
        let n_dev = *body.get(cur)? as usize;
        cur += 1;
        dev_fields = triples(cur, n_dev)?;
        cur += 3 * n_dev;
    }
    let total_size = fields
        .iter()
        .chain(&dev_fields)
        .map(|&(_, size, _)| size as usize)
        .sum();
    Some((
        Definition {
            global,
            big_endian,
            fields,
            dev_fields,
            total_size,
        },
        at + cur,
    ))
}

/// `BaseType::Unknown` holds the masked id (low 5 bits); `Field::base_type_raw` keeps the full byte.
fn base_type_from_id(id: u8) -> BaseType {
    match id {
        0 => BaseType::Enum,
        1 => BaseType::Sint8,
        2 => BaseType::Uint8,
        3 => BaseType::Sint16,
        4 => BaseType::Uint16,
        5 => BaseType::Sint32,
        6 => BaseType::Uint32,
        7 => BaseType::String,
        8 => BaseType::Float32,
        9 => BaseType::Float64,
        10 => BaseType::Uint8z,
        11 => BaseType::Uint16z,
        12 => BaseType::Uint32z,
        13 => BaseType::Byte,
        14 => BaseType::Sint64,
        15 => BaseType::Uint64,
        16 => BaseType::Uint64z,
        other => BaseType::Unknown(other),
    }
}

/// Size in bytes of one element; None for string / unknown.
fn element_size(t: BaseType) -> Option<usize> {
    use BaseType::*;
    match t {
        Enum | Sint8 | Uint8 | Uint8z | Byte => Some(1),
        Sint16 | Uint16 | Uint16z => Some(2),
        Sint32 | Uint32 | Uint32z | Float32 => Some(4),
        Sint64 | Uint64 | Uint64z | Float64 => Some(8),
        String | Unknown(_) => None,
    }
}

/// Read up to 8 bytes as an unsigned integer in the given endianness.
fn read_uint(chunk: &[u8], big_endian: bool) -> u64 {
    let fold = |acc: u64, &b: &u8| (acc << 8) | b as u64;
    if big_endian {
        chunk.iter().fold(0, fold)
    } else {
        chunk.iter().rev().fold(0, fold)
    }
}

fn decode_value(raw: &[u8], t: BaseType, big_endian: bool) -> Value {
    use BaseType::*;
    if t == String {
        let strings = raw
            .split(|&b| b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| std::string::String::from_utf8_lossy(p).into_owned())
            .collect();
        return Value::String(strings);
    }
    let Some(size) = element_size(t) else {
        return Value::Bytes(raw.to_vec());
    };
    if t == Byte || raw.is_empty() || raw.len() % size != 0 {
        return Value::Bytes(raw.to_vec());
    }
    let bits = size * 8;
    let all_ones = if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let chunks = raw.chunks_exact(size).map(|c| read_uint(c, big_endian));
    match t {
        Enum | Uint8 | Uint16 | Uint32 | Uint64 => {
            Value::Uint(chunks.map(|v| (v != all_ones).then_some(v)).collect())
        }
        Uint8z | Uint16z | Uint32z | Uint64z => {
            Value::Uint(chunks.map(|v| (v != 0).then_some(v)).collect())
        }
        Sint8 | Sint16 | Sint32 | Sint64 => {
            let invalid = all_ones >> 1;
            let shift = 64 - bits;
            Value::Sint(
                chunks
                    .map(|v| (v != invalid).then_some(((v << shift) as i64) >> shift))
                    .collect(),
            )
        }
        Float32 => Value::Float(
            chunks
                .map(|v| (v != all_ones).then(|| f32::from_bits(v as u32) as f64))
                .collect(),
        ),
        Float64 => Value::Float(
            chunks
                .map(|v| (v != all_ones).then(|| f64::from_bits(v)))
                .collect(),
        ),
        String | Byte | Unknown(_) => Value::Bytes(raw.to_vec()),
    }
}

// ---- field_description (global 206) -----------------------------------------

fn field_of(msg: &Message, num: u8) -> Option<&Field> {
    msg.fields.iter().find(|f| f.num == num)
}

fn uint_of(msg: &Message, num: u8) -> Option<u64> {
    match &field_of(msg, num)?.value {
        Value::Uint(v) => v.first().copied().flatten(),
        _ => None,
    }
}

fn sint_of(msg: &Message, num: u8) -> Option<i64> {
    match &field_of(msg, num)?.value {
        Value::Sint(v) => v.first().copied().flatten(),
        _ => None,
    }
}

fn string_of(msg: &Message, num: u8) -> Option<String> {
    match &field_of(msg, num)?.value {
        Value::String(s) => s.first().cloned(),
        _ => None,
    }
}

/// Build a description from a field_description message. None when it cannot
/// be keyed (developer_data_index or field_definition_number missing).
fn describe(msg: &Message) -> Option<DeveloperFieldDescription> {
    let developer_data_index = u8::try_from(uint_of(msg, 0)?).ok()?;
    let field_definition_number = u8::try_from(uint_of(msg, 1)?).ok()?;
    let base_type = match uint_of(msg, 2) {
        Some(id) => base_type_from_id((id & 0x1F) as u8),
        None => BaseType::Unknown(0xFF),
    };
    Some(DeveloperFieldDescription {
        developer_data_index,
        field_definition_number,
        base_type,
        name: string_of(msg, 3),
        units: string_of(msg, 8),
        scale: uint_of(msg, 6).map(|v| v as f64),
        offset: sint_of(msg, 7).map(|v| v as f64),
        native_mesg_num: uint_of(msg, 14).and_then(|v| u16::try_from(v).ok()),
        native_field_num: uint_of(msg, 15).and_then(|v| u8::try_from(v).ok()),
    })
}
