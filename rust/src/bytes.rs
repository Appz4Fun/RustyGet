//! Pure-Rust byte searches.

/// The first occurrence of `needle` in `hay` (an empty needle is at 0).
pub fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let Some((&first, rest)) = needle.split_first() else {
        return Some(0);
    };
    if needle.len() > hay.len() {
        return None;
    }
    let end = hay.len() - rest.len();
    let mut pos = 0;
    while pos < end {
        let off = find_byte(&hay[pos..end], first)?;
        let idx = pos + off;
        if &hay[idx + 1..idx + 1 + rest.len()] == rest {
            return Some(idx);
        }
        pos = idx + 1;
    }
    None
}

/// The first `byte` in `hay`.
pub fn find_byte(hay: &[u8], byte: u8) -> Option<usize> {
    hay.iter().position(|&b| b == byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_as_a_scan() {
        let naive = |h: &[u8], n: &[u8]| {
            if n.is_empty() {
                Some(0)
            } else {
                h.windows(n.len()).position(|w| w == n)
            }
        };
        let mut state = 0x2545f4914f6cdd1du64;
        for _ in 0..50_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let hay: Vec<u8> = (0..state % 40)
                .map(|i| b"ab<\0"[((state >> (i % 60)) % 4) as usize])
                .collect();
            let needle: Vec<u8> = (0..(state >> 8) % 5)
                .map(|i| b"ab<\0"[((state >> (i * 3 + 7)) % 4) as usize])
                .collect();
            assert_eq!(find(&hay, &needle), naive(&hay, &needle));
            assert_eq!(find_byte(&hay, b'<'), hay.iter().position(|&b| b == b'<'));
            assert_eq!(find_byte(&hay, 0), hay.iter().position(|&b| b == 0));
        }
    }
}
