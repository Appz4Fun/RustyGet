//! DownloadInfo, QueueCoordinator, and QueueEditor hot-path state machines
//! and counter update kernels ported from C++ to Rust.

/// Applies a `ServerStatList::EStatOperation` (`soSet`, `soAdd`, `soSubtract`) to a server's article counters.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn server_stat_apply_op(
    cur_success: i32,
    cur_failed: i32,
    delta_success: i32,
    delta_failed: i32,
    stat_op: i32,
    so_set: i32,
    so_add: i32,
    so_subtract: i32,
) -> (i32, i32) {
    if stat_op == so_set {
        (delta_success, delta_failed)
    } else if stat_op == so_add {
        (
            cur_success.wrapping_add(delta_success),
            cur_failed.wrapping_add(delta_failed),
        )
    } else if stat_op == so_subtract {
        (
            cur_success.wrapping_sub(delta_success),
            cur_failed.wrapping_sub(delta_failed),
        )
    } else {
        (cur_success, cur_failed)
    }
}

/// Updates `NzbInfo` completed file statistics in `NzbInfo::UpdateCompletedStats`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn nzb_info_update_completed_stats(
    file_success_size: i64,
    file_failed_size: i64,
    file_failed_articles: i32,
    file_success_articles: i32,
    file_extra_priority: bool,
    file_par_file: bool,
    file_paused: bool,
    inout_success_size: &mut i64,
    inout_failed_size: &mut i64,
    inout_failed_articles: &mut i32,
    inout_success_articles: &mut i32,
    inout_extra_priority: &mut i32,
    inout_par_success_size: &mut i64,
    inout_par_failed_size: &mut i64,
    inout_remaining_par_count: &mut i32,
    inout_paused_file_count: &mut i32,
) {
    *inout_success_size = inout_success_size.wrapping_add(file_success_size);
    *inout_failed_size = inout_failed_size.wrapping_add(file_failed_size);
    *inout_failed_articles = inout_failed_articles.wrapping_add(file_failed_articles);
    *inout_success_articles = inout_success_articles.wrapping_add(file_success_articles);
    if file_extra_priority {
        *inout_extra_priority = inout_extra_priority.wrapping_sub(1);
    }
    if file_par_file {
        *inout_par_success_size = inout_par_success_size.wrapping_add(file_success_size);
        *inout_par_failed_size = inout_par_failed_size.wrapping_add(file_failed_size);
        *inout_remaining_par_count = inout_remaining_par_count.wrapping_sub(1);
    }
    if file_paused {
        *inout_paused_file_count = inout_paused_file_count.wrapping_sub(1);
    }
}

/// Updates `NzbInfo` deleted file statistics in `NzbInfo::UpdateDeletedStats`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn nzb_info_update_deleted_stats(
    file_size: i64,
    file_success_size: i64,
    file_failed_size: i64,
    file_missed_size: i64,
    file_total_articles: i32,
    file_success_articles: i32,
    file_failed_articles: i32,
    file_missed_articles: i32,
    file_remaining_size: i64,
    file_extra_priority: bool,
    file_par_file: bool,
    file_paused: bool,
    inout_file_count: &mut i32,
    inout_size: &mut i64,
    inout_cur_success_size: &mut i64,
    inout_failed_size: &mut i64,
    inout_failed_articles: &mut i32,
    inout_cur_failed_size: &mut i64,
    inout_total_articles: &mut i32,
    inout_cur_success_articles: &mut i32,
    inout_cur_failed_articles: &mut i32,
    inout_remaining_size: &mut i64,
    inout_extra_priority: &mut i32,
    inout_remaining_par_count: &mut i32,
    inout_par_size: &mut i64,
    inout_par_cur_success_size: &mut i64,
    inout_par_failed_size: &mut i64,
    inout_par_cur_failed_size: &mut i64,
    inout_paused_file_count: &mut i32,
    inout_paused_size: &mut i64,
) {
    *inout_file_count = inout_file_count.wrapping_sub(1);
    *inout_size = inout_size.wrapping_sub(file_size);
    *inout_cur_success_size = inout_cur_success_size.wrapping_sub(file_success_size);
    *inout_failed_size = inout_failed_size.wrapping_sub(file_missed_size);
    *inout_failed_articles = inout_failed_articles.wrapping_sub(file_missed_articles);
    *inout_cur_failed_size =
        inout_cur_failed_size.wrapping_sub(file_failed_size.wrapping_add(file_missed_size));
    *inout_total_articles = inout_total_articles.wrapping_sub(file_total_articles);
    *inout_cur_success_articles = inout_cur_success_articles.wrapping_sub(file_success_articles);
    *inout_cur_failed_articles = inout_cur_failed_articles
        .wrapping_sub(file_failed_articles.wrapping_add(file_missed_articles));
    *inout_remaining_size = inout_remaining_size.wrapping_sub(file_remaining_size);
    if file_extra_priority {
        *inout_extra_priority = inout_extra_priority.wrapping_sub(1);
    }
    if file_par_file {
        *inout_remaining_par_count = inout_remaining_par_count.wrapping_sub(1);
        *inout_par_size = inout_par_size.wrapping_sub(file_size);
        *inout_par_cur_success_size = inout_par_cur_success_size.wrapping_sub(file_success_size);
        *inout_par_failed_size = inout_par_failed_size.wrapping_sub(file_missed_size);
        *inout_par_cur_failed_size =
            inout_par_cur_failed_size.wrapping_sub(file_failed_size.wrapping_add(file_missed_size));
    }
    if file_paused {
        *inout_paused_file_count = inout_paused_file_count.wrapping_sub(1);
        *inout_paused_size = inout_paused_size.wrapping_sub(file_remaining_size);
    }
}

/// Computes updated `(changed, new_paused_count, new_paused_size)` for `FileInfo::SetPaused`.
#[inline]
pub fn file_info_calc_paused_delta(
    cur_paused: bool,
    new_paused: bool,
    has_nzb: bool,
    cur_paused_count: i32,
    cur_paused_size: i64,
    remaining_size: i64,
) -> (bool, i32, i64) {
    if cur_paused != new_paused && has_nzb {
        let count_delta = if new_paused { 1 } else { -1 };
        let size_delta = if new_paused {
            remaining_size
        } else {
            remaining_size.wrapping_neg()
        };
        (
            true,
            cur_paused_count.wrapping_add(count_delta),
            cur_paused_size.wrapping_add(size_delta),
        )
    } else {
        (false, cur_paused_count, cur_paused_size)
    }
}

/// Computes updated `(changed, new_extra_priority)` for `FileInfo::SetExtraPriority`.
#[inline]
pub fn file_info_calc_extra_priority_delta(
    cur_extra: bool,
    new_extra: bool,
    has_nzb: bool,
    cur_nzb_extra: i32,
) -> (bool, i32) {
    if cur_extra != new_extra && has_nzb {
        let delta = if new_extra { 1 } else { -1 };
        (true, cur_nzb_extra.wrapping_add(delta))
    } else {
        (false, cur_nzb_extra)
    }
}

/// Determines whether `QueueCoordinator::Load` should reload and resave a completed file's state when server IDs changed.
#[inline]
pub fn queue_coord_should_resave_completed_file_state(
    status: i32,
    cf_partial: i32,
    cf_failure: i32,
    file_id: i32,
) -> bool {
    (status == cf_partial || status == cf_failure) && file_id > 0
}

/// Resolves `(use_specific_server, connection_level)` when requesting a connection in `QueueCoordinator::Run`.
#[inline]
pub fn queue_coord_resolve_desired_server_level(
    has_desired_server: bool,
    has_server_obj: bool,
    is_active: bool,
    norm_level: i32,
) -> (bool, i32) {
    if has_desired_server && has_server_obj && is_active && norm_level >= 0 {
        (true, norm_level)
    } else {
        (false, 0)
    }
}

/// Computes `(posted_min_ago, start_in_min)` for the `PropagationDelay` log message in `QueueCoordinator::GetNextArticle`.
#[inline]
pub fn queue_coord_calc_propagation_wait_log_mins(
    file_time: i64,
    propagation_delay: i32,
    cur_date: i64,
) -> (i32, i32) {
    let wait_sec = (file_time as i32)
        .wrapping_add(propagation_delay)
        .wrapping_sub(cur_date as i32);
    let posted_min = (cur_date.wrapping_sub(file_time) as i32) / 60;
    let start_min = (wait_sec / 60).wrapping_add(1);
    (posted_min, start_min)
}

/// Determines whether `QueueCoordinator::GetNextArticle` should prioritize fetching the first article of an unrenamed file.
#[inline]
pub fn queue_coord_should_fetch_first_article_for_rename(
    direct_rename: bool,
    direct_rename_status: i32,
    ts_running: i32,
    all_first: bool,
) -> bool {
    direct_rename && direct_rename_status <= ts_running && !all_first
}

/// Checks whether a finished donor-substituted article is misaligned in `QueueCoordinator::ArticleCompleted`.
#[inline]
pub fn queue_coord_is_misplaced_donor_article(
    status: i32,
    ad_finished: i32,
    dupe_fallback_round: i32,
    decoded_file_size: i64,
    segment_aligned: bool,
) -> bool {
    status == ad_finished && dupe_fallback_round > 0 && decoded_file_size > 0 && !segment_aligned
}

/// Checks whether a finished article arrived from its own primary posting (ending the run of consecutive failures).
#[inline]
pub fn queue_coord_should_reset_dupe_failed_run(
    dupe_fallback_round: i32,
    msg_id: &[u8],
    orig_msg_id: &[u8],
) -> bool {
    dupe_fallback_round == 0 || orig_msg_id.is_empty() || msg_id == orig_msg_id
}

/// Evaluates whether a finished article counts as a proven duplicate recovery and whether `DupeCutover` should be triggered.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn queue_coord_eval_dupe_recovery_and_cutover(
    dupe_fallback_round: i32,
    dupe_proactive: bool,
    msg_id: &[u8],
    orig_msg_id: &[u8],
    cur_file_recovered: i32,
    cur_nzb_recovered: i32,
    dupe_cutover: bool,
    cutover_threshold: i32,
) -> (bool, i32, i32, bool) {
    let recovered = dupe_fallback_round > 0
        && !dupe_proactive
        && !orig_msg_id.is_empty()
        && msg_id != orig_msg_id;
    if !recovered {
        return (false, cur_file_recovered, cur_nzb_recovered, false);
    }
    let next_file_rec = cur_file_recovered.wrapping_add(1);
    let next_nzb_rec = cur_nzb_recovered.wrapping_add(1);
    let trigger_cutover = !dupe_cutover && next_file_rec >= cutover_threshold;
    (true, next_file_rec, next_nzb_rec, trigger_cutover)
}

/// Advances remaining sizes and completed article count when an article finishes without retry in `QueueCoordinator::ArticleCompleted`.
#[inline]
pub fn queue_coord_advance_article_completion_counters(
    rem_file_size: i64,
    rem_nzb_size: i64,
    paused_size: i64,
    article_size: i32,
    is_paused: bool,
    completed_articles: i32,
    total_articles: i32,
) -> (i64, i64, i64, i32, bool) {
    let sz = i64::from(article_size);
    let new_rem_file = rem_file_size.wrapping_sub(sz);
    let new_rem_nzb = rem_nzb_size.wrapping_sub(sz);
    let new_paused = if is_paused {
        paused_size.wrapping_sub(sz)
    } else {
        paused_size
    };
    let new_completed = completed_articles.wrapping_add(1);
    let file_completed = total_articles == new_completed;
    (
        new_rem_file,
        new_rem_nzb,
        new_paused,
        new_completed,
        file_completed,
    )
}

/// Evaluates `(file_completed, complete_file_parts)` after health and dead-download checks in `QueueCoordinator::ArticleCompleted`.
#[inline]
pub fn queue_coord_eval_file_completion_and_assembly(
    file_completed: bool,
    parking: bool,
    active_downloads: i32,
    dupe_deleted: bool,
    file_deleted: bool,
    finished_before_cancel: bool,
) -> (bool, bool) {
    let fc = if parking && active_downloads == 1 && !dupe_deleted {
        true
    } else {
        file_completed
    };
    let complete_parts = fc && (!file_deleted || parking || finished_before_cancel);
    (fc, complete_parts)
}

/// Applies counter adjustments when `CompleteFileParts()` fails in `QueueCoordinator::ArticleCompleted`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn queue_coord_apply_assembly_failure_counters(
    success_articles: i32,
    success_size: i64,
    is_par_file: bool,
    inout_file_failed_articles: &mut i32,
    inout_file_failed_size: &mut i64,
    inout_nzb_cur_failed_articles: &mut i32,
    inout_nzb_cur_success_articles: &mut i32,
    inout_nzb_cur_failed_size: &mut i64,
    inout_nzb_cur_success_size: &mut i64,
    inout_nzb_par_cur_failed_size: &mut i64,
    inout_nzb_par_cur_success_size: &mut i64,
) {
    *inout_file_failed_articles = inout_file_failed_articles.wrapping_add(success_articles);
    *inout_file_failed_size = inout_file_failed_size.wrapping_add(success_size);
    *inout_nzb_cur_failed_articles = inout_nzb_cur_failed_articles.wrapping_add(success_articles);
    *inout_nzb_cur_success_articles = inout_nzb_cur_success_articles
        .wrapping_sub(success_articles)
        .max(0);
    *inout_nzb_cur_failed_size = inout_nzb_cur_failed_size.wrapping_add(success_size);
    *inout_nzb_cur_success_size = inout_nzb_cur_success_size.wrapping_sub(success_size).max(0);
    if is_par_file {
        *inout_nzb_par_cur_failed_size = inout_nzb_par_cur_failed_size.wrapping_add(success_size);
        *inout_nzb_par_cur_success_size = inout_nzb_par_cur_success_size
            .wrapping_sub(success_size)
            .max(0);
    }
}

/// Demotes a finished article's counters back to failed in `QueueCoordinator::CountAsFailed`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn queue_coord_demote_article_counters(
    article_size: i32,
    is_par_file: bool,
    dupe_proactive: bool,
    msg_id: &[u8],
    orig_msg_id: &[u8],
    inout_file_success_size: &mut i64,
    inout_file_failed_size: &mut i64,
    inout_file_success_articles: &mut i32,
    inout_file_failed_articles: &mut i32,
    inout_nzb_cur_success_size: &mut i64,
    inout_nzb_cur_failed_size: &mut i64,
    inout_nzb_par_cur_success_size: &mut i64,
    inout_nzb_par_cur_failed_size: &mut i64,
    inout_nzb_cur_success_articles: &mut i32,
    inout_nzb_cur_failed_articles: &mut i32,
    inout_file_dupe_recovered: &mut i32,
    inout_nzb_dupe_recovered: &mut i32,
) {
    let sz = i64::from(article_size);
    *inout_file_success_size = inout_file_success_size.wrapping_sub(sz);
    *inout_file_failed_size = inout_file_failed_size.wrapping_add(sz);
    *inout_file_success_articles = inout_file_success_articles.wrapping_sub(1);
    *inout_file_failed_articles = inout_file_failed_articles.wrapping_add(1);
    *inout_nzb_cur_success_size = inout_nzb_cur_success_size.wrapping_sub(sz);
    *inout_nzb_cur_failed_size = inout_nzb_cur_failed_size.wrapping_add(sz);
    if is_par_file {
        *inout_nzb_par_cur_success_size = inout_nzb_par_cur_success_size.wrapping_sub(sz);
        *inout_nzb_par_cur_failed_size = inout_nzb_par_cur_failed_size.wrapping_add(sz);
    }
    *inout_nzb_cur_success_articles = inout_nzb_cur_success_articles.wrapping_sub(1);
    *inout_nzb_cur_failed_articles = inout_nzb_cur_failed_articles.wrapping_add(1);

    if !dupe_proactive && !orig_msg_id.is_empty() && msg_id != orig_msg_id {
        *inout_file_dupe_recovered = inout_file_dupe_recovered.wrapping_sub(1);
        *inout_nzb_dupe_recovered = inout_nzb_dupe_recovered.wrapping_sub(1);
    }
}

/// Determines whether `QueueCoordinator::DiscardTempFiles` should delete an article's temporary result file.
#[inline]
pub fn queue_coord_should_delete_temp_article_file(
    has_result_filename: bool,
    direct_write: bool,
    force_direct_write: bool,
    dupe_fallback_round: i32,
) -> bool {
    has_result_filename && ((!direct_write && !force_direct_write) || dupe_fallback_round > 0)
}

/// Determines whether `QueueCoordinator::CheckDupeFailover` should defer failover and whether `DupeFailoverChecked` should be updated.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn queue_coord_should_defer_dupe_failover(
    fallback_enabled: bool,
    attempted: i32,
    recovered: i32,
    failover_sample: i32,
    defer_state: i32,
    dp_deferred: i32,
    par_cannot_cover: bool,
    failed_articles: i32,
    failover_checked: i32,
) -> (bool, bool) {
    if fallback_enabled
        && attempted > 0
        && (attempted < failover_sample || recovered.saturating_mul(2) >= attempted)
    {
        return (true, false);
    }
    if fallback_enabled && attempted == 0 && defer_state == dp_deferred && !par_cannot_cover {
        return (true, false);
    }
    if failover_checked >= 0 && failed_articles.wrapping_sub(failover_checked) < failover_sample {
        return (true, false);
    }
    (false, true)
}

/// Determines whether `QueueCoordinator::CheckDeadDownload` should park an unhealthy download that has no backup in history.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn queue_coord_should_park_dead_without_backup(
    fallback_enabled: bool,
    defer_state: i32,
    dp_deferred: i32,
    attempted: i32,
    recovered: i32,
    tried: i32,
    projected_sample: i32,
    files_tried: i32,
    far_below: bool,
    below: bool,
) -> bool {
    let borrowing = fallback_enabled
        && (defer_state == dp_deferred
            || (attempted > 0 && recovered.saturating_mul(2) >= attempted));
    tried >= projected_sample && files_tried >= 3 && (far_below || below) && !borrowing
}

/// Determines whether a partly alive download in `QueueCoordinator::CheckDeadDownload` should fail over to a backup.
#[inline]
pub fn queue_coord_is_partly_alive_failover_ready(
    backup_alive: i32,
    projected: i32,
    backup_lead: i32,
    critical: i32,
    failover_warranted: bool,
) -> (bool, bool) {
    let ready = backup_alive >= projected.wrapping_add(backup_lead)
        && backup_alive >= critical
        && failover_warranted;
    (ready, projected < critical)
}

/// Determines whether `QueueCoordinator::CheckParDamage` should wait for duplicate article borrowing before failing over.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn queue_coord_should_wait_for_borrowing_on_par_damage(
    fallback_enabled: bool,
    raw_article: bool,
    dupe_mode: i32,
    dm_force: i32,
    has_par2: bool,
    defer_state: i32,
    dp_deferred: i32,
    attempted: i32,
    failover_sample: i32,
    recovered: i32,
) -> bool {
    if fallback_enabled && !raw_article && dupe_mode != dm_force && has_par2 {
        defer_state == dp_deferred
            || attempted < failover_sample
            || recovered.saturating_mul(2) >= attempted
    } else {
        false
    }
}

/// Evaluates `(discard_par_probe_article, discard_article_infos)` in `QueueCoordinator::DiscardDirectRename`.
#[inline]
pub fn queue_coord_eval_discard_direct_rename_file(
    is_par_file: bool,
    completed_articles: i32,
    active_downloads: i32,
    server_mode: bool,
    has_articles: bool,
    continue_partial: bool,
    cached_articles: i32,
) -> (bool, bool) {
    let discard_par = is_par_file && completed_articles == 1 && active_downloads == 0;
    let discard_infos = server_mode
        && has_articles
        && continue_partial
        && active_downloads == 0
        && cached_articles == 0;
    (discard_par, discard_infos)
}

/// Validates a file entry in `QueueCoordinator::SplitQueueEntries`.
/// Returns `0` if valid, `1` if already (partially) downloaded, `2` if in post-process stage, `3` if from a different collection.
#[inline]
pub fn queue_coord_validate_split_file_state(
    active_downloads: i32,
    completed_articles: i32,
    has_post_info: bool,
    same_nzb: bool,
) -> i32 {
    if active_downloads > 0 || completed_articles > 0 {
        1
    } else if has_post_info {
        2
    } else if !same_nzb {
        3
    } else {
        0
    }
}

/// Compares two queue groups in `GroupSorter::operator()`.
#[allow(clippy::too_many_arguments)]
#[inline]
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

/// Counter bundle for `QueueCoordinator::MergeQueueEntries` and `QueueCoordinator::SplitQueueEntries`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NzbgetRsNzbMergeSplitCounters {
    pub file_count: i32,
    pub active_downloads: i32,
    pub full_content_hash: u32,
    pub filtered_content_hash: u32,
    pub size: i64,
    pub remaining_size: i64,
    pub paused_file_count: i32,
    pub paused_size: i64,
    pub success_size: i64,
    pub current_success_size: i64,
    pub failed_size: i64,
    pub current_failed_size: i64,
    pub par_size: i64,
    pub par_success_size: i64,
    pub par_current_success_size: i64,
    pub par_failed_size: i64,
    pub par_current_failed_size: i64,
    pub remaining_par_count: i32,
    pub total_articles: i32,
    pub success_articles: i32,
    pub failed_articles: i32,
    pub current_success_articles: i32,
    pub current_failed_articles: i32,
    pub min_time: i64,
    pub max_time: i64,
    pub downloaded_size: i64,
    pub download_sec: i32,
    pub download_start_time: i64,
}

/// File counter snapshot for `QueueCoordinator::SplitQueueEntries`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NzbgetRsSplitFileCounters {
    pub size: i64,
    pub remaining_size: i64,
    pub success_size: i64,
    pub failed_size: i64,
    pub missed_size: i64,
    pub total_articles: i32,
    pub missed_articles: i32,
    pub success_articles: i32,
    pub failed_articles: i32,
    pub par_file: i32,
    pub paused: i32,
}

/// Accumulates `src` counters into `dest` for `QueueCoordinator::MergeQueueEntries`.
pub fn queue_coord_merge_counters(
    dest: &mut NzbgetRsNzbMergeSplitCounters,
    src: &NzbgetRsNzbMergeSplitCounters,
) {
    dest.file_count = dest.file_count.wrapping_add(src.file_count);
    dest.active_downloads = dest.active_downloads.wrapping_add(src.active_downloads);
    dest.full_content_hash = 0;
    dest.filtered_content_hash = 0;

    dest.size = dest.size.wrapping_add(src.size);
    dest.remaining_size = dest.remaining_size.wrapping_add(src.remaining_size);
    dest.paused_file_count = dest.paused_file_count.wrapping_add(src.paused_file_count);
    dest.paused_size = dest.paused_size.wrapping_add(src.paused_size);

    dest.success_size = dest.success_size.wrapping_add(src.success_size);
    dest.current_success_size = dest.current_success_size.wrapping_add(src.current_success_size);
    dest.failed_size = dest.failed_size.wrapping_add(src.failed_size);
    dest.current_failed_size = dest.current_failed_size.wrapping_add(src.current_failed_size);

    dest.par_size = dest.par_size.wrapping_add(src.par_size);
    dest.par_success_size = dest.par_success_size.wrapping_add(src.par_success_size);
    dest.par_current_success_size = dest
        .par_current_success_size
        .wrapping_add(src.par_current_success_size);
    dest.par_failed_size = dest.par_failed_size.wrapping_add(src.par_failed_size);
    dest.par_current_failed_size = dest
        .par_current_failed_size
        .wrapping_add(src.par_current_failed_size);
    dest.remaining_par_count = dest.remaining_par_count.wrapping_add(src.remaining_par_count);

    dest.total_articles = dest.total_articles.wrapping_add(src.total_articles);
    dest.success_articles = dest.success_articles.wrapping_add(src.success_articles);
    dest.failed_articles = dest.failed_articles.wrapping_add(src.failed_articles);
    dest.current_success_articles = dest
        .current_success_articles
        .wrapping_add(src.current_success_articles);
    dest.current_failed_articles = dest
        .current_failed_articles
        .wrapping_add(src.current_failed_articles);

    let (min_t, max_t) = crate::queuecoordfallbackhist::qcf_queue_coord_merge_min_max_time(
        dest.min_time,
        src.min_time,
        dest.max_time,
        src.max_time,
    );
    dest.min_time = min_t;
    dest.max_time = max_t;

    dest.downloaded_size = dest.downloaded_size.wrapping_add(src.downloaded_size);
    dest.download_sec = dest.download_sec.wrapping_add(src.download_sec);
    dest.download_start_time =
        crate::coorddlwriterpool::queue_coord_select_merged_download_start_time(
            dest.download_start_time,
            src.download_start_time,
        );
}

/// Transfers one file's counters from `src` to `dst` in `QueueCoordinator::SplitQueueEntries`.
pub fn queue_coord_split_transfer_file(
    src: &mut NzbgetRsNzbMergeSplitCounters,
    dst: &mut NzbgetRsNzbMergeSplitCounters,
    f: &NzbgetRsSplitFileCounters,
) {
    let failed_and_missed_size = f.failed_size.wrapping_add(f.missed_size);
    let failed_and_missed_articles = f.failed_articles.wrapping_add(f.missed_articles);

    src.file_count = src.file_count.wrapping_sub(1);
    src.size = src.size.wrapping_sub(f.size);
    src.remaining_size = src.remaining_size.wrapping_sub(f.remaining_size);
    src.current_success_size = src.current_success_size.wrapping_sub(f.success_size);
    src.current_failed_size = src.current_failed_size.wrapping_sub(failed_and_missed_size);
    src.total_articles = src.total_articles.wrapping_sub(f.total_articles);
    src.failed_articles = src.failed_articles.wrapping_sub(f.missed_articles);
    src.current_success_articles = src
        .current_success_articles
        .wrapping_sub(f.success_articles);
    src.current_failed_articles = src
        .current_failed_articles
        .wrapping_sub(failed_and_missed_articles);

    dst.file_count = dst.file_count.wrapping_add(1);
    dst.size = dst.size.wrapping_add(f.size);
    dst.remaining_size = dst.remaining_size.wrapping_add(f.remaining_size);
    dst.current_success_size = dst.current_success_size.wrapping_add(f.success_size);
    dst.current_failed_size = dst.current_failed_size.wrapping_add(failed_and_missed_size);
    dst.total_articles = dst.total_articles.wrapping_add(f.total_articles);
    dst.failed_articles = dst.failed_articles.wrapping_add(f.missed_articles);
    dst.current_success_articles = dst
        .current_success_articles
        .wrapping_add(f.success_articles);
    dst.current_failed_articles = dst
        .current_failed_articles
        .wrapping_add(failed_and_missed_articles);

    if f.par_file != 0 {
        src.par_size = src.par_size.wrapping_sub(f.size);
        src.par_current_success_size = src.par_current_success_size.wrapping_sub(f.success_size);
        src.par_current_failed_size = src
            .par_current_failed_size
            .wrapping_sub(failed_and_missed_size);
        src.remaining_par_count = src.remaining_par_count.wrapping_sub(1);

        dst.par_size = dst.par_size.wrapping_add(f.size);
        dst.par_current_success_size = dst.par_current_success_size.wrapping_add(f.success_size);
        dst.par_current_failed_size = dst
            .par_current_failed_size
            .wrapping_add(failed_and_missed_size);
        dst.remaining_par_count = dst.remaining_par_count.wrapping_add(1);
    }

    if f.paused != 0 {
        src.paused_file_count = src.paused_file_count.wrapping_sub(1);
        src.paused_size = src.paused_size.wrapping_sub(f.remaining_size);

        dst.paused_file_count = dst.paused_file_count.wrapping_add(1);
        dst.paused_size = dst.paused_size.wrapping_add(f.remaining_size);
    }
}

/// Computes the post-process script parameter diffs (`<script>:\tyes|no\n`) when changing categories
/// in `QueueEditor::SetNzbCategory`.
pub fn queue_editor_diff_category_extensions(old_ext: &[u8], new_ext: &[u8]) -> Vec<u8> {
    if old_ext.eq_ignore_ascii_case(new_ext) {
        return Vec::new();
    }
    let mut out = Vec::new();
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
            out.extend_from_slice(&key);
            out.extend_from_slice(b"\tyes\n");
        }
    }
    for old_script in crate::util::tokens(old_ext, b",;") {
        let found = crate::util::tokens(new_ext, b",;")
            .any(|new_script| new_script.eq_ignore_ascii_case(old_script));
        if !found {
            let mut key = Vec::with_capacity(old_script.len() + 1);
            key.extend_from_slice(old_script);
            key.push(b':');
            if key.len() > 1023 {
                key.truncate(1023);
            }
            out.extend_from_slice(&key);
            out.extend_from_slice(b"\tno\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_info_stats_and_deltas() {
        assert_eq!(server_stat_apply_op(10, 2, 5, 1, 0, 0, 1, 2), (5, 1));
        assert_eq!(server_stat_apply_op(10, 2, 5, 1, 1, 0, 1, 2), (15, 3));
        assert_eq!(server_stat_apply_op(10, 2, 5, 1, 2, 0, 1, 2), (5, 1));

        let (mut ss, mut fs, mut fa, mut sa, mut ep, mut pss, mut pfs, mut rpc, mut pfc) =
            (100, 10, 1, 10, 2, 50, 5, 3, 4);
        nzb_info_update_completed_stats(
            20, 5, 1, 2, true, true, true, &mut ss, &mut fs, &mut fa, &mut sa, &mut ep, &mut pss,
            &mut pfs, &mut rpc, &mut pfc,
        );
        assert_eq!(
            (ss, fs, fa, sa, ep, pss, pfs, rpc, pfc),
            (120, 15, 2, 12, 1, 70, 10, 2, 3)
        );

        assert_eq!(
            file_info_calc_paused_delta(false, true, true, 2, 1000, 500),
            (true, 3, 1500)
        );
        assert_eq!(
            file_info_calc_paused_delta(true, false, true, 3, 1500, 500),
            (true, 2, 1000)
        );
        assert_eq!(
            file_info_calc_extra_priority_delta(false, true, true, 1),
            (true, 2)
        );
    }

    #[test]
    fn test_queue_coord_article_and_failover() {
        assert!(queue_coord_should_resave_completed_file_state(2, 2, 3, 10));
        assert!(!queue_coord_should_resave_completed_file_state(1, 2, 3, 10));

        assert_eq!(
            queue_coord_resolve_desired_server_level(true, true, true, 2),
            (true, 2)
        );
        assert_eq!(
            queue_coord_resolve_desired_server_level(true, true, false, 2),
            (false, 0)
        );

        assert_eq!(
            queue_coord_calc_propagation_wait_log_mins(1000, 300, 1120),
            (2, 4)
        );
        assert!(queue_coord_should_fetch_first_article_for_rename(
            true, 1, 1, false
        ));
        assert!(queue_coord_is_misplaced_donor_article(
            3, 3, 1, 1024, false
        ));
        assert!(!queue_coord_should_reset_dupe_failed_run(
            1,
            b"donor@msg",
            b"orig@msg"
        ));
        assert!(queue_coord_should_reset_dupe_failed_run(
            1, b"orig@msg", b"orig@msg"
        ));

        assert_eq!(
            queue_coord_eval_dupe_recovery_and_cutover(
                1,
                false,
                b"donor@msg",
                b"orig@msg",
                3,
                10,
                false,
                4
            ),
            (true, 4, 11, true)
        );

        assert_eq!(
            queue_coord_advance_article_completion_counters(1000, 5000, 1000, 200, true, 4, 5),
            (800, 4800, 800, 5, true)
        );

        assert_eq!(
            queue_coord_eval_file_completion_and_assembly(false, true, 1, false, true, false),
            (true, true)
        );

        assert_eq!(
            queue_coord_should_defer_dupe_failover(true, 2, 0, 5, 0, 1, false, 10, -1),
            (true, false)
        );
        assert_eq!(
            queue_coord_should_defer_dupe_failover(true, 6, 1, 5, 0, 1, false, 10, -1),
            (false, true)
        );

        assert!(queue_coord_should_park_dead_without_backup(
            true, 0, 1, 6, 1, 50, 20, 3, true, false
        ));
        assert_eq!(
            queue_coord_is_partly_alive_failover_ready(950, 800, 100, 850, true),
            (true, true)
        );
        assert!(queue_coord_should_wait_for_borrowing_on_par_damage(
            true, false, 0, 2, true, 1, 1, 0, 5, 0
        ));
        assert_eq!(
            queue_coord_eval_discard_direct_rename_file(true, 1, 0, true, true, true, 0),
            (true, true)
        );
        assert_eq!(queue_coord_validate_split_file_state(0, 0, false, true), 0);
        assert_eq!(queue_coord_validate_split_file_state(1, 0, false, true), 1);

        assert!(group_sorter_compare_values(
            1, 0, 1, 0, 100, 200, 0, 0, 0, 0, 0, 0, 0
        ));
        assert!(!group_sorter_compare_values(
            1, 1, 1, 0, 100, 200, 0, 0, 0, 0, 0, 0, 0
        ));

        let mut d = NzbgetRsNzbMergeSplitCounters {
            file_count: 2,
            size: 100,
            min_time: 50,
            max_time: 60,
            ..Default::default()
        };
        let s = NzbgetRsNzbMergeSplitCounters {
            file_count: 3,
            size: 200,
            min_time: 40,
            max_time: 80,
            ..Default::default()
        };
        queue_coord_merge_counters(&mut d, &s);
        assert_eq!((d.file_count, d.size, d.min_time, d.max_time), (5, 300, 40, 80));

        let diff = queue_editor_diff_category_extensions(b"a.py, b.py", b"B.PY, c.py");
        assert_eq!(diff, b"c.py:\tyes\na.py:\tno\n");
    }
}
