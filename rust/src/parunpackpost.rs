//
//  This file is part of nzbget. See <https://nzbget.com>.
//
//  Copyright (C) 2026 xbmc4lyfe <273732874+xbmc4lyfe@users.noreply.github.com>
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

//! Hot-path decision and calculation helpers for `ParChecker`, `UnpackController`,
//! `HistoryCoordinator`, `Scanner`, `QueueEditor`, and `DownloadInfo`.

/// Maps `ParChecker::EFileStatus` (`fsSuccess`, `fsPartial`, other) to `Par2::MatchType`.
pub fn parchecker_select_match_type(
    file_status: i32,
    fs_success: i32,
    fs_partial: i32,
    match_full: i32,
    match_partial: i32,
    match_none: i32,
) -> i32 {
    if file_status == fs_success {
        match_full
    } else if file_status == fs_partial {
        match_partial
    } else {
        match_none
    }
}

/// Decides whether `ParChecker::Execute` should run a forced full par-check pass after a quick pass.
pub fn parchecker_should_run_forced_full_check(
    status: i32,
    ps_repair_not_needed: i32,
    par_quick: bool,
    force_repair: bool,
    is_stopped: bool,
) -> bool {
    status == ps_repair_not_needed && par_quick && force_repair && !is_stopped
}

/// Decides whether `ParChecker::RunParCheck` should scan for missing files (`AddMissingFiles`).
#[allow(clippy::too_many_arguments)]
pub fn parchecker_should_add_missing_files(
    has_damaged: bool,
    is_stopped: bool,
    missing_file_count: u32,
    added_split: bool,
    res: i32,
    e_repair_possible: i32,
    par_scan: i32,
    ps_extended: i32,
    ps_dupe: i32,
) -> bool {
    has_damaged
        && !is_stopped
        && missing_file_count > 0
        && !(added_split && res == e_repair_possible)
        && (par_scan == ps_extended || par_scan == ps_dupe)
}

/// Computes remaining missing par2 blocks in `ParChecker::ProcessMorePars` / `ProcessTwinSet`.
pub fn parchecker_calc_missing_blocks(
    missing_block_count: u32,
    recovery_packet_count: usize,
) -> i32 {
    (missing_block_count as i32).wrapping_sub(recovery_packet_count as i32)
}

/// Decides whether `ParChecker::AddExtraFiles` should abort the extra par-scan early:
/// - `1`: all missing files found (`only_missing && missing_file_count == 0`)
/// - `2`: all missing blocks found (`!only_missing && missing_block_count == 0`)
/// - `0`: continue scanning
pub fn parchecker_should_abort_extra_scan(
    only_missing: bool,
    missing_file_count: u32,
    missing_block_count: u32,
) -> i32 {
    if only_missing && missing_file_count == 0 {
        1
    } else if !only_missing && missing_block_count == 0 {
        2
    } else {
        0
    }
}

/// Decides whether `ParChecker::SignalFilename` should print a stage progress message.
pub fn parchecker_should_print_stage_msg(
    stage: i32,
    pt_verifying_repaired: i32,
    par_quick: bool,
) -> bool {
    !(stage == pt_verifying_repaired && par_quick)
}

/// Decides whether `UnpackController::UnpackArchives` should try par-check before trying the password file.
pub fn unpack_ctrl_should_try_par_before_pass_file(
    unpack_ok: bool,
    has_par_files: bool,
    password_err: bool,
    par_status: i32,
    ps_skipped: i32,
) -> bool {
    !unpack_ok && has_par_files && !password_err && par_status <= ps_skipped
}

/// Decides whether `UnpackController::UnpackArchives` should iterate passwords from `UnpackPassFile`.
#[allow(clippy::too_many_arguments)]
pub fn unpack_ctrl_should_try_password_file(
    unpack_ok: bool,
    start_err: bool,
    space_err: bool,
    decrypt_err: bool,
    password_err: bool,
    terminated: bool,
    auto_terminated: bool,
    password_empty: bool,
    pass_file_empty: bool,
) -> bool {
    !unpack_ok
        && !start_err
        && !space_err
        && (decrypt_err || password_err)
        && (!terminated || auto_terminated)
        && password_empty
        && !pass_file_empty
}

/// Evaluates `UnpackController::ExecuteSevenZip` exit status (`(unpack_ok, unpack_start_error)`).
pub fn unpack_ctrl_eval_sevenzip_exit(
    exit_code: i32,
    all_ok_msg: bool,
    terminated: bool,
    auto_terminated: bool,
) -> (bool, bool) {
    let unpack_ok = exit_code == 0 && all_ok_msg && !terminated;
    let start_err = exit_code == -1 && !auto_terminated;
    (unpack_ok, start_err)
}

/// Decides whether `UnpackController::WillRetryWithPass` is true.
pub fn unpack_ctrl_will_retry_with_pass(
    decrypt_err: bool,
    password_err: bool,
    password_empty: bool,
    pass_file_not_empty: bool,
) -> bool {
    (decrypt_err || password_err) && password_empty && pass_file_not_empty
}

/// Decides whether `UnpackController::Completed` was stopped for reload/shutdown (`_unpack` preserved).
pub fn unpack_ctrl_is_stopped_for_reload(
    auto_terminated: bool,
    post_deleted: bool,
    is_stopped: bool,
    terminated: bool,
) -> bool {
    !auto_terminated && !post_deleted && (is_stopped || terminated)
}

/// Decides whether `UnpackController::Completed` should request a par-check after unpack failure.
#[allow(clippy::too_many_arguments)]
pub fn unpack_ctrl_should_request_par_after_fail(
    unpack_ok: bool,
    par_status: i32,
    ps_skipped: i32,
    par_full: bool,
    start_err: bool,
    space_err: bool,
    password_err: bool,
    terminated: bool,
    is_stopped: bool,
    auto_terminated: bool,
    has_par_files: bool,
) -> bool {
    !unpack_ok
        && (par_status <= ps_skipped || !par_full)
        && !start_err
        && !space_err
        && !password_err
        && (!terminated || auto_terminated)
        && (!is_stopped || auto_terminated)
        && has_par_files
}

/// Computes `forceRepair` flag passed to `UnpackController::RequestParCheck`.
pub fn unpack_ctrl_calc_force_repair_on_request(
    password_empty: bool,
    pass_file_empty: bool,
    pass_list_tried: bool,
    decrypt_err: bool,
    password_err: bool,
    par_status: i32,
    ps_skipped: i32,
) -> bool {
    !password_empty
        || pass_file_empty
        || pass_list_tried
        || !(decrypt_err || password_err)
        || par_status > ps_skipped
}

/// Computes `minTime` threshold in `HistoryCoordinator::ServiceWork` (`curTime - keepHistoryDays * 86400`).
pub fn history_coord_calc_min_time(cur_time: i64, keep_history_days: i32) -> i64 {
    cur_time.wrapping_sub((keep_history_days as i64).wrapping_mul(60 * 60 * 24))
}

/// Decides whether `HistoryCoordinator::AddToHistory` counts a `CompletedFile` as parked.
pub fn history_coord_should_count_parked_file(
    status: i32,
    cf_none: i32,
    cf_partial: i32,
    is_last: bool,
) -> bool {
    status == cf_none || (status == cf_partial && is_last)
}

/// Decides whether `HistoryCoordinator::RecheckFailedArticles` passes its initial eligibility gate.
pub fn history_coord_should_recheck_failed_articles(
    daf_none: bool,
    hc_dupe: bool,
    kind_nzb: bool,
    failed_articles: i32,
    text_status: &[u8],
    has_dupe_recheck_param: bool,
) -> bool {
    !((daf_none && !hc_dupe)
        || !kind_nzb
        || failed_articles <= 0
        || !text_status.starts_with(b"FAILURE")
        || text_status == b"FAILURE/BAD"
        || has_dupe_recheck_param)
}

/// Decides whether `HistoryCoordinator::HistoryDelete` should delete the destination directory.
#[allow(clippy::too_many_arguments)]
pub fn history_coord_should_delete_dest_dir(
    is_nzb: bool,
    delete_status: i32,
    ds_none: i32,
    par_status: i32,
    ps_failure: i32,
    unpack_status: i32,
    us_failure: i32,
    us_password: i32,
    dir_exists: bool,
) -> bool {
    is_nzb
        && (delete_status != ds_none
            || par_status == ps_failure
            || unpack_status == us_failure
            || unpack_status == us_password)
        && dir_exists
}

/// Decides whether `HistoryCoordinator::HistoryDelete` erases the history item instead of hiding it.
pub fn history_coord_should_erase_on_delete(
    final_delete: bool,
    dupe_check: bool,
    is_url: bool,
) -> bool {
    final_delete || !dupe_check || is_url
}

/// Checks whether a recorded completed filename is a `.out.tmp` temporary output name.
pub fn history_coord_is_temporary_output_name(filename: &[u8]) -> bool {
    filename.len() > 8 && filename.ends_with(b".out.tmp")
}

/// Parses the `eaHistorySetDupeBackup` argument into `NzbInfo::dsDupe` or `NzbInfo::dsManual`.
pub fn history_coord_parse_dupe_backup_flag(text: &[u8], ds_dupe: i32, ds_manual: i32) -> i32 {
    if text.eq_ignore_ascii_case(b"YES") || text.eq_ignore_ascii_case(b"TRUE") || text == b"1" {
        ds_dupe
    } else {
        ds_manual
    }
}

/// Computes `Scanner::ServiceInterval`.
pub fn scanner_select_service_interval(
    requested_scan: bool,
    nzb_dir_interval_opt: i32,
    cur_interval: i32,
    now_val: i32,
    sleep_val: i32,
) -> i32 {
    if requested_scan {
        now_val
    } else if nzb_dir_interval_opt <= 0 {
        sleep_val
    } else {
        cur_interval
    }
}

/// Checks whether `Scanner::ShouldSkipItem` should skip a directory entry.
pub fn scanner_should_skip_item(
    filename: &[u8],
    processed_dir: &[u8],
    broken_dir: &[u8],
) -> bool {
    if filename.is_empty() {
        return false;
    }
    filename.starts_with(b".") || filename == processed_dir || filename == broken_dir
}

/// Checks whether an NZB name or filename is an absolute path (`/`, `\`, or `<drive>:\` / `<drive>:/`).
pub fn scanner_is_absolute_nzb_name(nzb_name: &[u8]) -> bool {
    if nzb_name.is_empty() {
        return false;
    }
    if nzb_name[0] == b'/' || nzb_name[0] == b'\\' {
        return true;
    }
    nzb_name.len() >= 3
        && nzb_name[0].is_ascii_alphabetic()
        && nzb_name[1] == b':'
        && (nzb_name[2] == b'\\' || nzb_name[2] == b'/')
}

/// Checks whether `QueueEditor::EditList` should reject a post/history action that lacks an ID list.
pub fn queue_editor_is_history_or_post_without_ids(
    action: i32,
    ea_post_delete: i32,
    ea_history_delete: i32,
    ea_history_set_name: i32,
    has_id_list: bool,
) -> bool {
    (action == ea_post_delete || (ea_history_delete <= action && action <= ea_history_set_name))
        && !has_id_list
}

/// Strips a trailing `.nzb` extension (case-insensitive) when `remove_ext` is true.
pub fn nzb_info_strip_nzb_ext<'a>(nicename: &'a [u8], remove_ext: bool) -> &'a [u8] {
    if remove_ext && nicename.len() >= 4 {
        let split_at = nicename.len() - 4;
        if nicename[split_at..].eq_ignore_ascii_case(b".nzb") {
            return &nicename[..split_at];
        }
    }
    nicename
}

/// Updates download timing state in `NzbInfo::SetActiveDownloads`.
/// Returns `(download_sec, download_start_time, download_start_sec, changed)`.
pub fn nzb_info_calc_active_downloads_sec(
    old_active: i32,
    new_active: i32,
    is_nzb_kind: bool,
    cur_time: i64,
    download_sec: i32,
    download_start_time: i64,
    download_start_sec: i32,
) -> (i32, i64, i32, bool) {
    if ((old_active == 0 && new_active > 0) || (old_active > 0 && new_active == 0)) && is_nzb_kind {
        if new_active > 0 {
            (download_sec, cur_time, download_sec, false)
        } else {
            let elapsed = cur_time.wrapping_sub(download_start_time) as i32;
            (download_start_sec.wrapping_add(elapsed), 0, download_start_sec, true)
        }
    } else if new_active > 0 {
        let elapsed = cur_time.wrapping_sub(download_start_time) as i32;
        (
            download_start_sec.wrapping_add(elapsed),
            download_start_time,
            download_start_sec,
            true,
        )
    } else {
        (download_sec, download_start_time, download_start_sec, false)
    }
}

/// Decides whether `NzbInfo::IsDownloadCompleted` treats a `FileInfo` as still remaining.
pub fn file_info_blocks_download_completion(
    paused: bool,
    ignore_paused_pars: bool,
    par_file: bool,
    deleted: bool,
) -> bool {
    (!paused || !ignore_paused_pars || !par_file) && !deleted
}

/// Evaluates whether `HistoryCoordinator::HistoryRetry` should requeue a `CompletedFile` prior to disk load.
pub fn history_coord_can_attempt_requeue_file(
    status: i32,
    cf_success: i32,
    cf_failure: i32,
    reset_failed: bool,
    file_id: i32,
) -> bool {
    status != cf_success && (status != cf_failure || reset_failed) && file_id > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parchecker_helpers() {
        assert_eq!(parchecker_select_match_type(1, 1, 2, 10, 20, 30), 10);
        assert_eq!(parchecker_select_match_type(2, 1, 2, 10, 20, 30), 20);
        assert_eq!(parchecker_select_match_type(0, 1, 2, 10, 20, 30), 30);

        assert!(parchecker_should_run_forced_full_check(2, 2, true, true, false));
        assert!(!parchecker_should_run_forced_full_check(2, 2, false, true, false));
        assert!(!parchecker_should_run_forced_full_check(2, 2, true, true, true));

        assert!(parchecker_should_add_missing_files(
            true, false, 2, false, 1, 0, 2, 2, 3
        ));
        assert!(!parchecker_should_add_missing_files(
            true, false, 2, true, 0, 0, 2, 2, 3
        ));

        assert_eq!(parchecker_calc_missing_blocks(10, 4), 6);
        assert_eq!(parchecker_calc_missing_blocks(4, 10), -6);

        assert_eq!(parchecker_should_abort_extra_scan(true, 0, 5), 1);
        assert_eq!(parchecker_should_abort_extra_scan(false, 2, 0), 2);
        assert_eq!(parchecker_should_abort_extra_scan(false, 0, 3), 0);

        assert!(!parchecker_should_print_stage_msg(3, 3, true));
        assert!(parchecker_should_print_stage_msg(3, 3, false));
        assert!(parchecker_should_print_stage_msg(1, 3, true));
    }

    #[test]
    fn test_unpack_ctrl_helpers() {
        assert!(unpack_ctrl_should_try_par_before_pass_file(
            false, true, false, 1, 1
        ));
        assert!(!unpack_ctrl_should_try_par_before_pass_file(
            false, true, true, 1, 1
        ));

        assert!(unpack_ctrl_should_try_password_file(
            false, false, false, true, false, false, false, true, false
        ));
        assert!(!unpack_ctrl_should_try_password_file(
            false, false, true, true, false, false, false, true, false
        ));

        assert_eq!(
            unpack_ctrl_eval_sevenzip_exit(0, true, false, false),
            (true, false)
        );
        assert_eq!(
            unpack_ctrl_eval_sevenzip_exit(-1, false, false, false),
            (false, true)
        );

        assert!(unpack_ctrl_will_retry_with_pass(true, false, true, true));
        assert!(!unpack_ctrl_will_retry_with_pass(false, false, true, true));

        assert!(unpack_ctrl_is_stopped_for_reload(false, false, true, false));
        assert!(!unpack_ctrl_is_stopped_for_reload(true, false, true, false));

        assert!(unpack_ctrl_should_request_par_after_fail(
            false, 1, 1, true, false, false, false, false, false, false, true
        ));
        assert!(unpack_ctrl_calc_force_repair_on_request(
            false, false, false, false, false, 1, 1
        ));
        assert!(!unpack_ctrl_calc_force_repair_on_request(
             true, false, false, true, false, 1, 1
        ));
    }

    #[test]
    fn test_history_scanner_queue_download_helpers() {
        assert_eq!(history_coord_calc_min_time(100_000, 1), 13_600);
        assert!(history_coord_should_count_parked_file(0, 0, 2, false));
        assert!(history_coord_should_count_parked_file(2, 0, 2, true));
        assert!(!history_coord_should_count_parked_file(2, 0, 2, false));

        assert!(history_coord_should_recheck_failed_articles(
            false,
            false,
            true,
            3,
            b"FAILURE/HEALTH",
            false
        ));
        assert!(!history_coord_should_recheck_failed_articles(
            false,
            false,
            true,
            3,
            b"FAILURE/BAD",
            false
        ));

        assert!(history_coord_should_delete_dest_dir(
            true, 0, 0, 1, 1, 0, 2, 3, true
        ));
        assert!(history_coord_should_erase_on_delete(false, false, false));
        assert!(!history_coord_should_erase_on_delete(false, true, false));

        assert!(history_coord_is_temporary_output_name(b"12.out.tmp"));
        assert!(history_coord_is_temporary_output_name(b"12\xe9.out.tmp"));
        assert!(!history_coord_is_temporary_output_name(b".out.tmp"));
        assert_eq!(history_coord_parse_dupe_backup_flag(b"yes", 4, 1), 4);
        assert_eq!(history_coord_parse_dupe_backup_flag(b"no", 4, 1), 1);

        assert_eq!(scanner_select_service_interval(true, 10, 10, 0, -1), 0);
        assert_eq!(scanner_select_service_interval(false, 0, 10, 0, -1), -1);
        assert_eq!(scanner_select_service_interval(false, 10, 5, 0, -1), 5);

        assert!(scanner_should_skip_item(b".hidden", b"extracted", b"broken"));
        assert!(scanner_should_skip_item(b".caf\xe9", b"extracted", b"broken"));
        assert!(scanner_should_skip_item(b"extracted", b"extracted", b"broken"));
        assert!(!scanner_should_skip_item(b"movie.nzb", b"extracted", b"broken"));
        assert!(!scanner_should_skip_item(b"caf\xe9.nzb", b"extracted", b"broken"));

        assert!(scanner_is_absolute_nzb_name(b"/tmp/test.nzb"));
        assert!(scanner_is_absolute_nzb_name(b"/tmp/caf\xe9.nzb"));
        assert!(scanner_is_absolute_nzb_name(b"C:\\temp\\test.nzb"));
        assert!(!scanner_is_absolute_nzb_name(b"[16/22] - Title.nzb"));
        assert!(!scanner_is_absolute_nzb_name(b"caf\xe9.nzb"));

        assert!(queue_editor_is_history_or_post_without_ids(50, 40, 45, 60, false));
        assert!(!queue_editor_is_history_or_post_without_ids(50, 40, 45, 60, true));

        assert_eq!(nzb_info_strip_nzb_ext(b"Movie.NZB", true), b"Movie");
        assert_eq!(nzb_info_strip_nzb_ext(b"caf\xe9.nzb", true), b"caf\xe9");
        assert_eq!(nzb_info_strip_nzb_ext(b"caf\xe9", true), b"caf\xe9");
        assert_eq!(nzb_info_strip_nzb_ext(b"Movie.NZB", false), b"Movie.NZB");

        assert_eq!(
            nzb_info_calc_active_downloads_sec(0, 2, true, 1000, 15, 0, 0),
            (15, 1000, 15, false)
        );
        assert_eq!(
            nzb_info_calc_active_downloads_sec(2, 0, true, 1020, 15, 1000, 15),
            (35, 0, 15, true)
        );

        assert!(file_info_blocks_download_completion(false, true, true, false));
        assert!(!file_info_blocks_download_completion(true, true, true, false));

        assert!(history_coord_can_attempt_requeue_file(2, 1, 3, false, 10));
        assert!(!history_coord_can_attempt_requeue_file(3, 1, 3, false, 10));
    }
}
