//! UniFFI mirrors of the `nimrag_core::client` public types. Plain records and
//! enums only; conversion lives in `convert.rs`. Field meanings are those of
//! the core types (see core/src/client/mod.rs).

use std::fmt;

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
    /// False: never archive on the watch (alongside another sync app).
    pub archive: bool,
    /// Files to leave alone (not downloaded, `Event::FileSkipped`).
    pub skip: Vec<FileKey>,
    /// The companion duties the host has switched on (all off by default).
    pub duties: Duties,
}

/// The companion duties of stage 2f. The host also has to claim the matching
/// capability bits in `ClientConfig::capabilities`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, uniffi::Record)]
pub struct Duties {
    pub notifications: bool,
    pub music: bool,
    pub find_phone: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NotificationCategory {
    Other,
    Email,
    Message,
    Social,
    Location,
}

/// A notification as far as the watch needs it. Private content: `Debug`
/// prints only the id and the category.
#[derive(Clone, PartialEq, Eq, uniffi::Record)]
pub struct PhoneNotification {
    pub id: u32,
    pub app_id: String,
    pub app_name: String,
    pub category: NotificationCategory,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub when_unix_s: i64,
}

impl fmt::Debug for PhoneNotification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PhoneNotification")
            .field("id", &self.id)
            .field("category", &self.category)
            .finish_non_exhaustive()
    }
}

/// What the wearer did to a notification on the watch. `Reply` text is private.
#[derive(Clone, PartialEq, Eq, uniffi::Enum)]
pub enum NotificationAction {
    Dismiss,
    Block,
    Reply { text: String },
    AcceptCall,
    RejectCall,
    Custom { index: u8 },
}

impl fmt::Debug for NotificationAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotificationAction::Dismiss => f.write_str("Dismiss"),
            NotificationAction::Block => f.write_str("Block"),
            NotificationAction::Reply { .. } => f.write_str("Reply { .. }"),
            NotificationAction::AcceptCall => f.write_str("AcceptCall"),
            NotificationAction::RejectCall => f.write_str("RejectCall"),
            NotificationAction::Custom { index } => write!(f, "Custom {{ index: {index} }}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MusicCommand {
    TogglePlayPause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Play,
    Pause,
    SkipForward,
    SkipBackward,
}

/// The track that is playing. Private content: `Debug` hides it.
#[derive(Clone, PartialEq, Eq, uniffi::Record)]
pub struct MusicInfo {
    pub artist: String,
    pub album: String,
    pub title: String,
    pub duration_s: u32,
}

impl fmt::Debug for MusicInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MusicInfo")
            .field("duration_s", &self.duration_s)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct MusicState {
    pub playing: bool,
    /// Playback speed in percent (100 = normal).
    pub rate_percent: u16,
    pub position_s: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FindWatchState {
    NotSupported,
    Ringing,
    Stopped,
    Failed,
    FoundOnWatch,
}

/// Which message of an upload was being answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UploadStage {
    Create,
    Request,
    Data,
}

/// How an upload to the watch ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UploadResult {
    Sent,
    /// The watch refused; `status` is the create, upload or transfer status of
    /// that stage (see `nimrag_core::client::UploadResult`).
    Refused {
        stage: UploadStage,
        status: u8,
    },
    /// The watch answered with a status other than ACK.
    Nak {
        stage: UploadStage,
        status: u8,
    },
    TimedOut {
        stage: UploadStage,
    },
    BadReply {
        stage: UploadStage,
    },
    /// The link closed first.
    Aborted,
    /// Nothing was sent: no data, too much, a full queue, or a link too small.
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct FileKey {
    pub data_type: u8,
    pub sub_type: u8,
    pub file_number: u16,
    pub size: u32,
    pub timestamp: u32,
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
    /// A notification appeared or changed on the phone (reuse the id to change).
    PostNotification {
        notification: PhoneNotification,
    },
    RemoveNotification {
        id: u32,
    },
    MusicInfo {
        info: MusicInfo,
    },
    MusicState {
        state: MusicState,
    },
    /// The phone's media volume, 0-100.
    MusicVolume {
        percent: u8,
    },
    /// Ask the watch to ring or to stop ringing.
    FindWatch {
        start: bool,
    },
    /// Send a file to the watch (a workout is `data_type` 128, `sub_type` 5).
    /// The caller's `upload_id` comes back in `Event::UploadFinished`. The
    /// content is private: never log it.
    UploadFile {
        upload_id: u64,
        data_type: u8,
        sub_type: u8,
        data: Vec<u8>,
    },
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
    FileSkipped {
        entry: DirEntry,
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
        skipped: u32,
    },
    Unsupported {
        message_id: u16,
    },
    Battery {
        level_percent: u8,
        report: BatteryReport,
    },
    NotificationSubscription {
        enabled: bool,
    },
    NotificationAction {
        id: u32,
        action: NotificationAction,
    },
    FindPhoneStart {
        duration_s: u8,
    },
    FindPhoneStop,
    MusicCommand {
        command: MusicCommand,
    },
    MusicRequested,
    FindWatch {
        state: FindWatchState,
    },
    UploadFinished {
        upload_id: u64,
        result: UploadResult,
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
    DutyMessageRefused,
    UploadFailed,
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

/// What the device catalogue says about an advertised Garmin device
/// (`identify_device`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DeviceModelInfo {
    /// Catalogue id (Gadgetbridge `DeviceType` constant), e.g. `GARMIN_INSTINCT_2`.
    pub id: String,
    /// Model name, e.g. "Garmin Instinct 2".
    pub name: String,
    /// True when nothing matched and this is the `GARMIN_GENERIC` fallback.
    pub generic: bool,
    /// False when the catalogue says our client cannot sync with the model yet.
    pub can_sync: bool,
    /// The reason when `can_sync` is false.
    pub unsupported_reason: Option<String>,
    /// True only where our own captures verify the model.
    pub verified: bool,
}
