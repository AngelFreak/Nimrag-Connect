//! Message-level transcripts of a captured watch link, and the replay driver
//! that plays one session against `Client::handle` (stage 2a plan, section 5).
//!
//! The transcript is built at run time from a private capture and lives only
//! in memory: nothing derived from a capture is written to a committed file.
#![allow(dead_code)]

use std::collections::{BTreeMap, VecDeque};

use nimrag_core::capture::{self, Direction, L2capReassembler, ATT_CID};
use nimrag_core::client::reliable::MlrEndpoint;
use nimrag_core::client::{Client, ClientConfig, Event, Input, Now, Phase};
use nimrag_core::cobs::{self, FrameDecoder};
use nimrag_core::gfdi::Message;
use nimrag_core::link::{Event as LinkEvent, InfoPage, LinkDecoder};
use nimrag_core::messages as m;
use nimrag_core::mlr;
use nimrag_core::multilink::{self, Control};

use super::{close_all_response, register_response};

// ---- transcript ----------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Kind {
    CloseAll,
    CloseAllResp {
        status: u8,
    },
    Register {
        service: u16,
        mode: u8,
    },
    RegisterResp {
        service: u16,
        status: u8,
        handle: u8,
        flags: u8,
        extra: u8,
    },
    /// Teardown of a service handle (phone request, watch response or
    /// invalid-handle notice).
    CloseHandle {
        service: u16,
        handle: u8,
    },
    CloseHandleResp {
        service: u16,
        handle: u8,
    },
    /// Selector written to the registration handle (bytes after the handle).
    Info(Vec<u8>),
    /// Reply on the registration handle (bytes after the handle).
    InfoReply(Vec<u8>),
    /// Bytes on a registered non-GFDI service (after the handle).
    Service {
        service: u16,
        data: Vec<u8>,
    },
    Gfdi(Message),
    /// Something the decoder flagged; the harness cannot classify it.
    Other(String),
}

#[derive(Debug, Clone)]
pub struct Item {
    pub ts_us: i64,
    pub t_ms: u64,
    pub from_phone: bool,
    pub kind: Kind,
}

#[derive(Debug)]
pub struct SessionTranscript {
    pub name: String,
    pub client_id: u64,
    pub t0_us: i64,
    pub mtu: u16,
    pub items: Vec<Item>,
    /// Files the app downloaded in this session (index != 0), in order.
    pub files: Vec<(u16, Vec<u8>, bool)>,
}

/// Parse a btsnoop file into one transcript per (client id, close-all).
/// Sessions plus decoder findings that belong to no session.
pub struct Loaded {
    pub sessions: Vec<SessionTranscript>,
    pub stray: Vec<String>,
}

pub fn load_sessions(label: &str, bytes: &[u8]) -> Result<Loaded, String> {
    let records = capture::parse_btsnoop(bytes).map_err(|e| format!("btsnoop: {e:?}"))?;
    let mut l2 = L2capReassembler::new();
    let mut dec = LinkDecoder::new();
    let mut mtu = 23u16;
    let mut sessions: Vec<SessionTranscript> = Vec::new();
    let mut stray: Vec<String> = Vec::new();
    let first_ts = records.first().map_or(0, |r| r.timestamp_us);
    let mut current: BTreeMap<u64, usize> = BTreeMap::new();
    for rec in &records {
        let Some(pdu) = l2.push(rec) else { continue };
        if pdu.cid != ATT_CID {
            continue;
        }
        let Some(att) = capture::parse_att(&pdu.payload) else {
            continue;
        };
        for ev in dec.push(pdu.direction, &att) {
            let ts = pdu.timestamp_us;
            let from_phone = |d: Direction| d == Direction::Sent;
            let (client, item): (Option<u64>, Option<(bool, Kind)>) = match ev {
                LinkEvent::MtuExchange { direction, mtu: v } => {
                    if direction == Direction::Received {
                        mtu = v;
                    }
                    (None, None)
                }
                LinkEvent::Control {
                    direction,
                    client,
                    control,
                } => {
                    let kind = match control {
                        Control::CloseAllRequest { .. } => Kind::CloseAll,
                        Control::CloseAllResponse { status, .. } => Kind::CloseAllResp { status },
                        Control::RegisterRequest { service, mode, .. } => {
                            Kind::Register { service, mode }
                        }
                        Control::RegisterResponse {
                            service,
                            status,
                            handle,
                            flags,
                            extra,
                            ..
                        } => Kind::RegisterResp {
                            service,
                            status,
                            handle: handle.unwrap_or(0),
                            flags: flags.unwrap_or(0),
                            extra: extra.unwrap_or(0),
                        },
                        Control::CloseHandleRequest {
                            service, handle, ..
                        } => Kind::CloseHandle { service, handle },
                        Control::CloseHandleResponse {
                            service, handle, ..
                        } => Kind::CloseHandleResp { service, handle },
                        other => Kind::Other(format!("control {other:?}")),
                    };
                    (client, Some((from_phone(direction), kind)))
                }
                LinkEvent::InfoPage {
                    direction,
                    client,
                    page,
                } => {
                    let kind = match (direction, page) {
                        (Direction::Sent, InfoPage::Selector(v)) => Kind::Info(v),
                        (_, p) => Kind::InfoReply(info_reply_bytes(&p)),
                    };
                    (Some(client.client), Some((from_phone(direction), kind)))
                }
                LinkEvent::ServiceData {
                    direction,
                    client,
                    service,
                    data,
                } => (
                    Some(client.client),
                    Some((from_phone(direction), Kind::Service { service, data })),
                ),
                LinkEvent::Gfdi {
                    direction,
                    client,
                    message,
                } => (
                    Some(client.client),
                    Some((from_phone(direction), Kind::Gfdi(message))),
                ),
                LinkEvent::GfdiError {
                    direction,
                    client,
                    error,
                    ..
                } => (
                    Some(client.client),
                    Some((
                        from_phone(direction),
                        Kind::Other(format!("gfdi error {error}")),
                    )),
                ),
                LinkEvent::FileDownloaded {
                    client,
                    index,
                    data,
                    crc_ok,
                    ..
                } => {
                    if index != 0 {
                        if let Some(&i) = current.get(&client.client) {
                            sessions[i].files.push((index, data, crc_ok));
                        }
                    }
                    (None, None)
                }
                LinkEvent::DescriptorWrite { .. }
                | LinkEvent::MlrDuplicate { .. }
                | LinkEvent::Directory { .. } => (None, None),
                LinkEvent::MlrOutOfOrder {
                    client,
                    seq,
                    expected,
                    ..
                } => (
                    Some(client.client),
                    Some((
                        false,
                        Kind::Other(format!("MLR out of order: seq {seq}, expected {expected}")),
                    )),
                ),
                LinkEvent::Unknown {
                    direction,
                    att_handle,
                    reason,
                    ..
                } => {
                    stray.push(format!(
                        "decoder Unknown at {} ms ({direction:?}, att {att_handle:#x}): {reason}",
                        ((ts - first_ts).max(0)) / 1000
                    ));
                    (None, None)
                }
            };
            let (Some(client), Some((phone, kind))) = (client, item) else {
                continue;
            };
            if phone && matches!(kind, Kind::CloseAll) {
                sessions.push(SessionTranscript {
                    name: format!("{label}#{}", sessions.len() + 1),
                    client_id: client,
                    t0_us: ts,
                    mtu,
                    items: Vec::new(),
                    files: Vec::new(),
                });
                current.insert(client, sessions.len() - 1);
            }
            let Some(&i) = current.get(&client) else {
                continue;
            };
            let t0 = sessions[i].t0_us;
            sessions[i].items.push(Item {
                ts_us: ts,
                t_ms: ((ts - t0).max(0) / 1000) as u64,
                from_phone: phone,
                kind,
            });
        }
    }
    for s in &mut sessions {
        s.mtu = mtu;
    }
    Ok(Loaded { sessions, stray })
}

/// A stand-in reply body for an info page: only the page byte matters to the
/// client, and the real bytes (identifiers) are never kept.
fn info_reply_bytes(p: &InfoPage) -> Vec<u8> {
    match p {
        InfoPage::SupportedServices(_) => vec![0, 0, 0],
        InfoPage::AdvertisingData(_) => vec![1, 0, 0, 0],
        InfoPage::Version { .. } => vec![2, 1, 2, 2],
        InfoPage::ProductInfo { .. } => vec![3, 0, 0, 0, 0],
        InfoPage::IdentityAddress => vec![4, 0, 0, 0, 0, 0, 0, 0],
        InfoPage::ServiceRevision { service, .. } => {
            let mut v = vec![5];
            v.extend_from_slice(&service.to_le_bytes());
            v.push(0);
            v
        }
        InfoPage::Unsupported(page) => vec![0xFF, *page],
        InfoPage::Selector(v) | InfoPage::Raw(v) => v.clone(),
    }
}

// ---- classification ------------------------------------------------------------------------

/// The allowed-difference list of the stage 2a plan, section 5, as data.
/// The test prints which entries a session used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Allowed {
    InfoPage5NotSent,
    /// NFC and real-time service registrations, including the app's data on
    /// those handles.
    NfcAndRealtimeRegistrationsNotSent,
    SettingsFive026NotSent,
    HostForegroundNotSent,
    FeatureCapabilitiesNotSent,
    Notifications5033_5035_5049NotSent,
    ProtobufAnswers5044NotSent,
    UploadNotSent,
    WeatherNotSent,
    TxnAndProtobufIdsRewritten,
    TimeWithinTwoSeconds,
    LaneOrderDiffers,
    RegistrationOrder,
    ProtobufAckTail,
    UnsupportedInsteadOfAck(u16),
    WatchSyncAfterForegroundStillDelivered,
    /// The app's later close-handle (and the watch's response) for a handle
    /// it registered itself for a not-sent service (NFC 2, real-time 6-21, 25).
    AppCloseOfNotSentHandle(u16),
    /// App-initiated 5043 on Smart field 21 (music) or 42 (RealtimeSettings).
    AppProtobufField(u64),
}

/// Why a transcript item cannot be placed (stop rule 8.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unclassified {
    pub t_ms: u64,
    pub what: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Bring-up lane, compared byte-exact.
    BringUp,
    Handshake,
    Sync,
    /// The manual-sync 5043.
    ManualSync,
    /// Not sent by the client; allowed by the named entry.
    NotSent(Allowed),
}

const REALTIME_SERVICES: [u16; 17] = [
    6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 25,
];

/// Smart service number and its first inner field: `13{1..}` -> (13, 1).
fn smart_fields(body: &[u8]) -> Option<(u64, u64)> {
    fn varint(b: &[u8], i: &mut usize) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *b.get(*i)?;
            *i += 1;
            v |= ((byte & 0x7F) as u64) << shift;
            if byte & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
    let mut i = 0;
    let key = varint(body, &mut i)?;
    let len = varint(body, &mut i)? as usize;
    let inner = body.get(i..i + len)?;
    let mut j = 0;
    let inner_key = varint(inner, &mut j)?;
    Some((key >> 3, inner_key >> 3))
}

/// Setting ids of a 5026 payload: `u8 n`, then n x (`u8 id, u8 len, value`).
fn tlv_ids(p: &[u8]) -> Option<Vec<u8>> {
    let n = *p.first()? as usize;
    let mut pos = 1;
    let mut ids = Vec::new();
    for _ in 0..n {
        let id = *p.get(pos)?;
        let len = *p.get(pos + 1)? as usize;
        p.get(pos + 2..pos + 2 + len)?;
        ids.push(id);
        pos += 2 + len;
    }
    (pos == p.len()).then_some(ids)
}

/// Class of a phone -> watch item that is a request (not a response).
pub fn classify_phone_request(kind: &Kind) -> Result<Class, String> {
    Ok(match kind {
        Kind::CloseAll => Class::BringUp,
        Kind::Register { service, .. } => match *service {
            4 | 1 | 22 => Class::BringUp,
            2 => Class::NotSent(Allowed::NfcAndRealtimeRegistrationsNotSent),
            s if REALTIME_SERVICES.contains(&s) => {
                Class::NotSent(Allowed::NfcAndRealtimeRegistrationsNotSent)
            }
            s => return Err(format!("register of service {s}")),
        },
        Kind::CloseHandle { service, .. } => {
            Class::NotSent(Allowed::AppCloseOfNotSentHandle(*service))
        }
        Kind::Info(sel) => match sel.first() {
            Some(0..=4) => Class::BringUp,
            Some(5) => Class::NotSent(Allowed::InfoPage5NotSent),
            _ => return Err(format!("info selector {sel:02x?}")),
        },
        Kind::Service { service: 22, .. } => Class::BringUp,
        Kind::Service { service: 2, .. } => {
            Class::NotSent(Allowed::NfcAndRealtimeRegistrationsNotSent)
        }
        Kind::Service { service, .. } => return Err(format!("data on service {service}")),
        Kind::Gfdi(msg) => match msg.id {
            5050 | 5031 => Class::Handshake,
            5030 => match msg.payload.first() {
                Some(8) => Class::Handshake,
                Some(0) => Class::Sync,
                Some(6) => Class::NotSent(Allowed::HostForegroundNotSent),
                other => return Err(format!("system event {other:?}")),
            },
            5007 | 5002 => Class::Sync,
            5008 => Class::Sync,
            5026 => {
                // Only the three settings 6, 7, 8 (`03 | 06 01 01 | 07 01 01 | 08 01 00`).
                let ids = tlv_ids(&msg.payload).ok_or("malformed 5026")?;
                if ids != [6, 7, 8] {
                    return Err(format!("5026 with setting ids {ids:?}"));
                }
                Class::NotSent(Allowed::SettingsFive026NotSent)
            }
            5033 | 5035 | 5049 => Class::NotSent(Allowed::Notifications5033_5035_5049NotSent),
            5044 => Class::NotSent(Allowed::ProtobufAnswers5044NotSent),
            5003..=5005 => Class::NotSent(Allowed::UploadNotSent),
            5011 | 5012 => Class::NotSent(Allowed::WeatherNotSent),
            5043 => {
                let (_, body) = m::parse_protobuf(&msg.payload).ok_or("short 5043")?;
                match smart_fields(body) {
                    Some((13, 8)) => Class::NotSent(Allowed::FeatureCapabilitiesNotSent),
                    Some((13, 1)) if body == m::SYNC_NOW_PROTOBUF => Class::ManualSync,
                    Some((f @ (21 | 42), _)) => Class::NotSent(Allowed::AppProtobufField(f)),
                    other => {
                        return Err(format!("app-initiated protobuf request {other:?}"));
                    }
                }
            }
            id => return Err(format!("phone request {id}")),
        },
        Kind::Other(s) => return Err(s.clone()),
        Kind::CloseAllResp { .. }
        | Kind::RegisterResp { .. }
        | Kind::CloseHandleResp { .. }
        | Kind::InfoReply(_) => return Err("watch item on the phone side".into()),
    })
}

/// Key used to match a client request to an app request: protobuf request
/// ids and txns are normalised away.
pub fn request_key(msg: &Message) -> (u16, Vec<u8>) {
    let mut p = msg.payload.clone();
    if msg.id == 5043 && p.len() >= 2 {
        p[0] = 0;
        p[1] = 0;
    }
    (msg.id, p)
}

// ---- the replay driver ------------------------------------------------------------------------

#[derive(Debug)]
pub struct ReplayReport {
    pub name: String,
    pub used: BTreeMap<Allowed, usize>,
    pub failures: Vec<String>,
    pub unclassified: Vec<Unclassified>,
    pub files_replayed: usize,
    pub cycles: usize,
    pub client_requests: usize,
    pub closed: Option<String>,
    /// Anomaly events the client raised, by kind.
    pub anomalies: BTreeMap<String, usize>,
}

impl ReplayReport {
    pub fn ok(&self) -> bool {
        self.failures.is_empty()
            && self.unclassified.is_empty()
            && self.closed.is_none()
            && self.anomalies.is_empty()
    }
}

/// One item the client wrote, as the replay peer saw it.
#[derive(Debug, Clone)]
enum ClientItem {
    Raw(Vec<u8>),
    Gfdi(Message),
}

#[derive(Debug, Clone)]
struct Logged {
    t_ms: u64,
    item: ClientItem,
}

enum Scheduled {
    /// Deliver transcript item `i` (a watch item) to the client.
    Deliver(usize, Option<Rewrite>),
    StartSync,
}

#[derive(Default)]
struct Sched {
    items: Vec<(u64, u64, Scheduled)>,
    seq: u64,
}

impl Sched {
    fn push(&mut self, at: u64, what: Scheduled) {
        self.seq += 1;
        self.items.push((at, self.seq, what));
    }
}

#[derive(Clone)]
struct Rewrite {
    txn: Option<u8>,
    protobuf_id: Option<u16>,
}

/// The watch side of the replay: one reliable endpoint plus handle tables.
struct Peer {
    mtu: u16,
    endpoint: Option<MlrEndpoint>,
    cobs: FrameDecoder,
    reg_handle: Option<u8>,
    ka_handle: Option<u8>,
}

pub struct WallClock {
    pub base_unix_s: i64,
    pub utc_offset_s: i32,
    pub dst: Option<nimrag_core::client::DstPair>,
}

impl WallClock {
    pub fn now(&self, mono_ms: u64) -> Now {
        Now {
            mono_ms,
            unix_s: self.base_unix_s + (mono_ms / 1000) as i64,
            utc_offset_s: self.utc_offset_s,
            dst: self.dst,
        }
    }
}

fn host_config(s: &SessionTranscript) -> Result<ClientConfig, String> {
    let mut host = None;
    let mut caps = None;
    for it in &s.items {
        if let (true, Kind::Gfdi(msg)) = (it.from_phone, &it.kind) {
            if let Some(r) = msg.as_response() {
                if r.request_id == 5024 && host.is_none() {
                    host = m::parse_host_info(r.body);
                }
            } else if msg.id == 5050 && caps.is_none() {
                caps = m::parse_configuration(&msg.payload);
            }
        }
    }
    let host = host.ok_or("no ACK(5024) from the app in this session")?;
    let caps = caps.ok_or("no 5050 from the app in this session")?;
    Ok(ClientConfig {
        client_id: s.client_id,
        connection_epoch: 1,
        host_name: host.name,
        host_manufacturer: host.manufacturer,
        host_model: host.model,
        app_version: host.app_version,
        capabilities: caps,
        first_connect: false,
        archive: true,
        skip: vec![],
    })
}

fn wall_clock(s: &SessionTranscript) -> WallClock {
    let mut clock = WallClock {
        base_unix_s: (s.t0_us - capture::BTSNOOP_UNIX_OFFSET_US) / 1_000_000,
        utc_offset_s: 0,
        dst: None,
    };
    for it in &s.items {
        if let (true, Kind::Gfdi(msg)) = (it.from_phone, &it.kind) {
            if let Some(r) = msg.as_response() {
                if r.request_id == 5052 {
                    if let Some(t) = m::parse_current_time_reply(r.body) {
                        clock.utc_offset_s = t.utc_offset_s;
                        let epoch = nimrag_core::client::GARMIN_EPOCH_UNIX_S;
                        clock.dst = (t.dst_start != 0 || t.dst_end != 0).then(|| {
                            nimrag_core::client::DstPair {
                                start_unix_s: t.dst_start as i64 + epoch,
                                end_unix_s: t.dst_end as i64 + epoch,
                            }
                        });
                        break;
                    }
                }
            }
        }
    }
    clock
}

/// Binding of watch items to the phone items they answer.
struct Binding {
    /// For each item: the phone item it replies to (watch items only).
    reply_to: Vec<Option<usize>>,
    /// For each phone item: its watch replies, in transcript order.
    replies: Vec<Vec<usize>>,
    /// For each phone response item: the watch request it answers.
    answers: Vec<Option<usize>>,
    /// For each watch request item: the app's response item.
    answered_by: Vec<Option<usize>>,
    /// Watch items that are never delivered (services the client never opens).
    dropped: Vec<bool>,
    unclassified: Vec<Unclassified>,
}

fn bind(s: &SessionTranscript) -> Binding {
    let n = s.items.len();
    let mut b = Binding {
        reply_to: vec![None; n],
        replies: vec![Vec::new(); n],
        answers: vec![None; n],
        answered_by: vec![None; n],
        dropped: vec![false; n],
        unclassified: Vec::new(),
    };
    // Open phone requests, most recent last.
    let mut open_close: Vec<usize> = Vec::new();
    let mut open_reg: Vec<(u16, usize)> = Vec::new();
    let mut open_info: Vec<usize> = Vec::new();
    let mut open_hello: Vec<usize> = Vec::new();
    let mut open_req: Vec<(u16, Option<u8>, usize)> = Vec::new();
    let mut last_download: Option<usize> = None;
    let mut protobuf: Vec<(u16, usize)> = Vec::new();
    // Watch requests not yet answered by the app.
    let mut watch_open: Vec<(u16, Option<u8>, usize)> = Vec::new();
    let watch_rid = |i: usize| -> Option<u16> {
        match &s.items[i].kind {
            Kind::Gfdi(msg) if matches!(msg.id, 5043 | 5044) => {
                m::parse_protobuf(&msg.payload).map(|(h, _)| h.request_id)
            }
            _ => None,
        }
    };

    let note = |b: &mut Binding, i: usize, what: String| {
        b.unclassified.push(Unclassified {
            t_ms: s.items[i].t_ms,
            what,
        });
    };

    for (i, it) in s.items.iter().enumerate() {
        match (&it.kind, it.from_phone) {
            (Kind::CloseAll, true) => open_close.push(i),
            (Kind::Register { service, .. }, true) => open_reg.push((*service, i)),
            (Kind::Info(_), true) => open_info.push(i),
            (Kind::Service { service: 22, data }, true) if data.starts_with(&[1, 0x19]) => {
                open_hello.push(i)
            }
            (Kind::Service { .. }, true) => {}
            (Kind::Gfdi(msg), true) if msg.id == 5000 => {
                if let Some(r) = msg.as_response() {
                    let body_rid =
                        (r.body.len() >= 2).then(|| u16::from_le_bytes([r.body[0], r.body[1]]));
                    let pos = watch_open.iter().position(|(id, txn, w)| {
                        *id == r.request_id
                            && *txn == msg.txn
                            && (!matches!(*id, 5043 | 5044) || watch_rid(*w) == body_rid)
                    });
                    match pos {
                        Some(p) => {
                            let (_, _, w) = watch_open.remove(p);
                            b.answers[i] = Some(w);
                            b.answered_by[w] = Some(i);
                        }
                        None => note(
                            &mut b,
                            i,
                            format!("phone response to {} without a watch request", r.request_id),
                        ),
                    }
                } else {
                    note(&mut b, i, "short phone 5000".into());
                }
            }
            (Kind::Gfdi(msg), true) => {
                open_req.push((msg.id, msg.txn, i));
                if msg.id == 5002 {
                    last_download = Some(i);
                }
                if msg.id == 5043 {
                    if let Some((h, _)) = m::parse_protobuf(&msg.payload) {
                        protobuf.push((h.request_id, i));
                    }
                }
            }
            (Kind::CloseAllResp { .. }, false) => match open_close.pop() {
                Some(p) => b.reply_to[i] = Some(p),
                None => note(&mut b, i, "close-all response without request".into()),
            },
            (Kind::RegisterResp { service, .. }, false) => {
                match open_reg.iter().rposition(|(s2, _)| s2 == service) {
                    Some(pos) => b.reply_to[i] = Some(open_reg.remove(pos).1),
                    None => note(
                        &mut b,
                        i,
                        format!("register response for service {service} without request"),
                    ),
                }
            }
            (Kind::InfoReply(_), false) => match open_info.pop() {
                Some(p) => b.reply_to[i] = Some(p),
                None => note(&mut b, i, "info reply without request".into()),
            },
            (Kind::Service { service: 22, data }, false) => {
                if data == &[2, 0] {
                    match open_hello.pop() {
                        Some(p) => b.reply_to[i] = Some(p),
                        None => note(&mut b, i, "keep-alive 02 00 without hello".into()),
                    }
                }
            }
            (Kind::Service { .. }, false) => b.dropped[i] = true,
            (Kind::CloseHandle { .. }, true) => {}
            (Kind::CloseHandleResp { .. }, false) => b.dropped[i] = true,
            (Kind::Gfdi(msg), false) => match msg.id {
                5000 => {
                    let Some(r) = msg.as_response() else {
                        note(&mut b, i, "short watch 5000".into());
                        continue;
                    };
                    let pos = open_req.iter().rposition(|(id, txn, _)| {
                        *id == r.request_id
                            && (txn.is_none() || msg.txn.is_none() || *txn == msg.txn)
                    });
                    match pos {
                        Some(p) => {
                            let (_, _, pi) = open_req.remove(p);
                            b.reply_to[i] = Some(pi);
                        }
                        None => note(
                            &mut b,
                            i,
                            format!("watch response to {} without phone request", r.request_id),
                        ),
                    }
                }
                5004 => {
                    match last_download {
                        Some(p) => b.reply_to[i] = Some(p),
                        None => note(&mut b, i, "5004 without a 5002".into()),
                    }
                    watch_open.push((5004, msg.txn, i));
                }
                5044 => {
                    let rid = m::parse_protobuf(&msg.payload).map(|(h, _)| h.request_id);
                    let found = rid.and_then(|rid| protobuf.iter().rev().find(|(r, _)| *r == rid));
                    match found {
                        Some((_, p)) => b.reply_to[i] = Some(*p),
                        None => note(&mut b, i, "5044 answering no phone 5043".into()),
                    }
                    watch_open.push((5044, msg.txn, i));
                }
                id => watch_open.push((id, msg.txn, i)),
            },
            (Kind::Other(what), _) => note(&mut b, i, what.clone()),
            (k, p) => note(&mut b, i, format!("unplaceable item {k:?} phone={p}")),
        }
    }
    for i in 0..n {
        if let Some(p) = b.reply_to[i] {
            b.replies[p].push(i);
        }
    }
    b
}

/// Everything the client wrote, split by lane, plus the replay outcome.
pub fn replay(s: &SessionTranscript) -> ReplayReport {
    let mut report = ReplayReport {
        name: s.name.clone(),
        used: BTreeMap::new(),
        failures: Vec::new(),
        unclassified: Vec::new(),
        files_replayed: 0,
        cycles: 0,
        client_requests: 0,
        closed: None,
        anomalies: BTreeMap::new(),
    };
    let config = match host_config(s) {
        Ok(c) => c,
        Err(e) => {
            report.failures.push(format!("not replayable: {e}"));
            return report;
        }
    };
    let binding = bind(s);
    report
        .unclassified
        .extend(binding.unclassified.iter().cloned());

    // Stop rules 8.2 and 8.3: things the contract does not name.
    let mut registered: std::collections::HashMap<u8, u16> = std::collections::HashMap::new();
    for (i, it) in s.items.iter().enumerate() {
        if let Kind::RegisterResp {
            service,
            status: 0,
            handle,
            ..
        } = &it.kind
        {
            registered.insert(*handle, *service);
        }
        match &it.kind {
            Kind::RegisterResp {
                service,
                status,
                flags,
                ..
            } if matches!(*service, 4 | 1 | 22) => {
                let reliable_expected = *service == 1;
                if *status != 0
                    || (reliable_expected && flags & 1 == 0)
                    || (!reliable_expected && flags & 1 != 0)
                {
                    report.unclassified.push(Unclassified {
                        t_ms: it.t_ms,
                        what: format!("stop 8.2: register response service {service} status {status} flags {flags:#x}"),
                    });
                }
            }
            Kind::CloseHandle { service, handle } | Kind::CloseHandleResp { service, handle } => {
                let listed = *service == 2 || REALTIME_SERVICES.contains(service);
                if !(listed && registered.get(handle) == Some(service)) {
                    report.unclassified.push(Unclassified {
                        t_ms: it.t_ms,
                        what: format!(
                            "stop 8.2: close handle {handle} (service {service}) was not registered by the app for a not-sent service"
                        ),
                    });
                }
            }
            Kind::Gfdi(msg) if msg.id == 5054 => report.unclassified.push(Unclassified {
                t_ms: it.t_ms,
                what: "stop 8.3: 5054 compressed data".into(),
            }),
            Kind::Gfdi(msg) if msg.id == 5000 => {
                if let Some(r) = msg.as_response() {
                    if !it.from_phone && r.status != nimrag_core::gfdi::Status::Ack {
                        report.unclassified.push(Unclassified {
                            t_ms: it.t_ms,
                            what: format!(
                                "stop 8.3: watch status {:?} to request {}",
                                r.status, r.request_id
                            ),
                        });
                    }
                    if !it.from_phone && r.request_id == 5008 {
                        let asked = binding.reply_to[i].and_then(|p| match &s.items[p].kind {
                            Kind::Gfdi(req) => m::parse_set_file_flags(&req.payload),
                            _ => None,
                        });
                        let want = asked.map(|(idx, f)| {
                            let mut v = vec![0];
                            v.extend_from_slice(&idx.wrapping_sub(1).to_le_bytes());
                            v.push(f);
                            v
                        });
                        if want.as_deref() != Some(r.body) {
                            report.unclassified.push(Unclassified {
                                t_ms: it.t_ms,
                                what: "stop 8.3: 5008 reply body breaks the index - 1 pattern"
                                    .into(),
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Classify every phone request.
    let mut class: Vec<Option<Class>> = vec![None; s.items.len()];
    for (i, it) in s.items.iter().enumerate() {
        let is_request = it.from_phone && !matches!(&it.kind, Kind::Gfdi(msg) if msg.id == 5000);
        if is_request {
            match classify_phone_request(&it.kind) {
                Ok(c) => class[i] = Some(c),
                Err(what) => {
                    report.unclassified.push(Unclassified {
                        t_ms: it.t_ms,
                        what,
                    });
                }
            }
        }
    }

    if !report.unclassified.is_empty() {
        return report;
    }

    let wall = wall_clock(s);
    let mut client = Client::new(config);
    let mut peer = Peer {
        mtu: s.mtu,
        endpoint: None,
        cobs: FrameDecoder::new(),
        reg_handle: None,
        ka_handle: None,
    };
    let mut queue = Sched::default();

    // Unsolicited watch items at their timestamps; manual sync as StartSync.
    for (i, it) in s.items.iter().enumerate() {
        if binding.dropped[i] {
            continue;
        }
        if !it.from_phone && binding.reply_to[i].is_none() {
            let skip = matches!(&it.kind, Kind::Gfdi(msg) if msg.id == 5000);
            if !skip {
                queue.push(it.t_ms, Scheduled::Deliver(i, None));
            }
        }
        if class[i] == Some(Class::ManualSync) {
            queue.push(it.t_ms, Scheduled::StartSync);
        }
    }

    let end_ms = s.items.last().map_or(0, |i| i.t_ms) + 60_000;
    let mut t = 0u64;
    let mut consumed = vec![false; s.items.len()];
    let mut log: Vec<Logged> = Vec::new();
    let mut events: Vec<Event> = Vec::new();
    let mut downloaded: Vec<(u16, Vec<u8>)> = Vec::new();
    let mut inbox: VecDeque<Vec<u8>> = VecDeque::new();
    // Watch requests as delivered to the client, with the item index.
    let mut delivered_requests: Vec<(usize, u16, Option<u8>, Option<u16>)> = Vec::new();
    // Client responses, with the watch request index they answer.
    let mut client_answers: Vec<(usize, Message)> = Vec::new();
    let mut client_requests: Vec<(usize, Message)> = Vec::new();

    // Start the connection.
    let mut wake: Option<u64>;
    let mut pending_writes: Vec<Vec<u8>> = Vec::new();
    {
        let step = client.handle(wall.now(0), Input::Connected { att_mtu: s.mtu });
        events.extend(step.events);
        pending_writes.extend(step.writes);
        wake = step.wake_at_mono_ms;
    }

    let mut guard = 0u64;
    loop {
        guard += 1;
        if guard > 5_000_000 {
            report.failures.push("replay loop did not terminate".into());
            break;
        }
        // 1. Everything the client wrote reaches the peer.
        while !pending_writes.is_empty() || !inbox.is_empty() {
            for w in std::mem::take(&mut pending_writes) {
                handle_client_write(
                    s,
                    &binding,
                    &class,
                    &mut peer,
                    &mut consumed,
                    &mut queue,
                    &mut log,
                    &mut client_requests,
                    &mut client_answers,
                    &delivered_requests,
                    &mut report,
                    &mut inbox,
                    t,
                    w,
                );
            }
            while let Some(v) = inbox.pop_front() {
                let step = client.handle(wall.now(t), Input::Notification { value: v });
                absorb(&mut events, &mut pending_writes, &mut wake, step);
            }
            // FileDownloaded is answered at once with FileStored.
            let new: Vec<(u64, u16, Vec<u8>)> = events
                .iter()
                .filter_map(|e| match e {
                    Event::FileDownloaded {
                        transfer_id,
                        entry,
                        data,
                    } => Some((*transfer_id, entry.index, data.clone())),
                    _ => None,
                })
                .collect();
            for (id, index, data) in new.into_iter().skip(downloaded.len()) {
                downloaded.push((index, data));
                let step = client.handle(wall.now(t), Input::FileStored { transfer_id: id });
                absorb(&mut events, &mut pending_writes, &mut wake, step);
            }
        }
        if let Some(Event::Closed { reason }) =
            events.iter().find(|e| matches!(e, Event::Closed { .. }))
        {
            report.closed = Some(format!("{reason:?} at {t} ms"));
            break;
        }
        // 2. Next thing that happens.
        queue.items.sort_by_key(|(at, seq, _)| (*at, *seq));
        let next_sched = queue.items.first().map(|(at, _, _)| *at);
        let next_peer = peer.endpoint.as_ref().and_then(|e| e.next_deadline());
        let next = [next_sched, wake, next_peer].into_iter().flatten().min();
        let Some(next) = next else { break };
        if next > end_ms {
            break;
        }
        t = t.max(next);
        // Timers.
        if let Some(ep) = peer.endpoint.as_mut() {
            for p in ep.poll(t) {
                inbox.push_back(p);
            }
        }
        if wake.is_some_and(|w| w <= t) {
            let step = client.handle(wall.now(t), Input::Tick);
            absorb(&mut events, &mut pending_writes, &mut wake, step);
        }
        // Scheduled deliveries due now.
        while queue.items.first().is_some_and(|(at, _, _)| *at <= t) {
            let (at, _, what) = queue.items.remove(0);
            match what {
                Scheduled::StartSync => {
                    let step = client.handle(wall.now(t), Input::StartSync);
                    absorb(&mut events, &mut pending_writes, &mut wake, step);
                }
                Scheduled::Deliver(i, rewrite) => {
                    let it = &s.items[i];
                    match deliver_item(s, i, rewrite.as_ref(), &mut peer, t, &mut inbox) {
                        Delivery::Done => {
                            if let Kind::Gfdi(msg) = &it.kind {
                                if msg.id != 5000 {
                                    let rid = if msg.id == 5044 {
                                        rewrite.as_ref().and_then(|r| r.protobuf_id)
                                    } else {
                                        None
                                    }
                                    .or_else(|| {
                                        matches!(msg.id, 5043 | 5044)
                                            .then(|| m::parse_protobuf(&msg.payload))
                                            .flatten()
                                            .map(|(h, _)| h.request_id)
                                    });
                                    delivered_requests.push((i, msg.id, msg.txn, rid));
                                }
                            }
                        }
                        Delivery::Later => {
                            queue.push(at.max(t) + 5, Scheduled::Deliver(i, rewrite));
                        }
                    }
                }
            }
        }
    }
    report.client_requests = client_requests.len();
    for e in &events {
        if let Event::Anomaly { kind, message_id } = e {
            *report
                .anomalies
                .entry(format!("{kind:?}({message_id})"))
                .or_default() += 1;
        }
    }

    if report.closed.is_none() {
        compare(
            s,
            &binding,
            &class,
            &consumed,
            &log,
            &client_requests,
            &client_answers,
            &downloaded,
            &mut report,
        );
    }
    report
}

fn absorb(
    events: &mut Vec<Event>,
    writes: &mut Vec<Vec<u8>>,
    wake: &mut Option<u64>,
    step: nimrag_core::client::Step,
) {
    events.extend(step.events);
    writes.extend(step.writes);
    *wake = step.wake_at_mono_ms;
}

enum Delivery {
    Done,
    /// The client cannot receive this yet (no reliable stream); try again.
    Later,
}

/// Put one watch item on the wire towards the client.
fn deliver_item(
    s: &SessionTranscript,
    i: usize,
    rewrite: Option<&Rewrite>,
    peer: &mut Peer,
    t: u64,
    inbox: &mut VecDeque<Vec<u8>>,
) -> Delivery {
    let it = &s.items[i];
    match &it.kind {
        Kind::CloseAllResp { status } => {
            inbox.push_back(close_all_response(s.client_id, *status));
        }
        Kind::RegisterResp {
            service,
            status,
            handle,
            flags,
            extra,
        } => {
            match (*service, *status) {
                (4, 0) => peer.reg_handle = Some(*handle),
                (22, 0) => peer.ka_handle = Some(*handle),
                (1, 0) if flags & 1 != 0 => {
                    peer.endpoint = Some(MlrEndpoint::new(handle & 7, peer.mtu));
                }
                _ => {}
            }
            inbox.push_back(register_response(
                s.client_id,
                *service,
                *status,
                *handle,
                *flags,
                *extra,
            ));
        }
        Kind::InfoReply(bytes) => {
            let Some(h) = peer.reg_handle else {
                return Delivery::Later;
            };
            let mut v = vec![h];
            v.extend_from_slice(bytes);
            inbox.push_back(v);
        }
        Kind::Service { service: 22, data } => {
            let Some(h) = peer.ka_handle else {
                return Delivery::Later;
            };
            let mut v = vec![h];
            v.extend_from_slice(data);
            inbox.push_back(v);
        }
        Kind::Gfdi(msg) => {
            let Some(ep) = peer.endpoint.as_mut() else {
                return Delivery::Later;
            };
            let mut msg = msg.clone();
            if let Some(rw) = rewrite {
                if rw.txn.is_some() && msg.id == 5000 {
                    msg.txn = rw.txn;
                }
                if let Some(rid) = rw.protobuf_id {
                    if msg.id == 5044 && msg.payload.len() >= 2 {
                        msg.payload[0..2].copy_from_slice(&rid.to_le_bytes());
                    }
                    if msg.id == 5000
                        && msg.payload.len() >= 5
                        && msg.payload[0..2] == 5043u16.to_le_bytes()
                    {
                        msg.payload[3..5].copy_from_slice(&rid.to_le_bytes());
                    }
                }
            }
            let framed = cobs::encode_frame(&msg.encode());
            for p in ep.send(t, &framed) {
                inbox.push_back(p);
            }
        }
        other => panic!("cannot deliver {other:?}"),
    }
    Delivery::Done
}

#[allow(clippy::too_many_arguments)]
fn handle_client_write(
    s: &SessionTranscript,
    binding: &Binding,
    class: &[Option<Class>],
    peer: &mut Peer,
    consumed: &mut [bool],
    queue: &mut Sched,
    log: &mut Vec<Logged>,
    client_requests: &mut Vec<(usize, Message)>,
    client_answers: &mut Vec<(usize, Message)>,
    delivered_requests: &[(usize, u16, Option<u8>, Option<u16>)],
    report: &mut ReplayReport,
    inbox: &mut VecDeque<Vec<u8>>,
    t: u64,
    value: Vec<u8>,
) {
    if value.is_empty() {
        return;
    }
    // Raw (non-GFDI) writes: control, info, keep-alive.
    let mut raw_item = |value: &[u8], want: &dyn Fn(&Kind) -> Option<Vec<u8>>| {
        log.push(Logged {
            t_ms: t,
            item: ClientItem::Raw(value.to_vec()),
        });
        let found = s.items.iter().enumerate().position(|(i, it)| {
            it.from_phone && !consumed[i] && want(&it.kind).is_some_and(|b| b == value)
        });
        found
    };
    let phone_raw = |k: &Kind, reg: Option<u8>, ka: Option<u8>| -> Option<Vec<u8>> {
        match k {
            Kind::CloseAll => Some(multilink::close_all_request(s.client_id)),
            Kind::Register { service, mode } => {
                Some(multilink::register_request(s.client_id, *service, *mode))
            }
            Kind::Info(sel) => reg.map(|h| [&[h][..], sel].concat()),
            Kind::Service { service: 22, data } => ka.map(|h| [&[h][..], data].concat()),
            _ => None,
        }
    };
    if value[0] == 0 || Some(value[0]) == peer.reg_handle || Some(value[0]) == peer.ka_handle {
        let (reg, ka) = (peer.reg_handle, peer.ka_handle);
        let found = raw_item(&value, &|k| phone_raw(k, reg, ka));
        match found {
            Some(p) => {
                consumed[p] = true;
                schedule_replies(s, binding, p, None, t, queue);
            }
            None => report.failures.push(format!(
                "client wrote {value:02x?} at {t} ms; the app did not"
            )),
        }
        return;
    }
    if value[0] & 0x80 == 0 {
        report
            .failures
            .push(format!("client wrote on unknown handle {:#x}", value[0]));
        return;
    }
    let Some((header, data)) = mlr::parse(&value) else {
        return;
    };
    let Some(ep) = peer
        .endpoint
        .as_mut()
        .filter(|e| e.handle() == header.handle)
    else {
        report
            .failures
            .push("client wrote an MLR packet before GFDI was registered".into());
        return;
    };
    let received = ep.receive(t, header, data);
    for w in received.writes {
        inbox.push_back(w);
    }
    if received.data.is_empty() {
        return;
    }
    for frame in peer.cobs.push(&received.data) {
        let Ok(frame) = frame else {
            report.failures.push("client sent a bad COBS frame".into());
            continue;
        };
        let Ok(msg) = Message::parse(&frame) else {
            report
                .failures
                .push("client sent a bad GFDI message".into());
            continue;
        };
        log.push(Logged {
            t_ms: t,
            item: ClientItem::Gfdi(msg.clone()),
        });
        if msg.id == 5000 {
            // Pair with the delivered watch request it answers.
            let r = msg.as_response();
            let pos = r.and_then(|r| {
                let body_rid =
                    (r.body.len() >= 2).then(|| u16::from_le_bytes([r.body[0], r.body[1]]));
                delivered_requests
                    .iter()
                    .position(|(i, id, txn, rid)| {
                        *id == r.request_id
                            && *txn == msg.txn
                            && (!matches!(*id, 5043 | 5044) || *rid == body_rid)
                            && !client_answers.iter().any(|(w, _)| w == i)
                    })
                    .map(|p| delivered_requests[p].0)
            });
            match pos {
                Some(w) => client_answers.push((w, msg)),
                None => report.failures.push(format!(
                    "client response {:?} at {t} ms answers no delivered watch request",
                    msg.as_response().map(|r| r.request_id)
                )),
            }
        } else {
            let key = request_key(&msg);
            let found = s.items.iter().enumerate().position(|(i, it)| {
                it.from_phone
                    && !consumed[i]
                    && class[i].is_some()
                    && matches!(&it.kind, Kind::Gfdi(m2) if m2.id != 5000 && request_key(m2) == key)
            });
            match found {
                Some(p) => {
                    consumed[p] = true;
                    client_requests.push((p, msg.clone()));
                    schedule_replies(s, binding, p, Some(&msg), t, queue);
                }
                None => report.failures.push(format!(
                    "client request {} at {t} ms matches no unconsumed app request (payload {} bytes)",
                    msg.id,
                    msg.payload.len()
                )),
            }
        }
    }
}

/// The client made the request app item `p` stands for: schedule the watch's
/// replies after the captured latency, rewritten to the client's ids.
fn schedule_replies(
    s: &SessionTranscript,
    binding: &Binding,
    p: usize,
    client_msg: Option<&Message>,
    now: u64,
    queue: &mut Sched,
) {
    let rewrite = client_msg.map(|c| Rewrite {
        txn: c.txn,
        protobuf_id: if c.id == 5043 {
            m::parse_protobuf(&c.payload).map(|(h, _)| h.request_id)
        } else {
            None
        },
    });
    let p_t = s.items[p].t_ms;
    for &w in &binding.replies[p] {
        let latency = s.items[w].t_ms.saturating_sub(p_t);
        queue.push(now + latency, Scheduled::Deliver(w, rewrite.clone()));
    }
}

// ---- lane comparison ---------------------------------------------------------------------------

type Sig = (u16, Vec<u8>);

fn sig(msg: &Message) -> Sig {
    request_key(msg)
}

fn response_sig(msg: &Message) -> Sig {
    // Protobuf ACK bodies carry the request id first; normalise that too.
    let mut p = msg.payload.clone();
    if p.len() >= 5 && p[0..2] == 5043u16.to_le_bytes()
        || p.len() >= 5 && p[0..2] == 5044u16.to_le_bytes()
    {
        p[3] = 0;
        p[4] = 0;
    }
    (msg.id, p)
}

#[allow(clippy::too_many_arguments)]
fn compare(
    s: &SessionTranscript,
    binding: &Binding,
    class: &[Option<Class>],
    consumed: &[bool],
    log: &[Logged],
    client_requests: &[(usize, Message)],
    client_answers: &[(usize, Message)],
    downloaded: &[(u16, Vec<u8>)],
    report: &mut ReplayReport,
) {
    let mut used = |a: Allowed| *report.used.entry(a).or_default() += 1;
    let mut fail: Vec<String> = Vec::new();

    // App phone requests that the client never made must be on the list.
    for (i, c) in class.iter().enumerate() {
        match c {
            Some(Class::NotSent(a)) if !consumed[i] => used(*a),
            Some(_) if !consumed[i] => fail.push(format!(
                "the app sent {:?} at {} ms and the client did not",
                match &s.items[i].kind {
                    Kind::Gfdi(m2) => format!("GFDI {}", m2.id),
                    k => format!("{k:?}"),
                },
                s.items[i].t_ms
            )),
            _ => {}
        }
    }

    // Bring-up lane: byte-exact, registration order free.
    let app_bring: Vec<Vec<u8>> = {
        let mut reg = None;
        let mut ka = None;
        for it in &s.items {
            if let Kind::RegisterResp {
                service,
                handle,
                status: 0,
                ..
            } = &it.kind
            {
                match service {
                    4 => reg = Some(*handle),
                    22 => ka = Some(*handle),
                    _ => {}
                }
            }
        }
        s.items
            .iter()
            .enumerate()
            .filter(|(i, it)| it.from_phone && class[*i] == Some(Class::BringUp))
            .filter_map(|(_, it)| match &it.kind {
                Kind::CloseAll => Some(multilink::close_all_request(s.client_id)),
                Kind::Register { service, mode } => {
                    Some(multilink::register_request(s.client_id, *service, *mode))
                }
                Kind::Info(sel) => reg.map(|h| [&[h][..], sel].concat()),
                Kind::Service { data, .. } => ka.map(|h| [&[h][..], data].concat()),
                _ => None,
            })
            .collect()
    };
    let client_bring: Vec<Vec<u8>> = log
        .iter()
        .filter_map(|l| match &l.item {
            ClientItem::Raw(v) => Some(v.clone()),
            _ => None,
        })
        .collect();
    let normalise = |mut v: Vec<Vec<u8>>| -> (Vec<Vec<u8>>, bool) {
        let gfdi = multilink::register_request(s.client_id, 1, 2);
        let ka = multilink::register_request(s.client_id, 22, 0);
        let mut swapped = false;
        for i in 0..v.len().saturating_sub(1) {
            if v[i] == ka && v[i + 1] == gfdi {
                v.swap(i, i + 1);
                swapped = true;
            }
        }
        (v, swapped)
    };
    let (a, a_swapped) = normalise(app_bring.clone());
    let (c, c_swapped) = normalise(client_bring.clone());
    if (a_swapped != c_swapped || app_bring != client_bring) && a == c {
        used(Allowed::RegistrationOrder);
    }
    if a != c {
        fail.push(format!(
            "bring-up lane differs: app {} writes, client {} writes (first difference at {:?})",
            a.len(),
            c.len(),
            a.iter().zip(&c).position(|(x, y)| x != y)
        ));
    }

    // Handshake lane: ACK(5024), ACK(5050), 5050, 5031, 5030 HANDSHAKE_COMPLETE.
    let handshake_ids = [5024u16, 5050];
    let app_handshake: Vec<Sig> = s
        .items
        .iter()
        .enumerate()
        .filter(|(i, it)| {
            it.from_phone
                && match &it.kind {
                    Kind::Gfdi(msg) if msg.id == 5000 => msg
                        .as_response()
                        .is_some_and(|r| handshake_ids.contains(&r.request_id)),
                    _ => class[*i] == Some(Class::Handshake),
                }
        })
        .filter_map(|(_, it)| match &it.kind {
            Kind::Gfdi(msg) => Some(if msg.id == 5000 {
                response_sig(msg)
            } else {
                sig(msg)
            }),
            _ => None,
        })
        .collect();
    let client_handshake: Vec<Sig> = log
        .iter()
        .filter_map(|l| match &l.item {
            ClientItem::Gfdi(msg) => {
                let is = if msg.id == 5000 {
                    msg.as_response()
                        .is_some_and(|r| handshake_ids.contains(&r.request_id))
                } else {
                    matches!(msg.id, 5050 | 5031)
                        || (msg.id == 5030 && msg.payload.first() == Some(&8))
                };
                is.then(|| {
                    if msg.id == 5000 {
                        response_sig(msg)
                    } else {
                        sig(msg)
                    }
                })
            }
            _ => None,
        })
        .collect();
    if app_handshake != client_handshake {
        fail.push(format!(
            "handshake lane differs: app {:?} vs client {:?}",
            app_handshake
                .iter()
                .map(|(i, p)| (*i, p.len()))
                .collect::<Vec<_>>(),
            client_handshake
                .iter()
                .map(|(i, p)| (*i, p.len()))
                .collect::<Vec<_>>()
        ));
    }

    // Time lane: one reply per 5052; fields equal, time within 2 s.
    let w_time: Vec<usize> = s
        .items
        .iter()
        .enumerate()
        .filter(|(_, it)| !it.from_phone && matches!(&it.kind, Kind::Gfdi(msg) if msg.id == 5052))
        .map(|(i, _)| i)
        .collect();
    for w in &w_time {
        let app = binding.answered_by[*w].and_then(|a| match &s.items[a].kind {
            Kind::Gfdi(msg) => msg
                .as_response()
                .and_then(|r| m::parse_current_time_reply(r.body)),
            _ => None,
        });
        let ours = client_answers
            .iter()
            .find(|(x, _)| x == w)
            .and_then(|(_, msg)| {
                msg.as_response()
                    .and_then(|r| m::parse_current_time_reply(r.body))
            });
        match (app, ours) {
            (Some(a), Some(o)) => {
                if (a.reference, a.utc_offset_s, a.dst_start, a.dst_end)
                    != (o.reference, o.utc_offset_s, o.dst_start, o.dst_end)
                {
                    fail.push(format!(
                        "time reply fields differ at {} ms",
                        s.items[*w].t_ms
                    ));
                }
                let diff = (a.garmin_time as i64 - o.garmin_time as i64).abs();
                if diff > 2 {
                    fail.push(format!(
                        "time differs by {diff} s at {} ms",
                        s.items[*w].t_ms
                    ));
                } else {
                    used(Allowed::TimeWithinTwoSeconds);
                }
            }
            (None, Some(_)) => fail.push(format!(
                "client answered a 5052 the app left unanswered at {} ms",
                s.items[*w].t_ms
            )),
            (Some(_), None) => fail.push(format!(
                "client left 5052 at {} ms unanswered",
                s.items[*w].t_ms
            )),
            (None, None) => {}
        }
    }

    // Sync lane.
    let sync_response_ids = [5037u16, 5004];
    let app_sync: Vec<Sig> = s
        .items
        .iter()
        .enumerate()
        .filter(|(i, it)| {
            it.from_phone
                && match &it.kind {
                    Kind::Gfdi(msg) if msg.id == 5000 => msg
                        .as_response()
                        .is_some_and(|r| sync_response_ids.contains(&r.request_id)),
                    _ => class[*i] == Some(Class::Sync),
                }
        })
        .filter_map(|(_, it)| match &it.kind {
            Kind::Gfdi(msg) => Some(if msg.id == 5000 {
                response_sig(msg)
            } else {
                sig(msg)
            }),
            _ => None,
        })
        .collect();
    let client_sync: Vec<Sig> = log
        .iter()
        .filter_map(|l| match &l.item {
            ClientItem::Gfdi(msg) => {
                let is = if msg.id == 5000 {
                    msg.as_response()
                        .is_some_and(|r| sync_response_ids.contains(&r.request_id))
                } else {
                    matches!(msg.id, 5007 | 5002 | 5008)
                        || (msg.id == 5030 && msg.payload.first() == Some(&0))
                };
                is.then(|| {
                    if msg.id == 5000 {
                        response_sig(msg)
                    } else {
                        sig(msg)
                    }
                })
            }
            _ => None,
        })
        .collect();
    if app_sync != client_sync {
        let first = app_sync.iter().zip(&client_sync).position(|(x, y)| x != y);
        fail.push(format!(
            "sync lane differs: app {} items, client {} items, first difference at {:?} (app {:?} client {:?})",
            app_sync.len(),
            client_sync.len(),
            first,
            first.map(|i| (app_sync[i].0, app_sync[i].1.len())),
            first.map(|i| (client_sync[i].0, client_sync[i].1.len())),
        ));
    }

    // Protobuf lane: the manual sync request and one ACK per 5043 / 5044.
    let app_proto_req: Vec<Sig> = s
        .items
        .iter()
        .enumerate()
        .filter(|(i, _)| class[*i] == Some(Class::ManualSync))
        .filter_map(|(_, it)| match &it.kind {
            Kind::Gfdi(msg) => Some(sig(msg)),
            _ => None,
        })
        .collect();
    let client_proto_req: Vec<Sig> = log
        .iter()
        .filter_map(|l| match &l.item {
            ClientItem::Gfdi(msg) if msg.id == 5043 => Some(sig(msg)),
            _ => None,
        })
        .collect();
    if app_proto_req != client_proto_req {
        fail.push("protobuf lane: manual-sync request differs".into());
    }
    for (i, it) in s.items.iter().enumerate() {
        let is_watch_proto = !it.from_phone
            && matches!(&it.kind, Kind::Gfdi(msg) if msg.id == 5043 || msg.id == 5044);
        if !is_watch_proto
            || binding.reply_to[i].is_some()
                && matches!(&it.kind, Kind::Gfdi(m2) if m2.id == 5044 && class[binding.reply_to[i].unwrap()] != Some(Class::ManualSync))
        {
            continue;
        }
        let Kind::Gfdi(wm) = &it.kind else { continue };
        let app_ack = binding.answered_by[i].and_then(|a| match &s.items[a].kind {
            Kind::Gfdi(msg) => msg.as_response().map(|r| r.body.to_vec()),
            _ => None,
        });
        let ours = client_answers
            .iter()
            .find(|(w, _)| *w == i)
            .and_then(|(_, msg)| msg.as_response().map(|r| r.body.to_vec()));
        match (app_ack, ours) {
            (Some(a), Some(o)) => {
                let (mut a2, mut o2) = (a.clone(), o.clone());
                if wm.id == 5044 {
                    // Our request id differs from the app's.
                    if a2.len() >= 2 {
                        a2[0] = 0;
                        a2[1] = 0;
                    }
                    if o2.len() >= 2 {
                        o2[0] = 0;
                        o2[1] = 0;
                    }
                }
                if a2 != o2 {
                    let tail_only =
                        a2.len() == 8 && o2.len() == 8 && a2[..6] == o2[..6] && o2[6..] == [0, 0];
                    if tail_only {
                        used(Allowed::ProtobufAckTail);
                    } else {
                        fail.push(format!(
                            "protobuf ACK differs at {} ms: app {a:02x?} client {o:02x?}",
                            it.t_ms
                        ));
                    }
                }
            }
            (None, Some(_)) | (Some(_), None) | (None, None) => fail.push(format!(
                "protobuf {} at {} ms: app answered {}, client answered {}",
                wm.id,
                it.t_ms,
                binding.answered_by[i].is_some(),
                client_answers.iter().any(|(w, _)| *w == i)
            )),
        }
    }

    // Every other watch request: the client says "unsupported" (status 2).
    for (i, it) in s.items.iter().enumerate() {
        let Kind::Gfdi(wm) = &it.kind else { continue };
        if it.from_phone
            || binding.reply_to[i].is_some()
            || matches!(wm.id, 5000 | 5004 | 5024 | 5050 | 5052 | 5037 | 5043 | 5044)
        {
            continue;
        }
        let Some((_, ours)) = client_answers.iter().find(|(w, _)| *w == i) else {
            // Not delivered (never reached the client) is a harness failure.
            fail.push(format!(
                "watch request {} at {} ms got no client response",
                wm.id, it.t_ms
            ));
            continue;
        };
        let r = ours.as_response();
        let client_ok = r.is_some_and(|r| {
            r.status == nimrag_core::gfdi::Status::UnknownOrNotSupported && r.body.is_empty()
        });
        if !client_ok {
            fail.push(format!(
                "client's answer to watch {} is not status 2 without a body",
                wm.id
            ));
        }
        let app = binding.answered_by[i].and_then(|a| match &s.items[a].kind {
            Kind::Gfdi(msg) => msg.as_response().map(|r| (r.status, r.body.to_vec())),
            _ => None,
        });
        match app {
            Some((nimrag_core::gfdi::Status::UnknownOrNotSupported, body)) if body.is_empty() => {}
            Some((nimrag_core::gfdi::Status::Ack, body))
                if !body.is_empty() && matches!(wm.id, 5036 | 5042 | 5014 | 5034) =>
            {
                used(Allowed::UnsupportedInsteadOfAck(wm.id));
            }
            other => fail.push(format!(
                "watch request {} at {} ms: app answered {other:?}, not an allowed difference",
                wm.id, it.t_ms
            )),
        }
    }

    // Order between lanes: informational.
    let lane_of_response = |id: u16| match id {
        5024 | 5050 => Some(b'H'),
        5052 => Some(b'T'),
        5037 | 5004 => Some(b'S'),
        5043 | 5044 => Some(b'P'),
        _ => None,
    };
    let lane_of = |msg: &Message, class: Option<Class>| -> Option<u8> {
        if msg.id == 5000 {
            msg.as_response()
                .and_then(|r| lane_of_response(r.request_id))
        } else {
            match class {
                Some(Class::Handshake) => Some(b'H'),
                Some(Class::Sync) => Some(b'S'),
                Some(Class::ManualSync) => Some(b'P'),
                _ => None,
            }
        }
    };
    let app_lanes: Vec<u8> = s
        .items
        .iter()
        .enumerate()
        .filter(|(_, it)| it.from_phone)
        .filter_map(|(i, it)| match &it.kind {
            Kind::Gfdi(msg) => lane_of(msg, class[i]),
            _ => None,
        })
        .collect();
    let client_lanes: Vec<u8> = log
        .iter()
        .filter_map(|l| match &l.item {
            ClientItem::Gfdi(msg) => {
                let c = match msg.id {
                    5050 | 5031 => Some(Class::Handshake),
                    5030 if msg.payload.first() == Some(&8) => Some(Class::Handshake),
                    5007 | 5002 | 5008 | 5030 => Some(Class::Sync),
                    5043 => Some(Class::ManualSync),
                    _ => None,
                };
                lane_of(msg, c)
            }
            _ => None,
        })
        .collect();
    if app_lanes != client_lanes {
        used(Allowed::LaneOrderDiffers);
    }
    // Watch 5037s that followed the app's foreground event.
    let foreground = s.items.iter().position(|it| {
        it.from_phone && matches!(&it.kind, Kind::Gfdi(msg) if msg.id == 5030 && msg.payload.first() == Some(&6))
    });
    if let Some(f) = foreground {
        let after = s.items[f..]
            .iter()
            .filter(|it| !it.from_phone && matches!(&it.kind, Kind::Gfdi(msg) if msg.id == 5037))
            .count();
        for _ in 0..after.min(1) {
            used(Allowed::WatchSyncAfterForegroundStillDelivered);
        }
    }

    // The files.
    let app_files: Vec<&(u16, Vec<u8>, bool)> = s.files.iter().collect();
    if app_files.iter().any(|f| !f.2) {
        fail.push("the app's own download failed its CRC check".into());
    }
    if app_files.len() != downloaded.len()
        || app_files
            .iter()
            .zip(downloaded)
            .any(|(a, c)| a.0 != c.0 || a.1 != c.1)
    {
        fail.push(format!(
            "downloaded files differ: app {} files, client {} files",
            app_files.len(),
            downloaded.len()
        ));
    }
    report.files_replayed = downloaded.len();
    report.cycles = client_requests
        .iter()
        .filter(|(_, m2)| m2.id == 5007)
        .count();
    let _ = Phase::Closed;
    report.failures.extend(fail);
}
