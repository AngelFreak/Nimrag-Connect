//! Wellness parsers over synthetic FIT files. Name/scale dependent tests need the
//! locally generated Garmin profile (`cfg(fit_profile)`); without any table (`NIMRAG_FIT_PROFILE=empty`) `derive` refuses.
// Unused with Nimrag's own map only: the tests that use them need Garmin's profile or none.
#[cfg_attr(fit_map, allow(unused_imports))]
use nimrag_core::derive::test_support::{FitBuilder, Fv};
#[allow(unused_imports)]
use nimrag_core::derive::{derive, DeriveError, Derived, FIT_EPOCH_UNIX};
use nimrag_core::fit::decode;

#[cfg_attr(fit_map, allow(dead_code))]
fn run(b: FitBuilder) -> Result<Derived, DeriveError> {
    let d = decode(&b.build());
    assert_eq!(d.files.len(), 1, "{:?}", d.error);
    derive(&d.files[0])
}

#[cfg(not(any(fit_profile, fit_map)))]
#[test]
fn without_profile_derive_is_unavailable() {
    let b = FitBuilder::new().message(55, &[(253, Fv::U32(1_000_000_000)), (27, Fv::U8(60))]);
    assert_eq!(run(b), Err(DeriveError::ProfileUnavailable));
    // New in 2p: battery and multi-night sleep summaries degrade the same way.
    let b = FitBuilder::new()
        .message(104, &[(253, Fv::U32(1_000_000_000)), (2, Fv::U8(87))])
        .message(
            411,
            &[(10, Fv::U32(1_000_000_000)), (8, Fv::U32(1_000_028_800))],
        )
        .message(
            411,
            &[(10, Fv::U32(1_000_086_400)), (8, Fv::U32(1_000_115_200))],
        );
    assert_eq!(run(b), Err(DeriveError::ProfileUnavailable));
}

#[cfg(fit_profile)]
mod with_profile {
    use super::*;
    use nimrag_core::fit::profile;

    const T: u32 = 1_000_000_000; // FIT seconds, UTC-valid
    fn unix(t: u32) -> i64 {
        i64::from(t) + FIT_EPOCH_UNIX
    }

    #[test]
    fn monitoring_heart_rate_and_invalid() {
        let d = run(FitBuilder::new()
            .message(55, &[(253, Fv::U32(T)), (27, Fv::U8(62))])
            .message(55, &[(253, Fv::U32(T + 60)), (27, Fv::U8(0xFF))]))
        .unwrap();
        assert_eq!(d.mon_hr.len(), 1);
        assert_eq!((d.mon_hr[0].ts, d.mon_hr[0].bpm), (unix(T), 62.0));
        assert_eq!(d.skipped, 0);
    }

    #[test]
    fn timestamp_16_behind_an_event_does_not_push_last_ts_into_the_future() {
        let t = T + 0x1234;
        let d = run(FitBuilder::new()
            .message(55, &[(253, Fv::U32(t)), (27, Fv::U8(50))])
            .message(21, &[(253, Fv::U32(t + 60))])
            .message(55, &[(26, Fv::U16((t & 0xFFFF) as u16)), (27, Fv::U8(51))]))
        .unwrap();
        assert_eq!(d.facts.last_ts, Some(unix(t + 60)));
        let ts: Vec<i64> = d.mon_hr.iter().map(|r| r.ts).collect();
        assert_eq!(ts, vec![unix(t), unix(t)]);
    }

    #[test]
    fn timestamp_16_extends_and_rolls_over() {
        let base = (T & !0xFFFF) | 0xFFF0; // low 16 bits just below rollover
        let d = run(FitBuilder::new()
            .message(55, &[(253, Fv::U32(base)), (27, Fv::U8(50))])
            .message(55, &[(26, Fv::U16(0xFFF8)), (27, Fv::U8(51))])
            .message(55, &[(26, Fv::U16(0x0005)), (27, Fv::U8(52))]))
        .unwrap();
        let ts: Vec<i64> = d.mon_hr.iter().map(|r| r.ts).collect();
        assert_eq!(ts, vec![unix(base), unix(base + 8), unix(base + 0x15)]);
    }

    #[test]
    fn timestamp_16_without_reference_is_skipped_and_counted() {
        let d =
            run(FitBuilder::new().message(55, &[(26, Fv::U16(100)), (27, Fv::U8(70))])).unwrap();
        assert!(d.mon_hr.is_empty());
        assert_eq!(d.skipped, 1);
    }

    #[test]
    fn device_relative_timestamp_is_skipped_and_counted() {
        let d = run(FitBuilder::new()
            .message(55, &[(253, Fv::U32(1000)), (27, Fv::U8(60))])
            .message(55, &[(253, Fv::U32(T)), (27, Fv::U8(61))]))
        .unwrap();
        assert_eq!(d.mon_hr.len(), 1);
        assert_eq!(d.mon_hr[0].bpm, 61.0);
        assert_eq!(d.skipped, 1);
    }

    #[test]
    fn monitoring_activity_columns() {
        let d = run(FitBuilder::new().message(
            55,
            &[
                (253, Fv::U32(T)),
                (5, Fv::U8(6)),
                (3, Fv::U32(120)),
                (2, Fv::U32(5000)),
                (19, Fv::U16(7)),
                (4, Fv::U32(3000)),
                (29, Fv::U16(5)),
                (28, Fv::U8(30)),
            ],
        ))
        .unwrap();
        assert_eq!(d.mon_activity.len(), 1);
        let a = &d.mon_activity[0];
        assert_eq!((a.ts, a.activity_type), (unix(T), 6));
        assert_eq!(a.steps, Some(120.0)); // steps subfield: not halved
        assert_eq!(a.distance_m, Some(50.0));
        assert_eq!(a.active_calories, Some(7.0));
        assert_eq!(a.active_time_s, Some(3.0));
        assert_eq!(a.duration_min, Some(5.0));
        assert!(a.intensity.is_some());
    }

    #[test]
    fn activity_values_without_type_are_skipped_not_keyed() {
        let d =
            run(FitBuilder::new().message(55, &[(253, Fv::U32(T)), (2, Fv::U32(5000))])).unwrap();
        assert!(d.mon_activity.is_empty());
        assert_eq!(d.skipped, 1);
    }

    #[test]
    fn message_with_no_usable_value_emits_nothing() {
        let d =
            run(FitBuilder::new().message(55, &[(253, Fv::U32(T)), (27, Fv::U8(0xFF))])).unwrap();
        assert_eq!(
            d,
            Derived {
                facts: d.facts.clone(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn monitoring_extra_fields_35_to_38() {
        assert_eq!(profile::field(55, 35).unwrap().name, "ascent_total");
        assert_eq!(profile::field(55, 36).unwrap().name, "descent_total");
        assert_eq!(
            profile::field(55, 37).unwrap().name,
            "moderate_activity_minutes_total"
        );
        assert_eq!(
            profile::field(55, 38).unwrap().name,
            "vigorous_activity_minutes_total"
        );
        let d = run(FitBuilder::new().message(
            55,
            &[
                (253, Fv::U32(T)),
                (35, Fv::U32(12_500)),
                (36, Fv::U32(3_000)),
                (37, Fv::U16(20)),
                (38, Fv::U16(0xFFFF)),
            ],
        ))
        .unwrap();
        let e = &d.mon_extra[0];
        assert_eq!(e.ascent_total_m, Some(12.5)); // never /3.048
        assert_eq!(e.descent_total_m, Some(3.0));
        assert_eq!(e.moderate_total_min, Some(20.0));
        assert_eq!(e.vigorous_total_min, None);
    }

    #[test]
    fn stress_negative_is_status_code() {
        let d = run(FitBuilder::new()
            .message(
                227,
                &[
                    (1, Fv::U32(T)),
                    (0, Fv::S16(45)),
                    (3, Fv::S8(80)),
                    (4, Fv::S8(1)),
                ],
            )
            .message(
                227,
                &[(1, Fv::U32(T + 180)), (0, Fv::S16(-2)), (3, Fv::S8(0x7F))],
            )
            .message(
                227,
                &[
                    (1, Fv::U32(T + 360)),
                    (0, Fv::S16(0x7FFF)),
                    (3, Fv::S8(0x7F)),
                ],
            )
            .message(227, &[(1, Fv::U32(500)), (0, Fv::S16(10))]))
        .unwrap();
        assert_eq!(d.stress.len(), 2);
        let (a, b) = (&d.stress[0], &d.stress[1]);
        assert_eq!(
            (a.ts, a.stress, a.stress_raw),
            (unix(T), Some(45.0), Some(45.0))
        );
        assert_eq!(
            (a.body_battery, a.body_battery_status),
            (Some(80.0), Some(1))
        );
        assert_eq!(
            (b.stress, b.stress_raw, b.body_battery),
            (None, Some(-2.0), None)
        );
        assert_eq!(d.skipped, 1); // device-relative stress_level_time
    }

    #[test]
    fn respiration_non_positive_keeps_raw() {
        let d = run(FitBuilder::new()
            .message(297, &[(253, Fv::U32(T)), (0, Fv::S16(1550))])
            .message(297, &[(253, Fv::U32(T + 60)), (0, Fv::S16(0))])
            .message(297, &[(253, Fv::U32(T + 120)), (0, Fv::S16(-100))])
            .message(297, &[(253, Fv::U32(T + 180)), (0, Fv::S16(0x7FFF))]))
        .unwrap();
        let v: Vec<_> = d.respiration.iter().map(|r| (r.brpm, r.brpm_raw)).collect();
        assert_eq!(
            v,
            vec![
                (Some(15.5), Some(15.5)),
                (None, Some(0.0)),
                (None, Some(-1.0))
            ]
        );
    }

    #[test]
    fn spo2_and_resting_hr_and_info() {
        let d = run(FitBuilder::new()
            .message(
                269,
                &[
                    (253, Fv::U32(T)),
                    (0, Fv::U8(97)),
                    (1, Fv::U8(80)),
                    (2, Fv::U8(1)),
                ],
            )
            .message(
                269,
                &[(253, Fv::U32(T + 60)), (0, Fv::U8(0xFF)), (1, Fv::U8(0xFF))],
            )
            .message(
                211,
                &[(253, Fv::U32(T)), (0, Fv::U8(52)), (1, Fv::U8(0xFF))],
            )
            .message(
                103,
                &[
                    (253, Fv::U32(T)),
                    (0, Fv::U32(T + 7200)),
                    (5, Fv::U16(1700)),
                ],
            ))
        .unwrap();
        assert_eq!(d.spo2.len(), 1);
        assert_eq!(
            (d.spo2[0].percent, d.spo2[0].confidence, d.spo2[0].mode),
            (Some(97.0), Some(80.0), Some(1))
        );
        assert_eq!(
            (d.resting_hr[0].rhr, d.resting_hr[0].current_day_rhr),
            (Some(52.0), None)
        );
        let i = d.mon_info.unwrap();
        assert_eq!(
            (i.local_offset_s, i.rmr_kcal_day),
            (Some(7200.0), Some(1700.0))
        );
    }

    #[test]
    fn sleep_levels_and_session() {
        let d = run(FitBuilder::new()
            .message(275, &[(253, Fv::U32(T)), (0, Fv::U8(2))])
            .message(275, &[(253, Fv::U32(T + 60)), (0, Fv::U8(0xFF))])
            .message(
                346,
                &[
                    (6, Fv::U8(81)),
                    (7, Fv::U8(0xFF)),
                    (3, Fv::U8(70)),
                    (11, Fv::U8(3)),
                    (15, Fv::U16(2550)),
                ],
            )
            .message(
                411,
                &[(10, Fv::U32(T)), (8, Fv::U32(T + 28_800)), (0, Fv::U8(79))],
            ))
        .unwrap();
        assert_eq!(d.sleep_levels.len(), 1);
        assert_eq!(d.sleep_levels[0].level, 2);
        assert_eq!(
            d.sleep_levels[0].level_name.as_deref(),
            profile::enum_name("sleep_level", 2)
        );
        let s = &d.sleep_sessions[0];
        assert_eq!(
            (s.start_ts, s.end_ts),
            (Some(unix(T)), Some(unix(T + 28_800)))
        );
        assert_eq!(s.overall_score, Some(81.0)); // assessment beats summary
        assert_eq!((s.quality_score, s.deep_score), (None, Some(70.0)));
        assert_eq!((s.awakenings_count, s.avg_stress), (Some(3.0), Some(25.5)));
    }

    #[test]
    fn sleep_events_give_the_times_when_there_is_no_summary() {
        // Like the Instinct 2's sleep file (128/49): event 74 start/stop, levels, assessment.
        let event = |ts: u32, kind: u8| [(253, Fv::U32(ts)), (0, Fv::U8(74)), (1, Fv::U8(kind))];
        let d = run(FitBuilder::new()
            .message(21, &event(T, 0))
            .message(275, &[(253, Fv::U32(T + 600)), (0, Fv::U8(2))])
            .message(
                21,
                &[(253, Fv::U32(T + 900)), (0, Fv::U8(0)), (1, Fv::U8(0))],
            ) // timer, ignored
            .message(21, &event(T + 33_900, 1))
            .message(21, &event(T + 40_000, 1)) // second stop, ignored
            .message(346, &[(6, Fv::U8(78))]))
        .unwrap();
        assert_eq!(d.sleep_sessions.len(), 1);
        let s = &d.sleep_sessions[0];
        assert_eq!(
            (s.start_ts, s.end_ts, s.overall_score),
            (Some(unix(T)), Some(unix(T + 33_900)), Some(78.0))
        );
    }

    #[test]
    fn sleep_session_without_summary_has_no_times() {
        let d = run(FitBuilder::new().message(346, &[(6, Fv::U8(81))])).unwrap();
        assert_eq!(
            (d.sleep_sessions[0].start_ts, d.sleep_sessions[0].end_ts),
            (None, None)
        );
    }

    #[test]
    fn every_sleep_summary_becomes_a_session() {
        let night = |start: u32, score: u8| {
            [
                (10, Fv::U32(start)),
                (8, Fv::U32(start + 28_800)),
                (0, Fv::U8(score)),
            ]
        };
        let d = run(FitBuilder::new()
            .message(411, &night(T, 70))
            .message(411, &night(T + 86_400, 71))
            .message(411, &night(T, 99)) // same start: first wins
            .message(411, &night(T + 172_800, 72))
            .message(346, &[(6, Fv::U8(81))]))
        .unwrap();
        let got: Vec<_> = d
            .sleep_sessions
            .iter()
            .map(|s| (s.start_ts, s.overall_score))
            .collect();
        assert_eq!(
            got,
            vec![
                (Some(unix(T)), Some(70.0)),
                (Some(unix(T + 86_400)), Some(71.0)),
                (Some(unix(T + 172_800)), Some(72.0)),
            ]
        );
        assert_eq!(d.skipped, 1, "assessment not attributed to any night");
    }

    #[test]
    fn battery_capacity_rows() {
        let d = run(FitBuilder::new()
            .message(
                104,
                &[(253, Fv::U32(T)), (2, Fv::U8(87)), (0, Fv::U16(4100))],
            )
            .message(104, &[(253, Fv::U32(T + 60)), (0, Fv::U16(4090))]) // no capacity
            .message(104, &[(253, Fv::U32(100)), (2, Fv::U8(50))])) // device-relative
        .unwrap();
        assert_eq!(d.battery.len(), 1);
        assert_eq!((d.battery[0].ts, d.battery[0].percent), (unix(T), 87.0));
        assert_eq!(d.skipped, 1);
    }

    #[test]
    fn hrv_rows() {
        let d = run(FitBuilder::new()
            .message(371, &[(253, Fv::U32(T)), (0, Fv::U16(6400))])
            .message(371, &[(253, Fv::U32(T + 300)), (0, Fv::U16(0xFFFF))])
            .message(
                370,
                &[
                    (253, Fv::U32(T)),
                    (0, Fv::U16(6400)),
                    (1, Fv::U16(7680)),
                    (6, Fv::U8(2)),
                ],
            ))
        .unwrap();
        assert_eq!(d.hrv_values.len(), 1);
        assert_eq!(d.hrv_values[0].rmssd_ms, 50.0);
        let s = &d.hrv_summary[0];
        assert_eq!(
            (s.weekly_avg, s.last_night_avg, s.status),
            (Some(50.0), Some(60.0), Some(2))
        );
        assert_eq!(s.baseline_low_upper, None);
    }

    #[test]
    fn metrics_rows() {
        let d = run(FitBuilder::new()
            .message(
                378,
                &[
                    (253, Fv::U32(T)),
                    (3, Fv::U16(300)),
                    (4, Fv::U16(280)),
                    (5, Fv::U8(12)),
                    (0, Fv::U8(0xFF)),
                ],
            )
            .message(
                294,
                &[
                    (253, Fv::U32(T)),
                    (0, Fv::S32(65536 * 5)),
                    (3, Fv::U16(100)),
                    (1, Fv::S32(0x7FFF_FFFF)),
                ],
            )
            .message(284, &[(0, Fv::U16(1440)), (1, Fv::U32(T))])
            .message(284, &[(0, Fv::U16(60)), (1, Fv::U32(100))]))
        .unwrap();
        let l = &d.training_load[0];
        assert_eq!(
            (l.acute, l.chronic, l.acwr, l.acwr_percent),
            (Some(300.0), Some(280.0), Some(1.2), None)
        );
        let b = &d.training_balance[0];
        assert_eq!(
            (b.aerobic_low, b.aerobic_high, b.aerobic_low_target_min),
            (Some(5.0), None, Some(100.0))
        );
        assert_eq!(d.recovery_time.len(), 1);
        assert_eq!(
            (d.recovery_time[0].start_ts, d.recovery_time[0].minutes),
            (unix(T), 1440.0)
        );
        assert_eq!(d.skipped, 1); // device-relative recovery_time_start
    }

    #[test]
    fn training_status_row() {
        let d = run(FitBuilder::new().message(
            232,
            &[
                (253, Fv::U32(T)),
                (2, Fv::U8(4)),
                (5, Fv::U16(0xFFFF)),
                (3, Fv::U8(1)),
            ],
        ))
        .unwrap();
        let s = &d.training_status[0];
        assert_eq!(
            (s.status, s.fitness_trend, s.weekly_load_sum),
            (Some(4), Some(1), None)
        );
    }

    #[test]
    fn vo2max_from_max_met_data_and_standalone_firstbeat() {
        let d = run(FitBuilder::new()
            .message(
                229,
                &[(0, Fv::U32(T)), (1, Fv::S32(65536 * 12)), (5, Fv::U8(1))],
            )
            .message(229, &[(0, Fv::U32(T + 1)), (1, Fv::S32(0x7FFF_FFFF))])
            .message(140, &[(253, Fv::U32(T + 2)), (7, Fv::S32(65536 * 13))])
            .message(140, &[(253, Fv::U32(T + 3)), (7, Fv::S32(0x7FFF_FFFF))]))
        .unwrap();
        let v: Vec<_> = d
            .vo2max
            .iter()
            .map(|x| (x.ts, x.maximal_met, x.sport))
            .collect();
        assert_eq!(v, vec![(unix(T), 12.0, Some(1)), (unix(T + 2), 13.0, None)]);
    }

    #[test]
    fn firstbeat_in_activity_files_is_not_double_counted() {
        let d = run(FitBuilder::new()
            .message(34, &[(253, Fv::U32(T))])
            .message(140, &[(253, Fv::U32(T)), (7, Fv::S32(65536 * 13))]))
        .unwrap();
        assert!(d.activity.activity.is_some());
        assert!(d.vo2max.is_empty());
    }

    #[test]
    fn same_key_twice_in_one_file_emits_both_in_file_order() {
        let d = run(FitBuilder::new()
            .message(55, &[(253, Fv::U32(T)), (27, Fv::U8(60))])
            .message(55, &[(253, Fv::U32(T)), (27, Fv::U8(70))]))
        .unwrap();
        // The server keeps the first per (file, key); core stays faithful.
        let v: Vec<f64> = d.mon_hr.iter().map(|r| r.bpm).collect();
        assert_eq!(v, vec![60.0, 70.0]);
    }

    #[test]
    fn recovery_time_same_start_last_message_wins() {
        let d = run(FitBuilder::new()
            .message(284, &[(0, Fv::U16(600)), (1, Fv::U32(T))])
            .message(284, &[(0, Fv::U16(540)), (1, Fv::U32(T))]))
        .unwrap();
        assert_eq!(d.recovery_time.len(), 1);
        assert_eq!(d.recovery_time[0].minutes, 540.0);
    }

    #[test]
    fn non_step_cycles_are_counted_as_skipped() {
        // activity type 5 = swimming: cycles are strokes, not stored.
        let d =
            run(FitBuilder::new()
                .message(55, &[(253, Fv::U32(T)), (5, Fv::U8(5)), (3, Fv::U32(40))]))
            .unwrap();
        assert!(d.mon_activity.is_empty());
        assert_eq!(d.skipped, 1);
    }

    #[test]
    fn info_offset_needs_both_times_real() {
        let d = run(FitBuilder::new().message(
            103,
            &[(253, Fv::U32(T)), (0, Fv::U32(500)), (5, Fv::U16(1700))],
        ))
        .unwrap();
        assert_eq!(d.mon_info.unwrap().local_offset_s, None);
    }
}
