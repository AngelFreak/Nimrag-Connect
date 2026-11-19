//! Boring, exhaustive conversions between `nimrag_core::client` and the FFI
//! types. Exhaustive matches on purpose: a new core variant fails to compile
//! here instead of being dropped.

use nimrag_core::client as core;
use nimrag_core::messages::DirEntry as CoreDirEntry;

use crate::types::*;

impl From<ClientConfig> for core::ClientConfig {
    fn from(c: ClientConfig) -> Self {
        core::ClientConfig {
            client_id: c.client_id,
            connection_epoch: c.connection_epoch,
            host_name: c.host_name,
            host_manufacturer: c.host_manufacturer,
            host_model: c.host_model,
            app_version: c.app_version,
            capabilities: c.capabilities,
            first_connect: c.first_connect,
            archive: c.archive,
            skip: c.skip.into_iter().map(core::FileKey::from).collect(),
        }
    }
}

impl From<FileKey> for core::FileKey {
    fn from(k: FileKey) -> Self {
        core::FileKey {
            data_type: k.data_type,
            sub_type: k.sub_type,
            file_number: k.file_number,
            size: k.size,
            timestamp: k.timestamp,
        }
    }
}

impl From<Now> for core::Now {
    fn from(n: Now) -> Self {
        core::Now {
            mono_ms: n.mono_ms,
            unix_s: n.unix_s,
            utc_offset_s: n.utc_offset_s,
            dst: n.dst.map(|d| core::DstPair {
                start_unix_s: d.start_unix_s,
                end_unix_s: d.end_unix_s,
            }),
        }
    }
}

impl From<Input> for core::Input {
    fn from(i: Input) -> Self {
        match i {
            Input::Connected { att_mtu } => core::Input::Connected { att_mtu },
            Input::Notification { value } => core::Input::Notification { value },
            Input::Tick => core::Input::Tick,
            Input::StartSync => core::Input::StartSync,
            Input::FileStored { transfer_id } => core::Input::FileStored { transfer_id },
            Input::FileStoreFailed { transfer_id } => core::Input::FileStoreFailed { transfer_id },
            Input::RequestBattery => core::Input::RequestBattery,
            Input::Disconnected => core::Input::Disconnected,
        }
    }
}

impl From<core::Step> for Step {
    fn from(s: core::Step) -> Self {
        Step {
            writes: s.writes,
            events: s.events.into_iter().map(Event::from).collect(),
            wake_at_mono_ms: s.wake_at_mono_ms,
        }
    }
}

impl From<CoreDirEntry> for DirEntry {
    fn from(e: CoreDirEntry) -> Self {
        DirEntry {
            index: e.index,
            data_type: e.data_type,
            sub_type: e.sub_type,
            file_number: e.file_number,
            specific_flags: e.specific_flags,
            flags: e.flags,
            size: e.size,
            timestamp: e.timestamp,
        }
    }
}

impl From<core::FileType> for FileType {
    fn from(t: core::FileType) -> Self {
        FileType {
            data_type: t.data_type,
            sub_type: t.sub_type,
            name: t.name,
        }
    }
}

impl From<core::DeviceInfo> for DeviceInfo {
    fn from(d: core::DeviceInfo) -> Self {
        DeviceInfo {
            protocol_version: d.protocol_version,
            product_number: d.product_number,
            unit_id: d.unit_id,
            software_version: d.software_version,
            max_packet_size: d.max_packet_size,
            bluetooth_name: d.bluetooth_name,
            device_name: d.device_name,
            model_name: d.model_name,
            capabilities: d.capabilities,
        }
    }
}

impl From<core::Event> for Event {
    fn from(e: core::Event) -> Self {
        match e {
            core::Event::LinkReady => Event::LinkReady,
            core::Event::HandshakeDone { device, file_types } => Event::HandshakeDone {
                device: device.into(),
                file_types: file_types.into_iter().map(FileType::from).collect(),
            },
            core::Event::PairingCompleted { all_acked } => Event::PairingCompleted { all_acked },
            core::Event::TimeSent {
                garmin_time,
                utc_offset_s,
            } => Event::TimeSent {
                garmin_time,
                utc_offset_s,
            },
            core::Event::SyncStarted { cycle, trigger } => Event::SyncStarted {
                cycle,
                trigger: trigger.into(),
            },
            core::Event::Directory { cycle, entries } => Event::Directory {
                cycle,
                entries: entries.into_iter().map(DirEntry::from).collect(),
            },
            core::Event::FileDownloaded {
                transfer_id,
                entry,
                data,
            } => Event::FileDownloaded {
                transfer_id,
                entry: entry.into(),
                data,
            },
            core::Event::FileFailed {
                entry,
                reason,
                will_retry,
            } => Event::FileFailed {
                entry: entry.into(),
                reason: reason.into(),
                will_retry,
            },
            core::Event::FileSkipped { entry } => Event::FileSkipped {
                entry: entry.into(),
            },
            core::Event::FileArchived { transfer_id } => Event::FileArchived { transfer_id },
            core::Event::StaleConfirmation { transfer_id } => {
                Event::StaleConfirmation { transfer_id }
            }
            core::Event::SyncFinished {
                cycle,
                downloaded,
                archived,
                failed,
                skipped,
            } => Event::SyncFinished {
                cycle,
                downloaded,
                archived,
                failed,
                skipped,
            },
            core::Event::Unsupported { message_id } => Event::Unsupported { message_id },
            core::Event::Battery {
                level_percent,
                report,
            } => Event::Battery {
                level_percent,
                report: report.into(),
            },
            core::Event::Anomaly { kind, message_id } => Event::Anomaly {
                kind: kind.into(),
                message_id,
            },
            core::Event::Closed { reason } => Event::Closed {
                reason: reason.into(),
            },
        }
    }
}

impl From<core::SyncTrigger> for SyncTrigger {
    fn from(t: core::SyncTrigger) -> Self {
        match t {
            core::SyncTrigger::Watch { option } => SyncTrigger::Watch { option },
            core::SyncTrigger::Caller => SyncTrigger::Caller,
            core::SyncTrigger::FileAvailable => SyncTrigger::FileAvailable,
        }
    }
}

impl From<core::BatteryReport> for BatteryReport {
    fn from(r: core::BatteryReport) -> Self {
        match r {
            core::BatteryReport::Status { status, centivolts } => {
                BatteryReport::Status { status, centivolts }
            }
            core::BatteryReport::DeviceStatus {
                battery_status,
                hours_remaining,
            } => BatteryReport::DeviceStatus {
                battery_status,
                hours_remaining,
            },
        }
    }
}

impl From<core::FileFailure> for FileFailure {
    fn from(f: core::FileFailure) -> Self {
        match f {
            core::FileFailure::Crc => FileFailure::Crc,
            core::FileFailure::Offset => FileFailure::Offset,
            core::FileFailure::Refused { status } => FileFailure::Refused { status },
            core::FileFailure::Skipped => FileFailure::Skipped,
            core::FileFailure::Compressed => FileFailure::Compressed,
            core::FileFailure::StoreFailed => FileFailure::StoreFailed,
            core::FileFailure::ArchiveRefused => FileFailure::ArchiveRefused,
        }
    }
}

impl From<core::AnomalyKind> for AnomalyKind {
    fn from(k: core::AnomalyKind) -> Self {
        match k {
            core::AnomalyKind::BadFrame => AnomalyKind::BadFrame,
            core::AnomalyKind::UnexpectedResponse => AnomalyKind::UnexpectedResponse,
            core::AnomalyKind::BadTransport => AnomalyKind::BadTransport,
            core::AnomalyKind::ClockInvalid => AnomalyKind::ClockInvalid,
            core::AnomalyKind::ChunkedProtobuf => AnomalyKind::ChunkedProtobuf,
            core::AnomalyKind::SyncNotGranted => AnomalyKind::SyncNotGranted,
            core::AnomalyKind::GfdiClosedByWatch => AnomalyKind::GfdiClosedByWatch,
            core::AnomalyKind::BatteryRequestRefused => AnomalyKind::BatteryRequestRefused,
        }
    }
}

impl From<core::Phase> for Phase {
    fn from(p: core::Phase) -> Self {
        match p {
            core::Phase::Idle => Phase::Idle,
            core::Phase::BringUp => Phase::BringUp,
            core::Phase::Handshake => Phase::Handshake,
            core::Phase::Ready => Phase::Ready,
            core::Phase::Syncing => Phase::Syncing,
            core::Phase::Closed => Phase::Closed,
        }
    }
}

impl From<core::CloseReason> for CloseReason {
    fn from(r: core::CloseReason) -> Self {
        match r {
            core::CloseReason::Disconnected => CloseReason::Disconnected,
            core::CloseReason::MtuTooSmall => CloseReason::MtuTooSmall,
            core::CloseReason::ChannelInUse { retry_char } => {
                CloseReason::ChannelInUse { retry_char }
            }
            core::CloseReason::RegisterRejected { service, status } => {
                CloseReason::RegisterRejected { service, status }
            }
            core::CloseReason::AuthRequested => CloseReason::AuthRequested,
            core::CloseReason::ProtocolTooOld { version } => {
                CloseReason::ProtocolTooOld { version }
            }
            core::CloseReason::GfdiClosed { phase } => CloseReason::GfdiClosed {
                phase: phase.into(),
            },
            core::CloseReason::Timeout { phase } => CloseReason::Timeout {
                phase: phase.into(),
            },
            core::CloseReason::RequestFailed { message_id, status } => {
                CloseReason::RequestFailed { message_id, status }
            }
        }
    }
}
