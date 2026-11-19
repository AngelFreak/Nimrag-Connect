//! Replays every Connect session of the private phase 0 captures against
//! `Client::handle` (stage 2a plan, section 5). Skips with a notice when
//! `private/captures` is absent; fails instead when `NIMRAG_REQUIRE_PRIVATE=1`.
//!
//! Nothing derived from the captures is committed: transcripts exist only in
//! memory, and only counts and allowed-difference names are printed.

mod support;

use std::path::PathBuf;

use support::transcript::{load_sessions, replay};

/// Capture and the number of Connect sessions it holds.
const CAPTURES: [(&str, usize); 3] = [
    ("2026-11-19-reconnect-sync", 1),
    ("2026-11-19-activity-sync", 2),
    ("2026-11-19-settings-timeformat", 2),
];

/// Connect's multi-link client id in all three captures (Explore uses a
/// random id and a different handshake; it is not our reference).
const CONNECT_CLIENT_ID: u64 = 1;

fn captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../private/captures")
}

#[test]
fn every_connect_session_replays_without_unexpected_differences() {
    let dir = captures_dir();
    let required = std::env::var("NIMRAG_REQUIRE_PRIVATE").is_ok_and(|v| v == "1");
    if !dir.is_dir() {
        assert!(
            !required,
            "NIMRAG_REQUIRE_PRIVATE=1 but {} is missing",
            dir.display()
        );
        println!("SKIP: no private captures at {}", dir.display());
        return;
    }
    let mut failed = Vec::new();
    let mut total = 0;
    for (name, expected_sessions) in CAPTURES {
        let path = dir.join(name).join("btsnoop_hci.log");
        // The directory exists, so every capture must be there and load.
        let bytes =
            std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let loaded = load_sessions(name, &bytes).expect("capture parses");
        for stray in &loaded.stray {
            println!("    UNCLASSIFIED: {stray}");
            failed.push(format!("{name}: {stray}"));
        }
        let connect: Vec<_> = loaded
            .sessions
            .iter()
            .filter(|s| s.client_id == CONNECT_CLIENT_ID)
            .collect();
        assert_eq!(
            connect.len(),
            expected_sessions,
            "{name}: Connect sessions found"
        );
        for session in connect {
            total += 1;
            let report = replay(session);
            println!(
                "{}: {} watch/phone items, {} client requests, {} files, {} cycles",
                report.name,
                session.items.len(),
                report.client_requests,
                report.files_replayed,
                report.cycles
            );
            for (a, n) in &report.used {
                println!("    allowed difference used: {a:?} x{n}");
            }
            for (k, n) in &report.anomalies {
                println!("    client anomaly: {k} x{n}");
            }
            if let Some(c) = &report.closed {
                println!("    CLOSED: {c}");
            }
            for u in &report.unclassified {
                println!("    UNCLASSIFIED at {} ms: {}", u.t_ms, u.what);
            }
            for f in &report.failures {
                println!("    FAIL: {f}");
            }
            if report.files_replayed == 0 {
                println!("    FAIL: no file was replayed");
                failed.push(format!("{}: no files", report.name));
            }
            if !report.ok() {
                failed.push(report.name.clone());
            }
        }
    }
    println!("{total} Connect sessions replayed");
    assert_eq!(total, 5, "Connect sessions replayed");
    assert!(failed.is_empty(), "sessions with differences: {failed:?}");
}
