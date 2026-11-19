//! `nimrag-decode fit <file.fit> [--fields]`: dump a FIT file with nimrag-core.
//! Strict: exit status 2 on any decode error, incomplete file or CRC mismatch.

use std::io::Write;
use std::process::ExitCode;

use nimrag_core::fit::{decode, Crc};

pub const USAGE: &str = "\
usage: nimrag-decode fit [--fields] <file.fit>

  --fields   also print every field's number and raw bytes (hex)
exit status: 0 clean, 1 usage or I/O error, 2 decode error or CRC mismatch
";

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn run(args: &[String]) -> ExitCode {
    let mut fields = false;
    let mut path = None;
    for a in args {
        match a.as_str() {
            "--fields" => fields = true,
            s if path.is_none() && !s.starts_with("--") => path = Some(s.to_string()),
            _ => {
                eprint!("{USAGE}");
                return ExitCode::from(1);
            }
        }
    }
    let Some(path) = path else {
        eprint!("{USAGE}");
        return ExitCode::from(1);
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            return ExitCode::from(1);
        }
    };
    let d = decode(&bytes);
    let mut out = std::io::BufWriter::new(std::io::stdout());
    let mut bad = d.error.is_some();
    let mut total = 0usize;
    for (i, f) in d.files.iter().enumerate() {
        let crc = match f.crc {
            Crc::Ok => "ok".to_string(),
            Crc::Missing => "missing".to_string(),
            Crc::Mismatch { stored, computed } => {
                format!("mismatch(stored={stored:04x},computed={computed:04x})")
            }
        };
        bad |= f.crc != Crc::Ok || !f.complete;
        let _ = writeln!(
            out,
            "file {i} offset={} len={} complete={} crc={crc} messages={}",
            f.offset,
            f.len,
            f.complete,
            f.messages.len()
        );
        for m in &f.messages {
            let ts = m.timestamp().map_or("-".to_string(), |t| t.to_string());
            let _ = writeln!(
                out,
                "  msg global={} index={} ts={ts} fields={}",
                m.global,
                m.index,
                m.fields.len() + m.developer.len()
            );
            if fields {
                for x in &m.fields {
                    let _ = writeln!(out, "    f{} {}", x.num, hex(&x.raw));
                }
                for x in &m.developer {
                    let _ = writeln!(
                        out,
                        "    dev{}.{} {}",
                        x.developer_data_index,
                        x.num,
                        hex(&x.raw)
                    );
                }
            }
        }
        total += f.messages.len();
    }
    let status = if bad { "error" } else { "ok" };
    let _ = writeln!(
        out,
        "summary: files={} messages={total} status={status}",
        d.files.len()
    );
    let _ = out.flush();
    if let Some(e) = &d.error {
        eprintln!("error: {e:?}");
    }
    if bad {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}
