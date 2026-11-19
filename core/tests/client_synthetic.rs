//! Synthetic sessions through `Client::handle`: a fake watch built in code
//! with fake ids, no capture data. See IMPLEMENTATION_PLAN.md stage 2a, 6.

mod support;

use nimrag_core::capture::Direction;
use nimrag_core::client::reliable::WINDOW;
use nimrag_core::client::DstPair;
use nimrag_core::client::{
    AnomalyKind, BatteryReport, Client, CloseReason, Event, FileFailure, FileKey, Input, Phase,
    SyncTrigger,
};
use nimrag_core::gfdi::Message;
use nimrag_core::link::{Event as LinkEvent, LinkDecoder};
use nimrag_core::messages::{self as m, DirEntry};
use nimrag_core::mlr;
use nimrag_core::multilink;
use support::*;

const FILE_A: u16 = 71;
const FILE_B: u16 = 72;

fn two_file_sim() -> Sim {
    let mut sim = Sim::new();
    sim.watch
        .add_file(fake_entry(FILE_A, 32, 2000), lcg_bytes(1, 2000));
    sim.watch
        .add_file(fake_entry(FILE_B, 4, 300), lcg_bytes(2, 300));
    sim
}

fn latest_downloaded_id(sim: &Sim) -> u64 {
    sim.downloaded().last().expect("a file was downloaded").0
}

/// Run one full cycle: store every delivered file, return when finished.
fn store_everything(sim: &mut Sim) {
    let mut done = 0;
    while sim.downloaded().len() > done {
        let id = sim.downloaded()[done].0;
        done += 1;
        sim.input(Input::FileStored { transfer_id: id });
    }
}

fn ack_body(request_id: u16, body: &[u8]) -> Vec<u8> {
    let mut v = request_id.to_le_bytes().to_vec();
    v.push(0);
    v.extend_from_slice(body);
    v
}

fn chunk_ack(received: u32) -> Vec<u8> {
    ack_body(5004, &m::build_chunk_ack(0, received))
}

fn dl_request(index: u16) -> Vec<u8> {
    m::build_download_request(&m::DownloadRequest {
        index,
        offset: 0,
        kind: 1,
        crc_seed: 0,
        max_size: 0,
        flags: 1,
    })
}

// ---- 6.1 happy path --------------------------------------------------------------

#[test]
fn happy_path_two_files_then_empty_listing_matches_golden_sequence() {
    let mut sim = two_file_sim();
    sim.handshake();
    sim.watch_sends(&Message::new(5052, 0x0102_0304u32.to_le_bytes().to_vec()));
    sim.watch_sends(&sim.watch.sync_request(2));
    assert_eq!(sim.downloaded().len(), 1, "first file waits for the caller");
    let dir_events = sim.events_of(|e| matches!(e, Event::Directory { .. }));
    assert_eq!(dir_events.len(), 1);
    store_everything(&mut sim);
    sim.watch_sends(&sim.watch.sync_request(1));

    let host = m::build_host_info(&m::HostInfo {
        protocol: 150,
        app_version: 1234,
        name: "TestPhone".into(),
        manufacturer: "TestCo".into(),
        model: "T-1".into(),
    });
    let time_reply = m::build_current_time_reply(&m::CurrentTimeReply {
        reference: 0x0102_0304,
        garmin_time: garmin(UNIX_NOW),
        utc_offset_s: 3600,
        dst_start: garmin(UNIX_NOW + 86_400 * 30),
        dst_end: garmin(UNIX_NOW + 86_400 * 200),
    });
    let ack = |id: u16| ack_body(id, &[]);
    let expected: Vec<(u16, Option<u8>, Vec<u8>)> = vec![
        (5000, None, ack_body(5024, &host)),
        (5000, None, ack(5050)),
        (5050, Some(0), vec![3, 0x11, 0x22, 0x33]),
        (5031, Some(1), vec![]),
        (5030, Some(2), vec![8, 0]),
        (5000, None, ack_body(5052, &time_reply)),
        (5000, None, ack(5037)),
        (5007, Some(3), vec![3]),
        (5002, Some(4), dl_request(0)),
        (5000, None, chunk_ack(48)),
        (5002, Some(5), dl_request(FILE_A)),
        (5000, None, chunk_ack(380)),
        (5000, None, chunk_ack(760)),
        (5000, None, chunk_ack(1140)),
        (5000, None, chunk_ack(1520)),
        (5000, None, chunk_ack(1900)),
        (5000, None, chunk_ack(2000)),
        (5008, Some(6), vec![FILE_A as u8, 0, 0x10]),
        (5002, Some(7), dl_request(FILE_B)),
        (5000, None, chunk_ack(300)),
        (5008, Some(8), vec![FILE_B as u8, 0, 0x10]),
        (5030, Some(9), vec![0, 0]),
        (5000, None, ack(5037)),
        (5007, Some(10), vec![3]),
        (5002, Some(11), dl_request(0)),
        (5000, None, chunk_ack(32)),
        (5030, Some(12), vec![0, 0]),
    ];
    let got: Vec<(u16, Option<u8>, Vec<u8>)> = sim
        .watch
        .phone_msgs
        .iter()
        .map(|msg| (msg.id, msg.txn, msg.payload.clone()))
        .collect();
    assert_eq!(
        got.len(),
        expected.len(),
        "message count; got ids {:?}",
        sim.phone_ids()
    );
    for (i, (g, e)) in got.iter().zip(&expected).enumerate() {
        assert_eq!(g, e, "message #{i}");
    }

    // Events: both files delivered intact, archived in order, two cycles.
    let downloaded = sim.downloaded();
    assert_eq!(downloaded.len(), 2);
    assert_eq!(downloaded[0].2, lcg_bytes(1, 2000));
    assert_eq!(downloaded[1].2, lcg_bytes(2, 300));
    assert_eq!(sim.watch.archived_indices, vec![FILE_A, FILE_B]);
    let finished: Vec<_> = sim
        .events
        .iter()
        .filter_map(|e| match e {
            Event::SyncFinished {
                cycle,
                downloaded,
                archived,
                failed,
                ..
            } => Some((*cycle, *downloaded, *archived, *failed)),
            _ => None,
        })
        .collect();
    assert_eq!(finished, vec![(1, 2, 2, 0), (2, 0, 0, 0)]);
    assert!(sim.events.contains(&Event::SyncStarted {
        cycle: 1,
        trigger: SyncTrigger::Watch { option: 2 }
    }));
    assert!(sim
        .events
        .iter()
        .any(|e| matches!(e, Event::HandshakeDone { .. })));
    assert_eq!(sim.client.phase(), Phase::Ready);
    assert_archive_rule(&sim);
}

// ---- 6.2 seam check against the capture decoder -------------------------------------

#[test]
fn whole_exchange_decodes_cleanly_and_files_match() {
    let mut sim = two_file_sim();
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    store_everything(&mut sim);

    let mut dec = LinkDecoder::new();
    let mut events = Vec::new();
    for (d, att) in sim.trace_atts() {
        events.extend(dec.push(d, &att));
    }
    for e in &events {
        assert!(
            !matches!(
                e,
                LinkEvent::Unknown { .. }
                    | LinkEvent::GfdiError { .. }
                    | LinkEvent::MlrOutOfOrder { .. }
                    | LinkEvent::MlrDuplicate { .. }
            ),
            "decoder complained: {e:?}"
        );
    }
    let decoded: Vec<(u16, Vec<u8>)> = events
        .iter()
        .filter_map(|e| match e {
            LinkEvent::FileDownloaded {
                index,
                data,
                crc_ok,
                ..
            } if *index != 0 => {
                assert!(crc_ok);
                Some((*index, data.clone()))
            }
            _ => None,
        })
        .collect();
    let ours: Vec<(u16, Vec<u8>)> = sim
        .downloaded()
        .into_iter()
        .map(|(_, e, d)| (e.index, d))
        .collect();
    assert_eq!(decoded, ours);
    assert_eq!(ours.len(), 2);
    assert!(events
        .iter()
        .any(|e| matches!(e, LinkEvent::Gfdi { direction: Direction::Sent, message, .. } if message.id == 5008)));
}

// ---- 6.3 nothing is archived without FileStored ----------------------------------------

#[test]
fn no_archive_before_stored_while_time_passes_and_time_is_answered() {
    let mut sim = two_file_sim();
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    assert_eq!(sim.downloaded().len(), 1);
    for _ in 0..4 {
        sim.advance(60_000);
        sim.watch_sends(&Message::new(5052, 7u32.to_le_bytes().to_vec()));
    }
    assert!(
        sim.closed_reason().is_none(),
        "the caller owns the deadline"
    );
    assert_eq!(
        sim.events_of(|e| matches!(e, Event::TimeSent { .. })).len(),
        4
    );
    assert!(sim.phone_requests(5008).is_empty());
    assert!(sim.watch.archived_indices.is_empty());
    assert_eq!(sim.phone_requests(5002).len(), 2, "no second download yet");

    let id = latest_downloaded_id(&sim);
    sim.input(Input::FileStoreFailed { transfer_id: id });
    assert!(sim.events.iter().any(|e| matches!(
        e,
        Event::FileFailed {
            reason: FileFailure::StoreFailed,
            will_retry: false,
            ..
        }
    )));
    assert_eq!(sim.downloaded().len(), 2, "next file is fetched");
    assert!(
        sim.phone_requests(5008).is_empty(),
        "no 5008 for the failed one"
    );
    // The second file is also withheld until confirmed.
    sim.advance(30_000);
    assert!(sim.phone_requests(5008).is_empty());
}

/// Whatever else happens: a 5008 is only ever sent for a file that was
/// delivered by `FileDownloaded` and then confirmed with `FileStored` for
/// that very transfer, once.
fn assert_archive_rule(sim: &Sim) {
    let mut allowed: Vec<u16> = sim
        .downloaded()
        .into_iter()
        .filter(|(id, _, _)| sim.stored.contains(id))
        .map(|(_, entry, _)| entry.index)
        .collect();
    for req in sim.phone_requests(5008) {
        let (index, flags) = m::parse_set_file_flags(&req.payload).unwrap();
        assert_eq!(flags, 0x10);
        let pos = allowed
            .iter()
            .position(|i| *i == index)
            .unwrap_or_else(|| panic!("5008 for index {index} without a matching FileStored"));
        allowed.remove(pos);
    }
}

// ---- 6.4 stale confirmations ---------------------------------------------------------------

#[test]
fn stale_confirmations_write_nothing() {
    let mut sim = two_file_sim();
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    let before = sim.phone_writes.len();
    sim.input(Input::FileStored { transfer_id: 999 });
    assert_eq!(sim.phone_writes.len(), before, "unknown id writes nothing");
    assert!(sim
        .events
        .contains(&Event::StaleConfirmation { transfer_id: 999 }));

    let id = latest_downloaded_id(&sim);
    sim.input(Input::FileStored { transfer_id: id });
    let archived = sim.watch.archived_indices.clone();
    assert_eq!(archived, vec![FILE_A]);
    let before = sim.phone_writes.len();
    sim.input(Input::FileStored { transfer_id: id });
    assert!(
        sim.events
            .iter()
            .filter(|e| matches!(e, Event::StaleConfirmation { transfer_id } if *transfer_id == id))
            .count()
            >= 1
    );
    assert_eq!(sim.phone_writes.len(), before, "repeated id writes nothing");
    assert_eq!(sim.watch.archived_indices, archived);
    assert_archive_rule(&sim);
}

/// The exact hazard: client A delivers a file and the link drops; client B
/// (new connection) downloads its own first file and waits; A's id arrives.
#[test]
fn confirmation_from_a_previous_connection_never_archives_the_new_download() {
    let mut sim = two_file_sim();
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    let old_id = latest_downloaded_id(&sim);
    assert_eq!(old_id & 0xFFFF_FFFF, 1);
    sim.input(Input::Disconnected);

    sim.reconnect();
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    let new_id = latest_downloaded_id(&sim);
    assert_ne!(new_id, old_id, "ids differ across connections");
    assert_eq!(new_id & 0xFFFF_FFFF, 1, "same per-connection number");
    let before = sim.phone_writes.len();
    sim.input(Input::FileStored {
        transfer_id: old_id,
    });
    assert_eq!(sim.phone_writes.len(), before, "nothing written");
    assert!(sim.events.contains(&Event::StaleConfirmation {
        transfer_id: old_id
    }));
    assert!(sim.phone_requests(5008).is_empty(), "no 5008 follows");
    assert!(sim.watch.archived_indices.is_empty());
    // The new download is still waiting and its own id works.
    sim.input(Input::FileStored {
        transfer_id: new_id,
    });
    assert_eq!(sim.watch.archived_indices, vec![FILE_A]);
    assert_archive_rule(&sim);
}

// ---- 5054 and per-file refusals (plan 10) --------------------------------------------------

#[test]
fn compressed_data_fails_that_file_and_the_cycle_continues() {
    let mut sim = Sim::new();
    sim.watch
        .add_file(fake_entry(FILE_A, 32, 100), lcg_bytes(3, 100));
    sim.watch
        .add_file(fake_entry(FILE_B, 4, 100), lcg_bytes(4, 100));
    sim.watch.compress_file = Some(FILE_A);
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    assert!(sim.events.iter().any(|e| matches!(
        e,
        Event::FileFailed { entry, reason: FileFailure::Compressed, will_retry: false } if entry.index == FILE_A
    )));
    assert_eq!(sim.downloaded().len(), 1);
    assert_eq!(sim.downloaded()[0].1.index, FILE_B);
    store_everything(&mut sim);
    assert_eq!(sim.watch.archived_indices, vec![FILE_B]);
    assert!(sim.closed_reason().is_none());
}

#[test]
fn compressed_data_outside_a_download_is_an_anomaly() {
    let mut sim = Sim::new();
    sim.handshake();
    sim.watch_sends(&Message::new(5054, vec![0, 0, 0, 0, 0, 0, 0, 1]));
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::UnexpectedResponse,
        message_id: 5054
    }));
    assert!(sim.closed_reason().is_none());
}

#[test]
fn nak_to_a_download_fails_that_file_only() {
    let mut sim = two_file_sim();
    sim.watch.nak_download = Some(FILE_A);
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    assert!(sim.events.iter().any(|e| matches!(
        e,
        Event::FileFailed { entry, reason: FileFailure::Refused { status: 1 }, .. } if entry.index == FILE_A
    )));
    store_everything(&mut sim);
    assert_eq!(sim.watch.archived_indices, vec![FILE_B]);
    assert!(sim.closed_reason().is_none());
    assert_eq!(sim.client.phase(), Phase::Ready);
}

#[test]
fn nak_to_an_archive_request_fails_that_file_and_continues() {
    let mut sim = two_file_sim();
    sim.watch.nak_archive = Some(FILE_A);
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    store_everything(&mut sim);
    assert!(sim.events.iter().any(|e| matches!(
        e,
        Event::FileFailed { entry, reason: FileFailure::ArchiveRefused, .. } if entry.index == FILE_A
    )));
    assert_eq!(sim.watch.archived_indices, vec![FILE_B]);
    assert!(sim.closed_reason().is_none());
}

#[test]
fn a_download_that_goes_silent_still_closes_after_30_seconds() {
    let mut sim = two_file_sim();
    sim.watch.stall_after = Some(1);
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    sim.advance(30_000);
    assert_eq!(
        sim.closed_reason(),
        Some(CloseReason::Timeout {
            phase: Phase::Syncing
        })
    );
}

// ---- 6.5 chunk failures --------------------------------------------------------------------------

fn failure_sim(faults: Vec<Fault>) -> Sim {
    let mut sim = two_file_sim();
    sim.watch.faults.insert(FILE_A, faults.into());
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    sim
}

fn chunk_ack_statuses(sim: &Sim) -> Vec<u8> {
    sim.phone_responses(5004)
        .iter()
        .map(|msg| msg.as_response().unwrap().body[0])
        .collect()
}

#[test]
fn crc_mismatch_once_is_nacked_then_the_retry_is_archived() {
    let mut sim = failure_sim(vec![Fault::BadCrc]);
    assert!(chunk_ack_statuses(&sim).contains(&m::CHUNK_CRC_MISMATCH));
    assert!(sim.events.iter().any(|e| matches!(
        e,
        Event::FileFailed {
            reason: FileFailure::Crc,
            will_retry: true,
            ..
        }
    )));
    assert_eq!(sim.downloaded().len(), 1);
    assert_eq!(sim.downloaded()[0].2, lcg_bytes(1, 2000));
    assert_eq!(
        sim.phone_requests(5002).len(),
        3,
        "directory + file + retry"
    );
    store_everything(&mut sim);
    assert_eq!(sim.watch.archived_indices, vec![FILE_A, FILE_B]);
}

#[test]
fn crc_mismatch_twice_gives_up_without_archive_and_still_completes() {
    let mut sim = failure_sim(vec![Fault::BadCrc, Fault::BadCrc]);
    assert!(sim.events.iter().any(|e| matches!(
        e,
        Event::FileFailed {
            reason: FileFailure::Crc,
            will_retry: false,
            ..
        }
    )));
    store_everything(&mut sim);
    assert_eq!(
        sim.watch.archived_indices,
        vec![FILE_B],
        "failed file stays pending"
    );
    assert!(sim.downloaded().iter().all(|(_, e, _)| e.index != FILE_A));
    let last = sim.watch.phone_msgs.last().unwrap();
    assert_eq!((last.id, last.payload.clone()), (5030, vec![0, 0]));
    assert!(sim.events.iter().any(|e| matches!(
        e,
        Event::SyncFinished {
            downloaded: 1,
            archived: 1,
            failed: 1,
            ..
        }
    )));
    assert_eq!(sim.watch.pending_files().len(), 1);
}

#[test]
fn offset_mismatch_is_nacked_with_the_offset_status() {
    let sim = failure_sim(vec![Fault::BadOffset]);
    assert!(chunk_ack_statuses(&sim).contains(&m::CHUNK_OFFSET_MISMATCH));
    assert!(sim.events.iter().any(|e| matches!(
        e,
        Event::FileFailed {
            reason: FileFailure::Offset,
            will_retry: true,
            ..
        }
    )));
    assert_eq!(sim.downloaded()[0].2, lcg_bytes(1, 2000));
}

#[test]
fn data_beyond_the_announced_size_is_a_size_mismatch() {
    let sim = failure_sim(vec![Fault::OversizeData, Fault::OversizeData]);
    assert!(chunk_ack_statuses(&sim).contains(&m::CHUNK_CRC_MISMATCH));
    assert!(sim.events.iter().any(|e| matches!(
        e,
        Event::FileFailed {
            reason: FileFailure::Crc,
            will_retry: false,
            ..
        }
    )));
    assert!(sim.downloaded().iter().all(|(_, e, _)| e.index != FILE_A));
}

#[test]
fn not_ready_is_retried_after_one_second_up_to_five_times() {
    let mut sim = two_file_sim();
    sim.watch
        .faults
        .insert(FILE_A, vec![Fault::NotReady; 2].into());
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    assert!(sim.downloaded().is_empty());
    sim.advance(999);
    assert!(sim.downloaded().is_empty());
    sim.advance(1);
    sim.advance(1000);
    assert_eq!(sim.downloaded().len(), 1);

    let mut sim = two_file_sim();
    sim.watch
        .faults
        .insert(FILE_A, vec![Fault::NotReady; 6].into());
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    sim.advance(10_000);
    assert!(sim.events.iter().any(|e| matches!(
        e,
        Event::FileFailed {
            reason: FileFailure::Refused { status: 3 },
            ..
        }
    )));
}

#[test]
fn refusal_and_skipped_entries_are_not_downloaded() {
    let mut sim = Sim::new();
    sim.watch
        .add_file(fake_entry(FILE_A, 32, 100), lcg_bytes(3, 100));
    let mut not_fit = fake_entry(50, 0, 100);
    not_fit.data_type = 8;
    sim.watch.add_file(not_fit, lcg_bytes(4, 100));
    let mut unreadable = fake_entry(51, 4, 100);
    unreadable.flags = 0x20;
    sim.watch.add_file(unreadable, lcg_bytes(5, 100));
    sim.watch
        .faults
        .insert(FILE_A, vec![Fault::Refuse(2)].into());
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    let reasons: Vec<_> = sim
        .events
        .iter()
        .filter_map(|e| match e {
            Event::FileFailed { entry, reason, .. } => Some((entry.index, *reason)),
            _ => None,
        })
        .collect();
    assert!(reasons.contains(&(FILE_A, FileFailure::Refused { status: 2 })));
    assert!(reasons.contains(&(50, FileFailure::Skipped)));
    assert!(reasons.contains(&(51, FileFailure::Skipped)));
    assert!(sim.downloaded().is_empty());
    assert_eq!(
        sim.phone_requests(5002).len(),
        2,
        "only directory and FILE_A requested"
    );
    assert_eq!(sim.client.phase(), Phase::Ready);
}

// ---- 6.6 disconnect mid-download ----------------------------------------------------------------------

#[test]
fn disconnect_mid_download_delivers_nothing_and_a_new_client_starts_over() {
    let mut sim = two_file_sim();
    sim.watch.stall_after = Some(2);
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    // The directory and two chunks of FILE_A arrived, then the radio went.
    assert_eq!(sim.phone_responses(5004).len(), 1 + 2);
    assert!(sim.downloaded().is_empty());
    let step = sim.input(Input::Disconnected);
    assert!(step.events.contains(&Event::Closed {
        reason: CloseReason::Disconnected
    }));
    assert!(sim.downloaded().is_empty(), "no partial delivery");
    assert_eq!(sim.client.phase(), Phase::Closed);
    for input in [
        Input::Tick,
        Input::StartSync,
        Input::FileStored { transfer_id: 1 },
    ] {
        let after = sim.client.handle(test_now(sim.t + 1), input);
        assert!(
            after.writes.is_empty() && after.events.is_empty() && after.wake_at_mono_ms.is_none()
        );
    }

    sim.reconnect();
    sim.watch.stall_after = None;
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    // Fresh sequence numbers and transaction ids.
    let first_mlr = sim
        .watch
        .phone_raw
        .iter()
        .find(|raw| raw[0] & 0x80 != 0 && raw.len() > 2)
        .unwrap();
    assert_eq!(mlr::parse(first_mlr).unwrap().0.seq, 0);
    assert_eq!(
        sim.watch
            .phone_msgs
            .iter()
            .find(|msg| msg.id == 5050)
            .unwrap()
            .txn,
        Some(0)
    );
    let files = sim.downloaded();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].2, lcg_bytes(1, 2000));
    assert_eq!(
        files[0].0,
        (2u64 << 32) | 1,
        "transfer ids carry the connection epoch"
    );
}

// ---- 6.7 time ---------------------------------------------------------------------------------------------

#[test]
fn time_reply_carries_reference_garmin_time_offset_and_dst() {
    let mut sim = Sim::new();
    sim.handshake();
    sim.t = 5_000;
    sim.watch_sends(&Message::new(5052, 0xDEAD_BEEFu32.to_le_bytes().to_vec()));
    let reply = sim.phone_responses(5052).pop().unwrap();
    let r = reply.as_response().unwrap();
    let t = m::parse_current_time_reply(r.body).unwrap();
    assert_eq!(t.reference, 0xDEAD_BEEF);
    assert_eq!(t.garmin_time, garmin(UNIX_NOW + 5));
    assert_eq!(t.utc_offset_s, 3600);
    assert_eq!(t.dst_start, garmin(UNIX_NOW + 86_400 * 30));
    assert_eq!(t.dst_end, garmin(UNIX_NOW + 86_400 * 200));
    assert!(sim.events.contains(&Event::TimeSent {
        garmin_time: garmin(UNIX_NOW + 5),
        utc_offset_s: 3600
    }));
}

fn time_reply_for(dst: Option<DstPair>, unix_s: i64) -> m::CurrentTimeReply {
    let mut sim = Sim::new();
    sim.handshake();
    sim.unix_s_override = Some(unix_s);
    sim.dst_override = Some(dst);
    sim.watch_sends(&Message::new(5052, 1u32.to_le_bytes().to_vec()));
    let reply = sim.phone_responses(5052).pop().expect("a 5052 reply");
    m::parse_current_time_reply(reply.as_response().unwrap().body).unwrap()
}

#[test]
fn summer_time_reply_has_the_start_after_the_end() {
    // In summer the next transition is the end; the start is the later one.
    let end = UNIX_NOW + 86_400 * 40;
    let start = UNIX_NOW + 86_400 * 220;
    let t = time_reply_for(
        Some(DstPair {
            start_unix_s: start,
            end_unix_s: end,
        }),
        UNIX_NOW,
    );
    assert_eq!((t.dst_start, t.dst_end), (garmin(start), garmin(end)));
    assert!(t.dst_start > t.dst_end);
}

#[test]
fn no_dst_gives_zero_in_both_fields() {
    let t = time_reply_for(None, UNIX_NOW);
    assert_eq!((t.dst_start, t.dst_end), (0, 0));
    assert_eq!(t.garmin_time, garmin(UNIX_NOW));
}

#[test]
fn invalid_clock_sends_nothing_and_reports_it() {
    let mut sim = Sim::new();
    sim.handshake();
    sim.unix_s_override = Some(1_000_000_000);
    let before = sim.phone_writes.len();
    sim.watch_sends(&Message::new(5052, 1u32.to_le_bytes().to_vec()));
    assert!(sim.phone_responses(5052).is_empty());
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::ClockInvalid,
        message_id: 5052
    }));
    assert!(sim
        .events_of(|e| matches!(e, Event::TimeSent { .. }))
        .is_empty());
    // Only the watch's own MLR traffic (an ack) may have been written.
    for (_, w) in &sim.phone_writes[before..] {
        assert_eq!(w.len(), 2, "no GFDI data written");
    }
}

// ---- 6.8 unsupported and auth ------------------------------------------------------------------------------------

#[test]
fn unsupported_requests_get_status_two_and_an_event() {
    let mut sim = Sim::new();
    sim.handshake();
    for id in [5036u16, 5014, 5999] {
        sim.watch_sends(&Message::new(id, vec![1, 2, 3]));
        let reply = sim.phone_responses(id).pop().expect("a reply");
        let r = reply.as_response().unwrap();
        assert_eq!(r.status, nimrag_core::gfdi::Status::UnknownOrNotSupported);
        assert!(r.body.is_empty());
        assert!(sim.events.contains(&Event::Unsupported { message_id: id }));
    }
}

#[test]
fn auth_negotiation_closes_without_a_write() {
    let mut sim = Sim::new();
    sim.handshake();
    let before = sim.phone_writes.len();
    sim.watch_sends(&Message::new(5101, vec![]));
    assert_eq!(sim.closed_reason(), Some(CloseReason::AuthRequested));
    assert_eq!(sim.phone_writes.len(), before, "no write after 5101");
}

// ---- 6.9 StartSync ------------------------------------------------------------------------------------------------------

#[test]
fn start_sync_sends_the_protobuf_and_follows_the_manual_5037() {
    let mut sim = two_file_sim();
    sim.handshake();
    sim.input(Input::StartSync);
    let proto = sim.phone_requests(5043);
    assert_eq!(proto.len(), 1);
    let (h, body) = m::parse_protobuf(&proto[0].payload).unwrap();
    assert_eq!((h.request_id, h.offset, h.total, h.chunk), (0, 0, 6, 6));
    assert_eq!(body, [0x6A, 0x04, 0x0A, 0x02, 0x08, 0x00]);
    assert!(proto[0].txn.is_some());
    // The fake watch answered with 5044 and 5037 MANUAL.
    let acks = sim.phone_responses(5044);
    assert_eq!(acks.len(), 1);
    assert_eq!(
        acks[0].as_response().unwrap().body,
        m::build_protobuf_ack(0)
    );
    assert!(sim.events.contains(&Event::SyncStarted {
        cycle: 1,
        trigger: SyncTrigger::Caller
    }));
    assert_eq!(sim.downloaded().len(), 1);
}

#[test]
fn start_sync_without_a_5037_reports_not_granted_after_30_seconds() {
    let mut sim = Sim::new();
    sim.watch.grant_sync_on_protobuf = false;
    sim.handshake();
    sim.input(Input::StartSync);
    sim.advance(29_999);
    assert!(sim
        .events_of(|e| matches!(
            e,
            Event::Anomaly {
                kind: AnomalyKind::SyncNotGranted,
                ..
            }
        ))
        .is_empty());
    sim.advance(1);
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::SyncNotGranted,
        message_id: 5037
    }));
    assert_eq!(sim.client.phase(), Phase::Ready);
    assert!(
        sim.phone_requests(5007).is_empty(),
        "no listing without the watch's 5037"
    );
    assert!(sim.closed_reason().is_none());
}

#[test]
fn a_5037_during_a_cycle_is_acked_and_runs_one_more_cycle() {
    let mut sim = Sim::new();
    sim.watch
        .add_file(fake_entry(FILE_A, 32, 100), lcg_bytes(9, 100));
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    // File delivered, waiting for the caller: two more 5037s arrive.
    sim.watch_sends(&sim.watch.sync_request(1));
    sim.watch_sends(&sim.watch.sync_request(1));
    assert_eq!(sim.phone_responses(5037).len(), 3, "all acknowledged");
    store_everything(&mut sim);
    let started = sim
        .events_of(|e| matches!(e, Event::SyncStarted { .. }))
        .len();
    assert_eq!(started, 2, "one queued, the duplicate dropped");
    let finished = sim
        .events_of(|e| matches!(e, Event::SyncFinished { .. }))
        .len();
    assert_eq!(finished, 2);
}

// ---- 6.10 MLR through the client ----------------------------------------------------------------------------------------------

#[test]
fn lost_phone_packet_is_retransmitted_with_backoff_and_the_current_ack() {
    let mut sim = Sim::new();
    sim.handshake();
    // The reply to an unsupported request is lost, again and again; no
    // request of ours is outstanding, so only the MLR timer runs.
    sim.advance(100); // let bring-up acknowledgements settle
    let start = sim.phone_writes.len();
    sim.drop_phone_writes = usize::MAX;
    sim.watch_sends(&Message::new(5036, vec![1]));
    let data_writes = |sim: &Sim| {
        sim.phone_writes[start..]
            .iter()
            .filter(|(_, w)| w.len() > 2)
            .count()
    };
    let first = data_writes(&sim);
    assert_eq!(first, 1);
    let original_ack = mlr::parse(&sim.phone_writes.last().unwrap().1)
        .unwrap()
        .0
        .ack;
    // The watch says one more thing afterwards: the phone's ack moves on.
    sim.watch_sends(&Message::new(5000, vec![0x9A, 0x13, 0]));
    // Timeouts double up to 20 s: retransmissions at 2, 6, 14, 30, 50, 70 s.
    let mut elapsed = 0;
    for (i, at) in [2_000u64, 6_000, 14_000, 30_000, 50_000, 70_000]
        .into_iter()
        .enumerate()
    {
        sim.advance(at - 1 - elapsed);
        assert_eq!(data_writes(&sim), first + i, "early before {at} ms");
        sim.advance(1);
        assert_eq!(data_writes(&sim), first + i + 1, "retransmit at {at} ms");
        elapsed = at;
    }
    // The resent packet carries the current ack, not the original one.
    let (_, last) = sim.phone_writes.last().unwrap();
    let header = mlr::parse(last).unwrap().0;
    assert_eq!(header.ack, (original_ack + 1) & 0x3F);
    assert!(sim.closed_reason().is_none());
}

#[test]
fn gap_from_the_watch_delivers_nothing_until_retransmission() {
    let mut sim = Sim::new();
    sim.handshake();
    // A message that needs 3 packets; the first is lost.
    let msg = Message::new(5036, vec![7u8; 40]);
    sim.drop_watch_packets = 1;
    sim.watch_sends(&msg);
    assert!(sim
        .events_of(|e| matches!(e, Event::Unsupported { .. }))
        .is_empty());
    // The phone's ack covers an earlier packet, so the watch's timer
    // restarted at 10 ms: the retransmission comes just after 2 s.
    sim.advance(2_100);
    assert_eq!(
        sim.events_of(|e| matches!(e, Event::Unsupported { .. }))
            .len(),
        1
    );
}

#[test]
fn acks_after_five_packets_and_after_ten_ms() {
    let mut sim = Sim::new();
    sim.handshake();
    // A stray response elicits no reply, so only an ack-only can answer.
    let before = sim.phone_writes.len();
    sim.watch_sends(&Message::new(
        5000,
        vec![
            0x1A, 0x13, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
            21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42,
            43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
            65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80,
        ],
    ));
    let acks = sim.phone_writes[before..]
        .iter()
        .filter(|(_, w)| w.len() == 2)
        .count();
    assert_eq!(acks, 1, "one ack-only right after the fifth packet");

    let before = sim.phone_writes.len();
    sim.watch_sends(&Message::new(5000, vec![0x1A, 0x13, 0]));
    assert_eq!(
        sim.phone_writes.len(),
        before,
        "a single packet waits for the timer"
    );
    let wake = sim.wake.expect("ack delay armed");
    assert_eq!(wake, sim.t + 10);
    sim.advance(9);
    assert_eq!(sim.phone_writes.len(), before);
    sim.advance(1);
    assert_eq!(sim.phone_writes.len(), before + 1);
    assert_eq!(sim.phone_writes.last().unwrap().1.len(), 2);
}

#[test]
fn window_of_32_holds_back_and_sequence_numbers_wrap() {
    let mut sim = Sim::new();
    sim.handshake();
    sim.advance(100); // let bring-up acknowledgements settle
    let start = sim.phone_writes.len();
    // 100 watch requests arrive before any phone reply reaches the watch.
    sim.drop_phone_writes = usize::MAX;
    let mut packets = Vec::new();
    for i in 0..100u8 {
        let msg = Message {
            id: 5036,
            txn: Some(i & 31),
            payload: vec![i],
        };
        packets.extend(sim.watch.send(sim.t, &msg));
    }
    for p in packets {
        sim.deliver(p);
    }
    let data_since = |sim: &Sim| {
        sim.phone_writes[start..]
            .iter()
            .filter(|(_, w)| w.len() > 2)
            .count()
    };
    assert_eq!(
        data_since(&sim),
        WINDOW,
        "only a window's worth is in flight"
    );
    // Radio recovers; the retransmission timer restarts the flow.
    sim.drop_phone_writes = 0;
    sim.advance(60_000);
    let replies = sim.phone_responses(5036);
    assert_eq!(replies.len(), 100, "every reply reached the watch once");
    let txns: Vec<u8> = replies.iter().map(|r| r.txn.unwrap()).collect();
    let expected: Vec<u8> = (0..100u8).map(|i| i & 31).collect();
    assert_eq!(txns, expected, "in order");
    let seqs: Vec<u8> = sim.phone_writes[start..]
        .iter()
        .filter(|(_, w)| w.len() > 2)
        .map(|(_, w)| mlr::parse(w).unwrap().0.seq)
        .collect();
    assert!(
        seqs.first().is_some_and(|f| *f > 0) && seqs.contains(&63) && seqs.contains(&0),
        "sequence numbers wrapped"
    );
}

// ---- 6.11 failures ------------------------------------------------------------------------------------------------------------------

#[test]
fn register_already_in_use_closes_with_the_retry_characteristic() {
    let mut sim = Sim::new();
    sim.watch.register_status.insert(4, 3);
    sim.connect();
    assert_eq!(
        sim.closed_reason(),
        Some(CloseReason::ChannelInUse { retry_char: 0x2811 })
    );
}

#[test]
fn register_rejection_names_the_service() {
    let mut sim = Sim::new();
    sim.watch.register_status.insert(1, 4);
    sim.connect();
    assert_eq!(
        sim.closed_reason(),
        Some(CloseReason::RegisterRejected {
            service: 1,
            status: 4
        })
    );
}

// ---- plain mode and recovery from a watch-side close (inferred, Gadgetbridge) ----------

#[test]
fn gfdi_without_the_reliable_flag_runs_in_plain_mode() {
    let mut sim = two_file_sim();
    sim.watch.grant_reliable = false;
    sim.handshake();
    assert!(sim.events.contains(&Event::LinkReady));
    assert!(sim
        .events
        .iter()
        .any(|e| matches!(e, Event::HandshakeDone { .. })));
    assert!(
        sim.watch.endpoint().is_none(),
        "no MLR endpoint on the watch"
    );
    sim.watch_sends(&sim.watch.sync_request(2));
    store_everything(&mut sim);
    sim.watch_sends(&sim.watch.sync_request(1));

    assert!(sim.closed_reason().is_none());
    assert_eq!(sim.client.phase(), Phase::Ready);
    let downloaded = sim.downloaded();
    assert_eq!(downloaded.len(), 2);
    assert_eq!(downloaded[0].2, lcg_bytes(1, 2000));
    assert_eq!(downloaded[1].2, lcg_bytes(2, 300));
    assert_eq!(sim.watch.archived_indices, vec![FILE_A, FILE_B]);
    assert_archive_rule(&sim);

    // On the wire: after the bring-up every GFDI write is the plain handle and
    // at most MTU - 4 bytes; there is not one MLR packet in either direction.
    let gfdi_writes: Vec<&Vec<u8>> = sim
        .phone_writes
        .iter()
        .map(|(_, w)| w)
        .filter(|w| w[0] == PLAIN_GFDI_HANDLE)
        .collect();
    assert!(gfdi_writes.len() > 5);
    assert!(gfdi_writes.iter().all(|w| w.len() <= MTU as usize - 3));
    assert!(sim.trace.iter().all(|(_, v)| v[0] & 0x80 == 0 || v[0] == 0));
}

/// A reliable session in which the watch closes the GFDI handle between two
/// sync cycles. The watch grants MLR again (or not, `grant_after`).
fn gfdi_closed_by_the_watch_then_a_sync(grant_after: bool) -> Sim {
    let mut sim = two_file_sim();
    sim.handshake();
    let writes_before = sim.phone_writes.len();
    sim.watch.grant_reliable = grant_after;
    let close = sim.watch.close_gfdi();
    sim.deliver(close);

    // The transport registered GFDI again, in the bring-up mode, and said so.
    assert_eq!(
        sim.phone_writes[writes_before..]
            .iter()
            .map(|(_, w)| w.clone())
            .collect::<Vec<_>>(),
        vec![multilink::register_request(CLIENT_ID, 1, 2)]
    );
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::GfdiClosedByWatch,
        message_id: 0
    }));
    assert!(sim.closed_reason().is_none());
    assert_eq!(sim.client.phase(), Phase::Ready, "the session is not reset");
    assert_eq!(
        sim.events_of(|e| matches!(e, Event::LinkReady)).len(),
        1,
        "bring-up is not repeated"
    );
    assert_eq!(sim.watch.endpoint().is_some(), grant_after);
    // Nothing is pending: no timer is left to close the session.
    sim.advance(120_000);
    assert!(sim.closed_reason().is_none());

    // The new stream carries a full sync cycle.
    sim.watch_sends(&sim.watch.sync_request(2));
    store_everything(&mut sim);
    sim.watch_sends(&sim.watch.sync_request(1));
    assert!(sim.closed_reason().is_none());
    assert_eq!(sim.watch.archived_indices, vec![FILE_A, FILE_B]);
    assert_eq!(sim.downloaded()[0].2, lcg_bytes(1, 2000));
    assert_archive_rule(&sim);
    sim
}

#[test]
fn watch_closing_gfdi_mid_session_is_recovered_with_a_fresh_mlr_stream() {
    let sim = gfdi_closed_by_the_watch_then_a_sync(true);
    // Sequence numbers restarted: the first data packet after the
    // re-registration is seq 0 again, on handle 3. The handshake had already
    // used several numbers, so a continued stream would show a later one.
    let after = sim
        .phone_writes
        .iter()
        .rposition(|(_, w)| *w == multilink::register_request(CLIENT_ID, 1, 2))
        .and_then(|first| {
            sim.phone_writes[first + 1..]
                .iter()
                .position(|(_, w)| w.len() > 2)
                .map(|i| first + 1 + i)
        })
        .expect("data after the re-registration");
    let (header, _) = mlr::parse(&sim.phone_writes[after].1).unwrap();
    assert_eq!((header.handle, header.seq), (3, 0));
}

#[test]
fn watch_closing_gfdi_may_be_answered_with_a_plain_handle() {
    let sim = gfdi_closed_by_the_watch_then_a_sync(false);
    assert!(sim.watch.endpoint().is_none());
    assert!(sim
        .phone_writes
        .iter()
        .any(|(_, w)| w[0] == PLAIN_GFDI_HANDLE));
}

#[test]
fn unanswered_re_registration_closes_after_the_reply_deadline() {
    let mut sim = Sim::new();
    sim.handshake();
    let close = sim.watch.close_gfdi();
    sim.watch.mute = true;
    sim.deliver(close);
    assert!(sim.closed_reason().is_none());
    assert_eq!(sim.client.phase(), Phase::BringUp, "while re-registering");
    sim.advance(29_999);
    assert!(sim.closed_reason().is_none());
    sim.advance(1);
    assert_eq!(
        sim.closed_reason(),
        Some(CloseReason::Timeout {
            phase: Phase::BringUp
        })
    );
}

#[test]
fn watch_closing_gfdi_during_a_download_closes_at_once_naming_the_cause() {
    let mut sim = two_file_sim();
    sim.watch.stall_after = Some(1);
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    assert_eq!(sim.client.phase(), Phase::Syncing);
    let writes_before = sim.phone_writes.len();
    let close = sim.watch.close_gfdi();
    sim.deliver(close);
    assert_eq!(
        sim.closed_reason(),
        Some(CloseReason::GfdiClosed {
            phase: Phase::Syncing
        })
    );
    assert_eq!(sim.phone_writes.len(), writes_before, "nothing written");
    assert!(sim.downloaded().is_empty());
    assert!(sim.watch.archived_indices.is_empty());
}

#[test]
fn a_5024_after_a_reopen_runs_the_handshake_again() {
    let mut sim = two_file_sim();
    sim.handshake();
    let close = sim.watch.close_gfdi();
    sim.deliver(close);
    assert_eq!(sim.client.phase(), Phase::Ready);
    let info = sim.watch.device_information();
    sim.watch_sends(&info);
    assert_eq!(sim.client.phase(), Phase::Ready, "handshake finished again");
    assert_eq!(sim.phone_requests(5050).len(), 2);
    assert_eq!(sim.phone_requests(5031).len(), 2);
    let done = sim.events_of(|e| matches!(e, Event::HandshakeDone { .. }));
    assert_eq!(done.len(), 2);
    sim.watch_sends(&sim.watch.sync_request(2));
    store_everything(&mut sim);
    assert_eq!(sim.watch.archived_indices, vec![FILE_A, FILE_B]);
    assert!(sim.closed_reason().is_none());
}

#[test]
fn a_sync_asked_for_while_gfdi_reopens_goes_out_on_the_new_handle() {
    let mut sim = two_file_sim();
    sim.handshake();
    let close = sim.watch.close_gfdi();
    sim.watch.mute = true;
    sim.deliver(close);
    let writes_before = sim.phone_writes.len();
    sim.input(Input::StartSync);
    assert_eq!(sim.phone_writes.len(), writes_before, "held while closed");
    // The watch answers the re-registration.
    sim.watch.mute = false;
    let register = multilink::register_request(CLIENT_ID, 1, 2);
    for reply in sim.watch.on_write(sim.t, &register) {
        sim.deliver(reply);
    }
    assert_eq!(sim.phone_requests(5043).len(), 1, "sent after the reopen");
    store_everything(&mut sim);
    assert_eq!(sim.watch.archived_indices, vec![FILE_A, FILE_B]);
    assert!(sim.closed_reason().is_none());
}

#[test]
fn silence_for_30_seconds_closes_in_every_phase() {
    // Bring-up: the watch never answers close-all.
    let mut sim = Sim::new();
    sim.watch.mute = true;
    sim.connect();
    assert_eq!(sim.client.phase(), Phase::BringUp);
    sim.advance(29_999);
    assert!(sim.closed_reason().is_none());
    sim.advance(1);
    assert_eq!(
        sim.closed_reason(),
        Some(CloseReason::Timeout {
            phase: Phase::BringUp
        })
    );

    // Handshake: link is up but the watch never sends 5024.
    let mut sim = Sim::new();
    sim.connect();
    assert_eq!(sim.client.phase(), Phase::Handshake);
    sim.advance(30_000);
    assert_eq!(
        sim.closed_reason(),
        Some(CloseReason::Timeout {
            phase: Phase::Handshake
        })
    );

    // Handshake, later step: 5024 arrives, 5050 never does.
    let mut sim = Sim::new();
    sim.watch.auto_config_after_info_ack = false;
    sim.connect();
    let info = sim.watch.device_information();
    sim.watch_sends(&info);
    sim.advance(30_000);
    assert_eq!(
        sim.closed_reason(),
        Some(CloseReason::Timeout {
            phase: Phase::Handshake
        })
    );

    // Syncing: the watch triggers a cycle and then stops answering.
    let mut sim = two_file_sim();
    sim.handshake();
    sim.watch.mute = true;
    sim.watch_sends(&sim.watch.sync_request(2));
    // (the muted watch also ignores our acks, which is fine)
    sim.advance(30_000);
    assert_eq!(
        sim.closed_reason(),
        Some(CloseReason::Timeout {
            phase: Phase::Syncing
        })
    );
    assert_eq!(sim.client.phase(), Phase::Closed);

    // Ready with nothing awaited stays open.
    let mut sim = Sim::new();
    sim.handshake();
    sim.advance(600_000);
    assert!(sim.closed_reason().is_none());
}

#[test]
fn nak_to_the_filter_closes_with_the_status() {
    let mut sim = Sim::new();
    sim.watch.nak_filter = true;
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    assert_eq!(
        sim.closed_reason(),
        Some(CloseReason::RequestFailed {
            message_id: 5007,
            status: 1
        })
    );
}

#[test]
fn old_protocol_mtu_and_bad_transport_are_handled() {
    let mut sim = Sim::new();
    sim.watch.device.protocol = 149;
    sim.connect();
    let info = sim.watch.device_information();
    sim.watch_sends(&info);
    assert_eq!(
        sim.closed_reason(),
        Some(CloseReason::ProtocolTooOld { version: 149 })
    );

    let mut sim = Sim::new();
    let step = sim.input(Input::Connected { att_mtu: 22 });
    assert!(step.writes.is_empty());
    assert_eq!(sim.closed_reason(), Some(CloseReason::MtuTooSmall));

    let mut sim = Sim::new();
    sim.handshake();
    for bad in [vec![], vec![0x55, 1], vec![0, 4, 0, 0], vec![0xFE, 0, 0, 0]] {
        let step = sim.deliver(bad);
        assert!(step.writes.is_empty());
        assert!(step.events.iter().all(|e| matches!(
            e,
            Event::Anomaly {
                kind: AnomalyKind::BadTransport,
                ..
            }
        )));
        assert!(!step.events.is_empty());
    }
    assert!(sim.closed_reason().is_none());
}

#[test]
fn early_tick_is_a_no_op_and_late_tick_is_fine() {
    let mut sim = Sim::new();
    sim.handshake();
    sim.drop_phone_writes = usize::MAX;
    sim.watch_sends(&sim.watch.sync_request(2));
    let wake = sim.wake.unwrap();
    sim.drop_phone_writes = 0;
    let step = sim.client.handle(test_now(wake - 1), Input::Tick);
    assert!(step.writes.is_empty() && step.events.is_empty());
    assert_eq!(step.wake_at_mono_ms, Some(wake));
    let step = sim.client.handle(test_now(wake + 5_000), Input::Tick);
    assert!(!step.writes.is_empty(), "late tick still retransmits");
}

#[test]
fn bad_frames_and_stray_responses_are_anomalies_not_closes() {
    let mut sim = Sim::new();
    sim.handshake();
    // A frame with a bad CRC.
    let mut plain = Message::new(5036, vec![1]).encode();
    let last = plain.len() - 1;
    plain[last] ^= 1;
    let packets = sim
        .watch
        .send_raw_frame(sim.t, &nimrag_core::cobs::encode_frame(&plain));
    for p in packets {
        sim.deliver(p);
    }
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::BadFrame,
        message_id: 0
    }));
    // A response nobody asked for.
    sim.watch_sends(&Message::new(5000, vec![0x9A, 0x13, 0]));
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::UnexpectedResponse,
        message_id: 5018
    }));
    // A chunked protobuf request is acknowledged and flagged.
    let mut chunked = m::build_protobuf(5, &[1, 2, 3]);
    chunked[6..10].copy_from_slice(&100u32.to_le_bytes()); // total > chunk
    sim.watch_sends(&Message::new(5043, chunked));
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::ChunkedProtobuf,
        message_id: 5043
    }));
    assert_eq!(sim.phone_responses(5043).len(), 1);
    assert!(sim.closed_reason().is_none());
}

#[test]
fn watch_protobuf_requests_are_acked_without_an_answer() {
    let mut sim = Sim::new();
    sim.handshake();
    sim.watch_sends(&Message::new(
        5043,
        m::build_protobuf(0x22E4, &[0x52, 0x02, 0x3A, 0x00]),
    ));
    let acks = sim.phone_responses(5043);
    assert_eq!(acks.len(), 1);
    assert_eq!(
        acks[0].as_response().unwrap().body,
        [0xE4, 0x22, 0, 0, 0, 0, 0, 0]
    );
    assert!(sim.phone_requests(5044).is_empty());
}

// ---- 6.12 robustness -------------------------------------------------------------------------------------------------------------------

fn fuzz_values(seed: u32, n: usize) -> Vec<Vec<u8>> {
    let heads = [0u8, 0x80, 0x83, 0x2E, 0x30, 0x7F, 0xFF];
    let mut x = seed;
    let mut next = || {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (x >> 16) as u8
    };
    (0..n)
        .map(|_| {
            let len = (next() % 41) as usize;
            let mut v: Vec<u8> = (0..len).map(|_| next()).collect();
            if !v.is_empty() && next() % 4 != 0 {
                v[0] = heads[next() as usize % heads.len()];
            }
            // Often make it a plausible MLR packet for the registered handle.
            if v.len() >= 2 && next() % 3 == 0 {
                v[0] = 0xB0 | (v[0] & 0x03);
            }
            v
        })
        .collect()
}

fn hammer(sim: &mut Sim, seed: u32) {
    for (i, v) in fuzz_values(seed, 10_000).into_iter().enumerate() {
        sim.t += 1;
        let _ = sim.deliver(v);
        if sim.client.phase() == Phase::Closed {
            // Restart so every phase gets its full share of values.
            let _ = i;
            return;
        }
    }
}

#[test]
fn random_notifications_never_panic_in_any_phase() {
    // Bring-up.
    let mut sim = Sim::new();
    sim.watch.mute = true;
    sim.connect();
    hammer(&mut sim, 1);

    // Link up, handshake pending.
    let mut sim = Sim::new();
    sim.connect();
    hammer(&mut sim, 2);

    // Ready.
    let mut sim = Sim::new();
    sim.handshake();
    hammer(&mut sim, 3);

    // Syncing (file waiting for the caller).
    let mut sim = two_file_sim();
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    hammer(&mut sim, 4);

    // After close every input is an empty step.
    let mut sim = Sim::new();
    sim.handshake();
    sim.input(Input::Disconnected);
    for v in fuzz_values(5, 100) {
        let s = sim
            .client
            .handle(test_now(1), Input::Notification { value: v });
        assert!(s.writes.is_empty() && s.events.is_empty() && s.wake_at_mono_ms.is_none());
    }
}

/// Valid framing, random ids and payloads: the parsers see plausible bytes.
fn hammer_messages(make: fn() -> Sim, seed: u32) {
    const IDS: [u16; 14] = [
        5000, 5002, 5004, 5007, 5008, 5024, 5030, 5031, 5037, 5043, 5044, 5050, 5052, 5036,
    ];
    let mut x = seed;
    let mut next = move || {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (x >> 16) as usize
    };
    let mut sim = make();
    let mut closes = 0;
    for _ in 0..10_000 {
        let id = if next() % 8 == 0 {
            5000 + (next() % 200) as u16
        } else {
            IDS[next() % IDS.len()]
        };
        let len = next() % 30;
        let payload: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        let txn = (next() % 2 == 0).then(|| (next() % 32) as u8);
        if id == 5101 {
            continue;
        }
        sim.t += 1;
        sim.watch_sends(&Message { id, txn, payload });
        if sim.client.phase() == Phase::Closed {
            closes += 1;
            sim = make();
        }
    }
    assert!(
        closes < 5_000,
        "the fuzz should mostly keep the session alive"
    );
}

#[test]
fn random_gfdi_messages_never_panic_in_any_phase() {
    hammer_messages(
        || {
            let mut sim = Sim::new();
            sim.connect();
            sim
        },
        11,
    );
    hammer_messages(
        || {
            let mut sim = Sim::new();
            sim.handshake();
            sim
        },
        12,
    );
    hammer_messages(
        || {
            let mut sim = two_file_sim();
            sim.handshake();
            sim.watch_sends(&sim.watch.sync_request(2));
            sim
        },
        13,
    );
}

#[test]
fn watch_directory_entries_are_reported_without_the_placeholder() {
    let mut sim = Sim::new();
    sim.watch
        .add_file(fake_entry(FILE_A, 32, 10), lcg_bytes(7, 10));
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    let entries: Vec<DirEntry> = sim
        .events
        .iter()
        .find_map(|e| match e {
            Event::Directory { entries, .. } => Some(entries.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(entries, vec![fake_entry(FILE_A, 32, 10)]);
}

// ---- first connection (port-list row 16, inferred from Gadgetbridge) ---------------------------------------

fn first_connect_sim() -> Sim {
    let mut config = test_config();
    config.first_connect = true;
    let mut sim = Sim::with(nimrag_core::client::Client::new(config), FakeWatch::new());
    sim.watch
        .add_file(fake_entry(FILE_A, 32, 2000), lcg_bytes(1, 2000));
    sim
}

fn system_events(sim: &Sim) -> Vec<u8> {
    sim.phone_requests(5030)
        .iter()
        .map(|msg| msg.payload[0])
        .collect()
}

/// Position in `phone_ids` of the first 5030 that carries `event`.
fn event_position(sim: &Sim, event: u8) -> usize {
    sim.watch
        .phone_msgs
        .iter()
        .position(|msg| msg.id == 5030 && msg.payload.first() == Some(&event))
        .expect("event sent")
}

fn pairing_events(sim: &Sim) -> Vec<&Event> {
    sim.events_of(|e| matches!(e, Event::PairingCompleted { .. }))
}

#[test]
fn first_connect_announces_pairing_after_the_handshake_then_syncs() {
    let mut sim = first_connect_sim();
    sim.handshake();
    assert_eq!(system_events(&sim), vec![8, 4, 0, 14]);
    for msg in sim.phone_requests(5030) {
        assert_eq!(msg.payload, vec![msg.payload[0], 0], "value byte is 0");
    }
    let done = sim
        .events
        .iter()
        .position(|e| matches!(e, Event::HandshakeDone { .. }))
        .expect("handshake done");
    let paired: Vec<usize> = (0..sim.events.len())
        .filter(|&i| matches!(sim.events[i], Event::PairingCompleted { .. }))
        .collect();
    assert_eq!(paired.len(), 1, "announced once");
    assert!(paired[0] > done, "after HandshakeDone");
    assert_eq!(
        sim.events[paired[0]],
        Event::PairingCompleted { all_acked: true }
    );

    sim.watch_sends(&sim.watch.sync_request(2));
    store_everything(&mut sim);
    assert_eq!(sim.watch.archived_indices, vec![FILE_A]);
    assert_eq!(system_events(&sim), vec![8, 4, 0, 14, 0]);
}

#[test]
fn first_connect_events_go_before_a_sync_the_caller_asked_for_early() {
    let mut sim = first_connect_sim();
    sim.connect();
    sim.input(Input::StartSync);
    let info = sim.watch.device_information();
    sim.watch_sends(&info);
    assert_eq!(sim.client.phase(), Phase::Syncing, "the queued sync ran");
    let ids = sim.phone_ids();
    let first_sync = ids.iter().position(|&id| id == 5043).expect("sync asked");
    assert!(event_position(&sim, 14) < first_sync, "{ids:?}");
    assert_eq!(system_events(&sim)[..4], [8, 4, 0, 14]);
}

#[test]
fn a_5037_while_the_events_are_outstanding_is_acked_at_once_and_runs_after_them() {
    let mut sim = first_connect_sim();
    sim.watch.sync_after_system_event = Some(4);
    sim.connect();
    let info = sim.watch.device_information();
    sim.watch_sends(&info);
    let msgs = &sim.watch.phone_msgs;
    let sync_ack = msgs
        .iter()
        .position(|msg| msg.as_response().is_some_and(|r| r.request_id == 5037))
        .expect("5037 acknowledged");
    assert!(
        sync_ack < event_position(&sim, 14),
        "ACKed on arrival, not after the events"
    );
    let filter = msgs.iter().position(|msg| msg.id == 5007).expect("listing");
    assert!(
        filter > event_position(&sim, 14),
        "listing after the last event"
    );
    assert_eq!(pairing_events(&sim).len(), 1);
    store_everything(&mut sim);
    assert_eq!(sim.watch.archived_indices, vec![FILE_A]);
}

#[test]
fn a_normal_connection_sends_no_pairing_events() {
    let mut sim = two_file_sim();
    sim.handshake();
    assert_eq!(system_events(&sim), vec![8]);
    assert!(pairing_events(&sim).is_empty());
}

#[test]
fn a_refused_pairing_event_is_reported_and_the_session_goes_on() {
    for refused in [4u8, 14] {
        let mut sim = first_connect_sim();
        sim.watch.nak_system_event = Some(refused);
        sim.handshake();
        assert_eq!(system_events(&sim), vec![8, 4, 0, 14], "refused {refused}");
        assert_eq!(
            pairing_events(&sim),
            vec![&Event::PairingCompleted { all_acked: false }]
        );
        sim.watch_sends(&sim.watch.sync_request(2));
        store_everything(&mut sim);
        assert_eq!(sim.watch.archived_indices, vec![FILE_A]);
        assert!(sim.closed_reason().is_none());
    }
}

// ---- 5009 FileAvailable ------------------------------------------------------------------------------

fn file_available_sim() -> Sim {
    let mut sim = Sim::new();
    sim.watch
        .add_file(fake_entry(FILE_A, 32, 2000), lcg_bytes(1, 2000));
    sim
}

/// Ids of the phone's own requests, without the 5000 answers.
fn request_ids(sim: &Sim) -> Vec<u16> {
    sim.phone_ids()
        .into_iter()
        .filter(|&id| id != 5000)
        .collect()
}

fn sync_started(sim: &Sim) -> Vec<&Event> {
    sim.events_of(|e| matches!(e, Event::SyncStarted { .. }))
}

fn downloaded_indices(sim: &Sim) -> Vec<u16> {
    sim.downloaded().iter().map(|(_, e, _)| e.index).collect()
}

fn directory_events(sim: &Sim) -> Vec<&Event> {
    sim.events_of(|e| matches!(e, Event::Directory { .. }))
}

fn finished(cycle: u32, downloaded: u32, archived: u32, failed: u32) -> Event {
    Event::SyncFinished {
        cycle,
        downloaded,
        archived,
        failed,
        skipped: 0,
    }
}

#[test]
fn a_5009_while_idle_is_acked_and_fetched_without_a_listing() {
    let mut sim = file_available_sim();
    sim.handshake();
    sim.watch_sends(&sim.watch.file_available(FILE_A));
    let acks = sim.phone_responses(5009);
    assert_eq!(acks.len(), 1);
    let ack = acks[0].as_response().unwrap();
    assert_eq!(ack.status, nimrag_core::gfdi::Status::Ack);
    assert!(ack.body.is_empty());
    assert_eq!(
        sync_started(&sim),
        vec![&Event::SyncStarted {
            cycle: 1,
            trigger: SyncTrigger::FileAvailable
        }]
    );
    assert_eq!(downloaded_indices(&sim), vec![FILE_A]);
    assert!(
        sim.phone_requests(5008).is_empty(),
        "no archive before the file is stored"
    );
    assert_eq!(sim.client.phase(), Phase::Syncing);
    store_everything(&mut sim);
    assert_eq!(sim.watch.archived_indices, vec![FILE_A]);
    assert_eq!(
        request_ids(&sim),
        vec![5050, 5031, 5030, 5002, 5008, 5030],
        "no 5007, and 5002 only for the file, not for the directory"
    );
    assert_eq!(
        sim.phone_requests(5002)[0].payload,
        dl_request(FILE_A),
        "the announced file's index"
    );
    assert_eq!(sim.phone_requests(5030).last().unwrap().payload, vec![0, 0]);
    assert!(directory_events(&sim).is_empty());
    assert!(sim.events.contains(&finished(1, 1, 1, 0)));
    assert_eq!(sim.client.phase(), Phase::Ready);
    assert!(sim.closed_reason().is_none());
}

#[test]
fn a_5009_with_trailing_bytes_is_accepted() {
    let mut sim = file_available_sim();
    sim.handshake();
    let mut payload = fake_entry(FILE_A, 32, 2000).encode();
    payload.extend_from_slice(&[1, 0xFF]);
    sim.watch_sends(&Message::new(5009, payload));
    assert_eq!(downloaded_indices(&sim), vec![FILE_A]);
}

#[test]
fn a_5009_for_a_non_fit_unreadable_or_index_zero_entry_is_acked_and_ignored() {
    let mut sim = file_available_sim();
    sim.handshake();
    let good = fake_entry(FILE_A, 32, 2000);
    let not_fit = DirEntry {
        data_type: 4,
        ..good.clone()
    };
    let unreadable = DirEntry {
        flags: 0x20,
        ..good.clone()
    };
    let index_zero = DirEntry { index: 0, ..good };
    for entry in [not_fit, unreadable, index_zero] {
        sim.watch_sends(&Message::new(5009, entry.encode()));
    }
    assert_eq!(sim.phone_responses(5009).len(), 3, "all acknowledged");
    assert!(sync_started(&sim).is_empty());
    assert_eq!(request_ids(&sim), vec![5050, 5031, 5030]);
    assert_eq!(sim.client.phase(), Phase::Ready);
    // Nothing was kept either: a later cycle fetches only what it lists.
    sim.watch_sends(&sim.watch.sync_request(1));
    assert_eq!(downloaded_indices(&sim), vec![FILE_A]);
    assert_eq!(sync_started(&sim).len(), 1);
}

#[test]
fn a_malformed_5009_is_a_bad_frame_without_a_reply() {
    let mut sim = file_available_sim();
    sim.handshake();
    let before = sim.phone_writes.len();
    sim.watch_sends(&Message::new(5009, vec![0; 15]));
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::BadFrame,
        message_id: 5009
    }));
    assert!(sim.phone_responses(5009).is_empty());
    assert_eq!(sim.phone_writes.len(), before, "nothing written at all");
    assert!(sync_started(&sim).is_empty());
}

#[test]
fn a_5009_for_a_file_not_in_the_listing_is_fetched_in_the_same_cycle() {
    let mut sim = two_file_sim();
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    assert_eq!(
        downloaded_indices(&sim),
        vec![FILE_A],
        "waiting for the caller"
    );
    // A new file appears after the listing was taken.
    sim.watch
        .add_file(fake_entry(73, 32, 500), lcg_bytes(3, 500));
    sim.watch_sends(&sim.watch.file_available(73));
    assert_eq!(sim.phone_responses(5009).len(), 1, "acked at once");
    store_everything(&mut sim);
    assert_eq!(downloaded_indices(&sim), vec![FILE_A, FILE_B, 73]);
    assert_eq!(sim.watch.archived_indices, vec![FILE_A, FILE_B, 73]);
    assert_eq!(sync_started(&sim).len(), 1, "no extra cycle");
    assert_eq!(sim.phone_requests(5007).len(), 1);
    assert_eq!(directory_events(&sim).len(), 1);
    assert!(sim.events.contains(&finished(1, 3, 3, 0)));
    assert_eq!(sim.phone_requests(5030).last().unwrap().payload, vec![0, 0]);
    assert_eq!(sim.client.phase(), Phase::Ready);
}

#[test]
fn a_5009_for_a_file_already_in_the_listing_is_fetched_once() {
    let mut sim = two_file_sim();
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    // B is in the listing; the announcement carries other flags and is sent
    // twice.
    let mut entry = fake_entry(FILE_B, 4, 300);
    entry.flags = 0x81;
    sim.watch_sends(&Message::new(5009, entry.encode()));
    sim.watch_sends(&Message::new(5009, entry.encode()));
    // So is A, which is being waited on right now.
    sim.watch_sends(&sim.watch.file_available(FILE_A));
    store_everything(&mut sim);
    assert_eq!(downloaded_indices(&sim), vec![FILE_A, FILE_B]);
    let requested: Vec<Vec<u8>> = sim
        .phone_requests(5002)
        .iter()
        .map(|msg| msg.payload.clone())
        .collect();
    assert_eq!(
        requested,
        vec![dl_request(0), dl_request(FILE_A), dl_request(FILE_B)]
    );
    assert_eq!(sync_started(&sim).len(), 1, "and no second cycle");
    assert!(sim.events.contains(&finished(1, 2, 2, 0)));
    assert_eq!(sim.phone_responses(5009).len(), 3);
}

#[test]
fn a_5009_during_a_file_announced_cycle_joins_that_cycle() {
    let mut sim = file_available_sim();
    sim.watch
        .add_file(fake_entry(FILE_B, 4, 300), lcg_bytes(2, 300));
    sim.handshake();
    sim.watch_sends(&sim.watch.file_available(FILE_A));
    sim.watch_sends(&sim.watch.file_available(FILE_B));
    store_everything(&mut sim);
    assert_eq!(downloaded_indices(&sim), vec![FILE_A, FILE_B]);
    assert_eq!(sync_started(&sim).len(), 1);
    assert!(directory_events(&sim).is_empty());
    assert!(sim.phone_requests(5007).is_empty());
    assert!(sim.events.contains(&finished(1, 2, 2, 0)));
}

#[test]
fn a_5009_while_the_sync_complete_is_outstanding_starts_one_more_cycle() {
    let mut sim = file_available_sim();
    sim.watch.silent_system_event = Some(0);
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    store_everything(&mut sim);
    // SYNC_COMPLETE is out and unanswered: the 5009 is acked and kept.
    sim.watch
        .add_file(fake_entry(73, 32, 500), lcg_bytes(3, 500));
    sim.watch_sends(&sim.watch.file_available(73));
    assert_eq!(sim.phone_responses(5009).len(), 1);
    assert_eq!(sync_started(&sim).len(), 1, "busy: only noted");
    assert_eq!(sim.downloaded().len(), 1);
    let complete = (*sim.phone_requests(5030).last().unwrap()).clone();
    sim.watch.silent_system_event = None;
    sim.watch_sends(&watch_response(&complete, 0, &[]));
    assert!(sim.events.contains(&finished(1, 1, 1, 0)));
    assert_eq!(
        sync_started(&sim).last(),
        Some(&&Event::SyncStarted {
            cycle: 2,
            trigger: SyncTrigger::FileAvailable
        })
    );
    store_everything(&mut sim);
    assert_eq!(downloaded_indices(&sim), vec![FILE_A, 73]);
    assert!(sim.events.contains(&finished(2, 1, 1, 0)));
    assert_eq!(sim.client.phase(), Phase::Ready);
}

#[test]
fn a_5009_during_wait_grant_is_fetched_when_the_grant_times_out() {
    let mut sim = file_available_sim();
    sim.watch.grant_sync_on_protobuf = false;
    sim.handshake();
    sim.input(Input::StartSync);
    sim.watch_sends(&sim.watch.file_available(FILE_A));
    assert!(sync_started(&sim).is_empty(), "waiting for the grant");
    assert!(sim.phone_requests(5002).is_empty());
    sim.advance(30_000);
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::SyncNotGranted,
        message_id: 5037
    }));
    assert_eq!(
        sync_started(&sim),
        vec![&Event::SyncStarted {
            cycle: 1,
            trigger: SyncTrigger::FileAvailable
        }]
    );
    assert_eq!(downloaded_indices(&sim), vec![FILE_A]);
}

#[test]
fn a_5009_before_the_handshake_starts_a_cycle_after_it() {
    let mut sim = file_available_sim();
    sim.connect();
    sim.watch_sends(&sim.watch.file_available(FILE_A));
    assert_eq!(sim.phone_responses(5009).len(), 1, "acked at once");
    assert!(sync_started(&sim).is_empty());
    let info = sim.watch.device_information();
    sim.watch_sends(&info);
    let done = sim
        .events
        .iter()
        .position(|e| matches!(e, Event::HandshakeDone { .. }))
        .expect("handshake done");
    let started = sim
        .events
        .iter()
        .position(|e| matches!(e, Event::SyncStarted { .. }))
        .expect("cycle started");
    assert!(started > done);
    assert_eq!(
        sim.events[started],
        Event::SyncStarted {
            cycle: 1,
            trigger: SyncTrigger::FileAvailable
        }
    );
    assert!(sim.phone_requests(5007).is_empty());
    assert_eq!(downloaded_indices(&sim), vec![FILE_A]);
    store_everything(&mut sim);
    assert_eq!(sim.watch.archived_indices, vec![FILE_A]);
}

#[test]
fn a_queued_sync_goes_before_a_5009_announced_before_the_handshake() {
    for watch_asks in [false, true] {
        let mut sim = file_available_sim();
        sim.connect();
        if watch_asks {
            sim.watch_sends(&sim.watch.sync_request(2));
        } else {
            sim.input(Input::StartSync);
        }
        sim.watch_sends(&sim.watch.file_available(FILE_A));
        let info = sim.watch.device_information();
        sim.watch_sends(&info);
        store_everything(&mut sim);
        let trigger = if watch_asks {
            SyncTrigger::Watch { option: 2 }
        } else {
            SyncTrigger::Caller
        };
        assert_eq!(
            sync_started(&sim),
            vec![&Event::SyncStarted { cycle: 1, trigger }],
            "watch_asks {watch_asks}"
        );
        assert_eq!(sim.phone_requests(5007).len(), 1, "a listing was taken");
        assert_eq!(downloaded_indices(&sim), vec![FILE_A], "fetched once");
        assert_eq!(sim.watch.archived_indices, vec![FILE_A]);
        assert!(sim.events.contains(&finished(1, 1, 1, 0)));
        assert_eq!(sim.client.phase(), Phase::Ready, "no further cycle");
    }
}

#[test]
fn announced_files_are_capped_at_32_and_the_rest_wait_for_a_listing() {
    let mut sim = file_available_sim();
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    for index in 100..140u16 {
        sim.watch
            .add_file(fake_entry(index, 32, 20), lcg_bytes(index as u32, 20));
        sim.watch_sends(&sim.watch.file_available(index));
    }
    store_everything(&mut sim);
    let got = downloaded_indices(&sim);
    assert_eq!(
        got.len(),
        1 + 32,
        "listing file plus the first 32 announced"
    );
    assert_eq!(got[0], FILE_A);
    assert_eq!(got[1..], (100..132).collect::<Vec<u16>>()[..]);
    assert_eq!(sync_started(&sim).len(), 1);
    // The next full listing picks up the 8 that were dropped.
    sim.watch_sends(&sim.watch.sync_request(1));
    store_everything(&mut sim);
    assert_eq!(downloaded_indices(&sim).len(), 1 + 32 + 8);
    assert_eq!(sim.watch.pending_files().len(), 0);
}

// ---- 5023 BatteryStatus ----------------------------------------------------------------------------------

fn battery_events(sim: &Sim) -> Vec<&Event> {
    sim.events_of(|e| matches!(e, Event::Battery { .. }))
}

#[test]
fn a_5023_is_acked_and_reported_as_a_status_battery() {
    let mut sim = Sim::new();
    sim.handshake();
    sim.watch_sends(&battery_status_message(0x2A, 38, 87));
    let acks = sim.phone_responses(5023);
    assert_eq!(acks.len(), 1);
    let ack = acks[0].as_response().unwrap();
    assert_eq!(ack.status, nimrag_core::gfdi::Status::Ack);
    assert!(ack.body.is_empty());
    assert_eq!(
        battery_events(&sim),
        vec![&Event::Battery {
            level_percent: 87,
            report: BatteryReport::Status {
                status: 2,
                centivolts: 38
            }
        }]
    );
    assert!(sim
        .events_of(|e| matches!(e, Event::Unsupported { .. }))
        .is_empty());
    // The bare minimum, 3 bytes, is enough.
    sim.watch_sends(&Message::new(5023, vec![0x51, 41, 100]));
    assert_eq!(sim.phone_responses(5023).len(), 2);
    assert_eq!(
        battery_events(&sim).last(),
        Some(&&Event::Battery {
            level_percent: 100,
            report: BatteryReport::Status {
                status: 5,
                centivolts: 41
            }
        })
    );
}

#[test]
fn a_5023_with_status_invalid_is_acked_without_an_event() {
    let mut sim = Sim::new();
    sim.handshake();
    sim.watch_sends(&Message::new(5023, vec![0x71, 41, 100]));
    assert_eq!(sim.phone_responses(5023).len(), 1);
    assert!(battery_events(&sim).is_empty());
    assert!(sim
        .events_of(|e| matches!(e, Event::Anomaly { .. }))
        .is_empty());
}

#[test]
fn a_5023_percent_above_100_is_acked_without_an_event() {
    let mut sim = Sim::new();
    sim.handshake();
    sim.watch_sends(&battery_status_message(0x10, 40, 200));
    assert_eq!(sim.phone_responses(5023).len(), 1);
    assert!(battery_events(&sim).is_empty());
    assert!(sim
        .events_of(|e| matches!(e, Event::Anomaly { .. }))
        .is_empty());
}

#[test]
fn a_short_5023_is_a_bad_frame_without_a_reply() {
    let mut sim = Sim::new();
    sim.handshake();
    sim.watch_sends(&Message::new(5023, vec![0x10, 40]));
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::BadFrame,
        message_id: 5023
    }));
    assert!(sim.phone_responses(5023).is_empty());
    assert!(battery_events(&sim).is_empty());
}

// ---- battery request (device_status protobuf) ----------------------------------------------------

fn battery_requests(sim: &Sim) -> Vec<&Message> {
    sim.phone_requests(5043)
        .into_iter()
        .filter(|msg| {
            m::parse_protobuf(&msg.payload).is_some_and(|(_, b)| b == m::BATTERY_REQUEST_PROTOBUF)
        })
        .collect()
}

#[test]
fn request_battery_sends_the_protobuf_and_reports_the_answer() {
    let mut sim = Sim::new();
    sim.handshake();
    sim.input(Input::RequestBattery);
    let sent = battery_requests(&sim);
    assert_eq!(sent.len(), 1);
    let (h, body) = m::parse_protobuf(&sent[0].payload).unwrap();
    assert_eq!((h.request_id, h.offset, h.total, h.chunk), (0, 0, 4, 4));
    assert_eq!(body, [0x42, 0x02, 0x12, 0x00]);
    assert!(sent[0].txn.is_some());
    // The watch's 5044 is acknowledged with the 8-byte protobuf ACK.
    let acks = sim.phone_responses(5044);
    assert_eq!(acks.len(), 1);
    assert_eq!(
        acks[0].as_response().unwrap().body,
        m::build_protobuf_ack(0)
    );
    assert_eq!(
        battery_events(&sim),
        vec![&Event::Battery {
            level_percent: 87,
            report: BatteryReport::DeviceStatus {
                battery_status: Some(1),
                hours_remaining: Some(120)
            }
        }]
    );
    // It is a plain request: no sync was asked for or started.
    assert!(sync_started(&sim).is_empty());
    assert!(sim.phone_requests(5007).is_empty());
    assert_eq!(sim.client.phase(), Phase::Ready);
    // A second ask after the first was answered sends a second request.
    sim.input(Input::RequestBattery);
    assert_eq!(battery_requests(&sim).len(), 2);
    assert_eq!(battery_events(&sim).len(), 2);
}

#[test]
fn a_battery_answer_without_optional_fields_or_a_level_above_100() {
    let mut sim = Sim::new();
    sim.handshake();
    sim.watch.battery_response = Some(battery_smart(5, None, None));
    sim.input(Input::RequestBattery);
    sim.watch.battery_response = Some(battery_smart(101, Some(1), None));
    sim.input(Input::RequestBattery);
    assert_eq!(
        battery_events(&sim),
        vec![&Event::Battery {
            level_percent: 5,
            report: BatteryReport::DeviceStatus {
                battery_status: None,
                hours_remaining: None
            }
        }]
    );
    assert_eq!(sim.phone_responses(5044).len(), 2, "both acknowledged");
}

#[test]
fn a_battery_answer_arriving_as_a_watch_5043_is_read_too() {
    let mut sim = Sim::new();
    sim.handshake();
    sim.watch_sends(&Message::new(
        5043,
        m::build_protobuf(0x0102, &battery_smart(33, Some(2), Some(7))),
    ));
    assert_eq!(
        sim.phone_responses(5043)[0].as_response().unwrap().body,
        m::build_protobuf_ack(0x0102)
    );
    assert_eq!(
        battery_events(&sim),
        vec![&Event::Battery {
            level_percent: 33,
            report: BatteryReport::DeviceStatus {
                battery_status: Some(2),
                hours_remaining: Some(7)
            }
        }]
    );
    // A chunked message is acknowledged as before but not decoded.
    let body = battery_smart(44, None, None);
    let mut payload = m::build_protobuf(9, &body);
    payload[10..14].copy_from_slice(&(body.len() as u32 + 5).to_le_bytes()); // total > chunk
    sim.watch_sends(&Message::new(5044, payload));
    assert_eq!(battery_events(&sim).len(), 1);
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::ChunkedProtobuf,
        message_id: 5044
    }));
    // Garbage and other services are ignored without an event.
    sim.watch_sends(&Message::new(5044, m::build_protobuf(10, &[0xFF, 0xFF])));
    sim.watch_sends(&Message::new(
        5044,
        m::build_protobuf(11, &[0x6A, 0x04, 0x12, 0x02, 0x08, 0x01]),
    ));
    assert_eq!(battery_events(&sim).len(), 1);
}

#[test]
fn request_battery_before_the_handshake_goes_out_after_it_and_before_the_sync() {
    let mut sim = Sim::new();
    sim.connect();
    sim.input(Input::RequestBattery);
    sim.input(Input::RequestBattery);
    sim.input(Input::StartSync);
    assert!(
        sim.phone_requests(5043).is_empty(),
        "nothing before the handshake"
    );
    let info = sim.watch.device_information();
    sim.watch_sends(&info);
    assert_eq!(battery_requests(&sim).len(), 1, "two asks, one request");
    let ids = request_ids(&sim);
    assert_eq!(&ids[..5], [5050, 5031, 5030, 5043, 5043], "{ids:?}");
    let first = sim.phone_requests(5043)[0];
    assert_eq!(
        m::parse_protobuf(&first.payload).unwrap().1,
        m::BATTERY_REQUEST_PROTOBUF,
        "the battery request comes first, then the sync request"
    );
    let done = sim
        .events
        .iter()
        .position(|e| matches!(e, Event::HandshakeDone { .. }))
        .unwrap();
    let battery = sim
        .events
        .iter()
        .position(|e| matches!(e, Event::Battery { .. }))
        .expect("the answer was reported");
    assert!(battery > done);
    assert_eq!(battery_events(&sim).len(), 1);
    assert_eq!(sync_started(&sim).len(), 1, "the queued sync still ran");
    store_everything(&mut sim);
    assert_eq!(sim.client.phase(), Phase::Ready);
}

#[test]
fn request_battery_after_first_connect_events_keeps_them_in_front() {
    let mut sim = first_connect_sim();
    sim.connect();
    sim.input(Input::RequestBattery);
    let info = sim.watch.device_information();
    sim.watch_sends(&info);
    assert_eq!(battery_requests(&sim).len(), 1);
    let ids = request_ids(&sim);
    assert_eq!(
        &ids[..7],
        [5050, 5031, 5030, 5030, 5030, 5030, 5043],
        "{ids:?}"
    );
}

#[test]
fn a_refused_battery_request_is_reported_and_the_session_goes_on() {
    let mut sim = file_available_sim();
    sim.watch.nak_battery_request = true;
    sim.handshake();
    sim.input(Input::RequestBattery);
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::BatteryRequestRefused,
        message_id: 5043
    }));
    assert!(sim.closed_reason().is_none());
    assert!(battery_events(&sim).is_empty());
    assert_eq!(sim.client.phase(), Phase::Ready);
    sim.watch_sends(&sim.watch.sync_request(2));
    store_everything(&mut sim);
    assert_eq!(sim.watch.archived_indices, vec![FILE_A]);
    assert!(sim.closed_reason().is_none());
}

#[test]
fn request_battery_while_a_download_is_waiting_for_the_caller_is_sent_and_answered() {
    let mut sim = file_available_sim();
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    assert_eq!(sim.downloaded().len(), 1);
    sim.input(Input::RequestBattery);
    sim.input(Input::RequestBattery);
    assert_eq!(
        battery_requests(&sim).len(),
        2,
        "each ask was answered before the next"
    );
    assert_eq!(battery_events(&sim).len(), 2);
    store_everything(&mut sim);
    assert_eq!(sim.watch.archived_indices, vec![FILE_A]);
}

// ---- review fixes: 5009 identity, handled files, silent battery request ----------------------------

#[test]
fn a_5009_with_timestamp_zero_matches_the_listing_entry_and_is_fetched_once() {
    let mut sim = two_file_sim();
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    let mut entry = fake_entry(FILE_B, 4, 300);
    entry.timestamp = 0;
    sim.watch_sends(&Message::new(5009, entry.encode()));
    store_everything(&mut sim);
    assert_eq!(downloaded_indices(&sim), vec![FILE_A, FILE_B]);
    let for_b = sim
        .phone_requests(5002)
        .iter()
        .filter(|msg| msg.payload == dl_request(FILE_B))
        .count();
    assert_eq!(for_b, 1);
    assert_eq!(sync_started(&sim).len(), 1);
}

#[test]
fn a_re_announced_archived_file_while_sync_complete_is_outstanding_starts_no_cycle() {
    let mut sim = file_available_sim();
    sim.watch.silent_system_event = Some(0);
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    store_everything(&mut sim);
    sim.watch_sends(&sim.watch.file_available(FILE_A));
    assert_eq!(sim.phone_responses(5009).len(), 1, "acked");
    let complete = (*sim.phone_requests(5030).last().unwrap()).clone();
    sim.watch.silent_system_event = None;
    sim.watch_sends(&watch_response(&complete, 0, &[]));
    assert!(sim.events.contains(&finished(1, 1, 1, 0)));
    assert_eq!(sync_started(&sim).len(), 1, "no second cycle");
    assert_eq!(sim.downloaded().len(), 1);
    assert_eq!(sim.client.phase(), Phase::Ready);
}

#[test]
fn a_refused_announced_file_is_not_announced_into_endless_cycles_but_a_listing_retries_it() {
    let mut sim = file_available_sim();
    sim.watch.nak_download = Some(FILE_A);
    sim.handshake();
    sim.watch_sends(&sim.watch.file_available(FILE_A));
    assert!(matches!(
        sim.events
            .iter()
            .find(|e| matches!(e, Event::FileFailed { .. })),
        Some(Event::FileFailed {
            reason: FileFailure::Refused { status: 1 },
            will_retry: false,
            ..
        })
    ));
    assert!(sim.events.contains(&finished(1, 0, 0, 1)));
    assert_eq!(sim.phone_requests(5030).last().unwrap().payload, vec![0, 0]);
    // The watch announces it again: acked, nothing more happens.
    sim.watch_sends(&sim.watch.file_available(FILE_A));
    assert_eq!(sim.phone_responses(5009).len(), 2);
    assert_eq!(sync_started(&sim).len(), 1);
    assert_eq!(sim.phone_requests(5002).len(), 1);
    // A full listing still asks for it.
    sim.watch_sends(&sim.watch.sync_request(1));
    assert_eq!(sync_started(&sim).len(), 2);
    let for_a = sim
        .phone_requests(5002)
        .iter()
        .filter(|msg| msg.payload == dl_request(FILE_A))
        .count();
    assert_eq!(for_a, 2, "requested again by the listing cycle");
    assert!(sim.events.contains(&finished(2, 0, 0, 1)));
}

#[test]
fn a_silent_battery_request_is_dropped_after_the_deadline_and_the_session_goes_on() {
    let mut sim = file_available_sim();
    sim.watch.silent_battery_request = true;
    sim.handshake();
    sim.input(Input::RequestBattery);
    assert_eq!(battery_requests(&sim).len(), 1);
    sim.advance(29_999);
    assert!(sim
        .events_of(|e| matches!(e, Event::Anomaly { .. }))
        .is_empty());
    sim.advance(1);
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::BatteryRequestRefused,
        message_id: 5043
    }));
    assert!(sim.closed_reason().is_none());
    assert_eq!(sim.client.phase(), Phase::Ready);
    sim.advance(60_000);
    assert!(sim.closed_reason().is_none(), "no later timeout either");
    // A late answer is just an unexpected response.
    let late = (*battery_requests(&sim)[0]).clone();
    sim.watch_sends(&watch_response(&late, 0, &m::build_protobuf_ack(0)));
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::UnexpectedResponse,
        message_id: 5043
    }));
    // A sync still works.
    sim.watch.silent_battery_request = false;
    sim.input(Input::StartSync);
    store_everything(&mut sim);
    assert_eq!(sim.watch.archived_indices, vec![FILE_A]);
    assert!(sim.closed_reason().is_none());
}

#[test]
fn request_battery_queued_behind_a_pending_request_adds_no_second_5043() {
    let mut sim = file_available_sim();
    sim.watch.silent_system_event = Some(0);
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    store_everything(&mut sim);
    // SYNC_COMPLETE is unanswered, so both asks queue behind it.
    sim.input(Input::RequestBattery);
    sim.input(Input::RequestBattery);
    assert!(battery_requests(&sim).is_empty(), "queued, not sent");
    let complete = (*sim.phone_requests(5030).last().unwrap()).clone();
    sim.watch.silent_system_event = None;
    sim.watch_sends(&watch_response(&complete, 0, &[]));
    assert_eq!(battery_requests(&sim).len(), 1, "one request for two asks");
    assert_eq!(battery_events(&sim).len(), 1);
    assert!(sim.events.contains(&finished(1, 1, 1, 0)));
    assert_eq!(sim.client.phase(), Phase::Ready);
}

// ---- stage 2g: alongside another sync app (archive false) and the skip list --------------------------

fn client_with(archive: bool, skip: Vec<FileKey>) -> Client {
    let mut config = test_config();
    config.archive = archive;
    config.skip = skip;
    Client::new(config)
}

fn two_file_watch() -> FakeWatch {
    let mut watch = FakeWatch::new();
    watch.add_file(fake_entry(FILE_A, 32, 2000), lcg_bytes(1, 2000));
    watch.add_file(fake_entry(FILE_B, 4, 300), lcg_bytes(2, 300));
    watch
}

/// Archive false: two files on the watch, handshake done, no sync yet.
fn alongside_sim() -> Sim {
    let mut sim = Sim::with(client_with(false, vec![]), two_file_watch());
    sim.handshake();
    sim
}

fn key(index: u16, sub_type: u8, size: usize) -> FileKey {
    FileKey::of(&fake_entry(index, sub_type, size))
}

fn skipped_indices(sim: &Sim) -> Vec<u16> {
    sim.events
        .iter()
        .filter_map(|e| match e {
            Event::FileSkipped { entry } => Some(entry.index),
            _ => None,
        })
        .collect()
}

/// With archive false no 5008 exists anywhere on the wire, the watch holds
/// every file as pending, and no file was reported archived.
fn assert_never_archived(sim: &Sim) {
    assert!(sim.phone_requests(5008).is_empty(), "no 5008 built");
    assert!(
        !sim.phone_ids().contains(&5008),
        "no 5008 on the wire in any form"
    );
    assert!(sim.watch.archived_indices.is_empty());
    assert!(!sim
        .events
        .iter()
        .any(|e| matches!(e, Event::FileArchived { .. })));
}

#[test]
fn file_key_comes_from_the_entry_without_the_index() {
    let a = fake_entry(FILE_A, 32, 2000);
    let mut moved = a.clone();
    moved.index = 5;
    moved.flags = 0x80;
    assert_eq!(FileKey::of(&a), FileKey::of(&moved));
    let mut other = a.clone();
    other.timestamp += 1;
    assert_ne!(FileKey::of(&a), FileKey::of(&other));
    assert_eq!(
        FileKey::of(&a),
        FileKey {
            data_type: 128,
            sub_type: 32,
            file_number: FILE_A + 1000,
            size: 2000,
            timestamp: 1_000_000 + FILE_A as u32,
        }
    );
}

#[test]
fn alongside_stores_and_moves_on_without_archiving() {
    let mut sim = alongside_sim();
    sim.watch_sends(&sim.watch.sync_request(2));
    assert_eq!(sim.downloaded().len(), 1, "waiting for the first file");
    store_everything(&mut sim);
    assert_eq!(downloaded_indices(&sim), vec![FILE_A, FILE_B]);
    assert_never_archived(&sim);
    assert_eq!(sim.watch.pending_files().len(), 2, "left for the other app");
    let last = sim.watch.phone_msgs.last().unwrap();
    assert_eq!((last.id, last.payload.clone()), (5030, vec![0, 0]));
    assert!(sim.events.contains(&Event::SyncFinished {
        cycle: 1,
        downloaded: 2,
        archived: 0,
        failed: 0,
        skipped: 0,
    }));
    assert_eq!(sim.client.phase(), Phase::Ready);
    assert!(sim.closed_reason().is_none());
}

#[test]
fn alongside_retry_after_a_failed_download_is_copied_without_archive() {
    let mut sim = Sim::with(client_with(false, vec![]), two_file_watch());
    sim.watch.faults.insert(FILE_A, vec![Fault::BadCrc].into());
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    assert!(chunk_ack_statuses(&sim).contains(&m::CHUNK_CRC_MISMATCH));
    assert_eq!(sim.downloaded().len(), 1);
    assert_eq!(sim.downloaded()[0].2, lcg_bytes(1, 2000));
    store_everything(&mut sim);
    assert_eq!(downloaded_indices(&sim), vec![FILE_A, FILE_B]);
    assert_never_archived(&sim);
    assert!(sim.events.contains(&Event::SyncFinished {
        cycle: 1,
        downloaded: 2,
        archived: 0,
        failed: 0,
        skipped: 0,
    }));
}

#[test]
fn alongside_stale_confirmations_write_nothing_and_archive_nothing() {
    let mut sim = alongside_sim();
    sim.watch_sends(&sim.watch.sync_request(2));
    let before = sim.phone_writes.len();
    sim.input(Input::FileStored { transfer_id: 999 });
    assert_eq!(sim.phone_writes.len(), before, "unknown id writes nothing");
    assert!(sim
        .events
        .contains(&Event::StaleConfirmation { transfer_id: 999 }));
    let id = latest_downloaded_id(&sim);
    sim.input(Input::FileStored { transfer_id: id });
    assert_eq!(sim.downloaded().len(), 2, "moved on to the next file");
    // The first id again, now stale: still nothing archived.
    let before = sim.phone_writes.len();
    sim.input(Input::FileStored { transfer_id: id });
    assert_eq!(sim.phone_writes.len(), before);
    store_everything(&mut sim);
    assert_never_archived(&sim);
    assert_archive_rule(&sim);
}

#[test]
fn alongside_a_failed_store_is_not_archived_and_not_remembered() {
    let mut sim = alongside_sim();
    sim.watch_sends(&sim.watch.sync_request(2));
    let id = latest_downloaded_id(&sim);
    sim.input(Input::FileStoreFailed { transfer_id: id });
    assert!(sim.events.iter().any(|e| matches!(
        e,
        Event::FileFailed {
            reason: FileFailure::StoreFailed,
            ..
        }
    )));
    assert_eq!(sim.downloaded().len(), 2, "next file is fetched");
    let id = latest_downloaded_id(&sim);
    sim.input(Input::FileStored { transfer_id: id });
    assert_never_archived(&sim);
    // A second cycle on this connection fetches the failed file again and
    // skips the stored one.
    sim.watch_sends(&sim.watch.sync_request(2));
    assert_eq!(downloaded_indices(&sim), vec![FILE_A, FILE_B, FILE_A]);
    store_everything(&mut sim);
    assert_eq!(skipped_indices(&sim), vec![FILE_B]);
    assert_eq!(downloaded_indices(&sim), vec![FILE_A, FILE_B, FILE_A]);
    assert_never_archived(&sim);
}

#[test]
fn skip_keys_suppress_the_download_and_are_counted() {
    let skip = vec![key(FILE_A, 32, 2000)];
    let mut sim = Sim::with(client_with(true, skip), two_file_watch());
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    store_everything(&mut sim);
    assert_eq!(downloaded_indices(&sim), vec![FILE_B]);
    assert_eq!(
        sim.phone_requests(5002).len(),
        2,
        "directory + the one file; none for the skipped one"
    );
    assert_eq!(skipped_indices(&sim), vec![FILE_A]);
    assert!(sim.events.contains(&Event::SyncFinished {
        cycle: 1,
        downloaded: 1,
        archived: 1,
        failed: 0,
        skipped: 1,
    }));
    // Keys only skip: the skipped file is never archived either.
    assert_eq!(sim.watch.archived_indices, vec![FILE_B]);
    assert_archive_rule(&sim);
}

#[test]
fn skip_matches_the_whole_key_not_just_the_file_number() {
    // Same file number and type, different size or sub type: another file.
    let skip = vec![key(FILE_A, 32, 1999), key(FILE_B, 5, 300)];
    let mut sim = Sim::with(client_with(false, skip), two_file_watch());
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    store_everything(&mut sim);
    assert_eq!(downloaded_indices(&sim), vec![FILE_A, FILE_B]);
    assert!(skipped_indices(&sim).is_empty());
    assert_never_archived(&sim);
}

#[test]
fn skipping_everything_still_completes_the_cycle() {
    let skip = vec![key(FILE_A, 32, 2000), key(FILE_B, 4, 300)];
    let mut sim = Sim::with(client_with(false, skip), two_file_watch());
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    assert!(sim.downloaded().is_empty());
    assert_eq!(skipped_indices(&sim), vec![FILE_A, FILE_B]);
    let last = sim.watch.phone_msgs.last().unwrap();
    assert_eq!((last.id, last.payload.clone()), (5030, vec![0, 0]));
    assert!(sim.events.contains(&Event::SyncFinished {
        cycle: 1,
        downloaded: 0,
        archived: 0,
        failed: 0,
        skipped: 2,
    }));
    assert_never_archived(&sim);
    assert_eq!(sim.client.phase(), Phase::Ready);
}

#[test]
fn a_5009_for_a_skip_listed_file_is_ignored_without_a_cycle() {
    let skip = vec![key(FILE_A, 32, 2000)];
    let mut sim = Sim::with(client_with(false, skip), two_file_watch());
    sim.handshake();
    sim.watch_sends(&sim.watch.file_available(FILE_A));
    assert_eq!(sim.phone_responses(5009).len(), 1, "acknowledged");
    assert!(sync_started(&sim).is_empty(), "no cycle for a file we skip");
    assert!(sim.downloaded().is_empty());
    // An announced file that is not listed is still fetched.
    sim.watch_sends(&sim.watch.file_available(FILE_B));
    store_everything(&mut sim);
    assert_eq!(downloaded_indices(&sim), vec![FILE_B]);
    assert_never_archived(&sim);
}

#[test]
fn a_file_stored_alongside_is_skipped_by_the_next_cycle_of_the_same_connection() {
    let mut sim = alongside_sim();
    sim.watch_sends(&sim.watch.sync_request(2));
    store_everything(&mut sim);
    assert_eq!(downloaded_indices(&sim), vec![FILE_A, FILE_B]);
    assert!(skipped_indices(&sim).is_empty());

    sim.watch_sends(&sim.watch.sync_request(2));
    assert_eq!(
        downloaded_indices(&sim),
        vec![FILE_A, FILE_B],
        "no new download"
    );
    assert_eq!(skipped_indices(&sim), vec![FILE_A, FILE_B]);
    assert_eq!(
        sim.phone_requests(5002).len(),
        4,
        "directory + two files, then only the directory"
    );
    assert!(sim.events.contains(&Event::SyncFinished {
        cycle: 2,
        downloaded: 0,
        archived: 0,
        failed: 0,
        skipped: 2,
    }));
    assert_never_archived(&sim);
    assert_eq!(sim.client.phase(), Phase::Ready);
}

#[test]
fn with_archive_true_a_stored_file_is_archived_and_not_remembered() {
    // Unchanged behaviour: the watch drops archived files from the listing,
    // so the second cycle is simply empty (and skips nothing).
    let mut sim = Sim::with(client_with(true, vec![]), two_file_watch());
    sim.handshake();
    sim.watch_sends(&sim.watch.sync_request(2));
    store_everything(&mut sim);
    assert_eq!(sim.watch.archived_indices, vec![FILE_A, FILE_B]);
    sim.watch_sends(&sim.watch.sync_request(2));
    assert!(skipped_indices(&sim).is_empty());
    assert!(sim.events.contains(&finished(2, 0, 0, 0)));
}
