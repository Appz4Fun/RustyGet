//! ServerPool, StatMeter, PrePostProcessor, RarRenamer, DupeMatcher, Rename & ParChecker
//! hot-path kernels.
//!
//! Each function mirrors the corresponding C++ logic byte-for-byte (including wrapping
//! integer arithmetic, locale-aware `strcasecmp` delegation, and short-circuit flags).

use std::ffi::{c_char, c_int};

extern "C" {
    fn strcasecmp(a: *const c_char, b: *const c_char) -> c_int;
}

/// `ServerPool::FreeConnection`:
/// Determines whether freeing a connection should increment `m_levels[normLevel]`.
/// Mirrors:
/// `connection->GetNewsServer()->GetNormLevel() > -1 && connection->GetNewsServer()->GetActive()`
#[inline]
pub fn server_pool_should_increment_level_on_free(norm_level: i32, active: bool) -> bool {
    norm_level > -1 && active
}

/// `ServerPool::BlockServer`:
/// Determines whether the block timestamp changed (`new_block`) and whether to log a warning
/// (`should_warn = new_block && retry_interval > 0`).
#[inline]
pub fn server_pool_eval_block_server(
    prev_block_time: i64,
    cur_time: i64,
    retry_interval: i32,
) -> (bool, bool) {
    let new_block = prev_block_time != cur_time;
    let should_warn = new_block && retry_interval > 0;
    (new_block, should_warn)
}

/// `ServerPool::CloseUnusedConnections`:
/// Determines whether an idle pooled connection belongs to a server that was disabled
/// since the last check and should be disconnected and erased.
/// Mirrors:
/// `!connection->GetInUse() && (connection->GetNewsServer()->GetNormLevel() == -1 || !connection->GetNewsServer()->GetActive())`
#[inline]
pub fn server_pool_should_erase_disabled_connection(
    in_use: bool,
    norm_level: i32,
    active: bool,
) -> bool {
    !in_use && (norm_level == -1 || !active)
}

/// `StatMeter::AdjustTimeOffset`:
/// Computes `(local_time_delta, total_offset)` from `utc_time`, `loc_time`, and `time_correction`.
/// Mirrors:
/// `time_t localTimeDelta = utcTime - locTime;`
/// `(int)localTimeDelta + g_Options->GetTimeCorrection()`
#[inline]
pub fn statmeter_calc_local_time_offset(
    utc_time: i64,
    loc_time: i64,
    time_correction: i32,
) -> (i32, i32) {
    let delta = utc_time.wrapping_sub(loc_time) as i32;
    (delta, delta.wrapping_add(time_correction))
}

/// `StatMeter::EnterLeaveStandBy`:
/// Computes the updated `m_startDownload` timestamp when leaving standby.
/// Mirrors:
/// `m_startDownload += Util::CurrentTime() - m_pausedFrom;`
#[inline]
pub fn statmeter_calc_resumed_download_start(
    prev_start_download: i64,
    cur_time: i64,
    paused_from: i64,
) -> i64 {
    prev_start_download.wrapping_add(cur_time.wrapping_sub(paused_from))
}

/// `StatMeter::CalcMonthSlots`:
/// Computes elapsed days when `QuotaStartDay > dayparts.tm_mday` using the previous month start timestamp.
/// Mirrors:
/// `(int)(locCurTime - prevMonth) / 60 / 60 / 24 + 1`
#[inline]
pub fn statmeter_calc_month_slots_prev_month(loc_cur_time: i64, prev_month_time: i64) -> i32 {
    let diff = loc_cur_time.wrapping_sub(prev_month_time) as i32;
    (diff / 60 / 60 / 24).wrapping_add(1)
}

/// `StatMeter::CalcMonthSlots`:
/// Computes elapsed days when `QuotaStartDay <= dayparts.tm_mday` within the current month.
/// Mirrors:
/// `dayparts.tm_mday - g_Options->GetQuotaStartDay() + 1`
#[inline]
pub fn statmeter_calc_month_slots_same_month(tm_mday: i32, quota_start_day: i32) -> i32 {
    tm_mday.wrapping_sub(quota_start_day).wrapping_add(1)
}

/// `PrePostProcessor::HasActiveUnpack`:
/// Evaluates whether a `DirectUnpack` thread counts as active unpack work.
/// Mirrors:
/// `includeWaitingVolume || (directUnpack->IsUnpacking() && !directUnpack->IsWaitingVolume())`
#[inline]
pub fn prepost_is_direct_unpack_active(
    include_waiting_volume: bool,
    is_unpacking: bool,
    is_waiting_volume: bool,
) -> bool {
    include_waiting_volume || (is_unpacking && !is_waiting_volume)
}

/// `PrePostProcessor::DownloadQueueUpdate`:
/// Determines whether an `eaNzbDeleted` queue event is for a deleting NZB with no post-info
/// and an empty file list.
#[inline]
pub fn prepost_is_empty_deleting_nzb_event(
    deleting: bool,
    has_post_info: bool,
    file_list_empty: bool,
) -> bool {
    deleting && !has_post_info && file_list_empty
}

/// `PrePostProcessor::DownloadQueueUpdate`:
/// Determines whether an active post-job is in a par-check stage eligible to receive a newly
/// completed or deleted par file via `RepairController::AddPar`.
#[inline]
pub fn prepost_can_try_add_par_to_active_repair(
    has_post_job: bool,
    same_nzb: bool,
    has_post_info: bool,
    has_post_thread: bool,
    stage: i32,
    pt_loading_pars: i32,
    pt_verifying_repaired: i32,
) -> bool {
    has_post_job
        && same_nzb
        && has_post_info
        && has_post_thread
        && stage >= pt_loading_pars
        && stage <= pt_verifying_repaired
}

/// `PrePostProcessor::CleanupJobs`:
/// Evaluates whether an active post-job is ready for cleanup, whether its `PostTotalSec`
/// should be updated, whether `JobCompleted` should be invoked vs resetting stage to `ptQueued`,
/// and the updated `PostTotalSec`.
#[inline]
pub fn prepost_eval_cleanup_job_state(
    working: bool,
    has_running_thread: bool,
    stage: i32,
    pt_finished: i32,
    deleted: bool,
    start_time: i64,
    cur_time: i64,
    prev_post_total_sec: i32,
) -> (bool, bool, bool, i32) {
    if working || has_running_thread {
        return (false, false, false, prev_post_total_sec);
    }
    let update_total_sec = start_time > 0;
    let new_total_sec = if update_total_sec {
        let elapsed = cur_time.wrapping_sub(start_time) as i32;
        prev_post_total_sec.wrapping_add(elapsed)
    } else {
        prev_post_total_sec
    };
    let is_completed = stage == pt_finished || deleted;
    (true, update_total_sec, is_completed, new_total_sec)
}

/// `PrePostProcessor::StartJob`:
/// Determines whether the job should enter `ptParRenaming`.
/// Mirrors:
/// `nzbInfo->GetParRenameStatus() == NzbInfo::rsNone && nzbInfo->GetDeleteStatus() == NzbInfo::dsNone && g_Options->GetParRename()`
#[inline]
pub fn prepost_should_start_par_rename(
    par_rename_status: i32,
    rs_none: i32,
    delete_status: i32,
    ds_none: i32,
    opt_par_rename: bool,
) -> bool {
    par_rename_status == rs_none && delete_status == ds_none && opt_par_rename
}

/// `PrePostProcessor::StartJob`:
/// Determines whether stream repair is initially enabled for the collection.
/// Mirrors:
/// `g_Options->GetDupeArticleFallback() >= Options::dafStream && !nzbInfo->GetStreamRepairJobs()->empty() && nzbInfo->GetDeleteStatus() == NzbInfo::dsNone`
#[inline]
pub fn prepost_is_stream_repair_enabled(
    dupe_article_fallback: i32,
    daf_stream: i32,
    jobs_empty: bool,
    delete_status: i32,
    ds_none: i32,
) -> bool {
    dupe_article_fallback >= daf_stream && !jobs_empty && delete_status == ds_none
}

/// `PrePostProcessor::StartJob`:
/// Evaluates the cheap preconditions before checking `ParParser::FindMainPars` to unpause
/// the smallest paused par2 volume when the index par2 file is missing.
/// Mirrors:
/// `(nzbInfo->GetParStatus() == NzbInfo::psNone || nzbInfo->GetParStatus() == NzbInfo::psSkipped) && nzbInfo->GetDeleteStatus() == NzbInfo::dsNone && nzbInfo->CalcHealth() < 1000`
#[inline]
pub fn prepost_should_check_unpaused_par_for_missing_index(
    par_status: i32,
    ps_none: i32,
    ps_skipped: i32,
    delete_status: i32,
    ds_none: i32,
    health: i32,
) -> bool {
    (par_status == ps_none || par_status == ps_skipped) && delete_status == ds_none && health < 1000
}

/// `PrePostProcessor::StartJob`:
/// Determines whether `fileInfo` is a paused, non-deleted par file smaller than the current
/// `smallest` candidate.
/// Mirrors:
/// `fileInfo->GetParFile() && fileInfo->GetPaused() && !fileInfo->GetDeleted() && (!smallest || fileInfo->GetSize() < smallest->GetSize())`
#[inline]
pub fn prepost_is_better_paused_par_candidate(
    is_par_file: bool,
    paused: bool,
    deleted: bool,
    has_smallest: bool,
    file_size: i64,
    smallest_size: i64,
) -> bool {
    is_par_file && paused && !deleted && (!has_smallest || file_size < smallest_size)
}

/// `RarRenamer::CheckFiles`:
/// Computes stage progress in `[0, 1000]`.
/// Mirrors:
/// `m_fileCount > 0 ? m_curFile * 1000 / m_fileCount : 1000`
#[inline]
pub fn rar_renamer_calc_stage_progress(file_count: i32, cur_file: i32) -> i32 {
    if file_count > 0 {
        cur_file.wrapping_mul(1000) / file_count
    } else {
        1000
    }
}

/// `RarRenamer::GenOldExtension`:
/// Computes the old-style RAR extension character and 2-digit suffix for `volume_no > 0`.
/// Mirrors:
/// `unsigned char ch = 'r' + (volumeNo - 1) / 100;`
/// `(volumeNo - 1) % 100`
#[inline]
pub fn rar_renamer_calc_old_extension_parts(volume_no: i32) -> (u8, i32) {
    let idx = volume_no.wrapping_sub(1);
    let ch = b'r'.wrapping_add((idx / 100) as u8);
    let num = idx % 100;
    (ch, num)
}

/// `ParChecker::ProcessTwinSet`:
/// Computes the number of additional recovery blocks needed for a twin par2 set.
/// Mirrors:
/// `static_cast<int>(GetRepairer()->missingblockcount - GetRepairer()->recoverypacketmap.size())`
#[inline]
pub fn parchecker_calc_twin_blocks_needed(
    missing_block_count: usize,
    recovery_packet_count: usize,
) -> i32 {
    missing_block_count.wrapping_sub(recovery_packet_count) as i32
}

/// `ParChecker::AddSplittedFragments`:
/// Checks whether `original` is non-empty and differs case-insensitively from `current`
/// (using C `strcasecmp` to respect the active `LC_CTYPE` locale).
/// Mirrors:
/// `!Util::EmptyStr(original) && strcasecmp(original, current)`
///
/// # Safety
/// `original` and `current` must be either null or valid null-terminated C strings.
#[inline]
pub unsafe fn parchecker_should_check_original_splitted_fragment(
    original: *const c_char,
    current: *const c_char,
) -> bool {
    if original.is_null() || *original == 0 || current.is_null() {
        return false;
    }
    strcasecmp(original, current) != 0
}

/// `ParChecker::VerifyPartialDataFile`:
/// Computes the byte range and zero-padding length for a contiguous range of valid blocks
/// `[block_start..=block_end]`.
/// Mirrors:
/// ```cpp
/// int64 bytesStart = blockStart * blocksize;
/// int64 bytesEnd = blockEnd * blocksize + blocksize - 1;
/// int64 clampedEnd = bytesEnd < fileSize - 1 ? bytesEnd : fileSize - 1;
/// size_t padLen = bytesEnd > fileSize - 1 ? (size_t)(bytesEnd - (fileSize - 1)) : 0;
/// ```
#[inline]
pub fn parchecker_calc_block_byte_range(
    block_start: i32,
    block_end: i32,
    block_size: i64,
    file_size: i64,
) -> (i64, i64, i64, usize) {
    let bytes_start = (block_start as i64).wrapping_mul(block_size);
    let bytes_end = (block_end as i64)
        .wrapping_mul(block_size)
        .wrapping_add(block_size)
        .wrapping_sub(1);
    let file_last = file_size.wrapping_sub(1);
    let clamped_end = if bytes_end < file_last {
        bytes_end
    } else {
        file_last
    };
    let pad_len = if bytes_end > file_last {
        bytes_end.wrapping_sub(file_last) as usize
    } else {
        0
    };
    (bytes_start, bytes_end, clamped_end, pad_len)
}

/// `ParChecker::GetPacketCreator`:
/// Computes the payload length of a `CREATORPACKET` by subtracting `sizeof(Par2::PACKET_HEADER)`.
/// Mirrors:
/// `size_t len = (creatorpacket->header.length - sizeof(Par2::PACKET_HEADER));`
#[inline]
pub fn parchecker_calc_creator_packet_payload_len(
    packet_length: u64,
    header_size: usize,
) -> usize {
    packet_length.wrapping_sub(header_size as u64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_server_pool_helpers() {
        assert!(server_pool_should_increment_level_on_free(0, true));
        assert!(!server_pool_should_increment_level_on_free(-1, true));
        assert!(!server_pool_should_increment_level_on_free(1, false));

        assert_eq!(server_pool_eval_block_server(100, 200, 10), (true, true));
        assert_eq!(server_pool_eval_block_server(100, 200, 0), (true, false));
        assert_eq!(server_pool_eval_block_server(200, 200, 10), (false, false));

        assert!(server_pool_should_erase_disabled_connection(false, -1, true));
        assert!(server_pool_should_erase_disabled_connection(false, 0, false));
        assert!(!server_pool_should_erase_disabled_connection(true, -1, false));
        assert!(!server_pool_should_erase_disabled_connection(false, 0, true));
    }

    #[test]
    fn test_statmeter_helpers() {
        assert_eq!(
            statmeter_calc_local_time_offset(1_700_003_600, 1_700_000_000, -60),
            (3600, 3540)
        );
        assert_eq!(
            statmeter_calc_resumed_download_start(1000, 1500, 1200),
            1300
        );
        assert_eq!(
            statmeter_calc_month_slots_prev_month(1_700_000_000 + 5 * 86400, 1_700_000_000),
            6
        );
        assert_eq!(statmeter_calc_month_slots_same_month(20, 5), 16);
    }

    #[test]
    fn test_prepost_helpers() {
        assert!(prepost_is_direct_unpack_active(true, false, true));
        assert!(prepost_is_direct_unpack_active(false, true, false));
        assert!(!prepost_is_direct_unpack_active(false, true, true));
        assert!(!prepost_is_direct_unpack_active(false, false, false));

        assert!(prepost_is_empty_deleting_nzb_event(true, false, true));
        assert!(!prepost_is_empty_deleting_nzb_event(false, false, true));
        assert!(!prepost_is_empty_deleting_nzb_event(true, true, true));
        assert!(!prepost_is_empty_deleting_nzb_event(true, false, false));

        assert!(prepost_can_try_add_par_to_active_repair(true, true, true, true, 3, 2, 5));
        assert!(!prepost_can_try_add_par_to_active_repair(true, true, true, true, 6, 2, 5));

        assert_eq!(
            prepost_eval_cleanup_job_state(true, false, 10, 10, false, 100, 120, 50),
            (false, false, false, 50)
        );
        assert_eq!(
            prepost_eval_cleanup_job_state(false, false, 10, 10, false, 100, 120, 50),
            (true, true, true, 70)
        );
        assert_eq!(
            prepost_eval_cleanup_job_state(false, false, 2, 10, false, 0, 120, 50),
            (true, false, false, 50)
        );

        assert!(prepost_should_start_par_rename(0, 0, 0, 0, true));
        assert!(!prepost_should_start_par_rename(1, 0, 0, 0, true));

        assert!(prepost_is_stream_repair_enabled(2, 1, false, 0, 0));
        assert!(!prepost_is_stream_repair_enabled(2, 1, true, 0, 0));

        assert!(prepost_should_check_unpaused_par_for_missing_index(0, 0, 1, 0, 0, 950));
        assert!(!prepost_should_check_unpaused_par_for_missing_index(0, 0, 1, 0, 0, 1000));

        assert!(prepost_is_better_paused_par_candidate(true, true, false, false, 5000, 0));
        assert!(prepost_is_better_paused_par_candidate(true, true, false, true, 3000, 5000));
        assert!(!prepost_is_better_paused_par_candidate(true, true, false, true, 6000, 5000));
    }

    #[test]
    fn test_rar_dupe_rename_parchecker_helpers() {
        assert_eq!(rar_renamer_calc_stage_progress(10, 3), 300);
        assert_eq!(rar_renamer_calc_stage_progress(0, 0), 1000);
        assert_eq!(rar_renamer_calc_old_extension_parts(1), (b'r', 0));
        assert_eq!(rar_renamer_calc_old_extension_parts(105), (b's', 4));

        assert_eq!(parchecker_calc_twin_blocks_needed(15, 4), 11);

        let a = CString::new("movie.mkv").unwrap();
        let b = CString::new("MOVIE.MKV").unwrap();
        let c = CString::new("other.mkv").unwrap();
        let empty = CString::new("").unwrap();
        unsafe {
            assert!(!parchecker_should_check_original_splitted_fragment(
                a.as_ptr(),
                b.as_ptr()
            ));
            assert!(parchecker_should_check_original_splitted_fragment(
                a.as_ptr(),
                c.as_ptr()
            ));
            assert!(!parchecker_should_check_original_splitted_fragment(
                empty.as_ptr(),
                c.as_ptr()
            ));
        }

        assert_eq!(
            parchecker_calc_block_byte_range(2, 4, 1000, 4500),
            (2000, 4999, 4499, 500)
        );
        assert_eq!(parchecker_calc_creator_packet_payload_len(96, 64), 32);
    }
}
