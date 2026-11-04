//! Hot-path Rust kernels for QueueCoordinator, ArticleDownloader, ArticleWriter,
//! ArticleCache, MemoryMonitor, and ServerPool (`daemon/queue/QueueCoordinator.cpp`,
//! `daemon/nntp/ArticleDownloader.cpp`, `daemon/nntp/ArticleWriter.cpp`,
//! `daemon/nntp/ServerPool.cpp`).

/// Evaluates whether all files in an NZB collection are paused
/// (`file_list_size - paused_file_count <= 0`) in `QueueCoordinator::GetNextArticle`.
#[inline]
pub fn queue_coord_is_nzb_paused(file_list_size: i32, paused_file_count: i32) -> i32 {
    if file_list_size.wrapping_sub(paused_file_count) <= 0 {
        1
    } else {
        0
    }
}

/// Evaluates whether `QueueCoordinator::GetNextArticle` should proactively lead
/// with a duplicate article when `dupe_cutover != 0` and `dupe_fallback_round == 0`.
#[inline]
pub fn queue_coord_should_try_proactive_cutover(
    dupe_cutover: i32,
    dupe_fallback_round: i32,
) -> i32 {
    if dupe_cutover != 0 && dupe_fallback_round == 0 {
        1
    } else {
        0
    }
}

/// Selects the duplicate donor ID for a completed article in
/// `QueueCoordinator::ArticleCompleted`.
#[inline]
pub fn queue_coord_select_dupe_donor_id(
    dupe_fallback_round: i32,
    donors: &[i32],
    msg_id: &[u8],
    orig_msg_id: &[u8],
) -> i32 {
    let source = dupe_fallback_round.wrapping_sub(1);
    if source >= 0 && (source as usize) < donors.len() && msg_id != orig_msg_id {
        donors[source as usize]
    } else {
        0
    }
}

/// Updates the file and NZB failed-article counters in
/// `QueueCoordinator::ArticleCompleted`.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn queue_coord_apply_failed_article_counters(
    part_number: i32,
    par_file: i32,
    article_size: i32,
    dupe_failed_run: i32,
    file_failed_size: i64,
    nzb_cur_failed_size: i64,
    nzb_par_cur_failed_size: i64,
    file_failed_articles: i32,
    nzb_cur_failed_articles: i32,
) -> (i32, i64, i64, i64, i32, i32) {
    let next_dupe_failed_run = if part_number != 1 && par_file == 0 {
        dupe_failed_run.wrapping_add(1)
    } else {
        dupe_failed_run
    };
    let size64 = article_size as i64;
    let next_file_failed_size = file_failed_size.wrapping_add(size64);
    let next_nzb_cur_failed_size = nzb_cur_failed_size.wrapping_add(size64);
    let next_nzb_par_cur_failed_size =
        nzb_par_cur_failed_size.wrapping_add(if par_file != 0 { size64 } else { 0 });
    let next_file_failed_articles = file_failed_articles.wrapping_add(1);
    let next_nzb_cur_failed_articles = nzb_cur_failed_articles.wrapping_add(1);
    (
        next_dupe_failed_run,
        next_file_failed_size,
        next_nzb_cur_failed_size,
        next_nzb_par_cur_failed_size,
        next_file_failed_articles,
        next_nzb_cur_failed_articles,
    )
}

/// Evaluates `(delete_data, save_partial)` flags for `DiskState` operations in
/// `QueueCoordinator::DeleteFileInfo`.
#[inline]
pub fn queue_coord_eval_delete_file_diskstate_flags(
    file_status: i32,
    cf_success: i32,
    cf_partial: i32,
    file_deleted: i32,
    parking: i32,
    completed: i32,
) -> (bool, bool) {
    let delete_data = file_status == cf_success || (file_deleted != 0 && parking == 0);
    let save_partial = file_status == cf_partial && (completed != 0 || parking != 0);
    (delete_data, save_partial)
}

/// Evaluates whether `QueueCoordinator::DeleteFileInfo` should start a live stream
/// repair pass.
#[inline]
pub fn queue_coord_should_start_live_stream_repair(
    dupe_fallback_mode: i32,
    daf_live: i32,
    more_files_queued: i32,
    has_live_repair_thread: i32,
) -> i32 {
    if dupe_fallback_mode == daf_live && more_files_queued != 0 && has_live_repair_thread == 0 {
        1
    } else {
        0
    }
}

/// Evaluates whether an adjacent article is a finished donor segment that must be
/// checked for alignment in `QueueCoordinator::DemoteMisalignedDupeNeighbors`.
#[inline]
pub fn queue_coord_is_neighbor_eligible_for_demotion(
    status: i32,
    ai_finished: i32,
    dupe_fallback_round: i32,
    segment_size: i32,
) -> i32 {
    if status == ai_finished && dupe_fallback_round != 0 && segment_size > 0 {
        1
    } else {
        0
    }
}

/// Evaluates whether a file has at least one tried article
/// (`success_articles + failed_articles > 0`) in `QueueCoordinator::FilesTried`.
#[inline]
pub fn queue_coord_is_file_tried(success_articles: i32, failed_articles: i32) -> i32 {
    if success_articles.wrapping_add(failed_articles) > 0 {
        1
    } else {
        0
    }
}

/// Evaluates whether a file is eligible for sampling in a given pass of
/// `QueueCoordinator::StartDeadPickProbe`.
#[inline]
pub fn queue_coord_is_dead_pick_probe_file_eligible(
    deleted: i32,
    paused: i32,
    total_articles: i32,
    pass: i32,
    par_file: i32,
) -> i32 {
    if deleted == 0 && paused == 0 && total_articles > 0 && (pass == 1 || par_file == 0) {
        1
    } else {
        0
    }
}

/// Selects the merged `DownloadStartTime` in `QueueCoordinator::MergeQueueEntries`.
#[inline]
pub fn queue_coord_select_merged_download_start_time(dest_start: i64, src_start: i64) -> i64 {
    if (dest_start > 0 && dest_start < src_start) || src_start == 0 {
        dest_start
    } else {
        src_start
    }
}

/// Computes the updated NZB and file counters when discarding downloaded articles
/// in `QueueCoordinator::DiscardDownloadedArticles`.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn queue_coord_discard_downloaded_articles_counters(
    nzb_rem_size: i64,
    nzb_paused_size: i64,
    nzb_cur_succ_art: i32,
    nzb_cur_succ_size: i64,
    nzb_par_cur_succ_size: i64,
    nzb_cur_fail_art: i32,
    nzb_cur_fail_size: i64,
    nzb_par_cur_fail_size: i64,
    file_paused: i32,
    file_succ_art: i32,
    file_succ_size: i64,
    file_fail_art: i32,
    file_fail_size: i64,
    file_size: i64,
    file_missed_size: i64,
) -> (i64, i64, i32, i64, i64, i32, i64, i64, i64) {
    let delta_size = file_succ_size.wrapping_add(file_fail_size);
    let next_nzb_rem_size = nzb_rem_size.wrapping_add(delta_size);
    let next_nzb_paused_size = if file_paused != 0 {
        nzb_paused_size.wrapping_add(delta_size)
    } else {
        nzb_paused_size
    };
    let next_nzb_cur_succ_art = nzb_cur_succ_art.wrapping_sub(file_succ_art);
    let next_nzb_cur_succ_size = nzb_cur_succ_size.wrapping_sub(file_succ_size);
    let next_nzb_par_cur_succ_size = nzb_par_cur_succ_size.wrapping_sub(file_succ_size);
    let next_nzb_cur_fail_art = nzb_cur_fail_art.wrapping_sub(file_fail_art);
    let next_nzb_cur_fail_size = nzb_cur_fail_size.wrapping_sub(file_fail_size);
    let next_nzb_par_cur_fail_size = nzb_par_cur_fail_size.wrapping_sub(file_fail_size);
    let next_file_rem_size = file_size.wrapping_sub(file_missed_size);
    (
        next_nzb_rem_size,
        next_nzb_paused_size,
        next_nzb_cur_succ_art,
        next_nzb_cur_succ_size,
        next_nzb_par_cur_succ_size,
        next_nzb_cur_fail_art,
        next_nzb_cur_fail_size,
        next_nzb_par_cur_fail_size,
        next_file_rem_size,
    )
}

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
    fn test_queue_coord_helpers() {
        assert_eq!(queue_coord_is_nzb_paused(5, 5), 1);
        assert_eq!(queue_coord_is_nzb_paused(5, 4), 0);
        assert_eq!(queue_coord_should_try_proactive_cutover(1, 0), 1);
        assert_eq!(queue_coord_should_try_proactive_cutover(1, 1), 0);
        assert_eq!(queue_coord_should_try_proactive_cutover(0, 0), 0);

        let donors = [11, 22, 33];
        assert_eq!(
            queue_coord_select_dupe_donor_id(2, &donors, b"msg2@news", b"msg1@news"),
            22
        );
        assert_eq!(
            queue_coord_select_dupe_donor_id(2, &donors, b"msg1@news", b"msg1@news"),
            0
        );
        assert_eq!(
            queue_coord_select_dupe_donor_id(0, &donors, b"msg2@news", b"msg1@news"),
            0
        );

        let (run, f_fs, n_cfs, n_pcfs, f_fa, n_cfa) =
            queue_coord_apply_failed_article_counters(2, 0, 500, 3, 1000, 2000, 300, 1, 4);
        assert_eq!(
            (run, f_fs, n_cfs, n_pcfs, f_fa, n_cfa),
            (4, 1500, 2500, 300, 2, 5)
        );
        let (run2, _, _, n_pcfs2, _, _) =
            queue_coord_apply_failed_article_counters(2, 1, 500, 3, 1000, 2000, 300, 1, 4);
        assert_eq!((run2, n_pcfs2), (3, 800));

        assert_eq!(
            queue_coord_eval_delete_file_diskstate_flags(3, 3, 2, 0, 0, 1),
            (true, false)
        );
        assert_eq!(
            queue_coord_eval_delete_file_diskstate_flags(2, 3, 2, 1, 0, 1),
            (true, true)
        );
        assert_eq!(queue_coord_should_start_live_stream_repair(2, 2, 1, 0), 1);
        assert_eq!(queue_coord_should_start_live_stream_repair(2, 2, 1, 1), 0);
        assert_eq!(queue_coord_is_neighbor_eligible_for_demotion(2, 2, 1, 100), 1);
        assert_eq!(queue_coord_is_neighbor_eligible_for_demotion(2, 2, 0, 100), 0);
        assert_eq!(queue_coord_is_file_tried(0, 0), 0);
        assert_eq!(queue_coord_is_file_tried(0, 1), 1);
        assert_eq!(queue_coord_is_dead_pick_probe_file_eligible(0, 0, 10, 0, 0), 1);
        assert_eq!(queue_coord_is_dead_pick_probe_file_eligible(0, 0, 10, 0, 1), 0);
        assert_eq!(queue_coord_is_dead_pick_probe_file_eligible(0, 0, 10, 1, 1), 1);
        assert_eq!(queue_coord_select_merged_download_start_time(100, 200), 100);
        assert_eq!(queue_coord_select_merged_download_start_time(0, 200), 200);
        assert_eq!(queue_coord_select_merged_download_start_time(100, 0), 100);
    }

    #[test]
    fn test_discard_downloaded_articles_and_dl_writer_pool() {
        let res = queue_coord_discard_downloaded_articles_counters(
            1000, 500, 10, 2000, 800, 4, 400, 200, 1, 3, 300, 1, 100, 5000, 200,
        );
        assert_eq!(res, (1400, 900, 7, 1700, 500, 3, 300, 100, 4800));

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
