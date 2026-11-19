//! Selects the FIT profile table: a locally generated, git-ignored file
//! (`src/fit/generated/profile_data.rs`, or the path in `NIMRAG_FIT_PROFILE`) if
//! present, else the committed empty table. Sets `cfg(fit_profile)` only for the
//! former. See tools/fit-profile/README.md.
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// Also searched for by android/buildSrc (CargoTasks.kt); keep in sync.
const FIT_PROFILE_MARKER: &str = "NIMRAG-FIT-PROFILE-PRESENT-MARKER-7f3a9c";

fn main() {
    println!("cargo::rustc-check-cfg=cfg(fit_profile)");
    println!("cargo::rerun-if-env-changed=NIMRAG_FIT_PROFILE");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=src/fit/profile_empty.rs");

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let default = manifest.join("src/fit/generated/profile_data.rs");
    println!("cargo::rerun-if-changed={}", default.display());

    let chosen = match env::var_os("NIMRAG_FIT_PROFILE") {
        // `none` forces the committed empty profile even when a generated one exists.
        Some(p) if p == "none" => None,
        Some(p) if !p.is_empty() => {
            let p = PathBuf::from(p);
            println!("cargo::rerun-if-changed={}", p.display());
            Some(p)
        }
        _ => default.is_file().then_some(default),
    };

    // Licence guard: a generated profile (Garmin data) must never reach an
    // Android build. The Android Gradle tasks also set NIMRAG_FIT_PROFILE=none.
    if chosen.is_some() && env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
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
        None => manifest.join("src/fit/profile_empty.rs"),
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
