//! Activity parser tests: synthetic FIT through decode -> derive (needs the
//! generated Garmin profile; with the empty table `derive` must refuse), plus private replay.
#[cfg(fit_profile)]
use nimrag_core::crc;
use nimrag_core::derive::derive;
// Unused with Nimrag's own map only: the tests that build files need Garmin's profile or none.
#[cfg_attr(fit_map, allow(unused_imports))]
use nimrag_core::derive::test_support::{FitBuilder, Fv};
#[cfg(not(any(fit_profile, fit_map)))]
use nimrag_core::derive::DeriveError;
#[cfg(fit_profile)]
use nimrag_core::derive::{Derived, FIT_EPOCH_UNIX};
use nimrag_core::fit::{decode, profile};

#[cfg(fit_profile)]
const T0: u32 = 0x3000_0000; // a plausible UTC FIT timestamp

#[cfg(fit_profile)]
fn derive_bytes(bytes: &[u8]) -> Derived {
    let d = decode(bytes);
    assert_eq!(d.error, None);
    derive(&d.files[0]).expect("profile available")
}

/// (field number, base type byte, bytes)
#[cfg(fit_profile)]
type RawField = (u8, u8, Vec<u8>);

/// Message with arbitrary raw fields.
#[cfg(fit_profile)]
fn raw_file(msgs: &[(u16, Vec<RawField>)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (global, fields) in msgs {
        body.extend([0x40, 0, 0]);
        body.extend(global.to_le_bytes());
        body.push(fields.len() as u8);
        for (n, t, b) in fields {
            body.extend([*n, b.len() as u8, *t]);
        }
        body.push(0);
        for (_, _, b) in fields {
            body.extend(b);
        }
    }
    let mut v = vec![12, 0x20, 0x34, 0x08];
    v.extend((body.len() as u32).to_le_bytes());
    v.extend(b".FIT");
    v.extend(body);
    let c = crc::checksum(&v);
    v.extend(c.to_le_bytes());
    v
}

#[cfg(not(any(fit_profile, fit_map)))]
#[test]
fn without_profile_derive_refuses() {
    let bytes = FitBuilder::new().message(0, &[(3, Fv::U32(1))]).build();
    let d = decode(&bytes);
    assert_eq!(derive(&d.files[0]), Err(DeriveError::ProfileUnavailable));
    assert!(!profile::available());
}

#[cfg(fit_profile)]
fn activity_file() -> Vec<u8> {
    FitBuilder::new()
        .message(0, &[(3, Fv::U32(12345)), (4, Fv::U32(T0 - 100))])
        .message(
            20,
            &[
                (253, Fv::U32(T0)),
                (0, Fv::S32(1 << 29)),    // 45 degrees
                (1, Fv::S32(-(1 << 28))), // -22.5 degrees
                (3, Fv::U8(120)),
                (6, Fv::U16(1000)),
                (73, Fv::U32(2000)), // enhanced preferred
                (5, Fv::U32(1500)),  // 15.00 m
                (2, Fv::U16(3000)),  // 100 m
                (4, Fv::U8(80)),
                (13, Fv::S8(21)),
            ],
        )
        .message(
            20,
            &[(253, Fv::U32(T0 + 1)), (6, Fv::U16(1500)), (3, Fv::U8(255))],
        )
        .message(20, &[(253, Fv::U32(T0 + 2))]) // no usable value: no row
        .message(20, &[(253, Fv::U32(500)), (3, Fv::U8(100))]) // device-relative
        .message(
            19,
            &[
                (253, Fv::U32(T0 + 10)),
                (2, Fv::U32(T0)),
                (7, Fv::U32(10_000)),
                (9, Fv::U32(5000)),
                (13, Fv::U16(2500)),
            ],
        )
        .message(
            18,
            &[
                (253, Fv::U32(T0 + 20)),
                (2, Fv::U32(T0)),
                (5, Fv::U8(1)),
                (6, Fv::U8(0)),
                (7, Fv::U32(20_000)),
                (8, Fv::U32(19_000)),
                (9, Fv::U32(10_000)),
                (11, Fv::U16(250)),
                (16, Fv::U8(140)),
                (17, Fv::U8(170)),
                (14, Fv::U16(3000)),
                (124, Fv::U32(3500)),
                (22, Fv::U16(12)),
                (24, Fv::U8(35)),
                (137, Fv::U8(12)),
                (168, Fv::S32(2 << 16)),
            ],
        )
        .message(
            34,
            &[
                (253, Fv::U32(T0 + 30)),
                (5, Fv::U32(T0 + 30 + 7200)),
                (0, Fv::U32(19_000)),
                (1, Fv::U16(1)),
            ],
        )
        .build()
}

#[cfg(fit_profile)]
#[test]
fn full_activity_rows() {
    let d = derive_bytes(&activity_file());
    let a = &d.activity;
    let act = a.activity.as_ref().expect("activity row");
    assert_eq!(act.serial, Some(12345));
    assert_eq!(
        act.activity_key.as_deref(),
        Some(format!("12345:{}", T0 as i64 - 100 + FIT_EPOCH_UNIX).as_str())
    );
    assert_eq!(act.ts, Some(T0 as i64 + 30 + FIT_EPOCH_UNIX));
    assert_eq!(act.local_offset_s, Some(7200));
    assert_eq!(act.total_timer_s, Some(19.0));
    assert_eq!(act.num_sessions, Some(1));
    assert_eq!(act.start_ts, Some(T0 as i64 + FIT_EPOCH_UNIX));
    assert_eq!(act.sport.code, Some(1));
    assert_eq!(act.sport.name.as_deref(), Some("running"));

    assert_eq!(a.sessions.len(), 1);
    let s = &a.sessions[0];
    assert_eq!(s.s.elapsed_s, Some(20.0));
    assert_eq!(s.s.timer_s, Some(19.0));
    assert_eq!(s.s.distance_m, Some(100.0));
    assert_eq!(s.s.calories, Some(250.0));
    assert_eq!(s.s.avg_hr, Some(140.0));
    assert_eq!(s.s.avg_speed, Some(3.5), "enhanced_avg_speed preferred");
    assert_eq!(s.s.ascent_m, Some(12.0));
    assert_eq!(s.aerobic_te, Some(3.5));
    assert_eq!(s.anaerobic_te, Some(1.2));
    assert_eq!(s.training_load, Some(2.0));

    assert_eq!(a.laps.len(), 1);
    assert_eq!(a.laps[0].s.distance_m, Some(50.0));
    assert_eq!(a.laps[0].s.avg_speed, Some(2.5));

    assert_eq!(a.records.len(), 2);
    let r = &a.records[0];
    assert_eq!(r.idx, 0);
    assert_eq!(r.ts, T0 as i64 + FIT_EPOCH_UNIX);
    assert_eq!(r.lat, Some(45.0));
    assert_eq!(r.lon, Some(-22.5));
    assert_eq!(r.hr, Some(120.0));
    assert_eq!(r.speed, Some(2.0), "enhanced_speed preferred");
    assert_eq!(r.distance_m, Some(15.0));
    assert_eq!(r.altitude_m, Some(100.0));
    assert_eq!(r.cadence, Some(80.0));
    assert_eq!(r.temperature_c, Some(21.0));
    let r2 = &a.records[1];
    assert_eq!(r2.idx, 1);
    assert_eq!(r2.speed, Some(1.5), "speed component gives enhanced_speed");
    assert_eq!(r2.hr, None, "0xFF heart rate is invalid, not 255");
    assert_eq!(d.skipped, 1, "device-relative record counted");
}

#[cfg(fit_profile)]
#[test]
fn derivation_is_deterministic() {
    let b = activity_file();
    assert_eq!(derive_bytes(&b), derive_bytes(&b));
}

#[cfg(fit_profile)]
#[test]
fn strength_sets_invalid_weight_is_none_and_category_arrays() {
    let le16 = |x: u16| x.to_le_bytes().to_vec();
    let le32 = |x: u32| x.to_le_bytes().to_vec();
    let bytes = raw_file(&[
        (
            225,
            vec![
                (254, 0x86, le32(T0)),
                (0, 0x86, le32(30_000)),
                (3, 0x84, le16(8)),
                (4, 0x84, le16(160)), // 10 kg
                (5, 0x02, vec![0]),
                (6, 0x86, le32(T0 - 30)),
                (7, 0x84, le16(0)),
                (8, 0x84, le16(3)),
                (11, 0x84, le16(2)),
            ],
        ),
        (
            225,
            vec![
                (254, 0x86, le32(T0 + 60)),
                (3, 0x84, le16(5)),
                (4, 0x84, le16(0xFFFF)), // invalid weight
                (5, 0x02, vec![1]),
                (7, 0x84, [le16(28), le16(0xFFFF), le16(5)].concat()), // array, one invalid
                (8, 0x84, [le16(1), le16(2)].concat()),
            ],
        ),
    ]);
    let d = derive_bytes(&bytes);
    let sets = &d.activity.sets;
    assert_eq!(sets.len(), 2);
    assert_eq!(sets[0].weight_kg, Some(10.0));
    assert_eq!(sets[0].reps, Some(8.0));
    assert_eq!(sets[0].duration_s, Some(30.0));
    assert_eq!(sets[0].ts, Some(T0 as i64 + FIT_EPOCH_UNIX));
    assert_eq!(sets[0].start_ts, Some(T0 as i64 - 30 + FIT_EPOCH_UNIX));
    assert_eq!(sets[0].category.code, Some(0));
    // The app-derived profile has no exercise_category names (code kept, name None);
    // the FIT SDK profile names them.
    assert!(matches!(
        sets[0].category.name.as_deref(),
        None | Some("bench_press")
    ));
    assert_eq!(sets[0].wkt_step_index, Some(2));
    assert_eq!(sets[0].set_type.code, Some(0));
    assert_eq!(sets[1].weight_kg, None, "invalid weight is NULL, never 0");
    assert_eq!(sets[1].category.code, Some(28));
    assert_eq!(sets[1].category_all, vec![28, 5]);
    assert_eq!(sets[1].category_subtype, Some(1));
    assert_eq!(sets[1].category_subtype_all, vec![1, 2]);
}

#[cfg(fit_profile)]
#[test]
fn time_in_zone_exercise_title_and_firstbeat() {
    let le32 = |x: u32| x.to_le_bytes().to_vec();
    let bytes = raw_file(&[
        // metrics are kept only for files with activity rows
        (20, vec![(253, 0x86, le32(T0)), (3, 0x02, vec![90])]),
        (
            216,
            vec![
                (253, 0x86, le32(T0)),
                (0, 0x84, 18u16.to_le_bytes().to_vec()),
                (1, 0x84, 0u16.to_le_bytes().to_vec()),
                (
                    2,
                    0x86,
                    [le32(1000), le32(0xFFFF_FFFF), le32(3000)].concat(),
                ),
            ],
        ),
        (
            264,
            vec![
                (254, 0x84, 0u16.to_le_bytes().to_vec()),
                (0, 0x84, 0u16.to_le_bytes().to_vec()),
                (1, 0x84, 3u16.to_le_bytes().to_vec()),
                (2, 0x07, b"Bench\0".to_vec()),
            ],
        ),
        (
            140,
            vec![
                (253, 0x86, le32(T0)),
                (7, 0x85, le32(50 << 16)),
                (4, 0x02, vec![35]),
                (20, 0x02, vec![12]),
                (3, 0x85, le32(3 << 16)),
                (2, 0x85, le32(2 << 16)),
                (43, 0x83, 1440i16.to_le_bytes().to_vec()),
            ],
        ),
    ]);
    let d = derive_bytes(&bytes);
    let z = &d.activity.time_in_zone;
    assert_eq!(z.len(), 1);
    assert_eq!(z[0].ref_mesg, Some(18));
    assert_eq!(z[0].hr_zone_s, vec![Some(1.0), None, Some(3.0)]);
    let t = &d.activity.exercise_titles;
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].category.code, Some(0));
    assert!(matches!(
        t[0].category.name.as_deref(),
        None | Some("bench_press")
    ));
    assert_eq!(t[0].subtype, Some(3));
    assert_eq!(t[0].name.as_deref(), Some("Bench"));
    // Message 140 (Firstbeat results) is only in the app-derived profile, not in the
    // public FIT SDK; without it there are no per-activity metrics.
    if profile::message_name(140).is_none() {
        assert!(d.activity.metrics.is_none());
        return;
    }
    let m = d.activity.metrics.as_ref().expect("metrics");
    assert_eq!(m.maximal_met, Some(50.0));
    assert_eq!(m.aerobic_te, Some(3.5));
    assert_eq!(m.anaerobic_te, Some(1.2));
    assert_eq!(m.training_load_peak, Some(3.0));
    assert_eq!(m.epoc, Some(2.0));
    assert_eq!(m.recovery_time_physical_activity_raw, Some(1440.0));
}

#[cfg(fit_profile)]
#[test]
fn records_without_activity_message_still_give_an_activity_row() {
    let bytes = FitBuilder::new()
        .message(0, &[(3, Fv::U32(8)), (4, Fv::U32(T0))])
        .message(20, &[(253, Fv::U32(T0)), (3, Fv::U8(90))])
        .build();
    let a = derive_bytes(&bytes).activity;
    let act = a.activity.expect("activity row without activity message");
    assert_eq!(act.ts, None);
    assert_eq!(act.num_sessions, None);
    assert_eq!(act.serial, Some(8));
}

#[cfg(fit_profile)]
#[test]
fn unknown_messages_and_empty_files_yield_no_activity_rows() {
    let bytes = FitBuilder::new().message(0xFF10, &[(1, Fv::U8(1))]).build();
    let d = derive_bytes(&bytes);
    assert_eq!(d.activity, Default::default());
}

#[test]
fn private_activity_files_derive_and_report_counts() {
    if !profile::available() {
        return;
    }
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../private/captures");
    let Ok(caps) = std::fs::read_dir(dir) else {
        return;
    };
    for cap in caps.flatten() {
        let Ok(list) = std::fs::read_dir(cap.path().join("files")) else {
            continue;
        };
        for f in list.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if !(name.contains("_128_4_") || name.contains("_128_4.")) || !name.ends_with(".bin") {
                continue;
            }
            let d = decode(&std::fs::read(f.path()).unwrap());
            for ff in &d.files {
                let out = derive(ff).unwrap();
                let a = &out.activity;
                assert_eq!(out.skipped, 0, "unexpected skipped rows");
                eprintln!(
                    "private activity: sessions={} laps={} records={} sets={} titles={} zones={} metrics={} sport={:?}/{:?}",
                    a.sessions.len(),
                    a.laps.len(),
                    a.records.len(),
                    a.sets.len(),
                    a.exercise_titles.len(),
                    a.time_in_zone.len(),
                    a.metrics.is_some(),
                    a.activity.as_ref().and_then(|x| x.sport.name.clone()),
                    a.activity.as_ref().and_then(|x| x.sub_sport.name.clone()),
                );
                assert!(a.activity.is_some() || !a.sessions.is_empty());
            }
        }
    }
}
