// Rust hot-path kernels for HistoryCoordinator, Scanner, RepairController, and PrePostProcessor (Batch 22).

/// Classify action for an item in `HistoryCoordinator::ServiceWork`:
/// - `0`: keep item (`++it; index++;`)
/// - `1`: hide item (`HistoryHide`, `index++`)
/// - `2`: delete disk files & erase from history
/// - `3`: erase from history without deleting disk files
pub fn history_coord_classify_service_work_item(
    kind: i32,
    hk_dup: i32,
    hk_nzb: i32,
    item_time: i64,
    min_time: i64,
    dupe_check: bool,
) -> i32 {
    if kind != hk_dup && item_time < min_time {
        if dupe_check && kind == hk_nzb {
            1
        } else if kind == hk_nzb {
            2
        } else {
            3
        }
    } else {
        0
    }
}

/// Resolve post-history direct rename and direct unpack statuses in `HistoryCoordinator::AddToHistory`.
pub fn history_coord_resolve_post_history_direct_statuses(
    direct_rename_status: i32,
    ts_running: i32,
    ts_failure: i32,
    direct_unpack_status: i32,
    ns_running: i32,
    ns_none: i32,
) -> (i32, i32) {
    let new_rename = if direct_rename_status == ts_running {
        ts_failure
    } else {
        direct_rename_status
    };
    let new_unpack = if direct_unpack_status == ns_running {
        ns_none
    } else {
        direct_unpack_status
    };
    (new_rename, new_unpack)
}

/// Check whether a completed file should be sampled in `HistoryCoordinator::RecheckFailedArticles`,
/// returning `(should_sample, is_partial)`.
pub fn history_coord_should_sample_completed_file(
    status: i32,
    cf_partial: i32,
    cf_failure: i32,
    file_id: i32,
) -> (bool, bool) {
    if (status != cf_partial && status != cf_failure) || file_id <= 0 {
        (false, false)
    } else {
        (true, status == cf_partial)
    }
}

/// Check whether an article should be added to the failed sample list in `HistoryCoordinator::RecheckFailedArticles`.
pub fn history_coord_should_sample_article(
    is_partial: bool,
    article_status: i32,
    ai_failed: i32,
    message_id: &[u8],
) -> bool {
    (!is_partial || article_status == ai_failed) && !message_id.is_empty()
}

/// Calculate reset `PostTotalSec` and normalized `MoveStatus` in `HistoryCoordinator::MoveToQueue`.
pub fn history_coord_calc_move_to_queue_reset(
    unpack_cleaned_up_disk: bool,
    post_total_sec: i32,
    unpack_sec: i32,
    has_main_pars: bool,
    par_sec: i32,
    move_status: i32,
    ms_failure: i32,
    ms_none: i32,
) -> (i32, i32) {
    let mut new_post_total_sec = post_total_sec;
    if !unpack_cleaned_up_disk {
        new_post_total_sec -= unpack_sec;
        if has_main_pars {
            new_post_total_sec -= par_sec;
        }
    }
    let new_move_status = if move_status == ms_failure {
        ms_none
    } else {
        move_status
    };
    (new_post_total_sec, new_move_status)
}

/// Resolve URL redownload dupe hint in `HistoryCoordinator::HistoryRedownload`.
pub fn history_coord_resolve_url_redownload_dupe_hint(
    cur_dupe_hint: i32,
    dh_none: i32,
    dh_redownload_manual: i32,
) -> i32 {
    if cur_dupe_hint == dh_none {
        dh_redownload_manual
    } else {
        cur_dupe_hint
    }
}

/// Classify action in `HistoryCoordinator::HistoryReturn`:
/// - `1`: `HistoryRedownload` (`hkUrl`)
/// - `2`: warn no files left (`hkNzb && parked_file_count == 0`)
/// - `3`: `HistoryRetry` (`hkNzb && parked_file_count != 0`)
/// - `4`: error wrong type
pub fn history_coord_classify_return_action(
    kind: i32,
    hk_url: i32,
    hk_nzb: i32,
    parked_file_count: i32,
) -> i32 {
    if kind == hk_url {
        1
    } else if kind == hk_nzb && parked_file_count == 0 {
        2
    } else if kind == hk_nzb {
        3
    } else {
        4
    }
}

/// Evaluate whether a loaded file in `HistoryCoordinator::HistoryRetry` should be requeued.
pub fn history_coord_eval_requeue_loaded_file(
    status: i32,
    cf_none: i32,
    cf_failure: i32,
    cf_partial: i32,
    reset_failed: bool,
    state_loaded: bool,
    remaining_size: i64,
) -> bool {
    status == cf_none
        || (status == cf_failure && reset_failed)
        || (status == cf_partial && state_loaded && (reset_failed || remaining_size > 0))
}

/// Check if a completed partial file has at least one retryable article in `HistoryCoordinator::HistoryRetry`.
pub fn history_coord_has_retryable_article(
    statuses: &[i32],
    reset_failed: bool,
    ai_undefined: i32,
    ai_failed: i32,
) -> bool {
    statuses
        .iter()
        .any(|&st| st == ai_undefined || (reset_failed && st == ai_failed))
}

/// Apply initial header & NzbInfo counter adjustments in `HistoryCoordinator::ResetArticles`.
#[allow(clippy::too_many_arguments)]
pub fn history_coord_reset_articles_header(
    all_failed: bool,
    is_par_file: bool,
    file_size: i64,
    file_missed_size: i64,
    file_total_articles: i32,
    file_missed_articles: i32,
    cur_file_failed_size: i64,
    cur_file_failed_articles: i32,
    cur_file_remaining_size: i64,
    cur_file_completed_articles: i32,
    file_success_size: i64,
    file_success_articles: i32,
    cur_nzb_failed_size: i64,
    cur_nzb_success_size: i64,
    cur_nzb_failed_articles: i32,
    cur_nzb_success_articles: i32,
    cur_nzb_par_failed_size: i64,
    cur_nzb_par_success_size: i64,
) -> (i64, i32, i64, i32, i64, i64, i32, i32, i64, i64) {
    let (file_failed_size, file_failed_articles, file_remaining_size, file_completed_articles) =
        if all_failed {
            let fs = file_size - file_missed_size;
            let fa = file_total_articles - file_missed_articles;
            (fs, fa, 0, fa)
        } else {
            (
                cur_file_failed_size,
                cur_file_failed_articles,
                cur_file_remaining_size,
                cur_file_completed_articles,
            )
        };

    let nzb_failed_size = cur_nzb_failed_size - file_failed_size;
    let nzb_success_size = cur_nzb_success_size - file_success_size;
    let nzb_failed_articles = cur_nzb_failed_articles - file_failed_articles;
    let nzb_success_articles = cur_nzb_success_articles - file_success_articles;
    let (nzb_par_failed_size, nzb_par_success_size) = if is_par_file {
        (
            cur_nzb_par_failed_size - file_failed_size,
            cur_nzb_par_success_size - file_success_size,
        )
    } else {
        (cur_nzb_par_failed_size, cur_nzb_par_success_size)
    };

    (
        file_failed_size,
        file_failed_articles,
        file_remaining_size,
        file_completed_articles,
        nzb_failed_size,
        nzb_success_size,
        nzb_failed_articles,
        nzb_success_articles,
        nzb_par_failed_size,
        nzb_par_success_size,
    )
}

/// Apply per-article counter delta when resetting an article in `HistoryCoordinator::ResetArticles`.
#[allow(clippy::too_many_arguments)]
pub fn history_coord_apply_article_reset_delta(
    pa_status: i32,
    ai_failed: i32,
    ai_undefined: i32,
    ai_finished: i32,
    pa_size: i32,
    cur_completed_articles: i32,
    cur_remaining_size: i64,
    cur_failed_articles: i32,
    cur_failed_size: i64,
    cur_success_articles: i32,
    cur_success_size: i64,
) -> (i32, i64, i32, i64, i32, i64) {
    let completed_articles = cur_completed_articles - 1;
    let remaining_size = cur_remaining_size + pa_size as i64;
    let mut failed_articles = cur_failed_articles;
    let mut failed_size = cur_failed_size;
    let mut success_articles = cur_success_articles;
    let mut success_size = cur_success_size;

    if pa_status == ai_failed || pa_status == ai_undefined {
        failed_articles -= 1;
        failed_size -= pa_size as i64;
    } else if pa_status == ai_finished {
        success_articles -= 1;
        success_size -= pa_size as i64;
    }

    (
        completed_articles,
        remaining_size,
        failed_articles,
        failed_size,
        success_articles,
        success_size,
    )
}

/// Check whether `Scanner::ProcessIncomingFile` should execute scan scripts for `extension`.
pub fn scanner_should_run_scan_script(scan_script: bool, extension: &[u8]) -> bool {
    scan_script && !extension.eq_ignore_ascii_case(b".nzb_processed")
}

/// Classify incoming file dispatch in `Scanner::ProcessIncomingFile`:
/// Returns `(rename_to_processed_after_script, dispatch_action)` where `dispatch_action`:
/// - `1`: `.nzb_processed` (rename `.nzb` and `AddFileToQueue`)
/// - `2`: `.nzb` when `exists` (`AddFileToQueue`)
/// - `0`: no queue action
pub fn scanner_classify_incoming_extension(
    ran_scan_script: bool,
    exists: bool,
    extension: &[u8],
) -> (bool, i32) {
    let is_nzb = extension.eq_ignore_ascii_case(b".nzb");
    let is_nzb_processed = extension.eq_ignore_ascii_case(b".nzb_processed");
    let rename_to_processed = ran_scan_script && exists && !is_nzb;
    let action = if is_nzb_processed {
        1
    } else if exists && is_nzb {
        2
    } else {
        0
    };
    (rename_to_processed, action)
}

/// Classify outcome routing in `Scanner::AddFileToQueue`:
/// - `1`: `ok` -> `AddNzbFileToQueue`
/// - `2`: `!ok && has_url_info` -> copy cached messages to `urlInfo`
/// - `3`: `!ok && !has_url_info` -> set `dsScan` and `AddNzbFileToQueue`
pub fn scanner_classify_add_file_outcome(ok: bool, has_url_info: bool) -> i32 {
    if ok {
        1
    } else if has_url_info {
        2
    } else {
        3
    }
}

/// Aggregate archive add statuses in `Scanner::AddArchive`.
pub fn scanner_resolve_archive_add_status(
    has_url_info: bool,
    statuses: &[i32],
    as_success: i32,
    as_skipped: i32,
) -> i32 {
    if statuses.is_empty() {
        return as_skipped;
    }
    if has_url_info {
        return statuses[0];
    }
    if statuses.contains(&as_success) {
        as_success
    } else {
        statuses[0]
    }
}

/// Check cheap in-memory conditions for a twin candidate in `RepairController::PostParChecker::RequestTwinIndex`
/// before calling `FileSystem::FileExists`.
pub fn repair_ctrl_should_consider_twin_candidate(
    is_self: bool,
    kind: i32,
    nk_nzb: i32,
    same_name_or_key: bool,
    queued_filename: &[u8],
    has_print: bool,
    own_print_matches: bool,
) -> bool {
    if is_self || kind != nk_nzb || !same_name_or_key || queued_filename.contains(&b'|') {
        return false;
    }
    if has_print && !own_print_matches {
        return false;
    }
    true
}

/// Sum available twin volume blocks and check if they cover `block_needed` in `RepairController::PostParChecker::RequestTwinVolumes`.
pub fn repair_ctrl_sum_and_check_twin_blocks(vol_blocks: &[i32], block_needed: i32) -> (i32, bool) {
    let available: i32 = vol_blocks.iter().copied().sum();
    (available, available >= block_needed)
}

/// Calculate updated extra par blocks in `RepairController::PostParChecker::StatDupeSources`.
pub fn repair_ctrl_calc_dupe_source_extra_blocks(
    cur_history_extra: i32,
    used_blocks: i32,
    cur_total_extra: i32,
) -> (i32, i32) {
    (
        cur_history_extra - used_blocks,
        cur_total_extra + used_blocks,
    )
}

/// Determine whether `ParQuick` should be used in `RepairController::Run`.
pub fn repair_ctrl_should_use_par_quick(
    par_quick_opt: bool,
    force_par_full: bool,
    dupe_repaired: bool,
) -> bool {
    par_quick_opt && !force_par_full && !dupe_repaired
}

/// Evaluate stage transition and repair time accumulation in `RepairController::UpdateParCheckProgress`.
pub fn repair_ctrl_eval_stage_transition(
    cur_post_stage: i32,
    new_stage: i32,
    pt_repairing: i32,
    pt_verifying_repaired: i32,
    current_time: i64,
    repair_time: i64,
    cur_repair_sec: i32,
) -> (bool, bool, i32) {
    if cur_post_stage != new_stage {
        let set_repair_time = new_stage == pt_repairing;
        let new_repair_sec = if new_stage == pt_verifying_repaired {
            cur_repair_sec + (current_time - repair_time) as i32
        } else {
            cur_repair_sec
        };
        (true, set_repair_time, new_repair_sec)
    } else {
        (false, false, cur_repair_sec)
    }
}

/// Classify entry routing in `PrePostProcessor::NzbDownloaded`:
/// Returns `(route, enqueue_deleted_script, set_par_skipped)` where `route`:
/// - `1`: skip-script + skip-write immediate cleanup and `NzbCompleted`
/// - `2`: queue for post-processing
/// - `3`: direct `NzbCompleted`
#[allow(clippy::too_many_arguments)]
pub fn prepost_classify_nzb_downloaded_entry(
    skip_script_processing: bool,
    skip_disk_write: bool,
    opt_skip_write: bool,
    delete_status: i32,
    ds_health: i32,
    ds_bad: i32,
    has_post_info: bool,
    opt_raw_article: bool,
    par_status: i32,
    ps_none: i32,
    par_check_opt: i32,
    pc_always: i32,
    pc_force: i32,
) -> (i32, bool, bool) {
    if skip_script_processing && (skip_disk_write || opt_skip_write) {
        return (1, false, false);
    }
    let enqueue_deleted = delete_status == ds_health || delete_status == ds_bad;
    if !has_post_info && !opt_raw_article && !opt_skip_write && !skip_disk_write {
        let set_par_skipped =
            par_status == ps_none && par_check_opt != pc_always && par_check_opt != pc_force;
        (2, enqueue_deleted, set_par_skipped)
    } else {
        (3, enqueue_deleted, false)
    }
}

/// Resolve delete status and route in `PrePostProcessor::NzbDeleted`:
/// Returns `(new_delete_status, call_nzb_downloaded)`.
pub fn prepost_resolve_nzb_deleted_route(
    delete_status: i32,
    ds_none: i32,
    ds_manual: i32,
    ds_health: i32,
    ds_bad: i32,
) -> (i32, bool) {
    let new_status = if delete_status == ds_none {
        ds_manual
    } else {
        delete_status
    };
    let call_downloaded = new_status == ds_health || new_status == ds_bad;
    (new_status, call_downloaded)
}

/// Classify disk cleanup mode in `PrePostProcessor::DeleteCleanup`:
/// - `0`: no cleanup
/// - `1`: skip-write mode (`DeleteDirectoryWithContent` on `destDir`)
/// - `2`: standard cleanup (delete completed files, `.out.tmp` files, and empty dirs)
pub fn prepost_classify_delete_cleanup_mode(
    cleanup_disk: bool,
    delete_status: i32,
    ds_dupe: i32,
    parking: bool,
    skip_disk_write: bool,
    opt_skip_write: bool,
) -> i32 {
    if cleanup_disk || (delete_status == ds_dupe && !parking) {
        if skip_disk_write || opt_skip_write {
            1
        } else {
            2
        }
    } else {
        0
    }
}

/// Classify action for `PrePostProcessor::CheckRequestPar`:
/// Returns `(action, new_force_par_full)` where `action`:
/// - `0`: no action
/// - `1`: restart job (`psNone`, clear scripts, `ptQueued`, `working = false`)
/// - `2`: manual par-check with remaining files (`psManual`, `eaGroupResume`, `ptFinished`, `working = false`)
/// - `3`: manual par-check with no remaining files (`psManual`, `working = false`)
#[allow(clippy::too_many_arguments)]
pub fn prepost_classify_request_par_action(
    request_par_check: bool,
    par_status: i32,
    ps_skipped: i32,
    force_repair: bool,
    par_full: bool,
    par_check_opt: i32,
    pc_manual: i32,
    cur_force_par_full: bool,
    file_list_empty: bool,
) -> (i32, bool) {
    if request_par_check
        && (par_status <= ps_skipped || (force_repair && !par_full))
        && par_check_opt != pc_manual
    {
        let new_force_par_full = cur_force_par_full || par_status > ps_skipped;
        (1, new_force_par_full)
    } else if request_par_check && par_status <= ps_skipped && par_check_opt == pc_manual {
        if !file_list_empty {
            (2, cur_force_par_full)
        } else {
            (3, cur_force_par_full)
        }
    } else {
        (0, cur_force_par_full)
    }
}

/// Check whether `ParCannotCover` can return `false` early from collection health.
pub fn prepost_should_skip_par_cannot_cover_check(health: i32, critical_health: i32) -> bool {
    health > 0 && (critical_health >= 1000 || health >= critical_health)
}

/// Calculate a stream-repair job's protected damage and whether total damage exceeds remaining par bytes in `ParCannotCover`.
pub fn prepost_calc_job_protected_damage(
    decoded_file_size: i64,
    holes_total_size: i64,
    failed_size: i64,
    missed_size: i64,
) -> i64 {
    if decoded_file_size > 0 {
        holes_total_size
    } else {
        failed_size + missed_size
    }
}

/// Check if protected damage exceeds available par2 recovery bytes in `ParCannotCover`.
pub fn prepost_is_damage_beyond_par(
    protected_damage: i64,
    par_size: i64,
    par_current_failed_size: i64,
) -> bool {
    protected_damage > par_size - par_current_failed_size
}

/// Check cheap pre-conditions for skipping par-check after unrecovered stream repair in `PrePostProcessor::StartJob`.
#[allow(clippy::too_many_arguments)]
pub fn prepost_should_check_unrecovered_stream_skip(
    par_cannot_cover: bool,
    stream_repair_done: bool,
    stream_repair_recovered: bool,
    dupe_recovered_bytes: i64,
    par_check_opt: i32,
    pc_auto: i32,
    par_status: i32,
    ps_none: i32,
    delete_status: i32,
    ds_none: i32,
) -> bool {
    par_cannot_cover
        && stream_repair_done
        && !stream_repair_recovered
        && dupe_recovered_bytes == 0
        && par_check_opt == pc_auto
        && par_status == ps_none
        && delete_status == ds_none
}

/// Classify skipped par-check health action in `PrePostProcessor::StartJob` (before calling `ParParser::FindMainPars`):
/// - `0`: no action
/// - `1`: skip par-check due to health 0%
/// - `2`: skip par-check due to health below critical
/// - `3`: request par-check due to failed non-par bytes
#[allow(clippy::too_many_arguments)]
pub fn prepost_classify_skipped_par_health(
    par_status: i32,
    ps_skipped: i32,
    par_scan_opt: i32,
    ps_dupe: i32,
    health: i32,
    critical_health: i32,
    failed_size: i64,
    par_failed_size: i64,
) -> i32 {
    if par_status != ps_skipped {
        return 0;
    }
    if (par_scan_opt != ps_dupe && health < critical_health && critical_health < 1000)
        || health == 0
    {
        if health == 0 {
            1
        } else {
            2
        }
    } else if failed_size - par_failed_size > 0 {
        3
    } else {
        0
    }
}

/// Evaluate `wantUnpack`, `unpack`, and whether to start `RenameController::jkRar` in `PrePostProcessor::StartJob`.
#[allow(clippy::too_many_arguments)]
pub fn prepost_eval_want_unpack_and_rar_rename(
    has_unpack_param: bool,
    unpack_param_val: &[u8],
    unpack_status: i32,
    us_none: i32,
    delete_status: i32,
    ds_none: i32,
    rar_rename_status: i32,
    rs_none: i32,
    opt_rar_rename: bool,
) -> (bool, bool, bool) {
    let want_unpack = !(has_unpack_param && unpack_param_val.eq_ignore_ascii_case(b"no"));
    let unpack = want_unpack && unpack_status == us_none && delete_status == ds_none;
    let start_rar_rename = rar_rename_status == rs_none && unpack && opt_rar_rename;
    (want_unpack, unpack, start_rar_rename)
}

/// Classify direct-unpack action in `PrePostProcessor::FileDownloaded`:
/// - `0`: do nothing
/// - `1`: candidate for starting `DirectUnpack::StartJob`
/// - `2`: notify running `DirectUnpack::FileDownloaded`
#[allow(clippy::too_many_arguments)]
pub fn prepost_classify_file_downloaded_direct_unpack(
    opt_direct_unpack: bool,
    opt_raw_article: bool,
    opt_skip_write: bool,
    skip_disk_write: bool,
    direct_unpack_status: i32,
    ns_none: i32,
    ns_running: i32,
    direct_rename_status: i32,
    ts_running: i32,
    has_unpack_thread: bool,
) -> i32 {
    if !opt_direct_unpack || opt_raw_article || opt_skip_write || skip_disk_write {
        return 0;
    }
    if direct_unpack_status == ns_none && direct_rename_status != ts_running {
        1
    } else if direct_unpack_status == ns_running && has_unpack_thread {
        2
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_history_coord_kernels() {
        assert_eq!(
            history_coord_classify_service_work_item(1, 3, 1, 90, 100, true),
            1
        );
        assert_eq!(
            history_coord_classify_service_work_item(1, 3, 1, 90, 100, false),
            2
        );
        assert_eq!(
            history_coord_classify_service_work_item(2, 3, 1, 90, 100, true),
            3
        );
        assert_eq!(
            history_coord_classify_service_work_item(3, 3, 1, 90, 100, true),
            0
        );

        assert_eq!(
            history_coord_resolve_post_history_direct_statuses(1, 1, 2, 1, 1, 0),
            (2, 0)
        );
        assert_eq!(
            history_coord_should_sample_completed_file(2, 2, 3, 10),
            (true, true)
        );
        assert!(!history_coord_should_sample_article(true, 1, 2, b"msg@id"));
        assert!(history_coord_should_sample_article(true, 2, 2, b"msg@id"));
        assert_eq!(
            history_coord_calc_move_to_queue_reset(false, 100, 30, true, 20, 2, 2, 0),
            (50, 0)
        );
        assert_eq!(history_coord_resolve_url_redownload_dupe_hint(0, 0, 2), 2);
        assert_eq!(history_coord_classify_return_action(2, 2, 1, 0), 1);
        assert_eq!(history_coord_classify_return_action(1, 2, 1, 0), 2);
        assert_eq!(history_coord_classify_return_action(1, 2, 1, 3), 3);
        assert!(history_coord_eval_requeue_loaded_file(
            2, 0, 3, 2, false, true, 500
        ));
        assert!(history_coord_has_retryable_article(&[3, 2], true, 0, 2));

        let hdr = history_coord_reset_articles_header(
            true, true, 1000, 100, 10, 1, 0, 0, 500, 0, 0, 0, 2000, 500, 20, 5, 1000, 200,
        );
        assert_eq!(hdr.0, 900);
        assert_eq!(hdr.1, 9);
        assert_eq!(hdr.2, 0);
        assert_eq!(hdr.4, 1100);
        assert_eq!(hdr.8, 100);

        let delta =
            history_coord_apply_article_reset_delta(2, 2, 0, 1, 100, 5, 200, 3, 300, 2, 200);
        assert_eq!(delta, (4, 300, 2, 200, 2, 200));
    }

    #[test]
    fn test_scanner_and_repair_kernels() {
        assert!(scanner_should_run_scan_script(true, b".nzb"));
        assert!(!scanner_should_run_scan_script(true, b".nzb_processed"));
        assert_eq!(
            scanner_classify_incoming_extension(true, true, b".nzb"),
            (false, 2)
        );
        assert_eq!(
            scanner_classify_incoming_extension(false, true, b".nzb_processed"),
            (false, 1)
        );
        assert_eq!(
            scanner_classify_incoming_extension(true, true, b".zip"),
            (true, 0)
        );
        assert_eq!(scanner_classify_add_file_outcome(false, true), 2);
        assert_eq!(
            scanner_resolve_archive_add_status(false, &[0, 1], 1, 0),
            1
        );

        assert!(repair_ctrl_should_consider_twin_candidate(
            false,
            0,
            0,
            true,
            b"/tmp/a.nzb",
            true,
            true
        ));
        assert!(!repair_ctrl_should_consider_twin_candidate(
            false,
            0,
            0,
            true,
            b"/tmp/a.nzb|/tmp/b.nzb",
            false,
            false
        ));
        assert_eq!(
            repair_ctrl_sum_and_check_twin_blocks(&[2, 4, 8], 10),
            (14, true)
        );
        assert_eq!(repair_ctrl_calc_dupe_source_extra_blocks(10, 4, 2), (6, 6));
        assert!(repair_ctrl_should_use_par_quick(true, false, false));
        assert_eq!(
            repair_ctrl_eval_stage_transition(2, 3, 2, 3, 150, 100, 10),
            (true, false, 60)
        );
    }

    #[test]
    fn test_prepost_kernels() {
        assert_eq!(
            prepost_classify_nzb_downloaded_entry(
                false, false, false, 1, 1, 2, false, false, 0, 0, 0, 2, 3
            ),
            (2, true, true)
        );
        assert_eq!(prepost_resolve_nzb_deleted_route(0, 0, 1, 2, 3), (1, false));
        assert_eq!(
            prepost_classify_delete_cleanup_mode(false, 2, 2, false, false, false),
            2
        );
        assert_eq!(
            prepost_classify_request_par_action(true, 1, 1, false, false, 0, 3, false, false),
            (1, false)
        );
        assert!(prepost_should_skip_par_cannot_cover_check(950, 900));
        assert_eq!(prepost_calc_job_protected_damage(1000, 250, 500, 100), 250);
        assert!(prepost_is_damage_beyond_par(300, 500, 250));
        assert!(prepost_should_check_unrecovered_stream_skip(
            true, true, false, 0, 0, 0, 0, 0, 0, 0
        ));
        assert_eq!(
            prepost_classify_skipped_par_health(1, 1, 0, 2, 800, 900, 100, 0),
            2
        );
        assert_eq!(
            prepost_eval_want_unpack_and_rar_rename(
                true, b"yes", 0, 0, 0, 0, 0, 0, true
            ),
            (true, true, true)
        );
        assert_eq!(
            prepost_classify_file_downloaded_direct_unpack(
                true, false, false, false, 0, 0, 1, 0, 1, false
            ),
            1
        );
    }
}
