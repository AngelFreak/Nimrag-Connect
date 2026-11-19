# Provenance

Everything in `docs/protocol/` was derived from the inputs below. A "seen" finding
cites a class and method name; those names are only valid for this exact app build.

## Target app

| Item | Value |
| --- | --- |
| Package | `com.garmin.android.apps.connectmobile` |
| Version | 5.31 (versionCode 11761), minSdk 32, targetSdk 36 |
| Pulled from | Pixel 9a, Android 17 (user build), via `adb pull` of every `pm path` entry |
| Pulled on | 2026-11-19 |

| File | SHA-256 |
| --- | --- |
| `base.apk` | `7df724b82070a55a86f1ec41c98ef1f88aec772b2b735cd2a0212bdde81e4cd7` |
| `split_config.arm64_v8a.apk` | `7fb8b957efd2b20f01e5fdada5cf0ada2cac18de71bc410ea6718847a46485f1` |
| `split_config.xxhdpi.apk` | `ce8ec5a8c56188ddca5e02e4e49ca4854a02d2d31ec41c8c28bdf442f338d4c0` |
| `split_gcm_on_demand_resources.apk` | `216be04713d0752a0b7532b2baf45445cd1ecc05178e9226229d12ffb4332ed6` |

The APKs and decompiled output are kept in `private/` (git-ignored) and are not
redistributed.

## Watch

| Item | Value |
| --- | --- |
| Model | Instinct 2 Solar Tactical (BLE name "Instinct 2 SolTac") |
| Firmware | Software 17.22 (0b8ee59); GPS 6.70; Connect IQ 3.4.4; BMX 14.0.6; BLE/ANT 1.04; Sensor Hub 27.60 (Garmin Connect device page) |
| Unit ID | _recorded privately, not committed_ |
| Bond | LE bonded to the phone, LE link encrypted (16-byte key), seen in `dumpsys bluetooth_manager` |

## Reference implementation

| Item | Value |
| --- | --- |
| Project | Gadgetbridge, `https://codeberg.org/Freeyourgadget/Gadgetbridge` (AGPL-3.0) |
| Commit | `7257742b96a30fff0db9e79230435ac2aeb39f57` (2026-11-18) |
| Garmin code | `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/` (path confirmed) |
| Garmin protos | `GBProtobufGenerator/src/main/proto/garmin/`. The brief expected `app/src/main/proto/garmin/`, which does not exist at this commit. |

Gadgetbridge was read as documentation only. No code, message definitions or
`.proto` text from it is in this repository; see `docs/LICENSING.md`.

That describes phase 0. On 2026-11-19 the owner decided to port Gadgetbridge's Garmin work under the AGPL from here on; see `docs/LICENSING.md` and `docs/PORTED.md`.

## Tools

| Tool | Version |
| --- | --- |
| JADX | 1.5.3 (`--deobf --show-bad-code`) |
| REA (`rea-agents`) | 6.9.0, on Node 24.22.3 |
| jadx-headless-mcp (REA provider) | 0.7.1, SHA-256 `6e5eacf500b64292bfb73c49797c1958f6ee44646e43e868039ae7feb573ff75` |
| Ghidra | 12.2 (REA provider), Temurin JDK 25.0.5.1 |
| Java (JADX) | OpenJDK 23.0.2 |
| adb | 1.0.41 |
| TShark | 4.2.2 |
| Frida | 17.26.2 (installed, not used in this phase) |
