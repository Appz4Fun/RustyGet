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
    }
}
