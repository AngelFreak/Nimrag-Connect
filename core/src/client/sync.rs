//! The file sync cycle and its safety rule.
//!
//! Cycle: 5007 filter, 5002 for the directory (index 0), then for each
//! pending FIT file: 5002, chunks (5004, each ACKed), a `FileDownloaded`
//! event, and a wait for the caller. Only `FileStored` for exactly that
//! transfer permits the archive request (5008). Every cycle ends with
//! SYNC_COMPLETE, also after failures and for an empty listing.
//!
//! Archive rule: 5008 is built in exactly one place, `on_confirmation`,
//! and only from `State::WaitStore` with a matching transfer id, and only
//! when `ClientConfig::archive` is true. With it false (stage 2g, another sync
//! app archives) the stored file counts as copied, no 5008 is built anywhere,
//! and the cycle moves on.
//!
//! Skip list: an entry whose `FileKey` is in `ClientConfig::skip`, or was
//! stored earlier on this connection with `archive` false, is not downloaded
//! (`Event::FileSkipped`, counted in `SyncFinished::skipped`). A 5009 that
//! carries timestamp 0 does not match a key made from the listing and is
//! downloaded; the host's store deduplicates it. A 5009 for a skip-listed file
//! is ignored without an event and starts no cycle.
//!
//! FileAvailable (5009): the watch announces one new file. Handling is ported
//! from Gadgetbridge commit 7257742b (AGPL-3.0-or-later),
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/FileTransferHandler.java`
//! (`handle`, `conditionallyDownload`), Copyright (C) 2024-2026 Daniele
//! Gobbetti, José Rebelo, Thomas Kuehne and the other Gadgetbridge
//! contributors, and `.../garmin/messages/FileAvailableMessage.java`,
//! Copyright (C) 2026 Thomas Kuehne and the other Gadgetbridge contributors:
//! Gadgetbridge downloads the announced entry directly, without a listing.
//! That is *inferred (Gadgetbridge)*. The rest is ours [ours]: announced files
//! are kept in a short list (`announced`), so an announcement that arrives
//! while the session is busy is fetched in the running cycle or in one of
//! its own right after, and a file that is also in a listing is fetched once.

use std::collections::{HashSet, VecDeque};

use super::session::{response, Kind, Out, Requests};
use super::transport::REPLY_DEADLINE_MS;
use super::{AnomalyKind, ClientConfig, Event, FileFailure, FileKey, SyncTrigger};
use crate::crc;
use crate::gfdi::Message;
use crate::messages::{self as m, DirEntry};

/// Most announced files kept while the session is busy. Further ones are
/// dropped; the watch keeps them pending, so the next full listing has them.
/// [ours]
const MAX_ANNOUNCED: usize = 32;
/// Most files remembered in `handled` (the oldest are forgotten first). [ours]
const MAX_HANDLED: usize = 256;

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
    /// 5043 "sync now" queued or sent; waiting for the watch's 5037. The
    /// grant deadline starts when the watch acknowledges the 5043 (`None`
    /// before), so time spent queued behind other requests does not count.
    WaitGrant { until: Option<u64> },
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
    /// Files announced by 5009 that no cycle has taken yet (at most
    /// `MAX_ANNOUNCED`, no two the same file). They join the end of the
    /// running cycle's file list, or start a `SyncTrigger::FileAvailable`
    /// cycle when nothing else is queued.
    announced: Vec<DirEntry>,
    /// Files a cycle of this connection has finished with: archived, or
    /// failed for any reason. A 5009 for one of them is ignored, so a
    /// re-announcement never starts a cycle that only repeats it, and a file
    /// that keeps failing cannot cause cycle after cycle. Full listing cycles
    /// do not look here: they still retry failed files. [ours]
    handled: VecDeque<DirEntry>,
    entries: Vec<DirEntry>,
    next_entry: usize,
    downloaded: u32,
    archived: u32,
    failed: u32,
    skipped: u32,
    dl: Option<Download>,
    /// `ClientConfig::archive`.
    archive: bool,
    /// `ClientConfig::skip`, plus (archive false) the keys of files stored on
    /// this connection, so a later cycle does not fetch them again.
    skip: HashSet<FileKey>,
}

impl SyncCycle {
    pub fn new(config: &ClientConfig) -> Self {
        Self {
            state: State::Idle,
            cycle: 0,
            next_transfer: (config.connection_epoch as u64) << 32,
            queued: None,
            announced: Vec::new(),
            handled: VecDeque::new(),
            entries: Vec::new(),
            next_entry: 0,
            downloaded: 0,
            archived: 0,
            failed: 0,
            skipped: 0,
            dl: None,
            archive: config.archive,
            skip: config.skip.iter().copied().collect(),
        }
    }

    /// A cycle is in progress (listing, downloading, waiting, archiving).
    pub fn running(&self) -> bool {
        !matches!(self.state, State::Idle | State::WaitGrant { .. })
    }

    /// No cycle, and not waiting for the watch's 5037 either: an upload may
    /// use the link.
    pub fn is_idle(&self) -> bool {
        matches!(self.state, State::Idle)
    }

    /// The watch owes us chunks: the reply deadline applies.
    pub fn awaiting_chunks(&self) -> bool {
        matches!(self.state, State::Download) && self.dl.as_ref().is_some_and(|d| d.size.is_some())
    }

    pub fn next_wake(&self) -> Option<u64> {
        match self.state {
            State::WaitGrant { until } => until,
            State::Retry { at } => Some(at),
            _ => None,
        }
    }

    pub fn on_tick(&mut self, cx: &mut Ctx) {
        match self.state {
            State::WaitGrant { until: Some(until) } if cx.now >= until => {
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

    /// Watch 5009 (already acknowledged by the caller): one new file.
    pub fn on_file_available(&mut self, ready: bool, entry: DirEntry, cx: &mut Ctx) {
        if entry.index == 0 || !is_readable_fit(&entry) {
            return;
        }
        let running = !matches!(self.state, State::Idle);
        // A file to leave alone needs no cycle of its own: no event, no sync.
        if self.skip.contains(&FileKey::of(&entry))
            || self.handled.iter().any(|e| same_file(e, &entry))
            || (running && self.entries.iter().any(|e| same_file(e, &entry)))
        {
            return;
        }
        if self.announced.len() < MAX_ANNOUNCED
            && !self.announced.iter().any(|e| same_file(e, &entry))
        {
            self.announced.push(entry);
        }
        if ready && matches!(self.state, State::Idle) {
            self.start_queued(cx);
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
        self.state = State::WaitGrant { until: None };
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
            Some(SyncTrigger::FileAvailable) => {}
            None if !self.announced.is_empty() => {
                // No filter, no listing: straight to the announced files.
                self.reset_cycle(SyncTrigger::FileAvailable, cx);
                self.next_file(cx);
            }
            None => {}
        }
    }

    fn begin_cycle(&mut self, trigger: SyncTrigger, cx: &mut Ctx) {
        self.reset_cycle(trigger, cx);
        self.state = State::Filter;
        cx.req.send(
            cx.out,
            m::DIRECTORY_FILTER,
            vec![m::FILTER_PENDING],
            Kind::Filter,
        );
    }

    fn reset_cycle(&mut self, trigger: SyncTrigger, cx: &mut Ctx) {
        self.cycle += 1;
        self.downloaded = 0;
        self.archived = 0;
        self.failed = 0;
        self.skipped = 0;
        self.entries.clear();
        self.next_entry = 0;
        self.dl = None;
        cx.out.events.push(Event::SyncStarted {
            cycle: self.cycle,
            trigger,
        });
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
                        self.mark_handled(&entry);
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
                    skipped: self.skipped,
                });
                self.state = State::Idle;
                self.start_queued(cx);
            }
            Kind::SyncNow => {
                if let State::WaitGrant { until: None } = self.state {
                    self.state = State::WaitGrant {
                        until: Some(cx.now.saturating_add(REPLY_DEADLINE_MS)),
                    };
                }
            }
            Kind::Config
            | Kind::FileTypes
            | Kind::HandshakeComplete
            | Kind::FirstConnect(_)
            | Kind::BatteryRequest
            | Kind::NotificationUpdate
            | Kind::NotificationData
            | Kind::MusicInfo
            | Kind::MusicState
            | Kind::MusicVolume
            | Kind::FindWatch
            | Kind::WeatherDefinition
            | Kind::WeatherData
            | Kind::UploadCreate
            | Kind::UploadRequest
            | Kind::UploadData
            | Kind::UploadComplete => {}
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
        self.mark_handled(&entry);
        cx.out.events.push(Event::FileFailed {
            entry,
            reason,
            will_retry,
        });
    }

    // ---- moving through the listing ---------------------------------------------

    fn next_file(&mut self, cx: &mut Ctx) {
        loop {
            while let Some(entry) = self.entries.get(self.next_entry).cloned() {
                self.next_entry += 1;
                if !is_readable_fit(&entry) {
                    self.file_failed(entry, FileFailure::Skipped, false, cx);
                    continue;
                }
                if self.skip.contains(&FileKey::of(&entry)) {
                    self.skipped += 1;
                    self.mark_handled(&entry);
                    cx.out.events.push(Event::FileSkipped { entry });
                    continue;
                }
                self.dl = Some(Download::new(Some(entry)));
                self.request_download(cx);
                return;
            }
            if !self.take_announced() {
                break;
            }
        }
        self.complete(cx);
    }

    fn mark_handled(&mut self, entry: &DirEntry) {
        self.handled.retain(|e| !same_file(e, entry));
        if self.handled.len() >= MAX_HANDLED {
            self.handled.pop_front();
        }
        self.handled.push_back(entry.clone());
    }

    /// The list is used up: append the announced files that are not in it
    /// yet (processed or not) and empty `announced`. True when that added
    /// anything.
    fn take_announced(&mut self) -> bool {
        let before = self.entries.len();
        for entry in std::mem::take(&mut self.announced) {
            if !self.entries.iter().any(|e| same_file(e, &entry)) {
                self.entries.push(entry);
            }
        }
        self.entries.len() > before
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
    /// build the archive request, and only with `archive` set.
    pub fn on_confirmation(&mut self, transfer_id: u64, stored: bool, cx: &mut Ctx) {
        let waiting = match &self.state {
            State::WaitStore { id, entry } if *id == transfer_id => Some(entry.clone()),
            _ => None,
        };
        let Some(entry) = waiting else {
            cx.out.events.push(Event::StaleConfirmation { transfer_id });
            return;
        };
        if stored && !self.archive {
            // Alongside another sync app: copied, never archived. Remember
            // the key so a later cycle of this connection leaves it alone.
            self.skip.insert(FileKey::of(&entry));
            self.mark_handled(&entry);
            self.next_file(cx);
        } else if stored {
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

/// The test `next_file` and `on_file_available` share: a readable FIT file.
fn is_readable_fit(e: &DirEntry) -> bool {
    e.data_type == DATA_TYPE_FIT && e.flags & m::FLAG_READ != 0
}

/// Two entries name the same file when index, type, sub type and file number
/// agree. Flags and size may differ (they change as the watch marks and grows
/// a file), and so may the timestamp: a 5009 can carry 0 where the listing
/// has the real one (Gadgetbridge allows for that). [ours]
fn same_file(a: &DirEntry, b: &DirEntry) -> bool {
    (a.index, a.data_type, a.sub_type, a.file_number)
        == (b.index, b.data_type, b.sub_type, b.file_number)
}
