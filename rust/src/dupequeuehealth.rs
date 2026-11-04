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

/// Finds the index of `lead_nzb_id` in `donor_ids` for `DupeArticleFallback::RotateToLead`
/// (`-1` if `lead_nzb_id == 0` or not found).
pub fn dqh_dupe_fallback_find_lead_index(donor_ids: &[i32], lead_nzb_id: i32) -> isize {
    if lead_nzb_id == 0 {
        return -1;
    }
    donor_ids
        .iter()
        .position(|&id| id == lead_nzb_id)
        .map(|p| p as isize)
        .unwrap_or(-1)
}

/// Checks whether `DupeArticleFallback::RegisterLeadSuccess` should proceed.
pub fn dqh_dupe_fallback_can_register_lead_success(
    fallback_round: i32,
    lead_snapshot: i32,
    file_lead_donor_id: i32,
) -> bool {
    fallback_round == 1 && lead_snapshot != 0 && lead_snapshot == file_lead_donor_id
}

/// Checks whether any article in `fallback_rounds` has `round > 0` (`DupeArticleFallback::HasDonorArticles`).
pub fn dqh_dupe_fallback_has_donor_articles(fallback_rounds: &[i32]) -> bool {
    fallback_rounds.iter().any(|&r| r > 0)
}

/// Checks whether `DupeArticleFallback::FirstContent` can inspect the first article on disk.
pub fn dqh_dupe_fallback_can_read_first_content_from_disk(
    has_articles: bool,
    first_part_number: i32,
    first_status: i32,
    ai_finished: i32,
    output_filename: &[u8],
) -> bool {
    has_articles
        && first_part_number == 1
        && first_status == ai_finished
        && !output_filename.is_empty()
}

/// Checks whether both identities are non-empty and `kind` starts with `"twin"`
/// (`DupeArticleFallback::CollectDonors`, `DupeArticleFallback::MatchDonorFile`).
pub fn dqh_dupe_fallback_is_twin_kind(
    own_identity: &[u8],
    donor_identity: &[u8],
    kind: &[u8],
) -> bool {
    !own_identity.is_empty() && !donor_identity.is_empty() && kind.starts_with(b"twin")
}

/// Checks whether `DupeArticleFallback::PinSources` should reset a non-twin lead donor when `twin_only` is active.
pub fn dqh_dupe_fallback_should_reset_non_twin_lead(
    twin_only: bool,
    lead_id: i32,
    own_identity: &[u8],
    lead_identity: &[u8],
    kind: &[u8],
) -> bool {
    twin_only && lead_id != 0 && !dqh_dupe_fallback_is_twin_kind(own_identity, lead_identity, kind)
}

/// Accumulates `DonorHealth::Health::operator+=` counters.
#[allow(clippy::too_many_arguments)]
pub fn dqh_donor_health_accum_health(
    checked: &mut i32,
    present: &mut i32,
    missing: &mut i32,
    error: &mut i32,
    body_checked: &mut i32,
    body_bad: &mut i32,
    add_checked: i32,
    add_present: i32,
    add_missing: i32,
    add_error: i32,
    add_body_checked: i32,
    add_body_bad: i32,
) {
    *checked = checked.wrapping_add(add_checked);
    *present = present.wrapping_add(add_present);
    *missing = missing.wrapping_add(add_missing);
    *error = error.wrapping_add(add_error);
    *body_checked = body_checked.wrapping_add(add_body_checked);
    *body_bad = body_bad.wrapping_add(add_body_bad);
}

/// Computes a worker's remaining budget in `DonorHealth::CheckPostings`; returns `false` if the deadline has passed.
pub fn dqh_donor_health_calc_worker_budget_ms(
    deadline_ms: i64,
    now_ms: i64,
    cur_budget_ms: i32,
    out_budget_ms: &mut i32,
) -> bool {
    if deadline_ms > 0 {
        let left = deadline_ms.saturating_sub(now_ms);
        if left <= 0 {
            return false;
        }
        *out_budget_ms = i64::from(cur_budget_ms).min(left) as i32;
    } else {
        *out_budget_ms = cur_budget_ms;
    }
    true
}

/// Computes the worker thread count for `DonorHealth::CheckPostings`.
pub fn dqh_donor_health_calc_thread_count(postings_len: usize, concurrency: i32) -> usize {
    postings_len.min(1i32.max(concurrency) as usize)
}

/// Formats the HTTP fetch error message for `Fleet::Append`.
pub fn dqh_fleet_format_fetch_error(status: i32) -> Vec<u8> {
    if status != 0 {
        format!("the nzb-file could not be fetched (HTTP {status})").into_bytes()
    } else {
        b"the nzb-file could not be fetched (connection failed)".to_vec()
    }
}

/// Computes `(sample_maximum, budget_ms, check_deadline_ms)` for `Fleet::Append`.
pub fn dqh_fleet_calc_check_budget_and_deadline(
    opt_max: i32,
    fleet_sample_max: i32,
    deadline_ms: i64,
    check_start_ms: i64,
    finish_reserve_ms: i64,
) -> (i32, i32, i64) {
    let sample_maximum = opt_max.min(fleet_sample_max);
    let budget_ms = 1000i64.max(deadline_ms - check_start_ms - finish_reserve_ms) as i32;
    let check_deadline_ms = check_start_ms + i64::from(budget_ms);
    (sample_maximum, budget_ms, check_deadline_ms)
}

/// Computes the candidate tier (`0..=3`) in `Fleet::Append`.
pub fn dqh_fleet_candidate_tier(has_error: bool, dead: bool, alive: i32) -> i32 {
    if has_error {
        3
    } else if dead {
        2
    } else if alive < 0 {
        1
    } else {
        0
    }
}

/// Formats the final `Fleet of ...` summary log message in `Fleet::Append`.
pub fn dqh_fleet_format_summary_log(
    members_count: i32,
    dupe_key: &[u8],
    chosen: i32,
    reason: &[u8],
    complete: bool,
) -> Vec<u8> {
    let mut out = format!("Fleet of {members_count} nzb-file(s) for ").into_bytes();
    out.extend_from_slice(dupe_key);
    out.extend_from_slice(b": ");
    if chosen != 0 {
        out.extend_from_slice(format!("queued #{chosen}").as_bytes());
    } else {
        out.extend_from_slice(reason);
    }
    if !complete {
        out.extend_from_slice(b" (not every posting checked in time)");
    }
    out
}

/// Evaluates whether `DupeSearch::PrepareNative` generated the download's key (`DupeSearch::PrepareNative`).
pub fn dqh_dupesearch_is_generated_key(
    dupe_key_empty: bool,
    has_owner: bool,
    owner_value: &[u8],
    nzb_id: i32,
    key: &[u8],
) -> bool {
    if dupe_key_empty {
        return true;
    }
    if !has_owner || !key.starts_with(b"dupes:") {
        return false;
    }
    let mut expected = format!("{nzb_id}:").into_bytes();
    expected.extend_from_slice(key);
    owner_value == expected.as_slice()
}

/// Evaluates whether `DupeSearch::DownloadQueueUpdate` should inspect the queue to schedule
/// rank-only re-scoring for an existing parent download (`DupeSearch::DownloadQueueUpdate`).
pub fn dqh_dupesearch_should_check_added_for_parent_rank(
    action: i32,
    ea_nzb_added: i32,
    is_donor: bool,
    dupe_mode: i32,
    dm_score: i32,
    has_dupe_fleet_param: bool,
) -> bool {
    action == ea_nzb_added && !is_donor && dupe_mode == dm_score && !has_dupe_fleet_param
}

/// Evaluates whether a queued item is a better parent candidate for rank-only scheduling
/// when a lower-scored external backup is added (`DupeSearch::DownloadQueueUpdate`).
#[allow(clippy::too_many_arguments)]
pub fn dqh_dupesearch_is_better_parent_candidate(
    is_same_item: bool,
    kind: i32,
    nk_nzb: i32,
    deleting: bool,
    delete_status: i32,
    ds_none: i32,
    has_post_info: bool,
    same_lower_key: bool,
    item_score: i32,
    added_score: i32,
    has_parent: bool,
    parent_score: i32,
) -> bool {
    !is_same_item
        && kind == nk_nzb
        && !deleting
        && delete_status == ds_none
        && !has_post_info
        && same_lower_key
        && item_score > added_score
        && (!has_parent || item_score > parent_score)
}

/// Evaluates whether another queue/history item should be retained in the peer list
/// for `DupeSearch::PrepareNative`.
pub fn dqh_dupesearch_should_retain_peer(
    is_same_item: bool,
    item_score: i32,
    pick_score: i32,
    has_dupe_fleet_param: bool,
) -> bool {
    !is_same_item && (item_score > pick_score || has_dupe_fleet_param)
}

/// Formats comma-separated `"key: count"` rejection entries for `DupeSearch::Place`.
pub fn dqh_dupesearch_format_rejected_outcome(keys: &[&[u8]], counts: &[i32]) -> Vec<u8> {
    let mut out = Vec::new();
    for (k, &c) in keys.iter().zip(counts.iter()) {
        if c > 0 {
            if !out.is_empty() {
                out.extend_from_slice(b", ");
            }
            out.extend_from_slice(k);
            out.extend_from_slice(b": ");
            out.extend_from_slice(c.to_string().as_bytes());
        }
    }
    out
}

/// Evaluates whether a directory entry name is a positive canonical integer ID (`DupeSearch::ResumePending`).
pub fn dqh_dupesearch_parse_pending_dir_id(name: &[u8]) -> i32 {
    if name.is_empty() || name[0] == b'0' || !name.iter().all(|b| b.is_ascii_digit()) {
        return 0;
    }
    std::str::from_utf8(name)
        .ok()
        .and_then(|s| s.parse::<i32>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(0)
}

/// Evaluates whether the saved `pick-title`, `pick-key`, and `pick-posting` files match
/// the current pick's normalized identity (`DupeSearch::ResumePending`).
pub fn dqh_dupesearch_pending_identity_matches(
    has_pick_title_file: bool,
    saved_title: &[u8],
    cur_norm_title: &[u8],
    saved_key: &[u8],
    cur_lower_key: &[u8],
    saved_posting: &[u8],
    cur_fingerprint: &[u8],
) -> bool {
    has_pick_title_file
        && saved_title == cur_norm_title
        && saved_key == cur_lower_key
        && saved_posting == cur_fingerprint
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

        assert_eq!(dqh_dupe_fallback_find_lead_index(&[10, 20, 30], 20), 1);
        assert_eq!(dqh_dupe_fallback_find_lead_index(&[10, 20, 30], 0), -1);
        assert!(dqh_dupe_fallback_can_register_lead_success(1, 5, 5));
        assert!(!dqh_dupe_fallback_can_register_lead_success(2, 5, 5));
        assert!(dqh_dupe_fallback_has_donor_articles(&[0, 0, 2]));
        assert!(!dqh_dupe_fallback_has_donor_articles(&[0, 0]));
        assert!(dqh_dupe_fallback_can_read_first_content_from_disk(
            true, 1, 2, 2, b"/tmp/out"
        ));
        assert!(dqh_dupe_fallback_is_twin_kind(b"fp1", b"fp2", b"twin 16/18"));
        assert!(!dqh_dupe_fallback_is_twin_kind(b"fp1", b"fp2", b"alt"));
        assert!(dqh_dupe_fallback_should_reset_non_twin_lead(
            true, 3, b"fp1", b"fp2", b"alt"
        ));

        let mut b_ms = 0;
        assert!(dqh_donor_health_calc_worker_budget_ms(5000, 3000, 4000, &mut b_ms));
        assert_eq!(b_ms, 2000);
        assert!(!dqh_donor_health_calc_worker_budget_ms(3000, 3000, 4000, &mut b_ms));
        assert_eq!(dqh_donor_health_calc_thread_count(10, 4), 4);
        assert_eq!(
            dqh_fleet_format_fetch_error(503),
            b"the nzb-file could not be fetched (HTTP 503)"
        );
        assert_eq!(
            dqh_fleet_calc_check_budget_and_deadline(500, 200, 10000, 2000, 2000),
            (200, 6000, 8000)
        );
        assert_eq!(dqh_fleet_candidate_tier(false, false, 95), 0);
        assert_eq!(dqh_fleet_candidate_tier(false, false, -1), 1);
        assert_eq!(dqh_fleet_candidate_tier(false, true, 10), 2);
        assert_eq!(dqh_fleet_candidate_tier(true, false, -1), 3);
        assert_eq!(
            dqh_fleet_format_summary_log(3, b"show.s01e01", 12, b"", false),
            b"Fleet of 3 nzb-file(s) for show.s01e01: queued #12 (not every posting checked in time)"
        );

        assert!(dqh_dupesearch_is_generated_key(true, false, b"", 5, b""));
        assert!(dqh_dupesearch_is_generated_key(
            false,
            true,
            b"5:dupes:movie",
            5,
            b"dupes:movie"
        ));
        assert!(!dqh_dupesearch_is_generated_key(
            false,
            true,
            b"6:dupes:movie",
            5,
            b"dupes:movie"
        ));
        assert!(dqh_dupesearch_should_check_added_for_parent_rank(
            1, 1, false, 0, 0, false
        ));
        assert!(!dqh_dupesearch_should_check_added_for_parent_rank(
            1, 1, true, 0, 0, false
        ));
        assert!(dqh_dupesearch_is_better_parent_candidate(
            false, 0, 0, false, 0, 0, false, true, 1000, 500, false, 0
        ));
        assert!(dqh_dupesearch_should_retain_peer(false, 600, 500, false));
        assert!(dqh_dupesearch_should_retain_peer(false, 400, 500, true));
        assert!(!dqh_dupesearch_should_retain_peer(true, 600, 500, true));
        assert_eq!(
            dqh_dupesearch_format_rejected_outcome(
                &[b"dead", b"fetch", b"over-cap"],
                &[2, 0, 1]
            ),
            b"dead: 2, over-cap: 1"
        );
        assert_eq!(dqh_dupesearch_parse_pending_dir_id(b"42"), 42);
        assert_eq!(dqh_dupesearch_parse_pending_dir_id(b"042"), 0);
        assert_eq!(dqh_dupesearch_parse_pending_dir_id(b"abc"), 0);
        assert!(dqh_dupesearch_pending_identity_matches(
            true, b"title", b"title", b"key", b"key", b"fp", b"fp"
        ));
        assert!(!dqh_dupesearch_pending_identity_matches(
            false, b"title", b"title", b"key", b"key", b"fp", b"fp"
        ));
    }
}
