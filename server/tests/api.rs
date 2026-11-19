mod common;

use axum::http::{Method, StatusCode};
use common::*;
use nimrag_server::{derive, sha256_hex};

#[tokio::test]
async fn health_needs_no_auth() {
    let e = env().await;
    let (s, _) = e.request(Method::GET, "/api/health", None, vec![]).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn upload_requires_valid_token() {
    let e = env().await;
    let body = b"abc".to_vec();
    let (s, _) = e
        .request(Method::POST, "/api/files", None, body.clone())
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = e
        .request(Method::POST, "/api/files", Some("nrg_wrong"), body.clone())
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = e
        .request(
            Method::GET,
            &format!("/api/files/{}", sha256_hex(&body)),
            None,
            vec![],
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // Nothing was stored.
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn revoked_token_is_rejected() {
    let e = env().await;
    assert!(nimrag_server::tokens::revoke(&e.pool, 1).await.unwrap());
    let (s, _) = e.upload("data_type=4", b"abc").await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn token_is_stored_only_as_hash() {
    let e = env().await;
    let stored: String = sqlx::query_scalar("SELECT hash FROM tokens")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_ne!(stored, e.token);
    assert_eq!(stored, nimrag_server::tokens::hash_token(&e.token));
}

#[tokio::test]
async fn upload_is_idempotent_by_hash() {
    let e = env().await;
    let body = b"not a fit file";
    let (s, j) = e.upload("data_type=4&source=watch", body).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(j["created"], true);
    assert_eq!(j["sha256"], sha256_hex(body));
    assert_eq!(j["size"], body.len());
    let (s, j) = e.upload("data_type=4&source=watch", body).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(j["created"], false);
    let files: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    let uploads: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM uploads")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!((files, uploads), (1, 1)); // identical repeat folded
    let repeats: i64 = sqlx::query_scalar("SELECT repeat_count FROM uploads")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(repeats, 1);
    let sc: serde_json::Value = serde_json::from_slice(
        &std::fs::read(stored_path(e.dir.path(), &sha256_hex(body)).with_extension("json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(sc["uploads"].as_array().unwrap().len(), 1);
    assert_eq!(sc["uploads"][0]["repeat_count"], 1);
}

#[tokio::test]
async fn metadata_recorded_and_conflicts_never_overwrite_files() {
    let e = env().await;
    let body = b"payload";
    e.upload(
        "data_type=4&sub_type=1&file_index=7&file_number=9&watch_timestamp=123&source=watch",
        body,
    )
    .await;
    e.upload("data_type=20&sub_type=2&source=usb", body).await;

    type Meta = (
        String,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
    );
    let row: Meta = sqlx::query_as(
        "SELECT source, data_type, sub_type, file_index, file_number, watch_timestamp FROM files",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            "watch".into(),
            Some(4),
            Some(1),
            Some(7),
            Some(9),
            Some(123)
        )
    );

    let ups: Vec<(String, Option<i64>, Option<i64>, i64)> =
        sqlx::query_as("SELECT source, data_type, file_index, created FROM uploads ORDER BY id")
            .fetch_all(&e.pool)
            .await
            .unwrap();
    assert_eq!(
        ups,
        vec![
            ("watch".into(), Some(4), Some(7), 1),
            ("usb".into(), Some(20), None, 0)
        ]
    );
}

#[tokio::test]
async fn bad_source_is_rejected() {
    let e = env().await;
    let (s, _) = e.upload("source=cloud", b"x").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn missing_source_is_rejected() {
    let e = env().await;
    let (s, _) = e
        .request(
            Method::POST,
            "/api/files?data_type=4",
            Some(&e.token),
            b"x".to_vec(),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn file_is_stored_untouched_at_hash_path() {
    let e = env().await;
    let body: Vec<u8> = (0..=255u8).cycle().take(10_000).collect();
    let (_, j) = e.upload("data_type=4", &body).await;
    let sha = j["sha256"].as_str().unwrap();
    assert_eq!(std::fs::read(stored_path(e.dir.path(), sha)).unwrap(), body);
    // No temp files left behind.
    let dir = stored_path(e.dir.path(), sha)
        .parent()
        .unwrap()
        .to_path_buf();
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, vec![sha.to_string(), format!("{sha}.json")]);
    let sidecar: serde_json::Value = serde_json::from_slice(
        &std::fs::read(stored_path(e.dir.path(), sha).with_extension("json")).unwrap(),
    )
    .unwrap();
    assert_eq!(sidecar["uploads"].as_array().unwrap().len(), 1);
    assert_eq!(sidecar["uploads"][0]["device_name"], "test-device");
}

#[tokio::test]
async fn non_fit_bytes_are_stored_but_not_parsed_whatever_the_metadata() {
    let e = env().await;
    let (s, _) = e
        .upload("data_type=128", b"declared FIT but plainly not")
        .await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = e.upload("data_type=4", b"other").await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = e.upload("", b"no metadata at all").await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(dump_derived(&e.pool).await, "");
}

#[tokio::test]
async fn fit_bytes_are_parsed_regardless_of_data_type() {
    let e = env().await;
    let (_, a) = e.upload("", &tiny_fit(1)).await;
    let (_, b) = e.upload("data_type=4", &tiny_fit(2)).await;
    for j in [a, b] {
        let sha = j["sha256"].as_str().unwrap();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fit_messages WHERE sha256 = ?")
            .bind(sha)
            .fetch_one(&e.pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
    }
}

#[tokio::test]
async fn reparse_picks_up_fit_stored_without_metadata_and_ignores_non_fit() {
    let e = env().await;
    e.upload("", &tiny_fit(1)).await;
    e.upload("", b"not fit").await;
    let before = dump_derived(&e.pool).await;
    for q in [
        "DELETE FROM fit_messages",
        "DELETE FROM fit_files",
        "DELETE FROM file_parse",
    ] {
        sqlx::query(q).execute(&e.pool).await.unwrap();
    }
    let r = derive::reparse(&e.pool, e.dir.path()).await.unwrap();
    assert_eq!((r.files, r.fit_inputs), (2, 1));
    assert_eq!(dump_derived(&e.pool).await, before);
    assert!(before.contains("fit_messages"));
}

#[tokio::test]
async fn retry_after_partial_failure_repairs_derived_rows() {
    use nimrag_server::ledger::{self, Meta, Sidecar};
    let e = env().await;
    let bytes = tiny_fit(9);
    let sha = sha256_hex(&bytes);
    // State left by an interrupted upload: blob and sidecar exist, database is empty.
    let blob = stored_path(e.dir.path(), &sha);
    std::fs::create_dir_all(blob.parent().unwrap()).unwrap();
    std::fs::write(&blob, &bytes).unwrap();
    let meta = Meta {
        source: "import".into(),
        data_type: Some(128),
        sub_type: None,
        file_index: None,
        file_number: None,
        watch_timestamp: None,
    };
    ledger::write_sidecar(
        e.dir.path(),
        &Sidecar {
            sha256: sha.clone(),
            size: bytes.len() as i64,
            received_at: "2026-01-01T00:00:00.000Z".into(),
            meta,
            uploads: vec![],
        },
    )
    .unwrap();
    let (s, j) = e.upload("data_type=128", &bytes).await;
    assert_eq!(
        (s, &j["created"]),
        (StatusCode::OK, &serde_json::json!(false))
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fit_messages WHERE sha256 = ?")
        .bind(&sha)
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    let f: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(f, 1);
}

#[tokio::test]
async fn sweep_removes_incoming_leftovers() {
    let e = env().await;
    e.upload("data_type=4", b"x").await;
    let incoming = e.dir.path().join("files/.incoming");
    std::fs::write(incoming.join("leftover"), b"partial").unwrap();
    assert_eq!(
        nimrag_server::store::sweep_incoming(e.dir.path()).unwrap(),
        1
    );
    assert_eq!(std::fs::read_dir(incoming).unwrap().count(), 0);
}

#[tokio::test]
async fn truncated_fit_header_is_stored_with_parse_status_not_rejected() {
    let e = env().await;
    let good = tiny_fit(3);
    let (s, j) = e.upload("data_type=128", &good[..14]).await; // header + a few bytes
    assert_eq!(s, StatusCode::CREATED);
    let sha = j["sha256"].as_str().unwrap();
    let (s, info) = e
        .request(
            Method::GET,
            &format!("/api/files/{sha}"),
            Some(&e.token),
            vec![],
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(info["fit"]["parse_status"].is_string(), "{info}");
}

#[tokio::test]
async fn fit_upload_populates_derived_tables_and_summary() {
    let e = env().await;
    let (_, j) = e
        .upload("data_type=128&source=watch", &tiny_fit(1000))
        .await;
    let sha = j["sha256"].as_str().unwrap();
    let (s, info) = e
        .request(
            Method::GET,
            &format!("/api/files/{sha}"),
            Some(&e.token),
            vec![],
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(info["fit"]["fit_files"], 1, "{info}"); // needs a working decoder
    assert_eq!(info["fit"]["messages"], 1);
    assert_eq!(info["fit"]["crc_ok"], true);
    assert_eq!(info["fit"]["parse_error"], serde_json::Value::Null);
    assert_eq!(info["uploads"], 1);
}

#[tokio::test]
async fn file_info_unknown_and_malformed() {
    let e = env().await;
    let (s, _) = e
        .request(
            Method::GET,
            &format!("/api/files/{}", "0".repeat(64)),
            Some(&e.token),
            vec![],
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = e
        .request(Method::GET, "/api/files/zz", Some(&e.token), vec![])
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn oversized_body_is_rejected_and_not_stored() {
    let e = env_with_limit(1024).await;
    let (s, _) = e.upload("data_type=4", &vec![7u8; 1025]).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    let (s, _) = e.upload("data_type=4", &vec![7u8; 1024]).await;
    assert_eq!(s, StatusCode::CREATED);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

async fn ingest_many(e: &Env) {
    for t in [10u32, 20, 30] {
        e.upload("data_type=128", &tiny_fit(t)).await;
    }
    e.upload("data_type=128", &[tiny_fit(40), tiny_fit(50)].concat())
        .await; // chained
    e.upload("data_type=128", b"garbage").await;
    e.upload("data_type=4", b"other").await;
}

#[tokio::test]
async fn reparse_is_identical_and_rebuilds_deleted_tables() {
    let e = env().await;
    ingest_many(&e).await;
    let before = dump_derived(&e.pool).await;
    assert!(
        before.contains("fit_messages"),
        "needs a working decoder:\n{before}"
    );

    let r1 = derive::reparse(&e.pool, e.dir.path()).await.unwrap();
    assert_eq!(dump_derived(&e.pool).await, before);
    let r2 = derive::reparse(&e.pool, e.dir.path()).await.unwrap();
    assert_eq!(r1, r2);
    assert_eq!(r1.files, 6);
    assert_eq!(r1.fit_inputs, 4);
    assert_eq!(r1.fit_files, 5); // 3 single + 2 chained; garbage is not FIT
    assert_eq!((r1.parse_errors, r1.unreadable), (0, 0));

    for q in [
        "DELETE FROM fit_messages",
        "DELETE FROM fit_files",
        "DELETE FROM file_parse",
    ] {
        sqlx::query(q).execute(&e.pool).await.unwrap();
    }
    derive::reparse(&e.pool, e.dir.path()).await.unwrap();
    assert_eq!(dump_derived(&e.pool).await, before);
}

#[tokio::test]
async fn reparse_records_unreadable_blob_and_continues() {
    let e = env().await;
    let (_, bad) = e.upload("data_type=128", &tiny_fit(1)).await;
    e.upload("data_type=128", &tiny_fit(2)).await;
    let sha = bad["sha256"].as_str().unwrap();
    std::fs::write(stored_path(e.dir.path(), sha), b"tampered").unwrap();
    let r = derive::reparse(&e.pool, e.dir.path()).await.unwrap();
    assert_eq!((r.files, r.unreadable, r.fit_inputs), (2, 1, 1));
    let (status, err): (String, String) =
        sqlx::query_as("SELECT status, error FROM file_parse WHERE sha256 = ?")
            .bind(sha)
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert_eq!(status, "error");
    assert!(err.contains("corrupt"), "{err}");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fit_messages")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, 1); // the other file was still parsed
}

#[tokio::test]
async fn client_supplied_sha256_is_verified() {
    let e = env().await;
    let (s, _) = e
        .upload(&format!("sha256={}", "0".repeat(64)), b"abc")
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = e.upload("sha256=nothex", b"abc").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    // The rejected temp file must not linger in the store.
    let incoming = e.dir.path().join("files/.incoming");
    assert_eq!(std::fs::read_dir(incoming).unwrap().count(), 0);
    let (s, _) = e
        .upload(&format!("sha256={}", sha256_hex(b"abc")), b"abc")
        .await;
    assert_eq!(s, StatusCode::CREATED);
}

#[tokio::test]
async fn upload_through_router_creates_raw_message_rows() {
    let e = env().await;
    let (_, j) = e
        .upload("data_type=128&source=watch", &tiny_fit(1000))
        .await;
    let sha = j["sha256"].as_str().unwrap();
    let row: (i64, i64, Option<i64>, String) = sqlx::query_as(
        "SELECT chain_idx, global, timestamp, fields FROM fit_messages WHERE sha256 = ?",
    )
    .bind(sha)
    .fetch_one(&e.pool)
    .await
    .expect("raw message row (needs working decoder)");
    assert_eq!((row.0, row.1, row.2), (0, 0, None));
    let fields: serde_json::Value = serde_json::from_str(&row.3).unwrap();
    assert_eq!(fields["0"]["v"], 4);
    assert_eq!(fields["4"]["v"], 1000);
    assert_eq!(fields["4"]["t"], "uint32");
    assert_eq!(fields["4"]["raw"], "e8030000");
    let keys: Vec<_> = fields.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["0", "4"]); // definition order
    let (ver, status, crc): (i64, String, String) =
        sqlx::query_as("SELECT parser_version, status, crc_status FROM file_parse")
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert_eq!(
        (ver, status.as_str(), crc.as_str()),
        (derive::PARSER_VERSION, "ok", "ok")
    );
}

#[tokio::test]
async fn bad_crc_and_truncated_fit_are_stored_and_status_recorded() {
    let e = env().await;
    let mut bad = tiny_fit(5);
    *bad.last_mut().unwrap() ^= 0xff;
    let (s, _) = e.upload("data_type=128", &bad).await;
    assert_eq!(s, StatusCode::CREATED);
    let crc: String = sqlx::query_scalar("SELECT crc_status FROM file_parse")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(crc, "mismatch");

    let good = tiny_fit(6);
    let (s, j) = e.upload("data_type=128", &good[..good.len() - 5]).await;
    assert_eq!(s, StatusCode::CREATED);
    let sha = j["sha256"].as_str().unwrap();
    assert!(stored_path(e.dir.path(), sha).exists());
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM file_parse WHERE sha256 = ?")
        .bind(sha)
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn token_last_used_is_stamped() {
    let e = env().await;
    let before: Option<String> = sqlx::query_scalar("SELECT last_used_at FROM tokens")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert!(before.is_none());
    e.upload("data_type=4", b"x").await;
    let after: Option<String> = sqlx::query_scalar("SELECT last_used_at FROM tokens")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert!(after.is_some());
}

#[tokio::test]
async fn ledger_rebuilds_from_sidecars_and_reparse_leaves_it_alone() {
    let e = env().await;
    ingest_many(&e).await;
    e.upload("data_type=128", &tiny_fit(10)).await; // duplicate upload event
    let ledger = dump_ledger(&e.pool).await;
    let derived = dump_derived(&e.pool).await;

    derive::reparse(&e.pool, e.dir.path()).await.unwrap();
    assert_eq!(dump_ledger(&e.pool).await, ledger);

    for q in [
        "DELETE FROM fit_messages",
        "DELETE FROM fit_files",
        "DELETE FROM file_parse",
        "DELETE FROM file_derive",
        "DELETE FROM sources",
        "DELETE FROM uploads",
        "DELETE FROM files",
    ] {
        sqlx::query(q).execute(&e.pool).await.unwrap();
    }
    let r = nimrag_server::ledger::rebuild(&e.pool, e.dir.path())
        .await
        .unwrap();
    assert_eq!((r.sidecars, r.files_added, r.uploads), (6, 6, 6));
    assert_eq!(dump_ledger(&e.pool).await, ledger);
    derive::reparse(&e.pool, e.dir.path()).await.unwrap();
    assert_eq!(dump_derived(&e.pool).await, derived);
}

#[tokio::test]
async fn file_derive_and_sources_are_written_for_fit_files() {
    let e = env().await;
    let (_, j) = e.upload("data_type=128", &tiny_fit(1234)).await;
    let sha = j["sha256"].as_str().unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM file_derive WHERE sha256 = ?")
        .bind(sha)
        .fetch_one(&e.pool)
        .await
        .unwrap();
    let sources: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sources WHERE sha256 = ?")
        .bind(sha)
        .fetch_one(&e.pool)
        .await
        .unwrap();
    if nimrag_core::fit::profile::available() {
        assert_eq!((status.as_str(), sources), ("ok", 1));
        let (ok, src): (i64, i64) = sqlx::query_as("SELECT ok, src_id FROM sources")
            .fetch_one(&e.pool)
            .await
            .unwrap();
        assert_eq!((ok, src), (1, nimrag_server::derived::src_id(sha)));
    } else {
        // Raw tables are still filled; derived ones are not.
        assert_eq!((status.as_str(), sources), ("no_profile", 0));
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fit_messages")
            .fetch_one(&e.pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
    }
}
