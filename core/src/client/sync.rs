//! The file sync cycle and its safety rule.
//!
//! Cycle: 5007 filter, 5002 for the directory (index 0), then for each
//! pending FIT file: 5002, chunks (5004, each ACKed), a `FileDownloaded`
//! event, and a wait for the caller. Only `FileStored` for exactly that
//! transfer permits the archive request (5008). Every cycle ends with
//! SYNC_COMPLETE, also after failures and for an empty listing.
//!
//! Archive rule: 5008 is built in exactly one place, `on_confirmation`,
//! and only from `State::WaitStore` with a matching transfer id.

use super::session::{response, Kind, Out, Requests};
use super::transport::REPLY_DEADLINE_MS;
use super::{AnomalyKind, Event, FileFailure, SyncTrigger};
use crate::crc;
use crate::gfdi::Message;
use crate::messages::{self as m, DirEntry};

/// Retry delay and count for a "not ready" (status 3) download reply.
const NOT_READY_DELAY_MS: u64 = 1_000;
const NOT_READY_RETRIES: u8 = 5;
/// FIT files only: data type 128. [PROTOCOL.md 5.6]
const DATA_TYPE_FIT: u8 = 128;
/// Each file is requested at most twice per cycle (one retry after a CRC or
/// offset failure).
const MAX_ATTEMPTS: u8 = 2;

/// The session's view handed to every sync call.
pub struct Ctx<'a> {
    pub now: u64,
    pub req: &'a mut Requests,
    pub out: &'a mut Out,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum State {
    #[default]
    Idle,
    /// 5043 "sync now" sent; waiting for the watch's 5037.
    WaitGrant { until: u64 },
    /// 5007 outstanding.
    Filter,
    /// 5002 outstanding, or receiving its chunks (`dl` is set).
    Download,
    /// Status 3 from the watch: re-send 5002 at `at`.
    Retry { at: u64 },
    /// File delivered; frozen until the caller answers.
    WaitStore { id: u64, entry: DirEntry },
    /// 5008 outstanding.
    Archive { id: u64, entry: DirEntry },
    /// 5030 SYNC_COMPLETE outstanding.
    Complete,
}

#[derive(Debug)]
struct Download {
    /// None for the directory itself.
    entry: Option<DirEntry>,
    size: Option<u32>,
    data: Vec<u8>,
    crc: u16,
    attempts: u8,
    not_ready: u8,
}

impl Download {
    fn new(entry: Option<DirEntry>) -> Self {
        Self {
            entry,
            size: None,
            data: Vec::new(),
            crc: 0,
            attempts: 1,
            not_ready: 0,
        }
    }

    fn index(&self) -> u16 {
        self.entry.as_ref().map_or(0, |e| e.index)
    }

    fn restart(&mut self) {
        self.size = None;
        self.data.clear();
        self.crc = 0;
        self.attempts += 1;
        self.not_ready = 0;
    }
}

#[derive(Debug)]
pub struct SyncCycle {
    state: State,
    cycle: u32,
    /// `(connection epoch << 32) | n`, n counting from 1 per connection.
    next_transfer: u64,
    /// A trigger that arrived while busy (at most one).
    queued: Option<SyncTrigger>,
    entries: Vec<DirEntry>,
    next_entry: usize,
    downloaded: u32,
    archived: u32,
    failed: u32,
    dl: Option<Download>,
}

impl SyncCycle {
    /// A cycle is in progress (listing, downloading, waiting, archiving).
    pub fn new(connection_epoch: u32) -> Self {
        Self {
            state: State::Idle,
            cycle: 0,
            next_transfer: (connection_epoch as u64) << 32,
            queued: None,
            entries: Vec::new(),
            next_entry: 0,
            downloaded: 0,
            archived: 0,
            failed: 0,
            dl: None,
        }
    }

    pub fn running(&self) -> bool {
        !matches!(self.state, State::Idle | State::WaitGrant { .. })
    }

    /// The watch owes us chunks: the reply deadline applies.
    pub fn awaiting_chunks(&self) -> bool {
        matches!(self.state, State::Download) && self.dl.as_ref().is_some_and(|d| d.size.is_some())
    }

    pub fn next_wake(&self) -> Option<u64> {
        match self.state {
            State::WaitGrant { until } => Some(until),
            State::Retry { at } => Some(at),
            _ => None,
        }
    }

    pub fn on_tick(&mut self, cx: &mut Ctx) {
        match self.state {
            State::WaitGrant { until } if cx.now >= until => {
                self.state = State::Idle;
                cx.out.anomaly(AnomalyKind::SyncNotGranted, m::SYNC_REQUEST);
                self.start_queued(cx);
            }
            State::Retry { at } if cx.now >= at => self.request_download(cx),
            _ => {}
        }
    }

    // ---- triggers ---------------------------------------------------------

    /// Handshake finished: run a trigger that arrived early.
    pub fn on_ready(&mut self, cx: &mut Ctx) {
        self.start_queued(cx);
    }

    /// `Input::StartSync`.
    pub fn start_manual(&mut self, ready: bool, cx: &mut Ctx) {
        if ready && matches!(self.state, State::Idle) {
            self.send_sync_now(cx);
        } else if self.queued.is_none() {
            self.queued = Some(SyncTrigger::Caller);
        }
    }

    /// Watch 5037 (already acknowledged by the caller).
    pub fn on_watch_request(&mut self, ready: bool, option: u8, cx: &mut Ctx) {
        match self.state {
            State::Idle if ready => self.begin_cycle(SyncTrigger::Watch { option }, cx),
            State::WaitGrant { .. } => self.begin_cycle(SyncTrigger::Caller, cx),
            _ => {
                if self.queued.is_none() {
                    self.queued = Some(SyncTrigger::Watch { option });
                }
            }
        }
    }

    fn send_sync_now(&mut self, cx: &mut Ctx) {
        let id = cx.req.next_protobuf_id();
        cx.req.send(
            cx.out,
            m::PROTOBUF_REQUEST,
            m::build_protobuf(id, &m::SYNC_NOW_PROTOBUF),
            Kind::SyncNow,
        );
        self.state = State::WaitGrant {
            until: cx.now.saturating_add(REPLY_DEADLINE_MS),
        };
    }

    fn start_queued(&mut self, cx: &mut Ctx) {
        if !matches!(self.state, State::Idle) {
            return;
        }
        match self.queued.take() {
            Some(SyncTrigger::Watch { option }) => {
                self.begin_cycle(SyncTrigger::Watch { option }, cx)
            }
            Some(SyncTrigger::Caller) => self.send_sync_now(cx),
            None => {}
        }
    }

    fn begin_cycle(&mut self, trigger: SyncTrigger, cx: &mut Ctx) {
        self.cycle += 1;
        self.downloaded = 0;
        self.archived = 0;
        self.failed = 0;
        self.entries.clear();
        self.next_entry = 0;
        self.dl = None;
        cx.out.events.push(Event::SyncStarted {
            cycle: self.cycle,
            trigger,
        });
        self.state = State::Filter;
        cx.req.send(
            cx.out,
            m::DIRECTORY_FILTER,
            vec![m::FILTER_PENDING],
            Kind::Filter,
        );
    }

    // ---- answers to our requests (status already checked: ACK) --------------

    pub fn on_response(&mut self, kind: Kind, body: &[u8], cx: &mut Ctx) {
        match kind {
            Kind::Filter => {
                self.dl = Some(Download::new(None));
                self.request_download(cx);
            }
            Kind::Download => self.on_download_reply(body, cx),
            Kind::Archive => {
                if let State::Archive { id, entry } = self.state.clone() {
                    if body.first() == Some(&0) {
                        self.archived += 1;
                        cx.out.events.push(Event::FileArchived { transfer_id: id });
                    } else {
                        self.file_failed(entry, FileFailure::ArchiveRefused, false, cx);
                    }
                    self.next_file(cx);
                }
            }
            Kind::SyncComplete => {
                cx.out.events.push(Event::SyncFinished {
                    cycle: self.cycle,
                    downloaded: self.downloaded,
                    archived: self.archived,
                    failed: self.failed,
                });
                self.state = State::Idle;
                self.start_queued(cx);
            }
            Kind::SyncNow | Kind::Config | Kind::FileTypes | Kind::HandshakeComplete => {}
        }
    }

    fn request_download(&mut self, cx: &mut Ctx) {
        let Some(dl) = self.dl.as_ref() else {
            return;
        };
        self.state = State::Download;
        cx.req.send(
            cx.out,
            m::DOWNLOAD_FILE,
            m::build_download_request(&m::DownloadRequest {
                index: dl.index(),
                offset: 0,
                kind: 1,
                crc_seed: 0,
                max_size: 0,
                flags: 1,
            }),
            Kind::Download,
        );
    }

    fn on_download_reply(&mut self, body: &[u8], cx: &mut Ctx) {
        let Some(dl) = self.dl.as_mut() else {
            return;
        };
        let Some(reply) = m::parse_download_reply(body) else {
            cx.out.anomaly(AnomalyKind::BadFrame, m::DOWNLOAD_FILE);
            return self.fail_download(FileFailure::Refused { status: 0xFF }, cx);
        };
        match reply.status {
            0 if reply.size == 0 => self.fail_download(FileFailure::Skipped, cx),
            0 => dl.size = Some(reply.size),
            3 if dl.not_ready < NOT_READY_RETRIES => {
                dl.not_ready += 1;
                self.state = State::Retry {
                    at: cx.now.saturating_add(NOT_READY_DELAY_MS),
                };
            }
            status => self.fail_download(FileFailure::Refused { status }, cx),
        }
    }

    // ---- chunks ----------------------------------------------------------------

    /// A watch 5004 chunk. Returns true when it was accepted (progress).
    pub fn on_chunk(&mut self, msg: &Message, cx: &mut Ctx) -> bool {
        let Some(chunk) = m::parse_file_chunk(&msg.payload) else {
            cx.out.anomaly(AnomalyKind::BadFrame, msg.id);
            return false;
        };
        let ack = |status: u8, received: usize| {
            response(msg, 0, &m::build_chunk_ack(status, received as u32))
        };
        let Some(dl) = self.dl.as_mut().filter(|d| d.size.is_some()) else {
            // No transfer is waiting for data: tell the watch to stop.
            cx.out.msgs.push(ack(m::CHUNK_ABORT, 0));
            cx.out
                .anomaly(AnomalyKind::UnexpectedResponse, m::FILE_DATA);
            return false;
        };
        let received = dl.data.len();
        if chunk.offset as usize != received {
            cx.out.msgs.push(ack(m::CHUNK_OFFSET_MISMATCH, received));
            self.retry_or_fail(FileFailure::Offset, cx);
            return false;
        }
        let running = crc::update(dl.crc, &chunk.data);
        let total = received + chunk.data.len();
        let size = dl.size.unwrap_or(0) as usize;
        if running != chunk.crc || total > size {
            cx.out.msgs.push(ack(m::CHUNK_CRC_MISMATCH, received));
            self.retry_or_fail(FileFailure::Crc, cx);
            return false;
        }
        dl.crc = running;
        dl.data.extend_from_slice(&chunk.data);
        cx.out.msgs.push(ack(m::CHUNK_OK, total));
        if total == size {
            self.download_complete(cx);
        }
        // An empty chunk is not progress: it must not extend the deadline.
        !chunk.data.is_empty()
    }

    /// 5054 (compressed data) is not supported: during a download the file
    /// fails and the cycle goes on; otherwise it is an anomaly. Either way the
    /// watch is told to abort.
    pub fn on_compressed(&mut self, msg: &Message, cx: &mut Ctx) {
        let received = self.dl.as_ref().map_or(0, |d| d.data.len());
        cx.out.msgs.push(response(
            msg,
            0,
            &m::build_chunk_ack(m::CHUNK_ABORT, received as u32),
        ));
        if matches!(self.state, State::Download) && self.dl.is_some() {
            self.fail_download(FileFailure::Compressed, cx);
        } else {
            cx.out.anomaly(AnomalyKind::UnexpectedResponse, msg.id);
        }
    }

    /// The watch answered 5002 or 5008 with a non-ACK status: that file fails,
    /// the cycle continues.
    pub fn on_nak(&mut self, kind: Kind, status: u8, cx: &mut Ctx) {
        match kind {
            Kind::Download => self.fail_download(FileFailure::Refused { status }, cx),
            Kind::Archive => {
                if let State::Archive { entry, .. } = self.state.clone() {
                    self.file_failed(entry, FileFailure::ArchiveRefused, false, cx);
                    self.next_file(cx);
                }
            }
            _ => {}
        }
    }

    fn download_complete(&mut self, cx: &mut Ctx) {
        let Some(dl) = self.dl.take() else {
            return;
        };
        match dl.entry {
            None => {
                self.entries = m::parse_directory(&dl.data)
                    .into_iter()
                    .filter(|e| e.index != 0)
                    .collect();
                self.next_entry = 0;
                cx.out.events.push(Event::Directory {
                    cycle: self.cycle,
                    entries: self.entries.clone(),
                });
                self.next_file(cx);
            }
            Some(entry) => {
                self.next_transfer += 1;
                let id = self.next_transfer;
                self.downloaded += 1;
                cx.out.events.push(Event::FileDownloaded {
                    transfer_id: id,
                    entry: entry.clone(),
                    data: dl.data,
                });
                self.state = State::WaitStore { id, entry };
            }
        }
    }

    /// First CRC/offset failure: report and request the file again from
    /// offset 0. Second failure: give up on the file for this cycle.
    fn retry_or_fail(&mut self, reason: FileFailure, cx: &mut Ctx) {
        let Some(dl) = self.dl.as_mut() else {
            return;
        };
        if dl.attempts < MAX_ATTEMPTS {
            if let Some(entry) = dl.entry.clone() {
                cx.out.events.push(Event::FileFailed {
                    entry,
                    reason,
                    will_retry: true,
                });
            }
            dl.restart();
            self.request_download(cx);
        } else {
            self.fail_download(reason, cx);
        }
    }

    /// Give up on the current download. A failed directory ends the cycle.
    fn fail_download(&mut self, reason: FileFailure, cx: &mut Ctx) {
        let Some(dl) = self.dl.take() else {
            return;
        };
        match dl.entry {
            Some(entry) => {
                self.file_failed(entry, reason, false, cx);
                self.next_file(cx);
            }
            None => self.complete(cx),
        }
    }

    fn file_failed(
        &mut self,
        entry: DirEntry,
        reason: FileFailure,
        will_retry: bool,
        cx: &mut Ctx,
    ) {
        self.failed += 1;
        cx.out.events.push(Event::FileFailed {
            entry,
            reason,
            will_retry,
        });
    }

    // ---- moving through the listing ---------------------------------------------

    fn next_file(&mut self, cx: &mut Ctx) {
        while let Some(entry) = self.entries.get(self.next_entry).cloned() {
            self.next_entry += 1;
            if entry.data_type != DATA_TYPE_FIT || entry.flags & m::FLAG_READ == 0 {
                self.file_failed(entry, FileFailure::Skipped, false, cx);
                continue;
            }
            self.dl = Some(Download::new(Some(entry)));
            self.request_download(cx);
            return;
        }
        self.complete(cx);
    }

    fn complete(&mut self, cx: &mut Ctx) {
        self.dl = None;
        self.state = State::Complete;
        cx.req.send(
            cx.out,
            m::SYSTEM_EVENT,
            m::build_system_event(m::EVENT_SYNC_COMPLETE, 0),
            Kind::SyncComplete,
        );
    }

    // ---- the caller's answer -------------------------------------------------------

    /// `FileStored` (`stored`) or `FileStoreFailed`. The only place that can
    /// build the archive request.
    pub fn on_confirmation(&mut self, transfer_id: u64, stored: bool, cx: &mut Ctx) {
        let waiting = match &self.state {
            State::WaitStore { id, entry } if *id == transfer_id => Some(entry.clone()),
            _ => None,
        };
        let Some(entry) = waiting else {
            cx.out.events.push(Event::StaleConfirmation { transfer_id });
            return;
        };
        if stored {
            cx.req.send(
                cx.out,
                m::SET_FILE_FLAGS,
                m::build_set_file_flags(entry.index, m::FLAG_ARCHIVE),
                Kind::Archive,
            );
            self.state = State::Archive {
                id: transfer_id,
                entry,
            };
        } else {
            self.file_failed(entry, FileFailure::StoreFailed, false, cx);
            self.next_file(cx);
        }
    }
}
