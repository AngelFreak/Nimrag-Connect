//! Garmin Connect intraday series (`import-connect` reading the stress, heart rate and respiration
//! responses of a day) through the real binary and router, on SYNTHETIC files
//! (`tests/fixtures/connect_intraday`, made-up numbers; the DST day is generated here).
//!
//! Fixture days (Copenhagen):
//! - Thu 12 Nov: Connect only. Stress and body battery 08:00 to 08:18 with a -1, a -2 and a null
//!   body battery level, heart rate 03:00 and 08:00 to 08:10 with a null, breathing 02:00, 03:00
//!   and 08:00 to 08:06 with a -1.0.
//! - Fri 13 Nov: the Connect half of a mixed day (08:00, then 21:00 to 21:03); the tests add the
//!   watch's rows from 21:02 on, some at the very instant of a Connect sample.
//! - Sat 14 Nov: heart rate only; the daily summary has the other figures.
//! - `daily_stats/2026-11.json`: wild numbers for the three days; a page that shows them for a
//!   day that has samples would fail.
mod common;

use common::*;
use nimrag_server::derived::{rank_key, src_id, upsert_samples, V};
use nimrag_server::users::Role;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const TZ: &str = "Europe/Copenhagen";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/connect_intraday")
}

fn at(day: u32, h: u32, m: u32) -> i64 {
    cph(2026, 11, day, h, m)
}

fn run_connect(data: &Path, input: &Path, user: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .arg("import-connect")
        .args(["--user", user, "--timezone", TZ])
        .arg("--data-dir")
        .arg(data)
        .arg(input)
        .output()
        .unwrap()
}

fn import(e: &Env, input: &Path) -> String {
    let out = run_connect(e.dir.path(), input, USER);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn reparse(dir: &Path) {
    let out = Command::new(env!("CARGO_BIN_EXE_nimrag-server"))
        .args(["reparse", "--data-dir"])
        .arg(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// The three sample tables as text, in key order (with the origin of each row's source).
async fn dump(pool: &SqlitePool) -> Vec<String> {
    let mut out = Vec::new();
    for (t, cols) in [
        (
            "stress",
            "stress, stress_raw, body_battery, body_battery_status",
        ),
        ("mon_hr", "bpm"),
        ("respiration", "brpm, brpm_raw"),
    ] {
        let expr = cols
            .split(", ")
            .map(|c| format!("quote({c})"))
            .collect::<Vec<_>>()
            .join(" || '|' || ");
        let sql = format!(
            "SELECT '{t}|' || t.user_id || '|' || t.ts || '|' || {expr} || '|' || COALESCE(s.origin, '?')
             FROM {t} t LEFT JOIN sources s ON s.src_id = t.src ORDER BY t.user_id, t.ts"
        );
        out.extend(
            sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql))
                .fetch_all(pool)
                .await
                .unwrap(),
        );
    }
    out
}

/// An environment pinned to `now`, the data user in Copenhagen, the fixture imported.
async fn setup(now: i64) -> (Env, String) {
    let e = env_at(now).await;
    nimrag_server::settings::set_user(&e.pool, e.user_id, "timezone", TZ)
        .await
        .unwrap();
    import(&e, &fixture());
    let cookie = e.login().await;
    (e, cookie)
}

fn f(j: &Value) -> Option<f64> {
    j.as_f64()
}

fn len(j: &Value) -> usize {
    j.as_array().unwrap().len()
}

fn point(j: &Value, key: &str, date: &str) -> Option<f64> {
    j["series"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == key)
        .unwrap_or_else(|| panic!("no series {key}"))["points"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["date"] == date)
        .unwrap_or_else(|| panic!("no point {date} in {key}"))["value"]
        .as_f64()
}

/// A watch file's rows (stress with body battery, heart rate, breathing) at the given instants,
/// written through the same rank-based upsert a FIT file uses.
struct Watch {
    stress: Vec<(i64, f64, f64)>,
    hr: Vec<(i64, f64)>,
    resp: Vec<(i64, f64)>,
}

async fn watch(e: &Env, id: char, w: &Watch) {
    let sha = id.to_string().repeat(64);
    let last = w
        .stress
        .iter()
        .map(|r| r.0)
        .chain(w.hr.iter().map(|r| r.0))
        .max()
        .unwrap();
    e.exec(&format!(
        "INSERT INTO files (sha256, size, received_at, source) VALUES ('{sha}', 1, '2026-11-13T21:30:00.000Z', 'watch')"
    ))
    .await;
    e.exec(&format!(
        "INSERT INTO sources (sha256, src_id, ok, last_ts, msg_count, rank_key) VALUES ('{sha}', {}, 1, {last}, 1, '{}')",
        src_id(&sha),
        rank_key(true, Some(last), 1)
    ))
    .await;
    let (src, uid) = (src_id(&sha), e.user_id);
    let mut tx = e.pool.begin().await.unwrap();
    let rows = w
        .stress
        .iter()
        .map(|(ts, s, b)| {
            vec![
                V::I(Some(*ts)),
                V::R(Some(*s)),
                V::R(Some(*s)),
                V::R(Some(*b)),
            ]
        })
        .collect::<Vec<_>>();
    upsert_samples(
        &mut tx,
        "stress",
        &["ts"],
        &["stress", "stress_raw", "body_battery"],
        src,
        uid,
        rows,
    )
    .await
    .unwrap();
    let rows =
        w.hr.iter()
            .map(|(ts, v)| vec![V::I(Some(*ts)), V::R(Some(*v))])
            .collect::<Vec<_>>();
    upsert_samples(&mut tx, "mon_hr", &["ts"], &["bpm"], src, uid, rows)
        .await
        .unwrap();
    let rows = w
        .resp
        .iter()
        .map(|(ts, v)| vec![V::I(Some(*ts)), V::R(Some(*v)), V::R(Some(*v))])
        .collect::<Vec<_>>();
    upsert_samples(
        &mut tx,
        "respiration",
        &["ts"],
        &["brpm", "brpm_raw"],
        src,
        uid,
        rows,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

/// The watch's evening of Friday 13: its first rows come at 21:02, the 21:03 stress, the 21:02
/// heart rate and the 21:02 breathing at the very instant of a Connect sample (which they beat).
fn friday_evening() -> Watch {
    Watch {
        stress: vec![
            (at(13, 21, 3), 80.0, 36.0),
            (at(13, 21, 6), 10.0, 41.0),
            (at(13, 21, 9), 30.0, 45.0),
        ],
        hr: vec![(at(13, 21, 2), 70.0), (at(13, 21, 4), 95.0)],
        resp: vec![(at(13, 21, 2), 12.0)],
    }
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

// ---- import -----------------------------------------------------------------------------------

#[tokio::test]
async fn the_three_shapes_are_recognised_wherever_they_are_below_the_directory() {
    let e = env_at(at(14, 12, 0)).await;
    let dir = tempfile::tempdir().unwrap();
    // The fixture files, buried at different depths and under names that say nothing.
    copy_tree(&fixture().join("stress"), &dir.path().join("a/b/c"));
    copy_tree(&fixture().join("heart_rate"), &dir.path().join("x"));
    for (i, f) in std::fs::read_dir(fixture().join("respiration"))
        .unwrap()
        .enumerate()
    {
        std::fs::copy(f.unwrap().path(), dir.path().join(format!("resp-{i}.JSON"))).unwrap();
    }
    // Look alike, not the shape: values without their descriptors, and a descriptor-less list.
    std::fs::write(
        dir.path().join("no_desc.json"),
        r#"{"stressValuesArray": [[1, 2]]}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("wrong.json"),
        r#"{"heartRateValues": 5, "heartRateValueDescriptors": []}"#,
    )
    .unwrap();
    let out = import(&e, dir.path());
    assert!(
        out.contains("intraday_stress: 2 new, 0 already there"),
        "{out}"
    );
    assert!(
        out.contains("intraday_heart_rate: 3 new, 0 already there"),
        "{out}"
    );
    assert!(
        out.contains("intraday_respiration: 2 new, 0 already there"),
        "{out}"
    );
    assert!(out.contains("not Connect JSON (not stored): 2"), "{out}");
    assert!(out.contains("errors:               0"), "{out}");
    assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM stress").await, 10);
}

#[tokio::test]
async fn import_fills_the_sample_tables_and_a_rerun_and_a_reparse_change_nothing() {
    let e = env_at(at(14, 12, 0)).await;
    let out = import(&e, &fixture());
    assert!(
        out.contains("intraday_stress: 2 new, 0 already there"),
        "{out}"
    );
    assert!(out.contains("daily_stats: 1 new, 0 already there"), "{out}");
    assert!(out.contains("not Connect JSON (not stored): 0"), "{out}");

    // Every instant with a stress level or a body battery level is a row (7 + 3); a heart rate
    // of null and a breathing of -1.0 are not rows.
    assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM stress").await, 10);
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM mon_hr").await,
        6 + 3 + 2
    );
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM respiration").await,
        5 + 3
    );
    let uid = e.user_id;
    assert_eq!(
        count(
            &e.pool,
            &format!(
                "SELECT COUNT(*) FROM (SELECT user_id FROM stress UNION ALL SELECT user_id FROM mon_hr
                 UNION ALL SELECT user_id FROM respiration) WHERE user_id != {uid}"
            )
        )
        .await,
        0,
        "the import's user owns every row"
    );
    let rows = dump(&e.pool).await;
    assert!(rows.iter().all(|r| r.ends_with("|connect")), "{rows:#?}");
    let row = |t: &str, ts: i64| {
        rows.iter()
            .find(|r| r.starts_with(&format!("{t}|{uid}|{ts}|")))
            .unwrap_or_else(|| panic!("no {t} row at {ts}"))
    };
    // Stress -1 and -2 are no stress value; the body battery at that instant is kept.
    assert!(row("stress", at(12, 8, 6)).ends_with("|NULL|NULL|57.0|NULL|connect"));
    assert!(row("stress", at(12, 8, 9)).ends_with("|NULL|NULL|56.0|NULL|connect"));
    // A null body battery level leaves the stress value alone; stress_raw and the status are
    // the watch's own codes, so they stay empty.
    assert!(row("stress", at(12, 8, 15)).ends_with("|40.0|NULL|NULL|NULL|connect"));
    assert!(row("stress", at(12, 8, 0)).ends_with("|20.0|NULL|60.0|NULL|connect"));
    assert!(row("mon_hr", at(12, 8, 2)).ends_with("|62.0|connect"));
    assert!(
        !rows
            .iter()
            .any(|r| r.starts_with(&format!("mon_hr|{uid}|{}|", at(12, 8, 4)))),
        "a null heart rate is no row"
    );
    assert!(
        !rows
            .iter()
            .any(|r| r.starts_with(&format!("respiration|{uid}|{}|", at(12, 8, 2)))),
        "a breathing of -1.0 is no row"
    );
    assert!(row("respiration", at(12, 8, 4)).ends_with("|16.0|NULL|connect"));
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM file_derive WHERE status != 'ok'"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM sources WHERE origin = 'connect'"
        )
        .await,
        8
    );

    // Again: every blob is there already, nothing changes.
    let again = import(&e, &fixture());
    assert!(
        again.contains("intraday_stress: 0 new, 2 already there"),
        "{again}"
    );
    assert_eq!(dump(&e.pool).await, rows);
    // A reparse derives the same rows from the stored blobs.
    reparse(e.dir.path());
    assert_eq!(dump(&e.pool).await, rows);
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM sources WHERE origin = 'connect'"
        )
        .await,
        8
    );
}

#[tokio::test]
async fn a_pull_of_a_day_again_with_more_samples_adds_them_without_duplicates() {
    let e = env_at(at(14, 12, 0)).await;
    import(&e, &fixture());
    let before = count(&e.pool, "SELECT COUNT(*) FROM mon_hr").await;
    // The heart rate of Friday pulled again later: one more reading.
    let dir = tempfile::tempdir().unwrap();
    let mut j: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture().join("heart_rate/2026-11-13.json")).unwrap(),
    )
    .unwrap();
    j["heartRateValues"]
        .as_array_mut()
        .unwrap()
        .push(json!([(at(13, 21, 4) * 1000), 99]));
    std::fs::create_dir_all(dir.path().join("hr")).unwrap();
    std::fs::write(dir.path().join("hr/2026-11-13.json"), j.to_string()).unwrap();
    let out = import(&e, dir.path());
    assert!(
        out.contains("intraday_heart_rate: 1 new, 0 already there"),
        "{out}"
    );
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM mon_hr").await,
        before + 1
    );
    assert_eq!(
        count(
            &e.pool,
            &format!(
                "SELECT COUNT(*) FROM mon_hr WHERE ts = {} AND bpm = 100",
                at(13, 21, 0)
            )
        )
        .await,
        1,
        "the readings both pulls have are one row"
    );
}

#[tokio::test]
async fn the_account_profile_id_is_never_stored() {
    let e = env_at(at(14, 12, 0)).await;
    for kind in ["stress", "heart_rate", "respiration"] {
        let text = std::fs::read_to_string(fixture().join(kind).join("2026-11-13.json")).unwrap();
        assert!(
            text.contains("userProfilePK"),
            "the fixture carries it ({kind})"
        );
    }
    import(&e, &fixture());
    let shas: Vec<String> = sqlx::query_scalar("SELECT sha256 FROM files")
        .fetch_all(&e.pool)
        .await
        .unwrap();
    assert_eq!(shas.len(), 8);
    for sha in shas {
        let text = std::fs::read_to_string(stored_path(e.dir.path(), &sha)).unwrap();
        assert!(text.starts_with(r#"{"nimrag_connect":1,"#), "{sha}");
        assert!(!text.contains("userProfilePK"), "{sha}");
        if text.contains(r#""kind":"intraday_"#) {
            assert!(
                text.contains("calendarDate"),
                "the rest of the response is kept"
            );
        }
    }
    // Nowhere in the data directory (blobs, ledger, database).
    fn scan(dir: &Path, needle: &[u8], hits: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                scan(&p, needle, hits);
            } else if std::fs::read(&p)
                .unwrap()
                .windows(needle.len())
                .any(|w| w == needle)
            {
                hits.push(p);
            }
        }
    }
    for needle in ["userProfilePK", "9999001"] {
        let mut hits = Vec::new();
        scan(e.dir.path(), needle.as_bytes(), &mut hits);
        assert!(hits.is_empty(), "{needle}: {hits:?}");
    }
}

// ---- the Day view -----------------------------------------------------------------------------

#[tokio::test]
async fn a_day_with_only_connect_samples_gets_a_day_series_and_the_right_figures() {
    let (e, c) = setup(at(14, 12, 0)).await;
    let d = e.get(&c, "/api/days/2026-11-12").await;

    // Stress: five readings (20, 30, 50, 40, 70); the -1 and the -2 are no readings, the
    // instant with only a body battery level is no stress sample.
    let s = &d["stress"];
    assert_eq!(f(&s["average"]), Some(42.0), "{s}");
    assert_eq!(len(&s["series"]), 5);
    assert_eq!(f(&s["series"][1]["stress"]), Some(30.0));
    assert_eq!(s["series"][1]["level"], "low");
    // 20 stands for 3 minutes; 30 until 08:12 (the gap of the two missing readings, 9 minutes);
    // 50 and 40 3 each; 70, the last, 3.
    assert_eq!(
        (
            f(&s["rest_s"]),
            f(&s["low_s"]),
            f(&s["medium_s"]),
            f(&s["high_s"]),
            f(&s["activity_s"])
        ),
        (Some(180.0), Some(900.0), Some(180.0), Some(0.0), Some(0.0))
    );
    assert_eq!(s["main_level"], "low");

    // Energy: 60 58 57 56 56 (null) 61.
    let en = &d["energy"];
    assert_eq!(len(&en["series"]), 6);
    assert_eq!(
        (
            f(&en["high"]),
            f(&en["low"]),
            f(&en["charged"]),
            f(&en["drained"]),
            f(&en["now"])
        ),
        (Some(61.0), Some(56.0), Some(5.0), Some(4.0), Some(61.0))
    );
    assert!(en["now_at"]
        .as_str()
        .unwrap()
        .starts_with("2026-11-12T08:18:00+01:00"));

    // Heart rate: 48 at 03:00 and 60 62 (null) 75 90 58 in the morning, by 5 minutes.
    let hr = &d["heart_rate"]["day"];
    assert_eq!(
        (f(&hr["low_bpm"]), f(&hr["high_bpm"])),
        (Some(48.0), Some(90.0))
    );
    let vals: Vec<_> = hr["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| f(&p["value"]).unwrap())
        .collect();
    assert_eq!(vals, vec![48.0, 61.0, 82.5, 58.0]);

    // Breathing: 11 13 14 16 12; no sleep imported, so every reading is awake.
    let b = &d["breathing"];
    assert_eq!(
        (f(&b["lowest"]), f(&b["highest"]), f(&b["avg_awake"])),
        (Some(11.0), Some(16.0), Some(13.2))
    );
    assert!(b["avg_asleep"].is_null());
    let hourly: Vec<_> = b["hourly"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| f(&p["value"]).unwrap())
        .collect();
    assert_eq!(hourly, vec![11.0, 13.0, 14.0]);
}

#[tokio::test]
async fn the_daily_summary_is_not_read_for_a_day_that_has_samples() {
    let (e, c) = setup(at(14, 12, 0)).await;
    // 12 Nov: the summary says stress 90, heart rate 30 to 200, energy 1 to 97, breathing 25.
    let d = e.get(&c, "/api/days/2026-11-12").await;
    assert_eq!(f(&d["stress"]["average"]), Some(42.0));
    assert_eq!(f(&d["energy"]["high"]), Some(61.0));
    assert_eq!(f(&d["heart_rate"]["day"]["high_bpm"]), Some(90.0));
    assert_eq!(f(&d["breathing"]["avg_awake"]), Some(13.2));
    // Steps are not an intraday metric: the daily summary still gives them.
    assert_eq!(f(&d["steps"]), Some(1000.0));
    // The Today page and the metric pages agree.
    let w = e
        .get(&c, "/api/metrics/stress?range=7d&end=2026-11-12")
        .await;
    assert_eq!(point(&w, "average", "2026-11-12"), Some(42.0));
    let w = e
        .get(&c, "/api/metrics/energy?range=7d&end=2026-11-12")
        .await;
    assert_eq!(point(&w, "high", "2026-11-12"), Some(61.0));
    assert_eq!(point(&w, "charged", "2026-11-12"), Some(5.0));
    let w = e
        .get(&c, "/api/metrics/heart_rate?range=7d&end=2026-11-12")
        .await;
    assert_eq!(point(&w, "high", "2026-11-12"), Some(90.0));
    let w = e
        .get(&c, "/api/metrics/respiration?range=7d&end=2026-11-12")
        .await;
    assert_eq!(point(&w, "awake", "2026-11-12"), Some(13.2));

    // 14 Nov has a heart rate and nothing else: the heart rate is from the samples, the other
    // figures (stress, energy, breathing) from the summary, per metric.
    let d = e.get(&c, "/api/days/2026-11-14").await;
    let hr = &d["heart_rate"]["day"];
    assert_eq!(
        (f(&hr["low_bpm"]), f(&hr["high_bpm"])),
        (Some(66.0), Some(68.0))
    );
    assert_eq!(len(&hr["series"]), 1);
    assert_eq!(f(&d["stress"]["average"]), Some(33.0));
    assert!(d["stress"]["series"].as_array().unwrap().is_empty());
    let en = &d["energy"];
    assert_eq!(
        (
            f(&en["high"]),
            f(&en["low"]),
            f(&en["charged"]),
            f(&en["drained"])
        ),
        (Some(80.0), Some(40.0), Some(20.0), Some(25.0))
    );
    assert!(en["now"].is_null());
    assert_eq!(f(&d["breathing"]["avg_awake"]), Some(15.0));
    assert_eq!(f(&d["steps"]), Some(3000.0));
}

#[tokio::test]
async fn today_reads_the_samples_of_the_day_too() {
    let (e, c) = setup(at(12, 12, 0)).await;
    let t = e.get(&c, "/api/today").await;
    assert_eq!(f(&t["stress"]["average"]), Some(42.0));
    assert_eq!(f(&t["energy"]["now"]), Some(61.0));
    assert_eq!(f(&t["energy"]["high"]), Some(61.0));
    assert_eq!(len(&t["energy"]["series"]), 6);
    let hr = &t["heart_rate"]["day"];
    assert_eq!(
        (f(&hr["low_bpm"]), f(&hr["high_bpm"])),
        (Some(48.0), Some(90.0))
    );
    assert_eq!(f(&t["respiration"]["day_avg_brpm"]), Some(13.2));
    assert_eq!(f(&t["respiration"]["latest_brpm"]), Some(12.0));
}

#[tokio::test]
async fn breathing_and_heart_rate_in_the_sleep_window_come_from_the_samples() {
    let e = env_at(at(14, 12, 0)).await;
    nimrag_server::settings::set_user(&e.pool, e.user_id, "timezone", TZ)
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture(), dir.path());
    // A Connect night, 23:00 to 07:00: the breathing at 02:00 and 03:00 and the heart rate at
    // 03:00 are asleep.
    std::fs::write(
        dir.path().join("sleep.json"),
        json!({"nights": [{
            "date": "2026-11-12",
            "sleep_start": cph(2026, 11, 11, 23, 0) * 1000,
            "sleep_end": cph(2026, 11, 12, 7, 0) * 1000,
            "sleep_score": 80, "deep_sleep_seconds": 5000, "light_sleep_seconds": 16000,
            "rem_sleep_seconds": 6000, "awake_seconds": 900
        }]})
        .to_string(),
    )
    .unwrap();
    import(&e, dir.path());
    let c = e.login().await;
    let d = e.get(&c, "/api/days/2026-11-12").await;
    let b = &d["breathing"];
    assert_eq!(
        (
            f(&b["avg_asleep"]),
            f(&b["avg_awake"]),
            f(&b["lowest"]),
            f(&b["highest"])
        ),
        (Some(12.0), Some(14.0), Some(11.0), Some(16.0)),
        "{b}"
    );
    let n = &e.get(&c, "/api/sleep/2026-11-12").await["night"];
    assert_eq!(f(&n["breathing_brpm"]), Some(12.0), "{n}");
    assert_eq!(f(&n["lowest_hr_bpm"]), Some(48.0), "{n}");
}

// ---- a mixed day ------------------------------------------------------------------------------

#[tokio::test]
async fn a_mixed_day_merges_connect_until_the_evening_and_the_watch_after() {
    let (e, c) = setup(at(14, 12, 0)).await;
    watch(&e, 'a', &friday_evening()).await;

    // At the instant both have a sample the watch's row wins as a whole: its stress 80 and
    // body battery 36, not Connect's 40 and 38; the rest of the day is Connect's.
    let uid = e.user_id;
    let origin = |t: &'static str, ts: i64| {
        let sql = format!(
            "SELECT s.origin FROM {t} t JOIN sources s ON s.src_id = t.src WHERE t.user_id = {uid} AND t.ts = {ts}"
        );
        let pool = e.pool.clone();
        async move {
            sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql))
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    assert_eq!(origin("stress", at(13, 21, 3)).await, "fit");
    assert_eq!(origin("stress", at(13, 21, 0)).await, "connect");
    assert_eq!(origin("stress", at(13, 21, 6)).await, "fit");
    assert_eq!(origin("mon_hr", at(13, 21, 2)).await, "fit");
    assert_eq!(origin("mon_hr", at(13, 21, 0)).await, "connect");
    assert_eq!(origin("respiration", at(13, 21, 2)).await, "fit");
    assert_eq!(origin("respiration", at(13, 21, 0)).await, "connect");

    let d = e.get(&c, "/api/days/2026-11-13").await;
    // Stress 20 (Connect 08:00), 60 (21:00), 80 (watch 21:03), 10, 30: mean 40. With Connect's
    // 40 at 21:03 it would be 32; with the summary's wild 91, 91.
    let s = &d["stress"];
    assert_eq!(f(&s["average"]), Some(40.0), "{s}");
    assert_eq!(len(&s["series"]), 5);
    let times: Vec<_> = s["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["time"].as_str().unwrap()[11..16].to_owned())
        .collect();
    assert_eq!(times, ["08:00", "21:00", "21:03", "21:06", "21:09"]);
    assert_eq!(f(&s["series"][2]["stress"]), Some(80.0));
    assert_eq!(s["series"][2]["level"], "high");
    // 08:00 stands for at most 10 minutes (the long gap to 21:00), then 3 minutes each; the
    // last for 3: rest 600 + 180 (10), medium 180 (60), high 180 (80), low 180 (30).
    assert_eq!(
        (
            f(&s["rest_s"]),
            f(&s["low_s"]),
            f(&s["medium_s"]),
            f(&s["high_s"])
        ),
        (Some(780.0), Some(180.0), Some(180.0), Some(180.0))
    );

    // Energy 70, 40, 36 (watch), 41, 45: up 5 + 4, down 30 + 4.
    let en = &d["energy"];
    assert_eq!(len(&en["series"]), 5);
    assert_eq!(
        (
            f(&en["charged"]),
            f(&en["drained"]),
            f(&en["high"]),
            f(&en["low"]),
            f(&en["now"])
        ),
        (Some(9.0), Some(34.0), Some(70.0), Some(36.0), Some(45.0)),
        "{en}"
    );
    assert!(en["now_at"].as_str().unwrap().contains("T21:09:00"));

    // Heart rate 55, 100, (Connect's 105 loses to the watch's 70), 95.
    let hr = &d["heart_rate"]["day"];
    assert_eq!(
        (f(&hr["low_bpm"]), f(&hr["high_bpm"])),
        (Some(55.0), Some(100.0))
    );
    // Breathing 14, 16, (Connect's 18 loses to the watch's 12).
    let b = &d["breathing"];
    assert_eq!(
        (f(&b["lowest"]), f(&b["highest"]), f(&b["avg_awake"])),
        (Some(12.0), Some(16.0), Some(14.0))
    );

    // The same numbers on the other pages.
    let w = e
        .get(&c, "/api/metrics/stress?range=7d&end=2026-11-13")
        .await;
    assert_eq!(point(&w, "average", "2026-11-13"), Some(40.0));
    let w = e
        .get(&c, "/api/metrics/energy?range=7d&end=2026-11-13")
        .await;
    assert_eq!(point(&w, "drained", "2026-11-13"), Some(34.0));
    let w = e
        .get(&c, "/api/metrics/heart_rate?range=7d&end=2026-11-13")
        .await;
    assert_eq!(point(&w, "high", "2026-11-13"), Some(100.0));
    let cal = e.get(&c, "/api/calendar/2026-11").await;
    let day13 = cal["days"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["date"] == "2026-11-13")
        .unwrap()
        .clone();
    assert_eq!(
        (f(&day13["energy_high"]), f(&day13["energy_low"])),
        (Some(70.0), Some(36.0))
    );
}

#[tokio::test]
async fn the_import_order_against_the_watch_does_not_matter() {
    let mut dumps = Vec::new();
    for watch_first in [true, false] {
        let e = env_at(at(14, 12, 0)).await;
        if watch_first {
            watch(&e, 'a', &friday_evening()).await;
        }
        import(&e, &fixture());
        if !watch_first {
            watch(&e, 'a', &friday_evening()).await;
        }
        dumps.push(dump(&e.pool).await);
    }
    assert_eq!(dumps[0], dumps[1]);
    assert!(dumps[0]
        .iter()
        .any(|r| r.ends_with("|80.0|80.0|36.0|NULL|fit")));
}

// ---- a day with a DST change ------------------------------------------------------------------

#[tokio::test]
async fn a_day_with_a_dst_change_has_all_its_points() {
    // Sunday 25 Oct 2026: clocks go back, the local day has 25 hours (500 stress points of 3
    // minutes, 750 heart rate and breathing points of 2 minutes).
    let start = cph(2026, 10, 25, 0, 0);
    assert_eq!(cph(2026, 10, 26, 0, 0) - start, 25 * 3600);
    let dir = tempfile::tempdir().unwrap();
    let day = "2026-10-25";
    let desc = json!([{"key": "timestamp", "index": 0}, {"key": "stressLevel", "index": 1}]);
    let bb_desc = json!([
        {"bodyBatteryValueDescriptorIndex": 0, "bodyBatteryValueDescriptorKey": "timestamp"},
        {"bodyBatteryValueDescriptorIndex": 1, "bodyBatteryValueDescriptorKey": "bodyBatteryStatus"},
        {"bodyBatteryValueDescriptorIndex": 2, "bodyBatteryValueDescriptorKey": "bodyBatteryLevel"},
        {"bodyBatteryValueDescriptorIndex": 3, "bodyBatteryValueDescriptorKey": "bodyBatteryVersion"}
    ]);
    let stress: Vec<Value> = (0..500)
        .map(|i| {
            json!([
                (start + i * 180) * 1000,
                if i % 10 == 9 { -1 } else { i % 100 }
            ])
        })
        .collect();
    let bb: Vec<Value> = (0..500)
        .map(|i| json!([(start + i * 180) * 1000, "MEASURED", 50 + i % 7, 3.0]))
        .collect();
    let hr: Vec<Value> = (0..750)
        .map(|i| {
            json!([
                (start + i * 120) * 1000,
                if i % 10 == 9 {
                    Value::Null
                } else {
                    json!(60 + i % 30)
                }
            ])
        })
        .collect();
    let resp: Vec<Value> = (0..750)
        .map(|i| {
            json!([
                (start + i * 120) * 1000,
                if i % 10 == 9 {
                    -1.0
                } else {
                    12.0 + (i % 5) as f64
                }
            ])
        })
        .collect();
    let put = |name: &str, v: Value| {
        std::fs::create_dir_all(dir.path().join(name)).unwrap();
        std::fs::write(
            dir.path().join(name).join(format!("{day}.json")),
            v.to_string(),
        )
        .unwrap();
    };
    put(
        "stress",
        json!({"calendarDate": day, "stressValueDescriptorsDTOList": desc, "stressValuesArray": stress,
               "bodyBatteryValueDescriptorsDTOList": bb_desc, "bodyBatteryValuesArray": bb}),
    );
    put(
        "heart_rate",
        json!({"calendarDate": day, "heartRateValueDescriptors": [{"index": 0, "key": "timestamp"}, {"index": 1, "key": "heartrate"}],
               "heartRateValues": hr}),
    );
    put(
        "respiration",
        json!({"calendarDate": day, "respirationValueDescriptorsDTOList": [{"index": 0, "key": "timestamp"}, {"index": 1, "key": "respiration"}],
               "respirationValuesArray": resp}),
    );
    let e = env_at(cph(2026, 10, 27, 12, 0)).await;
    nimrag_server::settings::set_user(&e.pool, e.user_id, "timezone", TZ)
        .await
        .unwrap();
    import(&e, dir.path());
    assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM stress").await, 500);
    assert_eq!(
        count(
            &e.pool,
            "SELECT COUNT(*) FROM stress WHERE stress IS NOT NULL"
        )
        .await,
        450
    );
    assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM mon_hr").await, 675);
    assert_eq!(
        count(&e.pool, "SELECT COUNT(*) FROM respiration").await,
        675
    );

    let c = e.login().await;
    let d = e.get(&c, "/api/days/2026-10-25").await;
    assert_eq!(len(&d["stress"]["series"]), 450);
    assert_eq!(len(&d["energy"]["series"]), 500);
    let first = d["energy"]["series"][0]["time"].as_str().unwrap();
    let last = d["energy"]["series"][499]["time"].as_str().unwrap();
    assert!(first.starts_with("2026-10-25T00:00:00+02:00"), "{first}");
    assert!(last.starts_with("2026-10-25T23:57:00+01:00"), "{last}");
    assert_eq!(
        len(&d["heart_rate"]["day"]["series"]),
        300,
        "the 25 hours in 5-minute buckets"
    );
    assert_eq!(len(&d["breathing"]["hourly"]), 25);
    // The neighbouring days have none of it.
    let n = e.get(&c, "/api/days/2026-10-26").await;
    assert!(n["stress"]["series"].as_array().unwrap().is_empty());
    reparse(e.dir.path());
    assert_eq!(count(&e.pool, "SELECT COUNT(*) FROM stress").await, 500);
}

// ---- other users ------------------------------------------------------------------------------

#[tokio::test]
async fn another_user_sees_none_of_it() {
    let e = env_at(at(14, 12, 0)).await;
    e.adopt.store(false, std::sync::atomic::Ordering::Relaxed);
    let other = e.add_user("other", Role::User).await;
    import(&e, &fixture());
    nimrag_server::settings::set_user(&e.pool, other, "timezone", TZ)
        .await
        .unwrap();
    let cookie = e.login_as("other").await;
    let d = e.get(&cookie, "/api/days/2026-11-12").await;
    assert!(d["stress"]["series"].as_array().unwrap().is_empty());
    assert!(d["energy"]["series"].as_array().unwrap().is_empty());
    assert!(d["energy"]["high"].is_null() && d["stress"]["average"].is_null());
    assert!(d["heart_rate"]["day"]["series"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(d["heart_rate"]["day"]["high_bpm"].is_null());
    assert!(
        d["breathing"]["lowest"].is_null()
            && d["breathing"]["hourly"].as_array().unwrap().is_empty()
    );
    for t in ["stress", "mon_hr", "respiration"] {
        assert_eq!(
            count(
                &e.pool,
                &format!("SELECT COUNT(*) FROM {t} WHERE user_id = {other}")
            )
            .await,
            0,
            "{t}"
        );
    }
    // Nor does their own watch data lose to it: another user's sample at the same instant is
    // another row.
    assert_eq!(
        count(&e.pool, "SELECT COUNT(DISTINCT user_id) FROM stress").await,
        1
    );
}
