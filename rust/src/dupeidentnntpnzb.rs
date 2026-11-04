//! Hot-path functions for `DupeIdentity`, `NntpHealthServer`, `NzbReader`, and `Posting`.

/// Decodes the 12-byte inner-identity header from `nzbget_rs_dupe_inner` output
/// (`DupeIdentity::FromArticle`).
/// Returns `Some((bytes, ext_len, name_offset, name_len))`.
pub fn din_dupe_identity_decode_inner_buf(raw: &[u8]) -> Option<(u64, usize, usize, usize)> {
    if raw.len() < 12 {
        return None;
    }
    let mut bytes = 0u64;
    for (i, &b) in raw[0..8].iter().enumerate() {
        bytes |= u64::from(b) << (8 * i);
    }
    let mut ext_size = 0usize;
    for (i, &b) in raw[8..12].iter().enumerate() {
        ext_size |= usize::from(b) << (8 * i);
    }
    if ext_size > 0 && ext_size <= 4 && ext_size <= raw.len() - 12 {
        let name_offset = 12 + ext_size;
        let name_len = raw.len() - name_offset;
        Some((bytes, ext_size, name_offset, name_len))
    } else {
        None
    }
}

/// Returns whether `DupeIdentity::FetchArticle` should abort before starting
/// (`DupeIdentity::FetchArticle`).
pub fn din_dupe_identity_should_abort_fetch_article(
    msg_id_empty: bool,
    now: i64,
    deadline: i64,
    http_stopped: bool,
    health_stopping: bool,
) -> bool {
    msg_id_empty || now >= deadline || http_stopped || health_stopping
}

/// Returns whether `DupeIdentity::FetchArticle` should discard a completed article
/// fetch (`DupeIdentity::FetchArticle`).
pub fn din_dupe_identity_should_discard_fetched_article(
    now: i64,
    deadline: i64,
    http_stopped: bool,
    health_stopping: bool,
    offset: i64,
) -> bool {
    now >= deadline || http_stopped || health_stopping || offset != 0
}

/// Computes the 40% minimum inner file size threshold (`data_bytes * 2 / 5`)
/// without overflow (`DupeIdentity::Read`).
pub fn din_dupe_identity_calc_min_inner_bytes(data_bytes: i64) -> u64 {
    if data_bytes > 0 {
        let d = data_bytes as u64;
        (d / 5) * 2 + (d % 5) * 2 / 5
    } else {
        0
    }
}

/// Returns whether `DupeIdentity::ParDoomed` has valid inputs to fetch and inspect
/// the PAR2 index packet (`DupeIdentity::ParDoomed`).
pub fn din_dupe_identity_can_check_par_doomed(
    par_index_id_empty: bool,
    recovery_bytes_empty: bool,
    data_bytes: i64,
    article_bytes: u64,
) -> bool {
    !par_index_id_empty && !recovery_bytes_empty && data_bytes > 0 && article_bytes != 0
}

/// Returns whether a configured `NewsServer` is eligible for health checks
/// (`NntpHealthServer::Servers`).
pub fn din_nntp_health_is_server_eligible(active: bool, max_connections: i32) -> bool {
    active && max_connections != 0
}

/// Returns whether the cached `NntpHealthServer` entry needs to be created or
/// recreated due to a capacity change (`NntpHealthServer::Servers`).
pub fn din_nntp_health_needs_entry_recreate(
    has_entry: bool,
    current_cap: i32,
    wanted_cap: i32,
) -> bool {
    !has_entry || current_cap != wanted_cap
}

/// Returns whether `NntpHealthServer::Exchange` should continue waiting for a
/// pooled connection (`NntpHealthServer::Exchange`).
pub fn din_nntp_health_should_keep_waiting_for_conn(
    has_conn: bool,
    http_stopped: bool,
    now: i64,
    wait_start: i64,
    wait_sec: i32,
) -> bool {
    !has_conn && !http_stopped && now.saturating_sub(wait_start) <= i64::from(wait_sec)
}

/// Returns whether a `GROUP` command response starts with `'2'`
/// (`NntpHealthServer::Exchange`).
pub fn din_nntp_health_is_join_group_ok(joined: &[u8]) -> bool {
    joined.first() == Some(&b'2')
}

/// Classifies an `ArticleFetcher::FetchedArticle` result in `NntpHealthServer::Exchange`
/// (`(throw_exchange_error, answer_val)`).
pub fn din_nntp_health_classify_body_fetch_result(
    success: bool,
    transient: bool,
    ans_present: i32,
    ans_body_bad: i32,
) -> (bool, i32) {
    if success {
        (false, ans_present)
    } else if transient {
        (true, ans_body_bad)
    } else {
        (false, ans_body_bad)
    }
}

/// Returns whether a gated `Present` STAT answer must be downgraded to `Error`
/// when BODY verification is aborted (`NntpHealthServer::Exchange`).
pub fn din_nntp_health_should_reset_gated_present_to_error(
    has_gate: bool,
    answer: i32,
    ans_present: i32,
) -> bool {
    has_gate && answer == ans_present
}

/// Validates an NZB segment message-id (`ValidMessageId` in `NzbReader.cpp`).
pub fn din_nzb_reader_valid_message_id(id: &[u8]) -> bool {
    if id.is_empty() || id.len() > 250 {
        return false;
    }
    let len = id.len();
    for (i, &ch) in id.iter().enumerate() {
        let edge = i == 0 || i + 1 == len;
        if ch <= b' ' || ch == 0x7f || ((ch == b'<' || ch == b'>') && !edge) {
            return false;
        }
    }
    true
}

/// Returns whether `m_segments[index]` is superseded by a duplicate part number at
/// `index + 1` (`NzbReader.cpp`).
pub fn din_nzb_reader_is_duplicate_segment_number(
    index: usize,
    total: usize,
    cur_num: i64,
    next_num: i64,
) -> bool {
    index + 1 < total && next_num == cur_num
}

/// Updates per-file byte count, `geometryValid`, and whether `segment.bytes` should
/// be pushed to `articleSizes` (`NzbReader.cpp`).
pub fn din_nzb_reader_accum_segment_bytes(
    file_bytes: i64,
    seg_bytes: i64,
    is_par: bool,
    geometry_valid: bool,
) -> (i64, bool, bool) {
    let push_article_size = !is_par && seg_bytes > 0;
    if seg_bytes > 0 && file_bytes <= i64::MAX - seg_bytes {
        (file_bytes + seg_bytes, geometry_valid, push_article_size)
    } else {
        let new_geom = if !is_par { false } else { geometry_valid };
        (file_bytes, new_geom, push_article_size)
    }
}

/// Updates file-level data counters and determines whether parity/data head entries
/// should be recorded (`NzbReader.cpp`).
pub fn din_nzb_reader_accum_file_data_totals(
    is_par: bool,
    file_bytes: i64,
    first_id_empty: bool,
    data_files: i32,
    data_bytes: i64,
    geometry_valid: bool,
) -> (i32, i64, bool, bool, bool) {
    let push_parity_head = is_par && file_bytes > 0 && !first_id_empty;
    if !is_par && file_bytes > 0 {
        let new_files = data_files.wrapping_add(1);
        let (new_bytes, new_geom) = if data_bytes <= i64::MAX - file_bytes {
            (data_bytes + file_bytes, geometry_valid)
        } else {
            (data_bytes, false)
        };
        let push_data_head = !first_id_empty;
        (new_files, new_bytes, new_geom, push_parity_head, push_data_head)
    } else {
        let new_geom = if !is_par { false } else { geometry_valid };
        (data_files, data_bytes, new_geom, push_parity_head, false)
    }
}

/// Computes the 16-byte lowercase hex FNV-1a fingerprint over `messageIds`
/// (`NzbSummary::Fingerprint` in `NzbReader.cpp`).
pub fn din_nzb_reader_fnv1a_fingerprint(ids: &[&[u8]]) -> [u8; 16] {
    const FNV_OFFSET: u64 = 1_469_598_103_934_665_603;
    const FNV_PRIME: u64 = 1_099_511_628_211;
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut hash = FNV_OFFSET;
    for id in ids {
        for &ch in *id {
            hash = (hash ^ u64::from(ch)).wrapping_mul(FNV_PRIME);
        }
        hash = (hash ^ u64::from(b'\n')).wrapping_mul(FNV_PRIME);
    }

    let mut out = [0u8; 16];
    for (i, slot) in out.iter_mut().enumerate() {
        let shift = (15 - i) * 4;
        let nibble = ((hash >> shift) & 0x0f) as usize;
        *slot = HEX[nibble];
    }
    out
}

/// Returns whether a parsed `.par2` filename is the main PAR2 index file
/// (`baseLen + 5 == head.name.size()`, `NzbReader::Parse`).
pub fn din_nzb_reader_is_par_index_length(
    parsed_ok: bool,
    base_len: i32,
    name_len: usize,
) -> bool {
    parsed_ok && base_len >= 0 && (base_len as usize).saturating_add(5) == name_len
}

/// Returns whether a parsed `.par2` filename is a recovery volume file
/// (`baseLen + 5 < head.name.size()`, `NzbReader::Parse`).
pub fn din_nzb_reader_is_par_volume_length(
    parsed_ok: bool,
    base_len: i32,
    name_len: usize,
) -> bool {
    parsed_ok && base_len >= 0 && (base_len as usize).saturating_add(5) < name_len
}

/// Returns the median article size from a sorted slice of article sizes
/// (`NzbReader::Parse`).
pub fn din_nzb_reader_select_median_article_bytes(sorted_sizes: &[u64]) -> u64 {
    if sorted_sizes.is_empty() {
        0
    } else {
        sorted_sizes[sorted_sizes.len() / 2]
    }
}

/// Sums `total_bytes` across `sizes` and selects the index of `mainName`
/// (the largest non-PAR2 file, or the largest file overall when all files are PAR2;
/// `-1` when `sizes` is empty, `NzbReader::Parse` in `NzbReader.cpp`).
pub fn din_nzb_reader_select_main_file(sizes: &[i64], is_par: &[u8]) -> (i64, isize) {
    let n = sizes.len().min(is_par.len());
    let mut total_bytes: i64 = 0;
    let mut main_size: i64 = -1;
    let mut main_idx: isize = -1;
    let mut any_size: i64 = -1;
    let mut any_idx: isize = -1;
    for i in 0..n {
        let sz = sizes[i];
        total_bytes = total_bytes.wrapping_add(sz);
        if sz > any_size {
            any_size = sz;
            any_idx = i as isize;
        }
        if is_par[i] == 0 && sz > main_size {
            main_size = sz;
            main_idx = i as isize;
        }
    }
    let chosen = if main_size >= 0 { main_idx } else { any_idx };
    (total_bytes, chosen)
}

/// Stably sorts head indices by `head_bytes` descending and writes at most 3
/// top head indices into `out_indices` (`NzbReader::Parse` in `NzbReader.cpp`).
pub fn din_nzb_reader_select_top_heads(
    head_bytes: &[i64],
    out_indices: &mut [usize],
) -> usize {
    let mut order: Vec<usize> = (0..head_bytes.len()).collect();
    order.sort_by(|&a, &b| head_bytes[b].cmp(&head_bytes[a]));
    let count = order.len().min(3).min(out_indices.len());
    out_indices[..count].copy_from_slice(&order[..count]);
    count
}

/// Returns whether the `Posting::SketchOfFile` cache exceeds `max_entries` and
/// should be cleared (`Posting::SketchOfFile`).
pub fn din_posting_should_clear_sketch_cache(cache_len: usize, max_entries: usize) -> bool {
    cache_len > max_entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dupe_identity_and_nntp_health() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&4096u64.to_le_bytes());
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(b"mkvMovie.Name");
        assert_eq!(
            din_dupe_identity_decode_inner_buf(&buf),
            Some((4096, 3, 15, 10))
        );
        assert_eq!(din_dupe_identity_decode_inner_buf(&[0u8; 8]), None);

        assert!(din_dupe_identity_should_abort_fetch_article(
            true, 10, 20, false, false
        ));
        assert!(!din_dupe_identity_should_abort_fetch_article(
            false, 10, 20, false, false
        ));

        assert!(din_dupe_identity_should_discard_fetched_article(
            10, 20, false, false, 128
        ));
        assert!(!din_dupe_identity_should_discard_fetched_article(
            10, 20, false, false, 0
        ));

        assert_eq!(din_dupe_identity_calc_min_inner_bytes(100), 40);
        assert_eq!(din_dupe_identity_calc_min_inner_bytes(0), 0);

        assert!(din_dupe_identity_can_check_par_doomed(false, false, 1000, 512));
        assert!(!din_dupe_identity_can_check_par_doomed(true, false, 1000, 512));

        assert!(din_nntp_health_is_server_eligible(true, 4));
        assert!(!din_nntp_health_is_server_eligible(false, 4));
        assert!(!din_nntp_health_is_server_eligible(true, 0));

        assert!(din_nntp_health_needs_entry_recreate(false, 0, 4));
        assert!(din_nntp_health_needs_entry_recreate(true, 2, 4));
        assert!(!din_nntp_health_needs_entry_recreate(true, 4, 4));

        assert!(din_nntp_health_should_keep_waiting_for_conn(
            false, false, 102, 100, 3
        ));
        assert!(!din_nntp_health_should_keep_waiting_for_conn(
            false, false, 104, 100, 3
        ));

        assert!(din_nntp_health_is_join_group_ok(b"211 group selected"));
        assert!(!din_nntp_health_is_join_group_ok(b"411 no such group"));

        assert_eq!(
            din_nntp_health_classify_body_fetch_result(true, false, 1, 4),
            (false, 1)
        );
        assert_eq!(
            din_nntp_health_classify_body_fetch_result(false, true, 1, 4),
            (true, 4)
        );

        assert!(din_nntp_health_should_reset_gated_present_to_error(
            true, 1, 1
        ));
        assert!(!din_nntp_health_should_reset_gated_present_to_error(
            false, 1, 1
        ));
    }

    #[test]
    fn test_nzb_reader_and_posting() {
        assert!(din_nzb_reader_valid_message_id(b"abc@example.com"));
        assert!(din_nzb_reader_valid_message_id(b"<abc@example.com>"));
        assert!(!din_nzb_reader_valid_message_id(b"a<bc@example.com"));
        assert!(!din_nzb_reader_valid_message_id(b""));

        assert!(din_nzb_reader_is_duplicate_segment_number(0, 2, 1, 1));
        assert!(!din_nzb_reader_is_duplicate_segment_number(0, 2, 1, 2));

        assert_eq!(
            din_nzb_reader_accum_segment_bytes(100, 50, false, true),
            (150, true, true)
        );
        assert_eq!(
            din_nzb_reader_accum_segment_bytes(100, 0, false, true),
            (100, false, false)
        );

        assert_eq!(
            din_nzb_reader_accum_file_data_totals(false, 500, false, 2, 1000, true),
            (3, 1500, true, false, true)
        );

        let fp = din_nzb_reader_fnv1a_fingerprint(&[b"msg1@test", b"msg2@test"]);
        assert_eq!(fp.len(), 16);
        assert!(fp.iter().all(|b| b.is_ascii_hexdigit()));

        assert!(din_nzb_reader_is_par_index_length(true, 4, 9));
        assert!(!din_nzb_reader_is_par_index_length(true, 4, 15));
        assert!(din_nzb_reader_is_par_volume_length(true, 4, 15));

        assert_eq!(
            din_nzb_reader_select_median_article_bytes(&[100, 200, 300]),
            200
        );
        assert_eq!(din_nzb_reader_select_median_article_bytes(&[]), 0);

        assert!(din_posting_should_clear_sketch_cache(4097, 4096));
        assert!(!din_posting_should_clear_sketch_cache(4096, 4096));
    }
}
