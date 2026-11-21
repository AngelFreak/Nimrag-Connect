# Garmin Instinct 2 ↔ phone link protocol

Status: phase 0. The static map is complete and one live capture has been decoded.

## Summary

- **Transport.** The Instinct 2 (product 3888, firmware 17.22) uses the multi-link GATT service `6A4E2800-667B-11E3-949A-0800200C9A66`. Each app gets one notify/write characteristic pair (`6A4E281x`/`6A4E282x`). GFDI runs on it as service 1, in the reliable mode (MLR). The watch grants MLR when asked. The negotiated ATT MTU is 23, so every packet is at most 20 bytes. [seen: APK + CAP-1]
- **Application-layer crypto: none in our setup.** The app has a "Garmin Auth" scheme: message ids 5101–5112, a session key and per-packet counters; the cipher itself was not identified. It is only used when the phone is not BLE-bonded to the watch, or the phone is made by VIVO. The Pixel is bonded, and in CAP-1 every GFDI message after COBS is plaintext with a valid CRC. Link-layer security is standard LE bonding with a 16-byte key. [seen: APK `DefaultAuthDelegate.isGarminAuthAllowed`; CAP-1]
- **Where the app and Gadgetbridge disagree:**
  - unhandled requests: the app auto-replies, Gadgetbridge stays silent
  - 5101: the app treats it as an error on a bonded link; Gadgetbridge echoes it back
  - file chunks: the app has resend and abort statuses; Gadgetbridge names them but never sends one
  - reply to 5024: Gadgetbridge sends a bare ACK and then the ACK with host details; the app sends one
  - 5052 reply: the app sends (next DST start, next DST end); Gadgetbridge sends (transition after next, next transition), which is the same only in summer time
  - compact transaction id: the app masks it to 5 bits and echoes it in replies; Gadgetbridge drops it and never echoes it
  - protobuf schemas: Gadgetbridge has wrong types in RealtimeSettings and lacks the app's default values

  The multi-link and MLR header layouts agree. The full comparison, with every fact taken from Gadgetbridge alone, is §9. (An earlier version of this list said Gadgetbridge expects a 17th byte in FileReady; at `7257742b` it reads 16, as the app does.)

### Evidence keys

| Key | Meaning |
| --- | --- |
| APK `cls.method` | Garmin Connect 5.31 (11761), names as produced by jadx 1.5.3 `--deobf`. See `PROVENANCE.md`. |
| CAP-1 | HCI snoop capture of 2026-11-19 09:41: Bluetooth restart, reconnect, one manual sync and one app-started sync. Raw file kept private; SHA-256 `34d283c5…79e49ae`. Decoded by `tools/decode`. |
| CAP-2 | HCI snoop capture continuing to 2026-11-19 10:02: Connect and Explore reconnect on new characteristic pairs (about t = 700 s), then a sync after a recorded test activity (activity 128/4, monitoring 128/32, metrics 128/44, 128/58). Raw file kept private; SHA-256 `fc919d19…91417e7f`. |
| CAP-3 | HCI snoop capture continuing to 2026-11-19 10:13: the owner changed the watch's time format in Connect, plus a watch weather request. Raw file kept private; SHA-256 `7335e27a…3e77da54`. |
| GB `path` | Gadgetbridge at `7257742b`. Since stage 2p also a source of its own: *inferred (Gadgetbridge, file)* marks a fact that only its code supports (§9). |

Labels: **seen** means observed in code or capture. **inferred** means reasoned from surrounding evidence. **unknown** means each has an entry in `OPEN-QUESTIONS.md`.

## 1. Transport

### 1.1 GATT

| Item | Value | Label | Evidence |
| --- | --- | --- | --- |
| Service | `6A4E2800-…` | seen | APK `eit.f71464a`; CAP-1 |
| Info characteristic | `6A4E2803-…`: info pages by read, select a page by write | seen | APK `eit.f71465b`, `p16` |
| Data characteristics | notify `6A4E2810`–`2819`; write = the same UUID with the first `1` replaced by `2` (`2810` → `2820`) | seen | APK `vit.initialize`, `eb00.m39647a` |
| Choice | the app picks a random notify char from those present. A register status 3 means another client already holds it, and its 16-bit field names the characteristic to retry on. | seen | APK `vit`, `aww` |
| In CAP-1 | Connect: notify `0x0018` / write `0x001b`. Garmin Explore: `0x002c` / `0x002f`. Same LE link. | seen | CAP-1 |
| Reconnect | after a reconnect, Connect took a different pair (`0x001d` / `0x0020`) and reused client id 1. Handles are kept per characteristic pair, so a decoder or client must key its state by (pair, client id). | seen | CAP-2 |
| Write type | write without response (ATT `0x52`); watch → phone uses notifications (`0x1B`) | seen | CAP-1 |
| MTU | the app requests 515 (`a94`); the phone's stack sent 517 in CAP-1; the watch answers **23** | seen | APK `a94`; CAP-1 |
| Payload per packet | MTU − 3 − 1 (plain handle) or MTU − 3 − 2 (MLR), so 19 or 18 bytes on this watch | seen | APK `cit.m9423f`, `v84.onMtuChanged`; CAP-1 |
| Older transports | a single-channel V1 service (`6A4E2401`) and a legacy `9B012401-…` service exist in the app's tables. The Instinct 2 does not use them. | seen (tables), inferred (unused) | APK `cc4`; CAP-1 |

### 1.2 Multi-link control channel (handle 0)

Every characteristic value starts with a handle byte, and handle 0 is control. All fields are little-endian, and `client_id` is a u64 chosen by the app. In CAP-1, Connect uses client id 1 and Explore uses a random value. [seen: APK `ms6`; CAP-1]

| Type | Direction | Layout after `00 <type>` | Notes |
| --- | --- | --- | --- |
| 0 register | P→W | `u64 client, u16 service, u8 mode` | mode 0 = plain, 2 = request reliable (MLR) |
| 1 register response | W→P | `u64 client, u16 service, u8 status` + status 0: `u8 handle, u8 flags, u8 extra` / status 3: `u16 char` | status: 0 SUCCESS, 1 INVALID_SERVICE_ID, 2 PENDING_AUTH (the app keeps waiting), 3 ALREADY_IN_USE (+ characteristic to retry on), 4 REJECTED. flags bit 0 = reliable granted, and then handle = `0x80 \| mlr_handle`. For REGISTRATION, `extra` is the registration-service version. |
| 2 close handle | P→W | `u64 client, u16 service, u8 handle` | |
| 3 close response | W→P | `u64 client, u16 service, u8 handle, u8 status` | 0 SUCCESS, 1 INVALID_HANDLE, 2 NO_CONNECTION |
| 4 invalid handle | W→P | `u64, u16, u8 handle` | the app drops that handle |
| 5 close all | P→W | `u64 client, u16 0` | |
| 6 close all response | W→P | `u64 client, u16, u8 status` | CAP-1 status = 1; meaning **unknown** |
| 0xFF unknown command | W→P | `u64, u16, u8 original_type` | |

### 1.3 Session bring-up (seen in CAP-1, matches APK `vit`/`eb00`/`sb00`)

1. Enable notifications on the notify char's CCCD.
2. Send close-all, then register service 4 (REGISTRATION) in mode 0. On the Instinct 2 the response gives a plain handle with registration version 1.
3. Read info pages over the registration handle: write `[handle, page]`, receive a notification `[handle, page, data…]`.

   | Page | Contents |
   | --- | --- |
   | 0 | supported service bitmap |
   | 1 | advertising data |
   | 2 | multi-link version; 2.2.1 on this watch |
   | 3 | product 3888, firmware 1722 and unit id (personal) |
   | 4 | identity address (personal) |
   | 5 | per-service revision; unsupported here, the watch answers `FF 05` |

4. Register GFDI (1) in mode 2. The watch grants reliable mode (Connect: handle `0x82`). The app also registers KEEP_ALIVE (22) in mode 0, and NFC (2) reliable.
5. While the Connect UI is open, it registers real-time services: HR 6, steps 7, floors 9, intensity 10, body-battery 20 and training-readiness 25. These stream small records, for example HR `03 48 36 ff ff`. [seen: CAP-1; ids from APK `mit`, `mwz`]

Service ids: GFDI 1, NFC 2, HEALTH_SDK 3, REGISTRATION 4, CONNEXT 5, real-time 6–21, 25, 26, 29, AUTH_STATUS 14, ECHO 15, KEEP_ALIVE 22. [seen: APK `mit`, `mwz`]

### 1.4 Reliable mode (MLR)

Every packet on a reliable handle starts with a 2-byte **big-endian** header instead of the handle byte:

| Bits | Field |
| --- | --- |
| 15 | always 1 (plain handles are < 0x80) |
| 14–12 | MLR handle (3 bits) |
| 11–6 | ack: next sequence number expected from the peer (mod 64) |
| 5–0 | seq: this packet's sequence number (mod 64) |

[seen: CAP-1, for example watch `f0 00`…`f0 03` followed by phone ack `f1 00` (ack = 4); APK `cit.m9423f`; native `libreliable-ml.so` (`mlr_format_*`), checked against the library's own built-in header test vectors]

The behaviour below was recovered statically from the native library. Symbols are exported by name: `MLR_reliable_*`, `mlr_format_*`, `mlr_os_*`. Where possible it was cross-checked with CAP-1.

| Aspect | Behaviour | Label |
| --- | --- | --- |
| Packet types | two only: DATA (header + payload, always carries the current ack) and ACK-only (2 bytes, seq field 0). There is no NAK, reset, configure or handshake packet at this layer, and no length or CRC. | seen |
| Start state | both sides start at seq 0 / ack 0 when the handle is registered | seen; CAP-1 consistent |
| Payload | (ATT_MTU − 3) − 2 bytes, fixed when the connection opens; a packet never spans two blobs. 18 bytes on the Instinct 2 link. | seen; CAP-1 |
| Sequence space | 64 (6 bits) | seen |
| Send window | starts at 32, grows by 1 per RTT sample up to 63, halves on each timeout (minimum 1) | seen |
| Receiving | go-back-N: only the next expected seq is accepted. Out-of-order and duplicate packets are dropped without buffering. | seen |
| Retransmission | on timeout, resend from the oldest unacknowledged packet, rebuilt with the *current* ack | seen |
| ACK policy | after 5 received data packets, or 10 ms after the first unacknowledged one; any outgoing data packet carries the ack instead | seen |
| Timeout | Jacobson/Karels estimate (α 1/8, β 1/4, Karn's rule). Starts at 2000 ms, never below 2000 ms, doubles on each timeout up to 20000 ms. A duplicate ACK-only packet restarts the timer; there is no fast retransmit. On Android it retries every 20 s indefinitely. | seen |
| Handle check | a reliable handle without bit 7 is rejected; the watch returns `0x80 \| h` (CAP-1: 0x82, 0x83, 0x87) | seen |
| App driving model | one packet at a time through Java `onSendDataBlobStart`; the next packet waits for `mlrReadyToSend` after each BLE write (one token shared by all handles). Timers are Java `Timer`s that call `mlrExecuteTimer`. | seen |

Compared with Gadgetbridge:

- **Confirmed:** header layout, 64 sequence numbers, MTU − 2 payload, ACK after 5 packets, timeout doubling to 20 s, window halved on timeout, resend everything unacknowledged.
- **Refuted:** the ACK delay is 10 ms, not 20 ms; the first timeout is 2 s and adaptive, not 1 s; resent packets carry the current ack, not the original one.
- **Corrected:** 32 is only the starting window.

MLR is optional for a client. Without the native library the app registers GFDI in mode 0 and runs plain multi-link (seen: `kit.mo8947r`). Whether the Instinct 2 accepts mode 0 is Q-MLR-3. Our client registers GFDI with mode 2 and, if the response grants no MLR, uses plain multi-link as Gadgetbridge does (§9.1, *inferred (Gadgetbridge)*).

## 2. Framing (GFDI)

| Item | Value | Label | Evidence |
| --- | --- | --- | --- |
| Byte stuffing | COBS; each message is sent as `00 cobs(msg) 00`. Blocks hold at most 254 data bytes (code `0xFF`). | seen | APK `en6.m40411a`; CAP-1 |
| Message | `u16 len` (whole message, including itself and the CRC), 2-byte id field, payload, `u16 crc` | seen | APK `vns.m72690a`, `h6p` case 15 |
| CRC | CRC-16/ARC: reflected poly 0x8005, init 0, nibble table `0000 CC01 D801 1400 F001 3C00 2800 E401 A001 6C00 7800 B401 5000 9C01 8801 4400`, low nibble first; covers everything before the CRC. This is the same CRC FIT files use. | seen | APK `u45.m70310a`, `um9`; CAP-1 (all CRCs valid) |
| Id field | plain `u16 id`, or compact `[id − 5000, 0x80 \| txn]` with txn = low 5 bits | seen | APK `vns.m72690a`, `h6p` case 15; CAP-1 (txn 0–31, both directions) |
| Compact ids | used once both sides report protocol ≥ 150. The Instinct 2 reports 150. | seen | APK `zxl` (`PROTOCOL_VERSION_WITH_TRANSACTION_IDS`); CAP-1 |
| Max message | from DeviceInformation `max_packet` (4000 on this watch); the app derives its payload limit in `vid.mo38809d` | seen | APK; CAP-1 |
| Size check | a reader rejects a message whose length field ≠ frame length, or that is shorter than 6 bytes | seen | APK `h6p` |

### 2.1 Response (5000)

Payload: `u16 request_id, u8 status, body…`. Status values: 0 ACK, 1 NAK, 2 UNKNOWN_OR_NOT_SUPPORTED, 3 COBS_DECODER_ERROR, 4 CRC_ERROR, 5 LENGTH_ERROR. [seen: APK `ly00`, `gu2`; CAP-1]

A request with no registered handler gets an automatic reply (APK `vid` → `xid`, inferred status 2). Gadgetbridge sends nothing in that case.

A response to a request sent with a txn reuses that txn (CAP-1). A response to a protobuf message carries an 8-byte body. Gadgetbridge reads it as `u16 request_id, u32 data_offset, u8 chunk_status, u8 status_code` (inferred). In CAP-1 the last two bytes are almost always `00 00`; once they were `01 c9`, which is **unknown**; Gadgetbridge's names for the two bytes read it as "discarded, status 201" (§9.2, inferred).

## 3. Session start (GFDI)

Seen in CAP-1 in this order, and matching APK `zxl`, `ayl`, `smg`, `vxk`, `r6a`:

1. **W→P 5024 DeviceInformation:** `u16 protocol (150), u16 product (3888), u32 unit_id, u16 software (1722), u16 max_packet (4000), str bt_name, str device_name, str model_name` (each string is a u8 length plus UTF-8), followed by 2 more bytes. The app parses those as an optional `u8 dual_pairing` and an optional `u8 limitation`.
2. **P→W 5000 ACK(5024)** with the host's details: `u16 protocol (150), u16 0xFFFF, u32 0xFFFFFFFF, u16 app_version (Connect: 11761; Explore: 38215), u16 0xFFFF, str friendly_name, str manufacturer, str model, u8 1`.
3. **W→P 5050 Configuration:** `u8 n` then an n-byte capability bitmask (Instinct 2 bits: 0, 3–20, 26, 29, 33, 36–39, 42–44, 46, 47, 49, 51, 52, 54, 57–59, 65, 67–74, 76, 77, 81, 82, 84, 85, 88, 91, 92, 94–96). The phone ACKs it, then sends its own 5050.
4. **P→W 5043 protobuf `Smart.core(13)`** FeatureCapabilitiesRequest. It is sent because the watch has capability bit 95. The watch answers with a FeatureCapabilitiesResponse in 5044. [seen: CAP-1, APK `vxk`]
5. **P→W 5031 Supported file types** (see §5.6), **5026 SetDeviceSettings** `03 | 06 01 01 | 07 01 01 | 08 01 00` (three TLV settings, ids 6, 7, 8), and **5030 SystemEvent HANDSHAKE_COMPLETE (8)**.
6. **W→P 5052 CurrentTime** request `u32 ref`. The phone replies `u32 ref, u32 garmin_time, s32 utc_offset_s, u32 dst_start, u32 dst_end`. Time is seconds since 1989-12-31T00:00Z. `dst_start` is the next transition **into** daylight saving time and `dst_end` the next transition **out of** it, so in summer time `dst_start` is the later of the two. Both are 0 when the zone has no further transition. In CAP-1 (winter time) the fields were 1175130000 and 1193878800, which are Garmin-epoch times in 2027. [seen: CAP-1; APK `r6a.onMessageReceived` writes the pair returned by `r6a.m65223a`]

   How the app computes the pair (seen: APK `r6a.m65223a`): take the zone's next transition `t1` after now. If there is none, send `0, 0`. If the zone is in daylight time at `t1`, then `dst_start = t1` and `dst_end` = the next transition after `t1 + 1 day`; otherwise `dst_end = t1` and `dst_start` = the next transition after `t1 + 1 day`. The same pair fills 5026 settings 4 and 5 on watches without capability 71.
7. The watch then sends a series of protobuf requests: notification subscriptions (5036), music capabilities (5042), and others.

The app does **not** answer a 5101 Auth Negotiation on a bonded link. It treats it as an error and resets the link. No 5101 appears in CAP-1. [seen: APK `rq50`; CAP-1]

A FIT `capabilities` message (global 1) from the watch triggers a FIT `connectivity` message (global 127) in reply, sent over 5011/5012. [seen: APK `zxl`, `smg.m67689c`] This was **not** observed in CAP-1, which was a reconnect, not a first pairing.

## 4. Message catalogue

See `messages.csv`: all ids with layouts, labels, evidence and the Gadgetbridge comparison. Ids observed in CAP-1: 5000, 5002, 5003, 5004, 5005, 5007, 5008, 5024, 5026, 5030, 5031, 5033, 5034, 5035, 5036, 5037, 5042, 5043, 5044, 5049, 5050, 5052.

## 5. File sync

### 5.1 Who starts it

The watch sends **5037 SyncRequest** `u8 option (0 MANUAL, 1 INVISIBLE, 2 VISIBLE_AS_NEEDED), u8 n, n-byte bitmask`.

In CAP-1 the app sent 5030 HOST_DID_ENTER_FOREGROUND, and the watch answered with option 2. Its bitmask had bits 0–4, 6, 9, 11, 12, 14, 17–23, 25–27, 30, 31 and 36.

The app's "sync now" goes through protobuf `Smart.core(13)` SyncRequest. The watch then sends 5037 with option 0 (MANUAL). [seen: CAP-1 t=62.6 s; APK `p860`]

Bit meanings come from Gadgetbridge (inferred). For example 5 = activities, 26 = sleep and 36 = HRV status. The full list of names is in §9.4.

### 5.2 Listing

1. **P→W 5007** filter `3` (PENDING_UPLOADS_ONLY). The reply body is `u8 0` (supported).
2. **P→W 5002** for file index 0, the directory.

The directory is a 16-byte header followed by 16-byte entries, little-endian:

| Bytes | Field |
| --- | --- |
| 0–1 | u16 index |
| 2 | u8 data_type |
| 3 | u8 sub_type |
| 4–5 | u16 file_number |
| 6 | u8 specific_flags |
| 7 | u8 flags |
| 8–11 | u32 size |
| 12–15 | u32 timestamp (Garmin epoch) |

flags: READ 0x80, WRITE 0x40, ERASE 0x20, ARCHIVE 0x10, APPEND 0x08, CRYPTO 0x04. Pending files in CAP-1 had flags 0xA0 (READ | ERASE).

[seen: APK `C8648a` ~l.440–490, `EnumC8652e`; CAP-1: two entries, 128/32 and 128/58]

### 5.3 Download

- **P→W 5002:** `u16 index, u32 offset (0), u8 type (1 = new), u16 crc_seed (0), u32 max_size (0 = whole file), u8 flags`. The app sends flags `0x01`, which means compressed data is allowed (inferred). No compressed (5054) data appeared in CAP-1.
- **Reply body:** `u8 status, u32 size`. Status values: 0 OK, 1 no such index, 2 not readable, 3 not ready (retry after 1 s), 4 invalid request, 5 CRC mismatch, 6 beyond file size. [seen: APK `wtz`]
- **W→P 5004 FileData:** `u8 flags, u16 crc, u32 offset, data`. The crc is CRC-16/ARC over **all file bytes so far**. The Instinct 2 sent each file in **one** 5004 when it fits under `max_packet`; the 5157-byte activity in CAP-2 came in several chunks. When a FIT file is complete, the crc is 0x0000, because a FIT file ends with its own CRC.
- **P→W ACK(5004):** `u8 status, u32 bytes_received`. Status 0 = ok (also for a duplicate), 3 = CRC mismatch (abort), 4 = offset mismatch (resend), 2 = abort.

The app never resumes at an offset other than 0, although the fields would allow it. [seen: APK `yw40`; CAP-1]

### 5.4 Mark as synced

**P→W 5008** `u16 index, u8 0x10` (ARCHIVE). The reply body is `u8 0, u16 x, u8 0x10`. In CAP-1 `x` was the index minus 1 every time; its meaning is **unknown**.

The official app archives only **after Garmin's server has accepted the upload** (APK `a7d` "DeviceSyncUpload"). In CAP-1, archiving followed each download by about 1.7 s, which is consistent with a quick upload. Our client archives once the file is stored safely on the phone.

### 5.5 After the downloads (seen in CAP-1)

1. **P→W 5005 CreateFile + 5003 UploadFile + 5004** send a 1372-byte file of type 255/19 to the watch, at index 0xFFFB. Its content is **unknown**: possibly GPS assist data or settings. See `OPEN-QUESTIONS.md`. Gadgetbridge has no 255/19 type and never sends such a file (§9.4); its upload message layouts are in §9.5.
2. **P→W 5030 SYNC_COMPLETE (0).**
3. **W→P 5037 INVISIBLE [bit 8].** The watch then makes HTTP requests over protobuf: `Smart.connect_iq_http(2)` with an opaque body in field 11. The app answers with Garmin server JSON (stress score, sleep, LiveTrack settings) as an HTTP 200 inside `Smart(2)`. A second listing follows.

### 5.6 File types (Instinct 2, firmware 17.22)

The 5031 reply in CAP-1 lists FIT sub-types 4, 32, 38, 41, 44, 49, 58, 68, 70, 72 and 74, plus the non-FIT types ErrorShutdownReports (255/245), ULFLogs (255/247) and KPI (255/248).

A normal sync downloads the pending files among these. The app limits itself to the FIT sub-types the watch advertised. Details are in `filetypes.csv`.

## 6. Companion duties

| Duty | How the watch asks | App behaviour | If unanswered | Label |
| --- | --- | --- | --- | --- |
| Time and time zone | 5052 after HANDSHAKE_COMPLETE or TIME_UPDATED. Without capability 71 the app pushes 5026 TLV ids 1–5 instead. | reply as in §3 | APK: the app logs "Timed out waiting for device to request time". Watch behaviour **unknown**; it keeps its own clock (inferred). | seen |
| GPS assist | (a) upload of a 255/19 file after sync (inferred); (b) HTTP over protobuf to `api.gcs.garmin.com/ephemeris/cpe/...` (APK `GcsEphemerisApi`; Gadgetbridge) | fetched from Garmin with an account token | slower GPS fix (inferred) | inferred |
| Weather | 5014 or HTTP over protobuf `/weather/...` (APK URLs `api.gcs.garmin.com/weather`) | Garmin weather service | no weather on the watch | inferred |
| Notifications | 5036 subscription, 5034 control point, 5033 source and 5035 data from the phone | ANCS-like attributes | no notifications | seen (CAP-1) |
| Cloud widgets | HTTP over protobuf `Smart(2)` after sync | proxied to Garmin | stale or missing cloud-derived widgets | seen (CAP-1) |
| Find my phone | 5039 / 5040 | ring | — | inferred |
| Keep-alive | multi-link service 22, watch → phone counter `00 nn` about every 25 s | none needed | unknown | seen (CAP-1) |

The time duty is the only one our client needs on day one. The others are optional, and each becomes a setting.

## 6a. Watch settings (seen in CAP-3)

Changing a watch setting in Connect uses protobuf `Smart.realtime_settings (42)`, not a FIT settings file. Message names come from the recovered `gdi_realtime_settings.proto`.

1. `start_up_data_request (8)` with locale strings (`"en_US"`, a country code). The response (9) carries part-number strings.
2. `settings_request (3) {1: screen_id}`. The response (4) is the screen's tree of entries and option ids.
3. `composition_request (1) {1: screen_id, 3: locale}`. The response (2) carries display strings, for example "Time" and "Time Format" for screen 34.
4. `settings_change_request (5) {1: 34, 2: 0, 4: {1: new_value}}`. The response (6) has `{1: 0}`, which is success.

Our web interface can offer the same settings by walking this tree. The settings themselves stay on the watch; we store nothing. [seen: CAP-3 t ≈ 1775–1790 s]

## 6b. Weather (seen in CAP-3)

The watch sends **W→P 5014 WeatherRequest** with a 10-byte payload. The phone ACKs it and then pushes FIT weather messages: **5011** definitions and **5012** data, containing current conditions, hourly forecast and a location name. The 5014 payload layout is in `messages.csv` (from Gadgetbridge, inferred); the FIT messages are the standard `weather_conditions` messages.

## 7. Protobuf

All 201 generated schemas were recovered from the app into `proto/`. `Smart` is empty and binds each service as an extension. The services that matter: `connect_iq_http` 2, `data_transfer` 7, `device_status` 8, `core` 13, `realtime_settings` 42, `file_access` 43 (item-based "new sync") and `gncs` 49. The full map is in the `gdi_smart.proto` header. A 5043/5044 payload is `u16 request_id, u32 offset, u32 total, u32 chunk_len` followed by Smart bytes; that header is inferred from Gadgetbridge and matches every message in CAP-1. The Instinct 2 used legacy GFDI file sync, not FileAccess, in CAP-1.

## 8. Hook points

See `HOOKS.md`.

## 9. Gadgetbridge comparison (stage 2p)

Gadgetbridge at `7257742b` was read file by file and compared with the sections above. Everything here that comes only from Gadgetbridge is labelled *inferred (Gadgetbridge, file)*: it is what that code does or assumes, on the devices its authors tested, and none of it has been seen in our captures unless the row says so. File names are relative to `service/devices/garmin/` in Gadgetbridge unless they start with `devices/`. Where Gadgetbridge and a capture disagree, **the capture wins** and our client follows the capture.

### 9.1 Transport

| Topic | Gadgetbridge | Ours (capture / app) | Verdict |
| --- | --- | --- | --- |
| Device recognition | Advertised manufacturer data for Bluetooth company id 0x0087 (Garmin): the first two bytes are the product number, big-endian, looked up in a product table. If there is no such data or the number is unknown, the BLE name must match the model's pattern as a whole (`^Instinct 2$`). *inferred (Gadgetbridge, `devices/garmin/GarminCoordinator`, `GarminProductNumbers`)* | Product 3888 arrives in info page 3 and in 5024 (seen). The advertising payload was not looked at in CAP-1..3. | Gadgetbridge-only. Check the advertising bytes of the Instinct 2 in 2c. |
| Characteristic pair | Takes the first pair present among `2810/2820` … `2814/2824`; never looks at `2815`–`2819`. *inferred (Gadgetbridge, `communicator/v2/CommunicatorV2`)* | The app picks at random among `2810`–`2819` and retries on status 3 (seen: APK). | No conflict; ours follows the app. A fixed choice of the first pair would collide with Connect more often. |
| Client id | A fixed u64 `2`, and it ignores every control message whose client id is not `2`. *inferred (Gadgetbridge, `CommunicatorV2`)* | Connect uses 1, Explore a random value (seen: CAP-1). | Gadgetbridge-only fact: a client id other than Connect's works on the watches it supports. Annotates Q-ML-2; still to test with Connect connected at the same time. |
| Bring-up | close-all → on the close-all response, register GFDI. It never registers REGISTRATION (4) or KEEP_ALIVE (22) and never reads the info pages. Any close-all status is accepted. *inferred (Gadgetbridge, `CommunicatorV2`)* | The app registers REGISTRATION, reads pages 0–5, registers GFDI and KEEP_ALIVE (seen: CAP-1). | No conflict. Gadgetbridge shows the shorter sequence is enough on its devices (*inferred*); ours keeps the app's sequence. |
| Register response | Reads `status`, and for status 0 `handle` and one byte it treats as "reliable if non-zero". Other statuses are logged and dropped; no retry on status 3, no waiting on status 2. *inferred (Gadgetbridge, `CommunicatorV2`)* | Status 2 = keep waiting, 3 = retry on the named characteristic (seen: APK). | Capture/APK wins. |
| Plain mode (no MLR) | Supported and was the only mode for years: GFDI registered with mode 0, each write is `[handle] + up to (MTU − 4) bytes` of the COBS stream. MLR is a per-device preference, on by default; a comment says large files fail to sync without MLR on new-sync devices. The register response decides per registration: any non-zero reliable byte = MLR, else plain on the handle byte as returned. Outgoing: the COBS stream is cut into `[handle, ≤ max_write − 1 bytes]` values with `max_write = min(512, MTU − 3)` (so 19 data bytes at MTU 23), all queued at once with no windowing. Incoming: the value is routed to MLR if bit 7 is set *and* an MLR endpoint exists for that handle; otherwise the first byte is looked up as a plain handle (a code comment, issue #5476, says plain handles can have bit 7 set) and the rest feeds the COBS decoder. *inferred (Gadgetbridge, `CommunicatorV2.sendMessage`, `onCharacteristicChanged`, `processHandleManagement`; `devices/garmin/GarminCoordinator`)* | Never seen; the app only falls back to it without its native library. | Annotates Q-MLR-3: a mode 0 registration very probably works. **Built in plan row 8:** we still ask for mode 2 and, if the register response has no reliable flag (bit 0, as in the capture), carry on in plain mode with the layout described here instead of closing. Not run against a real watch. |
| GFDI handle closed by the watch | Type 3 (close-handle response) with service 1, whatever its status and handle, outside a close-all (a flag is set from the close-all request until its response, and that response registers GFDI itself): it drops the handle, the MLR endpoint and the service callback, then writes a register request for GFDI, mode 2 if its MLR preference is on, else 0. The register response builds a new callback, so the COBS decoder starts empty; a new MLR endpoint starts at sequence 0. Nothing above `CommunicatorV2` is told (the GFDI callback's close hook is empty): the file-transfer state and the handshake state of `GarminSupport` are not reset, and frames sent while no handle exists are logged and dropped. Closes of other services only drop their mappings. *inferred (Gadgetbridge, `CommunicatorV2.processHandleManagement`, case `CLOSE_HANDLE_RESP`)* | Not seen. | Gadgetbridge-only. **Built in plan row 8, transport side only:** in the up state, a close response (our client id) for service 1 drops the endpoint or plain handle, resets the COBS decoder, writes the register request (mode 2) and reports the anomaly `GfdiClosedByWatch`; the answer is handled like the bring-up one (MLR or plain, status 2 waits, other failures close, 30 s deadline). Ours, on the session side: the reopen only happens while the session is idle (handshake done, no request outstanding or queued, no sync cycle); otherwise the answer in flight is lost and the client closes at once with `CloseReason::GfdiClosed { phase }` so the caller reconnects. Frames the session sends during the reopen are held and written on the new handle (Gadgetbridge drops them). A 5024 arriving on an idle session restarts the handshake (Gadgetbridge answers each 5050 statelessly, so it would too). Open: whether a real watch re-sends 5024 after this, and whether type 4 INVALID_HANDLE (§1.2, the app drops the handle) is the real watch-side signal instead; today type 4 stays `BadTransport`. Not run against a real watch. |
| Control types | Names types 0–6 as we do, calls 4 "unknown handle", and lists 7 and 8 as an unknown request/response pair. *inferred (Gadgetbridge, `CommunicatorV2`)* | Type 4 = invalid handle, `0xFF` = unknown command (seen: APK). | Types 7 and 8 are Gadgetbridge-only and unexplained; never seen. |
| Service ids | Adds REALTIME_CALORIES 8, REALTIME_HRV 12, REALTIME_STRESS 13, REALTIME_ACCELEROMETER 16, REALTIME_SPO2 19, REALTIME_RESPIRATION 21, REALTIME_ACTIVE_TIME 26, and six file-transfer services `0x2018`, `0x4018`, `0x6018`, `0xA018`, `0xC018`, `0xE018` used by the new sync for bulk data. *inferred (Gadgetbridge, `CommunicatorV2`)* | We have the ids from APK `mit`/`mwz` with fewer names; the `0x..18` services are new to us. | Gadgetbridge-only names. `core::multilink::service_name` now names the six file-transfer ids `FILE_TRANSFER_2` … `FILE_TRANSFER_E` after Gadgetbridge; the other seven ids were already named there (`REAL_TIME_*`, from the app's tables). |
| Real-time payloads | HR: `u8 type, u8 bpm, u8 resting` (CAP-1's `03 48 36 …` reads as type 3, 72 bpm, resting 54). Steps: `u32 steps, u32 goal`. SpO2: `s8 percent (−1 = none), u32 Garmin time`. Respiration: `s8 breaths/min` (negative = none). HRV: `u16 RR interval ms, u32 unknown`. Accelerometer: write `01` to start; 16-byte packets, a u16 whose low 13 bits are a timestamp, then three samples of three 12-bit signed axes, 256 counts per g. *inferred (Gadgetbridge, `CommunicatorV2`, `communicator/v2/Realtime*Callback.kt`)* | CAP-1 has the HR bytes only. | Agrees with the one sample we have. |
| MLR | As §1.4 ("Compared with Gadgetbridge"). In addition: on an out-of-order packet it sends an ACK-only packet at once; its window never grows above 32. *inferred (Gadgetbridge, `communicator/v2/MlrCommunicator`)* | Native library: the ACK follows the normal policy; window grows to 63 (seen). | Native library wins. |
| Older transports | V1 service `6A4E2401-…` with characteristics `6A4E4C80-…` (write) and `6A4ECD28-…` (notify); V0 service `9B012401-BC30-CE9A-E111-0F67E491ABDE` with `DF334C80-E6A7-D082-274D-78FC66F85E16` and `4ACBCD28-7425-868E-F447-915C8F00D0CB`. One GFDI/COBS stream, no handle byte. *inferred (Gadgetbridge, `communicator/v1/CommunicatorV1`)* | UUIDs match the app's tables (§1.1); unused by the Instinct 2. | Agrees. |
| Classic Bluetooth | Some devices list no FIT files over BLE; Gadgetbridge then opens an RFCOMM link to the classic address found in 5024 and runs a second GFDI handshake there. *inferred (Gadgetbridge, `GarminRfcommListener`, `GarminSupport`)* | Never seen. | Gadgetbridge-only; not relevant to the Instinct 2 (its 5024 carries no address). |
| Connect IQ app link | Separate GATT service `6A4E8022-…` reusing the V1 characteristic UUIDs; messages are XXTEA-encrypted with a per-transfer 16-byte key exchanged in clear. *inferred (Gadgetbridge, `communicator/GarminWatchAppCommunicator`)* | Not analysed. | Gadgetbridge-only; out of scope. |

### 9.2 GFDI framing and replies

| Topic | Gadgetbridge | Ours (capture / app) | Verdict |
| --- | --- | --- | --- |
| Compact ids | Incoming: if bit 15 of the id field is set, id = low byte + 5000; the transaction number is thrown away. Outgoing: always the plain 16-bit id, and a reply never repeats the request's transaction number. *inferred (Gadgetbridge, `messages/GFDIMessage`)* | Both sides use compact ids with a 5-bit txn, and a reply reuses the request's txn (seen: CAP-1). | **Disagreement; capture wins.** (The summary's earlier "Gadgetbridge assumes 7 bits" was wrong: it does not read the txn at all.) Gadgetbridge shows that watches tolerate plain ids and replies without txn (*inferred*). |
| Unhandled request | No reply at all, for ids it does not know and for ids it knows but does not parse. *inferred (Gadgetbridge, `messages/UnhandledMessage`, `messages/status/GenericStatusMessage`)* | The app auto-replies (inferred status 2). | Unchanged: ours copies the app. Gadgetbridge's silence being tolerated by its devices annotates Q-GFDI-5. |
| Status 2 on the wire | With time sync switched off, it answers 5052 with a bare 5000 `status 2`. *inferred (Gadgetbridge, `GarminSupport.onMessage`)* | Status 2 never seen on the wire. | Gadgetbridge-only: the one place where it sends "unsupported"; supports our choice for Q-GFDI-5. |
| Reply order | For each incoming message: status reply first, then a message-specific reply, then a follow-up request. *inferred (Gadgetbridge, `GarminSupport.onMessage`)* | Same order in CAP-1. | Agrees. |
| Protobuf ACK body | `u16 request_id, u32 data_offset, u8 chunk_status, u8 status_code`; chunk status 0 = kept, 1 = discarded; status code 0 = no error, 100 = unknown request id, 101 = duplicate packet, 102 = missing packet, 103 = exceeded total length, 200 = parse error, 201 = unknown. *inferred (Gadgetbridge, `messages/status/ProtobufStatusMessage`)* | Body is 8 bytes, last two almost always `00 00`, once `01 c9` (seen: CAP-1). | Gadgetbridge-only names. They read the `01 c9` as "discarded, 201 unknown": the app rejected that watch message. Annotates Q-GFDI-3. |
| ACK of a complete protobuf message | A bare ACK without the 8-byte body when the message is a single chunk it did not handle specially; the body only for chunks, handled services and rejections (where it uses code 100, not 201). *inferred (Gadgetbridge, `messages/ProtobufMessage`, `ProtocolBufferHandler`)* | The app always sends the 8-byte body (seen: CAP-1). | **Disagreement; capture wins.** |
| Protobuf chunking | A message longer than `max_packet − 20` is cut into chunks of that size; the next chunk is sent when the status for the previous one arrives; incoming chunks are joined by request id and offset. *inferred (Gadgetbridge, `ProtocolBufferHandler`)* | No chunked message in CAP-1..3. | Gadgetbridge-only; port in 2f (plan row 27). |
| Message ids we had no name for | 5015 weather alert, 5016 LiveTrack tracking request, 5025 LiveTrack stop, 5045 cancel protobuf, 5046–5048 LiveTrack auto start / cancel / failure, 5055–5056 Vector pedal and sensor pairing, 5060–5066 children's-device messages (coin, chore, timer, active time, graphics, game level), 5102–5112 Garmin Auth steps (LTK reconnect, STK generation, confirm number, key distribution, session key, passkey, secure session, out-of-band passkey). *inferred (Gadgetbridge, `messages/GFDIMessage`)* | Names for 5101–5112 partly known from the APK. | Gadgetbridge-only names; added to `messages.csv` when that file is next edited. |

### 9.3 Session start

| Topic | Gadgetbridge | Ours (capture / app) | Verdict |
| --- | --- | --- | --- |
| 5024 tail | After the three strings: `u8 flag`; if the flag is 1, a 6-byte BLE address and a 6-byte classic Bluetooth address follow (each stored low byte first); then one more byte of unknown meaning. *inferred (Gadgetbridge, `messages/DeviceInformationMessage`)* | Two bytes follow on the Instinct 2; the app reads them as optional "dual pairing" and "limitation" (seen). | Agrees and explains the first byte: "dual pairing" = the device also has a classic address and sends both. Annotates Q-GFDI-1. |
| Reply to 5024 | Sends **two** 5000 messages: a bare ACK, then an ACK carrying the host details. Host details: protocol 150, product `0xFFFF`, unit `0xFFFFFFFF`, software version 7791, max packet `0xFFFF`, Bluetooth adapter name, phone manufacturer, phone device name, then `u8 1` when the watch's protocol version is 100–199, else 0. *inferred (Gadgetbridge, `DeviceInformationMessage`, `GarminSupport.onMessage`)* | One ACK with host details, same layout, last byte 1 (seen: CAP-1). | **Disagreement on the extra bare ACK; capture wins.** The layout agrees. The last byte is a protocol flag tied to protocol 1xx (*inferred*). |
| 5050 capability names | 129 named bits (list in `devices/garmin/GarminCapability`). For the Instinct 2's mask (§3 step 3): 0 CONNECT_MOBILE_FIT_LINK, 3 SYNC, 4 DEVICE_INITIATES_SYNC, 5 HOST_INITIATED_SYNC_REQUESTS, 6 GNCS, 7 ADVANCED_MUSIC_CONTROLS, 8 FIND_MY_PHONE, 9 FIND_MY_WATCH, 10 CONNECTIQ_HTTP, 11 CONNECTIQ_SETTINGS, 12–15 CONNECTIQ app / widget / watch-face / data-field download, 16 CONNECTIQ_APP_MANAGEMENT, 17 COURSE_DOWNLOAD, 18 WORKOUT_DOWNLOAD, 19 GOLF_COURSE_DOWNLOAD, 20 DELTA_SOFTWARE_UPDATE_FILES, 26 WEATHER_CONDITIONS, 29 EXPLICIT_ARCHIVE, 33 TRUEUP, 36–39 AUDIO_PROMPT lap / pace-speed / heart-rate / power, 42 SPORT_GENERIC, 43 SPORT_RUNNING, 44 SPORT_CYCLING, 46 SPORT_FITNESS_EQUIPMENT, 47 SPORT_SWIMMING, 49 CALENDAR, 51 SMS_NOTIFICATIONS, 52 BASIC_MUSIC_CONTROLS, 54 DELTA_SOFTWARE_UPDATES, 57 HSA_SUPPORT, 58 SPORT_STRENGTH, 59 SPORT_CARDIO, 65 LOCATION_UPDATE, 67 DEVICE_DRIVEN_LIVETRACK_SUPPORT, 68 CUSTOM_CANNED_TEXT_LIST_SUPPORT, 69 EXPLORE_SYNC, 70 INCIDENT_DETECT_AND_ASSISTANCE, 71 CURRENT_TIME_REQUEST_SUPPORT, 72 CONTACTS_SUPPORT, 73 LAUNCH_REMOTE_CIQ_APP_SUPPORT, 74 DEVICE_MESSAGES, 76 MULTI_LINK_SERVICE, 77 OAUTH_CREDENTIALS, 81 EVENT_SHARING, 82 HSA_FEATURE_MESSAGE, 84 LOCATION_UPDATE_PROVIDER, 85 LIVE_TRACK_COURSE_SHARING, 88 HTTP_RAW_RESOURCE_REQUEST, 91 NAVIGATION_SYNC, 92 REALTIME_SETTINGS, 94 TOYSTORE_MOBILE, 95 FEATURE_CAPABILITIES_EXCHANGE, 96 DEEP_LINKING. *inferred (Gadgetbridge, `devices/garmin/GarminCapability`)* | Bits 71 and 95 were already named from the APK and agree. | Gadgetbridge-only names. Useful absences on the Instinct 2: 27 WEATHER_ALERTS, 28 GPS_EPHEMERIS_DOWNLOAD, 90 SYNC_2 (the new sync), 75 WAYPOINT_TRANSFER. |
| Our own 5050 | Sets every bit it names except 22, 23 (LiveTrack), 28 (GPS ephemeris) and 104–111, 114–119. *inferred (Gadgetbridge, `GarminCapability.OUR_CAPABILITIES`)* | We replay Explore's mask (contract §9). | Gadgetbridge-only: claiming nearly everything works on its devices, at the price of the watch asking for everything. |
| FeatureCapabilities (5043/5044) | Never sent. *inferred (Gadgetbridge: no reference to it in `service/devices/garmin/`)* | Connect sends it when the watch has bit 95 (seen: CAP-1). | No conflict; one more sign that it is optional. To confirm in 2c. |
| After 5050 | 5031, then 5026 with settings 6 = on, 7 = on, 8 = off, then 5030 event 16, then 5030 event 8, then protobuf requests for battery status and the installed Connect IQ apps. *inferred (Gadgetbridge, `GarminSupport.completeInitialization`)* | 5031, 5026 with `06 01 01`, `07 01 01`, `08 01 00`, 5030 event 8 (seen: CAP-1). | The 5026 bytes agree exactly. Event 16 is Gadgetbridge-only; the watch asked for the time without it in CAP-1. |
| 5026 setting ids | 0 DEVICE_NAME, 1 CURRENT_TIME, 2 DAYLIGHT_SAVINGS_TIME_OFFSET, 3 TIME_ZONE_OFFSET, 4 NEXT_DAYLIGHT_SAVINGS_START, 5 NEXT_DAYLIGHT_SAVINGS_END, 6 AUTO_UPLOAD_ENABLED, 7 WEATHER_CONDITIONS_ENABLED, 8 WEATHER_ALERTS_ENABLED. Value encoding: a string, or length 4 + s32, or length 1 + bool. *inferred (Gadgetbridge, `messages/SetDeviceSettingsMessage`)* | Ids 1–5 are the time settings (seen: APK), 6–8 seen in CAP-1 without names. | Agrees; annotates Q-GFDI-4. |
| 5030 event names | 0 SYNC_COMPLETE, 1 SYNC_FAIL, 2 FACTORY_RESET, 3 PAIR_START, 4 PAIR_COMPLETE, 5 PAIR_FAIL, 6 HOST_DID_ENTER_FOREGROUND, 7 HOST_DID_ENTER_BACKGROUND, 8 SYNC_READY, 9 NEW_DOWNLOAD_AVAILABLE, 10 DEVICE_SOFTWARE_UPDATE, 11 DEVICE_DISCONNECT, 12 TUTORIAL_COMPLETE, 13 SETUP_WIZARD_START, 14 SETUP_WIZARD_COMPLETE, 15 SETUP_WIZARD_SKIPPED, 16 TIME_UPDATED, 17 TRIGGER_REBOOT. The value after the event byte is one byte, or a length-prefixed string. *inferred (Gadgetbridge, `messages/SystemEventMessage`)* | 0, 6 and 8 seen; we call 8 HANDSHAKE_COMPLETE after the app's flow. | Numbers agree. **Name disagreement on 8** (SYNC_READY against HANDSHAKE_COMPLETE): same byte, we keep our name. |
| First connection | When the user pairs for the first time it additionally sends events 4 (PAIR_COMPLETE), 0 (SYNC_COMPLETE) and 14 (SETUP_WIZARD_COMPLETE), in that order, after event 8. PAIR_START and TUTORIAL_COMPLETE are commented out. *inferred (Gadgetbridge, `GarminSupport.completeInitialization`)* | Not captured (Q-PAIR-1). | Gadgetbridge-only; the best description of a fresh pairing we have. Annotates Q-PAIR-1. Our client sends the same three events, value 0, when `ClientConfig::first_connect` is set (port-list row 16). Ours: one at a time after the ACK of event 8, before any sync request, without Gadgetbridge's battery and installed-apps requests in between; a refusal is reported, not fatal. |
| 5052 reply | `u32 ref, u32 Garmin time, s32 utc offset, u32 A, u32 B` where A = the transition **after** the next one and B = the **next** transition; both 0 when the zone has no further transitions. *inferred (Gadgetbridge, `messages/CurrentTimeRequestMessage`)* | In CAP-1 (19 November) the two fields were 28 March 2027 and 31 October 2027: the earlier date first. | **Disagreement; capture and APK win.** The app's code (APK `r6a.m65223a`, seen) settles Q-TIME-2: the fields are (next DST start, next DST end), see §3 step 6. Gadgetbridge's fixed order equals that only while daylight saving time is in force; in winter time it sends the two swapped. `0, 0` for a zone without transitions is what the app sends too (seen: APK), which answers stop rule 4 of the 2a contract. |
| 5101 | Payload `u8 unknown, u32 flags`; it answers with ACK + `u8 0` ("guess ok") + the unknown byte + the flags, all echoed. It implements none of 5102–5112. *inferred (Gadgetbridge, `messages/AuthNegotiationMessage`, `messages/status/AuthNegotiationStatusMessage`)* | The app treats 5101 on a bonded link as an error (seen: APK). | Unchanged disagreement; ours follows the app. The layout is Gadgetbridge-only. |

### 9.4 File sync

| Topic | Gadgetbridge | Ours (capture / app) | Verdict |
| --- | --- | --- | --- |
| 5037 layout | `u8 type, u8 size, bitmask` with size 4 or 8 only; other sizes are rejected. Types: 0 MANUAL, 1 AUTOMATIC, 2 unnamed. *inferred (Gadgetbridge, `messages/SynchronizationMessage`)* | 0 MANUAL, 1 INVISIBLE, 2 VISIBLE_AS_NEEDED (seen: APK). | Layout agrees; the app's names are better. |
| 5037 bit names | 0 SCHEDULES, 1 SETTINGS, 2 GOALS, 3 WORKOUTS, 4 COURSES, 5 ACTIVITIES, 6 PERSONAL_RECORDS, 8 SOFTWARE_UPDATE, 9 DEVICE_SETTINGS, 10 LANGUAGE_SETTINGS, 11 USER_PROFILE, 12 SPORTS, 13 SEGMENT_LEADERS, 14 GOLF_CLUB, 15 WELLNESS_DEVICE_INFO, 16 WELLNESS_DEVICE_CCF, 17 INSTALL_APP, 18 CHECK_BACK, 19 TRUE_UP, 20 SETTINGS_CHANGE, 21 ACTIVITY_SUMMARY, 22 METRICS_FILE, 23 PACE_BAND, 24 SPORT_BACKUP, 25 ULF_CONFIG, 26 SLEEP, 27 EXERCISE_BENCHMARK, 28 POWER_GUIDANCE, 29 SPORTING_EVENT, 30 ACTUAL_STEP_RECORDING_CONFIGURATION, 31 LHA_BACKUP, 32 BBI_RECORDING_CONFIGURATION, 36 HRV_STATUS, 39 UNINSTALL_APP; 7, 33–35, 37, 38 and 40–63 unnamed. *inferred (Gadgetbridge, `SynchronizationMessage`)* | Bits seen in CAP-1 (§5.1) had no names. | Gadgetbridge-only names; annotates Q-SYNC-1. Read with them, the mask after the foreground event names what the watch wants exchanged, not what it has: bit 5 (ACTIVITIES) is absent although activities sync. The single bit 8 in the 5037 after SYNC_COMPLETE (§5.5 step 3) is SOFTWARE_UPDATE, which fits the HTTP requests that follow. |
| When to sync | Starts a listing only if the mask has bit 3, 5, 21 or 26, or the type is MANUAL; otherwise it only ACKs. *inferred (Gadgetbridge, `SynchronizationMessage.shouldProceed`, `FileTransferHandler`)* | Connect lists after every 5037 seen, including the one with only bit 8 (seen: CAP-1). | **Disagreement; capture wins.** Ours lists after every 5037. |
| 5007 filter values | 0 = all files, 1 = default, 2 = unknown, 3 = only new. It always sends 3 and treats the reply as `u8 status, u8 unknown`. *inferred (Gadgetbridge, `messages/FilterMessage`, `messages/status/FilterStatusMessage`)* | 3 seen; reply body `00` (seen: CAP-1). | Agrees. Values 0–2 are Gadgetbridge-only names. |
| 5002 request | Same layout; request type 0 = continue, 1 = new; it sends flags **0**. *inferred (Gadgetbridge, `messages/DownloadRequestMessage`, `FileTransferHandler`)* | Connect sends flags `0x01` (seen: CAP-1). | **Disagreement on the flag; capture wins**, both evidently work. Gadgetbridge has no parser for 5054, so flags 0 is very probably "never send compressed data". Annotates Q-SYNC-4; the 5054 format stays unknown. |
| 5002 reply statuses | 0 OK, 1 INDEX_NOT_FOUND, 2 NOT_READABLE, 3 NOT_READY, 4 INVALID_REQUEST, 5 CRC_MISMATCH, 6 RANGE_EXCEEDS_FILE. *inferred (Gadgetbridge, `messages/status/DownloadRequestStatusMessage`)* | Same seven values (seen: APK). | Agrees. |
| Directory | Parses the whole download as 16-byte entries from offset 0 (no header skipped), drops entries whose type pair it does not know, and requires the length to be a multiple of 16. An entry time of 0 means "no date". *inferred (Gadgetbridge, `FileTransferHandler.parseDirectoryEntries`)* | A 16-byte header precedes the entries (seen: APK, CAP-1). | **Disagreement; capture and APK win.** Gadgetbridge survives because the header does not look like a known file type. |
| Which files | Downloads entries whose type has its `pull` flag (FIT sub-types 4, 9, 15, 28, 32, 35, 38, 41, 44, 49, 52, 57, 58, 61, 66, 68, 70, 71, 72, 73, 74, 77, 79, 82, and the non-FIT reports 255/244–248), unless "fetch unknown files" is on. It does not look at the READ flag. *inferred (Gadgetbridge, `FileType`, `FileTransferHandler.conditionallyDownload`)* | Ours: every entry of data type 128 with READ. | No conflict (ours is a superset for FIT). |
| File type names | 128/38 SCORE, 128/41 CHANGELOG, 128/44 METRICS, 128/49 SLEEP, 128/58 "device", 128/68 HRV_STATUS, 128/70 HSA, 128/72 FBT_BACKUP, 128/74 FBT_PTD_BACKUP, 128/73 SKIN_TEMP, 128/79 sleep disruption, 128/61 ECG, 255/17 Connect IQ program, 255/4 course download, 8/255 device XML at index `0xFFFD`. **255/19 is not in its list.** *inferred (Gadgetbridge, `FileType`)* | 255/19 is what Connect uploads after every sync (seen: CAP-1). | Gadgetbridge does not know 255/19 and never uploads it: Q-SYNC-2 stays open, with the added fact that watches sync fine without it (*inferred*). |
| 5004 from the watch | Checks `offset == bytes so far` and the running CRC; on a mismatch it throws instead of answering, so the watch gets no status. Its ACK is always `status 0, transfer status 0, u32 next offset`. *inferred (Gadgetbridge, `FileTransferHandler.FileFragment`, `messages/FileTransferDataMessage`)* | The app answers with status codes (seen: APK). | Capture/APK wins. |
| 5004 ACK status names | 0 OK, 1 RESEND, 2 ABORT, 3 CRC_MISMATCH, 4 OFFSET_MISMATCH, 5 SYNC_PAUSED. *inferred (Gadgetbridge, `messages/status/FileTransferDataStatusMessage`)* | 0, 2, 3, 4 known from the APK. | Agrees; 1 and 5 are Gadgetbridge-only names. (The summary's earlier "Gadgetbridge has no resend and abort statuses" is corrected: it names them but never sends them.) |
| 5008 | Request flags by bit: `0x04` CRYPTO, `0x08` APPEND, `0x10` ARCHIVE, `0x20` ERASE, `0x40` WRITE, `0x80` READ. Reply: `u8 status, u8 result (0 applied, 1 error — guessed), u16 x, u8 flags`; Gadgetbridge adds 1 to `x` to get back the file index and marks that as unsure. *inferred (Gadgetbridge, `messages/SetFileFlagsMessage`, `messages/status/SetFileFlagsStatusMessage`)* | Reply `00, x = index − 1, 10` (seen: CAP-1). | Agrees, including "index − 1", which its authors also only observed. Annotates Q-GFDI-2: the first body byte is a result code. |
| When to archive | Right after the file is written to phone storage, unless "keep data on device" is set. A file whose name (type, index, date) already exists locally is archived **without downloading**. *inferred (Gadgetbridge, `GarminSupport.evaluateGBDeviceEvent`, `processDownloadQueue`, `alreadyDownloaded`)* | Ours archives only after the caller confirms safe storage, and never without a download in the same cycle. | Ours is stricter on purpose: the watch reuses indices (seen: CAP-1/2), so archiving by name could lose a file. Not ported. |
| 5009 | Exactly one 16-byte directory entry; Gadgetbridge then downloads that file if its type is wanted. *inferred (Gadgetbridge, `messages/FileAvailableMessage`)* | Same 16 bytes (seen: APK). | Agrees. (The summary's earlier "Gadgetbridge expects a 17th byte" does not hold at `7257742b`.) Our client ACKs it and fetches a readable FIT entry without a listing (port-list row 22). Ours: while a cycle runs, announced files (at most 32) join the end of that cycle; a file also in the listing is fetched once (same index, type, sub-type and file number; the timestamp is not compared, since Gadgetbridge allows 0 there); a file this connection already archived or gave up on is not fetched again on a repeated 5009, only from the next listing (no endless cycles); every cycle still ends with SYNC_COMPLETE. |
| End of sync | Sends 5030 event 0 when the queue is empty and the files are parsed, and again after every finished upload. *inferred (Gadgetbridge, `GarminSupport.finishFileSync`, `FileTransferHandler.Upload`)* | 5030 event 0 after the downloads (seen: CAP-1). | Agrees. |
| No FIT files in the listing | Taken as "this device serves files over classic Bluetooth" (§9.1). *inferred (Gadgetbridge, `GarminSupport.onDirectoryWithoutFitFiles`)* | An empty pending listing is normal on the Instinct 2 (seen: CAP-1, second listing). | Gadgetbridge only acts when 5024 carried a classic address, so no conflict. |
| New sync protocol | On devices with capability 90 (or a per-model default) files are listed and pulled through `Smart.file_access` (43) and bulk data runs on the `0x..18` services; the legacy 5002 of index 0 is still sent first so the watch flushes its monitoring file. *inferred (Gadgetbridge, `FileSyncServiceHandler.kt`, `GarminSupport.onFetchRecordedData`)* | The Instinct 2 used the legacy sync (seen). | Gadgetbridge-only; not built. |

### 9.5 Uploads to the watch (never built by us; all *inferred*)

Seen in CAP-1 only as the 255/19 upload (§5.5), whose bytes were not decoded field by field. Gadgetbridge's flow, *inferred (Gadgetbridge, `FileTransferHandler.Upload`, `messages/CreateFileMessage`, `messages/UploadRequestMessage` and their status classes)*:

1. **P→W 5005** `u32 size, u8 data_type, u8 sub_type, u16 file_index (0), u8 reserved (0), u8 sub-type mask (0), u16 number mask (0xFFFF), u16 path length (0) + path, u64 random id`. Reply: `u8 status, u8 create_status, u16 file_index, u8 data_type, u8 sub_type, u16 file_number`; create status 0 OK, 1 DUPLICATE, 2 NO_SPACE, 3 UNSUPPORTED, 4 NO_SLOTS, 5 NO_SPACE_FOR_TYPE.
2. **P→W 5003** `u16 file_index (from the reply), u32 size, u32 offset (0), u16 crc_seed (0)`. Reply: `u8 status, u8 upload_status, u32 offset, u32 max_size, u16 crc_seed`; upload status 0 OK, 1 INDEX_UNKNOWN, 2 INDEX_NOT_WRITEABLE, 3 NO_SPACE_LEFT, 4 INVALID, 5 NOT_READY, 6 CRC_INCORRECT.
3. **P→W 5004** `u8 0, u16 running crc, u32 offset, data` with at most `max_packet − 13` data bytes; the next chunk goes out when the watch's ACK (`u8 status, u8 transfer_status, u32 next offset`) arrives with the expected offset.
4. **P→W 5030 event 0** after the last ACK.

Check against CAP-1 when `tools/decode` learns to reassemble uploads: Connect's upload went to index `0xFFFB`, which is not what a `file_index 0` request would be expected to return, so a fixed index may be requested for 255/19. Workouts are uploaded this way as type 128/5 (`GarminSupport.onSyncWorkoutTemplate`), alarms as a 128/2 settings file on watches without REALTIME_SETTINGS.

Chunk size note for Q-SYNC-6: a 5004 message is 13 bytes longer than its data (length 2, id 2, flags 1, crc 2, offset 4, message CRC 2). The first chunk of the 5157-byte activity in CAP-2 held 3969 bytes: 3969 + 13 = 3982, COBS adds one byte per 254 (16), and the two frame delimiters make exactly 4000 = `max_packet`. So the watch sizes a chunk so that the **framed** message fits `max_packet` (*inferred* from one chunk; our arithmetic, not Gadgetbridge's). Gadgetbridge's `max_packet − 13` for uploads ignores the COBS bytes and would exceed that limit by up to 18 bytes on a 4000-byte link; if we port the upload, use the stricter size.

### 9.6 Companion duties and protobuf services

| Topic | Gadgetbridge | Ours (capture / app) | Verdict |
| --- | --- | --- | --- |
| Smart services it handles | `calendar` 1, `connect_iq_http` 2, `connect_iq_installed_apps` 3, `connect_iq_app_settings` 4, `data_transfer` 7, `device_status` 8, `find_my_watch` 12, `core` 13, `sms_notification` 16, `explore_sync` 22, `device_message` 24, `credentials` 27, `samd` (ECG) 39, `realtime_settings` 42, `file_access` 43, `gncs` 49. In `core` it handles the sync response, a location request, the location-updates switch and the connection-ready notification (deliberately unanswered: some firmware misbehaves when it is answered). *inferred (Gadgetbridge, `ProtocolBufferHandler`)* | Field numbers agree with our recovered `gdi_smart.proto`. | Agrees. Gadgetbridge's schema names 71 services; ours has the full set from the app. |
| HTTP proxy | The watch sends either a raw request (`raw_resource_request`: url, method, headers, body, `use_data_xfer`) or a Connect IQ request (headers and body in Garmin's binary JSON). Gadgetbridge answers from local handlers chosen by host and URL path. Anything else is forwarded to the internet only if the user's per-device firewall allows it, and hosts ending in `garmin.com`, `garmin.cn` or `dciwx.com` are never forwarded. With `use_data_xfer` the reply carries a transfer id and size, and the watch pulls the body in chunks through `data_transfer` 7; with `accept-encoding: gzip` the body is gzipped. *inferred (Gadgetbridge, `http/HttpHandler`, `http/DataTransferHandler`, `http/interceptors/FirewallInterceptor`)* | The Instinct 2's requests after sync carried only an opaque field 11 (seen: CAP-1). | Our recovered schema names field 11 `encrypted_request` (an `EncryptedEnvelope`); Gadgetbridge's schema stops at field 9 and has a `DECRYPTION_FAILURE` response status. So those requests are encrypted for Garmin and a local proxy cannot answer them. Annotates Q-COMP-2. Whether the Instinct 2 also sends clear requests (weather, ephemeris) is not shown by our captures. |
| GPS assist | Only over HTTP: host `api.gcs.garmin.com` (or `.cn`), paths under `/ephemeris/`. Three body formats: a tar holding `CPE_GPS.BIN`, `CPE_GLO.BIN`, `CPE_GAL.BIN`, `CPE_QZSS.BIN` (the request's `constellations` query lists the ones wanted); RxNetworks data (path contains `/rxnetworks/`), gzip whose content starts `01 00` followed by a big-endian Unix time not older than 7 days; Sony CPE (`/ephemeris/cpe/sony`), starting `2A 12 A0 02`. Replies carry an ETag, honour `if-none-match` with 304, and set `cache-control: max-age=14400`. The user supplies the file; Gadgetbridge does not download it. The GFDI messages 5019/5020 and 5028/5029 (EPO) are named but not implemented, and capability 28 is not claimed. *inferred (Gadgetbridge, `http/interceptors/AgpsInterceptor`, `agps/GarminAgpsFile`, `agps/GarminAgpsDataType`, `devices/garmin/GarminCapability`)* | §6: path (a) a 255/19 upload, path (b) HTTP; neither confirmed. | Gadgetbridge-only. The Instinct 2 does not advertise capability 28, so the EPO messages are unlikely on it. Annotates Q-COMP-1, which still needs a capture. |
| Weather | 5014 payload `u8 format, s32 latitude, s32 longitude, u8 hours of forecast` (10 bytes). If the watch has capability 26, Gadgetbridge pushes FIT `weather_conditions` definitions (5011) and data (5012): one current-conditions record with the location name, up to 12 hourly forecast records, then daily forecast records. Newer watches instead ask over HTTP: `/weather/v1` or `/v2` + `/current`, `/forecast/hour`, `/forecast/day`, and `/weather/pointWinds`, answered with JSON. *inferred (Gadgetbridge, `messages/WeatherRequestMessage`, `GarminSupport.encodeWeather`, `http/interceptors/WeatherInterceptor`)* | 5014 with 10 bytes, then 5011/5012 (seen: CAP-3). | Agrees with CAP-3; settles Q-COMP-3 for this watch as "5014" (seen), with the field layout *inferred*. |
| 5011 / 5012 replies | 5011 status byte: 0 APPLIED, 1 NOT_UNIQUE, 2 OUT_OF_RANGE, 3 NOT_READY. 5012: 0 APPLIED, 1 NO_DEFINITION, 2 MISMATCH, 3 NOT_READY. A 5011 holds one or more FIT definition records, a 5012 FIT data records that use them. *inferred (Gadgetbridge, `messages/status/FitDefinitionStatusMessage`, `FitDataStatusMessage`)* | Seen without the status names. | Gadgetbridge-only names. |
| Notifications | 5036 `u8 enable, u8 unknown`; reply body `u8 0 enabled / 1 disabled, u8 enable echoed, u8 0`. 5033 announces a notification: `u8 update type (add / modify / remove), u8 category flags, u8 category, u8 count, u32 notification id, u8 phone flags`. 5034 is the watch's request: command 0 = get attributes (id + list of attribute ids with maximum lengths), 1 = get app attributes, 2 = legacy action, 128 = action (reply, dismiss, accept or reject call, block app, custom action index). 5035 carries the answer in chunks. Attribute ids: 0 app id, 1 title, 2 subtitle, 3 message, 4 message size, 5 date, 7 negative action label, 127 actions, 128 attachments. Pictures go through `gncs` 49. *inferred (Gadgetbridge, `NotificationsHandler`, `messages/Notification*Message`)* | 5033–5036 seen in CAP-1, not decoded in detail. | Gadgetbridge-only detail. Built in stage 2f (`core/src/duties/notification.rs`, `core/src/client/companion.rs`), byte-exact against synthetic frames only: **inferred (Gadgetbridge)** until the live test. Differences [ours]: the phone offers only "Dismiss" (and reports reply, call and custom actions it receives), the attachment count is answered 0, the date uses the phone's UTC offset, text is cut in bytes at a character boundary, one chunk is in flight at a time. The phone claims capability bit 6 only while the duty is on; with it off the watch's 5034/5036 are answered with status 2, as before. |
| Music | 5042 from the watch; reply body `u8 n` + n command ids: 0 toggle play/pause, 1 next, 2 previous, 3 volume up, 4 volume down, 5 play, 6 pause, 7 skip forward, 8 skip back. 5041 `u8 command`. 5049 pushes player, queue and track attributes to the watch. *inferred (Gadgetbridge, `messages/MusicControl*Message`)* | 5042 and 5049 seen in CAP-1. | Gadgetbridge-only detail. Built in stage 2f (`core/src/duties/music.rs`): **inferred (Gadgetbridge)** until the live test. The phone claims bits 7 and 52 only while the duty is on, advertises commands 0-6, pushes track (artist, album, title, duration), state (`playing,speed,position`) and volume through 5049 with a fixed attribute order, and turns 5041 into `Event::MusicCommand`. |
| Find my phone / watch | 5039 `u8 duration`, 5040 empty. Finding the watch is protobuf `find_my_watch` 12 with a 60 s timeout. *inferred (Gadgetbridge, `messages/FindMyPhone*Message`, `GarminSupport.onFindDevice`)* | Not seen. | Gadgetbridge-only. Built in stage 2f (`core/src/duties/find.rs`): **inferred (Gadgetbridge)** until the live test. 5039 becomes `Event::FindPhoneStart { duration_s }`, 5040 `Event::FindPhoneStop`; both are acknowledged. Find my watch (`Input::FindWatch`) is only sent when the watch's own capability mask has bit 9; a refusal or 30 s of silence is `Event::FindWatch { Failed }`, never a closed session. The phone claims bits 8 and 9 only while find my phone is on. |
| Battery | 5023 `u8 status, u8 voltage × 100, u8 percent`; status bits 4–6: 1 good, 2 ok, 3 low, 4 critical, 5 new, 7 invalid. Gadgetbridge also asks through `device_status` 8 at every connection. *inferred (Gadgetbridge, `messages/BatteryStatusMessage`, `GarminSupport.requestBatteryUpdate`)* | Not decoded. | Gadgetbridge-only. Our client ACKs 5023 and reports `Event::Battery` (port-list row 28). It sends the `device_status` request (`42 02 12 00`: Smart 8 { request 2 {} }) only when the host asks (`Input::RequestBattery`), not at every connection [ours], a request left unanswered for 30 s is dropped with an anomaly instead of closing the session; a 5023 with status 7 (invalid) gives no event. It reads the answer's level (field 3 → 2), battery status (3) and hours remaining (4) from a watch 5043 or 5044; field numbers from our schema (`proto/gdi_device_status.proto`). |
| Calendar, contacts | Calendar events answer a `calendar` 1 request. Contacts are served over the HTTP proxy at `/device-gateway/usercontact/contacts`. *inferred (Gadgetbridge, `ProtocolBufferHandler`, `http/interceptors/ContactsInterceptor`)* | Not seen. | Gadgetbridge-only. |
| Watch settings | Uses `realtime_settings` 42 with its own names for fields 1–9 (definition, state, change, change notification, init). It sends the init request (field 8) with a locale and region `"us"` right after 5050 when the watch has capability 92. *inferred (Gadgetbridge, `GarminSupport.evaluateGBDeviceEvent`, `devices/garmin/GarminRealtimeSettingsFragment`)* | Fields 1–13 with the app's names (§6a; seen: CAP-3). | Same wire fields; the app's names and types win (Gadgetbridge's types are wrong in places and it lacks fields 10–13). |
| Fake OAuth | When the watch asks for Garmin credentials (`credentials` 27, or HTTP `/oauth/…`), Gadgetbridge can hand out random tokens so the watch believes it is signed in. *inferred (Gadgetbridge, `ProtocolBufferHandler`, `http/interceptors/OauthInterceptor`)* | The Instinct 2 advertises capability 77 (OAUTH_CREDENTIALS); no such request in CAP-1..3. | Gadgetbridge-only; not built (owner decision if ever needed). |

### 9.7 FIT interpretation (compared with `core/src/derive`)

All *inferred (Gadgetbridge, `fit/FitImporter`, `fit/messages/FitMonitoring`, `devices/garmin/GarminSleepSessionProvider.kt`, `fit/RawSleepStream.kt`)* unless a capture or file is named.

- **Monitoring `timestamp_16`** is resolved against the last *monitoring* timestamp, with wrap-around handled in both directions. This is the rule we arrived at from our own files (Q-FIT-2); two independent sources now agree.
- **Activity type and intensity** can come packed in one byte: low 5 bits activity type, high 3 bits intensity. Steps are counted as `cycles × 2`, kept as a running total per activity type and summed over the types.
- **Stress and body battery** come from the same message (227); negative stress values are "no reading".
- **Sleep stages:** each `sleep_level` row marks the **end** of the stage it names; the stage begins at the previous row (or at the session start). Our parser stores the row at its own time and does not say which end it is.
- **Sleep session bounds** come from FIT `event` messages for sleep detection (start and stop), not from `sleep_summary`. Naps are message 412 with their own start and end.
- **Watches without sleep staging** write raw per-minute rows (movement, heart rate) without timestamps; the row's position after the `sleep_data_info` message gives its time. Gadgetbridge estimates stages from them; we do not (an estimate would be derived data).
- **SpO2 mode:** 1 = manual reading, 3 = automatic. Values ≤ 0 are dropped.
- **Resting heart rate:** prefers `current_day_resting_heart_rate` over `resting_heart_rate` in message 211.
- **Not answered:** global message 233 (Q-FIT-1) is absent from Gadgetbridge's profile too.

### 9.8 Disagreements in one list

Capture or app evidence wins in every case; our client already follows it.

1. Compact ids and transaction numbers: Gadgetbridge ignores them and never echoes a txn (§9.2).
2. Reply to 5024: Gadgetbridge sends a bare ACK and then the ACK with host details; the app sends one (§9.3).
3. ACK of a protobuf message: Gadgetbridge often sends no 8-byte body; the app always does (§9.2).
4. 5052 daylight-saving fields: Gadgetbridge's order is the reverse of CAP-1's and of the app's rule in winter time (§9.3; Q-TIME-2, settled).
5. 5037: Gadgetbridge syncs only for some masks; Connect lists after every one (§9.4).
6. 5002 flags: Gadgetbridge 0, Connect 1 (§9.4).
7. Directory: Gadgetbridge does not skip the 16-byte header (§9.4).
8. Bring-up: no REGISTRATION service, info pages or KEEP_ALIVE in Gadgetbridge; first characteristic pair instead of a random one; statuses 2 and 3 of a register response not handled (§9.1).
9. Unhandled requests: silence in Gadgetbridge, automatic reply in the app (§9.2).
10. 5101: echoed by Gadgetbridge, an error for the app on a bonded link (§9.3).
11. File chunk errors: Gadgetbridge sends no status; the app sends abort / CRC / offset statuses (§9.4).
12. MLR timing: ACK delay, first timeout and the ack number in retransmissions (§1.4); immediate ACK on out-of-order packets (§9.1).
13. Name of 5030 event 8 (§9.3) and of the 5037 types (§9.4): names only, same bytes.
14. Protobuf schemas: Gadgetbridge's lack `RawResourceRequest.encrypted_request` and the app's RealtimeSettings names and defaults (§9.6).
