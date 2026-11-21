//! GPS assist inside the session: the watch's HTTP-over-protobuf requests, the
//! answer from the host, and the data transfer that carries the file.
//!
//! The duty is a setting of the host (`Duties::gps_assist`). With it off the
//! client behaves exactly as before: a protobuf request from the watch is
//! acknowledged and otherwise ignored. With it on:
//!
//! - **A request to `api.gcs.garmin.com` / `.cn` under `/ephemeris/`** (a
//!   `GET`, a known format, a clean address) becomes `Event::GpsAssistRequest`.
//!   The host answers with `Input::GpsAssistData { id, body, etag }` or
//!   `Input::GpsAssistUnavailable { id }`. The data are checked
//!   (`duties::agps`), answered 200 or 304 and sent as protobuf 5044 messages;
//!   when the watch asked for `use_data_xfer` the body is registered and the
//!   watch pulls it in pieces (`data_transfer` `DataDownloadRequest`). The end
//!   of it all is one `Event::GpsAssistDone`.
//! - **Every other HTTP request** is refused, as Gadgetbridge's firewall does
//!   for Garmin hosts: nothing is forwarded anywhere, the watch gets the
//!   "network request timed out" status (no data), and
//!   `Event::HttpRequestRefused` carries the host and path (cleaned, never the
//!   query) so a live test shows what the watch asks for. A request we cannot
//!   read (the encrypted form of our captures, Q-COMP-2) and one with forms,
//!   an "open this page" request and an image or OAuth request get no answer
//!   at all, as in Gadgetbridge.
//! - **Large protobuf messages** from the watch (`offset` / `total` chunks) are
//!   reassembled, and ours are sent in chunks of `max_packet - 20` bytes, each
//!   waiting for the watch's OK (the 5000 answer carrying `request_id`,
//!   `offset`, chunk status, status code).
//!
//! Ported from Gadgetbridge commit 7257742b (AGPL-3.0-or-later),
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/`:
//! `ProtocolBufferHandler.java` (`processIncoming`, `processChunkedMessage`,
//! `prepareProtobufMessage`, `ProtobufFragment`, `setMaxPacketSize`; no
//! copyright line in the file), `messages/ProtobufMessage.java` and
//! `messages/status/ProtobufStatusMessage.java` (the chunk header, the status
//! body and its codes), `http/HttpHandler.java`, `http/DataTransferHandler.java`
//! and `http/interceptors/{AgpsInterceptor,FirewallInterceptor}.java`
//! (Copyright (C) 2024-2026 Daniele Gobbetti, José Rebelo, kuhy, Thomas Kuehne
//! and the other Gadgetbridge contributors). *inferred (Gadgetbridge)* until a
//! watch confirms it. What we do differently is marked [ours]: the host, not
//! this crate, fetches the file; at most four requests wait for the host; the
//! watch's request for a push by file access (a multi-link file transfer we have
//! not built) is answered with "unknown" and reported; an answer from the watch
//! that never comes drops that message after the usual 30 s and the session goes
//! on.

use std::collections::VecDeque;

use super::session::{response, Kind, Out, Requests};
use super::{AnomalyKind, Event, Now};
use crate::duties::agps::{self, Format, Invalid};
use crate::duties::http::{self, Body, Incoming, RawRequest};
use crate::duties::xfer::{self, Download, Transfers};
use crate::gfdi::Message;
use crate::messages::{self as m, ProtobufHeader};

/// Requests waiting for the host's answer. [ours]
const MAX_PENDING: usize = 4;
/// Partly received protobuf messages kept. [ours]
const MAX_FRAGMENTS: usize = 2;
/// The largest protobuf message from the watch we reassemble. [ours]
const MAX_INCOMING: usize = 64 * 1024;
/// Chunk size before the watch told its maximum packet size
/// (`ProtocolBufferHandler.maxChunkSize`).
const DEFAULT_CHUNK: usize = 375;
/// 2 length + 2 id + 14 protobuf header + 2 CRC (`setMaxPacketSize`).
const CHUNK_OVERHEAD: usize = 20;

/// `ProtobufStatusMessage.ProtobufStatusCode`.
const CODE_UNKNOWN_REQUEST_ID: u8 = 100;
const CODE_DUPLICATE_PACKET: u8 = 101;
const CODE_MISSING_PACKET: u8 = 102;
const CODE_EXCEEDED_TOTAL: u8 = 103;

/// How a GPS-assist request ended (`Event::GpsAssistDone`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpsAssistOutcome {
    /// The file reached the watch (a 200 whose bytes it took).
    Sent,
    /// The watch's ETag matched: it was told 304 and keeps its file.
    NotModified,
    /// The host had no data (`Input::GpsAssistUnavailable`); the watch was told
    /// "no data".
    Unavailable,
    /// The data failed the check for the format (or were empty or over 1 MiB);
    /// the watch was told "no data".
    Refused,
    /// The reply did not get through (the watch refused a chunk, or the request
    /// was dropped to make room).
    Failed,
    /// The watch asked to be given the file by file access (a multi-link push),
    /// which is not built; see PROTOCOL.md.
    NeedsFileAccess,
}

/// The slice of the session the duty needs for one call.
pub struct Cx<'a> {
    pub now: &'a Now,
    pub req: &'a mut Requests,
    pub out: &'a mut Out,
    /// The watch's maximum packet size, 0 when not known yet.
    pub max_packet: u16,
}

/// Whether the watch's 5000 answer to one of our 5044 chunks says "OK" (an ACK
/// whose protobuf status is KEPT / NO_ERROR; a short body is read as OK). The
/// body is `u16 request_id, u32 offset, u8 chunk status, u8 status code`.
pub fn answer_ok(body: &[u8]) -> bool {
    body.len() < 8 || (body[6] == 0 && body[7] == 0)
}

#[derive(Debug)]
struct Pending {
    id: u32,
    request_id: u16,
    format: Format,
    if_none_match: Option<String>,
    accept: Option<String>,
    gzip: bool,
    xfer: bool,
}

#[derive(Debug)]
struct Fragment {
    request_id: u16,
    total: usize,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
enum After {
    Nothing,
    /// The request `id` is finished with this outcome once the message is out.
    Done {
        id: u32,
        outcome: GpsAssistOutcome,
    },
    /// A piece of transfer `data_id` is out: `start..end`.
    Chunk {
        data_id: u32,
        start: u32,
        end: u32,
    },
}

#[derive(Debug)]
struct OutMsg {
    request_id: u16,
    bytes: Vec<u8>,
    /// Bytes acknowledged by the watch.
    sent: usize,
    /// Length of the chunk now on the wire.
    last: usize,
    after: After,
}

#[derive(Debug)]
pub struct GpsAssist {
    next_id: u32,
    pending: VecDeque<Pending>,
    fragments: Vec<Fragment>,
    transfers: Transfers,
    queue: VecDeque<OutMsg>,
    in_flight: bool,
}

impl GpsAssist {
    pub fn new(connection_epoch: u32) -> Self {
        Self {
            next_id: 1,
            pending: VecDeque::new(),
            fragments: Vec::new(),
            transfers: Transfers::new(connection_epoch),
            queue: VecDeque::new(),
            in_flight: false,
        }
    }

    // ---- what the watch sends ------------------------------------------------------

    /// One protobuf message (5043 or 5044) from the watch: acknowledge it and
    /// return the whole Smart bytes once they are all there. A message in one
    /// chunk is returned at once; chunks of a larger one are kept (each is
    /// acknowledged with its own offset) and the bytes come back with the last.
    pub fn reassemble(
        &mut self,
        msg: &Message,
        h: &ProtobufHeader,
        body: &[u8],
        out: &mut Out,
    ) -> Option<Vec<u8>> {
        let ack = |out: &mut Out, discarded: bool, code: u8| {
            let mut b = h.request_id.to_le_bytes().to_vec();
            b.extend_from_slice(&h.offset.to_le_bytes());
            b.push(u8::from(discarded));
            b.push(code);
            out.msgs.push(response(msg, 0, &b));
        };
        if h.offset == 0 && h.chunk == h.total {
            out.msgs
                .push(response(msg, 0, &m::build_protobuf_ack(h.request_id)));
            return Some(body.to_vec());
        }
        let total = h.total as usize;
        if total > MAX_INCOMING || body.len() != h.chunk as usize || body.len() > total {
            ack(out, true, CODE_EXCEEDED_TOTAL);
            out.anomaly(AnomalyKind::BadFrame, msg.id);
            return None;
        }
        if h.offset == 0 {
            self.fragments.retain(|f| f.request_id != h.request_id);
            if self.fragments.len() >= MAX_FRAGMENTS {
                self.fragments.remove(0);
            }
            self.fragments.push(Fragment {
                request_id: h.request_id,
                total,
                bytes: body.to_vec(),
            });
            ack(out, false, 0);
            return None;
        }
        let Some(at) = self
            .fragments
            .iter()
            .position(|f| f.request_id == h.request_id)
        else {
            ack(out, true, CODE_UNKNOWN_REQUEST_ID);
            return None;
        };
        let have = self.fragments[at].bytes.len();
        let offset = h.offset as usize;
        if offset != have {
            let code = if offset < have {
                CODE_DUPLICATE_PACKET
            } else {
                CODE_MISSING_PACKET
            };
            ack(out, true, code);
            return None;
        }
        if have + body.len() > self.fragments[at].total {
            self.fragments.remove(at);
            ack(out, true, CODE_EXCEEDED_TOTAL);
            return None;
        }
        self.fragments[at].bytes.extend_from_slice(body);
        ack(out, false, 0);
        if self.fragments[at].bytes.len() == self.fragments[at].total {
            return Some(self.fragments.remove(at).bytes);
        }
        None
    }

    /// The Smart bytes of a whole protobuf message from the watch (already
    /// acknowledged): act on HTTP and data-transfer requests, ignore the rest.
    pub fn on_smart(&mut self, request_id: u16, smart: &[u8], cx: &mut Cx) {
        match http::parse_smart(smart) {
            None => {}
            Some(Incoming::Raw(r)) => self.on_raw(request_id, r, cx),
            Some(Incoming::Web { url }) => {
                let (host, path) = split_for_log(&url);
                cx.out.events.push(Event::HttpRequestRefused { host, path });
                self.queue_msg(request_id, http::web_timed_out(), After::Nothing, cx);
            }
            Some(Incoming::Other { url }) => {
                let (host, path) = url
                    .as_deref()
                    .map_or_else(|| (String::new(), String::new()), split_for_log);
                cx.out.events.push(Event::HttpRequestRefused { host, path });
            }
            Some(Incoming::Unreadable) => {
                cx.out.events.push(Event::HttpRequestRefused {
                    host: String::new(),
                    path: String::new(),
                });
            }
            Some(Incoming::DataDownload {
                id,
                offset,
                max_chunk,
            }) => self.on_download(request_id, id, offset, max_chunk.unwrap_or(0), cx),
            Some(Incoming::FileAccess { data_xfer_id }) => {
                self.on_file_access(request_id, data_xfer_id, cx);
            }
        }
    }

    fn on_raw(&mut self, request_id: u16, r: RawRequest, cx: &mut Cx) {
        let url = http::parse_url(&r.url);
        let (host, path) = url.as_ref().map_or((String::new(), String::new()), |u| {
            (u.host.clone(), u.path.clone())
        });
        let refuse = |this: &mut Self, silent: bool, cx: &mut Cx| {
            cx.out.events.push(Event::HttpRequestRefused {
                host: http::loggable_host(&host),
                path: http::loggable_path(&path),
            });
            if !silent {
                this.queue_msg(request_id, http::raw_timed_out(), After::Nothing, cx);
            }
        };
        if r.has_forms {
            // Gadgetbridge: "requires unsupported MultiPartForm support", no answer.
            refuse(self, true, cx);
            return;
        }
        let Some(url) = url else {
            refuse(self, false, cx);
            return;
        };
        let serves = agps::HOSTS.contains(&url.host.as_str())
            && url.path.starts_with(agps::PATH_PREFIX)
            && r.is_get();
        let address = url.path_and_query();
        let format = if serves && agps::is_safe_path_and_query(&address) {
            agps::classify(&address)
        } else {
            None
        };
        let Some(format) = format else {
            refuse(self, false, cx);
            return;
        };
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        if self.pending.len() >= MAX_PENDING {
            if let Some(old) = self.pending.pop_front() {
                cx.out.events.push(Event::GpsAssistDone {
                    id: old.id,
                    outcome: GpsAssistOutcome::Failed,
                });
            }
        }
        let if_none_match = r.header("if-none-match").and_then(clean_header_value);
        self.pending.push_back(Pending {
            id,
            request_id,
            format,
            if_none_match: if_none_match.clone(),
            accept: r.header("accept").and_then(clean_header_value),
            // `"gzip".equals(accept-encoding)` in `HttpHandler.createRawResponse`.
            gzip: r.header("accept-encoding") == Some("gzip"),
            xfer: r.use_data_xfer,
        });
        cx.out.events.push(Event::GpsAssistRequest {
            id,
            path_and_query: address,
            if_none_match,
        });
    }

    fn on_download(&mut self, request_id: u16, id: u32, offset: u32, max: u32, cx: &mut Cx) {
        let (bytes, after) = match self.transfers.download(id, offset, max) {
            Download::Chunk(c) => (
                http::data_download_response(xfer::STATUS_SUCCESS, id, offset, Some(c)),
                After::Chunk {
                    data_id: id,
                    start: offset,
                    end: offset.saturating_add(c.len() as u32),
                },
            ),
            Download::InvalidId => (
                http::data_download_response(xfer::STATUS_INVALID_ID, id, offset, None),
                After::Nothing,
            ),
            Download::InvalidOffset => (
                http::data_download_response(xfer::STATUS_INVALID_OFFSET, id, offset, None),
                After::Nothing,
            ),
        };
        self.queue_msg(request_id, bytes, after, cx);
    }

    fn on_file_access(&mut self, request_id: u16, data_xfer_id: u32, cx: &mut Cx) {
        let status = match self.transfers.remove(data_xfer_id) {
            Some(tag) => {
                cx.out.events.push(Event::GpsAssistDone {
                    id: tag,
                    outcome: GpsAssistOutcome::NeedsFileAccess,
                });
                http::FILE_ACCESS_UNKNOWN
            }
            None => http::FILE_ACCESS_INVALID_ID,
        };
        self.queue_msg(
            request_id,
            http::file_access_response(status, data_xfer_id),
            After::Nothing,
            cx,
        );
    }

    // ---- what the host answers -----------------------------------------------------

    /// `Input::GpsAssistData`: the file for request `id`.
    pub fn data(&mut self, id: u32, body: &[u8], etag: &str, cx: &mut Cx) {
        let Some(at) = self.pending.iter().position(|p| p.id == id) else {
            return;
        };
        let Some(p) = self.pending.remove(at) else {
            return;
        };
        let reply = match agps::build_reply(
            &p.format,
            body,
            etag,
            p.if_none_match.as_deref(),
            p.accept.as_deref(),
            cx.now.unix_s,
        ) {
            Ok(r) => r,
            Err(why) => {
                self.refuse_data(&p, why, cx);
                return;
            }
        };
        let not_modified = reply.status == 304;
        let done = After::Done {
            id,
            outcome: if not_modified {
                GpsAssistOutcome::NotModified
            } else {
                GpsAssistOutcome::Sent
            },
        };
        let mut headers = reply.headers;
        let bytes = if p.xfer {
            if not_modified {
                let tid = self.transfers.fresh_id();
                self.queue_msg(
                    p.request_id,
                    http::raw_response(reply.status, &headers, Body::Xfer { id: tid, size: 0 }),
                    done,
                    cx,
                );
                return;
            }
            let size = reply.body.len() as u32;
            let (tid, dropped) = self.transfers.register(id, reply.body);
            if let Some(tag) = dropped {
                cx.out.events.push(Event::GpsAssistDone {
                    id: tag,
                    outcome: GpsAssistOutcome::Failed,
                });
            }
            http::raw_response(reply.status, &headers, Body::Xfer { id: tid, size })
        } else if p.gzip && !not_modified {
            headers.push(("Content-Encoding".into(), "gzip".into()));
            let gz = agps::gzip_stored(&reply.body);
            http::raw_response(reply.status, &headers, Body::Inline(&gz))
        } else {
            http::raw_response(reply.status, &headers, Body::Inline(&reply.body))
        };
        let after = if p.xfer { After::Nothing } else { done };
        self.queue_msg(p.request_id, bytes, after, cx);
    }

    fn refuse_data(&mut self, p: &Pending, _why: Invalid, cx: &mut Cx) {
        cx.out.events.push(Event::GpsAssistDone {
            id: p.id,
            outcome: GpsAssistOutcome::Refused,
        });
        self.queue_msg(p.request_id, http::raw_timed_out(), After::Nothing, cx);
    }

    /// `Input::GpsAssistUnavailable`: the host has no file for request `id`.
    pub fn unavailable(&mut self, id: u32, cx: &mut Cx) {
        let Some(at) = self.pending.iter().position(|p| p.id == id) else {
            return;
        };
        let Some(p) = self.pending.remove(at) else {
            return;
        };
        cx.out.events.push(Event::GpsAssistDone {
            id,
            outcome: GpsAssistOutcome::Unavailable,
        });
        self.queue_msg(p.request_id, http::raw_timed_out(), After::Nothing, cx);
    }

    // ---- sending 5044 in chunks ----------------------------------------------------

    fn queue_msg(&mut self, request_id: u16, bytes: Vec<u8>, after: After, cx: &mut Cx) {
        self.queue.push_back(OutMsg {
            request_id,
            bytes,
            sent: 0,
            last: 0,
            after,
        });
        self.pump(cx);
    }

    fn chunk_size(max_packet: u16) -> usize {
        match usize::from(max_packet).checked_sub(CHUNK_OVERHEAD) {
            Some(n) if n > 0 => n,
            _ => DEFAULT_CHUNK,
        }
    }

    /// Send the next chunk of the front message unless one is on its way.
    fn pump(&mut self, cx: &mut Cx) {
        if self.in_flight {
            return;
        }
        let size = Self::chunk_size(cx.max_packet);
        let Some(front) = self.queue.front_mut() else {
            return;
        };
        let total = front.bytes.len();
        let end = (front.sent + size).min(total);
        front.last = end - front.sent;
        let mut payload = front.request_id.to_le_bytes().to_vec();
        payload.extend_from_slice(&(front.sent as u32).to_le_bytes());
        payload.extend_from_slice(&(total as u32).to_le_bytes());
        payload.extend_from_slice(&(front.last as u32).to_le_bytes());
        payload.extend_from_slice(&front.bytes[front.sent..end]);
        self.in_flight = true;
        cx.req
            .send(cx.out, m::PROTOBUF_RESPONSE, payload, Kind::GpsAssistReply);
    }

    /// The watch answered the chunk on the wire. `ok` is "ACK with KEPT and
    /// NO_ERROR" (`answer_ok`).
    pub fn on_answer(&mut self, ok: bool, cx: &mut Cx) {
        self.in_flight = false;
        let Some(front) = self.queue.front_mut() else {
            return;
        };
        if ok {
            front.sent += front.last;
            if front.sent >= front.bytes.len() {
                if let Some(done) = self.queue.pop_front() {
                    self.complete(done.after, cx);
                }
            }
        } else {
            cx.out
                .anomaly(AnomalyKind::DutyMessageRefused, m::PROTOBUF_RESPONSE);
            self.abandon_front(cx);
        }
        self.pump(cx);
    }

    /// The chunk on the wire got no answer in time (the session already
    /// reported it): give that message up.
    pub fn dropped(&mut self, cx: &mut Cx) {
        self.in_flight = false;
        self.abandon_front(cx);
        self.pump(cx);
    }

    fn abandon_front(&mut self, cx: &mut Cx) {
        if let Some(msg) = self.queue.pop_front() {
            // A refused piece of a transfer is asked for again by the watch;
            // a refused reply ends its request.
            if let After::Done { id, .. } = msg.after {
                cx.out.events.push(Event::GpsAssistDone {
                    id,
                    outcome: GpsAssistOutcome::Failed,
                });
            }
        }
    }

    fn complete(&mut self, after: After, cx: &mut Cx) {
        match after {
            After::Nothing => {}
            After::Done { id, outcome } => {
                cx.out.events.push(Event::GpsAssistDone { id, outcome });
            }
            After::Chunk {
                data_id,
                start,
                end,
            } => {
                if let Some((tag, _size)) = self.transfers.received(data_id, start, end) {
                    cx.out.events.push(Event::GpsAssistDone {
                        id: tag,
                        outcome: GpsAssistOutcome::Sent,
                    });
                }
            }
        }
    }
}

/// A header value we pass on to the host: printable ASCII, at most 200 bytes.
fn clean_header_value(v: &str) -> Option<String> {
    let v = v.trim();
    if v.is_empty() || v.len() > 200 || !v.bytes().all(|b| (0x20..0x7F).contains(&b)) {
        return None;
    }
    Some(v.to_string())
}

/// Host and path of an address for `Event::HttpRequestRefused`.
fn split_for_log(url: &str) -> (String, String) {
    match http::parse_url(url) {
        Some(u) => (http::loggable_host(&u.host), http::loggable_path(&u.path)),
        None => (String::new(), String::new()),
    }
}
