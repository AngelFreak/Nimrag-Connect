//! Stage 2f: the companion duties (notifications, find my phone / find my watch,
//! music control), driven only through `Client::handle` against the scripted
//! fake watch. Every frame here is synthetic (fake ids, fake texts); the layouts
//! come from Gadgetbridge and are labelled *inferred (Gadgetbridge)* in
//! docs/protocol/PROTOCOL.md until a real watch confirms them.

mod support;

use nimrag_core::client::{
    AnomalyKind, Client, Duties, Event, FindWatchState, Input, MusicCommand, MusicInfo, MusicState,
    NotificationAction, NotificationCategory, Phase, PhoneNotification,
};
use nimrag_core::crc;
use nimrag_core::gfdi::Message;
use nimrag_core::messages as m;
use support::*;

fn sim_with(duties: Duties) -> Sim {
    let mut sim = Sim::with(Client::new(duty_config(duties)), FakeWatch::new());
    sim.handshake();
    sim
}

fn notifications_only() -> Duties {
    Duties {
        notifications: true,
        ..Duties::default()
    }
}

fn music_only() -> Duties {
    Duties {
        music: true,
        ..Duties::default()
    }
}

fn find_only() -> Duties {
    Duties {
        find_phone: true,
        ..Duties::default()
    }
}

fn note(id: u32, body: &str) -> PhoneNotification {
    PhoneNotification {
        id,
        app_id: "org.example.chat".into(),
        app_name: "Example Chat".into(),
        category: NotificationCategory::Message,
        title: "Ann".into(),
        subtitle: String::new(),
        body: body.into(),
        when_unix_s: 1_800_000_000,
    }
}

fn subscribe(sim: &mut Sim) {
    sim.watch_sends(&Message::new(5036, vec![1, 0]));
}

fn post(sim: &mut Sim, n: PhoneNotification) {
    sim.input(Input::PostNotification { notification: n });
}

/// Payloads the phone sent with this id, in order.
fn sent(sim: &Sim, id: u16) -> Vec<Vec<u8>> {
    sim.phone_requests(id)
        .iter()
        .map(|m| m.payload.clone())
        .collect()
}

/// The 5000 payloads the phone answered the watch's `request_id` with.
fn answers(sim: &Sim, request_id: u16) -> Vec<Vec<u8>> {
    sim.phone_responses(request_id)
        .iter()
        .map(|m| m.payload.clone())
        .collect()
}

/// 5034 command 0 for `id`, asking for the given (code, optional u16 max) pairs.
fn get_attributes(id: u32, attrs: &[(u8, Option<u16>)]) -> Message {
    let mut p = vec![0];
    p.extend_from_slice(&id.to_le_bytes());
    for (code, max) in attrs {
        p.push(*code);
        if let Some(max) = max {
            p.extend_from_slice(&max.to_le_bytes());
            if *code == 127 {
                p.push(0);
            }
        }
    }
    Message::new(5034, p)
}

fn action(id: u32, code: u8) -> Message {
    let mut p = vec![128];
    p.extend_from_slice(&id.to_le_bytes());
    p.push(code);
    Message::new(5034, p)
}

// ---- duties off: the behaviour before 2f --------------------------------------------

#[test]
fn with_every_duty_off_the_watchs_messages_get_status_two() {
    let mut sim = Sim::new();
    sim.handshake();
    for id in [5034u16, 5036, 5039, 5040, 5041, 5042, 5049] {
        sim.watch_sends(&Message::new(id, vec![1]));
        let want = vec![(id & 0xFF) as u8, (id >> 8) as u8, 2];
        assert_eq!(answers(&sim, id), vec![want], "message {id}");
        assert!(sim.events.contains(&Event::Unsupported { message_id: id }));
    }
}

#[test]
fn with_every_duty_off_the_inputs_send_nothing() {
    let mut sim = Sim::new();
    sim.handshake();
    let before = sim.phone_writes.len();
    post(&mut sim, note(1, "hi"));
    sim.input(Input::RemoveNotification { id: 1 });
    sim.input(Input::MusicInfo {
        info: MusicInfo {
            artist: "A".into(),
            album: "B".into(),
            title: "C".into(),
            duration_s: 9,
        },
    });
    sim.input(Input::MusicState {
        state: MusicState {
            playing: true,
            rate_percent: 100,
            position_s: 1,
        },
    });
    sim.input(Input::MusicVolume { percent: 50 });
    assert_eq!(sim.phone_writes.len(), before);
}

#[test]
fn a_duty_that_is_on_leaves_the_others_unsupported() {
    let mut sim = sim_with(find_only());
    sim.watch_sends(&Message::new(5036, vec![1, 0]));
    sim.watch_sends(&Message::new(5042, vec![]));
    assert_eq!(answers(&sim, 5036), vec![vec![0xAC, 0x13, 2]]);
    assert_eq!(answers(&sim, 5042), vec![vec![0xB2, 0x13, 2]]);
}

// ---- notifications --------------------------------------------------------------

#[test]
fn subscription_is_answered_and_reported() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    // 5000 body: id 5036, ACK, "enabled" 0, the enable value echoed, 0
    assert_eq!(answers(&sim, 5036), vec![vec![0xAC, 0x13, 0, 0, 1, 0]]);
    assert!(sim
        .events
        .contains(&Event::NotificationSubscription { enabled: true }));
}

#[test]
fn notifications_are_dropped_until_the_watch_subscribes() {
    let mut sim = sim_with(notifications_only());
    post(&mut sim, note(1, "hi"));
    assert!(sent(&sim, 5033).is_empty());
    subscribe(&mut sim);
    post(&mut sim, note(1, "hi"));
    assert_eq!(sent(&sim, 5033).len(), 1);
}

#[test]
fn a_posted_notification_is_announced_byte_exact() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    post(&mut sim, note(0x0102_0304, "hi"));
    // ADD, flags FOREGROUND|DECLINE, category 12 (message), count 1, id LE, NEW_ACTIONS
    assert_eq!(
        sent(&sim, 5033),
        vec![vec![0, 0x12, 12, 1, 4, 3, 2, 1, 0x02]]
    );
    // the same id again is a MODIFY and the count stays 1
    post(&mut sim, note(0x0102_0304, "hi again"));
    assert_eq!(sent(&sim, 5033)[1], vec![1, 0x12, 12, 1, 4, 3, 2, 1, 0x02]);
    // another one of the same category counts 2; a different category counts 1
    post(&mut sim, note(7, "x"));
    assert_eq!(sent(&sim, 5033)[2][3], 2);
    let mut mail = note(8, "x");
    mail.category = NotificationCategory::Email;
    post(&mut sim, mail);
    assert_eq!(&sent(&sim, 5033)[3][..4], &[0, 0x12, 6, 1]);
}

#[test]
fn removal_is_announced_also_for_an_unknown_id() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    post(&mut sim, note(5, "hi"));
    sim.input(Input::RemoveNotification { id: 5 });
    sim.input(Input::RemoveNotification { id: 99 });
    let updates = sent(&sim, 5033);
    // REMOVE, same category as announced, count 0 left, no actions
    assert_eq!(updates[1], vec![2, 0x12, 12, 0, 5, 0, 0, 0, 0]);
    // unknown id: category OTHER, count 0
    assert_eq!(updates[2], vec![2, 0x12, 0, 0, 99, 0, 0, 0, 0]);
}

#[test]
fn the_oldest_notification_is_removed_when_more_than_64_are_known() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    for id in 1..=65u32 {
        post(&mut sim, note(id, "x"));
    }
    let updates = sent(&sim, 5033);
    assert_eq!(updates.len(), 66);
    // after the 65th ADD comes a REMOVE of id 1 with 64 left
    assert_eq!(updates[64][0], 0);
    assert_eq!(updates[65], vec![2, 0x12, 12, 64, 1, 0, 0, 0, 0]);
}

#[test]
fn the_watch_asks_for_attributes_and_gets_them_byte_exact() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    post(&mut sim, note(0x0102_0304, "Hello there"));
    // app id, title (max 100), message (max 5), message size, date
    sim.watch_sends(&get_attributes(
        0x0102_0304,
        &[
            (0, None),
            (1, Some(100)),
            (3, Some(5)),
            (4, None),
            (5, None),
        ],
    ));
    // ACK of 5034: id 5034, ACK, chunk status OK, status code NO_ERROR
    assert_eq!(answers(&sim, 5034), vec![vec![0xAA, 0x13, 0, 0, 0]]);

    let mut data = vec![0, 4, 3, 2, 1];
    data.extend_from_slice(&[0, 16, 0]);
    data.extend_from_slice(b"org.example.chat");
    data.extend_from_slice(&[1, 3, 0]);
    data.extend_from_slice(b"Ann");
    data.extend_from_slice(&[3, 5, 0]);
    data.extend_from_slice(b"Hello");
    data.extend_from_slice(&[5, 15, 0]);
    // 1_800_000_000 is 2027-01-15 08:00:00 UTC; the test phone is at +01:00
    data.extend_from_slice(b"20270115T090000");
    data.extend_from_slice(&[4, 2, 0]);
    data.extend_from_slice(b"11");

    let mut want = Vec::new();
    want.extend_from_slice(&(data.len() as u16).to_le_bytes());
    want.extend_from_slice(&crc::checksum(&data).to_le_bytes());
    want.extend_from_slice(&0u16.to_le_bytes());
    want.extend_from_slice(&data);
    assert_eq!(sent(&sim, 5035), vec![want]);
    // the ACK went out before the data
    let ids = sim.phone_ids();
    let ack = ids.iter().rposition(|&i| i == 5000).unwrap();
    let chunk = ids.iter().position(|&i| i == 5035).unwrap();
    assert!(ack < chunk);
}

#[test]
fn a_long_answer_goes_out_in_chunks_each_after_the_watchs_ok() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    post(&mut sim, note(1, &"word ".repeat(200))); // 1000 bytes of body
    sim.watch.silent_duties = true; // the test answers the chunks itself
    sim.watch_sends(&get_attributes(1, &[(3, Some(0))]));
    let first = sim.phone_requests(5035).len();
    assert_eq!(first, 1, "only the first chunk goes out before the answer");

    let mut all = Vec::new();
    let mut offsets = Vec::new();
    let mut running = 0u16;
    loop {
        let chunks = sim.phone_requests(5035);
        let chunk = (*chunks.last().unwrap()).clone();
        let p = &chunk.payload;
        let total = u16::from_le_bytes([p[0], p[1]]) as usize;
        let body = &p[6..];
        running = crc::update(running, body);
        assert_eq!(u16::from_le_bytes([p[2], p[3]]), running, "running CRC");
        offsets.push(u16::from_le_bytes([p[4], p[5]]));
        assert!(body.len() <= 300);
        all.extend_from_slice(body);
        if all.len() >= total {
            break;
        }
        let count = chunks.len();
        sim.watch_sends(&watch_response(&chunk, 0, &[0]));
        assert_eq!(sim.phone_requests(5035).len(), count + 1);
    }
    assert_eq!(offsets, vec![0, 300, 600, 900]);
    // the last answer ends the transfer; nothing more is sent
    let last = (*sim.phone_requests(5035).last().unwrap()).clone();
    let writes = sim.phone_writes.len();
    sim.watch_sends(&watch_response(&last, 0, &[0]));
    assert_eq!(sim.phone_writes.len(), writes);
    assert_eq!(&all[..5], &[0, 1, 0, 0, 0]);
}

#[test]
fn a_transfer_the_watch_cannot_go_on_with_is_given_up() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    post(&mut sim, note(1, &"x".repeat(1000)));
    sim.watch.data_status = 2; // ABORT
    sim.watch_sends(&get_attributes(1, &[(3, Some(0))]));
    assert_eq!(sim.phone_requests(5035).len(), 1, "no second chunk");
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::DutyMessageRefused,
        message_id: 5035
    }));
    assert_eq!(sim.client.phase(), Phase::Ready);
}

#[test]
fn a_second_request_waits_for_the_first_transfer() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    post(&mut sim, note(1, &"a".repeat(700)));
    post(&mut sim, note(2, "short"));
    sim.watch.silent_duties = true;
    sim.watch_sends(&get_attributes(1, &[(3, Some(0))]));
    sim.watch_sends(&get_attributes(2, &[(1, Some(0))]));
    let chunks = sim.phone_requests(5035);
    assert_eq!(chunks.len(), 1, "one chunk on the wire at a time");
    // answer the first chunk: the second chunk of notification 1 follows, not notification 2
    let c0 = (*chunks.last().unwrap()).clone();
    sim.watch_sends(&watch_response(&c0, 0, &[0]));
    let chunks = sim.phone_requests(5035);
    assert_eq!(chunks.len(), 2);
    assert_eq!(
        u16::from_le_bytes([chunks[1].payload[4], chunks[1].payload[5]]),
        300
    );
}

#[test]
fn app_attributes_use_the_name_the_host_gave() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    post(&mut sim, note(1, "hi"));
    let mut p = vec![1];
    p.extend_from_slice(b"org.example.chat\0");
    p.push(0);
    sim.watch_sends(&Message::new(5034, p));
    let mut data = vec![1];
    data.extend_from_slice(b"org.example.chat");
    data.extend_from_slice(&[0, 0, 12, 0]);
    data.extend_from_slice(b"Example Chat");
    let chunk = &sent(&sim, 5035)[0];
    assert_eq!(&chunk[6..], &data[..]);

    // an app we know nothing about is called by its identifier
    let mut q = vec![1];
    q.extend_from_slice(b"org.other\0");
    q.push(0);
    sim.watch_sends(&Message::new(5034, q));
    let chunk = &sent(&sim, 5035)[1];
    assert_eq!(
        &chunk[6..],
        &[&[1u8][..], b"org.other", &[0, 0, 9, 0], b"org.other"].concat()[..]
    );
}

#[test]
fn actions_on_the_watch_come_back_as_events() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    post(&mut sim, note(9, "hi"));
    sim.watch_sends(&action(9, 98));
    sim.watch_sends(&action(9, 99));
    sim.watch_sends(&action(9, 97));
    let mut reply = action(9, 95);
    reply.payload.extend_from_slice(b"On my way\0");
    sim.watch_sends(&reply);
    sim.watch_sends(&action(9, 2)); // custom action 2 = index 1
    let got: Vec<_> = sim
        .events
        .iter()
        .filter_map(|e| match e {
            Event::NotificationAction { id, action } => Some((*id, action.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (9, NotificationAction::Dismiss),
            (9, NotificationAction::Block),
            (9, NotificationAction::RejectCall),
            (
                9,
                NotificationAction::Reply {
                    text: "On my way".into()
                }
            ),
            (9, NotificationAction::Custom { index: 1 }),
        ]
    );
    // every one was acknowledged
    assert_eq!(answers(&sim, 5034).len(), 5);
    assert!(answers(&sim, 5034)
        .iter()
        .all(|a| a == &vec![0xAA, 0x13, 0, 0, 0]));
}

#[test]
fn an_action_for_an_unknown_notification_is_acknowledged_but_not_reported() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    sim.watch_sends(&action(404, 98));
    assert_eq!(answers(&sim, 5034).len(), 1);
    assert!(sim
        .events_of(|e| matches!(e, Event::NotificationAction { .. }))
        .is_empty());
}

#[test]
fn control_messages_before_the_subscription_are_acknowledged_and_ignored() {
    let mut sim = sim_with(notifications_only());
    sim.watch_sends(&action(1, 98));
    sim.watch_sends(&get_attributes(1, &[(1, Some(10))]));
    assert_eq!(answers(&sim, 5034).len(), 2);
    assert!(sent(&sim, 5035).is_empty());
    assert!(sim
        .events_of(|e| matches!(e, Event::NotificationAction { .. }))
        .is_empty());
}

#[test]
fn unsubscribing_forgets_everything_and_stops_updates() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    post(&mut sim, note(1, "hi"));
    sim.watch_sends(&Message::new(5036, vec![0]));
    assert!(sim
        .events
        .contains(&Event::NotificationSubscription { enabled: false }));
    post(&mut sim, note(2, "hi"));
    assert_eq!(sent(&sim, 5033).len(), 1);
    // and the old notification is no longer known
    subscribe(&mut sim);
    sim.watch_sends(&action(1, 98));
    assert!(sim
        .events_of(|e| matches!(e, Event::NotificationAction { .. }))
        .is_empty());
}

#[test]
fn unknown_and_malformed_control_messages() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    // unknown command: status 2 and an Unsupported event
    sim.watch_sends(&Message::new(5034, vec![77, 0, 0, 0, 0]));
    assert_eq!(answers(&sim, 5034), vec![vec![0xAA, 0x13, 2]]);
    assert!(sim
        .events
        .contains(&Event::Unsupported { message_id: 5034 }));
    // an attribute we do not know cannot be skipped: also status 2
    sim.watch_sends(&Message::new(5034, vec![0, 1, 0, 0, 0, 9]));
    assert_eq!(answers(&sim, 5034).len(), 2);
    // truncated: an anomaly and no answer
    let count = answers(&sim, 5034).len();
    sim.watch_sends(&Message::new(5034, vec![0, 1]));
    assert_eq!(answers(&sim, 5034).len(), count);
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::BadFrame,
        message_id: 5034
    }));
    // an empty subscription
    sim.watch_sends(&Message::new(5036, vec![]));
    assert_eq!(answers(&sim, 5036).len(), 1);
}

#[test]
fn a_watch_that_refuses_an_update_does_not_end_the_session() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    sim.watch.nak_duty = Some(5033);
    post(&mut sim, note(1, "hi"));
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::DutyMessageRefused,
        message_id: 5033
    }));
    assert_eq!(sim.client.phase(), Phase::Ready);
    assert!(sim.closed_reason().is_none());
}

#[test]
fn a_silent_watch_costs_one_anomaly_and_the_next_update_still_goes_out() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    sim.watch.silent_duties = true;
    post(&mut sim, note(1, "hi"));
    post(&mut sim, note(2, "hi"));
    assert_eq!(
        sent(&sim, 5033).len(),
        1,
        "the second waits behind the first"
    );
    sim.advance(31_000);
    assert!(sim.closed_reason().is_none());
    assert!(sim.events.contains(&Event::Anomaly {
        kind: AnomalyKind::DutyMessageRefused,
        message_id: 5033
    }));
    // the first was dropped, so the second went out
    assert_eq!(sent(&sim, 5033).len(), 2);
}

#[test]
fn notifications_before_the_handshake_are_ignored() {
    let mut sim = Sim::with(
        Client::new(duty_config(notifications_only())),
        FakeWatch::new(),
    );
    sim.connect();
    post(&mut sim, note(1, "hi"));
    assert!(sent(&sim, 5033).is_empty());
}

#[test]
fn a_duty_message_does_not_block_the_sync() {
    let mut sim = Sim::with(
        Client::new(duty_config(notifications_only())),
        FakeWatch::new(),
    );
    let entry = fake_entry(1, 4, 0);
    sim.watch.add_file(entry, lcg_bytes(3, 900));
    sim.handshake();
    subscribe(&mut sim);
    post(&mut sim, note(1, "hi"));
    let step = sim.input(Input::StartSync);
    let _ = step;
    assert_eq!(sim.downloaded().len(), 1);
    assert!(sim.closed_reason().is_none());
}

// ---- find my phone / find my watch ---------------------------------------------------

#[test]
fn find_my_phone_request_and_cancel_become_events() {
    let mut sim = sim_with(find_only());
    sim.watch_sends(&Message::new(5039, vec![30]));
    assert!(sim
        .events
        .contains(&Event::FindPhoneStart { duration_s: 30 }));
    assert_eq!(answers(&sim, 5039), vec![vec![0xAF, 0x13, 0]]);
    sim.watch_sends(&Message::new(5040, vec![]));
    assert!(sim.events.contains(&Event::FindPhoneStop));
    assert_eq!(answers(&sim, 5040), vec![vec![0xB0, 0x13, 0]]);
    // an empty 5039 is malformed
    sim.watch_sends(&Message::new(5039, vec![]));
    assert_eq!(answers(&sim, 5039).len(), 1);
}

#[test]
fn find_my_watch_sends_the_protobuf_and_reports_the_answers() {
    let mut sim = sim_with(Duties::default());
    sim.watch.find_watch_response = Some(vec![0x62, 0x04, 0x12, 0x02, 0x08, 100]);
    sim.input(Input::FindWatch { start: true });
    let req = sim.phone_requests(5043).last().unwrap().payload.clone();
    let (_, body) = m::parse_protobuf(&req).unwrap();
    assert_eq!(body, [0x62, 0x04, 0x0A, 0x02, 0x08, 0x3C]);
    assert!(sim.events.contains(&Event::FindWatch {
        state: FindWatchState::Ringing
    }));

    sim.watch.find_watch_response = Some(vec![0x62, 0x04, 0x22, 0x02, 0x08, 100]);
    sim.input(Input::FindWatch { start: false });
    let req = sim.phone_requests(5043).last().unwrap().payload.clone();
    let (_, body) = m::parse_protobuf(&req).unwrap();
    assert_eq!(body, [0x62, 0x02, 0x1A, 0x00]);
    assert!(sim.events.contains(&Event::FindWatch {
        state: FindWatchState::Stopped
    }));
}

#[test]
fn find_my_watch_without_the_capability_sends_nothing() {
    let mut watch = FakeWatch::new();
    watch.capabilities = vec![0xAA, 0x00]; // bit 9 clear
    let mut sim = Sim::with(Client::new(duty_config(Duties::default())), watch);
    sim.handshake();
    let before = sim.phone_requests(5043).len();
    sim.input(Input::FindWatch { start: true });
    assert_eq!(sim.phone_requests(5043).len(), before);
    assert!(sim.events.contains(&Event::FindWatch {
        state: FindWatchState::NotSupported
    }));
}

#[test]
fn find_my_watch_failures_are_events_not_the_end_of_the_session() {
    let mut sim = sim_with(Duties::default());
    sim.watch.nak_duty = Some(5043);
    sim.input(Input::FindWatch { start: true });
    assert!(sim.events.contains(&Event::FindWatch {
        state: FindWatchState::Failed
    }));
    assert_eq!(sim.client.phase(), Phase::Ready);

    // an ERROR status in the watch's answer
    sim.watch.nak_duty = None;
    sim.watch.find_watch_response = Some(vec![0x62, 0x05, 0x12, 0x03, 0x08, 0xC8, 0x01]);
    sim.events.clear();
    sim.input(Input::FindWatch { start: true });
    assert!(sim.events.contains(&Event::FindWatch {
        state: FindWatchState::Failed
    }));

    // silence for 30 s
    sim.watch.silent_duties = true;
    sim.events.clear();
    sim.input(Input::FindWatch { start: true });
    sim.advance(31_000);
    assert!(sim.events.contains(&Event::FindWatch {
        state: FindWatchState::Failed
    }));
    assert!(sim.closed_reason().is_none());
}

#[test]
fn the_wearer_cancelling_on_the_watch_is_reported() {
    let mut sim = sim_with(Duties::default());
    let body = [0x62, 0x02, 0x1A, 0x00];
    sim.watch_sends(&Message::new(5043, m::build_protobuf(40, &body)));
    assert!(sim.events.contains(&Event::FindWatch {
        state: FindWatchState::FoundOnWatch
    }));
    // acknowledged like every watch protobuf request
    assert_eq!(answers(&sim, 5043).len(), 1);
}

// ---- music ------------------------------------------------------------------------

#[test]
fn music_capabilities_are_answered_and_the_host_is_told_to_push() {
    let mut sim = sim_with(music_only());
    sim.watch_sends(&Message::new(5042, vec![]));
    assert_eq!(
        answers(&sim, 5042),
        vec![vec![0xB2, 0x13, 0, 7, 0, 1, 2, 3, 4, 5, 6]]
    );
    assert!(sim.events.contains(&Event::MusicRequested));
    // some watches send a capabilities byte
    sim.watch_sends(&Message::new(5042, vec![0x1F]));
    assert_eq!(answers(&sim, 5042).len(), 2);
}

#[test]
fn watch_buttons_become_commands() {
    let mut sim = sim_with(music_only());
    for (code, want) in [
        (0u8, MusicCommand::TogglePlayPause),
        (1, MusicCommand::Next),
        (2, MusicCommand::Previous),
        (3, MusicCommand::VolumeUp),
        (4, MusicCommand::VolumeDown),
        (5, MusicCommand::Play),
        (6, MusicCommand::Pause),
    ] {
        sim.watch_sends(&Message::new(5041, vec![code]));
        assert!(
            sim.events.contains(&Event::MusicCommand { command: want }),
            "command {code}"
        );
    }
    assert_eq!(answers(&sim, 5041).len(), 7);
    assert!(answers(&sim, 5041)
        .iter()
        .all(|a| a == &vec![0xB1, 0x13, 0]));
    // a command we do not know: status 2
    sim.watch_sends(&Message::new(5041, vec![99]));
    assert_eq!(answers(&sim, 5041).last().unwrap(), &vec![0xB1, 0x13, 2]);
    assert!(sim
        .events
        .contains(&Event::Unsupported { message_id: 5041 }));
}

#[test]
fn now_playing_goes_to_the_watch_byte_exact() {
    let mut sim = sim_with(music_only());
    sim.input(Input::MusicInfo {
        info: MusicInfo {
            artist: "Ann".into(),
            album: "Al".into(),
            title: "Ti".into(),
            duration_s: 215,
        },
    });
    // artist, album, title, duration: len + 3, entity 2, attribute, 0, value
    assert_eq!(
        sent(&sim, 5049)[0],
        vec![
            6, 2, 0, 0, b'A', b'n', b'n', //
            5, 2, 1, 0, b'A', b'l', //
            5, 2, 2, 0, b'T', b'i', //
            6, 2, 3, 0, b'2', b'1', b'5',
        ]
    );
    sim.input(Input::MusicState {
        state: MusicState {
            playing: true,
            rate_percent: 100,
            position_s: 42,
        },
    });
    let mut want = vec![15, 0, 1, 0];
    want.extend_from_slice(b"1,1.0,42.000");
    assert_eq!(sent(&sim, 5049)[1], want);
    sim.input(Input::MusicVolume { percent: 50 });
    let mut want = vec![7, 0, 2, 0];
    want.extend_from_slice(b"0.50");
    assert_eq!(sent(&sim, 5049)[2], want);
}

#[test]
fn a_newer_state_replaces_one_that_still_waits() {
    let mut sim = sim_with(music_only());
    sim.watch.silent_duties = true; // the watch does not answer, so updates queue up
    let state = |position_s| MusicState {
        playing: true,
        rate_percent: 100,
        position_s,
    };
    sim.input(Input::MusicState { state: state(1) });
    sim.input(Input::MusicState { state: state(2) });
    sim.input(Input::MusicState { state: state(3) });
    assert_eq!(sent(&sim, 5049).len(), 1, "one on the wire");
    let first = (*sim.phone_requests(5049).last().unwrap()).clone();
    sim.watch_sends(&watch_response(&first, 0, &[]));
    let all = sent(&sim, 5049);
    assert_eq!(all.len(), 2);
    assert!(
        String::from_utf8_lossy(&all[1]).contains("3.000"),
        "the newest state won"
    );
}

#[test]
fn the_watch_asking_for_attributes_is_acknowledged() {
    let mut sim = sim_with(music_only());
    sim.watch_sends(&Message::new(5049, vec![3, 2, 0, 1, 1, 0]));
    assert_eq!(answers(&sim, 5049), vec![vec![0xB9, 0x13, 0]]);
}

// ---- all duties, hostile input --------------------------------------------------------

#[test]
fn random_bytes_on_every_duty_message_never_panic_or_end_the_session() {
    let mut sim = sim_with(all_duties());
    subscribe(&mut sim);
    post(&mut sim, note(1, "hi"));
    let ids = [5034u16, 5036, 5039, 5040, 5041, 5042, 5049];
    for i in 0..2_000u32 {
        let id = ids[(i as usize) % ids.len()];
        let len = (i % 23) as usize;
        let payload = lcg_bytes(i + 1, len);
        sim.watch_sends(&Message::new(id, payload));
    }
    assert!(sim.closed_reason().is_none(), "{:?}", sim.closed_reason());
}

#[test]
fn debug_output_of_events_and_inputs_hides_content() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    post(&mut sim, note(9, "very private text"));
    let mut reply = action(9, 95);
    reply.payload.extend_from_slice(b"secret reply\0");
    sim.watch_sends(&reply);
    let shown = format!("{:?}", sim.events);
    assert!(!shown.contains("secret reply"));
    let input = Input::PostNotification {
        notification: note(9, "very private text"),
    };
    let shown = format!("{input:?}");
    assert!(!shown.contains("private") && !shown.contains("Ann"));
    let info = Input::MusicInfo {
        info: MusicInfo {
            artist: "Hidden Artist".into(),
            album: String::new(),
            title: "Hidden Title".into(),
            duration_s: 1,
        },
    };
    assert!(!format!("{info:?}").contains("Hidden"));
}

#[test]
fn a_new_connection_starts_with_nothing_known() {
    let mut sim = sim_with(notifications_only());
    subscribe(&mut sim);
    post(&mut sim, note(1, "hi"));
    sim.reconnect();
    sim.client = Client::new(duty_config(notifications_only()));
    sim.handshake();
    // not subscribed on the new link: posting does nothing, an action is not reported
    post(&mut sim, note(2, "hi"));
    assert_eq!(sent(&sim, 5033).len(), 0);
}
