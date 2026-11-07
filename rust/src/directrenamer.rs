//! End-to-end Rust implementation of `DirectRenamer` (`daemon/queue/DirectRenamer.cpp`):
//! regular and par2 file renaming (`regular_name`, `par_name`, `par_name_formatted`),
//! par2 set rename check (`need_rename_pars`), article hash16k / par-flag delta
//! evaluation (`eval_direct_rename_article`), smallest-par-per-set selection
//! (`select_pars_to_unpause`), and state-check helpers.

use std::ffi::CStr;

/// `BuildNewRegularName`: the par2 file name the first par2 hash equal to
/// `hash16k` names, if that differs from `old_name` in more than case.
/// `hashes` yields `(filename, hash)` in list order; the result is the
/// hash's index.
pub fn regular_name<'a>(
    old_name: &CStr,
    hash16k: &CStr,
    hashes: impl Iterator<Item = (&'a CStr, &'a CStr)>,
) -> Option<usize> {
    if hash16k.is_empty() {
        return None;
    }
    let (index, (filename, _)) = hashes.enumerate().find(|(_, (_, hash))| *hash == hash16k)?;
    (!old_name.to_bytes().eq_ignore_ascii_case(filename.to_bytes())).then_some(index)
}

/// `ostream << setfill('0') << setw(3) << vol`: padded on the left, also
/// before a minus sign.
fn push_vol(out: &mut Vec<u8>, vol: i32) {
    let digits = vol.to_string();
    out.extend(std::iter::repeat_n(
        b'0',
        3usize.saturating_sub(digits.len()),
    ));
    out.extend_from_slice(digits.as_bytes());
}

/// `BuildNewParName`: `set_id` plus the old `".volNN+NN.par2"` suffix when that
/// is free in `dest_dir`, else the next free `"<set_id>.volNNN+01.PAR2"`,
/// counting `vol` up for every name tried.
pub fn par_name(
    old_name: &[u8],
    dest_dir: &[u8],
    set_id: &[u8],
    separator: u8,
    vol: &mut i32,
    exists: &mut dyn FnMut(&[u8]) -> bool,
) -> Vec<u8> {
    par_name_formatted(old_name, dest_dir, set_id, separator, vol, exists, &mut |vol| {
        let mut name = set_id.to_vec();
        name.extend_from_slice(b".vol");
        push_vol(&mut name, vol);
        name.extend_from_slice(b"+01.PAR2");
        Some(name)
    })
}

/// `BuildNewParName` checking candidate file existence directly on disk.
pub fn par_name_on_disk(
    old_name: &[u8],
    dest_dir: &[u8],
    set_id: &[u8],
    separator: u8,
    vol: &mut i32,
) -> Vec<u8> {
    let mut exists = |path: &[u8]| {
        #[cfg(unix)]
        let p = {
            use std::os::unix::ffi::OsStrExt;
            std::path::Path::new(std::ffi::OsStr::from_bytes(path))
        };
        #[cfg(not(unix))]
        let p = std::path::Path::new(std::str::from_utf8(path).unwrap_or(""));
        p.is_file()
    };
    par_name(old_name, dest_dir, set_id, separator, vol, &mut exists)
}

/// A non-classic C++ locale may group digits or install a custom `num_put`
/// facet. Let that stream format candidates; `None` stops after an exception.
pub fn par_name_formatted(
    old_name: &[u8],
    dest_dir: &[u8],
    set_id: &[u8],
    separator: u8,
    vol: &mut i32,
    exists: &mut dyn FnMut(&[u8]) -> bool,
    format: &mut dyn FnMut(i32) -> Option<Vec<u8>>,
) -> Vec<u8> {
    let full = |name: &[u8]| [dest_dir, &[separator], name].concat();
    let suffix = old_name
        .windows(4)
        .position(|w| w == b".vol")
        .map(|i| &old_name[i..]);
    if let Some(suffix) = suffix {
        let extension = &suffix[suffix.iter().rposition(|&b| b == b'.').unwrap_or(0)..];
        let ext_end = extension.iter().position(|&b| b == 0).unwrap_or(extension.len());
        if extension[..ext_end].eq_ignore_ascii_case(b".par2") {
            let name = [set_id, suffix].concat();
            if !exists(&full(&name)) {
                return name;
            }
        }
    }
    loop {
        let Some(name) = format(*vol) else {
            return Vec::new();
        };
        *vol = vol.wrapping_add(1);
        if !exists(&full(&name)) {
            return name;
        }
    }
}

/// `NeedRenamePars`: a par2 file without the `".par2"` extension (any case), or
/// two par2 files of one set that `ParParser::SameParCollection` does not put
/// in the same collection.
pub fn need_rename_pars(pars: &[(Option<&CStr>, &CStr)]) -> bool {
    pars.iter().enumerate().any(|(i, &(name, set_id))| {
        let Some(name) = name else { return true };
        !crate::util::ends_with(name.to_bytes(), b".par2", false)
            || pars[i + 1..].iter().any(|&(name2, set_id2)| {
                set_id == set_id2
                    && name2.is_none_or(|name2| {
                        !crate::parparser::same_par_collection(
                            name.to_bytes(),
                            name2.to_bytes(),
                            false,
                        )
                    })
            })
    })
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsDirectRenameParDelta {
    pub apply_hash16k: i32,
    pub par_flag_changed: i32,
    pub par_size: i64,
    pub par_current_success_size: i64,
    pub par_current_failed_size: i64,
    pub par_failed_size: i64,
    pub remaining_par_count: i32,
    pub unpause_non_par: i32,
}

/// Evaluates `DirectRenamer::ArticleDownloaded` hash16k eligibility and par-flag transition deltas.
#[allow(clippy::too_many_arguments)]
pub fn eval_direct_rename_article(
    article_size: i32,
    article_count: usize,
    old_par_file: bool,
    new_par_file: bool,
    file_paused: bool,
    file_size: i64,
    file_success_size: i64,
    file_failed_size: i64,
    file_missed_size: i64,
    par_size: i64,
    par_current_success_size: i64,
    par_current_failed_size: i64,
    par_failed_size: i64,
    remaining_par_count: i32,
) -> RsDirectRenameParDelta {
    let apply_hash16k = if article_size >= 16 * 1024 || article_count == 1 {
        1
    } else {
        0
    };
    if old_par_file == new_par_file {
        return RsDirectRenameParDelta {
            apply_hash16k,
            par_flag_changed: 0,
            par_size,
            par_current_success_size,
            par_current_failed_size,
            par_failed_size,
            remaining_par_count,
            unpause_non_par: 0,
        };
    }

    let delta: i64 = if new_par_file { 1 } else { -1 };
    let count_delta: i32 = if new_par_file { 1 } else { -1 };
    let unpause_non_par = if !new_par_file && file_paused { 1 } else { 0 };

    RsDirectRenameParDelta {
        apply_hash16k,
        par_flag_changed: 1,
        par_size: par_size + file_size * delta,
        par_current_success_size: par_current_success_size + file_success_size * delta,
        par_current_failed_size: par_current_failed_size
            + file_failed_size * delta
            + file_missed_size * delta,
        par_failed_size: par_failed_size + file_missed_size * delta,
        remaining_par_count: remaining_par_count + count_delta,
        unpause_non_par,
    }
}

/// Selects the smallest uncompleted par-file ID per par-set in `DirectRenamer::UnpausePars`.
/// Returns the number of selected file IDs written to `out_file_ids` (in exact `DirectRenamer::UnpausePars` order).
pub fn select_pars_to_unpause(
    ids: &[i32],
    set_ids: &[&[u8]],
    completed: &[u8],
    in_file_list: &[u8],
    sizes: &[i64],
    out_file_ids: &mut [i32],
) -> usize {
    let count = ids
        .len()
        .min(set_ids.len())
        .min(completed.len())
        .min(in_file_list.len())
        .min(sizes.len());
    if count == 0 || out_file_ids.is_empty() {
        return 0;
    }

    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&a, &b| {
        let f1 = in_file_list[a] != 0;
        let f2 = in_file_list[b] != 0;
        if (!f1 && f2) || (f1 && f2 && sizes[a] < sizes[b]) {
            std::cmp::Ordering::Less
        } else if (f1 && !f2) || (f1 && f2 && sizes[b] < sizes[a]) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });

    let mut parsets: Vec<&[u8]> = Vec::new();
    for &idx in &order {
        if completed[idx] != 0 {
            parsets.push(set_ids[idx]);
        }
    }

    let mut written = 0;
    for &idx in &order {
        if !parsets.iter().any(|&s| s == set_ids[idx]) {
            parsets.push(set_ids[idx]);
            if in_file_list[idx] != 0 && written < out_file_ids.len() {
                out_file_ids[written] = ids[idx];
                written += 1;
            }
        }
    }
    written
}

/// Resolves the old output path in `DirectRenamer::RenameFilesInProgress`.
pub fn resolve_old_output_path(
    curr_output_filename: &[u8],
    dest_dir: &[u8],
    sep: u8,
    filename: &[u8],
) -> Vec<u8> {
    if !curr_output_filename.is_empty() {
        return curr_output_filename.to_vec();
    }
    let mut out = Vec::with_capacity(dest_dir.len() + 1 + filename.len());
    out.extend_from_slice(dest_dir);
    out.push(sep);
    out.extend_from_slice(filename);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_name_takes_the_first_matching_hash() {
        let hashes = [(c"a.mkv", c"H1"), (c"b.mkv", c"H2"), (c"c.mkv", c"H2")];
        let find = |old: &CStr, hash: &CStr| regular_name(old, hash, hashes.iter().copied());
        assert_eq!(find(c"x.bin", c"H2"), Some(1));
        assert_eq!(find(c"B.MKV", c"H2"), None);
        assert_eq!(find(c"x.bin", c""), None);
        assert_eq!(find(c"x.bin", c"H3"), None);
    }

    #[test]
    fn par_name_reuses_the_suffix_or_counts_volumes() {
        let mut taken = vec![b"/d/S.vol01+02.par2".to_vec()];
        let mut vol = 1;
        let mut exists = |p: &[u8]| taken.iter().any(|t| t == p);
        assert_eq!(
            par_name(
                b"x.vol01+02.PaR2",
                b"/d",
                b"T",
                b'/',
                &mut vol,
                &mut exists
            ),
            b"T.vol01+02.PaR2"
        );
        assert_eq!(vol, 1);
        assert_eq!(
            par_name(
                b"x.vol01+02.par2",
                b"/d",
                b"S",
                b'/',
                &mut vol,
                &mut exists
            ),
            b"S.vol001+01.PAR2"
        );
        assert_eq!(vol, 2);
        taken.push(b"/d/S.vol002+01.PAR2".to_vec());
        let mut exists = |p: &[u8]| taken.iter().any(|t| t == p);
        assert_eq!(
            par_name(b"x.par2", b"/d", b"S", b'/', &mut vol, &mut exists),
            b"S.vol003+01.PAR2"
        );
        assert_eq!(vol, 4);
        assert_eq!(
            par_name(
                b"a.vol.b.vol1.par2",
                b"",
                b"S",
                b'/',
                &mut vol,
                &mut |_| false
            ),
            b"S.vol.b.vol1.par2"
        );
        assert_eq!(
            par_name(
                b"a.vol1.par2.x",
                b"",
                b"S",
                b'/',
                &mut vol,
                &mut |_| false
            ),
            b"S.vol004+01.PAR2"
        );
        let mut vol = -5;
        assert_eq!(
            par_name(b"", b"", b"S", b'/', &mut vol, &mut |_| false),
            b"S.vol0-5+01.PAR2"
        );
        let mut vol = 1234;
        assert_eq!(
            par_name(b"", b"", b"S", b'/', &mut vol, &mut |_| false),
            b"S.vol1234+01.PAR2"
        );
        let mut vol = i32::MAX;
        par_name(b"", b"", b"S", b'/', &mut vol, &mut |_| false);
        assert_eq!(vol, i32::MIN);
    }

    #[test]
    fn need_rename_pars_checks_extensions_and_sets() {
        let need = |pars: &[(Option<&CStr>, &CStr)]| need_rename_pars(pars);
        assert!(!need(&[]));
        assert!(!need(&[
            (Some(c"a.PAR2"), c"S"),
            (Some(c"A.vol1+2.par2"), c"S"),
            (Some(c"b.par2"), c"T")
        ]));
        assert!(need(&[
            (Some(c"a.par2"), c"S"),
            (Some(c"b.vol1+2.par2"), c"S")
        ]));
        assert!(need(&[(Some(c"a.par"), c"S")]));
        assert!(need(&[(None, c"S")]));
        assert!(need(&[(Some(c"a.par2"), c"S"), (None, c"S")]));
        assert!(need(&[(Some(c".par2"), c"S"), (Some(c".par2"), c"S")]));
        assert!(!need(&[(Some(c".par2"), c"S"), (Some(c".par2"), c"T")]));
    }
}
