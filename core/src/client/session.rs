//! GFDI session: handshake, time duty, answers to watch requests, and the
//! request bookkeeping (one outstanding request, txn counter, 30 s deadline).
//! The file sync cycle lives in `sync.rs`.

use std::collections::VecDeque;

use super::sync::{Ctx, SyncCycle};
use super::transport::REPLY_DEADLINE_MS;
use super::{
    AnomalyKind, ClientConfig, CloseReason, DeviceInfo, Event, FileType, Now, Phase,
    GARMIN_EPOCH_UNIX_S,
};
use crate::gfdi::{Message, Status, RESPONSE};
use crate::messages as m;

/// Wall clocks before this (2020-01-01T00:00Z) are treated as unset.
const MIN_VALID_UNIX_S: i64 = 1_577_836_800;

/// "Unknown or not supported" status for requests we do not implement.
const STATUS_UNSUPPORTED: u8 = 2;

/// What a request is for, so its answer reaches the right handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Config,
    FileTypes,
    HandshakeComplete,
    SyncNow,
    Filter,
    Download,
    Archive,
    SyncComplete,
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
    device: Option<DeviceInfo>,
    watch_capabilities: Option<Vec<u8>>,
    file_types: Vec<FileType>,
    handshake_started: bool,
    ready: bool,
    req: Requests,
    sync: SyncCycle,
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
            device: None,
            watch_capabilities: None,
            file_types: Vec::new(),
            handshake_started: false,
            ready: false,
            req: Requests::default(),
            sync: SyncCycle::new(config.connection_epoch),
            deadline: None,
        }
    }

    /// The link is up: wait for the watch's DeviceInformation.
    pub fn start(&mut self, now: &Now) {
        self.deadline = Some(now.mono_ms.saturating_add(REPLY_DEADLINE_MS));
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
            out.close(CloseReason::Timeout {
                phase: self.phase(),
            });
            return out;
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
        self.sync.start_manual(
            self.ready,
            &mut Ctx {
                now: now.mono_ms,
                req: &mut self.req,
                out: &mut out,
            },
        );
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
                        self.sync.on_watch_request(
                            self.ready,
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
            next_dst: garmin(now.next_dst_unix_s),
            following_dst: garmin(now.following_dst_unix_s),
        };
        out.msgs
            .push(response(msg, 0, &m::build_current_time_reply(&reply)));
        out.events.push(Event::TimeSent {
            garmin_time: reply.garmin_time,
            utc_offset_s: now.utc_offset_s,
        });
    }

    /// Watch protobuf requests (5043) and answers to ours (5044) are
    /// acknowledged with an 8-byte body; we never answer them (5044).
    fn on_protobuf(&mut self, msg: &Message, out: &mut Out) {
        let Some((header, _)) = m::parse_protobuf(&msg.payload) else {
            out.anomaly(AnomalyKind::BadFrame, msg.id);
            return;
        };
        if header.offset != 0 || header.chunk != header.total {
            out.anomaly(AnomalyKind::ChunkedProtobuf, msg.id);
        }
        out.msgs
            .push(response(msg, 0, &m::build_protobuf_ack(header.request_id)));
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
