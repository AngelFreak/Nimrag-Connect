//! The server's history on the phone's copy (docs/design/history-on-phone.md): files the phone
//! lacks are copied in (a Connect envelope and a FIT), a wrong hash is refused, a repeat changes
//! nothing, and a smaller window removes the copies outside it but never the watch's own files.

mod common;

use nimrag_core::derive::test_support::{FitBuilder, Fv};
use nimrag_server::connect::{self, Kind};
use nimrag_server::db;
use nimrag_server::embedded::{self, Embedded};
use nimrag_server::ledger::Meta;
use nimrag_server::store;
use sqlx::SqlitePool;

const SECRET: &str = "0123456789abcdef0123456789abcdef";

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Run `f` against the data dir's database while no embedded server has it open.
fn with_db<T>(dir: &std::path::Path, f: impl AsyncFnOnce(&SqlitePool) -> T) -> T {
    rt().block_on(async {
        let pool = db::open(dir).await.unwrap();
        let out = f(&pool).await;
        pool.close().await;
        out
    })
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A Connect envelope with one day of totals (`steps` on `date`).
fn connect_file(date: &str, steps: u32) -> Vec<u8> {
    connect::wrap(
        Kind::Stats,
        "Europe/Copenhagen".parse().unwrap(),
        &format!(r#"{{"days": [{{"date": "{date}", "steps": {steps}, "has_data": true}}]}}"#),
    )
}

fn fit_file(t: u32) -> Vec<u8> {
    let mut b = FitBuilder::new();
    for i in 0..5u32 {
        b = b.message(55, &[(253, Fv::U32(t + i)), (27, Fv::U8(60 + i as u8))]);
    }
    b.build()
}

fn server_meta(source: &str) -> Meta {
    Meta {
        source: source.into(),
        data_type: None,
        sub_type: None,
        file_index: None,
        file_number: None,
        watch_timestamp: None,
    }
}

fn watch_meta() -> Meta {
    Meta {
        source: "watch".into(),
        data_type: Some(128),
        sub_type: Some(4),
        file_index: Some(1),
        file_number: Some(1),
        watch_timestamp: None,
    }
}

fn copy(e: &Embedded, bytes: &[u8], source: &str, last_ts: Option<i64>) -> bool {
    e.ingest_history(
        bytes,
        &nimrag_server::sha256_hex(bytes),
        server_meta(source),
        last_ts,
    )
    .unwrap()
}

#[test]
fn missing_files_names_what_the_copy_lacks() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let have = connect_file("2026-03-11", 100);
    let other = connect_file("2026-03-12", 200);
    let (a, b) = (
        nimrag_server::sha256_hex(&have),
        nimrag_server::sha256_hex(&other),
    );
    assert_eq!(e.missing_files(&[]).unwrap(), Vec::<String>::new());
    assert_eq!(
        e.missing_files(&[a.clone(), b.clone()]).unwrap(),
        [a.clone(), b.clone()]
    );
    copy(&e, &have, "connect", None);
    assert_eq!(
        e.missing_files(&[a.clone(), b.clone()]).unwrap(),
        [b.clone()]
    );
    // More than one query's worth.
    let many: Vec<String> = (0..1200).map(|i| format!("{i:064x}")).collect();
    assert_eq!(e.missing_files(&many).unwrap(), many);
    // The phone's own files count as present.
    let own = fit_file(1_100_000_000);
    assert!(e.ingest_bytes(&own, watch_meta()).unwrap());
    let own_sha = nimrag_server::sha256_hex(&own);
    assert!(e.missing_files(&[own_sha]).unwrap().is_empty());
    e.stop();
}

#[test]
fn a_connect_envelope_and_a_fit_are_copied_and_derived() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let uid = e.user_id();
    let conn = connect_file("2026-03-11", 4321);
    let fit = fit_file(1_100_000_000);
    assert!(copy(&e, &conn, "connect", Some(1_773_000_000)));
    assert!(copy(&e, &fit, "import", Some(1_100_000_004)));
    // Again: nothing new, nothing doubled.
    assert!(!copy(&e, &conn, "connect", Some(1_773_000_000)));
    assert!(!copy(&e, &fit, "import", Some(1_100_000_004)));
    e.stop();

    let cs = nimrag_server::sha256_hex(&conn);
    let fs = nimrag_server::sha256_hex(&fit);
    with_db(dir.path(), async |pool| {
        assert_eq!(count(pool, "SELECT COUNT(*) FROM files").await, 2);
        let owned = format!("SELECT COUNT(*) FROM file_owners WHERE user_id = {uid}");
        assert_eq!(count(pool, &owned).await, 2, "owned by the local user");
        let kinds: Vec<(String, String)> =
            sqlx::query_as("SELECT source, sha256 FROM files ORDER BY source")
                .fetch_all(pool)
                .await
                .unwrap();
        assert_eq!(
            kinds,
            [
                ("connect".into(), cs.clone()),
                ("import".into(), fs.clone())
            ]
        );
        // The envelope derived to its day's total, as a Connect source.
        let src =
            format!("SELECT COUNT(*) FROM sources WHERE sha256 = '{cs}' AND origin = 'connect'");
        assert_eq!(count(pool, &src).await, 1);
        assert_eq!(
            count(pool, "SELECT COUNT(*) FROM daily_totals WHERE steps = 4321").await,
            1
        );
        // The FIT was parsed and derived (or, without the generated profile, recorded as such).
        let parse = format!("SELECT COUNT(*) FROM file_parse WHERE sha256 = '{fs}'");
        assert_eq!(count(pool, &parse).await, 1);
        let derive = format!("SELECT COUNT(*) FROM file_derive WHERE sha256 = '{fs}'");
        assert_eq!(count(pool, &derive).await, 1);
        // Both are marked as copies, with the server's data time.
        let marks: Vec<(String, Option<i64>)> =
            sqlx::query_as("SELECT sha256, last_ts FROM history_copies ORDER BY last_ts")
                .fetch_all(pool)
                .await
                .unwrap();
        assert_eq!(
            marks,
            [
                (fs.clone(), Some(1_100_000_004)),
                (cs.clone(), Some(1_773_000_000))
            ]
        );
    });
    // The bytes are in the store, as the server has them.
    assert_eq!(store::read_verified(dir.path(), &cs).unwrap(), conn);
}

#[test]
fn a_wrong_hash_is_refused_and_nothing_is_stored() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let bytes = connect_file("2026-03-11", 5);
    let wrong = nimrag_server::sha256_hex(b"something else");
    let err = e
        .ingest_history(&bytes, &wrong, server_meta("connect"), None)
        .unwrap_err();
    assert!(format!("{err:#}").contains("hash"), "{err:#}");
    // Cut short in transit: the same.
    let sha = nimrag_server::sha256_hex(&bytes);
    assert!(e
        .ingest_history(
            &bytes[..bytes.len() - 3],
            &sha,
            server_meta("connect"),
            None
        )
        .is_err());
    // A source the ledger does not know is refused too, and the copy still lacks the file.
    assert!(e
        .ingest_history(&bytes, &sha, server_meta("elsewhere"), None)
        .is_err());
    assert_eq!(e.missing_files(&[sha]).unwrap().len(), 1);
    e.stop();
    with_db(dir.path(), async |pool| {
        assert_eq!(count(pool, "SELECT COUNT(*) FROM files").await, 0);
        assert_eq!(count(pool, "SELECT COUNT(*) FROM daily_totals").await, 0);
    });
}

#[test]
fn pruning_removes_copies_outside_the_window_and_never_the_watchs_own_files() {
    let dir = tempfile::tempdir().unwrap();
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    let old = connect_file("2026-01-10", 1000);
    let recent = connect_file("2026-03-11", 2000);
    let unknown = connect_file("2026-02-01", 3000);
    let own = fit_file(1_100_000_000);
    let both = connect_file("2026-01-11", 4000);
    copy(&e, &old, "connect", Some(1_000));
    copy(&e, &recent, "connect", Some(5_000));
    copy(&e, &unknown, "connect", None);
    copy(&e, &both, "connect", Some(1_000));
    // The phone's own watch file is far older than the window, and is not a copy.
    assert!(e.ingest_bytes(&own, watch_meta()).unwrap());
    // The watch delivering a file the server copy brought first makes it the phone's own.
    assert!(!e.ingest_bytes(&both, watch_meta()).unwrap());

    assert_eq!(
        e.prune_history(Some(3_000)).unwrap(),
        1,
        "only the old copy"
    );
    assert_eq!(
        e.prune_history(Some(3_000)).unwrap(),
        0,
        "nothing more to do"
    );
    let sha = |b: &[u8]| nimrag_server::sha256_hex(b);
    let all = [
        sha(&old),
        sha(&recent),
        sha(&unknown),
        sha(&own),
        sha(&both),
    ];
    assert_eq!(e.missing_files(&all).unwrap(), [sha(&old)]);
    assert!(!store::path_for(dir.path(), &sha(&old)).exists());
    assert!(!store::sidecar_path_for(dir.path(), &sha(&old)).exists());
    assert!(store::path_for(dir.path(), &sha(&recent)).exists());
    e.stop();
    // What the old file derived is gone; what the others derived is still there.
    with_db(dir.path(), async |pool| {
        assert_eq!(
            count(pool, "SELECT COUNT(*) FROM daily_totals WHERE steps = 1000").await,
            0
        );
        assert_eq!(count(pool, "SELECT COUNT(*) FROM daily_totals").await, 3);
        assert_eq!(count(pool, "SELECT COUNT(*) FROM files").await, 4);
        assert_eq!(
            count(pool, "SELECT COUNT(*) FROM uploads").await,
            5,
            "one event each, two for the shared file"
        );
        assert_eq!(count(pool, "SELECT COUNT(*) FROM file_owners").await, 4);
        assert_eq!(count(pool, "SELECT COUNT(*) FROM history_copies").await, 2);
    });

    // Off removes every copy that is left, still keeping the watch's files; copying again later
    // (a widened choice) brings a file back.
    let e = embedded::start(dir.path(), 0, SECRET).unwrap();
    assert_eq!(e.prune_history(None).unwrap(), 2);
    assert_eq!(
        e.missing_files(&all).unwrap(),
        [sha(&old), sha(&recent), sha(&unknown)]
    );
    assert!(copy(&e, &recent, "connect", Some(5_000)));
    e.stop();
    with_db(dir.path(), async |pool| {
        assert_eq!(count(pool, "SELECT COUNT(*) FROM files").await, 3);
        assert_eq!(count(pool, "SELECT COUNT(*) FROM history_copies").await, 1);
        assert_eq!(count(pool, "SELECT COUNT(*) FROM file_parse").await, 1);
    });
}
