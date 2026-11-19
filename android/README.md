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
./gradlew :core:assembleRelease :app:assembleDebug :hosttest:test
```

Pinning: Gradle wrapper with `distributionSha256Sum`, every Gradle dependency in
`gradle/verification-metadata.xml` (sha256), `cargo --locked` everywhere, ABIs
limited to arm64-v8a and x86_64. The Rust library is built with
`--remap-path-prefix`, and the build fails if the `.so` contains a `/home/` path.

## No FIT profile on Android (licence)

The locally generated FIT profile is Garmin data and is never redistributed.
Every cargo call from Gradle (`buildSrc/src/main/kotlin/CargoTasks.kt`) sets
`NIMRAG_FIT_PROFILE=none`, then reads cargo's build-script output and fails the
build if core's `cfg(fit_profile)` is on. `core/build.rs` and `ffi/build.rs` also
refuse any Android target with a profile (or without the variable). Finally the
tasks grep both finished `.so` files: a marker string that core emits only when a
profile is compiled in, the test-fixtures `ScriptedWatch`, and build-machine paths
must all be absent. The app also checks `fitProfileCompiledIn()` at start. The FFI
does not expose FIT parsing.

## Dependencies

| Component | Version | Licence | Used for |
|---|---|---|---|
| Gradle | 9.8.2 (2026-10-19) | Apache-2.0 | build |
| Android Gradle Plugin | 9.5.0 (2026-10-28) | Apache-2.0 | `:core`, `:app` |
| Kotlin Gradle plugin (JVM) | 2.4.20 (2026-10-08) | Apache-2.0 | `:hosttest` (AGP 9 has built-in Kotlin for the Android modules) |
| JNA | 5.19.1 (2026-06-12) | Apache-2.0 OR LGPL-2.1-or-later | UniFFI's Kotlin loads the library through it (aar on Android, jar in `:hosttest`) |
| JUnit Jupiter / Platform Launcher | 6.1.3 (2026-08-07) | EPL-2.0 | `:hosttest` only; not shipped. EPL-2.0 is a test-time dependency, never linked into the app |
| UniFFI (Rust) | 0.32.2 | MPL-2.0 | see `ffi/README.md` |

No AndroidX or Compose in the app yet: the shell is a plain `Activity`.
