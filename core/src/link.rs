//! Sans-IO decoder for a captured watch link: ATT values in, protocol events out.
//!
//! Pipeline per ATT value: multi-link control (handle 0) | MLR packet (bit 7)
//! | plain handle -> per-service byte stream -> COBS frames -> GFDI messages.
//! Downloads (5002 / 5004) are reassembled into whole files; file index 0 is
//! parsed as the directory.

use std::collections::HashMap;

use crate::capture::{Att, Direction};
use crate::cobs::FrameDecoder;
use crate::describe::DirEntry;
use crate::gfdi::{Message, Status};
use crate::mlr::{self, Receive, ReceiveWindow};
use crate::multilink::{self, Control};

/// A decoded protocol event.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    MtuExchange {
        direction: Direction,
        mtu: u16,
    },
    Control {
        direction: Direction,
        client: Option<u64>,
        control: Control,
    },
    InfoPage {
        direction: Direction,
        client: u64,
        page: InfoPage,
    },
    /// Bytes for a registered non-GFDI service (e.g. keep-alive).
    ServiceData {
        direction: Direction,
        client: u64,
        service: u16,
        data: Vec<u8>,
    },
    Gfdi {
        direction: Direction,
        client: u64,
        message: Message,
    },
    GfdiError {
        direction: Direction,
        client: u64,
        error: String,
        frame: Vec<u8>,
    },
    MlrDuplicate {
        direction: Direction,
        client: u64,
        handle: u8,
        seq: u8,
    },
    MlrGap {
        direction: Direction,
        client: u64,
        handle: u8,
        seq: u8,
        expected: u8,
    },
    FileDownloaded {
        client: u64,
        index: u16,
        entry: Option<DirEntry>,
        data: Vec<u8>,
        crc_ok: bool,
    },
    Directory {
        client: u64,
        entries: Vec<DirEntry>,
    },
    /// Client characteristic configuration write (enable notify/indicate) on
    /// a characteristic not carrying multi-link traffic.
    DescriptorWrite {
        att_handle: u16,
        notify: bool,
        indicate: bool,
    },
    /// A value the decoder could not attribute to any known stream.
    Unknown {
        direction: Direction,
        att_handle: u16,
        value: Vec<u8>,
        reason: &'static str,
    },
}

/// A multi-link info page (identifiers are deliberately not exposed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InfoPage {
    Selector(Vec<u8>),
    SupportedServices(Vec<usize>),
    AdvertisingData(usize),
    Version {
        major: u8,
        minor: u8,
        micro: u8,
    },
    ProductInfo {
        product: u16,
        firmware: u16,
    },
    IdentityAddress,
    ServiceRevision {
        service: u16,
        status: u8,
        revision: Option<u8>,
    },
    Unsupported(u8),
    Raw(Vec<u8>),
}

#[derive(Debug, Default)]
struct Download {
    index: u16,
    size: Option<u32>,
    data: Vec<u8>,
    crc: u16,
    crc_ok: bool,
}

#[derive(Debug, Default)]
struct ClientState {
    /// plain handle -> service id
    plain: HashMap<u8, u16>,
    /// MLR handle (3 bits) -> service id
    reliable: HashMap<u8, u16>,
    windows: HashMap<(u8, Direction), ReceiveWindow>,
    cobs: HashMap<(u16, Direction), FrameDecoder>,
    download: Option<Download>,
    directory: HashMap<u16, DirEntry>,
}

/// The decoder. Feed it `(direction, Att)` pairs in capture order.
#[derive(Debug, Default)]
pub struct LinkDecoder {
    att_client: HashMap<u16, u64>,
    clients: HashMap<u64, ClientState>,
}

impl LinkDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, direction: Direction, att: &Att) -> Vec<Event> {
        match att {
            Att::MtuRequest(m) | Att::MtuResponse(m) => {
                vec![Event::MtuExchange { direction, mtu: *m }]
            }
            Att::Write { handle, value, .. } | Att::Notify { handle, value, .. } => {
                self.value(direction, *handle, value)
            }
            _ => Vec::new(),
        }
    }

    fn value(&mut self, direction: Direction, att_handle: u16, value: &[u8]) -> Vec<Event> {
        if value.is_empty() {
            return Vec::new();
        }
        if value[0] == 0 {
            return self.control(direction, att_handle, value);
        }
        let Some(&client) = self.att_client.get(&att_handle) else {
            if direction == Direction::Sent && value.len() == 2 && value[1] == 0 && value[0] <= 3 {
                return vec![Event::DescriptorWrite {
                    att_handle,
                    notify: value[0] & 1 != 0,
                    indicate: value[0] & 2 != 0,
                }];
            }
            return vec![Event::Unknown {
                direction,
                att_handle,
                value: value.to_vec(),
                reason: "no multi-link client seen on this characteristic",
            }];
        };
        let state = self.clients.entry(client).or_default();
        if let Some((h, data)) = mlr::parse(value) {
            let Some(&service) = state.reliable.get(&h.handle) else {
                return vec![Event::Unknown {
                    direction,
                    att_handle,
                    value: value.to_vec(),
                    reason: "MLR packet on unregistered handle",
                }];
            };
            if data.is_empty() {
                return Vec::new(); // pure acknowledgement
            }
            match state
                .windows
                .entry((h.handle, direction))
                .or_default()
                .offer(h.seq)
            {
                Receive::Accept => self.service_bytes(direction, client, service, data),
                Receive::Duplicate => vec![Event::MlrDuplicate {
                    direction,
                    client,
                    handle: h.handle,
                    seq: h.seq,
                }],
                Receive::Gap { expected } => {
                    // Resynchronise on the packet we have and report the loss.
                    let st = self.clients.get_mut(&client).unwrap();
                    st.windows
                        .get_mut(&(h.handle, direction))
                        .unwrap()
                        .resync_after(h.seq);
                    let mut ev = vec![Event::MlrGap {
                        direction,
                        client,
                        handle: h.handle,
                        seq: h.seq,
                        expected,
                    }];
                    ev.extend(self.service_bytes(direction, client, service, data));
                    ev
                }
            }
        } else {
            let Some(&service) = state.plain.get(&value[0]) else {
                return vec![Event::Unknown {
                    direction,
                    att_handle,
                    value: value.to_vec(),
                    reason: "plain handle not registered",
                }];
            };
            self.service_bytes(direction, client, service, &value[1..])
        }
    }

    fn control(&mut self, direction: Direction, att_handle: u16, value: &[u8]) -> Vec<Event> {
        let Some(control) = multilink::parse_control(value) else {
            return vec![Event::Unknown {
                direction,
                att_handle,
                value: value.to_vec(),
                reason: "unparseable control frame",
            }];
        };
        let client = match &control {
            Control::RegisterRequest { client_id, .. }
            | Control::RegisterResponse { client_id, .. }
            | Control::CloseHandleRequest { client_id, .. }
            | Control::CloseHandleResponse { client_id, .. }
            | Control::InvalidHandle { client_id, .. }
            | Control::CloseAllRequest { client_id }
            | Control::CloseAllResponse { client_id, .. }
            | Control::UnknownCommandResponse { client_id, .. } => Some(*client_id),
            Control::Unparsed { .. } => None,
        };
        if let Some(c) = client {
            self.att_client.insert(att_handle, c);
            let st = self.clients.entry(c).or_default();
            match &control {
                Control::RegisterResponse {
                    service,
                    status: 0,
                    handle: Some(h),
                    flags,
                    ..
                } => {
                    if flags.unwrap_or(0) & 1 != 0 {
                        let mh = h & 0x7;
                        st.reliable.insert(mh, *service);
                        st.windows.retain(|(hh, _), _| *hh != mh);
                    } else {
                        st.plain.insert(*h, *service);
                    }
                    st.cobs.remove(&(*service, Direction::Sent));
                    st.cobs.remove(&(*service, Direction::Received));
                }
                Control::CloseHandleResponse { handle, .. }
                | Control::InvalidHandle { handle, .. } => {
                    st.plain.remove(handle);
                    st.reliable.remove(&(handle & 0x7));
                }
                Control::CloseAllResponse { .. } => {
                    st.plain.clear();
                    st.reliable.clear();
                    st.windows.clear();
                    st.cobs.clear();
                }
                _ => {}
            }
        }
        vec![Event::Control {
            direction,
            client,
            control,
        }]
    }

    fn service_bytes(
        &mut self,
        direction: Direction,
        client: u64,
        service: u16,
        data: &[u8],
    ) -> Vec<Event> {
        match service {
            4 => vec![Event::InfoPage {
                direction,
                client,
                page: parse_info_page(direction, data),
            }],
            1 => {
                let frames = self
                    .clients
                    .get_mut(&client)
                    .unwrap()
                    .cobs
                    .entry((service, direction))
                    .or_default()
                    .push(data);
                let mut out = Vec::new();
                for f in frames {
                    match f {
                        Ok(frame) => match Message::parse(&frame) {
                            Ok(message) => {
                                let file_events = self.track_files(direction, client, &message);
                                out.push(Event::Gfdi {
                                    direction,
                                    client,
                                    message,
                                });
                                out.extend(file_events);
                            }
                            Err(e) => out.push(Event::GfdiError {
                                direction,
                                client,
                                error: format!("{e:?}"),
                                frame,
                            }),
                        },
                        Err(e) => out.push(Event::GfdiError {
                            direction,
                            client,
                            error: format!("{e:?}"),
                            frame: Vec::new(),
                        }),
                    }
                }
                out
            }
            _ => vec![Event::ServiceData {
                direction,
                client,
                service,
                data: data.to_vec(),
            }],
        }
    }

    fn track_files(&mut self, direction: Direction, client: u64, m: &Message) -> Vec<Event> {
        let st = self.clients.get_mut(&client).unwrap();
        match (direction, m.id) {
            (Direction::Sent, 5002) if m.payload.len() >= 2 => {
                let index = u16::from_le_bytes([m.payload[0], m.payload[1]]);
                st.download = Some(Download {
                    index,
                    crc_ok: true,
                    ..Default::default()
                });
                Vec::new()
            }
            (Direction::Received, 5000) => {
                if let (Some(r), Some(d)) = (m.as_response(), st.download.as_mut()) {
                    if r.request_id == 5002 {
                        if r.status == Status::Ack
                            && r.body.first() == Some(&0)
                            && r.body.len() >= 5
                        {
                            d.size = Some(u32::from_le_bytes(r.body[1..5].try_into().unwrap()));
                        } else {
                            st.download = None;
                        }
                    }
                }
                Vec::new()
            }
            (Direction::Received, 5004) if m.payload.len() >= 7 => {
                let Some(d) = st.download.as_mut() else {
                    return Vec::new();
                };
                let crc = u16::from_le_bytes([m.payload[1], m.payload[2]]);
                let offset = u32::from_le_bytes(m.payload[3..7].try_into().unwrap()) as usize;
                let chunk = &m.payload[7..];
                if offset != d.data.len() {
                    return Vec::new(); // duplicate / resend; the phone will answer with a status
                }
                d.crc = crate::crc::update(d.crc, chunk);
                d.crc_ok &= d.crc == crc;
                d.data.extend_from_slice(chunk);
                let done = d.size.is_some_and(|s| d.data.len() >= s as usize);
                if !done {
                    return Vec::new();
                }
                let d = st.download.take().unwrap();
                let entry = st.directory.get(&d.index).cloned();
                let mut ev = Vec::new();
                if d.index == 0 {
                    let entries: Vec<DirEntry> = d
                        .data
                        .chunks_exact(16)
                        .skip(1)
                        .filter_map(DirEntry::parse)
                        .collect();
                    for e in &entries {
                        st.directory.insert(e.index, e.clone());
                    }
                    ev.push(Event::Directory { client, entries });
                }
                ev.push(Event::FileDownloaded {
                    client,
                    index: d.index,
                    entry,
                    data: d.data,
                    crc_ok: d.crc_ok,
                });
                ev
            }
            _ => Vec::new(),
        }
    }
}

fn parse_info_page(direction: Direction, d: &[u8]) -> InfoPage {
    if direction == Direction::Sent {
        return InfoPage::Selector(d.to_vec());
    }
    let Some(&page) = d.first() else {
        return InfoPage::Raw(Vec::new());
    };
    let u16at = |i: usize| d.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]]));
    match page {
        0xFF => InfoPage::Unsupported(d.get(1).copied().unwrap_or(0)),
        0 => {
            let mut v = Vec::new();
            for (k, b) in d[1..].iter().enumerate() {
                for i in 0..8 {
                    if b & (1 << i) != 0 {
                        v.push(k * 8 + i);
                    }
                }
            }
            InfoPage::SupportedServices(v)
        }
        1 => InfoPage::AdvertisingData(d.len() - 1),
        2 if d.len() >= 4 => InfoPage::Version {
            micro: d[1],
            minor: d[2],
            major: d[3],
        },
        3 if d.len() >= 5 => InfoPage::ProductInfo {
            product: u16at(1).unwrap(),
            firmware: u16at(3).unwrap(),
        },
        4 => InfoPage::IdentityAddress,
        5 if d.len() >= 4 => InfoPage::ServiceRevision {
            service: u16at(1).unwrap(),
            status: d[3],
            revision: d.get(4).copied(),
        },
        _ => InfoPage::Raw(d.to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cobs::encode_frame;

    const W: u16 = 0x1B; // phone write char
    const N: u16 = 0x18; // watch notify char

    fn write(v: Vec<u8>) -> (Direction, Att) {
        (
            Direction::Sent,
            Att::Write {
                handle: W,
                value: v,
                command: true,
            },
        )
    }
    fn notify(v: Vec<u8>) -> (Direction, Att) {
        (
            Direction::Received,
            Att::Notify {
                handle: N,
                value: v,
                indication: false,
            },
        )
    }

    fn register_resp(service: u16, handle: u8, flags: u8) -> Vec<u8> {
        let mut v = vec![0, 1];
        v.extend_from_slice(&1u64.to_le_bytes());
        v.extend_from_slice(&service.to_le_bytes());
        v.extend_from_slice(&[0, handle, flags, 0]);
        v
    }

    fn run(dec: &mut LinkDecoder, items: Vec<(Direction, Att)>) -> Vec<Event> {
        items.iter().flat_map(|(d, a)| dec.push(*d, a)).collect()
    }

    fn gfdi_msgs(events: &[Event]) -> Vec<(Direction, u16)> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Gfdi {
                    direction, message, ..
                } => Some((*direction, message.id)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn reliable_gfdi_over_20_byte_packets_with_retransmission() {
        let mut dec = LinkDecoder::new();
        let msg = Message::new(5024, (0..60).collect()).encode();
        let wire = encode_frame(&msg);
        let mut items = vec![
            write(multilink::register_request(1, 1, 2)),
            notify(register_resp(1, 0x87, 1)),
        ];
        // 18 data bytes per 20-byte packet (2-byte MLR header).
        for (seq, chunk) in wire.chunks(18).enumerate() {
            items.push(notify(mlr::build(
                mlr::Header {
                    handle: 7,
                    ack: 0,
                    seq: seq as u8,
                },
                chunk,
            )));
            if seq == 1 {
                // retransmission of packet 1 must not corrupt the stream
                items.push(notify(mlr::build(
                    mlr::Header {
                        handle: 7,
                        ack: 0,
                        seq: 1,
                    },
                    chunk,
                )));
            }
        }
        items.push(write(vec![0xF1, 0x00])); // ack-only
        let ev = run(&mut dec, items);
        assert_eq!(gfdi_msgs(&ev), vec![(Direction::Received, 5024)]);
        assert!(ev
            .iter()
            .any(|e| matches!(e, Event::MlrDuplicate { seq: 1, .. })));
        assert!(!ev
            .iter()
            .any(|e| matches!(e, Event::Unknown { .. } | Event::GfdiError { .. })));
    }

    #[test]
    fn plain_registration_handle_carries_info_pages() {
        let mut dec = LinkDecoder::new();
        let ev = run(
            &mut dec,
            vec![
                write(multilink::register_request(1, 4, 0)),
                notify(register_resp(4, 0x2E, 0)),
                write(vec![0x2E, 0x03]),
                notify(vec![0x2E, 0x03, 0x30, 0x0F, 0xBA, 0x06, 1, 2, 3, 4]),
            ],
        );
        assert!(ev.contains(&Event::InfoPage {
            direction: Direction::Received,
            client: 1,
            page: InfoPage::ProductInfo {
                product: 3888,
                firmware: 1722
            }
        }));
    }

    #[test]
    fn download_of_directory_is_reassembled_and_crc_checked() {
        let mut dec = LinkDecoder::new();
        let mut dir = vec![0u8; 16]; // header
        let mut e = vec![7, 0, 128, 4, 1, 0, 0, 0];
        e.extend_from_slice(&500u32.to_le_bytes());
        e.extend_from_slice(&1000u32.to_le_bytes());
        dir.extend_from_slice(&e);
        let mut items = vec![
            write(multilink::register_request(1, 1, 0)),
            notify(register_resp(1, 0x30, 0)),
        ];
        let send = |dir: Direction, m: Message| -> (Direction, Att) {
            let mut v = vec![0x30];
            v.extend(encode_frame(&m.encode()));
            match dir {
                Direction::Sent => write(v),
                Direction::Received => notify(v),
            }
        };
        let req = [
            &0u16.to_le_bytes()[..],
            &0u32.to_le_bytes(),
            &[1],
            &0u16.to_le_bytes(),
            &0u32.to_le_bytes(),
            &[0],
        ]
        .concat();
        items.push(send(Direction::Sent, Message::new(5002, req)));
        let mut resp = vec![0x8A, 0x13, 0x00, 0x00];
        resp.extend_from_slice(&(dir.len() as u32).to_le_bytes());
        items.push(send(Direction::Received, Message::new(5000, resp)));
        let mut crc = 0;
        for (i, chunk) in dir.chunks(10).enumerate() {
            crc = crate::crc::update(crc, chunk);
            let mut p = vec![0];
            p.extend_from_slice(&crc.to_le_bytes());
            p.extend_from_slice(&((i * 10) as u32).to_le_bytes());
            p.extend_from_slice(chunk);
            items.push(send(Direction::Received, Message::new(5004, p)));
        }
        let ev = run(&mut dec, items);
        let entries = ev.iter().find_map(|e| match e {
            Event::Directory { entries, .. } => Some(entries.clone()),
            _ => None,
        });
        assert_eq!(entries.unwrap()[0].index, 7);
        assert!(ev.iter().any(|e| matches!(
            e,
            Event::FileDownloaded {
                index: 0,
                crc_ok: true,
                ..
            }
        )));
    }

    #[test]
    fn cccd_write_is_classified_not_unknown() {
        let mut dec = LinkDecoder::new();
        let ev = dec.push(
            Direction::Sent,
            &Att::Write {
                handle: 0x19,
                value: vec![1, 0],
                command: false,
            },
        );
        assert_eq!(
            ev,
            vec![Event::DescriptorWrite {
                att_handle: 0x19,
                notify: true,
                indicate: false
            }]
        );
    }

    #[test]
    fn value_on_unknown_characteristic_is_reported_not_dropped() {
        let mut dec = LinkDecoder::new();
        let ev = dec.push(
            Direction::Received,
            &Att::Notify {
                handle: 0x99,
                value: vec![0xF0, 0x00, 1],
                indication: false,
            },
        );
        assert!(matches!(
            ev[0],
            Event::Unknown {
                att_handle: 0x99,
                ..
            }
        ));
    }
}
