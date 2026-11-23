# nimrag-ffi

UniFFI (proc-macro, no UDL) bindings over `nimrag_core::client`, for the Android
app. `core` stays std-only; this crate is the only place `uniffi` appears.

## API

- `NimragClient(config)`: one per GATT connection (an `Arc<Mutex<Client>>`).
  `handle(now, input) -> Step` and `phase()`.
- Records and enums mirror the core types one to one: `ClientConfig`, `Now`,
  `Input`, `Step`, `Event`, `DeviceInfo`, `FileType`, `DirEntry`, `SyncTrigger`,
  `FileFailure`, `AnomalyKind`, `Phase`, `CloseReason`. Conversions are in
  `src/convert.rs`, exhaustive matches only, so a new core variant fails to
  compile here.
- `ffi_version()`, `fit_profile_compiled_in()` (true only for Garmin's generated profile; Nimrag's own field map does not count).
- FIT parsing is not exposed.
- Feature `local-server` (personal Android flavor only): `LocalServer.start(dataDir, port, secret)`,
  `ingestFile(bytes, meta)`, `summary()`, `stop()`: the nimrag server inside the app
  (`nimrag_server::embedded`). Off by default; the public library has none of it.

## Licence rule: no Garmin FIT profile on Android

The locally generated FIT profile is Garmin data and is never redistributed.
`NIMRAG_FIT_PROFILE=none` selects Nimrag's own clean-room field map instead
(`core/src/fit/profile_nimrag.rs`, `cfg(fit_map)`), which may be published and is what
the public library contains. `build.rs` panics for any `android` target unless `NIMRAG_FIT_PROFILE=none` (core's does the same for any Garmin profile), except when
`NIMRAG_ALLOW_ANDROID_FIT_PROFILE=personal-apk-never-published` is set (the owner's personal flavor, never published); the
Gradle tasks in `android/buildSrc` set it for every cargo call and then read
cargo's build-script output to prove core's `cfg(fit_profile)` is off.

## Features

- `bindgen`: builds the `uniffi-bindgen` binary (host tool, used by Gradle).
- `test-fixtures`: `ScriptedWatch`, a fake watch (bring-up and handshake, fake
  ids) for host tests through the real bindings. Enabled for `cargo test` through
  a self dev-dependency and for the Gradle host test library, never for the
  Android `.so` (`compile_error!` on Android, and the Gradle task greps the `.so`).

## Tests

`cargo test -p nimrag-ffi`: conversion tests, plus the real client driven through
the FFI object (close-all write, scripted handshake to `HandshakeDone`, stale
`FileStored`). The same three scenarios run from Kotlin in `android/hosttest`.

## Dependencies

| Crate | Version | Licence | Why |
|---|---|---|---|
| nimrag-core | path | AGPL-3.0-or-later | the client |
| uniffi | =0.32.2 (2026-09-23) | MPL-2.0 | proc-macro bindings, scaffolding |
| uniffi (feature `cli`, only with `bindgen`) | same | MPL-2.0 | Kotlin generator; pulls askama (MIT/Apache-2.0), goblin (MIT), clap, cargo_metadata, toml (all MIT or Apache-2.0) |

Transitive crates of the library build are MIT, Apache-2.0 or MPL-2.0 (and
Unlicense/Zlib/Unicode-3.0 for a few small ones); none is GPL-only.
