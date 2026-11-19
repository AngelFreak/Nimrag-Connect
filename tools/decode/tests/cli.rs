//! Integration tests: run the real nimrag-decode binary on btsnoop input.
//!
//! The synthetic session uses made-up identifiers and is safe to commit.
//! Real captures live in private/captures/ (git-ignored) and are replayed
//! when present.

use std::path::{Path, PathBuf};
use std::process::Command;

use nimrag_core::capture::BTSNOOP_UNIX_OFFSET_US;
use nimrag_core::cobs::encode_frame;
use nimrag_core::gfdi::Message;
use nimrag_core::mlr::{self, Header};
use nimrag_core::multilink;

const WRITE_CHAR: u16 = 0x1B;
const NOTIFY_CHAR: u16 = 0x18;
const ATT_PAYLOAD: usize = 20; // negotiated MTU 23 in the real capture

struct Session {
    records: Vec<(bool, Vec<u8>)>, // (received, H4 packet)
}

impl Session {
    fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// ATT write command / notification, split into two ACL fragments when long.
    fn att(&mut self, received: bool, handle: u16, value: &[u8]) {
        let opcode = if received { 0x1B } else { 0x52 };
        let mut att = vec![opcode];
        att.extend_from_slice(&handle.to_le_bytes());
        att.extend_from_slice(value);
        let mut l2 = (att.len() as u16).to_le_bytes().to_vec();
        l2.extend_from_slice(&4u16.to_le_bytes());
        l2.extend_from_slice(&att);
        let (a, b) = l2.split_at(l2.len().min(9));
        for (i, frag) in [a, b].iter().enumerate() {
            if frag.is_empty() {
                continue;
            }
            let pb: u16 = if i == 0 { 0x2000 } else { 0x1000 };
            let mut h4 = vec![0x02];
            h4.extend_from_slice(&(0x0040 | pb).to_le_bytes());
            h4.extend_from_slice(&(frag.len() as u16).to_le_bytes());
            h4.extend_from_slice(frag);
            self.records.push((received, h4));
        }
    }

    fn btsnoop(&self) -> Vec<u8> {
        let mut f = b"btsnoop\0".to_vec();
        f.extend_from_slice(&1u32.to_be_bytes());
        f.extend_from_slice(&1002u32.to_be_bytes());
        for (i, (rx, d)) in self.records.iter().enumerate() {
            f.extend_from_slice(&(d.len() as u32).to_be_bytes());
            f.extend_from_slice(&(d.len() as u32).to_be_bytes());
            f.extend_from_slice(&(*rx as u32).to_be_bytes());
            f.extend_from_slice(&0u32.to_be_bytes());
            f.extend_from_slice(&(BTSNOOP_UNIX_OFFSET_US + 1_000 * i as i64).to_be_bytes());
            f.extend_from_slice(d);
        }
        f
    }
}

/// GFDI over MLR handle 2 with 20-byte ATT packets, like the real link.
struct Gfdi {
    seq_tx: u8,
    seq_rx: u8,
}

impl Gfdi {
    fn send(&mut self, s: &mut Session, received: bool, m: Message) {
        let wire = encode_frame(&m.encode());
        for chunk in wire.chunks(ATT_PAYLOAD - 2) {
            let seq = if received {
                &mut self.seq_rx
            } else {
                &mut self.seq_tx
            };
            let pkt = mlr::build(
                Header {
                    handle: 2,
                    ack: 0,
                    seq: *seq,
                },
                chunk,
            );
            *seq = (*seq + 1) & 0x3F;
            let handle = if received { NOTIFY_CHAR } else { WRITE_CHAR };
            s.att(received, handle, &pkt);
        }
    }
}

fn register_response(service: u16, handle: u8, flags: u8, extra: u8) -> Vec<u8> {
    let mut v = vec![0, 1];
    v.extend_from_slice(&1u64.to_le_bytes());
    v.extend_from_slice(&service.to_le_bytes());
    v.extend_from_slice(&[0, handle, flags, extra]);
    v
}

fn response(request: u16, body: &[u8]) -> Message {
    let mut p = request.to_le_bytes().to_vec();
    p.push(0);
    p.extend_from_slice(body);
    Message::new(5000, p)
}

/// The fake FIT payload transferred as file index 7.
fn fake_fit() -> Vec<u8> {
    let mut f = vec![14, 0x20, 0, 0];
    f.extend_from_slice(&40u32.to_le_bytes());
    f.extend_from_slice(b".FIT");
    f.extend_from_slice(&[0, 0]);
    f.extend((0..42u8).map(|i| i.wrapping_mul(13)));
    f
}

fn synthetic_session() -> Vec<u8> {
    let mut s = Session::new();
    let mut g = Gfdi {
        seq_tx: 0,
        seq_rx: 0,
    };
    s.att(false, 0x19, &[1, 0]); // CCCD enable notifications
    s.att(false, WRITE_CHAR, &multilink::close_all_request(1));
    let mut close_all_resp = vec![0, 6];
    close_all_resp.extend_from_slice(&1u64.to_le_bytes());
    close_all_resp.extend_from_slice(&[0, 0, 1]);
    s.att(true, NOTIFY_CHAR, &close_all_resp);
    s.att(false, WRITE_CHAR, &multilink::register_request(1, 4, 0));
    s.att(true, NOTIFY_CHAR, &register_response(4, 0x2E, 0, 1));
    s.att(false, WRITE_CHAR, &[0x2E, 0x03]);
    s.att(
        true,
        NOTIFY_CHAR,
        &[0x2E, 0x03, 0x30, 0x0F, 0xBA, 0x06, 0xEF, 0xBE, 0xAD, 0xDE],
    );
    s.att(false, WRITE_CHAR, &multilink::register_request(1, 1, 2));
    s.att(true, NOTIFY_CHAR, &register_response(1, 0x82, 1, 0));

    // DeviceInformation with made-up identifiers.
    let mut di = Vec::new();
    di.extend_from_slice(&150u16.to_le_bytes());
    di.extend_from_slice(&3888u16.to_le_bytes());
    di.extend_from_slice(&0xDEADBEEFu32.to_le_bytes());
    di.extend_from_slice(&1722u16.to_le_bytes());
    di.extend_from_slice(&4000u16.to_le_bytes());
    for name in ["Test Watch", "Test", "Model"] {
        di.push(name.len() as u8);
        di.extend_from_slice(name.as_bytes());
    }
    g.send(&mut s, true, Message::new(5024, di));
    // A retransmitted copy of the watch's previous packet must be ignored.
    let dup = mlr::build(
        Header {
            handle: 2,
            ack: 0,
            seq: (g.seq_rx + 63) & 0x3F,
        },
        &[0x55, 0x55],
    );
    s.att(true, NOTIFY_CHAR, &dup);
    g.send(&mut s, false, response(5024, &[0x96, 0]));
    g.send(&mut s, true, Message::new(5037, vec![2, 1, 0b0010_0000]));

    // Directory (index 0): 16-byte header + one entry for index 7.
    let fit = fake_fit();
    let mut dir = vec![0u8; 16];
    dir.extend_from_slice(&7u16.to_le_bytes());
    dir.extend_from_slice(&[128, 4, 1, 0, 0, 0]);
    dir.extend_from_slice(&(fit.len() as u32).to_le_bytes());
    dir.extend_from_slice(&1_000_000u32.to_le_bytes());

    for (index, content) in [(0u16, dir), (7u16, fit)] {
        let mut req = index.to_le_bytes().to_vec();
        req.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1]);
        g.send(&mut s, false, Message::new(5002, req));
        let mut body = vec![0];
        body.extend_from_slice(&(content.len() as u32).to_le_bytes());
        g.send(&mut s, true, response(5002, &body));
        let mut crc = 0u16;
        for (i, chunk) in content.chunks(25).enumerate() {
            crc = nimrag_core::crc::update(crc, chunk);
            let mut p = vec![0];
            p.extend_from_slice(&crc.to_le_bytes());
            p.extend_from_slice(&((i * 25) as u32).to_le_bytes());
            p.extend_from_slice(chunk);
            g.send(&mut s, true, Message::new(5004, p));
            g.send(&mut s, false, response(5004, &[0, 0, 0, 0, 0]));
        }
        if index != 0 {
            g.send(&mut s, false, Message::new(5008, vec![7, 0, 0x10]));
            g.send(&mut s, true, response(5008, &[0]));
        }
    }
    g.send(&mut s, false, Message::new(5030, vec![0, 0]));
    s.btsnoop()
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_nimrag-decode")
}

fn tmp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nimrag-decode-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn synthetic_session_decodes_strictly_and_extracts_the_file() {
    let dir = tmp_dir("synthetic");
    let log = dir.join("btsnoop_hci.log");
    std::fs::write(&log, synthetic_session()).unwrap();
    let out = Command::new(bin())
        .args(["--strict", "--verbose", "--extract"])
        .arg(dir.join("files"))
        .arg(&log)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "decode failed:\n{text}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for expected in [
        "[c1] ML register-response GFDI(1) SUCCESS handle=Some(130) reliable=true",
        "ML info product=3888 firmware=17.22 unit_id=<redacted>",
        "DeviceInformation(5024) protocol=150 product=3888 unit_id=<redacted>",
        "MLR h2 retransmission",
        "SyncRequest(5037) option=VISIBLE_AS_NEEDED bits=[5]",
        "directory: 1 entries",
        "index=7 type=128/4 (activity)",
        "file index=7 (activity) 56 bytes, running CRC ok",
        "SetFileFlags(5008) index=7 flags=0x10",
        "GATT CCCD att=0x0019 notify=true",
        "unknown frames: 0  decode errors: 0",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
    }
    assert!(!text.contains("3735928559"), "unit id leaked into output");
    let extracted = std::fs::read(dir.join("files").join("002_idx00007_128_4.bin")).unwrap();
    assert_eq!(extracted, fake_fit());
}

#[test]
fn strict_mode_fails_on_undecodable_traffic() {
    let mut s = Session::new();
    s.att(true, 0x0042, &[0xF0, 0x00, 1, 2, 3]); // MLR packet with no registration seen
    let dir = tmp_dir("strict");
    let log = dir.join("btsnoop_hci.log");
    std::fs::write(&log, s.btsnoop()).unwrap();
    let out = Command::new(bin())
        .arg("--strict")
        .arg(&log)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn reads_btsnoop_from_stdin() {
    use std::io::Write;
    let mut child = Command::new(bin())
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&synthetic_session())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("unknown frames: 0  decode errors: 0"));
}

fn private_captures() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../private/captures");
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    // `live-*` recordings come from the snoop socket and usually start in the
    // middle of a session (registrations not seen), so only complete
    // bugreport captures are held to the strict no-unknown-frames gate.
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with("live-"))
        .map(|e| e.path().join("btsnoop_hci.log"))
        .filter(|p| p.exists())
        .collect();
    v.sort();
    v
}

#[test]
fn every_private_capture_decodes_without_unknown_frames() {
    let caps = private_captures();
    if caps.is_empty() {
        eprintln!("no private captures present; skipping");
        return;
    }
    for cap in caps {
        let out = Command::new(bin())
            .arg("--strict")
            .arg(&cap)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let summary = text.lines().last().unwrap_or("");
        assert!(out.status.success(), "{}: {summary}", cap.display());
    }
}
