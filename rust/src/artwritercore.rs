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

/// Output filenames computed by `ArticleWriter::BuildOutputFilename`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleWriterBuiltFilenames {
    pub result_filename: Vec<u8>,
    pub temp_filename: Vec<u8>,
    pub output_filename: Option<Vec<u8>>,
    pub update_file_info_output_filename: bool,
}

/// Computes `result_filename`, `temp_filename`, and (when `direct_write || force_direct_write`)
/// `output_filename` and `update_file_info_output_filename` for `ArticleWriter::BuildOutputFilename`.
#[allow(clippy::too_many_arguments)]
pub fn article_writer_build_output_filenames(
    tmp_dir: &[u8],
    dest_dir: &[u8],
    sep: u8,
    file_id: i32,
    part_num: i32,
    direct_write: bool,
    force_direct_write: bool,
    existing_output_filename: &[u8],
    output_initialized: bool,
    file_filename: &[u8],
    file_exists: impl FnOnce(&[u8]) -> bool,
) -> Option<ArticleWriterBuiltFilenames> {
    if tmp_dir.is_empty() || dest_dir.is_empty() {
        return None;
    }
    let result_filename = article_writer_format_result_filename(tmp_dir, sep, file_id, part_num);
    let temp_filename = article_writer_format_tmp_path(&result_filename);
    if !direct_write && !force_direct_write {
        return Some(ArticleWriterBuiltFilenames {
            result_filename,
            temp_filename,
            output_filename: None,
            update_file_info_output_filename: false,
        });
    }
    let has_existing_output = !existing_output_filename.is_empty();
    let dest_path = if !has_existing_output && force_direct_write && output_initialized {
        Some(article_writer_format_dest_file_path(
            dest_dir,
            sep,
            file_filename,
        ))
    } else {
        None
    };
    let dest_exists = dest_path.as_deref().is_some_and(file_exists);
    let direct_mode = article_writer_select_direct_output_mode(
        direct_write,
        force_direct_write,
        has_existing_output,
        output_initialized,
        dest_exists,
    );
    let (output_filename, update_file_info_output_filename) = match direct_mode {
        1 => (Some(existing_output_filename.to_vec()), false),
        2 => (dest_path, true),
        3 => (
            Some(article_writer_format_direct_out_tmp(dest_dir, sep, file_id)),
            true,
        ),
        _ => (Some(result_filename.clone()), false),
    };
    Some(ArticleWriterBuiltFilenames {
        result_filename,
        temp_filename,
        output_filename,
        update_file_info_output_filename,
    })
}

/// Output path setup plan computed by `ArticleWriter::SetupOutputFile`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleWriterSetupOutputPlan {
    pub final_path: Vec<u8>,
    pub temp_path: Vec<u8>,
    pub parent_dir_to_create: Option<Vec<u8>>,
    pub scenario: i32,
    pub direct_open_path: Vec<u8>,
    pub fallback_to_final_path: bool,
}

/// Plans `finalPath`, `tempPath`, optional `parentDir` creation, `setupScenario`, and
/// direct-write open fallback for `ArticleWriter::SetupOutputFile`.
#[allow(clippy::too_many_arguments)]
pub fn article_writer_plan_setup_output_paths(
    dest_dir: &[u8],
    filename: &[u8],
    sep: u8,
    raw_article: bool,
    direct_write: bool,
    force_direct_write: bool,
    output_filename: &[u8],
    same_filename: impl FnOnce(&[u8], &[u8]) -> bool,
    make_unique_filename: impl FnOnce(&[u8], &[u8]) -> Vec<u8>,
    mut file_exists: impl FnMut(&[u8]) -> bool,
) -> ArticleWriterSetupOutputPlan {
    let target_path = article_writer_format_dest_file_path(dest_dir, sep, filename);
    let is_same = !output_filename.is_empty() && same_filename(output_filename, &target_path);
    let final_path = if article_writer_use_target_path_directly(
        force_direct_write,
        direct_write,
        output_filename.is_empty(),
        is_same,
    ) {
        target_path
    } else {
        make_unique_filename(dest_dir, filename)
    };
    let temp_path = article_writer_format_tmp_path(&final_path);
    let parent_dir_to_create = article_writer_parent_dir_len(filename, &final_path, sep)
        .map(|len| final_path[..len].to_vec());
    let scenario = article_writer_classify_setup_output_scenario(raw_article, direct_write);
    let (direct_open_path, fallback_to_final_path) = if scenario == 2 {
        let open_exists = file_exists(output_filename);
        let final_exists = !open_exists && file_exists(&final_path);
        if article_writer_should_fallback_open_to_final_path(open_exists, final_exists) {
            (final_path.clone(), true)
        } else {
            (output_filename.to_vec(), false)
        }
    } else {
        (Vec::new(), false)
    };
    ArticleWriterSetupOutputPlan {
        final_path,
        temp_path,
        parent_dir_to_create,
        scenario,
        direct_open_path,
        fallback_to_final_path,
    }
}

/// Cleanup plan computed for `ArticleWriter::CleanupOldData`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleWriterCleanupOldDataPlan {
    pub direct_cleanup_action: i32,
    pub old_dest_dir_to_prune: Option<Vec<u8>>,
    pub delete_hardlinked_output: bool,
}

/// Plans the direct-write final file cleanup action (`1` = move, `2` = delete, `0` = none),
/// optional old destination directory to prune if empty, and hardlink output deletion for `ArticleWriter::CleanupOldData`.
#[allow(clippy::too_many_arguments)]
pub fn article_writer_plan_cleanup_old_data(
    direct_write: bool,
    nzb_dest_dir: &[u8],
    final_output_path: &[u8],
    output_filename: &[u8],
    is_hard_linked: bool,
    inter_dir_empty: bool,
    sep1: u8,
    sep2: u8,
    same_filename: impl FnOnce(&[u8], &[u8]) -> bool,
    file_exists: impl FnOnce(&[u8]) -> bool,
) -> ArticleWriterCleanupOldDataPlan {
    let final_empty = final_output_path.is_empty();
    let (direct_cleanup_action, old_dest_dir_to_prune) = if direct_write {
        let is_same = !final_empty && same_filename(output_filename, final_output_path);
        let out_exists = final_empty && file_exists(output_filename);
        let action = article_writer_classify_direct_final_cleanup(final_empty, is_same, out_exists);
        let old_dir = if !article_writer_is_within_dest_dir(
            nzb_dest_dir,
            output_filename,
            sep1,
            sep2,
        ) {
            let base_idx = output_filename
                .iter()
                .rposition(|&b| b == sep1 || b == sep2)
                .map_or(0, |k| k + 1);
            Some(output_filename[..base_idx.min(1023)].to_vec())
        } else {
            None
        };
        (action, old_dir)
    } else {
        (0, None)
    };
    let delete_hardlinked_output = final_empty && is_hard_linked && inter_dir_empty;
    ArticleWriterCleanupOldDataPlan {
        direct_cleanup_action,
        old_dest_dir_to_prune,
        delete_hardlinked_output,
    }
}

/// Execution result of `ArticleWriter::SetupOutputFile`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleWriterSetupOutputResult {
    pub ok: bool,
    pub final_path: Vec<u8>,
    pub temp_path: Vec<u8>,
    pub updated_output_filename: Vec<u8>,
    pub parent_dir_failed: Option<Vec<u8>>,
    pub error_kind: i32, // 0 = none, 1 = could not create file, 2 = could not open file, 3 = could not create directory
    pub error_path: Vec<u8>,
}

/// Executes `ArticleWriter::SetupOutputFile` end-to-end, invoking the caller's filesystem
/// primitives (`force_directories`, `delete_file`, `open_file`, `create_directory`).
#[allow(clippy::too_many_arguments)]
pub fn article_writer_execute_setup_output_file(
    dest_dir: &[u8],
    filename: &[u8],
    sep: u8,
    raw_article: bool,
    direct_write: bool,
    force_direct_write: bool,
    output_filename: &[u8],
    same_filename: impl FnOnce(&[u8], &[u8]) -> bool,
    make_unique_filename: impl FnOnce(&[u8], &[u8]) -> Vec<u8>,
    file_exists: impl FnMut(&[u8]) -> bool,
    force_directories: impl FnOnce(&[u8]) -> bool,
    delete_file: impl FnOnce(&[u8]) -> bool,
    open_file: impl FnOnce(&[u8], bool) -> bool,
    create_directory: impl FnOnce(&[u8]) -> bool,
) -> ArticleWriterSetupOutputResult {
    let plan = article_writer_plan_setup_output_paths(
        dest_dir,
        filename,
        sep,
        raw_article,
        direct_write,
        force_direct_write,
        output_filename,
        same_filename,
        make_unique_filename,
        file_exists,
    );
    let mut parent_dir_failed = None;
    if let Some(ref pdir) = plan.parent_dir_to_create {
        if !force_directories(pdir) {
            parent_dir_failed = Some(pdir.clone());
        }
    }
    let mut updated_output_filename = output_filename.to_vec();
    let mut temp_path = plan.temp_path.clone();
    let mut error_kind = 0;
    let mut error_path = Vec::new();
    let mut ok = true;

    match plan.scenario {
        1 => {
            let _ = delete_file(&temp_path);
            if !open_file(&temp_path, false) {
                ok = false;
                error_kind = 1;
                error_path = temp_path.clone();
            }
        }
        2 => {
            if plan.fallback_to_final_path {
                updated_output_filename = plan.final_path.clone();
            }
            if !open_file(&plan.direct_open_path, true) {
                ok = false;
                error_kind = 2;
                error_path = plan.direct_open_path.clone();
            } else {
                temp_path = updated_output_filename.clone();
            }
        }
        3 => {
            let _ = delete_file(&temp_path);
            if !create_directory(&plan.final_path) {
                ok = false;
                error_kind = 3;
                error_path = plan.final_path.clone();
            }
        }
        _ => {}
    }

    ArticleWriterSetupOutputResult {
        ok,
        final_path: plan.final_path,
        temp_path,
        updated_output_filename,
        parent_dir_failed,
        error_kind,
        error_path,
    }
}

/// Execution result of `ArticleWriter::CleanupOldData`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleWriterCleanupOldDataResult {
    pub move_failed: bool,
    pub checked_old_dest_dir: Option<Vec<u8>>,
    pub deleted_old_dest_dir: Option<Vec<u8>>,
    pub clear_output_filename: bool,
}

/// Executes `ArticleWriter::CleanupOldData` end-to-end, invoking the caller's filesystem
/// primitives (`delete_file`, `move_file`, `dir_empty`, `remove_directory`).
#[allow(clippy::too_many_arguments)]
pub fn article_writer_execute_cleanup_old_data(
    direct_write: bool,
    nzb_dest_dir: &[u8],
    final_output_path: &[u8],
    output_filename: &[u8],
    is_hard_linked: bool,
    inter_dir_empty: bool,
    sep1: u8,
    sep2: u8,
    articles: &[(i32, Option<&[u8]>)],
    same_filename: impl FnOnce(&[u8], &[u8]) -> bool,
    file_exists: impl FnOnce(&[u8]) -> bool,
    mut delete_file: impl FnMut(&[u8]) -> bool,
    move_file: impl FnOnce(&[u8], &[u8]) -> bool,
    dir_empty: impl FnOnce(&[u8]) -> bool,
    remove_directory: impl FnOnce(&[u8]) -> bool,
) -> ArticleWriterCleanupOldDataResult {
    let plan = article_writer_plan_cleanup_old_data(
        direct_write,
        nzb_dest_dir,
        final_output_path,
        output_filename,
        is_hard_linked,
        inter_dir_empty,
        sep1,
        sep2,
        same_filename,
        file_exists,
    );
    let mut move_failed = false;
    let mut checked_old_dest_dir = None;
    let mut deleted_old_dest_dir = None;
    let mut clear_output_filename = false;

    if direct_write {
        for &(dupe_fallback_round, result_filename) in articles {
            if let Some(rf) = result_filename {
                if article_writer_is_staged_dupe_article_file(dupe_fallback_round, true) {
                    let _ = delete_file(rf);
                }
            }
        }
        if plan.direct_cleanup_action == 1 {
            if !move_file(output_filename, final_output_path) {
                move_failed = true;
            }
        } else if plan.direct_cleanup_action == 2 {
            let _ = delete_file(output_filename);
        }
        if let Some(old_dir) = plan.old_dest_dir_to_prune {
            if dir_empty(&old_dir) {
                let _ = remove_directory(&old_dir);
                deleted_old_dest_dir = Some(old_dir.clone());
            }
            checked_old_dest_dir = Some(old_dir);
        }
    } else {
        for &(_, result_filename) in articles {
            if let Some(rf) = result_filename {
                let _ = delete_file(rf);
            }
        }
    }

    if plan.delete_hardlinked_output && delete_file(output_filename) {
        clear_output_filename = true;
    }

    ArticleWriterCleanupOldDataResult {
        move_failed,
        checked_old_dest_dir,
        deleted_old_dest_dir,
        clear_output_filename,
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

/// `ArticleWriter::SetWriteBuffer` (`ArticleWriter.cpp`).
pub fn calc_write_buffer_size(write_buffer_kb: i32, rec_size: i32) -> i32 {
    if write_buffer_kb <= 0 {
        return 0;
    }
    let max_bytes = write_buffer_kb.saturating_mul(1024);
    if rec_size > 0 && rec_size < max_bytes {
        rec_size
    } else {
        max_bytes
    }
}

pub const PRESSURE_NORMAL: i32 = 0;
pub const PRESSURE_CONSTRAINED: i32 = 1;
pub const PRESSURE_CRITICAL: i32 = 2;

/// `ArticleCache::Run` (`ArticleWriter.cpp`):
/// Evaluates whether the cache loop should call `CheckFlush` and with what flags.
#[allow(clippy::too_many_arguments)]
pub fn article_cache_should_flush(
    allocated: usize,
    effective_limit: usize,
    tier: i32,
    just_flushed: bool,
    reset_counter: i32,
    is_stopped: bool,
    direct_write: bool,
) -> (bool, bool, bool) {
    let is_critical = tier == PRESSURE_CRITICAL;
    let is_constrained = tier == PRESSURE_CONSTRAINED;
    let fill_threshold = if effective_limit > 0 {
        (effective_limit / 100) * 90
    } else {
        0
    };
    let need_emergency_flush = is_critical && allocated > 0;
    let need_constrained_flush = is_constrained && allocated > effective_limit;

    let should_flush = (just_flushed
        || reset_counter >= 1000
        || is_stopped
        || need_emergency_flush
        || need_constrained_flush
        || (direct_write && allocated >= fill_threshold))
        && allocated > 0;

    let flush_everything = is_stopped || need_emergency_flush || allocated >= fill_threshold;
    (should_flush, flush_everything, need_emergency_flush)
}

/// `MemoryMonitor::UpdateState` (`ArticleWriter.cpp`):
/// Immediate escalation on higher pressure (1 sample) and 2-sample hysteresis on de-escalation.
/// Returns `(next_tier, transitioned)`.
pub fn memory_monitor_step(
    raw_sample: i32,
    current_tier: i32,
    candidate_tier: &mut i32,
    consecutive_samples: &mut i32,
) -> (i32, bool) {
    let mut next_tier = current_tier;
    let mut transitioned = false;

    if raw_sample > current_tier {
        next_tier = raw_sample;
        *candidate_tier = raw_sample;
        *consecutive_samples = 0;
        transitioned = true;
    } else if raw_sample < current_tier {
        if raw_sample == *candidate_tier {
            *consecutive_samples += 1;
            if *consecutive_samples >= 2 {
                next_tier = raw_sample;
                *consecutive_samples = 0;
                transitioned = true;
            }
        } else {
            *candidate_tier = raw_sample;
            *consecutive_samples = 1;
        }
    } else {
        *candidate_tier = raw_sample;
        *consecutive_samples = 0;
    }

    (next_tier, transitioned)
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
        assert_eq!(calc_write_buffer_size(1024, 500_000), 500_000);
        assert_eq!(calc_write_buffer_size(512, 600_000), 512 * 1024);

        let mut cand = PRESSURE_NORMAL;
        let mut consec = 0;
        let (next1, trans1) =
            memory_monitor_step(PRESSURE_CRITICAL, PRESSURE_NORMAL, &mut cand, &mut consec);
        assert_eq!(next1, PRESSURE_CRITICAL);
        assert!(trans1);

        let (next2, trans2) =
            memory_monitor_step(PRESSURE_NORMAL, PRESSURE_CRITICAL, &mut cand, &mut consec);
        assert_eq!(next2, PRESSURE_CRITICAL);
        assert!(!trans2);
        let (next3, trans3) =
            memory_monitor_step(PRESSURE_NORMAL, PRESSURE_CRITICAL, &mut cand, &mut consec);
        assert_eq!(next3, PRESSURE_NORMAL);
        assert!(trans3);
    }

    #[test]
    fn article_writer_full_suite_port_from_cpp() {
        use std::ffi::CString;
        #[cfg(unix)]
        use std::os::unix::ffi::OsStrExt;
        use std::path::Path;

        fn fs_stat(p: &[u8]) -> i32 {
            #[cfg(unix)]
            let path = Path::new(std::ffi::OsStr::from_bytes(p));
            #[cfg(not(unix))]
            let path = Path::new(std::str::from_utf8(p).unwrap_or(""));
            match std::fs::metadata(path) {
                Ok(m) => {
                    if m.is_file() {
                        1
                    } else if m.is_dir() {
                        2
                    } else {
                        0
                    }
                }
                Err(e) => -e.raw_os_error().unwrap_or(2),
            }
        }

        // 1. ForcedDirectWriteKeepsExistingOutputInCollectionDirectory
        {
            let dir = std::env::temp_dir().join(format!(
                "nzbget-artwriter-force-direct-{}-{}",
                std::process::id(),
                1
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let output_path = dir.join("part151.rar");
            std::fs::write(&output_path, b"existing archive bytes").unwrap();

            #[cfg(unix)]
            let dir_bytes = dir.as_os_str().as_bytes();
            #[cfg(not(unix))]
            let dir_bytes = dir.to_string_lossy().as_bytes().to_vec();
            #[cfg(unix)]
            let out_bytes = output_path.as_os_str().as_bytes();
            #[cfg(not(unix))]
            let out_bytes = output_path.to_string_lossy().as_bytes().to_vec();

            let built = article_writer_build_output_filenames(
                b"/tmp",
                dir_bytes,
                crate::paths::PATH_SEPARATOR,
                1,
                1,
                false,
                true,
                out_bytes,
                true,
                b"part151.rar",
                |p| fs_stat(p) == 1,
            )
            .unwrap();
            assert_eq!(built.output_filename.as_deref(), Some(out_bytes));
            assert!(!built.update_file_info_output_filename);

            let mut opened_handle: Option<std::fs::File> = None;
            let setup = article_writer_execute_setup_output_file(
                dir_bytes,
                b"part151.rar",
                crate::paths::PATH_SEPARATOR,
                false,
                true,
                true,
                built.output_filename.as_deref().unwrap(),
                |a, b| a == b,
                |d, f| {
                    #[cfg(unix)]
                    {
                        let dc = CString::new(d).unwrap();
                        let fc = CString::new(f).unwrap();
                        crate::fsops::make_unique_filename(&dc, &fc, &fs_stat)
                    }
                    #[cfg(not(unix))]
                    {
                        [d, &[crate::paths::PATH_SEPARATOR], f].concat()
                    }
                },
                |p| fs_stat(p) == 1,
                |p| {
                    #[cfg(unix)]
                    let path = Path::new(std::ffi::OsStr::from_bytes(p));
                    #[cfg(not(unix))]
                    let path = Path::new(std::str::from_utf8(p).unwrap_or(""));
                    std::fs::create_dir_all(path).is_ok()
                },
                |p| {
                    #[cfg(unix)]
                    let path = Path::new(std::ffi::OsStr::from_bytes(p));
                    #[cfg(not(unix))]
                    let path = Path::new(std::str::from_utf8(p).unwrap_or(""));
                    std::fs::remove_file(path).is_ok()
                },
                |p, read_write| {
                    #[cfg(unix)]
                    let path = Path::new(std::ffi::OsStr::from_bytes(p));
                    #[cfg(not(unix))]
                    let path = Path::new(std::str::from_utf8(p).unwrap_or(""));
                    let res = if read_write {
                        std::fs::OpenOptions::new().read(true).write(true).open(path)
                    } else {
                        std::fs::OpenOptions::new()
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .open(path)
                    };
                    opened_handle = res.ok();
                    opened_handle.is_some()
                },
                |p| {
                    #[cfg(unix)]
                    let path = Path::new(std::ffi::OsStr::from_bytes(p));
                    #[cfg(not(unix))]
                    let path = Path::new(std::str::from_utf8(p).unwrap_or(""));
                    std::fs::create_dir(path).is_ok()
                },
            );
            assert!(setup.ok);
            drop(opened_handle);
            assert_eq!(setup.final_path, out_bytes);
            assert_eq!(setup.temp_path, out_bytes);

            let cleanup = article_writer_execute_cleanup_old_data(
                true,
                dir_bytes,
                &setup.final_path,
                &setup.updated_output_filename,
                false,
                true,
                crate::paths::PATH_SEPARATOR,
                crate::paths::ALT_PATH_SEPARATOR,
                &[(0, Some(built.result_filename.as_slice()))],
                |a, b| a == b,
                |p| fs_stat(p) == 1,
                |p| {
                    #[cfg(unix)]
                    let path = Path::new(std::ffi::OsStr::from_bytes(p));
                    #[cfg(not(unix))]
                    let path = Path::new(std::str::from_utf8(p).unwrap_or(""));
                    std::fs::remove_file(path).is_ok()
                },
                |from, to| {
                    #[cfg(unix)]
                    let src = Path::new(std::ffi::OsStr::from_bytes(from));
                    #[cfg(not(unix))]
                    let src = Path::new(std::str::from_utf8(from).unwrap_or(""));
                    #[cfg(unix)]
                    let dst = Path::new(std::ffi::OsStr::from_bytes(to));
                    #[cfg(not(unix))]
                    let dst = Path::new(std::str::from_utf8(to).unwrap_or(""));
                    std::fs::rename(src, dst).is_ok()
                },
                |d| {
                    #[cfg(unix)]
                    let path = Path::new(std::ffi::OsStr::from_bytes(d));
                    #[cfg(not(unix))]
                    let path = Path::new(std::str::from_utf8(d).unwrap_or(""));
                    std::fs::read_dir(path)
                        .map(|mut it| it.next().is_none())
                        .unwrap_or(false)
                },
                |d| {
                    #[cfg(unix)]
                    let path = Path::new(std::ffi::OsStr::from_bytes(d));
                    #[cfg(not(unix))]
                    let path = Path::new(std::str::from_utf8(d).unwrap_or(""));
                    std::fs::remove_dir(path).is_ok()
                },
            );
            assert!(!cleanup.move_failed);
            assert_eq!(cleanup.checked_old_dest_dir, None);
            assert!(!cleanup.clear_output_filename);
            assert!(output_path.exists());
            assert!(!dir.join("Release title").exists());
            assert_eq!(
                std::fs::read(&output_path).unwrap(),
                b"existing archive bytes"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }

        // 2. DirectWritePreservesUnrelatedExistingOutputInCollectionDirectory
        {
            let dir = std::env::temp_dir().join(format!(
                "nzbget-artwriter-direct-unrelated-{}-{}",
                std::process::id(),
                2
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let existing_path = dir.join("part151.rar");
            std::fs::write(&existing_path, b"unrelated existing archive bytes").unwrap();
            let temp_out_path = dir.join("42.out.tmp");
            std::fs::write(&temp_out_path, b"newly downloaded archive bytes").unwrap();

            #[cfg(unix)]
            let dir_bytes = dir.as_os_str().as_bytes();
            #[cfg(not(unix))]
            let dir_bytes = dir.to_string_lossy().as_bytes().to_vec();
            #[cfg(unix)]
            let existing_bytes = existing_path.as_os_str().as_bytes();
            #[cfg(not(unix))]
            let existing_bytes = existing_path.to_string_lossy().as_bytes().to_vec();
            #[cfg(unix)]
            let temp_out_bytes = temp_out_path.as_os_str().as_bytes();
            #[cfg(not(unix))]
            let temp_out_bytes = temp_out_path.to_string_lossy().as_bytes().to_vec();

            let built = article_writer_build_output_filenames(
                b"/tmp",
                dir_bytes,
                crate::paths::PATH_SEPARATOR,
                42,
                1,
                true,
                false,
                b"",
                true,
                b"part151.rar",
                |p| fs_stat(p) == 1,
            )
            .unwrap();
            assert_eq!(built.output_filename.as_deref(), Some(temp_out_bytes));
            assert!(built.update_file_info_output_filename);

            let mut opened_handle: Option<std::fs::File> = None;
            let setup = article_writer_execute_setup_output_file(
                dir_bytes,
                b"part151.rar",
                crate::paths::PATH_SEPARATOR,
                false,
                true,
                false,
                built.output_filename.as_deref().unwrap(),
                |a, b| a == b,
                |d, f| {
                    #[cfg(unix)]
                    {
                        let dc = CString::new(d).unwrap();
                        let fc = CString::new(f).unwrap();
                        crate::fsops::make_unique_filename(&dc, &fc, &fs_stat)
                    }
                    #[cfg(not(unix))]
                    {
                        [d, &[crate::paths::PATH_SEPARATOR], b"part151.duplicate1.rar"].concat()
                    }
                },
                |p| fs_stat(p) == 1,
                |p| {
                    #[cfg(unix)]
                    let path = Path::new(std::ffi::OsStr::from_bytes(p));
                    #[cfg(not(unix))]
                    let path = Path::new(std::str::from_utf8(p).unwrap_or(""));
                    std::fs::create_dir_all(path).is_ok()
                },
                |p| {
                    #[cfg(unix)]
                    let path = Path::new(std::ffi::OsStr::from_bytes(p));
                    #[cfg(not(unix))]
                    let path = Path::new(std::str::from_utf8(p).unwrap_or(""));
                    std::fs::remove_file(path).is_ok()
                },
                |p, read_write| {
                    #[cfg(unix)]
                    let path = Path::new(std::ffi::OsStr::from_bytes(p));
                    #[cfg(not(unix))]
                    let path = Path::new(std::str::from_utf8(p).unwrap_or(""));
                    let res = if read_write {
                        std::fs::OpenOptions::new().read(true).write(true).open(path)
                    } else {
                        std::fs::OpenOptions::new()
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .open(path)
                    };
                    opened_handle = res.ok();
                    opened_handle.is_some()
                },
                |p| {
                    #[cfg(unix)]
                    let path = Path::new(std::ffi::OsStr::from_bytes(p));
                    #[cfg(not(unix))]
                    let path = Path::new(std::str::from_utf8(p).unwrap_or(""));
                    std::fs::create_dir(path).is_ok()
                },
            );
            assert!(setup.ok);
            drop(opened_handle);
            assert_ne!(setup.final_path, existing_bytes);
            assert!(setup.final_path.ends_with(b"part151.duplicate1.rar"));
            assert_eq!(setup.temp_path, temp_out_bytes);
            assert_eq!(
                std::fs::read(&existing_path).unwrap(),
                b"unrelated existing archive bytes"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
