//! `QueueEditor` and `GroupSorter` (`daemon/queue/QueueEditor.cpp`) end-to-end Rust implementation.

use crate::queuecoord::strcasecmp_bytes;

pub const SORT_CRITERIA_NAME: i32 = 0;
pub const SORT_CRITERIA_SIZE: i32 = 1;
pub const SORT_CRITERIA_REMAINING_SIZE: i32 = 2;
pub const SORT_CRITERIA_AGE: i32 = 3;
pub const SORT_CRITERIA_CATEGORY: i32 = 4;
pub const SORT_CRITERIA_PRIORITY: i32 = 5;

pub const SORT_ORDER_ASCENDING: i32 = 0;
pub const SORT_ORDER_DESCENDING: i32 = 1;
pub const SORT_ORDER_AUTO: i32 = 2;

pub const DUPE_MODE_SCORE: i32 = 0;
pub const DUPE_MODE_ALL: i32 = 1;
pub const DUPE_MODE_FORCE: i32 = 2;

/// `GroupSorter::Execute` (`QueueEditor.cpp`):
/// Parses sort specification (`name[+-]`, `size[+-]`, `left[+-]`, `age[+-]`, `category[+-]`, `priority[+-]`).
pub fn parse_sort_spec(sort: &[u8]) -> Option<(i32, i32)> {
    if sort.is_empty() {
        return None;
    }
    let last_ch = *sort.last().unwrap();
    let (base, order) = if last_ch == b'+' {
        (&sort[..sort.len() - 1], SORT_ORDER_ASCENDING)
    } else if last_ch == b'-' {
        (&sort[..sort.len() - 1], SORT_ORDER_DESCENDING)
    } else {
        (sort, SORT_ORDER_AUTO)
    };

    let criteria = if strcasecmp_bytes(base, b"name") {
        SORT_CRITERIA_NAME
    } else if strcasecmp_bytes(base, b"size") {
        SORT_CRITERIA_SIZE
    } else if strcasecmp_bytes(base, b"left") {
        SORT_CRITERIA_REMAINING_SIZE
    } else if strcasecmp_bytes(base, b"age") {
        SORT_CRITERIA_AGE
    } else if strcasecmp_bytes(base, b"category") {
        SORT_CRITERIA_CATEGORY
    } else if strcasecmp_bytes(base, b"priority") {
        SORT_CRITERIA_PRIORITY
    } else {
        return None;
    };

    Some((criteria, order))
}

/// `GroupSorter::operator()` (`QueueEditor.cpp`) comparator.
#[allow(clippy::too_many_arguments)]
pub fn group_sorter_compare_values(
    sort_criteria: i32,
    sort_order: i32,
    so_descending: i32,
    cmp_name: i32,
    size1: i64,
    size2: i64,
    rem1: i64,
    rem2: i64,
    min_time1: i64,
    min_time2: i64,
    cmp_cat: i32,
    prio1: i32,
    prio2: i32,
) -> bool {
    let desc = sort_order == so_descending;
    let (nm, s1, s2, r1, r2, t1, t2, cat, p1, p2) = if desc {
        (
            -cmp_name, size2, size1, rem2, rem1, min_time2, min_time1, -cmp_cat, prio2, prio1,
        )
    } else {
        (
            cmp_name, size1, size2, rem1, rem2, min_time1, min_time2, cmp_cat, prio1, prio2,
        )
    };
    match sort_criteria {
        0 => nm < 0,
        1 => s1 < s2,
        2 => r1 < r2,
        3 => t1 > t2,
        4 => cat < 0,
        5 => p1 < p2,
        _ => false,
    }
}

/// `QueueEditor::MoveEntry` and `QueueEditor::MoveGroup` target index clamping.
pub fn clamp_move_target(entry: i32, offset: i32, size: i32) -> Option<i32> {
    let mut new_entry = entry.wrapping_add(offset);
    if new_entry < 0 {
        new_entry = 0;
    }
    if new_entry > size.wrapping_sub(1) {
        new_entry = size.wrapping_sub(1);
    }
    if new_entry >= 0 && new_entry <= size.wrapping_sub(1) {
        Some(new_entry)
    } else {
        None
    }
}

/// `QueueEditor::InternEditList` ID deduplication preserving first-seen order.
pub fn dedup_id_list(ids: &[i32]) -> Vec<i32> {
    let mut unique = Vec::with_capacity(ids.len());
    for &id in ids {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    unique
}

/// `QueueEditor::PrepareList` (`QueueEditor.cpp`):
/// Computes `workOffset` and updated `lastDestPos` when moving multiple items in queue order.
pub fn clamp_move_offset(
    index: i32,
    nr_entries: i32,
    offset: i32,
    last_dest_pos: i32,
) -> (i32, i32) {
    let mut work_offset = offset;
    let dest_pos = index + work_offset;
    if last_dest_pos == -1 {
        if dest_pos < 0 {
            work_offset = -index;
        } else if dest_pos > nr_entries - 1 {
            work_offset = nr_entries - 1 - index;
        }
    } else if work_offset < 0 && dest_pos <= last_dest_pos {
        work_offset = last_dest_pos - index + 1;
    } else if work_offset > 0 && dest_pos >= last_dest_pos {
        work_offset = last_dest_pos - index - 1;
    }
    let new_last_dest_pos = index + work_offset;
    (work_offset, new_last_dest_pos)
}

/// `QueueEditor::EditGroup` group deletion flags:
/// returns `(parking, avoid_history, set_dupe_status, cleanup_disk)`.
#[allow(clippy::too_many_arguments)]
pub fn eval_group_delete_flags(
    action: i32,
    ea_park_delete: i32,
    ea_dupe_delete: i32,
    ea_final_delete: i32,
    keep_history: i32,
    unpack_cleaned_up_disk: bool,
    current_success_articles: i32,
) -> (bool, bool, bool, bool) {
    let parking = action == ea_park_delete
        && keep_history > 0
        && !unpack_cleaned_up_disk
        && current_success_articles > 0;
    let avoid_history = action == ea_final_delete;
    let set_dupe_status = action == ea_dupe_delete;
    let cleanup_disk = action != ea_park_delete || !parking;
    (parking, avoid_history, set_dupe_status, cleanup_disk)
}

/// Maps a `DownloadQueue::EEditAction` group edit action to its corresponding file edit action
/// (`QueueEditor::EditGroup`).
pub fn map_group_to_file_action(action: i32) -> i32 {
    match action {
        1 | 11 => 1,                // eaFileMoveOffset / eaGroupMoveOffset -> eaFileMoveOffset
        2 | 12 => 2,                // eaFileMoveTop / eaGroupMoveTop -> eaFileMoveTop
        3 | 13 => 3,                // eaFileMoveBottom / eaGroupMoveBottom -> eaFileMoveBottom
        4 | 16 => 4,                // eaFilePause / eaGroupPause -> eaFilePause
        5 | 17 => 5,                // eaFileResume / eaGroupResume -> eaFileResume
        6 | 18 | 19 | 20 | 21 => 6, // eaFileDelete / eaGroup*Delete -> eaFileDelete
        7 | 22 => 7,                // eaFilePauseAllPars / eaGroupPauseAllPars -> eaFilePauseAllPars
        8 | 23 => 8,                // eaFilePauseExtraPars / eaGroupPauseExtraPars -> eaFilePauseExtraPars
        9 => 9,                     // eaFileReorder
        10 => 10,                   // eaFileSplit
        _ => 0,
    }
}

/// Checks case-insensitively whether `filename` contains `".vol"` for `QueueEditor::PausePars`.
pub fn filename_has_vol(filename: &[u8]) -> bool {
    filename
        .windows(4)
        .any(|w| w.eq_ignore_ascii_case(b".vol"))
}

/// `QueueEditor::PausePars` (`QueueEditor.cpp`):
/// Sets `out_pause[i] = 1` for every file in the group that `PausePars` should pause.
pub fn select_pars_to_pause(
    is_par: &[u8],
    has_vol: &[u8],
    sizes: &[i64],
    extra_pars_only: bool,
    out_pause: &mut [u8],
) {
    let n = is_par
        .len()
        .min(has_vol.len())
        .min(sizes.len())
        .min(out_pause.len());
    for p in out_pause[..n].iter_mut() {
        *p = 0;
    }
    if !extra_pars_only {
        for i in 0..n {
            if is_par[i] != 0 {
                out_pause[i] = 1;
            }
        }
        return;
    }

    let mut has_main_par = false;
    let mut vols: Vec<usize> = Vec::new();
    for i in 0..n {
        if is_par[i] != 0 {
            if has_vol[i] != 0 {
                vols.push(i);
            } else {
                has_main_par = true;
            }
        }
    }

    if has_main_par {
        for &idx in &vols {
            out_pause[idx] = 1;
        }
    } else {
        let mut smallest: Option<usize> = None;
        for &idx in &vols {
            match smallest {
                None => smallest = Some(idx),
                Some(prev) => {
                    if sizes[prev] > sizes[idx] {
                        out_pause[prev] = 1;
                        smallest = Some(idx);
                    } else {
                        out_pause[idx] = 1;
                    }
                }
            }
        }
    }
}

/// Resolves `(target_id, before, offset)` when moving groups relative to `target_id`
/// (`QueueEditor::MoveGroupsTo`).
pub fn resolve_move_groups_target(
    target_id: i32,
    queue_ids: &[i32],
    moved_ids: &[i32],
    before: bool,
    max_id: i32,
) -> (i32, bool, i32) {
    if !moved_ids.contains(&target_id) {
        return (target_id, before, 0);
    }
    if let Some(target_idx) = queue_ids.iter().position(|&id| id == target_id) {
        if before {
            for &qid in queue_ids[..=target_idx].iter().rev() {
                if !moved_ids.contains(&qid) {
                    return (qid, false, 0);
                }
            }
        } else {
            for &qid in &queue_ids[target_idx..] {
                if !moved_ids.contains(&qid) {
                    return (qid, true, 0);
                }
            }
        }
    }
    (target_id, before, if before { -max_id } else { max_id })
}

/// Computes the offset between `move_id` and `target_id` in `QueueEditor::MoveGroupsTo`.
pub fn calc_move_groups_to_offset(
    queue_ids: &[i32],
    target_id: i32,
    move_id: i32,
    before: bool,
    item_list_size: usize,
) -> i32 {
    let mut offset: i32 = 0;
    let mut progress = false;
    let mut step: i32 = 0;
    for &id in queue_ids {
        if id == target_id || id == move_id {
            if !progress {
                step = if id == target_id { -1 } else { 1 };
                let base: i32 = if before { 0 } else { 1 };
                let sub: i32 = if step > 0 { item_list_size as i32 } else { 0 };
                offset = base.wrapping_sub(sub);
                progress = true;
            } else {
                break;
            }
        }
        if progress {
            offset = offset.wrapping_add(step);
        }
    }
    offset
}

/// `QueueEditor::SortGroupFiles` (`QueueEditor.cpp`) comparator:
/// Returns `true` if `fileInfo1` should be ordered strictly before `fileInfo2`.
pub fn compare_inner_files(
    name1: &[u8],
    par1: bool,
    size1: i64,
    name2: &[u8],
    par2: bool,
    size2: i64,
    same_par_collection: bool,
) -> bool {
    if !par1 && !par2 {
        let len1 = name1.len();
        let len2 = name2.len();
        let ext1 = name1.iter().rposition(|&b| b == b'.').map(|p| &name1[p..]);
        let ext2 = name2.iter().rposition(|&b| b == b'.').map(|p| &name2[p..]);
        if let (Some(e1), Some(e2)) = (ext1, ext2) {
            if len1 == len2
                && e1.len() == 4
                && e2.len() == 4
                && name1[..len1 - 4] == name2[..len2 - 4]
            {
                if e1 == b".rar"
                    && e2[1] == b'r'
                    && e2[2].is_ascii_digit()
                    && e2[3].is_ascii_digit()
                {
                    return true;
                } else if e2 == b".rar"
                    && e1[1] == b'r'
                    && e1[2].is_ascii_digit()
                    && e1[3].is_ascii_digit()
                {
                    return false;
                }
            }
        }
    } else if par1 && par2 && same_par_collection {
        return size1 < size2;
    } else if !par1 && par2 {
        return true;
    } else if par1 && !par2 {
        return false;
    }

    name1 < name2
}

/// `QueueEditor::SetNzbDupeParam` & `HistoryCoordinator::HistorySetDupeParam`:
/// Parses `"SCORE"`, `"ALL"`, `"FORCE"` case-insensitively.
pub fn parse_dupe_mode(text: &[u8]) -> Option<i32> {
    if strcasecmp_bytes(text, b"SCORE") {
        Some(DUPE_MODE_SCORE)
    } else if strcasecmp_bytes(text, b"ALL") {
        Some(DUPE_MODE_ALL)
    } else if strcasecmp_bytes(text, b"FORCE") {
        Some(DUPE_MODE_FORCE)
    } else {
        None
    }
}

/// Panic-free stable merge sort (`left` wins ties: `!is_less(&right[j], &left[i])`).
/// Safe even when `is_less` is not a strict weak ordering (e.g. `GroupSorter` with unselected items).
pub fn stable_merge_sort_by<T: Clone, F: FnMut(&T, &T) -> bool>(
    slice: &mut [T],
    is_less: &mut F,
) {
    fn sort_rec<T: Clone, F: FnMut(&T, &T) -> bool>(
        slice: &mut [T],
        buf: &mut [T],
        is_less: &mut F,
    ) {
        let len = slice.len();
        if len <= 1 {
            return;
        }
        let mid = len / 2;
        sort_rec(&mut slice[..mid], &mut buf[..mid], is_less);
        sort_rec(&mut slice[mid..], &mut buf[mid..], is_less);
        buf[..len].clone_from_slice(slice);
        let (left, right) = buf[..len].split_at(mid);
        let (mut i, mut j, mut k) = (0, 0, 0);
        while i < left.len() && j < right.len() {
            if is_less(&right[j], &left[i]) {
                slice[k] = right[j].clone();
                j += 1;
            } else {
                slice[k] = left[i].clone();
                i += 1;
            }
            k += 1;
        }
        while i < left.len() {
            slice[k] = left[i].clone();
            i += 1;
            k += 1;
        }
        while j < right.len() {
            slice[k] = right[j].clone();
            j += 1;
            k += 1;
        }
    }
    if slice.len() > 1 {
        let mut buf = slice.to_vec();
        sort_rec(slice, &mut buf, is_less);
    }
}

#[cfg(unix)]
const MAX_ID: i32 = 1_000_000_000;

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
struct EditItem {
    file_ptr: usize,
    nzb_ptr: usize,
    offset: i32,
}

#[cfg(unix)]
fn push_bytes_msg(prefix: &[u8], a: &[u8], mid: &[u8], b: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + a.len() + mid.len() + b.len() + suffix.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(a);
    out.extend_from_slice(mid);
    out.extend_from_slice(b);
    out.extend_from_slice(suffix);
    out
}

#[cfg(unix)]
pub(crate) fn edit_entry_host(
    coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    id: i32,
    action: i32,
    args: Option<&[u8]>,
) -> bool {
    let ids = [id];
    intern_edit_list(coord, host, dq_ptr, None, Some(&ids), action, args)
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn edit_list_host(
    coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    ids: Option<&[i32]>,
    names: Option<&[&[u8]]>,
    match_mode: i32,
    action: i32,
    args: Option<&[u8]>,
) -> bool {
    use crate::queuesubsys_host::{
        EA_HISTORY_DELETE, EA_HISTORY_SET_NAME, EA_POST_DELETE, MM_REGEX,
    };

    if ids.is_none()
        && (action == EA_POST_DELETE || (EA_HISTORY_DELETE..=EA_HISTORY_SET_NAME).contains(&action))
    {
        return false;
    }

    if action == EA_POST_DELETE {
        return host.prepost_edit_list(dq_ptr, ids.unwrap_or(&[]), action, args);
    }
    if (EA_HISTORY_DELETE..=EA_HISTORY_SET_NAME).contains(&action) {
        return host.history_edit_list(dq_ptr, ids.unwrap_or(&[]), action, args);
    }

    let mut ok = true;
    let built_ids: Vec<i32>;
    let eff_ids = if let Some(name_list) = names {
        let mut out_ids = Vec::new();
        ok = build_id_list_from_name_list(host, dq_ptr, &mut out_ids, name_list, match_mode, action);
        built_ids = out_ids;
        Some(built_ids.as_slice())
    } else {
        ids
    };

    ok = ok
        && (intern_edit_list(coord, host, dq_ptr, None, eff_ids, action, args)
            || match_mode == MM_REGEX);

    host.dq_save(dq_ptr);
    ok
}

#[cfg(unix)]
fn build_id_list_from_name_list(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    out_ids: &mut Vec<i32>,
    name_list: &[&[u8]],
    match_mode: i32,
    action: i32,
) -> bool {
    use crate::queuesubsys_host::{c_bytes, EA_GROUP_MOVE_OFFSET, MM_NAME, MM_REGEX};
    use std::collections::HashSet;
    use std::ffi::CStr;

    struct NzbCand {
        id: i32,
        name: Vec<u8>,
        files: Vec<(i32, Vec<u8>)>,
    }

    let qlen = host.queue_len(dq_ptr);
    let mut queue_cands = Vec::with_capacity(qlen);
    for i in 0..qlen {
        let nzb_ptr = host.queue_at(dq_ptr, i);
        let nzb = host.nzb_snap(nzb_ptr, false);
        let mut files = Vec::new();
        if action < EA_GROUP_MOVE_OFFSET {
            let flen = host.nzb_file_len(nzb_ptr);
            files.reserve(flen);
            for fi in 0..flen {
                let fptr = host.nzb_file_at(nzb_ptr, fi);
                let f = host.file_snap(fptr);
                let base = crate::queuecoord::base_file_name(&f.filename);
                let mut cand = Vec::with_capacity(nzb.name.len() + 1 + base.len());
                cand.extend_from_slice(&nzb.name);
                cand.push(b'/');
                cand.extend_from_slice(base);
                if cand.len() > 1023 {
                    cand.truncate(1023);
                }
                files.push((f.id, cand));
            }
        }
        queue_cands.push(NzbCand {
            id: nzb.id,
            name: nzb.name,
            files,
        });
    }

    let mut unique_ids = HashSet::new();

    for &name in name_list {
        let mut regex_opt = if match_mode == MM_REGEX {
            let pat_c = c_bytes(name);
            let pat_cstr = unsafe { CStr::from_bytes_with_nul_unchecked(&pat_c) };
            let rx = crate::util::PosixRegEx::compile(pat_cstr, 0);
            if !rx.is_valid() {
                return false;
            }
            Some(rx)
        } else {
            None
        };

        let mut found = false;

        for nzb in &queue_cands {
            if action < EA_GROUP_MOVE_OFFSET {
                for &(fid, ref cand) in &nzb.files {
                    let matched = if let Some(ref mut rx) = regex_opt {
                        let cand_c = c_bytes(cand);
                        rx.match_cstr(unsafe { CStr::from_bytes_with_nul_unchecked(&cand_c) })
                    } else {
                        cand.as_slice() == name
                    };
                    if matched && unique_ids.insert(fid) {
                        out_ids.push(fid);
                        found = true;
                    }
                }
            } else {
                let matched = if let Some(ref mut rx) = regex_opt {
                    let cand_c = c_bytes(&nzb.name);
                    rx.match_cstr(unsafe { CStr::from_bytes_with_nul_unchecked(&cand_c) })
                } else {
                    nzb.name.as_slice() == name
                };
                if matched && unique_ids.insert(nzb.id) {
                    out_ids.push(nzb.id);
                    found = true;
                }
            }
        }

        if !found && match_mode == MM_NAME {
            return false;
        }
    }

    true
}

#[cfg(unix)]
fn prepare_list(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    id_list: Option<&[i32]>,
    action: i32,
    mut offset: i32,
) -> Vec<EditItem> {
    use crate::queuesubsys_host::{
        EA_FILE_MOVE_BOTTOM, EA_FILE_MOVE_OFFSET, EA_FILE_MOVE_TOP, EA_GROUP_MOVE_AFTER,
        EA_GROUP_MOVE_BEFORE, EA_GROUP_MOVE_BOTTOM, EA_GROUP_MOVE_OFFSET, EA_GROUP_MOVE_TOP,
    };

    let Some(ids) = id_list else {
        return Vec::new();
    };

    if action == EA_FILE_MOVE_TOP || action == EA_GROUP_MOVE_TOP {
        offset = -MAX_ID;
    } else if action == EA_FILE_MOVE_BOTTOM || action == EA_GROUP_MOVE_BOTTOM {
        offset = MAX_ID;
    }

    let mut items = Vec::with_capacity(ids.len());
    let qlen = host.queue_len(dq_ptr);

    if offset != 0
        && (action == EA_FILE_MOVE_OFFSET
            || action == EA_FILE_MOVE_TOP
            || action == EA_FILE_MOVE_BOTTOM)
    {
        for qi in 0..qlen {
            let nzb_ptr = host.queue_at(dq_ptr, qi);
            let nr_entries = host.nzb_file_len(nzb_ptr) as i32;
            let mut last_dest_pos = -1;
            let mut index = if offset < 0 { 0 } else { nr_entries - 1 };
            let end = if offset < 0 { nr_entries } else { -1 };
            let step = if offset < 0 { 1 } else { -1 };
            while index != end {
                let file_ptr = host.nzb_file_at(nzb_ptr, index as usize);
                let f = host.file_snap(file_ptr);
                if ids.contains(&f.id) {
                    let (work_offset, new_last) =
                        clamp_move_offset(index, nr_entries, offset, last_dest_pos);
                    last_dest_pos = new_last;
                    items.push(EditItem {
                        file_ptr,
                        nzb_ptr: 0,
                        offset: work_offset,
                    });
                }
                index += step;
            }
        }
    } else if (offset != 0
        && (action == EA_GROUP_MOVE_OFFSET
            || action == EA_GROUP_MOVE_TOP
            || action == EA_GROUP_MOVE_BOTTOM))
        || action == EA_GROUP_MOVE_BEFORE
        || action == EA_GROUP_MOVE_AFTER
    {
        let nr_entries = qlen as i32;
        let mut last_dest_pos = -1;
        let mut index = if offset <= 0 { 0 } else { nr_entries - 1 };
        let end = if offset <= 0 { nr_entries } else { -1 };
        let step = if offset <= 0 { 1 } else { -1 };
        while index != end {
            let nzb_ptr = host.queue_at(dq_ptr, index as usize);
            let nzb = host.nzb_snap(nzb_ptr, false);
            if ids.contains(&nzb.id) {
                let (work_offset, new_last) =
                    clamp_move_offset(index, nr_entries, offset, last_dest_pos);
                last_dest_pos = new_last;
                items.push(EditItem {
                    file_ptr: 0,
                    nzb_ptr,
                    offset: work_offset,
                });
            }
            index += step;
        }
    } else if action < EA_GROUP_MOVE_OFFSET {
        let mut max_id = 0i32;
        let mut min_id = MAX_ID;
        let mut all_files = Vec::new();
        for qi in 0..qlen {
            let nzb_ptr = host.queue_at(dq_ptr, qi);
            let flen = host.nzb_file_len(nzb_ptr);
            for fi in 0..flen {
                let fptr = host.nzb_file_at(nzb_ptr, fi);
                let fid = host.file_snap(fptr).id;
                if fid > max_id {
                    max_id = fid;
                }
                if fid < min_id {
                    min_id = fid;
                }
                all_files.push((fid, fptr));
            }
        }
        for &id in ids {
            if min_id <= id && id <= max_id {
                if let Some(&(_, fptr)) = all_files.iter().find(|&&(fid, _)| fid == id) {
                    items.push(EditItem {
                        file_ptr: fptr,
                        nzb_ptr: 0,
                        offset,
                    });
                }
            }
        }
    } else {
        let mut max_id = 0i32;
        let mut min_id = MAX_ID;
        let mut all_nzbs = Vec::with_capacity(qlen);
        for qi in 0..qlen {
            let nzb_ptr = host.queue_at(dq_ptr, qi);
            let nid = host.nzb_snap(nzb_ptr, false).id;
            if nid > max_id {
                max_id = nid;
            }
            if nid < min_id {
                min_id = nid;
            }
            all_nzbs.push((nid, nzb_ptr));
        }
        for &id in ids {
            if min_id <= id && id <= max_id {
                for &(nid, nptr) in &all_nzbs {
                    if id == nid {
                        items.push(EditItem {
                            file_ptr: 0,
                            nzb_ptr: nptr,
                            offset,
                        });
                    }
                }
            }
        }
    }

    items
}

#[cfg(unix)]
fn move_entry(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    file_ptr: usize,
    offset: i32,
) {
    let nzb_ptr = host.file_snap(file_ptr).nzb_ptr;
    let n = host.nzb_file_len(nzb_ptr);
    if n == 0 {
        return;
    }
    let mut order: Vec<usize> = (0..n).map(|i| host.nzb_file_at(nzb_ptr, i)).collect();
    let Some(entry) = order.iter().position(|&p| p == file_ptr) else {
        return;
    };
    let Some(new_entry) = clamp_move_target(entry as i32, offset, n as i32) else {
        return;
    };
    let moved = order.remove(entry);
    order.insert(new_entry as usize, moved);
    host.nzb_reorder_files(nzb_ptr, &order);
}

#[cfg(unix)]
fn move_group(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb_ptr: usize,
    offset: i32,
) {
    let n = host.queue_len(dq_ptr);
    if n == 0 {
        return;
    }
    let mut order: Vec<usize> = (0..n).map(|i| host.queue_at(dq_ptr, i)).collect();
    let Some(entry) = order.iter().position(|&p| p == nzb_ptr) else {
        return;
    };
    let Some(new_entry) = clamp_move_target(entry as i32, offset, n as i32) else {
        return;
    };
    let moved = order.remove(entry);
    order.insert(new_entry as usize, moved);
    host.queue_reorder(dq_ptr, &order);
}

#[cfg(unix)]
fn delete_entry(
    coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    file_ptr: usize,
) {
    use crate::queuesubsys_host::{MK_DETAIL, MK_INFO};
    let file = host.file_snap(file_ptr);
    if !file.deleted {
        let nzb = host.nzb_snap(file.nzb_ptr, false);
        let kind = if nzb.deleting { MK_DETAIL } else { MK_INFO };
        let msg = push_bytes_msg(
            b"Deleting file ",
            &file.filename,
            b" from download queue",
            b"",
            b"",
        );
        host.nzb_print_message(file.nzb_ptr, kind, &msg);
        coord.delete_queue_entry(host, dq_ptr, file_ptr);
    }
}

#[cfg(unix)]
fn pause_pars_in_groups(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    items: &mut Vec<EditItem>,
    extra_pars_only: bool,
) {
    while !items.is_empty() {
        let first_nzb = host.file_snap(items[0].file_ptr).nzb_ptr;
        let mut group_files = Vec::new();
        let mut rest = Vec::new();
        for item in items.drain(..) {
            let f = host.file_snap(item.file_ptr);
            if f.nzb_ptr == first_nzb {
                group_files.push(f);
            } else {
                rest.push(item);
            }
        }
        *items = rest;
        if !group_files.is_empty() {
            pause_pars(host, &group_files, extra_pars_only);
        } else {
            break;
        }
    }
}

#[cfg(unix)]
fn pause_pars(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    files: &[crate::queuesubsys_host::FileSnap],
    extra_pars_only: bool,
) {
    use crate::queuesubsys_host::{FILE_INT_PAUSED, MK_DEBUG};
    host.log(MK_DEBUG, b"QueueEditor: Pausing pars");
    if files.is_empty() {
        return;
    }
    let n = files.len();
    let mut is_par = Vec::with_capacity(n);
    let mut has_vol = Vec::with_capacity(n);
    let mut sizes = Vec::with_capacity(n);
    for f in files {
        is_par.push(u8::from(f.par_file));
        has_vol.push(u8::from(filename_has_vol(&f.filename)));
        sizes.push(f.size);
    }
    let mut pause = vec![0u8; n];
    select_pars_to_pause(&is_par, &has_vol, &sizes, extra_pars_only, &mut pause);
    for (i, &p) in pause.iter().enumerate() {
        if p != 0 {
            host.file_set_int(files[i].ptr, FILE_INT_PAUSED, 1);
        }
    }
}

#[cfg(unix)]
fn set_nzb_priority(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: usize,
    priority: Option<&[u8]>,
) {
    use crate::queuesubsys_host::{MK_DEBUG, NZB_INT_PRIORITY};
    let nzb = host.nzb_snap(nzb_ptr, false);
    let prio_disp = priority.unwrap_or(b"(null)");
    let msg = push_bytes_msg(b"Setting priority ", prio_disp, b" for ", &nzb.name, b"");
    host.log(MK_DEBUG, &msg);
    let val = crate::util::parse_c_atoi_or_default(priority, 0);
    host.nzb_set_int(nzb_ptr, NZB_INT_PRIORITY, val);
}

#[cfg(unix)]
fn resolve_category_settings(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    cat_name: &[u8],
) -> bool {
    let mut unpack = opts.unpack;
    if !cat_name.is_empty() {
        if let Some(cat_unpack) = host.find_category(cat_name) {
            unpack = cat_unpack;
        }
    }
    unpack
}

#[cfg(unix)]
fn set_nzb_category(
    coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: usize,
    category: Option<&[u8]>,
    apply_params: bool,
) {
    use crate::queuesubsys_host::MK_DEBUG;
    let nzb = host.nzb_snap(nzb_ptr, false);
    let cat_disp = category.unwrap_or(b"(null)");
    let msg = push_bytes_msg(
        b"QueueEditor: setting category '",
        cat_disp,
        b"' for '",
        &nzb.name,
        b"'",
    );
    host.log(MK_DEBUG, &msg);

    let opts = host.opts();
    let old_unpack = if apply_params {
        resolve_category_settings(host, &opts, &nzb.category)
    } else {
        opts.unpack
    };

    coord.set_queue_entry_category(host, nzb_ptr, category.unwrap_or(b""));

    if !apply_params {
        return;
    }

    let nzb_after = host.nzb_snap(nzb_ptr, false);
    let new_unpack = resolve_category_settings(host, &opts, &nzb_after.category);
    if old_unpack != new_unpack {
        host.nzb_set_param(
            nzb_ptr,
            b"*Unpack:",
            Some(if new_unpack { b"yes" } else { b"no" }),
        );
    }
}

#[cfg(unix)]
fn set_nzb_name(
    coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: usize,
    name: Option<&[u8]>,
) {
    use crate::queuesubsys_host::MK_DEBUG;
    let nzb = host.nzb_snap(nzb_ptr, false);
    let name_disp = name.unwrap_or(b"(null)");
    let msg = push_bytes_msg(
        b"QueueEditor: renaming '",
        &nzb.name,
        b"' to '",
        name_disp,
        b"'",
    );
    host.log(MK_DEBUG, &msg);

    let prev = nzb.name;
    coord.set_queue_entry_name(host, nzb_ptr, name.unwrap_or(b""));
    let after = host.nzb_snap(nzb_ptr, false);
    if prev != after.name {
        host.dupe_search_notify_name_changed(after.id);
    }
}

#[cfg(unix)]
fn set_nzb_parameter(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: usize,
    param_string: Option<&[u8]>,
) {
    use crate::queuesubsys_host::{MK_DEBUG, MK_ERROR};
    let nzb = host.nzb_snap(nzb_ptr, false);
    let param_disp = param_string.unwrap_or(b"(null)");
    let base_fn = crate::queuecoord::base_file_name(&nzb.filename);
    let dbg_msg = push_bytes_msg(
        b"QueueEditor: setting nzb parameter '",
        param_disp,
        b"' for '",
        base_fn,
        b"'",
    );
    host.log(MK_DEBUG, &dbg_msg);

    if let Some((name, val)) = param_string.and_then(crate::historycoord::split_param_assignment) {
        host.nzb_set_param(nzb_ptr, name, Some(val));
    } else {
        let err_msg = push_bytes_msg(
            b"Could not set nzb parameter for ",
            &nzb.name,
            b": invalid argument: ",
            param_disp,
            b"",
        );
        host.log(MK_ERROR, &err_msg);
    }
}

#[cfg(unix)]
fn set_nzb_dupe_param(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: usize,
    action: i32,
    text: Option<&[u8]>,
) {
    use crate::queuesubsys_host::{
        EA_GROUP_SET_DUPE_KEY, EA_GROUP_SET_DUPE_MODE, EA_GROUP_SET_DUPE_SCORE, MK_DEBUG, MK_ERROR,
        NZB_INT_DUPE_MODE, NZB_INT_DUPE_SCORE, NZB_STR_DUPE_KEY,
    };
    let nzb = host.nzb_snap(nzb_ptr, false);
    let text_disp = text.unwrap_or(b"(null)");
    let act_str = action.to_string();
    let mut dbg = Vec::with_capacity(64 + text_disp.len() + nzb.name.len());
    dbg.extend_from_slice(b"QueueEditor: setting dupe parameter ");
    dbg.extend_from_slice(act_str.as_bytes());
    dbg.extend_from_slice(b"='");
    dbg.extend_from_slice(text_disp);
    dbg.extend_from_slice(b"' for '");
    dbg.extend_from_slice(&nzb.name);
    dbg.push(b'\'');
    host.log(MK_DEBUG, &dbg);

    match action {
        EA_GROUP_SET_DUPE_KEY => {
            host.nzb_set_param(nzb_ptr, b"DupeSearchOwnedKey", None);
            host.nzb_set_str(nzb_ptr, NZB_STR_DUPE_KEY, text.unwrap_or(b""));
        }
        EA_GROUP_SET_DUPE_SCORE => {
            let score = crate::util::parse_c_atoi_or_default(text, 0);
            host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_SCORE, score);
        }
        EA_GROUP_SET_DUPE_MODE => {
            if let Some(mode) = text.and_then(parse_dupe_mode) {
                host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_MODE, mode);
            } else {
                let err = push_bytes_msg(
                    b"Could not set duplicate mode for ",
                    &nzb.name,
                    b": incorrect mode (",
                    text_disp,
                    b")",
                );
                host.log(MK_ERROR, &err);
            }
        }
        _ => {}
    }
}

#[cfg(unix)]
fn align_groups(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    items: &[EditItem],
) {
    let n = host.queue_len(dq_ptr);
    if n == 0 {
        return;
    }
    let mut order: Vec<usize> = (0..n).map(|i| host.queue_at(dq_ptr, i)).collect();
    let mut has_last = false;
    let mut last_num: usize = 0;
    let mut num: usize = 0;
    while num < order.len() {
        let nzb_ptr = order[num];
        let selected = items.iter().any(|it| it.nzb_ptr == nzb_ptr);
        if selected {
            if has_last && num - last_num > 1 {
                let moved = order.remove(num);
                order.insert(last_num + 1, moved);
                last_num += 1;
            } else {
                last_num = num;
            }
            has_last = true;
        }
        num += 1;
    }
    host.queue_reorder(dq_ptr, &order);
}

#[cfg(unix)]
fn sort_groups(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    items: &[EditItem],
    sort: Option<&[u8]>,
) -> bool {
    use crate::queuesubsys_host::MK_ERROR;
    align_groups(host, dq_ptr, items);
    let Some(sort_bytes) = sort else {
        return false;
    };
    let Some((crit, orig_order)) = parse_sort_spec(sort_bytes) else {
        let err = push_bytes_msg(
            b"Could not sort groups: incorrect sort order (",
            sort_bytes,
            b")",
            b"",
            b"",
        );
        host.log(MK_ERROR, &err);
        return false;
    };

    let n = host.queue_len(dq_ptr);
    let orig_snaps: Vec<crate::queuesubsys_host::NzbSnap> = (0..n)
        .map(|i| host.nzb_snap(host.queue_at(dq_ptr, i), false))
        .collect();

    let mut sort_order = if orig_order == SORT_ORDER_AUTO {
        if crit == SORT_CRITERIA_PRIORITY {
            SORT_ORDER_DESCENDING
        } else {
            SORT_ORDER_ASCENDING
        }
    } else {
        orig_order
    };

    let initial_sort_state = items.is_empty();
    let run_sort = |snaps: &mut [crate::queuesubsys_host::NzbSnap], order: i32| {
        stable_merge_sort_by(snaps, &mut |a, b| {
            let sort_a = initial_sort_state || items.iter().any(|it| it.nzb_ptr == a.ptr);
            let sort_b = initial_sort_state || items.iter().any(|it| it.nzb_ptr == b.ptr);
            if !sort_a || !sort_b {
                return false;
            }
            let cmp_name = match a.name.cmp(&b.name) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            };
            let cmp_cat = match a.category.cmp(&b.category) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            };
            let rem1 = a.remaining_size - a.paused_size;
            let rem2 = b.remaining_size - b.paused_size;
            group_sorter_compare_values(
                crit,
                order,
                SORT_ORDER_DESCENDING,
                cmp_name,
                a.size,
                b.size,
                rem1,
                rem2,
                a.min_time,
                b.min_time,
                cmp_cat,
                a.priority,
                b.priority,
            )
        });
    };

    let mut sorted = orig_snaps.clone();
    run_sort(&mut sorted, sort_order);

    if orig_order == SORT_ORDER_AUTO
        && sorted
            .iter()
            .zip(orig_snaps.iter())
            .all(|(a, b)| a.ptr == b.ptr)
    {
        sort_order = if sort_order == SORT_ORDER_DESCENDING {
            SORT_ORDER_ASCENDING
        } else {
            SORT_ORDER_DESCENDING
        };
        run_sort(&mut sorted, sort_order);
    }

    let new_ptrs: Vec<usize> = sorted.iter().map(|s| s.ptr).collect();
    host.queue_reorder(dq_ptr, &new_ptrs);
    true
}

#[cfg(unix)]
fn move_groups_to(
    coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    items: &[EditItem],
    id_list: Option<&[i32]>,
    mut before: bool,
    args: Option<&[u8]>,
) -> bool {
    use crate::queuesubsys_host::EA_GROUP_MOVE_OFFSET;
    let Some(args_bytes) = args else {
        return false;
    };
    if items.is_empty() || args_bytes.is_empty() {
        return false;
    }

    let mut target_id = crate::util::parse_c_atoi(args_bytes);
    let mut offset = 0i32;

    if host.queue_find_by_id(dq_ptr, target_id) == 0 {
        return false;
    }

    let moved_ids: Vec<i32> = items
        .iter()
        .map(|it| host.nzb_snap(it.nzb_ptr, false).id)
        .collect();
    if moved_ids.contains(&target_id) {
        let qlen = host.queue_len(dq_ptr);
        let queue_ids: Vec<i32> = (0..qlen)
            .map(|i| host.nzb_snap(host.queue_at(dq_ptr, i), false).id)
            .collect();
        let (new_target_id, new_before, new_offset) =
            resolve_move_groups_target(target_id, &queue_ids, &moved_ids, before, MAX_ID);
        target_id = new_target_id;
        before = new_before;
        offset = new_offset;
    }

    align_groups(host, dq_ptr, items);

    if offset == 0 {
        let qlen = host.queue_len(dq_ptr);
        let queue_ids: Vec<i32> = (0..qlen)
            .map(|i| host.nzb_snap(host.queue_at(dq_ptr, i), false).id)
            .collect();
        offset = calc_move_groups_to_offset(&queue_ids, target_id, moved_ids[0], before, items.len());
    }

    let off_str = offset.to_string();
    intern_edit_list(
        coord,
        host,
        dq_ptr,
        None,
        id_list,
        EA_GROUP_MOVE_OFFSET,
        Some(off_str.as_bytes()),
    )
}

#[cfg(unix)]
fn reorder_files(host: &crate::queuesubsys_host::QueueSubsysHost, items: &[EditItem]) {
    if items.is_empty() {
        return;
    }
    let nzb_ptr = host.file_snap(items[0].file_ptr).nzb_ptr;
    let n = host.nzb_file_len(nzb_ptr);
    if n == 0 {
        return;
    }
    let mut order: Vec<usize> = (0..n).map(|i| host.nzb_file_at(nzb_ptr, i)).collect();
    let mut insert_pos = 0usize;
    for item in items {
        if let Some(pos) = order.iter().position(|&p| p == item.file_ptr) {
            let moved = order.remove(pos);
            order.insert(insert_pos, moved);
            insert_pos += 1;
        }
    }
    host.nzb_reorder_files(nzb_ptr, &order);
}

#[cfg(unix)]
fn sort_group_files(host: &crate::queuesubsys_host::QueueSubsysHost, nzb_ptr: usize) {
    use crate::queuesubsys_host::MK_DEBUG;
    let nzb = host.nzb_snap(nzb_ptr, false);
    let dbg = push_bytes_msg(
        b"QueueEditor: sorting inner files for '",
        &nzb.name,
        b"'",
        b"",
        b"",
    );
    host.log(MK_DEBUG, &dbg);

    let n = host.nzb_file_len(nzb_ptr);
    if n <= 1 {
        return;
    }
    let mut snaps: Vec<crate::queuesubsys_host::FileSnap> = (0..n)
        .map(|i| host.file_snap(host.nzb_file_at(nzb_ptr, i)))
        .collect();
    stable_merge_sort_by(&mut snaps, &mut |f1, f2| {
        let same_par = f1.par_file
            && f2.par_file
            && crate::parparser::same_par_collection(
                &f1.filename,
                &f2.filename,
                f1.filename_confirmed && f2.filename_confirmed,
            );
        compare_inner_files(
            &f1.filename,
            f1.par_file,
            f1.size,
            &f2.filename,
            f2.par_file,
            f2.size,
            same_par,
        )
    });
    let new_ptrs: Vec<usize> = snaps.iter().map(|f| f.ptr).collect();
    host.nzb_reorder_files(nzb_ptr, &new_ptrs);
}

#[cfg(unix)]
fn merge_groups(
    coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    items: &[EditItem],
) -> bool {
    use crate::queuesubsys_host::MK_DEBUG;
    if items.is_empty() {
        return false;
    }
    let dest_nzb = items[0].nzb_ptr;
    let mut ok = true;
    for item in items {
        if item.nzb_ptr != dest_nzb {
            let src_snap = host.nzb_snap(item.nzb_ptr, false);
            let dest_snap = host.nzb_snap(dest_nzb, false);
            let dbg = push_bytes_msg(
                b"merge ",
                &src_snap.filename,
                b" to ",
                &dest_snap.filename,
                b"",
            );
            host.log(MK_DEBUG, &dbg);
            if !coord.merge_queue_entries(host, dq_ptr, dest_nzb, item.nzb_ptr) {
                ok = false;
            }
        }
    }
    ok
}

#[cfg(unix)]
fn split_group(
    coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    items: &[EditItem],
    name: Option<&[u8]>,
) -> bool {
    if items.is_empty() {
        return false;
    }
    let file_ptrs: Vec<usize> = items.iter().map(|it| it.file_ptr).collect();
    coord.split_queue_entries(host, dq_ptr, &file_ptrs, name.unwrap_or(b""))
}

#[cfg(unix)]
fn edit_group(
    coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb_ptr: usize,
    action: i32,
    args: Option<&[u8]>,
) -> bool {
    use crate::queuesubsys_host::{
        ASPECT_NZB_DELETED, DS_DUPE, EA_GROUP_DELETE, EA_GROUP_DUPE_DELETE, EA_GROUP_FINAL_DELETE,
        EA_GROUP_PARK_DELETE, NZB_INT_AVOID_HISTORY, NZB_INT_CLEANUP_DISK, NZB_INT_DELETE_PAUSED,
        NZB_INT_DELETE_STATUS, NZB_INT_DELETING, NZB_INT_PARKING,
    };

    let nzb = host.nzb_snap(nzb_ptr, false);
    let nzb_id = nzb.id;
    let flen = host.nzb_file_len(nzb_ptr);
    let mut group_items = Vec::with_capacity(flen);
    let mut all_paused = true;
    for i in 0..flen {
        let fptr = host.nzb_file_at(nzb_ptr, i);
        let f = host.file_snap(fptr);
        all_paused &= f.paused;
        group_items.push(EditItem {
            file_ptr: fptr,
            nzb_ptr: 0,
            offset: 0,
        });
    }

    let file_action = map_group_to_file_action(action);
    let is_group_del = matches!(
        action,
        EA_GROUP_DELETE | EA_GROUP_PARK_DELETE | EA_GROUP_DUPE_DELETE | EA_GROUP_FINAL_DELETE
    );
    if is_group_del {
        let opts = host.opts();
        let (parking, avoid_history, set_dupe_status, cleanup_disk) = eval_group_delete_flags(
            action,
            EA_GROUP_PARK_DELETE,
            EA_GROUP_DUPE_DELETE,
            EA_GROUP_FINAL_DELETE,
            opts.keep_history,
            nzb.unpack_cleaned_up_disk,
            nzb.current_success_articles,
        );
        host.nzb_set_int(nzb_ptr, NZB_INT_DELETING, 1);
        host.nzb_set_int(nzb_ptr, NZB_INT_PARKING, i32::from(parking));
        host.nzb_set_int(nzb_ptr, NZB_INT_AVOID_HISTORY, i32::from(avoid_history));
        host.nzb_set_int(nzb_ptr, NZB_INT_DELETE_PAUSED, i32::from(all_paused));
        if set_dupe_status {
            host.nzb_set_int(nzb_ptr, NZB_INT_DELETE_STATUS, DS_DUPE);
        }
        host.nzb_set_int(nzb_ptr, NZB_INT_CLEANUP_DISK, i32::from(cleanup_disk));
    }

    let ok = intern_edit_list(
        coord,
        host,
        dq_ptr,
        Some(&mut group_items),
        None,
        file_action,
        args,
    );

    if is_group_del && host.queue_find_by_id(dq_ptr, nzb_id) != 0 {
        host.dq_notify(dq_ptr, ASPECT_NZB_DELETED, nzb_ptr, 0);
    }

    ok
}

#[cfg(unix)]
fn intern_edit_list(
    coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    item_list_opt: Option<&mut Vec<EditItem>>,
    id_list_opt: Option<&[i32]>,
    action: i32,
    args: Option<&[u8]>,
) -> bool {
    use crate::queuesubsys_host::{
        EA_FILE_DELETE, EA_FILE_MOVE_BOTTOM, EA_FILE_MOVE_OFFSET, EA_FILE_MOVE_TOP, EA_FILE_PAUSE,
        EA_FILE_PAUSE_ALL_PARS, EA_FILE_PAUSE_EXTRA_PARS, EA_FILE_REORDER, EA_FILE_RESUME,
        EA_FILE_SPLIT, EA_GROUP_APPLY_CATEGORY, EA_GROUP_DELETE, EA_GROUP_DUPE_DELETE,
        EA_GROUP_FINAL_DELETE, EA_GROUP_MERGE, EA_GROUP_MOVE_AFTER, EA_GROUP_MOVE_BEFORE,
        EA_GROUP_MOVE_BOTTOM, EA_GROUP_MOVE_OFFSET, EA_GROUP_MOVE_TOP, EA_GROUP_PARK_DELETE,
        EA_GROUP_PAUSE, EA_GROUP_PAUSE_ALL_PARS, EA_GROUP_PAUSE_EXTRA_PARS, EA_GROUP_RESUME,
        EA_GROUP_SET_CATEGORY, EA_GROUP_SET_DUPE_KEY, EA_GROUP_SET_DUPE_MODE,
        EA_GROUP_SET_DUPE_SCORE, EA_GROUP_SET_NAME, EA_GROUP_SET_PARAMETER,
        EA_GROUP_SET_PRIORITY, EA_GROUP_SORT, EA_GROUP_SORT_FILES, FILE_INT_PAUSED, NK_URL,
    };

    let unique_ids = id_list_opt.map(dedup_id_list);
    let eff_ids = unique_ids.as_deref();

    let mut work_items;
    let items: &mut Vec<EditItem> = if let Some(passed) = item_list_opt {
        passed
    } else {
        let offset = match args {
            Some(a) if action == EA_FILE_MOVE_OFFSET || action == EA_GROUP_MOVE_OFFSET => {
                crate::util::parse_c_atoi(a)
            }
            _ => 0,
        };
        work_items = prepare_list(host, dq_ptr, eff_ids, action, offset);
        &mut work_items
    };

    if action == EA_FILE_PAUSE_ALL_PARS || action == EA_FILE_PAUSE_EXTRA_PARS {
        let any = !items.is_empty();
        pause_pars_in_groups(host, items, action == EA_FILE_PAUSE_EXTRA_PARS);
        return any;
    }

    match action {
        EA_GROUP_MERGE => return merge_groups(coord, host, dq_ptr, items),
        EA_GROUP_SORT => return sort_groups(host, dq_ptr, items, args),
        EA_GROUP_MOVE_AFTER | EA_GROUP_MOVE_BEFORE => {
            return move_groups_to(
                coord,
                host,
                dq_ptr,
                items,
                eff_ids,
                action == EA_GROUP_MOVE_BEFORE,
                args,
            );
        }
        EA_FILE_SPLIT => return split_group(coord, host, dq_ptr, items, args),
        EA_FILE_REORDER => reorder_files(host, items),
        _ => {
            for item in items.iter() {
                match action {
                    EA_FILE_PAUSE => host.file_set_int(item.file_ptr, FILE_INT_PAUSED, 1),
                    EA_FILE_RESUME => host.file_set_int(item.file_ptr, FILE_INT_PAUSED, 0),
                    EA_FILE_MOVE_OFFSET | EA_FILE_MOVE_TOP | EA_FILE_MOVE_BOTTOM => {
                        move_entry(host, item.file_ptr, item.offset);
                    }
                    EA_FILE_DELETE => delete_entry(coord, host, dq_ptr, item.file_ptr),
                    EA_GROUP_SET_PRIORITY => set_nzb_priority(host, item.nzb_ptr, args),
                    EA_GROUP_SET_CATEGORY | EA_GROUP_APPLY_CATEGORY => {
                        set_nzb_category(
                            coord,
                            host,
                            item.nzb_ptr,
                            args,
                            action == EA_GROUP_APPLY_CATEGORY,
                        );
                    }
                    EA_GROUP_SET_NAME => set_nzb_name(coord, host, item.nzb_ptr, args),
                    EA_GROUP_SET_DUPE_KEY | EA_GROUP_SET_DUPE_SCORE | EA_GROUP_SET_DUPE_MODE => {
                        set_nzb_dupe_param(host, item.nzb_ptr, action, args);
                    }
                    EA_GROUP_SET_PARAMETER => set_nzb_parameter(host, item.nzb_ptr, args),
                    EA_GROUP_MOVE_TOP | EA_GROUP_MOVE_BOTTOM | EA_GROUP_MOVE_OFFSET => {
                        move_group(host, dq_ptr, item.nzb_ptr, item.offset);
                    }
                    EA_GROUP_PAUSE
                    | EA_GROUP_RESUME
                    | EA_GROUP_PAUSE_ALL_PARS
                    | EA_GROUP_PAUSE_EXTRA_PARS => {
                        edit_group(coord, host, dq_ptr, item.nzb_ptr, action, args);
                    }
                    EA_GROUP_DELETE
                    | EA_GROUP_PARK_DELETE
                    | EA_GROUP_DUPE_DELETE
                    | EA_GROUP_FINAL_DELETE => {
                        let nzb = host.nzb_snap(item.nzb_ptr, false);
                        if nzb.kind == NK_URL {
                            host.url_delete_queue_entry(
                                dq_ptr,
                                item.nzb_ptr,
                                action == EA_GROUP_FINAL_DELETE,
                            );
                        } else {
                            edit_group(coord, host, dq_ptr, item.nzb_ptr, action, args);
                        }
                    }
                    EA_GROUP_SORT_FILES => sort_group_files(host, item.nzb_ptr),
                    _ => {}
                }
            }
        }
    }

    !items.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_spec_and_move_offsets() {
        assert_eq!(
            parse_sort_spec(b"priority+"),
            Some((SORT_CRITERIA_PRIORITY, SORT_ORDER_ASCENDING))
        );
        assert_eq!(
            parse_sort_spec(b"left"),
            Some((SORT_CRITERIA_REMAINING_SIZE, SORT_ORDER_AUTO))
        );
        assert_eq!(dedup_id_list(&[3, 1, 3, 2, 1]), vec![3, 1, 2]);
        assert_eq!(
            calc_move_groups_to_offset(&[10, 20, 30, 40], 30, 10, false, 1),
            2
        );
    }
}
