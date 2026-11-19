//! Consistent Overhead Byte Stuffing as used on the GFDI link.
//!
//! On the wire every frame is `0x00, cobs(message), 0x00`. The decoder is a
//! streaming, sans-IO state machine: feed it bytes as they arrive (in any
//! fragmentation) and it yields complete decoded frames.

/// Errors from decoding a single COBS frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CobsError {
    /// A code byte pointed past the end of the frame.
    Truncated,
}

/// Encode `data` with COBS (no delimiters).
pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 254 + 2);
    let mut code_pos = 0;
    out.push(0); // placeholder for the first code byte
    let mut code: u8 = 1;
    for &b in data {
        if b == 0 {
            out[code_pos] = code;
            code_pos = out.len();
            out.push(0);
            code = 1;
        } else {
            out.push(b);
            code += 1;
            if code == 0xFF {
                out[code_pos] = code;
                code_pos = out.len();
                out.push(0);
                code = 1;
            }
        }
    }
    out[code_pos] = code;
    out
}

/// Encode and add the leading and trailing 0x00 delimiters used on the link.
pub fn encode_frame(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 254 + 4);
    out.push(0);
    out.extend_from_slice(&encode(data));
    out.push(0);
    out
}

/// Decode one COBS block (no delimiters).
pub fn decode(data: &[u8]) -> Result<Vec<u8>, CobsError> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let code = data[i] as usize;
        if code == 0 || i + code > data.len() {
            return Err(CobsError::Truncated);
        }
        out.extend_from_slice(&data[i + 1..i + code]);
        i += code;
        if code != 0xFF && i < data.len() {
            out.push(0);
        }
    }
    Ok(out)
}

/// Streaming frame splitter + decoder.
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes; returns every frame completed by them. Empty frames (two
    /// adjacent delimiters, which occur between frames) are skipped.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Result<Vec<u8>, CobsError>> {
        let mut frames = Vec::new();
        for &b in bytes {
            if b == 0 {
                if !self.buf.is_empty() {
                    frames.push(decode(&self.buf));
                    self.buf.clear();
                }
            } else {
                self.buf.push(b);
            }
        }
        frames
    }

    /// Bytes buffered for an unfinished frame.
    pub fn pending(&self) -> usize {
        self.buf.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(encode(&[]), vec![0x01]);
        assert_eq!(encode(&[0x00]), vec![0x01, 0x01]);
        assert_eq!(encode(&[0x00, 0x00]), vec![0x01, 0x01, 0x01]);
        assert_eq!(
            encode(&[0x11, 0x22, 0x00, 0x33]),
            vec![0x03, 0x11, 0x22, 0x02, 0x33]
        );
        assert_eq!(
            encode(&[0x11, 0x00, 0x00, 0x00]),
            vec![0x02, 0x11, 0x01, 0x01, 0x01]
        );
    }

    #[test]
    fn run_of_254_nonzero_bytes_uses_ff_code() {
        let data: Vec<u8> = (1..=254).collect();
        let enc = encode(&data);
        assert_eq!(enc[0], 0xFF);
        assert_eq!(enc.len(), 256); // 0xFF block + trailing 0x01 code
        assert_eq!(decode(&enc).unwrap(), data);
    }

    #[test]
    fn roundtrip_assorted_lengths() {
        for len in [0usize, 1, 2, 253, 254, 255, 256, 508, 600] {
            let data: Vec<u8> = (0..len).map(|i| (i % 7) as u8).collect();
            assert_eq!(decode(&encode(&data)).unwrap(), data, "len {len}");
        }
    }

    #[test]
    fn stream_decoder_handles_fragmentation_and_back_to_back_frames() {
        let mut wire = encode_frame(&[1, 0, 2]);
        wire.extend(encode_frame(&[3, 4]));
        let mut dec = FrameDecoder::new();
        let mut out = Vec::new();
        for chunk in wire.chunks(2) {
            out.extend(dec.push(chunk));
        }
        assert_eq!(out, vec![Ok(vec![1, 0, 2]), Ok(vec![3, 4])]);
        assert_eq!(dec.pending(), 0);
    }

    #[test]
    fn truncated_block_is_an_error() {
        assert_eq!(decode(&[0x05, 0x01]), Err(CobsError::Truncated));
    }
}
