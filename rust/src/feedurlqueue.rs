// This file is part of nzbget. See <https://nzbget.com>.
//
// Copyright (C) 2024-2026nzbget community
//
// This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Rust hot-path helpers for `FeedCoordinator`, `UrlCoordinator`, `QueueCoordinator`,
//! `Options`, `PrePostProcessor`, `RepairController`, and `DirectUnpack`.

/// `FeedCoordinator`: whether at least 60 seconds have elapsed since the last history cleanup.
pub fn feed_coord_should_cleanup_history(cur_time: i64, last_cleanup: i64) -> bool {
    (cur_time.wrapping_sub(last_cleanup)).abs() >= 60
}

/// `FeedCoordinator`: wait interval (ms) for the feed loop.
pub fn feed_coord_calc_wait_interval(url_force: bool, feeds_empty: bool) -> i32 {
    if url_force || feeds_empty {
        60000
    } else {
        5000
    }
}

/// `FeedCoordinator`: whether an active feed download has exceeded its timeout.
pub fn feed_coord_is_hanging_download(
    cur_time: i64,
    last_update: i64,
    url_timeout: i32,
    status: i32,
    ad_running: i32,
) -> bool {
    cur_time.wrapping_sub(last_update) > i64::from(url_timeout) + 10 && status == ad_running
}

/// `FeedCoordinator`: selects the effective category for a feed item.
pub fn feed_coord_resolve_category<'a>(
    feed_cat: &'a [u8],
    cat_source: i32,
    cs_nzb_file: i32,
    cs_auto: i32,
    item_cat: &'a [u8],
) -> &'a [u8] {
    if !feed_cat.is_empty() {
        return feed_cat;
    }
    if cat_source == cs_nzb_file || cat_source == cs_auto {
        if !item_cat.is_empty() {
            item_cat
        } else {
            b""
        }
    } else {
        b""
    }
}

/// `FeedCoordinator`: classifies an accepted feed item against history and backlog settings.
pub fn feed_coord_classify_item_history(
    first_fetch: bool,
    backlog: bool,
    has_history: bool,
    hs_unknown: i32,
    hs_backlog: i32,
    hs_fetched: i32,
) -> (i32, bool) {
    let mut status = hs_unknown;
    if first_fetch && backlog {
        status = hs_backlog;
    } else if !has_history {
        status = hs_fetched;
    }
    let should_add = status == hs_fetched;
    (status, should_add)
}

/// `FeedCoordinator`: computes the history retention border date.
pub fn feed_coord_calc_history_border_date(oldest_update: i64, feed_history_days: i32) -> i64 {
    oldest_update.wrapping_sub(i64::from(feed_history_days).wrapping_mul(60 * 60 * 24))
}

/// `UrlCoordinator`: whether a queued URL download can start.
pub fn url_coord_should_start_download(
    has_nzb: bool,
    pause_download: bool,
    url_force: bool,
) -> bool {
    has_nzb && (!pause_download || url_force)
}

/// `UrlCoordinator`: sleep interval (ms) after checking URL downloads.
pub fn url_coord_calc_sleep_interval(download_started: bool) -> i32 {
    if download_started {
        0
    } else {
        100
    }
}

/// `UrlCoordinator`: whether a URL downloader status is terminal for the coordinator callback.
pub fn url_coord_is_completed_status(
    status: i32,
    ad_finished: i32,
    ad_failed: i32,
    ad_retry: i32,
) -> bool {
    status == ad_finished || status == ad_failed || status == ad_retry
}

/// `UrlCoordinator`: maps an NZB scan status to the corresponding URL status.
pub fn url_coord_resolve_scan_fail_status(
    add_status: i32,
    as_failed: i32,
    ls_scan_failed: i32,
    ls_scan_skipped: i32,
) -> i32 {
    if add_status == as_failed {
        ls_scan_failed
    } else {
        ls_scan_skipped
    }
}

/// `QueueCoordinator`: whether the next article download may be started.
pub fn queue_coord_can_start_article(
    has_more: bool,
    can_proceed: bool,
    download_held: bool,
    extra_priority: bool,
) -> bool {
    has_more && can_proceed && (!download_held || extra_priority)
}

/// `QueueCoordinator`: doubles the standby wait interval up to 2000 ms.
pub fn queue_coord_next_standby_wait(wait_interval: i32) -> i32 {
    wait_interval.saturating_mul(2).min(2000)
}

/// `QueueCoordinator`: sleep interval (ms) in the active download loop.
pub fn queue_coord_active_sleep_interval(download_started: bool) -> i32 {
    if download_started {
        0
    } else {
        5
    }
}

/// `QueueCoordinator`: tie-breaking comparison for duplicate file sizes and indices.
pub fn queue_coord_is_smaller_dupe_file(size1: i64, idx1: i32, size2: i64, idx2: i32) -> bool {
    size1 < size2 || (size1 == size2 && idx2 < idx1)
}

/// Identifies indices of files in `filenames` (`0..filenames.len()`) whose filename
/// appears exactly twice and which lose the size/index tie-breaker
/// (`QueueCoordinator::CheckDupeFileInfos` in `daemon/queue/QueueCoordinator.cpp`).
pub fn queue_coord_find_dupe_file_indices(
    filenames: &[&[u8]],
    sizes: &[i64],
    out_dupe_indices: &mut [usize],
) -> usize {
    let n = filenames.len().min(sizes.len());
    let mut by_name: std::collections::HashMap<&[u8], Vec<(i32, usize)>> =
        std::collections::HashMap::with_capacity(n);
    for (idx, &name) in filenames.iter().enumerate().take(n) {
        by_name.entry(name).or_default().push(((idx + 1) as i32, idx));
    }
    let mut written = 0usize;
    for (idx, &name) in filenames.iter().enumerate().take(n) {
        let Some(same) = by_name.get(name) else {
            continue;
        };
        if same.len() != 2 {
            continue;
        }
        let (self_pos, self_idx) = if same[0].1 == idx { same[0] } else { same[1] };
        let (other_pos, other_idx) = if same[0].1 == idx { same[1] } else { same[0] };
        if queue_coord_is_smaller_dupe_file(sizes[self_idx], self_pos, sizes[other_idx], other_pos)
            && written < out_dupe_indices.len()
        {
            out_dupe_indices[written] = idx;
            written += 1;
        }
    }
    written
}

/// `QueueCoordinator`: whether `nzb` has strictly higher priority than `file_nzb`.
pub fn queue_coord_is_higher_nzb_priority(
    has_file: bool,
    nzb_extra: bool,
    file_nzb_extra: bool,
    nzb_prio: i32,
    file_nzb_prio: i32,
) -> bool {
    has_file
        && ((nzb_extra == file_nzb_extra && nzb_prio > file_nzb_prio)
            || (nzb_extra && !file_nzb_extra))
}

/// `QueueCoordinator`: whether the first article should attach a content analyzer.
pub fn queue_coord_should_analyze_first_article(
    part_num: i32,
    raw_article: bool,
    direct_rename: bool,
    dupe_fallback_enabled: bool,
) -> bool {
    part_num == 1 && !raw_article && (direct_rename || dupe_fallback_enabled)
}

/// `QueueCoordinator`: whether an article downloader status triggers `ArticleCompleted`.
pub fn queue_coord_is_article_completed_status(
    status: i32,
    ad_finished: i32,
    ad_failed: i32,
    ad_retry: i32,
    ad_fatal_error: i32,
) -> bool {
    status == ad_finished || status == ad_failed || status == ad_retry || status == ad_fatal_error
}

/// `QueueCoordinator`: whether an article filename should overwrite the NZB filename.
pub fn queue_coord_should_use_article_filename(
    file_naming: i32,
    nf_article: i32,
    nf_auto: i32,
    is_obfuscated: bool,
    many_dupe_files: bool,
) -> bool {
    file_naming == nf_article || (file_naming == nf_auto && !is_obfuscated && !many_dupe_files)
}

/// `QueueCoordinator`: whether a file object should be deleted when a downloader finishes.
pub fn queue_coord_should_delete_file_obj(
    file_completed: bool,
    file_deleted: bool,
    active_downloads: i32,
) -> bool {
    let has_other_downloaders = active_downloads > 1;
    file_completed || (file_deleted && !has_other_downloaders)
}

/// `QueueCoordinator`: whether `CheckHealth` should return early without taking action.
#[allow(clippy::too_many_arguments)]
pub fn queue_coord_should_skip_health_check(
    health_check: i32,
    hc_none: i32,
    hc_park: i32,
    par_scan: i32,
    ps_dupe: i32,
    health_paused: bool,
    delete_status: i32,
    ds_health: i32,
    health: i32,
    critical_health: i32,
    success_articles: i32,
    total_articles: i32,
) -> bool {
    health_check == hc_none
        || health_paused
        || delete_status == ds_health
        || health >= critical_health
        || (par_scan == ps_dupe
            && health_check == hc_park
            && total_articles > 0
            && i64::from(success_articles) * 100 / i64::from(total_articles) > 10)
}

/// `QueueCoordinator`: computes the download-time critical health threshold.
pub fn queue_coord_resolve_download_critical_health(
    critical_est: i32,
    critical_exact: i32,
    health_check: i32,
    hc_dupe: i32,
    certainly_parless: bool,
    has_size_twin: bool,
) -> i32 {
    if health_check != hc_dupe
        || critical_exact != 1000
        || !certainly_parless
        || has_size_twin
    {
        critical_est
    } else {
        1000
    }
}

/// `QueueCoordinator`: whether a queued duplicate entry is considered active.
#[allow(clippy::too_many_arguments)]
pub fn queue_coord_is_queued_dupe_active(
    deleting: bool,
    delete_status: i32,
    ds_none: i32,
    active_downloads: i32,
    has_post_info: bool,
    post_working: bool,
    post_stage: i32,
    pt_queued: i32,
    extra_priority: bool,
    self_completed_empty: bool,
    direct_unpack_status: i32,
    ns_running: i32,
) -> bool {
    if (deleting || delete_status != ds_none) && active_downloads == 0 {
        return false;
    }
    let is_active_download = active_downloads > 0;
    let is_post_processing = has_post_info
        && (post_working || post_stage != pt_queued || extra_priority || self_completed_empty);
    let is_direct_unpacking = direct_unpack_status == ns_running;
    is_active_download || is_post_processing || is_direct_unpacking
}

/// `QueueCoordinator`: whether `FailOverDeadPick` should abort because enough own articles arrived.
pub fn queue_coord_should_abort_dead_pick_failover(
    cur_success: i32,
    dupe_recovered: i32,
    recovered_at_start: i32,
    min_alive_samples: i32,
) -> bool {
    let borrowed = (dupe_recovered - recovered_at_start).max(0);
    cur_success - borrowed >= min_alive_samples
}

/// `Options`: converts `TimeCorrection` (hours if in `[-24, 24]`, else minutes) to seconds.
pub fn options_normalize_time_correction(raw_val: i32) -> i32 {
    let mins = if (-24..=24).contains(&raw_val) {
        raw_val * 60
    } else {
        raw_val
    };
    mins * 60
}

/// `Options`: checks 32-bit memory limits for `ArticleCache` and `ParBuffer`.
/// Returns `(article_cache, par_buffer, err_kind)` where `err_kind`:
/// 0 = ok, 1 = article_cache > 1900, 2 = par_buffer > 1900, 3 = sum > 1900.
pub fn options_clamp_32bit_memory_limits(
    is_32bit: bool,
    article_cache: i32,
    par_buffer: i32,
) -> (i32, i32, i32) {
    let mut ac = article_cache;
    let mut pb = par_buffer;
    let mut err = 0;
    if ac < 0 {
        ac = 0;
    } else if is_32bit && ac > 1900 {
        ac = 1900;
        err = 1;
    } else if is_32bit && pb > 1900 {
        pb = 1900;
        err = 2;
    }
    if is_32bit && pb.saturating_add(ac) > 1900 {
        ac = 1500;
        pb = 400;
        err = 3;
    }
    (ac, pb, err)
}

/// `PrePostProcessor`: whether `CheckDownloadCompleted` should trigger `NzbDownloaded`.
#[allow(clippy::too_many_arguments)]
pub fn prepost_should_complete_after_direct_rename(
    is_stopped: bool,
    direct_rename_status: i32,
    ts_success: i32,
    has_post_info: bool,
    delete_status: i32,
    ds_none: i32,
    completed_not_empty: bool,
    download_completed: bool,
    raw_article: bool,
    skip_write: bool,
    skip_disk_write: bool,
) -> bool {
    !is_stopped
        && direct_rename_status == ts_success
        && !has_post_info
        && delete_status == ds_none
        && completed_not_empty
        && download_completed
        && !raw_article
        && !skip_write
        && !skip_disk_write
}

/// `RepairController`: computes `(wait_time, par_sec)` after par-check finishes.
pub fn repair_ctrl_calc_par_check_times(
    cur_dl_sec: i32,
    start_dl_sec: i32,
    cur_time: i64,
    par_time: i64,
) -> (i32, i32) {
    let wait_time = cur_dl_sec.wrapping_sub(start_dl_sec);
    let par_sec = (cur_time.wrapping_sub(par_time) as i32).wrapping_sub(wait_time);
    (wait_time, par_sec)
}

/// `DirectUnpack`: evaluates `(unpack_ok, interrupted)` after `Execute()` returns.
pub fn direct_unpack_eval_unrar_result(
    exit_code: i32,
    all_ok_msg: bool,
    terminated: bool,
    cancelled: bool,
    stopped: bool,
) -> (bool, bool) {
    let unpack_ok = exit_code == 0 && all_ok_msg && !terminated && !cancelled;
    let interrupted = !cancelled && (stopped || terminated);
    (unpack_ok, interrupted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feed_and_url_coord_helpers() {
        assert!(feed_coord_should_cleanup_history(1060, 1000));
        assert!(!feed_coord_should_cleanup_history(1059, 1000));
        assert_eq!(feed_coord_calc_wait_interval(true, false), 60000);
        assert_eq!(feed_coord_calc_wait_interval(false, false), 5000);
        assert!(feed_coord_is_hanging_download(100, 20, 60, 1, 1));
        assert_eq!(
            feed_coord_resolve_category(b"movies", 1, 0, 1, b"tv"),
            b"movies"
        );
        assert_eq!(
            feed_coord_resolve_category(b"", 1, 0, 1, b"tv"),
            b"tv"
        );
        assert_eq!(
            feed_coord_classify_item_history(true, true, false, 0, 1, 2),
            (1, false)
        );
        assert_eq!(
            feed_coord_calc_history_border_date(100_000, 1),
            100_000 - 86400
        );

        assert!(url_coord_should_start_download(true, false, false));
        assert!(!url_coord_should_start_download(true, true, false));
        assert_eq!(url_coord_calc_sleep_interval(true), 0);
        assert_eq!(url_coord_calc_sleep_interval(false), 100);
        assert!(url_coord_is_completed_status(2, 1, 2, 3));
        assert_eq!(url_coord_resolve_scan_fail_status(1, 1, 10, 20), 10);
    }

    #[test]
    fn test_queue_coord_helpers() {
        assert!(queue_coord_can_start_article(true, true, true, true));
        assert!(!queue_coord_can_start_article(true, true, true, false));
        assert_eq!(queue_coord_next_standby_wait(1500), 2000);
        assert_eq!(queue_coord_active_sleep_interval(false), 5);
        assert!(queue_coord_is_smaller_dupe_file(100, 5, 100, 2));
        assert!(queue_coord_is_higher_nzb_priority(true, true, false, 0, 10));
        assert!(queue_coord_should_analyze_first_article(1, false, true, false));
        assert!(queue_coord_is_article_completed_status(4, 1, 2, 3, 4));
        assert!(queue_coord_should_use_article_filename(1, 2, 1, false, false));
        assert!(queue_coord_should_delete_file_obj(false, true, 1));
        assert!(queue_coord_should_skip_health_check(
            2, 0, 2, 3, 3, false, 0, 1, 800, 900, 15, 100
        ));
        assert_eq!(
            queue_coord_resolve_download_critical_health(850, 1000, 4, 4, true, false),
            1000
        );
        assert!(queue_coord_is_queued_dupe_active(
            false, 0, 0, 1, false, false, 0, 0, false, false, 0, 1
        ));
        assert!(queue_coord_should_abort_dead_pick_failover(5, 3, 1, 2));
    }

    #[test]
    fn test_options_and_post_helpers() {
        assert_eq!(options_normalize_time_correction(2), 7200);
        assert_eq!(options_normalize_time_correction(30), 1800);
        assert_eq!(
            options_clamp_32bit_memory_limits(true, 1200, 800),
            (1500, 400, 3)
        );
        assert!(prepost_should_complete_after_direct_rename(
            false, 2, 2, false, 0, 0, true, true, false, false, false
        ));
        assert_eq!(repair_ctrl_calc_par_check_times(25, 10, 1100, 1000), (15, 85));
        assert_eq!(
            direct_unpack_eval_unrar_result(0, true, false, false, false),
            (true, false)
        );
    }
}
