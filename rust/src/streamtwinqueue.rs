//
//  This file is part of nzbget. See <https://nzbget.com>.
//
//  Copyright (C) 2026 Denis <denis@nzbget.com>
//
//  This program is free software; you can redistribute it and/or modify
//  it under the terms of the GNU General Public License as published by
//  the Free Software Foundation; either version 2 of the License, or
//  (at your option) any later version.
//
//  This program is distributed in the hope that it will be useful,
//  but WITHOUT ANY WARRANTY; without even the implied warranty of
//  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//  GNU General Public License for more details.
//
//  You should have received a copy of the GNU General Public License
//  along with this program.  If not, see <https://www.gnu.org/licenses/>.
//

//! Hot-path kernels for `StreamRepair`, `DupeStreamRepair`, `TwinCheck`, and `QueueEditor`.

/// Validates whether the first fetched donor article (`partIndex == 0`) can
/// initialize `DonorMemberSource::EnsureInit`.
pub fn stream_repair_is_first_donor_part_valid(
    has_first: bool,
    success: bool,
    offset: i64,
) -> bool {
    has_first && success && offset == 0
}

/// Evaluates one resolution step in `DonorMemberSource::PartForOffset`.
/// Returns `(status, new_index)` where:
/// - `status == 0`: out of bounds (`return nullptr`)
/// - `status == 1`: step again with `index = new_index`
/// - `status == 2`: found matching part at `index`
pub fn stream_repair_step_part_for_offset(
    offset: i64,
    fetched_offset: i64,
    fetched_len: usize,
    index: i32,
    ranges_len: usize,
) -> (i32, i32) {
    if offset < fetched_offset {
        if index <= 0 {
            (0, index)
        } else {
            (1, index - 1)
        }
    } else if offset >= fetched_offset.saturating_add(fetched_len as i64) {
        if (index + 1) as usize >= ranges_len {
            (0, index)
        } else {
            (1, index + 1)
        }
    } else {
        (2, index)
    }
}

/// Computes `(first, last)` inclusive prefetch part indices in `DonorMemberSource::Prefetch`.
pub fn stream_repair_calc_prefetch_bounds(
    first_est: i32,
    last_est: i32,
    ranges_len: usize,
) -> (i32, i32) {
    let first = (first_est - 1).max(0);
    let max_last = (ranges_len as i32) - 1;
    let last = (last_est + 1).min(max_last);
    (first, last)
}

/// Computes `(from, len)` for copying a slice of a fetched donor article in `DonorMemberSource::Read`.
pub fn stream_repair_calc_donor_read_chunk(
    pos: i64,
    fetched_offset: i64,
    fetched_data_len: usize,
    end_offset: i64,
) -> (i64, i64) {
    let from = pos - fetched_offset;
    let avail = (fetched_data_len as i64) - from;
    let rem = end_offset - pos;
    let len = avail.min(rem);
    (from, len)
}

/// Checks whether a target file in `StreamRepairController::CollectTargets` is a `.par2` file
/// that should be excluded from stream repair targets.
pub fn stream_repair_is_par2_job_or_file(is_par_file: bool, filename: &[u8]) -> bool {
    if is_par_file {
        return true;
    }
    filename.len() >= 5 && filename[filename.len() - 5..].eq_ignore_ascii_case(b".par2")
}

/// Evaluates fast pre-eligibility of a donor `NzbInfo` in `StreamRepairController::CollectDonors`
/// before calling `DupeCoordinator::SameNameOrKey`.
pub fn stream_repair_is_donor_nzb_pre_eligible(
    is_self: bool,
    kind: i32,
    nk_nzb: i32,
    dupe_mode: i32,
    dm_force: i32,
) -> bool {
    !is_self && kind == nk_nzb && dupe_mode != dm_force
}

/// Checks whether a donor candidate in `StreamRepairController::CollectDonors` should be skipped
/// (either because it shares the exact full content hash or has an empty queued filename).
pub fn stream_repair_should_skip_donor_candidate(
    own_full_hash: u32,
    donor_full_hash: u32,
    queued_filename: &[u8],
) -> bool {
    if own_full_hash > 0 && own_full_hash == donor_full_hash {
        return true;
    }
    queued_filename.is_empty()
}

/// Evaluates consecutive unproductive donor outcomes in `StreamRepairController::ExecRepair`.
/// Returns `(new_consecutive_failures, should_bail)`.
pub fn stream_repair_eval_donor_outcome_bail(
    outcome: i32,
    ro_productive: i32,
    ro_unproductive: i32,
    donor_proven: bool,
    cur_consecutive_failures: i32,
    donor_failure_bail: i32,
    unproven_donor_bail: i32,
) -> (i32, bool) {
    if outcome == ro_productive {
        (0, false)
    } else if outcome == ro_unproductive {
        let next = cur_consecutive_failures + 1;
        let limit = if donor_proven {
            donor_failure_bail
        } else {
            unproven_donor_bail
        };
        (next, next >= limit)
    } else {
        (cur_consecutive_failures, false)
    }
}

/// Resolves the `ERepairOutcome` at the end of `StreamRepairController::RepairFile`.
pub fn stream_repair_resolve_repair_file_outcome(
    patched: bool,
    spent_fetches: bool,
    ro_productive: i32,
    ro_unproductive: i32,
    ro_no_cost: i32,
) -> i32 {
    if patched {
        ro_productive
    } else if spent_fetches {
        ro_unproductive
    } else {
        ro_no_cost
    }
}

/// Computes the number of replacement probe parts to draw from `reserve` in `StreamRepairController::VerifyDonor`.
pub fn stream_repair_calc_probe_replacement_count(
    missing: i32,
    reserve_len: usize,
    max_probe_fetches: i32,
    attempted_len: usize,
) -> i32 {
    let budget_rem = max_probe_fetches - (attempted_len as i32);
    missing.min(reserve_len as i32).min(budget_rem).max(0)
}

/// Computes the `reserve` index for the `k`-th replacement probe part in `StreamRepairController::VerifyDonor`.
pub fn stream_repair_pick_replacement_index(
    reserve_len: usize,
    k: i32,
    replacements: i32,
) -> usize {
    if replacements <= 0 || reserve_len == 0 {
        return 0;
    }
    (reserve_len * ((2 * (k as usize)) + 1)) / (2 * (replacements as usize))
}

/// Validates a fetched probe article in `StreamRepairController::CompareProbes`.
/// Returns `(is_valid, is_missing)`.
pub fn stream_repair_is_probe_article_valid(
    success: bool,
    data_len: usize,
    file_size: i64,
    offset: i64,
    target_decoded_size: i64,
) -> (bool, bool) {
    if !success || data_len == 0 {
        return (false, true);
    }
    if file_size != target_decoded_size
        || offset.saturating_add(data_len as i64) > target_decoded_size
    {
        return (false, false);
    }
    (true, false)
}

/// Clips a target hole `(hole_offset, hole_size)` against a fetched donor article `(fetched_offset, fetched_len)`.
/// Returns `Some((from, to))` when `from < to`.
pub fn stream_repair_clip_hole_to_fetched(
    hole_offset: i64,
    hole_size: i64,
    fetched_offset: i64,
    fetched_len: usize,
) -> Option<(i64, i64)> {
    let hole_end = hole_offset.saturating_add(hole_size);
    let fetched_end = fetched_offset.saturating_add(fetched_len as i64);
    let from = hole_offset.max(fetched_offset);
    let to = hole_end.min(fetched_end);
    if from < to {
        Some((from, to))
    } else {
        None
    }
}

/// Classifies a target at the end of `StreamRepairController::ReportRemainingHoles` and updates
/// recovered article/byte-health credits when the target is fully repaired.
/// Returns `(status, new_recovered_articles, new_recovered_failed_size, new_recovered_failed_par_size)`:
/// - `status == 0`: fully repaired and credited
/// - `status == 1`: interrupted and unfinished (skip logging/credit)
/// - `status == 2`: never sized (`DecodedFileSize <= 0`)
/// - `status == 3`: still has remaining holes (`!Holes.empty()`)
#[allow(clippy::too_many_arguments)]
pub fn stream_repair_classify_remaining_hole_target(
    interrupted: bool,
    decoded_file_size: i64,
    holes_empty: bool,
    is_par_file: bool,
    failed_articles: i32,
    failed_size: i64,
    missed_size: i64,
    cur_recovered_articles: i32,
    cur_recovered_failed_size: i64,
    cur_recovered_failed_par_size: i64,
) -> (i32, i32, i64, i64) {
    if interrupted && (decoded_file_size <= 0 || !holes_empty) {
        return (
            1,
            cur_recovered_articles,
            cur_recovered_failed_size,
            cur_recovered_failed_par_size,
        );
    }
    if decoded_file_size <= 0 {
        return (
            2,
            cur_recovered_articles,
            cur_recovered_failed_size,
            cur_recovered_failed_par_size,
        );
    }
    if !holes_empty {
        return (
            3,
            cur_recovered_articles,
            cur_recovered_failed_size,
            cur_recovered_failed_par_size,
        );
    }
    let delta_size = failed_size.saturating_add(missed_size);
    let new_articles = cur_recovered_articles.saturating_add(failed_articles);
    let new_failed_size = cur_recovered_failed_size.saturating_add(delta_size);
    let new_failed_par_size = if is_par_file {
        cur_recovered_failed_par_size.saturating_add(delta_size)
    } else {
        cur_recovered_failed_par_size
    };
    (0, new_articles, new_failed_size, new_failed_par_size)
}

/// Evaluates chunk-size backoff or piece skip when `ReadDonorInner` fails in
/// `StreamRepairController::PatchFromDonorSet`.
/// Returns `(action, new_size, new_pos, new_failed_pieces)`:
/// - `action == 1`: retry smaller chunk (`size = new_size`)
/// - `action == 2`: skip failed piece (`pos = new_pos`, `failedPieces = new_failed_pieces`)
/// - `action == 0`: abort hole (`failedPieces >= max_failed_pieces`)
pub fn stream_repair_patch_from_donor_set_step_failure(
    chunk: i64,
    min_chunk: i64,
    cur_size: i64,
    cur_pos: i64,
    cur_failed_pieces: i32,
    max_failed_pieces: i32,
) -> (i32, i64, i64, i32) {
    if chunk > min_chunk {
        let new_size = min_chunk.max(chunk / 4);
        return (1, new_size, cur_pos, cur_failed_pieces);
    }
    let next_failed = cur_failed_pieces + 1;
    if next_failed >= max_failed_pieces {
        (0, cur_size, cur_pos, next_failed)
    } else {
        (2, cur_size, cur_pos.saturating_add(chunk), next_failed)
    }
}

/// Computes the live remainder of an inner hole in `StreamRepairController::PatchEncryptedFromSource`,
/// clips it to `last_computable`, updates `uncomputable_tail`, and expands to cipher-block bounds.
/// Returns `(is_patchable, new_uncomputable_tail, hole_from, patch_to, block_from, block_to)`.
pub fn stream_repair_encrypted_clip_live_hole(
    hole_offset: i64,
    hole_size: i64,
    live_holes: &[crate::dupefallback::NzbgetRsByteRange],
    last_computable: i64,
    block_size: i64,
    cur_uncomputable_tail: i64,
) -> (bool, i64, i64, i64, i64, i64) {
    let hole_end = hole_offset.saturating_add(hole_size);
    let mut hole_from: i64 = -1;
    let mut hole_to: i64 = -1;
    for live in live_holes {
        let live_end = live.offset.saturating_add(live.size);
        let from = live.offset.max(hole_offset);
        let to = live_end.min(hole_end);
        if from < to {
            if hole_from < 0 {
                hole_from = from;
            }
            hole_to = to;
        }
    }
    if hole_from < 0 {
        return (false, cur_uncomputable_tail, -1, -1, 0, 0);
    }
    let patch_to = hole_to.min(last_computable);
    let new_tail = cur_uncomputable_tail + (hole_to - patch_to.max(hole_from));
    if patch_to <= hole_from || block_size <= 0 {
        return (false, new_tail, hole_from, patch_to, 0, 0);
    }
    let block_from = (hole_from / block_size) * block_size;
    let block_to = ((patch_to + block_size - 1) / block_size) * block_size;
    (true, new_tail, hole_from, patch_to, block_from, block_to)
}

/// Checks whether the predecessor cipher block `[block_from - block_size, block_from)` is
/// completely outside all current `live_holes` in `StreamRepairController::PatchEncryptedFromSource`.
pub fn stream_repair_encrypted_is_prev_block_present(
    block_from: i64,
    block_size: i64,
    live_holes: &[crate::dupefallback::NzbgetRsByteRange],
) -> bool {
    let prev_start = block_from - block_size;
    for live in live_holes {
        let live_end = live.offset.saturating_add(live.size);
        if !(live_end <= prev_start || live.offset >= block_from) {
            return false;
        }
    }
    true
}

/// Resolves post-stream-repair par-check and par-rename state transitions in
/// `StreamRepairController::RepairCompleted`.
/// Returns `(reset_par_rename, set_par_status, new_par_status, force_par_full, set_request_par_check_false, check_main_pars)`.
#[allow(clippy::too_many_arguments)]
pub fn stream_repair_resolve_completion_par_action(
    recovered_bytes: i64,
    holes_remain: bool,
    par_rename_status: i32,
    rs_none: i32,
    par_status: i32,
    ps_skipped: i32,
    par_check_opt: i32,
    pc_manual: i32,
    ps_manual: i32,
    ps_none: i32,
) -> (bool, bool, i32, bool, bool, bool) {
    let recovered = recovered_bytes > 0;
    if !recovered && !holes_remain {
        return (false, false, par_status, false, false, false);
    }
    let reset_par_rename = recovered && par_rename_status != rs_none;
    if par_status > ps_skipped {
        if recovered {
            let new_status = if par_check_opt == pc_manual {
                ps_manual
            } else {
                ps_none
            };
            (reset_par_rename, true, new_status, true, true, false)
        } else {
            (reset_par_rename, false, par_status, false, false, false)
        }
    } else {
        (reset_par_rename, false, par_status, recovered, false, true)
    }
}

/// Validates an extracted regular file's size and cumulative byte cap in
/// `DupeStreamRepair::SelectExtractedInner`.
/// Returns `(is_valid, new_extracted_bytes, is_size_match)`.
pub fn dupe_stream_repair_is_extracted_entry_valid(
    entry_count: usize,
    max_entries: usize,
    size: i64,
    max_bytes: i64,
    cur_extracted_bytes: i64,
    inner_size: i64,
) -> (bool, i64, bool) {
    if entry_count > max_entries || size < 0 || size > max_bytes - cur_extracted_bytes {
        return (false, cur_extracted_bytes, false);
    }
    let next_bytes = cur_extracted_bytes + size;
    (true, next_bytes, size == inner_size)
}

/// Computes the absolute difference between two file sizes for candidate sorting in
/// `DupeStreamRepair::SelectDonorCandidates`.
pub fn dupe_stream_repair_calc_abs_size_diff(size1: i64, size2: i64) -> i64 {
    if size1 > size2 {
        size1 - size2
    } else {
        size2 - size1
    }
}

/// Evaluates whether a `TwinCheckJob::Run` index fetch needs a retry round.
/// Returns `(should_increment_attempts, retry_later)`.
pub fn twin_check_eval_index_fetch_retry(
    fetched: bool,
    known: bool,
    complete: bool,
    next_attempts: i32,
    max_attempts: i32,
) -> bool {
    fetched && (!known || !complete) && next_attempts < max_attempts
}

/// Validates a fetched segment part in `TwinCheck::FetchEntry` and computes the required vector size.
/// Returns `(is_valid, required_len)`.
pub fn twin_check_is_fetched_entry_part_valid(
    success: bool,
    offset: i64,
    data_len: usize,
    max_size: i64,
    cur_data_len: usize,
) -> (bool, usize) {
    if !success || offset < 0 || offset.saturating_add(data_len as i64) > max_size {
        return (false, cur_data_len);
    }
    let end_len = (offset as usize).saturating_add(data_len);
    let new_len = cur_data_len.max(end_len);
    (true, new_len)
}

/// Checks whether a `TwinCheck` fingerprint string is usable (non-empty and not `"none"` or `"lost"`).
pub fn twin_check_is_print_usable(print: &[u8]) -> bool {
    !print.is_empty() && print != b"none" && print != b"lost"
}

/// Checks whether a `TwinCheck` item with a usable fingerprint is listless and eligible for re-check.
pub fn twin_check_is_listless_from_counts(
    has_sigs: i32,
    attempts: i32,
    max_attempts: i32,
) -> bool {
    has_sigs < 2 && attempts < max_attempts
}

/// Evaluates `sampleable` and `sampledNow` in `TwinCheck::ServiceWork`.
/// Returns `(sampleable, sampled_now)`.
#[allow(clippy::too_many_arguments)]
pub fn twin_check_eval_sampleable(
    primary_ok: bool,
    is_primary: bool,
    is_dupe: bool,
    print_empty: bool,
    primary_print_empty: bool,
    by_par2: bool,
    has_sampled: bool,
    sampled_of: Option<&[u8]>,
    primary_id: &[u8],
) -> (bool, bool) {
    let sampleable =
        primary_ok && !is_primary && is_dupe && !print_empty && !primary_print_empty && !by_par2;
    let sampled_now = has_sampled && sampled_of.is_some_and(|s| s == primary_id);
    (sampleable, sampled_now)
}

/// Resolves initial auto-sort order in `GroupSorter::Execute` (`scPriority` defaults to `soDescending`).
pub fn queue_editor_resolve_auto_sort_order(
    sort_order: i32,
    so_auto: i32,
    so_descending: i32,
    sort_criteria: i32,
    sc_priority: i32,
) -> i32 {
    if sort_order == so_auto && sort_criteria == sc_priority {
        so_descending
    } else {
        sort_order
    }
}

/// Flips sort order in `GroupSorter::Execute` when `soAuto` sort leaves the list unchanged.
pub fn queue_editor_flip_sort_order(
    sort_order: i32,
    so_descending: i32,
    so_ascending: i32,
) -> i32 {
    if sort_order == so_descending {
        so_ascending
    } else {
        so_descending
    }
}

/// Resolves the effective move offset in `QueueEditor::PrepareList` for `MoveTop`/`MoveBottom` actions.
pub fn queue_editor_resolve_prepare_list_offset(
    action: i32,
    ea_file_move_top: i32,
    ea_group_move_top: i32,
    ea_file_move_bottom: i32,
    ea_group_move_bottom: i32,
    max_id: i32,
    cur_offset: i32,
) -> i32 {
    if action == ea_file_move_top || action == ea_group_move_top {
        -max_id
    } else if action == ea_file_move_bottom || action == ea_group_move_bottom {
        max_id
    } else {
        cur_offset
    }
}

/// Computes `(start, end, step)` for the queue move sweep in `QueueEditor::PrepareList`.
pub fn queue_editor_calc_move_sweep_bounds(
    nr_entries: i32,
    offset: i32,
    le_zero_forward: bool,
) -> (i32, i32, i32) {
    let forward = if le_zero_forward {
        offset <= 0
    } else {
        offset < 0
    };
    if forward {
        (0, nr_entries, 1)
    } else {
        (nr_entries - 1, -1, -1)
    }
}

/// Evaluates whether a selected group in `QueueEditor::AlignGroups` needs to be moved
/// to `last_num + 1`, and returns `(should_move, new_last_num)`.
pub fn queue_editor_eval_align_group_step(
    selected: bool,
    has_last_nzb: bool,
    num: u32,
    last_num: u32,
) -> (bool, u32) {
    if !selected {
        return (false, last_num);
    }
    if has_last_nzb && num.saturating_sub(last_num) > 1 {
        (true, last_num + 1)
    } else {
        (false, num)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_repair_donor_member_helpers() {
        assert!(stream_repair_is_first_donor_part_valid(true, true, 0));
        assert!(!stream_repair_is_first_donor_part_valid(true, true, 512));
        assert!(!stream_repair_is_first_donor_part_valid(false, true, 0));

        assert_eq!(stream_repair_step_part_for_offset(100, 200, 100, 0, 5), (0, 0));
        assert_eq!(stream_repair_step_part_for_offset(100, 200, 100, 2, 5), (1, 1));
        assert_eq!(stream_repair_step_part_for_offset(350, 200, 100, 4, 5), (0, 4));
        assert_eq!(stream_repair_step_part_for_offset(350, 200, 100, 2, 5), (1, 3));
        assert_eq!(stream_repair_step_part_for_offset(250, 200, 100, 2, 5), (2, 2));

        assert_eq!(stream_repair_calc_prefetch_bounds(0, 4, 5), (0, 4));
        assert_eq!(stream_repair_calc_prefetch_bounds(2, 3, 10), (1, 4));

        assert_eq!(
            stream_repair_calc_donor_read_chunk(150, 100, 200, 250),
            (50, 100)
        );
        assert_eq!(
            stream_repair_calc_donor_read_chunk(150, 100, 200, 500),
            (50, 150)
        );
    }

    #[test]
    fn test_stream_repair_collect_and_bail_helpers() {
        assert!(stream_repair_is_par2_job_or_file(true, b"video.mkv"));
        assert!(stream_repair_is_par2_job_or_file(false, b"movie.PAR2"));
        assert!(!stream_repair_is_par2_job_or_file(false, b"movie.r00"));

        assert!(stream_repair_is_donor_nzb_pre_eligible(false, 0, 0, 0, 2));
        assert!(!stream_repair_is_donor_nzb_pre_eligible(true, 0, 0, 0, 2));
        assert!(!stream_repair_is_donor_nzb_pre_eligible(false, 1, 0, 0, 2));
        assert!(!stream_repair_is_donor_nzb_pre_eligible(false, 0, 0, 2, 2));

        assert!(stream_repair_should_skip_donor_candidate(42, 42, b"a.nzb"));
        assert!(!stream_repair_should_skip_donor_candidate(0, 0, b"a.nzb"));
        assert!(stream_repair_should_skip_donor_candidate(0, 0, b""));

        assert_eq!(
            stream_repair_eval_donor_outcome_bail(0, 0, 1, false, 2, 8, 3),
            (0, false)
        );
        assert_eq!(
            stream_repair_eval_donor_outcome_bail(1, 0, 1, false, 2, 8, 3),
            (3, true)
        );
        assert_eq!(
            stream_repair_eval_donor_outcome_bail(1, 0, 1, true, 2, 8, 3),
            (3, false)
        );
        assert_eq!(
            stream_repair_resolve_repair_file_outcome(true, true, 0, 1, 2),
            0
        );
        assert_eq!(
            stream_repair_resolve_repair_file_outcome(false, true, 0, 1, 2),
            1
        );
        assert_eq!(
            stream_repair_resolve_repair_file_outcome(false, false, 0, 1, 2),
            2
        );
    }

    #[test]
    fn test_stream_repair_probe_and_patch_helpers() {
        assert_eq!(stream_repair_calc_probe_replacement_count(2, 5, 8, 4), 2);
        assert_eq!(stream_repair_calc_probe_replacement_count(4, 5, 8, 6), 2);
        assert_eq!(stream_repair_pick_replacement_index(10, 0, 2), 2);
        assert_eq!(stream_repair_pick_replacement_index(10, 1, 2), 7);

        assert_eq!(
            stream_repair_is_probe_article_valid(false, 100, 1000, 0, 1000),
            (false, true)
        );
        assert_eq!(
            stream_repair_is_probe_article_valid(true, 100, 2000, 0, 1000),
            (false, false)
        );
        assert_eq!(
            stream_repair_is_probe_article_valid(true, 100, 1000, 950, 1000),
            (false, false)
        );
        assert_eq!(
            stream_repair_is_probe_article_valid(true, 100, 1000, 900, 1000),
            (true, false)
        );

        assert_eq!(
            stream_repair_clip_hole_to_fetched(100, 200, 150, 100),
            Some((150, 250))
        );
        assert_eq!(stream_repair_clip_hole_to_fetched(100, 50, 200, 100), None);
    }

    #[test]
    fn test_stream_repair_remaining_and_encrypted_helpers() {
        assert_eq!(
            stream_repair_classify_remaining_hole_target(
                false, 1000, true, true, 3, 500, 100, 2, 1000, 200
            ),
            (0, 5, 1600, 800)
        );
        assert_eq!(
            stream_repair_classify_remaining_hole_target(
                true, 1000, false, false, 3, 500, 100, 2, 1000, 200
            ),
            (1, 2, 1000, 200)
        );

        assert_eq!(
            stream_repair_patch_from_donor_set_step_failure(1024, 64, 1024, 0, 0, 8),
            (1, 256, 0, 0)
        );
        assert_eq!(
            stream_repair_patch_from_donor_set_step_failure(64, 64, 64, 100, 1, 8),
            (2, 64, 164, 2)
        );
        assert_eq!(
            stream_repair_patch_from_donor_set_step_failure(64, 64, 64, 100, 7, 8),
            (0, 64, 100, 8)
        );

        let live = [crate::dupefallback::NzbgetRsByteRange {
            offset: 20,
            size: 30,
        }];
        let (ok, tail, h_from, p_to, b_from, b_to) =
            stream_repair_encrypted_clip_live_hole(16, 40, &live, 48, 16, 0);
        assert!(ok);
        assert_eq!(tail, 2);
        assert_eq!((h_from, p_to, b_from, b_to), (20, 48, 16, 48));
        assert!(stream_repair_encrypted_is_prev_block_present(16, 16, &live));
        assert!(!stream_repair_encrypted_is_prev_block_present(32, 16, &live));

        assert_eq!(
            stream_repair_resolve_completion_par_action(100, false, 2, 0, 3, 1, 0, 2, 4, 0),
            (true, true, 0, true, true, false)
        );
        assert_eq!(
            stream_repair_resolve_completion_par_action(100, false, 0, 0, 0, 1, 0, 2, 4, 0),
            (false, false, 0, true, false, true)
        );
    }

    #[test]
    fn test_dupe_stream_twin_and_queue_editor_helpers() {
        assert_eq!(
            dupe_stream_repair_is_extracted_entry_valid(1, 10, 500, 1000, 200, 500),
            (true, 700, true)
        );
        assert_eq!(
            dupe_stream_repair_is_extracted_entry_valid(1, 10, 900, 1000, 200, 500),
            (false, 200, false)
        );
        assert_eq!(dupe_stream_repair_calc_abs_size_diff(100, 250), 150);

        assert!(twin_check_eval_index_fetch_retry(true, false, false, 1, 3));
        assert!(!twin_check_eval_index_fetch_retry(true, true, true, 1, 3));
        assert_eq!(
            twin_check_is_fetched_entry_part_valid(true, 100, 50, 1000, 120),
            (true, 150)
        );
        assert!(twin_check_is_print_usable(b"0123456789abcdef-12"));
        assert!(!twin_check_is_print_usable(b"none"));
        assert!(!twin_check_is_print_usable(b"lost"));
        assert!(twin_check_is_listless_from_counts(1, 0, 3));
        assert_eq!(
            twin_check_eval_sampleable(true, false, true, false, false, false, true, Some(b"7"), b"7"),
            (true, true)
        );

        assert_eq!(queue_editor_resolve_auto_sort_order(2, 2, 1, 5, 5), 1);
        assert_eq!(queue_editor_flip_sort_order(1, 1, 0), 0);
        assert_eq!(
            queue_editor_resolve_prepare_list_offset(2, 2, 12, 3, 13, 1000, 5),
            -1000
        );
        assert_eq!(
            queue_editor_calc_move_sweep_bounds(10, -1, false),
            (0, 10, 1)
        );
        assert_eq!(
            queue_editor_calc_move_sweep_bounds(10, 1, false),
            (9, -1, -1)
        );
        assert_eq!(
            queue_editor_eval_align_group_step(true, true, 5, 2),
            (true, 3)
        );
        assert_eq!(
            queue_editor_eval_align_group_step(true, true, 3, 2),
            (false, 3)
        );
    }
}
