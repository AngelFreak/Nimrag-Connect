//! The export streams: a large file set goes out without being held in memory.
//!
//! One test in its own file so that it is the only thing running in the process while it
//! measures the resident memory (Linux only; elsewhere it only checks the archive).
mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use common::*;
use http_body_util::BodyExt;
use std::io::Write;
use tower::ServiceExt;

/// Resident memory of this process in bytes (`/proc/self/statm`), if the platform has it.
fn rss() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = s.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

const FILES: usize = 24;
const SIZE: usize = 2 << 20;

#[tokio::test]
async fn a_large_file_set_is_streamed_and_memory_stays_flat() {
    let e = env().await;
    // 24 files of 2 MiB of noise (it does not compress): 48 MiB in, about as much out.
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    for i in 0..FILES {
        let mut blob = vec![0u8; SIZE];
        for b in &mut blob {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = x as u8;
        }
        blob[0] = i as u8;
        let (s, _) = e.upload("data_type=4", &blob).await;
        assert_eq!(s, StatusCode::CREATED);
    }
    let cookie = e.login().await;
    let before = rss();

    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/export")
        .header("cookie", &cookie)
        .body(Body::empty())
        .unwrap();
    let resp = e.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut body = resp.into_body();

    // Read the first piece, then let the export wait: a streaming export fills a few small
    // queues and stops; one that built the archive first would by now hold all of it.
    let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert!(!first.is_empty());
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    if let (Some(b), Some(now)) = (before, rss()) {
        let grown = now.saturating_sub(b);
        assert!(
            grown < 24 << 20,
            "memory grew by {} MiB while the client was not reading",
            grown >> 20
        );
    }

    // Drain to a file (not memory), tracking the peak, and check the archive afterwards.
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let mut out = std::io::BufWriter::new(tmp.reopen().unwrap());
    out.write_all(&first).unwrap();
    let mut total = first.len() as u64;
    let mut peak = 0u64;
    while let Some(frame) = body.frame().await {
        let frame = frame.unwrap();
        if let Ok(d) = frame.into_data() {
            total += d.len() as u64;
            out.write_all(&d).unwrap();
        }
        if let (Some(b), Some(now)) = (before, rss()) {
            peak = peak.max(now.saturating_sub(b));
        }
    }
    out.flush().unwrap();
    drop(out);
    assert!(total > (FILES * SIZE) as u64, "{total} bytes");
    if before.is_some() {
        eprintln!("peak memory growth while streaming: {} KiB", peak >> 10);
        assert!(peak < 24 << 20, "peak growth {} MiB", peak >> 20);
    }

    let mut z = zip::ZipArchive::new(std::fs::File::open(tmp.path()).unwrap()).unwrap();
    let originals = (0..z.len())
        .filter(|&i| {
            z.by_index(i)
                .unwrap()
                .name()
                .unwrap()
                .starts_with("originals/")
        })
        .count();
    assert_eq!(originals, FILES);
    // One entry read in full, with its size and its checksum verified by the zip reader.
    let name = (0..z.len())
        .map(|i| z.by_index(i).unwrap().name().unwrap().into_owned())
        .find(|n| n.starts_with("originals/"))
        .unwrap();
    let mut f = z.by_name(&name).unwrap();
    assert_eq!(f.size(), SIZE as u64);
    let n = std::io::copy(&mut f, &mut std::io::sink()).unwrap();
    assert_eq!(n, SIZE as u64);
}
