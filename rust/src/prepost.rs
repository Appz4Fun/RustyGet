//! End-to-end Rust implementation of `PrePostProcessor` (`daemon/postprocess/PrePostProcessor.cpp`).
//!
//! Consolidates post-queue sanitisation, queue/file event routing, NZB completion & deletion
//! workflows, PAR/stream-repair coverage decisions, job scheduling strategies, stage
//! transitions, pause-state management, and direct-unpack triggers into a single module.

pub use crate::diskstatepp::{RsPostJobCandidate, RsPostStageDecision};

/// `PrePostProcessor::SanitisePostQueue` stage & `RequestParCheck` transition.
pub fn sanitise_post_stage(
    stage: i32,
    pt_executing_script: i32,
    pt_finished: i32,
    pt_queued: i32,
    dest_dir_exists: bool,
    unpack_tried: bool,
    par_status: i32,
    ps_skipped: i32,
) -> (i32, bool) {
    crate::diskstatepp::sanitise_post_stage(
        stage,
        pt_executing_script,
        pt_finished,
        pt_queued,
        dest_dir_exists,
        unpack_tried,
        par_status,
        ps_skipped,
    )
}

/// `PrePostProcessor::DownloadQueueUpdate` `eaFileCompleted` / `eaFileDeleted` classifier.
/// Returns:
/// - `1`: collection completely downloaded (`qeNzbDownloaded` + `NzbDownloaded`)
/// - `2`: collection deleted from queue (`NzbDeleted`)
/// - `0`: no collection transition
#[allow(clippy::too_many_arguments)]
pub fn classify_file_event(
    is_file_completed: bool,
    is_file_deleted: bool,
    dupe_deleted: bool,
    delete_status: i32,
    ds_none: i32,
    ds_health: i32,
    has_post_info: bool,
    download_completed_ignore_paused: bool,
    download_completed_all: bool,
) -> i32 {
    crate::diskstatepp::classify_file_event(
        is_file_completed,
        is_file_deleted,
        dupe_deleted,
        delete_status,
        ds_none,
        ds_health,
        has_post_info,
        download_completed_ignore_paused,
        download_completed_all,
    )
}

/// `PrePostProcessor::CheckDownloadCompleted`: whether completion should trigger after `DirectRename`.
#[allow(clippy::too_many_arguments)]
pub fn should_complete_after_direct_rename(
    is_stopped: bool,
    direct_rename_status: i32,
    ts_success: i32,
    has_post_info: bool,
    delete_status: i32,
    ds_none: i32,
    completed_not_empty: bool,
    download_completed: bool,
    raw_article: bool,
    skip_write: bool,
    skip_disk_write: bool,
) -> bool {
    !is_stopped
        && direct_rename_status == ts_success
        && !has_post_info
        && delete_status == ds_none
        && completed_not_empty
        && download_completed
        && !raw_article
        && !skip_write
        && !skip_disk_write
}

/// Classify entry routing in `PrePostProcessor::NzbDownloaded`:
/// Returns `(route, enqueue_deleted_script, set_par_skipped)` where `route`:
/// - `1`: skip-script + skip-write immediate cleanup and `NzbCompleted`
/// - `2`: queue for post-processing
/// - `3`: direct `NzbCompleted`
#[allow(clippy::too_many_arguments)]
pub fn classify_nzb_downloaded_entry(
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
    crate::histscanrepair::prepost_classify_nzb_downloaded_entry(
        skip_script_processing,
        skip_disk_write,
        opt_skip_write,
        delete_status,
        ds_health,
        ds_bad,
        has_post_info,
        opt_raw_article,
        par_status,
        ps_none,
        par_check_opt,
        pc_always,
        pc_force,
    )
}

/// Resolve delete status and route in `PrePostProcessor::NzbDeleted`:
/// Returns `(new_delete_status, call_nzb_downloaded)`.
pub fn resolve_nzb_deleted_route(
    delete_status: i32,
    ds_none: i32,
    ds_manual: i32,
    ds_health: i32,
    ds_bad: i32,
) -> (i32, bool) {
    crate::histscanrepair::prepost_resolve_nzb_deleted_route(
        delete_status,
        ds_none,
        ds_manual,
        ds_health,
        ds_bad,
    )
}

/// `PrePostProcessor::NzbCompleted` action flags evaluator.
/// Returns `(add_to_history, call_dupe_completed, enqueue_deleted_script, need_save)`.
#[allow(clippy::too_many_arguments)]
pub fn eval_nzb_completed(
    dupe_hint: i32,
    dh_redownload_auto: i32,
    keep_history: i32,
    avoid_history: bool,
    dupe_check: bool,
    dupe_mode: i32,
    dm_force: i32,
    fails_over: bool,
    delete_status: i32,
    ds_none: i32,
    ds_health: i32,
    ds_bad: i32,
    ds_scan: i32,
    ds_copy: i32,
) -> (bool, bool, bool, bool) {
    crate::diskstatepp::eval_nzb_completed(
        dupe_hint,
        dh_redownload_auto,
        keep_history,
        avoid_history,
        dupe_check,
        dupe_mode,
        dm_force,
        fails_over,
        delete_status,
        ds_none,
        ds_health,
        ds_bad,
        ds_scan,
        ds_copy,
    )
}

/// Classify disk cleanup mode in `PrePostProcessor::DeleteCleanup`:
/// - `0`: no cleanup
/// - `1`: skip-write mode (`DeleteDirectoryWithContent` on `destDir`)
/// - `2`: standard cleanup (delete completed files, `.out.tmp` files, and empty dirs)
pub fn classify_delete_cleanup_mode(
    cleanup_disk: bool,
    delete_status: i32,
    ds_dupe: i32,
    parking: bool,
    skip_disk_write: bool,
    opt_skip_write: bool,
) -> i32 {
    crate::histscanrepair::prepost_classify_delete_cleanup_mode(
        cleanup_disk,
        delete_status,
        ds_dupe,
        parking,
        skip_disk_write,
        opt_skip_write,
    )
}

/// Classify action for `PrePostProcessor::CheckRequestPar`:
/// Returns `(action, new_force_par_full)` where `action`:
/// - `0`: no action
/// - `1`: restart job (`psNone`, clear scripts, `ptQueued`, `working = false`)
/// - `2`: manual par-check with remaining files (`psManual`, `eaGroupResume`, `ptFinished`, `working = false`)
/// - `3`: manual par-check with no remaining files (`psManual`, `working = false`)
#[allow(clippy::too_many_arguments)]
pub fn classify_request_par_action(
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
    crate::histscanrepair::prepost_classify_request_par_action(
        request_par_check,
        par_status,
        ps_skipped,
        force_repair,
        par_full,
        par_check_opt,
        pc_manual,
        cur_force_par_full,
        file_list_empty,
    )
}

/// `PrePostProcessor::CleanupJobs`:
/// Evaluates whether an active post-job is ready for cleanup, whether its `PostTotalSec`
/// should be updated, whether `JobCompleted` should be invoked vs resetting stage to `ptQueued`,
/// and the updated `PostTotalSec`.
#[allow(clippy::too_many_arguments)]
pub fn eval_cleanup_job_state(
    working: bool,
    has_running_thread: bool,
    stage: i32,
    pt_finished: i32,
    deleted: bool,
    start_time: i64,
    cur_time: i64,
    prev_post_total_sec: i32,
) -> (bool, bool, bool, i32) {
    crate::poolstatprepost::prepost_eval_cleanup_job_state(
        working,
        has_running_thread,
        stage,
        pt_finished,
        deleted,
        start_time,
        cur_time,
        prev_post_total_sec,
    )
}

/// `PrePostProcessor::CanRunMoreJobs` scheduler evaluator.
/// `post_strategy`: `0` = `ppSequential`, `1` = `ppBalanced`, `2` = `ppAggressive`, `3` = `ppRocket`.
/// Returns `(can_run, allow_par)`.
pub fn can_run_more_jobs(
    job_stages: &[i32],
    pt_loading_pars: i32,
    pt_verifying_repaired: i32,
    pt_repairing: i32,
    post_strategy: i32,
) -> (bool, bool) {
    crate::diskstatepp::can_run_more_jobs(
        job_stages,
        pt_loading_pars,
        pt_verifying_repaired,
        pt_repairing,
        post_strategy,
    )
}

/// `PrePostProcessor::PickNextJob` candidate selector.
/// Returns the selected index or `-1`.
pub fn pick_next_post_job(
    candidates: &[RsPostJobCandidate],
    allow_par: bool,
    has_active_unpack: bool,
    pause_post_process: bool,
) -> i32 {
    crate::diskstatepp::pick_next_post_job(
        candidates,
        allow_par,
        has_active_unpack,
        pause_post_process,
    )
}

/// Classify skipped par-check health action in `PrePostProcessor::StartJob`:
/// - `0`: no action
/// - `1`: skip par-check due to health 0%
/// - `2`: skip par-check due to health below critical
/// - `3`: request par-check due to failed non-par bytes
#[allow(clippy::too_many_arguments)]
pub fn classify_skipped_par_health(
    par_status: i32,
    ps_skipped: i32,
    par_scan_opt: i32,
    ps_dupe: i32,
    health: i32,
    critical_health: i32,
    failed_size: i64,
    par_failed_size: i64,
) -> i32 {
    crate::histscanrepair::prepost_classify_skipped_par_health(
        par_status,
        ps_skipped,
        par_scan_opt,
        ps_dupe,
        health,
        critical_health,
        failed_size,
        par_failed_size,
    )
}

/// Evaluate `wantUnpack`, `unpack`, and whether to start `RenameController::jkRar` in `PrePostProcessor::StartJob`.
#[allow(clippy::too_many_arguments)]
pub fn eval_want_unpack_and_rar_rename(
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
    crate::histscanrepair::prepost_eval_want_unpack_and_rar_rename(
        has_unpack_param,
        unpack_param_val,
        unpack_status,
        us_none,
        delete_status,
        ds_none,
        rar_rename_status,
        rs_none,
        opt_rar_rename,
    )
}

/// `PrePostProcessor::StartJob` stage eligibility calculator.
#[allow(clippy::too_many_arguments)]
pub fn eval_post_stages(
    want_unpack: bool,
    unpack_status: i32,
    delete_status: i32,
    par_status: i32,
    cleanup_status: i32,
    has_ext_cleanup_disk: bool,
    health: i32,
    move_status: i32,
    dest_dir: &[u8],
    inter_dir: &[u8],
    sep: u8,
    direct_unpack_status: i32,
    opt_post_download_rename: bool,
    post_download_renaming_status: i32,
    has_dest_dir: bool,
    has_name: bool,
    opt_rename_after_unpack: bool,
    post_unpack_renaming_status: i32,
) -> RsPostStageDecision {
    let in_inter_dir =
        !inter_dir.is_empty() && dest_dir.len() > inter_dir.len() && dest_dir.starts_with(inter_dir) && dest_dir[inter_dir.len()] == sep;
    crate::diskstatepp::eval_post_stages(
        want_unpack,
        unpack_status,
        delete_status,
        par_status,
        cleanup_status,
        has_ext_cleanup_disk,
        health,
        move_status,
        in_inter_dir,
        direct_unpack_status,
        opt_post_download_rename,
        post_download_renaming_status,
        has_dest_dir,
        has_name,
        opt_rename_after_unpack,
        post_unpack_renaming_status,
    )
}

/// `PrePostProcessor::UpdatePauseState` pause-download evaluator.
pub fn calc_post_pause_state(
    job_stages: &[i32],
    par_pause_queue: bool,
    unpack_pause_queue: bool,
    script_pause_queue: bool,
) -> bool {
    crate::diskstatepp::calc_post_pause_state(
        job_stages,
        par_pause_queue,
        unpack_pause_queue,
        script_pause_queue,
    )
}

/// Classify direct-unpack action in `PrePostProcessor::FileDownloaded`:
/// - `0`: do nothing
/// - `1`: candidate for starting `DirectUnpack::StartJob`
/// - `2`: notify running `DirectUnpack::FileDownloaded`
#[allow(clippy::too_many_arguments)]
pub fn classify_file_downloaded_direct_unpack(
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
    crate::histscanrepair::prepost_classify_file_downloaded_direct_unpack(
        opt_direct_unpack,
        opt_raw_article,
        opt_skip_write,
        skip_disk_write,
        direct_unpack_status,
        ns_none,
        ns_running,
        direct_rename_status,
        ts_running,
        has_unpack_thread,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepost_stage_and_pause() {
        assert_eq!(
            sanitise_post_stage(11, 11, 12, 0, true, false, 0, 1),
            (12, false)
        );
        assert_eq!(
            sanitise_post_stage(2, 11, 12, 0, true, true, 1, 1),
            (0, true)
        );
        assert!(calc_post_pause_state(&[2], true, false, false));
        assert!(!calc_post_pause_state(&[2], false, true, true));
    }

    #[test]
    fn test_eval_post_stages_inter_dir() {
        let dec = eval_post_stages(
            false,
            0,
            0,
            3,
            0,
            false,
            1000,
            0,
            b"/inter/job1",
            b"/inter",
            b'/',
            0,
            false,
            0,
            true,
            true,
            false,
            0,
        );
        assert_eq!(dec.move_inter, 1);
    }
}
