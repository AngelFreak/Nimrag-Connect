//! GPS assist: what the watch may ask for under `/ephemeris/`, what a file has
//! to look like before it is sent, and the reply (status, headers, body) the
//! watch gets.
//!
//! Ported from Gadgetbridge commit 7257742b (AGPL-3.0-or-later):
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/http/interceptors/AgpsInterceptor.java`
//! (Copyright (C) 2024-2026 José Rebelo, kuhy, Daniele Gobbetti, Thomas Kuehne
//! and the other Gadgetbridge contributors; `supports`, `handle`: the hosts and
//! the path prefix, the 304 check before any validation, `cache-control`, the
//! content type, the choice of format), `agps/GarminAgpsFile.java`
//! (`isValidTar`, `isValidRxNetworks`, `isValidSonyCpe`, the two header
//! constants), `agps/GarminAgpsDataType.java` (constellation names and file
//! names) and `util/GBTarFile.java` (the tar header walk). *inferred
//! (Gadgetbridge)*: no Instinct 2 request for any of this has been seen
//! (Q-COMP-1).
//!
//! Differences, marked [ours]:
//! - Gadgetbridge computes the ETag as the MD5 of a file the user supplies;
//!   here the host gives the ETag of the file it hands over (Garmin's own, from
//!   the Nimrag server) and it is sent as given.
//! - The format is chosen when the watch asks (an unknown address is refused
//!   without bothering the host), not after the file was read.
//! - The tar walk is bounds-checked, and a size that is not octal makes the
//!   file invalid instead of an exception.
//! - A body is refused when it is empty or over 1 MiB (Gadgetbridge reads at
//!   most 1 MiB).
//! - The gzip reply to `accept-encoding: gzip` is stored deflate, not
//!   compressed (`gzip_stored`); a 304 is not gzipped.
//! - `If-None-Match` accepts a list and the weak `W/` form.
//! - The RxNetworks check inflates only the first six bytes, with a small
//!   inflate of our own, because `core` has no dependencies.

/// Hosts whose `/ephemeris/` requests are served (`AgpsInterceptor.supports`).
pub const HOSTS: [&str; 2] = ["api.gcs.garmin.com", "api.gcs.garmin.cn"];
pub const PATH_PREFIX: &str = "/ephemeris/";
/// `cache-control` of a 200 (4 hours).
pub const CACHE_CONTROL: &str = "max-age=14400";
/// Largest body sent (`FileUtils.readAll(agpsIn, 1024 * 1024)`).
pub const MAX_BODY: usize = 1024 * 1024;
/// The longest address (`path?query`) the duty passes on.
pub const MAX_PATH_AND_QUERY: usize = 512;

/// RxNetworks data are valid for a week (`isValidRxNetworks`).
const RX_MAX_AGE_S: i64 = 604_800;
const RX_HEADER: [u8; 2] = [0x01, 0x00];
const SONY_HEADER: [u8; 4] = [0x2A, 0x12, 0xA0, 0x02];
const GZ_HEADER: [u8; 2] = [0x1F, 0x8B];

const TAR_BLOCK: usize = 512;
const TAR_MAGIC: &[u8; 6] = b"ustar\0";
const TAR_MAGIC_AT: usize = 257;

/// The constellations a tar can hold (`GarminAgpsDataType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constellation {
    Gps,
    Glonass,
    Galileo,
    Qzss,
}

impl Constellation {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "GPS" => Self::Gps,
            "GLONASS" => Self::Glonass,
            "GALILEO" => Self::Galileo,
            "QZSS" => Self::Qzss,
            _ => return None,
        })
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Self::Gps => "CPE_GPS.BIN",
            Self::Glonass => "CPE_GLO.BIN",
            Self::Galileo => "CPE_GAL.BIN",
            Self::Qzss => "CPE_QZSS.BIN",
        }
    }
}

/// The three body formats (`AgpsInterceptor.handle`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Format {
    /// A tar with one file per requested constellation (`constellations` query).
    Tar(Vec<Constellation>),
    /// Gzip of RxNetworks data (path contains `/rxnetworks/`).
    RxNetworks,
    /// Sony CPE (path starts `/ephemeris/cpe/sony`).
    SonyCpe,
}

/// Why a body is not sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    Empty,
    TooBig,
    NotTar,
    BadTar,
    MissingFile,
    NotGzip,
    NotRxNetworks,
    RxTimestamp,
    NotSony,
}

/// Whether `s` (a path and query) is safe to hand to the Nimrag server and from
/// there to Garmin: it starts with `/ephemeris/`, is at most 512 bytes long, has
/// one `?` at most and only letters, digits and `/ . _ - , = & ? % +`, no `..`,
/// no `//` and no encoded dot or slash (`%2e`, `%2f`, either case). The server
/// applies the same rule. [ours]
pub fn is_safe_path_and_query(s: &str) -> bool {
    if !s.starts_with(PATH_PREFIX) || s.len() > MAX_PATH_AND_QUERY {
        return false;
    }
    if s.bytes().filter(|&b| b == b'?').count() > 1 {
        return false;
    }
    let ok = s.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'/' | b'.' | b'_' | b'-' | b',' | b'=' | b'&' | b'?' | b'%' | b'+'
            )
    });
    if !ok || s.contains("..") || s.contains("//") {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    !(lower.contains("%2e") || lower.contains("%2f"))
}

/// The format a request wants, or `None` when it is none of the three or names
/// a constellation we do not know (Gadgetbridge refuses those too).
pub fn classify(path_and_query: &str) -> Option<Format> {
    let (path, query) = match path_and_query.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path_and_query, None),
    };
    if !path.starts_with(PATH_PREFIX) {
        return None;
    }
    if let Some(list) = query.and_then(|q| query_value(q, "constellations")) {
        let mut wanted = Vec::new();
        for name in list.split(',') {
            wanted.push(Constellation::from_name(name)?);
        }
        return Some(Format::Tar(wanted));
    }
    if path.contains("/rxnetworks/") {
        Some(Format::RxNetworks)
    } else if path.starts_with("/ephemeris/cpe/sony") {
        Some(Format::SonyCpe)
    } else {
        None
    }
}

/// The value of `key` in a query: the last one wins, `%2C` read as a comma.
fn query_value(query: &str, key: &str) -> Option<String> {
    let mut found = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == key {
            found = Some(v.replace("%2C", ",").replace("%2c", ","));
        }
    }
    found
}

/// Check `body` against its format (`GarminAgpsFile`). `now_unix_s` is only
/// used for RxNetworks.
pub fn validate(format: &Format, body: &[u8], now_unix_s: i64) -> Result<(), Invalid> {
    match format {
        Format::Tar(wanted) => validate_tar(wanted, body),
        Format::RxNetworks => validate_rx(body, now_unix_s),
        Format::SonyCpe => {
            if body.starts_with(&SONY_HEADER) {
                Ok(())
            } else {
                Err(Invalid::NotSony)
            }
        }
    }
}

fn has_magic(b: &[u8], block: usize) -> bool {
    b.get(block + TAR_MAGIC_AT..block + TAR_MAGIC_AT + TAR_MAGIC.len()) == Some(&TAR_MAGIC[..])
}

fn validate_tar(wanted: &[Constellation], b: &[u8]) -> Result<(), Invalid> {
    if !has_magic(b, 0) {
        return Err(Invalid::NotTar);
    }
    let mut names: Vec<&[u8]> = Vec::new();
    let mut at = 0usize;
    while at + TAR_BLOCK <= b.len() && has_magic(b, at) {
        let header = &b[at..at + TAR_BLOCK];
        let name = &header[..100];
        let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())];
        names.push(name);
        let size = octal(&header[124..136]).ok_or(Invalid::BadTar)?;
        let blocks = size.div_ceil(TAR_BLOCK) + 1;
        at = blocks
            .checked_mul(TAR_BLOCK)
            .and_then(|n| at.checked_add(n))
            .ok_or(Invalid::BadTar)?;
    }
    for c in wanted {
        if !names.contains(&c.file_name().as_bytes()) {
            return Err(Invalid::MissingFile);
        }
    }
    Ok(())
}

/// A tar number field: octal digits, NUL or space padded.
fn octal(field: &[u8]) -> Option<usize> {
    let end = field.iter().position(|&c| c == 0).unwrap_or(field.len());
    let text = std::str::from_utf8(&field[..end]).ok()?.trim();
    usize::from_str_radix(text, 8).ok()
}

fn validate_rx(b: &[u8], now_unix_s: i64) -> Result<(), Invalid> {
    if !b.starts_with(&GZ_HEADER) {
        return Err(Invalid::NotGzip);
    }
    let head = gunzip_prefix(b, 6).ok_or(Invalid::NotRxNetworks)?;
    if head[..2] != RX_HEADER {
        return Err(Invalid::NotRxNetworks);
    }
    // A signed 32-bit big-endian time, as `ByteBuffer.getInt` reads it.
    let ts = i64::from(i32::from_be_bytes([head[2], head[3], head[4], head[5]]));
    let age = now_unix_s - ts;
    if !(0..=RX_MAX_AGE_S).contains(&age) {
        return Err(Invalid::RxTimestamp);
    }
    Ok(())
}

// ---- the reply -----------------------------------------------------------------------------

/// What goes to the watch for a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// 200 or 304.
    pub status: u16,
    /// In the order sent (Gadgetbridge's insertion order): `etag`,
    /// `cache-control` and `Content-Type` for a 200, `etag` alone for a 304.
    pub headers: Vec<(String, String)>,
    /// Empty for a 304.
    pub body: Vec<u8>,
}

/// Shape the answer to one request from the host's file and ETag.
///
/// `etag` is the header value as it must be sent (quotes included); empty means
/// the host has none. A request whose `if_none_match` matches gets a 304 before
/// anything else is looked at, as in Gadgetbridge; otherwise the body must pass
/// `validate`.
pub fn build_reply(
    format: &Format,
    body: &[u8],
    etag: &str,
    if_none_match: Option<&str>,
    accept: Option<&str>,
    now_unix_s: i64,
) -> Result<Reply, Invalid> {
    if body.is_empty() {
        return Err(Invalid::Empty);
    }
    if body.len() > MAX_BODY {
        return Err(Invalid::TooBig);
    }
    let mut headers = Vec::new();
    if !etag.is_empty() {
        headers.push(("etag".to_string(), etag.to_string()));
        if if_none_match.is_some_and(|inm| etag_matches(inm, etag)) {
            return Ok(Reply {
                status: 304,
                headers,
                body: Vec::new(),
            });
        }
    }
    headers.push(("cache-control".into(), CACHE_CONTROL.into()));
    validate(format, body, now_unix_s)?;
    headers.push((
        "Content-Type".into(),
        accept.unwrap_or("application/octet-stream").into(),
    ));
    Ok(Reply {
        status: 200,
        headers,
        body: body.to_vec(),
    })
}

/// `If-None-Match` against our ETag: exact, or one entry of a list, weak
/// entries (`W/"x"`) compared by their tag. `*` matches. [ours]
fn etag_matches(if_none_match: &str, etag: &str) -> bool {
    let strong = |s: &str| s.trim().trim_start_matches("W/").to_string();
    let ours = strong(etag);
    if_none_match
        .split(',')
        .map(strong)
        .any(|candidate| candidate == "*" || candidate == ours)
}

// ---- gzip ----------------------------------------------------------------------------------

/// CRC-32 (the gzip one, reflected 0xEDB88320).
pub fn crc32(data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut t = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
                k += 1;
            }
            t[i] = c;
            i += 1;
        }
        t
    };
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = TABLE[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// `data` as a gzip file made of stored (uncompressed) deflate blocks: valid for
/// any gzip reader, no smaller than the input. [ours]
pub fn gzip_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1F, 0x8B, 0x08, 0x00, 0, 0, 0, 0, 0x00, 0xFF];
    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    }
    let mut blocks = data.chunks(0xFFFF).peekable();
    while let Some(block) = blocks.next() {
        out.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

/// The first `want` bytes of a gzip stream's content, `None` when the header or
/// the deflate data are broken, or the content is shorter. The trailer (CRC and
/// size) is not checked. Never panics on any input.
pub fn gunzip_prefix(gz: &[u8], want: usize) -> Option<Vec<u8>> {
    if gz.len() < 18 || gz[..2] != GZ_HEADER || gz[2] != 8 {
        return None;
    }
    let flags = gz[3];
    let mut at = 10usize;
    if flags & 4 != 0 {
        let n = usize::from(u16::from_le_bytes([*gz.get(at)?, *gz.get(at + 1)?]));
        at = at.checked_add(2 + n)?;
    }
    for flag in [8u8, 16] {
        if flags & flag != 0 {
            let nul = gz.get(at..)?.iter().position(|&c| c == 0)?;
            at += nul + 1;
        }
    }
    if flags & 2 != 0 {
        at = at.checked_add(2)?;
    }
    inflate_prefix(gz.get(at..)?, want)
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    count: u32,
}

impl Bits<'_> {
    fn bits(&mut self, n: u32) -> Option<u32> {
        while self.count < n {
            self.buf |= u32::from(*self.data.get(self.pos)?) << self.count;
            self.pos += 1;
            self.count += 8;
        }
        let v = self.buf & ((1u32 << n) - 1);
        self.buf >>= n;
        self.count -= n;
        Some(v)
    }

    fn align(&mut self) {
        self.buf = 0;
        self.count = 0;
    }

    fn byte(&mut self) -> Option<u8> {
        let b = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }
}

/// Canonical Huffman code in the "counts per length, symbols in order" form.
struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Option<Huffman> {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[usize::from(l)] += 1;
        }
        // An over-subscribed set is invalid; an incomplete one is allowed.
        let mut left: i32 = 1;
        for &c in &count[1..] {
            left = (left << 1) - i32::from(c);
            if left < 0 {
                return None;
            }
        }
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[usize::from(offs[usize::from(l)])] = sym as u16;
                offs[usize::from(l)] += 1;
            }
        }
        Some(Huffman { count, symbol })
    }

    fn decode(&self, br: &mut Bits) -> Option<u16> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= br.bits(1)? as i32;
            let count = i32::from(self.count[len]);
            if code - count < first {
                return self.symbol.get((index + (code - first)) as usize).copied();
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        None
    }
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Inflate raw deflate data until `want` bytes are out. `None` when the data
/// are invalid or end before that many bytes.
fn inflate_prefix(data: &[u8], want: usize) -> Option<Vec<u8>> {
    let mut br = Bits {
        data,
        pos: 0,
        buf: 0,
        count: 0,
    };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = br.bits(1)?;
        match br.bits(2)? {
            0 => {
                br.align();
                let len = u16::from_le_bytes([br.byte()?, br.byte()?]);
                let nlen = u16::from_le_bytes([br.byte()?, br.byte()?]);
                if len != !nlen {
                    return None;
                }
                for _ in 0..len {
                    out.push(br.byte()?);
                    if out.len() >= want {
                        return Some(out);
                    }
                }
            }
            1 => {
                let mut lengths = [8u8; 288];
                lengths[144..256].fill(9);
                lengths[256..280].fill(7);
                let lit = Huffman::new(&lengths)?;
                let dist = Huffman::new(&[5u8; 30])?;
                if codes(&mut br, &mut out, want, &lit, &dist)? {
                    return Some(out);
                }
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut br)?;
                if codes(&mut br, &mut out, want, &lit, &dist)? {
                    return Some(out);
                }
            }
            _ => return None,
        }
        if last == 1 {
            return (out.len() >= want).then_some(out);
        }
    }
}

fn dynamic_tables(br: &mut Bits) -> Option<(Huffman, Huffman)> {
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let nlen = br.bits(5)? as usize + 257;
    let ndist = br.bits(5)? as usize + 1;
    let ncode = br.bits(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return None;
    }
    let mut lengths = [0u8; 320];
    for &slot in ORDER.iter().take(ncode) {
        lengths[slot] = br.bits(3)? as u8;
    }
    let code_lengths = Huffman::new(&lengths[..19])?;
    let mut index = 0;
    let mut lens = [0u8; 320];
    while index < nlen + ndist {
        let sym = code_lengths.decode(br)?;
        if sym < 16 {
            lens[index] = sym as u8;
            index += 1;
            continue;
        }
        let (value, repeat) = match sym {
            16 => {
                if index == 0 {
                    return None;
                }
                (lens[index - 1], 3 + br.bits(2)? as usize)
            }
            17 => (0, 3 + br.bits(3)? as usize),
            _ => (0, 11 + br.bits(7)? as usize),
        };
        if index + repeat > nlen + ndist {
            return None;
        }
        lens[index..index + repeat].fill(value);
        index += repeat;
    }
    if lens[256] == 0 {
        return None;
    }
    Some((
        Huffman::new(&lens[..nlen])?,
        Huffman::new(&lens[nlen..nlen + ndist])?,
    ))
}

/// Decode one block's symbols. `Some(true)` when `want` bytes are out,
/// `Some(false)` at the end of the block, `None` on invalid data.
fn codes(
    br: &mut Bits,
    out: &mut Vec<u8>,
    want: usize,
    lit: &Huffman,
    dist: &Huffman,
) -> Option<bool> {
    loop {
        let sym = usize::from(lit.decode(br)?);
        match sym {
            0..=255 => {
                out.push(sym as u8);
                if out.len() >= want {
                    return Some(true);
                }
            }
            256 => return Some(false),
            257..=285 => {
                let i = sym - 257;
                let len = usize::from(LEN_BASE[i]) + br.bits(u32::from(LEN_EXTRA[i]))? as usize;
                let d = usize::from(dist.decode(br)?);
                let back = usize::from(*DIST_BASE.get(d)?)
                    + br.bits(u32::from(*DIST_EXTRA.get(d)?))? as usize;
                if back > out.len() {
                    return None;
                }
                for _ in 0..len {
                    out.push(out[out.len() - back]);
                    if out.len() >= want {
                        return Some(true);
                    }
                }
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// A tar with these (name, size) entries, the way `tar` lays it out: a
    /// ustar header, the data padded to 512, two zero blocks at the end.
    fn tar(entries: &[(&str, usize)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, size) in entries {
            let mut h = [0u8; 512];
            h[..name.len()].copy_from_slice(name.as_bytes());
            h[100..107].copy_from_slice(b"0100644");
            let sz = format!("{size:011o}");
            h[124..135].copy_from_slice(sz.as_bytes());
            h[257..263].copy_from_slice(b"ustar\0");
            h[263..265].copy_from_slice(b"00");
            out.extend_from_slice(&h);
            let data_at = out.len();
            out.resize(data_at + *size, 0xAB);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.resize(out.len() + 1024, 0);
        out
    }

    #[test]
    fn the_path_rule_is_strict() {
        let good =
            "/ephemeris/cpe/sony/lle?coverage=WEEKS_1&constellations=GPS,GLONASS,GALILEO,QZSS";
        assert!(is_safe_path_and_query(good));
        assert!(is_safe_path_and_query("/ephemeris/rxnetworks/x"));
        for bad in [
            "/weather/x",
            "ephemeris/x",
            "/ephemeris/../x",
            "/ephemeris//x",
            "/ephemeris/a?b?c",
            "/ephemeris/a b",
            "/ephemeris/a\n",
            "/ephemeris/a#frag",
            "/ephemeris/a;b",
            "/ephemeris/%2e%2e/x",
            "/ephemeris/a%2Fb",
            "/ephemeris/\u{e9}",
            "http://evil/ephemeris/x",
            "",
        ] {
            assert!(!is_safe_path_and_query(bad), "{bad:?}");
        }
        let long = format!("/ephemeris/{}", "a".repeat(MAX_PATH_AND_QUERY));
        assert!(!is_safe_path_and_query(&long));
    }

    #[test]
    fn requests_are_classified_like_gadgetbridge() {
        use Constellation::*;
        assert_eq!(
            classify(
                "/ephemeris/cpe/sony/lle?coverage=WEEKS_1&constellations=GPS,GLONASS,GALILEO,QZSS"
            ),
            Some(Format::Tar(vec![Gps, Glonass, Galileo, Qzss]))
        );
        // The constellations query wins over the path.
        assert_eq!(
            classify("/ephemeris/rxnetworks/a?constellations=QZSS"),
            Some(Format::Tar(vec![Qzss]))
        );
        assert_eq!(
            classify("/ephemeris/x?constellations=GPS%2CGALILEO"),
            Some(Format::Tar(vec![Gps, Galileo]))
        );
        assert_eq!(
            classify("/ephemeris/rxnetworks/a"),
            Some(Format::RxNetworks)
        );
        assert_eq!(classify("/ephemeris/cpe/sony/lle"), Some(Format::SonyCpe));
        // Unknown: refused.
        assert_eq!(classify("/ephemeris/other"), None);
        assert_eq!(classify("/ephemeris/x?constellations=GPS,BEIDOU"), None);
        assert_eq!(classify("/ephemeris/x?constellations="), None);
        assert_eq!(classify("/ephemeris/x?constellations=gps"), None);
        assert_eq!(classify("/other/x?constellations=GPS"), None);
    }

    #[test]
    fn the_last_constellations_parameter_wins() {
        assert_eq!(
            classify("/ephemeris/x?constellations=GPS&constellations=QZSS"),
            Some(Format::Tar(vec![Constellation::Qzss]))
        );
    }

    #[test]
    fn a_tar_needs_every_requested_file() {
        use Constellation::*;
        let t = tar(&[("CPE_GPS.BIN", 700), ("CPE_GAL.BIN", 10)]);
        assert_eq!(validate(&Format::Tar(vec![Gps, Galileo]), &t, NOW), Ok(()));
        assert_eq!(validate(&Format::Tar(vec![Gps]), &t, NOW), Ok(()));
        assert_eq!(
            validate(&Format::Tar(vec![Gps, Qzss]), &t, NOW),
            Err(Invalid::MissingFile)
        );
        // An empty list of constellations cannot be asked for, and a tar with
        // nothing missing passes.
        assert_eq!(validate(&Format::Tar(vec![]), &t, NOW), Ok(()));
        assert_eq!(
            validate(&Format::Tar(vec![Gps]), b"not a tar", NOW),
            Err(Invalid::NotTar)
        );
        assert_eq!(
            validate(&Format::Tar(vec![Gps]), &[], NOW),
            Err(Invalid::NotTar)
        );
    }

    #[test]
    fn a_broken_tar_is_invalid_and_never_panics() {
        use Constellation::*;
        let mut t = tar(&[("CPE_GPS.BIN", 100)]);
        // A size that is not octal.
        t[124..135].copy_from_slice(b"zzzzzzzzzzz");
        assert_eq!(
            validate(&Format::Tar(vec![Gps]), &t, NOW),
            Err(Invalid::BadTar)
        );
        // A huge size runs past the end: the walk stops, the entry was seen.
        let mut t = tar(&[("CPE_GPS.BIN", 100)]);
        t[124..135].copy_from_slice(b"77777777777");
        assert_eq!(validate(&Format::Tar(vec![Gps]), &t, NOW), Ok(()));
        // Every prefix of a real-looking tar is handled.
        let t = tar(&[("CPE_GPS.BIN", 100), ("CPE_GAL.BIN", 100)]);
        for n in 0..t.len() {
            let _ = validate(&Format::Tar(vec![Gps, Galileo]), &t[..n], NOW);
        }
    }

    #[test]
    fn sony_cpe_is_recognised_by_its_first_bytes() {
        assert_eq!(
            validate(&Format::SonyCpe, &[0x2A, 0x12, 0xA0, 0x02, 9], NOW),
            Ok(())
        );
        assert_eq!(
            validate(&Format::SonyCpe, &[0x2A, 0x12, 0xA0], NOW),
            Err(Invalid::NotSony)
        );
        assert_eq!(validate(&Format::SonyCpe, &[], NOW), Err(Invalid::NotSony));
    }

    // Three gzip files of `01 00` + big-endian time 1799996400 (an hour before
    // `NOW`) + text, made with zlib: stored, fixed-Huffman and dynamic-Huffman
    // blocks.
    const GZ_STORED: &str =
        "1f8b0800000000000403010e00f1ff01006b49c3f068656c6c6f2072780bcfa45c0e000000";
    const GZ_FIXED: &str = "1f8b08000000000002036364c8f63cfc21233527275fa1a802000bcfa45c0e000000";
    const GZ_DYNAMIC: [u8; 180] = [
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x03, 0x75, 0x51, 0x41, 0x0e, 0xc3,
        0x20, 0x0c, 0x6b, 0x7f, 0xc2, 0x77, 0xfa, 0x8c, 0xa0, 0xa2, 0x76, 0x2a, 0xdd, 0x2a, 0x8d,
        0x13, 0xdf, 0xdb, 0x87, 0xf6, 0x84, 0x09, 0xbc, 0xad, 0x4e, 0x28, 0x17, 0x14, 0x25, 0xb6,
        0x63, 0x93, 0x71, 0xd8, 0xa6, 0xd7, 0x3b, 0x1c, 0xcf, 0x5b, 0x7c, 0xdc, 0x5d, 0x0e, 0x49,
        0x9c, 0xc4, 0x63, 0x15, 0x97, 0xd6, 0x52, 0xcf, 0x21, 0xfe, 0x3b, 0x8b, 0xec, 0xbb, 0x38,
        0x5f, 0xda, 0x15, 0xc7, 0x88, 0x52, 0x79, 0x4b, 0x38, 0x47, 0x3f, 0x7d, 0x48, 0x94, 0x0e,
        0x89, 0xa1, 0x84, 0x1a, 0x6a, 0xbc, 0x10, 0x61, 0x29, 0xbc, 0x8c, 0x51, 0xc6, 0x5b, 0x50,
        0xeb, 0x00, 0x72, 0x15, 0x1e, 0x3a, 0x5a, 0x67, 0xc2, 0x0b, 0x92, 0xe7, 0xbe, 0x02, 0x21,
        0x40, 0xb6, 0x89, 0x38, 0x17, 0x74, 0x3a, 0x56, 0xbe, 0xc3, 0xd4, 0xb4, 0xa0, 0xc0, 0x37,
        0x40, 0x2c, 0xfa, 0x6f, 0xda, 0x7d, 0xe1, 0xa9, 0x17, 0xc8, 0x1c, 0x2d, 0x1b, 0xff, 0x8a,
        0xd1, 0xcf, 0xdd, 0x5c, 0x52, 0x2f, 0xe7, 0xe4, 0x5e, 0x07, 0xa3, 0xb5, 0x9a, 0x63, 0x79,
        0xfc, 0x0f, 0x74, 0xe9, 0x0a, 0xf9, 0x00, 0x7f, 0xe0, 0x40, 0xd6, 0xc1, 0x02, 0x00, 0x00,
    ];

    #[test]
    fn inflate_reads_stored_fixed_and_dynamic_blocks() {
        let want = [0x01, 0x00, 0x6b, 0x49, 0xc3, 0xf0];
        for gz in [hex(GZ_STORED), hex(GZ_FIXED), GZ_DYNAMIC.to_vec()] {
            assert_eq!(gunzip_prefix(&gz, 6).as_deref(), Some(&want[..]));
        }
        assert_eq!(
            gunzip_prefix(&hex(GZ_STORED), 14).as_deref(),
            Some(&b"\x01\x00\x6b\x49\xc3\xf0hello rx"[..])
        );
        assert_eq!(
            gunzip_prefix(&hex(GZ_FIXED), 14).as_deref(),
            Some(&b"\x01\x00\x6b\x49\xc3\xf0hello rx"[..])
        );
        // Longer than the content: refused.
        assert_eq!(gunzip_prefix(&hex(GZ_FIXED), 15), None);
        // The dynamic block decodes completely (705 bytes of words after the
        // six header bytes).
        let all = gunzip_prefix(&GZ_DYNAMIC, 705).unwrap();
        assert_eq!(all.len(), 705);
        assert_eq!(all[..6], want);
        assert!(all[6..]
            .iter()
            .all(|&c| c == b' ' || c.is_ascii_lowercase()));
        assert!(String::from_utf8(all[6..].to_vec())
            .unwrap()
            .split(' ')
            .all(
                |w| ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta"]
                    .contains(&w)
            ));
    }

    #[test]
    fn inflate_skips_the_optional_gzip_header_parts() {
        // FNAME set: "name.bin\0" before the data.
        let gz = hex("1f8b08080000000000ff6e616d652e62696e006364c8f63cfc21233527275fa1a802000bcfa45c0e000000");
        assert_eq!(
            gunzip_prefix(&gz, 6).as_deref(),
            Some(&[0x01, 0x00, 0x6b, 0x49, 0xc3, 0xf0][..])
        );
    }

    #[test]
    fn inflate_never_panics_on_damaged_data() {
        for gz in [hex(GZ_STORED), hex(GZ_FIXED), GZ_DYNAMIC.to_vec()] {
            for n in 0..gz.len() {
                let _ = gunzip_prefix(&gz[..n], 6);
            }
            for i in 10..gz.len() {
                let mut g = gz.clone();
                g[i] ^= 0xFF;
                let _ = gunzip_prefix(&g, 6);
                g[i] = 0;
                let _ = gunzip_prefix(&g, 700);
            }
        }
        assert_eq!(gunzip_prefix(&[], 6), None);
        assert_eq!(gunzip_prefix(&[0x1f, 0x8b], 6), None);
    }

    #[test]
    fn rxnetworks_needs_its_header_and_a_fresh_time() {
        assert_eq!(validate(&Format::RxNetworks, &hex(GZ_STORED), NOW), Ok(()));
        assert_eq!(validate(&Format::RxNetworks, &hex(GZ_FIXED), NOW), Ok(()));
        assert_eq!(validate(&Format::RxNetworks, &GZ_DYNAMIC, NOW), Ok(()));
        // Exactly 7 days old is fine, one second more is not.
        let ts = 1_799_996_400;
        assert_eq!(
            validate(&Format::RxNetworks, &hex(GZ_STORED), ts + 604_800),
            Ok(())
        );
        assert_eq!(
            validate(&Format::RxNetworks, &hex(GZ_STORED), ts + 604_801),
            Err(Invalid::RxTimestamp)
        );
        // In the future.
        assert_eq!(
            validate(&Format::RxNetworks, &hex(GZ_STORED), ts - 1),
            Err(Invalid::RxTimestamp)
        );
        assert_eq!(
            validate(&Format::RxNetworks, b"plain", NOW),
            Err(Invalid::NotGzip)
        );
        // A gzip of something else.
        let other = gzip_stored(b"\x02\x00abcd more bytes");
        assert_eq!(
            validate(&Format::RxNetworks, &other, NOW),
            Err(Invalid::NotRxNetworks)
        );
    }

    #[test]
    fn gzip_stored_is_a_valid_gzip_of_the_input() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
        let small = gzip_stored(b"abc");
        assert_eq!(
            small,
            [
                0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0x00, 0xff, 0x01, 0x03, 0x00, 0xfc, 0xff, b'a',
                b'b', b'c', 0xc2, 0x41, 0x24, 0x35, 0x03, 0x00, 0x00, 0x00
            ]
        );
        assert_eq!(gunzip_prefix(&small, 3).as_deref(), Some(&b"abc"[..]));
        // Two blocks for a body over 65535 bytes.
        let big: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
        let gz = gzip_stored(&big);
        assert_eq!(gz.len(), 10 + 5 + 65_535 + 5 + (70_000 - 65_535) + 8);
        assert_eq!(gunzip_prefix(&gz, 70_000).as_deref(), Some(&big[..]));
        assert_eq!(&gz[gz.len() - 4..], &70_000u32.to_le_bytes());
        // Empty input still makes a valid stream.
        assert_eq!(
            gunzip_prefix(&gzip_stored(b""), 0).as_deref(),
            Some(&[][..])
        );
    }

    fn sony_body() -> Vec<u8> {
        let mut b = vec![0x2A, 0x12, 0xA0, 0x02];
        b.extend_from_slice(&[7; 40]);
        b
    }

    #[test]
    fn a_reply_is_200_with_the_gadgetbridge_headers() {
        let body = sony_body();
        let r = build_reply(&Format::SonyCpe, &body, "\"e1\"", None, None, NOW).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(
            r.headers,
            vec![
                ("etag".to_string(), "\"e1\"".to_string()),
                ("cache-control".to_string(), "max-age=14400".to_string()),
                (
                    "Content-Type".to_string(),
                    "application/octet-stream".to_string()
                ),
            ]
        );
        assert_eq!(r.body, body);
        // The request's accept header is the content type.
        let r = build_reply(
            &Format::SonyCpe,
            &body,
            "\"e1\"",
            None,
            Some("application/x-tar"),
            NOW,
        )
        .unwrap();
        assert_eq!(r.headers[2].1, "application/x-tar");
        // No ETag from the host: none sent.
        let r = build_reply(&Format::SonyCpe, &body, "", Some("\"e1\""), None, NOW).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.headers[0].0, "cache-control");
    }

    #[test]
    fn a_matching_etag_gives_304_before_any_check() {
        // The body is not even a valid Sony file: the 304 comes first.
        let r = build_reply(
            &Format::SonyCpe,
            b"junk",
            "\"e1\"",
            Some("\"e1\""),
            None,
            NOW,
        )
        .unwrap();
        assert_eq!(r.status, 304);
        assert_eq!(r.headers, vec![("etag".to_string(), "\"e1\"".to_string())]);
        assert!(r.body.is_empty());
        for inm in ["W/\"e1\"", "\"x\", \"e1\"", "*"] {
            let r = build_reply(&Format::SonyCpe, b"junk", "\"e1\"", Some(inm), None, NOW).unwrap();
            assert_eq!(r.status, 304, "{inm}");
        }
        // Another ETag: the file is checked and sent.
        assert_eq!(
            build_reply(
                &Format::SonyCpe,
                b"junk",
                "\"e1\"",
                Some("\"e0\""),
                None,
                NOW
            ),
            Err(Invalid::NotSony)
        );
        let r = build_reply(
            &Format::SonyCpe,
            &sony_body(),
            "\"e1\"",
            Some("\"e0\""),
            None,
            NOW,
        )
        .unwrap();
        assert_eq!(r.status, 200);
    }

    #[test]
    fn empty_and_oversize_bodies_are_refused() {
        assert_eq!(
            build_reply(&Format::SonyCpe, &[], "\"e\"", None, None, NOW),
            Err(Invalid::Empty)
        );
        let mut big = sony_body();
        big.resize(MAX_BODY, 0);
        assert!(build_reply(&Format::SonyCpe, &big, "\"e\"", None, None, NOW).is_ok());
        big.push(0);
        assert_eq!(
            build_reply(&Format::SonyCpe, &big, "\"e\"", None, None, NOW),
            Err(Invalid::TooBig)
        );
    }
}
