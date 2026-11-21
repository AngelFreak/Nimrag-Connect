//! Find my phone (5039 / 5040, watch to phone) and find my watch (protobuf
//! `Smart.find_my_watch_service` 12, phone to watch).
//!
//! Ported from Gadgetbridge commit 7257742b (AGPL-3.0-or-later):
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/messages/FindMyPhoneRequestMessage.java`,
//! `.../messages/FindMyPhoneCancelMessage.java`, `.../garmin/GarminSupport.java`
//! (`onFindDevice`; Copyright (C) 2024-2026 Daniele Gobbetti, José Rebelo, kuhy,
//! Thomas Kuehne and the other Gadgetbridge contributors) and
//! `.../garmin/ProtocolBufferHandler.java` (`processProtobufFindMyWatchResponse`;
//! no copyright line in the file; Gadgetbridge contributors). The two
//! `FindMyPhone*Message` files have no copyright line either. The protobuf is
//! written against our own recovered schema
//! `docs/protocol/proto/gdi_find_my_watch.proto`. *inferred (Gadgetbridge)*.

use crate::messages::{next_field, Wire};

pub const FIND_MY_PHONE_REQUEST: u16 = 5039;
pub const FIND_MY_PHONE_CANCEL: u16 = 5040;

/// Capability bit (in the watch's Configuration mask) for "the watch can be
/// asked to ring" (`GarminCapability.FIND_MY_WATCH`). Bit `i` is byte `i / 8`,
/// value `1 << (i % 8)`.
pub const CAP_FIND_MY_WATCH: usize = 9;

/// 5039 payload: `u8 duration` (seconds; Gadgetbridge ignores it and rings
/// until the cancel). `None` for an empty payload.
pub fn parse_find_phone_request(p: &[u8]) -> Option<u8> {
    p.first().copied()
}

/// `Smart.find_my_watch_service(12) { find_request(1) { timeout(1) = 60 } }`.
pub const FIND_WATCH_START_PROTOBUF: [u8; 6] = [0x62, 0x04, 0x0A, 0x02, 0x08, 0x3C];

/// `Smart.find_my_watch_service(12) { cancel_request(3) {} }`.
pub const FIND_WATCH_CANCEL_PROTOBUF: [u8; 4] = [0x62, 0x02, 0x1A, 0x00];

/// `ResponseStatus.OK` of `gdi_find_my_watch.proto`.
pub const RESPONSE_OK: u64 = 100;

/// What a `find_my_watch_service` message from the watch says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindWatchMessage {
    /// The answer to our find request; `ok` is false for any status but OK.
    FindResponse { ok: bool },
    /// The answer to our cancel request.
    CancelResponse { ok: bool },
    /// The watch cancels by itself (the wearer pressed a button).
    CancelRequest,
}

/// Decode the Smart bytes of a protobuf message from the watch and return the
/// first `find_my_watch_service` (field 12) message in it. `None` when there
/// is none, or the bytes are malformed.
pub fn parse_find_my_watch(body: &[u8]) -> Option<FindWatchMessage> {
    let mut i = 0;
    while i < body.len() {
        if let (12, Wire::Bytes(service)) = next_field(body, &mut i)? {
            return service_message(service);
        }
    }
    None
}

fn service_message(b: &[u8]) -> Option<FindWatchMessage> {
    let mut i = 0;
    while i < b.len() {
        match next_field(b, &mut i)? {
            (2, Wire::Bytes(r)) => {
                return Some(FindWatchMessage::FindResponse { ok: status_ok(r)? })
            }
            (3, Wire::Bytes(_)) => return Some(FindWatchMessage::CancelRequest),
            (4, Wire::Bytes(r)) => {
                return Some(FindWatchMessage::CancelResponse { ok: status_ok(r)? })
            }
            _ => {}
        }
    }
    None
}

/// `status` (field 1) of a response message; a missing status is not OK.
fn status_ok(b: &[u8]) -> Option<bool> {
    let mut i = 0;
    let mut ok = false;
    while i < b.len() {
        if let (1, Wire::Varint(v)) = next_field(b, &mut i)? {
            ok = v == RESPONSE_OK;
        }
    }
    Some(ok)
}

/// True when bit `bit` is set in a capability mask (bit `i` is byte `i / 8`,
/// value `1 << (i % 8)`).
pub fn has_capability(mask: &[u8], bit: usize) -> bool {
    mask.get(bit / 8).is_some_and(|b| b & (1 << (bit % 8)) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_bytes() {
        assert_eq!(parse_find_phone_request(&[30]), Some(30));
        assert_eq!(parse_find_phone_request(&[]), None);
    }

    #[test]
    fn protobuf_requests_are_smart_field_12() {
        // field 12, wire type 2 = (12 << 3) | 2 = 0x62
        assert_eq!(FIND_WATCH_START_PROTOBUF[0], 0x62);
        assert_eq!(FIND_WATCH_CANCEL_PROTOBUF[0], 0x62);
        // find_request { timeout = 60 }
        assert_eq!(&FIND_WATCH_START_PROTOBUF[2..], &[0x0A, 0x02, 0x08, 60]);
        // cancel_request {}
        assert_eq!(&FIND_WATCH_CANCEL_PROTOBUF[2..], &[0x1A, 0x00]);
    }

    #[test]
    fn responses_are_read() {
        // find_response { status = 100 }
        let ok = [0x62, 0x04, 0x12, 0x02, 0x08, 100];
        assert_eq!(
            parse_find_my_watch(&ok),
            Some(FindWatchMessage::FindResponse { ok: true })
        );
        // cancel_response { status = 200 (ERROR) }
        let err = [0x62, 0x05, 0x22, 0x03, 0x08, 0xC8, 0x01];
        assert_eq!(
            parse_find_my_watch(&err),
            Some(FindWatchMessage::CancelResponse { ok: false })
        );
        assert_eq!(
            parse_find_my_watch(&[0x62, 0x02, 0x1A, 0x00]),
            Some(FindWatchMessage::CancelRequest)
        );
    }

    #[test]
    fn other_services_and_garbage_give_none() {
        // device_status_service (8) only
        assert_eq!(parse_find_my_watch(&[0x42, 0x02, 0x12, 0x00]), None);
        assert_eq!(parse_find_my_watch(&[]), None);
        assert_eq!(parse_find_my_watch(&[0x62, 0x10, 0x12]), None);
        assert_eq!(parse_find_my_watch(&[0xFF; 12]), None);
    }

    #[test]
    fn capability_bits_follow_the_mask_layout() {
        // bit 9 = byte 1, value 0x02
        assert!(has_capability(&[0x00, 0x02], CAP_FIND_MY_WATCH));
        assert!(!has_capability(&[0xFF], CAP_FIND_MY_WATCH));
        assert!(has_capability(&[0x01], 0));
        assert!(!has_capability(&[], 0));
    }
}
