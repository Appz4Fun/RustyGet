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
}
