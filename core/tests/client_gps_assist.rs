//! Stage 2f: GPS assist and the HTTP firewall, driven only through
//! `Client::handle` against the scripted fake watch. Every frame is synthetic
//! (made-up files, made-up ETags); the layouts come from Gadgetbridge and are
//! labelled *inferred (Gadgetbridge)* in docs/protocol/PROTOCOL.md until a real
//! Instinct 2 confirms them (Q-COMP-1). The protobuf bytes are written here by
//! hand, not with the crate's own encoder, so a mistake in one is not repeated
//! in the other.

mod support;

use nimrag_core::client::{AnomalyKind, Client, Duties, Event, GpsAssistOutcome, Input, Phase};
use nimrag_core::duties::agps;
use nimrag_core::gfdi::Message;
use nimrag_core::messages as m;
use support::*;

// ---- a tiny protobuf writer, independent of the crate's ----------------------------

fn varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while v >= 0x80 {
        out.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
    out
}

fn field(number: u64, wire: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = varint((number << 3) | wire);
    if wire == 2 {
        out.extend(varint(payload.len() as u64));
    }
    out.extend_from_slice(payload);
    out
}

fn bytes_field(number: u64, payload: &[u8]) -> Vec<u8> {
    field(number, 2, payload)
}

fn uint_field(number: u64, v: u64) -> Vec<u8> {
    field(number, 0, &varint(v))
}

fn concat(parts: &[Vec<u8>]) -> Vec<u8> {
    parts.iter().flatten().copied().collect()
}

fn header(key: &str, value: &str) -> Vec<u8> {
    concat(&[
        bytes_field(1, key.as_bytes()),
        bytes_field(2, value.as_bytes()),
    ])
}

/// `Smart { connect_iq_http_service { raw_resource_request { ... } } }`.
fn raw_request(url: &str, headers: &[(&str, &str)], use_xfer: bool) -> Vec<u8> {
    let mut parts = vec![bytes_field(1, url.as_bytes())];
    for (k, v) in headers {
        parts.push(bytes_field(5, &header(k, v)));
    }
    if use_xfer {
        parts.push(uint_field(6, 1));
    }
    smart_http(5, &concat(&parts))
}

fn smart_http(message: u64, body: &[u8]) -> Vec<u8> {
    bytes_field(2, &bytes_field(message, body))
}

fn smart_transfer(message: u64, body: &[u8]) -> Vec<u8> {
    bytes_field(7, &bytes_field(message, body))
}

fn download_request(id: u64, offset: u64, max: Option<u64>) -> Vec<u8> {
    let mut parts = vec![uint_field(1, id), uint_field(2, offset)];
    if let Some(max) = max {
        parts.push(uint_field(3, max));
    }
    smart_transfer(1, &concat(&parts))
}

const GOOD_URL: &str =
    "https://api.gcs.garmin.com/ephemeris/cpe/sony/lle?coverage=WEEKS_1&constellations=GPS,GLONASS,GALILEO,QZSS";
const GOOD_PATH: &str =
    "/ephemeris/cpe/sony/lle?coverage=WEEKS_1&constellations=GPS,GLONASS,GALILEO,QZSS";

fn gps_on() -> Duties {
    Duties {
        gps_assist: true,
        ..Duties::default()
    }
}

fn sim_on() -> Sim {
    let mut sim = Sim::with(Client::new(duty_config(gps_on())), FakeWatch::new());
    sim.handshake();
    sim
}

/// A synthetic tar of (name, size) entries laid out like `tar` does.
fn tar(entries: &[(&str, usize)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, size) in entries {
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..107].copy_from_slice(b"0100644");
        h[124..135].copy_from_slice(format!("{size:011o}").as_bytes());
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        out.extend_from_slice(&h);
        out.extend(lcg_bytes(*size as u32 + 1, *size));
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.resize(out.len() + 1024, 0);
    out
}

fn four_constellations() -> Vec<u8> {
    tar(&[
        ("CPE_GLO.BIN", 300),
        ("CPE_GAL.BIN", 700),
        ("CPE_GPS.BIN", 900),
        ("CPE_QZSS.BIN", 100),
    ])
}

const ETAG: &str = "\"11111111-2222-3333-4444-555555555555\"";

/// The 5043 request from the watch, with its own request id.
fn watch_request(sim: &mut Sim, request_id: u16, smart: &[u8]) {
    sim.watch_sends(&Message::new(5043, m::build_protobuf(request_id, smart)));
}

/// The protobuf messages (5044) the phone sent, reassembled: (request id, Smart
/// bytes), in the order their last chunk was sent. Checks every chunk's header
/// (offsets follow each other, the total is the same, nothing is over
/// `max_packet - 20` bytes).
fn replies(sim: &Sim) -> Vec<(u16, Vec<u8>)> {
    let mut open: Vec<(u16, Vec<u8>, usize)> = Vec::new();
    let mut done = Vec::new();
    for msg in sim.phone_requests(5044) {
        let (h, body) = m::parse_protobuf(&msg.payload).expect("5044 header");
        assert!(
            body.len() <= 400 - 20,
            "chunk of {} bytes does not fit max_packet 400",
            body.len()
        );
        assert_eq!(h.chunk as usize, body.len());
        let at = open.iter().position(|(id, _, _)| *id == h.request_id);
        let (buf, total) = match at {
            Some(i) => {
                let entry = &mut open[i];
                assert_eq!(h.offset as usize, entry.1.len(), "chunks in order");
                assert_eq!(h.total as usize, entry.2, "same total");
                (&mut entry.1, entry.2)
            }
            None => {
                assert_eq!(h.offset, 0, "a new message starts at 0");
                open.push((h.request_id, Vec::new(), h.total as usize));
                let entry = open.last_mut().unwrap();
                (&mut entry.1, entry.2)
            }
        };
        buf.extend_from_slice(body);
        if buf.len() == total {
            let i = open
                .iter()
                .position(|(id, _, _)| *id == h.request_id)
                .unwrap();
            let (id, bytes, _) = open.remove(i);
            done.push((id, bytes));
        }
    }
    assert!(open.is_empty(), "a reply was left unfinished");
    done
}

fn request_events(sim: &Sim) -> Vec<(u32, String, Option<String>)> {
    sim.events
        .iter()
        .filter_map(|e| match e {
            Event::GpsAssistRequest {
                id,
                path_and_query,
                if_none_match,
            } => Some((*id, path_and_query.clone(), if_none_match.clone())),
            _ => None,
        })
        .collect()
}

fn done_events(sim: &Sim) -> Vec<(u32, GpsAssistOutcome)> {
    sim.events
        .iter()
        .filter_map(|e| match e {
            Event::GpsAssistDone { id, outcome } => Some((*id, *outcome)),
            _ => None,
        })
        .collect()
}

fn refused_events(sim: &Sim) -> Vec<(String, String)> {
    sim.events
        .iter()
        .filter_map(|e| match e {
            Event::HttpRequestRefused { host, path } => Some((host.clone(), path.clone())),
            _ => None,
        })
        .collect()
}

/// Smart bytes of `raw_resource_response { status 100, http status, headers,
/// resource_data }` in field-number order.
fn expected_inline(http_status: u64, data: &[u8], headers: &[(&str, &str)]) -> Vec<u8> {
    let mut parts = vec![
        uint_field(1, 100),
        uint_field(2, http_status),
        bytes_field(3, data),
    ];
    for (k, v) in headers {
        parts.push(bytes_field(5, &header(k, v)));
    }
    smart_http(6, &concat(&parts))
}

/// `raw_resource_response { status NETWORK_REQUEST_TIMED_OUT }`.
fn expected_timed_out() -> Vec<u8> {
    smart_http(6, &uint_field(1, 200))
}

// ---- the duty off: the behaviour before GPS assist -------------------------------

#[test]
fn with_the_duty_off_a_request_is_acknowledged_and_ignored() {
    let mut sim = Sim::new();
    sim.handshake();
    watch_request(&mut sim, 7, &raw_request(GOOD_URL, &[], false));
    assert_eq!(sim.phone_responses(5043).len(), 1);
    assert!(sim.phone_requests(5044).is_empty());
    assert!(request_events(&sim).is_empty() && refused_events(&sim).is_empty());
    // Nor does an answer from the host do anything.
    sim.input(Input::GpsAssistData {
        id: 1,
        body: vec![1],
        etag: ETAG.into(),
    });
    sim.input(Input::GpsAssistUnavailable { id: 1 });
    assert!(sim.phone_requests(5044).is_empty());
}

#[test]
fn with_the_duty_off_a_chunked_protobuf_is_still_an_anomaly() {
    let mut sim = Sim::new();
    sim.handshake();
    let mut p = 7u16.to_le_bytes().to_vec();
    p.extend_from_slice(&0u32.to_le_bytes());
    p.extend_from_slice(&900u32.to_le_bytes());
    p.extend_from_slice(&4u32.to_le_bytes());
    p.extend_from_slice(&[1, 2, 3, 4]);
    sim.watch_sends(&Message::new(5043, p));
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::ChunkedProtobuf,
        message_id: 5043
    }));
}

// ---- a request for the ephemeris -------------------------------------------------------

#[test]
fn an_ephemeris_request_becomes_an_event_and_is_acknowledged() {
    let mut sim = sim_on();
    watch_request(
        &mut sim,
        0x22E4,
        &raw_request(GOOD_URL, &[("If-None-Match", "\"old\"")], false),
    );
    assert_eq!(
        request_events(&sim),
        vec![(1, GOOD_PATH.to_string(), Some("\"old\"".to_string()))]
    );
    // The 5043 got the usual 8-byte ACK body.
    let acks: Vec<Vec<u8>> = sim
        .phone_responses(5043)
        .iter()
        .map(|m| m.payload.clone())
        .collect();
    assert_eq!(
        acks,
        vec![vec![0xB3, 0x13, 0, 0xE4, 0x22, 0, 0, 0, 0, 0, 0]]
    );
    // Nothing is sent until the host answers.
    assert!(sim.phone_requests(5044).is_empty());
    assert!(done_events(&sim).is_empty() && refused_events(&sim).is_empty());
}

#[test]
fn the_file_is_answered_200_byte_exact_and_reported_sent() {
    let mut sim = sim_on();
    let tar = four_constellations();
    watch_request(
        &mut sim,
        0x0102,
        &raw_request(
            GOOD_URL,
            &[("accept", "application/x-tar"), ("User-Agent", "x")],
            false,
        ),
    );
    sim.input(Input::GpsAssistData {
        id: 1,
        body: tar.clone(),
        etag: ETAG.into(),
    });
    let got = replies(&sim);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, 0x0102, "answered with the watch's request id");
    assert_eq!(
        got[0].1,
        expected_inline(
            200,
            &tar,
            &[
                ("etag", ETAG),
                ("cache-control", "max-age=14400"),
                ("Content-Type", "application/x-tar"),
            ]
        )
    );
    // Several chunks, each of the watch's OK answered.
    assert!(sim.phone_requests(5044).len() > 4);
    assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::Sent)]);
    assert_eq!(sim.client.phase(), Phase::Ready);
}

#[test]
fn the_default_content_type_is_octet_stream() {
    let mut sim = sim_on();
    watch_request(&mut sim, 5, &raw_request(GOOD_URL, &[], false));
    sim.input(Input::GpsAssistData {
        id: 1,
        body: four_constellations(),
        etag: ETAG.into(),
    });
    let smart = &replies(&sim)[0].1;
    let want = expected_inline(
        200,
        &four_constellations(),
        &[
            ("etag", ETAG),
            ("cache-control", "max-age=14400"),
            ("Content-Type", "application/octet-stream"),
        ],
    );
    assert_eq!(smart, &want);
}

#[test]
fn a_matching_etag_is_answered_304_without_a_body() {
    let mut sim = sim_on();
    watch_request(
        &mut sim,
        9,
        &raw_request(GOOD_URL, &[("if-none-match", ETAG)], false),
    );
    sim.input(Input::GpsAssistData {
        id: 1,
        body: four_constellations(),
        etag: ETAG.into(),
    });
    assert_eq!(
        replies(&sim),
        vec![(9, expected_inline(304, &[], &[("etag", ETAG)]))]
    );
    assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::NotModified)]);
}

#[test]
fn another_etag_gets_the_file_and_the_new_etag() {
    let mut sim = sim_on();
    watch_request(
        &mut sim,
        9,
        &raw_request(GOOD_URL, &[("if-none-match", "\"stale\"")], false),
    );
    sim.input(Input::GpsAssistData {
        id: 1,
        body: four_constellations(),
        etag: ETAG.into(),
    });
    let smart = &replies(&sim)[0].1;
    assert!(smart.windows(ETAG.len()).any(|w| w == ETAG.as_bytes()));
    assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::Sent)]);
}

#[test]
fn a_gzip_request_gets_a_gzip_body_and_the_header() {
    let mut sim = sim_on();
    watch_request(
        &mut sim,
        3,
        &raw_request(GOOD_URL, &[("accept-encoding", "gzip")], false),
    );
    let tar = four_constellations();
    sim.input(Input::GpsAssistData {
        id: 1,
        body: tar.clone(),
        etag: ETAG.into(),
    });
    let smart = replies(&sim).remove(0).1;
    // Smart.2.6: status, http status, resource_data, headers.
    let gz = agps::gzip_stored(&tar);
    let want = expected_inline(
        200,
        &gz,
        &[
            ("etag", ETAG),
            ("cache-control", "max-age=14400"),
            ("Content-Type", "application/octet-stream"),
            ("Content-Encoding", "gzip"),
        ],
    );
    assert_eq!(smart, want);
    assert_eq!(
        agps::gunzip_prefix(&gz, tar.len()).as_deref(),
        Some(&tar[..])
    );
}

#[test]
fn the_other_formats_are_served_too() {
    // Sony CPE.
    let mut sim = sim_on();
    watch_request(
        &mut sim,
        1,
        &raw_request(
            "https://api.gcs.garmin.com/ephemeris/cpe/sony/lle",
            &[],
            false,
        ),
    );
    let mut sony = vec![0x2A, 0x12, 0xA0, 0x02];
    sony.extend(lcg_bytes(3, 200));
    sim.input(Input::GpsAssistData {
        id: 1,
        body: sony,
        etag: ETAG.into(),
    });
    assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::Sent)]);

    // RxNetworks: gzip of 01 00 and a time a minute before the phone's now.
    let mut sim = sim_on();
    watch_request(
        &mut sim,
        1,
        &raw_request(
            "https://api.gcs.garmin.cn/ephemeris/rxnetworks/v1/x",
            &[],
            false,
        ),
    );
    let mut content = vec![0x01, 0x00];
    content.extend_from_slice(&((UNIX_NOW - 60) as u32).to_be_bytes());
    content.extend(lcg_bytes(9, 100));
    sim.input(Input::GpsAssistData {
        id: 1,
        body: agps::gzip_stored(&content),
        etag: ETAG.into(),
    });
    assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::Sent)]);
}

// ---- files and answers that must not be sent -----------------------------------------

#[test]
fn a_tar_missing_a_requested_constellation_is_refused() {
    let mut sim = sim_on();
    watch_request(&mut sim, 4, &raw_request(GOOD_URL, &[], false));
    sim.input(Input::GpsAssistData {
        id: 1,
        // No CPE_QZSS.BIN.
        body: tar(&[
            ("CPE_GLO.BIN", 100),
            ("CPE_GAL.BIN", 100),
            ("CPE_GPS.BIN", 100),
        ]),
        etag: ETAG.into(),
    });
    assert_eq!(replies(&sim), vec![(4, expected_timed_out())]);
    assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::Refused)]);
}

#[test]
fn the_wrong_kind_of_file_and_empty_files_are_refused() {
    for body in [b"<html>not a tar</html>".to_vec(), Vec::new()] {
        let mut sim = sim_on();
        watch_request(&mut sim, 4, &raw_request(GOOD_URL, &[], false));
        sim.input(Input::GpsAssistData {
            id: 1,
            body,
            etag: ETAG.into(),
        });
        assert_eq!(replies(&sim), vec![(4, expected_timed_out())]);
        assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::Refused)]);
    }
    // A stale RxNetworks file (older than 7 days) is refused.
    let mut sim = sim_on();
    watch_request(
        &mut sim,
        4,
        &raw_request(
            "https://api.gcs.garmin.com/ephemeris/rxnetworks/x",
            &[],
            false,
        ),
    );
    let mut content = vec![0x01, 0x00];
    content.extend_from_slice(&((UNIX_NOW - 8 * 86_400) as u32).to_be_bytes());
    sim.input(Input::GpsAssistData {
        id: 1,
        body: agps::gzip_stored(&content),
        etag: ETAG.into(),
    });
    assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::Refused)]);
}

#[test]
fn a_file_over_one_mebibyte_is_refused() {
    let mut sim = sim_on();
    watch_request(
        &mut sim,
        4,
        &raw_request(
            "https://api.gcs.garmin.com/ephemeris/cpe/sony/lle",
            &[],
            false,
        ),
    );
    let mut body = vec![0x2A, 0x12, 0xA0, 0x02];
    body.resize(1024 * 1024 + 1, 0);
    sim.input(Input::GpsAssistData {
        id: 1,
        body,
        etag: ETAG.into(),
    });
    assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::Refused)]);
    assert!(
        sim.phone_requests(5044).len() == 1,
        "only the short refusal"
    );
}

#[test]
fn no_data_from_the_host_is_told_to_the_watch() {
    let mut sim = sim_on();
    watch_request(&mut sim, 4, &raw_request(GOOD_URL, &[], false));
    sim.input(Input::GpsAssistUnavailable { id: 1 });
    assert_eq!(replies(&sim), vec![(4, expected_timed_out())]);
    assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::Unavailable)]);
    // An answer for a request that is gone changes nothing.
    sim.input(Input::GpsAssistData {
        id: 1,
        body: four_constellations(),
        etag: ETAG.into(),
    });
    sim.input(Input::GpsAssistData {
        id: 99,
        body: four_constellations(),
        etag: ETAG.into(),
    });
    assert_eq!(replies(&sim).len(), 1);
    assert_eq!(done_events(&sim).len(), 1);
}

#[test]
fn requests_are_told_apart_by_their_id() {
    let mut sim = sim_on();
    watch_request(&mut sim, 10, &raw_request(GOOD_URL, &[], false));
    watch_request(
        &mut sim,
        11,
        &raw_request(
            "https://api.gcs.garmin.com/ephemeris/cpe/sony/lle",
            &[],
            false,
        ),
    );
    let ids: Vec<u32> = request_events(&sim).iter().map(|r| r.0).collect();
    assert_eq!(ids, vec![1, 2]);
    // The second is answered first.
    let mut sony = vec![0x2A, 0x12, 0xA0, 0x02];
    sony.extend(lcg_bytes(1, 50));
    sim.input(Input::GpsAssistData {
        id: 2,
        body: sony,
        etag: "\"s\"".into(),
    });
    sim.input(Input::GpsAssistData {
        id: 1,
        body: four_constellations(),
        etag: ETAG.into(),
    });
    let got = replies(&sim);
    assert_eq!(got.iter().map(|r| r.0).collect::<Vec<_>>(), vec![11, 10]);
    assert_eq!(
        done_events(&sim),
        vec![(2, GpsAssistOutcome::Sent), (1, GpsAssistOutcome::Sent)]
    );
}

// ---- the firewall -----------------------------------------------------------------------

#[test]
fn every_other_request_is_refused_and_logged_without_its_query() {
    let cases: [(&str, &str, &str); 6] = [
        // a Garmin host that is not the ephemeris service
        (
            "https://weather.data.garmin.com/weather/v2/55.676,12.568/forecast?lat=55.676&lon=12.568&token=SECRET",
            "weather.data.garmin.com",
            "/weather/v2/*/forecast",
        ),
        // the ephemeris host, another service
        (
            "https://api.gcs.garmin.com/other/thing?secret=1",
            "api.gcs.garmin.com",
            "/other/thing",
        ),
        // not Garmin at all: never forwarded either
        (
            "https://example.org/a/b?q=SECRET",
            "example.org",
            "/a/b",
        ),
        // an unknown ephemeris path
        (
            "https://api.gcs.garmin.com/ephemeris/other?x=SECRET",
            "api.gcs.garmin.com",
            "/ephemeris/other",
        ),
        // a path that is not clean
        (
            "https://api.gcs.garmin.com/ephemeris/../admin?x=SECRET",
            "api.gcs.garmin.com",
            "/ephemeris/*/admin",
        ),
        // an unknown constellation
        (
            "https://api.gcs.garmin.com/ephemeris/cpe/sony/lle?constellations=GPS,BEIDOU&k=SECRET",
            "api.gcs.garmin.com",
            "/ephemeris/cpe/sony/lle",
        ),
    ];
    for (url, host, path) in cases {
        let mut sim = sim_on();
        watch_request(&mut sim, 6, &raw_request(url, &[], false));
        assert!(request_events(&sim).is_empty(), "{url}");
        assert_eq!(
            refused_events(&sim),
            vec![(host.to_string(), path.to_string())],
            "{url}"
        );
        assert_eq!(replies(&sim), vec![(6, expected_timed_out())], "{url}");
        // Nothing of the query reaches an event.
        let shown = format!("{:?}", sim.events);
        for secret in ["SECRET", "55.676", "12.568", "lat=", "token"] {
            assert!(!shown.contains(secret), "{secret} leaked for {url}");
        }
        assert_eq!(sim.client.phase(), Phase::Ready);
    }
}

#[test]
fn a_post_to_the_ephemeris_host_and_forms_are_not_served() {
    // POST (method 3) to the ephemeris path.
    let mut sim = sim_on();
    let post = smart_http(
        5,
        &concat(&[bytes_field(1, GOOD_URL.as_bytes()), uint_field(3, 3)]),
    );
    watch_request(&mut sim, 6, &post);
    assert!(request_events(&sim).is_empty());
    assert_eq!(replies(&sim), vec![(6, expected_timed_out())]);

    // Forms: refused without any answer (as Gadgetbridge).
    let mut sim = sim_on();
    let forms = smart_http(
        5,
        &concat(&[
            bytes_field(1, b"https://example.org/up"),
            bytes_field(8, &bytes_field(1, b"f")),
        ]),
    );
    watch_request(&mut sim, 6, &forms);
    assert_eq!(
        refused_events(&sim),
        vec![("example.org".to_string(), "/up".to_string())]
    );
    assert!(sim.phone_requests(5044).is_empty());
}

#[test]
fn a_connect_iq_request_gets_the_web_form_of_no_data() {
    let mut sim = sim_on();
    let web = smart_http(1, &bytes_field(1, b"https://example.org/x?k=SECRET"));
    watch_request(&mut sim, 8, &web);
    assert_eq!(
        refused_events(&sim),
        vec![("example.org".to_string(), "/x".to_string())]
    );
    // connect_iq_http_response { status NETWORK_REQUEST_TIMED_OUT, code 0 }
    assert_eq!(
        replies(&sim),
        vec![(
            8,
            smart_http(2, &concat(&[uint_field(1, 300), uint_field(2, 0)]))
        )]
    );
}

#[test]
fn requests_we_cannot_read_or_answer_get_no_answer_but_are_logged() {
    let mut sim = sim_on();
    // Only the opaque field 11 (what our captures show).
    watch_request(
        &mut sim,
        1,
        &smart_http(5, &bytes_field(11, &lcg_bytes(5, 60))),
    );
    // Open a web page, an image request, an OAuth request.
    watch_request(
        &mut sim,
        2,
        &smart_http(11, &bytes_field(1, b"https://example.org/p?x=1")),
    );
    watch_request(
        &mut sim,
        3,
        &smart_http(3, &bytes_field(1, b"https://example.org/i.png")),
    );
    watch_request(
        &mut sim,
        4,
        &smart_http(7, &bytes_field(1, b"https://example.org/auth?c=1")),
    );
    assert_eq!(
        refused_events(&sim),
        vec![
            (String::new(), String::new()),
            ("example.org".to_string(), "/p".to_string()),
            ("example.org".to_string(), "/*".to_string()),
            ("example.org".to_string(), "/auth".to_string()),
        ]
    );
    assert!(sim.phone_requests(5044).is_empty());
    // Every one was still acknowledged.
    assert_eq!(sim.phone_responses(5043).len(), 4);
}

// ---- large bodies through data_transfer ---------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Val {
    Num(u64),
    Bytes(Vec<u8>),
}

/// The fields of one protobuf message, in order (a reader independent of the
/// crate's).
fn fields(b: &[u8]) -> Vec<(u64, Val)> {
    fn varint(b: &[u8], at: &mut usize) -> u64 {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let byte = b[*at];
            *at += 1;
            v |= u64::from(byte & 0x7F) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                return v;
            }
        }
    }
    let mut out = Vec::new();
    let mut at = 0;
    while at < b.len() {
        let key = varint(b, &mut at);
        if key & 7 == 2 {
            let len = varint(b, &mut at) as usize;
            out.push((key >> 3, Val::Bytes(b[at..at + len].to_vec())));
            at += len;
        } else {
            assert_eq!(key & 7, 0, "only varints and byte strings");
            out.push((key >> 3, Val::Num(varint(b, &mut at))));
        }
    }
    out
}

fn bytes_of(fields: &[(u64, Val)], number: u64) -> Vec<u8> {
    fields
        .iter()
        .find_map(|(n, v)| match v {
            Val::Bytes(b) if *n == number => Some(b.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no field {number}"))
}

fn num_of(fields: &[(u64, Val)], number: u64) -> u64 {
    fields
        .iter()
        .find_map(|(n, v)| match v {
            Val::Num(x) if *n == number => Some(*x),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no field {number}"))
}

/// `Smart.7.2` (data_transfer_service.data_download_response): status, id,
/// offset and payload (empty when there is none).
fn download_response(smart: &[u8]) -> (u64, u64, u64, Vec<u8>) {
    let service = fields(&bytes_of(&fields(smart), 7));
    let resp = fields(&bytes_of(&service, 2));
    let payload = resp
        .iter()
        .find_map(|(n, v)| match v {
            Val::Bytes(b) if *n == 4 => Some(b.clone()),
            _ => None,
        })
        .unwrap_or_default();
    (
        num_of(&resp, 1),
        num_of(&resp, 2),
        num_of(&resp, 3),
        payload,
    )
}

/// The transfer id and size of a raw response that carries `xfer_data`
/// (`Smart.2.6.4`), after checking its status and that it has no inline body.
fn xfer_of(smart: &[u8]) -> (u32, u32) {
    let service = fields(&bytes_of(&fields(smart), 2));
    let resp = fields(&bytes_of(&service, 6));
    assert_eq!(num_of(&resp, 1), 100, "status OK");
    assert!(resp.iter().all(|(n, _)| *n != 3), "no resource_data");
    let item = fields(&bytes_of(&resp, 4));
    (num_of(&item, 1) as u32, num_of(&item, 2) as u32)
}

#[test]
fn a_use_data_xfer_reply_carries_an_id_and_the_watch_pulls_the_body() {
    let mut sim = sim_on();
    let tar = four_constellations();
    watch_request(&mut sim, 20, &raw_request(GOOD_URL, &[], true));
    sim.input(Input::GpsAssistData {
        id: 1,
        body: tar.clone(),
        etag: ETAG.into(),
    });
    // The first reply has the id and size, the headers, no body.
    let first = replies(&sim);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, 20);
    let (xfer_id, size) = xfer_of(&first[0].1);
    assert_eq!(size as usize, tar.len());
    let want_headers = concat(&[
        bytes_field(5, &header("etag", ETAG)),
        bytes_field(5, &header("cache-control", "max-age=14400")),
        bytes_field(5, &header("Content-Type", "application/octet-stream")),
    ]);
    assert!(first[0].1.ends_with(&want_headers));
    // Not sent yet: the watch has not pulled anything.
    assert!(done_events(&sim).is_empty());

    // The watch pulls 1000 bytes at a time.
    let mut got = Vec::new();
    let mut request_id = 100u16;
    while got.len() < tar.len() {
        let before = replies(&sim).len();
        watch_request(
            &mut sim,
            request_id,
            &download_request(u64::from(xfer_id), got.len() as u64, Some(1000)),
        );
        let all = replies(&sim);
        assert_eq!(all.len(), before + 1);
        let (rid, smart) = all.last().unwrap();
        assert_eq!(*rid, request_id);
        let (status, id, offset, payload) = download_response(smart);
        assert_eq!(
            (status, u64::from(xfer_id), got.len() as u64),
            (1, id, offset)
        );
        assert!(payload.len() <= 1000 && !payload.is_empty());
        got.extend_from_slice(&payload);
        request_id += 1;
        if got.len() < tar.len() {
            assert!(
                done_events(&sim).is_empty(),
                "not done before the last piece"
            );
        }
    }
    assert_eq!(got, tar);
    assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::Sent)]);
    // Asking again finds nothing: the transfer is forgotten.
    watch_request(
        &mut sim,
        request_id,
        &download_request(u64::from(xfer_id), 0, Some(10)),
    );
    let (rid, smart) = replies(&sim).pop().unwrap();
    assert_eq!(rid, request_id);
    assert_eq!(
        smart,
        smart_transfer(
            2,
            &concat(&[
                uint_field(1, 2),
                uint_field(2, u64::from(xfer_id)),
                uint_field(3, 0)
            ])
        )
    );
}

#[test]
fn a_pull_without_a_chunk_size_uses_4096_and_a_bad_offset_is_told() {
    let mut sim = sim_on();
    let tar = four_constellations();
    watch_request(&mut sim, 20, &raw_request(GOOD_URL, &[], true));
    sim.input(Input::GpsAssistData {
        id: 1,
        body: tar.clone(),
        etag: ETAG.into(),
    });
    let (xfer_id, _) = xfer_of(&replies(&sim)[0].1);
    watch_request(&mut sim, 21, &download_request(u64::from(xfer_id), 0, None));
    let (_, smart) = replies(&sim).pop().unwrap();
    let (status, _, _, payload) = download_response(&smart);
    assert_eq!((status, payload.len()), (1, 4096));
    // Past the end: INVALID_OFFSET (3).
    watch_request(
        &mut sim,
        22,
        &download_request(u64::from(xfer_id), tar.len() as u64, Some(10)),
    );
    let (_, smart) = replies(&sim).pop().unwrap();
    assert_eq!(download_response(&smart).0, 3);
    // An id that was never given: INVALID_ID (2).
    watch_request(&mut sim, 23, &download_request(999_999, 0, Some(10)));
    let (_, smart) = replies(&sim).pop().unwrap();
    assert_eq!(download_response(&smart).0, 2);
    assert!(done_events(&sim).is_empty());
}

#[test]
fn a_304_with_use_data_xfer_ends_at_once_with_an_empty_transfer() {
    let mut sim = sim_on();
    watch_request(
        &mut sim,
        20,
        &raw_request(GOOD_URL, &[("if-none-match", ETAG)], true),
    );
    sim.input(Input::GpsAssistData {
        id: 1,
        body: four_constellations(),
        etag: ETAG.into(),
    });
    let (_, size) = xfer_of(&replies(&sim)[0].1);
    assert_eq!(size, 0);
    assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::NotModified)]);
}

#[test]
fn a_file_access_request_is_refused_and_reported() {
    let mut sim = sim_on();
    watch_request(&mut sim, 20, &raw_request(GOOD_URL, &[], true));
    sim.input(Input::GpsAssistData {
        id: 1,
        body: four_constellations(),
        etag: ETAG.into(),
    });
    let (xfer_id, _) = xfer_of(&replies(&sim)[0].1);
    // file_access_item_request { data_xfer_id, expected_latency }
    watch_request(
        &mut sim,
        24,
        &smart_transfer(
            8,
            &concat(&[uint_field(1, u64::from(xfer_id)), uint_field(2, 0)]),
        ),
    );
    let (rid, smart) = replies(&sim).pop().unwrap();
    assert_eq!(rid, 24);
    // file_access_item_response { status UNKNOWN, data_xfer_id }
    assert_eq!(
        smart,
        smart_transfer(
            9,
            &concat(&[uint_field(1, 0), uint_field(2, u64::from(xfer_id))])
        )
    );
    assert_eq!(
        done_events(&sim),
        vec![(1, GpsAssistOutcome::NeedsFileAccess)]
    );
    // An id we never gave: INVALID_ID (2).
    watch_request(
        &mut sim,
        25,
        &smart_transfer(8, &concat(&[uint_field(1, 777_777), uint_field(2, 0)])),
    );
    let (_, smart) = replies(&sim).pop().unwrap();
    assert_eq!(
        smart,
        smart_transfer(9, &concat(&[uint_field(1, 2), uint_field(2, 777_777)]))
    );
}

// ---- chunked messages from the watch ----------------------------------------------------

fn chunk(request_id: u16, offset: usize, total: usize, bytes: &[u8]) -> Message {
    let mut p = request_id.to_le_bytes().to_vec();
    p.extend_from_slice(&(offset as u32).to_le_bytes());
    p.extend_from_slice(&(total as u32).to_le_bytes());
    p.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    p.extend_from_slice(bytes);
    Message::new(5043, p)
}

fn ack_bodies(sim: &Sim, request_id: u16) -> Vec<Vec<u8>> {
    sim.phone_responses(5043)
        .iter()
        .filter_map(|m| {
            let r = m.as_response()?;
            let b = r.body;
            (b.len() == 8 && b[..2] == request_id.to_le_bytes()).then(|| b.to_vec())
        })
        .collect()
}

#[test]
fn a_request_in_several_chunks_is_reassembled() {
    let mut sim = sim_on();
    // Long headers make the request larger than one chunk.
    let smart = raw_request(
        GOOD_URL,
        &[
            ("accept", "application/x-tar"),
            ("user-agent", &"x".repeat(300)),
            ("if-none-match", "\"old\""),
        ],
        false,
    );
    assert!(smart.len() > 200);
    let parts: Vec<&[u8]> = smart.chunks(150).collect();
    let mut offset = 0;
    for (i, part) in parts.iter().enumerate() {
        sim.watch_sends(&chunk(0x0BCD, offset, smart.len(), part));
        offset += part.len();
        if i + 1 < parts.len() {
            assert!(request_events(&sim).is_empty(), "not before the last chunk");
        }
    }
    assert_eq!(
        request_events(&sim),
        vec![(1, GOOD_PATH.to_string(), Some("\"old\"".to_string()))]
    );
    // Each chunk was acknowledged with its own offset, status KEPT, no error.
    let acks = ack_bodies(&sim, 0x0BCD);
    assert_eq!(acks.len(), parts.len());
    let mut want_offset = 0u32;
    for (ack, part) in acks.iter().zip(&parts) {
        assert_eq!(ack[2..6], want_offset.to_le_bytes());
        assert_eq!(ack[6..], [0, 0]);
        want_offset += part.len() as u32;
    }
}

#[test]
fn chunk_errors_are_reported_to_the_watch_with_the_status_codes() {
    let mut sim = sim_on();
    let smart = raw_request(GOOD_URL, &[("user-agent", &"x".repeat(300))], false);
    let total = smart.len();
    // A chunk for a message that never started: UNKNOWN_REQUEST_ID (100).
    sim.watch_sends(&chunk(30, 150, total, &smart[150..300]));
    assert_eq!(ack_bodies(&sim, 30)[0][6..], [1, 100]);
    // Start it, then skip ahead: MISSING_PACKET (102).
    sim.watch_sends(&chunk(31, 0, total, &smart[..150]));
    sim.watch_sends(&chunk(31, 200, total, &smart[200..300]));
    assert_eq!(ack_bodies(&sim, 31)[1][6..], [1, 102]);
    // The same chunk again: DUPLICATE_PACKET (101).
    sim.watch_sends(&chunk(31, 0, total, &smart[..150]));
    // (a new first chunk restarts the message, so this is acknowledged)
    assert_eq!(ack_bodies(&sim, 31)[2][6..], [0, 0]);
    sim.watch_sends(&chunk(31, 150, total, &smart[150..300]));
    sim.watch_sends(&chunk(31, 150, total, &smart[150..300]));
    assert_eq!(ack_bodies(&sim, 31).last().unwrap()[6..], [1, 101]);
    // More bytes than the total: EXCEEDED_TOTAL_PROTOBUF_LENGTH (103).
    sim.watch_sends(&chunk(32, 0, total, &smart[..150]));
    sim.watch_sends(&chunk(32, 150, 160, &smart[150..320.min(total)]));
    assert_eq!(ack_bodies(&sim, 32).last().unwrap()[6..], [1, 103]);
    // A message of 200 KiB is not reassembled: 103.
    sim.watch_sends(&chunk(33, 0, 200 * 1024, &[0; 100]));
    assert_eq!(ack_bodies(&sim, 33)[0][6..], [1, 103]);
    assert!(request_events(&sim).is_empty());
    assert_eq!(sim.client.phase(), Phase::Ready);
}

#[test]
fn the_watchs_own_answers_still_work_with_the_duty_on() {
    // The battery answer arrives as a 5044 and is read as before.
    let mut sim = sim_on();
    sim.input(Input::RequestBattery);
    assert!(sim.events.iter().any(|e| matches!(
        e,
        Event::Battery {
            level_percent: 87,
            ..
        }
    )));
    // And it is not taken for an HTTP request.
    assert!(refused_events(&sim).is_empty());
}

// ---- the watch not cooperating ------------------------------------------------------

#[test]
fn a_chunk_the_watch_refuses_ends_the_request_but_not_the_session() {
    let mut sim = sim_on();
    watch_request(&mut sim, 40, &raw_request(GOOD_URL, &[], false));
    sim.watch.protobuf_status = (1, 102);
    sim.input(Input::GpsAssistData {
        id: 1,
        body: four_constellations(),
        etag: ETAG.into(),
    });
    assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::Failed)]);
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::DutyMessageRefused,
        message_id: 5044
    }));
    // Only the first chunk was tried.
    assert_eq!(sim.phone_requests(5044).len(), 1);
    assert_eq!(sim.client.phase(), Phase::Ready);
    assert!(sim.closed_reason().is_none());
    // A NAK (status 1) does the same.
    sim.watch.protobuf_status = (0, 0);
    sim.watch.nak_duty = Some(5044);
    sim.events.clear();
    watch_request(&mut sim, 41, &raw_request(GOOD_URL, &[], false));
    sim.input(Input::GpsAssistData {
        id: 2,
        body: four_constellations(),
        etag: ETAG.into(),
    });
    assert_eq!(done_events(&sim), vec![(2, GpsAssistOutcome::Failed)]);
    assert_eq!(sim.client.phase(), Phase::Ready);
}

#[test]
fn a_silent_watch_drops_the_reply_after_thirty_seconds() {
    let mut sim = sim_on();
    watch_request(&mut sim, 40, &raw_request(GOOD_URL, &[], false));
    sim.watch.silent_duties = true;
    sim.input(Input::GpsAssistData {
        id: 1,
        body: four_constellations(),
        etag: ETAG.into(),
    });
    assert!(done_events(&sim).is_empty());
    sim.advance(31_000);
    assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::Failed)]);
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::DutyMessageRefused,
        message_id: 5044
    }));
    assert!(sim.closed_reason().is_none());
    assert_eq!(sim.client.phase(), Phase::Ready);
}

#[test]
fn a_late_answer_after_a_reconnect_changes_nothing() {
    let mut sim = sim_on();
    watch_request(&mut sim, 40, &raw_request(GOOD_URL, &[], false));
    sim.reconnect();
    // The new client has no such request and the duty is off in its config.
    sim.connect();
    let step = sim.input(Input::GpsAssistData {
        id: 1,
        body: four_constellations(),
        etag: ETAG.into(),
    });
    assert!(step.writes.is_empty());
}

#[test]
fn a_gps_reply_does_not_block_a_sync() {
    let mut sim = Sim::with(Client::new(duty_config(gps_on())), FakeWatch::new());
    sim.watch.add_file(fake_entry(1, 4, 0), lcg_bytes(77, 900));
    sim.handshake();
    watch_request(&mut sim, 40, &raw_request(GOOD_URL, &[], false));
    sim.input(Input::GpsAssistData {
        id: 1,
        body: four_constellations(),
        etag: ETAG.into(),
    });
    sim.input(Input::StartSync);
    assert_eq!(done_events(&sim), vec![(1, GpsAssistOutcome::Sent)]);
    assert_eq!(sim.downloaded().len(), 1);
    assert!(sim.closed_reason().is_none());
}
