//! Workout FIT files (stage 4a): a typed workout in, the bytes of a FIT workout
//! file out, ready to be uploaded to the watch as file type 128/5.
//!
//! Ported from Gadgetbridge commit 7257742b (AGPL-3.0-or-later):
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/fit/workouts/GarminWorkoutFitEncoder.kt`
//! and `.../fit/workouts/GarminWorkoutCodes.kt` (neither file carries a
//! copyright line; Gadgetbridge contributors), and `GarminSupport.onSyncWorkoutTemplate`
//! for the file type. See docs/PORTED.md. Scope: running, cycling, walking,
//! hiking, strength (as timed intervals) and other; steps with a duration and a
//! target, repeat blocks, step notes. Not ported: pool swimming, multisport legs,
//! power targets, Reps+ and the exercise catalogue (`GarminExerciseCatalog`,
//! a follow-up: it needs the exercise list, which is Garmin profile data).
//!
//! What was ported, all *inferred (Gadgetbridge)* until a watch runs a file made
//! here:
//! - message order: `file_id`, `file_creator`, `workout`, then every
//!   `workout_step` in index order, a step's notes followed by a `memo_glob`
//!   that carries the same text (up to 254 bytes, NUL-terminated);
//! - `file_id`: type workout, manufacturer garmin, product `connect` (65534),
//!   serial number 1, number 1, creation time; `file_creator.software_version` 1;
//! - `workout`: index 0, name, description (the note), sport, sub sport,
//!   `capabilities` 32 (Gadgetbridge: "what are these?"), `num_valid_steps`;
//! - a repeat is one extra `workout_step` after its steps:
//!   `repeat_until_steps_cmplt`, `duration_value` the index of the block's first
//!   step, `target_value` the number of passes; steps of the block are encoded
//!   before it, so indices are the step count so far;
//! - every step carries `intensity` and `weight_display_unit` = kilogram;
//! - durations: time and distance through the profile's own subfield scales,
//!   heart rate below/above as bpm + 100, no value for "open";
//! - targets: none = `open` with value 0; heart-rate zone = value; heart-rate range
//!   = custom low/high as bpm + 100; pace as a speed range in mm/s (the slower pace
//!   is the low speed); cadence halved for running (steps to revolutions).
//!
//! What is ours [ours]: the model's units (seconds, metres, s/km, km/h), the
//! validation (the limits below), a UTF-8-safe cut of long names, and the 50-byte
//! limit of the step's `notes` field (the full text is in the `memo_glob`). Where
//! the limits come from is not in Gadgetbridge; they are cautious and listed as
//! open questions for the live test.
//!
//! Every message and field is looked up **by name** in `core::fit::profile`
//! (`encode::MessageBuilder`). With no profile compiled in, [`encode`] answers
//! `EncodeError::Unavailable` and writes nothing.

use super::encode::{EncodeError, FitWriter, MessageBuilder, RawMessage};
use super::profile;

/// The file type of a workout on the watch: data type 128 (FIT), sub type 5.
/// *inferred (Gadgetbridge, `FileType.FILETYPE.WORKOUTS`)*
pub const WORKOUT_DATA_TYPE: u8 = 128;
pub const WORKOUT_SUB_TYPE: u8 = 5;

/// Most steps in one workout, repeat steps included.
pub const MAX_STEPS: usize = 100;
/// Most steps inside one repeat block.
pub const MAX_BLOCK_STEPS: usize = 20;
/// Repeat counts accepted (passes through the block).
pub const REPEAT_MIN: u16 = 2;
pub const REPEAT_MAX: u16 = 99;
/// Name length the encoder writes at most (bytes, without the NUL).
pub const NAME_MAX_BYTES: usize = 63;
/// The step's own `notes` field is cut to this (bytes, without the NUL); the
/// `memo_glob` carries the whole note.
pub const STEP_NOTES_FIELD_BYTES: usize = 49;
/// Longest note (bytes): what a `memo_glob` holds without its NUL.
pub const NOTE_MAX_BYTES: usize = 254;
/// A bpm in a heart-rate target or duration is stored with this offset
/// (values up to 100 mean percent of the maximum). *inferred (Gadgetbridge, `HR_OFFSET`)*
const HR_OFFSET: u64 = 100;
const CAPABILITIES: u64 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sport {
    Running,
    Cycling,
    Walking,
    Hiking,
    /// FIT sport `training`, sub sport `strength_training`.
    Strength,
    Other,
}

impl Sport {
    /// FIT sport and sub sport names.
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Sport::Running => ("running", "generic"),
            Sport::Cycling => ("cycling", "generic"),
            Sport::Walking => ("walking", "generic"),
            Sport::Hiking => ("hiking", "generic"),
            Sport::Strength => ("training", "strength_training"),
            Sport::Other => ("generic", "generic"),
        }
    }

    /// A running cadence is stored in revolutions of two steps. [Gadgetbridge `isHalvedCadence`]
    fn halves_cadence(self) -> bool {
        self == Sport::Running
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intensity {
    Warmup,
    Active,
    Rest,
    Cooldown,
    Recovery,
}

impl Intensity {
    fn name(self) -> &'static str {
        match self {
            Intensity::Warmup => "warmup",
            Intensity::Active => "active",
            Intensity::Rest => "rest",
            Intensity::Cooldown => "cooldown",
            Intensity::Recovery => "recovery",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Duration {
    /// Until the lap button is pressed.
    Open,
    Time {
        seconds: u32,
    },
    Distance {
        meters: u32,
    },
    HeartRateBelow {
        bpm: u16,
    },
    HeartRateAbove {
        bpm: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    None,
    /// Zone 1 to 5 of the watch's own heart-rate zones.
    HeartRateZone {
        zone: u8,
    },
    HeartRateRange {
        low_bpm: u16,
        high_bpm: u16,
    },
    /// A pace range in seconds per kilometre; `fast` is the smaller number.
    Pace {
        fast_s_per_km: f64,
        slow_s_per_km: f64,
    },
    Speed {
        low_kmh: f64,
        high_kmh: f64,
    },
    /// Steps (or revolutions, cycling) per minute.
    Cadence {
        low: u16,
        high: u16,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub intensity: Intensity,
    pub duration: Duration,
    pub target: Target,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Step(Step),
    /// The steps run `count` times in all.
    Repeat {
        count: u16,
        steps: Vec<Step>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Workout {
    pub name: String,
    pub sport: Sport,
    pub note: Option<String>,
    pub nodes: Vec<Node>,
}

impl Workout {
    /// Steps in the file: every step plus one per repeat block.
    pub fn step_count(&self) -> usize {
        self.nodes
            .iter()
            .map(|n| match n {
                Node::Step(_) => 1,
                Node::Repeat { steps, .. } => steps.len() + 1,
            })
            .sum()
    }

    /// Check the limits of this module; the text of the error says which one.
    pub fn validate(&self) -> Result<(), EncodeError> {
        let bad = |s: String| Err(EncodeError::Invalid(s));
        if self.name.trim().is_empty() {
            return bad("the workout needs a name".into());
        }
        if self.nodes.is_empty() {
            return bad("the workout has no steps".into());
        }
        if self.step_count() > MAX_STEPS {
            return bad(format!("a workout has at most {MAX_STEPS} steps"));
        }
        if self.note.as_ref().is_some_and(|n| n.len() > NOTE_MAX_BYTES) {
            return bad(format!("the note is at most {NOTE_MAX_BYTES} bytes"));
        }
        for node in &self.nodes {
            match node {
                Node::Step(s) => s.validate()?,
                Node::Repeat { count, steps } => {
                    if !(REPEAT_MIN..=REPEAT_MAX).contains(count) {
                        return bad(format!("a repeat runs {REPEAT_MIN} to {REPEAT_MAX} times"));
                    }
                    if steps.is_empty() || steps.len() > MAX_BLOCK_STEPS {
                        return bad(format!("a repeat block holds 1 to {MAX_BLOCK_STEPS} steps"));
                    }
                    for s in steps {
                        s.validate()?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Step {
    fn validate(&self) -> Result<(), EncodeError> {
        let bad = |s: &str| Err(EncodeError::Invalid(s.to_owned()));
        if self.note.as_ref().is_some_and(|n| n.len() > NOTE_MAX_BYTES) {
            return bad("a step note is at most 254 bytes");
        }
        match self.duration {
            Duration::Open => {}
            Duration::Time { seconds } if (1..=86_400).contains(&seconds) => {}
            Duration::Time { .. } => return bad("a step lasts 1 second to 24 hours"),
            Duration::Distance { meters } if (1..=500_000).contains(&meters) => {}
            Duration::Distance { .. } => return bad("a step covers 1 metre to 500 km"),
            Duration::HeartRateBelow { bpm } | Duration::HeartRateAbove { bpm }
                if (30..=240).contains(&bpm) => {}
            Duration::HeartRateBelow { .. } | Duration::HeartRateAbove { .. } => {
                return bad("a heart rate is 30 to 240 bpm")
            }
        }
        match self.target {
            Target::None => {}
            Target::HeartRateZone { zone } if (1..=5).contains(&zone) => {}
            Target::HeartRateZone { .. } => return bad("a heart-rate zone is 1 to 5"),
            Target::HeartRateRange { low_bpm, high_bpm } => {
                if !(30..=240).contains(&low_bpm)
                    || !(30..=240).contains(&high_bpm)
                    || low_bpm > high_bpm
                {
                    return bad("a heart-rate range is 30 to 240 bpm, low not above high");
                }
            }
            Target::Pace {
                fast_s_per_km,
                slow_s_per_km,
            } => {
                let ok = |p: f64| p.is_finite() && (60.0..=3600.0).contains(&p);
                if !ok(fast_s_per_km) || !ok(slow_s_per_km) || fast_s_per_km > slow_s_per_km {
                    return bad("a pace range is 1:00 to 60:00 per km, fast not slower than slow");
                }
            }
            Target::Speed { low_kmh, high_kmh } => {
                let ok = |v: f64| v.is_finite() && (0.5..=100.0).contains(&v);
                if !ok(low_kmh) || !ok(high_kmh) || low_kmh > high_kmh {
                    return bad("a speed range is 0.5 to 100 km/h, low not above high");
                }
            }
            Target::Cadence { low, high } => {
                if !(1..=300).contains(&low) || !(1..=300).contains(&high) || low > high {
                    return bad("a cadence range is 1 to 300, low not above high");
                }
            }
        }
        Ok(())
    }
}

/// Cut `text` to at most `max` bytes at a character boundary.
fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The messages [`encode`] writes. A profile that lacks one cannot write workouts.
const MESSAGES: [&str; 5] = [
    "file_id",
    "file_creator",
    "workout",
    "workout_step",
    "memo_glob",
];

/// True when the compiled-in profile knows every message a workout file needs
/// (always with Garmin's profile; never with Nimrag's own partial map, which
/// has none of the workout messages; never without a profile).
pub fn available() -> bool {
    MESSAGES.iter().all(|m| profile::has_message(m))
}

/// The FIT file of `workout`, created at `created_garmin_s` (seconds since the
/// FIT epoch, 1989-12-31T00:00Z). `Unavailable` when the profile cannot write
/// workouts (see [`available`]); then nothing is written.
pub fn encode(workout: &Workout, created_garmin_s: u32) -> Result<Vec<u8>, EncodeError> {
    if !available() {
        return Err(EncodeError::Unavailable);
    }
    workout.validate()?;
    let mut w = FitWriter::new();
    w.push(&file_id(created_garmin_s)?)?;
    w.push(&file_creator()?)?;
    w.push(&workout_message(workout)?)?;
    let mut index: u16 = 0;
    for node in &workout.nodes {
        match node {
            Node::Step(step) => {
                push_step(&mut w, step, &mut index, workout.sport)?;
            }
            Node::Repeat { count, steps } => {
                let first = index;
                for step in steps {
                    push_step(&mut w, step, &mut index, workout.sport)?;
                }
                let mut m = MessageBuilder::new("workout_step")?;
                m.uint("message_index", u64::from(index))?;
                m.enum_name("duration_type", "repeat_until_steps_cmplt")?;
                m.uint("duration_value", u64::from(first))?;
                m.uint("target_value", u64::from(*count))?;
                w.push(&m.build())?;
                index += 1;
            }
        }
    }
    w.finish()
}

fn file_id(created_garmin_s: u32) -> Result<RawMessage, EncodeError> {
    let mut m = MessageBuilder::new("file_id")?;
    m.enum_name("type", "workout")?;
    m.enum_name("manufacturer", "garmin")?;
    m.sub_enum_name("product", "garmin_product", "connect")?;
    m.uint("serial_number", 1)?;
    m.uint("number", 1)?;
    m.uint("time_created", u64::from(created_garmin_s))?;
    Ok(m.build())
}

fn file_creator() -> Result<RawMessage, EncodeError> {
    let mut m = MessageBuilder::new("file_creator")?;
    m.uint("software_version", 1)?;
    Ok(m.build())
}

fn workout_message(workout: &Workout) -> Result<RawMessage, EncodeError> {
    let (sport, sub_sport) = workout.sport.names();
    let mut m = MessageBuilder::new("workout")?;
    m.uint("message_index", 0)?;
    m.string("wkt_name", cut(workout.name.trim(), NAME_MAX_BYTES))?;
    if let Some(note) = workout.note.as_deref().filter(|n| !n.is_empty()) {
        m.string("wkt_description", cut(note, NOTE_MAX_BYTES))?;
    }
    m.enum_name("sport", sport)?;
    m.enum_name("sub_sport", sub_sport)?;
    m.uint("capabilities", CAPABILITIES)?;
    m.uint("num_valid_steps", workout.step_count() as u64)?;
    Ok(m.build())
}

fn push_step(
    w: &mut FitWriter,
    step: &Step,
    index: &mut u16,
    sport: Sport,
) -> Result<(), EncodeError> {
    let mut m = MessageBuilder::new("workout_step")?;
    m.uint("message_index", u64::from(*index))?;
    m.enum_name("intensity", step.intensity.name())?;
    m.enum_name("weight_display_unit", "kilogram")?;
    let note = step.note.as_deref().filter(|n| !n.is_empty());
    if let Some(note) = note {
        m.string("notes", cut(note, STEP_NOTES_FIELD_BYTES))?;
    }
    duration(&mut m, step.duration)?;
    target(&mut m, step.target, sport)?;
    w.push(&m.build())?;
    if let Some(note) = note {
        w.push(&note_memo(note, *index)?)?;
    }
    *index += 1;
    Ok(())
}

/// The `memo_glob` with the notes of the step at `step_index`: the text,
/// NUL-terminated, in `data`. *inferred (Gadgetbridge, `noteMemo`)*
fn note_memo(note: &str, step_index: u16) -> Result<RawMessage, EncodeError> {
    let text = cut(note, NOTE_MAX_BYTES);
    let mut data = text.as_bytes().to_vec();
    data.push(0);
    // The messages are checked by `encode` (`available`) before anything is written; a
    // partial map that lacks them still answers Unavailable here.
    let (notes_field, _) = profile::message_number("workout_step")
        .and_then(|g| profile::field_by_name(g, "notes"))
        .ok_or(EncodeError::Unavailable)?;
    let mut m = MessageBuilder::new("memo_glob")?;
    m.uint("part_index", 0)?;
    m.enum_name("mesg_num", "workout_step")?;
    m.uint("parent_index", u64::from(step_index))?;
    m.uint("field_num", u64::from(notes_field))?;
    m.raw_elements("data", &data)?;
    Ok(m.build())
}

fn duration(m: &mut MessageBuilder, d: Duration) -> Result<(), EncodeError> {
    match d {
        Duration::Open => {
            m.enum_name("duration_type", "open")?;
        }
        Duration::Time { seconds } => {
            m.enum_name("duration_type", "time")?;
            m.sub_scaled("duration_value", "duration_time", f64::from(seconds))?;
        }
        Duration::Distance { meters } => {
            m.enum_name("duration_type", "distance")?;
            m.sub_scaled("duration_value", "duration_distance", f64::from(meters))?;
        }
        Duration::HeartRateBelow { bpm } => {
            m.enum_name("duration_type", "hr_less_than")?;
            m.uint("duration_value", u64::from(bpm) + HR_OFFSET)?;
        }
        Duration::HeartRateAbove { bpm } => {
            m.enum_name("duration_type", "hr_greater_than")?;
            m.uint("duration_value", u64::from(bpm) + HR_OFFSET)?;
        }
    }
    Ok(())
}

fn target(m: &mut MessageBuilder, t: Target, sport: Sport) -> Result<(), EncodeError> {
    match t {
        Target::None => {
            m.enum_name("target_type", "open")?;
            m.uint("target_value", 0)?;
        }
        Target::HeartRateZone { zone } => {
            m.enum_name("target_type", "heart_rate")?;
            m.uint("target_value", u64::from(zone))?;
        }
        Target::HeartRateRange { low_bpm, high_bpm } => {
            m.enum_name("target_type", "heart_rate")?;
            m.uint("custom_target_value_low", u64::from(low_bpm) + HR_OFFSET)?;
            m.uint("custom_target_value_high", u64::from(high_bpm) + HR_OFFSET)?;
        }
        Target::Pace {
            fast_s_per_km,
            slow_s_per_km,
        } => {
            // Speed in m/s; the slower pace is the low speed.
            m.enum_name("target_type", "speed")?;
            m.sub_scaled(
                "custom_target_value_low",
                "custom_target_speed_low",
                1000.0 / slow_s_per_km,
            )?;
            m.sub_scaled(
                "custom_target_value_high",
                "custom_target_speed_high",
                1000.0 / fast_s_per_km,
            )?;
        }
        Target::Speed { low_kmh, high_kmh } => {
            m.enum_name("target_type", "speed")?;
            m.sub_scaled(
                "custom_target_value_low",
                "custom_target_speed_low",
                low_kmh / 3.6,
            )?;
            m.sub_scaled(
                "custom_target_value_high",
                "custom_target_speed_high",
                high_kmh / 3.6,
            )?;
        }
        Target::Cadence { low, high } => {
            m.enum_name("target_type", "cadence")?;
            let (low, high) = if sport.halves_cadence() {
                (low / 2, high / 2)
            } else {
                (low, high)
            };
            m.uint("custom_target_value_low", u64::from(low))?;
            m.uint("custom_target_value_high", u64::from(high))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(intensity: Intensity, duration: Duration, target: Target) -> Step {
        Step {
            intensity,
            duration,
            target,
            note: None,
        }
    }

    fn sample() -> Workout {
        Workout {
            name: "Intervals".into(),
            sport: Sport::Running,
            note: Some("Easy start".into()),
            nodes: vec![
                Node::Step(step(
                    Intensity::Warmup,
                    Duration::Time { seconds: 600 },
                    Target::HeartRateZone { zone: 2 },
                )),
                Node::Repeat {
                    count: 4,
                    steps: vec![
                        step(
                            Intensity::Active,
                            Duration::Distance { meters: 400 },
                            Target::Pace {
                                fast_s_per_km: 240.0,
                                slow_s_per_km: 300.0,
                            },
                        ),
                        step(
                            Intensity::Recovery,
                            Duration::Time { seconds: 90 },
                            Target::None,
                        ),
                    ],
                },
                Node::Step(step(
                    Intensity::Cooldown,
                    Duration::Open,
                    Target::HeartRateRange {
                        low_bpm: 100,
                        high_bpm: 130,
                    },
                )),
            ],
        }
    }

    #[test]
    fn step_count_includes_the_repeat_step() {
        assert_eq!(sample().step_count(), 5);
    }

    #[test]
    fn validation_names_the_limit() {
        let mut w = sample();
        assert!(w.validate().is_ok());
        w.name = "  ".into();
        assert!(w.validate().is_err());
        let mut w = sample();
        w.nodes.clear();
        assert!(w.validate().is_err());
        let mut w = sample();
        w.nodes = vec![Node::Repeat {
            count: 1,
            steps: vec![step(Intensity::Active, Duration::Open, Target::None)],
        }];
        assert!(w.validate().unwrap_err().to_string().contains("repeat"));
        let mut w = sample();
        w.nodes = vec![Node::Repeat {
            count: 3,
            steps: vec![],
        }];
        assert!(w.validate().is_err());
        for bad in [
            Duration::Time { seconds: 0 },
            Duration::Time { seconds: 86_401 },
            Duration::Distance { meters: 0 },
            Duration::HeartRateAbove { bpm: 20 },
        ] {
            let mut w = sample();
            w.nodes = vec![Node::Step(step(Intensity::Active, bad, Target::None))];
            assert!(w.validate().is_err(), "{bad:?}");
        }
        for bad in [
            Target::HeartRateZone { zone: 0 },
            Target::HeartRateZone { zone: 6 },
            Target::HeartRateRange {
                low_bpm: 150,
                high_bpm: 140,
            },
            Target::Pace {
                fast_s_per_km: 300.0,
                slow_s_per_km: 240.0,
            },
            Target::Pace {
                fast_s_per_km: f64::NAN,
                slow_s_per_km: 240.0,
            },
            Target::Speed {
                low_kmh: 12.0,
                high_kmh: 10.0,
            },
            Target::Cadence { low: 0, high: 90 },
        ] {
            let mut w = sample();
            w.nodes = vec![Node::Step(step(Intensity::Active, Duration::Open, bad))];
            assert!(w.validate().is_err(), "{bad:?}");
        }
        let mut w = sample();
        w.nodes = (0..101)
            .map(|_| Node::Step(step(Intensity::Active, Duration::Open, Target::None)))
            .collect();
        assert!(w.validate().unwrap_err().to_string().contains("100"));
    }

    #[test]
    fn long_names_are_cut_at_a_character_boundary() {
        assert_eq!(cut("abc", 5), "abc");
        assert_eq!(cut("abcdef", 3), "abc");
        // "æ" is two bytes: a cut in the middle backs up.
        assert_eq!(cut("aæb", 2), "a");
        assert_eq!(cut("aæb", 3), "aæ");
    }

    #[test]
    fn the_workout_file_type_is_128_5() {
        assert_eq!((WORKOUT_DATA_TYPE, WORKOUT_SUB_TYPE), (128, 5));
    }

    /// The committed configuration has no profile: nothing is written.
    #[cfg(not(any(fit_profile, fit_map)))]
    #[test]
    fn without_a_profile_the_workout_is_unavailable_never_wrong_bytes() {
        assert_eq!(
            encode(&sample(), 977_000_000).unwrap_err(),
            EncodeError::Unavailable
        );
        // Even for a workout that would be invalid: availability is decided first.
        let mut w = sample();
        w.nodes.clear();
        assert_eq!(encode(&w, 1).unwrap_err(), EncodeError::Unavailable);
    }

    /// Nimrag's own map has every name the writer uses: the workout messages were
    /// observed in Garmin's workout files, the rest (weight unit, custom targets,
    /// open and heart-rate durations, `rest`, speed and cadence targets, walking)
    /// was settled by output comparison (O).
    #[cfg(fit_map)]
    #[test]
    fn with_the_partial_map_the_workout_is_written() {
        assert!(available());
        let file = encode(&sample(), 977_000_000).unwrap();
        assert_eq!(&file[8..12], b".FIT");
        let mut w = sample();
        w.nodes.clear();
        assert!(matches!(
            encode(&w, 1).unwrap_err(),
            EncodeError::Invalid(_)
        ));
        // the step-note memo is in the map
        assert!(note_memo("n", 0).is_ok());
    }

    #[cfg(fit_profile)]
    mod with_profile {
        use super::*;
        use crate::fit::interpret::{interpret, Interpreted};
        use crate::fit::{decode, Crc};

        fn round_trip(w: &Workout) -> Vec<Interpreted> {
            let file = encode(w, 977_000_000).expect("encodes");
            let d = decode(&file);
            assert_eq!(d.error, None);
            assert_eq!(d.files.len(), 1);
            assert_eq!(d.files[0].crc, Crc::Ok);
            assert!(d.files[0].header.header_crc_ok);
            interpret(&d.files[0])
        }

        fn named<'a>(msgs: &'a [Interpreted], name: &str) -> Vec<&'a Interpreted> {
            msgs.iter()
                .filter(|m| m.message_name == Some(name))
                .collect()
        }

        fn raw(file: &crate::fit::FitFile, global: u16, index: usize, num: u8) -> Option<Vec<u8>> {
            file.messages
                .iter()
                .filter(|m| m.global == global)
                .nth(index)?
                .fields
                .iter()
                .find(|f| f.num == num)
                .map(|f| f.raw.clone())
        }

        #[test]
        fn message_order_file_id_creator_workout_then_steps() {
            let msgs = round_trip(&sample());
            let order: Vec<_> = msgs.iter().map(|m| m.message_name.unwrap()).collect();
            assert_eq!(
                order,
                [
                    "file_id",
                    "file_creator",
                    "workout",
                    "workout_step",
                    "workout_step",
                    "workout_step",
                    "workout_step",
                    "workout_step"
                ]
            );
        }

        #[test]
        fn file_id_creator_and_workout_values() {
            let msgs = round_trip(&sample());
            let id = &msgs[0];
            assert_eq!(id.get("type").unwrap().enum_label, Some("workout"));
            assert_eq!(id.get("manufacturer").unwrap().enum_label, Some("garmin"));
            assert_eq!(id.number("garmin_product"), Some(65534.0));
            assert_eq!(id.number("serial_number"), Some(1.0));
            assert_eq!(id.number("number"), Some(1.0));
            assert_eq!(id.number("time_created"), Some(977_000_000.0));
            assert_eq!(msgs[1].number("software_version"), Some(1.0));
            let w = &msgs[2];
            assert_eq!(w.number("message_index"), Some(0.0));
            assert_eq!(w.get("sport").unwrap().enum_label, Some("running"));
            assert_eq!(w.get("sub_sport").unwrap().enum_label, Some("generic"));
            assert_eq!(w.number("num_valid_steps"), Some(5.0));
            assert_eq!(w.number("capabilities"), Some(32.0));
            match &w.get("wkt_name").unwrap().value {
                Some(crate::fit::interpret::Val::Text(t)) => assert_eq!(t, &["Intervals"]),
                other => panic!("name: {other:?}"),
            }
            match &w.get("wkt_description").unwrap().value {
                Some(crate::fit::interpret::Val::Text(t)) => assert_eq!(t, &["Easy start"]),
                other => panic!("description: {other:?}"),
            }
        }

        #[test]
        fn steps_carry_intensity_duration_and_target() {
            let msgs = round_trip(&sample());
            let steps = named(&msgs, "workout_step");
            // 0: warmup 10 min, heart-rate zone 2
            assert_eq!(steps[0].number("message_index"), Some(0.0));
            assert_eq!(
                steps[0].get("intensity").unwrap().enum_label,
                Some("warmup")
            );
            assert_eq!(
                steps[0].get("duration_type").unwrap().enum_label,
                Some("time")
            );
            assert_eq!(steps[0].number("duration_time"), Some(600.0));
            assert_eq!(
                steps[0].get("target_type").unwrap().enum_label,
                Some("heart_rate")
            );
            assert_eq!(steps[0].number("target_hr_zone"), Some(2.0));
            assert_eq!(
                steps[0].get("weight_display_unit").unwrap().enum_label,
                Some("kilogram")
            );
            // 1: 400 m at 4:00-5:00 per km
            assert_eq!(
                steps[1].get("intensity").unwrap().enum_label,
                Some("active")
            );
            assert_eq!(steps[1].number("duration_distance"), Some(400.0));
            assert_eq!(
                steps[1].get("target_type").unwrap().enum_label,
                Some("speed")
            );
            // 1000/300 = 3.333 m/s low, 1000/240 = 4.167 m/s high, in mm/s: 3333 and 4167
            assert_eq!(steps[1].number("custom_target_speed_low"), Some(3.333));
            assert_eq!(steps[1].number("custom_target_speed_high"), Some(4.167));
            // 2: recovery 90 s, open target with value 0
            assert_eq!(
                steps[2].get("intensity").unwrap().enum_label,
                Some("recovery")
            );
            assert_eq!(
                steps[2].get("target_type").unwrap().enum_label,
                Some("open")
            );
            assert_eq!(steps[2].number("target_value"), Some(0.0));
            // 4: cooldown, open duration (no value), heart-rate range 100-130 stored +100
            assert_eq!(
                steps[4].get("intensity").unwrap().enum_label,
                Some("cooldown")
            );
            assert_eq!(
                steps[4].get("duration_type").unwrap().enum_label,
                Some("open")
            );
            assert!(steps[4].get("duration_value").is_none());
        }

        #[test]
        fn the_repeat_step_points_back_and_counts_passes() {
            let msgs = round_trip(&sample());
            let steps = named(&msgs, "workout_step");
            let repeat = steps[3];
            assert_eq!(repeat.number("message_index"), Some(3.0));
            assert_eq!(
                repeat.get("duration_type").unwrap().enum_label,
                Some("repeat_until_steps_cmplt")
            );
            // The block starts at step 1 (the two steps before the repeat are 1 and 2).
            assert_eq!(repeat.number("duration_step"), Some(1.0));
            assert_eq!(repeat.number("repeat_steps"), Some(4.0));
            assert!(repeat.get("intensity").is_none());
            // Each step after the repeat continues the numbering.
            assert_eq!(steps[4].number("message_index"), Some(4.0));
        }

        #[test]
        fn heart_rate_values_are_stored_with_the_offset_of_100() {
            let file = encode(&sample(), 1).unwrap();
            let d = decode(&file);
            let f = &d.files[0];
            let (_, low) = profile::field_by_name(27, "custom_target_value_low").unwrap();
            let _ = low;
            // step 4 is the 5th workout_step (index 4): range 100..130 bpm -> 200..230
            assert_eq!(raw(f, 27, 4, 5), Some(200u32.to_le_bytes().to_vec()));
            assert_eq!(raw(f, 27, 4, 6), Some(230u32.to_le_bytes().to_vec()));
            let mut w = sample();
            w.nodes = vec![
                Node::Step(step(
                    Intensity::Active,
                    Duration::HeartRateBelow { bpm: 140 },
                    Target::None,
                )),
                Node::Step(step(
                    Intensity::Rest,
                    Duration::HeartRateAbove { bpm: 120 },
                    Target::None,
                )),
            ];
            let msgs = round_trip(&w);
            let steps = named(&msgs, "workout_step");
            assert_eq!(
                steps[0].get("duration_type").unwrap().enum_label,
                Some("hr_less_than")
            );
            assert_eq!(steps[1].get("intensity").unwrap().enum_label, Some("rest"));
            let file = encode(&w, 1).unwrap();
            let d = decode(&file);
            assert_eq!(
                raw(&d.files[0], 27, 0, 2),
                Some(240u32.to_le_bytes().to_vec())
            );
            assert_eq!(
                raw(&d.files[0], 27, 1, 2),
                Some(220u32.to_le_bytes().to_vec())
            );
        }

        #[test]
        fn cadence_is_halved_for_running_only() {
            for (sport, want) in [
                (Sport::Running, (80.0, 90.0)),
                (Sport::Cycling, (160.0, 180.0)),
            ] {
                let mut w = sample();
                w.sport = sport;
                w.nodes = vec![Node::Step(step(
                    Intensity::Active,
                    Duration::Open,
                    Target::Cadence {
                        low: 160,
                        high: 180,
                    },
                ))];
                let msgs = round_trip(&w);
                let s = named(&msgs, "workout_step")[0];
                assert_eq!(s.get("target_type").unwrap().enum_label, Some("cadence"));
                assert_eq!(
                    s.number("custom_target_cadence_low"),
                    Some(want.0),
                    "{sport:?}"
                );
                assert_eq!(
                    s.number("custom_target_cadence_high"),
                    Some(want.1),
                    "{sport:?}"
                );
            }
        }

        #[test]
        fn speed_targets_and_sports_map_to_profile_names() {
            for (sport, name, sub) in [
                (Sport::Cycling, "cycling", "generic"),
                (Sport::Walking, "walking", "generic"),
                (Sport::Hiking, "hiking", "generic"),
                (Sport::Strength, "training", "strength_training"),
                (Sport::Other, "generic", "generic"),
            ] {
                let mut w = sample();
                w.sport = sport;
                w.nodes = vec![Node::Step(step(
                    Intensity::Active,
                    Duration::Open,
                    Target::Speed {
                        low_kmh: 18.0,
                        high_kmh: 25.2,
                    },
                ))];
                let msgs = round_trip(&w);
                let wk = named(&msgs, "workout")[0];
                assert_eq!(wk.get("sport").unwrap().enum_label, Some(name));
                assert_eq!(wk.get("sub_sport").unwrap().enum_label, Some(sub));
                let s = named(&msgs, "workout_step")[0];
                assert_eq!(s.number("custom_target_speed_low"), Some(5.0));
                assert_eq!(s.number("custom_target_speed_high"), Some(7.0));
            }
        }

        #[test]
        fn a_step_note_is_in_the_step_and_in_a_memo_glob_right_after_it() {
            let mut w = sample();
            let note = "Keep the shoulders loose and breathe through the nose all the way";
            if let Node::Step(s) = &mut w.nodes[0] {
                s.note = Some(note.into());
            }
            let msgs = round_trip(&w);
            let order: Vec<_> = msgs.iter().map(|m| m.message_name.unwrap()).collect();
            assert_eq!(order[3], "workout_step");
            assert_eq!(order[4], "memo_glob");
            assert_eq!(order[5], "workout_step");
            let first = named(&msgs, "workout_step")[0];
            match &first.get("notes").unwrap().value {
                Some(crate::fit::interpret::Val::Text(t)) => {
                    assert_eq!(t[0], &note[..STEP_NOTES_FIELD_BYTES]);
                }
                other => panic!("notes: {other:?}"),
            }
            let memo = &msgs[4];
            assert_eq!(memo.number("part_index"), Some(0.0));
            assert_eq!(
                memo.get("mesg_num").unwrap().enum_label,
                Some("workout_step")
            );
            assert_eq!(memo.number("parent_index"), Some(0.0));
            assert_eq!(memo.number("field_num"), Some(8.0));
            // The memo's data is the whole note and a NUL, as uint8z elements.
            let file = encode(&w, 1).unwrap();
            let d = decode(&file);
            let data = raw(&d.files[0], 145, 0, 4).expect("memo data");
            let mut want = note.as_bytes().to_vec();
            want.push(0);
            assert_eq!(data, want);
            // The step after the note keeps counting.
            assert_eq!(
                named(&msgs, "workout_step")[1].number("message_index"),
                Some(1.0)
            );
        }

        #[test]
        fn a_long_name_is_cut_and_the_file_still_decodes() {
            let mut w = sample();
            w.name = "Ø".repeat(100);
            let msgs = round_trip(&w);
            match &named(&msgs, "workout")[0].get("wkt_name").unwrap().value {
                Some(crate::fit::interpret::Val::Text(t)) => {
                    assert!(t[0].len() <= NAME_MAX_BYTES);
                    assert!(t[0].chars().all(|c| c == 'Ø'));
                }
                other => panic!("name: {other:?}"),
            }
        }

        #[test]
        fn invalid_workouts_are_refused_with_the_profile_present() {
            let mut w = sample();
            w.nodes.clear();
            assert!(matches!(encode(&w, 1), Err(EncodeError::Invalid(_))));
        }

        #[test]
        fn the_file_is_stable_for_the_same_input() {
            assert_eq!(encode(&sample(), 5).unwrap(), encode(&sample(), 5).unwrap());
            assert_ne!(encode(&sample(), 5).unwrap(), encode(&sample(), 6).unwrap());
        }
    }
}
