//! Tests for the FIT interpretation layer. Name/scale assertions are gated on
//! `cfg(fit_profile)` (generated profile present); the rest holds either way.
use nimrag_core::crc;
use nimrag_core::fit::interpret::{interpret, Interpreted, NamedValue, Source, Val};
use nimrag_core::fit::{decode, profile, FitFile};

fn def(local: u8, global: u16, fields: &[(u8, u8, u8)], dev: &[(u8, u8, u8)]) -> Vec<u8> {
    let mut v = vec![0x40 | local | if dev.is_empty() { 0 } else { 0x20 }, 0, 0];
    v.extend(global.to_le_bytes());
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

fn file(body: &[u8]) -> Vec<u8> {
    let mut v = vec![14, 0x20];
    v.extend(2132u16.to_le_bytes());
    v.extend((body.len() as u32).to_le_bytes());
    v.extend(b".FIT");
    let c = crc::checksum(&v);
    v.extend(c.to_le_bytes());
    v.extend(body);
    let c = crc::checksum(&v);
    v.extend(c.to_le_bytes());
    v
}

fn run(body: &[u8]) -> (FitFile, Vec<Interpreted>) {
    let d = decode(&file(body));
    assert_eq!(d.error, None);
    let f = d.files.into_iter().next().unwrap();
    let i = interpret(&f);
    (f, i)
}

fn get(i: &Interpreted, num: u8, source: Source) -> Vec<&NamedValue> {
    i.values
        .iter()
        .filter(|v| v.num == num && v.source == source)
        .collect()
}

fn number(v: &NamedValue) -> f64 {
    match v.value {
        Some(Val::Number(x)) => x,
        ref o => panic!("not a number: {o:?}"),
    }
}

// ---- profile independent ------------------------------------------------------

#[test]
fn unknown_fields_are_kept_unnamed_with_raw_value_and_invalid_is_none() {
    let mut b = def(0, 0xFF20, &[(200, 2, 0x84), (201, 1, 2)], &[]);
    b.extend(data(0, &[0x34, 0x12, 0xFF]));
    let (_, i) = run(&b);
    assert_eq!(i[0].values.len(), 2);
    assert_eq!(i[0].values[0].name, None);
    assert_eq!(i[0].values[0].value, Some(Val::Number(0x1234 as f64)));
    assert_eq!(i[0].values[1].value, None);
    assert_eq!(i[0].message_name, None);
}

#[test]
fn device_relative_timestamp_is_not_converted() {
    let mut b = def(0, 0xFF20, &[(253, 4, 0x86)], &[]);
    b.extend(data(0, &1000u32.to_le_bytes()));
    b.extend(data(0, &0x3000_0000u32.to_le_bytes()));
    let (_, i) = run(&b);
    assert_eq!(i[0].timestamp, None);
    assert_eq!(i[0].values[0].value, Some(Val::Number(1000.0)));
    assert_eq!(i[1].timestamp, Some(0x3000_0000));
}

#[test]
fn timestamp_16_rollover_against_last_full_timestamp() {
    let base = 0x3000_0000u32 + 0xFFF0;
    let mut b = def(0, 55, &[(253, 4, 0x86)], &[]);
    b.extend(data(0, &base.to_le_bytes()));
    b.extend(def(1, 55, &[(26, 2, 0x84)], &[]));
    b.extend(data(1, &0x0010u16.to_le_bytes())); // rolls over: base + 0x20
    b.extend(data(1, &0x0020u16.to_le_bytes())); // +0x10
    b.extend(data(1, &0xFFFFu16.to_le_bytes())); // invalid: unresolved, reference kept
    b.extend(data(1, &0x0030u16.to_le_bytes())); // +0x10
    let (_, i) = run(&b);
    assert_eq!(i[0].timestamp, Some(base));
    assert_eq!(i[1].timestamp, Some(base + 0x20));
    assert_eq!(i[2].timestamp, Some(base + 0x30));
    assert_eq!(i[3].timestamp, None);
    assert_eq!(i[4].timestamp, Some(base + 0x40));
    let c = get(&i[1], 253, Source::Component);
    assert_eq!(c.len(), 1);
    assert_eq!(number(c[0]), f64::from(base + 0x20));
    assert_eq!(get(&i[1], 26, Source::Direct).len(), 1, "raw field is kept");
}

#[test]
fn timestamp_16_reference_ignores_other_messages_event_ahead_of_sample() {
    // Real-data pattern: an event stamped T+60 precedes a monitoring sample whose
    // timestamp_16 is low16(T). Must resolve to T, not ~18 h in the future.
    let t = 0x3000_1234u32;
    let mut b = def(0, 55, &[(253, 4, 0x86)], &[]);
    b.extend(data(0, &t.to_le_bytes()));
    b.extend(def(1, 21, &[(253, 4, 0x86)], &[]));
    b.extend(data(1, &(t + 60).to_le_bytes()));
    b.extend(def(2, 55, &[(26, 2, 0x84)], &[]));
    b.extend(data(2, &((t & 0xFFFF) as u16).to_le_bytes()));
    b.extend(data(2, &(((t + 60) & 0xFFFF) as u16).to_le_bytes()));
    let (_, i) = run(&b);
    assert_eq!(
        i[1].timestamp,
        Some(t + 60),
        "event keeps its own timestamp"
    );
    assert_eq!(i[2].timestamp, Some(t));
    assert_eq!(i[3].timestamp, Some(t + 60));
}

#[test]
fn timestamp_16_without_reference_is_unresolved() {
    let mut b = def(0, 55, &[(26, 2, 0x84)], &[]);
    b.extend(data(0, &5u16.to_le_bytes()));
    let (_, i) = run(&b);
    assert_eq!(i[0].timestamp, None);
    assert_eq!(get(&i[0], 253, Source::Component)[0].value, None);
}

#[test]
fn timestamp_16_against_device_relative_reference_gives_no_timestamp() {
    let mut b = def(0, 55, &[(253, 4, 0x86)], &[]);
    b.extend(data(0, &100u32.to_le_bytes()));
    b.extend(def(1, 55, &[(26, 2, 0x84)], &[]));
    b.extend(data(1, &120u16.to_le_bytes()));
    let (_, i) = run(&b);
    assert_eq!(i[1].timestamp, None);
}

#[test]
fn compressed_timestamp_header_gives_timestamp() {
    let mut b = def(0, 0xFF20, &[(253, 4, 0x86)], &[]);
    b.extend(data(0, &(0x3000_0000u32 + 30).to_le_bytes()));
    b.extend(def(1, 0xFF20, &[(3, 1, 2)], &[]));
    b.extend([0x80 | (1 << 5) | 3, 9]); // offset 3 < 30: rollover
    let (_, i) = run(&b);
    assert_eq!(i[1].timestamp, Some(0x3000_0000 + 32 + 3));
}

#[test]
fn developer_field_is_included_with_description() {
    let mut b = def(
        0,
        206,
        &[(0, 1, 2), (1, 1, 2), (2, 1, 2), (3, 4, 7), (6, 1, 2)],
        &[],
    );
    b.extend(data(0, &[0, 5, 0x84, b'c', b'a', b'd', 0, 10]));
    b.extend(def(1, 0xFF20, &[(3, 1, 2)], &[(5, 2, 0)]));
    b.extend(data(1, &[1, 0x2C, 0x01])); // 300 / 10
    let (_, i) = run(&b);
    let d = get(&i[1], 5, Source::Developer);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].dev_name.as_deref(), Some("cad"));
    assert_eq!(number(d[0]), 30.0);
}

#[test]
fn no_message_is_dropped_and_order_kept() {
    let mut b = def(0, 0xFF20, &[(1, 1, 2)], &[]);
    for k in 0..5u8 {
        b.extend(data(0, &[k]));
    }
    let (f, i) = run(&b);
    assert_eq!(i.len(), f.messages.len());
    assert!(i.iter().enumerate().all(|(n, x)| x.index as usize == n));
}

#[test]
fn without_profile_values_are_raw_and_unnamed() {
    if profile::available() {
        return;
    }
    let mut b = def(0, 20, &[(2, 2, 0x84), (3, 1, 2)], &[]);
    b.extend(data(0, &[0xB8, 0x0B, 70]));
    let (_, i) = run(&b);
    assert_eq!(i[0].message_name, None);
    assert!(i[0].values.iter().all(|v| v.name.is_none()));
    assert_eq!(i[0].values.len(), 2, "no components without a profile");
    assert_eq!(number(&i[0].values[0]), 3000.0);
}

#[test]
fn timestamp_min_8_is_not_resolved_to_a_time() {
    let mut b = def(0, 55, &[(253, 4, 0x86)], &[]);
    b.extend(data(0, &0x3000_0000u32.to_le_bytes()));
    b.extend(def(1, 55, &[(25, 1, 2)], &[]));
    b.extend(data(1, &[7]));
    let (_, i) = run(&b);
    assert_eq!(i[1].timestamp, None);
    assert!(get(&i[1], 253, Source::Component).is_empty());
    assert_eq!(number(get(&i[1], 25, Source::Direct)[0]), 7.0);
}

#[test]
fn device_relative_field_253_is_never_a_reference() {
    let mut b = def(0, 55, &[(253, 4, 0x86)], &[]);
    b.extend(data(0, &100u32.to_le_bytes()));
    b.extend(def(1, 55, &[(26, 2, 0x84)], &[]));
    b.extend(data(1, &120u16.to_le_bytes()));
    let (_, i) = run(&b);
    assert_eq!(i[1].timestamp, None);
    assert_eq!(get(&i[1], 253, Source::Component)[0].value, None);
    assert!(get(&i[1], 253, Source::Component)
        .iter()
        .all(|c| c.value.is_none()));
}

#[test]
fn lookup_precedence_direct_before_developer_and_first_in_order() {
    let mut b = def(0, 206, &[(0, 1, 2), (1, 1, 2), (2, 1, 2), (3, 4, 7)], &[]);
    b.extend(data(0, &[0, 5, 0x02, b'h', b'r', b'x', 0]));
    b.extend(def(1, 0xFF20, &[(1, 1, 2), (1, 1, 2)], &[(5, 1, 0)]));
    b.extend(data(1, &[10, 20, 30]));
    let (_, i) = run(&b);
    assert_eq!(i[1].number("hrx"), Some(30.0));
    assert_eq!(i[1].get("hrx").map(|v| v.source), Some(Source::Developer));
    assert_eq!(i[1].number("nope"), None);
}

// ---- needs the generated profile -----------------------------------------------

#[cfg(fit_profile)]
#[test]
fn names_scale_offset_and_component_expansion() {
    let mut b = def(0, 20, &[(2, 2, 0x84), (3, 1, 2)], &[]);
    b.extend(data(0, &[0xB8, 0x0B, 70])); // altitude raw 3000
    let (_, i) = run(&b);
    assert_eq!(i[0].message_name, Some("record"));
    let alt = get(&i[0], 2, Source::Direct);
    assert_eq!(alt[0].name, Some("altitude"));
    assert_eq!(number(alt[0]), 3000.0 / 5.0 - 500.0);
    let hr = get(&i[0], 3, Source::Direct);
    assert_eq!(hr[0].name, Some("heart_rate"));
    assert_eq!(hr[0].units.as_deref(), Some("bpm"));
    assert_eq!(number(hr[0]), 70.0);
    let comp = get(&i[0], 78, Source::Component);
    assert_eq!(comp.len(), 1);
    assert_eq!(number(comp[0]), 3000.0 / 5.0 - 500.0);
}

#[cfg(fit_profile)]
#[test]
fn invalid_source_skips_components() {
    let mut b = def(0, 20, &[(2, 2, 0x84)], &[]);
    b.extend(data(0, &[0xFF, 0xFF]));
    let (_, i) = run(&b);
    assert_eq!(i[0].values.len(), 1);
    assert_eq!(i[0].values[0].value, None);
}

#[cfg(fit_profile)]
#[test]
fn subfield_chosen_by_reference_field() {
    // session.total_cycles is total_strides when sport is running (1).
    let mut b = def(0, 18, &[(5, 1, 0), (10, 4, 0x86)], &[]);
    b.extend(data(0, &[1, 100, 0, 0, 0]));
    b.extend(data(0, &[0xFF, 100, 0, 0, 0])); // sport invalid: base field
    let (_, i) = run(&b);
    let s = get(&i[0], 10, Source::Subfield);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].name, Some("total_strides"));
    assert_eq!(number(s[0]), 100.0);
    let base = get(&i[1], 10, Source::Direct);
    assert_eq!(base[0].name, Some("total_cycles"));
    assert!(get(&i[1], 10, Source::Subfield).is_empty());
}

#[cfg(fit_profile)]
#[test]
fn component_bit_expansion_with_accumulate() {
    // record.compressed_speed_distance: 12-bit speed (/100), 12-bit distance (/16, accumulated).
    let mut b = def(0, 20, &[(8, 3, 13)], &[]);
    b.extend(data(0, &[0xA0, 0x00, 0xFF])); // speed 0x0A0, distance 0xFF0
    b.extend(data(0, &[0xA0, 0x00, 0x01])); // distance 0x010 -> rolls over to 0x1010
    let (_, i) = run(&b);
    let speed = get(&i[0], 6, Source::Component);
    assert_eq!(number(speed[0]), 160.0 / 100.0);
    let d0 = get(&i[0], 5, Source::Component);
    assert_eq!(number(d0[0]), 0xFF0 as f64 / 16.0);
    let d1 = get(&i[1], 5, Source::Component);
    assert_eq!(number(d1[0]), 0x1010 as f64 / 16.0);
    assert_eq!(
        i[0].values[0].num, 8,
        "packed field itself is still emitted"
    );
}

#[cfg(fit_profile)]
#[test]
fn event_timestamp_12_packed_components_accumulate_from_event_timestamp() {
    // hr message: event_timestamp (field 9, /1024, accumulated) seeds the state;
    // event_timestamp_12 (field 10) packs 12-bit deltas LSB first.
    let pack = |a: u16, b: u16| {
        [
            (a & 0xFF) as u8,
            (a >> 8) as u8 | ((b & 0xF) << 4) as u8,
            (b >> 4) as u8,
        ]
    };
    let mut b = def(0, 132, &[(9, 4, 0x86), (10, 3, 13)], &[]);
    let mut p = 4000u32.to_le_bytes().to_vec();
    p.extend(pack(4050, 100)); // 4050, then 100 wraps: 4096 + 100 = 4196
    b.extend(data(0, &p));
    let (_, i) = run(&b);
    assert_eq!(number(get(&i[0], 9, Source::Direct)[0]), 4000.0 / 1024.0);
    let c = get(&i[0], 9, Source::Component);
    assert_eq!(c.len(), 2);
    assert_eq!(number(c[0]), 4050.0 / 1024.0);
    assert_eq!(number(c[1]), 4196.0 / 1024.0);
}

#[cfg(fit_profile)]
#[test]
fn lookup_prefers_direct_over_component_and_falls_back_to_component() {
    // record.speed (6, /1000) expands into enhanced_speed (73); give both directly too.
    let mut b = def(0, 20, &[(6, 2, 0x84), (73, 4, 0x86)], &[]);
    b.extend(data(0, &[0xE8, 0x03, 0x10, 0x27, 0, 0])); // speed 1000 -> 1.0; enhanced direct 10000 -> 10.0
    b.extend(def(1, 20, &[(6, 2, 0x84)], &[]));
    b.extend(data(1, &[0xE8, 0x03]));
    let (_, i) = run(&b);
    assert_eq!(i[0].number("enhanced_speed"), Some(10.0));
    assert_eq!(
        i[0].get("enhanced_speed").map(|v| v.source),
        Some(Source::Direct)
    );
    assert_eq!(i[1].number("enhanced_speed"), Some(1.0));
    assert_eq!(
        i[1].get("enhanced_speed").map(|v| v.source),
        Some(Source::Component)
    );
}

#[cfg(fit_profile)]
#[test]
fn subfield_reference_may_come_from_a_component() {
    // monitoring.cycles is `steps` for walking (6); the activity type is only
    // present packed inside current_activity_type_intensity (field 24).
    let mut b = def(0, 55, &[(24, 1, 13), (3, 4, 0x86)], &[]);
    b.extend(data(0, &[6, 100, 0, 0, 0]));
    let (_, i) = run(&b);
    let c = get(&i[0], 3, Source::Subfield);
    assert_eq!(c.len(), 1, "{:?}", i[0].values);
    assert_eq!(c[0].name, Some("steps"));
    assert_eq!(i[0].number("steps"), Some(100.0));
}

#[cfg(fit_profile)]
#[test]
fn accumulator_state_is_per_file() {
    let mut b = def(0, 20, &[(8, 3, 13)], &[]);
    b.extend(data(0, &[0xA0, 0x00, 0xFF]));
    let (f, first) = run(&b);
    let again = interpret(&f);
    assert_eq!(first, again);
}

#[cfg(fit_profile)]
#[test]
fn enum_label_for_scalar_enum_field() {
    let mut b = def(0, 0, &[(0, 1, 0)], &[]);
    b.extend(data(0, &[4]));
    let (_, i) = run(&b);
    assert_eq!(i[0].message_name, Some("file_id"));
    assert_eq!(i[0].values[0].enum_label, Some("activity"));
}

// ---- private real files ----------------------------------------------------------

#[test]
fn private_real_files_interpret_without_panic_and_monitoring_time_is_monotonic() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../private/captures");
    let Ok(caps) = std::fs::read_dir(dir) else {
        return;
    };
    let (mut files, mut msgs, mut stamped, mut named) = (0, 0, 0, 0);
    for cap in caps.flatten() {
        let Ok(list) = std::fs::read_dir(cap.path().join("files")) else {
            continue;
        };
        for f in list.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if !(name.contains("_128_") && name.ends_with(".bin")) {
                continue;
            }
            let d = decode(&std::fs::read(f.path()).unwrap());
            for ff in &d.files {
                let out = interpret(ff);
                assert_eq!(out.len(), ff.messages.len());
                files += 1;
                msgs += out.len();
                let mut last = 0u32;
                let max_full = ff.messages.iter().filter_map(|m| m.timestamp()).max();
                for x in &out {
                    named += usize::from(x.message_name.is_some());
                    if let Some(t) = x.timestamp {
                        stamped += 1;
                        if name.contains("_128_32_") {
                            assert!(t >= last, "monitoring timestamps must not go backwards");
                            last = t;
                        }
                        if let Some(m) = max_full {
                            assert!(
                                t <= m + 3600,
                                "interpreted timestamp far beyond any full timestamp"
                            );
                        }
                    }
                }
            }
        }
    }
    eprintln!("private interpret: files={files} messages={msgs} stamped={stamped} named={named}");
}
