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

/// `QueueEditor::SetNzbCategory`:
/// Visits `(param_name, "yes"|"no")` for each script added or removed between `old_ext` and `new_ext`.
pub fn visit_category_ext_diff<F>(old_ext: &[u8], new_ext: &[u8], mut cb: F)
where
    F: FnMut(&[u8], &[u8]),
{
    if old_ext.eq_ignore_ascii_case(new_ext) {
        return;
    }
    for new_script in crate::util::tokens(new_ext, b",;") {
        let found = crate::util::tokens(old_ext, b",;")
            .any(|old_script| new_script.eq_ignore_ascii_case(old_script));
        if !found {
            let mut key = Vec::with_capacity(new_script.len() + 1);
            key.extend_from_slice(new_script);
            key.push(b':');
            if key.len() > 1023 {
                key.truncate(1023);
            }
            cb(&key, b"yes");
        }
    }
    for old_script in crate::util::tokens(old_ext, b",;") {
        let found = crate::util::tokens(new_ext, b",;")
            .any(|new_script| old_script.eq_ignore_ascii_case(new_script));
        if !found {
            let mut key = Vec::with_capacity(old_script.len() + 1);
            key.extend_from_slice(old_script);
            key.push(b':');
            if key.len() > 1023 {
                key.truncate(1023);
            }
            cb(&key, b"no");
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
    let mut found = false;
    for &qid in queue_ids {
        if found {
            if !moved_ids.contains(&qid) {
                return (qid, true, 0);
            }
        } else if qid == target_id {
            found = true;
        }
    }
    (target_id, before, max_id)
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
