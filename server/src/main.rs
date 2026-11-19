//! nimrag-server CLI. Hand-rolled argument parsing: five fixed subcommands and
//! three flags do not justify a parser dependency.

use anyhow::{bail, Context, Result};
use nimrag_server::{api, db, derive, import, ledger, tokens};
use std::path::PathBuf;

const USAGE: &str = "\
usage:
  nimrag-server serve [--data-dir DIR] [--listen ADDR] [--log-json] [--insecure-cookies]
                      [--trusted-proxy CIDR[,CIDR...]]
  nimrag-server user set-password [--data-dir DIR]  read the owner password from stdin
  nimrag-server dev-seed [--data-dir DIR]          fill an empty data dir with 120 days of sample data
  nimrag-server export-types [--out DIR]           write the API's TypeScript types (default web/src/api/generated)
  nimrag-server token create --name NAME [--data-dir DIR]
  nimrag-server token list [--data-dir DIR]
  nimrag-server token revoke ID [--data-dir DIR]
  nimrag-server reparse [--data-dir DIR]          rebuild derived tables from the files
  nimrag-server import-export [--data-dir DIR] [--list-skipped]
                      [--max-depth N] [--max-entries N] [--max-total-bytes N] <archive.zip | extracted-dir>
  nimrag-server rebuild-ledger [--data-dir DIR]   recreate files/uploads rows from sidecars
--data-dir defaults to $NIMRAG_DATA_DIR or ./data; --listen to 127.0.0.1:8080.
--insecure-cookies drops the Secure flag from the session cookie (plain-http LAN use only) and
  uses the cookie name nimrag_session instead of __Host-nimrag_session.
--trusted-proxy (repeatable, or comma-separated) names the reverse-proxy networks whose
  X-Forwarded-For is believed for login rate limiting: only when the TCP peer is inside one, the
  rightmost entry outside all of them is the client. Without it the peer address is used.";

struct Opts {
    data_dir: PathBuf,
    listen: String,
    log_json: bool,
    insecure_cookies: bool,
    trusted_proxies: Vec<nimrag_server::auth::Cidr>,
    out: Option<PathBuf>,
    list_skipped: bool,
    limits: import::Limits,
    name: Option<String>,
    positional: Vec<String>,
}

fn number<T: std::str::FromStr>(s: &str) -> Result<T> {
    s.parse()
        .map_err(|_| anyhow::anyhow!("{s} is not a number"))
}

fn parse_opts(args: &[String]) -> Result<Opts> {
    let mut o = Opts {
        data_dir: std::env::var_os("NIMRAG_DATA_DIR").map_or_else(|| "data".into(), Into::into),
        listen: "127.0.0.1:8080".into(),
        log_json: false,
        insecure_cookies: false,
        trusted_proxies: vec![],
        out: None,
        list_skipped: false,
        limits: import::Limits::default(),
        name: None,
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
            "--log-json" => o.log_json = true,
            "--insecure-cookies" => o.insecure_cookies = true,
            "--trusted-proxy" => o.trusted_proxies.extend(
                nimrag_server::auth::parse_cidr_list(&value("--trusted-proxy")?)
                    .context("--trusted-proxy")?,
            ),
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
        "serve" | "import-export" | "reparse" | "rebuild-ledger" | "dev-seed"
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
                "reparsed: {} files ({} FIT inputs) -> {} fit files, {} messages, {} parse errors, {} unreadable",
                r.files, r.fit_inputs, r.fit_files, r.messages, r.parse_errors, r.unreadable
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
        "import-export" => import_export(o).await,
        "token" => token(o).await,
        "user" => user(o).await,
        "dev-seed" => {
            let pool = db::open(&o.data_dir).await?;
            let r = nimrag_server::seed::run(
                &pool,
                &o.data_dir,
                nimrag_server::seed::SeedOptions::today(),
            )
            .await?;
            println!(
                "seeded {} days ({} to {}), {} activities, in {}",
                r.days,
                r.first_date,
                r.last_date,
                r.activities,
                o.data_dir.display()
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
        eprintln!("WARNING: no generated FIT profile compiled in; derived tables (activities, wellness) will NOT be filled. See tools/fit-profile/README.md.");
        tracing::warn!("FIT profile unavailable: derived tables will not be filled");
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
            let (id, token) = tokens::create(&pool, &name).await?;
            eprintln!("device {id} ({name}) created; this token is shown once, store it now:");
            println!("{token}");
        }
        Some("list") => {
            for d in tokens::list(&pool).await? {
                println!(
                    "{}\t{}\tcreated {}\tlast used {}\t{}",
                    d.id,
                    d.name,
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
            if !tokens::revoke(&pool, id).await? {
                bail!("no active device with id {id}");
            }
            println!("revoked device {id}");
        }
        _ => bail!("{USAGE}"),
    }
    Ok(())
}

/// `user set-password`: the password is read from stdin (first line), never from argv.
async fn user(o: Opts) -> Result<()> {
    use std::io::{IsTerminal, Read};
    if o.positional.first().map(String::as_str) != Some("set-password") {
        bail!("{USAGE}");
    }
    let mut input = String::new();
    if std::io::stdin().is_terminal() {
        eprint!(
            "New password (typed text is visible; at least {} characters): ",
            nimrag_server::auth::MIN_PASSWORD_LEN
        );
    }
    std::io::stdin().take(4096).read_to_string(&mut input)?;
    let password = input.lines().next().unwrap_or("");
    let pool = db::open(&o.data_dir).await?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    nimrag_server::auth::set_password(&pool, password, now).await?;
    eprintln!("owner password set; existing sessions were signed out");
    Ok(())
}

async fn serve(o: Opts) -> Result<()> {
    let pool = db::open(&o.data_dir).await?;
    let swept = nimrag_server::store::sweep_incoming(&o.data_dir)?;
    if swept > 0 {
        tracing::warn!(swept, "removed leftovers of interrupted uploads");
    }
    let app = api::router(
        api::AppState::new(pool, o.data_dir, api::MAX_BODY_BYTES)
            .with_insecure_cookies(o.insecure_cookies)
            .with_trusted_proxies(o.trusted_proxies.clone()),
    );
    if !o.trusted_proxies.is_empty() {
        tracing::info!(
            networks = o.trusted_proxies.len(),
            "trusting X-Forwarded-For from the configured proxy networks"
        );
    }
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

async fn import_export(o: Opts) -> Result<()> {
    let [input] = o.positional.as_slice() else {
        bail!("import-export needs exactly one archive or directory\n{USAGE}");
    };
    let pool = db::open(&o.data_dir).await?;
    let state = api::AppState::new(pool, o.data_dir.clone(), api::MAX_BODY_BYTES);
    let r = import::run(&state, std::path::Path::new(input), o.limits.clone()).await?;
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
