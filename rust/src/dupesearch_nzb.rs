//! Terminal unquoted filename parsing and PAR classification for duplicate search.

fn trim_range(bytes: &[u8], mut start: usize, mut end: usize) -> (usize, usize) {
    let trim = |b| matches!(b, b' ' | b'\t' | b'\r' | b'\n');
    while start < end && trim(bytes[start]) {
        start += 1;
    }
    while end > start && trim(bytes[end - 1]) {
        end -= 1;
    }
    (start, end)
}

fn regex_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)
}

/// Strip a terminal yEnc decoration after C++ has handled the first quoted filename.
/// Return a borrowed range; quoting inside this input is left untouched.
pub fn unquoted_subject_range(subject: &[u8]) -> (usize, usize) {
    let mut end = subject.len();
    while end > 0 && regex_space(subject[end - 1]) {
        end -= 1;
    }
    // Match the optional parenthesized part count backwards, without allocating.
    let mut marker_end = end;
    if end > 0 && subject[end - 1] == b')' {
        let mut pos = end - 1;
        while pos > 0 && subject[pos - 1].is_ascii_digit() {
            pos -= 1;
        }
        if pos < end - 1 && pos > 0 && subject[pos - 1] == b'/' {
            let slash = pos - 1;
            pos = slash;
            while pos > 0 && subject[pos - 1].is_ascii_digit() {
                pos -= 1;
            }
            if pos < slash && pos > 0 && subject[pos - 1] == b'(' {
                pos -= 1;
                let space_end = pos;
                while pos > 0 && regex_space(subject[pos - 1]) {
                    pos -= 1;
                }
                if pos < space_end {
                    marker_end = pos;
                }
            }
        }
    }
    if marker_end >= 4 && subject[marker_end - 4..marker_end].eq_ignore_ascii_case(b"yEnc") {
        let mut start = marker_end - 4;
        let space_end = start;
        while start > 0 && regex_space(subject[start - 1]) {
            start -= 1;
        }
        if start < space_end {
            return trim_range(subject, 0, start);
        }
    }
    trim_range(subject, 0, subject.len())
}

/// Checks whether `name` ends with `.par2` (ASCII case-insensitively).
pub fn is_par_name(name: &[u8]) -> bool {
    name.len() >= 5 && name[name.len() - 5..].eq_ignore_ascii_case(b".par2")
}

/// Extracts the file name of a subject (`NameOfSubject` in `NzbReader.cpp`):
/// the first quoted non-empty string (trimmed of ASCII whitespace), else
/// `unquoted_subject_range(subject)`.
pub fn name_of_subject_range(subject: &[u8]) -> (usize, usize) {
    if let Some(open) = subject.iter().position(|&b| b == b'"') {
        if let Some(rel_close) = subject[open + 1..].iter().position(|&b| b == b'"') {
            let close = open + 1 + rel_close;
            if close > open + 1 {
                return trim_range(subject, open + 1, close);
            }
        }
    }
    unquoted_subject_range(subject)
}

/// Validates an NZB segment message-id (`ValidMessageId` in `NzbReader.cpp`).
pub fn valid_message_id(id: &[u8]) -> bool {
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

/// Updates per-file byte count, `geometryValid`, and whether `segment.bytes` should
/// be pushed to `articleSizes` (`NzbReader.cpp`).
pub fn accum_segment_bytes(
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
pub fn accum_file_data_totals(
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
        (
            new_files,
            new_bytes,
            new_geom,
            push_parity_head,
            push_data_head,
        )
    } else {
        let new_geom = if !is_par { false } else { geometry_valid };
        (data_files, data_bytes, new_geom, push_parity_head, false)
    }
}

/// Computes the 16-byte lowercase hex FNV-1a fingerprint over `messageIds`
/// (`NzbSummary::Fingerprint` in `NzbReader.cpp`).
pub fn fnv1a_fingerprint(ids: &[&[u8]]) -> [u8; 16] {
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

/// Sums `total_bytes` across `sizes` and selects the index of `mainName`
/// (`NzbReader::Parse` in `NzbReader.cpp`).
pub fn select_main_file(sizes: &[i64], is_par: &[u8]) -> (i64, isize) {
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
pub fn select_top_heads(head_bytes: &[i64], out_indices: &mut [usize]) -> usize {
    let mut order: Vec<usize> = (0..head_bytes.len()).collect();
    order.sort_by(|&a, &b| head_bytes[b].cmp(&head_bytes[a]));
    let count = order.len().min(3).min(out_indices.len());
    out_indices[..count].copy_from_slice(&order[..count]);
    count
}

/// Parses a TSV cooldown state line (`indexer\tuntil\t...`) for `NzbFetcher::Load`.
pub fn parse_cooldown_line(line: &[u8], now_sec: i64) -> Option<(Vec<u8>, i64)> {
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
    if when > now_sec {
        Some((indexer.to_vec(), when))
    } else {
        None
    }
}

/// Formats an active indexer cooldown entry (`indexer\twhen\t\n`) for `NzbFetcher::Save`.
pub fn format_cooldown_line(indexer: &[u8], when_sec: i64, now_sec: i64) -> Vec<u8> {
    if when_sec <= now_sec {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(indexer.len() + 24);
    out.extend_from_slice(indexer);
    out.push(b'\t');
    out.extend_from_slice(when_sec.to_string().as_bytes());
    out.extend_from_slice(b"\t\n");
    out
}

/// `DupeSearch::Search`:
/// Evaluates whether an indexer search learned nothing and should release the key.
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

/// In `DupeSearch::Place`, evaluates `(retry_wanted, retry_discovery, rank_incomplete)` after placement.
#[inline]
pub fn dupesearch_eval_post_place_retry(
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

/// In `DupeSearch::ResumePending`, classifies the persisted `phase` file contents into `(fetched, ranking)`.
#[inline]
pub fn dupesearch_classify_pending_phase(phase: &[u8]) -> (bool, bool) {
    (phase == b"fetched\n", phase == b"ranking\n")
}

/// Returns whether a queued item is an eligible rank-only member candidate before
/// checking key/release/filename (`DupeSearch::Collect`).
#[allow(clippy::too_many_arguments)]
pub fn dupesearch_is_rank_only_queued_member(
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
pub fn dupesearch_calc_expand_budget_ms(budget_sec: i32, deadline: i64, now: i64) -> i32 {
    let opt_ms = i64::from(budget_sec).saturating_mul(1000);
    let left_ms = deadline.saturating_sub(now).saturating_mul(1000);
    let chosen = opt_ms.min(left_ms).max(1);
    chosen.clamp(1, i64::from(i32::MAX)) as i32
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsPendingMetaView {
    pub valid: i32,
    pub title_offset: usize,
    pub title_len: usize,
    pub indexer_offset: usize,
    pub indexer_len: usize,
    pub grabs: i32,
    pub size: i64,
    pub date: i64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsDupeSearchStateLine {
    pub valid: i32,
    pub within_ttl: i32,
    pub key_offset: usize,
    pub key_len: usize,
    pub score: i32,
    pub time: i64,
    pub title_offset: usize,
    pub title_len: usize,
    pub retryable: i32,
}

fn c_atoi_bytes(bytes: &[u8]) -> i32 {
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return 0;
    }
    let mut neg = false;
    if bytes[i] == b'-' {
        neg = true;
        i += 1;
    } else if bytes[i] == b'+' {
        i += 1;
    }
    let mut acc: i64 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        acc = acc
            .saturating_mul(10)
            .saturating_add((bytes[i] - b'0') as i64);
        i += 1;
    }
    if neg {
        (-acc).clamp(i32::MIN as i64, i32::MAX as i64) as i32
    } else {
        acc.clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }
}

fn c_atoll_bytes(bytes: &[u8]) -> i64 {
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return 0;
    }
    let mut neg = false;
    if bytes[i] == b'-' {
        neg = true;
        i += 1;
    } else if bytes[i] == b'+' {
        i += 1;
    }
    let mut acc: i128 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        acc = acc
            .saturating_mul(10)
            .saturating_add((bytes[i] - b'0') as i128);
        i += 1;
    }
    if neg {
        (-acc).clamp(i64::MIN as i128, i64::MAX as i128) as i64
    } else {
        acc.clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }
}

// ============================================================================
// 1. DupeSearch (`daemon/dupesearch/DupeSearch.cpp`)
// ============================================================================

/// `LowerKey` in `DupeSearch.cpp`: lowercases ASCII and replaces `\t`, `\n`, `\r` with `' '`.
pub fn lower_key(key: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(key.len());
    for &b in key {
        let mut ch = b.to_ascii_lowercase();
        if ch == b'\t' || ch == b'\n' || ch == b'\r' {
            ch = b' ';
        }
        out.push(ch);
    }
    out
}

/// `SavedHistoryBackup` in `DupeSearch.cpp`.
pub fn saved_history_backup(
    has_pick: bool,
    pick_full_hash: u32,
    candidate_delete_status: i32,
    candidate_full_hash: u32,
    ds_dupe: i32,
    ds_copy: i32,
) -> bool {
    candidate_delete_status == ds_dupe
        || (has_pick && candidate_delete_status == ds_copy && candidate_full_hash != pick_full_hash)
}

/// `PlacementAvailable` in `DupeSearch.cpp`.
#[allow(clippy::too_many_arguments)]
pub fn placement_available(
    has_backup: bool,
    is_nzb_kind: bool,
    deleting: bool,
    delete_status: i32,
    ds_none: i32,
    ds_manual: i32,
    ds_dupe: i32,
    ds_copy: i32,
    mark_status: i32,
    ks_bad: i32,
    dupe_mode: i32,
    dm_force: i32,
    in_history: bool,
    whole_backup: bool,
    has_post_info: bool,
    is_dupe_success: bool,
    has_pick: bool,
    pick_full_hash: u32,
    backup_full_hash: u32,
    queued_file_exists: bool,
    health: i32,
    critical_health: i32,
    health_check_is_dupe: bool,
    has_marked_alive: bool,
    marked_alive_val: i32,
) -> bool {
    if !has_backup
        || !is_nzb_kind
        || deleting
        || delete_status == ds_manual
        || mark_status == ks_bad
        || dupe_mode == dm_force
    {
        return false;
    }
    if !in_history && delete_status == ds_none && (!whole_backup || !has_post_info) {
        return true;
    }
    if in_history && is_dupe_success {
        return true;
    }
    (!whole_backup || !has_post_info)
        && saved_history_backup(
            has_pick,
            pick_full_hash,
            delete_status,
            backup_full_hash,
            ds_dupe,
            ds_copy,
        )
        && queued_file_exists
        && (!whole_backup || health >= critical_health)
        && !(health_check_is_dupe && has_marked_alive && marked_alive_val == 0)
}

/// `DupeSearch::Place` initial verified-postings sort order:
/// closest `total_bytes` to `pick_bytes` first, then `grabs` desc, then `date` desc.
pub fn sort_verified_postings(
    total_bytes: &[i64],
    grabs: &[i32],
    dates: &[i64],
    pick_bytes: i64,
    out_order: &mut [usize],
) {
    let n = total_bytes
        .len()
        .min(grabs.len())
        .min(dates.len())
        .min(out_order.len());
    for (idx, slot) in out_order.iter_mut().enumerate().take(n) {
        *slot = idx;
    }
    let dist = |b: i64| -> u64 { b.abs_diff(pick_bytes) };
    out_order[..n].sort_by(|&a, &b| {
        dist(total_bytes[a])
            .cmp(&dist(total_bytes[b]))
            .then_with(|| grabs[b].cmp(&grabs[a]))
            .then_with(|| dates[b].cmp(&dates[a]))
    });
}

/// `DupeSearch::Place` member rerank score calculator.
/// Returns `(final_score, health_percent, add_alive_zero_param)`.
pub fn eval_member_rerank(
    base: i32,
    wanted_score: i32,
    dead_score: i32,
    old_score: i32,
    alive: f64,
    all_dead: bool,
) -> (i32, i32, bool) {
    let mut score = base.saturating_add(wanted_score);
    let percent = (100.0 * alive).round() as i32;
    let add_alive_zero = wanted_score == dead_score;
    if (percent < 100 && score > old_score) || all_dead {
        score = old_score;
    }
    (score, percent, add_alive_zero)
}

/// `DupeSearch::PickGone` reason code evaluator:
/// - `0`: ok (not gone)
/// - `1`: `"the pick was deleted"`
/// - `2`: `"the duplicate key of the pick changed"`
/// - `3`: `"the name of the pick changed"`
#[allow(clippy::too_many_arguments)]
pub fn eval_pick_gone(
    has_pick: bool,
    deleting: bool,
    delete_status: i32,
    ds_manual: i32,
    pick_eff_key: &[u8],
    job_dupe_key: &[u8],
    pick_norm_name: &[u8],
    job_norm_name: &[u8],
) -> i32 {
    if !has_pick || deleting || delete_status == ds_manual {
        return 1;
    }
    if lower_key(pick_eff_key) != lower_key(job_dupe_key) {
        return 2;
    }
    if pick_norm_name != job_norm_name {
        return 3;
    }
    0
}

/// `DupeSearch::SavePending` `.meta` serializer (`title\tindexer\tgrabs\tsize\tdate\t\n`).
pub fn format_pending_meta(
    title: &[u8],
    indexer: &[u8],
    grabs: i32,
    size: i64,
    date: i64,
) -> Vec<u8> {
    let tail = format!("\t{}\t{}\t{}\t\n", grabs, size, date);
    let mut out = Vec::with_capacity(title.len() + 1 + indexer.len() + tail.len());
    out.extend_from_slice(title);
    out.push(b'\t');
    out.extend_from_slice(indexer);
    out.extend_from_slice(tail.as_bytes());
    out
}

/// `DupeSearch::ResumePending` `.meta` parser.
pub fn parse_pending_meta(meta: &[u8]) -> RsPendingMetaView {
    let mut fields: [(usize, usize); 5] = [(0, 0); 5];
    let mut start = 0;
    for slot in &mut fields {
        if start > meta.len() {
            break;
        }
        if let Some(rel) = meta[start..].iter().position(|&b| b == b'\t') {
            *slot = (start, rel);
            start = start + rel + 1;
        } else {
            let rem = meta.len() - start;
            *slot = (start, rem);
            start = meta.len() + 1;
        }
    }
    let grabs = c_atoi_bytes(&meta[fields[2].0..fields[2].0 + fields[2].1]);
    let size = c_atoll_bytes(&meta[fields[3].0..fields[3].0 + fields[3].1]);
    let date = c_atoll_bytes(&meta[fields[4].0..fields[4].0 + fields[4].1]);
    RsPendingMetaView {
        valid: 1,
        title_offset: fields[0].0,
        title_len: fields[0].1,
        indexer_offset: fields[1].0,
        indexer_len: fields[1].1,
        grabs,
        size,
        date,
    }
}

/// `DupeSearch::LoadState` TSV line parser (`key\tscore\ttime\ttitle\tretry\t`).
pub fn parse_dupesearch_state_line(
    line: &[u8],
    now: i64,
    state_ttl_sec: i64,
) -> RsDupeSearchStateLine {
    let next_tab_field = |start: usize| -> Option<(usize, usize, usize)> {
        if start > line.len() {
            return None;
        }
        if let Some(rel) = line[start..].iter().position(|&b| b == b'\t') {
            Some((start, rel, start + rel + 1))
        } else if start < line.len() {
            Some((start, line.len() - start, line.len() + 1))
        } else {
            None
        }
    };

    let Some((k_off, k_len, pos1)) = next_tab_field(0) else {
        return RsDupeSearchStateLine::default();
    };
    let Some((s_off, s_len, pos2)) = next_tab_field(pos1) else {
        return RsDupeSearchStateLine::default();
    };
    let Some((t_off, t_len, pos3)) = next_tab_field(pos2) else {
        return RsDupeSearchStateLine::default();
    };
    let (title_off, title_len, pos4) =
        next_tab_field(pos3).unwrap_or((line.len(), 0, line.len() + 1));
    let (retry_off, retry_len, _) = next_tab_field(pos4).unwrap_or((line.len(), 0, line.len() + 1));

    let score = c_atoi_bytes(&line[s_off..s_off + s_len]);
    let time = c_atoll_bytes(&line[t_off..t_off + t_len]);
    let retry_slice = &line[retry_off..retry_off + retry_len];
    let retryable = retry_slice == b"1" || title_len == 0;
    let within_ttl = now - time < state_ttl_sec;

    RsDupeSearchStateLine {
        valid: 1,
        within_ttl: if within_ttl { 1 } else { 0 },
        key_offset: k_off,
        key_len: k_len,
        score,
        time,
        title_offset: title_off,
        title_len,
        retryable: if retryable { 1 } else { 0 },
    }
}

// ============================================================================
// 2. Fleet (`daemon/dupesearch/Fleet.cpp`)
// ============================================================================

/// `Fleet::Append` per-candidate health evaluation.
/// Returns `(alive_pct, dead, member_complete)`.
pub fn eval_fleet_member_health(
    checked: bool,
    result_complete: bool,
    present: i32,
    missing: i32,
    error: i32,
    min_known: i32,
    min_alive_pct: i32,
) -> (i32, bool, bool) {
    let answered = present.saturating_add(missing).saturating_add(error);
    let unknown = !checked || (!result_complete && present == 0);
    let alive = if unknown || answered < min_known {
        -1.0
    } else {
        present as f64 / answered as f64
    };
    let alive_pct = if alive < 0.0 {
        -1
    } else {
        (100.0 * alive).round() as i32
    };
    let dead_probe = present == 0 && missing >= min_known;
    let dead = !unknown
        && (dead_probe
            || (alive >= 0.0 && alive * 100.0 < min_alive_pct as f64 && missing >= min_known));
    let member_complete = checked && answered >= min_known;
    (alive_pct, dead, member_complete)
}

/// `Fleet::Append` candidate ranking and copy interleaving.
/// Usable members come before unknown, dead and broken ones; among them TWINs
/// go first, then borrowable ALTs, ALTs of unknown packaging and unborrowable
/// ALTs (`kind`, a [`crate::donorkind::DonorKind`] as `u8`), each kind ordered
/// by health and then by closeness to the typical size.
/// Returns number of ranked indices written to `out_ranked`.
pub fn rank_fleet_candidates(
    has_error: &[u8],
    same_as: &[i32],
    dead: &[u8],
    alive: &[i32],
    total_bytes: &[i64],
    kind: &[u8],
    out_ranked: &mut [usize],
) -> usize {
    let n = has_error
        .len()
        .min(same_as.len())
        .min(dead.len())
        .min(alive.len())
        .min(total_bytes.len())
        .min(kind.len());
    let mut sizes: Vec<i64> = Vec::with_capacity(n);
    for i in 0..n {
        if has_error[i] == 0 {
            sizes.push(total_bytes[i]);
        }
    }
    sizes.sort_unstable();
    let typical = if sizes.is_empty() {
        0
    } else {
        sizes[sizes.len() / 2]
    };

    let tier = |i: usize| -> i32 {
        if has_error[i] != 0 {
            3
        } else if dead[i] != 0 {
            2
        } else if alive[i] < 0 {
            1
        } else {
            0
        }
    };
    let distance = |i: usize| -> u64 { total_bytes[i].abs_diff(typical) };

    let mut order: Vec<usize> = (0..n).filter(|&i| same_as[i] < 0).collect();
    order.sort_by(|&a, &b| {
        tier(a)
            .cmp(&tier(b))
            .then_with(|| kind[a].cmp(&kind[b]))
            .then_with(|| alive[b].cmp(&alive[a]))
            .then_with(|| distance(a).cmp(&distance(b)))
    });

    let mut written = 0;
    for idx in order {
        if written < out_ranked.len() {
            out_ranked[written] = idx;
            written += 1;
        }
        for copy_idx in 0..n {
            if same_as[copy_idx] == idx as i32 && written < out_ranked.len() {
                out_ranked[written] = copy_idx;
                written += 1;
            }
        }
    }
    written
}

/// `Fleet::Append` final reason code when `result.chosen == 0 && result.reason.empty()`:
/// - `0`: `"NO_USABLE_MEMBERS"`
/// - `1`: `"INCOMPLETE"`
/// - `2`: `"ALL_DEAD"`
/// - `3`: `"ALREADY_DOWNLOADED"`
/// - `4`: `"NOT_QUEUED"`
pub fn eval_fleet_outcome_reason(
    any_usable: bool,
    any_alive: bool,
    any_unknown: bool,
    on_disk: bool,
) -> i32 {
    if !any_usable {
        0
    } else if !any_alive {
        if any_unknown {
            1
        } else {
            2
        }
    } else if on_disk {
        3
    } else {
        4
    }
}

// ============================================================================
// 3. DonorHealth (`daemon/dupesearch/DonorHealth.cpp`)
// ============================================================================

/// `DonorHealth::Health::Alive`.
pub fn donor_health_alive(present: i32, missing: i32, error: i32, min_known: i32) -> f64 {
    let answered = present.saturating_add(missing).saturating_add(error);
    if answered >= min_known && answered > 0 {
        present as f64 / answered as f64
    } else {
        -1.0
    }
}

/// `DonorHealth::DeadProbe`.
pub fn donor_health_dead_probe(present: i32, missing: i32, min_known: i32) -> bool {
    present == 0 && missing >= min_known
}

/// `DonorHealth::Sample` sample count computation.
pub fn donor_sample_count(total_ids: usize, percent: i32, minimum: i32, maximum: i32) -> usize {
    let wanted = ((total_ids as f64) * (percent as f64) / 100.0).ceil() as usize;
    let min_u = minimum.max(0) as usize;
    let max_u = maximum.max(0) as usize;
    total_ids.min(min_u.max(max_u.min(wanted)))
}

/// `DonorHealth::Server::Ask` pipeline depth adaptation.
pub fn adjust_pipeline_depth(depth: i32, took_ms: i64, pipeline_max: i32) -> i32 {
    if took_ms < 1000 {
        pipeline_max.min(depth.saturating_mul(2))
    } else if took_ms > 2000 {
        1.max(depth / 2)
    } else {
        depth
    }
}

/// `DonorHealth::Walk` article vote settler.
/// Vote values (`DonorHealth::Answer`): `0 = None, 1 = Present, 2 = Missing, 3 = BodyBad, 4 = Error`.
/// Verdict values (`Final`): `0 = fUnsettled, 1 = fPresent, 2 = fMissing, 3 = fError`.
pub fn eval_article_votes(votes: &[i32]) -> i32 {
    if votes.contains(&1) {
        return 1;
    }
    let mut all = true;
    let mut only_errors = true;
    for &v in votes {
        all &= v != 0;
        only_errors &= v == 4;
    }
    if all && !votes.is_empty() {
        if only_errors {
            3
        } else {
            2
        }
    } else {
        0
    }
}

/// `DonorHealth::CheckItems` timeout majority-miss resolver for an unsettled article.
pub fn resolve_unsettled_vote(votes: &[i32]) -> i32 {
    let mut misses = 0usize;
    let mut present = false;
    for &v in votes {
        if v == 2 {
            misses += 1;
        }
        if v == 1 {
            present = true;
        }
    }
    if !present && !votes.is_empty() && 2 * misses >= votes.len() {
        2
    } else {
        0
    }
}

/// Standard 32-bit Mersenne Twister (`std::mt19937`) for deterministic `DonorHealth::Sample` and `Plan`.
pub struct Mt19937 {
    state: [u32; 624],
    index: usize,
}

impl Mt19937 {
    pub fn new(seed: u32) -> Self {
        let mut state = [0u32; 624];
        state[0] = seed;
        for i in 1..624 {
            let prev = state[i - 1];
            state[i] = 1812433253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        Self { state, index: 624 }
    }

    fn twist(&mut self) {
        const MATRIX_A: u32 = 0x9908_B0DF;
        const UPPER_MASK: u32 = 0x8000_0000;
        const LOWER_MASK: u32 = 0x7FFF_FFFF;
        for i in 0..624 {
            let y = (self.state[i] & UPPER_MASK) | (self.state[(i + 1) % 624] & LOWER_MASK);
            self.state[i] =
                self.state[(i + 397) % 624] ^ (y >> 1) ^ if y & 1 != 0 { MATRIX_A } else { 0 };
        }
        self.index = 0;
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.index >= 624 {
            self.twist();
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9D2C_5680;
        y ^= (y << 15) & 0xEFC6_0000;
        y ^= y >> 18;
        y
    }

    pub fn uniform_percent(&mut self) -> f64 {
        let r0 = self.next_u32() as f64;
        let r1 = self.next_u32() as f64;
        (r0 + r1 * 4_294_967_296.0) / 18_446_744_073_709_551_616.0 * 100.0
    }
}

pub const DONOR_MIN_KNOWN: i32 = 5;
pub const DONOR_SERVER_GIVE_UP: i32 = 3;
pub const DONOR_PIPELINE_START: i32 = 4;
pub const DONOR_PIPELINE_MAX: i32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum DonorAnswer {
    None = 0,
    Present = 1,
    Missing = 2,
    BodyBad = 3,
    Error = 4,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DonorHealthResult {
    pub checked: i32,
    pub present: i32,
    pub missing: i32,
    pub error: i32,
    pub body_checked: i32,
    pub body_bad: i32,
}

impl DonorHealthResult {
    pub fn answered(&self) -> i32 {
        self.present + self.missing + self.error
    }

    pub fn alive(&self) -> f64 {
        donor_health_alive(self.present, self.missing, self.error, DONOR_MIN_KNOWN)
    }
}

impl std::ops::AddAssign for DonorHealthResult {
    fn add_assign(&mut self, other: Self) {
        self.checked += other.checked;
        self.present += other.present;
        self.missing += other.missing;
        self.error += other.error;
        self.body_checked += other.body_checked;
        self.body_bad += other.body_bad;
    }
}

pub fn donor_dead_probe(health: &DonorHealthResult) -> bool {
    donor_health_dead_probe(health.present, health.missing, DONOR_MIN_KNOWN)
}

#[derive(Debug, Clone)]
pub struct DonorItem {
    pub message_id: String,
    pub body: bool,
    pub groups: std::sync::Arc<Vec<String>>,
}

#[derive(Debug, Clone, Copy)]
pub struct DonorSampleOptions {
    pub percent: i32,
    pub minimum: i32,
    pub maximum: i32,
    pub body_percent: i32,
    pub max_body: i32,
    pub seed: u32,
}

impl Default for DonorSampleOptions {
    fn default() -> Self {
        Self {
            percent: 5,
            minimum: 50,
            maximum: 1000,
            body_percent: 20,
            max_body: 20,
            seed: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DonorOptions {
    pub sample: DonorSampleOptions,
    pub probe: i32,
    pub budget_ms: i32,
    pub deadline_ms: i64,
}

impl Default for DonorOptions {
    fn default() -> Self {
        Self {
            sample: DonorSampleOptions::default(),
            probe: 10,
            budget_ms: 120_000,
            deadline_ms: 0,
        }
    }
}

pub fn donor_sample(
    ids: &[String],
    percent: i32,
    minimum: i32,
    maximum: i32,
    seed: u32,
) -> Vec<String> {
    let mut sorted = ids.to_vec();
    sorted.sort();
    let count = donor_sample_count(sorted.len(), percent, minimum, maximum);
    let mut rng = Mt19937::new(seed);
    for i in 0..count {
        let j = i + (rng.next_u32() as usize) % (sorted.len() - i);
        sorted.swap(i, j);
    }
    sorted.truncate(count);
    sorted
}

pub fn donor_plan(
    ids: &[String],
    groups: &std::sync::Arc<Vec<String>>,
    options: &DonorSampleOptions,
) -> Vec<DonorItem> {
    let mut rng = Mt19937::new(options.seed.wrapping_add(1));
    let mut bodies = 0;
    let sampled = donor_sample(
        ids,
        options.percent,
        options.minimum,
        options.maximum,
        options.seed,
    );
    let mut items = Vec::with_capacity(sampled.len());
    for id in sampled {
        let body =
            bodies < options.max_body && rng.uniform_percent() < (options.body_percent as f64);
        if body {
            bodies += 1;
        }
        items.push(DonorItem {
            message_id: id,
            body,
            groups: groups.clone(),
        });
    }
    items
}

pub struct DonorBodyGate {
    lock: std::sync::Mutex<()>,
    ok: std::sync::atomic::AtomicBool,
    settled: Box<dyn Fn() -> bool + Send + Sync>,
}

impl DonorBodyGate {
    pub fn new<F: Fn() -> bool + Send + Sync + 'static>(settled: F) -> Self {
        Self {
            lock: std::sync::Mutex::new(()),
            ok: std::sync::atomic::AtomicBool::new(false),
            settled: Box::new(settled),
        }
    }

    pub fn done(&self) -> bool {
        self.ok.load(std::sync::atomic::Ordering::SeqCst) || (self.settled)()
    }

    pub fn body_once<F: FnOnce() -> DonorAnswer>(&self, download: F) -> DonorAnswer {
        self.body_once_fallible::<(), _>(|| Ok(download())).unwrap()
    }

    pub fn body_once_fallible<E, F: FnOnce() -> Result<DonorAnswer, E>>(
        &self,
        download: F,
    ) -> Result<DonorAnswer, E> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        if self.done() {
            return Ok(DonorAnswer::Present);
        }
        let ans = download()?;
        self.ok.store(
            ans == DonorAnswer::Present,
            std::sync::atomic::Ordering::SeqCst,
        );
        Ok(ans)
    }
}

#[derive(Clone)]
pub struct DonorRequest {
    pub message_id: String,
    pub gate: Option<std::sync::Arc<DonorBodyGate>>,
    pub groups: std::sync::Arc<Vec<String>>,
}

#[derive(Debug, Default, Clone)]
pub struct DonorExchangeError {
    pub answers: Vec<DonorAnswer>,
    pub busy: bool,
}

static DONOR_STOPPING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static DONOR_WALKERS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static DONOR_EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub fn donor_now_ms() -> i64 {
    DONOR_EPOCH
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as i64
}

pub fn donor_stop_all() {
    DONOR_STOPPING.store(true, std::sync::atomic::Ordering::SeqCst);
}

pub fn donor_reset() {
    DONOR_STOPPING.store(false, std::sync::atomic::Ordering::SeqCst);
}

pub fn donor_stopping() -> bool {
    DONOR_STOPPING.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn donor_wait_all() {
    while DONOR_WALKERS.load(std::sync::atomic::Ordering::SeqCst) > 0 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

pub trait DonorServerExchange: Send + Sync {
    fn exchange(&self, batch: &[DonorRequest]) -> Result<Vec<DonorAnswer>, DonorExchangeError>;
}

pub struct DonorServer {
    cap: i32,
    retry_after_ms: std::sync::atomic::AtomicI32,
    depth: std::sync::atomic::AtomicI32,
    errors: std::sync::atomic::AtomicI32,
    down_until_ms: std::sync::atomic::AtomicI64,
    slot_mutex: std::sync::Mutex<i32>,
    slot_cond: std::sync::Condvar,
    max_active: std::sync::atomic::AtomicI32,
    transport: Box<dyn DonorServerExchange>,
}

impl DonorServer {
    pub fn new<T: DonorServerExchange + 'static>(cap: i32, transport: T) -> Self {
        Self {
            cap: cap.max(1),
            retry_after_ms: std::sync::atomic::AtomicI32::new(30_000),
            depth: std::sync::atomic::AtomicI32::new(DONOR_PIPELINE_START),
            errors: std::sync::atomic::AtomicI32::new(0),
            down_until_ms: std::sync::atomic::AtomicI64::new(0),
            slot_mutex: std::sync::Mutex::new(0),
            slot_cond: std::sync::Condvar::new(),
            max_active: std::sync::atomic::AtomicI32::new(0),
            transport: Box::new(transport),
        }
    }

    pub fn cap(&self) -> i32 {
        self.cap
    }

    pub fn depth(&self) -> i32 {
        self.depth.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn paused(&self) -> bool {
        self.down_until_ms.load(std::sync::atomic::Ordering::SeqCst) > donor_now_ms()
    }

    pub fn max_active(&self) -> i32 {
        self.max_active.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn set_retry_after_ms(&self, ms: i32) {
        self.retry_after_ms
            .store(ms, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn ask<F: Fn() -> bool>(&self, batch: &[DonorRequest], cancelled: F) -> Vec<DonorAnswer> {
        let over = || donor_stopping() || cancelled();
        while self.down_until_ms.load(std::sync::atomic::Ordering::SeqCst) - donor_now_ms() > 0
            && !over()
        {
            let rem = (self.down_until_ms.load(std::sync::atomic::Ordering::SeqCst)
                - donor_now_ms())
            .clamp(1, 50) as u64;
            std::thread::sleep(std::time::Duration::from_millis(rem));
        }

        {
            let mut active = self.slot_mutex.lock().unwrap_or_else(|e| e.into_inner());
            while *active >= self.cap && !over() {
                let (guard, _) = self
                    .slot_cond
                    .wait_timeout(active, std::time::Duration::from_millis(50))
                    .unwrap_or_else(|e| e.into_inner());
                active = guard;
            }
            if over() {
                return vec![DonorAnswer::Error; batch.len()];
            }
            *active += 1;
            let cur = *active;
            self.max_active
                .fetch_max(cur, std::sync::atomic::Ordering::SeqCst);
        }

        let start = donor_now_ms();
        let mut answers = match self.transport.exchange(batch) {
            Ok(ans) => {
                let took = donor_now_ms() - start;
                let new_depth = adjust_pipeline_depth(self.depth(), took, DONOR_PIPELINE_MAX);
                self.depth
                    .store(new_depth, std::sync::atomic::Ordering::SeqCst);
                self.errors.store(0, std::sync::atomic::Ordering::SeqCst);
                ans
            }
            Err(err) => {
                if !err.busy {
                    let errs = self
                        .errors
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                        + 1;
                    if errs >= DONOR_SERVER_GIVE_UP {
                        let retry = self
                            .retry_after_ms
                            .load(std::sync::atomic::Ordering::SeqCst)
                            as i64;
                        self.down_until_ms
                            .store(donor_now_ms() + retry, std::sync::atomic::Ordering::SeqCst);
                        self.errors.store(0, std::sync::atomic::Ordering::SeqCst);
                    }
                }
                err.answers
            }
        };

        answers.resize(batch.len(), DonorAnswer::Error);
        for ans in &mut answers {
            if *ans == DonorAnswer::None {
                *ans = DonorAnswer::Error;
            }
        }

        {
            let mut active = self.slot_mutex.lock().unwrap_or_else(|e| e.into_inner());
            *active -= 1;
        }
        self.slot_cond.notify_one();
        answers
    }
}

pub fn donor_check_items(
    servers: &[std::sync::Arc<DonorServer>],
    items: &[DonorItem],
    budget_ms: i32,
) -> DonorHealthResult {
    donor_check_items_guarded(servers, items, budget_ms, None)
}

pub fn donor_check_items_guarded(
    servers: &[std::sync::Arc<DonorServer>],
    items: &[DonorItem],
    budget_ms: i32,
    keepalive: Option<std::sync::Arc<dyn Send + Sync>>,
) -> DonorHealthResult {
    let mut health = DonorHealthResult {
        checked: items.len() as i32,
        ..Default::default()
    };
    if items.is_empty() {
        return health;
    }

    struct SharedCheckState {
        items: Vec<DonorItem>,
        servers: Vec<std::sync::Arc<DonorServer>>,
        final_verdict: Vec<std::sync::atomic::AtomicI32>,
        stop: std::sync::atomic::AtomicBool,
        mutex: std::sync::Mutex<(Vec<Vec<DonorAnswer>>, Vec<bool>, usize)>,
        cond: std::sync::Condvar,
        _keepalive: Option<std::sync::Arc<dyn Send + Sync>>,
    }

    let count = items.len();
    for item in items {
        if item.body {
            health.body_checked += 1;
        }
    }

    let state = std::sync::Arc::new(SharedCheckState {
        items: items.to_vec(),
        servers: servers.to_vec(),
        final_verdict: (0..count)
            .map(|_| std::sync::atomic::AtomicI32::new(0))
            .collect(),
        stop: std::sync::atomic::AtomicBool::new(false),
        mutex: std::sync::Mutex::new((
            vec![vec![DonorAnswer::None; servers.len()]; count],
            vec![false; count],
            count,
        )),
        cond: std::sync::Condvar::new(),
        _keepalive: keepalive,
    });
    // Rebind gates so their settled closure reads `state.final_verdict[i]`
    let mut gates_bound = Vec::with_capacity(count);
    for (i, item) in items.iter().enumerate() {
        if item.body {
            let st = state.clone();
            gates_bound.push(Some(std::sync::Arc::new(DonorBodyGate::new(move || {
                st.final_verdict[i].load(std::sync::atomic::Ordering::SeqCst) != 0
            }))));
        } else {
            gates_bound.push(None);
        }
    }
    let gates_arc = std::sync::Arc::new(gates_bound);

    for s in 0..servers.len() {
        DONOR_WALKERS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let st = state.clone();
        let gt = gates_arc.clone();
        std::thread::spawn(move || {
            struct WalkerGuard;
            impl Drop for WalkerGuard {
                fn drop(&mut self) {
                    DONOR_WALKERS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
            let _guard = WalkerGuard;
            let server = &st.servers[s];
            let mut cursor = 0usize;
            while cursor < st.items.len() {
                if st.stop.load(std::sync::atomic::Ordering::SeqCst) || donor_stopping() {
                    return;
                }
                let mut todo = Vec::new();
                while cursor < st.items.len() && (todo.len() as i32) < server.depth() {
                    if st.final_verdict[cursor].load(std::sync::atomic::Ordering::SeqCst) == 0 {
                        todo.push(cursor);
                    }
                    cursor += 1;
                }
                if todo.is_empty() {
                    continue;
                }
                let others_can_answer = st
                    .servers
                    .iter()
                    .enumerate()
                    .any(|(idx, srv)| idx != s && !srv.paused());
                let answers = if server.paused() && others_can_answer {
                    vec![DonorAnswer::Error; todo.len()]
                } else {
                    let batch: Vec<DonorRequest> = todo
                        .iter()
                        .map(|&idx| DonorRequest {
                            message_id: st.items[idx].message_id.clone(),
                            gate: gt[idx].clone(),
                            groups: st.items[idx].groups.clone(),
                        })
                        .collect();
                    let st_cancel = st.clone();
                    server.ask(&batch, move || {
                        st_cancel.stop.load(std::sync::atomic::Ordering::SeqCst)
                    })
                };

                let mut guard = st.mutex.lock().unwrap_or_else(|e| e.into_inner());
                if st.stop.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                for (k, &idx) in todo.iter().enumerate() {
                    guard.0[idx][s] = answers[k];
                    if answers[k] == DonorAnswer::BodyBad {
                        guard.1[idx] = true;
                    }
                    let vote_vals: Vec<i32> = guard.0[idx].iter().map(|&v| v as i32).collect();
                    let verdict = eval_article_votes(&vote_vals);
                    if verdict != 0
                        && st.final_verdict[idx]
                            .compare_exchange(
                                0,
                                verdict,
                                std::sync::atomic::Ordering::SeqCst,
                                std::sync::atomic::Ordering::SeqCst,
                            )
                            .is_ok()
                    {
                        guard.2 -= 1;
                        if guard.2 == 0 {
                            st.cond.notify_all();
                        }
                    }
                }
            }
        });
    }

    let deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(budget_ms.max(10) as u64);
    let mut guard = state.mutex.lock().unwrap_or_else(|e| e.into_inner());
    while guard.2 > 0 && !donor_stopping() && std::time::Instant::now() < deadline {
        let now = std::time::Instant::now();
        let wait_dur = deadline
            .saturating_duration_since(now)
            .min(std::time::Duration::from_millis(100));
        let (g, _) = state
            .cond
            .wait_timeout(guard, wait_dur)
            .unwrap_or_else(|e| e.into_inner());
        guard = g;
    }
    state.stop.store(true, std::sync::atomic::Ordering::SeqCst);

    for i in 0..count {
        if state.final_verdict[i].load(std::sync::atomic::Ordering::SeqCst) == 0 {
            let vote_vals: Vec<i32> = guard.0[i].iter().map(|&v| v as i32).collect();
            let resolved = resolve_unsettled_vote(&vote_vals);
            if resolved != 0 {
                state.final_verdict[i].store(resolved, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let v = state.final_verdict[i].load(std::sync::atomic::Ordering::SeqCst);
        donor_health_accum_verdict_counts(
            v,
            1,
            2,
            3,
            guard.1[i],
            &mut health.present,
            &mut health.missing,
            &mut health.error,
            &mut health.body_bad,
        );
    }
    health
}

pub fn donor_check_posting(
    servers: &[std::sync::Arc<DonorServer>],
    ids: &[String],
    groups: &std::sync::Arc<Vec<String>>,
    options: &DonorOptions,
    full: bool,
) -> DonorHealthResult {
    donor_check_posting_guarded(servers, ids, groups, options, full, None)
}

pub fn donor_check_posting_guarded(
    servers: &[std::sync::Arc<DonorServer>],
    ids: &[String],
    groups: &std::sync::Arc<Vec<String>>,
    options: &DonorOptions,
    full: bool,
    keepalive: Option<std::sync::Arc<dyn Send + Sync>>,
) -> DonorHealthResult {
    let items = donor_plan(ids, groups, &options.sample);
    let first = if (items.len() as i32) >= options.probe {
        items[..options.probe as usize].to_vec()
    } else {
        let mut probe_sample = options.sample;
        probe_sample.percent = 0;
        probe_sample.minimum = options.probe;
        probe_sample.maximum = options.probe;
        donor_plan(ids, groups, &probe_sample)
    };

    let deadline = donor_now_ms() + (options.budget_ms as i64);
    let mut health =
        donor_check_items_guarded(servers, &first, options.budget_ms, keepalive.clone());
    if full && !donor_dead_probe(&health) {
        let probed: std::collections::BTreeSet<&str> =
            first.iter().map(|it| it.message_id.as_str()).collect();
        let rest: Vec<DonorItem> = items
            .into_iter()
            .filter(|it| !probed.contains(it.message_id.as_str()))
            .collect();
        let rem_budget = (deadline - donor_now_ms()).max(10) as i32;
        health += donor_check_items_guarded(servers, &rest, rem_budget, keepalive);
    }
    health
}

pub struct DonorPosting {
    pub key: String,
    pub ids: Vec<String>,
    pub groups: std::sync::Arc<Vec<String>>,
}

pub fn donor_check_postings<F>(
    servers: &[std::sync::Arc<DonorServer>],
    postings: &[DonorPosting],
    options: &DonorOptions,
    full: bool,
    concurrency: i32,
    emit: F,
) where
    F: Fn(&str, &DonorHealthResult) + Send + Sync,
{
    donor_check_postings_guarded(servers, postings, options, full, concurrency, None, emit);
}

pub fn donor_check_postings_guarded<F>(
    servers: &[std::sync::Arc<DonorServer>],
    postings: &[DonorPosting],
    options: &DonorOptions,
    full: bool,
    concurrency: i32,
    keepalive: Option<std::sync::Arc<dyn Send + Sync>>,
    emit: F,
) where
    F: Fn(&str, &DonorHealthResult) + Send + Sync,
{
    let next = std::sync::atomic::AtomicUsize::new(0);
    let thread_count = postings.len().min(concurrency.max(1) as usize);
    let next_ref = &next;
    let emit_ref = &emit;
    std::thread::scope(|s| {
        for _ in 0..thread_count {
            let ka = keepalive.clone();
            s.spawn(move || loop {
                let idx = next_ref.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if idx >= postings.len() {
                    break;
                }
                let mut own = *options;
                if options.deadline_ms > 0 {
                    let left = options.deadline_ms - donor_now_ms();
                    if left <= 0 {
                        continue;
                    }
                    own.budget_ms = own.budget_ms.min(left as i32);
                }
                let p = &postings[idx];
                let h =
                    donor_check_posting_guarded(servers, &p.ids, &p.groups, &own, full, ka.clone());
                emit_ref(&p.key, &h);
            });
        }
    });
}

// ============================================================================
// 3b. DupeIdentity::FetchArticle / Read & NzbFetcher (`DupeIdentity.cpp`, `NzbFetcher.cpp`)
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct FetchedArticleResult {
    pub success: bool,
    pub offset: i64,
    pub file_size: i64,
    pub data: Vec<u8>,
}

/// `DupeIdentity::FetchArticle` (`daemon/dupesearch/DupeIdentity.cpp`).
pub fn dupe_identity_fetch_article<N, S, F, C>(
    message_id: &[u8],
    groups: &[String],
    deadline: i64,
    now_fn: N,
    stopped_fn: S,
    fetch_fn: F,
    stop_fetch_fn: C,
) -> FetchedArticleResult
where
    N: Fn() -> i64,
    S: Fn() -> bool,
    F: FnOnce(&[u8], &[String]) -> Result<FetchedArticleResult, ()> + Send,
    C: FnOnce(),
{
    if message_id.is_empty() || now_fn() >= deadline || stopped_fn() || donor_stopping() {
        return FetchedArticleResult::default();
    }
    let bracketed_id = crate::nntp::nntp_health_format_bracketed_id(message_id);
    let done = std::sync::atomic::AtomicBool::new(false);
    let mut out = FetchedArticleResult::default();
    std::thread::scope(|s| {
        let done_ref = &done;
        let out_ref = &mut out;
        let id_slice = bracketed_id.as_slice();
        let handle = s.spawn(move || {
            if let Ok(res) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                fetch_fn(id_slice, groups)
            })) {
                if let Ok(article) = res {
                    *out_ref = article;
                }
            }
            done_ref.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        while !done.load(std::sync::atomic::Ordering::SeqCst)
            && now_fn() < deadline
            && !stopped_fn()
            && !donor_stopping()
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !done.load(std::sync::atomic::Ordering::SeqCst) {
            stop_fetch_fn();
        }
        let _ = handle.join();
    });
    if now_fn() >= deadline || stopped_fn() || donor_stopping() || out.offset != 0 {
        return FetchedArticleResult::default();
    }
    out
}

/// `DupeIdentity::Read` (`daemon/dupesearch/DupeIdentity.cpp`).
pub fn dupe_identity_read<N, S, F>(
    data_bytes: i64,
    heads: &[(Vec<u8>, Vec<u8>, i64)],
    groups: &[String],
    deadline: i64,
    now_fn: N,
    stopped_fn: S,
    mut fetch_article_fn: F,
) -> Option<crate::dupeidentity::Identity>
where
    N: Fn() -> i64,
    S: Fn() -> bool,
    F: FnMut(&[u8], &[String], i64) -> FetchedArticleResult,
{
    let minimum = crate::dupeidentity::calc_min_inner_bytes(data_bytes);
    for (name, message_id, _bytes) in heads {
        let article = fetch_article_fn(message_id, groups, deadline);
        if article.success {
            let total = if article.file_size > 0 {
                article.file_size as u64
            } else {
                0
            };
            if let Some(found) =
                crate::dupeidentity::inner_entry(&article.data, name, total, minimum)
            {
                return Some(found);
            }
        }
        if now_fn() >= deadline || stopped_fn() || donor_stopping() {
            break;
        }
    }
    None
}

pub const NZB_FETCHER_COOLDOWN_SEC: i64 = 30 * 60;
pub const NZB_FETCHER_RETRY_DELAY_SEC: i64 = 2;
pub const NZB_FETCHER_MAX_NZB_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum NzbFetchReason {
    Ok = 0,
    Fetch = 1,
    Parse = 2,
    Refused = 3,
    Budget = 4,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NzbFetcherStats {
    pub ok: i32,
    pub fetch: i32,
    pub parse: i32,
    pub refused: i32,
    pub relisted: i32,
    pub listing_mismatch: i32,
    pub budget: i32,
}

impl NzbFetcherStats {
    pub fn add(&mut self, other: &Self) {
        self.ok += other.ok;
        self.fetch += other.fetch;
        self.parse += other.parse;
        self.refused += other.refused;
        self.relisted += other.relisted;
        self.listing_mismatch += other.listing_mismatch;
        self.budget += other.budget;
    }
}

#[derive(Debug, Clone)]
pub struct NzbFetched {
    pub listing: NewznabResult,
    pub reason: NzbFetchReason,
    pub data: Vec<u8>,
    pub info: NzbSummaryData,
    pub same_inner: bool,
    pub lenient: bool,
}

impl Default for NzbFetched {
    fn default() -> Self {
        Self {
            listing: NewznabResult::default(),
            reason: NzbFetchReason::Fetch,
            data: Vec::new(),
            info: NzbSummaryData::default(),
            same_inner: false,
            lenient: false,
        }
    }
}

pub fn dupe_write_atomic(path: &std::path::Path, data: &[u8]) -> bool {
    let mut temp_os = path.as_os_str().to_os_string();
    temp_os.push(".new");
    let temp = std::path::PathBuf::from(temp_os);
    if std::fs::write(&temp, data).is_err() {
        let _ = std::fs::remove_file(&temp);
        return false;
    }
    std::fs::rename(&temp, path).is_ok()
}

#[cfg(unix)]
pub(crate) fn dupe_bytes_to_path(bytes: &[u8]) -> &std::path::Path {
    use std::os::unix::ffi::OsStrExt;
    std::path::Path::new(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
pub(crate) fn dupe_bytes_to_path(bytes: &[u8]) -> &std::path::Path {
    std::path::Path::new(std::str::from_utf8(bytes).unwrap_or(""))
}

pub fn dupe_write_atomic_bytes(path_bytes: &[u8], data: &[u8]) -> bool {
    if path_bytes.is_empty() {
        return false;
    }
    dupe_write_atomic(dupe_bytes_to_path(path_bytes), data)
}

pub fn dupe_read_all_bytes(path_bytes: &[u8]) -> Vec<u8> {
    if path_bytes.is_empty() {
        return Vec::new();
    }
    std::fs::read(dupe_bytes_to_path(path_bytes)).unwrap_or_default()
}

pub struct NzbFetcherCore {
    state_path: std::sync::Mutex<Vec<u8>>,
    refused_until: std::sync::Mutex<std::collections::BTreeMap<Vec<u8>, i64>>,
    indexer_locks:
        std::sync::Mutex<std::collections::BTreeMap<Vec<u8>, std::sync::Arc<std::sync::Mutex<()>>>>,
}

impl Default for NzbFetcherCore {
    fn default() -> Self {
        Self::new()
    }
}

impl NzbFetcherCore {
    pub fn new() -> Self {
        Self {
            state_path: std::sync::Mutex::new(Vec::new()),
            refused_until: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            indexer_locks: std::sync::Mutex::new(std::collections::BTreeMap::new()),
        }
    }

    pub fn set_state_path_bytes(&self, path: &[u8]) {
        if let Ok(mut guard) = self.state_path.lock() {
            *guard = path.to_vec();
        }
    }

    pub fn load(&self, now: i64) {
        let path = self
            .state_path
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        if path.is_empty() {
            return;
        }
        let Ok(content) = std::fs::read(dupe_bytes_to_path(&path)) else {
            return;
        };
        let mut map = self.refused_until.lock().unwrap_or_else(|e| e.into_inner());
        for line in content.split(|&b| b == b'\n') {
            if let Some((idx, when)) = parse_cooldown_line(line, now) {
                map.insert(idx, when);
            }
        }
    }

    fn save_locked(
        &self,
        map: &std::collections::BTreeMap<Vec<u8>, i64>,
        now: i64,
    ) -> Result<(), Vec<u8>> {
        let path = self
            .state_path
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        if path.is_empty() {
            return Ok(());
        }
        let mut text = Vec::new();
        for (idx, &when) in map.iter() {
            let line = format_cooldown_line(idx, when, now);
            if !line.is_empty() {
                text.extend_from_slice(&line);
            }
        }
        if !dupe_write_atomic(dupe_bytes_to_path(&path), &text) {
            return Err(path);
        }
        Ok(())
    }

    fn indexer_lock(&self, indexer: &[u8]) -> std::sync::Arc<std::sync::Mutex<()>> {
        let mut locks = self.indexer_locks.lock().unwrap_or_else(|e| e.into_inner());
        locks
            .entry(indexer.to_vec())
            .or_insert_with(|| std::sync::Arc::new(std::sync::Mutex::new(())))
            .clone()
    }

    pub fn is_refused(&self, indexer: &[u8], now: i64) -> bool {
        let map = self.refused_until.lock().unwrap_or_else(|e| e.into_inner());
        map.get(indexer).is_some_and(|&until| until > now)
    }

    pub fn refuse(&self, indexer: &[u8], now: i64) -> Result<(), Vec<u8>> {
        let mut map = self.refused_until.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(indexer.to_vec(), now + NZB_FETCHER_COOLDOWN_SEC);
        self.save_locked(&map, now)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fetch_with<N, M, S, W, H, LI, LW>(
        &self,
        listing: &NewznabResult,
        deadline: i64,
        retries: i32,
        budget: Option<&crate::dupewatch::FetchBudget>,
        now_fn: N,
        now_ms_fn: M,
        stopped_fn: S,
        sleep_100ms_fn: W,
        mut http_fetch_fn: H,
        mut log_info_fn: LI,
        mut log_warn_fn: LW,
    ) -> NzbFetched
    where
        N: Fn() -> i64,
        M: Fn() -> i64,
        S: Fn() -> bool,
        W: Fn(),
        H: FnMut(&[u8], &[u8], usize, i64) -> (bool, i32, Vec<u8>),
        LI: FnMut(&[u8]),
        LW: FnMut(&[u8]),
    {
        let mut fetched = NzbFetched {
            listing: listing.clone(),
            reason: NzbFetchReason::Fetch,
            ..Default::default()
        };
        if let Some(b) = budget {
            if b.spent() {
                fetched.reason = NzbFetchReason::Budget;
                return fetched;
            }
        }
        if stopped_fn() || now_fn() >= deadline {
            return fetched;
        }

        let lock_arc = self.indexer_lock(&listing.indexer);
        let _indexer_guard = lock_arc.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_refused(&listing.indexer, now_fn()) {
            fetched.reason = NzbFetchReason::Refused;
            return fetched;
        }

        let mut status = 0i32;
        for attempt in 0..=retries {
            if stopped_fn() || now_fn() >= deadline {
                break;
            }
            if let Some(b) = budget {
                if !b.take() {
                    fetched.reason = NzbFetchReason::Budget;
                    return fetched;
                }
            }
            let fetch_deadline_ms = 1i64.max(now_ms_fn() + (deadline - now_fn()) * 1000);
            let mut label = b"DupeSearch nzb of ".to_vec();
            label.extend_from_slice(&listing.indexer);
            let (ok, reply_status, reply_body) = http_fetch_fn(
                &listing.link,
                &label,
                NZB_FETCHER_MAX_NZB_BYTES,
                fetch_deadline_ms,
            );
            let why = if ok {
                status = 0;
                if let Some(info) = parse_nzb_summary(&reply_body) {
                    fetched.reason = NzbFetchReason::Ok;
                    fetched.data = reply_body;
                    fetched.info = info;
                    return fetched;
                }
                fetched.reason = NzbFetchReason::Parse;
                nzb_fetcher_format_parse_err(&reply_body)
            } else {
                status = reply_status;
                fetched.reason = NzbFetchReason::Fetch;
                nzb_fetcher_format_http_status_err(reply_status)
            };
            let mut msg = b"DupeSearch: the nzb-file of ".to_vec();
            msg.extend_from_slice(&listing.title);
            msg.extend_from_slice(b" from ");
            msg.extend_from_slice(&listing.indexer);
            msg.extend_from_slice(format!(" failed (attempt {}): ", attempt + 1).as_bytes());
            msg.extend_from_slice(&why);
            log_info_fn(&msg);

            if let Some(b) = budget {
                if b.spent() {
                    break;
                }
            }
            if attempt < retries {
                if now_fn() + NZB_FETCHER_RETRY_DELAY_SEC >= deadline {
                    break;
                }
                for _ in 0..(NZB_FETCHER_RETRY_DELAY_SEC * 10) {
                    if stopped_fn() {
                        break;
                    }
                    sleep_100ms_fn();
                }
            }
            if stopped_fn() {
                break;
            }
        }

        if (status == 403 || status == 429) && !listing.indexer.is_empty() {
            if let Err(failed_path) = self.refuse(&listing.indexer, now_fn()) {
                let mut warn_msg = b"DupeSearch: could not save ".to_vec();
                warn_msg.extend_from_slice(&failed_path);
                log_warn_fn(&warn_msg);
            }
            let mut info_msg = b"DupeSearch: indexer ".to_vec();
            info_msg.extend_from_slice(&listing.indexer);
            info_msg.extend_from_slice(
                format!(
                    " refused nzb-file downloads (HTTP {}): not asking it for {} minutes",
                    status,
                    NZB_FETCHER_COOLDOWN_SEC / 60
                )
                .as_bytes(),
            );
            log_info_fn(&info_msg);
        }
        fetched
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fetch_posting_with<N, M, S, W, H, LI, LW>(
        &self,
        listings: &[NewznabResult],
        deadline: i64,
        stats: &mut NzbFetcherStats,
        budget: Option<&crate::dupewatch::FetchBudget>,
        now_fn: N,
        now_ms_fn: M,
        stopped_fn: S,
        sleep_100ms_fn: W,
        mut http_fetch_fn: H,
        mut log_info_fn: LI,
        mut log_warn_fn: LW,
    ) -> NzbFetched
    where
        N: Fn() -> i64 + Copy,
        M: Fn() -> i64 + Copy,
        S: Fn() -> bool + Copy,
        W: Fn() + Copy,
        H: FnMut(&[u8], &[u8], usize, i64) -> (bool, i32, Vec<u8>),
        LI: FnMut(&[u8]),
        LW: FnMut(&[u8]),
    {
        let mut last = NzbFetched::default();
        if listings.is_empty() {
            stats.parse += 1;
            return last;
        }
        last.listing = listings[0].clone();
        let mut other: Option<NzbFetched> = None;

        for (i, listing) in listings.iter().enumerate() {
            let fetched = self.fetch_with(
                listing,
                deadline,
                1,
                budget,
                now_fn,
                now_ms_fn,
                stopped_fn,
                sleep_100ms_fn,
                &mut http_fetch_fn,
                &mut log_info_fn,
                &mut log_warn_fn,
            );
            if fetched.reason == NzbFetchReason::Budget {
                stats.budget += 1;
                last = fetched;
                break;
            }
            if fetched.reason == NzbFetchReason::Ok
                && !crate::posting::listing_mismatch(listing.size, fetched.info.total_bytes)
            {
                stats.ok += 1;
                stats.relisted += (listings.len() - i - 1) as i32;
                return fetched;
            }
            if fetched.reason == NzbFetchReason::Ok {
                if other.is_none() {
                    other = Some(fetched);
                }
                continue;
            }
            match fetched.reason {
                NzbFetchReason::Fetch => stats.fetch += 1,
                NzbFetchReason::Parse => stats.parse += 1,
                NzbFetchReason::Refused => stats.refused += 1,
                _ => {}
            }
            last = fetched;
        }

        if let Some(other_fetched) = other {
            stats.ok += 1;
            stats.listing_mismatch += 1;
            return other_fetched;
        }
        last
    }
}

// ============================================================================
// 4. Newznab (`daemon/dupesearch/Newznab.cpp`)
// ============================================================================

/// `UrlEncode` in `Newznab.cpp`.
pub fn url_encode(text: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = Vec::with_capacity(text.len());
    for &ch in text {
        if ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_' || ch == b'.' || ch == b'~' {
            out.push(ch);
        } else if ch == b' ' {
            out.push(b'+');
        } else {
            out.push(b'%');
            out.push(HEX[(ch >> 4) as usize]);
            out.push(HEX[(ch & 0x0F) as usize]);
        }
    }
    out
}

/// `DigitsOnly` in `Newznab.cpp`.
pub fn digits_only(text: &[u8]) -> Vec<u8> {
    text.iter()
        .copied()
        .filter(|b| b.is_ascii_digit())
        .collect()
}

/// `DaysFromCivil` in `Newznab.cpp`.
pub fn days_from_civil(mut year: i64, month: i32, day: i32) -> i64 {
    if month <= 2 {
        year -= 1;
    }
    let era = (if year >= 0 { year } else { year - 399 }) / 400;
    let yoe = year - era * 400;
    let m_adj = (month + if month > 2 { -3 } else { 9 }) as i64;
    let doy = (153 * m_adj + 2) / 5 + (day as i64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn parse_exact_digits(s: &[u8]) -> Option<i32> {
    if s.is_empty() || !s.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut acc: i32 = 0;
    for &b in s {
        acc = acc.saturating_mul(10).saturating_add((b - b'0') as i32);
    }
    Some(acc)
}

fn parse_iso_date(trimmed: &[u8]) -> Option<(i32, i32, i32, i32, i32, i32, Vec<u8>)> {
    // ^(\d{4})-(\d{2})-(\d{2})[Tt ](\d{2}):(\d{2}):(\d{2})(?:\.\d+)?\s*(Z|z|[+-]\d{2}:?\d{2})?$
    if trimmed.len() < 19 {
        return None;
    }
    if trimmed[4] != b'-' || trimmed[7] != b'-' {
        return None;
    }
    let sep = trimmed[10];
    if sep != b'T' && sep != b't' && sep != b' ' {
        return None;
    }
    if trimmed[13] != b':' || trimmed[16] != b':' {
        return None;
    }
    let year = parse_exact_digits(&trimmed[0..4])?;
    let month = parse_exact_digits(&trimmed[5..7])?;
    let day = parse_exact_digits(&trimmed[8..10])?;
    let hour = parse_exact_digits(&trimmed[11..13])?;
    let minute = parse_exact_digits(&trimmed[14..16])?;
    let second = parse_exact_digits(&trimmed[17..19])?;

    let mut pos = 19;
    if pos < trimmed.len() && trimmed[pos] == b'.' {
        pos += 1;
        let frac_start = pos;
        while pos < trimmed.len() && trimmed[pos].is_ascii_digit() {
            pos += 1;
        }
        if pos == frac_start {
            return None;
        }
    }
    while pos < trimmed.len() && trimmed[pos].is_ascii_whitespace() {
        pos += 1;
    }
    let rest = &trimmed[pos..];
    let mut zone = Vec::new();
    if !rest.is_empty() {
        if rest == b"Z" || rest == b"z" {
            zone.extend_from_slice(rest);
        } else if (rest[0] == b'+' || rest[0] == b'-') && (rest.len() == 5 || rest.len() == 6) {
            if rest.len() == 6 {
                if rest[3] != b':'
                    || !rest[1..3].iter().all(|b| b.is_ascii_digit())
                    || !rest[4..6].iter().all(|b| b.is_ascii_digit())
                {
                    return None;
                }
                zone.push(rest[0]);
                zone.extend_from_slice(&rest[1..3]);
                zone.extend_from_slice(&rest[4..6]);
            } else {
                if !rest[1..5].iter().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                zone.extend_from_slice(rest);
            }
        } else {
            return None;
        }
    }
    if !(1..=12).contains(&month) {
        return None;
    }
    Some((year, month, day, hour, minute, second, zone))
}

fn parse_rfc_date(trimmed: &[u8]) -> Option<(i32, i32, i32, i32, i32, i32, Vec<u8>)> {
    // ^(?:[A-Za-z]{3},\s*)?(\d{1,2})\s+([A-Za-z]{3})[a-z]*\s+(\d{4}|\d{2})\s+(\d{1,2}):(\d{2}):(\d{2})\s*([+-]\d{4}|[A-Za-z]+)?$
    let mut s = trimmed;
    if s.len() >= 4 && s[0..3].iter().all(|b| b.is_ascii_alphabetic()) && s[3] == b',' {
        s = &s[4..];
        while !s.is_empty() && s[0].is_ascii_whitespace() {
            s = &s[1..];
        }
    }
    fn next_token(input: &[u8]) -> Option<(&[u8], &[u8])> {
        let mut i = 0;
        while i < input.len() && input[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= input.len() {
            return None;
        }
        let start = i;
        while i < input.len() && !input[i].is_ascii_whitespace() {
            i += 1;
        }
        Some((&input[start..i], &input[i..]))
    }

    let (day_tok, r1) = next_token(s)?;
    if !(1..=2).contains(&day_tok.len()) {
        return None;
    }
    let day = parse_exact_digits(day_tok)?;

    let (mon_tok, r2) = next_token(r1)?;
    if mon_tok.len() < 3
        || !mon_tok[0..3].iter().all(|b| b.is_ascii_alphabetic())
        || !mon_tok[3..].iter().all(|b| b.is_ascii_lowercase())
    {
        return None;
    }
    const MONTHS: [&[u8; 3]; 12] = [
        b"jan", b"feb", b"mar", b"apr", b"may", b"jun", b"jul", b"aug", b"sep", b"oct", b"nov",
        b"dec",
    ];
    let mon_lower = [
        mon_tok[0].to_ascii_lowercase(),
        mon_tok[1].to_ascii_lowercase(),
        mon_tok[2].to_ascii_lowercase(),
    ];
    let month = MONTHS
        .iter()
        .position(|m| **m == mon_lower)
        .map(|idx| (idx + 1) as i32)?;

    let (year_tok, r3) = next_token(r2)?;
    if year_tok.len() != 2 && year_tok.len() != 4 {
        return None;
    }
    let mut year = parse_exact_digits(year_tok)?;
    if year_tok.len() == 2 {
        year += if year < 50 { 2000 } else { 1900 };
    }

    // Require at least one whitespace character after year_tok (`\s+`)
    if r3.is_empty() || !r3[0].is_ascii_whitespace() {
        return None;
    }
    let mut t_pos = 0;
    while t_pos < r3.len() && r3[t_pos].is_ascii_whitespace() {
        t_pos += 1;
    }
    let time_and_zone = &r3[t_pos..];
    let first_colon = time_and_zone.iter().position(|&b| b == b':')?;
    if !(1..=2).contains(&first_colon) {
        return None;
    }
    let after_first = &time_and_zone[first_colon + 1..];
    if after_first.len() < 5 || after_first[2] != b':' {
        return None;
    }
    let hour = parse_exact_digits(&time_and_zone[..first_colon])?;
    let minute = parse_exact_digits(&after_first[0..2])?;
    let second = parse_exact_digits(&after_first[3..5])?;

    let r4 = &after_first[5..];
    let mut zone = Vec::new();
    let r4_trimmed_start = {
        let mut idx = 0;
        while idx < r4.len() && r4[idx].is_ascii_whitespace() {
            idx += 1;
        }
        &r4[idx..]
    };
    if !r4_trimmed_start.is_empty() {
        let is_signed_4 = r4_trimmed_start.len() == 5
            && (r4_trimmed_start[0] == b'+' || r4_trimmed_start[0] == b'-')
            && r4_trimmed_start[1..5].iter().all(|b| b.is_ascii_digit());
        let is_alpha = r4_trimmed_start.iter().all(|b| b.is_ascii_alphabetic());
        if !is_signed_4 && !is_alpha {
            return None;
        }
        zone.extend_from_slice(r4_trimmed_start);
    }

    Some((year, month, day, hour, minute, second, zone))
}

/// `Newznab::ParseDate` in `Newznab.cpp`.
pub fn parse_newznab_date(text: &[u8]) -> i64 {
    if text.len() > 64 {
        return 0;
    }
    let mut start = 0;
    let mut end = text.len();
    while start < end && text[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && text[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let trimmed = &text[start..end];
    if trimmed.is_empty() {
        return 0;
    }

    let (year, month, day, hour, minute, second, zone) =
        if let Some(parsed) = parse_iso_date(trimmed) {
            parsed
        } else if let Some(parsed) = parse_rfc_date(trimmed) {
            parsed
        } else {
            return 0;
        };

    if !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return 0;
    }

    let seconds = days_from_civil(year as i64, month, day) * 86400
        + (hour as i64) * 3600
        + (minute as i64) * 60
        + (second as i64);

    let mut offset: i64 = 0;
    if zone.len() == 5 && (zone[0] == b'+' || zone[0] == b'-') {
        let zh = parse_exact_digits(&zone[1..3]).unwrap_or(0) as i64;
        let zm = parse_exact_digits(&zone[3..5]).unwrap_or(0) as i64;
        let sign = if zone[0] == b'+' { 1 } else { -1 };
        offset = (zh * 3600 + zm * 60) * sign;
    } else if !zone.is_empty() {
        let upper: Vec<u8> = zone.iter().map(|b| b.to_ascii_uppercase()).collect();
        offset = match upper.as_slice() {
            b"UT" | b"UTC" | b"GMT" | b"Z" => 0,
            b"EST" => -5 * 3600,
            b"EDT" => -4 * 3600,
            b"CST" => -6 * 3600,
            b"CDT" => -5 * 3600,
            b"MST" => -7 * 3600,
            b"MDT" => -6 * 3600,
            b"PST" => -8 * 3600,
            b"PDT" => -7 * 3600,
            _ => {
                if upper.len() != 1 || upper[0] == b'J' {
                    return 0;
                }
                0
            }
        };
    }

    seconds - offset
}

/// `Newznab::BuildUrl` in `Newznab.cpp`.
pub fn build_newznab_url(
    base: &[u8],
    param_keys: &[&[u8]],
    param_vals: &[&[u8]],
    api_key: &[u8],
) -> Vec<u8> {
    let mut url = base.to_vec();
    if let Some(scheme) = base.windows(3).position(|w| w == b"://") {
        let after_scheme = scheme + 3;
        let host_end = base[after_scheme..]
            .iter()
            .position(|&b| b == b'/' || b == b'?')
            .map(|p| after_scheme + p)
            .unwrap_or(base.len());
        let query_start = base[host_end..]
            .iter()
            .position(|&b| b == b'?')
            .map(|p| host_end + p)
            .unwrap_or(base.len());
        let path = &base[host_end..query_start];
        if path.is_empty() || path == b"/" {
            url.clear();
            url.extend_from_slice(&base[..host_end]);
            url.extend_from_slice(b"/api");
            url.extend_from_slice(&base[query_start..]);
        }
    }
    if url.contains(&b'?') {
        url.push(b'&');
    } else {
        url.push(b'?');
    }
    let count = param_keys.len().min(param_vals.len());
    for i in 0..count {
        url.extend_from_slice(&url_encode(param_keys[i]));
        url.push(b'=');
        url.extend_from_slice(&url_encode(param_vals[i]));
        url.push(b'&');
    }
    url.extend_from_slice(b"apikey=");
    url.extend_from_slice(&url_encode(api_key));
    url
}

/// `Newznab::Mask` in `Newznab.cpp`.
pub fn mask_newznab_text(text: &[u8]) -> Vec<u8> {
    let cut = &text[..text.len().min(1024)];
    // Pass 1: replace `(apikey=)[^&\s]+` case-insensitively with `$1***`
    let mut pass1 = Vec::with_capacity(cut.len());
    let mut i = 0;
    while i < cut.len() {
        if i + 7 < cut.len() && cut[i..i + 7].eq_ignore_ascii_case(b"apikey=") {
            let mut j = i + 7;
            while j < cut.len() && cut[j] != b'&' && !cut[j].is_ascii_whitespace() {
                j += 1;
            }
            if j > i + 7 {
                pass1.extend_from_slice(&cut[i..i + 7]);
                pass1.extend_from_slice(b"***");
                i = j;
                continue;
            }
        }
        pass1.push(cut[i]);
        i += 1;
    }

    // Pass 2: replace `//[^/@\s:]+:[^/@\s]+@` with `//***@`
    let mut out = Vec::with_capacity(pass1.len());
    let mut k = 0;
    while k < pass1.len() {
        if k + 2 < pass1.len() && &pass1[k..k + 2] == b"//" {
            let mut u = k + 2;
            while u < pass1.len()
                && pass1[u] != b'/'
                && pass1[u] != b'@'
                && !pass1[u].is_ascii_whitespace()
                && pass1[u] != b':'
            {
                u += 1;
            }
            if u > k + 2 && u < pass1.len() && pass1[u] == b':' {
                let mut p = u + 1;
                while p < pass1.len()
                    && pass1[p] != b'/'
                    && pass1[p] != b'@'
                    && !pass1[p].is_ascii_whitespace()
                {
                    p += 1;
                }
                if p > u + 1 && p < pass1.len() && pass1[p] == b'@' {
                    out.extend_from_slice(b"//***@");
                    k = p + 1;
                    continue;
                }
            }
        }
        out.push(pass1[k]);
        k += 1;
    }
    out
}

/// Finds the first dot-separated token in `normalized` matching `(?:^|\.)s(\d+)e(\d+)(?:\.|$)`
/// and returns `(season, episode)` converted with C `atoi` semantics (`Newznab::BuildQueries`).
fn find_normalized_episode(normalized: &[u8]) -> Option<(i32, i32)> {
    for tok in normalized.split(|&b| b == b'.') {
        if tok.first() != Some(&b's') {
            continue;
        }
        let s_digits = tok[1..].iter().take_while(|&&b| b.is_ascii_digit()).count();
        if s_digits == 0 || tok.get(1 + s_digits) != Some(&b'e') {
            continue;
        }
        let ep_part = &tok[2 + s_digits..];
        if !ep_part.is_empty() && ep_part.iter().all(|b| b.is_ascii_digit()) {
            let season = c_atoi_bytes(&tok[1..1 + s_digits]);
            let ep = c_atoi_bytes(ep_part);
            return Some((season, ep));
        }
    }
    None
}

/// `Newznab::BuildQueries` in `daemon/dupesearch/Newznab.cpp`: builds the ordered list
/// of Newznab `(key, value)` query parameter sets for a release `title`, `imdb`, and `tvdb`.
pub fn build_newznab_queries(
    title: &[u8],
    imdb: &[u8],
    tvdb: &[u8],
) -> Vec<Vec<(Vec<u8>, Vec<u8>)>> {
    let normalized = crate::releasename::normalize(title);
    let full: Vec<u8> = normalized
        .iter()
        .map(|&b| if b == b'.' { b' ' } else { b })
        .collect();
    let short_query = crate::releasename::short_query(title);
    let group = crate::releasename::parse(title).group;

    let mut queries: Vec<Vec<(Vec<u8>, Vec<u8>)>> = Vec::new();
    queries.push(vec![
        (b"t".to_vec(), b"search".to_vec()),
        (b"q".to_vec(), full.clone()),
    ]);
    if !short_query.is_empty() && short_query != full {
        queries.push(vec![
            (b"t".to_vec(), b"search".to_vec()),
            (b"q".to_vec(), short_query.clone()),
        ]);
    }
    if !short_query.is_empty() && !group.is_empty() {
        let mut q_with_group = Vec::with_capacity(short_query.len() + 1 + group.len());
        q_with_group.extend_from_slice(&short_query);
        q_with_group.push(b' ');
        q_with_group.extend_from_slice(&group);
        queries.push(vec![
            (b"t".to_vec(), b"search".to_vec()),
            (b"q".to_vec(), q_with_group),
        ]);
    }

    let imdb_id = digits_only(imdb);
    if !imdb_id.is_empty() {
        queries.push(vec![
            (b"t".to_vec(), b"movie".to_vec()),
            (b"imdbid".to_vec(), imdb_id),
        ]);
    }

    let tvdb_id = digits_only(tvdb);
    if !tvdb_id.is_empty() {
        if let Some((season, ep)) = find_normalized_episode(&normalized) {
            queries.push(vec![
                (b"t".to_vec(), b"tvsearch".to_vec()),
                (b"tvdbid".to_vec(), tvdb_id),
                (b"season".to_vec(), season.to_string().into_bytes()),
                (b"ep".to_vec(), ep.to_string().into_bytes()),
            ]);
        }
    }

    queries
}

/// Clamps a fleet request's timeout in seconds to `[5, max_timeout_sec]`
/// (`Fleet::Append`).
pub fn fleet_clamp_timeout_sec(timeout_sec: i32, max_timeout_sec: i32) -> i32 {
    timeout_sec.min(max_timeout_sec).max(5)
}

/// Updates the key's `(top, lowest, running_id, running_score)` bounds for a
/// matching item in queue or history (`Fleet::Append`).
#[allow(clippy::too_many_arguments)]
pub fn fleet_update_key_score_bounds(
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

/// Computes the clamped duplicate score for adding a ranked fleet member
/// (`Fleet::Append`).
pub fn fleet_calc_member_add_score(running_id: i32, lowest: i32, top: i64, rank: i32) -> i32 {
    let base: i64 = if running_id != 0 {
        i64::from(lowest)
    } else {
        top.min(i64::from(i32::MAX))
    };
    let raw = base.saturating_sub(i64::from(rank));
    raw.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Accumulates an article's final verdict and soft body-bad flag into `Health`
/// counters (`DonorHealth::CheckItems`).
#[allow(clippy::too_many_arguments)]
pub fn donor_health_accum_verdict_counts(
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
pub fn fleet_ensure_nzb_ext(name: &[u8]) -> Vec<u8> {
    let mut out = name.to_vec();
    if name.len() < 4 || !name[name.len() - 4..].eq_ignore_ascii_case(b".nzb") {
        out.extend_from_slice(b".nzb");
    }
    out
}

/// Visits each non-empty `|`-separated segment of `names` (`Fleet::Add`).
pub fn fleet_visit_pipe_names(names: &[u8], mut visit: impl FnMut(&[u8])) {
    for part in names.split(|&b| b == b'|') {
        if !part.is_empty() {
            visit(part);
        }
    }
}

/// Formats the HTTP fetch error message for `Fleet::Append`.
pub fn fleet_format_fetch_error(status: i32) -> Vec<u8> {
    if status != 0 {
        format!("the nzb-file could not be fetched (HTTP {status})").into_bytes()
    } else {
        b"the nzb-file could not be fetched (connection failed)".to_vec()
    }
}

/// Why `Fleet::Append` must not add a member, or `None` to add it.
///
/// A fleet parks its backups through the duplicate check: every member is added
/// with a lower score and option DupeCheck moves it to history. With DupeCheck
/// off nothing parks them, so each added member downloads in parallel and
/// nothing stops the rest after one succeeds. Then only the best usable member
/// is added, and none at all while a download of the key is already running.
pub fn fleet_member_skip_reason(
    dupe_check: bool,
    running_id: i32,
    already_added: bool,
) -> Option<&'static [u8]> {
    if dupe_check {
        None
    } else if running_id != 0 {
        Some(b"DupeCheck is off: a download of the key is already running, a backup would download alongside it")
    } else if already_added {
        Some(b"DupeCheck is off: a backup would download alongside the chosen copy")
    } else {
        None
    }
}

/// Formats the final `Fleet of ...` summary log message in `Fleet::Append`.
pub fn fleet_format_summary_log(
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

/// Evaluates whether a queued item is a better parent candidate for rank-only scheduling
/// when a lower-scored external backup is added (`DupeSearch::DownloadQueueUpdate`).
#[allow(clippy::too_many_arguments)]
pub fn dupesearch_is_better_parent_candidate(
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

/// Formats comma-separated `"key: count"` rejection entries for `DupeSearch::Place`.
pub fn dupesearch_format_rejected_outcome(keys: &[&[u8]], counts: &[i32]) -> Vec<u8> {
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
pub fn dupesearch_parse_pending_dir_id(name: &[u8]) -> i32 {
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
pub fn dupesearch_pending_identity_matches(
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

/// Formats `"DupeSearch <label> page <page_one_based>"` for `Newznab::Search`.
pub fn newznab_format_search_info_name(label: &[u8], page_one_based: i32) -> Vec<u8> {
    let suffix = format!(" page {page_one_based}");
    let mut out = Vec::with_capacity(11 + label.len() + suffix.len());
    out.extend_from_slice(b"DupeSearch ");
    out.extend_from_slice(label);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Formats `"<queue_dir><sep>dupesearch-pending<sep><nzb_id>"` when `nzb_id > 0`
/// else `"<queue_dir><sep>dupesearch-pending"` for `DupeSearch::PendingDir`.
pub fn dupesearch_format_pending_dir(queue_dir: &[u8], sep: u8, nzb_id: i32) -> Vec<u8> {
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

/// Formats `"HTTP <status>"` for `NzbFetcher::Fetch`.
pub fn nzb_fetcher_format_http_status_err(status: i32) -> Vec<u8> {
    format!("HTTP {status}").into_bytes()
}

/// Formats `"<temp_dir><sep>dupesearch-<now_sec>-<counter>.tmp"` for `HttpGet::Fetch`.
pub fn http_get_format_temp_path(temp_dir: &[u8], sep: u8, now_sec: i32, counter: i32) -> Vec<u8> {
    let suffix = format!("dupesearch-{now_sec}-{counter}.tmp");
    let mut out = Vec::with_capacity(temp_dir.len() + 1 + suffix.len());
    out.extend_from_slice(temp_dir);
    out.push(sep);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Classifies an XML start element in `Newznab::PageReader::Start`:
/// `1` (`depth == 1 && name == "error"`), `2` (`"item"`), `3` (`in_item && name == "attr"`), or `0` (other).
pub fn newznab_classify_start_element(depth: i32, in_item: bool, name: &[u8]) -> i32 {
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
pub fn newznab_classify_item_attr_key(key: &[u8]) -> i32 {
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
pub fn newznab_classify_end_element(in_item: bool, name: &[u8]) -> i32 {
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
pub fn nzb_reader_classify_start_element(name: &[u8], in_file: bool) -> i32 {
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
pub fn nzb_reader_classify_end_element(
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

/// Computes `(start, len)` after trimming any byte in `chars` from both ends of `text`,
/// or `None` when every byte in `text` is in `chars`, for `DupeUtil::Trim`.
pub fn dupe_util_trim_bounds(text: &[u8], chars: &[u8]) -> Option<(usize, usize)> {
    let begin = text.iter().position(|b| !chars.contains(b))?;
    let end = text.iter().rposition(|b| !chars.contains(b))?;
    Some((begin, end - begin + 1))
}

/// Lowercases `text` byte-by-byte using the active C locale (`c_lower`), for `DupeUtil::Lower`.
pub fn dupe_util_lower(text: &[u8]) -> Vec<u8> {
    crate::releasename::lower(text)
}

/// Formats `"not an nzb-file: <masked first 160 bytes>"` for `NzbFetcher::Fetch`.
pub fn nzb_fetcher_format_parse_err(body: &[u8]) -> Vec<u8> {
    let prefix = &body[..body.len().min(160)];
    let masked = crate::dupesearch_nzb::mask_newznab_text(prefix);
    let mut out = Vec::with_capacity(17 + masked.len());
    out.extend_from_slice(b"not an nzb-file: ");
    out.extend_from_slice(&masked);
    out
}

/// Visits up to 8 non-empty lenient search queries for `DupeSearch::Expand`.
pub fn dupesearch_visit_expand_queries<F: FnMut(&[u8])>(name: &[u8], mut cb: F) {
    for q in crate::dupewatch::lenient_queries(name).into_iter().take(8) {
        if !q.is_empty() {
            cb(&q);
        }
    }
}

/// Formats `Newznab::Search` query label (`"key1=val1 key2=val2"`).
pub fn newznab_format_query_label(keys: &[&[u8]], vals: &[&[u8]]) -> Vec<u8> {
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

/// One `<item>` parsed from a Newznab RSS/XML response (`Newznab::Result` in `Newznab.h`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewznabResult {
    pub title: Vec<u8>,
    pub link: Vec<u8>,
    pub size: i64,
    pub grabs: i32,
    pub date: i64,
    pub indexer: Vec<u8>,
}

/// A parsed Newznab RSS/XML response (`Newznab::Page` in `Newznab.h`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewznabPage {
    pub items: Vec<NewznabResult>,
    pub error: bool,
    pub error_code: i32,
    pub error_text: Vec<u8>,
}

fn trim_ascii_slice(text: &[u8]) -> Vec<u8> {
    match dupe_util_trim_bounds(text, b" \t\r\n") {
        Some((start, len)) => text[start..start + len].to_vec(),
        None => Vec::new(),
    }
}

fn normalize_xml_eol(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.as_bytes().contains(&b'\r') {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    std::borrow::Cow::Owned(out)
}

fn normalize_xml_attr_ws(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.bytes().any(|b| matches!(b, b'\r' | b'\n' | b'\t')) {
        return std::borrow::Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push(' ');
        } else if c == '\n' || c == '\t' {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    std::borrow::Cow::Owned(out)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum XmlSourceEncoding {
    ByteStream,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
}

fn transcode_xml_input(xml: &[u8]) -> Option<(std::borrow::Cow<'_, [u8]>, XmlSourceEncoding)> {
    if xml.is_empty() {
        return None;
    }
    if xml.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Some((
            std::borrow::Cow::Borrowed(&xml[3..]),
            XmlSourceEncoding::ByteStream,
        ));
    }
    if xml.starts_with(&[0xFF, 0xFE, 0x00, 0x00])
        || (xml.len() >= 4 && xml[0] != 0 && xml[1] == 0 && xml[2] == 0 && xml[3] == 0)
    {
        if xml.len() % 4 != 0 {
            return None;
        }
        let body = if xml.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
            &xml[4..]
        } else {
            xml
        };
        let mut out = String::with_capacity(body.len() / 4);
        for chunk in body.chunks_exact(4) {
            let code = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            out.push(char::from_u32(code)?);
        }
        return Some((
            std::borrow::Cow::Owned(out.into_bytes()),
            XmlSourceEncoding::Utf32Le,
        ));
    }
    if xml.starts_with(&[0x00, 0x00, 0xFE, 0xFF])
        || (xml.len() >= 4 && xml[0] == 0 && xml[1] == 0 && xml[2] == 0 && xml[3] != 0)
    {
        if xml.len() % 4 != 0 {
            return None;
        }
        let body = if xml.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
            &xml[4..]
        } else {
            xml
        };
        let mut out = String::with_capacity(body.len() / 4);
        for chunk in body.chunks_exact(4) {
            let code = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            out.push(char::from_u32(code)?);
        }
        return Some((
            std::borrow::Cow::Owned(out.into_bytes()),
            XmlSourceEncoding::Utf32Be,
        ));
    }
    if xml.starts_with(&[0xFF, 0xFE])
        || (xml.len() >= 4 && xml[0] != 0 && xml[1] == 0 && xml[2] != 0 && xml[3] == 0)
    {
        if xml.len() % 2 != 0 {
            return None;
        }
        let body = if xml.starts_with(&[0xFF, 0xFE]) {
            &xml[2..]
        } else {
            xml
        };
        let mut out = String::with_capacity(body.len() / 2);
        let iter = body
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]));
        for res in char::decode_utf16(iter) {
            out.push(res.ok()?);
        }
        return Some((
            std::borrow::Cow::Owned(out.into_bytes()),
            XmlSourceEncoding::Utf16Le,
        ));
    }
    if xml.starts_with(&[0xFE, 0xFF])
        || (xml.len() >= 4 && xml[0] == 0 && xml[1] != 0 && xml[2] == 0 && xml[3] != 0)
    {
        if xml.len() % 2 != 0 {
            return None;
        }
        let body = if xml.starts_with(&[0xFE, 0xFF]) {
            &xml[2..]
        } else {
            xml
        };
        let mut out = String::with_capacity(body.len() / 2);
        let iter = body
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]));
        for res in char::decode_utf16(iter) {
            out.push(res.ok()?);
        }
        return Some((
            std::borrow::Cow::Owned(out.into_bytes()),
            XmlSourceEncoding::Utf16Be,
        ));
    }
    Some((
        std::borrow::Cow::Borrowed(xml),
        XmlSourceEncoding::ByteStream,
    ))
}

fn validate_xml_decl_encoding(
    decl: &quick_xml::events::BytesDecl<'_>,
    source_enc: XmlSourceEncoding,
) -> bool {
    let Some(enc_res) = decl.encoding() else {
        return true;
    };
    let Ok(enc) = enc_res else {
        return false;
    };
    let lower: Vec<u8> = enc.iter().map(|b| b.to_ascii_lowercase()).collect();
    match source_enc {
        XmlSourceEncoding::Utf16Le => matches!(
            lower.as_slice(),
            b"utf-16" | b"utf-16le" | b"ucs-2" | b"iso-10646-ucs-2" | b"unicode"
        ),
        XmlSourceEncoding::Utf16Be => matches!(
            lower.as_slice(),
            b"utf-16" | b"utf-16be" | b"ucs-2" | b"iso-10646-ucs-2" | b"unicode"
        ),
        XmlSourceEncoding::Utf32Le => matches!(
            lower.as_slice(),
            b"utf-32" | b"utf-32le" | b"ucs-4" | b"iso-10646-ucs-4"
        ),
        XmlSourceEncoding::Utf32Be => matches!(
            lower.as_slice(),
            b"utf-32" | b"utf-32be" | b"ucs-4" | b"iso-10646-ucs-4"
        ),
        XmlSourceEncoding::ByteStream => !matches!(
            lower.as_slice(),
            b"utf-16"
                | b"utf-16le"
                | b"utf-16be"
                | b"ucs-2"
                | b"iso-10646-ucs-2"
                | b"unicode"
                | b"utf-32"
                | b"utf-32le"
                | b"utf-32be"
                | b"ucs-4"
                | b"iso-10646-ucs-4"
        ),
    }
}

fn decode_xml_bytes<'a>(
    raw: &'a [u8],
    decoder: quick_xml::Decoder,
    source_enc: XmlSourceEncoding,
) -> Option<std::borrow::Cow<'a, str>> {
    if source_enc == XmlSourceEncoding::ByteStream {
        decoder.decode(raw).ok()
    } else {
        std::str::from_utf8(raw)
            .ok()
            .map(std::borrow::Cow::Borrowed)
    }
}

/// Parses a Newznab RSS/XML response (`Newznab::ParseResponse` in `Newznab.cpp`),
/// rejecting empty documents, unclosed elements, and DTD `<!ENTITY` declarations.
pub fn parse_newznab_response(xml: &[u8]) -> Option<NewznabPage> {
    use quick_xml::events::Event;
    let (xml_utf8, source_enc) = transcode_xml_input(xml)?;
    let mut reader = quick_xml::Reader::from_reader(xml_utf8.as_ref());
    let mut page = NewznabPage::default();
    let mut item = NewznabResult::default();
    let mut text = Vec::new();
    let mut depth: i32 = 0;
    let mut in_item = false;
    let mut attr_size: i64 = 0;
    let mut element_size: i64 = 0;
    let mut saw_root = false;
    let mut root_closed = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if root_closed {
                    return None;
                }
                saw_root = true;
                let mut attrs = Vec::new();
                for attr_res in e.attributes() {
                    let a = attr_res.ok()?;
                    let decoded = decode_xml_bytes(&a.value, reader.decoder(), source_enc)?;
                    let norm = normalize_xml_attr_ws(&decoded);
                    let unescaped = quick_xml::escape::unescape(&norm).ok()?;
                    attrs.push((
                        a.key.local_name().as_ref().to_vec(),
                        unescaped.into_owned().into_bytes(),
                    ));
                }
                let local = e.local_name();
                depth += 1;
                text.clear();
                let find_attr = |k: &[u8]| -> &[u8] {
                    attrs
                        .iter()
                        .find(|(ak, _)| ak == k)
                        .map(|(_, av)| av.as_slice())
                        .unwrap_or(b"")
                };
                match newznab_classify_start_element(depth, in_item, local.as_ref()) {
                    1 => {
                        page.error = true;
                        page.error_code = c_atoi_bytes(find_attr(b"code"));
                        page.error_text = find_attr(b"description").to_vec();
                    }
                    2 => {
                        in_item = true;
                        item = NewznabResult::default();
                        attr_size = 0;
                        element_size = 0;
                    }
                    3 => {
                        let key = find_attr(b"name");
                        let val = find_attr(b"value");
                        match newznab_classify_item_attr_key(key) {
                            1 => attr_size = c_atoll_bytes(val),
                            2 => item.grabs = c_atoll_bytes(val) as i32,
                            3 => item.date = parse_newznab_date(val),
                            4 => item.indexer = val.to_vec(),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                if root_closed {
                    return None;
                }
                saw_root = true;
                let mut attrs = Vec::new();
                for attr_res in e.attributes() {
                    let a = attr_res.ok()?;
                    let decoded = decode_xml_bytes(&a.value, reader.decoder(), source_enc)?;
                    let norm = normalize_xml_attr_ws(&decoded);
                    let unescaped = quick_xml::escape::unescape(&norm).ok()?;
                    attrs.push((
                        a.key.local_name().as_ref().to_vec(),
                        unescaped.into_owned().into_bytes(),
                    ));
                }
                let local = e.local_name().as_ref().to_vec();
                depth += 1;
                text.clear();
                let find_attr = |k: &[u8]| -> &[u8] {
                    attrs
                        .iter()
                        .find(|(ak, _)| ak == k)
                        .map(|(_, av)| av.as_slice())
                        .unwrap_or(b"")
                };
                match newznab_classify_start_element(depth, in_item, &local) {
                    1 => {
                        page.error = true;
                        page.error_code = c_atoi_bytes(find_attr(b"code"));
                        page.error_text = find_attr(b"description").to_vec();
                    }
                    2 => {
                        in_item = true;
                        item = NewznabResult::default();
                        attr_size = 0;
                        element_size = 0;
                    }
                    3 => {
                        let key = find_attr(b"name");
                        let val = find_attr(b"value");
                        match newznab_classify_item_attr_key(key) {
                            1 => attr_size = c_atoll_bytes(val),
                            2 => item.grabs = c_atoll_bytes(val) as i32,
                            3 => item.date = parse_newznab_date(val),
                            4 => item.indexer = val.to_vec(),
                            _ => {}
                        }
                    }
                    _ => {}
                }
                match newznab_classify_end_element(in_item, &local) {
                    1 => item.title = trim_ascii_slice(&text),
                    2 => item.link = trim_ascii_slice(&text),
                    3 => element_size = c_atoll_bytes(&text),
                    4 => {
                        item.size = if attr_size > 0 {
                            attr_size
                        } else {
                            element_size
                        };
                        page.items.push(std::mem::take(&mut item));
                        in_item = false;
                    }
                    _ => {}
                }
                depth -= 1;
                if depth == 0 {
                    root_closed = true;
                }
                text.clear();
            }
            Ok(Event::Text(t)) => {
                let decoded = decode_xml_bytes(t.as_ref(), reader.decoder(), source_enc)?;
                let norm = normalize_xml_eol(&decoded);
                if depth == 0 {
                    let rest = norm.strip_prefix('\u{feff}').unwrap_or(&norm);
                    if !rest
                        .bytes()
                        .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                    {
                        return None;
                    }
                } else {
                    text.extend_from_slice(norm.as_bytes());
                }
            }
            Ok(Event::CData(c)) => {
                if depth == 0 {
                    return None;
                }
                let decoded = decode_xml_bytes(c.as_ref(), reader.decoder(), source_enc)?;
                let norm = normalize_xml_eol(&decoded);
                text.extend_from_slice(norm.as_bytes());
            }
            Ok(Event::GeneralRef(r)) => {
                if depth == 0 {
                    return None;
                }
                let ent = match r.as_ref() {
                    b"lt" => Some(b"<".to_vec()),
                    b"gt" => Some(b">".to_vec()),
                    b"amp" => Some(b"&".to_vec()),
                    b"quot" => Some(b"\"".to_vec()),
                    b"apos" => Some(b"'".to_vec()),
                    _ => r
                        .resolve_char_ref()
                        .ok()
                        .flatten()
                        .map(|c| c.to_string().into_bytes()),
                };
                if let Some(bytes) = ent {
                    text.extend_from_slice(&bytes);
                } else {
                    return None;
                }
            }
            Ok(Event::Decl(ref decl)) => {
                if saw_root || !validate_xml_decl_encoding(decl, source_enc) {
                    return None;
                }
            }
            Ok(Event::DocType(d)) => {
                if saw_root || d.windows(8).any(|w| w.eq_ignore_ascii_case(b"<!entity")) {
                    return None;
                }
            }
            Ok(Event::End(e)) => {
                let local = e.local_name();
                match newznab_classify_end_element(in_item, local.as_ref()) {
                    1 => item.title = trim_ascii_slice(&text),
                    2 => item.link = trim_ascii_slice(&text),
                    3 => element_size = c_atoll_bytes(&text),
                    4 => {
                        item.size = if attr_size > 0 {
                            attr_size
                        } else {
                            element_size
                        };
                        page.items.push(std::mem::take(&mut item));
                        in_item = false;
                    }
                    _ => {}
                }
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                if depth == 0 {
                    root_closed = true;
                }
                text.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }

    (saw_root && root_closed && depth == 0).then_some(page)
}

/// First segment of a file in an NZB summary (`NzbSummary::Head` in `NzbReader.h`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NzbSummaryHead {
    pub name: Vec<u8>,
    pub id: Vec<u8>,
    pub bytes: i64,
}

/// Parsed NZB summary (`NzbSummary` in `NzbReader.h`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NzbSummaryData {
    pub poster: Vec<u8>,
    pub groups: std::collections::BTreeSet<Vec<u8>>,
    pub meta: std::collections::BTreeMap<Vec<u8>, Vec<u8>>,
    pub filenames: std::collections::BTreeSet<Vec<u8>>,
    pub message_ids: Vec<Vec<u8>>,
    pub data_message_ids: Vec<Vec<u8>>,
    pub heads: Vec<NzbSummaryHead>,
    pub main_name: Vec<u8>,
    pub par_index: NzbSummaryHead,
    pub recovery_bytes: Vec<u64>,
    pub files: i32,
    pub total_bytes: i64,
    pub data_files: i32,
    pub data_bytes: i64,
    pub listed_parts: u64,
    pub declared_parts: u64,
    pub data_listed_parts: u64,
    pub data_declared_parts: u64,
    pub article_bytes: u64,
    pub geometry_valid: bool,
}

impl NzbSummaryData {
    /// 16-byte lowercase hex FNV-1a fingerprint over `message_ids` (`NzbSummary::Fingerprint`).
    pub fn fingerprint(&self) -> Vec<u8> {
        let refs: Vec<&[u8]> = self.message_ids.iter().map(|v| v.as_slice()).collect();
        fnv1a_fingerprint(&refs).to_vec()
    }
}

fn declared_parts_c_locale(subject: &[u8]) -> i32 {
    let s = &subject[..subject
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(subject.len())];
    let is_sp = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let is_dg = |b: u8| b.is_ascii_digit();
    let char_val = |b: u8| (b as std::ffi::c_char) as i32;

    let mut end = s.len();
    while end > 0 && is_sp(s[end - 1]) {
        end -= 1;
    }
    if end == 0 || s[end - 1] != b')' {
        return 0;
    }
    let mut p = end - 1;
    let mut total: i64 = 0;
    let mut scale: i64 = 1;
    while p > 0 && is_dg(s[p - 1]) && scale <= 1_000_000 {
        p -= 1;
        total += (char_val(s[p]) - b'0' as i32) as i64 * scale;
        scale *= 10;
    }
    if scale == 1 || p == 0 || s[p - 1] != b'/' {
        return 0;
    }
    p -= 1;
    let digits = p;
    while p > 0 && is_dg(s[p - 1]) {
        p -= 1;
    }
    if p == digits || p == 0 || s[p - 1] != b'(' {
        return 0;
    }
    if total <= 1_000_000 {
        total as i32
    } else {
        0
    }
}

fn cstring_trunc(bytes: &[u8]) -> std::ffi::CString {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    std::ffi::CString::new(&bytes[..end]).unwrap_or_default()
}

/// Parses an NZB XML document into an `NzbSummaryData` (`NzbReader::Parse` in `NzbReader.cpp`).
pub fn parse_nzb_summary(xml: &[u8]) -> Option<NzbSummaryData> {
    use quick_xml::events::Event;
    use std::collections::{BTreeMap, BTreeSet};

    let (xml_utf8, source_enc) = transcode_xml_input(xml)?;
    let mut reader = quick_xml::Reader::from_reader(xml_utf8.as_ref());
    let mut out = NzbSummaryData {
        geometry_valid: true,
        ..Default::default()
    };
    let mut sizes: BTreeMap<Vec<u8>, i64> = BTreeMap::new();
    let mut ids: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut data_ids: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut article_sizes: Vec<u64> = Vec::new();
    let mut parity_heads: Vec<NzbSummaryHead> = Vec::new();
    let mut data_files: i32 = 0;
    let mut data_bytes: i64 = 0;
    let mut listed_parts: u64 = 0;
    let mut declared_parts: u64 = 0;
    let mut data_listed_parts: u64 = 0;
    let mut data_declared_parts: u64 = 0;
    let mut geometry_valid = true;

    let mut m_text: Vec<u8> = Vec::new();
    let mut m_meta_type: Vec<u8> = Vec::new();
    let mut m_name: Vec<u8> = Vec::new();
    let mut m_subject: Vec<u8> = Vec::new();
    let mut m_first_id: Vec<u8> = Vec::new();
    let mut m_segments: Vec<(i64, i64, Vec<u8>)> = Vec::new();
    let mut m_parts: Vec<(i64, i64)> = Vec::new();
    let mut m_file_bytes: i64 = 0;
    let mut m_valid_parts: usize = 0;
    let mut m_seg_bytes: i64 = 0;
    let mut m_seg_number: i64 = 0;
    let mut m_in_file = false;
    let mut m_in_segment = false;

    let mut depth: i32 = 0;
    let mut saw_root = false;
    let mut root_closed = false;
    let mut buf = Vec::new();

    macro_rules! handle_start {
        ($local:expr, $attrs:expr) => {{
            let local_bytes: &[u8] = $local;
            let attrs_slice: &[(Vec<u8>, Vec<u8>)] = $attrs;
            m_text.clear();
            let find_attr = |k: &[u8]| -> &[u8] {
                attrs_slice
                    .iter()
                    .find(|(ak, _)| ak == k)
                    .map(|(_, av)| av.as_slice())
                    .unwrap_or(b"")
            };
            match nzb_reader_classify_start_element(local_bytes, m_in_file) {
                1 => {
                    let ty = find_attr(b"type");
                    if !ty.is_empty() {
                        m_meta_type = dupe_util_lower(ty);
                    }
                }
                2 => {
                    out.files += 1;
                    if out.poster.is_empty() {
                        out.poster = find_attr(b"poster").to_vec();
                    }
                    let subj = find_attr(b"subject");
                    let (s_start, s_end) = name_of_subject_range(subj);
                    m_name = subj[s_start..s_end].to_vec();
                    m_subject = subj.to_vec();
                    m_first_id.clear();
                    m_segments.clear();
                    m_parts.clear();
                    m_file_bytes = 0;
                    m_valid_parts = 0;
                    m_in_file = true;
                }
                3 => {
                    m_seg_bytes = c_atoll_bytes(find_attr(b"bytes"));
                    m_seg_number = c_atoll_bytes(find_attr(b"number"));
                    m_in_segment = true;
                }
                _ => {}
            }
        }};
    }

    macro_rules! finish_element {
        ($local:expr) => {{
            let local_bytes: &[u8] = $local;
            match nzb_reader_classify_end_element(
                local_bytes,
                !m_meta_type.is_empty(),
                m_in_segment,
                m_in_file,
            ) {
                1 => {
                    out.meta
                        .insert(std::mem::take(&mut m_meta_type), trim_ascii_slice(&m_text));
                }
                2 => {
                    let id = trim_ascii_slice(&m_text);
                    m_segments.push((m_seg_number, m_seg_bytes, id));
                    m_in_segment = false;
                }
                3 => {
                    let group = trim_ascii_slice(&m_text);
                    if !group.is_empty() {
                        out.groups.insert(group);
                    }
                }
                4 => {
                    m_segments.sort_by_key(|s| s.0);
                    let is_par = is_par_name(&m_name);
                    let seg_len = m_segments.len();
                    for i in 0..seg_len {
                        if i + 1 < seg_len && m_segments[i + 1].0 == m_segments[i].0 {
                            continue;
                        }
                        let (seg_num, seg_bytes, ref seg_id) = m_segments[i];
                        m_parts.push((seg_num, seg_bytes));
                        if !valid_message_id(seg_id) {
                            if !is_par {
                                geometry_valid = false;
                            }
                            continue;
                        }
                        let entry = sizes.entry(m_name.clone()).or_insert(0);
                        *entry = entry.wrapping_add(seg_bytes);
                        ids.insert(seg_id.clone());
                        if !is_par {
                            data_ids.insert(seg_id.clone());
                        }
                        m_valid_parts += 1;
                        let (new_file_bytes, new_geom, push_art) =
                            accum_segment_bytes(m_file_bytes, seg_bytes, is_par, geometry_valid);
                        m_file_bytes = new_file_bytes;
                        geometry_valid = new_geom;
                        if push_art {
                            article_sizes.push(seg_bytes as u64);
                        }
                        if seg_num == 1 {
                            m_first_id = seg_id.clone();
                        }
                    }
                    let count = declared_parts_c_locale(&m_subject);
                    let (_file_listed, file_total) = crate::dupewatch::part_counts(
                        &m_parts,
                        if count > 0 { count as u32 } else { 0 },
                        is_par,
                    );
                    listed_parts = listed_parts.wrapping_add(m_valid_parts as u64);
                    declared_parts = declared_parts.wrapping_add(file_total);
                    if !is_par {
                        data_listed_parts = data_listed_parts.wrapping_add(m_valid_parts as u64);
                        data_declared_parts = data_declared_parts.wrapping_add(file_total);
                    }
                    let (new_df, new_db, new_geom2, push_par_head, push_data_head) =
                        accum_file_data_totals(
                            is_par,
                            m_file_bytes,
                            m_first_id.is_empty(),
                            data_files,
                            data_bytes,
                            geometry_valid,
                        );
                    data_files = new_df;
                    data_bytes = new_db;
                    geometry_valid = new_geom2;
                    if push_par_head {
                        parity_heads.push(NzbSummaryHead {
                            name: m_name.clone(),
                            id: m_first_id.clone(),
                            bytes: m_file_bytes,
                        });
                    }
                    if push_data_head {
                        out.heads.push(NzbSummaryHead {
                            name: m_name.clone(),
                            id: m_first_id.clone(),
                            bytes: m_file_bytes,
                        });
                    }
                    m_in_file = false;
                    m_in_segment = false;
                }
                _ => {}
            }
            m_text.clear();
        }};
    }

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if root_closed {
                    return None;
                }
                saw_root = true;
                let mut attrs = Vec::new();
                for attr_res in e.attributes() {
                    let a = attr_res.ok()?;
                    let decoded = decode_xml_bytes(&a.value, reader.decoder(), source_enc)?;
                    let norm = normalize_xml_attr_ws(&decoded);
                    let unescaped = quick_xml::escape::unescape(&norm).ok()?;
                    attrs.push((
                        a.key.local_name().as_ref().to_vec(),
                        unescaped.into_owned().into_bytes(),
                    ));
                }
                let local = e.local_name();
                depth += 1;
                handle_start!(local.as_ref(), &attrs);
            }
            Ok(Event::Empty(e)) => {
                if root_closed {
                    return None;
                }
                saw_root = true;
                let mut attrs = Vec::new();
                for attr_res in e.attributes() {
                    let a = attr_res.ok()?;
                    let decoded = decode_xml_bytes(&a.value, reader.decoder(), source_enc)?;
                    let norm = normalize_xml_attr_ws(&decoded);
                    let unescaped = quick_xml::escape::unescape(&norm).ok()?;
                    attrs.push((
                        a.key.local_name().as_ref().to_vec(),
                        unescaped.into_owned().into_bytes(),
                    ));
                }
                let local = e.local_name().as_ref().to_vec();
                depth += 1;
                handle_start!(&local, &attrs);
                finish_element!(&local);
                depth -= 1;
                if depth == 0 {
                    root_closed = true;
                }
            }
            Ok(Event::Text(t)) => {
                let decoded = decode_xml_bytes(t.as_ref(), reader.decoder(), source_enc)?;
                let norm = normalize_xml_eol(&decoded);
                if depth == 0 {
                    let rest = norm.strip_prefix('\u{feff}').unwrap_or(&norm);
                    if !rest
                        .bytes()
                        .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                    {
                        return None;
                    }
                } else {
                    m_text.extend_from_slice(norm.as_bytes());
                }
            }
            Ok(Event::CData(c)) => {
                if depth == 0 {
                    return None;
                }
                let decoded = decode_xml_bytes(c.as_ref(), reader.decoder(), source_enc)?;
                let norm = normalize_xml_eol(&decoded);
                m_text.extend_from_slice(norm.as_bytes());
            }
            Ok(Event::GeneralRef(r)) => {
                if depth == 0 {
                    return None;
                }
                let ent = match r.as_ref() {
                    b"lt" => Some(b"<".to_vec()),
                    b"gt" => Some(b">".to_vec()),
                    b"amp" => Some(b"&".to_vec()),
                    b"quot" => Some(b"\"".to_vec()),
                    b"apos" => Some(b"'".to_vec()),
                    _ => r
                        .resolve_char_ref()
                        .ok()
                        .flatten()
                        .map(|c| c.to_string().into_bytes()),
                };
                if let Some(bytes) = ent {
                    m_text.extend_from_slice(&bytes);
                } else {
                    return None;
                }
            }
            Ok(Event::Decl(ref decl)) => {
                if saw_root || !validate_xml_decl_encoding(decl, source_enc) {
                    return None;
                }
            }
            Ok(Event::DocType(d)) => {
                if saw_root || d.windows(8).any(|w| w.eq_ignore_ascii_case(b"<!entity")) {
                    return None;
                }
            }
            Ok(Event::End(e)) => {
                let local = e.local_name().as_ref().to_vec();
                finish_element!(&local);
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                if depth == 0 {
                    root_closed = true;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }

    if !(saw_root && root_closed && depth == 0) || out.files == 0 || ids.is_empty() {
        return None;
    }

    out.listed_parts = listed_parts;
    out.declared_parts = declared_parts;
    out.data_listed_parts = data_listed_parts;
    out.data_declared_parts = data_declared_parts;
    if !article_sizes.is_empty() {
        let mid = article_sizes.len() / 2;
        let (_, median, _) = article_sizes.select_nth_unstable(mid);
        out.article_bytes = *median;
    }
    out.message_ids = ids.into_iter().collect();
    out.data_message_ids = data_ids.into_iter().collect();

    for head in &parity_heads {
        let head_c = cstring_trunc(&head.name);
        if let Some((base_len, _blocks)) =
            crate::parparser::parse_par_filename(head_c.as_bytes(), true)
        {
            if base_len + 5 == head.name.len() {
                if !out.par_index.id.is_empty() {
                    let idx_c = cstring_trunc(&out.par_index.name);
                    if !crate::parparser::same_par_collection(
                        idx_c.as_bytes(),
                        head_c.as_bytes(),
                        true,
                    ) {
                        out.par_index = NzbSummaryHead::default();
                        break;
                    }
                }
                if out.par_index.id.is_empty() {
                    out.par_index = head.clone();
                }
            }
        }
    }
    if !out.par_index.id.is_empty() {
        let idx_c = cstring_trunc(&out.par_index.name);
        for head in &parity_heads {
            let head_c = cstring_trunc(&head.name);
            if let Some((base_len, _blocks)) =
                crate::parparser::parse_par_filename(head_c.as_bytes(), true)
            {
                if base_len + 5 < head.name.len()
                    && crate::parparser::same_par_collection(
                        idx_c.as_bytes(),
                        head_c.as_bytes(),
                        true,
                    )
                {
                    out.recovery_bytes.push(head.bytes as u64);
                }
            }
        }
    }

    let mut size_names: Vec<Vec<u8>> = Vec::with_capacity(sizes.len());
    let mut size_vals: Vec<i64> = Vec::with_capacity(sizes.len());
    let mut size_is_par: Vec<u8> = Vec::with_capacity(sizes.len());
    for (name, bytes) in sizes {
        out.filenames.insert(dupe_util_lower(&name));
        size_is_par.push(u8::from(is_par_name(&name)));
        size_names.push(name);
        size_vals.push(bytes);
    }
    let (total_bytes, chosen_idx) = select_main_file(&size_vals, &size_is_par);
    out.total_bytes = total_bytes;
    if chosen_idx >= 0 && (chosen_idx as usize) < size_names.len() {
        out.main_name = std::mem::take(&mut size_names[chosen_idx as usize]);
    }

    out.geometry_valid = geometry_valid;
    if out.geometry_valid {
        out.data_files = data_files;
        out.data_bytes = data_bytes;
        let head_bytes: Vec<i64> = out.heads.iter().map(|h| h.bytes).collect();
        let mut top_idx = [0usize; 3];
        let kept = select_top_heads(&head_bytes, &mut top_idx);
        let mut top_heads = Vec::with_capacity(kept);
        for &idx in &top_idx[..kept] {
            top_heads.push(out.heads[idx].clone());
        }
        out.heads = top_heads;
    } else {
        out.heads.clear();
    }

    Some(out)
}

/// Reads a temporary HTTP response file if its size is `<= max_bytes`, and deletes the file
/// unconditionally (`HttpGet::Fetch`).
pub fn http_get_read_and_delete(path: &[u8], max_bytes: usize, read_body: bool) -> Option<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    let fs_path = std::path::Path::new(std::ffi::OsStr::from_bytes(path));
    let result = if read_body {
        match std::fs::metadata(fs_path) {
            Ok(meta) if meta.len() <= max_bytes as u64 => std::fs::read(fs_path).ok(),
            _ => None,
        }
    } else {
        None
    };
    let _ = std::fs::remove_file(fs_path);
    result
}

pub fn parse_atoi_bytes(bytes: &[u8]) -> i32 {
    c_atoi_bytes(bytes)
}

pub fn url_encode_percent(text: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = Vec::with_capacity(text.len());
    for &ch in text {
        if ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_' || ch == b'.' || ch == b'~' {
            out.push(ch);
        } else {
            out.push(b'%');
            out.push(HEX[(ch >> 4) as usize]);
            out.push(HEX[(ch & 0x0F) as usize]);
        }
    }
    out
}

pub fn build_newznab_url_percent(
    base: &[u8],
    param_keys: &[&[u8]],
    param_vals: &[&[u8]],
    api_key: &[u8],
) -> Vec<u8> {
    let mut url = base.to_vec();
    if let Some(scheme) = base.windows(3).position(|w| w == b"://") {
        let after_scheme = scheme + 3;
        let host_end = base[after_scheme..]
            .iter()
            .position(|&b| b == b'/' || b == b'?')
            .map(|p| after_scheme + p)
            .unwrap_or(base.len());
        let query_start = base[host_end..]
            .iter()
            .position(|&b| b == b'?')
            .map(|p| host_end + p)
            .unwrap_or(base.len());
        let path = &base[host_end..query_start];
        if path.is_empty() || path == b"/" {
            url.clear();
            url.extend_from_slice(&base[..host_end]);
            url.extend_from_slice(b"/api");
            url.extend_from_slice(&base[query_start..]);
        }
    }
    if url.contains(&b'?') {
        url.push(b'&');
    } else {
        url.push(b'?');
    }
    let count = param_keys.len().min(param_vals.len());
    for i in 0..count {
        url.extend_from_slice(&url_encode_percent(param_keys[i]));
        url.push(b'=');
        url.extend_from_slice(&url_encode_percent(param_vals[i]));
        url.push(b'&');
    }
    url.extend_from_slice(b"apikey=");
    url.extend_from_slice(&url_encode_percent(api_key));
    url
}

pub fn mark_searching_pending(
    dir: &[u8],
    job_name: &[u8],
    job_dupe_key: &[u8],
    rank_only: bool,
) -> bool {
    if dir.is_empty() {
        return false;
    }
    let dir_path = dupe_bytes_to_path(dir);
    let _ = std::fs::remove_dir_all(dir_path);
    if std::fs::create_dir_all(dir_path).is_err() {
        return false;
    }
    let norm_title = crate::releasename::normalize(job_name);
    let low_key = lower_key(job_dupe_key);
    let phase: &[u8] = if rank_only {
        b"ranking\n"
    } else {
        b"searching\n"
    };
    dupe_write_atomic(&dir_path.join("pick-title"), &norm_title)
        && dupe_write_atomic(&dir_path.join("pick-key"), &low_key)
        && dupe_write_atomic(&dir_path.join("phase"), phase)
}

pub fn save_pending_header(
    dir: &[u8],
    job_name: &[u8],
    job_dupe_key: &[u8],
    pick_fingerprint: &[u8],
) -> bool {
    if dir.is_empty() {
        return false;
    }
    let dir_path = dupe_bytes_to_path(dir);
    let norm_title = crate::releasename::normalize(job_name);
    let low_key = lower_key(job_dupe_key);
    dupe_write_atomic(&dir_path.join("pick-title"), &norm_title)
        && dupe_write_atomic(&dir_path.join("pick-key"), &low_key)
        && dupe_write_atomic(&dir_path.join("pick-posting"), pick_fingerprint)
}

#[allow(clippy::too_many_arguments)]
pub fn save_pending_item(
    dir: &[u8],
    idx: usize,
    title: &[u8],
    indexer: &[u8],
    grabs: i32,
    size: i64,
    date: i64,
    data: &[u8],
    same_inner: bool,
) -> bool {
    if dir.is_empty() {
        return false;
    }
    let dir_path = dupe_bytes_to_path(dir);
    let meta = format_pending_meta(title, indexer, grabs, size, date);
    let inner: &[u8] = if same_inner { b"1\n" } else { b"0\n" };
    dupe_write_atomic(&dir_path.join(format!("{idx}.nzb")), data)
        && dupe_write_atomic(&dir_path.join(format!("{idx}.meta")), &meta)
        && dupe_write_atomic(&dir_path.join(format!("{idx}.inner")), inner)
}

pub fn save_pending_finish(dir: &[u8]) -> bool {
    if dir.is_empty() {
        return false;
    }
    let dir_path = dupe_bytes_to_path(dir);
    dupe_write_atomic(&dir_path.join("phase"), b"fetched\n")
}

pub fn check_pending_identity(
    dir: &[u8],
    job_name: &[u8],
    job_dupe_key: &[u8],
    pick_fingerprint: &[u8],
) -> bool {
    if dir.is_empty() {
        return false;
    }
    let dir_path = dupe_bytes_to_path(dir);
    let title_path = dir_path.join("pick-title");
    if !std::fs::metadata(&title_path).is_ok_and(|m| !m.is_dir()) {
        return false;
    }
    let saved_title = std::fs::read(&title_path).unwrap_or_default();
    let saved_key = std::fs::read(dir_path.join("pick-key")).unwrap_or_default();
    let saved_posting = std::fs::read(dir_path.join("pick-posting")).unwrap_or_default();
    let cur_norm_title = crate::releasename::normalize(job_name);
    let cur_lower_key = lower_key(job_dupe_key);
    dupesearch_pending_identity_matches(
        true,
        &saved_title,
        &cur_norm_title,
        &saved_key,
        &cur_lower_key,
        &saved_posting,
        pick_fingerprint,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strips_only_complete_terminal_yenc() {
        for (input, want) in [
            (&b" \"\" data.bin yEnc (1/2) \t"[..], &b"\"\" data.bin"[..]),
            (&b"data.bin\tYeNc\x0b(01/002)\x0c"[..], &b"data.bin"[..]),
            (&b"data.bin yEnc"[..], &b"data.bin"[..]),
            (&b"a yEnc (1/2)\0tail"[..], &b"a yEnc (1/2)\0tail"[..]),
            (&b"data.bin yEnc(1/2)"[..], &b"data.bin yEnc(1/2)"[..]),
            (&b"data.bin yEnc (1/)"[..], &b"data.bin yEnc (1/)"[..]),
            (
                &b"data.bin yEnc (1/2)junk"[..],
                &b"data.bin yEnc (1/2)junk"[..],
            ),
            (&b"\x0bdata.bin\x0c"[..], &b"\x0bdata.bin\x0c"[..]),
            (&b"\"unterminated yEnc"[..], &b"\"unterminated"[..]),
        ] {
            let (start, end) = unquoted_subject_range(input);
            assert_eq!(&input[start..end], want, "{input:?}");
        }
    }
    #[test]
    fn only_terminal_par2_extension_classifies_recovery() {
        assert!(is_par_name(b"x.PaR2"));
        assert!(is_par_name(b".par2"));
        for name in [b"par2".as_slice(), b"a.vol1+2.mkv", b"a.par2 ", b"a.par2\0"] {
            assert!(!is_par_name(name));
        }
    }

    #[test]
    fn test_dupesearch_helpers() {
        assert_eq!(lower_key(b"Dupes:My.Release\t\n"), b"dupes:my.release  ");
        assert!(saved_history_backup(true, 100, 3, 200, 3, 6));
        assert!(saved_history_backup(true, 100, 6, 200, 3, 6));
        assert!(!saved_history_backup(true, 100, 6, 100, 3, 6));

        let mut order = [0usize; 3];
        sort_verified_postings(
            &[1050, 1000, 1000],
            &[5, 2, 10],
            &[100, 200, 150],
            1000,
            &mut order,
        );
        assert_eq!(order, [2, 1, 0]);

        assert_eq!(
            eval_member_rerank(1000, 50, 1, 1020, 0.90, false),
            (1020, 90, false)
        );
        assert_eq!(
            eval_member_rerank(1000, 50, 1, 1020, 1.00, false),
            (1050, 100, false)
        );
        assert_eq!(
            eval_member_rerank(1000, 1, 1, 1020, 0.0, true),
            (1020, 0, true)
        );

        let meta = format_pending_meta(b"Release.A", b"NZBGeek", 42, 9876543210, 1749517805);
        let parsed = parse_pending_meta(&meta);
        assert_eq!(
            &meta[parsed.title_offset..parsed.title_offset + parsed.title_len],
            b"Release.A"
        );
        assert_eq!(
            &meta[parsed.indexer_offset..parsed.indexer_offset + parsed.indexer_len],
            b"NZBGeek"
        );
        assert_eq!(parsed.grabs, 42);
        assert_eq!(parsed.size, 9876543210);
        assert_eq!(parsed.date, 1749517805);

        let st = parse_dupesearch_state_line(
            b"dupes:rel\t1000\t1700000000\trel\t0\t",
            1700000100,
            86400,
        );
        assert_eq!(st.valid, 1);
        assert_eq!(st.within_ttl, 1);
        assert_eq!(st.score, 1000);
        assert_eq!(st.retryable, 0);
    }

    #[test]
    fn fleet_member_skip_reason_parks_nothing_without_dupecheck() {
        assert!(fleet_member_skip_reason(true, 0, false).is_none());
        assert!(fleet_member_skip_reason(true, 0, true).is_none());
        assert!(fleet_member_skip_reason(true, 7, true).is_none());
        assert!(fleet_member_skip_reason(false, 0, false).is_none());
        assert!(fleet_member_skip_reason(false, 0, true).is_some());
        assert!(fleet_member_skip_reason(false, 7, false).is_some());
    }

    #[test]
    fn test_fleet_and_donor_health() {
        let (alive, dead, complete) = eval_fleet_member_health(true, true, 95, 5, 0, 5, 80);
        assert_eq!((alive, dead, complete), (95, false, true));

        let mut ranked = [0usize; 3];
        let count = rank_fleet_candidates(
            &[0, 0, 0],
            &[-1, 0, -1],
            &[0, 0, 0],
            &[90, 90, 100],
            &[1000, 1000, 1000],
            &[2, 2, 2],
            &mut ranked,
        );
        assert_eq!(count, 3);
        assert_eq!(ranked, [2, 0, 1]);

        // TWINs by health, then borrowable, then unborrowable ALTs; a dead or
        // unchecked member stays behind every usable one whatever its kind
        let mut ranked = [0usize; 6];
        let count = rank_fleet_candidates(
            &[0, 0, 0, 0, 0, 0],
            &[-1, -1, -1, -1, -1, -1],
            &[0, 0, 0, 0, 1, 0],
            &[100, 70, 90, 99, 80, -1],
            &[1000, 1000, 1000, 1000, 1000, 1000],
            &[3, 0, 0, 1, 0, 0],
            &mut ranked,
        );
        assert_eq!(count, 6);
        assert_eq!(ranked, [2, 1, 3, 0, 5, 4]);

        assert_eq!(donor_sample_count(5000, 2, 20, 300), 100);
        assert_eq!(adjust_pipeline_depth(4, 500, 32), 8);
        assert_eq!(adjust_pipeline_depth(8, 2500, 32), 4);
        assert_eq!(eval_article_votes(&[2, 1]), 1);
        assert_eq!(eval_article_votes(&[2, 4]), 2);
        assert_eq!(eval_article_votes(&[4, 4]), 3);
        assert_eq!(resolve_unsettled_vote(&[2, 0]), 2);
    }

    #[test]
    fn test_newznab_date_url_and_mask() {
        assert_eq!(
            parse_newznab_date(b"Tue, 10 Jun 2025 01:10:05 +0000"),
            1749517805
        );
        assert_eq!(
            parse_newznab_date(b"Tue, 10 Jun 2025 01:10:05+0000"),
            1749517805
        );
        assert_eq!(
            parse_newznab_date(b"10 Jun 2025 01:10:05EST"),
            1749517805 + 5 * 3600
        );
        assert_eq!(
            parse_newznab_date(b"10 Jun 2025 01:10:05 EST"),
            1749517805 + 5 * 3600
        );
        assert_eq!(parse_newznab_date(b"2025-06-10T03:10:05+02:00"), 1749517805);
        assert_eq!(parse_newznab_date(b"10 Jun 2025 01:10:05 XYZ"), 0);

        let url = build_newznab_url(
            b"http://h:5076",
            &[b"t", b"q"],
            &[b"search", b"show s01e02"],
            b"k&y 1",
        );
        assert_eq!(
            url,
            b"http://h:5076/api?t=search&q=show+s01e02&apikey=k%26y+1"
        );

        assert_eq!(
            mask_newznab_text(b"http://u:p@h/api?APIKEY=SECRET&q=x"),
            b"http://***@h/api?APIKEY=***&q=x"
        );
    }

    #[test]
    fn newznab_full_suite_port_from_cpp() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/testdata/dupesearch");

        // NewznabParseSearchPageTest
        let search_xml = std::fs::read(root.join("search_industry_s03e07.xml")).unwrap();
        let page = parse_newznab_response(&search_xml).unwrap();
        assert!(!page.error);
        assert_eq!(page.items.len(), 25);

        let first = &page.items[0];
        assert_eq!(
            first.title,
            b"Industry.S03E07.Useful.Idiot.2160p.MAX.WEB-DL.DDP5.1.Atmos.DV.H.265-FLUX"
        );
        assert_eq!(
            first.link,
            b"http://127.0.0.1:5076/getnzb/api/2114962605753885140?apikey=APIKEY"
        );
        assert_eq!(first.size, 10896716818);
        assert_eq!(first.grabs, 111);
        assert_eq!(first.date, 1749517805);
        assert_eq!(first.indexer, b"altHUB");

        let second = &page.items[1];
        assert_eq!(
            second.title,
            b"Industry.S03E07.Useful.Idiot.2160p.MAX.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265-FLUX"
        );
        assert_eq!(second.size, 10893813254);
        assert_eq!(second.grabs, 0);
        assert_eq!(second.date, 1728651862);
        assert_eq!(second.indexer, b"Square Eyed");

        let last = &page.items[24];
        assert_eq!(
            last.title,
            b"Industry S03E07 Useful Idiot 2160p MAX WEB-DL DDP5 1 Atmos DV H 265-FLUX"
        );
        assert_eq!(last.size, 11692039895);
        assert_eq!(last.grabs, 6);
        assert_eq!(last.indexer, b"miatrix");
        assert_eq!(page.items[5].date, 0);
        assert_eq!(page.items[10].date, 0);
        assert!(page.items[0].date > 0);

        // NewznabParseErrorTest
        let err_xml = std::fs::read(root.join("error_bad_apikey.xml")).unwrap();
        let err_page = parse_newznab_response(&err_xml).unwrap();
        assert!(err_page.error);
        assert_eq!(err_page.error_code, 100);
        assert_eq!(err_page.error_text, b"Wrong api key");
        assert!(err_page.items.is_empty());

        let limit_page = parse_newznab_response(
            b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<error code=\"429\" description=\"Request limit reached\"/>",
        )
        .unwrap();
        assert!(limit_page.error);
        assert_eq!(limit_page.error_code, 429);

        // NewznabParseSizeAndMalformedTest
        let sz_page = parse_newznab_response(
            b"<rss xmlns:newznab=\"n\"><channel><item><title> A </title><link>http://x/1</link><size>100</size></item><item><title>B</title><link>http://x/2</link><size>100</size><newznab:attr name=\"size\" value=\"200\"/></item><item><title>C</title><link>http://x/3</link></item></channel></rss>",
        )
        .unwrap();
        assert_eq!(sz_page.items.len(), 3);
        assert_eq!(sz_page.items[0].title, b"A");
        assert_eq!(sz_page.items[0].size, 100);
        assert_eq!(sz_page.items[1].size, 200);
        assert_eq!(sz_page.items[2].size, 0);
        assert!(parse_newznab_response(b"").is_none());
        assert!(parse_newznab_response(b"<html><body>Forbidden").is_none());
        assert!(parse_newznab_response(b"<rss><channel><item>").is_none());

        // NewznabRejectsEntityDeclarationsTest
        let bomb = b"<?xml version=\"1.0\"?><!DOCTYPE n [<!ENTITY a \"aaaa\"><!ENTITY b \"&a;&a;&a;&a;\">]><rss><channel><item><title>&b;</title><link>http://x/1</link></item></channel></rss>";
        assert!(parse_newznab_response(bomb).is_none());
        let ok_doctype = parse_newznab_response(
            b"<?xml version=\"1.0\"?><!DOCTYPE rss PUBLIC \"-//x//DTD//EN\" \"http://x/dtd\"><rss><channel><item><title>T</title><link>http://x/1</link></item></channel></rss>",
        )
        .unwrap();
        assert_eq!(ok_doctype.items.len(), 1);

        // NewznabParseDateTest
        assert_eq!(parse_newznab_date(b"Thu, 01 Jan 1970 00:00:00 GMT"), 0);
        assert_eq!(parse_newznab_date(b"Thu, 01 Jan 1970 01:00:00 +0100"), 0);
        assert_eq!(
            parse_newznab_date(b"10 Jun 2025 01:10:05 -0200"),
            1749517805 + 7200
        );
        assert_eq!(
            parse_newznab_date(b"Sun, 22 Sep 2024 00:00:00 +0000"),
            1726963200
        );
        assert_eq!(
            parse_newznab_date(b"10 Jun 2025 01:10:05 EDT"),
            1749517805 + 4 * 3600
        );
        assert_eq!(
            parse_newznab_date(b"10 Jun 2025 01:10:05 PST"),
            1749517805 + 8 * 3600
        );
        assert_eq!(
            parse_newznab_date(b"10 Jun 2025 01:10:05 CDT"),
            1749517805 + 5 * 3600
        );
        assert_eq!(parse_newznab_date(b"10 Jun 2025 01:10:05 UT"), 1749517805);
        assert_eq!(parse_newznab_date(b"10 Jun 2025 01:10:05 Z"), 1749517805);
        assert_eq!(
            parse_newznab_date(b"Tue, 10 Jun 25 01:10:05 +0000"),
            1749517805
        );
        assert_eq!(parse_newznab_date(b"2025-06-10T01:10:05Z"), 1749517805);
        assert_eq!(parse_newznab_date(b"2025-06-10 01:10:05"), 1749517805);
        assert_eq!(parse_newznab_date(b""), 0);
        assert_eq!(parse_newznab_date(b"yesterday"), 0);
        assert_eq!(parse_newznab_date(b"Tue, 10 Foo 2025 01:10:05 +0000"), 0);
        assert_eq!(parse_newznab_date(b"Tue, 40 Jun 2025 01:10:05 +0000"), 0);

        // NewznabBuildQueriesTest
        let describe = |q: &[(Vec<u8>, Vec<u8>)]| -> String {
            q.iter()
                .map(|(k, v)| {
                    format!(
                        "{}={}",
                        String::from_utf8_lossy(k),
                        String::from_utf8_lossy(v)
                    )
                })
                .collect::<Vec<_>>()
                .join("&")
        };
        let title = b"Lucifer.S02E14.Candy.Morningstar.1080p.DTS-HD.MA.5.1.AVC.REMUX-FraMeSToR";
        let q1 = build_newznab_queries(title, b"", b"");
        assert_eq!(q1.len(), 3);
        assert_eq!(
            describe(&q1[0]),
            "t=search&q=lucifer s02e14 candy morningstar 1080p dts hd ma 5 1 avc remux framestor"
        );
        assert_eq!(describe(&q1[1]), "t=search&q=lucifer s02e14 1080p");
        assert_eq!(
            describe(&q1[2]),
            "t=search&q=lucifer s02e14 1080p framestor"
        );

        let q2 = build_newznab_queries(title, b"tt1234567", b"tvdb-371796");
        assert_eq!(q2.len(), 5);
        assert_eq!(describe(&q2[3]), "t=movie&imdbid=1234567");
        assert_eq!(describe(&q2[4]), "t=tvsearch&tvdbid=371796&season=2&ep=14");

        let q3 = build_newznab_queries(b"Some.Movie.2020", b"", b"55");
        assert_eq!(q3.len(), 1);
        assert_eq!(describe(&q3[0]), "t=search&q=some movie 2020");

        let q4 = build_newznab_queries(b"Some.Movie.2020.Extended.2160p.UHD-GRP", b"", b"");
        assert_eq!(q4.len(), 3);
        assert_eq!(describe(&q4[1]), "t=search&q=some movie 2020 2160p");
        assert_eq!(describe(&q4[2]), "t=search&q=some movie 2020 2160p grp");

        let q5 = build_newznab_queries(b"NoMarkers-GRP", b"", b"");
        assert_eq!(q5.len(), 1);

        // NewznabBuildUrlAndMaskTest
        assert_eq!(
            build_newznab_url(
                b"http://127.0.0.1:5076/api",
                &[b"t", b"q", b"limit"],
                &[b"search", b"show s01e02 1080p", b"100"],
                b"k&y 1"
            ),
            b"http://127.0.0.1:5076/api?t=search&q=show+s01e02+1080p&limit=100&apikey=k%26y+1"
        );
        assert_eq!(
            build_newznab_url(b"http://h/api?x=1", &[], &[], b"key"),
            b"http://h/api?x=1&apikey=key"
        );
        assert_eq!(
            build_newznab_url(b"http://h:5076", &[], &[], b"key"),
            b"http://h:5076/api?apikey=key"
        );
        assert_eq!(
            build_newznab_url(b"http://h:5076/", &[], &[], b"key"),
            b"http://h:5076/api?apikey=key"
        );
        assert_eq!(
            build_newznab_url(b"https://h?x=1", &[], &[], b"key"),
            b"https://h/api?x=1&apikey=key"
        );
        assert_eq!(
            build_newznab_url(b"http://h/nzbhydra/api", &[], &[], b"key"),
            b"http://h/nzbhydra/api?apikey=key"
        );
        assert_eq!(
            mask_newznab_text(b"http://h/api?t=search&apikey=SECRET&q=x"),
            b"http://h/api?t=search&apikey=***&q=x"
        );
        assert_eq!(
            mask_newznab_text(b"http://h/api?APIKEY=SECRET"),
            b"http://h/api?APIKEY=***"
        );
        assert_eq!(
            mask_newznab_text(b"http://u:p@h/jsonrpc /admin:pw/jsonrpc"),
            b"http://***@h/jsonrpc /admin:pw/jsonrpc"
        );
        assert_eq!(mask_newznab_text(b"nothing to hide"), b"nothing to hide");

        // HostileLengthsTest & NzbReaderMessageIdTest
        let mut long_date = vec![b' '; 1_000_000];
        long_date.extend_from_slice(b"2025-06-10T01:10:05Z");
        assert_eq!(parse_newznab_date(&long_date), 0);

        let mut long_key = b"apikey=".to_vec();
        long_key.extend(std::iter::repeat_n(b'k', 1_000_000));
        assert!(mask_newznab_text(&long_key).len() <= 1024);

        assert!(valid_message_id(b"good1@test"));
        assert!(!valid_message_id(b"bad\r\nQUIT\r\nx@test"));
        assert!(!valid_message_id(b"bad space@test"));

        assert!(parse_newznab_response(
            br#"<rss><channel><item><newznab:attr name="size" name="dup" value="1"/></item></channel></rss>"#
        )
        .is_none());
        assert!(parse_newznab_response(
            br#"<rss><channel><item><title>a</title></item></channel></rss><rss><channel><item><title>b</title></item></channel></rss>"#
        )
        .is_none());
        assert!(parse_newznab_response(
            br#"<rss><channel><item><title>a</title></item></channel></rss>trailing"#
        )
        .is_none());
        let cdata_ent = parse_newznab_response(
            br#"<rss><channel><item><title><![CDATA[Understanding <!ENTITY declarations]]></title><link>http://x/c.nzb</link></item></channel></rss>"#,
        )
        .unwrap();
        assert_eq!(
            cdata_ent.items[0].title,
            b"Understanding <!ENTITY declarations"
        );
        let latin1_cdata = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><rss><channel><item><title><![CDATA[Caf\xe9\r\nBeta\rGamma]]></title><link>http://x/c.nzb</link><newznab:attr name=\"hydraIndexerName\" value=\"Idx\tOne\r\nTwo&#9;Tab\"/></item></channel></rss>";
        let page_latin1 = parse_newznab_response(latin1_cdata).unwrap();
        assert_eq!(page_latin1.items[0].title, "Café\nBeta\nGamma".as_bytes());
        assert_eq!(page_latin1.items[0].indexer, b"Idx One Two\tTab");
    }

    fn make_test_nzb(files: &[(&str, &[i32])], prefix: &str, meta: &str) -> Vec<u8> {
        let mut xml =
            String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">\n");
        if !meta.is_empty() {
            xml.push_str(&format!("<head><meta type=\"imdb\">{meta}</meta></head>\n"));
        }
        for (f, (name, sizes)) in files.iter().enumerate() {
            xml.push_str(&format!(
                "<file poster=\"poster@example.com\" date=\"1\" subject=\"[1/1] &quot;{name}&quot; yEnc (1/1)\"><groups><group>alt.binaries.test</group></groups><segments>"
            ));
            for (s, sz) in sizes.iter().enumerate() {
                xml.push_str(&format!(
                    "<segment bytes=\"{sz}\" number=\"{}\">{prefix}-{f}-{s}@x</segment>",
                    s + 1
                ));
            }
            xml.push_str("</segments></file>\n");
        }
        xml.push_str("</nzb>\n");
        xml.into_bytes()
    }

    #[test]
    fn nzb_reader_full_suite_port_from_cpp() {
        use std::collections::BTreeSet;

        // NzbReaderCountsBytesNamesIdsTest
        let info = parse_nzb_summary(&make_test_nzb(
            &[("rel.part1.rar", &[100, 100]), ("rel.par2", &[50])],
            "a",
            "tt123",
        ))
        .unwrap();
        assert_eq!(info.files, 2);
        assert_eq!(info.total_bytes, 250);
        assert_eq!(
            info.filenames,
            BTreeSet::from([b"rel.part1.rar".to_vec(), b"rel.par2".to_vec()])
        );
        assert_eq!(
            info.message_ids,
            vec![
                b"a-0-0@x".to_vec(),
                b"a-0-1@x".to_vec(),
                b"a-1-0@x".to_vec()
            ]
        );
        assert_eq!(info.meta.get(b"imdb".as_slice()).unwrap(), b"tt123");
        assert_eq!(info.poster, b"poster@example.com");

        let other =
            parse_nzb_summary(&make_test_nzb(&[("other", &[1, 2]), ("x", &[3])], "a", "")).unwrap();
        assert_eq!(info.fingerprint(), other.fingerprint());
        let other_b =
            parse_nzb_summary(&make_test_nzb(&[("other", &[1, 2]), ("x", &[3])], "b", "")).unwrap();
        assert_ne!(info.fingerprint(), other_b.fingerprint());

        // NzbReaderMainNameTest
        let info = parse_nzb_summary(&make_test_nzb(
            &[
                ("small.nfo", &[10]),
                ("big.mkv", &[100, 100]),
                ("mid.par2", &[50]),
            ],
            "a",
            "",
        ))
        .unwrap();
        assert_eq!(info.main_name, b"big.mkv");

        let info = parse_nzb_summary(&make_test_nzb(
            &[
                ("abc.vol063-121.par2", &[500]),
                ("Show.S01E01.1080p.WEB.H264-GRP.mkv", &[100]),
            ],
            "a",
            "",
        ))
        .unwrap();
        assert_eq!(info.main_name, b"Show.S01E01.1080p.WEB.H264-GRP.mkv");

        let info = parse_nzb_summary(&make_test_nzb(
            &[("a.par2", &[5]), ("a.vol0+1.par2", &[50])],
            "a",
            "",
        ))
        .unwrap();
        assert_eq!(info.main_name, b"a.vol0+1.par2");

        // NzbReaderNativeIdentityMetadataTest
        let info = parse_nzb_summary(&make_test_nzb(
            &[
                ("small.nfo", &[10]),
                ("big.part1.rar", &[100, 100]),
                ("rel.vol0+1.par2", &[500]),
                ("rel.par2", &[50]),
            ],
            "identity",
            "",
        ))
        .unwrap();
        assert_eq!(info.data_bytes, 210);
        assert_eq!(info.data_files, 2);
        assert_eq!(info.heads.len(), 2);
        assert_eq!(info.heads[0].name, b"big.part1.rar");
        assert_eq!(info.heads[0].id, b"identity-1-0@x");
        assert_eq!(info.heads[0].bytes, 200);
        assert_eq!(info.heads[1].name, b"small.nfo");

        let xml = b"<nzb><file subject='&quot;x.rar&quot;'><segments><segment number='2' bytes='100'>second@x</segment><segment number='1' bytes='100'>first@x</segment></segments></file></nzb>";
        let info = parse_nzb_summary(xml).unwrap();
        assert_eq!(info.heads.len(), 1);
        assert_eq!(info.heads[0].id, b"first@x");

        let info = parse_nzb_summary(&make_test_nzb(&[("only.par2", &[50])], "p", "")).unwrap();
        assert_eq!(info.data_bytes, 0);
        assert_eq!(info.data_files, 0);
        assert!(info.heads.is_empty());

        // NativeIdentityHintTest & NativeIdentityDuplicateFileGeometryTest
        let pick = parse_nzb_summary(&make_test_nzb(
            &[("rel.part1.rar", &[200]), ("rel.par2", &[10])],
            "pick",
            "",
        ))
        .unwrap();
        let donor = parse_nzb_summary(&make_test_nzb(
            &[("rel.part1.rar", &[200]), ("rel.vol1+9.par2", &[90])],
            "donor",
            "",
        ))
        .unwrap();
        assert!(crate::dupeidentity::same_data(
            pick.data_bytes as u64,
            pick.data_files as u32,
            donor.data_bytes as u64,
            donor.data_files as u32
        ));

        let separate = parse_nzb_summary(&make_test_nzb(
            &[("same.rar", &[100]), ("same.rar", &[100])],
            "a",
            "",
        ))
        .unwrap();
        let joined = parse_nzb_summary(&make_test_nzb(&[("same.rar", &[200])], "b", "")).unwrap();
        assert_eq!(separate.data_files, 2);
        assert!(!crate::dupeidentity::same_data(
            separate.data_bytes as u64,
            separate.data_files as u32,
            joined.data_bytes as u64,
            joined.data_files as u32
        ));
        assert_eq!(separate.heads.len(), 2);
        assert_ne!(separate.heads[0].id, separate.heads[1].id);

        // NativeFetchBudgetConcurrentTest
        let budget = std::sync::Arc::new(crate::dupewatch::FetchBudget::from_max_donors(1));
        let admitted = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
        let mut threads = Vec::new();
        for _ in 0..16 {
            let b = budget.clone();
            let a = admitted.clone();
            threads.push(std::thread::spawn(move || {
                if b.take() {
                    a.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }));
        }
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(admitted.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert!(budget.spent());
        let unlimited = crate::dupewatch::FetchBudget::from_max_donors(0);
        for _ in 0..100 {
            assert!(unlimited.take());
        }
        assert!(!unlimited.spent());

        // NativePartialNzbHealthTest
        let xml = b"<nzb><file subject='&quot;rel.part01.rar&quot; yEnc (1/150)'><segments><segment number='1' bytes='10000'>a@x</segment><segment number='2' bytes='10000'>b@x</segment></segments></file></nzb>";
        let info = parse_nzb_summary(xml).unwrap();
        assert_eq!(info.listed_parts, 2);
        assert_eq!(info.declared_parts, 150);
        assert_eq!(info.article_bytes, 10000);
        assert_eq!(
            crate::dupewatch::whole_bytes(
                info.total_bytes as u64,
                info.listed_parts,
                info.declared_parts
            ),
            1_500_000
        );

        // NativeDataArticleIdsExcludeParityTest
        let mut xml = String::from(
            "<nzb><file subject='&quot;rel.mkv&quot;'><segments><segment number='1' bytes='100'>discarded@x</segment><segment number='2' bytes='100'>a-data@x</segment><segment number='1' bytes='100'>z-data@x</segment><segment number='3' bytes='100'>z-data@x</segment></segments></file><file subject='&quot;rel.vol00+25.PAR2&quot;'><segments>",
        );
        for i in 1..=25 {
            xml.push_str(&format!(
                "<segment number='{i}' bytes='10'>par-{i}@x</segment>"
            ));
        }
        xml.push_str("</segments></file></nzb>");
        let info = parse_nzb_summary(xml.as_bytes()).unwrap();
        assert_eq!(info.data_bytes, 300);
        assert_eq!(
            info.data_message_ids,
            vec![b"a-data@x".to_vec(), b"z-data@x".to_vec()]
        );
        assert_eq!(info.message_ids.len(), 27);
        assert!(!info.message_ids.contains(&b"discarded@x".to_vec()));
        let orig_fp = info.fingerprint();
        let xml_mod = xml.replacen("par-1@x", "par-new@x", 1);
        let info_mod = parse_nzb_summary(xml_mod.as_bytes()).unwrap();
        assert_eq!(
            info_mod.data_message_ids,
            vec![b"a-data@x".to_vec(), b"z-data@x".to_vec()]
        );
        assert_ne!(info_mod.fingerprint(), orig_fp);

        let xml_bad = b"<nzb><file subject='&quot;rel.mkv&quot;'><segments><segment number='1' bytes='100'>valid@x</segment><segment number='2' bytes='100'>invalid id@x</segment></segments></file></nzb>";
        let info_bad = parse_nzb_summary(xml_bad).unwrap();
        assert_eq!(info_bad.data_bytes, 0);
        assert_eq!(info_bad.data_message_ids, vec![b"valid@x".to_vec()]);

        // NativeDataPartGeometryExcludesParityTest
        let mut xml = String::from(
            "<nzb><file subject='&quot;rel.mkv&quot; yEnc (1/150)'><segments><segment number='1' bytes='100'>data-1@x</segment><segment number='2' bytes='100'>data-2@x</segment></segments></file><file subject='&quot;rel.vol00+25.par2&quot;'><segments>",
        );
        for i in 1..=25 {
            xml.push_str(&format!(
                "<segment number='{i}' bytes='10'>par-{i}@x</segment>"
            ));
        }
        xml.push_str("</segments></file></nzb>");
        let info = parse_nzb_summary(xml.as_bytes()).unwrap();
        assert_eq!(info.listed_parts, 27);
        assert_eq!(info.declared_parts, 175);
        assert_eq!(info.data_listed_parts, 2);
        assert_eq!(info.data_declared_parts, 150);
        assert_eq!(info.data_message_ids.len(), 2);

        // NativeDeclaredTailGuardTableTest
        type GuardCase<'a> = (&'a str, Vec<(i64, i64)>, &'a str, bool, u64, u64, i64, bool);
        let make_parts =
            |count: i64| -> Vec<(i64, i64)> { (1..=count).map(|n| (n, 100)).collect() };
        let cases: Vec<GuardCase> = vec![
            (
                "true_tail",
                vec![(1, 100), (2, 100)],
                "(1/150)",
                false,
                2,
                150,
                200,
                true,
            ),
            (
                "number_gap_without_tail",
                vec![(1, 100), (3, 100)],
                "(1/0)",
                false,
                2,
                3,
                200,
                true,
            ),
            (
                "single_segment",
                vec![(9374, 100)],
                "(1/10000)",
                false,
                1,
                9374,
                100,
                true,
            ),
            (
                "zero_subject_count",
                vec![(1, 100), (2, 100)],
                "(1/0)",
                false,
                2,
                2,
                200,
                true,
            ),
            (
                "max_subject_count",
                vec![(1, 100), (2, 100)],
                "(1/200000)",
                false,
                2,
                200000,
                200,
                true,
            ),
            (
                "over_max_subject_count",
                vec![(1, 100), (2, 100)],
                "(1/200001)",
                false,
                2,
                2,
                200,
                true,
            ),
            (
                "two_thirds_equal",
                make_parts(200),
                "(1/300)",
                false,
                200,
                200,
                20000,
                true,
            ),
            (
                "two_thirds_below",
                make_parts(199),
                "(1/300)",
                false,
                199,
                300,
                19900,
                true,
            ),
            (
                "tail_gap_99",
                vec![(1, 100), (101, 100)],
                "(1/200)",
                false,
                2,
                101,
                200,
                true,
            ),
            (
                "tail_gap_100",
                vec![(1, 100), (101, 100)],
                "(1/201)",
                false,
                2,
                201,
                200,
                true,
            ),
            (
                "zero_part_number",
                vec![(0, 100), (2, 100)],
                "(1/1000)",
                false,
                2,
                2,
                200,
                true,
            ),
            (
                "negative_part_number",
                vec![(-1, 100), (2, 100)],
                "(1/1000)",
                false,
                2,
                2,
                200,
                true,
            ),
            (
                "invalid_number_suppresses_valid_max",
                vec![(1, 100), (199999, 100), (200001, 100)],
                "(1/200000)",
                false,
                3,
                3,
                300,
                true,
            ),
            (
                "maximum_valid_part",
                vec![(1, 100), (200000, 100)],
                "(1/200000)",
                false,
                2,
                200000,
                200,
                true,
            ),
            (
                "zero_bytes",
                vec![(1, 100), (2, 0)],
                "(1/150)",
                false,
                2,
                2,
                100,
                false,
            ),
            (
                "negative_bytes",
                vec![(1, 100), (2, -1)],
                "(1/150)",
                false,
                2,
                2,
                99,
                false,
            ),
            (
                "strict_majority_short_last",
                vec![(1, 100), (2, 100), (3, 98)],
                "(1/150)",
                false,
                3,
                3,
                298,
                true,
            ),
            (
                "strict_majority_large_last",
                vec![(1, 100), (2, 100), (3, 101)],
                "(1/150)",
                false,
                3,
                3,
                301,
                true,
            ),
            (
                "strict_majority_equal_last",
                vec![(1, 100), (2, 98), (3, 100)],
                "(1/150)",
                false,
                3,
                150,
                298,
                true,
            ),
            (
                "median_exact_99_percent",
                vec![(1, 900), (2, 1000), (3, 1100), (4, 990)],
                "(1/150)",
                false,
                4,
                150,
                3990,
                true,
            ),
            (
                "median_below_99_percent",
                vec![(1, 900), (2, 1000), (3, 1100), (4, 989)],
                "(1/150)",
                false,
                4,
                4,
                3989,
                true,
            ),
            (
                "half_is_not_strict_majority",
                vec![(1, 99), (2, 100), (3, 99), (4, 100)],
                "(1/150)",
                false,
                4,
                150,
                398,
                true,
            ),
            (
                "parity_never_infers",
                vec![(1, 100), (2, 100)],
                "(1/150)",
                true,
                2,
                2,
                200,
                true,
            ),
            (
                "malformed_tail",
                vec![(1, 100), (3, 100)],
                "(x/150)",
                false,
                2,
                3,
                200,
                true,
            ),
            (
                "trailing_subject_whitespace",
                vec![(1, 100), (2, 100)],
                "(1/150)  ",
                false,
                2,
                150,
                200,
                true,
            ),
            (
                "unsorted_input",
                vec![(3, 100), (1, 100), (2, 100)],
                "(1/150)",
                false,
                3,
                150,
                300,
                true,
            ),
            (
                "duplicate_keeps_last_bytes",
                vec![(1, 100), (2, 100), (2, 30)],
                "(1/150)",
                false,
                2,
                2,
                130,
                true,
            ),
        ];
        for (label, parts, tail, parity, listed, declared, bytes, geom) in cases {
            let name = if parity { "guard.par2" } else { "guard.mkv" };
            let mut xml = format!("<nzb><file subject='&quot;{name}&quot; yEnc {tail}'><segments>");
            for (i, (num, sz)) in parts.iter().enumerate() {
                xml.push_str(&format!(
                    "<segment number='{num}' bytes='{sz}'>guard-{i}@x</segment>"
                ));
            }
            xml.push_str("</segments></file></nzb>");
            let info = parse_nzb_summary(xml.as_bytes()).expect(label);
            assert_eq!(info.files, 1, "{label}");
            assert_eq!(info.listed_parts, listed, "{label}");
            assert_eq!(info.declared_parts, declared, "{label}");
            assert_eq!(
                info.data_listed_parts,
                if parity { 0 } else { listed },
                "{label}"
            );
            assert_eq!(
                info.data_declared_parts,
                if parity { 0 } else { declared },
                "{label}"
            );
            assert_eq!(info.message_ids.len() as u64, listed, "{label}");
            assert_eq!(
                info.data_message_ids.len() as u64,
                if parity { 0 } else { listed },
                "{label}"
            );
            assert_eq!(info.total_bytes, bytes, "{label}");
            assert_eq!(
                info.data_bytes,
                if parity || !geom { 0 } else { bytes },
                "{label}"
            );
            assert_eq!(
                info.data_files,
                if parity || !geom { 0 } else { 1 },
                "{label}"
            );
            if label == "duplicate_keeps_last_bytes" {
                assert!(!info.message_ids.contains(&b"guard-1@x".to_vec()));
                assert!(info.message_ids.contains(&b"guard-2@x".to_vec()));
            }
        }

        // NativeEmptyFilePartCountsTest, NativeDuplicateSegmentKeepsLastTest, NativeDuplicateSegmentInvalidLastTest
        let empty =
            "<file subject='&quot;empty.mkv&quot; yEnc (1/150)'><segments></segments></file>";
        assert!(parse_nzb_summary(format!("<nzb>{empty}</nzb>").as_bytes()).is_none());
        let valid = "<file subject='&quot;valid.mkv&quot;'><segments><segment number='1' bytes='100'>valid@x</segment></segments></file>";
        let info = parse_nzb_summary(format!("<nzb>{empty}{valid}</nzb>").as_bytes()).unwrap();
        assert_eq!(info.files, 2);
        assert_eq!(info.listed_parts, 1);
        assert_eq!(info.declared_parts, 1);
        assert_eq!(info.total_bytes, 100);
        assert_eq!(info.data_bytes, 0);
        assert_eq!(info.data_files, 0);
        assert!(info.heads.is_empty());

        // NativeIndependentParityCollectionsUnknownTest & NativeParityIndexByFilenameTest
        for reverse in [false, true] {
            let mut files: Vec<(&str, &[i32])> = vec![
                ("first.par2", &[100]),
                ("first.vol00+01.par2", &[200]),
                ("second.par2", &[300]),
                ("second.vol00+01.par2", &[400]),
                ("first.mkv", &[1000]),
                ("second.mkv", &[2000]),
            ];
            if reverse {
                files.reverse();
            }
            let info = parse_nzb_summary(&make_test_nzb(&files, "par-sets", "")).unwrap();
            assert_eq!(info.data_bytes, 3000);
            assert!(info.par_index.id.is_empty());
            assert!(info.recovery_bytes.is_empty());
        }

        let info = parse_nzb_summary(&make_test_nzb(
            &[
                ("rel.vol00+01.par2", &[100]),
                ("rel.par2", &[900]),
                ("rel.vol01+02.PAR2", &[200]),
                ("other.vol00+10.par2", &[1000]),
                ("rel.mkv", &[10000]),
            ],
            "par-index",
            "",
        ))
        .unwrap();
        assert_eq!(info.par_index.name, b"rel.par2");
        assert_eq!(info.recovery_bytes, vec![100, 200]);

        let info = parse_nzb_summary(&make_test_nzb(
            &[
                ("rel.vol00+01.par2", &[100]),
                ("rel.vol01+02.par2", &[200]),
                ("rel.mkv", &[10000]),
            ],
            "par-volumes",
            "",
        ))
        .unwrap();
        assert!(info.par_index.id.is_empty());
        assert!(info.recovery_bytes.is_empty());

        // NzbReaderRejectsBadNzbTest
        assert!(parse_nzb_summary(b"<nzb><file").is_none());
        assert!(parse_nzb_summary(b"<nzb></nzb>").is_none());
        assert!(parse_nzb_summary(b"").is_none());
        assert!(parse_nzb_summary(b"<html><body>Forbidden</body></html>").is_none());
        assert!(parse_nzb_summary(
            b"<?xml version=\"1.0\"?><error code=\"429\" description=\"Request limit reached\"/>"
        )
        .is_none());
        let bomb = "<?xml version=\"1.0\"?><!DOCTYPE n [<!ENTITY a \"aaaa\"><!ENTITY b \"&a;&a;\">]><nzb><file subject=\"&b;\"><segments><segment bytes=\"1\">x@y</segment></segments></file></nzb>";
        assert!(parse_nzb_summary(bomb.as_bytes()).is_none());
        let padded = format!(
            "<?xml version=\"1.0\"?><!--{}-->{}",
            " ".repeat(8000),
            &bomb[bomb.find("<!DOCTYPE").unwrap()..]
        );
        assert!(parse_nzb_summary(padded.as_bytes()).is_none());
        let mut standard = String::from_utf8(make_test_nzb(&[("a.mkv", &[5])], "a", "")).unwrap();
        let pos = standard.find("<nzb").unwrap();
        standard.insert_str(
            pos,
            "<!DOCTYPE nzb PUBLIC \"-//newzBin//DTD NZB 1.1//EN\" \"http://www.newzbin.com/DTD/nzb/nzb-1.1.dtd\">\n",
        );
        let info = parse_nzb_summary(standard.as_bytes()).unwrap();
        assert_eq!(info.files, 1);

        // NzbReaderUtf16Test (UTF-16LE & UTF-16BE with BOM, without BOM, and with encoding="UTF-16")
        let utf8_src = "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n<nzb><head><meta type=\"title\">Café.2026</meta></head><file poster=\"u@x\" subject=\"&quot;Café.2026.mkv&quot; yEnc (1/2)\"><groups><group>alt.binaries.test</group></groups><segments><segment bytes=\"500\" number=\"1\">u16_1@x</segment><segment bytes=\"500\" number=\"2\">u16_2@x</segment></segments></file></nzb>";
        let expected_utf8 = parse_nzb_summary(
            utf8_src
                .replace("encoding=\"UTF-16\"", "encoding=\"UTF-8\"")
                .as_bytes(),
        )
        .unwrap();

        let mut utf16le_bom = vec![0xFFu8, 0xFEu8];
        let mut utf16le_nobom = Vec::new();
        let mut utf16be_bom = vec![0xFEu8, 0xFFu8];
        let mut utf16be_nobom = Vec::new();
        for unit in utf8_src.encode_utf16() {
            utf16le_bom.extend_from_slice(&unit.to_le_bytes());
            utf16le_nobom.extend_from_slice(&unit.to_le_bytes());
            utf16be_bom.extend_from_slice(&unit.to_be_bytes());
            utf16be_nobom.extend_from_slice(&unit.to_be_bytes());
        }
        for candidate in [&utf16le_bom, &utf16le_nobom, &utf16be_bom, &utf16be_nobom] {
            let parsed = parse_nzb_summary(candidate).expect("UTF-16 NZB should parse");
            assert_eq!(parsed.files, expected_utf8.files);
            assert_eq!(parsed.data_bytes, expected_utf8.data_bytes);
            assert_eq!(parsed.main_name, b"Caf\xc3\xa9.2026.mkv");
            assert_eq!(
                parsed.meta.get(&b"title"[..]).map(|v| v.as_slice()),
                Some(&b"Caf\xc3\xa9.2026"[..])
            );
            assert_eq!(parsed.data_message_ids, expected_utf8.data_message_ids);
        }
        // Single-byte stream falsely claiming UTF-16 is rejected
        assert!(parse_nzb_summary(utf8_src.as_bytes()).is_none());
    }

    static DONOR_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn donor_health_full_suite_port_from_cpp() {
        let _donor_guard = DONOR_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        donor_stop_all();
        donor_wait_all();
        donor_reset();
        use std::collections::BTreeSet;
        use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
        use std::sync::{Arc, Mutex};

        struct FakeTransport {
            has: Mutex<BTreeSet<String>>,
            has_all: AtomicBool,
            answers_451: AtomicBool,
            body_gone: AtomicBool,
            auth_fails: AtomicBool,
            fail_first: AtomicI32,
            delay_ms: AtomicI32,
            drop_on_first_body: AtomicBool,
            pool_busy: AtomicBool,
            stats: AtomicI32,
            bodies: AtomicI32,
            pipelined: AtomicI32,
            sessions: AtomicI32,
            exchanges: AtomicI32,
        }

        impl FakeTransport {
            fn new() -> Arc<Self> {
                Arc::new(Self {
                    has: Mutex::new(BTreeSet::new()),
                    has_all: AtomicBool::new(false),
                    answers_451: AtomicBool::new(false),
                    body_gone: AtomicBool::new(false),
                    auth_fails: AtomicBool::new(false),
                    fail_first: AtomicI32::new(0),
                    delay_ms: AtomicI32::new(0),
                    drop_on_first_body: AtomicBool::new(false),
                    pool_busy: AtomicBool::new(false),
                    stats: AtomicI32::new(0),
                    bodies: AtomicI32::new(0),
                    pipelined: AtomicI32::new(0),
                    sessions: AtomicI32::new(0),
                    exchanges: AtomicI32::new(0),
                })
            }
        }

        impl DonorServerExchange for Arc<FakeTransport> {
            fn exchange(
                &self,
                batch: &[DonorRequest],
            ) -> Result<Vec<DonorAnswer>, DonorExchangeError> {
                if self.pool_busy.load(Ordering::SeqCst) {
                    self.exchanges.fetch_add(1, Ordering::SeqCst);
                    return Err(DonorExchangeError {
                        answers: Vec::new(),
                        busy: true,
                    });
                }
                let prev_ex = self.exchanges.fetch_add(1, Ordering::SeqCst);
                let broken = self.auth_fails.load(Ordering::SeqCst)
                    || prev_ex < self.fail_first.load(Ordering::SeqCst);
                if broken || self.sessions.load(Ordering::SeqCst) == 0 {
                    self.sessions.fetch_add(1, Ordering::SeqCst);
                }
                if broken {
                    return Err(DonorExchangeError::default());
                }

                let delay = self.delay_ms.load(Ordering::SeqCst);
                if delay > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(
                        (delay as u64) * (batch.len() as u64),
                    ));
                }
                self.stats.fetch_add(batch.len() as i32, Ordering::SeqCst);
                if batch.len() > 1 {
                    self.pipelined
                        .fetch_add(batch.len() as i32, Ordering::SeqCst);
                }

                let has_all = self.has_all.load(Ordering::SeqCst);
                let ans_451 = self.answers_451.load(Ordering::SeqCst);
                let has_set = self.has.lock().unwrap();
                let mut out: Vec<DonorAnswer> = batch
                    .iter()
                    .map(|req| {
                        let present = has_all || has_set.contains(&req.message_id);
                        if present {
                            DonorAnswer::Present
                        } else if ans_451 {
                            DonorAnswer::Error
                        } else {
                            DonorAnswer::Missing
                        }
                    })
                    .collect();
                drop(has_set);

                for i in 0..batch.len() {
                    let Some(gate) = &batch[i].gate else {
                        continue;
                    };
                    if out[i] != DonorAnswer::Present {
                        continue;
                    }
                    if self.drop_on_first_body.load(Ordering::SeqCst)
                        && self.bodies.load(Ordering::SeqCst) == 0
                    {
                        self.bodies.fetch_add(1, Ordering::SeqCst);
                        self.sessions.fetch_add(1, Ordering::SeqCst);
                        let mut err_ans = out.clone();
                        err_ans[i] = DonorAnswer::Error;
                        return Err(DonorExchangeError {
                            answers: err_ans,
                            busy: false,
                        });
                    }
                    out[i] = gate.body_once(|| {
                        self.bodies.fetch_add(1, Ordering::SeqCst);
                        if delay > 0 {
                            std::thread::sleep(std::time::Duration::from_millis(delay as u64));
                        }
                        if self.body_gone.load(Ordering::SeqCst) {
                            DonorAnswer::BodyBad
                        } else {
                            DonorAnswer::Present
                        }
                    });
                }
                Ok(out)
            }
        }

        let make_server = |cap: i32| -> (Arc<DonorServer>, Arc<FakeTransport>) {
            let t = FakeTransport::new();
            let s = Arc::new(DonorServer::new(cap, t.clone()));
            (s, t)
        };
        let ids = |prefix: &str, count: usize| -> Vec<String> {
            (0..count).map(|i| format!("{prefix}{i}@x")).collect()
        };
        let no_groups = || Arc::new(Vec::<String>::new());
        let everything = |body_percent: i32| DonorOptions {
            sample: DonorSampleOptions {
                percent: 100,
                minimum: 1,
                maximum: 100_000,
                body_percent,
                max_body: 1000,
                seed: 0,
            },
            probe: 10,
            budget_ms: 10_000,
            deadline_ms: 0,
        };

        donor_reset();

        // 1. DonorHealthSampleTest
        assert_eq!(donor_sample(&ids("a", 100), 2, 20, 300, 0).len(), 20);
        assert_eq!(donor_sample(&ids("a", 5000), 2, 20, 300, 0).len(), 100);
        assert_eq!(donor_sample(&ids("a", 100_000), 2, 20, 300, 0).len(), 300);
        assert_eq!(donor_sample(&ids("a", 20_000), 5, 50, 1000, 0).len(), 1000);
        assert_eq!(donor_sample(&ids("a", 300), 5, 50, 1000, 0).len(), 50);
        assert_eq!(donor_sample(&ids("a", 12), 5, 50, 1000, 0).len(), 12);

        let a = donor_sample(&ids("a", 5000), 2, 20, 300, 7);
        assert_eq!(a, donor_sample(&ids("a", 5000), 2, 20, 300, 7));
        assert_ne!(a, donor_sample(&ids("a", 5000), 2, 20, 300, 8));
        let unique: BTreeSet<_> = a.iter().collect();
        assert_eq!(unique.len(), a.len());

        // 2. DonorHealthPlanBodyTest
        let mut s_opts = DonorSampleOptions {
            percent: 30,
            minimum: 20,
            maximum: 300,
            body_percent: 20,
            max_body: 5,
            seed: 0,
        };
        let planned = donor_plan(&ids("a", 1000), &no_groups(), &s_opts);
        assert_eq!(planned.len(), 300);
        assert_eq!(planned.iter().filter(|it| it.body).count(), 5);
        s_opts.max_body = 1000;
        let planned2 = donor_plan(&ids("a", 1000), &no_groups(), &s_opts);
        let bodies = planned2.iter().filter(|it| it.body).count();
        assert!(bodies > 30 && bodies < 90);

        // 3. DonorHealthVerdictTest
        let mk_h = |checked, present, missing, error| DonorHealthResult {
            checked,
            present,
            missing,
            error,
            body_checked: 0,
            body_bad: 0,
        };
        assert_eq!(mk_h(300, 0, 1, 299).alive(), 0.0);
        assert_eq!(mk_h(300, 150, 0, 150).alive(), 0.5);
        assert_eq!(mk_h(300, 10, 0, 0).alive(), 1.0);
        assert_eq!(mk_h(300, 0, 2, 2).alive(), -1.0);
        assert!(donor_dead_probe(&mk_h(10, 0, 5, 5)));
        assert!(donor_dead_probe(&mk_h(10, 0, 10, 0)));
        assert!(!donor_dead_probe(&mk_h(10, 0, 4, 6)));
        assert!(!donor_dead_probe(&mk_h(10, 1, 9, 0)));
        assert!(!donor_dead_probe(&mk_h(10, 0, 0, 10)));

        // 4. DonorHealthAllServersTest
        let id_list = ids("a", 40);
        let (sa, ta) = make_server(1);
        let (sb, tb) = make_server(1);
        for (i, id) in id_list.iter().enumerate() {
            if i < 20 {
                ta.has.lock().unwrap().insert(id.clone());
            } else {
                tb.has.lock().unwrap().insert(id.clone());
            }
        }
        let h = donor_check_items(
            &[sa, sb],
            &donor_plan(&id_list, &no_groups(), &everything(0).sample),
            5000,
        );
        assert_eq!(h.checked, 40);
        assert_eq!(h.present, 40);

        // 5. DonorHealthDeadProbeAsksEveryServerTest
        let (sa, ta) = make_server(1);
        let (sb, tb) = make_server(1);
        let h = donor_check_posting(
            &[sa, sb],
            &ids("dead", 30),
            &no_groups(),
            &everything(0),
            false,
        );
        assert_eq!(h.checked, 10);
        assert_eq!(h.missing, 10);
        assert_eq!(h.present, 0);
        assert_eq!(ta.stats.load(Ordering::SeqCst), 10);
        assert_eq!(tb.stats.load(Ordering::SeqCst), 10);
        assert!(donor_dead_probe(&h));

        // 6. DonorHealthSlowServerNeverHoldsUpTheOthersTest
        let (fast_s, fast_t) = make_server(1);
        fast_t.has_all.store(true, Ordering::SeqCst);
        let (slow_s, slow_t) = make_server(1);
        slow_t.has_all.store(true, Ordering::SeqCst);
        slow_t.delay_ms.store(500, Ordering::SeqCst);
        let start = donor_now_ms();
        let h = donor_check_items(
            &[fast_s, slow_s],
            &donor_plan(&ids("a", 20), &no_groups(), &everything(0).sample),
            10_000,
        );
        assert_eq!(h.present, 20);
        assert!(donor_now_ms() - start < 3000);
        assert!(slow_t.stats.load(Ordering::SeqCst) < 20);

        // 7. DonorHealthBadAuthServerTest
        let (bad_s, bad_t) = make_server(1);
        bad_t.auth_fails.store(true, Ordering::SeqCst);
        let (good_s, _) = make_server(1);
        let h = donor_check_items(
            &[bad_s, good_s],
            &donor_plan(&ids("dead", 20), &no_groups(), &everything(0).sample),
            5000,
        );
        assert_eq!(h.missing, 20);
        assert_eq!(h.error, 0);

        let (bad2_s, bad2_t) = make_server(1);
        bad2_t.auth_fails.store(true, Ordering::SeqCst);
        let h = donor_check_items(
            &[bad2_s],
            &donor_plan(&ids("a", 20), &no_groups(), &everything(0).sample),
            5000,
        );
        assert_eq!(h.missing, 0);
        assert_eq!(h.present, 0);
        assert!(!donor_dead_probe(&h));

        // 8. DonorHealth451IsAnAnswerTest
        let (sa, ta) = make_server(1);
        ta.answers_451.store(true, Ordering::SeqCst);
        sa.set_retry_after_ms(30_000);
        let (sb, _) = make_server(1);
        let start = donor_now_ms();
        let h = donor_check_items(
            &[sa, sb],
            &donor_plan(&ids("dead", 20), &no_groups(), &everything(0).sample),
            10_000,
        );
        assert_eq!(h.missing, 20);
        assert!(donor_now_ms() - start < 3000);
        assert_eq!(ta.sessions.load(Ordering::SeqCst), 1);
        assert_eq!(ta.stats.load(Ordering::SeqCst), 20);

        let (sc, tc) = make_server(1);
        tc.answers_451.store(true, Ordering::SeqCst);
        let h = donor_check_items(
            &[sc],
            &donor_plan(&ids("dead", 20), &no_groups(), &everything(0).sample),
            5000,
        );
        assert_eq!(h.missing, 0);
        assert_eq!(h.error, 20);
        assert!(!donor_dead_probe(&h));

        // 9. DonorHealthPausedServerAbstainsTest
        let (bad_s, bad_t) = make_server(1);
        bad_t.auth_fails.store(true, Ordering::SeqCst);
        bad_s.set_retry_after_ms(30_000);
        let (good_s, good_t) = make_server(1);
        good_t.delay_ms.store(5, Ordering::SeqCst);
        let start = donor_now_ms();
        let h = donor_check_items(
            &[bad_s, good_s],
            &donor_plan(&ids("dead", 40), &no_groups(), &everything(0).sample),
            10_000,
        );
        assert_eq!(h.missing, 40);
        assert!(donor_now_ms() - start < 5000);

        // 10. DonorHealthBusyPoolDoesNotPauseTest & 11. DonorHealthPausedAskIsInterruptibleTest
        let (busy_s, busy_t) = make_server(1);
        busy_t.pool_busy.store(true, Ordering::SeqCst);
        busy_s.set_retry_after_ms(30_000);
        let batch1 = vec![DonorRequest {
            message_id: "a@x".to_string(),
            gate: None,
            groups: no_groups(),
        }];
        for _ in 0..(DONOR_SERVER_GIVE_UP * 2) {
            busy_s.ask(&batch1, || false);
        }
        assert!(!busy_s.paused());
        assert_eq!(
            busy_t.exchanges.load(Ordering::SeqCst),
            DONOR_SERVER_GIVE_UP * 2
        );

        let (paused_s, paused_t) = make_server(1);
        paused_t.auth_fails.store(true, Ordering::SeqCst);
        paused_s.set_retry_after_ms(30_000);
        for _ in 0..DONOR_SERVER_GIVE_UP {
            paused_s.ask(&batch1, || false);
        }
        assert!(paused_s.paused());
        let before = paused_t.exchanges.load(Ordering::SeqCst);
        let ans = paused_s.ask(&batch1, || true);
        assert_eq!(ans, vec![DonorAnswer::Error]);
        assert_eq!(paused_t.exchanges.load(Ordering::SeqCst), before);

        donor_stop_all();
        paused_s.ask(&batch1, || false);
        assert_eq!(paused_t.exchanges.load(Ordering::SeqCst), before);
        donor_reset();

        // 12. DonorHealthSingleServerRecoversTest
        let (srv, tr) = make_server(1);
        tr.has_all.store(true, Ordering::SeqCst);
        tr.fail_first.store(3, Ordering::SeqCst);
        srv.set_retry_after_ms(20);
        let h = donor_check_items(
            &[srv],
            &donor_plan(&ids("a", 60), &no_groups(), &everything(0).sample),
            10_000,
        );
        assert!(h.present >= 60 - 3 * 4);
        assert_eq!(h.present + h.error, 60);

        // 13. DonorHealthBudgetEndBiasTest & 14. DonorHealthSlowServerBudgetTest
        let id20 = ids("a", 20);
        let (sa, ta) = make_server(1);
        let (sb, _) = make_server(1);
        let (sc, tc) = make_server(1);
        for id in id20.iter().take(10) {
            ta.has.lock().unwrap().insert(id.clone());
        }
        tc.has_all.store(true, Ordering::SeqCst);
        tc.delay_ms.store(1000, Ordering::SeqCst);
        let h = donor_check_items(
            &[sa, sb, sc],
            &donor_plan(&id20, &no_groups(), &everything(0).sample),
            200,
        );
        assert_eq!(h.present, 10);
        assert_eq!(h.missing, 10);
        assert_eq!(h.alive(), 0.5);

        // 15. DonorHealthBodyChecksTest, 16. OneServerPerBody, 17. DropsDuringBody, Pipelining, Cap, ProbeEndsDead
        let id30 = ids("a", 30);
        let (soft_s, soft_t) = make_server(1);
        soft_t.has_all.store(true, Ordering::SeqCst);
        soft_t.body_gone.store(true, Ordering::SeqCst);
        let h = donor_check_items(
            &[soft_s.clone()],
            &donor_plan(&id30, &no_groups(), &everything(0).sample),
            5000,
        );
        assert_eq!(h.alive(), 1.0);
        let h = donor_check_items(
            &[soft_s],
            &donor_plan(&id30, &no_groups(), &everything(100).sample),
            5000,
        );
        assert_eq!(h.alive(), 0.0);
        assert_eq!(h.body_checked, 30);
        assert_eq!(h.body_bad, 30);

        let (s1, t1) = make_server(1);
        let (s2, t2) = make_server(1);
        let (s3, t3) = make_server(1);
        for t in [&t1, &t2, &t3] {
            t.has_all.store(true, Ordering::SeqCst);
        }
        let h = donor_check_items(
            &[s1, s2, s3],
            &donor_plan(&ids("a", 10), &no_groups(), &everything(100).sample),
            5000,
        );
        assert_eq!(h.present, 10);
        assert_eq!(
            t1.bodies.load(Ordering::SeqCst)
                + t2.bodies.load(Ordering::SeqCst)
                + t3.bodies.load(Ordering::SeqCst),
            10
        );

        let (drop_s, drop_t) = make_server(1);
        drop_t.has_all.store(true, Ordering::SeqCst);
        drop_t.drop_on_first_body.store(true, Ordering::SeqCst);
        let h = donor_check_items(
            &[drop_s],
            &donor_plan(&ids("a", 40), &no_groups(), &everything(100).sample),
            10_000,
        );
        assert!(h.present >= 40 - 4 * 2);
        assert_eq!(h.present + h.error, 40);

        let (pipe_s, pipe_t) = make_server(1);
        pipe_t.has_all.store(true, Ordering::SeqCst);
        let h = donor_check_items(
            &[pipe_s.clone()],
            &donor_plan(&ids("a", 64), &no_groups(), &everything(0).sample),
            5000,
        );
        assert_eq!(h.present, 64);
        assert_eq!(pipe_t.sessions.load(Ordering::SeqCst), 1);
        assert!(pipe_t.pipelined.load(Ordering::SeqCst) >= 32);
        assert_eq!(pipe_s.depth(), DONOR_PIPELINE_MAX);

        let (cap_s, cap_t) = make_server(4);
        cap_t.has_all.store(true, Ordering::SeqCst);
        cap_t.delay_ms.store(2, Ordering::SeqCst);
        let postings: Vec<DonorPosting> = (0..15)
            .map(|i| DonorPosting {
                key: format!("p{i}"),
                ids: ids(&format!("p{i}-"), 20),
                groups: no_groups(),
            })
            .collect();
        let done = AtomicI32::new(0);
        let alive = AtomicI32::new(0);
        donor_check_postings(
            &[cap_s.clone()],
            &postings,
            &everything(0),
            true,
            8,
            |_, h| {
                done.fetch_add(1, Ordering::SeqCst);
                if h.present == 20 {
                    alive.fetch_add(1, Ordering::SeqCst);
                }
            },
        );
        assert_eq!(done.load(Ordering::SeqCst), 15);
        assert_eq!(alive.load(Ordering::SeqCst), 15);
        assert!(cap_s.max_active() <= 4);

        let (dead_s, dead_t) = make_server(1);
        let h = donor_check_posting(
            &[dead_s],
            &ids("dead", 500),
            &no_groups(),
            &everything(0),
            true,
        );
        assert_eq!(h.checked, 10);
        assert!(donor_dead_probe(&h));
        assert_eq!(dead_t.stats.load(Ordering::SeqCst), 10);

        let (live_s, live_t) = make_server(1);
        live_t.has_all.store(true, Ordering::SeqCst);
        let h = donor_check_posting(
            &[live_s],
            &ids("a", 500),
            &no_groups(),
            &everything(0),
            true,
        );
        assert_eq!(h.checked, 500);
        assert_eq!(h.present, 500);

        // Regression test: soft BodyBad is counted even when final verdict remains unsettled on timeout
        let (bad_s, bad_t) = make_server(1);
        let (slow_s, slow_t) = make_server(1);
        bad_t.has_all.store(true, Ordering::SeqCst);
        bad_t.body_gone.store(true, Ordering::SeqCst);
        slow_t.delay_ms.store(300, Ordering::SeqCst);
        let unsettled_body_items = vec![DonorItem {
            message_id: "unsettled_body".to_string(),
            body: true,
            groups: no_groups(),
        }];
        let h_unsettled = donor_check_items(&[bad_s, slow_s], &unsettled_body_items, 40);
        assert_eq!(h_unsettled.checked, 1);
        assert_eq!(h_unsettled.answered(), 0);
        assert_eq!(h_unsettled.body_checked, 1);
        assert_eq!(h_unsettled.body_bad, 1);

        donor_stop_all();
        donor_wait_all();
        donor_reset();
    }

    #[test]
    fn posting_identity_and_budget_suite_port_from_cpp() {
        let _donor_guard = DONOR_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        donor_stop_all();
        donor_wait_all();
        donor_reset();
        // Port of tests/dupesearch/Posting.cpp exercising production Rust functions:
        // 1. NativeIdentityFetchExceptionTest
        let heads = vec![
            (b"first.mkv".to_vec(), b"a@x".to_vec(), 100i64),
            (b"second.mkv".to_vec(), b"b@x".to_vec(), 100i64),
        ];
        let groups = vec!["alt.binaries.test".to_string()];
        let mut attempts = 0;
        let now_sec = 1000i64;
        let deadline = now_sec + 10;
        let found = dupe_identity_read(
            200,
            &heads,
            &groups,
            deadline,
            || now_sec,
            || false,
            |msg_id, grp, dl| {
                attempts += 1;
                let call_no = attempts;
                dupe_identity_fetch_article(
                    msg_id,
                    grp,
                    dl,
                    || now_sec,
                    || false,
                    move |_bracketed_id, _g| {
                        if call_no == 1 {
                            panic!("fake transport exception");
                        }
                        Ok(FetchedArticleResult {
                            success: true,
                            offset: 0,
                            file_size: 1000,
                            data: b"v".to_vec(),
                        })
                    },
                    || {},
                )
            },
        );
        assert_eq!(attempts, 2);
        let found = found.expect("identity should be recovered from second head");
        assert_eq!(found.bytes, 1000);

        // 2. NativeFetchBudgetChargesRetriesTest
        let mut calls = 0;
        let fetcher = NzbFetcherCore::new();
        let budget = crate::dupewatch::FetchBudget::from_max_donors(1);
        let mut listing = NewznabResult::default();
        listing.indexer = b"idx".to_vec();
        listing.title = b"Title".to_vec();
        listing.link = b"http://127.0.0.1/nzb".to_vec();
        let fetched = fetcher.fetch_with(
            &listing,
            now_sec + 20,
            3,
            Some(&budget),
            || now_sec,
            donor_now_ms,
            || false,
            || {},
            |_, _, _, _| {
                calls += 1;
                (true, 200, b"<html>indexer error</html>".to_vec())
            },
            |_| {},
            |_| {},
        );
        assert_eq!(fetched.reason, NzbFetchReason::Parse);
        assert_eq!(calls, 3);
        assert!(budget.spent());

        // 3. NativeFetchBudgetCooldownIsFreeTest
        let mut calls2 = 0;
        let fetcher2 = NzbFetcherCore::new();
        let budget2 = crate::dupewatch::FetchBudget::from_max_donors(1);
        let first = fetcher2.fetch_with(
            &listing,
            now_sec + 20,
            0,
            Some(&budget2),
            || now_sec,
            donor_now_ms,
            || false,
            || {},
            |_, _, _, _| {
                calls2 += 1;
                (false, 403, Vec::new())
            },
            |_| {},
            |_| {},
        );
        assert_eq!(first.reason, NzbFetchReason::Fetch);
        let second = fetcher2.fetch_with(
            &listing,
            now_sec + 20,
            0,
            Some(&budget2),
            || now_sec,
            donor_now_ms,
            || false,
            || {},
            |_, _, _, _| {
                calls2 += 1;
                (false, 403, Vec::new())
            },
            |_| {},
            |_| {},
        );
        assert_eq!(second.reason, NzbFetchReason::Refused);
        assert_eq!(calls2, 1);
        assert!(budget2.take());
        assert!(budget2.take());
        assert!(!budget2.take());
    }
}
