//! Scheduled backups set by the admin in the web app: saved settings against start-up options,
//! validation, the scheduler following a change, run now, status and the list. Backups are only
//! ever written into temporary folders.
mod common;

use axum::http::{Method, StatusCode};
use common::*;
use nimrag_server::users::Role;
use nimrag_server::{api, backup, db};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

struct Rig {
    e: Env,
    admin: String,
    ctl: Arc<backup::Control>,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// A server with an admin signed in and the scheduler running, as `serve` starts it.
async fn rig(startup: Option<backup::Config>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::open(dir.path()).await.unwrap();
    let state = api::AppState::new(pool.clone(), dir.path().to_path_buf(), api::MAX_BODY_BYTES)
        .with_backup_startup(startup);
    let ctl = state.backups.clone();
    ctl.reload(&pool).await.unwrap();
    let handle = backup::spawn_scheduler_with(pool.clone(), dir.path().to_path_buf(), ctl.clone());
    let e = Env::from_parts(dir, pool, api::router(state), String::new(), None);
    e.add_user("boss", Role::Admin).await;
    let admin = e.login_as("boss").await;
    Rig {
        e,
        admin,
        ctl,
        handle,
    }
}

fn startup_cfg(dir: &Path) -> backup::Config {
    backup::Config {
        dir: dir.to_path_buf(),
        every: Duration::from_secs(3600),
        keep: 3,
    }
}

fn names(dir: &Path) -> Vec<String> {
    backup::list(dir)
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect()
}

fn settings(dir: &Path) -> Value {
    json!({ "enabled": true, "dir": dir.to_str().unwrap(), "every_s": 86400, "keep": 5 })
}

impl Rig {
    async fn get(&self) -> Value {
        self.e.get(&self.admin, "/api/admin/backups").await
    }
    async fn put(&self, body: Value) -> (StatusCode, Value) {
        self.e
            .send(&self.admin, Method::PUT, "/api/admin/backups", body)
            .await
    }
    async fn post_run(&self) -> (StatusCode, Value) {
        self.e
            .send(
                &self.admin,
                Method::POST,
                "/api/admin/backups/run",
                json!({}),
            )
            .await
    }
}

async fn until<F: FnMut() -> bool>(what: &str, mut f: F) {
    for _ in 0..100 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
async fn only_admins_with_the_csrf_header_reach_the_backup_routes() {
    let r = rig(None).await;
    r.e.add_user(USER, Role::User).await;
    let user = r.e.login().await;
    let routes = [
        (Method::GET, "/api/admin/backups"),
        (Method::PUT, "/api/admin/backups"),
        (Method::DELETE, "/api/admin/backups"),
        (Method::POST, "/api/admin/backups/run"),
    ];
    let out = tempfile::tempdir().unwrap();
    for (m, uri) in routes {
        let body = (m == Method::PUT).then(|| settings(out.path()));
        let (s, _, _) = r.e.call(m.clone(), uri, None, true, body.clone()).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{m} {uri} without a session");
        let (s, _, _) =
            r.e.call(m.clone(), uri, Some(&user), true, body.clone())
                .await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{m} {uri} as a normal user");
        if m != Method::GET {
            let (s, _, _) = r.e.call(m.clone(), uri, Some(&r.admin), false, body).await;
            assert_eq!(
                s,
                StatusCode::FORBIDDEN,
                "{m} {uri} without the CSRF header"
            );
        }
    }
    assert!(names(out.path()).is_empty());
}

#[tokio::test]
async fn saved_settings_beat_the_start_up_options_and_delete_goes_back() {
    let startup = tempfile::tempdir().unwrap();
    let saved = tempfile::tempdir().unwrap();
    let r = rig(Some(startup_cfg(startup.path()))).await;
    // The start-up options work as before: the first backup is due at once.
    until("the start-up backup", || names(startup.path()).len() == 1).await;
    let j = r.get().await;
    assert_eq!(j["source"], "startup");
    assert_eq!(j["settings"], Value::Null);
    assert_eq!(j["startup"]["dir"], startup.path().to_str().unwrap());
    assert_eq!(j["startup"]["every_s"], 3600);
    assert_eq!(j["startup"]["keep"], 3);
    assert_eq!(j["backups"].as_array().unwrap().len(), 1);

    let (s, j) = r.put(settings(saved.path())).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["source"], "saved");
    assert_eq!(j["settings"]["dir"], saved.path().to_str().unwrap());
    assert_eq!(j["settings"]["every_s"], 86400);
    assert_eq!(j["settings"]["keep"], 5);
    assert_eq!(j["startup"]["dir"], startup.path().to_str().unwrap());
    // The new folder is empty: a backup is made there, and the start-up folder is left alone.
    until("a backup in the saved folder", || {
        names(saved.path()).len() == 1
    })
    .await;
    assert_eq!(names(startup.path()).len(), 1);
    let j = r.get().await;
    assert_eq!(j["backups"][0]["name"], names(saved.path())[0].as_str());

    // Back to the start-up options: its folder is in force again and fresh, so nothing new.
    let (s, j) =
        r.e.send(&r.admin, Method::DELETE, "/api/admin/backups", json!({}))
            .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["source"], "startup");
    assert_eq!(j["settings"], Value::Null);
    assert_eq!(j["backups"][0]["name"], names(startup.path())[0].as_str());
    assert_eq!(r.ctl.current().unwrap().dir, startup.path());
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(names(startup.path()).len(), 1);
    assert_eq!(names(saved.path()).len(), 1);
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM settings WHERE key LIKE 'backup\\_%' ESCAPE '\\'")
            .fetch_one(&r.e.pool)
            .await
            .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn nothing_set_means_off() {
    let r = rig(None).await;
    let j = r.get().await;
    assert_eq!(j["source"], "none");
    assert_eq!(j["settings"], Value::Null);
    assert_eq!(j["startup"], Value::Null);
    assert_eq!(j["status"]["next_at"], Value::Null);
    assert_eq!(j["status"]["running"], false);
    assert_eq!(j["backups"], json!([]));
    let (s, j) = r.post_run().await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{j}");
}

#[tokio::test]
async fn every_bad_value_is_refused_with_a_plain_reason_and_nothing_is_saved() {
    let r = rig(None).await;
    let data = r.e.dir.path().to_str().unwrap().to_owned();
    let out = tempfile::tempdir().unwrap();
    let good = out.path().to_str().unwrap();
    let inside = r.e.dir.path().join("files");
    std::fs::create_dir_all(&inside).unwrap();
    let parent =
        r.e.dir
            .path()
            .parent()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
    let long = format!("/{}", "a".repeat(1100));
    let missing = out.path().join("not-there");
    let ro = tempfile::tempdir().unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(ro.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let mut cases: Vec<(&str, Value, &str)> = vec![
        ("relative", json!({"dir": "backups"}), "dir"),
        ("empty", json!({"dir": ""}), "dir"),
        ("missing", json!({"dir": missing.to_str().unwrap()}), "dir"),
        (
            "inside data",
            json!({"dir": inside.to_str().unwrap()}),
            "dir",
        ),
        ("data dir itself", json!({"dir": data}), "dir"),
        ("contains data", json!({"dir": parent}), "dir"),
        ("too long", json!({"dir": long}), "dir"),
        (
            "every short",
            json!({"dir": good, "every_s": 3599}),
            "every_s",
        ),
        (
            "every long",
            json!({"dir": good, "every_s": 2_592_001}),
            "every_s",
        ),
        ("every zero", json!({"dir": good, "every_s": 0}), "every_s"),
        ("keep zero", json!({"dir": good, "keep": 0}), "keep"),
        ("keep many", json!({"dir": good, "keep": 366}), "keep"),
    ];
    // A folder the server cannot write to (not when the tests run as root).
    if std::fs::File::create(ro.path().join("probe")).is_err() {
        cases.push((
            "read-only",
            json!({"dir": ro.path().to_str().unwrap()}),
            "dir",
        ));
    }
    for (what, over, field) in cases {
        let mut body = settings(out.path());
        for (k, v) in over.as_object().unwrap() {
            body[k] = v.clone();
        }
        let (s, j) = r.put(body).await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{what}: {j}");
        assert_eq!(j["field"], field, "{what}: {j}");
        let text = j["error"].as_str().unwrap();
        assert!(!text.is_empty(), "{what}");
        assert!(
            !text.contains(&data),
            "{what}: the data dir path leaked: {text}"
        );
    }
    let (_, j) = r
        .put({
            let mut b = settings(&missing);
            b["dir"] = json!(missing.to_str().unwrap());
            b
        })
        .await;
    assert!(j["error"].as_str().unwrap().contains("mount a folder"));
    assert_eq!(r.get().await["settings"], Value::Null);
    assert!(names(out.path()).is_empty());
    std::fs::set_permissions(ro.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    // The probe file does not stay behind in a good folder.
    let (s, _) = r.put(settings(out.path())).await;
    assert_eq!(s, StatusCode::OK);
    assert!(!out.path().join(".nimrag-write-test").exists());
    // Unknown fields are refused like everywhere.
    let mut body = settings(out.path());
    body["extra"] = json!(1);
    assert_eq!(r.put(body).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn saving_with_an_empty_folder_makes_a_backup_promptly_and_reports_it() {
    let r = rig(None).await;
    let out = tempfile::tempdir().unwrap();
    let (s, j) = r.put(settings(out.path())).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    until("the first backup", || names(out.path()).len() == 1).await;
    until("the status", || {
        r.ctl.status().last_ok.is_some() && !r.ctl.status().running
    })
    .await;
    let j = r.get().await;
    let name = names(out.path())[0].clone();
    assert_eq!(j["status"]["last_ok"]["name"], name.as_str());
    assert_eq!(j["status"]["last_error"], Value::Null);
    assert_eq!(j["status"]["running"], false);
    let now = chrono::Utc::now().timestamp();
    let next = j["status"]["next_at"].as_i64().unwrap();
    assert!(
        (next - now - 86400).abs() < 60,
        "next backup at {next}, now {now}"
    );
    let b = &j["backups"][0];
    assert_eq!(b["name"], name.as_str());
    assert!(b["db_bytes"].as_u64().unwrap() > 0);
    assert!(b["files"].is_u64() && b["files_bytes"].is_u64());
    assert!(b["at"].as_i64().unwrap() > now - 120);
}

#[tokio::test]
async fn switching_off_stops_the_scheduling_and_keeps_the_values() {
    let r = rig(None).await;
    let out = tempfile::tempdir().unwrap();
    r.put(settings(out.path())).await;
    until("the first backup", || names(out.path()).len() == 1).await;
    let mut off = settings(out.path());
    off["enabled"] = json!(false);
    off["every_s"] = json!(7200);
    off["keep"] = json!(9);
    let (s, j) = r.put(off).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["source"], "saved");
    assert_eq!(j["settings"]["enabled"], false);
    assert_eq!(j["settings"]["dir"], out.path().to_str().unwrap());
    assert_eq!(j["settings"]["every_s"], 7200);
    assert_eq!(j["settings"]["keep"], 9);
    assert!(r.ctl.current().is_none());
    until("next_at to clear", || r.ctl.status().next_at.is_none()).await;
    // With no backup in the folder one would be due at once. Off, none is made.
    for n in names(out.path()) {
        std::fs::remove_dir_all(out.path().join(n)).unwrap();
    }
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(names(out.path()).is_empty());
    assert_eq!(r.get().await["status"]["next_at"], Value::Null);
    // Back on: the scheduler follows.
    let (s, _) = r.put(settings(out.path())).await;
    assert_eq!(s, StatusCode::OK);
    until("a backup after switching on", || {
        names(out.path()).len() == 1
    })
    .await;
}

#[tokio::test]
async fn backups_can_be_switched_off_when_the_folder_has_gone() {
    let r = rig(None).await;
    let gone = tempfile::tempdir().unwrap();
    let path = gone.path().to_path_buf();
    r.put(settings(&path)).await;
    until("the first backup", || names(&path).len() == 1).await;
    drop(gone);
    let mut off = settings(&path);
    off["enabled"] = json!(false);
    let (s, j) = r.put(off).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(j["settings"]["enabled"], false);
    // Off with no folder at all is fine too (nothing is configured yet).
    let mut blank = settings(&path);
    blank["enabled"] = json!(false);
    blank["dir"] = json!("");
    assert_eq!(r.put(blank).await.0, StatusCode::OK);
}

#[tokio::test]
async fn run_now_answers_202_then_409_while_one_runs_and_the_list_grows() {
    let r = rig(None).await;
    let out = tempfile::tempdir().unwrap();
    // Off, but with a folder: Back up now still works.
    let mut off = settings(out.path());
    off["enabled"] = json!(false);
    assert_eq!(r.put(off).await.0, StatusCode::OK);
    let (s, j) = r.post_run().await;
    assert_eq!(s, StatusCode::ACCEPTED, "{j}");
    until("the backup by hand", || {
        names(out.path()).len() == 1 && !r.ctl.status().running
    })
    .await;
    let st = r.get().await["status"].clone();
    assert!(st["last_ok"]["copied_bytes"].is_u64());
    assert_eq!(st["next_at"], Value::Null);

    // While one is running, another is refused.
    let busy = r.ctl.try_begin().unwrap();
    assert_eq!(r.get().await["status"]["running"], true);
    let (s, j) = r.post_run().await;
    assert_eq!(s, StatusCode::CONFLICT, "{j}");
    assert!(j["error"].as_str().unwrap().contains("already running"));
    drop(busy);
    assert_eq!(r.get().await["status"]["running"], false);

    let (s, _) = r.post_run().await;
    assert_eq!(s, StatusCode::ACCEPTED);
    until("the second backup", || {
        names(out.path()).len() == 2 && !r.ctl.status().running
    })
    .await;
    let j = r.get().await;
    let list = j["backups"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    let on_disk = names(out.path());
    // Newest first.
    assert_eq!(list[0]["name"], on_disk[1].as_str());
    assert_eq!(list[1]["name"], on_disk[0].as_str());
    for b in list {
        assert!(b["db_bytes"].as_u64().unwrap() > 0);
    }
}

#[tokio::test]
async fn run_now_prunes_to_the_saved_keep_count() {
    let r = rig(None).await;
    let out = tempfile::tempdir().unwrap();
    let mut off = settings(out.path());
    off["enabled"] = json!(false);
    off["keep"] = json!(1);
    r.put(off).await;
    for n in 1..=2 {
        assert_eq!(r.post_run().await.0, StatusCode::ACCEPTED);
        until("a backup by hand", || {
            !r.ctl.status().running && r.ctl.status().last_ok.is_some()
        })
        .await;
        let _ = n;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(names(out.path()).len(), 1);
}

#[tokio::test]
async fn a_failed_backup_is_reported_without_the_data_dir_path() {
    let r = rig(None).await;
    let data = r.e.dir.path().to_str().unwrap().to_owned();
    // A dangling link in the file store makes every backup fail, and the error names its path.
    let broken = r.e.dir.path().join("files/ef/gh");
    std::fs::create_dir_all(&broken).unwrap();
    std::os::unix::fs::symlink("/nonexistent", broken.join("ef".repeat(32))).unwrap();
    let out = tempfile::tempdir().unwrap();
    let (s, _) = r.put(settings(out.path())).await;
    assert_eq!(s, StatusCode::OK);
    until("the failure", || r.ctl.status().last_error.is_some()).await;
    let j = r.get().await;
    let text = j["status"]["last_error"]["text"].as_str().unwrap();
    assert!(!text.is_empty());
    assert!(!text.contains(&data), "the data dir path leaked: {text}");
    assert!(j["status"]["last_error"]["at"].as_i64().unwrap() > 0);
    assert!(names(out.path()).is_empty());
    // The next success clears the error.
    std::fs::remove_file(broken.join("ef".repeat(32))).unwrap();
    assert_eq!(r.post_run().await.0, StatusCode::ACCEPTED);
    until("the backup by hand", || {
        r.ctl.status().last_error.is_none() && names(out.path()).len() == 1
    })
    .await;
}

#[tokio::test]
async fn an_unreadable_saved_value_is_ignored_and_the_start_up_options_apply() {
    let startup = tempfile::tempdir().unwrap();
    let r = rig(Some(startup_cfg(startup.path()))).await;
    r.e.exec("INSERT INTO settings (key, value) VALUES ('backup_dir', '/x'), ('backup_enabled', 'maybe')")
        .await;
    r.ctl.reload(&r.e.pool).await.unwrap();
    assert_eq!(r.ctl.current().unwrap().dir, startup.path());
    assert_eq!(r.get().await["source"], "startup");
}
