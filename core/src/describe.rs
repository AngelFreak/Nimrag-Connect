//! Human-readable summaries of message payloads, for tools/decode.
//! Layouts are documented in docs/protocol/PROTOCOL.md.

use crate::gfdi::{message_name, Message, Status};
use crate::messages;

fn u32le(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn bits(mask: &[u8]) -> Vec<usize> {
    let mut v = Vec::new();
    for (k, byte) in mask.iter().enumerate() {
        for i in 0..8 {
            if byte & (1 << i) != 0 {
                v.push(k * 8 + i);
            }
        }
    }
    v
}

/// Seconds since 1989-12-31T00:00:00Z ("Garmin time") as an ISO-like string.
pub fn garmin_time(t: u32) -> String {
    unix_time(t as i64 + 631_065_600)
}

pub fn unix_time(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

pub use crate::messages::DirEntry;

impl DirEntry {
    pub fn describe(&self) -> String {
        let ts = if self.timestamp == 0 {
            "-".into()
        } else {
            garmin_time(self.timestamp)
        };
        format!(
            "index={} type={}/{} ({}) number={} flags=0x{:02x}{} specific=0x{:02x} size={} time={}",
            self.index,
            self.data_type,
            self.sub_type,
            file_type_name(self.data_type, self.sub_type),
            self.file_number,
            self.flags,
            if self.flags & 0x10 != 0 {
                "[archived]"
            } else {
                ""
            },
            self.specific_flags,
            self.size,
            ts
        )
    }
}

/// Our names for FIT file sub-types (FIT `file` enum values; data type 128).
pub fn file_type_name(data_type: u8, sub_type: u8) -> &'static str {
    match (data_type, sub_type) {
        (0, 0) => "directory",
        (8, 255) => "device-xml",
        (128, 1) => "device",
        (128, 2) => "settings",
        (128, 3) => "sport",
        (128, 4) => "activity",
        (128, 5) => "workout",
        (128, 6) => "course",
        (128, 7) => "schedules",
        (128, 9) => "weight",
        (128, 10) => "totals",
        (128, 11) => "goals",
        (128, 14) => "blood-pressure",
        (128, 15) => "monitoring-a",
        (128, 20) => "activity-summary",
        (128, 28) => "monitoring-daily",
        (128, 32) => "monitoring-b",
        (128, 34) => "segment",
        (128, 35) => "segment-list",
        (128, 41) => "changelog",
        (128, 44) => "metrics",
        (128, 49) => "sleep",
        (128, 52) => "usage-log",
        (128, 57) => "sport-backup",
        (128, 68) => "hrv-status",
        (128, _) => "fit-other",
        _ => "other",
    }
}

/// Generic protobuf wire-format dump (no schema): `field:type=value` list.
pub fn protobuf_wire(b: &[u8], depth: usize) -> Option<String> {
    fn varint(b: &[u8], i: &mut usize) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *b.get(*i)?;
            *i += 1;
            v |= ((byte & 0x7F) as u64) << shift;
            if byte & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
    let mut i = 0;
    let mut parts = Vec::new();
    while i < b.len() {
        let key = varint(b, &mut i)?;
        let (field, wt) = (key >> 3, key & 7);
        if field == 0 {
            return None;
        }
        match wt {
            0 => parts.push(format!("{field}={}", varint(b, &mut i)?)),
            1 => {
                let v = u64::from_le_bytes(b.get(i..i + 8)?.try_into().ok()?);
                i += 8;
                parts.push(format!("{field}=0x{v:016x}"));
            }
            5 => {
                let v = u32le(b, i)?;
                i += 4;
                parts.push(format!("{field}=0x{v:08x}"));
            }
            2 => {
                let n = varint(b, &mut i)? as usize;
                let s = b.get(i..i + n)?;
                i += n;
                let printable = !s.is_empty() && s.iter().all(|c| (0x20..0x7F).contains(c));
                let nested = if depth < 6 && !printable {
                    protobuf_wire(s, depth + 1)
                } else {
                    None
                };
                match nested {
                    _ if printable => {
                        parts.push(format!("{field}={:?}", String::from_utf8_lossy(s)))
                    }
                    Some(n) if !s.is_empty() => parts.push(format!("{field}{{{n}}}")),
                    _ => parts.push(format!("{field}=<{}>", hex(s))),
                }
            }
            _ => return None,
        }
    }
    Some(parts.join(" "))
}

fn system_event_name(e: u8) -> &'static str {
    match e {
        0 => "SYNC_COMPLETE",
        1 => "SYNC_FAIL",
        2 => "FACTORY_RESET",
        3 => "PAIR_START",
        4 => "PAIR_COMPLETE",
        5 => "PAIR_FAIL",
        6 => "HOST_DID_ENTER_FOREGROUND",
        7 => "HOST_DID_ENTER_BACKGROUND",
        8 => "HANDSHAKE_COMPLETE",
        9 => "NEW_DOWNLOAD_AVAILABLE",
        10 => "DEVICE_SOFTWARE_UPDATE",
        11 => "DEVICE_DISCONNECT",
        12 => "TUTORIAL_COMPLETE",
        13 => "SETUP_WIZARD_START",
        14 => "SETUP_WIZARD_COMPLETE",
        15 => "SETUP_WIZARD_SKIPPED",
        16 => "TIME_UPDATED",
        17 => "REQUEST_REBOOT",
        _ => "UNKNOWN",
    }
}

/// Summarise a request (non-5000) message's payload.
fn describe_request(id: u16, p: &[u8]) -> Option<String> {
    Some(match id {
        5024 => {
            let d = messages::parse_device_information(p)?;
            format!(
                "protocol={} product={} unit_id=<redacted> software={} max_packet={} bt_name={:?} device={:?} model={:?} tail=<{}>",
                d.protocol,
                d.product,
                d.software,
                d.max_packet,
                d.bluetooth_name,
                d.device_name,
                d.model_name,
                d.tail.len()
            )
        }
        5050 => {
            let n = *p.first()? as usize;
            format!("capabilities={:?}", bits(p.get(1..1 + n)?))
        }
        5037 | 5027 => {
            let (opt, mask) = if id == 5037 {
                let n = *p.get(1)? as usize;
                (Some(*p.first()?), p.get(2..2 + n)?)
            } else {
                let n = *p.first()? as usize;
                (None, p.get(1..1 + n)?)
            };
            let o = match opt {
                Some(0) => "option=MANUAL ",
                Some(1) => "option=INVISIBLE ",
                Some(2) => "option=VISIBLE_AS_NEEDED ",
                Some(_) => "option=? ",
                None => "",
            };
            format!("{o}bits={:?}", bits(mask))
        }
        5009 => DirEntry::parse(p)?.describe(),
        5002 => {
            let r = messages::parse_download_request(p)?;
            format!(
                "index={} offset={} type={} crc_seed=0x{:04x} max_size={} flags=0x{:02x}",
                r.index, r.offset, r.kind, r.crc_seed, r.max_size, r.flags
            )
        }
        5004 | 5054 => {
            let c = messages::parse_file_chunk(p)?;
            format!(
                "flags=0x{:02x} crc=0x{:04x} offset={} len={}",
                c.flags,
                c.crc,
                c.offset,
                c.data.len()
            )
        }
        5007 => format!("filter={}", p.first()?),
        5008 => {
            let (index, flags) = messages::parse_set_file_flags(p)?;
            format!("index={index} flags=0x{flags:02x}")
        }
        5030 => {
            let (event, value) = messages::parse_system_event(p)?;
            format!("event={} value={value}", system_event_name(event))
        }
        5052 => format!("reference=0x{:08x}", u32le(p, 0)?),
        5031 => "request".into(),
        5023 => format!("raw={}", hex(p)),
        5043 | 5044 => {
            let (h, body) = messages::parse_protobuf(p)?;
            format!(
                "request_id={} offset={} total={} chunk={} proto=[{}]",
                h.request_id,
                h.offset,
                h.total,
                h.chunk,
                protobuf_wire(body, 0).unwrap_or_else(|| format!("raw {}", hex(body)))
            )
        }
        _ => return None,
    })
}

/// Summarise the body of a 5000 response to `request_id`.
fn describe_response_body(request_id: u16, b: &[u8]) -> Option<String> {
    Some(match request_id {
        5024 => {
            let h = messages::parse_host_info(b)?;
            format!(
                "host protocol={} app_version={} name={:?} manufacturer={:?} model={:?}",
                h.protocol, h.app_version, h.name, h.manufacturer, h.model
            )
        }
        5002 => {
            let r = messages::parse_download_reply(b)?;
            format!("download_status={} size={}", r.status, r.size)
        }
        5004 | 5054 => {
            let (status, received) = messages::parse_chunk_ack(b)?;
            format!("transfer_status={status} received={received}")
        }
        5007 => format!("filter_status={}", b.first()?),
        5052 => {
            let r = messages::parse_current_time_reply(b)?;
            format!(
                "reference=0x{:08x} time={} utc_offset={}s dst_start={} dst_end={}",
                r.reference,
                garmin_time(r.garmin_time),
                r.utc_offset_s,
                r.dst_start,
                r.dst_end
            )
        }
        5031 => {
            let types = messages::parse_supported_file_types(b)?;
            let v: Vec<String> = types
                .iter()
                .map(|t| format!("{}/{}:{}", t.data_type, t.sub_type, t.name))
                .collect();
            format!("types=[{}]", v.join(", "))
        }
        _ if b.is_empty() => String::new(),
        _ => format!("body={}", hex(b)),
    })
}

/// One-line description of a GFDI message.
pub fn describe(m: &Message) -> String {
    let name = message_name(m.id).unwrap_or("Unknown");
    let txn = m.txn.map(|t| format!(" txn={t}")).unwrap_or_default();
    if let Some(r) = m.as_response() {
        let req = message_name(r.request_id).unwrap_or("Unknown");
        let status = match r.status {
            Status::Ack => "ACK".to_string(),
            s => format!("{s:?}"),
        };
        let body = describe_response_body(r.request_id, r.body)
            .unwrap_or_else(|| format!("body={}", hex(r.body)));
        return format!("Response({req} {}){txn} {status} {body}", r.request_id)
            .trim_end()
            .to_string();
    }
    let body = describe_request(m.id, &m.payload)
        .unwrap_or_else(|| format!("payload={}", hex(&m.payload)));
    format!("{name}({}){txn} {body}", m.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garmin_epoch_is_1989_12_31() {
        assert_eq!(garmin_time(0), "1989-12-31T00:00:00Z");
        assert_eq!(unix_time(0), "1970-01-01T00:00:00Z");
        assert_eq!(unix_time(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn protobuf_wire_dump() {
        // field 1 varint 150, field 2 bytes "hi", field 3 nested {1=1}
        let b = [
            0x08, 0x96, 0x01, 0x12, 0x02, b'h', b'i', 0x1A, 0x02, 0x08, 0x01,
        ];
        assert_eq!(protobuf_wire(&b, 0).unwrap(), "1=150 2=\"hi\" 3{1=1}");
    }

    #[test]
    fn describes_response_and_request() {
        let r = Message::new(5000, vec![0xA6, 0x13, 0x00]);
        assert_eq!(describe(&r), "Response(SystemEvent 5030) ACK");
        let e = Message::new(5030, vec![16, 0]);
        assert_eq!(describe(&e), "SystemEvent(5030) event=TIME_UPDATED value=0");
    }

    #[test]
    fn dir_entry_parse() {
        let mut b = vec![5, 0, 128, 4, 1, 0, 0, 0x90];
        b.extend_from_slice(&1000u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        let e = DirEntry::parse(&b).unwrap();
        assert_eq!(
            (e.index, e.data_type, e.sub_type, e.size),
            (5, 128, 4, 1000)
        );
        assert!(e.describe().contains("activity"));
    }
}
