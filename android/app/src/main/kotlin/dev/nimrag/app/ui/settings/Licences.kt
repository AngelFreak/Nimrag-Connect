package dev.nimrag.app.ui.settings

/** One bundled component: [licence] is an SPDX licence expression. */
data class Component(val name: String, val version: String, val licence: String)

/** This app itself. */
val NimragComponent = Component("Nimrag Connect", "", "AGPL-3.0-or-later")

/**
 * Libraries bundled in the APK besides the Rust library. Versions follow
 * android/gradle/libs.versions.toml (AndroidX and Compose are pulled in transitively, all
 * Apache-2.0). Written by hand; update when the catalogue changes.
 */
val BundledLibraries: List<Component> = listOf(
    Component("UniFFI (Kotlin bindings and runtime)", "0.32.2", "MPL-2.0"),
    Component("JNA", "5.19.1", "Apache-2.0 OR LGPL-2.1-or-later"),
    Component("AndroidX and Jetpack Compose (BOM 2026.09.00)", "", "Apache-2.0"),
    Component("AndroidX WebKit", "1.18.0", "Apache-2.0"),
    Component("kotlinx.coroutines", "1.11.0", "Apache-2.0"),
    Component("Kotlin standard library", "2.4.20", "Apache-2.0"),
)

/**
 * Fonts bundled in the APK (res/font, converted from the web fonts of @fontsource 5.3.0). Both are
 * under the SIL Open Font License 1.1; the licence texts ship in assets/licences/.
 */
val BundledFonts: List<Component> = listOf(
    Component("Inter (Copyright 2016 The Inter Project Authors)", "5.3.0", "OFL-1.1"),
    Component("Space Grotesk (Copyright 2020 The Space Grotesk Project Authors)", "5.3.0", "OFL-1.1"),
)

/**
 * Data the app's server fetches for features that are switched on. Nothing of it is bundled or
 * redistributed; the attribution is the licence's condition (also in NOTICE and the READMEs).
 */
val DataSources: List<Component> = listOf(
    Component("Weather data by Open-Meteo.com", "", "CC-BY-4.0"),
)

/**
 * The Rust crates linked into libnimrag_ffi.so. Generated once (not at build time) on 2026-11-19
 * from the repository root with:
 *
 *   cargo tree -p nimrag-ffi --target aarch64-linux-android -e normal --prefix none
 *   cargo metadata --format-version 1      (the `license` field of each crate in that list)
 *
 * The tree contains every normal (non-dev, non-build) dependency of the target, including
 * proc-macro crates, which are compile-time only, so this list is a superset of what is
 * linked. Re-run both commands when Cargo.lock changes.
 */
val RustCrates: List<Component> = listOf(
    Component("anyhow", "1.0.104", "MIT OR Apache-2.0"),
    Component("bitflags", "2.13.2", "MIT OR Apache-2.0"),
    Component("bytes", "1.12.2", "MIT"),
    Component("camino", "1.2.6", "MIT OR Apache-2.0"),
    Component("cargo-platform", "0.3.3", "MIT OR Apache-2.0"),
    Component("cargo_metadata", "0.23.1", "MIT"),
    Component("cfg-if", "1.0.5", "MIT OR Apache-2.0"),
    Component("equivalent", "1.0.3", "Apache-2.0 OR MIT"),
    Component("errno", "0.3.15", "MIT OR Apache-2.0"),
    Component("fastrand", "2.5.0", "Apache-2.0 OR MIT"),
    Component("fs-err", "3.3.2", "MIT OR Apache-2.0"),
    Component("getrandom", "0.4.3", "MIT OR Apache-2.0"),
    Component("hashbrown", "0.17.1", "MIT OR Apache-2.0"),
    Component("heck", "0.5.0", "MIT OR Apache-2.0"),
    Component("indexmap", "2.14.2", "Apache-2.0 OR MIT"),
    Component("itoa", "1.0.19", "MIT OR Apache-2.0"),
    Component("libc", "0.2.191", "MIT OR Apache-2.0"),
    Component("linux-raw-sys", "0.12.1", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT"),
    Component("memchr", "2.8.3", "Unlicense OR MIT"),
    Component("nimrag-core", "0.1.0", "AGPL-3.0-or-later"),
    Component("nimrag-ffi", "0.1.0", "AGPL-3.0-or-later"),
    Component("once_cell", "1.21.4", "MIT OR Apache-2.0"),
    Component("proc-macro2", "1.0.107", "MIT OR Apache-2.0"),
    Component("quote", "1.0.47", "MIT OR Apache-2.0"),
    Component("rustix", "1.1.6", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT"),
    Component("semver", "1.0.28", "MIT OR Apache-2.0"),
    Component("serde", "1.0.229", "MIT OR Apache-2.0"),
    Component("serde_core", "1.0.229", "MIT OR Apache-2.0"),
    Component("serde_derive", "1.0.229", "MIT OR Apache-2.0"),
    Component("serde_json", "1.0.154", "MIT OR Apache-2.0"),
    Component("serde_spanned", "1.1.3", "MIT OR Apache-2.0"),
    Component("siphasher", "1.0.4", "MIT OR Apache-2.0"),
    Component("static_assertions", "1.1.0", "MIT OR Apache-2.0"),
    Component("syn", "2.0.119", "MIT OR Apache-2.0"),
    Component("syn", "3.0.10", "MIT OR Apache-2.0"),
    Component("tempfile", "3.27.0", "MIT OR Apache-2.0"),
    Component("thiserror", "2.0.21", "MIT OR Apache-2.0"),
    Component("thiserror-impl", "2.0.21", "MIT OR Apache-2.0"),
    Component("toml", "1.2.1+spec-1.1.0", "MIT OR Apache-2.0"),
    Component("toml_datetime", "1.1.3+spec-1.1.0", "MIT OR Apache-2.0"),
    Component("toml_parser", "1.1.5+spec-1.1.0", "MIT OR Apache-2.0"),
    Component("toml_writer", "1.2.0+spec-1.1.0", "MIT OR Apache-2.0"),
    Component("unicode-ident", "1.0.27", "(MIT OR Apache-2.0) AND Unicode-3.0"),
    Component("uniffi", "0.32.2", "MPL-2.0"),
    Component("uniffi_core", "0.32.2", "MPL-2.0"),
    Component("uniffi_internal_macros", "0.32.2", "MPL-2.0"),
    Component("uniffi_macros", "0.32.2", "MPL-2.0"),
    Component("uniffi_meta", "0.32.2", "MPL-2.0"),
    Component("uniffi_pipeline", "0.32.2", "MPL-2.0"),
    Component("winnow", "1.0.5", "MIT"),
    Component("zmij", "1.0.23", "MIT"),
)
