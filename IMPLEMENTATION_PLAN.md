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
