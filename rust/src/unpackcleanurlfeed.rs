//! DirectUnpack, Cleanup, UnpackController, UrlCoordinator, FeedCoordinator & RemoteServer
//! Hot-Path Kernels (`unpackcleanurlfeed.rs`).
//!
//! Preserves exact byte-level, case-sensitivity, integer-arithmetic, and control-flow
//! semantics of the C++ call sites in:
//! - `daemon/postprocess/DirectUnpack.cpp`
//! - `daemon/postprocess/Cleanup.cpp`
//! - `daemon/postprocess/UnpackController.cpp`
//! - `daemon/queue/UrlCoordinator.cpp`
//! - `daemon/feed/FeedCoordinator.cpp`
//! - `daemon/remote/RemoteServer.cpp`

/// `DirectUnpack::Run` / `DirectUnpack::Stop`:
/// `if (nzbInfo->GetPostInfo() && !nzbInfo->GetLiveRepairThread())`
pub fn direct_unpack_should_clear_working(has_post_info: bool, has_live_repair_thread: bool) -> bool {
    has_post_info && !has_live_repair_thread
}

/// `DirectUnpack::CreateUnpackDir`:
/// `const char* destDir = useInterDir && !m_finalDir.Empty() ? *m_finalDir : *m_destDir;`
pub fn direct_unpack_use_final_dir(use_inter_dir: bool, final_dir: &[u8]) -> bool {
    use_inter_dir && !final_dir.is_empty()
}

/// `DirectUnpack::ReadLine` / `UnpackController::ReadLine`:
/// `if (!strncmp(buf, "...", 3))` -> returns true when `buf` starts with `"..."`.
pub fn direct_unpack_is_progress_dots_line(buf: &[u8]) -> bool {
    buf.starts_with(b"...")
}

/// `DirectUnpack::Stop`:
/// `if (!(g_PrePostProcessor && g_PrePostProcessor->IsStopped()))`
pub fn direct_unpack_should_set_cancelled(has_prepost: bool, prepost_stopped: bool) -> bool {
    !(has_prepost && prepost_stopped)
}

/// `DirectUnpack::Stop` / `UnpackController::Stop`:
/// `if (!isExecThread && !m_cancelled && HasActiveProcess())`
pub fn direct_unpack_should_try_break(
    is_exec_thread: bool,
    cancelled_or_auto_terminated: bool,
    has_active_process: bool,
) -> bool {
    !is_exec_thread && !cancelled_or_auto_terminated && has_active_process
}

/// `DirectUnpack::WaitNextVolume`:
/// `if (nzbInfo && (strcmp(m_destDir, nzbInfo->GetDestDir()) || strcmp(m_finalDir, nzbInfo->BuildFinalDirName())))`
pub fn direct_unpack_has_dest_changed(
    has_nzb: bool,
    cur_dest: &[u8],
    nzb_dest: &[u8],
    cur_final: &[u8],
    nzb_final: &[u8],
) -> bool {
    has_nzb && (cur_dest != nzb_dest || cur_final != nzb_final)
}

/// `DirectUnpack::FileDownloaded`:
/// `if (m_waitingFile && !strcasecmp(fileInfo->GetFilename(), m_waitingFile))`
pub fn direct_unpack_matches_waiting_file(
    waiting_file: Option<&[u8]>,
    downloaded_filename: &[u8],
) -> bool {
    match waiting_file {
        Some(wf) if !wf.is_empty() => wf.eq_ignore_ascii_case(downloaded_filename),
        _ => false,
    }
}

/// `DirectUnpack::CanProceed`:
/// `if (!IsArchiveFilename(fileInfo->GetFilename())) return true; return fileInfo->GetFailedArticles() == 0;`
pub fn direct_unpack_can_proceed(is_archive: bool, failed_articles: i32) -> bool {
    !is_archive || failed_articles == 0
}

/// `DirectUnpack::AddExtraTime`:
/// Calculates updated `UnpackSec` and `PostTotalSec` when `m_extraStartTime != 0`.
pub fn direct_unpack_calc_extra_time(
    extra_start_time: i64,
    cur_time: i64,
    cur_unpack_sec: i32,
    cur_post_total_sec: i32,
) -> (bool, i32, i32) {
    if extra_start_time != 0 {
        let extra_time = cur_time.wrapping_sub(extra_start_time) as i32;
        (
            true,
            cur_unpack_sec.wrapping_add(extra_time),
            cur_post_total_sec.wrapping_add(extra_time),
        )
    } else {
        (false, cur_unpack_sec, cur_post_total_sec)
    }
}

/// `MoveController::MoveFiles` / `UnpackController::Cleanup`:
/// `bool isDotFile = !filename.empty() && filename[0] == '.';`
pub fn move_ctrl_is_dot_file(filename: &[u8]) -> bool {
    filename.first() == Some(&b'.')
}

/// `UnpackController::Run`:
/// `unpack = !(parameter && !strcasecmp(parameter->GetValue(), "no"));`
pub fn unpack_ctrl_eval_unpack_param(param_val: Option<&[u8]>) -> bool {
    match param_val {
        Some(v) => !v.eq_ignore_ascii_case(b"no"),
        None => true,
    }
}

/// `UnpackController::Run`:
/// `bool hasFiles = m_hasRarFiles || m_hasSevenZipFiles || m_hasSevenZipMultiFiles || m_hasSplittedFiles;`
pub fn unpack_ctrl_has_any_archive_files(
    has_rar: bool,
    has_7z: bool,
    has_7z_multi: bool,
    has_splitted: bool,
) -> bool {
    has_rar || has_7z || has_7z_multi || has_splitted
}

/// `UnpackController::UnpackArchives`:
/// `while (!m_unpackOk && !m_unpackStartError && !m_unpackSpaceError && (m_unpackDecryptError || m_unpackPasswordError) && ...)`
pub fn unpack_ctrl_can_continue_password_loop(
    unpack_ok: bool,
    start_err: bool,
    space_err: bool,
    decrypt_err: bool,
    password_err: bool,
) -> bool {
    !unpack_ok && !start_err && !space_err && (decrypt_err || password_err)
}

/// `UnpackController::UnpackArchives`:
/// `if (IsStopped() && m_autoTerminated)`
pub fn unpack_ctrl_should_resume_before_password_try(
    is_stopped: bool,
    auto_terminated: bool,
) -> bool {
    is_stopped && auto_terminated
}

/// `UnpackController::UnpackArchives`:
/// `m_passListTried = !IsStopped() || m_autoTerminated;`
pub fn unpack_ctrl_eval_pass_list_tried(is_stopped: bool, auto_terminated: bool) -> bool {
    !is_stopped || auto_terminated
}

/// `UnpackController::JoinSplittedFiles`:
/// Matches `.*\.[a-z,0-9]{3}\.001$` (case-insensitive on `[a-z,0-9]`, matching `RegEx(".*\\.[a-z,0-9]{3}\\.001$")`
/// where `RegEx` uses `REG_ICASE` by default in NZBGet).
pub fn unpack_ctrl_is_split_first_piece(filename: &[u8]) -> bool {
    if filename.len() < 8 {
        return false;
    }
    let tail = &filename[filename.len() - 8..];
    if tail[0] != b'.' || &tail[4..8] != b".001" {
        return false;
    }
    tail[1..4]
        .iter()
        .all(|&b| b.is_ascii_alphanumeric() || b == b',')
}

/// `UnpackController::JoinFile`:
/// Updates `written` and returns stage progress permille `int(written * 1000 / totalSize)`.
pub fn unpack_ctrl_calc_join_progress(
    cur_written: i64,
    cnt: i32,
    total_size: i64,
) -> (i64, i32) {
    let add = if cnt > 0 { cnt as i64 } else { 0 };
    let new_written = cur_written.wrapping_add(add);
    let progress = if total_size > 0 {
        (new_written.wrapping_mul(1000) / total_size) as i32
    } else {
        0
    };
    (new_written, progress)
}

/// `UnpackController::JoinFile`:
/// `if (ok && written != totalSize)`
pub fn unpack_ctrl_is_join_size_mismatch(ok: bool, written: i64, total_size: i64) -> bool {
    ok && written != total_size
}

/// `UnpackController::IsDownloadedFile`:
/// Returns `true` when `g_Options->GetUseTempUnpackDir() || m_downloadedFiles.empty()`
/// allows bypassing the `m_downloadedFiles.Exists(filename)` linear scan.
pub fn unpack_ctrl_is_downloaded_file_allowed(
    use_temp_unpack_dir: bool,
    downloaded_empty: bool,
) -> bool {
    use_temp_unpack_dir || downloaded_empty
}

/// `UnpackController::Cleanup`:
/// `if (m_unpackOk && g_Options->GetUseTempUnpackDir())`
pub fn unpack_ctrl_should_move_from_temp_unpack(
    unpack_ok: bool,
    use_temp_unpack_dir: bool,
) -> bool {
    unpack_ok && use_temp_unpack_dir
}

/// `UnpackController::Cleanup`:
/// `if (m_unpackOk && ok && g_Options->GetUnpackCleanupDisk())`
pub fn unpack_ctrl_should_cleanup_archive_disks(
    unpack_ok: bool,
    move_ok: bool,
    unpack_cleanup_disk: bool,
) -> bool {
    unpack_ok && move_ok && unpack_cleanup_disk
}

/// `UrlCoordinator::DownloadQueueUpdate`:
/// `if (queueAspect->action == DownloadQueue::eaUrlAdded || queueAspect->action == DownloadQueue::eaUrlReturned)`
pub fn url_coord_is_wake_action(action: i32, ea_url_added: i32, ea_url_returned: i32) -> bool {
    action == ea_url_added || action == ea_url_returned
}

/// `UrlCoordinator::DeleteQueueEntry`:
/// `nzbInfo->GetDeleteStatus() == NzbInfo::dsNone ? NzbInfo::dsManual : nzbInfo->GetDeleteStatus()`
pub fn url_coord_resolve_delete_status(
    cur_delete_status: i32,
    ds_none: i32,
    ds_manual: i32,
) -> i32 {
    if cur_delete_status == ds_none {
        ds_manual
    } else {
        cur_delete_status
    }
}

/// `FeedCoordinator::Run`:
/// `if (!g_WorkState->GetPauseDownload() || m_force || g_Options->GetUrlForce())`
pub fn feed_coord_can_check_feeds(
    pause_download: bool,
    force: bool,
    url_force: bool,
) -> bool {
    !pause_download || force || url_force
}

/// `FeedCoordinator::StartFeedDownload`:
/// `if (feedInfo->GetId() > 0 && !feedInfo->GetPreview())`
pub fn feed_coord_use_feed_id_temp_file(id: i32, preview: bool) -> bool {
    id > 0 && !preview
}

/// `FeedCoordinator::PreviewFeed`:
/// `if (cacheTimeSec > 0 && *cacheId != '\0')`
pub fn feed_coord_can_use_cache(cache_time_sec: i32, cache_id: &[u8]) -> bool {
    cache_time_sec > 0 && !cache_id.is_empty()
}

/// `FeedCoordinator::PreviewFeed`:
/// `if (!strcmp(feedInfo2->GetUrl(), feedInfo->GetUrl()) && !strcmp(feedInfo2->GetFilter(), feedInfo->GetFilter()) && feedInfo2->GetLastUpdate() > 0)`
pub fn feed_coord_matches_existing_feed(
    url1: &[u8],
    url2: &[u8],
    filter1: &[u8],
    filter2: &[u8],
    last_update: i64,
) -> bool {
    url1 == url2 && filter1 == filter2 && last_update > 0
}

/// `FeedCoordinator::PreviewFeed`:
/// Resolves preview item status from `firstFetch`, `backlog`, and optional history item status.
pub fn feed_coord_resolve_preview_item_status(
    has_history: bool,
    history_status: i32,
    first_fetch: bool,
    backlog: bool,
    is_backlog: i32,
    is_new: i32,
) -> i32 {
    if has_history {
        history_status
    } else if first_fetch && backlog {
        is_backlog
    } else {
        is_new
    }
}

/// `RemoteServer::Run`:
/// `if (bind && !acceptedConnection && m_connection->GetAcceptBusy() && !IsStopped())`
pub fn remote_server_is_accept_busy_retry(
    bind: bool,
    has_accepted_conn: bool,
    accept_busy: bool,
    is_stopped: bool,
) -> bool {
    bind && !has_accepted_conn && accept_busy && !is_stopped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_unpack_helpers() {
        assert!(direct_unpack_should_clear_working(true, false));
        assert!(!direct_unpack_should_clear_working(true, true));
        assert!(!direct_unpack_should_clear_working(false, false));

        assert!(direct_unpack_use_final_dir(true, b"/dst/final"));
        assert!(!direct_unpack_use_final_dir(true, b""));
        assert!(!direct_unpack_use_final_dir(false, b"/dst/final"));

        assert!(direct_unpack_is_progress_dots_line(b"...50%"));
        assert!(!direct_unpack_is_progress_dots_line(b"..50%"));

        assert!(direct_unpack_should_set_cancelled(false, false));
        assert!(direct_unpack_should_set_cancelled(true, false));
        assert!(!direct_unpack_should_set_cancelled(true, true));

        assert!(direct_unpack_should_try_break(false, false, true));
        assert!(!direct_unpack_should_try_break(true, false, true));
        assert!(!direct_unpack_should_try_break(false, true, true));

        assert!(!direct_unpack_has_dest_changed(
            true,
            b"/a",
            b"/a",
            b"/b",
            b"/b"
        ));
        assert!(direct_unpack_has_dest_changed(
            true,
            b"/a",
            b"/a2",
            b"/b",
            b"/b"
        ));
        assert!(!direct_unpack_has_dest_changed(
            false,
            b"/a",
            b"/a2",
            b"/b",
            b"/b"
        ));

        assert!(direct_unpack_matches_waiting_file(
            Some(b"Show.r01"),
            b"show.R01"
        ));
        assert!(!direct_unpack_matches_waiting_file(None, b"show.R01"));

        assert!(direct_unpack_can_proceed(false, 5));
        assert!(direct_unpack_can_proceed(true, 0));
        assert!(!direct_unpack_can_proceed(true, 1));

        assert_eq!(
            direct_unpack_calc_extra_time(100, 115, 10, 20),
            (true, 25, 35)
        );
        assert_eq!(
            direct_unpack_calc_extra_time(0, 115, 10, 20),
            (false, 10, 20)
        );
    }

    #[test]
    fn test_cleanup_and_unpack_ctrl_helpers() {
        assert!(move_ctrl_is_dot_file(b".hidden"));
        assert!(!move_ctrl_is_dot_file(b"visible.mkv"));
        assert!(!move_ctrl_is_dot_file(b""));

        assert!(unpack_ctrl_eval_unpack_param(None));
        assert!(unpack_ctrl_eval_unpack_param(Some(b"yes")));
        assert!(!unpack_ctrl_eval_unpack_param(Some(b"NO")));

        assert!(unpack_ctrl_has_any_archive_files(false, false, true, false));
        assert!(!unpack_ctrl_has_any_archive_files(
            false, false, false, false
        ));

        assert!(unpack_ctrl_can_continue_password_loop(
            false, false, false, true, false
        ));
        assert!(!unpack_ctrl_can_continue_password_loop(
            true, false, false, true, false
        ));

        assert!(unpack_ctrl_should_resume_before_password_try(true, true));
        assert!(!unpack_ctrl_should_resume_before_password_try(true, false));

        assert!(unpack_ctrl_eval_pass_list_tried(false, false));
        assert!(unpack_ctrl_eval_pass_list_tried(true, true));
        assert!(!unpack_ctrl_eval_pass_list_tried(true, false));

        assert!(unpack_ctrl_is_split_first_piece(b"movie.mkv.001"));
        assert!(unpack_ctrl_is_split_first_piece(b"movie.MKV.001"));
        assert!(!unpack_ctrl_is_split_first_piece(b"movie.mkv.002"));
        assert!(!unpack_ctrl_is_split_first_piece(b"movie.mk.001"));

        assert_eq!(
            unpack_ctrl_calc_join_progress(500, 500, 2000),
            (1000, 500)
        );
        assert!(unpack_ctrl_is_join_size_mismatch(true, 900, 1000));
        assert!(!unpack_ctrl_is_join_size_mismatch(true, 1000, 1000));

        assert!(unpack_ctrl_is_downloaded_file_allowed(true, false));
        assert!(unpack_ctrl_is_downloaded_file_allowed(false, true));
        assert!(!unpack_ctrl_is_downloaded_file_allowed(false, false));

        assert!(unpack_ctrl_should_move_from_temp_unpack(true, true));
        assert!(!unpack_ctrl_should_move_from_temp_unpack(true, false));

        assert!(unpack_ctrl_should_cleanup_archive_disks(true, true, true));
        assert!(!unpack_ctrl_should_cleanup_archive_disks(true, false, true));
    }

    #[test]
    fn test_url_feed_remote_helpers() {
        assert!(url_coord_is_wake_action(2, 1, 2));
        assert!(!url_coord_is_wake_action(3, 1, 2));

        assert_eq!(url_coord_resolve_delete_status(0, 0, 1), 1);
        assert_eq!(url_coord_resolve_delete_status(2, 0, 1), 2);

        assert!(feed_coord_can_check_feeds(false, false, false));
        assert!(feed_coord_can_check_feeds(true, true, false));
        assert!(!feed_coord_can_check_feeds(true, false, false));

        assert!(feed_coord_use_feed_id_temp_file(1, false));
        assert!(!feed_coord_use_feed_id_temp_file(1, true));
        assert!(!feed_coord_use_feed_id_temp_file(0, false));

        assert!(feed_coord_can_use_cache(60, b"abc"));
        assert!(!feed_coord_can_use_cache(0, b"abc"));
        assert!(!feed_coord_can_use_cache(60, b""));

        assert!(feed_coord_matches_existing_feed(
            b"https://a",
            b"https://a",
            b"f1",
            b"f1",
            100
        ));
        assert!(!feed_coord_matches_existing_feed(
            b"https://a",
            b"https://a",
            b"f1",
            b"f1",
            0
        ));

        assert_eq!(
            feed_coord_resolve_preview_item_status(true, 3, true, true, 1, 2),
            3
        );
        assert_eq!(
            feed_coord_resolve_preview_item_status(false, 3, true, true, 1, 2),
            1
        );
        assert_eq!(
            feed_coord_resolve_preview_item_status(false, 3, false, true, 1, 2),
            2
        );

        assert!(remote_server_is_accept_busy_retry(true, false, true, false));
        assert!(!remote_server_is_accept_busy_retry(true, true, true, false));
    }
}
