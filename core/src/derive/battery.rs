//! Wellness parser: device battery level (104 `battery`).
//!
//! Ported from Gadgetbridge commit 7257742b (AGPL-3.0-or-later), rule in
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/fit/FitImporter.java`
//! (the `FitBattery` branch). Copyright (C) 2024-2026 José Rebelo, CaptKentish,
//! Daniele Gobbetti, Thomas Kuehne and the other Gadgetbridge contributors.
//!
//! Rule: a `battery` message with a UTC time and a `capacity` (percent) is one
//! level sample; Gadgetbridge records it as battery index 0 and ignores voltage,
//! current and temperature, as do we. The field is read by name through
//! `fit::interpret` (no Gadgetbridge tables). Seen in real Instinct 2 activity
//! files (message 104, fields `capacity`, `voltage`, `current`, `temperature`).
use super::{Battery, Derived, MsgExt};
use crate::fit::interpret::Interpreted;

const BATTERY: u16 = 104;

pub fn run(msgs: &[Interpreted], out: &mut Derived) {
    for m in msgs.iter().filter(|m| m.global == BATTERY) {
        let Some(percent) = m.number("capacity") else {
            continue;
        };
        match m.unix_ts() {
            Some(ts) => out.battery.push(Battery { ts, percent }),
            None => out.skipped += 1,
        }
    }
}
