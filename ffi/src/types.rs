//! UniFFI mirrors of the `nimrag_core::client` public types. Plain records and
//! enums only; conversion lives in `convert.rs`. Field meanings are those of
//! the core types (see core/src/client/mod.rs).

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ClientConfig {
    pub client_id: u64,
    pub connection_epoch: u32,
    pub host_name: String,
    pub host_manufacturer: String,
    pub host_model: String,
    pub app_version: u16,
    pub capabilities: Vec<u8>,
    pub first_connect: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Now {
    pub mono_ms: u64,
    pub unix_s: i64,
    pub utc_offset_s: i32,
    /// Daylight-saving pair for the time reply (5052); `None` sends 0, 0.
    /// Caller rule: t1 = next zone transition after now (none gives `None`).
    /// If t1 is into DST: start = t1, end = next transition after t1 + 1 day.
    /// Otherwise: start = next transition after t1 + 1 day, end = t1.
    pub dst: Option<DstPair>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct DstPair {
    pub start_unix_s: i64,
    pub end_unix_s: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Input {
    Connected {
        att_mtu: u16,
    },
    Notification {
        value: Vec<u8>,
    },
    Tick,
    StartSync,
    FileStored {
        transfer_id: u64,
    },
    FileStoreFailed {
        transfer_id: u64,
    },
    /// Ask the watch for its battery level; the answer arrives as
    /// `Event::Battery`.
    RequestBattery,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Step {
    pub writes: Vec<Vec<u8>>,
    pub events: Vec<Event>,
    pub wake_at_mono_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DirEntry {
    pub index: u16,
    pub data_type: u8,
    pub sub_type: u8,
    pub file_number: u16,
    pub specific_flags: u8,
    pub flags: u8,
    pub size: u32,
    pub timestamp: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FileType {
    pub data_type: u8,
    pub sub_type: u8,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DeviceInfo {
    pub protocol_version: u16,
    pub product_number: u16,
    /// Personal identifier: never log it.
    pub unit_id: u32,
    pub software_version: u16,
    pub max_packet_size: u16,
    pub bluetooth_name: String,
    pub device_name: String,
    pub model_name: String,
    pub capabilities: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Event {
    LinkReady,
    HandshakeDone {
        device: DeviceInfo,
        file_types: Vec<FileType>,
    },
    PairingCompleted {
        all_acked: bool,
    },
    TimeSent {
        garmin_time: u32,
        utc_offset_s: i32,
    },
    SyncStarted {
        cycle: u32,
        trigger: SyncTrigger,
    },
    Directory {
        cycle: u32,
        entries: Vec<DirEntry>,
    },
    FileDownloaded {
        transfer_id: u64,
        entry: DirEntry,
        data: Vec<u8>,
    },
    FileFailed {
        entry: DirEntry,
        reason: FileFailure,
        will_retry: bool,
    },
    FileArchived {
        transfer_id: u64,
    },
    StaleConfirmation {
        transfer_id: u64,
    },
    SyncFinished {
        cycle: u32,
        downloaded: u32,
        archived: u32,
        failed: u32,
    },
    Unsupported {
        message_id: u16,
    },
    Battery {
        level_percent: u8,
        report: BatteryReport,
    },
    Anomaly {
        kind: AnomalyKind,
        message_id: u16,
    },
    Closed {
        reason: CloseReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SyncTrigger {
    Watch {
        option: u8,
    },
    Caller,
    /// The watch announced new file(s) (5009); no listing.
    FileAvailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BatteryReport {
    /// 5023: status bits 4-6 of byte 0 as sent (1 good, 2 ok, 3 low,
    /// 4 critical, 5 new) and the voltage in 10 mV. Status 7 (invalid) is not
    /// reported.
    Status { status: u8, centivolts: u8 },
    /// Answer to `Input::RequestBattery`; `battery_status` is the app's
    /// battery status number (0 unknown, 1 normal, 2 low, 3 critical,
    /// 4 charging, 5 fully charged, 6 error, 7 low pulse-ox) when present.
    DeviceStatus {
        battery_status: Option<u32>,
        hours_remaining: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FileFailure {
    Crc,
    Offset,
    Refused { status: u8 },
    Skipped,
    Compressed,
    StoreFailed,
    ArchiveRefused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AnomalyKind {
    BadFrame,
    UnexpectedResponse,
    BadTransport,
    ClockInvalid,
    ChunkedProtobuf,
    SyncNotGranted,
    GfdiClosedByWatch,
    BatteryRequestRefused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Phase {
    Idle,
    BringUp,
    Handshake,
    Ready,
    Syncing,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CloseReason {
    Disconnected,
    MtuTooSmall,
    ChannelInUse { retry_char: u16 },
    RegisterRejected { service: u16, status: u8 },
    AuthRequested,
    ProtocolTooOld { version: u16 },
    Timeout { phase: Phase },
    GfdiClosed { phase: Phase },
    RequestFailed { message_id: u16, status: u8 },
}
