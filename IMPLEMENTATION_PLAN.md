# nimrag implementation plan

Source of truth for scope: the owner's build prompt (phases 0–4) and the
phase 0 brief "map the Garmin Connect ↔ Instinct 2 BLE sync protocol".
Each phase stops at its gate for an owner report.

## Stage 0a: Static protocol map (RE brief)
**Goal**: `docs/protocol/` deliverables: PROTOCOL.md, messages.csv, filetypes.csv, proto/, HOOKS.md, OPEN-QUESTIONS.md, PROVENANCE.md
**Success Criteria**: brief's "done when": transport, framing, session start and file sync have no unknowns, or each has a concrete phase 2 test; must-have file types identified; HOOKS.md has raw write/notify and dispatch
**Tests**: n/a (documentation); every "seen" claim cites class/method + anchor constant
**Status**: In Progress

## Stage 0b: Live captures (HCI snoop)
**Goal**: snoop logs for fresh pairing, normal sync, sync after activity, sync after night, settings change
**Success Criteria**: logs stored under `private/captures/` (git-ignored), scrubbed copies under `fixtures/`
**Tests**: n/a
**Status**: In Progress (snoop log enabled via Developer options UI 2026-11-19 09:41; first capture = BT restart reconnect + manual sync)

## Stage 0c: /core framing + tools/decode
**Goal**: sans-IO frame codec (COBS, CRC-16, message ids) in /core; CLI decodes snoop logs / live adb stream
**Success Criteria**: decodes every capture with no unknown frames, or lists unknowns in OPEN-QUESTIONS.md
**Tests**: codec unit tests; fixture replay integration test through the CLI entry point
**Status**: In Progress

## Gate 0
Report to owner. Do not start phase 1 unasked.

# Phase 1: server ingest and parsing (started 2026-11-19 at owner's request; phase 0 still awaits night + fresh-pairing captures)

Roles: main session orchestrates and integrates; Sonnet builders implement on disjoint paths and do not commit; a Fable advisor reviews the contract and finished work.

## Stage 1a: FIT decoder (/core/src/fit/decode.rs) — builder A
**Goal**: sans-IO FIT binary decoder per contract in core/src/fit/mod.rs; keeps every message and field (unknown, manufacturer-specific, developer) with raw bytes
**Success Criteria**: decodes all FIT files extracted from private captures with header+file CRC ok; round-trip raw bytes per field; chained files; compressed timestamps
**Tests**: synthetic FIT files built in tests (definition/data, big-endian arch, compressed timestamp, developer fields, invalid values, chained); private-file replay
**Status**: Complete

## Stage 1b: FIT profile from the app (/core/src/fit/profile.rs, tools/fit-profile/) — builder B
**Goal**: generated table of message/field names, types, scale/offset/units and enum value names, extracted from the FIT profile bundled in Garmin Connect 5.31
**Success Criteria**: lookup API per contract; covers record, session, lap, monitoring, sleep_level, hrv_*, set, exercise_title, etc.; generator reproducible
**Tests**: known entries (record.heart_rate, session.total_timer_time scale 1000, sport enum names)
**Status**: Complete

## Stage 1c: server ingest (/server) — builder C
**Goal**: axum + sqlx SQLite (WAL) service: file store by SHA-256, POST /api/files idempotent, device-token auth, migrations, `reparse` subcommand skeleton, raw FIT message table
**Success Criteria**: upload same file twice -> one stored copy, 200 both times with same id; unauthenticated -> 401; reparse rebuilds derived tables from store
**Tests**: integration tests through the real router (axum test client) and the real binary for reparse
**Status**: Complete

## Stage 1d: parsers (activities, monitoring, sleep, HRV, metrics)
**Status**: Complete (sleep/HRV/SpO2 unverified on real files until a night capture)
## Stage 1e: Garmin export importer
**Status**: Complete (FIT only; TCX/GPX/JSON counted, not stored — owner decision pending)
## Gate 1
**Status**: Passed 2026-11-19. USB copy of the watch (mass storage, mounted read-only, 319 files, every checksum verified against the watch): 274 FIT files uploaded over HTTP with source=usb, all 201; all parse and derive `ok`; `reparse` (789 109 messages) and `rebuild-ledger` give byte-identical tables; a repeat upload returns 200 and stores nothing. 189 activities (2023-07 to today), no implausible timestamps. Only 2 of the 16 Bluetooth files are byte-identical on the drive (the activity and the Firstbeat backup): the watch cuts monitoring/metrics files per sync and keeps only the current day's growing file on the drive, so per-file hash comparison belongs to gate 2.

### Owner decisions, 2026-11-19 ("do it all")
- VO2 max may be shown as max MET × 3.5, labelled as derived from the watch's maximal_met (not Garmin's published value).
- Oracle check against Garmin Connect: approved and done once, read-only (results: docs/protocol/OPEN-QUESTIONS.md Q-FIT-1/2; values in private/oracle/).
- JSON wellness from the export: import approved in principle; the parser waits for a real export (formats must be seen, not guessed).
- Project licence: still open.

# Phase 2: Android sync
Roles as phase 1. Order follows the brief: the core state machine passes the phase 0 captures on the desktop before anything runs on the phone.

## Stage 2a: sans-IO protocol client in /core
**Goal**: `nimrag_core::client::Client`, our side of the watch link as a pure state machine: multi-link bring-up, MLR, GFDI handshake, time duty, pending-file listing, download with CRC check, archive only after the caller confirms safe storage. Bytes and caller decisions in; GATT writes, events and one deadline out.
**Success Criteria**: (1) the committed synthetic session tests pass through `Client::handle` only; (2) for every session in the three private captures, the replay harness below reports zero differences outside the allowed list; (3) no 5008 is ever emitted without a matching `FileStored`; (4) `core` stays std-only; `cargo clippy` and `cargo fmt --check` clean.
**Tests**: `core/tests/client_synthetic.rs` (committed, fake ids), `core/tests/client_replay_private.rs` (skips with a printed notice when `private/captures` is absent; fails instead when `NIMRAG_REQUIRE_PRIVATE=1`), unit tests per module.
**Status**: Complete (private replay: 5 Connect sessions, 28 files, zero differences outside §3/§5/§10)

### Stage 2a contract

Labels as in PROTOCOL.md: **seen** / **inferred** / **unknown**; **ours** = a free choice of this client. CAP-1 = `2026-11-19-reconnect-sync`, CAP-2 = `…-activity-sync`, CAP-3 = `…-settings-timeformat`; `t=` is seconds in `nimrag-decode --verbose` output. Connect is client `c1`, Explore the other client.

#### 1. API (fixed in `core/src/client/mod.rs`; change it only via the orchestrator)
- One `Client` per GATT connection and per characteristic pair; created at connect, dropped at disconnect. The caller picks the pair (notify `6A4E281x`, write `6A4E282x`), bonds, exchanges MTU, enables the CCCD, then sends `Input::Connected { att_mtu }`. All writes go to that pair's write characteristic as write-without-response, in order. [seen: PROTOCOL §1.1]
- `handle(now, input) -> Step { writes, events, wake_at_mono_ms }`. No IO, no clock reads, no panics on peer data. After `Closed`, every input returns an empty `Step`.
- Time: `Now.mono_ms` drives every timer; the wall fields are used only for the 5052 reply. All timers (MLR ack delay, MLR retransmit, reply deadlines, retry delays) collapse into the single `wake_at_mono_ms`; the caller arms one timer and sends `Input::Tick`. A late Tick is fine; an early one is a no-op.
- Caller decisions: `StartSync`, `FileStored { transfer_id }`, `FileStoreFailed { transfer_id }`. No SHA-256 crosses the boundary: `core` has no hash and could not check it; hashing belongs to the store (2d).
- `ClientConfig` has no defaults in `core` (client id, host strings, app version, capability mask). Tests take them from the capture or use fake values.

#### 2. Transport and MLR (`client/transport.rs`, `client/reliable.rs`)
- Bring-up, strictly request → reply: close-all (any reply status accepted; status 1 seen, meaning unknown Q-ML-1) → register REGISTRATION(4) mode 0 → info pages 0–4 as `[handle, page]` → register GFDI(1) mode 2 → register KEEP_ALIVE(22) mode 0 → write `[ka_handle, 01 19 00 00 00]`. [seen: CAP-1 t=0.86–1.89, both apps; keep-alive payload meaning inferred]. Keep-alive values from the watch (`02 00`, counters) are consumed silently.
- Register replies: status 0 → continue; status 2 PENDING_AUTH → keep waiting [seen APK]; status 3 → `Closed(ChannelInUse)`; other → `Closed(RegisterRejected)`. GFDI without the reliable flag → `Closed(ReliableNotGranted)` (plain mode is Q-MLR-3, not built).
- `MlrEndpoint` (symmetric; the test fake watch uses the same type): header via `mlr::{parse, build}`, receive via `mlr::ReceiveWindow`. Payload per packet `att_mtu − 5`. Each GFDI frame (`cobs::encode_frame`) starts a new packet. Start seq 0 / ack 0. [seen: PROTOCOL §1.4]
- Receive: Accept → deliver, Duplicate / OutOfOrder → drop [seen]; all three schedule an ack [inferred for the two drop cases]. Ack policy: ACK-only packet (2 bytes, seq 0) after 5 unacknowledged data packets or 10 ms after the first; any outgoing data packet carries the ack and cancels it. [seen]
- Send: window fixed at 32 unacknowledged packets [ours; the app grows it to 63]; retransmit timer 2000 ms from the oldest unacknowledged packet, doubling per timeout to 20000 ms, reset to 2000 ms on ack progress; a timeout resends every unacknowledged packet with the current ack; retries never give up [seen, minus the RTT estimator, whose floor is 2000 ms anyway — ours].
- A value on an unknown handle, a malformed control message, control type 4 or 0xFF → `Anomaly(BadTransport)`, nothing sent.

#### 3. GFDI session (`client/session.rs`)
- Our requests use compact ids with a txn counter 0..31, +1 per request, wrapping [seen: CAP-1 t=55.43 wrap]. One request outstanding at a time [ours]. Replies to watch requests are sent at once, in arrival order, with the request's txn if it had one (none seen). A reply deadline of 30 s applies to every awaited answer in every phase; expiry → `Closed(Timeout)` [ours]. A non-ACK status to one of our requests → `Closed(RequestFailed)`.
- Handshake order: wait 5024 → ACK with host info (PROTOCOL §3 step 2; protocol 150) → wait 5050 → bare ACK → our 5050 (`config.capabilities`) → 5031 → 5030 `08 00` HANDSHAKE_COMPLETE → on its ACK emit `HandshakeDone`. [seen: CAP-1 t=2.62–4.77, Explore's order]. Protocol < 150 in 5024 → `Closed(ProtocolTooOld)`.

| Watch → phone | Our answer | Label, evidence |
| --- | --- | --- |
| 5024 | ACK + host info | seen CAP-1 t=2.68 |
| 5050 | bare ACK | seen t=3.76 |
| 5052 | ACK `ref, unix_s − 631065600, utc_offset_s, next_dst, following_dst` (Garmin epoch); emit `TimeSent`. `unix_s` before 2020-01-01 → no reply, `Anomaly(ClockInvalid)` (never set the watch to a bad time) | seen t=5.11, 43.68, 62.67; guard ours |
| 5037 (any option, any bitmask) | bare ACK, then one sync cycle (queued if not `Ready`; at most one queued) | seen t=43.86, 53.50, 62.81; CAP-2 t=1044.68 |
| 5004 | ACK `status, u32 total bytes received` per chunk | seen CAP-2 t=1081.99 (3969), 1082.85 (5157) |
| 5043 | ACK body `u16 request_id` + six zero bytes; no 5044 follows. Several chunks → same ACK + `Anomaly(ChunkedProtobuf)` | seen for requests the app leaves unanswered (CAP-1 t=4.92→5.21); inferred for the rest |
| 5044 (answer to our sync request) | ACK body as above | seen t=62.89 |
| 5101 | nothing; `Closed(AuthRequested)` | seen APK `rq50` |
| any other request (5036, 5042, 5014, 5034, 5009, 5011/5012, 5022, 5023, 5027, 5054, …) | 5000 `id, status 2`, no body; emit `Unsupported` | inferred (APK auto-reply; status value never seen on the wire, Q-GFDI-5) |
| 5000 matching no request; bad COBS / length / CRC | `Anomaly`, nothing sent | ours |

- Deliberately not sent (each is on the allowed-differences list): info page 5; NFC and real-time service registrations, and the app's later close-handle (control type 2) for those same handles with the watch's close-handle responses (type 3); 5043 FeatureCapabilitiesRequest; app-initiated 5043 protobuf requests on Smart field 21 (music) and field 42 (RealtimeSettings: settings screens, a later companion duty) together with the watch's ACK/5044 replies to them; 5026 settings 6/7/8 (Explore omits it too); 5030 HOST_DID_ENTER_FOREGROUND; 5033/5035/5049; every 5044 answer to a watch protobuf request, including the cloud HTTP proxy; the 255/19 upload (5005/5003/5004); weather 5011/5012.

#### 4. Sync cycle and safety (`client/sync.rs`)
- Trigger: a watch 5037, or `StartSync`, which sends 5043 with `Smart.core(13){1{1=0}}` (header `request_id, offset 0, total 6, chunk 6`; request ids count from 0) and then waits for the watch's 5037 [seen: CAP-1 t=62.56–62.71]. No 5037 within 30 s → `Anomaly(SyncNotGranted)`, back to `Ready`, no listing.
- Cycle: 5007 `03` → 5002 index 0 (`index, offset 0, type 1, crc_seed 0, max_size 0, flags 01`) → parse directory (16-byte header skipped; `DirEntry`) → emit `Directory` → for each entry in directory order: download → wait for the caller → archive → next → finally 5030 `00 00` SYNC_COMPLETE, also after failures and for an empty listing → `SyncFinished`. [seen: CAP-1 t=44.04–57.40; CAP-2 t=1076.54–1098.20; failure case inferred]
- Download policy: only entries with `data_type == 128` and flag READ (0x80); others → `FileFailed(Skipped)` [inferred from "FIT sub-types only", PROTOCOL §5.6]. 5002 reply status 3 → retry after 1 s, 5 times [seen APK]; other non-zero → `FileFailed(Refused)`.
- Check per chunk: `offset == bytes so far` and `crc::update(running, data) == chunk crc`; at the end `total == size from the 5002 reply`. Good chunk → ACK status 0. CRC or size mismatch → ACK status 3; offset mismatch → ACK status 2; then the partial data is dropped and the file is requested again from offset 0, once per cycle [statuses seen APK `yw40`, never on the wire; retry count ours]. Second failure → `FileFailed { will_retry: false }`, no archive; the file stays pending for the next sync.
- **Archive rule.** 5008 `index, 10` is sent only when all hold: the file was delivered by `FileDownloaded` in this cycle of this `Client`; the client is waiting on exactly that `transfer_id`; the caller sent `FileStored` for it. While waiting, the file lane is frozen (no other 5002/5008, no timeout: the caller owns that deadline) but watch requests are still answered. `FileStoreFailed` → `FileFailed(StoreFailed)`, next file. Any other `transfer_id` → `StaleConfirmation`, nothing sent. The directory is never archived. 5008 reply: ACK with first body byte 0 → `FileArchived`; else `FileFailed(ArchiveRefused)` (rest of the body is unknown, Q-GFDI-2).
- Disconnect: `Closed(Disconnected)`; partial downloads are discarded and never delivered; pending confirmations become void. No resume at an offset [seen: the app never does]. Idempotence across reconnects comes from the watch re-listing unarchived files (inferred; Q-SYNC-3) and the store deduplicating by content hash (2d); indices are reused by the watch (index 89 held file numbers 144, 145, 146 in CAP-1/2), so an index is never carried from one connection to the next.

#### 5. Replay harness = "passes the phase 0 fixtures"
`core/tests/support/` holds `FakeWatch` (scripted peer: `MlrEndpoint` + public codecs) and `transcript.rs`. Both test files drive only `Client::handle`.
1. Decode a capture with `capture::{parse_btsnoop, L2capReassembler, parse_att}` and `link::LinkDecoder`; split by `Link`; one replay per Connect session found (a session starts at each close-all; CAP-2 has a second one from t≈698). Config = that session's client id, host strings, app version and 5050 bytes.
2. Transcript = that link's ordered items (control, info page, service data, GFDI message) with capture timestamps. Each watch → phone item is a **reply** to an app item (register response ↔ register by service; info page ↔ page; 5000 ↔ request id + txn; 5004 chunks ↔ their 5002; keep-alive `02 00` ↔ `01 19…`; 5044 ↔ protobuf request id) or **unsolicited**.
3. Drive with the capture's clock. Unsolicited items are delivered at their timestamp. A reply is delivered once the client has emitted the matching request, after the captured latency, with txn and protobuf request id rewritten to the client's. Replies to app requests the client never makes are dropped; that request must be on the not-sent list of §3, else fail. A client request with no reply in the transcript → fail. The app's manual-sync 5043 becomes `Input::StartSync` at its timestamp. `FileDownloaded` is answered at once with `FileStored`. The fake watch re-frames everything through its own `MlrEndpoint` (the captured MLR seq/ack numbers belong to the app's traffic and cannot be replayed raw).
4. Compare, per lane, the client's phone → watch items with the app's, as exact sequences of (id, payload) after normalising txn and protobuf request id:
   - **bring-up**: control and info writes, byte-exact; GFDI/KEEP_ALIVE register order free;
   - **handshake**: ACK(5024), ACK(5050), 5050, 5031, 5030 HANDSHAKE_COMPLETE;
   - **time**: one reply per 5052; reference, offset and DST fields equal; time within ±2 s of the app's;
   - **sync**: ACK(5037), 5007, 5002, ACK(5004), 5008, 5030 SYNC_COMPLETE; and every downloaded file byte-identical to `LinkDecoder`'s `FileDownloaded`;
   - **protobuf**: the manual-sync 5043; one ACK per watch 5043 and per 5044.
5. Allowed differences, complete list (the test encodes it as data and prints which were used): the not-sent list of §3; txn and protobuf request ids; time within ±2 s; order between lanes; order of the GFDI and KEEP_ALIVE registrations; the last two bytes of a protobuf ACK body where the app sent non-zero (Q-GFDI-3, CAP-1 t=5.42); `Unsupported` replies (status 2, no body) where the app ACKed 5036 / 5042 / 5014 / 5034 with a body; watch 5037s that followed the app's foreground event are still delivered. Anything else is a failure; the builder does not extend this list.
- Synthetic fixture: the same harness over a transcript built in code with fake ids (unit id, names, client id), one multi-chunk file larger than `max_packet` at MTU 23 (more than 64 MLR packets, so seq wraps). Nothing derived from private captures is committed except protocol constants already in PROTOCOL.md.

#### 6. Tests (all through `Client::handle` unless marked unit)
1. Happy path: bring-up → handshake → 5037 → two files → stored → archived → SYNC_COMPLETE → 5037 INVISIBLE → empty listing → SYNC_COMPLETE; the full phone → watch GFDI sequence equals a golden list.
2. Seam check: the whole synthetic exchange (both directions) fed to `LinkDecoder` gives no `Unknown`, `GfdiError` or `MlrOutOfOrder`, and its files equal the client's.
3. No 5008 before `FileStored`: confirmation withheld while time advances and a 5052 is answered; then `FileStoreFailed` → next file, still no 5008 for it.
4. `StaleConfirmation`: unknown id, repeated id, id from before a disconnect (new `Client`) → no write.
5. CRC mismatch once → ACK status 3, second attempt archived; twice → `FileFailed`, no 5008, SYNC_COMPLETE still sent. Offset mismatch → status 2. Size mismatch.
6. Disconnect mid-download → `Closed`, no `FileDownloaded`; a new `Client` starts at seq 0 / txn 0, lists and downloads the file again.
7. Time: reply fields; `ClockInvalid` sends nothing.
8. Unsupported and auth: 5036 / 5014 / an unlisted id → status 2; 5101 → `Closed`, no write.
9. `StartSync`: protobuf bytes, 5037 MANUAL, ACK of 5044; no 5037 → `SyncNotGranted`.
10. MLR (unit + through the client): lost phone → watch packet retransmitted at 2 s, 4 s, … capped at 20 s with the current ack; gap from the watch delivers nothing until retransmission; ACK-only after 5 packets and after 10 ms; window of 32; seq wrap.
11. Failures: register status 3, GFDI without reliable flag, 30 s silence in each phase, NAK to 5007.
12. Robustness: 10 000 pseudo-random notification values (fixed-seed LCG) in each phase never panic.
13. Private replay (§5) for all sessions of CAP-1/2/3.

#### 7. Layout, reuse, scope
- New: `core/src/client/{mod,transport,reliable,session,sync}.rs`, `core/src/messages.rs` (typed build/parse for 5024, host info, 5031 reply, 5037, 5052, 5002 + reply, 5004 + ACK, 5008, 5043/5044 header). Where `describe.rs` or `link.rs` already parse one of these, move the parser into `messages.rs` and call it from there: one implementation per layer. `DirEntry` moves the same way (re-export from `describe`).
- Reuse unchanged: `crc`, `cobs::{encode_frame, FrameDecoder}`, `gfdi::{Message, Status}`, `multilink::{parse_control, register_request, close_all_request, Control}`, `mlr::{Header, parse, build, ReceiveWindow}`; `capture` and `link::LinkDecoder` in tests only.
- Not in 2a: BLE, GATT discovery, bonding, pairing (Q-PAIR-1), Android, UniFFI bindings, storage, hashing, upload queue, notifications, weather, GPS assist, settings, Garmin Auth, plain (non-MLR) GFDI, 5054 decompression, uploads to the watch, changes to `tools/decode` output.
- Licence: implement from PROTOCOL.md and the captures only; do not open Gadgetbridge.

#### 8. Stop and report; do not guess
1. Any replay difference outside §5's list, or a transcript item the harness cannot classify.
2. A register status, flag or control type this contract does not name, in a capture.
3. 5054, a chunked protobuf, a non-ACK status, or a 5008 reply body that breaks the "index − 1" pattern, in a capture.
4. 5052 for a zone without DST transitions (encoding unknown): `Now` requires both fields; do not invent a sentinel.
5. Need for a default capability mask, client id or app version in `core`.
6. Anything that would require copying capture bytes containing ids, names, notification text or cloud answers into a committed file.
7. A conflict between this contract and PROTOCOL.md: PROTOCOL.md and the captures win; report it.

#### 9. Orchestrator decisions on the advisor's open points (2026-11-19)
- Capability mask, client id, app version, host strings: config only, no defaults in `core`. 2c starts with Explore's 5050 set (seen working; weather 5014 then gets status 2) and confirms it live.
- 5043 FeatureCapabilitiesRequest: not sent in 2a. 2c checks live whether the watch needs it; if so, Explore's body (no account GUID).
- Unhandled watch requests → 5000 status 2 (inferred, Q-GFDI-5); watch protobuf requests → 8-byte ACK, no 5044.
- 5002 flags 0x01 as the app sends; a 5054 reply → `FileFailed`, no archive.
- No SHA-256 in `FileStored`: the store (2d) owns hashing and its own audit log.
- 5052 without DST transitions: out of scope (owner's zone has DST); stop rule 4 stands.
- MLR: fixed window 32, no RTT estimator, 30 s reply deadline closes the session.
- Committing a scrubbed message-level transcript of a real capture: owner decision, not now.

#### 10. Orchestrator decisions after the advisor code review (2026-11-19)
- **Transfer ids unique across connections (blocker B1).** `ClientConfig` gains `connection_epoch: u32`; the caller gives every `Client` a new value (an in-process counter is enough: stale confirmations can only come from the same process). `transfer_id` becomes `u64 = (epoch << 32) | n`. A confirmation carrying another epoch → `StaleConfirmation`, nothing sent.
- **Per-file failure instead of closing (S2).** A non-ACK status to 5002 → `FileFailed(Refused)`; to 5008 → `FileFailed(ArchiveRefused)`; both continue with the next file. Non-ACK to 5007, a handshake request or any bring-up step still closes. Reply silence (30 s) still closes in every phase (the link is probably gone); a file that makes the watch go silent every time is a known risk to watch in 2c.
- **5054 (S1).** A 5054 while a download is active fails that file (`FileFailed`, no archive) and the cycle continues; outside a download → `Anomaly`. 5002 flags stay 0x01.
- **Allowed differences (S4).** "NFC and real-time service registrations" explicitly includes the app's data on those handles. 5026 only for settings 6/7/8. Control type 4, decoder `Unknown`, `MlrOutOfOrder` and anything else unnamed → unclassified (fail).
- **Replay cannot pass empty (S3).** If `private/captures` exists, all three captures must load, sessions per capture must be 1/2/2, files replayed > 0, and a client `Anomaly` fails the session.
- **Nits accepted:** input applied before timers; caller inputs never extend the reply deadline; empty 5004 chunks are not progress; saturating arithmetic on caller times; a duplicate ACK-only restarts the retransmit timer (PROTOCOL §1.4); `link.rs` tracks a 5002 as leniently as before; `FileType` lives in `messages` (re-exported by `client`). Kept as is, to check live in 2c: a stray 5004 gets ACK status 2.

## Stage 2b: Android build of /core (cargo-ndk, UniFFI, Gradle module)
**Toolchain (installed 2026-11-19, owner approved incl. SDK licences)**: `ANDROID_HOME=~/opt/android-sdk` (cmdline-tools 23.0, checksum-verified; platform-tools 37.0.1, build-tools 37.0.0, platforms/android-37.2, NDK 30.0.16248370), Rust targets aarch64/x86_64-linux-android, cargo-ndk 4.1.2, JDK 25 at ~/opt/rea-engines/jdk25. `cargo ndk -t arm64-v8a -t x86_64 build -p nimrag-core` builds. UniFFI goes in a separate FFI crate so `core` stays std-only.
**Goal**: `ffi/` crate `nimrag-ffi` (workspace member) exposing `nimrag_core::client` through UniFFI (proc-macros; `core` stays std-only); `android/` Gradle project with `:core` (Android library: generated Kotlin bindings + `.so` for arm64-v8a and x86_64 built by cargo-ndk from Gradle) and a minimal `:app` shell for 2c.
**Success Criteria**: `./gradlew :core:assembleRelease :app:assembleDebug` builds from a clean checkout; the APK contains `libnimrag_ffi.so` for both ABIs and no FIT profile data (built with `NIMRAG_FIT_PROFILE=none`, enforced in the Gradle task); Kotlin can drive a `Client` end to end.
**Tests**: Rust tests in `ffi/` for the conversion layer; a host JVM test (bindings + the x86_64-linux `.so` via JNA) that drives a `Client` through Kotlin with fake ids: Connected → close-all write; a scripted bring-up/handshake reaching `HandshakeDone`; a FileStored for a stale transfer id gives `StaleConfirmation`. Optional instrumented smoke test on the Pixel.
**Status**: In Progress

## Stage 2c: Android foreground service, BLE link, pairing without the official app — needs owner (watch re-pairing)
**Status**: Not Started

## Stage 2d: sync to phone storage, mark synced only after safe storage; upload queue (persistent, backoff, Wi-Fi/VPN only, dedupe by hash)
**Status**: Not Started

## Stage 2e: time duty, Devices and sync screen
**Status**: Not Started

### Advisor notes for stage 1d (parsers), recorded 2026-11-19
- timestamp_16 / timestamp_min_8 (monitoring) and event_timestamp_12 (hr) accumulate against the last full timestamp; timestamps < 0x10000000 are device-relative, not UTC → show as unavailable.
- Cross-file duplicates (watch vs export, overlapping monitoring): derived entities need natural keys (e.g. file_id serial + time_created; (timestamp, metric)) and a precedence rule independent of ingest order.
- Expansion of components/subfields and accumulation live in a pure `fit::interpret` layer; the decoder stays raw.
- Phase 4 encoder: separate `fit::encode` with typed inputs; needs BaseType::{size, invalid, to_byte}.

## Stage 1d contract (advisor draft, accepted by orchestrator 2026-11-19)

### Architecture
- Core (pure, std-only, reusable on Android): `core/src/derive/{mod,activity,monitoring,sleep,hrv,metrics}.rs`. `pub fn derive(file: &FitFile) -> Result<Derived, DeriveError>`. `Derived` = plain structs, one Vec per row type, plus `FileFacts`. No IO, serde or SQL.
- Message-driven: every parser runs over every file and emits rows for messages it knows; never dispatch on upload metadata (file_id is provenance only).
- Fields read by name through `fit::interpret` (scaled, subfields/components expanded); parsers never re-implement scaling.
- `profile::available()` false → `DeriveError::ProfileUnavailable`; never emit unscaled guesses.
- Absent/invalid field → None; a row with no usable value is not emitted. No interpolation, differencing or estimation.
- Timestamps: i64 unix seconds UTC (FIT + 631065600). Device-relative → row skipped and counted.
- Server: `server/src/derived/{mod,activity,wellness}.rs` persists Derived. Table `file_derive(sha256 PK, derive_version, status ok|partial|error|no_profile, error, skipped_rows)` STRICT. `serve` and `reparse` log a loud warning when the profile is unavailable.
- Units: SI as REAL (m, m/s, s, kg, bpm, degrees). Lat/long degrees = semicircles × 180 / 2^31 (unit conversion only).

### Duplicates and precedence
- The watch re-sends a growing monitoring file (file N at one sync is a prefix of file N at the next) → "more complete file wins".
- `sources(sha256 PK, src_id INTEGER UNIQUE, ok INTEGER, last_ts INTEGER, msg_count INTEGER, serial INTEGER, time_created INTEGER, file_type INTEGER)` STRICT, all from the bytes (FileFacts). src_id = first 8 bytes of sha256 as positive i64. ok = all chain files complete and CRC ok.
- Rank "A outranks B": ok desc, last_ts desc, msg_count desc, sha256 asc. Never upload source, received_at or ingest order.
- Entity tables (activities, sleep sessions): keyed by sha256, one row set per file, never overwritten; view `*_current` picks the best-ranked file per natural key; per-file delete + reinsert.
- Sample tables (time series): keyed by natural key (ts, …) with `src INTEGER` = src_id; upsert `ON CONFLICT DO UPDATE` only when the new source outranks the stored one. Reparse of sample tables: one transaction clearing all sample tables, then upsert every file in sha256 order.
- Required, tested: any ingest order and reparse give byte-identical dumps.

### Activity tables (builder A) — STRICT, WITHOUT ROWID, FK sha256 → files; [message.field]
- `activities(sha256 PK, activity_key TEXT, serial, time_created, start_ts, local_offset_s, total_timer_s, num_sessions, sport, sub_sport)`; activity_key = "<serial>:<time_created>" [file_id]; [activity.timestamp, local_timestamp (offset = local − utc), total_timer_time, num_sessions]; sport/sub_sport/start_ts from first session. View `activities_current`.
- `activity_sessions(sha256, idx, PK(sha256, idx), start_ts, sport, sub_sport, elapsed_s, timer_s, distance_m, calories, avg_hr, max_hr, avg_speed, max_speed, ascent_m, descent_m, avg_cadence, aerobic_te, anaerobic_te, training_load)` [session.*: enhanced_* speed preferred; total_training_effect, total_anaerobic_training_effect, training_load_peak].
- `activity_laps(sha256, idx, …)`: session columns minus TE/load, from lap.*.
- `activity_records(sha256, idx, PK(sha256, idx), ts, lat, lon, hr, speed, altitude_m, distance_m, cadence, temperature_c)` [record.*, enhanced_* preferred]. Pace not stored (UI derives from speed). idx = ordinal among messages of that type.
- `strength_sets(sha256, idx, PK, ts, start_ts, duration_s, set_type, reps, weight_kg, category, category_subtype, category_all, wkt_step_index)` [set.*; set.timestamp is field 254 — check profile]. category/subtype arrays: first valid element as INTEGER plus enum name TEXT when known; >1 valid → full list JSON in category_all. Invalid weight → NULL, never 0.
- `exercise_titles(sha256, idx, category, subtype, name)` [exercise_title.*].
- `activity_time_in_zone(sha256, idx, ref_mesg, ref_index, hr_zone_s JSON)` [time_in_zone.*].
- `activity_metrics(sha256 PK, maximal_met, aerobic_te, anaerobic_te, training_load_peak, epoc, recovery_min)` [firstbeat_results (140)]: only fields the local profile resolves; document chosen names.
- Events not derived in 1d. Multi-sport: activities.sport from first session; UI lists sessions.

### Wellness tables (builder B) — sample tables with src + rank upsert
- `mon_hr(ts PK, bpm, src)` [monitoring.heart_rate; ts from 253 or timestamp_16 via interpret].
- `mon_activity(ts, activity_type, PK(ts, activity_type), steps, distance_m, active_calories, active_time_s, duration_min, intensity, src)` [monitoring.cycles→steps subfield, …]. Cumulative counters stored as reported; no differencing; daily-steps view only after reset behaviour is confirmed on real files.
- `mon_extra(ts PK, ascent_m, descent_m, moderate_min, vigorous_min, src)` [monitoring fields 35–38 by local profile names]; floors/intensity minutes only if a watch-written field exists; never ascent/3.048.
- `stress(ts PK, stress, stress_raw, body_battery, body_battery_status, src)` [stress_level.*; ts = stress_level_time]; negative stress = status code → stress NULL, stress_raw kept.
- `respiration(ts PK, brpm, brpm_raw, src)` (non-positive → NULL + raw); `spo2(ts PK, percent, confidence, mode, src)` [spo2_data]; `resting_hr(ts PK, rhr, current_day_rhr, src)` [monitoring_hr_data 211]; `mon_info(sha256 PK, local_offset_s, rmr_kcal_day, steps_goal)` [monitoring_info] entity.
- Sleep/HRV (not in captures yet → synthetic tests, modules marked "unverified on real files"): `sleep_levels(ts PK, level, level_name, src)`; `sleep_sessions(sha256 PK, start_ts, end_ts, overall_score, quality_score, duration_score, recovery_score, deep_score, light_score, rem_score, awakenings_count, avg_stress)` [sleep_assessment]; start/end only from explicit watch fields (else NULL); view `sleep_sessions_current`. `hrv_values(ts PK, rmssd_ms, src)`; `hrv_summary(ts PK, weekly_avg, last_night_avg, last_night_5min_high, baseline_low_upper, baseline_balanced_lower, baseline_balanced_upper, status, src)`.
- Metrics (real 128/44 files): `training_load(ts PK, acute, chronic, acwr, acwr_percent, status, src)` [acute_load_results 378]; `training_status(ts PK, status, weekly_load_sum, load_min, load_max, fitness_trend, load_trend, src)` [training_history_results 232]; `training_balance(ts PK, aerobic_low, aerobic_high, anaerobic, targets…, src)` [294]; `recovery_time(start_ts PK, minutes, src)` [284, key recovery_time_start]; vo2max: store maximal_met as written (MET×3.5 conversion is an owner/UI decision).

### Split and tests
- Scaffolding (builder-server first): `core/src/derive/mod.rs` (Derived, FileFacts, DeriveError, row structs), migration `0002_derive_shared.sql` (sources, file_derive), rank helper in `server/src/derived/mod.rs`, shared synthetic-FIT test builder moved into a test-support module. Then A: activity (`core/src/derive/activity.rs`, `server/src/derived/activity.rs`, `0003_activity.sql`). B: wellness (`core/src/derive/{monitoring,sleep,hrv,metrics}.rs`, `server/src/derived/wellness.rs`, `0004_wellness.sql`).
- Tests: per-parser synthetic under cfg(fit_profile); empty profile → ProfileUnavailable, file_derive no_profile, raw tables still filled; server integration through the real router; precedence (prefix-then-longer and reverse give identical sample tables; bad-CRC loses); determinism (two shuffled ingest orders, reparse, reparse after drop → identical dumps, real binary); private replay (status ok, row counts, zero unexpected skips); oracle check against Garmin Connect for one day (lead, later, with owner approval).
