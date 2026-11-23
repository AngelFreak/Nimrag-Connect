# Android build

Gradle (Kotlin DSL, version catalog in `gradle/libs.versions.toml`, wrapper pinned
to Gradle 9.8.2 with `distributionSha256Sum`).

- `:core` (`dev.nimrag.core`, Android library): builds `libnimrag_ffi.so` for
  arm64-v8a and x86_64 with `cargo ndk` and generates the Kotlin bindings with
  `uniffi-bindgen`; both into `core/build/generated/`, nothing committed.
- `:app` (`dev.nimrag.app`, "Nimrag Connect", minSdk 31, compile/target SDK 37):
  one activity that shows the core version string from the FFI. Stage 2c fills it.
- `:hosttest` (pure JVM): JUnit tests of the real bindings against the
  x86_64-linux-gnu `libnimrag_ffi.so` built with the `test-fixtures` feature
  (never in the APK).

## Build

Needs: JDK 25, the Android SDK (`ANDROID_HOME`) with platform 37.2, build-tools
37.0.0 and NDK 30.0.16248370, `cargo` with the pinned toolchain from
`rust-toolchain.toml` (rustup installs it and the two Android targets), and
`cargo-ndk` 4.1.2. The build checks the NDK revision and the cargo-ndk version
and fails on a mismatch.

```
./gradlew :app:assemblePublicRelease :app:assemblePublicDebug :app:testPublicDebugUnitTest :app:lintPublicDebug :hosttest:test
```

Two flavors (dimension `distribution`): `public` is the build that is published, `personal` is the
owner's own build (see "Personal flavor" below). Both have the phone's own copy of the web app
(the local server); the only difference is Garmin's FIT profile, which only `personal` has. Task names carry the flavor, for example
`:app:testPublicDebugUnitTest`.

Pinning: Gradle wrapper with `distributionSha256Sum`, every Gradle dependency in
`gradle/verification-metadata.xml` (sha256), `cargo --locked` everywhere, ABIs
limited to arm64-v8a and x86_64. The Rust library is built with
`--remap-path-prefix`, and the build fails if the `.so` contains a `/home/` path.

## No FIT profile on Android (licence)

This section is about the `public` flavor. The `personal` flavor is the one deliberate exception.

The locally generated FIT profile is Garmin data and is never redistributed. The `public`
library still has the local server (cargo feature `local-server`, web app from `web/dist`
embedded: run `npm run build` in `web/` first); it derives with Nimrag's own clean-room field map
(`NIMRAG_FIT_PROFILE=none`, `core/src/fit/profile_nimrag.rs`), which may be published.
Every cargo call from Gradle (`buildSrc/src/main/kotlin/CargoTasks.kt`) sets
`NIMRAG_FIT_PROFILE=none`, then reads cargo's build-script output and fails the
build if core's `cfg(fit_profile)` is on. `core/build.rs` and `ffi/build.rs` also
refuse any Android target with a profile (or without the variable). Finally the
tasks grep both finished `.so` files: a marker string that core emits only when a
profile is compiled in, the test-fixtures `ScriptedWatch`, and build-machine paths
must all be absent. The app also checks `fitProfileCompiledIn()` at start (it must be false in
`public`, true in `personal`; it is true only for Garmin's profile, never for Nimrag's own map).
The FFI does not expose FIT parsing.

## Personal flavor (never published)

The owner's own build: Garmin's FIT profile is compiled in. (The server inside the app is in both
flavors: cargo feature `local-server` of `nimrag-ffi`, `nimrag_server::embedded`, with the web app
embedded by rust-embed; the public build uses Nimrag's own field map there.) Every stored watch file is ingested into that local server too, so Today, Activities,
Health and Strength can show the newest data from the phone itself, also offline. See
IMPLEMENTATION_PLAN.md, stage 3o.

```
(cd ../web && npm ci && npm run build)      # the web app is embedded into the library
CARGO_BUILD_JOBS=4 ./gradlew -Pnimrag.fitProfile=/path/to/profile_data.rs :app:assemblePersonalRelease
```

`-Pnimrag.fitProfile` (or `NIMRAG_FIT_PROFILE_FILE`) names the locally generated profile
(tools/fit-profile/README.md); without it the personal tasks fail. Only the personal cargo tasks
set `NIMRAG_ALLOW_ANDROID_FIT_PROFILE=personal-apk-never-published`, the explicit opt-in that
`core/build.rs` and `ffi/build.rs` require before they accept a profile for an Android target.
The personal build checks that the profile marker IS in the `.so`. Never upload a personal APK
(it contains Garmin data); install it from the owner's own server or with `adb`. The library is
about 17 MB per ABI (the APK stores `.so` files uncompressed): build for arm64 only when size matters.

The phone's copy has exactly one local user (`local`, a normal user, no admin); every file in it
belongs to that user and the server side attaches that session, so it needs no sign-in and never
starts Forward. It is read-only over HTTP (GET and HEAD) and guarded by the per-launch secret cookie
and a loopback Host check. Gradle refuses to build a personal variant through any release,
publish, upload or sign task unless `-Pnimrag.personal` is also given, so a personal APK cannot be
produced by accident (the FIT profile it holds is Garmin data and is never uploaded anywhere).

## Dependencies

| Component | Version | Licence | Used for |
|---|---|---|---|
| Gradle | 9.8.2 (2026-10-19) | Apache-2.0 | build |
| Android Gradle Plugin | 9.5.0 (2026-10-28) | Apache-2.0 | `:core`, `:app` |
| Kotlin Gradle plugin (JVM) | 2.4.20 (2026-10-08) | Apache-2.0 | `:hosttest` (AGP 9 has built-in Kotlin for the Android modules) |
| JNA | 5.19.1 (2026-06-12) | Apache-2.0 OR LGPL-2.1-or-later | UniFFI's Kotlin loads the library through it (aar on Android, jar in `:hosttest`) |
| JUnit Jupiter / Platform Launcher | 6.1.3 (2026-08-07) | EPL-2.0 | `:hosttest` only; not shipped. EPL-2.0 is a test-time dependency, never linked into the app |
| AndroidX WebKit | 1.18.0 | Apache-2.0 | `WebViewCompat.addDocumentStartJavaScript` for the web shell (stage 3n) |
| Inter, Space Grotesk (fonts) | 5.3.0 (@fontsource) | OFL-1.1 | `app/src/main/res/font` (TrueType rebuilt from the latin WOFF files of `@fontsource/inter` and `@fontsource/space-grotesk`); licence texts in `app/src/main/assets/licences/` and in the app's Open-source licences screen |
| UniFFI (Rust) | 0.32.2 | MPL-2.0 | see `ffi/README.md` |

AndroidX and Compose are used for the UI (versions in `gradle/libs.versions.toml`).

## Web shell (stage 3n)

The app is a native bottom bar (Today, Activities, Health, Watch). The first four show one
`WebView` (`ui/shell/ServerWebView.kt`) that loads the saved server address and keeps running while
the Watch tab (one native page and its sub-pages) is on screen. The web page talks to the
app through `window.NimragApp` (document-start script, server origin only) over the
`NimragAppNative` JavaScript interface; the pure rules (origin checks, URL validation, route to tab,
status JSON) live in `ui/shell/logic` with unit tests. See IMPLEMENTATION_PLAN.md, stage 3n.
A page without a tab (Profile, Send to Garmin) highlights no tab, as the web app does. Downloads:
the web app's download rows call `NimragApp.download(path, name)` (0.8.1), which hands the file to
Android's DownloadManager with the session cookie. The web view's own download listener stays for
plain links, but the Samsung's WebView 157 never called it for "Download my data".

The Watch tab is one page (`ui/watch/WatchScreen.kt`: a problem banner, the status card or the setup
checklist, then settings rows in three groups) and sub-pages opened from its rows; `WatchPage` names
them and gives each its parent, the shell keeps the current one (it survives rotation and the system
back button goes to the parent). The pure rules (which banner, the status word, the next pairing step)
live in `ui/watch/logic` with unit tests; the shared rows, group, banner and dialog are in
`ui/components`.

## Garmin Connect (0.8.0)

The Watch tab has a "Garmin Connect" page (same row under Sync as before). It is information only:
Nimrag must be the only app that syncs the watch (Garmin Connect's Nearby devices permission off), and
to keep a Garmin account up to date the owner turns on "Send to Garmin" in the Nimrag profile
(Forward, done by the server: activities, or everything from the watch). A row opens that web page
(`/profile/garmin`) through the web view. The earlier Alongside mode (both apps sync, Nimrag copies and
never archives) was removed: the app always connects with `archive = true` and an empty skip list. The
core still supports `archive = false`; only the app no longer uses it. A stored Alongside setting is
ignored and cleared at the next start (one log line); files that were copied but never archived are listed
again on the next sync, the store dedupes them by hash and they are archived, which clears the watch.
The `copied` table stays in the database, unused.

## Phone features (stage 2f, built, live test pending)

The Watch tab has a "Phone features" page with five switches, **all off by default**:
notifications on the watch, music control, find my phone, GPS assist and weather (see "Weather"
below), plus a "Find my watch" button.
Everything here is ported from Gadgetbridge and *inferred (Gadgetbridge)* until it has been tried on
the owner's Instinct 2; only the unit tests (core byte-exact frames, ffi conversions, the Kotlin
rules) are verified.

- **Claiming a duty.** The watch only asks for a duty when our Configuration mask claims it
  (`engine/phone/logic/PhoneDuties.kt`: notifications bit 6, music bits 7 and 52, find my phone bits
  8 and 9). The mask in Settings stays the base; the bits are added per connection, and the same
  switches are passed to the client as `ClientConfig.duties`. With a switch off nothing is claimed and
  the watch behaves as before 2f. Changing a switch (or the system access below) reconnects the watch
  once (`WatchLink.onDutiesChanged`).
- **Notification access.** Notifications and music need Android's "Notification access" for Nimrag
  (`NimragNotificationListener`; system settings only, no dialog). The page shows a card with a button
  that opens that setting. Apps installed outside the Play Store get the setting greyed out until
  "Allow restricted settings" is chosen in the app info (Android 13+); the card says so and links the
  app info. A duty is effective only when its switch is on **and** the access is given
  (`PhoneDuties.effective`).
- **Notifications.** Default when on: every app except Nimrag itself and the apps left out on the
  "Apps left out" page. Group summaries, ongoing notifications (players, navigation, running
  services; calls stay) and notifications without text are skipped; an unchanged text is not sent
  twice (`engine/phone/logic/NotificationRules.kt`, tested). The listener does not touch a
  notification at all while forwarding is off (`PhoneBridge.forwarding`). "Dismiss" on the watch
  dismisses it on the phone; "Block" adds the app to the left-out list (shown on the page, easy to
  undo); other watch actions are logged as not supported yet.
- **Find my phone.** The watch's request rings the alarm stream at full volume (put back afterwards),
  vibrates and posts a heads-up with "Found it" that also asks for the full screen
  (`FindPhoneActivity` over the lock screen when Android allows it). It stops on "Found it", when the
  watch cancels, or after the time the watch asked for (10 s to 2 min, default 30 s).
- **Music.** The watch follows the phone's active media session (the one that plays, else the one with
  a track), shows title, artist, state, position and volume, and its buttons play, pause, skip, seek
  and change the volume (`MusicBridge`, `MusicRules`).
- **GPS assist.** The "GPS assist" switch makes the watch's request for satellite data (a web request
  under `/ephemeris/`) answerable, so the watch finds GPS faster. The Nimrag **server** downloads the
  file from Garmin (`GET /api/device/gps-assist`, see `server/README.md`; this is Nimrag's only
  contact with Garmin and the server has an off switch, `NIMRAG_GPS_ASSIST=off`); the phone only
  talks to that server, with the device token and the upload network rule (Wi-Fi unless "allow mobile
  data" is on), never to Garmin, and the token never follows a redirect. The switch claims capability
  bits 10 (CONNECTIQ_HTTP) and 88 (HTTP_RAW_RESOURCE_REQUEST) only while it is on (not 28: the EPO
  messages are not built), and the client is built with `Duties.gps_assist`. With it on, **every other
  web request the watch makes is refused** (nothing is forwarded; the core's firewall) and logged as
  host and path only, never the query. Where the bytes come from (`engine/gps/GpsAssist`,
  `engine/gps/logic`, tested): the phone's own copy when it is under 4 hours old; else the server
  (10 s wait); else a copy under 3 days old; else no answer (the core then replies "timed out"). After
  each sync, copies of 4 hours or more are refreshed from the server so the watch's next ask is quick.
  A file the core refuses is dropped from the phone's copy. The page shows when satellite data last
  reached the watch. Copies live in the app's private `files/gps-assist/` (up to 8 paths).
- **Privacy.** Notification, music and reply content only passes through memory on its way to the
  watch: never logged (log lines say what happened, not what or from whom), never stored, never
  uploaded; the types that carry it print nothing of it (`toString`/`Debug`).
- **Manifest.** `VIBRATE`, `USE_FULL_SCREEN_INTENT`, the listener service (bound by the system, exported
  with `BIND_NOTIFICATION_LISTENER_SERVICE`), a `<queries>` entry for launcher apps (the left-out list),
  `FindPhoneActivity` and `FindPhoneReceiver` (not exported).

**To try on the watch** (each line is one of the owner's checks; none is verified yet):

1. Phone features: all switches are off; watch behaviour and sync are as before.
2. Switch Notifications on, grant Notification access: the watch reconnects once; send yourself a
   message: it shows on the watch with title and text; dismiss it on the watch: it goes from the shade.
3. Leave one app out: its notifications stay on the phone only.
4. Switch Music on, play something: title, artist and state show on the watch; play/pause, next,
   previous and volume buttons on the watch act on the phone.
5. Switch Find my phone on, use find-phone on the watch (also with the phone silent and locked): it
   rings, vibrates and shows "Found it"; cancelling on the watch stops it.
6. "Make the watch ring" on the Phone features page rings the watch and stops it.
7. Switch everything off again: the watch reconnects once and sends nothing more.

**To try GPS assist** (needs a server with `NIMRAG_GPS_ASSIST` not `off`, the phone signed in to it,
and the watch connected; none of this is verified yet):

1. GPS assist is off after install; the Phone features page says "Off", and the log shows no web
   request activity from the watch.
2. Switch it on: the watch reconnects once (the Share log gets "GPS assist: on"). The page says
   "On · the watch finds GPS faster" and "Last sent to the watch: Not yet".
3. Wait a few minutes with the watch connected, or open an outdoor activity so it looks for GPS. Then
   open Share log and look for one of:
   - "GPS assist: the watch asked for satellite data, answered from the server" followed by
     "the satellite data reached the watch" (or "the watch already had the current satellite data"):
     the Instinct 2 asks in the clear and the whole chain works. The page now shows "Last sent: just now".
     Run the activity outdoors from a cold start and compare the time to the first fix with GPS assist
     off (switch it off, reconnect, repeat on another day).
   - "Refused a web request from the watch: a request it sent encrypted": the watch asks, but its
     request is encrypted for Garmin (Q-COMP-2); a local proxy cannot read it, so GPS assist cannot work
     on this watch this way. Not a bug in the phone. Note the lines (they carry no query) and switch the
     feature off.
   - "Refused a web request from the watch: <host><path>" for a host that is not `api.gcs.garmin.com`:
     the watch asks for something else; nothing was forwarded. The line shows what (path cleaned,
     no query), which tells us what else it wants.
   - "the watch asked for the file in a way this version does not do yet (file access)": the watch wants
     the file through the multi-link file-access path, which is not built; report it.
   - "there is none to give": the server could not be reached (check "Test the server" in Settings)
     and the phone has no copy under 3 days old.
4. On the server: `nimrag-server` logs "gps-assist: fetched N bytes from upstream" once per 4 hours at
   most; with `NIMRAG_GPS_ASSIST=off` the endpoint answers 404 and the phone logs the "none to give" line.
5. Switch GPS assist off again: the watch reconnects once and the Share log shows no more satellite lines.

## Weather (stage 2f, built, live test pending)

The Phone features page also has a switch for weather, **Weather on the watch, off by default**, with one
sentence under it: your Nimrag server gets the forecast from Open-Meteo for your area, rounded to
about 10 km. Like the rest of 2f it is ported from Gadgetbridge and *inferred (Gadgetbridge)* until
the watch has shown a forecast from us; only the unit tests (core byte-exact frames through
`Client::handle`, ffi conversions, the server tests with a fake Open-Meteo, the Kotlin rules) are
verified.

- **Flow.** The watch's weather widget sends 5014 with its position. The protocol client ACKs it,
  rounds the position to 0.1 degree and raises `Event.WeatherRequest`; `engine/phone/WeatherBridge.kt`
  asks the server (`POST /api/device/weather`, device token, rules and tests in
  `engine/phone/logic/WeatherRules.kt`), and gives the answer to the client (`Input.SendWeather`),
  which sends it as 5011 (definitions) and 5012 (data) and reports `Event.WeatherSent`. **The server
  writes the FIT bytes** (it needs the locally generated FIT profile); the phone only carries them,
  so the public build contains no profile table and the string check in "No FIT profile on Android"
  still finds nothing. A server without the profile, or with `NIMRAG_WEATHER=off`, answers 503 and
  the watch simply gets no weather.
- **Claiming it.** `PhoneDuties.kt` claims capability bit 26 (`WEATHER_CONDITIONS`) only when the
  switch is on (bit 27, alerts, never), and passes `Duties.weather` to the client. Changing the
  switch reconnects the watch once, like the other duties. Weather needs no Notification access.
- **When the server cannot be reached** (no server set, offline, an error): the last answer for the
  same cell is sent again if it is under 3 hours old; otherwise nothing more is sent (the request was
  already acknowledged), which the watch shows as no weather. The page says which of these happened.
  The last answer is kept in memory only; switching Weather off or restarting the app forgets it.
- **The page** shows when the watch last took a forecast ("On · last sent 5 min ago", with "an
  earlier forecast" when it was the 3-hour fallback) and what went wrong last (no forecast, no
  position, the watch did not take it). It is held in memory, so it starts empty after a restart.
- **Network rule.** Unlike file uploads this does not wait for Wi-Fi: a request is a few hundred
  bytes out and a few kilobytes back, and weather is mostly wanted away from home.
- **Privacy.** The position is never written to settings, the database or the log (not even
  rounded); log lines say what happened, not where, and the types that hold it print nothing.
  Open-Meteo only sees the rounded cell and the server's address. Attribution (CC BY 4.0) is in
  Settings, About, Open-source licences, and in NOTICE and the READMEs.

**To try on the watch** (none is verified yet). Needs a server built with the FIT profile and
reachable from the phone (the Docker image has none, so it answers 503):

1. Weather is off after a fresh install and nothing about weather changes on the watch.
2. Switch Weather on: the watch reconnects once. Open the weather widget on the watch (or pull the
   glance) with the phone connected: after a few seconds it shows the current temperature and
   condition, hourly and the next days.
3. In the app's log (the Share log button), a good request reads "Weather: the watch took the forecast";
   the page's row says "On · last sent just now". The log never shows a position.
4. Stop the server (or put the phone in flight mode with Bluetooth on) and open the widget again:
   within 3 hours of a good answer for the same area the log says "sending the earlier forecast for
   this area" and the row says "an earlier forecast"; with none, "no forecast for the watch ...
   nothing sent" and the widget shows no weather.
5. If the widget never asks: the log has no "Weather" line at all (check the watch has the weather
   widget on and that bit 26 reached the watch: reconnect once). If the log says "the watch did not
   take the forecast", note the reason (refused, did not answer in time) and the server log's
   `weather answered` line; that is the capture for OPEN-QUESTIONS Q-COMP-3. Also check the units
   (the temperature is whole degrees Celsius) and that the place is right (the coordinates are read
   as FIT semicircles; a forecast for the wrong place means that reading is wrong).
6. Switch Weather off again: the watch reconnects once and the widget stops asking.

## Workouts (stage 4a, built, live test pending)

A workout built on the web and sent with "Send to watch" waits on the server; the phone carries it
to the watch. Nothing here needs the FIT profile: the server writes the file, the phone only moves
the bytes, so the `public` flavor does this as it is (and `fitProfileCompiledIn()` stays false).
The upload protocol is ported from Gadgetbridge and *inferred (Gadgetbridge)* until it has been tried
on the owner's Instinct 2; only the unit tests (core byte-exact frames through `Client::handle`, the
ffi conversions, the Kotlin rules and API client) are verified.

- **When.** `engine/workouts/WorkoutSender` looks at the server's queue (`GET /api/device/workouts`,
  Bearer token) as soon as the handshake with the watch is done, then every minute while the link is
  up, and again 2 s after an upload ends. At most four workouts are fetched per look; one that is
  already on its way is not fetched twice.
- **How.** Each queued workout is fetched as a FIT file (`/fit`, at most 256 KiB), handed to the
  protocol client as `Input.UploadFile` (file type 128, sub type 5) through `WatchLink.sendFile`, and
  the outcome is reported (`/result`): `sent`, or `failed` with a fixed sentence that says which stage
  the watch refused and with which status name (never the workout's content). The owner sees it on
  the workout's page on the web.
- **Link trouble.** If the link closes before the upload ends (`UploadResult.Aborted`) nothing is
  reported: the workout stays queued and goes again at the next connection. A refusal, a NAK, an
  answer that makes no sense and 30 s of silence are reported as failed; none of them closes the
  session or stops a sync.
- **Network rule.** The same as the file upload: Wi-Fi, Ethernet or VPN, or mobile data when "Allow
  mobile data" is on (`UploadWorker.networkAllowsUploads`). With no server URL or token nothing is asked.
- **Privacy.** Workout names are never logged, only the workout's number. The token only goes into
  the `Authorization` header.
- **Order with syncing.** Uploads run one at a time and only when no sync cycle is running; a sync the
  watch asks for while an upload runs waits until the queue is empty (`core::client::upload`).
- **Not done.** A report that cannot reach the server is retried at the next look, but only in memory:
  if the app is killed first the server still shows the workout as queued and the phone sends it
  again, which the watch may answer with DUPLICATE (OPEN-QUESTIONS Q-UPLOAD-4).

**To try on the watch** (each line is one of the owner's checks; none is verified yet):

1. On the web, Strength > Workouts > New workout: name it "Test", keep the default warm-up step
   (10:00) and press "Save and send to watch". The page says it is waiting for the phone.
   (If it says this server was built without the FIT profile, the server cannot write the file:
   use the build that has the profile; workouts can still be saved.)
2. With the watch connected to the phone (the Watch tab shows it linked), wait up to a minute. The
   web page changes to "Sent to the watch ... ago", or shows why the watch refused. In the app's log
   (the Share log button) look for "Workout N sent to the watch" or "could not be sent" with the stage.
3. On the watch open its workouts list (the Training or Workouts entry): "Test" is there. If it is not, sync
   once (Watch tab) and look again, and note whether that was needed (Q-UPLOAD-3).
4. Start it on the watch: a 10 minute warm-up step, with the note if you gave one.
5. Build one with a repeat block (4 x 400 m with a 90 s rest), a heart-rate zone, a pace range and a
   cadence range; send it. Check each step on the watch shows the end and the target as typed
   (Q-WORKOUT-3).
6. Edit "Test" and send it again: the watch should have one workout of that name, or two if it keeps
   them apart; note which, and any "DUPLICATE" or other failure text on the web page (Q-UPLOAD-4).
7. Switch the watch's Bluetooth off, press "Send to watch" and wait: nothing fails; switch it on again
   and it goes within a minute of the connection coming back. Press "Cancel sending" before it does:
   it is not sent.
8. Sync still works afterwards (a new activity or the usual 15 minute sync arrives on the server as before).

## New data soon (stage 3q, 0.5.1)

The auto-sync interval (default 15 minutes, Settings) is kept by an inexact alarm that is allowed in
Doze (`AlarmManager.setAndAllowWhileIdle`, `ELAPSED_REALTIME_WAKEUP`; no exact-alarm permission), not
by a `Handler` delay: Handler time stops in deep sleep, which stretched an hour to many.
`AutoSyncReceiver` hands the alarm to `WatchLink.onAutoSyncAlarm()`, which syncs when due, hurries a
waiting reconnect and sets the next alarm. The same due-check (`LinkRules.shouldStartAutoSync`) also
runs after the handshake and after any watch event. Opening the app starts a sync when the link is
ready and the last finished sync is older than 5 minutes (`LinkRules.shouldSyncOnOpen`, not when
auto-sync is Off). After an upload pass took a file, the shell dispatches `nimrag:data` in the web
view (`AppState.dataVersion`) and the page fetches again (`web/src/dataRefresh.ts`).

## Signing in (stage 3p)

The only way to connect the app to a server is to sign in: server address, username, password and
the 6-digit code from the authenticator app (`POST /api/app/login`, see `server/README.md`). There
is no device token to create, copy or paste: the server issues the phone's upload token in that
call and the app keeps it. Normal user accounts only (an admin account is refused, and so is an
account that still has a temporary password: finish that in a browser first). Phones that had a
token entered by hand keep uploading with it until it stops working; the app then shows the
sign-in. A refused upload (`401`/`403`) always ends there: credentials wiped, sign in again.

What is stored where (code in `engine/auth`, pure rules and the HTTP calls in `engine/auth/logic`
with JVM tests):

- the **upload token** is in `EngineSettings` (private preferences, never in `AppState`, never
  logged); the upload worker uses it, so uploads keep going while the session is locked;
- the **device secret** (`nrd_...`) only exists encrypted, with Android Keystore keys, in two
  copies. The *fingerprint copy* is RSA-OAEP under a key pair that needs a `BIOMETRIC_STRONG`
  authentication for every use (`setUserAuthenticationRequired`, timeout 0, invalidated when a
  fingerprint is enrolled; the public half encrypts without a prompt). The *password copy* is
  AES-GCM under a Keystore key with no user authentication, read only to be sent together with the
  typed password to `/api/app/unlock`. Without an enrolled fingerprint only the password copy
  exists, and unlock is by password. Trade-off, stated plainly: someone who can run code as the
  app (root) can read the password copy and call `/api/app/resume`; the fingerprint copy only
  protects against using the unlocked phone without the fingerprint and without the password. The
  password and the 6-digit code never leave the sign-in screen's memory (not saved instance state);
- the **session cookie** is put into the web view's `CookieManager` jar for the server origin (rebuilt
  from `Set-Cookie` with our own attributes, `Secure` for the `__Host-` name) before the web view
  loads, so the web app opens signed in. It is read back from the jar only to call sign-out and the
  cheap `GET /api/me` check; it is never logged.

Later launches: the app asks `GET /api/me` (cheap, throttled to every 30 s on resume). A live session
shows the web app. No session on a phone that holds a secret shows the native **unlock** screen:
"Use fingerprint" (BiometricPrompt, then `POST /api/app/resume`) or the password (`POST
/api/app/unlock`); on success the new cookie goes in and the web view reloads. A `401` on resume
means the server revoked this phone: credentials are wiped and the full sign-in shows. A `401` on
password unlock is ambiguous (wrong password, or removed) and says so; "Use a different account"
signs out. Inside the app the web page never shows its own login page: a `401` calls
`NimragApp.sessionExpired()` (see `web/src/native.ts`) and the app takes over. Offline at launch the
app keeps showing the cached pages. Sign out (Watch tab, Settings, "Account and server") revokes the
phone's token on the server (when it can reach it), then clears the cookie, the web view's storage
and cache, the token and the secret. The phone's own copy (the local server, both flavors) needs no account.

Failure messages are fixed sentences: a wrong username, password or code is one answer, `429`
shows the wait from `Retry-After`, and the server's own text is never shown.
