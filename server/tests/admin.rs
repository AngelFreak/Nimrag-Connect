//! The admin area: who may use it, user lifecycle, the last-admin rule, deleting users, and the
//! unassigned-data flow (assign tokens and their files to a user).

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use nimrag_core::derive::test_support::{FitBuilder, Fv};
use nimrag_server::users::Role;
use serde_json::{json, Value};

const NOW: i64 = 1_800_000_000;

/// An environment with a data user (`tester`) and an admin (`boss`), the admin signed in.
/// Rows are NOT adopted automatically: these tests are about who owns what.
async fn setup() -> (Env, String, i64) {
    let e = env_at(NOW).await;
    e.adopt.store(false, std::sync::atomic::Ordering::Relaxed);
    let admin_id = e.add_user("boss", Role::Admin).await;
    let cookie = e.login_as("boss").await;
    (e, cookie, admin_id)
}

async fn admin_post(e: &Env, cookie: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    e.send(cookie, Method::POST, uri, body).await
}

async fn count(e: &Env, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(&e.pool)
        .await
        .unwrap()
}

fn hr_file(t: u32, bpm: u8) -> Vec<u8> {
    FitBuilder::new()
        .message(55, &[(253, Fv::U32(t)), (27, Fv::U8(bpm))])
        .build()
}

async fn id_of(e: &Env, name: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(name)
        .fetch_one(&e.pool)
        .await
        .unwrap()
}

// ---- who may use it ------------------------------------------------------------------

#[tokio::test]
async fn only_admins_reach_the_admin_endpoints() {
    let (e, admin, _) = setup().await;
    let user = e.login().await;
    let routes = [
        (Method::GET, "/api/admin/users"),
        (Method::POST, "/api/admin/users"),
        (Method::DELETE, "/api/admin/users/1"),
        (Method::POST, "/api/admin/users/1/reset-password"),
        (Method::POST, "/api/admin/users/1/reset-2fa"),
        (Method::POST, "/api/admin/users/1/disable"),
        (Method::POST, "/api/admin/users/1/enable"),
        (Method::GET, "/api/admin/unassigned"),
        (Method::POST, "/api/admin/unassigned/assign"),
    ];
    for (m, uri) in routes {
        let body = (m == Method::POST)
            .then(|| json!({ "username": "zzz", "token_ids": [], "user_id": 1 }));
        let (s, _, _) = e.call(m.clone(), uri, None, true, body.clone()).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{m} {uri} without a session");
        let (s, _, _) = e.call(m.clone(), uri, Some(&user), true, body).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{m} {uri} as a normal user");
    }
    let (s, _, j) = e
        .call(Method::GET, "/api/admin/users", Some(&admin), false, None)
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(j["users"].as_array().unwrap().len(), 2);
    // The CSRF header is required here as everywhere.
    let (s, _, _) = e
        .call(
            Method::POST,
            "/api/admin/users",
            Some(&admin),
            false,
            Some(json!({ "username": "alice" })),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_admin_has_no_data_and_gets_403_on_every_data_endpoint() {
    let (e, admin, _) = setup().await;
    let user = e.login().await;
    let sha = "0".repeat(64);
    let reads = [
        "/api/today".to_string(),
        "/api/days/2026-11-19".into(),
        "/api/activities".into(),
        format!("/api/activities/{sha}"),
        format!("/api/activities/{sha}/track"),
        format!("/api/activities/{sha}/strength"),
        "/api/sleep/2026-11-18".into(),
        "/api/sleep?nights=7".into(),
        "/api/trends".into(),
        "/api/movement/2026-11-19".into(),
        "/api/training".into(),
        "/api/body".into(),
        "/api/calendar/2026-11".into(),
        "/api/devices".into(),
        "/api/storage".into(),
        "/api/settings".into(),
        "/api/tokens".into(),
        "/api/garmin".into(),
    ];
    for uri in &reads {
        let (s, _, _) = e.call(Method::GET, uri, Some(&admin), false, None).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "GET {uri} as admin");
        let (s, _, _) = e.call(Method::GET, uri, Some(&user), false, None).await;
        assert!(
            s != StatusCode::FORBIDDEN && s != StatusCode::UNAUTHORIZED,
            "GET {uri} as user: {s}"
        );
    }
    let writes = [
        (
            Method::PATCH,
            "/api/settings".to_string(),
            json!({ "step_goal": 9000 }),
        ),
        (
            Method::POST,
            "/api/body".into(),
            json!({ "weight_kg": 80.0 }),
        ),
        (Method::DELETE, "/api/body/1".into(), json!(null)),
        (
            Method::PATCH,
            format!("/api/activities/{sha}/strength"),
            json!({ "sets": [] }),
        ),
        (
            Method::POST,
            "/api/tokens".into(),
            json!({ "name": "x", "password": PASSWORD }),
        ),
        (Method::DELETE, "/api/tokens/1".into(), json!(null)),
        (
            Method::POST,
            "/api/garmin/login".into(),
            json!({ "email": "a@b.c", "password": "x" }),
        ),
        (
            Method::POST,
            "/api/garmin/mfa".into(),
            json!({ "code": "123456" }),
        ),
        (Method::POST, "/api/garmin/logout".into(), json!(null)),
        (Method::POST, "/api/garmin/retry".into(), json!(null)),
        (
            Method::POST,
            "/api/garmin/backfill".into(),
            json!({ "days": 7 }),
        ),
        (
            Method::POST,
            "/api/garmin/import-tokens".into(),
            json!({ "di_token": "a", "di_refresh_token": "b" }),
        ),
    ];
    for (m, uri, body) in writes {
        let (s, _, _) = e
            .call(m.clone(), &uri, Some(&admin), true, Some(body))
            .await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{m} {uri} as admin");
    }
    // Their own account pages still work.
    let me = e.get(&admin, "/api/me").await;
    assert_eq!(me["role"], "admin");
}

// ---- user lifecycle -----------------------------------------------------------------

#[tokio::test]
async fn a_created_user_gets_a_temporary_password_and_must_enrol_and_choose_their_own() {
    let (e, admin, _) = setup().await;
    let (s, j) = admin_post(
        &e,
        &admin,
        "/api/admin/users",
        json!({ "username": "  Alice " }),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{j}");
    assert_eq!(j["user"]["username"], "alice");
    assert_eq!(j["user"]["role"], "user");
    assert_eq!(
        (
            j["user"]["totp_enabled"].as_bool(),
            j["user"]["must_change_password"].as_bool()
        ),
        (Some(false), Some(true))
    );
    let temp = j["temporary_password"].as_str().unwrap().to_owned();
    assert!(temp.len() >= 16, "{temp}");
    // Only a hash is stored.
    let stored: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE username = 'alice'")
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert!(stored.starts_with("$argon2id$") && !stored.contains(&temp));
    // The list never shows secrets.
    let (_, _, list) = e
        .call(Method::GET, "/api/admin/users", Some(&admin), false, None)
        .await;
    let text = list.to_string();
    assert!(!text.contains(&temp) && !text.contains("argon2") && !text.contains("secret"));

    // Taken (case-insensitive) and invalid names.
    let (s, _) = admin_post(
        &e,
        &admin,
        "/api/admin/users",
        json!({ "username": "ALICE" }),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    for bad in ["al", "has space", "x".repeat(33).as_str(), "ünï"] {
        let (s, _) = admin_post(&e, &admin, "/api/admin/users", json!({ "username": bad })).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
    }

    // The first login: temporary password -> enrol 2FA while choosing a password.
    let (s, _, step) = e
        .call(
            Method::POST,
            "/api/login",
            None,
            false,
            Some(json!({ "username": "alice", "password": temp })),
        )
        .await;
    assert_eq!(
        (
            s,
            step["step"].as_str(),
            step["must_change_password"].as_bool()
        ),
        (StatusCode::OK, Some("enrol2fa"), Some(true))
    );
    let secret = step["secret"].as_str().unwrap();
    let code = nimrag_server::totp::code_at(secret, NOW).unwrap();
    let (s, _, j) = e
        .call(
            Method::POST,
            "/api/login/totp-enrol",
            None,
            true,
            Some(json!({ "pending_token": step["pending_token"], "code": code, "new_password": "alice picks this one" })),
        )
        .await;
    assert_eq!(
        (s, j["role"].as_str()),
        (StatusCode::OK, Some("user")),
        "{j}"
    );
    // Created admins work the same way but are admins.
    let (s, j) = admin_post(
        &e,
        &admin,
        "/api/admin/users",
        json!({ "username": "second-admin", "role": "admin" }),
    )
    .await;
    assert_eq!(
        (s, j["user"]["role"].as_str()),
        (StatusCode::CREATED, Some("admin"))
    );
}

#[tokio::test]
async fn resetting_a_password_forces_a_change_and_ends_sessions_and_trusted_devices() {
    let (e, admin, admin_id) = setup().await;
    let uid = e.user_id;
    let session = e.login().await;
    sqlx::query("UPDATE tokens SET device_secret_hash = 'x' WHERE user_id = ?")
        .bind(uid)
        .execute(&e.pool)
        .await
        .unwrap();

    let (s, j) = admin_post(
        &e,
        &admin,
        &format!("/api/admin/users/{uid}/reset-password"),
        json!(null),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    let temp = j["temporary_password"].as_str().unwrap().to_owned();
    let (s, _, _) = e
        .call(Method::GET, "/api/me", Some(&session), false, None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "the old session ended");
    let secrets = count(
        &e,
        &format!(
            "SELECT COUNT(*) FROM tokens WHERE user_id = {uid} AND device_secret_hash IS NOT NULL"
        ),
    )
    .await;
    assert_eq!(
        secrets, 0,
        "trusted devices must unlock with the new credentials"
    );
    // Old password is dead; the temporary one leads to the change step (code first: 2FA is enrolled).
    let (s, _, _) = e.call(Method::POST, "/api/login", None, false, Some(json!({ "username": USER, "password": PASSWORD, "code": nimrag_server::totp::code_at(TOTP_SECRET, NOW).unwrap() }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, step) = e.call(Method::POST, "/api/login", None, false, Some(json!({ "username": USER, "password": temp, "code": nimrag_server::totp::code_at(TOTP_SECRET, NOW + 30).unwrap() }))).await;
    assert_eq!(
        (s, step["step"].as_str()),
        (StatusCode::OK, Some("change_password"))
    );

    // Not for yourself, not for strangers.
    let (s, _) = admin_post(
        &e,
        &admin,
        &format!("/api/admin/users/{admin_id}/reset-password"),
        json!(null),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = admin_post(
        &e,
        &admin,
        "/api/admin/users/999/reset-password",
        json!(null),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn resetting_2fa_makes_the_user_enrol_again() {
    let (e, admin, admin_id) = setup().await;
    let uid = e.user_id;
    let session = e.login().await;
    let (s, _) = admin_post(
        &e,
        &admin,
        &format!("/api/admin/users/{uid}/reset-2fa"),
        json!(null),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _, _) = e
        .call(Method::GET, "/api/me", Some(&session), false, None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (enabled, secret): (i64, Option<String>) =
        sqlx::query_as("SELECT totp_enabled, totp_secret FROM users WHERE id = ?")
            .bind(uid)
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert_eq!((enabled, secret), (0, None));
    let (s, _, step) = e
        .call(
            Method::POST,
            "/api/login",
            None,
            false,
            Some(json!({ "username": USER, "password": PASSWORD })),
        )
        .await;
    assert_eq!(
        (s, step["step"].as_str()),
        (StatusCode::OK, Some("enrol2fa"))
    );
    let (s, _) = admin_post(
        &e,
        &admin,
        &format!("/api/admin/users/{admin_id}/reset-2fa"),
        json!(null),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn disabling_a_user_ends_everything_and_enabling_restores_it() {
    let (e, admin, _) = setup().await;
    let uid = e.user_id;
    let session = e.login().await;
    let (s, _) = admin_post(
        &e,
        &admin,
        &format!("/api/admin/users/{uid}/disable"),
        json!(null),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _, _) = e
        .call(Method::GET, "/api/me", Some(&session), false, None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // Their phone cannot upload either.
    let (s, _) = e.upload("data_type=4", b"abc").await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = e.call(Method::POST, "/api/login", None, false, Some(json!({ "username": USER, "password": PASSWORD, "code": nimrag_server::totp::code_at(TOTP_SECRET, NOW).unwrap() }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (_, _, list) = e
        .call(Method::GET, "/api/admin/users", Some(&admin), false, None)
        .await;
    let me = list["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == USER)
        .unwrap();
    assert_eq!(me["disabled"], true);

    let (s, _) = admin_post(
        &e,
        &admin,
        &format!("/api/admin/users/{uid}/enable"),
        json!(null),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = e.upload("data_type=4", b"abc").await;
    assert_eq!(s, StatusCode::CREATED);
    e.login().await;
}

#[tokio::test]
async fn the_last_active_admin_cannot_be_disabled_or_deleted() {
    let (e, admin, admin_id) = setup().await;
    let (s, _) = admin_post(
        &e,
        &admin,
        &format!("/api/admin/users/{admin_id}/disable"),
        json!(null),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = e
        .send(
            &admin,
            Method::DELETE,
            &format!("/api/admin/users/{admin_id}"),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    // With a second admin the first may go; then the second is the last.
    let second = e.add_user("backup", Role::Admin).await;
    let (s, _) = admin_post(
        &e,
        &admin,
        &format!("/api/admin/users/{second}/disable"),
        json!(null),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    // A disabled admin does not count as an active one.
    let (s, _) = admin_post(
        &e,
        &admin,
        &format!("/api/admin/users/{admin_id}/disable"),
        json!(null),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = e
        .send(
            &admin,
            Method::DELETE,
            &format!("/api/admin/users/{second}"),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(
        count(&e, "SELECT COUNT(*) FROM users WHERE role = 'admin'").await,
        1
    );
}

#[tokio::test]
async fn deleting_a_user_needs_confirm_when_they_have_data_and_never_deletes_files() {
    let (e, admin, _) = setup().await;
    let empty = e.add_user("empty", Role::User).await;
    let (s, _) = e
        .send(
            &admin,
            Method::DELETE,
            &format!("/api/admin/users/{empty}"),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT, "no data, no confirm needed");

    // The data user has a token and a file.
    let uid = e.user_id;
    let (s, up) = e.upload("data_type=4&source=watch", b"some bytes").await;
    assert_eq!(s, StatusCode::CREATED);
    let sha = up["sha256"].as_str().unwrap().to_owned();
    assert_eq!(
        count(
            &e,
            &format!("SELECT COUNT(*) FROM file_owners WHERE user_id = {uid}")
        )
        .await,
        1
    );
    let (_, _, list) = e
        .call(Method::GET, "/api/admin/users", Some(&admin), false, None)
        .await;
    let info = list["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"] == uid)
        .unwrap();
    assert_eq!(
        (
            info["has_data"].as_bool(),
            info["files"].as_i64(),
            info["active_tokens"].as_i64()
        ),
        (Some(true), Some(1), Some(1))
    );

    let (s, _) = e
        .send(
            &admin,
            Method::DELETE,
            &format!("/api/admin/users/{uid}"),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(
        count(&e, "SELECT COUNT(*) FROM users WHERE username = 'tester'").await,
        1
    );
    let (s, _) = e
        .send(
            &admin,
            Method::DELETE,
            &format!("/api/admin/users/{uid}?confirm=true"),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);

    // The user is gone, their original file is not: it is unassigned now, and so is their token.
    assert_eq!(
        count(&e, "SELECT COUNT(*) FROM users WHERE username = 'tester'").await,
        0
    );
    assert!(
        stored_path(e.dir.path(), &sha).exists(),
        "the original file stays on disk"
    );
    assert_eq!(count(&e, "SELECT COUNT(*) FROM files").await, 1);
    assert_eq!(count(&e, "SELECT COUNT(*) FROM file_owners").await, 0);
    assert_eq!(
        count(
            &e,
            "SELECT COUNT(*) FROM tokens WHERE user_id IS NULL AND revoked_at IS NULL"
        )
        .await,
        1
    );
    let (_, _, un) = e
        .call(
            Method::GET,
            "/api/admin/unassigned",
            Some(&admin),
            false,
            None,
        )
        .await;
    assert_eq!(un["tokens"][0]["files"], 1);
    // The deleted user's name is free again.
    let (s, _) = admin_post(
        &e,
        &admin,
        "/api/admin/users",
        json!({ "username": "tester" }),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
}

// ---- unassigned data ------------------------------------------------------------------

#[tokio::test]
async fn unassigned_tokens_and_their_files_are_assigned_to_a_user() {
    let (e, admin, _) = setup().await;
    let uid = e.user_id;
    // An old phone token that belongs to nobody uploads two files; the tester's own token one.
    let (_, old) = nimrag_server::tokens::create(&e.pool, "old phone", None)
        .await
        .unwrap();
    let up = |bytes: Vec<u8>, tok: String| {
        let e = &e;
        async move {
            e.request(
                Method::POST,
                "/api/files?data_type=128&source=watch",
                Some(&tok),
                bytes,
            )
            .await
        }
    };
    let (s, _) = up(hr_file(1_000_000_000, 61), old.clone()).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = up(hr_file(1_000_000_100, 62), old.clone()).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = up(hr_file(1_000_000_200, 63), e.token.clone()).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(
        count(&e, "SELECT COUNT(*) FROM file_owners").await,
        1,
        "only the owned token's file has an owner"
    );
    let profile = nimrag_core::fit::profile::available();
    if profile {
        assert_eq!(
            count(&e, "SELECT COUNT(*) FROM mon_hr WHERE user_id = 0").await,
            2,
            "unowned samples sit in partition 0"
        );
    }

    let (_, _, un) = e
        .call(
            Method::GET,
            "/api/admin/unassigned",
            Some(&admin),
            false,
            None,
        )
        .await;
    assert_eq!(un["tokens"].as_array().unwrap().len(), 1);
    assert_eq!(
        (
            un["tokens"][0]["name"].as_str(),
            un["tokens"][0]["files"].as_i64()
        ),
        (Some("old phone"), Some(2))
    );
    assert_eq!(un["unattributed_files"], 0);
    let old_id = un["tokens"][0]["id"].as_i64().unwrap();

    // Validation: unknown token, admin as target, nothing to do.
    let (s, _) = admin_post(
        &e,
        &admin,
        "/api/admin/unassigned/assign",
        json!({ "token_ids": [999], "user_id": uid }),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let admin_id = id_of(&e, "boss").await;
    let (s, _) = admin_post(
        &e,
        &admin,
        "/api/admin/unassigned/assign",
        json!({ "token_ids": [old_id], "user_id": admin_id }),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = admin_post(
        &e,
        &admin,
        "/api/admin/unassigned/assign",
        json!({ "token_ids": [], "user_id": uid }),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = admin_post(
        &e,
        &admin,
        "/api/admin/unassigned/assign",
        json!({ "token_ids": [old_id], "user_id": 999 }),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(
        count(&e, "SELECT COUNT(*) FROM file_owners").await,
        1,
        "failed requests change nothing"
    );

    let (s, j) = admin_post(
        &e,
        &admin,
        "/api/admin/unassigned/assign",
        json!({ "token_ids": [old_id], "user_id": uid }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(
        (
            j["tokens_moved"].as_i64(),
            j["files_moved"].as_i64(),
            j["legacy_adopted"].as_bool()
        ),
        (Some(1), Some(2), Some(false))
    );
    assert_eq!(
        count(
            &e,
            &format!("SELECT COUNT(*) FROM file_owners WHERE user_id = {uid}")
        )
        .await,
        3
    );
    assert_eq!(
        count(
            &e,
            &format!("SELECT COUNT(*) FROM tokens WHERE user_id = {uid}")
        )
        .await,
        2
    );
    if profile {
        assert_eq!(
            count(&e, "SELECT COUNT(*) FROM mon_hr WHERE user_id = 0").await,
            0
        );
        assert_eq!(
            count(
                &e,
                &format!("SELECT COUNT(*) FROM mon_hr WHERE user_id = {uid}")
            )
            .await,
            3,
            "derived rows moved with the files"
        );
    }
    let user = e.login().await;
    let st = e.get(&user, "/api/storage").await;
    assert_eq!(st["files"], 3);
    let tokens = e.get(&user, "/api/tokens").await;
    assert!(tokens["tokens"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"] == "old phone"));
    // The old phone's token now uploads as the user: a new file lands in their account.
    let (s, _) = up(hr_file(1_000_000_300, 64), old).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(e.get(&user, "/api/storage").await["files"], 4);
    // Nothing is left unassigned, and assigning again is refused.
    let (_, _, un) = e
        .call(
            Method::GET,
            "/api/admin/unassigned",
            Some(&admin),
            false,
            None,
        )
        .await;
    assert_eq!(un["tokens"].as_array().unwrap().len(), 0);
    let (s, _) = admin_post(
        &e,
        &admin,
        "/api/admin/unassigned/assign",
        json!({ "token_ids": [old_id], "user_id": uid }),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn unattributed_files_and_pre_accounts_records_are_assigned_on_request() {
    let (e, admin, _) = setup().await;
    let uid = e.user_id;
    // A file nobody owns and no unassigned token uploaded (a CLI import), plus pre-accounts records.
    let (s, _) = e
        .request(
            Method::POST,
            "/api/files?data_type=4&source=import",
            Some(&e.token),
            b"mine".to_vec(),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    e.exec("DELETE FROM file_owners").await;
    e.exec("INSERT INTO user_settings (user_id, key, value) VALUES (0, 'step_goal', '12000')")
        .await;
    e.exec("INSERT INTO body_measurements (user_id, measured_at, weight_kg, created_at) VALUES (0, 1700000000, 77.0, 1)").await;
    let (_, _, un) = e
        .call(
            Method::GET,
            "/api/admin/unassigned",
            Some(&admin),
            false,
            None,
        )
        .await;
    assert_eq!(
        (
            un["unattributed_files"].as_i64(),
            un["legacy"]["body_entries"].as_i64(),
            un["legacy"]["settings"].as_bool()
        ),
        (Some(1), Some(1), Some(true))
    );

    let (s, j) = admin_post(&e, &admin, "/api/admin/unassigned/assign", json!({ "token_ids": [], "user_id": uid, "include_unattributed": true, "adopt_legacy": true })).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    assert_eq!(
        (j["files_moved"].as_i64(), j["legacy_adopted"].as_bool()),
        (Some(1), Some(true))
    );
    let user = e.login().await;
    assert_eq!(e.get(&user, "/api/settings").await["step_goal"], 12000);
    assert_eq!(
        e.get(&user, "/api/body").await["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // A user who already has records of their own cannot adopt (no silent merge).
    let other = e.add_user("other", Role::User).await;
    e.exec(&format!("INSERT INTO body_measurements (user_id, measured_at, weight_kg, created_at) VALUES (0, 1700000100, 70.0, 1), ({other}, 1700000200, 60.0, 1)")).await;
    let (s, _) = admin_post(
        &e,
        &admin,
        "/api/admin/unassigned/assign",
        json!({ "token_ids": [], "user_id": other, "adopt_legacy": true }),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(
        count(
            &e,
            "SELECT COUNT(*) FROM body_measurements WHERE user_id = 0"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn deleting_a_user_returns_their_derived_rows_to_the_unassigned_partition() {
    if !nimrag_core::fit::profile::available() {
        return;
    }
    let (e, admin, _) = setup().await;
    let uid = e.user_id;
    let (s, _) = e
        .request(
            Method::POST,
            "/api/files?data_type=128&source=watch",
            Some(&e.token),
            hr_file(1_000_000_000, 70),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(
        count(
            &e,
            &format!("SELECT COUNT(*) FROM mon_hr WHERE user_id = {uid}")
        )
        .await,
        1
    );
    let (s, _) = e
        .send(
            &admin,
            Method::DELETE,
            &format!("/api/admin/users/{uid}?confirm=true"),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(
        count(&e, "SELECT COUNT(*) FROM mon_hr WHERE user_id = 0").await,
        1
    );
    assert_eq!(
        count(
            &e,
            &format!("SELECT COUNT(*) FROM mon_hr WHERE user_id = {uid}")
        )
        .await,
        0
    );
}
