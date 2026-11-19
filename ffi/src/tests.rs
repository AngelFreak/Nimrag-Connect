use nimrag_core::client as core;
use nimrag_core::multilink;

use crate::fixtures::ScriptedWatch;
use crate::*;

const CLIENT_ID: u64 = 0x1234_5678_9ABC_DEF0;

fn config() -> ClientConfig {
    ClientConfig {
        client_id: CLIENT_ID,
        connection_epoch: 7,
        host_name: "TestPhone".into(),
        host_manufacturer: "TestCo".into(),
        host_model: "T-1".into(),
        app_version: 1234,
        capabilities: vec![0x11, 0x22],
        first_connect: false,
    }
}

fn now(mono_ms: u64) -> Now {
    Now {
        mono_ms,
        unix_s: 1_800_000_000,
        utc_offset_s: 3600,
        dst: Some(DstPair {
            start_unix_s: 1_802_000_000,
            end_unix_s: 1_817_000_000,
        }),
    }
}

#[test]
fn config_converts_every_field() {
    let c: core::ClientConfig = config().into();
    assert_eq!(c.client_id, CLIENT_ID);
    assert_eq!(c.connection_epoch, 7);
    assert_eq!(c.host_name, "TestPhone");
    assert_eq!(c.host_manufacturer, "TestCo");
    assert_eq!(c.host_model, "T-1");
    assert_eq!(c.app_version, 1234);
    assert_eq!(c.capabilities, vec![0x11, 0x22]);
    assert!(!c.first_connect);
}

#[test]
fn now_converts_every_field() {
    let n: core::Now = now(5).into();
    assert_eq!(
        (n.mono_ms, n.unix_s, n.utc_offset_s),
        (5, 1_800_000_000, 3600)
    );
    let dst = n.dst.expect("dst kept");
    assert_eq!(
        (dst.start_unix_s, dst.end_unix_s),
        (1_802_000_000, 1_817_000_000)
    );
    let none: core::Now = Now {
        dst: None,
        ..now(5)
    }
    .into();
    assert_eq!(none.dst, None);
}

#[test]
fn every_input_variant_converts() {
    assert_eq!(
        core::Input::from(Input::Connected { att_mtu: 185 }),
        core::Input::Connected { att_mtu: 185 }
    );
    assert_eq!(
        core::Input::from(Input::Notification { value: vec![1, 2] }),
        core::Input::Notification { value: vec![1, 2] }
    );
    assert_eq!(core::Input::from(Input::Tick), core::Input::Tick);
    assert_eq!(
        core::Input::from(Input::RequestBattery),
        core::Input::RequestBattery
    );
    assert_eq!(core::Input::from(Input::StartSync), core::Input::StartSync);
    assert_eq!(
        core::Input::from(Input::FileStored { transfer_id: 9 }),
        core::Input::FileStored { transfer_id: 9 }
    );
    assert_eq!(
        core::Input::from(Input::FileStoreFailed { transfer_id: 9 }),
        core::Input::FileStoreFailed { transfer_id: 9 }
    );
    assert_eq!(
        core::Input::from(Input::Disconnected),
        core::Input::Disconnected
    );
}

#[test]
fn downloaded_event_keeps_entry_and_data() {
    let entry = nimrag_core::messages::DirEntry {
        index: 71,
        data_type: 128,
        sub_type: 32,
        file_number: 1071,
        specific_flags: 1,
        flags: 0xA0,
        size: 3,
        timestamp: 1_000_071,
    };
    let e: Event = core::Event::FileDownloaded {
        transfer_id: 42,
        entry,
        data: vec![7, 8, 9],
    }
    .into();
    assert_eq!(
        e,
        Event::FileDownloaded {
            transfer_id: 42,
            entry: DirEntry {
                index: 71,
                data_type: 128,
                sub_type: 32,
                file_number: 1071,
                specific_flags: 1,
                flags: 0xA0,
                size: 3,
                timestamp: 1_000_071,
            },
            data: vec![7, 8, 9],
        }
    );
}

#[test]
fn battery_and_file_available_variants_convert() {
    let e: Event = core::Event::Battery {
        level_percent: 87,
        report: core::BatteryReport::Status {
            status: 2,
            centivolts: 38,
        },
    }
    .into();
    assert_eq!(
        e,
        Event::Battery {
            level_percent: 87,
            report: BatteryReport::Status {
                status: 2,
                centivolts: 38
            }
        }
    );
    let e: Event = core::Event::Battery {
        level_percent: 5,
        report: core::BatteryReport::DeviceStatus {
            battery_status: Some(2),
            hours_remaining: None,
        },
    }
    .into();
    assert_eq!(
        e,
        Event::Battery {
            level_percent: 5,
            report: BatteryReport::DeviceStatus {
                battery_status: Some(2),
                hours_remaining: None
            }
        }
    );
    let e: Event = core::Event::SyncStarted {
        cycle: 3,
        trigger: core::SyncTrigger::FileAvailable,
    }
    .into();
    assert_eq!(
        e,
        Event::SyncStarted {
            cycle: 3,
            trigger: SyncTrigger::FileAvailable
        }
    );
    let e: Event = core::Event::Anomaly {
        kind: core::AnomalyKind::BatteryRequestRefused,
        message_id: 5043,
    }
    .into();
    assert_eq!(
        e,
        Event::Anomaly {
            kind: AnomalyKind::BatteryRequestRefused,
            message_id: 5043
        }
    );
}

#[test]
fn nested_enums_convert() {
    let e: Event = core::Event::Closed {
        reason: core::CloseReason::Timeout {
            phase: core::Phase::Handshake,
        },
    }
    .into();
    assert_eq!(
        e,
        Event::Closed {
            reason: CloseReason::Timeout {
                phase: Phase::Handshake
            }
        }
    );
    let e: Event = core::Event::FileFailed {
        entry: nimrag_core::messages::DirEntry {
            index: 1,
            data_type: 0,
            sub_type: 0,
            file_number: 0,
            specific_flags: 0,
            flags: 0,
            size: 0,
            timestamp: 0,
        },
        reason: core::FileFailure::Refused { status: 3 },
        will_retry: true,
    }
    .into();
    assert!(matches!(
        e,
        Event::FileFailed {
            reason: FileFailure::Refused { status: 3 },
            will_retry: true,
            ..
        }
    ));
}

#[test]
fn connected_writes_close_all_first() {
    let client = NimragClient::new(config());
    assert_eq!(client.phase(), Phase::Idle);
    let step = client.handle(now(0), Input::Connected { att_mtu: 23 });
    assert_eq!(step.writes[0], multilink::close_all_request(CLIENT_ID));
    assert_eq!(client.phase(), Phase::BringUp);
}

#[test]
fn stale_file_stored_gives_stale_confirmation() {
    // Above 2^53 and with the top bit set: must survive u64 -> Kotlin ULong intact.
    const ID: u64 = 0xFEDC_BA98_7654_3210;
    let client = NimragClient::new(config());
    client.handle(now(0), Input::Connected { att_mtu: 23 });
    let step = client.handle(now(1), Input::FileStored { transfer_id: ID });
    assert_eq!(
        step.events,
        vec![Event::StaleConfirmation { transfer_id: ID }]
    );
}

#[test]
fn small_mtu_closes_the_client() {
    let client = NimragClient::new(config());
    let step = client.handle(now(0), Input::Connected { att_mtu: 10 });
    assert_eq!(
        step.events,
        vec![Event::Closed {
            reason: CloseReason::MtuTooSmall
        }]
    );
    assert_eq!(client.phase(), Phase::Closed);
}

/// Drive the whole bring-up and handshake through the FFI object against the
/// scripted watch, the same way the Kotlin test does.
#[test]
fn scripted_watch_reaches_handshake_done() {
    let client = NimragClient::new(config());
    let watch = ScriptedWatch::new(23);
    let mut events = Vec::new();
    let mut pending = client.handle(now(0), Input::Connected { att_mtu: 23 });
    let mut t = 0;
    let mut sent_info = false;
    loop {
        events.extend(pending.events.clone());
        let mut inbox: Vec<Vec<u8>> = pending
            .writes
            .iter()
            .flat_map(|w| watch.on_write(t, w.clone()))
            .collect();
        if !sent_info {
            let info = watch.device_information(t);
            if !info.is_empty() {
                sent_info = true;
                inbox.extend(info);
            }
        }
        if inbox.is_empty() {
            break;
        }
        let mut next = Step {
            writes: Vec::new(),
            events: Vec::new(),
            wake_at_mono_ms: None,
        };
        for value in inbox {
            t += 1;
            let s = client.handle(now(t), Input::Notification { value });
            next.writes.extend(s.writes);
            next.events.extend(s.events);
        }
        pending = next;
    }
    assert_eq!(client.phase(), Phase::Ready, "events: {events:?}");
    assert!(events.iter().any(
        |e| matches!(e, Event::HandshakeDone { device, .. } if device.model_name == "Watch One")
    ));
}

#[test]
fn version_is_the_crate_version() {
    assert_eq!(ffi_version(), env!("CARGO_PKG_VERSION"));
}

/// With `NIMRAG_FIT_PROFILE=none` (what every Android build sets) the profile
/// is off. Compiled in only when the variable was `none` at build time.
#[test]
fn fit_profile_is_off_when_none() {
    if option_env!("NIMRAG_FIT_PROFILE") == Some("none") {
        assert!(!fit_profile_compiled_in());
    }
}

#[test]
fn poisoned_lock_fails_closed() {
    let client = NimragClient::new(config());
    client.handle(now(0), Input::Connected { att_mtu: 23 });
    client.poison();
    let step = client.handle(now(1), Input::Tick);
    assert!(step.writes.is_empty() && step.events.is_empty() && step.wake_at_mono_ms.is_none());
    assert_eq!(client.phase(), Phase::Closed);
}
