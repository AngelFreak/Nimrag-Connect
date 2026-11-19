//! Multi-Link Reliable (MLR) packet header.
//!
//! When a service is registered in reliable mode (register response flags
//! bit 0), every packet on its handle starts with a 2-byte big-endian header
//! instead of the 1-byte handle:
//!
//! ```text
//! bit 15      : 1 (marks an MLR packet; plain handles are < 0x80)
//! bits 14..12 : handle (3 bits; register response handle byte = 0x80 | handle)
//! bits 11..6  : ack  = next sequence number expected from the peer (mod 64)
//! bits  5..0  : seq  = this packet's sequence number (mod 64)
//! ```
//!
//! A 2-byte packet is a pure acknowledgement and carries no data.
//! Observed in capture 2026-11-19 (see docs/protocol/PROTOCOL.md); the bit
//! layout is cross-checked against the native library analysis.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub handle: u8,
    pub ack: u8,
    pub seq: u8,
}

/// Split an MLR packet into header and data. None if not an MLR packet.
pub fn parse(value: &[u8]) -> Option<(Header, &[u8])> {
    if value.len() < 2 || value[0] & 0x80 == 0 {
        return None;
    }
    let w = u16::from_be_bytes([value[0], value[1]]);
    Some((
        Header {
            handle: ((w >> 12) & 0x7) as u8,
            ack: ((w >> 6) & 0x3F) as u8,
            seq: (w & 0x3F) as u8,
        },
        &value[2..],
    ))
}

/// Build an MLR packet.
pub fn build(h: Header, data: &[u8]) -> Vec<u8> {
    let w: u16 = 0x8000
        | ((h.handle as u16 & 7) << 12)
        | ((h.ack as u16 & 0x3F) << 6)
        | (h.seq as u16 & 0x3F);
    let mut v = w.to_be_bytes().to_vec();
    v.extend_from_slice(data);
    v
}

/// Outcome of offering one data packet to a receive window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receive {
    /// In order: deliver the data.
    Accept,
    /// Already delivered (a retransmission): drop.
    Duplicate,
    /// Sequence number ahead of the next expected one: data was lost.
    Gap { expected: u8 },
}

/// Per-direction in-order receive tracker (sequence numbers mod 64).
#[derive(Debug, Clone, Default)]
pub struct ReceiveWindow {
    next: u8,
}

impl ReceiveWindow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_expected(&self) -> u8 {
        self.next
    }

    /// Accept `seq` as delivered after a gap; the next expected becomes seq + 1.
    pub fn resync_after(&mut self, seq: u8) {
        self.next = (seq + 1) & 0x3F;
    }

    pub fn offer(&mut self, seq: u8) -> Receive {
        let seq = seq & 0x3F;
        if seq == self.next {
            self.next = (self.next + 1) & 0x3F;
            return Receive::Accept;
        }
        // Distance behind the next expected number, mod 64. Treat the half
        // window behind as retransmissions, the rest as a gap.
        let behind = (self.next + 64 - seq) & 0x3F;
        if behind <= 32 {
            Receive::Duplicate
        } else {
            Receive::Gap {
                expected: self.next,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_observed_headers() {
        // From capture: watch data packets f0 00.., phone ack-only f1 00.
        assert_eq!(
            parse(&[0xF0, 0x03, 0xAA]).unwrap(),
            (
                Header {
                    handle: 7,
                    ack: 0,
                    seq: 3
                },
                &[0xAA][..]
            )
        );
        assert_eq!(
            parse(&[0xF1, 0x00]).unwrap().0,
            Header {
                handle: 7,
                ack: 4,
                seq: 0
            }
        );
        assert_eq!(
            parse(&[0xA1, 0x00]).unwrap().0,
            Header {
                handle: 2,
                ack: 4,
                seq: 0
            }
        );
        assert!(parse(&[0x2D, 0x00]).is_none());
    }

    #[test]
    fn build_parse_roundtrip() {
        for handle in 0..8 {
            for ack in [0u8, 1, 33, 63] {
                for seq in [0u8, 5, 63] {
                    let h = Header { handle, ack, seq };
                    let p = build(h, &[1, 2]);
                    assert_eq!(parse(&p).unwrap(), (h, &[1u8, 2][..]));
                }
            }
        }
    }

    #[test]
    fn window_accepts_in_order_drops_duplicates_reports_gaps_and_wraps() {
        let mut w = ReceiveWindow::new();
        for s in 0..64u8 {
            assert_eq!(w.offer(s), Receive::Accept);
        }
        assert_eq!(w.next_expected(), 0);
        assert_eq!(w.offer(63), Receive::Duplicate);
        assert_eq!(w.offer(0), Receive::Accept);
        assert_eq!(w.offer(5), Receive::Gap { expected: 1 });
    }
}
