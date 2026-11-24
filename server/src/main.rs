//! nimrag-server CLI. Hand-rolled argument parsing: five fixed subcommands and
//! three flags do not justify a parser dependency.

use anyhow::{bail, Context, Result};
use nimrag_server::{api, backup, connect, db, derive, import, ledger, tokens, users};
use std::path::PathBuf;
use std::time::Duration;

const USAGE: &str = "\
usage:
  nimrag-server serve [--data-dir DIR] [--listen ADDR] [--log-json] [--insecure-cookies]
                      [--trusted-proxy CIDR[,CIDR...]] [--tiles-dir DIR] [--no-gps-assist]
                      [--backup-dir DIR [--backup-every 24h] [--backup-keep 7]]
                      [--weather on|off] [--weather-url URL] [--max-import-bytes N]
  nimrag-server user set-password [--username NAME] [--data-dir DIR]
                                                   read the new password from stdin (default user: the only admin)
  nimrag-server user reset-2fa --username NAME [--data-dir DIR]
                                                   the user enrols 2FA again at the next login
  nimrag-server user list [--data-dir DIR]
  nimrag-server dev-seed [--data-dir DIR]          fill an empty data dir with 120 days of sample data, an admin
                                                   and the user demo (credentials are printed unless given in
                                                   NIMRAG_SEED_{ADMIN,USER}_{PASSWORD,TOTP})
  nimrag-server export-types [--out DIR]           write the API's TypeScript types (default web/src/api/generated)
  nimrag-server token create --name NAME [--user USERNAME] [--data-dir DIR]
  nimrag-server token list [--data-dir DIR]
  nimrag-server token revoke ID [--data-dir DIR]
  nimrag-server reparse [--data-dir DIR]          rebuild derived tables from the files
  nimrag-server import-export [--data-dir DIR] [--user USERNAME] [--list-skipped]
                      [--max-depth N] [--max-entries N] [--max-total-bytes N] <archive.zip | extracted-dir>
  nimrag-server import-connect --user USERNAME [--data-dir DIR] [--timezone IANA] <dir>
                                                   import a directory of Garmin Connect JSON (sleep, HRV, training
                                                   status and load, VO2 max, daily stats, stress, heart rate
                                                   and breathing of a day, activities) for a user
  nimrag-server rebuild-ledger [--data-dir DIR]   recreate files/uploads rows from sidecars
  nimrag-server backup --out DIR [--keep N] [--data-dir DIR]
                                                   one backup now (database snapshot + files); safe while serve runs
  nimrag-server export --user USERNAME --out FILE.zip [--data-dir DIR]
                                                   a user's originals and CSV files as a zip (reads only; safe while serve runs)
  nimrag-server restore --from BACKUP --data-dir NEWDIR
                                                   copy a backup folder into a new, empty data dir
--data-dir defaults to $NIMRAG_DATA_DIR or ./data; --listen to 127.0.0.1:8080.
--user on token create and import-export names the user who owns the token or the imported files;
  without it they stay unassigned until an admin assigns them in the web app (import-connect
  needs it, and reads local dates and times in that user's timezone unless --timezone is given).
--insecure-cookies drops the Secure flag from the session cookie (plain-http LAN use only) and
  uses the cookie name nimrag_session instead of __Host-nimrag_session.
--tiles-dir (or $NIMRAG_TILES_DIR) serves the *.pmtiles files of DIR to signed-in users under
  /tiles/<name>.pmtiles (map tiles for the web app); without it /tiles/* is a 404.
--no-gps-assist (or NIMRAG_GPS_ASSIST=off) switches off GET /api/device/gps-assist, the only place this server
  contacts Garmin (satellite data for the watch's GPS, fetched on the phone's behalf); on otherwise.
--max-import-bytes (or $NIMRAG_MAX_IMPORT_BYTES) is the largest Garmin export the web app's Import from Garmin page takes
  (default 8 GiB; larger gets 413). The upload is kept on disk next to the files while it is read.
--backup-dir (or $NIMRAG_BACKUP_DIR) makes serve back up by itself: every --backup-every ($NIMRAG_BACKUP_EVERY,
  default 24h; 30m, 6h, 1d) a folder nimrag-backup-<UTC time> with a database snapshot and the files, of which
  the newest --backup-keep ($NIMRAG_BACKUP_KEEP, default 7) stay. Without --backup-dir nothing is backed up.
  An admin can also set the folder, interval, keep count and on/off on the Admin page (Backups). Settings
  saved there win over these options (the page can go back to them); with nothing saved the options apply
  exactly as given.
--weather off (or $NIMRAG_WEATHER=off) switches the watch's weather off: POST /api/device/weather answers
  503 and Open-Meteo is never contacted. Default on. --weather-url (or $NIMRAG_WEATHER_URL) replaces
  https://api.open-meteo.com (tests and proxies); the phone only ever talks to this server.
--trusted-proxy (repeatable, or comma-separated) names the reverse-proxy networks whose
  X-Forwarded-For is believed for login rate limiting: only when the TCP peer is inside one, the
  rightmost entry outside all of them is the client. Without it the peer address is used.";

struct Opts {
    data_dir: PathBuf,
    listen: String,
    log_json: bool,
    insecure_cookies: bool,
    trusted_proxies: Vec<nimrag_server::auth::Cidr>,
    tiles_dir: Option<PathBuf>,
    weather: bool,
    weather_url: Option<String>,
    gps_assist: bool,
    backup_dir: Option<PathBuf>,
    backup_every: Duration,
    backup_keep: usize,
    max_import: usize,
    from: Option<PathBuf>,
    keep: Option<usize>,
    out: Option<PathBuf>,
    list_skipped: bool,
    limits: import::Limits,
    name: Option<String>,
    user: Option<String>,
    username: Option<String>,
    timezone: Option<String>,
    positional: Vec<String>,
}

fn number<T: std::str::FromStr>(s: &str) -> Result<T> {
    s.parse()
        .map_err(|_| anyhow::anyhow!("{s} is not a number"))
}

/// `on` / `off` (also 1, 0, true, false, yes, no).
fn on_off(s: &str) -> Result<bool> {
    match s.to_ascii_lowercase().as_str() {
        "on" | "1" | "true" | "yes" => Ok(true),
        "off" | "0" | "false" | "no" => Ok(false),
        _ => bail!("{s} is not on or off"),
    }
}

fn env_text(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn parse_opts(args: &[String]) -> Result<Opts> {
    let mut o = Opts {
        data_dir: std::env::var_os("NIMRAG_DATA_DIR").map_or_else(|| "data".into(), Into::into),
        listen: "127.0.0.1:8080".into(),
        log_json: false,
        insecure_cookies: false,
        trusted_proxies: vec![],
        tiles_dir: std::env::var_os("NIMRAG_TILES_DIR")
            .filter(|v| !v.is_empty())
            .map(Into::into),
        weather: match env_text("NIMRAG_WEATHER") {
            Some(v) => on_off(&v).context("NIMRAG_WEATHER")?,
            None => true,
        },
        weather_url: env_text("NIMRAG_WEATHER_URL"),
        gps_assist: match env_text("NIMRAG_GPS_ASSIST") {
            Some(v) => nimrag_server::gps_assist::parse_switch(&v)
                .map_err(anyhow::Error::msg)
                .context("NIMRAG_GPS_ASSIST")?,
            None => true,
        },
        backup_dir: std::env::var_os("NIMRAG_BACKUP_DIR")
            .filter(|v| !v.is_empty())
            .map(Into::into),
        backup_every: match env_text("NIMRAG_BACKUP_EVERY") {
            Some(v) => backup::parse_interval(&v).context("NIMRAG_BACKUP_EVERY")?,
            None => Duration::from_secs(24 * 3600),
        },
        backup_keep: match env_text("NIMRAG_BACKUP_KEEP") {
            Some(v) => number(&v).context("NIMRAG_BACKUP_KEEP")?,
            None => 7,
        },
        max_import: match env_text("NIMRAG_MAX_IMPORT_BYTES") {
            Some(v) => number(&v).context("NIMRAG_MAX_IMPORT_BYTES")?,
            None => api::MAX_IMPORT_BYTES,
        },
        from: None,
        keep: None,
        out: None,
        list_skipped: false,
        limits: import::Limits::default(),
        name: None,
        user: None,
        username: None,
        timezone: None,
        positional: vec![],
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |flag: &str| -> Result<String> {
            it.next()
                .cloned()
                .with_context(|| format!("{flag} needs a value"))
        };
        match a.as_str() {
            "--data-dir" => o.data_dir = value("--data-dir")?.into(),
            "--listen" => o.listen = value("--listen")?,
            "--name" => o.name = Some(value("--name")?),
            "--user" => o.user = Some(value("--user")?),
            "--username" => o.username = Some(value("--username")?),
            "--timezone" => o.timezone = Some(value("--timezone")?),
            "--log-json" => o.log_json = true,
            "--insecure-cookies" => o.insecure_cookies = true,
            "--trusted-proxy" => o.trusted_proxies.extend(
                nimrag_server::auth::parse_cidr_list(&value("--trusted-proxy")?)
                    .context("--trusted-proxy")?,
            ),
            "--tiles-dir" => o.tiles_dir = Some(value("--tiles-dir")?.into()),
            "--weather" => o.weather = on_off(&value("--weather")?).context("--weather")?,
            "--weather-url" => o.weather_url = Some(value("--weather-url")?),
            "--no-gps-assist" => o.gps_assist = false,
            "--backup-dir" => o.backup_dir = Some(value("--backup-dir")?.into()),
            "--backup-every" => {
                o.backup_every =
                    backup::parse_interval(&value("--backup-every")?).context("--backup-every")?;
            }
            "--backup-keep" => o.backup_keep = number(&value("--backup-keep")?)?,
            "--max-import-bytes" => o.max_import = number(&value("--max-import-bytes")?)?,
            "--keep" => o.keep = Some(number(&value("--keep")?)?),
            "--from" => o.from = Some(value("--from")?.into()),
            "--out" => o.out = Some(value("--out")?.into()),
            "--list-skipped" => o.list_skipped = true,
            "--max-depth" => o.limits.max_depth = number(&value("--max-depth")?)?,
            "--max-entries" => o.limits.max_entries = number(&value("--max-entries")?)?,
            "--max-total-bytes" => {
                o.limits.max_total_bytes = number(&value("--max-total-bytes")?)?;
            }
            s if s.starts_with("--") => bail!("unknown flag {s}\n{USAGE}"),
            _ => o.positional.push(a.clone()),
        }
    }
    Ok(o)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else {
        bail!("{USAGE}");
    };
    let o = parse_opts(rest)?;
    init_logging(o.log_json);
    // Hold for the whole process: one writer per data dir.
    let _lock = if matches!(
        cmd.as_str(),
        "serve" | "import-export" | "import-connect" | "reparse" | "rebuild-ledger" | "dev-seed"
    ) {
        Some(db::lock_data_dir(&o.data_dir, cmd)?)
    } else {
        None
    };
    if matches!(cmd.as_str(), "serve" | "reparse" | "import-export") {
        warn_if_no_profile();
    }

    match cmd.as_str() {
        "serve" => serve(o).await,
        "reparse" => {
            let pool = db::open(&o.data_dir).await?;
            let r = derive::reparse(&pool, &o.data_dir).await?;
            println!(
                "reparsed: {} files ({} FIT inputs, {} Garmin Connect inputs) -> {} fit files, {} messages, {} parse errors, {} unreadable",
                r.files,
                r.fit_inputs,
                r.connect_inputs,
                r.fit_files,
                r.messages,
                r.parse_errors,
                r.unreadable
            );
            if r.unreadable > 0 {
                bail!(
                    "{} blobs are missing or corrupt (see file_parse errors)",
                    r.unreadable
                );
            }
            Ok(())
        }
        "rebuild-ledger" => {
            let pool = db::open(&o.data_dir).await?;
            let r = ledger::rebuild(&pool, &o.data_dir).await?;
            println!(
                "ledger rebuilt: {} sidecars, {} files added, {} upload events",
                r.sidecars, r.files_added, r.uploads
            );
            Ok(())
        }
        "backup" => backup_now(o).await,
        "restore" => restore(o).await,
        "export" => export(o).await,
        "import-export" => import_export(o).await,
        "import-connect" => import_connect(o).await,
        "token" => token(o).await,
        "user" => user(o).await,
        "dev-seed" => {
            let pool = db::open(&o.data_dir).await?;
            let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
            let given = nimrag_server::seed::SeedAccounts {
                admin_password: env("NIMRAG_SEED_ADMIN_PASSWORD"),
                admin_totp_secret: env("NIMRAG_SEED_ADMIN_TOTP"),
                user_password: env("NIMRAG_SEED_USER_PASSWORD"),
                user_totp_secret: env("NIMRAG_SEED_USER_TOTP"),
            };
            let mut opts = nimrag_server::seed::SeedOptions::today();
            opts.accounts = given.clone();
            let r = nimrag_server::seed::run(&pool, &o.data_dir, opts).await?;
            println!(
                "seeded {} days ({} to {}), {} activities, in {}",
                r.days,
                r.first_date,
                r.last_date,
                r.activities,
                o.data_dir.display()
            );
            // Credentials go to stdout, to whoever ran the command; only the ones the caller did
            // not supply themselves.
            let show = |l: &nimrag_server::seed::SeedLogin, pw: bool, totp: bool| {
                if pw {
                    println!("{} password: {}", l.username, l.password);
                }
                if totp {
                    println!("{} 2FA secret (base32): {}", l.username, l.totp_secret);
                }
            };
            show(
                &r.admin,
                given.admin_password.is_none(),
                given.admin_totp_secret.is_none(),
            );
            show(
                &r.user,
                given.user_password.is_none(),
                given.user_totp_secret.is_none(),
            );
            Ok(())
        }
        "export-types" => {
            let dir = o
                .out
                .unwrap_or_else(|| PathBuf::from("web/src/api/generated"));
            nimrag_server::web_api::export_types(&dir)?;
            println!("TypeScript types written to {}", dir.display());
            Ok(())
        }
        _ => bail!("unknown command {cmd}\n{USAGE}"),
    }
}

fn warn_if_no_profile() {
    if !nimrag_core::fit::profile::available() {
        eprintln!("WARNING: no FIT field map or profile compiled in; derived tables (activities, wellness) will NOT be filled. See tools/fit-profile/README.md.");
        tracing::warn!("FIT field map unavailable: derived tables will not be filled");
    }
}

fn init_logging(json: bool) {
    // Logs go to stderr so that stdout carries only command output (tokens).
    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let b = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr);
    if json {
        b.json().init();
    } else {
        b.init();
    }
}

async fn token(o: Opts) -> Result<()> {
    let pool = db::open(&o.data_dir).await?;
    match o.positional.first().map(String::as_str) {
        Some("create") => {
            let name = o.name.context("token create needs --name NAME")?;
            let owner = match &o.user {
                Some(u) => Some(find_user(&pool, u).await?.id),
                None => {
                    eprintln!(
                        "note: no --user given; the token is unassigned until an admin assigns it"
                    );
                    None
                }
            };
            let (id, token) = tokens::create(&pool, &name, owner).await?;
            eprintln!("device {id} ({name}) created; this token is shown once, store it now:");
            println!("{token}");
        }
        Some("list") => {
            for d in tokens::list(&pool, tokens::Whose::All).await? {
                println!(
                    "{}\t{}\t{}\tcreated {}\tlast used {}\t{}",
                    d.id,
                    d.name,
                    d.username.unwrap_or_else(|| "(unassigned)".into()),
                    d.created_at,
                    d.last_used_at.unwrap_or_else(|| "never".into()),
                    d.revoked_at
                        .map_or("active".into(), |t| format!("revoked {t}"))
                );
            }
        }
        Some("revoke") => {
            let id: i64 = o
                .positional
                .get(1)
                .context("token revoke needs ID")?
                .parse()
                .context("ID must be a number")?;
            if !tokens::revoke(&pool, id, None).await? {
                bail!("no active device with id {id}");
            }
            println!("revoked device {id}");
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}

async fn find_user(pool: &sqlx::SqlitePool, name: &str) -> Result<users::User> {
    users::by_username(pool, name)
        .await?
        .with_context(|| format!("no user named {name}"))
}

/// `user set-password` (the password is read from stdin, first line, never from argv),
/// `user reset-2fa` and `user list`: the recovery tools of whoever runs the server.
async fn user(o: Opts) -> Result<()> {
    use std::io::{IsTerminal, Read};
    let pool = db::open(&o.data_dir).await?;
    match o.positional.first().map(String::as_str) {
        Some("set-password") => {
            let target = match &o.username {
                Some(n) => find_user(&pool, n).await?,
                None => {
                    let admins: Vec<_> = users::list(&pool)
                        .await?
                        .into_iter()
                        .filter(|u| u.role == users::Role::Admin)
                        .collect();
                    match <[users::User; 1]>::try_from(admins) {
                        Ok([only]) => only,
                        Err(_) => {
                            bail!("--username NAME is needed: there is not exactly one admin")
                        }
                    }
                }
            };
            let mut input = String::new();
            if std::io::stdin().is_terminal() {
                eprint!(
                    "New password for {} (typed text is visible; at least {} characters): ",
                    target.username,
                    nimrag_server::auth::MIN_PASSWORD_LEN
                );
            }
            std::io::stdin().take(4096).read_to_string(&mut input)?;
            let password = input.lines().next().unwrap_or("").to_owned();
            if password.chars().count() < nimrag_server::auth::MIN_PASSWORD_LEN {
                bail!(
                    "the password must have at least {} characters",
                    nimrag_server::auth::MIN_PASSWORD_LEN
                );
            }
            let hash =
                tokio::task::spawn_blocking(move || nimrag_server::auth::hash_password(&password))
                    .await??;
            users::set_password(&pool, target.id, &hash, false).await?;
            eprintln!(
                "password of {} set; its sessions and trusted devices were signed out",
                target.username
            );
        }
        Some("reset-2fa") => {
            let name = o
                .username
                .as_deref()
                .context("user reset-2fa needs --username NAME")?;
            let target = find_user(&pool, name).await?;
            users::reset_totp(&pool, target.id).await?;
            eprintln!(
                "2FA of {} cleared; they enrol again at the next login",
                target.username
            );
        }
        Some("list") => {
            for u in users::list(&pool).await? {
                let twofa = if u.totp_enabled { "on" } else { "not enrolled" };
                let state = if u.disabled_at.is_some() {
                    "disabled"
                } else {
                    "active"
                };
                println!(
                    "{}\t{}\t{}\t2FA {twofa}\t{state}",
                    u.id,
                    u.username,
                    u.role.as_str()
                );
            }
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}

async fn serve(o: Opts) -> Result<()> {
    let pool = db::open(&o.data_dir).await?;
    let swept = nimrag_server::store::sweep_incoming(&o.data_dir)?;
    if swept > 0 {
        tracing::warn!(swept, "removed leftovers of interrupted uploads");
    }
    let backup_startup = match &o.backup_dir {
        Some(dir) => {
            if o.backup_every < Duration::from_secs(60) {
                bail!("--backup-every must be at least 1m");
            }
            if o.backup_keep == 0 {
                bail!("--backup-keep must be at least 1");
            }
            Some(backup::Config {
                dir: dir.clone(),
                every: o.backup_every,
                keep: o.backup_keep,
            })
        }
        None => None,
    };
    let state = api::AppState::new(pool, o.data_dir, api::MAX_BODY_BYTES)
        .with_backup_startup(backup_startup)
        .with_max_import(o.max_import)
        .with_insecure_cookies(o.insecure_cookies)
        .with_trusted_proxies(o.trusted_proxies.clone())
        .with_tiles_dir(o.tiles_dir.clone())
        .with_weather(nimrag_server::weather::WeatherConfig {
            enabled: o.weather,
            base_url: o
                .weather_url
                .clone()
                .unwrap_or_else(|| nimrag_server::weather::DEFAULT_BASE_URL.into()),
            ..Default::default()
        })
        .with_gps_assist(nimrag_server::gps_assist::GpsAssistConfig {
            enabled: o.gps_assist,
            ..Default::default()
        });
    tracing::info!(
        weather = if o.weather { "on" } else { "off" },
        "weather for the watch (Open-Meteo)"
    );
    if o.gps_assist {
        tracing::info!("GPS assist on: /api/device/gps-assist fetches satellite data from Garmin for the phone");
    } else {
        tracing::info!("GPS assist off: this server makes no Garmin request for the watch's GPS");
    }
    if let Some(dir) = &o.tiles_dir {
        let files = std::fs::read_dir(dir).map(|d| {
            d.flatten()
                .filter(|e| nimrag_server::tiles::valid_name(&e.file_name().to_string_lossy()))
                .count()
        });
        match files {
            Ok(n) => tracing::info!(dir = %dir.display(), files = n, "serving map tiles at /tiles"),
            Err(e) => tracing::warn!(
                dir = %dir.display(),
                error = %e,
                "--tiles-dir cannot be read; /tiles answers 404 until it can"
            ),
        }
    }
    if !o.trusted_proxies.is_empty() {
        tracing::info!(
            networks = o.trusted_proxies.len(),
            "trusting X-Forwarded-For from the configured proxy networks"
        );
    }
    // Backups: the settings the admin saved in the web app win over the start-up options.
    state.backups.reload(&state.pool).await?;
    match state.backups.current() {
        Some(cfg) => tracing::info!(
            dir = %cfg.dir.display(),
            every_s = cfg.every.as_secs(),
            keep = cfg.keep,
            source = if nimrag_server::backup::load_saved(&state.pool).await?.is_some() {
                "web app"
            } else {
                "start-up options"
            },
            "scheduled backups on"
        ),
        None => tracing::info!("scheduled backups off (the admin page can switch them on)"),
    }
    backup::spawn_scheduler_with(
        state.pool.clone(),
        state.data_dir.as_ref().clone(),
        state.backups.clone(),
    );
    // Forward mode: uploads queued activity files to Garmin Connect (idle until a user logs in to Garmin).
    nimrag_server::garmin::queue::spawn_worker(state.clone());
    let app = api::router(state);
    if o.insecure_cookies {
        tracing::warn!(
            "session cookies are sent without the Secure flag; use only on a trusted plain-http network"
        );
    }
    let listener = tokio::net::TcpListener::bind(&o.listen)
        .await
        .with_context(|| format!("binding {}", o.listen))?;
    tracing::info!(listen = %o.listen, "serving");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}

/// `backup`: one backup now. It reads the database through a read-only connection and does not
/// take the data-dir lock, so it works while `serve` runs.
async fn backup_now(o: Opts) -> Result<()> {
    let out = o
        .out
        .context("backup needs --out DIR (a folder outside the data dir)")?;
    let pool = db::open_read_only(&o.data_dir).await?;
    let r = backup::run_once(&pool, &o.data_dir, &out, o.keep).await?;
    println!("backup written to {}", r.path.display());
    println!("{}", r.summary());
    for p in &r.pruned {
        println!("pruned {p}");
    }
    Ok(())
}

/// `export`: the same zip as `GET /api/export`, for a named user, into a file. Reads the database
/// through a read-only connection and does not take the data-dir lock.
async fn export(o: Opts) -> Result<()> {
    let name = o.user.as_deref().context("export needs --user USERNAME")?;
    let out = o.out.context("export needs --out FILE.zip")?;
    let pool = db::open_read_only(&o.data_dir).await?;
    db::require_current_schema(&pool).await?;
    let user = find_user(&pool, name).await?;
    if user.role == users::Role::Admin {
        bail!(
            "{} is an admin account, which owns no data; pick a normal user",
            user.username
        );
    }
    let prefs = nimrag_server::settings::load(&pool, user.id).await?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    // Written beside the target and renamed when whole: a failed export leaves no half archive.
    let mut part = out.clone().into_os_string();
    part.push(".part");
    let part = PathBuf::from(part);
    let (data_dir, rt) = (o.data_dir.clone(), tokio::runtime::Handle::current());
    let (p2, part2) = (pool.clone(), part.clone());
    let who = nimrag_server::export::ExportUser {
        id: user.id,
        username: user.username.clone(),
    };
    let result = tokio::task::spawn_blocking(move || {
        let file = std::fs::File::create_new(&part2)
            .with_context(|| format!("creating {}", part2.display()))?;
        let (summary, file) = nimrag_server::export::write_zip(
            &rt,
            &p2,
            &data_dir,
            &who,
            prefs.tz,
            now,
            std::io::BufWriter::new(file),
        )?;
        file.into_inner()
            .map_err(|e| anyhow::anyhow!("writing the archive: {e}"))?
            .sync_all()?;
        anyhow::Ok(summary)
    })
    .await?;
    match result {
        Ok(s) => {
            std::fs::rename(&part, &out)
                .with_context(|| format!("renaming into {}", out.display()))?;
            println!(
                "{}: {} CSV files ({} rows), {} original files{}",
                out.display(),
                s.csv_files,
                s.rows,
                s.originals,
                if s.originals_missing > 0 {
                    format!(
                        " ({} listed files are not in the store)",
                        s.originals_missing
                    )
                } else {
                    String::new()
                }
            );
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// `restore`: copy a backup folder into a new data dir. Run `reparse` on it afterwards to
/// rebuild the derived tables from the files if you want them freshly derived.
async fn restore(o: Opts) -> Result<()> {
    let from = o
        .from
        .context("restore needs --from BACKUP (a backup folder)")?;
    let r = backup::restore(&from, &o.data_dir).await?;
    println!(
        "restored {} files ({} bytes) and the database ({} bytes) into {}",
        r.files,
        r.files_bytes,
        r.db_bytes,
        o.data_dir.display()
    );
    if let Some(m) = r.manifest {
        println!("backup taken {}", m.created_at);
    }
    println!("start the server on it, or run `nimrag-server reparse --data-dir ...` first to derive the tables again");
    Ok(())
}

async fn import_connect(o: Opts) -> Result<()> {
    let [input] = o.positional.as_slice() else {
        bail!("import-connect needs exactly one directory\n{USAGE}");
    };
    let username = o
        .user
        .as_deref()
        .context("import-connect needs --user USERNAME: the history belongs to a user")?;
    let pool = db::open(&o.data_dir).await?;
    let user = find_user(&pool, username).await?;
    if user.role == users::Role::Admin {
        bail!(
            "{} is an admin account, which owns no data; pick a normal user",
            user.username
        );
    }
    let tz = match &o.timezone {
        Some(name) => name
            .parse::<chrono_tz::Tz>()
            .map_err(|_| anyhow::anyhow!("{name} is not an IANA time zone name"))?,
        None => nimrag_server::settings::load(&pool, user.id).await?.tz,
    };
    let state = api::AppState::new(pool, o.data_dir.clone(), api::MAX_BODY_BYTES);
    let r = connect::import::run(&state, std::path::Path::new(input), user.id, tz).await?;
    println!("user:                 {}", user.username);
    println!("time zone:            {}", tz.name());
    println!("JSON files read:      {}", r.files);
    println!("blobs stored new:     {}", r.new_blobs());
    println!("blobs already there:  {}", r.present_blobs());
    for (kind, (new, present)) in &r.by_kind {
        println!("  {kind}: {new} new, {present} already there");
    }
    println!("activities with records: {}", r.with_records);
    if !r.records_without_activity.is_empty() {
        println!(
            "records without an activity file (not stored): {}",
            r.records_without_activity.len()
        );
    }
    println!("not Connect JSON (not stored): {}", r.unrecognised.len());
    for p in &r.unrecognised {
        println!("    {p}");
    }
    println!("errors:               {}", r.errors.len());
    for e in &r.errors {
        eprintln!("error: {e}");
    }
    if !r.errors.is_empty() {
        bail!("{} files failed", r.errors.len());
    }
    Ok(())
}

async fn import_export(o: Opts) -> Result<()> {
    let [input] = o.positional.as_slice() else {
        bail!("import-export needs exactly one archive or directory\n{USAGE}");
    };
    let pool = db::open(&o.data_dir).await?;
    let state = api::AppState::new(pool, o.data_dir.clone(), api::MAX_BODY_BYTES);
    let owner = match &o.user {
        Some(u) => Some(find_user(&state.pool, u).await?.id),
        None => None,
    };
    let r = import::run(&state, std::path::Path::new(input), o.limits.clone(), owner).await?;
    println!("entries scanned:      {}", r.scanned);
    println!("FIT imported new:     {}", r.fit_new);
    println!("FIT already present:  {}", r.fit_present);
    let non_fit: usize = r.non_fit.values().map(Vec::len).sum();
    println!("non-FIT (not stored): {non_fit}");
    for (ext, paths) in &r.non_fit {
        println!("  .{ext}: {}", paths.len());
        let shown = if o.list_skipped { paths.len() } else { 5 };
        for p in paths.iter().take(shown) {
            println!("    {p}");
        }
        if paths.len() > shown {
            println!(
                "    ... {} more (--list-skipped lists all)",
                paths.len() - shown
            );
        }
    }
    if !r.unsafe_names.is_empty() {
        println!(
            "unsafe names (imported, name kept as metadata only): {}",
            r.unsafe_names.len()
        );
        for n in &r.unsafe_names {
            println!("    {n}");
        }
    }
    println!("errors:               {}", r.errors.len());
    for e in &r.errors {
        eprintln!("error: {e}");
    }
    if !r.errors.is_empty() {
        bail!("{} entries failed", r.errors.len());
    }
    Ok(())
}
