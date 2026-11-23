//! Nimrag's own FIT field map (stage 5a, `cfg(fit_map)`): names and scales of what
//! Nimrag reads, resolved from small hand-built files, and the writers that
//! must answer "unavailable" because the map has none of their messages.
#![cfg(fit_map)]
use nimrag_core::derive::test_support::{FitBuilder, Fv};
use nimrag_core::derive::{derive, FIT_EPOCH_UNIX};
use nimrag_core::fit::interpret::{interpret, Interpreted};
use nimrag_core::fit::{decode, profile, weather, workout};

const T: u32 = 1_000_000_000; // FIT seconds, UTC-valid

fn interpreted(b: FitBuilder) -> Vec<Interpreted> {
    let d = decode(&b.build());
    assert_eq!(d.files.len(), 1, "{:?}", d.error);
    interpret(&d.files[0])
}

#[test]
fn the_map_is_available_but_is_not_a_garmin_profile() {
    assert!(profile::available());
    assert!(!profile::garmin_profile());
    assert_eq!(profile::profile_version(), None);
    assert_eq!(profile::message_name(20), Some("record"));
    assert_eq!(profile::message_number("sleep_level"), Some(275));
    // Only what Nimrag reads: nothing else is named.
    assert!(profile::field(20, 250).is_none());
    assert!(profile::message_name(0xFFFE).is_none());
}

#[test]
fn sleep_level_enum_names() {
    for (raw, name) in [(1u8, "awake"), (2, "light"), (3, "deep"), (4, "rem")] {
        let i = interpreted(FitBuilder::new().message(275, &[(253, Fv::U32(T)), (0, Fv::U8(raw))]));
        assert_eq!(i[0].message_name, Some("sleep_level"));
        assert_eq!(i[0].get("sleep_level").unwrap().enum_label, Some(name));
    }
    // Value 0 is not in the map: the number survives, the name is absent.
    let d = derive(
        &decode(
            &FitBuilder::new()
                .message(275, &[(253, Fv::U32(T)), (0, Fv::U8(0))])
                .build(),
        )
        .files[0],
    )
    .unwrap();
    assert_eq!(d.sleep_levels.len(), 1);
    assert_eq!(d.sleep_levels[0].level, 0);
    assert_eq!(d.sleep_levels[0].level_name, None);
}

#[test]
fn hrv_value_scale_is_128() {
    let d = derive(
        &decode(
            &FitBuilder::new()
                .message(371, &[(253, Fv::U32(T)), (0, Fv::U16(34 * 128 + 64))])
                .build(),
        )
        .files[0],
    )
    .unwrap();
    assert_eq!(d.hrv_values.len(), 1);
    assert_eq!(d.hrv_values[0].ts, i64::from(T) + FIT_EPOCH_UNIX);
    assert_eq!(d.hrv_values[0].rmssd_ms, 34.5);
}

#[test]
fn monitoring_cycles_are_steps_only_for_running_and_walking() {
    let rows = |activity_type: u8| {
        derive(
            &decode(
                &FitBuilder::new()
                    .message(
                        55,
                        &[
                            (253, Fv::U32(T)),
                            (5, Fv::U8(activity_type)),
                            (3, Fv::U32(120)),
                            (2, Fv::U32(5000)),
                            (4, Fv::U32(3000)),
                        ],
                    )
                    .build(),
            )
            .files[0],
        )
        .unwrap()
    };
    let walking = rows(6);
    assert_eq!(walking.mon_activity[0].steps, Some(120.0));
    assert_eq!(walking.mon_activity[0].distance_m, Some(50.0));
    assert_eq!(walking.mon_activity[0].active_time_s, Some(3.0));
    assert_eq!(walking.skipped, 0);
    assert_eq!(rows(1).mon_activity[0].steps, Some(120.0));
    // Another type: the counter is `cycles` (not stored, counted as skipped).
    let other = rows(0);
    assert_eq!(other.mon_activity[0].steps, None);
    assert_eq!(other.skipped, 1);
}

#[test]
fn monitoring_activity_type_and_intensity_unpack_from_field_24() {
    // walking (6) in the low 5 bits, intensity 3 in the high 3 bits
    let packed = 6 | (3 << 5);
    let i = interpreted(FitBuilder::new().message(
        55,
        &[(253, Fv::U32(T)), (24, Fv::U8(packed)), (3, Fv::U32(100))],
    ));
    assert_eq!(i[0].number("activity_type"), Some(6.0));
    assert_eq!(i[0].number("intensity"), Some(3.0));
    // the activity type selects the `steps` subfield of field 3
    assert_eq!(i[0].number("steps"), Some(100.0));
    assert_eq!(i[0].number("cycles"), None);
}

#[test]
fn monitoring_timestamp_16_extends_the_last_full_timestamp() {
    let base = (T & !0xFFFF) | 0xFFF0;
    let d = derive(
        &decode(
            &FitBuilder::new()
                .message(55, &[(253, Fv::U32(base)), (27, Fv::U8(50))])
                .message(55, &[(26, Fv::U16(0x0005)), (27, Fv::U8(52))])
                .build(),
        )
        .files[0],
    )
    .unwrap();
    let ts: Vec<i64> = d.mon_hr.iter().map(|r| r.ts - FIT_EPOCH_UNIX).collect();
    assert_eq!(ts, vec![i64::from(base), i64::from(base) + 0x15]);
}

#[test]
fn session_and_record_names_and_scales() {
    let i = interpreted(
        FitBuilder::new()
            .message(
                18,
                &[
                    (253, Fv::U32(T)),
                    (5, Fv::U8(2)),
                    (6, Fv::U8(0)),
                    (8, Fv::U32(499_323)),
                    (9, Fv::U32(153_863)),
                    (24, Fv::U8(40)),
                    (124, Fv::U32(3_081)),
                    (168, Fv::S32(300_171)),
                ],
            )
            .message(
                20,
                &[
                    (253, Fv::U32(T)),
                    (3, Fv::U8(116)),
                    (5, Fv::U32(153_863)),
                    (73, Fv::U32(5_318)),
                    (78, Fv::U32(2_581)),
                ],
            ),
    );
    assert_eq!(i[0].message_name, Some("session"));
    assert_eq!(i[0].get("sport").unwrap().enum_label, Some("cycling"));
    assert_eq!(i[0].get("sub_sport").unwrap().enum_label, Some("generic"));
    assert_eq!(i[0].number("total_timer_time"), Some(499.323));
    assert_eq!(i[0].number("total_distance"), Some(1538.63));
    assert_eq!(i[0].number("total_training_effect"), Some(4.0));
    assert_eq!(i[0].number("enhanced_avg_speed"), Some(3.081));
    assert_eq!(
        i[0].number("training_load_peak"),
        Some(300_171.0 / 65_536.0)
    );
    assert_eq!(i[1].message_name, Some("record"));
    assert_eq!(i[1].number("heart_rate"), Some(116.0));
    assert_eq!(i[1].number("distance"), Some(1538.63));
    assert_eq!(i[1].number("enhanced_speed"), Some(5.318));
    // (raw / 5) - 500
    assert!((i[1].number("enhanced_altitude").unwrap() - (2581.0 / 5.0 - 500.0)).abs() < 1e-9);
}

#[test]
fn stress_respiration_and_heart_rate_scales() {
    let i = interpreted(
        FitBuilder::new()
            .message(297, &[(253, Fv::U32(T)), (0, Fv::S16(1305))])
            .message(227, &[(0, Fv::S16(-2)), (1, Fv::U32(T)), (3, Fv::S8(18))]),
    );
    assert_eq!(i[0].number("respiration_rate"), Some(13.05));
    assert_eq!(i[1].number("stress_level_value"), Some(-2.0));
    assert_eq!(i[1].number("body_battery"), Some(18.0));
}

#[test]
fn the_writers_report_unavailable_never_wrong_bytes() {
    assert!(!workout::available());
    assert!(!weather::available());
    assert!(!profile::has_message("workout"));
    assert!(!profile::has_message("workout_step"));
    assert!(!profile::has_message("weather_conditions"));
}
