//! Selects the FIT profile table: a locally generated, git-ignored file
//! (`src/fit/generated/profile_data.rs`, or the path in `NIMRAG_FIT_PROFILE`) if
//! present, else the committed empty table. Sets `cfg(fit_profile)` only for the
//! former. See tools/fit-profile/README.md.
use std::env;
use std::fs;
use std::path::PathBuf;

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

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("profile_selected.rs");
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
}
