//! Par2 filename parsing and collection matching, as par-rs does it
//! (`par_rs::filenames`), in the shapes RustyGet's callers use:
//! - `parse_par_filename`: `Some((base_name_len, block_count))` for a `.par2` file name
//!   (`block_count` is `0` for a base `.par2` index file, `> 0` for `.volNN+BB.par2`, and
//!   `-1` for a `.vol` name with a `-` and no `+`).
//! - `same_par_collection`: whether two filenames belong to the same `.par2` set.
//! - `find_main_pars`: one `.par2` file per set in a directory.

use par_rs::filenames::{ascii_lower, NAME_CAPACITY};

/// The longest name read; the rest is ignored.
pub const BSTRING_MAX: usize = NAME_CAPACITY - 1;

/// Returns `Some((base_name_len, block_count))` if `name` is a `.par2` filename.
/// When `confirmed` is false, everything after the last ".par2" is cut off first.
pub fn parse_par_filename(name: &[u8], confirmed: bool) -> Option<(usize, i32)> {
    par_rs::filenames::parse_par_filename(name, confirmed, ascii_lower)
        .map(|parsed| (parsed.base_len, parsed.blocks))
}

/// Whether `name1` and `name2` are `.par2` filenames with the same ASCII
/// case-insensitive base name.
pub fn same_par_collection(name1: &[u8], name2: &[u8], confirmed: bool) -> bool {
    par_rs::filenames::same_par_collection(name1, name2, confirmed)
}

/// Scans `dir_path` for `.par2` files, one per par collection, in directory
/// iteration order. When `collect` is false, returns `true` as soon as a
/// `.par2` file is found. When `collect` is true, calls `on_file` with each
/// collection's first `.par2` filename and returns `true` if there was one.
pub fn find_main_pars<G>(dir_path: &std::path::Path, collect: bool, mut on_file: G) -> bool
where
    G: FnMut(&[u8]),
{
    let Ok(entries) = std::fs::read_dir(dir_path) else {
        return false;
    };
    let names: Vec<Vec<u8>> = entries
        .flatten()
        .map(|entry| par_rs::filenames::os_str_bytes(&entry.file_name()))
        .filter(|name| !name.contains(&0))
        .collect();
    let mains = par_rs::filenames::find_main_pars(names.iter().map(Vec::as_slice));
    if collect {
        for name in &mains {
            on_file(name);
        }
    }
    !mains.is_empty()
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
    }

    #[test]
    fn main_pars_one_per_set() {
        let dir = std::env::temp_dir().join(format!("parparser-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a.par2", "a.vol00+01.par2", "b.vol00+02.PAR2", "c.rar"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        let mut found: Vec<Vec<u8>> = Vec::new();
        assert!(find_main_pars(&dir, true, |name| found.push(name.to_vec())));
        found.sort();
        assert_eq!(found.len(), 2);
        assert!(found[0].starts_with(b"a."));
        assert!(found[1].starts_with(b"b."));
        assert!(find_main_pars(&dir, false, |_| panic!("not collecting")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
