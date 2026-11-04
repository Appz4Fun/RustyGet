// Copyright (C) 2026 Denis <denis@nzbget.com>
//
// This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Hot-path kernels for `ParChecker`, `UnpackController`, `DupeArticleFallback`,
//! `DupeCoordinator`, and `NzbFile`.

/// Accumulates the worst `ParChecker::EStatus` across multiple PAR2 sets in `ParChecker::RunParCheckAll`
/// (lower enum values represent worse status: `psFailed < psRepairPossible < psRepaired < psRepairNotNeeded`).
#[inline]
pub fn parchecker_accumulate_worst_status(all_status: i32, set_status: i32) -> i32 {
    if all_status > set_status {
        set_status
    } else {
        all_status
    }
}

/// Evaluates whether `ParChecker::RunParCheck` should attempt an additional recovery step
/// (`AddSplittedFragments`, `ProcessMorePars`, `AddDupeFiles`, or `ProcessTwinSet`).
#[inline]
pub fn parchecker_should_try_more_recovery(
    has_damaged_files: bool,
    is_stopped: bool,
    res: i32,
    e_repair_not_possible: i32,
    require_dupe_scan: bool,
    par_scan: i32,
    ps_dupe: i32,
) -> bool {
    has_damaged_files
        && !is_stopped
        && res == e_repair_not_possible
        && (!require_dupe_scan || par_scan == ps_dupe)
}

/// Evaluates the post-verify / post-repair status and cancellation state in `ParChecker::RunParCheck`.
/// Returns `(status, action_code, use_cmdline_err)`:
/// - `action_code`:
///   - `0`: repair not needed (`PrintMessage(mkInfo, "Repair not needed...")`)
///   - `1`: repair possible and `ParRepair` enabled (caller runs repair pass)
///   - `2`: repair possible and `ParRepair` disabled (`PrintMessage(mkInfo, "Repair possible...")`)
///   - `3`: repair failed (`status == ps_failed`)
#[inline]
pub fn parchecker_eval_post_verify_action(
    res: i32,
    e_success: i32,
    e_repair_possible: i32,
    has_damaged_files: bool,
    par_repair: bool,
    ps_failed: i32,
    ps_repair_not_needed: i32,
    ps_repair_possible: i32,
) -> (i32, i32) {
    if res == e_success || !has_damaged_files {
        (ps_repair_not_needed, 0)
    } else if res == e_repair_possible {
        if par_repair {
            (ps_repair_possible, 1)
        } else {
            (ps_repair_possible, 2)
        }
    } else {
        (ps_failed, 3)
    }
}

/// Evaluates cancellation or failure outcome at the end of `ParChecker::RunParCheck`.
/// Returns `(final_status, outcome_kind, use_cmdline_err_idx)`:
/// - `outcome_kind`:
///   - `0`: normal completion (no cancel or failure message)
///   - `1`: repair cancelled (`m_stage >= ptRepairing`)
///   - `2`: par-check cancelled (`m_stage < ptRepairing`)
///   - `3`: repair failed (`status == psFailed`)
#[inline]
pub fn parchecker_eval_cancel_or_fail_outcome(
    is_stopped: bool,
    stage: i32,
    pt_repairing: i32,
    cur_status: i32,
    ps_failed: i32,
    ps_repair_possible: i32,
    err_msg_empty: bool,
    res: i32,
) -> (i32, i32, bool) {
    if is_stopped {
        if stage >= pt_repairing {
            (ps_repair_possible, 1, false)
        } else {
            (ps_failed, 2, false)
        }
    } else if cur_status == ps_failed {
        let use_cmd_err = err_msg_empty && (0..=8).contains(&res);
        (ps_failed, 3, use_cmd_err)
    } else {
        (cur_status, 0, false)
    }
}

/// Determines whether a libpar2 sourcefile should be recorded in `m_verifiedFiles` at the end of `ParChecker::RunParCheck`.
#[inline]
pub fn parchecker_is_verified_complete_target(
    status: i32,
    ps_repaired: i32,
    ps_repair_not_needed: i32,
    has_complete_file: bool,
    complete_eq_target: bool,
) -> bool {
    (status == ps_repaired || status == ps_repair_not_needed)
        && has_complete_file
        && complete_eq_target
}

/// Evaluates `ParChecker::AddDupeFiles` block deltas after scanning a dupe directory.
/// Returns `(new_used_blocks, total_extra_blocks_found)`.
#[inline]
pub fn parchecker_calc_dupe_blocks_delta(
    prev_used_blocks: i32,
    was_blocks_missing2: u32,
    blocks_missing2: u32,
    was_blocks_missing_total: u32,
) -> (i32, u32) {
    let delta2 = was_blocks_missing2.wrapping_sub(blocks_missing2) as i32;
    let new_used = prev_used_blocks.wrapping_add(delta2);
    let total_found = if blocks_missing2 < was_blocks_missing_total {
        was_blocks_missing_total - blocks_missing2
    } else {
        0
    };
    (new_used, total_found)
}

/// Evaluates `ParChecker::AddExtraFiles` step outcome for one extra file.
/// Returns `(file_added, block_added, new_files_added, missing_blocks_delta, register_parred)`.
#[inline]
pub fn parchecker_eval_extra_file_step(
    was_files_missing: u32,
    now_files_missing: u32,
    was_blocks_missing: u32,
    now_blocks_missing: u32,
    external_dir: bool,
    prev_files_added: bool,
) -> (bool, bool, bool, u32, bool) {
    let file_added = was_files_missing > now_files_missing;
    let block_added = was_blocks_missing > now_blocks_missing;
    let new_files_added = prev_files_added || file_added || block_added;
    let blocks_delta = if block_added {
        was_blocks_missing - now_blocks_missing
    } else {
        0
    };
    let register_parred = file_added && !external_dir;
    (
        file_added,
        block_added,
        new_files_added,
        blocks_delta,
        register_parred,
    )
}

/// Evaluates `ParChecker::SignalDone` damaged-file state when `m_stage == ptVerifyingSources`.
/// Returns `(should_report, new_has_damaged_files, bad_blocks)`.
#[inline]
pub fn parchecker_eval_signal_done(
    stage: i32,
    pt_verifying_sources: i32,
    available: i32,
    total: i32,
    verifying_extra_files: bool,
    ignore: bool,
    prev_has_damaged_files: bool,
) -> (bool, bool, i32) {
    if stage == pt_verifying_sources && available < total && !verifying_extra_files {
        (
            true,
            prev_has_damaged_files || !ignore,
            total.wrapping_sub(available),
        )
    } else {
        (false, prev_has_damaged_files, 0)
    }
}

/// Evaluates the initial preconditions in `ParChecker::VerifyDataFile`.
/// Returns:
/// - `0`: proceed with CRC lookup
/// - `1`: return `fsSuccess` (`m_stage != ptVerifyingSources`)
/// - `2`: return `fsUnknown` (`!targetExists || !hasVerificationPacket`)
/// - `3`: return `fsFailure` and set `availableBlocks = 0` (`fileSize == 0 && blockCount > 0`)
#[inline]
pub fn parchecker_check_verify_data_file_preconditions(
    stage: i32,
    pt_verifying_sources: i32,
    target_exists: bool,
    has_verification_packet: bool,
    file_size: i64,
    block_count: u64,
) -> i32 {
    if stage != pt_verifying_sources {
        return 1;
    }
    if !target_exists || !has_verification_packet {
        return 2;
    }
    if file_size == 0 && block_count > 0 {
        return 3;
    }
    0
}

/// Computes the chunk size to read in `ParChecker::DumbCalcFileRangeCrc`.
/// Returns `0` when the loop should terminate (`cnt != buffer_size || start >= end`).
#[inline]
pub fn parchecker_calc_dumb_crc_read_size(
    cnt: i32,
    buffer_size: i32,
    start: i64,
    end: i64,
) -> i32 {
    if cnt != buffer_size || start >= end {
        return 0;
    }
    let rem = end.wrapping_sub(start).wrapping_add(1);
    if rem > buffer_size as i64 {
        buffer_size
    } else {
        rem as i32
    }
}

/// Classifies the top-level action in `UnpackController::Run` when not skipped by second-attempt guard:
/// - `1`: unpack archives (`unpack && has_files`)
/// - `2`: nothing to unpack, request par-check (`unpack && !has_files && par_status <= ps_skipped && par_rename_status <= rs_skipped && has_par_files`)
/// - `3`: fail due to renamed archive files (`unpack && !has_files && has_renamed_archive_files`)
/// - `4`: nothing to unpack, set `usSkipped` (`unpack && !has_files`)
/// - `5`: unpack skipped (`!unpack`)
#[inline]
pub fn unpack_ctrl_classify_run_branch(
    unpack: bool,
    has_files: bool,
    par_status: i32,
    ps_skipped: i32,
    par_rename_status: i32,
    rs_skipped: i32,
    has_par_files: bool,
    has_renamed_archive_files: bool,
) -> i32 {
    if unpack && has_files {
        1
    } else if unpack {
        if par_status <= ps_skipped && par_rename_status <= rs_skipped && has_par_files {
            2
        } else if has_renamed_archive_files {
            3
        } else {
            4
        }
    } else {
        5
    }
}

/// Evaluates RAR unpack mode in `UnpackController::Run`:
/// Returns `(run_unrar, log_kind)`:
/// - `log_kind`:
///   - `0`: no special direct-unpack log
///   - `1`: warn `"Could not find files unpacked by direct unpack, unpacking all files again"`
///   - `2`: info `"Found archive files not processed by direct unpack, unpacking all files again"`
///   - `3`: info `"Using directly unpacked files"`
#[inline]
pub fn unpack_ctrl_eval_rar_unpack_mode(
    has_not_unpacked_rar_files: bool,
    unpack_dir_created: bool,
    direct_unpack_status: i32,
    ns_success: i32,
) -> (bool, i32) {
    if has_not_unpacked_rar_files || unpack_dir_created {
        let log_kind = if direct_unpack_status == ns_success {
            if unpack_dir_created {
                1
            } else {
                2
            }
        } else {
            0
        };
        (true, log_kind)
    } else {
        (false, 3)
    }
}

/// Determines whether `UnpackController::ExecuteUnrar` / `ExecuteSevenZip` should log a non-zero exit code error.
#[inline]
pub fn unpack_ctrl_should_log_exit_error(
    unpack_ok: bool,
    exit_code: i32,
    will_retry_with_pass: bool,
    is_stopped: bool,
    is_terminated: bool,
) -> bool {
    !unpack_ok && exit_code > 0 && !will_retry_with_pass && !is_stopped && !is_terminated
}

/// Updates `(count, min, max, dif_size_min)` when scanning a split file piece in `UnpackController::JoinFile`.
/// Returns `true` if `filename` belongs to `dest_base_name` (case-insensitive prefix + `.NNN` where `N` are digits).
#[inline]
pub fn unpack_ctrl_accumulate_split_piece(
    filename: &[u8],
    dest_base_name: &[u8],
    segment_size: i64,
    first_segment_size: i64,
    count: &mut i32,
    min: &mut i32,
    max: &mut i32,
    dif_size_min: &mut i32,
) -> bool {
    let base_len = dest_base_name.len();
    if filename.len() != base_len + 4 {
        return false;
    }
    if !filename[..base_len].eq_ignore_ascii_case(dest_base_name) {
        return false;
    }
    if filename[base_len] != b'.' {
        return false;
    }
    let ext = &filename[base_len + 1..];
    if !ext.iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    //Also verify the 4 chars before `.NNN` match `\.[a-z,0-9]{3}`
    if base_len < 4 || dest_base_name[base_len - 4] != b'.' {
        return false;
    }
    let prev_ext = &dest_base_name[base_len - 3..];
    if !prev_ext
        .iter()
        .all(|b| b.is_ascii_alphabetic() || b.is_ascii_digit() || *b == b',')
    {
        return false;
    }

    let seg_num =
        ((ext[0] - b'0') as i32) * 100 + ((ext[1] - b'0') as i32) * 10 + ((ext[2] - b'0') as i32);
    *count = count.wrapping_add(1);
    if *min == -1 || seg_num < *min {
        *min = seg_num;
    }
    if seg_num > *max {
        *max = seg_num;
    }
    if segment_size != first_segment_size && seg_num < *dif_size_min {
        *dif_size_min = seg_num;
    }
    true
}

/// Evaluates whether a file in `m_destDir` should be deleted during `UnpackController::Cleanup` (`UnpackCleanupDisk`).
/// Returns:
/// - `0`: do not delete
/// - `1`: delete immediately (`.rar`, `.7z`, `.7z.NNN`, or joined split file)
/// - `2`: delete if `FileHasRarSignature(fullFilename)` is true (`.[r-z][0-9][0-9]`)
#[inline]
pub fn unpack_ctrl_classify_cleanup_archive_file(
    is_dir: bool,
    inter_dir: bool,
    in_extracted_files: bool,
    is_downloaded_file: bool,
    has_splitted_files: bool,
    in_joined_files: bool,
    filename: &[u8],
) -> i32 {
    if is_dir || (!inter_dir && in_extracted_files) || !is_downloaded_file {
        return 0;
    }
    let len = filename.len();
    if len >= 4 && filename[len - 4..].eq_ignore_ascii_case(b".rar") {
        return 1;
    }
    if len >= 3 && filename[len - 3..].eq_ignore_ascii_case(b".7z") {
        return 1;
    }
    // Check `.*\.7z\.[0-9]+$`
    if let Some(dot_pos) = filename.iter().rposition(|&b| b == b'.') {
        let digits = &filename[dot_pos + 1..];
        if !digits.is_empty()
            && digits.iter().all(|b| b.is_ascii_digit())
            && dot_pos >= 3
            && filename[dot_pos - 3..dot_pos].eq_ignore_ascii_case(b".7z")
        {
            return 1;
        }
    }
    // Check `.*\.[r-z][0-9][0-9]$`
    if len >= 4
        && filename[len - 4] == b'.'
        && (b'r'..=b'z').contains(&filename[len - 3].to_ascii_lowercase())
        && filename[len - 2].is_ascii_digit()
        && filename[len - 1].is_ascii_digit()
    {
        return 2;
    }
    // Check `.*\.[a-z,0-9]{3}\.[0-9]{3}$` when `has_splitted_files && in_joined_files`
    if has_splitted_files
        && in_joined_files
        && len >= 8
        && filename[len - 4] == b'.'
        && filename[len - 8] == b'.'
        && filename[len - 3..].iter().all(|b| b.is_ascii_digit())
        && filename[len - 7..len - 4]
            .iter()
            .all(|b| b.is_ascii_alphabetic() || b.is_ascii_digit() || *b == b',')
    {
        return 1;
    }
    0
}

/// Evaluates header-level conditions in `DupeArticleFallback::ShouldDeferToPar`.
/// Returns:
/// - `1`: return `true` immediately
/// - `-1`: return `false` immediately
/// - `0`: inspect `FileList` and `CompletedFiles`
#[inline]
pub fn dupe_fallback_should_defer_to_par_header(
    has_nzb_info: bool,
    direct_rename_status: i32,
    ts_running: i32,
    all_first: bool,
    par_status: i32,
    ps_failure: i32,
    par_size: i64,
) -> i32 {
    if !has_nzb_info {
        return -1;
    }
    if direct_rename_status == ts_running && !all_first {
        return 1;
    }
    if par_status == ps_failure {
        return -1;
    }
    if par_size > 0 {
        return 1;
    }
    0
}

/// Evaluates whether a finished or queued article counts as a borrowed donor article in `DupeArticleFallback::BorrowedPar2Mismatches`.
#[inline]
pub fn dupe_fallback_is_borrowed_article(donor_id: i32, fallback_round: i32) -> bool {
    donor_id > 0 || fallback_round > 0
}

/// Validates PAR2 block size and CRC count in `DupeArticleFallback::BorrowedPar2Mismatches`.
#[inline]
pub fn dupe_fallback_is_valid_par2_block_spec(
    has_sums: bool,
    block_size: u64,
    max_par2_block_size: u64,
    crc_count: usize,
    file_size: i64,
) -> bool {
    if !has_sums || block_size == 0 || block_size % 4 != 0 || block_size > max_par2_block_size {
        return false;
    }
    let needed_blocks = ((file_size as u64).wrapping_add(block_size - 1)) / block_size;
    (crc_count as u64) >= needed_blocks
}

/// Computes `(first_block, last_block)` for a borrowed article in `DupeArticleFallback::BorrowedPar2Mismatches`.
#[inline]
pub fn dupe_fallback_calc_article_block_range(
    segment_offset: i64,
    segment_size: i32,
    block_size: u64,
) -> (i64, i64) {
    let bs = block_size as i64;
    if bs <= 0 || segment_size <= 0 {
        return (0, -1);
    }
    let first = segment_offset / bs;
    let last = (segment_offset + (segment_size as i64) - 1) / bs;
    (first, last)
}

/// Computes `(lead_snapshot, next_lead, donor_count, new_file_lead_id)` from `contributors` in `DupeArticleFallback::FinishPin`.
pub fn dupe_fallback_compute_pin_lead_state(
    contributors: &[i32],
    cur_file_lead_donor_id: i32,
) -> (i32, i32, i32, i32) {
    let mut donor_ids: Vec<i32> = Vec::new();
    for &id in contributors {
        if !donor_ids.contains(&id) {
            donor_ids.push(id);
        }
    }
    let lead_snapshot = donor_ids.first().copied().unwrap_or(0);
    let next_lead = donor_ids.get(1).copied().unwrap_or(0);
    let donor_count = donor_ids.len() as i32;
    let new_file_lead = if cur_file_lead_donor_id == 0 && !donor_ids.is_empty() {
        donor_ids[0]
    } else {
        cur_file_lead_donor_id
    };
    (lead_snapshot, next_lead, donor_count, new_file_lead)
}

/// Determines whether `DupeArticleFallback::VacateGhostLead` should clear the file's lead donor state.
#[inline]
pub fn dupe_fallback_should_vacate_ghost_lead(
    lead_nzb_id: i32,
    is_barred: bool,
    in_donors: bool,
) -> bool {
    if lead_nzb_id == 0 || (!is_barred && in_donors) {
        false
    } else {
        true
    }
}

/// Compares two donor NZBs in `DupeArticleFallback::CollectDonors` (`true` if `donor1` sorts before `donor2`).
#[inline]
pub fn dupe_fallback_compare_donors(
    twin1: bool,
    score1: i32,
    id1: i32,
    twin2: bool,
    score2: i32,
    id2: i32,
) -> bool {
    if twin1 != twin2 {
        return twin1;
    }
    score1 > score2 || (score1 == score2 && id1 < id2)
}

/// Evaluates whether a history item causes `DupeCoordinator::NzbFound` to skip the new NZB
/// (after content-hash checks).
/// Returns `(skip, good)`.
#[inline]
pub fn dupecoord_check_history_good_or_score_skip(
    is_hk_nzb: bool,
    is_hk_dup: bool,
    hist_dupe_mode: i32,
    dm_force: i32,
    hist_mark_status: i32,
    ks_good: i32,
    hist_dup_status: i32,
    ds_good: i32,
    ds_success: i32,
    nzb_dupe_mode: i32,
    dm_score: i32,
    nzb_dupe_score: i32,
    hist_dupe_score: i32,
    same_name_or_key: bool,
) -> (bool, bool) {
    if !same_name_or_key || hist_dupe_mode == dm_force {
        return (false, false);
    }
    if is_hk_nzb && hist_mark_status == ks_good {
        return (true, true);
    }
    if is_hk_dup {
        if hist_dup_status == ds_good {
            return (true, true);
        }
        if nzb_dupe_mode == dm_score
            && hist_dup_status == ds_success
            && nzb_dupe_score <= hist_dupe_score
        {
            return (true, false);
        }
    }
    (false, false)
}

/// Classifies a same-content history item in `DupeCoordinator::NzbFound` (`HealthCheck=dupe`).
/// Returns `(succeeded, failed)`:
/// - `(true, false)` if the history item succeeded
/// - `(false, true)` if the history item failed
/// - `(false, false)` if the history item is an untried backup or manually deleted
#[inline]
pub fn dupecoord_classify_same_content_history_item(
    is_hk_nzb: bool,
    is_hk_dup: bool,
    same_hash: bool,
    delete_status: i32,
    ds_dupe: i32,
    ds_copy: i32,
    ds_manual: i32,
    is_dupe_success: bool,
    dup_status: i32,
    dup_ds_success: i32,
    dup_ds_good: i32,
    dup_ds_failed: i32,
    dup_ds_bad: i32,
) -> (bool, bool) {
    if !same_hash {
        return (false, false);
    }
    if is_hk_nzb {
        if delete_status == ds_dupe || delete_status == ds_copy || delete_status == ds_manual {
            return (false, false);
        }
        if is_dupe_success {
            (true, false)
        } else {
            (false, true)
        }
    } else if is_hk_dup {
        if dup_status == dup_ds_success || dup_status == dup_ds_good {
            (true, false)
        } else if dup_status == dup_ds_failed || dup_status == dup_ds_bad {
            (false, true)
        } else {
            (false, false)
        }
    } else {
        (false, false)
    }
}

/// Determines how `DupeCoordinator::NzbFound` handles a skipped duplicate.
/// Returns `(delete_status, delete_disk_files_and_warn)`:
/// - `delete_disk_files_and_warn == true`: feed item (`feed_id != 0 && dupe_hint == dh_none`), sets `ds_manual` and deletes disk files
/// - `delete_disk_files_and_warn == false`: sets `ds_copy` (if `same_content`) or `ds_good` and adds warning message
#[inline]
pub fn dupecoord_resolve_skip_action(
    feed_id: i32,
    dupe_hint: i32,
    dh_none: i32,
    same_content: bool,
    ds_manual: i32,
    ds_copy: i32,
    ds_good: i32,
) -> (i32, bool) {
    if feed_id != 0 && dupe_hint == dh_none {
        (ds_manual, true)
    } else if same_content {
        (ds_copy, false)
    } else {
        (ds_good, false)
    }
}

/// Accumulates `DupeCoordinator::EDupeStatus` bits for a matching queue item in `DupeCoordinator::GetDupeStatus`.
#[inline]
pub fn dupecoord_accum_queue_dupe_status(
    cur_statuses: i32,
    same_name_or_key: bool,
    success_articles: i32,
    failed_articles: i32,
    ds_downloading: i32,
    ds_queued: i32,
) -> i32 {
    if !same_name_or_key {
        return cur_statuses;
    }
    if success_articles.wrapping_add(failed_articles) > 0 {
        cur_statuses | ds_downloading
    } else {
        cur_statuses | ds_queued
    }
}

/// Accumulates `DupeCoordinator::EDupeStatus` bits for a matching `hkDup` history item in `DupeCoordinator::GetDupeStatus`.
#[inline]
pub fn dupecoord_accum_dup_history_status(
    cur_statuses: i32,
    same_name_or_key: bool,
    dup_status: i32,
    dup_ds_success: i32,
    dup_ds_good: i32,
    dup_ds_failed: i32,
    dup_ds_bad: i32,
    ds_success: i32,
    ds_failure: i32,
) -> i32 {
    if !same_name_or_key {
        return cur_statuses;
    }
    if dup_status == dup_ds_success || dup_status == dup_ds_good {
        cur_statuses | ds_success
    } else if dup_status == dup_ds_failed || dup_status == dup_ds_bad {
        cur_statuses | ds_failure
    } else {
        cur_statuses
    }
}

/// Detects whether `filename` contains `.par2` (case-insensitive) and accumulates per-file counters into `NzbInfo`
/// during `NzbFile::ProcessFiles`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn nzbfile_accumulate_file_stats(
    filename: &[u8],
    file_total_articles: i32,
    file_missed_articles: i32,
    file_size: i64,
    file_remaining_size: i64,
    file_missed_size: i64,
    nzb_file_count: &mut i32,
    nzb_total_articles: &mut i32,
    nzb_failed_articles: &mut i32,
    nzb_cur_failed_articles: &mut i32,
    nzb_size: &mut i64,
    nzb_remaining_size: &mut i64,
    nzb_failed_size: &mut i64,
    nzb_cur_failed_size: &mut i64,
    nzb_par_size: &mut i64,
    nzb_par_failed_size: &mut i64,
    nzb_par_cur_failed_size: &mut i64,
    nzb_remaining_par_count: &mut i32,
) -> bool {
    let is_par_file = filename
        .windows(5)
        .any(|w| w.eq_ignore_ascii_case(b".par2"));

    *nzb_file_count = nzb_file_count.wrapping_add(1);
    *nzb_total_articles = nzb_total_articles.wrapping_add(file_total_articles);
    *nzb_failed_articles = nzb_failed_articles.wrapping_add(file_missed_articles);
    *nzb_cur_failed_articles = nzb_cur_failed_articles.wrapping_add(file_missed_articles);
    *nzb_size = nzb_size.wrapping_add(file_size);
    *nzb_remaining_size = nzb_remaining_size.wrapping_add(file_remaining_size);
    *nzb_failed_size = nzb_failed_size.wrapping_add(file_missed_size);
    *nzb_cur_failed_size = *nzb_failed_size;

    if is_par_file {
        *nzb_par_size = nzb_par_size.wrapping_add(file_size);
        *nzb_par_failed_size = nzb_par_failed_size.wrapping_add(file_missed_size);
        *nzb_par_cur_failed_size = *nzb_par_failed_size;
        *nzb_remaining_par_count = nzb_remaining_par_count.wrapping_add(1);
    }

    is_par_file
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parchecker_and_unpack_kernels() {
        assert_eq!(parchecker_accumulate_worst_status(3, 1), 1);
        assert_eq!(parchecker_accumulate_worst_status(1, 3), 1);
        assert!(parchecker_should_try_more_recovery(
            true, false, 2, 2, true, 3, 3
        ));
        assert!(!parchecker_should_try_more_recovery(
            true, false, 2, 2, true, 1, 3
        ));
        assert_eq!(
            parchecker_eval_post_verify_action(0, 0, 1, true, true, 0, 3, 1),
            (3, 0)
        );
        assert_eq!(
            parchecker_eval_post_verify_action(1, 0, 1, true, true, 0, 3, 1),
            (1, 1)
        );
        assert_eq!(
            parchecker_eval_cancel_or_fail_outcome(true, 2, 2, 0, 0, 1, true, 2),
            (1, 1, false)
        );
        assert_eq!(
            parchecker_eval_cancel_or_fail_outcome(false, 1, 2, 0, 0, 1, true, 2),
            (0, 3, true)
        );

        let mut count = 0;
        let mut min = -1;
        let mut max = -1;
        let mut dif_min = 999999;
        assert!(unpack_ctrl_accumulate_split_piece(
            b"movie.mkv.001",
            b"movie.mkv",
            1000,
            1000,
            &mut count,
            &mut min,
            &mut max,
            &mut dif_min,
        ));
        assert!(unpack_ctrl_accumulate_split_piece(
            b"movie.mkv.002",
            b"movie.mkv",
            500,
            1000,
            &mut count,
            &mut min,
            &mut max,
            &mut dif_min,
        ));
        assert_eq!((count, min, max, dif_min), (2, 1, 2, 2));

        assert_eq!(
            unpack_ctrl_classify_cleanup_archive_file(
                false, false, false, true, false, false, b"part1.rar"
            ),
            1
        );
        assert_eq!(
            unpack_ctrl_classify_cleanup_archive_file(
                false, false, false, true, false, false, b"part1.r00"
            ),
            2
        );
    }

    #[test]
    fn test_dupe_and_nzbfile_kernels() {
        let (lead, next, cnt, file_lead) =
            dupe_fallback_compute_pin_lead_state(&[10, 10, 20, 30], 0);
        assert_eq!((lead, next, cnt, file_lead), (10, 20, 3, 10));
        assert!(dupe_fallback_compare_donors(true, 100, 5, false, 999, 1));
        assert!(dupe_fallback_compare_donors(false, 200, 5, false, 100, 1));

        let mut fc = 0;
        let mut ta = 0;
        let mut fa = 0;
        let mut cfa = 0;
        let mut sz = 0;
        let mut rsz = 0;
        let mut fsz = 0;
        let mut cfsz = 0;
        let mut psz = 0;
        let mut pfsz = 0;
        let mut pcfsz = 0;
        let mut rpc = 0;
        let is_par = nzbfile_accumulate_file_stats(
            b"Release.vol01+02.PAR2",
            10,
            1,
            10000,
            9000,
            1000,
            &mut fc,
            &mut ta,
            &mut fa,
            &mut cfa,
            &mut sz,
            &mut rsz,
            &mut fsz,
            &mut cfsz,
            &mut psz,
            &mut pfsz,
            &mut pcfsz,
            &mut rpc,
        );
        assert!(is_par);
        assert_eq!((fc, ta, fa, cfa), (1, 10, 1, 1));
        assert_eq!((sz, rsz, fsz, cfsz), (10000, 9000, 1000, 1000));
        assert_eq!((psz, pfsz, pcfsz, rpc), (10000, 1000, 1000, 1));
    }
}
