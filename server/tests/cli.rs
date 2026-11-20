//! Drives the real binary against a temp data dir.
mod common;

use axum::http::{Method, StatusCode};
use std::process::Command;

fn bin(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .args(args)
        .arg("--data-dir")
        .arg(dir)
        .output()
        .unwrap()
}

#[tokio::test]
async fn token_create_list_revoke_and_reparse_via_binary() {
    let dir = tempfile::tempdir().unwrap();
    let out = bin(dir.path(), &["token", "create", "--name", "phone"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let token = String::from_utf8(out.stdout).unwrap().trim().to_string();
    assert!(token.starts_with("nrg_"));
    assert!(
        !String::from_utf8(bin(dir.path(), &["token", "list"]).stdout)
            .unwrap()
            .contains(&token)
    );

    // Ingest through the router using the binary-created token and the same data dir.
    let pool = nimrag_server::db::open(dir.path()).await.unwrap();
    let state = nimrag_server::api::AppState::new(pool.clone(), dir.path().to_path_buf(), 1 << 20);
    let e = common::Env::from_parts(
        dir,
        pool.clone(),
        nimrag_server::api::router(state),
        token.clone(),
        None,
    );
    let (s, _) = e.upload("data_type=128", &common::tiny_fit(77)).await;
    assert_eq!(s, StatusCode::CREATED);
    e.upload("data_type=128", b"garbage").await;
    let before = common::dump_derived(&e.pool).await;
    assert!(
        before.contains("fit_messages"),
        "needs a working decoder:\n{before}"
    );

    let out = bin(e.dir.path(), &["reparse"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("2 files"));
    assert_eq!(common::dump_derived(&e.pool).await, before);
    let out = bin(e.dir.path(), &["reparse"]);
    assert!(out.status.success());
    assert_eq!(common::dump_derived(&e.pool).await, before);

    // Drop the derived tables, then rebuild them with the real binary.
    for q in [
        "DELETE FROM fit_messages",
        "DELETE FROM fit_files",
        "DELETE FROM file_parse",
    ] {
        sqlx::query(q).execute(&e.pool).await.unwrap();
    }
    assert_eq!(common::dump_derived(&e.pool).await, "");
    assert!(bin(e.dir.path(), &["reparse"]).status.success());
    assert_eq!(common::dump_derived(&e.pool).await, before);
    assert!(bin(e.dir.path(), &["rebuild-ledger"]).status.success());

    let out = bin(e.dir.path(), &["token", "revoke", "1"]);
    assert!(out.status.success());
    let (s, _) = e
        .request(Method::POST, "/api/files", Some(&token), b"x".to_vec())
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(!bin(e.dir.path(), &["token", "revoke", "1"])
        .status
        .success());
}

fn http(port: u16, request: &str, body: &[u8]) -> String {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(request.as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

fn post(port: u16, token: &str, body: &[u8]) -> String {
    http(
        port,
        &format!(
            "POST /api/files?data_type=4&source=import HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        ),
        body,
    )
}

struct Server(std::process::Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn create_token(dir: &std::path::Path, name: &str) -> String {
    let out = bin(dir, &["token", "create", "--name", name]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// Fresh dir -> token create -> real `serve` accepts it; a token created while
/// the server is running works too.
#[test]
fn real_serve_accepts_tokens_created_before_and_after() {
    let dir = tempfile::tempdir().unwrap();
    let before = create_token(dir.path(), "before");
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let child = Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .args([
            "serve",
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--data-dir",
        ])
        .arg(dir.path())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server(child);
    let mut up = false;
    for _ in 0..100 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            up = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(up, "server did not start");

    assert!(post(port, &before, b"one").starts_with("HTTP/1.1 201"));
    assert!(post(port, "nrg_bogus", b"one").starts_with("HTTP/1.1 401"));
    let after = create_token(dir.path(), "after");
    assert!(post(port, &after, b"two").starts_with("HTTP/1.1 201"));
}

#[test]
fn second_writer_on_the_same_data_dir_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut serve = Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .args(["serve", "--listen", "127.0.0.1:0", "--data-dir"])
        .arg(dir.path())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // The holder writes its command name once it owns the lock.
    let lock = dir.path().join(".lock");
    for _ in 0..100 {
        if std::fs::read_to_string(&lock).is_ok_and(|s| s == "serve") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    for cmd in [
        "reparse",
        "rebuild-ledger",
        "import-export",
        "import-connect",
        "serve",
    ] {
        let out = bin(dir.path(), &[cmd, dir.path().to_str().unwrap()]);
        assert!(!out.status.success(), "{cmd} must be refused");
        let e = String::from_utf8_lossy(&out.stderr);
        assert!(e.contains("in use by `serve`"), "{cmd}: {e}");
    }
    serve.kill().unwrap();
    serve.wait().unwrap();
    // The lock dies with the process.
    assert!(bin(dir.path(), &["reparse"]).status.success());
}
