//! CRC-16/ARC (reflected polynomial 0x8005, initial value 0), the checksum
//! used both by GFDI messages and by FIT files.
//!
//! Computed a nibble at a time, low nibble first; the 16-entry table is the
//! standard reflected-0x8005 nibble table. Written from the algorithm
//! description in docs/protocol/PROTOCOL.md, not ported from any implementation.

const NIBBLE_TABLE: [u16; 16] = [
    0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00, 0x7800, 0xB401,
    0x5000, 0x9C01, 0x8801, 0x4400,
];

/// Feed one byte into a running CRC.
pub fn update_byte(crc: u16, byte: u8) -> u16 {
    let mut crc = crc;
    crc = (crc >> 4) ^ NIBBLE_TABLE[(crc & 0xF) as usize] ^ NIBBLE_TABLE[(byte & 0xF) as usize];
    crc = (crc >> 4) ^ NIBBLE_TABLE[(crc & 0xF) as usize] ^ NIBBLE_TABLE[(byte >> 4) as usize];
    crc
}

/// Continue a running CRC over `data`, starting from `seed`.
pub fn update(seed: u16, data: &[u8]) -> u16 {
    data.iter().fold(seed, |crc, &b| update_byte(crc, b))
}

/// CRC of `data` from a zero seed.
pub fn checksum(data: &[u8]) -> u16 {
    update(0, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_check_value_for_crc16_arc() {
        assert_eq!(checksum(b"123456789"), 0xBB3D);
    }

    #[test]
    fn empty_input_is_zero() {
        assert_eq!(checksum(&[]), 0);
    }

    #[test]
    fn running_crc_equals_crc_of_concatenation() {
        let first = update(0, b"12345");
        assert_eq!(update(first, b"6789"), checksum(b"123456789"));
    }

    #[test]
    fn matches_bitwise_reference_on_all_single_bytes() {
        // Independent bit-by-bit CRC-16/ARC as a cross-check of the nibble table.
        fn bitwise(data: &[u8]) -> u16 {
            let mut crc = 0u16;
            for &b in data {
                crc ^= b as u16;
                for _ in 0..8 {
                    crc = if crc & 1 != 0 {
                        (crc >> 1) ^ 0xA001
                    } else {
                        crc >> 1
                    };
                }
            }
            crc
        }
        for b in 0..=255u8 {
            assert_eq!(
                checksum(&[b, b.wrapping_mul(7)]),
                bitwise(&[b, b.wrapping_mul(7)])
            );
        }
    }
}
