//! GFDI messages: the unit carried inside each COBS frame.
//!
//! Layout (little-endian): `u16 total_len` (whole message, including itself
//! and the CRC), a 2-byte id field, payload, `u16 crc` (CRC-16/ARC over all
//! preceding bytes). The id field is either a plain `u16` message id, or the
//! compact transaction form: byte 2 = id - 5000, byte 3 = 0x80 | txn (txn in
//! the low 5 bits). See docs/protocol/PROTOCOL.md "Framing".

use crate::crc;

pub const RESPONSE: u16 = 5000;

/// A decoded GFDI message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: u16,
    /// Transaction id when the compact id form was used.
    pub txn: Option<u8>,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageError {
    TooShort(usize),
    LengthMismatch { header: usize, actual: usize },
    CrcMismatch { expected: u16, computed: u16 },
}

impl Message {
    pub fn new(id: u16, payload: Vec<u8>) -> Self {
        Self {
            id,
            txn: None,
            payload,
        }
    }

    /// Parse a de-COBSed frame.
    pub fn parse(frame: &[u8]) -> Result<Self, MessageError> {
        if frame.len() < 6 {
            return Err(MessageError::TooShort(frame.len()));
        }
        let header = u16::from_le_bytes([frame[0], frame[1]]) as usize;
        if header != frame.len() {
            return Err(MessageError::LengthMismatch {
                header,
                actual: frame.len(),
            });
        }
        let body_end = frame.len() - 2;
        let expected = u16::from_le_bytes([frame[body_end], frame[body_end + 1]]);
        let computed = crc::checksum(&frame[..body_end]);
        if expected != computed {
            return Err(MessageError::CrcMismatch { expected, computed });
        }
        let (id, txn) = if frame[3] & 0x80 != 0 {
            (frame[2] as u16 + 5000, Some(frame[3] & 0x1F))
        } else {
            (u16::from_le_bytes([frame[2], frame[3]]), None)
        };
        Ok(Self {
            id,
            txn,
            payload: frame[4..body_end].to_vec(),
        })
    }

    /// Serialize to the plaintext message (before COBS).
    pub fn encode(&self) -> Vec<u8> {
        let len = self.payload.len() + 6;
        let mut out = Vec::with_capacity(len);
        out.extend_from_slice(&(len as u16).to_le_bytes());
        match self.txn {
            Some(txn) => {
                out.push((self.id - 5000) as u8);
                out.push(0x80 | (txn & 0x1F));
            }
            None => out.extend_from_slice(&self.id.to_le_bytes()),
        }
        out.extend_from_slice(&self.payload);
        let c = crc::checksum(&out);
        out.extend_from_slice(&c.to_le_bytes());
        out
    }

    /// For a 5000 response: the message it answers, its status and the rest.
    pub fn as_response(&self) -> Option<Response<'_>> {
        if self.id != RESPONSE || self.payload.len() < 3 {
            return None;
        }
        Some(Response {
            request_id: u16::from_le_bytes([self.payload[0], self.payload[1]]),
            status: Status::from(self.payload[2]),
            body: &self.payload[3..],
        })
    }
}

/// Body of a 5000 response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response<'a> {
    pub request_id: u16,
    pub status: Status,
    pub body: &'a [u8],
}

/// Response status byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ack,
    Nak,
    UnknownOrNotSupported,
    CobsDecoderError,
    CrcError,
    LengthError,
    Other(u8),
}

impl From<u8> for Status {
    fn from(b: u8) -> Self {
        match b {
            0 => Status::Ack,
            1 => Status::Nak,
            2 => Status::UnknownOrNotSupported,
            3 => Status::CobsDecoderError,
            4 => Status::CrcError,
            5 => Status::LengthError,
            other => Status::Other(other),
        }
    }
}

/// Our own descriptive name for a message id, or None if unknown.
pub fn message_name(id: u16) -> Option<&'static str> {
    Some(match id {
        5000 => "Response",
        5002 => "DownloadFile",
        5003 => "UploadFile",
        5004 => "FileData",
        5005 => "CreateFile",
        5006 => "DeleteFile",
        5007 => "DirectoryFilter",
        5008 => "SetFileFlags",
        5009 => "FileReady",
        5011 => "FitDefinition",
        5012 => "FitData",
        5014 => "WeatherRequest",
        5015 => "WeatherAlert",
        5016 => "LiveTrackRequest",
        5019 => "EphemerisDataRequest",
        5020 => "EphemerisData",
        5022 => "CancelFileTransfer",
        5023 => "BatteryStatus",
        5024 => "DeviceInformation",
        5025 => "LiveTrackStop",
        5026 => "SetDeviceSettings",
        5027 => "QueuedDownload",
        5028 => "EphemerisEpoRequest",
        5029 => "EphemerisEpoData",
        5030 => "SystemEvent",
        5031 => "SupportedFileTypes",
        5033 => "NotificationSource",
        5034 => "NotificationControlPoint",
        5035 => "NotificationDataSource",
        5036 => "NotificationSubscription",
        5037 => "SyncRequest",
        5039 => "FindMyPhone",
        5040 => "CancelFindMyPhone",
        5041 => "MusicControl",
        5042 => "MusicControlCapabilities",
        5043 => "ProtobufRequest",
        5044 => "ProtobufResponse",
        5045 => "CancelProtobuf",
        5046 => "LiveTrackAutoStart",
        5047 => "LiveTrackAutoStartCancel",
        5048 => "LiveTrackAutoStartFailure",
        5049 => "MusicEntityUpdate",
        5050 => "Configuration",
        5052 => "CurrentTime",
        5054 => "CompressedFileData",
        5101 => "AuthNegotiationBegin",
        5102 => "LtkReconnect",
        5103 => "StkBeginGeneration",
        5104 => "ConfirmNumber",
        5105 => "StkRandNumber",
        5106 => "StkGenerationStatus",
        5107 => "LtkKeyDistribution",
        5108 => "SessionKeyDistribution",
        5109 => "SessionKeyVerification",
        5110 => "PasskeyRedisplay",
        5111 => "SecureSession",
        5112 => "OutOfBandPasskeyData",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_plain_id() {
        let m = Message::new(5024, vec![1, 2, 3]);
        let bytes = m.encode();
        assert_eq!(&bytes[..4], &[9, 0, 0xA0, 0x13]);
        assert_eq!(Message::parse(&bytes).unwrap(), m);
    }

    #[test]
    fn roundtrip_compact_transaction_id() {
        let m = Message {
            id: 5043,
            txn: Some(7),
            payload: vec![9],
        };
        let bytes = m.encode();
        assert_eq!(bytes[2], 43);
        assert_eq!(bytes[3], 0x87);
        assert_eq!(Message::parse(&bytes).unwrap(), m);
    }

    #[test]
    fn rejects_bad_crc_and_length() {
        let mut bytes = Message::new(5030, vec![16, 0]).encode();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        assert!(matches!(
            Message::parse(&bytes),
            Err(MessageError::CrcMismatch { .. })
        ));
        bytes.pop();
        assert!(matches!(
            Message::parse(&bytes),
            Err(MessageError::LengthMismatch { .. })
        ));
        assert_eq!(Message::parse(&[1, 2]), Err(MessageError::TooShort(2)));
    }

    #[test]
    fn response_body_is_split() {
        let m = Message::new(RESPONSE, vec![0xA0, 0x13, 0x00, 0xAA, 0xBB]);
        let r = m.as_response().unwrap();
        assert_eq!(r.request_id, 5024);
        assert_eq!(r.status, Status::Ack);
        assert_eq!(r.body, &[0xAA, 0xBB]);
    }

    #[test]
    fn every_named_id_roundtrips_through_cobs_and_parse() {
        // Integration of the two codecs: what goes on the wire comes back.
        for id in 5000u16..5200 {
            if message_name(id).is_none() {
                continue;
            }
            let m = Message::new(id, vec![0, 1, 0, 2]);
            let wire = crate::cobs::encode_frame(&m.encode());
            let mut dec = crate::cobs::FrameDecoder::new();
            let frames = dec.push(&wire);
            assert_eq!(frames.len(), 1);
            assert_eq!(Message::parse(frames[0].as_ref().unwrap()).unwrap(), m);
        }
    }
}
