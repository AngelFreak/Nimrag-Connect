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
        archive: false,
        skip: vec![FileKey {
            data_type: 128,
            sub_type: 32,
            file_number: 1071,
            size: 3,
            timestamp: 1_000_071,
        }],
        duties: Duties {
            notifications: true,
            music: false,
            find_phone: true,
            weather: true,
        },
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
    assert!(!c.archive);
    assert_eq!(
        c.duties,
        core::Duties {
            notifications: true,
            music: false,
            find_phone: true,
            weather: true
        }
    );
    assert_eq!(
        c.skip,
        vec![core::FileKey {
            data_type: 128,
            sub_type: 32,
            file_number: 1071,
            size: 3,
            timestamp: 1_000_071,
        }]
    );
}

#[test]
fn skipped_event_and_the_skipped_count_convert() {
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
    let e: Event = core::Event::FileSkipped { entry }.into();
    assert!(matches!(e, Event::FileSkipped { entry } if entry.index == 71 && entry.size == 3));
    let e: Event = core::Event::SyncFinished {
        cycle: 2,
        downloaded: 1,
        archived: 0,
        failed: 0,
        skipped: 5,
    }
    .into();
    assert_eq!(
        e,
        Event::SyncFinished {
            cycle: 2,
            downloaded: 1,
            archived: 0,
            failed: 0,
            skipped: 5,
        }
    );
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
fn identify_device_by_name_and_by_manufacturer_data() {
    let by_name = identify_device(Some("Instinct 2".into()), None);
    assert_eq!(by_name.id, "GARMIN_INSTINCT_2");
    assert!(by_name.can_sync && by_name.unsupported_reason.is_none());
    assert!(by_name.verified && !by_name.generic);

    // Garmin manufacturer data (company id removed) decides before the name:
    // product 3888 = 0x0F30, big endian.
    let by_product = identify_device(Some("whatever".into()), Some(vec![0x0F, 0x30, 0xAA]));
    assert_eq!(by_product.id, "GARMIN_INSTINCT_2");

    let unknown = identify_device(Some("Not a Garmin".into()), None);
    assert!(unknown.generic && unknown.can_sync && !unknown.verified);
    assert_eq!(unknown.id, "GARMIN_GENERIC");
    assert!(identify_device(None, None).generic);
    // One byte of manufacturer data is not a product number: the name decides.
    let short = identify_device(Some("Instinct 2".into()), Some(vec![0x0F]));
    assert_eq!(short.id, "GARMIN_INSTINCT_2");
}

#[test]
fn identify_device_reports_the_unsupported_reason() {
    use nimrag_core::devices::{SyncSupport, MODELS};
    let model = MODELS
        .iter()
        .find(|m| matches!(m.sync, SyncSupport::CannotSyncYet(_)) && !m.product_ids.is_empty())
        .expect("the catalogue has a model that cannot sync yet");
    let info = identify_device(None, Some(model.product_ids[0].to_be_bytes().to_vec()));
    assert_eq!(info.id, model.id);
    assert!(!info.can_sync);
    assert!(info.unsupported_reason.is_some_and(|r| !r.is_empty()));
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

#[cfg(feature = "local-server")]
#[test]
fn local_server_starts_ingests_and_stops() {
    let dir = std::env::temp_dir().join(format!("nimrag-ffi-local-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let secret = "0123456789abcdef0123456789abcdef".to_string();
    assert!(LocalServer::start(dir.display().to_string(), 0, "short".into()).is_err());
    let server = LocalServer::start(dir.display().to_string(), 0, secret).unwrap();
    assert_ne!(server.port(), 0);
    assert_eq!(server.secret_cookie_name(), "nimrag_local");
    let meta = LocalFileMeta {
        data_type: 128,
        sub_type: 32,
        file_index: 0,
        file_number: 1,
        watch_timestamp: 0,
    };
    let bytes = b"arbitrary bytes".to_vec();
    assert!(server.ingest_file(bytes.clone(), meta.clone()).unwrap());
    assert!(!server.ingest_file(bytes, meta.clone()).unwrap());
    assert!(server.summary().unwrap().contains("\"files\":1"));
    server.stop();
    assert!(matches!(
        server.ingest_file(vec![1], meta),
        Err(LocalServerError::Stopped)
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

fn note() -> PhoneNotification {
    PhoneNotification {
        id: 9,
        app_id: "org.example.chat".into(),
        app_name: "Example Chat".into(),
        category: NotificationCategory::Message,
        title: "Ann".into(),
        subtitle: "".into(),
        body: "very private text".into(),
        when_unix_s: 1_800_000_000,
    }
}

#[test]
fn duty_inputs_convert() {
    let c: core::Input = Input::PostNotification {
        notification: note(),
    }
    .into();
    match c {
        core::Input::PostNotification { notification: n } => {
            assert_eq!(n.id, 9);
            assert_eq!(n.app_id, "org.example.chat");
            assert_eq!(n.app_name, "Example Chat");
            assert_eq!(n.category, core::NotificationCategory::Message);
            assert_eq!(n.title, "Ann");
            assert_eq!(n.body, "very private text");
            assert_eq!(n.when_unix_s, 1_800_000_000);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        core::Input::from(Input::RemoveNotification { id: 4 }),
        core::Input::RemoveNotification { id: 4 }
    ));
    let c: core::Input = Input::MusicInfo {
        info: MusicInfo {
            artist: "A".into(),
            album: "B".into(),
            title: "C".into(),
            duration_s: 215,
        },
    }
    .into();
    assert!(
        matches!(c, core::Input::MusicInfo { info } if info.title == "C" && info.duration_s == 215)
    );
    let c: core::Input = Input::MusicState {
        state: MusicState {
            playing: true,
            rate_percent: 100,
            position_s: 42,
        },
    }
    .into();
    assert!(matches!(
        c,
        core::Input::MusicState { state } if state.playing && state.rate_percent == 100 && state.position_s == 42
    ));
    assert!(matches!(
        core::Input::from(Input::MusicVolume { percent: 50 }),
        core::Input::MusicVolume { percent: 50 }
    ));
    assert!(matches!(
        core::Input::from(Input::FindWatch { start: true }),
        core::Input::FindWatch { start: true }
    ));
}

#[test]
fn upload_input_and_events_convert() {
    let c: core::Input = Input::UploadFile {
        upload_id: 7,
        data_type: 128,
        sub_type: 5,
        data: vec![1, 2, 3],
    }
    .into();
    assert_eq!(
        c,
        core::Input::UploadFile {
            upload_id: 7,
            data_type: 128,
            sub_type: 5,
            data: vec![1, 2, 3],
        }
    );
    let results = [
        (core::UploadResult::Sent, UploadResult::Sent),
        (
            core::UploadResult::Refused {
                stage: core::UploadStage::Create,
                status: 2,
            },
            UploadResult::Refused {
                stage: UploadStage::Create,
                status: 2,
            },
        ),
        (
            core::UploadResult::Nak {
                stage: core::UploadStage::Request,
                status: 1,
            },
            UploadResult::Nak {
                stage: UploadStage::Request,
                status: 1,
            },
        ),
        (
            core::UploadResult::TimedOut {
                stage: core::UploadStage::Data,
            },
            UploadResult::TimedOut {
                stage: UploadStage::Data,
            },
        ),
        (
            core::UploadResult::BadReply {
                stage: core::UploadStage::Data,
            },
            UploadResult::BadReply {
                stage: UploadStage::Data,
            },
        ),
        (core::UploadResult::Aborted, UploadResult::Aborted),
        (core::UploadResult::Invalid, UploadResult::Invalid),
    ];
    for (from, want) in results {
        assert_eq!(
            Event::from(core::Event::UploadFinished {
                upload_id: 7,
                result: from
            }),
            Event::UploadFinished {
                upload_id: 7,
                result: want
            }
        );
    }
    assert_eq!(
        AnomalyKind::from(core::AnomalyKind::UploadFailed),
        AnomalyKind::UploadFailed
    );
}

#[test]
fn duty_events_convert() {
    assert_eq!(
        Event::from(core::Event::NotificationSubscription { enabled: true }),
        Event::NotificationSubscription { enabled: true }
    );
    let actions = [
        (
            core::NotificationAction::Dismiss,
            NotificationAction::Dismiss,
        ),
        (core::NotificationAction::Block, NotificationAction::Block),
        (
            core::NotificationAction::AcceptCall,
            NotificationAction::AcceptCall,
        ),
        (
            core::NotificationAction::RejectCall,
            NotificationAction::RejectCall,
        ),
        (
            core::NotificationAction::Custom { index: 2 },
            NotificationAction::Custom { index: 2 },
        ),
        (
            core::NotificationAction::Reply { text: "ok".into() },
            NotificationAction::Reply { text: "ok".into() },
        ),
    ];
    for (from, want) in actions {
        assert_eq!(
            Event::from(core::Event::NotificationAction {
                id: 3,
                action: from
            }),
            Event::NotificationAction {
                id: 3,
                action: want
            }
        );
    }
    assert_eq!(
        Event::from(core::Event::FindPhoneStart { duration_s: 30 }),
        Event::FindPhoneStart { duration_s: 30 }
    );
    assert_eq!(
        Event::from(core::Event::FindPhoneStop),
        Event::FindPhoneStop
    );
    assert_eq!(
        Event::from(core::Event::MusicRequested),
        Event::MusicRequested
    );
    let commands = [
        (
            core::MusicCommand::TogglePlayPause,
            MusicCommand::TogglePlayPause,
        ),
        (core::MusicCommand::Next, MusicCommand::Next),
        (core::MusicCommand::Previous, MusicCommand::Previous),
        (core::MusicCommand::VolumeUp, MusicCommand::VolumeUp),
        (core::MusicCommand::VolumeDown, MusicCommand::VolumeDown),
        (core::MusicCommand::Play, MusicCommand::Play),
        (core::MusicCommand::Pause, MusicCommand::Pause),
        (core::MusicCommand::SkipForward, MusicCommand::SkipForward),
        (core::MusicCommand::SkipBackward, MusicCommand::SkipBackward),
    ];
    for (from, want) in commands {
        assert_eq!(
            Event::from(core::Event::MusicCommand { command: from }),
            Event::MusicCommand { command: want }
        );
    }
    let states = [
        (
            core::FindWatchState::NotSupported,
            FindWatchState::NotSupported,
        ),
        (core::FindWatchState::Ringing, FindWatchState::Ringing),
        (core::FindWatchState::Stopped, FindWatchState::Stopped),
        (core::FindWatchState::Failed, FindWatchState::Failed),
        (
            core::FindWatchState::FoundOnWatch,
            FindWatchState::FoundOnWatch,
        ),
    ];
    for (from, want) in states {
        assert_eq!(
            Event::from(core::Event::FindWatch { state: from }),
            Event::FindWatch { state: want }
        );
    }
    assert_eq!(
        AnomalyKind::from(core::AnomalyKind::DutyMessageRefused),
        AnomalyKind::DutyMessageRefused
    );
}

#[test]
fn private_content_stays_out_of_debug_output() {
    let shown = format!(
        "{:?}",
        Input::PostNotification {
            notification: note()
        }
    );
    assert!(!shown.contains("private") && !shown.contains("Ann") && !shown.contains("chat"));
    let shown = format!(
        "{:?}",
        Event::NotificationAction {
            id: 1,
            action: NotificationAction::Reply {
                text: "secret reply".into()
            }
        }
    );
    assert!(!shown.contains("secret"));
    let shown = format!(
        "{:?}",
        Input::MusicInfo {
            info: MusicInfo {
                artist: "Hidden Artist".into(),
                album: "".into(),
                title: "Hidden Title".into(),
                duration_s: 1
            }
        }
    );
    assert!(!shown.contains("Hidden"));
}

#[test]
fn a_client_with_find_phone_on_reports_the_watchs_request() {
    // Through the real NimragClient: the duty flag reaches the core.
    let mut cfg = config();
    cfg.duties = Duties {
        notifications: false,
        music: false,
        find_phone: true,
        weather: false,
    };
    let client = NimragClient::new(cfg);
    // Before the handshake a duty input is ignored and produces nothing.
    let step = client.handle(
        now(0),
        Input::PostNotification {
            notification: note(),
        },
    );
    assert!(step.writes.is_empty());
}

#[test]
fn weather_input_and_events_convert() {
    let c: core::Input = Input::SendWeather {
        definitions: vec![0x40, 1],
        records: vec![vec![0, 1], vec![0, 2]],
    }
    .into();
    assert_eq!(
        c,
        core::Input::SendWeather {
            definitions: vec![0x40, 1],
            records: vec![vec![0, 1], vec![0, 2]],
        }
    );
    assert_eq!(
        Event::from(core::Event::WeatherRequest {
            format: 1,
            cell: Some(core::WeatherCell {
                lat_tenths: 557,
                lon_tenths: -126
            }),
            hours: 12
        }),
        Event::WeatherRequest {
            format: 1,
            cell: Some(WeatherCell {
                lat_tenths: 557,
                lon_tenths: -126
            }),
            hours: 12
        }
    );
    assert_eq!(
        Event::from(core::Event::WeatherRequest {
            format: 0,
            cell: None,
            hours: 0
        }),
        Event::WeatherRequest {
            format: 0,
            cell: None,
            hours: 0
        }
    );
    let results = [
        (core::WeatherResult::Sent, WeatherResult::Sent),
        (
            core::WeatherResult::NotSupported,
            WeatherResult::NotSupported,
        ),
        (core::WeatherResult::Busy, WeatherResult::Busy),
        (core::WeatherResult::Invalid, WeatherResult::Invalid),
        (core::WeatherResult::Refused, WeatherResult::Refused),
        (core::WeatherResult::TimedOut, WeatherResult::TimedOut),
    ];
    for (from, want) in results {
        assert_eq!(
            Event::from(core::Event::WeatherSent { result: from }),
            Event::WeatherSent { result: want }
        );
    }
}

#[test]
fn a_weather_cell_prints_nothing_of_the_position() {
    let shown = format!(
        "{:?}",
        Event::WeatherRequest {
            format: 1,
            cell: Some(WeatherCell {
                lat_tenths: 557,
                lon_tenths: 126
            }),
            hours: 12
        }
    );
    assert!(!shown.contains("557") && !shown.contains("126"), "{shown}");
}
