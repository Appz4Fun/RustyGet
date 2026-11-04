//! Batch 40: Post-processing, unpack controller, direct unpack, and history coordinator hot paths.

/// `PrePostProcessor::DownloadQueueUpdate`: returns true when `action` is
/// `eaNzbFound` or `eaUrlFound`.
#[inline]
pub fn pur_prepost_is_nzb_or_url_found_action(
    action: i32,
    ea_nzb_found: i32,
    ea_url_found: i32,
) -> bool {
    action == ea_nzb_found || action == ea_url_found
}

/// `PrePostProcessor::DownloadQueueUpdate`: returns true when `action` is
/// `eaNzbAdded` or `eaUrlAdded`.
#[inline]
pub fn pur_prepost_is_nzb_or_url_added_action(
    action: i32,
    ea_nzb_added: i32,
    ea_url_added: i32,
) -> bool {
    action == ea_nzb_added || action == ea_url_added
}

/// `PrePostProcessor::DownloadQueueUpdate`: returns true when `action` is
/// `eaFileCompleted` or `eaFileDeleted`.
#[inline]
pub fn pur_prepost_is_file_completed_or_deleted_action(
    action: i32,
    ea_file_completed: i32,
    ea_file_deleted: i32,
) -> bool {
    action == ea_file_completed || action == ea_file_deleted
}

/// `PrePostProcessor::NzbFound`: returns true when duplicate check should run
/// for the newly found NZB (`dupe_check && dupe_mode != dm_force`).
#[inline]
pub fn pur_prepost_should_run_dupe_found(
    dupe_check: bool,
    dupe_mode: i32,
    dm_force: i32,
) -> bool {
    dupe_check && dupe_mode != dm_force
}

/// `PrePostProcessor::CheckPostQueue`: returns true when a picked post-job in
/// `ptQueued` stage can be started (`stage == pt_queued && (!pause_post_process || force_priority)`).
#[inline]
pub fn pur_prepost_can_start_queued_post_job(
    stage: i32,
    pt_queued: i32,
    pause_post_process: bool,
    force_priority: bool,
) -> bool {
    stage == pt_queued && (!pause_post_process || force_priority)
}

/// `PrePostProcessor::StartJob`: returns true when a skipped PAR check should
/// be requested because `stream_repair` is active (`stream_repair && par_status == ps_skipped`).
#[inline]
pub fn pur_prepost_should_request_par_for_skipped_stream_repair(
    stream_repair: bool,
    par_status: i32,
    ps_skipped: i32,
) -> bool {
    stream_repair && par_status == ps_skipped
}

/// `PrePostProcessor::StartJob`: returns true when PAR status is `psNone` and
/// delete status is `dsNone`.
#[inline]
pub fn pur_prepost_is_par_none_and_not_deleted(
    par_status: i32,
    ps_none: i32,
    delete_status: i32,
    ds_none: i32,
) -> bool {
    par_status == ps_none && delete_status == ds_none
}

/// `PrePostProcessor::UpdatePauseState`: evaluates the transition in temporary
/// download pause state:
/// - `1`: pausing download before post-processing (`need_pause && !temp_pause_download`)
/// - `2`: unpausing download after post-processing (`!need_pause && temp_pause_download`)
/// - `0`: no state transition log message
#[inline]
pub fn pur_prepost_eval_temp_pause_download_transition(
    need_pause: bool,
    temp_pause_download: bool,
) -> i32 {
    if need_pause && !temp_pause_download {
        1
    } else if !need_pause && temp_pause_download {
        2
    } else {
        0
    }
}

/// `PrePostProcessor::FileDownloaded`: returns true when `DirectUnpack::StartJob`
/// should be invoked (`want_unpack && failed_articles == 0`).
#[inline]
pub fn pur_prepost_should_start_direct_unpack_job(
    want_unpack: bool,
    failed_articles: i32,
) -> bool {
    want_unpack && failed_articles == 0
}

/// `HistoryCoordinator::RecheckFailedArticles`: returns true when a duplicate
/// backup in history should be checked before probing (`dupe_check && dupe_mode == dm_score`).
#[inline]
pub fn pur_history_coord_should_skip_recheck_for_dupe_backup(
    dupe_check: bool,
    dupe_mode: i32,
    dm_score: i32,
) -> bool {
    dupe_check && dupe_mode == dm_score
}

/// `HistoryCoordinator::HistoryRetry`: returns true when the recorded filename
/// from `CompletedFile` should overwrite `fileInfo->GetFilename()`.
#[inline]
pub fn pur_history_coord_should_keep_recorded_filename(
    temporary_name: bool,
    file_filename_empty: bool,
) -> bool {
    !temporary_name || file_filename_empty
}

/// `HistoryCoordinator::HistoryRetry`: returns true when the output file should
/// be deleted before retrying (`success_articles == 0 || file_size == 0`).
#[inline]
pub fn pur_history_coord_should_delete_empty_or_zero_success_output(
    success_articles: i32,
    file_size: i64,
) -> bool {
    success_articles == 0 || file_size == 0
}

/// `HistoryCoordinator::HistoryRetry`: returns true when `HealthPaused` should
/// be set on an NZB returned from history (`!reset_failed && !reprocess`).
#[inline]
pub fn pur_history_coord_should_pause_health_on_remaining_retry(
    reset_failed: bool,
    reprocess: bool,
) -> bool {
    !reset_failed && !reprocess
}

/// `HistoryCoordinator::HistorySetParameter` / `HistorySetCategory` /
/// `HistorySetName` / `HistorySetDupeParam`: returns true when the history item
/// kind is `hkNzb` or `hkUrl`.
#[inline]
pub fn pur_history_coord_is_nzb_or_url_kind(
    kind: i32,
    hk_nzb: i32,
    hk_url: i32,
) -> bool {
    kind == hk_nzb || kind == hk_url
}

/// `HistoryCoordinator::HistorySetName`: returns true when `DupeSearch` should
/// be notified of a name change (`kind == hk_nzb && has_dupe_search && name_changed`).
#[inline]
pub fn pur_history_coord_should_notify_dupe_search_name_changed(
    kind: i32,
    hk_nzb: i32,
    has_dupe_search: bool,
    name_changed: bool,
) -> bool {
    kind == hk_nzb && has_dupe_search && name_changed
}

/// `HistoryCoordinator::HistorySetDupeParam`: returns true when the NZB delete
/// status is valid for toggling `eaHistorySetDupeBackup` (`dsDupe` or `dsManual`).
#[inline]
pub fn pur_history_coord_is_valid_dupe_backup_delete_status(
    delete_status: i32,
    ds_dupe: i32,
    ds_manual: i32,
) -> bool {
    delete_status == ds_dupe || delete_status == ds_manual
}

/// `UnpackController::UnpackArchives`: returns true when initial unpack should
/// be executed before any password-list loop (`!unpack_tried || par_repaired`).
#[inline]
pub fn pur_unpack_ctrl_should_execute_initial_unpack(
    unpack_tried: bool,
    par_repaired: bool,
) -> bool {
    !unpack_tried || par_repaired
}

/// `UnpackController::ExecuteUnrar`, `UnpackController::ExecuteSevenZip`, and
/// `DirectUnpack::ExecuteUnrar`: returns true when the default `"x"` extraction
/// command flag should be added (`!has_x && !has_e`).
#[inline]
pub fn pur_unpack_ctrl_should_add_extract_cmd_flag(
    has_x: bool,
    has_e: bool,
) -> bool {
    !has_x && !has_e
}

/// `UnpackController::ExecuteUnrar` and `DirectUnpack::ExecuteUnrar`: returns
/// true when the default `"-o+"` overwrite flag should be added
/// (`!has_o_plus && !has_o_minus && !has_or`).
#[inline]
pub fn pur_unpack_ctrl_should_add_default_overwrite_flag(
    has_o_plus: bool,
    has_o_minus: bool,
    has_or: bool,
) -> bool {
    !has_o_plus && !has_o_minus && !has_or
}

/// `DirectUnpack::ExecuteUnrar`: returns true when an unrar exit code error
/// message should be logged (`!unpack_ok && exit_code > 0 && !interrupted`).
#[inline]
pub fn pur_direct_unpack_should_log_unrar_exit_error(
    unpack_ok: bool,
    exit_code: i32,
    interrupted: bool,
) -> bool {
    !unpack_ok && exit_code > 0 && !interrupted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepost_actions_and_dupe_found() {
        assert!(pur_prepost_is_nzb_or_url_found_action(1, 1, 2));
        assert!(pur_prepost_is_nzb_or_url_found_action(2, 1, 2));
        assert!(!pur_prepost_is_nzb_or_url_found_action(3, 1, 2));

        assert!(pur_prepost_is_nzb_or_url_added_action(4, 4, 5));
        assert!(pur_prepost_is_nzb_or_url_added_action(5, 4, 5));
        assert!(!pur_prepost_is_nzb_or_url_added_action(1, 4, 5));

        assert!(pur_prepost_is_file_completed_or_deleted_action(7, 7, 8));
        assert!(pur_prepost_is_file_completed_or_deleted_action(8, 7, 8));
        assert!(!pur_prepost_is_file_completed_or_deleted_action(6, 7, 8));

        assert!(pur_prepost_should_run_dupe_found(true, 0, 2));
        assert!(!pur_prepost_should_run_dupe_found(false, 0, 2));
        assert!(!pur_prepost_should_run_dupe_found(true, 2, 2));
    }

    #[test]
    fn test_prepost_job_and_pause_transitions() {
        assert!(pur_prepost_can_start_queued_post_job(0, 0, false, false));
        assert!(pur_prepost_can_start_queued_post_job(0, 0, true, true));
        assert!(!pur_prepost_can_start_queued_post_job(0, 0, true, false));
        assert!(!pur_prepost_can_start_queued_post_job(1, 0, false, true));

        assert!(pur_prepost_should_request_par_for_skipped_stream_repair(true, 4, 4));
        assert!(!pur_prepost_should_request_par_for_skipped_stream_repair(false, 4, 4));
        assert!(!pur_prepost_should_request_par_for_skipped_stream_repair(true, 0, 4));

        assert!(pur_prepost_is_par_none_and_not_deleted(0, 0, 0, 0));
        assert!(!pur_prepost_is_par_none_and_not_deleted(1, 0, 0, 0));
        assert!(!pur_prepost_is_par_none_and_not_deleted(0, 0, 1, 0));

        assert_eq!(pur_prepost_eval_temp_pause_download_transition(true, false), 1);
        assert_eq!(pur_prepost_eval_temp_pause_download_transition(false, true), 2);
        assert_eq!(pur_prepost_eval_temp_pause_download_transition(true, true), 0);
        assert_eq!(pur_prepost_eval_temp_pause_download_transition(false, false), 0);

        assert!(pur_prepost_should_start_direct_unpack_job(true, 0));
        assert!(!pur_prepost_should_start_direct_unpack_job(false, 0));
        assert!(!pur_prepost_should_start_direct_unpack_job(true, 1));
    }

    #[test]
    fn test_history_coord_decisions() {
        assert!(pur_history_coord_should_skip_recheck_for_dupe_backup(true, 0, 0));
        assert!(!pur_history_coord_should_skip_recheck_for_dupe_backup(false, 0, 0));
        assert!(!pur_history_coord_should_skip_recheck_for_dupe_backup(true, 1, 0));

        assert!(pur_history_coord_should_keep_recorded_filename(false, false));
        assert!(pur_history_coord_should_keep_recorded_filename(true, true));
        assert!(!pur_history_coord_should_keep_recorded_filename(true, false));

        assert!(pur_history_coord_should_delete_empty_or_zero_success_output(0, 500));
        assert!(pur_history_coord_should_delete_empty_or_zero_success_output(5, 0));
        assert!(!pur_history_coord_should_delete_empty_or_zero_success_output(5, 500));

        assert!(pur_history_coord_should_pause_health_on_remaining_retry(false, false));
        assert!(!pur_history_coord_should_pause_health_on_remaining_retry(true, false));
        assert!(!pur_history_coord_should_pause_health_on_remaining_retry(false, true));

        assert!(pur_history_coord_is_nzb_or_url_kind(1, 1, 2));
        assert!(pur_history_coord_is_nzb_or_url_kind(2, 1, 2));
        assert!(!pur_history_coord_is_nzb_or_url_kind(3, 1, 2));

        assert!(pur_history_coord_should_notify_dupe_search_name_changed(1, 1, true, true));
        assert!(!pur_history_coord_should_notify_dupe_search_name_changed(2, 1, true, true));
        assert!(!pur_history_coord_should_notify_dupe_search_name_changed(1, 1, false, true));
        assert!(!pur_history_coord_should_notify_dupe_search_name_changed(1, 1, true, false));

        assert!(pur_history_coord_is_valid_dupe_backup_delete_status(3, 3, 1));
        assert!(pur_history_coord_is_valid_dupe_backup_delete_status(1, 3, 1));
        assert!(!pur_history_coord_is_valid_dupe_backup_delete_status(0, 3, 1));
    }

    #[test]
    fn test_unpack_and_direct_unpack_decisions() {
        assert!(pur_unpack_ctrl_should_execute_initial_unpack(false, false));
        assert!(pur_unpack_ctrl_should_execute_initial_unpack(true, true));
        assert!(!pur_unpack_ctrl_should_execute_initial_unpack(true, false));

        assert!(pur_unpack_ctrl_should_add_extract_cmd_flag(false, false));
        assert!(!pur_unpack_ctrl_should_add_extract_cmd_flag(true, false));
        assert!(!pur_unpack_ctrl_should_add_extract_cmd_flag(false, true));

        assert!(pur_unpack_ctrl_should_add_default_overwrite_flag(false, false, false));
        assert!(!pur_unpack_ctrl_should_add_default_overwrite_flag(true, false, false));
        assert!(!pur_unpack_ctrl_should_add_default_overwrite_flag(false, true, false));
        assert!(!pur_unpack_ctrl_should_add_default_overwrite_flag(false, false, true));

        assert!(pur_direct_unpack_should_log_unrar_exit_error(false, 2, false));
        assert!(!pur_direct_unpack_should_log_unrar_exit_error(true, 2, false));
        assert!(!pur_direct_unpack_should_log_unrar_exit_error(false, 0, false));
        assert!(!pur_direct_unpack_should_log_unrar_exit_error(false, 2, true));
    }
}
