# nimrag implementation plan

Source of truth for scope: the owner's build prompt (phases 0–4, hard rules, decisions, design system, engineering standards) and the phase 0 brief "map the Garmin Connect ↔ Instinct 2 BLE sync protocol". Each phase stops at its gate for an owner report. Workflow: AI-assisted. An integrator plans each stage and commits; AI implementers build on separate paths and never commit; an independent AI reviewer drafts contracts and reviews finished work before each commit; every commit is verified by the integrator (tests in both profile modes, real-data checks); the owner makes the product decisions.

## Roadmap: phase 0 to finished product

The finished product: an Android app that pairs with the Instinct 2 without Garmin Connect, pulls every file, and uploads it to a self-hosted server; the server keeps the original files (SHA-256), derives everything else from them, and serves our own web interface (also inside the app); workouts can be sent to the watch; backups, export and a runbook exist. Nothing talks to Garmin except the optional GPS-assist duty.

| Phase | Stage | What | Status |
| --- | --- | --- | --- |
| 0 Protocol | 0a | Static protocol map (`docs/protocol/`) | Complete |
| | 0b | Live captures: reconnect, activity, settings | Complete |
| | 0b | Live captures: night sync, fresh pairing | Night: no longer needed for the data (the app's own syncs bring the night files; sleep stages match the watch to the minute, 2026-11-20). Fresh pairing (a watch with no bond): **open** |
| | 0c | `/core` framing + `tools/decode` | Complete |
| | Gate 0 | Report | Reported 2026-11-19 |
| 1 Server | 1a | FIT decoder | Complete |
| | 1b | FIT profile (generated locally, never committed) | Complete |
| | 1c | Server ingest, store, tokens, `reparse` | Complete |
| | 1d | Parsers: activities, monitoring, sleep, HRV, metrics | Complete; sleep verified on real nights (2026-11-20); cumulative counters read per the interval they close (2026-11-21); SpO2 not yet seen in real data |
| | 1e | Garmin export importer | Complete (FIT only) |
| | Gate 1 | USB copy uploads, parses, reparses identically | **Passed** 2026-11-19 |
| 2 Android sync | 2a | sans-IO protocol client + capture replay | Complete |
| | 2p | Port Gadgetbridge's Garmin work: survey, device catalogue (≈197 models), protocol diff against our captures | Mostly done: port list, protocol diff, catalogue (193 models + sync status), battery; sleep stages wait for the night capture |
| | 2b | FFI crate (UniFFI) + Android Gradle project | Complete |
| | 2c | Foreground service, BLE link, pairing without Garmin Connect | Live since 2026-11-19 (0.2.3) on the owner's Pixel 9a with the Instinct 2; pairing reuses an existing bond. Not tried: a watch with no bond |
| | 2d | Phone storage, archive only after safe storage, upload queue | Live: files stored, archived after storage and uploaded within seconds to the deployed server (since 2026-11-20) |
| | 2e | Time duty + native Devices and sync screen + sync log | Live: time sent every sync, shareable sync log; Watch tab redesigned in 0.6.0 |
| | 2f | Further companion duties, one at a time, ported from Gadgetbridge where it has them: GPS assist (setting, the only Garmin call), weather, notifications, music, find my phone | Not Started |
| | 2g | Garmin Connect too, for owners who want both: **Alongside** (phone side, both apps share the watch, Nimrag copies, Connect archives) now; **Forward** (server uploads activities to Garmin Connect), approved, after 3a | Alongside: built, live test pending; Forward: server and web built, tested against a fake Garmin only |
| | Gate 2 | A week of normal wear: no manual steps, no lost or duplicated files; counts and hashes against a USB copy | **In progress**: syncing unattended every 15 min since 2026-11-20; the week ends about 2026-11-27, then the USB comparison |
| 3 Web | 3a | JSON API, web login (password + optional passkey), device tokens created in the web UI (since 2026-11-20 issued only by the app's sign-in; the web lists and signs out phones), generated TS types | Done (7eb0ea7; passkeys later) |
| | 3b | Web scaffold: Vite/React/TS, TanStack Query, design system, PWA, embedded in the server binary, Docker image | Done (Docker image builds and serves, 162 MB) |
| | 3c–3m | Pages 1–11 of the brief (Today … Desktop extras) | Done; on real data since the server was deployed (2026-11-20), plus Garmin-style status pages (training status, HRV, sleep), metric detail pages and maps |
| | 3n | Android shell: web view with native bottom bar, offline via PWA cache | Done (0.4.0); in daily use against the deployed server |
| | 3o | Local server inside the app (personal flavor, offline data) | Spike works on the Pixel 8a; not in use |
| | 3p | App sign-in with the accounts (fingerprint or password unlock) | Done (0.5.0); fingerprint unlock not yet verified on a phone |
| | 3q | New data shows up soon: alarm-driven auto sync, refetch after uploads | Done (0.5.1); verified live 2026-11-21 (syncs every 15 min through the night, page refreshes after a sync) |
| | 3r | Live use, 0.6.x: metric detail pages, Watch/Profile redesign, Connect daily history, maps, sync icon, day-total fix | Done (0.6.0–0.6.2, 2026-11-20/21); see "Live use (2026-11-20/21)" |
| | Gate 3 | Every page from real data, "unavailable" shown correctly, accessibility check | **In progress**: pages run on real data; the accessibility check has only run on seeded data |
| 4 Writing + ops | 4a | FIT encoder + workout builder, sent to the watch via the phone (encoder and upload path ported from Gadgetbridge) | Not Started |
| | 4b | Scheduled backups + documented restore test | Not Started |
| | 4c | Export: originals + CSV per data type | Not Started |
| | 4d | Runbook: install, upgrade, restore, re-pair, firmware over USB | Not Started |
| | Gate 4 | Final report | — |

Every phase ends with an update to `docs/ARCHITECTURE.md` and a gate report (engineering standards).

**Licence and reuse (owner decision, 2026-11-19):** Nimrag Connect is AGPL-3.0-or-later and ports Gadgetbridge's Garmin work instead of decoding every device from scratch. Rules: `docs/LICENSING.md` (attribution header per file, `docs/PORTED.md` register, ported behaviour labelled *inferred (Gadgetbridge)* until verified on a device, no Garmin FIT profile tables, no artwork).

# Phase 0: protocol (static map + captures)

## Stage 0a: Static protocol map (RE brief)
**Goal**: `docs/protocol/` deliverables: PROTOCOL.md, messages.csv, filetypes.csv, proto/, HOOKS.md, OPEN-QUESTIONS.md, PROVENANCE.md
**Success Criteria**: brief's "done when": transport, framing, session start and file sync have no unknowns, or each has a concrete phase 2 test; must-have file types identified; HOOKS.md has raw write/notify and dispatch
**Tests**: n/a (documentation); every "seen" claim cites class/method + anchor constant
**Status**: Complete

## Stage 0b: Live captures (HCI snoop)
**Goal**: snoop logs for fresh pairing, normal sync, sync after activity, sync after night, settings change
**Success Criteria**: logs stored under `private/captures/` (git-ignored), scrubbed copies under `fixtures/`
**Tests**: n/a
**Status**: Partly complete. Done: CAP-1 reconnect + manual sync, CAP-2 activity sync, CAP-3 settings change (all decoded with 0 unknown frames). Sync after a night: no longer needed for the data, since the app's own syncs bring the night files and the sleep stages match the watch to the minute (2026-11-20). Open: fresh pairing of a watch with no bond (needs the owner). Scrubbed fixtures are synthetic, built in code.

## Stage 0c: /core framing + tools/decode
**Goal**: sans-IO frame codec (COBS, CRC-16, message ids) in /core; CLI decodes snoop logs / live adb stream
**Success Criteria**: decodes every capture with no unknown frames, or lists unknowns in OPEN-QUESTIONS.md
**Tests**: codec unit tests; fixture replay integration test through the CLI entry point
**Status**: Complete

## Gate 0
**Status**: Reported to the owner 2026-11-19; phase 1 started at the owner's request while the night and pairing captures remain open.

# Phase 1: server ingest and parsing (started 2026-11-19 at owner's request; phase 0 still awaits night + fresh-pairing captures)

Workflow as described at the top of this file.

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
**Status**: Complete. Sleep verified on real nights from the app (2026-11-20: a `sleep_levels` row is the stage of the period ending at its ts); cumulative monitoring counters count for the day of `ts - 1` (2026-11-21, the watch stamps the finished day's totals at 00:00); SpO2 not yet seen in real files.
## Stage 1e: Garmin export importer
**Status**: Complete (FIT only; TCX/GPX/JSON counted, not stored — owner decision pending)
## Gate 1
**Status**: Passed 2026-11-19. USB copy of the watch (mass storage, mounted read-only, 319 files, every checksum verified against the watch): 274 FIT files uploaded over HTTP with source=usb, all 201; all parse and derive `ok`; `reparse` (789 109 messages) and `rebuild-ledger` give byte-identical tables; a repeat upload returns 200 and stores nothing. 189 activities (2023-07 to today), no implausible timestamps. Only 2 of the 16 Bluetooth files are byte-identical on the drive (the activity and the Firstbeat backup): the watch cuts monitoring/metrics files per sync and keeps only the current day's growing file on the drive, so per-file hash comparison belongs to gate 2.

### Owner decisions, 2026-11-19 ("do it all")
- VO2 max may be shown as max MET × 3.5, labelled as derived from the watch's maximal_met (not Garmin's published value).
- Oracle check against Garmin Connect: approved and done once, read-only (results: docs/protocol/OPEN-QUESTIONS.md Q-FIT-1/2; values in private/oracle/).
- JSON wellness from the export: import approved in principle; the parser waits for a real export (formats must be seen, not guessed).
- Project licence: AGPL-3.0-or-later, porting from Gadgetbridge (decided later the same day; see docs/LICENSING.md).

# Phase 2: Android sync

## Where work stopped (updated 2026-11-21)
- **Now:** the owner's Instinct 2 syncs to their Pixel 9a every 15 minutes, unattended, and the files reach the deployed server within seconds. App 0.6.2 is the current pre-release. Phase 3 is in daily use; Gate 2's week of wear runs until about 2026-11-27. Details of the last two days: "Live use (2026-11-20/21)" at the end of the plan.
- **Next candidates (owner to pick):** scheduled backups (4b); data gaps (19 Nov transition day, intraday Connect data for recent days, SpO2); companion duties (2f); workouts to the watch (4a); accessibility pass for Gate 3.
- History of this section, oldest first:
- Done and committed: phase 0 decoding, phase 1 (gate 1 passed on a USB copy), stage 2a (protocol client), 2b (Android build of core), most of 2p (Gadgetbridge port list, protocol diff, device catalogue with sync status, battery parser), the 5052 DST fix (Q-TIME-2), every sleep summary kept.
- Next: the owner tests debug 0.2.0 (stages 2c–2e built: pairing through the companion-device dialog, link, sync, storage, uploads, screens) with the watch; fix what the live test shows. Pairing replaces the Garmin Connect pairing. Rows 16 (first-connect events), 8 (plain GFDI, reopen after a watch-side close), 22 (5009 FileAvailable) and 28 (battery: 5023 and `Input::RequestBattery`) are ported (2026-11-19, second machine without `private/` captures: the private replay was skipped for all four, so run it before 2c; neither 5009 nor 5023 occurs in the captures).
- 0.2.1 (2026-11-19): the owner's first try on their own phone (Garmin Connect installed and connected) did not pair; no log yet. 0.2.1 reuses an existing Bluetooth bond and never removes one it did not create (pairing failure or Forget used to remove Garmin Connect's bond), shows why pairing failed, explains the Garmin Connect clash with a Force-stop route, adds Share log, and uses the repo logo as launcher icon. Open: does the watch accept us over Garmin Connect's bond with Connect force-stopped, and does `first_connect` on a reused bond do harm.
- 0.2.2 (2026-11-19): Force stop does not keep Garmin Connect away (the system restarts it when the watch is near). The Watch tab now reads Garmin Connect's BLUETOOTH_CONNECT grant and tells the owner to turn off its Nearby devices permission instead; a second card confirms when it is off. New flat two-colour logo (NIMRAG band over a sync arrow) in the app, docs/assets/logo.svg and the banner.
- 0.2.3 (2026-11-19): the owner's log (Pixel 9a, Android 17) showed the companion dialog ending "declined" after its 20 s search, which aborted pairing; with the association skipped (21:55) the existing bond was reused and the link reached "Connected, setting up the link" before Forget was tapped. The association is now optional (pairing continues), the request also carries a classic filter so the dialog lists the already-bonded watch at once, and the reused-bond decision is made before anything can fail (a failed pairing tried to remove Garmin Connect's bond; Android 17 refused).
- **First live success (2026-11-19 22:30, 0.2.3, Pixel 9a, Instinct 2 firmware 17.22):** companion association created, Garmin Connect's bond reused (its Nearby devices permission off), handshake in 2 s, `first_connect` accepted, battery read, two syncs: 4 files (128/32 monitoring, 128/58 device) stored, archived on the watch, 0 failed. MTU 23 as with Connect. Not yet tried live: uploads to a server, reconnect after the watch leaves range, restart after a process death, a large activity file, giving the watch back to Garmin Connect.
- 0.3.0 (stage 2g, Alongside): Settings → Garmin Connect "Nimrag only" / "Alongside". Alongside never sends 5008, remembers copied files (`copied` table, 60-day window) and skips them on later connections; Garmin Connect keeps its Nearby devices permission and archives. Built and tested here (362 Rust tests, Android unit/host tests, lint); not yet tried live, and the race where Garmin Connect archives a file before Nimrag lists it is still to be measured.
- 0.4.2 (2026-11-20, link stalls): the owner's 0.4.1 log after an app update showed "Connected" and never "MTU", every 20 s for 40 minutes. Not reproducible here (no watch). The link had one 20 s deadline for the whole setup, no word on which step stalled, closed the old client straight after `disconnect()` (a half-closed link could survive and be reused), and an unanswered `requestMtu` blocked everything although MTU 23 is what the watch gives anyway. Now: a deadline per step (MTU 4 s and then skipped, discovery 12 s, notifications 8 s) and "Link stalled at <step>" in the log; the old client is closed only after its disconnect callback (or 1.5 s); after 2 stalls the GATT cache is refreshed (hidden `BluetoothGatt.refresh`, guarded), after 3 the connection is direct instead of `autoConnect`; one start at a time for the watch service (`ServiceStartGate`: app update + process start + every app resume used to start it again and again, logged as "permissions changed"); the system's presence callback ends a reconnect delay at once. Logic in `engine/logic/LinkSupervisor.kt` and `ServiceStartGate.kt` with JVM tests. Not verified: any of it against a watch.
- Night of 2026-11-19: phase 3 built. 3a API and owner login (hardened after a security review), 3b web scaffold, pages 3c–3m wired to the API, 3n Android shell (0.4.0), Forward on the server and in web Settings (fake Garmin only). Not yet: a deployed server with real synced data, Forward against real Garmin, passkeys, Gate 3 on real data.
- Waiting for data: the night-sync capture (sleep stages, port-list row 42; the live recorder was left running) and the fresh-pairing capture.
- Owner decisions still open: committing a scrubbed real transcript as a fixture; importing non-FIT export JSON once a real export exists; for 2f, who fetches GPS-assist data, which weather source, and confirming the skipped features.
- 0.6.0 (2026-11-20, evening): value axes on every chart; metric detail pages behind the Today tiles (`/metrics/<key>`, tabs per metric, rolling 7/28/84/182/364-day windows, weekly buckets past 4w) built from a clean-room spec (`docs/design/clean-room/metric-detail-spec.md`; the analyst's full copy with evidence stays in `private/re/`); the Watch tab is one page with sub-pages instead of Watch/Sync/Settings sub-tabs, and the web avatar opens a Profile page (`/profile`, `/settings` redirects; `/devices` is the web Watch page), per `docs/design/settings-redesign/`. Checked in the app on the Samsung (no watch there): paired, syncing and banner states of the new Watch page are seen only in Compose previews.
- The Samsung on adb may be used for app checks when the owner says so (2026-11-20); the Pixel 9a is the phone.

## Stage 2a: sans-IO protocol client in /core
**Goal**: `nimrag_core::client::Client`, our side of the watch link as a pure state machine: multi-link bring-up, MLR, GFDI handshake, time duty, pending-file listing, download with CRC check, archive only after the caller confirms safe storage. Bytes and caller decisions in; GATT writes, events and one deadline out.
**Success Criteria**: (1) the committed synthetic session tests pass through `Client::handle` only; (2) for every session in the three private captures, the replay harness below reports zero differences outside the allowed list; (3) no 5008 is ever emitted without a matching `FileStored`; (4) `core` stays std-only; `cargo clippy` and `cargo fmt --check` clean.
**Tests**: `core/tests/client_synthetic.rs` (committed, fake ids), `core/tests/client_replay_private.rs` (skips with a printed notice when `private/captures` is absent; fails instead when `NIMRAG_REQUIRE_PRIVATE=1`), unit tests per module.
**Status**: Complete (private replay: 5 Connect sessions, 28 files, zero differences outside §3/§5/§10)

### Stage 2a contract

Labels as in PROTOCOL.md: **seen** / **inferred** / **unknown**; **ours** = a free choice of this client. CAP-1 = `2026-11-19-reconnect-sync`, CAP-2 = `…-activity-sync`, CAP-3 = `…-settings-timeformat`; `t=` is seconds in `nimrag-decode --verbose` output. Connect is client `c1`, Explore the other client.

#### 1. API (fixed in `core/src/client/mod.rs`; change it only via the integrator)
- One `Client` per GATT connection and per characteristic pair; created at connect, dropped at disconnect. The caller picks the pair (notify `6A4E281x`, write `6A4E282x`), bonds, exchanges MTU, enables the CCCD, then sends `Input::Connected { att_mtu }`. All writes go to that pair's write characteristic as write-without-response, in order. [seen: PROTOCOL §1.1]
- `handle(now, input) -> Step { writes, events, wake_at_mono_ms }`. No IO, no clock reads, no panics on peer data. After `Closed`, every input returns an empty `Step`.
- Time: `Now.mono_ms` drives every timer; the wall fields are used only for the 5052 reply. All timers (MLR ack delay, MLR retransmit, reply deadlines, retry delays) collapse into the single `wake_at_mono_ms`; the caller arms one timer and sends `Input::Tick`. A late Tick is fine; an early one is a no-op.
- Caller decisions: `StartSync`, `FileStored { transfer_id }`, `FileStoreFailed { transfer_id }`, `RequestBattery` (port-list row 28, added 2026-11-19: one device_status request, deferred until the handshake is done; never sent by the client on its own [ours]). No SHA-256 crosses the boundary: `core` has no hash and could not check it; hashing belongs to the store (2d).
- `ClientConfig` has no defaults in `core` (client id, host strings, app version, capability mask, `first_connect`). Tests take them from the capture or use fake values. `first_connect` (port-list row 16, added 2026-11-19) is set by the caller for the first connection after bonding and cleared once `Event::PairingCompleted { all_acked }` has arrived.

#### 2. Transport and MLR (`client/transport.rs`, `client/reliable.rs`)
- Bring-up, strictly request → reply: close-all (any reply status accepted; status 1 seen, meaning unknown Q-ML-1) → register REGISTRATION(4) mode 0 → info pages 0–4 as `[handle, page]` → register GFDI(1) mode 2 → register KEEP_ALIVE(22) mode 0 → write `[ka_handle, 01 19 00 00 00]`. [seen: CAP-1 t=0.86–1.89, both apps; keep-alive payload meaning inferred]. Keep-alive values from the watch (`02 00`, counters) are consumed silently.
- Register replies: status 0 → continue; status 2 PENDING_AUTH → keep waiting [seen APK]; status 3 → `Closed(ChannelInUse)`; other → `Closed(RegisterRejected)`. GFDI without the reliable flag → continue in plain mode (one handle byte per ATT value; plan row 8, *inferred (Gadgetbridge)*; `CloseReason::ReliableNotGranted` was removed). The watch closing the GFDI handle → anomaly `GfdiClosedByWatch`; if the session is idle, register GFDI again (phase `BringUp` until answered; frames sent meanwhile are held), else `Closed(GfdiClosed { phase })`. A 5024 on an idle session restarts the handshake (row 8).
- `MlrEndpoint` (symmetric; the test fake watch uses the same type): header via `mlr::{parse, build}`, receive via `mlr::ReceiveWindow`. Payload per packet `att_mtu − 5`. Each GFDI frame (`cobs::encode_frame`) starts a new packet. Start seq 0 / ack 0. [seen: PROTOCOL §1.4]
- Receive: Accept → deliver, Duplicate / OutOfOrder → drop [seen]; all three schedule an ack [inferred for the two drop cases]. Ack policy: ACK-only packet (2 bytes, seq 0) after 5 unacknowledged data packets or 10 ms after the first; any outgoing data packet carries the ack and cancels it. [seen]
- Send: window fixed at 32 unacknowledged packets [ours; the app grows it to 63]; retransmit timer 2000 ms from the oldest unacknowledged packet, doubling per timeout to 20000 ms, reset to 2000 ms on ack progress; a timeout resends every unacknowledged packet with the current ack; retries never give up [seen, minus the RTT estimator, whose floor is 2000 ms anyway — ours].
- A value on an unknown handle, a malformed control message, control type 4 or 0xFF → `Anomaly(BadTransport)`, nothing sent.

#### 3. GFDI session (`client/session.rs`)
- Our requests use compact ids with a txn counter 0..31, +1 per request, wrapping [seen: CAP-1 t=55.43 wrap]. One request outstanding at a time [ours]. Replies to watch requests are sent at once, in arrival order, with the request's txn if it had one (none seen). A reply deadline of 30 s applies to every awaited answer in every phase; expiry → `Closed(Timeout)` [ours]. A non-ACK status to one of our requests → `Closed(RequestFailed)`, except 5002/5008 (one file fails, §4) the first-connection events (reported in `PairingCompleted { all_acked: false }`, session goes on) and the battery request (`Anomaly(BatteryRequestRefused)`, session goes on; also when it is unanswered at the deadline, unless a download is waiting for chunks [ours]).
- Handshake order: wait 5024 → ACK with host info (PROTOCOL §3 step 2; protocol 150) → wait 5050 → bare ACK → our 5050 (`config.capabilities`) → 5031 → 5030 `08 00` HANDSHAKE_COMPLETE → on its ACK emit `HandshakeDone`. [seen: CAP-1 t=2.62–4.77, Explore's order]. With `first_connect`: then 5030 `04 00`, `00 00`, `0E 00`, one at a time and before any sync request; after the answer to the last, emit `PairingCompleted`. [inferred (Gadgetbridge `completeInitialization`); one at a time after event 8 is ours] Protocol < 150 in 5024 → `Closed(ProtocolTooOld)`.

| Watch → phone | Our answer | Label, evidence |
| --- | --- | --- |
| 5024 | ACK + host info | seen CAP-1 t=2.68 |
| 5050 | bare ACK | seen t=3.76 |
| 5052 | ACK `ref, unix_s − 631065600, utc_offset_s, next_dst, following_dst` (Garmin epoch); emit `TimeSent`. `unix_s` before 2020-01-01 → no reply, `Anomaly(ClockInvalid)` (never set the watch to a bad time) | seen t=5.11, 43.68, 62.67; guard ours |
| 5037 (any option, any bitmask) | bare ACK, then one sync cycle (queued if not `Ready`; at most one queued) | seen t=43.86, 53.50, 62.81; CAP-2 t=1044.68 |
| 5004 | ACK `status, u32 total bytes received` per chunk | seen CAP-2 t=1081.99 (3969), 1082.85 (5157) |
| 5043 | ACK body `u16 request_id` + six zero bytes; no 5044 follows. Several chunks → same ACK + `Anomaly(ChunkedProtobuf)` | seen for requests the app leaves unanswered (CAP-1 t=4.92→5.21); inferred for the rest |
| 5044 (answer to our sync request) | ACK body as above | seen t=62.89 |
| 5043 / 5044 carrying a `device_status` battery answer | ACK body as above, then `Battery { level_percent, report: DeviceStatus }` when the level is ≤ 100 | inferred (Gadgetbridge, row 28); field numbers from our schema |
| 5009 | bare ACK; a readable FIT entry (index ≠ 0) is fetched, see §4 | inferred (Gadgetbridge, row 22) |
| 5023 | bare ACK; `Battery { level_percent, report: Status }` when the level is ≤ 100 and the status is not 7 (invalid). Under 3 bytes → `Anomaly(BadFrame)`, no reply | inferred (Gadgetbridge, row 28) |
| 5101 | nothing; `Closed(AuthRequested)` | seen APK `rq50` |
| any other request (5036, 5042, 5014, 5034, 5011/5012, 5022, 5027, 5054, …) | 5000 `id, status 2`, no body; emit `Unsupported` | inferred (APK auto-reply; status value never seen on the wire, Q-GFDI-5) |
| 5000 matching no request; bad COBS / length / CRC | `Anomaly`, nothing sent | ours |

- Deliberately not sent (each is on the allowed-differences list): info page 5; NFC and real-time service registrations, and the app's later close-handle (control type 2) for those same handles with the watch's close-handle responses (type 3); 5043 FeatureCapabilitiesRequest; app-initiated 5043 protobuf requests on Smart field 21 (music) and field 42 (RealtimeSettings: settings screens, a later companion duty) together with the watch's ACK/5044 replies to them; 5026 settings 6/7/8 (Explore omits it too); 5030 HOST_DID_ENTER_FOREGROUND; 5033/5035/5049; every 5044 answer to a watch protobuf request, including the cloud HTTP proxy; the 255/19 upload (5005/5003/5004); weather 5011/5012.

#### 4. Sync cycle and safety (`client/sync.rs`)
- Trigger: a watch 5037, or `StartSync`, which sends 5043 with `Smart.core(13){1{1=0}}` (header `request_id, offset 0, total 6, chunk 6`; request ids count from 0) and then waits for the watch's 5037 [seen: CAP-1 t=62.56–62.71]. No 5037 within 30 s of the watch's ACK to that 5043 → `Anomaly(SyncNotGranted)`, back to `Ready`, no listing. (Counted from the ACK since 2026-11-19, so time queued behind first-connection events does not count.) A watch 5009 (row 22, 2026-11-19) is the third trigger: with the session ready and no cycle running, a `FileAvailable` cycle goes straight to the announced file (no 5007, no listing, no `Directory`); otherwise the entry is kept (at most 32; the same file, by index, type, sub-type and file number, once) and joins the end of the running cycle's list, or starts a `FileAvailable` cycle once the current one ends or the handshake finishes. A queued 5037 or `StartSync` goes first. A 5009 for a file this connection already archived or failed (at most 256 remembered), or for one in the running cycle, is ignored, so a file the watch keeps refusing cannot cause endless cycles; listings still retry such files. The archive rule below applies unchanged.
- Cycle: 5007 `03` → 5002 index 0 (`index, offset 0, type 1, crc_seed 0, max_size 0, flags 01`) → parse directory (16-byte header skipped; `DirEntry`) → emit `Directory` → for each entry in directory order: download → wait for the caller → archive → next → finally 5030 `00 00` SYNC_COMPLETE, also after failures and for an empty listing → `SyncFinished`. [seen: CAP-1 t=44.04–57.40; CAP-2 t=1076.54–1098.20; failure case inferred]
- Download policy: only entries with `data_type == 128` and flag READ (0x80); others → `FileFailed(Skipped)` [inferred from "FIT sub-types only", PROTOCOL §5.6]. 5002 reply status 3 → retry after 1 s, 5 times [seen APK]; other non-zero → `FileFailed(Refused)`.
- Check per chunk: `offset == bytes so far` and `crc::update(running, data) == chunk crc`; at the end `total == size from the 5002 reply`. Good chunk → ACK status 0. CRC or size mismatch → ACK status 3; offset mismatch → ACK status 2; then the partial data is dropped and the file is requested again from offset 0, once per cycle [statuses seen APK `yw40`, never on the wire; retry count ours]. Second failure → `FileFailed { will_retry: false }`, no archive; the file stays pending for the next sync.
- **Archive rule.** (Stage 2g adds `ClientConfig::archive`: with it false the client never builds 5008 and a stored file is only counted as copied; with it true, the default of every host before 2g, this section holds as written. See Stage 2g.) 5008 `index, 10` is sent only when all hold: the file was delivered by `FileDownloaded` in this cycle of this `Client`; the client is waiting on exactly that `transfer_id`; the caller sent `FileStored` for it. While waiting, the file lane is frozen (no other 5002/5008, no timeout: the caller owns that deadline) but watch requests are still answered. `FileStoreFailed` → `FileFailed(StoreFailed)`, next file. Any other `transfer_id` → `StaleConfirmation`, nothing sent. The directory is never archived. 5008 reply: ACK with first body byte 0 → `FileArchived`; else `FileFailed(ArchiveRefused)` (rest of the body is unknown, Q-GFDI-2).
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
4. (Removed: 5052 without DST transitions is settled by Q-TIME-2; `Now.dst: Option<DstPair>`, `None` sends 0, 0.)
5. Need for a default capability mask, client id or app version in `core`.
6. Anything that would require copying capture bytes containing ids, names, notification text or cloud answers into a committed file.
7. A conflict between this contract and PROTOCOL.md: PROTOCOL.md and the captures win; report it.

#### 9. Integrator decisions on the reviewer's open points (2026-11-19)
- Capability mask, client id, app version, host strings: config only, no defaults in `core`. 2c starts with Explore's 5050 set (seen working; weather 5014 then gets status 2) and confirms it live.
- 5043 FeatureCapabilitiesRequest: not sent in 2a. 2c checks live whether the watch needs it; if so, Explore's body (no account GUID).
- Unhandled watch requests → 5000 status 2 (inferred, Q-GFDI-5); watch protobuf requests → 8-byte ACK, no 5044.
- 5002 flags 0x01 as the app sends; a 5054 reply → `FileFailed`, no archive.
- No SHA-256 in `FileStored`: the store (2d) owns hashing and its own audit log.
- 5052 DST fields: settled by Q-TIME-2 (start, end; 0, 0 without transitions); the caller computes `Now.dst`, stop rule 4 is gone.
- MLR: fixed window 32, no RTT estimator, 30 s reply deadline closes the session.
- Committing a scrubbed message-level transcript of a real capture: owner decision, not now.

#### 10. Integrator decisions after the reviewer code review (2026-11-19)
- **Transfer ids unique across connections (blocker B1).** `ClientConfig` gains `connection_epoch: u32`; the caller gives every `Client` a new value (an in-process counter is enough: stale confirmations can only come from the same process). `transfer_id` becomes `u64 = (epoch << 32) | n`. A confirmation carrying another epoch → `StaleConfirmation`, nothing sent.
- **Per-file failure instead of closing (S2).** A non-ACK status to 5002 → `FileFailed(Refused)`; to 5008 → `FileFailed(ArchiveRefused)`; both continue with the next file. Non-ACK to 5007, a handshake request or any bring-up step still closes. Reply silence (30 s) still closes in every phase (the link is probably gone); a file that makes the watch go silent every time is a known risk to watch in 2c.
- **5054 (S1).** A 5054 while a download is active fails that file (`FileFailed`, no archive) and the cycle continues; outside a download → `Anomaly`. 5002 flags stay 0x01.
- **Allowed differences (S4).** "NFC and real-time service registrations" explicitly includes the app's data on those handles. 5026 only for settings 6/7/8. Control type 4, decoder `Unknown`, `MlrOutOfOrder` and anything else unnamed → unclassified (fail).
- **Replay cannot pass empty (S3).** If `private/captures` exists, all three captures must load, sessions per capture must be 1/2/2, files replayed > 0, and a client `Anomaly` fails the session.
- **Nits accepted:** input applied before timers; caller inputs never extend the reply deadline; empty 5004 chunks are not progress; saturating arithmetic on caller times; a duplicate ACK-only restarts the retransmit timer (PROTOCOL §1.4); `link.rs` tracks a 5002 as leniently as before; `FileType` lives in `messages` (re-exported by `client`). Kept as is, to check live in 2c: a stray 5004 gets ACK status 2.

## Stage 2p: port Gadgetbridge's Garmin work
Base: Gadgetbridge commit `7257742b` (local copy in `private/re/gadgetbridge`, git-ignored). Rules: `docs/LICENSING.md`.
**Goal**:
1. Survey (reviewer): map Gadgetbridge's Garmin modules to our components and decide for each: port, covered by our own code, or skip. Output: the port list below. **Done 2026-11-19.**
2. Device catalogue in `core/src/devices/` (ported): how a Garmin device is recognised, model names, feature flags and per-model quirks for the ≈197 Gadgetbridge models. Each model is marked *supported per Gadgetbridge, not verified* until someone syncs one.
3. Protocol diff: compare Gadgetbridge's message handling with `docs/protocol/PROTOCOL.md` and our captures; record every disagreement in `PROTOCOL.md` (captures win) and add Gadgetbridge-only knowledge as *inferred (Gadgetbridge)*. **Done 2026-11-19: PROTOCOL.md §9, OPEN-QUESTIONS.md "Gadgetbridge answers".**
4. Parser parity: port the FIT interpretation rules marked `2p` in the port list (rows 40 and 42; row 41 is a comparison only) into `core/src/derive`, by field name through `core::fit::profile`, never from Gadgetbridge's generated tables.
**Success Criteria**: every ported file has the attribution header and a `docs/PORTED.md` row; the catalogue recognises the Instinct 2 (product 3888) exactly as our captures show; the private replay and all tests still pass; nothing from Gadgetbridge's generated FIT profile (`FitCodeGenerator/src/main/resources/fit_profile.json`, generated `fit/messages/Fit*`, `fit/enums/*`, `NativeFITMessages`, `assets/workouts/exercises/garmin.json`) is committed; every row marked `2p` below is either done or moved to a later stage with a reason.
**Tests**: catalogue lookups (known ids, unknown id → generic Garmin); a parity test per ported parser rule over our real files (private) and synthetic files (committed); for every later-stage port, an integration test through `Client::handle` (the real entry point), not only a unit test of the codec.
**Status**: Mostly done (port list, protocol diff, catalogue with 193 models and their sync status, battery); parser parity not started; sleep stages were solved from real nights instead (2026-11-20)

### Port list

Paths in the first column are relative to `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/` unless they start with another top-level directory. `D/` = `devices/garmin/`, `S/` = `service/devices/garmin/`. Decision: **port** = translate the logic with attribution; **covered** = our own code already does it (differences are in PROTOCOL.md §9); **skip** = not ported, with the reason. **PROFILE** marks code that depends on Gadgetbridge's generated FIT profile; see "Profile-dependent code" below the table.

| # | Gadgetbridge module / files | Our target | Stage | Decision | Reason |
| --- | --- | --- | --- | --- | --- |
| 1 | `D/GarminCoordinator.supports`, `D/GarminProductNumbers`, `devices/AbstractDeviceCoordinator.supports` | `core/src/devices/{mod,pattern}.rs` | 2p | port | Recognition: Garmin manufacturer data (Bluetooth company id 0x0087) whose first two bytes are the product number, big-endian; if absent or unknown, the BLE name must match the model's pattern as a whole. |
| 2 | ≈197 coordinators in `D/watches/**`, `D/bike/*`, `D/gps/*`, `D/hrm/*`; `model/DeviceType` (Garmin entries) | `core/src/devices/catalogue.rs` (generated by `tools/gen-devices`) | 2p | port | Name patterns and per-model flags (training load/status, race prediction, solar, battery count, experimental, `defaultNewSyncProtocol`, `defaultExploreSync`). |
| 3 | `FitCodeGenerator/src/main/resources/fit_profile.json`, key `devices` (product → `DeviceType`) | input of `tools/gen-devices` only | 2p | port (link only) | **PROFILE.** Only the product number → model link is taken; product numbers are also sent by the device itself (5024, advertising). Garmin's product names from that file are not copied. |
| 4 | `D/GarminCapability` (129 names for the 5050 bits, `OUR_CAPABILITIES`) | `core/src/devices/` capability names; `ClientConfig.capabilities` chosen in 2c | 2p | port | We only had bit numbers. The list is Gadgetbridge's own naming, not FIT profile data. |
| 5 | `D/GarminPreferences`, `S/GarminPrefs`, `D/GarminDeviceSettings.kt`, `D/GarminSettingsCustomizer`, `D/GarminAgpsSettings.kt`, `D/GarminDeviceCardItems.kt`, `D/GarminChartsProvider.kt`, `D/GarminWidgetsProvider.kt`, `D/GarminAuthExpiredActivity.kt`, `D/actions/*` | — | — | skip | Gadgetbridge's Android preference screens, charts and map activities. Our interface is the web app (phase 3) and the native Devices screen (2e). |
| 6 | `S/communicator/CobsCoDec`, `S/ChecksumCalculator` | `core::cobs`, `core::crc` | 0c | covered | Same algorithms; ours are tested against captures. |
| 7 | `S/communicator/v2/CommunicatorV2` (control channel, register/close, client id, characteristic choice) | `core::multilink`, `core::client::transport` | 2a | covered | Ours follows the app (REGISTRATION and KEEP_ALIVE services, info pages, status 2/3 handling). Gadgetbridge is simpler; see PROTOCOL.md §9.1. |
| 8 | `CommunicatorV2`: plain (mode 0) GFDI with one handle byte per fragment; re-register GFDI when the watch closes its handle; service ids 8, 12, 13, 16, 19, 21, 26 and `0x2018`…`0xE018` | `core::client::transport`, `core::multilink::service_name` | 2c | port | Fallback for watches that do not grant MLR (answers Q-MLR-3 as *inferred*), and recovery from an unexpected close. Small. **Done 2026-11-19** (transport plain mode and reopen; session side ours, see the 2a contract §2). |
| 9 | `S/communicator/v2/MlrCommunicator` | `core::mlr`, `core::client::reliable` | 2a | covered | Ours follows the app's native library; Gadgetbridge differs in three parameters (PROTOCOL.md §1.4). Not ported. |
| 10 | `S/communicator/v1/CommunicatorV1` (services `9B012401-…` and `6A4E2401-…`, single channel) | `core::client::transport` variant + Android GATT pair choice | 2c (after the Instinct 2 path works) | port | Needed for the older models in the catalogue; the Instinct 2 does not use it. Without it those models are "recognised, cannot connect". |
| 11 | `CommunicatorV2` real-time callbacks, `S/communicator/v2/Realtime*Callback.kt` (HR, steps, floors, calories, body battery, SpO2, respiration, HRV, accelerometer) | `core::link` decoding; live view later | 2f | port | Payload layouts for services we only saw as bytes in CAP-1. Low priority. |
| 12 | `S/GarminRfcommListener` (classic Bluetooth file transfer) | — | — | skip | For devices that list files only over RFCOMM. None owned; cannot be tested. |
| 13 | `S/GarminSupportHrm`, `D/hrm/*` behaviour | — | — | skip | Heart-rate straps use a different support class. The catalogue recognises them; no sync. |
| 14 | `S/messages/GFDIMessage` (id enum, length/CRC, status enum), `MessageWriter`, `S/GarminByteBufferReader`, `UnhandledMessage`, `status/GenericStatusMessage` | `core::gfdi`, `core::messages` | 0c/2a | covered | Ours also handles compact ids with transaction numbers and auto-replies status 2. Message names we lacked go into `messages.csv` (2p, docs only). |
| 15 | `S/messages/{DeviceInformation,Configuration,SystemEvent,SetDeviceSettings,SupportedFileTypes,CurrentTimeRequest}Message`, `S/GarminTimeUtils` | `core::messages`, `core::client::session` | 2a | covered | Byte layouts agree with CAP-1 except the 5052 field order (PROTOCOL.md §9.3, Q-TIME-2). Enum names (18 system events, 9 setting ids) are added to the docs. |
| 16 | `GarminSupport.completeInitialization`, `connectFirstTime` (PAIR_COMPLETE, SYNC_COMPLETE, SETUP_WIZARD_COMPLETE on first connect; TIME_UPDATED; SYNC_READY) | `core::client::session` (first-pairing branch) | 2c | port | The only description we have of a fresh pairing until the Q-PAIR-1 capture exists. Must be confirmed by that capture. **Done 2026-11-19:** `ClientConfig::first_connect`, `Event::PairingCompleted`; TIME_UPDATED (16) not sent, as before (CAP-1 shows the watch asks for the time without it). |
| 17 | `S/messages/AuthNegotiationMessage`, `status/AuthNegotiationStatusMessage`, ids 5101–5112 | — | — | skip | Gadgetbridge only echoes 5101 with "guess ok"; it has no Garmin Auth crypto. We close on 5101 as the app does. Revisit only if 2c pairing shows a 5101 (Q-AUTH-1). |
| 18 | FeatureCapabilities exchange (5043/5044 `core`) | `core::client::session` | 2c | covered | Gadgetbridge never sends it, which is one more hint that it is optional. Our 2a client does not send it either. |
| 19 | `S/FileTransferHandler.Download`, `S/messages/{DownloadRequest,FileTransferData,Filter,SetFileFlags,Synchronization}Message` + statuses, `S/FileToDownload` | `core::client::sync` | 2a | covered | Ours checks offset, CRC and size, sends the app's status codes, and archives only after confirmed storage. Gadgetbridge's names for the 5037 bits and the status enums go into the docs. |
| 20 | `S/FileType` (`FILETYPE`: type/sub-type names, `pull` flags, string-named types of the new sync) | `core::messages::FileType` names, `docs/protocol/filetypes.csv`; download filter in 2d | 2d | port | We download every readable FIT file; Gadgetbridge's `pull` list is a useful default for non-FIT types (logs, KPI). Names of 128/x sub-types are Gadgetbridge's own. |
| 21 | `FileTransferHandler.Upload`, `S/messages/{CreateFile,UploadRequest}Message` + statuses, `FileTransferData` outgoing | new `core/src/client/upload.rs` | 4a (earlier if a 2f duty needs it) | port | We have no upload path. Needed for workouts (4a) and any FIT settings file. Layouts are *inferred* until a capture of our own upload. |
| 22 | `S/messages/FileAvailableMessage` (5009) | `core::client::sync` | 2d | port | The watch announces one new file; download it without a full listing. Small. **Done 2026-11-19:** `SyncTrigger::FileAvailable`; files announced during a cycle join it (see the 2a contract §4). |
| 23 | `S/FileSyncServiceHandler.kt`, `GarminSupport.downloadFileFromServiceV2` / `pushDataXferItem`, `FILE_TRANSFER_*` services (new sync over `Smart.file_access` 43) | new `core/src/client/sync2.rs` | later (first owner of a `SYNC_2` device) | port | Required by models with `defaultNewSyncProtocol` (e.g. Venu X1). The Instinct 2 does not advertise capability 90. Cannot be verified without such a device. |
| 24 | `S/ExploreSyncHandler` (`Smart.explore_sync` 22) | — | — | skip | Waypoints and tracks of Garmin Explore. Not in the brief. |
| 25 | `S/EcgServiceHandler.kt`, `D/GarminEcgDataProvider` | — | — | skip | No ECG on the owner's watch; depends on the new sync. |
| 26 | `GarminSupport.alreadyDownloaded`, `S/GarminUtils.buildExportPath` | server store (2d) | 2d | covered | We deduplicate by content hash, not by file name. Gadgetbridge's name-based rule would be wrong for us: the watch reuses indices. |
| 27 | `S/messages/ProtobufMessage`, `status/ProtobufStatusMessage`, chunking in `S/ProtocolBufferHandler` | `core::messages` (header, ACK body) + chunk reassembly in `core::client::session` | 2f | port | We parse the header and send the 8-byte ACK, but treat chunked messages as an anomaly. Port reassembly, the chunk rule (`max_packet − 20`) and the status codes. |
| 28 | `ProtocolBufferHandler` dispatch: `device_status` 8 (battery), `S/messages/BatteryStatusMessage` (5023) | `core::client` event `Battery` | 2e | port | Battery level for the Devices screen. **Done 2026-11-19:** `Event::Battery`, `Input::RequestBattery` (sent only when the host asks; see the 2a contract §1 and §3). |
| 29 | `ProtocolBufferHandler`: `core` 13 `GetLocationRequest`, `LocationUpdatedSetEnabledRequest`, `GarminSupport.onSetGpsLocation` | Android duty "share phone location" | 2f | port | Optional; off by default (privacy). |
| 30 | `ProtocolBufferHandler`: `credentials` 27 fake OAuth, `S/http/interceptors/OauthInterceptor` | — | — | skip | Hands the watch made-up Garmin tokens. Only worth it if a duty we want fails without it; owner decision. |
| 31 | `GBProtobufGenerator/src/main/proto/garmin/*.proto` | — | — | skip | Our schemas are recovered from the app (`docs/protocol/proto/`, 201 files) and are more complete (e.g. `RawResourceRequest.encrypted_request`, RealtimeSettings names). |
| 32 | `S/agps/*`, `S/http/interceptors/AgpsInterceptor`, `GarminSupport.getAgpsFile` | `core` duty "GPS assist" (request/response shaping) + Android fetch | 2f | port | Serves `/ephemeris/…` requests from a file (tar of `CPE_*.BIN`, RxNetworks gzip, Sony CPE), with ETag and 304. Needs Q-COMP-1 settled for the Instinct 2 first. |
| 33 | `S/http/HttpHandler`, `GarminHttpRequest`, `GarminHttpResponse`, `DataTransferHandler`, `GarminJson`, `interceptors/FirewallInterceptor` | new `core/src/companion/http.rs` (sans-IO) + Android | 2f | port | The HTTP-over-protobuf proxy: raw and Connect IQ requests, gzip, large bodies through `data_transfer` 7. Default must be "answer nothing to Garmin" (the brief allows only GPS assist). |
| 34 | `GarminSupport.encodeWeather`, `S/FitLocalMessageHandler`, `S/fit/FitLocalMessageBuilder`, `S/messages/{FitDefinition,FitData,WeatherRequest}Message` + statuses, `S/fit/messages/FitWeatherConditions`, `fieldDefinitions/FieldDefinitionWeather*`, `interceptors/WeatherInterceptor` | duty "weather": `core::fit::encode` + `core::client` | 2f | port | **PROFILE.** 5014 → FIT `weather_conditions` over 5011/5012, as seen in CAP-3. Needs a weather source decision (no Garmin service). |
| 35 | `S/NotificationsHandler`, `S/messages/Notification{Update,Control,Data,Subscription}Message` + statuses, GNCS media in `ProtocolBufferHandler`, `S/PushNotificationHandler.kt` | duty "notifications" in `core::client` + Android listener | 2f | port | ANCS-like attribute protocol; large but self-contained. |
| 36 | `S/messages/MusicControl*Message` (5041, 5042, 5049) | duty "music control" | 2f | port | Small. |
| 37 | `S/messages/FindMyPhone{Request,Cancel}Message` (5039/5040), `GarminSupport.onFindDevice` (`find_my_watch` 12) | duty "find my phone / watch" | 2f | port | Small. |
| 38 | `ProtocolBufferHandler.processProtobufCalendarRequest` (`calendar` 1) | duty "calendar" | 2f | port | Optional; the Instinct 2 advertises CALENDAR (bit 49). Last in 2f. |
| 39 | SMS canned replies (`sms_notification` 16), `interceptors/{Contacts,Hydration,ImageService}Interceptor`, `S/AppConfigHandler.kt`, Sleep as Android hooks, `GarminSupport.onSetAlarms` (alarms through a FIT settings file) | — | — | skip | Not in the brief. Alarms on watches with REALTIME_SETTINGS (the Instinct 2) are not set this way. |
| 40 | `S/fit/FitImporter` (per-message rules), `D/Garmin*SampleProvider` | `core::derive` | 2p | port | **PROFILE.** Rules we lack: naps (412), restless moments (382), intensity minutes per sample, SpO2 mode (1 manual, 3 automatic), race predictions, hill/endurance score, training readiness, functional metrics, battery, solar. Rules we have: stress, body battery, respiration, SpO2, resting HR, HRV, training load, VO2 max, recovery time. |
| 41 | `S/fit/messages/FitMonitoring` (hand-written: `timestamp_16` against the last monitoring timestamp; activity type and intensity packed in one byte), `FitImporter.persistActivitySamples` (per-type running totals, gap filling, "not worn" after 10 min) | `core::derive::monitoring` | 2p | covered | Same `timestamp_16` rule as ours (independent confirmation of Q-FIT-2). Gap filling and "not worn" are presentation choices for 3g, not ported now. |
| 42 | `D/GarminSleepSessionProvider.kt`, sleep events in `FitImporter` | `core::derive::sleep` | 2p | port | **PROFILE.** Session start/end from FIT `event` messages (sleep detection, start/stop) and each `sleep_level` row as the *end* of its stage. We only use `sleep_summary`, which is unverified. Check against the night-sync files. |
| 43 | `S/fit/RawSleepStream.kt`, `RawSleepNights.kt`, `S/fit/messages/FitSleepDataRaw` | `core::derive::sleep` | later | skip | Sleep estimated from raw minutes, for watches that do not stage sleep. The Instinct 2 stages sleep; an estimate would have to be labelled as derived (hard rule 3). |
| 44 | `D/GarminWorkoutParser`, `S/fit/messages/{FitSession,FitRecord}`, `S/fit/enums/GarminSport` | `core::derive::activity` | 1d | covered | Ours keeps sessions, laps, records, sets, time in zone and metrics. Sport labels come from our profile lookup. |
| 45 | FIT codec: `S/fit/{FitFile,RecordData,RecordDefinition,RecordHeader,FieldDefinition,DevFieldDefinition,FieldDefinitionFactory}`, `baseTypes/*`, hand-written `fieldDefinitions/*` | `core::fit::decode`, `core::fit::interpret` | 1a | covered | Ours is lossless and profile-independent. |
| 46 | FIT writing: `FitFile.getOutgoingMessage`, `RecordData`/`RecordDefinition.generateOutgoing*`, `S/fit/FitRecordDataBuilder`, `FitLocalMessageBuilder` | new `core::fit::encode` | 4a (2f if weather comes first) | port | **PROFILE.** We cannot write FIT yet. Port the encoding logic over typed inputs and `profile` lookups by name. |
| 47 | Generated code: `S/fit/messages/Fit*` (≈219 classes), `S/fit/enums/*` (generated), `S/fit/NativeFITMessage(s)`, `FitCodeGenerator/**`, `assets/workouts/exercises/garmin.json` | — | — | skip | **PROFILE.** Garmin profile data (LICENSING rule 3). Never committed; replaced by `core::fit::profile`. |
| 48 | `S/fit/workouts/GarminWorkoutFitEncoder.kt`, `GarminWorkoutCodes.kt`, `GarminExerciseCatalog.kt`, `D/GarminWorkoutTemplateSpec.kt`, `GarminSupport.onSyncWorkoutTemplate` | `core::fit::encode` + server workout model | 4a | port | **PROFILE.** Workout, steps, repeats, targets, exercise titles, notes; uploaded as type 128/5. The exercise catalogue is generated locally. |
| 49 | `S/fit/GpxRouteFileConverter`, `D/Garmin{GpxRoute,FitFile}InstallHandler` | — | later | skip | Courses are not in the brief; reuses rows 21 and 46 if wanted. |
| 50 | `S/communicator/GarminWatchAppCommunicator`, `Xxtea`, `S/watchapps/*`, `S/GarminPrgFile.kt`, `D/GarminPrgInstallHandler.kt`, installed-apps handling | — | — | skip | Connect IQ apps (separate GATT service `6A4E8022-…`, XXTEA-wrapped messages, PRG install). Not in the brief. |
| 51 | `S/deviceevents/*` (8 event classes) | `core::client::Event` | 2a | covered | Internal plumbing of Gadgetbridge; our client has its own events. |
| 52 | `S/fit/AntGadget.kt`, `FitDebug`, `FitAsyncProcessor`, `D/GarminVendorDataImporter.kt` | — | — | skip | Gadgetbridge plumbing; we have `reparse` and `import-export`. |

Counts: 52 rows — port 26, covered 12, skip 14. Port rows by stage: 2p 6 (rows 1–4, 40, 42), 2c 3 (8, 10, 16), 2d 2 (20, 22), 2e 1 (28), 2f 10 (11, 27, 29, 32–38), 4a 3 (21, 46, 48), later 1 (23). Rows 21 and 46 move forward to 2f if a duty needs them first.

### Profile-dependent code (rows 3, 34, 40, 42, 46, 47, 48)

Gadgetbridge generates one Java class per FIT message from `fit_profile.json` and its code calls typed getters (`getOverallSleepScore()`, `FitWorkoutStep.Builder`). That JSON is a community description of Garmin's profile plus messages nobody documents; we treat all of it as profile data and commit none of it. To port a rule:

1. Read the rule in the hand-written file (`FitImporter`, `GarminWorkoutFitEncoder`, …) and note the message and field **names** it uses.
2. In Rust, look the message and fields up by name through `core::fit::profile` (reading) or the future `fit::encode` (writing). Individual standard names such as `sleep_level` or `workout_step.duration_type` may appear in code and tests (LICENSING, last rule); tables may not.
3. Where Gadgetbridge names a field our locally generated profile lacks, or the names differ (for example Gadgetbridge `TRAINING_LOAD` 378 against our `acute_load_results`), the rule goes by **message and field number** with a comment naming both, and the test uses a synthetic file.
4. With `NIMRAG_FIT_PROFILE=none` the rule must degrade to "unavailable", never to a wrong value; a committed test asserts that.

### Integrator decisions on the survey (2026-11-19)
1. **Q-TIME-2 (5052 DST fields).** Settle from evidence before anything is renamed: the app's 5052 reply builder in the decompiled APK (static), else a capture in summer time. `Now` keeps its field names until then; the reviewer checks the APK. Must be closed before the March clock change.
2. **5002 flags** stay 0x01 (as Garmin Connect sends) until 2c; switch to 0 only if a 5054 ever arrives.
3. **Upload chunk size** (row 21, when ported): the whole frame must fit `max_packet` (one CAP-2 chunk fits that exactly), stricter than Gadgetbridge's `max_packet − 13`; confirm with a capture.
4. **Catalogue sync status**: models that use the new sync (row 23; `defaultNewSyncProtocol`, 5 models) are marked "recognised, cannot sync yet" in the catalogue. The V0/V1 transports (row 10) are not a per-model fact in Gadgetbridge: it picks the communicator at connect time from the GATT services the device exposes. Our app does the same in 2c and reports "cannot sync yet" for a V1-only device until row 10 is ported.
5. **`GarminCapability` and `FileType` names** are Gadgetbridge's own lists, not FIT profile data: portable.
6. **Row 42 data** (corrected): the USB copy has no sleep-stage data (empty `Sleep/`, no `sleep_level`, no sleep events; the Instinct 2 hands sleep over Bluetooth at sync). Row 42 waits for the night-sync capture. The copy does hold a 7-night `sleep_summary` history in `FbtBackup/lha_bkup.fit`, of which `derive::sleep` kept only the first: fixed separately.

Owner decisions, needed by stage 2f (not blocking now): who fetches GPS-assist data (Gadgetbridge serves a user-supplied file; the brief allows one Garmin call); a non-Garmin weather source; confirm the skipped features (Explore sync, Connect IQ, contacts, canned SMS replies, hydration, ECG, alarms via FIT); fake OAuth stays skipped unless a wanted duty fails without it.

### Order of work after this stage

1. Rows 1–4 (catalogue; in progress). 2. Rows 40 and 42 (parser parity; do row 42 once the night-sync capture exists). 3. Row 16 before the owner re-pairs the watch (2c). 4. Row 8. 5. Rows 27, 33, 32 as the base of 2f. The rest in the order of the stage they belong to.

## Stage 2b: Android build of /core (cargo-ndk, UniFFI, Gradle module)
**Toolchain (installed 2026-11-19, owner approved incl. SDK licences)**: `ANDROID_HOME=~/opt/android-sdk` (cmdline-tools 23.0, checksum-verified; platform-tools 37.0.1, build-tools 37.0.0, platforms/android-37.2, NDK 30.0.16248370), Rust targets aarch64/x86_64-linux-android, cargo-ndk 4.1.2, JDK 25 at ~/opt/rea-engines/jdk25. `cargo ndk -t arm64-v8a -t x86_64 build -p nimrag-core` builds. UniFFI goes in a separate FFI crate so `core` stays std-only.
**Goal**: `ffi/` crate `nimrag-ffi` (workspace member) exposing `nimrag_core::client` through UniFFI (proc-macros; `core` stays std-only); `android/` Gradle project with `:core` (Android library: generated Kotlin bindings + `.so` for arm64-v8a and x86_64 built by cargo-ndk from Gradle) and a minimal `:app` shell for 2c.
**Success Criteria**: `./gradlew :app:assemblePublicRelease :app:assemblePublicDebug` builds from a clean checkout; the APK contains `libnimrag_ffi.so` for both ABIs and no FIT profile data (built with `NIMRAG_FIT_PROFILE=none`, enforced in the Gradle task); Kotlin can drive a `Client` end to end.
**Tests**: Rust tests in `ffi/` for the conversion layer; a host JVM test (bindings + the x86_64-linux `.so` via JNA) that drives a `Client` through Kotlin with fake ids: Connected → close-all write; a scripted bring-up/handshake reaching `HandshakeDone`; a FileStored for a stale transfer id gives `StaleConfirmation`. Optional instrumented smoke test on the Pixel.
**Status**: Complete 2026-11-19. UniFFI 0.32.2, Gradle 9.8.2 (checksum-verified wrapper, dependency verification metadata), AGP 9.5.0, NDK 30 and Rust 1.100.0 pinned; APK 4.5 MB with arm64-v8a and x86_64 only. Guards, each tested by trying the forbidden build: core refuses an Android build with a FIT profile; the fake-watch test fixtures refuse to compile for Android; the Gradle task fails if a .so holds the profile marker, fixture symbols or a /home/ path. A JVM host test drives the client through the generated Kotlin bindings and the real .so, and reruns when the library changes.

### Before the APK is handed to anyone but the owner (AGPL and dependency notices)
From the 2b review: an in-app licence screen with the AGPL source offer (§6), the MPL-2.0 notice for UniFFI, the Apache-2.0 notice for JNA, and MIT/Apache attributions for every linked Rust crate (generated, e.g. with `cargo about`). Not needed while the APK stays on the owner's devices; required in whichever stage first distributes it.

## Stage 2c: Android foreground service, BLE link, pairing without the official app — needs owner (watch re-pairing)
**Goal**: the app pairs with the watch on its own, keeps a link in a foreground service, and drives `NimragClient` over it.
**Success Criteria**: on the owner's phone the app bonds with the Instinct 2 (watch in "Pair phone" mode), reaches `HandshakeDone`, answers the time request, reconnects by itself after the watch leaves and comes back, and survives the screen being off for an hour.
**Tests**: unit tests (JVM, `app/src/test`) for the DST pair, the capability hex parser, characteristic-pair choice and the write queue; the live checks above by the owner.
**Status**: Live since 2026-11-19 22:30 (0.2.3, Pixel 9a, Instinct 2 firmware 17.22): the companion association plus Garmin Connect's existing bond (its Nearby devices permission off); link setup hardened in 0.4.2. Not tried: pairing a watch that has no bond.

Design (integrator, 2026-11-19; contract file `android/app/src/main/kotlin/dev/nimrag/app/model/AppState.kt`):
- Built: after review, pairing first creates a CompanionDeviceManager association (system dialog) so the service may restart in the background and on the watch's presence (`WatchPresenceService`); without one, a refused background start posts "Open Nimrag Connect to reconnect". A removed bond shows `LinkState.BondLost` and stops retrying. `first_connect` is charged only on connections that reached `HandshakeDone`.
- Process: `NimragApplication` creates the engine (`WatchController`) and sets `AppGraph.controller`. `WatchService` (foreground, type `connectedDevice`, ongoing notification with link state) owns the GATT link and the `NimragClient`. It is started when a watch is paired and on `BOOT_COMPLETED` (when one is remembered), and stopped on `forget()`. One `HandlerThread` serialises every `handle` call and every GATT callback; writes go out one at a time (`writeCharacteristic(char, value, WRITE_TYPE_NO_RESPONSE)`, next one after `onCharacteristicWrite`).
- Scan: `BluetoothLeScanner`, no filter, low latency, only while the Watch screen asks (stops after 30 s). A result counts as Garmin when its manufacturer data has company id 0x0087 or `identifyDevice` matches its name; model and sync support come from the device catalogue through a new FFI function `identify_device(name, manufacturer_data)`.
- Pairing: `createBond()`, wait for `BOND_BONDED` (a broadcast receiver), then remember address, name and model, set `first_connect = true`, start the service. `first_connect` is cleared on `PairingCompleted`, or after one more connection that ended without it (the 2a contract §1).
- Link: `connectGatt(autoConnect = true, TRANSPORT_LE)`; `requestMtu(515)`; discover; service `6A4E2800-…`; notify characteristics `6A4E2810…2819` present → pick one at random, write = same UUID with `281x` → `282x`; enable the CCCD; then `Input.Connected(att_mtu)`. `CloseReason.ChannelInUse(retry_char)` → close and reconnect on that characteristic (its 16-bit id names the notify char). Any other close or a disconnect → drop the client, reconnect with backoff 2 s … 5 min (autoConnect keeps waiting in range). One client per connection; `connection_epoch` = in-process counter.
- `ClientConfig`: `client_id` random u64 per install (stored); host name = the phone's device name, manufacturer/model from `Build`; `app_version` = 38215 (Garmin Explore's, seen working in CAP-1) [ours, revisit]; `capabilities` from settings, default hex `39000020000000008010000000` = bits 0 CONNECT_MOBILE_FIT_LINK, 3 SYNC, 4 DEVICE_INITIATES_SYNC, 5 HOST_INITIATED_SYNC_REQUESTS, 29 EXPLICIT_ARCHIVE, 71 CURRENT_TIME_REQUEST_SUPPORT, 76 MULTI_LINK_SERVICE (bit i = byte i/8, value 1 << i%8). Explore's mask (contract §9) lives only in the private captures; this minimal mask claims only what the client implements [ours, to verify live].
- `Now`: `mono_ms = SystemClock.elapsedRealtime()`, wall clock and offset from `java.time`; the DST pair by the rule on `Now.dst` (PROTOCOL §3 step 6) from `ZoneRules.nextTransition`. Timer: the single `wake_at_mono_ms`, armed on the HandlerThread (replaces the previous one).
- After `HandshakeDone`: `RequestBattery`; `StartSync` if the last finished sync is older than `autoSyncMinutes`; repeat both every `autoSyncMinutes` while connected (0 = never; the watch also asks by itself). Since 0.5.1 (default 15 min): the repeat is an inexact `AlarmManager.setAndAllowWhileIdle(ELAPSED_REALTIME_WAKEUP)` alarm, not a Handler delay (Handler time is uptime and stood still in deep sleep: syncs came hours apart instead of every 60 min); see stage 3q.

## Stage 2d: sync to phone storage, mark synced only after safe storage; upload queue (persistent, backoff, Wi-Fi/VPN only, dedupe by hash)
**Goal**: every downloaded file is durable on the phone before the watch may archive it, then reaches the server exactly once.
**Success Criteria**: kill the app between `FileDownloaded` and upload: the file is on disk, not archived twice, and uploaded on the next start; the server receives each file once (dedupe by SHA-256).
**Tests**: JVM unit tests for the store's write path (temp, fsync, rename), the upload backoff and the network rule.
**Status**: Live: since the server was deployed (2026-11-20) every sync's files are stored, archived on the watch after storage and uploaded within seconds; an auto sync is one 128/32 monitoring file plus one 128/58 device file. Not yet seen live: a long outage of the server (backoff), a large activity file.

Design (integrator, 2026-11-19):
- Store: `filesDir/watch/<sha256>.fit`, written to a temp file, `FileOutputStream.fd.sync()`, renamed, directory fsynced; a row in SQLite (`android.database.sqlite`, no Room): `files(sha256 PK, size, data_type, sub_type, file_index, file_number, watch_timestamp, received_at, uploaded_at, attempts, next_attempt_at, last_error)`. Only after the row is committed does the engine send `FileStored`; any failure → `FileStoreFailed`. Bytes already stored (same hash) count as stored.
- Upload: a worker thread in the process, woken on a new file, a network change, `uploadNow()` and at start. Rule: the active network must be unmetered or a VPN, unless `allowMobileData`. `POST {server}/api/files?data_type=&sub_type=&file_index=&file_number=&watch_timestamp=&source=watch&sha256=` with `Authorization: Bearer <token>` and the raw bytes (`HttpURLConnection`, 30 s timeouts). 200/201 → `uploaded_at`. 401/403 → stop all uploads; the account forgets the token and asks for a sign-in (stage 3p). 413 or another 4xx → that file is marked failed with the status, others go on. IO errors / 5xx / 408 / 429 → exponential backoff 30 s … 1 h per file. Files stay on the phone after upload (cleanup is a later decision).
- Settings and secrets in private `SharedPreferences` (the token is never logged nor put into `AppState`). The phone is the owner's; Android's app sandbox is the protection.
- Log: last 500 lines in memory and in SQLite (`log` table, trimmed to 1000), no unit id, no token.

## Stage 2e: time duty, Devices and sync screen
**Goal**: the native app UI: Watch (scan, pair, link, battery, forget), Sync (sync now, progress, last result, stored files and upload state, log) and Settings (account and server, network rule, auto-sync interval, capability mask, about and licences). Time duty = the `Now` above (the client answers 5052).
**Success Criteria**: the owner can pair, see the link and battery, start a sync, watch files arrive and upload, and configure the server without adb.
**Tests**: Compose previews with a fake controller; manual check on the phone.
**Status**: Live: time sent before every sync, sync log shared from the app; in 0.6.0 the Watch tab became one page with sub-pages (states other than "connected and idle" checked only in Compose previews).

Design (integrator, 2026-11-19): Jetpack Compose + Material 3 (dynamic colour, light and dark), one activity, bottom bar with the three screens; the UI reads `AppState` and calls `WatchController` only. Runtime permissions (`BLUETOOTH_SCAN` with `neverForLocation`, `BLUETOOTH_CONNECT`, `POST_NOTIFICATIONS`) are asked for on the Watch screen with a short reason. Settings → About shows the version, the AGPL-3.0-or-later notice with the source link (`https://github.com/AngelFreak/Nimrag-Connect`) and the licences of the bundled libraries (UniFFI MPL-2.0, JNA Apache-2.0, AndroidX/Compose and kotlinx-coroutines Apache-2.0, and the Rust crates linked into `libnimrag_ffi.so` with their licences), which covers the 2b review note for the public debug builds.

## Stage 2f: further companion duties
**Goal**: one at a time, each a user setting: GPS assist data (the only allowed Garmin call; off switch), weather, notifications, music control, find my phone. Port Gadgetbridge's implementation of each (`agps`, `NotificationsHandler`, weather, music handlers) instead of decoding from scratch.
**Success Criteria**: each duty verified live on the watch; where PROTOCOL.md marks it unknown (Q-COMP-1..3, Q-SYNC-2), the ported behaviour is confirmed by a capture before it is marked verified
**Status**: Not Started

## Stage 2g: Garmin Connect too (owner request, 2026-11-19)
**Goal**: owners who also want Garmin Connect keep it. Two routes, chosen in Settings → Garmin Connect:
- **Off** (default, what 0.2.x does): Nimrag is the only sync app and archives each file after safe storage. Garmin Connect, if installed, has its Nearby devices permission turned off.
- **Alongside** (phone side, no Garmin account): Garmin Connect keeps Bluetooth and does what it always does (archives files, uploads them to Garmin, all-day health included). The watch serves several apps at once over multi-link, each on its own characteristic pair (seen: CAP-2, Connect and Explore together), so Nimrag holds a pair of its own, copies every file to its storage and server, and **never archives**.
- **Forward** (server side): nimrag-server uploads activity files (128/4) to the owner's Garmin Connect account. **Owner approved 2026-11-19**, on these terms: activities only (Garmin's import takes no all-day health, sleep or HRV), an unofficial login (the mobile SSO flow; garth broke in March 2026 and is deprecated, so it can break again and may conflict with Garmin's terms), tokens stored, never the password. Built after 3a is merged (it lives in the server).

**Alongside contract** (core, ffi, Android):
- Core: `ClientConfig.archive: bool` (true keeps today's behaviour). False: after `FileStored` the file counts as copied, no 5008 is ever built (the archive rule's one place, `on_confirmation`, checks it), and the cycle moves on. `ClientConfig.skip: Vec<FileKey>` with `FileKey { data_type, sub_type, file_number, size, timestamp }` (all from the directory entry; the index is not stable): listed entries matching a key are not downloaded; each gives `Event::FileSkipped { entry }`, and `SyncFinished` gains `skipped: u32`. Keys only skip; they never cause an archive. Archive true + a skip list is allowed (the host does not do it).
- Alongside sends no `first_connect` events (Garmin Connect owns the pairing) and keeps answering time requests (harmless when both answer).
- Android: setting `garminConnectMode` (Off | Alongside), persisted; table `copied(data_type, sub_type, file_number, size, timestamp, sha256, copied_at)`, filled on `FileStored` while Alongside; at connect, Alongside passes `archive = false` and every key from `copied` newer than 60 days. Off passes `archive = true`, empty skip list (files copied but never archived are re-listed then; the store dedupes them by hash and archives them).
- Mode changes take effect on the next connection; the Settings screen says so and reconnects.
- Watch tab: in Alongside, the "Garmin Connect is using the watch" warning and the log hint are replaced by an info line "Sharing the watch with Garmin Connect"; if Garmin Connect's Nearby devices is off, it says Connect will not get data until it is allowed again.
- Sync tab: "Copied, left for Garmin Connect" count; the last result shows skipped files.

**Forward contract** (server and web; Android only shows a hint):
- Research (2026-11-19, from python-garminconnect 0.3.20 source, nothing called live): `docs/research/garmin-forward.md`. Login is mobile SSO → CAS ticket → DI OAuth2 bearer + refresh token at `diauth.garmin.com`; Garmin blocks non-browser TLS fingerprints at login, so the Rust login uses a browser-TLS client (e.g. `wreq`) if plain reqwest is refused, and Settings also accepts a token file from python-garminconnect as a fallback. Upload: multipart `file` to `connectapi.garmin.com/upload-service/upload`; duplicate = 409 with code 202. Live unknowns: refresh with plain reqwest, the success body, whether a type-32 monitoring file is accepted.
- `server/src/garmin/`: `auth` (email + password login, optional MFA code step, token store and refresh) and `upload` (POST one activity FIT; outcome `Uploaded { garmin_activity_id } | Duplicate | Rejected(reason) | AuthExpired | RateLimited | Transient`). The login flow is ported from the current python-garminconnect release (MIT; the version and file are cited in a comment); the builder checks that release before porting, not from memory.
- Secrets: the password is used once and never stored or logged; tokens live in table `garmin_account` (one row), are never logged and never returned by the API.
- Queue: table `garmin_uploads(file_sha256 PK, status, attempts, next_attempt_at, garmin_activity_id, last_error, created_at)`. Every activity file stored after Forward is switched on is queued; "also send the last N days" is an explicit backfill action (default none). Garmin's duplicate answer counts as done.
- Backoff: transient errors 1 min doubling to 6 h; 429 honours Retry-After; an expired login pauses the queue and the web shows "Log in to Garmin again". No upload is retried more than 10 times without an owner action.
- API (session auth and the `X-Nimrag: 1` header like the rest): `GET /api/garmin` (state `off | needs_login | needs_mfa | connected | paused`, queue counts, last error), `POST /api/garmin/login {email, password}` → `{mfa_required}`, `POST /api/garmin/mfa {code}`, `POST /api/garmin/logout`, `POST /api/garmin/retry`, `POST /api/garmin/backfill {days}`. Types via ts-rs.
- Web: Settings → Garmin Connect section (state, log in, MFA, counts, retry, log out), wording that says activities only.
- With Alongside on as well, Garmin gets the activity twice; the second is a duplicate and counts as done.
- Tests: a fake Garmin (axum test server on a local port, base URL configurable) for login, MFA, refresh, upload, duplicate, 401, 429 and 5xx. No test calls Garmin. Live test by the owner.

**Known risk, to measure live**: a file Garmin Connect fetches and archives before Nimrag lists it is never copied. Nimrag asks for a sync on every watch-initiated sync it sees and on its timer; the live test compares Nimrag's copies with Connect's synced list for a day. If misses are common, the fallback is Off mode plus Forward.

**Success Criteria**: core tests: archive false never emits 5008 in any path (including retries and stale confirmations); skip keys suppress download and are counted; archive true unchanged (all existing tests pass untouched). Live: with Connect's permission on and Alongside set, both apps stay connected, Connect still shows new data, and Nimrag stores the same files.
**Tests**: `core/tests/client_synthetic.rs` cases for archive false / skip; Android unit tests for the copied-key store and mode switch; live test by the owner.
**Status**: Alongside built (0.3.0, live test pending; the owner runs Nimrag only). Forward: server and web built (night of 2026-11-19), tested against a fake Garmin only.

## Gate 2
A week of normal wear syncs with no manual steps and no lost or duplicated files; file counts and hashes compared against a USB copy of the watch (note: the watch cuts monitoring/metrics files per sync, so compare at the level of files the watch lists, plus a USB copy taken at the end of the week).
**Status**: In progress. Unattended syncs every 15 minutes since 2026-11-20 (the owner's log of 2026-11-21 shows one every 15 minutes from 00:27 to 07:17, 0 failed). The week ends about 2026-11-27; then the USB copy and the comparison.

# Phase 3: web interface

Every page works at 390 px and at desktop width, uses the brief's design system (dark, Space Grotesk + Inter self-hosted, metric colours, 16 px card radius, 44 px touch targets, no colour-only meaning), and shows "unavailable" for missing metrics (hard rule 3).

**UI source (owner, 2026-11-19):** the design canvas "Self-hosted fitness app: phone and web" (claude.ai artifact), copied to `docs/design/artifact/project/*.dc.html`: 13 phone screens at 390 px (Today, Sleep, Strength session, Activities, Activity detail, Day, Health trends, Movement, Training, Body, Calendar, Workout builder, Devices and sync) and the web dashboard. Build the pages to match them: layout, colours, type, copy. Their numbers are sample values; the "Sample data" chip shows only when the API says the data is sample data.

## Stage 3a: JSON API and auth
**Goal**: read API over the derived tables (days, activities, sleep, trends), write API for strength-set edits and body measurements; web login with password and optional passkey; device tokens created and revoked in the web UI; API types generated from Rust (ts-rs or OpenAPI, chosen after verification), never written twice
**Contract (integrator, 2026-11-19)**:
- One owner account. `nimrag-server user set-password` (password from stdin, argon2id hash in the database). `POST /api/login {password}` sets an HttpOnly, SameSite=Strict session cookie (30 days, `Secure` unless `--insecure-cookies`); `POST /api/logout`. Login attempts are rate-limited. Passkeys come later in this stage, after the pages work.
- Every `/api` read needs the session cookie; uploads keep using device tokens (a device token cannot read). Requests that change something also need the header `X-Nimrag: 1` (CSRF guard with SameSite).
- Endpoints (all JSON, metric units in field names, every metric nullable: `null` is "unavailable", never 0): `GET /api/me`; `GET/POST/DELETE /api/tokens`; `GET /api/today`; `GET /api/days/{date}`; `GET /api/activities?type&before&limit`; `GET /api/activities/{id}` (stats, laps, zones, series); `GET /api/activities/{id}/track`; `GET/PATCH /api/activities/{id}/strength`; `GET /api/sleep/{date}` and `GET /api/sleep?nights=7|14|30`; `GET /api/trends?range=7d|4w|3m|1y`; `GET /api/movement/{date}`; `GET /api/training`; `GET/POST/DELETE /api/body`; `GET /api/calendar/{yyyy-mm}`; `GET /api/devices`; `GET /api/storage`; `GET/PATCH /api/settings` (timezone, step goal, intensity goal).
- Days are local days in the owner's timezone (setting `timezone`, IANA name; default the server's local zone).
- Responses carry `sample: true` when they come from seeded sample data.
- Types: Rust structs are the only definition; ts-rs writes them to `web/src/api/generated/`, and a test fails when the committed files are stale.
- `nimrag-server dev-seed --data-dir DIR` fills an empty data directory with 120 days of plausible sample rows in the derived tables (marked sample) for development, screenshots and the Playwright test; it refuses a directory that holds any uploaded file.
**Tests**: API integration tests through the router; auth tests; generated types compile in /web
**Status**: Done (7eb0ea7, hardened after a security review); accounts with 2FA replaced the single owner login (see "Multi-user"); passkeys not done.

## Stage 3b: web scaffold
**Goal**: /web with Vite, React, TypeScript, TanStack Query; uPlot for series; MapLibre GL JS with self-hosted PMTiles (no third-party tile servers); PWA with service worker + IndexedDB cache; frontend embedded in the server binary; Docker image
**Contract**: tokens from the design canvas (background #0F1417, card #171E22, raised #12181C, line #243037/#2A353B, text #E8EDEE / #9AA7AC, link #7DBBFF, metric colours: energy #4FD1B5, sleep #A594FF, heart #FF6B6B, steps #F2D45C, stress/warn #FFB066, zones #5B8DBE/#4FD1B5/#F2D45C/#FFA24D/#FF6B6B), Inter + Space Grotesk self-hosted (no Google Fonts at runtime), components (card, stat tile, ring, series chart on uPlot, zone bars, chips, segmented control, top bars with back / previous / next, bottom nav Today · Activities · Health · Strength, desktop sidebar layout from the web dashboard), a component gallery route for review, the login page, routing for every page as a stub. The server embeds `web/dist` (rust-embed) and serves it with an SPA fallback; the Dockerfile builds the web app first.
**Tests**: build in CI; Playwright smoke test against the real server binary
**Status**: Done; the Docker image builds the web app and embeds it (deployed 2026-11-20).

## Stages 3c–3m: pages (in the brief's order)
**Today page structure (owner, 2026-11-19, from screenshots of their Garmin Connect home, kept in `private/design/garmin-home/`; structure only, in our design system and our words, no Garmin names such as "Body Battery"):**
- **In focus**: a swipeable row of large cards with page dots; first Energy (value now, charged / drained, the day's energy line with stress bars below it and the wake time marked), then Sleep last night, then HRV.
- **At a glance**: a two-column grid of small cards the owner can **choose, reorder and hide** (Heart rate with resting and day range, Stress, Sleep score + duration + stages, HRV status with 4-week strip, Training status, Training readiness when the watch provides it, Intensity minutes this week, Training load, Steps, Energy, Respiration, Blood oxygen, Recovery time, VO2 max, Body weight). A card whose data does not exist shows "unavailable" or is offered as hidden by default.
- **Yesterday**: a list with one line per metric (energy +/−, steps, sleep score / duration, heart rate rest / high, stress, HRV 7-day average, intensity minutes).
- **Last 7 days**: activities count by type, averages for steps, sleep score / duration, resting heart rate, stress.
- **Edit home** button at the end: the card picker (show/hide toggles, drag and keyboard reorder, reset to default). The layout is stored on the server (setting `home_cards`: ordered list of card ids plus hidden ids) so phone and web match.
- Garmin services (events, coach plans, challenges) are left out.
- **Top bar on the main tabs** (owner): user icon on the left (opens profile and settings), and on the right a sync button and a watch icon. The watch icon opens Devices and sync and carries a small status dot (connected / waiting / problem). The sync button asks the watch for a sync when the page runs inside the Android app (through the shell's JS bridge, stage 3n) and shows the spinning state while it runs; in a plain browser it refreshes the data from the server and its label says so. Both show the last sync time on long press / hover.

3c Today · 3d Activities list + detail (map, stats, HR zones, laps) · 3e Strength session (edit sets, compare to previous) · 3f Sleep night (+ last 7 nights) · 3g Day view (HR, stress, energy; prev/next day) · 3h Health trends (HRV vs personal range, RHR, breathing, SpO2, sleep score; 7 d / 4 w / 3 m / 1 y) · 3i Movement · 3j Training (recovery time, 7-day load vs target, load by intensity, VO2 max as max MET × 3.5 labelled derived) · 3k Body (weight trend, manual measurements) · 3l Calendar · 3m Desktop extras (overview, activities table, strength progress, compare and reports, devices, storage and backups)
- **Status pages** (after Garmin Connect's layout, information only): Training status (Current / 4w / 12w / 6m, a timeline with one coloured segment per day, a list of runs of equal status), HRV status (1d / 7d / 4w, 7-day averages against the balanced range with a marker per status, overnight toggle, a card per day) and Sleep score (1d / 7d / 4w / 1y, score line, time-asleep bars, avg / avg highest / avg lowest). Served by `GET /api/training/status`, `/api/hrv` and `/api/sleep/history`; routes `/training/status`, `/hrv` and `/sleep?range=`. Training status codes verified so far: 1, 4, 5, 7, 8 (0 = no status); others show as "Status N".
**Tests per page**: renders from real synced data; "unavailable" states; keyboard reachable controls; labelled charts with text summaries; contrast ≥ 4.5:1
**Status**: Done (night of 2026-11-19, 52 Playwright tests incl. axe on a seeded server). On real data since 2026-11-20, then reworked from the owner's phone reviews: Garmin-style status pages (training status timeline, HRV baseline, sleep score and duration), metric detail pages behind the Today tiles (0.6.0), value axes on every chart, maps from the server's own tiles.

## Stage 3n: Android shell
**Goal**: the app shows the web interface in an embedded web view with a native bottom bar; offline viewing from the PWA cache; Devices and sync stays native
**Bridge contract (integrator, 2026-11-19)**:
- The app loads the saved server URL (Settings; blank → a native "Set your server" screen). Only that origin is loaded in the web view; other links open in the browser. No file access, no mixed content, cookies persist (session login happens in the web view).
- Android exposes `NimragAppNative` (addJavascriptInterface, methods `syncNow()`, `status(): String` JSON `{link, lastSyncAt}`, `openWatch()`, `onRoute(path)`), and a document-start script restricted to the server origin (`WebViewCompat.addDocumentStartJavaScript`) defines `window.NimragApp = { native: true, syncNow, status (parsed), openWatch, onRoute }`. Every interface method also checks that the web view's current URL is on the server origin.
- The app dispatches `nimrag:status` on `window` when the link state or last sync changes, and `nimrag:navigate` with `detail.path` when a native tab is tapped. Since 0.5.1 also `nimrag:data` (no detail) after new watch data reached the server (stage 3q).
- Web: with `NimragApp.native`, the web bottom bar is hidden (the native one shows Today · Activities · Health · Strength · Watch), the watch icon calls `openWatch()` instead of linking to /devices, the router follows `nimrag:navigate`, and route changes call `onRoute(path)` so the native bar highlights the right tab.
- Watch tab = the existing native Watch, Sync and Settings screens, restyled to the canvas's Devices design and tokens.
**Status**: Done (0.4.0); in daily use against the deployed server. The web view reloads itself when the server has a newer web build (0.5.1); the bridge status carries `syncing` (0.6.1, 0.6.2).
**Android notes (2026-11-19)**: tab taps send `nimrag:navigate` with path `/`, `/activities`, `/health`, `/strength` (the web router must map `/strength` to `/activities?type=strength`); `onRoute` accepts both forms. `status()` link mapping: no watch = unknown, pairing / Waiting / Connecting = waiting, Ready = connected, Offline / BondLost / unsupported model / Bluetooth permission or adapter off = problem. The bridge script is injected with `addDocumentStartJavaScript` for the server origin when the web view supports DOCUMENT_START_SCRIPT, else on page start and finish (a page that reads `NimragApp` very early can then miss it). Offline: the service worker serves cached pages; with no network the web view also prefers its HTTP cache; with no cached page the native "Server unreachable" overlay shows (a main-frame network error or HTTP 502 to 504). Service workers need a secure context: https, or `localhost` / `127.0.0.1`; a plain-http LAN address has no offline cache.

## Stage 3o: local server on the phone (spike, personal flavor only)
**Goal** (owner): the app loads the data locally on sync, so the newest data shows at once and offline; the existing upload queue still pushes the files to the server.
**Design under test**: the same server code (`nimrag_server::embedded`, ffi feature `local-server`) runs inside the app process on 127.0.0.1. After the engine stores a watch file it also ingests it into the local server (`files.local_at` in the engine db marks done; a catch-up pass at start covers the rest). Guard: every request needs the launch secret in the `nimrag_local` cookie (set through `CookieManager` before the first load), else 403; the Host header must be loopback; the owner session is attached server side, so there is no login. Read-only over HTTP (non-GET gets 403): edits on the phone would never reach the real server. The web view shows the server when it is the better choice and the phone's copy when offline, when the server did not answer, or while uploads are pending (`WebSourcePolicy`).
**Flavors**: `public` = no FIT profile, no local server code in the library. `personal` = profile + local server, built only on the owner's machine, never published.
**Multi-user (2026-11-20)**: the phone's copy has exactly one local user (`local`, role user, no admin); its files belong to that user; the session is attached server side; read-only (GET/HEAD) with the per-launch secret cookie and a loopback Host check; Forward is never started; an older spike data dir becomes that user's. Gradle refuses a personal variant from any release/publish/upload/sign task without `-Pnimrag.personal`.
**Status**: spike works on the Pixel 8a (see the spike report); open work is listed there.

## Stage 3p: app sign-in with the accounts (Android, 0.5.0)
**Goal**: the app signs in with the multi-user server: native sign-in, silent token, unlock by fingerprint or password, the web app opens signed in.
**Design**: see `android/README.md`, "Signing in". Server address + username + password + code → `/api/app/login`; the token goes to the upload worker, the device secret into the Keystore (fingerprint-bound copy and a password copy), the session cookie into `CookieManager`. Session gone → native unlock (`/api/app/resume` or `/api/app/unlock`); a revoked secret or a refused upload token wipes everything and shows the sign-in. Web: `NimragApp.sessionExpired()` replaces the login page inside the app. Manual token entry removed. Sign out revokes the phone on the server.
**Status**: Done (0.5.0); verified on the Pixel 8a against a local server and in use on the owner's Pixel 9a: sign-in, signed-in web view, Settings account card, a web 401 landing on Unlock, unlock by password (wrong then right), a revoked phone landing on Unlock with the failure text and "Use a different account", sign-out (server token revoked, session gone) and Settings, "Sign in" opening the sign-in screen. Not yet verified on a device: the fingerprint unlock (no fingerprint is enrolled on the test phone), the upload-401 path (covered only by the unit-tested policy, `UploadPolicy`, and code review) and the personal flavor's offline local copy without an account.

## Stage 3q: new data shows up soon (Android 0.5.1)
**Problem** (owner, 2026-11-20): new watch data (Body Battery etc.) took hours to show. Files reached the server seconds after a phone sync, but phone to watch syncs ran at 09:27, 13:01 and 15:52 UTC with auto sync = 60 min.
**Fix**: (1) the auto-sync wake-up is an `AlarmManager.setAndAllowWhileIdle(ELAPSED_REALTIME_WAKEUP)` alarm (`engine/AutoSyncAlarm.kt`, no exact-alarm permission) delivered to `AutoSyncReceiver` (`goAsync`), which runs `WatchLink.onAutoSyncAlarm()` on the link's thread: sync when due (`LinkRules.shouldStartAutoSync`, wall-clock last sync, never while one runs or was just asked for), hurry a reconnect that waits out its (uptime) delay, set the next alarm (`LinkRules.nextAutoSyncDelayMs`). Backstop: the same due-check runs after the handshake, after every step of the client (any watch event) and when the interval changes. The alarm is cancelled for Off (0), when the service stops and on forget. Default interval 15 min (the Settings card already offered it). (2) Opening the app (`ON_RESUME`, `WatchController.appOpened()`) starts a sync when the link is ready and the last finished sync is older than 5 minutes (`LinkRules.shouldSyncOnOpen`; auto-sync Off stays off); `ServiceStartGate` is untouched. (3) After an upload pass that took at least one file, and (personal flavor) after the local server ingested files, `AppState.dataVersion` is bumped and the shell runs `window.dispatchEvent(new CustomEvent('nimrag:data'))` in the page; `web/src/dataRefresh.ts` debounces 2 s and calls `queryClient.invalidateQueries()`, and refetches stale active queries when the page becomes visible again.
**Status**: Done (0.5.1). Verified live 2026-11-21: the owner's log shows an auto sync every 15 minutes through the night (the alarm keeps firing while the phone is idle; Doze itself not confirmed), and the page showed the new data after a sync (Energy 78 → 76 after the 09:53 sync).

## Gate 3
Each page renders from real synced data, shows "unavailable" correctly for missing metrics, and passes an accessibility check.
**Status**: In progress. Every page runs on the owner's real data (watch files plus the imported Garmin Connect history) and was reviewed on phones at 100 % text size; the axe check has only run on a seeded server.

# Phase 4: writing to the watch and operations

## Stage 4a: workouts
**Goal**: workout builder (steps, repeat blocks) saved on the server, encoded with a new `fit::encode` (typed inputs; BaseType size/invalid/to_byte), sent to the watch through the phone
**Success Criteria**: the workout appears and runs on the watch (verified on the watch before calling it done)
**Approach**: port Gadgetbridge's workout encoder (`fit/workouts`) and its upload path to the watch; confirm the upload path with one capture of a workout transfer before calling it verified (Q-SYNC-2)
**Status**: Not Started

## Stage 4b: backups
**Goal**: scheduled copy of the file store and database; documented restore test that rebuilds from the files with `reparse`
**Status**: Not Started

## Stage 4c: export
**Goal**: all original files plus CSV per data type
**Status**: Not Started

## Stage 4d: runbook (`docs/`)
**Goal**: install, upgrade, restore, re-pair the watch, firmware updates over USB
**Status**: Not Started

## Gate 4
Final report: what works, what was verified and how, what is still unknown.

## Multi-user (2026-11-20)
**Goal**: several users on one server. The first-run setup creates the admin, who only sets the server up. Normal users are created by the admin and own all data. Sign-in is username, password and TOTP 2FA; on the phone the first login is the same and later unlocks use the password or the fingerprint.
**Contract** (details in `server/README.md`, "Accounts, 2FA and the app"):
- `users` (unique case-insensitive username, argon2id, role admin|user, TOTP RFC 6238 with replay rejection, `must_change_password`, `disabled_at`); sessions carry `user_id`; the admin gets 403 on every data endpoint.
- Ownership: `tokens.user_id`, `file_owners(sha256, user_id)` filled at ingest and assign; sample tables have `user_id` in the primary key (0 = unassigned); settings, body entries, strength edits and Garmin Forward are per user; `reparse` keeps ownership. Single owner per file, first claim wins.
- First run: `/api/setup` + `/api/setup/confirm` (closes only once 2FA is confirmed). Login: `/api/login` (indistinguishable failures, per-address and per-username limiter, enrol-2FA and change-password steps). Admin API for users and the unassigned data (`/api/admin/*`).
- App API: `/api/app/login` (token + `device_secret`, 2FA once), `/api/app/unlock` (password + secret), `/api/app/resume` (secret only; the fingerprint gate lives on the phone in the Keystore). Revoking the token, or disabling the user or resetting their password or 2FA, kills the secret.
- Migration 0010 is lossless: the owner becomes user `admin` (same hash, 2FA not enrolled), sessions are cleared, old tokens, files and records stay unassigned until the admin assigns them to a user.
**Device tokens are a background thing (owner, 2026-11-20)**: a user never creates, sees or copies a token. The app's sign-in (`/api/app/login`) issues it and the app keeps it. `POST /api/tokens` is gone; web Settings show "Signed-in phones" (`GET /api/tokens`, active ones only) with "Sign out this phone" (`DELETE /api/tokens/{id}`, which also kills the device secret and sessions). The CLI `token create` stays for tools and the admin's unassigned data. A phone with a hand-entered token uploads until it is refused, then shows the sign-in.
**Status**: Server and web done and tested (server integration tests for migration, accounts and 2FA, admin, isolation, app login and the CLI; web unit tests and Playwright against the real server). Android app: stage 3p below.
**Open**: file ownership is stored only in the database, so `rebuild-ledger` from sidecars yields unowned files; a user without 2FA can enrol with the password alone at first login (by design, so new users get their temporary password in person); a Garmin Forward account from before accounts (user 0) idles until an admin assigns it.

## Garmin Connect history (2026-11-20)
**Goal**: the owner's year of Garmin Connect history (pulled as JSON through a Connect connector) shows on the history pages before the watch has synced that long to this server.
**Contract** (details in `server/README.md`, "Garmin Connect history"):
- `nimrag-server import-connect --user USERNAME [--timezone IANA] <dir>`: same lock as `import-export`, data owned by the named normal user. Each JSON becomes a content-addressed blob (`files.source = 'connect'`) holding an envelope around the original text, derived through `ingest` and `reparse` like a FIT file, so it survives reparse, assign and restarts; a re-import adds nothing.
- Rank: `sources.origin = 'connect'` with a rank key starting `-`, below every FIT key: the watch wins whatever the import order. A Connect activity is hidden by a FIT activity of the same user starting within 120 s, a Connect night by an overlapping FIT night (views `activities_current`, `sleep_sessions_current`).
- Activity records (added 2026-11-20): `DIR/records/<id>_<offset>.json` (pages of the connector's `get_activity_fit_messages` with `record`, `time_in_zone` and `zones_target`) is optional. For an activity file with pages, the importer keeps the columns of `activity_records` (time, position in degrees, heart rate, speed, altitude, distance, cadence, temperature) and the `time_in_zone` messages (seconds per HR zone, as a watch FIT file) as a `records` member of the activity's envelope, so Route, Heart rate, Elevation and Time in heart rate zones fill like a watch activity and survive reparse. Fields are found by FIT definition number (the connector's decoder lacks parts of the profile) and scaled with the profile's factors when it did not name them. An activity imported earlier without records becomes a second blob with the same `connect:<id>` key that outranks it (more rows); the old one stays, hidden by `activities_current`. Zone boundaries and zones computed from heart rate are not stored.
- Mapped onto the existing tables (training status and load, HRV with baselines, resting HR, VO2 max, sleep, activities with sessions, laps and strength sets); new: `sleep_sessions` stage totals and `daily_totals` (steps and calories per day, used when a day has no minute data). Migration 0011 also rebuilds `files` so its `source` CHECK accepts `connect`.
**Status**: done and tested on synthetic fixtures; dry run on a scratch copy of the real history showed training status runs from May to November, a year of sleep nights, 28 days of HRV with baselines and the activity list.
**Open**: the real history has no night to check Connect's sleep instants against the watch-verified night of 2026-11-19/20 (it was pulled before that night); the instants were checked against the HRV local strings instead. Date-only rows sit at local midnight, so a DST change inside the imported range moves nothing but is unverified on real data; a user who changes zone and re-imports gets a second set of blobs (the zone is part of the blob). Records and zones need the pull of `records/` first (a pull in progress gives them for those activities only; re-run the import afterwards). Run against the live database for the owner's user on 2026-11-20 (backup first), re-run with activity records and zones, and again with the daily summaries (below).
**Daily summaries (2026-11-20, migration 0012)**: `daily_stats/YYYY-MM.json` (Connect's day totals: steps, distance, floors, intensity minutes, heart rate range, stress average, body battery, breathing) go into `connect_daily`; a page falls back to it per metric per day only when the watch has no row of that metric that day. 365 days imported live. Known artefact: on the first day of watch data (2026-11-19, watch rows only from 21:06) the watch's fragment wins over Connect's full day.

## Live use (2026-11-20/21)
- **0.6.0** (2026-11-20 evening): value axes on every chart; metric detail pages behind the Today tiles (`/metrics/<key>`) from a clean-room spec (`docs/design/clean-room/metric-detail-spec.md`); Watch tab as one page with sub-pages; Profile page in the web app.
- **Training load tile** reads the newest training status row that has a load (the watch writes today's row with a status only).
- **Maps**: `serve --tiles-dir DIR` (or `NIMRAG_TILES_DIR`) serves `*.pmtiles` to signed-in users with range requests; the web app draws routes on a Protomaps basemap and needs MapLibre's worker URL set explicitly (fixed 2026-11-20). The owner's extract covers the areas of their routes (git-ignored `tiles/`); a route elsewhere has no basemap until the extract is redone.
- **0.6.1 / 0.6.2**: the sync icon spins while the app pulls from the watch and uploads, then until the page has fetched the new data; recovery time 0 reads "Recovered".
- **Day totals across midnight (2026-11-21)**: the watch writes the finished day's cumulative totals stamped exactly 00:00 before it resets, so today showed yesterday's steps. Cumulative counters (steps, distance, calories, intensity minutes) now count for the day of `ts - 1`.
- **Open**: SpO2 has no readings yet; intraday Connect data for imported days (their Day views have no charts); training status, load, recovery and VO2 max change only when the watch writes its metrics file (after an activity), which is expected.

# Appendix: contracts and reviewer notes

### Reviewer notes for stage 1d (parsers), recorded 2026-11-19
- timestamp_16 / timestamp_min_8 (monitoring) and event_timestamp_12 (hr) accumulate against the last full timestamp; timestamps < 0x10000000 are device-relative, not UTC → show as unavailable.
- Cross-file duplicates (watch vs export, overlapping monitoring): derived entities need natural keys (e.g. file_id serial + time_created; (timestamp, metric)) and a precedence rule independent of ingest order.
- Expansion of components/subfields and accumulation live in a pure `fit::interpret` layer; the decoder stays raw.
- Phase 4 encoder: separate `fit::encode` with typed inputs; needs BaseType::{size, invalid, to_byte}.

## Stage 1d contract (reviewer draft, accepted by integrator 2026-11-19)

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
