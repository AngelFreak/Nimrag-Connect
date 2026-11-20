//! The user-related commands of the real binary: `token create --user`, `user list`,
//! `user set-password`, `user reset-2fa` and `dev-seed` (and what none of them may print).

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use nimrag_server::users::Role;
use serde_json::json;
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(dir: &std::path::Path, args: &[&str], envs: &[(&str, &str)], stdin: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_nimrag-server"));
    cmd.args(args)
        .arg("--data-dir")
        .arg(dir)
        .env_remove("NIMRAG_SEED_ADMIN_PASSWORD")
        .env_remove("NIMRAG_SEED_ADMIN_TOTP")
        .env_remove("NIMRAG_SEED_USER_PASSWORD")
        .env_remove("NIMRAG_SEED_USER_TOTP")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    if let Some(s) = stdin {
        input.write_all(s.as_bytes()).unwrap();
    }
    drop(input);
    child.wait_with_output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A data dir holding a data user `tester` and an admin `boss`, both enrolled.
async fn dir_with_users() -> (tempfile::TempDir, sqlx::SqlitePool) {
    let dir = tempfile::tempdir().unwrap();
    let pool = nimrag_server::db::open(dir.path()).await.unwrap();
    make_user(&pool, USER, Role::User, true).await;
    make_user(&pool, "boss", Role::Admin, true).await;
    (dir, pool)
}

async fn env_over(dir: tempfile::TempDir, pool: sqlx::SqlitePool) -> Env {
    let state = nimrag_server::api::AppState::new(pool.clone(), dir.path().to_path_buf(), 1 << 20);
    let app = nimrag_server::api::router(state);
    Env::from_parts(dir, pool, app, String::new(), None)
}

async fn try_login(e: &Env, user: &str, pw: &str, secret: &str) -> StatusCode {
    let code = nimrag_server::totp::code_at(secret, e.now()).unwrap();
    e.call(Method::POST, "/api/login", None, false, Some(json!({ "username": user, "password": pw, "code": code })))
        .await
        .0
}

#[tokio::test]
async fn token_create_assigns_the_named_user_or_leaves_the_token_unassigned() {
    let (dir, pool) = dir_with_users().await;
    let o = run(dir.path(), &["token", "create", "--name", "phone", "--user", "Tester"], &[], None);
    assert!(o.status.success(), "{}", err(&o));
    let token = out(&o).trim().to_owned();
    assert!(token.starts_with("nrg_"));
    let uid: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'tester'").fetch_one(&pool).await.unwrap();
    let owner: Option<i64> = sqlx::query_scalar("SELECT user_id FROM tokens WHERE hash = ?").bind(nimrag_server::tokens::hash_token(&token)).fetch_one(&pool).await.unwrap();
    assert_eq!(owner, Some(uid));

    let o = run(dir.path(), &["token", "create", "--name", "old"], &[], None);
    assert!(o.status.success());
    assert!(err(&o).contains("unassigned"), "{}", err(&o));
    let o = run(dir.path(), &["token", "create", "--name", "x", "--user", "ghost"], &[], None);
    assert!(!o.status.success());
    assert!(err(&o).contains("no user named ghost"), "{}", err(&o));
    assert!(out(&o).trim().is_empty(), "no token is printed on failure");

    let list = out(&run(dir.path(), &["token", "list"], &[], None));
    assert!(list.lines().any(|l| l.contains("phone") && l.contains("tester") && l.contains("active")), "{list}");
    assert!(list.lines().any(|l| l.contains("old") && l.contains("(unassigned)")), "{list}");
    assert!(!list.contains(&token));
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tokens WHERE name = 'x'").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn user_list_shows_accounts_and_no_secrets() {
    let (dir, _pool) = dir_with_users().await;
    let o = run(dir.path(), &["user", "list"], &[], None);
    assert!(o.status.success(), "{}", err(&o));
    let text = out(&o);
    assert!(text.lines().any(|l| l.contains("tester") && l.contains("user")), "{text}");
    assert!(text.lines().any(|l| l.contains("boss") && l.contains("admin")), "{text}");
    for secret in ["argon2", TOTP_SECRET, PASSWORD] {
        assert!(!text.contains(secret) && !err(&o).contains(secret), "{secret}");
    }
}

#[tokio::test]
async fn set_password_reads_stdin_signs_out_and_validates() {
    let (dir, pool) = dir_with_users().await;
    let path = dir.path().to_path_buf();

    // Two admins would make the default ambiguous; a user name is always fine.
    let o = run(&path, &["user", "set-password", "--username", USER], &[], Some("short\n"));
    assert!(!o.status.success());
    assert!(err(&o).contains("at least"), "{}", err(&o));
    let o = run(&path, &["user", "set-password", "--username", "ghost"], &[], Some("long enough password\n"));
    assert!(!o.status.success());

    let new = "a much better passphrase";
    let o = run(&path, &["user", "set-password", "--username", USER], &[], Some(&format!("{new}\nignored second line\n")));
    assert!(o.status.success(), "{}", err(&o));
    assert!(!out(&o).contains(new) && !err(&o).contains(new), "the password is never echoed");

    // Without --username the only admin is meant.
    let o = run(&path, &["user", "set-password"], &[], Some("another long passphrase\n"));
    assert!(o.status.success(), "{}", err(&o));
    make_user(&pool, "boss2", Role::Admin, true).await;
    let o = run(&path, &["user", "set-password"], &[], Some("yet another passphrase\n"));
    assert!(!o.status.success(), "two admins: the name is required");

    let e = env_over(dir, pool).await;
    assert_eq!(try_login(&e, USER, PASSWORD, TOTP_SECRET).await, StatusCode::UNAUTHORIZED);
    e.exec("UPDATE users SET totp_last_step = 0").await;
    assert_eq!(try_login(&e, USER, new, TOTP_SECRET).await, StatusCode::OK);
    e.exec("UPDATE users SET totp_last_step = 0").await;
    assert_eq!(try_login(&e, "boss", "another long passphrase", TOTP_SECRET).await, StatusCode::OK);
}

#[tokio::test]
async fn reset_2fa_makes_the_user_enrol_again_and_needs_a_name() {
    let (dir, pool) = dir_with_users().await;
    let o = run(dir.path(), &["user", "reset-2fa"], &[], None);
    assert!(!o.status.success(), "--username is required");
    let o = run(dir.path(), &["user", "reset-2fa", "--username", "ghost"], &[], None);
    assert!(!o.status.success());
    let o = run(dir.path(), &["user", "reset-2fa", "--username", USER], &[], None);
    assert!(o.status.success(), "{}", err(&o));
    let (enabled, secret): (i64, Option<String>) = sqlx::query_as("SELECT totp_enabled, totp_secret FROM users WHERE username = 'tester'").fetch_one(&pool).await.unwrap();
    assert_eq!((enabled, secret), (0, None));
    let e = env_over(dir, pool).await;
    let (s, _, j) = e.call(Method::POST, "/api/login", None, false, Some(json!({ "username": USER, "password": PASSWORD }))).await;
    assert_eq!((s, j["step"].as_str()), (StatusCode::OK, Some("enrol2fa")));
}

#[tokio::test]
async fn dev_seed_prints_only_the_credentials_it_made_up_and_they_work() {
    let dir = tempfile::tempdir().unwrap();
    let o = run(dir.path(), &["dev-seed"], &[], None);
    assert!(o.status.success(), "{}", err(&o));
    let text = out(&o);
    let line = |prefix: &str| -> String {
        text.lines()
            .find_map(|l| l.strip_prefix(prefix))
            .unwrap_or_else(|| panic!("no line {prefix:?} in:\n{text}"))
            .trim()
            .to_owned()
    };
    let (admin_pw, admin_totp) = (line("admin password: "), line("admin 2FA secret (base32): "));
    let (demo_pw, demo_totp) = (line("demo password: "), line("demo 2FA secret (base32): "));
    for v in [&admin_pw, &admin_totp, &demo_pw, &demo_totp] {
        assert!(!err(&o).contains(v.as_str()), "a credential reached the logs");
    }
    assert_ne!(admin_pw, demo_pw);

    let pool = nimrag_server::db::open(dir.path()).await.unwrap();
    let e = env_over(dir, pool).await;
    assert_eq!(try_login(&e, "demo", &demo_pw, &demo_totp).await, StatusCode::OK);
    assert_eq!(try_login(&e, "admin", &admin_pw, &admin_totp).await, StatusCode::OK);
    // Seeding twice is refused and prints no new credentials.
    let again = run(e.dir.path(), &["dev-seed"], &[], None);
    assert!(!again.status.success());
    assert!(!out(&again).contains("password:"));
}

#[tokio::test]
async fn dev_seed_uses_the_credentials_it_is_given_and_does_not_print_them() {
    let dir = tempfile::tempdir().unwrap();
    let envs = [
        ("NIMRAG_SEED_ADMIN_PASSWORD", "admin seed passphrase"),
        ("NIMRAG_SEED_ADMIN_TOTP", "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP"),
        ("NIMRAG_SEED_USER_PASSWORD", "demo seed passphrase"),
        ("NIMRAG_SEED_USER_TOTP", TOTP_SECRET),
    ];
    let o = run(dir.path(), &["dev-seed"], &envs, None);
    assert!(o.status.success(), "{}", err(&o));
    let all = format!("{}{}", out(&o), err(&o));
    for (_, v) in envs {
        assert!(!all.contains(v), "{v} was printed");
    }
    assert!(!all.contains("password:") && !all.contains("2FA secret"), "{all}");
    let pool = nimrag_server::db::open(dir.path()).await.unwrap();
    let e = env_over(dir, pool).await;
    assert_eq!(try_login(&e, "demo", "demo seed passphrase", TOTP_SECRET).await, StatusCode::OK);
    assert_eq!(try_login(&e, "admin", "admin seed passphrase", "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP").await, StatusCode::OK);
}

#[tokio::test]
async fn dev_seed_with_only_some_credentials_prints_just_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let o = run(dir.path(), &["dev-seed"], &[("NIMRAG_SEED_USER_PASSWORD", "demo seed passphrase")], None);
    assert!(o.status.success(), "{}", err(&o));
    let text = out(&o);
    assert!(text.contains("admin password: ") && text.contains("admin 2FA secret") && text.contains("demo 2FA secret"), "{text}");
    assert!(!text.contains("demo password:") && !text.contains("demo seed passphrase"));
}
