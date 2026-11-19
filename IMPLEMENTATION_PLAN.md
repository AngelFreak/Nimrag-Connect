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
