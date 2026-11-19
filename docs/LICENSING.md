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
- The recovered `.proto` schemas describe a wire format and exist for interoperability with the owner's own device. Remove them from any public branch if the owner decides they should not be published.
- FIT profile (owner decision, 2026-11-19):
  - `core/src/fit/profile_data.rs` is generated from Garmin's **public** FIT SDK (npm `@garmin/fitsdk`) under that SDK's licence. The package version and licence are recorded in the file header.
  - `core/src/fit/profile_overlay.rs` holds only the messages, fields and enum values the public profile lacks, extracted from the FIT SDK copy inside the Garmin Connect app. It is governed by the app's terms rather than the FIT SDK licence. It is kept separate so that it can be replaced by an empty table on any public branch.
