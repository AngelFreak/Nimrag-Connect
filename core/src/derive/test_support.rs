//! Synthetic FIT builder for tests in this crate and in the server (hidden from docs).
//! Each message is written with its own little-endian definition on local type 0.

#[derive(Debug, Clone)]
pub enum Fv {
    U8(u8),
    U16(u16),
    U32(u32),
    S8(i8),
    S16(i16),
    S32(i32),
    /// Zero-padded to `len` bytes.
    Str(&'static str, u8),
}

impl Fv {
    fn base_type(&self) -> u8 {
        match self {
            Fv::U8(_) => 0x02,
            Fv::U16(_) => 0x84,
            Fv::U32(_) => 0x86,
            Fv::S8(_) => 0x01,
            Fv::S16(_) => 0x83,
            Fv::S32(_) => 0x85,
            Fv::Str(..) => 0x07,
        }
    }
    fn bytes(&self) -> Vec<u8> {
        match self {
            Fv::U8(x) => vec![*x],
            Fv::U16(x) => x.to_le_bytes().to_vec(),
            Fv::U32(x) => x.to_le_bytes().to_vec(),
            Fv::S8(x) => x.to_le_bytes().to_vec(),
            Fv::S16(x) => x.to_le_bytes().to_vec(),
            Fv::S32(x) => x.to_le_bytes().to_vec(),
            Fv::Str(s, len) => {
                let mut b = s.as_bytes().to_vec();
                b.resize(*len as usize, 0);
                b
            }
        }
    }
}

#[derive(Default)]
pub struct FitBuilder {
    records: Vec<u8>,
}

impl FitBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one data message: `fields` are `(field number, value)`.
    pub fn message(mut self, global: u16, fields: &[(u8, Fv)]) -> Self {
        self.records.extend_from_slice(&[0x40, 0x00, 0x00]);
        self.records.extend_from_slice(&global.to_le_bytes());
        self.records.push(fields.len() as u8);
        for (num, v) in fields {
            self.records
                .extend_from_slice(&[*num, v.bytes().len() as u8, v.base_type()]);
        }
        self.records.push(0x00);
        for (_, v) in fields {
            self.records.extend_from_slice(&v.bytes());
        }
        self
    }

    /// 12-byte header, records, CRC-16/ARC trailer.
    pub fn build(self) -> Vec<u8> {
        let mut file = vec![12, 0x10, 0x34, 0x08];
        file.extend_from_slice(&(self.records.len() as u32).to_le_bytes());
        file.extend_from_slice(b".FIT");
        file.extend_from_slice(&self.records);
        let crc = crate::crc::checksum(&file);
        file.extend_from_slice(&crc.to_le_bytes());
        file
    }
}
