//! DirectRenamer, RarReader, Repair, ArticleDownloader, WebDownloader & NzbFile
//! Hot-Path Kernels (`renamerarrepdl.rs`).
//!
//! Preserves exact byte-level, case-sensitivity, integer-arithmetic, and control-flow
//! semantics of the C++ call sites in:
//! - `daemon/queue/DirectRenamer.cpp`
//! - `daemon/postprocess/RarReader.cpp`
//! - `daemon/postprocess/Repair.cpp`
//! - `daemon/nntp/ArticleDownloader.cpp`
//! - `daemon/connect/WebDownloader.cpp`
//! - `daemon/queue/NzbFile.cpp`

use std::ffi::{c_char, c_int, CStr};

extern "C" {
    fn strcasecmp(a: *const c_char, b: *const c_char) -> c_int;
}

/// `DirectParLoader::StartLoader` / `DirectParLoader::Run`:
/// `if (nzbInfo && !nzbInfo->GetSkipDiskWrite() && !g_Options->GetSkipWrite())`
pub fn direct_renamer_can_write_par_loader(
    has_nzb: bool,
    skip_disk_write: bool,
    opt_skip_write: bool,
) -> bool {
    has_nzb && !skip_disk_write && !opt_skip_write
}

/// `DirectRenamer::CheckState`:
/// `if (Util::EmptyStr(hash16k) || (is_par_file && Util::EmptyStr(par_set_id)))`
pub fn direct_renamer_is_file_missing_state_hash(
    hash16k: &[u8],
    is_par_file: bool,
    par_set_id: &[u8],
) -> bool {
    hash16k.is_empty() || (is_par_file && par_set_id.is_empty())
}

/// `DirectRenamer::CheckState`:
/// `if (waiting_par && !loading_par)`
pub fn direct_renamer_should_check_extra_priority(
    waiting_par: bool,
    loading_par: bool,
) -> bool {
    waiting_par && !loading_par
}

/// `DirectRenamer::RenameFilesInProgress` / `DirectRenamer::RenameCompletedFiles`:
/// `if (newName.empty() && !isParFile && hardLinking)` -> returns 1 (force rename to current filename),
/// `else if (newName.empty())` -> returns 2 (skip),
/// else returns 0 (use `newName`).
pub fn direct_renamer_resolve_empty_new_name_action(
    new_name_empty: bool,
    is_par_file: bool,
    hard_linking: bool,
) -> i32 {
    if new_name_empty && !is_par_file && hard_linking {
        1
    } else if new_name_empty {
        2
    } else {
        0
    }
}

/// `RarSourceCursor::Read`:
/// Calculates `available` bytes, `eof` flag (`available < size`), and updated position
/// `(int64)((uint64)position + (uint64)available)`.
pub fn rar_cursor_calc_read_window(
    source_size: i64,
    cur_pos: i64,
    req_size: i64,
) -> (i64, bool, i64) {
    let rem = (source_size as u64).wrapping_sub(cur_pos as u64) as i64;
    let clamped_rem = if rem > 0 { rem } else { 0 };
    let available = if req_size < clamped_rem {
        req_size
    } else {
        clamped_rem
    };
    let eof = available < req_size;
    let new_pos = if available > 0 {
        (cur_pos as u64).wrapping_add(available as u64) as i64
    } else {
        cur_pos
    };
    (available, eof, new_pos)
}

/// `RarSourceCursor::Seek`:
/// Calculates target position with wrapping unsigned addition for `soCur` / `soEnd`
/// and rejects `target < 0`.
pub fn rar_cursor_calc_seek_target(
    cur_pos: i64,
    source_size: i64,
    offset: i64,
    origin: i32,
    so_cur: i32,
    so_end: i32,
) -> (bool, i64) {
    let target = if origin == so_cur {
        (cur_pos as u64).wrapping_add(offset as u64) as i64
    } else if origin == so_end {
        (source_size as u64).wrapping_add(offset as u64) as i64
    } else {
        offset
    };
    (target >= 0, target)
}

/// `RepairController::PostParChecker::IsParredFile` / `FindFileCrc` / `FindFileOrigname`:
/// Case-insensitive comparison in the active C locale (`!strcasecmp(a, b)`).
pub fn repair_ctrl_filename_eq_ignore_case(a: &CStr, b: &CStr) -> bool {
    unsafe { strcasecmp(a.as_ptr(), b.as_ptr()) == 0 }
}

/// `RepairController::PostParChecker::FindFileCrc`:
/// `if (status == CompletedFile::cfPartial && id > 0 && !reprocess)`
pub fn repair_ctrl_should_load_partial_segments(
    status: i32,
    cf_partial: i32,
    id: i32,
    reprocess: bool,
) -> bool {
    status == cf_partial && id > 0 && !reprocess
}

/// `RepairController::PostParChecker::RequestDupeSources`:
/// `bool useDupe = !sizeComparisonPossible || dupeMatcher.MatchDupeContent(...);`
pub fn repair_ctrl_can_accept_dupe_without_content_match(size_comparison_possible: bool) -> bool {
    !size_comparison_possible
}

/// `RepairController::AddPar`:
/// `if (sameCollection && !deleted)`
pub fn repair_ctrl_should_add_par_file(same_collection: bool, deleted: bool) -> bool {
    same_collection && !deleted
}

/// `RepairController::RequestMorePars`:
/// `bool ok = blockNeeded <= 0 || hasUnpausedParFiles;`
pub fn repair_ctrl_is_request_more_pars_ok(
    block_needed: i32,
    has_unpaused_par_files: bool,
) -> bool {
    block_needed <= 0 || has_unpaused_par_files
}

/// `RepairController::CheckPauseState`:
/// `if (g_WorkState->GetPausePostProcess() && !postInfo->GetNzbInfo()->GetForcePriority() && !IsStopped())`
pub fn repair_ctrl_is_post_process_paused(
    pause_post_process: bool,
    force_priority: bool,
    is_stopped: bool,
) -> bool {
    pause_post_process && !force_priority && !is_stopped
}

/// `RepairController::CheckPauseState`:
/// Computes shifted timestamps `(stageTime + delta, startTime + delta, parTime + delta, repairTime + delta)`
/// for positive timestamps.
pub fn repair_ctrl_calc_paused_timestamps(
    cur_time: i64,
    wait_time: i64,
    stage_time: i64,
    start_time: i64,
    par_time: i64,
    repair_time: i64,
) -> (i64, i64, i64, i64) {
    let delta = cur_time.wrapping_sub(wait_time);
    (
        if stage_time > 0 {
            stage_time.wrapping_add(delta)
        } else {
            0
        },
        if start_time > 0 {
            start_time.wrapping_add(delta)
        } else {
            0
        },
        if par_time > 0 {
            par_time.wrapping_add(delta)
        } else {
            0
        },
        if repair_time > 0 {
            repair_time.wrapping_add(delta)
        } else {
            0
        },
    )
}

/// `ArticleDownloader::Run`:
/// `int retries = g_Options->GetArticleRetries() > 0 ? g_Options->GetArticleRetries() : 1;`
pub fn article_dl_resolve_initial_retries(opt_article_retries: i32) -> i32 {
    if opt_article_retries > 0 {
        opt_article_retries
    } else {
        1
    }
}

/// `ArticleDownloader::Run`:
/// Evaluates `adConnectError` normalization and `remainedRetries` decrement:
/// `if (status == adConnectError) { connected = false; status = adFailed; }`
/// `if (connected && status == adFailed) { remainedRetries--; }`
pub fn article_dl_normalize_connect_and_decrement_retries(
    connected: bool,
    status: i32,
    ad_connect_error: i32,
    ad_failed: i32,
    remained_retries: i32,
) -> (bool, i32, i32) {
    let (new_connected, new_status) = if status == ad_connect_error {
        (false, ad_failed)
    } else {
        (connected, status)
    };
    let new_retries = if new_connected && new_status == ad_failed {
        remained_retries.wrapping_sub(1)
    } else {
        remained_retries
    };
    (new_connected, new_status, new_retries)
}

/// `ArticleDownloader::Download`:
/// `if (response && !strncmp(response, "2", 1))`
pub fn article_dl_is_join_group_ok(response: Option<&[u8]>) -> bool {
    matches!(response, Some(r) if r.first() == Some(&b'2'))
}

/// `ArticleDownloader::FreeConnection`:
/// `if (!keepConnected || m_connection->GetStatus() == Connection::csCancelled)`
pub fn article_dl_should_disconnect_on_free(
    keep_connected: bool,
    conn_status: i32,
    cs_cancelled: i32,
) -> bool {
    !keep_connected || conn_status == cs_cancelled
}

/// `WebDownloader::Run`:
/// `if (IsStopped() || (!m_force && g_WorkState->GetPauseDownload()))`
pub fn webdl_should_abort_retry_loop(
    is_stopped: bool,
    force: bool,
    pause_download: bool,
) -> bool {
    is_stopped || (!force && pause_download)
}

/// `WebDownloader::Run`:
/// Updates `(remainedDownloadRetries, remainedConnectRetries)` after an unsuccessful attempt.
pub fn webdl_step_retry_counters(
    status: i32,
    ad_connect_error: i32,
    rem_dl: i32,
    rem_conn: i32,
) -> (i32, i32) {
    if status != ad_connect_error {
        (rem_dl.wrapping_sub(1), rem_conn)
    } else {
        (rem_dl, rem_conn.wrapping_sub(1))
    }
}

/// `WebDownloader::Run`:
/// `if (Status != adFinished && Status != adRetry) { Status = adFailed; }`
pub fn webdl_resolve_final_loop_status(
    status: i32,
    ad_finished: i32,
    ad_retry: i32,
    ad_failed: i32,
) -> i32 {
    if status != ad_finished && status != ad_retry {
        ad_failed
    } else {
        status
    }
}

/// `WebDownloader::DownloadBody`:
/// Resolves final body status from `(end, status, is_stopped)`:
/// returns `(new_status, warn_incomplete)`.
pub fn webdl_resolve_body_end_status(
    end: bool,
    status: i32,
    is_stopped: bool,
    ad_running: i32,
    ad_failed: i32,
    ad_finished: i32,
) -> (i32, bool) {
    let mut cur = status;
    let mut warn_incomplete = false;
    if !end && cur == ad_running && !is_stopped {
        warn_incomplete = true;
        cur = ad_failed;
    }
    if end {
        cur = ad_finished;
    }
    (cur, warn_incomplete)
}

/// `NzbFile::Parse_StartElement`:
/// Classifies `<meta type="...">` attribute value (`password`=1, `category`=2, `name`=3, `title`=4, else 0)
/// using `strcasecmp` in the active C locale.
pub fn nzbfile_classify_meta_type(attr_name: &CStr, attr_val: &CStr) -> i32 {
    unsafe {
        if strcasecmp(attr_name.as_ptr(), c"type".as_ptr()) != 0 {
            return 0;
        }
        if strcasecmp(attr_val.as_ptr(), c"password".as_ptr()) == 0 {
            1
        } else if strcasecmp(attr_val.as_ptr(), c"category".as_ptr()) == 0 {
            2
        } else if strcasecmp(attr_val.as_ptr(), c"name".as_ptr()) == 0 {
            3
        } else if strcasecmp(attr_val.as_ptr(), c"title".as_ptr()) == 0 {
            4
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_renamer_and_rar_cursor() {
        assert!(direct_renamer_can_write_par_loader(true, false, false));
        assert!(!direct_renamer_can_write_par_loader(true, true, false));
        assert!(!direct_renamer_can_write_par_loader(false, false, false));

        assert!(direct_renamer_is_file_missing_state_hash(b"", false, b""));
        assert!(direct_renamer_is_file_missing_state_hash(
            b"abc", true, b""
        ));
        assert!(!direct_renamer_is_file_missing_state_hash(
            b"abc", true, b"set1"
        ));

        assert!(direct_renamer_should_check_extra_priority(true, false));
        assert!(!direct_renamer_should_check_extra_priority(true, true));

        assert_eq!(
            direct_renamer_resolve_empty_new_name_action(true, false, true),
            1
        );
        assert_eq!(
            direct_renamer_resolve_empty_new_name_action(true, true, true),
            2
        );
        assert_eq!(
            direct_renamer_resolve_empty_new_name_action(false, false, true),
            0
        );

        assert_eq!(rar_cursor_calc_read_window(100, 90, 20), (10, true, 100));
        assert_eq!(rar_cursor_calc_read_window(100, 50, 20), (20, false, 70));

        assert_eq!(
            rar_cursor_calc_seek_target(50, 100, 10, 1, 1, 2),
            (true, 60)
        );
        assert_eq!(
            rar_cursor_calc_seek_target(50, 100, -10, 2, 1, 2),
            (true, 90)
        );
        assert_eq!(
            rar_cursor_calc_seek_target(50, 100, -5, 0, 1, 2),
            (false, -5)
        );
    }

    #[test]
    fn test_repair_and_article_dl_helpers() {
        assert!(repair_ctrl_filename_eq_ignore_case(c"File.RAR", c"file.rar"));
        assert!(!repair_ctrl_filename_eq_ignore_case(
            c"File.RAR",
            c"other.rar"
        ));

        assert!(repair_ctrl_should_load_partial_segments(2, 2, 5, false));
        assert!(!repair_ctrl_should_load_partial_segments(2, 2, 5, true));

        assert!(repair_ctrl_can_accept_dupe_without_content_match(false));
        assert!(!repair_ctrl_can_accept_dupe_without_content_match(true));

        assert!(repair_ctrl_should_add_par_file(true, false));
        assert!(!repair_ctrl_should_add_par_file(true, true));

        assert!(repair_ctrl_is_request_more_pars_ok(0, false));
        assert!(repair_ctrl_is_request_more_pars_ok(5, true));
        assert!(!repair_ctrl_is_request_more_pars_ok(5, false));

        assert!(repair_ctrl_is_post_process_paused(true, false, false));
        assert!(!repair_ctrl_is_post_process_paused(true, true, false));

        assert_eq!(
            repair_ctrl_calc_paused_timestamps(120, 100, 10, 20, 0, 30),
            (30, 40, 0, 50)
        );

        assert_eq!(article_dl_resolve_initial_retries(3), 3);
        assert_eq!(article_dl_resolve_initial_retries(0), 1);

        assert_eq!(
            article_dl_normalize_connect_and_decrement_retries(true, 4, 4, 2, 3),
            (false, 2, 3)
        );
        assert_eq!(
            article_dl_normalize_connect_and_decrement_retries(true, 2, 4, 2, 3),
            (true, 2, 2)
        );

        assert!(article_dl_is_join_group_ok(Some(b"211 1234 1 1234 group")));
        assert!(!article_dl_is_join_group_ok(Some(b"411 no such group")));
        assert!(!article_dl_is_join_group_ok(None));

        assert!(article_dl_should_disconnect_on_free(false, 0, 2));
        assert!(article_dl_should_disconnect_on_free(true, 2, 2));
        assert!(!article_dl_should_disconnect_on_free(true, 0, 2));
    }

    #[test]
    fn test_webdl_and_nzbfile_helpers() {
        assert!(webdl_should_abort_retry_loop(false, false, true));
        assert!(!webdl_should_abort_retry_loop(false, true, true));

        assert_eq!(webdl_step_retry_counters(2, 4, 3, 10), (2, 10));
        assert_eq!(webdl_step_retry_counters(4, 4, 3, 10), (3, 9));

        assert_eq!(webdl_resolve_final_loop_status(5, 1, 3, 2), 2);
        assert_eq!(webdl_resolve_final_loop_status(1, 1, 3, 2), 1);

        assert_eq!(
            webdl_resolve_body_end_status(false, 0, false, 0, 2, 1),
            (2, true)
        );
        assert_eq!(
            webdl_resolve_body_end_status(true, 0, false, 0, 2, 1),
            (1, false)
        );

        assert_eq!(nzbfile_classify_meta_type(c"type", c"Password"), 1);
        assert_eq!(nzbfile_classify_meta_type(c"type", c"CATEGORY"), 2);
        assert_eq!(nzbfile_classify_meta_type(c"type", c"name"), 3);
        assert_eq!(nzbfile_classify_meta_type(c"type", c"title"), 4);
        assert_eq!(nzbfile_classify_meta_type(c"other", c"password"), 0);
    }
}
