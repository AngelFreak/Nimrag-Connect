//! Multi-link transport for one characteristic pair: bring-up, handle
//! routing, and the GFDI byte stream (reliable, or plain when the watch does
//! not grant MLR).
//!
//! Bring-up is strictly request then reply (PROTOCOL.md 1.3): close-all,
//! REGISTRATION (plain), info pages 0-4, GFDI (reliable requested), KEEP_ALIVE
//! (plain) and the keep-alive payload. Nothing here interprets GFDI messages.
//!
//! Plain GFDI mode and the re-registration after a watch-side close are
//! ported from Gadgetbridge commit 7257742b96a30fff0db9e79230435ac2aeb39f57
//! (AGPL-3.0-or-later), `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/
//! service/devices/garmin/communicator/v2/CommunicatorV2.java`
//! (`sendMessage`, `onCharacteristicChanged`, `processHandleManagement`).
//! Copyright (C) 2024-2026 Daniele Gobbetti, José Rebelo, Thomas Kuehne and
//! the other Gadgetbridge contributors. Both behaviours are *inferred
//! (Gadgetbridge)*: never seen in our captures (PROTOCOL.md 9.1).

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
    /// Up, but the watch closed the GFDI handle and our new register request
    /// is outstanding. [inferred (Gadgetbridge)]
    ReopenGfdi,
}

/// How the GFDI byte stream travels over its registered handle.
#[derive(Debug)]
enum GfdiLink {
    /// Reliable multi-link (the register response granted MLR).
    Reliable(MlrEndpoint),
    /// Plain multi-link: every ATT value is `[handle, bytes...]`.
    Plain { handle: u8 },
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
    /// The watch closed the GFDI handle and a new registration was asked for.
    pub gfdi_closed: bool,
    pub closed: Option<CloseReason>,
}

#[derive(Debug)]
pub struct Transport {
    client_id: u64,
    att_mtu: u16,
    stage: Stage,
    registration_handle: Option<u8>,
    keep_alive_handle: Option<u8>,
    gfdi: Option<GfdiLink>,
    /// GFDI frames the session sent while the handle was closed; written
    /// once it is registered again. [ours; Gadgetbridge drops them]
    held: Vec<Vec<u8>>,
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
            held: Vec::new(),
            cobs: FrameDecoder::new(),
            deadline: None,
        }
    }

    /// Bring-up is done. Stays true while GFDI is being registered again.
    pub fn is_up(&self) -> bool {
        matches!(self.stage, Stage::Up | Stage::ReopenGfdi)
    }

    /// GFDI is being registered again after the watch closed it.
    pub fn reopening(&self) -> bool {
        self.stage == Stage::ReopenGfdi
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

    /// Queue one GFDI frame (already COBS-framed) on the GFDI stream. While
    /// the handle is closed (see `ReopenGfdi`) the frame is held and written
    /// on the new handle; Gadgetbridge drops it ("handle not yet set").
    pub fn send_gfdi(&mut self, now: u64, framed: &[u8]) -> Vec<Vec<u8>> {
        if self.stage == Stage::ReopenGfdi {
            self.held.push(framed.to_vec());
            return Vec::new();
        }
        let att_mtu = self.att_mtu;
        match self.gfdi.as_mut() {
            Some(GfdiLink::Reliable(ep)) => ep.send(now, framed),
            Some(GfdiLink::Plain { handle }) => plain_fragments(*handle, att_mtu, framed),
            None => Vec::new(),
        }
    }

    pub fn on_notification(&mut self, now: u64, value: &[u8]) -> TransportOut {
        let mut out = TransportOut::default();
        let Some(&first) = value.first() else {
            out.anomalies.push(AnomalyKind::BadTransport);
            return out;
        };
        // An MLR packet is recognised by its handle; a plain handle may also
        // have bit 7 set, so anything that is not ours as MLR falls through to
        // the plain handles. [inferred (Gadgetbridge), issue #5476 comment]
        if first == 0 {
            self.on_control(now, value, &mut out);
        } else if self.is_mlr_packet(value) {
            self.on_mlr(now, value, &mut out);
        } else if matches!(self.gfdi, Some(GfdiLink::Plain { handle }) if handle == first) {
            self.push_cobs(&value[1..], &mut out);
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
        if let Some(GfdiLink::Reliable(ep)) = self.gfdi.as_mut() {
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
        let mlr = match &self.gfdi {
            Some(GfdiLink::Reliable(ep)) => ep.next_deadline(),
            _ => None,
        };
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
            // The watch closed the GFDI handle on its own: register GFDI again.
            // Gadgetbridge acts on the service id alone; the status and the
            // handle are not looked at. [inferred (Gadgetbridge), `CommunicatorV2`
            // CLOSE_HANDLE_RESP]
            (
                Stage::Up,
                Control::CloseHandleResponse {
                    client_id,
                    service: SERVICE_GFDI,
                    ..
                },
            ) if client_id == self.client_id => self.reopen_gfdi(now, out),
            _ => out.anomalies.push(AnomalyKind::BadTransport),
        }
    }

    /// Drop the closed GFDI link and ask for a new one. Both byte streams
    /// start over: a new MLR endpoint (sequence numbers from 0, unacked and
    /// queued data lost) or plain handle, and an empty COBS decoder
    /// (Gadgetbridge builds a new decoder in the register handler). The mode
    /// requested is the one of bring-up (Gadgetbridge: the MLR preference).
    fn reopen_gfdi(&mut self, now: u64, out: &mut TransportOut) {
        self.gfdi = None;
        self.cobs = FrameDecoder::new();
        out.anomalies.push(AnomalyKind::GfdiClosedByWatch);
        out.gfdi_closed = true;
        self.register(now, SERVICE_GFDI, 2, Stage::ReopenGfdi, out);
    }

    fn expected_service(&self, stage: Stage) -> Option<u16> {
        match stage {
            Stage::RegisterRegistration => Some(SERVICE_REGISTRATION),
            Stage::RegisterGfdi | Stage::ReopenGfdi => Some(SERVICE_GFDI),
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
            Stage::RegisterGfdi | Stage::ReopenGfdi => {
                // Flag bit 0 set: MLR [seen CAP-1]. Not set: plain multi-link
                // on the register response handle as is. [inferred
                // (Gadgetbridge), which takes any non-zero flag byte as
                // reliable; bit 0 is ours, PROTOCOL.md 9.1]
                self.gfdi = Some(if flags.unwrap_or(0) & 1 != 0 {
                    GfdiLink::Reliable(MlrEndpoint::new(handle & 7, self.att_mtu))
                } else {
                    GfdiLink::Plain { handle }
                });
                if stage == Stage::ReopenGfdi {
                    self.stage = Stage::Up;
                    self.deadline = None;
                    for framed in std::mem::take(&mut self.held) {
                        let writes = self.send_gfdi(now, &framed);
                        out.writes.extend(writes);
                    }
                } else {
                    self.register(now, SERVICE_KEEP_ALIVE, 0, Stage::RegisterKeepAlive, out);
                }
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

    /// Is this value an MLR packet for our reliable GFDI handle?
    fn is_mlr_packet(&self, value: &[u8]) -> bool {
        match (&self.gfdi, mlr::parse(value)) {
            (Some(GfdiLink::Reliable(ep)), Some((header, _))) => ep.handle() == header.handle,
            _ => false,
        }
    }

    fn on_mlr(&mut self, now: u64, value: &[u8], out: &mut TransportOut) {
        let Some((header, data)) = mlr::parse(value) else {
            out.anomalies.push(AnomalyKind::BadTransport);
            return;
        };
        let Some(GfdiLink::Reliable(ep)) = self.gfdi.as_mut() else {
            out.anomalies.push(AnomalyKind::BadTransport);
            return;
        };
        if ep.handle() != header.handle {
            out.anomalies.push(AnomalyKind::BadTransport);
            return;
        }
        let received = ep.receive(now, header, data);
        out.writes.extend(received.writes);
        self.push_cobs(&received.data, out);
    }

    /// Feed in-order GFDI stream bytes to the COBS decoder.
    fn push_cobs(&mut self, data: &[u8], out: &mut TransportOut) {
        if data.is_empty() {
            return;
        }
        out.frames
            .extend(self.cobs.push(data).into_iter().map(|f| f.map_err(|_| ())));
        if self.cobs.pending() > MAX_PENDING_FRAME {
            self.cobs = FrameDecoder::new();
            out.anomalies.push(AnomalyKind::BadFrame);
        }
    }
}

/// Largest ATT value Gadgetbridge writes: `min(512, mtu - 3)`, 512 being the
/// largest attribute value. [inferred (Gadgetbridge), `calcMaxWriteChunk`]
const MAX_ATT_VALUE: usize = 512;

/// Split COBS-framed GFDI bytes into plain-mode ATT values: the handle byte
/// and at most `max write - 1` data bytes, so 19 data bytes at MTU 23.
/// [inferred (Gadgetbridge), `CommunicatorV2.sendMessage`]
fn plain_fragments(handle: u8, att_mtu: u16, framed: &[u8]) -> Vec<Vec<u8>> {
    let data_max = (att_mtu as usize)
        .saturating_sub(3)
        .min(MAX_ATT_VALUE)
        .saturating_sub(1)
        .max(1);
    framed
        .chunks(data_max)
        .map(|chunk| {
            let mut value = Vec::with_capacity(chunk.len() + 1);
            value.push(handle);
            value.extend_from_slice(chunk);
            value
        })
        .collect()
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

    fn close_handle_response(service: u16, handle: u8) -> Vec<u8> {
        let mut v = vec![0, 3];
        v.extend_from_slice(&ID.to_le_bytes());
        v.extend_from_slice(&service.to_le_bytes());
        v.extend_from_slice(&[handle, 0]);
        v
    }

    /// Drive bring-up with a cooperative watch and return every write.
    fn bring_up() -> (Transport, Vec<Vec<u8>>) {
        bring_up_with(23, register_ok(1, 0x83, 1))
    }

    /// Bring-up where the GFDI register reply is `gfdi_reply`.
    fn bring_up_with(att_mtu: u16, gfdi_reply: Vec<u8>) -> (Transport, Vec<Vec<u8>>) {
        let mut t = Transport::new(ID);
        let mut writes = t.start(0, att_mtu).writes;
        let replies: Vec<Vec<u8>> = vec![
            close_all_ok(),
            register_ok(4, 0x2E, 0),
            vec![0x2E, 0, 1],
            vec![0x2E, 1, 1],
            vec![0x2E, 2, 1],
            vec![0x2E, 3, 1],
            vec![0x2E, 0xFF, 4],
            gfdi_reply,
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

    // ---- plain mode: GFDI registered without the reliable flag ---------------

    const PLAIN: u8 = 0x31;

    fn plain_bring_up() -> Transport {
        let (t, writes) = bring_up_with(23, register_ok(1, PLAIN, 0));
        assert!(t.is_up());
        // Same requests as the reliable bring-up: reliable is always asked for.
        assert_eq!(writes[7], multilink::register_request(ID, 1, 2));
        t
    }

    #[test]
    fn plain_mode_splits_outgoing_bytes_behind_one_handle_byte() {
        let mut t = plain_bring_up();
        let framed: Vec<u8> = (1..=45).collect();
        let writes = t.send_gfdi(5, &framed);
        // MTU 23: 20 bytes per write, one of them the handle.
        let sizes: Vec<usize> = writes.iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![20, 20, 8]);
        assert!(writes.iter().all(|w| w[0] == PLAIN));
        let joined: Vec<u8> = writes.iter().flat_map(|w| w[1..].to_vec()).collect();
        assert_eq!(joined, framed);
        // Short frame: one write. No timers in plain mode.
        assert_eq!(t.send_gfdi(6, &[9, 9]), vec![vec![PLAIN, 9, 9]]);
        assert_eq!(t.next_wake(), None);
        let out = t.on_tick(10_000);
        assert!(out.writes.is_empty() && out.closed.is_none());
    }

    #[test]
    fn plain_mode_write_size_follows_the_mtu_up_to_512() {
        let sizes = |mtu: u16, n: usize| {
            let (mut t, _) = bring_up_with(mtu, register_ok(1, PLAIN, 0));
            t.send_gfdi(0, &vec![7; n])
                .iter()
                .map(Vec::len)
                .collect::<Vec<_>>()
        };
        assert_eq!(sizes(247, 500), vec![244, 244, 15]);
        // ATT values are never longer than 512 bytes (MTU 517 would allow 514).
        assert_eq!(sizes(517, 1200), vec![512, 512, 179]);
    }

    #[test]
    fn plain_mode_incoming_values_feed_the_cobs_decoder() {
        let mut t = plain_bring_up();
        let msg: Vec<u8> = (1..=40).collect();
        let framed = crate::cobs::encode_frame(&msg);
        let (a, b) = framed.split_at(19);
        let mut first = vec![PLAIN];
        first.extend_from_slice(a);
        let mut second = vec![PLAIN];
        second.extend_from_slice(b);
        let out = t.on_notification(20, &first);
        assert!(out.frames.is_empty() && out.anomalies.is_empty() && out.writes.is_empty());
        let out = t.on_notification(21, &second);
        assert_eq!(out.frames, vec![Ok(msg)]);
        assert!(out.anomalies.is_empty() && out.writes.is_empty());
        // A bare handle byte carries no data; other handles stay unknown.
        let out = t.on_notification(22, &[PLAIN]);
        assert!(out.frames.is_empty() && out.anomalies.is_empty());
        assert_eq!(
            t.on_notification(23, &[0x55, 1]).anomalies,
            vec![AnomalyKind::BadTransport]
        );
        // An MLR packet has no handle to land on in plain mode.
        let header = mlr::Header {
            handle: 3,
            ack: 0,
            seq: 1,
        };
        assert_eq!(
            t.on_notification(24, &mlr::build(header, &[1])).anomalies,
            vec![AnomalyKind::BadTransport]
        );
    }

    #[test]
    fn plain_handle_with_bit_7_set_is_still_a_plain_handle() {
        // Gadgetbridge notes that non-MLR handles can have the top bit set.
        let (mut t, _) = bring_up_with(23, register_ok(1, 0x91, 0));
        let framed = crate::cobs::encode_frame(&[1, 2, 3]);
        let mut value = vec![0x91];
        value.extend_from_slice(&framed);
        let out = t.on_notification(1, &value);
        assert_eq!(out.frames, vec![Ok(vec![1, 2, 3])]);
        assert!(out.anomalies.is_empty());
        assert_eq!(t.send_gfdi(2, &[5]), vec![vec![0x91, 5]]);
    }

    // ---- the watch closes GFDI: register it again ---------------------------

    #[test]
    fn watch_closing_gfdi_makes_the_transport_register_it_again() {
        let (mut t, _) = bring_up();
        // Data on the old endpoint is queued and unacknowledged.
        assert!(!t.send_gfdi(5, &[1, 2, 3]).is_empty());
        let out = t.on_notification(100, &close_handle_response(1, 0x83));
        assert_eq!(out.writes, vec![multilink::register_request(ID, 1, 2)]);
        assert_eq!(out.anomalies, vec![AnomalyKind::GfdiClosedByWatch]);
        assert!(out.closed.is_none() && !out.link_ready);
        assert!(t.is_up(), "the session layer keeps running");
        // While the handle is closed nothing is written (the frame is held),
        // and the old endpoint's retransmit timer is gone; the reply deadline
        // is armed.
        assert!(t.reopening());
        assert!(t.send_gfdi(101, &[4]).is_empty());
        assert_eq!(t.next_wake(), Some(100 + REPLY_DEADLINE_MS));
        assert!(t.on_tick(5_000).writes.is_empty());
        // The reply grants MLR on another handle: sequence numbers restart,
        // and the held frame goes out first.
        let out = t.on_notification(110, &register_ok(1, 0x84, 1));
        assert!(out.anomalies.is_empty() && out.closed.is_none());
        assert!(!out.link_ready, "bring-up is not repeated");
        assert!(!t.reopening());
        let (header, data) = mlr::parse(&out.writes[0]).unwrap();
        assert_eq!((header.handle, header.seq, data), (4, 0, &[4][..]));
        assert_eq!(out.writes.len(), 1);
        let writes = t.send_gfdi(111, &[9]);
        let (header, data) = mlr::parse(&writes[0]).unwrap();
        assert_eq!((header.handle, header.seq, data), (4, 1, &[9][..]));
    }

    #[test]
    fn re_registration_may_come_back_plain_and_drops_a_half_frame() {
        let (mut t, _) = bring_up_with(23, register_ok(1, PLAIN, 0));
        let framed = crate::cobs::encode_frame(&[1, 2, 3, 4, 5, 6]);
        let mut half = vec![PLAIN];
        half.extend_from_slice(&framed[..4]);
        assert!(t.on_notification(1, &half).frames.is_empty());
        let out = t.on_notification(2, &close_handle_response(1, PLAIN));
        assert_eq!(out.writes, vec![multilink::register_request(ID, 1, 2)]);
        t.on_notification(3, &register_ok(1, 0x32, 0));
        // The old half frame must not be glued to the new stream.
        let fresh = crate::cobs::encode_frame(&[8, 8]);
        let mut value = vec![0x32];
        value.extend_from_slice(&fresh);
        assert_eq!(t.on_notification(4, &value).frames, vec![Ok(vec![8, 8])]);
        // The old handle is gone.
        assert_eq!(
            t.on_notification(5, &half).anomalies,
            vec![AnomalyKind::BadTransport]
        );
    }

    #[test]
    fn re_registration_answers_follow_the_bring_up_rules() {
        let register = |status: u8| {
            let mut v = vec![0, 1];
            v.extend_from_slice(&ID.to_le_bytes());
            v.extend_from_slice(&1u16.to_le_bytes());
            v.push(status);
            v
        };
        let reopened = || {
            let (mut t, _) = bring_up();
            t.on_notification(10, &close_handle_response(1, 0x83));
            t
        };
        // Pending authorisation: keep waiting.
        let mut t = reopened();
        let out = t.on_notification(11, &register(2));
        assert!(out.closed.is_none() && out.writes.is_empty());
        // Rejected: the session is over.
        let mut t = reopened();
        let out = t.on_notification(11, &register(4));
        assert_eq!(
            out.closed,
            Some(CloseReason::RegisterRejected {
                service: 1,
                status: 4
            })
        );
        // Silence: closed after the reply deadline, like any registration.
        let mut t = reopened();
        let out = t.on_tick(10 + REPLY_DEADLINE_MS);
        assert_eq!(
            out.closed,
            Some(CloseReason::Timeout {
                phase: super::super::Phase::BringUp
            })
        );
    }

    #[test]
    fn only_the_gfdi_close_of_our_client_id_triggers_a_re_registration() {
        let (mut t, _) = bring_up();
        // Another service, another client id, a request instead of a response.
        let mut foreign = close_handle_response(1, 0x83);
        foreign[2] ^= 0xFF;
        let mut request = close_handle_response(1, 0x83);
        request[1] = 2;
        for value in [close_handle_response(22, 0x30), foreign, request] {
            let out = t.on_notification(20, &value);
            assert_eq!(
                out.anomalies,
                vec![AnomalyKind::BadTransport],
                "{value:02x?}"
            );
            assert!(out.writes.is_empty() && out.closed.is_none());
        }
        // A second close while the register request is outstanding does not
        // send a second request.
        let out = t.on_notification(30, &close_handle_response(1, 0x83));
        assert_eq!(out.writes.len(), 1);
        let out = t.on_notification(31, &close_handle_response(1, 0x83));
        assert!(out.writes.is_empty());
        assert_eq!(out.anomalies, vec![AnomalyKind::BadTransport]);
    }

    #[test]
    fn a_close_before_bring_up_is_finished_is_not_a_re_registration() {
        let mut t = Transport::new(ID);
        t.start(0, 23);
        let out = t.on_notification(1, &close_handle_response(1, 0x83));
        assert!(out.writes.is_empty());
        assert_eq!(out.anomalies, vec![AnomalyKind::BadTransport]);
    }
}
