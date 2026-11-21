//! GFDI session: handshake, time duty, answers to watch requests, and the
//! request bookkeeping (one outstanding request, txn counter, 30 s deadline).
//! The file sync cycle lives in `sync.rs`.
//!
//! The first-connection events (`ClientConfig::first_connect`) are ported
//! from Gadgetbridge commit 7257742b (AGPL-3.0-or-later),
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/GarminSupport.java`
//! (`completeInitialization`, `connectFirstTime`). Copyright (C) 2024-2026
//! Daniele Gobbetti, José Rebelo, kuhy, Thomas Kuehne and the other
//! Gadgetbridge contributors. See docs/PORTED.md.
//!
//! The handling of FileAvailable (5009), BatteryStatus (5023) and the
//! device_status battery request and answer is ported from the same commit:
//! `.../garmin/messages/FileAvailableMessage.java`,
//! `.../garmin/messages/BatteryStatusMessage.java` (both Copyright (C) 2026
//! Thomas Kuehne and the other Gadgetbridge contributors),
//! `.../garmin/FileTransferHandler.java` (`handle`; Copyright (C) 2024-2026
//! Daniele Gobbetti, José Rebelo, Thomas Kuehne and the other Gadgetbridge
//! contributors), `.../garmin/ProtocolBufferHandler.java`
//! (`processProtobufDeviceStatusResponse`; no copyright line in the file;
//! Gadgetbridge contributors) and `.../garmin/GarminSupport.java`
//! (`requestBatteryUpdate`; Copyright (C) 2024-2026 Daniele Gobbetti, José
//! Rebelo, kuhy, Thomas Kuehne and the other Gadgetbridge contributors).
//! Those behaviours are *inferred (Gadgetbridge)*; what we do differently is
//! marked [ours].
//!
//! The companion duties of stage 2f (notifications, find my phone, music) are in
//! `companion.rs`; this file routes the watch's messages and the host's inputs
//! to it when the duty is on, and treats the messages we send for them as soft
//! requests (see `Kind::soft`).

use std::collections::VecDeque;

use super::companion::{Companion, Cx, MusicInfo, MusicState};
use super::sync::{Ctx, SyncCycle};
use super::transport::REPLY_DEADLINE_MS;
use super::upload::{self, Uploads};
use super::weather::Weather;
use super::{
    AnomalyKind, BatteryReport, ClientConfig, CloseReason, DeviceInfo, Duties, Event, FileType,
    FindWatchState, Now, Phase, GARMIN_EPOCH_UNIX_S,
};
use crate::duties::weather as wx;
use crate::duties::{find, music, notification as notif};
use crate::gfdi::{Message, Status, RESPONSE};
use crate::messages as m;

/// Wall clocks before this (2020-01-01T00:00Z) are treated as unset.
const MIN_VALID_UNIX_S: i64 = 1_577_836_800;

/// Status 7 of a 5023: the watch marks the reading as invalid.
const BATTERY_STATUS_INVALID: u8 = 7;

/// "Unknown or not supported" status for requests we do not implement.
const STATUS_UNSUPPORTED: u8 = 2;

/// System events of a first connection, in Gadgetbridge's order
/// (`completeInitialization`; PAIR_START and TUTORIAL_COMPLETE are commented
/// out there). *inferred (Gadgetbridge)*
const FIRST_CONNECT_EVENTS: [u8; 3] = [
    m::EVENT_PAIR_COMPLETE,
    m::EVENT_SYNC_COMPLETE,
    m::EVENT_SETUP_WIZARD_COMPLETE,
];

/// What a request is for, so its answer reaches the right handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Config,
    FileTypes,
    HandshakeComplete,
    /// A first-connection system event (5030); carries the event number.
    FirstConnect(u8),
    SyncNow,
    Filter,
    Download,
    Archive,
    SyncComplete,
    /// The protobuf battery request (5043, device_status 8).
    BatteryRequest,
    /// 5033 (stage 2f).
    NotificationUpdate,
    /// One 5035 chunk (stage 2f).
    NotificationData,
    /// 5049 with the track (stage 2f).
    MusicInfo,
    /// 5049 with the player state (stage 2f).
    MusicState,
    /// 5049 with the volume (stage 2f).
    MusicVolume,
    /// The protobuf find-my-watch request (5043, find_my_watch 12).
    FindWatch,
    /// 5011 with the FIT definition records of a weather answer (stage 2f).
    WeatherDefinition,
    /// One 5012 with FIT data records of a weather answer.
    WeatherData,
    /// 5005 CreateFile of an upload (stage 4a).
    UploadCreate,
    /// 5003 UploadFile of an upload.
    UploadRequest,
    /// One 5004 chunk of an upload.
    UploadData,
    /// The 5030 SYNC_COMPLETE that closes an upload.
    UploadComplete,
}

impl Kind {
    /// Requests whose failure must not end the session: a refusal or 30 s of
    /// silence drops the request and reports this anomaly. [ours]
    pub fn soft(self) -> Option<AnomalyKind> {
        match self {
            Kind::BatteryRequest => Some(AnomalyKind::BatteryRequestRefused),
            Kind::NotificationUpdate
            | Kind::NotificationData
            | Kind::MusicInfo
            | Kind::MusicState
            | Kind::MusicVolume
            | Kind::FindWatch
            | Kind::WeatherDefinition
            | Kind::WeatherData => Some(AnomalyKind::DutyMessageRefused),
            Kind::UploadCreate | Kind::UploadRequest | Kind::UploadData | Kind::UploadComplete => {
                Some(AnomalyKind::UploadFailed)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Pending {
    pub id: u16,
    pub txn: u8,
    pub kind: Kind,
}

#[derive(Debug)]
struct Queued {
    id: u16,
    payload: Vec<u8>,
    kind: Kind,
}

/// What one call produced, to be written and reported by the client.
#[derive(Debug, Default)]
pub struct Out {
    pub msgs: Vec<Message>,
    pub events: Vec<Event>,
    pub closed: Option<CloseReason>,
}

impl Out {
    pub fn anomaly(&mut self, kind: AnomalyKind, message_id: u16) {
        self.events.push(Event::Anomaly { kind, message_id });
    }

    pub fn close(&mut self, reason: CloseReason) {
        if self.closed.is_none() {
            self.closed = Some(reason);
        }
    }
}

/// Our outgoing requests: compact txn ids 0..31, one outstanding at a time.
#[derive(Debug, Default)]
pub struct Requests {
    next_txn: u8,
    next_protobuf_id: u16,
    pending: Option<Pending>,
    queue: VecDeque<Queued>,
}

impl Requests {
    pub fn send(&mut self, out: &mut Out, id: u16, payload: Vec<u8>, kind: Kind) {
        self.queue.push_back(Queued { id, payload, kind });
        self.pump(out);
    }

    /// Like `send`, but a request of the same kind that still waits in the
    /// queue is replaced instead of followed (a newer now-playing state makes
    /// the older one pointless). One already on the wire is not touched. [ours]
    pub fn send_replacing(&mut self, out: &mut Out, id: u16, payload: Vec<u8>, kind: Kind) {
        match self.queue.iter_mut().find(|q| q.kind == kind) {
            Some(queued) => queued.payload = payload,
            None => self.send(out, id, payload, kind),
        }
    }

    /// Send the next queued request when none is outstanding.
    pub fn pump(&mut self, out: &mut Out) {
        if self.pending.is_some() {
            return;
        }
        let Some(q) = self.queue.pop_front() else {
            return;
        };
        let txn = self.next_txn;
        self.next_txn = (self.next_txn + 1) & 0x1F;
        self.pending = Some(Pending {
            id: q.id,
            txn,
            kind: q.kind,
        });
        out.msgs.push(Message {
            id: q.id,
            txn: Some(txn),
            payload: q.payload,
        });
    }

    pub fn next_protobuf_id(&mut self) -> u16 {
        let id = self.next_protobuf_id;
        self.next_protobuf_id = self.next_protobuf_id.wrapping_add(1);
        id
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// A request of this kind is outstanding or queued.
    pub fn has(&self, kind: Kind) -> bool {
        self.pending.is_some_and(|p| p.kind == kind) || self.queue.iter().any(|q| q.kind == kind)
    }
}

/// A 5000 response to a watch request, with the request's txn if it had one.
pub fn response(to: &Message, status: u8, body: &[u8]) -> Message {
    let mut payload = to.id.to_le_bytes().to_vec();
    payload.push(status);
    payload.extend_from_slice(body);
    Message {
        id: RESPONSE,
        txn: to.txn,
        payload,
    }
}

fn status_byte(s: Status) -> u8 {
    match s {
        Status::Ack => 0,
        Status::Nak => 1,
        Status::UnknownOrNotSupported => 2,
        Status::CobsDecoderError => 3,
        Status::CrcError => 4,
        Status::LengthError => 5,
        Status::Other(b) => b,
    }
}

#[derive(Debug)]
pub struct Session {
    host: m::HostInfo,
    capabilities: Vec<u8>,
    first_connect: bool,
    /// Every first-connection event answered so far was an ACK.
    first_connect_acked: bool,
    device: Option<DeviceInfo>,
    watch_capabilities: Option<Vec<u8>>,
    file_types: Vec<FileType>,
    handshake_started: bool,
    ready: bool,
    /// `Input::RequestBattery` arrived before the handshake was done: send
    /// one request right after it.
    battery_requested: bool,
    duties: Duties,
    companion: Companion,
    weather: Weather,
    req: Requests,
    sync: SyncCycle,
    uploads: Uploads,
    /// A sync trigger was held back because an upload was queued or running;
    /// start it when the uploads are done.
    sync_held: bool,
    deadline: Option<u64>,
}

impl Session {
    pub fn new(config: &ClientConfig) -> Self {
        Self {
            host: m::HostInfo {
                protocol: m::PROTOCOL_VERSION,
                app_version: config.app_version,
                name: config.host_name.clone(),
                manufacturer: config.host_manufacturer.clone(),
                model: config.host_model.clone(),
            },
            capabilities: config.capabilities.clone(),
            first_connect: config.first_connect,
            first_connect_acked: true,
            device: None,
            watch_capabilities: None,
            file_types: Vec::new(),
            handshake_started: false,
            ready: false,
            battery_requested: false,
            duties: config.duties,
            companion: Companion::new(config.duties),
            weather: Weather::new(config.duties.weather),
            req: Requests::default(),
            sync: SyncCycle::new(config),
            uploads: Uploads::new(config.connection_epoch),
            sync_held: false,
            deadline: None,
        }
    }

    /// The link is up: wait for the watch's DeviceInformation.
    pub fn start(&mut self, now: &Now) {
        self.deadline = Some(now.mono_ms.saturating_add(REPLY_DEADLINE_MS));
    }

    /// Handshake done, no request outstanding or queued, no sync cycle.
    pub fn is_idle(&self) -> bool {
        self.ready
            && !self.req.is_pending()
            && self.req.queue.is_empty()
            && !self.sync.running()
            && !self.uploads.busy()
    }

    /// Whether a sync trigger may start now. While an upload is queued or
    /// running it may not (the two would interleave on the one request line);
    /// the trigger is kept and started afterwards. [ours]
    fn sync_ready(&mut self) -> bool {
        let held = self.ready && self.uploads.busy();
        if held {
            self.sync_held = true;
        }
        self.ready && !held
    }

    /// `Input::UploadFile`.
    pub fn upload_file(
        &mut self,
        now: &Now,
        upload_id: u64,
        data_type: u8,
        sub_type: u8,
        data: Vec<u8>,
    ) -> Out {
        let mut out = Out::default();
        self.uploads
            .enqueue(upload_id, data_type, sub_type, data, &mut out);
        self.finish(now, &mut out, false);
        out
    }

    /// Ids of the uploads the closing link cut short.
    pub fn take_unfinished_uploads(&mut self) -> Vec<u64> {
        self.uploads.take_unfinished()
    }

    /// Start the next upload when the line is free, and the sync trigger that
    /// waited for the uploads once they are done.
    fn pump_uploads(&mut self, now: &Now, out: &mut Out) {
        if out.closed.is_some() {
            return;
        }
        let max_packet = self.device.as_ref().map_or(0, |d| d.max_packet_size);
        let may_start = self.ready && self.sync.is_idle();
        let mut cx = upload::Cx {
            now,
            req: &mut self.req,
            out,
            max_packet,
        };
        self.uploads.pump(may_start, &mut cx);
        if self.sync_held && self.ready && !self.uploads.busy() {
            self.sync_held = false;
            self.sync.on_ready(&mut Ctx {
                now: now.mono_ms,
                req: &mut self.req,
                out,
            });
        }
    }

    pub fn phase(&self) -> Phase {
        if !self.ready {
            Phase::Handshake
        } else if self.sync.running() {
            Phase::Syncing
        } else {
            Phase::Ready
        }
    }

    pub fn next_wake(&self) -> Option<u64> {
        match (self.deadline, self.sync.next_wake()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    pub fn on_tick(&mut self, now: &Now) -> Out {
        let mut out = Out::default();
        if self.deadline.is_some_and(|d| now.mono_ms >= d) {
            self.deadline = None;
            let soft = self
                .req
                .pending
                .and_then(|p| p.kind.soft().map(|anomaly| (p, anomaly)));
            if let (Some((p, anomaly)), false) = (soft, self.sync.awaiting_chunks()) {
                // A silent battery request (or duty message) must not end the
                // session: drop it and go on; a late answer is then
                // unexpected. [ours]
                self.req.pending = None;
                out.anomaly(anomaly, p.id);
                self.soft_dropped(now, p.kind, &mut out);
            } else {
                out.close(CloseReason::Timeout {
                    phase: self.phase(),
                });
                return out;
            }
        }
        self.sync.on_tick(&mut Ctx {
            now: now.mono_ms,
            req: &mut self.req,
            out: &mut out,
        });
        self.finish(now, &mut out, false);
        out
    }

    pub fn start_sync(&mut self, now: &Now) -> Out {
        let mut out = Out::default();
        let ready = self.sync_ready();
        self.sync.start_manual(
            ready,
            &mut Ctx {
                now: now.mono_ms,
                req: &mut self.req,
                out: &mut out,
            },
        );
        self.finish(now, &mut out, false);
        out
    }

    /// `Input::RequestBattery`. Before the handshake is done the request is
    /// remembered (once) and goes out after it; afterwards it is queued
    /// unless one is already queued or unanswered.
    pub fn request_battery(&mut self, now: &Now) -> Out {
        let mut out = Out::default();
        if self.ready {
            self.send_battery_request(&mut out);
        } else {
            self.battery_requested = true;
        }
        self.finish(now, &mut out, false);
        out
    }

    fn send_battery_request(&mut self, out: &mut Out) {
        self.battery_requested = false;
        if self.req.has(Kind::BatteryRequest) {
            return;
        }
        let id = self.req.next_protobuf_id();
        self.req.send(
            out,
            m::PROTOBUF_REQUEST,
            m::build_protobuf(id, &m::BATTERY_REQUEST_PROTOBUF),
            Kind::BatteryRequest,
        );
    }

    /// A soft request got no answer in time and was dropped.
    fn soft_dropped(&mut self, now: &Now, kind: Kind, out: &mut Out) {
        match kind {
            Kind::NotificationData => {
                let mut cx = Cx {
                    now,
                    req: &mut self.req,
                    out,
                };
                self.companion.drop_transfer(&mut cx);
            }
            Kind::FindWatch => out.events.push(Event::FindWatch {
                state: FindWatchState::Failed,
            }),
            Kind::WeatherDefinition | Kind::WeatherData => self.weather.on_timeout(out),
            Kind::UploadCreate | Kind::UploadRequest | Kind::UploadData => {
                let max_packet = self.device.as_ref().map_or(0, |d| d.max_packet_size);
                let mut cx = upload::Cx {
                    now,
                    req: &mut self.req,
                    out,
                    max_packet,
                };
                self.uploads.on_timeout(kind, &mut cx);
            }
            _ => {}
        }
    }

    /// `Input::PostNotification`; ignored before the handshake is done.
    pub fn post_notification(&mut self, now: &Now, n: notif::PhoneNotification) -> Out {
        let mut out = Out::default();
        if self.ready {
            let mut cx = Cx {
                now,
                req: &mut self.req,
                out: &mut out,
            };
            self.companion.post(n, &mut cx);
        }
        self.finish(now, &mut out, false);
        out
    }

    /// `Input::RemoveNotification`; ignored before the handshake is done.
    pub fn remove_notification(&mut self, now: &Now, id: u32) -> Out {
        let mut out = Out::default();
        if self.ready {
            let mut cx = Cx {
                now,
                req: &mut self.req,
                out: &mut out,
            };
            self.companion.remove(id, &mut cx);
        }
        self.finish(now, &mut out, false);
        out
    }

    pub fn music_info(&mut self, now: &Now, info: &MusicInfo) -> Out {
        let mut out = Out::default();
        if self.ready {
            let mut cx = Cx {
                now,
                req: &mut self.req,
                out: &mut out,
            };
            self.companion.music_info(info, &mut cx);
        }
        self.finish(now, &mut out, false);
        out
    }

    pub fn music_state(&mut self, now: &Now, state: MusicState) -> Out {
        let mut out = Out::default();
        if self.ready {
            let mut cx = Cx {
                now,
                req: &mut self.req,
                out: &mut out,
            };
            self.companion.music_state(state, &mut cx);
        }
        self.finish(now, &mut out, false);
        out
    }

    pub fn music_volume(&mut self, now: &Now, percent: u8) -> Out {
        let mut out = Out::default();
        if self.ready {
            let mut cx = Cx {
                now,
                req: &mut self.req,
                out: &mut out,
            };
            self.companion.music_volume(percent, &mut cx);
        }
        self.finish(now, &mut out, false);
        out
    }

    /// `Input::SendWeather`.
    pub fn send_weather(&mut self, now: &Now, definitions: Vec<u8>, records: Vec<Vec<u8>>) -> Out {
        let mut out = Out::default();
        let max_packet = self.device.as_ref().map_or(0, |d| d.max_packet_size);
        let watch_ready = self.ready
            && self
                .watch_capabilities
                .as_deref()
                .is_some_and(|c| find::has_capability(c, wx::CAP_WEATHER_CONDITIONS));
        self.weather.send(
            definitions,
            records,
            watch_ready,
            wx::payload_budget(max_packet),
            &mut self.req,
            &mut out,
        );
        self.finish(now, &mut out, false);
        out
    }

    /// `Input::FindWatch`; ignored before the handshake is done.
    pub fn find_watch(&mut self, now: &Now, start: bool) -> Out {
        let mut out = Out::default();
        if self.ready {
            let caps = self.watch_capabilities.clone().unwrap_or_default();
            let mut cx = Cx {
                now,
                req: &mut self.req,
                out: &mut out,
            };
            self.companion.find_watch(start, &caps, &mut cx);
        }
        self.finish(now, &mut out, false);
        out
    }

    pub fn file_stored(&mut self, now: &Now, transfer_id: u64, stored: bool) -> Out {
        let mut out = Out::default();
        self.sync.on_confirmation(
            transfer_id,
            stored,
            &mut Ctx {
                now: now.mono_ms,
                req: &mut self.req,
                out: &mut out,
            },
        );
        self.finish(now, &mut out, false);
        out
    }

    pub fn on_frame(&mut self, now: &Now, frame: Result<Vec<u8>, ()>) -> Out {
        let mut out = Out::default();
        let message = frame.ok().and_then(|f| Message::parse(&f).ok());
        let progressed = match message {
            None => {
                out.anomaly(AnomalyKind::BadFrame, 0);
                false
            }
            Some(msg) if msg.id == RESPONSE => self.on_response(now, &msg, &mut out),
            Some(msg) => self.on_request(now, &msg, &mut out),
        };
        self.finish(now, &mut out, progressed);
        out
    }

    /// Common tail of every entry point: send queued requests and re-arm
    /// the reply deadline when something made progress.
    fn finish(&mut self, now: &Now, out: &mut Out, progressed: bool) {
        self.pump_uploads(now, out);
        self.req.pump(out);
        if out.closed.is_some() {
            return;
        }
        let waiting = self.req.is_pending()
            || (!self.ready && !self.handshake_started)
            || self.sync.awaiting_chunks();
        if !waiting {
            self.deadline = None;
        } else if progressed || self.deadline.is_none() {
            self.deadline = Some(now.mono_ms.saturating_add(REPLY_DEADLINE_MS));
        }
    }

    // ---- answers to our requests ----------------------------------------

    fn on_response(&mut self, now: &Now, msg: &Message, out: &mut Out) -> bool {
        let Some(r) = msg.as_response() else {
            out.anomaly(AnomalyKind::BadFrame, RESPONSE);
            return false;
        };
        let pending = match self.req.pending {
            Some(p) if p.id == r.request_id && (msg.txn.is_none() || msg.txn == Some(p.txn)) => p,
            _ => {
                out.anomaly(AnomalyKind::UnexpectedResponse, r.request_id);
                return false;
            }
        };
        self.req.pending = None;
        if let Kind::FirstConnect(event) = pending.kind {
            // Gadgetbridge ignores these replies; a refusal must not keep a
            // freshly paired watch from ever syncing. [ours]
            if r.status != Status::Ack {
                self.first_connect_acked = false;
            }
            if Some(&event) == FIRST_CONNECT_EVENTS.last() {
                out.events.push(Event::PairingCompleted {
                    all_acked: self.first_connect_acked,
                });
            }
            return true;
        }
        if let Some(anomaly) = pending.kind.soft() {
            // Like the first-connection events, a refusal is not fatal: the
            // battery and the companion duties are conveniences. [ours]
            let ack = r.status == Status::Ack;
            match pending.kind {
                Kind::UploadCreate | Kind::UploadRequest | Kind::UploadData => {
                    // The upload reports its own anomaly and ends itself;
                    // the session goes on whatever the status was.
                    let max_packet = self.device.as_ref().map_or(0, |d| d.max_packet_size);
                    let mut cx = upload::Cx {
                        now,
                        req: &mut self.req,
                        out,
                        max_packet,
                    };
                    if ack {
                        self.uploads.on_reply(pending.kind, r.body, &mut cx);
                    } else {
                        self.uploads
                            .on_nak(pending.kind, status_byte(r.status), &mut cx);
                    }
                }
                Kind::NotificationData => {
                    // The anomaly for a transfer that cannot go on is raised
                    // by the companion (it also knows the status byte).
                    let ok = ack && notif::transfer_ok(r.body);
                    let mut cx = Cx {
                        now,
                        req: &mut self.req,
                        out,
                    };
                    self.companion.on_data_answer(ok, &mut cx);
                }
                Kind::WeatherDefinition | Kind::WeatherData => {
                    // The weather answer reports its own anomaly and result.
                    self.weather.on_answer(
                        pending.kind,
                        ack,
                        wx::status_code(r.body),
                        &mut self.req,
                        out,
                    );
                }
                Kind::FindWatch if !ack => {
                    out.anomaly(anomaly, r.request_id);
                    out.events.push(Event::FindWatch {
                        state: FindWatchState::Failed,
                    });
                }
                _ if !ack => out.anomaly(anomaly, r.request_id),
                _ => {}
            }
            return true;
        }
        if r.status != Status::Ack {
            if matches!(pending.kind, Kind::Download | Kind::Archive) {
                // One file goes wrong; the cycle continues. [plan 10]
                self.sync.on_nak(
                    pending.kind,
                    status_byte(r.status),
                    &mut Ctx {
                        now: now.mono_ms,
                        req: &mut self.req,
                        out,
                    },
                );
                return true;
            }
            out.close(CloseReason::RequestFailed {
                message_id: r.request_id,
                status: status_byte(r.status),
            });
            return true;
        }
        match pending.kind {
            Kind::Config => {
                self.req
                    .send(out, m::SUPPORTED_FILE_TYPES, Vec::new(), Kind::FileTypes);
            }
            Kind::FileTypes => {
                match m::parse_supported_file_types(r.body) {
                    Some(types) => self.file_types = types,
                    None => out.anomaly(AnomalyKind::BadFrame, m::SUPPORTED_FILE_TYPES),
                }
                self.req.send(
                    out,
                    m::SYSTEM_EVENT,
                    m::build_system_event(m::EVENT_HANDSHAKE_COMPLETE, 0),
                    Kind::HandshakeComplete,
                );
            }
            Kind::HandshakeComplete => {
                self.ready = true;
                if let Some(device) = self.device.clone() {
                    out.events.push(Event::HandshakeDone {
                        device,
                        file_types: self.file_types.clone(),
                    });
                }
                if self.first_connect {
                    self.first_connect = false;
                    self.send_first_connect_events(out);
                }
                if self.battery_requested {
                    self.send_battery_request(out);
                }
                self.sync.on_ready(&mut Ctx {
                    now: now.mono_ms,
                    req: &mut self.req,
                    out,
                });
            }
            kind => self.sync.on_response(
                kind,
                r.body,
                &mut Ctx {
                    now: now.mono_ms,
                    req: &mut self.req,
                    out,
                },
            ),
        }
        true
    }

    /// Gadgetbridge sends these after SYNC_READY (our event 8), with its
    /// battery and installed-apps requests in between, which we do not send.
    /// Ours: queued after the ACK of event 8, before any sync request.
    fn send_first_connect_events(&mut self, out: &mut Out) {
        for event in FIRST_CONNECT_EVENTS {
            self.req.send(
                out,
                m::SYSTEM_EVENT,
                m::build_system_event(event, 0),
                Kind::FirstConnect(event),
            );
        }
    }

    // ---- watch requests -----------------------------------------------------

    fn on_request(&mut self, now: &Now, msg: &Message, out: &mut Out) -> bool {
        match msg.id {
            m::DEVICE_INFORMATION => self.on_device_information(msg, out),
            m::CONFIGURATION => self.on_configuration(msg, out),
            m::CURRENT_TIME => {
                self.on_current_time(now, msg, out);
                false
            }
            m::SYNC_REQUEST => {
                match m::parse_sync_request(&msg.payload) {
                    Some(req) => {
                        out.msgs.push(response(msg, 0, &[]));
                        let ready = self.sync_ready();
                        self.sync.on_watch_request(
                            ready,
                            req.option,
                            &mut Ctx {
                                now: now.mono_ms,
                                req: &mut self.req,
                                out,
                            },
                        );
                    }
                    None => out.anomaly(AnomalyKind::BadFrame, msg.id),
                }
                false
            }
            m::FILE_AVAILABLE => {
                match m::parse_file_available(&msg.payload) {
                    Some(entry) => {
                        out.msgs.push(response(msg, 0, &[]));
                        let ready = self.sync_ready();
                        self.sync.on_file_available(
                            ready,
                            entry,
                            &mut Ctx {
                                now: now.mono_ms,
                                req: &mut self.req,
                                out,
                            },
                        );
                    }
                    None => out.anomaly(AnomalyKind::BadFrame, msg.id),
                }
                false
            }
            m::BATTERY_STATUS => {
                match m::parse_battery_status(&msg.payload) {
                    Some(b) => {
                        out.msgs.push(response(msg, 0, &[]));
                        // A percentage above 100 is not a battery level, and
                        // status 7 ("invalid") marks the reading as unusable.
                        if b.percent <= 100 && b.status != BATTERY_STATUS_INVALID {
                            out.events.push(Event::Battery {
                                level_percent: b.percent,
                                report: BatteryReport::Status {
                                    status: b.status,
                                    centivolts: b.centivolts,
                                },
                            });
                        }
                    }
                    None => out.anomaly(AnomalyKind::BadFrame, msg.id),
                }
                false
            }
            m::COMPRESSED_FILE_DATA => {
                self.sync.on_compressed(
                    msg,
                    &mut Ctx {
                        now: now.mono_ms,
                        req: &mut self.req,
                        out,
                    },
                );
                false
            }
            m::FILE_DATA => self.sync.on_chunk(
                msg,
                &mut Ctx {
                    now: now.mono_ms,
                    req: &mut self.req,
                    out,
                },
            ),
            m::PROTOBUF_REQUEST | m::PROTOBUF_RESPONSE => {
                self.on_protobuf(msg, out);
                false
            }
            m::AUTH_NEGOTIATION => {
                out.close(CloseReason::AuthRequested);
                false
            }
            notif::NOTIFICATION_SUBSCRIPTION if self.duties.notifications => {
                let mut cx = Cx {
                    now,
                    req: &mut self.req,
                    out,
                };
                self.companion.on_subscription(msg, &mut cx);
                false
            }
            notif::NOTIFICATION_CONTROL if self.duties.notifications => {
                let mut cx = Cx {
                    now,
                    req: &mut self.req,
                    out,
                };
                self.companion.on_control(msg, &mut cx);
                false
            }
            find::FIND_MY_PHONE_REQUEST if self.duties.find_phone => {
                let mut cx = Cx {
                    now,
                    req: &mut self.req,
                    out,
                };
                self.companion.on_find_phone_request(msg, &mut cx);
                false
            }
            find::FIND_MY_PHONE_CANCEL if self.duties.find_phone => {
                let mut cx = Cx {
                    now,
                    req: &mut self.req,
                    out,
                };
                self.companion.on_find_phone_cancel(msg, &mut cx);
                false
            }
            music::MUSIC_CONTROL if self.duties.music => {
                let mut cx = Cx {
                    now,
                    req: &mut self.req,
                    out,
                };
                self.companion.on_music_control(msg, &mut cx);
                false
            }
            music::MUSIC_CAPABILITIES if self.duties.music => {
                let mut cx = Cx {
                    now,
                    req: &mut self.req,
                    out,
                };
                self.companion.on_music_capabilities(msg, &mut cx);
                false
            }
            music::MUSIC_ENTITY_UPDATE if self.duties.music => {
                let mut cx = Cx {
                    now,
                    req: &mut self.req,
                    out,
                };
                self.companion.on_music_entity_request(msg, &mut cx);
                false
            }
            wx::WEATHER_REQUEST if self.duties.weather => {
                match wx::parse_weather_request(&msg.payload) {
                    Some(r) => {
                        out.msgs.push(response(msg, 0, &[]));
                        out.events.push(Event::WeatherRequest {
                            format: r.format,
                            cell: r.cell,
                            hours: r.hours,
                        });
                    }
                    None => out.anomaly(AnomalyKind::BadFrame, msg.id),
                }
                false
            }
            id => {
                out.msgs.push(response(msg, STATUS_UNSUPPORTED, &[]));
                out.events.push(Event::Unsupported { message_id: id });
                false
            }
        }
    }

    fn on_device_information(&mut self, msg: &Message, out: &mut Out) -> bool {
        let Some(d) = m::parse_device_information(&msg.payload) else {
            out.anomaly(AnomalyKind::BadFrame, msg.id);
            return false;
        };
        if d.protocol < m::PROTOCOL_VERSION {
            out.close(CloseReason::ProtocolTooOld {
                version: d.protocol,
            });
            return false;
        }
        if self.is_idle() {
            // A new 5024 on an idle, finished session (e.g. after the watch
            // reopened GFDI) starts the handshake again: wait for its 5050,
            // then our 5050, 5031 and event 8. [ours]
            self.ready = false;
            self.handshake_started = false;
            self.watch_capabilities = None;
        }
        self.device = Some(DeviceInfo {
            protocol_version: d.protocol,
            product_number: d.product,
            unit_id: d.unit_id,
            software_version: d.software,
            max_packet_size: d.max_packet,
            bluetooth_name: d.bluetooth_name,
            device_name: d.device_name,
            model_name: d.model_name,
            capabilities: self.watch_capabilities.clone().unwrap_or_default(),
        });
        out.msgs
            .push(response(msg, 0, &m::build_host_info(&self.host)));
        self.maybe_send_configuration(out);
        true
    }

    fn on_configuration(&mut self, msg: &Message, out: &mut Out) -> bool {
        let Some(mask) = m::parse_configuration(&msg.payload) else {
            out.anomaly(AnomalyKind::BadFrame, msg.id);
            return false;
        };
        if let Some(d) = self.device.as_mut() {
            d.capabilities = mask.clone();
        }
        self.watch_capabilities = Some(mask);
        out.msgs.push(response(msg, 0, &[]));
        self.maybe_send_configuration(out);
        true
    }

    /// Our own Configuration follows once the watch's DeviceInformation and
    /// Configuration have both arrived and been acknowledged.
    fn maybe_send_configuration(&mut self, out: &mut Out) {
        if self.handshake_started || self.device.is_none() || self.watch_capabilities.is_none() {
            return;
        }
        self.handshake_started = true;
        self.req.send(
            out,
            m::CONFIGURATION,
            m::build_configuration(&self.capabilities),
            Kind::Config,
        );
    }

    fn on_current_time(&mut self, now: &Now, msg: &Message, out: &mut Out) {
        let Some(reference) = m::parse_current_time_request(&msg.payload) else {
            out.anomaly(AnomalyKind::BadFrame, msg.id);
            return;
        };
        if now.unix_s < MIN_VALID_UNIX_S {
            out.anomaly(AnomalyKind::ClockInvalid, msg.id);
            return;
        }
        let garmin = |unix: i64| (unix - GARMIN_EPOCH_UNIX_S).clamp(0, u32::MAX as i64) as u32;
        let reply = m::CurrentTimeReply {
            reference,
            garmin_time: garmin(now.unix_s),
            utc_offset_s: now.utc_offset_s,
            // No transition ahead: a literal 0 in both fields.
            dst_start: now.dst.map_or(0, |d| garmin(d.start_unix_s)),
            dst_end: now.dst.map_or(0, |d| garmin(d.end_unix_s)),
        };
        out.msgs
            .push(response(msg, 0, &m::build_current_time_reply(&reply)));
        out.events.push(Event::TimeSent {
            garmin_time: reply.garmin_time,
            utc_offset_s: now.utc_offset_s,
        });
    }

    /// Watch protobuf requests (5043) and answers to ours (5044) are
    /// acknowledged with an 8-byte body; we never answer them (5044). Of the
    /// content we read only the battery response (device_status 8), from
    /// single-chunk messages; everything else is ignored.
    fn on_protobuf(&mut self, msg: &Message, out: &mut Out) {
        let Some((header, body)) = m::parse_protobuf(&msg.payload) else {
            out.anomaly(AnomalyKind::BadFrame, msg.id);
            return;
        };
        if header.offset != 0 || header.chunk != header.total {
            out.anomaly(AnomalyKind::ChunkedProtobuf, msg.id);
        }
        out.msgs
            .push(response(msg, 0, &m::build_protobuf_ack(header.request_id)));
        if header.offset == 0 && header.chunk == header.total {
            if let Some(b) = m::parse_battery_protobuf(body) {
                out.events.push(Event::Battery {
                    level_percent: b.level,
                    report: BatteryReport::DeviceStatus {
                        battery_status: b.battery_status,
                        hours_remaining: b.hours_remaining,
                    },
                });
            }
            self.companion.on_find_watch_protobuf(body, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txn_counts_from_zero_and_wraps_at_32() {
        let mut req = Requests::default();
        let mut out = Out::default();
        let mut seen = Vec::new();
        for _ in 0..40 {
            req.send(&mut out, m::DIRECTORY_FILTER, vec![3], Kind::Filter);
            let sent = out.msgs.pop().expect("sent at once");
            seen.push(sent.txn.unwrap());
            req.pending = None;
        }
        let expected: Vec<u8> = (0..40).map(|i| (i % 32) as u8).collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn only_one_request_is_outstanding_and_order_is_kept() {
        let mut req = Requests::default();
        let mut out = Out::default();
        req.send(&mut out, 5007, vec![1], Kind::Filter);
        req.send(&mut out, 5002, vec![2], Kind::Download);
        assert_eq!(out.msgs.len(), 1, "second request waits");
        req.pending = None;
        req.pump(&mut out);
        let ids: Vec<u16> = out.msgs.iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![5007, 5002]);
    }

    #[test]
    fn response_echoes_the_requests_txn_and_id() {
        let request = Message {
            id: 5036,
            txn: Some(9),
            payload: vec![],
        };
        let r = response(&request, 2, &[]);
        assert_eq!(
            (r.id, r.txn, r.payload),
            (5000, Some(9), vec![0xAC, 0x13, 2])
        );
        let plain = response(&Message::new(5036, vec![]), 0, &[7]);
        assert_eq!(plain.txn, None);
    }
}
