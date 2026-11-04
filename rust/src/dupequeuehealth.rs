//! Hot-path decision predicates for duplicate article fallback, duplicate queue
//! coordination, fleet member scoring, and donor health exchange/verdict accounting.

/// Returns whether deferred fallback should check for available donors to log the
/// par-first deferral message (`DupeArticleFallback::TryNextSource`).
pub fn dqh_dupe_fallback_should_log_par_defer(
    is_proactive: bool,
    par_defer_state: i32,
    dp_none_val: i32,
    has_download_queue: bool,
) -> bool {
    !is_proactive && par_defer_state == dp_none_val && has_download_queue
}

/// Returns whether the article's original message-id should be restored when
/// deferring fallback after an advanced round (`DupeArticleFallback::TryNextSource`).
pub fn dqh_dupe_fallback_should_restore_original_msg_id(
    pinned_round: i32,
    orig_msg_id_empty: bool,
) -> bool {
    pinned_round > 0 && !orig_msg_id_empty
}

/// Returns whether a queued item is eligible as a donor before comparing
/// names/dupe-keys (`DupeArticleFallback::CollectDonors`).
pub fn dqh_dupe_fallback_is_queued_donor_eligible(
    is_self: bool,
    kind: i32,
    nk_nzb_val: i32,
    dupe_mode: i32,
    dm_force_val: i32,
) -> bool {
    !is_self && kind == nk_nzb_val && dupe_mode != dm_force_val
}

/// Returns whether a history item is eligible as a donor or history duplicate
/// before comparing names/dupe-keys (`DupeArticleFallback::CollectDonors`,
/// `DupeCoordinator::ListHistoryDupes`).
pub fn dqh_dupe_fallback_is_history_donor_eligible(
    kind: i32,
    hk_nzb_val: i32,
    dupe_mode: i32,
    dm_force_val: i32,
) -> bool {
    kind == hk_nzb_val && dupe_mode != dm_force_val
}

/// Returns whether two NZBs share a non-zero full content hash
/// (`DupeArticleFallback::CollectDonors`, `DupeArticleFallback::PinSources`).
pub fn dqh_dupe_fallback_is_same_full_content_hash(
    own_full_hash: u32,
    donor_full_hash: u32,
) -> bool {
    own_full_hash > 0 && own_full_hash == donor_full_hash
}

/// Returns whether `PinSources` should emit the once-per-file "No duplicate source"
/// detail message (`DupeArticleFallback::PinSources`).
pub fn dqh_dupe_fallback_should_log_no_source(
    twin_only: bool,
    candidates_empty: bool,
    donors_empty: bool,
    no_source_logged: bool,
) -> bool {
    !twin_only && candidates_empty && !donors_empty && !no_source_logged
}

/// Returns whether `MatchByContent` rejected the donor file (explicit `"-"` or
/// empty when `twin_only` is required) (`DupeArticleFallback::MatchDonorFile`).
pub fn dqh_dupe_fallback_is_content_match_rejected(by_content: &[u8], twin_only: bool) -> bool {
    by_content == b"-" || (twin_only && by_content.is_empty())
}

/// Returns whether a donor file's name matches either the target's current filename
/// or its original NZB filename case-insensitively in the active C locale
/// (`DupeArticleFallback::MatchDonorFile`).
pub fn dqh_dupe_fallback_is_donor_filename_match(
    target_filename: &[u8],
    target_nzb_filename: &[u8],
    donor_filename: &[u8],
) -> bool {
    crate::queuehotpath::strcasecmp_bytes(target_filename, donor_filename)
        || (!target_nzb_filename.is_empty()
            && crate::queuehotpath::strcasecmp_bytes(target_nzb_filename, donor_filename))
}

/// Returns whether `NzbFound` should check if the release is already downloaded on
/// disk (`DupeCoordinator::NzbFound`).
pub fn dqh_dupecoord_can_check_downloaded_on_disk(
    kind: i32,
    nk_nzb_val: i32,
    dupe_mode: i32,
    dm_force_val: i32,
) -> bool {
    kind == nk_nzb_val && dupe_mode != dm_force_val
}

/// Returns whether a queued item is a same-content duplicate of the newly found NZB
/// (`DupeCoordinator::NzbFound`).
pub fn dqh_dupecoord_is_queued_same_content_dupe(
    is_self: bool,
    same_content: bool,
    kind: i32,
    nk_nzb_val: i32,
) -> bool {
    !is_self && same_content && kind == nk_nzb_val
}

/// Returns whether `NzbFound` should inherit `dupekey` and `dupescore` from an
/// existing queue or history item with the same name (`DupeCoordinator::NzbFound`).
pub fn dqh_dupecoord_needs_dupe_prop_inheritance(dupe_key_empty: bool, dupe_score: i32) -> bool {
    dupe_key_empty && dupe_score == 0
}

/// Returns whether `NzbFound` should inspect same-content history items under
/// `HealthCheck=dupe` (`DupeCoordinator::NzbFound`).
#[allow(clippy::too_many_arguments)]
pub fn dqh_dupecoord_should_inspect_same_content_history(
    skip: bool,
    same_content: bool,
    health_check: i32,
    hc_dupe_val: i32,
    kind: i32,
    nk_nzb_val: i32,
    dupe_hint: i32,
    dh_none_val: i32,
) -> bool {
    skip && same_content
        && health_check == hc_dupe_val
        && kind == nk_nzb_val
        && dupe_hint == dh_none_val
}

/// Returns whether a queued item is a candidate for score-mode duplicate comparison
/// before checking `SameNameOrKey` (`DupeCoordinator::NzbFound`).
#[allow(clippy::too_many_arguments)]
pub fn dqh_dupecoord_is_queue_score_dupe_candidate(
    is_self: bool,
    delete_status: i32,
    ds_none_val: i32,
    queued_kind: i32,
    own_kind: i32,
    nk_nzb_val: i32,
    nk_url_val: i32,
    queued_dupe_mode: i32,
    dm_force_val: i32,
) -> bool {
    !is_self
        && delete_status == ds_none_val
        && (queued_kind == nk_nzb_val || (queued_kind == nk_url_val && own_kind == nk_url_val))
        && queued_dupe_mode != dm_force_val
}

/// Returns whether `HistoryMark` should skip duplicate cleanup / backup return after
/// marking an item (`DupeCoordinator::HistoryMark`).
pub fn dqh_dupecoord_should_skip_mark_action(
    dupe_check_enabled: bool,
    kind: i32,
    hk_nzb_val: i32,
    hk_dup_val: i32,
    item_dupe_mode: i32,
    dm_force_val: i32,
) -> bool {
    !dupe_check_enabled
        || (kind == hk_nzb_val && item_dupe_mode == dm_force_val)
        || (kind == hk_dup_val && item_dupe_mode == dm_force_val)
}

/// Clamps a fleet request's timeout in seconds to `[5, max_timeout_sec]`
/// (`Fleet::Append`).
pub fn dqh_fleet_clamp_timeout_sec(timeout_sec: i32, max_timeout_sec: i32) -> i32 {
    timeout_sec.min(max_timeout_sec).max(5)
}

/// Updates the key's `(top, lowest, running_id, running_score)` bounds for a
/// matching item in queue or history (`Fleet::Append`).
#[allow(clippy::too_many_arguments)]
pub fn dqh_fleet_update_key_score_bounds(
    cur_top: i64,
    cur_lowest: i32,
    any_seen: bool,
    queued: bool,
    deleting: bool,
    delete_status: i32,
    ds_none_val: i32,
    cur_running_id: i32,
    cur_running_score: i32,
    item_id: i32,
    item_score: i32,
) -> (i64, i32, i32, i32) {
    let new_top = cur_top.max(i64::from(item_score) + 1000);
    let new_lowest = if any_seen {
        cur_lowest.min(item_score)
    } else {
        item_score
    };
    let (new_running_id, new_running_score) = if queued
        && !deleting
        && delete_status == ds_none_val
        && (cur_running_id == 0 || item_score > cur_running_score)
    {
        (item_id, item_score)
    } else {
        (cur_running_id, cur_running_score)
    };
    (new_top, new_lowest, new_running_id, new_running_score)
}

/// Returns whether a history item is an eligible successful download before
/// checking `ReleaseName::Readable` and `FilesOnDisk` (`Fleet::Append`).
pub fn dqh_fleet_is_already_downloaded_history_item(
    has_item: bool,
    delete_status: i32,
    ds_none_val: i32,
    is_dupe_success: bool,
    mark_status: i32,
    ks_bad_val: i32,
) -> bool {
    has_item && delete_status == ds_none_val && is_dupe_success && mark_status != ks_bad_val
}

/// Computes the clamped duplicate score for adding a ranked fleet member
/// (`Fleet::Append`).
pub fn dqh_fleet_calc_member_add_score(
    running_id: i32,
    lowest: i32,
    top: i64,
    rank: i32,
) -> i32 {
    let base: i64 = if running_id != 0 {
        i64::from(lowest)
    } else {
        top.min(i64::from(i32::MAX))
    };
    let raw = base.saturating_sub(i64::from(rank));
    raw.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Returns the atomic increment (`0` if busy, `1` otherwise) for `m_errors` on an
/// exchange error (`DonorHealth::Server::Ask`).
pub fn dqh_donor_health_error_increment(busy: bool) -> i32 {
    if busy { 0 } else { 1 }
}

/// Returns whether the server's error counter has reached `server_give_up` and
/// should trigger a pause and reset (`DonorHealth::Server::Ask`).
pub fn dqh_donor_health_should_pause_server(errors: i32, server_give_up: i32) -> bool {
    errors >= server_give_up
}

/// Updates error count and server pause deadline on an exchange error
/// (`DonorHealth::Server::Ask`).
pub fn dqh_donor_health_record_exchange_error(
    cur_errors: i32,
    busy: bool,
    server_give_up: i32,
    now_ms: i64,
    retry_after_ms: i64,
    cur_down_until_ms: i64,
) -> (i32, i64) {
    let mut errors = cur_errors + if busy { 0 } else { 1 };
    let mut down_until = cur_down_until_ms;
    if errors >= server_give_up {
        down_until = now_ms.saturating_add(retry_after_ms);
        errors = 0;
    }
    (errors, down_until)
}

/// Accumulates an article's final verdict and soft body-bad flag into `Health`
/// counters (`DonorHealth::CheckItems`).
#[allow(clippy::too_many_arguments)]
pub fn dqh_donor_health_accum_verdict_counts(
    verdict: i32,
    f_present: i32,
    f_missing: i32,
    f_error: i32,
    soft: bool,
    present: &mut i32,
    missing: &mut i32,
    error: &mut i32,
    body_bad: &mut i32,
) {
    if verdict == f_present {
        *present += 1;
    }
    if verdict == f_missing {
        *missing += 1;
    }
    if verdict == f_error {
        *error += 1;
    }
    if soft && verdict != f_present {
        *body_bad += 1;
    }
}

/// Ensures `name` ends with `.nzb` (case-insensitive), appending `.nzb` if not (`Fleet::Add`).
pub fn dqh_fleet_ensure_nzb_ext(name: &[u8]) -> Vec<u8> {
    let mut out = name.to_vec();
    if name.len() < 4 || !name[name.len() - 4..].eq_ignore_ascii_case(b".nzb") {
        out.extend_from_slice(b".nzb");
    }
    out
}

/// Visits each non-empty `|`-separated segment of `names` (`Fleet::Add`).
pub fn dqh_fleet_visit_pipe_names(names: &[u8], mut visit: impl FnMut(&[u8])) {
    for part in names.split(|&b| b == b'|') {
        if !part.is_empty() {
            visit(part);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dupe_fallback_helpers() {
        assert!(dqh_dupe_fallback_should_log_par_defer(false, 0, 0, true));
        assert!(!dqh_dupe_fallback_should_log_par_defer(true, 0, 0, true));
        assert!(!dqh_dupe_fallback_should_log_par_defer(false, 1, 0, true));
        assert!(!dqh_dupe_fallback_should_log_par_defer(false, 0, 0, false));

        assert!(dqh_dupe_fallback_should_restore_original_msg_id(1, false));
        assert!(!dqh_dupe_fallback_should_restore_original_msg_id(0, false));
        assert!(!dqh_dupe_fallback_should_restore_original_msg_id(1, true));

        assert!(dqh_dupe_fallback_is_queued_donor_eligible(false, 0, 0, 1, 2));
        assert!(!dqh_dupe_fallback_is_queued_donor_eligible(true, 0, 0, 1, 2));
        assert!(!dqh_dupe_fallback_is_queued_donor_eligible(false, 1, 0, 1, 2));
        assert!(!dqh_dupe_fallback_is_queued_donor_eligible(false, 0, 0, 2, 2));

        assert!(dqh_dupe_fallback_is_history_donor_eligible(1, 1, 0, 2));
        assert!(!dqh_dupe_fallback_is_history_donor_eligible(0, 1, 0, 2));
        assert!(!dqh_dupe_fallback_is_history_donor_eligible(1, 1, 2, 2));

        assert!(dqh_dupe_fallback_is_same_full_content_hash(42, 42));
        assert!(!dqh_dupe_fallback_is_same_full_content_hash(0, 0));
        assert!(!dqh_dupe_fallback_is_same_full_content_hash(42, 43));

        assert!(dqh_dupe_fallback_should_log_no_source(false, true, false, false));
        assert!(!dqh_dupe_fallback_should_log_no_source(true, true, false, false));
        assert!(!dqh_dupe_fallback_should_log_no_source(false, false, false, false));
        assert!(!dqh_dupe_fallback_should_log_no_source(false, true, true, false));
        assert!(!dqh_dupe_fallback_should_log_no_source(false, true, false, true));

        assert!(dqh_dupe_fallback_is_content_match_rejected(b"-", false));
        assert!(dqh_dupe_fallback_is_content_match_rejected(b"", true));
        assert!(!dqh_dupe_fallback_is_content_match_rejected(b"", false));
        assert!(!dqh_dupe_fallback_is_content_match_rejected(b"part01.rar", true));

        assert!(dqh_dupe_fallback_is_donor_filename_match(
            b"Movie.part01.rar",
            b"",
            b"movie.part01.rar"
        ));
        assert!(dqh_dupe_fallback_is_donor_filename_match(
            b"obfuscated.bin",
            b"Movie.part01.rar",
            b"movie.part01.rar"
        ));
        assert!(!dqh_dupe_fallback_is_donor_filename_match(
            b"obfuscated.bin",
            b"",
            b"movie.part01.rar"
        ));
    }

    #[test]
    fn test_dupecoord_helpers() {
        assert!(dqh_dupecoord_can_check_downloaded_on_disk(0, 0, 1, 2));
        assert!(!dqh_dupecoord_can_check_downloaded_on_disk(1, 0, 1, 2));
        assert!(!dqh_dupecoord_can_check_downloaded_on_disk(0, 0, 2, 2));

        assert!(dqh_dupecoord_is_queued_same_content_dupe(false, true, 0, 0));
        assert!(!dqh_dupecoord_is_queued_same_content_dupe(true, true, 0, 0));
        assert!(!dqh_dupecoord_is_queued_same_content_dupe(false, false, 0, 0));

        assert!(dqh_dupecoord_needs_dupe_prop_inheritance(true, 0));
        assert!(!dqh_dupecoord_needs_dupe_prop_inheritance(false, 0));
        assert!(!dqh_dupecoord_needs_dupe_prop_inheritance(true, 10));

        assert!(dqh_dupecoord_should_inspect_same_content_history(
            true, true, 3, 3, 0, 0, 0, 0
        ));
        assert!(!dqh_dupecoord_should_inspect_same_content_history(
            false, true, 3, 3, 0, 0, 0, 0
        ));

        assert!(dqh_dupecoord_is_queue_score_dupe_candidate(
            false, 0, 0, 0, 1, 0, 1, 0, 2
        ));
        assert!(dqh_dupecoord_is_queue_score_dupe_candidate(
            false, 0, 0, 1, 1, 0, 1, 0, 2
        ));
        assert!(!dqh_dupecoord_is_queue_score_dupe_candidate(
            false, 0, 0, 1, 0, 0, 1, 0, 2
        ));

        assert!(dqh_dupecoord_should_skip_mark_action(false, 1, 1, 2, 0, 2));
        assert!(dqh_dupecoord_should_skip_mark_action(true, 1, 1, 2, 2, 2));
        assert!(dqh_dupecoord_should_skip_mark_action(true, 2, 1, 2, 2, 2));
        assert!(!dqh_dupecoord_should_skip_mark_action(true, 1, 1, 2, 0, 2));
    }

    #[test]
    fn test_fleet_and_donor_health_helpers() {
        assert_eq!(dqh_fleet_clamp_timeout_sec(2, 60), 5);
        assert_eq!(dqh_fleet_clamp_timeout_sec(30, 60), 30);
        assert_eq!(dqh_fleet_clamp_timeout_sec(90, 60), 60);

        let (top, lowest, run_id, run_score) =
            dqh_fleet_update_key_score_bounds(1_000_000, 0, false, true, false, 0, 0, 0, 0, 7, 500);
        assert_eq!((top, lowest, run_id, run_score), (1_000_000, 500, 7, 500));

        assert!(dqh_fleet_is_already_downloaded_history_item(
            true, 0, 0, true, 1, 2
        ));
        assert!(!dqh_fleet_is_already_downloaded_history_item(
            true, 0, 0, true, 2, 2
        ));

        assert_eq!(dqh_fleet_calc_member_add_score(0, 100, 1_000_000, 1), 999_999);
        assert_eq!(dqh_fleet_calc_member_add_score(5, 100, 1_000_000, 2), 98);

        assert_eq!(
            dqh_donor_health_record_exchange_error(1, false, 2, 5000, 10000, 0),
            (0, 15000)
        );
        assert_eq!(
            dqh_donor_health_record_exchange_error(1, true, 2, 5000, 10000, 0),
            (1, 0)
        );

        let (mut p, mut m, mut e, mut b) = (0, 0, 0, 0);
        dqh_donor_health_accum_verdict_counts(1, 1, 2, 3, true, &mut p, &mut m, &mut e, &mut b);
        dqh_donor_health_accum_verdict_counts(2, 1, 2, 3, true, &mut p, &mut m, &mut e, &mut b);
        dqh_donor_health_accum_verdict_counts(3, 1, 2, 3, false, &mut p, &mut m, &mut e, &mut b);
        assert_eq!((p, m, e, b), (1, 1, 1, 1));

        assert_eq!(dqh_fleet_ensure_nzb_ext(b"Movie.Release"), b"Movie.Release.nzb");
        assert_eq!(dqh_fleet_ensure_nzb_ext(b"Movie.Release.NZB"), b"Movie.Release.NZB");
        let mut parts = Vec::new();
        dqh_fleet_visit_pipe_names(b"first.nzb||second.nzb|", |s| parts.push(s.to_vec()));
        assert_eq!(parts, vec![b"first.nzb".to_vec(), b"second.nzb".to_vec()]);
    }
}
