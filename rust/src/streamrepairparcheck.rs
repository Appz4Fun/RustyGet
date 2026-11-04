//! StreamRepair, ContentMap, ParChecker, and RepairController hot-path helpers (Batch 41).

/// Returns whether `member_index` is within `[0, entries_len)`.
pub fn srp_stream_repair_is_member_index_in_bounds(
    member_index: i32,
    entries_len: usize,
) -> bool {
    member_index >= 0 && (member_index as usize) < entries_len
}

/// Returns the effective member size as the maximum of on-disk size and declared member size.
pub fn srp_stream_repair_max_disk_and_member_size(
    disk_size: i64,
    member_size: i64,
) -> i64 {
    disk_size.max(member_size)
}

/// Returns whether `DonorMemberSource::FetchPart` can proceed for `part_index`.
pub fn srp_stream_repair_donor_fetch_part_can_proceed(
    part_index: i32,
    articles_len: usize,
    is_bad: bool,
) -> bool {
    !is_bad && part_index >= 0 && (part_index as usize) < articles_len
}

/// Evaluates and decrements an optional donor fetch budget.
/// Returns `true` when the fetch is allowed, writing the updated budget to `out_new_budget`.
pub fn srp_stream_repair_donor_consume_fetch_budget(
    has_budget: bool,
    current_budget: i32,
    out_new_budget: &mut i32,
) -> bool {
    *out_new_budget = current_budget;
    if !has_budget {
        return true;
    }
    if current_budget <= 0 {
        return false;
    }
    *out_new_budget = current_budget - 1;
    true
}

/// Returns whether `DonorMemberSource::Prefetch` can proceed for a read range.
pub fn srp_stream_repair_can_prefetch_donor_range(
    has_batch_fetcher: bool,
    has_fetch_budget: bool,
    is_bad: bool,
    ranges_empty: bool,
    size: i64,
) -> bool {
    has_batch_fetcher && !has_fetch_budget && !is_bad && !ranges_empty && size > 0
}

/// Returns whether cross-packing stream repair should run after M1 direct stream repair.
pub fn srp_stream_repair_should_run_cross_pack(
    any_holes: bool,
    is_stopped: bool,
) -> bool {
    any_holes && !is_stopped
}

/// Returns whether `RepairCompletedLive` should clear `postInfo->SetWorking(false)`.
pub fn srp_stream_repair_should_clear_post_working_after_live(
    has_post_info: bool,
    has_unpack_thread: bool,
) -> bool {
    has_post_info && !has_unpack_thread
}

/// Returns whether a donor file should be included in the current `DonorDead` pass (`0` = data only, `1` = all).
pub fn srp_stream_repair_should_include_file_in_donor_dead_pass(
    total_articles: i32,
    pass: i32,
    is_par_file: bool,
) -> bool {
    total_articles > 0 && (pass == 1 || !is_par_file)
}

/// Returns whether `DonorDead` has enough sample articles to run a live server check.
pub fn srp_stream_repair_should_check_donor_dead_samples(
    samples_len: usize,
    min_articles: i32,
    is_stopped: bool,
) -> bool {
    !is_stopped && min_articles >= 0 && samples_len >= min_articles as usize
}

/// Returns whether a repair target is eligible for the current `ExecRepair` round (`0` = partial holes, `1` = whole file).
pub fn srp_stream_repair_is_eligible_target_for_round(
    patch_eligible: bool,
    needs_repair: bool,
    decoded_file_size: i64,
    round: i32,
) -> bool {
    let whole_file = decoded_file_size <= 0;
    patch_eligible && needs_repair && (whole_file == (round == 1))
}

/// Returns whether `RepairFile` should stop probing further sibling donor candidates once one member verified.
pub fn srp_stream_repair_should_stop_sibling_donor_probes(
    verified: bool,
    probed: i32,
    max_donor_candidates: i32,
) -> bool {
    verified && probed >= max_donor_candidates
}

/// Returns whether a fetched donor article in `PatchFromDonor` is valid and within the target's decoded size.
pub fn srp_stream_repair_is_patch_article_usable(
    success: bool,
    fetched_file_size: i64,
    target_decoded_size: i64,
    data_len: usize,
    fetched_offset: i64,
) -> bool {
    if !success
        || fetched_file_size != target_decoded_size
        || data_len == 0
        || fetched_offset < 0
    {
        return false;
    }
    match i64::try_from(data_len).ok().and_then(|len| fetched_offset.checked_add(len)) {
        Some(end) => end <= target_decoded_size,
        None => false,
    }
}

/// Returns whether `ExecCrossPackRepair` should retry building a donor map with the donor's password.
pub fn srp_stream_repair_should_retry_donor_map_with_password(
    has_map: bool,
    password_empty: bool,
    skip_is_encrypted_data: bool,
    skip_is_encrypted_headers: bool,
) -> bool {
    !has_map && !password_empty && (skip_is_encrypted_data || skip_is_encrypted_headers)
}

/// Returns whether a target `ContentMap` and extracted donor inner file match encryption and size requirements.
pub fn srp_stream_repair_can_verify_donor_inner_file(
    is_encrypted: bool,
    expect_encrypted: bool,
    donor_inner_size: i64,
    target_inner_size: i64,
) -> bool {
    (is_encrypted == expect_encrypted) && donor_inner_size == target_inner_size
}

/// Classifies the completion mark action for a `RepairTarget` in `RepairCompleted`:
/// - `1`: call `MarkRepaired` (`decoded_file_size > 0 && holes_empty`)
/// - `2`: call `MarkRecreated` (`recreated`)
/// - `0`: no mark action
pub fn srp_stream_repair_classify_completed_target_mark(
    decoded_file_size: i64,
    holes_empty: bool,
    recreated: bool,
) -> i32 {
    if decoded_file_size > 0 && holes_empty {
        1
    } else if recreated {
        2
    } else {
        0
    }
}

/// Returns whether any stream-repair recovery counters were incremented during the pass.
pub fn srp_stream_repair_has_any_recovered_stats(
    recovered_articles: i32,
    recovered_bytes: i64,
    recovered_holes: i32,
) -> bool {
    recovered_articles > 0 || recovered_bytes > 0 || recovered_holes > 0
}

/// Returns whether `Repairer::ScanDataFile` can attempt quick PAR verification.
pub fn srp_parchecker_can_quick_scan_data_file(
    par_quick: bool,
    has_sourcefile: bool,
    has_diskfile: bool,
) -> bool {
    par_quick && has_sourcefile && has_diskfile
}

/// Evaluates how `ParChecker::LoadMainParBak` / `ProcessMorePars` should proceed after `RequestMorePars`:
/// - `0`: neither requested nor has more queued pars (abort/break)
/// - `1`: requested but no queued pars yet (wait for `m_queuedParFilesChanged`)
/// - `2`: queued pars are immediately available
pub fn srp_parchecker_should_wait_for_queued_pars(
    requested: bool,
    has_more_pars: bool,
) -> i32 {
    if !requested && !has_more_pars {
        0
    } else if !has_more_pars {
        1
    } else {
        2
    }
}

/// Returns whether a non-directory entry in `ParChecker::FindExtraFiles` should be added to `extrafiles`.
pub fn srp_parchecker_should_add_extra_file_entry(
    external_dir: bool,
    is_parred: bool,
    is_processed: bool,
) -> bool {
    external_dir || (!is_parred && !is_processed)
}

/// Returns whether a parsed PAR2 file with `block_count` is a usable recovery-block candidate in `RepairController::FindPars`.
pub fn srp_repair_ctrl_is_usable_par_block_candidate(
    parsed: bool,
    block_count: i32,
) -> bool {
    parsed && block_count > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_repair_source_and_budget_helpers() {
        assert!(srp_stream_repair_is_member_index_in_bounds(0, 3));
        assert!(srp_stream_repair_is_member_index_in_bounds(2, 3));
        assert!(!srp_stream_repair_is_member_index_in_bounds(-1, 3));
        assert!(!srp_stream_repair_is_member_index_in_bounds(3, 3));

        assert_eq!(srp_stream_repair_max_disk_and_member_size(100, 250), 250);
        assert_eq!(srp_stream_repair_max_disk_and_member_size(500, 250), 500);

        assert!(srp_stream_repair_donor_fetch_part_can_proceed(1, 5, false));
        assert!(!srp_stream_repair_donor_fetch_part_can_proceed(-1, 5, false));
        assert!(!srp_stream_repair_donor_fetch_part_can_proceed(5, 5, false));
        assert!(!srp_stream_repair_donor_fetch_part_can_proceed(1, 5, true));

        let mut new_budget = 0;
        assert!(srp_stream_repair_donor_consume_fetch_budget(false, 0, &mut new_budget));
        assert_eq!(new_budget, 0);
        assert!(!srp_stream_repair_donor_consume_fetch_budget(true, 0, &mut new_budget));
        assert_eq!(new_budget, 0);
        assert!(srp_stream_repair_donor_consume_fetch_budget(true, 4, &mut new_budget));
        assert_eq!(new_budget, 3);

        assert!(srp_stream_repair_can_prefetch_donor_range(true, false, false, false, 4096));
        assert!(!srp_stream_repair_can_prefetch_donor_range(false, false, false, false, 4096));
        assert!(!srp_stream_repair_can_prefetch_donor_range(true, true, false, false, 4096));
        assert!(!srp_stream_repair_can_prefetch_donor_range(true, false, true, false, 4096));
        assert!(!srp_stream_repair_can_prefetch_donor_range(true, false, false, true, 4096));
        assert!(!srp_stream_repair_can_prefetch_donor_range(true, false, false, false, 0));
    }

    #[test]
    fn test_stream_repair_pass_and_donor_dead_helpers() {
        assert!(srp_stream_repair_should_run_cross_pack(true, false));
        assert!(!srp_stream_repair_should_run_cross_pack(false, false));
        assert!(!srp_stream_repair_should_run_cross_pack(true, true));

        assert!(srp_stream_repair_should_clear_post_working_after_live(true, false));
        assert!(!srp_stream_repair_should_clear_post_working_after_live(true, true));
        assert!(!srp_stream_repair_should_clear_post_working_after_live(false, false));

        assert!(srp_stream_repair_should_include_file_in_donor_dead_pass(10, 0, false));
        assert!(!srp_stream_repair_should_include_file_in_donor_dead_pass(10, 0, true));
        assert!(srp_stream_repair_should_include_file_in_donor_dead_pass(10, 1, true));
        assert!(!srp_stream_repair_should_include_file_in_donor_dead_pass(0, 1, false));

        assert!(srp_stream_repair_should_check_donor_dead_samples(8, 8, false));
        assert!(!srp_stream_repair_should_check_donor_dead_samples(7, 8, false));
        assert!(!srp_stream_repair_should_check_donor_dead_samples(8, 8, true));

        assert!(srp_stream_repair_is_eligible_target_for_round(true, true, 1024, 0));
        assert!(!srp_stream_repair_is_eligible_target_for_round(true, true, 1024, 1));
        assert!(srp_stream_repair_is_eligible_target_for_round(true, true, 0, 1));
        assert!(!srp_stream_repair_is_eligible_target_for_round(false, true, 1024, 0));
        assert!(!srp_stream_repair_is_eligible_target_for_round(true, false, 1024, 0));
    }

    #[test]
    fn test_stream_repair_patch_and_completion_helpers() {
        assert!(srp_stream_repair_should_stop_sibling_donor_probes(true, 3, 3));
        assert!(!srp_stream_repair_should_stop_sibling_donor_probes(true, 2, 3));
        assert!(!srp_stream_repair_should_stop_sibling_donor_probes(false, 5, 3));

        assert!(srp_stream_repair_is_patch_article_usable(true, 1000, 1000, 200, 800));
        assert!(!srp_stream_repair_is_patch_article_usable(false, 1000, 1000, 200, 800));
        assert!(!srp_stream_repair_is_patch_article_usable(true, 999, 1000, 200, 800));
        assert!(!srp_stream_repair_is_patch_article_usable(true, 1000, 1000, 0, 800));
        assert!(!srp_stream_repair_is_patch_article_usable(true, 1000, 1000, 201, 800));

        assert!(srp_stream_repair_should_retry_donor_map_with_password(false, false, true, false));
        assert!(srp_stream_repair_should_retry_donor_map_with_password(false, false, false, true));
        assert!(!srp_stream_repair_should_retry_donor_map_with_password(true, false, true, false));
        assert!(!srp_stream_repair_should_retry_donor_map_with_password(false, true, true, false));
        assert!(!srp_stream_repair_should_retry_donor_map_with_password(false, false, false, false));

        assert!(srp_stream_repair_can_verify_donor_inner_file(false, false, 5000, 5000));
        assert!(srp_stream_repair_can_verify_donor_inner_file(true, true, 5000, 5000));
        assert!(!srp_stream_repair_can_verify_donor_inner_file(true, false, 5000, 5000));
        assert!(!srp_stream_repair_can_verify_donor_inner_file(false, false, 4999, 5000));

        assert_eq!(srp_stream_repair_classify_completed_target_mark(1024, true, false), 1);
        assert_eq!(srp_stream_repair_classify_completed_target_mark(1024, false, true), 2);
        assert_eq!(srp_stream_repair_classify_completed_target_mark(0, true, false), 0);

        assert!(srp_stream_repair_has_any_recovered_stats(1, 0, 0));
        assert!(srp_stream_repair_has_any_recovered_stats(0, 512, 0));
        assert!(srp_stream_repair_has_any_recovered_stats(0, 0, 2));
        assert!(!srp_stream_repair_has_any_recovered_stats(0, 0, 0));
    }

    #[test]
    fn test_parchecker_and_repair_ctrl_helpers() {
        assert!(srp_parchecker_can_quick_scan_data_file(true, true, true));
        assert!(!srp_parchecker_can_quick_scan_data_file(false, true, true));
        assert!(!srp_parchecker_can_quick_scan_data_file(true, false, true));
        assert!(!srp_parchecker_can_quick_scan_data_file(true, true, false));

        assert_eq!(srp_parchecker_should_wait_for_queued_pars(false, false), 0);
        assert_eq!(srp_parchecker_should_wait_for_queued_pars(true, false), 1);
        assert_eq!(srp_parchecker_should_wait_for_queued_pars(false, true), 2);
        assert_eq!(srp_parchecker_should_wait_for_queued_pars(true, true), 2);

        assert!(srp_parchecker_should_add_extra_file_entry(true, true, true));
        assert!(srp_parchecker_should_add_extra_file_entry(false, false, false));
        assert!(!srp_parchecker_should_add_extra_file_entry(false, true, false));
        assert!(!srp_parchecker_should_add_extra_file_entry(false, false, true));

        assert!(srp_repair_ctrl_is_usable_par_block_candidate(true, 4));
        assert!(!srp_repair_ctrl_is_usable_par_block_candidate(true, 0));
        assert!(!srp_repair_ctrl_is_usable_par_block_candidate(false, 4));
    }
}
