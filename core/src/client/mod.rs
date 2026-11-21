//! Sans-IO protocol client: our side of the watch link (stage 2a).
//!
//! Signatures are fixed by the "Stage 2a contract" in IMPLEMENTATION_PLAN.md.
//! Protocol facts come from docs/protocol/PROTOCOL.md and our own captures;
//! behaviour ported from Gadgetbridge is labelled *inferred (Gadgetbridge)*
//! and listed in docs/PORTED.md (see docs/LICENSING.md).
//!
//! One `Client` value serves one GATT connection on one multi-link
//! characteristic pair (notify 6A4E281x / write 6A4E282x, chosen by the
//! caller). It is created when the link is up and dropped on disconnect, so
//! no sequence number, transaction id, handle or transfer id can leak into
//! the next connection.
//!
//! The caller owns all IO and all clocks:
//! - every call passes `Now` (monotonic time for timers, wall time for the
//!   time duty);
//! - `Step::writes` are ATT write-without-response values for the pair's
//!   write characteristic, to be sent in order, one at a time;
//! - `Step::wake_at_mono_ms` is the single deadline to arm; when it passes,
//!   call `handle(now, Input::Tick)`. It replaces any earlier deadline.
//!
//! The public API is UniFFI-friendly on purpose: owned values, no lifetimes
//! or generics, plain records and enums.

use crate::cobs;
use crate::describe::DirEntry;

mod companion;
pub mod reliable;
mod session;
mod sync;
mod transport;
mod upload;
mod weather;

pub use crate::duties::music::MusicCommand;
pub use crate::duties::notification::{
    NotificationAction, NotificationCategory, PhoneNotification,
};
pub use crate::duties::weather::{WeatherCell, WeatherResult};
pub use companion::{MusicInfo, MusicState};
use reliable::MIN_ATT_MTU;
use session::{Out, Session};
use transport::{Transport, TransportOut};
pub use upload::{UploadResult, UploadStage};

/// Seconds between the Unix epoch and the Garmin epoch (1989-12-31T00:00Z).
pub const GARMIN_EPOCH_UNIX_S: i64 = 631_065_600;

/// Values the client cannot derive from the protocol. None has a default in
/// `core`: the caller (and the replay tests) must supply every field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    /// Multi-link client id (u64 chosen by the host).
    pub client_id: u64,
    /// Distinct for every `Client` the caller creates (an in-process counter
    /// is enough). Transfer ids carry it, so a confirmation that belongs to an
    /// earlier connection can never match a download of this one.
    pub connection_epoch: u32,
    /// Host details for the reply to DeviceInformation (5024).
    pub host_name: String,
    pub host_manufacturer: String,
    pub host_model: String,
    pub app_version: u16,
    /// Our Configuration (5050) capability bitmask, sent as `u8 n` + bytes.
    pub capabilities: Vec<u8>,
    /// First connection after a fresh bonding: after the handshake the
    /// client also announces pair complete, sync complete and setup wizard
    /// complete (5030 events 4, 0, 14, value 0). Events, order and values are
    /// *inferred (Gadgetbridge)*, to be confirmed by Q-PAIR-1; sending them
    /// one at a time after the ACK of event 8 is ours. A refusal does not end
    /// the session. Set it for the first connection after bonding only;
    /// clear it once `Event::PairingCompleted` arrives, and after at most one
    /// more connection that ends without it (Gadgetbridge sends them once).
    pub first_connect: bool,
    /// True: after `FileStored` the client archives the file on the watch
    /// (5008), as before. False ("alongside" another sync app, stage 2g): the
    /// client never builds 5008; a stored file counts as copied and is left
    /// pending on the watch for the other app.
    pub archive: bool,
    /// Files to leave alone: a listed (or announced) entry whose `FileKey` is
    /// in this list is not downloaded and gives `Event::FileSkipped`. Keys
    /// only skip; they never cause an archive. The host passes the files it
    /// copied on earlier connections when `archive` is false.
    pub skip: Vec<FileKey>,
    /// The companion duties of stage 2f the host has switched on.
    pub duties: Duties,
}

/// The companion duties (stage 2f) a host can switch on. Each defaults to off
/// in the hosts; with a duty off the client answers the watch's messages for it
/// with "not supported" and sends nothing unasked, as before 2f. The caller also
/// has to claim the matching bits in `ClientConfig::capabilities`, or the watch
/// does not ask (notifications: bit 6, music: bits 7 and 52, find my phone: bits
/// 8 and 9, weather: bit 26; *inferred (Gadgetbridge)*, `GarminCapability`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Duties {
    /// Show the phone's notifications on the watch.
    pub notifications: bool,
    /// Music control from the watch and now-playing on the watch.
    pub music: bool,
    /// Let the watch ring the phone (find my phone).
    pub find_phone: bool,
    /// Answer the watch's weather request (5014) with a forecast the host
    /// fetched (`Input::SendWeather`). The caller also claims bit 26.
    pub weather: bool,
}

/// What names a file on the watch across connections: the directory entry's
/// type, sub type, file number, size and timestamp. The index is not part of
/// it because the watch reuses indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileKey {
    pub data_type: u8,
    pub sub_type: u8,
    pub file_number: u16,
    pub size: u32,
    pub timestamp: u32,
}

impl FileKey {
    pub fn of(entry: &DirEntry) -> FileKey {
        FileKey {
            data_type: entry.data_type,
            sub_type: entry.sub_type,
            file_number: entry.file_number,
            size: entry.size,
            timestamp: entry.timestamp,
        }
    }
}

/// The caller's clocks at the moment of the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Now {
    /// Monotonic milliseconds, any origin; must never go backwards.
    pub mono_ms: u64,
    /// Wall clock, Unix seconds UTC.
    pub unix_s: i64,
    /// Current total offset from UTC of the phone's time zone, seconds.
    pub utc_offset_s: i32,
    /// Daylight-saving pair for the 5052 reply, computed by the caller.
    /// Rule (Garmin Connect): t1 is the next zone transition after now; with
    /// none, pass `None` (both reply fields are sent as 0). If t1 is a
    /// transition into DST, `start_unix_s = t1` and `end_unix_s` is the next
    /// transition after t1 + 1 day; otherwise `start_unix_s` is the next
    /// transition after t1 + 1 day and `end_unix_s = t1`. In summer time the
    /// start is therefore the later of the two.
    pub dst: Option<DstPair>,
}

/// Next DST start and next DST end, Unix seconds UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DstPair {
    pub start_unix_s: i64,
    pub end_unix_s: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// The link is bonded, the MTU exchange is done and notifications are
    /// enabled on the pair's notify characteristic. First input, exactly once.
    Connected { att_mtu: u16 },
    /// One notification value from the pair's notify characteristic.
    Notification { value: Vec<u8> },
    /// The deadline from `Step::wake_at_mono_ms` has passed.
    Tick,
    /// Owner or scheduler asks for a sync now.
    StartSync,
    /// The bytes of `Event::FileDownloaded { transfer_id, .. }` are durably
    /// stored on the phone. Only this permits the archive request (5008).
    FileStored { transfer_id: u64 },
    /// Storing failed; the file stays pending on the watch.
    FileStoreFailed { transfer_id: u64 },
    /// Ask the watch for its battery level; the answer arrives as
    /// `Event::Battery` with `BatteryReport::DeviceStatus`. Sends one
    /// protobuf request (5043, device_status 8) once the handshake is done;
    /// a call before that is remembered and sent right after it. Calls while
    /// a request is still queued or unanswered add no second request. The
    /// client never asks by itself: Gadgetbridge requests the battery after
    /// every handshake, but our bring-up sequence is verified against our own
    /// captures and stays as it is, so the host decides when to ask. [ours]
    RequestBattery,
    /// A notification appeared or changed on the phone (notifications duty;
    /// ignored unless the duty is on and the watch has subscribed). Reuse the id
    /// to change a notification. The content is private: never log it.
    PostNotification { notification: PhoneNotification },
    /// The notification with this id is gone on the phone.
    RemoveNotification { id: u32 },
    /// The track that is playing (music duty). Replaces an update still waiting.
    MusicInfo { info: MusicInfo },
    /// The player state (music duty). Replaces an update still waiting.
    MusicState { state: MusicState },
    /// The phone's media volume, 0-100 (music duty).
    MusicVolume { percent: u8 },
    /// The forecast for the watch's last weather request (weather duty): FIT
    /// `weather_conditions` definition records and data records as the server
    /// made them, without a FIT file header. The client sends them as 5011 and
    /// 5012 and always answers with one `Event::WeatherSent`. The content is
    /// private: never log it.
    SendWeather {
        definitions: Vec<u8>,
        records: Vec<Vec<u8>>,
    },
    /// Ask the watch to ring (`start`) or to stop ringing. Needs the watch's
    /// find-my-watch capability; the outcome is `Event::FindWatch`.
    FindWatch { start: bool },
    /// Send a file to the watch (stage 4a: a workout is `data_type` 128,
    /// `sub_type` 5). `upload_id` is the caller's own number for it, echoed by
    /// `Event::UploadFinished`. Uploads run one at a time, in order, after the
    /// handshake and while no sync cycle runs; every upload ends with exactly
    /// one `Event::UploadFinished`, whatever happens. The content is private:
    /// never log it.
    UploadFile {
        upload_id: u64,
        data_type: u8,
        sub_type: u8,
        data: Vec<u8>,
    },
    /// The GATT link is gone. Terminal.
    Disconnected,
}

/// Result of one `Client::handle` call.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Step {
    pub writes: Vec<Vec<u8>>,
    pub events: Vec<Event>,
    pub wake_at_mono_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// GFDI is registered (reliable, or plain when MLR is not granted);
    /// waiting for 5024.
    LinkReady,
    /// HANDSHAKE_COMPLETE was acknowledged by the watch.
    HandshakeDone {
        device: DeviceInfo,
        file_types: Vec<FileType>,
    },
    /// The watch answered all first-connection events
    /// (`ClientConfig::first_connect`); record the watch as paired.
    /// `all_acked` is false when it refused at least one of them.
    PairingCompleted {
        all_acked: bool,
    },
    /// A CurrentTime (5052) request was answered with this Garmin time.
    TimeSent {
        garmin_time: u32,
        utc_offset_s: i32,
    },
    SyncStarted {
        cycle: u32,
        trigger: SyncTrigger,
    },
    /// The pending-files listing of this cycle (directory entry 0 removed).
    Directory {
        cycle: u32,
        entries: Vec<DirEntry>,
    },
    /// A complete file whose size and running CRC both checked out. The
    /// client now waits for `FileStored` / `FileStoreFailed`.
    FileDownloaded {
        transfer_id: u64,
        entry: DirEntry,
        data: Vec<u8>,
    },
    /// A file was not obtained in this cycle; it is not archived.
    FileFailed {
        entry: DirEntry,
        reason: FileFailure,
        will_retry: bool,
    },
    /// The entry's `FileKey` is in `ClientConfig::skip` (or the file was
    /// stored earlier on this connection with `archive` false): not
    /// downloaded, not archived.
    FileSkipped {
        entry: DirEntry,
    },
    /// The watch acknowledged the archive flag for this transfer.
    FileArchived {
        transfer_id: u64,
    },
    /// A `FileStored` that did not match the transfer being waited on.
    StaleConfirmation {
        transfer_id: u64,
    },
    SyncFinished {
        cycle: u32,
        downloaded: u32,
        archived: u32,
        failed: u32,
        /// Files left alone because of `ClientConfig::skip`.
        skipped: u32,
    },
    /// A watch request we do not implement; answered "not supported".
    Unsupported {
        message_id: u16,
    },
    /// The watch's battery level, 0-100. Either pushed by the watch (5023) or
    /// the answer to `Input::RequestBattery`; `report` says which and carries
    /// the extra fields of that message. Levels above 100 and 5023 readings
    /// with status 7 (invalid) are dropped.
    Battery {
        level_percent: u8,
        report: BatteryReport,
    },
    /// The watch subscribed to (`enabled`) or left the notification stream
    /// (notifications duty).
    NotificationSubscription {
        enabled: bool,
    },
    /// The wearer did something to notification `id` on the watch.
    NotificationAction {
        id: u32,
        action: NotificationAction,
    },
    /// The watch asks the phone to ring (find my phone duty).
    FindPhoneStart {
        duration_s: u8,
    },
    /// The watch cancels the ring request.
    FindPhoneStop,
    /// A watch button asks the player to do something (music duty).
    MusicCommand {
        command: MusicCommand,
    },
    /// The watch asked what it can control: push the current track and state
    /// again (music duty).
    MusicRequested,
    /// Progress of `Input::FindWatch`, or the watch cancelling by itself.
    FindWatch {
        state: FindWatchState,
    },
    /// The watch asks for weather (5014; weather duty on). Already
    /// acknowledged. `cell` is the watch's position rounded to 0.1 degree, or
    /// `None` when it has no usable position; the exact position is never
    /// reported. Answer with `Input::SendWeather` or not at all.
    WeatherRequest {
        format: u8,
        cell: Option<WeatherCell>,
        hours: u8,
    },
    /// `Input::SendWeather` ended.
    WeatherSent {
        result: WeatherResult,
    },
    /// The upload `upload_id` of `Input::UploadFile` ended. Failure is not the
    /// session's: it goes on (with `AnomalyKind::UploadFailed` unless the
    /// result is `Aborted` or `Invalid`).
    UploadFinished {
        upload_id: u64,
        result: UploadResult,
    },
    /// Something unexpected that did not end the session.
    Anomaly {
        kind: AnomalyKind,
        message_id: u16,
    },
    /// The session is over; every later input yields an empty `Step`.
    Closed {
        reason: CloseReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub protocol_version: u16,
    pub product_number: u16,
    /// Personal identifier: never log it, never put it in a committed file.
    pub unit_id: u32,
    pub software_version: u16,
    pub max_packet_size: u16,
    pub bluetooth_name: String,
    pub device_name: String,
    pub model_name: String,
    /// Capability bitmask from the watch's Configuration (5050).
    pub capabilities: Vec<u8>,
}

pub use crate::messages::FileType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryReport {
    /// 5023: status bits 4-6 of byte 0 as sent (1 good, 2 ok, 3 low,
    /// 4 critical, 5 new) and the voltage in 10 mV. Status 7 (invalid) is
    /// acknowledged but not reported. *inferred (Gadgetbridge)*
    Status { status: u8, centivolts: u8 },
    /// Answer to `Input::RequestBattery` (device_status 8). `battery_status`
    /// is the app's battery status number (0 unknown, 1 normal, 2 low,
    /// 3 critical, 4 charging, 5 fully charged, 6 error, 7 low pulse-ox) when
    /// present.
    DeviceStatus {
        battery_status: Option<u32>,
        hours_remaining: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindWatchState {
    /// The watch does not advertise find my watch; nothing was sent.
    NotSupported,
    /// The watch confirmed it is ringing.
    Ringing,
    /// The watch confirmed it stopped.
    Stopped,
    /// The watch refused or the request failed.
    Failed,
    /// The wearer cancelled on the watch.
    FoundOnWatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncTrigger {
    /// SyncRequest (5037) from the watch; `option` as received.
    Watch { option: u8 },
    /// `Input::StartSync` (the watch still sends 5037 before the listing).
    Caller,
    /// The watch announced new file(s) with FileAvailable (5009): the cycle
    /// goes straight to the announced files, with no filter, no listing and
    /// no `Event::Directory`.
    FileAvailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFailure {
    /// A chunk's running CRC or the total size did not match.
    Crc,
    /// A chunk arrived at an unexpected offset.
    Offset,
    /// The watch refused the download; `status` from the 5002 reply.
    Refused {
        status: u8,
    },
    /// The entry is not a readable FIT file; skipped by policy.
    Skipped,
    /// The watch sent compressed data (5054), which is not supported.
    Compressed,
    StoreFailed,
    /// The watch refused the archive flag.
    ArchiveRefused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyKind {
    /// COBS, length or CRC error in a received GFDI frame.
    BadFrame,
    /// A response that matches no outstanding request.
    UnexpectedResponse,
    /// A value on an unknown handle or a malformed control message.
    BadTransport,
    /// Wall clock before 2020-01-01: the time request was left unanswered.
    ClockInvalid,
    /// A protobuf request spanning several chunks (never seen so far).
    ChunkedProtobuf,
    /// `StartSync` was sent but the watch did not follow with 5037.
    SyncNotGranted,
    /// The watch closed the GFDI handle; the transport registers GFDI again
    /// (inferred from Gadgetbridge; PROTOCOL.md 9.1). Only while the session
    /// is idle: otherwise the client closes with `CloseReason::GfdiClosed`.
    /// Frames sent meanwhile go out on the new handle; a 5024 from the watch
    /// afterwards restarts the handshake.
    GfdiClosedByWatch,
    /// The battery request (5043 from `Input::RequestBattery`) was refused
    /// with a non-ACK status, or not answered within the reply deadline (it
    /// is then dropped). Not fatal; the session goes on.
    BatteryRequestRefused,
    /// A message of a companion duty (a notification update or its data, a
    /// now-playing update, a find-my-watch request) was refused by the watch or
    /// not answered in time. Not fatal: the message is dropped and the session
    /// goes on.
    DutyMessageRefused,
    /// An upload to the watch failed: refused, answered with a non-ACK status,
    /// a reply that made no sense, or no answer within the deadline. The
    /// matching `Event::UploadFinished` says how; `message_id` is the message
    /// of the step (5005, 5003 or 5004). Not fatal. The closing 5030 event after
    /// an upload that went through uses this kind too when it is refused.
    UploadFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    Disconnected,
    /// MTU below the minimum of 23.
    MtuTooSmall,
    /// Register status 3 for REGISTRATION or GFDI; retry on this pair.
    ChannelInUse {
        retry_char: u16,
    },
    RegisterRejected {
        service: u16,
        status: u8,
    },
    /// The watch closed the GFDI handle while a request or a sync cycle was
    /// outstanding in this phase; its answer is lost. Reconnect.
    GfdiClosed {
        phase: Phase,
    },
    /// Auth Negotiation (5101): unexpected on a bonded link.
    AuthRequested,
    /// The watch reports a GFDI protocol version below 150.
    ProtocolTooOld {
        version: u16,
    },
    /// No answer within the deadline while in this phase.
    Timeout {
        phase: Phase,
    },
    /// The watch answered one of our requests with a non-ACK status.
    RequestFailed {
        message_id: u16,
        status: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Before `Input::Connected`.
    Idle,
    /// Close-all, REGISTRATION, info pages, GFDI and KEEP_ALIVE registration.
    BringUp,
    /// 5024 / 5050 / 5031 / HANDSHAKE_COMPLETE.
    Handshake,
    /// Handshake done, no sync cycle running.
    Ready,
    /// Listing, downloading, waiting for the caller, archiving.
    Syncing,
    Closed,
}

/// The client state machine. See the module documentation.
#[derive(Debug)]
pub struct Client {
    state: State,
    clock_ms: u64,
    transport: Transport,
    session: Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Before `Input::Connected`.
    Idle,
    Active,
    Closed,
}

impl Client {
    pub fn new(config: ClientConfig) -> Client {
        Client {
            state: State::Idle,
            clock_ms: 0,
            transport: Transport::new(config.client_id),
            session: Session::new(&config),
        }
    }

    /// Feed one input and collect what to write, what happened and when to
    /// call again. Never blocks, never panics on malformed peer data.
    pub fn handle(&mut self, now: Now, input: Input) -> Step {
        let mut step = Step::default();
        if self.state == State::Closed {
            return step;
        }
        // Monotonic time must not go backwards; tolerate a caller that does.
        self.clock_ms = self.clock_ms.max(now.mono_ms);
        let now = Now {
            mono_ms: self.clock_ms,
            ..now
        };
        match input {
            Input::Disconnected => self.close(CloseReason::Disconnected, &mut step),
            Input::Connected { att_mtu } => self.connect(&now, att_mtu, &mut step),
            // Before the link is up nothing can be sent; an upload is reported
            // instead of being dropped silently.
            Input::UploadFile { upload_id, .. } if self.state == State::Idle => {
                step.events.push(Event::UploadFinished {
                    upload_id,
                    result: UploadResult::Aborted,
                });
            }
            Input::SendWeather { .. } if self.state == State::Idle => {
                step.events.push(Event::WeatherSent {
                    result: WeatherResult::NotSupported,
                });
            }
            _ if self.state == State::Idle => {}
            input => {
                self.apply_input(&now, input, &mut step);
                if self.state == State::Active {
                    self.run_timers(&now, &mut step);
                }
            }
        }
        if self.state == State::Active {
            step.wake_at_mono_ms = self.next_wake();
        }
        step
    }

    pub fn phase(&self) -> Phase {
        match self.state {
            State::Idle => Phase::Idle,
            State::Closed => Phase::Closed,
            State::Active if !self.transport.is_up() || self.transport.reopening() => {
                Phase::BringUp
            }
            State::Active => self.session.phase(),
        }
    }

    fn connect(&mut self, now: &Now, att_mtu: u16, step: &mut Step) {
        if self.state != State::Idle {
            return;
        }
        if att_mtu < MIN_ATT_MTU {
            self.close(CloseReason::MtuTooSmall, step);
            return;
        }
        self.state = State::Active;
        let out = self.transport.start(now.mono_ms, att_mtu);
        self.apply_transport(now, out, step);
    }

    fn apply_input(&mut self, now: &Now, input: Input, step: &mut Step) {
        match input {
            Input::Notification { value } => {
                let out = self.transport.on_notification(now.mono_ms, &value);
                self.apply_transport(now, out, step);
            }
            Input::Tick => {}
            Input::StartSync => {
                let out = self.session.start_sync(now);
                self.apply_session(now, out, step);
            }
            Input::RequestBattery => {
                let out = self.session.request_battery(now);
                self.apply_session(now, out, step);
            }
            Input::FileStored { transfer_id } => {
                let out = self.session.file_stored(now, transfer_id, true);
                self.apply_session(now, out, step);
            }
            Input::FileStoreFailed { transfer_id } => {
                let out = self.session.file_stored(now, transfer_id, false);
                self.apply_session(now, out, step);
            }
            Input::PostNotification { notification } => {
                let out = self.session.post_notification(now, notification);
                self.apply_session(now, out, step);
            }
            Input::RemoveNotification { id } => {
                let out = self.session.remove_notification(now, id);
                self.apply_session(now, out, step);
            }
            Input::MusicInfo { info } => {
                let out = self.session.music_info(now, &info);
                self.apply_session(now, out, step);
            }
            Input::MusicState { state } => {
                let out = self.session.music_state(now, state);
                self.apply_session(now, out, step);
            }
            Input::MusicVolume { percent } => {
                let out = self.session.music_volume(now, percent);
                self.apply_session(now, out, step);
            }
            Input::FindWatch { start } => {
                let out = self.session.find_watch(now, start);
                self.apply_session(now, out, step);
            }
            Input::SendWeather {
                definitions,
                records,
            } => {
                let out = self.session.send_weather(now, definitions, records);
                self.apply_session(now, out, step);
            }
            Input::UploadFile {
                upload_id,
                data_type,
                sub_type,
                data,
            } => {
                let out = self
                    .session
                    .upload_file(now, upload_id, data_type, sub_type, data);
                self.apply_session(now, out, step);
            }
            Input::Connected { .. } | Input::Disconnected => {}
        }
    }

    /// Run every timer that is due; early calls do nothing.
    fn run_timers(&mut self, now: &Now, step: &mut Step) {
        let out = self.transport.on_tick(now.mono_ms);
        self.apply_transport(now, out, step);
        if self.state == State::Active && self.transport.is_up() {
            let out = self.session.on_tick(now);
            self.apply_session(now, out, step);
        }
    }

    fn apply_transport(&mut self, now: &Now, out: TransportOut, step: &mut Step) {
        step.writes.extend(out.writes);
        for kind in out.anomalies {
            step.events.push(Event::Anomaly {
                kind,
                message_id: 0,
            });
        }
        if let Some(reason) = out.closed {
            self.close(reason, step);
            return;
        }
        if out.gfdi_closed && !self.session.is_idle() {
            let phase = self.session.phase();
            self.close(CloseReason::GfdiClosed { phase }, step);
            return;
        }
        if out.link_ready {
            step.events.push(Event::LinkReady);
            self.session.start(now);
        }
        for frame in out.frames {
            if self.state != State::Active {
                break;
            }
            let session_out = self.session.on_frame(now, frame);
            self.apply_session(now, session_out, step);
        }
    }

    fn apply_session(&mut self, now: &Now, out: Out, step: &mut Step) {
        step.events.extend(out.events);
        for msg in out.msgs {
            let framed = cobs::encode_frame(&msg.encode());
            step.writes
                .extend(self.transport.send_gfdi(now.mono_ms, &framed));
        }
        if let Some(reason) = out.closed {
            self.close(reason, step);
        }
    }

    fn close(&mut self, reason: CloseReason, step: &mut Step) {
        self.state = State::Closed;
        // Nothing is worth writing on a link we are abandoning.
        step.writes.clear();
        step.wake_at_mono_ms = None;
        for upload_id in self.session.take_unfinished_uploads() {
            step.events.push(Event::UploadFinished {
                upload_id,
                result: UploadResult::Aborted,
            });
        }
        step.events.push(Event::Closed { reason });
    }

    fn next_wake(&self) -> Option<u64> {
        let transport = self.transport.next_wake();
        let session = if self.transport.is_up() {
            self.session.next_wake()
        } else {
            None
        };
        match (transport, session) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}
