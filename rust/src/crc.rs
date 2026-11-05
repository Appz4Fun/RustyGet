//! CRC-32 combination (Crc32::Combine): the CRC of A followed by B from the
//! CRCs of A and B and the length of B.
//!
//! The C++ code used zlib's original GF(2) matrix method, which squares 32x32
//! bit matrices about twice per bit of the length. This uses the method of
//! later zlib versions (1.2.12 on): multiplication modulo the CRC polynomial
//! with a table of x^(2^k), giving the same results with far less work.

/// The reflected CRC-32 polynomial.
const POLY: u32 = 0xedb8_8320;

const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

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
        let mut c = self.state;
        for &b in data {
            c = CRC32_TABLE[((c as u8) ^ b) as usize] ^ (c >> 8);
        }
        self.state = c;
    }

    pub const fn finish(&self) -> u32 {
        !self.state
    }
}

/// a * b modulo POLY (bit-reflected: bit 31 is x^0).
const fn mult_mod_p(a: u32, mut b: u32) -> u32 {
    let mut m: u32 = 1 << 31;
    let mut p = 0;
    loop {
        if a & m != 0 {
            p ^= b;
            if a & (m - 1) == 0 {
                break;
            }
        }
        m >>= 1;
        b = if b & 1 != 0 { (b >> 1) ^ POLY } else { b >> 1 };
    }
    p
}

/// x^(2^k) modulo POLY, k = 0..32.
const X2N: [u32; 32] = {
    let mut t = [0u32; 32];
    let mut p: u32 = 1 << 30; // x^1
    t[0] = p;
    let mut k = 1;
    while k < 32 {
        p = mult_mod_p(p, p);
        t[k] = p;
        k += 1;
    }
    t
};

/// x^(n * 2^k) modulo POLY.
fn x2n_mod_p(mut n: u32, mut k: usize) -> u32 {
    let mut p: u32 = 1 << 31; // x^0
    while n != 0 {
        if n & 1 != 0 {
            p = mult_mod_p(X2N[k & 31], p);
        }
        n >>= 1;
        k += 1;
    }
    p
}

/// Crc32::Combine: the CRC of A then B, given CRC(A), CRC(B) and B's length.
/// A length of 0 returns `crc1` unchanged, as the C++ code did.
pub fn combine(crc1: u32, crc2: u32, len2: u32) -> u32 {
    if len2 == 0 {
        return crc1;
    }
    // len2 bytes = len2 * 2^3 zero bits
    mult_mod_p(x2n_mod_p(len2, 3), crc1) ^ crc2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crc32(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
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
            assert_eq!(combine(crc32(a), crc32(b), b.len() as u32), crc32(&data), "split {split}");
        }
        assert_eq!(combine(0x1234_5678, 0x9abc_def0, 0), 0x1234_5678);
    }
}
