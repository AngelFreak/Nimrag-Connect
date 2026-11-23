//! Selects the FIT table that `core::fit::profile` compiles in.
//!
//! `NIMRAG_FIT_PROFILE`:
//! - unset or empty: the locally generated, git-ignored Garmin profile
//!   (`src/fit/generated/profile_data.rs`) if present, else Nimrag's own map;
//! - `none`: no Garmin profile, i.e. Nimrag's own committed map
//!   (`src/fit/profile_nimrag.rs`);
//! - `empty`: the committed empty table (every lookup answers None);
//! - anything else: the path of a generated Garmin profile file.
//!
//! `cfg(fit_profile)` (and the marker below) are set only for a Garmin profile,
//! which must never reach a published artifact. `cfg(fit_map)` is set when
//! Nimrag's own map is selected: it is clean-room data and may be published.
//! See tools/fit-profile/README.md and docs/LICENSING.md.
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Explicit opt-in for the owner's personal Android build (never published; Gradle flavor
/// `personal`). Keep the value in sync with ffi/build.rs and android/buildSrc.
const ALLOW_ANDROID_ENV: &str = "NIMRAG_ALLOW_ANDROID_FIT_PROFILE";
const ALLOW_ANDROID_VALUE: &str = "personal-apk-never-published";

/// Also searched for by android/buildSrc (CargoTasks.kt); keep in sync.
const FIT_PROFILE_MARKER: &str = "NIMRAG-FIT-PROFILE-PRESENT-MARKER-7f3a9c";

fn main() {
    println!("cargo::rustc-check-cfg=cfg(fit_profile)");
    println!("cargo::rustc-check-cfg=cfg(fit_map)");
    println!("cargo::rerun-if-env-changed=NIMRAG_FIT_PROFILE");
    println!("cargo::rerun-if-env-changed={ALLOW_ANDROID_ENV}");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=src/fit/profile_empty.rs");
    println!("cargo::rerun-if-changed=src/fit/profile_nimrag.rs");

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let default = manifest.join("src/fit/generated/profile_data.rs");
    println!("cargo::rerun-if-changed={}", default.display());

    // `Some(path)`: a Garmin profile file. `None`: no Garmin profile.
    let (chosen, empty) = match env::var_os("NIMRAG_FIT_PROFILE") {
        // `none` forces Nimrag's own map even when a generated profile exists.
        Some(p) if p == "none" => (None, false),
        // `empty` forces the committed empty table.
        Some(p) if p == "empty" => (None, true),
        Some(p) if !p.is_empty() => {
            let p = PathBuf::from(p);
            println!("cargo::rerun-if-changed={}", p.display());
            (Some(p), false)
        }
        _ => (default.is_file().then_some(default), false),
    };

    // Licence guard: a generated profile (Garmin data) must never reach an
    // Android build. The Android Gradle tasks also set NIMRAG_FIT_PROFILE=none. The only
    // way around is the explicit variable of the owner's personal build (flavor `personal`).
    let personal_ok = env::var(ALLOW_ANDROID_ENV).as_deref() == Ok(ALLOW_ANDROID_VALUE);
    if chosen.is_some()
        && env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android")
        && !personal_ok
    {
        panic!(
            "refusing to build core for Android with a FIT profile (Garmin data); \
             set NIMRAG_FIT_PROFILE=none"
        );
    }

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("profile_selected.rs");
    let on = chosen.is_some();
    let source = match chosen {
        Some(p) => {
            assert!(
                p.is_file(),
                "NIMRAG_FIT_PROFILE file not found: {}",
                p.display()
            );
            println!("cargo::rustc-cfg=fit_profile");
            p
        }
        None if empty => manifest.join("src/fit/profile_empty.rs"),
        None => {
            println!("cargo::rustc-cfg=fit_map");
            manifest.join("src/fit/profile_nimrag.rs")
        }
    };
    fs::copy(&source, &out)
        .unwrap_or_else(|e| panic!("copy {} -> {}: {e}", source.display(), out.display()));
    if on {
        // Marker that ends up in any binary containing the profile, so the
        // Android build can prove by grep that none does.
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&out)
            .unwrap_or_else(|e| panic!("open {}: {e}", out.display()));
        writeln!(
            f,
            "\n#[used]\n#[allow(dead_code)]\nstatic FIT_PROFILE_MARKER: [u8; {}] = *b\"{}\";",
            FIT_PROFILE_MARKER.len(),
            FIT_PROFILE_MARKER
        )
        .expect("append marker");
    }
}
