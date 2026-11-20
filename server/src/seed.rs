//! `dev-seed`: 120 days of plausible, internally consistent sample data in the
//! derived tables, for development, screenshots and tests. Deterministic (fixed
//! RNG seed; the only inputs are the end date, the time zone and "now").
//!
//! Two accounts are created: the admin (`admin`, no data) and the user `demo`, who owns all of
//! the sample data. Both have 2FA enrolled. Their passwords and 2FA secrets are random unless the
//! caller passes them in [`SeedAccounts`]; the report hands them back to the caller and nothing
//! is logged.
//!
//! Everything is marked as sample data (`settings.sample_data = 1`), so API
//! responses carry `sample: true`. Rows are written straight into the derived
//! tables with synthetic hashes; no file is stored, which is why a data
//! directory that holds uploaded files is refused. Foreign keys are switched off
//! for the one seeding connection because the `files` rows do not exist.
//!
//! The sample person: rests around 50 bpm, sleeps about 7 hours, runs and walks
//! around a loop near an arbitrary point (50.000 N, 10.000 E), lifts twice a week.

use crate::derived::{rank_key, src_id, V};
use crate::settings::{self, day_start};
use crate::users::Role;
use anyhow::{bail, Result};
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use chrono_tz::Tz;
use sqlx::SqlitePool;
use std::collections::BTreeMap;
use std::path::Path;

const DAYS: i64 = 120;
const BASE_LAT: f64 = 50.0;
const BASE_LON: f64 = 10.0;
const SAMPLE_SRC: i64 = 1;

/// Name of the user who owns the sample data.
pub const DEMO_USER: &str = "demo";
pub const ADMIN_USER: &str = "admin";

/// Credentials to use instead of random ones (a test that must log in knows them up front).
#[derive(Default, Clone)]
pub struct SeedAccounts {
    pub admin_password: Option<String>,
    pub admin_totp_secret: Option<String>,
    pub user_password: Option<String>,
    pub user_totp_secret: Option<String>,
}

pub struct SeedOptions {
    pub tz: Tz,
    /// Data is generated up to this instant (unix seconds).
    pub now: i64,
    pub days: i64,
    pub accounts: SeedAccounts,
}

impl SeedOptions {
    pub fn today() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        Self {
            tz: settings::system_timezone(),
            now,
            days: DAYS,
            accounts: SeedAccounts::default(),
        }
    }
}

/// A seeded login. `Debug` shows only the username.
#[derive(Clone)]
pub struct SeedLogin {
    pub username: String,
    pub password: String,
    /// Base32 TOTP secret (RFC 6238, SHA-1, 6 digits, 30 s).
    pub totp_secret: String,
}

impl std::fmt::Debug for SeedLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeedLogin")
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct SeedReport {
    pub days: i64,
    pub first_date: String,
    pub last_date: String,
    pub activities: usize,
    pub admin: SeedLogin,
    pub user: SeedLogin,
    pub user_id: i64,
}

// ---- deterministic randomness --------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f()
    }
    fn chance(&mut self, p: f64) -> bool {
        self.f() < p
    }
}

// ---- row buffers -----------------------------------------------------------------------------

fn i(x: i64) -> V {
    V::I(Some(x))
}
fn r(x: f64) -> V {
    V::R(Some(x))
}
fn t(x: &str) -> V {
    V::T(Some(x.to_owned()))
}
fn nul() -> V {
    V::R(None)
}

#[derive(Default)]
struct Buf {
    tables: BTreeMap<&'static str, (&'static str, Vec<Vec<V>>)>,
}

impl Buf {
    fn add(&mut self, table: &'static str, cols: &'static str, row: Vec<V>) {
        self.tables
            .entry(table)
            .or_insert((cols, Vec::new()))
            .1
            .push(row);
    }
}

/// Tables whose rows belong to a user (`user_id` column, first in every statement here).
const PARTITIONED: &[&str] = &[
    "mon_hr",
    "mon_activity",
    "mon_extra",
    "stress",
    "respiration",
    "spo2",
    "resting_hr",
    "sleep_levels",
    "hrv_values",
    "hrv_summary",
    "training_load",
    "training_status",
    "training_balance",
    "recovery_time",
    "vo2max",
    "battery",
    "body_measurements",
];

async fn flush(conn: &mut sqlx::SqliteConnection, buf: Buf, uid: i64) -> Result<()> {
    for (table, (cols, rows)) in buf.tables {
        let owned = PARTITIONED.contains(&table);
        let cols = if owned {
            format!("user_id, {cols}")
        } else {
            cols.to_owned()
        };
        let width = cols.split(',').count();
        let marks = format!("({})", vec!["?"; width].join(", "));
        for chunk in rows.chunks(30_000 / width) {
            let sql = format!(
                "INSERT INTO {table} ({cols}) VALUES {}",
                vec![marks.as_str(); chunk.len()].join(", ")
            );
            let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
            for row in chunk {
                if owned {
                    q = q.bind(uid);
                }
                for v in row {
                    q = match v {
                        V::I(x) => q.bind(*x),
                        V::R(x) => q.bind(*x),
                        V::T(x) => q.bind(x.clone()),
                    };
                }
            }
            q.execute(&mut *conn).await?;
        }
    }
    Ok(())
}

// ---- activities --------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Run,
    Walk,
    Strength,
}

struct Act {
    kind: Kind,
    start: i64,
    end: i64,
    /// Heart rate every 10 s from `start`.
    hr: Vec<f64>,
    load: f64,
    kcal: f64,
}

impl Act {
    fn hr_at(&self, t: i64) -> f64 {
        let i = (((t - self.start) / 10) as usize).min(self.hr.len().saturating_sub(1));
        self.hr.get(i).copied().unwrap_or(100.0)
    }
}

fn sha_of(label: &str) -> String {
    crate::sha256_hex(label.as_bytes())
}

struct Exercise {
    category: i64,
    category_name: &'static str,
    subtype: i64,
    title: &'static str,
    weight: Option<f64>,
    reps: f64,
    duration: f64,
}

const EXERCISES: [Exercise; 6] = [
    Exercise {
        category: 0,
        category_name: "bench_press",
        subtype: 1,
        title: "Dumbbell bench press",
        weight: Some(22.0),
        reps: 10.0,
        duration: 40.0,
    },
    Exercise {
        category: 23,
        category_name: "row",
        subtype: 2,
        title: "Bent-over row",
        weight: Some(20.0),
        reps: 10.0,
        duration: 40.0,
    },
    Exercise {
        category: 28,
        category_name: "squat",
        subtype: 3,
        title: "Goblet squat",
        weight: Some(24.0),
        reps: 12.0,
        duration: 45.0,
    },
    Exercise {
        category: 24,
        category_name: "shoulder_press",
        subtype: 4,
        title: "Overhead press",
        weight: Some(14.0),
        reps: 8.0,
        duration: 35.0,
    },
    Exercise {
        category: 8,
        category_name: "deadlift",
        subtype: 5,
        title: "Romanian deadlift",
        weight: Some(28.0),
        reps: 10.0,
        duration: 45.0,
    },
    Exercise {
        category: 19,
        category_name: "plank",
        subtype: 6,
        title: "Plank",
        weight: None,
        reps: 0.0,
        duration: 60.0,
    },
];

/// Build one activity's rows into `buf` and return its schedule entry.
fn make_activity(
    buf: &mut Buf,
    rng: &mut Rng,
    n: usize,
    kind: Kind,
    start: i64,
    progress: f64,
) -> Act {
    let sha = sha_of(&format!("sample-activity-{n}"));
    let sha = sha.as_str();
    let (sport, sport_name, sub, sub_name) = match kind {
        Kind::Run => (1, "running", 0, "generic"),
        Kind::Walk => (11, "walking", 0, "generic"),
        Kind::Strength => (10, "training", 20, "strength_training"),
    };
    let mut hr: Vec<f64> = Vec::new();
    let mut idx = 0i64;
    let (mut dist_total, mut ascent, mut max_hr) = (0.0_f64, 0.0_f64, 0.0_f64);
    let (duration, speed_avg);
    let mut laps: Vec<(i64, f64, f64, f64, f64)> = Vec::new(); // start, timer, distance, avg_hr, max_hr
    let (load, aero, anaerobic);

    match kind {
        Kind::Run | Kind::Walk => {
            let (km, speed) = if kind == Kind::Run {
                (rng.range(4.5, 10.0), rng.range(2.65, 3.05))
            } else {
                (rng.range(3.0, 6.0), rng.range(1.2, 1.5))
            };
            let total = km * 1000.0;
            duration = (total / speed).round();
            speed_avg = total / duration;
            let center = (
                BASE_LAT + rng.range(-0.004, 0.004),
                BASE_LON + rng.range(-0.006, 0.006),
            );
            let radius = total / std::f64::consts::TAU;
            let phase = rng.range(0.0, std::f64::consts::TAU);
            let (hr_rest, hr_work) = if kind == Kind::Run {
                (125.0, rng.range(148.0, 160.0))
            } else {
                (88.0, rng.range(98.0, 110.0))
            };
            let mut lap_start = (start, 0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64); // start, hr sum, hr n, distance at start, max
            let mut elapsed = 0.0;
            let mut last_alt = 120.0;
            while elapsed <= duration {
                let frac = elapsed / duration;
                let dist = frac * total;
                let theta = phase + frac * std::f64::consts::TAU;
                let wobble = 1.0 + 0.04 * (7.0 * theta).sin();
                let lat = center.0 + radius * wobble * theta.sin() / 111_320.0;
                let lon = center.1
                    + radius * wobble * theta.cos() / (111_320.0 * BASE_LAT.to_radians().cos());
                let warm = (elapsed / 300.0).min(1.0);
                let h = hr_rest
                    + (hr_work - hr_rest) * warm
                    + 6.0 * (elapsed / 400.0).sin()
                    + rng.range(-2.5, 2.5)
                    + 8.0 * frac * frac;
                let sp = speed_avg * (1.0 + 0.06 * (elapsed / 90.0).sin()) + rng.range(-0.08, 0.08);
                let alt = 120.0 + 14.0 * (2.0 * theta).sin() + 5.0 * (5.0 * theta).sin();
                if alt > last_alt {
                    ascent += alt - last_alt;
                }
                last_alt = alt;
                hr.push(h);
                max_hr = max_hr.max(h);
                buf.add(
                    "activity_records",
                    "sha256, idx, ts, lat, lon, hr, speed, altitude_m, distance_m, cadence, temperature_c",
                    vec![t(sha), i(idx), i(start + elapsed as i64), r(lat), r(lon), r(h.round()), r((sp * 100.0).round() / 100.0),
                         r((alt * 10.0).round() / 10.0), r(dist.round()), r(if kind == Kind::Run { (86.0 + rng.range(-3.0, 3.0)).round() } else { 55.0 }),
                         r(11.0)],
                );
                idx += 1;
                lap_start.1 += h;
                lap_start.2 += 1.0;
                lap_start.4 = lap_start.4.max(h);
                let km_done = (dist / 1000.0).floor();
                if dist - lap_start.3 >= 1000.0 || elapsed + 10.0 > duration {
                    let d = dist - lap_start.3;
                    laps.push((
                        lap_start.0,
                        ((start + elapsed as i64) - lap_start.0) as f64,
                        d,
                        lap_start.1 / lap_start.2,
                        lap_start.4,
                    ));
                    lap_start = (start + elapsed as i64, 0.0, 0.0, km_done * 1000.0, 0.0);
                    if elapsed + 10.0 > duration {
                        break;
                    }
                }
                elapsed += 10.0;
            }
            dist_total = total;
            if kind == Kind::Run {
                load = rng.range(70.0, 140.0) * (km / 7.0).max(0.6);
                aero = rng.range(2.4, 3.9);
                anaerobic = rng.range(0.1, 1.2);
            } else {
                load = rng.range(15.0, 35.0);
                aero = rng.range(0.6, 1.4);
                anaerobic = 0.0;
            }
        }
        Kind::Strength => {
            duration = (rng.range(38.0, 47.0) * 60.0).round();
            speed_avg = 0.0;
            let mut cursor = 0.0;
            let mut set_idx = 0i64;
            let mut exercise_laps = Vec::new();
            for (e_idx, e) in EXERCISES.iter().enumerate() {
                let lap_from = cursor;
                buf.add(
                    "exercise_titles",
                    "sha256, idx, category, category_name, subtype, name",
                    vec![
                        t(sha),
                        i(e_idx as i64),
                        i(e.category),
                        t(e.category_name),
                        i(e.subtype),
                        t(e.title),
                    ],
                );
                let weight = e
                    .weight
                    .map(|w| ((w * (1.0 + 0.1 * progress)) / 2.0).round() * 2.0);
                for _ in 0..3 {
                    let st = start + cursor as i64;
                    buf.add(
                        "strength_sets",
                        "sha256, idx, ts, start_ts, duration_s, set_type, set_type_name, reps, weight_kg, category, category_name, category_subtype, category_all, category_subtype_all, wkt_step_index",
                        vec![t(sha), i(set_idx), i(st + e.duration as i64), i(st), r(e.duration), i(1), t("active"),
                             if e.weight.is_some() { r(e.reps) } else { nul() }, weight.map_or(nul(), r),
                             i(e.category), t(e.category_name), i(e.subtype), V::T(None), V::T(None), i(e_idx as i64)],
                    );
                    set_idx += 1;
                    // The rest row the watch writes between sets; the API leaves it out.
                    let rest_start = st + e.duration as i64;
                    buf.add(
                        "strength_sets",
                        "sha256, idx, ts, start_ts, duration_s, set_type, set_type_name, reps, weight_kg, category, category_name, category_subtype, category_all, category_subtype_all, wkt_step_index",
                        vec![t(sha), i(set_idx), i(rest_start + 90), i(rest_start), r(90.0), i(0), t("rest"), nul(), nul(),
                             V::I(None), V::T(None), V::I(None), V::T(None), V::T(None), i(e_idx as i64)],
                    );
                    set_idx += 1;
                    // Heart rate per 10 s: up during the set, down in the rest.
                    let mut s = cursor;
                    while s < cursor + e.duration + 90.0 && s < duration {
                        let in_set = s < cursor + e.duration;
                        let h = if in_set {
                            118.0 + (s - cursor) * 0.4 + rng.range(-4.0, 6.0)
                        } else {
                            104.0 - (s - cursor - e.duration) * 0.05 + rng.range(-3.0, 3.0)
                        };
                        hr.push(h);
                        max_hr = max_hr.max(h);
                        buf.add(
                            "activity_records",
                            "sha256, idx, ts, lat, lon, hr, speed, altitude_m, distance_m, cadence, temperature_c",
                            vec![t(sha), i(idx), i(start + s as i64), nul(), nul(), r(h.round()), nul(), nul(), nul(), nul(), nul()],
                        );
                        idx += 1;
                        s += 10.0;
                    }
                    cursor += e.duration + 90.0;
                }
                exercise_laps.push((start + lap_from as i64, cursor - lap_from, 0.0, 0.0, 0.0));
            }
            // Cool-down samples to the end of the session.
            let mut s = cursor;
            while s <= duration {
                hr.push(100.0 - (s - cursor) * 0.05);
                s += 10.0;
            }
            max_hr = max_hr.max(1.0);
            laps = exercise_laps;
            load = rng.range(35.0, 70.0);
            aero = rng.range(1.2, 2.4);
            anaerobic = rng.range(0.5, 1.6);
        }
    }
    let end = start + duration as i64;
    let avg_hr = hr.iter().sum::<f64>() / hr.len().max(1) as f64;
    let kcal = match kind {
        Kind::Run => dist_total / 1000.0 * 68.0,
        Kind::Walk => dist_total / 1000.0 * 46.0,
        Kind::Strength => duration / 60.0 * 6.8,
    }
    .round();

    // Entity rows.
    buf.add(
        "sources",
        "sha256, src_id, ok, last_ts, msg_count, serial, time_created, file_type, rank_key",
        vec![
            t(sha),
            i(src_id(sha)),
            i(1),
            i(end),
            i(idx),
            V::I(None),
            i(start),
            V::I(None),
            V::T(Some(rank_key(true, Some(end), idx as u64))),
        ],
    );
    buf.add(
        "activities",
        "sha256, activity_key, serial, time_created, ts, start_ts, local_offset_s, total_timer_s, num_sessions, sport, sport_name, sub_sport, sub_sport_name",
        vec![t(sha), V::T(Some(format!("sample:{n}"))), V::I(None), i(start), i(end), i(start), i(0), r(duration), i(1), i(sport),
             t(sport_name), i(sub), t(sub_name)],
    );
    buf.add(
        "activity_sessions",
        "sha256, idx, start_ts, sport, sport_name, sub_sport, sub_sport_name, elapsed_s, timer_s, distance_m, calories, avg_hr, max_hr, avg_speed, max_speed, ascent_m, descent_m, avg_cadence, aerobic_te, anaerobic_te, training_load",
        vec![t(sha), i(0), i(start), i(sport), t(sport_name), i(sub), t(sub_name), r(duration), r(duration),
             if dist_total > 0.0 { r(dist_total.round()) } else { nul() }, r(kcal), r(avg_hr.round()), r(max_hr.round()),
             if speed_avg > 0.0 { r((speed_avg * 100.0).round() / 100.0) } else { nul() },
             if speed_avg > 0.0 { r((speed_avg * 1.25 * 100.0).round() / 100.0) } else { nul() },
             if ascent > 0.0 { r(ascent.round()) } else { nul() }, if ascent > 0.0 { r(ascent.round()) } else { nul() },
             if kind == Kind::Run { r(86.0) } else { nul() }, r((aero * 10.0).round() / 10.0), r((anaerobic * 10.0).round() / 10.0), r(load.round())],
    );
    for (li, (lap_start, timer, dist, lap_hr, lap_max)) in laps.iter().enumerate() {
        buf.add(
            "activity_laps",
            "sha256, idx, start_ts, sport, sport_name, sub_sport, sub_sport_name, elapsed_s, timer_s, distance_m, calories, avg_hr, max_hr, avg_speed, max_speed, ascent_m, descent_m, avg_cadence",
            vec![t(sha), i(li as i64), i(*lap_start), i(sport), t(sport_name), i(sub), t(sub_name), r(*timer), r(*timer),
                 if *dist > 0.0 { r(dist.round()) } else { nul() }, nul(), if *lap_hr > 0.0 { r(lap_hr.round()) } else { nul() },
                 if *lap_max > 0.0 { r(lap_max.round()) } else { nul() },
                 if *dist > 0.0 && *timer > 0.0 { r((dist / timer * 100.0).round() / 100.0) } else { nul() }, nul(), nul(), nul(), nul()],
        );
    }
    // Time in zone (seconds per zone; entry 0 is below zone 1).
    let mut zones = [0.0_f64; 6];
    for h in &hr {
        let z = match *h {
            h if h < 100.0 => 0,
            h if h < 120.0 => 1,
            h if h < 140.0 => 2,
            h if h < 155.0 => 3,
            h if h < 170.0 => 4,
            _ => 5,
        };
        zones[z] += 10.0;
    }
    buf.add(
        "activity_time_in_zone",
        "sha256, idx, ref_mesg, ref_index, hr_zone_s",
        vec![
            t(sha),
            i(0),
            i(18),
            i(0),
            V::T(Some(serde_json::to_string(&zones).unwrap_or_default())),
        ],
    );
    buf.add(
        "activity_metrics",
        "sha256, maximal_met, aerobic_te, anaerobic_te, training_load_peak, epoc, recovery_time_physical_activity_raw",
        vec![t(sha), r(13.0 + 0.2 * progress), r((aero * 10.0).round() / 10.0), r((anaerobic * 10.0).round() / 10.0), r(load.round()),
             r((load * 1.4).round()), nul()],
    );
    Act {
        kind,
        start,
        end,
        hr,
        load,
        kcal,
    }
}

// ---- accounts -----------------------------------------------------------------------------------

fn login_for(username: &str, password: Option<String>, totp: Option<String>) -> Result<SeedLogin> {
    let password = match password {
        Some(p) => {
            if p.chars().count() < crate::auth::MIN_PASSWORD_LEN {
                bail!(
                    "the seeded password for {username} must have at least {} characters",
                    crate::auth::MIN_PASSWORD_LEN
                );
            }
            p
        }
        None => crate::users::temporary_password()?,
    };
    let totp_secret = match totp {
        Some(s) => s,
        None => crate::totp::new_secret()?,
    };
    Ok(SeedLogin {
        username: username.to_owned(),
        password,
        totp_secret,
    })
}

async fn hash(password: &str) -> Result<String> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || crate::auth::hash_password(&password)).await?
}

// ---- the run ------------------------------------------------------------------------------------

pub async fn run(pool: &SqlitePool, data_dir: &Path, opts: SeedOptions) -> Result<SeedReport> {
    let files: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
        .fetch_one(pool)
        .await?;
    let blobs = std::fs::read_dir(data_dir.join("files"))
        .map(|d| d.flatten().any(|e| e.file_name() != ".incoming"))
        .unwrap_or(false);
    if files > 0 || blobs {
        bail!(
            "{} holds uploaded files; dev-seed only fills an empty data directory",
            data_dir.display()
        );
    }
    if settings::get(pool, "sample_data").await?.is_some() {
        bail!(
            "{} was already seeded; use a fresh data directory",
            data_dir.display()
        );
    }
    if crate::users::count(pool).await? > 0 {
        bail!(
            "{} already has user accounts; dev-seed only fills a fresh data directory",
            data_dir.display()
        );
    }

    let tz = opts.tz;
    let now = opts.now;
    let end_date = settings::date_of(tz, now);
    let first_date = end_date - Duration::days(opts.days - 1);
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut buf = Buf::default();

    // Nights: the night that starts on day k (k = -1 is the night before the range).
    struct Night {
        bed: i64,
        wake: i64,
        rhr: f64,
    }
    let mut nights: Vec<Night> = Vec::new();
    for k in -1..opts.days {
        let d = first_date + Duration::days(k);
        let bed = day_start(tz, d) + (rng.range(22.25, 24.0) * 3600.0) as i64;
        let wake = bed + (rng.range(6.3, 8.0) * 3600.0) as i64;
        let rhr = 51.0 + 1.5 * (k as f64 / 9.0).sin() + rng.range(-1.2, 1.2);
        nights.push(Night { bed, wake, rhr });
    }

    // Activities.
    let mut acts: Vec<Act> = Vec::new();
    for k in 0..opts.days {
        let d = first_date + Duration::days(k);
        let kind = match (d.weekday(), rng.chance(0.85)) {
            (_, false) => None,
            (Weekday::Mon | Weekday::Thu, _) => Some(Kind::Run),
            (Weekday::Wed | Weekday::Sat, _) => Some(Kind::Strength),
            (Weekday::Fri | Weekday::Sun, _) if rng.chance(0.55) => Some(if rng.chance(0.3) {
                Kind::Run
            } else {
                Kind::Walk
            }),
            _ => None,
        };
        let Some(kind) = kind else { continue };
        let hour = match kind {
            Kind::Walk => rng.range(12.0, 12.6),
            _ => rng.range(17.4, 18.4),
        };
        let start = day_start(tz, d) + (hour * 3600.0) as i64;
        if start + 3 * 3600 > now {
            continue; // not finished yet
        }
        let n = acts.len();
        acts.push(make_activity(
            &mut buf,
            &mut rng,
            n,
            kind,
            start,
            k as f64 / opts.days as f64,
        ));
    }

    // Sleep.
    let mut hrv_nights: Vec<f64> = Vec::new();
    for (k, night) in nights.iter().enumerate() {
        if night.wake > now {
            continue;
        }
        let sha = sha_of(&format!("sample-sleep-{k}"));
        // Stage segments: light, deep, light, REM per cycle; short awakenings.
        let mut ts = night.bed;
        let (mut deep, mut light, mut rem) = (0.0, 0.0, 0.0);
        let mut cycle = 0.0_f64;
        let mut awakenings = 0;
        while ts < night.wake {
            let plan = [
                (2, rng.range(15.0, 30.0)),
                (3, (45.0 - cycle * 10.0).max(8.0) + rng.range(-5.0, 5.0)),
                (2, rng.range(15.0, 30.0)),
                (4, 10.0 + cycle * 8.0 + rng.range(0.0, 8.0)),
            ];
            for (level, mins) in plan {
                if ts >= night.wake {
                    break;
                }
                let len = ((mins * 60.0) as i64).min(night.wake - ts);
                buf.add(
                    "sleep_levels",
                    "ts, level, level_name, src",
                    // A level row ends its period (see web_api::daily::stages_of).
                    vec![
                        i(ts + len),
                        i(level),
                        t(match level {
                            2 => "light",
                            3 => "deep",
                            _ => "rem",
                        }),
                        i(SAMPLE_SRC),
                    ],
                );
                match level {
                    3 => deep += len as f64,
                    2 => light += len as f64,
                    _ => rem += len as f64,
                }
                ts += len;
            }
            if ts < night.wake && rng.chance(0.55) {
                let len = ((rng.range(2.0, 6.0) * 60.0) as i64).min(night.wake - ts);
                buf.add(
                    "sleep_levels",
                    "ts, level, level_name, src",
                    vec![i(ts + len), i(1), t("awake"), i(SAMPLE_SRC)],
                );
                awakenings += 1;
                ts += len;
            }
            cycle += 1.0;
        }
        let asleep_h = (deep + light + rem) / 3600.0;
        let score = (62.0 + (asleep_h - 6.3) * 11.0 + rng.range(-5.0, 8.0))
            .clamp(55.0, 95.0)
            .round();
        buf.add(
            "sources",
            "sha256, src_id, ok, last_ts, msg_count, serial, time_created, file_type, rank_key",
            vec![
                t(&sha),
                i(src_id(&sha)),
                i(1),
                i(night.wake),
                i(1),
                V::I(None),
                i(night.wake),
                V::I(None),
                V::T(Some(rank_key(true, Some(night.wake), 1))),
            ],
        );
        buf.add(
            "sleep_sessions",
            "sha256, idx, start_ts, end_ts, overall_score, quality_score, duration_score, recovery_score, deep_score, light_score, rem_score, awakenings_count, avg_stress",
            vec![t(&sha), i(0), i(night.bed), i(night.wake), r(score), r((score + rng.range(-6.0, 6.0)).clamp(40.0, 100.0).round()),
                 r((asleep_h / 8.0 * 100.0).clamp(30.0, 100.0).round()), r(rng.range(60.0, 95.0).round()), r((deep / 3600.0 / 1.4 * 80.0).clamp(30.0, 100.0).round()),
                 r(rng.range(70.0, 95.0).round()), r((rem / 3600.0 / 1.8 * 80.0).clamp(30.0, 100.0).round()), r(f64::from(awakenings)), r(rng.range(12.0, 22.0).round())],
        );
        // HRV: values every 5 min asleep around a slowly drifting personal mean, one dip.
        let mu =
            62.0 + 5.0 * (k as f64 / 14.0).sin() - if (88..=92).contains(&k) { 14.0 } else { 0.0 };
        let mut vals = Vec::new();
        let mut s = night.bed + 600;
        while s < night.wake {
            let v = (mu + rng.range(-9.0, 9.0)).max(25.0);
            vals.push(v);
            buf.add(
                "hrv_values",
                "ts, rmssd_ms, src",
                vec![i(s), r((v * 10.0).round() / 10.0), i(SAMPLE_SRC)],
            );
            s += 300;
        }
        let avg = vals.iter().sum::<f64>() / vals.len().max(1) as f64;
        hrv_nights.push(avg);
        let last7: Vec<f64> = hrv_nights.iter().rev().take(7).copied().collect();
        let weekly = last7.iter().sum::<f64>() / last7.len() as f64;
        let all_mean = hrv_nights.iter().rev().take(28).sum::<f64>()
            / hrv_nights.iter().rev().take(28).count() as f64;
        let (lo, hi) = ((all_mean - 7.0).round(), (all_mean + 8.0).round());
        let status = if avg < lo - 8.0 {
            2
        } else if avg < lo || avg > hi {
            3
        } else {
            4
        };
        buf.add(
            "hrv_summary",
            "ts, weekly_avg, last_night_avg, last_night_5min_high, baseline_low_upper, baseline_balanced_lower, baseline_balanced_upper, status, src",
            vec![i(night.wake + 300), r(weekly.round()), r(avg.round()), r(vals.iter().copied().fold(0.0, f64::max).round()),
                 r(lo - 1.0), r(lo), r(hi), i(status), i(SAMPLE_SRC)],
        );
        // Resting heart rate, reported shortly after waking.
        buf.add(
            "resting_hr",
            "ts, rhr, current_day_rhr, src",
            vec![
                i(night.wake + 600),
                r(night.rhr.round()),
                r((night.rhr + rng.range(-1.0, 1.0)).round()),
                i(SAMPLE_SRC),
            ],
        );
    }

    // Continuous series, day by day.
    let in_sleep = |ts: i64| nights.iter().any(|n| ts >= n.bed && ts < n.wake);
    let act_at = |ts: i64| acts.iter().find(|a| ts >= a.start && ts < a.end);
    let rhr_at = |ts: i64| {
        nights
            .iter()
            .find(|n| ts >= n.bed - 20 * 3600 && ts < n.bed + 4 * 3600)
            .map_or(51.0, |n| n.rhr)
    };
    let mut energy = 70.0_f64;
    let (mut week_mod, mut week_vig) = (0.0_f64, 0.0_f64);
    let mut battery = 96.0_f64;
    for k in 0..opts.days {
        let d: NaiveDate = first_date + Duration::days(k);
        let (day_from, day_to) = (day_start(tz, d), day_start(tz, d + Duration::days(1)));
        let end = day_to.min(now + 1);
        if d.weekday() == Weekday::Mon {
            week_mod = 0.0;
            week_vig = 0.0;
        }
        let step_factor = rng.range(0.6, 1.5);
        let slump = rng.chance(0.4); // an afternoon with more stress

        let mut ts = day_from;
        while ts < end {
            let (sleeping, act) = (in_sleep(ts), act_at(ts));
            let rhr = rhr_at(ts);
            let hour = (ts - day_from) as f64 / 3600.0;
            let h = if sleeping {
                rhr - 3.0 + 2.5 * ((ts as f64) / 2400.0).sin() + rng.range(-1.5, 2.5)
            } else if let Some(a) = act {
                a.hr_at(ts)
            } else {
                rhr + 14.0 + 9.0 * (-((hour - 14.0) / 5.0).powi(2)).exp() + rng.range(-5.0, 7.0)
            };
            buf.add(
                "mon_hr",
                "ts, bpm, src",
                vec![i(ts), r(h.round().max(42.0)), i(SAMPLE_SRC)],
            );
            ts += 180;
        }

        let (mut steps_walk, mut steps_run, mut cal, mut active_s) =
            (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
        let (mut ascent, mut descent) = (0.0_f64, 0.0_f64);
        let mut ts = day_from;
        let mut step_i = 0;
        while ts < end {
            let (sleeping, act) = (in_sleep(ts), act_at(ts));
            let hour = (ts - day_from) as f64 / 3600.0;
            // Stress (0 to 100), or a movement status while exercising.
            let stress = if sleeping {
                rng.range(8.0, 24.0)
            } else {
                22.0 + 10.0 * (hour / 3.0).sin()
                    + if slump && (13.0..16.0).contains(&hour) {
                        rng.range(22.0, 34.0)
                    } else {
                        0.0
                    }
                    + rng.range(-6.0, 8.0)
            }
            .clamp(3.0, 95.0);
            // Energy: charges asleep, drains awake (faster when stressed or exercising).
            if sleeping {
                energy = (energy + 0.62).min(98.0 + (k % 3) as f64);
            } else {
                let exercise = if act.is_some() { 0.55 } else { 0.0 };
                energy = (energy - (0.09 + stress / 100.0 * 0.22 + exercise)).max(8.0);
            }
            match act {
                Some(_) => buf.add(
                    "stress",
                    "ts, stress, stress_raw, body_battery, body_battery_status, src",
                    vec![
                        i(ts),
                        nul(),
                        r(-2.0),
                        r(energy.round()),
                        V::I(None),
                        i(SAMPLE_SRC),
                    ],
                ),
                None => buf.add(
                    "stress",
                    "ts, stress, stress_raw, body_battery, body_battery_status, src",
                    vec![
                        i(ts),
                        r(stress.round()),
                        r(stress.round()),
                        r(energy.round()),
                        V::I(None),
                        i(SAMPLE_SRC),
                    ],
                ),
            }
            // Steps this 5-minute slot.
            if !sleeping {
                match act {
                    Some(a) if a.kind == Kind::Run => steps_run += 5.0 * 165.0,
                    Some(a) if a.kind == Kind::Walk => steps_walk += 5.0 * 108.0,
                    Some(_) => steps_walk += rng.range(0.0, 20.0),
                    None => {
                        if (7.0..22.0).contains(&hour) && rng.chance(0.55) {
                            steps_walk += rng.range(20.0, 95.0) * step_factor;
                        }
                    }
                }
                cal += rng.range(0.4, 1.4)
                    + act.map_or(0.0, |a| a.kcal / ((a.end - a.start) as f64 / 300.0));
                active_s += if act.is_some() {
                    300.0
                } else {
                    rng.range(0.0, 150.0)
                };
                if act.is_some_and(|a| a.kind != Kind::Strength) {
                    ascent += rng.range(0.5, 2.5);
                    descent += rng.range(0.5, 2.5);
                }
                if let Some(a) = act {
                    // Intensity minutes: vigorous while running, moderate for walks and lifting.
                    if a.kind == Kind::Run {
                        week_vig += 3.0;
                    } else {
                        week_mod += if a.kind == Kind::Walk { 4.0 } else { 1.0 };
                    }
                } else if (8.0..20.0).contains(&hour) && rng.chance(0.012) {
                    week_mod += 5.0;
                }
            }
            // Respiration every 10 minutes, SpO2 every 15 minutes asleep, others every 15.
            if step_i % 2 == 0 {
                let brpm = if sleeping {
                    rng.range(12.0, 14.6)
                } else if act.is_some_and(|a| a.kind == Kind::Run) {
                    rng.range(24.0, 34.0)
                } else {
                    rng.range(14.0, 17.0)
                };
                buf.add(
                    "respiration",
                    "ts, brpm, brpm_raw, src",
                    vec![
                        i(ts),
                        r((brpm * 10.0).round() / 10.0),
                        r((brpm * 10.0).round() / 10.0),
                        i(SAMPLE_SRC),
                    ],
                );
            }
            if sleeping && step_i % 3 == 0 {
                buf.add(
                    "spo2",
                    "ts, percent, confidence, mode, src",
                    vec![
                        i(ts),
                        r(rng.range(93.0, 97.9).round()),
                        r(100.0),
                        i(2),
                        i(SAMPLE_SRC),
                    ],
                );
            }
            if step_i % 3 == 0 {
                // Cumulative counters since midnight, as the watch writes them.
                buf.add("mon_activity", "ts, activity_type, steps, distance_m, active_calories, active_time_s, duration_min, intensity, src",
                    vec![i(ts), i(6), r(steps_walk.round()), r((steps_walk * 0.74).round()), r(cal.round()), r(active_s.round()), nul(), nul(), i(SAMPLE_SRC)]);
                if steps_run > 0.0 {
                    buf.add("mon_activity", "ts, activity_type, steps, distance_m, active_calories, active_time_s, duration_min, intensity, src",
                        vec![i(ts), i(1), r(steps_run.round()), r((steps_run * 1.0).round()), r(0.0), r(0.0), nul(), nul(), i(SAMPLE_SRC)]);
                }
                buf.add("mon_extra", "ts, ascent_total_m, descent_total_m, moderate_total_min, vigorous_total_min, src",
                    vec![i(ts), r(ascent.round()), r(descent.round()), r(week_mod.round()), r(week_vig.round()), i(SAMPLE_SRC)]);
            }
            ts += 300;
            step_i += 1;
        }

        // Battery: four readings a day, slowly falling, recharged when low.
        for q in 0..4 {
            let ts = day_from + q * 6 * 3600;
            if ts <= now {
                battery -= 0.28;
                if battery < 18.0 {
                    battery = 100.0;
                }
                buf.add(
                    "battery",
                    "ts, percent, src",
                    vec![i(ts), r(battery.round()), i(SAMPLE_SRC)],
                );
            }
        }
    }

    // Training metrics from the activity loads.
    let n_days = opts.days as usize;
    let mut load = vec![0.0_f64; n_days];
    let (mut low, mut high, mut anaerobic) =
        (vec![0.0; n_days], vec![0.0; n_days], vec![0.0; n_days]);
    for a in &acts {
        let day = (settings::date_of(tz, a.start) - first_date).num_days() as usize;
        load[day] += a.load;
        match a.kind {
            Kind::Walk => low[day] += a.load,
            Kind::Run => {
                high[day] += a.load * 0.8;
                anaerobic[day] += a.load * 0.2;
            }
            Kind::Strength => anaerobic[day] += a.load * 0.6,
        }
        let end = a.end;
        let minutes = (a.load * 9.0).clamp(540.0, 2880.0).round();
        buf.add(
            "recovery_time",
            "start_ts, minutes, src",
            vec![i(end), r(minutes), i(SAMPLE_SRC)],
        );
    }
    let window_sum = |v: &[f64], day: usize, len: usize| {
        v[day.saturating_sub(len - 1)..=day].iter().sum::<f64>()
    };
    for day in 0..n_days {
        let ts = day_start(tz, first_date + Duration::days(day as i64)) + 6 * 3600 + 1800;
        if ts > now {
            continue;
        }
        let acute = window_sum(&load, day, 7);
        let chronic = window_sum(&load, day, 28) / 4.0;
        let ratio = if chronic > 0.0 { acute / chronic } else { 0.0 };
        buf.add(
            "training_load",
            "ts, acute, chronic, acwr, acwr_percent, status, src",
            vec![
                i(ts),
                r(acute.round()),
                r(chronic.round()),
                r((ratio * 10.0).round() / 10.0),
                r((ratio * 50.0).round()),
                i(2),
                i(SAMPLE_SRC),
            ],
        );
        // A plausible mix of the verified status codes (1 detraining, 4 maintaining, 5 recovery,
        // 7 productive, 8 strained) from the acute to chronic ratio; the first weeks have no
        // chronic history yet and stay "maintaining".
        let status = match ratio {
            _ if day < 28 => 4,
            r if r > 1.3 => 8,
            r if r > 1.1 => 7,
            r if r >= 0.8 => 4,
            r if r >= 0.5 => 5,
            _ => 1,
        };
        buf.add(
            "training_status",
            "ts, status, weekly_load_sum, load_min, load_max, fitness_trend, load_trend, src",
            vec![
                i(ts),
                i(status),
                r(acute.round()),
                r((chronic * 0.8).round()),
                r((chronic * 1.4).round()),
                i(1),
                i(1),
                i(SAMPLE_SRC),
            ],
        );
        buf.add("training_balance",
            "ts, aerobic_low, aerobic_high, anaerobic, aerobic_low_target_min, aerobic_low_target_max, aerobic_high_target_min, aerobic_high_target_max, anaerobic_target_min, anaerobic_target_max, src",
            vec![i(ts), r(window_sum(&low, day, 28).round()), r(window_sum(&high, day, 28).round()), r(window_sum(&anaerobic, day, 28).round()),
                 r(250.0), r(500.0), r(400.0), r(700.0), r(60.0), r(180.0), i(SAMPLE_SRC)]);
        if day % 7 == 0 {
            buf.add(
                "vo2max",
                "ts, maximal_met, sport, src",
                vec![
                    i(ts + 7 * 3600),
                    r(
                        ((13.0 + 0.2 * day as f64 / n_days as f64 + rng.range(-0.04, 0.04))
                            * 100.0)
                            .round()
                            / 100.0,
                    ),
                    i(1),
                    i(SAMPLE_SRC),
                ],
            );
        }
    }

    // Watch settings row (resting metabolic rate).
    let info_sha = sha_of("sample-monitoring-info");
    buf.add(
        "sources",
        "sha256, src_id, ok, last_ts, msg_count, serial, time_created, file_type, rank_key",
        vec![
            t(&info_sha),
            i(src_id(&info_sha)),
            i(1),
            i(now),
            i(1),
            V::I(None),
            i(now),
            V::I(None),
            V::T(Some(rank_key(true, Some(now), 1))),
        ],
    );
    buf.add(
        "mon_info",
        "sha256, local_offset_s, rmr_kcal_day, steps_goal",
        vec![t(&info_sha), nul(), r(1780.0), nul()],
    );

    // Body measurements: a weigh-in about weekly, body fat and waist about monthly.
    let mut weight = 79.4;
    for k in (0..opts.days).step_by(7) {
        weight += rng.range(-0.5, 0.3);
        let at = day_start(tz, first_date + Duration::days(k)) + 7 * 3600 + 600;
        if at > now {
            continue;
        }
        let monthly = k % 28 == 0;
        buf.add(
            "body_measurements",
            "measured_at, weight_kg, body_fat_pct, waist_cm, created_at",
            vec![
                i(at),
                r((weight * 10.0).round() / 10.0),
                if monthly {
                    r((18.5 - k as f64 * 0.02 + rng.range(-0.3, 0.3)).round() * 1.0)
                } else {
                    nul()
                },
                if monthly {
                    r((86.0 - k as f64 * 0.03).round())
                } else {
                    nul()
                },
                i(at),
            ],
        );
    }

    // Accounts. Hashing is slow on purpose, so it happens before the transaction opens.
    let admin = login_for(
        ADMIN_USER,
        opts.accounts.admin_password.clone(),
        opts.accounts.admin_totp_secret.clone(),
    )?;
    let user = login_for(
        DEMO_USER,
        opts.accounts.user_password.clone(),
        opts.accounts.user_totp_secret.clone(),
    )?;
    let admin_hash = hash(&admin.password).await?;
    let user_hash = hash(&user.password).await?;

    let mut conn = pool.acquire().await?;
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *conn)
        .await?;
    let result = async {
        let mut tx = sqlx::Connection::begin(&mut *conn).await?;
        let mut ids = Vec::new();
        for (login, hash, role) in [
            (&admin, &admin_hash, Role::Admin),
            (&user, &user_hash, Role::User),
        ] {
            let id = sqlx::query(
                "INSERT INTO users (username, password_hash, role, totp_secret, totp_enabled, created_at)
                 VALUES (?, ?, ?, ?, 1, ?)",
            )
            .bind(&login.username)
            .bind(hash)
            .bind(role.as_str())
            .bind(&login.totp_secret)
            .bind(now)
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
            ids.push(id);
        }
        let uid = ids[1];
        flush(&mut tx, buf, uid).await?;
        // The sample files belong to the demo user (their rows have no `files` entry).
        sqlx::query("INSERT INTO file_owners (sha256, user_id) SELECT sha256, ? FROM sources")
            .bind(uid)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO user_settings (user_id, key, value) VALUES (?, 'timezone', ?)")
            .bind(uid)
            .bind(tz.name())
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO settings (key, value) VALUES ('sample_data', '1') ON CONFLICT (key) DO UPDATE SET value = excluded.value")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        anyhow::Ok(uid)
    }
    .await;
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *conn)
        .await?;
    let user_id = result?;
    Ok(SeedReport {
        days: opts.days,
        first_date: settings::date_string(first_date),
        last_date: settings::date_string(end_date),
        activities: acts.len(),
        admin,
        user,
        user_id,
    })
}
