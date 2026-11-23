// Nimrag's own FIT field map (stage 5a). Not Garmin's profile: it was derived by
// observing the owner's own watch files and comparing them with values Garmin
// Connect reports for the same data, in a clean-room analysis that never read
// Garmin's profile (see docs/LICENSING.md). It covers only the messages, fields
// and enum values Nimrag reads, so lookups for anything else return None.
//
// Same format as a generated profile: `MESSAGES` sorted by global number with
// fields sorted by field number, `ENUMS` sorted by type name with values sorted.
// "ambiguous" marks a field whose observed values are identical in another
// field; the chosen one is the better-supported candidate.
pub(super) static MESSAGES: &[MessageInfo] = &[
    MessageInfo {
        global: 0,
        name: "file_id",
        fields: &[
            fe(0, "type", BaseType::Enum, "file", None, None, None, false, &[], &[]),
            fe(1, "manufacturer", BaseType::Uint16, "manufacturer", None, None, None, false, &[], &[]),
            fe(2, "product", BaseType::Uint16, "uint16", None, None, None, false, &[], &[]),
            fe(3, "serial_number", BaseType::Uint32z, "uint32z", None, None, None, false, &[], &[]),
            fe(4, "time_created", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
            fe(5, "number", BaseType::Uint16, "uint16", None, None, None, false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 18,
        name: "session",
        fields: &[
            fe(2, "start_time", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
            fe(5, "sport", BaseType::Enum, "sport", None, None, None, false, &[], &[]),
            fe(6, "sub_sport", BaseType::Enum, "sub_sport", None, None, None, false, &[], &[]),
            fe(7, "total_elapsed_time", BaseType::Uint32, "uint32", Some(1000.0), None, Some("s"), false, &[], &[]),
            fe(8, "total_timer_time", BaseType::Uint32, "uint32", Some(1000.0), None, Some("s"), false, &[], &[]),
            fe(9, "total_distance", BaseType::Uint32, "uint32", Some(100.0), None, Some("m"), false, &[], &[]),
            fe(11, "total_calories", BaseType::Uint16, "uint16", None, None, Some("kcal"), false, &[], &[]),
            fe(16, "avg_heart_rate", BaseType::Uint8, "uint8", None, None, Some("bpm"), false, &[], &[]),
            fe(17, "max_heart_rate", BaseType::Uint8, "uint8", None, None, Some("bpm"), false, &[], &[]),
            fe(22, "total_ascent", BaseType::Uint16, "uint16", None, None, Some("m"), false, &[], &[]),
            fe(23, "total_descent", BaseType::Uint16, "uint16", None, None, Some("m"), false, &[], &[]),
            fe(24, "total_training_effect", BaseType::Uint8, "uint8", Some(10.0), None, None, false, &[], &[]),
            fe(124, "enhanced_avg_speed", BaseType::Uint32, "uint32", Some(1000.0), None, Some("m/s"), false, &[], &[]),
            fe(125, "enhanced_max_speed", BaseType::Uint32, "uint32", Some(1000.0), None, Some("m/s"), false, &[], &[]),
            fe(137, "total_anaerobic_training_effect", BaseType::Uint8, "uint8", Some(10.0), None, None, false, &[], &[]),
            fe(168, "training_load_peak", BaseType::Sint32, "sint32", Some(65536.0), None, None, false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
            fe(254, "message_index", BaseType::Uint16, "uint16", None, None, None, false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 19,
        name: "lap",
        fields: &[
            fe(2, "start_time", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
            fe(7, "total_elapsed_time", BaseType::Uint32, "uint32", Some(1000.0), None, Some("s"), false, &[], &[]),
            fe(8, "total_timer_time", BaseType::Uint32, "uint32", Some(1000.0), None, Some("s"), false, &[], &[]),
            fe(9, "total_distance", BaseType::Uint32, "uint32", Some(100.0), None, Some("m"), false, &[], &[]),
            fe(11, "total_calories", BaseType::Uint16, "uint16", None, None, Some("kcal"), false, &[], &[]),
            fe(15, "avg_heart_rate", BaseType::Uint8, "uint8", None, None, Some("bpm"), false, &[], &[]),
            fe(16, "max_heart_rate", BaseType::Uint8, "uint8", None, None, Some("bpm"), false, &[], &[]),
            fe(21, "total_ascent", BaseType::Uint16, "uint16", None, None, Some("m"), false, &[], &[]),
            fe(22, "total_descent", BaseType::Uint16, "uint16", None, None, Some("m"), false, &[], &[]),
            fe(25, "sport", BaseType::Enum, "sport", None, None, None, false, &[], &[]),
            fe(39, "sub_sport", BaseType::Enum, "sub_sport", None, None, None, false, &[], &[]),
            fe(110, "enhanced_avg_speed", BaseType::Uint32, "uint32", Some(1000.0), None, Some("m/s"), false, &[], &[]),
            fe(111, "enhanced_max_speed", BaseType::Uint32, "uint32", Some(1000.0), None, Some("m/s"), false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 20,
        name: "record",
        fields: &[
            fe(0, "position_lat", BaseType::Sint32, "sint32", None, None, Some("semicircles"), false, &[], &[]),
            fe(1, "position_long", BaseType::Sint32, "sint32", None, None, Some("semicircles"), false, &[], &[]),
            fe(3, "heart_rate", BaseType::Uint8, "uint8", None, None, Some("bpm"), false, &[], &[]),
            fe(5, "distance", BaseType::Uint32, "uint32", Some(100.0), None, Some("m"), false, &[], &[]),
            fe(73, "enhanced_speed", BaseType::Uint32, "uint32", Some(1000.0), None, Some("m/s"), false, &[], &[]),
            fe(78, "enhanced_altitude", BaseType::Uint32, "uint32", Some(5.0), Some(500.0), Some("m"), false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 21,
        name: "event",
        fields: &[
            fe(0, "event", BaseType::Enum, "event", None, None, None, false, &[], &[]),
            fe(1, "event_type", BaseType::Enum, "event_type", None, None, None, false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 34,
        name: "activity",
        fields: &[
            fe(0, "total_timer_time", BaseType::Uint32, "uint32", Some(1000.0), None, Some("s"), false, &[], &[]),
            fe(1, "num_sessions", BaseType::Uint16, "uint16", None, None, None, false, &[], &[]),
            fe(5, "local_timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 49,
        name: "file_creator",
        fields: &[
            fe(0, "software_version", BaseType::Uint16, "uint16", None, None, None, false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 55,
        name: "monitoring",
        fields: &[
            fe(2, "distance", BaseType::Uint32, "uint32", Some(100.0), None, Some("m"), false, &[], &[]),
            // scale 2 is Nimrag's own assumption (no non-step counter observed); `steps` applies to running and walking
            fe(3, "cycles", BaseType::Uint32, "uint32", Some(2.0), None, Some("cycles"), false, &[], &[sub("steps", BaseType::Uint32, "uint32", None, None, Some("steps"), &[(5, 1), (5, 6)], &[])]),
            fe(4, "active_time", BaseType::Uint32, "uint32", Some(1000.0), None, Some("s"), false, &[], &[]),
            fe(5, "activity_type", BaseType::Enum, "activity_type", None, None, None, false, &[], &[]),
            fe(19, "active_calories", BaseType::Uint16, "uint16", None, None, Some("kcal"), false, &[], &[]),
            fe(24, "current_activity_type_intensity", BaseType::Byte, "byte", None, None, None, false, &[comp(5, 5, None, None, None, false), comp(28, 3, None, None, None, false)], &[]),
            fe(26, "timestamp_16", BaseType::Uint16, "uint16", None, None, Some("s"), false, &[], &[]),
            fe(27, "heart_rate", BaseType::Uint8, "uint8", None, None, Some("bpm"), false, &[], &[]),
            // target of the 3-bit component of field 24 (number as in Nimrag's own derive test)
            fe(28, "intensity", BaseType::Uint8, "uint8", None, None, None, false, &[], &[]),
            fe(29, "duration_min", BaseType::Uint16, "uint16", None, None, Some("min"), false, &[], &[]),
            fe(35, "ascent_total", BaseType::Uint32, "uint32", Some(1000.0), None, Some("m"), false, &[], &[]),
            fe(36, "descent_total", BaseType::Uint32, "uint32", Some(1000.0), None, Some("m"), false, &[], &[]),
            fe(37, "moderate_activity_minutes_total", BaseType::Uint16, "uint16", None, None, Some("min"), false, &[], &[]),
            fe(38, "vigorous_activity_minutes_total", BaseType::Uint16, "uint16", None, None, Some("min"), false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 103,
        name: "monitoring_info",
        fields: &[
            fe(0, "local_timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
            fe(5, "resting_metabolic_rate", BaseType::Uint16, "uint16", None, None, Some("kcal/day"), false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 104,
        name: "battery",
        fields: &[
            fe(0, "voltage", BaseType::Uint16, "uint16", Some(1000.0), None, Some("V"), false, &[], &[]),
            fe(2, "capacity", BaseType::Uint8, "uint8", None, None, Some("%"), false, &[], &[]),
            fe(3, "temperature", BaseType::Sint8, "sint8", None, None, Some("C"), false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 140,
        name: "firstbeat_results",
        fields: &[
            // ambiguous: epoc only observed in the activity-file message (<= peak load); also not field 5 (larger than peak)
            fe(2, "epoc", BaseType::Sint32, "sint32", Some(65536.0), None, None, false, &[], &[]),
            fe(4, "training_effect", BaseType::Uint8, "uint8", Some(10.0), None, None, false, &[], &[]),
            // valid in activity files only (MET x 3.5 matches Connect's VO2 max), invalid in the history file
            fe(7, "maximal_met", BaseType::Sint32, "sint32", Some(65536.0), None, Some("MET"), false, &[], &[]),
            fe(9, "recovery_time_physical_activity", BaseType::Uint16, "uint16", None, None, Some("min"), false, &[], &[]),
            fe(11, "sport", BaseType::Enum, "sport", None, None, None, false, &[], &[]),
            fe(20, "anaerobic_training_effect", BaseType::Uint8, "uint8", Some(10.0), None, None, false, &[], &[]),
            // ambiguous: also fields 3, 37
            fe(39, "training_load_peak", BaseType::Sint32, "sint32", Some(65536.0), None, None, false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 211,
        name: "monitoring_hr_data",
        fields: &[
            fe(0, "resting_heart_rate", BaseType::Uint8, "uint8", None, None, Some("bpm"), false, &[], &[]),
            fe(1, "current_day_resting_heart_rate", BaseType::Uint8, "uint8", None, None, Some("bpm"), false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 216,
        name: "time_in_zone",
        fields: &[
            fe(0, "reference_mesg", BaseType::Uint16, "uint16", None, None, None, false, &[], &[]),
            fe(1, "reference_index", BaseType::Uint16, "uint16", None, None, None, false, &[], &[]),
            arr(fe(2, "time_in_hr_zone", BaseType::Uint32, "uint32", Some(1000.0), None, Some("s"), false, &[], &[])),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 227,
        name: "stress_level",
        fields: &[
            fe(0, "stress_level_value", BaseType::Sint16, "sint16", None, None, None, false, &[], &[]),
            fe(1, "stress_level_time", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
            fe(3, "body_battery", BaseType::Sint8, "sint8", None, None, Some("%"), false, &[], &[]),
            fe(4, "body_battery_status", BaseType::Sint8, "body_battery_status", None, None, None, false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 232,
        name: "training_history_results",
        fields: &[
            fe(2, "training_status", BaseType::Enum, "training_status", None, None, None, false, &[], &[]),
            fe(3, "fitness_level_trend", BaseType::Enum, "fitness_level_trend", None, None, None, false, &[], &[]),
            // load_level_trend: left out. The 1:1 diff showed the reference never fills it.
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 275,
        name: "sleep_level",
        fields: &[
            fe(0, "sleep_level", BaseType::Enum, "sleep_level", None, None, None, false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 284,
        name: "recovery_time",
        fields: &[
            // ambiguous: also field 5
            fe(0, "recovery_time_minutes", BaseType::Uint16, "uint16", None, None, Some("min"), false, &[], &[]),
            fe(1, "recovery_time_start", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 294,
        name: "training_balance_results",
        fields: &[
            fe(0, "monthly_load_aerobic_low", BaseType::Sint32, "sint32", Some(65536.0), None, None, false, &[], &[]),
            fe(1, "monthly_load_aerobic_high", BaseType::Sint32, "sint32", Some(65536.0), None, None, false, &[], &[]),
            fe(2, "monthly_load_anaerobic", BaseType::Sint32, "sint32", Some(65536.0), None, None, false, &[], &[]),
            fe(3, "monthly_load_aerobic_low_target_min", BaseType::Uint16, "uint16", None, None, None, false, &[], &[]),
            // Fields 4, 5, 6 (aerobic target ranges): left out. The 1:1 diff showed the
            // reference never fills the columns they would feed.
            fe(7, "monthly_load_anaerobic_target_min", BaseType::Uint16, "uint16", None, None, None, false, &[], &[]),
            fe(8, "monthly_load_anaerobic_target_max", BaseType::Uint16, "uint16", None, None, None, false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 297,
        name: "respiration_rate",
        fields: &[
            fe(0, "respiration_rate", BaseType::Sint16, "sint16", Some(100.0), None, Some("breaths/min"), false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 346,
        name: "sleep_assessment",
        fields: &[
            fe(3, "deep_sleep_score", BaseType::Uint8, "uint8", None, None, None, false, &[], &[]),
            fe(4, "sleep_duration_score", BaseType::Uint8, "uint8", None, None, None, false, &[], &[]),
            // ambiguous: fields 5..9 follow name order (light, overall, quality, recovery, rem); 5 and 9 can swap
            fe(5, "light_sleep_score", BaseType::Uint8, "uint8", None, None, None, false, &[], &[]),
            fe(6, "overall_sleep_score", BaseType::Uint8, "uint8", None, None, None, false, &[], &[]),
            fe(7, "sleep_quality_score", BaseType::Uint8, "uint8", None, None, None, false, &[], &[]),
            fe(8, "sleep_recovery_score", BaseType::Uint8, "uint8", None, None, None, false, &[], &[]),
            fe(9, "rem_sleep_score", BaseType::Uint8, "uint8", None, None, None, false, &[], &[]),
            fe(11, "awakenings_count", BaseType::Uint8, "uint8", None, None, None, false, &[], &[]),
            // weak: scale 100 assumed
            fe(15, "average_stress_during_sleep", BaseType::Uint16, "uint16", Some(100.0), None, None, false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 370,
        name: "hrv_status_summary",
        fields: &[
            fe(0, "weekly_average", BaseType::Uint16, "uint16", Some(128.0), None, Some("ms"), false, &[], &[]),
            fe(1, "last_night_average", BaseType::Uint16, "uint16", Some(128.0), None, Some("ms"), false, &[], &[]),
            fe(2, "last_night_5_min_high", BaseType::Uint16, "uint16", Some(128.0), None, Some("ms"), false, &[], &[]),
            fe(3, "baseline_low_upper", BaseType::Uint16, "uint16", Some(128.0), None, Some("ms"), false, &[], &[]),
            fe(4, "baseline_balanced_lower", BaseType::Uint16, "uint16", Some(128.0), None, Some("ms"), false, &[], &[]),
            fe(5, "baseline_balanced_upper", BaseType::Uint16, "uint16", Some(128.0), None, Some("ms"), false, &[], &[]),
            fe(6, "status", BaseType::Enum, "hrv_status", None, None, None, false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 371,
        name: "hrv_value",
        fields: &[
            fe(0, "value", BaseType::Uint16, "uint16", Some(128.0), None, Some("ms"), false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 378,
        name: "acute_load_results",
        fields: &[
            // ambiguous: also field 5 (the other of the ratio/percent pair)
            fe(0, "acwr_percent", BaseType::Uint8, "uint8", None, None, None, false, &[], &[]),
            fe(3, "daily_training_load_acute", BaseType::Uint16, "uint16", None, None, None, false, &[], &[]),
            fe(4, "daily_training_load_chronic", BaseType::Uint16, "uint16", None, None, None, false, &[], &[]),
            // ambiguous: also field 0 (the other of the ratio/percent pair)
            fe(5, "daily_acute_chronic_workload_ratio", BaseType::Uint8, "uint8", None, None, None, false, &[], &[]),
            fe(253, "timestamp", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
        ],
    },
    MessageInfo {
        global: 411,
        name: "sleep_summary",
        fields: &[
            fe(0, "overall_sleep_score", BaseType::Uint8, "uint8", None, None, None, false, &[], &[]),
            fe(8, "sleep_end_timestamp_utc", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
            fe(10, "sleep_start_timestamp_utc", BaseType::Uint32, "date_time", None, None, Some("s"), false, &[], &[]),
            fe(254, "message_index", BaseType::Uint16, "uint16", None, None, None, false, &[], &[]),
        ],
    },
];
pub(super) static ENUMS: &[(&str, &[(u64, &str)])] = &[
    ("activity_type", &[(0, "generic"), (1, "running"), (6, "walking"), (8, "sedentary")]),
    ("body_battery_status", &[(1, "measured")]),
    ("event", &[(74, "sleep")]),
    ("event_type", &[(0, "start"), (1, "stop")]),
    ("hrv_status", &[(3, "unbalanced")]),
    ("sleep_level", &[(1, "awake"), (2, "light"), (3, "deep"), (4, "rem")]),
    ("sport", &[(0, "generic"), (1, "running"), (2, "cycling")]),
    ("sub_sport", &[(0, "generic")]),
    ("training_status", &[(5, "recovery")]),
];
pub(super) static VERSION: Option<u32> = None;
