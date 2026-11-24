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
   - Product ids: the device catalogue takes Garmin product numbers from the `devices` section of Gadgetbridge's `FitCodeGenerator/src/main/resources/fit_profile.json`. That file describes itself as Gadgetbridge's community-maintained description of FIT, and its product-to-model links are maintained by Gadgetbridge contributors, so the mapping is ported AGPL work. Only that section is used; its message and enum tables are not ported (integrator decision, 2026-11-19).
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
  - Without the generated file, `core` builds with Nimrag's own field map (next section). Raw decoding works for everything; names, scales and enum values exist only for what Nimrag reads.
  - `NIMRAG_FIT_PROFILE=none` forces Nimrag's own map (no Garmin profile), which is the configuration that gets committed, tested and published. `NIMRAG_FIT_PROFILE=empty` forces the committed empty table (no names at all), which is also tested.
  - Binaries and Docker images built with a generated profile contain it. Do not push or share them.
  - Since 2026-11-20 the profile can also be generated from Garmin's public Python FIT SDK (`tools/fit-profile/from_sdk.py`, FIT Protocol License) plus a local overlay of Garmin-internal fields; the same local-only rules apply to its output and to the overlay.
  - Code and tests may name individual standard FIT messages, fields and enum values (for example `record.heart_rate`) where they are needed for interoperability. Tables of profile data may not be committed. This rule was proposed by the advisor and follows the owner's decision; the owner can tighten it.

### Nimrag's own FIT field map (stage 5a, 2026-11-23)

- **What it is.** `core/src/fit/profile_nimrag.rs` is a small committed table (24 messages, about 140 fields, 18 enum values) in the same shape as a generated profile. It names and scales only the messages and fields Nimrag reads in `core/src/derive/` (activity, battery, HRV, metrics, monitoring, sleep) and the enum values those derivers and the web interface use. Since 2026-11-24 it also holds the messages the two FIT writers need (workout, workout step, step notes, weather). It is not a general FIT profile; `nimrag-decode` shows everything else by number.
- **Why it is allowed.** It is Nimrag's own work, not Garmin's profile or anything derived from it. The numbers, base types, scales and offsets were read off the owner's own watch files (a raw decode with no profile) and checked against values Garmin Connect reports for the same data, against internal consistency of the files (for example a session's total distance equals the last record's) and against physical sense. The names are the ones already used in Nimrag's own public code, which the rule above allows for interoperability. The analysis (spec and evidence) never read Garmin's profile, the FIT SDK, the local overlay, the app decompilation, `tools/fit-profile` output, or any published profile table (Gadgetbridge, fitparse, fitdecode and the like). The spec stays in `private/fitmap/` and is not published.
- **How it was made.** A clean-room split: an analyst who had none of the Garmin material wrote the spec; the builder who wrote the table read only that spec and the committed Nimrag code. Entries the analyst could not confirm are left out, so the map is partial on purpose (for example no SpO2). Where two fields were indistinguishable in the data, the table takes the better-supported one and says so in a comment.
- **The write side (2026-11-24).** The workout messages were read off Garmin's own workout files the same way as above. What those files never contain (some step types and targets, the weight unit, and the whole weather message) was settled by **output comparison**: the integrator ran Nimrag's writers twice on the same 31 inputs, once with Garmin's profile and once with the map, and a tool told the analyst only which message and which of the analyst's own field numbers differed, never the reference bytes or values. Those entries are marked `// O:` in the table. All 31 outputs are now byte-identical. Two caveats are recorded on purpose: the comparison build used Garmin's profile as a black box, and for the weather message the analyst's first guess at field numbers, base types and two enum lists came from their own memory of the public FIT profile before the comparison confirmed them. The owner decided on output comparison for the write side; the memory-based guesses are disclosed here rather than hidden.
- **How it is selected.** `NIMRAG_FIT_PROFILE` unset: Garmin's generated profile if it exists, else this map. `none`: this map. `empty`: the empty table. A path: that Garmin profile file. `cfg(fit_map)` marks the map; `cfg(fit_profile)` and the profile marker mean Garmin's profile only, so the Android licence guard and the "never published" rules concern only that. The public server image and the public APK are built with `none` and contain the map.
- **What it cannot do.** Read everything: messages and fields outside the table have no names, scales or enum values (for example SpO2 and some strength details). The writers are complete for what Nimrag writes; with the `empty` table they still answer "unavailable" (HTTP 503) and write nothing.
