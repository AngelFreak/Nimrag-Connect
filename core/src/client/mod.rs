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

pub mod reliable;
mod session;
mod sync;
mod transport;

use reliable::MIN_ATT_MTU;
use session::{Out, Session};
use transport::{Transport, TransportOut};

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
    /// GFDI is registered in reliable mode; waiting for 5024.
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
    },
    /// A watch request we do not implement; answered "not supported".
    Unsupported {
        message_id: u16,
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
pub enum SyncTrigger {
    /// SyncRequest (5037) from the watch; `option` as received.
    Watch { option: u8 },
    /// `Input::StartSync` (the watch still sends 5037 before the listing).
    Caller,
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
    /// GFDI was registered without the reliable flag.
    ReliableNotGranted,
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
            State::Active if !self.transport.is_up() => Phase::BringUp,
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
            Input::FileStored { transfer_id } => {
                let out = self.session.file_stored(now, transfer_id, true);
                self.apply_session(now, out, step);
            }
            Input::FileStoreFailed { transfer_id } => {
                let out = self.session.file_stored(now, transfer_id, false);
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
