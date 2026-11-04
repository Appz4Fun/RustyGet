//! DupeSearch, DiskState, DownloadInfo, ArticleDownloader, Connection &
//! WebDownloader hot-path helpers (Batch 38).

/// In `DupeSearch::Search`, computes `(parallel, count)` for a posting-fetch batch.
#[inline]
pub fn dds_dupesearch_parallel_fetch_count(
    max_donors: i32,
    fetch_parallel: usize,
    remaining: usize,
) -> (usize, usize) {
    let parallel = if max_donors > 0 { 1 } else { fetch_parallel };
    let count = parallel.min(remaining);
    (parallel, count)
}

/// In `DupeSearch::Place` (`slotsUsed`), evaluates whether a placed entry occupies a donor slot.
#[inline]
pub fn dds_dupesearch_slot_counts_as_used(
    is_member: bool,
    is_valid: bool,
    score: i32,
    dead_score: i32,
    id: i32,
    dry_run: bool,
    placement_available: bool,
) -> bool {
    !is_member
        && is_valid
        && score > dead_score
        && if id < 0 {
            dry_run
        } else {
            placement_available
        }
}

/// In `DupeSearch::Place`, returns `true` when the donor cap is enabled and reached.
#[inline]
pub fn dds_dupesearch_should_defer_checked_donor(cap: i32, slots_used: i32) -> bool {
    cap > 0 && slots_used >= cap
}

/// In `DupeSearch::Place`, returns the initial placeholder score for a measured history member.
#[inline]
pub fn dds_dupesearch_member_initial_score(is_dead: bool, dead_score: i32) -> i32 {
    if is_dead {
        dead_score
    } else {
        dead_score.saturating_add(1)
    }
}

/// In `DupeSearch::Place`, evaluates whether a placed entry qualifies as a usable whole backup.
#[inline]
pub fn dds_dupesearch_is_usable_whole_backup(
    is_valid: bool,
    score: i32,
    dead_score: i32,
    alive: f64,
    id: i32,
    dry_run: bool,
    placement_available: bool,
) -> bool {
    is_valid
        && score > dead_score
        && alive >= 0.999
        && if id < 0 {
            dry_run
        } else {
            placement_available
        }
}

/// In `DupeSearch::Place`, evaluates `(retry_wanted, retry_discovery, rank_incomplete)` after placement.
#[inline]
pub fn dds_dupesearch_eval_post_place_retry(
    rank_only: bool,
    state_retryable: bool,
    usable_backup: bool,
    unchecked_members_len: usize,
) -> (bool, bool, bool) {
    let retry_wanted = if rank_only { state_retryable } else { true };
    let retry_discovery = retry_wanted && !usable_backup;
    let rank_incomplete = rank_only && unchecked_members_len > 0;
    (retry_wanted, retry_discovery, rank_incomplete)
}

/// In `DupeSearch::AddDonor`, returns `true` when `.nzb` should be appended to a title.
#[inline]
pub fn dds_dupesearch_needs_nzb_ext(name: &[u8]) -> bool {
    name.len() < 4 || !name[name.len() - 4..].eq_ignore_ascii_case(b".nzb")
}

/// In `DupeSearch::ResumePending`, classifies the persisted `phase` file contents into `(fetched, ranking)`.
#[inline]
pub fn dds_dupesearch_classify_pending_phase(phase: &[u8]) -> (bool, bool) {
    (phase == b"fetched\n", phase == b"ranking\n")
}

/// In `DiskState::LoadQueue` and `DiskState::LoadHistory`, returns `true` when the dupe trailer should be loaded.
#[inline]
pub fn dds_diskstate_should_load_dupe_trailer(
    format_version: i32,
    min_version: i32,
    max_version: i32,
) -> bool {
    format_version >= min_version && format_version <= max_version
}

/// In `DiskState::LoadNzbInfo`, returns `true` when inline dupe extras (formats 65 and 66) should be loaded.
#[inline]
pub fn dds_diskstate_should_load_inline_dupe_extras(format_version: i32) -> bool {
    format_version == 65 || format_version == 66
}

/// In `DiskState::DiscardFiles`, returns `true` when a completed file's disk state should be discarded.
#[inline]
pub fn dds_diskstate_should_discard_completed_file(status: i32, cf_success: i32) -> bool {
    status != cf_success
}

/// In `DiskState::SaveHistory`, `DiskState::LoadHistory`, and `DiskState::CleanupQueueDir`,
/// returns `true` when a `HistoryInfo` record carries an `NzbInfo`.
#[inline]
pub fn dds_diskstate_history_has_nzb_info(kind: i32, hk_nzb: i32, hk_url: i32) -> bool {
    kind == hk_nzb || kind == hk_url
}

/// In `DiskState::LoadAllFileStates`, returns `true` when an older branch file-state version should be rewritten.
#[inline]
pub fn dds_diskstate_should_save_updated_file_state_version(
    last_version: i32,
    current_version: i32,
) -> bool {
    last_version > current_version
}

/// In `DiskState::LoadAllFileStates`, selects `psCompleted` (`'c'`) vs `psPartial` (`'s'`).
#[inline]
pub fn dds_diskstate_select_partial_state(
    suffix: u8,
    ps_completed: i32,
    ps_partial: i32,
) -> i32 {
    if suffix == b'c' {
        ps_completed
    } else {
        ps_partial
    }
}

/// In `DiskState::LoadVolumeStat`, validates the `articlesPerDays` array size against `max_size`.
#[inline]
pub fn dds_diskstate_validate_articles_per_day_count(arr_size: u32, max_size: u32) -> bool {
    arr_size <= max_size
}

/// In `NzbInfo::AddMessage`, returns `true` when the message deque exceeds `(uint32)LogBuffer`.
#[inline]
pub fn dds_dl_info_should_trim_message_buffer(
    messages_len: usize,
    log_buffer_limit: i32,
) -> bool {
    messages_len > (log_buffer_limit as u32 as usize)
}

/// In `FileInfo::SetActiveDownloads`, returns `1` (create mutex), `2` (reset mutex), or `0` (no change).
#[inline]
pub fn dds_file_info_eval_output_mutex_action(active_downloads: i32, has_mutex: bool) -> i32 {
    if active_downloads > 0 && !has_mutex {
        1
    } else if active_downloads == 0 {
        2
    } else {
        0
    }
}

/// In `ArticleDownloader::Download`, returns `true` when the NNTP `GROUP` command loop should run.
#[inline]
pub fn dds_article_dl_should_join_group(join_group_opt: bool, groups_empty: bool) -> bool {
    join_group_opt && !groups_empty
}

/// In `Connection::WriteLine` and `Connection::Send`, returns `true` when `send()` result marks the socket broken.
#[inline]
pub fn dds_conn_write_line_marks_broken(send_res: i32) -> bool {
    send_res <= 0
}

/// In `WebDownloader::DownloadWithRedirects`, evaluates `(should_continue, too_many_redirects)`.
#[inline]
pub fn dds_webdl_redirect_loop_step(
    status: i32,
    ad_redirect: i32,
    max_redirects: i32,
) -> (bool, bool) {
    (
        status == ad_redirect && max_redirects >= 0,
        status == ad_redirect && max_redirects < 0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dupesearch_parallel_and_slots() {
        assert_eq!(dds_dupesearch_parallel_fetch_count(0, 4, 10), (4, 4));
        assert_eq!(dds_dupesearch_parallel_fetch_count(0, 4, 2), (4, 2));
        assert_eq!(dds_dupesearch_parallel_fetch_count(2, 4, 10), (1, 1));

        assert!(dds_dupesearch_slot_counts_as_used(
            false, true, 10, -1000, 5, false, true
        ));
        assert!(!dds_dupesearch_slot_counts_as_used(
            true, true, 10, -1000, 5, false, true
        ));
        assert!(dds_dupesearch_slot_counts_as_used(
            false, true, 10, -1000, -1, true, false
        ));
        assert!(!dds_dupesearch_slot_counts_as_used(
            false, true, -1000, -1000, 5, false, true
        ));

        assert!(dds_dupesearch_should_defer_checked_donor(2, 2));
        assert!(!dds_dupesearch_should_defer_checked_donor(2, 1));
        assert!(!dds_dupesearch_should_defer_checked_donor(0, 0));

        assert_eq!(dds_dupesearch_member_initial_score(true, -1000), -1000);
        assert_eq!(dds_dupesearch_member_initial_score(false, -1000), -999);
    }

    #[test]
    fn test_dupesearch_backup_retry_and_pending() {
        assert!(dds_dupesearch_is_usable_whole_backup(
            true, 10, -1000, 0.999, 5, false, true
        ));
        assert!(!dds_dupesearch_is_usable_whole_backup(
            true, 10, -1000, 0.998, 5, false, true
        ));
        assert!(dds_dupesearch_is_usable_whole_backup(
            true, 10, -1000, 1.0, -1, true, false
        ));

        assert_eq!(
            dds_dupesearch_eval_post_place_retry(false, false, false, 0),
            (true, true, false)
        );
        assert_eq!(
            dds_dupesearch_eval_post_place_retry(true, true, true, 2),
            (true, false, true)
        );
        assert_eq!(
            dds_dupesearch_eval_post_place_retry(true, false, false, 0),
            (false, false, false)
        );

        assert!(dds_dupesearch_needs_nzb_ext(b"Release.Title"));
        assert!(!dds_dupesearch_needs_nzb_ext(b"Release.Title.nzb"));
        assert!(!dds_dupesearch_needs_nzb_ext(b"Release.Title.NZB"));
        assert!(dds_dupesearch_needs_nzb_ext(b"nzb"));

        assert_eq!(
            dds_dupesearch_classify_pending_phase(b"fetched\n"),
            (true, false)
        );
        assert_eq!(
            dds_dupesearch_classify_pending_phase(b"ranking\n"),
            (false, true)
        );
        assert_eq!(
            dds_dupesearch_classify_pending_phase(b"searching\n"),
            (false, false)
        );
    }

    #[test]
    fn test_diskstate_helpers() {
        assert!(dds_diskstate_should_load_dupe_trailer(67, 57, 67));
        assert!(!dds_diskstate_should_load_dupe_trailer(56, 57, 67));
        assert!(!dds_diskstate_should_load_dupe_trailer(68, 57, 67));

        assert!(dds_diskstate_should_load_inline_dupe_extras(65));
        assert!(dds_diskstate_should_load_inline_dupe_extras(66));
        assert!(!dds_diskstate_should_load_inline_dupe_extras(67));

        assert!(dds_diskstate_should_discard_completed_file(2, 1));
        assert!(!dds_diskstate_should_discard_completed_file(1, 1));

        assert!(dds_diskstate_history_has_nzb_info(1, 1, 3));
        assert!(dds_diskstate_history_has_nzb_info(3, 1, 3));
        assert!(!dds_diskstate_history_has_nzb_info(2, 1, 3));

        assert!(dds_diskstate_should_save_updated_file_state_version(9, 7));
        assert!(!dds_diskstate_should_save_updated_file_state_version(7, 7));

        assert_eq!(dds_diskstate_select_partial_state(b'c', 2, 1), 2);
        assert_eq!(dds_diskstate_select_partial_state(b's', 2, 1), 1);

        assert!(dds_diskstate_validate_articles_per_day_count(100, 100000));
        assert!(!dds_diskstate_validate_articles_per_day_count(100001, 100000));
    }

    #[test]
    fn test_dlinfo_articledl_conn_webdl() {
        assert!(dds_dl_info_should_trim_message_buffer(1001, 1000));
        assert!(!dds_dl_info_should_trim_message_buffer(1000, 1000));

        assert_eq!(dds_file_info_eval_output_mutex_action(1, false), 1);
        assert_eq!(dds_file_info_eval_output_mutex_action(1, true), 0);
        assert_eq!(dds_file_info_eval_output_mutex_action(0, true), 2);
        assert_eq!(dds_file_info_eval_output_mutex_action(0, false), 2);

        assert!(dds_article_dl_should_join_group(true, false));
        assert!(!dds_article_dl_should_join_group(true, true));
        assert!(!dds_article_dl_should_join_group(false, false));

        assert!(dds_conn_write_line_marks_broken(0));
        assert!(dds_conn_write_line_marks_broken(-1));
        assert!(!dds_conn_write_line_marks_broken(12));

        assert_eq!(dds_webdl_redirect_loop_step(3, 3, 5), (true, false));
        assert_eq!(dds_webdl_redirect_loop_step(3, 3, 0), (true, false));
        assert_eq!(dds_webdl_redirect_loop_step(3, 3, -1), (false, true));
        assert_eq!(dds_webdl_redirect_loop_step(1, 3, -1), (false, false));
    }
}
