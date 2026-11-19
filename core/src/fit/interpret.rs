//! Interpretation layer: names, scale/offset, units, subfields, components and
//! timestamp resolution over decoded messages, using `profile`.
//!
//! Pure and std-only. It never changes decoded data: raw values stay in
//! `FitFile`; this layer derives a parallel, named view.
//!
//! Rules:
//! - `value = raw / scale - offset` (scale/offset default 1 / 0). Invalid raw
//!   values give `None`; nothing is substituted.
//! - A subfield replaces the field's name/scale/offset/units/type when any of
//!   its `(reference field, value)` pairs matches a valid raw value in the same
//!   message (first matching subfield wins; `source` = `Subfield`).
//! - Components expand a packed field (LSB-first bit stream over the field's
//!   little-endian bytes) into extra values with `source` = `Component`. The
//!   packed field itself is still emitted. Components of an invalid source are
//!   skipped.
//! - Accumulate: per (message global, target field) running value, extended on
//!   `bits`-wide rollover. The state is seeded whenever the target field appears
//!   directly (profile `accumulate` flag). State resets per file.
//! - Timestamps: `timestamp_16` (monitoring field 26, built in by number) is
//!   extended against the last full timestamp and emitted as a `Component`
//!   value for field 253. `timestamp_min_8` is NOT resolved (semantics
//!   unconfirmed, see `relative_timestamp`). Profile component targets of field 253 accumulate
//!   against the last full timestamp; with no reference the component value is
//!   `None`. Timestamps below 0x10000000 are device-relative: they give
//!   `Interpreted::timestamp = None`, are never converted to UTC, and are never
//!   used as the reference for relative timestamps (so no device-relative
//!   field-253 `Component` value is ever emitted).
//! - Components are expanded one level: a component's target is not itself
//!   expanded again.
//! - Subfield selection sees both the message's direct raw values and the raw
//!   bits of non-accumulating components of its other fields (e.g. activity
//!   type packed in `current_activity_type_intensity`).
//! - Lookup helpers (`Interpreted::get`/`number`/`numbers`) prefer
//!   Direct/Subfield, then Component, then Developer values; within a rank the
//!   first in field order wins. Stage 1d parsers should use them.
//! - Unnamed fields keep `name: None` and their raw-derived, unscaled value.
//! - Developer fields are included, named/scaled from their description.
//! - When `profile::available()` is false there are no names, scales, units,
//!   subfields or components: every value is raw and unscaled.
use std::collections::HashMap;

use super::{profile, DeveloperFieldDescription, FitFile, Message, Value};

/// Timestamps below this are device-relative, not UTC seconds since the FIT epoch.
pub const FIT_UTC_MIN: u32 = 0x1000_0000;
const FIELD_TIMESTAMP: u8 = 253;

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Number(f64),
    /// Array field; invalid elements are `None`.
    Numbers(Vec<Option<f64>>),
    /// Zero or more NUL-separated strings, as decoded.
    Text(Vec<String>),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Direct,
    Subfield,
    Component,
    Developer,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NamedValue {
    /// Field number (the target field's number for components).
    pub num: u8,
    pub name: Option<&'static str>,
    /// Name of the developer field, when `source` is `Developer`.
    pub dev_name: Option<String>,
    /// None when the raw value is invalid (or unresolvable).
    pub value: Option<Val>,
    pub units: Option<String>,
    pub source: Source,
    /// Enum label for a scalar value of an enum profile type, when known.
    pub enum_label: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Interpreted {
    pub global: u16,
    pub index: u32,
    /// UTC seconds since the FIT epoch; None if absent or device-relative.
    pub timestamp: Option<u32>,
    pub message_name: Option<&'static str>,
    pub values: Vec<NamedValue>,
}

impl Interpreted {
    fn rank(v: &NamedValue) -> u8 {
        match v.source {
            Source::Direct | Source::Subfield => 0,
            Source::Component => 1,
            Source::Developer => 2,
        }
    }

    /// Values called `name`, best first: Direct/Subfield, then Component, then
    /// Developer (matched by developer name); field order within a rank.
    fn candidates<'a>(&'a self, name: &str) -> Vec<&'a NamedValue> {
        let mut c: Vec<&NamedValue> = self
            .values
            .iter()
            .filter(|v| match v.source {
                Source::Developer => v.dev_name.as_deref() == Some(name),
                _ => v.name == Some(name),
            })
            .collect();
        c.sort_by_key(|v| Self::rank(v)); // stable: keeps field order
        c
    }

    /// Highest-precedence value with this name, valid or not.
    pub fn get<'a>(&'a self, name: &str) -> Option<&'a NamedValue> {
        self.candidates(name).into_iter().next()
    }

    /// Highest-precedence scalar number with this name (skips invalid and
    /// non-scalar entries, so a valid component can back an invalid direct one).
    pub fn number(&self, name: &str) -> Option<f64> {
        self.candidates(name)
            .into_iter()
            .find_map(|v| match v.value {
                Some(Val::Number(x)) => Some(x),
                _ => None,
            })
    }

    /// Highest-precedence array with this name.
    pub fn numbers<'a>(&'a self, name: &str) -> Option<&'a [Option<f64>]> {
        self.candidates(name)
            .into_iter()
            .find_map(|v| match &v.value {
                Some(Val::Numbers(x)) => Some(x.as_slice()),
                _ => None,
            })
    }
}

#[derive(Default)]
struct State {
    last_full: Option<u32>,
    accum: HashMap<(u16, u8), u64>,
}

/// Interpret every message of a file, in file order.
pub fn interpret(file: &FitFile) -> Vec<Interpreted> {
    let mut st = State::default();
    file.messages
        .iter()
        .map(|m| interpret_message(m, &file.developer_fields, &mut st))
        .collect()
}

fn first_uint(v: &Value) -> Option<u64> {
    match v {
        Value::Uint(v) => v.first().copied().flatten(),
        _ => None,
    }
}

fn convert(x: f64, scale: Option<f64>, offset: Option<f64>) -> f64 {
    let s = scale.unwrap_or(1.0);
    let s = if s == 0.0 { 1.0 } else { s };
    x / s - offset.unwrap_or(0.0)
}

fn to_val(v: &Value, scale: Option<f64>, offset: Option<f64>) -> Option<Val> {
    let one = |len: usize, x: Option<f64>| -> Option<Val> {
        if len == 1 {
            x.map(|x| Val::Number(convert(x, scale, offset)))
        } else {
            None
        }
    };
    match v {
        Value::Uint(v) if v.len() == 1 => one(1, v[0].map(|x| x as f64)),
        Value::Sint(v) if v.len() == 1 => one(1, v[0].map(|x| x as f64)),
        Value::Float(v) if v.len() == 1 => one(1, v[0]),
        Value::Uint(v) => Some(Val::Numbers(
            v.iter()
                .map(|x| x.map(|x| convert(x as f64, scale, offset)))
                .collect(),
        )),
        Value::Sint(v) => Some(Val::Numbers(
            v.iter()
                .map(|x| x.map(|x| convert(x as f64, scale, offset)))
                .collect(),
        )),
        Value::Float(v) => Some(Val::Numbers(
            v.iter()
                .map(|x| x.map(|x| convert(x, scale, offset)))
                .collect(),
        )),
        Value::String(s) => Some(Val::Text(s.clone())),
        Value::Bytes(b) => Some(Val::Bytes(b.clone())),
    }
}

/// Little-endian byte stream of a numeric value, for component extraction.
fn bit_source(v: &Value, raw: &[u8]) -> Option<Vec<u8>> {
    let elems = |vals: Vec<u64>| -> Option<Vec<u8>> {
        let size = raw.len().checked_div(vals.len())?;
        Some(
            vals.iter()
                .flat_map(|x| x.to_le_bytes()[..size.min(8)].to_vec())
                .collect(),
        )
    };
    match v {
        Value::Uint(v) => elems(v.iter().copied().collect::<Option<Vec<u64>>>()?),
        Value::Sint(v) => elems(
            v.iter()
                .map(|x| x.map(|x| x as u64))
                .collect::<Option<Vec<u64>>>()?,
        ),
        Value::Bytes(b) if !b.is_empty() && !b.iter().all(|&x| x == 0xFF) => Some(b.clone()),
        _ => None,
    }
}

fn read_bits(buf: &[u8], pos: usize, bits: usize) -> Option<u64> {
    if bits == 0 || bits > 64 || pos + bits > buf.len() * 8 {
        return None;
    }
    let mut v = 0u64;
    for i in 0..bits {
        let p = pos + i;
        v |= (((buf[p / 8] >> (p % 8)) & 1) as u64) << i;
    }
    Some(v)
}

fn mask(bits: usize) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

fn interpret_message(
    m: &Message,
    dev_descs: &[DeveloperFieldDescription],
    st: &mut State,
) -> Interpreted {
    let mut refs: Vec<(u8, u64)> = m
        .fields
        .iter()
        .filter_map(|f| first_uint(&f.value).map(|v| (f.num, v)))
        .collect();
    // Raw bits of non-accumulating components also count as reference values.
    for f in &m.fields {
        let Some(info) = profile::field(m.global, f.num) else {
            continue;
        };
        let Some(buf) = bit_source(&f.value, &f.raw) else {
            continue;
        };
        let mut pos = 0usize;
        for c in info.components {
            let bits = c.bits as usize;
            let Some(r) = read_bits(&buf, pos, bits) else {
                break;
            };
            pos += bits;
            if !c.accumulate {
                refs.push((c.field_num, r));
            }
        }
    }
    let mut values = Vec::new();
    let mut resolved_ts: Option<u32> = None;

    for f in &m.fields {
        let info = profile::field(m.global, f.num);
        let sub = info.and_then(|i| {
            i.subfields
                .iter()
                .find(|s| s.when.iter().any(|w| refs.contains(w)))
        });
        let (name, scale, offset, units, type_name, components, source) = match (info, sub) {
            (_, Some(s)) => (
                Some(s.name),
                s.scale,
                s.offset,
                s.units,
                Some(s.type_name),
                s.components,
                Source::Subfield,
            ),
            (Some(i), None) => (
                Some(i.name),
                i.scale,
                i.offset,
                i.units,
                Some(i.type_name),
                i.components,
                Source::Direct,
            ),
            (None, None) => (None, None, None, None, None, &[][..], Source::Direct),
        };
        let raw_uint = first_uint(&f.value);
        if info.is_some_and(|i| i.accumulate) {
            if let Some(v) = raw_uint {
                st.accum.insert((m.global, f.num), v);
            }
        }
        let scalar = matches!(&f.value, Value::Uint(v) if v.len() == 1);
        let enum_label = match (type_name, raw_uint, scalar, scale) {
            (Some(t), Some(v), true, None) => profile::enum_name(t, v),
            _ => None,
        };
        values.push(NamedValue {
            num: f.num,
            name,
            dev_name: None,
            value: to_val(&f.value, scale, offset),
            units: units.map(str::to_string),
            source,
            enum_label,
        });

        if let Some(bits) = relative_timestamp(m.global, f.num) {
            let ts = raw_uint.and_then(|r| resolve_relative(st, r, bits));
            if let Some(t) = ts {
                resolved_ts = Some(t);
            }
            values.push(NamedValue {
                num: FIELD_TIMESTAMP,
                name: profile::field(m.global, FIELD_TIMESTAMP).map(|t| t.name),
                dev_name: None,
                value: ts.map(|t| Val::Number(f64::from(t))),
                units: Some("s".to_string()),
                source: Source::Component,
                enum_label: None,
            });
        }
        if let Some(buf) = bit_source(&f.value, &f.raw) {
            let mut pos = 0usize;
            for c in components {
                let bits = c.bits as usize;
                let Some(raw) = read_bits(&buf, pos, bits) else {
                    break;
                };
                pos += bits;
                let target = profile::field(m.global, c.field_num);
                let counted = expand(st, m.global, c, raw, bits);
                if c.field_num == FIELD_TIMESTAMP {
                    if let Some(t) = counted {
                        resolved_ts = Some(t as u32);
                    }
                }
                values.push(NamedValue {
                    num: c.field_num,
                    name: target.map(|t| t.name),
                    dev_name: None,
                    value: counted.map(|x| Val::Number(convert(x as f64, c.scale, c.offset))),
                    units: c.units.or(target.and_then(|t| t.units)).map(str::to_string),
                    source: Source::Component,
                    enum_label: None,
                });
            }
        }
    }

    for d in &m.developer {
        let desc = dev_descs.iter().rev().find(|x| {
            x.developer_data_index == d.developer_data_index && x.field_definition_number == d.num
        });
        values.push(NamedValue {
            num: d.num,
            name: None,
            dev_name: desc.and_then(|x| x.name.clone()),
            value: to_val(
                &d.value,
                desc.and_then(|x| x.scale),
                desc.and_then(|x| x.offset),
            ),
            units: desc.and_then(|x| x.units.clone()),
            source: Source::Developer,
            enum_label: None,
        });
    }

    let own = m.timestamp();
    if let Some(t) = own.filter(|&t| t >= FIT_UTC_MIN) {
        st.last_full = Some(t);
    }
    let timestamp = own.or(resolved_ts).filter(|&t| t >= FIT_UTC_MIN);
    Interpreted {
        global: m.global,
        index: m.index,
        timestamp,
        message_name: profile::message_name(m.global),
        values,
    }
}

const GLOBAL_MONITORING: u16 = 55;
const FIELD_TIMESTAMP_16: u8 = 26;

/// Relative-timestamp fields the profile does not express as components:
/// bit width. Only monitoring `timestamp_16`. `timestamp_min_8` (monitoring
/// field 25) is deliberately NOT resolved: it exists only as a plain uint8
/// "min" field in the app's profile and the Gadgetbridge profile JSON, with no
/// component, and the FIT SDK (21.222.0) does not define it. Nothing in
/// private/re shows how its minutes relate to the last full timestamp, so a
/// resolved time would be a guess; it stays a raw `Direct` value and yields no
/// timestamp.
fn relative_timestamp(global: u16, num: u8) -> Option<usize> {
    (global == GLOBAL_MONITORING && num == FIELD_TIMESTAMP_16).then_some(16)
}

/// Extend a 16-bit timestamp against the last full timestamp; the result
/// becomes the new reference.
fn resolve_relative(st: &mut State, raw: u64, bits: usize) -> Option<u32> {
    let last = u64::from(st.last_full?);
    let ts = u32::try_from(last + (raw.wrapping_sub(last & mask(bits)) & mask(bits))).ok()?;
    st.last_full = Some(ts);
    Some(ts)
}

/// Resolve one component's raw bits to its (unscaled) value, updating state.
fn expand(
    st: &mut State,
    global: u16,
    c: &profile::Component,
    raw: u64,
    bits: usize,
) -> Option<u64> {
    if c.field_num == FIELD_TIMESTAMP && c.accumulate {
        let last = u64::from(st.last_full?);
        let ts = last + (raw.wrapping_sub(last & mask(bits)) & mask(bits));
        let ts = u32::try_from(ts).ok()?;
        st.last_full = Some(ts);
        return Some(u64::from(ts));
    }
    if !c.accumulate {
        return Some(raw);
    }
    let key = (global, c.field_num);
    let prev = st.accum.get(&key).copied().unwrap_or(0);
    let next = prev.wrapping_add(raw.wrapping_sub(prev & mask(bits)) & mask(bits));
    st.accum.insert(key, next);
    Some(next)
}
