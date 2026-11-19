//! Typed build and parse for the GFDI message payloads the client uses.
//!
//! Layouts are in docs/protocol/PROTOCOL.md (sections 3 and 5). Every parser
//! returns `None` on short or malformed input and never panics; this is the
//! one implementation of each layout, shared by the client, `describe` and
//! `link`.

pub const DOWNLOAD_FILE: u16 = 5002;
pub const FILE_DATA: u16 = 5004;
pub const COMPRESSED_FILE_DATA: u16 = 5054;
pub const DIRECTORY_FILTER: u16 = 5007;
pub const SET_FILE_FLAGS: u16 = 5008;
pub const DEVICE_INFORMATION: u16 = 5024;
pub const SYSTEM_EVENT: u16 = 5030;
pub const SUPPORTED_FILE_TYPES: u16 = 5031;
pub const SYNC_REQUEST: u16 = 5037;
pub const PROTOBUF_REQUEST: u16 = 5043;
pub const PROTOBUF_RESPONSE: u16 = 5044;
pub const CONFIGURATION: u16 = 5050;
pub const CURRENT_TIME: u16 = 5052;
pub const AUTH_NEGOTIATION: u16 = 5101;

/// GFDI protocol version we speak and require (compact transaction ids).
pub const PROTOCOL_VERSION: u16 = 150;

/// System event numbers carried by 5030.
pub const EVENT_SYNC_COMPLETE: u8 = 0;
/// First connection only. Event numbers: PROTOCOL.md §9 (Gadgetbridge
/// `SystemEventMessage`, *inferred*).
pub const EVENT_PAIR_COMPLETE: u8 = 4;
pub const EVENT_HANDSHAKE_COMPLETE: u8 = 8;
/// First connection only (see `EVENT_PAIR_COMPLETE`).
pub const EVENT_SETUP_WIZARD_COMPLETE: u8 = 14;

/// Directory entry flag: the file may be read.
pub const FLAG_READ: u8 = 0x80;
/// Archive flag for 5008.
pub const FLAG_ARCHIVE: u8 = 0x10;

fn u16le(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
}

fn u32le(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// u8 length-prefixed string at `*pos`.
fn lstr(b: &[u8], pos: &mut usize) -> Option<String> {
    let n = *b.get(*pos)? as usize;
    let s = b.get(*pos + 1..*pos + 1 + n)?;
    *pos += 1 + n;
    Some(String::from_utf8_lossy(s).into_owned())
}

fn push_lstr(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let n = bytes.len().min(255);
    out.push(n as u8);
    out.extend_from_slice(&bytes[..n]);
}

/// One entry of the watch's supported-file-types list (5031 reply).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileType {
    pub data_type: u8,
    pub sub_type: u8,
    pub name: String,
}

// ---- directory ----------------------------------------------------------

/// One 16-byte directory entry (also the FileReady payload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub index: u16,
    pub data_type: u8,
    pub sub_type: u8,
    pub file_number: u16,
    pub specific_flags: u8,
    pub flags: u8,
    pub size: u32,
    pub timestamp: u32,
}

impl DirEntry {
    pub fn parse(b: &[u8]) -> Option<Self> {
        Some(Self {
            index: u16le(b, 0)?,
            data_type: *b.get(2)?,
            sub_type: *b.get(3)?,
            file_number: u16le(b, 4)?,
            specific_flags: *b.get(6)?,
            flags: *b.get(7)?,
            size: u32le(b, 8)?,
            timestamp: u32le(b, 12)?,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(16);
        v.extend_from_slice(&self.index.to_le_bytes());
        v.push(self.data_type);
        v.push(self.sub_type);
        v.extend_from_slice(&self.file_number.to_le_bytes());
        v.push(self.specific_flags);
        v.push(self.flags);
        v.extend_from_slice(&self.size.to_le_bytes());
        v.extend_from_slice(&self.timestamp.to_le_bytes());
        v
    }
}

/// The downloaded directory file: a 16-byte header, then 16-byte entries.
/// Every entry is returned, including a placeholder with index 0.
pub fn parse_directory(data: &[u8]) -> Vec<DirEntry> {
    data.chunks_exact(16)
        .skip(1)
        .filter_map(DirEntry::parse)
        .collect()
}

/// Build a directory file (header of zeros, then the entries).
pub fn build_directory(entries: &[DirEntry]) -> Vec<u8> {
    let mut v = vec![0u8; 16];
    for e in entries {
        v.extend_from_slice(&e.encode());
    }
    v
}

// ---- 5024 DeviceInformation and the host reply ----------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInformation {
    pub protocol: u16,
    pub product: u16,
    pub unit_id: u32,
    pub software: u16,
    pub max_packet: u16,
    pub bluetooth_name: String,
    pub device_name: String,
    pub model_name: String,
    /// Bytes after the three strings (dual-pairing and limitation flags).
    pub tail: Vec<u8>,
}

pub fn parse_device_information(p: &[u8]) -> Option<DeviceInformation> {
    let mut pos = 12;
    let bluetooth_name = lstr(p, &mut pos)?;
    let device_name = lstr(p, &mut pos)?;
    let model_name = lstr(p, &mut pos)?;
    Some(DeviceInformation {
        protocol: u16le(p, 0)?,
        product: u16le(p, 2)?,
        unit_id: u32le(p, 4)?,
        software: u16le(p, 8)?,
        max_packet: u16le(p, 10)?,
        bluetooth_name,
        device_name,
        model_name,
        tail: p[pos..].to_vec(),
    })
}

pub fn build_device_information(d: &DeviceInformation) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&d.protocol.to_le_bytes());
    v.extend_from_slice(&d.product.to_le_bytes());
    v.extend_from_slice(&d.unit_id.to_le_bytes());
    v.extend_from_slice(&d.software.to_le_bytes());
    v.extend_from_slice(&d.max_packet.to_le_bytes());
    push_lstr(&mut v, &d.bluetooth_name);
    push_lstr(&mut v, &d.device_name);
    push_lstr(&mut v, &d.model_name);
    v.extend_from_slice(&d.tail);
    v
}

/// What the phone tells the watch about itself in the ACK of 5024.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInfo {
    pub protocol: u16,
    pub app_version: u16,
    pub name: String,
    pub manufacturer: String,
    pub model: String,
}

/// Body of the 5000 ACK to 5024 (after the status byte).
pub fn build_host_info(h: &HostInfo) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&h.protocol.to_le_bytes());
    v.extend_from_slice(&0xFFFFu16.to_le_bytes());
    v.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    v.extend_from_slice(&h.app_version.to_le_bytes());
    v.extend_from_slice(&0xFFFFu16.to_le_bytes());
    push_lstr(&mut v, &h.name);
    push_lstr(&mut v, &h.manufacturer);
    push_lstr(&mut v, &h.model);
    v.push(1);
    v
}

pub fn parse_host_info(b: &[u8]) -> Option<HostInfo> {
    let mut pos = 12;
    let name = lstr(b, &mut pos)?;
    let manufacturer = lstr(b, &mut pos)?;
    let model = lstr(b, &mut pos)?;
    Some(HostInfo {
        protocol: u16le(b, 0)?,
        app_version: u16le(b, 8)?,
        name,
        manufacturer,
        model,
    })
}

// ---- 5050 Configuration -----------------------------------------------------

/// `u8 n` followed by the n-byte capability bitmask.
pub fn build_configuration(mask: &[u8]) -> Vec<u8> {
    let n = mask.len().min(255);
    let mut v = vec![n as u8];
    v.extend_from_slice(&mask[..n]);
    v
}

pub fn parse_configuration(p: &[u8]) -> Option<Vec<u8>> {
    let n = *p.first()? as usize;
    Some(p.get(1..1 + n)?.to_vec())
}

// ---- 5031 supported file types ------------------------------------------------

/// Body of the ACK to 5031: `u8 n`, then n x (`u8 type, u8 subtype, str name`).
pub fn parse_supported_file_types(b: &[u8]) -> Option<Vec<FileType>> {
    let n = *b.first()? as usize;
    let mut pos = 1;
    let mut v = Vec::new();
    for _ in 0..n {
        let data_type = *b.get(pos)?;
        let sub_type = *b.get(pos + 1)?;
        pos += 2;
        let name = lstr(b, &mut pos)?;
        v.push(FileType {
            data_type,
            sub_type,
            name,
        });
    }
    Some(v)
}

pub fn build_supported_file_types(types: &[FileType]) -> Vec<u8> {
    let mut v = vec![types.len().min(255) as u8];
    for t in types.iter().take(255) {
        v.push(t.data_type);
        v.push(t.sub_type);
        push_lstr(&mut v, &t.name);
    }
    v
}

// ---- 5037 SyncRequest -----------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRequest {
    pub option: u8,
    pub mask: Vec<u8>,
}

pub fn parse_sync_request(p: &[u8]) -> Option<SyncRequest> {
    let n = *p.get(1)? as usize;
    Some(SyncRequest {
        option: *p.first()?,
        mask: p.get(2..2 + n)?.to_vec(),
    })
}

pub fn build_sync_request(r: &SyncRequest) -> Vec<u8> {
    let mut v = vec![r.option, r.mask.len().min(255) as u8];
    v.extend_from_slice(&r.mask);
    v
}

// ---- 5052 CurrentTime ---------------------------------------------------------------

pub fn parse_current_time_request(p: &[u8]) -> Option<u32> {
    u32le(p, 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentTimeReply {
    pub reference: u32,
    /// Seconds since 1989-12-31T00:00Z.
    pub garmin_time: u32,
    pub utc_offset_s: i32,
    /// Garmin-epoch time of the next DST start (0 when there is none).
    pub dst_start: u32,
    /// Garmin-epoch time of the next DST end (0 when there is none).
    pub dst_end: u32,
}

pub fn build_current_time_reply(r: &CurrentTimeReply) -> Vec<u8> {
    let mut v = Vec::with_capacity(20);
    v.extend_from_slice(&r.reference.to_le_bytes());
    v.extend_from_slice(&r.garmin_time.to_le_bytes());
    v.extend_from_slice(&r.utc_offset_s.to_le_bytes());
    v.extend_from_slice(&r.dst_start.to_le_bytes());
    v.extend_from_slice(&r.dst_end.to_le_bytes());
    v
}

pub fn parse_current_time_reply(b: &[u8]) -> Option<CurrentTimeReply> {
    Some(CurrentTimeReply {
        reference: u32le(b, 0)?,
        garmin_time: u32le(b, 4)?,
        utc_offset_s: u32le(b, 8)? as i32,
        dst_start: u32le(b, 12)?,
        dst_end: u32le(b, 16)?,
    })
}

// ---- 5002 DownloadFile / 5004 FileData ------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadRequest {
    pub index: u16,
    pub offset: u32,
    /// 1 = new download.
    pub kind: u8,
    pub crc_seed: u16,
    /// 0 = the whole file.
    pub max_size: u32,
    pub flags: u8,
}

pub fn build_download_request(r: &DownloadRequest) -> Vec<u8> {
    let mut v = Vec::with_capacity(14);
    v.extend_from_slice(&r.index.to_le_bytes());
    v.extend_from_slice(&r.offset.to_le_bytes());
    v.push(r.kind);
    v.extend_from_slice(&r.crc_seed.to_le_bytes());
    v.extend_from_slice(&r.max_size.to_le_bytes());
    v.push(r.flags);
    v
}

/// The flags byte is optional (older captures end after `max_size`).
pub fn parse_download_request(p: &[u8]) -> Option<DownloadRequest> {
    Some(DownloadRequest {
        index: u16le(p, 0)?,
        offset: u32le(p, 2)?,
        kind: *p.get(6)?,
        crc_seed: u16le(p, 7)?,
        max_size: u32le(p, 9)?,
        flags: p.get(13).copied().unwrap_or(0),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadReply {
    /// 0 ok, 3 not ready (retry after 1 s), others refuse.
    pub status: u8,
    pub size: u32,
}

/// Body of the ACK to 5002. A refusal may omit the size.
pub fn parse_download_reply(b: &[u8]) -> Option<DownloadReply> {
    Some(DownloadReply {
        status: *b.first()?,
        size: u32le(b, 1).unwrap_or(0),
    })
}

pub fn build_download_reply(r: &DownloadReply) -> Vec<u8> {
    let mut v = vec![r.status];
    v.extend_from_slice(&r.size.to_le_bytes());
    v
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChunk {
    pub flags: u8,
    /// CRC-16/ARC over all file bytes so far, including this chunk.
    pub crc: u16,
    pub offset: u32,
    pub data: Vec<u8>,
}

pub fn parse_file_chunk(p: &[u8]) -> Option<FileChunk> {
    Some(FileChunk {
        flags: *p.first()?,
        crc: u16le(p, 1)?,
        offset: u32le(p, 3)?,
        data: p.get(7..)?.to_vec(),
    })
}

pub fn build_file_chunk(c: &FileChunk) -> Vec<u8> {
    let mut v = vec![c.flags];
    v.extend_from_slice(&c.crc.to_le_bytes());
    v.extend_from_slice(&c.offset.to_le_bytes());
    v.extend_from_slice(&c.data);
    v
}

/// Status values of the ACK to a 5004 chunk. [PROTOCOL.md 5.3]
pub const CHUNK_OK: u8 = 0;
pub const CHUNK_ABORT: u8 = 2;
pub const CHUNK_CRC_MISMATCH: u8 = 3;
/// Offset mismatch: answered with "abort" (2) as the stage 2a plan says; the
/// client then drops the partial data and re-requests from offset 0.
/// PROTOCOL.md 5.3 lists 4 as "offset mismatch, resend (keep the partial)".
/// That resend semantics is deliberately unused until a capture shows how the
/// watch reacts. Neither value was ever seen on the wire.
pub const CHUNK_OFFSET_MISMATCH: u8 = 2;

/// Body of the ACK to a chunk (after the status byte of the 5000 response is
/// built by the caller): `u8 status, u32 bytes_received`.
pub fn build_chunk_ack(status: u8, received: u32) -> Vec<u8> {
    let mut v = vec![status];
    v.extend_from_slice(&received.to_le_bytes());
    v
}

pub fn parse_chunk_ack(b: &[u8]) -> Option<(u8, u32)> {
    Some((*b.first()?, u32le(b, 1)?))
}

// ---- 5007 / 5008 / 5030 -------------------------------------------------------------------

/// 5007 filter value "pending uploads only".
pub const FILTER_PENDING: u8 = 3;

pub fn build_set_file_flags(index: u16, flags: u8) -> Vec<u8> {
    let mut v = index.to_le_bytes().to_vec();
    v.push(flags);
    v
}

pub fn parse_set_file_flags(p: &[u8]) -> Option<(u16, u8)> {
    Some((u16le(p, 0)?, *p.get(2)?))
}

pub fn build_system_event(event: u8, value: u8) -> Vec<u8> {
    vec![event, value]
}

pub fn parse_system_event(p: &[u8]) -> Option<(u8, u8)> {
    Some((*p.first()?, *p.get(1)?))
}

// ---- 5043 / 5044 protobuf envelope ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtobufHeader {
    pub request_id: u16,
    pub offset: u32,
    pub total: u32,
    pub chunk: u32,
}

/// Header and the Smart bytes that follow it.
pub fn parse_protobuf(p: &[u8]) -> Option<(ProtobufHeader, &[u8])> {
    Some((
        ProtobufHeader {
            request_id: u16le(p, 0)?,
            offset: u32le(p, 2)?,
            total: u32le(p, 6)?,
            chunk: u32le(p, 10)?,
        },
        p.get(14..)?,
    ))
}

/// A single-chunk protobuf message payload.
pub fn build_protobuf(request_id: u16, body: &[u8]) -> Vec<u8> {
    let mut v = request_id.to_le_bytes().to_vec();
    v.extend_from_slice(&0u32.to_le_bytes());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(body);
    v
}

/// Body of the ACK to a protobuf message: `u16 request_id` and six zero
/// bytes (data offset, chunk status, status code). [PROTOCOL.md 2.1]
pub fn build_protobuf_ack(request_id: u16) -> Vec<u8> {
    let mut v = request_id.to_le_bytes().to_vec();
    v.extend_from_slice(&[0; 6]);
    v
}

/// Smart.core(13) { 1 { 1 = 0 } }: the app's "sync now" request.
pub const SYNC_NOW_PROTOBUF: [u8; 6] = [0x6A, 0x04, 0x0A, 0x02, 0x08, 0x00];

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> DirEntry {
        DirEntry {
            index: 89,
            data_type: 128,
            sub_type: 32,
            file_number: 144,
            specific_flags: 0,
            flags: 0xA0,
            size: 1438,
            timestamp: 1_000_000,
        }
    }

    #[test]
    fn dir_entry_roundtrip() {
        assert_eq!(DirEntry::parse(&entry().encode()), Some(entry()));
        assert_eq!(DirEntry::parse(&[0; 15]), None);
    }

    #[test]
    fn directory_skips_the_header() {
        let dir = build_directory(&[entry()]);
        assert_eq!(dir.len(), 32);
        assert_eq!(parse_directory(&dir), vec![entry()]);
        assert!(parse_directory(&[0; 16]).is_empty());
    }

    #[test]
    fn device_information_roundtrip_and_short_input() {
        let d = DeviceInformation {
            protocol: 150,
            product: 1,
            unit_id: 2,
            software: 3,
            max_packet: 4000,
            bluetooth_name: "a".into(),
            device_name: "b".into(),
            model_name: "c".into(),
            tail: vec![0, 0],
        };
        let bytes = build_device_information(&d);
        assert_eq!(parse_device_information(&bytes), Some(d));
        assert_eq!(parse_device_information(&bytes[..bytes.len() - 5]), None);
    }

    #[test]
    fn host_info_layout() {
        let h = HostInfo {
            protocol: 150,
            app_version: 7,
            name: "n".into(),
            manufacturer: "m".into(),
            model: "o".into(),
        };
        let b = build_host_info(&h);
        assert_eq!(
            &b[..12],
            &[150, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 7, 0, 0xFF, 0xFF]
        );
        assert_eq!(b.last(), Some(&1));
        assert_eq!(parse_host_info(&b), Some(h));
    }

    #[test]
    fn current_time_reply_roundtrip() {
        let r = CurrentTimeReply {
            reference: 1,
            garmin_time: 2,
            utc_offset_s: -3600,
            dst_start: 4,
            dst_end: 5,
        };
        assert_eq!(
            parse_current_time_reply(&build_current_time_reply(&r)),
            Some(r)
        );
    }

    #[test]
    fn download_and_chunk_roundtrip() {
        let r = DownloadRequest {
            index: 9,
            offset: 0,
            kind: 1,
            crc_seed: 0,
            max_size: 0,
            flags: 1,
        };
        assert_eq!(parse_download_request(&build_download_request(&r)), Some(r));
        let c = FileChunk {
            flags: 0,
            crc: 0x1234,
            offset: 18,
            data: vec![1, 2, 3],
        };
        assert_eq!(parse_file_chunk(&build_file_chunk(&c)), Some(c));
        assert_eq!(parse_chunk_ack(&build_chunk_ack(3, 77)), Some((3, 77)));
    }

    #[test]
    fn sync_request_and_file_types_roundtrip() {
        let s = SyncRequest {
            option: 2,
            mask: vec![1, 2, 3],
        };
        assert_eq!(parse_sync_request(&build_sync_request(&s)), Some(s));
        assert_eq!(parse_sync_request(&[0, 5, 1]), None);
        let t = vec![FileType {
            data_type: 128,
            sub_type: 4,
            name: "x".into(),
        }];
        assert_eq!(
            parse_supported_file_types(&build_supported_file_types(&t)),
            Some(t)
        );
    }

    #[test]
    fn protobuf_envelope_and_sync_now_bytes() {
        let p = build_protobuf(3, &SYNC_NOW_PROTOBUF);
        let (h, body) = parse_protobuf(&p).unwrap();
        assert_eq!((h.request_id, h.offset, h.total, h.chunk), (3, 0, 6, 6));
        assert_eq!(body, &SYNC_NOW_PROTOBUF);
        assert_eq!(build_protobuf_ack(0x22e4), [0xe4, 0x22, 0, 0, 0, 0, 0, 0]);
    }
}
