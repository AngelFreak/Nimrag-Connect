//! The weather duty inside the session (stage 2f): the watch's 5014 becomes
//! `Event::WeatherRequest`; the host answers with `Input::SendWeather` (the FIT
//! definition and data records the server made) and the client sends them as
//! 5011 then 5012 and reports `Event::WeatherSent`.
//!
//! With the duty off the client behaves exactly as before: a 5014 gets the
//! "not supported" answer (status 2) and `Input::SendWeather` only reports
//! `WeatherResult::NotSupported`.
//!
//! The flow is ported from Gadgetbridge commit 7257742b (AGPL-3.0-or-later),
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/`
//! `FitLocalMessageHandler.java` (`init`, `sendFollowUp`, `handle`: the
//! definitions go first, the watch's status for them releases the first data
//! chunk, each data status releases the next chunk) and `GarminSupport.java`
//! (`sendWeatherConditions`: only when the watch has WEATHER_CONDITIONS;
//! Copyright (C) 2024-2026 Daniele Gobbetti, José Rebelo, kuhy, Thomas Kuehne
//! and the other Gadgetbridge contributors). *inferred (Gadgetbridge)*; see
//! `crate::duties::weather` for the layouts. What we do differently is marked
//! [ours]:
//!
//! - Gadgetbridge goes on with the data when the watch did not apply the
//!   definitions; so do we. It ignores the data status; we stop on a status
//!   other than "applied" and report `Refused`.
//! - A refusal (NAK) or silence ends this answer only, never the session
//!   (`Kind::soft`), and a result event always follows a `SendWeather`.
//! - Nothing is logged or put into an event but the result: the bytes carry a
//!   forecast for the owner's area and are never printed.

use std::collections::VecDeque;

use super::session::{Kind, Out, Requests};
use super::{AnomalyKind, Event};
use crate::duties::weather::{self as wx, WeatherResult};

/// The state of the one answer that may be on its way.
#[derive(Debug, Default)]
pub struct Weather {
    enabled: bool,
    /// 5012 payloads still to send. `Some` while an answer is in flight.
    chunks: Option<VecDeque<Vec<u8>>>,
}

impl Weather {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            chunks: None,
        }
    }

    /// An answer is queued or unanswered.
    pub fn busy(&self) -> bool {
        self.chunks.is_some()
    }

    /// `Input::SendWeather`. `watch_ready` is "the handshake is done and the
    /// watch has capability 26"; `budget` the payload bytes one 5011 or 5012
    /// may carry (0 when the watch's `max_packet` is unknown).
    pub fn send(
        &mut self,
        definitions: Vec<u8>,
        records: Vec<Vec<u8>>,
        watch_ready: bool,
        budget: usize,
        req: &mut Requests,
        out: &mut Out,
    ) {
        if !(self.enabled && watch_ready) {
            finish(out, WeatherResult::NotSupported);
            return;
        }
        if self.busy() {
            finish(out, WeatherResult::Busy);
            return;
        }
        let Ok(chunks) = wx::plan(&definitions, &records, budget) else {
            finish(out, WeatherResult::Invalid);
            return;
        };
        self.chunks = Some(chunks.into());
        req.send(
            out,
            wx::FIT_DEFINITION,
            definitions,
            Kind::WeatherDefinition,
        );
    }

    /// The watch answered 5011 or 5012. `ack` is "status byte was ACK", `code`
    /// the status code after it.
    pub fn on_answer(
        &mut self,
        kind: Kind,
        ack: bool,
        code: Option<u8>,
        req: &mut Requests,
        out: &mut Out,
    ) {
        let Some(chunks) = self.chunks.as_mut() else {
            return;
        };
        if !ack {
            self.chunks = None;
            out.anomaly(AnomalyKind::DutyMessageRefused, message_of(kind));
            finish(out, WeatherResult::Refused);
            return;
        }
        // A definition the watch did not apply: Gadgetbridge sends the data
        // anyway. A data status other than "applied" ends the answer. [ours]
        if kind == Kind::WeatherData && code != Some(wx::CODE_APPLIED) {
            self.chunks = None;
            out.anomaly(AnomalyKind::DutyMessageRefused, wx::FIT_DATA);
            finish(out, WeatherResult::Refused);
            return;
        }
        match chunks.pop_front() {
            Some(next) => req.send(out, wx::FIT_DATA, next, Kind::WeatherData),
            None => {
                self.chunks = None;
                finish(out, WeatherResult::Sent);
            }
        }
    }

    /// A 5011 or 5012 got no answer in time and was dropped.
    pub fn on_timeout(&mut self, out: &mut Out) {
        if self.chunks.take().is_some() {
            finish(out, WeatherResult::TimedOut);
        }
    }
}

fn message_of(kind: Kind) -> u16 {
    if kind == Kind::WeatherDefinition {
        wx::FIT_DEFINITION
    } else {
        wx::FIT_DATA
    }
}

fn finish(out: &mut Out, result: WeatherResult) {
    out.events.push(Event::WeatherSent { result });
}
