//! Shared test support for the client tests: a scripted fake watch and a
//! simulation driver. Everything here is built in code with fake ids; no
//! capture data lives in this file.
//!
//! The fake watch speaks through the same `MlrEndpoint` as the client and
//! through the public codecs, so a mismatch in framing shows up as a failed
//! test, not as two copies of one bug.
#![allow(dead_code)]

pub mod transcript;

use std::collections::{HashMap, VecDeque};

use nimrag_core::capture::{Att, Direction};
use nimrag_core::client::reliable::MlrEndpoint;
use nimrag_core::client::{
    Client, ClientConfig, DstPair, Duties, Event, FileType, Input, Now, Phase, Step,
    GARMIN_EPOCH_UNIX_S,
};
use nimrag_core::cobs::{self, FrameDecoder};
use nimrag_core::crc;
use nimrag_core::gfdi::Message;
use nimrag_core::messages::{self as m, DirEntry};
use nimrag_core::mlr;
use nimrag_core::multilink::{self, Control};

pub const CLIENT_ID: u64 = 0x1234_5678_9ABC_DEF0;
pub const NOTIFY_ATT: u16 = 0x18;
pub const WRITE_ATT: u16 = 0x1B;
pub const REG_HANDLE: u8 = 0x2E;
pub const GFDI_HANDLE_BYTE: u8 = 0x83;
/// GFDI handle the fake watch hands out when it does not grant MLR.
pub const PLAIN_GFDI_HANDLE: u8 = 0x31;
pub const KA_HANDLE: u8 = 0x30;
pub const MTU: u16 = 23;

pub fn test_config() -> ClientConfig {
    test_config_epoch(1)
}

pub fn test_config_epoch(connection_epoch: u32) -> ClientConfig {
    ClientConfig {
        client_id: CLIENT_ID,
        connection_epoch,
        host_name: "TestPhone".into(),
        host_manufacturer: "TestCo".into(),
        host_model: "T-1".into(),
        app_version: 1234,
        capabilities: vec![0x11, 0x22, 0x33],
        first_connect: false,
        archive: true,
        skip: vec![],
        duties: Duties::default(),
    }
}

/// The test config with the given duties switched on.
pub fn duty_config(duties: Duties) -> ClientConfig {
    ClientConfig {
        duties,
        ..test_config()
    }
}

/// A watch that answers phone messages of the companion duties.
pub fn all_duties() -> Duties {
    Duties {
        notifications: true,
        music: true,
        find_phone: true,
    }
}

pub const UNIX_NOW: i64 = 1_800_000_000;

pub fn test_now(mono_ms: u64) -> Now {
    Now {
        mono_ms,
        unix_s: UNIX_NOW + (mono_ms / 1000) as i64,
        utc_offset_s: 3600,
        dst: Some(DstPair {
            start_unix_s: UNIX_NOW + 86_400 * 30,
            end_unix_s: UNIX_NOW + 86_400 * 200,
        }),
    }
}

/// Deterministic pseudo-random bytes (LCG), zeros included.
pub fn lcg_bytes(seed: u32, n: usize) -> Vec<u8> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (x >> 24) as u8 & if x & 7 == 0 { 0 } else { 0xFF }
        })
        .collect()
}

pub fn fake_entry(index: u16, sub_type: u8, size: usize) -> DirEntry {
    DirEntry {
        index,
        data_type: 128,
        sub_type,
        file_number: index + 1000,
        specific_flags: 0,
        flags: 0xA0,
        size: size as u32,
        timestamp: 1_000_000 + index as u32,
    }
}

pub fn fake_device() -> m::DeviceInformation {
    m::DeviceInformation {
        protocol: 150,
        product: 4242,
        unit_id: 0xC0FF_EE01,
        software: 1234,
        max_packet: 400,
        bluetooth_name: "FakeWatch BT".into(),
        device_name: "Fake".into(),
        model_name: "Watch One".into(),
        tail: vec![0, 0],
    }
}

pub fn fake_file_types() -> Vec<FileType> {
    vec![
        FileType {
            data_type: 128,
            sub_type: 4,
            name: "FIT_TYPE_4".into(),
        },
        FileType {
            data_type: 128,
            sub_type: 32,
            name: "FIT_TYPE_32".into(),
        },
    ]
}

// ---- control frames from the watch ------------------------------------------

pub fn register_response(
    client: u64,
    service: u16,
    status: u8,
    handle: u8,
    flags: u8,
    extra: u8,
) -> Vec<u8> {
    let mut v = vec![0, 1];
    v.extend_from_slice(&client.to_le_bytes());
    v.extend_from_slice(&service.to_le_bytes());
    v.push(status);
    match status {
        0 => v.extend_from_slice(&[handle, flags, extra]),
        3 => v.extend_from_slice(&0x2811u16.to_le_bytes()),
        _ => {}
    }
    v
}

pub fn close_all_response(client: u64, status: u8) -> Vec<u8> {
    let mut v = vec![0, 6];
    v.extend_from_slice(&client.to_le_bytes());
    v.extend_from_slice(&[0, 0, status]);
    v
}

fn push_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Smart.device_status_service(8) { remote_device_battery_status_response(3)
/// { status = 1, current_battery_level = `level`, battery_status = `status`,
/// estimated_hours_remaining = `hours` } } as the watch sends it.
pub fn battery_smart(level: u64, status: Option<u64>, hours: Option<u64>) -> Vec<u8> {
    let mut resp = vec![0x08, 0x01, 0x10];
    push_varint(&mut resp, level);
    for (key, value) in [(0x18, status), (0x20, hours)] {
        if let Some(v) = value {
            resp.push(key);
            push_varint(&mut resp, v);
        }
    }
    let mut service = vec![0x1A];
    push_varint(&mut service, resp.len() as u64);
    service.extend_from_slice(&resp);
    let mut smart = vec![0x42];
    push_varint(&mut smart, service.len() as u64);
    smart.extend_from_slice(&service);
    smart
}

/// 5023 BatteryStatus from the watch: `status` is the whole first byte.
pub fn battery_status_message(status: u8, centivolts: u8, percent: u8) -> Message {
    Message::new(5023, vec![status, centivolts, percent, 1, 2, 3, 4])
}

/// A watch response (5000) to a phone request, echoing its txn.
pub fn watch_response(to: &Message, status: u8, body: &[u8]) -> Message {
    let mut payload = to.id.to_le_bytes().to_vec();
    payload.push(status);
    payload.extend_from_slice(body);
    Message {
        id: 5000,
        txn: to.txn,
        payload,
    }
}

// ---- the fake watch -----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// First chunk carries a wrong running CRC.
    BadCrc,
    /// First chunk carries offset 1 instead of 0.
    BadOffset,
    /// The 5002 reply announces 10 bytes less than the data sent.
    OversizeData,
    /// The 5002 reply says "not ready" (status 3).
    NotReady,
    /// The 5002 reply refuses with this status.
    Refuse(u8),
}

#[derive(Debug, Clone)]
pub struct WatchFile {
    pub entry: DirEntry,
    pub data: Vec<u8>,
    pub archived: bool,
}

struct Serving {
    index: u16,
    chunks: VecDeque<Message>,
}

pub struct FakeWatch {
    pub client_id: u64,
    pub mtu: u16,
    pub device: m::DeviceInformation,
    pub capabilities: Vec<u8>,
    pub file_types: Vec<FileType>,
    pub files: Vec<WatchFile>,
    /// Data bytes per 5004 chunk.
    pub chunk_size: usize,
    pub faults: HashMap<u16, VecDeque<Fault>>,
    /// Behaviour switches.
    pub grant_reliable: bool,
    pub register_status: HashMap<u16, u8>,
    pub close_all_status: u8,
    pub mute: bool,
    pub auto_config_after_info_ack: bool,
    pub grant_sync_on_protobuf: bool,
    pub nak_filter: bool,
    /// Answer the 5002 / 5008 of this index with a NAK.
    pub nak_download: Option<u16>,
    pub nak_archive: Option<u16>,
    /// Answer every 5030 carrying this system event with a NAK (0 also
    /// refuses the SYNC_COMPLETE at the end of each cycle).
    pub nak_system_event: Option<u8>,
    /// Leave every 5030 carrying this system event unanswered (the test
    /// answers it later with `watch_response`).
    pub silent_system_event: Option<u8>,
    /// After acknowledging the 5030 carrying this system event, send a 5037.
    pub sync_after_system_event: Option<u8>,
    /// Answer the 5002 of this index with 5054 compressed data.
    pub compress_file: Option<u16>,
    /// Withhold file chunks once this many have been sent on this link.
    pub stall_after: Option<usize>,
    /// Answer the phone's battery request (5043, device_status 8) with a NAK.
    pub nak_battery_request: bool,
    /// Do not answer the phone's battery request at all.
    pub silent_battery_request: bool,
    /// Smart bytes of the 5044 sent after the ACK of a battery request
    /// (`None`: no 5044).
    pub battery_response: Option<Vec<u8>>,
    /// Leave every 5033 / 5035 / 5049 and find-my-watch request unanswered.
    pub silent_duties: bool,
    /// Answer the phone's 5033 / 5035 / 5049 of this id with a NAK.
    pub nak_duty: Option<u16>,
    /// Transfer status the watch answers a 5035 chunk with (0 = OK).
    pub data_status: u8,
    /// Smart bytes of the 5044 sent after the ACK of a find-my-watch request.
    pub find_watch_response: Option<Vec<u8>>,
    /// What the phone sent on this link.
    pub phone_msgs: Vec<Message>,
    pub phone_raw: Vec<Vec<u8>>,
    pub archived_indices: Vec<u16>,
    file_chunks_sent: usize,
    endpoint: Option<MlrEndpoint>,
    /// Handle of a plain (mode 0) GFDI registration.
    plain_gfdi: Option<u8>,
    cobs: FrameDecoder,
    serving: Option<Serving>,
}

impl FakeWatch {
    pub fn new() -> Self {
        Self {
            client_id: CLIENT_ID,
            mtu: MTU,
            device: fake_device(),
            capabilities: vec![0xAA, 0xBB],
            file_types: fake_file_types(),
            files: Vec::new(),
            chunk_size: 380,
            faults: HashMap::new(),
            grant_reliable: true,
            register_status: HashMap::new(),
            close_all_status: 1,
            mute: false,
            auto_config_after_info_ack: true,
            grant_sync_on_protobuf: true,
            nak_filter: false,
            nak_download: None,
            nak_archive: None,
            nak_system_event: None,
            silent_system_event: None,
            sync_after_system_event: None,
            compress_file: None,
            stall_after: None,
            nak_battery_request: false,
            silent_battery_request: false,
            battery_response: Some(battery_smart(87, Some(1), Some(120))),
            silent_duties: false,
            nak_duty: None,
            data_status: 0,
            find_watch_response: None,
            file_chunks_sent: 0,
            phone_msgs: Vec::new(),
            phone_raw: Vec::new(),
            archived_indices: Vec::new(),
            endpoint: None,
            plain_gfdi: None,
            cobs: FrameDecoder::new(),
            serving: None,
        }
    }

    pub fn add_file(&mut self, entry: DirEntry, data: Vec<u8>) {
        let mut entry = entry;
        entry.size = data.len() as u32;
        self.files.push(WatchFile {
            entry,
            data,
            archived: false,
        });
    }

    /// New radio link: everything link-level starts over, files persist.
    pub fn reset_link(&mut self) {
        self.endpoint = None;
        self.plain_gfdi = None;
        self.cobs = FrameDecoder::new();
        self.serving = None;
        self.file_chunks_sent = 0;
        self.phone_msgs.clear();
        self.phone_raw.clear();
    }

    pub fn pending_files(&self) -> Vec<&WatchFile> {
        self.files.iter().filter(|f| !f.archived).collect()
    }

    pub fn next_deadline(&self) -> Option<u64> {
        self.endpoint.as_ref().and_then(|e| e.next_deadline())
    }

    pub fn poll(&mut self, now: u64) -> Vec<Vec<u8>> {
        self.endpoint
            .as_mut()
            .map(|e| e.poll(now))
            .unwrap_or_default()
    }

    pub fn endpoint(&self) -> Option<&MlrEndpoint> {
        self.endpoint.as_ref()
    }

    /// Queue a GFDI message from the watch; returns the packets to deliver.
    pub fn send(&mut self, now: u64, msg: &Message) -> Vec<Vec<u8>> {
        let framed = cobs::encode_frame(&msg.encode());
        self.send_raw_frame(now, &framed)
    }

    /// Send already COBS-framed bytes (to inject malformed frames). In plain
    /// mode: `[handle, up to mtu - 4 bytes]` per packet.
    pub fn send_raw_frame(&mut self, now: u64, framed: &[u8]) -> Vec<Vec<u8>> {
        if let (Some(handle), None) = (self.plain_gfdi, self.endpoint.as_ref()) {
            return framed
                .chunks(self.mtu as usize - 4)
                .map(|c| [&[handle][..], c].concat())
                .collect();
        }
        match self.endpoint.as_mut() {
            Some(e) => e.send(now, framed),
            None => Vec::new(),
        }
    }

    /// The watch closes its GFDI handle on its own: forget the stream and
    /// return the close-handle response (type 3) it sends.
    pub fn close_gfdi(&mut self) -> Vec<u8> {
        let handle = match (self.endpoint.take(), self.plain_gfdi.take()) {
            (Some(_), _) => GFDI_HANDLE_BYTE,
            (None, Some(handle)) => handle,
            (None, None) => panic!("GFDI is not registered"),
        };
        self.cobs = FrameDecoder::new();
        self.serving = None;
        let mut v = vec![0, 3];
        v.extend_from_slice(&self.client_id.to_le_bytes());
        v.extend_from_slice(&[1, 0, handle, 0]);
        v
    }

    pub fn device_information(&self) -> Message {
        Message::new(5024, m::build_device_information(&self.device))
    }

    pub fn configuration(&self) -> Message {
        Message::new(5050, m::build_configuration(&self.capabilities))
    }

    /// The phone wrote `value`; returns notifications to deliver, in order.
    pub fn on_write(&mut self, now: u64, value: &[u8]) -> Vec<Vec<u8>> {
        self.phone_raw.push(value.to_vec());
        if self.mute || value.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        if value[0] == 0 {
            self.on_control(value, &mut out);
        } else if Some(value[0]) == self.plain_gfdi {
            self.on_gfdi_bytes(now, &value[1..], &mut out);
        } else if value[0] & 0x80 != 0 {
            self.on_mlr(now, value, &mut out);
        } else if value[0] == REG_HANDLE {
            let page = value.get(1).copied().unwrap_or(0);
            if page < 5 {
                out.push(vec![REG_HANDLE, page, 1, 2, 3]);
            } else {
                out.push(vec![REG_HANDLE, 0xFF, page]);
            }
        } else if value[0] == KA_HANDLE {
            out.push(vec![KA_HANDLE, 0x02, 0x00]);
        }
        out
    }

    fn on_control(&mut self, value: &[u8], out: &mut Vec<Vec<u8>>) {
        match multilink::parse_control(value) {
            Some(Control::CloseAllRequest { client_id }) => {
                out.push(close_all_response(client_id, self.close_all_status));
            }
            Some(Control::RegisterRequest {
                client_id,
                service,
                mode,
            }) => {
                let status = self.register_status.get(&service).copied().unwrap_or(0);
                let reliable = mode == 2 && self.grant_reliable;
                let (handle, flags) = match service {
                    4 => (REG_HANDLE, 0),
                    1 if reliable => (GFDI_HANDLE_BYTE, 1),
                    1 => (PLAIN_GFDI_HANDLE, 0),
                    _ => (KA_HANDLE, 0),
                };
                out.push(register_response(
                    client_id, service, status, handle, flags, 1,
                ));
                if service == 1 && status == 0 {
                    // A new registration starts a new byte stream.
                    self.cobs = FrameDecoder::new();
                    self.endpoint =
                        reliable.then(|| MlrEndpoint::new(GFDI_HANDLE_BYTE & 7, self.mtu));
                    self.plain_gfdi = (!reliable).then_some(PLAIN_GFDI_HANDLE);
                }
            }
            _ => {}
        }
    }

    fn on_mlr(&mut self, now: u64, value: &[u8], out: &mut Vec<Vec<u8>>) {
        let Some((header, data)) = mlr::parse(value) else {
            return;
        };
        let Some(ep) = self.endpoint.as_mut() else {
            return;
        };
        let received = ep.receive(now, header, data);
        out.extend(received.writes);
        self.on_gfdi_bytes(now, &received.data, out);
    }

    /// In-order GFDI stream bytes from the phone, reliable or plain.
    fn on_gfdi_bytes(&mut self, now: u64, bytes: &[u8], out: &mut Vec<Vec<u8>>) {
        if bytes.is_empty() {
            return;
        }
        let frames = self.cobs.push(bytes);
        for frame in frames {
            let frame = frame.expect("phone sent a valid COBS frame");
            let msg = Message::parse(&frame).expect("phone sent a valid GFDI message");
            self.phone_msgs.push(msg.clone());
            for reply in self.handle_phone_message(&msg) {
                out.extend(self.send(now, &reply));
            }
        }
    }

    /// Scripted watch behaviour: what the watch says in answer.
    fn handle_phone_message(&mut self, msg: &Message) -> Vec<Message> {
        if msg.id == 5000 {
            return self.on_phone_response(msg);
        }
        match msg.id {
            5050 | 5030 | 5008 | 5007 | 5043 | 5031 => self.on_phone_request(msg),
            5002 => self.serve_download(msg),
            5033 | 5035 | 5049 => self.on_duty_message(msg),
            _ => Vec::new(),
        }
    }

    /// 5033 / 5035 / 5049 from the phone: an ACK (5035 with its transfer status).
    fn on_duty_message(&mut self, msg: &Message) -> Vec<Message> {
        if self.silent_duties {
            return Vec::new();
        }
        if self.nak_duty == Some(msg.id) {
            return vec![watch_response(msg, 1, &[])];
        }
        if msg.id == 5035 {
            return vec![watch_response(msg, 0, &[self.data_status])];
        }
        vec![watch_response(msg, 0, &[])]
    }

    fn on_phone_response(&mut self, msg: &Message) -> Vec<Message> {
        let r = msg.as_response().expect("response");
        match r.request_id {
            5024 if self.auto_config_after_info_ack => vec![self.configuration()],
            5004 => {
                // Chunk ACK: send the next chunk on status 0, stop otherwise.
                let status = r.body.first().copied().unwrap_or(255);
                let stalled = |sent: usize, index: u16, limit: Option<usize>| {
                    index != 0 && limit.is_some_and(|l| sent >= l)
                };
                match self.serving.as_mut() {
                    Some(s)
                        if status == 0
                            && stalled(self.file_chunks_sent, s.index, self.stall_after) =>
                    {
                        Vec::new()
                    }
                    Some(s) if status == 0 => {
                        let next: Vec<Message> = s.chunks.pop_front().into_iter().collect();
                        if s.index != 0 {
                            self.file_chunks_sent += next.len();
                        }
                        next
                    }
                    _ => {
                        self.serving = None;
                        Vec::new()
                    }
                }
            }
            _ => Vec::new(),
        }
    }

    fn on_phone_request(&mut self, msg: &Message) -> Vec<Message> {
        match msg.id {
            5031 => vec![watch_response(
                msg,
                0,
                &m::build_supported_file_types(&self.file_types),
            )],
            5007 if self.nak_filter => vec![watch_response(msg, 1, &[])],
            5030 if self.silent_system_event.is_some()
                && msg.payload.first().copied() == self.silent_system_event =>
            {
                Vec::new()
            }
            5030 if self.nak_system_event.is_some()
                && msg.payload.first().copied() == self.nak_system_event =>
            {
                vec![watch_response(msg, 1, &[])]
            }
            5030 if self.sync_after_system_event.is_some()
                && msg.payload.first().copied() == self.sync_after_system_event =>
            {
                vec![watch_response(msg, 0, &[]), self.sync_request(1)]
            }
            5007 => vec![watch_response(msg, 0, &[0])],
            5008 => {
                let (index, flags) = m::parse_set_file_flags(&msg.payload).expect("5008 payload");
                if self.nak_archive == Some(index) {
                    return vec![watch_response(msg, 1, &[])];
                }
                self.archived_indices.push(index);
                for f in self.files.iter_mut().filter(|f| f.entry.index == index) {
                    f.archived = true;
                }
                let mut body = vec![0];
                body.extend_from_slice(&index.wrapping_sub(1).to_le_bytes());
                body.push(flags);
                vec![watch_response(msg, 0, &body)]
            }
            5043 => {
                let (h, body) = m::parse_protobuf(&msg.payload).expect("protobuf");
                if body == m::BATTERY_REQUEST_PROTOBUF {
                    if self.silent_battery_request {
                        return Vec::new();
                    }
                    if self.nak_battery_request {
                        return vec![watch_response(msg, 1, &[])];
                    }
                    let mut out =
                        vec![watch_response(msg, 0, &m::build_protobuf_ack(h.request_id))];
                    out.extend(
                        self.battery_response
                            .iter()
                            .map(|b| Message::new(5044, m::build_protobuf(h.request_id, b))),
                    );
                    return out;
                }
                if body == nimrag_core::duties::find::FIND_WATCH_START_PROTOBUF
                    || body == nimrag_core::duties::find::FIND_WATCH_CANCEL_PROTOBUF
                {
                    if self.silent_duties {
                        return Vec::new();
                    }
                    if self.nak_duty == Some(5043) {
                        return vec![watch_response(msg, 1, &[])];
                    }
                    let mut out =
                        vec![watch_response(msg, 0, &m::build_protobuf_ack(h.request_id))];
                    out.extend(
                        self.find_watch_response
                            .iter()
                            .map(|b| Message::new(5044, m::build_protobuf(h.request_id, b))),
                    );
                    return out;
                }
                let mut out = vec![watch_response(msg, 0, &m::build_protobuf_ack(h.request_id))];
                if self.grant_sync_on_protobuf {
                    // The watch's answer (5044) and then its 5037 MANUAL.
                    out.push(Message::new(
                        5044,
                        m::build_protobuf(h.request_id, &[0x6A, 0x04, 0x12, 0x02, 0x08, 0x01]),
                    ));
                    out.push(self.sync_request(0));
                }
                out
            }
            _ => vec![watch_response(msg, 0, &[])],
        }
    }

    /// 5009 FileAvailable for a file this watch holds: its directory entry.
    pub fn file_available(&self, index: u16) -> Message {
        let file = self
            .files
            .iter()
            .find(|f| f.entry.index == index)
            .expect("the watch holds this file");
        Message::new(5009, file.entry.encode())
    }

    pub fn sync_request(&self, option: u8) -> Message {
        Message::new(
            5037,
            m::build_sync_request(&m::SyncRequest {
                option,
                mask: vec![0x1F, 0x00],
            }),
        )
    }

    fn directory_bytes(&self) -> Vec<u8> {
        let mut entries: Vec<DirEntry> = self
            .pending_files()
            .iter()
            .map(|f| f.entry.clone())
            .collect();
        if entries.is_empty() {
            // An empty listing still carries one all-zero entry (32 bytes).
            entries.push(DirEntry {
                index: 0,
                data_type: 0,
                sub_type: 0,
                file_number: 0,
                specific_flags: 0,
                flags: 0,
                size: 0,
                timestamp: 0,
            });
        }
        m::build_directory(&entries)
    }

    fn serve_download(&mut self, msg: &Message) -> Vec<Message> {
        let req = m::parse_download_request(&msg.payload).expect("5002 payload");
        if self.nak_download == Some(req.index) {
            return vec![watch_response(msg, 1, &[])];
        }
        if self.compress_file == Some(req.index) {
            let reply = watch_response(
                msg,
                0,
                &m::build_download_reply(&m::DownloadReply {
                    status: 0,
                    size: 100,
                }),
            );
            return vec![reply, Message::new(5054, vec![0, 0, 0, 0, 0, 0, 0, 1])];
        }
        let data = if req.index == 0 {
            Some(self.directory_bytes())
        } else {
            self.files
                .iter()
                .find(|f| f.entry.index == req.index)
                .map(|f| f.data.clone())
        };
        let Some(data) = data else {
            return vec![watch_response(
                msg,
                0,
                &m::build_download_reply(&m::DownloadReply { status: 1, size: 0 }),
            )];
        };
        let fault = self.faults.get_mut(&req.index).and_then(|q| q.pop_front());
        let mut size = data.len() as u32;
        match fault {
            Some(Fault::NotReady) => {
                return vec![watch_response(
                    msg,
                    0,
                    &m::build_download_reply(&m::DownloadReply { status: 3, size: 0 }),
                )]
            }
            Some(Fault::Refuse(status)) => {
                return vec![watch_response(
                    msg,
                    0,
                    &m::build_download_reply(&m::DownloadReply { status, size: 0 }),
                )]
            }
            Some(Fault::OversizeData) => size -= 10,
            _ => {}
        }
        let mut chunks = VecDeque::new();
        let mut running = 0u16;
        let mut offset = 0usize;
        for piece in data.chunks(self.chunk_size) {
            running = crc::update(running, piece);
            chunks.push_back(Message::new(
                5004,
                m::build_file_chunk(&m::FileChunk {
                    flags: 0,
                    crc: running,
                    offset: offset as u32,
                    data: piece.to_vec(),
                }),
            ));
            offset += piece.len();
        }
        match fault {
            Some(Fault::BadCrc) => {
                if let Some(first) = chunks.front_mut() {
                    first.payload[1] ^= 0x55;
                }
            }
            Some(Fault::BadOffset) => {
                if let Some(first) = chunks.front_mut() {
                    first.payload[3] = 1;
                }
            }
            _ => {}
        }
        let first = chunks.pop_front();
        if req.index != 0 {
            self.file_chunks_sent += first.iter().len();
        }
        self.serving = Some(Serving {
            index: req.index,
            chunks,
        });
        let mut out = vec![watch_response(
            msg,
            0,
            &m::build_download_reply(&m::DownloadReply { status: 0, size }),
        )];
        out.extend(first);
        out
    }
}

// ---- the simulation driver --------------------------------------------------------

/// A client wired to a fake watch with zero latency and a virtual clock.
pub struct Sim {
    pub client: Client,
    pub watch: FakeWatch,
    pub t: u64,
    pub events: Vec<Event>,
    /// Both directions, in order, as the ATT values on the wire.
    pub trace: Vec<(Direction, Vec<u8>)>,
    /// Packets the phone writes that are lost on air.
    pub drop_phone_writes: usize,
    /// Packets the watch sends that are lost on air.
    pub drop_watch_packets: usize,
    /// Lose the watch's ACK-only packets (data packets still arrive).
    pub drop_watch_acks: bool,
    /// Client writes in order, with the time they were made.
    pub phone_writes: Vec<(u64, Vec<u8>)>,
    pub wake: Option<u64>,
    /// Transfer ids the caller confirmed with `FileStored`.
    pub stored: Vec<u64>,
    pub epoch: u32,
    pub unix_s_override: Option<i64>,
    /// `Some(x)` replaces the DST pair given to the client with `x`.
    pub dst_override: Option<Option<DstPair>>,
    inbox: VecDeque<Vec<u8>>,
}

impl Sim {
    pub fn new() -> Self {
        Self::with(Client::new(test_config()), FakeWatch::new())
    }

    pub fn with(client: Client, watch: FakeWatch) -> Self {
        Self {
            client,
            watch,
            t: 0,
            events: Vec::new(),
            trace: Vec::new(),
            drop_phone_writes: 0,
            drop_watch_packets: 0,
            drop_watch_acks: false,
            phone_writes: Vec::new(),
            wake: None,
            stored: Vec::new(),
            epoch: 1,
            unix_s_override: None,
            dst_override: None,
            inbox: VecDeque::new(),
        }
    }

    pub fn now(&self) -> Now {
        let mut n = test_now(self.t);
        if let Some(u) = self.unix_s_override {
            n.unix_s = u;
        }
        if let Some(d) = self.dst_override {
            n.dst = d;
        }
        n
    }

    /// Start a new GATT connection with a fresh client on the same watch.
    pub fn reconnect(&mut self) {
        self.epoch += 1;
        self.client = Client::new(test_config_epoch(self.epoch));
        self.watch.reset_link();
        self.inbox.clear();
        self.wake = None;
    }

    /// Feed one input, then let the wire settle. Returns the first step.
    pub fn input(&mut self, input: Input) -> Step {
        if let Input::FileStored { transfer_id } = &input {
            self.stored.push(*transfer_id);
        }
        let step = self.client.handle(self.now(), input);
        self.apply(&step);
        self.settle();
        step
    }

    pub fn connect(&mut self) -> Step {
        self.input(Input::Connected { att_mtu: MTU })
    }

    /// Connect and run the full handshake with the watch's scripted side.
    pub fn handshake(&mut self) {
        self.connect();
        let info = self.watch.device_information();
        self.watch_sends(&info);
        assert_eq!(
            self.client.phase(),
            Phase::Ready,
            "handshake did not finish"
        );
    }

    pub fn watch_sends(&mut self, msg: &Message) {
        let packets = self.watch.send(self.t, msg);
        for p in packets {
            self.queue_notification(p);
        }
        self.settle();
    }

    /// Deliver one raw notification value to the client now.
    pub fn deliver(&mut self, value: Vec<u8>) -> Step {
        self.trace_notification(&value);
        self.input(Input::Notification { value })
    }

    fn queue_notification(&mut self, value: Vec<u8>) {
        if self.drop_watch_packets > 0 {
            self.drop_watch_packets -= 1;
            return;
        }
        if self.drop_watch_acks && value.len() == 2 && value[0] & 0x80 != 0 {
            return;
        }
        self.inbox.push_back(value);
    }

    fn trace_notification(&mut self, value: &[u8]) {
        self.trace.push((Direction::Received, value.to_vec()));
    }

    fn apply(&mut self, step: &Step) {
        self.events.extend(step.events.iter().cloned());
        self.wake = step.wake_at_mono_ms;
        for w in &step.writes {
            self.phone_writes.push((self.t, w.clone()));
            self.trace.push((Direction::Sent, w.clone()));
            if self.drop_phone_writes > 0 {
                self.drop_phone_writes -= 1;
                continue;
            }
            let replies = self.watch.on_write(self.t, w);
            for r in replies {
                self.queue_notification(r);
            }
        }
    }

    /// Deliver queued watch notifications until the wire is quiet.
    pub fn settle(&mut self) {
        while let Some(v) = self.inbox.pop_front() {
            self.trace_notification(&v);
            let step = self
                .client
                .handle(self.now(), Input::Notification { value: v });
            self.apply(&step);
        }
    }

    /// Advance the virtual clock, firing client and watch timers on the way.
    pub fn advance(&mut self, ms: u64) {
        let target = self.t + ms;
        let mut iterations = 0;
        loop {
            iterations += 1;
            assert!(
                iterations < 100_000,
                "timer livelock while advancing the clock"
            );
            let next = [self.wake, self.watch.next_deadline()]
                .into_iter()
                .flatten()
                .min();
            match next {
                Some(n) if n <= target => {
                    self.t = self.t.max(n);
                    for p in self.watch.poll(self.t) {
                        self.queue_notification(p);
                    }
                    let step = self.client.handle(self.now(), Input::Tick);
                    self.apply(&step);
                    self.settle();
                }
                _ => break,
            }
        }
        self.t = target;
    }

    // ---- views for assertions ---------------------------------------------------

    pub fn phone_ids(&self) -> Vec<u16> {
        self.watch.phone_msgs.iter().map(|m| m.id).collect()
    }

    pub fn phone_requests(&self, id: u16) -> Vec<&Message> {
        self.watch
            .phone_msgs
            .iter()
            .filter(|m| m.id == id)
            .collect()
    }

    /// Phone ACKs to watch requests of `request_id` (5000 messages).
    pub fn phone_responses(&self, request_id: u16) -> Vec<Message> {
        self.watch
            .phone_msgs
            .iter()
            .filter(|m| m.as_response().is_some_and(|r| r.request_id == request_id))
            .cloned()
            .collect()
    }

    pub fn events_of<F: Fn(&Event) -> bool>(&self, f: F) -> Vec<&Event> {
        self.events.iter().filter(|e| f(e)).collect()
    }

    pub fn downloaded(&self) -> Vec<(u64, DirEntry, Vec<u8>)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::FileDownloaded {
                    transfer_id,
                    entry,
                    data,
                } => Some((*transfer_id, entry.clone(), data.clone())),
                _ => None,
            })
            .collect()
    }

    pub fn closed_reason(&self) -> Option<nimrag_core::client::CloseReason> {
        self.events.iter().find_map(|e| match e {
            Event::Closed { reason } => Some(*reason),
            _ => None,
        })
    }

    pub fn trace_atts(&self) -> Vec<(Direction, Att)> {
        self.trace
            .iter()
            .map(|(d, v)| {
                let att = match d {
                    Direction::Sent => Att::Write {
                        handle: WRITE_ATT,
                        value: v.clone(),
                        command: true,
                    },
                    Direction::Received => Att::Notify {
                        handle: NOTIFY_ATT,
                        value: v.clone(),
                        indication: false,
                    },
                };
                (*d, att)
            })
            .collect()
    }
}

/// Garmin-epoch seconds for a Unix time.
pub fn garmin(unix_s: i64) -> u32 {
    (unix_s - GARMIN_EPOCH_UNIX_S).max(0) as u32
}
