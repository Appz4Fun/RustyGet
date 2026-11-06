//! Rust domain logic for `ArticleWriter`, `ArticleCache`, and `MemoryMonitor`
//! (`daemon/nntp/ArticleWriter.cpp`).

use std::ffi::CStr;

/// Formats `<tmp_dir><sep><file_id>.<part_num:03>` as in `ArticleWriter::BuildOutputFilename`.
pub fn article_writer_format_result_filename(
    tmp_dir: &[u8],
    sep: u8,
    file_id: i32,
    part_num: i32,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(tmp_dir.len() + 20);
    out.extend_from_slice(tmp_dir);
    out.push(sep);
    out.extend_from_slice(format!("{file_id}.{part_num:03}").as_bytes());
    out
}

/// Formats `<dest_dir><sep><file_id>.out.tmp` as in `ArticleWriter::BuildOutputFilename`.
pub fn article_writer_format_direct_out_tmp(dest_dir: &[u8], sep: u8, file_id: i32) -> Vec<u8> {
    let mut out = Vec::with_capacity(dest_dir.len() + 24);
    out.extend_from_slice(dest_dir);
    out.push(sep);
    out.extend_from_slice(format!("{file_id}.out.tmp").as_bytes());
    out
}

/// Matches `!strncmp(nzbDestDir.data(), m_outputFilename.c_str(), len) && (m_outputFilename[len] == PATH_SEPARATOR || m_outputFilename[len] == ALT_PATH_SEPARATOR)`
/// in `ArticleWriter::CleanupOldData`.
pub fn article_writer_is_within_dest_dir(
    nzb_dest_dir: &[u8],
    output_filename: &[u8],
    sep1: u8,
    sep2: u8,
) -> bool {
    let len = nzb_dest_dir.len();
    if output_filename.len() <= len {
        return false;
    }
    output_filename.starts_with(nzb_dest_dir)
        && (output_filename[len] == sep1 || output_filename[len] == sep2)
}

/// Classifies completion message kind in `ArticleWriter::ReportCompletionStatus`:
/// - `1`: `total_articles == success_articles` ("Successfully downloaded ...")
/// - `2`: `missed_articles + failed_articles > 0` ("%i of %i article downloads failed ...")
/// - `0`: partial ("Partially downloaded ...")
pub fn article_writer_classify_completion_status(
    total_articles: i32,
    success_articles: i32,
    missed_articles: i32,
    failed_articles: i32,
) -> i32 {
    if total_articles == success_articles {
        1
    } else if missed_articles.saturating_add(failed_articles) > 0 {
        2
    } else {
        0
    }
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

/// Classifies the output file setup scenario in `ArticleWriter::SetupOutputFile`:
/// `1` = standard assembly (`.tmp`), `2` = direct write (`omReadWrite`), `3` = raw mode (create dir), `0` = none.
pub fn article_writer_classify_setup_output_scenario(
    raw_article: bool,
    direct_write: bool,
) -> i32 {
    if !raw_article && !direct_write {
        1
    } else if direct_write {
        2
    } else if raw_article {
        3
    } else {
        0
    }
}

/// Matches `fileInfo->GetCachedArticles() > 0 && (fileInfo->GetActiveDownloads() == 0 || flushEverything) && (!fileInfo->GetNzbInfo()->GetSkipDiskWrite() || flushEverything)`
/// in `ArticleCache::CheckFlush`.
pub fn article_cache_can_flush_file(
    cached_articles: i32,
    active_downloads: i32,
    skip_disk_write: bool,
    flush_everything: bool,
) -> bool {
    cached_articles > 0
        && (active_downloads == 0 || flush_everything)
        && (!skip_disk_write || flush_everything)
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

/// Matches `MemoryMonitor::GetTierName`.
pub fn memory_monitor_tier_name(tier: i32, constrained: i32, critical: i32) -> &'static CStr {
    if tier == critical {
        c"CRITICAL"
    } else if tier == constrained {
        c"CONSTRAINED"
    } else {
        c"NORMAL"
    }
}

/// Matches `MemoryMonitor::GetEffectiveDownloadsLimit`.
pub fn memory_monitor_effective_downloads_limit(
    tier: i32,
    configured_limit: i32,
    constrained: i32,
    critical: i32,
) -> i32 {
    if tier == critical {
        1.max(configured_limit / 4)
    } else if tier == constrained {
        2.max(configured_limit / 2)
    } else {
        configured_limit
    }
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

/// Classifies the action for `MemoryMonitor::ApplyTierTransition`:
/// `1` = CRITICAL transition, `2` = CONSTRAINED transition, `3` = return to NORMAL, `0` = no-op.
pub fn memory_monitor_classify_tier_transition_action(
    old_tier: i32,
    new_tier: i32,
    tier_normal: i32,
    tier_constrained: i32,
    tier_critical: i32,
) -> i32 {
    if new_tier == tier_critical {
        1
    } else if new_tier == tier_constrained {
        2
    } else if new_tier == tier_normal && old_tier != tier_normal {
        3
    } else {
        0
    }
}

/// Formats `"<dest_dir><sep><filename>"` for `ArticleWriter`.
pub fn article_writer_format_dest_file_path(
    dest_dir: &[u8],
    sep: u8,
    filename: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(dest_dir.len() + 1 + filename.len());
    out.extend_from_slice(dest_dir);
    out.push(sep);
    out.extend_from_slice(filename);
    out
}

/// Formats `"<base_path>.tmp"` for `ArticleWriter`.
pub fn article_writer_format_tmp_path(base_path: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(base_path.len() + 4);
    out.extend_from_slice(base_path);
    out.extend_from_slice(b".tmp");
    out
}

/// Converts megabytes to bytes (`mb * 1024 * 1024`) for `ArticleCache` and `MemoryMonitor`.
pub fn article_cache_mb_to_bytes(mb: i32) -> usize {
    (mb.max(0) as usize)
        .saturating_mul(1024)
        .saturating_mul(1024)
}

/// Evaluates whether `ArticleWriter::SetupOutputFile` should use `targetPath` directly instead of `MakeUniqueFilename`.
pub fn article_writer_use_target_path_directly(
    force_direct_write: bool,
    direct_write: bool,
    output_filename_empty: bool,
    same_filename: bool,
) -> bool {
    force_direct_write || (direct_write && !output_filename_empty && same_filename)
}

/// Evaluates whether a candidate news server is available for article download (`ArticleDownloader::Run`).
pub fn article_dl_is_server_available(
    active: bool,
    max_connections: i32,
    optional: bool,
    is_blocked: bool,
) -> bool {
    active && max_connections != 0 && !(optional && is_blocked)
}

/// Formats `ArticleDownloader::Run` `m_connectionName` (`"%s (%s)"`, `server_name`, `host`).
pub fn article_dl_format_connection_name(server_name: &[u8], host: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(server_name.len() + host.len() + 3);
    out.extend_from_slice(server_name);
    out.extend_from_slice(b" (");
    out.extend_from_slice(host);
    out.push(b')');
    out
}

/// Formats `ArticleDownloader::Download` NNTP `ARTICLE` / `BODY` command line.
pub fn article_dl_format_fetch_command(raw_article: bool, message_id: &[u8]) -> Vec<u8> {
    let verb: &[u8] = if raw_article { b"ARTICLE " } else { b"BODY " };
    let mut out = Vec::with_capacity(verb.len() + message_id.len() + 2);
    out.extend_from_slice(verb);
    out.extend_from_slice(message_id);
    out.extend_from_slice(b"\r\n");
    out
}

/// Selects the yEnc article CRC in `ArticleDownloader::DecodeCheck`.
pub fn article_dl_select_yenc_crc(
    crc_check: bool,
    calculated_crc: u32,
    expected_crc: u32,
) -> u32 {
    if crc_check {
        calculated_crc
    } else {
        expected_crc
    }
}

/// Returns the static NUL-terminated error verb (`"open"` vs `"create"`) in `ArticleWriter::Start`.
pub fn article_writer_open_verb(direct_write: bool) -> &'static [u8] {
    if direct_write {
        b"open\0"
    } else {
        b"create\0"
    }
}

/// Formats `fullInfoPath` / `infoName` (`"%s%c%s"`, `nzb_name`, `sep`, `filename`) in `ArticleWriter` / `ArticleCache`.
pub fn article_writer_format_info_path(nzb_name: &[u8], sep: u8, filename: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(nzb_name.len() + 1 + filename.len());
    out.extend_from_slice(nzb_name);
    out.push(sep);
    out.extend_from_slice(filename);
    out
}

/// Computes the parent directory prefix length of `final_path` when `filename` contains `sep` in `ArticleWriter::SetupOutputFile`.
pub fn article_writer_parent_dir_len(
    filename: &[u8],
    final_path: &[u8],
    sep: u8,
) -> Option<usize> {
    if !filename.contains(&sep) {
        return None;
    }
    Some(
        final_path
            .iter()
            .rposition(|&b| b == sep)
            .unwrap_or(final_path.len()),
    )
}

/// Updates `(failed_articles, success_articles)` when a part file is missing in `ArticleWriter::ProcessArticles`.
pub fn article_writer_apply_missing_part_counters(
    failed_articles: i32,
    success_articles: i32,
) -> (i32, i32) {
    (
        failed_articles.wrapping_add(1),
        success_articles.wrapping_sub(1),
    )
}

/// Formats the raw article destination path (`"%s%c%03i"`) in `ArticleWriter::ProcessArticles`.
pub fn article_writer_format_raw_part_dest(
    final_output_path: &[u8],
    sep: u8,
    part_number: i32,
) -> Vec<u8> {
    let part_str = format!("{part_number:03}");
    let mut out = Vec::with_capacity(final_output_path.len() + 1 + part_str.len());
    out.extend_from_slice(final_output_path);
    out.push(sep);
    out.extend_from_slice(part_str.as_bytes());
    out
}

/// Computes total failed+missed articles in `ArticleWriter::ReportCompletionStatus`.
pub fn article_writer_calc_total_failed_articles(
    missed_articles: i32,
    failed_articles: i32,
) -> i32 {
    missed_articles.wrapping_add(failed_articles)
}

/// Steps the cumulative file CRC in `ArticleWriter::ProcessArticles` for yEnc articles.
/// Returns `(next_crc, next_first_article)` where `next_first_article` is always `false`.
pub fn article_writer_step_file_crc(
    first_article: bool,
    current_crc: u32,
    article_crc: u32,
    segment_size: i32,
) -> (u32, bool) {
    let next_crc = if first_article {
        article_crc
    } else {
        crate::crc::combine(current_crc, article_crc, segment_size as u32)
    };
    (next_crc, false)
}

/// Evaluates existing output file status in `ArticleWriter::CreateOutputFile`:
/// `0` = does not exist, `1` = keep existing file of equal size, `2` = delete existing file of different size.
pub fn article_writer_check_existing_output_file(
    exists: bool,
    existing_size: i64,
    target_size: i64,
) -> i32 {
    if !exists {
        0
    } else if existing_size == target_size {
        1
    } else {
        2
    }
}

/// Selects the direct-write output path mode in `ArticleWriter::BuildOutputFilename`:
/// `0` = not direct write, `1` = reuse existing `outputFilename`, `2` = use `destPath`, `3` = format `<id>.out.tmp`.
pub fn article_writer_select_direct_output_mode(
    direct_write: bool,
    force_direct_write: bool,
    has_existing_output: bool,
    output_initialized: bool,
    dest_exists: bool,
) -> i32 {
    if !direct_write && !force_direct_write {
        0
    } else if has_existing_output {
        1
    } else if force_direct_write && output_initialized && dest_exists {
        2
    } else {
        3
    }
}

/// Returns true when `ArticleWriter::SetupOutputFile` in direct-write mode should fall back to opening `finalPath`.
pub fn article_writer_should_fallback_open_to_final_path(
    open_exists: bool,
    final_exists: bool,
) -> bool {
    !open_exists && final_exists
}

/// Returns true when a staged duplicate article's size or copied byte count matches `segment_size`.
pub fn article_writer_validate_staged_dupe_size(
    actual_size: i64,
    segment_size: i32,
) -> bool {
    actual_size == i64::from(segment_size)
}

/// Evaluates post-close commit action in `ArticleWriter::CommitDiskFile`:
/// `-1` = flush/close failed, `1` = move `tempDestPath` to `finalOutputPath`, `0` = direct-write complete.
pub fn article_writer_eval_commit_post_close(
    flushed: bool,
    closed: bool,
    direct_write: bool,
) -> i32 {
    if !flushed || !closed {
        -1
    } else if !direct_write {
        1
    } else {
        0
    }
}

/// Returns true when an article has a staged duplicate temp file to delete in `ArticleWriter::CleanupOldData`.
pub fn article_writer_is_staged_dupe_article_file(
    dupe_fallback_round: i32,
    has_result_filename: bool,
) -> bool {
    dupe_fallback_round > 0 && has_result_filename
}

/// Classifies the final output file cleanup action in `ArticleWriter::CleanupOldData` (direct-write branch):
/// `1` = move `m_outputFilename` to `finalOutputPath`, `2` = delete `m_outputFilename`, `0` = no-op.
pub fn article_writer_classify_direct_final_cleanup(
    final_empty: bool,
    same_filename: bool,
    output_exists: bool,
) -> i32 {
    if !final_empty {
        if !same_filename {
            1
        } else {
            0
        }
    } else if output_exists {
        2
    } else {
        0
    }
}

/// Returns true when `ArticleCache::Alloc` should write the `acache` flag file on disk.
pub fn article_cache_should_write_cache_flag(
    was_empty: bool,
    server_mode: bool,
    continue_partial: bool,
    skip_write: bool,
) -> bool {
    was_empty && server_mode && continue_partial && !skip_write
}

/// Selects the post-flush action in `ArticleCache::Run`:
/// `1` = `MemoryMonitor::TrimHeap()`, `2` = `Util::Sleep(5)`, `0` = none.
pub fn article_cache_post_flush_action(
    need_emergency_flush: bool,
    allocated: usize,
    just_flushed: bool,
    is_stopped: bool,
) -> i32 {
    if need_emergency_flush && allocated == 0 {
        1
    } else if !just_flushed && !is_stopped {
        2
    } else {
        0
    }
}

/// Returns true when `ArticleDownloader::Run` should break out of the server loop (`status == adFinished || status == adFatalError`).
pub fn article_dl_is_terminal_loop_status(
    status: i32,
    ad_finished: i32,
    ad_fatal_error: i32,
) -> bool {
    status == ad_finished || status == ad_fatal_error
}

/// Returns true when `ArticleDownloader::Run` should fail immediately because a desired server failed and isn't being retried (`!has_want_server && has_desired_server`).
pub fn article_dl_should_fail_for_desired_server(
    has_want_server: bool,
    has_desired_server: bool,
) -> bool {
    !has_want_server && has_desired_server
}

/// Returns true when `ArticleDownloader::Download` should continue reading/decoding body chunks (`!is_stopped && !decoder_eof`).
pub fn article_dl_should_continue_read_loop(is_stopped: bool, decoder_eof: bool) -> bool {
    !is_stopped && !decoder_eof
}

/// Returns true when `ArticleDownloader::Download` should release the connection and run `DecodeCheck()` (`status == adRunning`).
pub fn article_dl_should_run_decode_check(status: i32, ad_running: i32) -> bool {
    status == ad_running
}

/// Returns true when `ArticleWriter::IsDupeFallbackArticle` is active (`has_article_info && dupe_fallback_round > 0`).
pub fn article_writer_is_dupe_fallback_article(
    has_article_info: bool,
    dupe_fallback_round: i32,
) -> bool {
    has_article_info && dupe_fallback_round > 0
}

/// Returns true when `ArticleWriter::HandlePostProcessing` should rename the output file because `m_fileInfo->GetFilename() != originalFilename`.
pub fn article_writer_was_renamed_during_completion(
    cur_filename: &[u8],
    orig_filename: &[u8],
) -> bool {
    cur_filename != orig_filename
}

/// Returns true when `ArticleWriter::HandlePostProcessing` should delete the hard-linked temporary file (`is_hard_linked && inter_dir_empty`).
pub fn article_writer_should_delete_hardlinked_without_interdir(
    is_hard_linked: bool,
    inter_dir_empty: bool,
) -> bool {
    is_hard_linked && inter_dir_empty
}

/// Returns true when `ArticleWriter::HandlePostProcessing` should move completed files because `DestDir` changed (`cur_dest_dir != orig_dest_dir`).
pub fn article_writer_did_dest_dir_change(
    cur_dest_dir: &[u8],
    orig_dest_dir: &[u8],
) -> bool {
    cur_dest_dir != orig_dest_dir
}

/// Returns true when `ArticleCache::Run` should continue its main loop (`!is_stopped || allocated > 0`).
pub fn article_cache_should_continue_run_loop(is_stopped: bool, allocated: usize) -> bool {
    !is_stopped || allocated > 0
}

/// Returns true when `MemoryMonitor::Run` should keep sleeping between state checks (`!is_stopped && slept < interval`).
pub fn memory_monitor_should_keep_sleeping(
    is_stopped: bool,
    slept: i32,
    interval: i32,
) -> bool {
    !is_stopped && slept < interval
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_article_writer_and_cache_helpers() {
        assert_eq!(
            article_writer_format_result_filename(b"/tmp", b'/', 42, 7),
            b"/tmp/42.007"
        );
        assert_eq!(
            article_writer_format_direct_out_tmp(b"/dest", b'/', 42),
            b"/dest/42.out.tmp"
        );
        assert!(article_writer_is_within_dest_dir(
            b"/dest/job",
            b"/dest/job/file.rar",
            b'/',
            b'\\'
        ));
        assert!(!article_writer_is_within_dest_dir(
            b"/dest/job",
            b"/dest/job2/file.rar",
            b'/',
            b'\\'
        ));
        assert_eq!(article_writer_classify_completion_status(10, 10, 0, 0), 1);
        assert_eq!(article_writer_classify_completion_status(10, 8, 2, 0), 2);
        assert_eq!(article_writer_classify_completion_status(10, 8, 0, 0), 0);
        assert_eq!(
            article_writer_advance_and_check_range(false, 100, 50, 200),
            (150, false)
        );
        assert_eq!(
            article_writer_advance_and_check_range(false, 180, 50, 200),
            (230, true)
        );
        assert_eq!(
            article_writer_resolve_direct_write_ready(true, false, false, true, false),
            (true, true)
        );
        assert_eq!(article_writer_classify_setup_output_scenario(false, false), 1);
        assert_eq!(article_writer_classify_setup_output_scenario(false, true), 2);
        assert_eq!(article_writer_classify_setup_output_scenario(true, false), 3);
        assert!(article_cache_can_flush_file(3, 0, false, false));
        assert!(!article_cache_can_flush_file(3, 1, false, false));
        assert!(article_cache_can_flush_file(3, 1, true, true));
        assert_eq!(article_cache_can_alloc(1000, 0, 500), (true, true));
        assert_eq!(article_cache_can_alloc(1000, 600, 500), (false, false));
        assert_eq!(
            memory_monitor_tier_name(2, 1, 2).to_bytes(),
            b"CRITICAL"
        );
        assert_eq!(memory_monitor_effective_downloads_limit(2, 20, 1, 2), 5);
        assert_eq!(memory_monitor_effective_downloads_limit(1, 20, 1, 2), 10);
        assert_eq!(memory_monitor_effective_downloads_limit(0, 20, 1, 2), 20);
        assert_eq!(
            memory_monitor_calc_critical_or_normal_par_buffer(2, 64, 2, 0),
            (true, 16)
        );
        assert_eq!(
            memory_monitor_resolve_tier_limits(2, 2, 500, 250, 20, 0, 1, 2, 125, 64),
            (0, 16, 5)
        );
        assert_eq!(
            memory_monitor_classify_tier_transition_action(0, 2, 0, 1, 2),
            1
        );
    }
}
