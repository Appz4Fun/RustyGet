//! Finds where the yEnc body's fast path must stop: the next `\r\n.`,
//! `\r\n=`, `=\r` or `=\n`. Everything before it can go straight to the SIMD
//! body kernel. This scan touches every byte of every article, so it has
//! vector versions chosen once at runtime (AVX-512BW, AVX2, NEON) and the
//! scalar loop as the reference and fallback.

/// The first `i` in `from..max_end` where a break starts, else `max_end`.
/// `s` must hold `max_end + 2` bytes (a break is up to three bytes long, and
/// the scan looks at the two after its start); a shorter one panics.
#[inline]
pub(super) fn find_break(s: &[u8], from: usize, max_end: usize) -> usize {
    assert!(max_end + 2 <= s.len(), "find_break past the end of its buffer");
    // SAFETY: the assert above keeps every read inside `s`.
    unsafe { imp::find_break(s.as_ptr(), from, max_end) }
}

/// The reference loop. The vector versions return exactly what it returns.
///
/// # Safety
/// As [`find_break`].
pub(super) unsafe fn find_break_scalar(s: *const u8, from: usize, max_end: usize) -> usize {
    let mut i = from;
    while i < max_end {
        let b = *s.add(i);
        if b == b'\r' {
            if *s.add(i + 1) == b'\n' {
                let b2 = *s.add(i + 2);
                if b2 == b'.' || b2 == b'=' {
                    return i;
                }
            }
        } else if b == b'=' {
            let b1 = *s.add(i + 1);
            if b1 == b'\r' || b1 == b'\n' {
                return i;
            }
        }
        i += 1;
    }
    max_end
}

#[cfg(target_arch = "x86_64")]
pub(super) mod imp {
    use super::find_break_scalar;
    use std::arch::x86_64::*;
    use std::sync::atomic::{AtomicU8, Ordering};

    /// 0 = not chosen yet, 1 = scalar, 2 = AVX2, 3 = AVX-512BW.
    static CHOSEN: AtomicU8 = AtomicU8::new(0);

    pub(crate) fn kernel() -> u8 {
        match CHOSEN.load(Ordering::Relaxed) {
            0 => {
                // RUSTYGET_SIMD caps the kernel ("scalar", "avx2"): for
                // benchmarks and for ruling a kernel out on a misbehaving host.
                let cap = match std::env::var("RUSTYGET_SIMD").as_deref() {
                    Ok("scalar") => 1,
                    Ok("avx2") => 2,
                    _ => 3,
                };
                let k = if cap >= 3 && is_x86_feature_detected!("avx512bw") {
                    3
                } else if cap >= 2 && is_x86_feature_detected!("avx2") {
                    2
                } else {
                    1
                };
                CHOSEN.store(k, Ordering::Relaxed);
                k
            }
            k => k,
        }
    }

    #[inline]
    pub(super) unsafe fn find_break(s: *const u8, from: usize, max_end: usize) -> usize {
        match kernel() {
            3 => find_break_avx512(s, from, max_end),
            2 => find_break_avx2(s, from, max_end),
            _ => find_break_scalar(s, from, max_end),
        }
    }

    /// # Safety
    /// As `find_break`, and the CPU must support AVX2.
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn find_break_avx2(s: *const u8, from: usize, max_end: usize) -> usize {
        let cr = _mm256_set1_epi8(b'\r' as i8);
        let lf = _mm256_set1_epi8(b'\n' as i8);
        let eq = _mm256_set1_epi8(b'=' as i8);
        let dot = _mm256_set1_epi8(b'.' as i8);
        let mut i = from;
        // The block at i reads bytes i..i+34, inside max_end + 2.
        while i + 32 <= max_end {
            let a = _mm256_loadu_si256(s.add(i).cast());
            let b = _mm256_loadu_si256(s.add(i + 1).cast());
            let c = _mm256_loadu_si256(s.add(i + 2).cast());
            let b_lf = _mm256_cmpeq_epi8(b, lf);
            let crlf = _mm256_and_si256(_mm256_cmpeq_epi8(a, cr), b_lf);
            let c_dot_eq = _mm256_or_si256(_mm256_cmpeq_epi8(c, dot), _mm256_cmpeq_epi8(c, eq));
            let eq_nl = _mm256_and_si256(
                _mm256_cmpeq_epi8(a, eq),
                _mm256_or_si256(_mm256_cmpeq_epi8(b, cr), b_lf),
            );
            let hit = _mm256_or_si256(_mm256_and_si256(crlf, c_dot_eq), eq_nl);
            let mask = _mm256_movemask_epi8(hit) as u32;
            if mask != 0 {
                return i + mask.trailing_zeros() as usize;
            }
            i += 32;
        }
        find_break_scalar(s, i, max_end)
    }

    /// # Safety
    /// As `find_break`, and the CPU must support AVX-512BW.
    #[target_feature(enable = "avx512bw")]
    pub(crate) unsafe fn find_break_avx512(s: *const u8, from: usize, max_end: usize) -> usize {
        let cr = _mm512_set1_epi8(b'\r' as i8);
        let lf = _mm512_set1_epi8(b'\n' as i8);
        let eq = _mm512_set1_epi8(b'=' as i8);
        let dot = _mm512_set1_epi8(b'.' as i8);
        let mut i = from;
        // The block at i reads bytes i..i+66, inside max_end + 2.
        while i + 64 <= max_end {
            let a = _mm512_loadu_si512(s.add(i).cast());
            let b = _mm512_loadu_si512(s.add(i + 1).cast());
            let c = _mm512_loadu_si512(s.add(i + 2).cast());
            let b_lf = _mm512_cmpeq_epi8_mask(b, lf);
            let crlf = _mm512_cmpeq_epi8_mask(a, cr) & b_lf;
            let c_dot_eq = _mm512_cmpeq_epi8_mask(c, dot) | _mm512_cmpeq_epi8_mask(c, eq);
            let eq_nl = _mm512_cmpeq_epi8_mask(a, eq) & (_mm512_cmpeq_epi8_mask(b, cr) | b_lf);
            let mask = (crlf & c_dot_eq) | eq_nl;
            if mask != 0 {
                return i + mask.trailing_zeros() as usize;
            }
            i += 64;
        }
        find_break_avx2(s, i, max_end)
    }
}

#[cfg(target_arch = "aarch64")]
pub(super) mod imp {
    use super::find_break_scalar;
    use std::arch::aarch64::*;

    pub(crate) fn kernel() -> u8 {
        4
    }

    #[inline]
    pub(super) unsafe fn find_break(s: *const u8, from: usize, max_end: usize) -> usize {
        find_break_neon(s, from, max_end)
    }

    /// NEON is part of the aarch64 baseline. 16-byte blocks, narrowed to a
    /// 4-bit-per-byte mask (the usual movemask substitute on ARM).
    ///
    /// # Safety
    /// As `find_break`.
    pub(crate) unsafe fn find_break_neon(s: *const u8, from: usize, max_end: usize) -> usize {
        let cr = vdupq_n_u8(b'\r');
        let lf = vdupq_n_u8(b'\n');
        let eq = vdupq_n_u8(b'=');
        let dot = vdupq_n_u8(b'.');
        let mut i = from;
        // Two blocks per pass: the block at i reads bytes i..i+34.
        while i + 32 <= max_end {
            let hit0 = block(s.add(i), cr, lf, eq, dot);
            let hit1 = block(s.add(i + 16), cr, lf, eq, dot);
            if vmaxvq_u8(vorrq_u8(hit0, hit1)) != 0 {
                let m0 = nibble_mask(hit0);
                if m0 != 0 {
                    return i + (m0.trailing_zeros() / 4) as usize;
                }
                return i + 16 + (nibble_mask(hit1).trailing_zeros() / 4) as usize;
            }
            i += 32;
        }
        while i + 16 <= max_end {
            let m = nibble_mask(block(s.add(i), cr, lf, eq, dot));
            if m != 0 {
                return i + (m.trailing_zeros() / 4) as usize;
            }
            i += 16;
        }
        find_break_scalar(s, i, max_end)
    }

    #[inline(always)]
    unsafe fn block(
        p: *const u8,
        cr: uint8x16_t,
        lf: uint8x16_t,
        eq: uint8x16_t,
        dot: uint8x16_t,
    ) -> uint8x16_t {
        let a = vld1q_u8(p);
        let b = vld1q_u8(p.add(1));
        let c = vld1q_u8(p.add(2));
        let b_lf = vceqq_u8(b, lf);
        let crlf = vandq_u8(vceqq_u8(a, cr), b_lf);
        let c_dot_eq = vorrq_u8(vceqq_u8(c, dot), vceqq_u8(c, eq));
        let eq_nl = vandq_u8(vceqq_u8(a, eq), vorrq_u8(vceqq_u8(b, cr), b_lf));
        vorrq_u8(vandq_u8(crlf, c_dot_eq), eq_nl)
    }

    #[inline(always)]
    unsafe fn nibble_mask(hit: uint8x16_t) -> u64 {
        let narrowed = vshrn_n_u16::<4>(vreinterpretq_u16_u8(hit));
        vget_lane_u64::<0>(vreinterpret_u64_u8(narrowed))
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub(super) mod imp {
    pub(crate) fn kernel() -> u8 {
        1
    }

    #[inline]
    pub(super) unsafe fn find_break(s: *const u8, from: usize, max_end: usize) -> usize {
        super::find_break_scalar(s, from, max_end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every available version against the scalar reference.
    fn versions() -> Vec<(&'static str, unsafe fn(*const u8, usize, usize) -> usize)> {
        let mut v: Vec<(&'static str, unsafe fn(*const u8, usize, usize) -> usize)> =
            vec![("dispatch", imp::find_break)];
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx2") {
                v.push(("avx2", imp::find_break_avx2));
            }
            if is_x86_feature_detected!("avx512bw") {
                v.push(("avx512bw", imp::find_break_avx512));
            }
        }
        #[cfg(target_arch = "aarch64")]
        v.push(("neon", imp::find_break_neon));
        v
    }

    #[test]
    fn matches_scalar_on_dense_and_sparse_breaks() {
        // Alphabets heavy in the bytes that matter, so every pattern and
        // near-miss lands at every lane position and block boundary.
        let alphabets: [&[u8]; 4] = [b"\r\n.=a", b"\r\n=", b"abc\r", b"\r\n.=xyzqwertyuiop"];
        let mut x = 0x2545f4914f6cdd1du64;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for alpha in alphabets {
            for len in (3..300).chain([1000, 4099]) {
                let buf: Vec<u8> = (0..len)
                    .map(|_| alpha[(next() % alpha.len() as u64) as usize])
                    .collect();
                let max_end = len - 2;
                for from in [0, 1, 2, 15, 31, 33, 63, 65]
                    .into_iter()
                    .filter(|&f| f <= max_end)
                {
                    let want = unsafe { find_break_scalar(buf.as_ptr(), from, max_end) };
                    for (name, f) in versions() {
                        let got = unsafe { f(buf.as_ptr(), from, max_end) };
                        assert_eq!(got, want, "{name} len {len} from {from}");
                    }
                }
            }
        }
    }

    #[test]
    fn each_pattern_at_each_offset() {
        let patterns: [&[u8]; 6] = [b"\r\n.", b"\r\n=", b"=\r", b"=\n", b"\r\nx", b"=y"];
        for p in patterns {
            for at in 0..140 {
                let mut buf = vec![b'a'; 160];
                buf[at..at + p.len()].copy_from_slice(p);
                let max_end = buf.len() - 2;
                let want = unsafe { find_break_scalar(buf.as_ptr(), 0, max_end) };
                for (name, f) in versions() {
                    let got = unsafe { f(buf.as_ptr(), 0, max_end) };
                    assert_eq!(got, want, "{name} {p:?} at {at}");
                }
            }
        }
    }
}
