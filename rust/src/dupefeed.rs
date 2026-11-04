// Hot-path Rust port for DupeSearch, Fleet, DonorHealth, Newznab,
// FeedCoordinator, FeedInfo, and FeedFile.

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

// ============================================================================
// 5. FeedCoordinator, FeedInfo, and FeedFile (`daemon/feed/*.cpp`)
// ============================================================================

/// `NzbInfoCreator::Create` `.nzb` extension normalization.
pub fn normalize_feed_nzb_name(filename: &[u8]) -> Vec<u8> {
    let mut stem = filename;
    if let Some(dot) = filename.iter().rposition(|&b| b == b'.') {
        if filename[dot..].eq_ignore_ascii_case(b".nzb") {
            stem = &filename[..dot];
        }
    }
    if stem.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(stem.len() + 4);
    out.extend_from_slice(stem);
    out.extend_from_slice(b".nzb");
    out
}

/// `FeedCoordinator::FilterHelper::CalcDupeStatus` bitmask formatter.
pub fn format_dupe_status_mask(dupe_status: i32) -> Vec<u8> {
    const TABLE: [(i32, &[u8]); 5] = [
        (1, b"QUEUED"),
        (2, b"DOWNLOADING"),
        (4, b"SUCCESS"),
        (8, b"WARNING"),
        (16, b"FAILURE"),
    ];
    let mut out = Vec::new();
    for (bit, name) in TABLE {
        if (dupe_status & bit) != 0 {
            if !out.is_empty() {
                out.push(b',');
            }
            out.extend_from_slice(name);
        }
    }
    out
}

/// `FeedCoordinator::Run` feed schedule check.
/// Returns `(should_start_download, set_force)`.
pub fn should_start_feed(
    interval_min: i32,
    next_update: i64,
    current_time: i64,
    fetch: bool,
    status: i32,
    fs_running: i32,
) -> (bool, bool) {
    let interval_sec = (interval_min as i64).saturating_mul(60);
    let due = (interval_min > 0
        && (next_update == 0
            || current_time >= next_update
            || current_time < next_update - interval_sec))
        || fetch;
    if due && status != fs_running {
        (true, false)
    } else if fetch {
        (false, true)
    } else {
        (false, false)
    }
}

/// `FeedCoordinator::SchedulerNextUpdate` interval calculator.
/// Returns `(interval_sec, new_last_interval_sec)`.
pub fn calc_feed_next_interval(
    success: bool,
    interval_min: i32,
    last_interval_sec: i32,
) -> (i32, i32) {
    let max_interval = interval_min.saturating_mul(60);
    if success {
        (max_interval, 0)
    } else {
        let mut interval = last_interval_sec.saturating_mul(2);
        interval = interval.max(60);
        interval = interval.min(max_interval);
        (interval, interval)
    }
}

/// `FeedCoordinator::CleanupCache` expiration check.
pub fn is_feed_cache_expired(last_usage: i64, cache_time_sec: i32, cur_time: i64) -> bool {
    last_usage.saturating_add(cache_time_sec as i64) < cur_time || last_usage > cur_time
}

/// `FeedItemInfo::ParsePrefixedInt` in `FeedInfo.cpp`.
pub fn parse_prefixed_int(value: &[u8]) -> i32 {
    if value.is_empty() {
        return 0;
    }
    let slice = if !value[0].is_ascii_digit() {
        &value[1..]
    } else {
        value
    };
    c_atoi_bytes(slice)
}

/// `FeedItemInfo::BuildDupeKey` in `FeedInfo.cpp`.
#[allow(clippy::too_many_arguments)]
pub fn build_feed_dupe_key(
    rage_id_str: &[u8],
    tvdb_id_str: &[u8],
    tvmaze_id_str: &[u8],
    series: &[u8],
    item_rage_id: i32,
    item_tvdb_id: i32,
    item_tvmaze_id: i32,
    item_imdb_id: i32,
    season_num: i32,
    episode_num: i32,
    season: &[u8],
    episode: &[u8],
) -> Vec<u8> {
    let rage_id_val = if !rage_id_str.is_empty() {
        c_atoi_bytes(rage_id_str)
    } else {
        item_rage_id
    };
    let tvdb_id_val = if !tvdb_id_str.is_empty() {
        c_atoi_bytes(tvdb_id_str)
    } else {
        item_tvdb_id
    };
    let tvmaze_id_val = if !tvmaze_id_str.is_empty() {
        c_atoi_bytes(tvmaze_id_str)
    } else {
        item_tvmaze_id
    };

    let has_se = season_num != 0 && episode_num != 0;
    if !series.is_empty() && has_se {
        let mut out = Vec::with_capacity(7 + series.len() + 1 + season.len() + 1 + episode.len());
        out.extend_from_slice(b"series=");
        out.extend_from_slice(series);
        out.push(b'-');
        out.extend_from_slice(season);
        out.push(b'-');
        out.extend_from_slice(episode);
        out
    } else if rage_id_val != 0 && has_se {
        let mut out = format!("rageid={}-", rage_id_val).into_bytes();
        out.extend_from_slice(season);
        out.push(b'-');
        out.extend_from_slice(episode);
        out
    } else if tvdb_id_val != 0 && has_se {
        let mut out = format!("tvdbid={}-", tvdb_id_val).into_bytes();
        out.extend_from_slice(season);
        out.push(b'-');
        out.extend_from_slice(episode);
        out
    } else if tvmaze_id_val != 0 && has_se {
        let mut out = format!("tvmazeid={}-", tvmaze_id_val).into_bytes();
        out.extend_from_slice(season);
        out.push(b'-');
        out.extend_from_slice(episode);
        out
    } else if item_imdb_id != 0 {
        format!("imdb={}", item_imdb_id).into_bytes()
    } else {
        Vec::new()
    }
}

/// `FeedFile::ParseSubject` in `FeedFile.cpp`.
pub fn parse_feed_subject(title: &[u8]) -> Vec<u8> {
    if let Some(first_quote) = title.iter().position(|&b| b == b'"') {
        let start = first_quote + 1;
        if start + 1 <= title.len() {
            if let Some(rel_end) = title[start + 1..].iter().position(|&b| b == b'"') {
                let end = start + 1 + rel_end;
                let quoted = &title[start..end];
                if quoted.len() > 1 && quoted[1..].contains(&b'.') {
                    let mut filename = quoted;
                    if let Some(dot) = filename.iter().rposition(|&b| b == b'.') {
                        if filename[dot..].eq_ignore_ascii_case(b".par2") {
                            filename = &filename[..dot];
                        }
                    }
                    return filename.to_vec();
                }
            }
        }
    }
    title.to_vec()
}

/// `FeedFile::ExtractSizeFromDescription` in `FeedFile.cpp`.
pub fn extract_size_from_description(description: &[u8]) -> i64 {
    extern "C" {
        fn strtod(nptr: *const std::ffi::c_char, endptr: *mut *mut std::ffi::c_char) -> f64;
    }

    let pos_opt = description
        .windows(4)
        .position(|w| w == b"Size")
        .or_else(|| description.windows(4).position(|w| w == b"size"));
    let Some(mut pos) = pos_opt else {
        return 0;
    };
    pos += 4;

    let mut size: f64 = 0.0;
    let mut found_size = false;
    let mut units: &[u8] = b"";

    while pos < description.len() {
        let ch = description[pos];
        if ch == b':' || ch.is_ascii_whitespace() {
            pos += 1;
            continue;
        }
        if !found_size {
            let tail = &description[pos..];
            let mut c_buf = Vec::with_capacity(tail.len() + 1);
            for &b in tail {
                if b == 0 {
                    break;
                }
                c_buf.push(b);
            }
            c_buf.push(0);
            let mut end_ptr: *mut std::ffi::c_char = std::ptr::null_mut();
            let val = unsafe {
                strtod(
                    c_buf.as_ptr() as *const std::ffi::c_char,
                    &mut end_ptr as *mut *mut std::ffi::c_char,
                )
            };
            if end_ptr == c_buf.as_ptr() as *mut std::ffi::c_char || !val.is_finite() {
                break;
            }
            found_size = true;
            size = val;
            while pos < description.len() && !description[pos].is_ascii_whitespace() {
                pos += 1;
            }
            pos += 1;
            continue;
        }

        if pos + 2 <= description.len() {
            units = &description[pos..pos + 2];
            break;
        }
        pos += 1;
    }

    match units {
        b"KB" => size *= 1024.0,
        b"MB" => size *= 1024.0 * 1024.0,
        b"GB" => size *= 1024.0 * 1024.0 * 1024.0,
        b"TB" => size *= 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => {}
    }

    size.round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_feed_helpers() {
        assert_eq!(normalize_feed_nzb_name(b"Movie.NZB"), b"Movie.nzb");
        assert_eq!(format_dupe_status_mask(1 | 4 | 16), b"QUEUED,SUCCESS,FAILURE");
        assert_eq!(parse_prefixed_int(b"S03"), 3);
        assert_eq!(
            build_feed_dupe_key(b"", b"", b"", b"", 0, 439270, 0, 28369634, 2, 69, b"S02", b"E69"),
            b"tvdbid=439270-S02-E69"
        );
        assert_eq!(
            parse_feed_subject(b"Post [1/10] - \"My.Release.par2\" yEnc (1/1)"),
            b"My.Release"
        );
        assert_eq!(
            extract_size_from_description(b"Total Size : 299.6 MB | MultiUp"),
            (299.6f64 * 1024.0 * 1024.0).round() as i64
        );
        assert_eq!(
            extract_size_from_description(b"Size: 1e3 MB"),
            1000 * 1024 * 1024
        );
        assert_eq!(extract_size_from_description(b"Size: 300"), 300);
    }
}
