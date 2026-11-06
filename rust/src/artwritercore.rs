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
