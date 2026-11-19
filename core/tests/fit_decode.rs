//! Integration tests for the FIT decoder (public API only).
use nimrag_core::crc;
use nimrag_core::fit::{
    decode, BaseType, Crc, Decoded, Field, FitError, FitFile, Header, Message, Value,
};

// ---- tiny FIT builder -------------------------------------------------------

/// Definition record. `fields` = (num, size, base type byte); `dev` = (num, size, dev index).
fn def(
    local: u8,
    big: bool,
    global: u16,
    fields: &[(u8, u8, u8)],
    dev: &[(u8, u8, u8)],
) -> Vec<u8> {
    let mut v = vec![
        0x40 | local | if dev.is_empty() { 0 } else { 0x20 },
        0,
        big as u8,
    ];
    v.extend(if big {
        global.to_be_bytes()
    } else {
        global.to_le_bytes()
    });
    v.push(fields.len() as u8);
    for &(n, s, t) in fields {
        v.extend([n, s, t]);
    }
    if !dev.is_empty() {
        v.push(dev.len() as u8);
        for &(n, s, i) in dev {
            v.extend([n, s, i]);
        }
    }
    v
}

fn data(local: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![local];
    v.extend(payload);
    v
}

fn compressed(local: u8, offset: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![0x80 | (local << 5) | offset];
    v.extend(payload);
    v
}

fn file(body: &[u8], header14: bool) -> Vec<u8> {
    let mut v = vec![if header14 { 14 } else { 12 }, 0x20];
    v.extend(2132u16.to_le_bytes());
    v.extend((body.len() as u32).to_le_bytes());
    v.extend(b".FIT");
    if header14 {
        let c = crc::checksum(&v);
        v.extend(c.to_le_bytes());
    }
    v.extend(body);
    let c = crc::checksum(&v);
    v.extend(c.to_le_bytes());
    v
}

fn one(d: &Decoded) -> &FitFile {
    assert_eq!(d.error, None);
    assert_eq!(d.files.len(), 1);
    &d.files[0]
}

fn sample_body() -> Vec<u8> {
    let mut b = def(0, false, 0, &[(0, 1, 0), (4, 4, 0x86)], &[]);
    b.extend(data(0, &[4, 0x78, 0x56, 0x34, 0x12]));
    b.extend(def(1, false, 20, &[(253, 4, 0x86), (3, 1, 2)], &[]));
    b.extend(data(1, &[1, 0, 0, 0, 77]));
    b
}

// ---- 1. minimal files / headers / CRC --------------------------------------

#[test]
fn minimal_file_little_endian() {
    let bytes = file(&sample_body(), true);
    let d = decode(&bytes);
    let f = one(&d);
    assert_eq!(f.messages.len(), 2);
    assert_eq!(f.messages[0].global, 0);
    assert_eq!(f.messages[0].fields[0].value, Value::Uint(vec![Some(4)]));
    assert_eq!(
        f.messages[0].fields[1].value,
        Value::Uint(vec![Some(0x12345678)])
    );
    assert_eq!(f.messages[0].fields[1].base_type, BaseType::Uint32);
    assert_eq!(f.messages[0].fields[1].raw, vec![0x78, 0x56, 0x34, 0x12]);
    assert_eq!(f.messages[1].global, 20);
    assert_eq!(f.messages[1].fields[1].value, Value::Uint(vec![Some(77)]));
    assert_eq!(f.header.header_size, 14);
    assert_eq!(f.header.profile_version, 2132);
    assert!(f.header.header_crc_ok && f.header.header_crc.is_some());
    assert_eq!(f.crc, Crc::Ok);
}

#[test]
fn message_offset_is_record_header_position() {
    let bytes = file(&sample_body(), true);
    let d = decode(&bytes);
    let f = one(&d);
    assert_eq!(f.messages[0].index, 0);
    assert_eq!(f.messages[1].index, 1);
    let first = 14 + def(0, false, 0, &[(0, 1, 0), (4, 4, 0x86)], &[]).len();
    assert_eq!(f.messages[0].offset, first as u64);
    assert_eq!(bytes[first], 0);
}

#[test]
fn header_12_variant() {
    let d = decode(&file(&sample_body(), false));
    let f = one(&d);
    assert_eq!(f.header.header_size, 12);
    assert_eq!(f.header.header_crc, None);
    assert_eq!(f.crc, Crc::Ok);
    assert_eq!(f.messages.len(), 2);
}

#[test]
fn bad_file_crc_is_reported_but_messages_kept() {
    let mut bytes = file(&sample_body(), true);
    let n = bytes.len();
    bytes[n - 1] ^= 0xFF;
    let d = decode(&bytes);
    let f = one(&d);
    assert!(matches!(f.crc, Crc::Mismatch { .. }));
    assert!(f.complete);
    assert_eq!(f.messages.len(), 2);
}

#[test]
fn bad_header_crc_is_reported() {
    let mut bytes = file(&sample_body(), true);
    bytes[12] ^= 0x01;
    let d = decode(&bytes);
    let f = one(&d);
    assert!(!f.header.header_crc_ok);
    assert!(f.header.header_crc.is_some());
}

#[test]
fn zero_header_crc_is_not_present() {
    let mut bytes = file(&sample_body(), true);
    bytes[12] = 0;
    bytes[13] = 0;
    // file CRC now wrong, but header CRC is "absent"
    let d = decode(&bytes);
    let f = one(&d);
    assert_eq!(f.header.header_crc, None);
}

#[test]
fn missing_trailing_crc_is_truncated() {
    let mut bytes = file(&sample_body(), true);
    bytes.truncate(bytes.len() - 2);
    let d = decode(&bytes);
    assert!(matches!(d.error, Some(FitError::Truncated { .. })));
    let f = &d.files[0];
    assert_eq!(f.crc, Crc::Missing);
    assert!(!f.complete);
    assert_eq!(f.messages.len(), 2);
}

// ---- 2. endianness, arrays, invalid, strings, bytes --------------------------

#[test]
fn big_endian_definition() {
    let mut b = def(2, true, 0x1234, &[(1, 2, 0x84), (2, 4, 0x86)], &[]);
    b.extend(data(2, &[0x01, 0x02, 0, 0, 1, 0]));
    let d = decode(&file(&b, true));
    let f = one(&d);
    assert_eq!(f.messages[0].global, 0x1234);
    assert!(f.messages[0].fields[0].big_endian);
    assert_eq!(
        f.messages[0].fields[0].value,
        Value::Uint(vec![Some(0x0102)])
    );
    assert_eq!(f.messages[0].fields[1].value, Value::Uint(vec![Some(256)]));
}

#[test]
fn arrays_and_invalid_values() {
    // uint16 x3 with middle invalid; sint8 x2 with invalid 0x7F; float32 invalid.
    let mut b = def(0, false, 5, &[(0, 6, 0x84), (1, 2, 1), (2, 4, 8)], &[]);
    let mut p = vec![1, 0, 0xFF, 0xFF, 3, 0, 0xFE, 0x7F];
    p.extend(1.5f32.to_le_bytes());
    b.extend(data(0, &p));
    b.extend(data(0, &[0xFF; 12]));
    let d = decode(&file(&b, true));
    let f = one(&d);
    let m = &f.messages[0];
    assert_eq!(m.fields[0].value, Value::Uint(vec![Some(1), None, Some(3)]));
    assert_eq!(m.fields[1].value, Value::Sint(vec![Some(-2), None]));
    assert_eq!(m.fields[2].value, Value::Float(vec![Some(1.5)]));
    let m2 = &f.messages[1];
    assert_eq!(m2.fields[0].value, Value::Uint(vec![None, None, None]));
    assert_eq!(m2.fields[2].value, Value::Float(vec![None]));
}

#[test]
fn zero_invalid_types_and_sint32() {
    let mut b = def(
        0,
        false,
        5,
        &[(0, 1, 10), (1, 2, 0x8B), (2, 4, 5), (3, 8, 0x8F)],
        &[],
    );
    let mut p = vec![0, 0, 0];
    p.extend(0x7FFF_FFFFu32.to_le_bytes());
    p.extend(u64::MAX.to_le_bytes());
    b.extend(data(0, &p));
    let d = decode(&file(&b, true));
    let m = &one(&d).messages[0];
    assert_eq!(m.fields[0].value, Value::Uint(vec![None]));
    assert_eq!(m.fields[1].value, Value::Uint(vec![None]));
    assert_eq!(m.fields[2].value, Value::Sint(vec![None]));
    assert_eq!(m.fields[3].value, Value::Uint(vec![None]));
}

#[test]
fn string_trimmed_at_nul_and_empty_is_none() {
    let mut b = def(0, false, 5, &[(0, 6, 7), (1, 3, 7)], &[]);
    b.extend(data(0, &[b'a', b'b', 0, b'x', b'y', 0, 0, 0, 0]));
    let d = decode(&file(&b, true));
    let m = &one(&d).messages[0];
    assert_eq!(
        m.fields[0].value,
        Value::String(vec!["ab".to_string(), "xy".to_string()])
    );
    assert_eq!(m.fields[0].raw.len(), 6);
    assert_eq!(m.fields[1].value, Value::String(vec![]));
}

#[test]
fn odd_size_is_bytes() {
    let mut b = def(0, false, 5, &[(0, 3, 0x84), (1, 3, 13)], &[]);
    b.extend(data(0, &[1, 2, 3, 0xFF, 0xFF, 0xFF]));
    let d = decode(&file(&b, true));
    let m = &one(&d).messages[0];
    assert_eq!(m.fields[0].value, Value::Bytes(vec![1, 2, 3]));
    assert_eq!(m.fields[1].base_type, BaseType::Byte);
    assert_eq!(m.fields[1].value, Value::Bytes(vec![0xFF, 0xFF, 0xFF]));
}

#[test]
fn unknown_base_type_is_bytes() {
    let mut b = def(0, false, 5, &[(0, 2, 0x1F)], &[]);
    b.extend(data(0, &[9, 8]));
    let d = decode(&file(&b, true));
    let m = &one(&d).messages[0];
    assert_eq!(m.fields[0].base_type, BaseType::Unknown(0x1F));
    assert_eq!(m.fields[0].value, Value::Bytes(vec![9, 8]));
}

#[test]
fn sint16_negative_and_uint64() {
    let mut b = def(0, false, 5, &[(0, 2, 0x83), (1, 8, 0x8F)], &[]);
    let mut p = (-5i16).to_le_bytes().to_vec();
    p.extend(7u64.to_le_bytes());
    b.extend(data(0, &p));
    let d = decode(&file(&b, true));
    let m = &one(&d).messages[0];
    assert_eq!(m.fields[0].value, Value::Sint(vec![Some(-5)]));
    assert_eq!(m.fields[1].value, Value::Uint(vec![Some(7)]));
}

// ---- 3. compressed timestamps / redefinition --------------------------------

#[test]
fn compressed_timestamp_with_rollover() {
    let base: u32 = 1000 * 32 + 30; // low 5 bits = 30
    let mut b = def(0, false, 20, &[(253, 4, 0x86), (3, 1, 2)], &[]);
    b.extend(data(0, &[&base.to_le_bytes()[..], &[1]].concat()));
    b.extend(def(1, false, 20, &[(3, 1, 2)], &[]));
    b.extend(compressed(1, 31, &[2])); // no rollover: 1000*32+31
    b.extend(compressed(1, 3, &[3])); // 3 < 31 -> rollover: 1001*32+3
    let d = decode(&file(&b, true));
    let f = one(&d);
    assert_eq!(f.messages[0].compressed_timestamp, None);
    assert_eq!(f.messages[0].time_offset, None);
    assert_eq!(f.messages[1].time_offset, Some(31));
    assert_eq!(f.messages[1].compressed_timestamp, Some(1000 * 32 + 31));
    assert_eq!(f.messages[1].local, 1);
    assert_eq!(f.messages[2].compressed_timestamp, Some(1001 * 32 + 3));
    assert_eq!(f.messages[2].fields[0].value, Value::Uint(vec![Some(3)]));
}

#[test]
fn compressed_header_without_prior_timestamp_does_not_panic() {
    let mut b = def(1, false, 20, &[(3, 1, 2)], &[]);
    b.extend(compressed(1, 5, &[2]));
    let d = decode(&file(&b, true));
    let f = one(&d);
    assert_eq!(f.messages.len(), 1);
}

#[test]
fn local_type_redefinition_uses_latest() {
    let mut b = def(0, false, 5, &[(0, 1, 2)], &[]);
    b.extend(data(0, &[9]));
    b.extend(def(0, true, 6, &[(0, 2, 0x84)], &[]));
    b.extend(data(0, &[1, 2]));
    let d = decode(&file(&b, true));
    let f = one(&d);
    assert_eq!(f.messages[0].global, 5);
    assert_eq!(f.messages[1].global, 6);
    assert_eq!(
        f.messages[1].fields[0].value,
        Value::Uint(vec![Some(0x0102)])
    );
}

// ---- 4. developer fields, unknown numbers ----------------------------------

#[test]
fn developer_field_described_by_field_description() {
    let mut b = def(
        0,
        false,
        206,
        &[
            (0, 1, 2),
            (1, 1, 2),
            (2, 1, 2),
            (3, 8, 7),
            (6, 1, 2),
            (7, 1, 1),
            (8, 4, 7),
            (14, 2, 0x84),
            (15, 1, 2),
        ],
        &[],
    );
    let mut p = vec![0, 5, 0x84]; // idx 0, def num 5, uint16
    p.extend(b"cadx\0\0\0\0");
    p.extend([10, 0xFE]); // scale 10, offset -2
    p.extend(b"rpm\0");
    p.extend(20u16.to_le_bytes());
    p.push(3);
    b.extend(data(0, &p));
    b.extend(def(1, false, 20, &[(3, 1, 2)], &[(5, 2, 0), (9, 2, 0)]));
    b.extend(data(1, &[60, 0x2C, 0x01, 0xAA, 0xBB]));
    let d = decode(&file(&b, true));
    let f = one(&d);
    assert_eq!(f.developer_fields.len(), 1);
    let df = &f.developer_fields[0];
    assert_eq!(df.developer_data_index, 0);
    assert_eq!(df.field_definition_number, 5);
    assert_eq!(df.base_type, BaseType::Uint16);
    assert_eq!(df.name.as_deref(), Some("cadx"));
    assert_eq!(df.units.as_deref(), Some("rpm"));
    assert_eq!(df.scale, Some(10.0));
    assert_eq!(df.offset, Some(-2.0));
    assert_eq!(df.native_mesg_num, Some(20));
    assert_eq!(df.native_field_num, Some(3));
    let m = &f.messages[1];
    assert_eq!(m.developer.len(), 2);
    assert_eq!(m.developer[0].raw, vec![0x2C, 0x01]);
    assert_eq!(m.developer[0].value, Value::Uint(vec![Some(300)]));
    assert_eq!(m.developer[1].num, 9);
    assert_eq!(m.developer[1].value, Value::Bytes(vec![0xAA, 0xBB]));
}

#[test]
fn unknown_global_and_field_numbers_preserved() {
    let mut b = def(3, false, 0xFF10, &[(200, 2, 0x84), (201, 3, 13)], &[]);
    b.extend(data(3, &[1, 0, 7, 8, 9]));
    b.extend(def(4, false, 60000, &[(1, 1, 2)], &[]));
    b.extend(data(4, &[5]));
    let d = decode(&file(&b, true));
    let f = one(&d);
    assert_eq!(f.messages[0].global, 0xFF10);
    assert_eq!(f.messages[0].local, 3);
    assert_eq!(f.messages[0].fields[0].num, 200);
    assert_eq!(f.messages[0].fields[1].raw, vec![7, 8, 9]);
    assert_eq!(f.messages[1].global, 60000);
}

// ---- 5. chains, truncation, garbage ----------------------------------------

#[test]
fn chained_files() {
    let mut bytes = file(&sample_body(), true);
    let second = file(&sample_body(), false);
    bytes.extend(&second);
    let d = decode(&bytes);
    assert_eq!(d.error, None);
    assert_eq!(d.files.len(), 2);
    assert_eq!(d.files[1].header.header_size, 12);
    assert_eq!(d.files[1].messages.len(), 2);
    // offsets are relative to the whole buffer
    assert!(d.files[1].messages[0].offset >= (bytes.len() - second.len() + 12) as u64);
    assert_eq!(d.files[1].offset, (bytes.len() - second.len()) as u64);
    assert_eq!(d.files[1].len, second.len() as u64);
    assert_eq!(d.files[0].len + d.files[1].len, bytes.len() as u64);
    assert_eq!(d.files[1].messages[0].index, 0);
}

#[test]
fn truncated_input_gives_partial_result() {
    let bytes = file(&sample_body(), true);
    let cut = &bytes[..bytes.len() - 5]; // inside the last data record
    let d = decode(cut);
    assert!(matches!(d.error, Some(FitError::Truncated { .. })));
    assert_eq!(d.files.len(), 1);
    assert!(!d.files[0].complete);
    assert_eq!(d.files[0].crc, Crc::Missing);
    assert_eq!(d.files[0].messages.len(), 1);
}

#[test]
fn garbage_is_not_fit() {
    let d = decode(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]);
    assert_eq!(d.error, Some(FitError::NotFit { offset: 0 }));
    assert!(d.files.is_empty());
    assert_eq!(decode(&[]).error, Some(FitError::NotFit { offset: 0 }));
}

#[test]
fn undefined_local_type() {
    let b = data(5, &[1, 2, 3]);
    let d = decode(&file(&b, true));
    assert!(matches!(
        d.error,
        Some(FitError::UndefinedLocal { local: 5, .. })
    ));
}

/// Structural invariants that must hold for ANY decode result on `bytes`.
fn check_invariants(bytes: &[u8], d: &Decoded) {
    let mut prev_end = 0u64;
    for f in &d.files {
        assert!(f.offset >= prev_end);
        assert!(f.offset + f.len <= bytes.len() as u64);
        prev_end = f.offset + f.len;
        let mut last_offset: Option<u64> = None;
        for (i, m) in f.messages.iter().enumerate() {
            assert_eq!(m.index as usize, i);
            if let Some(l) = last_offset {
                assert!(m.offset > l, "offsets must strictly increase");
            }
            last_offset = Some(m.offset);
            assert!(m.offset >= f.offset + f.header.header_size as u64);
            let mut at = m.offset as usize + 1;
            let raws = m
                .fields
                .iter()
                .map(|x| &x.raw)
                .chain(m.developer.iter().map(|x| &x.raw));
            for raw in raws {
                assert_eq!(
                    &bytes[at..at + raw.len()],
                    &raw[..],
                    "raw must equal input slice"
                );
                at += raw.len();
            }
        }
    }
}

#[test]
fn every_prefix_and_bitflip_does_not_panic() {
    let mut body = sample_body();
    body.extend(def(
        2,
        true,
        206,
        &[(0, 1, 2), (1, 1, 2), (2, 1, 2), (3, 4, 7)],
        &[(0, 2, 0)],
    ));
    body.extend(data(2, &[0, 1, 2, b'a', 0, 0, 0, 1, 2]));
    let bytes = file(&body, true);
    check_invariants(&bytes, &decode(&bytes));
    for n in 0..bytes.len() {
        let d = decode(&bytes[..n]);
        check_invariants(&bytes[..n], &d);
        let clean = d.error.is_none() && d.files.iter().all(|f| f.complete);
        assert!(!clean, "strict prefix of length {n} decoded as clean");
    }
    for i in 0..bytes.len() {
        for bit in 0..8 {
            let mut c = bytes.clone();
            c[i] ^= 1 << bit;
            check_invariants(&c, &decode(&c));
        }
    }
}

// ---- v2 contract ---------------------------------------------------------------

#[test]
fn header_size_16_extra_bytes_skipped() {
    let body = sample_body();
    let mut v = vec![16u8, 0x20];
    v.extend(2132u16.to_le_bytes());
    v.extend((body.len() as u32).to_le_bytes());
    v.extend(b".FIT");
    let hc = crc::checksum(&v);
    v.extend(hc.to_le_bytes());
    v.extend([0xAA, 0xBB]);
    v.extend(&body);
    let c = crc::checksum(&v);
    v.extend(c.to_le_bytes());
    let d = decode(&v);
    let f = one(&d);
    assert_eq!(f.header.header_size, 16);
    assert!(f.header.header_crc_ok && f.header.header_crc.is_some());
    assert_eq!(f.messages.len(), 2);
    assert_eq!(f.crc, Crc::Ok);
    assert!(f.complete);
}

#[test]
fn header_size_below_12_is_not_fit() {
    let mut bytes = file(&sample_body(), false);
    bytes[0] = 11;
    assert_eq!(decode(&bytes).error, Some(FitError::NotFit { offset: 0 }));
}

#[test]
fn trailing_data_after_last_file() {
    let mut bytes = file(&sample_body(), true);
    let n = bytes.len();
    bytes.extend([1, 2, 3, 4, 5]);
    let d = decode(&bytes);
    assert_eq!(
        d.error,
        Some(FitError::TrailingData {
            offset: n as u64,
            len: 5
        })
    );
    assert_eq!(d.files.len(), 1);
    assert!(d.files[0].complete);
}

#[test]
fn partial_file_is_returned_incomplete() {
    let bytes = file(&sample_body(), true);
    let d = decode(&bytes[..bytes.len() - 5]);
    assert_eq!(d.files.len(), 1);
    assert!(!d.files[0].complete);
    assert_eq!(d.files[0].offset, 0);
    assert_eq!(d.files[0].len, (bytes.len() - 5) as u64);
}

#[test]
fn second_file_truncated_keeps_first_complete() {
    let mut bytes = file(&sample_body(), true);
    let n = bytes.len();
    bytes.extend(&file(&sample_body(), true)[..30]);
    let d = decode(&bytes);
    assert_eq!(d.files.len(), 2);
    assert!(d.files[0].complete);
    assert!(!d.files[1].complete);
    assert_eq!(d.files[1].offset, n as u64);
    assert!(matches!(d.error, Some(FitError::Truncated { .. })));
}

#[test]
fn undefined_local_file_is_returned_incomplete() {
    let mut b = def(0, false, 5, &[(0, 1, 2)], &[]);
    b.extend(data(0, &[1]));
    b.extend(data(9, &[1]));
    let d = decode(&file(&b, true));
    assert!(matches!(
        d.error,
        Some(FitError::UndefinedLocal { local: 9, .. })
    ));
    assert_eq!(d.files.len(), 1);
    assert!(!d.files[0].complete);
    assert_eq!(d.files[0].messages.len(), 1);
}

#[test]
fn multi_string_field_splits_on_nul_and_omits_empty() {
    let mut b = def(0, false, 5, &[(0, 9, 7), (1, 2, 7)], &[]);
    b.extend(data(0, &[b'a', 0, b'b', b'c', 0, 0, b'd', 0, 0, 0, 0]));
    let d = decode(&file(&b, true));
    let m = &one(&d).messages[0];
    assert_eq!(
        m.fields[0].value,
        Value::String(vec!["a".into(), "bc".into(), "d".into()])
    );
    assert_eq!(m.fields[1].value, Value::String(vec![]));
}

#[test]
fn chained_file_resets_local_definitions() {
    let mut first = def(0, false, 5, &[(0, 1, 2)], &[]);
    first.extend(data(0, &[1]));
    let second = data(0, &[1]); // uses local 0 without defining it in this file
    let mut bytes = file(&first, true);
    bytes.extend(file(&second, true));
    let d = decode(&bytes);
    assert_eq!(d.files.len(), 2);
    assert!(d.files[0].complete);
    assert!(matches!(
        d.error,
        Some(FitError::UndefinedLocal { local: 0, .. })
    ));
}

#[test]
fn chained_file_resets_timestamp_reference() {
    let mut first = def(0, false, 20, &[(253, 4, 0x86)], &[]);
    first.extend(data(0, &100u32.to_le_bytes()));
    let mut second = def(1, false, 20, &[(3, 1, 2)], &[]);
    second.extend(compressed(1, 7, &[1]));
    let mut bytes = file(&first, true);
    bytes.extend(file(&second, true));
    let d = decode(&bytes);
    assert_eq!(d.error, None);
    assert_eq!(d.files[1].messages[0].compressed_timestamp, None);
    assert_eq!(d.files[1].messages[0].time_offset, Some(7));
}

#[test]
fn chained_file_resets_developer_descriptions() {
    let mut first = def(0, false, 206, &[(0, 1, 2), (1, 1, 2), (2, 1, 2)], &[]);
    first.extend(data(0, &[0, 5, 0x84]));
    let mut second = def(1, false, 20, &[(3, 1, 2)], &[(5, 2, 0)]);
    second.extend(data(1, &[1, 0x2C, 0x01]));
    let mut bytes = file(&first, true);
    bytes.extend(file(&second, true));
    let d = decode(&bytes);
    assert_eq!(d.error, None);
    assert_eq!(d.files[0].developer_fields.len(), 1);
    assert!(d.files[1].developer_fields.is_empty());
    assert_eq!(
        d.files[1].messages[0].developer[0].value,
        Value::Bytes(vec![0x2C, 0x01])
    );
}

#[test]
fn timestamp_reference_updated_by_compressed_and_ignores_invalid() {
    let mut b = def(0, false, 20, &[(253, 4, 0x86)], &[]);
    b.extend(data(0, &(64u32 + 10).to_le_bytes()));
    b.extend(def(1, false, 20, &[(3, 1, 2)], &[]));
    b.extend(compressed(1, 20, &[1])); // 64+20
    b.extend(compressed(1, 5, &[1])); // 5 < 20 -> 96+5
    b.extend(data(0, &[0xFF; 4])); // invalid timestamp: reference stays 101
    b.extend(compressed(1, 6, &[1])); // 6 >= 5 -> 96+6
    let d = decode(&file(&b, true));
    let f = one(&d);
    assert_eq!(f.messages[1].compressed_timestamp, Some(84));
    assert_eq!(f.messages[2].compressed_timestamp, Some(101));
    assert_eq!(f.messages[4].compressed_timestamp, Some(102));
}

#[test]
fn endian_flag_only_for_multibyte_types_and_raw_type_kept() {
    let mut b = def(
        0,
        true,
        5,
        &[(0, 1, 2), (1, 2, 0x84), (2, 3, 7), (3, 1, 0)],
        &[],
    );
    b.extend(data(0, &[1, 0, 2, b'x', 0, 0, 0xFF]));
    let d = decode(&file(&b, true));
    let m = &one(&d).messages[0];
    assert!(!m.fields[0].big_endian);
    assert!(m.fields[1].big_endian);
    assert!(!m.fields[2].big_endian);
    assert_eq!(m.fields[1].base_type_raw, 0x84);
    assert_eq!(m.fields[0].base_type_raw, 2);
    // enum decodes to Uint with 0xFF invalid
    assert_eq!(m.fields[3].value, Value::Uint(vec![None]));
}

#[test]
fn float_invalid_is_all_ones_pattern_not_nan() {
    let mut b = def(0, false, 5, &[(0, 4, 8), (1, 8, 9)], &[]);
    let mut p = 0x7FC0_0000u32.to_le_bytes().to_vec(); // a NaN, but not 0xFFFFFFFF
    p.extend([0xFF; 8]);
    b.extend(data(0, &p));
    let d = decode(&file(&b, true));
    let m = &one(&d).messages[0];
    match &m.fields[0].value {
        Value::Float(v) => assert!(v[0].is_some_and(f64::is_nan)),
        o => panic!("{o:?}"),
    }
    assert_eq!(m.fields[1].value, Value::Float(vec![None]));
}

#[test]
fn malformed_record_in_full_file_keeps_crc_and_chained_files() {
    let mut bad = def(0, false, 5, &[(0, 4, 0x86)], &[]);
    let bad_at = 14 + bad.len();
    bad.extend(data(0, &[1, 2])); // needs 4 payload bytes, only 2 remain in the file
    let first = file(&bad, true);
    let mut bytes = first.clone();
    bytes.extend(file(&sample_body(), true));
    let d = decode(&bytes);
    assert_eq!(
        d.error,
        Some(FitError::Malformed {
            offset: bad_at as u64
        })
    );
    assert_eq!(d.files.len(), 2);
    assert!(!d.files[0].complete);
    assert_eq!(d.files[0].crc, Crc::Ok);
    assert_eq!(d.files[0].len, first.len() as u64);
    assert!(d.files[1].complete);
    assert_eq!(d.files[1].messages.len(), 2);
}

#[test]
fn undefined_local_in_full_file_still_decodes_next_file() {
    let mut bytes = file(&data(7, &[1]), true);
    bytes.extend(file(&sample_body(), true));
    let d = decode(&bytes);
    assert!(matches!(
        d.error,
        Some(FitError::UndefinedLocal { local: 7, .. })
    ));
    assert_eq!(d.files.len(), 2);
    assert_eq!(d.files[0].crc, Crc::Ok);
    assert!(d.files[1].complete);
}

#[test]
fn input_ending_at_or_just_after_data_end_is_truncated() {
    let bytes = file(&sample_body(), true);
    let data_end = bytes.len() - 2;
    for cut in [data_end, data_end + 1] {
        let d = decode(&bytes[..cut]);
        assert_eq!(
            d.error,
            Some(FitError::Truncated {
                offset: data_end as u64
            })
        );
        assert_eq!(d.files.len(), 1);
        assert_eq!(d.files[0].crc, Crc::Missing);
        assert!(!d.files[0].complete);
        assert_eq!(d.files[0].messages.len(), 2);
    }
}

#[test]
fn golden_full_structure() {
    let mut body = def(0, false, 0, &[(0, 1, 0), (3, 4, 0x86)], &[]);
    body.extend(data(0, &[4, 1, 0, 0, 0]));
    let bytes = file(&body, true);
    let hc = crc::checksum(&bytes[..12]);
    let expected = FitFile {
        header: Header {
            header_size: 14,
            protocol_version: 0x20,
            profile_version: 2132,
            data_size: body.len() as u32,
            header_crc: Some(hc),
            header_crc_ok: true,
        },
        offset: 0,
        len: bytes.len() as u64,
        complete: true,
        messages: vec![Message {
            global: 0,
            local: 0,
            index: 0,
            offset: (14 + 12) as u64,
            time_offset: None,
            compressed_timestamp: None,
            fields: vec![
                Field {
                    num: 0,
                    base_type: BaseType::Enum,
                    base_type_raw: 0,
                    big_endian: false,
                    raw: vec![4],
                    value: Value::Uint(vec![Some(4)]),
                },
                Field {
                    num: 3,
                    base_type: BaseType::Uint32,
                    base_type_raw: 0x86,
                    big_endian: false,
                    raw: vec![1, 0, 0, 0],
                    value: Value::Uint(vec![Some(1)]),
                },
            ],
            developer: vec![],
        }],
        developer_fields: vec![],
        crc: Crc::Ok,
    };
    assert_eq!(
        decode(&bytes),
        Decoded {
            files: vec![expected],
            error: None
        }
    );
}

/// Canonical text dump of every message (stable across decodes).
fn dump(d: &Decoded) -> String {
    use std::fmt::Write;
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    let mut out = String::new();
    for (fi, f) in d.files.iter().enumerate() {
        writeln!(
            out,
            "file {fi} off={} len={} complete={} crc={:?}",
            f.offset, f.len, f.complete, f.crc
        )
        .unwrap();
        for m in &f.messages {
            writeln!(
                out,
                "msg g={} i={} off={} ts={:?}",
                m.global,
                m.index,
                m.offset,
                m.timestamp()
            )
            .unwrap();
            for x in &m.fields {
                writeln!(
                    out,
                    "  f{} t={:#04x} raw={} v={:?}",
                    x.num,
                    x.base_type_raw,
                    hex(&x.raw),
                    x.value
                )
                .unwrap();
            }
            for x in &m.developer {
                writeln!(
                    out,
                    "  d{}.{} raw={} v={:?}",
                    x.developer_data_index,
                    x.num,
                    hex(&x.raw),
                    x.value
                )
                .unwrap();
            }
        }
    }
    out
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

fn golden_bytes() -> Vec<u8> {
    let mut b = def(0, false, 0, &[(0, 1, 0), (4, 4, 0x86), (8, 4, 7)], &[]);
    b.extend(data(0, &[4, 0x78, 0x56, 0x34, 0x12, b'h', b'i', 0, 0]));
    b.extend(def(
        1,
        true,
        20,
        &[(253, 4, 0x86), (3, 2, 0x84), (5, 2, 1)],
        &[],
    ));
    b.extend(data(1, &[0, 0, 1, 0, 0, 7, 0x7F, 0x80]));
    b.extend(compressed_def_local_two());
    b.extend(compressed(2, 9, &[0xFF, 0xFF]));
    b.extend(data(0, &[0xFF; 9]));
    file(&b, true)
}

fn compressed_def_local_two() -> Vec<u8> {
    def(2, false, 21, &[(1, 2, 0x84)], &[])
}

#[test]
fn golden_dump_digest_is_pinned() {
    let bytes = golden_bytes();
    let d = decode(&bytes);
    assert_eq!(d.error, None);
    let text = dump(&d);
    assert_eq!(dump(&decode(&bytes)), text, "dump must be deterministic");
    assert_eq!(fnv1a(&text), GOLDEN_DIGEST, "dump changed:\n{text}");
}

const GOLDEN_DIGEST: u64 = 11731214256472791534;

// ---- more edge cases -------------------------------------------------------------

fn ts_def_and_full(ts: u32) -> Vec<u8> {
    let mut b = def(0, false, 20, &[(253, 4, 0x86)], &[]);
    b.extend(data(0, &ts.to_le_bytes()));
    b
}

#[test]
fn compressed_offset_equal_to_reference_low_bits_does_not_roll_over() {
    let mut b = ts_def_and_full(32 * 10 + 7);
    b.extend(def(1, false, 20, &[(3, 1, 2)], &[]));
    b.extend(compressed(1, 7, &[1]));
    let d = decode(&file(&b, true));
    assert_eq!(one(&d).messages[1].compressed_timestamp, Some(32 * 10 + 7));
}

#[test]
fn second_rollover_from_compressed_reference() {
    let mut b = ts_def_and_full(32 * 10 + 30);
    b.extend(def(1, false, 20, &[(3, 1, 2)], &[]));
    b.extend(compressed(1, 3, &[1])); // 11*32+3
    b.extend(compressed(1, 2, &[1])); // 2 < 3 -> 12*32+2
    let d = decode(&file(&b, true));
    let f = one(&d);
    assert_eq!(f.messages[1].compressed_timestamp, Some(11 * 32 + 3));
    assert_eq!(f.messages[2].compressed_timestamp, Some(12 * 32 + 2));
    assert_eq!(f.messages[2].timestamp(), Some(12 * 32 + 2));
}

#[test]
fn field_253_in_other_message_type_updates_reference() {
    let mut b = def(0, false, 21, &[(253, 4, 0x86)], &[]);
    b.extend(data(0, &(32u32 * 5 + 1).to_le_bytes()));
    b.extend(def(1, false, 20, &[(3, 1, 2)], &[]));
    b.extend(compressed(1, 4, &[1]));
    let d = decode(&file(&b, true));
    let f = one(&d);
    assert_eq!(f.messages[0].timestamp(), Some(32 * 5 + 1));
    assert_eq!(f.messages[1].compressed_timestamp, Some(32 * 5 + 4));
}

#[test]
fn big_endian_field_253_updates_reference() {
    let mut b = def(0, true, 20, &[(253, 4, 0x86)], &[]);
    b.extend(data(0, &(32u32 * 9 + 2).to_be_bytes()));
    b.extend(def(1, false, 20, &[(3, 1, 2)], &[]));
    b.extend(compressed(1, 6, &[1]));
    let d = decode(&file(&b, true));
    let f = one(&d);
    assert_eq!(f.messages[0].timestamp(), Some(32 * 9 + 2));
    assert_eq!(f.messages[1].compressed_timestamp, Some(32 * 9 + 6));
}

#[test]
fn big_endian_developer_field() {
    let mut b = def(0, true, 206, &[(0, 1, 2), (1, 1, 2), (2, 1, 2)], &[]);
    b.extend(data(0, &[0, 5, 0x84]));
    b.extend(def(1, true, 20, &[(3, 1, 2)], &[(5, 2, 0)]));
    b.extend(data(1, &[1, 0x01, 0x2C]));
    let d = decode(&file(&b, true));
    assert_eq!(
        one(&d).messages[1].developer[0].value,
        Value::Uint(vec![Some(300)])
    );
}

#[test]
fn field_description_redefined_mid_file_applies_to_later_data_only() {
    let mut b = def(0, false, 206, &[(0, 1, 2), (1, 1, 2), (2, 1, 2)], &[]);
    b.extend(data(0, &[0, 5, 0x02])); // uint8
    b.extend(def(1, false, 20, &[(3, 1, 2)], &[(5, 2, 0)]));
    b.extend(data(1, &[1, 0x2C, 0x01]));
    b.extend(data(0, &[0, 5, 0x84])); // redefined as uint16
    b.extend(data(1, &[1, 0x2C, 0x01]));
    let d = decode(&file(&b, true));
    let f = one(&d);
    assert_eq!(f.developer_fields.len(), 2);
    assert_eq!(
        f.messages[1].developer[0].value,
        Value::Uint(vec![Some(0x2C), Some(0x01)])
    );
    assert_eq!(
        f.messages[3].developer[0].value,
        Value::Uint(vec![Some(300)])
    );
}

#[test]
fn zero_size_field_is_empty_bytes() {
    let mut b = def(0, false, 5, &[(0, 0, 0x84), (1, 1, 2)], &[]);
    b.extend(data(0, &[9]));
    let d = decode(&file(&b, true));
    let m = &one(&d).messages[0];
    assert_eq!(m.fields[0].raw, Vec::<u8>::new());
    assert_eq!(m.fields[0].value, Value::Bytes(vec![]));
    assert_eq!(m.fields[1].value, Value::Uint(vec![Some(9)]));
}

#[test]
fn one_byte_tail_after_complete_file_is_trailing_data() {
    let mut bytes = file(&sample_body(), true);
    let n = bytes.len() as u64;
    bytes.push(0x0E);
    let d = decode(&bytes);
    assert_eq!(d.error, Some(FitError::TrailingData { offset: n, len: 1 }));
    assert!(d.files[0].complete);
}

#[test]
fn truncated_second_chained_header_is_incomplete_file() {
    let mut bytes = file(&sample_body(), true);
    let n = bytes.len();
    bytes.extend(&file(&sample_body(), true)[..13]); // 14-byte header cut inside
    let d = decode(&bytes);
    assert_eq!(d.files.len(), 2);
    assert!(d.files[0].complete);
    assert!(!d.files[1].complete);
    assert_eq!(d.files[1].offset, n as u64);
    assert!(matches!(d.error, Some(FitError::Truncated { .. })));
}

// ---- 6. private real files --------------------------------------------------

#[test]
fn private_real_files_decode_cleanly() {
    let pattern_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../private/captures");
    let Ok(caps) = std::fs::read_dir(pattern_dir) else {
        return;
    };
    let mut counts: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
    for cap in caps.flatten() {
        let Ok(files) = std::fs::read_dir(cap.path().join("files")) else {
            continue;
        };
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if !(name.contains("_128_") && name.ends_with(".bin")) {
                continue;
            }
            let bytes = std::fs::read(f.path()).unwrap();
            let d = decode(&bytes);
            check_invariants(&bytes, &d);
            assert_eq!(dump(&decode(&bytes)), dump(&d), "dump must be stable");
            assert_eq!(d.error, None, "decode error in a private file");
            assert_eq!(d.files.len(), 1, "expected exactly one FIT file");
            let ff = &d.files[0];
            assert!(ff.header.header_crc_ok && ff.complete);
            assert_eq!(ff.crc, Crc::Ok);
            assert_eq!(ff.len, bytes.len() as u64);
            assert!(!ff.messages.is_empty());
            for m in &ff.messages {
                for fl in &m.fields {
                    assert!(!fl.raw.is_empty());
                    if fl.base_type != BaseType::String {
                        if let Some(sz) = base_size(fl.base_type) {
                            if fl.raw.len() % sz == 0 {
                                let n = match &fl.value {
                                    Value::Uint(v) => v.len(),
                                    Value::Sint(v) => v.len(),
                                    Value::Float(v) => v.len(),
                                    Value::Bytes(_) => fl.raw.len() / sz,
                                    Value::String(_) => 0,
                                };
                                assert_eq!(n * sz, fl.raw.len());
                            }
                        }
                    }
                }
            }
            // Each record occupies header + raw field bytes; the next record cannot start earlier.
            for w in ff.messages.windows(2) {
                let used = 1
                    + w[0].fields.iter().map(|f| f.raw.len()).sum::<usize>()
                    + w[0].developer.iter().map(|f| f.raw.len()).sum::<usize>();
                assert!(w[1].offset >= w[0].offset + used as u64);
            }
            // Consistency: all messages of one global/local-definition share field layout sizes.
            let mut layouts: std::collections::HashMap<(u16, Vec<(u8, usize)>), usize> =
                Default::default();
            for m in &ff.messages {
                let key = (
                    m.global,
                    m.fields.iter().map(|f| (f.num, f.raw.len())).collect(),
                );
                *layouts.entry(key).or_default() += 1;
            }
            assert!(!layouts.is_empty());
            let kind = name.split("_128_").nth(1).unwrap_or("?").to_string();
            let kind = kind.split('_').next().unwrap_or("?").to_string();
            counts.entry(kind).or_default().push(ff.messages.len());
        }
    }
    for (k, v) in &counts {
        eprintln!(
            "private 128/{k}: {} files, {} messages",
            v.len(),
            v.iter().sum::<usize>()
        );
    }
}

fn base_size(t: BaseType) -> Option<usize> {
    use BaseType::*;
    Some(match t {
        Enum | Sint8 | Uint8 | Uint8z | Byte => 1,
        Sint16 | Uint16 | Uint16z => 2,
        Sint32 | Uint32 | Uint32z | Float32 => 4,
        Sint64 | Uint64 | Uint64z | Float64 => 8,
        String | Unknown(_) => return None,
    })
}
