//! Hot-path Rust implementations for duplicate article fallback (`DupeArticleFallback`),
//! PAR2 block-damage estimation (`ParDamage`), early dead-posting probing (`DupeProbe`),
//! NNTP server pool level normalization/hold management (`ServerPool`), and
//! NNTP article fetch/download bounds and response classification (`ArticleFetcher`,
//! `ArticleDownloader`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const PART_SIZE_TOLERANCE_DIV: i32 = 16;
pub const TOTAL_SIZE_TOLERANCE_DIV: i32 = 64;
pub const LEAD_DEMOTE_THRESHOLD: i32 = 3;
pub const MIN_STEP_FINGERPRINT_ARTICLES: usize = 8;
pub const PROJECTION_SAMPLE: i32 = 200;
pub const PAR_EDGE_MARGIN: i32 = 50;
pub const MAX_PAR2_BLOCK_SIZE: u64 = 256 * 1024 * 1024;
pub const MAX_PAR2_PACKET_BODY: u64 = 64 * 1024 * 1024;

pub const ARTICLE_STATUS_FINISHED: i32 = 2;

pub const DUPE_PROBE_EXISTS: i32 = 0;
pub const DUPE_PROBE_MISSING: i32 = 1;
pub const DUPE_PROBE_UNKNOWN: i32 = 2;
pub const DUPE_PROBE_MIN_ALIVE_SAMPLES: i32 = 2;

pub const ARTICLE_FETCH_MAX_DECODED_BYTES: i64 = 64 * 1024 * 1024;
pub const ARTICLE_FETCH_MAX_HEADER_BYTES: i64 = 64 * 1024;
pub const ARTICLE_FETCH_MAX_RAW_BYTES: i64 =
    ARTICLE_FETCH_MAX_DECODED_BYTES * 3 + ARTICLE_FETCH_MAX_HEADER_BYTES;

pub const AD_FINISHED: i32 = 3;
pub const AD_FAILED: i32 = 4;
pub const AD_NOT_FOUND: i32 = 7;
pub const AD_CONNECT_ERROR: i32 = 8;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NzbgetRsArticleGeom {
    pub part_number: i32,
    pub status: i32,
    pub segment_offset: i64,
    pub segment_size: i32,
    pub fallback_round: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NzbgetRsByteRange {
    pub offset: i64,
    pub size: i64,
}

/// Checks whether damaged data bytes (or projected damage from articles tried so far)
/// exceed available PAR2 recovery bytes.
pub fn par_cannot_cover(
    par_size: i64,
    par_failed_size: i64,
    current_failed_size: i64,
    current_success_size: i64,
    par_current_success_size: i64,
    total_size: i64,
    current_success_articles: i32,
    current_failed_articles: i32,
    with_margin: bool,
) -> bool {
    let par_available = par_size.saturating_sub(par_failed_size);
    let data_failed = current_failed_size.saturating_sub(par_failed_size);
    if data_failed <= 0 {
        return false;
    }
    if data_failed > par_available {
        return true;
    }
    let tried = current_success_articles.saturating_add(current_failed_articles);
    let data_total = total_size.saturating_sub(par_size);
    let data_tried = current_success_size
        .saturating_sub(par_current_success_size)
        .saturating_add(data_failed);
    if tried < PROJECTION_SAMPLE || data_tried <= 0 || data_total <= 0 {
        return false;
    }
    let projected_failed = (data_failed as f64) * (data_total as f64) / (data_tried as f64);
    let margin = if with_margin {
        (data_total as f64) * (PAR_EDGE_MARGIN as f64) / 1000.0
    } else {
        0.0
    };
    projected_failed + margin > (par_available as f64)
}

/// True when `filename` ends with `.` followed by 1..=4 ASCII alphanumeric characters,
/// and has at least one byte before the last `.`.
pub fn has_extension(filename: &[u8]) -> bool {
    let Some(dot_pos) = filename.iter().rposition(|&b| b == b'.') else {
        return false;
    };
    if dot_pos == 0 {
        return false;
    }
    let ext = &filename[dot_pos + 1..];
    if ext.is_empty() || ext.len() > 4 {
        return false;
    }
    ext.iter().all(|b| b.is_ascii_alphanumeric())
}

/// True when `|size1 - size2| <= max(size1, size2) / div`.
pub fn sizes_match(size1: i64, size2: i64, div: i32) -> bool {
    if div <= 0 {
        return false;
    }
    let diff = if size1 > size2 {
        (size1 as i128) - (size2 as i128)
    } else {
        (size2 as i128) - (size1 as i128)
    };
    let max_val = size1.max(size2) as i128;
    diff <= max_val / (div as i128)
}

fn parse_c_int_prefix(bytes: &[u8]) -> Option<(i32, usize)> {
    let mut idx = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    if idx >= bytes.len() {
        return None;
    }
    let mut sign: i64 = 1;
    if bytes[idx] == b'+' {
        idx += 1;
    } else if bytes[idx] == b'-' {
        sign = -1;
        idx += 1;
    }
    let start_digits = idx;
    let mut acc: i64 = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        acc = acc
            .saturating_mul(10)
            .saturating_add((bytes[idx] - b'0') as i64);
        idx += 1;
    }
    if idx == start_digits {
        return None;
    }
    let signed = acc.saturating_mul(sign);
    let val = signed.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    Some((val, idx))
}

/// Extracts the last `(part/count)` declared part count in a yEnc subject, matching
/// `sscanf(p, "(%d/%d%c", &part, &count, &close) == 3 && close == ')' && part >= 0 && count > 0`.
pub fn declared_parts(subject: &[u8]) -> i32 {
    let mut declared = 0;
    for (i, &b) in subject.iter().enumerate() {
        if b != b'(' {
            continue;
        }
        let rest = &subject[i + 1..];
        let Some((part, used1)) = parse_c_int_prefix(rest) else {
            continue;
        };
        let after_part = &rest[used1..];
        if after_part.first() != Some(&b'/') {
            continue;
        }
        let after_slash = &after_part[1..];
        let Some((count, used2)) = parse_c_int_prefix(after_slash) else {
            continue;
        };
        let after_count = &after_slash[used2..];
        if after_count.first() == Some(&b')') && part >= 0 && count > 0 {
            declared = count;
        }
    }
    declared
}

/// Computes FNV-1a over the article count and consecutive article size steps.
/// Returns 0 if fewer than `MIN_STEP_FINGERPRINT_ARTICLES` articles or no non-last step varies.
pub fn article_size_steps_hash(sizes: &[i32]) -> u64 {
    if sizes.len() < MIN_STEP_FINGERPRINT_ARTICLES {
        return 0;
    }
    let mut hash: u64 = 14695981039346656037;
    let mut mix = |value: i64| {
        for i in 0..8 {
            hash ^= ((value >> (i * 8)) & 0xff) as u64;
            hash = hash.wrapping_mul(1099511628211);
        }
    };
    mix(sizes.len() as i64);
    let mut varying = false;
    for i in 1..sizes.len() {
        let step = sizes[i].wrapping_sub(sizes[i - 1]);
        mix(step as i64);
        if step != 0 && i < sizes.len() - 1 {
            varying = true;
        }
    }
    if varying && hash != 0 {
        hash
    } else {
        0
    }
}

/// Checks whether two files have identical non-constant consecutive article size steps.
pub fn article_size_steps_match(target_sizes: &[i32], donor_sizes: &[i32]) -> bool {
    if target_sizes.len() < MIN_STEP_FINGERPRINT_ARTICLES || target_sizes.len() != donor_sizes.len()
    {
        return false;
    }
    let mut varying = false;
    for i in 1..target_sizes.len() {
        let target_step = target_sizes[i].wrapping_sub(target_sizes[i - 1]);
        let donor_step = donor_sizes[i].wrapping_sub(donor_sizes[i - 1]);
        if target_step != donor_step {
            return false;
        }
        if target_step != 0 && i < target_sizes.len() - 1 {
            varying = true;
        }
    }
    varying
}

/// Structural match between target and donor files, supporting both exact segment lists
/// and missing-segment gap tolerance (up to `max(1, donor_len / 16)` extra parts and `<= 1` trailing).
pub fn structure_matches(
    target_subject: &[u8],
    target_total_articles: i32,
    target_size: i64,
    target_parts: &[i32],
    target_sizes: &[i32],
    donor_subject: &[u8],
    donor_total_articles: i32,
    donor_size: i64,
    donor_parts: &[i32],
    donor_sizes: &[i32],
) -> bool {
    if target_parts.is_empty()
        || target_parts.len() != target_sizes.len()
        || donor_parts.len() != donor_sizes.len()
    {
        return false;
    }

    let target_declared = declared_parts(target_subject);
    let donor_declared = declared_parts(donor_subject);
    if target_declared > 0 && donor_declared > 0 && target_declared != donor_declared {
        return false;
    }
    if target_declared > 0 && donor_parts.iter().any(|&p| p > target_declared) {
        return false;
    }

    if target_parts.len() < donor_parts.len() {
        let extra = donor_parts.len() - target_parts.len();
        if extra > 1usize.max(donor_parts.len() / 16) {
            return false;
        }
        let mut donor_map = BTreeMap::new();
        for (&part, &size) in donor_parts.iter().zip(donor_sizes.iter()) {
            donor_map.insert(part, size);
        }
        let mut last_target_part = 0i32;
        let mut target_part_set = BTreeSet::new();
        for (&part, &size) in target_parts.iter().zip(target_sizes.iter()) {
            let Some(&d_size) = donor_map.get(&part) else {
                return false;
            };
            if !sizes_match(size as i64, d_size as i64, PART_SIZE_TOLERANCE_DIV) {
                return false;
            }
            target_part_set.insert(part);
            last_target_part = last_target_part.max(part);
        }
        let mut trailing = 0i32;
        let mut trailing_size = 0i64;
        for (&part, &size) in donor_parts.iter().zip(donor_sizes.iter()) {
            if !target_part_set.contains(&part) && part > last_target_part {
                trailing += 1;
                trailing_size = trailing_size.saturating_add(size as i64);
            }
        }
        return trailing <= 1
            && sizes_match(
                target_size.saturating_add(trailing_size),
                donor_size,
                TOTAL_SIZE_TOLERANCE_DIV,
            );
    }

    if target_parts.len() != donor_parts.len()
        || target_total_articles != donor_total_articles
        || !sizes_match(target_size, donor_size, TOTAL_SIZE_TOLERANCE_DIV)
    {
        return false;
    }

    for i in 0..target_parts.len() {
        if target_parts[i] != donor_parts[i]
            || !sizes_match(
                target_sizes[i] as i64,
                donor_sizes[i] as i64,
                PART_SIZE_TOLERANCE_DIV,
            )
        {
            return false;
        }
    }

    true
}

/// Computes the expected decoded byte offset of article at `index` from its left neighbour.
pub fn expected_segment_offset(
    index: isize,
    part_number: i32,
    prev_part_number: i32,
    prev_status: i32,
    prev_offset: i64,
    prev_size: i32,
) -> i64 {
    if index < 0 {
        return -1;
    }
    if part_number == 1 {
        return 0;
    }
    if index == 0 {
        return -1;
    }
    if prev_part_number == part_number.wrapping_sub(1)
        && prev_status == ARTICLE_STATUS_FINISHED
        && prev_size > 0
    {
        return prev_offset.saturating_add(prev_size as i64);
    }
    -1
}

/// Computes the expected decoded byte end of article at `index` from its right neighbour or file size.
pub fn expected_segment_end(
    index: isize,
    count: usize,
    part_number: i32,
    decoded_file_size: i64,
    next_part_number: i32,
    next_status: i32,
    next_offset: i64,
    next_size: i32,
) -> i64 {
    if index < 0 || (index as usize) >= count {
        return -1;
    }
    if (index as usize) + 1 == count {
        return if decoded_file_size > 0 {
            decoded_file_size
        } else {
            -1
        };
    }
    if next_part_number == part_number.wrapping_add(1)
        && next_status == ARTICLE_STATUS_FINISHED
        && next_size > 0
    {
        return next_offset;
    }
    -1
}

/// Checks whether a finished segment `[segment_offset, segment_offset + segment_size)` aligns
/// with its expected start and end bounds.
pub fn segment_aligned(
    segment_offset: i64,
    segment_size: i32,
    expected_offset: i64,
    expected_end: i64,
) -> bool {
    if segment_size <= 0 {
        return true;
    }
    if expected_offset >= 0 && segment_offset != expected_offset {
        return false;
    }
    if expected_end >= 0 && segment_offset.saturating_add(segment_size as i64) != expected_end {
        return false;
    }
    true
}

/// Inspects a completed file's article list and returns the index of the first article
/// that breaks contiguous yEnc byte tiling `[0..decoded_size)`, or `-1` if tiled/incomplete.
pub fn first_untiled_article(articles: &[NzbgetRsArticleGeom], decoded_size: i64) -> isize {
    if decoded_size <= 0 {
        return -1;
    }
    let mut expected: i64 = 0;
    for (index, article) in articles.iter().enumerate() {
        if article.status != ARTICLE_STATUS_FINISHED || article.segment_size <= 0 {
            return -1;
        }
        if article.segment_offset != expected {
            if index > 0 {
                let prev = &articles[index - 1];
                let prev_borrowed = prev.fallback_round > 0;
                let this_borrowed = article.fallback_round > 0;
                if prev_borrowed != this_borrowed {
                    return if prev_borrowed {
                        (index - 1) as isize
                    } else {
                        index as isize
                    };
                }
                let end = article
                    .segment_offset
                    .saturating_add(article.segment_size as i64);
                let placed_right = if index + 1 < articles.len() {
                    let next = &articles[index + 1];
                    next.status == ARTICLE_STATUS_FINISHED
                        && next.segment_size > 0
                        && next.segment_offset == end
                } else {
                    end == decoded_size
                };
                if article.segment_offset > expected && placed_right {
                    return (index - 1) as isize;
                }
            }
            return index as isize;
        }
        expected = article
            .segment_offset
            .saturating_add(article.segment_size as i64);
    }
    if expected != decoded_size {
        return if articles.is_empty() {
            -1
        } else {
            (articles.len() - 1) as isize
        };
    }
    -1
}

/// Merges a newly decoded article's declared file size into the file's `decoded_file_size`.
/// Returns `(conflict, new_decoded_file_size)`.
pub fn merge_decoded_file_size(decoded_file_size: i64, article_file_size: i64) -> (bool, i64) {
    if article_file_size <= 0 || decoded_file_size < 0 || decoded_file_size == article_file_size {
        return (false, decoded_file_size);
    }
    if decoded_file_size == 0 {
        return (false, article_file_size);
    }
    (true, -1)
}

/// Updates file lead-donor state after a round-1 fallback failure.
/// Returns `true` if the lead donor was rotated to `next_lead`.
pub fn register_lead_failure(
    fallback_round: i32,
    lead_snapshot: i32,
    donor_count: i32,
    next_lead: i32,
    lead_donor_id: &mut i32,
    lead_failures: &mut i32,
    lead_switches: &mut i32,
) -> bool {
    if fallback_round != 1 || lead_snapshot == 0 || lead_snapshot != *lead_donor_id {
        return false;
    }
    let failures = lead_failures.saturating_add(1);
    *lead_failures = failures;
    if failures >= LEAD_DEMOTE_THRESHOLD && next_lead != 0 && *lead_switches < donor_count {
        *lead_donor_id = next_lead;
        *lead_failures = 0;
        *lead_switches = lead_switches.saturating_add(1);
        return true;
    }
    false
}

/// Resets file lead-donor failure streak (and switch count when neighbour bounds are known)
/// after a round-1 fallback success.
pub fn register_lead_success(
    fallback_round: i32,
    lead_snapshot: i32,
    lead_donor_id: i32,
    expected_offset: i64,
    expected_end: i64,
    lead_failures: &mut i32,
    lead_switches: &mut i32,
) {
    if fallback_round != 1 || lead_snapshot == 0 || lead_snapshot != lead_donor_id {
        return;
    }
    *lead_failures = 0;
    if expected_offset != -1 && expected_end != -1 {
        *lead_switches = 0;
    }
}

/// Checks whether `[from, to)` is completely covered by a sorted slice of `[offset, offset + size)` ranges.
pub fn range_covered(sorted_ranges: &[NzbgetRsByteRange], mut from: i64, to: i64) -> bool {
    for r in sorted_ranges {
        let start = r.offset;
        let end = r.offset.saturating_add(r.size);
        if start > from {
            return false;
        }
        from = from.max(end);
        if from >= to {
            return true;
        }
    }
    from >= to
}

fn hex_lower(bytes: &[u8]) -> [u8; 32] {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = [0u8; 32];
    for (i, &b) in bytes.iter().take(16).enumerate() {
        out[i * 2] = DIGITS[(b >> 4) as usize];
        out[i * 2 + 1] = DIGITS[(b & 0x0f) as usize];
    }
    out
}

/// Scans a single PAR2 file on disk for `Main`, `FileDesc`, and `IFSC` packets, skipping
/// recovery packets via `Seek` without reading their bodies.
pub fn scan_par2_file<FMain, FDesc, FIfsc>(
    path: &Path,
    mut on_main: FMain,
    mut on_desc: FDesc,
    mut on_ifsc: FIfsc,
) where
    FMain: FnMut(&[u8; 32], u64),
    FDesc: FnMut(&[u8; 32], &[u8; 32], &[u8; 32], u64, &[u8]),
    FIfsc: FnMut(&[u8; 32], &[u8; 32], &[u32]),
{
    let Ok(mut file) = File::open(path) else {
        return;
    };
    let mut header = [0u8; 64];
    let mut pos: u64 = 0;
    while file.seek(SeekFrom::Start(pos)).is_ok() && file.read_exact(&mut header).is_ok() {
        if &header[0..8] != b"PAR2\0PKT" {
            break;
        }
        let length = u64::from_le_bytes(header[8..16].try_into().unwrap());
        if length < 64 || length % 4 != 0 {
            break;
        }
        let pkt_type = &header[48..64];
        let set_id = hex_lower(&header[32..48]);
        let body_length = length - 64;
        let is_main = pkt_type == b"PAR 2.0\0Main\0\0\0\0";
        let is_desc = pkt_type == b"PAR 2.0\0FileDesc";
        let is_ifsc = pkt_type == b"PAR 2.0\0IFSC\0\0\0\0";
        if (is_main || is_desc || is_ifsc) && body_length <= MAX_PAR2_PACKET_BODY {
            let mut body = vec![0u8; body_length as usize];
            if file.read_exact(&mut body).is_err() {
                break;
            }
            if is_main && body.len() >= 8 {
                let block_size = u64::from_le_bytes(body[0..8].try_into().unwrap());
                on_main(&set_id, block_size);
            } else if is_desc && body.len() >= 56 {
                let file_id = hex_lower(&body[0..16]);
                let hash16k = hex_lower(&body[32..48]);
                let file_len = u64::from_le_bytes(body[48..56].try_into().unwrap());
                let raw_name = &body[56..];
                let name_end = raw_name
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(raw_name.len());
                on_desc(&set_id, &file_id, &hash16k, file_len, &raw_name[..name_end]);
            } else if is_ifsc && body.len() >= 16 {
                let file_id = hex_lower(&body[0..16]);
                let mut crcs = Vec::with_capacity((body.len() - 16) / 20);
                let mut at = 16usize;
                while at + 20 <= body.len() {
                    let crc = u32::from_le_bytes(body[at + 16..at + 20].try_into().unwrap());
                    crcs.push(crc);
                    at += 20;
                }
                on_ifsc(&set_id, &file_id, &crcs);
            }
        }
        let Some(next_pos) = pos.checked_add(length) else {
            break;
        };
        pos = next_pos;
    }
}

/// Counts distinct blocks of `block_size` touched by `holes` (`ParDamage::BlocksTouched`).
pub fn blocks_touched(holes: &[NzbgetRsByteRange], block_size: u64) -> i32 {
    if block_size == 0 || block_size > (i64::MAX as u64) {
        return 0;
    }
    let bs = block_size as i64;
    let mut spans = Vec::with_capacity(holes.len());
    for hole in holes {
        if hole.size > 0 && hole.offset >= 0 {
            let first = hole.offset / bs;
            let last = hole.offset.saturating_add(hole.size).saturating_sub(1) / bs;
            spans.push((first, last));
        }
    }
    spans.sort_unstable();

    let mut count: i64 = 0;
    let mut covered_to: i64 = -1;
    for (first, last) in spans {
        let from = first.max(covered_to.saturating_add(1));
        if last >= from {
            count = count.saturating_add(last.saturating_sub(from).saturating_add(1));
            covered_to = last;
        }
    }
    count.min(i32::MAX as i64) as i32
}

/// Computes `ParDamage::Verdict` projection and `Certain`/`Projected` flags.
pub fn project_par_damage(
    known: bool,
    damaged_blocks: i32,
    recovery_blocks: i32,
    done_bytes: i64,
    total_bytes: i64,
) -> (i32, bool, bool) {
    let total = total_bytes.max(0);
    let projected_blocks = if done_bytes > 0 {
        let prod = (damaged_blocks as i128) * (total as i128) / (done_bytes as i128);
        prod.clamp(0, i32::MAX as i128) as i32
    } else {
        damaged_blocks
    };
    let certain = known && damaged_blocks > recovery_blocks;
    let projected = known
        && done_bytes.saturating_mul(5) >= total
        && (projected_blocks as i64).saturating_mul(4)
            > (recovery_blocks as i64).saturating_mul(5).saturating_add(8);
    (projected_blocks, certain, projected)
}

/// Computes `DupeProbe::SampleIndexes(total, count)`.
pub fn dupe_probe_sample_indexes(total: usize, count: i32) -> Vec<usize> {
    if total == 0 || count <= 0 {
        return Vec::new();
    }
    let wanted = total.min(count as usize);
    let mut indexes = Vec::with_capacity(wanted);
    for i in 0..wanted {
        let num = (2 * i + 1) as u128 * (total as u128);
        let den = (2 * wanted) as u128;
        indexes.push((num / den) as usize);
    }
    indexes
}

/// Classifies an NNTP `STAT` response for `DupeProbe::Classify`.
pub fn dupe_probe_classify(response: Option<&[u8]>) -> i32 {
    let Some(resp) = response else {
        return DUPE_PROBE_UNKNOWN;
    };
    if resp.starts_with(b"2") {
        return DUPE_PROBE_EXISTS;
    }
    if resp.starts_with(b"400") || resp.starts_with(b"499") {
        return DUPE_PROBE_UNKNOWN;
    }
    if resp.starts_with(b"41")
        || resp.starts_with(b"42")
        || resp.starts_with(b"43")
        || resp.starts_with(b"451")
    {
        return DUPE_PROBE_MISSING;
    }
    DUPE_PROBE_UNKNOWN
}

/// Computes `DupeProbe::IsDead(existing, definitive_servers, active_servers)`.
pub fn dupe_probe_is_dead(existing: i32, definitive_servers: i32, active_servers: i32) -> bool {
    existing < DUPE_PROBE_MIN_ALIVE_SAMPLES
        && definitive_servers > 0
        && definitive_servers >= active_servers
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NzbgetRsServerNormInput {
    pub level: i32,
    pub active: i32,
    pub max_connections: i32,
}

/// Computes normalized levels and sorted server order for `ServerPool::NormalizeLevels`.
/// Writes the sorted original indices into `out_sorted_indices`, each server's normalized
/// level (by original index) into `out_norm_levels`, and returns `max_norm_level`.
pub fn server_pool_normalize_levels(
    servers: &[NzbgetRsServerNormInput],
    out_sorted_indices: &mut [usize],
    out_norm_levels: &mut [i32],
) -> i32 {
    if servers.is_empty() {
        return 0;
    }
    for (i, slot) in out_sorted_indices.iter_mut().enumerate() {
        *slot = i;
    }
    // Stable sort by level preserves insertion order for ties
    out_sorted_indices.sort_by_key(|&i| servers[i].level);

    let min_level = servers[out_sorted_indices[0]].level;
    let mut max_norm_level = 0i32;
    let mut last_level = min_level;
    for &idx in out_sorted_indices.iter() {
        let srv = &servers[idx];
        if (srv.active != 0 && srv.max_connections > 0) || srv.level == min_level {
            if srv.level != last_level {
                max_norm_level += 1;
            }
            out_norm_levels[idx] = max_norm_level;
            last_level = srv.level;
        } else {
            out_norm_levels[idx] = -1;
        }
    }
    max_norm_level
}

/// Checks whether a server is currently blocked (`ServerPool::IsServerBlocked`).
pub fn server_pool_is_blocked(block_time: i64, cur_time: i64, retry_interval: i32) -> bool {
    if block_time == 0 {
        return false;
    }
    block_time <= cur_time && cur_time < block_time.saturating_add(retry_interval as i64)
}

/// Checks whether all open connections on a server pool level should be closed
/// (`ServerPool::CloseUnusedConnections` hold-timeout check).
pub fn server_pool_level_should_close(
    in_use: &[u8],
    connected: &[u8],
    free_times: &[i64],
    cur_time: i64,
    hold_seconds: i32,
) -> bool {
    let mut has_open = false;
    let mut inactive_time: i32 = 0;
    let n = in_use.len().min(connected.len()).min(free_times.len());
    for i in 0..n {
        if in_use[i] != 0 {
            return false;
        }
        if connected[i] != 0 {
            let tdiff = cur_time.wrapping_sub(free_times[i]) as i32;
            if !has_open || tdiff < inactive_time {
                inactive_time = tdiff;
            }
            has_open = true;
        }
    }
    has_open && inactive_time > hold_seconds
}

/// Updates `ArticleFetchLimits::AddRawBytes`.
pub fn article_fetch_add_raw_bytes(raw_bytes: &mut i64, bytes: i64) -> bool {
    if bytes < 0 || bytes > ARTICLE_FETCH_MAX_RAW_BYTES - *raw_bytes {
        return false;
    }
    *raw_bytes += bytes;
    true
}

/// Checks `ArticleFetchLimits::HeaderWithinLimit`.
pub fn article_fetch_header_within_limit(raw_bytes: i64, declared_range_known: bool) -> bool {
    declared_range_known || raw_bytes <= ARTICLE_FETCH_MAX_HEADER_BYTES
}

/// Updates `ArticleFetchLimits::AddDecodedBytes`.
pub fn article_fetch_add_decoded_bytes(
    decoded_bytes: &mut i64,
    bytes: i64,
    begin: i64,
    end: i64,
    file_size: i64,
) -> bool {
    if bytes < 0 || begin <= 0 || end < begin || file_size <= 0 || end > file_size {
        return false;
    }
    let Some(diff) = end.checked_sub(begin) else {
        return false;
    };
    let Some(declared_bytes) = diff.checked_add(1) else {
        return false;
    };
    if declared_bytes > ARTICLE_FETCH_MAX_DECODED_BYTES || bytes > declared_bytes - *decoded_bytes {
        return false;
    }
    *decoded_bytes += bytes;
    true
}

/// Classifies an NNTP command response in `ArticleDownloader::CheckResponse`.
pub fn article_downloader_check_response(response: Option<&[u8]>, auth_error: bool) -> i32 {
    let Some(resp) = response else {
        return AD_CONNECT_ERROR;
    };
    if auth_error || resp.starts_with(b"400") || resp.starts_with(b"499") {
        return AD_CONNECT_ERROR;
    }
    if resp.starts_with(b"41")
        || resp.starts_with(b"42")
        || resp.starts_with(b"43")
        || resp.starts_with(b"451")
    {
        return AD_NOT_FOUND;
    }
    if resp.starts_with(b"2") {
        return AD_FINISHED;
    }
    AD_FAILED
}

/// Validates yEnc decoded range and duplicate-fallback expected bounds in `ArticleDownloader::Write`.
/// Return codes:
/// - `0`: valid (`*out_offset` and `*out_size` populated)
/// - `1`: missing begin or end (`begin_pos == 0 || end_pos == 0`)
/// - `2`: malformed range (`offset < 0 || range_size <= 0 || range_size > 1 GiB || offset + range_size > article_file_size`)
/// - `3`: duplicate file size mismatch
/// - `4`: duplicate decoded offset mismatch
/// - `5`: duplicate decoded end mismatch
pub fn article_downloader_validate_yenc(
    begin_pos: i64,
    end_pos: i64,
    article_file_size: i64,
    expected_file_size: i64,
    fallback_round: i32,
    dupe_expected_offset: i64,
    dupe_expected_end: i64,
) -> Result<(i64, i32), i32> {
    if begin_pos == 0 || end_pos == 0 {
        return Err(1);
    }
    let article_offset = begin_pos.saturating_sub(1);
    let range_size = end_pos.saturating_sub(begin_pos).saturating_add(1);
    if article_offset < 0
        || range_size <= 0
        || range_size > 1024 * 1024 * 1024
        || (article_file_size > 0
            && article_offset.saturating_add(range_size) > article_file_size)
    {
        return Err(2);
    }
    let article_size = range_size as i32;
    if fallback_round > 0 && expected_file_size > 0 && article_file_size != expected_file_size {
        return Err(3);
    }
    if fallback_round > 0 && dupe_expected_offset >= 0 && article_offset != dupe_expected_offset {
        return Err(4);
    }
    if fallback_round > 0
        && dupe_expected_end >= 0
        && article_offset.saturating_add(article_size as i64) != dupe_expected_end
    {
        return Err(5);
    }
    Ok((article_offset, article_size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_par_cannot_cover() {
        // No data failed -> false
        assert!(!par_cannot_cover(100, 0, 0, 1000, 0, 2000, 50, 0, false));
        // Direct data_failed > par_available -> true
        assert!(par_cannot_cover(100, 20, 110, 500, 0, 2000, 10, 2, false));
        // Below ProjectionSample (200) -> false
        assert!(!par_cannot_cover(200, 0, 50, 150, 0, 1200, 150, 49, false));
        // Above ProjectionSample (200): 21% failure rate vs 20% par2 -> true
        assert!(par_cannot_cover(200, 0, 42, 158, 0, 1200, 192, 8, false));
        // Edge margin (50 per mille = 5%) triggers when with_margin = true
        assert!(!par_cannot_cover(200, 0, 35, 165, 0, 1200, 192, 8, false));
        assert!(par_cannot_cover(200, 0, 35, 165, 0, 1200, 192, 8, true));
    }

    #[test]
    fn test_has_extension() {
        assert!(has_extension(b"video.mkv"));
        assert!(has_extension(b"archive.7z.001"));
        assert!(has_extension(b"part.r00"));
        assert!(!has_extension(b".hidden"));
        assert!(!has_extension(b"noext"));
        assert!(!has_extension(b"file.toolong"));
        assert!(!has_extension(b"file.a_b"));
    }

    #[test]
    fn test_declared_parts() {
        assert_eq!(declared_parts(b"release [01/25] - \"vol1.rar\" yEnc (1/30)"), 30);
        assert_eq!(declared_parts(b"(1/10) (5/42)"), 42);
        assert_eq!(declared_parts(b"obfuscated subject without parens"), 0);
        assert_eq!(declared_parts(b"(1/0) (-1/10) (1/10x)"), 0);
    }

    #[test]
    fn test_steps_hash_and_match() {
        let a = [700000, 700120, 699980, 700050, 700010, 699900, 700040, 500000];
        let b = [700050, 700170, 700030, 700100, 700060, 699950, 700090, 500050];
        let flat = [700000, 700000, 700000, 700000, 700000, 700000, 700000, 500000];
        assert_ne!(article_size_steps_hash(&a), 0);
        assert_eq!(article_size_steps_hash(&a), article_size_steps_hash(&b));
        assert!(article_size_steps_match(&a, &b));
        assert_eq!(article_size_steps_hash(&flat), 0);
        assert!(!article_size_steps_match(&flat, &flat));
    }

    #[test]
    fn test_structure_matches_exact_and_gapped() {
        let parts_full = [1, 2, 3, 4];
        let sizes_full = [1000, 1000, 1000, 500];
        assert!(structure_matches(
            b"file (1/4)",
            4,
            3500,
            &parts_full,
            &sizes_full,
            b"donor (1/4)",
            4,
            3500,
            &parts_full,
            &sizes_full,
        ));
        // Target missing 1 interior segment
        let parts_gap = [1, 3, 4];
        let sizes_gap = [1000, 1000, 500];
        assert!(structure_matches(
            b"file (1/4)",
            4,
            3500,
            &parts_gap,
            &sizes_gap,
            b"donor (1/4)",
            4,
            3500,
            &parts_full,
            &sizes_full,
        ));
        // Conflicting declared part counts rejected
        assert!(!structure_matches(
            b"file (1/29)",
            4,
            3500,
            &parts_full,
            &sizes_full,
            b"donor (1/30)",
            4,
            3500,
            &parts_full,
            &sizes_full,
        ));
    }

    #[test]
    fn test_first_untiled_article() {
        let tiled = [
            NzbgetRsArticleGeom {
                part_number: 1,
                status: ARTICLE_STATUS_FINISHED,
                segment_offset: 0,
                segment_size: 100,
                fallback_round: 0,
            },
            NzbgetRsArticleGeom {
                part_number: 2,
                status: ARTICLE_STATUS_FINISHED,
                segment_offset: 100,
                segment_size: 100,
                fallback_round: 1,
            },
        ];
        assert_eq!(first_untiled_article(&tiled, 200), -1);

        // Borrowed article at index 0 decoded short, leaving a gap before own article at index 1
        let short_borrowed = [
            NzbgetRsArticleGeom {
                part_number: 1,
                status: ARTICLE_STATUS_FINISHED,
                segment_offset: 0,
                segment_size: 90,
                fallback_round: 1,
            },
            NzbgetRsArticleGeom {
                part_number: 2,
                status: ARTICLE_STATUS_FINISHED,
                segment_offset: 100,
                segment_size: 100,
                fallback_round: 0,
            },
        ];
        assert_eq!(first_untiled_article(&short_borrowed, 200), 0);
    }

    #[test]
    fn test_blocks_touched_and_project_par_damage() {
        let holes = [
            NzbgetRsByteRange {
                offset: 0,
                size: 100,
            },
            NzbgetRsByteRange {
                offset: 50,
                size: 200,
            },
            NzbgetRsByteRange {
                offset: 1000,
                size: 10,
            },
        ];
        assert_eq!(blocks_touched(&holes, 100), 4);

        let (proj, certain, projected) = project_par_damage(true, 10, 8, 500, 1000);
        assert_eq!(proj, 20);
        assert!(certain);
        assert!(projected);
    }

    #[test]
    fn test_dupe_probe_and_server_pool() {
        assert_eq!(dupe_probe_sample_indexes(100, 4), vec![12, 37, 62, 87]);
        assert_eq!(dupe_probe_classify(Some(b"223 0 <id>")), DUPE_PROBE_EXISTS);
        assert_eq!(
            dupe_probe_classify(Some(b"430 No such article")),
            DUPE_PROBE_MISSING
        );
        assert_eq!(
            dupe_probe_classify(Some(b"400 Service unavailable")),
            DUPE_PROBE_UNKNOWN
        );
        assert!(dupe_probe_is_dead(1, 3, 3));
        assert!(!dupe_probe_is_dead(2, 3, 3));

        let servers = [
            NzbgetRsServerNormInput {
                level: 2,
                active: 1,
                max_connections: 4,
            },
            NzbgetRsServerNormInput {
                level: 0,
                active: 0,
                max_connections: 0,
            },
            NzbgetRsServerNormInput {
                level: 5,
                active: 0,
                max_connections: 4,
            },
        ];
        let mut sorted = [0usize; 3];
        let mut norm = [0i32; 3];
        let max_norm = server_pool_normalize_levels(&servers, &mut sorted, &mut norm);
        assert_eq!(max_norm, 1);
        assert_eq!(norm, [1, 0, -1]);
    }

    #[test]
    fn test_article_fetcher_and_downloader_helpers() {
        let mut raw = 0i64;
        assert!(article_fetch_add_raw_bytes(&mut raw, 1024));
        assert!(article_fetch_header_within_limit(raw, false));
        let mut dec = 0i64;
        assert!(article_fetch_add_decoded_bytes(&mut dec, 100, 1, 100, 1000));
        assert!(!article_fetch_add_decoded_bytes(&mut dec, 1, 1, 100, 1000));

        assert_eq!(
            article_downloader_check_response(Some(b"222 body"), false),
            AD_FINISHED
        );
        assert_eq!(
            article_downloader_check_response(Some(b"451 no such"), false),
            AD_NOT_FOUND
        );
        assert_eq!(
            article_downloader_check_response(Some(b"222 body"), true),
            AD_CONNECT_ERROR
        );

        assert_eq!(
            article_downloader_validate_yenc(101, 200, 1000, 1000, 1, 100, 200),
            Ok((100, 100))
        );
        assert_eq!(
            article_downloader_validate_yenc(101, 200, 1000, 999, 1, 100, 200),
            Err(3)
        );
        assert_eq!(
            article_downloader_validate_yenc(101, 200, 1000, 1000, 1, 90, 200),
            Err(4)
        );
    }
}
