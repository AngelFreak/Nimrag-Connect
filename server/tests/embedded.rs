//! The in-process server (the Android app's local server): secret guard, no login,
//! ingest through the library call, derived data when the FIT profile is compiled in.
mod common;

use nimrag_core::derive::test_support::{FitBuilder, Fv};
use nimrag_server::embedded;
use nimrag_server::ledger::Meta;

const SECRET: &str = "0123456789abcdef0123456789abcdef";

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn get(
    rt: &tokio::runtime::Runtime,
    port: u16,
    path: &str,
    cookie: Option<&str>,
    host: Option<&str>,
) -> (u16, String) {
    rt.block_on(async {
        let mut r = reqwest::Client::new().get(format!("http://127.0.0.1:{port}{path}"));
        if let Some(c) = cookie {
            r = r.header("cookie", c);
        }
        if let Some(h) = host {
            r = r.header("host", h);
        }
        let resp = r.send().await.unwrap();
        (resp.status().as_u16(), resp.text().await.unwrap())
    })
}

fn meta(data_type: i64) -> Meta {
    Meta {
        source: "watch".into(),
        data_type: Some(data_type),
        sub_type: None,
        file_index: None,
        file_number: None,
        watch_timestamp: None,
    }
}

#[test]
fn secret_guard_and_no_login() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let rt = rt();
    let p = e.port();
    assert_ne!(p, 0);
    let ok = format!("{}={SECRET}", embedded::SECRET_COOKIE);

    // No secret, wrong secret, and the right secret under a foreign Host header: 403.
    assert_eq!(get(&rt, p, "/api/health", None, None).0, 403);
    let wrong = format!("{}=nope", embedded::SECRET_COOKIE);
    assert_eq!(get(&rt, p, "/api/today", Some(&wrong), None).0, 403);
    assert_eq!(get(&rt, p, "/", Some(&wrong), None).0, 403);
    assert_eq!(
        get(&rt, p, "/api/health", Some(&ok), Some("evil.example")).0,
        403
    );

    // With the secret: signed in without a login, and first-run setup is not offered.
    assert_eq!(get(&rt, p, "/api/health", Some(&ok), None).0, 200);
    let (s, body) = get(&rt, p, "/api/me", Some(&ok), None);
    assert_eq!(s, 200, "{body}");
    let (_, setup) = get(&rt, p, "/api/setup", Some(&ok), None);
    assert!(setup.contains("false"), "{setup}");
    assert_eq!(get(&rt, p, "/api/today", Some(&ok), None).0, 200);
    // A client-supplied session cookie cannot widen anything: only the secret matters.
    let forged = format!("{ok}; nimrag_session=deadbeef");
    assert_eq!(get(&rt, p, "/api/me", Some(&forged), None).0, 200);
    // Read-only over HTTP: edits made on the phone would never reach the real server.
    let status = rt.block_on(async {
        reqwest::Client::new()
            .post(format!("http://127.0.0.1:{p}/api/files"))
            .header("cookie", &ok)
            .body("x")
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    });
    assert_eq!(status, 403);
    e.stop();
}

#[test]
fn ingest_derives_only_with_a_profile_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let t = 1_100_000_000u32;
    let mut b = FitBuilder::new();
    for i in 0..5u32 {
        b = b.message(55, &[(253, Fv::U32(t + i)), (27, Fv::U8(60 + i as u8))]);
    }
    let file = b.build();

    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    assert!(e.ingest_bytes(&file, meta(128)).unwrap());
    assert!(
        !e.ingest_bytes(&file, meta(128)).unwrap(),
        "same bytes are not new"
    );
    let summary: serde_json::Value = serde_json::from_str(&e.summary().unwrap()).unwrap();
    assert_eq!(summary["files"], 1);
    let key = if nimrag_core::fit::profile::available() {
        "ok"
    } else {
        "no_profile"
    };
    assert_eq!(summary["derive"][key], 1, "{summary}");
    let rt = rt();
    let ok = format!("{}={SECRET}", embedded::SECRET_COOKIE);
    assert_eq!(get(&rt, e.port(), "/api/today", Some(&ok), None).0, 200);
    e.stop();

    // The data dir survives a restart and a second process cannot open it meanwhile.
    let e1 = embedded::start(dir.path(), 0, SECRET).unwrap();
    assert!(
        embedded::start(dir.path(), 0, SECRET).is_err(),
        "one writer per data dir"
    );
    let summary: serde_json::Value = serde_json::from_str(&e1.summary().unwrap()).unwrap();
    assert_eq!(summary["files"], 1);
    e1.stop();
}

#[test]
fn short_secret_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    assert!(embedded::start(dir.path(), 0, "short").is_err());
}
