//! Capture parsing: btsnoop records -> HCI ACL -> L2CAP (reassembled) -> ATT.
//!
//! Sans-IO: callers hand in bytes, these functions return parsed values. Used
//! by tools/decode for Android HCI snoop logs and live snoop streams.

use std::collections::HashMap;

/// Direction of a packet relative to the phone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Phone -> watch.
    Sent,
    /// Watch -> phone.
    Received,
}

/// One btsnoop record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub direction: Direction,
    /// Microseconds since 0000-01-01 (btsnoop epoch).
    pub timestamp_us: i64,
    /// H4 packet (first byte = packet type).
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtsnoopError {
    BadMagic,
    UnsupportedDatalink(u32),
    Truncated { offset: usize },
}

/// Microseconds between the btsnoop epoch (0 AD) and the Unix epoch.
pub const BTSNOOP_UNIX_OFFSET_US: i64 = 0x00dc_ddb3_0f2f_8000;

/// Parse a whole btsnoop file. Trailing partial records are reported as Truncated.
pub fn parse_btsnoop(file: &[u8]) -> Result<Vec<Record>, BtsnoopError> {
    if file.len() < 16 || &file[..8] != b"btsnoop\0" {
        return Err(BtsnoopError::BadMagic);
    }
    let datalink = u32::from_be_bytes(file[12..16].try_into().unwrap());
    if datalink != 1002 {
        return Err(BtsnoopError::UnsupportedDatalink(datalink));
    }
    let mut out = Vec::new();
    let mut off = 16;
    while off < file.len() {
        if off + 24 > file.len() {
            return Err(BtsnoopError::Truncated { offset: off });
        }
        let incl = u32::from_be_bytes(file[off + 4..off + 8].try_into().unwrap()) as usize;
        let flags = u32::from_be_bytes(file[off + 8..off + 12].try_into().unwrap());
        let ts = i64::from_be_bytes(file[off + 16..off + 24].try_into().unwrap());
        let start = off + 24;
        if start + incl > file.len() {
            return Err(BtsnoopError::Truncated { offset: off });
        }
        out.push(Record {
            direction: if flags & 1 == 0 {
                Direction::Sent
            } else {
                Direction::Received
            },
            timestamp_us: ts,
            data: file[start..start + incl].to_vec(),
        });
        off = start + incl;
    }
    Ok(out)
}

/// Streaming btsnoop parser for live sources (stdin, a forwarded snoop socket).
#[derive(Debug, Default)]
pub struct BtsnoopStream {
    buf: Vec<u8>,
    header_done: bool,
}

impl BtsnoopStream {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes; returns completed records. A bad header is an error.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Record>, BtsnoopError> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        if !self.header_done {
            if self.buf.len() < 16 {
                return Ok(out);
            }
            if &self.buf[..8] != b"btsnoop\0" {
                return Err(BtsnoopError::BadMagic);
            }
            let datalink = u32::from_be_bytes(self.buf[12..16].try_into().unwrap());
            if datalink != 1002 {
                return Err(BtsnoopError::UnsupportedDatalink(datalink));
            }
            self.buf.drain(..16);
            self.header_done = true;
        }
        loop {
            if self.buf.len() < 24 {
                break;
            }
            let incl = u32::from_be_bytes(self.buf[4..8].try_into().unwrap()) as usize;
            if self.buf.len() < 24 + incl {
                break;
            }
            let flags = u32::from_be_bytes(self.buf[8..12].try_into().unwrap());
            let ts = i64::from_be_bytes(self.buf[16..24].try_into().unwrap());
            out.push(Record {
                direction: if flags & 1 == 0 {
                    Direction::Sent
                } else {
                    Direction::Received
                },
                timestamp_us: ts,
                data: self.buf[24..24 + incl].to_vec(),
            });
            self.buf.drain(..24 + incl);
        }
        Ok(out)
    }
}

/// A complete L2CAP PDU reassembled from ACL fragments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct L2capPdu {
    pub direction: Direction,
    pub timestamp_us: i64,
    pub conn_handle: u16,
    pub cid: u16,
    pub payload: Vec<u8>,
}

/// Reassembles L2CAP PDUs from H4 ACL packets, per connection and direction.
#[derive(Debug, Default)]
pub struct L2capReassembler {
    partial: HashMap<(u16, Direction), (u16, usize, Vec<u8>)>,
    /// Count of ACL fragments dropped because no start was seen.
    pub orphan_fragments: usize,
}

impl L2capReassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one btsnoop record; returns a PDU when one completes.
    pub fn push(&mut self, rec: &Record) -> Option<L2capPdu> {
        let d = &rec.data;
        if d.first() != Some(&0x02) || d.len() < 5 {
            return None; // not ACL
        }
        let hf = u16::from_le_bytes([d[1], d[2]]);
        let conn = hf & 0x0FFF;
        let pb = (hf >> 12) & 0x3;
        let len = u16::from_le_bytes([d[3], d[4]]) as usize;
        let frag = &d[5..(5 + len).min(d.len())];
        let key = (conn, rec.direction);
        if pb == 0b01 {
            // continuation
            let Some((cid, total, buf)) = self.partial.get_mut(&key) else {
                self.orphan_fragments += 1;
                return None;
            };
            buf.extend_from_slice(frag);
            if buf.len() >= *total {
                let (cid, total, mut buf) = (*cid, *total, std::mem::take(buf));
                self.partial.remove(&key);
                buf.truncate(total);
                return Some(L2capPdu {
                    direction: rec.direction,
                    timestamp_us: rec.timestamp_us,
                    conn_handle: conn,
                    cid,
                    payload: buf,
                });
            }
            return None;
        }
        // start of a PDU
        if frag.len() < 4 {
            return None;
        }
        let total = u16::from_le_bytes([frag[0], frag[1]]) as usize;
        let cid = u16::from_le_bytes([frag[2], frag[3]]);
        let body = frag[4..].to_vec();
        if body.len() >= total {
            self.partial.remove(&key);
            let mut body = body;
            body.truncate(total);
            return Some(L2capPdu {
                direction: rec.direction,
                timestamp_us: rec.timestamp_us,
                conn_handle: conn,
                cid,
                payload: body,
            });
        }
        self.partial.insert(key, (cid, total, body));
        None
    }
}

pub const ATT_CID: u16 = 0x0004;

/// A 16- or 128-bit UUID, stored as 128-bit big-endian.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Uuid(pub [u8; 16]);

impl Uuid {
    const BASE: [u8; 16] = [
        0, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0x80, 0x5F, 0x9B, 0x34, 0xFB,
    ];

    /// From ATT little-endian bytes (2 or 16 long).
    pub fn from_att(le: &[u8]) -> Option<Self> {
        match le.len() {
            2 => {
                let mut u = Self::BASE;
                u[2] = le[1];
                u[3] = le[0];
                Some(Uuid(u))
            }
            16 => {
                let mut u = [0u8; 16];
                for (i, b) in le.iter().rev().enumerate() {
                    u[i] = *b;
                }
                Some(Uuid(u))
            }
            _ => None,
        }
    }

    /// For Garmin UUIDs 6A4Exxxx-667B-11E3-949A-0800200C9A66, the xxxx part.
    pub fn garmin_short(&self) -> Option<u16> {
        const TAIL: [u8; 12] = [
            0x66, 0x7B, 0x11, 0xE3, 0x94, 0x9A, 0x08, 0x00, 0x20, 0x0C, 0x9A, 0x66,
        ];
        (self.0[0] == 0x6A && self.0[1] == 0x4E && self.0[4..] == TAIL)
            .then(|| u16::from_be_bytes([self.0[2], self.0[3]]))
    }
}

impl std::fmt::Display for Uuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let h: String = self.0.iter().map(|b| format!("{b:02x}")).collect();
        write!(
            f,
            "{}-{}-{}-{}-{}",
            &h[0..8],
            &h[8..12],
            &h[12..16],
            &h[16..20],
            &h[20..32]
        )
    }
}

/// The ATT PDUs the decoder cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Att {
    MtuRequest(u16),
    MtuResponse(u16),
    /// Write request (0x12) or write command (0x52).
    Write {
        handle: u16,
        value: Vec<u8>,
        command: bool,
    },
    /// Notification (0x1B) or indication (0x1D).
    Notify {
        handle: u16,
        value: Vec<u8>,
        indication: bool,
    },
    ReadRequest(u16),
    ReadResponse(Vec<u8>),
    /// Characteristic declarations from a Read By Type response.
    Characteristics(Vec<(u16, Uuid)>),
    /// Descriptor/handle info from a Find Information response.
    HandleUuids(Vec<(u16, Uuid)>),
    /// Primary services from a Read By Group Type response: (start, end, uuid).
    Services(Vec<(u16, u16, Uuid)>),
    Other(u8),
}

/// Parse one ATT PDU.
pub fn parse_att(pdu: &[u8]) -> Option<Att> {
    let op = *pdu.first()?;
    let u16at =
        |i: usize| -> Option<u16> { Some(u16::from_le_bytes([*pdu.get(i)?, *pdu.get(i + 1)?])) };
    Some(match op {
        0x02 => Att::MtuRequest(u16at(1)?),
        0x03 => Att::MtuResponse(u16at(1)?),
        0x12 | 0x52 => Att::Write {
            handle: u16at(1)?,
            value: pdu[3..].to_vec(),
            command: op == 0x52,
        },
        0x1B | 0x1D => Att::Notify {
            handle: u16at(1)?,
            value: pdu[3..].to_vec(),
            indication: op == 0x1D,
        },
        0x0A => Att::ReadRequest(u16at(1)?),
        0x0B => Att::ReadResponse(pdu[1..].to_vec()),
        0x09 => {
            let len = *pdu.get(1)? as usize;
            let mut chars = Vec::new();
            if len == 7 || len == 21 {
                for e in pdu[2..].chunks_exact(len) {
                    // attr handle(2), props(1), value handle(2), uuid
                    let vh = u16::from_le_bytes([e[3], e[4]]);
                    if let Some(u) = Uuid::from_att(&e[5..]) {
                        chars.push((vh, u));
                    }
                }
            }
            Att::Characteristics(chars)
        }
        0x05 => {
            let fmt = *pdu.get(1)?;
            let ulen = if fmt == 1 { 2 } else { 16 };
            let items = pdu[2..]
                .chunks_exact(2 + ulen)
                .filter_map(|e| Some((u16::from_le_bytes([e[0], e[1]]), Uuid::from_att(&e[2..])?)))
                .collect();
            Att::HandleUuids(items)
        }
        0x11 => {
            let len = *pdu.get(1)? as usize;
            let mut svcs = Vec::new();
            if len == 6 || len == 20 {
                for e in pdu[2..].chunks_exact(len) {
                    if let Some(u) = Uuid::from_att(&e[4..]) {
                        svcs.push((
                            u16::from_le_bytes([e[0], e[1]]),
                            u16::from_le_bytes([e[2], e[3]]),
                            u,
                        ));
                    }
                }
            }
            Att::Services(svcs)
        }
        other => Att::Other(other),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn btsnoop_file(records: &[(u32, &[u8])]) -> Vec<u8> {
        let mut f = b"btsnoop\0".to_vec();
        f.extend_from_slice(&1u32.to_be_bytes());
        f.extend_from_slice(&1002u32.to_be_bytes());
        for (i, (flags, data)) in records.iter().enumerate() {
            f.extend_from_slice(&(data.len() as u32).to_be_bytes());
            f.extend_from_slice(&(data.len() as u32).to_be_bytes());
            f.extend_from_slice(&flags.to_be_bytes());
            f.extend_from_slice(&0u32.to_be_bytes());
            f.extend_from_slice(&(BTSNOOP_UNIX_OFFSET_US + i as i64).to_be_bytes());
            f.extend_from_slice(data);
        }
        f
    }

    #[test]
    fn parses_btsnoop_records_and_directions() {
        let f = btsnoop_file(&[(0, &[0x02, 1, 2]), (1, &[0x04, 9])]);
        let recs = parse_btsnoop(&f).unwrap();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].direction, Direction::Sent);
        assert_eq!(recs[1].direction, Direction::Received);
        assert_eq!(recs[1].data, vec![0x04, 9]);
    }

    #[test]
    fn stream_parser_matches_file_parser_under_any_chunking() {
        let f = btsnoop_file(&[(0, &[0x02, 1, 2]), (1, &[0x04, 9, 9, 9]), (1, &[0x02])]);
        let whole = parse_btsnoop(&f).unwrap();
        for chunk in [1usize, 3, 7, 1000] {
            let mut s = BtsnoopStream::new();
            let mut got = Vec::new();
            for c in f.chunks(chunk) {
                got.extend(s.push(c).unwrap());
            }
            assert_eq!(got, whole, "chunk {chunk}");
        }
    }

    #[test]
    fn rejects_non_btsnoop() {
        assert_eq!(
            parse_btsnoop(b"hello world 12345"),
            Err(BtsnoopError::BadMagic)
        );
    }

    #[test]
    fn reassembles_fragmented_l2cap() {
        // ATT notify of 6 value bytes split over two ACL fragments.
        let l2cap: Vec<u8> = [&[9u8, 0, 4, 0][..], &[0x1B, 0x20, 0x00, 1, 2, 3, 4, 5, 6]].concat();
        let (a, b) = l2cap.split_at(6);
        let first: Vec<u8> = [
            &[0x02u8, 0x40, 0x20][..],
            &(a.len() as u16).to_le_bytes(),
            a,
        ]
        .concat();
        let cont: Vec<u8> = [
            &[0x02u8, 0x40, 0x10][..],
            &(b.len() as u16).to_le_bytes(),
            b,
        ]
        .concat();
        let mut r = L2capReassembler::new();
        let rec = |d: Vec<u8>| Record {
            direction: Direction::Received,
            timestamp_us: 0,
            data: d,
        };
        assert!(r.push(&rec(first)).is_none());
        let pdu = r.push(&rec(cont)).unwrap();
        assert_eq!(pdu.cid, ATT_CID);
        assert_eq!(pdu.conn_handle, 0x40);
        assert_eq!(
            parse_att(&pdu.payload),
            Some(Att::Notify {
                handle: 0x20,
                value: vec![1, 2, 3, 4, 5, 6],
                indication: false
            })
        );
    }

    #[test]
    fn garmin_uuid_from_att_bytes() {
        // 6A4E2810-667B-11E3-949A-0800200C9A66 in ATT little-endian order.
        let be = [
            0x6A, 0x4E, 0x28, 0x10, 0x66, 0x7B, 0x11, 0xE3, 0x94, 0x9A, 0x08, 0x00, 0x20, 0x0C,
            0x9A, 0x66,
        ];
        let le: Vec<u8> = be.iter().rev().copied().collect();
        let u = Uuid::from_att(&le).unwrap();
        assert_eq!(u.garmin_short(), Some(0x2810));
        assert_eq!(u.to_string(), "6a4e2810-667b-11e3-949a-0800200c9a66");
    }

    #[test]
    fn characteristic_declarations_map_value_handles() {
        let mut pdu = vec![0x09, 21];
        let be = [
            0x6A, 0x4E, 0x28, 0x20, 0x66, 0x7B, 0x11, 0xE3, 0x94, 0x9A, 0x08, 0x00, 0x20, 0x0C,
            0x9A, 0x66,
        ];
        pdu.extend_from_slice(&[0x10, 0x00, 0x0C, 0x11, 0x00]);
        pdu.extend(be.iter().rev());
        assert_eq!(
            parse_att(&pdu),
            Some(Att::Characteristics(vec![(
                0x11,
                Uuid::from_att(&be.iter().rev().copied().collect::<Vec<_>>()).unwrap()
            )]))
        );
    }
}
