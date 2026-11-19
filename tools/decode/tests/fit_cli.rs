//! Drives the real `nimrag-decode fit` binary on a synthetic FIT file and
//! replays private real FIT files when present (git-ignored, skipped if absent).

use std::path::PathBuf;
use std::process::Command;

use nimrag_core::crc;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_nimrag-decode")
}

/// Minimal FIT: file_id-like message plus a record with a timestamp.
fn synthetic_fit() -> Vec<u8> {
    let mut body = vec![0x40, 0, 0, 0, 0, 2, 0, 1, 0, 4, 4, 0x86];
    body.extend([0, 4, 0x78, 0x56, 0x34, 0x12]);
    body.extend([0x41, 0, 0, 20, 0, 2, 253, 4, 0x86, 3, 1, 2]);
    body.extend([1, 100, 0, 0, 0, 77]);
    let mut v = vec![14, 0x20];
    v.extend(2132u16.to_le_bytes());
    v.extend((body.len() as u32).to_le_bytes());
    v.extend(b".FIT");
    let hc = crc::checksum(&v);
    v.extend(hc.to_le_bytes());
    v.extend(&body);
    let c = crc::checksum(&v);
    v.extend(c.to_le_bytes());
    v
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nimrag-fit-cli-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(path: &std::path::Path, extra: &[&str]) -> (i32, String) {
    let o = Command::new(bin())
        .arg("fit")
        .args(extra)
        .arg(path)
        .output()
        .unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

#[test]
fn synthetic_fit_dumps_and_exits_zero() {
    let dir = tmp("ok");
    let p = dir.join("a.fit");
    std::fs::write(&p, synthetic_fit()).unwrap();
    let (code, out) = run(&p, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("msg global=0 index=0 ts=- fields=2"), "{out}");
    assert!(
        out.contains("msg global=20 index=1 ts=100 fields=2"),
        "{out}"
    );
    assert!(
        out.contains("summary: files=1 messages=2 status=ok"),
        "{out}"
    );
    assert!(!out.contains("    f"), "values must not print by default");
    let (_, with) = run(&p, &["--fields"]);
    assert!(with.contains("f4 78563412"), "{with}");
    assert!(with.contains("f253 64000000"), "{with}");
}

#[test]
fn crc_mismatch_exits_two() {
    let dir = tmp("crc");
    let p = dir.join("bad.fit");
    let mut b = synthetic_fit();
    let n = b.len();
    b[n - 1] ^= 0xFF;
    std::fs::write(&p, b).unwrap();
    let (code, out) = run(&p, &[]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("crc=mismatch"), "{out}");
}

#[test]
fn truncated_and_garbage_exit_two_and_missing_file_exits_one() {
    let dir = tmp("trunc");
    let p = dir.join("t.fit");
    let b = synthetic_fit();
    std::fs::write(&p, &b[..b.len() - 4]).unwrap();
    assert_eq!(run(&p, &[]).0, 2);
    std::fs::write(&p, b"not a fit file at all").unwrap();
    assert_eq!(run(&p, &[]).0, 2);
    assert_eq!(run(&dir.join("missing.fit"), &[]).0, 1);
}

#[test]
fn every_private_fit_file_replays_strictly() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../private/captures");
    let Ok(caps) = std::fs::read_dir(root) else {
        return;
    };
    for cap in caps.flatten() {
        let Ok(files) = std::fs::read_dir(cap.path().join("files")) else {
            continue;
        };
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            if name.contains("_128_") && name.ends_with(".bin") {
                let (code, _) = run(&f.path(), &[]);
                assert_eq!(code, 0, "a private FIT file did not replay cleanly");
            }
        }
    }
}
