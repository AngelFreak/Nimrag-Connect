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
owner's own build (see "Personal flavor" below). Task names carry the flavor, for example
`:app:testPublicDebugUnitTest`.

Pinning: Gradle wrapper with `distributionSha256Sum`, every Gradle dependency in
`gradle/verification-metadata.xml` (sha256), `cargo --locked` everywhere, ABIs
limited to arm64-v8a and x86_64. The Rust library is built with
`--remap-path-prefix`, and the build fails if the `.so` contains a `/home/` path.

## No FIT profile on Android (licence)

This section is about the `public` flavor. The `personal` flavor is the one deliberate exception.

The locally generated FIT profile is Garmin data and is never redistributed.
Every cargo call from Gradle (`buildSrc/src/main/kotlin/CargoTasks.kt`) sets
`NIMRAG_FIT_PROFILE=none`, then reads cargo's build-script output and fails the
build if core's `cfg(fit_profile)` is on. `core/build.rs` and `ffi/build.rs` also
refuse any Android target with a profile (or without the variable). Finally the
tasks grep both finished `.so` files: a marker string that core emits only when a
profile is compiled in, the test-fixtures `ScriptedWatch`, and build-machine paths
must all be absent. The app also checks `fitProfileCompiledIn()` at start (it must be false in
`public`, true in `personal`). The `public` FFI does not expose FIT parsing.

## Personal flavor (never published)

The owner's own build: the FIT profile is compiled in and the server runs inside the app
(cargo feature `local-server` of `nimrag-ffi`: `nimrag_server::embedded`, with the web app embedded
by rust-embed). Every stored watch file is ingested into that local server too, so Today, Activities,
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

The app is a native bottom bar (Today, Activities, Health, Strength, Watch). The first four show one
`WebView` (`ui/shell/ServerWebView.kt`) that loads the saved server address and keeps running while
the Watch tab (one native page and its sub-pages) is on screen. The web page talks to the
app through `window.NimragApp` (document-start script, server origin only) over the
`NimragAppNative` JavaScript interface; the pure rules (origin checks, URL validation, route to tab,
status JSON) live in `ui/shell/logic` with unit tests. See IMPLEMENTATION_PLAN.md, stage 3n.

The Watch tab is one page (`ui/watch/WatchScreen.kt`: a problem banner, the status card or the setup
checklist, then settings rows in three groups) and sub-pages opened from its rows; `WatchPage` names
them and gives each its parent, the shell keeps the current one (it survives rotation and the system
back button goes to the parent). The pure rules (which banner, the status word, the next pairing step)
live in `ui/watch/logic` with unit tests; the shared rows, group, banner and dialog are in
`ui/components`.

## Phone features (stage 2f, built, live test pending)

The Watch tab has a "Phone features" page with three switches, **all off by default**:
notifications on the watch, music control and find my phone, plus a "Find my watch" button.
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
- **Privacy.** Notification, music and reply content only passes through memory on its way to the
  watch: never logged (log lines say what happened, not what or from whom), never stored, never
  uploaded; the types that carry it print nothing of it (`toString`/`Debug`).
- **Manifest.** `VIBRATE`, `USE_FULL_SCREEN_INTENT`, the listener service (bound by the system, exported
  with `BIND_NOTIFICATION_LISTENER_SERVICE`), a `<queries>` entry for launcher apps (the left-out list),
  `FindPhoneActivity` and `FindPhoneReceiver` (not exported).

**To try on the watch** (each line is one of the owner's checks; none is verified yet):

1. Phone features: all three switches are off; watch behaviour and sync are as before.
2. Switch Notifications on, grant Notification access: the watch reconnects once; send yourself a
   message: it shows on the watch with title and text; dismiss it on the watch: it goes from the shade.
3. Leave one app out: its notifications stay on the phone only.
4. Switch Music on, play something: title, artist and state show on the watch; play/pause, next,
   previous and volume buttons on the watch act on the phone.
5. Switch Find my phone on, use find-phone on the watch (also with the phone silent and locked): it
   rings, vibrates and shows "Found it"; cancelling on the watch stops it.
6. "Make the watch ring" on the Phone features page rings the watch and stops it.
7. Switch everything off again: the watch reconnects once and sends nothing more.

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
and cache, the token and the secret. The phone's own copy (personal flavor) needs no account.

Failure messages are fixed sentences: a wrong username, password or code is one answer, `429`
shows the wait from `Retry-After`, and the server's own text is never shown.
