//! nimrag-decode: print the Garmin watch link protocol found in an Android
//! HCI snoop log, a btsnoop stream on stdin, or a forwarded snoop socket.
//!
//! Thin CLI over nimrag-core; all parsing lives there.

mod fit_cmd;

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use nimrag_core::capture::{self, Direction, L2capReassembler, Record, ATT_CID};
use nimrag_core::describe::{describe, file_type_name, hex};
use nimrag_core::link::{Event, InfoPage, Link, LinkDecoder};
use nimrag_core::multilink::{service_name, Control};

const USAGE: &str = "\
usage: nimrag-decode [options] <btsnoop_hci.log | - | tcp:HOST:PORT>

  -            read a btsnoop stream from stdin
  tcp:H:P      read a btsnoop stream from a TCP socket, e.g. after
               `adb forward tcp:8872 tcp:8872` with the snoop log socket enabled
options:
  --verbose    also print control frames, info pages, service data, MLR events
  --extract D  write every downloaded file to directory D (name: index_type_sub.bin)
  --strict     exit with status 2 if any frame could not be decoded

subcommand: nimrag-decode fit [--fields] <file.fit>   (see `nimrag-decode fit`)
";

struct Options {
    source: String,
    verbose: bool,
    extract: Option<PathBuf>,
    strict: bool,
}

fn parse_args() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    let mut o = Options {
        source: String::new(),
        verbose: false,
        extract: None,
        strict: false,
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--verbose" | "-v" => o.verbose = true,
            "--strict" => o.strict = true,
            "--extract" => {
                o.extract = Some(args.next().ok_or("--extract needs a directory")?.into())
            }
            "--help" | "-h" => return Err(String::new()),
            s if o.source.is_empty() => o.source = s.to_string(),
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    if o.source.is_empty() {
        return Err("missing input".into());
    }
    Ok(o)
}

/// Stable short labels for multi-link client ids (raw ids are not printed).
#[derive(Default)]
struct ClientLabels(HashMap<u64, String>);

impl ClientLabels {
    /// Label for a session: client label plus the notify handle of its pair.
    fn link(&mut self, l: Link) -> String {
        format!("{}@{:02x}", self.get(l.client), l.channel)
    }

    fn get(&mut self, id: u64) -> String {
        let n = self.0.len();
        self.0
            .entry(id)
            .or_insert_with(|| {
                if id == 1 {
                    "c1".into()
                } else {
                    format!("c{}", (b'A' + n as u8) as char)
                }
            })
            .clone()
    }
}

struct Decoder {
    opts: Options,
    l2cap: L2capReassembler,
    link: LinkDecoder,
    labels: ClientLabels,
    first_ts: Option<i64>,
    counts: BTreeMap<String, usize>,
    unknown: usize,
    errors: usize,
    files: usize,
    out: std::io::BufWriter<std::io::Stdout>,
}

impl Decoder {
    fn record(&mut self, rec: &Record) {
        let Some(pdu) = self.l2cap.push(rec) else {
            return;
        };
        if pdu.cid != ATT_CID {
            return;
        }
        let Some(att) = capture::parse_att(&pdu.payload) else {
            return;
        };
        let t0 = *self.first_ts.get_or_insert(pdu.timestamp_us);
        let t = (pdu.timestamp_us - t0) as f64 / 1e6;
        for ev in self.link.push(pdu.direction, &att) {
            self.event(t, ev);
        }
    }

    fn event(&mut self, t: f64, ev: Event) {
        let arrow = |d: Direction| if d == Direction::Sent { "->" } else { "<-" };
        let line = match ev {
            Event::Gfdi {
                direction,
                client,
                message,
            } => {
                *self
                    .counts
                    .entry(format!(
                        "{} {}",
                        arrow(direction),
                        nimrag_core::gfdi::message_name(message.id).unwrap_or("Unknown")
                    ))
                    .or_default() += 1;
                if nimrag_core::gfdi::message_name(message.id).is_none() {
                    self.unknown += 1;
                }
                if message.id == 5004 && !self.opts.verbose {
                    return; // chunk traffic is summarised by FileDownloaded
                }
                if message.id == 5000
                    && message.as_response().is_some_and(|r| r.request_id == 5004)
                    && !self.opts.verbose
                {
                    return;
                }
                Some(format!(
                    "{} [{}] {}",
                    arrow(direction),
                    self.labels.link(client),
                    describe(&message)
                ))
            }
            Event::GfdiError {
                direction,
                client,
                error,
                frame,
            } => {
                self.errors += 1;
                Some(format!(
                    "{} [{}] GFDI ERROR {error} frame={}",
                    arrow(direction),
                    self.labels.link(client),
                    hex(&frame)
                ))
            }
            Event::Unknown {
                direction,
                att_handle,
                value,
                reason,
            } => {
                self.unknown += 1;
                Some(format!(
                    "{} UNKNOWN att=0x{att_handle:04x} ({reason}) {}",
                    arrow(direction),
                    hex(&value)
                ))
            }
            Event::FileDownloaded {
                client,
                index,
                entry,
                data,
                crc_ok,
            } => {
                let kind = entry
                    .as_ref()
                    .map(|e| file_type_name(e.data_type, e.sub_type))
                    .unwrap_or(if index == 0 { "directory" } else { "?" });
                if let Some(dir) = &self.opts.extract {
                    let (dt, st) = entry
                        .as_ref()
                        .map(|e| (e.data_type, e.sub_type))
                        .unwrap_or((0, 0));
                    self.files += 1;
                    let path = dir.join(format!("{:03}_idx{index:05}_{dt}_{st}.bin", self.files));
                    if let Err(e) =
                        std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, &data))
                    {
                        eprintln!("cannot write {}: {e}", path.display());
                    }
                }
                if !crc_ok {
                    self.errors += 1;
                }
                Some(format!(
                    "== [{}] file index={index} ({kind}) {} bytes, running CRC {}",
                    self.labels.link(client),
                    data.len(),
                    if crc_ok { "ok" } else { "MISMATCH" }
                ))
            }
            Event::Directory { client, entries } => {
                let mut s = format!(
                    "== [{}] directory: {} entries",
                    self.labels.link(client),
                    entries.len()
                );
                for e in entries {
                    s.push_str(&format!("\n     {}", e.describe()));
                }
                Some(s)
            }
            _ if !self.opts.verbose => None,
            Event::MtuExchange { direction, mtu } => {
                Some(format!("{} ATT MTU {mtu}", arrow(direction)))
            }
            Event::DescriptorWrite {
                att_handle,
                notify,
                indicate,
            } => Some(format!(
                "-> GATT CCCD att=0x{att_handle:04x} notify={notify} indicate={indicate}"
            )),
            Event::Control {
                direction,
                client,
                control,
            } => {
                let who = client
                    .map(|c| self.labels.get(c))
                    .unwrap_or_else(|| "?".into());
                Some(format!(
                    "{} [{who}] ML {}",
                    arrow(direction),
                    describe_control(&control)
                ))
            }
            Event::InfoPage {
                direction,
                client,
                page,
            } => Some(format!(
                "{} [{}] ML info {}",
                arrow(direction),
                self.labels.link(client),
                describe_page(&page)
            )),
            Event::ServiceData {
                direction,
                client,
                service,
                data,
            } => Some(format!(
                "{} [{}] {} {}",
                arrow(direction),
                self.labels.link(client),
                service_name(service),
                hex(&data)
            )),
            Event::MlrDuplicate {
                direction,
                client,
                handle,
                seq,
            } => Some(format!(
                "{} [{}] MLR h{handle} retransmission seq={seq}",
                arrow(direction),
                self.labels.link(client)
            )),
            Event::MlrOutOfOrder {
                direction,
                client,
                handle,
                seq,
                expected,
            } => {
                // Normal after a radio loss: the receiver drops it and the
                // sender retransmits (go-back-N). Not a decode error.
                Some(format!(
                    "{} [{}] MLR h{handle} out of order seq={seq} expected={expected} (dropped)",
                    arrow(direction),
                    self.labels.link(client)
                ))
            }
        };
        if let Some(l) = line {
            let _ = writeln!(self.out, "{t:9.3} {l}");
            if self.opts.source == "-" || self.opts.source.starts_with("tcp:") {
                let _ = self.out.flush(); // live source: show each line as it happens
            }
        }
    }

    fn finish(&mut self) -> ExitCode {
        let _ = writeln!(self.out, "\n-- summary --");
        for (k, v) in &self.counts {
            let _ = writeln!(self.out, "{v:6}  {k}");
        }
        let _ = writeln!(
            self.out,
            "unknown frames: {}  decode errors: {}  orphan ACL fragments: {}",
            self.unknown, self.errors, self.l2cap.orphan_fragments
        );
        let _ = self.out.flush();
        if self.opts.strict && (self.unknown > 0 || self.errors > 0) {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        }
    }
}

fn describe_control(c: &Control) -> String {
    match c {
        Control::RegisterRequest { service, mode, .. } => format!("register {}({service}) mode={mode}", service_name(*service)),
        Control::RegisterResponse { service, status, handle, flags, extra, redirect, .. } => format!(
            "register-response {}({service}) {} handle={handle:?} reliable={} extra={extra:?} redirect={redirect:?}",
            service_name(*service),
            Control::register_status_name(*status),
            flags.is_some_and(|f| f & 1 != 0)
        ),
        Control::CloseHandleRequest { service, handle, .. } => format!("close-handle {}({service}) handle={handle}", service_name(*service)),
        Control::CloseHandleResponse { service, handle, status, .. } => format!("close-handle-response {}({service}) handle={handle} status={status}", service_name(*service)),
        Control::InvalidHandle { handle, .. } => format!("invalid-handle {handle}"),
        Control::CloseAllRequest { .. } => "close-all".into(),
        Control::CloseAllResponse { status, .. } => format!("close-all-response status={status}"),
        Control::UnknownCommandResponse { original, .. } => format!("unknown-command-response original={original}"),
        Control::Unparsed { kind, raw } => format!("unparsed kind={kind} {}", hex(raw)),
    }
}

fn describe_page(p: &InfoPage) -> String {
    match p {
        InfoPage::Selector(b) => format!("request page {}", hex(b)),
        InfoPage::SupportedServices(v) => format!(
            "supported services {:?}",
            v.iter()
                .map(|s| format!("{}({s})", service_name(*s as u16)))
                .collect::<Vec<_>>()
        ),
        InfoPage::AdvertisingData(n) => format!("advertising data ({n} bytes, not shown)"),
        InfoPage::Version {
            major,
            minor,
            micro,
        } => format!("multi-link version {major}.{minor}.{micro}"),
        InfoPage::ProductInfo { product, firmware } => format!(
            "product={product} firmware={}.{:02} unit_id=<redacted>",
            firmware / 100,
            firmware % 100
        ),
        InfoPage::IdentityAddress => "identity address <redacted>".into(),
        InfoPage::ServiceRevision {
            service,
            status,
            revision,
        } => format!(
            "service {} revision status={status} revision={revision:?}",
            service_name(*service)
        ),
        InfoPage::Unsupported(p) => format!("page {p} unsupported"),
        InfoPage::Raw(b) => format!("raw {}", hex(b)),
    }
}

fn open_stream(source: &str) -> std::io::Result<Box<dyn Read>> {
    if source == "-" {
        return Ok(Box::new(std::io::stdin()));
    }
    if let Some(addr) = source.strip_prefix("tcp:") {
        return Ok(Box::new(std::net::TcpStream::connect(addr)?));
    }
    Ok(Box::new(std::fs::File::open(source)?))
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("fit") {
        return fit_cmd::run(&argv[1..]);
    }
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("error: {e}");
            }
            eprint!("{USAGE}");
            return ExitCode::from(1);
        }
    };
    let mut input = match open_stream(&opts.source) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: cannot open {}: {e}", opts.source);
            return ExitCode::from(1);
        }
    };
    let mut d = Decoder {
        opts,
        l2cap: L2capReassembler::new(),
        link: LinkDecoder::new(),
        labels: ClientLabels::default(),
        first_ts: None,
        counts: BTreeMap::new(),
        unknown: 0,
        errors: 0,
        files: 0,
        out: std::io::BufWriter::new(std::io::stdout()),
    };
    let mut stream = capture::BtsnoopStream::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                eprintln!("error: read failed: {e}");
                break;
            }
        };
        match stream.push(&buf[..n]) {
            Ok(records) => records.iter().for_each(|r| d.record(r)),
            Err(e) => {
                eprintln!("error: not a btsnoop stream: {e:?}");
                return ExitCode::from(1);
            }
        }
    }
    d.finish()
}
