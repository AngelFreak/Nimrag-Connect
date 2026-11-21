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
| Q-TIME-2 | **Settled (seen: APK `r6a.m65223a`, called by `r6a.onMessageReceived`; CAP-1 agrees).** The two daylight-saving fields of the 5052 reply are (next transition into DST, next transition out of DST), not (next transition, the one after). Both are 0 when the zone has no further transition. Our client's "next, following" gives the same bytes only in winter time. | was blocking; the client must change before the March clock change (PROTOCOL §3 step 6) | Done statically. A summer-time capture would be a second confirmation, not a requirement. |
| Q-ADV-1 | Does the Instinct 2 advertise Garmin manufacturer data (company id 0x0087) starting with its product number, as Gadgetbridge assumes for recognition (PROTOCOL §9.1)? | no (the name pattern is the fallback) | Read the advertising data in 2c, or info page 1 in CAP-1. |

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

## Workouts and uploads (stage 4a)

All of this is *inferred (Gadgetbridge)* and built; none of it has met a real watch. They are the first things the stage 4a live test must settle (the owner's numbered list is in `android/README.md`, "Workouts").

| Id | Question | Blocking? | How to settle |
| --- | --- | --- | --- |
| Q-UPLOAD-1 | Does the Instinct 2 accept a 5005 CreateFile for type 128/5 with `file_index 0`, answer 5003 with status OK and take the 5004 chunks? Connect's own 255/19 upload went to index `0xFFFB` (PROTOCOL 5.5): a fixed index may be what the watch expects for some types | yes, for 4a | Send a one-step workout; the phone log says which stage refused and with which status name (`Refused ... status N, NAME`) |
| Q-UPLOAD-2 | Is our 3969-byte chunk (whole frame in `max_packet`) accepted? Gadgetbridge sends `max_packet - 13`; our rule is stricter, so only a watch that wants *larger* chunks would object. A workout file is a few hundred bytes, so the first live test sends one chunk and does not exercise this | no | A workout with many steps is still under 4 KB; a multi-chunk upload needs a file over 3969 bytes (not a workout) |
| Q-UPLOAD-3 | After the upload, does the watch need anything more (a file-list refresh, a sync, a restart) before the workout shows under Training > Workouts? Gadgetbridge ends with 5030 event 0 only | yes, for 4a | Open the workout list on the watch after "Sent"; note whether a sync cycle was needed |
| Q-UPLOAD-4 | Sending the same workout again: does the watch answer `create_status` 1 (DUPLICATE) because it keeps files by content or by name? The id in 5005 is different every time, but the file is the same. Sending after an edit, after a deletion on the watch, and after a phone report that did not arrive (the workout stays queued and is sent again) all depend on this | no, but it decides what "Send again" does | Send twice; look at the failure text on the web page |
| Q-WORKOUT-1 | The limits in `core::fit::workout` (100 steps in the file, 20 steps in a block, 2 to 99 repeats, 63 bytes of name, 254 of note, 50 bytes of step `notes`) are cautious guesses, not Gadgetbridge's | no | Build a workout at each limit and see whether the watch shows it; widen only on evidence |
| Q-WORKOUT-2 | The values Gadgetbridge writes without explanation: `workout.capabilities` 32, `file_id.product` 65534 ("connect"), serial number 1, `weight_display_unit` kilogram on every step. Does the watch care? | no | The first accepted file settles it |
| Q-WORKOUT-3 | Targets: pace is written as a speed range in mm/s with the slower pace as the low speed; cadence is halved for running; heart rate is bpm + 100. Does the watch show them as typed on the web (a pace range, a heart-rate range, a cadence range)? | no | Build one step per target kind; compare what the watch shows |
| Q-WORKOUT-4 | Which sports and sub sports the watch lists (strength is written as timed intervals; "other" as generic) | no | See the sport shown for each workout |
| Q-WORKOUT-5 | The exercise catalogue (named strength exercises with sets and reps) is not ported: it needs the exercise list from the local FIT profile and a picker in the builder | no (follow-up) | Port `GarminExerciseCatalog` once the plain workouts work |

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

## Gadgetbridge answers (stage 2p)

What Gadgetbridge at `7257742b` says about the questions above. Each answer is *inferred (Gadgetbridge)*: it is what that code does, not something we have seen. A question stays open until the capture or test in its own row is done; the last column says what changed. Details and file names are in PROTOCOL.md §9.

| Id | Gadgetbridge's answer | State after 2p |
| --- | --- | --- |
| Q-MLR-1, Q-MLR-2 | Nothing new; its MLR is a re-implementation with different timing (PROTOCOL §1.4). | open |
| Q-MLR-3 | Plain mode is its original transport and still a per-device option: register GFDI with mode 0, then one handle byte per write. | answered as *inferred*: mode 0 works on the devices Gadgetbridge supports (per its authors). Built as a fallback in `core::client::transport` (plan row 8): we ask for mode 2 and carry on in plain mode when the response has no reliable flag. The Instinct 2 itself is still untested: a register response without the flag has never been seen, so the proof is one real session in plain mode (`grant_reliable = false` in the synthetic tests only shows our side). |
| Q-ML-1 | It accepts any close-all status and goes on. | open; confirms the status can be ignored |
| Q-ML-2 | It uses the fixed client id 2 on every device and drops control messages addressed to another id. | open for the case "Connect connected at the same time"; a client id of our own is safe otherwise |
| Q-ML-3 | It reads only the byte after the handle and treats non-zero as "reliable". | open |
| Q-GFDI-1 | First tail byte 1 means a BLE and a classic Bluetooth address (6 bytes each) follow; then one unknown byte. | first byte explained (matches the app's "dual pairing"); second byte open |
| Q-GFDI-2 | Body = `u8 result (0 applied), u16 x, u8 flags`; it also only observed `x = index − 1`. | layout explained; why `− 1` is open |
| Q-GFDI-3 | The two bytes are `chunk status` (0 kept, 1 discarded) and a status code (0, 100–103, 200, 201). `01 c9` = discarded, 201 "unknown". | answered as *inferred*; which watch message the app rejected can be read from CAP-1 t=5.42 |
| Q-GFDI-4 | 6 = auto upload enabled, 7 = weather conditions enabled, 8 = weather alerts enabled; it sends the same three values as Connect. | answered as *inferred*; the diff capture is still the proof |
| Q-GFDI-5 | It stays silent on unhandled requests on all its devices, and sends status 2 only to refuse 5052 when time sync is off. | open; both silence and status 2 are evidently tolerated, so our status 2 reply is low risk |
| Q-PAIR-1 | On a first connection it sends system events 4, 0 and 14 (pair complete, sync complete, setup wizard complete) after the normal handshake; bonding is left to Android. | open; ported (row 16): `ClientConfig::first_connect` sends this sequence and reports `Event::PairingCompleted { all_acked }`; the capture must confirm it |
| Q-AUTH-1 | It answers 5101 by echoing it with "guess ok" and implements nothing further, which only works if watches do not insist on Garmin Auth. | open |
| Q-TIME-1 | Nothing: it always answers, or refuses with status 2. | open |
| Q-SYNC-1 | Names for bits 0–6, 8–32, 36 and 39 (PROTOCOL §9.4). | answered as *inferred*; the bit-by-bit captures are still the proof |
| Q-SYNC-2 | It has no 255/19 type and never uploads such a file; its devices sync without it. Upload message layouts are in PROTOCOL §9.5. | open; "not needed for syncing" is now better supported |
| Q-SYNC-3 | It can leave files unarchived ("keep data on device") and then skips files it already has by name, so re-listing of unarchived files is what it expects. | open; supports the assumption in the 2a contract |
| Q-SYNC-4 | It sends 5002 flags 0 and has no 5054 parser. So flags 0 very probably means "uncompressed only". The 5054 format is not in Gadgetbridge. | half answered as *inferred*; format open. Our client could send flags 0 to avoid 5054 altogether (integrator decision). |
| Q-SYNC-5 | Type names only (38 score, 58 "device", 70 HSA, 72/74 backups); no parser for their content. | open |
| Q-SYNC-6 | Nothing for downloads. Its upload chunk is `max_packet − 13` data bytes. Our own arithmetic on CAP-2 suggests the watch fits the whole COBS frame into `max_packet` (PROTOCOL §9.5). | candidate rule from one chunk; check on the next multi-chunk file |
| Q-FIT-1 | Message 233 is not in its profile. | open |
| Q-FIT-2 | It resolves `timestamp_16` against the last monitoring timestamp, the rule we found ourselves. | supported by a second source; still re-checked on every capture |
| Q-COMP-1 | GPS assist only over HTTP (`api.gcs.garmin.com/ephemeris/…`), three file formats; no 255/19 upload, no EPO messages. | open for the Instinct 2; if its ephemeris request is in clear, the ported handler can serve it |
| Q-COMP-2 | Its schema has no field 11. Our recovered schema names it `encrypted_request`; Gadgetbridge has a "decryption failure" response status. | answered from our own schema: the request is encrypted for Garmin and cannot be served locally. What it asks for stays unknown. |
| Q-COMP-3 | 5014 layout `u8 format, s32 lat, s32 lon, u8 hours`; answer by FIT weather messages if the watch has capability 26, else HTTP `/weather/…`. | settled for this watch: 5014 (seen in CAP-3), layout *inferred* |
