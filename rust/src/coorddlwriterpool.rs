//! Hot-path Rust kernels for ArticleDownloader, ArticleWriter,
//! ArticleCache, MemoryMonitor, and ServerPool (`daemon/nntp/ArticleDownloader.cpp`,
//! `daemon/nntp/ArticleWriter.cpp`, `daemon/nntp/ServerPool.cpp`).

/// Evaluates content-rejected handling in `ArticleDownloader::Run`.
/// Returns `(handled, should_break, next_status, next_content_rejected, next_remained_retries)`.
#[inline]
pub fn article_dl_handle_content_rejected(
    content_rejected: i32,
    dupe_fallback_round: i32,
    cur_status: i32,
    ad_failed: i32,
    remained_retries: i32,
) -> (bool, bool, i32, bool, i32) {
    if content_rejected != 0 {
        if dupe_fallback_round > 0 {
            (true, true, ad_failed, true, remained_retries)
        } else {
            (true, false, ad_failed, false, 0)
        }
    } else {
        (false, false, cur_status, false, remained_retries)
    }
}

/// Computes the line buffer capacity (`read_chunk + 128`) in
/// `ArticleDownloader::Download`.
#[inline]
pub fn article_dl_calc_line_buf_capacity(read_chunk: i32) -> i32 {
    read_chunk.wrapping_add(128)
}

/// Classifies a write failure in `ArticleDownloader::Download` into
/// `(status, content_rejected)`.
#[inline]
pub fn article_dl_classify_write_failure(
    local_write_error: i32,
    ad_fatal_error: i32,
    ad_failed: i32,
) -> (i32, bool) {
    if local_write_error != 0 {
        (ad_fatal_error, false)
    } else {
        (ad_failed, true)
    }
}

/// Selects the effective article size in `ArticleWriter::Start`.
#[inline]
pub fn article_writer_select_article_size(article_size: i32, info_size: i32) -> i32 {
    if article_size != 0 {
        article_size
    } else {
        info_size
    }
}

/// Evaluates whether direct write is enabled for the current article in
/// `ArticleWriter::Start` and `ArticleWriter::Finish`.
#[inline]
pub fn article_writer_is_direct_write_enabled(
    opt_direct_write: i32,
    force_direct_write: i32,
    format: i32,
    ef_yenc: i32,
    is_dupe_fallback: i32,
) -> i32 {
    if (opt_direct_write != 0 || force_direct_write != 0)
        && format == ef_yenc
        && is_dupe_fallback == 0
    {
        1
    } else {
        0
    }
}

/// Evaluates `ArticleWriter::GetSkipDiskWrite`.
#[inline]
pub fn article_writer_should_skip_disk_write(
    has_file_and_nzb: i32,
    nzb_skip_disk_write: i32,
    opt_skip_write: i32,
) -> i32 {
    if (has_file_and_nzb != 0 && nzb_skip_disk_write != 0) || opt_skip_write != 0 {
        1
    } else {
        0
    }
}

/// Computes the gap-fill write chunk size in `ArticleWriter::ProcessArticles`.
#[inline]
pub fn article_writer_calc_gap_fill_chunk(
    segment_offset: i64,
    position: i64,
    buffer_size: i64,
) -> i64 {
    segment_offset.wrapping_sub(position).min(buffer_size)
}

/// Computes the signed byte delta when reallocating a cached segment in
/// `ArticleCache::Realloc`.
#[inline]
pub fn article_cache_calc_realloc_delta(new_size: i32, old_size: i32) -> isize {
    new_size.wrapping_sub(old_size) as isize
}

/// Clamps the memory monitor sleep interval (`max(50, interval)`) in
/// `MemoryMonitor::Run`.
#[inline]
pub fn memory_monitor_clamp_interval(interval: i32) -> i32 {
    if interval < 50 {
        50
    } else {
        interval
    }
}

/// Computes how many new pooled connections must be created for an active server in
/// `ServerPool::InitConnections`.
#[inline]
pub fn server_pool_calc_missing_connections(
    existing_connections: i32,
    max_connections: i32,
) -> i32 {
    if max_connections > existing_connections {
        max_connections.wrapping_sub(existing_connections)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dl_writer_pool() {
        assert_eq!(
            article_dl_handle_content_rejected(1, 2, 0, 3, 5),
            (true, true, 3, true, 5)
        );
        assert_eq!(
            article_dl_handle_content_rejected(1, 0, 0, 3, 5),
            (true, false, 3, false, 0)
        );
        assert_eq!(article_dl_calc_line_buf_capacity(4096), 4224);
        assert_eq!(article_dl_classify_write_failure(1, 9, 3), (9, false));
        assert_eq!(article_dl_classify_write_failure(0, 9, 3), (3, true));
        assert_eq!(article_writer_select_article_size(0, 750), 750);
        assert_eq!(article_writer_select_article_size(700, 750), 700);
        assert_eq!(article_writer_is_direct_write_enabled(1, 0, 1, 1, 0), 1);
        assert_eq!(article_writer_is_direct_write_enabled(1, 0, 1, 1, 1), 0);
        assert_eq!(article_writer_should_skip_disk_write(1, 1, 0), 1);
        assert_eq!(article_writer_should_skip_disk_write(1, 0, 0), 0);
        assert_eq!(article_writer_calc_gap_fill_chunk(1000, 200, 512), 512);
        assert_eq!(article_writer_calc_gap_fill_chunk(500, 200, 512), 300);
        assert_eq!(article_cache_calc_realloc_delta(800, 1000), -200);
        assert_eq!(memory_monitor_clamp_interval(10), 50);
        assert_eq!(memory_monitor_clamp_interval(250), 250);
        assert_eq!(server_pool_calc_missing_connections(2, 8), 6);
        assert_eq!(server_pool_calc_missing_connections(8, 8), 0);
    }
}
