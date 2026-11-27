//! Incremental CRC-32/ISO-HDLC as used by GPT metadata.

const TABLE: [u32; 256] = {
    let mut table = [0; 256];
    let mut index = 0;
    while index < table.len() {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = (value >> 1) ^ (0xedb8_8320 * (value & 1));
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
};

pub(super) struct Crc32(u32);

impl Crc32 {
    pub const fn new() -> Self {
        Self(u32::MAX)
    }

    pub fn update(&mut self, data: &[u8]) {
        for &byte in data {
            let index = usize::from(self.0 as u8 ^ byte);
            self.0 = (self.0 >> 8) ^ TABLE[index];
        }
    }

    pub fn finish(self) -> u32 {
        !self.0
    }
}
