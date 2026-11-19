# Licensing record

Hard rule 6 of the build brief: if any Gadgetbridge (AGPL-3.0) code or message
definitions are ported or copied, including by translation into Rust, the
component containing them becomes AGPL. This file records which path was taken.

## Path taken: original implementation (no Gadgetbridge code)

| Component | Status | Basis |
| --- | --- | --- |
| `/core` | Original | Written from `docs/protocol/`, which was derived from static analysis of the Garmin Connect app and from our own HCI captures. |
| `/tools/decode` | Original | Thin CLI over `/core`. |
| `/tools/proto-recover` | Original | Extracts schemas from the app's compiled protobuf descriptors. |
| `docs/protocol/proto/*.proto` | Recovered from the Garmin Connect app | Generated mechanically from the app's own descriptors; not taken from Gadgetbridge's `.proto` files. |

How Gadgetbridge was used:

- It was read as documentation only, at commit `7257742b` (see `docs/protocol/PROVENANCE.md`).
- A survey of it was written in our own words and kept in the private notes. It is not in the repository.
- Every protocol fact we implement was checked against the Garmin app or a capture before it went into code. Where Gadgetbridge and the app disagree, `docs/protocol/PROTOCOL.md` says so and we follow the app or the capture.
- Generic algorithms are implemented from their public definitions: CRC-16/ARC, COBS, the btsnoop format, Bluetooth HCI, L2CAP and ATT.

Rules for future work:

- Do not copy or translate code from Gadgetbridge into any module listed above.
- If someone decides to port Gadgetbridge code, put it in a separate, clearly named module and mark it AGPL in `LICENSE` and `NOTICE`. Because `/core` is linked into both the server and the Android app, that decision makes both AGPL. The owner must agree to it first.

## Project licence

The owner has not chosen a licence yet. Crates declare `LicenseRef-nimrag-pending` until then.

## Garmin material

- APKs, decompiled sources and raw captures stay in `private/`, which git ignores, and are not redistributed.
- The recovered `.proto` schemas describe a wire format and exist for interoperability with the owner's own device. Owner decision, 2026-11-19: they are published in the public repository (github.com/AngelFreak/Nimrag-Connect), as Gadgetbridge publishes its own schemas. The decompiled sources, APKs and captures they were recovered from stay private.
- FIT profile (owner decision, 2026-11-19, revised the same day):
  - Garmin's public FIT SDK (npm `@garmin/fitsdk` 21.222.0) ships under the "FIT Protocol License Agreement". That licence treats the SDK as confidential and forbids distributing it or anything derived from it (§2(c), §4). The table extracted from the app is Garmin's work too.
  - Decision: **profile tables are generated locally and never committed.** Only the generator (`tools/fit-profile/`) and the lookup API (`core/src/fit/profile.rs`) are in the repository.
  - The generated data goes to `core/src/fit/generated/`, which git ignores. It is produced from the owner's own copy of the Garmin Connect app, or optionally from the FIT SDK for personal use under its licence.
  - Without the generated file, `core` builds with an empty profile. Raw decoding works and names are unavailable.
  - `NIMRAG_FIT_PROFILE=none` forces the empty profile, which is the configuration that gets committed and tested.
  - Binaries and Docker images built with a generated profile contain it. Do not push or share them.
  - Code and tests may name individual standard FIT messages, fields and enum values (for example `record.heart_rate`) where they are needed for interoperability. Tables of profile data may not be committed. This rule was proposed by the advisor and follows the owner's decision; the owner can tighten it.
