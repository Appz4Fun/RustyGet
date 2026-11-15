//! CRC-32 combination (Crc32::Combine): the CRC of A followed by B from the
//! CRCs of A and B and the length of B.
//!
//! The combination itself is par-rs's (`par_rs::crc::crc32_combine`).

/// Incremental IEEE 802.3 CRC-32 accumulator matching C++ `Crc32`.
#[derive(Debug, Clone, Copy)]
pub struct Crc32 {
    state: u32,
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    pub const fn new() -> Self {
        Self { state: !0u32 }
    }

    pub fn append(&mut self, data: &[u8]) {
        self.state = !crc32_append(data, !self.state);
    }

    pub const fn finish(&self) -> u32 {
        !self.state
    }
}

/// Hardware-accelerated IEEE 802.3 CRC-32 update matching `rapidyenc_crc`.
pub fn crc32_append(data: &[u8], init: u32) -> u32 {
    let mut hasher = crc32fast::Hasher::new_with_initial(init);
    hasher.update(data);
    hasher.finalize()
}

/// Crc32::Combine: the CRC of A then B, given CRC(A), CRC(B) and B's length
/// (par-rs does the arithmetic). A length of 0 returns `crc1` unchanged, as the
/// C++ code did.
pub fn combine(crc1: u32, crc2: u32, len2: u32) -> u32 {
    if len2 == 0 {
        return crc1;
    }
    par_rs::crc::crc32_combine(crc1, crc2, u64::from(len2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crc32(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    (c >> 1) ^ par_rs::crc::POLY
                } else {
                    c >> 1
                };
            }
        }
        !c
    }

    #[test]
    fn combines() {
        let data: Vec<u8> = (0..5000u32).map(|i| (i * 7 + i / 13) as u8).collect();
        for split in [0usize, 1, 2, 3, 100, 2499, 4999, 5000] {
            let (a, b) = data.split_at(split);
            if b.is_empty() {
                continue;
            }
            assert_eq!(
                combine(crc32(a), crc32(b), b.len() as u32),
                crc32(&data),
                "split {split}"
            );
        }
        assert_eq!(combine(0x1234_5678, 0x9abc_def0, 0), 0x1234_5678);
    }
}
