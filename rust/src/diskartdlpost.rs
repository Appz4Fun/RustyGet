//! DiskState, ArticleWriter, ArticleDownloader, DirectUnpack, and PrePostProcessor
//! hot-path decision and validation kernels ported from C++ to Rust.

/// Validates the 4-field header of `DiskState::LoadDupeExtras`.
#[inline]
pub fn diskstate_validate_dupe_extras_header(
    scan_cnt: i32,
    recovered_articles: i32,
    recovered_holes: i32,
) -> bool {
    scan_cnt == 4 && recovered_articles >= 0 && recovered_holes >= 0
}

/// Validates a per-repair-job header line in `DiskState::LoadDupeExtras`.
#[inline]
pub fn diskstate_validate_repair_job_header(
    scan_cnt: i32,
    expected_cnt: i32,
    file_id: i32,
    par_file: i32,
    failed_articles: i32,
) -> bool {
    scan_cnt == expected_cnt && failed_articles >= 0 && file_id > 0 && (0..=1).contains(&par_file)
}

/// Determines whether `DiskState::LoadNzbInfo` should apply the loaded `urlStatus`.
#[inline]
pub fn diskstate_should_apply_url_status(
    kind: i32,
    nk_nzb: i32,
    url_status: i32,
    ls_failed: i32,
) -> bool {
    kind == nk_nzb || url_status >= ls_failed
}

/// Normalizes a legacy script status code in `DiskState::LoadNzbInfo`.
#[inline]
pub fn diskstate_normalize_script_status(status: i32, format_version: i32) -> i32 {
    if status > 1 && format_version < 25 {
        status - 1
    } else {
        status
    }
}

/// Evaluates whether a restored `OutputInitialized` flag remains valid in `DiskState::LoadFileInfo`.
#[inline]
pub fn diskstate_eval_restored_output_init(
    output_initialized: i32,
    direct_write: bool,
    force_direct_write: bool,
    has_own_out_file: bool,
    has_out_tmp: bool,
    has_force_dest: bool,
) -> bool {
    let restored = output_initialized != 0;
    if restored && (direct_write || force_direct_write) {
        has_own_out_file || has_out_tmp || has_force_dest
    } else {
        restored
    }
}

/// Computes `(staged_fallback, saved_status)` when serializing an `ArticleInfo` in `DiskState::SaveFileState`.
#[inline]
pub fn diskstate_calc_staged_fallback(
    completed: bool,
    article_status: i32,
    ai_finished: i32,
    dupe_fallback_round: i32,
    has_result_filename: bool,
    ai_undefined: i32,
) -> (i32, i32) {
    let staged = if !completed
        && article_status == ai_finished
        && dupe_fallback_round > 0
        && has_result_filename
    {
        1
    } else {
        0
    };
    let saved_status = if staged != 0 {
        ai_undefined
    } else {
        article_status
    };
    (staged, saved_status)
}

/// Validates the article count header in `DiskState::LoadFileState`.
#[inline]
pub fn diskstate_validate_article_count(
    size: i32,
    has_articles: bool,
    articles_len: i32,
) -> bool {
    !(size < 0 || (has_articles && size > articles_len) || size > 10_000_000)
}

/// Determines whether `DiskState::LoadFileState` needs to probe the temp directory for a staged part file.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn diskstate_need_staged_file_check(
    completed: bool,
    format_version: i32,
    status_int: i32,
    ai_finished: i32,
    segment_offset: i64,
    segment_size: i32,
    staged_fallback: i32,
    part_number: i32,
    direct_write: bool,
    force_direct_write: bool,
) -> bool {
    let range_valid =
        completed || (segment_offset >= 0 && (0..=1024 * 1024 * 1024).contains(&segment_size));
    let effective_finished = status_int == ai_finished && (format_version < 2 || range_valid);
    part_number > 0
        && ((format_version >= 2 && staged_fallback == 1 && range_valid)
            || (!completed
                && format_version < 9
                && effective_finished
                && (direct_write || force_direct_write)))
}

/// Checks whether a queue filename consists solely of ASCII digits `'0'..='9'`.
/// Matches the C++ loop where an empty string also evaluates to `true`.
#[inline]
pub fn diskstate_is_digits_only_filename(filename: &[u8]) -> bool {
    filename.iter().all(|&b| b.is_ascii_digit())
}

/// Determines whether a file state with the given suffix (`'c'` or `'s'`) should be loaded in `DiskState::LoadAllFileStates`.
#[inline]
pub fn diskstate_should_load_file_state(
    suffix: u8,
    continue_partial: bool,
    cache_was_active: bool,
) -> bool {
    suffix == b'c' || (suffix == b's' && continue_partial && !cache_was_active)
}

/// Validates and computes the target array length for a server volume slot in `DiskState::LoadVolumeStat`.
#[inline]
pub fn diskstate_calc_volume_slot_size(slot_index: i32, arr_size: i32) -> (bool, i32) {
    if !(0..=100_000).contains(&arr_size) {
        return (false, 0);
    }
    let fixed = match slot_index {
        0 | 1 => 60,
        2 => 24,
        _ => 0,
    };
    (true, if fixed > 0 { fixed } else { arr_size })
}

/// Checks whether `ArticleWriter::Start` should inspect destination files for duplicates.
#[inline]
pub fn article_writer_should_check_dupe_file(
    format: i32,
    ef_yenc: i32,
    dupe_check: bool,
    dupe_mode: i32,
    dm_force: i32,
    many_dupe_files: bool,
) -> bool {
    format == ef_yenc && dupe_check && dupe_mode != dm_force && !many_dupe_files
}

/// Determines whether the destination file collides with an existing non-owned file in `ArticleWriter::Start`.
#[inline]
pub fn article_writer_is_duplicate_dest(
    output_initialized: bool,
    is_own_output_file: bool,
    has_filename: bool,
    dest_exists: bool,
) -> bool {
    !output_initialized && !is_own_output_file && has_filename && dest_exists
}

/// Determines whether `ArticleWriter::Start` should attempt to allocate from the article cache.
#[inline]
pub fn article_writer_can_use_article_cache(
    article_cache_mb: i32,
    raw_article: bool,
    format: i32,
    ef_yenc: i32,
    is_dupe_fallback: bool,
    is_critical: bool,
) -> bool {
    article_cache_mb > 0
        && !raw_article
        && format == ef_yenc
        && !is_dupe_fallback
        && !is_critical
}

/// Advances the article write pointer (when not in raw mode) and checks whether the article boundary was exceeded.
#[inline]
pub fn article_writer_advance_and_check_range(
    raw_article: bool,
    cur_ptr: i32,
    len: i32,
    article_size: i32,
) -> (i32, bool) {
    let new_ptr = if !raw_article {
        cur_ptr.wrapping_add(len)
    } else {
        cur_ptr
    };
    (new_ptr, new_ptr > article_size)
}

/// Resolves `(new_output_initialized, direct_write)` in `ArticleWriter::CompleteFileParts`.
#[inline]
pub fn article_writer_resolve_direct_write_ready(
    direct_write_opt: bool,
    force_direct_write: bool,
    output_initialized: bool,
    has_own_out: bool,
    has_force_dest: bool,
) -> (bool, bool) {
    let want_direct = direct_write_opt || force_direct_write;
    let new_init = if want_direct
        && !output_initialized
        && (has_own_out || (force_direct_write && has_force_dest))
    {
        true
    } else {
        output_initialized
    };
    (new_init, want_direct && new_init)
}

/// Checks whether `ArticleWriter::CompleteFileParts` needs to zero-fill a gap before writing an article.
#[inline]
pub fn article_writer_need_gap_fill(
    raw_article: bool,
    direct_write: bool,
    segment_offset: i64,
    out_pos: i64,
) -> bool {
    !raw_article && !direct_write && segment_offset > -1 && segment_offset > out_pos && out_pos > -1
}

/// Evaluates whether `ArticleCache::Alloc` can allocate `size` bytes within `effective_limit` and whether the cache was previously empty.
#[inline]
pub fn article_cache_can_alloc(
    effective_limit: usize,
    allocated: usize,
    size: usize,
) -> (bool, bool) {
    let can_alloc = effective_limit > 0
        && allocated
            .checked_add(size)
            .is_some_and(|sum| sum <= effective_limit);
    (can_alloc, allocated == 0)
}

/// Computes the effective par buffer MB when the pressure tier is Critical or Normal in `MemoryMonitor::GetEffectiveParBufferMB`.
#[inline]
pub fn memory_monitor_calc_critical_or_normal_par_buffer(
    tier: i32,
    configured_mb: i32,
    tier_critical: i32,
    tier_normal: i32,
) -> (bool, i32) {
    if tier == tier_critical {
        let mb = if configured_mb == 0 {
            0
        } else {
            configured_mb.min(16).max(8)
        };
        (true, mb)
    } else if tier == tier_normal {
        (true, configured_mb)
    } else {
        (false, 0)
    }
}

/// Computes `(eff_cache, eff_par, eff_limit)` in `MemoryMonitor::UpdateState`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn memory_monitor_resolve_tier_limits(
    next_tier: i32,
    raw_sample: i32,
    cfg_cache: u32,
    cfg_par: u32,
    cfg_limit: u32,
    tier_normal: i32,
    tier_constrained: i32,
    tier_critical: i32,
    rs_eff_cache: u32,
    rs_eff_par: u32,
) -> (u32, u32, u32) {
    let eff_cache = if next_tier == tier_critical {
        0
    } else if next_tier == tier_normal {
        cfg_cache
    } else if raw_sample == next_tier {
        rs_eff_cache
    } else {
        cfg_cache / 2
    };

    let eff_par = if next_tier == tier_critical {
        if cfg_par == 0 {
            0
        } else {
            cfg_par.min(16).max(8)
        }
    } else if next_tier == tier_normal {
        cfg_par
    } else if raw_sample == next_tier {
        rs_eff_par
    } else {
        (cfg_par / 2).max(16)
    };

    let eff_limit = if next_tier == tier_critical {
        (cfg_limit / 4).max(1)
    } else if next_tier == tier_constrained {
        (cfg_limit / 2).max(2)
    } else {
        cfg_limit
    };

    (eff_cache, eff_par, eff_limit)
}

/// Determines whether `ArticleDownloader::Run` should abort with `adRetry` due to work state or server generation changes.
#[inline]
pub fn article_dl_should_abort_for_work_state(
    is_stopped: bool,
    pause_download: bool,
    quota_reached: bool,
    force: bool,
    download_held: bool,
    extra_priority: bool,
    gen_changed: bool,
) -> bool {
    is_stopped
        || ((pause_download || quota_reached) && !force)
        || (download_held && !extra_priority)
        || gen_changed
}

/// Classifies an article download attempt status into `(record, success, failed)` server stats.
#[inline]
pub fn article_dl_classify_attempt_stat(
    status: i32,
    ad_finished: i32,
    ad_failed: i32,
    ad_not_found: i32,
    ad_crc_error: i32,
) -> (bool, i32, i32) {
    let record = status == ad_finished
        || status == ad_failed
        || status == ad_not_found
        || status == ad_crc_error;
    if !record {
        (false, 0, 0)
    } else if status == ad_finished {
        (true, 1, 0)
    } else {
        (true, 0, 1)
    }
}

/// Determines whether `ArticleDownloader::Run` should retry the same server or free the connection.
#[inline]
pub fn article_dl_should_retry_same_server(
    connected: bool,
    status: i32,
    ad_failed: i32,
    remained_retries: i32,
    retention_failure: bool,
) -> bool {
    connected && status == ad_failed && remained_retries > 0 && !retention_failure
}

/// Determines whether `ArticleDownloader::Run` should advance through the server list and whether `lastServer` should be added to `failedServers`.
#[inline]
pub fn article_dl_should_advance_server_list(
    has_want_server: bool,
    connected: bool,
    retention_failure: bool,
    optional_blocked: bool,
    auth_rejected: bool,
) -> (bool, bool) {
    let advance =
        !has_want_server && (connected || retention_failure || optional_blocked || auth_rejected);
    (advance, advance && !optional_blocked)
}

/// Determines whether `ArticleDownloader::Download` should sleep 10ms to pace uncached reads during critical memory pressure.
#[inline]
pub fn article_dl_should_pace_critical_uncached(
    is_stopped: bool,
    has_cached_buffer: bool,
    is_critical: bool,
    is_flushing: bool,
) -> bool {
    !is_stopped && !has_cached_buffer && is_critical && is_flushing
}

/// Computes `(interrupted, should_cleanup)` after the unpack loop in `DirectUnpack::Run`.
#[inline]
pub fn direct_unpack_eval_post_run_actions(
    unpack_ok: bool,
    cancelled: bool,
    is_stopped: bool,
    terminated: bool,
) -> (bool, bool) {
    let interrupted = !cancelled && (is_stopped || terminated);
    let should_cleanup = !unpack_ok && !interrupted;
    (interrupted, should_cleanup)
}

/// Determines whether `DirectUnpack::Run` should move `m_extractedArchives` into `nzbInfo->GetExtractedArchives()`.
#[inline]
pub fn direct_unpack_should_commit_extracted_archives(
    has_archives: bool,
    status: i32,
    ns_success: i32,
    interrupted: bool,
) -> bool {
    has_archives && (status == ns_success || interrupted)
}

/// Determines whether `PrePostProcessor::SanitisePostQueue` should trigger completion for an NZB whose last file finished during reload/shutdown.
#[inline]
pub fn prepost_should_complete_interrupted_download(
    has_post_info: bool,
    kind: i32,
    nk_nzb: i32,
    completed_files_empty: bool,
    delete_status: i32,
    ds_none: i32,
    download_completed: bool,
) -> bool {
    !has_post_info
        && kind == nk_nzb
        && !completed_files_empty
        && delete_status == ds_none
        && download_completed
}

/// Checks whether `PrePostProcessor::NzbAdded` should immediately mark an NZB completed due to its delete status.
#[inline]
pub fn prepost_is_skipped_delete_status(
    delete_status: i32,
    ds_dupe: i32,
    ds_copy: i32,
    ds_good: i32,
    ds_scan: i32,
) -> bool {
    delete_status == ds_dupe
        || delete_status == ds_copy
        || delete_status == ds_good
        || delete_status == ds_scan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diskstate_headers_and_statuses() {
        assert!(diskstate_validate_dupe_extras_header(4, 0, 0));
        assert!(diskstate_validate_dupe_extras_header(4, 12, 3));
        assert!(!diskstate_validate_dupe_extras_header(3, 0, 0));
        assert!(!diskstate_validate_dupe_extras_header(4, -1, 0));
        assert!(!diskstate_validate_dupe_extras_header(4, 0, -1));

        assert!(diskstate_validate_repair_job_header(11, 11, 5, 0, 2));
        assert!(diskstate_validate_repair_job_header(9, 9, 1, 1, 0));
        assert!(!diskstate_validate_repair_job_header(10, 11, 5, 0, 2));
        assert!(!diskstate_validate_repair_job_header(11, 11, 0, 0, 2));
        assert!(!diskstate_validate_repair_job_header(11, 11, 5, 2, 2));
        assert!(!diskstate_validate_repair_job_header(11, 11, 5, 0, -1));

        assert!(diskstate_should_apply_url_status(0, 0, 1, 3));
        assert!(diskstate_should_apply_url_status(1, 0, 3, 3));
        assert!(!diskstate_should_apply_url_status(1, 0, 2, 3));

        assert_eq!(diskstate_normalize_script_status(2, 24), 1);
        assert_eq!(diskstate_normalize_script_status(2, 25), 2);
        assert_eq!(diskstate_normalize_script_status(1, 20), 1);
    }

    #[test]
    fn test_diskstate_file_state_and_volumes() {
        assert!(!diskstate_eval_restored_output_init(
            0, true, false, true, true, true
        ));
        assert!(diskstate_eval_restored_output_init(
            1, false, false, false, false, false
        ));
        assert!(!diskstate_eval_restored_output_init(
            1, true, false, false, false, false
        ));
        assert!(diskstate_eval_restored_output_init(
            1, true, false, false, true, false
        ));

        assert_eq!(
            diskstate_calc_staged_fallback(false, 2, 2, 1, true, 0),
            (1, 0)
        );
        assert_eq!(
            diskstate_calc_staged_fallback(true, 2, 2, 1, true, 0),
            (0, 2)
        );

        assert!(diskstate_validate_article_count(10, true, 10));
        assert!(!diskstate_validate_article_count(11, true, 10));
        assert!(!diskstate_validate_article_count(-1, false, 0));
        assert!(!diskstate_validate_article_count(10_000_001, false, 0));

        assert!(diskstate_need_staged_file_check(
            false, 9, 0, 2, 100, 500, 1, 1, false, false
        ));
        assert!(diskstate_need_staged_file_check(
            false, 8, 2, 2, 100, 500, 0, 1, true, false
        ));
        assert!(!diskstate_need_staged_file_check(
            false, 9, 2, 2, 100, 500, 0, 1, true, false
        ));

        assert!(diskstate_is_digits_only_filename(b"12345"));
        assert!(diskstate_is_digits_only_filename(b""));
        assert!(!diskstate_is_digits_only_filename(b"123s"));

        assert!(diskstate_should_load_file_state(b'c', false, true));
        assert!(diskstate_should_load_file_state(b's', true, false));
        assert!(!diskstate_should_load_file_state(b's', true, true));

        assert_eq!(diskstate_calc_volume_slot_size(0, 10), (true, 60));
        assert_eq!(diskstate_calc_volume_slot_size(2, 10), (true, 24));
        assert_eq!(diskstate_calc_volume_slot_size(3, 365), (true, 365));
        assert_eq!(diskstate_calc_volume_slot_size(0, -1), (false, 0));
        assert_eq!(diskstate_calc_volume_slot_size(3, 100_001), (false, 0));
    }

    #[test]
    fn test_article_writer_and_memory_monitor() {
        assert!(article_writer_should_check_dupe_file(
            1, 1, true, 0, 2, false
        ));
        assert!(!article_writer_should_check_dupe_file(
            1, 1, true, 2, 2, false
        ));

        assert!(article_writer_is_duplicate_dest(false, false, true, true));
        assert!(!article_writer_is_duplicate_dest(true, false, true, true));

        assert!(article_writer_can_use_article_cache(
            250, false, 1, 1, false, false
        ));
        assert!(!article_writer_can_use_article_cache(
            250, false, 1, 1, false, true
        ));

        assert_eq!(
            article_writer_advance_and_check_range(false, 100, 50, 200),
            (150, false)
        );
        assert_eq!(
            article_writer_advance_and_check_range(false, 180, 50, 200),
            (230, true)
        );
        assert_eq!(
            article_writer_advance_and_check_range(true, 180, 50, 200),
            (180, false)
        );

        assert_eq!(
            article_writer_resolve_direct_write_ready(true, false, false, true, false),
            (true, true)
        );
        assert_eq!(
            article_writer_resolve_direct_write_ready(false, false, true, true, true),
            (true, false)
        );

        assert!(article_writer_need_gap_fill(false, false, 1024, 0));
        assert!(!article_writer_need_gap_fill(false, true, 1024, 0));

        assert_eq!(article_cache_can_alloc(1000, 0, 500), (true, true));
        assert_eq!(article_cache_can_alloc(1000, 600, 500), (false, false));

        assert_eq!(
            memory_monitor_calc_critical_or_normal_par_buffer(2, 64, 2, 0),
            (true, 16)
        );
        assert_eq!(
            memory_monitor_calc_critical_or_normal_par_buffer(0, 64, 2, 0),
            (true, 64)
        );
        assert_eq!(
            memory_monitor_calc_critical_or_normal_par_buffer(1, 64, 2, 0),
            (false, 0)
        );

        assert_eq!(
            memory_monitor_resolve_tier_limits(2, 2, 500, 250, 20, 0, 1, 2, 125, 64),
            (0, 16, 5)
        );
        assert_eq!(
            memory_monitor_resolve_tier_limits(1, 1, 500, 250, 20, 0, 1, 2, 250, 125),
            (250, 125, 10)
        );
    }

    #[test]
    fn test_article_dl_direct_unpack_and_prepost() {
        assert!(!article_dl_should_abort_for_work_state(
            false, false, false, false, false, false, false
        ));
        assert!(article_dl_should_abort_for_work_state(
            false, true, false, false, false, false, false
        ));
        assert!(!article_dl_should_abort_for_work_state(
            false, true, false, true, false, false, false
        ));

        assert_eq!(article_dl_classify_attempt_stat(3, 3, 2, 4, 5), (true, 1, 0));
        assert_eq!(article_dl_classify_attempt_stat(4, 3, 2, 4, 5), (true, 0, 1));
        assert_eq!(article_dl_classify_attempt_stat(1, 3, 2, 4, 5), (false, 0, 0));

        assert!(article_dl_should_retry_same_server(true, 2, 2, 1, false));
        assert!(!article_dl_should_retry_same_server(true, 2, 2, 1, true));

        assert_eq!(
            article_dl_should_advance_server_list(false, true, false, false, false),
            (true, true)
        );
        assert_eq!(
            article_dl_should_advance_server_list(false, false, false, true, false),
            (true, false)
        );

        assert!(article_dl_should_pace_critical_uncached(
            false, false, true, true
        ));
        assert!(!article_dl_should_pace_critical_uncached(
            false, true, true, true
        ));

        assert_eq!(
            direct_unpack_eval_post_run_actions(false, false, true, false),
            (true, false)
        );
        assert_eq!(
            direct_unpack_eval_post_run_actions(false, false, false, false),
            (false, true)
        );

        assert!(direct_unpack_should_commit_extracted_archives(
            true, 2, 2, false
        ));
        assert!(direct_unpack_should_commit_extracted_archives(
            true, 0, 2, true
        ));
        assert!(!direct_unpack_should_commit_extracted_archives(
            true, 3, 2, false
        ));

        assert!(prepost_should_complete_interrupted_download(
            false, 0, 0, false, 0, 0, true
        ));
        assert!(!prepost_should_complete_interrupted_download(
            true, 0, 0, false, 0, 0, true
        ));

        assert!(prepost_is_skipped_delete_status(3, 2, 3, 4, 5));
        assert!(!prepost_is_skipped_delete_status(0, 2, 3, 4, 5));
    }
}
