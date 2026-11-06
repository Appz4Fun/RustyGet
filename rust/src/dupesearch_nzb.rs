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

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
unsafe extern "C" {
    fn tolower(ch: std::ffi::c_int) -> std::ffi::c_int;
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
unsafe extern "C" {
    fn __ctype_tolower_loc() -> *const *const std::ffi::c_int;
}

/// Keep C locale case folding used by DupeUtil::Lower, including unsigned bytes.
pub fn is_par_name(name: &[u8]) -> bool {
    if name.len() < 5 {
        return false;
    }
    let tail = &name[name.len() - 5..];
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // glibc's inline C/C++ tolower uses this per-thread locale table. Obtain it once,
        // preserving every byte's locale mapping without five separate native calls.
        // SAFETY: glibc guarantees entries for all unsigned char values; nothing escapes.
        let table = unsafe { *__ctype_tolower_loc() };
        tail.iter()
            .zip(b".par2")
            .all(|(&a, &b)| unsafe { *table.add(usize::from(a)) == i32::from(b) })
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        tail.iter().zip(b".par2").all(|(&a, &b)| {
            // SAFETY: an unsigned byte is always in the C tolower argument domain.
            unsafe { tolower(a.into()) == i32::from(b) }
        })
    }
}

#[repr(C)]
pub struct SubjectRange {
    pub start: usize,
    pub len: usize,
}

unsafe fn bytes<'a>(data: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    if data.is_null() || len > isize::MAX as usize {
        return None;
    }
    // SAFETY: callers promise a readable allocation of len bytes for this call.
    Some(unsafe { std::slice::from_raw_parts(data, len) })
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

/// # Safety
/// data must point to len readable bytes (or be null for zero len).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupesearch_name_of_subject(
    data: *const u8,
    len: usize,
) -> SubjectRange {
    let (start, end) = unsafe { bytes(data, len) }
        .map(name_of_subject_range)
        .unwrap_or((0, 0));
    SubjectRange {
        start,
        len: end - start,
    }
}

/// # Safety
/// data must point to len readable bytes (or be null for zero len).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupesearch_unquoted_subject(
    data: *const u8,
    len: usize,
) -> SubjectRange {
    let (start, end) = unsafe { bytes(data, len) }
        .map(unquoted_subject_range)
        .unwrap_or((0, 0));
    SubjectRange {
        start,
        len: end - start,
    }
}

/// # Safety
/// data must point to len readable bytes (or be null for zero len).
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_dupesearch_par_name(data: *const u8, len: usize) -> i32 {
    i32::from(unsafe { bytes(data, len) }.is_some_and(is_par_name))
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
        (new_files, new_bytes, new_geom, push_parity_head, push_data_head)
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
/// Computes the `AliveParam` percentage for a rescored fast donor.
pub fn dupesearch_fast_donor_alive_param(is_dead: bool, alive: f64) -> i32 {
    if is_dead {
        0
    } else if alive < 0.0 {
        100
    } else {
        (100.0 * alive).round() as i32
    }
}

/// In `DupeSearch::Place` (`slotsUsed`), evaluates whether a placed entry occupies a donor slot.
#[inline]
pub fn dupesearch_slot_counts_as_used(
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

/// In `DupeSearch::Place`, evaluates whether a placed entry qualifies as a usable whole backup.
#[inline]
pub fn dupesearch_is_usable_whole_backup(
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

/// In `DupeSearch::AddDonor`, returns `true` when `.nzb` should be appended to a title.
#[inline]
pub fn dupesearch_needs_nzb_ext(name: &[u8]) -> bool {
    name.len() < 4 || !name[name.len() - 4..].eq_ignore_ascii_case(b".nzb")
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
pub fn dupesearch_calc_expand_budget_ms(
    budget_sec: i32,
    deadline: i64,
    now: i64,
) -> i32 {
    let opt_ms = i64::from(budget_sec).saturating_mul(1000);
    let left_ms = deadline.saturating_sub(now).saturating_mul(1000);
    let chosen = opt_ms.min(left_ms).max(1);
    chosen.clamp(1, i64::from(i32::MAX)) as i32
}

/// Port of watchdog wait & stop calculation in `HttpGet::Fetch`.
/// Returns `(should_stop, wait_ms)`.
pub fn http_get_calc_watchdog_wait(
    deadline_ms: i64,
    now_ms: i64,
    file_size: i64,
    max_bytes: usize,
) -> (bool, i64) {
    let left = if deadline_ms > 0 {
        deadline_ms.saturating_sub(now_ms)
    } else {
        250
    };
    if left <= 0 || file_size > (max_bytes as i64) {
        (true, 0)
    } else {
        (false, left.min(250))
    }
}

/// Port of `LocalName` in `XmlReader.cpp`.
/// Returns the byte offset of the local name (after the last `':'`, or `0`).
pub fn xml_reader_local_name_offset(name: &[u8]) -> usize {
    match name.iter().rposition(|&b| b == b':') {
        Some(pos) => pos + 1,
        None => 0,
    }
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
    let (title_off, title_len, pos4) = next_tab_field(pos3).unwrap_or((line.len(), 0, line.len() + 1));
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
/// Returns number of ranked indices written to `out_ranked`.
pub fn rank_fleet_candidates(
    has_error: &[u8],
    same_as: &[i32],
    dead: &[u8],
    alive: &[i32],
    total_bytes: &[i64],
    out_ranked: &mut [usize],
) -> usize {
    let n = has_error
        .len()
        .min(same_as.len())
        .min(dead.len())
        .min(alive.len())
        .min(total_bytes.len());
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
pub fn donor_sample_count(
    total_ids: usize,
    percent: i32,
    minimum: i32,
    maximum: i32,
) -> usize {
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
    text.iter().copied().filter(|b| b.is_ascii_digit()).collect()
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

/// Returns whether a history item is an eligible successful download before
/// checking `ReleaseName::Readable` and `FilesOnDisk` (`Fleet::Append`).
pub fn fleet_is_already_downloaded_history_item(
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
pub fn fleet_calc_member_add_score(
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
pub fn donor_health_error_increment(busy: bool) -> i32 {
    if busy { 0 } else { 1 }
}

/// Returns whether the server's error counter has reached `server_give_up` and
/// should trigger a pause and reset (`DonorHealth::Server::Ask`).
pub fn donor_health_should_pause_server(errors: i32, server_give_up: i32) -> bool {
    errors >= server_give_up
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

/// Accumulates `DonorHealth::Health::operator+=` counters.
#[allow(clippy::too_many_arguments)]
pub fn donor_health_accum_health(
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
pub fn donor_health_calc_worker_budget_ms(
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
pub fn donor_health_calc_thread_count(postings_len: usize, concurrency: i32) -> usize {
    postings_len.min(1i32.max(concurrency) as usize)
}

/// Formats the HTTP fetch error message for `Fleet::Append`.
pub fn fleet_format_fetch_error(status: i32) -> Vec<u8> {
    if status != 0 {
        format!("the nzb-file could not be fetched (HTTP {status})").into_bytes()
    } else {
        b"the nzb-file could not be fetched (connection failed)".to_vec()
    }
}

/// Computes `(sample_maximum, budget_ms, check_deadline_ms)` for `Fleet::Append`.
pub fn fleet_calc_check_budget_and_deadline(
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
pub fn fleet_candidate_tier(has_error: bool, dead: bool, alive: i32) -> i32 {
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

/// Evaluates whether `DupeSearch::PrepareNative` generated the download's key (`DupeSearch::PrepareNative`).
pub fn dupesearch_is_generated_key(
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
pub fn dupesearch_should_check_added_for_parent_rank(
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

/// Evaluates whether another queue/history item should be retained in the peer list
/// for `DupeSearch::PrepareNative`.
pub fn dupesearch_should_retain_peer(
    is_same_item: bool,
    item_score: i32,
    pick_score: i32,
    has_dupe_fleet_param: bool,
) -> bool {
    !is_same_item && (item_score > pick_score || has_dupe_fleet_param)
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

/// Formats `"<param_name>=<val>"` for `DupeSearch`.
pub fn dupesearch_format_key_val_param(param_name: &[u8], val: i32) -> Vec<u8> {
    let val_str = format!("={val}");
    let mut out = Vec::with_capacity(param_name.len() + val_str.len());
    out.extend_from_slice(param_name);
    out.extend_from_slice(val_str.as_bytes());
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

/// Formats `"<dir><sep><idx>"` for `DupeSearch::SavePending` and `DupeSearch::ResumePending`.
pub fn dupesearch_format_indexed_base_path(dir: &[u8], sep: u8, idx: usize) -> Vec<u8> {
    let suffix = idx.to_string();
    let mut out = Vec::with_capacity(dir.len() + 1 + suffix.len());
    out.extend_from_slice(dir);
    out.push(sep);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Formats `idx` as a decimal string for `DupeSearch`.
pub fn dupesearch_format_usize_str(idx: usize) -> Vec<u8> {
    idx.to_string().into_bytes()
}

/// Formats `"HTTP <status>"` for `NzbFetcher::Fetch`.
pub fn nzb_fetcher_format_http_status_err(status: i32) -> Vec<u8> {
    format!("HTTP {status}").into_bytes()
}

/// Formats `"<temp_dir><sep>dupesearch-<now_sec>-<counter>.tmp"` for `HttpGet::Fetch`.
pub fn http_get_format_temp_path(
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
pub fn fleet_calc_check_lock_wait_ms(
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

/// Returns the NUL-terminated status suffix for `DupeSearch::PrepareNative` search log message.
pub fn dupesearch_prepare_status_suffix(rank_only: bool, history: bool) -> &'static [u8] {
    if rank_only {
        b": ranking new backups\0"
    } else if history {
        b": it failed before its search was due\0"
    } else {
        b"\0"
    }
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
    fn ffi_rejects_null_nonempty_and_unrepresentable_lengths() {
        // These inputs must be rejected before constructing a slice.
        unsafe {
            for len in [0, 1, usize::MAX] {
                let range = nzbget_rs_dupesearch_unquoted_subject(std::ptr::null(), len);
                assert_eq!((range.start, range.len), (0, 0));
                assert_eq!(nzbget_rs_dupesearch_par_name(std::ptr::null(), len), 0);
            }
            let range = nzbget_rs_dupesearch_unquoted_subject(b"a".as_ptr(), usize::MAX);
            assert_eq!((range.start, range.len), (0, 0));
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
        sort_verified_postings(&[1050, 1000, 1000], &[5, 2, 10], &[100, 200, 150], 1000, &mut order);
        assert_eq!(order, [2, 1, 0]);

        assert_eq!(eval_member_rerank(1000, 50, 1, 1020, 0.90, false), (1020, 90, false));
        assert_eq!(eval_member_rerank(1000, 50, 1, 1020, 1.00, false), (1050, 100, false));
        assert_eq!(eval_member_rerank(1000, 1, 1, 1020, 0.0, true), (1020, 0, true));

        let meta = format_pending_meta(b"Release.A", b"NZBGeek", 42, 9876543210, 1749517805);
        let parsed = parse_pending_meta(&meta);
        assert_eq!(&meta[parsed.title_offset..parsed.title_offset + parsed.title_len], b"Release.A");
        assert_eq!(&meta[parsed.indexer_offset..parsed.indexer_offset + parsed.indexer_len], b"NZBGeek");
        assert_eq!(parsed.grabs, 42);
        assert_eq!(parsed.size, 9876543210);
        assert_eq!(parsed.date, 1749517805);

        let st = parse_dupesearch_state_line(b"dupes:rel\t1000\t1700000000\trel\t0\t", 1700000100, 86400);
        assert_eq!(st.valid, 1);
        assert_eq!(st.within_ttl, 1);
        assert_eq!(st.score, 1000);
        assert_eq!(st.retryable, 0);
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
            &mut ranked,
        );
        assert_eq!(count, 3);
        assert_eq!(ranked, [2, 0, 1]);

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
        assert_eq!(parse_newznab_date(b"Tue, 10 Jun 2025 01:10:05 +0000"), 1749517805);
        assert_eq!(parse_newznab_date(b"Tue, 10 Jun 2025 01:10:05+0000"), 1749517805);
        assert_eq!(parse_newznab_date(b"10 Jun 2025 01:10:05EST"), 1749517805 + 5 * 3600);
        assert_eq!(parse_newznab_date(b"10 Jun 2025 01:10:05 EST"), 1749517805 + 5 * 3600);
        assert_eq!(parse_newznab_date(b"2025-06-10T03:10:05+02:00"), 1749517805);
        assert_eq!(parse_newznab_date(b"10 Jun 2025 01:10:05 XYZ"), 0);

        let url = build_newznab_url(
            b"http://h:5076",
            &[b"t", b"q"],
            &[b"search", b"show s01e02"],
            b"k&y 1",
        );
        assert_eq!(url, b"http://h:5076/api?t=search&q=show+s01e02&apikey=k%26y+1");

        assert_eq!(
            mask_newznab_text(b"http://u:p@h/api?APIKEY=SECRET&q=x"),
            b"http://***@h/api?APIKEY=***&q=x"
        );
    }
}
