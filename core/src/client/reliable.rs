//! One end of a reliable (MLR) stream. Symmetric: the client uses it for the
//! phone side, the test fake watch uses the same type for the watch side.
//!
//! Sans-IO and clock-free: every call takes the caller's monotonic
//! milliseconds and returns the packets to write. Behaviour follows
//! PROTOCOL.md 1.4 (go-back-N, 64 sequence numbers, ACK after 5 packets or
//! 10 ms, 2 s retransmit timer doubling to 20 s). The send window is fixed at
//! 32 packets, our own choice (the app grows it to 63).

use std::collections::VecDeque;

use crate::mlr::{self, Header, Receive, ReceiveWindow};

/// Unacknowledged packets allowed in flight.
pub const WINDOW: usize = 32;
/// ACK-only packet after this many received data packets.
pub const ACK_AFTER_PACKETS: u32 = 5;
/// ... or this long after the first unacknowledged one.
pub const ACK_DELAY_MS: u64 = 10;
pub const RTO_INITIAL_MS: u64 = 2_000;
pub const RTO_MAX_MS: u64 = 20_000;

/// Smallest ATT MTU the client accepts (default LE MTU).
pub const MIN_ATT_MTU: u16 = 23;

#[derive(Debug, Clone)]
struct Packet {
    seq: u8,
    data: Vec<u8>,
}

#[derive(Debug)]
pub struct MlrEndpoint {
    /// 3-bit MLR handle (register response handle byte & 7).
    handle: u8,
    /// Data bytes per packet: ATT MTU - 3 (ATT header) - 2 (MLR header).
    payload_max: usize,
    next_seq: u8,
    /// Packets sent and not yet acknowledged, oldest first.
    unacked: VecDeque<Packet>,
    /// Packets waiting for window space.
    queued: VecDeque<Vec<u8>>,
    rx: ReceiveWindow,
    /// Data packets received since we last sent an ack.
    rx_unacked: u32,
    ack_at: Option<u64>,
    rto_ms: u64,
    retransmit_at: Option<u64>,
}

/// What one received packet produced.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Received {
    /// Stream bytes delivered in order (empty for ACK-only, duplicates, gaps).
    pub data: Vec<u8>,
    /// Packets to write, in order.
    pub writes: Vec<Vec<u8>>,
}

impl MlrEndpoint {
    pub fn new(handle: u8, att_mtu: u16) -> Self {
        Self {
            handle: handle & 7,
            payload_max: (att_mtu as usize).saturating_sub(5).max(1),
            next_seq: 0,
            unacked: VecDeque::new(),
            queued: VecDeque::new(),
            rx: ReceiveWindow::new(),
            rx_unacked: 0,
            ack_at: None,
            rto_ms: RTO_INITIAL_MS,
            retransmit_at: None,
        }
    }

    pub fn handle(&self) -> u8 {
        self.handle
    }

    pub fn payload_max(&self) -> usize {
        self.payload_max
    }

    /// Packets sent and not yet acknowledged.
    pub fn in_flight(&self) -> usize {
        self.unacked.len()
    }

    /// Packets waiting for window space.
    pub fn queued(&self) -> usize {
        self.queued.len()
    }

    /// Next sequence number we expect from the peer.
    pub fn next_expected(&self) -> u8 {
        self.rx.next_expected()
    }

    /// Queue one unit (a COBS-framed GFDI message) for sending. It starts a
    /// new packet; it is split into `payload_max` pieces. Returns the
    /// packets that fit in the window now.
    pub fn send(&mut self, now_ms: u64, bytes: &[u8]) -> Vec<Vec<u8>> {
        for piece in bytes.chunks(self.payload_max) {
            self.queued.push_back(piece.to_vec());
        }
        self.fill_window(now_ms)
    }

    /// Handle one received MLR packet (header already known to carry our
    /// handle).
    pub fn receive(&mut self, now_ms: u64, header: Header, data: &[u8]) -> Received {
        let mut out = Received::default();
        let progress = self.process_ack(now_ms, header.ack);
        if data.is_empty()
            && !progress
            && !self.unacked.is_empty()
            && self.is_duplicate_ack(header.ack)
        {
            // A duplicate ACK-only packet restarts the timer. [PROTOCOL.md 1.4]
            self.retransmit_at = Some(now_ms.saturating_add(self.rto_ms));
        }
        if !data.is_empty() {
            if self.rx.offer(header.seq) == Receive::Accept {
                out.data = data.to_vec();
            }
            self.rx_unacked += 1;
            if self.rx_unacked == 1 {
                self.ack_at = Some(now_ms.saturating_add(ACK_DELAY_MS));
            }
        }
        out.writes = self.fill_window(now_ms);
        if self.rx_unacked >= ACK_AFTER_PACKETS {
            out.writes.push(self.ack_only());
        }
        out
    }

    /// Run due timers: delayed ack, retransmission.
    pub fn poll(&mut self, now_ms: u64) -> Vec<Vec<u8>> {
        let mut writes = Vec::new();
        if self.retransmit_at.is_some_and(|t| now_ms >= t) {
            self.rto_ms = (self.rto_ms * 2).min(RTO_MAX_MS);
            self.retransmit_at = Some(now_ms.saturating_add(self.rto_ms));
            let ack = self.rx.next_expected();
            for p in &self.unacked {
                writes.push(mlr::build(
                    Header {
                        handle: self.handle,
                        ack,
                        seq: p.seq,
                    },
                    &p.data,
                ));
            }
            if !writes.is_empty() {
                self.rx_unacked = 0;
                self.ack_at = None;
            }
        }
        if self.ack_at.is_some_and(|t| now_ms >= t) && self.rx_unacked > 0 {
            writes.push(self.ack_only());
        }
        writes
    }

    /// Earliest time `poll` has something to do.
    pub fn next_deadline(&self) -> Option<u64> {
        match (self.ack_at, self.retransmit_at) {
            (Some(a), Some(r)) => Some(a.min(r)),
            (a, r) => a.or(r),
        }
    }

    fn ack_only(&mut self) -> Vec<u8> {
        self.rx_unacked = 0;
        self.ack_at = None;
        mlr::build(
            Header {
                handle: self.handle,
                ack: self.rx.next_expected(),
                seq: 0,
            },
            &[],
        )
    }

    fn fill_window(&mut self, now_ms: u64) -> Vec<Vec<u8>> {
        let mut writes = Vec::new();
        while self.unacked.len() < WINDOW {
            let Some(data) = self.queued.pop_front() else {
                break;
            };
            let seq = self.next_seq;
            self.next_seq = (self.next_seq + 1) & 0x3F;
            writes.push(mlr::build(
                Header {
                    handle: self.handle,
                    ack: self.rx.next_expected(),
                    seq,
                },
                &data,
            ));
            if self.unacked.is_empty() {
                self.retransmit_at = Some(now_ms.saturating_add(self.rto_ms));
            }
            self.unacked.push_back(Packet { seq, data });
            // The data packet carries the ack.
            self.rx_unacked = 0;
            self.ack_at = None;
        }
        writes
    }

    fn is_duplicate_ack(&self, ack: u8) -> bool {
        self.unacked.front().is_some_and(|p| ack & 0x3F == p.seq)
    }

    /// Returns true when the ack moved the window.
    fn process_ack(&mut self, now_ms: u64, ack: u8) -> bool {
        let Some(oldest) = self.unacked.front().map(|p| p.seq) else {
            return false;
        };
        let acked = (ack.wrapping_sub(oldest) & 0x3F) as usize;
        if acked == 0 || acked > self.unacked.len() {
            return false;
        }
        self.unacked.drain(..acked);
        self.rto_ms = RTO_INITIAL_MS;
        self.retransmit_at = if self.unacked.is_empty() {
            None
        } else {
            Some(now_ms.saturating_add(self.rto_ms))
        };
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: u8 = 3;

    fn pair(mtu: u16) -> (MlrEndpoint, MlrEndpoint) {
        (MlrEndpoint::new(H, mtu), MlrEndpoint::new(H, mtu))
    }

    fn feed(ep: &mut MlrEndpoint, now: u64, packets: &[Vec<u8>]) -> Received {
        let mut all = Received::default();
        for p in packets {
            let (h, d) = mlr::parse(p).unwrap();
            let r = ep.receive(now, h, d);
            all.data.extend(r.data);
            all.writes.extend(r.writes);
        }
        all
    }

    #[test]
    fn payload_is_mtu_minus_five_and_each_unit_starts_a_packet() {
        let mut a = MlrEndpoint::new(H, 23);
        assert_eq!(a.payload_max(), 18);
        let p = a.send(0, &[1u8; 20]);
        assert_eq!(p.len(), 2);
        assert_eq!(mlr::parse(&p[0]).unwrap().1.len(), 18);
        assert_eq!(mlr::parse(&p[1]).unwrap().1.len(), 2);
        let q = a.send(0, &[9]);
        assert_eq!(mlr::parse(&q[0]).unwrap().0.seq, 2);
    }

    #[test]
    fn stream_roundtrips_and_wraps_the_sequence_space() {
        let (mut a, mut b) = pair(23);
        let mut got = Vec::new();
        for n in 0..200u32 {
            let unit: Vec<u8> = (0..30).map(|i| (n as u8).wrapping_add(i)).collect();
            let pk = a.send(n as u64, &unit);
            let r = feed(&mut b, n as u64, &pk);
            got.extend(r.data);
            // peer acks flow back
            feed(&mut a, n as u64, &r.writes);
            let tail = b.poll(n as u64 + 10);
            feed(&mut a, n as u64 + 10, &tail);
        }
        assert_eq!(got.len(), 200 * 30);
        assert_eq!(a.in_flight(), 0);
    }

    #[test]
    fn window_holds_back_packets_until_acked() {
        let (mut a, mut b) = pair(23);
        let first = a.send(0, &vec![7u8; 18 * 40]);
        assert_eq!(first.len(), WINDOW);
        assert_eq!(a.queued(), 8);
        // b receives all 32, acks after every 5th.
        let r = feed(&mut b, 1, &first);
        assert_eq!(r.data.len(), 18 * 32);
        let more = feed(&mut a, 2, &r.writes);
        assert_eq!(more.writes.len(), 8);
        assert_eq!(a.queued(), 0);
    }

    #[test]
    fn ack_only_after_five_packets_and_after_ten_ms() {
        let (mut a, mut b) = pair(23);
        let five = a.send(0, &[0u8; 18 * 5]);
        let r = feed(&mut b, 100, &five);
        let acks: Vec<_> = r.writes.iter().map(|w| w.len()).collect();
        assert_eq!(acks, vec![2], "one ack-only packet after the fifth");
        assert_eq!(mlr::parse(&r.writes[0]).unwrap().0.ack, 5);

        let one = a.send(200, &[1u8; 4]);
        let r = feed(&mut b, 200, &one);
        assert!(r.writes.is_empty());
        assert_eq!(b.next_deadline(), Some(210));
        assert!(b.poll(209).is_empty());
        let w = b.poll(210);
        assert_eq!(w.len(), 1);
        assert_eq!(mlr::parse(&w[0]).unwrap().0.ack, 6);
        assert_eq!(b.next_deadline(), None);
    }

    #[test]
    fn outgoing_data_carries_the_ack_and_cancels_the_delay() {
        let (mut a, mut b) = pair(23);
        let p = a.send(0, &[1, 2, 3]);
        feed(&mut b, 0, &p);
        assert!(b.next_deadline().is_some());
        let reply = b.send(5, &[4]);
        assert_eq!(mlr::parse(&reply[0]).unwrap().0.ack, 1);
        assert!(
            b.poll(100).iter().all(|w| w.len() > 2),
            "no ack-only after data"
        );
    }

    #[test]
    fn lost_packet_is_retransmitted_at_2_4_8_16_20_20_seconds() {
        let mut a = MlrEndpoint::new(H, 23);
        a.send(1_000, &[1, 2, 3]);
        assert_eq!(a.next_deadline(), Some(3_000));
        assert!(a.poll(2_999).is_empty());
        let mut now = 3_000;
        let mut gaps = Vec::new();
        for _ in 0..6 {
            assert_eq!(a.poll(now).len(), 1);
            let next = a.next_deadline().unwrap();
            gaps.push(next - now);
            now = next;
        }
        assert_eq!(gaps, vec![4_000, 8_000, 16_000, 20_000, 20_000, 20_000]);
    }

    #[test]
    fn retransmission_resends_everything_with_the_current_ack() {
        let (mut a, mut b) = pair(23);
        // b has received one packet from a, so its ack is 1.
        let p = a.send(0, &[1]);
        feed(&mut b, 0, &p);
        b.poll(10);
        b.send(20, &[1u8; 18]);
        b.send(20, &[2u8; 18]);
        let w = b.poll(2_020);
        assert_eq!(w.len(), 2);
        for p in &w {
            assert_eq!(mlr::parse(p).unwrap().0.ack, 1);
        }
        let seqs: Vec<u8> = w.iter().map(|p| mlr::parse(p).unwrap().0.seq).collect();
        assert_eq!(seqs, vec![0, 1]);
    }

    #[test]
    fn ack_progress_resets_the_timer_to_two_seconds() {
        let (mut a, mut b) = pair(23);
        let p = a.send(0, &[1u8; 18 * 2]);
        a.poll(2_000);
        assert_eq!(a.next_deadline(), Some(6_000));
        let r = feed(&mut b, 2_100, &p[..1]);
        let _ = r;
        let ack = mlr::build(
            Header {
                handle: H,
                ack: 1,
                seq: 0,
            },
            &[],
        );
        feed(&mut a, 2_200, &[ack]);
        assert_eq!(a.in_flight(), 1);
        assert_eq!(a.next_deadline(), Some(4_200));
    }

    #[test]
    fn gap_delivers_nothing_until_retransmission() {
        let (mut a, mut b) = pair(23);
        let pk = a.send(0, &[5u8; 18 * 3]);
        // packet 0 lost
        let r = feed(&mut b, 1, &pk[1..]);
        assert!(r.data.is_empty());
        assert_eq!(b.next_expected(), 0);
        // go-back-N: retransmission delivers all three in order
        let again = a.poll(2_000);
        assert_eq!(again.len(), 3);
        let r = feed(&mut b, 2_001, &again);
        assert_eq!(r.data.len(), 54);
    }

    #[test]
    fn duplicates_are_dropped_but_still_acked() {
        let (mut a, mut b) = pair(23);
        let p = a.send(0, &[1, 2, 3]);
        feed(&mut b, 0, &p);
        b.poll(10);
        let r = feed(&mut b, 20, &p);
        assert!(r.data.is_empty());
        assert_eq!(b.next_deadline(), Some(30));
    }

    #[test]
    fn duplicate_ack_only_restarts_the_retransmit_timer() {
        let mut a = MlrEndpoint::new(H, 23);
        a.send(0, &[1]);
        assert_eq!(a.next_deadline(), Some(2_000));
        let dup = Header {
            handle: H,
            ack: 0,
            seq: 0,
        };
        a.receive(1_500, dup, &[]);
        assert_eq!(a.next_deadline(), Some(3_500));
        // An ack beyond what was sent is not a duplicate: ignored.
        a.receive(1_600, Header { ack: 40, ..dup }, &[]);
        assert_eq!(a.next_deadline(), Some(3_500));
    }

    #[test]
    fn invalid_ack_values_are_ignored() {
        let mut a = MlrEndpoint::new(H, 23);
        a.send(0, &[1]);
        let bogus = Header {
            handle: H,
            ack: 40,
            seq: 0,
        };
        a.receive(5, bogus, &[]);
        assert_eq!(a.in_flight(), 1);
    }
}
