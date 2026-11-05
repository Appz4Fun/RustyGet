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

/// Evaluates whether `DupeArticleFallback::OrderSources` should append the primary message ID
/// after the donor candidates.
pub fn dqh_dupe_fallback_should_append_primary_on_order(
    cutover: bool,
    donors_len: usize,
) -> bool {
    cutover && donors_len > 0
}

/// Formats the optional `" and N whole file(s)"` suffix for `StreamRepairController::StopWatchdog`.
pub fn dqh_stream_repair_format_whole_files_suffix(unsized_count: i32) -> Vec<u8> {
    if unsized_count > 0 {
        format!(" and {unsized_count} whole file(s)").into_bytes()
    } else {
        Vec::new()
    }
}

/// Converts a byte count into megabytes (`bytes / 1024.0 / 1024.0`).
pub fn dqh_stream_repair_bytes_to_mb(bytes: i64) -> f64 {
    (bytes as f64) / 1024.0 / 1024.0
}

/// Evaluates whether stream repair donor verification succeeded (`totalCompared >= required && sawVariedData`).
pub fn dqh_stream_repair_is_verification_satisfied(
    total_compared: i64,
    required: i64,
    saw_varied_data: bool,
) -> bool {
    total_compared >= required && saw_varied_data
}

/// Evaluates whether `ArticleWriter::SetupOutputFile` should use `targetPath` directly instead of `MakeUniqueFilename`.
pub fn dqh_article_writer_use_target_path_directly(
    force_direct_write: bool,
    direct_write: bool,
    output_filename_empty: bool,
    same_filename: bool,
) -> bool {
    force_direct_write || (direct_write && !output_filename_empty && same_filename)
}

/// Evaluates whether a candidate news server is available for article download (`ArticleDownloader::Run`).
pub fn dqh_article_dl_is_server_available(
    active: bool,
    max_connections: i32,
    optional: bool,
    is_blocked: bool,
) -> bool {
    active && max_connections != 0 && !(optional && is_blocked)
}

/// Deduplicates, sorts, and formats a comma-separated list of donor IDs
/// (`QueueCoordinator::RejectBorrowedArticles`).
pub fn dqh_queue_coord_format_donor_id_list(donor_ids: &[i32]) -> (Vec<i32>, Vec<u8>) {
    let mut unique: Vec<i32> = donor_ids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    let mut out = String::new();
    for (i, id) in unique.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&id.to_string());
    }
    (unique, out.into_bytes())
}

/// Evaluates whether a completed file is an incomplete side file and whether it is
/// eligible for stream repair job creation (`QueueCoordinator::DeleteFileInfo`).
pub fn dqh_queue_coord_eval_completed_file_repair_eligibility(
    completed: bool,
    file_status: i32,
    cf_partial: i32,
    cf_failure: i32,
    is_side_file: bool,
) -> (bool, bool) {
    let incomplete = completed && (file_status == cf_partial || file_status == cf_failure);
    let side_file = incomplete && is_side_file;
    let check_stream_repair = incomplete && !side_file;
    (side_file, check_stream_repair)
}

/// Evaluates whether `other` is a size twin of `nzbInfo` (`QueueCoordinator::HasSizeTwin`).
pub fn dqh_queue_coord_is_size_twin(
    is_self: bool,
    same_name_or_key: bool,
    self_size: i64,
    other_size: i64,
    other_par_size: i64,
    largest_self_file: i64,
    other_file_sizes: &[i64],
) -> bool {
    if is_self || !same_name_or_key {
        return false;
    }
    if crate::queuehotpath::size_near(self_size, other_size)
        || (other_par_size > 0
            && crate::queuehotpath::size_near(self_size, other_size - other_par_size))
    {
        return true;
    }
    other_file_sizes
        .iter()
        .any(|&fs| crate::queuehotpath::size_near(largest_self_file, fs))
}

/// Evaluates the entry guard for `QueueCoordinator::CheckDeadDownload`.
#[allow(clippy::too_many_arguments)]
pub fn dqh_queue_coord_can_check_dead_download(
    health_check: i32,
    hc_dupe: i32,
    dupe_check: bool,
    fails_over: bool,
    kind: i32,
    nk_nzb: i32,
    deleting: bool,
    parking: bool,
    delete_status: i32,
    ds_none: i32,
) -> bool {
    health_check == hc_dupe
        && dupe_check
        && fails_over
        && kind == nk_nzb
        && !deleting
        && !parking
        && delete_status == ds_none
}

/// Evaluates whether a queued item is an active unpaused duplicate of the current download
/// (`QueueCoordinator::CheckParDamage`).
pub fn dqh_queue_coord_is_active_unpaused_queued_dupe(
    is_self: bool,
    paused_size: i64,
    remaining_size: i64,
    same_name_or_key: bool,
) -> bool {
    !is_self && paused_size < remaining_size && same_name_or_key
}

/// Evaluates the entry guard for `QueueCoordinator::FailOverDeadPick`.
pub fn dqh_queue_coord_can_fail_over_dead_pick(
    has_nzb: bool,
    deleting: bool,
    parking: bool,
    delete_status: i32,
    ds_none: i32,
) -> bool {
    has_nzb && !deleting && !parking && delete_status == ds_none
}

/// Returns the NUL-terminated download state label (`"paused"`, `"temp-paused"`, or `"active"`)
/// for `QueueCoordinator::LogDebugInfo`.
pub fn dqh_queue_coord_download_debug_state_label(
    pause_download: bool,
    temp_pause_download: bool,
) -> &'static [u8] {
    if pause_download {
        b"paused\0"
    } else if temp_pause_download {
        b"temp-paused\0"
    } else {
        b"active\0"
    }
}

/// Converts a permille health value into percentage (`permille / 10.0`).
pub fn dqh_prepost_permille_to_percent(permille: i32) -> f64 {
    f64::from(permille) / 10.0
}

/// Formats the queued duplicate skip warning message in `DupeCoordinator::NzbFound`.
pub fn dqh_dupecoord_format_queued_dupe_skip_message(
    name: &[u8],
    queued_name: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len() + queued_name.len() + 48);
    out.extend_from_slice(b"Skipping duplicate ");
    out.extend_from_slice(name);
    if name == queued_name {
        out.extend_from_slice(b", already queued");
    } else {
        out.extend_from_slice(b", already queued as ");
        out.extend_from_slice(queued_name);
    }
    out
}

/// Formats the history duplicate skip warning message in `DupeCoordinator::NzbFound`.
pub fn dqh_dupecoord_format_history_dupe_skip_message(
    name: &[u8],
    dupe_name: &[u8],
    same_content: bool,
    good: bool,
) -> Vec<u8> {
    let reason: &[u8] = if same_content {
        b"exactly same content (the same posting, no new articles)"
    } else if good {
        b"good status"
    } else {
        b"success status"
    };
    let mut out = Vec::with_capacity(name.len() + dupe_name.len() + reason.len() + 48);
    out.extend_from_slice(b"Skipping duplicate ");
    out.extend_from_slice(name);
    if name == dupe_name {
        out.extend_from_slice(b", found in history with ");
    } else {
        out.extend_from_slice(b", found in history ");
        out.extend_from_slice(dupe_name);
        out.extend_from_slice(b" with ");
    }
    out.extend_from_slice(reason);
    out
}

/// Evaluates whether `DupeCoordinator::NzbCompleted` should check `FilesOnDisk` to stop other
/// queued duplicates of the same key, and resolves the group delete action.
#[allow(clippy::too_many_arguments)]
pub fn dqh_dupecoord_eval_completed_stop_others(
    dupe_health: bool,
    dupe_key_empty: bool,
    delete_status: i32,
    ds_none: i32,
    is_dupe_success: bool,
    health_check: i32,
    hc_park: i32,
    ea_group_park_delete: i32,
    ea_group_dupe_delete: i32,
) -> (bool, i32) {
    let should_check =
        dupe_health && !dupe_key_empty && delete_status == ds_none && is_dupe_success;
    let delete_action = if health_check == hc_park {
        ea_group_park_delete
    } else {
        ea_group_dupe_delete
    };
    (should_check, delete_action)
}

/// Returns the NUL-terminated non-failover reason in `DupeCoordinator::NoBackupReason`
/// when `!FailsOver(nzbInfo)`.
pub fn dqh_dupecoord_format_non_failover_reason(dupe_mode: i32, dm_force: i32) -> &'static [u8] {
    if dupe_mode == dm_force {
        b"its DupeMode is force\0"
    } else {
        b"its DupeMode is all\0"
    }
}

/// Returns the NUL-terminated mark-status label for `DupeCoordinator::HistoryMark`.
pub fn dqh_dupecoord_mark_status_label(mark_status: i32) -> &'static [u8] {
    match mark_status {
        1 => b"bad\0",
        2 => b"good\0",
        3 => b"success\0",
        _ => b"NONE\0",
    }
}

/// Formats `NzbInfo::MakeNiceUrlName` (`DownloadInfo.cpp`).
pub fn dqh_nzbinfo_format_nice_url_name(
    url_str: &[u8],
    nzb_nicename: &[u8],
    has_nzb_filename: bool,
    url_valid: bool,
    url_host: &[u8],
    url_resource: &[u8],
) -> Vec<u8> {
    if has_nzb_filename {
        let mut out = Vec::with_capacity(nzb_nicename.len() + url_host.len() + 3);
        out.extend_from_slice(nzb_nicename);
        out.extend_from_slice(b" @ ");
        out.extend_from_slice(url_host);
        out
    } else if url_valid {
        let mut out = Vec::with_capacity(url_host.len() + url_resource.len());
        out.extend_from_slice(url_host);
        out.extend_from_slice(url_resource);
        out
    } else {
        url_str.to_vec()
    }
}

/// Formats the final leaf directory suffix (`"%c%s-%i"` or `"%c%s"`) in `NzbInfo::BuildFinalDirName`.
pub fn dqh_nzbinfo_format_final_dir_suffix(
    sep: u8,
    name_empty: bool,
    dir_name: &[u8],
    nzb_id: i32,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(dir_name.len() + 16);
    out.push(sep);
    if name_empty {
        out.extend_from_slice(format!("nzb-{nzb_id}").as_bytes());
    } else {
        out.extend_from_slice(dir_name);
    }
    out
}

/// Formats `ArticleDownloader::Run` `m_connectionName` (`"%s (%s)"`, `server_name`, `host`).
pub fn dqh_article_dl_format_connection_name(server_name: &[u8], host: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(server_name.len() + host.len() + 3);
    out.extend_from_slice(server_name);
    out.extend_from_slice(b" (");
    out.extend_from_slice(host);
    out.push(b')');
    out
}

/// Formats `ArticleDownloader::Download` NNTP `ARTICLE` / `BODY` command line.
pub fn dqh_article_dl_format_fetch_command(raw_article: bool, message_id: &[u8]) -> Vec<u8> {
    let verb: &[u8] = if raw_article { b"ARTICLE " } else { b"BODY " };
    let mut out = Vec::with_capacity(verb.len() + message_id.len() + 2);
    out.extend_from_slice(verb);
    out.extend_from_slice(message_id);
    out.extend_from_slice(b"\r\n");
    out
}

/// Selects the yEnc article CRC in `ArticleDownloader::DecodeCheck`.
pub fn dqh_article_dl_select_yenc_crc(
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
pub fn dqh_article_writer_open_verb(direct_write: bool) -> &'static [u8] {
    if direct_write {
        b"open\0"
    } else {
        b"create\0"
    }
}

/// Formats `fullInfoPath` / `infoName` (`"%s%c%s"`, `nzb_name`, `sep`, `filename`) in `ArticleWriter` / `ArticleCache`.
pub fn dqh_article_writer_format_info_path(nzb_name: &[u8], sep: u8, filename: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(nzb_name.len() + 1 + filename.len());
    out.extend_from_slice(nzb_name);
    out.push(sep);
    out.extend_from_slice(filename);
    out
}

/// Computes the parent directory prefix length of `final_path` when `filename` contains `sep` in `ArticleWriter::SetupOutputFile`.
pub fn dqh_article_writer_parent_dir_len(
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
pub fn dqh_article_writer_apply_missing_part_counters(
    failed_articles: i32,
    success_articles: i32,
) -> (i32, i32) {
    (
        failed_articles.wrapping_add(1),
        success_articles.wrapping_sub(1),
    )
}

/// Formats the raw article destination path (`"%s%c%03i"`) in `ArticleWriter::ProcessArticles`.
pub fn dqh_article_writer_format_raw_part_dest(
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
pub fn dqh_article_writer_calc_total_failed_articles(
    missed_articles: i32,
    failed_articles: i32,
) -> i32 {
    missed_articles.wrapping_add(failed_articles)
}

/// Computes `(speed_kb, time_diff)` in `StatMeter::LogDebugInfo`.
pub fn dqh_statmeter_calc_debug_speed_and_timediff(
    current_download_speed: i32,
    cur_time: i32,
    speed_start_time: i32,
    slot_size: i32,
) -> (i32, i32) {
    (
        current_download_speed / 1024,
        cur_time.wrapping_sub(speed_start_time.wrapping_mul(slot_size)),
    )
}

/// Converts bytes to megabytes (`(int)(bytes / 1024 / 1024)`) in `StatMeter::CalcQuotaUsage`.
pub fn dqh_statmeter_bytes_to_mb_i32(bytes: i64) -> i32 {
    (bytes / 1024 / 1024) as i32
}

/// Format the `-m<mem>`, `-t<threads>`, and `-T<threads>` CLI flags for
/// `Repairer::PreProcess` in `ParChecker.cpp`.
pub fn dqh_parchecker_format_cli_flags(
    threads_to_use: i32,
    mem_to_use: i32,
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let threads_str = threads_to_use.to_string();
    let mem_param = format!("-m{mem_to_use}").into_bytes();
    let threads_param = format!("-t{threads_str}").into_bytes();
    let file_threads_param = format!("-T{threads_str}").into_bytes();
    (mem_param, threads_param, file_threads_param)
}

/// Select the PAR2 packet creator display string (`"<unknown program>"` when
/// empty) for `ParChecker::RunParCheck` in `ParChecker.cpp`.
pub fn dqh_parchecker_creator_or_unknown(creator: &[u8]) -> &[u8] {
    if creator.is_empty() {
        b"<unknown program>"
    } else {
        creator
    }
}

/// Format `"<verb> <info_name>"` (e.g. `"Verifying <info_name>"` or
/// `"Repairing <info_name>"`) for `ParChecker::RunParCheck` in `ParChecker.cpp`.
pub fn dqh_parchecker_format_action_label(verb: &[u8], info_name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(verb.len() + 1 + info_name.len());
    out.extend_from_slice(verb);
    out.push(b' ');
    out.extend_from_slice(info_name);
    out
}

/// Compute the effective `sourceFilesCount` passed to stage-progress calculation
/// in `ParChecker::SignalProgress` (`ParChecker.cpp`).
pub fn dqh_parchecker_effective_source_files_count(
    stage: i32,
    pt_repairing: i32,
    pt_verifying_repaired: i32,
    total_source_files: usize,
) -> i32 {
    if stage != pt_repairing && stage != pt_verifying_repaired {
        total_source_files as i32
    } else {
        0
    }
}

/// Return whether `ParChecker::SignalDone` should check `ParIgnoreExt` for a
/// verified file (`ParChecker.cpp`).
pub fn dqh_parchecker_should_check_ignore_ext(
    stage: i32,
    pt_verifying_sources: i32,
    available: i32,
    total: i32,
    verifying_extra_files: bool,
) -> bool {
    stage == pt_verifying_sources && available < total && !verifying_extra_files
}

/// Return the NUL-terminated `", ignoring"` or `""` suffix for damaged/missing
/// file warnings in `ParChecker::SignalDone` and `ParChecker::CheckEmptyFiles`
/// (`ParChecker.cpp`).
pub fn dqh_parchecker_ignore_suffix(ignore: bool) -> &'static [u8] {
    if ignore {
        b", ignoring\0"
    } else {
        b"\0"
    }
}

/// Return the NUL-terminated `"good"` or `"damaged"` label for
/// `ParChecker::VerifyDataFile` messages in `ParChecker.cpp`.
pub fn dqh_parchecker_file_quality_label(file_status: i32, fs_success: i32) -> &'static [u8] {
    if file_status == fs_success {
        b"good\0"
    } else {
        b"damaged\0"
    }
}

/// Return whether `PrePostProcessor::StartJob` should compute `nzbInfo->CalcHealth()`
/// when checking for an unpaused PAR volume to replace a missing index file
/// (`PrePostProcessor.cpp`).
pub fn dqh_prepost_should_calc_health_for_par_index(
    par_status: i32,
    ps_none: i32,
    ps_skipped: i32,
    delete_status: i32,
    ds_none: i32,
) -> bool {
    (par_status == ps_none || par_status == ps_skipped) && delete_status == ds_none
}

/// Return the NUL-terminated reason string (`"required par-repair"` vs
/// `"par-failure"`) when skipping unpack due to PAR status in
/// `PrePostProcessor::StartJob` (`PrePostProcessor.cpp`).
pub fn dqh_prepost_skip_unpack_par_reason(par_status: i32, ps_manual: i32) -> &'static [u8] {
    if par_status == ps_manual {
        b"required par-repair\0"
    } else {
        b"par-failure\0"
    }
}

/// Select the write error message or the `"incomplete write (disk full?)"`
/// fallback when `last_err` is empty (`StreamRepair.cpp`).
pub fn dqh_stream_repair_write_errmsg_or_default(last_err: &[u8]) -> &[u8] {
    if last_err.is_empty() {
        b"incomplete write (disk full?)"
    } else {
        last_err
    }
}

/// Format `"nothing recovered for <timeout> seconds"` for
/// `StreamRepairController::StartWatchdog` (`StreamRepair.cpp`).
pub fn dqh_stream_repair_format_stall_reason(timeout: i32) -> Vec<u8> {
    format!("nothing recovered for {timeout} seconds").into_bytes()
}

/// Format `"still waiting for news-server connections after <elapsed_sec> seconds"`
/// for `StreamRepairController::StartWatchdog` (`StreamRepair.cpp`).
pub fn dqh_stream_repair_format_conn_wait_reason(elapsed_sec: i32) -> Vec<u8> {
    format!("still waiting for news-server connections after {elapsed_sec} seconds").into_bytes()
}

/// Format `"still running after <max_sec> seconds, <max_run_factor> times the timeout"`
/// for `StreamRepairController::StartWatchdog` (`StreamRepair.cpp`).
pub fn dqh_stream_repair_format_max_run_reason(max_sec: i32, max_run_factor: i32) -> Vec<u8> {
    format!("still running after {max_sec} seconds, {max_run_factor} times the timeout")
        .into_bytes()
}

/// Format `"at <repair_mb_s:.2> MB/s the <remaining_mb:.1> MB left would take <repair_sec:.0> seconds, more than <slow_factor> times a download at <download_mb_s:.2> MB/s"`
/// for `StreamRepairController::StartWatchdog` (`StreamRepair.cpp`).
pub fn dqh_stream_repair_format_slow_rate_reason(
    repair_mb_s: f64,
    remaining_mb: f64,
    repair_sec: f64,
    slow_factor: i32,
    download_mb_s: f64,
) -> Vec<u8> {
    format!(
        "at {repair_mb_s:.2} MB/s the {remaining_mb:.1} MB left would take {repair_sec:.0} seconds, more than {slow_factor} times a download at {download_mb_s:.2} MB/s"
    )
    .into_bytes()
}

/// Format `"<prefix> <donor_name>"` (e.g. `"Repairing from duplicate <donor_name>"`)
/// for `StreamRepairController` stage progress labels (`StreamRepair.cpp`).
pub fn dqh_stream_repair_format_donor_progress_label(
    prefix: &[u8],
    donor_name: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + 1 + donor_name.len());
    out.extend_from_slice(prefix);
    out.push(b' ');
    out.extend_from_slice(donor_name);
    out
}

/// Select the message log kind (`mk_info` when `probed <= max_donor_candidates`,
/// `mk_detail` otherwise) when logging a rejected donor file in
/// `StreamRepairController::RepairFile` (`StreamRepair.cpp`).
pub fn dqh_stream_repair_select_reject_log_kind(
    probed: i32,
    max_donor_candidates: i32,
    mk_info: i32,
    mk_detail: i32,
) -> i32 {
    if probed <= max_donor_candidates {
        mk_info
    } else {
        mk_detail
    }
}

/// Return the NUL-terminated PAR disposition string (`"left to par-repair"` vs
/// `"no par2 files to repair it"`) for `StreamRepairController::ReportRemainingHoles`
/// (`StreamRepair.cpp`).
pub fn dqh_stream_repair_missing_par_disposition(has_par_files: bool) -> &'static [u8] {
    if has_par_files {
        b"left to par-repair\0"
    } else {
        b"no par2 files to repair it\0"
    }
}

/// Format `"<dest_dir><sep>.stream-decompress.<suffix>"` for
/// `StreamRepairController::ExecDecompressRepair` (`StreamRepair.cpp`).
pub fn dqh_stream_repair_format_decompress_temp_dir(
    dest_dir: &[u8],
    sep: u8,
    suffix: i32,
) -> Vec<u8> {
    let tail = format!(".stream-decompress.{suffix}");
    let mut out = Vec::with_capacity(dest_dir.len() + 1 + tail.len());
    out.extend_from_slice(dest_dir);
    out.push(sep);
    out.extend_from_slice(tail.as_bytes());
    out
}

/// Format `"<temp_dir><sep>extracted"` for
/// `StreamRepairController::ExecDecompressRepair` (`StreamRepair.cpp`).
pub fn dqh_stream_repair_format_extract_dir(temp_dir: &[u8], sep: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(temp_dir.len() + 10);
    out.extend_from_slice(temp_dir);
    out.push(sep);
    out.extend_from_slice(b"extracted");
    out
}

/// Return the NUL-terminated decompression failure reason (`"extraction failed"`
/// vs `"no extraction tool configured"`) for
/// `StreamRepairController::ExecDecompressRepair` (`StreamRepair.cpp`).
pub fn dqh_stream_repair_decompress_fail_reason(has_extractor: bool) -> &'static [u8] {
    if has_extractor {
        b"extraction failed\0"
    } else {
        b"no extraction tool configured\0"
    }
}

/// Formats `"member <name> unreadable"` for `ContentMapper::BuildSplitMap`.
pub fn dqh_content_map_format_member_unreadable_reason(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len() + 18);
    out.extend_from_slice(b"member ");
    out.extend_from_slice(name);
    out.extend_from_slice(b" unreadable");
    out
}

/// Returns `b""` or `b", size mismatch"` for `DupeMatcher::Prepare` and `DupeMatcher::MatchDupeContent`.
pub fn dqh_dupematcher_size_mismatch_suffix(ok: bool) -> &'static [u8] {
    if ok {
        b"\0"
    } else {
        b", size mismatch\0"
    }
}

/// Returns the static skip/warning message for `PostDownloadRenamer::Controller::Run`
/// and `PostUnpackRenamer::Controller::Run` when `plan_class` is `1`, `2`, or `3`.
pub fn dqh_post_renamer_plan_skip_static_msg(
    is_post_unpack: bool,
    plan_class: i32,
) -> &'static [u8] {
    if is_post_unpack {
        match plan_class {
            1 => b"Skipping Post-unpack renaming: disc structure detected\0",
            2 => b"Skipping Post-unpack renaming: ambiguous multi-file collection detected\0",
            3 => b"No qualifying media file found for Post-unpack renaming\0",
            _ => b"\0",
        }
    } else {
        match plan_class {
            1 => b"Skipping Post-download renaming: disc structure detected\0",
            2 => b"Skipping Post-download renaming: ambiguous multi-file collection detected\0",
            3 => b"No qualifying media file found for Post-download renaming\0",
            _ => b"\0",
        }
    }
}

/// Formats the final status message for `PostDownloadRenamer::Controller::Run`
/// and `PostUnpackRenamer::Controller::Run`.
pub fn dqh_post_renamer_format_final_status_msg(
    info_name: &[u8],
    final_decision: i32,
) -> Vec<u8> {
    match final_decision {
        1 => {
            let mut out = Vec::with_capacity(info_name.len() + 10);
            out.extend_from_slice(info_name);
            out.extend_from_slice(b" cancelled");
            out
        }
        2 => {
            let mut out = Vec::with_capacity(info_name.len() + 21);
            out.extend_from_slice(info_name);
            out.extend_from_slice(b" finished with errors");
            out
        }
        3 => {
            let mut out = Vec::with_capacity(info_name.len() + 11);
            out.extend_from_slice(info_name);
            out.extend_from_slice(b" successful");
            out
        }
        _ => {
            let mut out = Vec::with_capacity(info_name.len() + 29);
            out.extend_from_slice(b"No files needed renaming for ");
            out.extend_from_slice(info_name);
            out
        }
    }
}

/// Computes `file.m_dataOffset` via two's-complement subtraction for `RarVolume::ReadFrom`.
pub fn dqh_rar_volume_set_data_offset(position: i64, packed_size: i64) -> i64 {
    (position as u64).wrapping_sub(packed_size as u64) as i64
}

/// Formats the 3-byte little-endian round counter for `StreamCrypto::DeriveRar3`.
pub fn dqh_stream_crypto_format_rar3_round_bytes(round: i32) -> [u8; 3] {
    [
        (round & 0xff) as u8,
        ((round >> 8) & 0xff) as u8,
        ((round >> 16) & 0xff) as u8,
    ]
}

/// Returns the IV byte index (`0..16`) if `round` is an IV sampling round in `StreamCrypto::DeriveRar3`,
/// or `-1` otherwise.
pub fn dqh_stream_crypto_rar3_iv_index(round: i32, rounds: i32) -> i32 {
    let step = rounds / 16;
    if step > 0 && round >= 0 && round % step == 0 {
        round / step
    } else {
        -1
    }
}

/// Computes the read buffer capacity (`read_buf_size + 1 + decode_slack`) for `Connection::Connection`.
pub fn dqh_conn_calc_read_buffer_cap(read_buf_size: i32, decode_slack: i32) -> i32 {
    read_buf_size.saturating_add(1).saturating_add(decode_slack)
}

/// Formats `port` as a decimal byte string for `Connection::Bind` and `Connection::DoConnect`.
pub fn dqh_conn_format_port_str(port: i32) -> Vec<u8> {
    format!("{port}").into_bytes()
}

/// Clamps `buf_avail` to `>= 0` for `Connection::ReadLine`.
pub fn dqh_conn_clamp_buf_avail(buf_avail: i32) -> i32 {
    if buf_avail > 0 {
        buf_avail
    } else {
        0
    }
}

/// Formats `"<prefix>: Error <err_code> - <err_msg>"` for `Connection::ReportError`.
pub fn dqh_conn_format_report_err_with_code(
    prefix: &[u8],
    err_code: i32,
    err_msg: &[u8],
) -> Vec<u8> {
    let mid = format!(": Error {err_code} - ");
    let mut out = Vec::with_capacity(prefix.len() + mid.len() + err_msg.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(mid.as_bytes());
    out.extend_from_slice(err_msg);
    out
}

/// Formats `"<err_msg>: <err_str>"` for `TlsSocket::ReportError`.
pub fn dqh_tls_format_report_err(err_msg: &[u8], err_str: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(err_msg.len() + 2 + err_str.len());
    out.extend_from_slice(err_msg);
    out.extend_from_slice(b": ");
    out.extend_from_slice(err_str);
    out
}

/// Formats `"TLS certificate verification failed for <host>: <verify_err_str>. For more info visit https://nzbget.com/documentation/certificate-verification/"`
/// for `TlsSocket::Start` and `TlsSocket::ValidateCert`.
pub fn dqh_tls_format_cert_verify_err(host: &[u8], verify_err_str: &[u8]) -> Vec<u8> {
    const PREFIX: &[u8] = b"TLS certificate verification failed for ";
    const SUFFIX: &[u8] =
        b". For more info visit https://nzbget.com/documentation/certificate-verification/";
    let mut out = Vec::with_capacity(PREFIX.len() + host.len() + 2 + verify_err_str.len() + SUFFIX.len());
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(host);
    out.extend_from_slice(b": ");
    out.extend_from_slice(verify_err_str);
    out.extend_from_slice(SUFFIX);
    out
}

/// Formats `"TLS handshake failed for <host>"` for `TlsSocket::Start`.
pub fn dqh_tls_format_handshake_err(host: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(host.len() + 25);
    out.extend_from_slice(b"TLS handshake failed for ");
    out.extend_from_slice(host);
    out
}

/// Formats `"TLS certificate verification failed for <host>: certificate hostname mismatch (<cert_host>). For more info visit https://nzbget.com/documentation/certificate-verification/"`
/// for `TlsSocket::ValidateCert`.
pub fn dqh_tls_format_hostname_mismatch_err(host: &[u8], cert_host: &[u8]) -> Vec<u8> {
    const PREFIX: &[u8] = b"TLS certificate verification failed for ";
    const MID: &[u8] = b": certificate hostname mismatch (";
    const SUFFIX: &[u8] =
        b"). For more info visit https://nzbget.com/documentation/certificate-verification/";
    let mut out = Vec::with_capacity(
        PREFIX.len() + host.len() + MID.len() + cert_host.len() + SUFFIX.len(),
    );
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(host);
    out.extend_from_slice(MID);
    out.extend_from_slice(cert_host);
    out.extend_from_slice(SUFFIX);
    out
}

/// Selects the static TLS read/write error string for `TlsSocket::Send` and `TlsSocket::Recv`.
pub fn dqh_tls_select_io_err_msg(is_write: bool, peek_err_zero: bool) -> &'static [u8] {
    match (is_write, peek_err_zero) {
        (true, true) => b"Could not write to TLS-Socket: Connection closed by remote host\0",
        (true, false) => b"Could not write to TLS-Socket\0",
        (false, true) => b"Could not read from TLS-Socket: Connection closed by remote host\0",
        (false, false) => b"Could not read from TLS-Socket\0",
    }
}

/// Resolves `script` against `app_dir` if `script` does not start with `sep` for `Maintenance::StartUpdate`.
pub fn dqh_maintenance_resolve_script_path(app_dir: &[u8], sep: u8, script: &[u8]) -> Vec<u8> {
    if script.first().copied() != Some(sep) {
        let mut out = Vec::with_capacity(app_dir.len() + 1 + script.len());
        out.extend_from_slice(app_dir);
        out.push(sep);
        out.extend_from_slice(script);
        out
    } else {
        script.to_vec()
    }
}

/// Formats `"<web_dir><sep>package-info.json"` for `Maintenance::ReadPackageInfoStr`.
pub fn dqh_maintenance_format_package_info_path(web_dir: &[u8], sep: u8) -> Vec<u8> {
    const SUFFIX: &[u8] = b"package-info.json";
    let mut out = Vec::with_capacity(web_dir.len() + 1 + SUFFIX.len());
    out.extend_from_slice(web_dir);
    out.push(sep);
    out.extend_from_slice(SUFFIX);
    out
}

/// Formats `"update-script <base_name>"` or `"update-info-script <base_name>"`
/// for `UpdateScriptController::Run` and `UpdateInfoScriptController::ExecuteScript`.
pub fn dqh_maintenance_format_script_info_name(
    is_info_script: bool,
    base_name: &[u8],
) -> Vec<u8> {
    let prefix: &[u8] = if is_info_script {
        b"update-info-script "
    } else {
        b"update-script "
    };
    let mut out = Vec::with_capacity(prefix.len() + base_name.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(base_name);
    out
}

/// Returns `"STABLE"`, `"TESTING"`, or `"DEVEL"` for `UpdateScriptController::Run`.
pub fn dqh_maintenance_branch_name(branch: i32) -> &'static [u8] {
    match branch {
        1 => b"TESTING\0",
        2 => b"DEVEL\0",
        _ => b"STABLE\0",
    }
}

/// Returns `"DAEMON"` or `"SERVER"` for `UpdateScriptController::Run`.
pub fn dqh_maintenance_run_mode_name(daemon_mode: bool) -> &'static [u8] {
    if daemon_mode {
        b"DAEMON\0"
    } else {
        b"SERVER\0"
    }
}

/// Formats `"NZBUP_CMDLINE<index>"` for `UpdateScriptController::Run`.
pub fn dqh_maintenance_format_cmdline_env_name(index: i32) -> Vec<u8> {
    format!("NZBUP_CMDLINE{index}").into_bytes()
}

/// Returns `(log_prefix, prefix_len)` after bounding `base_name` to `max_len` bytes
/// and stripping the file extension for `UpdateScriptController::Run` and
/// `UpdateInfoScriptController::ExecuteScript`.
pub fn dqh_maintenance_strip_ext_and_calc_prefix_len(
    base_name: &[u8],
    max_len: usize,
) -> (Vec<u8>, usize) {
    let bounded = &base_name[..base_name.len().min(max_len)];
    let stem_len = bounded
        .iter()
        .rposition(|&b| b == b'.')
        .unwrap_or(bounded.len());
    (bounded[..stem_len].to_vec(), stem_len.saturating_add(2))
}

/// Formats `"\"RSA-SHA256(<base_name>)\" : \""` for `Signature::ReadSignature`.
pub fn dqh_maintenance_format_sig_title(base_name: &[u8]) -> Vec<u8> {
    const PREFIX: &[u8] = b"\"RSA-SHA256(";
    const SUFFIX: &[u8] = b")\" : \"";
    let mut out = Vec::with_capacity(PREFIX.len() + base_name.len() + SUFFIX.len());
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(base_name);
    out.extend_from_slice(SUFFIX);
    out
}

/// Converts bytes per second to KB/s (`bytes / 1024`) for `Scheduler::PrintLog`.
pub fn dqh_scheduler_bytes_to_kb(bytes: i32) -> i32 {
    bytes / 1024
}

/// Returns `"pausing"` or `"unpausing"` for `Scheduler::PrintLog`.
pub fn dqh_scheduler_pause_action_str(paused: bool) -> &'static [u8] {
    if paused {
        b"pausing\0"
    } else {
        b"unpausing\0"
    }
}

/// Returns `"activating"` or `"deactivating"` for `Scheduler::PrintLog`.
pub fn dqh_scheduler_server_action_str(active: bool) -> &'static [u8] {
    if active {
        b"activating\0"
    } else {
        b"deactivating\0"
    }
}

/// Returns `"par2-file"` or `"non-par2-file"` for `DirectRenamer::ArticleDownloaded`.
pub fn dqh_direct_renamer_par_kind_label(is_par: bool) -> &'static [u8] {
    if is_par {
        b"par2-file\0"
    } else {
        b"non-par2-file\0"
    }
}

/// Resolves the old output path for `DirectRenamer::RenameFilesInProgress`:
/// returns `curr_output_filename` when non-empty, otherwise `"<dest_dir><sep><filename>"`.
pub fn dqh_direct_renamer_resolve_old_output_path(
    curr_output_filename: &[u8],
    dest_dir: &[u8],
    sep: u8,
    filename: &[u8],
) -> Vec<u8> {
    if !curr_output_filename.is_empty() {
        return curr_output_filename.to_vec();
    }
    let mut out = Vec::with_capacity(dest_dir.len() + 1 + filename.len());
    out.extend_from_slice(dest_dir);
    out.push(sep);
    out.extend_from_slice(filename);
    out
}

/// Returns `"yes"` or `"no"` for boolean PP-parameter values in `Scanner::InitPPParameters`
/// and `QueueEditor::SetNzbCategory`.
pub fn dqh_scanner_yes_no_str(val: bool) -> &'static [u8] {
    if val {
        b"yes\0"
    } else {
        b"no\0"
    }
}

/// Formats `"<temp_dir><sep>nzb-<num>.tmp"` for `Scanner::AddExternalFile`.
pub fn dqh_scanner_format_temp_nzb_path(temp_dir: &[u8], sep: u8, num: i32) -> Vec<u8> {
    let suffix = format!("nzb-{num}.tmp");
    let mut out = Vec::with_capacity(temp_dir.len() + 1 + suffix.len());
    out.extend_from_slice(temp_dir);
    out.push(sep);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Formats `"<temp_dir><sep>url-<id>.tmp"` for `UrlCoordinator::StartUrlDownload`.
pub fn dqh_url_coord_format_temp_url_path(temp_dir: &[u8], sep: u8, id: i32) -> Vec<u8> {
    let suffix = format!("url-{id}.tmp");
    let mut out = Vec::with_capacity(temp_dir.len() + 1 + suffix.len());
    out.extend_from_slice(temp_dir);
    out.push(sep);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Formats `"<base_path>.new"` for `StateFile::StateFile` and `DiskState::SaveIdFloor`.
pub fn dqh_diskstate_format_new_path(base_path: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(base_path.len() + 4);
    out.extend_from_slice(base_path);
    out.extend_from_slice(b".new");
    out
}

/// Formats `"nzb <nzb_id> file <file_id>\n"` for `DiskState::SaveIdFloor`.
pub fn dqh_diskstate_format_ids_line(nzb_id: i32, file_id: i32) -> Vec<u8> {
    format!("nzb {nzb_id} file {file_id}\n").into_bytes()
}

/// Returns legacy suffix NUL-terminated string (`"s\0"`, `"c\0"`, or `"\0"`) for `DiskState::ConvertLegacyFileStates`.
pub fn dqh_diskstate_legacy_suffix_str(suffix_kind: i32) -> &'static [u8] {
    match suffix_kind {
        1 => b"s\0",
        2 => b"c\0",
        _ => b"\0",
    }
}

/// Formats `"<filename>.unreadable-<now>"` for `DiskState::SetAsideQueueFiles`.
pub fn dqh_diskstate_format_unreadable_path(filename: &[u8], now: i64) -> Vec<u8> {
    let suffix = format!(".unreadable-{now}");
    let mut out = Vec::with_capacity(filename.len() + suffix.len());
    out.extend_from_slice(filename);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Formats `"<dest_dir><sep><file_id>.out.tmp"` for `DiskState::LoadFileInfo`, `DiskState::LoadFileState`, and `QueueCoordinator::LoadPartialState`.
pub fn dqh_diskstate_format_out_tmp_path(dest_dir: &[u8], sep: u8, file_id: i32) -> Vec<u8> {
    let suffix = format!("{file_id}.out.tmp");
    let mut out = Vec::with_capacity(dest_dir.len() + 1 + suffix.len());
    out.extend_from_slice(dest_dir);
    out.push(sep);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Formats `"<temp_dir><sep><file_id>.<part_num:03>"` for `DiskState::LoadFileState`.
pub fn dqh_diskstate_format_staged_part_path(
    temp_dir: &[u8],
    sep: u8,
    file_id: i32,
    part_num: i32,
) -> Vec<u8> {
    let suffix = if part_num < 0 {
        format!("{file_id}.-{:03}", part_num.unsigned_abs())
    } else {
        format!("{file_id}.{part_num:03}")
    };
    let mut out = Vec::with_capacity(temp_dir.len() + 1 + suffix.len());
    out.extend_from_slice(temp_dir);
    out.push(sep);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Formats `"<file_id>c"` or `"<file_id>s"` for `DiskState::SaveFileState` and `DiskState::LoadFileState`.
pub fn dqh_diskstate_format_file_state_name(file_id: i32, completed: bool) -> Vec<u8> {
    let ch = if completed { 'c' } else { 's' };
    format!("{file_id}{ch}").into_bytes()
}

/// Formats `"<queue_dir><sep>n<nzb_id>.log"` for `DiskState::DiscardFiles`, `DiskState::AppendNzbMessage`, and `DiskState::LoadNzbMessages`.
pub fn dqh_diskstate_format_nzb_log_path(queue_dir: &[u8], sep: u8, nzb_id: i32) -> Vec<u8> {
    let suffix = format!("n{nzb_id}.log");
    let mut out = Vec::with_capacity(queue_dir.len() + 1 + suffix.len());
    out.extend_from_slice(queue_dir);
    out.push(sep);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Formats `"STAT <message_id>\r\n"` for `DupeProbe::ProbeServer`.
pub fn dqh_dupe_probe_format_stat_cmd(message_id: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + message_id.len() + 2);
    out.extend_from_slice(b"STAT ");
    out.extend_from_slice(message_id);
    out.extend_from_slice(b"\r\n");
    out
}

/// Returns `"; retrying them"` or `"; not retrying"` for `DupeProbe::Recheck`.
pub fn dqh_dupe_probe_recheck_retry_suffix(retry: bool) -> &'static [u8] {
    if retry {
        b"; retrying them\0"
    } else {
        b"; not retrying\0"
    }
}

/// Returns `""` or `", found on "` for `DupeProbe::Run`.
pub fn dqh_dupe_probe_found_on_prefix(found_on_empty: bool) -> &'static [u8] {
    if found_on_empty {
        b"\0"
    } else {
        b", found on \0"
    }
}

/// Computes `max(0, size - par_size)` for `ParDamage::Judge`.
pub fn dqh_par_damage_calc_total_data_bytes(size: i64, par_size: i64) -> i64 {
    (size - par_size).max(0)
}

/// Returns `"alt"` (`verdict == 2`) or `"twin"` for `TwinCheckJob::Sample`.
pub fn dqh_twin_check_sample_verdict_label(verdict: i32) -> &'static [u8] {
    if verdict == 2 {
        b"alt\0"
    } else {
        b"twin\0"
    }
}

/// Formats `val` as a decimal string for `TwinCheck::Posting`.
pub fn dqh_twin_check_format_u32_str(val: u32) -> Vec<u8> {
    val.to_string().into_bytes()
}

/// Lowercases bytes in `dupe_key` via `releasename::lower` (`libc` `tolower`) for `TwinCheck::ServiceWork`.
pub fn dqh_twin_check_lowercase_key(dupe_key: &[u8]) -> Vec<u8> {
    crate::releasename::lower(dupe_key)
}

/// Formats `"Joining from <piece_name>"` for `UnpackController::JoinFile`.
pub fn dqh_unpack_ctrl_format_joining_progress(piece_name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(13 + piece_name.len());
    out.extend_from_slice(b"Joining from ");
    out.extend_from_slice(piece_name);
    out
}

/// Strips the last `.ext` from `frag_base_name` for `UnpackController::JoinFile`.
pub fn dqh_unpack_ctrl_strip_last_extension(frag_base_name: &[u8]) -> Vec<u8> {
    match frag_base_name.iter().rposition(|&b| b == b'.') {
        Some(idx) => frag_base_name[..idx].to_vec(),
        None => frag_base_name.to_vec(),
    }
}

/// Formats `"Downloading duplicate <donor_name> (volume <member_num>/<total_members>, article <delivered_articles>/<total_articles>, <mb:.1> MB)"`
/// for `StreamRepairController::MaterializeDonorSet`.
pub fn dqh_stream_repair_format_materialize_progress(
    donor_name: &[u8],
    member_num: i32,
    total_members: i32,
    delivered_articles: i32,
    total_articles: i32,
    total_bytes: i64,
) -> Vec<u8> {
    let mb = dqh_stream_repair_bytes_to_mb(total_bytes);
    let suffix = format!(
        " (volume {member_num}/{total_members}, article {delivered_articles}/{total_articles}, {mb:.1} MB)"
    );
    let mut out = Vec::with_capacity(22 + donor_name.len() + suffix.len());
    out.extend_from_slice(b"Downloading duplicate ");
    out.extend_from_slice(donor_name);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Formats `"BODY <message_id>\r\n"` for `ArticleFetcher::FetchOnConnection`.
pub fn dqh_article_fetcher_format_body_cmd(message_id: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + message_id.len() + 2);
    out.extend_from_slice(b"BODY ");
    out.extend_from_slice(message_id);
    out.extend_from_slice(b"\r\n");
    out
}

/// Formats `" (forwarded for: <forwarded_for>)"` when non-empty else `""` for `WebProcessor::CheckCredentials`.
pub fn dqh_webserver_format_forwarded_for_suffix(forwarded_for: &[u8]) -> Vec<u8> {
    if forwarded_for.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(17 + forwarded_for.len() + 1);
    out.extend_from_slice(b" (forwarded for: ");
    out.extend_from_slice(forwarded_for);
    out.push(b')');
    out
}

/// Formats `"script <script_name> with command <command>"` or `"Script <script_name> with command <command>"`
/// for `CommandScriptController::ExecuteScript`.
pub fn dqh_cmd_script_format_info_name(
    script_name: &[u8],
    command: &[u8],
    uppercase: bool,
) -> Vec<u8> {
    let prefix: &[u8] = if uppercase { b"Script " } else { b"script " };
    let mut out = Vec::with_capacity(prefix.len() + script_name.len() + 14 + command.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(script_name);
    out.extend_from_slice(b" with command ");
    out.extend_from_slice(command);
    out
}

/// Formats `".<leaf>.tls-<pid>-<seq>"` for `ScriptConfig.cpp`.
pub fn dqh_script_config_format_tls_temp_name(leaf: &[u8], pid: i32, seq: u64) -> Vec<u8> {
    let suffix = format!(".tls-{pid}-{seq}");
    let mut out = Vec::with_capacity(1 + leaf.len() + suffix.len());
    out.push(b'.');
    out.extend_from_slice(leaf);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Formats `"DupeSearch <label> page <page_one_based>"` for `Newznab::Search`.
pub fn dqh_newznab_format_search_info_name(label: &[u8], page_one_based: i32) -> Vec<u8> {
    let suffix = format!(" page {page_one_based}");
    let mut out = Vec::with_capacity(11 + label.len() + suffix.len());
    out.extend_from_slice(b"DupeSearch ");
    out.extend_from_slice(label);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Formats `"<param_name>=<val>"` for `DupeSearch`.
pub fn dqh_dupesearch_format_key_val_param(param_name: &[u8], val: i32) -> Vec<u8> {
    let val_str = format!("={val}");
    let mut out = Vec::with_capacity(param_name.len() + val_str.len());
    out.extend_from_slice(param_name);
    out.extend_from_slice(val_str.as_bytes());
    out
}

/// Formats `"<queue_dir><sep>dupesearch-pending<sep><nzb_id>"` when `nzb_id > 0`
/// else `"<queue_dir><sep>dupesearch-pending"` for `DupeSearch::PendingDir`.
pub fn dqh_dupesearch_format_pending_dir(queue_dir: &[u8], sep: u8, nzb_id: i32) -> Vec<u8> {
    let mut out = Vec::with_capacity(queue_dir.len() + 32);
    out.extend_from_slice(queue_dir);
    out.push(sep);
    out.extend_from_slice(b"dupesearch-pending");
    if nzb_id > 0 {
        out.push(sep);
        out.extend_from_slice(nzb_id.to_string().as_bytes());
    }
    out
}

/// Formats `"<dir><sep><idx>"` for `DupeSearch::SavePending` and `DupeSearch::ResumePending`.
pub fn dqh_dupesearch_format_indexed_base_path(dir: &[u8], sep: u8, idx: usize) -> Vec<u8> {
    let suffix = idx.to_string();
    let mut out = Vec::with_capacity(dir.len() + 1 + suffix.len());
    out.extend_from_slice(dir);
    out.push(sep);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Formats `idx` as a decimal string for `DupeSearch`.
pub fn dqh_dupesearch_format_usize_str(idx: usize) -> Vec<u8> {
    idx.to_string().into_bytes()
}

/// Formats `"HTTP <status>"` for `NzbFetcher::Fetch`.
pub fn dqh_nzb_fetcher_format_http_status_err(status: i32) -> Vec<u8> {
    format!("HTTP {status}").into_bytes()
}

/// Formats `"<temp_dir><sep>dupesearch-<now_sec>-<counter>.tmp"` for `HttpGet::Fetch`.
pub fn dqh_http_get_format_temp_path(
    temp_dir: &[u8],
    sep: u8,
    now_sec: i32,
    counter: i32,
) -> Vec<u8> {
    let suffix = format!("dupesearch-{now_sec}-{counter}.tmp");
    let mut out = Vec::with_capacity(temp_dir.len() + 1 + suffix.len());
    out.extend_from_slice(temp_dir);
    out.push(sep);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Computes `max(0, deadline_ms - now_ms - finish_reserve_ms - 1000)` for `Fleet::Run`.
pub fn dqh_fleet_calc_check_lock_wait_ms(
    deadline_ms: i64,
    now_ms: i64,
    finish_reserve_ms: i64,
) -> i64 {
    deadline_ms
        .saturating_sub(now_ms)
        .saturating_sub(finish_reserve_ms)
        .saturating_sub(1000)
        .max(0)
}

/// Formats `"<name>=<value>"` for `ScriptController::SetEnvVar`.
pub fn dqh_script_ctrl_format_env_assignment(name: &[u8], value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len() + 1 + value.len());
    out.extend_from_slice(name);
    out.push(b'=');
    out.extend_from_slice(value);
    out
}

/// Formats `"<prefix>_<name>"` for `ScriptController::SetEnvVarSpecial`.
pub fn dqh_script_ctrl_format_prefixed_varname(prefix: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + 1 + name.len());
    out.extend_from_slice(prefix);
    out.push(b'_');
    out.extend_from_slice(name);
    out
}

/// Formats `"<msg> (<base_filename>:<line_nr>:<funcname>)"` or `"<msg> (<base_filename>:<line_nr>)"` for `debug()`.
pub fn dqh_log_format_debug_line(
    msg: &[u8],
    base_filename: &[u8],
    line_nr: i32,
    funcname: Option<&[u8]>,
) -> Vec<u8> {
    let line_str = format!(":{line_nr}");
    let mut out = Vec::with_capacity(
        msg.len() + 3 + base_filename.len() + line_str.len() + funcname.map_or(0, |f| 1 + f.len()),
    );
    out.extend_from_slice(msg);
    out.extend_from_slice(b" (");
    out.extend_from_slice(base_filename);
    out.extend_from_slice(line_str.as_bytes());
    if let Some(fn_bytes) = funcname {
        out.push(b':');
        out.extend_from_slice(fn_bytes);
    }
    out.push(b')');
    out
}

/// Formats `"<base_name>-####-##-##<base_ext>"` for `Log::RotateLog`.
pub fn dqh_log_format_rotate_mask(base_name: &[u8], base_ext: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(base_name.len() + 11 + base_ext.len());
    out.extend_from_slice(base_name);
    out.extend_from_slice(b"-####-##-##");
    out.extend_from_slice(base_ext);
    out
}

/// Formats `"Deleting old log-file <filename>\n"` for `Log::RotateLog`.
pub fn dqh_log_format_deleting_old_msg(filename: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(22 + filename.len() + 1);
    out.extend_from_slice(b"Deleting old log-file ");
    out.extend_from_slice(filename);
    out.push(b'\n');
    out
}

/// Formats `"<directory><sep><base_name>-<year>-<month:02>-<mday:02><base_ext>"` for `Log::RotateLog`.
pub fn dqh_log_format_rotated_path(
    directory: &[u8],
    sep: u8,
    base_name: &[u8],
    year: i32,
    month: i32,
    mday: i32,
    base_ext: &[u8],
) -> Vec<u8> {
    let date_part = format!("-{year}-{month:02}-{mday:02}");
    let mut out = Vec::with_capacity(
        directory.len() + 1 + base_name.len() + date_part.len() + base_ext.len(),
    );
    out.extend_from_slice(directory);
    out.push(sep);
    out.extend_from_slice(base_name);
    out.extend_from_slice(date_part.as_bytes());
    out.extend_from_slice(base_ext);
    out
}

/// Formats `"<prefix><idx>.<field>"` for `Options::InitServers`, `Options::InitCategories`,
/// `Options::InitFeeds`, and `Options::InitScheduler`.
pub fn dqh_options_format_indexed_opt_name(prefix: &[u8], idx: i32, field: &[u8]) -> Vec<u8> {
    let idx_str = idx.to_string();
    let mut out = Vec::with_capacity(prefix.len() + idx_str.len() + 1 + field.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(idx_str.as_bytes());
    out.push(b'.');
    out.extend_from_slice(field);
    out
}

/// Formats `"<pid>\n"` for `NZBGet::Daemonize`.
pub fn dqh_nzbget_format_pid_line(pid: i32) -> Vec<u8> {
    format!("{pid}\n").into_bytes()
}

/// Formats the padded/truncated line buffer for `NCursesFrontend::PlotLine`.
pub fn dqh_ncurses_format_plot_line(
    text: &[u8],
    screen_width: i32,
    pos: i32,
    max_screen_width: i32,
) -> Vec<u8> {
    let w = screen_width.max(0) as usize;
    let mut out = Vec::with_capacity(w);
    let copy_len = text.len().min(w);
    out.extend_from_slice(&text[..copy_len]);
    if copy_len < w {
        out.resize(w, b' ');
    }
    let size = (w + 1) as i64;
    let rem = (screen_width - pos) as i64;
    if size > rem && rem < max_screen_width as i64 {
        let target_len = rem.max(0) as usize;
        out.truncate(target_len);
    }
    out
}

/// Formats `"<cur_dir>/<rel_filename>"` for `FileSystem::ExpandFileName`.
pub fn dqh_fs_format_expanded_rel_path(cur_dir: &[u8], rel_filename: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(cur_dir.len() + 1 + rel_filename.len());
    out.extend_from_slice(cur_dir);
    out.push(b'/');
    out.extend_from_slice(rel_filename);
    out
}

/// Formats `"nzbget_disktest_<epoch_ticks>.bin"` for `Benchmark::GetUniqueFilename`.
pub fn dqh_benchmark_format_unique_filename(epoch_ticks: i64) -> Vec<u8> {
    format!("nzbget_disktest_{epoch_ticks}.bin").into_bytes()
}

/// Resolves the benchmark block size (`default_block_size` when `block_size_bytes == 0`).
pub fn dqh_benchmark_resolve_block_size(
    block_size_bytes: usize,
    default_block_size: usize,
) -> usize {
    if block_size_bytes == 0 {
        default_block_size
    } else {
        block_size_bytes
    }
}

/// Formats `"[first_idx]=v0 [first_idx+1]=v1 ... "` for `ServerVolume::LogDebugInfo`.
pub fn dqh_statmeter_format_indexed_bytes_line(first_idx: i32, values: &[i64]) -> Vec<u8> {
    let mut out = String::with_capacity(values.len() * 12);
    for (i, &val) in values.iter().enumerate() {
        use std::fmt::Write;
        let _ = write!(out, "[{}]={val} ", first_idx.wrapping_add(i as i32));
    }
    out.into_bytes()
}

/// Formats `"<part>=<offset>-<size>"` for `NntpProcessor::SendSegment`.
pub fn dqh_nntpserver_format_cache_file_name(part: i32, offset: i64, size: i32) -> Vec<u8> {
    format!("{part}={offset}-{size}").into_bytes()
}

/// Formats `"<data_dir><sep><base_name>.nzb"` for `NzbGenerator::GenerateNzb`.
pub fn dqh_nzbgen_format_nzb_output_path(data_dir: &[u8], sep: u8, base_name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data_dir.len() + 1 + base_name.len() + 4);
    out.extend_from_slice(data_dir);
    out.push(sep);
    out.extend_from_slice(base_name);
    out.extend_from_slice(b".nzb");
    out
}

/// Formats `"LogFile=<log_file>"` for `NServMain`.
pub fn dqh_nserv_format_logfile_opt(log_file: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + log_file.len());
    out.extend_from_slice(b"LogFile=");
    out.extend_from_slice(log_file);
    out
}

/// Formats `"Malformed nzb-file, tag <<tag_name>> must have attributes"` for `NzbFile::Parse_StartElement`.
pub fn dqh_nzbfile_format_missing_attrs_msg(tag_name: &[u8]) -> Vec<u8> {
    let prefix = b"Malformed nzb-file, tag <";
    let suffix = b"> must have attributes";
    let mut out = Vec::with_capacity(prefix.len() + tag_name.len() + suffix.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(tag_name);
    out.extend_from_slice(suffix);
    out
}

/// Classifies an XML start element in `NzbFile::Parse_StartElement`:
/// `1` (`"file"`), `2` (`"segment"`), `3` (`"meta"`), or `0` (other).
pub fn dqh_nzbfile_classify_start_element(name: &[u8]) -> i32 {
    match name {
        b"file" => 1,
        b"segment" => 2,
        b"meta" => 3,
        _ => 0,
    }
}

/// Classifies a `<file>` attribute name in `NzbFile::Parse_StartElement`:
/// `1` (`"subject"`), `2` (`"date"`), or `0` (other).
pub fn dqh_nzbfile_classify_file_attr(attr_name: &[u8]) -> i32 {
    match attr_name {
        b"subject" => 1,
        b"date" => 2,
        _ => 0,
    }
}

/// Classifies a `<segment>` attribute name in `NzbFile::Parse_StartElement`:
/// `1` (`"bytes"`), `2` (`"number"`), or `0` (other).
pub fn dqh_nzbfile_classify_segment_attr(attr_name: &[u8]) -> i32 {
    match attr_name {
        b"bytes" => 1,
        b"number" => 2,
        _ => 0,
    }
}

/// Classifies an XML end element in `NzbFile::Parse_EndElement`:
/// `1` (`"file"`), `2` (`"group"`), `3` (`"segment"`), `4` (`"meta"`), or `0` (other).
pub fn dqh_nzbfile_classify_end_element(name: &[u8]) -> i32 {
    match name {
        b"file" => 1,
        b"group" => 2,
        b"segment" => 3,
        b"meta" => 4,
        _ => 0,
    }
}

/// Formats `"<major>.<minor>.<patch>"` from a packed 24-bit version integer for `SystemInfo`.
pub fn dqh_sysinfo_format_rapidyenc_version(ver: i32) -> Vec<u8> {
    let major = (ver >> 16) & 0xFF;
    let minor = (ver >> 8) & 0xFF;
    let patch = ver & 0xFF;
    format!("{major}.{minor}.{patch}").into_bytes()
}

/// Formats `"could not delete (null): <last_err>"` for `FileSystem::DeleteDirectoryWithContent`.
pub fn dqh_fs_format_null_delete_errmsg(last_err: &[u8]) -> Vec<u8> {
    let prefix = b"could not delete (null): ";
    let mut out = Vec::with_capacity(prefix.len() + last_err.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(last_err);
    out
}

/// Extracts the parent directory path up to the last `sep` (or the whole `filename` if no `sep` exists)
/// for `FileSystem::FlushDirBuffers`.
pub fn dqh_fs_extract_parent_dir_path(filename: &[u8], sep: u8) -> Vec<u8> {
    match filename.iter().rposition(|&b| b == sep) {
        Some(pos) => filename[..pos].to_vec(),
        None => filename.to_vec(),
    }
}

/// Classifies an XML start element in `FeedFile::Parse_StartElement`:
/// `1` (`"item"`), `2` (`"enclosure"`), or `0` (other).
pub fn dqh_feed_file_classify_start_element(name: &[u8]) -> i32 {
    match name {
        b"item" => 1,
        b"enclosure" => 2,
        _ => 0,
    }
}

/// Returns `true` when `line` starts with `"X-DNZB-"` for `UrlDownloader::ProcessHeader`.
pub fn dqh_url_dl_has_dnzb_prefix(line: &[u8]) -> bool {
    line.starts_with(b"X-DNZB-")
}

/// Returns `true` when `filename` is `<digits>`, `<digits>s`, or `<digits>c` for `DiskState::SetAsideQueueFiles`.
pub fn dqh_diskstate_is_candidate_queue_file_name(filename: &[u8]) -> bool {
    let digits = filename.iter().take_while(|&&b| b.is_ascii_digit()).count();
    if digits == 0 {
        return false;
    }
    matches!(&filename[digits..], b"" | b"s" | b"c")
}

/// Finds the first `sep` byte in `line` and returns its byte index (`Some(pos)`), or `None` if absent.
pub fn dqh_diskstate_find_char_pos(line: &[u8], sep: u8) -> Option<usize> {
    line.iter().position(|&b| b == sep)
}

/// Returns `true` when `ver == b"TLSv1.3"` for `TlsCipherBackend::Suites` and `TlsSocket::ApplyCipherPin`.
pub fn dqh_tls_is_tls13_version_str(ver: &[u8]) -> bool {
    ver == b"TLSv1.3"
}

/// Returns `true` when `cipher_name` contains `"CCM8"`.
pub fn dqh_tls_cipher_has_ccm8(cipher_name: &[u8]) -> bool {
    cipher_name.windows(4).any(|w| w == b"CCM8")
}

/// Resolves effective cipher bits (`64` when `cipher_name` contains `"CCM8"`, else `raw_bits`).
pub fn dqh_tls_resolve_cipher_bits(cipher_name: &[u8], raw_bits: u32) -> u32 {
    if dqh_tls_cipher_has_ccm8(cipher_name) {
        64
    } else {
        raw_bits
    }
}

/// Formats the bulk cipher name (`bulk_name` or `"unavailable"`, plus `"@8"` if `cipher_name` contains `"CCM8"`).
pub fn dqh_tls_format_bulk_cipher_name(
    bulk_name: Option<&[u8]>,
    cipher_name: &[u8],
) -> Vec<u8> {
    let base = bulk_name.unwrap_or(b"unavailable");
    let has_ccm8 = dqh_tls_cipher_has_ccm8(cipher_name);
    let mut out = Vec::with_capacity(base.len() + if has_ccm8 { 2 } else { 0 });
    out.extend_from_slice(base);
    if has_ccm8 {
        out.extend_from_slice(b"@8");
    }
    out
}

/// Computes the 3-bit TLS suite capability mask (`1 = AEAD`, `2 = cert auth`, `4 = ephemeral KX`).
pub fn dqh_tls_compute_suite_flags(
    is_aead: bool,
    tls13: bool,
    auth_is_cert: bool,
    kx_is_ephemeral: bool,
) -> u32 {
    let mut flags = 0u32;
    if is_aead {
        flags |= 1;
    }
    if tls13 || auth_is_cert {
        flags |= 2;
    }
    if tls13 || kx_is_ephemeral {
        flags |= 4;
    }
    flags
}

/// Returns `true` when `!active_group.is_empty() && active_group == grp` for `NntpConnection::JoinGroup`.
pub fn dqh_nntp_conn_is_already_in_group(active_group: &[u8], grp: &[u8]) -> bool {
    !active_group.is_empty() && active_group == grp
}

/// Returns `true` when `!no_disk_access` and `value` starts with `"~/"` for `Options::SetOption`.
pub fn dqh_options_should_expand_home_value(value: Option<&[u8]>, no_disk_access: bool) -> bool {
    !no_disk_access && value.is_some_and(|v| v.starts_with(b"~/"))
}

/// Returns `true` when `username == b"root"` for `NZBGet::Daemonize`.
pub fn dqh_nzbget_is_backward_compat_root_user(username: &[u8]) -> bool {
    username == b"root"
}

/// Returns `true` when `is_xmlrpc_proto && is_http_post` and `method_name` matches `"system.multicall"`
/// case-insensitively (`dqh_bytes_eq_ignore_case`) for `XmlRpcProcessor::Execute`.
pub fn dqh_xmlrpc_is_multicall_post(
    method_name: &[u8],
    is_xmlrpc_proto: bool,
    is_http_post: bool,
) -> bool {
    is_xmlrpc_proto
        && is_http_post
        && dqh_bytes_eq_ignore_case(method_name, b"system.multicall")
}

/// Returns `true` when `name == b"_TlsCipherBaseline"` for `SaveConfigXmlCommand::Execute`.
pub fn dqh_xmlrpc_is_tls_cipher_baseline_entry(name: &[u8]) -> bool {
    name == b"_TlsCipherBaseline"
}

/// Returns `true` when `content_type == Some(b"text/html")` for `WebProcessor::SendFileResponse`.
pub fn dqh_webserver_is_html_content_type(content_type: Option<&[u8]>) -> bool {
    content_type == Some(b"text/html")
}

/// Finds the last occurrence of `ch` in `text` and returns its byte index (`Some(pos)`), or `None`.
pub fn dqh_scanner_find_last_char_pos(text: &[u8], ch: u8) -> Option<usize> {
    text.iter().rposition(|&b| b == ch)
}

/// Splits `base_name` at the last `.` (when `pos > 0`) into `(stem, ext_with_dot)` for `Log::RotateLog`.
pub fn dqh_log_split_basename_and_ext(base_name: &[u8]) -> (Vec<u8>, Vec<u8>) {
    match base_name.iter().rposition(|&b| b == b'.') {
        Some(pos) if pos > 0 => (base_name[..pos].to_vec(), base_name[pos..].to_vec()),
        _ => (base_name.to_vec(), Vec::new()),
    }
}

/// Returns `true` when `line` contains `marker` as a contiguous byte slice for `SystemInfo::GetUnpackerVersion`.
pub fn dqh_sysinfo_line_contains_marker(line: &[u8], marker: &[u8]) -> bool {
    if marker.is_empty() {
        return true;
    }
    line.windows(marker.len()).any(|w| w == marker)
}

/// Returns `true` when byte slices `a` and `b` are equal (`strcmp(a, b) == 0`).
pub fn dqh_bytes_equal(a: &[u8], b: &[u8]) -> bool {
    a == b
}

/// Returns `true` when byte slices `a` and `b` are equal under `releasename::c_lower` (`strcasecmp(a, b) == 0`)
/// without allocating temporary buffers and stopping at the first mismatch.
pub fn dqh_bytes_eq_ignore_case(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(&x, &y)| {
            x == y || crate::releasename::c_lower(x) == crate::releasename::c_lower(y)
        })
}

/// Classifies an XML start element in `Newznab::PageReader::Start`:
/// `1` (`depth == 1 && name == "error"`), `2` (`"item"`), `3` (`in_item && name == "attr"`), or `0` (other).
pub fn dqh_newznab_classify_start_element(depth: i32, in_item: bool, name: &[u8]) -> i32 {
    if depth == 1 && name == b"error" {
        1
    } else if name == b"item" {
        2
    } else if in_item && name == b"attr" {
        3
    } else {
        0
    }
}

/// Classifies a `<newznab:attr>` `name` attribute in `Newznab::PageReader::Start`:
/// `1` (`"size"`), `2` (`"grabs"`), `3` (`"usenetdate"`), `4` (`"hydraIndexerName"`), or `0` (other).
pub fn dqh_newznab_classify_item_attr_key(key: &[u8]) -> i32 {
    match key {
        b"size" => 1,
        b"grabs" => 2,
        b"usenetdate" => 3,
        b"hydraIndexerName" => 4,
        _ => 0,
    }
}

/// Classifies an XML end element in `Newznab::PageReader::End` when `in_item` is `true`:
/// `1` (`"title"`), `2` (`"link"`), `3` (`"size"`), `4` (`"item"`), or `0` (other / `!in_item`).
pub fn dqh_newznab_classify_end_element(in_item: bool, name: &[u8]) -> i32 {
    if !in_item {
        return 0;
    }
    match name {
        b"title" => 1,
        b"link" => 2,
        b"size" => 3,
        b"item" => 4,
        _ => 0,
    }
}

/// Classifies an XML start element in `NzbReader::Reader::Start`:
/// `1` (`"meta"`), `2` (`"file"`), `3` (`"segment"` when `in_file`), or `0` (other).
pub fn dqh_nzb_reader_classify_start_element(name: &[u8], in_file: bool) -> i32 {
    match name {
        b"meta" => 1,
        b"file" => 2,
        b"segment" if in_file => 3,
        _ => 0,
    }
}

/// Classifies an XML end element in `NzbReader::Reader::End`:
/// `1` (`"meta"` when `has_meta_type`), `2` (`"segment"` when `in_segment`),
/// `3` (`"group"` when `in_file`), `4` (`"file"`), or `0` (other).
pub fn dqh_nzb_reader_classify_end_element(
    name: &[u8],
    has_meta_type: bool,
    in_segment: bool,
    in_file: bool,
) -> i32 {
    match name {
        b"meta" if has_meta_type => 1,
        b"segment" if in_segment => 2,
        b"group" if in_file => 3,
        b"file" => 4,
        _ => 0,
    }
}

/// Lexicographically compares byte slices `a` and `b` (`-1`, `0`, or `1`).
pub fn dqh_bytes_cmp(a: &[u8], b: &[u8]) -> i32 {
    match a.cmp(b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Classifies how `donor_filename` matches `by_content` in `DupeArticleFallback::MatchDonorFile`:
/// `1` (exact byte match), `2` (case-insensitive match via `dqh_bytes_eq_ignore_case`), or `0` (no match).
pub fn dqh_dupe_fallback_classify_content_name_match(
    donor_filename: &[u8],
    by_content: &[u8],
) -> i32 {
    if donor_filename == by_content {
        1
    } else if dqh_bytes_eq_ignore_case(donor_filename, by_content) {
        2
    } else {
        0
    }
}

/// Resolves the probe outcome in `TlsCipherBackend::Probe`.
pub fn dqh_tls_resolve_probe_result(
    connected: bool,
    negotiated_cipher: &[u8],
    expected_cipher: &[u8],
    negotiated_version: i32,
    protocol: i32,
    tls13_ver: i32,
    tls12_ver: i32,
    auth_error: bool,
    tls_probe_failure: i32,
) -> i32 {
    let expected_ver = if protocol == 13 { tls13_ver } else { tls12_ver };
    if connected && negotiated_cipher == expected_cipher && negotiated_version == expected_ver {
        1
    } else if auth_error {
        -5
    } else {
        tls_probe_failure
    }
}

/// Returns `Some(sig_title.len())` when `line` starts with `sig_title`, or `None` otherwise,
/// for `Signature::ReadSignature`.
pub fn dqh_maintenance_match_sig_line_prefix(line: &[u8], sig_title: &[u8]) -> Option<usize> {
    if line.starts_with(sig_title) {
        Some(sig_title.len())
    } else {
        None
    }
}

/// Parses a leading signed base-10 integer from `text` following C `atoll` / `strtoll(..., 10)`
/// semantics (skips leading ASCII whitespace, optional `+`/`-`, consecutive decimal digits).
pub fn dqh_parse_c_atoll(text: &[u8]) -> i64 {
    let mut i = 0;
    while i < text.len() && matches!(text[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut neg = false;
    if i < text.len() {
        if text[i] == b'-' {
            neg = true;
            i += 1;
        } else if text[i] == b'+' {
            i += 1;
        }
    }
    let mut acc: u64 = 0;
    let mut any = false;
    while i < text.len() && text[i].is_ascii_digit() {
        any = true;
        acc = acc
            .saturating_mul(10)
            .saturating_add(u64::from(text[i] - b'0'));
        i += 1;
    }
    if !any {
        return 0;
    }
    if neg {
        let max_neg = (i64::MAX as u64) + 1;
        if acc >= max_neg {
            i64::MIN
        } else {
            -(acc as i64)
        }
    } else {
        acc.min(i64::MAX as u64) as i64
    }
}

/// Parses a leading signed base-10 integer from `text` following C `atoi` semantics.
pub fn dqh_parse_c_atoi(text: &[u8]) -> i32 {
    dqh_parse_c_atoll(text) as i32
}

/// Parses a leading signed base-10 integer from `text` when `Some`, or returns `default_val` when `None`.
pub fn dqh_parse_c_atoi_or_default(text: Option<&[u8]>, default_val: i32) -> i32 {
    match text {
        Some(s) => dqh_parse_c_atoi(s),
        None => default_val,
    }
}

/// Parses `(id, suffix)` from a queue state filename following `sscanf(filename, "%i%c", &id, &suffix) == 2`
/// semantics in `DiskState::LoadAllFileStates`.
pub fn dqh_diskstate_parse_queue_file_id_and_suffix(filename: &[u8]) -> Option<(i32, u8)> {
    let mut i = 0;
    while i < filename.len() && matches!(filename[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut neg = false;
    if i < filename.len() {
        if filename[i] == b'-' {
            neg = true;
            i += 1;
        } else if filename[i] == b'+' {
            i += 1;
        }
    }
    if i >= filename.len() {
        return None;
    }
    let mut acc: u64 = 0;
    let mut any = false;
    if filename[i] == b'0' {
        if i + 2 < filename.len()
            && (filename[i + 1] == b'x' || filename[i + 1] == b'X')
            && filename[i + 2].is_ascii_hexdigit()
        {
            i += 2;
            while i < filename.len() && filename[i].is_ascii_hexdigit() {
                any = true;
                let d = match filename[i] {
                    b'0'..=b'9' => filename[i] - b'0',
                    b'a'..=b'f' => filename[i] - b'a' + 10,
                    b'A'..=b'F' => filename[i] - b'A' + 10,
                    _ => unreachable!(),
                };
                acc = acc.saturating_mul(16).saturating_add(u64::from(d));
                i += 1;
            }
        } else {
            while i < filename.len() && matches!(filename[i], b'0'..=b'7') {
                any = true;
                acc = acc
                    .saturating_mul(8)
                    .saturating_add(u64::from(filename[i] - b'0'));
                i += 1;
            }
        }
    } else {
        while i < filename.len() && filename[i].is_ascii_digit() {
            any = true;
            acc = acc
                .saturating_mul(10)
                .saturating_add(u64::from(filename[i] - b'0'));
            i += 1;
        }
    }
    if !any || i >= filename.len() {
        return None;
    }
    let val = if neg {
        let max_neg = (i64::MAX as u64) + 1;
        if acc >= max_neg {
            i64::MIN
        } else {
            -(acc as i64)
        }
    } else {
        acc.min(i64::MAX as u64) as i64
    };
    Some((val as i32, filename[i]))
}

/// Finds the first occurrence of `needle` in `haystack` starting at `pos` for `CString::Find`.
pub fn dqh_nstring_find_substr_from(haystack: &[u8], needle: &[u8], pos: i32) -> i32 {
    if pos < 0 || (pos != 0 && (pos as usize) >= haystack.len()) {
        return -1;
    }
    let start = pos as usize;
    if needle.is_empty() {
        return pos;
    }
    match haystack[start..]
        .windows(needle.len())
        .position(|w| w == needle)
    {
        Some(rel) => pos + (rel as i32),
        None => -1,
    }
}

/// Parses `(tm_year, tm_mon, tm_mday)` from a rotated log filename at the given match offsets
/// for `Log::RotateLog`.
pub fn dqh_log_parse_rotated_date_fields(
    filename: &[u8],
    year_off: usize,
    mon_off: usize,
    mday_off: usize,
) -> (i32, i32, i32) {
    let year = dqh_parse_c_atoi(filename.get(year_off..).unwrap_or(b"")) - 1900;
    let mon = dqh_parse_c_atoi(filename.get(mon_off..).unwrap_or(b"")) - 1;
    let mday = dqh_parse_c_atoi(filename.get(mday_off..).unwrap_or(b""));
    (year, mon, mday)
}

/// Returns `true` when two byte slices are identical.
pub fn dqh_mem_slices_equal(a: &[u8], b: &[u8]) -> bool {
    a == b
}

/// Returns `true` when `cipher` contains `:` (a multi-suite cipher list rather than a single suite).
pub fn dqh_tls_is_cipher_list(cipher: &[u8]) -> bool {
    cipher.contains(&b':')
}

/// Extracts the substring after the first ASCII space in `text` (or `text` if no space exists),
/// for `SystemInfo::InitLibraries`.
pub fn dqh_sysinfo_extract_version_after_space(text: &[u8]) -> Vec<u8> {
    match text.iter().position(|&b| b == b' ') {
        Some(idx) => text[idx + 1..].to_vec(),
        None => text.to_vec(),
    }
}

/// Classifies an `/etc/os-release` line for `OS::InitOSInfoFromOSRelease`:
/// `(1, 5)` for `NAME=`, `(2, 11)` for `VERSION_ID=`, `(3, 9)` for `BUILD_ID=`, or `(0, 0)`.
pub fn dqh_os_classify_os_release_line(
    line: &[u8],
    has_name: bool,
    has_version: bool,
) -> (i32, usize) {
    if !has_name && line.starts_with(b"NAME=") {
        (1, 5)
    } else if !has_version && line.starts_with(b"VERSION_ID=") {
        (2, 11)
    } else if !has_version && line.starts_with(b"BUILD_ID=") {
        (3, 9)
    } else {
        (0, 0)
    }
}

/// Computes `(start, len)` after trimming any byte in `chars` from both ends of `text`,
/// or `None` when every byte in `text` is in `chars`, for `DupeUtil::Trim`.
pub fn dqh_dupe_util_trim_bounds(text: &[u8], chars: &[u8]) -> Option<(usize, usize)> {
    let begin = text.iter().position(|b| !chars.contains(b))?;
    let end = text.iter().rposition(|b| !chars.contains(b))?;
    Some((begin, end - begin + 1))
}

/// Lowercases `text` byte-by-byte using the active C locale (`c_lower`), for `DupeUtil::Lower`.
pub fn dqh_dupe_util_lower(text: &[u8]) -> Vec<u8> {
    crate::releasename::lower(text)
}

/// Formats `"not an nzb-file: <masked first 160 bytes>"` for `NzbFetcher::Fetch`.
pub fn dqh_nzb_fetcher_format_parse_err(body: &[u8]) -> Vec<u8> {
    let prefix = &body[..body.len().min(160)];
    let masked = crate::dupefeed::mask_newznab_text(prefix);
    let mut out = Vec::with_capacity(17 + masked.len());
    out.extend_from_slice(b"not an nzb-file: ");
    out.extend_from_slice(&masked);
    out
}

/// Classifies the file extension in `Util::FindExecutorProgram`:
/// returns `(0, 0)` when no `.` is present, or `(kind, dot_idx)` where `kind` is:
/// `1` = `".py"`, `2` = `".sh"`, `3` = `".js"`, `4` = any other extension.
pub fn dqh_util_classify_executor_ext(filename: &[u8]) -> (i32, usize) {
    match filename.iter().rposition(|&b| b == b'.') {
        None => (0, 0),
        Some(idx) => {
            let ext = &filename[idx..];
            let kind = match ext {
                b".py" => 1,
                b".sh" => 2,
                b".js" => 3,
                _ => 4,
            };
            (kind, idx)
        }
    }
}

/// Compares `a` and `b` lexicographically after folding each byte via `c_lower` (`tolower`),
/// with zero heap allocations, for `ScriptConfig::SaveConfig`'s `CaseInsensitiveLess`.
pub fn dqh_bytes_lt_ignore_case(a: &[u8], b: &[u8]) -> bool {
    for (&x, &y) in a.iter().zip(b.iter()) {
        let lx = crate::releasename::c_lower(x);
        let ly = crate::releasename::c_lower(y);
        if lx != ly {
            return lx < ly;
        }
    }
    a.len() < b.len()
}

/// Returns `true` when `no_disk_access` is set, `value` is empty, or `value` contains
/// neither `,` nor `;`, for `Options::CheckScriptDir`.
pub fn dqh_options_should_check_single_script_dir(no_disk_access: bool, value: &[u8]) -> bool {
    no_disk_access || value.is_empty() || !value.iter().any(|&b| b == b',' || b == b';')
}

/// Encodes `bytes` as lowercase hexadecimal ASCII bytes (`2 * bytes.len()`) for `GenerateSecureToken`.
pub fn dqh_webserver_encode_hex_token(bytes: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 0x0f) as usize]);
    }
    out
}

/// Formats `"<key>=<val>\n"` for `ScriptConfig::SaveConfig`.
pub fn dqh_script_config_format_option_line(key: &[u8], val: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(key.len() + val.len() + 2);
    out.extend_from_slice(key);
    out.push(b'=');
    out.extend_from_slice(val);
    out.push(b'\n');
    out
}

/// Returns `line` with a trailing `\n` appended if `line` does not already end with `\n`,
/// for `ScriptConfig::SaveConfig`.
pub fn dqh_script_config_ensure_trailing_lf(line: &[u8]) -> Vec<u8> {
    if line.ends_with(b"\n") {
        line.to_vec()
    } else {
        let mut out = Vec::with_capacity(line.len() + 1);
        out.extend_from_slice(line);
        out.push(b'\n');
        out
    }
}

/// Formats `=ybegin part=<part> line=128 size=<file_size> name=<base_filename>\r\n=ypart begin=<offset + 1> end=<offset + size>\r\n`
/// for `YEncoder::WriteSegment`.
pub fn dqh_yenc_format_begin_headers(
    part: i32,
    file_size: i64,
    base_filename: &[u8],
    offset: i64,
    size: i32,
) -> Vec<u8> {
    let part_s = part.to_string();
    let fsize_s = file_size.to_string();
    let begin_s = offset.saturating_add(1).to_string();
    let end_s = offset.saturating_add(i64::from(size)).to_string();
    let mut out = Vec::with_capacity(
        48 + part_s.len() + fsize_s.len() + base_filename.len() + begin_s.len() + end_s.len(),
    );
    out.extend_from_slice(b"=ybegin part=");
    out.extend_from_slice(part_s.as_bytes());
    out.extend_from_slice(b" line=128 size=");
    out.extend_from_slice(fsize_s.as_bytes());
    out.extend_from_slice(b" name=");
    out.extend_from_slice(base_filename);
    out.extend_from_slice(b"\r\n=ypart begin=");
    out.extend_from_slice(begin_s.as_bytes());
    out.extend_from_slice(b" end=");
    out.extend_from_slice(end_s.as_bytes());
    out.extend_from_slice(b"\r\n");
    out
}

/// Formats `=yend size=<size> part=0 pcrc32=<pcrc32:08x>\r\n` for `YEncoder::WriteSegment`.
pub fn dqh_yenc_format_end_trailer(size: i32, pcrc32: u32) -> Vec<u8> {
    format!("=yend size={} part=0 pcrc32={:08x}\r\n", size, pcrc32).into_bytes()
}

/// Executes one step of the yEnc byte-encoding state machine for `YEncoder::WriteSegment`.
/// Returns `(bytes_consumed_from_in_slice, need_flush)`.
pub fn dqh_yenc_encode_chunk_step(
    in_slice: &[u8],
    last_block: bool,
    lnsz: &mut i32,
    out_slice: &mut [u8],
    out_len: &mut usize,
    out_cap: usize,
) -> (usize, bool) {
    let len = in_slice.len();
    for (idx, &raw) in in_slice.iter().enumerate() {
        let mut ch = raw.wrapping_add(42);
        let mut cur_off = *out_len + (*lnsz as usize);
        if matches!(ch, b'\0' | b'\n' | b'\r' | b'=' | b' ' | b'\t') {
            if cur_off < out_slice.len() {
                out_slice[cur_off] = b'=';
            }
            cur_off += 1;
            *lnsz += 1;
            ch = ch.wrapping_add(64);
        }
        if ch == b'.' && *lnsz == 0 {
            if cur_off < out_slice.len() {
                out_slice[cur_off] = b'.';
            }
            cur_off += 1;
            *lnsz += 1;
        }
        if cur_off < out_slice.len() {
            out_slice[cur_off] = ch;
        }
        cur_off += 1;
        *lnsz += 1;

        if *lnsz >= 128 || (idx + 1 == len && last_block) {
            if cur_off + 1 < out_slice.len() {
                out_slice[cur_off] = b'\r';
                out_slice[cur_off + 1] = b'\n';
            }
            *lnsz += 2;
            *out_len += *lnsz as usize;
            if *out_len < out_slice.len() {
                out_slice[*out_len] = 0;
            }
            *lnsz = 0;
            if *out_len > out_cap.saturating_sub(200) {
                return (idx + 1, true);
            }
        }
    }
    (len, false)
}

/// Formats NNTP server reply lines for `NntpProcessor`:
/// `kind`:
/// - `0`: `"211 0 0 0 <arg>\r\n"`
/// - `1`: `"403 <arg>\r\n"`
/// - `2`: `"223 0 <arg>\r\n"`
/// - `3`: `"220 0 <arg>\r\n"`
/// - `4`: `"222 0 <arg>\r\n"`
/// - `5`: `"Message-ID: <arg>\r\n"`
/// - `6`: `"Subject: \"<arg>\"\r\n"`
pub fn dqh_nntpserver_format_reply_line(kind: i32, arg: &[u8]) -> Vec<u8> {
    let (prefix, suffix): (&[u8], &[u8]) = match kind {
        0 => (b"211 0 0 0 ", b"\r\n"),
        1 => (b"403 ", b"\r\n"),
        2 => (b"223 0 ", b"\r\n"),
        3 => (b"220 0 ", b"\r\n"),
        4 => (b"222 0 ", b"\r\n"),
        5 => (b"Message-ID: ", b"\r\n"),
        6 => (b"Subject: \"", b"\"\r\n"),
        _ => (b"", b"\r\n"),
    };
    let mut out = Vec::with_capacity(prefix.len() + arg.len() + suffix.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(arg);
    out.extend_from_slice(suffix);
    out
}

/// Formats `"-o<unpack_dir>"` for `UnpackController::ExecuteSevenZip`.
pub fn dqh_unpack_format_7z_output_arg(unpack_dir: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + unpack_dir.len());
    out.extend_from_slice(b"-o");
    out.extend_from_slice(unpack_dir);
    out
}

/// Formats `"<prefix>: <text>"` for `ScriptController::PrintMessage`.
pub fn dqh_script_format_prefixed_message(prefix: &[u8], text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + 2 + text.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(b": ");
    out.extend_from_slice(text);
    out
}

/// Concatenates `decoded_path` and (when `has_query` is true) `&url[query_offset..]`
/// for `XmlRpcProcessor::SetUrl`.
pub fn dqh_xmlrpc_concat_decoded_path_and_query(
    decoded_path: &[u8],
    url: &[u8],
    has_query: bool,
    query_offset: usize,
) -> Vec<u8> {
    let query = if has_query && query_offset <= url.len() {
        &url[query_offset..]
    } else {
        &[][..]
    };
    let mut out = Vec::with_capacity(decoded_path.len() + query.len());
    out.extend_from_slice(decoded_path);
    out.extend_from_slice(query);
    out
}

/// Finds the next non-empty comma-delimited token `(tok_start, tok_len, next_cursor)` in `s`
/// starting from `cursor`, for `DiskState::LoadDupeRepairState` and `DiskState::LoadNzbInfo`.
pub fn dqh_diskstate_next_comma_token(
    s: &[u8],
    cursor: usize,
) -> Option<(usize, usize, usize)> {
    let mut pos = cursor;
    while pos < s.len() {
        while pos < s.len() && s[pos] == b',' {
            pos += 1;
        }
        if pos >= s.len() {
            return None;
        }
        let end = pos
            + s[pos..]
                .iter()
                .take_while(|&&b| b != b',')
                .count();
        let next = if end < s.len() { end + 1 } else { end };
        if end > pos {
            return Some((pos, end - pos, next));
        }
        pos = next;
    }
    None
}

/// Finds the next non-empty trimmed token `(tok_start, tok_end, next_pos)` in `s`
/// separated by any byte in `separators` and trimmed via `crate::util::trim_line`,
/// for `Tokenizer::Next`.
pub fn dqh_tokenizer_find_next_token(
    s: &[u8],
    separators: &[u8],
) -> Option<(usize, usize, usize)> {
    let mut pos = 0;
    while pos < s.len() {
        while pos < s.len() && separators.contains(&s[pos]) {
            pos += 1;
        }
        if pos >= s.len() {
            return None;
        }
        let raw_end = pos
            + s[pos..]
                .iter()
                .take_while(|&&b| !separators.contains(&b))
                .count();
        let next_pos = if raw_end < s.len() {
            raw_end + 1
        } else {
            raw_end
        };
        let (trim_start, trim_end) = crate::util::trim_line(&s[pos..raw_end]);
        if trim_start < trim_end {
            return Some((pos + trim_start, pos + trim_end, next_pos));
        }
        pos = next_pos;
    }
    None
}

/// Visits `\n`-delimited `param_name\tparam_val` lines from `dqh_queue_editor_diff_category_extensions`
/// output, invoking `cb(param_name, param_val)` for each line containing a tab,
/// for `QueueEditor::SetNzbCategory`.
pub fn dqh_queue_editor_visit_ext_diff_lines<F>(lines: &[u8], mut cb: F)
where
    F: FnMut(&[u8], &[u8]),
{
    let mut pos = 0;
    while pos < lines.len() {
        let Some(rel_nl) = lines[pos..].iter().position(|&b| b == b'\n') else {
            break;
        };
        let nl = pos + rel_nl;
        let line = &lines[pos..nl];
        if let Some(tab) = line.iter().position(|&b| b == b'\t') {
            cb(&line[..tab], &line[tab + 1..]);
        }
        pos = nl + 1;
    }
}

/// Maps a `QueueScriptCoordinator::EEvent` value to its NUL-terminated static event name
/// for `QueueScriptController::PrepareParams`.
pub fn dqh_queue_script_event_name(event: i32) -> &'static [u8] {
    match event {
        0 => b"FILE_DOWNLOADED\0",
        1 => b"URL_COMPLETED\0",
        2 => b"NZB_MARKED\0",
        3 => b"NZB_ADDED\0",
        4 => b"NZB_NAMED\0",
        5 => b"NZB_DOWNLOADED\0",
        6 => b"NZB_DELETED\0",
        _ => b"NONE\0",
    }
}

/// Maps an `NzbInfo::EDeleteStatus` value to its NUL-terminated static status name
/// for `QueueScriptController::PrepareParams`.
pub fn dqh_queue_script_delete_status_name(delete_status: i32) -> &'static [u8] {
    match delete_status {
        0 => b"NONE\0",
        1 => b"MANUAL\0",
        2 => b"HEALTH\0",
        3 => b"DUPE\0",
        4 => b"BAD\0",
        5 => b"GOOD\0",
        6 => b"COPY\0",
        7 => b"SCAN\0",
        _ => b"NONE\0",
    }
}

/// Maps an `NzbInfo::EUrlStatus` value to its NUL-terminated static status name
/// for `QueueScriptController::PrepareParams`.
pub fn dqh_queue_script_url_status_name(url_status: i32) -> &'static [u8] {
    match url_status {
        0 => b"NONE\0",
        1 => b"UNKNOWN\0",
        2 => b"SUCCESS\0",
        3 => b"FAILURE\0",
        4 => b"UNKNOWN\0",
        5 => b"SCAN_SKIPPED\0",
        6 => b"SCAN_FAILURE\0",
        _ => b"NONE\0",
    }
}

/// Maps an `NzbInfo::EMarkStatus` value to its NUL-terminated static status name
/// for `QueueScriptController::PrepareParams`.
pub fn dqh_queue_script_mark_status_name(mark_status: i32) -> &'static [u8] {
    match mark_status {
        0 => b"NONE\0",
        1 => b"BAD\0",
        2 => b"GOOD\0",
        3 => b"SUCCESS\0",
        _ => b"NONE\0",
    }
}

/// Computes `(window_end, from, block_to)` for an encrypted verification window in
/// `StreamRepairController::VerifyEncryptedFromSource`, returning `None` when `from >= window_end`.
pub fn dqh_stream_repair_calc_encrypted_window_bounds(
    window_offset: i64,
    window_end_raw: i64,
    last_computable: i64,
    block_from: i64,
    block_size: i64,
) -> Option<(i64, i64, i64)> {
    if block_size <= 0 {
        return None;
    }
    let window_end = window_end_raw.min(last_computable);
    let from = window_offset.max(block_from);
    if from >= window_end {
        return None;
    }
    let block_to = (window_end + block_size - 1) / block_size * block_size;
    Some((window_end, from, block_to))
}

/// Computes `(chunk_end, block_count)` for an encrypted patch chunk in
/// `StreamRepairController::PatchEncryptedFromSource`.
pub fn dqh_stream_repair_calc_encrypted_chunk_end(
    block_from: i64,
    block_to: i64,
    max_chunk: i64,
    block_size: i64,
) -> (i64, i64) {
    let chunk_end = (block_from.saturating_add(max_chunk)).min(block_to);
    let block_count = if block_size > 0 {
        (chunk_end - block_from) / block_size
    } else {
        0
    };
    (chunk_end, block_count)
}

/// Returns true when a stream-repair target is still unfinished (`decoded_file_size <= 0 || !holes_empty`)
/// for `StreamRepairController::ReportRemainingHoles`.
pub fn dqh_stream_repair_is_target_unfinished(decoded_file_size: i64, holes_empty: bool) -> bool {
    decoded_file_size <= 0 || !holes_empty
}

/// Returns true when `NzbFile::SAX_characters` should trim XML character content
/// (`current_element == b"meta" && has_category`).
pub fn dqh_nzbfile_should_trim_sax_chars(current_element: &[u8], has_category: bool) -> bool {
    has_category && current_element == b"meta"
}

/// Maps `RepairController::PostParChecker::GetStage()` (`0..=3`) to the corresponding
/// `PostInfo::EStage` value for `RepairController::UpdateParCheckProgress`.
pub fn dqh_repair_ctrl_map_par_stage_to_post_stage(
    par_stage: i32,
    pt_loading_pars: i32,
    pt_verifying_sources: i32,
    pt_repairing: i32,
    pt_verifying_repaired: i32,
) -> i32 {
    match par_stage {
        0 => pt_loading_pars,
        1 => pt_verifying_sources,
        2 => pt_repairing,
        3 => pt_verifying_repaired,
        _ => pt_loading_pars,
    }
}

/// Returns the NUL-terminated error message for `ParChecker` `Par2CmdLineErrStr[res]`.
pub fn dqh_parchecker_cmdline_err_str(res: i32) -> &'static [u8] {
    match res {
        0 => b"OK\0",
        1 => b"data files are damaged and there is enough recovery data available to repair them\0",
        2 => b"data files are damaged and there is insufficient recovery data available to be able to repair them\0",
        3 => b"there was something wrong with the command line arguments\0",
        4 => b"the PAR2 files did not contain sufficient information about the data files to be able to verify them\0",
        5 => b"repair completed but the data files still appear to be damaged\0",
        6 => b"an error occurred when accessing files\0",
        7 => b"internal error occurred\0",
        8 => b"out of memory\0",
        _ => b"internal error occurred\0",
    }
}

/// Returns the NUL-terminated stage message for `ParChecker` `Par2StageMessage[stage]`.
pub fn dqh_parchecker_stage_message(stage: i32) -> &'static [u8] {
    match stage {
        0 => b"Loading file\0",
        1 => b"Verifying file\0",
        2 => b"Repairing file\0",
        3 => b"Verifying repaired file\0",
        _ => b"Verifying file\0",
    }
}

/// Returns the NUL-terminated `NZBPP_SCRIPTSTATUS` name for `ScriptStatus::EStatus`.
pub fn dqh_post_script_status_name(status: i32) -> &'static [u8] {
    match status {
        0 => b"NONE\0",
        1 => b"FAILURE\0",
        2 => b"SUCCESS\0",
        _ => b"NONE\0",
    }
}

/// Maps `PostScriptController::PrepareParams` deprecated `NZBPP_PARSTATUS` /
/// `NZBPP_UNPACKSTATUS` enum value (`0..=5`) to `{ 0, 0, 1, 2, 3, 4 }`.
pub fn dqh_post_script_deprecated_status_code(status: i32) -> i32 {
    match status {
        0 | 1 => 0,
        2 => 1,
        3 => 2,
        4 => 3,
        5 => 4,
        _ => 0,
    }
}

/// Returns the NUL-terminated `Scheduler::ExecuteTask` debug command name.
pub fn dqh_scheduler_command_name(command: i32) -> &'static [u8] {
    match command {
        0 => b"Pause\0",
        1 => b"Unpause\0",
        2 => b"Pause Post-processing\0",
        3 => b"Unpause Post-processing\0",
        4 => b"Set download rate\0",
        5 => b"Execute process\0",
        6 => b"Execute script\0",
        7 => b"Pause Scan\0",
        8 => b"Unpause Scan\0",
        9 => b"Enable Server\0",
        10 => b"Disable Server\0",
        11 => b"Fetch Feed\0",
        _ => b"Unknown\0",
    }
}

/// Formats `ExtensionManager::Manager::UpdateExtension` backup path `<target_path>.update-backup`.
pub fn dqh_ext_mgr_format_update_backup_path(target_path: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(target_path.len() + 14);
    out.extend_from_slice(target_path);
    out.extend_from_slice(b".update-backup");
    out
}

/// Formats `ExtensionManager::Manager` lookup/use-count error messages:
/// - `0`: `"Update failed: Extension '<ext_name>' not found"`
/// - `1`: `"Update failed: Extension '<ext_name>' is currently in use or running"`
/// - `2`: `"Deletion failed: Extension '<ext_name>' not found"`
/// - `3`: `"Deletion failed: Extension '<ext_name>' is currently in use"`
pub fn dqh_ext_mgr_format_lookup_error(ext_name: &[u8], kind: i32) -> Vec<u8> {
    let (prefix, suffix): (&[u8], &[u8]) = match kind {
        0 => (b"Update failed: Extension '", b"' not found"),
        1 => (
            b"Update failed: Extension '",
            b"' is currently in use or running",
        ),
        2 => (b"Deletion failed: Extension '", b"' not found"),
        _ => (b"Deletion failed: Extension '", b"' is currently in use"),
    };
    let mut out = Vec::with_capacity(prefix.len() + ext_name.len() + suffix.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(ext_name);
    out.extend_from_slice(suffix);
    out
}

/// Formats `ExtensionManager::Manager` single-path install/move/delete error messages:
/// - `0`: `"Failed to initialize extraction for archive '<path>"`
/// - `1`: `"Extraction failed for archive '<path>"`
/// - `2`: `"Extension unpacked, but failed to delete temporary archive '<path>': <detail>"`
/// - `3`: `"Failed to move aside <path>: <detail>"`
/// - `4`: `"Failed to delete <path>: <detail>"`
pub fn dqh_ext_mgr_format_install_or_delete_error(
    path: &[u8],
    detail: &[u8],
    kind: i32,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(path.len() + detail.len() + 64);
    match kind {
        0 => {
            out.extend_from_slice(b"Failed to initialize extraction for archive '");
            out.extend_from_slice(path);
        }
        1 => {
            out.extend_from_slice(b"Extraction failed for archive '");
            out.extend_from_slice(path);
        }
        2 => {
            out.extend_from_slice(
                b"Extension unpacked, but failed to delete temporary archive '",
            );
            out.extend_from_slice(path);
            out.extend_from_slice(b"': ");
            out.extend_from_slice(detail);
        }
        3 => {
            out.extend_from_slice(b"Failed to move aside ");
            out.extend_from_slice(path);
            out.extend_from_slice(b": ");
            out.extend_from_slice(detail);
        }
        _ => {
            out.extend_from_slice(b"Failed to delete ");
            out.extend_from_slice(path);
            out.extend_from_slice(b": ");
            out.extend_from_slice(detail);
        }
    }
    out
}

/// Formats `ExtensionManager::Manager::UpdateExtension` compound rollback/cleanup errors:
/// - `0`: `"Failed to remove existing extension (Error: <primary_err>) and failed to cleanup temporary file '<path1>' (Error: <ec_msg>)"`
/// - `1`: `"<primary_err>; failed to restore <path1> from <path2>: <ec_msg>"`
pub fn dqh_ext_mgr_format_compound_update_error(
    primary_err: &[u8],
    path1: &[u8],
    path2: &[u8],
    ec_msg: &[u8],
    kind: i32,
) -> Vec<u8> {
    let mut out =
        Vec::with_capacity(primary_err.len() + path1.len() + path2.len() + ec_msg.len() + 96);
    if kind == 0 {
        out.extend_from_slice(b"Failed to remove existing extension (Error: ");
        out.extend_from_slice(primary_err);
        out.extend_from_slice(b") and failed to cleanup temporary file '");
        out.extend_from_slice(path1);
        out.extend_from_slice(b"' (Error: ");
        out.extend_from_slice(ec_msg);
        out.push(b')');
    } else {
        out.extend_from_slice(primary_err);
        out.extend_from_slice(b"; failed to restore ");
        out.extend_from_slice(path1);
        out.extend_from_slice(b" from ");
        out.extend_from_slice(path2);
        out.extend_from_slice(b": ");
        out.extend_from_slice(ec_msg);
    }
    out
}

/// Merges `,;`-separated script names from `add_list` into `existing` if not already
/// present (case-insensitively via `crate::options::has_script`), matching
/// `Options::MergeOldScriptOption`.
pub fn dqh_options_merge_script_list(existing: &[u8], add_list: &[u8]) -> Vec<u8> {
    let mut out = existing.to_vec();
    for script_name in crate::util::tokens(add_list, b",;") {
        let mut cbuf = Vec::with_capacity(script_name.len() + 1);
        cbuf.extend_from_slice(script_name);
        cbuf.push(0);
        if let Ok(cstr) = std::ffi::CStr::from_bytes_with_nul(&cbuf) {
            if !crate::options::has_script(&out, cstr) {
                if !out.is_empty() {
                    out.push(b',');
                }
                out.extend_from_slice(script_name);
            }
        }
    }
    out
}

/// Returns `Some(b"127.0.0.1\0")` when `control_ip` is `"0.0.0.0"`,
/// or `None` when the caller's `control_ip` should be used as-is.
pub fn dqh_frontend_resolve_control_ip(control_ip: &[u8]) -> Option<&'static [u8]> {
    if control_ip == b"0.0.0.0" {
        Some(b"127.0.0.1\0")
    } else {
        None
    }
}

/// Returns the 8-character NUL-terminated message type prefix for `NCursesFrontend::PrintMessage`.
/// When `first_line` is false, returns 8 spaces (`"        \0"`).
pub fn dqh_ncurses_message_type_label(kind: i32, first_line: bool) -> &'static [u8] {
    if !first_line {
        return b"        \0";
    }
    match kind {
        0 => b"INFO    \0",
        1 => b"WARNING \0",
        2 => b"ERROR   \0",
        3 => b"DEBUG   \0",
        4 => b"DETAIL  \0",
        _ => b"        \0",
    }
}

/// Maps `Message::EKind` (`0..=4`) to its ncurses color pair (`2..=6`).
pub fn dqh_ncurses_message_color_pair(kind: i32) -> i32 {
    match kind {
        0 => 2, // NCURSES_COLORPAIR_INFO
        1 => 3, // NCURSES_COLORPAIR_WARNING
        2 => 4, // NCURSES_COLORPAIR_ERROR
        3 => 5, // NCURSES_COLORPAIR_DEBUG
        4 => 6, // NCURSES_COLORPAIR_DETAIL
        _ => 1, // NCURSES_COLORPAIR_TEXT
    }
}

/// Returns the NUL-terminated HTTP `Content-Type` string for `XmlRpcProcessor::BuildResponse`.
pub fn dqh_xmlrpc_response_content_type(is_xml_rpc: bool) -> &'static [u8] {
    if is_xml_rpc {
        b"text/xml\0"
    } else {
        b"application/json\0"
    }
}

/// Returns the NUL-terminated server validation error message for `TestServerSpeedXmlCommand::Execute`.
pub fn dqh_xmlrpc_test_server_speed_err_msg(server_err: i32) -> &'static [u8] {
    match server_err {
        1 => b"Server not found\0",
        2 => b"Server is not active\0",
        _ => b"Server has no connections configured\0",
    }
}

/// Returns the NUL-terminated response message for `WriteLogBinCommand::Execute`.
pub fn dqh_binrpc_write_log_response_msg(ok: bool) -> &'static [u8] {
    if ok {
        b"Message added to log\0"
    } else {
        b"Invalid message-kind\0"
    }
}

/// Returns the NUL-terminated response message for `ScanBinCommand::Execute`.
pub fn dqh_binrpc_scan_response_msg(sync_mode: bool) -> &'static [u8] {
    if sync_mode {
        b"Scan-Command completed\0"
    } else {
        b"Scan-Command scheduled successfully\0"
    }
}

/// Steps the cumulative file CRC in `ArticleWriter::ProcessArticles` for yEnc articles.
/// Returns `(next_crc, next_first_article)` where `next_first_article` is always `false`.
pub fn dqh_article_writer_step_file_crc(
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

/// Classifies the final outcome of `RenameController::Run`:
/// `1` = cancelled (`is_stopped == true`),
/// `2` = renamed files (`renamed_count > 0`),
/// `3` = no renamed files found.
pub fn dqh_rename_ctrl_classify_run_outcome(is_stopped: bool, renamed_count: i32) -> i32 {
    if is_stopped {
        1
    } else if renamed_count > 0 {
        2
    } else {
        3
    }
}

/// Uppercases the first byte of `buf` in-place if it is an ASCII lowercase letter (`a..=z`).
pub fn dqh_post_capitalize_first_ascii(buf: &mut [u8]) {
    if let Some(first) = buf.first_mut() {
        *first = first.to_ascii_uppercase();
    }
}

/// Returns the NUL-terminated status suffix for `DupeSearch::PrepareNative` search log message.
pub fn dqh_dupesearch_prepare_status_suffix(rank_only: bool, history: bool) -> &'static [u8] {
    if rank_only {
        b": ranking new backups\0"
    } else if history {
        b": it failed before its search was due\0"
    } else {
        b"\0"
    }
}

/// Visits up to 8 non-empty lenient search queries for `DupeSearch::Expand`.
pub fn dqh_dupesearch_visit_expand_queries<F: FnMut(&[u8])>(name: &[u8], mut cb: F) {
    for q in crate::dupewatch::lenient_queries(name).into_iter().take(8) {
        if !q.is_empty() {
            cb(&q);
        }
    }
}

/// Formats `Newznab::Search` query label (`"key1=val1 key2=val2"`).
pub fn dqh_newznab_format_query_label(keys: &[&[u8]], vals: &[&[u8]]) -> Vec<u8> {
    let n = keys.len().min(vals.len());
    let mut out = Vec::new();
    for i in 0..n {
        if !out.is_empty() {
            out.push(b' ');
        }
        out.extend_from_slice(keys[i]);
        out.push(b'=');
        out.extend_from_slice(vals[i]);
    }
    out
}

/// Formats an NNTP message-id in angle brackets (`<id>`) if not already bracketed.
pub fn dqh_nntp_health_format_bracketed_id(message_id: &[u8]) -> Vec<u8> {
    if message_id.first() == Some(&b'<') && message_id.last() == Some(&b'>') {
        message_id.to_vec()
    } else {
        let mut out = Vec::with_capacity(message_id.len() + 2);
        out.push(b'<');
        out.extend_from_slice(message_id);
        out.push(b'>');
        out
    }
}

/// Formats a pipelined batch of `"STAT <id>\r\n"` commands for `NntpHealthServer::Exchange`.
pub fn dqh_nntp_health_format_stat_batch(ids: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ids.len() * 48);
    for id in ids {
        out.extend_from_slice(b"STAT ");
        if id.first() == Some(&b'<') && id.last() == Some(&b'>') {
            out.extend_from_slice(id);
        } else {
            out.push(b'<');
            out.extend_from_slice(id);
            out.push(b'>');
        }
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Returns the NUL-terminated format string for `QueueCoordinator::DeleteFileInfo` stream repair queueing log.
pub fn dqh_queue_coord_stream_repair_queue_msg_fmt(is_failure: bool) -> &'static [u8] {
    if is_failure {
        b"Queueing stream repair of %s (no article available) from duplicate collections\0"
    } else {
        b"Queueing stream repair of %s from duplicate collections\0"
    }
}

/// Selects the edit action (`ea_group_park_delete` vs `ea_group_delete`) for `QueueCoordinator::CheckHealth`.
pub fn dqh_queue_coord_select_health_delete_action(
    health_check: i32,
    hc_park: i32,
    ea_group_park_delete: i32,
    ea_group_delete: i32,
) -> i32 {
    if health_check == hc_park {
        ea_group_park_delete
    } else {
        ea_group_delete
    }
}

/// Returns the maximum positive `i64` in `values` (floor `0`) for `QueueCoordinator::HasSizeTwin`.
pub fn dqh_queue_coord_max_i64_slice(values: &[i64]) -> i64 {
    let mut largest = 0i64;
    for &v in values {
        if v > largest {
            largest = v;
        }
    }
    largest
}

/// Returns the NUL-terminated relation string (`"under half the"` vs `"below"`) for
/// `QueueCoordinator::CheckDeadDownload` no-backup parking warning.
pub fn dqh_queue_coord_projected_health_rel_str(far_below: bool) -> &'static [u8] {
    if far_below {
        b"under half the\0"
    } else {
        b"below\0"
    }
}

/// Returns the NUL-terminated relation string (`"below"` vs `"too close to"`) for
/// `QueueCoordinator::CheckDeadDownload` partly-alive failover warning.
pub fn dqh_queue_coord_partly_alive_rel_str(proj_below_crit: bool) -> &'static [u8] {
    if proj_below_crit {
        b"below\0"
    } else {
        b"too close to\0"
    }
}

/// Decides the parameter mutation action for `NzbParameterList::SetParameter`:
/// `1` = erase existing, `2` = update existing in-place, `3` = append new, `0` = no-op.
pub fn dqh_nzb_param_decide_set_action(empty_val: bool, found: bool) -> i32 {
    if empty_val && found {
        1
    } else if found {
        2
    } else if !empty_val {
        3
    } else {
        0
    }
}

/// Returns true when a directory entry in `DupeCoordinator::FilesOnDisk` is a valid non-side regular file (`> 0` bytes).
pub fn dqh_dupecoord_is_valid_disk_file(is_side_file: bool, is_dir: bool, file_size: i64) -> bool {
    !is_side_file && !is_dir && file_size > 0
}

/// Selects the edit action (`ea_post_delete` vs `delete_action`) for other queued duplicates
/// in `DupeCoordinator::NzbCompleted`.
pub fn dqh_dupecoord_select_other_dupe_delete_action(
    in_post_process: bool,
    ea_post_delete: i32,
    delete_action: i32,
) -> i32 {
    if in_post_process {
        ea_post_delete
    } else {
        delete_action
    }
}

/// Selects the message kind (`mk_detail` vs `mk_info`) for `QueueEditor::DeleteEntry`.
pub fn dqh_queue_editor_delete_entry_msg_kind(
    deleting: bool,
    mk_detail: i32,
    mk_info: i32,
) -> i32 {
    if deleting {
        mk_detail
    } else {
        mk_info
    }
}

/// Selects the message kind (`mk_warning` vs `mk_detail`) for `Scanner::AddFileToQueue` missed articles log.
pub fn dqh_scanner_missed_articles_msg_kind(
    is_warn: bool,
    mk_warning: i32,
    mk_detail: i32,
) -> i32 {
    if is_warn {
        mk_warning
    } else {
        mk_detail
    }
}

/// Evaluates existing output file status in `ArticleWriter::CreateOutputFile`:
/// `0` = does not exist, `1` = keep existing file of equal size, `2` = delete existing file of different size.
pub fn dqh_article_writer_check_existing_output_file(
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
pub fn dqh_article_writer_select_direct_output_mode(
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
pub fn dqh_article_writer_should_fallback_open_to_final_path(
    open_exists: bool,
    final_exists: bool,
) -> bool {
    !open_exists && final_exists
}

/// Returns true when a staged duplicate article's size or copied byte count matches `segment_size`.
pub fn dqh_article_writer_validate_staged_dupe_size(
    actual_size: i64,
    segment_size: i32,
) -> bool {
    actual_size == i64::from(segment_size)
}

/// Evaluates post-close commit action in `ArticleWriter::CommitDiskFile`:
/// `-1` = flush/close failed, `1` = move `tempDestPath` to `finalOutputPath`, `0` = direct-write complete.
pub fn dqh_article_writer_eval_commit_post_close(
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
pub fn dqh_article_writer_is_staged_dupe_article_file(
    dupe_fallback_round: i32,
    has_result_filename: bool,
) -> bool {
    dupe_fallback_round > 0 && has_result_filename
}

/// Classifies the final output file cleanup action in `ArticleWriter::CleanupOldData` (direct-write branch):
/// `1` = move `m_outputFilename` to `finalOutputPath`, `2` = delete `m_outputFilename`, `0` = no-op.
pub fn dqh_article_writer_classify_direct_final_cleanup(
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
pub fn dqh_article_cache_should_write_cache_flag(
    was_empty: bool,
    server_mode: bool,
    continue_partial: bool,
    skip_write: bool,
) -> bool {
    was_empty && server_mode && continue_partial && !skip_write
}

/// Selects the post-flush action in `ArticleCache::Run`:
/// `1` = `MemoryMonitor::TrimHeap()`, `2` = `Util::Sleep(5)`, `0` = none.
pub fn dqh_article_cache_post_flush_action(
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

/// Returns true when `ArticleFetcher::Fetch` should clear the unreachable flag for the server.
pub fn dqh_article_fetcher_should_clear_unreachable(success: bool, transient: bool) -> bool {
    success || !transient
}

/// Returns true when `ArticleFetcher::Fetch` should evaluate transient retry on an answering/reachable server.
pub fn dqh_article_fetcher_can_attempt_transient_retry(
    transient: bool,
    server_unreachable: bool,
) -> bool {
    transient && !server_unreachable
}

/// Returns true when `ArticleFetcher::Fetch` has remaining transient retry attempts (`attempt < max_transient_attempts`).
pub fn dqh_article_fetcher_has_more_transient_attempts(
    attempt: i32,
    max_transient_attempts: i32,
) -> bool {
    attempt < max_transient_attempts
}

/// Returns true when `ArticleBatchFetcher::StoreResult` should accept and store the fetched result.
pub fn dqh_article_batch_can_store_result(
    batch_id: i32,
    current_batch_id: i32,
    cancelled: bool,
) -> bool {
    batch_id == current_batch_id && !cancelled
}

/// Returns true when `ArticleBatchFetcher::Next` can deliver the next slot.
pub fn dqh_article_batch_can_deliver_next(
    cancelled: bool,
    stopped: bool,
    next_deliver: usize,
    total_requests: usize,
) -> bool {
    !cancelled && !stopped && next_deliver < total_requests
}

/// Classifies the pre-URL HTTP method check in `WebProcessor::Execute`:
/// `1` = reject POST with `content_len <= 0`, `2` = answer OPTIONS request, `0` = continue.
pub fn dqh_webserver_classify_pre_parse_method(
    http_method: i32,
    hm_post: i32,
    hm_options: i32,
    content_len: i32,
) -> i32 {
    if http_method == hm_post && content_len <= 0 {
        1
    } else if http_method == hm_options {
        2
    } else {
        0
    }
}

/// Classifies which history entry fields to populate in `HistoryBinCommand::Execute`:
/// `1` = NZB fields, `2` = Dup size fields, `3` = URL status field, `0` = none.
pub fn dqh_binrpc_classify_history_entry_fields(
    kind: i32,
    hk_nzb: i32,
    hk_dup: i32,
    hk_url: i32,
    show_hidden: bool,
) -> i32 {
    if kind == hk_nzb {
        1
    } else if kind == hk_dup && show_hidden {
        2
    } else if kind == hk_url {
        3
    } else {
        0
    }
}

/// Classifies transactional temp/dest file recovery in `StateFile::BeginRead`:
/// `1` = delete leftover temp file, `2` = restore temp file to dest file, `0` = normal open.
pub fn dqh_diskstate_classify_begin_read_recovery(
    transactional: bool,
    dest_exists: bool,
    temp_exists: bool,
) -> i32 {
    if transactional && dest_exists && temp_exists {
        1
    } else if !dest_exists && temp_exists {
        2
    } else {
        0
    }
}

/// Selects the `serverVolumes` slot index for `DiskState::LoadVolumeStat` entry `index`:
/// returns `0` for `index == 0`, the last matching `server_id` where `state_id == index`, or `-1` if none.
pub fn dqh_diskstate_select_volume_server_slot(
    index: i32,
    state_ids: &[i32],
    server_ids: &[i32],
) -> i32 {
    if index == 0 {
        return 0;
    }
    let count = state_ids.len().min(server_ids.len());
    let mut slot = -1;
    for k in 0..count {
        if state_ids[k] == index {
            slot = server_ids[k];
        }
    }
    slot
}

/// Classifies the `TlsCipherXmlCommand::Execute` operation code:
/// `0` = `Start`, `1` = `Status`, `2` = `Cancel`.
pub fn dqh_xmlrpc_classify_tls_cipher_op(operation: i32) -> i32 {
    if operation == 0 {
        0
    } else if operation == 1 {
        1
    } else {
        2
    }
}

/// Validates the `ScheduleResumeXmlCommand::Execute` parameter and computes the resume timestamp.
pub fn dqh_xmlrpc_calc_schedule_resume_time(
    has_param: bool,
    seconds: i32,
    cur_time: i64,
) -> Option<i64> {
    if has_param && seconds >= 0 {
        Some(cur_time.saturating_add(i64::from(seconds)))
    } else {
        None
    }
}

/// Steps the `(post_job_count, url_count)` accumulators in `StatusXmlCommand::Execute`.
pub fn dqh_xmlrpc_accum_queue_job_counts(
    has_post_info: bool,
    kind: i32,
    nk_url: i32,
    cur_post_count: i32,
    cur_url_count: i32,
) -> (i32, i32) {
    (
        cur_post_count.saturating_add(i32::from(has_post_info)),
        cur_url_count.saturating_add(i32::from(kind == nk_url)),
    )
}

/// Returns true when `AppendFleetXmlCommand::Execute` has reached or exceeded `max_members`.
pub fn dqh_xmlrpc_is_fleet_member_limit_reached(current_count: usize, max_members: i32) -> bool {
    (current_count as i32) >= max_members
}

/// Classifies a history entry kind in `HistoryXmlCommand::Execute` and `HistoryXmlCommand::DetectStatus`:
/// `1` = `hkNzb` or `hkUrl`, `2` = `hkDup`, `0` = other.
pub fn dqh_xmlrpc_classify_history_entry_kind(
    kind: i32,
    hk_nzb: i32,
    hk_url: i32,
    hk_dup: i32,
) -> i32 {
    if kind == hk_nzb || kind == hk_url {
        1
    } else if kind == hk_dup {
        2
    } else {
        0
    }
}

/// Returns true when a server matches the target ID in `EditServerXmlCommand::Execute`.
pub fn dqh_xmlrpc_matches_edit_server_id(server_id: i32, target_id: i32) -> bool {
    server_id == target_id
}

/// Returns true when a server volume slot matches `server_id` in `ResetServerVolumeXmlCommand::Execute`.
pub fn dqh_xmlrpc_matches_reset_server_volume(index: i32, server_id: i32) -> bool {
    index == server_id || server_id == -1
}

/// Returns true when `certVerifLevel` is within `[0, count)` in `TestServerXmlCommand::Execute`.
pub fn dqh_xmlrpc_is_valid_cert_verif_level(level: i32, count: i32) -> bool {
    level >= 0 && level < count
}

/// Selects the response string for `TestServerXmlCommand::Execute`.
pub fn dqh_xmlrpc_select_test_server_err_text(ok: bool, err_text: &[u8]) -> &[u8] {
    if ok {
        b""
    } else if err_text.is_empty() {
        b"Unknown error"
    } else {
        err_text
    }
}

/// Checks individual argument bounds in `TestDiskSpeedXmlCommand::Execute`:
/// `1` = `write_buffer_kib < 0`, `2` = `max_file_size_gib < 1`, `3` = `timeout_sec < 1`, `0` = valid.
pub fn dqh_xmlrpc_check_disk_speed_arg_ranges(
    write_buffer_kib: i32,
    max_file_size_gib: i32,
    timeout_sec: i32,
) -> i32 {
    if write_buffer_kib < 0 {
        1
    } else if max_file_size_gib < 1 {
        2
    } else if timeout_sec < 1 {
        3
    } else {
        0
    }
}

/// Selects the effective extension script list for `FeedCoordinator::FeedCompleted` and `PreviewFeed`.
pub fn dqh_feed_coord_select_extensions<'a>(
    feed_extensions: &'a [u8],
    global_extensions: &'a [u8],
) -> &'a [u8] {
    if !feed_extensions.is_empty() {
        feed_extensions
    } else {
        global_extensions
    }
}

/// Computes the minimum `last_update` timestamp across all feeds and `cur_oldest` in `FeedCoordinator::CleanupHistory`.
pub fn dqh_feed_coord_min_last_update(cur_oldest: i64, last_updates: &[i64]) -> i64 {
    last_updates.iter().copied().fold(cur_oldest, i64::min)
}

/// Returns true when `FeedFile::SAX_textHandler` should trim whitespace from the text chunk (`current_element == "category"`).
pub fn dqh_feed_file_should_trim_text_chunk(current_element: &[u8]) -> bool {
    current_element == b"category"
}

/// Returns true when an extension shared pointer is currently in use (`use_count > 1`).
pub fn dqh_ext_mgr_is_extension_in_use(use_count: i64) -> bool {
    use_count > 1
}

/// Returns true when `ExtensionManager::GetExtensionPath` should target the entry file instead of the directory (`same_location_count > 1`).
pub fn dqh_ext_mgr_should_use_entry_as_path(same_location_count: isize) -> bool {
    same_location_count > 1
}

/// Returns true when `ExtensionManager::CreateTasks` should create a scheduler task for an extension.
pub fn dqh_ext_mgr_can_create_scheduler_task(
    is_scheduler_script: bool,
    task_time_empty: bool,
    in_extensions_list: bool,
) -> bool {
    is_scheduler_script && !task_time_empty && in_extensions_list
}

/// Selects the log message kind (`mkDetail` vs `mkInfo`) for `QueueScriptController::ExecuteScript`.
pub fn dqh_queue_script_select_exec_msg_kind(
    event: i32,
    qe_file_downloaded: i32,
    mk_detail: i32,
    mk_info: i32,
) -> i32 {
    if event == qe_file_downloaded {
        mk_detail
    } else {
        mk_info
    }
}

/// Returns true when a queued item in `QueueScriptCoordinator::EnqueueScript` is a duplicate `qeFileDownloaded` entry for the same NZB and script.
pub fn dqh_queue_script_is_duplicate_file_downloaded_item(
    event: i32,
    qe_file_downloaded: i32,
    item_nzb_id: i32,
    target_nzb_id: i32,
    same_script: bool,
) -> bool {
    event == qe_file_downloaded && item_nzb_id == target_nzb_id && same_script
}

/// Returns true when row `index` is within the visible scroll window in `NCursesFrontend`.
pub fn dqh_ncurses_is_row_in_scroll_window(
    index: i32,
    scroll_offset: i32,
    win_height: i32,
) -> bool {
    index >= scroll_offset && index < scroll_offset + win_height - 1
}

/// Accumulates file queue remaining bytes, paused bytes, and paused file count in `NCursesFrontend::PrintFileQueue`.
pub fn dqh_ncurses_accum_file_queue_stats(
    paused: bool,
    file_rem_size: i64,
    cur_rem: i64,
    cur_paused: i64,
    cur_paused_files: i32,
) -> (i64, i64, i32) {
    let next_rem = cur_rem + file_rem_size;
    if paused {
        (
            next_rem,
            cur_paused + file_rem_size,
            cur_paused_files + 1,
        )
    } else {
        (next_rem, cur_paused, cur_paused_files)
    }
}

/// Accumulates group queue remaining bytes and paused bytes in `NCursesFrontend::PrintGroupQueue`.
pub fn dqh_ncurses_accum_group_queue_stats(
    group_rem_size: i64,
    group_paused_size: i64,
    cur_rem: i64,
    cur_paused: i64,
) -> (i64, i64) {
    (cur_rem + group_rem_size, cur_paused + group_paused_size)
}

/// Evaluates whether `NCursesFrontend::UpdateInput` can enter `editQueue` mode and returns the updated queue window percentage.
pub fn dqh_ncurses_enter_edit_queue_win_pct(queue_size: i32, cur_win_pct: i32) -> Option<i32> {
    if queue_size > 0 {
        Some(if cur_win_pct == 0 { 50 } else { cur_win_pct })
    } else {
        None
    }
}

/// Computes the last valid queue index (`queue_size - 1` or `0` when empty) in `NCursesFrontend::UpdateInput`.
pub fn dqh_ncurses_last_queue_index(queue_size: i32) -> i32 {
    if queue_size > 0 {
        queue_size - 1
    } else {
        0
    }
}

/// Returns true when `DiskService::ServiceWork` should check free disk space.
pub fn dqh_disk_service_should_check_disk_space(disk_space_mb: i32, is_downloading: bool) -> bool {
    disk_space_mb > 0 && is_downloading
}

/// Evaluates a required directory check in `DiskService::CheckRequiredDir`:
/// returns `(is_missing, should_log_waiting, next_waiting_reported)`.
pub fn dqh_disk_service_eval_missing_required_dir(
    file_exists: bool,
    dir_exists: bool,
    was_waiting_reported: bool,
    cur_waiting_reported: bool,
) -> (bool, bool, bool) {
    if !file_exists && !dir_exists {
        let should_log = !was_waiting_reported;
        let next_reported = if should_log {
            true
        } else {
            cur_waiting_reported
        };
        (true, should_log, next_reported)
    } else {
        (false, false, cur_waiting_reported)
    }
}

/// Returns true when `Maintenance::CheckUpdates` received non-empty update info output.
pub fn dqh_maintenance_has_update_info(len: i32) -> bool {
    len > 0
}

/// Resolves `m_configLine` in `Options::LocateOptionSrcPos`.
pub fn dqh_options_resolve_locate_src_line(found: bool, line_no: i32) -> i32 {
    if found {
        line_no
    } else {
        0
    }
}

/// Updates `m_configLine` in `Options::GetOption` when `line_no > 0`.
pub fn dqh_options_update_config_line_on_get(line_no: i32, cur_config_line: i32) -> i32 {
    if line_no > 0 {
        line_no
    } else {
        cur_config_line
    }
}

/// Returns true when a parsed scheduler task hour represents every hour (`hours == -2`).
pub fn dqh_options_is_every_hour_task(hours: i32) -> bool {
    hours == -2
}

/// Returns true when `NZBGet::Phase1_Init` should run the system health check.
pub fn dqh_nzbget_should_run_system_health_check(
    server_mode: bool,
    system_health_check: bool,
) -> bool {
    server_mode && system_health_check
}

/// Evaluates whether `NZBGet::BootConfig` should print usage and exit, returning the exit code (`0` or `1`).
pub fn dqh_nzbget_classify_boot_usage_or_error(
    print_usage: bool,
    has_errors: bool,
    arg_count: i32,
) -> Option<i32> {
    if print_usage || has_errors || arg_count <= 1 {
        Some(if print_usage { 0 } else { 1 })
    } else {
        None
    }
}

/// Returns true when `NZBGet::StopFrontend` should immediately stop the frontend thread before waiting.
pub fn dqh_nzbget_should_stop_frontend_immediately(
    remote_client_mode: bool,
    stopped: bool,
) -> bool {
    !remote_client_mode || stopped
}

/// Classifies the log file initialization action in `Log::InitOptions`:
/// `0` = skip, `1` = update log path only, `2` = update log path and reset log.
pub fn dqh_log_classify_init_logfile_action(
    remote_client_mode: bool,
    direct_client_mode: bool,
    write_log: i32,
    wl_none: i32,
    wl_reset: i32,
    has_log_file: bool,
    server_mode: bool,
) -> i32 {
    if !remote_client_mode && !direct_client_mode && write_log != wl_none && has_log_file {
        if server_mode && write_log == wl_reset {
            2
        } else {
            1
        }
    } else {
        0
    }
}

/// Evaluates whether `ScriptController::Terminate` succeeded.
pub fn dqh_script_ctrl_is_terminate_ok(child_pid: i32, kill_res: i32, completed: bool) -> bool {
    child_pid <= 0 || kill_res == 0 || completed
}

/// Returns true when `ScriptController::Break` has an active uncompleted process to signal.
pub fn dqh_script_ctrl_can_send_break(pid: i32, completed: bool) -> bool {
    pid > 0 && !completed
}

/// Returns true when `FileSystem::MoveFile` should fall back from `rename` to `CopyFile` + `DeleteFile` (`!rename_ok && err_no == exdev`).
pub fn dqh_fs_should_fallback_move_to_copy(rename_ok: bool, err_no: i32, exdev: i32) -> bool {
    !rename_ok && err_no == exdev
}

/// Returns true when `FileSystem::CopyFile` closed cleanly, had no read/write error, and copied the exact source size.
pub fn dqh_fs_is_copied_file_valid(
    close_ok: bool,
    loop_ok: bool,
    copied_bytes: i64,
    src_size: i64,
) -> bool {
    close_ok && loop_ok && copied_bytes == src_size
}

/// Resolves the fallback PAR buffer size in MB when `g_MemoryMonitor` is null in `Repairer::Repairer`.
pub fn dqh_parchecker_resolve_fallback_par_buffer_mb(configured_mb: i32) -> i32 {
    if configured_mb > 0 {
        configured_mb
    } else {
        16
    }
}

/// Returns true when `ch` is a line break (`\n` or `\r`) in `ParChecker::StreamBuf::overflow`.
pub fn dqh_parchecker_is_streambuf_line_break(ch: i32) -> bool {
    ch == i32::from(b'\n') || ch == i32::from(b'\r')
}

/// Classifies the step outcome after `Repairer::PreProcess` in `ParChecker::PreProcessPar`:
/// `1` = stopped, `2` = invalid command line arguments, `3` = failed (request more pars), `0` = success.
pub fn dqh_parchecker_classify_preprocess_step(
    is_stopped: bool,
    res: i32,
    e_success: i32,
    e_invalid_args: i32,
) -> i32 {
    if is_stopped {
        1
    } else if res == e_invalid_args {
        2
    } else if res != e_success {
        3
    } else {
        0
    }
}

/// Returns true when `ParChecker::ProcessTwinSet` can proceed with a non-empty twin index while not stopped.
pub fn dqh_parchecker_can_start_twin_set(index_empty: bool, is_stopped: bool) -> bool {
    !index_empty && !is_stopped
}

/// Returns true when `ParChecker::ProcessTwinSet` should re-verify and process after loading twin volumes.
pub fn dqh_parchecker_should_update_after_twin_volumes(
    volumes_empty: bool,
    is_stopped: bool,
) -> bool {
    !volumes_empty && !is_stopped
}

/// Returns true when a repairer sourcefile in `ParChecker::SignalDone` is the missing target for `file_name`.
pub fn dqh_parchecker_is_missing_target_sourcefile(
    target_exists: bool,
    file_name: &[u8],
    target_base_name: &[u8],
) -> bool {
    !target_exists && file_name == target_base_name
}

/// Returns true when `FindFileCrc` returned a terminal quick-verification status (`fsFailure` or `fsUnknown`) in `ParChecker::VerifyDataFile`.
pub fn dqh_parchecker_is_terminal_quick_crc_status(
    file_status: i32,
    fs_failure: i32,
    fs_unknown: i32,
) -> bool {
    file_status == fs_failure || file_status == fs_unknown
}

/// Returns true when a valid block range in `ParChecker::VerifyPartialDataFile` passed range CRC verification.
pub fn dqh_parchecker_is_partial_range_crc_valid(
    ok: bool,
    download_crc: u32,
    par_crc: u32,
) -> bool {
    ok && download_crc == par_crc
}

/// Accumulates elapsed unpack seconds in `UnpackController::Run`.
pub fn dqh_unpack_ctrl_accum_unpack_sec(
    prev_unpack_sec: i32,
    cur_time: i64,
    start_time: i64,
) -> i32 {
    prev_unpack_sec + (cur_time - start_time) as i32
}

/// Selects the archive wildcard pattern (`"*.7z.001"` vs `"*.7z"`) in `UnpackController::ExecuteSevenZip`.
pub fn dqh_unpack_ctrl_select_sevenzip_wildcard(multi_volumes: bool) -> &'static [u8] {
    if multi_volumes {
        b"*.7z.001\0"
    } else {
        b"*.7z\0"
    }
}

/// Computes the write buffer size in bytes (`write_buffer_kb * 1024`) when `write_buffer_kb > 0` in `UnpackController::JoinFile`.
pub fn dqh_unpack_ctrl_calc_write_buffer_bytes(write_buffer_kb: i32) -> Option<i32> {
    if write_buffer_kb > 0 {
        Some(write_buffer_kb.saturating_mul(1024))
    } else {
        None
    }
}

/// Returns true when closing the joined output file in `UnpackController::JoinFile` failed after a clean loop.
pub fn dqh_unpack_ctrl_should_fail_on_close(close_ok: bool, prev_ok: bool) -> bool {
    !close_ok && prev_ok
}

/// Returns true when both unpacking and cleanup succeeded in `UnpackController::Completed`.
pub fn dqh_unpack_ctrl_is_completed_success(unpack_ok: bool, cleanup_success: bool) -> bool {
    unpack_ok && cleanup_success
}

/// Selects `final_dir` when non-empty or falls back to `dest_dir` in `UnpackController::CreateUnpackDir` and `Cleanup`.
pub fn dqh_unpack_ctrl_select_unpack_parent_dir<'a>(
    final_dir: &'a [u8],
    dest_dir: &'a [u8],
) -> &'a [u8] {
    if !final_dir.is_empty() {
        final_dir
    } else {
        dest_dir
    }
}

/// Returns true when moving an extracted file in `UnpackController::Cleanup` failed on a non-hidden file.
pub fn dqh_unpack_ctrl_is_move_extracted_error(move_ok: bool, hidden_file: bool) -> bool {
    !move_ok && !hidden_file
}

/// Returns true when `UnpackController::Cleanup` should delete a newly created `final_dir` after unpack failure.
pub fn dqh_unpack_ctrl_should_delete_created_final_dir(
    unpack_ok: bool,
    final_dir_created: bool,
) -> bool {
    !unpack_ok && final_dir_created
}

/// Returns true when `PrePostProcessor::SanitisePostQueue` should reset a stale `nsRunning` direct unpack status.
pub fn dqh_prepost_should_reset_stale_direct_unpack(
    direct_unpack_status: i32,
    ns_running: i32,
    has_unpack_thread: bool,
) -> bool {
    direct_unpack_status == ns_running && !has_unpack_thread
}

/// Returns true when `ParCannotCover` in `PrePostProcessor.cpp` found exactly one main PAR2 set.
pub fn dqh_prepost_has_single_main_par(found_main_pars: bool, main_pars_count: usize) -> bool {
    found_main_pars && main_pars_count == 1
}

/// Returns true when a queued NZB entry matches a post-queue delete target ID in `PrePostProcessor::PostQueueDelete`.
pub fn dqh_prepost_matches_post_delete_target(
    has_post_info: bool,
    nzb_id: i32,
    target_id: i32,
) -> bool {
    has_post_info && nzb_id == target_id
}

/// Returns true when a twin NZB entry should be skipped while collecting recovery volumes in `RepairController::PostParChecker::RequestTwinVolumes`.
pub fn dqh_repair_ctrl_should_skip_twin_volume_entry(
    is_par2: bool,
    entry_filename: &[u8],
    twin_index: &[u8],
) -> bool {
    !is_par2 || entry_filename == twin_index
}

/// Returns true when a history entry matches a dupe source in `RepairController::PostParChecker::StatDupeSources`.
pub fn dqh_repair_ctrl_matches_history_dupe_source(
    history_kind: i32,
    hk_nzb: i32,
    history_nzb_id: i32,
    dupe_source_id: i32,
) -> bool {
    history_kind == hk_nzb && history_nzb_id == dupe_source_id
}

/// Returns true when `CleanupController::Run` should also run cleanup on `final_dir`.
pub fn dqh_cleanup_ctrl_should_cleanup_final_dir(ok: bool, final_dir_empty: bool) -> bool {
    ok && !final_dir_empty
}

/// Returns true when `CleanupController::Cleanup` should recurse into an entry (`dir_exists && !is_link`).
pub fn dqh_cleanup_ctrl_is_traversable_dir(dir_exists: bool, is_link: bool) -> bool {
    dir_exists && !is_link
}

/// Returns true when `DirectUnpack::Run` should print the final status message (`status == nsSuccess || status == nsFailure`).
pub fn dqh_direct_unpack_should_log_final_status(
    status: i32,
    ns_success: i32,
    ns_failure: i32,
) -> bool {
    status == ns_success || status == ns_failure
}

/// Returns true when `DirectUnpack::Stop` should keep sleeping while waiting for active process break (`i < 50 && unpacking && has_active_process`).
pub fn dqh_direct_unpack_should_keep_waiting_break(
    iter: i32,
    unpacking: bool,
    has_active_process: bool,
) -> bool {
    iter < 50 && unpacking && has_active_process
}

/// Returns true when `DirectUnpack::Cleanup` should attempt to delete the temporary unpack directory (`use_temp_unpack_dir && dir_exists`).
pub fn dqh_direct_unpack_should_delete_temp_unpack_dir(
    use_temp_unpack_dir: bool,
    dir_exists: bool,
) -> bool {
    use_temp_unpack_dir && dir_exists
}

/// Returns true when `RenameController::RenameCompleted` should reset `ParRenameStatus` to `rsNone` after requesting extra PAR2 files (`remaining_size > 0`).
pub fn dqh_rename_ctrl_should_reset_par_rename_status(remaining_size: i64) -> bool {
    remaining_size > 0
}

/// Returns true when `ParParser::FindMainPars` found at least one main PAR2 file (`has_file_list && !file_list_empty`).
pub fn dqh_parparser_has_found_main_pars(has_file_list: bool, file_list_empty: bool) -> bool {
    has_file_list && !file_list_empty
}

/// Returns true when `ParRenamer::RenameFile` needs to create target directories (`old_path != new_path`).
pub fn dqh_par_renamer_needs_create_target_dir(old_path: &[u8], new_path: &[u8]) -> bool {
    old_path != new_path
}

/// Returns true when `RarVolume::Read` obtained a valid file size and rewound to 0 (`size >= 0 && seek_zero_ok`).
pub fn dqh_rar_volume_is_read_handle_ready(size: i64, seek_zero_ok: bool) -> bool {
    size >= 0 && seek_zero_ok
}

/// Returns true when `RarVolume::ReadFrom` should update `m_version` (`!has_error || signature_read`).
pub fn dqh_rar_volume_should_update_version(has_error: bool, signature_read: bool) -> bool {
    !has_error || signature_read
}

/// Returns true when `RarVolume::DecryptBuf` succeeded (`evp_ok && outlen == 16`).
pub fn dqh_rar_volume_is_decrypt_buf_valid(evp_ok: bool, out_len: i32) -> bool {
    evp_ok && out_len == 16
}

/// Returns true when `RarRenamer::CheckOneFile` should skip a file matching the ignore extension list (`has_ignore_ext && matched_ext`).
pub fn dqh_rar_renamer_should_ignore_file(has_ignore_ext: bool, matched_ext: bool) -> bool {
    has_ignore_ext && matched_ext
}

/// Returns true when `RarLister::FindLargestFile` in `DupeMatcher.cpp` should continue waiting for `unrar` (`is_running && start_time + timeout_sec > cur_time && start_time >= cur_time`).
pub fn dqh_dupematcher_should_wait_rar_lister(
    is_running: bool,
    start_time: i64,
    timeout_sec: i32,
    cur_time: i64,
) -> bool {
    is_running && start_time + i64::from(timeout_sec) > cur_time && start_time >= cur_time
}

/// Returns true when `DupeMatcher::FindLargestFile` found a new largest file (`file_size > cur_max_size`).
pub fn dqh_dupematcher_is_larger_file(file_size: i64, cur_max_size: i64) -> bool {
    file_size > cur_max_size
}

/// Returns true when `ContentMap::MapCipherRange` arguments and state are valid (`offset >= 0 && size > 0 && cryptos_len == runs_len`).
pub fn dqh_content_map_can_map_cipher_range(
    offset: i64,
    size: i64,
    cryptos_len: usize,
    runs_len: usize,
) -> bool {
    offset >= 0 && size > 0 && cryptos_len == runs_len
}

/// Classifies an unreadable volume during `ContentMapper::BuildRarMap`:
/// `1` = encrypted headers (`SkipEncryptedHeaders`),
/// `2` = second unreadable volume (`"unreadable headers in more than one volume"`),
/// `0` = record `vol_index` as `unknown_volume`.
pub fn dqh_content_mapper_classify_unreadable_rar_vol(
    has_source: bool,
    is_encrypted: bool,
    unknown_volume: i32,
) -> i32 {
    if has_source && is_encrypted {
        1
    } else if unknown_volume >= 0 {
        2
    } else {
        0
    }
}

/// Returns true when `StreamRepairController::RunLive` is attached to a live `NzbInfo` (`has_nzb && is_this_thread`).
pub fn dqh_stream_repair_is_attached_live_thread(has_nzb: bool, is_this_thread: bool) -> bool {
    has_nzb && is_this_thread
}

/// Returns true when `StreamRepairController::RepairCompletedLive` should update recovered stats (`recovered_bytes > 0 || recovered_holes > 0`).
pub fn dqh_stream_repair_has_live_recovered_stats(
    recovered_bytes: i64,
    recovered_holes: i32,
) -> bool {
    recovered_bytes > 0 || recovered_holes > 0
}

/// Returns true when `StreamRepairController::Watchdog` should reset its progress timer (`progress_bytes != prev_progress || waiting || materializing > 0`).
pub fn dqh_stream_repair_watchdog_has_activity(
    progress_bytes: i64,
    prev_progress: i64,
    waiting: bool,
    materializing: i32,
) -> bool {
    progress_bytes != prev_progress || waiting || materializing > 0
}

/// Computes remaining missing bytes (`max(0, missing_bytes - (progress - start_progress))`) in `StreamRepairController::StartWatchdog`.
pub fn dqh_stream_repair_watchdog_calc_remaining_bytes(
    missing_bytes: i64,
    progress: i64,
    start_progress: i64,
) -> i64 {
    let recovered = progress.saturating_sub(start_progress);
    missing_bytes.saturating_sub(recovered).max(0)
}

/// Returns true when `StreamRepairController::CompareToFile` has a valid seek and chunk read (`pos == offset && read_bytes == chunk`).
pub fn dqh_stream_repair_is_chunk_read_complete(read_bytes: i64, chunk: i64) -> bool {
    read_bytes == chunk
}

/// Returns true when `ArticleDownloader::Run` should break out of the server loop (`status == adFinished || status == adFatalError`).
pub fn dqh_article_dl_is_terminal_loop_status(
    status: i32,
    ad_finished: i32,
    ad_fatal_error: i32,
) -> bool {
    status == ad_finished || status == ad_fatal_error
}

/// Returns true when `ArticleDownloader::Run` should fail immediately because a desired server failed and isn't being retried (`!has_want_server && has_desired_server`).
pub fn dqh_article_dl_should_fail_for_desired_server(
    has_want_server: bool,
    has_desired_server: bool,
) -> bool {
    !has_want_server && has_desired_server
}

/// Returns true when `ArticleDownloader::Download` should continue reading/decoding body chunks (`!is_stopped && !decoder_eof`).
pub fn dqh_article_dl_should_continue_read_loop(is_stopped: bool, decoder_eof: bool) -> bool {
    !is_stopped && !decoder_eof
}

/// Returns true when `ArticleDownloader::Download` should release the connection and run `DecodeCheck()` (`status == adRunning`).
pub fn dqh_article_dl_should_run_decode_check(status: i32, ad_running: i32) -> bool {
    status == ad_running
}

/// Returns true when `ServerPool` has a valid level index with free connection slots (`level >= 0 && (level as usize) < levels_len && level_free_count > 0`).
pub fn dqh_server_pool_has_available_level_conns(
    level: i32,
    levels_len: usize,
    level_free_count: i32,
) -> bool {
    level >= 0 && (level as usize) < levels_len && level_free_count > 0
}

/// Computes `rand_val % candidates_len` for `ServerPool::LockedGetConnection`.
pub fn dqh_server_pool_pick_candidate_index(rand_val: i32, candidates_len: usize) -> usize {
    if candidates_len == 0 {
        0
    } else {
        (rand_val.unsigned_abs() as usize) % candidates_len
    }
}

/// Returns true when `ServerPool::CloseUnusedConnections` should disconnect a pooled connection on an idle level (`conn_norm_level == level && conn_status == cs_connected`).
pub fn dqh_server_pool_should_disconnect_idle_level_conn(
    conn_norm_level: i32,
    level: i32,
    conn_status: i32,
    cs_connected: i32,
) -> bool {
    conn_norm_level == level && conn_status == cs_connected
}

/// Returns true when `StatMeter::CheckQuota` has at least one quota configured (`daily_quota != 0 || monthly_quota != 0`).
pub fn dqh_statmeter_has_any_quota_configured(daily_quota: i32, monthly_quota: i32) -> bool {
    daily_quota != 0 || monthly_quota != 0
}

/// Returns true when `StatMeter::CalcMonthSlots` is before `QuotaStartDay` in the current month (`quota_start_day > cur_mday`).
pub fn dqh_statmeter_is_before_quota_start_day(quota_start_day: i32, cur_mday: i32) -> bool {
    quota_start_day > cur_mday
}

/// Returns true when `StatMeter::CalcMonthSlots` previous month day overflowed (`prev_mday != quota_start_day`).
pub fn dqh_statmeter_did_prev_month_day_overflow(prev_mday: i32, quota_start_day: i32) -> bool {
    prev_mday != quota_start_day
}

/// Classifies `NntpConnection::Connect` handshake result:
/// `0` = connected (`result == 0`),
/// `1` = disconnect & fail (`result == 1`),
/// `-1` = fail without disconnect (`result != 0 && result != 1`).
pub fn dqh_nntp_conn_classify_handshake_result(result: i32) -> i32 {
    if result == 1 {
        1
    } else if result == 0 {
        0
    } else {
        -1
    }
}

/// Returns true when `ArticleFetcher::WaitBeforeRetry` should keep sleeping (`waited < seconds * 10 && !is_stopped`).
pub fn dqh_article_fetcher_should_keep_waiting_retry(
    waited: i32,
    seconds: i32,
    is_stopped: bool,
) -> bool {
    waited < seconds.saturating_mul(10) && !is_stopped
}

/// Returns true when `ArticleFetcher::ReleaseConnection` should disconnect the connection (`!keep_connected || conn_status == cs_cancelled`).
pub fn dqh_article_fetcher_should_disconnect_on_release(
    keep_connected: bool,
    conn_status: i32,
    cs_cancelled: i32,
) -> bool {
    !keep_connected || conn_status == cs_cancelled
}

/// Returns true when `ArticleFetcher::FetchFromConnection` should treat a failed group join as transient and disconnect (`!has_response`).
pub fn dqh_article_fetcher_is_join_group_failure_transient(has_response: bool) -> bool {
    !has_response
}

/// Returns true when `ArticleBatchFetcher::Begin` should call `EnsureWorkers()` (`worker_count > 0 && !is_stopped`).
pub fn dqh_article_batch_should_ensure_workers(worker_count: i32, is_stopped: bool) -> bool {
    worker_count > 0 && !is_stopped
}

/// Returns true when `ArticleWriter::IsDupeFallbackArticle` is active (`has_article_info && dupe_fallback_round > 0`).
pub fn dqh_article_writer_is_dupe_fallback_article(
    has_article_info: bool,
    dupe_fallback_round: i32,
) -> bool {
    has_article_info && dupe_fallback_round > 0
}

/// Returns true when `ArticleWriter::HandlePostProcessing` should rename the output file because `m_fileInfo->GetFilename() != originalFilename`.
pub fn dqh_article_writer_was_renamed_during_completion(
    cur_filename: &[u8],
    orig_filename: &[u8],
) -> bool {
    cur_filename != orig_filename
}

/// Returns true when `ArticleWriter::HandlePostProcessing` should delete the hard-linked temporary file (`is_hard_linked && inter_dir_empty`).
pub fn dqh_article_writer_should_delete_hardlinked_without_interdir(
    is_hard_linked: bool,
    inter_dir_empty: bool,
) -> bool {
    is_hard_linked && inter_dir_empty
}

/// Returns true when `ArticleWriter::HandlePostProcessing` should move completed files because `DestDir` changed (`cur_dest_dir != orig_dest_dir`).
pub fn dqh_article_writer_did_dest_dir_change(
    cur_dest_dir: &[u8],
    orig_dest_dir: &[u8],
) -> bool {
    cur_dest_dir != orig_dest_dir
}

/// Returns true when `ArticleCache::Run` should continue its main loop (`!is_stopped || allocated > 0`).
pub fn dqh_article_cache_should_continue_run_loop(is_stopped: bool, allocated: usize) -> bool {
    !is_stopped || allocated > 0
}

/// Returns true when `MemoryMonitor::Run` should keep sleeping between state checks (`!is_stopped && slept < interval`).
pub fn dqh_memory_monitor_should_keep_sleeping(
    is_stopped: bool,
    slept: i32,
    interval: i32,
) -> bool {
    !is_stopped && slept < interval
}

/// Returns true when `WebDownloader::Run` should continue its retry loop (`!is_stopped && remained_download_retries > 0 && remained_connect_retries > 0`).
pub fn dqh_webdl_should_continue_retry_loop(
    is_stopped: bool,
    remained_download_retries: i32,
    remained_connect_retries: i32,
) -> bool {
    !is_stopped && remained_download_retries > 0 && remained_connect_retries > 0
}

/// Returns true when `WebDownloader::Run` should keep sleeping during the retry interval (`!is_stopped && msec < url_interval_sec * 1000 && !(!force && pause_download)`).
pub fn dqh_webdl_should_keep_waiting_retry_interval(
    is_stopped: bool,
    msec: i32,
    url_interval_sec: i32,
    force: bool,
    pause_download: bool,
) -> bool {
    !is_stopped
        && msec < url_interval_sec.saturating_mul(1000)
        && !(!force && pause_download)
}

/// Returns true when `WebDownloader::Run` has reached a terminal status (`status == adFinished || status == adFatalError || status == adNotFound`).
pub fn dqh_webdl_is_terminal_retry_status(
    status: i32,
    ad_finished: i32,
    ad_fatal_error: i32,
    ad_not_found: i32,
) -> bool {
    status == ad_finished || status == ad_fatal_error || status == ad_not_found
}

/// Returns true when a Unix-domain socket path length overflows `sun_path` (`host_len >= sun_path_cap`).
pub fn dqh_conn_is_unix_path_too_long(host_len: usize, sun_path_cap: usize) -> bool {
    host_len >= sun_path_cap
}

/// Resolves the `getaddrinfo` error code passed to `Connection::ReportError` (`if res != eai_system { res } else { 0 }`).
pub fn dqh_conn_resolve_gai_report_err_code(res: i32, eai_system: i32) -> i32 {
    if res != eai_system {
        res
    } else {
        0
    }
}

/// Returns true when `Connection::DoConnect` should log `"Socket creation failed"` (`is_invalid_socket && has_addr_list`).
pub fn dqh_conn_should_report_socket_create_failed(
    is_invalid_socket: bool,
    has_addr_list: bool,
) -> bool {
    is_invalid_socket && has_addr_list
}

/// Returns true when `Connection::DoConnect` should report connection failure and close a created socket (`!connected && !is_invalid_socket`).
pub fn dqh_conn_should_cleanup_unconnected_socket(
    connected: bool,
    is_invalid_socket: bool,
) -> bool {
    !connected && !is_invalid_socket
}

/// Returns true when `Connection::DoDisconnect` should `unlink` a listening Unix-domain socket (`is_unix_path && getsockopt_res == 0 && is_listening != 0`).
pub fn dqh_conn_should_unlink_listening_unix_socket(
    is_unix_path: bool,
    getsockopt_res: i32,
    is_listening: i32,
) -> bool {
    is_unix_path && getsockopt_res == 0 && is_listening != 0
}

/// Resolves the error code in `Connection::ReportError` (`if err_code == 0 { last_net_err } else { err_code }`).
pub fn dqh_conn_resolve_report_err_code(err_code: i32, last_net_err: i32) -> i32 {
    if err_code == 0 {
        last_net_err
    } else {
        err_code
    }
}

/// Returns true when `TlsSocket::Start` should load both certificate chain and private key files (`!cert_empty && !key_empty`).
pub fn dqh_tls_has_cert_and_key_files(cert_empty: bool, key_empty: bool) -> bool {
    !cert_empty && !key_empty
}

/// Returns true when `TlsSocket::Start` should set the SNI hostname (`is_client && !host_empty`).
pub fn dqh_tls_should_set_sni_hostname(is_client: bool, host_empty: bool) -> bool {
    is_client && !host_empty
}

/// Returns true when `TlsSocket::Start` should run `ValidateCert()` after handshake (`is_client && should_verify`).
pub fn dqh_tls_should_validate_client_cert(is_client: bool, should_verify: bool) -> bool {
    is_client && should_verify
}

/// Returns true when a TLS protocol version falls outside configured `[minimum, maximum]` bounds (`(minimum != 0 && version < minimum) || (maximum != 0 && version > maximum)`).
pub fn dqh_tls_is_proto_version_out_of_bounds(version: i32, minimum: i32, maximum: i32) -> bool {
    (minimum != 0 && version < minimum) || (maximum != 0 && version > maximum)
}

/// Returns true when `TlsSocket::Close` should perform `SSL_shutdown` (`has_session && connected && !has_deadline`).
pub fn dqh_tls_should_send_close_notify(
    has_session: bool,
    connected: bool,
    has_deadline: bool,
) -> bool {
    has_session && connected && !has_deadline
}

/// Returns true when `WebDownloader::Download` should abort right after `Connect()` (`!connected || is_stopped`).
pub fn dqh_webdl_is_connect_aborted(connected: bool, is_stopped: bool) -> bool {
    !connected || is_stopped
}

/// Returns true when `WebDownloader::Download` should proceed to `DownloadBody()` (`status == ad_running`).
pub fn dqh_webdl_should_download_body(status: i32, ad_running: i32) -> bool {
    status == ad_running
}

/// Returns true when `WebDownloader::Download` should delete a broken partial output file (`status != ad_finished`).
pub fn dqh_webdl_should_delete_broken_output(status: i32, ad_finished: i32) -> bool {
    status != ad_finished
}

/// Returns true when `WebDownloader::Write` needs to call `PrepareFile()` (`!out_file_active`).
pub fn dqh_webdl_needs_prepare_file(out_file_active: bool) -> bool {
    !out_file_active
}

/// Returns true when `WebDownloader::Write` should fail after gunzipping a chunk (`out_len > 0 && written_bytes <= 0`).
pub fn dqh_webdl_is_gunzip_write_failed(out_len: i32, written_bytes: i64) -> bool {
    out_len > 0 && written_bytes <= 0
}

/// Returns true when `WebDownloader::FreeConnection` should disconnect a cancelled connection (`conn_status == cs_cancelled`).
pub fn dqh_webdl_should_disconnect_on_free(conn_status: i32, cs_cancelled: i32) -> bool {
    conn_status == cs_cancelled
}

/// Returns true when `Network::SpeedTest::RunTest` should count an iteration as a failure (`elapsed_sec == 0.0 || downloaded < target_size`).
pub fn dqh_speed_test_is_iteration_failed(
    elapsed_sec: f64,
    downloaded: u32,
    target_size: u32,
) -> bool {
    elapsed_sec == 0.0 || downloaded < target_size
}

/// Returns true when `Network::SpeedTest::ExecuteTest` should break from its receive loop (`received <= 0 || elapsed_sec >= timeout_sec`).
pub fn dqh_speed_test_should_stop_recv_loop(
    received: i32,
    elapsed_sec: f64,
    timeout_sec: f64,
) -> bool {
    received <= 0 || elapsed_sec >= timeout_sec
}

/// Populates the deterministic AEAD benchmark plaintext buffer (`(i * 31 + i / 251) & 255`) in `TlsCipherBackend.cpp`.
pub fn dqh_tls_cipher_fill_test_plaintext(buf: &mut [u8]) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = ((i * 31 + i / 251) & 255) as u8;
    }
}

/// Returns true when `Crypto::Prepare` batch arguments in `TlsCipherBackend.cpp` are valid (`count > 0 && count <= 64 && sequence <= u64::MAX - u64::from(count)`).
pub fn dqh_tls_cipher_is_prepare_batch_valid(sequence: u64, count: u32) -> bool {
    count > 0 && count <= 64 && sequence <= u64::MAX - u64::from(count)
}

/// Writes a 12-byte TLS record nonce (`prefix[0..4]` followed by big-endian `seq`) in `TlsCipherBackend.cpp`.
pub fn dqh_tls_cipher_write_record_nonce(prefix: &[u8; 4], seq: u64, out_nonce: &mut [u8; 12]) {
    out_nonce[..4].copy_from_slice(prefix);
    out_nonce[4..12].copy_from_slice(&seq.to_be_bytes());
}

/// Returns true when `sequence` is within `[first, first + count)` in `Crypto::Record` (`TlsCipherBackend.cpp`).
pub fn dqh_tls_cipher_is_record_seq_in_range(sequence: u64, first: u64, count: usize) -> bool {
    sequence >= first && (sequence - first) < (count as u64)
}

/// Prechecks `Probe` arguments in `TlsCipherBackend.cpp`:
/// `0` = proceed, `-6` = cancelled, `-3` = zero timeout, `-1` = timeout > 10000ms, `-4` = missing cert store when verification enabled.
pub fn dqh_tls_cipher_precheck_probe_args(
    cancelled: bool,
    milliseconds: u64,
    verification: bool,
    has_cert_store: bool,
) -> i32 {
    if cancelled {
        -6
    } else if milliseconds == 0 {
        -3
    } else if milliseconds > 10000 {
        -1
    } else if verification && !has_cert_store {
        -4
    } else {
        0
    }
}

/// Returns true when `Probe` in `TlsCipherBackend.cpp` has a valid resolved address count (`addr_count > 0 && addr_count <= 16`).
pub fn dqh_tls_cipher_is_probe_addr_list_valid(addr_count: usize) -> bool {
    addr_count > 0 && addr_count <= 16
}

/// Returns true when `QueueCoordinator::Load` should clean up the temporary directory (`queue_loaded && stat_loaded`).
pub fn dqh_queue_coord_should_cleanup_temp_dir(queue_loaded: bool, stat_loaded: bool) -> bool {
    queue_loaded && stat_loaded
}

/// Returns true when `QueueCoordinator::Load` should immediately resave a legacy-format queue (`queue_loaded && legacy_queue_read`).
pub fn dqh_queue_coord_should_resave_legacy_queue(
    queue_loaded: bool,
    legacy_queue_read: bool,
) -> bool {
    queue_loaded && legacy_queue_read
}

/// Returns true when `QueueCoordinator::Load` should resave queue and server stats after a server mismatch (`queue_loaded && stat_loaded && !perfect_server_match`).
pub fn dqh_queue_coord_should_resave_on_server_mismatch(
    queue_loaded: bool,
    stat_loaded: bool,
    perfect_server_match: bool,
) -> bool {
    queue_loaded && stat_loaded && !perfect_server_match
}

/// Returns true when an `NzbInfo` entry in `QueueCoordinator::GetNextArticle` is eligible for candidate file selection.
pub fn dqh_queue_coord_is_nzb_eligible_for_next_article(
    has_file_info: bool,
    nzb_higher_priority: bool,
    nzb_paused: bool,
    has_active_dupe: bool,
    pause_download: bool,
    quota_reached: bool,
    force_priority: bool,
) -> bool {
    (!has_file_info || nzb_higher_priority)
        && !nzb_paused
        && !has_active_dupe
        && (!(pause_download || quota_reached) || force_priority)
}

/// Returns true when `QueueCoordinator::GetNextArticle` should log a propagation-wait notice (`has_prop_file && prop_file_id != last_logged_id`).
pub fn dqh_queue_coord_should_log_propagation_wait(
    has_prop_file: bool,
    prop_file_id: i32,
    last_logged_id: i32,
) -> bool {
    has_prop_file && prop_file_id != last_logged_id
}

/// Returns true when `QueueCoordinator::GetNextFirstArticle` can select the first article of a file (`!filename_confirmed && !articles_empty && first_status == ai_undefined`).
pub fn dqh_queue_coord_can_select_first_article(
    filename_confirmed: bool,
    articles_empty: bool,
    first_status: i32,
    ai_undefined: i32,
) -> bool {
    !filename_confirmed && !articles_empty && first_status == ai_undefined
}

/// Returns true when `QueueCoordinator::ArticleCompleted` should check segment alignment on a finished donor article (`status == ad_finished && dupe_fallback_round > 0 && decoded_file_size > 0`).
pub fn dqh_queue_coord_should_check_donor_alignment(
    status: i32,
    ad_finished: i32,
    dupe_fallback_round: i32,
    decoded_file_size: i64,
) -> bool {
    status == ad_finished && dupe_fallback_round > 0 && decoded_file_size > 0
}

/// Returns true when `QueueCoordinator::ArticleCompleted` can attempt reactive duplicate fallback (`status == ad_failed || misplaced`).
pub fn dqh_queue_coord_can_try_reactive_dupe_fallback(
    status: i32,
    ad_failed: i32,
    misplaced: bool,
) -> bool {
    status == ad_failed || misplaced
}

/// Returns true when `QueueCoordinator::ArticleCompleted` should record a terminal article failure (`status == ad_failed || misplaced || status == ad_fatal_error`).
pub fn dqh_queue_coord_is_terminal_article_failure(
    status: i32,
    ad_failed: i32,
    ad_fatal_error: i32,
    misplaced: bool,
) -> bool {
    status == ad_failed || misplaced || status == ad_fatal_error
}

/// Returns true when `QueueCoordinator::ArticleCompleted` should confirm the filename from a finished article (`!filename_confirmed && !misplaced && status == ad_finished && has_article_filename`).
pub fn dqh_queue_coord_should_confirm_article_filename(
    filename_confirmed: bool,
    misplaced: bool,
    status: i32,
    ad_finished: i32,
    has_article_filename: bool,
) -> bool {
    !filename_confirmed && !misplaced && status == ad_finished && has_article_filename
}

/// Returns true when `QueueCoordinator::ArticleCompleted` should check whether a confirmed destination file already exists (`dupe_check && dupe_mode != dm_force && !many_dupe_files`).
pub fn dqh_queue_coord_should_check_existing_dest_dupe(
    dupe_check: bool,
    dupe_mode: i32,
    dm_force: i32,
    many_dupe_files: bool,
) -> bool {
    dupe_check && dupe_mode != dm_force && !many_dupe_files
}

/// Returns true when `QueueCoordinator::ArticleCompleted` should process a finished article's `ContentAnalyzer` (`has_analyzer && !misplaced && status == ad_finished`).
pub fn dqh_queue_coord_should_process_analyzed_first_article(
    has_analyzer: bool,
    misplaced: bool,
    status: i32,
    ad_finished: i32,
) -> bool {
    has_analyzer && !misplaced && status == ad_finished
}

/// Returns true when `QueueCoordinator::DeleteFileInfo` should log the per-file duplicate recovery summary (`completed && dupe_attempted_articles > 0`).
pub fn dqh_queue_coord_should_log_file_dupe_recovery_summary(
    completed: bool,
    dupe_attempted_articles: i32,
) -> bool {
    completed && dupe_attempted_articles > 0
}

/// Returns true when `QueueCoordinator::DeleteFileInfo` should update completed stats instead of deleted stats (`completed || nzb_deleting`).
pub fn dqh_queue_coord_should_update_completed_stats_on_delete(
    completed: bool,
    nzb_deleting: bool,
) -> bool {
    completed || nzb_deleting
}

/// Returns true when `QueueCoordinator::DeleteFileInfo` needs to check on-disk existence of a failed completed file (`completed && file_status == cf_failure && !filename_empty`).
pub fn dqh_queue_coord_should_check_failed_output_exists(
    completed: bool,
    file_status: i32,
    cf_failure: i32,
    filename_empty: bool,
) -> bool {
    completed && file_status == cf_failure && !filename_empty
}

/// Returns true when `QueueCoordinator::ValidateCompletedFileTiling` needs to inspect donor tiling (`success_articles == total_articles`).
pub fn dqh_queue_coord_needs_completed_tiling_validation(
    success_articles: i32,
    total_articles: i32,
) -> bool {
    success_articles == total_articles
}

/// Returns true when `QueueCoordinator::LoadPartialState` needs to initialize output filename for direct write (`(direct_write || force_direct_write) && !output_initialized`).
pub fn dqh_queue_coord_needs_partial_direct_write_init(
    direct_write: bool,
    force_direct_write: bool,
    output_initialized: bool,
) -> bool {
    (direct_write || force_direct_write) && !output_initialized
}

/// Returns true when `QueueCoordinator::CheckHealth` should skip critical health calculation (`health_check == hc_none || health_paused || delete_status == ds_health`).
pub fn dqh_queue_coord_should_skip_critical_health_calc(
    health_check: i32,
    hc_none: i32,
    health_paused: bool,
    delete_status: i32,
    ds_health: i32,
) -> bool {
    health_check == hc_none || health_paused || delete_status == ds_health
}

/// Returns true when `QueueCoordinator::DownloadCriticalHealth` should check `CertainlyParless` (`health_check == hc_dupe && critical_exact == 1000`).
pub fn dqh_queue_coord_should_check_certainly_parless(
    health_check: i32,
    hc_dupe: i32,
    critical_exact: i32,
) -> bool {
    health_check == hc_dupe && critical_exact == 1000
}

/// Returns the effective health passed to `DupeFailoverWarranted` in `QueueCoordinator::CheckDupeFailover` (`0` if `dl_crit_health > base_crit_health`, else `cur_health`).
pub fn dqh_queue_coord_select_failover_effective_health(
    dl_crit_health: i32,
    base_crit_health: i32,
    cur_health: i32,
) -> i32 {
    if dl_crit_health > base_crit_health {
        0
    } else {
        cur_health
    }
}

/// Returns true when `QueueCoordinator::CheckDeadDownload` should return early without failing over or parking (`(!dead && !doomed) || (probing >= 0 && !hopeless)`).
pub fn dqh_queue_coord_should_abort_dead_download_action(
    dead: bool,
    doomed: bool,
    probing: i32,
    hopeless: bool,
) -> bool {
    (!dead && !doomed) || (probing >= 0 && !hopeless)
}

/// Validates whether two `NzbInfo` entries in `QueueCoordinator::MergeQueueEntries` can be merged:
/// returns `1` if either is in post-process stage, `2` if either is a URL entry, or `0` if valid.
pub fn dqh_queue_coord_validate_merge_entries(
    dest_has_post: bool,
    src_has_post: bool,
    dest_kind: i32,
    src_kind: i32,
    nk_url: i32,
) -> i32 {
    if dest_has_post || src_has_post {
        1
    } else if dest_kind == nk_url || src_kind == nk_url {
        2
    } else {
        0
    }
}

/// Returns true when `QueueCoordinator::DirectRenameCompleted` should resume the group and pause all pars (`par_check != pc_force`).
pub fn dqh_queue_coord_should_pause_pars_after_direct_rename(
    par_check: i32,
    pc_force: i32,
) -> bool {
    par_check != pc_force
}

/// Returns true when `QueueCoordinator::DiscardDirectRename` should log discarded size (`discarded_size > 0`).
pub fn dqh_queue_coord_should_log_discarded_direct_rename(discarded_size: i64) -> bool {
    discarded_size > 0
}

/// Resolves `m_readVersion` in `StateFile::StateFile` (`read_version` if non-zero, else `format_version`).
pub fn dqh_diskstate_resolve_read_version(read_version: i32, format_version: i32) -> i32 {
    if read_version != 0 {
        read_version
    } else {
        format_version
    }
}

/// Returns true when `StateFile::FileExists` finds the destination or transactional temp file (`dest_exists || (transactional && temp_exists)`).
pub fn dqh_diskstate_state_file_exists(
    dest_exists: bool,
    transactional: bool,
    temp_exists: bool,
) -> bool {
    dest_exists || (transactional && temp_exists)
}

/// Returns true when `StateFile::FinishWrite` should flush file or directory buffers (`flush && flush_queue_opt`).
pub fn dqh_diskstate_should_flush_queue_file(flush: bool, flush_queue_opt: bool) -> bool {
    flush && flush_queue_opt
}

/// Returns true when `StateFile::BeginRead` detects an unsupported newer file version (`file_version > read_version`).
pub fn dqh_diskstate_is_file_version_unsupported(file_version: i32, read_version: i32) -> bool {
    file_version > read_version
}

/// Returns true when `DiskState::LoadDownloadQueue` should load the separate `history` state file (`format_version == 0 || format_version >= 57`).
pub fn dqh_diskstate_should_load_separate_history_file(format_version: i32) -> bool {
    format_version == 0 || format_version >= 57
}

/// Returns true when `DiskState::LoadDownloadQueue` error cleanup should still load file infos (`!file_infos_loaded && !queue_empty`).
pub fn dqh_diskstate_should_reload_fallback_file_infos(
    file_infos_loaded: bool,
    queue_empty: bool,
) -> bool {
    !file_infos_loaded && !queue_empty
}

/// Returns the expected field count for a stream-repair job header in `DiskState::LoadDupeExtras` (`11` if `format_version >= 66`, else `9`).
pub fn dqh_diskstate_expected_repair_job_fields(format_version: i32) -> i32 {
    if format_version >= 66 {
        11
    } else {
        9
    }
}

/// Returns true when a `%i` count scan in `DiskState` failed or produced a negative count (`scan_cnt != 1 || count < 0`).
pub fn dqh_diskstate_is_nonneg_count_scan_invalid(scan_cnt: i32, count: i32) -> bool {
    scan_cnt != 1 || count < 0
}

/// Returns true when `DiskState::LoadNzbInfo` should read the `MetaName` line (`format_version >= 64 && format_version != 65`).
pub fn dqh_diskstate_has_meta_name_field(format_version: i32) -> bool {
    format_version >= 64 && format_version != 65
}

/// Returns true when `DiskState::LoadServerStats` matches a server's `StateId` (`news_server_state_id == server_id`).
pub fn dqh_diskstate_matches_server_state_id(news_server_state_id: i32, server_id: i32) -> bool {
    news_server_state_id == server_id
}

/// Selects `orig_msg_id` when non-empty, otherwise `cur_msg_id` (`DiskState::SaveFileInfo` and `DupeArticleFallback::NzbFilenameOf`).
pub fn dqh_diskstate_select_original_or_current_msg_id<'a>(
    orig_msg_id: &'a [u8],
    cur_msg_id: &'a [u8],
) -> &'a [u8] {
    if !orig_msg_id.is_empty() {
        orig_msg_id
    } else {
        cur_msg_id
    }
}

/// Returns true when `DiskState::LoadFileInfo` should probe on-disk direct-write files (`output_initialized != 0 && (direct_write_opt || force_direct_write)`).
pub fn dqh_diskstate_should_probe_direct_write_files(
    output_initialized: i32,
    direct_write_opt: bool,
    force_direct_write: bool,
) -> bool {
    output_initialized != 0 && (direct_write_opt || force_direct_write)
}

/// Returns true when `DiskState::LoadFileState` can check for a completed destination file (`(completed || force_direct_write) && !dest_dir_empty && !filename_empty`).
pub fn dqh_diskstate_can_check_completed_dest_file(
    completed: bool,
    force_direct_write: bool,
    dest_dir_empty: bool,
    filename_empty: bool,
) -> bool {
    (completed || force_direct_write) && !dest_dir_empty && !filename_empty
}

/// Returns true when `DiskState::LoadHistory` matches a history entry's `NzbInfo` by ID (`(kind == hk_nzb || kind == hk_url) && nzb_id == target_id`).
pub fn dqh_diskstate_matches_history_nzb_id(
    kind: i32,
    hk_nzb: i32,
    hk_url: i32,
    nzb_id: i32,
    target_id: i32,
) -> bool {
    (kind == hk_nzb || kind == hk_url) && nzb_id == target_id
}

/// Returns true when `DiskState::SaveFeeds` should discard the feeds file (`feeds_empty && feed_history_empty`).
pub fn dqh_diskstate_should_discard_feeds_file(
    feeds_empty: bool,
    feed_history_empty: bool,
) -> bool {
    feeds_empty && feed_history_empty
}

/// Returns true when `DiskState::LoadVolumeStat` can store a scanned value at `index` (`has_volume_array && index >= 0 && (index as usize) < array_size`).
pub fn dqh_diskstate_can_store_volume_slot(
    has_volume_array: bool,
    index: i32,
    array_size: usize,
) -> bool {
    has_volume_array && index >= 0 && (index as usize) < array_size
}

/// Returns true when `DupeArticleFallback::TryFallback` should immediately skip fallback (`fallback_mode == daf_none || raw_article || is_par_file`).
pub fn dqh_dupe_fallback_should_skip_try_entry(
    fallback_mode: i32,
    daf_none: i32,
    raw_article: bool,
    is_par_file: bool,
) -> bool {
    fallback_mode == daf_none || raw_article || is_par_file
}

/// Returns true when `DupeArticleFallback::TryFallback` can defer recovery to par-check (`par_check != pc_manual && should_defer_to_par`).
pub fn dqh_dupe_fallback_can_defer_to_par(
    par_check: i32,
    pc_manual: i32,
    should_defer_to_par: bool,
) -> bool {
    par_check != pc_manual && should_defer_to_par
}

/// Returns true when `DupeArticleFallback::TryFallback` should advance past a blocked donor round (`round >= 0 && (round as usize) < sources_len && (round as usize) < donors_len && is_blocked`).
pub fn dqh_dupe_fallback_should_skip_blocked_donor_round(
    round: i32,
    sources_len: usize,
    donors_len: usize,
    is_blocked: bool,
) -> bool {
    round >= 0 && (round as usize) < sources_len && (round as usize) < donors_len && is_blocked
}

/// Returns true when `DupeArticleFallback::RotateToLead` should rotate the donor list (`idx > 0 && (idx as usize) < donors_len`).
pub fn dqh_dupe_fallback_should_rotate_donors(idx: isize, donors_len: usize) -> bool {
    idx > 0 && (idx as usize) < donors_len
}

/// Returns true when `DupeArticleFallback::PinSources` needs to check whether the current lead is a twin (`twin_only && lead_donor_id != 0`).
pub fn dqh_dupe_fallback_needs_twin_lead_check(twin_only: bool, lead_donor_id: i32) -> bool {
    twin_only && lead_donor_id != 0
}

/// Returns true when `DupeArticleFallback::ParseDonorNzb` can parse a donor NZB file (`!filename_empty && file_exists`).
pub fn dqh_dupe_fallback_can_parse_donor_nzb(filename_empty: bool, file_exists: bool) -> bool {
    !filename_empty && file_exists
}

/// Returns true when `DupeArticleFallback::MatchDonorFile` can query `TwinCheck::MatchByContent` (`donor_nzb_id != 0 && !own_identity_empty && !donor_print_empty`).
pub fn dqh_dupe_fallback_can_query_content_match(
    donor_nzb_id: i32,
    own_identity_empty: bool,
    donor_print_empty: bool,
) -> bool {
    donor_nzb_id != 0 && !own_identity_empty && !donor_print_empty
}

/// Returns true when a donor file in `DupeArticleFallback::MatchDonorFile` is a structural candidate (`!is_par_file && structure_matches`).
pub fn dqh_dupe_fallback_is_donor_file_struct_candidate(
    is_par_file: bool,
    structure_matches: bool,
) -> bool {
    !is_par_file && structure_matches
}

/// Returns true when `HistoryCoordinator::PrepareEdit` should mark a history entry as bad (`action == ea_history_mark_bad && has_history_info && kind == hk_nzb`).
pub fn dqh_history_coord_should_mark_bad_in_prepare_edit(
    action: i32,
    ea_history_mark_bad: i32,
    has_history_info: bool,
    kind: i32,
    hk_nzb: i32,
) -> bool {
    action == ea_history_mark_bad && has_history_info && kind == hk_nzb
}

/// Returns true when `HistoryCoordinator::HistoryRetry` should load partial file state (`file_loaded && status == cf_partial`).
pub fn dqh_history_coord_should_load_partial_file_state(
    file_loaded: bool,
    status: i32,
    cf_partial: i32,
) -> bool {
    file_loaded && status == cf_partial
}

/// Classifies how `HistoryCoordinator::HistoryRetry` should update partial state for a file with `success_articles > 0`:
/// `0` = no change, `1` = set `ps_completed`, `2` = warn and set `ps_none`.
pub fn dqh_history_coord_classify_success_output_state(
    success_articles: i32,
    file_exists: bool,
    reprocess: bool,
) -> i32 {
    if success_articles <= 0 {
        0
    } else if file_exists {
        1
    } else if !reprocess {
        2
    } else {
        0
    }
}

/// Selects `final_dir` when non-empty, otherwise `dest_dir` (`DupeCoordinator::FilesOnDisk`).
pub fn dqh_dupecoord_select_disk_dir<'a>(final_dir: &'a [u8], dest_dir: &'a [u8]) -> &'a [u8] {
    if !final_dir.is_empty() {
        final_dir
    } else {
        dest_dir
    }
}

/// Returns true when `DupeCoordinator::NzbFound` finds an active queued duplicate blocking backup return (`!is_self && delete_status == ds_none && same_name_or_key`).
pub fn dqh_dupecoord_is_unbacked_active_queue_dupe(
    is_self: bool,
    delete_status: i32,
    ds_none: i32,
    same_name_or_key: bool,
) -> bool {
    !is_self && delete_status == ds_none && same_name_or_key
}

/// Returns true when `DupeCoordinator::NzbFound` should return early for a URL redownload without same-content match (`!same_content && dupe_hint != dh_none`).
pub fn dqh_dupecoord_should_skip_url_redownload_non_same_content(
    same_content: bool,
    dupe_hint: i32,
    dh_none: i32,
) -> bool {
    !same_content && dupe_hint != dh_none
}

/// Returns true when `DupeCoordinator::NoBackupReason` should check whether the queued NZB file exists (`(delete_status == ds_dupe || delete_status == ds_copy) && dupe_mode != dm_force`).
pub fn dqh_dupecoord_should_check_no_backup_file_exists(
    delete_status: i32,
    ds_dupe: i32,
    ds_copy: i32,
    dupe_mode: i32,
    dm_force: i32,
) -> bool {
    (delete_status == ds_dupe || delete_status == ds_copy) && dupe_mode != dm_force
}

/// Classifies the `QueueEditor::PrepareList` branch:
/// `1` = file move with non-zero offset, `2` = group move with non-zero offset or before/after,
/// `3` = file action (`action < ea_group_move_offset`), `4` = group action.
pub fn dqh_queue_editor_classify_prepare_list_branch(
    offset: i32,
    action: i32,
    ea_file_move_offset: i32,
    ea_file_move_top: i32,
    ea_file_move_bottom: i32,
    ea_group_move_offset: i32,
    ea_group_move_top: i32,
    ea_group_move_bottom: i32,
    ea_group_move_before: i32,
    ea_group_move_after: i32,
) -> i32 {
    if offset != 0
        && (action == ea_file_move_offset
            || action == ea_file_move_top
            || action == ea_file_move_bottom)
    {
        1
    } else if (offset != 0
        && (action == ea_group_move_offset
            || action == ea_group_move_top
            || action == ea_group_move_bottom))
        || action == ea_group_move_before
        || action == ea_group_move_after
    {
        2
    } else if action < ea_group_move_offset {
        3
    } else {
        4
    }
}

/// Returns `(is_pause_pars_action, extra_pars_only)` for `QueueEditor::InternEditList`.
pub fn dqh_queue_editor_is_pause_pars_action(
    action: i32,
    ea_file_pause_all_pars: i32,
    ea_file_pause_extra_pars: i32,
) -> (bool, bool) {
    let is_pause = action == ea_file_pause_all_pars || action == ea_file_pause_extra_pars;
    let extra_only = action == ea_file_pause_extra_pars;
    (is_pause, extra_only)
}

/// Returns true when `Scanner::FindArchives` can include a regular file (`is_archive && can_process_file`).
pub fn dqh_scanner_can_process_archive_entry(is_archive: bool, can_process_file: bool) -> bool {
    is_archive && can_process_file
}

/// Returns true when `Scanner::AddArchive` should attach `urlInfo` to the extracted NZB entry (`index == 0 && has_url_info`).
pub fn dqh_scanner_should_assign_url_info_to_archive_entry(
    index: usize,
    has_url_info: bool,
) -> bool {
    index == 0 && has_url_info
}

/// Returns true when `TwinCheckJob::Run` unregister guard should nudge `TwinCheck` (`!stopping && stored`).
pub fn dqh_twin_check_should_unregister_nudge(stopping: bool, stored: bool) -> bool {
    !stopping && stored
}

/// Returns true when `TwinCheck::ServiceWork` should queue a fingerprint job for an item (`(print_empty || listless) && !is_merged && file_exists`).
pub fn dqh_twin_check_can_queue_fingerprint_job(
    print_empty: bool,
    listless: bool,
    is_merged: bool,
    file_exists: bool,
) -> bool {
    (print_empty || listless) && !is_merged && file_exists
}

/// Returns true when `TwinCheck::ServiceWork` should queue a sample job for an item (`sampleable && !sampled_now && !is_merged && file_exists`).
pub fn dqh_twin_check_can_queue_sample_job(
    sampleable: bool,
    sampled_now: bool,
    is_merged: bool,
    file_exists: bool,
) -> bool {
    sampleable && !sampled_now && !is_merged && file_exists
}

/// Returns true when `TwinCheck::ServiceWork` should look up `Kind` by stored file lists (`!is_primary && is_dupe && by_par2`).
pub fn dqh_twin_check_should_lookup_kind_by_files(
    is_primary: bool,
    is_dupe: bool,
    by_par2: bool,
) -> bool {
    !is_primary && is_dupe && by_par2
}

/// Classifies how `DirectRenamer` should build a new filename (`1` = `BuildNewParName`, `2` = `BuildNewRegularName`, `0` = none).
pub fn dqh_direct_renamer_classify_rename_target(
    is_par_file: bool,
    need_rename_pars: bool,
) -> i32 {
    if is_par_file && need_rename_pars {
        1
    } else if !is_par_file {
        2
    } else {
        0
    }
}

/// Returns true when `NzbInfo::SetFilename` should auto-populate `m_name` (`(!has_name || !had_filename) && !filename_empty`).
pub fn dqh_dl_info_should_update_auto_filename_name(
    has_name: bool,
    had_filename: bool,
    filename_empty: bool,
) -> bool {
    (!has_name || !had_filename) && !filename_empty
}

/// Returns true when `NzbInfo::AddMessage` should append the message to disk (`server_mode && nzb_log`).
pub fn dqh_dl_info_should_append_message_to_disk(server_mode: bool, nzb_log: bool) -> bool {
    server_mode && nzb_log
}

/// Classifies how `DupeStreamRepair::BuildRepairJob` should build a repair job:
/// `0` = skip (`false`), `1` = whole-file hole job (`true`), `2` = compute holes from decoded articles.
pub fn dqh_stream_repair_classify_build_job_mode(
    empty_or_no_articles: bool,
    success_articles: i32,
    encoded_size: i64,
    decoded_file_size: i64,
) -> i32 {
    if empty_or_no_articles {
        0
    } else if success_articles == 0 {
        if encoded_size <= 0 {
            0
        } else {
            1
        }
    } else if decoded_file_size <= 0 {
        0
    } else {
        2
    }
}

/// Returns true when `NzbFile::Parse_StartElement` should create an `ArticleInfo` for a `<segment>` (`part_number > 0`).
pub fn dqh_nzbfile_should_create_segment_article(part_number: i32) -> bool {
    part_number > 0
}

/// Returns true when `DupeProbe::ProbeServer` should stop probing early (`!count_all && found_count >= min_alive_samples`).
pub fn dqh_dupe_probe_should_stop_early_on_found(
    count_all: bool,
    found_count: i32,
    min_alive_samples: i32,
) -> bool {
    !count_all && found_count >= min_alive_samples
}

/// Returns true when `DupeProbe::ProbeServer` should disconnect the connection before freeing (`is_stopped || conn_status == cs_cancelled`).
pub fn dqh_dupe_probe_should_disconnect_after_probe(
    is_stopped: bool,
    conn_status: i32,
    cs_cancelled: i32,
) -> bool {
    is_stopped || conn_status == cs_cancelled
}

/// Returns true when `UrlCoordinator::Run` should wait on `m_waitCond` (`!has_more_jobs && !is_stopped`).
pub fn dqh_url_coord_should_wait_on_cond(has_more_jobs: bool, is_stopped: bool) -> bool {
    !has_more_jobs && !is_stopped
}

/// Classifies how `ParDamage::FileCompleted` handles a completed file:
/// `1` = par file (`parFilesDone++`), `2` = undamaged data file, `3` = damaged with decoded size (`ComputeHoles`),
/// `4` = unarrived whole file (`success_articles == 0`), `5` = articles disagreed on size.
pub fn dqh_par_damage_classify_completed_file(
    is_par: bool,
    failed_plus_missed: i32,
    decoded_size: i64,
    success_articles: i32,
) -> i32 {
    if is_par {
        1
    } else if failed_plus_missed == 0 {
        2
    } else if decoded_size > 0 {
        3
    } else if success_articles == 0 {
        4
    } else {
        5
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
        assert!(dqh_dupe_fallback_should_append_primary_on_order(true, 2));
        assert!(!dqh_dupe_fallback_should_append_primary_on_order(false, 2));
        assert!(!dqh_dupe_fallback_should_append_primary_on_order(true, 0));
        assert_eq!(
            dqh_stream_repair_format_whole_files_suffix(3),
            b" and 3 whole file(s)"
        );
        assert!(dqh_stream_repair_format_whole_files_suffix(0).is_empty());
        assert_eq!(dqh_stream_repair_bytes_to_mb(1048576), 1.0);
        assert!(dqh_stream_repair_is_verification_satisfied(64, 64, true));
        assert!(!dqh_stream_repair_is_verification_satisfied(63, 64, true));
        assert!(!dqh_stream_repair_is_verification_satisfied(64, 64, false));
        assert!(dqh_article_writer_use_target_path_directly(
            true, false, true, false
        ));
        assert!(dqh_article_writer_use_target_path_directly(
            false, true, false, true
        ));
        assert!(!dqh_article_writer_use_target_path_directly(
            false, true, true, true
        ));
        assert!(dqh_article_dl_is_server_available(true, 4, false, false));
        assert!(!dqh_article_dl_is_server_available(true, 0, false, false));
        assert!(!dqh_article_dl_is_server_available(true, 4, true, true));

        let (uniq, formatted) = dqh_queue_coord_format_donor_id_list(&[12, 5, 12, 8]);
        assert_eq!(uniq, vec![5, 8, 12]);
        assert_eq!(formatted, b"5, 8, 12");
        assert_eq!(
            dqh_queue_coord_eval_completed_file_repair_eligibility(true, 2, 2, 3, true),
            (true, false)
        );
        assert_eq!(
            dqh_queue_coord_eval_completed_file_repair_eligibility(true, 2, 2, 3, false),
            (false, true)
        );
        assert!(dqh_queue_coord_is_size_twin(
            false,
            true,
            1_000_000,
            1_100_000,
            100_000,
            500_000,
            &[]
        ));
        assert!(dqh_queue_coord_is_size_twin(
            false,
            true,
            1_000_000,
            2_000_000,
            0,
            500_000,
            &[100_000, 500_500]
        ));
        assert!(!dqh_queue_coord_is_size_twin(
            true,
            true,
            1_000_000,
            1_000_000,
            0,
            500_000,
            &[]
        ));
        assert!(dqh_queue_coord_can_check_dead_download(
            3, 3, true, true, 0, 0, false, false, 0, 0
        ));
        assert!(!dqh_queue_coord_can_check_dead_download(
            3, 3, true, true, 0, 0, true, false, 0, 0
        ));
        assert!(dqh_queue_coord_is_active_unpaused_queued_dupe(
            false, 100, 200, true
        ));
        assert!(!dqh_queue_coord_is_active_unpaused_queued_dupe(
            false, 200, 200, true
        ));
        assert!(dqh_queue_coord_can_fail_over_dead_pick(
            true, false, false, 0, 0
        ));
        assert!(!dqh_queue_coord_can_fail_over_dead_pick(
            false, false, false, 0, 0
        ));
        assert_eq!(
            dqh_queue_coord_download_debug_state_label(true, false),
            b"paused\0"
        );
        assert_eq!(
            dqh_queue_coord_download_debug_state_label(false, true),
            b"temp-paused\0"
        );
        assert_eq!(
            dqh_queue_coord_download_debug_state_label(false, false),
            b"active\0"
        );
        assert_eq!(dqh_prepost_permille_to_percent(985), 98.5);
        assert_eq!(
            dqh_dupecoord_format_queued_dupe_skip_message(b"Rel.A", b"Rel.A"),
            b"Skipping duplicate Rel.A, already queued"
        );
        assert_eq!(
            dqh_dupecoord_format_queued_dupe_skip_message(b"Rel.A", b"Rel.B"),
            b"Skipping duplicate Rel.A, already queued as Rel.B"
        );
        assert_eq!(
            dqh_dupecoord_format_history_dupe_skip_message(b"Rel.A", b"Rel.A", false, true),
            b"Skipping duplicate Rel.A, found in history with good status"
        );
        assert_eq!(
            dqh_dupecoord_format_history_dupe_skip_message(b"Rel.A", b"Rel.B", false, false),
            b"Skipping duplicate Rel.A, found in history Rel.B with success status"
        );
        assert_eq!(
            dqh_dupecoord_eval_completed_stop_others(true, false, 0, 0, true, 4, 4, 24, 25),
            (true, 24)
        );
        assert_eq!(
            dqh_dupecoord_eval_completed_stop_others(true, true, 0, 0, true, 3, 4, 24, 25),
            (false, 25)
        );
        assert_eq!(
            dqh_dupecoord_format_non_failover_reason(2, 2),
            b"its DupeMode is force\0"
        );
        assert_eq!(
            dqh_dupecoord_format_non_failover_reason(1, 2),
            b"its DupeMode is all\0"
        );
        assert_eq!(dqh_dupecoord_mark_status_label(1), b"bad\0");
        assert_eq!(dqh_dupecoord_mark_status_label(2), b"good\0");
        assert_eq!(dqh_dupecoord_mark_status_label(3), b"success\0");
        assert_eq!(dqh_dupecoord_mark_status_label(0), b"NONE\0");
        assert_eq!(
            dqh_nzbinfo_format_nice_url_name(
                b"https://host/p",
                b"Movie",
                true,
                true,
                b"host",
                b"/p"
            ),
            b"Movie @ host"
        );
        assert_eq!(
            dqh_nzbinfo_format_nice_url_name(b"https://host/p", b"", false, true, b"host", b"/p"),
            b"host/p"
        );
        assert_eq!(
            dqh_nzbinfo_format_final_dir_suffix(b'/', true, b"", 42),
            b"/nzb-42"
        );
        assert_eq!(
            dqh_nzbinfo_format_final_dir_suffix(b'/', false, b"MyRelease", 42),
            b"/MyRelease"
        );
        assert_eq!(
            dqh_article_dl_format_connection_name(b"News1", b"news.example.com"),
            b"News1 (news.example.com)"
        );
        assert_eq!(
            dqh_article_dl_format_fetch_command(false, b"<msg1@host>"),
            b"BODY <msg1@host>\r\n"
        );
        assert_eq!(
            dqh_article_dl_format_fetch_command(true, b"<msg1@host>"),
            b"ARTICLE <msg1@host>\r\n"
        );
        assert_eq!(dqh_article_dl_select_yenc_crc(true, 0x1111, 0x2222), 0x1111);
        assert_eq!(
            dqh_article_dl_select_yenc_crc(false, 0x1111, 0x2222),
            0x2222
        );
        assert_eq!(dqh_article_writer_open_verb(true), b"open\0");
        assert_eq!(dqh_article_writer_open_verb(false), b"create\0");
        assert_eq!(
            dqh_article_writer_format_info_path(b"MyNzb", b'/', b"part01.rar"),
            b"MyNzb/part01.rar"
        );
        assert_eq!(
            dqh_article_writer_parent_dir_len(b"Sub/file.bin", b"/dst/Sub/file.bin", b'/'),
            Some(8)
        );
        assert_eq!(
            dqh_article_writer_parent_dir_len(b"file.bin", b"/dst/file.bin", b'/'),
            None
        );
        assert_eq!(
            dqh_article_writer_apply_missing_part_counters(2, 10),
            (3, 9)
        );
        assert_eq!(
            dqh_article_writer_format_raw_part_dest(b"/dst/raw", b'/', 7),
            b"/dst/raw/007"
        );
        assert_eq!(dqh_article_writer_calc_total_failed_articles(3, 4), 7);
        assert_eq!(
            dqh_statmeter_calc_debug_speed_and_timediff(20480, 1000, 30, 30),
            (20, 100)
        );
        assert_eq!(dqh_statmeter_bytes_to_mb_i32(10 * 1024 * 1024), 10);
        assert_eq!(
            dqh_parchecker_format_cli_flags(4, 256),
            (b"-m256".to_vec(), b"-t4".to_vec(), b"-T4".to_vec())
        );
        assert_eq!(dqh_parchecker_creator_or_unknown(b""), b"<unknown program>");
        assert_eq!(dqh_parchecker_creator_or_unknown(b"par2cmdline"), b"par2cmdline");
        assert_eq!(
            dqh_parchecker_format_action_label(b"Verifying", b"MyNzb/vol"),
            b"Verifying MyNzb/vol"
        );
        assert_eq!(
            dqh_parchecker_effective_source_files_count(1, 2, 3, 12),
            12
        );
        assert_eq!(
            dqh_parchecker_effective_source_files_count(2, 2, 3, 12),
            0
        );
        assert!(dqh_parchecker_should_check_ignore_ext(1, 1, 5, 10, false));
        assert!(!dqh_parchecker_should_check_ignore_ext(1, 1, 10, 10, false));
        assert!(!dqh_parchecker_should_check_ignore_ext(1, 1, 5, 10, true));
        assert_eq!(dqh_parchecker_ignore_suffix(true), b", ignoring\0");
        assert_eq!(dqh_parchecker_ignore_suffix(false), b"\0");
        assert_eq!(dqh_parchecker_file_quality_label(1, 1), b"good\0");
        assert_eq!(dqh_parchecker_file_quality_label(2, 1), b"damaged\0");
        assert!(dqh_prepost_should_calc_health_for_par_index(0, 0, 4, 0, 0));
        assert!(dqh_prepost_should_calc_health_for_par_index(4, 0, 4, 0, 0));
        assert!(!dqh_prepost_should_calc_health_for_par_index(1, 0, 4, 0, 0));
        assert_eq!(
            dqh_prepost_skip_unpack_par_reason(3, 3),
            b"required par-repair\0"
        );
        assert_eq!(dqh_prepost_skip_unpack_par_reason(1, 3), b"par-failure\0");
        assert_eq!(
            dqh_stream_repair_write_errmsg_or_default(b""),
            b"incomplete write (disk full?)"
        );
        assert_eq!(
            dqh_stream_repair_write_errmsg_or_default(b"No space left"),
            b"No space left"
        );
        assert_eq!(
            dqh_stream_repair_format_stall_reason(60),
            b"nothing recovered for 60 seconds"
        );
        assert_eq!(
            dqh_stream_repair_format_conn_wait_reason(125),
            b"still waiting for news-server connections after 125 seconds"
        );
        assert_eq!(
            dqh_stream_repair_format_max_run_reason(180, 3),
            b"still running after 180 seconds, 3 times the timeout"
        );
        assert_eq!(
            dqh_stream_repair_format_slow_rate_reason(0.5, 100.0, 200.0, 4, 20.0),
            b"at 0.50 MB/s the 100.0 MB left would take 200 seconds, more than 4 times a download at 20.00 MB/s"
        );
        assert_eq!(
            dqh_stream_repair_format_donor_progress_label(
                b"Repairing from duplicate",
                b"MyDonor"
            ),
            b"Repairing from duplicate MyDonor"
        );
        assert_eq!(dqh_stream_repair_select_reject_log_kind(2, 3, 1, 2), 1);
        assert_eq!(dqh_stream_repair_select_reject_log_kind(4, 3, 1, 2), 2);
        assert_eq!(
            dqh_stream_repair_missing_par_disposition(true),
            b"left to par-repair\0"
        );
        assert_eq!(
            dqh_stream_repair_missing_par_disposition(false),
            b"no par2 files to repair it\0"
        );
        assert_eq!(
            dqh_stream_repair_format_decompress_temp_dir(b"/dst", b'/', 0),
            b"/dst/.stream-decompress.0"
        );
        assert_eq!(
            dqh_stream_repair_format_extract_dir(b"/dst/.stream-decompress.0", b'/'),
            b"/dst/.stream-decompress.0/extracted"
        );
        assert_eq!(
            dqh_stream_repair_decompress_fail_reason(true),
            b"extraction failed\0"
        );
        assert_eq!(
            dqh_stream_repair_decompress_fail_reason(false),
            b"no extraction tool configured\0"
        );
        assert_eq!(
            dqh_content_map_format_member_unreadable_reason(b"video.mkv"),
            b"member video.mkv unreadable"
        );
        assert_eq!(dqh_dupematcher_size_mismatch_suffix(true), b"\0");
        assert_eq!(
            dqh_dupematcher_size_mismatch_suffix(false),
            b", size mismatch\0"
        );
        assert_eq!(
            dqh_post_renamer_plan_skip_static_msg(false, 2),
            b"Skipping Post-download renaming: ambiguous multi-file collection detected\0"
        );
        assert_eq!(
            dqh_post_renamer_plan_skip_static_msg(true, 3),
            b"No qualifying media file found for Post-unpack renaming\0"
        );
        assert_eq!(
            dqh_post_renamer_format_final_status_msg(b"Post-download renaming for Nzb", 3),
            b"Post-download renaming for Nzb successful"
        );
        assert_eq!(
            dqh_post_renamer_format_final_status_msg(b"Post-unpack renaming for Nzb", 4),
            b"No files needed renaming for Post-unpack renaming for Nzb"
        );
        assert_eq!(dqh_rar_volume_set_data_offset(1000, 400), 600);
        assert_eq!(
            dqh_stream_crypto_format_rar3_round_bytes(0x030201),
            [0x01, 0x02, 0x03]
        );
        assert_eq!(dqh_stream_crypto_rar3_iv_index(0, 0x40000), 0);
        assert_eq!(dqh_stream_crypto_rar3_iv_index(0x4000, 0x40000), 1);
        assert_eq!(dqh_stream_crypto_rar3_iv_index(1, 0x40000), -1);
        assert_eq!(dqh_conn_calc_read_buffer_cap(1024, 128), 1153);
        assert_eq!(dqh_conn_format_port_str(6789), b"6789");
        assert_eq!(dqh_conn_clamp_buf_avail(-1), 0);
        assert_eq!(dqh_conn_clamp_buf_avail(42), 42);
        assert_eq!(
            dqh_conn_format_report_err_with_code(b"Connect failed", 111, b"Connection refused"),
            b"Connect failed: Error 111 - Connection refused"
        );
        assert_eq!(
            dqh_tls_format_report_err(b"TLS error", b"bad cert"),
            b"TLS error: bad cert"
        );
        assert_eq!(
            dqh_tls_format_cert_verify_err(b"news.example.com", b"certificate has expired"),
            b"TLS certificate verification failed for news.example.com: certificate has expired. For more info visit https://nzbget.com/documentation/certificate-verification/"
        );
        assert_eq!(
            dqh_tls_format_handshake_err(b"news.example.com"),
            b"TLS handshake failed for news.example.com"
        );
        assert_eq!(
            dqh_tls_format_hostname_mismatch_err(b"news.example.com", b"other.example.com"),
            b"TLS certificate verification failed for news.example.com: certificate hostname mismatch (other.example.com). For more info visit https://nzbget.com/documentation/certificate-verification/"
        );
        assert_eq!(
            dqh_tls_select_io_err_msg(true, true),
            b"Could not write to TLS-Socket: Connection closed by remote host\0"
        );
        assert_eq!(
            dqh_tls_select_io_err_msg(false, false),
            b"Could not read from TLS-Socket\0"
        );
        assert_eq!(
            dqh_maintenance_resolve_script_path(b"/opt/nzbget", b'/', b"install-update.sh"),
            b"/opt/nzbget/install-update.sh"
        );
        assert_eq!(
            dqh_maintenance_resolve_script_path(b"/opt/nzbget", b'/', b"/usr/bin/install.sh"),
            b"/usr/bin/install.sh"
        );
        assert_eq!(
            dqh_maintenance_format_package_info_path(b"/opt/nzbget/webui", b'/'),
            b"/opt/nzbget/webui/package-info.json"
        );
        assert_eq!(
            dqh_maintenance_format_script_info_name(false, b"update.sh"),
            b"update-script update.sh"
        );
        assert_eq!(
            dqh_maintenance_format_script_info_name(true, b"info.sh"),
            b"update-info-script info.sh"
        );
        assert_eq!(dqh_maintenance_branch_name(0), b"STABLE\0");
        assert_eq!(dqh_maintenance_branch_name(1), b"TESTING\0");
        assert_eq!(dqh_maintenance_branch_name(2), b"DEVEL\0");
        assert_eq!(dqh_maintenance_run_mode_name(true), b"DAEMON\0");
        assert_eq!(dqh_maintenance_run_mode_name(false), b"SERVER\0");
        assert_eq!(
            dqh_maintenance_format_cmdline_env_name(3),
            b"NZBUP_CMDLINE3"
        );
        assert_eq!(
            dqh_maintenance_strip_ext_and_calc_prefix_len(b"install-update.sh", 99),
            (b"install-update".to_vec(), 16)
        );
        assert_eq!(
            dqh_maintenance_strip_ext_and_calc_prefix_len(b".update", 99),
            (b"".to_vec(), 2)
        );
        assert_eq!(
            dqh_maintenance_strip_ext_and_calc_prefix_len(&vec![b'a'; 120], 99),
            (vec![b'a'; 99], 101)
        );
        assert_eq!(
            dqh_maintenance_format_sig_title(b"nzbget-bin.tar.gz"),
            b"\"RSA-SHA256(nzbget-bin.tar.gz)\" : \""
        );
        assert_eq!(dqh_scheduler_bytes_to_kb(20480), 20);
        assert_eq!(dqh_scheduler_pause_action_str(true), b"pausing\0");
        assert_eq!(dqh_scheduler_pause_action_str(false), b"unpausing\0");
        assert_eq!(dqh_scheduler_server_action_str(true), b"activating\0");
        assert_eq!(dqh_scheduler_server_action_str(false), b"deactivating\0");
        assert_eq!(dqh_direct_renamer_par_kind_label(true), b"par2-file\0");
        assert_eq!(
            dqh_direct_renamer_par_kind_label(false),
            b"non-par2-file\0"
        );
        assert_eq!(
            dqh_direct_renamer_resolve_old_output_path(
                b"/custom/out.rar",
                b"/dst",
                b'/',
                b"part01.rar"
            ),
            b"/custom/out.rar"
        );
        assert_eq!(
            dqh_direct_renamer_resolve_old_output_path(b"", b"/dst", b'/', b"part01.rar"),
            b"/dst/part01.rar"
        );
        assert_eq!(dqh_scanner_yes_no_str(true), b"yes\0");
        assert_eq!(dqh_scanner_yes_no_str(false), b"no\0");
        assert_eq!(
            dqh_scanner_format_temp_nzb_path(b"/tmp", b'/', 7),
            b"/tmp/nzb-7.tmp"
        );
        assert_eq!(
            dqh_url_coord_format_temp_url_path(b"/tmp", b'/', 42),
            b"/tmp/url-42.tmp"
        );
        assert_eq!(
            dqh_diskstate_format_new_path(b"/queue/ids"),
            b"/queue/ids.new"
        );
        assert_eq!(dqh_diskstate_format_ids_line(12, 345), b"nzb 12 file 345\n");
        assert_eq!(dqh_diskstate_legacy_suffix_str(1), b"s\0");
        assert_eq!(dqh_diskstate_legacy_suffix_str(2), b"c\0");
        assert_eq!(dqh_diskstate_legacy_suffix_str(0), b"\0");
        assert_eq!(
            dqh_diskstate_format_unreadable_path(b"/queue/queue", 1700000000),
            b"/queue/queue.unreadable-1700000000"
        );
        assert_eq!(
            dqh_diskstate_format_out_tmp_path(b"/dst/show", b'/', 99),
            b"/dst/show/99.out.tmp"
        );
        assert_eq!(
            dqh_diskstate_format_staged_part_path(b"/tmp", b'/', 99, 7),
            b"/tmp/99.007"
        );
        assert_eq!(dqh_diskstate_format_file_state_name(99, true), b"99c");
        assert_eq!(dqh_diskstate_format_file_state_name(99, false), b"99s");
        assert_eq!(
            dqh_diskstate_format_nzb_log_path(b"/queue", b'/', 12),
            b"/queue/n12.log"
        );
        assert_eq!(
            dqh_dupe_probe_format_stat_cmd(b"<abc@example.com>"),
            b"STAT <abc@example.com>\r\n"
        );
        assert_eq!(
            dqh_dupe_probe_recheck_retry_suffix(true),
            b"; retrying them\0"
        );
        assert_eq!(
            dqh_dupe_probe_recheck_retry_suffix(false),
            b"; not retrying\0"
        );
        assert_eq!(dqh_dupe_probe_found_on_prefix(true), b"\0");
        assert_eq!(dqh_dupe_probe_found_on_prefix(false), b", found on \0");
        assert_eq!(dqh_par_damage_calc_total_data_bytes(1000, 300), 700);
        assert_eq!(dqh_par_damage_calc_total_data_bytes(200, 300), 0);
        assert_eq!(dqh_twin_check_sample_verdict_label(2), b"alt\0");
        assert_eq!(dqh_twin_check_sample_verdict_label(1), b"twin\0");
        assert_eq!(dqh_twin_check_format_u32_str(4294967295), b"4294967295");
        assert_eq!(
            dqh_twin_check_lowercase_key(b"RagE.S01E02-GRP"),
            b"rage.s01e02-grp"
        );
        assert_eq!(
            dqh_unpack_ctrl_format_joining_progress(b"movie.mkv.001"),
            b"Joining from movie.mkv.001"
        );
        assert_eq!(
            dqh_unpack_ctrl_strip_last_extension(b"movie.mkv.001"),
            b"movie.mkv"
        );
        assert_eq!(
            dqh_stream_repair_format_materialize_progress(
                b"MyShow.copy2",
                2,
                5,
                10,
                40,
                2 * 1024 * 1024,
            ),
            b"Downloading duplicate MyShow.copy2 (volume 2/5, article 10/40, 2.0 MB)"
        );
        assert_eq!(
            dqh_article_fetcher_format_body_cmd(b"<part1@example.com>"),
            b"BODY <part1@example.com>\r\n"
        );
        assert_eq!(dqh_webserver_format_forwarded_for_suffix(b""), b"");
        assert_eq!(
            dqh_webserver_format_forwarded_for_suffix(b"10.0.0.1"),
            b" (forwarded for: 10.0.0.1)"
        );
        assert_eq!(
            dqh_cmd_script_format_info_name(b"EMail.py", b"Test", false),
            b"script EMail.py with command Test"
        );
        assert_eq!(
            dqh_cmd_script_format_info_name(b"EMail.py", b"Test", true),
            b"Script EMail.py with command Test"
        );
        assert_eq!(
            dqh_script_config_format_tls_temp_name(b"nzbget.conf", 1234, 7),
            b".nzbget.conf.tls-1234-7"
        );
        assert_eq!(
            dqh_newznab_format_search_info_name(b"Indexer1 q=Show", 2),
            b"DupeSearch Indexer1 q=Show page 2"
        );
        assert_eq!(
            dqh_dupesearch_format_key_val_param(b"DupeAlive", 98),
            b"DupeAlive=98"
        );
        assert_eq!(
            dqh_dupesearch_format_pending_dir(b"/queue", b'/', 0),
            b"/queue/dupesearch-pending"
        );
        assert_eq!(
            dqh_dupesearch_format_pending_dir(b"/queue", b'/', 42),
            b"/queue/dupesearch-pending/42"
        );
        assert_eq!(
            dqh_dupesearch_format_indexed_base_path(b"/queue/pending", b'/', 7),
            b"/queue/pending/7"
        );
        assert_eq!(dqh_dupesearch_format_usize_str(19), b"19");
        assert_eq!(dqh_nzb_fetcher_format_http_status_err(429), b"HTTP 429");
        assert_eq!(
            dqh_http_get_format_temp_path(b"/tmp", b'/', 1700000000, 3),
            b"/tmp/dupesearch-1700000000-3.tmp"
        );
        assert_eq!(dqh_fleet_calc_check_lock_wait_ms(10000, 3000, 2000), 4000);
        assert_eq!(dqh_fleet_calc_check_lock_wait_ms(4000, 3000, 2000), 0);
        assert_eq!(
            dqh_script_ctrl_format_env_assignment(b"NZBOP_TEMPDIR", b"/tmp"),
            b"NZBOP_TEMPDIR=/tmp"
        );
        assert_eq!(
            dqh_script_ctrl_format_prefixed_varname(b"NZBPO", b"Host"),
            b"NZBPO_Host"
        );
        assert_eq!(
            dqh_log_format_debug_line(b"hello", b"Log.cpp", 42, Some(b"RotateLog")),
            b"hello (Log.cpp:42:RotateLog)"
        );
        assert_eq!(
            dqh_log_format_debug_line(b"hello", b"Log.cpp", 42, None),
            b"hello (Log.cpp:42)"
        );
        assert_eq!(
            dqh_log_format_rotate_mask(b"nzbget", b".log"),
            b"nzbget-####-##-##.log"
        );
        assert_eq!(
            dqh_log_format_deleting_old_msg(b"nzbget-2026-10-01.log"),
            b"Deleting old log-file nzbget-2026-10-01.log\n"
        );
        assert_eq!(
            dqh_log_format_rotated_path(b"/var/log", b'/', b"nzbget", 2026, 11, 5, b".log"),
            b"/var/log/nzbget-2026-11-05.log"
        );
        assert_eq!(
            dqh_options_format_indexed_opt_name(b"Server", 3, b"Active"),
            b"Server3.Active"
        );
        assert_eq!(dqh_nzbget_format_pid_line(4321), b"4321\n");
        assert_eq!(
            dqh_ncurses_format_plot_line(b"abc", 6, 0, 500),
            b"abc   "
        );
        assert_eq!(
            dqh_ncurses_format_plot_line(b"abcdef", 6, 2, 500),
            b"abcd"
        );
        assert_eq!(
            dqh_fs_format_expanded_rel_path(b"/work/dir", b"sub/file.nzb"),
            b"/work/dir/sub/file.nzb"
        );
        assert_eq!(
            dqh_benchmark_format_unique_filename(123456789),
            b"nzbget_disktest_123456789.bin"
        );
        assert_eq!(dqh_benchmark_resolve_block_size(0, 4096), 4096);
        assert_eq!(dqh_benchmark_resolve_block_size(8192, 4096), 8192);
        assert_eq!(
            dqh_statmeter_format_indexed_bytes_line(10, &[100, 250]),
            b"[10]=100 [11]=250 "
        );
        assert_eq!(
            dqh_nntpserver_format_cache_file_name(2, 65536, 32768),
            b"2=65536-32768"
        );
        assert_eq!(
            dqh_nzbgen_format_nzb_output_path(b"/data", b'/', b"release"),
            b"/data/release.nzb"
        );
        assert_eq!(
            dqh_nserv_format_logfile_opt(b"/var/log/nserv.log"),
            b"LogFile=/var/log/nserv.log"
        );
        assert_eq!(
            dqh_nzbfile_format_missing_attrs_msg(b"file"),
            b"Malformed nzb-file, tag <file> must have attributes"
        );
        assert_eq!(dqh_nzbfile_classify_start_element(b"file"), 1);
        assert_eq!(dqh_nzbfile_classify_start_element(b"segment"), 2);
        assert_eq!(dqh_nzbfile_classify_start_element(b"meta"), 3);
        assert_eq!(dqh_nzbfile_classify_start_element(b"nzb"), 0);
        assert_eq!(dqh_nzbfile_classify_file_attr(b"subject"), 1);
        assert_eq!(dqh_nzbfile_classify_file_attr(b"date"), 2);
        assert_eq!(dqh_nzbfile_classify_file_attr(b"poster"), 0);
        assert_eq!(dqh_nzbfile_classify_segment_attr(b"bytes"), 1);
        assert_eq!(dqh_nzbfile_classify_segment_attr(b"number"), 2);
        assert_eq!(dqh_nzbfile_classify_segment_attr(b"id"), 0);
        assert_eq!(dqh_nzbfile_classify_end_element(b"file"), 1);
        assert_eq!(dqh_nzbfile_classify_end_element(b"group"), 2);
        assert_eq!(dqh_nzbfile_classify_end_element(b"segment"), 3);
        assert_eq!(dqh_nzbfile_classify_end_element(b"meta"), 4);
        assert_eq!(dqh_nzbfile_classify_end_element(b"nzb"), 0);
        assert_eq!(dqh_sysinfo_format_rapidyenc_version(0x010203), b"1.2.3");
        assert_eq!(
            dqh_fs_format_null_delete_errmsg(b"Bad address"),
            b"could not delete (null): Bad address"
        );
        assert_eq!(
            dqh_fs_extract_parent_dir_path(b"/var/queue/file.tmp", b'/'),
            b"/var/queue"
        );
        assert_eq!(dqh_fs_extract_parent_dir_path(b"file.tmp", b'/'), b"file.tmp");
        assert_eq!(dqh_feed_file_classify_start_element(b"item"), 1);
        assert_eq!(dqh_feed_file_classify_start_element(b"enclosure"), 2);
        assert_eq!(dqh_feed_file_classify_start_element(b"channel"), 0);
        assert!(dqh_url_dl_has_dnzb_prefix(b"X-DNZB-Category: Movies"));
        assert!(!dqh_url_dl_has_dnzb_prefix(b"Content-Type: text/xml"));
        assert!(dqh_diskstate_is_candidate_queue_file_name(b"123"));
        assert!(dqh_diskstate_is_candidate_queue_file_name(b"123s"));
        assert!(dqh_diskstate_is_candidate_queue_file_name(b"123c"));
        assert!(!dqh_diskstate_is_candidate_queue_file_name(b"s"));
        assert!(!dqh_diskstate_is_candidate_queue_file_name(b"123x"));
        assert_eq!(dqh_diskstate_find_char_pos(b"key=val", b'='), Some(3));
        assert_eq!(dqh_diskstate_find_char_pos(b"keyval", b'='), None);
        assert!(dqh_tls_is_tls13_version_str(b"TLSv1.3"));
        assert!(!dqh_tls_is_tls13_version_str(b"TLSv1.2"));
        assert!(dqh_tls_cipher_has_ccm8(b"ECDHE-RSA-AES128-CCM8"));
        assert!(!dqh_tls_cipher_has_ccm8(b"TLS_AES_256_GCM_SHA384"));
        assert_eq!(dqh_tls_resolve_cipher_bits(b"ECDHE-RSA-AES128-CCM8", 128), 64);
        assert_eq!(dqh_tls_resolve_cipher_bits(b"TLS_AES_256_GCM_SHA384", 256), 256);
        assert_eq!(
            dqh_tls_format_bulk_cipher_name(Some(b"AES-128-CCM"), b"ECDHE-RSA-AES128-CCM8"),
            b"AES-128-CCM@8"
        );
        assert_eq!(
            dqh_tls_format_bulk_cipher_name(None, b"TLS_AES_256_GCM_SHA384"),
            b"unavailable"
        );
        assert_eq!(dqh_tls_compute_suite_flags(true, true, false, false), 7);
        assert_eq!(dqh_tls_compute_suite_flags(false, false, true, false), 2);
        assert!(dqh_nntp_conn_is_already_in_group(b"alt.binaries.test", b"alt.binaries.test"));
        assert!(!dqh_nntp_conn_is_already_in_group(b"", b""));
        assert!(dqh_options_should_expand_home_value(Some(b"~/downloads"), false));
        assert!(!dqh_options_should_expand_home_value(Some(b"~/downloads"), true));
        assert!(!dqh_options_should_expand_home_value(Some(b"/downloads"), false));
        assert!(!dqh_options_should_expand_home_value(None, false));
        assert!(dqh_nzbget_is_backward_compat_root_user(b"root"));
        assert!(!dqh_nzbget_is_backward_compat_root_user(b"daemon"));
        assert!(dqh_xmlrpc_is_multicall_post(b"System.MultiCall", true, true));
        assert!(!dqh_xmlrpc_is_multicall_post(b"System.MultiCall", false, true));
        assert!(dqh_xmlrpc_is_tls_cipher_baseline_entry(b"_TlsCipherBaseline"));
        assert!(!dqh_xmlrpc_is_tls_cipher_baseline_entry(b"Server1.Cipher"));
        assert!(dqh_webserver_is_html_content_type(Some(b"text/html")));
        assert!(!dqh_webserver_is_html_content_type(Some(b"text/plain")));
        assert!(!dqh_webserver_is_html_content_type(None));
        assert_eq!(dqh_scanner_find_last_char_pos(b"movie.nzb.queued", b'.'), Some(9));
        assert_eq!(dqh_scanner_find_last_char_pos(b"movienzb", b'.'), None);
        assert_eq!(
            dqh_log_split_basename_and_ext(b"nzbget.log"),
            (b"nzbget".to_vec(), b".log".to_vec())
        );
        assert_eq!(
            dqh_log_split_basename_and_ext(b".hidden"),
            (b".hidden".to_vec(), Vec::new())
        );
        assert!(dqh_sysinfo_line_contains_marker(b"UNRAR 7.00 freeware", b"UNRAR"));
        assert!(!dqh_sysinfo_line_contains_marker(b"7-Zip 24.01", b"UNRAR"));
        assert!(dqh_bytes_equal(b"abc", b"abc"));
        assert!(!dqh_bytes_equal(b"abc", b"ABC"));
        assert!(dqh_bytes_eq_ignore_case(b"Server1.Host", b"server1.host"));
        assert!(!dqh_bytes_eq_ignore_case(b"Server1.Host", b"server2.host"));
        assert_eq!(dqh_newznab_classify_start_element(1, false, b"error"), 1);
        assert_eq!(dqh_newznab_classify_start_element(2, false, b"item"), 2);
        assert_eq!(dqh_newznab_classify_start_element(3, true, b"attr"), 3);
        assert_eq!(dqh_newznab_classify_start_element(3, false, b"attr"), 0);
        assert_eq!(dqh_newznab_classify_item_attr_key(b"size"), 1);
        assert_eq!(dqh_newznab_classify_item_attr_key(b"grabs"), 2);
        assert_eq!(dqh_newznab_classify_item_attr_key(b"usenetdate"), 3);
        assert_eq!(dqh_newznab_classify_item_attr_key(b"hydraIndexerName"), 4);
        assert_eq!(dqh_newznab_classify_item_attr_key(b"other"), 0);
        assert_eq!(dqh_newznab_classify_end_element(true, b"title"), 1);
        assert_eq!(dqh_newznab_classify_end_element(true, b"link"), 2);
        assert_eq!(dqh_newznab_classify_end_element(true, b"size"), 3);
        assert_eq!(dqh_newznab_classify_end_element(true, b"item"), 4);
        assert_eq!(dqh_newznab_classify_end_element(false, b"title"), 0);
        assert_eq!(dqh_nzb_reader_classify_start_element(b"meta", false), 1);
        assert_eq!(dqh_nzb_reader_classify_start_element(b"file", false), 2);
        assert_eq!(dqh_nzb_reader_classify_start_element(b"segment", true), 3);
        assert_eq!(dqh_nzb_reader_classify_start_element(b"segment", false), 0);
        assert_eq!(dqh_nzb_reader_classify_end_element(b"meta", true, false, false), 1);
        assert_eq!(dqh_nzb_reader_classify_end_element(b"segment", false, true, true), 2);
        assert_eq!(dqh_nzb_reader_classify_end_element(b"group", false, false, true), 3);
        assert_eq!(dqh_nzb_reader_classify_end_element(b"file", false, false, true), 4);
        assert_eq!(dqh_bytes_cmp(b"abc", b"abd"), -1);
        assert_eq!(dqh_bytes_cmp(b"abc", b"abc"), 0);
        assert_eq!(dqh_bytes_cmp(b"abd", b"abc"), 1);
        assert_eq!(
            dqh_dupe_fallback_classify_content_name_match(b"Movie.rar", b"Movie.rar"),
            1
        );
        assert_eq!(
            dqh_dupe_fallback_classify_content_name_match(b"Movie.rar", b"movie.rar"),
            2
        );
        assert_eq!(
            dqh_dupe_fallback_classify_content_name_match(b"Movie.rar", b"other.rar"),
            0
        );
        assert_eq!(
            dqh_tls_resolve_probe_result(
                true,
                b"TLS_AES_256_GCM_SHA384",
                b"TLS_AES_256_GCM_SHA384",
                0x0304,
                13,
                0x0304,
                0x0303,
                false,
                -2
            ),
            1
        );
        assert_eq!(
            dqh_tls_resolve_probe_result(
                false,
                b"",
                b"TLS_AES_256_GCM_SHA384",
                0,
                13,
                0x0304,
                0x0303,
                true,
                -2
            ),
            -5
        );
        assert_eq!(
            dqh_maintenance_match_sig_line_prefix(b"SHA256 (pkg.run) = deadbeef", b"SHA256 (pkg.run) = "),
            Some(19)
        );
        assert_eq!(
            dqh_maintenance_match_sig_line_prefix(b"SHA256 (other.run) = deadbeef", b"SHA256 (pkg.run) = "),
            None
        );
        assert_eq!(dqh_parse_c_atoll(b"  -9876543210xyz"), -9876543210);
        assert_eq!(dqh_parse_c_atoll(b"abc"), 0);
        assert_eq!(dqh_parse_c_atoi(b" 42 "), 42);
        assert_eq!(dqh_parse_c_atoi_or_default(Some(b"6791"), 119), 6791);
        assert_eq!(dqh_parse_c_atoi_or_default(None, 119), 119);
        assert_eq!(
            dqh_diskstate_parse_queue_file_id_and_suffix(b"123s"),
            Some((123, b's'))
        );
        assert_eq!(
            dqh_diskstate_parse_queue_file_id_and_suffix(b"123"),
            None
        );
        assert_eq!(
            dqh_diskstate_parse_queue_file_id_and_suffix(b"acache"),
            None
        );
        assert_eq!(dqh_nstring_find_substr_from(b"hello world", b"world", 0), 6);
        assert_eq!(dqh_nstring_find_substr_from(b"hello world", b"o", 5), 7);
        assert_eq!(dqh_nstring_find_substr_from(b"", b"", 0), 0);
        assert_eq!(dqh_nstring_find_substr_from(b"hello", b"a", 5), -1);
        assert_eq!(
            dqh_log_parse_rotated_date_fields(b"nzbget-2026-11-05.log", 7, 12, 15),
            (126, 10, 5)
        );
        assert!(dqh_mem_slices_equal(b"abcdef", b"abcdef"));
        assert!(!dqh_mem_slices_equal(b"abcdef", b"abcdeg"));
        assert!(dqh_tls_is_cipher_list(b"TLS_AES_128_GCM_SHA256:TLS_AES_256_GCM_SHA384"));
        assert!(!dqh_tls_is_cipher_list(b"TLS_AES_256_GCM_SHA384"));
        assert_eq!(
            dqh_sysinfo_extract_version_after_space(b"OpenSSL 3.0.12 24 Oct 2023"),
            b"3.0.12 24 Oct 2023"
        );
        assert_eq!(
            dqh_os_classify_os_release_line(b"NAME=\"Debian\"", false, false),
            (1, 5)
        );
        assert_eq!(
            dqh_os_classify_os_release_line(b"VERSION_ID=\"12\"", true, false),
            (2, 11)
        );
        assert_eq!(
            dqh_os_classify_os_release_line(b"BUILD_ID=rolling", true, false),
            (3, 9)
        );
        assert_eq!(
            dqh_dupe_util_trim_bounds(b"  \t hello \r\n ", b" \t\r\n"),
            Some((4, 5))
        );
        assert_eq!(dqh_dupe_util_trim_bounds(b" \t\r\n", b" \t\r\n"), None);
        assert_eq!(dqh_dupe_util_lower(b"HeLLo"), b"hello");
        assert_eq!(
            dqh_nzb_fetcher_format_parse_err(b"<error apikey=secret123 />"),
            b"not an nzb-file: <error apikey=*** />"
        );
        assert_eq!(dqh_util_classify_executor_ext(b"/path/script.py"), (1, 12));
        assert_eq!(dqh_util_classify_executor_ext(b"/path/script.sh"), (2, 12));
        assert_eq!(dqh_util_classify_executor_ext(b"/path/script.js"), (3, 12));
        assert_eq!(dqh_util_classify_executor_ext(b"/path/script.rb"), (4, 12));
        assert_eq!(dqh_util_classify_executor_ext(b"/path/script"), (0, 0));
        assert!(dqh_bytes_lt_ignore_case(b"Alpha", b"beta"));
        assert!(!dqh_bytes_lt_ignore_case(b"beta", b"Alpha"));
        assert!(!dqh_bytes_lt_ignore_case(b"Alpha", b"alpha"));
        assert!(dqh_bytes_lt_ignore_case(b"Alpha", b"alpha2"));
        assert!(dqh_options_should_check_single_script_dir(false, b"/scripts"));
        assert!(!dqh_options_should_check_single_script_dir(false, b"/a;/b"));
        assert!(dqh_options_should_check_single_script_dir(true, b"/a;/b"));
        assert_eq!(
            dqh_webserver_encode_hex_token(&[0x00, 0xab, 0xff]),
            b"00abff"
        );
        assert_eq!(
            dqh_script_config_format_option_line(b"Server1.Port", b"563"),
            b"Server1.Port=563\n"
        );
        assert_eq!(
            dqh_script_config_ensure_trailing_lf(b"# comment"),
            b"# comment\n"
        );
        assert_eq!(
            dqh_script_config_ensure_trailing_lf(b"# comment\n"),
            b"# comment\n"
        );
        assert_eq!(
            dqh_yenc_format_begin_headers(1, 50000, b"test.dat", 0, 1000),
            b"=ybegin part=1 line=128 size=50000 name=test.dat\r\n=ypart begin=1 end=1000\r\n"
        );
        assert_eq!(
            dqh_yenc_format_end_trailer(1000, 0x0a1b2c3d),
            b"=yend size=1000 part=0 pcrc32=0a1b2c3d\r\n"
        );
        {
            let mut lnsz = 0;
            let mut out = vec![0u8; 256];
            let mut out_len = 0usize;
            let (consumed, flush) =
                dqh_yenc_encode_chunk_step(&[4, b'A', 214], true, &mut lnsz, &mut out, &mut out_len, 256);
            assert_eq!(consumed, 3);
            assert!(!flush);
            assert_eq!(lnsz, 0);
            assert_eq!(&out[..out_len], b"..k=@\r\n");
        }
        assert_eq!(
            dqh_nntpserver_format_reply_line(0, b"alt.binaries.test"),
            b"211 0 0 0 alt.binaries.test\r\n"
        );
        assert_eq!(
            dqh_nntpserver_format_reply_line(6, b"file.rar"),
            b"Subject: \"file.rar\"\r\n"
        );
        assert_eq!(
            dqh_unpack_format_7z_output_arg(b"/dst/unpack"),
            b"-o/dst/unpack"
        );
        assert_eq!(
            dqh_script_format_prefixed_message(b"7-Zip", b"Everything is Ok"),
            b"7-Zip: Everything is Ok"
        );
        assert_eq!(
            dqh_xmlrpc_concat_decoded_path_and_query(b"/jsonrpc", b"/jsonrpc?a=1", true, 8),
            b"/jsonrpc?a=1"
        );
        assert_eq!(
            dqh_diskstate_next_comma_token(b",,a.rar,b.rar,", 0),
            Some((2, 5, 8))
        );
        assert_eq!(
            dqh_diskstate_next_comma_token(b",,a.rar,b.rar,", 8),
            Some((8, 5, 14))
        );
        assert_eq!(dqh_diskstate_next_comma_token(b",,a.rar,b.rar,", 14), None);
        assert_eq!(
            dqh_tokenizer_find_next_token(b" ,;  foo \t ; bar ", b",;"),
            Some((5, 8, 12))
        );
        {
            let mut visited = Vec::new();
            dqh_queue_editor_visit_ext_diff_lines(b"*Script1:\tyes\n*Script2:\tno\n", |k, v| {
                visited.push((k.to_vec(), v.to_vec()));
            });
            assert_eq!(
                visited,
                vec![
                    (b"*Script1:".to_vec(), b"yes".to_vec()),
                    (b"*Script2:".to_vec(), b"no".to_vec()),
                ]
            );
        }
        assert_eq!(dqh_queue_script_event_name(0), b"FILE_DOWNLOADED\0");
        assert_eq!(dqh_queue_script_event_name(6), b"NZB_DELETED\0");
        assert_eq!(dqh_queue_script_delete_status_name(3), b"DUPE\0");
        assert_eq!(dqh_queue_script_url_status_name(5), b"SCAN_SKIPPED\0");
        assert_eq!(dqh_queue_script_mark_status_name(2), b"GOOD\0");
        assert_eq!(
            dqh_stream_repair_calc_encrypted_window_bounds(10, 50, 48, 16, 16),
            Some((48, 16, 48))
        );
        assert_eq!(
            dqh_stream_repair_calc_encrypted_window_bounds(10, 50, 16, 16, 16),
            None
        );
        assert_eq!(
            dqh_stream_repair_calc_encrypted_chunk_end(0, 64, 32, 16),
            (32, 2)
        );
        assert!(dqh_stream_repair_is_target_unfinished(0, true));
        assert!(dqh_stream_repair_is_target_unfinished(100, false));
        assert!(!dqh_stream_repair_is_target_unfinished(100, true));
        assert!(dqh_nzbfile_should_trim_sax_chars(b"meta", true));
        assert!(!dqh_nzbfile_should_trim_sax_chars(b"meta", false));
        assert!(!dqh_nzbfile_should_trim_sax_chars(b"file", true));
        assert_eq!(dqh_repair_ctrl_map_par_stage_to_post_stage(2, 10, 11, 12, 13), 12);
        assert_eq!(dqh_parchecker_cmdline_err_str(0), b"OK\0");
        assert_eq!(dqh_parchecker_cmdline_err_str(8), b"out of memory\0");
        assert_eq!(dqh_parchecker_stage_message(2), b"Repairing file\0");
        assert_eq!(dqh_post_script_status_name(2), b"SUCCESS\0");
        assert_eq!(dqh_post_script_deprecated_status_code(1), 0);
        assert_eq!(dqh_post_script_deprecated_status_code(5), 4);
        assert_eq!(dqh_scheduler_command_name(11), b"Fetch Feed\0");
        assert_eq!(
            dqh_ext_mgr_format_update_backup_path(b"/scripts/ext"),
            b"/scripts/ext.update-backup"
        );
        assert_eq!(
            dqh_ext_mgr_format_lookup_error(b"MyExt", 0),
            b"Update failed: Extension 'MyExt' not found"
        );
        assert_eq!(
            dqh_ext_mgr_format_install_or_delete_error(b"/tmp/a.zip", b"perm denied", 2),
            b"Extension unpacked, but failed to delete temporary archive '/tmp/a.zip': perm denied"
        );
        assert_eq!(
            dqh_ext_mgr_format_compound_update_error(b"err1", b"/a", b"/b", b"err2", 1),
            b"err1; failed to restore /a from /b: err2"
        );
        assert_eq!(
            dqh_options_merge_script_list(b"ScriptA", b"scripta, ScriptB ; ScriptC"),
            b"ScriptA,ScriptB,ScriptC"
        );
        assert_eq!(
            dqh_frontend_resolve_control_ip(b"0.0.0.0"),
            Some(&b"127.0.0.1\0"[..])
        );
        assert_eq!(dqh_frontend_resolve_control_ip(b"::"), None);
        assert_eq!(dqh_frontend_resolve_control_ip(b"192.168.1.10"), None);
        assert_eq!(dqh_ncurses_message_type_label(0, true), b"INFO    \0");
        assert_eq!(dqh_ncurses_message_type_label(2, true), b"ERROR   \0");
        assert_eq!(dqh_ncurses_message_type_label(2, false), b"        \0");
        assert_eq!(dqh_ncurses_message_color_pair(0), 2);
        assert_eq!(dqh_ncurses_message_color_pair(4), 6);
        assert_eq!(dqh_xmlrpc_response_content_type(true), b"text/xml\0");
        assert_eq!(
            dqh_xmlrpc_response_content_type(false),
            b"application/json\0"
        );
        assert_eq!(
            dqh_xmlrpc_test_server_speed_err_msg(1),
            b"Server not found\0"
        );
        assert_eq!(
            dqh_xmlrpc_test_server_speed_err_msg(2),
            b"Server is not active\0"
        );
        assert_eq!(
            dqh_xmlrpc_test_server_speed_err_msg(3),
            b"Server has no connections configured\0"
        );
        assert_eq!(
            dqh_binrpc_write_log_response_msg(true),
            b"Message added to log\0"
        );
        assert_eq!(
            dqh_binrpc_write_log_response_msg(false),
            b"Invalid message-kind\0"
        );
        assert_eq!(
            dqh_binrpc_scan_response_msg(true),
            b"Scan-Command completed\0"
        );
        assert_eq!(
            dqh_binrpc_scan_response_msg(false),
            b"Scan-Command scheduled successfully\0"
        );
        {
            let (crc1, first1) = dqh_article_writer_step_file_crc(true, 0, 0x12345678, 100);
            assert_eq!(crc1, 0x12345678);
            assert!(!first1);
            let (crc2, first2) = dqh_article_writer_step_file_crc(first1, crc1, 0x9abcdef0, 200);
            assert_eq!(
                crc2,
                crate::crc::combine(0x12345678, 0x9abcdef0, 200)
            );
            assert!(!first2);
        }
        assert_eq!(dqh_rename_ctrl_classify_run_outcome(true, 5), 1);
        assert_eq!(dqh_rename_ctrl_classify_run_outcome(false, 5), 2);
        assert_eq!(dqh_rename_ctrl_classify_run_outcome(false, 0), 3);
        {
            let mut s = *b"move for test";
            dqh_post_capitalize_first_ascii(&mut s);
            assert_eq!(&s, b"Move for test");
        }
        assert_eq!(
            dqh_dupesearch_prepare_status_suffix(true, false),
            b": ranking new backups\0"
        );
        assert_eq!(
            dqh_dupesearch_prepare_status_suffix(false, true),
            b": it failed before its search was due\0"
        );
        assert_eq!(dqh_dupesearch_prepare_status_suffix(false, false), b"\0");
        {
            let mut qs = Vec::new();
            dqh_dupesearch_visit_expand_queries(b"Show.Name.S01E02.1080p.WEB-DL", |q| {
                qs.push(q.to_vec())
            });
            assert!(!qs.is_empty());
        }
        assert_eq!(
            dqh_newznab_format_query_label(&[b"t", b"q"], &[b"search", b"movie"]),
            b"t=search q=movie"
        );
        assert_eq!(
            dqh_nntp_health_format_bracketed_id(b"abc@host"),
            b"<abc@host>"
        );
        assert_eq!(
            dqh_nntp_health_format_bracketed_id(b"<abc@host>"),
            b"<abc@host>"
        );
        assert_eq!(
            dqh_nntp_health_format_stat_batch(&[b"id1@h", b"<id2@h>"]),
            b"STAT <id1@h>\r\nSTAT <id2@h>\r\n"
        );
        assert_eq!(
            dqh_queue_coord_stream_repair_queue_msg_fmt(true),
            b"Queueing stream repair of %s (no article available) from duplicate collections\0"
        );
        assert_eq!(
            dqh_queue_coord_stream_repair_queue_msg_fmt(false),
            b"Queueing stream repair of %s from duplicate collections\0"
        );
        assert_eq!(
            dqh_queue_coord_select_health_delete_action(2, 2, 10, 20),
            10
        );
        assert_eq!(
            dqh_queue_coord_select_health_delete_action(1, 2, 10, 20),
            20
        );
        assert_eq!(dqh_queue_coord_max_i64_slice(&[-5, 100, 42, 250]), 250);
        assert_eq!(dqh_queue_coord_max_i64_slice(&[]), 0);
        assert_eq!(
            dqh_queue_coord_projected_health_rel_str(true),
            b"under half the\0"
        );
        assert_eq!(dqh_queue_coord_projected_health_rel_str(false), b"below\0");
        assert_eq!(dqh_queue_coord_partly_alive_rel_str(true), b"below\0");
        assert_eq!(
            dqh_queue_coord_partly_alive_rel_str(false),
            b"too close to\0"
        );
        assert_eq!(dqh_nzb_param_decide_set_action(true, true), 1);
        assert_eq!(dqh_nzb_param_decide_set_action(false, true), 2);
        assert_eq!(dqh_nzb_param_decide_set_action(false, false), 3);
        assert_eq!(dqh_nzb_param_decide_set_action(true, false), 0);
        assert!(dqh_dupecoord_is_valid_disk_file(false, false, 1024));
        assert!(!dqh_dupecoord_is_valid_disk_file(true, false, 1024));
        assert!(!dqh_dupecoord_is_valid_disk_file(false, true, 1024));
        assert!(!dqh_dupecoord_is_valid_disk_file(false, false, 0));
        assert_eq!(
            dqh_dupecoord_select_other_dupe_delete_action(true, 11, 22),
            11
        );
        assert_eq!(
            dqh_dupecoord_select_other_dupe_delete_action(false, 11, 22),
            22
        );
        assert_eq!(dqh_queue_editor_delete_entry_msg_kind(true, 4, 0), 4);
        assert_eq!(dqh_queue_editor_delete_entry_msg_kind(false, 4, 0), 0);
        assert_eq!(dqh_scanner_missed_articles_msg_kind(true, 1, 4), 1);
        assert_eq!(dqh_scanner_missed_articles_msg_kind(false, 1, 4), 4);
        assert_eq!(
            dqh_article_writer_check_existing_output_file(false, 0, 1000),
            0
        );
        assert_eq!(
            dqh_article_writer_check_existing_output_file(true, 1000, 1000),
            1
        );
        assert_eq!(
            dqh_article_writer_check_existing_output_file(true, 500, 1000),
            2
        );
        assert_eq!(
            dqh_article_writer_select_direct_output_mode(false, false, true, true, true),
            0
        );
        assert_eq!(
            dqh_article_writer_select_direct_output_mode(true, false, true, false, false),
            1
        );
        assert_eq!(
            dqh_article_writer_select_direct_output_mode(false, true, false, true, true),
            2
        );
        assert_eq!(
            dqh_article_writer_select_direct_output_mode(true, false, false, true, true),
            3
        );
        assert!(dqh_article_writer_should_fallback_open_to_final_path(
            false, true
        ));
        assert!(!dqh_article_writer_should_fallback_open_to_final_path(
            true, true
        ));
        assert!(dqh_article_writer_validate_staged_dupe_size(768000, 768000));
        assert!(!dqh_article_writer_validate_staged_dupe_size(768001, 768000));
        assert_eq!(
            dqh_article_writer_eval_commit_post_close(false, true, false),
            -1
        );
        assert_eq!(
            dqh_article_writer_eval_commit_post_close(true, true, false),
            1
        );
        assert_eq!(
            dqh_article_writer_eval_commit_post_close(true, true, true),
            0
        );
        assert!(dqh_article_writer_is_staged_dupe_article_file(1, true));
        assert!(!dqh_article_writer_is_staged_dupe_article_file(0, true));
        assert!(!dqh_article_writer_is_staged_dupe_article_file(1, false));
        assert_eq!(
            dqh_article_writer_classify_direct_final_cleanup(false, false, true),
            1
        );
        assert_eq!(
            dqh_article_writer_classify_direct_final_cleanup(false, true, true),
            0
        );
        assert_eq!(
            dqh_article_writer_classify_direct_final_cleanup(true, false, true),
            2
        );
        assert_eq!(
            dqh_article_writer_classify_direct_final_cleanup(true, false, false),
            0
        );
        assert!(dqh_article_cache_should_write_cache_flag(
            true, true, true, false
        ));
        assert!(!dqh_article_cache_should_write_cache_flag(
            true, true, true, true
        ));
        assert_eq!(dqh_article_cache_post_flush_action(true, 0, true, false), 1);
        assert_eq!(
            dqh_article_cache_post_flush_action(false, 100, false, false),
            2
        );
        assert_eq!(
            dqh_article_cache_post_flush_action(false, 100, true, false),
            0
        );
        assert!(dqh_article_fetcher_should_clear_unreachable(true, true));
        assert!(dqh_article_fetcher_should_clear_unreachable(false, false));
        assert!(!dqh_article_fetcher_should_clear_unreachable(false, true));
        assert!(dqh_article_fetcher_can_attempt_transient_retry(true, false));
        assert!(!dqh_article_fetcher_can_attempt_transient_retry(true, true));
        assert!(dqh_article_fetcher_has_more_transient_attempts(1, 3));
        assert!(!dqh_article_fetcher_has_more_transient_attempts(3, 3));
        assert!(dqh_article_batch_can_store_result(5, 5, false));
        assert!(!dqh_article_batch_can_store_result(4, 5, false));
        assert!(!dqh_article_batch_can_store_result(5, 5, true));
        assert!(dqh_article_batch_can_deliver_next(false, false, 2, 5));
        assert!(!dqh_article_batch_can_deliver_next(false, false, 5, 5));
        assert_eq!(dqh_webserver_classify_pre_parse_method(1, 1, 2, 0), 1);
        assert_eq!(dqh_webserver_classify_pre_parse_method(2, 1, 2, 0), 2);
        assert_eq!(dqh_webserver_classify_pre_parse_method(0, 1, 2, 0), 0);
        assert_eq!(
            dqh_binrpc_classify_history_entry_fields(1, 1, 2, 3, false),
            1
        );
        assert_eq!(
            dqh_binrpc_classify_history_entry_fields(2, 1, 2, 3, true),
            2
        );
        assert_eq!(
            dqh_binrpc_classify_history_entry_fields(2, 1, 2, 3, false),
            0
        );
        assert_eq!(
            dqh_binrpc_classify_history_entry_fields(3, 1, 2, 3, false),
            3
        );
        assert_eq!(
            dqh_diskstate_classify_begin_read_recovery(true, true, true),
            1
        );
        assert_eq!(
            dqh_diskstate_classify_begin_read_recovery(true, false, true),
            2
        );
        assert_eq!(
            dqh_diskstate_classify_begin_read_recovery(true, true, false),
            0
        );
        assert_eq!(
            dqh_diskstate_select_volume_server_slot(0, &[1, 2], &[10, 20]),
            0
        );
        assert_eq!(
            dqh_diskstate_select_volume_server_slot(2, &[1, 2], &[10, 20]),
            20
        );
        assert_eq!(
            dqh_diskstate_select_volume_server_slot(3, &[1, 2], &[10, 20]),
            -1
        );
        assert_eq!(dqh_xmlrpc_classify_tls_cipher_op(0), 0);
        assert_eq!(dqh_xmlrpc_classify_tls_cipher_op(1), 1);
        assert_eq!(dqh_xmlrpc_classify_tls_cipher_op(2), 2);
        assert_eq!(
            dqh_xmlrpc_calc_schedule_resume_time(true, 60, 1000),
            Some(1060)
        );
        assert_eq!(dqh_xmlrpc_calc_schedule_resume_time(false, 60, 1000), None);
        assert_eq!(dqh_xmlrpc_calc_schedule_resume_time(true, -1, 1000), None);
        assert_eq!(
            dqh_xmlrpc_accum_queue_job_counts(true, 2, 2, 3, 4),
            (4, 5)
        );
        assert_eq!(
            dqh_xmlrpc_accum_queue_job_counts(false, 1, 2, 3, 4),
            (3, 4)
        );
        assert!(dqh_xmlrpc_is_fleet_member_limit_reached(16, 16));
        assert!(!dqh_xmlrpc_is_fleet_member_limit_reached(15, 16));
        assert_eq!(dqh_xmlrpc_classify_history_entry_kind(1, 1, 2, 3), 1);
        assert_eq!(dqh_xmlrpc_classify_history_entry_kind(2, 1, 2, 3), 1);
        assert_eq!(dqh_xmlrpc_classify_history_entry_kind(3, 1, 2, 3), 2);
        assert_eq!(dqh_xmlrpc_classify_history_entry_kind(0, 1, 2, 3), 0);
        assert!(dqh_xmlrpc_matches_edit_server_id(5, 5));
        assert!(!dqh_xmlrpc_matches_edit_server_id(4, 5));
        assert!(dqh_xmlrpc_matches_reset_server_volume(2, 2));
        assert!(dqh_xmlrpc_matches_reset_server_volume(2, -1));
        assert!(!dqh_xmlrpc_matches_reset_server_volume(2, 3));
        assert!(dqh_xmlrpc_is_valid_cert_verif_level(0, 3));
        assert!(!dqh_xmlrpc_is_valid_cert_verif_level(-1, 3));
        assert!(!dqh_xmlrpc_is_valid_cert_verif_level(3, 3));
        assert_eq!(dqh_xmlrpc_select_test_server_err_text(true, b"err"), b"");
        assert_eq!(
            dqh_xmlrpc_select_test_server_err_text(false, b""),
            b"Unknown error"
        );
        assert_eq!(
            dqh_xmlrpc_select_test_server_err_text(false, b"Connection refused"),
            b"Connection refused"
        );
        assert_eq!(dqh_xmlrpc_check_disk_speed_arg_ranges(-1, 1, 1), 1);
        assert_eq!(dqh_xmlrpc_check_disk_speed_arg_ranges(0, 0, 1), 2);
        assert_eq!(dqh_xmlrpc_check_disk_speed_arg_ranges(0, 1, 0), 3);
        assert_eq!(dqh_xmlrpc_check_disk_speed_arg_ranges(4, 1, 5), 0);
        assert_eq!(
            dqh_feed_coord_select_extensions(b"feed.py", b"global.py"),
            b"feed.py"
        );
        assert_eq!(
            dqh_feed_coord_select_extensions(b"", b"global.py"),
            b"global.py"
        );
        assert_eq!(dqh_feed_coord_min_last_update(1000, &[1200, 800, 950]), 800);
        assert_eq!(dqh_feed_coord_min_last_update(1000, &[]), 1000);
        assert!(dqh_feed_file_should_trim_text_chunk(b"category"));
        assert!(!dqh_feed_file_should_trim_text_chunk(b"title"));
        assert!(dqh_ext_mgr_is_extension_in_use(2));
        assert!(!dqh_ext_mgr_is_extension_in_use(1));
        assert!(dqh_ext_mgr_should_use_entry_as_path(2));
        assert!(!dqh_ext_mgr_should_use_entry_as_path(1));
        assert!(dqh_ext_mgr_can_create_scheduler_task(true, false, true));
        assert!(!dqh_ext_mgr_can_create_scheduler_task(false, false, true));
        assert!(!dqh_ext_mgr_can_create_scheduler_task(true, true, true));
        assert!(!dqh_ext_mgr_can_create_scheduler_task(true, false, false));
        assert_eq!(dqh_queue_script_select_exec_msg_kind(0, 0, 4, 1), 4);
        assert_eq!(dqh_queue_script_select_exec_msg_kind(1, 0, 4, 1), 1);
        assert!(dqh_queue_script_is_duplicate_file_downloaded_item(
            0, 0, 42, 42, true
        ));
        assert!(!dqh_queue_script_is_duplicate_file_downloaded_item(
            1, 0, 42, 42, true
        ));
        assert!(!dqh_queue_script_is_duplicate_file_downloaded_item(
            0, 0, 41, 42, true
        ));
        assert!(!dqh_queue_script_is_duplicate_file_downloaded_item(
            0, 0, 42, 42, false
        ));
        assert!(dqh_ncurses_is_row_in_scroll_window(2, 2, 5));
        assert!(dqh_ncurses_is_row_in_scroll_window(5, 2, 5));
        assert!(!dqh_ncurses_is_row_in_scroll_window(1, 2, 5));
        assert!(!dqh_ncurses_is_row_in_scroll_window(6, 2, 5));
        assert_eq!(
            dqh_ncurses_accum_file_queue_stats(true, 100, 500, 200, 3),
            (600, 300, 4)
        );
        assert_eq!(
            dqh_ncurses_accum_file_queue_stats(false, 100, 500, 200, 3),
            (600, 200, 3)
        );
        assert_eq!(
            dqh_ncurses_accum_group_queue_stats(400, 150, 1000, 300),
            (1400, 450)
        );
        assert_eq!(dqh_ncurses_enter_edit_queue_win_pct(3, 0), Some(50));
        assert_eq!(dqh_ncurses_enter_edit_queue_win_pct(3, 75), Some(75));
        assert_eq!(dqh_ncurses_enter_edit_queue_win_pct(0, 0), None);
        assert_eq!(dqh_ncurses_last_queue_index(5), 4);
        assert_eq!(dqh_ncurses_last_queue_index(0), 0);
        assert!(dqh_disk_service_should_check_disk_space(100, true));
        assert!(!dqh_disk_service_should_check_disk_space(0, true));
        assert!(!dqh_disk_service_should_check_disk_space(100, false));
        assert_eq!(
            dqh_disk_service_eval_missing_required_dir(false, false, false, false),
            (true, true, true)
        );
        assert_eq!(
            dqh_disk_service_eval_missing_required_dir(false, false, true, true),
            (true, false, true)
        );
        assert_eq!(
            dqh_disk_service_eval_missing_required_dir(true, false, false, false),
            (false, false, false)
        );
        assert!(dqh_maintenance_has_update_info(12));
        assert!(!dqh_maintenance_has_update_info(0));
        assert_eq!(dqh_options_resolve_locate_src_line(true, 42), 42);
        assert_eq!(dqh_options_resolve_locate_src_line(false, 42), 0);
        assert_eq!(dqh_options_update_config_line_on_get(55, 10), 55);
        assert_eq!(dqh_options_update_config_line_on_get(0, 10), 10);
        assert!(dqh_options_is_every_hour_task(-2));
        assert!(!dqh_options_is_every_hour_task(8));
        assert!(dqh_nzbget_should_run_system_health_check(true, true));
        assert!(!dqh_nzbget_should_run_system_health_check(true, false));
        assert!(!dqh_nzbget_should_run_system_health_check(false, true));
        assert_eq!(dqh_nzbget_classify_boot_usage_or_error(true, false, 2), Some(0));
        assert_eq!(dqh_nzbget_classify_boot_usage_or_error(false, true, 2), Some(1));
        assert_eq!(dqh_nzbget_classify_boot_usage_or_error(false, false, 1), Some(1));
        assert_eq!(dqh_nzbget_classify_boot_usage_or_error(false, false, 2), None);
        assert!(dqh_nzbget_should_stop_frontend_immediately(false, false));
        assert!(dqh_nzbget_should_stop_frontend_immediately(true, true));
        assert!(!dqh_nzbget_should_stop_frontend_immediately(true, false));
        assert_eq!(
            dqh_log_classify_init_logfile_action(false, false, 2, 0, 2, true, true),
            2
        );
        assert_eq!(
            dqh_log_classify_init_logfile_action(false, false, 1, 0, 2, true, true),
            1
        );
        assert_eq!(
            dqh_log_classify_init_logfile_action(true, false, 2, 0, 2, true, true),
            0
        );
        assert!(dqh_script_ctrl_is_terminate_ok(0, -1, false));
        assert!(dqh_script_ctrl_is_terminate_ok(123, 0, false));
        assert!(dqh_script_ctrl_is_terminate_ok(123, -1, true));
        assert!(!dqh_script_ctrl_is_terminate_ok(123, -1, false));
        assert!(dqh_script_ctrl_can_send_break(123, false));
        assert!(!dqh_script_ctrl_can_send_break(0, false));
        assert!(!dqh_script_ctrl_can_send_break(123, true));
        assert!(dqh_fs_should_fallback_move_to_copy(false, 18, 18));
        assert!(!dqh_fs_should_fallback_move_to_copy(true, 18, 18));
        assert!(!dqh_fs_should_fallback_move_to_copy(false, 13, 18));
        assert!(dqh_fs_is_copied_file_valid(true, true, 4096, 4096));
        assert!(!dqh_fs_is_copied_file_valid(false, true, 4096, 4096));
        assert!(!dqh_fs_is_copied_file_valid(true, false, 4096, 4096));
        assert!(!dqh_fs_is_copied_file_valid(true, true, 4000, 4096));
        assert_eq!(dqh_parchecker_resolve_fallback_par_buffer_mb(250), 250);
        assert_eq!(dqh_parchecker_resolve_fallback_par_buffer_mb(0), 16);
        assert!(dqh_parchecker_is_streambuf_line_break(i32::from(b'\n')));
        assert!(dqh_parchecker_is_streambuf_line_break(i32::from(b'\r')));
        assert!(!dqh_parchecker_is_streambuf_line_break(i32::from(b'a')));
        assert_eq!(dqh_parchecker_classify_preprocess_step(true, 0, 0, 5), 1);
        assert_eq!(dqh_parchecker_classify_preprocess_step(false, 5, 0, 5), 2);
        assert_eq!(dqh_parchecker_classify_preprocess_step(false, 3, 0, 5), 3);
        assert_eq!(dqh_parchecker_classify_preprocess_step(false, 0, 0, 5), 0);
        assert!(dqh_parchecker_can_start_twin_set(false, false));
        assert!(!dqh_parchecker_can_start_twin_set(true, false));
        assert!(!dqh_parchecker_can_start_twin_set(false, true));
        assert!(dqh_parchecker_should_update_after_twin_volumes(false, false));
        assert!(!dqh_parchecker_should_update_after_twin_volumes(true, false));
        assert!(!dqh_parchecker_should_update_after_twin_volumes(false, true));
        assert!(dqh_parchecker_is_missing_target_sourcefile(
            false,
            b"part01.rar",
            b"part01.rar"
        ));
        assert!(!dqh_parchecker_is_missing_target_sourcefile(
            true,
            b"part01.rar",
            b"part01.rar"
        ));
        assert!(!dqh_parchecker_is_missing_target_sourcefile(
            false,
            b"part01.rar",
            b"part02.rar"
        ));
        assert!(dqh_parchecker_is_terminal_quick_crc_status(2, 2, 0));
        assert!(dqh_parchecker_is_terminal_quick_crc_status(0, 2, 0));
        assert!(!dqh_parchecker_is_terminal_quick_crc_status(1, 2, 0));
        assert!(dqh_parchecker_is_partial_range_crc_valid(true, 0x1234, 0x1234));
        assert!(!dqh_parchecker_is_partial_range_crc_valid(false, 0x1234, 0x1234));
        assert!(!dqh_parchecker_is_partial_range_crc_valid(true, 0x1234, 0x5678));
        assert_eq!(dqh_unpack_ctrl_accum_unpack_sec(10, 1050, 1000), 60);
        assert_eq!(dqh_unpack_ctrl_select_sevenzip_wildcard(true), b"*.7z.001\0");
        assert_eq!(dqh_unpack_ctrl_select_sevenzip_wildcard(false), b"*.7z\0");
        assert_eq!(dqh_unpack_ctrl_calc_write_buffer_bytes(64), Some(65536));
        assert_eq!(dqh_unpack_ctrl_calc_write_buffer_bytes(0), None);
        assert!(dqh_unpack_ctrl_should_fail_on_close(false, true));
        assert!(!dqh_unpack_ctrl_should_fail_on_close(true, true));
        assert!(!dqh_unpack_ctrl_should_fail_on_close(false, false));
        assert!(dqh_unpack_ctrl_is_completed_success(true, true));
        assert!(!dqh_unpack_ctrl_is_completed_success(true, false));
        assert_eq!(
            dqh_unpack_ctrl_select_unpack_parent_dir(b"/final", b"/dest"),
            b"/final"
        );
        assert_eq!(
            dqh_unpack_ctrl_select_unpack_parent_dir(b"", b"/dest"),
            b"/dest"
        );
        assert!(dqh_unpack_ctrl_is_move_extracted_error(false, false));
        assert!(!dqh_unpack_ctrl_is_move_extracted_error(false, true));
        assert!(!dqh_unpack_ctrl_is_move_extracted_error(true, false));
        assert!(dqh_unpack_ctrl_should_delete_created_final_dir(false, true));
        assert!(!dqh_unpack_ctrl_should_delete_created_final_dir(true, true));
        assert!(!dqh_unpack_ctrl_should_delete_created_final_dir(false, false));
        assert!(dqh_prepost_should_reset_stale_direct_unpack(1, 1, false));
        assert!(!dqh_prepost_should_reset_stale_direct_unpack(1, 1, true));
        assert!(!dqh_prepost_should_reset_stale_direct_unpack(0, 1, false));
        assert!(dqh_prepost_has_single_main_par(true, 1));
        assert!(!dqh_prepost_has_single_main_par(true, 2));
        assert!(!dqh_prepost_has_single_main_par(false, 1));
        assert!(dqh_prepost_matches_post_delete_target(true, 7, 7));
        assert!(!dqh_prepost_matches_post_delete_target(false, 7, 7));
        assert!(!dqh_prepost_matches_post_delete_target(true, 7, 8));
        assert!(dqh_repair_ctrl_should_skip_twin_volume_entry(
            false,
            b"a.rar",
            b"a.par2"
        ));
        assert!(dqh_repair_ctrl_should_skip_twin_volume_entry(
            true,
            b"a.par2",
            b"a.par2"
        ));
        assert!(!dqh_repair_ctrl_should_skip_twin_volume_entry(
            true,
            b"a.vol00+01.par2",
            b"a.par2"
        ));
        assert!(dqh_repair_ctrl_matches_history_dupe_source(1, 1, 99, 99));
        assert!(!dqh_repair_ctrl_matches_history_dupe_source(2, 1, 99, 99));
        assert!(!dqh_repair_ctrl_matches_history_dupe_source(1, 1, 98, 99));
        assert!(dqh_cleanup_ctrl_should_cleanup_final_dir(true, false));
        assert!(!dqh_cleanup_ctrl_should_cleanup_final_dir(false, false));
        assert!(!dqh_cleanup_ctrl_should_cleanup_final_dir(true, true));
        assert!(dqh_cleanup_ctrl_is_traversable_dir(true, false));
        assert!(!dqh_cleanup_ctrl_is_traversable_dir(true, true));
        assert!(!dqh_cleanup_ctrl_is_traversable_dir(false, false));
        assert!(dqh_direct_unpack_should_log_final_status(1, 1, 2));
        assert!(dqh_direct_unpack_should_log_final_status(2, 1, 2));
        assert!(!dqh_direct_unpack_should_log_final_status(0, 1, 2));
        assert!(dqh_direct_unpack_should_keep_waiting_break(10, true, true));
        assert!(!dqh_direct_unpack_should_keep_waiting_break(50, true, true));
        assert!(!dqh_direct_unpack_should_keep_waiting_break(10, false, true));
        assert!(dqh_direct_unpack_should_delete_temp_unpack_dir(true, true));
        assert!(!dqh_direct_unpack_should_delete_temp_unpack_dir(false, true));
        assert!(!dqh_direct_unpack_should_delete_temp_unpack_dir(true, false));
        assert!(dqh_rename_ctrl_should_reset_par_rename_status(1024));
        assert!(!dqh_rename_ctrl_should_reset_par_rename_status(0));
        assert!(dqh_parparser_has_found_main_pars(true, false));
        assert!(!dqh_parparser_has_found_main_pars(false, false));
        assert!(!dqh_parparser_has_found_main_pars(true, true));
        assert!(dqh_par_renamer_needs_create_target_dir(b"/a", b"/b"));
        assert!(!dqh_par_renamer_needs_create_target_dir(b"/a", b"/a"));
        assert!(dqh_rar_volume_is_read_handle_ready(100, true));
        assert!(!dqh_rar_volume_is_read_handle_ready(-1, true));
        assert!(!dqh_rar_volume_is_read_handle_ready(100, false));
        assert!(dqh_rar_volume_should_update_version(false, false));
        assert!(dqh_rar_volume_should_update_version(true, true));
        assert!(!dqh_rar_volume_should_update_version(true, false));
        assert!(dqh_rar_volume_is_decrypt_buf_valid(true, 16));
        assert!(!dqh_rar_volume_is_decrypt_buf_valid(false, 16));
        assert!(!dqh_rar_volume_is_decrypt_buf_valid(true, 8));
        assert!(dqh_rar_renamer_should_ignore_file(true, true));
        assert!(!dqh_rar_renamer_should_ignore_file(false, true));
        assert!(!dqh_rar_renamer_should_ignore_file(true, false));
        assert!(dqh_dupematcher_should_wait_rar_lister(true, 1000, 60, 1000));
        assert!(!dqh_dupematcher_should_wait_rar_lister(true, 1000, 60, 1020));
        assert!(!dqh_dupematcher_should_wait_rar_lister(true, 1000, 60, 1060));
        assert!(dqh_dupematcher_is_larger_file(500, 400));
        assert!(!dqh_dupematcher_is_larger_file(400, 400));
        assert!(dqh_content_map_can_map_cipher_range(0, 16, 3, 3));
        assert!(!dqh_content_map_can_map_cipher_range(-1, 16, 3, 3));
        assert!(!dqh_content_map_can_map_cipher_range(0, 0, 3, 3));
        assert!(!dqh_content_map_can_map_cipher_range(0, 16, 2, 3));
        assert_eq!(dqh_content_mapper_classify_unreadable_rar_vol(true, true, -1), 1);
        assert_eq!(dqh_content_mapper_classify_unreadable_rar_vol(true, false, 0), 2);
        assert_eq!(dqh_content_mapper_classify_unreadable_rar_vol(true, false, -1), 0);
        assert!(dqh_stream_repair_is_attached_live_thread(true, true));
        assert!(!dqh_stream_repair_is_attached_live_thread(true, false));
        assert!(dqh_stream_repair_has_live_recovered_stats(1024, 0));
        assert!(dqh_stream_repair_has_live_recovered_stats(0, 1));
        assert!(!dqh_stream_repair_has_live_recovered_stats(0, 0));
        assert!(dqh_stream_repair_watchdog_has_activity(200, 100, false, 0));
        assert!(dqh_stream_repair_watchdog_has_activity(100, 100, true, 0));
        assert!(dqh_stream_repair_watchdog_has_activity(100, 100, false, 1));
        assert!(!dqh_stream_repair_watchdog_has_activity(100, 100, false, 0));
        assert_eq!(dqh_stream_repair_watchdog_calc_remaining_bytes(1000, 400, 100), 700);
        assert_eq!(dqh_stream_repair_watchdog_calc_remaining_bytes(200, 500, 100), 0);
        assert!(dqh_stream_repair_is_chunk_read_complete(4096, 4096));
        assert!(!dqh_stream_repair_is_chunk_read_complete(2048, 4096));
        assert!(dqh_article_dl_is_terminal_loop_status(2, 2, 5));
        assert!(dqh_article_dl_is_terminal_loop_status(5, 2, 5));
        assert!(!dqh_article_dl_is_terminal_loop_status(1, 2, 5));
        assert!(dqh_article_dl_should_fail_for_desired_server(false, true));
        assert!(!dqh_article_dl_should_fail_for_desired_server(true, true));
        assert!(!dqh_article_dl_should_fail_for_desired_server(false, false));
        assert!(dqh_article_dl_should_continue_read_loop(false, false));
        assert!(!dqh_article_dl_should_continue_read_loop(true, false));
        assert!(!dqh_article_dl_should_continue_read_loop(false, true));
        assert!(dqh_article_dl_should_run_decode_check(1, 1));
        assert!(!dqh_article_dl_should_run_decode_check(3, 1));
        assert!(dqh_server_pool_has_available_level_conns(0, 3, 2));
        assert!(!dqh_server_pool_has_available_level_conns(3, 3, 2));
        assert!(!dqh_server_pool_has_available_level_conns(0, 3, 0));
        assert!(!dqh_server_pool_has_available_level_conns(-1, 3, 2));
        assert_eq!(dqh_server_pool_pick_candidate_index(7, 3), 1);
        assert_eq!(dqh_server_pool_pick_candidate_index(7, 0), 0);
        assert!(dqh_server_pool_should_disconnect_idle_level_conn(1, 1, 2, 2));
        assert!(!dqh_server_pool_should_disconnect_idle_level_conn(0, 1, 2, 2));
        assert!(!dqh_server_pool_should_disconnect_idle_level_conn(1, 1, 0, 2));
        assert!(dqh_statmeter_has_any_quota_configured(10, 0));
        assert!(dqh_statmeter_has_any_quota_configured(0, 100));
        assert!(!dqh_statmeter_has_any_quota_configured(0, 0));
        assert!(dqh_statmeter_is_before_quota_start_day(15, 10));
        assert!(!dqh_statmeter_is_before_quota_start_day(15, 15));
        assert!(dqh_statmeter_did_prev_month_day_overflow(2, 31));
        assert!(!dqh_statmeter_did_prev_month_day_overflow(15, 15));
        assert_eq!(dqh_nntp_conn_classify_handshake_result(0), 0);
        assert_eq!(dqh_nntp_conn_classify_handshake_result(1), 1);
        assert_eq!(dqh_nntp_conn_classify_handshake_result(-1), -1);
        assert!(dqh_article_fetcher_should_keep_waiting_retry(9, 1, false));
        assert!(!dqh_article_fetcher_should_keep_waiting_retry(10, 1, false));
        assert!(!dqh_article_fetcher_should_keep_waiting_retry(0, 1, true));
        assert!(dqh_article_fetcher_should_disconnect_on_release(false, 1, 3));
        assert!(dqh_article_fetcher_should_disconnect_on_release(true, 3, 3));
        assert!(!dqh_article_fetcher_should_disconnect_on_release(true, 1, 3));
        assert!(dqh_article_fetcher_is_join_group_failure_transient(false));
        assert!(!dqh_article_fetcher_is_join_group_failure_transient(true));
        assert!(dqh_article_batch_should_ensure_workers(4, false));
        assert!(!dqh_article_batch_should_ensure_workers(0, false));
        assert!(!dqh_article_batch_should_ensure_workers(4, true));
        assert!(dqh_article_writer_is_dupe_fallback_article(true, 1));
        assert!(!dqh_article_writer_is_dupe_fallback_article(true, 0));
        assert!(!dqh_article_writer_is_dupe_fallback_article(false, 1));
        assert!(dqh_article_writer_was_renamed_during_completion(b"new.rar", b"old.rar"));
        assert!(!dqh_article_writer_was_renamed_during_completion(b"same.rar", b"same.rar"));
        assert!(dqh_article_writer_should_delete_hardlinked_without_interdir(true, true));
        assert!(!dqh_article_writer_should_delete_hardlinked_without_interdir(true, false));
        assert!(!dqh_article_writer_should_delete_hardlinked_without_interdir(false, true));
        assert!(dqh_article_writer_did_dest_dir_change(b"/dst2", b"/dst1"));
        assert!(!dqh_article_writer_did_dest_dir_change(b"/dst1", b"/dst1"));
        assert!(dqh_article_cache_should_continue_run_loop(false, 0));
        assert!(dqh_article_cache_should_continue_run_loop(true, 1024));
        assert!(!dqh_article_cache_should_continue_run_loop(true, 0));
        assert!(dqh_memory_monitor_should_keep_sleeping(false, 100, 500));
        assert!(!dqh_memory_monitor_should_keep_sleeping(false, 500, 500));
        assert!(!dqh_memory_monitor_should_keep_sleeping(true, 100, 500));
        assert!(dqh_webdl_should_continue_retry_loop(false, 2, 2));
        assert!(!dqh_webdl_should_continue_retry_loop(true, 2, 2));
        assert!(!dqh_webdl_should_continue_retry_loop(false, 0, 2));
        assert!(!dqh_webdl_should_continue_retry_loop(false, 2, 0));
        assert!(dqh_webdl_should_keep_waiting_retry_interval(false, 500, 2, false, false));
        assert!(!dqh_webdl_should_keep_waiting_retry_interval(false, 2000, 2, false, false));
        assert!(!dqh_webdl_should_keep_waiting_retry_interval(false, 500, 2, false, true));
        assert!(dqh_webdl_should_keep_waiting_retry_interval(false, 500, 2, true, true));
        assert!(dqh_webdl_is_terminal_retry_status(1, 1, 2, 3));
        assert!(dqh_webdl_is_terminal_retry_status(2, 1, 2, 3));
        assert!(dqh_webdl_is_terminal_retry_status(3, 1, 2, 3));
        assert!(!dqh_webdl_is_terminal_retry_status(4, 1, 2, 3));
        assert!(dqh_conn_is_unix_path_too_long(108, 108));
        assert!(!dqh_conn_is_unix_path_too_long(107, 108));
        assert_eq!(dqh_conn_resolve_gai_report_err_code(-2, -11), -2);
        assert_eq!(dqh_conn_resolve_gai_report_err_code(-11, -11), 0);
        assert!(dqh_conn_should_report_socket_create_failed(true, true));
        assert!(!dqh_conn_should_report_socket_create_failed(false, true));
        assert!(!dqh_conn_should_report_socket_create_failed(true, false));
        assert!(dqh_conn_should_cleanup_unconnected_socket(false, false));
        assert!(!dqh_conn_should_cleanup_unconnected_socket(true, false));
        assert!(!dqh_conn_should_cleanup_unconnected_socket(false, true));
        assert!(dqh_conn_should_unlink_listening_unix_socket(true, 0, 1));
        assert!(!dqh_conn_should_unlink_listening_unix_socket(false, 0, 1));
        assert!(!dqh_conn_should_unlink_listening_unix_socket(true, -1, 1));
        assert!(!dqh_conn_should_unlink_listening_unix_socket(true, 0, 0));
        assert_eq!(dqh_conn_resolve_report_err_code(0, 111), 111);
        assert_eq!(dqh_conn_resolve_report_err_code(22, 111), 22);
        assert!(dqh_tls_has_cert_and_key_files(false, false));
        assert!(!dqh_tls_has_cert_and_key_files(true, false));
        assert!(!dqh_tls_has_cert_and_key_files(false, true));
        assert!(dqh_tls_should_set_sni_hostname(true, false));
        assert!(!dqh_tls_should_set_sni_hostname(false, false));
        assert!(!dqh_tls_should_set_sni_hostname(true, true));
        assert!(dqh_tls_should_validate_client_cert(true, true));
        assert!(!dqh_tls_should_validate_client_cert(false, true));
        assert!(!dqh_tls_should_validate_client_cert(true, false));
        assert!(dqh_tls_is_proto_version_out_of_bounds(0x0303, 0x0304, 0));
        assert!(dqh_tls_is_proto_version_out_of_bounds(0x0304, 0, 0x0303));
        assert!(!dqh_tls_is_proto_version_out_of_bounds(0x0304, 0x0303, 0x0304));
        assert!(dqh_tls_should_send_close_notify(true, true, false));
        assert!(!dqh_tls_should_send_close_notify(true, true, true));
        assert!(!dqh_tls_should_send_close_notify(true, false, false));
        assert!(dqh_webdl_is_connect_aborted(false, false));
        assert!(dqh_webdl_is_connect_aborted(true, true));
        assert!(!dqh_webdl_is_connect_aborted(true, false));
        assert!(dqh_webdl_should_download_body(1, 1));
        assert!(!dqh_webdl_should_download_body(2, 1));
        assert!(dqh_webdl_should_delete_broken_output(2, 1));
        assert!(!dqh_webdl_should_delete_broken_output(1, 1));
        assert!(dqh_webdl_needs_prepare_file(false));
        assert!(!dqh_webdl_needs_prepare_file(true));
        assert!(dqh_webdl_is_gunzip_write_failed(128, 0));
        assert!(!dqh_webdl_is_gunzip_write_failed(128, 128));
        assert!(!dqh_webdl_is_gunzip_write_failed(0, 0));
        assert!(dqh_webdl_should_disconnect_on_free(3, 3));
        assert!(!dqh_webdl_should_disconnect_on_free(1, 3));
        assert!(dqh_speed_test_is_iteration_failed(0.0, 1024, 1024));
        assert!(dqh_speed_test_is_iteration_failed(1.5, 512, 1024));
        assert!(!dqh_speed_test_is_iteration_failed(1.5, 1024, 1024));
        assert!(dqh_speed_test_should_stop_recv_loop(0, 0.5, 5.0));
        assert!(dqh_speed_test_should_stop_recv_loop(1024, 5.0, 5.0));
        assert!(!dqh_speed_test_should_stop_recv_loop(1024, 1.0, 5.0));
        let mut pt = [0u8; 4];
        dqh_tls_cipher_fill_test_plaintext(&mut pt);
        assert_eq!(pt, [0, 31, 62, 93]);
        assert!(dqh_tls_cipher_is_prepare_batch_valid(100, 16));
        assert!(!dqh_tls_cipher_is_prepare_batch_valid(100, 0));
        assert!(!dqh_tls_cipher_is_prepare_batch_valid(100, 65));
        assert!(!dqh_tls_cipher_is_prepare_batch_valid(u64::MAX - 5, 10));
        let mut nonce = [0u8; 12];
        dqh_tls_cipher_write_record_nonce(&[1, 2, 3, 4], 0x0102_0304_0506_0708, &mut nonce);
        assert_eq!(nonce, [1, 2, 3, 4, 1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(dqh_tls_cipher_is_record_seq_in_range(105, 100, 8));
        assert!(!dqh_tls_cipher_is_record_seq_in_range(99, 100, 8));
        assert!(!dqh_tls_cipher_is_record_seq_in_range(108, 100, 8));
        assert_eq!(dqh_tls_cipher_precheck_probe_args(true, 1000, true, true), -6);
        assert_eq!(dqh_tls_cipher_precheck_probe_args(false, 0, true, true), -3);
        assert_eq!(dqh_tls_cipher_precheck_probe_args(false, 10001, true, true), -1);
        assert_eq!(dqh_tls_cipher_precheck_probe_args(false, 1000, true, false), -4);
        assert_eq!(dqh_tls_cipher_precheck_probe_args(false, 1000, true, true), 0);
        assert!(dqh_tls_cipher_is_probe_addr_list_valid(4));
        assert!(!dqh_tls_cipher_is_probe_addr_list_valid(0));
        assert!(!dqh_tls_cipher_is_probe_addr_list_valid(17));
        assert!(dqh_queue_coord_should_cleanup_temp_dir(true, true));
        assert!(!dqh_queue_coord_should_cleanup_temp_dir(true, false));
        assert!(!dqh_queue_coord_should_cleanup_temp_dir(false, true));
        assert!(dqh_queue_coord_should_resave_legacy_queue(true, true));
        assert!(!dqh_queue_coord_should_resave_legacy_queue(true, false));
        assert!(dqh_queue_coord_should_resave_on_server_mismatch(true, true, false));
        assert!(!dqh_queue_coord_should_resave_on_server_mismatch(true, true, true));
        assert!(!dqh_queue_coord_should_resave_on_server_mismatch(false, true, false));
        assert!(dqh_queue_coord_is_nzb_eligible_for_next_article(
            false, false, false, false, false, false, false
        ));
        assert!(dqh_queue_coord_is_nzb_eligible_for_next_article(
            true, true, false, false, true, false, true
        ));
        assert!(!dqh_queue_coord_is_nzb_eligible_for_next_article(
            true, false, false, false, false, false, false
        ));
        assert!(!dqh_queue_coord_is_nzb_eligible_for_next_article(
            false, false, false, false, true, false, false
        ));
        assert!(dqh_queue_coord_should_log_propagation_wait(true, 10, 5));
        assert!(!dqh_queue_coord_should_log_propagation_wait(true, 10, 10));
        assert!(!dqh_queue_coord_should_log_propagation_wait(false, 10, 5));
        assert!(dqh_queue_coord_can_select_first_article(false, false, 0, 0));
        assert!(!dqh_queue_coord_can_select_first_article(true, false, 0, 0));
        assert!(!dqh_queue_coord_can_select_first_article(false, true, 0, 0));
        assert!(!dqh_queue_coord_can_select_first_article(false, false, 1, 0));
        assert!(dqh_queue_coord_should_check_donor_alignment(2, 2, 1, 5000));
        assert!(!dqh_queue_coord_should_check_donor_alignment(2, 2, 0, 5000));
        assert!(!dqh_queue_coord_should_check_donor_alignment(2, 2, 1, 0));
        assert!(dqh_queue_coord_can_try_reactive_dupe_fallback(3, 3, false));
        assert!(dqh_queue_coord_can_try_reactive_dupe_fallback(2, 3, true));
        assert!(!dqh_queue_coord_can_try_reactive_dupe_fallback(2, 3, false));
        assert!(dqh_queue_coord_is_terminal_article_failure(3, 3, 5, false));
        assert!(dqh_queue_coord_is_terminal_article_failure(5, 3, 5, false));
        assert!(dqh_queue_coord_is_terminal_article_failure(2, 3, 5, true));
        assert!(!dqh_queue_coord_is_terminal_article_failure(2, 3, 5, false));
        assert!(dqh_queue_coord_should_confirm_article_filename(false, false, 2, 2, true));
        assert!(!dqh_queue_coord_should_confirm_article_filename(true, false, 2, 2, true));
        assert!(!dqh_queue_coord_should_confirm_article_filename(false, true, 2, 2, true));
        assert!(!dqh_queue_coord_should_confirm_article_filename(false, false, 2, 2, false));
        assert!(dqh_queue_coord_should_check_existing_dest_dupe(true, 0, 2, false));
        assert!(!dqh_queue_coord_should_check_existing_dest_dupe(false, 0, 2, false));
        assert!(!dqh_queue_coord_should_check_existing_dest_dupe(true, 2, 2, false));
        assert!(!dqh_queue_coord_should_check_existing_dest_dupe(true, 0, 2, true));
        assert!(dqh_queue_coord_should_process_analyzed_first_article(true, false, 2, 2));
        assert!(!dqh_queue_coord_should_process_analyzed_first_article(false, false, 2, 2));
        assert!(!dqh_queue_coord_should_process_analyzed_first_article(true, true, 2, 2));
        assert!(dqh_queue_coord_should_log_file_dupe_recovery_summary(true, 3));
        assert!(!dqh_queue_coord_should_log_file_dupe_recovery_summary(false, 3));
        assert!(!dqh_queue_coord_should_log_file_dupe_recovery_summary(true, 0));
        assert!(dqh_queue_coord_should_update_completed_stats_on_delete(true, false));
        assert!(dqh_queue_coord_should_update_completed_stats_on_delete(false, true));
        assert!(!dqh_queue_coord_should_update_completed_stats_on_delete(false, false));
        assert!(dqh_queue_coord_should_check_failed_output_exists(true, 3, 3, false));
        assert!(!dqh_queue_coord_should_check_failed_output_exists(false, 3, 3, false));
        assert!(!dqh_queue_coord_should_check_failed_output_exists(true, 1, 3, false));
        assert!(!dqh_queue_coord_should_check_failed_output_exists(true, 3, 3, true));
        assert!(dqh_queue_coord_needs_completed_tiling_validation(10, 10));
        assert!(!dqh_queue_coord_needs_completed_tiling_validation(9, 10));
        assert!(dqh_queue_coord_needs_partial_direct_write_init(true, false, false));
        assert!(dqh_queue_coord_needs_partial_direct_write_init(false, true, false));
        assert!(!dqh_queue_coord_needs_partial_direct_write_init(true, false, true));
        assert!(!dqh_queue_coord_needs_partial_direct_write_init(false, false, false));
        assert!(dqh_queue_coord_should_skip_critical_health_calc(0, 0, false, 0, 4));
        assert!(dqh_queue_coord_should_skip_critical_health_calc(1, 0, true, 0, 4));
        assert!(dqh_queue_coord_should_skip_critical_health_calc(1, 0, false, 4, 4));
        assert!(!dqh_queue_coord_should_skip_critical_health_calc(1, 0, false, 0, 4));
        assert!(dqh_queue_coord_should_check_certainly_parless(3, 3, 1000));
        assert!(!dqh_queue_coord_should_check_certainly_parless(3, 3, 850));
        assert!(!dqh_queue_coord_should_check_certainly_parless(1, 3, 1000));
        assert_eq!(dqh_queue_coord_select_failover_effective_health(1000, 850, 950), 0);
        assert_eq!(dqh_queue_coord_select_failover_effective_health(850, 850, 800), 800);
        assert!(dqh_queue_coord_should_abort_dead_download_action(false, false, -1, false));
        assert!(dqh_queue_coord_should_abort_dead_download_action(true, false, 5, false));
        assert!(!dqh_queue_coord_should_abort_dead_download_action(true, false, 5, true));
        assert!(!dqh_queue_coord_should_abort_dead_download_action(true, false, -1, false));
        assert_eq!(dqh_queue_coord_validate_merge_entries(true, false, 0, 0, 1), 1);
        assert_eq!(dqh_queue_coord_validate_merge_entries(false, false, 1, 0, 1), 2);
        assert_eq!(dqh_queue_coord_validate_merge_entries(false, false, 0, 0, 1), 0);
        assert!(dqh_queue_coord_should_pause_pars_after_direct_rename(0, 2));
        assert!(!dqh_queue_coord_should_pause_pars_after_direct_rename(2, 2));
        assert!(dqh_queue_coord_should_log_discarded_direct_rename(1024));
        assert!(!dqh_queue_coord_should_log_discarded_direct_rename(0));
        assert_eq!(dqh_diskstate_resolve_read_version(66, 64), 66);
        assert_eq!(dqh_diskstate_resolve_read_version(0, 64), 64);
        assert!(dqh_diskstate_state_file_exists(true, false, false));
        assert!(dqh_diskstate_state_file_exists(false, true, true));
        assert!(!dqh_diskstate_state_file_exists(false, false, true));
        assert!(dqh_diskstate_should_flush_queue_file(true, true));
        assert!(!dqh_diskstate_should_flush_queue_file(true, false));
        assert!(!dqh_diskstate_should_flush_queue_file(false, true));
        assert!(dqh_diskstate_is_file_version_unsupported(67, 66));
        assert!(!dqh_diskstate_is_file_version_unsupported(66, 66));
        assert!(dqh_diskstate_should_load_separate_history_file(0));
        assert!(dqh_diskstate_should_load_separate_history_file(57));
        assert!(!dqh_diskstate_should_load_separate_history_file(56));
        assert!(dqh_diskstate_should_reload_fallback_file_infos(false, false));
        assert!(!dqh_diskstate_should_reload_fallback_file_infos(true, false));
        assert!(!dqh_diskstate_should_reload_fallback_file_infos(false, true));
        assert_eq!(dqh_diskstate_expected_repair_job_fields(66), 11);
        assert_eq!(dqh_diskstate_expected_repair_job_fields(65), 9);
        assert!(dqh_diskstate_is_nonneg_count_scan_invalid(0, 5));
        assert!(dqh_diskstate_is_nonneg_count_scan_invalid(1, -1));
        assert!(!dqh_diskstate_is_nonneg_count_scan_invalid(1, 0));
        assert!(dqh_diskstate_has_meta_name_field(64));
        assert!(!dqh_diskstate_has_meta_name_field(65));
        assert!(dqh_diskstate_has_meta_name_field(66));
        assert!(!dqh_diskstate_has_meta_name_field(63));
        assert!(dqh_diskstate_matches_server_state_id(2, 2));
        assert!(!dqh_diskstate_matches_server_state_id(1, 2));
        assert_eq!(
            dqh_diskstate_select_original_or_current_msg_id(b"orig@id", b"cur@id"),
            b"orig@id"
        );
        assert_eq!(
            dqh_diskstate_select_original_or_current_msg_id(b"", b"cur@id"),
            b"cur@id"
        );
        assert!(dqh_diskstate_should_probe_direct_write_files(1, true, false));
        assert!(dqh_diskstate_should_probe_direct_write_files(1, false, true));
        assert!(!dqh_diskstate_should_probe_direct_write_files(0, true, true));
        assert!(!dqh_diskstate_should_probe_direct_write_files(1, false, false));
        assert!(dqh_diskstate_can_check_completed_dest_file(true, false, false, false));
        assert!(dqh_diskstate_can_check_completed_dest_file(false, true, false, false));
        assert!(!dqh_diskstate_can_check_completed_dest_file(false, false, false, false));
        assert!(!dqh_diskstate_can_check_completed_dest_file(true, false, true, false));
        assert!(!dqh_diskstate_can_check_completed_dest_file(true, false, false, true));
        assert!(dqh_diskstate_matches_history_nzb_id(1, 1, 2, 42, 42));
        assert!(dqh_diskstate_matches_history_nzb_id(2, 1, 2, 42, 42));
        assert!(!dqh_diskstate_matches_history_nzb_id(3, 1, 2, 42, 42));
        assert!(!dqh_diskstate_matches_history_nzb_id(1, 1, 2, 41, 42));
        assert!(dqh_diskstate_should_discard_feeds_file(true, true));
        assert!(!dqh_diskstate_should_discard_feeds_file(false, true));
        assert!(!dqh_diskstate_should_discard_feeds_file(true, false));
        assert!(dqh_diskstate_can_store_volume_slot(true, 3, 5));
        assert!(!dqh_diskstate_can_store_volume_slot(false, 3, 5));
        assert!(!dqh_diskstate_can_store_volume_slot(true, 5, 5));
        assert!(dqh_dupe_fallback_should_skip_try_entry(0, 0, false, false));
        assert!(dqh_dupe_fallback_should_skip_try_entry(1, 0, true, false));
        assert!(dqh_dupe_fallback_should_skip_try_entry(1, 0, false, true));
        assert!(!dqh_dupe_fallback_should_skip_try_entry(1, 0, false, false));
        assert!(dqh_dupe_fallback_can_defer_to_par(0, 1, true));
        assert!(!dqh_dupe_fallback_can_defer_to_par(1, 1, true));
        assert!(!dqh_dupe_fallback_can_defer_to_par(0, 1, false));
        assert!(dqh_dupe_fallback_should_skip_blocked_donor_round(1, 3, 3, true));
        assert!(!dqh_dupe_fallback_should_skip_blocked_donor_round(3, 3, 3, true));
        assert!(!dqh_dupe_fallback_should_skip_blocked_donor_round(1, 3, 3, false));
        assert!(dqh_dupe_fallback_should_rotate_donors(2, 4));
        assert!(!dqh_dupe_fallback_should_rotate_donors(0, 4));
        assert!(!dqh_dupe_fallback_should_rotate_donors(4, 4));
        assert!(dqh_dupe_fallback_needs_twin_lead_check(true, 7));
        assert!(!dqh_dupe_fallback_needs_twin_lead_check(false, 7));
        assert!(!dqh_dupe_fallback_needs_twin_lead_check(true, 0));
        assert!(dqh_dupe_fallback_can_parse_donor_nzb(false, true));
        assert!(!dqh_dupe_fallback_can_parse_donor_nzb(true, true));
        assert!(!dqh_dupe_fallback_can_parse_donor_nzb(false, false));
        assert!(dqh_dupe_fallback_can_query_content_match(5, false, false));
        assert!(!dqh_dupe_fallback_can_query_content_match(0, false, false));
        assert!(!dqh_dupe_fallback_can_query_content_match(5, true, false));
        assert!(!dqh_dupe_fallback_can_query_content_match(5, false, true));
        assert!(dqh_dupe_fallback_is_donor_file_struct_candidate(false, true));
        assert!(!dqh_dupe_fallback_is_donor_file_struct_candidate(true, true));
        assert!(!dqh_dupe_fallback_is_donor_file_struct_candidate(false, false));
        assert!(dqh_history_coord_should_mark_bad_in_prepare_edit(5, 5, true, 1, 1));
        assert!(!dqh_history_coord_should_mark_bad_in_prepare_edit(4, 5, true, 1, 1));
        assert!(!dqh_history_coord_should_mark_bad_in_prepare_edit(5, 5, false, 1, 1));
        assert!(!dqh_history_coord_should_mark_bad_in_prepare_edit(5, 5, true, 2, 1));
        assert!(dqh_history_coord_should_load_partial_file_state(true, 2, 2));
        assert!(!dqh_history_coord_should_load_partial_file_state(false, 2, 2));
        assert!(!dqh_history_coord_should_load_partial_file_state(true, 1, 2));
        assert_eq!(dqh_history_coord_classify_success_output_state(0, true, false), 0);
        assert_eq!(dqh_history_coord_classify_success_output_state(5, true, false), 1);
        assert_eq!(dqh_history_coord_classify_success_output_state(5, false, false), 2);
        assert_eq!(dqh_history_coord_classify_success_output_state(5, false, true), 0);
        assert_eq!(dqh_dupecoord_select_disk_dir(b"/final", b"/dest"), b"/final");
        assert_eq!(dqh_dupecoord_select_disk_dir(b"", b"/dest"), b"/dest");
        assert!(dqh_dupecoord_is_unbacked_active_queue_dupe(false, 0, 0, true));
        assert!(!dqh_dupecoord_is_unbacked_active_queue_dupe(true, 0, 0, true));
        assert!(!dqh_dupecoord_is_unbacked_active_queue_dupe(false, 1, 0, true));
        assert!(!dqh_dupecoord_is_unbacked_active_queue_dupe(false, 0, 0, false));
        assert!(dqh_dupecoord_should_skip_url_redownload_non_same_content(false, 2, 0));
        assert!(!dqh_dupecoord_should_skip_url_redownload_non_same_content(true, 2, 0));
        assert!(!dqh_dupecoord_should_skip_url_redownload_non_same_content(false, 0, 0));
        assert!(dqh_dupecoord_should_check_no_backup_file_exists(1, 1, 2, 0, 2));
        assert!(dqh_dupecoord_should_check_no_backup_file_exists(2, 1, 2, 0, 2));
        assert!(!dqh_dupecoord_should_check_no_backup_file_exists(0, 1, 2, 0, 2));
        assert!(!dqh_dupecoord_should_check_no_backup_file_exists(1, 1, 2, 2, 2));
        assert_eq!(
            dqh_queue_editor_classify_prepare_list_branch(1, 1, 1, 2, 3, 10, 11, 12, 13, 14),
            1
        );
        assert_eq!(
            dqh_queue_editor_classify_prepare_list_branch(1, 10, 1, 2, 3, 10, 11, 12, 13, 14),
            2
        );
        assert_eq!(
            dqh_queue_editor_classify_prepare_list_branch(0, 13, 1, 2, 3, 10, 11, 12, 13, 14),
            2
        );
        assert_eq!(
            dqh_queue_editor_classify_prepare_list_branch(0, 5, 1, 2, 3, 10, 11, 12, 13, 14),
            3
        );
        assert_eq!(
            dqh_queue_editor_classify_prepare_list_branch(0, 15, 1, 2, 3, 10, 11, 12, 13, 14),
            4
        );
        assert_eq!(dqh_queue_editor_is_pause_pars_action(7, 7, 8), (true, false));
        assert_eq!(dqh_queue_editor_is_pause_pars_action(8, 7, 8), (true, true));
        assert_eq!(dqh_queue_editor_is_pause_pars_action(9, 7, 8), (false, false));
        assert!(dqh_scanner_can_process_archive_entry(true, true));
        assert!(!dqh_scanner_can_process_archive_entry(true, false));
        assert!(!dqh_scanner_can_process_archive_entry(false, true));
        assert!(dqh_scanner_should_assign_url_info_to_archive_entry(0, true));
        assert!(!dqh_scanner_should_assign_url_info_to_archive_entry(1, true));
        assert!(!dqh_scanner_should_assign_url_info_to_archive_entry(0, false));
        assert!(dqh_twin_check_should_unregister_nudge(false, true));
        assert!(!dqh_twin_check_should_unregister_nudge(true, true));
        assert!(!dqh_twin_check_should_unregister_nudge(false, false));
        assert!(dqh_twin_check_can_queue_fingerprint_job(true, false, false, true));
        assert!(dqh_twin_check_can_queue_fingerprint_job(false, true, false, true));
        assert!(!dqh_twin_check_can_queue_fingerprint_job(false, false, false, true));
        assert!(!dqh_twin_check_can_queue_fingerprint_job(true, false, true, true));
        assert!(!dqh_twin_check_can_queue_fingerprint_job(true, false, false, false));
        assert!(dqh_twin_check_can_queue_sample_job(true, false, false, true));
        assert!(!dqh_twin_check_can_queue_sample_job(false, false, false, true));
        assert!(!dqh_twin_check_can_queue_sample_job(true, true, false, true));
        assert!(!dqh_twin_check_can_queue_sample_job(true, false, true, true));
        assert!(!dqh_twin_check_can_queue_sample_job(true, false, false, false));
        assert!(dqh_twin_check_should_lookup_kind_by_files(false, true, true));
        assert!(!dqh_twin_check_should_lookup_kind_by_files(true, true, true));
        assert!(!dqh_twin_check_should_lookup_kind_by_files(false, false, true));
        assert!(!dqh_twin_check_should_lookup_kind_by_files(false, true, false));
        assert_eq!(dqh_direct_renamer_classify_rename_target(true, true), 1);
        assert_eq!(dqh_direct_renamer_classify_rename_target(true, false), 0);
        assert_eq!(dqh_direct_renamer_classify_rename_target(false, false), 2);
        assert!(dqh_dl_info_should_update_auto_filename_name(false, true, false));
        assert!(dqh_dl_info_should_update_auto_filename_name(true, false, false));
        assert!(!dqh_dl_info_should_update_auto_filename_name(true, true, false));
        assert!(!dqh_dl_info_should_update_auto_filename_name(false, false, true));
        assert!(dqh_dl_info_should_append_message_to_disk(true, true));
        assert!(!dqh_dl_info_should_append_message_to_disk(true, false));
        assert!(!dqh_dl_info_should_append_message_to_disk(false, true));
        assert_eq!(dqh_stream_repair_classify_build_job_mode(true, 0, 100, 100), 0);
        assert_eq!(dqh_stream_repair_classify_build_job_mode(false, 0, 0, 0), 0);
        assert_eq!(dqh_stream_repair_classify_build_job_mode(false, 0, 100, 0), 1);
        assert_eq!(dqh_stream_repair_classify_build_job_mode(false, 5, 100, 0), 0);
        assert_eq!(dqh_stream_repair_classify_build_job_mode(false, 5, 100, 100), 2);
        assert!(dqh_nzbfile_should_create_segment_article(1));
        assert!(!dqh_nzbfile_should_create_segment_article(0));
        assert!(dqh_dupe_probe_should_stop_early_on_found(false, 2, 2));
        assert!(!dqh_dupe_probe_should_stop_early_on_found(true, 2, 2));
        assert!(!dqh_dupe_probe_should_stop_early_on_found(false, 1, 2));
        assert!(dqh_dupe_probe_should_disconnect_after_probe(true, 0, 2));
        assert!(dqh_dupe_probe_should_disconnect_after_probe(false, 2, 2));
        assert!(!dqh_dupe_probe_should_disconnect_after_probe(false, 0, 2));
        assert!(dqh_url_coord_should_wait_on_cond(false, false));
        assert!(!dqh_url_coord_should_wait_on_cond(true, false));
        assert!(!dqh_url_coord_should_wait_on_cond(false, true));
        assert_eq!(dqh_par_damage_classify_completed_file(true, 1, 100, 5), 1);
        assert_eq!(dqh_par_damage_classify_completed_file(false, 0, 100, 5), 2);
        assert_eq!(dqh_par_damage_classify_completed_file(false, 2, 100, 5), 3);
        assert_eq!(dqh_par_damage_classify_completed_file(false, 2, 0, 0), 4);
        assert_eq!(dqh_par_damage_classify_completed_file(false, 2, 0, 5), 5);
    }
}
