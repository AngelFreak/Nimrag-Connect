//! Licence guard: the Android library must never contain the locally generated
//! FIT profile (Garmin data, not redistributable). core's build.rs picks that
//! profile up from `NIMRAG_FIT_PROFILE` or a git-ignored default file, so for
//! Android targets the variable must be exactly `none`.
use std::env;

fn main() {
    println!("cargo::rerun-if-env-changed=NIMRAG_FIT_PROFILE");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android")
        && env::var("NIMRAG_FIT_PROFILE").as_deref() != Ok("none")
    {
        panic!(
            "Android builds must set NIMRAG_FIT_PROFILE=none (the FIT profile is Garmin data \
             and is never redistributed); got {:?}",
            env::var("NIMRAG_FIT_PROFILE")
        );
    }
}
