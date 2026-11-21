//! Stage 2f, weather: the duty driven only through `Client::handle` against the
//! scripted fake watch. Every frame here is synthetic (made-up positions and
//! made-up "FIT" bytes); the layouts come from Gadgetbridge and are labelled
//! *inferred (Gadgetbridge)* in docs/protocol/PROTOCOL.md until a real watch
//! confirms them. The FIT bytes are opaque to the core, so no profile is needed.

mod support;

use nimrag_core::client::{
    AnomalyKind, Client, Duties, Event, Input, Phase, WeatherCell, WeatherResult,
};
use nimrag_core::gfdi::Message;
use support::*;

/// Capability bit 26 (weather conditions) is byte 3, value 0x04.
const WATCH_WITH_WEATHER: [u8; 4] = [0xAA, 0xBB, 0x00, 0x04];

fn weather_on() -> Duties {
    Duties {
        weather: true,
        ..Duties::default()
    }
}

fn sim_with(duties: Duties, caps: &[u8]) -> Sim {
    let mut watch = FakeWatch::new();
    watch.capabilities = caps.to_vec();
    let mut sim = Sim::with(Client::new(duty_config(duties)), watch);
    sim.handshake();
    sim
}

fn weather_sim() -> Sim {
    sim_with(weather_on(), &WATCH_WITH_WEATHER)
}

/// Degrees to FIT semicircles, as a watch would write them.
fn semi(deg: f64) -> i32 {
    (deg * 2_147_483_648.0 / 180.0).round() as i32
}

/// A 5014 payload: `u8 format, s32 lat, s32 lon, u8 hours`.
fn request_5014(format: u8, lat: i32, lon: i32, hours: u8) -> Message {
    let mut p = vec![format];
    p.extend_from_slice(&lat.to_le_bytes());
    p.extend_from_slice(&lon.to_le_bytes());
    p.push(hours);
    Message::new(5014, p)
}

/// Made-up FIT definition bytes: a definition header, then filler.
fn definitions() -> Vec<u8> {
    vec![0x40, 0, 0, 128, 0, 2, 253, 4, 0x86, 1, 1, 0x02]
}

/// `n` made-up data records of `len` bytes each (header byte 0 = local type 0).
fn records(n: u8, len: usize) -> Vec<Vec<u8>> {
    (0..n)
        .map(|i| {
            let mut r = vec![0x00];
            r.extend(std::iter::repeat(i + 1).take(len - 1));
            r
        })
        .collect()
}

fn send(sim: &mut Sim, definitions: Vec<u8>, records: Vec<Vec<u8>>) {
    sim.input(Input::SendWeather {
        definitions,
        records,
    });
}

fn results(sim: &Sim) -> Vec<WeatherResult> {
    sim.events
        .iter()
        .filter_map(|e| match e {
            Event::WeatherSent { result } => Some(*result),
            _ => None,
        })
        .collect()
}

fn requests(sim: &Sim) -> Vec<Event> {
    sim.events
        .iter()
        .filter(|e| matches!(e, Event::WeatherRequest { .. }))
        .cloned()
        .collect()
}

/// Answer the last phone message with this id: 5000, status, then the code.
fn answer(sim: &mut Sim, id: u16, status: u8, code: u8) {
    let last = (*sim.phone_requests(id).last().expect("phone sent it")).clone();
    sim.watch_sends(&watch_response(&last, status, &[code]));
}

fn payloads(sim: &Sim, id: u16) -> Vec<Vec<u8>> {
    sim.phone_requests(id)
        .iter()
        .map(|m| m.payload.clone())
        .collect()
}

// ---- weather off: the behaviour before this duty ----------------------------------------

#[test]
fn with_weather_off_a_5014_gets_status_two_and_no_event() {
    let mut sim = sim_with(Duties::default(), &WATCH_WITH_WEATHER);
    sim.watch_sends(&request_5014(1, semi(55.6761), semi(12.5683), 12));
    let answers: Vec<Vec<u8>> = sim
        .phone_responses(5014)
        .iter()
        .map(|m| m.payload.clone())
        .collect();
    assert_eq!(answers, vec![vec![0x96, 0x13, 2]]);
    assert!(sim
        .events
        .contains(&Event::Unsupported { message_id: 5014 }));
    assert!(requests(&sim).is_empty());
}

#[test]
fn with_weather_off_send_weather_sends_nothing_and_says_so() {
    let mut sim = sim_with(Duties::default(), &WATCH_WITH_WEATHER);
    let before = sim.phone_writes.len();
    send(&mut sim, definitions(), records(2, 20));
    assert_eq!(sim.phone_writes.len(), before);
    assert_eq!(results(&sim), vec![WeatherResult::NotSupported]);
}

#[test]
fn send_weather_before_the_link_is_up_is_reported() {
    let mut client = Client::new(duty_config(weather_on()));
    let step = client.handle(
        test_now(0),
        Input::SendWeather {
            definitions: definitions(),
            records: records(1, 10),
        },
    );
    assert_eq!(
        step.events,
        vec![Event::WeatherSent {
            result: WeatherResult::NotSupported
        }]
    );
    assert!(step.writes.is_empty());
}

// ---- the watch's request -----------------------------------------------------------------

#[test]
fn a_5014_is_acknowledged_and_reported_with_a_position_rounded_to_a_tenth_of_a_degree() {
    let mut sim = weather_sim();
    sim.watch_sends(&request_5014(1, semi(55.6761), semi(12.5683), 12));
    // 5014 = 0x1396: the ACK is `96 13 00`, status ACK, nothing more
    let answers: Vec<Vec<u8>> = sim
        .phone_responses(5014)
        .iter()
        .map(|m| m.payload.clone())
        .collect();
    assert_eq!(answers, vec![vec![0x96, 0x13, 0]]);
    assert_eq!(
        requests(&sim),
        vec![Event::WeatherRequest {
            format: 1,
            cell: Some(WeatherCell {
                lat_tenths: 557,
                lon_tenths: 126
            }),
            hours: 12
        }]
    );
    assert_eq!(sim.client.phase(), Phase::Ready);
}

#[test]
fn the_exact_position_is_nowhere_in_the_events() {
    let mut sim = weather_sim();
    let (lat, lon) = (semi(55.6761), semi(12.5683));
    sim.watch_sends(&request_5014(1, lat, lon, 12));
    let text = format!("{:?}", sim.events);
    // neither the raw integers, nor the degrees, nor the rounded tenths
    for secret in [
        lat.to_string(),
        lon.to_string(),
        "55.6".into(),
        "557".into(),
        "126".into(),
    ] {
        assert!(!text.contains(&secret), "events show {secret}: {text}");
    }
}

#[test]
fn a_watch_without_a_position_gets_a_request_without_a_cell() {
    let mut sim = weather_sim();
    sim.watch_sends(&request_5014(0, 0, 0, 6));
    sim.watch_sends(&request_5014(0, i32::MAX, i32::MAX, 6));
    assert_eq!(
        requests(&sim),
        vec![
            Event::WeatherRequest {
                format: 0,
                cell: None,
                hours: 6
            };
            2
        ]
    );
    // both were acknowledged
    assert_eq!(sim.phone_responses(5014).len(), 2);
}

#[test]
fn a_short_5014_is_an_anomaly_and_not_acknowledged() {
    let mut sim = weather_sim();
    sim.watch_sends(&Message::new(5014, vec![1, 2, 3]));
    assert!(sim.phone_responses(5014).is_empty());
    assert!(requests(&sim).is_empty());
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::BadFrame,
        message_id: 5014
    }));
    assert_eq!(sim.client.phase(), Phase::Ready);
}

// ---- the answer: 5011, then 5012 --------------------------------------------------------

#[test]
fn the_definitions_go_first_then_the_data_then_the_result() {
    let mut sim = weather_sim();
    let recs = records(3, 20);
    send(&mut sim, definitions(), recs.clone());

    // only 5011 is out, byte for byte what the host gave
    assert_eq!(payloads(&sim, 5011), vec![definitions()]);
    assert!(payloads(&sim, 5012).is_empty(), "data waits for the status");
    assert!(results(&sim).is_empty());

    answer(&mut sim, 5011, 0, 0);
    assert_eq!(payloads(&sim, 5012), vec![recs.concat()]);
    assert!(results(&sim).is_empty(), "the data status is still due");

    answer(&mut sim, 5012, 0, 0);
    assert_eq!(results(&sim), vec![WeatherResult::Sent]);
    assert_eq!(payloads(&sim, 5011).len(), 1);
    assert_eq!(payloads(&sim, 5012).len(), 1);
    assert_eq!(sim.client.phase(), Phase::Ready);
}

#[test]
fn the_messages_on_the_wire_are_byte_exact() {
    let mut sim = weather_sim();
    send(&mut sim, definitions(), vec![vec![0x00, 0xDE, 0xAD]]);
    let def = (*sim.phone_requests(5011)[0]).clone();
    // compact id form: id - 5000 = 11, then 0x80 | txn
    let txn = def.txn.expect("a request carries a txn");
    let mut want = Vec::new();
    want.extend_from_slice(&((definitions().len() + 6) as u16).to_le_bytes());
    want.extend_from_slice(&[11, 0x80 | txn]);
    want.extend_from_slice(&definitions());
    let crc = nimrag_core::crc::checksum(&want);
    want.extend_from_slice(&crc.to_le_bytes());
    assert_eq!(def.encode(), want);

    answer(&mut sim, 5011, 0, 0);
    let data = (*sim.phone_requests(5012)[0]).clone();
    let txn = data.txn.unwrap();
    let mut want = vec![9, 0, 12, 0x80 | txn, 0x00, 0xDE, 0xAD];
    let crc = nimrag_core::crc::checksum(&want);
    want.extend_from_slice(&crc.to_le_bytes());
    assert_eq!(data.encode(), want);
}

#[test]
fn many_records_are_cut_into_whole_records_that_fit_one_message() {
    // max_packet is 400: at most 390 payload bytes per message. 100-byte
    // records fit three to a message.
    let mut sim = weather_sim();
    let recs = records(7, 100);
    send(&mut sim, definitions(), recs.clone());
    answer(&mut sim, 5011, 0, 0);

    let mut got = Vec::new();
    for round in 1..=3 {
        let chunks = payloads(&sim, 5012);
        assert_eq!(chunks.len(), round, "one chunk per status");
        got.push(chunks.last().unwrap().clone());
        assert!(results(&sim).is_empty());
        answer(&mut sim, 5012, 0, 0);
    }
    assert_eq!(
        got.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![300, 300, 100]
    );
    assert_eq!(got.concat(), recs.concat());
    assert_eq!(results(&sim), vec![WeatherResult::Sent]);
    assert_eq!(
        payloads(&sim, 5012).len(),
        3,
        "nothing after the last status"
    );
}

#[test]
fn a_definition_the_watch_did_not_apply_does_not_stop_the_data() {
    // Gadgetbridge sends the data anyway
    let mut sim = weather_sim();
    send(&mut sim, definitions(), records(2, 10));
    answer(&mut sim, 5011, 0, 1); // NOT_UNIQUE
    assert_eq!(payloads(&sim, 5012).len(), 1);
    answer(&mut sim, 5012, 0, 0);
    assert_eq!(results(&sim), vec![WeatherResult::Sent]);
}

#[test]
fn data_the_watch_does_not_apply_ends_the_answer_but_not_the_session() {
    let mut sim = weather_sim();
    send(&mut sim, definitions(), records(7, 100));
    answer(&mut sim, 5011, 0, 0);
    answer(&mut sim, 5012, 0, 2); // MISMATCH
    assert_eq!(payloads(&sim, 5012).len(), 1, "no further chunk");
    assert_eq!(results(&sim), vec![WeatherResult::Refused]);
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::DutyMessageRefused,
        message_id: 5012
    }));
    assert_eq!(sim.client.phase(), Phase::Ready);
    assert!(sim.closed_reason().is_none());
}

#[test]
fn a_nak_for_the_definitions_ends_the_answer_but_not_the_session() {
    let mut sim = weather_sim();
    send(&mut sim, definitions(), records(2, 10));
    answer(&mut sim, 5011, 1, 0); // NAK
    assert!(payloads(&sim, 5012).is_empty());
    assert_eq!(results(&sim), vec![WeatherResult::Refused]);
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::DutyMessageRefused,
        message_id: 5011
    }));
    assert!(sim.closed_reason().is_none());
}

#[test]
fn a_silent_watch_costs_one_anomaly_and_the_next_answer_still_works() {
    let mut sim = weather_sim();
    send(&mut sim, definitions(), records(2, 10));
    sim.advance(31_000);
    assert!(sim.closed_reason().is_none());
    assert_eq!(results(&sim), vec![WeatherResult::TimedOut]);
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::DutyMessageRefused,
        message_id: 5011
    }));
    assert_eq!(sim.client.phase(), Phase::Ready);

    // the line is free again
    send(&mut sim, definitions(), records(2, 10));
    assert_eq!(payloads(&sim, 5011).len(), 2);
    answer(&mut sim, 5011, 0, 0);
    answer(&mut sim, 5012, 0, 0);
    assert_eq!(
        results(&sim),
        vec![WeatherResult::TimedOut, WeatherResult::Sent]
    );
}

#[test]
fn a_second_answer_while_one_is_on_its_way_is_busy_and_the_first_still_completes() {
    let mut sim = weather_sim();
    send(&mut sim, definitions(), records(2, 10));
    send(&mut sim, definitions(), records(3, 10));
    assert_eq!(results(&sim), vec![WeatherResult::Busy]);
    assert_eq!(payloads(&sim, 5011).len(), 1);
    answer(&mut sim, 5011, 0, 0);
    answer(&mut sim, 5012, 0, 0);
    assert_eq!(
        results(&sim),
        vec![WeatherResult::Busy, WeatherResult::Sent]
    );
    // and a new one is accepted afterwards
    send(&mut sim, definitions(), records(1, 10));
    assert_eq!(payloads(&sim, 5011).len(), 2);
}

#[test]
fn a_watch_without_capability_26_is_not_sent_weather() {
    let mut sim = sim_with(weather_on(), &[0xAA, 0xBB, 0x00, 0x00]);
    let before = sim.phone_writes.len();
    send(&mut sim, definitions(), records(2, 10));
    assert_eq!(sim.phone_writes.len(), before);
    assert_eq!(results(&sim), vec![WeatherResult::NotSupported]);
    // the 5014 itself is still acknowledged and reported: the host decides
    sim.watch_sends(&request_5014(1, semi(55.6), semi(12.5), 12));
    assert_eq!(requests(&sim).len(), 1);
}

#[test]
fn bytes_that_are_not_fit_records_are_invalid_and_never_sent() {
    let mut sim = weather_sim();
    let before = sim.phone_writes.len();
    send(&mut sim, vec![], records(1, 10));
    send(&mut sim, definitions(), vec![]);
    send(&mut sim, vec![0x00, 1, 2], records(1, 10)); // not a definition header
    send(&mut sim, definitions(), vec![vec![0x40, 1, 2]]); // not a data header
    send(&mut sim, definitions(), vec![vec![0x00; 500]]); // one record over a message
    assert_eq!(sim.phone_writes.len(), before);
    assert_eq!(results(&sim), vec![WeatherResult::Invalid; 5]);
    // an invalid answer does not occupy the duty
    send(&mut sim, definitions(), records(1, 10));
    assert_eq!(payloads(&sim, 5011).len(), 1);
}
