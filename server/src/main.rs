//! nimrag-server CLI. Hand-rolled argument parsing: five fixed subcommands and
//! three flags do not justify a parser dependency.

use anyhow::{bail, Context, Result};
use nimrag_server::{api, db, derive, ledger, tokens};
use std::path::PathBuf;

const USAGE: &str = "\
usage:
  nimrag-server serve [--data-dir DIR] [--listen ADDR] [--log-json]
  nimrag-server token create --name NAME [--data-dir DIR]
  nimrag-server token list [--data-dir DIR]
  nimrag-server token revoke ID [--data-dir DIR]
  nimrag-server reparse [--data-dir DIR]          rebuild derived tables from the files
  nimrag-server rebuild-ledger [--data-dir DIR]   recreate files/uploads rows from sidecars
--data-dir defaults to $NIMRAG_DATA_DIR or ./data; --listen to 127.0.0.1:8080";

struct Opts {
    data_dir: PathBuf,
    listen: String,
    log_json: bool,
    name: Option<String>,
    positional: Vec<String>,
}

fn parse_opts(args: &[String]) -> Result<Opts> {
    let mut o = Opts {
        data_dir: std::env::var_os("NIMRAG_DATA_DIR").map_or_else(|| "data".into(), Into::into),
        listen: "127.0.0.1:8080".into(),
        log_json: false,
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
        "token" => token(o).await,
        _ => bail!("unknown command {cmd}\n{USAGE}"),
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

async fn serve(o: Opts) -> Result<()> {
    let pool = db::open(&o.data_dir).await?;
    let swept = nimrag_server::store::sweep_incoming(&o.data_dir)?;
    if swept > 0 {
        tracing::warn!(swept, "removed leftovers of interrupted uploads");
    }
    let app = api::router(api::AppState::new(pool, o.data_dir, api::MAX_BODY_BYTES));
    let listener = tokio::net::TcpListener::bind(&o.listen)
        .await
        .with_context(|| format!("binding {}", o.listen))?;
    tracing::info!(listen = %o.listen, "serving");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
