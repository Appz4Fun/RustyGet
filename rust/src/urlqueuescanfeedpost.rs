//! UrlCoordinator, QueueEditor, Scanner, FeedCoordinator & PrePostProcessor Hot-Path Kernels
//!
//! Pure, zero-allocation state predicates and arithmetic kernels extracted from:
//! - `daemon/queue/UrlCoordinator.cpp`
//! - `daemon/queue/QueueEditor.cpp`
//! - `daemon/queue/Scanner.cpp`
//! - `daemon/feed/FeedCoordinator.cpp`
//! - `daemon/postprocess/PrePostProcessor.cpp`

/// Evaluates whether `UrlCoordinator::Run` has any remaining active or queued URL download work.
///
/// Mirrors `UrlCoordinator::Run` in `UrlCoordinator.cpp`:
/// `m_hasMoreJobs = !m_activeDownloads.empty() || nzbInfo;`
#[inline]
pub fn url_coord_has_more_jobs(active_empty: i32, has_nzb_info: i32) -> i32 {
    if active_empty == 0 || has_nzb_info != 0 {
        1
    } else {
        0
    }
}

/// Evaluates whether a primary C-string is non-empty so it overrides the fallback string in `UrlCoordinator::UrlCompleted`.
///
/// Mirrors `UrlCoordinator::UrlCompleted` in `UrlCoordinator.cpp`:
/// `const char* name = !Util::EmptyStr(nzbInfo->GetFilename()) ? nzbInfo->GetFilename() : *filename;`
#[inline]
pub fn url_coord_use_primary_str(primary: &[u8]) -> i32 {
    if !primary.is_empty() {
        1
    } else {
        0
    }
}

/// Updates the `[min_id, max_id]` bounds in `QueueEditor::PrepareList`.
///
/// Mirrors `QueueEditor::PrepareList` in `QueueEditor.cpp`:
/// `if (ID > maxId) maxId = ID; if (ID < minId) minId = ID;`
#[inline]
pub fn queue_editor_update_min_max_id(id: i32, in_min_id: i32, in_max_id: i32) -> (i32, i32) {
    let min_id = if id < in_min_id { id } else { in_min_id };
    let max_id = if id > in_max_id { id } else { in_max_id };
    (min_id, max_id)
}

/// Evaluates whether `id` falls within `[min_id, max_id]` inclusive in `QueueEditor::PrepareList`.
///
/// Mirrors `QueueEditor::PrepareList` in `QueueEditor.cpp`:
/// `if (minId <= id && id <= maxId)`
#[inline]
pub fn queue_editor_is_id_in_bounds(id: i32, min_id: i32, max_id: i32) -> i32 {
    if min_id <= id && id <= max_id {
        1
    } else {
        0
    }
}

/// Evaluates whether a queue item matches a name or regex filter and has not yet been added to `uniqueIds`.
///
/// Mirrors `QueueEditor::BuildIdListFromNameList` in `QueueEditor.cpp`:
/// `((!regEx && !strcmp(filename, name)) || (regEx && regEx->Match(filename))) && (uniqueIds.find(id) == uniqueIds.end())`
#[inline]
pub fn queue_editor_matches_name_filter(
    has_regex: i32,
    regex_matched: i32,
    candidate: &[u8],
    pattern: &[u8],
    already_in_unique: i32,
) -> i32 {
    if already_in_unique == 0
        && ((has_regex == 0 && candidate == pattern) || (has_regex != 0 && regex_matched != 0))
    {
        1
    } else {
        0
    }
}

/// Evaluates whether `action` is one of the 4 group-deletion actions in `QueueEditor::EditGroup`.
///
/// Mirrors `QueueEditor::EditGroup` in `QueueEditor.cpp`:
/// `action == eaGroupDelete || action == eaGroupParkDelete || action == eaGroupDupeDelete || action == eaGroupFinalDelete`
#[inline]
pub fn queue_editor_is_group_delete_action(
    action: i32,
    ea_group_delete: i32,
    ea_group_park_delete: i32,
    ea_group_dupe_delete: i32,
    ea_group_final_delete: i32,
) -> i32 {
    if action == ea_group_delete
        || action == ea_group_park_delete
        || action == ea_group_dupe_delete
        || action == ea_group_final_delete
    {
        1
    } else {
        0
    }
}

/// Evaluates whether `Scanner::ServiceWork` should skip scanning because scanning is paused and no explicit scan was requested.
///
/// Mirrors `Scanner::ServiceWork` in `Scanner.cpp`:
/// `if (g_WorkState->GetPauseScan() && !m_requestedNzbDirScan) return;`
#[inline]
pub fn scanner_should_pause_scan(pause_scan: i32, requested_nzb_dir_scan: i32) -> i32 {
    if pause_scan != 0 && requested_nzb_dir_scan == 0 {
        1
    } else {
        0
    }
}

/// Evaluates whether `Scanner::ServiceWork` needs a second immediate pass to process files extracted by scan-scripts.
///
/// Mirrors `Scanner::ServiceWork` in `Scanner.cpp`:
/// `if (!checkStat && m_scanScript)`
#[inline]
pub fn scanner_needs_second_immediate_scan(check_stat: i32, scan_script: i32) -> i32 {
    if check_stat == 0 && scan_script != 0 {
        1
    } else {
        0
    }
}

/// Evaluates whether an extension script qualifies for automatic post-processing parameter initialization in `Scanner::InitPPParameters`.
///
/// Mirrors `Scanner::InitPPParameters` in `Scanner.cpp`:
/// `(script->GetPostScript() || script->GetQueueScript()) && !parameters->Find(paramName)`
#[inline]
pub fn scanner_script_qualifies_for_pp_param(
    is_post_script: i32,
    is_queue_script: i32,
    already_has_param: i32,
) -> i32 {
    if (is_post_script != 0 || is_queue_script != 0) && already_has_param == 0 {
        1
    } else {
        0
    }
}

/// Evaluates whether `Scanner::AddFileToQueue` should rename the NZB file with `.queued` (`1`) or `.error` (`0`).
///
/// Mirrors `Scanner::AddFileToQueue` in `Scanner.cpp`:
/// `ok ? "queued" : "error"`
#[inline]
pub fn scanner_select_rename_bak_queued(ok: i32) -> i32 {
    if ok != 0 {
        1
    } else {
        0
    }
}

/// Evaluates whether a `FeedDownloader` status represents a completed download state in `FeedCoordinator::Update`.
///
/// Mirrors `FeedCoordinator::Update` in `FeedCoordinator.cpp`:
/// `status == WebDownloader::adFinished || status == WebDownloader::adFailed || status == WebDownloader::adRetry`
#[inline]
pub fn feed_coord_is_completed_status(
    status: i32,
    ad_finished: i32,
    ad_failed: i32,
    ad_retry: i32,
) -> i32 {
    if status == ad_finished || status == ad_failed || status == ad_retry {
        1
    } else {
        0
    }
}

/// Evaluates whether a 1-based feed ID is within `[1, feeds_len]` inclusive.
///
/// Mirrors `FeedCoordinator::ViewFeed` and `PreviewFeed` in `FeedCoordinator.cpp`:
/// `id >= 1 && id <= static_cast<int>(m_feeds.size())`
#[inline]
pub fn feed_coord_is_valid_feed_id(id: i32, feeds_len: i32) -> i32 {
    if id >= 1 && id <= feeds_len {
        1
    } else {
        0
    }
}

/// Evaluates whether a feed matches a `FetchFeed(id)` request (`feed_id == target_id || target_id == 0`).
///
/// Mirrors `FeedCoordinator::FetchFeed` in `FeedCoordinator.cpp`:
/// `if (feedInfo->GetId() == id || id == 0)`
#[inline]
pub fn feed_coord_matches_fetch_feed_id(feed_id: i32, target_id: i32) -> i32 {
    if feed_id == target_id || target_id == 0 {
        1
    } else {
        0
    }
}

/// Evaluates whether `PrePostProcessor::WaitJobs` still has time remaining within the graceful wait window.
///
/// Mirrors `PrePostProcessor::WaitJobs` in `PrePostProcessor.cpp`:
/// `Util::CurrentTime() < waitStart + 5`
#[inline]
pub fn prepost_is_within_graceful_wait(cur_time: i64, wait_start: i64, timeout_sec: i64) -> i32 {
    if cur_time < wait_start.wrapping_add(timeout_sec) {
        1
    } else {
        0
    }
}

/// Evaluates whether `PrePostProcessor::NzbAdded` should reorder files for the newly added NZB.
///
/// Mirrors `PrePostProcessor::NzbAdded` in `PrePostProcessor.cpp`:
/// `g_Options->GetReorderFiles() && nzbInfo->GetDeleteStatus() == NzbInfo::dsNone`
#[inline]
pub fn prepost_should_reorder_added_nzb(
    reorder_files: i32,
    delete_status: i32,
    ds_none: i32,
) -> i32 {
    if reorder_files != 0 && delete_status == ds_none {
        1
    } else {
        0
    }
}

/// Evaluates whether a PAR2 file on disk is a better (smaller positive size) candidate for reading description packets in `ParCannotCover`.
///
/// Mirrors `ParCannotCover` in `PrePostProcessor.cpp`:
/// `size > 0 && (parFileSize < 0 || size < parFileSize)`
#[inline]
pub fn prepost_is_better_main_par_file(size: i64, best_size: i64) -> i32 {
    if size > 0 && (best_size < 0 || size < best_size) {
        1
    } else {
        0
    }
}

/// Evaluates the pre-condition for deferring stream repair to an untried duplicate backup in `PrePostProcessor::StartJob`.
///
/// Mirrors `PrePostProcessor::StartJob` in `PrePostProcessor.cpp`:
/// `streamRepair && !postInfo->GetStreamRepairDone() && g_Options->GetHealthCheck() == Options::hcDupe && DupeCoordinator::FailsOver(nzbInfo)`
#[inline]
pub fn prepost_should_defer_stream_repair_to_backup(
    stream_repair: i32,
    stream_repair_done: i32,
    health_check: i32,
    hc_dupe: i32,
    fails_over: i32,
) -> i32 {
    if stream_repair != 0
        && stream_repair_done == 0
        && health_check == hc_dupe
        && fails_over != 0
    {
        1
    } else {
        0
    }
}

/// Evaluates the pre-condition for running stream repair before par-check when par2 files cannot cover damage in `PrePostProcessor::StartJob`.
///
/// Mirrors `PrePostProcessor::StartJob` in `PrePostProcessor.cpp`:
/// `streamRepair && !postInfo->GetStreamRepairDone() && nzbInfo->GetParStatus() <= NzbInfo::psSkipped && g_Options->GetParCheck() == Options::pcAuto`
#[inline]
pub fn prepost_should_stream_repair_before_par(
    stream_repair: i32,
    stream_repair_done: i32,
    par_status: i32,
    ps_skipped: i32,
    par_check: i32,
    pc_auto: i32,
) -> i32 {
    if stream_repair != 0
        && stream_repair_done == 0
        && par_status <= ps_skipped
        && par_check == pc_auto
    {
        1
    } else {
        0
    }
}

/// Evaluates whether `PrePostProcessor::StartJob` should enter the post-PAR `ptStreamRepairing` stage.
///
/// Mirrors `PrePostProcessor::StartJob` in `PrePostProcessor.cpp`:
/// `streamRepair && nzbInfo->GetParStatus() != NzbInfo::psRepairPossible && nzbInfo->GetParStatus() != NzbInfo::psManual`
#[inline]
pub fn prepost_can_enter_final_stream_repair(
    stream_repair: i32,
    par_status: i32,
    ps_repair_possible: i32,
    ps_manual: i32,
) -> i32 {
    if stream_repair != 0 && par_status != ps_repair_possible && par_status != ps_manual {
        1
    } else {
        0
    }
}

/// Evaluates the pre-condition for requesting par-check on a collection without archive files in `PrePostProcessor::StartJob`.
///
/// Mirrors `PrePostProcessor::StartJob` in `PrePostProcessor.cpp`:
/// `nzbInfo->GetParStatus() == NzbInfo::psSkipped && nzbInfo->GetDeleteStatus() == NzbInfo::dsNone && g_Options->GetParCheck() == Options::pcAuto`
#[inline]
pub fn prepost_should_request_par_without_archives(
    par_status: i32,
    ps_skipped: i32,
    delete_status: i32,
    ds_none: i32,
    par_check: i32,
    pc_auto: i32,
) -> i32 {
    if par_status == ps_skipped && delete_status == ds_none && par_check == pc_auto {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_queue_scanner_and_feed_kernels() {
        assert_eq!(url_coord_has_more_jobs(0, 0), 1);
        assert_eq!(url_coord_has_more_jobs(1, 1), 1);
        assert_eq!(url_coord_has_more_jobs(1, 0), 0);

        assert_eq!(url_coord_use_primary_str(b"movie.nzb"), 1);
        assert_eq!(url_coord_use_primary_str(b""), 0);

        assert_eq!(queue_editor_update_min_max_id(5, 10, 20), (5, 20));
        assert_eq!(queue_editor_update_min_max_id(25, 10, 20), (10, 25));

        assert_eq!(queue_editor_is_id_in_bounds(15, 10, 20), 1);
        assert_eq!(queue_editor_is_id_in_bounds(25, 10, 20), 0);

        assert_eq!(
            queue_editor_matches_name_filter(0, 0, b"rel/a.rar", b"rel/a.rar", 0),
            1
        );
        assert_eq!(
            queue_editor_matches_name_filter(0, 0, b"rel/a.rar", b"rel/a.rar", 1),
            0
        );
        assert_eq!(
            queue_editor_matches_name_filter(1, 1, b"rel/a.rar", b".*", 0),
            1
        );

        assert_eq!(queue_editor_is_group_delete_action(18, 18, 19, 20, 21), 1);
        assert_eq!(queue_editor_is_group_delete_action(10, 18, 19, 20, 21), 0);

        assert_eq!(scanner_should_pause_scan(1, 0), 1);
        assert_eq!(scanner_should_pause_scan(1, 1), 0);

        assert_eq!(scanner_needs_second_immediate_scan(0, 1), 1);
        assert_eq!(scanner_needs_second_immediate_scan(1, 1), 0);

        assert_eq!(scanner_script_qualifies_for_pp_param(1, 0, 0), 1);
        assert_eq!(scanner_script_qualifies_for_pp_param(1, 0, 1), 0);
        assert_eq!(scanner_script_qualifies_for_pp_param(0, 0, 0), 0);

        assert_eq!(scanner_select_rename_bak_queued(1), 1);
        assert_eq!(scanner_select_rename_bak_queued(0), 0);

        assert_eq!(feed_coord_is_completed_status(2, 1, 2, 3), 1);
        assert_eq!(feed_coord_is_completed_status(0, 1, 2, 3), 0);

        assert_eq!(feed_coord_is_valid_feed_id(1, 3), 1);
        assert_eq!(feed_coord_is_valid_feed_id(0, 3), 0);
        assert_eq!(feed_coord_is_valid_feed_id(4, 3), 0);

        assert_eq!(feed_coord_matches_fetch_feed_id(2, 2), 1);
        assert_eq!(feed_coord_matches_fetch_feed_id(2, 0), 1);
        assert_eq!(feed_coord_matches_fetch_feed_id(2, 3), 0);
    }

    #[test]
    fn test_prepost_kernels() {
        assert_eq!(prepost_is_within_graceful_wait(1004, 1000, 5), 1);
        assert_eq!(prepost_is_within_graceful_wait(1005, 1000, 5), 0);

        assert_eq!(prepost_should_reorder_added_nzb(1, 0, 0), 1);
        assert_eq!(prepost_should_reorder_added_nzb(1, 1, 0), 0);

        assert_eq!(prepost_is_better_main_par_file(512, -1), 1);
        assert_eq!(prepost_is_better_main_par_file(256, 512), 1);
        assert_eq!(prepost_is_better_main_par_file(1024, 512), 0);
        assert_eq!(prepost_is_better_main_par_file(0, -1), 0);

        assert_eq!(
            prepost_should_defer_stream_repair_to_backup(1, 0, 3, 3, 1),
            1
        );
        assert_eq!(
            prepost_should_defer_stream_repair_to_backup(1, 1, 3, 3, 1),
            0
        );

        assert_eq!(
            prepost_should_stream_repair_before_par(1, 0, 1, 1, 0, 0),
            1
        );
        assert_eq!(
            prepost_should_stream_repair_before_par(1, 0, 2, 1, 0, 0),
            0
        );

        assert_eq!(prepost_can_enter_final_stream_repair(1, 2, 3, 4), 1);
        assert_eq!(prepost_can_enter_final_stream_repair(1, 3, 3, 4), 0);

        assert_eq!(
            prepost_should_request_par_without_archives(1, 1, 0, 0, 0, 0),
            1
        );
        assert_eq!(
            prepost_should_request_par_without_archives(0, 1, 0, 0, 0, 0),
            0
        );
    }
}
