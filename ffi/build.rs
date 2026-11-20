//! Licence guard: the Android library must never contain the locally generated
//! FIT profile (Garmin data, not redistributable). core's build.rs picks that
//! profile up from `NIMRAG_FIT_PROFILE` or a default file that is not in the repository, so for
//! Android targets the variable must be exactly `none`, except for the owner's
//! personal build (Gradle flavor `personal`, never published), which says so with
//! `NIMRAG_ALLOW_ANDROID_FIT_PROFILE`. Keep the names in sync with core/build.rs.
use std::env;

const ALLOW_ANDROID_ENV: &str = "NIMRAG_ALLOW_ANDROID_FIT_PROFILE";
const ALLOW_ANDROID_VALUE: &str = "personal-apk-never-published";

fn main() {
    println!("cargo::rerun-if-env-changed=NIMRAG_FIT_PROFILE");
    println!("cargo::rerun-if-env-changed={ALLOW_ANDROID_ENV}");
    let personal_ok = env::var(ALLOW_ANDROID_ENV).as_deref() == Ok(ALLOW_ANDROID_VALUE);
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android")
        && env::var("NIMRAG_FIT_PROFILE").as_deref() != Ok("none")
        && !personal_ok
    {
        panic!(
            "Android builds must set NIMRAG_FIT_PROFILE=none (the FIT profile is Garmin data \
             and is never redistributed); got {:?}",
            env::var("NIMRAG_FIT_PROFILE")
        );
    }
}
