//! Hot-path predicates and calculations for `DupeSearch`, `Newznab`, and `NzbFetcher`.

/// Returns whether a queue notification should schedule a duplicate search
/// (`DupeSearch::DownloadQueueUpdate`).
pub fn dqh_or_dsf_action_matches(action: i32, ea_nzb_added: i32, ea_nzb_named: i32) -> bool {
    action == ea_nzb_added || action == ea_nzb_named
}

pub fn dsf_dupesearch_should_schedule_queue_event(
    action: i32,
    ea_nzb_added: i32,
    ea_nzb_named: i32,
    has_nzb_info: bool,
    dupe_search_opt: bool,
    is_stopped: bool,
) -> bool {
    dqh_or_dsf_action_matches(action, ea_nzb_added, ea_nzb_named)
        && has_nzb_info
        && dupe_search_opt
        && !is_stopped
}

/// Returns whether a history item is a recent failed NZB that should be scanned
/// on startup (`DupeSearch::ScanQueue`).
pub fn dsf_dupesearch_is_recent_failed_history_item(
    kind: i32,
    hk_nzb_val: i32,
    item_time: i64,
    recent_cutoff: i64,
    is_dupe_success: bool,
) -> bool {
    kind == hk_nzb_val && item_time >= recent_cutoff && !is_dupe_success
}

/// Returns whether an item has a valid non-deleted NZB file that can be added to
/// `job.knownFiles` (`DupeSearch::Collect`).
pub fn dsf_dupesearch_can_add_known_file(
    kind: i32,
    nk_nzb_val: i32,
    queued_filename_empty: bool,
    delete_status: i32,
    ds_manual_val: i32,
) -> bool {
    kind == nk_nzb_val && !queued_filename_empty && delete_status != ds_manual_val
}

/// Returns whether a queued item is an eligible rank-only member candidate before
/// checking key/release/filename (`DupeSearch::Collect`).
#[allow(clippy::too_many_arguments)]
pub fn dsf_dupesearch_is_rank_only_queued_member(
    rank_only: bool,
    is_self: bool,
    kind: i32,
    nk_nzb_val: i32,
    dupe_mode: i32,
    dm_score_val: i32,
    has_post_info: bool,
    delete_status: i32,
    ds_none_val: i32,
    ds_dupe_val: i32,
    item_score: i32,
    pick_score: i32,
) -> bool {
    rank_only
        && !is_self
        && kind == nk_nzb_val
        && dupe_mode == dm_score_val
        && !has_post_info
        && (delete_status == ds_none_val || delete_status == ds_dupe_val)
        && item_score < pick_score
}

/// Computes the bounded health-check budget in milliseconds for `DupeSearch::Expand`.
pub fn dsf_dupesearch_calc_expand_budget_ms(
    budget_sec: i32,
    deadline: i64,
    now: i64,
) -> i32 {
    let opt_ms = i64::from(budget_sec).saturating_mul(1000);
    let left_ms = deadline.saturating_sub(now).saturating_mul(1000);
    let chosen = opt_ms.min(left_ms).max(1);
    chosen.clamp(1, i64::from(i32::MAX)) as i32
}

/// Returns whether `DupeSearch::Expand` should run a separate data-only health check
/// to test if PAR2 recovery is doomed.
pub fn dsf_dupesearch_should_check_data_doomed(
    alive: f64,
    data_msg_ids_empty: bool,
    par_index_msg_id_empty: bool,
    recovery_bytes_empty: bool,
) -> bool {
    alive >= 0.85
        && !data_msg_ids_empty
        && !par_index_msg_id_empty
        && !recovery_bytes_empty
}

/// Returns whether the data-only health sample has enough known missing articles to
/// evaluate `DupeIdentity::ParDoomed` (`DupeSearch::Expand`).
pub fn dsf_dupesearch_should_eval_par_doomed(
    data_alive: f64,
    data_missing: i64,
    min_known: i32,
) -> bool {
    data_alive >= 0.0 && data_missing >= i64::from(min_known)
}

/// Returns whether a duplicate search/expansion loop should stop due to shutdown,
/// deadline expiration, or exhausted grab budget (`DupeSearch::Expand`, `DupeSearch::Search`).
pub fn dsf_dupesearch_should_stop_fetch_loop(
    is_stopped: bool,
    now: i64,
    deadline: i64,
    budget_spent: bool,
) -> bool {
    is_stopped || now >= deadline || budget_spent
}

/// Returns whether a fetched posting should be rejected as `other-release`
/// (`DupeSearch::Search`).
pub fn dsf_dupesearch_is_other_release_rejected(
    lenient: bool,
    main_name_readable: bool,
    same_release: bool,
) -> bool {
    !lenient && main_name_readable && !same_release
}

/// Returns whether `DupeSearch::Place` should inspect the queue to reschedule
/// discovery or backup ranking.
pub fn dsf_dupesearch_should_reschedule_retry_or_rank(
    retry_discovery: bool,
    rank_incomplete: bool,
    is_stopped: bool,
) -> bool {
    (retry_discovery || rank_incomplete) && !is_stopped
}

/// Returns whether the current queued pick is eligible for a 15-minute discovery
/// or ranking retry (`DupeSearch::Place`).
pub fn dsf_dupesearch_is_current_pick_eligible_for_retry(
    has_current: bool,
    deleting: bool,
    delete_status: i32,
    ds_none_val: i32,
    has_post_info: bool,
) -> bool {
    has_current && !deleting && delete_status == ds_none_val && !has_post_info
}

/// Returns whether `g_Scanner->AddExternalFile` succeeded with a positive NZB ID
/// (`DupeSearch::AddDonor`).
pub fn dsf_dupesearch_is_add_external_success(
    status: i32,
    as_success_val: i32,
    nzb_id: i32,
) -> bool {
    status == as_success_val && nzb_id > 0
}

/// Resolves the Newznab item size from `attr_size` (preferred when positive) or
/// `element_size` (`Newznab::PageReader::End`).
pub fn dsf_newznab_resolve_item_size(attr_size: i64, element_size: i64) -> i64 {
    if attr_size > 0 {
        attr_size
    } else {
        element_size
    }
}

/// Returns whether a dropped Newznab HTTP request (`!ok && status == 0`) should be
/// retried once (`Newznab::Search`).
pub fn dsf_newznab_should_retry_dropped_fetch(
    reply_ok: bool,
    reply_status: i32,
    http_stopped: bool,
    now: i64,
    deadline: i64,
) -> bool {
    !reply_ok && reply_status == 0 && !http_stopped && now < deadline
}

/// Computes the positive millisecond deadline for an HTTP fetch given the current
/// steady-clock milliseconds and wall-clock seconds remaining (`Newznab::Search`,
/// `NzbFetcher::Fetch`).
pub fn dsf_newznab_calc_page_deadline_ms(now_ms: i64, deadline: i64, now_sec: i64) -> i64 {
    let rem_ms = deadline.saturating_sub(now_sec).saturating_mul(1000);
    now_ms.saturating_add(rem_ms).max(1)
}

/// Returns whether Newznab pagination should stop because the page returned fewer
/// than `page_size` items (`Newznab::Search`).
pub fn dsf_newznab_should_stop_pagination(items_count: usize, page_size: i32) -> bool {
    (items_count as i32) < page_size
}

/// Accumulates one `NzbFetcher::Stats` record into another (`NzbFetcher::Stats::Add`).
#[allow(clippy::too_many_arguments)]
pub fn dsf_nzb_fetcher_accum_stats(
    io_ok: &mut i32,
    io_fetch: &mut i32,
    io_parse: &mut i32,
    io_refused: &mut i32,
    io_relisted: &mut i32,
    io_listing_mismatch: &mut i32,
    io_budget: &mut i32,
    add_ok: i32,
    add_fetch: i32,
    add_parse: i32,
    add_refused: i32,
    add_relisted: i32,
    add_listing_mismatch: i32,
    add_budget: i32,
) {
    *io_ok = io_ok.wrapping_add(add_ok);
    *io_fetch = io_fetch.wrapping_add(add_fetch);
    *io_parse = io_parse.wrapping_add(add_parse);
    *io_refused = io_refused.wrapping_add(add_refused);
    *io_relisted = io_relisted.wrapping_add(add_relisted);
    *io_listing_mismatch = io_listing_mismatch.wrapping_add(add_listing_mismatch);
    *io_budget = io_budget.wrapping_add(add_budget);
}

/// Returns whether an indexer refusal cooldown timestamp is still in the future
/// (`NzbFetcher::Load`, `NzbFetcher::Save`, `NzbFetcher::IsRefused`).
pub fn dsf_nzb_fetcher_is_cooldown_active(until_sec: i64, now_sec: i64) -> bool {
    until_sec > now_sec
}

/// Returns whether `NzbFetcher::Fetch` should pause and retry after a failed
/// attempt (`NzbFetcher::Fetch`).
pub fn dsf_nzb_fetcher_can_pause_before_retry(
    attempt: i32,
    retries: i32,
    now_sec: i64,
    retry_delay_sec: i32,
    deadline_sec: i64,
) -> bool {
    attempt < retries && now_sec.saturating_add(i64::from(retry_delay_sec)) < deadline_sec
}

/// Returns whether an indexer should be placed in cooldown after returning `status`
/// (`NzbFetcher::Fetch`).
pub fn dsf_nzb_fetcher_should_refuse_indexer(status: i32, indexer_empty: bool) -> bool {
    (status == 403 || status == 429) && !indexer_empty
}

/// Parses a TSV cooldown state line (`indexer\tuntil\t...`) for `NzbFetcher::Load`,
/// returning `Some((indexer, when))` when `when > now_sec`.
pub fn dsf_nzb_fetcher_parse_cooldown_line(line: &[u8], now_sec: i64) -> Option<(Vec<u8>, i64)> {
    let tab1 = line.iter().position(|&b| b == b'\t')?;
    let indexer = &line[..tab1];
    let rest = &line[tab1 + 1..];
    let tab2 = rest.iter().position(|&b| b == b'\t')?;
    let until_bytes = &rest[..tab2];
    let s = std::str::from_utf8(until_bytes).ok()?.trim_start();
    let mut len = 0usize;
    for (i, b) in s.bytes().enumerate() {
        if i == 0 && (b == b'+' || b == b'-') {
            len += 1;
        } else if b.is_ascii_digit() {
            len += 1;
        } else {
            break;
        }
    }
    let when: i64 = if len == 0 {
        0
    } else {
        s[..len].parse::<i64>().unwrap_or(0)
    };
    if dsf_nzb_fetcher_is_cooldown_active(when, now_sec) {
        Some((indexer.to_vec(), when))
    } else {
        None
    }
}

/// Formats an active indexer cooldown entry (`indexer\twhen\t\n`) for `NzbFetcher::Save`.
pub fn dsf_nzb_fetcher_format_cooldown_line(indexer: &[u8], when_sec: i64, now_sec: i64) -> Vec<u8> {
    if !dsf_nzb_fetcher_is_cooldown_active(when_sec, now_sec) {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(indexer.len() + 24);
    out.extend_from_slice(indexer);
    out.push(b'\t');
    out.extend_from_slice(when_sec.to_string().as_bytes());
    out.extend_from_slice(b"\t\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dupesearch_helpers() {
        assert!(dsf_dupesearch_should_schedule_queue_event(
            1, 1, 2, true, true, false
        ));
        assert!(dsf_dupesearch_should_schedule_queue_event(
            2, 1, 2, true, true, false
        ));
        assert!(!dsf_dupesearch_should_schedule_queue_event(
            3, 1, 2, true, true, false
        ));
        assert!(!dsf_dupesearch_should_schedule_queue_event(
            1, 1, 2, true, true, true
        ));

        assert!(dsf_dupesearch_is_recent_failed_history_item(
            1, 1, 100, 90, false
        ));
        assert!(!dsf_dupesearch_is_recent_failed_history_item(
            1, 1, 80, 90, false
        ));
        assert!(!dsf_dupesearch_is_recent_failed_history_item(
            1, 1, 100, 90, true
        ));

        assert!(dsf_dupesearch_can_add_known_file(0, 0, false, 0, 3));
        assert!(!dsf_dupesearch_can_add_known_file(0, 0, true, 0, 3));
        assert!(!dsf_dupesearch_can_add_known_file(0, 0, false, 3, 3));

        assert!(dsf_dupesearch_is_rank_only_queued_member(
            true, false, 0, 0, 0, 0, false, 0, 0, 1, 900, 1000
        ));
        assert!(!dsf_dupesearch_is_rank_only_queued_member(
            false, false, 0, 0, 0, 0, false, 0, 0, 1, 900, 1000
        ));

        assert_eq!(dsf_dupesearch_calc_expand_budget_ms(10, 105, 100), 5000);
        assert_eq!(dsf_dupesearch_calc_expand_budget_ms(10, 100, 105), 1);

        assert!(dsf_dupesearch_should_check_data_doomed(
            0.90, false, false, false
        ));
        assert!(!dsf_dupesearch_should_check_data_doomed(
            0.80, false, false, false
        ));

        assert!(dsf_dupesearch_should_eval_par_doomed(0.95, 5, 5));
        assert!(!dsf_dupesearch_should_eval_par_doomed(-1.0, 5, 5));

        assert!(!dsf_dupesearch_should_stop_fetch_loop(false, 99, 100, false));
        assert!(dsf_dupesearch_should_stop_fetch_loop(false, 100, 100, false));
        assert!(dsf_dupesearch_should_stop_fetch_loop(false, 99, 100, true));

        assert!(dsf_dupesearch_is_other_release_rejected(false, true, false));
        assert!(!dsf_dupesearch_is_other_release_rejected(true, true, false));
        assert!(!dsf_dupesearch_is_other_release_rejected(false, true, true));

        assert!(dsf_dupesearch_should_reschedule_retry_or_rank(
            true, false, false
        ));
        assert!(!dsf_dupesearch_should_reschedule_retry_or_rank(
            true, false, true
        ));

        assert!(dsf_dupesearch_is_current_pick_eligible_for_retry(
            true, false, 0, 0, false
        ));
        assert!(!dsf_dupesearch_is_current_pick_eligible_for_retry(
            true, false, 0, 0, true
        ));

        assert!(dsf_dupesearch_is_add_external_success(2, 2, 10));
        assert!(!dsf_dupesearch_is_add_external_success(2, 2, 0));
    }

    #[test]
    fn test_newznab_and_nzb_fetcher_helpers() {
        assert_eq!(dsf_newznab_resolve_item_size(500, 300), 500);
        assert_eq!(dsf_newznab_resolve_item_size(0, 300), 300);

        assert!(dsf_newznab_should_retry_dropped_fetch(
            false, 0, false, 90, 100
        ));
        assert!(!dsf_newznab_should_retry_dropped_fetch(
            false, 500, false, 90, 100
        ));

        assert_eq!(dsf_newznab_calc_page_deadline_ms(1000, 20, 15), 6000);
        assert_eq!(dsf_newznab_calc_page_deadline_ms(1000, 10, 15), 1);

        assert!(dsf_newznab_should_stop_pagination(99, 100));
        assert!(!dsf_newznab_should_stop_pagination(100, 100));

        let (mut ok, mut f, mut p, mut r, mut rl, mut lm, mut b) = (1, 2, 3, 4, 5, 6, 7);
        dsf_nzb_fetcher_accum_stats(
            &mut ok, &mut f, &mut p, &mut r, &mut rl, &mut lm, &mut b, 10, 20, 30, 40, 50, 60, 70,
        );
        assert_eq!((ok, f, p, r, rl, lm, b), (11, 22, 33, 44, 55, 66, 77));

        assert!(dsf_nzb_fetcher_is_cooldown_active(101, 100));
        assert!(!dsf_nzb_fetcher_is_cooldown_active(100, 100));

        assert!(dsf_nzb_fetcher_can_pause_before_retry(0, 1, 90, 5, 100));
        assert!(!dsf_nzb_fetcher_can_pause_before_retry(0, 1, 95, 5, 100));
        assert!(!dsf_nzb_fetcher_can_pause_before_retry(1, 1, 90, 5, 100));

        assert!(dsf_nzb_fetcher_should_refuse_indexer(429, false));
        assert!(dsf_nzb_fetcher_should_refuse_indexer(403, false));
        assert!(!dsf_nzb_fetcher_should_refuse_indexer(500, false));
        assert!(!dsf_nzb_fetcher_should_refuse_indexer(429, true));

        let line = dsf_nzb_fetcher_format_cooldown_line(b"https://idx.example", 250, 100);
        assert_eq!(line, b"https://idx.example\t250\t\n");
        let parsed = dsf_nzb_fetcher_parse_cooldown_line(b"https://idx.example\t250\t", 100);
        assert_eq!(parsed, Some((b"https://idx.example".to_vec(), 250)));
        assert_eq!(
            dsf_nzb_fetcher_parse_cooldown_line(b"https://idx.example\t90\t", 100),
            None
        );
    }
}
