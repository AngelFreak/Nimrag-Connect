//! FIT profile lookups: message/field names, base and profile types,
//! scale/offset/units, subfields, components and enum value names.
//!
//! This file is the API only and contains no profile data. The table
//! (`MESSAGES`, `ENUMS`, `VERSION`) is generated locally by
//! `tools/fit-profile/extract.py` into the git-ignored
//! `src/fit/generated/profile_data.rs`; `build.rs` selects it (or the committed
//! empty table) and sets `cfg(fit_profile)`. Without it every lookup returns
//! `None` and [`available`] is false.
//!
//! Extraction notes (overlay): the app's profile does not record array-ness or a
//! profile version, so `array` is false and `profile_version()` is None unless a
//! table provides them. `scale`/`offset` are `None` for 1.0 / 0.0 and `units` is
//! `None` when empty. `type_name` is the lower-case profile type.

use super::BaseType;

/// Static description of one message field.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldInfo {
    pub name: &'static str,
    pub base_type: BaseType,
    /// Profile type name (`uint8`, `date_time`, `sport`, ...), lower case.
    pub type_name: &'static str,
    pub scale: Option<f64>,
    pub offset: Option<f64>,
    pub units: Option<&'static str>,
    pub array: bool,
    pub accumulate: bool,
    pub subfields: &'static [SubField],
    pub components: &'static [Component],
}

/// Expansion of a packed field into another field. Data only; no expansion logic.
#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    /// Target field number.
    pub field_num: u8,
    pub bits: u8,
    pub scale: Option<f64>,
    pub offset: Option<f64>,
    pub units: Option<&'static str>,
    pub accumulate: bool,
}

/// Alternative interpretation of a field, selected by another field's value.
#[derive(Debug, Clone, PartialEq)]
pub struct SubField {
    pub name: &'static str,
    pub base_type: BaseType,
    pub type_name: &'static str,
    pub scale: Option<f64>,
    pub offset: Option<f64>,
    pub units: Option<&'static str>,
    /// `(reference field number, value)`; the subfield applies if any pair matches.
    pub when: &'static [(u8, u64)],
    pub components: &'static [Component],
}

struct FieldEntry {
    num: u8,
    info: FieldInfo,
}

struct MessageInfo {
    global: u16,
    name: &'static str,
    fields: &'static [FieldEntry],
}

type EnumTable = &'static [(&'static str, &'static [(u64, &'static str)])];

#[derive(Clone, Copy)]
struct Tables {
    messages: &'static [MessageInfo],
    enums: EnumTable,
    version: Option<u32>,
}

#[cfg_attr(not(fit_profile), allow(dead_code))]
#[allow(clippy::too_many_arguments)]
const fn fe(
    num: u8,
    name: &'static str,
    base_type: BaseType,
    type_name: &'static str,
    scale: Option<f64>,
    offset: Option<f64>,
    units: Option<&'static str>,
    accumulate: bool,
    components: &'static [Component],
    subfields: &'static [SubField],
) -> FieldEntry {
    FieldEntry {
        num,
        info: FieldInfo {
            name,
            base_type,
            type_name,
            scale,
            offset,
            units,
            array: false,
            accumulate,
            subfields,
            components,
        },
    }
}

#[cfg_attr(not(fit_profile), allow(dead_code))]
const fn comp(
    field_num: u8,
    bits: u8,
    scale: Option<f64>,
    offset: Option<f64>,
    units: Option<&'static str>,
    accumulate: bool,
) -> Component {
    Component {
        field_num,
        bits,
        scale,
        offset,
        units,
        accumulate,
    }
}

#[cfg_attr(not(fit_profile), allow(dead_code))]
#[allow(clippy::too_many_arguments)]
const fn sub(
    name: &'static str,
    base_type: BaseType,
    type_name: &'static str,
    scale: Option<f64>,
    offset: Option<f64>,
    units: Option<&'static str>,
    when: &'static [(u8, u64)],
    components: &'static [Component],
) -> SubField {
    SubField {
        name,
        base_type,
        type_name,
        scale,
        offset,
        units,
        when,
        components,
    }
}

#[allow(clippy::all, clippy::pedantic, unused_imports)]
mod data {
    use super::{comp, fe, sub, BaseType, MessageInfo};
    include!(concat!(env!("OUT_DIR"), "/profile_selected.rs"));
}

const DATA: Tables = Tables {
    messages: data::MESSAGES,
    enums: data::ENUMS,
    version: data::VERSION,
};
const ALL: [Tables; 1] = [DATA];

/// True when a generated profile table was compiled in (`cfg(fit_profile)`).
pub fn available() -> bool {
    cfg!(fit_profile)
}

fn message_in(t: &[Tables], global: u16) -> impl Iterator<Item = &'static MessageInfo> + '_ {
    t.iter().filter_map(move |t| {
        let i = t
            .messages
            .binary_search_by_key(&global, |m| m.global)
            .ok()?;
        Some(&t.messages[i])
    })
}

fn entry_in(t: &[Tables], global: u16, num: u8) -> Option<&'static FieldEntry> {
    message_in(t, global).find_map(|m| {
        let i = m.fields.binary_search_by_key(&num, |f| f.num).ok()?;
        Some(&m.fields[i])
    })
}

fn message_name_in(t: &[Tables], global: u16) -> Option<&'static str> {
    message_in(t, global).next().map(|m| m.name)
}

fn message_number_in(t: &[Tables], name: &str) -> Option<u16> {
    t.iter()
        .find_map(|t| t.messages.iter().find(|m| m.name == name).map(|m| m.global))
}

fn field_by_name_in(t: &[Tables], global: u16, name: &str) -> Option<(u8, &'static FieldInfo)> {
    message_in(t, global).find_map(|m| {
        m.fields
            .iter()
            .find(|f| f.info.name == name)
            .map(|f| (f.num, &f.info))
    })
}

fn enum_name_in(t: &[Tables], type_name: &str, value: u64) -> Option<&'static str> {
    t.iter().find_map(|t| {
        let i = t.enums.binary_search_by_key(&type_name, |e| e.0).ok()?;
        let vals = t.enums[i].1;
        let j = vals.binary_search_by_key(&value, |v| v.0).ok()?;
        Some(vals[j].1)
    })
}

/// Message name for a global message number, e.g. `20` -> `"record"`.
pub fn message_name(global: u16) -> Option<&'static str> {
    message_name_in(&ALL, global)
}

/// Global message number for a message name, e.g. `"record"` -> `20`.
pub fn message_number(name: &str) -> Option<u16> {
    message_number_in(&ALL, name)
}

/// Field description (incl. subfields and components) by message and field number.
pub fn field(global: u16, num: u8) -> Option<&'static FieldInfo> {
    entry_in(&ALL, global, num).map(|e| &e.info)
}

/// Field number and description by message and field name.
pub fn field_by_name(global: u16, name: &str) -> Option<(u8, &'static FieldInfo)> {
    field_by_name_in(&ALL, global, name)
}

/// Enum value name, e.g. `("file", 4)` -> `"activity"`. `type_name` is the
/// lower-case profile type; `"mesg_num"` maps global message numbers to names.
pub fn enum_name(type_name: &str, value: u64) -> Option<&'static str> {
    enum_name_in(&ALL, type_name, value)
}

/// Profile version, if the table provides one.
pub fn profile_version() -> Option<u32> {
    ALL.iter().find_map(|t| t.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(fit_profile)]
    #[test]
    fn record_heart_rate() {
        let f = field(20, 3).expect("record.heart_rate");
        assert_eq!(f.name, "heart_rate");
        assert_eq!(f.base_type, BaseType::Uint8);
        assert_eq!(f.units, Some("bpm"));
        assert_eq!(f.scale, None);
    }

    #[cfg(fit_profile)]
    #[test]
    fn record_position_lat() {
        let f = field(20, 0).expect("record.position_lat");
        assert_eq!(f.name, "position_lat");
        assert_eq!(f.base_type, BaseType::Sint32);
        assert_eq!(f.units, Some("semicircles"));
    }

    #[cfg(fit_profile)]
    #[test]
    fn session_total_timer_time_scale() {
        // The app's profile (like the public FIT profile) has total_elapsed_time = 7
        // and total_timer_time = 8.
        assert_eq!(field(18, 7).map(|f| f.name), Some("total_elapsed_time"));
        let f = field(18, 8).expect("session.total_timer_time");
        assert_eq!(f.name, "total_timer_time");
        assert_eq!(f.scale, Some(1000.0));
        assert_eq!(f.units, Some("s"));
    }

    #[cfg(fit_profile)]
    #[test]
    fn message_names_round_trip() {
        assert_eq!(message_name(20), Some("record"));
        assert_eq!(message_number("record"), Some(20));
        assert_eq!(message_number("sleep_level"), Some(275));
        assert_eq!(message_name(275), Some("sleep_level"));
    }

    #[cfg(fit_profile)]
    #[test]
    fn field_by_name_finds_number() {
        let (num, f) = field_by_name(20, "heart_rate").expect("heart_rate");
        assert_eq!(num, 3);
        assert_eq!(f.units, Some("bpm"));
    }

    #[cfg(fit_profile)]
    #[test]
    fn enums_resolve() {
        assert_eq!(enum_name("file", 4), Some("activity"));
        assert_eq!(enum_name("file", 32), Some("monitoring_b"));
        assert_eq!(enum_name("mesg_num", 20), Some("record"));
    }

    #[test]
    fn unknown_lookups_return_none() {
        assert_eq!(message_name(0xFFFE), None);
        assert_eq!(message_number("no_such_message"), None);
        assert!(field(20, 250).is_none());
        assert!(field(0xFFFE, 0).is_none());
        assert!(field_by_name(20, "no_such_field").is_none());
        assert_eq!(enum_name("no_such_type", 0), None);
        assert_eq!(enum_name("file", 0xFFFF), None);
    }

    #[cfg(fit_profile)]
    #[test]
    fn record_altitude_component_and_session_subfields() {
        let c = field(20, 2).expect("altitude").components;
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].field_num, 78);
        assert_eq!(c[0].bits, 16);
        assert_eq!(c[0].scale, Some(5.0));
        assert_eq!(c[0].offset, Some(500.0));
        let subs = field(18, 10).expect("session.total_cycles").subfields;
        let strides = subs
            .iter()
            .find(|s| s.name == "total_strides")
            .expect("total_strides");
        assert!(strides.when.contains(&(5, 1)));
    }

    #[cfg(fit_profile)]
    #[test]
    fn record_speed_components_and_accumulate() {
        let sp = field(20, 6).expect("record.speed").components;
        assert!(sp.iter().any(|c| c.field_num == 73));
        assert!(field(20, 5).expect("record.distance").accumulate);
    }

    #[cfg(fit_profile)]
    #[test]
    fn monitoring_cycles_subfields_keyed_on_activity_type() {
        let (_, f) = field_by_name(55, "cycles").expect("monitoring.cycles");
        let steps = f
            .subfields
            .iter()
            .find(|s| s.name == "steps")
            .expect("steps");
        assert!(!steps.when.is_empty());
        assert!(f.subfields.iter().any(|s| s.name == "strokes"));
    }

    #[cfg(fit_profile)]
    #[test]
    fn monitoring_intensity_components() {
        let (_, f) = field_by_name(55, "current_activity_type_intensity").expect("intensity");
        let bits: Vec<u8> = f.components.iter().map(|c| c.bits).collect();
        assert_eq!(bits, [5, 3]);
    }

    #[cfg(fit_profile)]
    #[test]
    fn event_data_has_subfields_keyed_on_event() {
        let (_, f) = field_by_name(21, "data").expect("event.data");
        assert!(!f.subfields.is_empty());
        assert!(f.subfields.iter().all(|s| s.when.iter().all(|w| w.0 == 0)));
    }

    #[test]
    fn empty_tables_are_handled() {
        let empty = Tables {
            messages: &[],
            enums: &[],
            version: None,
        };
        assert!(message_name_in(&[empty], 20).is_none());
        assert!(entry_in(&[empty], 20, 3).is_none());
        assert!(enum_name_in(&[empty], "file", 4).is_none());
        assert_eq!(
            message_name_in(&[empty, DATA], 20),
            message_name_in(&[DATA], 20)
        );
        assert_eq!(
            message_name_in(&[DATA, empty], 20),
            message_name_in(&[DATA], 20)
        );
    }

    #[test]
    fn availability_matches_build_config() {
        assert_eq!(available(), cfg!(fit_profile));
        if !available() {
            assert!(message_name(20).is_none());
            assert!(message_number("record").is_none());
            assert!(field(20, 3).is_none());
            assert!(field_by_name(20, "heart_rate").is_none());
            assert!(enum_name("file", 4).is_none());
            assert!(profile_version().is_none());
        }
    }

    #[cfg(fit_profile)]
    #[test]
    fn priority_messages_present() {
        for name in [
            "file_id",
            "device_info",
            "record",
            "lap",
            "session",
            "event",
            "activity",
            "set",
            "exercise_title",
            "split",
            "hrv",
            "monitoring",
            "monitoring_info",
            "sleep_level",
            "sleep_assessment",
            "stress_level",
            "respiration_rate",
            "hrv_status_summary",
            "hrv_value",
            "spo2_data",
            "max_met_data",
            "training_readiness",
            "endurance_score",
        ] {
            assert!(message_number(name).is_some(), "missing message {name}");
        }
    }
}
