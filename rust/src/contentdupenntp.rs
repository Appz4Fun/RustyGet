//! ContentMap, DupeSearch, DupeArticleFallback, DupeStreamRepair,
//! StreamRepairController & ArticleFetcher hot-path helpers ported from C++ to Rust.

/// `DiskContentSource::Read` / `CompositeSource::Read` / `DonorMemberSource::Read`:
/// Returns true when `[offset, offset + size)` is a valid non-negative subrange
/// within `total_size`.
pub fn content_source_range_valid(offset: i64, size: i64, total_size: i64) -> bool {
    offset >= 0 && size >= 0 && offset.saturating_add(size) <= total_size
}

/// `ContentMap::ReadInnerDecrypted`:
/// Computes the aligned cipher block range `[cipher_from, cipher_to)` covering
/// `[inner_offset, inner_offset + inner_size)` and validates it against `inner_size_total`
/// and `cipher_stream_size`.
/// Returns `(valid, empty, cipher_from, cipher_to)`.
pub fn content_map_calc_decrypt_cipher_bounds(
    inner_offset: i64,
    inner_size: i64,
    inner_size_total: i64,
    block_size: i64,
    cipher_stream_size: i64,
) -> (bool, bool, i64, i64) {
    if inner_offset < 0
        || inner_size < 0
        || block_size <= 0
        || inner_offset.saturating_add(inner_size) > inner_size_total
    {
        return (false, false, 0, 0);
    }
    if inner_size == 0 {
        return (true, true, 0, 0);
    }
    let end = inner_offset + inner_size;
    let cipher_from = (inner_offset / block_size) * block_size;
    let cipher_to = ((end + block_size - 1) / block_size) * block_size;
    if cipher_to > cipher_stream_size {
        return (false, false, cipher_from, cipher_to);
    }
    (true, false, cipher_from, cipher_to)
}

/// `ContentMapper::IsCompressibleArchive`:
/// Returns true when `format` is `mfRar`, `mfZip`, or `mfSevenZip`.
pub fn content_mapper_is_compressible_archive(
    format: i32,
    mf_rar: i32,
    mf_zip: i32,
    mf_seven_zip: i32,
) -> bool {
    format == mf_rar || format == mf_zip || format == mf_seven_zip
}

/// `ContentMapper::BuildRarMap`:
/// Validates that the total encrypted packed bytes across volumes fit store-mode
/// AES-CBC geometry (`inner_size > 0`, `cipher_total >= inner_size`,
/// `cipher_total - inner_size < 16`, and `cipher_total % 16 == 0`).
pub fn content_mapper_rar_cipher_geometry_valid(inner_size: i64, cipher_total: i64) -> bool {
    inner_size > 0
        && cipher_total >= inner_size
        && cipher_total - inner_size < 16
        && cipher_total % 16 == 0
}

/// `ContentMapper::BuildRarMap`:
/// Computes the plaintext chunk covered by an encrypted store-mode volume slab:
/// `min(packed_size, inner_size - cipher_base)`.
pub fn content_mapper_rar_calc_plain_chunk(
    packed_size: i64,
    inner_size: i64,
    cipher_base: i64,
) -> i64 {
    packed_size.min(inner_size.saturating_sub(cipher_base))
}

/// `ContentMapper::BuildRarMap`:
/// Validates store-mode packed size sum against `inner_size` when at most one
/// volume had unreadable headers (`unknown_volume >= 0`).
/// Returns `(0 = ok, 1 = packed sizes exceed inner file size, 2 = packed sizes do not sum to inner file size)`
/// and the inferred packed size for `unknown_volume`.
pub fn content_mapper_rar_infer_unknown_packed(
    unknown_volume: i32,
    inner_size: i64,
    known_packed: i64,
) -> (i32, i64) {
    if unknown_volume >= 0 {
        let inferred = inner_size.saturating_sub(known_packed);
        if inferred < 0 {
            return (1, inferred);
        }
        (0, inferred)
    } else if known_packed != inner_size {
        (2, 0)
    } else {
        (0, 0)
    }
}

/// `DupeSearch::Search`:
/// Evaluates whether an indexer search learned nothing and should release the key
/// so it can be searched again:
/// - `0`: keep search stamp
/// - `1`: `"no indexer answered"` (`allFailed`)
/// - `2`: `"an indexer query failed and nothing came back"` (`queriesFailed`)
/// - `3`: `"no posting could be fetched"` (`nothingFetched`)
pub fn dupesearch_classify_search_retry_reason(
    queries: i32,
    failed: i32,
    verified_empty: bool,
    fetched_empty: bool,
    refused: i32,
    fetch_err: i32,
    not_tried: i32,
) -> i32 {
    let all_failed = queries > 0 && failed == queries;
    let queries_failed = failed > 0 && verified_empty;
    let nothing_fetched =
        fetched_empty && refused.saturating_add(fetch_err).saturating_add(not_tried) > 0;
    if !(queries_failed || nothing_fetched) {
        0
    } else if all_failed {
        1
    } else if queries_failed {
        2
    } else {
        3
    }
}

/// `DupeSearch::Place`:
/// Computes the number of fast-placed donors: `min(dupe_fast_donors, live_count)`
/// clamped to `cap` when `cap > 0`.
pub fn dupesearch_calc_fast_donor_count(
    dupe_fast_donors: i32,
    live_count: usize,
    cap: i32,
) -> usize {
    let fast_opt = dupe_fast_donors.max(0) as usize;
    let mut fast = fast_opt.min(live_count);
    if cap > 0 {
        fast = fast.min(cap as usize);
    }
    fast
}

/// `DupeSearch::Place` / `DupeSearch::AddDonor`:
/// Converts a `[0.0, 1.0]` alive fraction to rounded percentage, or `-1` when `alive < 0`.
pub fn dupesearch_alive_to_percent(alive: f64) -> i32 {
    if alive < 0.0 {
        -1
    } else {
        (100.0 * alive).round() as i32
    }
}

/// `DupeSearch::Place`:
/// Computes the `AliveParam` percentage for a rescored fast donor:
/// `0` if `is_dead`, `100` if `alive < 0`, otherwise `lround(100 * alive)`.
pub fn dupesearch_fast_donor_alive_param(is_dead: bool, alive: f64) -> i32 {
    if is_dead {
        0
    } else if alive < 0.0 {
        100
    } else {
        (100.0 * alive).round() as i32
    }
}

/// `DupeCoordinator::NzbFound`:
/// Checks whether two NZBs share the same non-zero full or filtered content hash.
pub fn dupecoord_same_content_hash(
    full1: u32,
    filtered1: u32,
    full2: u32,
    filtered2: u32,
) -> bool {
    (full1 > 0 && full1 == full2) || (filtered1 > 0 && filtered1 == filtered2)
}

/// `DupeCoordinator::NzbFound`:
/// Checks whether a queued duplicate that failover returned (`dhRedownloadAuto`)
/// is actively downloading in good health and should be kept ahead of a newly
/// arriving duplicate.
pub fn dupecoord_should_keep_returned_dupe(
    health_check: i32,
    hc_dupe: i32,
    dupe_hint: i32,
    dh_redownload_auto: i32,
    current_success_articles: i32,
    health: i32,
    critical_health: i32,
) -> bool {
    health_check == hc_dupe
        && dupe_hint == dh_redownload_auto
        && current_success_articles > 0
        && health >= critical_health
}

/// `TwinCheckJob::Sample`:
/// Evaluates the outcome of sampling `compared` articles with `differ` mismatches
/// against `sample_count`:
/// - `0`: inconclusive (too few arrived and no mismatch, or stopped -> retry later)
/// - `1`: `"twin"` (`differ == 0` and `compared >= sample_count / 2`)
/// - `2`: `"alt"` (`differ > 0`)
pub fn twin_check_eval_sample_verdict(
    is_stopped: bool,
    compared: i32,
    differ: i32,
    sample_count: i32,
) -> i32 {
    if is_stopped || (compared < sample_count / 2 && differ == 0) {
        0
    } else if differ > 0 {
        2
    } else {
        1
    }
}

/// `ArticleFetcher::FetchFromConnection`:
/// Validates a completed yEnc decode in `ArticleFetcher` and computes
/// `(valid, offset, file_size, success)`.
pub fn article_fetcher_eval_decoded_yenc(
    check_status: i32,
    ds_finished: i32,
    format: i32,
    ef_yenc: i32,
    begin_pos: i64,
    end_pos: i64,
    size: i64,
    data_len: usize,
) -> (bool, i64, i64, bool) {
    if check_status != ds_finished
        || format != ef_yenc
        || begin_pos <= 0
        || end_pos <= 0
        || data_len == 0
    {
        return (false, 0, 0, false);
    }
    let offset = begin_pos - 1;
    let success = (data_len as i64) == end_pos.saturating_sub(begin_pos).saturating_add(1);
    (true, offset, size, success)
}

/// `ArticleBatchFetcher::ClaimNext`:
/// Checks whether a worker can claim the next request in the batch window.
pub fn article_batch_can_claim_next(
    batch_id: i32,
    cur_batch_id: i32,
    cancelled: bool,
    stopped: bool,
    next_claim: usize,
    requests_len: usize,
    next_deliver: usize,
    window_parts: i32,
    buffered_bytes: i64,
    max_buffered_bytes: i64,
) -> bool {
    batch_id == cur_batch_id
        && !cancelled
        && !stopped
        && next_claim < requests_len
        && next_claim.saturating_sub(next_deliver) < (window_parts.max(0) as usize)
        && buffered_bytes < max_buffered_bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_map_and_rar_helpers() {
        assert!(content_source_range_valid(0, 100, 100));
        assert!(!content_source_range_valid(-1, 10, 100));
        assert!(!content_source_range_valid(90, 20, 100));

        let (valid, empty, from, to) =
            content_map_calc_decrypt_cipher_bounds(17, 30, 100, 16, 112);
        assert!(valid && !empty);
        assert_eq!((from, to), (16, 48));

        assert!(content_mapper_is_compressible_archive(2, 2, 3, 4));
        assert!(!content_mapper_is_compressible_archive(0, 2, 3, 4));

        assert!(content_mapper_rar_cipher_geometry_valid(100, 112));
        assert!(!content_mapper_rar_cipher_geometry_valid(100, 128));
        assert_eq!(content_mapper_rar_calc_plain_chunk(64, 100, 48), 52);

        assert_eq!(content_mapper_rar_infer_unknown_packed(1, 1000, 600), (0, 400));
        assert_eq!(content_mapper_rar_infer_unknown_packed(1, 500, 600).0, 1);
        assert_eq!(content_mapper_rar_infer_unknown_packed(-1, 500, 400).0, 2);
    }

    #[test]
    fn test_dupesearch_and_fallback_helpers() {
        assert_eq!(
            dupesearch_classify_search_retry_reason(2, 2, true, true, 0, 0, 0),
            1
        );
        assert_eq!(
            dupesearch_classify_search_retry_reason(2, 1, true, false, 0, 0, 0),
            2
        );
        assert_eq!(
            dupesearch_classify_search_retry_reason(2, 0, true, true, 1, 0, 0),
            3
        );
        assert_eq!(
            dupesearch_classify_search_retry_reason(2, 0, false, false, 0, 0, 0),
            0
        );

        assert_eq!(dupesearch_calc_fast_donor_count(3, 5, 2), 2);
        assert_eq!(dupesearch_alive_to_percent(-1.0), -1);
        assert_eq!(dupesearch_alive_to_percent(0.956), 96);
        assert_eq!(dupesearch_fast_donor_alive_param(true, 0.9), 0);
        assert_eq!(dupesearch_fast_donor_alive_param(false, -1.0), 100);
    }

    #[test]
    fn test_stream_repair_and_fetcher_helpers() {
        assert_eq!(twin_check_eval_sample_verdict(false, 4, 0, 8), 1);
        assert_eq!(twin_check_eval_sample_verdict(false, 1, 1, 8), 2);
        assert_eq!(twin_check_eval_sample_verdict(false, 2, 0, 8), 0);

        let (valid, off, sz, ok) =
            article_fetcher_eval_decoded_yenc(1, 1, 1, 1, 101, 200, 1000, 100);
        assert!(valid && ok && off == 100 && sz == 1000);

        assert!(article_batch_can_claim_next(
            5, 5, false, false, 2, 10, 0, 4, 1024, 1048576
        ));
        assert!(!article_batch_can_claim_next(
            5, 5, false, false, 4, 10, 0, 4, 1024, 1048576
        ));
    }
}
