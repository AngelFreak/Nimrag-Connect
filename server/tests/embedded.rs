//! The in-process server (the Android app's local server): secret guard, no login, one local
//! user who owns the data and no admin, ingest through the library call, derived data when the
//! FIT profile is compiled in.
mod common;

use nimrag_core::derive::test_support::{FitBuilder, Fv};
use nimrag_server::embedded;
use nimrag_server::ledger::Meta;
use nimrag_server::users::{self, Role};
use nimrag_server::{auth, db};

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
    let me: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(me["authenticated"], true);
    assert_eq!(me["username"], embedded::LOCAL_USERNAME);
    assert_eq!(me["role"], "user");
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

fn storage_files(rt: &tokio::runtime::Runtime, port: u16) -> i64 {
    let ok = format!("{}={SECRET}", embedded::SECRET_COOKIE);
    let (s, body) = get(rt, port, "/api/storage", Some(&ok), None);
    assert_eq!(s, 200, "{body}");
    serde_json::from_str::<serde_json::Value>(&body).unwrap()["files"]
        .as_i64()
        .unwrap()
}

fn fit_file() -> Vec<u8> {
    let t = 1_100_000_000u32;
    let mut b = FitBuilder::new();
    for i in 0..5u32 {
        b = b.message(55, &[(253, Fv::U32(t + i)), (27, Fv::U8(60 + i as u8))]);
    }
    b.build()
}

/// Run `f` against the data dir's database while no embedded server has it open.
fn with_db<T>(dir: &std::path::Path, f: impl AsyncFnOnce(&sqlx::SqlitePool) -> T) -> T {
    rt().block_on(async {
        let pool = db::open(dir).await.unwrap();
        let out = f(&pool).await;
        pool.close().await;
        out
    })
}

#[test]
fn one_local_user_owns_the_data_and_no_admin_exists() {
    let dir = tempfile::tempdir().unwrap();
    let rt = rt();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let uid = e.user_id();
    let ok = format!("{}={SECRET}", embedded::SECRET_COOKIE);

    // Nothing yet; then a file ingested in process shows up for the local user.
    assert_eq!(storage_files(&rt, e.port()), 0);
    assert!(e.ingest_bytes(&fit_file(), meta(128)).unwrap());
    assert_eq!(storage_files(&rt, e.port()), 1);

    // The account-management API is not reachable: the local user is no admin.
    for path in ["/api/admin/users", "/api/admin/unassigned"] {
        assert_eq!(get(&rt, e.port(), path, Some(&ok), None).0, 403, "{path}");
    }
    // First-run setup would create an admin: it is closed (a user exists) and read-only anyway.
    let (_, setup) = get(&rt, e.port(), "/api/setup", Some(&ok), None);
    assert_eq!(setup, r#"{"needed":false}"#);
    let post = rt.block_on(async {
        reqwest::Client::new()
            .post(format!("http://127.0.0.1:{}/api/setup", e.port()))
            .header("cookie", &ok)
            .header("x-nimrag", "1")
            .header("content-type", "application/json")
            .body(r#"{"username":"boss","password":"correct horse battery"}"#)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    });
    assert_eq!(post, 403);
    e.stop();

    // Exactly one account, a normal user who owns the file; a restart reuses it.
    with_db(dir.path(), async |pool| {
        let all = users::list(pool).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, uid);
        assert_eq!(all[0].role, Role::User);
        assert!(!all[0].totp_enabled, "2FA is irrelevant here");
        let owned: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM file_owners WHERE user_id = ?")
            .bind(uid)
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(owned, 1);
        // Nobody can sign in with a password: it is random and unknown.
        assert!(!auth::verify_password("", &all[0].password_hash));
    });
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    assert_eq!(e.user_id(), uid);
    assert_eq!(storage_files(&rt, e.port()), 1);
    e.stop();
}

#[test]
fn a_data_dir_with_an_admin_or_several_users_is_refused() {
    let hash = auth::hash_password("a password nobody uses").unwrap();

    let admin_dir = tempfile::tempdir().unwrap();
    with_db(admin_dir.path(), async |pool| {
        let id = users::create(pool, "boss", &hash, Role::Admin, false, 0)
            .await
            .unwrap();
        users::set_totp(pool, id.unwrap(), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ", 0)
            .await
            .unwrap();
    });
    let err = embedded::start(admin_dir.path(), 0, SECRET).err().unwrap();
    assert!(format!("{err:#}").contains("no admin"), "{err:#}");

    let two_dir = tempfile::tempdir().unwrap();
    with_db(two_dir.path(), async |pool| {
        for name in ["anna", "bjarne"] {
            users::create(pool, name, &hash, Role::User, false, 0)
                .await
                .unwrap();
        }
    });
    assert!(embedded::start(two_dir.path(), 0, SECRET).is_err());
}

#[test]
fn a_spike_era_data_dir_becomes_the_local_users() {
    // Build a data dir the way the single-owner spike left it after migration 0010: the only
    // account is the migrated `admin` (2FA never enrolled) and the files have no owner.
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    assert!(e.ingest_bytes(&fit_file(), meta(128)).unwrap());
    let uid = e.user_id();
    e.stop();
    with_db(dir.path(), async |pool| {
        sqlx::query("DELETE FROM file_owners")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("UPDATE users SET role = 'admin', username = 'admin'")
            .execute(pool)
            .await
            .unwrap();
    });

    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    assert_eq!(e.user_id(), uid);
    let rt = rt();
    let ok = format!("{}={SECRET}", embedded::SECRET_COOKIE);
    let (_, me) = get(&rt, e.port(), "/api/me", Some(&ok), None);
    assert!(me.contains(r#""role":"user""#), "{me}");
    assert_eq!(
        storage_files(&rt, e.port()),
        1,
        "the old file is the local user's now"
    );
    e.stop();
}
