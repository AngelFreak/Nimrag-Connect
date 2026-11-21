//! Stage 4a: uploads to the watch (a workout is file type 128/5), driven only
//! through `Client::handle` against the scripted fake watch. Every byte here is
//! synthetic. The upload layouts (5005 CreateFile, 5003 UploadFile, 5004 chunks
//! and their replies) come from Gadgetbridge and are labelled *inferred
//! (Gadgetbridge)* in docs/protocol/PROTOCOL.md: these tests pin what the client
//! sends, and that failures end one upload and never the session. They do not
//! show that a real watch accepts it.

mod support;

use nimrag_core::client::{AnomalyKind, Client, Event, Input, Phase, UploadResult, UploadStage};
use nimrag_core::cobs;
use nimrag_core::crc;
use nimrag_core::messages as m;
use support::*;

fn sim_with_watch(watch: FakeWatch) -> Sim {
    let mut sim = Sim::with(Client::new(test_config()), watch);
    sim.handshake();
    sim
}

fn sim() -> Sim {
    sim_with_watch(FakeWatch::new())
}

fn upload(id: u64, data: Vec<u8>) -> Input {
    Input::UploadFile {
        upload_id: id,
        data_type: 128,
        sub_type: 5,
        data,
    }
}

fn finished(sim: &Sim) -> Vec<(u64, UploadResult)> {
    sim.events
        .iter()
        .filter_map(|e| match e {
            Event::UploadFinished { upload_id, result } => Some((*upload_id, *result)),
            _ => None,
        })
        .collect()
}

fn upload_anomalies(sim: &Sim) -> Vec<u16> {
    sim.events
        .iter()
        .filter_map(|e| match e {
            Event::Anomaly {
                kind: AnomalyKind::UploadFailed,
                message_id,
            } => Some(*message_id),
            _ => None,
        })
        .collect()
}

/// Payloads of the phone's messages with this id, in order.
fn sent(sim: &Sim, id: u16) -> Vec<Vec<u8>> {
    sim.phone_requests(id)
        .iter()
        .map(|msg| msg.payload.clone())
        .collect()
}

#[test]
fn a_small_file_goes_out_byte_for_byte_as_gadgetbridge_sends_it() {
    let mut sim = sim();
    let data = lcg_bytes(7, 10);
    sim.input(upload(9, data.clone()));

    // 5005 CreateFile: size, type 128, sub type 5, index 0, 0, 0, number mask
    // 0xFFFF, path length 0, then a 64-bit file id.
    let creates = sent(&sim, 5005);
    assert_eq!(creates.len(), 1);
    let mut want = vec![10, 0, 0, 0, 128, 5, 0, 0, 0, 0, 0xFF, 0xFF, 0, 0];
    assert_eq!(&creates[0][..14], &want[..]);
    assert_eq!(creates[0].len(), 22);
    want.clear();

    // 5003 UploadFile: the index the watch handed out, size, offset 0, seed 0.
    assert_eq!(
        sent(&sim, 5003),
        vec![vec![0x07, 0x01, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0]]
    );

    // 5004: flags 0, running CRC, offset 0, the data.
    let crc = crc::checksum(&data).to_le_bytes();
    let mut chunk = vec![0, crc[0], crc[1], 0, 0, 0, 0];
    chunk.extend_from_slice(&data);
    assert_eq!(sent(&sim, 5004), vec![chunk]);

    // The closing SYNC_COMPLETE (event 0, value 0) follows the last ACK.
    let ids = sim.phone_ids();
    let first = ids.iter().position(|&i| i == 5005).unwrap();
    let after: Vec<u16> = ids[first..]
        .iter()
        .copied()
        .filter(|i| [5005, 5003, 5004, 5030].contains(i))
        .collect();
    assert_eq!(after, vec![5005, 5003, 5004, 5030]);
    assert_eq!(sim.phone_requests(5030).last().unwrap().payload, vec![0, 0]);

    assert_eq!(finished(&sim), vec![(9, UploadResult::Sent)]);
    assert!(upload_anomalies(&sim).is_empty());
    assert_eq!(sim.client.phase(), Phase::Ready);
    assert_eq!(sim.watch.uploads.len(), 1);
    let got = &sim.watch.uploads[0];
    assert_eq!(got.data, data);
    assert_eq!((got.request.data_type, got.request.sub_type), (128, 5));
}

#[test]
fn every_upload_gets_its_own_file_id() {
    let mut sim = sim();
    sim.input(upload(1, vec![1; 4]));
    sim.input(upload(2, vec![1; 4]));
    let ids: Vec<u64> = sent(&sim, 5005)
        .iter()
        .map(|p| m::parse_create_file(p).unwrap().file_id)
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn a_large_file_is_cut_so_each_whole_frame_fits_the_watchs_max_packet() {
    let mut watch = FakeWatch::new();
    watch.device.max_packet = 4000;
    let mut sim = sim_with_watch(watch);
    let data = lcg_bytes(11, 10_000);
    sim.input(upload(1, data.clone()));

    let chunks = sent(&sim, 5004);
    let sizes: Vec<usize> = chunks.iter().map(|c| c.len() - 7).collect();
    // 3969 is the size of the first chunk of CAP-2's download at max_packet 4000.
    assert_eq!(sizes, vec![3969, 3969, 2062]);
    let mut offset = 0usize;
    for (chunk, size) in chunks.iter().zip(&sizes) {
        let c = m::parse_file_chunk(chunk).unwrap();
        assert_eq!(c.offset as usize, offset);
        offset += size;
        assert_eq!(c.crc, crc::checksum(&data[..offset]), "running CRC");
        assert_eq!(&c.data[..], &data[offset - size..offset]);
    }
    for msg in sim.phone_requests(5004) {
        let frame = cobs::encode_frame(&msg.encode());
        assert!(frame.len() <= 4000, "frame of {} bytes", frame.len());
    }
    assert_eq!(finished(&sim), vec![(1, UploadResult::Sent)]);
    assert_eq!(sim.watch.uploads[0].data, data);
    assert_eq!(sim.watch.uploads[0].announced, 10_000);
}

#[test]
fn with_the_default_link_every_chunk_frame_is_at_most_400_bytes() {
    let mut sim = sim();
    let data = lcg_bytes(3, 2_000);
    sim.input(upload(1, data.clone()));
    let frames: Vec<usize> = sim
        .phone_requests(5004)
        .iter()
        .map(|msg| cobs::encode_frame(&msg.encode()).len())
        .collect();
    assert!(frames.len() > 1);
    assert!(frames.iter().all(|&n| n <= 400), "{frames:?}");
    assert_eq!(sim.watch.uploads[0].data, data);
}

#[test]
fn a_create_refusal_ends_that_upload_and_not_the_session() {
    let mut watch = FakeWatch::new();
    watch.upload_fault = Some(UploadFault::CreateStatus(2));
    let mut sim = sim_with_watch(watch);
    sim.input(upload(1, vec![5; 20]));
    assert_eq!(
        finished(&sim),
        vec![(
            1,
            UploadResult::Refused {
                stage: UploadStage::Create,
                status: 2
            }
        )]
    );
    assert_eq!(upload_anomalies(&sim), vec![5005]);
    assert!(sim.closed_reason().is_none());
    assert!(
        sent(&sim, 5003).is_empty(),
        "nothing after a refused create"
    );
    assert_eq!(sim.client.phase(), Phase::Ready);

    // The session still works: the next upload goes through.
    sim.watch.upload_fault = None;
    sim.input(upload(2, vec![6; 20]));
    assert_eq!(finished(&sim).last(), Some(&(2, UploadResult::Sent)));
}

#[test]
fn an_upload_status_refusal_ends_the_upload_at_5003() {
    let mut watch = FakeWatch::new();
    watch.upload_fault = Some(UploadFault::RequestStatus(5));
    let mut sim = sim_with_watch(watch);
    sim.input(upload(1, vec![5; 20]));
    assert_eq!(
        finished(&sim),
        vec![(
            1,
            UploadResult::Refused {
                stage: UploadStage::Request,
                status: 5
            }
        )]
    );
    assert_eq!(upload_anomalies(&sim), vec![5003]);
    assert!(sent(&sim, 5004).is_empty());
    assert!(sim.closed_reason().is_none());
}

#[test]
fn a_transfer_status_refusal_in_the_middle_ends_the_upload() {
    let mut watch = FakeWatch::new();
    watch.upload_fault = Some(UploadFault::ChunkStatus(2));
    watch.upload_fault_after_chunks = 1;
    let mut sim = sim_with_watch(watch);
    let closings = sim.phone_requests(5030).len();
    sim.input(upload(1, lcg_bytes(1, 1_500)));
    assert_eq!(sent(&sim, 5004).len(), 2, "the second chunk is refused");
    assert_eq!(
        finished(&sim),
        vec![(
            1,
            UploadResult::Refused {
                stage: UploadStage::Data,
                status: 2
            }
        )]
    );
    assert_eq!(upload_anomalies(&sim), vec![5004]);
    assert!(sim.closed_reason().is_none());
    assert_eq!(
        sim.phone_requests(5030).len(),
        closings,
        "no closing SYNC_COMPLETE for a failed upload"
    );
}

#[test]
fn a_nak_at_any_step_ends_the_upload_with_nak_and_the_session_goes_on() {
    for (id, stage) in [
        (5005u16, UploadStage::Create),
        (5003, UploadStage::Request),
        (5004, UploadStage::Data),
    ] {
        let mut watch = FakeWatch::new();
        watch.upload_fault = Some(UploadFault::Nak(id));
        let mut sim = sim_with_watch(watch);
        sim.input(upload(4, vec![9; 30]));
        assert_eq!(
            finished(&sim),
            vec![(4, UploadResult::Nak { stage, status: 1 })],
            "NAK of {id}"
        );
        assert_eq!(upload_anomalies(&sim), vec![id]);
        assert!(sim.closed_reason().is_none());
        assert_eq!(sim.client.phase(), Phase::Ready);
    }
}

#[test]
fn silence_ends_the_upload_after_30_seconds_and_not_the_session() {
    let mut watch = FakeWatch::new();
    watch.upload_fault = Some(UploadFault::Silent(5003));
    let mut sim = sim_with_watch(watch);
    sim.input(upload(1, vec![9; 30]));
    assert!(finished(&sim).is_empty(), "still waiting");
    sim.advance(29_000);
    assert!(finished(&sim).is_empty());
    sim.advance(2_000);
    assert_eq!(
        finished(&sim),
        vec![(
            1,
            UploadResult::TimedOut {
                stage: UploadStage::Request
            }
        )]
    );
    assert_eq!(upload_anomalies(&sim), vec![5003]);
    assert!(sim.closed_reason().is_none());
    assert_eq!(sim.client.phase(), Phase::Ready);

    sim.watch.upload_fault = None;
    sim.input(upload(2, vec![9; 30]));
    assert_eq!(finished(&sim).last(), Some(&(2, UploadResult::Sent)));
}

#[test]
fn replies_that_make_no_sense_end_the_upload_with_bad_reply() {
    let cases = [
        (UploadFault::ShortCreateReply, UploadStage::Create),
        (UploadFault::RequestOffset(5), UploadStage::Request),
        (UploadFault::ChunkOffset(3), UploadStage::Data),
    ];
    for (fault, stage) in cases {
        let mut watch = FakeWatch::new();
        watch.upload_fault = Some(fault);
        let mut sim = sim_with_watch(watch);
        sim.input(upload(1, vec![9; 30]));
        assert_eq!(
            finished(&sim),
            vec![(1, UploadResult::BadReply { stage })],
            "{fault:?}"
        );
        assert_eq!(upload_anomalies(&sim).len(), 1, "{fault:?}");
        assert!(sim.closed_reason().is_none(), "{fault:?}");
    }
}

#[test]
fn uploads_run_one_at_a_time_in_the_order_they_were_given() {
    let mut sim = sim();
    // Hold the first upload at 5003 so the others have to queue.
    sim.watch.upload_fault = Some(UploadFault::Silent(5003));
    sim.input(upload(1, vec![1; 8]));
    sim.input(upload(2, vec![2; 8]));
    sim.input(upload(3, vec![3; 8]));
    assert_eq!(
        sent(&sim, 5005).len(),
        1,
        "only the first one is on the wire"
    );

    sim.watch.upload_fault = None;
    sim.advance(31_000);
    assert_eq!(
        finished(&sim),
        vec![
            (
                1,
                UploadResult::TimedOut {
                    stage: UploadStage::Request
                }
            ),
            (2, UploadResult::Sent),
            (3, UploadResult::Sent),
        ]
    );
    let datas: Vec<Vec<u8>> = sim.watch.uploads.iter().map(|u| u.data.clone()).collect();
    assert_eq!(datas, vec![vec![2; 8], vec![3; 8]]);
}

#[test]
fn nothing_that_cannot_be_sent_reaches_the_wire() {
    let mut sim = sim();
    sim.input(upload(1, Vec::new()));
    sim.input(upload(2, vec![0; m::MAX_UPLOAD_BYTES + 1]));
    assert_eq!(
        finished(&sim),
        vec![(1, UploadResult::Invalid), (2, UploadResult::Invalid)]
    );
    assert!(sent(&sim, 5005).is_empty());

    // A queue that is full refuses the next one, the ones in it still run.
    sim.watch.upload_fault = Some(UploadFault::Silent(5005));
    for id in 10..20 {
        sim.input(upload(id, vec![1; 4]));
    }
    let invalid: Vec<u64> = finished(&sim)
        .iter()
        .filter(|(id, r)| *id >= 10 && *r == UploadResult::Invalid)
        .map(|(id, _)| *id)
        .collect();
    // One runs, eight wait, so the tenth is too many.
    assert_eq!(invalid, vec![19]);
}

#[test]
fn a_disconnect_reports_every_unfinished_upload_aborted_before_closed() {
    let mut sim = sim();
    sim.watch.upload_fault = Some(UploadFault::Silent(5003));
    sim.input(upload(1, vec![1; 8]));
    sim.input(upload(2, vec![2; 8]));
    sim.input(Input::Disconnected);
    let closed_at = sim
        .events
        .iter()
        .position(|e| matches!(e, Event::Closed { .. }))
        .expect("closed");
    let last_upload = sim
        .events
        .iter()
        .rposition(|e| matches!(e, Event::UploadFinished { .. }))
        .expect("reported");
    assert!(
        last_upload < closed_at,
        "aborted uploads come before Closed"
    );
    assert_eq!(
        finished(&sim),
        vec![(1, UploadResult::Aborted), (2, UploadResult::Aborted)]
    );
}

#[test]
fn an_upload_before_the_link_is_up_is_reported_aborted() {
    let mut client = Client::new(test_config());
    let step = client.handle(test_now(0), upload(5, vec![1; 3]));
    assert_eq!(
        step.events,
        vec![Event::UploadFinished {
            upload_id: 5,
            result: UploadResult::Aborted
        }]
    );
}

#[test]
fn an_upload_handed_over_during_the_handshake_waits_for_it() {
    let mut sim = Sim::with(Client::new(test_config()), FakeWatch::new());
    sim.connect();
    sim.input(upload(1, vec![4; 12]));
    assert!(
        sent(&sim, 5005).is_empty(),
        "not before the handshake is done"
    );
    let info = sim.watch.device_information();
    sim.watch_sends(&info);
    assert_eq!(finished(&sim), vec![(1, UploadResult::Sent)]);
    let ids = sim.phone_ids();
    let hs = ids.iter().rposition(|&i| i == 5030).unwrap();
    assert!(ids.iter().position(|&i| i == 5005).unwrap() < hs + 20);
    assert_eq!(sim.watch.uploads.len(), 1);
}

#[test]
fn a_sync_the_watch_asks_for_waits_until_the_upload_is_done() {
    let mut sim = sim();
    sim.watch.upload_fault = Some(UploadFault::Silent(5003));
    sim.input(upload(1, vec![1; 8]));
    sim.watch_sends(&sim.watch.sync_request(0));
    assert!(
        sent(&sim, 5007).is_empty(),
        "no directory request while the upload runs"
    );
    // The watch gets its 5037 acknowledged at once.
    assert_eq!(answers(&sim, 5037).len(), 1);

    sim.watch.upload_fault = None;
    sim.advance(31_000);
    assert_eq!(finished(&sim).len(), 1);
    assert_eq!(sent(&sim, 5007).len(), 1, "the held sync starts afterwards");
}

fn answers(sim: &Sim, request_id: u16) -> Vec<Vec<u8>> {
    sim.phone_responses(request_id)
        .iter()
        .map(|msg| msg.payload.clone())
        .collect()
}

#[test]
fn an_upload_waits_for_a_running_sync_cycle_and_goes_out_after_it() {
    let mut sim = sim();
    sim.watch
        .add_file(fake_entry(3, 4, 600), lcg_bytes(21, 600));
    sim.input(Input::StartSync);
    // The cycle is frozen until the caller says whether it stored the file.
    let (transfer_id, _, _) = sim.downloaded().pop().expect("file downloaded");

    sim.input(upload(1, vec![8; 16]));
    assert!(
        sent(&sim, 5005).is_empty(),
        "no upload in the middle of a cycle"
    );

    sim.input(Input::FileStored { transfer_id });
    assert_eq!(finished(&sim), vec![(1, UploadResult::Sent)]);
    let ids = sim.phone_ids();
    let archive = ids.iter().position(|&i| i == 5008).expect("archived");
    let create = ids.iter().position(|&i| i == 5005).expect("created");
    assert!(
        archive < create,
        "the cycle finishes before the upload begins"
    );
    assert_eq!(sim.watch.uploads[0].data, vec![8; 16]);
}
