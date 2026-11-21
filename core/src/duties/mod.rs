//! Message layouts of the phone-side companion duties of stage 2f:
//! notifications (5033-5036), find my phone / find my watch (5039, 5040 and the
//! `find_my_watch` protobuf service) and music control (5041, 5042, 5049).
//!
//! Only layouts live here: pure build and parse functions that never panic on
//! peer data. The client state around them (which notification is known, which
//! chunk is next, what to answer) is in `client/companion.rs`.
//!
//! Everything in this module is ported from Gadgetbridge commit 7257742b
//! (AGPL-3.0-or-later) and is *inferred (Gadgetbridge)* until a test on a real
//! watch confirms it. See docs/PORTED.md and docs/LICENSING.md. The individual
//! files name their Gadgetbridge sources.

pub mod find;
pub mod music;
pub mod notification;

/// `s` cut to at most `max` bytes, never inside a character. `max == 0` means
/// "no limit" (what the watch sends when it has none).
pub fn truncate_utf8(s: &str, max: usize) -> &str {
    if max == 0 || s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_respects_character_boundaries() {
        assert_eq!(truncate_utf8("hello", 0), "hello");
        assert_eq!(truncate_utf8("hello", 3), "hel");
        assert_eq!(truncate_utf8("hello", 10), "hello");
        // "é" is two bytes: cutting at 1 byte leaves nothing of it.
        assert_eq!(truncate_utf8("é", 1), "");
        assert_eq!(truncate_utf8("aé", 2), "a");
        assert_eq!(truncate_utf8("aé", 3), "aé");
    }
}
