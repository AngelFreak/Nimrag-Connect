//! Multi-link transport over GATT service 6A4E2800.
//!
//! Every characteristic value starts with a one-byte handle. Handle 0 is the
//! control channel; other handles carry a registered service's byte stream
//! (GFDI is service id 1). See docs/protocol/PROTOCOL.md "Transport".

/// 128-bit UUIDs (big-endian text form) used by multi-link.
pub const SERVICE_UUID: &str = "6a4e2800-667b-11e3-949a-0800200c9a66";
pub const INFO_CHAR_UUID: &str = "6a4e2803-667b-11e3-949a-0800200c9a66";

/// Is this 16-bit Garmin UUID prefix (6A4Exxxx) a multi-link notify char (2810..2819)?
pub fn is_notify_char(short: u16) -> bool {
    (0x2810..=0x2819).contains(&short)
}

/// Is this a multi-link write char (2820..2829)?
pub fn is_write_char(short: u16) -> bool {
    (0x2820..=0x2829).contains(&short)
}

/// Known multi-link service ids.
pub fn service_name(id: u16) -> &'static str {
    match id {
        1 => "GFDI",
        2 => "NFC",
        3 => "HEALTH_SDK",
        4 => "REGISTRATION",
        5 => "CONNEXT",
        6 => "REAL_TIME_HR",
        7 => "REAL_TIME_STEPS",
        8 => "REAL_TIME_CALORIES",
        9 => "REAL_TIME_FLOORS",
        10 => "REAL_TIME_INTENSITY",
        12 => "REAL_TIME_HRV",
        13 => "REAL_TIME_STRESS",
        14 => "AUTH_STATUS",
        15 => "ECHO",
        16 => "REAL_TIME_ACCELEROMETER",
        17 => "REAL_TIME_SPAM",
        19 => "REAL_TIME_SPO2",
        20 => "REAL_TIME_BODY_BATTERY",
        21 => "REAL_TIME_RESPIRATION",
        22 => "KEEP_ALIVE",
        25 => "REAL_TIME_TRAINING_READINESS",
        26 => "REAL_TIME_ACTIVE_TIME",
        29 => "REAL_TIME_PUSHES",
        _ => "UNKNOWN",
    }
}

/// A control-channel (handle 0) message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control {
    RegisterRequest {
        client_id: u64,
        service: u16,
        mode: u8,
    },
    RegisterResponse {
        client_id: u64,
        service: u16,
        status: u8,
        /// Present when status == 0.
        handle: Option<u8>,
        /// bit0 = reliable (MLR) granted.
        flags: Option<u8>,
        /// Registration-service version byte when present.
        extra: Option<u8>,
        /// status 3: another characteristic to retry on (6A4Exxxx).
        redirect: Option<u16>,
    },
    CloseHandleRequest {
        client_id: u64,
        service: u16,
        handle: u8,
    },
    CloseHandleResponse {
        client_id: u64,
        service: u16,
        handle: u8,
        status: u8,
    },
    InvalidHandle {
        client_id: u64,
        service: u16,
        handle: u8,
    },
    CloseAllRequest {
        client_id: u64,
    },
    CloseAllResponse {
        client_id: u64,
        status: u8,
    },
    UnknownCommandResponse {
        client_id: u64,
        service: u16,
        original: u8,
    },
    Unparsed {
        kind: u8,
        raw: Vec<u8>,
    },
}

impl Control {
    pub fn register_status_name(status: u8) -> &'static str {
        match status {
            0 => "SUCCESS",
            1 => "INVALID_SERVICE_ID",
            2 => "PENDING_AUTH",
            3 => "ALREADY_IN_USE",
            4 => "REJECTED",
            _ => "UNKNOWN",
        }
    }
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
}

fn u64_at(b: &[u8], i: usize) -> Option<u64> {
    let s = b.get(i..i + 8)?;
    Some(u64::from_le_bytes(s.try_into().ok()?))
}

/// Parse a characteristic value whose first byte is handle 0.
pub fn parse_control(value: &[u8]) -> Option<Control> {
    if value.first() != Some(&0) || value.len() < 2 {
        return None;
    }
    let kind = value[1];
    let parsed = (|| {
        let client_id = u64_at(value, 2)?;
        Some(match kind {
            0 => Control::RegisterRequest {
                client_id,
                service: u16_at(value, 10)?,
                mode: *value.get(12)?,
            },
            1 => {
                let service = u16_at(value, 10)?;
                let status = *value.get(12)?;
                let (handle, flags, extra, redirect) = match status {
                    0 => (
                        value.get(13).copied(),
                        value.get(14).copied(),
                        value.get(15).copied(),
                        None,
                    ),
                    3 => (None, None, None, u16_at(value, 13)),
                    _ => (None, None, None, None),
                };
                Control::RegisterResponse {
                    client_id,
                    service,
                    status,
                    handle,
                    flags,
                    extra,
                    redirect,
                }
            }
            2 => Control::CloseHandleRequest {
                client_id,
                service: u16_at(value, 10)?,
                handle: *value.get(12)?,
            },
            3 => Control::CloseHandleResponse {
                client_id,
                service: u16_at(value, 10)?,
                handle: *value.get(12)?,
                status: *value.get(13)?,
            },
            4 => Control::InvalidHandle {
                client_id,
                service: u16_at(value, 10)?,
                handle: *value.get(12)?,
            },
            5 => Control::CloseAllRequest { client_id },
            6 => Control::CloseAllResponse {
                client_id,
                status: *value.get(12)?,
            },
            0xFF => Control::UnknownCommandResponse {
                client_id,
                service: u16_at(value, 10)?,
                original: *value.get(12)?,
            },
            _ => return None,
        })
    })();
    Some(parsed.unwrap_or(Control::Unparsed {
        kind,
        raw: value.to_vec(),
    }))
}

/// Build a register request (13 bytes).
pub fn register_request(client_id: u64, service: u16, mode: u8) -> Vec<u8> {
    let mut v = vec![0, 0];
    v.extend_from_slice(&client_id.to_le_bytes());
    v.extend_from_slice(&service.to_le_bytes());
    v.push(mode);
    v
}

/// Build a close-all request (12 bytes).
pub fn close_all_request(client_id: u64) -> Vec<u8> {
    let mut v = vec![0, 5];
    v.extend_from_slice(&client_id.to_le_bytes());
    v.extend_from_slice(&0u16.to_le_bytes());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_request_roundtrip() {
        let v = register_request(0x1122334455667788, 1, 2);
        assert_eq!(v.len(), 13);
        assert_eq!(
            parse_control(&v),
            Some(Control::RegisterRequest {
                client_id: 0x1122334455667788,
                service: 1,
                mode: 2
            })
        );
    }

    #[test]
    fn register_response_success_with_reliable_flag() {
        let mut v = vec![0, 1];
        v.extend_from_slice(&7u64.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&[0, 3, 1, 0]);
        match parse_control(&v).unwrap() {
            Control::RegisterResponse {
                status: 0,
                handle: Some(3),
                flags: Some(1),
                ..
            } => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn register_response_redirect() {
        let mut v = vec![0, 1];
        v.extend_from_slice(&7u64.to_le_bytes());
        v.extend_from_slice(&4u16.to_le_bytes());
        v.push(3);
        v.extend_from_slice(&0x2811u16.to_le_bytes());
        match parse_control(&v).unwrap() {
            Control::RegisterResponse {
                status: 3,
                redirect: Some(0x2811),
                ..
            } => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn close_all_roundtrip_and_non_control_is_none() {
        assert_eq!(
            parse_control(&close_all_request(9)),
            Some(Control::CloseAllRequest { client_id: 9 })
        );
        assert_eq!(parse_control(&[3, 1, 2]), None);
    }

    #[test]
    fn short_known_kind_is_unparsed_not_panic() {
        assert!(matches!(
            parse_control(&[0, 1, 0]),
            Some(Control::Unparsed { kind: 1, .. })
        ));
    }
}
