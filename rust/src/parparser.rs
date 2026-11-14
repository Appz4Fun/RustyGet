//! Par2 filename parsing and collection matching:
//! - `parse_par_filename`: returns `Some((base_name_len, block_count))` for a `.par2` file name
//!   (where `block_count` is `0` for a base `.par2` index file, `> 0` for `.volNN+BB.par2`, or `-1`
//!   for `.vol-NN.par2` / `.vol+BB.par2` without a starting block count).
//! - `same_par_collection`: returns `true` when two filenames belong to the same `.par2` set.
//! - `find_main_pars`: scans a directory for unique `.par2` sets.

pub const BSTRING_MAX: usize = 1023;

/// Parses a C-style leading digit sequence as a signed 32-bit integer (skipping leading ASCII whitespace).
fn parse_leading_i32(bytes: &[u8]) -> i32 {
    let trimmed: Vec<u8> = bytes
        .iter()
        .copied()
        .skip_while(|b| b.is_ascii_whitespace())
        .collect();
    let (sign, rest) = match trimmed.first() {
        Some(b'-') => (-1i64, &trimmed[1..]),
        Some(b'+') => (1i64, &trimmed[1..]),
        _ => (1i64, &trimmed[..]),
    };
    let mut val = 0i64;
    for &b in rest.iter().take_while(|b| b.is_ascii_digit()) {
        val = val.wrapping_mul(10).wrapping_add(i64::from(b - b'0'));
    }
    (val.wrapping_mul(sign)) as i32
}

/// Returns `Some((base_name_len, block_count))` if `name` is a `.par2` filename.
/// `block_count` is `-1` for a volume numbered without a starting block offset.
pub fn parse_par_filename(name: &[u8], confirmed: bool) -> Option<(usize, i32)> {
    let max_len = name.len().min(BSTRING_MAX);
    let end_nul = name[..max_len]
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(max_len);
    let mut f: Vec<u8> = name[..end_nul].iter().map(u8::to_ascii_lowercase).collect();
    let mut len = f.len();
    if len < 6 {
        return None;
    }
    if !confirmed {
        let mut end = 0;
        let mut at = 0;
        while let Some(k) = crate::bytes::find(&f[at..len], b".par2") {
            end = at + k + 5;
            at = end;
        }
        len = end;
    }
    if len < 6 || !f[len - 5..len].eq_ignore_ascii_case(b".par2") {
        return None;
    }
    len -= 5;
    f.truncate(len);
    let mut blocks = 0;
    if let Some(dot) = f[..len].iter().rposition(|&b| b == b'.') {
        if f[dot..len].len() >= 4 && f[dot..dot + 4].eq_ignore_ascii_case(b".vol") {
            let p = &f[dot..len];
            let sep = p
                .iter()
                .position(|&b| b == b'+')
                .or_else(|| p.iter().position(|&b| b == b'-'));
            if let Some(b) = sep.filter(|&b| b < p.len()) {
                blocks = if b == 4 {
                    -1
                } else {
                    parse_leading_i32(&p[b + 1..])
                };
                len = dot;
            }
        }
    }
    Some((len, blocks))
}

/// Returns `true` when `name1` and `name2` are `.par2` filenames with the same ASCII case-insensitive base name.
pub fn same_par_collection(name1: &[u8], name2: &[u8], confirmed: bool) -> bool {
    let Some((b1, _)) = parse_par_filename(name1, confirmed) else {
        return false;
    };
    let Some((b2, _)) = parse_par_filename(name2, confirmed) else {
        return false;
    };
    b1 == b2
        && name1
            .get(..b1)
            .zip(name2.get(..b2))
            .is_some_and(|(s1, s2)| s1.eq_ignore_ascii_case(s2))
}

#[cfg(unix)]
fn os_str_to_bytes(s: &std::ffi::OsStr) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes()
}

#[cfg(not(unix))]
fn os_str_to_bytes(s: &std::ffi::OsStr) -> &[u8] {
    s.to_str().map(str::as_bytes).unwrap_or(b"")
}

/// Scans `dir_path` for confirmed `.par2` files, deduplicating by par collection base name
/// in directory iteration order. When `collect` is false, stops and returns `true` as soon as
/// the first `.par2` file is found. When `collect` is true, invokes `on_file` for each distinct
/// collection's first `.par2` filename and returns `true` if at least one was found.
pub fn find_main_pars<G>(dir_path: &std::path::Path, collect: bool, mut on_file: G) -> bool
where
    G: FnMut(&[u8]),
{
    let Ok(entries) = std::fs::read_dir(dir_path) else {
        return false;
    };
    let mut found: Vec<Vec<u8>> = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name_bytes = os_str_to_bytes(&file_name);
        if name_bytes.contains(&0) {
            continue;
        }
        if parse_par_filename(name_bytes, true).is_some() {
            if !collect {
                return true;
            }
            if !found
                .iter()
                .any(|existing| same_par_collection(name_bytes, existing, true))
            {
                on_file(name_bytes);
                found.push(name_bytes.to_vec());
            }
        }
    }
    !found.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(parse_par_filename(b"Movie.par2", true), Some((5, 0)));
        assert_eq!(
            parse_par_filename(b"Movie.vol07+08.PAR2", true),
            Some((5, 8))
        );
        assert_eq!(
            parse_par_filename(b"Movie.vol-06.par2", true),
            Some((5, -1))
        );
        assert_eq!(parse_par_filename(b"Movie.par2.1", true), None);
        assert_eq!(parse_par_filename(b"Movie.par2.1", false), Some((5, 0)));
        assert_eq!(parse_par_filename(b".par2", true), None);
        assert!(same_par_collection(b"A.vol00+01.par2", b"a.par2", true));
        assert!(!same_par_collection(b"A.par2", b"B.par2", true));
        assert_eq!(parse_par_filename(b"a\xffpar2", false), None);
        assert_eq!(
            parse_par_filename(b"a.par2\0ignored.par2", true),
            Some((1, 0))
        );
    }
}
