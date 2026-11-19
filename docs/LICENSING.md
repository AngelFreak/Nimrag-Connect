# Licensing record

Hard rule 6 of the build brief: if any Gadgetbridge (AGPL-3.0) code or message
definitions are ported or copied, including by translation into Rust, the
component containing them becomes AGPL. This file records which path was taken.

## Path taken: AGPL-3.0-or-later, porting from Gadgetbridge (owner decision, 2026-11-19)

The owner decided to port Gadgetbridge's Garmin wearable work instead of decoding every device from scratch. Following rule 6, the whole project is licensed **AGPL-3.0-or-later** (`LICENSE`, `NOTICE`, `license` in `Cargo.toml`). `/core` is linked into the server and the Android app, so all three are covered.

History: until this decision, every module was written without Gadgetbridge code (from `docs/protocol/`, the Garmin Connect app's behaviour and our own captures), with Gadgetbridge read as documentation only at commit `7257742b`. That code stays as it is; it is now simply AGPL like the rest.

### Rules for ported code

1. **Source and attribution.** Port from Gadgetbridge commit `7257742b96a30fff0db9e79230435ac2aeb39f57` unless a later commit is recorded. Each ported Rust or Kotlin file starts with a header naming the Gadgetbridge source file(s) and the commit, and keeps the original copyright lines. `docs/PORTED.md` lists every ported file with its source.
2. **Evidence still applies (hard rule 5).** Ported behaviour is labelled *inferred (Gadgetbridge)* in `docs/protocol/` until a capture or a test on a real device shows it. Where Gadgetbridge and our captures disagree, the capture wins and `PROTOCOL.md` says so. Devices we don't own are listed as *supported per Gadgetbridge, not verified*.
3. **No Garmin profile data.** Gadgetbridge's generated FIT message and field tables are derived from Garmin's FIT SDK profile. The owner's earlier decision stands: profile tables are generated locally and never committed (see below). Port the code that uses them, not the tables.
4. **No artwork.** Gadgetbridge's artwork (`LICENSE.artwork`, CC BY-SA 4.0) is not copied.
5. **Network use.** AGPL §13: users who interact with the server over a network must be offered its source. The web interface links to the source repository and the running version (phase 3).
6. **Credit.** Gadgetbridge is credited and thanked in `README.md` and `NOTICE`.

### Recovered from the Garmin Connect app

| Component | Basis |
| --- | --- |
| `docs/protocol/proto/*.proto` | Generated mechanically from the app's own descriptors by `tools/proto-recover`, not taken from Gadgetbridge's `.proto` files. |
| `docs/protocol/*.md`, `*.csv` | Our own notes from static analysis of the app and our own captures. |

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
