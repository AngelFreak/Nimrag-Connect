# Garmin Instinct 2 ↔ phone link protocol

Status: phase 0. The static map is complete and one live capture has been decoded.

## Summary

- **Transport.** The Instinct 2 (product 3888, firmware 17.22) uses the multi-link GATT service `6A4E2800-667B-11E3-949A-0800200C9A66`. Each app gets one notify/write characteristic pair (`6A4E281x`/`6A4E282x`). GFDI runs on it as service 1, in the reliable mode (MLR). The watch grants MLR when asked. The negotiated ATT MTU is 23, so every packet is at most 20 bytes. [seen: APK + CAP-1]
- **Application-layer crypto: none in our setup.** The app has a "Garmin Auth" scheme: message ids 5101–5112, a session key and per-packet counters; the cipher itself was not identified. It is only used when the phone is not BLE-bonded to the watch, or the phone is made by VIVO. The Pixel is bonded, and in CAP-1 every GFDI message after COBS is plaintext with a valid CRC. Link-layer security is standard LE bonding with a 16-byte key. [seen: APK `DefaultAuthDelegate.isGarminAuthAllowed`; CAP-1]
- **Where the app and Gadgetbridge disagree:**
  - unhandled requests: the app auto-replies, Gadgetbridge stays silent
  - 5101: the app treats it as an error on a bonded link; Gadgetbridge echoes it back
  - file chunks: the app has resend and abort statuses; Gadgetbridge has none
  - FileReady: Gadgetbridge expects a 17th byte that the app does not read
  - compact transaction id: the app masks it to 5 bits, Gadgetbridge assumes 7
  - protobuf schemas: Gadgetbridge has wrong types in RealtimeSettings and lacks the app's default values

  The multi-link and MLR header layouts agree.

### Evidence keys

| Key | Meaning |
| --- | --- |
| APK `cls.method` | Garmin Connect 5.31 (11761), names as produced by jadx 1.5.3 `--deobf`. See `PROVENANCE.md`. |
| CAP-1 | HCI snoop capture of 2026-11-19 09:41: Bluetooth restart, reconnect, one manual sync and one app-started sync. Raw file kept private; SHA-256 `34d283c5…79e49ae`. Decoded by `tools/decode`. |
| CAP-2 | HCI snoop capture continuing to 2026-11-19 10:02: Connect and Explore reconnect on new characteristic pairs (about t = 700 s), then a sync after a recorded test activity (activity 128/4, monitoring 128/32, metrics 128/44, 128/58). Raw file kept private; SHA-256 `fc919d19…91417e7f`. |
| GB `path` | Gadgetbridge at `7257742b`; cross-check only. |

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

MLR is optional for a client. Without the native library the app registers GFDI in mode 0 and runs plain multi-link (seen: `kit.mo8947r`). Whether the Instinct 2 accepts mode 0 is Q-MLR-3.

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

A response to a request sent with a txn reuses that txn (CAP-1). A response to a protobuf message carries an 8-byte body. Gadgetbridge reads it as `u16 request_id, u32 data_offset, u8 chunk_status, u8 status_code` (inferred). In CAP-1 the last two bytes are almost always `00 00`; once they were `01 c9`, which is **unknown**.

## 3. Session start (GFDI)

Seen in CAP-1 in this order, and matching APK `zxl`, `ayl`, `smg`, `vxk`, `r6a`:

1. **W→P 5024 DeviceInformation:** `u16 protocol (150), u16 product (3888), u32 unit_id, u16 software (1722), u16 max_packet (4000), str bt_name, str device_name, str model_name` (each string is a u8 length plus UTF-8), followed by 2 more bytes. The app parses those as an optional `u8 dual_pairing` and an optional `u8 limitation`.
2. **P→W 5000 ACK(5024)** with the host's details: `u16 protocol (150), u16 0xFFFF, u32 0xFFFFFFFF, u16 app_version (Connect: 11761; Explore: 38215), u16 0xFFFF, str friendly_name, str manufacturer, str model, u8 1`.
3. **W→P 5050 Configuration:** `u8 n` then an n-byte capability bitmask (Instinct 2 bits: 0, 3–20, 26, 29, 33, 36–39, 42–44, 46, 47, 49, 51, 52, 54, 57–59, 65, 67–74, 76, 77, 81, 82, 84, 85, 88, 91, 92, 94–96). The phone ACKs it, then sends its own 5050.
4. **P→W 5043 protobuf `Smart.core(13)`** FeatureCapabilitiesRequest. It is sent because the watch has capability bit 95. The watch answers with a FeatureCapabilitiesResponse in 5044. [seen: CAP-1, APK `vxk`]
5. **P→W 5031 Supported file types** (see §5.6), **5026 SetDeviceSettings** `03 | 06 01 01 | 07 01 01 | 08 01 00` (three TLV settings, ids 6, 7, 8), and **5030 SystemEvent HANDSHAKE_COMPLETE (8)**.
6. **W→P 5052 CurrentTime** request `u32 ref`. The phone replies `u32 ref, u32 garmin_time, s32 utc_offset_s, u32 next_dst, u32 following_dst`. Time is seconds since 1989-12-31T00:00Z. In CAP-1 the DST fields were 1175130000 and 1193878800, which are Garmin-epoch times in 2027. [seen]
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

Bit meanings come from Gadgetbridge (inferred). For example 5 = activities, 26 = sleep and 36 = HRV status.

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

1. **P→W 5005 CreateFile + 5003 UploadFile + 5004** send a 1372-byte file of type 255/19 to the watch, at index 0xFFFB. Its content is **unknown**: possibly GPS assist data or settings. See `OPEN-QUESTIONS.md`.
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

## 7. Protobuf

All 201 generated schemas were recovered from the app into `proto/`. `Smart` is empty and binds each service as an extension. The services that matter: `connect_iq_http` 2, `data_transfer` 7, `device_status` 8, `core` 13, `realtime_settings` 42, `file_access` 43 (item-based "new sync") and `gncs` 49. The full map is in the `gdi_smart.proto` header. A 5043/5044 payload is `u16 request_id, u32 offset, u32 total, u32 chunk_len` followed by Smart bytes; that header is inferred from Gadgetbridge and matches every message in CAP-1. The Instinct 2 used legacy GFDI file sync, not FileAccess, in CAP-1.

## 8. Hook points

See `HOOKS.md`.
