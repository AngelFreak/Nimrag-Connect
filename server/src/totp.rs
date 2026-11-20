//! Two-factor codes: RFC 6238 TOTP (HMAC-SHA1, 6 digits, 30 s steps, one step of skew either
//! way) on top of the `totp-rs` crate, which does the HMAC and the dynamic truncation.
//!
//! Secrets are 160 random bits stored base32 (the form authenticator apps import). A code is
//! only accepted for a step newer than the last accepted one ([`check`] returns the step, the
//! caller stores it with a compare-and-set), so a code cannot be replayed within its window.
//!
//! Nothing here logs a secret or a code.

use anyhow::{anyhow, Result};
use totp_rs::{Builder, Secret, Totp};

/// What authenticator apps show as the account's issuer.
pub const ISSUER: &str = "Nimrag Connect";
pub const DIGITS: usize = 6;
pub const STEP_SECS: i64 = 30;

/// A new random secret, base32 (RFC 4648, no padding needed for 20 bytes).
pub fn new_secret() -> Result<String> {
    let mut raw = [0u8; 20];
    getrandom::fill(&mut raw).map_err(|e| anyhow!("no randomness: {e}"))?;
    Ok(Secret::from(raw).to_base32())
}

/// `otpauth://` URI for the authenticator app (and the QR code the web app draws from it).
/// Usernames are `[a-z0-9._-]`, so only the issuer needs escaping.
pub fn otpauth_uri(secret_b32: &str, username: &str) -> String {
    let issuer = ISSUER.replace(' ', "%20");
    format!(
        "otpauth://totp/{issuer}:{username}?secret={secret_b32}&issuer={issuer}&algorithm=SHA1&digits={DIGITS}&period={STEP_SECS}"
    )
}

fn totp(secret_b32: &str) -> Option<Totp> {
    let secret = Secret::try_from_base32(secret_b32).ok()?;
    Builder::new().with_secret(secret).build().ok()
}

/// The step number (`unix time / 30`) `code` is valid for at `now`, within one step either way.
/// `None` for a wrong or malformed code, a bad secret, or a step that is not newer than
/// `last_step` (replay).
pub fn check(secret_b32: &str, code: &str, now: i64, last_step: i64) -> Option<i64> {
    let code = code.trim();
    if code.len() != DIGITS || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let step = i64::try_from(totp(secret_b32)?.check(code, u64::try_from(now).ok()?)?).ok()?;
    (step > last_step).then_some(step)
}

/// The code for the step containing `now` (tests, `dev-seed` and the e2e seed).
pub fn code_at(secret_b32: &str, now: i64) -> Option<String> {
    let t = totp(secret_b32)?;
    Some(t.generate(u64::try_from(now).ok()?).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 6238 appendix B, SHA-1, secret "12345678901234567890", 8 digits there; the same HMAC
    // truncated to 6 digits gives the last six digits of each value.
    const RFC_SECRET_B32: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    #[test]
    fn rfc6238_test_vectors() {
        for (time, code) in [
            (59, "287082"),
            (1_111_111_109, "081804"),
            (1_111_111_111, "050471"),
            (1_234_567_890, "005924"),
            (2_000_000_000, "279037"),
        ] {
            assert_eq!(code_at(RFC_SECRET_B32, time).unwrap(), code, "t={time}");
        }
    }

    #[test]
    fn accepts_one_step_of_skew_and_reports_the_step() {
        let now = 1_700_000_000;
        let step = now / STEP_SECS;
        let code = |s: i64| code_at(RFC_SECRET_B32, s * STEP_SECS).unwrap();
        assert_eq!(check(RFC_SECRET_B32, &code(step), now, 0), Some(step));
        assert_eq!(
            check(RFC_SECRET_B32, &code(step - 1), now, 0),
            Some(step - 1)
        );
        assert_eq!(
            check(RFC_SECRET_B32, &code(step + 1), now, 0),
            Some(step + 1)
        );
        assert_eq!(check(RFC_SECRET_B32, &code(step - 2), now, 0), None);
        assert_eq!(check(RFC_SECRET_B32, &code(step + 2), now, 0), None);
    }

    #[test]
    fn a_step_is_never_accepted_twice() {
        let now = 1_700_000_000;
        let step = now / STEP_SECS;
        let code = code_at(RFC_SECRET_B32, now).unwrap();
        assert_eq!(check(RFC_SECRET_B32, &code, now, step - 1), Some(step));
        assert_eq!(check(RFC_SECRET_B32, &code, now, step), None);
        // An older code is refused once a newer step was used.
        let older = code_at(RFC_SECRET_B32, now - STEP_SECS).unwrap();
        assert_eq!(check(RFC_SECRET_B32, &older, now, step), None);
    }

    #[test]
    fn rejects_malformed_codes_and_secrets() {
        let now = 1_700_000_000;
        for bad in ["", "12345", "1234567", "12345a", "١٢٣٤٥٦"] {
            assert_eq!(check(RFC_SECRET_B32, bad, now, 0), None, "{bad:?}");
        }
        assert_eq!(check("not base32!", "123456", now, 0), None);
        assert_eq!(check(RFC_SECRET_B32, "123456", -5, 0), None);
    }

    #[test]
    fn new_secrets_are_random_base32_and_work() {
        let a = new_secret().unwrap();
        let b = new_secret().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
        assert!(a
            .bytes()
            .all(|c| c.is_ascii_uppercase() || (b'2'..=b'7').contains(&c)));
        let now = 1_700_000_000;
        let code = code_at(&a, now).unwrap();
        assert!(check(&a, &code, now, 0).is_some());
    }

    #[test]
    fn otpauth_uri_carries_everything_an_app_needs() {
        let uri = otpauth_uri("ABCDEFGH", "anna.k");
        assert_eq!(
            uri,
            "otpauth://totp/Nimrag%20Connect:anna.k?secret=ABCDEFGH&issuer=Nimrag%20Connect&algorithm=SHA1&digits=6&period=30"
        );
    }
}
