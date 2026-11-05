//! Hot-path Rust kernels for DupeCoordinator, DupeArticleFallback, DupeStreamRepair,
//! ParDamage, and NzbFile (`daemon/queue/DupeCoordinator.cpp`,
//! `daemon/queue/DupeArticleFallback.cpp`, `daemon/queue/DupeStreamRepair.cpp`,
//! `daemon/queue/ParDamage.cpp`, `daemon/queue/NzbFile.cpp`).

/// Evaluates whether a history item matches `DupeCoordinator::DownloadedOnDisk`
/// (before checking `FilesOnDisk`).
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn dupecoord_is_downloaded_on_disk_candidate(
    is_self: i32,
    delete_status: i32,
    ds_none: i32,
    is_dupe_success: i32,
    mark_status: i32,
    ks_bad: i32,
    item_dupe_key: &[u8],
    nzb_dupe_key: &[u8],
) -> i32 {
    if is_self == 0
        && delete_status == ds_none
        && is_dupe_success != 0
        && mark_status != ks_bad
        && item_dupe_key.eq_ignore_ascii_case(nzb_dupe_key)
    {
        1
    } else {
        0
    }
}

/// Evaluates whether a queue or history item has the same name and non-empty
/// `dupe_key` or non-zero `dupe_score` in `DupeCoordinator::NzbFound`.
#[inline]
pub fn dupecoord_should_inherit_dupe_props(
    cand_name: &[u8],
    nzb_name: &[u8],
    cand_dupe_key: &[u8],
    cand_dupe_score: i32,
) -> i32 {
    if cand_name == nzb_name && (!cand_dupe_key.is_empty() || cand_dupe_score != 0) {
        1
    } else {
        0
    }
}

/// Evaluates whether an incoming NZB in `dmScore` mode should be marked `dsDupe`
/// because a recent history item with the same name/key succeeded with at least as
/// high a score in `DupeCoordinator::NzbFound`.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn dupecoord_is_recent_history_score_duplicate(
    kind: i32,
    hk_nzb: i32,
    hk_url: i32,
    item_dupe_mode: i32,
    dm_force: i32,
    same_name_or_key: i32,
    nzb_dupe_score: i32,
    item_dupe_score: i32,
    is_dupe_success: i32,
) -> i32 {
    if (kind == hk_nzb || kind == hk_url)
        && item_dupe_mode != dm_force
        && same_name_or_key != 0
        && nzb_dupe_score <= item_dupe_score
        && is_dupe_success != 0
    {
        1
    } else {
        0
    }
}

/// Computes the queue iterator index adjustment after `EditEntry(eaGroupDelete)`
/// in `DupeCoordinator::NzbFound`.
#[inline]
pub fn dupecoord_calc_post_delete_index(index: i32, old_size: i32, new_size: i32) -> i32 {
    let step = if old_size == new_size { 1 } else { 0 };
    index.wrapping_add(step)
}

/// Evaluates whether another queued NZB of the same `dupe_key` should be stopped
/// when a copy completes on disk in `DupeCoordinator::NzbCompleted`.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn dupecoord_should_stop_other_queued_dupe(
    is_self: i32,
    kind: i32,
    nk_nzb: i32,
    deleting: i32,
    delete_status: i32,
    ds_none: i32,
    dupe_mode: i32,
    dm_force: i32,
    queued_dupe_key: &[u8],
    nzb_dupe_key: &[u8],
) -> i32 {
    if is_self == 0
        && kind == nk_nzb
        && deleting == 0
        && delete_status == ds_none
        && dupe_mode != dm_force
        && queued_dupe_key.eq_ignore_ascii_case(nzb_dupe_key)
    {
        1
    } else {
        0
    }
}

/// Maps `NzbInfo::EMarkStatus` to `DupInfo::EStatus` in `DupeCoordinator::HistoryMark`.
#[inline]
pub fn dupecoord_map_mark_to_dup_status(
    mark_status: i32,
    ks_good: i32,
    ks_success: i32,
    ds_good: i32,
    ds_success: i32,
    ds_bad: i32,
) -> i32 {
    if mark_status == ks_good {
        ds_good
    } else if mark_status == ks_success {
        ds_success
    } else {
        ds_bad
    }
}

/// Evaluates whether a history item should be hidden during `DupeCoordinator::HistoryCleanup`.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn dupecoord_should_hide_history_dupe(
    kind: i32,
    hk_nzb: i32,
    hk_url: i32,
    dupe_mode: i32,
    dm_force: i32,
    delete_status: i32,
    ds_dupe: i32,
    is_self: i32,
    same_name_or_key: i32,
) -> i32 {
    if (kind == hk_nzb || kind == hk_url)
        && dupe_mode != dm_force
        && delete_status == ds_dupe
        && is_self == 0
        && same_name_or_key != 0
    {
        1
    } else {
        0
    }
}

/// Evaluates whether a `CompletedFile` is a `.par2` file in
/// `DupeArticleFallback::HasPar2` and `DupeArticleFallback::ShouldDeferToPar`.
#[inline]
pub fn dupe_fallback_completed_file_is_par2(par_file: i32, filename: &[u8]) -> i32 {
    if par_file != 0
        || (filename.len() >= 5 && filename[filename.len() - 5..].eq_ignore_ascii_case(b".par2"))
    {
        1
    } else {
        0
    }
}

/// Evaluates whether `DupeArticleFallback::TryFallback` must abort early when `defer`
/// is active and no queue/donors are available.
#[inline]
pub fn dupe_fallback_should_abort_deferred_try(
    defer: i32,
    round: i32,
    has_download_queue: i32,
    source_donors_empty: i32,
) -> i32 {
    if defer != 0 {
        let missing = if round == 0 {
            has_download_queue == 0
        } else {
            source_donors_empty != 0
        };
        if missing {
            return 1;
        }
    }
    0
}

/// Evaluates whether `pinned_round == 0 && !dupe_proactive` in
/// `DupeArticleFallback::TryFallback`.
#[inline]
pub fn dupe_fallback_is_reactive_first_round(pinned_round: i32, dupe_proactive: i32) -> i32 {
    if pinned_round == 0 && dupe_proactive == 0 {
        1
    } else {
        0
    }
}

/// Evaluates whether a `Par2FileSums` entry matches the target file on disk in
/// `DupeArticleFallback::BorrowedPar2Mismatches`.
#[inline]
pub fn dupe_fallback_par2_sums_entry_matches(
    entry_length: u64,
    file_size: i64,
    crcs_empty: i32,
    entry_hash16k: &[u8],
    hash16k: &[u8],
    entry_name: &[u8],
    filename: &[u8],
) -> i32 {
    if (entry_length as i64) == file_size
        && crcs_empty == 0
        && (entry_hash16k.eq_ignore_ascii_case(hash16k) || entry_name == filename)
    {
        1
    } else {
        0
    }
}

/// Evaluates whether an article in the parsed own NZB matches a live article in
/// `DupeArticleFallback::NzbFilenameOf`.
#[inline]
pub fn dupe_fallback_own_article_matches(
    own_part_number: i32,
    part_number: i32,
    own_msg_id: &[u8],
    msg_id: &[u8],
) -> i32 {
    if own_part_number == part_number && !msg_id.is_empty() && own_msg_id == msg_id {
        1
    } else {
        0
    }
}

/// Evaluates whether a failed donor parse is still within the retry cooldown in
/// `DupeArticleFallback::GetParsedDonor`.
#[inline]
pub fn dupe_fallback_is_bad_donor_in_cooldown(
    cur_time: i64,
    bad_time: i64,
    retry_sec: i32,
) -> i32 {
    if cur_time.wrapping_sub(bad_time) < (retry_sec as i64) {
        1
    } else {
        0
    }
}

/// Selects the content-matched donor file bucket (`1` = exact[0], `2` = folded[0],
/// `0` = none) in `DupeArticleFallback::MatchDonorFile`.
#[inline]
pub fn dupe_fallback_select_named_content_match(exact_count: usize, folded_count: usize) -> i32 {
    if exact_count == 1 {
        1
    } else if exact_count == 0 && folded_count == 1 {
        2
    } else {
        0
    }
}

/// Evaluates whether `NzbFile::ProcessFiles` should fall back to `m_metaTitle` when
/// `m_metaName` is empty.
#[inline]
pub fn nzbfile_should_fallback_meta_name_to_title(
    meta_name_empty: i32,
    meta_title_empty: i32,
) -> i32 {
    if meta_name_empty != 0 && meta_title_empty == 0 {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dupecoord_and_fallback_helpers() {
        assert_eq!(
            dupecoord_is_downloaded_on_disk_candidate(0, 0, 0, 1, 2, 1, b"Key1", b"key1"),
            1
        );
        assert_eq!(
            dupecoord_is_downloaded_on_disk_candidate(1, 0, 0, 1, 2, 1, b"Key1", b"key1"),
            0
        );
        assert_eq!(
            dupecoord_should_inherit_dupe_props(b"Show.S01E01", b"Show.S01E01", b"k", 0),
            1
        );
        assert_eq!(
            dupecoord_should_inherit_dupe_props(b"Show.S01E01", b"Show.S01E01", b"", 0),
            0
        );
        assert_eq!(
            dupecoord_is_recent_history_score_duplicate(1, 1, 2, 0, 2, 1, 100, 100, 1),
            1
        );
        assert_eq!(dupecoord_calc_post_delete_index(3, 5, 5), 4);
        assert_eq!(dupecoord_calc_post_delete_index(3, 5, 4), 3);
        assert_eq!(
            dupecoord_should_stop_other_queued_dupe(0, 1, 1, 0, 0, 0, 0, 2, b"abc", b"ABC"),
            1
        );
        assert_eq!(dupecoord_map_mark_to_dup_status(2, 2, 3, 10, 20, 30), 10);
        assert_eq!(dupecoord_map_mark_to_dup_status(3, 2, 3, 10, 20, 30), 20);
        assert_eq!(dupecoord_map_mark_to_dup_status(1, 2, 3, 10, 20, 30), 30);
        assert_eq!(
            dupecoord_should_hide_history_dupe(1, 1, 2, 0, 2, 4, 4, 0, 1),
            1
        );
        assert_eq!(dupe_fallback_completed_file_is_par2(0, b"file.PAR2"), 1);
        assert_eq!(dupe_fallback_completed_file_is_par2(0, b"file.rar"), 0);
        assert_eq!(dupe_fallback_should_abort_deferred_try(1, 0, 0, 0), 1);
        assert_eq!(dupe_fallback_should_abort_deferred_try(1, 1, 1, 1), 1);
        assert_eq!(dupe_fallback_should_abort_deferred_try(1, 1, 1, 0), 0);
        assert_eq!(dupe_fallback_is_reactive_first_round(0, 0), 1);
        assert_eq!(dupe_fallback_is_reactive_first_round(0, 1), 0);
    }

    #[test]
    fn test_fallback_stream_pardamage_nzbfile() {
        assert_eq!(
            dupe_fallback_par2_sums_entry_matches(1000, 1000, 0, b"abcd", b"ABCD", b"x", b"y"),
            1
        );
        assert_eq!(
            dupe_fallback_own_article_matches(3, 3, b"<m1@news>", b"<m1@news>"),
            1
        );
        assert_eq!(dupe_fallback_is_bad_donor_in_cooldown(105, 100, 10), 1);
        assert_eq!(dupe_fallback_is_bad_donor_in_cooldown(115, 100, 10), 0);
        assert_eq!(dupe_fallback_select_named_content_match(1, 2), 1);
        assert_eq!(dupe_fallback_select_named_content_match(0, 1), 2);
        assert_eq!(dupe_fallback_select_named_content_match(0, 2), 0);
        assert_eq!(nzbfile_should_fallback_meta_name_to_title(1, 0), 1);
        assert_eq!(nzbfile_should_fallback_meta_name_to_title(0, 0), 0);
    }
}
