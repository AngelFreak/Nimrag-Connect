//! Uploads to the watch (stage 4a): a file the host hands over is created on the
//! watch (5005), announced (5003), sent in 5004 chunks, and closed with a
//! SYNC_COMPLETE (5030 event 0). Used for workouts (file type 128/5).
//!
//! Ported from Gadgetbridge commit 7257742b (AGPL-3.0-or-later),
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/FileTransferHandler.java`
//! (`Upload`, `FileFragment.take`, `handle`; Copyright (C) 2024-2026 Daniele
//! Gobbetti, José Rebelo, Thomas Kuehne and the other Gadgetbridge contributors)
//! and the message classes named in `crate::messages` (`CreateFileMessage`,
//! `UploadRequestMessage` and their status classes). The whole upload path is
//! *inferred (Gadgetbridge)*: no capture of our own shows one decoded field by
//! field (docs/protocol/PROTOCOL.md 9.5), so every layout waits for the first
//! upload to a real watch. What we do differently is marked [ours]:
//!
//! - **Chunk size.** The whole COBS frame of a chunk must fit the watch's
//!   `max_packet` (integrator decision 3 of the port survey): one 5004 message is
//!   13 bytes longer than its data, COBS adds one byte per 254 and a final one,
//!   and the two delimiters add two. Gadgetbridge uses `max_packet - 13`, which can
//!   exceed `max_packet` by up to 18 bytes. With `max_packet` 4000 a chunk holds
//!   3969 data bytes, the size of the first chunk of CAP-2's download. [ours]
//! - **File id.** Gadgetbridge sends a random 64-bit id in 5005. `core` has no
//!   randomness, so the id is derived from the caller's clocks, the connection
//!   epoch and a counter, which differ for every upload. [ours]
//! - **Failure never ends the session.** A refusal (a create, upload or transfer
//!   status other than OK), a NAK, a reply that makes no sense or 30 s of silence
//!   ends that upload with `Event::UploadFinished` and an
//!   `AnomalyKind::UploadFailed`, and the session goes on. Gadgetbridge throws
//!   on some of these. When the link closes, every upload that has not finished
//!   is reported `Aborted` before `Event::Closed`. [ours]
//! - **Order.** Uploads run one at a time, in the order they were handed over,
//!   and only when the handshake is done and no sync cycle runs. A sync the
//!   watch asks for while an upload runs waits until the queue is empty
//!   (Gadgetbridge defers its filter request the same way). [ours, after
//!   Gadgetbridge's `runWhenUploadIdle`]
//! - The CRC seed the watch returns in its 5003 answer is ignored, as in
//!   Gadgetbridge: the running CRC starts at 0.

use std::collections::VecDeque;

use super::session::{Kind, Out, Requests};
use super::{AnomalyKind, Event, Now};
use crate::crc;
use crate::messages as m;

/// Uploads waiting behind the running one. A further one is refused
/// (`UploadResult::Invalid`). [ours]
const MAX_QUEUED: usize = 8;
/// A 5004 message is its data plus this many bytes (length 2, id 2, flags 1,
/// crc 2, offset 4, message CRC 2).
const CHUNK_OVERHEAD: usize = 13;

/// Which message of the upload was being answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadStage {
    /// 5005 CreateFile.
    Create,
    /// 5003 UploadFile.
    Request,
    /// 5004 FileData (any chunk).
    Data,
}

impl UploadStage {
    fn message_id(self) -> u16 {
        match self {
            UploadStage::Create => m::CREATE_FILE,
            UploadStage::Request => m::UPLOAD_FILE,
            UploadStage::Data => m::FILE_DATA,
        }
    }
}

/// How an upload ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadResult {
    /// Every byte was acknowledged by the watch.
    Sent,
    /// The watch refused: `status` is its create status (5005: 1 DUPLICATE,
    /// 2 NO_SPACE, 3 UNSUPPORTED, 4 NO_SLOTS, 5 NO_SPACE_FOR_TYPE), upload status
    /// (5003: 1 INDEX_UNKNOWN, 2 INDEX_NOT_WRITEABLE, 3 NO_SPACE_LEFT, 4 INVALID,
    /// 5 NOT_READY, 6 CRC_INCORRECT) or transfer status (5004: 1 RESEND, 2 ABORT,
    /// 3 CRC_MISMATCH, 4 OFFSET_MISMATCH, 5 SYNC_PAUSED). Names are
    /// Gadgetbridge's. *inferred (Gadgetbridge)*
    Refused { stage: UploadStage, status: u8 },
    /// The watch answered with a status other than ACK in its 5000 response.
    Nak { stage: UploadStage, status: u8 },
    /// No answer within 30 s.
    TimedOut { stage: UploadStage },
    /// The answer did not parse, or named an offset that does not fit.
    BadReply { stage: UploadStage },
    /// The link closed before the upload ended (or began).
    Aborted,
    /// Nothing was sent: no data, too much data, a full queue, or a `max_packet`
    /// too small to carry a chunk.
    Invalid,
}

#[derive(Debug)]
struct Waiting {
    id: u64,
    data_type: u8,
    sub_type: u8,
    data: Vec<u8>,
}

#[derive(Debug)]
struct Running {
    up: Waiting,
    stage: UploadStage,
    /// Offset of the first byte not yet sent.
    next: usize,
    /// Offset after the chunk in flight.
    chunk_end: usize,
    /// Running CRC over the bytes sent so far.
    crc: u16,
}

/// The slice of the session an upload call needs.
pub struct Cx<'a> {
    pub now: &'a Now,
    pub req: &'a mut Requests,
    pub out: &'a mut Out,
    /// The watch's `max_packet_size` from its DeviceInformation (0 if unknown).
    pub max_packet: u16,
}

#[derive(Debug)]
pub struct Uploads {
    epoch: u32,
    started: u32,
    queue: VecDeque<Waiting>,
    current: Option<Running>,
}

impl Uploads {
    pub fn new(connection_epoch: u32) -> Self {
        Self {
            epoch: connection_epoch,
            started: 0,
            queue: VecDeque::new(),
            current: None,
        }
    }

    /// An upload runs, or is about to (it waits behind a running one).
    pub fn busy(&self) -> bool {
        self.current.is_some() || !self.queue.is_empty()
    }

    /// `Input::UploadFile`: queue it. Refused (`Invalid`) when empty, over
    /// `MAX_UPLOAD_BYTES`, or when the queue is full.
    pub fn enqueue(&mut self, id: u64, data_type: u8, sub_type: u8, data: Vec<u8>, out: &mut Out) {
        if data.is_empty() || data.len() > m::MAX_UPLOAD_BYTES || self.queue.len() >= MAX_QUEUED {
            out.events.push(Event::UploadFinished {
                upload_id: id,
                result: UploadResult::Invalid,
            });
            return;
        }
        self.queue.push_back(Waiting {
            id,
            data_type,
            sub_type,
            data,
        });
    }

    /// Start the next upload when none runs and `may_start` (handshake done, no
    /// sync cycle).
    pub fn pump(&mut self, may_start: bool, cx: &mut Cx) {
        if self.current.is_some() || !may_start {
            return;
        }
        let Some(up) = self.queue.pop_front() else {
            return;
        };
        self.started = self.started.wrapping_add(1);
        let file_id = file_id(cx.now, self.epoch, self.started);
        cx.req.send(
            cx.out,
            m::CREATE_FILE,
            m::build_create_file(up.data.len() as u32, up.data_type, up.sub_type, file_id),
            Kind::UploadCreate,
        );
        self.current = Some(Running {
            up,
            stage: UploadStage::Create,
            next: 0,
            chunk_end: 0,
            crc: 0,
        });
    }

    /// The watch ACKed an upload message; `body` is what follows the status byte.
    pub fn on_reply(&mut self, kind: Kind, body: &[u8], cx: &mut Cx) {
        let Some(run) = self.current.as_mut() else {
            return;
        };
        match (kind, run.stage) {
            (Kind::UploadCreate, UploadStage::Create) => {
                let Some(reply) = m::parse_create_file_reply(body) else {
                    return self.fail(
                        UploadResult::BadReply {
                            stage: UploadStage::Create,
                        },
                        true,
                        cx,
                    );
                };
                if reply.create_status != m::CREATE_OK {
                    let result = UploadResult::Refused {
                        stage: UploadStage::Create,
                        status: reply.create_status,
                    };
                    return self.fail(result, true, cx);
                }
                run.stage = UploadStage::Request;
                cx.req.send(
                    cx.out,
                    m::UPLOAD_FILE,
                    m::build_upload_request(reply.file_index, run.up.data.len() as u32),
                    Kind::UploadRequest,
                );
            }
            (Kind::UploadRequest, UploadStage::Request) => {
                let Some(reply) = m::parse_upload_reply(body) else {
                    return self.fail(
                        UploadResult::BadReply {
                            stage: UploadStage::Request,
                        },
                        true,
                        cx,
                    );
                };
                if reply.upload_status != m::UPLOAD_OK {
                    let result = UploadResult::Refused {
                        stage: UploadStage::Request,
                        status: reply.upload_status,
                    };
                    return self.fail(result, true, cx);
                }
                // Gadgetbridge insists on offset 0 here.
                if reply.offset != 0 {
                    return self.fail(
                        UploadResult::BadReply {
                            stage: UploadStage::Request,
                        },
                        true,
                        cx,
                    );
                }
                run.stage = UploadStage::Data;
                self.send_chunk(cx);
            }
            (Kind::UploadData, UploadStage::Data) => {
                let Some((status, offset)) = m::parse_upload_chunk_reply(body) else {
                    return self.fail(
                        UploadResult::BadReply {
                            stage: UploadStage::Data,
                        },
                        true,
                        cx,
                    );
                };
                if status != m::TRANSFER_OK {
                    let result = UploadResult::Refused {
                        stage: UploadStage::Data,
                        status,
                    };
                    return self.fail(result, true, cx);
                }
                let total = run.up.data.len();
                if offset as usize >= total {
                    self.finish_sent(cx);
                } else if offset as usize == run.chunk_end {
                    run.next = run.chunk_end;
                    self.send_chunk(cx);
                } else {
                    self.fail(
                        UploadResult::BadReply {
                            stage: UploadStage::Data,
                        },
                        true,
                        cx,
                    );
                }
            }
            // An answer that belongs to no upload step in progress.
            _ => {}
        }
    }

    /// The watch answered an upload message with a status other than ACK.
    pub fn on_nak(&mut self, kind: Kind, status: u8, cx: &mut Cx) {
        if let Some(stage) = stage_of(kind) {
            if self.current.as_ref().is_some_and(|r| r.stage == stage) {
                self.fail(UploadResult::Nak { stage, status }, true, cx);
            }
        }
    }

    /// 30 s passed without an answer; the session already reported the anomaly.
    pub fn on_timeout(&mut self, kind: Kind, cx: &mut Cx) {
        if let Some(stage) = stage_of(kind) {
            if self.current.as_ref().is_some_and(|r| r.stage == stage) {
                self.fail(UploadResult::TimedOut { stage }, false, cx);
            }
        }
    }

    /// Ids of every upload that has not finished, running one first. The caller
    /// reports them `Aborted`.
    pub fn take_unfinished(&mut self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.current.take().map(|r| r.up.id).into_iter().collect();
        ids.extend(self.queue.drain(..).map(|u| u.id));
        ids
    }

    fn send_chunk(&mut self, cx: &mut Cx) {
        let Some(run) = self.current.as_mut() else {
            return;
        };
        let room = chunk_data_size(cx.max_packet);
        if room == 0 {
            return self.fail(UploadResult::Invalid, true, cx);
        }
        let end = (run.next + room).min(run.up.data.len());
        let piece = &run.up.data[run.next..end];
        run.crc = crc::update(run.crc, piece);
        let payload = m::build_file_chunk(&m::FileChunk {
            flags: 0,
            crc: run.crc,
            offset: run.next as u32,
            data: piece.to_vec(),
        });
        run.chunk_end = end;
        cx.req.send(cx.out, m::FILE_DATA, payload, Kind::UploadData);
    }

    fn finish_sent(&mut self, cx: &mut Cx) {
        let Some(run) = self.current.take() else {
            return;
        };
        cx.out.events.push(Event::UploadFinished {
            upload_id: run.up.id,
            result: UploadResult::Sent,
        });
        // Gadgetbridge closes an upload with SYNC_COMPLETE (event 0, value 0).
        cx.req.send(
            cx.out,
            m::SYSTEM_EVENT,
            m::build_system_event(m::EVENT_SYNC_COMPLETE, 0),
            Kind::UploadComplete,
        );
    }

    /// End the running upload. `anomaly` is false when the session already
    /// reported one (a timeout).
    fn fail(&mut self, result: UploadResult, anomaly: bool, cx: &mut Cx) {
        let Some(run) = self.current.take() else {
            return;
        };
        if anomaly {
            let id = match result {
                UploadResult::Refused { stage, .. }
                | UploadResult::Nak { stage, .. }
                | UploadResult::BadReply { stage }
                | UploadResult::TimedOut { stage } => stage.message_id(),
                _ => run.stage.message_id(),
            };
            cx.out.anomaly(AnomalyKind::UploadFailed, id);
        }
        cx.out.events.push(Event::UploadFinished {
            upload_id: run.up.id,
            result,
        });
    }
}

fn stage_of(kind: Kind) -> Option<UploadStage> {
    match kind {
        Kind::UploadCreate => Some(UploadStage::Create),
        Kind::UploadRequest => Some(UploadStage::Request),
        Kind::UploadData => Some(UploadStage::Data),
        _ => None,
    }
}

/// Data bytes one 5004 chunk may carry so that the whole COBS frame (two
/// delimiters, the code bytes) fits `max_packet`; 0 when not even one byte
/// fits. The bound is the worst case, so it does not depend on the data. [ours]
pub fn chunk_data_size(max_packet: u16) -> usize {
    let max = usize::from(max_packet);
    // The frame of an L-byte message is at most L + L / 254 + 1 (COBS) + 2 bytes.
    let mut len = max.saturating_sub(3);
    while len > 0 && len + len / 254 + 3 > max {
        len -= 1;
    }
    len.saturating_sub(CHUNK_OVERHEAD)
}

/// The id sent in 5005: derived, different for every upload of a connection and
/// for equal uploads at different times (splitmix64 over the inputs). [ours]
fn file_id(now: &Now, epoch: u32, n: u32) -> u64 {
    let mut x = (now.unix_s as u64)
        ^ now.mono_ms.rotate_left(29)
        ^ ((u64::from(epoch) << 32) | u64::from(n));
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cobs;
    use crate::gfdi::Message;

    #[test]
    fn a_4000_byte_link_carries_3969_data_bytes_like_the_first_chunk_in_cap_2() {
        assert_eq!(chunk_data_size(4000), 3969);
    }

    #[test]
    fn every_chunk_frame_fits_max_packet_and_one_more_byte_would_not() {
        for max_packet in [20u16, 100, 255, 256, 400, 1000, 2047, 4000, 4094, 5000] {
            let n = chunk_data_size(max_packet);
            if n == 0 {
                continue;
            }
            // The worst case for COBS: no zero byte in the message.
            let frame = |data: usize| {
                let chunk = m::build_file_chunk(&m::FileChunk {
                    flags: 0,
                    crc: 0xFFFF,
                    offset: 0x0101_0101,
                    data: vec![0x55; data],
                });
                let msg = Message {
                    id: m::FILE_DATA,
                    txn: Some(7),
                    payload: chunk,
                };
                cobs::encode_frame(&msg.encode()).len()
            };
            assert!(
                frame(n) <= usize::from(max_packet),
                "max_packet {max_packet}"
            );
            assert!(
                frame(n + 1) > usize::from(max_packet),
                "max_packet {max_packet}: {n} is not the largest"
            );
        }
    }

    #[test]
    fn tiny_links_carry_nothing() {
        assert_eq!(chunk_data_size(0), 0);
        assert_eq!(chunk_data_size(16), 0);
        assert_eq!(chunk_data_size(17), 1);
        assert_eq!(chunk_data_size(18), 2);
    }

    #[test]
    fn file_ids_differ_by_time_and_by_upload() {
        let t = |mono_ms, unix_s| Now {
            mono_ms,
            unix_s,
            utc_offset_s: 0,
            dst: None,
        };
        let a = file_id(&t(10, 1_800_000_000), 1, 1);
        assert_ne!(a, file_id(&t(10, 1_800_000_000), 1, 2));
        assert_ne!(a, file_id(&t(11, 1_800_000_000), 1, 1));
        assert_ne!(a, file_id(&t(10, 1_800_000_001), 1, 1));
        assert_ne!(a, file_id(&t(10, 1_800_000_000), 2, 1));
        assert_eq!(a, file_id(&t(10, 1_800_000_000), 1, 1));
    }
}
