# Open questions

Each entry lists what is unknown, whether our client needs the answer to pair, list and download (**blocking**), and the capture, hook or test that would settle it. "Phase 2 test" means a test our own client runs against the watch.

## Transport

| Id | Question | Blocking? | How to settle |
| --- | --- | --- | --- |
| Q-MLR-1 | Does the **watch** side use the same MLR core as the app's library (window, 10 ms ACK, 2 s minimum timeout)? The app side is now known statically (PROTOCOL §1.4). | no; our client follows the app's parameters | Phase 2 test: withhold ACKs and measure the watch's retransmit timing in a capture. |
| Q-MLR-2 | How the watch handles a seq gap and wraparound in practice. Statically: go-back-N, ACK-only packets have seq 0, no reset packet. | no | A capture with radio loss or a large file (> 64 packets) |
| Q-MLR-3 | Can a client register GFDI with mode 0 (no MLR) and get plain-handle traffic? This would simplify our client. | no (MLR is understood) | Phase 2 test: register mode 0 and check the response flags and traffic. |
| Q-ML-1 | Meaning of close-all response status 1 (seen in CAP-1 for both apps) | no | Phase 2 test: send close-all right after a fresh connect and see whether the status changes. |
| Q-ML-2 | Does the watch tie anything to the u64 client id across characteristic pairs? CAP-2 shows Connect using id 1 on two pairs in turn, with per-pair handles. Would our client clash with Connect if both used 1 at the same time? | **yes**, while Connect is still installed | Phase 2 test with our own client id; with the official app removed it no longer matters. |
| Q-ML-3 | What do the `flags` bits other than bit 0 and the `extra` byte in a register response mean? | no | More captures and native analysis |

## GFDI

| Id | Question | Blocking? | How to settle |
| --- | --- | --- | --- |
| Q-GFDI-1 | The 2-byte tail after the strings in 5024 (APK: optional dual-pairing byte and "limitation") | no | Hook `zxl.onMessageReceived` and log the parsed values, or reason from APK `zxl` |
| Q-GFDI-2 | The 5008 reply body `00 xx xx 10`, where xx xx = index − 1 in CAP-1 | no | More captures with other indices |
| Q-GFDI-3 | The last two bytes of the protobuf ACK body (`01 c9` seen once) | no | More captures |
| Q-GFDI-4 | The 5026 setting ids 6, 7 and 8 the app sends at session start (Gadgetbridge: auto-upload, weather conditions, weather alerts) | no | Change those settings in the app, capture, and diff |
| Q-GFDI-5 | What the watch does when a request goes unanswered; the app auto-replies "not supported", Gadgetbridge stays silent | **yes**, so our client should copy the app | Phase 2 test |

## Session start and pairing

| Id | Question | Blocking? | How to settle |
| --- | --- | --- | --- |
| Q-PAIR-1 | The fresh-pairing flow: bonding, PAIR_START/PAIR_COMPLETE events, FIT capabilities → connectivity, setup wizard. CAP-1 is only a reconnect. | **yes** | Capture: unpair the watch in Connect and in Android, then pair again with the official app and snoop logging on. This erases the pairing, so the owner has to agree first. |
| Q-AUTH-1 | Does the Instinct 2 ever start Garmin Auth (5101) after a normal LE bonding? APK and CAP-1 say no on a bonded link. | **yes** (it decides whether we need crypto) | The fresh-pairing capture (Q-PAIR-1) |
| Q-TIME-1 | Watch behaviour when its 5052 time request gets no reply | no | Phase 2 test |

## File sync

| Id | Question | Blocking? | How to settle |
| --- | --- | --- | --- |
| Q-SYNC-1 | The meaning of each 5037 bitmask bit (Gadgetbridge's names are unverified) | no; our client can just list files | Captures after an activity and after a night, compared bit by bit |
| Q-SYNC-2 | Content of the 1372-byte 255/19 file the app uploads after each sync (GPS assist? settings?) | no for downloading; yes for the GPS-assist duty | Extend `tools/decode` to reassemble uploads and inspect the bytes; hook `wtz`/upload task to see which app component produced it |
| Q-SYNC-3 | Behaviour if a file is never archived: is it listed again (filter 3)? Is there a limit? | no | Phase 2 test |
| Q-SYNC-4 | Meaning of 5002 `flags = 0x01` and the 5054 compressed format; 5054 has never been seen | no | APK `qv7` (compressed parser) plus a capture of a large file |
| Q-SYNC-5 | Contents of FIT sub-types 38, 58, 70, 72 and 74 (Gadgetbridge names: SCORE?, DEVICE_58, HSA, FBT_BACKUP, FBT_PTD_BACKUP) | no; phase 1 parsers decide | Decode the extracted files with the FIT decoder (phase 1). CAP-1 already holds two 128/58 files. |
| Q-SYNC-6 | Chunk size of files over `max_packet` (4000): CAP-2 has a 5157-byte activity in several 5004 chunks, all decoded; the exact rule for chunk length is not yet derived | no | Measure chunk sizes in CAP-2 and the overnight capture |

## FIT data (from the oracle check, 2026-11-19)

The owner approved one read-only comparison of a morning's derived data against their Garmin Connect account. Values, timestamps and account data stay in `private/oracle/`.

| Id | Question | Blocking? | How to settle |
| --- | --- | --- | --- |
| Q-FIT-1 | Garmin's server time series (heart rate every 2 min, stress every 3 min, respiration every 2 min) are not the mean of the per-minute values in the monitoring file: 17 of 21 HR windows agree, stress and respiration often differ by 1–9. Values the watch writes as totals (resting HR, body battery, cumulative steps) match exactly. Does the server use other data, e.g. the undecoded global message 233 the watch writes every minute? | no; we show the watch's own samples and must not claim to reproduce Garmin's charts | Decode message 233 (layout unknown); compare once more after a night sync |
| Q-FIT-2 | Monitoring `timestamp_16`: the watch wrote one sample 60 s behind a preceding event message's full timestamp. Resolving against the last *monitoring* timestamp (instead of any message) gives no backwards sample in all five captured files (inferred, not documented) | was a bug (a sample resolved to +18 h); fixed by that rule | Re-check on every new capture via the private replay test |

## Companion duties

| Id | Question | Blocking? | How to settle |
| --- | --- | --- | --- |
| Q-COMP-1 | Which GPS-assist path the Instinct 2 uses (a 255/19 upload or HTTP `ephemeris/cpe`), and what happens without it | no | Q-SYNC-2, plus a capture with mobile data off |
| Q-COMP-2 | The opaque field 11 in the watch's HTTP-over-protobuf requests (`Smart(2)`): is the request encrypted for Garmin? | no (optional duty) | Proto schema `connect_iq_http` field 11 plus a hook on the HTTP handler |
| Q-COMP-3 | Weather request format on this watch: 5014 versus HTTP | no | Capture with the weather widget open |

## Captures still needed (phase 0 step 1)

| Capture | Status |
| --- | --- |
| Reconnect and normal sync | **done** (CAP-1) |
| Fresh pairing | needs the owner (Q-PAIR-1) |
| Sync after an activity | **done** (CAP-2) |
| Sync after a night | **not yet**: CAP-1 was the first sync of the day, but the overnight sleep and HRV files had already synced before logging started. Needs tomorrow morning's first sync. |
| Settings change | **done** (CAP-3: time format, via RealtimeSettings) |
