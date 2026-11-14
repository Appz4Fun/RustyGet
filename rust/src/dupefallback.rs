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
pub const AD_TRANSIENT_ERROR: i32 = 9;

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
        || (sums.crcs.len() as u64) < (file_size as u64).wrapping_add(block_size - 1) / block_size
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

fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

/// Classifies a non-null NNTP response line in `ArticleDownloader::ClassifyResponse`.
pub fn article_downloader_classify_response(resp: &[u8]) -> i32 {
    if resp.starts_with(b"2") {
        return AD_FINISHED;
    }
    if resp.starts_with(b"400") || resp.starts_with(b"499") {
        return AD_CONNECT_ERROR;
    }
    if resp.starts_with(b"480")
        || resp.starts_with(b"481")
        || resp.starts_with(b"482")
        || resp.starts_with(b"502")
        || resp.starts_with(b"503")
        || contains_ascii_case_insensitive(resp, b"connection unavailable")
        || contains_ascii_case_insensitive(resp, b"pool exhausted")
    {
        return AD_TRANSIENT_ERROR;
    }
    if resp.starts_with(b"41")
        || resp.starts_with(b"42")
        || resp.starts_with(b"43")
        || resp.starts_with(b"451")
    {
        return AD_NOT_FOUND;
    }
    AD_FAILED
}

/// Classifies an NNTP command response in `ArticleDownloader::CheckResponse`.
pub fn article_downloader_check_response(response: Option<&[u8]>, auth_error: bool) -> i32 {
    let Some(resp) = response else {
        return AD_CONNECT_ERROR;
    };
    if auth_error {
        return AD_CONNECT_ERROR;
    }
    article_downloader_classify_response(resp)
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
        || (article_file_size > 0 && article_offset.saturating_add(range_size) > article_file_size)
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
        || (filename.len() >= 5 && filename[filename.len() - 5..].eq_ignore_ascii_case(b".par2"))
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
    } else if crate::queuecoord::strcasecmp_bytes(donor_filename, by_content) {
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
    crate::queuecoord::strcasecmp_bytes(target_filename, donor_filename)
        || (!target_nzb_filename.is_empty()
            && crate::queuecoord::strcasecmp_bytes(target_nzb_filename, donor_filename))
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

/// Pure-Rust descriptor for a target or donor file in `match_donor_file`.
#[derive(Debug, Clone)]
pub struct DonorFileSpec<'a> {
    pub filename: &'a [u8],
    pub subject: &'a [u8],
    pub total_articles: i32,
    pub size: i64,
    pub is_par_file: bool,
    pub parts: &'a [i32],
    pub sizes: &'a [i32],
}

/// `DupeArticleFallback::MatchDonorFile`:
/// Finds the index of the donor file in `donor_files` corresponding to `target`:
/// 1. Rejects PAR2 targets or `twin_only` when `!twin_allowed`.
/// 2. Uses `by_content` (from PAR2 `FileDesc` content matching) when available.
/// 3. Falls back to structural + case-insensitive filename/NZB-filename matching,
///    and consecutive article size step-fingerprint tie-breaking.
/// Returns `None` if no candidate or multiple ambiguous candidates match.
pub fn match_donor_file(
    target: &DonorFileSpec<'_>,
    target_nzb_filename: &[u8],
    by_content: &[u8],
    twin_allowed: bool,
    twin_only: bool,
    donor_files: &[DonorFileSpec<'_>],
) -> Option<usize> {
    if target.is_par_file {
        return None;
    }
    if twin_only && !twin_allowed {
        return None;
    }
    if by_content == b"-" || (twin_only && by_content.is_empty()) {
        return None;
    }
    let check_structure = |df: &DonorFileSpec<'_>| -> bool {
        structure_matches(
            target.subject,
            target.total_articles,
            target.size,
            target.parts,
            target.sizes,
            df.subject,
            df.total_articles,
            df.size,
            df.parts,
            df.sizes,
        )
    };
    if !by_content.is_empty() {
        let mut exact = Vec::new();
        let mut folded = Vec::new();
        for (i, df) in donor_files.iter().enumerate() {
            if df.is_par_file {
                continue;
            }
            match classify_content_name_match(df.filename, by_content) {
                1 => exact.push(i),
                2 => folded.push(i),
                _ => {}
            }
        }
        let match_kind = select_named_content_match(exact.len(), folded.len());
        let named = match match_kind {
            1 => Some(exact[0]),
            2 => Some(folded[0]),
            _ => None,
        };
        if named.is_some() || twin_only {
            return named.filter(|&idx| check_structure(&donor_files[idx]));
        }
    }

    let mut structural_matches = Vec::new();
    let mut filename_match = None;
    let mut filename_ambiguous = false;

    for (i, df) in donor_files.iter().enumerate() {
        if df.is_par_file || !check_structure(df) {
            continue;
        }
        if is_donor_filename_match(target.filename, target_nzb_filename, df.filename) {
            if filename_match.is_some() {
                filename_ambiguous = true;
            } else {
                filename_match = Some(i);
            }
            continue;
        }
        structural_matches.push(i);
    }

    match resolve_structural_or_filename_match(
        filename_ambiguous,
        filename_match.is_some(),
        structural_matches.len(),
    ) {
        -1 => return None,
        1 => return filename_match,
        2 => return Some(structural_matches[0]),
        _ => {}
    }

    let mut step_match = None;
    for &idx in &structural_matches {
        if article_size_steps_match(target.sizes, donor_files[idx].sizes) {
            if step_match.is_some() {
                return None;
            }
            step_match = Some(idx);
        }
    }
    step_match
}

/// `DupeArticleFallback::FindDonorMessageId`:
/// Returns the first non-empty message-id for `part_number` in `articles`.
pub fn find_donor_message_id<'a>(
    articles: &'a [(i32, &'a [u8])],
    part_number: i32,
) -> Option<&'a [u8]> {
    articles
        .iter()
        .find(|&&(part, msg_id)| part == part_number && !msg_id.is_empty())
        .map(|&(_, msg_id)| msg_id)
}

/// `DupeArticleFallback::OrderSources`:
/// Orders the message-ids an article is tried against. Reactive (`!cutover`): donor candidates only.
/// Cut over (`cutover`): `[donor_candidates..., primary_message_id]` when `donor_candidates` is non-empty.
pub fn order_sources(
    donor_candidates: &[Vec<u8>],
    cutover: bool,
    primary_message_id: &[u8],
) -> Vec<Vec<u8>> {
    let append_primary = cutover && !donor_candidates.is_empty();
    let mut sources = Vec::with_capacity(donor_candidates.len() + usize::from(append_primary));
    sources.extend_from_slice(donor_candidates);
    if append_primary {
        sources.push(primary_message_id.to_vec());
    }
    sources
}

/// `DupeArticleFallback::RotateToLead`:
/// Rotates `donors` in-place so the donor whose ID equals `lead_nzb_id` comes first.
pub fn rotate_to_lead<T, F>(donors: &mut [T], lead_nzb_id: i32, id_fn: F)
where
    F: Fn(&T) -> i32,
{
    let donor_ids: Vec<i32> = donors.iter().map(id_fn).collect();
    let idx = find_rotate_offset(&donor_ids, lead_nzb_id);
    if idx > 0 && idx < donors.len() {
        donors.rotate_left(idx);
    }
}

pub(crate) const CUTOVER_THRESHOLD: i32 = 3;
const MAX_CACHED_DONORS: usize = 16;
const BAD_DONOR_RETRY_SEC: i64 = 600;

#[cfg(unix)]
#[derive(Default)]
pub(crate) struct DonorCacheState {
    parsed_donors: BTreeMap<i32, std::sync::Arc<crate::articlefetcher::DonorParsedNzb>>,
    bad_donors: BTreeMap<i32, i64>,
    file_parts_sizes: Option<(i32, i32, usize, Vec<i32>, Vec<i32>)>,
}

#[cfg(unix)]
impl DonorCacheState {
    pub(crate) fn get_parsed_donor(
        &mut self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        donor: &crate::queuesubsys_host::NzbSnap,
        opts: &crate::queuesubsys_host::GlobalOptsSnap,
    ) -> Option<std::sync::Arc<crate::articlefetcher::DonorParsedNzb>> {
        if let Some(&bad_ts) = self.bad_donors.get(&donor.id) {
            if opts.cur_time - bad_ts < BAD_DONOR_RETRY_SEC {
                return None;
            }
        }
        if let Some(cached) = self.parsed_donors.get(&donor.id) {
            return Some(std::sync::Arc::clone(cached));
        }
        let parsed = crate::articlefetcher::parse_donor_nzb_file(
            &donor.queued_filename,
            &opts.par_ignore_ext,
            |msg| host.log(crate::queuesubsys_host::MK_DETAIL, msg),
        );
        let Some(nzb) = parsed else {
            self.bad_donors.insert(donor.id, opts.cur_time);
            return None;
        };
        if self.parsed_donors.len() >= MAX_CACHED_DONORS {
            self.parsed_donors.pop_first();
        }
        let arc = std::sync::Arc::new(nzb);
        self.parsed_donors
            .insert(donor.id, std::sync::Arc::clone(&arc));
        Some(arc)
    }
}

#[cfg(unix)]
pub(crate) fn file_is_par_snap(f: &crate::queuesubsys_host::FileSnap) -> bool {
    is_par_file(
        f.par_file,
        f.first_content,
        crate::queuesubsys_host::FC_PAR2,
        f.filename_confirmed,
        &f.filename,
    )
}

#[cfg(unix)]
pub(crate) fn completed_file_is_par2_snap(cf: &crate::queuesubsys_host::CompletedFileSnap) -> bool {
    is_par_file(cf.par_file, 0, 1, true, &cf.filename)
}

#[cfg(unix)]
pub(crate) fn has_par2_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
) -> bool {
    if nzb.par_size > 0 {
        return true;
    }
    let flen = host.nzb_file_len(nzb.ptr);
    for i in 0..flen {
        let f = host.file_snap(host.nzb_file_at(nzb.ptr, i));
        if file_is_par_snap(&f) {
            return true;
        }
    }
    for cf in host.nzb_completed_files(nzb.ptr) {
        if completed_file_is_par2_snap(&cf) {
            return true;
        }
    }
    false
}

#[cfg(unix)]
pub(crate) fn should_defer_to_par_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: usize,
) -> bool {
    if nzb_ptr == 0 {
        return false;
    }
    let nzb = host.nzb_snap(nzb_ptr, false);
    if crate::options::dupe_borrow_first() {
        // Live donor recovery runs alongside the par2 set; it only waits while direct rename
        // still identifies obfuscated files.
        return nzb.direct_rename_status == crate::queuesubsys_host::TS_RUNNING && !nzb.all_first;
    }
    should_defer_to_par_snap(host, &nzb)
}

#[cfg(unix)]
pub(crate) fn should_defer_to_par_snap(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
) -> bool {
    if nzb.ptr == 0 {
        return false;
    }
    if nzb.direct_rename_status == crate::queuesubsys_host::TS_RUNNING && !nzb.all_first {
        return true;
    }
    if nzb.par_status == crate::queuesubsys_host::PS_FAILURE {
        return false;
    }
    if nzb.par_size > 0 {
        return true;
    }
    let flen = host.nzb_file_len(nzb.ptr);
    for i in 0..flen {
        let f = host.file_snap(host.nzb_file_at(nzb.ptr, i));
        if !f.deleted && file_is_par_snap(&f) {
            return true;
        }
    }
    for cf in host.nzb_completed_files(nzb.ptr) {
        if completed_file_is_par2_snap(&cf) {
            return true;
        }
    }
    false
}

#[cfg(unix)]
pub(crate) fn par_cannot_cover_snap(
    nzb: &crate::queuesubsys_host::NzbSnap,
    with_margin: bool,
) -> bool {
    par_cannot_cover(
        nzb.par_size,
        nzb.par_current_failed_size,
        nzb.current_failed_size,
        nzb.current_success_size,
        nzb.par_current_success_size,
        nzb.size,
        nzb.current_success_articles,
        nzb.current_failed_articles,
        with_margin,
    )
}

#[cfg(unix)]
pub(crate) fn article_size_steps_hash_slice(
    articles: &[crate::queuesubsys_host::ArticleSnap],
) -> u64 {
    if articles.len() < MIN_STEP_FINGERPRINT_ARTICLES {
        return 0;
    }
    let sizes: Vec<i32> = articles.iter().map(|a| a.size).collect();
    article_size_steps_hash(&sizes)
}

#[cfg(unix)]
pub(crate) fn compute_holes_slice(
    articles: &[crate::queuesubsys_host::ArticleSnap],
    file_size: i64,
) -> Vec<NzbgetRsByteRange> {
    if file_size <= 0 {
        return Vec::new();
    }
    let mut covered = Vec::with_capacity(articles.len());
    for a in articles {
        if a.status == crate::queuesubsys_host::AI_FINISHED
            && a.segment_size > 0
            && a.segment_offset >= 0
        {
            covered.push(NzbgetRsByteRange {
                offset: a.segment_offset,
                size: i64::from(a.segment_size),
            });
        }
    }
    crate::streamrepair::compute_holes(&covered, file_size)
}

#[cfg(unix)]
pub(crate) fn build_repair_job_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    nzb: &crate::queuesubsys_host::NzbSnap,
    file: &crate::queuesubsys_host::FileSnap,
    articles: &[crate::queuesubsys_host::ArticleSnap],
    disk_basename: &[u8],
) -> bool {
    let ends_with_par2 = disk_basename.len() >= 5
        && disk_basename[disk_basename.len() - 5..].eq_ignore_ascii_case(b".par2");
    if opts.dupe_article_fallback < crate::queuesubsys_host::DAF_STREAM
        || opts.raw_article
        || file_is_par_snap(file)
        || ends_with_par2
        || disk_basename.is_empty()
        || articles.is_empty()
        || nzb.ptr == 0
        || nzb.has_post_info
    {
        return false;
    }
    let health_cancel = nzb.delete_status == crate::queuesubsys_host::DS_HEALTH;
    if !health_cancel
        && (nzb.deleting || nzb.parking || nzb.delete_status != crate::queuesubsys_host::DS_NONE)
    {
        return false;
    }

    if file.success_articles == 0 {
        if file.size <= 0 {
            return false;
        }
        host.nzb_push_stream_repair_job(
            nzb.ptr,
            file.id,
            disk_basename,
            0,
            file.failed_size,
            file.missed_size,
            file.failed_articles + file.missed_articles,
            file.par_file,
            &[],
            article_size_steps_hash_slice(articles),
        );
        return true;
    }
    if file.decoded_file_size <= 0 {
        return false;
    }

    let holes = compute_holes_slice(articles, file.decoded_file_size);
    if holes.is_empty() {
        return false;
    }

    host.nzb_push_stream_repair_job(
        nzb.ptr,
        file.id,
        disk_basename,
        file.decoded_file_size,
        file.failed_size,
        file.missed_size,
        file.failed_articles + file.missed_articles,
        file.par_file,
        &holes,
        article_size_steps_hash_slice(articles),
    );
    true
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ParDamageVerdict {
    pub known: bool,
    pub block_size: u64,
    pub damaged_blocks: i32,
    pub recovery_blocks: i32,
    pub done_bytes: i64,
    pub total_bytes: i64,
    pub projected_blocks: i32,
}

#[cfg(unix)]
impl ParDamageVerdict {
    pub fn certain(&self) -> bool {
        self.known && self.damaged_blocks > self.recovery_blocks
    }
    pub fn projected(&self) -> bool {
        self.known
            && self.done_bytes.saturating_mul(5) >= self.total_bytes
            && i64::from(self.projected_blocks) * 4 > i64::from(self.recovery_blocks) * 5 + 8
    }
}

#[cfg(unix)]
pub(crate) fn par_damage_file_completed_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
    file: &crate::queuesubsys_host::FileSnap,
    articles: &[crate::queuesubsys_host::ArticleSnap],
    filename: &[u8],
) {
    let is_par = file.par_file || file_is_par_snap(file);
    let failed_plus_missed = file.failed_articles + file.missed_articles;
    let decoded_size = file.decoded_file_size;
    let holes = if !is_par && failed_plus_missed > 0 && decoded_size > 0 {
        compute_holes_slice(articles, decoded_size)
    } else {
        Vec::new()
    };
    crate::pardamage::file_completed(
        nzb.id,
        is_par,
        failed_plus_missed,
        decoded_size,
        file.success_articles,
        file.size,
        filename,
        &holes,
    );

    if !is_par
        && !filename.is_empty()
        && decoded_size > 0
        && crate::parchecker::streaming_session_has_session(nzb.id)
    {
        // The file is complete on disk now: let the live par2 session verify the slices it
        // has not seen in memory (articles fetched before the session started, slices that
        // left the article cache) and mark the holes as damaged, so it knows exactly how
        // many recovery blocks the collection needs.
        for a in articles {
            if a.status == crate::queuesubsys_host::AI_FINISHED
                && a.segment_size > 0
                && a.segment_offset >= 0
            {
                let _ = crate::parchecker::streaming_session_on_article_finished_by_name(
                    nzb.id,
                    filename,
                    a.segment_offset as u64,
                    a.segment_size as u64,
                    a.crc,
                    None,
                );
            }
        }
        for h in &holes {
            if h.size > 0 && h.offset >= 0 {
                let _ = crate::parchecker::streaming_session_on_article_failed_permanently_by_name(
                    nzb.id,
                    filename,
                    h.offset as u64,
                    h.size as u64,
                );
            }
        }
    }

    if is_par && failed_plus_missed == 0 && !filename.is_empty() {
        // `filename` is relative to the download's directory.
        let par_full = [
            nzb.dest_dir.as_slice(),
            &[crate::paths::PATH_SEPARATOR],
            filename,
        ]
        .concat();
        if !crate::parchecker::streaming_session_has_session(nzb.id) {
            if crate::parchecker::streaming_session_start(
                nzb.id,
                crate::fsops::path(&par_full),
                crate::fsops::path(&nzb.dest_dir),
                crate::parchecker::SessionOptions::default(),
            )
            .is_ok()
            {
                let flen = host.nzb_file_len(nzb.ptr);
                for i in 0..flen {
                    let qf_ptr = host.nzb_file_at(nzb.ptr, i);
                    if qf_ptr == 0 || qf_ptr == file.ptr {
                        continue;
                    }
                    let qf = host.file_snap(qf_ptr);
                    if qf.deleted {
                        continue;
                    }
                    if let Some((_, blocks)) =
                        crate::parparser::parse_par_filename(&qf.filename, qf.filename_confirmed)
                    {
                        if blocks != 0
                            && crate::parparser::same_par_collection(
                                &file.filename,
                                &qf.filename,
                                file.filename_confirmed && qf.filename_confirmed,
                            )
                        {
                            crate::parchecker::streaming_session_register_queued_volume(
                                nzb.id,
                                qf.id as u64,
                                if blocks > 0 { blocks } else { 1 },
                                qf.paused,
                            );
                        }
                    }
                }
            }
        } else {
            let _ = crate::parchecker::streaming_session_on_recovery_volume_arrived(
                nzb.id,
                crate::fsops::path(&par_full),
                Some(file.id as u64),
            );
        }
    }
}

#[cfg(unix)]
pub(crate) fn par_damage_judge_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
) -> ParDamageVerdict {
    if !crate::pardamage::has_damage(nzb.id) {
        return ParDamageVerdict::default();
    }
    let flen = host.nzb_file_len(nzb.ptr);
    let mut q_snaps = Vec::with_capacity(flen);
    for i in 0..flen {
        q_snaps.push(host.file_snap(host.nzb_file_at(nzb.ptr, i)));
    }
    let q_candidates: Vec<crate::pardamage::QueuedParCandidate<'_>> = q_snaps
        .iter()
        .map(|f| crate::pardamage::QueuedParCandidate {
            filename: &f.filename,
            confirmed: f.filename_confirmed,
            size: f.size,
        })
        .collect();
    let c_snaps = host.nzb_completed_files(nzb.ptr);
    let c_candidates: Vec<crate::pardamage::CompletedParCandidate<'_>> = c_snaps
        .iter()
        .map(|c| crate::pardamage::CompletedParCandidate {
            filename: &c.filename,
            not_failed: c.status != crate::queuesubsys_host::CF_FAILURE,
        })
        .collect();
    let raw = crate::pardamage::judge(
        nzb.id,
        crate::fsops::path(&nzb.dest_dir),
        nzb.size,
        nzb.par_size,
        &q_candidates,
        &c_candidates,
    );
    ParDamageVerdict {
        known: raw.known != 0,
        block_size: raw.block_size,
        damaged_blocks: raw.damaged_blocks,
        recovery_blocks: raw.recovery_blocks,
        done_bytes: raw.done_bytes,
        total_bytes: raw.total_bytes,
        projected_blocks: raw.projected_blocks,
    }
}

/// Live par2: as soon as a completed file is known to be damaged, the recovery volumes that
/// cover the damage seen so far are unpaused, so they download next to the rest of the
/// collection instead of after it. Returns the number of volumes unpaused.
#[cfg(unix)]
pub(crate) fn unpause_pars_for_damage_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
) -> usize {
    let verdict = par_damage_judge_host(host, nzb);
    if !verdict.known || verdict.damaged_blocks <= 0 || verdict.block_size == 0 {
        return 0;
    }
    let mut have: i64 = 0;
    let mut paused: Vec<(usize, i32, Vec<u8>)> = Vec::new();
    let flen = host.nzb_file_len(nzb.ptr);
    for i in 0..flen {
        let f = host.file_snap(host.nzb_file_at(nzb.ptr, i));
        if f.deleted {
            continue;
        }
        let Some((_, parsed)) =
            crate::parparser::parse_par_filename(&f.filename, f.filename_confirmed)
        else {
            continue;
        };
        let blocks =
            crate::pardamage::calc_queued_volume_blocks(parsed, f.size, verdict.block_size);
        if blocks <= 0 {
            continue;
        }
        if f.paused {
            paused.push((f.ptr, blocks, f.filename.clone()));
        } else {
            have += i64::from(blocks);
        }
    }
    for cf in host.nzb_completed_files(nzb.ptr) {
        if cf.status == crate::queuesubsys_host::CF_FAILURE {
            continue;
        }
        if let Some((_, blocks)) = crate::parparser::parse_par_filename(&cf.filename, true) {
            if blocks > 0 {
                have += i64::from(blocks);
            }
        }
    }
    let need = i64::from(verdict.damaged_blocks) - have;
    if need <= 0 || paused.is_empty() {
        return 0;
    }
    paused.sort_by_key(|p| p.1);
    let counts: Vec<i32> = paused.iter().map(|p| p.1).collect();
    let total: i64 = counts.iter().map(|&c| i64::from(c)).sum();
    let selected: Vec<usize> = if total <= need {
        (0..paused.len()).collect()
    } else {
        crate::postrepair::select_more_pars(&counts, need as i32).0
    };
    for &idx in &selected {
        let (ptr, blocks, ref name) = paused[idx];
        host.file_set_int(ptr, crate::queuesubsys_host::FILE_INT_PAUSED, 0);
        host.file_set_int(ptr, crate::queuesubsys_host::FILE_INT_EXTRA_PRIORITY, 1);
        let mut msg = b"Unpausing ".to_vec();
        msg.extend_from_slice(&nzb.name);
        msg.push(crate::paths::PATH_SEPARATOR);
        msg.extend_from_slice(name);
        msg.extend_from_slice(
            format!(
                " for live par-recovery ({blocks} block(s), {} damaged so far)",
                verdict.damaged_blocks
            )
            .as_bytes(),
        );
        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
    }
    selected.len()
}

#[cfg(unix)]
pub(crate) fn par_damage_forget_all(nzb_id: i32) {
    crate::pardamage::forget(nzb_id);
    crate::parchecker::streaming_session_forget(nzb_id);
}

#[cfg(unix)]
pub(crate) fn expected_segment_offset_at(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    file_ptr: usize,
    articles_len: usize,
    index: usize,
    art: &crate::queuesubsys_host::ArticleSnap,
) -> i64 {
    if index >= articles_len {
        return -1;
    }
    if index == 0 {
        return if art.part_number == 1 { 0 } else { -1 };
    }
    let prev = host.article_at(file_ptr, index - 1);
    if prev.part_number + 1 == art.part_number
        && prev.status == crate::queuesubsys_host::AI_FINISHED
        && prev.segment_size > 0
        && prev.segment_offset >= 0
    {
        return prev.segment_offset + i64::from(prev.segment_size);
    }
    -1
}

#[cfg(unix)]
pub(crate) fn expected_segment_end_at(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    file_ptr: usize,
    articles_len: usize,
    index: usize,
    art: &crate::queuesubsys_host::ArticleSnap,
    decoded_file_size: i64,
) -> i64 {
    if index >= articles_len {
        return -1;
    }
    if index + 1 == articles_len {
        return if decoded_file_size > 0 {
            decoded_file_size
        } else {
            -1
        };
    }
    let next = host.article_at(file_ptr, index + 1);
    if art.part_number + 1 == next.part_number
        && next.status == crate::queuesubsys_host::AI_FINISHED
        && next.segment_size > 0
        && next.segment_offset > 0
    {
        return next.segment_offset;
    }
    -1
}

#[cfg(unix)]
pub(crate) fn segment_aligned_at(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    file_ptr: usize,
    articles_len: usize,
    index: usize,
    art: &crate::queuesubsys_host::ArticleSnap,
    decoded_file_size: i64,
) -> bool {
    if index >= articles_len {
        return true;
    }
    let size = art.segment_size;
    if size <= 0 {
        return true;
    }
    let begin = art.segment_offset;
    if begin < 0 {
        return false;
    }
    let expected_begin = expected_segment_offset_at(host, file_ptr, articles_len, index, art);
    if expected_begin >= 0 && begin != expected_begin {
        return false;
    }
    let expected_end =
        expected_segment_end_at(host, file_ptr, articles_len, index, art, decoded_file_size);
    if expected_end >= 0 && begin + i64::from(size) != expected_end {
        return false;
    }
    true
}

#[cfg(unix)]
pub(crate) fn first_untiled_article_slice(
    articles: &[crate::queuesubsys_host::ArticleSnap],
    decoded_size: i64,
) -> Option<usize> {
    if decoded_size <= 0 {
        return None;
    }
    let geom: Vec<NzbgetRsArticleGeom> = articles
        .iter()
        .map(|a| NzbgetRsArticleGeom {
            part_number: a.part_number,
            status: a.status,
            segment_offset: a.segment_offset,
            segment_size: a.segment_size,
            fallback_round: a.dupe_fallback_round,
        })
        .collect();
    let idx = first_untiled_article(&geom, decoded_size);
    if idx >= 0 && (idx as usize) < articles.len() {
        Some(idx as usize)
    } else {
        None
    }
}

#[cfg(unix)]
pub(crate) fn has_donor_articles_slice(articles: &[crate::queuesubsys_host::ArticleSnap]) -> bool {
    articles.iter().any(|a| a.dupe_fallback_round > 0)
}

#[cfg(unix)]
pub(crate) fn borrowed_par2_mismatches_slice(
    articles: &[crate::queuesubsys_host::ArticleSnap],
    path: &[u8],
    dest_dir: &[u8],
    filename: &[u8],
) -> Vec<usize> {
    if articles.is_empty() || path.is_empty() || dest_dir.is_empty() {
        return Vec::new();
    }
    let mut statuses = Vec::with_capacity(articles.len());
    let mut donor_ids = Vec::with_capacity(articles.len());
    let mut fallback_rounds = Vec::with_capacity(articles.len());
    let mut segment_offsets = Vec::with_capacity(articles.len());
    let mut segment_sizes = Vec::with_capacity(articles.len());
    for a in articles {
        statuses.push(a.status);
        donor_ids.push(a.dupe_donor_id);
        fallback_rounds.push(a.dupe_fallback_round);
        segment_offsets.push(a.segment_offset);
        segment_sizes.push(a.segment_size);
    }
    let raw = borrowed_par2_mismatches_with_crc(
        crate::fsops::path(path),
        crate::fsops::path(dest_dir),
        filename,
        &statuses,
        &donor_ids,
        &fallback_rounds,
        &segment_offsets,
        &segment_sizes,
        crc32_ieee,
    );
    raw.into_iter()
        .filter_map(|idx| {
            if idx >= 0 && (idx as usize) < articles.len() {
                Some(idx as usize)
            } else {
                None
            }
        })
        .collect()
}

#[cfg(unix)]
pub(crate) fn merge_decoded_file_size_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    file: &mut crate::queuesubsys_host::FileSnap,
    article_file_size: i64,
) -> bool {
    let (conflict, next_size) = merge_decoded_file_size(file.decoded_file_size, article_file_size);
    if next_size != file.decoded_file_size {
        file.decoded_file_size = next_size;
        host.file_set_i64(
            file.ptr,
            crate::queuesubsys_host::FILE_I64_DECODED_FILE_SIZE,
            next_size,
        );
    }
    conflict
}

#[cfg(unix)]
pub(crate) fn first_content_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    file: &mut crate::queuesubsys_host::FileSnap,
) -> i32 {
    if file.first_content != crate::queuesubsys_host::FC_UNKNOWN {
        return file.first_content;
    }
    if file.articles_len == 0 || file.output_filename.is_empty() {
        return crate::queuesubsys_host::FC_UNKNOWN;
    }
    let a0 = host.article_at(file.ptr, 0);
    if a0.part_number != 1 || a0.status != crate::queuesubsys_host::AI_FINISHED {
        return crate::queuesubsys_host::FC_UNKNOWN;
    }
    let Ok(mut f) = File::open(crate::fsops::path(&file.output_filename)) else {
        return crate::queuesubsys_host::FC_UNKNOWN;
    };
    let mut head = [0u8; 8];
    if f.read_exact(&mut head).is_err() {
        return crate::queuesubsys_host::FC_UNKNOWN;
    }
    let cls = classify_first_head(&head);
    if cls == 0 {
        return crate::queuesubsys_host::FC_UNKNOWN;
    }
    let fc = if cls == 1 {
        crate::queuesubsys_host::FC_PAR2
    } else {
        crate::queuesubsys_host::FC_OTHER
    };
    file.first_content = fc;
    host.file_set_int(
        file.ptr,
        crate::queuesubsys_host::FILE_INT_FIRST_CONTENT,
        fc,
    );
    fc
}

#[cfg(unix)]
pub(crate) fn register_lead_failure_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    file: &mut crate::queuesubsys_host::FileSnap,
    art: &crate::queuesubsys_host::ArticleSnap,
) -> bool {
    let mut lead_donor_id = file.dupe_lead_donor_id;
    let mut lead_failures = file.dupe_lead_failures;
    let mut lead_switches = file.dupe_lead_switches;
    let rotated = register_lead_failure(
        art.dupe_fallback_round,
        art.dupe_lead_snapshot,
        art.dupe_donor_count,
        art.dupe_next_lead,
        &mut lead_donor_id,
        &mut lead_failures,
        &mut lead_switches,
    );
    file.dupe_lead_donor_id = lead_donor_id;
    file.dupe_lead_failures = lead_failures;
    file.dupe_lead_switches = lead_switches;
    host.file_set_int(
        file.ptr,
        crate::queuesubsys_host::FILE_INT_DUPE_LEAD_DONOR_ID,
        lead_donor_id,
    );
    host.file_set_int(
        file.ptr,
        crate::queuesubsys_host::FILE_INT_DUPE_LEAD_FAILURES,
        lead_failures,
    );
    host.file_set_int(
        file.ptr,
        crate::queuesubsys_host::FILE_INT_DUPE_LEAD_SWITCHES,
        lead_switches,
    );
    rotated
}

#[cfg(unix)]
pub(crate) fn register_lead_success_at(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    file: &mut crate::queuesubsys_host::FileSnap,
    articles_len: usize,
    art_idx: usize,
    art: &crate::queuesubsys_host::ArticleSnap,
) {
    if art_idx >= articles_len {
        return;
    }
    if art.dupe_fallback_round != 1
        || art.dupe_lead_snapshot <= 0
        || art.dupe_lead_snapshot != file.dupe_lead_donor_id
    {
        return;
    }
    let exp_off = expected_segment_offset_at(host, file.ptr, articles_len, art_idx, art);
    let exp_end = expected_segment_end_at(
        host,
        file.ptr,
        articles_len,
        art_idx,
        art,
        file.decoded_file_size,
    );
    let mut lead_failures = file.dupe_lead_failures;
    let mut lead_switches = file.dupe_lead_switches;
    register_lead_success(
        art.dupe_fallback_round,
        art.dupe_lead_snapshot,
        file.dupe_lead_donor_id,
        exp_off,
        exp_end,
        &mut lead_failures,
        &mut lead_switches,
    );
    file.dupe_lead_failures = lead_failures;
    file.dupe_lead_switches = lead_switches;
    host.file_set_int(
        file.ptr,
        crate::queuesubsys_host::FILE_INT_DUPE_LEAD_FAILURES,
        lead_failures,
    );
    host.file_set_int(
        file.ptr,
        crate::queuesubsys_host::FILE_INT_DUPE_LEAD_SWITCHES,
        lead_switches,
    );
}

#[cfg(unix)]
pub(crate) fn twin_identity_of(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
) -> Vec<u8> {
    let files_param = host.nzb_get_param(nzb.ptr, b"DupeFiles");
    let posting = nzb.full_content_hash.to_string();
    crate::twin::format_identity(files_param.as_deref().unwrap_or(b""), posting.as_bytes())
}

#[cfg(unix)]
pub(crate) fn collect_donors_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb: &crate::queuesubsys_host::NzbSnap,
    qdir: &[u8],
) -> Vec<crate::queuesubsys_host::NzbSnap> {
    let mut donors = Vec::new();
    if dq_ptr == 0 {
        return donors;
    }
    let qlen = host.queue_len(dq_ptr);
    for i in 0..qlen {
        let q_ptr = host.queue_at(dq_ptr, i);
        if q_ptr == 0 || q_ptr == nzb.ptr {
            continue;
        }
        let q = host.nzb_snap(q_ptr, false);
        if q.kind == crate::queuesubsys_host::NK_NZB
            && q.dupe_mode != crate::queuesubsys_host::DM_FORCE
            && crate::dupecoord::same_name_or_key(&q.name, &q.dupe_key, &nzb.name, &nzb.dupe_key)
        {
            donors.push(q);
        }
    }

    let hlen = host.history_len(dq_ptr);
    for i in 0..hlen {
        let hi = host.history_at(dq_ptr, i);
        if hi.kind == crate::queuesubsys_host::HK_NZB && hi.inner_ptr != 0 {
            let hn = host.nzb_snap(hi.inner_ptr, false);
            if hn.dupe_mode != crate::queuesubsys_host::DM_FORCE
                && crate::dupecoord::same_name_or_key(
                    &hn.name,
                    &hn.dupe_key,
                    &nzb.name,
                    &nzb.dupe_key,
                )
            {
                donors.push(hn);
            }
        }
    }

    let own_identity = twin_identity_of(host, nzb);
    let mut twin_ptrs = BTreeSet::new();
    if !own_identity.is_empty() {
        for donor in &donors {
            if nzb.full_content_hash > 0 && nzb.full_content_hash == donor.full_content_hash {
                continue;
            }
            let donor_identity = twin_identity_of(host, donor);
            if !donor_identity.is_empty() {
                let kind = crate::twin::store_kind_dir(
                    qdir,
                    nzb.id,
                    &own_identity,
                    donor.id,
                    &donor_identity,
                );
                if kind.starts_with(b"twin") {
                    twin_ptrs.insert(donor.ptr);
                }
            }
        }
    }

    donors.sort_by(|d1, d2| {
        let t1 = twin_ptrs.contains(&d1.ptr);
        let t2 = twin_ptrs.contains(&d2.ptr);
        if t1 != t2 {
            return if t1 {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        if d1.dupe_score != d2.dupe_score {
            return d2.dupe_score.cmp(&d1.dupe_score);
        }
        d1.id.cmp(&d2.id)
    });

    donors
}

#[cfg(unix)]
fn nzb_filename_of_host(
    cache: &mut DonorCacheState,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    own_nzb: &crate::queuesubsys_host::NzbSnap,
    file_ptr: usize,
    articles_len: usize,
) -> Vec<u8> {
    if articles_len == 0 {
        return Vec::new();
    }
    let Some(parsed_own) = cache.get_parsed_donor(host, own_nzb, opts) else {
        return Vec::new();
    };
    let mut probed: Vec<Option<crate::queuesubsys_host::ArticleSnap>> = vec![None; articles_len];
    for own_file in &parsed_own.files {
        if own_file.article_parts.len() != articles_len
            || own_file.article_msg_ids.len() != articles_len
        {
            continue;
        }
        for i in 0..articles_len {
            let article = probed[i].get_or_insert_with(|| host.article_at(file_ptr, i));
            let msg_id = if !article.dupe_original_message_id.is_empty() {
                article.dupe_original_message_id.as_slice()
            } else {
                article.message_id.as_slice()
            };
            if own_file.article_parts[i] == article.part_number && !msg_id.is_empty() {
                if own_file.article_msg_ids[i].as_slice() == msg_id {
                    return own_file.filename.clone();
                }
                break;
            }
        }
    }
    Vec::new()
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn match_donor_file_parsed<'a>(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    qdir: &[u8],
    own_nzb: &crate::queuesubsys_host::NzbSnap,
    target_file: &crate::queuesubsys_host::FileSnap,
    target_parts: &[i32],
    target_sizes: &[i32],
    parsed_donor: &'a crate::articlefetcher::DonorParsedNzb,
    target_nzb_filename: &[u8],
    donor_nzb_id: i32,
    donor_print: &[u8],
    twin_only: bool,
) -> Option<&'a crate::articlefetcher::DonorParsedFile> {
    if file_is_par_snap(target_file) {
        return None;
    }
    let own_identity = if own_nzb.ptr != 0 {
        twin_identity_of(host, own_nzb)
    } else {
        Vec::new()
    };
    let mut twin_allowed = false;
    if twin_only {
        let kind = if donor_nzb_id != 0 && !own_identity.is_empty() && !donor_print.is_empty() {
            crate::twin::store_kind_dir(qdir, own_nzb.id, &own_identity, donor_nzb_id, donor_print)
        } else {
            Vec::new()
        };
        twin_allowed = donor_nzb_id != 0
            && !own_identity.is_empty()
            && !donor_print.is_empty()
            && kind.starts_with(b"twin");
        if !twin_allowed {
            return None;
        }
    }
    let by_content = if donor_nzb_id != 0 && !own_identity.is_empty() && !donor_print.is_empty() {
        crate::twin::store_match_dir(
            qdir,
            own_nzb.id,
            &own_identity,
            &target_file.filename,
            target_nzb_filename,
            donor_nzb_id,
            donor_print,
        )
    } else {
        Vec::new()
    };

    let target_spec = DonorFileSpec {
        filename: &target_file.filename,
        subject: &target_file.subject,
        total_articles: target_file.total_articles,
        size: target_file.size,
        is_par_file: false,
        parts: target_parts,
        sizes: target_sizes,
    };

    let donor_specs: Vec<DonorFileSpec<'_>> = parsed_donor
        .files
        .iter()
        .map(|df| DonorFileSpec {
            filename: &df.filename,
            subject: &df.subject,
            total_articles: df.total_articles,
            size: df.size,
            is_par_file: is_par_file(df.par_file, 0, 1, false, &df.filename),
            parts: &df.article_parts,
            sizes: &df.article_sizes,
        })
        .collect();

    let idx = match_donor_file(
        &target_spec,
        target_nzb_filename,
        &by_content,
        twin_allowed,
        twin_only,
        &donor_specs,
    )?;
    parsed_donor.files.get(idx)
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn pin_sources_host(
    cache: &mut DonorCacheState,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    dq_ptr: usize,
    nzb: &crate::queuesubsys_host::NzbSnap,
    file: &mut crate::queuesubsys_host::FileSnap,
    art: &mut crate::queuesubsys_host::ArticleSnap,
    twin_only: bool,
) {
    let mut donors = collect_donors_host(host, dq_ptr, nzb, &opts.queue_dir);

    // VacateGhostLead
    if file.dupe_lead_donor_id != 0 {
        let lead_id = file.dupe_lead_donor_id;
        let barred = nzb.ptr != 0 && host.nzb_blocked_donor_op(nzb.ptr, false, lead_id);
        let present = donors.iter().any(|d| d.id == lead_id);
        if barred || !present {
            file.dupe_lead_donor_id = 0;
            file.dupe_lead_failures = 0;
            file.dupe_lead_switches = 0;
            host.file_set_int(
                file.ptr,
                crate::queuesubsys_host::FILE_INT_DUPE_LEAD_DONOR_ID,
                0,
            );
            host.file_set_int(
                file.ptr,
                crate::queuesubsys_host::FILE_INT_DUPE_LEAD_FAILURES,
                0,
            );
            host.file_set_int(
                file.ptr,
                crate::queuesubsys_host::FILE_INT_DUPE_LEAD_SWITCHES,
                0,
            );
        }
    }

    if twin_only && file.dupe_lead_donor_id != 0 {
        let own_identity = twin_identity_of(host, nzb);
        let lead_id = file.dupe_lead_donor_id;
        let lead_identity = donors
            .iter()
            .find(|d| d.id == lead_id)
            .map(|d| twin_identity_of(host, d))
            .unwrap_or_default();
        let lead_kind = if !own_identity.is_empty() && !lead_identity.is_empty() {
            crate::twin::store_kind_dir(
                &opts.queue_dir,
                nzb.id,
                &own_identity,
                lead_id,
                &lead_identity,
            )
        } else {
            Vec::new()
        };
        if own_identity.is_empty() || lead_identity.is_empty() || !lead_kind.starts_with(b"twin") {
            file.dupe_lead_donor_id = 0;
            file.dupe_lead_failures = 0;
            file.dupe_lead_switches = 0;
            host.file_set_int(
                file.ptr,
                crate::queuesubsys_host::FILE_INT_DUPE_LEAD_DONOR_ID,
                0,
            );
            host.file_set_int(
                file.ptr,
                crate::queuesubsys_host::FILE_INT_DUPE_LEAD_FAILURES,
                0,
            );
            host.file_set_int(
                file.ptr,
                crate::queuesubsys_host::FILE_INT_DUPE_LEAD_SWITCHES,
                0,
            );
        }
    }

    rotate_to_lead(&mut donors, file.dupe_lead_donor_id, |d| d.id);

    let target_nzb_filename = if donors.is_empty() {
        Vec::new()
    } else {
        nzb_filename_of_host(cache, host, opts, nzb, file.ptr, file.articles_len)
    };

    let (target_parts, target_sizes) = if donors.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        let hit = cache
            .file_parts_sizes
            .as_ref()
            .is_some_and(|c| c.0 == nzb.id && c.1 == file.id && c.2 == file.articles_len);
        if !hit {
            let mut parts = Vec::with_capacity(file.articles_len);
            let mut sizes = Vec::with_capacity(file.articles_len);
            for i in 0..file.articles_len {
                let a = host.article_at(file.ptr, i);
                parts.push(a.part_number);
                sizes.push(a.size);
            }
            cache.file_parts_sizes = Some((nzb.id, file.id, file.articles_len, parts, sizes));
        }
        let cached = cache.file_parts_sizes.as_ref().unwrap();
        (cached.3.clone(), cached.4.clone())
    };

    let part_number = art.part_number;
    let mut candidates: Vec<Vec<u8>> = Vec::new();
    let mut contributors: Vec<i32> = Vec::new();
    let mut same_posting = 0i32;
    let mut unparsed = 0i32;

    for donor in &donors {
        if host.nzb_blocked_donor_op(nzb.ptr, false, donor.id) {
            continue;
        }
        if nzb.full_content_hash > 0 && nzb.full_content_hash == donor.full_content_hash {
            same_posting += 1;
            continue;
        }
        if let Some(parsed_donor) = cache.get_parsed_donor(host, donor, opts) {
            let donor_identity = twin_identity_of(host, donor);
            if let Some(df) = match_donor_file_parsed(
                host,
                &opts.queue_dir,
                nzb,
                file,
                &target_parts,
                &target_sizes,
                &parsed_donor,
                &target_nzb_filename,
                donor.id,
                &donor_identity,
                twin_only,
            ) {
                let mut found_msg: Option<&[u8]> = None;
                for (j, &part) in df.article_parts.iter().enumerate() {
                    if part == part_number {
                        if let Some(m) = df.article_msg_ids.get(j) {
                            if !m.is_empty() {
                                found_msg = Some(m.as_slice());
                                break;
                            }
                        }
                    }
                }
                if let Some(msg_id) = found_msg {
                    if !candidates.iter().any(|c| c.as_slice() == msg_id) {
                        candidates.push(msg_id.to_vec());
                        contributors.push(donor.id);
                    }
                }
            }
        } else {
            unparsed += 1;
        }
    }

    if !twin_only && candidates.is_empty() && !donors.is_empty() && !file.dupe_no_source_logged {
        file.dupe_no_source_logged = true;
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_DUPE_NO_SOURCE_LOGGED,
            1,
        );
        let nzb_name_disp: &[u8] = if target_nzb_filename.is_empty() {
            b"unknown"
        } else {
            &target_nzb_filename
        };
        let mut msg = b"No duplicate source for ".to_vec();
        msg.extend_from_slice(&file.filename);
        msg.extend_from_slice(
            format!(
                " [{}]: {} duplicate(s), {} same posting, {} without readable nzb-file, none with a matching file (nzb name: ",
                part_number,
                donors.len(),
                same_posting,
                unparsed
            )
            .as_bytes(),
        );
        msg.extend_from_slice(nzb_name_disp);
        msg.extend_from_slice(b"); not repeated for its other articles");
        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_DETAIL, &msg);
    }

    // FinishPin
    let (lead_snapshot, next_lead, donor_count, new_file_lead) =
        compute_pin_lead_state(&contributors, file.dupe_lead_donor_id);
    file.dupe_lead_donor_id = new_file_lead;
    host.file_set_int(
        file.ptr,
        crate::queuesubsys_host::FILE_INT_DUPE_LEAD_DONOR_ID,
        new_file_lead,
    );

    let append_primary = file.dupe_cutover && !candidates.is_empty();
    let mut sources = candidates;
    if append_primary {
        sources.push(art.dupe_original_message_id.clone());
    }
    art.dupe_lead_snapshot = lead_snapshot;
    art.dupe_next_lead = next_lead;
    art.dupe_donor_count = donor_count;
    host.article_store_pin(
        art.ptr,
        lead_snapshot,
        next_lead,
        donor_count,
        &sources,
        &contributors,
    );
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_fallback_host(
    cache: &mut DonorCacheState,
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    dq_ptr: usize,
    nzb: &mut crate::queuesubsys_host::NzbSnap,
    file: &mut crate::queuesubsys_host::FileSnap,
    art_idx: usize,
    art: &mut crate::queuesubsys_host::ArticleSnap,
) -> bool {
    if art_idx >= file.articles_len {
        return false;
    }
    if opts.dupe_article_fallback == crate::queuesubsys_host::DAF_NONE
        || opts.raw_article
        || file_is_par_snap(file)
    {
        return false;
    }

    let content = first_content_host(host, file);
    if !check_try_preconditions(
        opts.dupe_article_fallback,
        crate::queuesubsys_host::DAF_NONE,
        opts.raw_article,
        false,
        content,
        crate::queuesubsys_host::FC_PAR2,
        crate::queuesubsys_host::FC_UNKNOWN,
        art.part_number,
        &file.filename,
        file.deleted,
        nzb.deleting,
        nzb.parking,
        nzb.delete_status,
        crate::queuesubsys_host::DS_NONE,
        nzb.dupe_mode,
        crate::queuesubsys_host::DM_FORCE,
    ) {
        return false;
    }

    if !has_par2_host(host, nzb) {
        if !nzb.dupe_no_par_noted {
            nzb.dupe_no_par_noted = true;
            host.nzb_set_int(
                nzb.ptr,
                crate::queuesubsys_host::NZB_INT_DUPE_NO_PAR_NOTED,
                1,
            );
            let mut msg = b"Not borrowing articles of ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b" from duplicates: it has no par2 files to check them");
            host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
        }
        return false;
    }

    let mut defer = false;
    if opts.dupe_article_fallback >= crate::queuesubsys_host::DAF_STREAM {
        let par_defer = opts.par_check != crate::queuesubsys_host::PC_MANUAL
            && should_defer_to_par_snap(host, nzb);
        let damage = if par_defer {
            par_damage_judge_host(host, nzb)
        } else {
            ParDamageVerdict::default()
        };
        // Option DupeBorrowFirst: a missing article is fetched from a duplicate right away
        // (a few MB, no repair pass); the par2 set stays the final check. Names are still
        // unknown while direct rename runs, so that case keeps deferring.
        let rename_pending =
            nzb.direct_rename_status == crate::queuesubsys_host::TS_RUNNING && !nzb.all_first;
        defer = par_defer
            && (rename_pending || !crate::options::dupe_borrow_first())
            && art.part_number != 1
            && nzb.dupe_par_defer_state != crate::queuesubsys_host::DP_LIFTED
            && !par_cannot_cover_snap(nzb, true)
            && !damage.certain()
            && !damage.projected();
        if par_defer && !defer && art.part_number != 1 {
            if nzb.dupe_par_defer_state == crate::queuesubsys_host::DP_DEFERRED {
                let mut msg = b"Damage of ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(
                    b" exceeds its par2 recovery data (or will, judged by the articles tried so far), recovering missing articles from duplicates",
                );
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
            }
            nzb.dupe_par_defer_state = crate::queuesubsys_host::DP_LIFTED;
            host.nzb_set_int(
                nzb.ptr,
                crate::queuesubsys_host::NZB_INT_DUPE_PAR_DEFER_STATE,
                crate::queuesubsys_host::DP_LIFTED,
            );
        }
    }

    let mut round = art.dupe_fallback_round;
    if defer {
        if round == 0 && dq_ptr == 0 {
            return false;
        }
        if round > 0 {
            let (_, source_donors) = host.article_pinned_sources(art.ptr);
            if source_donors.is_empty() {
                return false;
            }
        }
    }

    if art.dupe_original_message_id.is_empty() {
        let orig = art.message_id.clone();
        art.dupe_original_message_id = orig.clone();
        host.article_set_str(
            art.ptr,
            crate::queuesubsys_host::ART_STR_DUPE_ORIG_MESSAGE_ID,
            Some(&orig),
        );
    }

    if round == 0 {
        pin_sources_host(cache, host, opts, dq_ptr, nzb, file, art, defer);
    } else if round == 1 && register_lead_failure_host(host, file, art) {
        let mut msg = b"Switching lead duplicate collection for ".to_vec();
        msg.extend_from_slice(&file.filename);
        msg.extend_from_slice(b" (the lead duplicate is missing many articles)");
        host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
    }

    let (sources, source_donors) = host.article_pinned_sources(art.ptr);
    let pinned_round = round;
    while round >= 0
        && (round as usize) < sources.len()
        && (round as usize) < source_donors.len()
        && host.nzb_blocked_donor_op(nzb.ptr, false, source_donors[round as usize])
    {
        round += 1;
    }

    if round < 0 || (round as usize) >= sources.len() {
        if defer {
            if !art.dupe_proactive
                && nzb.dupe_par_defer_state == crate::queuesubsys_host::DP_NONE
                && dq_ptr != 0
                && !collect_donors_host(host, dq_ptr, nzb, &opts.queue_dir).is_empty()
            {
                nzb.dupe_par_defer_state = crate::queuesubsys_host::DP_DEFERRED;
                host.nzb_set_int(
                    nzb.ptr,
                    crate::queuesubsys_host::NZB_INT_DUPE_PAR_DEFER_STATE,
                    crate::queuesubsys_host::DP_DEFERRED,
                );
                let mut msg = b"Deferring duplicate recovery for ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(b" to par-check (its par2 files may cover the damage)");
                host.nzb_print_message(nzb.ptr, crate::queuesubsys_host::MK_INFO, &msg);
            }
            if pinned_round > 0 && !art.dupe_original_message_id.is_empty() {
                let orig = art.dupe_original_message_id.clone();
                art.message_id = orig.clone();
                host.article_set_str(
                    art.ptr,
                    crate::queuesubsys_host::ART_STR_MESSAGE_ID,
                    Some(&orig),
                );
            }
        } else if pinned_round == 0 && !art.dupe_proactive {
            nzb.dupe_unsourced_articles += 1;
            host.nzb_set_int(
                nzb.ptr,
                crate::queuesubsys_host::NZB_INT_DUPE_UNSOURCED_ARTICLES,
                nzb.dupe_unsourced_articles,
            );
        } else if !art.dupe_original_message_id.is_empty() {
            let orig = art.dupe_original_message_id.clone();
            art.message_id = orig.clone();
            host.article_set_str(
                art.ptr,
                crate::queuesubsys_host::ART_STR_MESSAGE_ID,
                Some(&orig),
            );
        }
        return false;
    }

    if pinned_round == 0 && !art.dupe_proactive {
        file.dupe_attempted_articles += 1;
        nzb.dupe_attempted_articles += 1;
        host.file_set_int(
            file.ptr,
            crate::queuesubsys_host::FILE_INT_DUPE_ATTEMPTED_ARTICLES,
            file.dupe_attempted_articles,
        );
        host.nzb_set_int(
            nzb.ptr,
            crate::queuesubsys_host::NZB_INT_DUPE_ATTEMPTED_ARTICLES,
            nzb.dupe_attempted_articles,
        );
    }

    let exp_off = expected_segment_offset_at(host, file.ptr, file.articles_len, art_idx, art);
    let exp_end = expected_segment_end_at(
        host,
        file.ptr,
        file.articles_len,
        art_idx,
        art,
        file.decoded_file_size,
    );
    host.article_set_i64_pair(art.ptr, exp_off, exp_end);

    let chosen = sources[round as usize].clone();
    art.message_id = chosen.clone();
    art.dupe_fallback_round = round + 1;
    host.article_set_str(
        art.ptr,
        crate::queuesubsys_host::ART_STR_MESSAGE_ID,
        Some(&chosen),
    );
    host.article_set_int(
        art.ptr,
        crate::queuesubsys_host::ART_INT_DUPE_FALLBACK_ROUND,
        round + 1,
    );
    true
}

#[cfg(unix)]
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
        assert_eq!(
            declared_parts(b"release [01/25] - \"vol1.rar\" yEnc (1/30)"),
            30
        );
        assert_eq!(declared_parts(b"(1/10) (5/42)"), 42);
        assert_eq!(declared_parts(b"obfuscated subject without parens"), 0);
        assert_eq!(declared_parts(b"(1/0) (-1/10) (1/10x)"), 0);
    }

    #[test]
    fn test_steps_hash_and_match() {
        let a = [
            700000, 700120, 699980, 700050, 700010, 699900, 700040, 500000,
        ];
        let b = [
            700050, 700170, 700030, 700100, 700060, 699950, 700090, 500050,
        ];
        let flat = [
            700000, 700000, 700000, 700000, 700000, 700000, 700000, 500000,
        ];
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

        // ArticleResponsesTellMissingFromTemporary
        assert_eq!(
            article_downloader_classify_response(b"222 body follows"),
            AD_FINISHED
        );
        assert_eq!(
            article_downloader_classify_response(b"430 no such article"),
            AD_NOT_FOUND
        );
        assert_eq!(
            article_downloader_classify_response(b"423 no such article number"),
            AD_NOT_FOUND
        );
        assert_eq!(
            article_downloader_classify_response(b"411 no such group"),
            AD_NOT_FOUND
        );
        assert_eq!(
            article_downloader_classify_response(b"451 no such article"),
            AD_NOT_FOUND
        );
        for response in [
            &b"480 authentication required"[..],
            &b"481 authentication failed"[..],
            &b"482 out of sequence"[..],
            &b"502 access denied"[..],
            &b"503 temporarily unavailable"[..],
        ] {
            assert_eq!(
                article_downloader_classify_response(response),
                AD_TRANSIENT_ERROR
            );
        }
        assert_eq!(
            article_downloader_classify_response(b"436 Connection unavailable"),
            AD_TRANSIENT_ERROR
        );
        assert_eq!(
            article_downloader_classify_response(b"430 backend pool exhausted"),
            AD_TRANSIENT_ERROR
        );
        assert_eq!(
            article_downloader_classify_response(b"400 service discontinued"),
            AD_CONNECT_ERROR
        );
        assert_eq!(
            article_downloader_classify_response(b"500 command not recognized"),
            AD_FAILED
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

        assert_eq!(
            should_defer_to_par_header(false, 0, 1, false, 0, 2, 100),
            -1
        );
        assert_eq!(should_defer_to_par_header(true, 1, 1, false, 0, 2, 0), 1);
        assert_eq!(should_defer_to_par_header(true, 0, 1, false, 2, 2, 100), -1);
        assert_eq!(should_defer_to_par_header(true, 0, 1, false, 0, 2, 100), 1);
        assert_eq!(should_defer_to_par_header(true, 0, 1, false, 0, 2, 0), 0);

        assert!(check_try_preconditions(
            1,
            0,
            false,
            false,
            0,
            2,
            0,
            2,
            b"part01.rar",
            false,
            false,
            false,
            0,
            0,
            0,
            2
        ));
        assert!(!check_try_preconditions(
            1,
            0,
            false,
            false,
            0,
            2,
            0,
            2,
            b"obfuscated",
            false,
            false,
            false,
            0,
            0,
            0,
            2
        ));
        assert!(check_try_preconditions(
            1,
            0,
            false,
            false,
            0,
            2,
            0,
            1,
            b"obfuscated",
            false,
            false,
            false,
            0,
            0,
            0,
            2
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

        assert!(own_article_matches(
            3,
            3,
            b"<orig@id>",
            b"<orig@id>",
            b"<sub@id>"
        ));
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

    #[derive(Clone, Debug)]
    struct TestArticle {
        part_number: i32,
        size: i32,
        message_id: Vec<u8>,
        status: i32,
        segment_offset: i64,
        segment_size: i32,
        fallback_round: i32,
        lead_snapshot: i32,
        next_lead: i32,
        donor_count: i32,
        sources: Vec<Vec<u8>>,
        source_donors: Vec<i32>,
    }

    #[derive(Clone, Debug)]
    struct TestFile {
        filename: Vec<u8>,
        subject: Vec<u8>,
        size: i64,
        total_articles: i32,
        decoded_file_size: i64,
        par_file: bool,
        filename_confirmed: bool,
        first_content: i32,
        lead_donor_id: i32,
        lead_failures: i32,
        lead_switches: i32,
        articles: Vec<TestArticle>,
    }

    impl TestFile {
        fn build(filename: &[u8], parts: &[(i32, i32)], msg_id_prefix: &[u8]) -> Self {
            let mut size = 0i64;
            let mut articles = Vec::with_capacity(parts.len());
            for &(part_num, part_size) in parts {
                size += part_size as i64;
                let msg_id = format!(
                    "{}-{}@example.com",
                    String::from_utf8_lossy(msg_id_prefix),
                    part_num
                )
                .into_bytes();
                articles.push(TestArticle {
                    part_number: part_num,
                    size: part_size,
                    message_id: msg_id,
                    status: 0,
                    segment_offset: 0,
                    segment_size: 0,
                    fallback_round: 0,
                    lead_snapshot: 0,
                    next_lead: 0,
                    donor_count: 0,
                    sources: Vec::new(),
                    source_donors: Vec::new(),
                });
            }
            Self {
                filename: filename.to_vec(),
                subject: Vec::new(),
                size,
                total_articles: parts.len() as i32,
                decoded_file_size: 0,
                par_file: false,
                filename_confirmed: false,
                first_content: 0,
                lead_donor_id: 0,
                lead_failures: 0,
                lead_switches: 0,
                articles,
            }
        }

        fn is_par(&self) -> bool {
            is_par_file(
                self.par_file,
                self.first_content,
                1,
                self.filename_confirmed,
                &self.filename,
            )
        }

        fn parts_and_sizes(&self) -> (Vec<i32>, Vec<i32>) {
            let parts = self.articles.iter().map(|a| a.part_number).collect();
            let sizes = self.articles.iter().map(|a| a.size).collect();
            (parts, sizes)
        }

        fn finish_article(&mut self, index: usize, offset: i64, size: i32) {
            let a = &mut self.articles[index];
            a.status = ARTICLE_STATUS_FINISHED;
            a.segment_offset = offset;
            a.segment_size = size;
        }

        fn expected_offset(&self, index: usize) -> i64 {
            let a = &self.articles[index];
            let prev = if index > 0 {
                Some(&self.articles[index - 1])
            } else {
                None
            };
            expected_segment_offset(
                index as isize,
                a.part_number,
                prev.map_or(0, |p| p.part_number),
                prev.map_or(0, |p| p.status),
                prev.map_or(0, |p| p.segment_offset),
                prev.map_or(0, |p| p.segment_size),
            )
        }

        fn expected_end(&self, index: usize) -> i64 {
            let a = &self.articles[index];
            let next = self.articles.get(index + 1);
            expected_segment_end(
                index as isize,
                self.articles.len(),
                a.part_number,
                self.decoded_file_size,
                next.map_or(0, |n| n.part_number),
                next.map_or(0, |n| n.status),
                next.map_or(0, |n| n.segment_offset),
                next.map_or(0, |n| n.segment_size),
            )
        }

        fn segment_aligned(&self, index: usize) -> bool {
            let a = &self.articles[index];
            segment_aligned(
                a.segment_offset,
                a.segment_size,
                self.expected_offset(index),
                self.expected_end(index),
            )
        }

        fn first_untiled(&self) -> Option<usize> {
            let geom: Vec<NzbgetRsArticleGeom> = self
                .articles
                .iter()
                .map(|a| NzbgetRsArticleGeom {
                    part_number: a.part_number,
                    status: a.status,
                    segment_offset: a.segment_offset,
                    segment_size: a.segment_size,
                    fallback_round: a.fallback_round,
                })
                .collect();
            let idx = first_untiled_article(&geom, self.decoded_file_size);
            if idx >= 0 {
                Some(idx as usize)
            } else {
                None
            }
        }

        fn merge_decoded_size(&mut self, article_file_size: i64) -> bool {
            let (conflict, new_size) =
                merge_decoded_file_size(self.decoded_file_size, article_file_size);
            self.decoded_file_size = new_size;
            conflict
        }

        fn has_donor_articles(&self) -> bool {
            self.articles.iter().any(|a| a.fallback_round > 0)
        }

        fn finish_pin(
            &mut self,
            article_idx: usize,
            candidates: &[Vec<u8>],
            contributors: &[i32],
            cutover: bool,
            primary_msg_id: &[u8],
        ) {
            let (lead_snapshot, next_lead, donor_count, new_file_lead) =
                compute_pin_lead_state(contributors, self.lead_donor_id);
            self.lead_donor_id = new_file_lead;
            let a = &mut self.articles[article_idx];
            a.lead_snapshot = lead_snapshot;
            a.next_lead = next_lead;
            a.donor_count = donor_count;
            a.sources = order_sources(candidates, cutover, primary_msg_id);
            a.source_donors = contributors.to_vec();
        }

        fn register_lead_failure(&mut self, article_idx: usize) -> bool {
            let a = &self.articles[article_idx];
            register_lead_failure(
                a.fallback_round,
                a.lead_snapshot,
                a.donor_count,
                a.next_lead,
                &mut self.lead_donor_id,
                &mut self.lead_failures,
                &mut self.lead_switches,
            )
        }

        fn register_lead_success(&mut self, article_idx: usize) {
            let exp_off = self.expected_offset(article_idx);
            let exp_end = self.expected_end(article_idx);
            let a = &self.articles[article_idx];
            register_lead_success(
                a.fallback_round,
                a.lead_snapshot,
                self.lead_donor_id,
                exp_off,
                exp_end,
                &mut self.lead_failures,
                &mut self.lead_switches,
            );
        }

        fn vacate_ghost_lead(&mut self, is_barred: bool, donor_ids: &[i32]) {
            if should_vacate_ghost_lead(self.lead_donor_id, is_barred, donor_ids) {
                self.lead_donor_id = 0;
                self.lead_failures = 0;
                self.lead_switches = 0;
            }
        }
    }

    fn match_test_donor<'a>(
        target: &TestFile,
        donor_files: &'a [TestFile],
        target_nzb_filename: &[u8],
        by_content: &[u8],
        twin_allowed: bool,
        twin_only: bool,
    ) -> Option<&'a TestFile> {
        let (t_parts, t_sizes) = target.parts_and_sizes();
        let target_spec = DonorFileSpec {
            filename: &target.filename,
            subject: &target.subject,
            total_articles: target.total_articles,
            size: target.size,
            is_par_file: target.is_par(),
            parts: &t_parts,
            sizes: &t_sizes,
        };
        let d_parts_sizes: Vec<(Vec<i32>, Vec<i32>)> =
            donor_files.iter().map(|df| df.parts_and_sizes()).collect();
        let donor_specs: Vec<DonorFileSpec<'_>> = donor_files
            .iter()
            .enumerate()
            .map(|(i, df)| DonorFileSpec {
                filename: &df.filename,
                subject: &df.subject,
                total_articles: df.total_articles,
                size: df.size,
                is_par_file: df.is_par(),
                parts: &d_parts_sizes[i].0,
                sizes: &d_parts_sizes[i].1,
            })
            .collect();
        let idx = match_donor_file(
            &target_spec,
            target_nzb_filename,
            by_content,
            twin_allowed,
            twin_only,
            &donor_specs,
        )?;
        Some(&donor_files[idx])
    }

    fn build_candidate_message_ids(
        parsed_donors: &[Vec<TestFile>],
        target: &TestFile,
        part_number: i32,
    ) -> Vec<Vec<u8>> {
        let mut candidates: Vec<Vec<u8>> = Vec::new();
        for donor_files in parsed_donors {
            let Some(matched) = match_test_donor(target, donor_files, b"", b"", false, false)
            else {
                continue;
            };
            let pairs: Vec<(i32, &[u8])> = matched
                .articles
                .iter()
                .map(|a| (a.part_number, a.message_id.as_slice()))
                .collect();
            let Some(msg_id) = find_donor_message_id(&pairs, part_number) else {
                continue;
            };
            if !candidates.iter().any(|c| c.as_slice() == msg_id) {
                candidates.push(msg_id.to_vec());
            }
        }
        candidates
    }

    #[test]
    fn dupe_article_fallback_full_suite_port_from_cpp() {
        // 1. DupeArticleFallbackRejectsDamagedPar2BlockSizeTest
        {
            let dir =
                std::env::temp_dir().join(format!("nzbget-par2-block-rs-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let data = vec![b'x'; 100];
            let f_bin = dir.join("f.bin");
            std::fs::write(&f_bin, &data).unwrap();

            let packet = |pkt_type: &[u8; 16], body: &[u8]| -> Vec<u8> {
                let mut out = Vec::from(&b"PAR2\0PKT"[..]);
                let length = (64 + body.len()) as u64;
                out.extend_from_slice(&length.to_le_bytes());
                out.extend_from_slice(&[0u8; 32]);
                out.extend_from_slice(pkt_type);
                out.extend_from_slice(body);
                out
            };
            let file_id = [1u8; 16];
            let mut desc = Vec::new();
            desc.extend_from_slice(&file_id);
            desc.extend_from_slice(&[0u8; 32]);
            desc.extend_from_slice(&(data.len() as u64).to_le_bytes());
            desc.extend_from_slice(b"f.bin\0\0\0");
            let mut ifsc = Vec::new();
            ifsc.extend_from_slice(&file_id);
            ifsc.extend_from_slice(&[0u8; 20]);

            let write_par = |block_size: u64| {
                let mut main_body = Vec::new();
                main_body.extend_from_slice(&block_size.to_le_bytes());
                main_body.extend_from_slice(&[0u8; 8]);
                let mut par = packet(b"PAR 2.0\0Main\0\0\0\0", &main_body);
                par.extend(packet(b"PAR 2.0\0FileDesc", &desc));
                par.extend(packet(b"PAR 2.0\0IFSC\0\0\0\0", &ifsc));
                std::fs::write(dir.join("f.par2"), par).unwrap();
            };

            write_par(100);
            let mismatches = borrowed_par2_mismatches(
                &f_bin,
                &dir,
                b"f.bin",
                &[ARTICLE_STATUS_FINISHED],
                &[1],
                &[0],
                &[0],
                &[100],
            );
            assert_eq!(mismatches, vec![0]);

            write_par(1u64 << 50);
            let mismatches = borrowed_par2_mismatches(
                &f_bin,
                &dir,
                b"f.bin",
                &[ARTICLE_STATUS_FINISHED],
                &[1],
                &[0],
                &[0],
                &[100],
            );
            assert!(mismatches.is_empty());

            let mismatches = borrowed_par2_mismatches(
                &f_bin,
                &dir,
                b"f.bin",
                &[ARTICLE_STATUS_FINISHED],
                &[1],
                &[0],
                &[400],
                &[100],
            );
            assert!(mismatches.is_empty());
            let _ = std::fs::remove_dir_all(&dir);
        }

        // 2. DupeArticleFallbackPar2CheckUsesTheFilesOwnSetTest
        {
            let dir =
                std::env::temp_dir().join(format!("nzbget-par2-sets-rs-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let data = vec![b'x'; 100];
            let f_bin = dir.join("f.bin");
            std::fs::write(&f_bin, &data).unwrap();

            let packet = |set_id: &[u8; 16], pkt_type: &[u8; 16], body: &[u8]| -> Vec<u8> {
                let mut out = Vec::from(&b"PAR2\0PKT"[..]);
                let length = (64 + body.len()) as u64;
                out.extend_from_slice(&length.to_le_bytes());
                out.extend_from_slice(&[0u8; 16]);
                out.extend_from_slice(set_id);
                out.extend_from_slice(pkt_type);
                out.extend_from_slice(body);
                out
            };
            let block_crc = crc32_ieee(&data);
            let file_id = [1u8; 16];
            let set_a = [b'A'; 16];
            let set_b = [b'B'; 16];

            let mut main_a = Vec::new();
            main_a.extend_from_slice(&100u64.to_le_bytes());
            main_a.extend_from_slice(&0u64.to_le_bytes());
            let mut desc_a = Vec::new();
            desc_a.extend_from_slice(&file_id);
            desc_a.extend_from_slice(&[0u8; 32]);
            desc_a.extend_from_slice(&(data.len() as u64).to_le_bytes());
            desc_a.extend_from_slice(b"f.bin\0\0\0");
            let mut ifsc_a = Vec::new();
            ifsc_a.extend_from_slice(&file_id);
            ifsc_a.extend_from_slice(&[0u8; 16]);
            ifsc_a.extend_from_slice(&block_crc.to_le_bytes());
            let mut par_a = packet(&set_a, b"PAR 2.0\0Main\0\0\0\0", &main_a);
            par_a.extend(packet(&set_a, b"PAR 2.0\0FileDesc", &desc_a));
            par_a.extend(packet(&set_a, b"PAR 2.0\0IFSC\0\0\0\0", &ifsc_a));

            let mut main_b = Vec::new();
            main_b.extend_from_slice(&200u64.to_le_bytes());
            main_b.extend_from_slice(&0u64.to_le_bytes());
            let mut desc_b = Vec::new();
            desc_b.extend_from_slice(&[2u8; 16]);
            desc_b.extend_from_slice(&[0u8; 32]);
            desc_b.extend_from_slice(&64u64.to_le_bytes());
            desc_b.extend_from_slice(b"g.bin\0\0\0");
            let mut ifsc_b = Vec::new();
            ifsc_b.extend_from_slice(&[2u8; 16]);
            ifsc_b.extend_from_slice(&[0u8; 20]);
            let mut par_b = packet(&set_b, b"PAR 2.0\0Main\0\0\0\0", &main_b);
            par_b.extend(packet(&set_b, b"PAR 2.0\0FileDesc", &desc_b));
            par_b.extend(packet(&set_b, b"PAR 2.0\0IFSC\0\0\0\0", &ifsc_b));

            for first_a in [true, false] {
                let mut combined = Vec::new();
                if first_a {
                    combined.extend_from_slice(&par_a);
                    combined.extend_from_slice(&par_b);
                } else {
                    combined.extend_from_slice(&par_b);
                    combined.extend_from_slice(&par_a);
                }
                std::fs::write(dir.join("set.par2"), combined).unwrap();
                let mismatches = borrowed_par2_mismatches(
                    &f_bin,
                    &dir,
                    b"f.bin",
                    &[ARTICLE_STATUS_FINISHED],
                    &[1],
                    &[0],
                    &[0],
                    &[100],
                );
                assert!(mismatches.is_empty());
            }
            let _ = std::fs::remove_dir_all(&dir);
        }

        // 3. DupeArticleFallbackProactiveTryIsNotUnsourcedTest
        for proactive in [true, false] {
            let (_check_log_par_defer, inc_unsourced, _restore_orig) =
                resolve_exhausted_sources(false, 0, proactive, 0, 0, true, false);
            assert_eq!(
                if inc_unsourced { 1 } else { 0 },
                if proactive { 0 } else { 1 }
            );
        }

        // 4. DupeArticleFallbackSizesMatchTest
        assert!(sizes_match(100000, 100000, 64));
        assert!(sizes_match(100000, 101000, 64));
        assert!(!sizes_match(100000, 103000, 64));
        assert!(sizes_match(100000, 105000, 16));
        assert!(!sizes_match(100000, 110000, 16));
        assert!(sizes_match(0, 0, 64));

        // 5. DupeArticleFallbackFilenameMatchTest
        {
            let target = TestFile::build(
                b"release.r01",
                &[(1, 500000), (2, 500000), (3, 120000)],
                b"orig",
            );
            let donors = vec![
                TestFile::build(
                    b"release.r00",
                    &[(1, 500000), (2, 500000), (3, 120000)],
                    b"donor0",
                ),
                TestFile::build(
                    b"Release.R01",
                    &[(1, 500200), (2, 500200), (3, 120100)],
                    b"donor1",
                ),
            ];
            let m = match_test_donor(&target, &donors, b"", b"", false, false).unwrap();
            assert_eq!(m.filename, b"Release.R01");
        }

        // 6. DupeArticleFallbackRefusesParTargetsTest
        for flagged in [false, true] {
            let filename: &[u8] = if flagged {
                b"obfuscated.bin"
            } else {
                b"release.vol01+02.PaR2"
            };
            let mut target = TestFile::build(filename, &[(1, 500000)], b"orig");
            target.par_file = flagged;
            target.filename_confirmed = !flagged;
            let donor = vec![TestFile::build(filename, &[(1, 500000)], b"foreign-par")];
            assert!(match_test_donor(&target, &donor, b"", b"", false, false).is_none());
            assert!(build_candidate_message_ids(&[donor], &target, 1).is_empty());
        }

        // 7. DupeArticleFallbackExcludesParDonorsTest
        for flagged in [false, true] {
            let target = TestFile::build(b"release.r01", &[(1, 500000)], b"orig");
            let mut parity = TestFile::build(
                if flagged {
                    b"release.r01"
                } else {
                    b"release.PAR2"
                },
                &[(1, 500000)],
                b"foreign-par",
            );
            parity.par_file = flagged;
            parity.filename_confirmed = !flagged;
            let mut donor = vec![parity];
            assert!(match_test_donor(&target, &donor, b"", b"", false, false).is_none());

            donor.push(TestFile::build(b"renamed.bin", &[(1, 500000)], b"data"));
            let candidates = build_candidate_message_ids(&[donor], &target, 1);
            assert_eq!(candidates, vec![b"data-1@example.com".to_vec()]);
        }

        // 8. DupeArticleFallbackStopsPinnedParSourcesTest
        for flagged in [false, true] {
            let mut target = TestFile::build(
                if flagged {
                    b"obfuscated.bin"
                } else {
                    b"release.PAR2"
                },
                &[(1, 500000)],
                b"orig",
            );
            target.par_file = flagged;
            target.filename_confirmed = !flagged;
            assert!(!check_try_preconditions(
                2,
                0,
                false,
                target.is_par(),
                target.first_content,
                1,
                0,
                1,
                &target.filename,
                false,
                false,
                false,
                0,
                0,
                0,
                2,
            ));
        }

        // 9. DupeArticleFallbackDefersDataToAvailableParTest
        for kind in 0..10 {
            let par_size = if kind == 0 || kind >= 6 { 500000 } else { 0 };
            let direct_rename_status = if kind == 5 { 1 } else { 0 };
            let par_status = match kind {
                6 => 3, // psSkipped
                7 => 4, // psRepairPossible
                8 => 1, // psSuccess
                9 => 5, // psManual
                _ => 0,
            };
            let header = should_defer_to_par_header(
                true,
                direct_rename_status,
                1,
                false,
                par_status,
                2, // psFailure
                par_size,
            );
            let should_defer = if header != 0 {
                header > 0
            } else if kind == 1 || kind == 3 {
                is_par_file(
                    kind == 1,
                    0,
                    1,
                    kind == 3,
                    if kind == 1 {
                        b"obfuscated.bin"
                    } else {
                        b"release.PAR2"
                    },
                )
            } else if kind == 2 || kind == 4 {
                completed_file_is_par2(
                    kind == 2,
                    if kind == 2 {
                        b"obfuscated.bin"
                    } else {
                        b"release.PAR2"
                    },
                )
            } else {
                false
            };
            assert!(should_defer, "fixture {kind} must defer to PAR");
            let (defer, _, _) = eval_par_defer(should_defer, 2, 0, 1, 2, false, false, false);
            assert!(defer);
            assert!(should_abort_deferred_try(defer, 1, false, true));
        }
        // First article of a file (part_number == 1) is never deferred
        {
            let (defer, _, _) = eval_par_defer(true, 1, 0, 1, 2, false, false, false);
            assert!(!defer);
        }
        // DirectRename running vs AllFirst vs ParSize
        assert_eq!(should_defer_to_par_header(true, 1, 1, false, 0, 2, 0), 1);
        assert_eq!(should_defer_to_par_header(true, 1, 1, true, 0, 2, 0), 0);
        assert_eq!(
            should_defer_to_par_header(true, 1, 1, true, 0, 2, 500000),
            1
        );

        // 10. DupeArticleFallbackAllowsDataWithoutUsableParTest
        for par_failed in [false, true] {
            let par_size = if par_failed { 500000 } else { 0 };
            let par_status = if par_failed { 2 } else { 0 };
            let has_par2 = par_size > 0;
            let should_defer =
                should_defer_to_par_header(true, 2, 1, false, par_status, 2, par_size) > 0;
            assert_eq!(has_par2 && !should_defer, par_failed);
        }

        // 11. DupeArticleFallbackLiftsDeferralWhenParCannotCoverTest
        for (failed, expect_fallback) in [
            (100000i64, false),
            (479999, false),
            (480001, true),
            (2000000, true),
        ] {
            let should_defer = should_defer_to_par_header(true, 2, 1, false, 0, 2, 500000) > 0;
            assert!(should_defer);
            let cannot_cover = par_cannot_cover(500000, 20000, failed + 20000, 0, 0, 0, 0, 0, true);
            assert_eq!(cannot_cover, expect_fallback);
            let (defer, lift, _) =
                eval_par_defer(should_defer, 2, 0, 1, 2, cannot_cover, false, false);
            assert_eq!(!defer, expect_fallback);
            assert_eq!(lift, expect_fallback);
        }

        // 12. DupeArticleFallbackArticleModePreservesRecoveryWithParTest
        for kind in 0..3 {
            let mut target = TestFile::build(
                match kind {
                    0 => b"release.r01",
                    1 => b"release.PAR2",
                    _ => b"obfuscated.bin",
                },
                &[(1, 500000)],
                b"orig",
            );
            target.par_file = kind == 2;
            target.filename_confirmed = kind != 2;
            let allowed = check_try_preconditions(
                1, // dafArticle
                0,
                false,
                target.is_par(),
                target.first_content,
                1,
                0,
                1,
                &target.filename,
                false,
                false,
                false,
                0,
                0,
                0,
                2,
            );
            assert_eq!(allowed, kind == 0);
        }

        // 13. DupeArticleFallbackAmbiguousFilenameMatchTest
        {
            let target = TestFile::build(b"release.r01", &[(1, 500000), (2, 500000)], b"orig");
            let donors = vec![
                TestFile::build(b"release.r01", &[(1, 500000), (2, 500000)], b"donor0"),
                TestFile::build(b"RELEASE.R01", &[(1, 500000), (2, 500000)], b"donor1"),
            ];
            assert!(match_test_donor(&target, &donors, b"", b"", false, false).is_none());
        }

        // 14. DupeArticleFallbackStructuralMatchTest
        {
            let target = TestFile::build(
                b"release.r01",
                &[(1, 500000), (2, 500000), (3, 120000)],
                b"orig",
            );
            let donors = vec![
                TestFile::build(
                    b"adf7e6b1.bin",
                    &[(1, 500100), (2, 500100), (3, 120050)],
                    b"donor0",
                ),
                TestFile::build(b"b23f98c4.bin", &[(1, 500000), (2, 300000)], b"donor1"),
            ];
            let m = match_test_donor(&target, &donors, b"", b"", false, false).unwrap();
            assert_eq!(m.filename, b"adf7e6b1.bin");
        }

        // 15. DupeArticleFallbackAmbiguousStructuralMatchTest
        {
            let target = TestFile::build(b"release.r01", &[(1, 500000), (2, 500000)], b"orig");
            let donors = vec![
                TestFile::build(b"adf7e6b1.bin", &[(1, 500000), (2, 500000)], b"donor0"),
                TestFile::build(b"b23f98c4.bin", &[(1, 500000), (2, 500000)], b"donor1"),
            ];
            assert!(match_test_donor(&target, &donors, b"", b"", false, false).is_none());
        }

        // 16. DupeArticleFallbackStepFingerprintMatchTest
        {
            const SIZES: [[i32; 10]; 4] = [
                [0; 10],
                [
                    769211, 770840, 768392, 771005, 769930, 768777, 770123, 769504, 771288, 412004,
                ],
                [
                    770318, 768951, 771402, 769037, 770660, 769845, 768590, 771113, 769722, 398760,
                ],
                [
                    768845, 771230, 769918, 770402, 768611, 771077, 769283, 770936, 768458, 405391,
                ],
            ];
            let volume = |seed: usize, shift: i32| -> Vec<(i32, i32)> {
                (1..=10)
                    .map(|i| (i as i32, SIZES[seed][i - 1] + shift))
                    .collect()
            };
            let target = TestFile::build(b"release.part02.rar", &volume(2, 0), b"orig");
            let mut donors = vec![
                TestFile::build(b"x9d0a1.bin", &volume(1, 31), b"donor1"),
                TestFile::build(b"q7c3e2.bin", &volume(2, 31), b"donor2"),
                TestFile::build(b"m2b8f4.bin", &volume(3, 31), b"donor3"),
            ];
            let m = match_test_donor(&target, &donors, b"", b"", false, false).unwrap();
            assert_eq!(m.filename, b"q7c3e2.bin");

            donors.push(TestFile::build(b"z1y2x3.bin", &volume(2, 77), b"donor4"));
            assert!(match_test_donor(&target, &donors, b"", b"", false, false).is_none());

            assert!(!article_size_steps_match(
                &[500000, 500100, 500050],
                &[500020, 500120, 500070]
            ));
            assert!(!article_size_steps_match(&[750000; 10], &[750000; 10]));
        }

        // 17. DupeArticleFallbackNzbFilenameMatchTest
        {
            let flat: Vec<(i32, i32)> = (1..=10).map(|i| (i, 716800)).collect();
            let target = TestFile::build(b"eywx5KxUK17tPqpAtlTI8cRgOZdSqZTV", &flat, b"orig");
            let donors = vec![
                TestFile::build(b"Release.part01.rar", &flat, b"donor1"),
                TestFile::build(b"Release.part02.rar", &flat, b"donor2"),
                TestFile::build(b"Release.part03.rar", &flat, b"donor3"),
            ];
            assert!(match_test_donor(&target, &donors, b"", b"", false, false).is_none());
            let m = match_test_donor(&target, &donors, b"Release.part02.rar", b"", false, false)
                .unwrap();
            assert_eq!(m.filename, b"Release.part02.rar");
        }

        // 18. DupeArticleFallbackPartCountMismatchTest
        {
            let target = TestFile::build(b"release.r01", &[(1, 500000), (2, 500000)], b"orig");
            let donors = vec![TestFile::build(
                b"release.r01",
                &[(1, 400000), (2, 400000), (3, 200000)],
                b"donor0",
            )];
            assert!(match_test_donor(&target, &donors, b"", b"", false, false).is_none());
        }

        // 19. DupeArticleFallbackTotalSizeMismatchTest
        {
            let target = TestFile::build(b"release.r01", &[(1, 500000), (2, 500000)], b"orig");
            let donors = vec![TestFile::build(
                b"release.r01",
                &[(1, 525000), (2, 525000)],
                b"donor0",
            )];
            assert!(match_test_donor(&target, &donors, b"", b"", false, false).is_none());
        }

        // 20. DupeArticleFallbackPartSizeMismatchTest
        {
            let target = TestFile::build(b"release.r01", &[(1, 100000), (2, 100000)], b"orig");
            let donors = vec![TestFile::build(
                b"release.r01",
                &[(1, 90000), (2, 110000)],
                b"donor0",
            )];
            assert!(match_test_donor(&target, &donors, b"", b"", false, false).is_none());
        }

        // 21. DupeArticleFallbackCandidateListStableAcrossRoundsTest
        {
            let mut target = TestFile::build(b"release.r01", &[(1, 500000), (2, 500000)], b"orig");
            let parsed_donors = vec![
                vec![TestFile::build(
                    b"release.r01",
                    &[(1, 500000), (2, 500000)],
                    b"donor0",
                )],
                vec![TestFile::build(
                    b"release.r01",
                    &[(1, 500100), (2, 500100)],
                    b"donor1",
                )],
            ];
            let round0 = build_candidate_message_ids(&parsed_donors, &target, 2);
            assert_eq!(
                round0,
                vec![
                    b"donor0-2@example.com".to_vec(),
                    b"donor1-2@example.com".to_vec()
                ]
            );
            target.articles[1].message_id = b"donor0-2@example.com".to_vec();
            let round1 = build_candidate_message_ids(&parsed_donors, &target, 2);
            assert_eq!(round1, round0);
        }

        // 22. DupeArticleFallbackCandidateListDedupeTest
        {
            let target = TestFile::build(b"release.r01", &[(1, 500000), (2, 500000)], b"orig");
            let parsed_donors = vec![
                vec![TestFile::build(
                    b"release.r01",
                    &[(1, 500000), (2, 500000)],
                    b"shared",
                )],
                vec![TestFile::build(
                    b"release.r01",
                    &[(1, 500000), (2, 500000)],
                    b"shared",
                )],
            ];
            let candidates = build_candidate_message_ids(&parsed_donors, &target, 1);
            assert_eq!(candidates, vec![b"shared-1@example.com".to_vec()]);
        }

        // 23–25. OrderSources (Reactive, Cutover, CutoverNoDonor)
        {
            let donors = vec![b"d0@example.com".to_vec(), b"d1@example.com".to_vec()];
            assert_eq!(
                order_sources(&donors, false, b"primary@example.com"),
                donors
            );
            let donors3 = vec![
                b"d0@example.com".to_vec(),
                b"d1@example.com".to_vec(),
                b"d2@example.com".to_vec(),
            ];
            assert_eq!(
                order_sources(&donors3, true, b"primary@example.com"),
                vec![
                    b"d0@example.com".to_vec(),
                    b"d1@example.com".to_vec(),
                    b"d2@example.com".to_vec(),
                    b"primary@example.com".to_vec(),
                ]
            );
            assert!(order_sources(&[], true, b"primary@example.com").is_empty());
        }

        // 26. DupeArticleFallbackRotateToLeadTest
        {
            let mut donors = vec![11, 21, 31];
            rotate_to_lead(&mut donors, 21, |&id| id);
            assert_eq!(donors, vec![21, 31, 11]);
            rotate_to_lead(&mut donors, 0, |&id| id);
            assert_eq!(donors, vec![21, 31, 11]);
            rotate_to_lead(&mut donors, 99, |&id| id);
            assert_eq!(donors, vec![21, 31, 11]);
        }

        // 27 & 28. FinishPinTest & FinishPinNoDonorsTest
        {
            let mut file = TestFile::build(b"release.r01", &[(1, 1000), (2, 1000)], b"orig");
            let candidates = vec![
                b"a1@example.com".to_vec(),
                b"b1@example.com".to_vec(),
                b"a2@example.com".to_vec(),
            ];
            let contributors = vec![11, 21, 11];
            file.finish_pin(0, &candidates, &contributors, false, b"primary@example.com");
            assert_eq!(file.articles[0].lead_snapshot, 11);
            assert_eq!(file.articles[0].next_lead, 21);
            assert_eq!(file.articles[0].donor_count, 2);
            assert_eq!(file.lead_donor_id, 11);
            assert_eq!(file.articles[0].sources, candidates);

            file.finish_pin(1, &candidates, &contributors, true, b"primary@example.com");
            assert_eq!(file.articles[1].sources.len(), 4);
            assert_eq!(file.articles[1].sources[3], b"primary@example.com");
            assert_eq!(file.lead_donor_id, 11);

            let mut file2 = TestFile::build(b"release.r01", &[(1, 1000)], b"orig");
            file2.finish_pin(0, &[], &[], true, b"primary@example.com");
            assert_eq!(file2.articles[0].lead_snapshot, 0);
            assert_eq!(file2.articles[0].next_lead, 0);
            assert_eq!(file2.articles[0].donor_count, 0);
            assert_eq!(file2.lead_donor_id, 0);
            assert!(file2.articles[0].sources.is_empty());
        }

        // Helper for FailedLeadArticle
        let mark_failed_lead =
            |file: &mut TestFile, idx: usize, next_lead: i32, donor_count: i32| {
                let lead = file.lead_donor_id;
                let a = &mut file.articles[idx];
                a.fallback_round = 1;
                a.lead_snapshot = lead;
                a.next_lead = next_lead;
                a.donor_count = donor_count;
            };

        // 29. DupeArticleFallbackLeadDemotionTest
        {
            let mut file = TestFile::build(
                b"release.r01",
                &[(1, 1000), (2, 1000), (3, 1000), (4, 1000)],
                b"orig",
            );
            file.lead_donor_id = 11;
            mark_failed_lead(&mut file, 0, 21, 2);
            assert!(!file.register_lead_failure(0));
            mark_failed_lead(&mut file, 1, 21, 2);
            assert!(!file.register_lead_failure(1));
            assert_eq!(file.lead_donor_id, 11);
            assert_eq!(file.lead_failures, 2);

            mark_failed_lead(&mut file, 2, 21, 2);
            file.articles[2].fallback_round = 2;
            assert!(!file.register_lead_failure(2));
            assert_eq!(file.lead_failures, 2);

            mark_failed_lead(&mut file, 3, 21, 2);
            assert!(file.register_lead_failure(3));
            assert_eq!(file.lead_donor_id, 21);
            assert_eq!(file.lead_failures, 0);
            assert_eq!(file.lead_switches, 1);
        }

        // 30. DupeArticleFallbackLeadSuccessResetsStreakTest
        {
            let mut file = TestFile::build(
                b"release.r01",
                &[(1, 1000), (2, 1000), (3, 1000), (4, 1000), (5, 1000)],
                b"orig",
            );
            file.lead_donor_id = 11;
            for i in 0..2 {
                mark_failed_lead(&mut file, i, 21, 2);
                assert!(!file.register_lead_failure(i));
            }
            mark_failed_lead(&mut file, 2, 21, 2);
            file.register_lead_success(2);
            assert_eq!(file.lead_failures, 0);
            for i in 3..5 {
                mark_failed_lead(&mut file, i, 21, 2);
                assert!(!file.register_lead_failure(i));
            }
            assert_eq!(file.lead_donor_id, 11);
        }

        // 31. DupeArticleFallbackLeadStaleSnapshotTest
        {
            let mut file = TestFile::build(
                b"release.r01",
                &[
                    (1, 1000),
                    (2, 1000),
                    (3, 1000),
                    (4, 1000),
                    (5, 1000),
                    (6, 1000),
                ],
                b"orig",
            );
            file.lead_donor_id = 11;
            mark_failed_lead(&mut file, 0, 21, 2);
            mark_failed_lead(&mut file, 1, 21, 2);
            mark_failed_lead(&mut file, 2, 21, 2);
            assert!(!file.register_lead_failure(2));
            mark_failed_lead(&mut file, 3, 21, 2);
            assert!(!file.register_lead_failure(3));
            mark_failed_lead(&mut file, 4, 21, 2);
            assert!(file.register_lead_failure(4));
            assert_eq!(file.lead_donor_id, 21);

            assert!(!file.register_lead_failure(0));
            assert!(!file.register_lead_failure(1));
            assert_eq!(file.lead_failures, 0);
            assert_eq!(file.lead_donor_id, 21);

            mark_failed_lead(&mut file, 5, 11, 2);
            assert!(!file.register_lead_failure(5));
            assert_eq!(file.lead_failures, 1);
            file.register_lead_success(0);
            assert_eq!(file.lead_failures, 1);
        }

        // 32. DupeArticleFallbackLeadSingleDonorNoDemotionTest
        {
            let mut file = TestFile::build(
                b"release.r01",
                &[(1, 1000), (2, 1000), (3, 1000), (4, 1000)],
                b"orig",
            );
            file.lead_donor_id = 11;
            for i in 0..3 {
                mark_failed_lead(&mut file, i, 0, 1);
                assert!(!file.register_lead_failure(i));
            }
            assert_eq!(file.lead_donor_id, 11);
            mark_failed_lead(&mut file, 3, 21, 2);
            assert!(file.register_lead_failure(3));
            assert_eq!(file.lead_donor_id, 21);
        }

        // 33. DupeArticleFallbackLeadRotationBoundedTest
        {
            let parts: Vec<(i32, i32)> = (1..=13).map(|i| (i, 1000)).collect();
            let mut file = TestFile::build(b"release.r01", &parts, b"orig");
            file.lead_donor_id = 11;
            let miss_streak = |f: &mut TestFile, first_idx: usize, next_lead: i32| -> bool {
                mark_failed_lead(f, first_idx, next_lead, 2);
                assert!(!f.register_lead_failure(first_idx));
                mark_failed_lead(f, first_idx + 1, next_lead, 2);
                assert!(!f.register_lead_failure(first_idx + 1));
                mark_failed_lead(f, first_idx + 2, next_lead, 2);
                f.register_lead_failure(first_idx + 2)
            };
            assert!(miss_streak(&mut file, 0, 21));
            assert_eq!(file.lead_donor_id, 21);
            assert!(miss_streak(&mut file, 3, 11));
            assert_eq!(file.lead_donor_id, 11);
            assert_eq!(file.lead_switches, 2);

            assert!(!miss_streak(&mut file, 6, 21));
            assert_eq!(file.lead_donor_id, 11);

            // Provisional success: resets failures, does NOT reset switches
            mark_failed_lead(&mut file, 9, 21, 2);
            file.register_lead_success(9);
            assert_eq!(file.lead_failures, 0);
            assert_eq!(file.lead_switches, 2);
            assert!(!miss_streak(&mut file, 10, 21));

            // Verifiable success (neighbours 8 and 10 finished): resets switches
            file.finish_article(8, 8000, 1000);
            file.finish_article(10, 10000, 1000);
            mark_failed_lead(&mut file, 9, 21, 2);
            file.register_lead_success(9);
            assert_eq!(file.lead_switches, 0);
            assert!(miss_streak(&mut file, 3, 21));
            assert_eq!(file.lead_donor_id, 21);
        }

        // 34. DupeArticleFallbackVacateGhostLeadTest
        {
            let mut file = TestFile::build(b"release.r01", &[(1, 1000)], b"orig");
            let donors = [11, 21];
            file.lead_donor_id = 21;
            file.lead_failures = 2;
            file.vacate_ghost_lead(false, &donors);
            assert_eq!(file.lead_donor_id, 21);
            assert_eq!(file.lead_failures, 2);

            file.lead_donor_id = 99;
            file.lead_switches = 2;
            file.vacate_ghost_lead(false, &donors);
            assert_eq!(file.lead_donor_id, 0);
            assert_eq!(file.lead_failures, 0);
            assert_eq!(file.lead_switches, 0);
        }

        // 35. DupeArticleFallbackFindDonorMessageIdTest
        {
            let donor = TestFile::build(b"release.r01", &[(1, 500000), (3, 120000)], b"donor");
            let pairs: Vec<(i32, &[u8])> = donor
                .articles
                .iter()
                .map(|a| (a.part_number, a.message_id.as_slice()))
                .collect();
            assert_eq!(
                find_donor_message_id(&pairs, 3),
                Some(&b"donor-3@example.com"[..])
            );
            assert_eq!(find_donor_message_id(&pairs, 2), None);
        }

        // 36. DupeArticleFallbackExpectedSegmentOffsetTest
        {
            let mut target =
                TestFile::build(b"release.r01", &[(1, 1300), (2, 1300), (3, 700)], b"orig");
            assert_eq!(target.expected_offset(0), 0);
            assert_eq!(target.expected_offset(1), -1);
            assert_eq!(target.expected_end(1), -1);
            assert_eq!(target.expected_end(2), -1);

            target.finish_article(0, 0, 1000);
            target.finish_article(2, 2000, 500);
            target.decoded_file_size = 2500;
            assert_eq!(target.expected_offset(1), 1000);
            assert_eq!(target.expected_end(1), 2000);
            assert_eq!(target.expected_end(2), 2500);
            assert_eq!(target.expected_offset(2), -1);
        }

        // 37. DupeArticleFallbackStructureGapTest (B30)
        {
            let full: Vec<(i32, i32)> = (1..=20)
                .map(|p| (p, if p == 20 { 70000 } else { 130000 }))
                .collect();
            let mut gap = full.clone();
            gap.remove(3); // part 4 missing
            let mut target = TestFile::build(b"release.r01", &gap, b"orig");
            target.size += 130000;

            let twin = vec![TestFile::build(b"other.r01", &full, b"donor")];
            assert!(match_test_donor(&target, &twin, b"", b"", false, false).is_some());

            let full_target = TestFile::build(b"release.r01", &full, b"orig");
            let lacking = vec![TestFile::build(b"other.r01", &gap, b"donor")];
            assert!(match_test_donor(&full_target, &lacking, b"", b"", false, false).is_none());

            let mut longer = full.clone();
            for p in 21..=30 {
                longer.push((p, 130000));
            }
            let other = vec![TestFile::build(b"other.r01", &longer, b"donor")];
            assert!(match_test_donor(&target, &other, b"", b"", false, false).is_none());

            let mut resized = full.clone();
            resized[10].1 = 60000;
            let different = vec![TestFile::build(b"other.r01", &resized, b"donor")];
            assert!(match_test_donor(&target, &different, b"", b"", false, false).is_none());
        }

        // 38. DupeArticleFallbackStructureGapStrictTest (B32)
        {
            let make_parts = |first: i32, last: i32, skip: &[i32]| -> Vec<(i32, i32)> {
                (first..=last)
                    .filter(|p| !skip.contains(p))
                    .map(|p| (p, 130000))
                    .collect()
            };
            let short160 = TestFile::build(b"release.r01", &make_parts(1, 160, &[]), b"orig");
            let longer = vec![TestFile::build(
                b"other.r01",
                &make_parts(1, 170, &[]),
                b"donor",
            )];
            assert!(match_test_donor(&short160, &longer, b"", b"", false, false).is_none());

            let mut gaps = TestFile::build(b"release.r01", &make_parts(1, 170, &[50, 51]), b"orig");
            gaps.size += 2 * 130000;
            assert!(match_test_donor(&gaps, &longer, b"", b"", false, false).is_some());

            let one_short = TestFile::build(b"release.r01", &make_parts(1, 169, &[]), b"orig");
            assert!(match_test_donor(&one_short, &longer, b"", b"", false, false).is_some());
            let two_short = TestFile::build(b"release.r01", &make_parts(1, 168, &[]), b"orig");
            assert!(match_test_donor(&two_short, &longer, b"", b"", false, false).is_none());
        }

        // 39. DupeArticleFallbackStructureDeclaredPartsTest (B39)
        {
            let make_parts =
                |last: i32| -> Vec<(i32, i32)> { (1..=last).map(|p| (p, 100000)).collect() };
            let donor_with = |last: i32, subject: &[u8]| -> Vec<TestFile> {
                let mut f = TestFile::build(b"Healthy.bin", &make_parts(last), b"donor");
                f.subject = subject.to_vec();
                vec![f]
            };
            let mut complete = TestFile::build(b"chB0.bin", &make_parts(29), b"orig");
            complete.subject = b"\"chB0.bin\" yEnc (1/29)".to_vec();
            let healthy = donor_with(30, b"\"Healthy.bin\" yEnc (1/30)");
            assert!(match_test_donor(&complete, &healthy, b"", b"", false, false).is_none());
            let unnamed = donor_with(30, b"abc123");
            assert!(match_test_donor(&complete, &unnamed, b"", b"", false, false).is_none());

            let mut lacking = TestFile::build(b"chB0.bin", &make_parts(29), b"orig");
            lacking.subject = b"\"chB0.bin\" yEnc (1/30)".to_vec();
            assert!(match_test_donor(&lacking, &healthy, b"", b"", false, false).is_some());

            let mut obfuscated = TestFile::build(b"chB0.bin", &make_parts(29), b"orig");
            obfuscated.subject = b"a1b2c3 [2019/2020]".to_vec();
            let plain = donor_with(30, b"d4e5f6");
            assert!(match_test_donor(&obfuscated, &plain, b"", b"", false, false).is_some());
        }

        // 40. DupeArticleFallbackExpectedSegmentGapTest (B3)
        {
            let mut target = TestFile::build(
                b"release.r01",
                &[(1, 1300), (2, 1300), (3, 1300), (5, 700)],
                b"orig",
            );
            target.finish_article(0, 0, 1000);
            target.finish_article(1, 1000, 1000);
            target.finish_article(2, 2000, 1000);
            assert_eq!(target.expected_offset(3), -1);
            target.finish_article(3, 4000, 500);
            assert_eq!(target.expected_end(2), -1);

            let no_first = TestFile::build(b"release.r01", &[(2, 1300), (3, 700)], b"orig");
            assert_eq!(no_first.expected_offset(0), -1);
        }

        // 41. DupeArticleFallbackFirstUntiledTest (B7)
        {
            let mut target =
                TestFile::build(b"release.r01", &[(1, 1300), (2, 1300), (3, 700)], b"orig");
            target.decoded_file_size = 2500;
            target.finish_article(0, 0, 1000);
            target.finish_article(2, 2000, 500);
            target.finish_article(1, 1000, 1000);
            assert_eq!(target.first_untiled(), None);

            target.finish_article(1, 1000, 960);
            assert_eq!(target.first_untiled(), Some(1));

            target.finish_article(1, 1000, 1040);
            target.articles[1].fallback_round = 1;
            assert_eq!(target.first_untiled(), Some(1));
        }

        // 42. DupeArticleFallbackSegmentAlignedTest
        {
            let mut target =
                TestFile::build(b"release.r01", &[(1, 1300), (2, 1300), (3, 700)], b"orig");
            target.finish_article(0, 0, 1000);
            target.finish_article(2, 2000, 500);
            target.decoded_file_size = 2500;

            target.finish_article(1, 1000, 1000);
            assert!(target.segment_aligned(1));
            target.finish_article(1, 1040, 1000);
            assert!(!target.segment_aligned(1));
            target.finish_article(1, 960, 1000);
            assert!(!target.segment_aligned(1));
            target.finish_article(1, 1000, 960);
            assert!(!target.segment_aligned(1));
        }

        // 43. DupeArticleFallbackSegmentAlignedUnknownNeighborsTest
        {
            let mut target =
                TestFile::build(b"release.r01", &[(1, 1300), (2, 1300), (3, 700)], b"orig");
            target.finish_article(1, 1040, 1000);
            assert!(target.segment_aligned(1));
            target.articles[1].segment_size = 0;
            assert!(target.segment_aligned(1));

            let mut target2 =
                TestFile::build(b"release.r01", &[(1, 1300), (2, 1300), (3, 700)], b"orig");
            target2.finish_article(0, 40, 1000);
            assert!(!target2.segment_aligned(0));
            target2.finish_article(0, 0, 1000);
            assert!(target2.segment_aligned(0));

            let mut target3 =
                TestFile::build(b"release.r01", &[(1, 1300), (2, 1300), (3, 700)], b"orig");
            target3.decoded_file_size = 2500;
            target3.finish_article(2, 1960, 500);
            assert!(!target3.segment_aligned(2));
            target3.finish_article(2, 2000, 500);
            assert!(target3.segment_aligned(2));
        }

        // 44. DupeArticleFallbackFirstUntiledArticleTest
        {
            let mut file =
                TestFile::build(b"release.r01", &[(1, 1300), (2, 1300), (3, 700)], b"orig");
            file.decoded_file_size = 2500;
            file.finish_article(0, 0, 1000);
            file.finish_article(1, 1000, 1000);
            file.finish_article(2, 2000, 500);
            assert_eq!(file.first_untiled(), None);

            file.finish_article(1, 1040, 1000);
            assert_eq!(file.articles[file.first_untiled().unwrap()].part_number, 2);

            file.finish_article(0, 40, 1000);
            file.finish_article(1, 1040, 1000);
            file.finish_article(2, 2040, 500);
            assert_eq!(file.articles[file.first_untiled().unwrap()].part_number, 1);

            file.finish_article(0, 0, 1000);
            file.finish_article(1, 1000, 1000);
            file.finish_article(2, 2000, 460);
            assert_eq!(file.articles[file.first_untiled().unwrap()].part_number, 3);

            file.decoded_file_size = 0;
            file.finish_article(0, 0, 1000);
            file.finish_article(1, 0, 1000);
            file.finish_article(2, 0, 500);
            assert_eq!(file.first_untiled(), None);

            let mut incomplete =
                TestFile::build(b"release.r01", &[(1, 1300), (2, 1300), (3, 700)], b"orig");
            incomplete.decoded_file_size = 2500;
            incomplete.finish_article(0, 0, 1000);
            incomplete.finish_article(2, 2000, 500);
            assert_eq!(incomplete.first_untiled(), None);
        }

        // 45 & 46 & 47. MergeDecodedFileSizeTest, ObfuscatedSizeNotUntiledTest, HasDonorArticlesTest
        {
            let mut file = TestFile::build(b"release.r01", &[(1, 1300), (2, 700)], b"orig");
            assert!(!file.merge_decoded_size(1500));
            assert_eq!(file.decoded_file_size, 1500);
            assert!(!file.merge_decoded_size(1500));
            assert_eq!(file.decoded_file_size, 1500);
            assert!(!file.merge_decoded_size(0));
            assert_eq!(file.decoded_file_size, 1500);

            let mut obf = TestFile::build(b"release.r01", &[(1, 1300), (2, 700)], b"orig");
            assert!(!obf.merge_decoded_size(9731448));
            assert!(obf.merge_decoded_size(10894771));
            assert_eq!(obf.decoded_file_size, -1);
            assert!(!obf.merge_decoded_size(9731448));
            assert!(!obf.merge_decoded_size(7548362));
            assert_eq!(obf.decoded_file_size, -1);

            let mut vol = TestFile::build(
                b"vol.part001.rar",
                &[(1, 1300), (2, 1300), (3, 700)],
                b"orig",
            );
            vol.merge_decoded_size(9731448);
            vol.finish_article(0, 0, 1000);
            vol.merge_decoded_size(10894771);
            vol.finish_article(1, 1000, 1000);
            vol.merge_decoded_size(7548362);
            vol.finish_article(2, 2000, 500);
            assert_eq!(vol.first_untiled(), None);
            assert_eq!(vol.expected_end(2), -1);
            assert!(!vol.has_donor_articles());
            vol.articles[1].fallback_round = 1;
            assert!(vol.has_donor_articles());
        }

        // 48 & 49. DupeArticleFallbackBorrowsFromOwnTwinWhileDeferringToParTest &
        //          DupeArticleFallbackBorrowsOnlyFromOwnTwinAcrossTwoTwinPairsTest
        {
            use md5::{Digest, Md5};
            let build_valid_par2_packet =
                |set_id: &[u8; 16], pkt_type: &[u8; 16], body: &[u8]| -> Vec<u8> {
                    let mut tail = Vec::with_capacity(32 + body.len());
                    tail.extend_from_slice(set_id);
                    tail.extend_from_slice(pkt_type);
                    tail.extend_from_slice(body);
                    let length = (32 + tail.len()) as u64;
                    let hash = Md5::digest(&tail);
                    let mut pkt = Vec::with_capacity(length as usize);
                    pkt.extend_from_slice(b"PAR2\0PKT");
                    pkt.extend_from_slice(&length.to_le_bytes());
                    pkt.extend_from_slice(&hash);
                    pkt.extend_from_slice(&tail);
                    pkt
                };
            let build_valid_par2_set = |specs: &[(&str, u64, u8)], block_size: u64| -> Vec<u8> {
                let set_id = [b'S'; 16];
                let mut main_body = Vec::new();
                main_body.extend_from_slice(&block_size.to_le_bytes());
                main_body.extend_from_slice(&(specs.len() as u32).to_le_bytes());
                let mut desc_packets = Vec::new();
                for (i, &(name, length, md5_fill)) in specs.iter().enumerate() {
                    let file_id = [b'A' + (i as u8); 16];
                    main_body.extend_from_slice(&file_id);
                    let mut padded = name.as_bytes().to_vec();
                    while !padded.len().is_multiple_of(4) {
                        padded.push(0);
                    }
                    let mut desc_body = Vec::new();
                    desc_body.extend_from_slice(&file_id);
                    desc_body.extend_from_slice(&[md5_fill; 16]);
                    desc_body.extend_from_slice(&[md5_fill; 16]);
                    desc_body.extend_from_slice(&length.to_le_bytes());
                    desc_body.extend_from_slice(&padded);
                    desc_packets.extend(build_valid_par2_packet(
                        &set_id,
                        b"PAR 2.0\0FileDesc",
                        &desc_body,
                    ));
                }
                let mut out =
                    build_valid_par2_packet(&set_id, b"PAR 2.0\0Main\0\0\0\0", &main_body);
                out.extend(desc_packets);
                out
            };

            let make_listed = |hash: u32, specs: &[(&str, u64, u8)]| -> crate::twin::Listed {
                let par2 = build_valid_par2_set(specs, 1000);
                let (sigs, complete) = crate::twin::par2_set(&par2);
                assert!(complete);
                let fp = crate::twin::fingerprint(&sigs);
                assert!(!fp.is_empty());
                crate::twin::Listed {
                    sigs,
                    complete,
                    print: format!("{fp}#{hash:x}"),
                }
            };

            // 48. BorrowsFromOwnTwinWhileDeferringToParTest
            let primary_listed = make_listed(
                1001,
                &[
                    ("vol1.rar", 1_000_000, b'1'),
                    ("vol2.rar", 1_000_000, b'2'),
                    ("vol3.rar", 1_000_000, b'3'),
                ],
            );
            let near_twin_listed = make_listed(
                1002,
                &[
                    ("vol1.rar", 1_000_000, b'1'),
                    ("vol2-par2only.rar", 1_000_000, b'2'),
                    ("vol3.rar", 1_000_000, b'9'),
                ],
            );
            let alt_listed = make_listed(
                1003,
                &[
                    ("vol1.rar", 1_000_000, b'1'),
                    ("vol2.rar", 1_000_000, b'8'),
                    ("vol3.rar", 1_000_000, b'7'),
                ],
            );

            let near_kind = crate::twin::kind(&primary_listed, &near_twin_listed);
            let alt_kind = crate::twin::kind(&primary_listed, &alt_listed);
            assert!(is_twin_kind(
                primary_listed.print.as_bytes(),
                near_twin_listed.print.as_bytes(),
                near_kind.as_bytes(),
            ));
            assert!(!is_twin_kind(
                primary_listed.print.as_bytes(),
                alt_listed.print.as_bytes(),
                alt_kind.as_bytes(),
            ));

            let near_donor_files = vec![
                TestFile::build(b"vol1.rar", &[(1, 500000), (2, 500000)], b"<a2-v1"),
                TestFile::build(b"vol2.rar", &[(1, 500000), (2, 500000)], b"<a2-v2"),
                TestFile::build(b"vol3.rar", &[(1, 500000), (2, 500000)], b"<a2-v3"),
            ];
            let alt_donor_files = vec![
                TestFile::build(b"vol1.rar", &[(1, 500000), (2, 500000)], b"<alt-v1"),
                TestFile::build(b"vol2.rar", &[(1, 500000), (2, 500000)], b"<alt-v2"),
                TestFile::build(b"vol3.rar", &[(1, 500000), (2, 500000)], b"<alt-v3"),
            ];

            let match_by_content_bytes = |own: &crate::twin::Listed,
                                          target_name: &str,
                                          donor: &crate::twin::Listed|
             -> Vec<u8> {
                match crate::twin::match_by_content(own, target_name, target_name, donor) {
                    Some(s) if s.is_empty() => b"-".to_vec(),
                    Some(s) => s.into_bytes(),
                    None => Vec::new(),
                }
            };

            // vol1.rar is shared with nearTwin -> borrows from nearTwin
            let vol1 = TestFile::build(b"vol1.rar", &[(1, 500000), (2, 500000)], b"a1-v1");
            let bc1 = match_by_content_bytes(&primary_listed, "vol1.rar", &near_twin_listed);
            let m1 =
                match_test_donor(&vol1, &near_donor_files, b"vol1.rar", &bc1, true, true).unwrap();
            assert_eq!(m1.filename, b"vol1.rar");

            // vol2.rar has PAR2 name "vol2-par2only.rar" in nearTwin (not in nearTwin NZB) -> twinOnly refuses fallback
            let vol2 = TestFile::build(b"vol2.rar", &[(1, 500000), (2, 500000)], b"a1-v2");
            let bc2 = match_by_content_bytes(&primary_listed, "vol2.rar", &near_twin_listed);
            assert_eq!(bc2, b"vol2-par2only.rar");
            assert!(
                match_test_donor(&vol2, &near_donor_files, b"vol2.rar", &bc2, true, true).is_none()
            );

            // vol3.rar differs in nearTwin ("-") and altDonor is not a twin (!twin_allowed) -> neither matches
            let vol3 = TestFile::build(b"vol3.rar", &[(1, 500000), (2, 500000)], b"a1-v3");
            let bc3_near = match_by_content_bytes(&primary_listed, "vol3.rar", &near_twin_listed);
            assert_eq!(bc3_near, b"-");
            assert!(
                match_test_donor(&vol3, &near_donor_files, b"vol3.rar", &bc3_near, true, true)
                    .is_none()
            );
            let bc3_alt = match_by_content_bytes(&primary_listed, "vol3.rar", &alt_listed);
            assert!(
                match_test_donor(&vol3, &alt_donor_files, b"vol3.rar", &bc3_alt, false, true)
                    .is_none()
            );

            // 49. BorrowsOnlyFromOwnTwinAcrossTwoTwinPairsTest
            let a1_listed = make_listed(
                2001,
                &[("vol1.rar", 1_000_000, b'a'), ("vol2.rar", 1_000_000, b'b')],
            );
            let a2_listed = make_listed(
                2002,
                &[("vol1.rar", 1_000_000, b'a'), ("vol2.rar", 1_000_000, b'b')],
            );
            let b1_listed = make_listed(
                2003,
                &[("vol1.rar", 1_000_000, b'k'), ("vol2.rar", 1_000_000, b'l')],
            );
            let b2_listed = make_listed(
                2004,
                &[("vol1.rar", 1_000_000, b'k'), ("vol2.rar", 1_000_000, b'l')],
            );

            // A1 sees only A2 as its twin (B1 and B2 are "alt" relative to A1)
            assert!(is_twin_kind(
                a1_listed.print.as_bytes(),
                a2_listed.print.as_bytes(),
                crate::twin::kind(&a1_listed, &a2_listed).as_bytes()
            ));
            assert!(!is_twin_kind(
                a1_listed.print.as_bytes(),
                b1_listed.print.as_bytes(),
                crate::twin::kind(&a1_listed, &b1_listed).as_bytes()
            ));
            assert!(!is_twin_kind(
                a1_listed.print.as_bytes(),
                b2_listed.print.as_bytes(),
                crate::twin::kind(&a1_listed, &b2_listed).as_bytes()
            ));

            // B1 sees only B2 as its twin (A1 and A2 are "alt" relative to B1)
            assert!(is_twin_kind(
                b1_listed.print.as_bytes(),
                b2_listed.print.as_bytes(),
                crate::twin::kind(&b1_listed, &b2_listed).as_bytes()
            ));
            assert!(!is_twin_kind(
                b1_listed.print.as_bytes(),
                a1_listed.print.as_bytes(),
                crate::twin::kind(&b1_listed, &a1_listed).as_bytes()
            ));
            assert!(!is_twin_kind(
                b1_listed.print.as_bytes(),
                a2_listed.print.as_bytes(),
                crate::twin::kind(&b1_listed, &a2_listed).as_bytes()
            ));
        }
    }
}
