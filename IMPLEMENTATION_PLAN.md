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
**Status**: Not Started

## Stage 1b: FIT profile from the app (/core/src/fit/profile.rs, tools/fit-profile/) — builder B
**Goal**: generated table of message/field names, types, scale/offset/units and enum value names, extracted from the FIT profile bundled in Garmin Connect 5.31
**Success Criteria**: lookup API per contract; covers record, session, lap, monitoring, sleep_level, hrv_*, set, exercise_title, etc.; generator reproducible
**Tests**: known entries (record.heart_rate, session.total_timer_time scale 1000, sport enum names)
**Status**: Not Started

## Stage 1c: server ingest (/server) — builder C
**Goal**: axum + sqlx SQLite (WAL) service: file store by SHA-256, POST /api/files idempotent, device-token auth, migrations, `reparse` subcommand skeleton, raw FIT message table
**Success Criteria**: upload same file twice -> one stored copy, 200 both times with same id; unauthenticated -> 401; reparse rebuilds derived tables from store
**Tests**: integration tests through the real router (axum test client) and the real binary for reparse
**Status**: Not Started

## Stage 1d: parsers (activities, monitoring, sleep, HRV, metrics) — after 1a+1b
## Stage 1e: Garmin export importer — after 1c
## Gate 1

### Advisor notes for stage 1d (parsers), recorded 2026-11-19
- timestamp_16 / timestamp_min_8 (monitoring) and event_timestamp_12 (hr) accumulate against the last full timestamp; timestamps < 0x10000000 are device-relative, not UTC → show as unavailable.
- Cross-file duplicates (watch vs export, overlapping monitoring): derived entities need natural keys (e.g. file_id serial + time_created; (timestamp, metric)) and a precedence rule independent of ingest order.
- Expansion of components/subfields and accumulation live in a pure `fit::interpret` layer; the decoder stays raw.
- Phase 4 encoder: separate `fit::encode` with typed inputs; needs BaseType::{size, invalid, to_byte}.
