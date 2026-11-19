//! Synthetic sessions through `Client::handle`: a fake watch built in code
//! with fake ids, no capture data. See IMPLEMENTATION_PLAN.md stage 2a, 6.

mod support;

use nimrag_core::capture::Direction;
use nimrag_core::client::reliable::WINDOW;
use nimrag_core::client::{
    AnomalyKind, CloseReason, Event, FileFailure, Input, Phase, SyncTrigger,
};
use nimrag_core::gfdi::Message;
use nimrag_core::link::{Event as LinkEvent, LinkDecoder};
use nimrag_core::messages::{self as m, DirEntry};
use nimrag_core::mlr;
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
        next_dst: garmin(UNIX_NOW + 86_400 * 30),
        following_dst: garmin(UNIX_NOW + 86_400 * 200),
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
    assert_eq!(t.next_dst, garmin(UNIX_NOW + 86_400 * 30));
    assert_eq!(t.following_dst, garmin(UNIX_NOW + 86_400 * 200));
    assert!(sim.events.contains(&Event::TimeSent {
        garmin_time: garmin(UNIX_NOW + 5),
        utc_offset_s: 3600
    }));
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

#[test]
fn gfdi_without_the_reliable_flag_is_refused() {
    let mut sim = Sim::new();
    sim.watch.grant_reliable = false;
    sim.connect();
    assert_eq!(sim.closed_reason(), Some(CloseReason::ReliableNotGranted));
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
