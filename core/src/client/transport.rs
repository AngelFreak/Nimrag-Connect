//! Multi-link transport for one characteristic pair: bring-up, handle
//! routing, and the reliable GFDI byte stream.
//!
//! Bring-up is strictly request then reply (PROTOCOL.md 1.3): close-all,
//! REGISTRATION (plain), info pages 0-4, GFDI (reliable), KEEP_ALIVE (plain)
//! and the keep-alive payload. Nothing here interprets GFDI messages.

use super::reliable::MlrEndpoint;
use super::{AnomalyKind, CloseReason};
use crate::cobs::FrameDecoder;
use crate::mlr;
use crate::multilink::{self, Control};

pub const SERVICE_GFDI: u16 = 1;
pub const SERVICE_REGISTRATION: u16 = 4;
pub const SERVICE_KEEP_ALIVE: u16 = 22;

/// Info pages the client reads (page 5 is deliberately not read).
pub const INFO_PAGES: u8 = 5;
/// Payload written once to the keep-alive handle. [PROTOCOL.md 1.3, CAP-1]
pub const KEEP_ALIVE_HELLO: [u8; 5] = [0x01, 0x19, 0x00, 0x00, 0x00];

/// Reply deadline for every awaited answer.
pub const REPLY_DEADLINE_MS: u64 = 30_000;

/// Largest unfinished COBS frame we buffer.
const MAX_PENDING_FRAME: usize = 1 << 20;

const REGISTER_PENDING_AUTH: u8 = 2;
const REGISTER_ALREADY_IN_USE: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    CloseAll,
    RegisterRegistration,
    InfoPage(u8),
    RegisterGfdi,
    RegisterKeepAlive,
    Up,
}

/// Everything one transport call produced.
#[derive(Debug, Default)]
pub struct TransportOut {
    /// ATT values to write, in order.
    pub writes: Vec<Vec<u8>>,
    /// Decoded GFDI frames, in order (`Err` = COBS error).
    pub frames: Vec<Result<Vec<u8>, ()>>,
    pub anomalies: Vec<AnomalyKind>,
    /// Bring-up finished with this call.
    pub link_ready: bool,
    pub closed: Option<CloseReason>,
}

#[derive(Debug)]
pub struct Transport {
    client_id: u64,
    att_mtu: u16,
    stage: Stage,
    registration_handle: Option<u8>,
    keep_alive_handle: Option<u8>,
    gfdi: Option<MlrEndpoint>,
    cobs: FrameDecoder,
    deadline: Option<u64>,
}

impl Transport {
    pub fn new(client_id: u64) -> Self {
        Self {
            client_id,
            att_mtu: 0,
            stage: Stage::Idle,
            registration_handle: None,
            keep_alive_handle: None,
            gfdi: None,
            cobs: FrameDecoder::new(),
            deadline: None,
        }
    }

    pub fn is_up(&self) -> bool {
        self.stage == Stage::Up
    }

    /// Start bring-up: first write is close-all.
    pub fn start(&mut self, now: u64, att_mtu: u16) -> TransportOut {
        self.att_mtu = att_mtu;
        self.stage = Stage::CloseAll;
        self.deadline = Some(now.saturating_add(REPLY_DEADLINE_MS));
        TransportOut {
            writes: vec![multilink::close_all_request(self.client_id)],
            ..Default::default()
        }
    }

    /// Queue one GFDI frame (already COBS-framed) on the reliable stream.
    pub fn send_gfdi(&mut self, now: u64, framed: &[u8]) -> Vec<Vec<u8>> {
        match self.gfdi.as_mut() {
            Some(ep) => ep.send(now, framed),
            None => Vec::new(),
        }
    }

    pub fn on_notification(&mut self, now: u64, value: &[u8]) -> TransportOut {
        let mut out = TransportOut::default();
        let Some(&first) = value.first() else {
            out.anomalies.push(AnomalyKind::BadTransport);
            return out;
        };
        if first == 0 {
            self.on_control(now, value, &mut out);
        } else if first & 0x80 != 0 {
            self.on_mlr(now, value, &mut out);
        } else if Some(first) == self.registration_handle {
            self.on_info_reply(now, value, &mut out);
        } else if Some(first) == self.keep_alive_handle {
            // Keep-alive values (`02 00`, counters) are consumed silently.
        } else {
            out.anomalies.push(AnomalyKind::BadTransport);
        }
        out
    }

    pub fn on_tick(&mut self, now: u64) -> TransportOut {
        let mut out = TransportOut::default();
        if let Some(ep) = self.gfdi.as_mut() {
            out.writes.extend(ep.poll(now));
        }
        if self.deadline.is_some_and(|d| now >= d) {
            self.deadline = None;
            out.closed = Some(CloseReason::Timeout {
                phase: super::Phase::BringUp,
            });
        }
        out
    }

    pub fn next_wake(&self) -> Option<u64> {
        let mlr = self.gfdi.as_ref().and_then(|e| e.next_deadline());
        match (mlr, self.deadline) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn on_control(&mut self, now: u64, value: &[u8], out: &mut TransportOut) {
        let Some(control) = multilink::parse_control(value) else {
            out.anomalies.push(AnomalyKind::BadTransport);
            return;
        };
        match (self.stage, control) {
            (Stage::CloseAll, Control::CloseAllResponse { client_id, .. })
                if client_id == self.client_id =>
            {
                // Any status is accepted (1 seen, meaning unknown Q-ML-1).
                self.register(
                    now,
                    SERVICE_REGISTRATION,
                    0,
                    Stage::RegisterRegistration,
                    out,
                );
            }
            (
                stage,
                Control::RegisterResponse {
                    client_id,
                    service,
                    status,
                    handle,
                    flags,
                    redirect,
                    ..
                },
            ) if client_id == self.client_id && Some(service) == self.expected_service(stage) => {
                match status {
                    0 => self.registered(now, stage, handle, flags, out),
                    // Pending authorisation: keep waiting. [seen APK]
                    REGISTER_PENDING_AUTH => {}
                    REGISTER_ALREADY_IN_USE => {
                        out.closed = Some(CloseReason::ChannelInUse {
                            retry_char: redirect.unwrap_or(0),
                        })
                    }
                    other => {
                        out.closed = Some(CloseReason::RegisterRejected {
                            service,
                            status: other,
                        })
                    }
                }
            }
            _ => out.anomalies.push(AnomalyKind::BadTransport),
        }
    }

    fn expected_service(&self, stage: Stage) -> Option<u16> {
        match stage {
            Stage::RegisterRegistration => Some(SERVICE_REGISTRATION),
            Stage::RegisterGfdi => Some(SERVICE_GFDI),
            Stage::RegisterKeepAlive => Some(SERVICE_KEEP_ALIVE),
            _ => None,
        }
    }

    fn register(&mut self, now: u64, service: u16, mode: u8, next: Stage, out: &mut TransportOut) {
        self.stage = next;
        self.deadline = Some(now.saturating_add(REPLY_DEADLINE_MS));
        out.writes
            .push(multilink::register_request(self.client_id, service, mode));
    }

    fn registered(
        &mut self,
        now: u64,
        stage: Stage,
        handle: Option<u8>,
        flags: Option<u8>,
        out: &mut TransportOut,
    ) {
        let Some(handle) = handle else {
            out.anomalies.push(AnomalyKind::BadTransport);
            return;
        };
        match stage {
            Stage::RegisterRegistration => {
                self.registration_handle = Some(handle);
                self.request_info_page(now, 0, out);
            }
            Stage::RegisterGfdi => {
                if flags.unwrap_or(0) & 1 == 0 {
                    out.closed = Some(CloseReason::ReliableNotGranted);
                    return;
                }
                self.gfdi = Some(MlrEndpoint::new(handle & 7, self.att_mtu));
                self.register(now, SERVICE_KEEP_ALIVE, 0, Stage::RegisterKeepAlive, out);
            }
            Stage::RegisterKeepAlive => {
                self.keep_alive_handle = Some(handle);
                let mut hello = vec![handle];
                hello.extend_from_slice(&KEEP_ALIVE_HELLO);
                out.writes.push(hello);
                self.stage = Stage::Up;
                self.deadline = None;
                out.link_ready = true;
            }
            _ => {}
        }
    }

    fn request_info_page(&mut self, now: u64, page: u8, out: &mut TransportOut) {
        let Some(handle) = self.registration_handle else {
            return;
        };
        self.stage = Stage::InfoPage(page);
        self.deadline = Some(now.saturating_add(REPLY_DEADLINE_MS));
        out.writes.push(vec![handle, page]);
    }

    fn on_info_reply(&mut self, now: u64, value: &[u8], out: &mut TransportOut) {
        let Stage::InfoPage(page) = self.stage else {
            out.anomalies.push(AnomalyKind::BadTransport);
            return;
        };
        // `[handle, page, data...]`, or `[handle, 0xFF, page]` when unsupported.
        match value.get(1) {
            Some(&p) if p == page || p == 0xFF => {}
            _ => {
                out.anomalies.push(AnomalyKind::BadTransport);
                return;
            }
        }
        if page + 1 < INFO_PAGES {
            self.request_info_page(now, page + 1, out);
        } else {
            self.register(now, SERVICE_GFDI, 2, Stage::RegisterGfdi, out);
        }
    }

    fn on_mlr(&mut self, now: u64, value: &[u8], out: &mut TransportOut) {
        let Some((header, data)) = mlr::parse(value) else {
            out.anomalies.push(AnomalyKind::BadTransport);
            return;
        };
        let Some(ep) = self.gfdi.as_mut().filter(|e| e.handle() == header.handle) else {
            out.anomalies.push(AnomalyKind::BadTransport);
            return;
        };
        let received = ep.receive(now, header, data);
        out.writes.extend(received.writes);
        if received.data.is_empty() {
            return;
        }
        out.frames.extend(
            self.cobs
                .push(&received.data)
                .into_iter()
                .map(|f| f.map_err(|_| ())),
        );
        if self.cobs.pending() > MAX_PENDING_FRAME {
            self.cobs = FrameDecoder::new();
            out.anomalies.push(AnomalyKind::BadFrame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: u64 = 0xABCD;

    fn register_ok(service: u16, handle: u8, flags: u8) -> Vec<u8> {
        let mut v = vec![0, 1];
        v.extend_from_slice(&ID.to_le_bytes());
        v.extend_from_slice(&service.to_le_bytes());
        v.extend_from_slice(&[0, handle, flags, 1]);
        v
    }

    fn close_all_ok() -> Vec<u8> {
        let mut v = vec![0, 6];
        v.extend_from_slice(&ID.to_le_bytes());
        v.extend_from_slice(&[0, 0, 1]);
        v
    }

    /// Drive bring-up with a cooperative watch and return every write.
    fn bring_up() -> (Transport, Vec<Vec<u8>>) {
        let mut t = Transport::new(ID);
        let mut writes = t.start(0, 23).writes;
        let replies: Vec<Vec<u8>> = vec![
            close_all_ok(),
            register_ok(4, 0x2E, 0),
            vec![0x2E, 0, 1],
            vec![0x2E, 1, 1],
            vec![0x2E, 2, 1],
            vec![0x2E, 3, 1],
            vec![0x2E, 0xFF, 4],
            register_ok(1, 0x83, 1),
            register_ok(22, 0x30, 0),
        ];
        for r in replies {
            let out = t.on_notification(10, &r);
            assert!(out.anomalies.is_empty() && out.closed.is_none(), "{r:02x?}");
            writes.extend(out.writes);
        }
        (t, writes)
    }

    #[test]
    fn bring_up_is_request_reply_with_the_documented_bytes() {
        let (t, writes) = bring_up();
        assert!(t.is_up());
        let mut expected = vec![
            multilink::close_all_request(ID),
            multilink::register_request(ID, 4, 0),
        ];
        expected.extend((0..5u8).map(|p| vec![0x2E, p]));
        expected.push(multilink::register_request(ID, 1, 2));
        expected.push(multilink::register_request(ID, 22, 0));
        expected.push(vec![0x30, 0x01, 0x19, 0x00, 0x00, 0x00]);
        assert_eq!(writes, expected);
    }

    #[test]
    fn nothing_is_written_until_the_reply_arrives() {
        let mut t = Transport::new(ID);
        assert_eq!(t.start(0, 23).writes.len(), 1);
        // A register response before close-all was answered is not ours yet.
        let out = t.on_notification(1, &register_ok(4, 0x2E, 0));
        assert!(out.writes.is_empty());
        assert_eq!(out.anomalies, vec![AnomalyKind::BadTransport]);
    }

    #[test]
    fn pending_auth_keeps_waiting_and_in_use_names_the_retry_characteristic() {
        let mut t = Transport::new(ID);
        t.start(0, 23);
        t.on_notification(1, &close_all_ok());
        let mut pending = vec![0, 1];
        pending.extend_from_slice(&ID.to_le_bytes());
        pending.extend_from_slice(&4u16.to_le_bytes());
        pending.push(2);
        let out = t.on_notification(2, &pending);
        assert!(out.writes.is_empty() && out.closed.is_none());
        let mut in_use = vec![0, 1];
        in_use.extend_from_slice(&ID.to_le_bytes());
        in_use.extend_from_slice(&4u16.to_le_bytes());
        in_use.push(3);
        in_use.extend_from_slice(&0x2812u16.to_le_bytes());
        let out = t.on_notification(3, &in_use);
        assert_eq!(
            out.closed,
            Some(CloseReason::ChannelInUse { retry_char: 0x2812 })
        );
    }

    #[test]
    fn invalid_handle_and_unknown_command_are_anomalies() {
        let (mut t, _) = bring_up();
        for kind in [4u8, 0xFF] {
            let mut v = vec![0, kind];
            v.extend_from_slice(&ID.to_le_bytes());
            v.extend_from_slice(&[1, 0, 9]);
            let out = t.on_notification(20, &v);
            assert_eq!(
                out.anomalies,
                vec![AnomalyKind::BadTransport],
                "type {kind}"
            );
            assert!(out.writes.is_empty());
        }
        // Unknown plain handle and an MLR packet on another handle.
        assert_eq!(
            t.on_notification(21, &[0x55, 1]).anomalies,
            vec![AnomalyKind::BadTransport]
        );
        assert_eq!(
            t.on_notification(21, &[0xA0, 0, 1]).anomalies,
            vec![AnomalyKind::BadTransport]
        );
    }

    #[test]
    fn keep_alive_values_are_consumed_silently() {
        let (mut t, _) = bring_up();
        let out = t.on_notification(30, &[0x30, 0x02, 0x00]);
        assert!(out.anomalies.is_empty() && out.writes.is_empty() && out.frames.is_empty());
    }
}
