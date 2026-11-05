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

#[derive(Debug, Clone, Default)]
pub struct Par2FileSums {
    pub hash16k: [u8; 32],
    pub name: Vec<u8>,
    pub length: u64,
    pub crcs: Vec<u32>,
    pub set_id: [u8; 32],
    pub block_size: u64,
}

/// Scans all PAR2 files in `dir` and returns their `file_id -> Par2FileSums` map.
pub fn load_par2_sums(dir: &Path) -> BTreeMap<[u8; 32], Par2FileSums> {
    let files: std::cell::RefCell<BTreeMap<[u8; 32], Par2FileSums>> =
        std::cell::RefCell::new(BTreeMap::new());
    let mut set_block_sizes: BTreeMap<[u8; 32], u64> = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return files.into_inner();
    };
    for entry_res in rd {
        let Ok(entry) = entry_res else {
            continue;
        };
        scan_par2_file(
            &entry.path(),
            |set_id, bs| {
                set_block_sizes.insert(*set_id, bs);
            },
            |set_id, file_id, hash16k, length, name| {
                let mut map = files.borrow_mut();
                let sums = map.entry(*file_id).or_default();
                sums.set_id = *set_id;
                sums.hash16k = *hash16k;
                sums.length = length;
                sums.name = name.to_vec();
            },
            |set_id, file_id, crcs| {
                let mut map = files.borrow_mut();
                let sums = map.entry(*file_id).or_default();
                sums.set_id = *set_id;
                sums.crcs = crcs.to_vec();
            },
        );
    }
    let mut files = files.into_inner();
    for sums in files.values_mut() {
        sums.block_size = set_block_sizes.get(&sums.set_id).copied().unwrap_or(0);
    }
    files
}

const CRC32_SLICE8: [[u32; 256]; 8] = {
    let mut table = [[0u32; 256]; 8];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            j += 1;
        }
        table[0][i] = crc;
        i += 1;
    }
    let mut i = 0usize;
    while i < 256 {
        let mut crc = table[0][i];
        let mut k = 1usize;
        while k < 8 {
            crc = table[0][(crc & 0xFF) as usize] ^ (crc >> 8);
            table[k][i] = crc;
            k += 1;
        }
        i += 1;
    }
    table
};

pub fn crc32_ieee(mut bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    while bytes.len() >= 8 {
        let b0 = bytes[0] ^ (crc as u8);
        let b1 = bytes[1] ^ ((crc >> 8) as u8);
        let b2 = bytes[2] ^ ((crc >> 16) as u8);
        let b3 = bytes[3] ^ ((crc >> 24) as u8);
        let b4 = bytes[4];
        let b5 = bytes[5];
        let b6 = bytes[6];
        let b7 = bytes[7];
        crc = CRC32_SLICE8[7][b0 as usize]
            ^ CRC32_SLICE8[6][b1 as usize]
            ^ CRC32_SLICE8[5][b2 as usize]
            ^ CRC32_SLICE8[4][b3 as usize]
            ^ CRC32_SLICE8[3][b4 as usize]
            ^ CRC32_SLICE8[2][b5 as usize]
            ^ CRC32_SLICE8[1][b6 as usize]
            ^ CRC32_SLICE8[0][b7 as usize];
        bytes = &bytes[8..];
    }
    for &b in bytes {
        crc = CRC32_SLICE8[0][((crc as u8) ^ b) as usize] ^ (crc >> 8);
    }
    !crc
}

/// Computes the lower-case 32-hex-digit MD5 of the first 16 KiB of `path`.
pub fn hash16k_of(path: &Path) -> [u8; 32] {
    use md5::{Digest, Md5};
    let mut buf = vec![0u8; 16 * 1024];
    let mut read_len = 0usize;
    if let Ok(mut f) = File::open(path) {
        while read_len < buf.len() {
            match f.read(&mut buf[read_len..]) {
                Ok(0) | Err(_) => break,
                Ok(n) => read_len += n,
            }
        }
    }
    let digest = Md5::digest(&buf[..read_len]);
    hex_lower(&digest)
}

/// `DupeArticleFallback::ListPar2Files`: returns `(name, length, block_size)` for all PAR2 entries in `dir`.
pub fn list_par2_files(dir: &Path) -> Vec<(Vec<u8>, u64, u64)> {
    load_par2_sums(dir)
        .into_values()
        .map(|s| (s.name, s.length, s.block_size))
        .collect()
}

/// `DupeArticleFallback::BorrowedPar2Mismatches`: returns the indices of borrowed finished articles
/// whose completed PAR2 blocks fail CRC32 verification.
#[allow(clippy::too_many_arguments)]
pub fn borrowed_par2_mismatches(
    path: &Path,
    dest_dir: &Path,
    filename: &[u8],
    statuses: &[i32],
    donor_ids: &[i32],
    fallback_rounds: &[i32],
    segment_offsets: &[i64],
    segment_sizes: &[i32],
) -> Vec<i32> {
    borrowed_par2_mismatches_with_crc(
        path,
        dest_dir,
        filename,
        statuses,
        donor_ids,
        fallback_rounds,
        segment_offsets,
        segment_sizes,
        crc32_ieee,
    )
}

/// `DupeArticleFallback::BorrowedPar2Mismatches` with a caller-supplied CRC32 backend.
#[allow(clippy::too_many_arguments)]
pub fn borrowed_par2_mismatches_with_crc<F>(
    path: &Path,
    dest_dir: &Path,
    filename: &[u8],
    statuses: &[i32],
    donor_ids: &[i32],
    fallback_rounds: &[i32],
    segment_offsets: &[i64],
    segment_sizes: &[i32],
    crc_fn: F,
) -> Vec<i32>
where
    F: Fn(&[u8]) -> u32,
{
    let n = statuses
        .len()
        .min(donor_ids.len())
        .min(fallback_rounds.len())
        .min(segment_offsets.len())
        .min(segment_sizes.len());
    let borrowed = |i: usize| donor_ids[i] > 0 || fallback_rounds[i] > 0;
    if !(0..n).any(|i| statuses[i] == ARTICLE_STATUS_FINISHED && borrowed(i)) {
        return Vec::new();
    }
    let files = load_par2_sums(dest_dir);
    if files.is_empty() {
        return Vec::new();
    }
    let file_size = std::fs::metadata(path)
        .map(|m| m.len() as i64)
        .unwrap_or(-1);
    if file_size <= 0 {
        return Vec::new();
    }
    let hash16k = hash16k_of(path);
    let sums = files.values().find(|s| {
        s.length == (file_size as u64)
            && !s.crcs.is_empty()
            && (s.hash16k.eq_ignore_ascii_case(&hash16k) || s.name == filename)
    });
    let Some(sums) = sums else {
        return Vec::new();
    };
    let block_size = sums.block_size;
    if block_size == 0
        || block_size % 4 != 0
        || block_size > MAX_PAR2_BLOCK_SIZE
        || (sums.crcs.len() as u64)
            < (file_size as u64).wrapping_add(block_size - 1) / block_size
    {
        return Vec::new();
    }

    let mut arrived: Vec<(i64, i64)> = (0..n)
        .filter(|&i| statuses[i] == ARTICLE_STATUS_FINISHED && segment_sizes[i] > 0)
        .map(|i| {
            (
                segment_offsets[i],
                segment_offsets[i].wrapping_add(segment_sizes[i] as i64),
            )
        })
        .collect();
    arrived.sort_unstable();
    let arrived_ranges: Vec<NzbgetRsByteRange> = arrived
        .into_iter()
        .map(|(start, end)| NzbgetRsByteRange {
            offset: start,
            size: end.wrapping_sub(start),
        })
        .collect();

    let mut in_file = File::open(path);
    let mut buffer = vec![0u8; block_size as usize];
    let mut verdicts: BTreeMap<i64, bool> = BTreeMap::new();
    let mut mismatches = Vec::new();
    let bs = block_size as i64;

    for i in 0..n {
        if statuses[i] != ARTICLE_STATUS_FINISHED || !borrowed(i) || segment_sizes[i] <= 0 {
            continue;
        }
        let seg_off = segment_offsets[i];
        let seg_sz = segment_sizes[i] as i64;
        if seg_off < 0 || seg_sz <= 0 {
            continue;
        }
        let first = seg_off / bs;
        let last = (seg_off.saturating_add(seg_sz).saturating_sub(1)) / bs;
        let mut bad = false;
        for block in first..=last {
            if bad {
                break;
            }
            let from = block.saturating_mul(bs);
            let to = from.saturating_add(bs).min(file_size);
            if from >= file_size
                || (block as usize) >= sums.crcs.len()
                || !range_covered(&arrived_ranges, from, to)
            {
                continue;
            }
            let ok = *verdicts.entry(block).or_insert_with(|| {
                buffer.fill(0);
                if let Ok(ref mut f) = in_file {
                    if f.seek(SeekFrom::Start(from as u64)).is_ok() {
                        let target_read = (to - from) as usize;
                        let mut read_so_far = 0usize;
                        while read_so_far < target_read {
                            match f.read(&mut buffer[read_so_far..target_read]) {
                                Ok(0) | Err(_) => break,
                                Ok(k) => read_so_far += k,
                            }
                        }
                    }
                }
                crc_fn(&buffer) == sums.crcs[block as usize]
            });
            bad = !ok;
        }
        if bad {
            mismatches.push(i as i32);
        }
    }
    mismatches
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

/// `DupeArticleFallback::IsParFile`:
/// Checks whether a file is a PAR2 file by flag, first-article content (`fcPar2`),
/// or confirmed `.par2` extension (case-insensitive on ASCII bytes).
pub fn is_par_file(
    par_file: bool,
    first_content: i32,
    fc_par2: i32,
    filename_confirmed: bool,
    filename: &[u8],
) -> bool {
    if par_file || first_content == fc_par2 {
        return true;
    }
    filename_confirmed
        && filename.len() >= 5
        && filename[filename.len() - 5..].eq_ignore_ascii_case(b".par2")
}

/// `DupeArticleFallback::HasPar2` / `ShouldDeferToPar`:
/// Checks whether a completed file is a PAR2 file by flag or `.par2` extension.
pub fn completed_file_is_par2(par_file: bool, filename: &[u8]) -> bool {
    par_file
        || (filename.len() >= 5
            && filename[filename.len() - 5..].eq_ignore_ascii_case(b".par2"))
}

/// `DupeArticleFallback::ShouldDeferToPar` header evaluation:
/// - `-1`: do not defer (`false`)
/// - `1`: defer (`true`)
/// - `0`: check file list and completed files for PAR2 entries
pub fn should_defer_to_par_header(
    has_nzb_info: bool,
    direct_rename_status: i32,
    ts_running: i32,
    all_first: bool,
    par_status: i32,
    ps_failure: i32,
    par_size: i64,
) -> i32 {
    if !has_nzb_info {
        return -1;
    }
    if direct_rename_status == ts_running && !all_first {
        return 1;
    }
    if par_status == ps_failure {
        return -1;
    }
    if par_size > 0 {
        return 1;
    }
    0
}

/// `DupeArticleFallback::TryFallback`:
/// Evaluates all entry preconditions (mode, raw article, PAR file, first-article content,
/// and file/NZB deletion/parking/force status). Returns `true` when `TryFallback` may proceed.
#[allow(clippy::too_many_arguments)]
pub fn check_try_preconditions(
    fallback_mode: i32,
    daf_none: i32,
    raw_article: bool,
    par_file: bool,
    first_content: i32,
    fc_par2: i32,
    fc_unknown: i32,
    part_number: i32,
    filename: &[u8],
    file_deleted: bool,
    nzb_deleting: bool,
    nzb_parking: bool,
    delete_status: i32,
    ds_none: i32,
    dupe_mode: i32,
    dm_force: i32,
) -> bool {
    if fallback_mode == daf_none || raw_article || par_file {
        return false;
    }
    if first_content == fc_par2
        || (first_content == fc_unknown && part_number != 1 && !has_extension(filename))
    {
        return false;
    }
    if file_deleted
        || nzb_deleting
        || nzb_parking
        || delete_status != ds_none
        || dupe_mode == dm_force
    {
        return false;
    }
    true
}

/// `DupeArticleFallback::TryFallback`:
/// Evaluates whether duplicate article fallback should defer to par-check and
/// whether defer state transitions to `dpLifted`.
/// Returns `(defer, lift_defer_state, log_lifted)`.
#[allow(clippy::too_many_arguments)]
pub fn eval_par_defer(
    par_defer: bool,
    part_number: i32,
    dupe_par_defer_state: i32,
    dp_deferred: i32,
    dp_lifted: i32,
    par_cannot_cover_flag: bool,
    damage_certain: bool,
    damage_projected: bool,
) -> (bool, bool, bool) {
    let defer = par_defer
        && part_number != 1
        && dupe_par_defer_state != dp_lifted
        && !par_cannot_cover_flag
        && !damage_certain
        && !damage_projected;
    let lift_defer_state = par_defer && !defer && part_number != 1;
    let log_lifted = lift_defer_state && dupe_par_defer_state == dp_deferred;
    (defer, lift_defer_state, log_lifted)
}

/// `DupeArticleFallback::TryFallback`:
/// Checks whether deferred fallback must abort before pinning or advancing sources.
pub fn should_abort_deferred_try(
    defer: bool,
    round: i32,
    has_download_queue: bool,
    source_donors_empty: bool,
) -> bool {
    defer && ((round == 0 && !has_download_queue) || (round > 0 && source_donors_empty))
}

/// `DupeArticleFallback::TryFallback`:
/// Evaluates actions when `round >= sources.len()` (all candidate sources exhausted):
/// Returns `(check_log_par_defer, inc_unsourced, restore_orig_msg_id)`.
pub fn resolve_exhausted_sources(
    defer: bool,
    pinned_round: i32,
    dupe_proactive: bool,
    par_defer_state: i32,
    dp_none: i32,
    has_download_queue: bool,
    orig_msg_id_empty: bool,
) -> (bool, bool, bool) {
    if defer {
        let check_log_par_defer =
            !dupe_proactive && par_defer_state == dp_none && has_download_queue;
        let restore_orig_msg_id = pinned_round > 0 && !orig_msg_id_empty;
        (check_log_par_defer, false, restore_orig_msg_id)
    } else if pinned_round == 0 && !dupe_proactive {
        (false, true, false)
    } else {
        (false, false, !orig_msg_id_empty)
    }
}

/// `DupeArticleFallback::RotateToLead`:
/// Returns the rotation index (`1..donor_ids.len()`) when `lead_nzb_id` is found after index 0,
/// or `0` when no rotation should occur.
pub fn find_rotate_offset(donor_ids: &[i32], lead_nzb_id: i32) -> usize {
    if lead_nzb_id == 0 {
        return 0;
    }
    match donor_ids.iter().position(|&id| id == lead_nzb_id) {
        Some(idx) if idx > 0 && idx < donor_ids.len() => idx,
        _ => 0,
    }
}

/// `DupeArticleFallback::FinishPin`:
/// Computes `(lead_snapshot, next_lead, donor_count, new_file_lead_donor_id)` from `contributors`.
pub fn compute_pin_lead_state(
    contributors: &[i32],
    cur_file_lead_donor_id: i32,
) -> (i32, i32, i32, i32) {
    let mut donor_ids: Vec<i32> = Vec::new();
    for &id in contributors {
        if !donor_ids.contains(&id) {
            donor_ids.push(id);
        }
    }
    let lead_snapshot = donor_ids.first().copied().unwrap_or(0);
    let next_lead = donor_ids.get(1).copied().unwrap_or(0);
    let donor_count = donor_ids.len() as i32;
    let new_file_lead = if cur_file_lead_donor_id == 0 && !donor_ids.is_empty() {
        donor_ids[0]
    } else {
        cur_file_lead_donor_id
    };
    (lead_snapshot, next_lead, donor_count, new_file_lead)
}

/// `DupeArticleFallback::VacateGhostLead`:
/// Returns `true` when the file's current lead donor is barred or absent from `donor_ids`.
pub fn should_vacate_ghost_lead(lead_nzb_id: i32, is_barred: bool, donor_ids: &[i32]) -> bool {
    if lead_nzb_id == 0 {
        return false;
    }
    if is_barred {
        return true;
    }
    !donor_ids.contains(&lead_nzb_id)
}

/// `DupeArticleFallback::NzbFilenameOf`:
/// Checks whether `own_article` matches `article` by part number and original (or current) message-id.
pub fn own_article_matches(
    own_part_number: i32,
    part_number: i32,
    own_msg_id: &[u8],
    orig_msg_id: &[u8],
    cur_msg_id: &[u8],
) -> bool {
    let msg_id = if !orig_msg_id.is_empty() {
        orig_msg_id
    } else {
        cur_msg_id
    };
    own_part_number == part_number && !msg_id.is_empty() && own_msg_id == msg_id
}

/// `DupeArticleFallback::CollectDonors`:
/// Checks whether a queued or history item is eligible as a donor before comparing names/dupe-keys.
pub fn is_donor_eligible(
    is_self: bool,
    kind: i32,
    expected_kind: i32,
    dupe_mode: i32,
    dm_force: i32,
) -> bool {
    !is_self && kind == expected_kind && dupe_mode != dm_force
}

/// `DupeArticleFallback::CollectDonors` / `MatchDonorFile`:
/// Checks whether both identities are non-empty and `kind` starts with `"twin"`.
pub fn is_twin_kind(own_identity: &[u8], donor_identity: &[u8], kind: &[u8]) -> bool {
    !own_identity.is_empty() && !donor_identity.is_empty() && kind.starts_with(b"twin")
}

/// `DupeArticleFallback::CollectDonors`:
/// Comparator ordering twin donors first, then higher `dupe_score`, then lower `id`.
pub fn compare_donors(
    twin1: bool,
    score1: i32,
    id1: i32,
    twin2: bool,
    score2: i32,
    id2: i32,
) -> bool {
    if twin1 != twin2 {
        return twin1;
    }
    if score1 != score2 {
        return score1 > score2;
    }
    id1 < id2
}

/// `DupeArticleFallback::PinSources`:
/// Checks whether a non-twin lead donor should be reset when `twin_only` is active.
pub fn should_reset_non_twin_lead(
    twin_only: bool,
    lead_id: i32,
    own_identity: &[u8],
    lead_identity: &[u8],
    kind: &[u8],
) -> bool {
    twin_only && lead_id != 0 && !is_twin_kind(own_identity, lead_identity, kind)
}

/// `DupeArticleFallback::GetParsedDonor`:
/// Checks whether a donor whose NZB failed to parse is still within its retry cooldown window.
pub fn is_bad_donor_in_cooldown(now_sec: i64, bad_since_sec: i64, retry_sec: i64) -> bool {
    now_sec.wrapping_sub(bad_since_sec) < retry_sec
}

/// `DupeArticleFallback::MatchDonorFile`:
/// Classifies how `donor_filename` matches `by_content`:
/// `1` (exact byte match), `2` (case-insensitive match), or `0` (no match).
pub fn classify_content_name_match(donor_filename: &[u8], by_content: &[u8]) -> i32 {
    if donor_filename == by_content {
        1
    } else if crate::queuehotpath::strcasecmp_bytes(donor_filename, by_content) {
        2
    } else {
        0
    }
}

/// `DupeArticleFallback::MatchDonorFile`:
/// Selects between exact-case and case-folded content-matched donor files:
/// `1` when `exact_count == 1`, `2` when `exact_count == 0 && folded_count == 1`, `0` otherwise.
pub fn select_named_content_match(exact_count: usize, folded_count: usize) -> i32 {
    if exact_count == 1 {
        1
    } else if exact_count == 0 && folded_count == 1 {
        2
    } else {
        0
    }
}

/// `DupeArticleFallback::MatchDonorFile`:
/// Checks whether a donor file's name matches either the target's current filename
/// or its original NZB filename case-insensitively.
pub fn is_donor_filename_match(
    target_filename: &[u8],
    target_nzb_filename: &[u8],
    donor_filename: &[u8],
) -> bool {
    crate::queuehotpath::strcasecmp_bytes(target_filename, donor_filename)
        || (!target_nzb_filename.is_empty()
            && crate::queuehotpath::strcasecmp_bytes(target_nzb_filename, donor_filename))
}

/// `DupeArticleFallback::MatchDonorFile`:
/// Resolves structural / filename match:
/// `-1` to abort (`nullptr`), `1` to return `filenameMatch`, `2` to return `structuralMatches[0]`,
/// or `0` to continue to step-size matching.
pub fn resolve_structural_or_filename_match(
    filename_ambiguous: bool,
    has_filename_match: bool,
    structural_matches_len: usize,
) -> i32 {
    if filename_ambiguous {
        -1
    } else if has_filename_match {
        1
    } else if structural_matches_len == 1 {
        2
    } else {
        0
    }
}

/// `DupeArticleFallback::FirstContent`:
/// Classifies the first 8 bytes of a file's first article on disk:
/// - `0`: all zeroes (not flushed from article cache yet -> `fcUnknown`)
/// - `1`: `"PAR2\0PKT"` (`fcPar2`)
/// - `2`: any other non-zero header (`fcOther`)
pub fn classify_first_head(head8: &[u8]) -> i32 {
    if head8.len() < 8 || head8[..8].iter().all(|&b| b == 0) {
        0
    } else if &head8[..8] == b"PAR2\0PKT" {
        1
    } else {
        2
    }
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

    #[test]
    fn test_consolidated_dupe_fallback_domain_functions() {
        assert!(is_par_file(false, 0, 2, true, b"caf\xe9.PAR2"));
        assert!(!is_par_file(false, 0, 2, false, b"caf\xe9.PAR2"));
        assert!(completed_file_is_par2(false, b"vol01.PAR2"));
        assert!(!completed_file_is_par2(false, b"vol01.rar"));

        assert_eq!(should_defer_to_par_header(false, 0, 1, false, 0, 2, 100), -1);
        assert_eq!(should_defer_to_par_header(true, 1, 1, false, 0, 2, 0), 1);
        assert_eq!(should_defer_to_par_header(true, 0, 1, false, 2, 2, 100), -1);
        assert_eq!(should_defer_to_par_header(true, 0, 1, false, 0, 2, 100), 1);
        assert_eq!(should_defer_to_par_header(true, 0, 1, false, 0, 2, 0), 0);

        assert!(check_try_preconditions(
            1, 0, false, false, 0, 2, 0, 2, b"part01.rar", false, false, false, 0, 0, 0, 2
        ));
        assert!(!check_try_preconditions(
            1, 0, false, false, 0, 2, 0, 2, b"obfuscated", false, false, false, 0, 0, 0, 2
        ));
        assert!(check_try_preconditions(
            1, 0, false, false, 0, 2, 0, 1, b"obfuscated", false, false, false, 0, 0, 0, 2
        ));

        assert_eq!(
            eval_par_defer(true, 2, 1, 1, 2, false, false, false),
            (true, false, false)
        );
        assert_eq!(
            eval_par_defer(true, 2, 1, 1, 2, true, false, false),
            (false, true, true)
        );

        assert!(should_abort_deferred_try(true, 0, false, false));
        assert!(should_abort_deferred_try(true, 1, true, true));
        assert!(!should_abort_deferred_try(true, 1, true, false));

        assert_eq!(
            resolve_exhausted_sources(true, 0, false, 0, 0, true, false),
            (true, false, false)
        );
        assert_eq!(
            resolve_exhausted_sources(false, 0, false, 0, 0, true, false),
            (false, true, false)
        );
        assert_eq!(
            resolve_exhausted_sources(false, 1, false, 0, 0, true, false),
            (false, false, true)
        );

        assert_eq!(find_rotate_offset(&[10, 20, 30], 20), 1);
        assert_eq!(find_rotate_offset(&[10, 20, 30], 10), 0);
        assert_eq!(find_rotate_offset(&[10, 20, 30], 0), 0);

        assert_eq!(
            compute_pin_lead_state(&[11, 11, 22, 33], 0),
            (11, 22, 3, 11)
        );
        assert_eq!(compute_pin_lead_state(&[11], 99), (11, 0, 1, 99));

        assert!(should_vacate_ghost_lead(10, true, &[10, 20]));
        assert!(should_vacate_ghost_lead(30, false, &[10, 20]));
        assert!(!should_vacate_ghost_lead(10, false, &[10, 20]));

        assert!(own_article_matches(3, 3, b"<orig@id>", b"<orig@id>", b"<sub@id>"));
        assert!(!own_article_matches(3, 3, b"<orig@id>", b"", b"<sub@id>"));

        assert!(is_donor_eligible(false, 1, 1, 0, 2));
        assert!(!is_donor_eligible(true, 1, 1, 0, 2));
        assert!(is_twin_kind(b"fp1", b"fp2", b"twin 16/18"));
        assert!(!is_twin_kind(b"fp1", b"fp2", b"alt"));
        assert!(compare_donors(true, 10, 5, false, 100, 1));
        assert!(should_reset_non_twin_lead(true, 3, b"fp1", b"fp2", b"alt"));
        assert!(is_bad_donor_in_cooldown(105, 100, 30));
        assert!(!is_bad_donor_in_cooldown(130, 100, 30));

        assert_eq!(classify_content_name_match(b"Movie.rar", b"Movie.rar"), 1);
        assert_eq!(classify_content_name_match(b"Movie.rar", b"movie.rar"), 2);
        assert_eq!(select_named_content_match(0, 1), 2);
        assert!(is_donor_filename_match(
            b"obfuscated.bin",
            b"Movie.part01.rar",
            b"movie.part01.rar"
        ));
        assert_eq!(resolve_structural_or_filename_match(false, true, 2), 1);
        assert_eq!(classify_first_head(b"PAR2\0PKT"), 1);
    }
}
