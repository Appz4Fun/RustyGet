// Hot-path helpers for QueueCoordinator, DupeCoordinator, and DupeArticleFallback (batch 39).

/// Applies article completion success counters to file and NZB counters.
#[allow(clippy::too_many_arguments)]
pub fn qcf_queue_coord_apply_success_article_counters(
    article_size: i64,
    is_par_file: bool,
    file_success_size: i64,
    nzb_cur_success_size: i64,
    nzb_par_cur_success_size: i64,
    file_success_articles: i32,
    nzb_cur_success_articles: i32,
) -> (i64, i64, i64, i32, i32) {
    let par_add = if is_par_file { article_size } else { 0 };
    (
        file_success_size.wrapping_add(article_size),
        nzb_cur_success_size.wrapping_add(article_size),
        nzb_par_cur_success_size.wrapping_add(par_add),
        file_success_articles.wrapping_add(1),
        nzb_cur_success_articles.wrapping_add(1),
    )
}

/// Returns whether QueueCoordinator::Run has more jobs (`hasMoreArticles || articeDownloadsRunning`).
pub fn qcf_queue_coord_has_more_jobs(
    has_more_articles: bool,
    active_downloads_running: bool,
) -> bool {
    has_more_articles || active_downloads_running
}

/// Returns whether QueueCoordinator::Run can proceed with starting downloads.
pub fn qcf_queue_coord_can_proceed_downloads(
    is_stopped: bool,
    active_count: usize,
    effective_limit: i32,
) -> bool {
    let clamped_active = active_count.min(i32::MAX as usize) as i32;
    !is_stopped && clamped_active < effective_limit
}

/// Returns whether `AddNzbFileToQueue` should run `CheckDupeFileInfos`.
pub fn qcf_queue_coord_should_run_check_dupe_file_infos(
    delete_status: i32,
    ds_none: i32,
    dupe_check: bool,
    dupe_mode: i32,
    dm_force: i32,
) -> bool {
    delete_status == ds_none && dupe_check && dupe_mode != dm_force
}

/// Returns whether `AddNzbFileToQueue` should notify `eaNzbAdded`.
pub fn qcf_queue_coord_should_notify_nzb_added(delete_status: i32, ds_manual: i32) -> bool {
    delete_status != ds_manual
}

/// Returns whether `ArticleCompleted` should re-validate misaligned duplicate neighbors.
pub fn qcf_queue_coord_should_revalidate_dupe_neighbors(
    dupe_recovered_articles: i32,
    decoded_file_size: i64,
) -> bool {
    dupe_recovered_articles > 0 && decoded_file_size > 0
}

/// Returns whether `ArticleCompleted` should add the attempt's `ServerStatList` to file/NZB stats.
pub fn qcf_queue_coord_should_account_attempt_server_stats(dl_status: i32, ad_retry: i32) -> bool {
    dl_status != ad_retry
}

/// Selects the CRC stored on `CompletedFile` in `DeleteFileInfo`.
pub fn qcf_queue_coord_completed_file_crc(status: i32, cf_success: i32, crc: u32) -> u32 {
    if status == cf_success {
        crc
    } else {
        0
    }
}

/// Returns whether `DeleteFileInfo` should call `DiscardDirectRename`.
pub fn qcf_queue_coord_should_discard_direct_rename_on_complete(
    direct_rename_status: i32,
    ts_running: i32,
    deleting: bool,
    download_completed: bool,
) -> bool {
    direct_rename_status == ts_running && !deleting && download_completed
}

/// Selects the `DownloadQueue::EAspectAction` emitted at the end of `DeleteFileInfo`.
pub fn qcf_queue_coord_select_file_removed_event(
    completed: bool,
    file_deleted: bool,
    ea_file_completed: i32,
    ea_file_deleted: i32,
) -> i32 {
    if completed && !file_deleted {
        ea_file_completed
    } else {
        ea_file_deleted
    }
}

/// Computes `(delete_result_file, discard_cached_segment)` in `QueueCoordinator::DiscardArticleSegment`.
pub fn qcf_queue_coord_discard_segment_actions(
    fallback_round: i32,
    has_result_filename: bool,
    has_segment_content: bool,
    flush_locked: bool,
) -> (bool, bool) {
    let delete_result_file = fallback_round > 0 && has_result_filename;
    let discard_cached_segment = has_segment_content && !flush_locked;
    (delete_result_file, discard_cached_segment)
}

/// Returns whether `DiscardTempFiles` should delete the direct-write output file (and hard link).
pub fn qcf_queue_coord_should_delete_direct_write_output(
    direct_write: bool,
    output_filename_empty: bool,
    force_direct_write: bool,
) -> bool {
    direct_write && !output_filename_empty && !force_direct_write
}

/// Returns whether an NZB is exempt from being blocked in `QueueCoordinator::HasActiveDupe`.
pub fn qcf_queue_coord_has_active_dupe_self_exempt(
    has_extra_priority: bool,
    has_post_info: bool,
    post_working: bool,
    post_stage: i32,
    pt_queued: i32,
) -> bool {
    has_extra_priority || (has_post_info && (post_working || post_stage != pt_queued))
}

/// Returns whether `CheckParDamage` is allowed to fail over to the backup duplicate.
pub fn qcf_queue_coord_par_damage_failover_allowed(
    certain: bool,
    backup_alive: i32,
    failover_warranted: bool,
) -> bool {
    (certain || backup_alive >= 999) && failover_warranted
}

/// Evaluates `(mark_probed, continue_probe)` at the start of `QueueCoordinator::StartDeadPickProbe`.
#[allow(clippy::too_many_arguments)]
pub fn qcf_queue_coord_start_dead_pick_probe_eligible(
    hc_is_dupe: bool,
    dupe_check: bool,
    dead_pick_probed: bool,
    fails_over: bool,
    is_nzb_kind: bool,
    deleting: bool,
    parking: bool,
    delete_status: i32,
    ds_none: i32,
    current_success_articles: i32,
) -> (bool, bool) {
    let mark_probed = hc_is_dupe
        && dupe_check
        && !dead_pick_probed
        && fails_over
        && is_nzb_kind
        && !deleting
        && !parking
        && delete_status == ds_none;
    let continue_probe = mark_probed && current_success_articles <= 0;
    (mark_probed, continue_probe)
}

/// Computes merged `(min_time, max_time)` in `QueueCoordinator::MergeQueueEntries`.
pub fn qcf_queue_coord_merge_min_max_time(
    dest_min: i64,
    src_min: i64,
    dest_max: i64,
    src_max: i64,
) -> (i64, i64) {
    let merged_min = if src_min < dest_min { src_min } else { dest_min };
    let merged_max = if src_max > dest_max { src_max } else { dest_max };
    (merged_min, merged_max)
}

/// Returns whether `HealthCheck` is set to `hcDupe` or `hcPark` in `DupeCoordinator`.
pub fn qcf_dupecoord_is_dupe_health_mode(health_check: i32, hc_dupe: i32, hc_park: i32) -> bool {
    health_check == hc_dupe || health_check == hc_park
}

/// Evaluates `(clear_skip_and_same_content, check_return_backup)` in `DupeCoordinator::NzbFound`.
pub fn qcf_dupecoord_same_content_history_outcome(succeeded: bool, failed: bool) -> (bool, bool) {
    let clear_skip_and_same_content = !succeeded && !failed;
    let check_return_backup = failed && !succeeded;
    (clear_skip_and_same_content, check_return_backup)
}

/// Returns whether `DupeArticleFallback::GetParsedDonor` should evict the oldest cached donor.
pub fn qcf_dupe_fallback_should_evict_parsed_donor_cache(
    cache_len: usize,
    max_cached_donors: i32,
) -> bool {
    (cache_len as i64) >= (max_cached_donors as i64)
}

/// Resolves structural / filename match in `DupeArticleFallback::MatchDonorFile`.
/// Returns `-1` to abort (`nullptr`), `1` to return `filenameMatch`, `2` to return `structuralMatches[0]`,
/// or `0` to continue to step-size matching.
pub fn qcf_dupe_fallback_resolve_structural_or_filename_match(
    filename_ambiguous: bool,
    has_filename_match: bool,
    structural_matches_len: usize,
) -> i32 {
    if filename_ambiguous {
        -1
    } else if has_filename_match {
        1
    } else if structural_matches_len == 1 {
        2
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_coord_counters_and_run_gates() {
        assert_eq!(
            qcf_queue_coord_apply_success_article_counters(1000, false, 500, 2000, 300, 1, 4),
            (1500, 3000, 300, 2, 5)
        );
        assert_eq!(
            qcf_queue_coord_apply_success_article_counters(1000, true, 500, 2000, 300, 1, 4),
            (1500, 3000, 1300, 2, 5)
        );
        assert!(qcf_queue_coord_has_more_jobs(true, false));
        assert!(qcf_queue_coord_has_more_jobs(false, true));
        assert!(!qcf_queue_coord_has_more_jobs(false, false));
        assert!(qcf_queue_coord_can_proceed_downloads(false, 3, 4));
        assert!(!qcf_queue_coord_can_proceed_downloads(false, 4, 4));
        assert!(!qcf_queue_coord_can_proceed_downloads(true, 0, 4));
    }

    #[test]
    fn test_queue_coord_add_and_complete_decisions() {
        assert!(qcf_queue_coord_should_run_check_dupe_file_infos(0, 0, true, 1, 2));
        assert!(!qcf_queue_coord_should_run_check_dupe_file_infos(1, 0, true, 1, 2));
        assert!(!qcf_queue_coord_should_run_check_dupe_file_infos(0, 0, false, 1, 2));
        assert!(!qcf_queue_coord_should_run_check_dupe_file_infos(0, 0, true, 2, 2));

        assert!(qcf_queue_coord_should_notify_nzb_added(0, 2));
        assert!(!qcf_queue_coord_should_notify_nzb_added(2, 2));

        assert!(qcf_queue_coord_should_revalidate_dupe_neighbors(1, 1024));
        assert!(!qcf_queue_coord_should_revalidate_dupe_neighbors(0, 1024));
        assert!(!qcf_queue_coord_should_revalidate_dupe_neighbors(1, 0));

        assert!(qcf_queue_coord_should_account_attempt_server_stats(1, 3));
        assert!(!qcf_queue_coord_should_account_attempt_server_stats(3, 3));

        assert_eq!(qcf_queue_coord_completed_file_crc(1, 1, 0xDEAD_BEEF), 0xDEAD_BEEF);
        assert_eq!(qcf_queue_coord_completed_file_crc(2, 1, 0xDEAD_BEEF), 0);
    }

    #[test]
    fn test_queue_coord_delete_and_discard_helpers() {
        assert!(qcf_queue_coord_should_discard_direct_rename_on_complete(1, 1, false, true));
        assert!(!qcf_queue_coord_should_discard_direct_rename_on_complete(1, 1, true, true));
        assert!(!qcf_queue_coord_should_discard_direct_rename_on_complete(1, 1, false, false));

        assert_eq!(qcf_queue_coord_select_file_removed_event(true, false, 10, 20), 10);
        assert_eq!(qcf_queue_coord_select_file_removed_event(true, true, 10, 20), 20);
        assert_eq!(qcf_queue_coord_select_file_removed_event(false, false, 10, 20), 20);

        assert_eq!(
            qcf_queue_coord_discard_segment_actions(1, true, true, false),
            (true, true)
        );
        assert_eq!(
            qcf_queue_coord_discard_segment_actions(0, true, true, true),
            (false, false)
        );

        assert!(qcf_queue_coord_should_delete_direct_write_output(true, false, false));
        assert!(!qcf_queue_coord_should_delete_direct_write_output(true, true, false));
        assert!(!qcf_queue_coord_should_delete_direct_write_output(true, false, true));
    }

    #[test]
    fn test_queue_coord_dupe_probe_and_fallback_helpers() {
        assert!(qcf_queue_coord_has_active_dupe_self_exempt(true, false, false, 0, 0));
        assert!(qcf_queue_coord_has_active_dupe_self_exempt(false, true, true, 0, 0));
        assert!(qcf_queue_coord_has_active_dupe_self_exempt(false, true, false, 1, 0));
        assert!(!qcf_queue_coord_has_active_dupe_self_exempt(false, true, false, 0, 0));

        assert!(qcf_queue_coord_par_damage_failover_allowed(true, 500, true));
        assert!(qcf_queue_coord_par_damage_failover_allowed(false, 999, true));
        assert!(!qcf_queue_coord_par_damage_failover_allowed(false, 998, true));
        assert!(!qcf_queue_coord_par_damage_failover_allowed(true, 1000, false));

        assert_eq!(
            qcf_queue_coord_start_dead_pick_probe_eligible(
                true, true, false, true, true, false, false, 0, 0, 0
            ),
            (true, true)
        );
        assert_eq!(
            qcf_queue_coord_start_dead_pick_probe_eligible(
                true, true, false, true, true, false, false, 0, 0, 5
            ),
            (true, false)
        );
        assert_eq!(
            qcf_queue_coord_start_dead_pick_probe_eligible(
                true, true, true, true, true, false, false, 0, 0, 0
            ),
            (false, false)
        );

        assert_eq!(qcf_queue_coord_merge_min_max_time(100, 80, 200, 250), (80, 250));
        assert!(qcf_dupecoord_is_dupe_health_mode(2, 2, 3));
        assert!(qcf_dupecoord_is_dupe_health_mode(3, 2, 3));
        assert!(!qcf_dupecoord_is_dupe_health_mode(1, 2, 3));

        assert_eq!(qcf_dupecoord_same_content_history_outcome(false, false), (true, false));
        assert_eq!(qcf_dupecoord_same_content_history_outcome(false, true), (false, true));
        assert_eq!(qcf_dupecoord_same_content_history_outcome(true, false), (false, false));

        assert!(qcf_dupe_fallback_should_evict_parsed_donor_cache(8, 8));
        assert!(!qcf_dupe_fallback_should_evict_parsed_donor_cache(7, 8));

        assert_eq!(
            qcf_dupe_fallback_resolve_structural_or_filename_match(true, true, 1),
            -1
        );
        assert_eq!(
            qcf_dupe_fallback_resolve_structural_or_filename_match(false, true, 2),
            1
        );
        assert_eq!(
            qcf_dupe_fallback_resolve_structural_or_filename_match(false, false, 1),
            2
        );
        assert_eq!(
            qcf_dupe_fallback_resolve_structural_or_filename_match(false, false, 3),
            0
        );
    }
}
