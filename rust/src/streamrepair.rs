//! Rust port of hot-path stream repair and container content mapping logic:
//! - `DupeStreamRepair`: `IsStreamEligible`, `ComputeHoles`, `TotalSize`, `IntersectsAny`,
//!   `EstimateDonorRanges`, `SelectProbeParts`, `SubtractCovered`, `VolumeSetKey`,
//!   `DecodedSizePlausible`, `SuffixKey`, `VolumeKey`, `ExceedsDecompressCap`
//! - `ContentMap` & `ContentMapper`: `CompositeSource::ToMembers`, `HoledSource::Read` check,
//!   `ContentMap::MapToInner`, `ContentMap::MapFromInner`, `ContentMap::MapCipherRange`,
//!   `ContentMapper::GroupSets`, `ContentMapper::CoalesceRanges`,
//!   `ContentMapper::BuildZipMap`, `ContentMapper::BuildSevenZipMap`.

use crate::dupefallback::NzbgetRsByteRange;
use crate::releasename::lower as c_lower_bytes;

pub const MAX_DECOMPRESS_BYTES: i64 = 16 * 1024 * 1024 * 1024;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NzbgetRsContentRun {
    pub inner_offset: i64,
    pub member_index: i32,
    pub member_offset: i64,
    pub size: i64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NzbgetRsMemberRange {
    pub member_index: i32,
    pub offset: i64,
    pub size: i64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NzbgetRsGroupedSet {
    pub format: i32,
    pub member_start: usize,
    pub member_count: usize,
}

pub const FORMAT_BARE: i32 = 0;
pub const FORMAT_SPLIT: i32 = 1;
pub const FORMAT_RAR: i32 = 2;
pub const FORMAT_ZIP: i32 = 3;
pub const FORMAT_SEVEN_ZIP: i32 = 4;

fn base_file_name(path: &[u8]) -> &[u8] {
    let off = crate::paths::base_file_name(path);
    &path[off..]
}

#[inline]
fn range_end(r: NzbgetRsByteRange) -> i64 {
    r.offset.wrapping_add(r.size)
}

#[inline]
fn ranges_intersect(r1: NzbgetRsByteRange, r2: NzbgetRsByteRange) -> bool {
    r1.offset < range_end(r2) && r2.offset < range_end(r1)
}

/// `DupeStreamRepair::IsStreamEligible`: checks whether `filename` ends with a directly
/// posted media container extension.
pub fn is_stream_eligible(filename: &[u8]) -> bool {
    if filename.is_empty() {
        return false;
    }
    let Some(dot) = filename.iter().rposition(|&b| b == b'.') else {
        return false;
    };
    let ext = c_lower_bytes(&filename[dot..]);
    const MEDIA_EXTS: &[&[u8]] = &[
        b".mkv", b".mp4", b".m4v", b".avi", b".ts", b".mov", b".wmv", b".mpg", b".mpeg", b".webm",
    ];
    MEDIA_EXTS.iter().any(|&m| ext.as_slice() == m)
}

/// `DupeStreamRepair::ComputeHoles`: computes the missing `[offset, size)` ranges in `[0, file_size)`
/// given the finished article ranges.
pub fn compute_holes(covered: &[NzbgetRsByteRange], file_size: i64) -> Vec<NzbgetRsByteRange> {
    if file_size <= 0 {
        return Vec::new();
    }
    let mut sorted = covered.to_vec();
    sorted.sort_by_key(|r| r.offset);

    let mut holes = Vec::new();
    let mut pos: i64 = 0;
    for r in sorted {
        if r.offset > pos {
            holes.push(NzbgetRsByteRange {
                offset: pos,
                size: r.offset.wrapping_sub(pos),
            });
        }
        pos = pos.max(range_end(r));
    }
    if pos < file_size {
        holes.push(NzbgetRsByteRange {
            offset: pos,
            size: file_size.wrapping_sub(pos),
        });
    }
    holes
}

/// `DupeStreamRepair::TotalSize`: sums `size` across `ranges`.
pub fn total_size(ranges: &[NzbgetRsByteRange]) -> i64 {
    let mut total: i64 = 0;
    for r in ranges {
        total = total.wrapping_add(r.size);
    }
    total
}

/// `DupeStreamRepair::IntersectsAny`: returns `true` if `range` intersects any element of `ranges`.
pub fn intersects_any(range: NzbgetRsByteRange, ranges: &[NzbgetRsByteRange]) -> bool {
    ranges.iter().any(|&other| ranges_intersect(range, other))
}

/// `DupeStreamRepair::EstimateDonorRanges`: scales cumulative encoded article sizes into
/// `[0, decoded_file_size)`.
pub fn estimate_donor_ranges(
    article_sizes: &[i32],
    decoded_file_size: i64,
) -> Vec<NzbgetRsByteRange> {
    let total_encoded: i64 = article_sizes.iter().map(|&s| s as i64).sum();
    if total_encoded <= 0 || decoded_file_size <= 0 {
        return Vec::new();
    }

    let mut ranges = Vec::with_capacity(article_sizes.len());
    let mut cumulative: i64 = 0;
    let mut previous_end: i64 = 0;
    for &size in article_sizes {
        cumulative = cumulative.wrapping_add(size as i64);
        let mut end =
            ((cumulative as f64) / (total_encoded as f64) * (decoded_file_size as f64)) as i64;
        end = end.min(decoded_file_size).max(previous_end);
        ranges.push(NzbgetRsByteRange {
            offset: previous_end,
            size: end - previous_end,
        });
        previous_end = end;
    }
    if let Some(last) = ranges.last_mut() {
        last.size += decoded_file_size - previous_end;
    }
    ranges
}

/// `DupeStreamRepair::SelectProbeParts`: selects up to `probe_count` donor article indices
/// that do not intersect (and whose immediate neighbours do not intersect) `holes`.
pub fn select_probe_parts(
    donor_ranges: &[NzbgetRsByteRange],
    holes: &[NzbgetRsByteRange],
    probe_count: i32,
) -> Vec<i32> {
    if probe_count <= 0 || donor_ranges.is_empty() {
        return Vec::new();
    }

    fn classify(ranges: &[NzbgetRsByteRange]) -> (bool, bool) {
        let mut ordered = true;
        let mut previous_end: i64 = 0;
        for (i, r) in ranges.iter().enumerate() {
            if (r.size > 0 && r.offset > i64::MAX - r.size)
                || (r.size < 0 && r.offset < i64::MIN - r.size)
            {
                return (false, false);
            }
            if r.size <= 0 || (i > 0 && previous_end > r.offset) {
                ordered = false;
            }
            previous_end = range_end(*r);
        }
        (true, ordered)
    }

    let mut donor_props = (true, true);
    let mut hole_props = (true, true);
    if !holes.is_empty() {
        donor_props = classify(donor_ranges);
        if donor_props.0 {
            hole_props = classify(holes);
        }
    }
    let safe_ends = donor_props.0 && hole_props.0;

    let mut candidates = Vec::with_capacity(donor_ranges.len());
    if safe_ends {
        let mut intersects = vec![false; donor_ranges.len()];
        if donor_props.1 && hole_props.1 {
            let mut hole_idx = 0usize;
            for (i, &dr) in donor_ranges.iter().enumerate() {
                while hole_idx < holes.len() && range_end(holes[hole_idx]) <= dr.offset {
                    hole_idx += 1;
                }
                intersects[i] = hole_idx < holes.len() && ranges_intersect(dr, holes[hole_idx]);
            }
        } else {
            for (i, &dr) in donor_ranges.iter().enumerate() {
                intersects[i] = intersects_any(dr, holes);
            }
        }
        for i in 0..donor_ranges.len() {
            if !intersects[i]
                && (i == 0 || !intersects[i - 1])
                && (i + 1 == donor_ranges.len() || !intersects[i + 1])
            {
                candidates.push(i as i32);
            }
        }
    } else {
        for i in 0..donor_ranges.len() {
            let mut clear = true;
            let from = i.saturating_sub(1);
            let to = (i + 1).min(donor_ranges.len() - 1);
            for &dr in &donor_ranges[from..=to] {
                if !clear {
                    break;
                }
                clear = !intersects_any(dr, holes);
            }
            if clear {
                candidates.push(i as i32);
            }
        }
    }

    let wanted = probe_count as usize;
    if candidates.len() <= wanted {
        return candidates;
    }

    let mut probes = Vec::with_capacity(wanted);
    for k in 0..wanted {
        let idx = (candidates.len() as u128 * (2 * k + 1) as u128 / (2 * wanted) as u128) as usize;
        probes.push(candidates[idx]);
    }
    probes
}

/// `DupeStreamRepair::SubtractCovered`: subtracts `covered` from `ranges`.
pub fn subtract_covered(
    ranges: &[NzbgetRsByteRange],
    covered: NzbgetRsByteRange,
) -> Vec<NzbgetRsByteRange> {
    let mut remaining = Vec::with_capacity(ranges.len());
    let cov_end = range_end(covered);
    for &r in ranges {
        if !ranges_intersect(r, covered) {
            remaining.push(r);
            continue;
        }
        if r.offset < covered.offset {
            remaining.push(NzbgetRsByteRange {
                offset: r.offset,
                size: covered.offset.wrapping_sub(r.offset),
            });
        }
        let r_end = range_end(r);
        if cov_end < r_end {
            remaining.push(NzbgetRsByteRange {
                offset: cov_end,
                size: r_end.wrapping_sub(cov_end),
            });
        }
    }
    remaining
}

/// `DupeStreamRepair::VolumeSetKey`: returns the normalized archive volume set key
/// (`"rel.part#.rar"`, `"rel.r#"`, `"rel.z#"`, `"rel.7z.#"`, `"rel.#"`), or empty if not an archive volume.
pub fn volume_set_key(filename: &[u8]) -> Vec<u8> {
    if filename.is_empty() {
        return Vec::new();
    }
    let name: Vec<u8> = c_lower_bytes(base_file_name(filename));

    let digits_before = |end: usize| -> usize {
        let mut begin = end;
        while begin > 0 && name[begin - 1].is_ascii_digit() {
            begin -= 1;
        }
        begin
    };
    let ends_with = |tail: &[u8]| -> bool { name.len() > tail.len() && name.ends_with(tail) };

    if ends_with(b".rar") {
        let end = name.len() - 4;
        let begin = digits_before(end);
        if begin < end && begin >= 5 && &name[begin - 5..begin] == b".part" {
            let mut out = name[..begin].to_vec();
            out.extend_from_slice(b"#.rar");
            return out;
        }
        let mut out = name[..name.len() - 4].to_vec();
        out.extend_from_slice(b".r#");
        return out;
    }
    if ends_with(b".zip") {
        let mut out = name[..name.len() - 4].to_vec();
        out.extend_from_slice(b".z#");
        return out;
    }
    let begin = digits_before(name.len());
    if begin < name.len()
        && begin >= 2
        && name[begin - 1] != b'.'
        && name[begin - 2] == b'.'
        && (name[begin - 1] == b'r' || name[begin - 1] == b'z')
    {
        let mut out = name[..begin].to_vec();
        out.push(b'#');
        return out;
    }
    if name.len() - begin == 2
        && begin >= 2
        && name[begin - 2] == b'.'
        && (b's'..=b'y').contains(&name[begin - 1])
    {
        let mut out = name[..begin - 1].to_vec();
        out.extend_from_slice(b"r#");
        return out;
    }
    if begin < name.len() && name.len() - begin >= 3 && begin >= 1 && name[begin - 1] == b'.' {
        let mut out = name[..begin].to_vec();
        out.push(b'#');
        return out;
    }
    Vec::new()
}

/// `DupeStreamRepair::DecodedSizePlausible`: checks whether `decoded_file_size` is plausible for `encoded_size`.
pub fn decoded_size_plausible(decoded_file_size: i64, encoded_size: i64) -> bool {
    decoded_file_size > 0
        && encoded_size > 0
        && decoded_file_size <= encoded_size
        && decoded_file_size >= encoded_size - encoded_size / 8
}

/// `DupeStreamRepair::SuffixKey`: returns the last two dot-separated segments of `filename`, lowercased.
pub fn suffix_key(filename: &[u8]) -> Vec<u8> {
    if filename.is_empty() {
        return Vec::new();
    }
    let mut last_dot: Option<usize> = None;
    let mut prev_dot: Option<usize> = None;
    for (i, &b) in filename.iter().enumerate() {
        if b == b'.' {
            prev_dot = last_dot;
            last_dot = Some(i);
        }
    }
    let Some(ld) = last_dot else {
        return Vec::new();
    };
    let start = prev_dot.unwrap_or(ld) + 1;
    c_lower_bytes(&filename[start..])
}

/// `DupeStreamRepair::VolumeKey`: `suffix_key` with leading zeros of every digit run stripped.
pub fn volume_key(filename: &[u8]) -> Vec<u8> {
    let key = suffix_key(filename);
    let mut out = Vec::with_capacity(key.len());
    let mut i = 0;
    while i < key.len() {
        if !key[i].is_ascii_digit() {
            out.push(key[i]);
            i += 1;
            continue;
        }
        let mut end = i;
        while end < key.len() && key[end].is_ascii_digit() {
            end += 1;
        }
        let mut first = i;
        while first + 1 < end && key[first] == b'0' {
            first += 1;
        }
        out.extend_from_slice(&key[first..end]);
        i = end;
    }
    out
}

/// `DupeStreamRepair::ExceedsDecompressCap`.
pub fn exceeds_decompress_cap(
    total_bytes: i64,
    add_bytes: i64,
    total_extent: i64,
    add_extent: i64,
) -> bool {
    total_bytes.saturating_add(add_bytes) > MAX_DECOMPRESS_BYTES
        || total_extent.saturating_add(add_extent) > MAX_DECOMPRESS_BYTES
}

/// `CompositeSource::ToMembers`: maps a logical byte range across ordered members.
pub fn composite_to_members(
    member_indexes: &[i32],
    member_bases: &[i64],
    member_sizes: &[i64],
    logical_range: NzbgetRsByteRange,
) -> Vec<NzbgetRsMemberRange> {
    let mut pieces = Vec::new();
    let n = member_indexes
        .len()
        .min(member_bases.len())
        .min(member_sizes.len());
    let log_end = range_end(logical_range);
    for i in 0..n {
        let from = logical_range.offset.max(member_bases[i]);
        let to = log_end.min(member_bases[i].wrapping_add(member_sizes[i]));
        if from < to {
            pieces.push(NzbgetRsMemberRange {
                member_index: member_indexes[i],
                offset: from.wrapping_sub(member_bases[i]),
                size: to.wrapping_sub(from),
            });
        }
    }
    pieces
}

/// `HoledSource::Read` hole-intersection check: returns `true` if `[offset, offset + size)` intersects any hole.
pub fn holed_source_intersects(holes: &[NzbgetRsByteRange], offset: i64, size: i64) -> bool {
    let end = offset.wrapping_add(size);
    holes
        .iter()
        .any(|&h| offset < range_end(h) && h.offset < end)
}

/// `ContentMap::MapToInner`: maps a member range into inner stream coordinates.
pub fn content_map_to_inner(
    runs: &[NzbgetRsContentRun],
    member_index: i32,
    member_range: NzbgetRsByteRange,
) -> Vec<NzbgetRsByteRange> {
    let mut inner_ranges = Vec::new();
    let mem_end = range_end(member_range);
    for run in runs {
        if run.member_index != member_index {
            continue;
        }
        let from = member_range.offset.max(run.member_offset);
        let to = mem_end.min(run.member_offset.wrapping_add(run.size));
        if from < to {
            inner_ranges.push(NzbgetRsByteRange {
                offset: run
                    .inner_offset
                    .wrapping_add(from.wrapping_sub(run.member_offset)),
                size: to.wrapping_sub(from),
            });
        }
    }
    inner_ranges
}

/// `ContentMap::MapFromInner`: maps an inner stream range into member coordinates.
pub fn content_map_from_inner(
    runs: &[NzbgetRsContentRun],
    inner_range: NzbgetRsByteRange,
) -> Vec<NzbgetRsMemberRange> {
    let mut pieces = Vec::new();
    let in_end = range_end(inner_range);
    for run in runs {
        let from = inner_range.offset.max(run.inner_offset);
        let to = in_end.min(run.inner_offset.wrapping_add(run.size));
        if from < to {
            pieces.push(NzbgetRsMemberRange {
                member_index: run.member_index,
                offset: run
                    .member_offset
                    .wrapping_add(from.wrapping_sub(run.inner_offset)),
                size: to.wrapping_sub(from),
            });
        }
    }
    pieces
}

/// `ContentMap::MapCipherRange`: maps a contiguous cipher-space range into member coordinates,
/// failing closed (`Vec::new()`) on gaps, out-of-order runs, plaintext runs, or incomplete coverage.
pub fn content_map_cipher_range(
    runs: &[NzbgetRsContentRun],
    cipher_data_offsets: &[i64],
    cipher_sizes: &[i64],
    has_crypto: &[u8],
    cipher_range: NzbgetRsByteRange,
) -> Vec<NzbgetRsMemberRange> {
    if cipher_range.offset < 0
        || cipher_range.size <= 0
        || runs.len() != cipher_data_offsets.len()
        || runs.len() != cipher_sizes.len()
        || runs.len() != has_crypto.len()
    {
        return Vec::new();
    }
    let mut pieces = Vec::new();
    let mut covered: i64 = 0;
    let c_end = range_end(cipher_range);
    for i in 0..runs.len() {
        if has_crypto[i] == 0 {
            return Vec::new();
        }
        let slab_base = runs[i].inner_offset;
        let from = cipher_range.offset.max(slab_base);
        let to = c_end.min(slab_base.wrapping_add(cipher_sizes[i]));
        if from >= to {
            continue;
        }
        if from != cipher_range.offset.wrapping_add(covered) {
            return Vec::new();
        }
        pieces.push(NzbgetRsMemberRange {
            member_index: runs[i].member_index,
            offset: cipher_data_offsets[i].wrapping_add(from.wrapping_sub(slab_base)),
            size: to.wrapping_sub(from),
        });
        covered = covered.wrapping_add(to.wrapping_sub(from));
    }
    if covered != cipher_range.size {
        return Vec::new();
    }
    pieces
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scheme {
    None,
    RarNew,
    RarOld,
    RarOldFirst,
    ZipSpan,
    ZipFinal,
    SevenSplit,
    SevenSingle,
    Split,
}

fn all_digits(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_digit())
}

fn parse_c_atoi(bytes: &[u8]) -> i32 {
    let mut val: i64 = 0;
    for &b in bytes {
        if !b.is_ascii_digit() {
            break;
        }
        val = val.wrapping_mul(10).wrapping_add((b - b'0') as i64);
    }
    val as i32
}

fn parse_member_name(name: &[u8]) -> (Scheme, Vec<u8>, i32) {
    let lowered: Vec<u8> = c_lower_bytes(name);

    if lowered.ends_with(b".rar") {
        let stem = &lowered[..lowered.len() - 4];
        if let Some(part_pos) = stem.windows(5).rposition(|w| w == b".part") {
            let digits = &stem[part_pos + 5..];
            if all_digits(digits) {
                return (
                    Scheme::RarNew,
                    stem[..part_pos].to_vec(),
                    parse_c_atoi(digits),
                );
            }
        }
        return (Scheme::RarOldFirst, stem.to_vec(), 0);
    }

    let Some(dot_pos) = lowered.iter().rposition(|&b| b == b'.') else {
        return (Scheme::None, Vec::new(), 0);
    };
    if dot_pos == 0 {
        return (Scheme::None, Vec::new(), 0);
    }
    let extension = &lowered[dot_pos + 1..];
    let stem = &lowered[..dot_pos];

    if extension.len() == 3
        && (extension[0] == b'r' || extension[0] == b's')
        && all_digits(&extension[1..])
    {
        let vol = 1
            + parse_c_atoi(&extension[1..])
            + if extension[0] == b's' { 100 } else { 0 };
        return (Scheme::RarOld, stem.to_vec(), vol);
    }

    if extension == b"zip" {
        return (Scheme::ZipFinal, stem.to_vec(), 0);
    }
    if extension.len() >= 3 && extension[0] == b'z' && all_digits(&extension[1..]) {
        return (
            Scheme::ZipSpan,
            stem.to_vec(),
            parse_c_atoi(&extension[1..]),
        );
    }

    if extension == b"7z" {
        return (Scheme::SevenSingle, stem.to_vec(), 0);
    }
    if all_digits(extension) && (2..=4).contains(&extension.len()) {
        if stem.ends_with(b".7z") {
            return (
                Scheme::SevenSplit,
                stem[..stem.len() - 3].to_vec(),
                parse_c_atoi(extension),
            );
        }
        if stem.contains(&b'.') {
            return (Scheme::Split, stem.to_vec(), parse_c_atoi(extension));
        }
    }

    (Scheme::None, Vec::new(), 0)
}

/// `ContentMapper::GroupSets`: groups member filenames into container sets in data order.
/// Returns `(format, member_indices)` pairs sorted deterministically by each set's minimum member index.
pub fn content_mapper_group_sets(member_names: &[&[u8]]) -> Vec<(i32, Vec<i32>)> {
    struct SetBucket {
        format: i32,
        volumes: Vec<(i32, i32)>,
        has_final: bool,
        final_member: i32,
    }

    let mut buckets: Vec<(Vec<u8>, SetBucket)> = Vec::new();
    let bucket_for = |buckets: &mut Vec<(Vec<u8>, SetBucket)>, key: Vec<u8>, format: i32| -> usize {
        if let Some(pos) = buckets.iter().position(|(k, _)| *k == key) {
            return pos;
        }
        buckets.push((
            key,
            SetBucket {
                format,
                volumes: Vec::new(),
                has_final: false,
                final_member: -1,
            },
        ));
        buckets.len() - 1
    };

    let mut consumed = vec![false; member_names.len()];

    for (i, &name) in member_names.iter().enumerate() {
        let (scheme, base, volume) = parse_member_name(name);
        let idx = i as i32;
        match scheme {
            Scheme::RarNew => {
                let mut key = b"rarnew:".to_vec();
                key.extend_from_slice(&base);
                let b_idx = bucket_for(&mut buckets, key, FORMAT_RAR);
                buckets[b_idx].1.volumes.push((volume, idx));
                consumed[i] = true;
            }
            Scheme::RarOldFirst => {
                let mut key = b"rarold:".to_vec();
                key.extend_from_slice(&base);
                let b_idx = bucket_for(&mut buckets, key, FORMAT_RAR);
                buckets[b_idx].1.has_final = true;
                buckets[b_idx].1.volumes.push((0, idx));
                consumed[i] = true;
            }
            Scheme::RarOld => {
                let mut key = b"rarold:".to_vec();
                key.extend_from_slice(&base);
                let b_idx = bucket_for(&mut buckets, key, FORMAT_RAR);
                buckets[b_idx].1.volumes.push((volume, idx));
                consumed[i] = true;
            }
            Scheme::ZipSpan => {
                let mut key = b"zip:".to_vec();
                key.extend_from_slice(&base);
                let b_idx = bucket_for(&mut buckets, key, FORMAT_ZIP);
                buckets[b_idx].1.volumes.push((volume, idx));
                consumed[i] = true;
            }
            Scheme::ZipFinal => {
                let mut key = b"zip:".to_vec();
                key.extend_from_slice(&base);
                let b_idx = bucket_for(&mut buckets, key, FORMAT_ZIP);
                buckets[b_idx].1.has_final = true;
                buckets[b_idx].1.final_member = idx;
                consumed[i] = true;
            }
            Scheme::SevenSingle => {
                let mut key = b"7z:".to_vec();
                key.extend_from_slice(&base);
                let b_idx = bucket_for(&mut buckets, key, FORMAT_SEVEN_ZIP);
                buckets[b_idx].1.volumes.push((1, idx));
                consumed[i] = true;
            }
            Scheme::SevenSplit => {
                let mut key = b"7zsplit:".to_vec();
                key.extend_from_slice(&base);
                let b_idx = bucket_for(&mut buckets, key, FORMAT_SEVEN_ZIP);
                buckets[b_idx].1.volumes.push((volume, idx));
                consumed[i] = true;
            }
            Scheme::Split => {
                let mut key = b"split:".to_vec();
                key.extend_from_slice(&base);
                let b_idx = bucket_for(&mut buckets, key, FORMAT_SPLIT);
                buckets[b_idx].1.volumes.push((volume, idx));
                consumed[i] = true;
            }
            Scheme::None => {}
        }
    }

    let mut sets: Vec<(i32, Vec<i32>)> = Vec::new();

    for (key, mut bucket) in buckets {
        bucket.volumes.sort_unstable();

        let mut contiguous = true;
        for i in 1..bucket.volumes.len() {
            contiguous &= bucket.volumes[i].0 == bucket.volumes[i - 1].0.wrapping_add(1);
        }
        let mut complete = contiguous
            && !bucket.volumes.is_empty()
            && (bucket.format != FORMAT_RAR
                || if key.starts_with(b"rarold:") {
                    bucket.has_final && bucket.volumes[0].0 == 0
                } else {
                    bucket.volumes[0].0 == 1
                })
            && (bucket.format != FORMAT_SPLIT || bucket.volumes[0].0 == 1)
            && (bucket.format != FORMAT_SEVEN_ZIP || bucket.volumes[0].0 == 1);

        if bucket.format == FORMAT_ZIP {
            complete = bucket.has_final
                && (bucket.volumes.is_empty() || (contiguous && bucket.volumes[0].0 == 1));
        }

        if !complete {
            continue;
        }

        let mut members: Vec<i32> = bucket.volumes.into_iter().map(|(_, m)| m).collect();
        if bucket.format == FORMAT_ZIP {
            members.push(bucket.final_member);
        }
        sets.push((bucket.format, members));
    }

    for (i, &name) in member_names.iter().enumerate() {
        let base_name = base_file_name(name);
        if !consumed[i] && (is_stream_eligible(base_name) || !base_name.contains(&b'.')) {
            sets.push((FORMAT_BARE, vec![i as i32]));
        }
    }

    sets.sort_by_key(|(_, members)| members.iter().copied().min().unwrap_or(i32::MAX));
    sets
}

/// `ContentMapper::CoalesceRanges`: sorts and merges overlapping or adjacent `ranges`.
pub fn content_mapper_coalesce_ranges(ranges: &[NzbgetRsByteRange]) -> Vec<NzbgetRsByteRange> {
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|r| r.offset);

    let mut merged: Vec<NzbgetRsByteRange> = Vec::with_capacity(sorted.len());
    for r in sorted {
        if let Some(last) = merged.last_mut() {
            let last_end = range_end(*last);
            if r.offset <= last_end {
                let new_end = last_end.max(range_end(r));
                last.size = new_end.wrapping_sub(last.offset);
                continue;
            }
        }
        merged.push(r);
    }
    merged
}

#[inline]
fn get_le16(buf: &[u8]) -> u16 {
    u16::from_le_bytes([buf[0], buf[1]])
}

#[inline]
fn get_le32(buf: &[u8]) -> u32 {
    u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
}

#[inline]
fn get_le64(buf: &[u8]) -> u64 {
    u64::from_le_bytes([
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
    ])
}

/// `ContentMapper::BuildZipMap`: builds the inner-content map for a stored/spanned ZIP set.
/// Returns `Ok((inner_name, inner_size, runs))` or `Err(skip_reason)`.
pub fn content_mapper_build_zip_map<F>(
    member_indexes: &[i32],
    member_sizes: &[i64],
    mut read_member: F,
) -> Result<(Vec<u8>, i64, Vec<NzbgetRsContentRun>), &'static str>
where
    F: FnMut(i32, i64, &mut [u8]) -> bool,
{
    let n = member_indexes.len().min(member_sizes.len());
    let mut disk_bases = Vec::with_capacity(n);
    let mut total_size: i64 = 0;
    for &sz in &member_sizes[..n] {
        if sz <= 0 {
            return Err("member unreadable");
        }
        disk_bases.push(total_size);
        total_size = total_size.wrapping_add(sz);
    }

    let mut logical_read = |offset: i64, buf: &mut [u8]| -> bool {
        let size = buf.len() as i64;
        if offset < 0 || size < 0 || offset.saturating_add(size) > total_size {
            return false;
        }
        let pieces = composite_to_members(
            &member_indexes[..n],
            &disk_bases,
            &member_sizes[..n],
            NzbgetRsByteRange { offset, size },
        );
        let mut cursor = 0usize;
        for piece in pieces {
            let len = piece.size as usize;
            if !read_member(piece.member_index, piece.offset, &mut buf[cursor..cursor + len]) {
                return false;
            }
            cursor += len;
        }
        true
    };

    let window_size = total_size.min(66000) as usize;
    if window_size < 22 {
        return Err("archive directory unreadable");
    }
    let mut window = vec![0u8; window_size];
    if !logical_read(total_size - (window_size as i64), &mut window) {
        return Err("archive directory unreadable");
    }

    let mut eocd_pos: Option<usize> = None;
    for i in (0..=window_size - 22).rev() {
        if get_le32(&window[i..i + 4]) == 0x06054b50 {
            eocd_pos = Some(i);
            break;
        }
    }
    let Some(ep) = eocd_pos else {
        return Err("no zip end-of-directory record");
    };
    let eocd = &window[ep..];
    let mut entry_count = get_le16(&eocd[10..12]) as u64;
    let mut cd_disk = get_le16(&eocd[6..8]) as u32;
    let mut cd_offset = get_le32(&eocd[16..20]) as u64;

    if (cd_offset == 0xffff_ffff || entry_count == 0xffff)
        && ep >= 20
        && get_le32(&window[ep - 20..ep - 16]) == 0x07064b50
    {
        let eocd64_disk = get_le32(&window[ep - 16..ep - 12]) as usize;
        let eocd64_offset = get_le64(&window[ep - 12..ep - 4]);
        let mut eocd64 = [0u8; 56];
        if eocd64_disk >= disk_bases.len()
            || eocd64_offset > (total_size - disk_bases[eocd64_disk]) as u64
            || !logical_read(
                disk_bases[eocd64_disk] + (eocd64_offset as i64),
                &mut eocd64,
            )
            || get_le32(&eocd64[0..4]) != 0x06064b50
        {
            return Err("corrupt zip64 directory");
        }
        cd_disk = get_le32(&eocd64[20..24]);
        entry_count = get_le64(&eocd64[32..40]);
        cd_offset = get_le64(&eocd64[48..56]);
    }

    if (cd_disk as usize) >= disk_bases.len()
        || cd_offset > (total_size - disk_bases[cd_disk as usize]) as u64
    {
        return Err("corrupt zip directory");
    }
    let mut cd_pos = disk_bases[cd_disk as usize] + (cd_offset as i64);

    struct ZipEntry {
        name: Vec<u8>,
        comp_size: u64,
        uncomp_size: u64,
        local_offset: u64,
        disk: u32,
        method: u16,
        flags: u16,
    }
    let mut primary: Option<ZipEntry> = None;

    for _ in 0..entry_count {
        let mut fixed = [0u8; 46];
        if !logical_read(cd_pos, &mut fixed) || get_le32(&fixed[0..4]) != 0x02014b50 {
            return Err("corrupt zip directory");
        }
        let flags = get_le16(&fixed[8..10]);
        let method = get_le16(&fixed[10..12]);
        let mut comp_size = get_le32(&fixed[20..24]) as u64;
        let mut uncomp_size = get_le32(&fixed[24..28]) as u64;
        let name_len = get_le16(&fixed[28..30]) as usize;
        let extra_len = get_le16(&fixed[30..32]) as usize;
        let comment_len = get_le16(&fixed[32..34]) as usize;
        let mut disk = get_le16(&fixed[34..36]) as u32;
        let mut local_offset = get_le32(&fixed[42..46]) as u64;

        let mut name_buf = vec![0u8; name_len];
        if name_len > 0 && !logical_read(cd_pos + 46, &mut name_buf) {
            return Err("corrupt zip directory");
        }

        if extra_len > 0 {
            let mut extra = vec![0u8; extra_len];
            if !logical_read(cd_pos + 46 + (name_len as i64), &mut extra) {
                return Err("corrupt zip directory");
            }
            let (z64_uncomp, z64_comp) = zipview::zip64_sizes(
                &extra,
                uncomp_size.min(u32::MAX as u64) as u32,
                comp_size.min(u32::MAX as u64) as u32,
            );
            let mut field_pos = 0usize;
            while field_pos + 4 <= extra_len {
                let field_id = get_le16(&extra[field_pos..field_pos + 2]);
                let field_size = get_le16(&extra[field_pos + 2..field_pos + 4]) as usize;
                if field_id == 0x0001 {
                    let mut val_pos = field_pos + 4;
                    let field_end = (field_pos + 4 + field_size).min(extra_len);
                    if uncomp_size == 0xffff_ffff && val_pos + 8 <= field_end {
                        val_pos += 8;
                    }
                    if comp_size == 0xffff_ffff && val_pos + 8 <= field_end {
                        val_pos += 8;
                    }
                    if local_offset == 0xffff_ffff && val_pos + 8 <= field_end {
                        local_offset = get_le64(&extra[val_pos..val_pos + 8]);
                        val_pos += 8;
                    }
                    if disk == 0xffff && val_pos + 4 <= field_end {
                        disk = get_le32(&extra[val_pos..val_pos + 4]);
                    }
                }
                field_pos += 4 + field_size;
            }
            uncomp_size = z64_uncomp;
            comp_size = z64_comp;
        }

        cd_pos += 46 + (name_len as i64) + (extra_len as i64) + (comment_len as i64);

        let entry = ZipEntry {
            name: name_buf,
            comp_size,
            uncomp_size,
            local_offset,
            disk,
            method,
            flags,
        };
        if primary
            .as_ref()
            .is_none_or(|p| entry.uncomp_size > p.uncomp_size)
        {
            primary = Some(entry);
        }
    }

    let Some(prim) = primary else {
        return Err("empty zip directory");
    };

    let inner_name = match prim.name.iter().rposition(|&b| b == b'/') {
        Some(pos) => prim.name[pos + 1..].to_vec(),
        None => prim.name,
    };

    if prim.method != 0 {
        return Err("compressed zip entry (only stored maps)");
    }
    if (prim.flags & 0x0001) != 0 {
        return Err("encrypted zip entry");
    }
    if prim.comp_size != prim.uncomp_size {
        return Err("stored entry sizes disagree");
    }
    if !is_stream_eligible(&inner_name) {
        return Err("inner file is not a media file");
    }
    if (prim.disk as usize) >= disk_bases.len() {
        return Err("corrupt zip directory");
    }

    let local_pos = disk_bases[prim.disk as usize].wrapping_add(prim.local_offset as i64);
    let mut local = [0u8; 30];
    if !logical_read(local_pos, &mut local) || get_le32(&local[0..4]) != 0x04034b50 {
        return Err("corrupt zip local header");
    }
    let data_pos = local_pos
        + 30
        + (get_le16(&local[26..28]) as i64)
        + (get_le16(&local[28..30]) as i64);
    if data_pos > total_size || prim.uncomp_size > (total_size - data_pos) as u64 {
        return Err("implausible data run geometry");
    }

    let pieces = composite_to_members(
        &member_indexes[..n],
        &disk_bases,
        &member_sizes[..n],
        NzbgetRsByteRange {
            offset: data_pos,
            size: prim.uncomp_size as i64,
        },
    );
    let mut runs = Vec::with_capacity(pieces.len());
    let mut inner_offset: i64 = 0;
    for piece in pieces {
        runs.push(NzbgetRsContentRun {
            inner_offset,
            member_index: piece.member_index,
            member_offset: piece.offset,
            size: piece.size,
        });
        inner_offset += piece.size;
    }

    Ok((inner_name, prim.uncomp_size as i64, runs))
}

struct SevenZipReader<'a> {
    buf: &'a [u8],
    pos: usize,
    ok: bool,
}

impl<'a> SevenZipReader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self {
            buf,
            pos: 0,
            ok: true,
        }
    }

    fn read_byte(&mut self) -> u8 {
        if self.pos >= self.buf.len() {
            self.ok = false;
            return 0;
        }
        let b = self.buf[self.pos];
        self.pos += 1;
        b
    }

    fn read_number(&mut self) -> u64 {
        let first_byte = self.read_byte();
        let mut value: u64 = 0;
        let mut mask: u8 = 0x80;
        for i in 0..8 {
            if (first_byte & mask) == 0 {
                value |= ((first_byte & (mask - 1)) as u64) << (8 * i);
                return value;
            }
            value |= (self.read_byte() as u64) << (8 * i);
            mask >>= 1;
        }
        value
    }

    fn skip(&mut self, count: u64) {
        if count > (self.buf.len() - self.pos) as u64 {
            self.ok = false;
            return;
        }
        self.pos += count as usize;
    }

    fn read_bit_vector(&mut self, count: u64) -> Vec<bool> {
        if count > ((self.buf.len() - self.pos) as u64).saturating_mul(8) {
            self.ok = false;
            return Vec::new();
        }
        let mut bits = Vec::with_capacity(count as usize);
        let mut current_byte: u8 = 0;
        let mut mask: u8 = 0;
        for _ in 0..count {
            if mask == 0 {
                current_byte = self.read_byte();
                mask = 0x80;
            }
            bits.push((current_byte & mask) != 0);
            mask >>= 1;
        }
        bits
    }

    fn skip_digests(&mut self, count: u64) {
        let mut defined = count;
        if self.read_byte() == 0 {
            let bits = self.read_bit_vector(count);
            defined = bits.into_iter().filter(|&b| b).count() as u64;
        }
        self.skip(defined.saturating_mul(4));
    }
}

struct SevenZipStreams {
    pack_pos: u64,
    pack_sizes: Vec<u64>,
    folder_unpack_sizes: Vec<u64>,
    folder_crc_defined: Vec<bool>,
    substream_sizes: Vec<Vec<u64>>,
    copy_only: bool,
}

impl Default for SevenZipStreams {
    fn default() -> Self {
        Self {
            pack_pos: 0,
            pack_sizes: Vec::new(),
            folder_unpack_sizes: Vec::new(),
            folder_crc_defined: Vec::new(),
            substream_sizes: Vec::new(),
            copy_only: true,
        }
    }
}

fn parse_seven_zip_streams_info(
    reader: &mut SevenZipReader<'_>,
    streams: &mut SevenZipStreams,
) -> bool {
    let mut record_id = reader.read_number();

    if record_id == 0x06 {
        streams.pack_pos = reader.read_number();
        let pack_count = reader.read_number();
        loop {
            let id = reader.read_number();
            if !reader.ok || id == 0x00 {
                break;
            }
            if id == 0x09 {
                for _ in 0..pack_count {
                    if !reader.ok {
                        break;
                    }
                    streams.pack_sizes.push(reader.read_number());
                }
            } else if id == 0x0a {
                reader.skip_digests(pack_count);
            } else {
                return false;
            }
        }
        record_id = reader.read_number();
    }

    if record_id == 0x07 {
        if reader.read_number() != 0x0b {
            return false;
        }
        let folder_count = reader.read_number();
        if reader.read_byte() != 0 {
            return false;
        }
        for _ in 0..folder_count {
            if !reader.ok {
                break;
            }
            let coder_count = reader.read_number();
            streams.copy_only &= coder_count == 1;
            for _ in 0..coder_count {
                if !reader.ok {
                    break;
                }
                let flags = reader.read_byte();
                let id_size = flags & 0x0f;
                let mut copy_coder = id_size == 1 && (flags & 0x30) == 0;
                let mut coder_id: u8 = 0xff;
                for _ in 0..id_size {
                    coder_id = reader.read_byte();
                }
                copy_coder &= coder_count == 1 && coder_id == 0x00;
                streams.copy_only &= copy_coder;
                if (flags & 0x10) != 0 {
                    reader.read_number();
                    reader.read_number();
                }
                if (flags & 0x20) != 0 {
                    let attr_len = reader.read_number();
                    reader.skip(attr_len);
                }
            }
        }
        if reader.read_number() != 0x0c {
            return false;
        }
        for _ in 0..folder_count {
            if !reader.ok {
                break;
            }
            streams.folder_unpack_sizes.push(reader.read_number());
        }
        streams.folder_crc_defined = vec![false; folder_count as usize];
        loop {
            let id = reader.read_number();
            if !reader.ok || id == 0x00 {
                break;
            }
            if id == 0x0a {
                let mut defined = folder_count;
                if reader.read_byte() == 0 {
                    streams.folder_crc_defined = reader.read_bit_vector(folder_count);
                    defined = streams
                        .folder_crc_defined
                        .iter()
                        .filter(|&&b| b)
                        .count() as u64;
                } else {
                    streams.folder_crc_defined = vec![true; folder_count as usize];
                }
                reader.skip(defined.saturating_mul(4));
            } else {
                return false;
            }
        }
        record_id = reader.read_number();
    }

    for &unpack_size in &streams.folder_unpack_sizes {
        streams.substream_sizes.push(vec![unpack_size]);
    }

    if record_id == 0x08 {
        let mut stream_counts = vec![1u64; streams.folder_unpack_sizes.len()];
        let mut id = reader.read_number();
        if id == 0x0d {
            for count in &mut stream_counts {
                *count = reader.read_number();
            }
            id = reader.read_number();
        }
        if id == 0x09 {
            streams.substream_sizes.clear();
            for f in 0..stream_counts.len() {
                let mut sizes = Vec::new();
                let mut used: u64 = 0;
                let mut s = 0u64;
                while reader.ok && s + 1 < stream_counts[f] {
                    let sz = reader.read_number();
                    sizes.push(sz);
                    used = used.wrapping_add(sz);
                    s += 1;
                }
                if stream_counts[f] > 0 {
                    if used > streams.folder_unpack_sizes[f] {
                        return false;
                    }
                    sizes.push(streams.folder_unpack_sizes[f] - used);
                }
                streams.substream_sizes.push(sizes);
            }
            id = reader.read_number();
        } else if id != 0x00 && id != 0x0a {
            return false;
        } else {
            streams.substream_sizes.clear();
            for f in 0..stream_counts.len() {
                if stream_counts[f] != 1 {
                    return false;
                }
                streams
                    .substream_sizes
                    .push(vec![streams.folder_unpack_sizes[f]]);
            }
        }
        if id == 0x0a {
            let mut digest_count: u64 = 0;
            for f in 0..stream_counts.len() {
                let folder_covered = f < streams.folder_crc_defined.len()
                    && streams.folder_crc_defined[f]
                    && stream_counts[f] == 1;
                digest_count += if folder_covered { 0 } else { stream_counts[f] };
            }
            reader.skip_digests(digest_count);
            id = reader.read_number();
        }
        if id != 0x00 {
            return false;
        }
        record_id = reader.read_number();
    }

    reader.ok && record_id == 0x00
}

/// `ContentMapper::BuildSevenZipMap`: builds the inner-content map for a Copy-mode 7z set.
/// Returns `Ok((inner_name, inner_size, runs))` or `Err(skip_reason)`.
pub fn content_mapper_build_seven_zip_map<F>(
    member_indexes: &[i32],
    member_sizes: &[i64],
    mut read_member: F,
) -> Result<(Vec<u8>, i64, Vec<NzbgetRsContentRun>), &'static str>
where
    F: FnMut(i32, i64, &mut [u8]) -> bool,
{
    let n = member_indexes.len().min(member_sizes.len());
    let mut disk_bases = Vec::with_capacity(n);
    let mut total_size: i64 = 0;
    for &sz in &member_sizes[..n] {
        if sz <= 0 {
            return Err("member unreadable");
        }
        disk_bases.push(total_size);
        total_size = total_size.wrapping_add(sz);
    }

    let mut logical_read = |offset: i64, buf: &mut [u8]| -> bool {
        let size = buf.len() as i64;
        if offset < 0 || size < 0 || offset.saturating_add(size) > total_size {
            return false;
        }
        let pieces = composite_to_members(
            &member_indexes[..n],
            &disk_bases,
            &member_sizes[..n],
            NzbgetRsByteRange { offset, size },
        );
        let mut cursor = 0usize;
        for piece in pieces {
            let len = piece.size as usize;
            if !read_member(piece.member_index, piece.offset, &mut buf[cursor..cursor + len]) {
                return false;
            }
            cursor += len;
        }
        true
    };

    const SIGNATURE: &[u8; 6] = &[b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c];
    let mut sig_hdr = [0u8; 32];
    if total_size < 32
        || !logical_read(0, &mut sig_hdr)
        || &sig_hdr[0..6] != SIGNATURE
    {
        return Err("not a 7z archive");
    }
    let next_header_offset = get_le64(&sig_hdr[12..20]);
    let next_header_size = get_le64(&sig_hdr[20..28]);
    if next_header_size == 0
        || next_header_size > 16 * 1024 * 1024
        || 32u64
            .saturating_add(next_header_offset)
            .saturating_add(next_header_size)
            > (total_size as u64)
    {
        return Err("implausible 7z header");
    }

    let mut header_bytes = vec![0u8; next_header_size as usize];
    if !logical_read(32 + (next_header_offset as i64), &mut header_bytes) {
        return Err("archive header unreadable");
    }

    let mut unpacked_header = Vec::new();
    let mut reader = SevenZipReader::new(&header_bytes);
    let mut header_id = reader.read_number();

    if header_id == 0x17 {
        let mut header_streams = SevenZipStreams::default();
        if !parse_seven_zip_streams_info(&mut reader, &mut header_streams)
            || !header_streams.copy_only
            || header_streams.pack_sizes.len() != 1
        {
            return Err("compressed 7z header");
        }
        let pack_sz = header_streams.pack_sizes[0];
        if pack_sz == 0
            || pack_sz > 16 * 1024 * 1024
            || header_streams.pack_pos > (total_size as u64)
            || 32u64
                .saturating_add(header_streams.pack_pos)
                .saturating_add(pack_sz)
                > (total_size as u64)
        {
            return Err("implausible 7z header");
        }
        unpacked_header.resize(pack_sz as usize, 0);
        if !logical_read(32 + (header_streams.pack_pos as i64), &mut unpacked_header) {
            return Err("archive header unreadable");
        }
        reader = SevenZipReader::new(&unpacked_header);
        header_id = reader.read_number();
    }
    if header_id != 0x01 {
        return Err("unsupported 7z structure");
    }

    let mut streams = SevenZipStreams::default();
    let mut names: Vec<Vec<u8>> = Vec::new();
    let mut empty_stream: Vec<bool> = Vec::new();
    let mut file_count: u64 = 0;

    loop {
        let id = reader.read_number();
        if !reader.ok || id == 0x00 {
            break;
        }
        if id == 0x02 {
            loop {
                let prop_type = reader.read_number();
                if !reader.ok || prop_type == 0x00 {
                    break;
                }
                let sz = reader.read_number();
                reader.skip(sz);
            }
        } else if id == 0x04 {
            if !parse_seven_zip_streams_info(&mut reader, &mut streams) {
                return Err("unsupported 7z structure");
            }
        } else if id == 0x05 {
            file_count = reader.read_number();
            loop {
                let prop_type = reader.read_number();
                if !reader.ok || prop_type == 0x00 {
                    break;
                }
                let prop_size = reader.read_number();
                if prop_size > (reader.buf.len() - reader.pos) as u64 {
                    return Err("unsupported 7z structure");
                }
                let prop_end = reader.pos + (prop_size as usize);
                if prop_type == 0x0e {
                    empty_stream = reader.read_bit_vector(file_count);
                } else if prop_type == 0x11 {
                    if reader.read_byte() != 0 {
                        return Err("unsupported 7z structure");
                    }
                    let mut current = Vec::new();
                    while reader.ok && reader.pos + 1 < prop_end {
                        let low = reader.read_byte();
                        let high = reader.read_byte();
                        if low == 0 && high == 0 {
                            names.push(std::mem::take(&mut current));
                        } else {
                            current.push(if high == 0 && low < 128 { low } else { b'_' });
                        }
                    }
                }
                if reader.pos > prop_end {
                    return Err("unsupported 7z structure");
                }
                reader.pos = prop_end;
            }
        } else {
            return Err("unsupported 7z structure");
        }
    }

    if !reader.ok || streams.folder_unpack_sizes.is_empty() {
        return Err("unsupported 7z structure");
    }
    if !streams.copy_only {
        return Err("non-Copy 7z coder (only copy mode maps)");
    }
    if streams.pack_sizes.len() != streams.folder_unpack_sizes.len() {
        return Err("unsupported 7z structure");
    }
    for f in 0..streams.pack_sizes.len() {
        if streams.pack_sizes[f] != streams.folder_unpack_sizes[f] {
            return Err("7z pack/unpack sizes disagree (not copy mode?)");
        }
    }

    let mut pack_end = streams.pack_pos;
    if pack_end > (total_size as u64) - 32 {
        return Err("implausible data run geometry");
    }
    pack_end += 32;
    for &pack_size in &streams.pack_sizes {
        if pack_size > (total_size as u64) - pack_end {
            return Err("implausible data run geometry");
        }
        pack_end += pack_size;
    }

    struct SevenZipFile {
        name: Vec<u8>,
        data_pos: i64,
        size: i64,
    }
    let mut data_files = Vec::new();
    let pack_base = 32 + (streams.pack_pos as i64);
    let mut folder_pos = pack_base;
    for f in 0..streams.substream_sizes.len() {
        let mut file_pos = folder_pos;
        for &sub_sz in &streams.substream_sizes[f] {
            data_files.push(SevenZipFile {
                name: Vec::new(),
                data_pos: file_pos,
                size: sub_sz as i64,
            });
            file_pos += sub_sz as i64;
        }
        folder_pos += streams.pack_sizes[f] as i64;
    }

    let mut data_index = 0usize;
    let max_names = (file_count as usize).min(names.len());
    for i in 0..max_names {
        let has_stream =
            empty_stream.is_empty() || i >= empty_stream.len() || !empty_stream[i];
        if has_stream && data_index < data_files.len() {
            let base = match names[i].iter().rposition(|&b| b == b'/' || b == b'\\') {
                Some(pos) => names[i][pos + 1..].to_vec(),
                None => names[i].clone(),
            };
            data_files[data_index].name = base;
            data_index += 1;
        }
    }

    let mut primary: Option<&SevenZipFile> = None;
    for file in &data_files {
        if primary.is_none_or(|p| file.size > p.size) {
            primary = Some(file);
        }
    }
    let Some(prim) = primary else {
        return Err("unsupported 7z structure");
    };
    if prim.name.is_empty() {
        return Err("unsupported 7z structure");
    }
    if !is_stream_eligible(&prim.name) {
        return Err("inner file is not a media file");
    }
    if prim.data_pos.saturating_add(prim.size) > total_size {
        return Err("implausible data run geometry");
    }

    let pieces = composite_to_members(
        &member_indexes[..n],
        &disk_bases,
        &member_sizes[..n],
        NzbgetRsByteRange {
            offset: prim.data_pos,
            size: prim.size,
        },
    );
    let mut runs = Vec::with_capacity(pieces.len());
    let mut inner_offset: i64 = 0;
    for piece in pieces {
        runs.push(NzbgetRsContentRun {
            inner_offset,
            member_index: piece.member_index,
            member_offset: piece.offset,
            size: piece.size,
        });
        inner_offset += piece.size;
    }

    Ok((prim.name.clone(), prim.size, runs))
}

/// `ContentMap::ReadInnerDecrypted` (`ContentMap.cpp`): computes the 16-byte AES-CBC
/// block-aligned `[cipher_from, cipher_to)` bounds covering `[inner_offset, inner_offset + inner_size)`
/// and validates it against `inner_size_total` and `cipher_stream_size`.
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

/// `ContentMapper::IsCompressibleArchive` (`ContentMap.cpp`): returns `true` if `format` is one of
/// the compressible archive formats (`FORMAT_RAR`, `FORMAT_ZIP`, or `FORMAT_SEVEN_ZIP`).
#[inline]
pub fn content_mapper_is_compressible_archive(format: i32) -> bool {
    format == FORMAT_RAR || format == FORMAT_ZIP || format == FORMAT_SEVEN_ZIP
}

/// `ContentMapper::BuildRarMap` (`ContentMap.cpp`): classifies an unreadable RAR volume (`1` = encrypted
/// headers, `2` = unreadable headers in more than one volume, `0` = first unknown volume).
#[inline]
pub fn content_mapper_classify_unreadable_rar_vol(
    has_source: bool,
    is_encrypted: bool,
    unknown_volume: i32,
) -> i32 {
    if has_source && is_encrypted {
        1
    } else if unknown_volume >= 0 {
        2
    } else {
        0
    }
}

/// Formats the `DEBUG`-build RAR volume and file-entry diagnostic lines matching
/// `ContentMapDetail::LogDebugRarVolume`.
pub fn format_rar_volume_debug_lines(
    member_name: &[u8],
    vol: &crate::rarreader::Volume,
    files: &[crate::rarreader::RarFile],
) -> Vec<Vec<u8>> {
    let member_c = &member_name[..member_name
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(member_name.len())];
    let mut lines = Vec::with_capacity(1 + files.len());
    let mut vol_line = format!(
        "Volume: version:{}, multi:{}, vol-no:{}, new-naming:{}, has-next:{}, encrypted:{}, file-count:{}, [",
        vol.version,
        i32::from(vol.multi_volume),
        vol.volume_no as i32,
        i32::from(vol.new_naming),
        i32::from(vol.has_next_volume),
        i32::from(vol.encrypted),
        files.len() as i32,
    )
    .into_bytes();
    vol_line.extend_from_slice(base_file_name(member_c));
    vol_line.push(b']');
    lines.push(vol_line);

    for rf in files {
        let fn_c = &rf.filename[..rf
            .filename
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(rf.filename.len())];
        let mut file_line = format!(
            "  time:{}, size:{}, attr:{}, split-before:{}, split-after:{}, [",
            rf.time as i32,
            rf.size,
            rf.attr as i32,
            i32::from(rf.split_before),
            i32::from(rf.split_after),
        )
        .into_bytes();
        file_line.extend_from_slice(fn_c);
        file_line.push(b']');
        lines.push(file_line);
    }
    lines
}

// ============================================================================
// StreamRepairController Hot-Path Helpers
// ============================================================================

/// `StreamRepairController::BuildProbeWindows`: builds present-byte probe windows
/// hugging each current hole, clipped against `original_holes`, and coalesces overlaps.
pub fn build_probe_windows(
    inner_size: i64,
    holes: &[NzbgetRsByteRange],
    original_holes: &[NzbgetRsByteRange],
    probe_count: i32,
    min_probe_compare_bytes: i64,
) -> Vec<NzbgetRsByteRange> {
    let max_windows = (probe_count.max(0) as usize).saturating_mul(2);
    let mut windows = Vec::new();
    for &hole in holes {
        if windows.len() >= max_windows {
            break;
        }
        let hole_end = range_end(hole);
        let mut low_clip: i64 = 0;
        let mut high_clip: i64 = inner_size;
        for &other in original_holes {
            let other_end = range_end(other);
            if other.offset < hole.offset {
                low_clip = low_clip.max(other_end);
            }
            if other_end > hole_end {
                high_clip = high_clip.min(other.offset);
            }
        }
        let before_from = low_clip.max(hole.offset.saturating_sub(min_probe_compare_bytes));
        if before_from < hole.offset {
            windows.push(NzbgetRsByteRange {
                offset: before_from,
                size: hole.offset - before_from,
            });
        }
        let after_to = high_clip.min(hole_end.saturating_add(min_probe_compare_bytes));
        if hole_end < after_to {
            windows.push(NzbgetRsByteRange {
                offset: hole_end,
                size: after_to - hole_end,
            });
        }
    }
    content_mapper_coalesce_ranges(&windows)
}

/// `StreamRepairController::PlainCompareFloor`: computes the required compare floor
/// for a plaintext target set (`-1` if fewer than 64 bytes are reachable).
pub fn plain_compare_floor(
    runs: &[NzbgetRsContentRun],
    original_inner_holes: &[NzbgetRsByteRange],
    windows: &[NzbgetRsByteRange],
) -> i64 {
    let mut mapped_bytes: i64 = 0;
    for run in runs {
        mapped_bytes = mapped_bytes.wrapping_add(run.size);
    }
    let present = mapped_bytes.wrapping_sub(total_size(original_inner_holes));
    let base = present.clamp(64, 16 * 1024);
    let mut achievable: i64 = 0;
    for &window in windows {
        for piece in content_map_from_inner(runs, window) {
            achievable = achievable.wrapping_add(piece.size);
        }
    }
    if achievable < 64 {
        -1
    } else {
        base.min(achievable)
    }
}

/// `VerifyEncryptedFromSource` `chainStart`: finds the first 16-byte block in `[window.offset/block_size*block_size, window_end)`
/// whose predecessor block is outside `original_holes` (or block 0).
pub fn encrypted_chain_start(
    window_offset: i64,
    window_end: i64,
    block_size: i64,
    original_holes: &[NzbgetRsByteRange],
) -> i64 {
    if block_size <= 0 {
        return window_offset;
    }
    let mut block_from = (window_offset / block_size) * block_size;
    while block_from > 0 && block_from < window_end {
        let mut prev_present = true;
        for &hole in original_holes {
            let hole_end = range_end(hole);
            prev_present &= hole_end <= block_from - block_size || hole.offset >= block_from;
        }
        if prev_present {
            break;
        }
        block_from += block_size;
    }
    block_from
}

/// Checks whether `data` contains at least two distinct byte values (non-constant run).
pub fn buffer_has_varied_data(data: &[u8]) -> bool {
    if data.len() < 2 {
        return false;
    }
    let first = data[0];
    data[1..].iter().any(|&b| b != first)
}

fn locale_casecmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let la = c_lower_bytes(a);
    let lb = c_lower_bytes(b);
    for (&ca, &cb) in la.iter().zip(lb.iter()) {
        if ca != cb {
            return ca.cmp(&cb);
        }
    }
    la.len().cmp(&lb.len())
}

/// `StreamRepairController::ComputePositionalRanks` per-target rank and window size.
pub fn compute_positional_rank(
    member_names: &[&[u8]],
    member_sizes: &[i64],
    target_filename: &[u8],
    target_decoded_size: i64,
) -> (i32, i32) {
    let mut rank: i32 = 0;
    let mut window_size: i32 = 0;
    let mut self_seen = false;
    let n = member_names.len().min(member_sizes.len());
    for i in 0..n {
        let sz = member_sizes[i];
        if sz <= 0 || !crate::dupefallback::sizes_match(sz, target_decoded_size, 8) {
            continue;
        }
        window_size += 1;
        let ord = locale_casecmp(member_names[i], target_filename);
        if ord == std::cmp::Ordering::Equal {
            self_seen = true;
            continue;
        }
        if ord == std::cmp::Ordering::Less {
            rank += 1;
        }
    }
    if self_seen {
        (rank, window_size)
    } else {
        (-1, 0)
    }
}

fn ends_with_par2_ci(filename: &[u8]) -> bool {
    filename.len() >= 5 && c_lower_bytes(&filename[filename.len() - 5..]) == b".par2"
}

/// `DupeStreamRepair::SelectWholeFileDonor`: returns the unique matching donor index (`-1` if none).
#[allow(clippy::too_many_arguments)]
pub fn select_whole_file_donor(
    target_filename: &[u8],
    target_encoded_size: i64,
    target_article_count: i32,
    target_steps_hash: u64,
    donor_set: &[u8],
    donor_filenames: &[&[u8]],
    donor_sizes: &[i64],
    donor_is_par: &[u8],
    donor_has_articles: &[u8],
    donor_is_claimed: &[u8],
    donor_steps_hashes: &[u64],
) -> i32 {
    if target_filename.is_empty()
        || ends_with_par2_ci(target_filename)
        || target_encoded_size <= 0
        || target_article_count <= 0
    {
        return -1;
    }
    let n = donor_filenames
        .len()
        .min(donor_sizes.len())
        .min(donor_is_par.len())
        .min(donor_has_articles.len())
        .min(donor_is_claimed.len())
        .min(donor_steps_hashes.len());
    let mut pool = Vec::new();
    for i in 0..n {
        if donor_is_par[i] == 0
            && !ends_with_par2_ci(donor_filenames[i])
            && donor_has_articles[i] != 0
            && donor_is_claimed[i] == 0
            && (donor_set.is_empty() || volume_set_key(donor_filenames[i]) == donor_set)
            && crate::dupefallback::sizes_match(donor_sizes[i], target_encoded_size, 64)
        {
            pool.push(i);
        }
    }
    let unique = |pred: &dyn Fn(usize) -> bool| -> Option<usize> {
        let mut found = None;
        for &idx in &pool {
            if pred(idx) {
                if found.is_some() {
                    return None;
                }
                found = Some(idx);
            }
        }
        found
    };
    if target_steps_hash != 0 {
        if let Some(idx) = unique(&|i| donor_steps_hashes[i] == target_steps_hash) {
            return idx as i32;
        }
    }
    if let Some(idx) =
        unique(&|i| locale_casecmp(donor_filenames[i], target_filename) == std::cmp::Ordering::Equal)
    {
        return idx as i32;
    }
    let target_key = volume_key(target_filename);
    if !target_key.is_empty() {
        if let Some(idx) = unique(&|i| volume_key(donor_filenames[i]) == target_key) {
            return idx as i32;
        }
    }
    -1
}

/// `DupeStreamRepair::SelectDonorCandidates`: returns ordered candidate donor indices.
#[allow(clippy::too_many_arguments)]
pub fn select_donor_candidates(
    target_filename: &[u8],
    target_decoded_file_size: i64,
    positional_rank: i32,
    positional_window: i32,
    max_candidates: i32,
    target_steps_hash: u64,
    donor_filenames: &[&[u8]],
    donor_sizes: &[i64],
    donor_is_par: &[u8],
    donor_has_articles: &[u8],
    donor_is_claimed: &[u8],
    donor_steps_hashes: &[u64],
) -> Vec<i32> {
    if target_filename.is_empty() || ends_with_par2_ci(target_filename) || max_candidates <= 0 {
        return Vec::new();
    }
    let n = donor_filenames
        .len()
        .min(donor_sizes.len())
        .min(donor_is_par.len())
        .min(donor_has_articles.len())
        .min(donor_is_claimed.len())
        .min(donor_steps_hashes.len());
    let mut window = Vec::new();
    for i in 0..n {
        if donor_is_par[i] == 0
            && !ends_with_par2_ci(donor_filenames[i])
            && donor_has_articles[i] != 0
            && crate::dupefallback::sizes_match(donor_sizes[i], target_decoded_file_size, 8)
        {
            window.push(i);
        }
    }
    let mut candidates: Vec<i32> = Vec::new();
    let mut add = |idx: usize| {
        if (candidates.len() as i32) < max_candidates
            && donor_is_claimed[idx] == 0
            && !candidates.contains(&(idx as i32))
        {
            candidates.push(idx as i32);
        }
    };

    if target_steps_hash != 0 {
        let mut step_match: Option<usize> = None;
        let mut ambiguous = false;
        for i in 0..n {
            if donor_is_par[i] == 0
                && !ends_with_par2_ci(donor_filenames[i])
                && donor_steps_hashes[i] == target_steps_hash
            {
                ambiguous = step_match.is_some();
                step_match = Some(i);
            }
        }
        if let Some(idx) = step_match {
            if !ambiguous {
                add(idx);
            }
        }
    }

    for &idx in &window {
        if locale_casecmp(donor_filenames[idx], target_filename) == std::cmp::Ordering::Equal {
            add(idx);
        }
    }

    let target_key = volume_key(target_filename);
    if !target_key.is_empty() {
        let mut key_match: Option<usize> = None;
        let mut ambiguous = false;
        for &idx in &window {
            if volume_key(donor_filenames[idx]) == target_key {
                ambiguous = key_match.is_some();
                key_match = Some(idx);
            }
        }
        if let Some(idx) = key_match {
            if !ambiguous {
                add(idx);
            }
        }
    }

    if positional_rank >= 0
        && positional_window >= 2
        && (window.len() as i32) == positional_window
        && (positional_rank as usize) < window.len()
    {
        let mut by_name = window.clone();
        by_name.sort_by(|&a, &b| locale_casecmp(donor_filenames[a], donor_filenames[b]));
        add(by_name[positional_rank as usize]);
    }

    let mut by_size = window;
    by_size.sort_by_key(|&idx| {
        let sz = donor_sizes[idx];
        if sz >= target_decoded_file_size {
            sz.wrapping_sub(target_decoded_file_size)
        } else {
            target_decoded_file_size.wrapping_sub(sz)
        }
    });
    for idx in by_size {
        add(idx);
    }

    candidates
}

/// `StreamRepairController::ProveDonorOnIntactFile` candidate selection.
#[allow(clippy::too_many_arguments)]
pub fn select_intact_proof_candidates(
    intact_basename: &[u8],
    intact_size: i64,
    donor_set: &[u8],
    stand_in_index: i32,
    max_candidates: i32,
    donor_filenames: &[&[u8]],
    donor_sizes: &[i64],
    donor_has_articles: &[u8],
    donor_is_claimed: &[u8],
) -> Vec<i32> {
    if max_candidates <= 0 {
        return Vec::new();
    }
    let n = donor_filenames
        .len()
        .min(donor_sizes.len())
        .min(donor_has_articles.len())
        .min(donor_is_claimed.len());
    let vol_key = volume_key(intact_basename);
    let mut candidates: Vec<i32> = Vec::new();
    for pass in 0..2 {
        for i in 0..n {
            if (candidates.len() as i32) >= max_candidates {
                break;
            }
            if (i as i32) == stand_in_index
                || donor_is_claimed[i] != 0
                || donor_has_articles[i] == 0
                || volume_set_key(donor_filenames[i]) != donor_set
                || !crate::dupefallback::sizes_match(donor_sizes[i], intact_size, 8)
                || candidates.contains(&(i as i32))
            {
                continue;
            }
            let same_number = volume_key(donor_filenames[i]) == vol_key;
            if (pass == 0 && same_number) || (pass == 1 && !same_number) {
                candidates.push(i as i32);
            }
        }
    }
    candidates
}

/// `StreamRepairController::VerifyDonor` probe and reserve selection.
pub fn select_verify_donor_probes(
    donor_ranges: &[NzbgetRsByteRange],
    holes: &[NzbgetRsByteRange],
    decoded_file_size: i64,
    probe_count: i32,
) -> (Vec<i32>, Vec<i32>) {
    let mut probe_parts = select_probe_parts(donor_ranges, holes, probe_count);
    let mut reserve = Vec::new();
    if !probe_parts.is_empty() {
        for part_index in select_probe_parts(donor_ranges, holes, donor_ranges.len() as i32) {
            if !probe_parts.contains(&part_index) {
                reserve.push(part_index);
            }
        }
    } else if !donor_ranges.is_empty() && !holes.is_empty() {
        let donors: Vec<crate::streampatchparts::Range> = donor_ranges
            .iter()
            .map(|r| crate::streampatchparts::Range {
                offset: r.offset,
                size: r.size,
            })
            .collect();
        let hole_ranges: Vec<crate::streampatchparts::Range> = holes
            .iter()
            .map(|r| crate::streampatchparts::Range {
                offset: r.offset,
                size: r.size,
            })
            .collect();
        let mut pool = vec![0i32; donors.len()];
        let count = crate::streampatchparts::select(&donors, &hole_ranges, 1, &mut pool);
        pool.truncate(count);
        let mut overlaps: Vec<(i64, i32)> = Vec::new();
        for part_index in pool {
            if part_index < 0 || (part_index as usize) >= donor_ranges.len() {
                continue;
            }
            let mut present = vec![donor_ranges[part_index as usize]];
            for &hole in holes {
                present = subtract_covered(&present, hole);
            }
            let overlap = total_size(&present);
            if overlap > 0 {
                overlaps.push((overlap, part_index));
            }
        }
        overlaps.sort_by(|a, b| b.0.cmp(&a.0));
        let base = (decoded_file_size.wrapping_sub(total_size(holes))).clamp(64, 16 * 1024);
        let mut pooled: i64 = 0;
        for (overlap, part_index) in overlaps {
            let pc = probe_parts.len() as i32;
            let pool_stop = pc >= probe_count
                && (pooled >= base || pc >= probe_count.saturating_mul(2));
            if pool_stop {
                reserve.push(part_index);
            } else {
                probe_parts.push(part_index);
                pooled = pooled.wrapping_add(overlap);
            }
        }
    }
    (probe_parts, reserve)
}

/// `StreamRepairController::VerifyEncryptedFromSource` compare floor (`-1` if `< 64` bytes reachable).
pub fn encrypted_compare_floor(
    inner_size: i64,
    block_size: i64,
    runs: &[NzbgetRsContentRun],
    original_holes: &[NzbgetRsByteRange],
    windows: &[NzbgetRsByteRange],
) -> i64 {
    if block_size <= 0 {
        return -1;
    }
    let last_computable = (inner_size / block_size) * block_size;
    let mut mapped_bytes: i64 = 0;
    for run in runs {
        mapped_bytes = mapped_bytes.wrapping_add(run.size);
    }
    let base = (mapped_bytes.wrapping_sub(total_size(original_holes))).clamp(64, 16 * 1024);
    let mut achievable: i64 = 0;
    for &window in windows {
        let window_end = range_end(window).min(last_computable);
        let block_from =
            encrypted_chain_start(window.offset, window_end, block_size, original_holes);
        let from = window.offset.max(block_from);
        if window_end > from {
            achievable = achievable.wrapping_add(window_end - from);
        }
    }
    if achievable < 64 {
        -1
    } else {
        base.min(achievable)
    }
}

/// `StreamRepairController::PatchEncryptedFromSource` chunk hole/present partitioning.
pub fn partition_encrypted_chunk(
    block_from: i64,
    chunk_end: i64,
    live_holes: &[NzbgetRsByteRange],
) -> (Vec<NzbgetRsByteRange>, Vec<NzbgetRsByteRange>) {
    let mut hole_parts = Vec::new();
    for &live in live_holes {
        let from = live.offset.max(block_from);
        let to = range_end(live).min(chunk_end);
        if from < to {
            hole_parts.push(NzbgetRsByteRange {
                offset: from,
                size: to - from,
            });
        }
    }
    let mut present_parts = vec![NzbgetRsByteRange {
        offset: block_from,
        size: chunk_end - block_from,
    }];
    for &part in &hole_parts {
        present_parts = subtract_covered(&present_parts, part);
    }
    (hole_parts, present_parts)
}

/// `ContentMapper::BuildRepairSets` inner-hole mapping and sorting.
pub fn content_mapper_collect_inner_holes(
    runs: &[NzbgetRsContentRun],
    hole_member_indices: &[i32],
    holes: &[NzbgetRsByteRange],
) -> Vec<NzbgetRsByteRange> {
    let mut inner_holes = Vec::new();
    for (&member_idx, &hole) in hole_member_indices.iter().zip(holes.iter()) {
        inner_holes.extend(content_map_to_inner(runs, member_idx, hole));
    }
    inner_holes.sort_by_key(|r| r.offset);
    inner_holes
}

#[derive(Debug, Clone)]
pub struct RarFileEntryInput<'a> {
    pub volume_index: i32,
    pub filename: &'a [u8],
    pub size: i64,
    pub packed_size: i64,
    pub data_offset: i64,
    pub stored: bool,
    pub encrypted_data: bool,
    pub has_salt: bool,
    pub salt: [u8; 8],
    pub has_crypt: bool,
    pub crypt_version: i32,
    pub crypt_kdf_count: i32,
    pub crypt_salt: [u8; 16],
    pub crypt_iv: [u8; 16],
}

#[derive(Debug, Clone)]
pub struct RarMapOutput {
    pub inner_name: Vec<u8>,
    pub inner_size: i64,
    pub runs: Vec<NzbgetRsContentRun>,
    pub encrypted: bool,
    pub cipher_data_offsets: Vec<i64>,
    pub cipher_sizes: Vec<i64>,
}

/// `ContentMapper::BuildRarMap` run validation and `ContentMap` construction.
#[allow(clippy::too_many_arguments)]
pub fn content_mapper_build_rar_map<FCrypto>(
    unknown_volume: i32,
    has_password: bool,
    vol_readable: &[u8],
    vol_versions: &[i32],
    vol_member_indices: &[i32],
    vol_has_source: &[u8],
    vol_source_sizes: &[i64],
    entries: &[RarFileEntryInput<'_>],
    mut make_crypto: FCrypto,
) -> Result<RarMapOutput, &'static str>
where
    FCrypto: FnMut(usize, i32) -> bool,
{
    let vol_count = vol_readable
        .len()
        .min(vol_versions.len())
        .min(vol_member_indices.len())
        .min(vol_has_source.len())
        .min(vol_source_sizes.len());

    let mut primary: Option<&RarFileEntryInput<'_>> = None;
    for (v, &readable) in vol_readable.iter().take(vol_count).enumerate() {
        if readable == 0 {
            continue;
        }
        for entry in entries.iter().filter(|e| e.volume_index == v as i32) {
            if primary.map_or(true, |p| entry.size > p.size) {
                primary = Some(entry);
            }
        }
        break;
    }
    let Some(primary) = primary else {
        return Err("no file entries in the first readable volume");
    };

    let inner_name = base_file_name(primary.filename).to_vec();
    let inner_size = primary.size;
    if !is_stream_eligible(&inner_name) {
        return Err("inner file is not a media file");
    }

    #[derive(Clone, Default)]
    struct VolumeRun {
        entry_index: usize,
        packed_size: i64,
        data_offset: i64,
        version: i32,
        encrypted: bool,
        has_salt: bool,
        salt: [u8; 8],
        has_crypt: bool,
        crypt_version: i32,
        crypt_kdf_count: i32,
        crypt_salt: [u8; 16],
        crypt_iv: [u8; 16],
    }

    let mut runs: Vec<VolumeRun> = (0..vol_count)
        .map(|_| VolumeRun {
            data_offset: -1,
            ..Default::default()
        })
        .collect();
    let mut known_packed: i64 = 0;
    let mut any_encrypted = false;
    let mut any_plain = false;

    for i in 0..vol_count {
        if vol_readable[i] == 0 {
            continue;
        }
        for (entry_idx, entry) in entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.volume_index == i as i32)
        {
            if locale_casecmp(base_file_name(entry.filename), &inner_name)
                != std::cmp::Ordering::Equal
            {
                continue;
            }
            if !entry.stored {
                return Err("compressed archive (only store mode is mappable)");
            }
            if entry.encrypted_data {
                if !has_password {
                    return Err("encrypted archive data");
                }
                any_encrypted = true;
                runs[i].entry_index = entry_idx;
                runs[i].encrypted = true;
                runs[i].version = vol_versions[i];
                if entry.has_salt {
                    runs[i].has_salt = true;
                    runs[i].salt = entry.salt;
                }
                if entry.has_crypt {
                    runs[i].has_crypt = true;
                    runs[i].crypt_version = entry.crypt_version;
                    runs[i].crypt_kdf_count = entry.crypt_kdf_count;
                    runs[i].crypt_salt = entry.crypt_salt;
                    runs[i].crypt_iv = entry.crypt_iv;
                }
            } else {
                any_plain = true;
            }
            if entry.size != inner_size {
                return Err("inconsistent inner file size across volumes");
            }
            let src_sz = vol_source_sizes[i];
            if entry.data_offset < 0
                || entry.packed_size < 0
                || vol_has_source[i] == 0
                || entry.data_offset > src_sz
                || entry.packed_size > src_sz - entry.data_offset
            {
                return Err("implausible data run geometry");
            }
            runs[i].packed_size = entry.packed_size;
            runs[i].data_offset = entry.data_offset;
            known_packed = known_packed.wrapping_add(entry.packed_size);
        }
    }

    if any_encrypted {
        if any_plain {
            return Err("encrypted volume geometry does not fit store mode");
        }
        if unknown_volume >= 0 {
            return Err("encrypted archive without crypto parameters");
        }
        let enc_vols: Vec<usize> = (0..vol_count)
            .filter(|&i| runs[i].data_offset >= 0 && runs[i].encrypted)
            .collect();
        if enc_vols.is_empty() {
            return Err("encrypted volume geometry does not fit store mode");
        }
        let first = enc_vols[0];
        let version = runs[first].version;
        if version == 3 {
            if !runs[first].has_salt || !make_crypto(runs[first].entry_index, 3) {
                return Err("encrypted archive without crypto parameters");
            }
        } else if version == 5 {
            if !runs[first].has_crypt || runs[first].crypt_version != 0 {
                return Err("encrypted archive without crypto parameters");
            }
            if !make_crypto(runs[first].entry_index, 5) {
                return Err("archive password rejected");
            }
        } else {
            return Err("encrypted archive without crypto parameters");
        }

        for &v in &enc_vols[1..] {
            let vr = &runs[v];
            if vr.version != version {
                return Err("encrypted volume geometry does not fit store mode");
            }
            if version == 3 && (!vr.has_salt || vr.salt != runs[first].salt) {
                return Err("encrypted volume geometry does not fit store mode");
            }
            if version == 5
                && (!vr.has_crypt
                    || vr.crypt_version != runs[first].crypt_version
                    || vr.crypt_kdf_count != runs[first].crypt_kdf_count
                    || vr.crypt_salt != runs[first].crypt_salt
                    || vr.crypt_iv != runs[first].crypt_iv)
            {
                return Err("encrypted volume geometry does not fit store mode");
            }
        }

        let mut cipher_total: i64 = 0;
        for &v in &enc_vols {
            if runs[v].packed_size <= 0 {
                return Err("encrypted volume geometry does not fit store mode");
            }
            cipher_total = cipher_total.wrapping_add(runs[v].packed_size);
        }
        let pad = (16 - (inner_size % 16)) % 16;
        if inner_size <= 0
            || cipher_total < inner_size
            || cipher_total - inner_size != pad
            || cipher_total % 16 != 0
        {
            return Err("encrypted volume geometry does not fit store mode");
        }

        let mut out_runs = Vec::with_capacity(enc_vols.len());
        let mut cipher_data_offsets = Vec::with_capacity(enc_vols.len());
        let mut cipher_sizes = Vec::with_capacity(enc_vols.len());
        let mut cipher_base: i64 = 0;
        for &v in &enc_vols {
            let plain_chunk = runs[v].packed_size.min(inner_size - cipher_base);
            if plain_chunk <= 0 {
                return Err("encrypted volume geometry does not fit store mode");
            }
            out_runs.push(NzbgetRsContentRun {
                inner_offset: cipher_base,
                member_index: vol_member_indices[v],
                member_offset: runs[v].data_offset,
                size: plain_chunk,
            });
            cipher_data_offsets.push(runs[v].data_offset);
            cipher_sizes.push(runs[v].packed_size);
            cipher_base = cipher_base.wrapping_add(runs[v].packed_size);
        }
        return Ok(RarMapOutput {
            inner_name,
            inner_size,
            runs: out_runs,
            encrypted: true,
            cipher_data_offsets,
            cipher_sizes,
        });
    }

    if unknown_volume < 0 {
        if known_packed > inner_size {
            return Err("packed sizes exceed the inner file size");
        }
        if known_packed != inner_size {
            return Err("packed sizes do not sum to the inner file size");
        }
    } else {
        if known_packed > inner_size {
            return Err("packed sizes exceed the inner file size");
        }
        if (unknown_volume as usize) < runs.len() {
            runs[unknown_volume as usize].packed_size = inner_size - known_packed;
            runs[unknown_volume as usize].data_offset = -1;
        }
    }

    let mut out_runs = Vec::new();
    let mut inner_offset: i64 = 0;
    for i in 0..vol_count {
        if runs[i].data_offset >= 0 && runs[i].packed_size > 0 {
            out_runs.push(NzbgetRsContentRun {
                inner_offset,
                member_index: vol_member_indices[i],
                member_offset: runs[i].data_offset,
                size: runs[i].packed_size,
            });
        }
        inner_offset = inner_offset.wrapping_add(runs[i].packed_size);
    }

    Ok(RarMapOutput {
        inner_name,
        inner_size,
        runs: out_runs,
        encrypted: false,
        cipher_data_offsets: Vec::new(),
        cipher_sizes: Vec::new(),
    })
}

/// `DupeStreamRepair::SelectExtractedInner`: walks `dir` recursively without following
/// symlinks, returning `(selected_path, found_symlink)`.
pub fn select_extracted_inner(
    dir: &[u8],
    inner_size: i64,
    inner_name: &[u8],
) -> (Vec<u8>, bool) {
    if dir.is_empty() || inner_size < 0 {
        return (Vec::new(), false);
    }
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;

    const MAX_EXTRACTED_ENTRIES: usize = 10000;
    let mut entry_count: usize = 0;
    let mut extracted_bytes: i64 = 0;
    let mut matches: Vec<Vec<u8>> = Vec::new();
    let mut stack = vec![PathBuf::from(OsStr::from_bytes(dir))];

    while let Some(cur) = stack.pop() {
        let rd = match std::fs::read_dir(&cur) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => continue,
            Err(_) => break,
        };
        for entry_res in rd {
            let Ok(entry) = entry_res else {
                return (Vec::new(), false);
            };
            entry_count += 1;
            if entry_count > MAX_EXTRACTED_ENTRIES {
                return (Vec::new(), false);
            }
            let path = entry.path();
            let Ok(sym_meta) = std::fs::symlink_metadata(&path) else {
                return (Vec::new(), true);
            };
            let ft = sym_meta.file_type();
            if ft.is_symlink() {
                return (Vec::new(), true);
            }
            if ft.is_dir() {
                stack.push(path);
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            let size = sym_meta.len() as i64;
            if size < 0
                || size > MAX_DECOMPRESS_BYTES
                || extracted_bytes > MAX_DECOMPRESS_BYTES - size
            {
                return (Vec::new(), false);
            }
            extracted_bytes += size;
            if size == inner_size {
                matches.push(path.as_os_str().as_bytes().to_vec());
            }
        }
    }

    if matches.is_empty() {
        return (Vec::new(), false);
    }
    matches.sort();
    if !inner_name.is_empty() {
        for m in &matches {
            if locale_casecmp(base_file_name(m), inner_name) == std::cmp::Ordering::Equal {
                return (m.clone(), false);
            }
        }
    }
    (matches.into_iter().next().unwrap(), false)
}

/// Port of `DupeStreamRepair::BuildDonorKey`: builds a length-prefixed canonical key for a donor NZB
/// and password, preserving parser order of files, groups, and articles.
#[allow(clippy::too_many_arguments)]
pub fn build_donor_key(
    password: &[u8],
    file_names: &[&[u8]],
    file_sizes: &[i64],
    file_is_par: &[u8],
    file_group_counts: &[usize],
    groups: &[&[u8]],
    file_article_counts: &[usize],
    article_part_numbers: &[i32],
    article_sizes: &[i32],
    article_message_ids: &[&[u8]],
) -> Vec<u8> {
    let mut key = Vec::new();
    let append = |out: &mut Vec<u8>, val: &[u8]| {
        let len_str = val.len().to_string();
        out.extend_from_slice(len_str.as_bytes());
        out.push(b':');
        out.extend_from_slice(val);
    };
    append(&mut key, password);
    let file_count = file_names
        .len()
        .min(file_sizes.len())
        .min(file_is_par.len())
        .min(file_group_counts.len())
        .min(file_article_counts.len());
    append(&mut key, file_count.to_string().as_bytes());
    let mut grp_idx = 0usize;
    let mut art_idx = 0usize;
    for i in 0..file_count {
        append(&mut key, file_names[i]);
        append(&mut key, file_sizes[i].to_string().as_bytes());
        append(&mut key, if file_is_par[i] != 0 { b"par" } else { b"data" });
        let g_cnt = file_group_counts[i];
        append(&mut key, g_cnt.to_string().as_bytes());
        for _ in 0..g_cnt {
            if grp_idx < groups.len() {
                append(&mut key, groups[grp_idx]);
                grp_idx += 1;
            }
        }
        let a_cnt = file_article_counts[i];
        append(&mut key, a_cnt.to_string().as_bytes());
        for _ in 0..a_cnt {
            if art_idx < article_part_numbers.len()
                && art_idx < article_sizes.len()
                && art_idx < article_message_ids.len()
            {
                append(&mut key, article_part_numbers[art_idx].to_string().as_bytes());
                append(&mut key, article_sizes[art_idx].to_string().as_bytes());
                append(&mut key, article_message_ids[art_idx]);
                art_idx += 1;
            }
        }
    }
    key
}

/// Computes `m_memberBases` and `m_totalSize` for `CompositeSource::CompositeSource`.
pub fn composite_compute_member_bases(member_sizes: &[i64], out_bases: &mut [i64]) -> i64 {
    let n = member_sizes.len().min(out_bases.len());
    let mut total_size: i64 = 0;
    for i in 0..n {
        out_bases[i] = total_size;
        total_size = total_size.wrapping_add(member_sizes[i]);
    }
    total_size
}

/// Sums `RunCrypto::CipherSize` values for `ContentMap::GetCipherStreamSize`.
pub fn content_map_sum_cipher_sizes(cipher_sizes: &[i64]) -> i64 {
    cipher_sizes
        .iter()
        .fold(0i64, |acc, &sz| acc.wrapping_add(sz))
}

/// Extracts the inner name for `ContentMapper::BuildSplitMap`:
/// returns `first_name[..rfind('.')]` if `is_stream_eligible(inner_name)` is true, else empty.
pub fn content_mapper_split_inner_name(first_name: &[u8]) -> Vec<u8> {
    let stem = match first_name.iter().rposition(|&b| b == b'.') {
        Some(pos) => &first_name[..pos],
        None => first_name,
    };
    if is_stream_eligible(stem) {
        stem.to_vec()
    } else {
        Vec::new()
    }
}

/// Orders donor indices for `StreamRepairController::CollectDonors` by descending `dupe_scores`
/// and ascending `ids` on ties.
pub fn stream_repair_order_donor_indices(dupe_scores: &[i32], ids: &[i32]) -> Vec<usize> {
    let n = dupe_scores.len().min(ids.len());
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        dupe_scores[b]
            .cmp(&dupe_scores[a])
            .then_with(|| ids[a].cmp(&ids[b]))
    });
    order
}

/// Checks whether a completed member in `StreamRepairController::ProveDonorOnIntactFile` is an
/// intact non-PAR2 member of `target_set` that is not in `target_filenames`.
pub fn stream_repair_is_intact_proof_member_eligible(
    base_name: &[u8],
    member_name: &[u8],
    target_set: &[u8],
    target_filenames: &[&[u8]],
) -> bool {
    !ends_with_par2_ci(base_name)
        && volume_set_key(base_name) == target_set
        && !target_filenames
            .iter()
            .any(|t| locale_casecmp(t, member_name) == std::cmp::Ordering::Equal)
}

/// Builds the RAR3 KDF seed (`len * 2` bytes of UTF-16LE low-16-bit code units from `wchars`
/// followed by the 8-byte `salt`). Returns `false` if `wchars` is empty or `out_seed` is too small.
pub fn streamcrypto_fill_rar3_seed(
    wchars: &[u32],
    salt: &[u8; 8],
    out_seed: &mut [u8],
) -> bool {
    if wchars.is_empty() {
        return false;
    }
    let needed = wchars.len() * 2 + 8;
    if out_seed.len() < needed {
        return false;
    }
    for (i, &ch) in wchars.iter().enumerate() {
        out_seed[i * 2] = (ch & 0xFF) as u8;
        out_seed[i * 2 + 1] = ((ch & 0xFF00) >> 8) as u8;
    }
    out_seed[wchars.len() * 2..needed].copy_from_slice(salt);
    true
}

/// Computes the RAR5 PBKDF2-HMAC-SHA256 iteration count for `kdf_count` (must be `<= 24`),
/// optionally adding `32` for `DeriveRar5PswCheck`.
pub fn streamcrypto_rar5_iterations(
    kdf_count: u8,
    add_psw_check_offset: bool,
) -> Option<i32> {
    if kdf_count > 24 {
        return None;
    }
    let base = 1i32 << kdf_count;
    Some(if add_psw_check_offset {
        base + 32
    } else {
        base
    })
}

/// Computes the next batch block count (capped at `1 << 16` blocks = 1 MiB) and byte length
/// for `RarCryptoContext::Crypt`.
pub fn streamcrypto_next_batch(
    done_blocks: i64,
    total_blocks: i64,
    block_size: i64,
) -> (i64, i32) {
    const BATCH_BLOCKS: i64 = 1 << 16;
    let rem = total_blocks.saturating_sub(done_blocks);
    let batch = if rem < BATCH_BLOCKS { rem } else { BATCH_BLOCKS };
    let len = (batch * block_size) as i32;
    (batch, len)
}

/// Evaluates `DupeStreamRepair::BuildRepairJob`:
/// - `0`: skip (`return false`)
/// - `1`: whole-file hole job (`success_articles == 0 && encoded_size > 0`)
/// - `2`: compute holes from decoded articles (`success_articles > 0 && decoded_file_size > 0`)
#[allow(clippy::too_many_arguments)]
pub fn eval_build_repair_job(
    fallback_mode: i32,
    daf_stream: i32,
    raw_article: bool,
    is_par_file: bool,
    disk_basename: &[u8],
    has_nzb: bool,
    delete_status: i32,
    ds_none: i32,
    ds_health: i32,
    deleting: bool,
    parking: bool,
    has_post_info: bool,
    articles_empty: bool,
    success_articles: i32,
    encoded_size: i64,
    decoded_file_size: i64,
) -> i32 {
    if fallback_mode < daf_stream
        || raw_article
        || is_par_file
        || ends_with_par2_ci(disk_basename)
        || disk_basename.is_empty()
        || articles_empty
        || !has_nzb
        || has_post_info
    {
        return 0;
    }
    let health_cancel = delete_status == ds_health;
    if !health_cancel && (deleting || parking || delete_status != ds_none) {
        return 0;
    }
    if success_articles == 0 {
        if encoded_size <= 0 {
            0
        } else {
            1
        }
    } else if decoded_file_size <= 0 {
        0
    } else {
        2
    }
}

/// `DonorMemberSource::StoreFetched`:
/// Checks whether a fetched donor article is inconsistent with the member's
/// decoded file size or bounds. Returns `(invalid, mark_member_bad)`.
pub fn donor_check_fetched_part(
    success: bool,
    data_len: usize,
    offset: i64,
    fetched_file_size: i64,
    member_size: i64,
) -> (bool, bool) {
    if !success {
        return (false, false);
    }
    let size_mismatch = member_size >= 0 && fetched_file_size != member_size;
    let invalid = data_len == 0
        || offset < 0
        || offset.saturating_add(data_len as i64) > fetched_file_size
        || size_mismatch;
    (invalid, invalid && size_mismatch)
}

/// Resolves the estimated donor part index for `offset` corrected by `drift`
/// in `DonorMemberSource::PartForOffset` and `DonorMemberSource::Prefetch`.
pub fn donor_index_for_offset(
    offset: i64,
    drift: i64,
    ranges: &[NzbgetRsByteRange],
) -> i32 {
    let target = offset.saturating_sub(drift);
    let mut index: usize = 0;
    while index + 1 < ranges.len()
        && ranges[index].offset.saturating_add(ranges[index].size) <= target
    {
        index += 1;
    }
    index as i32
}

/// Evaluates one resolution step in `DonorMemberSource::PartForOffset`.
/// Returns `(status, new_index)` where:
/// - `status == 0`: out of bounds (`return nullptr`)
/// - `status == 1`: step again with `index = new_index`
/// - `status == 2`: found matching part at `index`
pub fn donor_step_part_for_offset(
    offset: i64,
    fetched_offset: i64,
    fetched_len: usize,
    index: i32,
    ranges_len: usize,
) -> (i32, i32) {
    if offset < fetched_offset {
        if index <= 0 {
            (0, index)
        } else {
            (1, index - 1)
        }
    } else if offset >= fetched_offset.saturating_add(fetched_len as i64) {
        if (index + 1) as usize >= ranges_len {
            (0, index)
        } else {
            (1, index + 1)
        }
    } else {
        (2, index)
    }
}

/// Computes `(first, last)` inclusive prefetch part indices in `DonorMemberSource::Prefetch`.
pub fn donor_calc_prefetch_bounds(
    first_est: i32,
    last_est: i32,
    ranges_len: usize,
) -> (i32, i32) {
    let first = (first_est - 1).max(0);
    let max_last = (ranges_len as i32) - 1;
    let last = (last_est + 1).min(max_last);
    (first, last)
}

/// Computes `(from, len)` for copying a slice of a fetched donor article in `DonorMemberSource::Read`.
pub fn donor_calc_read_chunk(
    pos: i64,
    fetched_offset: i64,
    fetched_data_len: usize,
    end_offset: i64,
) -> (i64, i64) {
    let from = pos - fetched_offset;
    let avail = (fetched_data_len as i64) - from;
    let rem = end_offset - pos;
    let len = avail.min(rem);
    (from, len)
}

/// Evaluates a single watchdog tick in `StreamRepairController::StartWatchdog`.
/// Updates `*out_new_progress` and `*out_reset_last_progress`, and returns `Some(reason)`
/// when the watchdog should stop the stream-repair pass.
#[allow(clippy::too_many_arguments)]
pub fn watchdog_eval_tick(
    progress_bytes: i64,
    prev_progress: i64,
    waiting: bool,
    materializing: i32,
    since_last_progress_sec: i64,
    since_begun_sec: i64,
    since_start_sec: i64,
    elapsed_sec: f64,
    timeout_sec: i32,
    max_run_factor: i32,
    max_conn_wait_sec: i32,
    slow_repair_factor: i32,
    download_rate: i64,
    start_progress: i64,
    missing_bytes: i64,
    out_new_progress: &mut i64,
    out_reset_last_progress: &mut bool,
) -> Option<Vec<u8>> {
    let has_activity = progress_bytes != prev_progress || waiting || materializing > 0;
    if has_activity {
        *out_new_progress = progress_bytes;
        *out_reset_last_progress = true;
    } else {
        *out_new_progress = prev_progress;
        *out_reset_last_progress = false;
        if since_last_progress_sec >= i64::from(timeout_sec) {
            return Some(format!("nothing recovered for {timeout_sec} seconds").into_bytes());
        }
    }

    let max_sec = timeout_sec.saturating_mul(max_run_factor);
    if since_begun_sec >= i64::from(max_sec.saturating_add(max_conn_wait_sec)) {
        return Some(
            format!("still waiting for news-server connections after {since_begun_sec} seconds")
                .into_bytes(),
        );
    }
    if since_start_sec >= i64::from(max_sec) {
        return Some(
            format!("still running after {max_sec} seconds, {max_run_factor} times the timeout")
                .into_bytes(),
        );
    }

    let recovered = (*out_new_progress).saturating_sub(start_progress);
    let remaining = missing_bytes.saturating_sub(recovered).max(0);
    if download_rate > 0
        && recovered > 0
        && remaining > 0
        && elapsed_sec >= (timeout_sec as f64) / 2.0
    {
        let repair_rate = (recovered as f64) / elapsed_sec;
        let repair_sec = (remaining as f64) / repair_rate;
        let download_sec = (remaining as f64) / (download_rate as f64);
        if repair_sec > (slow_repair_factor as f64) * download_sec
            && repair_sec > (timeout_sec as f64)
        {
            let repair_mb_s = repair_rate / 1024.0 / 1024.0;
            let remaining_mb = (remaining as f64) / 1024.0 / 1024.0;
            let download_mb_s = (download_rate as f64) / 1024.0 / 1024.0;
            return Some(
                format!(
                    "at {repair_mb_s:.2} MB/s the {remaining_mb:.1} MB left would take {repair_sec:.0} seconds, more than {slow_repair_factor} times a download at {download_mb_s:.2} MB/s"
                )
                .into_bytes(),
            );
        }
    }

    None
}

/// Formats the optional `" and N whole file(s)"` suffix for `StreamRepairController::StopWatchdog`.
pub fn format_whole_files_suffix(unsized_count: i32) -> Vec<u8> {
    if unsized_count > 0 {
        format!(" and {unsized_count} whole file(s)").into_bytes()
    } else {
        Vec::new()
    }
}

/// Checks whether a target file in `StreamRepairController::CollectTargets` is a `.par2` file
/// that should be excluded from stream repair targets.
pub fn is_par2_job_or_file(is_par_file: bool, filename: &[u8]) -> bool {
    is_par_file || ends_with_par2_ci(filename)
}

/// Evaluates consecutive unproductive donor outcomes in `StreamRepairController::ExecRepair`.
/// Returns `(new_consecutive_failures, should_bail)`.
pub fn eval_donor_outcome_bail(
    outcome: i32,
    ro_productive: i32,
    ro_unproductive: i32,
    donor_proven: bool,
    cur_consecutive_failures: i32,
    donor_failure_bail: i32,
    unproven_donor_bail: i32,
) -> (i32, bool) {
    if outcome == ro_productive {
        (0, false)
    } else if outcome == ro_unproductive {
        let next = cur_consecutive_failures + 1;
        let limit = if donor_proven {
            donor_failure_bail
        } else {
            unproven_donor_bail
        };
        (next, next >= limit)
    } else {
        (cur_consecutive_failures, false)
    }
}

/// Selects replacement probe parts from `reserve` in `StreamRepairController::VerifyDonor`.
pub fn select_probe_replacements(
    missing: i32,
    reserve: &[i32],
    max_probe_fetches: i32,
    attempted_len: usize,
) -> Vec<i32> {
    let budget_rem = max_probe_fetches - (attempted_len as i32);
    let replacements = missing.min(reserve.len() as i32).min(budget_rem).max(0);
    let mut batch = Vec::with_capacity(replacements as usize);
    for k in 0..replacements {
        let r_idx = if replacements <= 0 || reserve.is_empty() {
            0
        } else {
            (reserve.len() * ((2 * (k as usize)) + 1)) / (2 * (replacements as usize))
        };
        batch.push(reserve[r_idx]);
    }
    batch
}

/// Validates a fetched probe article in `StreamRepairController::CompareProbes`.
/// Returns `(is_valid, is_missing)`.
pub fn check_probe_article(
    success: bool,
    data_len: usize,
    file_size: i64,
    offset: i64,
    target_decoded_size: i64,
) -> (bool, bool) {
    if !success || data_len == 0 {
        return (false, true);
    }
    if file_size != target_decoded_size
        || offset.saturating_add(data_len as i64) > target_decoded_size
    {
        return (false, false);
    }
    (true, false)
}

/// Returns whether a fetched donor article in `PatchFromDonor` is valid and within the target's decoded size.
pub fn is_patch_article_usable(
    success: bool,
    fetched_file_size: i64,
    target_decoded_size: i64,
    data_len: usize,
    fetched_offset: i64,
) -> bool {
    if !success
        || fetched_file_size != target_decoded_size
        || data_len == 0
        || fetched_offset < 0
    {
        return false;
    }
    match i64::try_from(data_len)
        .ok()
        .and_then(|len| fetched_offset.checked_add(len))
    {
        Some(end) => end <= target_decoded_size,
        None => false,
    }
}

/// Clips a target hole `(hole_offset, hole_size)` against a fetched donor article `(fetched_offset, fetched_len)`.
/// Returns `Some((from, to))` when `from < to`.
pub fn clip_hole_to_fetched(
    hole_offset: i64,
    hole_size: i64,
    fetched_offset: i64,
    fetched_len: usize,
) -> Option<(i64, i64)> {
    let hole_end = hole_offset.saturating_add(hole_size);
    let fetched_end = fetched_offset.saturating_add(fetched_len as i64);
    let from = hole_offset.max(fetched_offset);
    let to = hole_end.min(fetched_end);
    if from < to {
        Some((from, to))
    } else {
        None
    }
}

/// Classifies a target at the end of `StreamRepairController::ReportRemainingHoles` and updates
/// recovered article/byte-health credits when the target is fully repaired.
#[allow(clippy::too_many_arguments)]
pub fn classify_remaining_hole_target(
    interrupted: bool,
    decoded_file_size: i64,
    holes_empty: bool,
    is_par_file: bool,
    failed_articles: i32,
    failed_size: i64,
    missed_size: i64,
    cur_recovered_articles: i32,
    cur_recovered_failed_size: i64,
    cur_recovered_failed_par_size: i64,
) -> (i32, i32, i64, i64) {
    if interrupted && (decoded_file_size <= 0 || !holes_empty) {
        return (
            1,
            cur_recovered_articles,
            cur_recovered_failed_size,
            cur_recovered_failed_par_size,
        );
    }
    if decoded_file_size <= 0 {
        return (
            2,
            cur_recovered_articles,
            cur_recovered_failed_size,
            cur_recovered_failed_par_size,
        );
    }
    if !holes_empty {
        return (
            3,
            cur_recovered_articles,
            cur_recovered_failed_size,
            cur_recovered_failed_par_size,
        );
    }
    let delta_size = failed_size.saturating_add(missed_size);
    let new_articles = cur_recovered_articles.saturating_add(failed_articles);
    let new_failed_size = cur_recovered_failed_size.saturating_add(delta_size);
    let new_failed_par_size = if is_par_file {
        cur_recovered_failed_par_size.saturating_add(delta_size)
    } else {
        cur_recovered_failed_par_size
    };
    (0, new_articles, new_failed_size, new_failed_par_size)
}

/// Returns true when the missing byte count exceeds the target's encoded size by
/// more than 10% + 1 MiB, indicating an incomplete NZB (`ReportRemainingHoles`).
pub fn is_incomplete_nzb_hole(missing: i64, encoded_size: i64) -> bool {
    missing
        > encoded_size
            .saturating_add(encoded_size / 10)
            .saturating_add(1024 * 1024)
}

/// Evaluates chunk-size backoff or piece skip when `ReadDonorInner` fails in
/// `StreamRepairController::PatchFromDonorSet`.
pub fn patch_donor_set_step_failure(
    chunk: i64,
    min_chunk: i64,
    cur_size: i64,
    cur_pos: i64,
    cur_failed_pieces: i32,
    max_failed_pieces: i32,
) -> (i32, i64, i64, i32) {
    if chunk > min_chunk {
        let new_size = min_chunk.max(chunk / 4);
        return (1, new_size, cur_pos, cur_failed_pieces);
    }
    let next_failed = cur_failed_pieces + 1;
    if next_failed >= max_failed_pieces {
        (0, cur_size, cur_pos, next_failed)
    } else {
        (2, cur_size, cur_pos.saturating_add(chunk), next_failed)
    }
}

/// Computes `(window_end, from, block_to)` for an encrypted verification window in
/// `StreamRepairController::VerifyEncryptedFromSource`, returning `None` when `from >= window_end`.
pub fn calc_encrypted_window_bounds(
    window_offset: i64,
    window_end_raw: i64,
    last_computable: i64,
    block_from: i64,
    block_size: i64,
) -> Option<(i64, i64, i64)> {
    if block_size <= 0 {
        return None;
    }
    let window_end = window_end_raw.min(last_computable);
    let from = window_offset.max(block_from);
    if from >= window_end {
        return None;
    }
    let block_to = (window_end + block_size - 1) / block_size * block_size;
    Some((window_end, from, block_to))
}

/// Computes the live remainder of an inner hole in `StreamRepairController::PatchEncryptedFromSource`,
/// clips it to `last_computable`, updates `uncomputable_tail`, and expands to cipher-block bounds.
pub fn encrypted_clip_live_hole(
    hole_offset: i64,
    hole_size: i64,
    live_holes: &[NzbgetRsByteRange],
    last_computable: i64,
    block_size: i64,
    cur_uncomputable_tail: i64,
) -> (bool, i64, i64, i64, i64, i64) {
    let hole_end = hole_offset.saturating_add(hole_size);
    let mut hole_from: i64 = -1;
    let mut hole_to: i64 = -1;
    for live in live_holes {
        let live_end = live.offset.saturating_add(live.size);
        let from = live.offset.max(hole_offset);
        let to = live_end.min(hole_end);
        if from < to {
            if hole_from < 0 {
                hole_from = from;
            }
            hole_to = to;
        }
    }
    if hole_from < 0 {
        return (false, cur_uncomputable_tail, -1, -1, 0, 0);
    }
    let patch_to = hole_to.min(last_computable);
    let new_tail = cur_uncomputable_tail + (hole_to - patch_to.max(hole_from));
    if patch_to <= hole_from || block_size <= 0 {
        return (false, new_tail, hole_from, patch_to, 0, 0);
    }
    let block_from = (hole_from / block_size) * block_size;
    let block_to = ((patch_to + block_size - 1) / block_size) * block_size;
    (true, new_tail, hole_from, patch_to, block_from, block_to)
}

/// Checks whether the predecessor cipher block `[block_from - block_size, block_from)` is
/// completely outside all current `live_holes` in `StreamRepairController::PatchEncryptedFromSource`.
pub fn encrypted_is_prev_block_present(
    block_from: i64,
    block_size: i64,
    live_holes: &[NzbgetRsByteRange],
) -> bool {
    let prev_start = block_from - block_size;
    for live in live_holes {
        let live_end = live.offset.saturating_add(live.size);
        if !(live_end <= prev_start || live.offset >= block_from) {
            return false;
        }
    }
    true
}

/// Computes `(chunk_end, block_count)` for an encrypted patch chunk in
/// `StreamRepairController::PatchEncryptedFromSource`.
pub fn calc_encrypted_chunk_end(
    block_from: i64,
    block_to: i64,
    max_chunk: i64,
    block_size: i64,
) -> (i64, i64) {
    let chunk_end = (block_from.saturating_add(max_chunk)).min(block_to);
    let block_count = if block_size > 0 {
        (chunk_end - block_from) / block_size
    } else {
        0
    };
    (chunk_end, block_count)
}

/// Formats `"Downloading duplicate <donor_name> (volume <member_num>/<total_members>, article <delivered_articles>/<total_articles>, <mb:.1> MB)"`
/// for `StreamRepairController::MaterializeDonorSet`.
pub fn format_materialize_progress(
    donor_name: &[u8],
    member_num: i32,
    total_members: i32,
    delivered_articles: i32,
    total_articles: i32,
    total_bytes: i64,
) -> Vec<u8> {
    let mb = (total_bytes as f64) / 1024.0 / 1024.0;
    let suffix = format!(
        " (volume {member_num}/{total_members}, article {delivered_articles}/{total_articles}, {mb:.1} MB)"
    );
    let mut out = Vec::with_capacity(22 + donor_name.len() + suffix.len());
    out.extend_from_slice(b"Downloading duplicate ");
    out.extend_from_slice(donor_name);
    out.extend_from_slice(suffix.as_bytes());
    out
}

/// Validates a fetched donor article before writing it into the materialized
/// archive member file (`StreamRepairController::MaterializeDonorSet`).
pub fn materialize_article_valid(
    success: bool,
    data_len: usize,
    offset: i64,
    fetched_file_size: i64,
    member_size: i64,
) -> bool {
    success
        && data_len > 0
        && offset >= 0
        && fetched_file_size > 0
        && offset <= fetched_file_size
        && (data_len as i64) <= fetched_file_size - offset
        && (member_size < 0 || fetched_file_size == member_size)
}

/// Computes updated NZB failed/success byte counters after crediting fully
/// repaired files back to health (`StreamRepairController::RepairCompleted`).
#[allow(clippy::too_many_arguments)]
pub fn credit_repaired_sizes(
    recovered_failed_size: i64,
    recovered_failed_par_size: i64,
    failed_size: i64,
    success_size: i64,
    current_failed_size: i64,
    current_success_size: i64,
    par_failed_size: i64,
    par_success_size: i64,
    par_current_failed_size: i64,
    par_current_success_size: i64,
) -> (i64, i64, i64, i64, i64, i64, i64, i64) {
    let mut new_failed = failed_size;
    let mut new_success = success_size;
    let mut new_cur_failed = current_failed_size;
    let mut new_cur_success = current_success_size;
    let mut new_par_failed = par_failed_size;
    let mut new_par_success = par_success_size;
    let mut new_par_cur_failed = par_current_failed_size;
    let mut new_par_cur_success = par_current_success_size;

    if recovered_failed_size > 0 {
        new_failed = 0.max(failed_size.saturating_sub(recovered_failed_size));
        new_success = success_size.saturating_add(recovered_failed_size);
        new_cur_failed = 0.max(current_failed_size.saturating_sub(recovered_failed_size));
        new_cur_success = current_success_size.saturating_add(recovered_failed_size);
        if recovered_failed_par_size > 0 {
            new_par_failed = 0.max(par_failed_size.saturating_sub(recovered_failed_par_size));
            new_par_success = par_success_size.saturating_add(recovered_failed_par_size);
            new_par_cur_failed =
                0.max(par_current_failed_size.saturating_sub(recovered_failed_par_size));
            new_par_cur_success =
                par_current_success_size.saturating_add(recovered_failed_par_size);
        }
    }

    (
        new_failed,
        new_success,
        new_cur_failed,
        new_cur_success,
        new_par_failed,
        new_par_success,
        new_par_cur_failed,
        new_par_cur_success,
    )
}

/// Resolves post-stream-repair par-check and par-rename state transitions in
/// `StreamRepairController::RepairCompleted`.
#[allow(clippy::too_many_arguments)]
pub fn resolve_completion_par_action(
    recovered_bytes: i64,
    holes_remain: bool,
    par_rename_status: i32,
    rs_none: i32,
    par_status: i32,
    ps_skipped: i32,
    par_check_opt: i32,
    pc_manual: i32,
    ps_manual: i32,
    ps_none: i32,
) -> (bool, bool, i32, bool, bool, bool) {
    let recovered = recovered_bytes > 0;
    if !recovered && !holes_remain {
        return (false, false, par_status, false, false, false);
    }
    let reset_par_rename = recovered && par_rename_status != rs_none;
    if par_status > ps_skipped {
        if recovered {
            let new_status = if par_check_opt == pc_manual {
                ps_manual
            } else {
                ps_none
            };
            (reset_par_rename, true, new_status, true, true, false)
        } else {
            (reset_par_rename, false, par_status, false, false, false)
        }
    } else {
        (reset_par_rename, false, par_status, recovered, false, true)
    }
}

/// Converts a byte count into megabytes (`bytes / 1024.0 / 1024.0`).
pub fn stream_repair_bytes_to_mb(bytes: i64) -> f64 {
    (bytes as f64) / 1024.0 / 1024.0
}

// ============================================================================
// Pure-Rust StreamCrypto & RarCryptoContext (`daemon/postprocess/StreamCrypto.cpp`)
// ============================================================================

#[derive(Clone)]
struct Sha1 {
    h: [u32; 5],
    buf: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

impl Sha1 {
    fn new() -> Self {
        Self {
            h: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
            buf: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    #[inline]
    fn compress(h: &mut [u32; 5], block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];

        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    fn update(&mut self, mut data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u64);
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                Self::compress(&mut self.h, &self.buf);
                self.buf_len = 0;
            }
        }
        while data.len() >= 64 {
            let block: &[u8; 64] = data[..64].try_into().unwrap();
            Self::compress(&mut self.h, block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    fn finalize(&self) -> [u8; 20] {
        let mut h = self.h;
        let mut block = self.buf;
        let mut pos = self.buf_len;
        block[pos] = 0x80;
        pos += 1;
        if pos > 56 {
            block[pos..64].fill(0);
            Self::compress(&mut h, &block);
            block.fill(0);
        } else {
            block[pos..56].fill(0);
        }
        let bits = self.total_len.wrapping_mul(8);
        block[56..64].copy_from_slice(&bits.to_be_bytes());
        Self::compress(&mut h, &block);

        let mut out = [0u8; 20];
        for (i, &word) in h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
    0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
    0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
    0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
    0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
    0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

const SHA256_H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
    0x5be0cd19,
];

#[inline]
fn sha256_compress(h: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            block[i * 4],
            block[i * 4 + 1],
            block[i * 4 + 2],
            block[i * 4 + 3],
        ]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let mut a = h[0];
    let mut b = h[1];
    let mut c = h[2];
    let mut d = h[3];
    let mut e = h[4];
    let mut f = h[5];
    let mut g = h[6];
    let mut hh = h[7];

    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let temp1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA256_K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);

        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
    h[5] = h[5].wrapping_add(f);
    h[6] = h[6].wrapping_add(g);
    h[7] = h[7].wrapping_add(hh);
}

#[derive(Clone)]
struct Sha256 {
    h: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            h: SHA256_H0,
            buf: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u64);
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                sha256_compress(&mut self.h, &self.buf);
                self.buf_len = 0;
            }
        }
        while data.len() >= 64 {
            let block: &[u8; 64] = data[..64].try_into().unwrap();
            sha256_compress(&mut self.h, block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    fn finalize(&self) -> [u8; 32] {
        let mut h = self.h;
        let mut block = self.buf;
        let mut pos = self.buf_len;
        block[pos] = 0x80;
        pos += 1;
        if pos > 56 {
            block[pos..64].fill(0);
            sha256_compress(&mut h, &block);
            block.fill(0);
        } else {
            block[pos..56].fill(0);
        }
        let bits = self.total_len.wrapping_mul(8);
        block[56..64].copy_from_slice(&bits.to_be_bytes());
        sha256_compress(&mut h, &block);

        let mut out = [0u8; 32];
        for (i, &word) in h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

fn pbkdf2_hmac_sha256_32(password: &[u8], salt: &[u8; 16], iterations: usize) -> [u8; 32] {
    let mut key_block = [0u8; 64];
    if password.len() > 64 {
        let mut s = Sha256::new();
        s.update(password);
        let d = s.finalize();
        key_block[..32].copy_from_slice(&d);
    } else {
        key_block[..password.len()].copy_from_slice(password);
    }

    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }

    let mut ipad_h = SHA256_H0;
    sha256_compress(&mut ipad_h, &ipad);
    let mut opad_h = SHA256_H0;
    sha256_compress(&mut opad_h, &opad);

    // Iteration 1: HMAC(password, salt || 0x00000001)
    let mut inner0 = Sha256 {
        h: ipad_h,
        buf: [0u8; 64],
        buf_len: 0,
        total_len: 64,
    };
    inner0.update(salt);
    inner0.update(&[0, 0, 0, 1]);
    let u_inner = inner0.finalize();

    let mut block = [0u8; 64];
    block[..32].copy_from_slice(&u_inner);
    block[32] = 0x80;
    // total_len = 64 + 32 = 96 bytes = 768 bits (0x0300)
    block[62] = 0x03;
    block[63] = 0x00;

    let mut h_out = opad_h;
    sha256_compress(&mut h_out, &block);
    let mut u_words = h_out;
    let mut dk_words = u_words;

    for _ in 1..iterations {
        for (i, &w) in u_words.iter().enumerate() {
            block[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
        }
        let mut h_in = ipad_h;
        sha256_compress(&mut h_in, &block);
        for (i, &w) in h_in.iter().enumerate() {
            block[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
        }
        let mut h_o = opad_h;
        sha256_compress(&mut h_o, &block);
        u_words = h_o;
        for i in 0..8 {
            dk_words[i] ^= u_words[i];
        }
    }

    let mut out = [0u8; 32];
    for (i, &w) in dk_words.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
    }
    out
}

/// `StreamCrypto::DeriveRar3`: derives the 16-byte AES-128 key and 16-byte CBC IV from UTF-8 `password` and 8-byte `salt`.
pub fn streamcrypto_derive_rar3(password: &[u8], salt: &[u8; 8]) -> Option<([u8; 16], [u8; 16])> {
    let nul_pos = password.iter().position(|&b| b == 0).unwrap_or(password.len());
    let pwd = &password[..nul_pos];
    if pwd.is_empty() {
        return None;
    }
    let mut utf16 = vec![0u16; pwd.len() * 2 + 1];
    let written = crate::util::wstring_decode_utf8_to_utf16(pwd, &mut utf16);
    if written == 0 {
        return None;
    }
    let wchars: Vec<u32> = utf16[..written].iter().map(|&u| u32::from(u)).collect();
    let mut seed = vec![0u8; wchars.len() * 2 + 8];
    if !streamcrypto_fill_rar3_seed(&wchars, salt, &mut seed) {
        return None;
    }

    const ROUNDS: usize = 0x40000;
    let mut sha1 = Sha1::new();
    let mut iv = [0u8; 16];
    for i in 0..ROUNDS {
        sha1.update(&seed);
        let buf = [i as u8, (i >> 8) as u8, (i >> 16) as u8];
        sha1.update(&buf);
        if i % (ROUNDS / 16) == 0 {
            let d = sha1.finalize();
            iv[i / (ROUNDS / 16)] = d[19];
        }
    }
    let digest = sha1.finalize();
    let mut key = [0u8; 16];
    crate::postrepair::rar3_swizzle_key(&digest, &mut key);
    Some((key, iv))
}

/// `StreamCrypto::DeriveRar5`: derives the 32-byte AES-256 key via PBKDF2-HMAC-SHA256.
pub fn streamcrypto_derive_rar5(
    password: &[u8],
    kdf_count: u8,
    salt: &[u8; 16],
) -> Option<[u8; 32]> {
    let iterations = streamcrypto_rar5_iterations(kdf_count, false)? as usize;
    let nul_pos = password.iter().position(|&b| b == 0).unwrap_or(password.len());
    let pwd = &password[..nul_pos];
    Some(pbkdf2_hmac_sha256_32(pwd, salt, iterations))
}

/// `StreamCrypto::DeriveRar5PswCheck`: derives the 8-byte RAR5 password check value.
pub fn streamcrypto_derive_rar5_psw_check(
    password: &[u8],
    kdf_count: u8,
    salt: &[u8; 16],
) -> Option<[u8; 8]> {
    let iterations = streamcrypto_rar5_iterations(kdf_count, true)? as usize;
    let nul_pos = password.iter().position(|&b| b == 0).unwrap_or(password.len());
    let pwd = &password[..nul_pos];
    let value = pbkdf2_hmac_sha256_32(pwd, salt, iterations);
    let mut check = [0u8; 8];
    crate::postrepair::rar5_fold_psw_check(&value, &mut check);
    Some(check)
}

const AES_SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab,
    0x76, 0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4,
    0x72, 0xc0, 0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71,
    0xd8, 0x31, 0x15, 0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2,
    0xeb, 0x27, 0xb2, 0x75, 0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6,
    0xb3, 0x29, 0xe3, 0x2f, 0x84, 0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb,
    0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf, 0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45,
    0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8, 0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5,
    0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2, 0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44,
    0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73, 0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a,
    0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb, 0xe0, 0x32, 0x3a, 0x0a, 0x49,
    0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79, 0xe7, 0xc8, 0x37, 0x6d,
    0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08, 0xba, 0x78, 0x25,
    0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a, 0x70, 0x3e,
    0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e, 0xe1,
    0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb,
    0x16,
];

const AES_INV_SBOX: [u8; 256] = [
    0x52, 0x09, 0x6a, 0xd5, 0x30, 0x36, 0xa5, 0x38, 0xbf, 0x40, 0xa3, 0x9e, 0x81, 0xf3, 0xd7,
    0xfb, 0x7c, 0xe3, 0x39, 0x82, 0x9b, 0x2f, 0xff, 0x87, 0x34, 0x8e, 0x43, 0x44, 0xc4, 0xde,
    0xe9, 0xcb, 0x54, 0x7b, 0x94, 0x32, 0xa6, 0xc2, 0x23, 0x3d, 0xee, 0x4c, 0x95, 0x0b, 0x42,
    0xfa, 0xc3, 0x4e, 0x08, 0x2e, 0xa1, 0x66, 0x28, 0xd9, 0x24, 0xb2, 0x76, 0x5b, 0xa2, 0x49,
    0x6d, 0x8b, 0xd1, 0x25, 0x72, 0xf8, 0xf6, 0x64, 0x86, 0x68, 0x98, 0x16, 0xd4, 0xa4, 0x5c,
    0xcc, 0x5d, 0x65, 0xb6, 0x92, 0x6c, 0x70, 0x48, 0x50, 0xfd, 0xed, 0xb9, 0xda, 0x5e, 0x15,
    0x46, 0x57, 0xa7, 0x8d, 0x9d, 0x84, 0x90, 0xd8, 0xab, 0x00, 0x8c, 0xbc, 0xd3, 0x0a, 0xf7,
    0xe4, 0x58, 0x05, 0xb8, 0xb3, 0x45, 0x06, 0xd0, 0x2c, 0x1e, 0x8f, 0xca, 0x3f, 0x0f, 0x02,
    0xc1, 0xaf, 0xbd, 0x03, 0x01, 0x13, 0x8a, 0x6b, 0x3a, 0x91, 0x11, 0x41, 0x4f, 0x67, 0xdc,
    0xea, 0x97, 0xf2, 0xcf, 0xce, 0xf0, 0xb4, 0xe6, 0x73, 0x96, 0xac, 0x74, 0x22, 0xe7, 0xad,
    0x35, 0x85, 0xe2, 0xf9, 0x37, 0xe8, 0x1c, 0x75, 0xdf, 0x6e, 0x47, 0xf1, 0x1a, 0x71, 0x1d,
    0x29, 0xc5, 0x89, 0x6f, 0xb7, 0x62, 0x0e, 0xaa, 0x18, 0xbe, 0x1b, 0xfc, 0x56, 0x3e, 0x4b,
    0xc6, 0xd2, 0x79, 0x20, 0x9a, 0xdb, 0xc0, 0xfe, 0x78, 0xcd, 0x5a, 0xf4, 0x1f, 0xdd, 0xa8,
    0x33, 0x88, 0x07, 0xc7, 0x31, 0xb1, 0x12, 0x10, 0x59, 0x27, 0x80, 0xec, 0x5f, 0x60, 0x51,
    0x7f, 0xa9, 0x19, 0xb5, 0x4a, 0x0d, 0x2d, 0xe5, 0x7a, 0x9f, 0x93, 0xc9, 0x9c, 0xef, 0xa0,
    0xe0, 0x3b, 0x4d, 0xae, 0x2a, 0xf5, 0xb0, 0xc8, 0xeb, 0xbb, 0x3c, 0x83, 0x53, 0x99, 0x61,
    0x17, 0x2b, 0x04, 0x7e, 0xba, 0x77, 0xd6, 0x26, 0xe1, 0x69, 0x14, 0x63, 0x55, 0x21, 0x0c,
    0x7d,
];

const AES_RCON: [u32; 10] = [
    0x01000000, 0x02000000, 0x04000000, 0x08000000, 0x10000000, 0x20000000, 0x40000000,
    0x80000000, 0x1b000000, 0x36000000,
];

#[inline]
fn aes_sub_word(w: u32) -> u32 {
    let b = w.to_be_bytes();
    u32::from_be_bytes([
        AES_SBOX[b[0] as usize],
        AES_SBOX[b[1] as usize],
        AES_SBOX[b[2] as usize],
        AES_SBOX[b[3] as usize],
    ])
}

#[inline]
fn aes_xtime(x: u8) -> u8 {
    (x << 1) ^ if x & 0x80 != 0 { 0x1b } else { 0 }
}

#[inline]
fn aes_gmul(mut a: u8, mut b: u8) -> u8 {
    let mut p = 0u8;
    for _ in 0..8 {
        if b & 1 != 0 {
            p ^= a;
        }
        a = aes_xtime(a);
        b >>= 1;
    }
    p
}

#[derive(Clone)]
struct AesRoundKeys {
    rounds: usize,
    rk: [[u8; 16]; 15],
}

impl AesRoundKeys {
    fn new(key: &[u8]) -> Option<Self> {
        let (nk, nr) = match key.len() {
            16 => (4usize, 10usize),
            32 => (8usize, 14usize),
            _ => return None,
        };
        let total_words = 4 * (nr + 1);
        let mut w = [0u32; 60];
        for i in 0..nk {
            w[i] = u32::from_be_bytes([key[i * 4], key[i * 4 + 1], key[i * 4 + 2], key[i * 4 + 3]]);
        }
        for i in nk..total_words {
            let mut temp = w[i - 1];
            if i % nk == 0 {
                temp = aes_sub_word(temp.rotate_left(8)) ^ AES_RCON[i / nk - 1];
            } else if nk == 8 && i % nk == 4 {
                temp = aes_sub_word(temp);
            }
            w[i] = w[i - nk] ^ temp;
        }
        let mut rk = [[0u8; 16]; 15];
        for r in 0..=nr {
            for c in 0..4 {
                rk[r][c * 4..c * 4 + 4].copy_from_slice(&w[r * 4 + c].to_be_bytes());
            }
        }
        Some(Self { rounds: nr, rk })
    }

    #[inline]
    fn encrypt_block(&self, input: &[u8; 16]) -> [u8; 16] {
        let mut s = *input;
        for i in 0..16 {
            s[i] ^= self.rk[0][i];
        }
        for r in 1..self.rounds {
            for b in &mut s {
                *b = AES_SBOX[*b as usize];
            }
            // ShiftRows
            let t1 = s[1];
            s[1] = s[5];
            s[5] = s[9];
            s[9] = s[13];
            s[13] = t1;
            s.swap(2, 10);
            s.swap(6, 14);
            let t3 = s[15];
            s[15] = s[11];
            s[11] = s[7];
            s[7] = s[3];
            s[3] = t3;
            // MixColumns
            for c in 0..4 {
                let i = c * 4;
                let (s0, s1, s2, s3) = (s[i], s[i + 1], s[i + 2], s[i + 3]);
                let tm = s0 ^ s1 ^ s2 ^ s3;
                s[i] ^= tm ^ aes_xtime(s0 ^ s1);
                s[i + 1] ^= tm ^ aes_xtime(s1 ^ s2);
                s[i + 2] ^= tm ^ aes_xtime(s2 ^ s3);
                s[i + 3] ^= tm ^ aes_xtime(s3 ^ s0);
            }
            for i in 0..16 {
                s[i] ^= self.rk[r][i];
            }
        }
        for b in &mut s {
            *b = AES_SBOX[*b as usize];
        }
        let t1 = s[1];
        s[1] = s[5];
        s[5] = s[9];
        s[9] = s[13];
        s[13] = t1;
        s.swap(2, 10);
        s.swap(6, 14);
        let t3 = s[15];
        s[15] = s[11];
        s[11] = s[7];
        s[7] = s[3];
        s[3] = t3;
        for i in 0..16 {
            s[i] ^= self.rk[self.rounds][i];
        }
        s
    }

    #[inline]
    fn decrypt_block(&self, input: &[u8; 16]) -> [u8; 16] {
        let mut s = *input;
        for i in 0..16 {
            s[i] ^= self.rk[self.rounds][i];
        }
        for r in (1..self.rounds).rev() {
            // InvShiftRows
            let t1 = s[13];
            s[13] = s[9];
            s[9] = s[5];
            s[5] = s[1];
            s[1] = t1;
            s.swap(2, 10);
            s.swap(6, 14);
            let t3 = s[3];
            s[3] = s[7];
            s[7] = s[11];
            s[11] = s[15];
            s[15] = t3;
            // InvSubBytes & AddRoundKey
            for i in 0..16 {
                s[i] = AES_INV_SBOX[s[i] as usize] ^ self.rk[r][i];
            }
            // InvMixColumns
            for c in 0..4 {
                let i = c * 4;
                let (s0, s1, s2, s3) = (s[i], s[i + 1], s[i + 2], s[i + 3]);
                s[i] = aes_gmul(s0, 14) ^ aes_gmul(s1, 11) ^ aes_gmul(s2, 13) ^ aes_gmul(s3, 9);
                s[i + 1] =
                    aes_gmul(s0, 9) ^ aes_gmul(s1, 14) ^ aes_gmul(s2, 11) ^ aes_gmul(s3, 13);
                s[i + 2] =
                    aes_gmul(s0, 13) ^ aes_gmul(s1, 9) ^ aes_gmul(s2, 14) ^ aes_gmul(s3, 11);
                s[i + 3] =
                    aes_gmul(s0, 11) ^ aes_gmul(s1, 13) ^ aes_gmul(s2, 9) ^ aes_gmul(s3, 14);
            }
        }
        let t1 = s[13];
        s[13] = s[9];
        s[9] = s[5];
        s[5] = s[1];
        s[1] = t1;
        s.swap(2, 10);
        s.swap(6, 14);
        let t3 = s[3];
        s[3] = s[7];
        s[7] = s[11];
        s[11] = s[15];
        s[15] = t3;
        for i in 0..16 {
            s[i] = AES_INV_SBOX[s[i] as usize] ^ self.rk[0][i];
        }
        s
    }
}

/// Pure-Rust `RarCryptoContext::Crypt` (AES-128-CBC or AES-256-CBC with padding disabled).
pub fn streamcrypto_crypt_cbc(
    encrypt: bool,
    key: &[u8],
    iv: &[u8; 16],
    input: &[u8],
    output: &mut [u8],
    blocks: i64,
) -> bool {
    if blocks < 0 {
        return false;
    }
    if blocks == 0 {
        return true;
    }
    let Some(byte_len) = (blocks as usize).checked_mul(16) else {
        return false;
    };
    if input.len() < byte_len || output.len() < byte_len {
        return false;
    }
    let Some(aes) = AesRoundKeys::new(key) else {
        return false;
    };
    let mut cv = *iv;
    if encrypt {
        for b in 0..blocks as usize {
            let off = b * 16;
            let mut block = [0u8; 16];
            for i in 0..16 {
                block[i] = input[off + i] ^ cv[i];
            }
            cv = aes.encrypt_block(&block);
            output[off..off + 16].copy_from_slice(&cv);
        }
    } else {
        for b in 0..blocks as usize {
            let off = b * 16;
            let c_block: [u8; 16] = input[off..off + 16].try_into().unwrap();
            let mut dec = aes.decrypt_block(&c_block);
            for i in 0..16 {
                dec[i] ^= cv[i];
            }
            output[off..off + 16].copy_from_slice(&dec);
            cv = c_block;
        }
    }
    true
}

/// Pure-Rust `RarCryptoContext` mirroring `daemon/postprocess/StreamCrypto.h`.
#[derive(Clone)]
pub struct RarCryptoContext {
    key: Vec<u8>,
    iv: [u8; 16],
}

impl RarCryptoContext {
    pub const CRYPTO_BLOCK_SIZE: usize = 16;

    pub fn make_rar3(password: &[u8], salt: &[u8; 8]) -> Option<Self> {
        let (key, iv) = streamcrypto_derive_rar3(password, salt)?;
        Some(Self {
            key: key.to_vec(),
            iv,
        })
    }

    pub fn make_rar5(password: &[u8], crypt: &crate::rarreader::Rar5Crypt) -> Option<Self> {
        let nul_pos = password.iter().position(|&b| b == 0).unwrap_or(password.len());
        if nul_pos == 0 || crypt.version != 0 {
            return None;
        }
        let key = streamcrypto_derive_rar5(password, crypt.kdf_count, &crypt.salt)?;
        if crypt.has_check {
            let check = streamcrypto_derive_rar5_psw_check(password, crypt.kdf_count, &crypt.salt)?;
            if check != crypt.check_value[..8] {
                return None;
            }
        }
        Some(Self {
            key: key.to_vec(),
            iv: crypt.iv,
        })
    }

    pub fn decrypt_range(
        &self,
        prev_cipher_block: Option<&[u8; 16]>,
        cipher: &[u8],
        plain: &mut [u8],
        blocks: i64,
    ) -> bool {
        let iv = prev_cipher_block.unwrap_or(&self.iv);
        streamcrypto_crypt_cbc(false, &self.key, iv, cipher, plain, blocks)
    }

    pub fn encrypt_range(
        &self,
        prev_cipher_block: Option<&[u8; 16]>,
        plain: &[u8],
        cipher: &mut [u8],
        blocks: i64,
    ) -> bool {
        let iv = prev_cipher_block.unwrap_or(&self.iv);
        streamcrypto_crypt_cbc(true, &self.key, iv, plain, cipher, blocks)
    }

    pub fn key(&self) -> &[u8] {
        &self.key
    }

    pub fn iv(&self) -> &[u8; 16] {
        &self.iv
    }

    pub fn from_key_iv(key: &[u8], iv: [u8; 16]) -> Option<Self> {
        if key.len() != 16 && key.len() != 32 {
            return None;
        }
        Some(Self {
            key: key.to_vec(),
            iv,
        })
    }
}

/// Pure-Rust implementation of `crate::rarreader::Cipher` for parsing header-encrypted (`-hp`) RAR3/RAR5 volumes.
#[derive(Clone, Default)]
pub struct NativeRarCipher {
    password: Vec<u8>,
    key: [u8; 32],
    iv: [u8; 16],
    aes: Option<AesRoundKeys>,
    cv: [u8; 16],
}

impl NativeRarCipher {
    pub fn new(password: &[u8]) -> Self {
        Self {
            password: password.to_vec(),
            ..Self::default()
        }
    }

    pub fn with_state(password: &[u8], key: [u8; 32], iv: [u8; 16]) -> Self {
        Self {
            password: password.to_vec(),
            key,
            iv,
            aes: None,
            cv: [0; 16],
        }
    }

    pub fn state(&self) -> ([u8; 32], [u8; 16]) {
        (self.key, self.iv)
    }
}

impl crate::rarreader::Cipher for NativeRarCipher {
    fn rar3_prepare(&mut self, salt: &[u8; 8]) -> bool {
        let Some((key, iv)) = streamcrypto_derive_rar3(&self.password, salt) else {
            return false;
        };
        self.key[..16].copy_from_slice(&key);
        self.iv = iv;
        true
    }

    fn rar5_prepare(&mut self, kdf_count: u8, salt: &[u8; 16]) -> bool {
        let Some(key) = streamcrypto_derive_rar5(&self.password, kdf_count, salt) else {
            return false;
        };
        self.key = key;
        true
    }

    fn init(&mut self, key_bits: i32, iv: Option<&[u8; 16]>) -> bool {
        if let Some(iv) = iv {
            self.iv = *iv;
        }
        let key_slice = match key_bits {
            128 => &self.key[..16],
            256 => &self.key[..32],
            _ => return false,
        };
        self.aes = AesRoundKeys::new(key_slice);
        self.cv = self.iv;
        self.aes.is_some()
    }

    fn decrypt(&mut self, input: &[u8; 16], output: &mut [u8; 16]) -> bool {
        let Some(aes) = &self.aes else {
            return false;
        };
        let mut dec = aes.decrypt_block(input);
        for i in 0..16 {
            dec[i] ^= self.cv[i];
        }
        *output = dec;
        self.cv = *input;
        true
    }

    fn free(&mut self) {
        self.aes = None;
    }
}

/// Pure-Rust `ContentSource` trait mirroring `daemon/postprocess/ContentMap.h`.
pub trait ContentSource {
    fn size(&self) -> i64;
    fn read(&self, offset: i64, buf: &mut [u8]) -> bool;
}

/// Pure-Rust `ContentSourceSet` trait mirroring `daemon/postprocess/ContentMap.h`.
pub trait ContentSourceSet {
    fn get_source(&self, member_index: i32) -> Option<&dyn ContentSource>;

    fn aborted(&self) -> bool {
        false
    }

    fn on_rar_volume_parsed(
        &self,
        _member_name: &[u8],
        _vol: &crate::rarreader::Volume,
        _files: &[crate::rarreader::RarFile],
    ) {
    }
}

struct ContentSourceRarAdapter<'a>(&'a dyn ContentSource);

impl crate::rarreader::Source for ContentSourceRarAdapter<'_> {
    fn size(&mut self) -> i64 {
        self.0.size()
    }
    fn read(&mut self, offset: i64, buf: &mut [u8]) -> bool {
        self.0.read(offset, buf)
    }
}

/// A member file of a posting (`SetMember` in `ContentMap.h`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetMember {
    pub name: Vec<u8>,
    pub size: i64,
}

impl SetMember {
    pub fn new(name: impl AsRef<[u8]>, size: i64) -> Self {
        Self {
            name: name.as_ref().to_vec(),
            size,
        }
    }
}

/// Container format for `MemberSet` (`MemberSet::EFormat` in `ContentMap.h`).
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberFormat {
    Bare = FORMAT_BARE,
    Split = FORMAT_SPLIT,
    Rar = FORMAT_RAR,
    Zip = FORMAT_ZIP,
    SevenZip = FORMAT_SEVEN_ZIP,
}

impl MemberFormat {
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            FORMAT_BARE => Some(Self::Bare),
            FORMAT_SPLIT => Some(Self::Split),
            FORMAT_RAR => Some(Self::Rar),
            FORMAT_ZIP => Some(Self::Zip),
            FORMAT_SEVEN_ZIP => Some(Self::SevenZip),
            _ => None,
        }
    }
}

/// Member files grouped into one container set, in data order (`MemberSet` in `ContentMap.h`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberSet {
    pub format: MemberFormat,
    pub members: Vec<i32>,
}

/// Presents ordered members as one logical byte stream (`CompositeSource` in `ContentMap.h`).
pub struct CompositeSource<'a> {
    sources: &'a dyn ContentSourceSet,
    member_indexes: Vec<i32>,
    member_sizes: Vec<i64>,
    member_bases: Vec<i64>,
    total_size: i64,
}

impl<'a> CompositeSource<'a> {
    pub fn new(
        sources: &'a dyn ContentSourceSet,
        member_indexes: Vec<i32>,
        member_sizes: Vec<i64>,
    ) -> Self {
        let mut member_bases = vec![0i64; member_sizes.len()];
        let total_size = composite_compute_member_bases(&member_sizes, &mut member_bases);
        Self {
            sources,
            member_indexes,
            member_sizes,
            member_bases,
            total_size,
        }
    }

    pub fn to_members(&self, logical_range: NzbgetRsByteRange) -> Vec<NzbgetRsMemberRange> {
        composite_to_members(
            &self.member_indexes,
            &self.member_bases,
            &self.member_sizes,
            logical_range,
        )
    }
}

impl ContentSource for CompositeSource<'_> {
    fn size(&self) -> i64 {
        self.total_size
    }

    fn read(&self, offset: i64, buf: &mut [u8]) -> bool {
        let size = buf.len() as i64;
        if offset < 0 || size < 0 || offset > self.total_size || size > self.total_size - offset {
            return false;
        }
        let mut filled = 0usize;
        for piece in self.to_members(NzbgetRsByteRange { offset, size }) {
            let Some(source) = self.sources.get_source(piece.member_index) else {
                return false;
            };
            let len = piece.size as usize;
            if !source.read(piece.offset, &mut buf[filled..filled + len]) {
                return false;
            }
            filled += len;
        }
        true
    }
}

/// Wraps another `ContentSource`, refusing reads that intersect captured holes (`HoledSource` in `ContentMap.h`).
pub struct HoledSource<'a> {
    inner: &'a dyn ContentSource,
    holes: Vec<NzbgetRsByteRange>,
}

impl<'a> HoledSource<'a> {
    pub fn new(inner: &'a dyn ContentSource, holes: Vec<NzbgetRsByteRange>) -> Self {
        Self { inner, holes }
    }
}

impl ContentSource for HoledSource<'_> {
    fn size(&self) -> i64 {
        self.inner.size()
    }

    fn read(&self, offset: i64, buf: &mut [u8]) -> bool {
        if holed_source_intersects(&self.holes, offset, buf.len() as i64) {
            return false;
        }
        self.inner.read(offset, buf)
    }
}

/// Per-member hole-aware view over another source set (`HoledSourceSet` in `ContentMap.h`).
pub struct HoledSourceSet<'a> {
    inner: &'a dyn ContentSourceSet,
    member_holes: Vec<Vec<NzbgetRsByteRange>>,
    wrapped: Vec<std::cell::OnceCell<Option<HoledSource<'a>>>>,
}

impl<'a> HoledSourceSet<'a> {
    pub fn new(inner: &'a dyn ContentSourceSet, member_holes: &[Vec<NzbgetRsByteRange>]) -> Self {
        let mut wrapped = Vec::with_capacity(member_holes.len());
        wrapped.resize_with(member_holes.len(), std::cell::OnceCell::new);
        Self {
            inner,
            member_holes: member_holes.to_vec(),
            wrapped,
        }
    }
}

impl ContentSourceSet for HoledSourceSet<'_> {
    fn get_source(&self, member_index: i32) -> Option<&dyn ContentSource> {
        if member_index < 0 {
            return None;
        }
        let idx = member_index as usize;
        let cell = self.wrapped.get(idx)?;
        cell.get_or_init(|| {
            let src = self.inner.get_source(member_index)?;
            Some(HoledSource::new(src, self.member_holes[idx].clone()))
        })
        .as_ref()
        .map(|h| h as &dyn ContentSource)
    }

    fn aborted(&self) -> bool {
        self.inner.aborted()
    }

    fn on_rar_volume_parsed(
        &self,
        member_name: &[u8],
        vol: &crate::rarreader::Volume,
        files: &[crate::rarreader::RarFile],
    ) {
        self.inner.on_rar_volume_parsed(member_name, vol, files);
    }
}

/// Crypto annotation for a run whose member bytes are RAR-encrypted ciphertext (`RunCrypto` in `ContentMap.h`).
#[derive(Clone)]
pub struct RunCrypto {
    pub crypto: Option<std::sync::Arc<RarCryptoContext>>,
    pub cipher_data_offset: i64,
    pub cipher_size: i64,
}

/// Where the inner content stream lives inside a member set (`ContentMap` in `ContentMap.h`).
#[derive(Clone, Default)]
pub struct ContentMap {
    inner_name: Vec<u8>,
    inner_size: i64,
    runs: Vec<NzbgetRsContentRun>,
    run_cryptos: Vec<RunCrypto>,
}

impl ContentMap {
    pub fn inner_name_bytes(&self) -> &[u8] {
        &self.inner_name
    }

    pub fn inner_name(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.inner_name)
    }

    pub fn set_inner_name_bytes(&mut self, name: &[u8]) {
        self.inner_name = name.to_vec();
    }

    pub fn set_inner_name(&mut self, name: &str) {
        self.inner_name = name.as_bytes().to_vec();
    }

    pub fn inner_size(&self) -> i64 {
        self.inner_size
    }

    pub fn set_inner_size(&mut self, size: i64) {
        self.inner_size = size;
    }

    pub fn runs(&self) -> &[NzbgetRsContentRun] {
        &self.runs
    }

    pub fn runs_mut(&mut self) -> &mut Vec<NzbgetRsContentRun> {
        &mut self.runs
    }

    pub fn run_cryptos(&self) -> &[RunCrypto] {
        &self.run_cryptos
    }

    pub fn run_cryptos_mut(&mut self) -> &mut Vec<RunCrypto> {
        &mut self.run_cryptos
    }

    pub fn run_crypto(&self, run_index: usize) -> Option<&RunCrypto> {
        self.run_cryptos
            .get(run_index)
            .filter(|rc| rc.crypto.is_some())
    }

    pub fn encrypted(&self) -> bool {
        !self.run_cryptos.is_empty()
    }

    pub fn cipher_stream_size(&self) -> i64 {
        let sizes: Vec<i64> = self.run_cryptos.iter().map(|rc| rc.cipher_size).collect();
        content_map_sum_cipher_sizes(&sizes)
    }

    pub fn map_cipher_range(&self, cipher_range: NzbgetRsByteRange) -> Vec<NzbgetRsMemberRange> {
        if cipher_range.offset < 0
            || cipher_range.size <= 0
            || self.run_cryptos.len() != self.runs.len()
        {
            return Vec::new();
        }
        let cipher_offsets: Vec<i64> = self
            .run_cryptos
            .iter()
            .map(|rc| rc.cipher_data_offset)
            .collect();
        let cipher_sizes: Vec<i64> = self.run_cryptos.iter().map(|rc| rc.cipher_size).collect();
        let has_crypto: Vec<u8> = self
            .run_cryptos
            .iter()
            .map(|rc| u8::from(rc.crypto.is_some()))
            .collect();
        content_map_cipher_range(
            &self.runs,
            &cipher_offsets,
            &cipher_sizes,
            &has_crypto,
            cipher_range,
        )
    }

    pub fn read_cipher_range(
        &self,
        sources: &dyn ContentSourceSet,
        cipher_range: NzbgetRsByteRange,
        buffer: &mut [u8],
    ) -> bool {
        if cipher_range.size == 0 {
            return true;
        }
        if (buffer.len() as i64) < cipher_range.size {
            return false;
        }
        let pieces = self.map_cipher_range(cipher_range);
        if pieces.is_empty() {
            return false;
        }
        let mut filled = 0usize;
        for piece in pieces {
            let Some(source) = sources.get_source(piece.member_index) else {
                return false;
            };
            let len = piece.size as usize;
            if !source.read(piece.offset, &mut buffer[filled..filled + len]) {
                return false;
            }
            filled += len;
        }
        true
    }

    pub fn read_inner_decrypted(
        &self,
        sources: &dyn ContentSourceSet,
        inner_range: NzbgetRsByteRange,
        buffer: &mut [u8],
    ) -> bool {
        let block_size = RarCryptoContext::CRYPTO_BLOCK_SIZE as i64;
        let (ok, empty, cipher_from, cipher_to) = content_map_calc_decrypt_cipher_bounds(
            inner_range.offset,
            inner_range.size,
            self.inner_size,
            block_size,
            self.cipher_stream_size(),
        );
        if !ok {
            return false;
        }
        if empty {
            return true;
        }
        if (buffer.len() as i64) < inner_range.size {
            return false;
        }
        let Some(rc) = self.run_crypto(0) else {
            return false;
        };
        let Some(crypto) = &rc.crypto else {
            return false;
        };
        let mut prev = [0u8; RarCryptoContext::CRYPTO_BLOCK_SIZE];
        if cipher_from > 0
            && !self.read_cipher_range(
                sources,
                NzbgetRsByteRange {
                    offset: cipher_from - block_size,
                    size: block_size,
                },
                &mut prev,
            )
        {
            return false;
        }
        let total_cipher = (cipher_to - cipher_from) as usize;
        let mut cipher = vec![0u8; total_cipher];
        let mut plain = vec![0u8; total_cipher];
        if !self.read_cipher_range(
            sources,
            NzbgetRsByteRange {
                offset: cipher_from,
                size: cipher_to - cipher_from,
            },
            &mut cipher,
        ) || !crypto.decrypt_range(
            if cipher_from > 0 { Some(&prev) } else { None },
            &cipher,
            &mut plain,
            (cipher_to - cipher_from) / block_size,
        ) {
            return false;
        }
        let rel = (inner_range.offset - cipher_from) as usize;
        let len = inner_range.size as usize;
        buffer[..len].copy_from_slice(&plain[rel..rel + len]);
        true
    }

    pub fn map_to_inner(
        &self,
        member_index: i32,
        member_range: NzbgetRsByteRange,
    ) -> Vec<NzbgetRsByteRange> {
        content_map_to_inner(&self.runs, member_index, member_range)
    }

    pub fn map_from_inner(&self, inner_range: NzbgetRsByteRange) -> Vec<NzbgetRsMemberRange> {
        content_map_from_inner(&self.runs, inner_range)
    }

    pub fn exclude_member(&mut self, member_index: i32) {
        let parallel = self.run_cryptos.len() == self.runs.len();
        let mut kept_runs = Vec::new();
        let mut kept_cryptos = Vec::new();
        for i in 0..self.runs.len() {
            if self.runs[i].member_index == member_index {
                continue;
            }
            kept_runs.push(self.runs[i]);
            if parallel {
                kept_cryptos.push(self.run_cryptos[i].clone());
            }
        }
        self.runs = kept_runs;
        if parallel {
            self.run_cryptos = kept_cryptos;
        }
    }

    /// Reads donor plaintext for `inner_range` (`StreamRepairController::ReadDonorInner` in `StreamRepair.cpp`),
    /// decrypting through the donor map when the donor is encrypted.
    pub fn read_donor_inner(
        &self,
        sources: &dyn ContentSourceSet,
        inner_range: NzbgetRsByteRange,
        buffer: &mut Vec<u8>,
    ) -> bool {
        if inner_range.size < 0 {
            return false;
        }
        buffer.resize(inner_range.size as usize, 0);
        if self.encrypted() {
            return self.read_inner_decrypted(sources, inner_range, buffer);
        }
        let mut covered: i64 = 0;
        for piece in self.map_from_inner(inner_range) {
            let inner_pos = self.map_to_inner(
                piece.member_index,
                NzbgetRsByteRange {
                    offset: piece.offset,
                    size: piece.size,
                },
            );
            if inner_pos.len() != 1 {
                return false;
            }
            let Some(source) = sources.get_source(piece.member_index) else {
                return false;
            };
            let rel = (inner_pos[0].offset - inner_range.offset) as usize;
            let len = piece.size as usize;
            if !source.read(piece.offset, &mut buffer[rel..rel + len]) {
                return false;
            }
            covered += piece.size;
        }
        covered == inner_range.size
    }

    /// Assembles raw ciphertext of the target's contiguous cipher space from its member files on disk
    /// (`StreamRepairController::ReadTargetCipher` in `StreamRepair.cpp`).
    pub fn read_target_cipher(
        &self,
        target_files: &mut dyn TargetFileSet,
        cipher_range: NzbgetRsByteRange,
        buffer: &mut [u8],
    ) -> bool {
        let pieces = self.map_cipher_range(cipher_range);
        if pieces.is_empty() && cipher_range.size > 0 {
            return false;
        }
        if (buffer.len() as i64) < cipher_range.size {
            return false;
        }
        let mut filled = 0usize;
        for piece in pieces {
            let len = piece.size as usize;
            if !target_files.read_file(
                piece.member_index,
                piece.offset,
                &mut buffer[filled..filled + len],
            ) {
                return false;
            }
            filled += len;
        }
        true
    }

    /// Writes `data` (bytes of `inner_range`) through the target map with M2 containment guards
    /// (`StreamRepairController::WriteInnerRange` in `StreamRepair.cpp`).
    /// Returns the number of bytes written, or `-1` when a write was blocked or failed.
    pub fn write_inner_range(
        &self,
        target_files: &mut dyn TargetFileSet,
        target_holes: &mut [Vec<NzbgetRsByteRange>],
        member_targets: &[i32],
        set_members: &[SetMember],
        inner_range: NzbgetRsByteRange,
        data: &[u8],
    ) -> i64 {
        let mut written: i64 = 0;
        for piece in self.map_from_inner(inner_range) {
            let piece_range = NzbgetRsByteRange {
                offset: piece.offset,
                size: piece.size,
            };
            let inner_pos = self.map_to_inner(piece.member_index, piece_range);
            let has_file = target_files.has_file(piece.member_index);
            if inner_pos.len() != 1 || !has_file {
                return -1;
            }
            let member_idx = piece.member_index as usize;
            let member_name = set_members
                .get(member_idx)
                .map_or(&[][..], |m| m.name.as_slice());
            let target_idx = member_targets.get(member_idx).copied().unwrap_or(-1);
            if target_idx < 0 || (target_idx as usize) >= target_holes.len() {
                let mut msg = b"Stream repair write outside a repair target blocked for ".to_vec();
                msg.extend_from_slice(member_name);
                target_files.on_message_bytes(TargetMessageKind::Warning, &msg);
                return -1;
            }
            let holes = &mut target_holes[target_idx as usize];
            let mut remainder = vec![piece_range];
            for &th in holes.iter() {
                remainder = subtract_covered(&remainder, th);
            }
            if !remainder.is_empty() {
                let mut msg = b"Stream repair write outside a captured hole blocked for ".to_vec();
                msg.extend_from_slice(member_name);
                target_files.on_message_bytes(TargetMessageKind::Warning, &msg);
                return -1;
            }
            let rel = (inner_pos[0].offset - inner_range.offset) as usize;
            let len = piece.size as usize;
            if rel.saturating_add(len) > data.len()
                || !target_files.write_file(
                    piece.member_index,
                    piece.offset,
                    &data[rel..rel + len],
                )
            {
                return -1;
            }
            *holes = subtract_covered(holes, piece_range);
            written += piece.size;
        }
        written
    }
}

/// Log severity emitted by inner-space verification and patching.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetMessageKind {
    Info = 0,
    Warning = 1,
    Error = 2,
    Detail = 3,
}

/// Target member files opened read-write on demand (`TargetSetFiles` in `StreamRepair.h`).
pub trait TargetFileSet {
    fn is_stopped(&self) -> bool {
        false
    }
    fn has_file(&mut self, member_index: i32) -> bool;
    fn read_file(&mut self, member_index: i32, offset: i64, buf: &mut [u8]) -> bool;
    fn compare_file(&mut self, member_index: i32, offset: i64, expected: &[u8]) -> bool {
        if expected.is_empty() {
            return self.read_file(member_index, offset, &mut []);
        }
        let mut buf = vec![0u8; expected.len().min(64 * 1024)];
        let mut pos = 0usize;
        while pos < expected.len() {
            let chunk = (expected.len() - pos).min(buf.len());
            if !self.read_file(member_index, offset + (pos as i64), &mut buf[..chunk]) {
                return false;
            }
            if buf[..chunk] != expected[pos..pos + chunk] {
                return false;
            }
            pos += chunk;
        }
        true
    }
    fn write_file(&mut self, member_index: i32, offset: i64, data: &[u8]) -> bool;
    fn on_message(&mut self, _kind: TargetMessageKind, _text: &str) {}
    fn on_message_bytes(&mut self, kind: TargetMessageKind, text: &[u8]) {
        self.on_message(kind, &String::from_utf8_lossy(text));
    }
}

/// Adapts a mapped donor to `ContentSource` (`DonorInnerPlaintextSource` in `StreamRepair.cpp`).
pub struct DonorInnerPlaintextSource<'a> {
    map: &'a ContentMap,
    sources: &'a dyn ContentSourceSet,
}

impl<'a> DonorInnerPlaintextSource<'a> {
    pub fn new(map: &'a ContentMap, sources: &'a dyn ContentSourceSet) -> Self {
        Self { map, sources }
    }
}

impl ContentSource for DonorInnerPlaintextSource<'_> {
    fn size(&self) -> i64 {
        self.map.inner_size()
    }

    fn read(&self, offset: i64, buf: &mut [u8]) -> bool {
        let mut scratch = Vec::new();
        if !self.map.read_donor_inner(
            self.sources,
            NzbgetRsByteRange {
                offset,
                size: buf.len() as i64,
            },
            &mut scratch,
        ) {
            return false;
        }
        buf.copy_from_slice(&scratch);
        true
    }
}

/// One target-side set the repair stage can work on (`RepairSetData` in `ContentMap.h`).
pub struct RepairSetData {
    pub set: MemberSet,
    pub map: Option<ContentMap>,
    pub inner_holes: Vec<NzbgetRsByteRange>,
    pub original_inner_holes: Vec<NzbgetRsByteRange>,
    pub skip_reason: String,
}

impl RepairSetData {
    pub const PROBE_COUNT: i32 = 2;
    pub const MIN_PROBE_COMPARE_BYTES: i64 = 16 * 1024;

    /// Builds probe windows hugging each current hole, clipped against `original_inner_holes`
    /// (`StreamRepairController::BuildProbeWindows` in `StreamRepair.cpp`).
    pub fn build_probe_windows(&self) -> Vec<NzbgetRsByteRange> {
        let Some(map) = &self.map else {
            return Vec::new();
        };
        build_probe_windows(
            map.inner_size(),
            &self.inner_holes,
            &self.original_inner_holes,
            Self::PROBE_COUNT,
            Self::MIN_PROBE_COMPARE_BYTES,
        )
    }

    /// Computes the plaintext-target compare floor (`StreamRepairController::PlainCompareFloor`).
    pub fn plain_compare_floor(&self, windows: &[NzbgetRsByteRange]) -> i64 {
        let Some(map) = &self.map else {
            return -1;
        };
        plain_compare_floor(map.runs(), &self.original_inner_holes, windows)
    }

    /// Verifies a plaintext target set against a plaintext `ContentSource` over `windows`.
    pub fn verify_plain_from_source(
        &self,
        plaintext: &dyn ContentSource,
        target_files: &mut dyn TargetFileSet,
        windows: &[NzbgetRsByteRange],
    ) -> bool {
        let Some(target_map) = &self.map else {
            return false;
        };
        let required = self.plain_compare_floor(windows);
        if required < 0 {
            return false;
        }
        let mut total_compared: i64 = 0;
        let mut saw_varied_data = false;
        let mut donor_data = Vec::new();

        for &window in windows {
            if target_files.is_stopped() {
                return false;
            }
            if total_compared >= required && saw_varied_data {
                break;
            }
            if window.size < 0 {
                continue;
            }
            donor_data.resize(window.size as usize, 0);
            if !plaintext.read(window.offset, &mut donor_data) {
                continue;
            }
            for piece in target_map.map_from_inner(window) {
                let inner_pos = target_map.map_to_inner(
                    piece.member_index,
                    NzbgetRsByteRange {
                        offset: piece.offset,
                        size: piece.size,
                    },
                );
                let has_file = target_files.has_file(piece.member_index);
                if inner_pos.len() != 1 || !has_file {
                    return false;
                }
                let rel = (inner_pos[0].offset - window.offset) as usize;
                let len = piece.size as usize;
                let piece_data = &donor_data[rel..rel + len];
                if !target_files.compare_file(piece.member_index, piece.offset, piece_data) {
                    return false;
                }
                total_compared += piece.size;
                if !saw_varied_data && piece.size > 1 {
                    saw_varied_data = buffer_has_varied_data(piece_data);
                }
            }
        }

        total_compared >= required && saw_varied_data
    }

    /// Encrypted-target probe in ciphertext space (`StreamRepairController::VerifyEncryptedFromSource`).
    pub fn verify_encrypted_from_source(
        &self,
        plaintext: &dyn ContentSource,
        target_files: &mut dyn TargetFileSet,
        windows: &[NzbgetRsByteRange],
    ) -> bool {
        const BLOCK_SIZE: i64 = RarCryptoContext::CRYPTO_BLOCK_SIZE as i64;
        let Some(target_map) = &self.map else {
            return false;
        };
        let Some(run_crypto) = target_map.run_crypto(0) else {
            return false;
        };
        let Some(crypto) = &run_crypto.crypto else {
            return false;
        };
        let last_computable = (target_map.inner_size() / BLOCK_SIZE) * BLOCK_SIZE;
        let required = encrypted_compare_floor(
            target_map.inner_size(),
            BLOCK_SIZE,
            target_map.runs(),
            &self.original_inner_holes,
            windows,
        );
        if required < 0 {
            return false;
        }

        let mut total_compared: i64 = 0;
        let mut saw_varied_data = false;
        let mut donor_data = Vec::new();
        let mut cipher = Vec::new();

        for &window in windows {
            if target_files.is_stopped() {
                return false;
            }
            if total_compared >= required && saw_varied_data {
                break;
            }
            let window_end_raw = range_end(window);
            let window_end_clamped = window_end_raw.min(last_computable);
            let block_from = encrypted_chain_start(
                window.offset,
                window_end_clamped,
                BLOCK_SIZE,
                &self.original_inner_holes,
            );
            let Some((window_end, from, block_to)) = calc_encrypted_window_bounds(
                window.offset,
                window_end_raw,
                last_computable,
                block_from,
                BLOCK_SIZE,
            ) else {
                continue;
            };
            let mut prev = [0u8; RarCryptoContext::CRYPTO_BLOCK_SIZE];
            if block_from > 0
                && !target_map.read_target_cipher(
                    target_files,
                    NzbgetRsByteRange {
                        offset: block_from - BLOCK_SIZE,
                        size: BLOCK_SIZE,
                    },
                    &mut prev,
                )
            {
                continue;
            }
            let block_len = (block_to - block_from) as usize;
            donor_data.resize(block_len, 0);
            if !plaintext.read(block_from, &mut donor_data) {
                continue;
            }
            cipher.resize(block_len, 0);
            if !crypto.encrypt_range(
                if block_from > 0 { Some(&prev) } else { None },
                &donor_data,
                &mut cipher,
                (block_to - block_from) / BLOCK_SIZE,
            ) {
                continue;
            }
            let pieces = target_map.map_cipher_range(NzbgetRsByteRange {
                offset: from,
                size: window_end - from,
            });
            if pieces.is_empty() {
                continue;
            }
            let mut seen = 0usize;
            let rel_start = (from - block_from) as usize;
            for piece in pieces {
                if !target_files.has_file(piece.member_index) {
                    return false;
                }
                let len = piece.size as usize;
                let expected = &cipher[rel_start + seen..rel_start + seen + len];
                if !target_files.compare_file(piece.member_index, piece.offset, expected) {
                    return false;
                }
                seen += len;
            }
            total_compared += window_end - from;
            let plain_window = &donor_data[rel_start..rel_start + (window_end - from) as usize];
            if !saw_varied_data && window_end - from > 1 {
                saw_varied_data = buffer_has_varied_data(plain_window);
            }
        }

        total_compared >= required && saw_varied_data
    }

    /// M3 entry: encrypted target verified against a mapped donor (`StreamRepairController::VerifyDonorSetEncrypted`).
    pub fn verify_donor_set_encrypted(
        &self,
        donor_map: &ContentMap,
        donor_sources: &dyn ContentSourceSet,
        target_files: &mut dyn TargetFileSet,
        windows: &[NzbgetRsByteRange],
    ) -> bool {
        let plaintext = DonorInnerPlaintextSource::new(donor_map, donor_sources);
        self.verify_encrypted_from_source(&plaintext, target_files, windows)
    }

    /// Verifies a mapped donor set against this target set (`StreamRepairController::VerifyDonorSet`).
    pub fn verify_donor_set(
        &self,
        donor_map: &ContentMap,
        donor_sources: &dyn ContentSourceSet,
        target_files: &mut dyn TargetFileSet,
    ) -> bool {
        let Some(target_map) = &self.map else {
            return false;
        };
        let windows = self.build_probe_windows();
        if target_map.encrypted() {
            return self.verify_donor_set_encrypted(
                donor_map,
                donor_sources,
                target_files,
                &windows,
            );
        }
        let plaintext = DonorInnerPlaintextSource::new(donor_map, donor_sources);
        self.verify_plain_from_source(&plaintext, target_files, &windows)
    }

    /// Encrypted-target patch core (`StreamRepairController::PatchEncryptedFromSource`).
    #[allow(clippy::too_many_arguments)]
    pub fn patch_encrypted_from_source(
        &mut self,
        plaintext: &dyn ContentSource,
        target_files: &mut dyn TargetFileSet,
        target_holes: &mut [Vec<NzbgetRsByteRange>],
        member_targets: &[i32],
        set_members: &[SetMember],
        donor_name: &str,
    ) -> i64 {
        const BLOCK_SIZE: i64 = RarCryptoContext::CRYPTO_BLOCK_SIZE as i64;
        let Some(target_map) = self.map.clone() else {
            return 0;
        };
        let Some(run_crypto) = target_map.run_crypto(0) else {
            return 0;
        };
        let Some(crypto) = &run_crypto.crypto else {
            return 0;
        };
        let inner_name = target_map.inner_name().into_owned();
        let last_computable = (target_map.inner_size() / BLOCK_SIZE) * BLOCK_SIZE;
        let mut written: i64 = 0;
        let mut uncomputable_tail: i64 = 0;
        let mut donor_data = Vec::new();
        let mut cipher = Vec::new();

        let holes = self.inner_holes.clone();
        for hole in holes {
            if target_files.is_stopped() {
                break;
            }
            let (ok, new_tail, _hole_from, _patch_to, mut block_from, block_to) =
                encrypted_clip_live_hole(
                    hole.offset,
                    hole.size,
                    &self.inner_holes,
                    last_computable,
                    BLOCK_SIZE,
                    uncomputable_tail,
                );
            uncomputable_tail = new_tail;
            if !ok {
                continue;
            }

            let mut prev = [0u8; RarCryptoContext::CRYPTO_BLOCK_SIZE];
            let mut have_prev = false;
            if block_from > 0 {
                let prev_present =
                    encrypted_is_prev_block_present(block_from, BLOCK_SIZE, &self.inner_holes);
                if !prev_present
                    || !target_map.read_target_cipher(
                        target_files,
                        NzbgetRsByteRange {
                            offset: block_from - BLOCK_SIZE,
                            size: BLOCK_SIZE,
                        },
                        &mut prev,
                    )
                {
                    target_files.on_message(
                        TargetMessageKind::Info,
                        &format!(
                            "Skipping a hole of {inner_name} for duplicate {donor_name}: predecessor cipher block is missing"
                        ),
                    );
                    continue;
                }
                have_prev = true;
            }

            while block_from < block_to && !target_files.is_stopped() {
                let (chunk_end, block_count) =
                    calc_encrypted_chunk_end(block_from, block_to, 4 * 1024 * 1024, BLOCK_SIZE);
                let chunk_len = (chunk_end - block_from) as usize;
                donor_data.resize(chunk_len, 0);
                if !plaintext.read(block_from, &mut donor_data) {
                    break;
                }
                cipher.resize(chunk_len, 0);
                if !crypto.encrypt_range(
                    if have_prev { Some(&prev) } else { None },
                    &donor_data,
                    &mut cipher,
                    block_count,
                ) {
                    break;
                }

                let (hole_parts, present_parts) =
                    partition_encrypted_chunk(block_from, chunk_end, &self.inner_holes);

                for part in present_parts {
                    let pieces = target_map.map_cipher_range(part);
                    if pieces.is_empty() {
                        target_files.on_message(
                            TargetMessageKind::Info,
                            &format!(
                                "Skipping packing of duplicate {donor_name} for {inner_name}: unmappable boundary bytes"
                            ),
                        );
                        return written;
                    }
                    let mut seen = 0usize;
                    let rel_start = (part.offset - block_from) as usize;
                    for piece in pieces {
                        let len = piece.size as usize;
                        let expected = &cipher[rel_start + seen..rel_start + seen + len];
                        if !target_files.has_file(piece.member_index)
                            || !target_files.compare_file(
                                piece.member_index,
                                piece.offset,
                                expected,
                            )
                        {
                            target_files.on_message(
                                TargetMessageKind::Info,
                                &format!(
                                    "Skipping packing of duplicate {donor_name} for {inner_name}: boundary block mismatch"
                                ),
                            );
                            return written;
                        }
                        seen += len;
                    }
                }

                for part in hole_parts {
                    let rel_start = (part.offset - block_from) as usize;
                    let part_len = part.size as usize;
                    let wrote = target_map.write_inner_range(
                        target_files,
                        target_holes,
                        member_targets,
                        set_members,
                        part,
                        &cipher[rel_start..rel_start + part_len],
                    );
                    if wrote < 0 {
                        return written;
                    }
                    written += wrote;
                    self.inner_holes = subtract_covered(&self.inner_holes, part);
                }

                prev.copy_from_slice(&cipher[chunk_len - (BLOCK_SIZE as usize)..chunk_len]);
                have_prev = true;
                block_from = chunk_end;
            }
        }

        if uncomputable_tail > 0 {
            target_files.on_message(
                TargetMessageKind::Info,
                &format!(
                    "Cross-packing repair of {inner_name}: {uncomputable_tail} byte(s) in the final partial cipher block stay for par-repair"
                ),
            );
        }
        written
    }

    /// Patches this target set from a mapped donor (`StreamRepairController::PatchFromDonorSet`
    /// and `StreamRepairController::PatchFromDonorSetEncrypted`).
    /// Returns `(written, credit_recovery)`.
    #[allow(clippy::too_many_arguments)]
    pub fn patch_from_donor_set(
        &mut self,
        donor_map: &ContentMap,
        donor_sources: &dyn ContentSourceSet,
        target_files: &mut dyn TargetFileSet,
        target_holes: &mut [Vec<NzbgetRsByteRange>],
        member_targets: &[i32],
        set_members: &[SetMember],
        donor_name: &str,
    ) -> (i64, bool) {
        let Some(target_map) = self.map.clone() else {
            return (0, false);
        };
        if target_map.encrypted() {
            let plaintext = DonorInnerPlaintextSource::new(donor_map, donor_sources);
            let written = self.patch_encrypted_from_source(
                &plaintext,
                target_files,
                target_holes,
                member_targets,
                set_members,
                donor_name,
            );
            return (written, written > 0);
        }

        const FULL_CHUNK: i64 = 4 * 1024 * 1024;
        const MIN_CHUNK: i64 = 64 * 1024;
        const MAX_FAILED_PIECES: i32 = 8;

        let mut written: i64 = 0;
        let mut donor_data = Vec::new();
        let holes = self.inner_holes.clone();

        for hole in holes {
            let mut target_hole = true;
            for piece in target_map.map_from_inner(hole) {
                let idx = piece.member_index as usize;
                target_hole &= member_targets.get(idx).copied().unwrap_or(-1) >= 0;
            }
            if !target_hole {
                continue;
            }

            let hole_end = range_end(hole);
            let mut pos = hole.offset;
            let mut size = FULL_CHUNK;
            let mut failed_pieces = 0;
            while pos < hole_end && !target_files.is_stopped() {
                let chunk = (hole_end - pos).min(size);
                let chunk_range = NzbgetRsByteRange {
                    offset: pos,
                    size: chunk,
                };
                if !donor_map.read_donor_inner(donor_sources, chunk_range, &mut donor_data) {
                    let (step_action, new_size, new_pos, new_failed) =
                        patch_donor_set_step_failure(
                            chunk,
                            MIN_CHUNK,
                            size,
                            pos,
                            failed_pieces,
                            MAX_FAILED_PIECES,
                        );
                    size = new_size;
                    pos = new_pos;
                    failed_pieces = new_failed;
                    if step_action == 0 {
                        break;
                    }
                    continue;
                }
                failed_pieces = 0;
                size = FULL_CHUNK;
                let wrote = target_map.write_inner_range(
                    target_files,
                    target_holes,
                    member_targets,
                    set_members,
                    chunk_range,
                    &donor_data,
                );
                if wrote < 0 {
                    return (written, false);
                }
                written += wrote;
                self.inner_holes = subtract_covered(&self.inner_holes, chunk_range);
                pos += chunk;
            }
        }

        (written, written > 0)
    }

    /// M4 identity probe for a plaintext target (`StreamRepairController::VerifyDonorInnerFile`).
    pub fn verify_donor_inner_file(
        &self,
        donor_inner: &dyn ContentSource,
        donor_inner_size: i64,
        target_files: &mut dyn TargetFileSet,
    ) -> bool {
        let Some(target_map) = &self.map else {
            return false;
        };
        if target_map.encrypted() || donor_inner_size != target_map.inner_size() {
            return false;
        }
        let windows = self.build_probe_windows();
        self.verify_plain_from_source(donor_inner, target_files, &windows)
    }

    /// M4 identity probe for an encrypted target (`StreamRepairController::VerifyDonorInnerFileEncrypted`).
    pub fn verify_donor_inner_file_encrypted(
        &self,
        donor_inner: &dyn ContentSource,
        donor_inner_size: i64,
        target_files: &mut dyn TargetFileSet,
    ) -> bool {
        let Some(target_map) = &self.map else {
            return false;
        };
        if !target_map.encrypted() || donor_inner_size != target_map.inner_size() {
            return false;
        }
        let windows = self.build_probe_windows();
        self.verify_encrypted_from_source(donor_inner, target_files, &windows)
    }

    /// Unified M4 identity probe (dispatches to encrypted or plaintext verification).
    pub fn verify_donor_inner(
        &self,
        donor_inner: &dyn ContentSource,
        donor_inner_size: i64,
        target_files: &mut dyn TargetFileSet,
    ) -> bool {
        let Some(target_map) = &self.map else {
            return false;
        };
        if target_map.encrypted() {
            self.verify_donor_inner_file_encrypted(donor_inner, donor_inner_size, target_files)
        } else {
            self.verify_donor_inner_file(donor_inner, donor_inner_size, target_files)
        }
    }

    /// M4 patch for a plaintext target (`StreamRepairController::PatchFromDonorInnerFile`).
    /// Returns `(written, holes_filled, credit_recovery)`.
    #[allow(clippy::too_many_arguments)]
    pub fn patch_from_donor_inner_file(
        &mut self,
        donor_inner: &dyn ContentSource,
        target_files: &mut dyn TargetFileSet,
        target_holes: &mut [Vec<NzbgetRsByteRange>],
        member_targets: &[i32],
        set_members: &[SetMember],
    ) -> (i64, i32, bool) {
        let Some(target_map) = self.map.clone() else {
            return (0, 0, false);
        };
        let mut written: i64 = 0;
        let mut holes_filled: i32 = 0;
        let mut donor_data = Vec::new();
        let holes = self.inner_holes.clone();

        for hole in holes {
            let hole_end = range_end(hole);
            let mut pos = hole.offset;
            while pos < hole_end && !target_files.is_stopped() {
                let chunk = (hole_end - pos).min(4 * 1024 * 1024);
                donor_data.resize(chunk as usize, 0);
                if !donor_inner.read(pos, &mut donor_data) {
                    break;
                }
                let chunk_range = NzbgetRsByteRange {
                    offset: pos,
                    size: chunk,
                };
                let wrote = target_map.write_inner_range(
                    target_files,
                    target_holes,
                    member_targets,
                    set_members,
                    chunk_range,
                    &donor_data,
                );
                if wrote < 0 {
                    return (written, holes_filled, false);
                }
                written += wrote;
                self.inner_holes = subtract_covered(&self.inner_holes, chunk_range);
                pos += chunk;
            }
            if pos >= hole_end {
                holes_filled += 1;
            }
        }

        (written, holes_filled, written > 0)
    }

    /// M4 patch for an encrypted target (`StreamRepairController::PatchFromDonorInnerFileEncrypted`).
    /// Returns `(written, holes_filled, credit_recovery)`.
    #[allow(clippy::too_many_arguments)]
    pub fn patch_from_donor_inner_file_encrypted(
        &mut self,
        donor_inner: &dyn ContentSource,
        target_files: &mut dyn TargetFileSet,
        target_holes: &mut [Vec<NzbgetRsByteRange>],
        member_targets: &[i32],
        set_members: &[SetMember],
        donor_name: &str,
    ) -> (i64, i32, bool) {
        let before = self.inner_holes.clone();
        let written = self.patch_encrypted_from_source(
            donor_inner,
            target_files,
            target_holes,
            member_targets,
            set_members,
            donor_name,
        );
        let mut holes_filled: i32 = 0;
        if written > 0 {
            for hole in before {
                if !intersects_any(hole, &self.inner_holes) {
                    holes_filled += 1;
                }
            }
        }
        (written, holes_filled, written > 0)
    }

    /// Unified M4 patch (dispatches to encrypted or plaintext patching).
    /// Returns `(written, holes_filled, credit_recovery)`.
    #[allow(clippy::too_many_arguments)]
    pub fn patch_from_donor_inner(
        &mut self,
        donor_inner: &dyn ContentSource,
        target_files: &mut dyn TargetFileSet,
        target_holes: &mut [Vec<NzbgetRsByteRange>],
        member_targets: &[i32],
        set_members: &[SetMember],
        donor_name: &str,
    ) -> (i64, i32, bool) {
        let encrypted = self.map.as_ref().is_some_and(ContentMap::encrypted);
        if encrypted {
            self.patch_from_donor_inner_file_encrypted(
                donor_inner,
                target_files,
                target_holes,
                member_targets,
                set_members,
                donor_name,
            )
        } else {
            self.patch_from_donor_inner_file(
                donor_inner,
                target_files,
                target_holes,
                member_targets,
                set_members,
            )
        }
    }
}

pub const DONOR_MAX_CACHED_PARTS: usize = 16;
pub const DONOR_MAX_PREFETCH_PARTS: usize = DONOR_MAX_CACHED_PARTS / 2;
pub const DONOR_MAX_RESOLVE_STEPS: usize = 6;

pub const DONOR_PROBE_COUNT: i32 = 2;
pub const DONOR_MAX_PROBE_FETCHES: i32 = 8;
pub const DONOR_PATCH_MARGIN_PARTS: i32 = 2;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FetchedDonorArticle {
    pub success: bool,
    pub offset: i64,
    pub file_size: i64,
    pub data: Vec<u8>,
}

/// Transport adapter for fetching and prefetching donor member articles on demand.
pub trait DonorArticleTransport {
    fn has_prefetch_batch(&self) -> bool;
    fn fetch_part(&mut self, part_index: i32) -> Option<FetchedDonorArticle>;
    fn begin_prefetch_batch(&mut self, part_indices: &[i32]);
    fn next_prefetch_part(&mut self) -> Option<FetchedDonorArticle>;
    fn cancel_prefetch_batch(&mut self);
}

/// Donor-side byte access to one member of a duplicate posting (`DonorMemberSource` in `StreamRepair.h`).
/// Articles are fetched and decoded on demand, offsets resolve through estimated ranges with
/// measured-drift correction, and recent articles stay cached up to `DONOR_MAX_CACHED_PARTS`.
#[derive(Debug, Default)]
pub struct DonorMemberSource {
    article_sizes: Vec<i32>,
    ranges: Vec<NzbgetRsByteRange>,
    cache: std::collections::BTreeMap<i32, FetchedDonorArticle>,
    cache_order: std::collections::VecDeque<i32>,
    served_parts: std::collections::BTreeSet<i32>,
    size: i64,
    drift: i64,
    bad: bool,
}

impl DonorMemberSource {
    pub fn new(article_sizes: Vec<i32>) -> Self {
        Self {
            article_sizes,
            ranges: Vec::new(),
            cache: std::collections::BTreeMap::new(),
            cache_order: std::collections::VecDeque::new(),
            served_parts: std::collections::BTreeSet::new(),
            size: -1,
            drift: 0,
            bad: false,
        }
    }

    pub fn take_served_parts(&mut self) -> i32 {
        let count = self.served_parts.len() as i32;
        self.served_parts.clear();
        count
    }

    pub fn store_fetched(&mut self, part_index: i32, mut fetched: FetchedDonorArticle) -> bool {
        let (invalid, mark_bad) = donor_check_fetched_part(
            fetched.success,
            fetched.data.len(),
            fetched.offset,
            fetched.file_size,
            self.size,
        );
        if invalid {
            self.bad = mark_bad;
            fetched.success = false;
        }
        if self.cache.len() >= DONOR_MAX_CACHED_PARTS {
            if let Some(oldest) = self.cache_order.pop_front() {
                self.cache.remove(&oldest);
            }
        }
        self.cache_order.push_back(part_index);
        self.cache.entry(part_index).or_insert(fetched).success
    }

    pub fn fetch_part(
        &mut self,
        transport: &mut dyn DonorArticleTransport,
        part_index: i32,
    ) -> Option<&FetchedDonorArticle> {
        if self.bad || part_index < 0 || (part_index as usize) >= self.article_sizes.len() {
            return None;
        }
        if self.cache.contains_key(&part_index) {
            let cached = self.cache.get(&part_index)?;
            return cached.success.then_some(cached);
        }
        let fetched = transport.fetch_part(part_index)?;
        if self.store_fetched(part_index, fetched) {
            self.cache.get(&part_index)
        } else {
            None
        }
    }

    pub fn ensure_init(&mut self, transport: &mut dyn DonorArticleTransport) -> bool {
        if self.size >= 0 {
            return true;
        }
        if self.bad || self.article_sizes.is_empty() {
            return false;
        }
        let (first_ok, first_file_size) = match self.fetch_part(transport, 0) {
            Some(first) if first.success && first.offset == 0 => (true, first.file_size),
            _ => (false, 0),
        };
        if !first_ok {
            self.bad = true;
            return false;
        }
        self.size = first_file_size;
        self.ranges = estimate_donor_ranges(&self.article_sizes, self.size);
        if self.ranges.is_empty() {
            self.bad = true;
            self.size = -1;
            return false;
        }
        true
    }

    pub fn size_with_transport(&mut self, transport: &mut dyn DonorArticleTransport) -> i64 {
        if self.ensure_init(transport) {
            self.size
        } else {
            0
        }
    }

    fn part_for_offset(
        &mut self,
        transport: &mut dyn DonorArticleTransport,
        offset: i64,
    ) -> Option<(i32, &FetchedDonorArticle)> {
        let mut index = donor_index_for_offset(offset, self.drift, &self.ranges);
        for _step in 0..DONOR_MAX_RESOLVE_STEPS {
            let (fetched_offset, fetched_len) = {
                let fetched = self.fetch_part(transport, index)?;
                (fetched.offset, fetched.data.len())
            };
            self.drift = fetched_offset - self.ranges[index as usize].offset;
            let (status, new_index) = donor_step_part_for_offset(
                offset,
                fetched_offset,
                fetched_len,
                index,
                self.ranges.len(),
            );
            if status == 0 {
                return None;
            }
            if status == 1 {
                index = new_index;
            } else {
                let fetched = self.cache.get(&index)?;
                return fetched.success.then_some((index, fetched));
            }
        }
        None
    }

    fn prefetch(&mut self, transport: &mut dyn DonorArticleTransport, offset: i64, size: i64) {
        if !transport.has_prefetch_batch() || self.bad || self.ranges.is_empty() || size <= 0 {
            return;
        }
        let first_est = donor_index_for_offset(offset, self.drift, &self.ranges);
        let last_est = donor_index_for_offset(offset + size - 1, self.drift, &self.ranges);
        let (first, last) = donor_calc_prefetch_bounds(first_est, last_est, self.ranges.len());
        let mut parts = Vec::new();
        let mut index = first;
        while index <= last && parts.len() < DONOR_MAX_PREFETCH_PARTS {
            if !self.cache.contains_key(&index) {
                parts.push(index);
            }
            index += 1;
        }
        if parts.len() < 2 {
            return;
        }
        transport.begin_prefetch_batch(&parts);
        for &part_index in &parts {
            let Some(fetched) = transport.next_prefetch_part() else {
                break;
            };
            self.store_fetched(part_index, fetched);
        }
        transport.cancel_prefetch_batch();
    }

    pub fn read_with_transport(
        &mut self,
        transport: &mut dyn DonorArticleTransport,
        offset: i64,
        buffer: &mut [u8],
    ) -> bool {
        let size = buffer.len() as i64;
        if !self.ensure_init(transport)
            || !(offset >= 0 && size >= 0 && offset <= self.size && size <= self.size - offset)
        {
            return false;
        }
        self.prefetch(transport, offset, size);
        let mut pos = offset;
        let end_offset = offset + size;
        let mut out_pos = 0usize;
        while pos < end_offset {
            let Some((part_index, fetched)) = self.part_for_offset(transport, pos) else {
                return false;
            };
            let (from, len) =
                donor_calc_read_chunk(pos, fetched.offset, fetched.data.len(), end_offset);
            let from_usize = from as usize;
            let len_usize = len as usize;
            buffer[out_pos..out_pos + len_usize]
                .copy_from_slice(&fetched.data[from_usize..from_usize + len_usize]);
            self.served_parts.insert(part_index);
            out_pos += len_usize;
            pos += len;
        }
        true
    }
}

/// Batch fetcher adapter for M1 per-file donor verification and patching.
pub trait DonorBatchClient {
    fn is_stopped(&self) -> bool;
    fn begin_batch(&mut self, part_indices: &[i32]);
    fn next_article(&mut self) -> Option<FetchedDonorArticle>;
    fn cancel_remaining(&mut self);
}

/// Target file reader for M1 per-file verification (`CompareToFile` in `StreamRepair.cpp`).
pub trait TargetFileReader {
    fn read_range(&mut self, offset: i64, buf: &mut [u8]) -> bool;

    fn compare_range(&mut self, offset: i64, expected: &[u8]) -> bool {
        if expected.is_empty() {
            return self.read_range(offset, &mut []);
        }
        const CHUNK: usize = 64 * 1024;
        let mut buf = vec![0u8; expected.len().min(CHUNK)];
        let mut done = 0usize;
        while done < expected.len() {
            let n = (expected.len() - done).min(CHUNK);
            if !self.read_range(offset + done as i64, &mut buf[..n]) {
                return false;
            }
            if buf[..n] != expected[done..done + n] {
                return false;
            }
            done += n;
        }
        true
    }
}

/// Target file writer and progress/message sink for M1 per-file patching (`PatchFromDonor`).
pub trait TargetFilePatcher {
    fn write_range(&mut self, offset: i64, data: &[u8]) -> bool;
    fn add_progress(&mut self, bytes: i64);
    fn emit_message_bytes(&mut self, kind: TargetMessageKind, text: &[u8]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DonorFilePatchResult {
    pub recovered_parts: i32,
    pub recovered_bytes: i64,
    pub credit_recovery: bool,
}

/// M1 per-file donor identity verification (`StreamRepairController::VerifyDonor`
/// and `StreamRepairController::CompareProbes` in `StreamRepair.cpp`).
pub fn verify_donor_file(
    target_decoded_size: i64,
    target_holes: &[NzbgetRsByteRange],
    donor_ranges: &[NzbgetRsByteRange],
    batch_client: &mut dyn DonorBatchClient,
    target_reader: &mut dyn TargetFileReader,
) -> bool {
    let (probe_parts, mut reserve) = select_verify_donor_probes(
        donor_ranges,
        target_holes,
        target_decoded_size,
        DONOR_PROBE_COUNT,
    );
    let hole_cmp_ranges: Vec<crate::streamcompare::Range> = target_holes
        .iter()
        .map(|h| crate::streamcompare::Range {
            offset: h.offset,
            size: h.size,
        })
        .collect();
    let donor_cmp_ranges: Vec<crate::streamcompare::Range> = donor_ranges
        .iter()
        .map(|d| crate::streamcompare::Range {
            offset: d.offset,
            size: d.size,
        })
        .collect();

    let mut total_compared: i64 = 0;
    let mut saw_varied_data = false;
    let mut attempted: Vec<i32> = Vec::new();
    let mut batch = probe_parts;

    while !batch.is_empty() {
        attempted.extend_from_slice(&batch);
        batch_client.begin_batch(&batch);

        let mut missing: i32 = 0;
        while let Some(fetched) = batch_client.next_article() {
            if batch_client.is_stopped() {
                batch_client.cancel_remaining();
                return false;
            }
            let (is_valid, is_missing) = check_probe_article(
                fetched.success,
                fetched.data.len(),
                fetched.file_size,
                fetched.offset,
                target_decoded_size,
            );
            if !is_valid {
                if is_missing {
                    missing += 1;
                    continue;
                }
                batch_client.cancel_remaining();
                return false;
            }

            let mut present = vec![NzbgetRsByteRange {
                offset: fetched.offset,
                size: fetched.data.len() as i64,
            }];
            for &hole in target_holes {
                present = subtract_covered(&present, hole);
            }

            for range in present {
                let rel_start = (range.offset - fetched.offset) as usize;
                let rel_end = rel_start + (range.size as usize);
                let range_data = &fetched.data[rel_start..rel_end];
                if !target_reader.compare_range(range.offset, range_data) {
                    batch_client.cancel_remaining();
                    return false;
                }
                total_compared += range.size;
                if !saw_varied_data && range.size > 1 {
                    saw_varied_data = buffer_has_varied_data(range_data);
                }
            }
        }

        let required_compare = crate::streamcompare::required_compare_floor(
            target_decoded_size,
            &hole_cmp_ranges,
            &donor_cmp_ranges,
            &attempted,
        );
        if batch_client.is_stopped() {
            return false;
        }
        if total_compared >= required_compare && saw_varied_data {
            return true;
        }

        batch = select_probe_replacements(
            missing,
            &reserve,
            DONOR_MAX_PROBE_FETCHES,
            attempted.len(),
        );
        for &part_index in &batch {
            if let Some(pos) = reserve.iter().position(|&p| p == part_index) {
                reserve.remove(pos);
            }
        }
    }

    false
}

/// M1 per-file hole patching from a verified donor (`StreamRepairController::PatchFromDonor`).
pub fn patch_from_donor_file(
    target_filename: &[u8],
    target_decoded_size: i64,
    target_holes: &mut Vec<NzbgetRsByteRange>,
    donor_ranges: &[NzbgetRsByteRange],
    donor_name: &[u8],
    batch_client: &mut dyn DonorBatchClient,
    target_patcher: &mut dyn TargetFilePatcher,
) -> DonorFilePatchResult {
    let mut recovered_parts: i32 = 0;
    let mut recovered_bytes: i64 = 0;
    let mut drift: i64 = 0;
    let mut tried = std::collections::BTreeSet::<i32>::new();
    let donor_patch_ranges: Vec<crate::streampatchparts::Range> = donor_ranges
        .iter()
        .map(|d| crate::streampatchparts::Range {
            offset: d.offset,
            size: d.size,
        })
        .collect();

    for _pass in 0..2 {
        if target_holes.is_empty() || batch_client.is_stopped() {
            break;
        }
        let shifted_holes: Vec<crate::streampatchparts::Range> = target_holes
            .iter()
            .map(|h| crate::streampatchparts::Range {
                offset: h.offset.wrapping_sub(drift),
                size: h.size,
            })
            .collect();
        let mut patch_parts = vec![0i32; donor_patch_ranges.len()];
        let patch_count = crate::streampatchparts::select(
            &donor_patch_ranges,
            &shifted_holes,
            DONOR_PATCH_MARGIN_PARTS,
            &mut patch_parts,
        );
        patch_parts.truncate(patch_count);

        let mut request_parts = Vec::with_capacity(patch_parts.len());
        for part_index in patch_parts {
            if tried.insert(part_index) {
                request_parts.push(part_index);
            }
        }
        batch_client.begin_batch(&request_parts);

        let mut fetched_any = false;
        let mut result_index = 0usize;
        while let Some(fetched) = batch_client.next_article() {
            let part_index = request_parts[result_index];
            result_index += 1;
            if batch_client.is_stopped() || target_holes.is_empty() {
                batch_client.cancel_remaining();
                break;
            }
            if !is_patch_article_usable(
                fetched.success,
                fetched.file_size,
                target_decoded_size,
                fetched.data.len(),
                fetched.offset,
            ) {
                continue;
            }

            fetched_any = true;
            drift = fetched.offset - donor_ranges[part_index as usize].offset;

            let mut written = Vec::new();
            for &hole in target_holes.iter() {
                if let Some((from, to)) = clip_hole_to_fetched(
                    hole.offset,
                    hole.size,
                    fetched.offset,
                    fetched.data.len(),
                ) {
                    let rel_start = (from - fetched.offset) as usize;
                    let rel_end = (to - fetched.offset) as usize;
                    if !target_patcher.write_range(from, &fetched.data[rel_start..rel_end]) {
                        batch_client.cancel_remaining();
                        return DonorFilePatchResult {
                            recovered_parts,
                            recovered_bytes,
                            credit_recovery: false,
                        };
                    }
                    written.push(NzbgetRsByteRange {
                        offset: from,
                        size: to - from,
                    });
                }
            }

            for &range in &written {
                *target_holes = subtract_covered(target_holes, range);
                recovered_bytes += range.size;
                target_patcher.add_progress(range.size);
            }

            if !written.is_empty() {
                recovered_parts += 1;
            }
        }

        if !fetched_any || drift == 0 {
            break;
        }
    }

    if recovered_parts > 0 {
        let mb = stream_repair_bytes_to_mb(recovered_bytes);
        let mut msg =
            format!("Recovered {mb:.1} MB ({recovered_parts} donor article(s)) of ").into_bytes();
        msg.extend_from_slice(target_filename);
        msg.extend_from_slice(b" from duplicate ");
        msg.extend_from_slice(donor_name);
        target_patcher.emit_message_bytes(TargetMessageKind::Info, &msg);
    }

    DonorFilePatchResult {
        recovered_parts,
        recovered_bytes,
        credit_recovery: recovered_parts > 0,
    }
}

/// Probes a whole-file donor's articles (`{0, 1, count / 2, count - 1}`) to learn a plausible
/// decoded file size (`StreamRepairController::RepairWholeFile` in `StreamRepair.cpp`).
pub fn probe_whole_file_donor_size(
    article_count: usize,
    target_encoded_size: i64,
    batch_client: &mut dyn DonorBatchClient,
) -> Option<i64> {
    if article_count == 0 {
        return None;
    }
    let size_probes = [0usize, 1, article_count / 2, article_count - 1];
    let mut probed = std::collections::BTreeSet::<usize>::new();
    for index in size_probes {
        if batch_client.is_stopped() || index >= article_count || !probed.insert(index) {
            continue;
        }
        let req = [index as i32];
        batch_client.begin_batch(&req);
        let sized = batch_client
            .next_article()
            .filter(|fetched| {
                fetched.success && decoded_size_plausible(fetched.file_size, target_encoded_size)
            })
            .map(|fetched| fetched.file_size);
        batch_client.cancel_remaining();
        if let Some(file_size) = sized {
            return Some(file_size);
        }
    }
    None
}

/// Callback boundary for materializing a donor archive set on disk
/// (`StreamRepairController::MaterializeDonorSet` in `StreamRepair.cpp`).
pub trait DonorMaterializeClient {
    fn is_stopped(&mut self) -> bool;
    fn open_member(&mut self, member_index: i32) -> bool;
    fn write_member(&mut self, offset: i64, data: &[u8]) -> bool;
    fn close_member(&mut self);
    fn begin_member_batch(&mut self, member_index: i32);
    fn next_article(&mut self) -> Option<FetchedDonorArticle>;
    fn cancel_remaining(&mut self);
    fn update_progress(&mut self, label: &[u8], stage_progress: i32);
    fn emit_message_bytes(&mut self, kind: TargetMessageKind, text: &[u8]);
}

/// Materializes all member files of `set_members` into `temp_dir` via `client`, enforcing
/// article validity, hole coverage, and `exceeds_decompress_cap`
/// (`StreamRepairController::MaterializeDonorSet` in `StreamRepair.cpp`).
pub fn materialize_donor_set(
    set_members: &[i32],
    file_names: &[&[u8]],
    file_article_counts: &[i32],
    donor_name: &[u8],
    total_bytes: &mut i64,
    client: &mut dyn DonorMaterializeClient,
) -> bool {
    let mut total_articles: i64 = 0;
    for &member_index in set_members {
        if member_index < 0 || (member_index as usize) >= file_article_counts.len() {
            return false;
        }
        let article_count = file_article_counts[member_index as usize];
        if article_count <= 0 {
            return false;
        }
        total_articles += article_count as i64;
    }

    let mut completed_articles: i64 = 0;
    let mut total_extent: i64 = 0;
    let mut member_number: i32 = 0;
    let set_member_count = set_members.len() as i32;

    for &member_index in set_members {
        member_number += 1;
        let member_idx = member_index as usize;
        let member_name = file_names.get(member_idx).copied().unwrap_or(b"");
        let member_article_count = file_article_counts[member_idx];

        if !client.open_member(member_index) {
            return false;
        }

        let mut member_extent: i64 = 0;
        let mut member_size: i64 = -1;
        let mut member_holes: Vec<NzbgetRsByteRange> = Vec::new();
        let mut delivered_articles: i32 = 0;

        let report_progress =
            |client: &mut dyn DonorMaterializeClient, delivered: i32, completed: i64, bytes: i64| {
                let label = format_materialize_progress(
                    donor_name,
                    member_number,
                    set_member_count,
                    delivered,
                    member_article_count,
                    bytes,
                );
                let stage_progress = if total_articles > 0 {
                    (completed * 1000 / total_articles) as i32
                } else {
                    0
                };
                client.update_progress(&label, stage_progress);
            };

        report_progress(client, delivered_articles, completed_articles, *total_bytes);
        client.begin_member_batch(member_index);

        while let Some(fetched) = client.next_article() {
            if client.is_stopped() {
                client.cancel_remaining();
                return false;
            }
            if !materialize_article_valid(
                fetched.success,
                fetched.data.len(),
                fetched.offset,
                fetched.file_size,
                member_size,
            ) {
                client.cancel_remaining();
                let mut msg = b"Stopping download of duplicate ".to_vec();
                msg.extend_from_slice(donor_name);
                msg.extend_from_slice(
                    format!(
                        ": article {} of {} in ",
                        delivered_articles + 1,
                        member_article_count
                    )
                    .as_bytes(),
                );
                msg.extend_from_slice(member_name);
                msg.extend_from_slice(b" is unavailable or invalid");
                client.emit_message_bytes(TargetMessageKind::Info, &msg);
                return false;
            }
            if member_size < 0 {
                member_size = fetched.file_size;
                member_holes.push(NzbgetRsByteRange {
                    offset: 0,
                    size: member_size,
                });
            }
            let write_end = fetched.offset + (fetched.data.len() as i64);
            if exceeds_decompress_cap(
                *total_bytes,
                fetched.data.len() as i64,
                total_extent,
                fetched.file_size,
            ) {
                client.cancel_remaining();
                return false;
            }
            if !client.write_member(fetched.offset, &fetched.data) {
                client.cancel_remaining();
                return false;
            }
            *total_bytes += fetched.data.len() as i64;
            member_extent = member_extent.max(write_end);
            member_holes = subtract_covered(
                &member_holes,
                NzbgetRsByteRange {
                    offset: fetched.offset,
                    size: fetched.data.len() as i64,
                },
            );
            delivered_articles += 1;
            completed_articles += 1;
            report_progress(client, delivered_articles, completed_articles, *total_bytes);
        }

        if client.is_stopped() {
            client.cancel_remaining();
            return false;
        }
        if member_size < 0 || !member_holes.is_empty() {
            client.cancel_remaining();
            let mut msg = b"Stopping download of duplicate ".to_vec();
            msg.extend_from_slice(donor_name);
            msg.extend_from_slice(b": ");
            msg.extend_from_slice(member_name);
            msg.extend_from_slice(b" has missing ranges after all listed articles");
            client.emit_message_bytes(TargetMessageKind::Info, &msg);
            return false;
        }
        total_extent += member_extent;
        client.close_member();
    }
    true
}

/// Pure-Rust `ContentMapper` mirroring `daemon/postprocess/ContentMap.h`.
pub struct ContentMapper;

impl ContentMapper {
    pub const SKIP_ENCRYPTED_DATA: &'static str = "encrypted archive data";
    pub const SKIP_ENCRYPTED_HEADERS: &'static str = "encrypted archive headers";

    pub fn is_compressible_archive(set: &MemberSet) -> bool {
        content_mapper_is_compressible_archive(set.format as i32)
    }

    pub fn group_sets(members: &[SetMember]) -> Vec<MemberSet> {
        let names: Vec<&[u8]> = members.iter().map(|m| m.name.as_slice()).collect();
        content_mapper_group_sets(&names)
            .into_iter()
            .filter_map(|(fmt, idxs)| {
                Some(MemberSet {
                    format: MemberFormat::from_i32(fmt)?,
                    members: idxs,
                })
            })
            .collect()
    }

    pub fn build_map(
        members: &[SetMember],
        set: &MemberSet,
        sources: &dyn ContentSourceSet,
        password: Option<&str>,
    ) -> Result<ContentMap, String> {
        Self::build_map_with_tls(members, set, sources, password.map(str::as_bytes), true)
    }

    pub fn build_map_with_tls(
        members: &[SetMember],
        set: &MemberSet,
        sources: &dyn ContentSourceSet,
        password: Option<&[u8]>,
        tls_enabled: bool,
    ) -> Result<ContentMap, String> {
        match set.format {
            MemberFormat::Bare => Self::build_bare_map(members, set, sources),
            MemberFormat::Split => Self::build_split_map(members, set, sources),
            MemberFormat::Rar => {
                Self::build_rar_map(members, set, sources, password, tls_enabled)
            }
            MemberFormat::Zip => Self::build_zip_map(set, sources),
            MemberFormat::SevenZip => Self::build_seven_zip_map(set, sources),
        }
    }

    fn build_bare_map(
        members: &[SetMember],
        set: &MemberSet,
        sources: &dyn ContentSourceSet,
    ) -> Result<ContentMap, String> {
        let member_index = set.members[0];
        let Some(source) = sources.get_source(member_index) else {
            return Err("member unreadable".to_string());
        };
        let sz = source.size();
        if sz <= 0 {
            return Err("member unreadable".to_string());
        }
        let mut map = ContentMap::default();
        map.set_inner_name_bytes(&members[member_index as usize].name);
        map.set_inner_size(sz);
        map.runs_mut().push(NzbgetRsContentRun {
            inner_offset: 0,
            member_index,
            member_offset: 0,
            size: sz,
        });
        Ok(map)
    }

    fn build_split_map(
        members: &[SetMember],
        set: &MemberSet,
        sources: &dyn ContentSourceSet,
    ) -> Result<ContentMap, String> {
        let first_name = &members[set.members[0] as usize].name;
        let inner_bytes = content_mapper_split_inner_name(first_name);
        if inner_bytes.is_empty() {
            return Err("inner file is not a media file".to_string());
        }
        let mut map = ContentMap::default();
        map.set_inner_name_bytes(&inner_bytes);
        let mut inner_offset = 0i64;
        for &member_index in &set.members {
            let Some(source) = sources.get_source(member_index) else {
                return Err(format!(
                    "member {} unreadable",
                    String::from_utf8_lossy(&members[member_index as usize].name)
                ));
            };
            let sz = source.size();
            if sz <= 0 {
                return Err(format!(
                    "member {} unreadable",
                    String::from_utf8_lossy(&members[member_index as usize].name)
                ));
            };
            map.runs_mut().push(NzbgetRsContentRun {
                inner_offset,
                member_index,
                member_offset: 0,
                size: sz,
            });
            inner_offset += sz;
        }
        map.set_inner_size(inner_offset);
        Ok(map)
    }

    fn build_rar_map(
        members: &[SetMember],
        set: &MemberSet,
        sources: &dyn ContentSourceSet,
        password: Option<&[u8]>,
        tls_enabled: bool,
    ) -> Result<ContentMap, String> {
        let mut volumes: Vec<Option<(crate::rarreader::Volume, Vec<crate::rarreader::RarFile>)>> =
            vec![None; set.members.len()];
        let mut unknown_volume: i32 = -1;

        for (i, &member_index) in set.members.iter().enumerate() {
            if sources.aborted() {
                return Err(String::new());
            }
            let source = sources.get_source(member_index);
            if sources.aborted() {
                return Err(String::new());
            }
            let mut vol = crate::rarreader::Volume {
                has_password: password.is_some_and(|pw| !pw.is_empty()),
                ..crate::rarreader::Volume::default()
            };
            let mut files = Vec::new();
            let parsed = if let Some(src) = source {
                let mut adapter = ContentSourceRarAdapter(src);
                let ok = if tls_enabled {
                    let mut cipher = NativeRarCipher::new(password.unwrap_or(&[]));
                    crate::rarreader::read_from(&mut vol, &mut adapter, &mut cipher, &mut files)
                } else {
                    let mut cipher = crate::rarreader::DisabledCipher;
                    crate::rarreader::read_from(&mut vol, &mut adapter, &mut cipher, &mut files)
                };
                if sources.aborted() {
                    return Err(String::new());
                }
                let member_name = members
                    .get(member_index as usize)
                    .map_or(&[][..], |m| m.name.as_slice());
                sources.on_rar_volume_parsed(member_name, &vol, &files);
                if sources.aborted() {
                    return Err(String::new());
                }
                ok
            } else {
                false
            };
            if parsed {
                volumes[i] = Some((vol, files));
                continue;
            }
            match content_mapper_classify_unreadable_rar_vol(
                source.is_some(),
                vol.encrypted,
                unknown_volume,
            ) {
                1 => return Err(Self::SKIP_ENCRYPTED_HEADERS.to_string()),
                2 => return Err("unreadable headers in more than one volume".to_string()),
                _ => unknown_volume = i as i32,
            }
        }

        let mut vol_readable = vec![0u8; set.members.len()];
        let mut vol_versions = vec![0i32; set.members.len()];
        let mut vol_member_indices = vec![0i32; set.members.len()];
        let mut vol_has_source = vec![0u8; set.members.len()];
        let mut vol_source_sizes = vec![0i64; set.members.len()];
        let mut entries = Vec::new();
        let mut entry_files: Vec<&crate::rarreader::RarFile> = Vec::new();

        for (i, &member_index) in set.members.iter().enumerate() {
            let source = sources.get_source(member_index);
            vol_readable[i] = u8::from(volumes[i].is_some());
            vol_versions[i] = volumes[i].as_ref().map_or(0, |(v, _)| v.version);
            vol_member_indices[i] = member_index;
            vol_has_source[i] = u8::from(source.is_some());
            vol_source_sizes[i] = source.map_or(0, |s| s.size());
            let Some((_, files)) = &volumes[i] else {
                continue;
            };
            for rf in files {
                let (has_crypt, crypt_version, crypt_kdf_count, crypt_salt, crypt_iv) =
                    if let Some(c) = &rf.crypt {
                        (true, c.version as i32, c.kdf_count as i32, c.salt, c.iv)
                    } else {
                        (false, 0, 0, [0u8; 16], [0u8; 16])
                    };
                entries.push(RarFileEntryInput {
                    volume_index: i as i32,
                    filename: &rf.filename,
                    size: rf.size,
                    packed_size: rf.packed_size,
                    data_offset: rf.data_offset,
                    stored: rf.stored,
                    encrypted_data: rf.encrypted_data,
                    has_salt: rf.has_salt,
                    salt: rf.salt,
                    has_crypt,
                    crypt_version,
                    crypt_kdf_count,
                    crypt_salt,
                    crypt_iv,
                });
                entry_files.push(rf);
            }
        }

        let mut crypto_ctx: Option<std::sync::Arc<RarCryptoContext>> = None;
        let pw_bytes = password.unwrap_or(&[]);
        let out = content_mapper_build_rar_map(
            unknown_volume,
            password.is_some(),
            &vol_readable,
            &vol_versions,
            &vol_member_indices,
            &vol_has_source,
            &vol_source_sizes,
            &entries,
            |entry_idx, version| {
                if !tls_enabled {
                    return false;
                }
                let Some(&rf) = entry_files.get(entry_idx) else {
                    return false;
                };
                if version == 3 {
                    crypto_ctx = RarCryptoContext::make_rar3(pw_bytes, &rf.salt)
                        .map(std::sync::Arc::new);
                } else if version == 5 {
                    if let Some(crypt) = &rf.crypt {
                        crypto_ctx = RarCryptoContext::make_rar5(pw_bytes, crypt)
                            .map(std::sync::Arc::new);
                    }
                }
                crypto_ctx.is_some()
            },
        )
        .map_err(str::to_string)?;

        let mut map = ContentMap::default();
        map.set_inner_name_bytes(&out.inner_name);
        map.set_inner_size(out.inner_size);
        for (i, run) in out.runs.into_iter().enumerate() {
            map.runs_mut().push(run);
            if out.encrypted {
                map.run_cryptos_mut().push(RunCrypto {
                    crypto: crypto_ctx.clone(),
                    cipher_data_offset: out.cipher_data_offsets[i],
                    cipher_size: out.cipher_sizes[i],
                });
            }
        }
        Ok(map)
    }

    fn build_zip_map(
        set: &MemberSet,
        sources: &dyn ContentSourceSet,
    ) -> Result<ContentMap, String> {
        let mut member_sizes = Vec::with_capacity(set.members.len());
        for &member_index in &set.members {
            let Some(source) = sources.get_source(member_index) else {
                return Err("member unreadable".to_string());
            };
            let sz = source.size();
            if sz <= 0 {
                return Err("member unreadable".to_string());
            }
            member_sizes.push(sz);
        }
        let (inner_name, inner_size, runs) = content_mapper_build_zip_map(
            &set.members,
            &member_sizes,
            |member_index, offset, buf| {
                sources
                    .get_source(member_index)
                    .is_some_and(|s| s.read(offset, buf))
            },
        )
        .map_err(str::to_string)?;
        let mut map = ContentMap::default();
        map.set_inner_name_bytes(&inner_name);
        map.set_inner_size(inner_size);
        *map.runs_mut() = runs;
        Ok(map)
    }

    fn build_seven_zip_map(
        set: &MemberSet,
        sources: &dyn ContentSourceSet,
    ) -> Result<ContentMap, String> {
        let mut member_sizes = Vec::with_capacity(set.members.len());
        for &member_index in &set.members {
            let Some(source) = sources.get_source(member_index) else {
                return Err("member unreadable".to_string());
            };
            let sz = source.size();
            if sz <= 0 {
                return Err("member unreadable".to_string());
            }
            member_sizes.push(sz);
        }
        let (inner_name, inner_size, runs) = content_mapper_build_seven_zip_map(
            &set.members,
            &member_sizes,
            |member_index, offset, buf| {
                sources
                    .get_source(member_index)
                    .is_some_and(|s| s.read(offset, buf))
            },
        )
        .map_err(str::to_string)?;
        let mut map = ContentMap::default();
        map.set_inner_name_bytes(&inner_name);
        map.set_inner_size(inner_size);
        *map.runs_mut() = runs;
        Ok(map)
    }

    pub fn build_repair_sets(
        members: &[SetMember],
        member_holes: &[Vec<NzbgetRsByteRange>],
        sources: &dyn ContentSourceSet,
        password: Option<&str>,
    ) -> Vec<RepairSetData> {
        Self::build_repair_sets_with_tls(
            members,
            member_holes,
            sources,
            password.map(str::as_bytes),
            true,
        )
    }

    pub fn build_repair_sets_with_tls(
        members: &[SetMember],
        member_holes: &[Vec<NzbgetRsByteRange>],
        sources: &dyn ContentSourceSet,
        password: Option<&[u8]>,
        tls_enabled: bool,
    ) -> Vec<RepairSetData> {
        let mut repair_sets = Vec::new();
        for set in Self::group_sets(members) {
            let any_holes = set
                .members
                .iter()
                .any(|&idx| !member_holes[idx as usize].is_empty());
            if !any_holes {
                continue;
            }
            let mut data = RepairSetData {
                set: set.clone(),
                map: None,
                inner_holes: Vec::new(),
                original_inner_holes: Vec::new(),
                skip_reason: String::new(),
            };
            match Self::build_map_with_tls(members, &set, sources, password, tls_enabled) {
                Ok(map) => {
                    if sources.aborted() {
                        break;
                    }
                    let mut hole_members = Vec::new();
                    let mut hole_ranges = Vec::new();
                    for &member_index in &set.members {
                        for &hole in &member_holes[member_index as usize] {
                            hole_members.push(member_index);
                            hole_ranges.push(hole);
                        }
                    }
                    data.inner_holes = content_mapper_collect_inner_holes(
                        map.runs(),
                        &hole_members,
                        &hole_ranges,
                    );
                    data.original_inner_holes = data.inner_holes.clone();
                    if data.inner_holes.is_empty() {
                        data.skip_reason =
                            "holes lie outside the mappable inner stream".to_string();
                    } else {
                        data.map = Some(map);
                    }
                }
                Err(reason) => {
                    if sources.aborted() {
                        break;
                    }
                    data.skip_reason = reason;
                }
            }
            repair_sets.push(data);
        }
        repair_sets
    }

    pub fn coalesce_ranges(ranges: &[NzbgetRsByteRange]) -> Vec<NzbgetRsByteRange> {
        content_mapper_coalesce_ranges(ranges)
    }
}

/// Read-only on-disk `ContentSource` (`DiskContentSource` in `ContentMap.h`).
pub struct DiskContentSource {
    file: std::cell::RefCell<std::fs::File>,
    size: i64,
}

impl DiskContentSource {
    pub fn new(file: std::fs::File, size: i64) -> Self {
        Self {
            file: std::cell::RefCell::new(file),
            size,
        }
    }
}

impl ContentSource for DiskContentSource {
    fn size(&self) -> i64 {
        self.size
    }

    fn read(&self, offset: i64, buf: &mut [u8]) -> bool {
        use std::io::{Read, Seek, SeekFrom};
        let size = buf.len() as i64;
        if offset < 0 || size < 0 || offset > self.size || size > self.size - offset {
            return false;
        }
        let mut file = self.file.borrow_mut();
        if file.seek(SeekFrom::Start(offset as u64)).is_err() {
            return false;
        }
        if size == 0 {
            return true;
        }
        file.read_exact(buf).is_ok()
    }
}

fn file_size_bytes(path_bytes: &[u8]) -> i64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(crate::fsops::path(path_bytes)).map_or(-1, |m| m.size() as i64)
}

fn write_error_message_bytes(err: &std::io::Error) -> Vec<u8> {
    let errno = err
        .raw_os_error()
        .unwrap_or_else(crate::fsops::last_errno);
    if errno != 0 {
        let msg = crate::fsops::error_message(errno);
        if !msg.is_empty() {
            return msg;
        }
    }
    b"incomplete write (disk full?)".to_vec()
}

/// Target-side member files in `dest_dir`, opened read-only on demand (`DiskSourceSet` in `StreamRepair.h`).
pub struct DiskSourceSet<'a> {
    dest_dir: Vec<u8>,
    members: &'a [SetMember],
    entries: Vec<std::cell::OnceCell<Option<DiskContentSource>>>,
    aborted_cb: Option<&'a dyn Fn() -> bool>,
    rar_volume_cb:
        Option<&'a dyn Fn(&[u8], &crate::rarreader::Volume, &[crate::rarreader::RarFile])>,
}

impl<'a> DiskSourceSet<'a> {
    pub fn new(dest_dir: &[u8], members: &'a [SetMember]) -> Self {
        Self::with_callbacks(dest_dir, members, None, None)
    }

    pub fn with_callbacks(
        dest_dir: &[u8],
        members: &'a [SetMember],
        aborted_cb: Option<&'a dyn Fn() -> bool>,
        rar_volume_cb: Option<
            &'a dyn Fn(&[u8], &crate::rarreader::Volume, &[crate::rarreader::RarFile]),
        >,
    ) -> Self {
        let mut entries = Vec::with_capacity(members.len());
        entries.resize_with(members.len(), std::cell::OnceCell::new);
        Self {
            dest_dir: dest_dir.to_vec(),
            members,
            entries,
            aborted_cb,
            rar_volume_cb,
        }
    }
}

impl ContentSourceSet for DiskSourceSet<'_> {
    fn get_source(&self, member_index: i32) -> Option<&dyn ContentSource> {
        if member_index < 0 {
            return None;
        }
        let idx = member_index as usize;
        let cell = self.entries.get(idx)?;
        cell.get_or_init(|| {
            let member = self.members.get(idx)?;
            let path_bytes = crate::artwritercore::article_writer_format_dest_file_path(
                &self.dest_dir,
                std::path::MAIN_SEPARATOR as u8,
                &member.name,
            );
            let size = file_size_bytes(&path_bytes).max(member.size);
            if size <= 0 {
                return None;
            }
            let file = std::fs::File::open(crate::fsops::path(&path_bytes)).ok()?;
            Some(DiskContentSource::new(file, size))
        })
        .as_ref()
        .map(|s| s as &dyn ContentSource)
    }

    fn aborted(&self) -> bool {
        self.aborted_cb.is_some_and(|cb| cb())
    }

    fn on_rar_volume_parsed(
        &self,
        member_name: &[u8],
        vol: &crate::rarreader::Volume,
        files: &[crate::rarreader::RarFile],
    ) {
        if let Some(cb) = self.rar_volume_cb {
            cb(member_name, vol, files);
        }
    }
}

/// Target member files in `dest_dir` opened read-write on demand for verify and patch
/// (`TargetSetFiles` in `StreamRepair.h`).
pub struct DiskTargetFileSet {
    dest_dir: Vec<u8>,
    files: std::collections::BTreeMap<i32, Option<std::fs::File>>,
}

impl DiskTargetFileSet {
    pub fn new(dest_dir: &[u8]) -> Self {
        Self {
            dest_dir: dest_dir.to_vec(),
            files: std::collections::BTreeMap::new(),
        }
    }

    pub fn get_file(
        &mut self,
        members: &[SetMember],
        member_index: i32,
    ) -> Option<&mut std::fs::File> {
        if !self.files.contains_key(&member_index) {
            let opened = if member_index >= 0 {
                members.get(member_index as usize).and_then(|member| {
                    let path_bytes = crate::artwritercore::article_writer_format_dest_file_path(
                        &self.dest_dir,
                        std::path::MAIN_SEPARATOR as u8,
                        &member.name,
                    );
                    std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(crate::fsops::path(&path_bytes))
                        .ok()
                })
            } else {
                None
            };
            self.files.insert(member_index, opened);
        }
        self.files.get_mut(&member_index).and_then(Option::as_mut)
    }
}

/// Target descriptor passed when initializing [`CrossPackRepairSession`].
#[derive(Clone, Debug)]
pub struct CrossPackTargetInput {
    pub filename: Vec<u8>,
    pub decoded_file_size: i64,
    pub patch_eligible: bool,
    pub holes: Vec<NzbgetRsByteRange>,
}

/// Host callback boundary for [`CrossPackRepairSession::process_donor`].
pub trait CrossPackDonorEnv {
    fn is_stopped(&mut self) -> bool;
    fn has_batch_fetcher(&self) -> bool {
        true
    }
    fn fetch_donor_part(&mut self, member_index: i32, part_index: i32) -> Option<FetchedDonorArticle>;
    fn begin_donor_prefetch(&mut self, member_index: i32, part_indices: &[i32]);
    fn next_donor_prefetch(&mut self) -> Option<FetchedDonorArticle>;
    fn cancel_donor_prefetch(&mut self);
    fn begin_materialize_member(&mut self, member_index: i32);
    fn next_materialize_article(&mut self) -> Option<FetchedDonorArticle>;
    fn cancel_materialize_member(&mut self);
    fn set_progress(&mut self, label: &[u8], stage_progress: i32);
    fn set_materializing(&mut self, delta: i32);
    fn add_recovered(&mut self, bytes: i64, holes: i32);
    fn sync_target_holes(&mut self, _target_holes: &[Vec<NzbgetRsByteRange>]) {}
    fn emit_message_bytes(&mut self, kind: TargetMessageKind, text: &[u8]);
    fn emit_extractor_log(&mut self, kind: i32, msg: &str) {
        let target_kind = match kind {
            1 => TargetMessageKind::Error,
            2 => TargetMessageKind::Warning,
            _ => TargetMessageKind::Info,
        };
        self.emit_message_bytes(target_kind, msg.as_bytes());
    }
    fn on_rar_volume_parsed(
        &mut self,
        _member_name: &[u8],
        _vol: &crate::rarreader::Volume,
        _files: &[crate::rarreader::RarFile],
    ) {
    }
}

struct DonorSetShared<'a, 'b> {
    env: &'a std::cell::RefCell<&'b mut dyn CrossPackDonorEnv>,
    fetch_budget: std::cell::Cell<Option<i32>>,
    use_batch_fetcher: std::cell::Cell<bool>,
}

struct MemberTransportAdapter<'a, 'b, 'c> {
    shared: &'c DonorSetShared<'a, 'b>,
    member_index: i32,
}

impl DonorArticleTransport for MemberTransportAdapter<'_, '_, '_> {
    fn has_prefetch_batch(&self) -> bool {
        self.shared.use_batch_fetcher.get()
            && self.shared.fetch_budget.get().is_none()
            && self.shared.env.borrow().has_batch_fetcher()
    }

    fn fetch_part(&mut self, part_index: i32) -> Option<FetchedDonorArticle> {
        if let Some(budget) = self.shared.fetch_budget.get() {
            if budget <= 0 {
                return None;
            }
            self.shared.fetch_budget.set(Some(budget - 1));
        }
        self.shared
            .env
            .borrow_mut()
            .fetch_donor_part(self.member_index, part_index)
    }

    fn begin_prefetch_batch(&mut self, part_indices: &[i32]) {
        self.shared
            .env
            .borrow_mut()
            .begin_donor_prefetch(self.member_index, part_indices);
    }

    fn next_prefetch_part(&mut self) -> Option<FetchedDonorArticle> {
        self.shared.env.borrow_mut().next_donor_prefetch()
    }

    fn cancel_prefetch_batch(&mut self) {
        self.shared.env.borrow_mut().cancel_donor_prefetch();
    }
}

struct DonorSetMemberSlot<'a, 'b> {
    member_index: i32,
    shared: std::rc::Rc<DonorSetShared<'a, 'b>>,
    inner: std::cell::RefCell<DonorMemberSource>,
}

impl ContentSource for DonorSetMemberSlot<'_, '_> {
    fn size(&self) -> i64 {
        let mut adapter = MemberTransportAdapter {
            shared: &self.shared,
            member_index: self.member_index,
        };
        self.inner.borrow_mut().size_with_transport(&mut adapter)
    }

    fn read(&self, offset: i64, buf: &mut [u8]) -> bool {
        let mut adapter = MemberTransportAdapter {
            shared: &self.shared,
            member_index: self.member_index,
        };
        self.inner
            .borrow_mut()
            .read_with_transport(&mut adapter, offset, buf)
    }
}

struct DonorSetSources<'a, 'b> {
    shared: std::rc::Rc<DonorSetShared<'a, 'b>>,
    article_sizes: Vec<Vec<i32>>,
    slots: Vec<std::cell::OnceCell<DonorSetMemberSlot<'a, 'b>>>,
}

impl<'a, 'b> DonorSetSources<'a, 'b> {
    fn new(
        env: &'a std::cell::RefCell<&'b mut dyn CrossPackDonorEnv>,
        article_sizes: Vec<Vec<i32>>,
    ) -> Self {
        let mut slots = Vec::with_capacity(article_sizes.len());
        slots.resize_with(article_sizes.len(), std::cell::OnceCell::new);
        let shared = std::rc::Rc::new(DonorSetShared {
            env,
            fetch_budget: std::cell::Cell::new(None),
            use_batch_fetcher: std::cell::Cell::new(true),
        });
        Self {
            shared,
            article_sizes,
            slots,
        }
    }

    fn set_fetch_budget(&self, budget: Option<i32>) {
        self.shared.fetch_budget.set(budget);
    }

    fn take_served_parts(&self) -> i32 {
        let mut total = 0i32;
        for cell in &self.slots {
            if let Some(slot) = cell.get() {
                total += slot.inner.borrow_mut().take_served_parts();
            }
        }
        total
    }
}

impl ContentSourceSet for DonorSetSources<'_, '_> {
    fn get_source(&self, member_index: i32) -> Option<&dyn ContentSource> {
        if member_index < 0 {
            return None;
        }
        let idx = member_index as usize;
        let cell = self.slots.get(idx)?;
        let slot = cell.get_or_init(|| DonorSetMemberSlot {
            member_index,
            shared: std::rc::Rc::clone(&self.shared),
            inner: std::cell::RefCell::new(DonorMemberSource::new(
                self.article_sizes[idx].clone(),
            )),
        });
        Some(slot as &dyn ContentSource)
    }

    fn aborted(&self) -> bool {
        self.shared.env.borrow_mut().is_stopped()
    }

    fn on_rar_volume_parsed(
        &self,
        member_name: &[u8],
        vol: &crate::rarreader::Volume,
        files: &[crate::rarreader::RarFile],
    ) {
        self.shared
            .env
            .borrow_mut()
            .on_rar_volume_parsed(member_name, vol, files);
    }
}

struct DiskTargetFileSetAdapter<'a, 'b, 'c> {
    files: &'a mut DiskTargetFileSet,
    members: &'a [SetMember],
    env: &'b std::cell::RefCell<&'c mut dyn CrossPackDonorEnv>,
}

impl TargetFileSet for DiskTargetFileSetAdapter<'_, '_, '_> {
    fn is_stopped(&self) -> bool {
        self.env.borrow_mut().is_stopped()
    }

    fn has_file(&mut self, member_index: i32) -> bool {
        self.files.get_file(self.members, member_index).is_some()
    }

    fn read_file(&mut self, member_index: i32, offset: i64, buf: &mut [u8]) -> bool {
        use std::io::{Read, Seek, SeekFrom};
        if offset < 0 {
            return false;
        }
        let Some(file) = self.files.get_file(self.members, member_index) else {
            return false;
        };
        let Ok(pos) = file.seek(SeekFrom::Start(offset as u64)) else {
            return false;
        };
        if pos != offset as u64 {
            return false;
        }
        if buf.is_empty() {
            return true;
        }
        file.read_exact(buf).is_ok()
    }

    fn write_file(&mut self, member_index: i32, offset: i64, data: &[u8]) -> bool {
        use std::io::{Seek, SeekFrom, Write};
        if offset < 0 {
            return false;
        }
        let Some(file) = self.files.get_file(self.members, member_index) else {
            return false;
        };
        let io_res = file
            .seek(SeekFrom::Start(offset as u64))
            .and_then(|pos| {
                if pos != offset as u64 {
                    Err(std::io::Error::other("seek mismatch"))
                } else {
                    file.write_all(data).and_then(|()| file.flush())
                }
            });
        if let Err(err) = io_res {
            let err_msg = write_error_message_bytes(&err);
            let member_name = self
                .members
                .get(member_index as usize)
                .map_or(&[][..], |m| m.name.as_slice());
            let mut msg = b"Could not write to ".to_vec();
            msg.extend_from_slice(member_name);
            msg.extend_from_slice(b" during stream repair: ");
            msg.extend_from_slice(&err_msg);
            self.env
                .borrow_mut()
                .emit_message_bytes(TargetMessageKind::Error, &msg);
            return false;
        }
        true
    }

    fn on_message(&mut self, kind: TargetMessageKind, text: &str) {
        self.env
            .borrow_mut()
            .emit_message_bytes(kind, text.as_bytes());
    }

    fn on_message_bytes(&mut self, kind: TargetMessageKind, text: &[u8]) {
        self.env.borrow_mut().emit_message_bytes(kind, text);
    }
}

struct MaterializeClientAdapter<'a, 'b, 'c> {
    temp_dir: &'a [u8],
    donor_members: &'a [SetMember],
    env: &'b std::cell::RefCell<&'c mut dyn CrossPackDonorEnv>,
    current_file: Option<std::fs::File>,
    current_path: Vec<u8>,
}

impl DonorMaterializeClient for MaterializeClientAdapter<'_, '_, '_> {
    fn is_stopped(&mut self) -> bool {
        self.env.borrow_mut().is_stopped()
    }

    fn open_member(&mut self, member_index: i32) -> bool {
        let member_name = self
            .donor_members
            .get(member_index as usize)
            .map_or(&[][..], |m| m.name.as_slice());
        self.current_path = crate::artwritercore::article_writer_format_dest_file_path(
            self.temp_dir,
            std::path::MAIN_SEPARATOR as u8,
            member_name,
        );
        match std::fs::File::create(crate::fsops::path(&self.current_path)) {
            Ok(f) => {
                self.current_file = Some(f);
                true
            }
            Err(err) => {
                let errno = err
                    .raw_os_error()
                    .unwrap_or_else(crate::fsops::last_errno);
                let err_msg = crate::fsops::error_message(errno);
                let mut msg = b"Could not create ".to_vec();
                msg.extend_from_slice(&self.current_path);
                msg.extend_from_slice(b" for donor materialization: ");
                msg.extend_from_slice(&err_msg);
                self.env
                    .borrow_mut()
                    .emit_message_bytes(TargetMessageKind::Warning, &msg);
                false
            }
        }
    }

    fn write_member(&mut self, offset: i64, data: &[u8]) -> bool {
        use std::io::{Seek, SeekFrom, Write};
        if offset < 0 {
            return false;
        }
        let Some(file) = self.current_file.as_mut() else {
            return false;
        };
        let io_res = file
            .seek(SeekFrom::Start(offset as u64))
            .and_then(|pos| {
                if pos != offset as u64 {
                    Err(std::io::Error::other("seek mismatch"))
                } else {
                    file.write_all(data).and_then(|()| file.flush())
                }
            });
        if let Err(err) = io_res {
            let err_msg = write_error_message_bytes(&err);
            let mut msg = b"Could not write to ".to_vec();
            msg.extend_from_slice(&self.current_path);
            msg.extend_from_slice(b" during donor materialization: ");
            msg.extend_from_slice(&err_msg);
            self.env
                .borrow_mut()
                .emit_message_bytes(TargetMessageKind::Warning, &msg);
            return false;
        }
        true
    }

    fn close_member(&mut self) {
        self.current_file = None;
    }

    fn begin_member_batch(&mut self, member_index: i32) {
        self.env
            .borrow_mut()
            .begin_materialize_member(member_index);
    }

    fn next_article(&mut self) -> Option<FetchedDonorArticle> {
        self.env.borrow_mut().next_materialize_article()
    }

    fn cancel_remaining(&mut self) {
        self.env.borrow_mut().cancel_materialize_member();
    }

    fn update_progress(&mut self, label: &[u8], stage_progress: i32) {
        self.env
            .borrow_mut()
            .set_progress(label, stage_progress);
    }

    fn emit_message_bytes(&mut self, kind: TargetMessageKind, text: &[u8]) {
        self.env.borrow_mut().emit_message_bytes(kind, text);
    }
}

/// Pure-Rust cross-packing (M2/M3) and decompression-assisted (M4) stream repair session
/// (`StreamRepairController::ExecCrossPackRepair` and `ExecDecompressRepair` in `StreamRepair.h`).
pub struct CrossPackRepairSession {
    dest_dir: Vec<u8>,
    set_members: Vec<SetMember>,
    member_targets: Vec<i32>,
    target_holes: Vec<Vec<NzbgetRsByteRange>>,
    repair_sets: Vec<RepairSetData>,
    target_files: DiskTargetFileSet,
    decompress_tried: std::collections::BTreeSet<(usize, Vec<u8>)>,
    tls_enabled: bool,
}

impl CrossPackRepairSession {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dest_dir: &[u8],
        member_names: &[&[u8]],
        targets: &[CrossPackTargetInput],
        target_password: Option<&[u8]>,
        tls_enabled: bool,
        aborted_cb: Option<&dyn Fn() -> bool>,
        emit_msg: &mut dyn FnMut(TargetMessageKind, &[u8]),
        rar_volume_cb: Option<
            &dyn Fn(&[u8], &crate::rarreader::Volume, &[crate::rarreader::RarFile]),
        >,
    ) -> Option<Self> {
        let mut set_members = Vec::with_capacity(member_names.len());
        let mut member_holes = Vec::with_capacity(member_names.len());
        let mut member_targets = vec![-1i32; member_names.len()];
        let target_holes: Vec<Vec<NzbgetRsByteRange>> =
            targets.iter().map(|t| t.holes.clone()).collect();

        for (i, &member_name) in member_names.iter().enumerate() {
            let path_bytes = crate::artwritercore::article_writer_format_dest_file_path(
                dest_dir,
                std::path::MAIN_SEPARATOR as u8,
                member_name,
            );
            let mut size = file_size_bytes(&path_bytes);
            let mut holes_i = Vec::new();
            for (t, target) in targets.iter().enumerate() {
                if target.filename.eq_ignore_ascii_case(member_name) {
                    holes_i = target.holes.clone();
                    member_targets[i] = if target.patch_eligible { t as i32 } else { -1 };
                    size = size.max(target.decoded_file_size);
                    break;
                }
            }
            set_members.push(SetMember::new(member_name, size));
            member_holes.push(holes_i);
        }

        let disk_sources =
            DiskSourceSet::with_callbacks(dest_dir, &set_members, aborted_cb, rar_volume_cb);
        let holed_sources = HoledSourceSet::new(&disk_sources, &member_holes);
        let pw = target_password.filter(|p| !p.is_empty());
        let repair_sets = ContentMapper::build_repair_sets_with_tls(
            &set_members,
            &member_holes,
            &holed_sources,
            pw,
            tls_enabled,
        );

        let mut any_mapped = false;
        for repair_set in &repair_sets {
            let first_member_name = &set_members[repair_set.set.members[0] as usize].name;
            if let Some(map) = &repair_set.map {
                any_mapped = true;
                let mb = stream_repair_bytes_to_mb(total_size(&repair_set.inner_holes));
                let mut msg = b"Cross-packing repair of ".to_vec();
                msg.extend_from_slice(first_member_name);
                msg.extend_from_slice(format!(": {mb:.1} MB missing of inner file ").as_bytes());
                msg.extend_from_slice(map.inner_name_bytes());
                emit_msg(TargetMessageKind::Info, &msg);
            } else {
                let mut msg = b"Cross-packing repair of ".to_vec();
                msg.extend_from_slice(first_member_name);
                msg.extend_from_slice(b" not possible: ");
                msg.extend_from_slice(repair_set.skip_reason.as_bytes());
                msg.extend_from_slice(b"; left to par-repair");
                emit_msg(TargetMessageKind::Info, &msg);
            }
        }
        if !any_mapped {
            return None;
        }

        Some(Self {
            dest_dir: dest_dir.to_vec(),
            set_members,
            member_targets,
            target_holes,
            repair_sets,
            target_files: DiskTargetFileSet::new(dest_dir),
            decompress_tried: std::collections::BTreeSet::new(),
            tls_enabled,
        })
    }

    pub fn has_inner_holes(&self) -> bool {
        self.repair_sets
            .iter()
            .any(|rs| rs.map.is_some() && !rs.inner_holes.is_empty())
    }

    pub fn target_holes(&self) -> &[Vec<NzbgetRsByteRange>] {
        &self.target_holes
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process_donor(
        &mut self,
        donor_queued_filename: &[u8],
        donor_info_name: &[u8],
        donor_password: &[u8],
        donor_file_names: &[&[u8]],
        donor_file_sizes: &[i64],
        donor_article_sizes: &[Vec<i32>],
        allow_decompress: bool,
        env: &mut dyn CrossPackDonorEnv,
    ) {
        {
            let mut label = b"Cross-packing repair from duplicate ".to_vec();
            label.extend_from_slice(donor_info_name);
            env.set_progress(&label, 0);
        }

        let donor_members: Vec<SetMember> = donor_file_names
            .iter()
            .zip(donor_file_sizes.iter())
            .map(|(&name, &size)| SetMember::new(base_file_name(name), size))
            .collect();
        let donor_sets = ContentMapper::group_sets(&donor_members);

        let env_cell = std::cell::RefCell::new(env);
        let donor_sources = DonorSetSources::new(&env_cell, donor_article_sizes.to_vec());

        let mut set_maps: Vec<Option<ContentMap>> = vec![None; donor_sets.len()];
        let mut set_skips: Vec<String> = vec![String::new(); donor_sets.len()];
        let mut set_tried: Vec<bool> = vec![false; donor_sets.len()];

        for repair_set_idx in 0..self.repair_sets.len() {
            if self.repair_sets[repair_set_idx].map.is_none()
                || self.repair_sets[repair_set_idx].inner_holes.is_empty()
                || env_cell.borrow_mut().is_stopped()
            {
                continue;
            }

            for set_index in 0..donor_sets.len() {
                let donor_set = &donor_sets[set_index];
                if env_cell.borrow_mut().is_stopped()
                    || self.repair_sets[repair_set_idx].inner_holes.is_empty()
                {
                    break;
                }

                if !set_tried[set_index] {
                    set_tried[set_index] = true;
                    let mapping_budget = 8 + (donor_set.members.len() as i32) * 4;
                    donor_sources.set_fetch_budget(Some(mapping_budget));
                    match ContentMapper::build_map_with_tls(
                        &donor_members,
                        donor_set,
                        &donor_sources,
                        None,
                        self.tls_enabled,
                    ) {
                        Ok(map) => set_maps[set_index] = Some(map),
                        Err(reason) => set_skips[set_index] = reason,
                    }
                    if set_maps[set_index].is_none()
                        && !donor_password.is_empty()
                        && (set_skips[set_index] == ContentMapper::SKIP_ENCRYPTED_DATA
                            || set_skips[set_index] == ContentMapper::SKIP_ENCRYPTED_HEADERS)
                    {
                        let retry_budget = 8 + (donor_set.members.len() as i32) * 4;
                        donor_sources.set_fetch_budget(Some(retry_budget));
                        match ContentMapper::build_map_with_tls(
                            &donor_members,
                            donor_set,
                            &donor_sources,
                            Some(donor_password),
                            self.tls_enabled,
                        ) {
                            Ok(map) => {
                                set_maps[set_index] = Some(map);
                                set_skips[set_index].clear();
                            }
                            Err(reason) => {
                                set_skips[set_index] = reason;
                                let first_donor_name =
                                    &donor_members[donor_set.members[0] as usize].name;
                                let mut msg = b"Skipping packing ".to_vec();
                                msg.extend_from_slice(first_donor_name);
                                msg.extend_from_slice(b" of duplicate ");
                                msg.extend_from_slice(donor_info_name);
                                msg.extend_from_slice(b": ");
                                msg.extend_from_slice(set_skips[set_index].as_bytes());
                                env_cell
                                    .borrow_mut()
                                    .emit_message_bytes(TargetMessageKind::Info, &msg);
                            }
                        }
                    }
                    donor_sources.set_fetch_budget(None);
                }

                let target_inner_size = self.repair_sets[repair_set_idx]
                    .map
                    .as_ref()
                    .map_or(0, ContentMap::inner_size);
                let size_matches = set_maps[set_index]
                    .as_ref()
                    .is_some_and(|dm| dm.inner_size() == target_inner_size);
                if !size_matches {
                    let can_decompress =
                        allow_decompress && ContentMapper::is_compressible_archive(donor_set);
                    if can_decompress {
                        let first_donor_name =
                            &donor_members[donor_set.members[0] as usize].name;
                        let mut key = Vec::with_capacity(
                            donor_queued_filename.len() + 1 + first_donor_name.len(),
                        );
                        key.extend_from_slice(donor_queued_filename);
                        key.push(b'|');
                        key.extend_from_slice(first_donor_name);
                        if self.decompress_tried.insert((repair_set_idx, key)) {
                            self.exec_decompress_repair(
                                repair_set_idx,
                                &donor_members,
                                donor_set,
                                donor_article_sizes,
                                donor_info_name,
                                donor_password,
                                &env_cell,
                            );
                        }
                    }
                    continue;
                }

                let donor_map = set_maps[set_index].as_ref().unwrap();
                let verified = {
                    let mut target_adapter = DiskTargetFileSetAdapter {
                        files: &mut self.target_files,
                        members: &self.set_members,
                        env: &env_cell,
                    };
                    self.repair_sets[repair_set_idx].verify_donor_set(
                        donor_map,
                        &donor_sources,
                        &mut target_adapter,
                    )
                };
                if !verified {
                    let first_donor_name = &donor_members[donor_set.members[0] as usize].name;
                    let inner_name = self.repair_sets[repair_set_idx]
                        .map
                        .as_ref()
                        .map_or(&[][..], ContentMap::inner_name_bytes);
                    let mut msg = b"Skipping packing ".to_vec();
                    msg.extend_from_slice(first_donor_name);
                    msg.extend_from_slice(b" of duplicate ");
                    msg.extend_from_slice(donor_info_name);
                    msg.extend_from_slice(b" for ");
                    msg.extend_from_slice(inner_name);
                    msg.extend_from_slice(b": content identity not confirmed");
                    env_cell
                        .borrow_mut()
                        .emit_message_bytes(TargetMessageKind::Info, &msg);
                    continue;
                }
                donor_sources.take_served_parts();

                let donor_info_str = String::from_utf8_lossy(donor_info_name);
                let (written, credit_recovery) = {
                    let mut target_adapter = DiskTargetFileSetAdapter {
                        files: &mut self.target_files,
                        members: &self.set_members,
                        env: &env_cell,
                    };
                    self.repair_sets[repair_set_idx].patch_from_donor_set(
                        donor_map,
                        &donor_sources,
                        &mut target_adapter,
                        &mut self.target_holes,
                        &self.member_targets,
                        &self.set_members,
                        &donor_info_str,
                    )
                };
                env_cell.borrow_mut().sync_target_holes(&self.target_holes);
                if credit_recovery && written > 0 {
                    let recovered_parts = donor_sources.take_served_parts();
                    env_cell.borrow_mut().add_recovered(written, 0);
                    let mb = stream_repair_bytes_to_mb(written);
                    let inner_name = self.repair_sets[repair_set_idx]
                        .map
                        .as_ref()
                        .map_or(&[][..], ContentMap::inner_name_bytes);
                    let mut msg = format!(
                        "Recovered {mb:.1} MB ({recovered_parts} donor article(s)) of "
                    )
                    .into_bytes();
                    msg.extend_from_slice(inner_name);
                    msg.extend_from_slice(b" from duplicate ");
                    msg.extend_from_slice(donor_info_name);
                    msg.extend_from_slice(b" (cross-packing)");
                    env_cell
                        .borrow_mut()
                        .emit_message_bytes(TargetMessageKind::Info, &msg);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn exec_decompress_repair(
        &mut self,
        repair_set_idx: usize,
        donor_members: &[SetMember],
        donor_set: &MemberSet,
        donor_article_sizes: &[Vec<i32>],
        donor_info_name: &[u8],
        donor_password: &[u8],
        env_cell: &std::cell::RefCell<&mut dyn CrossPackDonorEnv>,
    ) {
        let set_name = &donor_members[donor_set.members[0] as usize].name;
        let mut main_volume: Option<&[u8]> = None;
        for &member_index in &donor_set.members {
            let member_name = &donor_members[member_index as usize].name;
            if crate::unpack::can_extract_with_zipfast(crate::fsops::path(member_name)) {
                main_volume = Some(member_name);
                break;
            }
        }
        let Some(main_volume) = main_volume else {
            let mut msg = b"Skipping decompression of ".to_vec();
            msg.extend_from_slice(set_name);
            msg.extend_from_slice(b" of duplicate ");
            msg.extend_from_slice(donor_info_name);
            msg.extend_from_slice(b": no extractable main volume");
            env_cell
                .borrow_mut()
                .emit_message_bytes(TargetMessageKind::Info, &msg);
            return;
        };

        let mut temp_dir: Vec<u8>;
        let mut suffix: i32 = 0;
        loop {
            let candidate_name = format!(".stream-decompress.{suffix}");
            temp_dir = crate::artwritercore::article_writer_format_dest_file_path(
                &self.dest_dir,
                std::path::MAIN_SEPARATOR as u8,
                candidate_name.as_bytes(),
            );
            let mut c_temp = temp_dir.clone();
            c_temp.push(0);
            if let Ok(cstr) = std::ffi::CStr::from_bytes_with_nul(&c_temp) {
                if crate::fsops::create_directory_exclusive(cstr) {
                    break;
                }
            }
            let errno = crate::fsops::last_errno();
            if errno != crate::fsops::sys::EEXIST {
                let err_msg = crate::fsops::error_message(errno);
                let mut msg = b"Skipping decompression of ".to_vec();
                msg.extend_from_slice(set_name);
                msg.extend_from_slice(b" of duplicate ");
                msg.extend_from_slice(donor_info_name);
                msg.extend_from_slice(b": could not create ");
                msg.extend_from_slice(&temp_dir);
                msg.extend_from_slice(b": ");
                msg.extend_from_slice(&err_msg);
                env_cell
                    .borrow_mut()
                    .emit_message_bytes(TargetMessageKind::Info, &msg);
                return;
            }
            suffix += 1;
        }

        struct TempDirGuard {
            dir: Vec<u8>,
        }
        impl Drop for TempDirGuard {
            fn drop(&mut self) {
                let _ = crate::fsdelete::delete_directory_with_content(
                    &mut crate::fsops::PosixFsOps,
                    &self.dir,
                    std::path::MAIN_SEPARATOR as u8,
                    &mut Vec::new(),
                );
            }
        }
        let _temp_dir_guard = TempDirGuard {
            dir: temp_dir.clone(),
        };

        {
            let mut label = b"Downloading duplicate ".to_vec();
            label.extend_from_slice(donor_info_name);
            env_cell.borrow_mut().set_progress(&label, 0);
        }

        struct MaterializingGuard<'a, 'b> {
            env: &'a std::cell::RefCell<&'b mut dyn CrossPackDonorEnv>,
        }
        impl<'a, 'b> MaterializingGuard<'a, 'b> {
            fn new(env: &'a std::cell::RefCell<&'b mut dyn CrossPackDonorEnv>) -> Self {
                env.borrow_mut().set_materializing(1);
                Self { env }
            }
        }
        impl Drop for MaterializingGuard<'_, '_> {
            fn drop(&mut self) {
                self.env.borrow_mut().set_materializing(-1);
            }
        }
        let _materializing_guard = MaterializingGuard::new(env_cell);

        let file_names: Vec<&[u8]> = donor_members.iter().map(|m| m.name.as_slice()).collect();
        let file_article_counts: Vec<i32> =
            donor_article_sizes.iter().map(|v| v.len() as i32).collect();
        let mut total_bytes: i64 = 0;
        let materialized = {
            let mut client = MaterializeClientAdapter {
                temp_dir: &temp_dir,
                donor_members,
                env: env_cell,
                current_file: None,
                current_path: Vec::new(),
            };
            materialize_donor_set(
                &donor_set.members,
                &file_names,
                &file_article_counts,
                donor_info_name,
                &mut total_bytes,
                &mut client,
            )
        };
        if !materialized {
            let mut msg = b"Skipping decompression of ".to_vec();
            msg.extend_from_slice(set_name);
            msg.extend_from_slice(b" of duplicate ");
            msg.extend_from_slice(donor_info_name);
            msg.extend_from_slice(b": donor not materializable (stopped, incomplete, over the size cap or disk error)");
            env_cell
                .borrow_mut()
                .emit_message_bytes(TargetMessageKind::Info, &msg);
            return;
        }

        if env_cell.borrow_mut().is_stopped() {
            return;
        }
        {
            let mut label = b"Decompressing duplicate ".to_vec();
            label.extend_from_slice(donor_info_name);
            env_cell.borrow_mut().set_progress(&label, 0);
        }

        let archive_path_bytes = crate::artwritercore::article_writer_format_dest_file_path(
            &temp_dir,
            std::path::MAIN_SEPARATOR as u8,
            main_volume,
        );
        let extract_dir_bytes = crate::artwritercore::article_writer_format_dest_file_path(
            &temp_dir,
            std::path::MAIN_SEPARATOR as u8,
            b"extracted",
        );
        let archive_path = crate::fsops::path(&archive_path_bytes);
        let extract_dir = crate::fsops::path(&extract_dir_bytes);
        let can_extract = crate::unpack::can_extract_with_zipfast(archive_path);
        let extracted_ok = if can_extract {
            let pw_str = String::from_utf8_lossy(donor_password);
            let mut never_cancel = || false;
            crate::unpack::extract_single_archive_with_zipfast_cancellable(
                archive_path,
                extract_dir,
                &pw_str,
                0,
                |kind, msg| {
                    env_cell.borrow_mut().emit_extractor_log(kind, msg);
                },
                &mut never_cancel,
            )
            .is_ok()
        } else {
            false
        };
        if !extracted_ok {
            let reason: &[u8] = if can_extract {
                b"extraction failed"
            } else {
                b"no extraction tool configured"
            };
            let mut msg = b"Skipping decompression of ".to_vec();
            msg.extend_from_slice(set_name);
            msg.extend_from_slice(b" of duplicate ");
            msg.extend_from_slice(donor_info_name);
            msg.extend_from_slice(b": ");
            msg.extend_from_slice(reason);
            env_cell
                .borrow_mut()
                .emit_message_bytes(TargetMessageKind::Info, &msg);
            return;
        }

        let (target_inner_size, target_inner_name) = {
            let Some(map) = &self.repair_sets[repair_set_idx].map else {
                return;
            };
            (map.inner_size(), map.inner_name_bytes().to_vec())
        };
        let (inner_path_bytes, found_symlink) =
            select_extracted_inner(&extract_dir_bytes, target_inner_size, &target_inner_name);
        if found_symlink {
            env_cell
                .borrow_mut()
                .emit_extractor_log(0, "Stream repair: archive contains link");
        }
        if inner_path_bytes.is_empty() {
            let mut msg = b"Skipping decompression of ".to_vec();
            msg.extend_from_slice(set_name);
            msg.extend_from_slice(b" of duplicate ");
            msg.extend_from_slice(donor_info_name);
            msg.extend_from_slice(b": no extracted file matches inner file ");
            msg.extend_from_slice(&target_inner_name);
            env_cell
                .borrow_mut()
                .emit_message_bytes(TargetMessageKind::Info, &msg);
            return;
        }

        let Ok(inner_file) = std::fs::File::open(crate::fsops::path(&inner_path_bytes)) else {
            let mut msg = b"Skipping decompression of ".to_vec();
            msg.extend_from_slice(set_name);
            msg.extend_from_slice(b" of duplicate ");
            msg.extend_from_slice(donor_info_name);
            msg.extend_from_slice(b": could not open the extracted file");
            env_cell
                .borrow_mut()
                .emit_message_bytes(TargetMessageKind::Info, &msg);
            return;
        };
        let inner_size = file_size_bytes(&inner_path_bytes);
        let donor_source = DiskContentSource::new(inner_file, inner_size);

        let verified = {
            let mut target_adapter = DiskTargetFileSetAdapter {
                files: &mut self.target_files,
                members: &self.set_members,
                env: env_cell,
            };
            self.repair_sets[repair_set_idx].verify_donor_inner(
                &donor_source,
                inner_size,
                &mut target_adapter,
            )
        };
        if !verified {
            let mut msg = b"Skipping decompression of ".to_vec();
            msg.extend_from_slice(set_name);
            msg.extend_from_slice(b" of duplicate ");
            msg.extend_from_slice(donor_info_name);
            msg.extend_from_slice(b" for ");
            msg.extend_from_slice(&target_inner_name);
            msg.extend_from_slice(b": content identity not confirmed");
            env_cell
                .borrow_mut()
                .emit_message_bytes(TargetMessageKind::Info, &msg);
            return;
        }

        let donor_info_str = String::from_utf8_lossy(donor_info_name);
        let (written, holes_filled, credit_recovery) = {
            let mut target_adapter = DiskTargetFileSetAdapter {
                files: &mut self.target_files,
                members: &self.set_members,
                env: env_cell,
            };
            self.repair_sets[repair_set_idx].patch_from_donor_inner(
                &donor_source,
                &mut target_adapter,
                &mut self.target_holes,
                &self.member_targets,
                &self.set_members,
                &donor_info_str,
            )
        };
        env_cell.borrow_mut().sync_target_holes(&self.target_holes);
        if credit_recovery && written > 0 {
            env_cell.borrow_mut().add_recovered(written, holes_filled);
            let mb = stream_repair_bytes_to_mb(written);
            let mut msg = format!("Recovered {mb:.1} MB of ").into_bytes();
            msg.extend_from_slice(&target_inner_name);
            msg.extend_from_slice(b" from duplicate ");
            msg.extend_from_slice(donor_info_name);
            msg.extend_from_slice(b" (decompressed)");
            env_cell
                .borrow_mut()
                .emit_message_bytes(TargetMessageKind::Info, &msg);
        }
    }
}

/// M1 per-file repair target state (`StreamRepairController::RepairTarget` in `StreamRepair.h`).
#[derive(Clone, Debug)]
pub struct M1RepairTarget {
    pub filename: Vec<u8>,
    pub decoded_file_size: i64,
    pub holes: Vec<NzbgetRsByteRange>,
    pub encoded_size: i64,
    pub article_count: i32,
    pub recreated: bool,
    pub donor_set: Vec<u8>,
    pub patch_eligible: bool,
    pub positional_rank: i32,
    pub positional_window: i32,
    pub steps_hash: u64,
}

impl M1RepairTarget {
    #[inline]
    pub fn needs_repair(&self) -> bool {
        !self.holes.is_empty() || self.decoded_file_size <= 0
    }
}

/// Outcome of one donor's attempt at repairing a single target file (`ERepairOutcome`).
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum M1RepairOutcome {
    NoCost = 0,
    Unproductive = 1,
    Productive = 2,
}

/// Callback environment for M1 same-bytes donor repair (`StreamRepairController::ExecRepair`).
pub trait M1DonorEnv {
    fn is_stopped(&self) -> bool;
    fn refresh_live_target_name(&mut self, target_index: usize) -> Option<Vec<u8>>;
    fn begin_member_batch(&mut self, donor_file_index: i32, part_indices: &[i32]);
    fn next_article(&mut self) -> Option<FetchedDonorArticle>;
    fn cancel_remaining(&mut self);
    fn add_progress_bytes(&mut self, bytes: i64);
    fn add_recovered_bytes(&mut self, bytes: i64);
    fn emit_message_bytes(&mut self, kind: TargetMessageKind, text: &[u8]);
}

struct M1MemberBatchClient<'a> {
    env: &'a mut dyn M1DonorEnv,
    donor_file_index: i32,
}

impl DonorBatchClient for M1MemberBatchClient<'_> {
    fn is_stopped(&self) -> bool {
        self.env.is_stopped()
    }

    fn begin_batch(&mut self, part_indices: &[i32]) {
        self.env
            .begin_member_batch(self.donor_file_index, part_indices);
    }

    fn next_article(&mut self) -> Option<FetchedDonorArticle> {
        self.env.next_article()
    }

    fn cancel_remaining(&mut self) {
        self.env.cancel_remaining()
    }
}

struct DiskTargetFileReader<'a> {
    file: &'a mut std::fs::File,
}

impl TargetFileReader for DiskTargetFileReader<'_> {
    fn read_range(&mut self, offset: i64, buf: &mut [u8]) -> bool {
        use std::io::{Read, Seek, SeekFrom};
        if offset < 0 {
            return false;
        }
        let Ok(pos) = self.file.seek(SeekFrom::Start(offset as u64)) else {
            return false;
        };
        if pos != offset as u64 {
            return false;
        }
        if buf.is_empty() {
            return true;
        }
        self.file.read_exact(buf).is_ok()
    }
}

const M1_MAX_DONOR_CANDIDATES: i32 = 4;
const M1_MAX_SIBLING_CANDIDATES: i32 = 32;
const M1_MAX_INTACT_PROOF_FILES: i32 = 2;
const M1_DONOR_FAILURE_BAIL: i32 = 5;
const M1_UNPROVEN_DONOR_BAIL: i32 = 2;

#[allow(clippy::too_many_arguments)]
fn patch_disk_file_from_donor(
    file: &mut std::fs::File,
    target_filename: &[u8],
    target_decoded_size: i64,
    target_holes: &mut Vec<NzbgetRsByteRange>,
    donor_file_index: i32,
    donor_ranges: &[NzbgetRsByteRange],
    donor_name: &[u8],
    env: &mut dyn M1DonorEnv,
) -> i32 {
    struct EnvCellAdapter<'a, 'b, 'e> {
        env: &'a std::cell::RefCell<&'b mut (dyn M1DonorEnv + 'e)>,
        donor_file_index: i32,
    }
    impl DonorBatchClient for EnvCellAdapter<'_, '_, '_> {
        fn is_stopped(&self) -> bool {
            self.env.borrow().is_stopped()
        }
        fn begin_batch(&mut self, part_indices: &[i32]) {
            self.env
                .borrow_mut()
                .begin_member_batch(self.donor_file_index, part_indices);
        }
        fn next_article(&mut self) -> Option<FetchedDonorArticle> {
            self.env.borrow_mut().next_article()
        }
        fn cancel_remaining(&mut self) {
            self.env.borrow_mut().cancel_remaining()
        }
    }

    struct DiskPatcherAdapter<'a, 'b, 'e> {
        file: &'a mut std::fs::File,
        target_filename: &'a [u8],
        env: &'a std::cell::RefCell<&'b mut (dyn M1DonorEnv + 'e)>,
    }
    impl TargetFilePatcher for DiskPatcherAdapter<'_, '_, '_> {
        fn write_range(&mut self, offset: i64, data: &[u8]) -> bool {
            use std::io::{Seek, SeekFrom, Write};
            if offset < 0 {
                return false;
            }
            let io_res = self
                .file
                .seek(SeekFrom::Start(offset as u64))
                .and_then(|pos| {
                    if pos != offset as u64 {
                        Err(std::io::Error::other("seek mismatch"))
                    } else {
                        self.file.write_all(data).and_then(|()| self.file.flush())
                    }
                });
            if let Err(err) = io_res {
                let err_msg = write_error_message_bytes(&err);
                let mut msg = b"Could not write to ".to_vec();
                msg.extend_from_slice(self.target_filename);
                msg.extend_from_slice(b" during stream repair: ");
                msg.extend_from_slice(&err_msg);
                self.env
                    .borrow_mut()
                    .emit_message_bytes(TargetMessageKind::Error, &msg);
                return false;
            }
            true
        }

        fn add_progress(&mut self, bytes: i64) {
            self.env.borrow_mut().add_progress_bytes(bytes);
        }

        fn emit_message_bytes(&mut self, kind: TargetMessageKind, text: &[u8]) {
            self.env.borrow_mut().emit_message_bytes(kind, text);
        }
    }

    let env_cell = std::cell::RefCell::new(env);
    let mut batch_client = EnvCellAdapter {
        env: &env_cell,
        donor_file_index,
    };
    let mut patcher = DiskPatcherAdapter {
        file,
        target_filename,
        env: &env_cell,
    };
    let res = patch_from_donor_file(
        target_filename,
        target_decoded_size,
        target_holes,
        donor_ranges,
        donor_name,
        &mut batch_client,
        &mut patcher,
    );
    if res.credit_recovery && res.recovered_parts > 0 {
        env_cell
            .borrow_mut()
            .add_recovered_bytes(res.recovered_bytes);
    }
    res.recovered_parts
}

/// `StreamRepairController::ProveDonorOnIntactFile` (`StreamRepair.h`).
#[allow(clippy::too_many_arguments)]
pub fn prove_donor_on_intact_file(
    dest_dir: &[u8],
    whole_target_idx: usize,
    targets: &[M1RepairTarget],
    member_names: &[&[u8]],
    donor_name: &[u8],
    donor_file_names: &[&[u8]],
    donor_file_sizes: &[i64],
    donor_file_is_par: &[u8],
    donor_has_articles: &[u8],
    donor_steps_hashes: &[u64],
    donor_article_sizes: &[Vec<i32>],
    claimed: &mut [u8],
    donor_set: &[u8],
    env: &mut dyn M1DonorEnv,
) -> bool {
    let whole_target = &targets[whole_target_idx];
    let target_set = volume_set_key(&whole_target.filename);
    let stand_in_idx = select_whole_file_donor(
        base_file_name(&whole_target.filename),
        whole_target.encoded_size,
        whole_target.article_count,
        whole_target.steps_hash,
        donor_set,
        donor_file_names,
        donor_file_sizes,
        donor_file_is_par,
        donor_has_articles,
        claimed,
        donor_steps_hashes,
    );
    if target_set.is_empty() || donor_set.is_empty() || stand_in_idx < 0 {
        return false;
    }

    let target_filenames: Vec<&[u8]> = targets.iter().map(|t| t.filename.as_slice()).collect();
    let mut tried = 0;

    for &member_name in member_names {
        if env.is_stopped() || tried >= M1_MAX_INTACT_PROOF_FILES {
            break;
        }
        let base_name = base_file_name(member_name);
        if !stream_repair_is_intact_proof_member_eligible(
            base_name,
            member_name,
            &target_set,
            &target_filenames,
        ) {
            continue;
        }

        let path_bytes = crate::artwritercore::article_writer_format_dest_file_path(
            dest_dir,
            std::path::MAIN_SEPARATOR as u8,
            member_name,
        );
        let size = file_size_bytes(&path_bytes);
        if size <= 0 {
            continue;
        }
        let Ok(mut file) = std::fs::File::open(crate::fsops::path(&path_bytes)) else {
            continue;
        };
        tried += 1;

        let cand_indices = select_intact_proof_candidates(
            base_name,
            size,
            donor_set,
            stand_in_idx,
            M1_MAX_DONOR_CANDIDATES,
            donor_file_names,
            donor_file_sizes,
            donor_has_articles,
            claimed,
        );
        for cand_idx in cand_indices {
            if env.is_stopped() {
                break;
            }
            let cand_usize = cand_idx as usize;
            let donor_ranges = estimate_donor_ranges(&donor_article_sizes[cand_usize], size);
            if donor_ranges.is_empty() {
                continue;
            }
            let ok = {
                let mut batch_client = M1MemberBatchClient {
                    env,
                    donor_file_index: cand_idx,
                };
                let mut reader = DiskTargetFileReader { file: &mut file };
                verify_donor_file(size, &[], &donor_ranges, &mut batch_client, &mut reader)
            };
            if ok {
                claimed[cand_usize] = 1;
                let mut msg = b"Duplicate ".to_vec();
                msg.extend_from_slice(donor_name);
                msg.extend_from_slice(
                    b" is byte-identical to this release (verified on intact file ",
                );
                msg.extend_from_slice(member_name);
                msg.push(b')');
                env.emit_message_bytes(TargetMessageKind::Info, &msg);
                return true;
            }
        }
    }
    false
}

/// `StreamRepairController::RepairWholeFile` (`StreamRepair.h`).
#[allow(clippy::too_many_arguments)]
pub fn repair_whole_file(
    dest_dir: &[u8],
    target: &mut M1RepairTarget,
    donor_name: &[u8],
    donor_file_names: &[&[u8]],
    donor_file_sizes: &[i64],
    donor_file_is_par: &[u8],
    donor_has_articles: &[u8],
    donor_steps_hashes: &[u64],
    donor_article_sizes: &[Vec<i32>],
    claimed: &mut [u8],
    donor_proven: bool,
    env: &mut dyn M1DonorEnv,
) -> M1RepairOutcome {
    if !donor_proven {
        return M1RepairOutcome::NoCost;
    }
    let donor_idx = select_whole_file_donor(
        base_file_name(&target.filename),
        target.encoded_size,
        target.article_count,
        target.steps_hash,
        &target.donor_set,
        donor_file_names,
        donor_file_sizes,
        donor_file_is_par,
        donor_has_articles,
        claimed,
        donor_steps_hashes,
    );
    if donor_idx < 0 || donor_article_sizes[donor_idx as usize].is_empty() {
        return M1RepairOutcome::NoCost;
    }

    let donor_usize = donor_idx as usize;
    let donor_filename = donor_file_names[donor_usize];
    let sized = {
        let mut batch_client = M1MemberBatchClient {
            env,
            donor_file_index: donor_idx,
        };
        probe_whole_file_donor_size(
            donor_article_sizes[donor_usize].len(),
            target.encoded_size,
            &mut batch_client,
        )
    };
    if env.is_stopped() {
        return M1RepairOutcome::NoCost;
    }
    let Some(decoded_file_size) = sized else {
        let mut msg = b"Skipping file ".to_vec();
        msg.extend_from_slice(donor_filename);
        msg.extend_from_slice(b" of duplicate ");
        msg.extend_from_slice(donor_name);
        msg.extend_from_slice(b" for ");
        msg.extend_from_slice(&target.filename);
        msg.extend_from_slice(b": could not learn the file size from it");
        env.emit_message_bytes(TargetMessageKind::Detail, &msg);
        return M1RepairOutcome::Unproductive;
    };

    let file_path = crate::artwritercore::article_writer_format_dest_file_path(
        dest_dir,
        std::path::MAIN_SEPARATOR as u8,
        &target.filename,
    );
    let os_path = crate::fsops::path(&file_path);
    if std::fs::metadata(os_path).is_ok() {
        let existing = file_size_bytes(&file_path);
        if existing != decoded_file_size && existing != 0 {
            let mut msg = b"Could not recreate ".to_vec();
            msg.extend_from_slice(&file_path);
            msg.extend_from_slice(b": file already exists");
            env.emit_message_bytes(TargetMessageKind::Warning, &msg);
            return M1RepairOutcome::NoCost;
        }
        let mut msg = b"Recreating ".to_vec();
        msg.extend_from_slice(&file_path);
        msg.extend_from_slice(b" again: an earlier recreation was cut short");
        env.emit_message_bytes(TargetMessageKind::Info, &msg);
        let _ = std::fs::remove_file(os_path);
    }

    let mut c_path_buf = file_path.clone();
    c_path_buf.push(0);
    let mut errmsg = Vec::new();
    let alloc_ok = std::ffi::CStr::from_bytes_with_nul(&c_path_buf)
        .ok()
        .is_some_and(|c_path| crate::fsops::allocate_file(c_path, decoded_file_size, &mut errmsg));
    if !alloc_ok {
        let mut msg = b"Could not recreate ".to_vec();
        msg.extend_from_slice(&file_path);
        msg.extend_from_slice(b": ");
        msg.extend_from_slice(&errmsg);
        env.emit_message_bytes(TargetMessageKind::Warning, &msg);
        return M1RepairOutcome::NoCost;
    }

    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(os_path)
    {
        Ok(f) => f,
        Err(err) => {
            let errno = err
                .raw_os_error()
                .unwrap_or_else(crate::fsops::last_errno);
            let err_msg = crate::fsops::error_message(errno);
            let mut msg = b"Could not open ".to_vec();
            msg.extend_from_slice(&file_path);
            msg.extend_from_slice(b" for stream repair: ");
            msg.extend_from_slice(&err_msg);
            env.emit_message_bytes(TargetMessageKind::Warning, &msg);
            let _ = std::fs::remove_file(os_path);
            return M1RepairOutcome::NoCost;
        }
    };

    {
        let mut msg = b"Recreating ".to_vec();
        msg.extend_from_slice(&target.filename);
        msg.extend_from_slice(b" (no article of the file was available) from file ");
        msg.extend_from_slice(donor_filename);
        msg.extend_from_slice(b" of duplicate ");
        msg.extend_from_slice(donor_name);
        env.emit_message_bytes(TargetMessageKind::Info, &msg);
    }

    target.decoded_file_size = decoded_file_size;
    target.holes = vec![NzbgetRsByteRange {
        offset: 0,
        size: decoded_file_size,
    }];
    claimed[donor_usize] = 1;

    let donor_ranges = estimate_donor_ranges(&donor_article_sizes[donor_usize], decoded_file_size);
    let recovered_parts = patch_disk_file_from_donor(
        &mut file,
        &target.filename,
        decoded_file_size,
        &mut target.holes,
        donor_idx,
        &donor_ranges,
        donor_name,
        env,
    );
    drop(file);

    if recovered_parts == 0 {
        let _ = std::fs::remove_file(os_path);
        target.decoded_file_size = 0;
        target.holes.clear();
        return M1RepairOutcome::Unproductive;
    }

    target.recreated = true;
    M1RepairOutcome::Productive
}

/// `StreamRepairController::RepairFile` (`StreamRepair.h`).
#[allow(clippy::too_many_arguments)]
pub fn repair_file(
    dest_dir: &[u8],
    live_mode: bool,
    target_idx: usize,
    targets: &mut [M1RepairTarget],
    donor_name: &[u8],
    donor_file_names: &[&[u8]],
    donor_file_sizes: &[i64],
    donor_file_is_par: &[u8],
    donor_has_articles: &[u8],
    donor_steps_hashes: &[u64],
    donor_article_sizes: &[Vec<i32>],
    claimed: &mut [u8],
    verified: &mut bool,
    donor_proven: bool,
    env: &mut dyn M1DonorEnv,
) -> M1RepairOutcome {
    if targets[target_idx].decoded_file_size <= 0 {
        return repair_whole_file(
            dest_dir,
            &mut targets[target_idx],
            donor_name,
            donor_file_names,
            donor_file_sizes,
            donor_file_is_par,
            donor_has_articles,
            donor_steps_hashes,
            donor_article_sizes,
            claimed,
            donor_proven,
            env,
        );
    }

    if live_mode {
        if let Some(refreshed) = env.refresh_live_target_name(target_idx) {
            if !refreshed.is_empty() {
                targets[target_idx].filename = refreshed;
            }
        }
    }

    let file_path = crate::artwritercore::article_writer_format_dest_file_path(
        dest_dir,
        std::path::MAIN_SEPARATOR as u8,
        &targets[target_idx].filename,
    );
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(crate::fsops::path(&file_path))
    {
        Ok(f) => f,
        Err(err) => {
            let errno = err
                .raw_os_error()
                .unwrap_or_else(crate::fsops::last_errno);
            let err_msg = crate::fsops::error_message(errno);
            let mut msg = b"Could not open ".to_vec();
            msg.extend_from_slice(&file_path);
            msg.extend_from_slice(b" for stream repair: ");
            msg.extend_from_slice(&err_msg);
            env.emit_message_bytes(TargetMessageKind::Warning, &msg);
            return M1RepairOutcome::NoCost;
        }
    };

    let candidates = {
        let target = &targets[target_idx];
        select_donor_candidates(
            base_file_name(&target.filename),
            target.decoded_file_size,
            target.positional_rank,
            target.positional_window,
            M1_MAX_SIBLING_CANDIDATES,
            target.steps_hash,
            donor_file_names,
            donor_file_sizes,
            donor_file_is_par,
            donor_has_articles,
            claimed,
            donor_steps_hashes,
        )
    };

    let mut patched = false;
    let mut spent_fetches = false;
    let mut probed = 0i32;

    for cand_idx in candidates {
        if env.is_stopped() || targets[target_idx].holes.is_empty() {
            break;
        }
        if *verified && probed >= M1_MAX_DONOR_CANDIDATES {
            break;
        }

        let cand_usize = cand_idx as usize;
        let decoded_file_size = targets[target_idx].decoded_file_size;
        let donor_ranges =
            estimate_donor_ranges(&donor_article_sizes[cand_usize], decoded_file_size);
        if donor_ranges.is_empty() {
            continue;
        }
        spent_fetches = true;
        probed += 1;

        let ok = {
            let mut batch_client = M1MemberBatchClient {
                env,
                donor_file_index: cand_idx,
            };
            let mut reader = DiskTargetFileReader { file: &mut file };
            verify_donor_file(
                decoded_file_size,
                &targets[target_idx].holes,
                &donor_ranges,
                &mut batch_client,
                &mut reader,
            )
        };
        if !ok {
            let kind = if probed <= M1_MAX_DONOR_CANDIDATES {
                TargetMessageKind::Info
            } else {
                TargetMessageKind::Detail
            };
            let mut msg = b"Skipping file ".to_vec();
            msg.extend_from_slice(donor_file_names[cand_usize]);
            msg.extend_from_slice(b" of duplicate ");
            msg.extend_from_slice(donor_name);
            msg.extend_from_slice(b" for ");
            msg.extend_from_slice(&targets[target_idx].filename);
            msg.extend_from_slice(b": content identity not confirmed");
            env.emit_message_bytes(kind, &msg);
            continue;
        }

        *verified = true;
        claimed[cand_usize] = 1;
        let target = &mut targets[target_idx];
        let recovered_parts = patch_disk_file_from_donor(
            &mut file,
            &target.filename,
            target.decoded_file_size,
            &mut target.holes,
            cand_idx,
            &donor_ranges,
            donor_name,
            env,
        );
        patched |= recovered_parts > 0;
    }

    if patched {
        M1RepairOutcome::Productive
    } else if spent_fetches {
        M1RepairOutcome::Unproductive
    } else {
        M1RepairOutcome::NoCost
    }
}

/// Runs the two-round M1 same-bytes donor pass across `targets` for one duplicate posting
/// (`StreamRepairController::ExecRepair` in `StreamRepair.h`).
#[allow(clippy::too_many_arguments)]
pub fn process_m1_donor(
    dest_dir: &[u8],
    live_mode: bool,
    targets: &mut [M1RepairTarget],
    member_names: &[&[u8]],
    donor_info_name: &[u8],
    donor_file_names: &[&[u8]],
    donor_file_sizes: &[i64],
    donor_file_is_par: &[u8],
    donor_file_steps_hashes: &[u64],
    donor_article_sizes: &[Vec<i32>],
    env: &mut dyn M1DonorEnv,
) {
    let donor_has_articles: Vec<u8> = donor_article_sizes
        .iter()
        .map(|a| u8::from(!a.is_empty()))
        .collect();
    let mut claimed = vec![0u8; donor_file_names.len()];
    let mut proven_sets = std::collections::BTreeSet::<(Vec<u8>, Vec<u8>)>::new();
    let mut disproven_sets = std::collections::BTreeSet::<(Vec<u8>, Vec<u8>)>::new();
    let mut consecutive_failures = 0i32;
    let mut donor_proven = false;

    for round in 0..2 {
        for target_idx in 0..targets.len() {
            if env.is_stopped() {
                break;
            }
            let whole_file = targets[target_idx].decoded_file_size <= 0;
            if !targets[target_idx].patch_eligible
                || !targets[target_idx].needs_repair()
                || (whole_file != (round == 1))
            {
                continue;
            }

            let mut set_proven = false;
            if whole_file {
                let our_set = volume_set_key(&targets[target_idx].filename);
                let mut donor_sets = std::collections::BTreeSet::<Vec<u8>>::new();
                for &df_name in donor_file_names {
                    let key = volume_set_key(df_name);
                    if !key.is_empty() {
                        donor_sets.insert(key);
                    }
                }
                for their_set in donor_sets {
                    if our_set.is_empty() || env.is_stopped() {
                        break;
                    }
                    if select_whole_file_donor(
                        base_file_name(&targets[target_idx].filename),
                        targets[target_idx].encoded_size,
                        targets[target_idx].article_count,
                        targets[target_idx].steps_hash,
                        &their_set,
                        donor_file_names,
                        donor_file_sizes,
                        donor_file_is_par,
                        &donor_has_articles,
                        &claimed,
                        donor_file_steps_hashes,
                    ) < 0
                    {
                        continue;
                    }
                    let pair = (our_set.clone(), their_set.clone());
                    if disproven_sets.contains(&pair) {
                        continue;
                    }
                    if proven_sets.contains(&pair)
                        || prove_donor_on_intact_file(
                            dest_dir,
                            target_idx,
                            targets,
                            member_names,
                            donor_info_name,
                            donor_file_names,
                            donor_file_sizes,
                            donor_file_is_par,
                            &donor_has_articles,
                            donor_file_steps_hashes,
                            donor_article_sizes,
                            &mut claimed,
                            &their_set,
                            env,
                        )
                    {
                        proven_sets.insert(pair);
                        targets[target_idx].donor_set = their_set;
                        set_proven = true;
                        break;
                    }
                    disproven_sets.insert(pair);
                }
                if !set_proven {
                    continue;
                }
            }

            let claimed_before = claimed.clone();
            let mut verified = false;
            let outcome = repair_file(
                dest_dir,
                live_mode,
                target_idx,
                targets,
                donor_info_name,
                donor_file_names,
                donor_file_sizes,
                donor_file_is_par,
                &donor_has_articles,
                donor_file_steps_hashes,
                donor_article_sizes,
                &mut claimed,
                &mut verified,
                set_proven,
                env,
            );
            donor_proven |= verified;
            if verified {
                let our_set = volume_set_key(&targets[target_idx].filename);
                if !our_set.is_empty() {
                    for (m_idx, &is_c) in claimed.iter().enumerate() {
                        if is_c != 0 && claimed_before[m_idx] == 0 {
                            let their_set = volume_set_key(donor_file_names[m_idx]);
                            if !their_set.is_empty() {
                                proven_sets.insert((our_set.clone(), their_set));
                            }
                        }
                    }
                }
            }
            let (new_failures, should_bail) = eval_donor_outcome_bail(
                outcome as i32,
                M1RepairOutcome::Productive as i32,
                M1RepairOutcome::Unproductive as i32,
                donor_proven,
                consecutive_failures,
                M1_DONOR_FAILURE_BAIL,
                M1_UNPROVEN_DONOR_BAIL,
            );
            consecutive_failures = new_failures;
            if should_bail {
                let mut msg = b"Skipping remaining files for duplicate ".to_vec();
                msg.extend_from_slice(donor_info_name);
                msg.extend_from_slice(
                    format!(
                        " ({consecutive_failures} consecutive files without a byte-identical match)"
                    )
                    .as_bytes(),
                );
                env.emit_message_bytes(TargetMessageKind::Info, &msg);
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_whole_and_donor_candidates() {
        let names: &[&[u8]] = &[b"Show.part01.rar", b"Show.part02.rar", b"Show.par2"];
        let sizes = [1000i64, 1000, 1000];
        let is_par = [0u8, 0, 1];
        let has_articles = [1u8, 1, 1];
        let is_claimed = [0u8, 0, 0];
        let hashes = [111u64, 222, 333];

        assert_eq!(
            select_whole_file_donor(
                b"Other.part02.rar",
                1000,
                5,
                0,
                b"",
                names,
                &sizes,
                &is_par,
                &has_articles,
                &is_claimed,
                &hashes
            ),
            1
        );
        let cands = select_donor_candidates(
            b"Other.part02.rar",
            980,
            -1,
            0,
            5,
            0,
            names,
            &sizes,
            &is_par,
            &has_articles,
            &is_claimed,
            &hashes,
        );
        assert_eq!(cands, vec![1, 0]);
    }

    #[test]
    fn test_stream_eligible_and_keys() {
        assert!(is_stream_eligible(b"Show.S01E01.mkv"));
        assert!(is_stream_eligible(b"Show.S01E01.MP4"));
        assert!(!is_stream_eligible(b"Show.S01E01.rar"));
        assert!(!is_stream_eligible(b"noext"));

        assert_eq!(volume_set_key(b"/tmp/Rel.part03.rar"), b"rel.part#.rar");
        assert_eq!(volume_set_key(b"Rel.rar"), b"rel.r#");
        assert_eq!(volume_set_key(b"Rel.r07"), b"rel.r#");
        assert_eq!(volume_set_key(b"Rel.s00"), b"rel.r#");
        assert_eq!(volume_set_key(b"Rel.7z.002"), b"rel.7z.#");
        assert_eq!(volume_set_key(b"Rel.z01"), b"rel.z#");
        assert_eq!(volume_set_key(b"Rel.zip"), b"rel.z#");
        assert!(volume_set_key(b"movie.mkv").is_empty());

        assert_eq!(suffix_key(b"Rel.part003.rar"), b"part003.rar");
        assert_eq!(volume_key(b"Rel.part003.rar"), b"part3.rar");
        assert_eq!(volume_key(b"X.r000"), b"r0");
        assert!(decoded_size_plausible(950, 1000));
        assert!(!decoded_size_plausible(800, 1000));
    }

    #[test]
    fn test_holes_and_probes() {
        let covered = [
            NzbgetRsByteRange {
                offset: 500,
                size: 300,
            },
            NzbgetRsByteRange {
                offset: 0,
                size: 200,
            },
        ];
        let holes = compute_holes(&covered, 1000);
        assert_eq!(
            holes,
            vec![
                NzbgetRsByteRange {
                    offset: 200,
                    size: 300
                },
                NzbgetRsByteRange {
                    offset: 800,
                    size: 200
                },
            ]
        );
        assert_eq!(total_size(&holes), 500);

        let donor = estimate_donor_ranges(&[100, 100, 100, 100, 100], 1000);
        assert_eq!(donor.len(), 5);
        assert_eq!(donor[0], NzbgetRsByteRange { offset: 0, size: 200 });
        let rem = subtract_covered(
            &holes,
            NzbgetRsByteRange {
                offset: 250,
                size: 100,
            },
        );
        assert_eq!(rem.len(), 3);
    }

    #[test]
    fn test_group_sets_and_content_map() {
        let names: &[&[u8]] = &[
            b"show.part02.rar",
            b"show.part01.rar",
            b"standalone.mkv",
            b"readme.nfo",
        ];
        let sets = content_mapper_group_sets(names);
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[0], (FORMAT_RAR, vec![1, 0]));
        assert_eq!(sets[1], (FORMAT_BARE, vec![2]));

        let runs = [
            NzbgetRsContentRun {
                inner_offset: 0,
                member_index: 1,
                member_offset: 100,
                size: 500,
            },
            NzbgetRsContentRun {
                inner_offset: 500,
                member_index: 0,
                member_offset: 100,
                size: 500,
            },
        ];
        let inner = content_map_to_inner(
            &runs,
            1,
            NzbgetRsByteRange {
                offset: 200,
                size: 200,
            },
        );
        assert_eq!(
            inner,
            vec![NzbgetRsByteRange {
                offset: 100,
                size: 200
            }]
        );
        let mem = content_map_from_inner(
            &runs,
            NzbgetRsByteRange {
                offset: 400,
                size: 200,
            },
        );
        assert_eq!(mem.len(), 2);
        assert_eq!(
            mem[0],
            NzbgetRsMemberRange {
                member_index: 1,
                offset: 500,
                size: 100
            }
        );
        assert_eq!(
            mem[1],
            NzbgetRsMemberRange {
                member_index: 0,
                offset: 100,
                size: 100
            }
        );

        let key = build_donor_key(
            b"secret",
            &[b"movie.part01.rar"],
            &[1024],
            &[0],
            &[1],
            &[b"alt.binaries.test"],
            &[1],
            &[1],
            &[512],
            &[b"<msg1@news>"],
        );
        assert_eq!(
            key,
            b"6:secret1:116:movie.part01.rar4:10244:data1:117:alt.binaries.test1:11:13:51211:<msg1@news>"
        );

        let mut bases = [0i64; 3];
        assert_eq!(
            composite_compute_member_bases(&[100, 250, 50], &mut bases),
            400
        );
        assert_eq!(bases, [0, 100, 350]);

        assert_eq!(content_map_sum_cipher_sizes(&[16, 32, 64]), 112);

        assert_eq!(
            content_mapper_split_inner_name(b"movie.mkv.001"),
            b"movie.mkv"
        );
        assert!(content_mapper_split_inner_name(b"archive.rar.001").is_empty());

        assert_eq!(
            stream_repair_order_donor_indices(&[100, 200, 200], &[5, 9, 2]),
            vec![2, 1, 0]
        );
        let target_set = volume_set_key(b"Show.part01.rar");
        assert!(stream_repair_is_intact_proof_member_eligible(
            b"Show.part02.rar",
            b"Show.part02.rar",
            &target_set,
            &[b"Show.part01.rar"]
        ));
        assert!(!stream_repair_is_intact_proof_member_eligible(
            b"Show.part01.rar",
            b"Show.part01.rar",
            &target_set,
            &[b"Show.part01.rar"]
        ));
    }

    #[test]
    fn test_consolidated_stream_repair_and_crypto() {
        let mut seed = [0u8; 12];
        assert!(streamcrypto_fill_rar3_seed(
            &[0x41, 0x42],
            &[1, 2, 3, 4, 5, 6, 7, 8],
            &mut seed
        ));
        assert_eq!(&seed[..4], &[0x41, 0x00, 0x42, 0x00]);
        assert_eq!(streamcrypto_rar5_iterations(15, false), Some(32768));
        assert_eq!(streamcrypto_rar5_iterations(15, true), Some(32800));
        assert_eq!(streamcrypto_rar5_iterations(25, false), None);
        assert_eq!(streamcrypto_next_batch(0, 100, 16), (100, 1600));

        assert_eq!(
            eval_build_repair_job(
                2, 2, false, false, b"movie.mkv", true, 0, 0, 3, false, false, false, false, 0,
                1000, 0
            ),
            1
        );
        assert_eq!(
            eval_build_repair_job(
                2, 2, false, false, b"movie.mkv", true, 0, 0, 3, false, false, false, false, 5,
                1000, 950
            ),
            2
        );
        assert_eq!(
            eval_build_repair_job(
                2, 2, false, false, b"movie.par2", true, 0, 0, 3, false, false, false, false, 5,
                1000, 950
            ),
            0
        );

        let mut new_prog = 0i64;
        let mut reset_last = false;
        let reason = watchdog_eval_tick(
            100, 100, false, 0, 30, 30, 30, 30.0, 30, 4, 120, 4, 0, 0, 1000, &mut new_prog,
            &mut reset_last,
        );
        assert_eq!(reason.as_deref(), Some(&b"nothing recovered for 30 seconds"[..]));

        assert_eq!(
            select_probe_replacements(2, &[10, 20, 30, 40, 50], 8, 4),
            vec![20, 40]
        );
        assert_eq!(check_probe_article(true, 100, 1000, 0, 1000), (true, false));
        assert!(is_patch_article_usable(true, 1000, 1000, 100, 900));
        assert_eq!(clip_hole_to_fetched(100, 200, 150, 100), Some((150, 250)));
        assert!(is_incomplete_nzb_hole(12_000_000, 9_000_000));
        assert_eq!(
            patch_donor_set_step_failure(1024, 64, 1024, 0, 0, 8),
            (1, 256, 0, 0)
        );
        assert_eq!(
            calc_encrypted_window_bounds(16, 64, 64, 16, 16),
            Some((64, 16, 64))
        );
        assert_eq!(calc_encrypted_chunk_end(16, 64, 32, 16), (48, 2));
        assert!(materialize_article_valid(true, 100, 0, 1000, -1));
    }

    struct MemSource(Vec<u8>);
    impl crate::rarreader::Source for MemSource {
        fn size(&mut self) -> i64 {
            self.0.len() as i64
        }
        fn read(&mut self, offset: i64, buf: &mut [u8]) -> bool {
            if offset < 0 {
                return false;
            }
            let Some(end) = (offset as usize).checked_add(buf.len()) else {
                return false;
            };
            let Some(slice) = self.0.get(offset as usize..end) else {
                return false;
            };
            buf.copy_from_slice(slice);
            true
        }
    }

    struct VecFiles(Vec<crate::rarreader::RarFile>);
    impl crate::rarreader::Files for VecFiles {
        fn add(&mut self, file: &crate::rarreader::RarFile) -> bool {
            self.0.push(file.clone());
            true
        }
        fn set_data_offset(&mut self, position: i64) {
            if let Some(last) = self.0.last_mut() {
                last.data_offset = position.wrapping_sub(last.packed_size);
            }
        }
    }

    fn parse_rar_bytes(
        bytes: Vec<u8>,
        password: &[u8],
    ) -> (bool, crate::rarreader::Volume, Vec<crate::rarreader::RarFile>) {
        let mut vol = crate::rarreader::Volume {
            has_password: !password.is_empty(),
            ..crate::rarreader::Volume::default()
        };
        let mut src = MemSource(bytes);
        let mut cipher = NativeRarCipher::new(password);
        let mut files = VecFiles(Vec::new());
        let ok = crate::rarreader::read_from(&mut vol, &mut src, &mut cipher, &mut files);
        (ok, vol, files.0)
    }

    fn put_vint(out: &mut Vec<u8>, mut value: u64) {
        loop {
            let seven = (value & 0x7f) as u8;
            value >>= 7;
            out.push(seven | if value != 0 { 0x80 } else { 0 });
            if value == 0 {
                break;
            }
        }
    }

    fn build_rar3_salt_volume(
        data: &[u8],
        name: &[u8],
        salt: &[u8],
        trailing: bool,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"Rar!\x1a\x07\x00");
        // MAIN: VOLUME | NEWNUMBERING
        out.extend_from_slice(&0u16.to_le_bytes());
        out.push(0x73);
        out.extend_from_slice(&0x0011u16.to_le_bytes());
        out.extend_from_slice(&13u16.to_le_bytes());
        out.extend_from_slice(&[0u8; 6]);

        let name_len = name.len() as u16;
        out.extend_from_slice(&0u16.to_le_bytes());
        out.push(0x74);
        out.extend_from_slice(&(0x8000u16 | 0x0400 | 0x0004).to_le_bytes());
        out.extend_from_slice(&(32 + name_len + 8).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.push(0);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.push(29);
        out.push(0x30);
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0x20u32.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(salt);
        if !trailing {
            return out;
        }
        out.extend_from_slice(data);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.push(0x7b);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&7u16.to_le_bytes());
        out
    }

    struct Rar5CryptSpec {
        version: u64,
        flags: u64,
        kdf_count: u8,
        include_salt: bool,
        include_iv: bool,
        include_check: bool,
        record_size_override: Option<u64>,
        trailing: bool,
    }

    impl Default for Rar5CryptSpec {
        fn default() -> Self {
            Self {
                version: 0,
                flags: 0x01,
                kdf_count: 15,
                include_salt: true,
                include_iv: true,
                include_check: true,
                record_size_override: None,
                trailing: true,
            }
        }
    }

    fn build_rar5_crypt_volume(
        data: &[u8],
        name: &[u8],
        spec: &Rar5CryptSpec,
        salt: &[u8; 16],
        iv: &[u8; 16],
        check: &[u8; 12],
    ) -> (Vec<u8>, usize) {
        let mut content = Vec::new();
        put_vint(&mut content, spec.version);
        put_vint(&mut content, spec.flags);
        content.push(spec.kdf_count);
        let salt_in_content = content.len();
        if spec.include_salt {
            content.extend_from_slice(salt);
        }
        if spec.include_iv {
            content.extend_from_slice(iv);
        }
        if spec.include_check {
            content.extend_from_slice(check);
        }

        let mut record = Vec::new();
        put_vint(&mut record, 0x01);
        let salt_in_record = record.len() + salt_in_content;
        record.extend_from_slice(&content);

        let record_size = spec.record_size_override.unwrap_or(record.len() as u64);
        let mut extra = Vec::new();
        put_vint(&mut extra, record_size);
        let salt_in_extra = extra.len() + salt_in_record;
        extra.extend_from_slice(&record);

        let mut fh = Vec::new();
        put_vint(&mut fh, 2);
        put_vint(&mut fh, 0x01 | 0x02);
        put_vint(&mut fh, extra.len() as u64);
        put_vint(&mut fh, data.len() as u64);
        put_vint(&mut fh, 0);
        put_vint(&mut fh, data.len() as u64);
        put_vint(&mut fh, 0);
        put_vint(&mut fh, 0);
        put_vint(&mut fh, 0);
        put_vint(&mut fh, name.len() as u64);
        fh.extend_from_slice(name);
        let extra_in_fh = fh.len();
        fh.extend_from_slice(&extra);

        let mut out = Vec::new();
        out.extend_from_slice(b"Rar!\x1a\x07\x01\x00");
        let mut mh = Vec::new();
        put_vint(&mut mh, 1);
        put_vint(&mut mh, 0);
        put_vint(&mut mh, 0x01);
        out.extend_from_slice(&0u32.to_le_bytes());
        put_vint(&mut out, mh.len() as u64);
        out.extend_from_slice(&mh);

        out.extend_from_slice(&0u32.to_le_bytes());
        put_vint(&mut out, fh.len() as u64);
        let fh_start = out.len();
        out.extend_from_slice(&fh);
        let salt_offset = fh_start + extra_in_fh + salt_in_extra;
        out.extend_from_slice(data);

        if spec.trailing {
            let mut eh = Vec::new();
            put_vint(&mut eh, 5);
            put_vint(&mut eh, 0);
            put_vint(&mut eh, 0);
            out.extend_from_slice(&0u32.to_le_bytes());
            put_vint(&mut out, eh.len() as u64);
            out.extend_from_slice(&eh);
        }
        (out, salt_offset)
    }

    fn seed_bytes(n: usize, seed: u8) -> Vec<u8> {
        (0..n).map(|i| seed.wrapping_add((i as u8).wrapping_mul(3))).collect()
    }

    #[test]
    fn stream_crypto_full_suite_port_from_cpp() {
        // 1. StreamCryptoDeriveRar3VectorTest
        let salt3: [u8; 8] = [0x92, 0x80, 0x37, 0x09, 0x6e, 0x88, 0x92, 0xdb];
        let expect_key3: [u8; 16] = [
            0x56, 0xb3, 0x08, 0x9b, 0xf3, 0x26, 0x3f, 0xfa, 0x21, 0xae, 0x16, 0x43, 0x5e, 0x80,
            0x72, 0xf6,
        ];
        let expect_iv3: [u8; 16] = [
            0x8c, 0x84, 0x7e, 0xfd, 0x84, 0x80, 0xe0, 0x41, 0x95, 0x50, 0xee, 0x18, 0xb6, 0x88,
            0x61, 0xff,
        ];
        let (key3, iv3) = streamcrypto_derive_rar3(b"123", &salt3).unwrap();
        assert_eq!(key3, expect_key3);
        assert_eq!(iv3, expect_iv3);
        assert!(streamcrypto_derive_rar3(b"", &salt3).is_none());

        // 2. StreamCryptoDeriveRar5VectorTest
        let salt5: [u8; 16] = [
            0x99, 0xaa, 0x76, 0x9b, 0x5d, 0x9c, 0x20, 0x19, 0xbd, 0x73, 0xb1, 0x72, 0xb1, 0x51,
            0xcf, 0xe9,
        ];
        let expect_key5: [u8; 32] = [
            0x26, 0x53, 0xc2, 0x2b, 0xb5, 0x33, 0x32, 0xa9, 0x77, 0xc1, 0x4b, 0x7e, 0x1b, 0x9b,
            0xc0, 0xdc, 0x54, 0xb8, 0x51, 0x26, 0x3d, 0xd7, 0xf9, 0x87, 0xc1, 0xb0, 0x5f, 0x26,
            0xcd, 0x33, 0xe7, 0x00,
        ];
        let key5 = streamcrypto_derive_rar5(b"123", 15, &salt5).unwrap();
        assert_eq!(key5, expect_key5);
        assert!(streamcrypto_derive_rar5(b"123", 25, &salt5).is_none());
        assert!(streamcrypto_derive_rar5(b"123", 200, &salt5).is_none());

        // 3. RarReaderRar3SaltFieldTest & RarReaderRar3TruncatedSaltTest
        let s8: [u8; 8] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let (ok, _, files) = parse_rar_bytes(
            build_rar3_salt_volume(&seed_bytes(48, 5), b"movie.mkv", &s8, true),
            b"",
        );
        assert!(ok);
        assert_eq!(files.len(), 1);
        assert!(files[0].has_salt);
        assert!(files[0].encrypted_data);
        assert_eq!(files[0].salt, s8);

        let (ok_trunc, _, _) = parse_rar_bytes(
            build_rar3_salt_volume(&[], b"movie.mkv", &[0x11, 0x22, 0x33], false),
            b"",
        );
        assert!(!ok_trunc);

        // 4. RarReaderRar5CryptFieldTest, NoCheckTest, ShortCryptRecordTest, TruncatedCryptRecordTest, OversizeKdfCountTest
        let salt_r5: [u8; 16] = [
            0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad,
            0xae, 0xaf,
        ];
        let iv_r5: [u8; 16] = [
            0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xbb, 0xbc, 0xbd,
            0xbe, 0xbf,
        ];
        let check_r5: [u8; 12] = [
            0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xcb,
        ];
        let spec = Rar5CryptSpec::default();
        let (vol_bytes, _) = build_rar5_crypt_volume(
            &seed_bytes(48, 7),
            b"movie.mkv",
            &spec,
            &salt_r5,
            &iv_r5,
            &check_r5,
        );
        let (ok, _, files) = parse_rar_bytes(vol_bytes, b"");
        assert!(ok);
        assert!(files[0].encrypted_data);
        let crypt = files[0].crypt.unwrap();
        assert_eq!(crypt.version, 0);
        assert_eq!(crypt.flags, 0x01);
        assert_eq!(crypt.kdf_count, 15);
        assert!(crypt.has_check);
        assert_eq!(crypt.salt, salt_r5);
        assert_eq!(crypt.iv, iv_r5);
        assert_eq!(crypt.check_value, check_r5);

        let spec_no_check = Rar5CryptSpec {
            flags: 0,
            include_check: false,
            ..Rar5CryptSpec::default()
        };
        let (vol_nc, _) = build_rar5_crypt_volume(
            &seed_bytes(32, 9),
            b"movie.mkv",
            &spec_no_check,
            &salt_r5,
            &iv_r5,
            &[0u8; 12],
        );
        let (ok, _, files) = parse_rar_bytes(vol_nc, b"");
        assert!(ok);
        let crypt = files[0].crypt.unwrap();
        assert!(!crypt.has_check);
        assert_eq!(crypt.flags, 0);

        let spec_short = Rar5CryptSpec {
            include_iv: false,
            include_check: false,
            ..Rar5CryptSpec::default()
        };
        let (vol_short, _) = build_rar5_crypt_volume(
            &seed_bytes(16, 11),
            b"movie.mkv",
            &spec_short,
            &salt_r5,
            &iv_r5,
            &[0u8; 12],
        );
        let (ok, _, files) = parse_rar_bytes(vol_short, b"");
        assert!(ok);
        assert!(files[0].encrypted_data);
        assert!(files[0].crypt.is_none());

        let spec_trunc = Rar5CryptSpec {
            trailing: false,
            ..Rar5CryptSpec::default()
        };
        let (mut vol_trunc, salt_offset) =
            build_rar5_crypt_volume(&[], b"movie.mkv", &spec_trunc, &salt_r5, &iv_r5, &check_r5);
        vol_trunc.truncate(salt_offset + 3);
        let (ok, _, _) = parse_rar_bytes(vol_trunc, b"");
        assert!(!ok);

        let spec_oversize = Rar5CryptSpec {
            kdf_count: 200,
            ..Rar5CryptSpec::default()
        };
        let (vol_over, _) = build_rar5_crypt_volume(
            &seed_bytes(32, 13),
            b"movie.mkv",
            &spec_oversize,
            &salt_r5,
            &iv_r5,
            &check_r5,
        );
        let (ok, _, files) = parse_rar_bytes(vol_over, b"");
        assert!(ok);
        let crypt = files[0].crypt.unwrap();
        assert_eq!(crypt.kdf_count, 200);
        assert!(streamcrypto_derive_rar5(b"123", crypt.kdf_count, &crypt.salt).is_none());

        // 5. Real WinRAR testdata tests: StreamCryptoRar5PswCheckVectorTest, StreamCryptoRar5RealCiphertextCrossCheckTest,
        // StreamCryptoRar3HeaderCrossCheckTest, StreamCryptoRar3WrongPasswordTest
        let rar_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/testdata/rarrenamer");

        let r5_bytes = std::fs::read(rar_dir.join("testfile5encdata.part01.rar")).unwrap();
        let (ok, _, files) = parse_rar_bytes(r5_bytes.clone(), b"");
        assert!(ok);
        let inner = &files[0];
        let crypt = inner.crypt.unwrap();
        assert!(crypt.has_check);

        let check = streamcrypto_derive_rar5_psw_check(b"123", crypt.kdf_count, &crypt.salt).unwrap();
        assert_eq!(check, crypt.check_value[..8]);
        let wrong = streamcrypto_derive_rar5_psw_check(b"456", crypt.kdf_count, &crypt.salt).unwrap();
        assert_ne!(wrong, crypt.check_value[..8]);
        assert!(streamcrypto_derive_rar5_psw_check(b"123", 25, &crypt.salt).is_none());

        assert!(RarCryptoContext::make_rar5(b"123", &crypt).is_some());
        assert!(RarCryptoContext::make_rar5(b"wrongpass", &crypt).is_none());
        assert!(RarCryptoContext::make_rar5(b"", &crypt).is_none());

        let ctx = RarCryptoContext::make_rar5(b"123", &crypt).unwrap();
        let data_offset = inner.data_offset as usize;
        let blocks = 16;
        let cipher_slice = &r5_bytes[data_offset..data_offset + blocks * 16];
        let mut plain = vec![0u8; blocks * 16];
        assert!(ctx.decrypt_range(None, cipher_slice, &mut plain, blocks as i64));
        let expect0: [u8; 16] = [
            0xcd, 0xf2, 0x36, 0x53, 0x40, 0x75, 0x44, 0x43, 0x33, 0x44, 0x44, 0x00, 0x57, 0x65,
            0xfb, 0x6a,
        ];
        assert_eq!(&plain[..16], &expect0);
        let mut recip = vec![0u8; blocks * 16];
        assert!(ctx.encrypt_range(None, &plain, &mut recip, blocks as i64));
        assert_eq!(recip, cipher_slice);

        // StreamCryptoRar3HeaderCrossCheckTest & WrongPasswordTest
        let r3_bytes = std::fs::read(rar_dir.join("testfile3encnam.part01.rar")).unwrap();
        let main_size = u16::from_le_bytes([r3_bytes[12], r3_bytes[13]]) as usize;
        let salt_off = 7 + main_size;
        let cipher_off = salt_off + 8;
        let salt_hdr: &[u8; 8] = r3_bytes[salt_off..salt_off + 8].try_into().unwrap();
        let ctx3 = RarCryptoContext::make_rar3(b"123", salt_hdr).unwrap();
        let mut plain3 = [0u8; 64];
        assert!(ctx3.decrypt_range(None, &r3_bytes[cipher_off..cipher_off + 64], &mut plain3, 4));
        assert_eq!(plain3[2], 0x74);
        let name_len = u16::from_le_bytes([plain3[26], plain3[27]]) as usize;
        assert_eq!(name_len, 19);
        assert_eq!(&plain3[32..32 + name_len], b"testfile3encnam.dat");

        let (ok, _, files3) = parse_rar_bytes(r3_bytes.clone(), b"123");
        assert!(ok);
        assert_eq!(files3[0].filename, b"testfile3encnam.dat");

        let wrong3 = RarCryptoContext::make_rar3(b"wrongpw", salt_hdr).unwrap();
        let mut p_right = [0u8; 16];
        let mut p_wrong = [0u8; 16];
        assert!(ctx3.decrypt_range(None, &r3_bytes[cipher_off..cipher_off + 16], &mut p_right, 1));
        assert!(wrong3.decrypt_range(None, &r3_bytes[cipher_off..cipher_off + 16], &mut p_wrong, 1));
        assert_eq!(p_right[2], 0x74);
        assert_ne!(p_right, p_wrong);
        assert!(RarCryptoContext::make_rar3(b"", salt_hdr).is_none());

        // 6. StreamCryptoRoundTripTest, MultiBatchRoundTripTest, RandomAccessConsistencyTest,
        // ZeroAndNegativeBlocksTest, MakeRar5RejectsTest
        let r3_rt = RarCryptoContext::make_rar3(b"secret", &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let mut crypt5 = crate::rarreader::Rar5Crypt {
            version: 0,
            flags: 0,
            kdf_count: 12,
            salt: [0; 16],
            iv: [0; 16],
            check_value: [0; 12],
            has_check: false,
        };
        for i in 0..16 {
            crypt5.salt[i] = (i + 1) as u8;
            crypt5.iv[i] = (0x40 + i) as u8;
        }
        let r5_rt = RarCryptoContext::make_rar5(b"secret", &crypt5).unwrap();

        let plain_rt: Vec<u8> = (0..5 * 16).map(|i| (i * 7 + 1) as u8).collect();
        let prev_rt: [u8; 16] = core::array::from_fn(|i| (0xA0 + i) as u8);
        for ctx in [&r3_rt, &r5_rt] {
            let mut cipher = vec![0u8; 5 * 16];
            let mut back = vec![0u8; 5 * 16];
            assert!(ctx.encrypt_range(Some(&prev_rt), &plain_rt, &mut cipher, 5));
            assert!(ctx.decrypt_range(Some(&prev_rt), &cipher, &mut back, 5));
            assert_eq!(back, plain_rt);
            assert_ne!(cipher, plain_rt);

            let mut cipher0 = vec![0u8; 5 * 16];
            let mut back0 = vec![0u8; 5 * 16];
            assert!(ctx.encrypt_range(None, &plain_rt, &mut cipher0, 5));
            assert!(ctx.decrypt_range(None, &cipher0, &mut back0, 5));
            assert_eq!(back0, plain_rt);
            assert_ne!(&cipher[..16], &cipher0[..16]);
        }

        // MultiBatchRoundTripTest (70000 blocks > 65536)
        let mut crypt_mb = crypt5;
        crypt_mb.kdf_count = 8;
        for i in 0..16 {
            crypt_mb.salt[i] = (i * 3) as u8;
            crypt_mb.iv[i] = (i * 5 + 1) as u8;
        }
        let ctx_mb = RarCryptoContext::make_rar5(b"secret", &crypt_mb).unwrap();
        let blocks_mb: usize = 70000;
        let plain_mb: Vec<u8> = (0..blocks_mb * 16).map(|i| (i * 131 + 7) as u8).collect();
        let mut cipher_mb = vec![0u8; blocks_mb * 16];
        let mut back_mb = vec![0u8; blocks_mb * 16];
        assert!(ctx_mb.encrypt_range(None, &plain_mb, &mut cipher_mb, blocks_mb as i64));
        assert!(ctx_mb.decrypt_range(None, &cipher_mb, &mut back_mb, blocks_mb as i64));
        assert_eq!(back_mb, plain_mb);
        let edge = 65536usize;
        let prev_edge: &[u8; 16] = cipher_mb[(edge - 1) * 16..edge * 16].try_into().unwrap();
        let mut one = [0u8; 16];
        assert!(ctx_mb.decrypt_range(
            Some(prev_edge),
            &cipher_mb[edge * 16..(edge + 1) * 16],
            &mut one,
            1
        ));
        assert_eq!(&one[..], &plain_mb[edge * 16..(edge + 1) * 16]);

        // RandomAccessConsistencyTest
        let start = 3;
        let mut cipher8 = vec![0u8; 8 * 16];
        let plain8: Vec<u8> = (0..8 * 16).map(|i| (i * 5 + 3) as u8).collect();
        assert!(r3_rt.encrypt_range(None, &plain8, &mut cipher8, 8));
        let prev_block: &[u8; 16] = cipher8[(start - 1) * 16..start * 16].try_into().unwrap();
        let mut part = vec![0u8; (8 - start) * 16];
        assert!(r3_rt.decrypt_range(
            Some(prev_block),
            &cipher8[start * 16..],
            &mut part,
            (8 - start) as i64
        ));
        assert_eq!(part, &plain8[start * 16..]);

        // ZeroAndNegativeBlocksTest
        let mut out16 = [0xABu8; 16];
        assert!(r3_rt.decrypt_range(None, &[0u8; 16], &mut out16, 0));
        assert!(r3_rt.encrypt_range(None, &[0u8; 16], &mut out16, 0));
        assert_eq!(out16[0], 0xAB);
        assert!(!r3_rt.decrypt_range(None, &[0u8; 16], &mut out16, -1));
        assert!(!r3_rt.encrypt_range(None, &[0u8; 16], &mut out16, -4));

        // MakeRar5RejectsTest
        let mut v1 = crypt5;
        v1.version = 1;
        assert!(RarCryptoContext::make_rar5(b"pw", &v1).is_none());
        let mut big = crypt5;
        big.kdf_count = 200;
        assert!(RarCryptoContext::make_rar5(b"pw", &big).is_none());
        assert!(RarCryptoContext::make_rar5(b"", &crypt5).is_none());
    }

    struct MemoryContentSource(Vec<u8>);

    impl ContentSource for MemoryContentSource {
        fn size(&self) -> i64 {
            self.0.len() as i64
        }
        fn read(&self, offset: i64, buf: &mut [u8]) -> bool {
            let sz = buf.len() as i64;
            if offset < 0 || sz < 0 || offset + sz > self.0.len() as i64 {
                return false;
            }
            let start = offset as usize;
            buf.copy_from_slice(&self.0[start..start + buf.len()]);
            true
        }
    }

    #[derive(Default)]
    struct MemorySourceSet {
        sources: Vec<Option<MemoryContentSource>>,
    }

    impl MemorySourceSet {
        fn push(&mut self, data: Vec<u8>) {
            self.sources.push(Some(MemoryContentSource(data)));
        }
        fn push_null(&mut self) {
            self.sources.push(None);
        }
    }

    impl ContentSourceSet for MemorySourceSet {
        fn get_source(&self, member_index: i32) -> Option<&dyn ContentSource> {
            if member_index < 0 {
                return None;
            }
            self.sources
                .get(member_index as usize)?
                .as_ref()
                .map(|s| s as &dyn ContentSource)
        }
    }

    fn cm_pattern(size: usize, seed: u8) -> Vec<u8> {
        (0..size)
            .map(|i| seed.wrapping_add((i as u8).wrapping_mul(7)))
            .collect()
    }

    fn cm_put_le16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn cm_put_le32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn cm_put_vint(out: &mut Vec<u8>, mut value: u64) {
        loop {
            let seven = (value & 0x7f) as u8;
            value >>= 7;
            out.push(seven | if value != 0 { 0x80 } else { 0 });
            if value == 0 {
                break;
            }
        }
    }

    fn build_rar3_store_volume(
        slice: &[u8],
        full_size: i64,
        split_before: bool,
        split_after: bool,
        inner_name: impl AsRef<[u8]>,
        method: u8,
    ) -> Vec<u8> {
        let mut out = vec![0x52, 0x61, 0x72, 0x21, 0x1a, 0x07, 0x00];
        cm_put_le16(&mut out, 0);
        out.push(0x73);
        cm_put_le16(&mut out, 0x0011);
        cm_put_le16(&mut out, 13);
        out.extend_from_slice(&[0u8; 6]);

        let name_bytes = inner_name.as_ref();
        let name_len = name_bytes.len() as u16;
        cm_put_le16(&mut out, 0);
        out.push(0x74);
        cm_put_le16(
            &mut out,
            0x8000
                | if split_before { 0x0001 } else { 0 }
                | if split_after { 0x0002 } else { 0 },
        );
        cm_put_le16(&mut out, 32 + name_len);
        cm_put_le32(&mut out, slice.len() as u32);
        cm_put_le32(&mut out, full_size as u32);
        out.push(0);
        cm_put_le32(&mut out, 0);
        cm_put_le32(&mut out, 0);
        out.push(29);
        out.push(method);
        cm_put_le16(&mut out, name_len);
        cm_put_le32(&mut out, 0x20);
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(slice);

        cm_put_le16(&mut out, 0);
        out.push(0x7b);
        cm_put_le16(&mut out, 0);
        cm_put_le16(&mut out, 7);
        out
    }

    fn build_rar5_store_volume(
        slice: &[u8],
        full_size: i64,
        split_before: bool,
        split_after: bool,
        inner_name: &str,
        method: u64,
    ) -> Vec<u8> {
        let mut out = vec![0x52, 0x61, 0x72, 0x21, 0x1a, 0x07, 0x01, 0x00];
        let mut main_header = Vec::new();
        cm_put_vint(&mut main_header, 1);
        cm_put_vint(&mut main_header, 0);
        cm_put_vint(&mut main_header, 0x01);
        cm_put_le32(&mut out, 0);
        cm_put_vint(&mut out, main_header.len() as u64);
        out.extend_from_slice(&main_header);

        let mut file_header = Vec::new();
        cm_put_vint(&mut file_header, 2);
        cm_put_vint(
            &mut file_header,
            0x02 | if split_before { 0x08 } else { 0 } | if split_after { 0x10 } else { 0 },
        );
        cm_put_vint(&mut file_header, slice.len() as u64);
        cm_put_vint(&mut file_header, 0);
        cm_put_vint(&mut file_header, full_size as u64);
        cm_put_vint(&mut file_header, 0);
        cm_put_vint(&mut file_header, method << 7);
        cm_put_vint(&mut file_header, 0);
        cm_put_vint(&mut file_header, inner_name.len() as u64);
        file_header.extend_from_slice(inner_name.as_bytes());
        cm_put_le32(&mut out, 0);
        cm_put_vint(&mut out, file_header.len() as u64);
        out.extend_from_slice(&file_header);
        out.extend_from_slice(slice);

        let mut end_header = Vec::new();
        cm_put_vint(&mut end_header, 5);
        cm_put_vint(&mut end_header, 0);
        cm_put_vint(&mut end_header, 0);
        cm_put_le32(&mut out, 0);
        cm_put_vint(&mut out, end_header.len() as u64);
        out.extend_from_slice(&end_header);
        out
    }

    fn build_rar3_encrypted_data_volume(slice: &[u8], full_size: i64, inner_name: &str) -> Vec<u8> {
        let mut out = vec![0x52, 0x61, 0x72, 0x21, 0x1a, 0x07, 0x00];
        cm_put_le16(&mut out, 0);
        out.push(0x73);
        cm_put_le16(&mut out, 0x0011);
        cm_put_le16(&mut out, 13);
        out.extend_from_slice(&[0u8; 6]);

        let name_bytes = inner_name.as_bytes();
        let name_len = name_bytes.len() as u16;
        cm_put_le16(&mut out, 0);
        out.push(0x74);
        cm_put_le16(&mut out, 0x8000 | 0x0004);
        cm_put_le16(&mut out, 32 + name_len);
        cm_put_le32(&mut out, slice.len() as u32);
        cm_put_le32(&mut out, full_size as u32);
        out.push(0);
        cm_put_le32(&mut out, 0);
        cm_put_le32(&mut out, 0);
        out.push(29);
        out.push(0x30);
        cm_put_le16(&mut out, name_len);
        cm_put_le32(&mut out, 0x20);
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(slice);

        cm_put_le16(&mut out, 0);
        out.push(0x7b);
        cm_put_le16(&mut out, 0);
        cm_put_le16(&mut out, 7);
        out
    }

    fn build_rar3_encrypted_header_volume() -> Vec<u8> {
        let mut out = vec![0x52, 0x61, 0x72, 0x21, 0x1a, 0x07, 0x00];
        cm_put_le16(&mut out, 0);
        out.push(0x73);
        cm_put_le16(&mut out, 0x0091);
        cm_put_le16(&mut out, 13);
        out.extend_from_slice(&[0u8; 6]);
        out
    }

    fn build_rar3_enc_store_volume(
        cipher: &[u8],
        full_size: i64,
        split_before: bool,
        split_after: bool,
        salt: &[u8; 8],
        inner_name: &str,
    ) -> Vec<u8> {
        let mut out = vec![0x52, 0x61, 0x72, 0x21, 0x1a, 0x07, 0x00];
        cm_put_le16(&mut out, 0);
        out.push(0x73);
        cm_put_le16(&mut out, 0x0011);
        cm_put_le16(&mut out, 13);
        out.extend_from_slice(&[0u8; 6]);

        let name_bytes = inner_name.as_bytes();
        let name_len = name_bytes.len() as u16;
        cm_put_le16(&mut out, 0);
        out.push(0x74);
        cm_put_le16(
            &mut out,
            0x8000
                | 0x0400
                | 0x0004
                | if split_before { 0x0001 } else { 0 }
                | if split_after { 0x0002 } else { 0 },
        );
        cm_put_le16(&mut out, 32 + name_len + 8);
        cm_put_le32(&mut out, cipher.len() as u32);
        cm_put_le32(&mut out, full_size as u32);
        out.push(0);
        cm_put_le32(&mut out, 0);
        cm_put_le32(&mut out, 0);
        out.push(29);
        out.push(0x30);
        cm_put_le16(&mut out, name_len);
        cm_put_le32(&mut out, 0x20);
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(salt);
        out.extend_from_slice(cipher);

        cm_put_le16(&mut out, 0);
        out.push(0x7b);
        cm_put_le16(&mut out, 0);
        cm_put_le16(&mut out, 7);
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn build_rar5_enc_store_volume(
        cipher: &[u8],
        full_size: i64,
        split_before: bool,
        split_after: bool,
        kdf_count: u8,
        salt: &[u8; 16],
        iv: &[u8; 16],
        check: Option<&[u8; 12]>,
        inner_name: &str,
    ) -> Vec<u8> {
        let mut out = vec![0x52, 0x61, 0x72, 0x21, 0x1a, 0x07, 0x01, 0x00];
        let mut main_header = Vec::new();
        cm_put_vint(&mut main_header, 1);
        cm_put_vint(&mut main_header, 0);
        cm_put_vint(&mut main_header, 0x01);
        cm_put_le32(&mut out, 0);
        cm_put_vint(&mut out, main_header.len() as u64);
        out.extend_from_slice(&main_header);

        let mut content = Vec::new();
        cm_put_vint(&mut content, 0);
        cm_put_vint(&mut content, if check.is_some() { 0x01 } else { 0 });
        content.push(kdf_count);
        content.extend_from_slice(salt);
        content.extend_from_slice(iv);
        if let Some(chk) = check {
            content.extend_from_slice(chk);
        }
        let mut record = Vec::new();
        cm_put_vint(&mut record, 0x01);
        record.extend_from_slice(&content);
        let mut extra = Vec::new();
        cm_put_vint(&mut extra, record.len() as u64);
        extra.extend_from_slice(&record);

        let mut file_header = Vec::new();
        cm_put_vint(&mut file_header, 2);
        cm_put_vint(
            &mut file_header,
            0x01 | 0x02 | if split_before { 0x08 } else { 0 } | if split_after { 0x10 } else { 0 },
        );
        cm_put_vint(&mut file_header, extra.len() as u64);
        cm_put_vint(&mut file_header, cipher.len() as u64);
        cm_put_vint(&mut file_header, 0);
        cm_put_vint(&mut file_header, full_size as u64);
        cm_put_vint(&mut file_header, 0);
        cm_put_vint(&mut file_header, 0);
        cm_put_vint(&mut file_header, 0);
        cm_put_vint(&mut file_header, inner_name.len() as u64);
        file_header.extend_from_slice(inner_name.as_bytes());
        file_header.extend_from_slice(&extra);
        cm_put_le32(&mut out, 0);
        cm_put_vint(&mut out, file_header.len() as u64);
        out.extend_from_slice(&file_header);
        out.extend_from_slice(cipher);

        let mut end_header = Vec::new();
        cm_put_vint(&mut end_header, 5);
        cm_put_vint(&mut end_header, 0);
        cm_put_vint(&mut end_header, 0);
        cm_put_le32(&mut out, 0);
        cm_put_vint(&mut out, end_header.len() as u64);
        out.extend_from_slice(&end_header);
        out
    }

    fn build_stored_zip(files: &[(impl AsRef<[u8]>, &[u8])], method: u16, flags: u16) -> Vec<u8> {
        let mut out = Vec::new();
        let mut local_offsets = Vec::new();
        for (name, data) in files {
            let name_bytes = name.as_ref();
            local_offsets.push(out.len() as u32);
            cm_put_le32(&mut out, 0x04034b50);
            cm_put_le16(&mut out, 20);
            cm_put_le16(&mut out, flags);
            cm_put_le16(&mut out, method);
            cm_put_le16(&mut out, 0);
            cm_put_le16(&mut out, 0);
            cm_put_le32(&mut out, 0);
            cm_put_le32(&mut out, data.len() as u32);
            cm_put_le32(&mut out, data.len() as u32);
            cm_put_le16(&mut out, name_bytes.len() as u16);
            cm_put_le16(&mut out, 0);
            out.extend_from_slice(name_bytes);
            out.extend_from_slice(data);
        }
        let cd_start = out.len() as u32;
        for (i, (name, data)) in files.iter().enumerate() {
            let name_bytes = name.as_ref();
            cm_put_le32(&mut out, 0x02014b50);
            cm_put_le16(&mut out, 20);
            cm_put_le16(&mut out, 20);
            cm_put_le16(&mut out, flags);
            cm_put_le16(&mut out, method);
            cm_put_le16(&mut out, 0);
            cm_put_le16(&mut out, 0);
            cm_put_le32(&mut out, 0);
            cm_put_le32(&mut out, data.len() as u32);
            cm_put_le32(&mut out, data.len() as u32);
            cm_put_le16(&mut out, name_bytes.len() as u16);
            cm_put_le16(&mut out, 0);
            cm_put_le16(&mut out, 0);
            cm_put_le16(&mut out, 0);
            cm_put_le16(&mut out, 0);
            cm_put_le32(&mut out, 0);
            cm_put_le32(&mut out, local_offsets[i]);
            out.extend_from_slice(name_bytes);
        }
        let cd_size = (out.len() as u32) - cd_start;
        cm_put_le32(&mut out, 0x06054b50);
        cm_put_le16(&mut out, 0);
        cm_put_le16(&mut out, 0);
        cm_put_le16(&mut out, files.len() as u16);
        cm_put_le16(&mut out, files.len() as u16);
        cm_put_le32(&mut out, cd_size);
        cm_put_le32(&mut out, cd_start);
        cm_put_le16(&mut out, 0);
        out
    }

    fn cm_put_7z_number(out: &mut Vec<u8>, value: u64) {
        let mut first_byte: u8 = 0;
        let mut mask: u8 = 0x80;
        let mut extra_bytes = 0usize;
        while extra_bytes < 8 && value >= (1u64 << (7 * (extra_bytes + 1))) {
            first_byte |= mask;
            mask >>= 1;
            extra_bytes += 1;
        }
        if extra_bytes < 8 {
            first_byte |= (value >> (8 * extra_bytes)) as u8;
        }
        out.push(first_byte);
        for i in 0..extra_bytes {
            out.push(((value >> (8 * i)) & 0xff) as u8);
        }
    }

    fn build_7z_copy(files: &[(&str, &[u8])], coder_id: u8) -> Vec<u8> {
        let mut header = vec![0x01, 0x04, 0x06];
        cm_put_7z_number(&mut header, 0);
        cm_put_7z_number(&mut header, files.len() as u64);
        header.push(0x09);
        for &(_, data) in files {
            cm_put_7z_number(&mut header, data.len() as u64);
        }
        header.push(0x00);
        header.push(0x07);
        header.push(0x0b);
        cm_put_7z_number(&mut header, files.len() as u64);
        header.push(0x00);
        for _ in 0..files.len() {
            cm_put_7z_number(&mut header, 1);
            header.push(0x01);
            header.push(coder_id);
        }
        header.push(0x0c);
        for &(_, data) in files {
            cm_put_7z_number(&mut header, data.len() as u64);
        }
        header.push(0x00);
        header.push(0x00);
        header.push(0x05);
        cm_put_7z_number(&mut header, files.len() as u64);
        let mut names = vec![0x00];
        for &(name, _) in files {
            for &ch in name.as_bytes() {
                names.push(ch);
                names.push(0x00);
            }
            names.push(0x00);
            names.push(0x00);
        }
        header.push(0x11);
        cm_put_7z_number(&mut header, names.len() as u64);
        header.extend_from_slice(&names);
        header.push(0x00);
        header.push(0x00);

        let mut data = Vec::new();
        for &(_, bytes) in files {
            data.extend_from_slice(bytes);
        }
        wrap_7z_header(&header, &data)
    }

    fn wrap_7z_header(header: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = vec![b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c, 0, 4];
        cm_put_le32(&mut out, 0);
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        out.extend_from_slice(&(header.len() as u64).to_le_bytes());
        cm_put_le32(&mut out, 0);
        out.extend_from_slice(data);
        out.extend_from_slice(header);
        out
    }

    fn append_7z_single_file_info(header: &mut Vec<u8>, name: &str) {
        header.push(0x05);
        cm_put_7z_number(header, 1);
        let mut names = vec![0x00];
        for &ch in name.as_bytes() {
            names.push(ch);
            names.push(0x00);
        }
        names.push(0x00);
        names.push(0x00);
        header.push(0x11);
        cm_put_7z_number(header, names.len() as u64);
        header.extend_from_slice(&names);
        header.push(0x00);
    }

    fn encrypt_padded(ctx: &RarCryptoContext, plain: &[u8]) -> Vec<u8> {
        let padded = (plain.len() + 15) / 16 * 16;
        let mut input = vec![0u8; padded];
        input[..plain.len()].copy_from_slice(plain);
        let mut out = vec![0u8; padded];
        assert!(ctx.encrypt_range(None, &input, &mut out, (padded / 16) as i64));
        out
    }

    /// Full pure-Rust port of `tests/postprocess/ContentMap.cpp` (all 27 `BOOST_AUTO_TEST_CASE` tests).
    #[test]
    fn content_map_full_suite_port_from_cpp() {
        // 1. ContentMapModelTest
        {
            let mut map = ContentMap::default();
            map.set_inner_name("movie.mkv");
            map.set_inner_size(100);
            map.runs_mut().push(NzbgetRsContentRun {
                inner_offset: 0,
                member_index: 0,
                member_offset: 20,
                size: 60,
            });
            map.runs_mut().push(NzbgetRsContentRun {
                inner_offset: 60,
                member_index: 1,
                member_offset: 20,
                size: 40,
            });

            let inner = map.map_to_inner(0, NzbgetRsByteRange { offset: 0, size: 30 });
            assert_eq!(inner, vec![NzbgetRsByteRange { offset: 0, size: 10 }]);

            let pieces = map.map_from_inner(NzbgetRsByteRange { offset: 50, size: 20 });
            assert_eq!(
                pieces,
                vec![
                    NzbgetRsMemberRange {
                        member_index: 0,
                        offset: 70,
                        size: 10,
                    },
                    NzbgetRsMemberRange {
                        member_index: 1,
                        offset: 20,
                        size: 10,
                    },
                ]
            );

            assert!(map
                .map_to_inner(0, NzbgetRsByteRange { offset: 0, size: 20 })
                .is_empty());

            map.exclude_member(0);
            assert!(map
                .map_to_inner(0, NzbgetRsByteRange { offset: 20, size: 60 })
                .is_empty());
            let after = map.map_from_inner(NzbgetRsByteRange { offset: 0, size: 100 });
            assert_eq!(after.len(), 1);
            assert_eq!(after[0].member_index, 1);
        }

        // 2. CompositeSourceTest
        {
            let mut sources = MemorySourceSet::default();
            sources.push(cm_pattern(10, 1));
            sources.push(cm_pattern(20, 2));
            sources.push(cm_pattern(5, 3));

            let composite = CompositeSource::new(&sources, vec![0, 1, 2], vec![10, 20, 5]);
            assert_eq!(composite.size(), 35);

            let mut buffer = [0u8; 15];
            assert!(composite.read(5, &mut buffer));
            let member0 = cm_pattern(10, 1);
            let member1 = cm_pattern(20, 2);
            assert_eq!(&buffer[..5], &member0[5..10]);
            assert_eq!(&buffer[5..15], &member1[..10]);

            assert!(!composite.read(30, &mut buffer[..10]));

            let pieces = composite.to_members(NzbgetRsByteRange { offset: 8, size: 24 });
            assert_eq!(
                pieces,
                vec![
                    NzbgetRsMemberRange {
                        member_index: 0,
                        offset: 8,
                        size: 2,
                    },
                    NzbgetRsMemberRange {
                        member_index: 1,
                        offset: 0,
                        size: 20,
                    },
                    NzbgetRsMemberRange {
                        member_index: 2,
                        offset: 0,
                        size: 2,
                    },
                ]
            );
        }

        // 3. ContentMapperGroupSetsTest
        {
            let members: Vec<SetMember> = [
                "Rel.part02.rar",
                "Rel.part01.rar",
                "old.r00",
                "old.rar",
                "old.r01",
                "span.z01",
                "span.zip",
                "span.z02",
                "seven.7z.002",
                "seven.7z.001",
                "movie.mkv.001",
                "movie.mkv.002",
                "bare.mkv",
                "Rel.vol00+01.par2",
                "gap.part01.rar",
                "gap.part03.rar",
                "readme.txt",
            ]
            .into_iter()
            .map(|n| SetMember::new(n, 100))
            .collect();

            let sets = ContentMapper::group_sets(&members);
            assert_eq!(sets.len(), 6);
            assert_eq!(
                sets[0],
                MemberSet {
                    format: MemberFormat::Rar,
                    members: vec![1, 0],
                }
            );
            assert_eq!(
                sets[1],
                MemberSet {
                    format: MemberFormat::Rar,
                    members: vec![3, 2, 4],
                }
            );
            assert_eq!(
                sets[2],
                MemberSet {
                    format: MemberFormat::Zip,
                    members: vec![5, 7, 6],
                }
            );
            assert_eq!(
                sets[3],
                MemberSet {
                    format: MemberFormat::SevenZip,
                    members: vec![9, 8],
                }
            );
            assert_eq!(
                sets[4],
                MemberSet {
                    format: MemberFormat::Split,
                    members: vec![10, 11],
                }
            );
            assert_eq!(
                sets[5],
                MemberSet {
                    format: MemberFormat::Bare,
                    members: vec![12],
                }
            );
        }

        // 4. ContentMapperSplitAndBareMapTest
        {
            let members = vec![
                SetMember::new("movie.mkv.001", 0),
                SetMember::new("movie.mkv.002", 0),
                SetMember::new("bare.mkv", 0),
            ];
            let mut sources = MemorySourceSet::default();
            sources.push(cm_pattern(30, 1));
            sources.push(cm_pattern(12, 2));
            sources.push(cm_pattern(50, 3));

            let split_set = MemberSet {
                format: MemberFormat::Split,
                members: vec![0, 1],
            };
            let split_map = ContentMapper::build_map(&members, &split_set, &sources, None).unwrap();
            assert_eq!(split_map.inner_name(), "movie.mkv");
            assert_eq!(split_map.inner_size(), 42);
            assert_eq!(split_map.runs().len(), 2);
            assert_eq!(split_map.runs()[1].inner_offset, 30);
            assert_eq!(split_map.runs()[1].member_offset, 0);

            let bare_set = MemberSet {
                format: MemberFormat::Bare,
                members: vec![2],
            };
            let bare_map = ContentMapper::build_map(&members, &bare_set, &sources, None).unwrap();
            assert_eq!(bare_map.inner_name(), "bare.mkv");
            assert_eq!(bare_map.inner_size(), 50);
            assert_eq!(bare_map.runs().len(), 1);
            assert_eq!(bare_map.runs()[0].member_offset, 0);

            let mut holed = MemorySourceSet::default();
            holed.push(cm_pattern(30, 1));
            let bad_set = MemberSet {
                format: MemberFormat::Split,
                members: vec![0, 1],
            };
            assert!(ContentMapper::build_map(&members, &bad_set, &holed, None).is_err());

            let rar_set = MemberSet {
                format: MemberFormat::Rar,
                members: vec![0],
            };
            assert!(ContentMapper::build_map(&members, &rar_set, &sources, None).is_err());
        }

        // 5. ContentMapperRarStoreMapTest
        {
            let inner = cm_pattern(100, 5);
            let slice0 = &inner[..40];
            let slice1 = &inner[40..80];
            let slice2 = &inner[80..];

            let members = vec![
                SetMember::new("rel.part01.rar", 0),
                SetMember::new("rel.part02.rar", 0),
                SetMember::new("rel.part03.rar", 0),
            ];
            let mut sources = MemorySourceSet::default();
            sources.push(build_rar3_store_volume(
                slice0,
                100,
                false,
                true,
                "inner.mkv",
                0x30,
            ));
            sources.push(build_rar3_store_volume(
                slice1,
                100,
                true,
                true,
                "inner.mkv",
                0x30,
            ));
            sources.push(build_rar3_store_volume(
                slice2,
                100,
                true,
                false,
                "inner.mkv",
                0x30,
            ));

            let set = MemberSet {
                format: MemberFormat::Rar,
                members: vec![0, 1, 2],
            };
            let map = ContentMapper::build_map(&members, &set, &sources, None).unwrap();
            assert_eq!(map.inner_name(), "inner.mkv");
            assert_eq!(map.inner_size(), 100);
            assert_eq!(map.runs().len(), 3);

            let mut reassembled = vec![0u8; 100];
            for piece in map.map_from_inner(NzbgetRsByteRange { offset: 0, size: 100 }) {
                let source = sources.get_source(piece.member_index).unwrap();
                let inner_pos = map.map_to_inner(
                    piece.member_index,
                    NzbgetRsByteRange {
                        offset: piece.offset,
                        size: piece.size,
                    },
                );
                assert_eq!(inner_pos.len(), 1);
                let dst = inner_pos[0].offset as usize;
                assert!(source.read(
                    piece.offset,
                    &mut reassembled[dst..dst + piece.size as usize]
                ));
            }
            assert_eq!(reassembled, inner);

            let mut sources5 = MemorySourceSet::default();
            sources5.push(build_rar5_store_volume(
                slice0,
                100,
                false,
                true,
                "inner.mkv",
                0,
            ));
            sources5.push(build_rar5_store_volume(
                slice1,
                100,
                true,
                true,
                "inner.mkv",
                0,
            ));
            sources5.push(build_rar5_store_volume(
                slice2,
                100,
                true,
                false,
                "inner.mkv",
                0,
            ));
            let map5 = ContentMapper::build_map(&members, &set, &sources5, None).unwrap();
            assert_eq!(map5.inner_size(), 100);
            assert_eq!(map5.runs().len(), 3);

            // non-UTF8 CP437 inner filename preserves exact raw bytes
            let mut non_utf8_sources = MemorySourceSet::default();
            non_utf8_sources.push(build_rar3_store_volume(
                &inner,
                100,
                false,
                false,
                b"caf\x82.mkv",
                0x30,
            ));
            let single_set = MemberSet {
                format: MemberFormat::Rar,
                members: vec![0],
            };
            let non_utf8_map =
                ContentMapper::build_map(&members, &single_set, &non_utf8_sources, None).unwrap();
            assert_eq!(non_utf8_map.inner_name_bytes(), b"caf\x82.mkv");
        }

        // 6. ContentMapperRarMapDegradationTest
        {
            let inner = cm_pattern(100, 6);
            let slice0 = &inner[..40];
            let slice1 = &inner[40..80];
            let slice2 = &inner[80..];
            let members = vec![
                SetMember::new("rel.part01.rar", 0),
                SetMember::new("rel.part02.rar", 0),
                SetMember::new("rel.part03.rar", 0),
            ];
            let set = MemberSet {
                format: MemberFormat::Rar,
                members: vec![0, 1, 2],
            };

            // one unreadable volume
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar3_store_volume(
                    slice0,
                    100,
                    false,
                    true,
                    "inner.mkv",
                    0x30,
                ));
                sources.push_null();
                sources.push(build_rar3_store_volume(
                    slice2,
                    100,
                    true,
                    false,
                    "inner.mkv",
                    0x30,
                ));
                let map = ContentMapper::build_map(&members, &set, &sources, None).unwrap();
                assert_eq!(map.runs().len(), 2);
                assert!(map
                    .map_from_inner(NzbgetRsByteRange { offset: 40, size: 40 })
                    .is_empty());
                assert_eq!(map.runs()[1].inner_offset, 80);
            }

            // two unreadable volumes
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar3_store_volume(
                    slice0,
                    100,
                    false,
                    true,
                    "inner.mkv",
                    0x30,
                ));
                sources.push_null();
                sources.push_null();
                assert!(ContentMapper::build_map(&members, &set, &sources, None).is_err());
            }

            // compressed method
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar3_store_volume(
                    slice0,
                    100,
                    false,
                    false,
                    "inner.mkv",
                    0x33,
                ));
                let single = MemberSet {
                    format: MemberFormat::Rar,
                    members: vec![0],
                };
                let err = ContentMapper::build_map(&members, &single, &sources, None).err().unwrap();
                assert!(err.contains("store"));
            }

            // non-media inner file
            {
                let whole = cm_pattern(40, 7);
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar3_store_volume(
                    &whole,
                    40,
                    false,
                    false,
                    "inner.iso",
                    0x30,
                ));
                let single = MemberSet {
                    format: MemberFormat::Rar,
                    members: vec![0],
                };
                let err = ContentMapper::build_map(&members, &single, &sources, None).err().unwrap();
                assert_eq!(err, "inner file is not a media file");
            }

            // packed sizes that do not sum to inner size
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar3_store_volume(
                    slice0,
                    100,
                    false,
                    true,
                    "inner.mkv",
                    0x30,
                ));
                sources.push(build_rar3_store_volume(
                    slice1,
                    100,
                    true,
                    true,
                    "inner.mkv",
                    0x30,
                ));
                sources.push(build_rar3_store_volume(
                    &slice2[..slice2.len() - 5],
                    100,
                    true,
                    false,
                    "inner.mkv",
                    0x30,
                ));
                let err = ContentMapper::build_map(&members, &set, &sources, None).err().unwrap();
                assert!(err.contains("sum"));
            }

            // packed sizes exceed inner size
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar3_store_volume(
                    &cm_pattern(60, 10),
                    100,
                    false,
                    true,
                    "inner.mkv",
                    0x30,
                ));
                sources.push_null();
                sources.push(build_rar3_store_volume(
                    &cm_pattern(60, 11),
                    100,
                    true,
                    false,
                    "inner.mkv",
                    0x30,
                ));
                let err = ContentMapper::build_map(&members, &set, &sources, None).err().unwrap();
                assert!(err.contains("exceed"));
            }

            // encrypted archive headers without password
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar3_encrypted_header_volume());
                let single = MemberSet {
                    format: MemberFormat::Rar,
                    members: vec![0],
                };
                let err = ContentMapper::build_map(&members, &single, &sources, None).err().unwrap();
                assert_eq!(err, "encrypted archive headers");
            }

            // encrypted archive data without password
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar3_encrypted_data_volume(
                    &cm_pattern(100, 12),
                    100,
                    "inner.mkv",
                ));
                let single = MemberSet {
                    format: MemberFormat::Rar,
                    members: vec![0],
                };
                let err = ContentMapper::build_map(&members, &single, &sources, None).err().unwrap();
                assert_eq!(err, "encrypted archive data");
            }

            // inconsistent inner file size
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar3_store_volume(
                    &cm_pattern(100, 13),
                    100,
                    false,
                    false,
                    "inner.mkv",
                    0x30,
                ));
                sources.push(build_rar3_store_volume(
                    &cm_pattern(30, 14),
                    90,
                    false,
                    false,
                    "inner.mkv",
                    0x30,
                ));
                let pair = MemberSet {
                    format: MemberFormat::Rar,
                    members: vec![0, 1],
                };
                let err = ContentMapper::build_map(&members, &pair, &sources, None).err().unwrap();
                assert_eq!(err, "inconsistent inner file size across volumes");
            }
        }

        // 7. ContentMapperZipStoreMapTest
        {
            let inner = cm_pattern(90, 8);
            let nfo = cm_pattern(10, 9);
            let zip_bytes = build_stored_zip(
                &[("release/info.nfo", &nfo), ("release/movie.mkv", &inner)],
                0,
                0,
            );

            // lone .zip
            {
                let members = vec![SetMember::new("rel.zip", 0)];
                let mut sources = MemorySourceSet::default();
                sources.push(zip_bytes.clone());
                let set = MemberSet {
                    format: MemberFormat::Zip,
                    members: vec![0],
                };
                let map = ContentMapper::build_map(&members, &set, &sources, None).unwrap();
                assert_eq!(map.inner_name(), "movie.mkv");
                assert_eq!(map.inner_size(), 90);
                assert_eq!(map.runs().len(), 1);
                let mut data = vec![0u8; 90];
                let run = &map.runs()[0];
                assert!(sources.get_source(0).unwrap().read(run.member_offset, &mut data));
                assert_eq!(data, inner);
            }

            // non-UTF8 CP437 entry in .zip preserves exact raw bytes
            {
                let members = vec![SetMember::new("rel.zip", 0)];
                let mut sources = MemorySourceSet::default();
                sources.push(build_stored_zip(
                    &[(&b"release/caf\x82.mkv"[..], &inner)],
                    0,
                    0,
                ));
                let set = MemberSet {
                    format: MemberFormat::Zip,
                    members: vec![0],
                };
                let map = ContentMapper::build_map(&members, &set, &sources, None).unwrap();
                assert_eq!(map.inner_name_bytes(), b"caf\x82.mkv");
            }

            // spanned zip
            {
                let members = vec![
                    SetMember::new("rel.z01", 0),
                    SetMember::new("rel.z02", 0),
                    SetMember::new("rel.zip", 0),
                ];
                let mut sources = MemorySourceSet::default();
                sources.push(zip_bytes[..40].to_vec());
                sources.push(zip_bytes[40..95].to_vec());
                sources.push(zip_bytes[95..].to_vec());
                let set = MemberSet {
                    format: MemberFormat::Zip,
                    members: vec![0, 1, 2],
                };
                let map = ContentMapper::build_map(&members, &set, &sources, None).unwrap();
                assert_eq!(map.inner_size(), 90);

                let mut reassembled = vec![0u8; 90];
                for piece in map.map_from_inner(NzbgetRsByteRange { offset: 0, size: 90 }) {
                    let inner_pos = map.map_to_inner(
                        piece.member_index,
                        NzbgetRsByteRange {
                            offset: piece.offset,
                            size: piece.size,
                        },
                    );
                    assert_eq!(inner_pos.len(), 1);
                    let dst = inner_pos[0].offset as usize;
                    assert!(sources.get_source(piece.member_index).unwrap().read(
                        piece.offset,
                        &mut reassembled[dst..dst + piece.size as usize]
                    ));
                }
                assert_eq!(reassembled, inner);
            }
        }

        // 8. ContentMapperZipDegradationTest
        {
            let inner = cm_pattern(50, 10);
            let members = vec![SetMember::new("rel.zip", 0)];
            let set = MemberSet {
                format: MemberFormat::Zip,
                members: vec![0],
            };

            // compressed
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_stored_zip(&[("movie.mkv", &inner)], 8, 0));
                let err = ContentMapper::build_map(&members, &set, &sources, None).err().unwrap();
                assert!(err.contains("stored"));
            }

            // encrypted
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_stored_zip(&[("movie.mkv", &inner)], 0, 1));
                let err = ContentMapper::build_map(&members, &set, &sources, None).err().unwrap();
                assert!(err.contains("encrypted"));
            }

            // zip64 per-entry extra
            {
                let zip_bytes = build_stored_zip(&[("movie.mkv", &inner)], 0, 0);
                let eocd = zip_bytes.len() - 22;
                let cd_start = u32::from_le_bytes(zip_bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
                let mut rebuilt = zip_bytes[..cd_start].to_vec();
                let mut entry = zip_bytes[cd_start..eocd].to_vec();
                entry[20..28].fill(0xff);
                entry[42..46].fill(0xff);
                entry[30] = 28;
                entry[31] = 0;
                let mut extra = Vec::new();
                cm_put_le16(&mut extra, 0x0001);
                cm_put_le16(&mut extra, 24);
                for _ in 0..2 {
                    extra.push(50);
                    extra.extend_from_slice(&[0u8; 7]);
                }
                extra.extend_from_slice(&[0u8; 8]);
                entry.extend_from_slice(&extra);
                let new_cd_size = entry.len() as u16;
                rebuilt.extend_from_slice(&entry);
                let mut trailer = zip_bytes[eocd..].to_vec();
                trailer[12] = (new_cd_size & 0xff) as u8;
                trailer[13] = (new_cd_size >> 8) as u8;
                rebuilt.extend_from_slice(&trailer);

                let mut sources = MemorySourceSet::default();
                sources.push(rebuilt);
                let map = ContentMapper::build_map(&members, &set, &sources, None).unwrap();
                assert_eq!(map.inner_size(), 50);
            }
        }

        // 9. ContentMapperSevenZipCopyMapTest
        {
            let inner = cm_pattern(90, 11);
            let nfo = cm_pattern(10, 12);
            let archive = build_7z_copy(&[("info.nfo", &nfo), ("movie.mkv", &inner)], 0x00);

            // lone .7z
            {
                let members = vec![SetMember::new("rel.7z", 0)];
                let mut sources = MemorySourceSet::default();
                sources.push(archive.clone());
                let set = MemberSet {
                    format: MemberFormat::SevenZip,
                    members: vec![0],
                };
                let map = ContentMapper::build_map(&members, &set, &sources, None).unwrap();
                assert_eq!(map.inner_name(), "movie.mkv");
                assert_eq!(map.inner_size(), 90);
                assert_eq!(map.runs().len(), 1);
                assert_eq!(map.runs()[0].member_offset, 32 + 10);
            }

            // .7z.001/.7z.002 splits
            {
                let members = vec![
                    SetMember::new("rel.7z.001", 0),
                    SetMember::new("rel.7z.002", 0),
                ];
                let mut sources = MemorySourceSet::default();
                sources.push(archive[..60].to_vec());
                sources.push(archive[60..].to_vec());
                let set = MemberSet {
                    format: MemberFormat::SevenZip,
                    members: vec![0, 1],
                };
                let map = ContentMapper::build_map(&members, &set, &sources, None).unwrap();
                assert_eq!(map.inner_size(), 90);
                let mut reassembled = vec![0u8; 90];
                for piece in map.map_from_inner(NzbgetRsByteRange { offset: 0, size: 90 }) {
                    let inner_pos = map.map_to_inner(
                        piece.member_index,
                        NzbgetRsByteRange {
                            offset: piece.offset,
                            size: piece.size,
                        },
                    );
                    assert_eq!(inner_pos.len(), 1);
                    let dst = inner_pos[0].offset as usize;
                    assert!(sources.get_source(piece.member_index).unwrap().read(
                        piece.offset,
                        &mut reassembled[dst..dst + piece.size as usize]
                    ));
                }
                assert_eq!(reassembled, inner);
            }
        }

        // 10. ContentMapperSevenZipDegradationTest
        {
            let inner = cm_pattern(50, 13);
            let members = vec![SetMember::new("rel.7z", 0)];
            let set = MemberSet {
                format: MemberFormat::SevenZip,
                members: vec![0],
            };

            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_7z_copy(&[("movie.mkv", &inner)], 0x21));
                let err = ContentMapper::build_map(&members, &set, &sources, None).err().unwrap();
                assert!(err.contains("opy"));
            }
            {
                let mut sources = MemorySourceSet::default();
                sources.push(cm_pattern(64, 14));
                assert!(ContentMapper::build_map(&members, &set, &sources, None).is_err());
            }
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_7z_copy(&[("backup.iso", &inner)], 0x00));
                let err = ContentMapper::build_map(&members, &set, &sources, None).err().unwrap();
                assert_eq!(err, "inner file is not a media file");
            }
        }

        // 11. ContentMapperSevenZipEncodedHeaderTest
        {
            let inner = cm_pattern(30, 15);
            let plain = build_7z_copy(&[("movie.mkv", &inner)], 0x00);
            let real_header = &plain[32 + 30..];

            let build_encoded = |header_coder_id: u8| -> Vec<u8> {
                let mut top = vec![0x17, 0x06];
                cm_put_7z_number(&mut top, 30);
                cm_put_7z_number(&mut top, 1);
                top.push(0x09);
                cm_put_7z_number(&mut top, real_header.len() as u64);
                top.push(0x00);
                top.push(0x07);
                top.push(0x0b);
                cm_put_7z_number(&mut top, 1);
                top.push(0x00);
                cm_put_7z_number(&mut top, 1);
                top.push(0x01);
                top.push(header_coder_id);
                top.push(0x0c);
                cm_put_7z_number(&mut top, real_header.len() as u64);
                top.push(0x00);
                top.push(0x00);

                let mut data = inner.clone();
                data.extend_from_slice(real_header);
                wrap_7z_header(&top, &data)
            };

            let members = vec![SetMember::new("rel.7z", 0)];
            let set = MemberSet {
                format: MemberFormat::SevenZip,
                members: vec![0],
            };

            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_encoded(0x00));
                let map = ContentMapper::build_map(&members, &set, &sources, None).unwrap();
                assert_eq!(map.inner_name(), "movie.mkv");
                assert_eq!(map.inner_size(), 30);
                assert_eq!(map.runs().len(), 1);
                assert_eq!(map.runs()[0].member_offset, 32);
            }
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_encoded(0x21));
                let err = ContentMapper::build_map(&members, &set, &sources, None).err().unwrap();
                assert_eq!(err, "compressed 7z header");
            }
        }

        // 12. ContentMapperSevenZipAdversarialTest
        {
            let members = vec![SetMember::new("rel.7z", 0)];
            let set = MemberSet {
                format: MemberFormat::SevenZip,
                members: vec![0],
            };
            let rejects = |header: &[u8], data: &[u8]| -> String {
                let mut sources = MemorySourceSet::default();
                sources.push(wrap_7z_header(header, data));
                ContentMapper::build_map(&members, &set, &sources, None)
                    .err()
                    .unwrap()
            };

            {
                let mut header = vec![0x01, 0x02, 0x03];
                cm_put_7z_number(&mut header, 1u64 << 63);
                rejects(&header, &[]);
            }
            {
                let mut header = vec![0x01, 0x04, 0x06];
                cm_put_7z_number(&mut header, 0);
                cm_put_7z_number(&mut header, 1u64 << 60);
                header.push(0x0a);
                header.push(0x00);
                rejects(&header, &[]);
            }
            {
                let mut header = vec![0x17, 0x06];
                cm_put_7z_number(&mut header, 0);
                cm_put_7z_number(&mut header, 1);
                header.push(0x09);
                cm_put_7z_number(&mut header, 1u64 << 60);
                header.push(0x00);
                header.push(0x07);
                header.push(0x0b);
                cm_put_7z_number(&mut header, 1);
                header.push(0x00);
                cm_put_7z_number(&mut header, 1);
                header.push(0x01);
                header.push(0x00);
                header.push(0x0c);
                cm_put_7z_number(&mut header, 1u64 << 60);
                header.push(0x00);
                header.push(0x00);
                rejects(&header, &[]);
            }
            {
                let mut header = vec![0x01, 0x04, 0x06];
                cm_put_7z_number(&mut header, (1u64 << 63) - 16);
                cm_put_7z_number(&mut header, 1);
                header.push(0x09);
                cm_put_7z_number(&mut header, 10);
                header.push(0x00);
                header.push(0x07);
                header.push(0x0b);
                cm_put_7z_number(&mut header, 1);
                header.push(0x00);
                cm_put_7z_number(&mut header, 1);
                header.push(0x01);
                header.push(0x00);
                header.push(0x0c);
                cm_put_7z_number(&mut header, 10);
                header.push(0x00);
                header.push(0x00);
                append_7z_single_file_info(&mut header, "movie.mkv");
                header.push(0x00);
                let err = rejects(&header, &cm_pattern(10, 17));
                assert_eq!(err, "implausible data run geometry");
            }
            {
                let mut header = vec![0x01, 0x04, 0x06];
                cm_put_7z_number(&mut header, 0);
                cm_put_7z_number(&mut header, 1);
                header.push(0x09);
                cm_put_7z_number(&mut header, 10);
                header.push(0x00);
                header.push(0x07);
                header.push(0x0b);
                cm_put_7z_number(&mut header, 1);
                header.push(0x00);
                cm_put_7z_number(&mut header, 0);
                header.push(0x0c);
                cm_put_7z_number(&mut header, 10);
                header.push(0x00);
                header.push(0x00);
                append_7z_single_file_info(&mut header, "movie.mkv");
                header.push(0x00);
                rejects(&header, &cm_pattern(10, 19));
            }
            {
                let mut header = vec![0x01, 0x04, 0x06];
                cm_put_7z_number(&mut header, 0);
                cm_put_7z_number(&mut header, 1u64 << 60);
                header.push(0x09);
                rejects(&header, &[]);
            }
            {
                let mut header = vec![0x01, 0x04, 0x06];
                cm_put_7z_number(&mut header, 0);
                cm_put_7z_number(&mut header, 1);
                header.push(0x09);
                cm_put_7z_number(&mut header, 10);
                header.push(0x00);
                header.push(0x07);
                header.push(0x0b);
                cm_put_7z_number(&mut header, 1);
                header.push(0x00);
                cm_put_7z_number(&mut header, 1);
                header.push(0x01);
                header.push(0x00);
                header.push(0x0c);
                cm_put_7z_number(&mut header, 10);
                header.push(0x00);
                header.push(0x08);
                header.push(0x0d);
                cm_put_7z_number(&mut header, 1u64 << 40);
                header.push(0x09);
                rejects(&header, &cm_pattern(10, 16));
            }
            {
                let folder_count = 4096u64;
                let mut header = vec![0x01, 0x04, 0x06];
                cm_put_7z_number(&mut header, 0);
                cm_put_7z_number(&mut header, folder_count);
                header.push(0x09);
                for _ in 0..folder_count {
                    cm_put_7z_number(&mut header, 1);
                }
                header.push(0x00);
                header.push(0x07);
                header.push(0x0b);
                cm_put_7z_number(&mut header, folder_count);
                header.push(0x00);
                for _ in 0..folder_count {
                    cm_put_7z_number(&mut header, 1);
                    header.push(0x01);
                    header.push(0x00);
                }
                header.push(0x0c);
                rejects(&header, &[]);
            }
            {
                let mut archive = build_7z_copy(&[("movie.mkv", &cm_pattern(20, 18))], 0x00);
                archive.truncate(archive.len() - 5);
                let mut sources = MemorySourceSet::default();
                sources.push(archive);
                assert!(ContentMapper::build_map(&members, &set, &sources, None).is_err());
            }
        }

        // 13. ContentMapperSevenZipSubstreamsTest
        {
            let nfo = cm_pattern(40, 24);
            let inner = cm_pattern(60, 25);
            let mut data = nfo.clone();
            data.extend_from_slice(&inner);

            let mut header = vec![0x01, 0x04, 0x06];
            cm_put_7z_number(&mut header, 0);
            cm_put_7z_number(&mut header, 1);
            header.push(0x09);
            cm_put_7z_number(&mut header, 100);
            header.push(0x00);
            header.push(0x07);
            header.push(0x0b);
            cm_put_7z_number(&mut header, 1);
            header.push(0x00);
            cm_put_7z_number(&mut header, 1);
            header.push(0x01);
            header.push(0x00);
            header.push(0x0c);
            cm_put_7z_number(&mut header, 100);
            header.push(0x00);
            header.push(0x08);
            header.push(0x0d);
            cm_put_7z_number(&mut header, 2);
            header.push(0x09);
            cm_put_7z_number(&mut header, 40);
            header.push(0x00);
            header.push(0x00);
            header.push(0x05);
            cm_put_7z_number(&mut header, 3);
            header.push(0x0e);
            cm_put_7z_number(&mut header, 1);
            header.push(0x80);
            let mut names = vec![0x00];
            for name in ["sample", "info.nfo", "movie.mkv"] {
                for &ch in name.as_bytes() {
                    names.push(ch);
                    names.push(0x00);
                }
                names.push(0x00);
                names.push(0x00);
            }
            header.push(0x11);
            cm_put_7z_number(&mut header, names.len() as u64);
            header.extend_from_slice(&names);
            header.push(0x00);
            header.push(0x00);

            let members = vec![SetMember::new("rel.7z", 0)];
            let mut sources = MemorySourceSet::default();
            sources.push(wrap_7z_header(&header, &data));
            let set = MemberSet {
                format: MemberFormat::SevenZip,
                members: vec![0],
            };
            let map = ContentMapper::build_map(&members, &set, &sources, None).unwrap();
            assert_eq!(map.inner_name(), "movie.mkv");
            assert_eq!(map.inner_size(), 60);
            assert_eq!(map.runs().len(), 1);
            assert_eq!(map.runs()[0].member_offset, 32 + 40);
            let mut reassembled = vec![0u8; 60];
            assert!(sources
                .get_source(0)
                .unwrap()
                .read(map.runs()[0].member_offset, &mut reassembled));
            assert_eq!(reassembled, inner);
        }

        // 14. ContentMapperSevenZipDigestSkipTest
        {
            let nfo = cm_pattern(20, 26);
            let notes = cm_pattern(30, 27);
            let inner = cm_pattern(60, 28);
            let mut data = nfo.clone();
            data.extend_from_slice(&notes);
            data.extend_from_slice(&inner);

            let mut header = vec![0x01, 0x04, 0x06];
            cm_put_7z_number(&mut header, 0);
            cm_put_7z_number(&mut header, 2);
            header.push(0x09);
            cm_put_7z_number(&mut header, 20);
            cm_put_7z_number(&mut header, 90);
            header.push(0x0a);
            header.push(0x00);
            header.push(0x40);
            cm_put_le32(&mut header, 0xdead_beef);
            header.push(0x00);
            header.push(0x07);
            header.push(0x0b);
            cm_put_7z_number(&mut header, 2);
            header.push(0x00);
            for _ in 0..2 {
                cm_put_7z_number(&mut header, 1);
                header.push(0x01);
                header.push(0x00);
            }
            header.push(0x0c);
            cm_put_7z_number(&mut header, 20);
            cm_put_7z_number(&mut header, 90);
            header.push(0x0a);
            header.push(0x01);
            cm_put_le32(&mut header, 0xdead_beef);
            cm_put_le32(&mut header, 0xfeed_face);
            header.push(0x00);
            header.push(0x08);
            header.push(0x0d);
            cm_put_7z_number(&mut header, 1);
            cm_put_7z_number(&mut header, 2);
            header.push(0x09);
            cm_put_7z_number(&mut header, 30);
            header.push(0x0a);
            header.push(0x01);
            cm_put_le32(&mut header, 0xdead_beef);
            cm_put_le32(&mut header, 0xfeed_face);
            header.push(0x00);
            header.push(0x00);
            header.push(0x05);
            cm_put_7z_number(&mut header, 3);
            let mut names = vec![0x00];
            for name in ["info.nfo", "notes.txt", "movie.mkv"] {
                for &ch in name.as_bytes() {
                    names.push(ch);
                    names.push(0x00);
                }
                names.push(0x00);
                names.push(0x00);
            }
            header.push(0x11);
            cm_put_7z_number(&mut header, names.len() as u64);
            header.extend_from_slice(&names);
            header.push(0x00);
            header.push(0x00);

            let members = vec![SetMember::new("rel.7z", 0)];
            let mut sources = MemorySourceSet::default();
            sources.push(wrap_7z_header(&header, &data));
            let set = MemberSet {
                format: MemberFormat::SevenZip,
                members: vec![0],
            };
            let map = ContentMapper::build_map(&members, &set, &sources, None).unwrap();
            assert_eq!(map.inner_name(), "movie.mkv");
            assert_eq!(map.inner_size(), 60);
            assert_eq!(map.runs().len(), 1);
            assert_eq!(map.runs()[0].member_offset, 32 + 20 + 30);
            let mut reassembled = vec![0u8; 60];
            assert!(sources
                .get_source(0)
                .unwrap()
                .read(map.runs()[0].member_offset, &mut reassembled));
            assert_eq!(reassembled, inner);
        }

        // 15. HoledSourceTest
        {
            let inner = MemoryContentSource(cm_pattern(100, 17));
            let holed = HoledSource::new(&inner, vec![NzbgetRsByteRange { offset: 30, size: 10 }]);
            let mut buf = [0u8; 100];
            assert!(holed.read(0, &mut buf[..30]));
            assert!(holed.read(40, &mut buf[..60]));
            assert!(!holed.read(25, &mut buf[..10]));
            assert!(!holed.read(35, &mut buf[..1]));
            assert!(!holed.read(0, &mut buf[..100]));
            assert_eq!(holed.size(), 100);
        }

        // 16. ContentMapperBuildRepairSetsTest
        {
            let inner = cm_pattern(100, 18);
            let slice0 = &inner[..40];
            let slice1 = &inner[40..80];
            let slice2 = &inner[80..];
            let vol0 = build_rar3_store_volume(slice0, 100, false, true, "inner.mkv", 0x30);
            let vol1 = build_rar3_store_volume(slice1, 100, true, true, "inner.mkv", 0x30);
            let vol2 = build_rar3_store_volume(slice2, 100, true, false, "inner.mkv", 0x30);
            let data_start = 61i64;

            let members = vec![
                SetMember::new("rel.part01.rar", vol0.len() as i64),
                SetMember::new("rel.part02.rar", vol1.len() as i64),
                SetMember::new("rel.part03.rar", vol2.len() as i64),
                SetMember::new("bare.mkv", 50),
            ];
            let mut raw = MemorySourceSet::default();
            raw.push(vol0);
            raw.push(vol1);
            raw.push(vol2);
            raw.push(cm_pattern(50, 19));

            let member_holes = vec![
                vec![],
                vec![NzbgetRsByteRange {
                    offset: data_start + 5,
                    size: 10,
                }],
                vec![NzbgetRsByteRange { offset: 0, size: 20 }],
                vec![NzbgetRsByteRange { offset: 10, size: 5 }],
            ];
            let sources = HoledSourceSet::new(&raw, &member_holes);
            let sets = ContentMapper::build_repair_sets(&members, &member_holes, &sources, None);
            assert_eq!(sets.len(), 2);

            let m0 = sets[0].map.as_ref().unwrap();
            assert_eq!(m0.inner_size(), 100);
            assert_eq!(m0.runs().len(), 2);
            assert_eq!(
                sets[0].inner_holes,
                vec![NzbgetRsByteRange { offset: 45, size: 10 }]
            );
            assert_eq!(sets[0].original_inner_holes, sets[0].inner_holes);

            let m1 = sets[1].map.as_ref().unwrap();
            assert_eq!(m1.inner_size(), 50);
            assert_eq!(
                sets[1].inner_holes,
                vec![NzbgetRsByteRange { offset: 10, size: 5 }]
            );
            assert_eq!(sets[1].original_inner_holes, sets[1].inner_holes);

            // unmappable set still reports
            {
                let comp_members = vec![SetMember::new("c.rar", 0)];
                let mut comp_raw = MemorySourceSet::default();
                comp_raw.push(build_rar3_store_volume(
                    slice0,
                    100,
                    false,
                    false,
                    "inner.mkv",
                    0x33,
                ));
                let comp_holes = vec![vec![NzbgetRsByteRange { offset: 70, size: 5 }]];
                let comp_sources = HoledSourceSet::new(&comp_raw, &comp_holes);
                let comp_sets =
                    ContentMapper::build_repair_sets(&comp_members, &comp_holes, &comp_sources, None);
                assert_eq!(comp_sets.len(), 1);
                assert!(comp_sets[0].map.is_none());
                assert!(!comp_sets[0].skip_reason.is_empty());
            }

            // sets without holes never appear
            {
                let no_holes = vec![vec![]; 4];
                let clean_sources = HoledSourceSet::new(&raw, &no_holes);
                assert!(ContentMapper::build_repair_sets(&members, &no_holes, &clean_sources, None)
                    .is_empty());
            }
        }

        // 17. ContentMapperCoalesceRangesTest
        {
            assert!(ContentMapper::coalesce_ranges(&[]).is_empty());
            let disjoint = ContentMapper::coalesce_ranges(&[
                NzbgetRsByteRange {
                    offset: 100,
                    size: 10,
                },
                NzbgetRsByteRange { offset: 0, size: 10 },
            ]);
            assert_eq!(
                disjoint,
                vec![
                    NzbgetRsByteRange { offset: 0, size: 10 },
                    NzbgetRsByteRange {
                        offset: 100,
                        size: 10,
                    },
                ]
            );
            let overlap = ContentMapper::coalesce_ranges(&[
                NzbgetRsByteRange { offset: 0, size: 20 },
                NzbgetRsByteRange { offset: 10, size: 30 },
            ]);
            assert_eq!(overlap, vec![NzbgetRsByteRange { offset: 0, size: 40 }]);
            let adjacent = ContentMapper::coalesce_ranges(&[
                NzbgetRsByteRange { offset: 0, size: 10 },
                NzbgetRsByteRange { offset: 10, size: 5 },
                NzbgetRsByteRange { offset: 2, size: 3 },
            ]);
            assert_eq!(adjacent, vec![NzbgetRsByteRange { offset: 0, size: 15 }]);
            let windows = ContentMapper::coalesce_ranges(&[
                NzbgetRsByteRange {
                    offset: 100000,
                    size: 40,
                },
                NzbgetRsByteRange {
                    offset: 100000,
                    size: 40,
                },
            ]);
            assert_eq!(
                windows,
                vec![NzbgetRsByteRange {
                    offset: 100000,
                    size: 40,
                }]
            );
        }

        // 18. ContentMapperRar3EncryptedStoreMapTest
        {
            let inner = cm_pattern(100, 5);
            let salt = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
            let ctx = RarCryptoContext::make_rar3(b"123", &salt).unwrap();
            let cipher = encrypt_padded(&ctx, &inner);
            assert_eq!(cipher.len(), 112);

            let members = vec![
                SetMember::new("rel.part01.rar", 0),
                SetMember::new("rel.part02.rar", 0),
                SetMember::new("rel.part03.rar", 0),
            ];
            let mut sources = MemorySourceSet::default();
            sources.push(build_rar3_enc_store_volume(
                &cipher[..48],
                100,
                false,
                true,
                &salt,
                "inner.mkv",
            ));
            sources.push(build_rar3_enc_store_volume(
                &cipher[48..96],
                100,
                true,
                true,
                &salt,
                "inner.mkv",
            ));
            sources.push(build_rar3_enc_store_volume(
                &cipher[96..],
                100,
                true,
                false,
                &salt,
                "inner.mkv",
            ));

            let set = MemberSet {
                format: MemberFormat::Rar,
                members: vec![0, 1, 2],
            };
            let map = ContentMapper::build_map(&members, &set, &sources, Some("123")).unwrap();
            assert_eq!(map.inner_name(), "inner.mkv");
            assert_eq!(map.inner_size(), 100);
            assert!(map.encrypted());
            assert_eq!(map.runs().len(), 3);
            assert_eq!(map.runs()[0].inner_offset, 0);
            assert_eq!(map.runs()[0].size, 48);
            assert_eq!(map.runs()[1].inner_offset, 48);
            assert_eq!(map.runs()[1].size, 48);
            assert_eq!(map.runs()[2].inner_offset, 96);
            assert_eq!(map.runs()[2].size, 4);

            let rc0 = map.run_crypto(0).unwrap();
            for i in 0..map.runs().len() {
                let rc = map.run_crypto(i).unwrap();
                assert!(std::sync::Arc::ptr_eq(
                    rc.crypto.as_ref().unwrap(),
                    rc0.crypto.as_ref().unwrap()
                ));
                assert_eq!(rc.cipher_data_offset, map.runs()[i].member_offset);
            }
            let mut plain_out = vec![0u8; cipher.len()];
            assert!(rc0.crypto.as_ref().unwrap().decrypt_range(
                None,
                &cipher,
                &mut plain_out,
                (cipher.len() / 16) as i64
            ));
            assert_eq!(&plain_out[..100], &inner[..]);
        }

        // 19. ContentMapperRar5EncryptedStoreMapTest
        {
            let inner = cm_pattern(50, 9);
            let salt = [
                0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad,
                0xae, 0xaf,
            ];
            let iv = [
                0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xbb, 0xbc, 0xbd,
                0xbe, 0xbf,
            ];
            let kdf = 12u8;
            let check8 = streamcrypto_derive_rar5_psw_check(b"123", kdf, &salt).unwrap();
            let mut check = [0u8; 12];
            check[..8].copy_from_slice(&check8);

            let crypt = crate::rarreader::Rar5Crypt {
                version: 0,
                flags: 1,
                kdf_count: kdf,
                salt,
                iv,
                check_value: check,
                has_check: true,
            };
            let ctx = RarCryptoContext::make_rar5(b"123", &crypt).unwrap();
            let cipher = encrypt_padded(&ctx, &inner);
            assert_eq!(cipher.len(), 64);

            let members = vec![SetMember::new("rel.part01.rar", 0)];
            let mut sources = MemorySourceSet::default();
            sources.push(build_rar5_enc_store_volume(
                &cipher,
                50,
                false,
                false,
                kdf,
                &salt,
                &iv,
                Some(&check),
                "inner.mkv",
            ));
            let set = MemberSet {
                format: MemberFormat::Rar,
                members: vec![0],
            };
            let map = ContentMapper::build_map(&members, &set, &sources, Some("123")).unwrap();
            assert_eq!(map.inner_size(), 50);
            assert!(map.encrypted());
            assert_eq!(map.runs().len(), 1);
            assert_eq!(map.runs()[0].size, 50);
            let rc = map.run_crypto(0).unwrap();
            let mut plain_out = vec![0u8; cipher.len()];
            assert!(rc.crypto.as_ref().unwrap().decrypt_range(
                None,
                &cipher,
                &mut plain_out,
                (cipher.len() / 16) as i64
            ));
            assert_eq!(&plain_out[..50], &inner[..]);
        }

        // 20. ContentMapperRarEncryptedRejectionsTest
        {
            let inner = cm_pattern(50, 3);
            let salt3 = [1, 2, 3, 4, 5, 6, 7, 8];
            let salt5 = [
                0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
                0x1e, 0x1f,
            ];
            let iv5 = [
                0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d,
                0x2e, 0x2f,
            ];
            let kdf = 11u8;
            let check8 = streamcrypto_derive_rar5_psw_check(b"123", kdf, &salt5).unwrap();
            let mut check = [0u8; 12];
            check[..8].copy_from_slice(&check8);

            let ctx3 = RarCryptoContext::make_rar3(b"123", &salt3).unwrap();
            let cipher3 = encrypt_padded(&ctx3, &inner);

            let single = MemberSet {
                format: MemberFormat::Rar,
                members: vec![0],
            };
            let members = vec![SetMember::new("rel.part01.rar", 0)];

            // wrong RAR5 password
            {
                let crypt = crate::rarreader::Rar5Crypt {
                    version: 0,
                    flags: 1,
                    kdf_count: kdf,
                    salt: salt5,
                    iv: iv5,
                    check_value: check,
                    has_check: true,
                };
                let ctx5 = RarCryptoContext::make_rar5(b"123", &crypt).unwrap();
                let cipher5 = encrypt_padded(&ctx5, &inner);
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar5_enc_store_volume(
                    &cipher5,
                    50,
                    false,
                    false,
                    kdf,
                    &salt5,
                    &iv5,
                    Some(&check),
                    "inner.mkv",
                ));
                let err = ContentMapper::build_map(&members, &single, &sources, Some("wrongpw"))
                    .err()
                    .unwrap();
                assert_eq!(err, "archive password rejected");
            }

            // no password
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar3_enc_store_volume(
                    &cipher3,
                    50,
                    false,
                    false,
                    &salt3,
                    "inner.mkv",
                ));
                let err = ContentMapper::build_map(&members, &single, &sources, None)
                    .err()
                    .unwrap();
                assert_eq!(err, "encrypted archive data");
            }

            // non-16-aligned non-last volume (47 + 17 = 64)
            let pair = vec![
                SetMember::new("rel.part01.rar", 0),
                SetMember::new("rel.part02.rar", 0),
            ];
            let two_vol = MemberSet {
                format: MemberFormat::Rar,
                members: vec![0, 1],
            };
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar3_enc_store_volume(
                    &cipher3[..47],
                    50,
                    false,
                    true,
                    &salt3,
                    "inner.mkv",
                ));
                sources.push(build_rar3_enc_store_volume(
                    &cipher3[47..],
                    50,
                    true,
                    false,
                    &salt3,
                    "inner.mkv",
                ));
                let map = ContentMapper::build_map(&pair, &two_vol, &sources, Some("123")).unwrap();
                assert_eq!(map.runs().len(), 2);
                assert_eq!(map.runs()[0].size, 47);
                assert_eq!(map.runs()[1].size, 3);
                assert_eq!(map.run_crypto(0).unwrap().cipher_size, 47);
                assert_eq!(map.run_crypto(1).unwrap().cipher_size, 17);
                assert_eq!(map.cipher_stream_size(), 64);
            }

            // slabs not totaling ceil16(innerSize)
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar3_enc_store_volume(
                    &cipher3[..47],
                    50,
                    false,
                    true,
                    &salt3,
                    "inner.mkv",
                ));
                sources.push(build_rar3_enc_store_volume(
                    &cipher3[47..60],
                    50,
                    true,
                    false,
                    &salt3,
                    "inner.mkv",
                ));
                let err = ContentMapper::build_map(&pair, &two_vol, &sources, Some("123"))
                    .err()
                    .unwrap();
                assert_eq!(err, "encrypted volume geometry does not fit store mode");
            }

            // padding-only last volume (52 >= 50)
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar3_enc_store_volume(
                    &cipher3[..52],
                    50,
                    false,
                    true,
                    &salt3,
                    "inner.mkv",
                ));
                sources.push(build_rar3_enc_store_volume(
                    &cipher3[52..],
                    50,
                    true,
                    false,
                    &salt3,
                    "inner.mkv",
                ));
                let err = ContentMapper::build_map(&pair, &two_vol, &sources, Some("123"))
                    .err()
                    .unwrap();
                assert_eq!(err, "encrypted volume geometry does not fit store mode");
            }

            // single volume whose packed size is not ceil16(innerSize)
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar3_enc_store_volume(
                    &cipher3[..60],
                    50,
                    false,
                    false,
                    &salt3,
                    "inner.mkv",
                ));
                let err = ContentMapper::build_map(&members, &single, &sources, Some("123"))
                    .err()
                    .unwrap();
                assert_eq!(err, "encrypted volume geometry does not fit store mode");
            }
        }

        // 21. ContentMapperRarEncryptedCompositeTest
        {
            let inner = cm_pattern(100, 41);
            let salt = [0x9a, 0x8b, 0x7c, 0x6d, 0x5e, 0x4f, 0x30, 0x21];
            let ctx = RarCryptoContext::make_rar3(b"123", &salt).unwrap();
            let cipher = encrypt_padded(&ctx, &inner);
            assert_eq!(cipher.len(), 112);

            let c0 = &cipher[..45];
            let c1 = &cipher[45..82];
            let c2 = &cipher[82..];

            let members = vec![
                SetMember::new("rel.part01.rar", 0),
                SetMember::new("rel.part02.rar", 0),
                SetMember::new("rel.part03.rar", 0),
            ];
            let mut sources = MemorySourceSet::default();
            sources.push(build_rar3_enc_store_volume(
                c0,
                100,
                false,
                true,
                &salt,
                "inner.mkv",
            ));
            sources.push(build_rar3_enc_store_volume(
                c1,
                100,
                true,
                true,
                &salt,
                "inner.mkv",
            ));
            sources.push(build_rar3_enc_store_volume(
                c2,
                100,
                true,
                false,
                &salt,
                "inner.mkv",
            ));

            let set = MemberSet {
                format: MemberFormat::Rar,
                members: vec![0, 1, 2],
            };
            let map = ContentMapper::build_map(&members, &set, &sources, Some("123")).unwrap();
            assert!(map.encrypted());
            assert_eq!(map.inner_size(), 100);
            assert_eq!(map.cipher_stream_size(), 112);
            assert_eq!(map.runs().len(), 3);
            let run_sizes = [45, 37, 18];
            let slab_sizes = [45, 37, 30];
            let rc0 = map.run_crypto(0).unwrap();
            for i in 0..3 {
                let run = &map.runs()[i];
                let rc = map.run_crypto(i).unwrap();
                assert!(std::sync::Arc::ptr_eq(
                    rc.crypto.as_ref().unwrap(),
                    rc0.crypto.as_ref().unwrap()
                ));
                assert_eq!(run.size, run_sizes[i]);
                assert_eq!(rc.cipher_size, slab_sizes[i]);
                assert_eq!(rc.cipher_data_offset, run.member_offset);
            }

            let data_off0 = map.runs()[0].member_offset;
            let data_off1 = map.runs()[1].member_offset;
            let pieces = map.map_cipher_range(NzbgetRsByteRange { offset: 32, size: 32 });
            assert_eq!(
                pieces,
                vec![
                    NzbgetRsMemberRange {
                        member_index: 0,
                        offset: data_off0 + 32,
                        size: 13,
                    },
                    NzbgetRsMemberRange {
                        member_index: 1,
                        offset: data_off1,
                        size: 19,
                    },
                ]
            );
            assert!(map
                .map_cipher_range(NzbgetRsByteRange { offset: 0, size: 113 })
                .is_empty());
            assert!(map
                .map_cipher_range(NzbgetRsByteRange { offset: 112, size: 16 })
                .is_empty());
            assert!(map
                .map_cipher_range(NzbgetRsByteRange { offset: -1, size: 4 })
                .is_empty());

            let mut out = vec![0u8; 100];
            assert!(map.read_inner_decrypted(
                &sources,
                NzbgetRsByteRange { offset: 0, size: 100 },
                &mut out
            ));
            assert_eq!(out, inner);
            for probe in [
                NzbgetRsByteRange { offset: 40, size: 10 },
                NzbgetRsByteRange { offset: 80, size: 10 },
                NzbgetRsByteRange { offset: 95, size: 5 },
                NzbgetRsByteRange { offset: 0, size: 1 },
                NzbgetRsByteRange { offset: 99, size: 1 },
                NzbgetRsByteRange { offset: 44, size: 3 },
            ] {
                let mut piece = vec![0u8; probe.size as usize];
                assert!(map.read_inner_decrypted(&sources, probe, &mut piece));
                let p_off = probe.offset as usize;
                assert_eq!(piece, &inner[p_off..p_off + probe.size as usize]);
            }
            assert!(map.read_inner_decrypted(
                &sources,
                NzbgetRsByteRange { offset: 50, size: 0 },
                &mut out
            ));
            assert!(!map.read_inner_decrypted(
                &sources,
                NzbgetRsByteRange { offset: 96, size: 10 },
                &mut out
            ));
            assert!(!map.read_inner_decrypted(
                &sources,
                NzbgetRsByteRange { offset: -16, size: 16 },
                &mut out
            ));

            let mut partial = MemorySourceSet::default();
            partial.push(build_rar3_enc_store_volume(
                c0,
                100,
                false,
                true,
                &salt,
                "inner.mkv",
            ));
            partial.push_null();
            partial.push(build_rar3_enc_store_volume(
                c2,
                100,
                true,
                false,
                &salt,
                "inner.mkv",
            ));
            assert!(map.read_inner_decrypted(
                &partial,
                NzbgetRsByteRange { offset: 0, size: 32 },
                &mut out
            ));
            assert_eq!(&out[..32], &inner[..32]);
            assert!(!map.read_inner_decrypted(
                &partial,
                NzbgetRsByteRange { offset: 50, size: 10 },
                &mut out
            ));
            assert!(!map.read_inner_decrypted(
                &partial,
                NzbgetRsByteRange { offset: 0, size: 48 },
                &mut out
            ));
            assert!(!map.read_inner_decrypted(
                &partial,
                NzbgetRsByteRange { offset: 96, size: 4 },
                &mut out
            ));
        }

        // 22. ContentMapperRar5EncryptedNonAlignedTest
        {
            let inner = cm_pattern(60, 27);
            let salt = [
                0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd,
                0xce, 0xcf,
            ];
            let iv = [
                0xd0, 0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xdb, 0xdc, 0xdd,
                0xde, 0xdf,
            ];
            let kdf = 10u8;
            let check8 = streamcrypto_derive_rar5_psw_check(b"123", kdf, &salt).unwrap();
            let mut check = [0u8; 12];
            check[..8].copy_from_slice(&check8);
            let crypt = crate::rarreader::Rar5Crypt {
                version: 0,
                flags: 1,
                kdf_count: kdf,
                salt,
                iv,
                check_value: check,
                has_check: true,
            };
            let ctx = RarCryptoContext::make_rar5(b"123", &crypt).unwrap();
            let cipher = encrypt_padded(&ctx, &inner);
            let c0 = &cipher[..23];
            let c1 = &cipher[23..];
            let members = vec![
                SetMember::new("rel.part01.rar", 0),
                SetMember::new("rel.part02.rar", 0),
            ];
            let set = MemberSet {
                format: MemberFormat::Rar,
                members: vec![0, 1],
            };
            {
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar5_enc_store_volume(
                    c0,
                    60,
                    false,
                    true,
                    kdf,
                    &salt,
                    &iv,
                    Some(&check),
                    "inner.mkv",
                ));
                sources.push(build_rar5_enc_store_volume(
                    c1,
                    60,
                    true,
                    false,
                    kdf,
                    &salt,
                    &iv,
                    Some(&check),
                    "inner.mkv",
                ));
                let map = ContentMapper::build_map(&members, &set, &sources, Some("123")).unwrap();
                assert_eq!(map.runs().len(), 2);
                assert_eq!(map.runs()[0].size, 23);
                assert_eq!(map.runs()[1].size, 37);
                assert_eq!(map.run_crypto(1).unwrap().cipher_size, 41);
                let mut out = vec![0u8; 60];
                assert!(map.read_inner_decrypted(
                    &sources,
                    NzbgetRsByteRange { offset: 0, size: 60 },
                    &mut out
                ));
                assert_eq!(out, inner);
                assert!(map.read_inner_decrypted(
                    &sources,
                    NzbgetRsByteRange { offset: 20, size: 10 },
                    &mut out
                ));
                assert_eq!(&out[..10], &inner[20..30]);
            }
            {
                let mut other_iv = iv;
                other_iv[0] ^= 0x01;
                let mut sources = MemorySourceSet::default();
                sources.push(build_rar5_enc_store_volume(
                    c0,
                    60,
                    false,
                    true,
                    kdf,
                    &salt,
                    &iv,
                    Some(&check),
                    "inner.mkv",
                ));
                sources.push(build_rar5_enc_store_volume(
                    c1,
                    60,
                    true,
                    false,
                    kdf,
                    &salt,
                    &other_iv,
                    Some(&check),
                    "inner.mkv",
                ));
                let err = ContentMapper::build_map(&members, &set, &sources, Some("123"))
                    .err()
                    .unwrap();
                assert_eq!(err, "encrypted volume geometry does not fit store mode");
            }
        }

        // 23. ContentMapperEncryptedRepairSetsTest
        {
            let inner = cm_pattern(50, 33);
            let salt = [8, 7, 6, 5, 4, 3, 2, 1];
            let ctx = RarCryptoContext::make_rar3(b"123", &salt).unwrap();
            let cipher = encrypt_padded(&ctx, &inner);
            let volume = build_rar3_enc_store_volume(&cipher, 50, false, false, &salt, "inner.mkv");
            let data_start = 69i64;

            let members = vec![SetMember::new("rel.part01.rar", volume.len() as i64)];
            let member_holes = vec![vec![NzbgetRsByteRange {
                offset: data_start + 20,
                size: 10,
            }]];
            let mut raw = MemorySourceSet::default();
            raw.push(volume);
            let sources = HoledSourceSet::new(&raw, &member_holes);

            let without = ContentMapper::build_repair_sets(&members, &member_holes, &sources, None);
            assert_eq!(without.len(), 1);
            assert!(without[0].map.is_none());
            assert_eq!(without[0].skip_reason, "encrypted archive data");

            let with =
                ContentMapper::build_repair_sets(&members, &member_holes, &sources, Some("123"));
            assert_eq!(with.len(), 1);
            let map = with[0].map.as_ref().unwrap();
            assert!(map.encrypted());
            assert_eq!(
                with[0].inner_holes,
                vec![NzbgetRsByteRange { offset: 20, size: 10 }]
            );
        }

        // 24. ContentMapperEncryptedTargetHeaderHoleTest
        {
            let inner = cm_pattern(50, 71);
            let salt = [0xde, 0xad, 0xbe, 0xef, 0x01, 0x02, 0x03, 0x04];
            let ctx = RarCryptoContext::make_rar3(b"123", &salt).unwrap();
            let cipher = encrypt_padded(&ctx, &inner);
            let volume = build_rar3_enc_store_volume(&cipher, 50, false, false, &salt, "inner.mkv");
            let data_start = 69i64;
            let members = vec![SetMember::new("rel.part01.rar", volume.len() as i64)];

            {
                let member_holes = vec![vec![NzbgetRsByteRange { offset: 30, size: 12 }]];
                let mut raw = MemorySourceSet::default();
                raw.push(volume.clone());
                let sources = HoledSourceSet::new(&raw, &member_holes);
                let sets = ContentMapper::build_repair_sets(
                    &members,
                    &member_holes,
                    &sources,
                    Some("123"),
                );
                assert_eq!(sets.len(), 1);
                assert!(sets[0].map.is_none());
                assert!(!sets[0].skip_reason.is_empty());
            }
            {
                let member_holes = vec![vec![NzbgetRsByteRange {
                    offset: data_start + 16,
                    size: 10,
                }]];
                let mut raw = MemorySourceSet::default();
                raw.push(volume);
                let sources = HoledSourceSet::new(&raw, &member_holes);
                let sets = ContentMapper::build_repair_sets(
                    &members,
                    &member_holes,
                    &sources,
                    Some("123"),
                );
                assert_eq!(sets.len(), 1);
                let map = sets[0].map.as_ref().unwrap();
                assert!(map.encrypted());
                assert_eq!(
                    sets[0].inner_holes,
                    vec![NzbgetRsByteRange { offset: 16, size: 10 }]
                );
            }
        }

        // 25. ContentMapperRarEncryptedHpDonorTest
        {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/testdata/rarrenamer");
            for file in ["testfile3encnam.part01.rar", "testfile5encnam.part01.rar"] {
                let bytes = std::fs::read(dir.join(file)).unwrap();
                assert!(bytes.len() > 100);
                let members = vec![SetMember::new(file, 0)];
                let single = MemberSet {
                    format: MemberFormat::Rar,
                    members: vec![0],
                };
                let mut no_pw = MemorySourceSet::default();
                no_pw.push(bytes.clone());
                let reason = ContentMapper::build_map(&members, &single, &no_pw, None)
                    .err()
                    .unwrap();
                assert_eq!(reason, "encrypted archive headers");

                let mut with_pw = MemorySourceSet::default();
                with_pw.push(bytes);
                let reason2 = ContentMapper::build_map(&members, &single, &with_pw, Some("123"))
                    .err()
                    .unwrap();
                assert_ne!(reason2, "encrypted archive headers");
                assert!(!reason2.is_empty());
            }
        }

        // 26. ContentMapperIsCompressibleArchiveTest
        {
            assert!(ContentMapper::is_compressible_archive(&MemberSet {
                format: MemberFormat::Rar,
                members: vec![0],
            }));
            assert!(ContentMapper::is_compressible_archive(&MemberSet {
                format: MemberFormat::Zip,
                members: vec![0],
            }));
            assert!(ContentMapper::is_compressible_archive(&MemberSet {
                format: MemberFormat::SevenZip,
                members: vec![0],
            }));
            assert!(!ContentMapper::is_compressible_archive(&MemberSet {
                format: MemberFormat::Bare,
                members: vec![0],
            }));
            assert!(!ContentMapper::is_compressible_archive(&MemberSet {
                format: MemberFormat::Split,
                members: vec![0, 1],
            }));
        }

        // 27. ContentMapRustBoundaryTest
        {
            let m_ids = [10i32, 20];
            let d_bases = [0i64, 100];
            let m_sizes = [100i64, 100];
            let n_comp = composite_to_members(
                &m_ids,
                &d_bases,
                &m_sizes,
                NzbgetRsByteRange {
                    offset: 50,
                    size: 100,
                },
            );
            assert_eq!(n_comp.len(), 2);

            let holes = [NzbgetRsByteRange { offset: 10, size: 20 }];
            assert!(holed_source_intersects(&holes, 15, 5));
            assert!(!holed_source_intersects(&holes, 0, 10));

            let runs = [NzbgetRsContentRun {
                inner_offset: 0,
                member_index: 0,
                member_offset: 100,
                size: 50,
            }];
            assert_eq!(
                content_map_to_inner(
                    &runs,
                    0,
                    NzbgetRsByteRange {
                        offset: 110,
                        size: 20,
                    },
                )
                .len(),
                1
            );
            assert_eq!(
                content_map_from_inner(&runs, NzbgetRsByteRange { offset: 10, size: 20 }).len(),
                1
            );
            let c_offs = [100i64];
            let c_sizes = [64i64];
            let has_c = [1u8];
            assert_eq!(
                content_map_cipher_range(
                    &runs,
                    &c_offs,
                    &c_sizes,
                    &has_c,
                    NzbgetRsByteRange { offset: 0, size: 32 },
                )
                .len(),
                1
            );

            let grouped = content_mapper_group_sets(&[b"movie.part01.rar", b"movie.part02.rar"]);
            assert_eq!(grouped.len(), 1);

            let uncoalesced = [
                NzbgetRsByteRange { offset: 0, size: 10 },
                NzbgetRsByteRange { offset: 10, size: 10 },
            ];
            assert_eq!(content_mapper_coalesce_ranges(&uncoalesced).len(), 1);

            let member_idx = [0i32];
            let member_sz = [10i64];
            assert!(content_mapper_build_zip_map(&member_idx, &member_sz, |_, _, _| false).is_err());
            assert!(content_mapper_build_seven_zip_map(&member_idx, &member_sz, |_, _, _| false).is_err());

            // Verify on_rar_volume_parsed is invoked for RAR volume mapping diagnostics.
            let rar_vol =
                build_rar5_store_volume(&[b'M'; 64], 64, false, false, "movie.mkv", 0);
            struct RarDiagSourceSet {
                vol: MemoryContentSource,
                logged_vols: std::cell::RefCell<Vec<(String, i32, usize, String)>>,
            }
            impl ContentSourceSet for RarDiagSourceSet {
                fn get_source(&self, member_index: i32) -> Option<&dyn ContentSource> {
                    (member_index == 0).then_some(&self.vol as &dyn ContentSource)
                }
                fn on_rar_volume_parsed(
                    &self,
                    member_name: &[u8],
                    vol: &crate::rarreader::Volume,
                    files: &[crate::rarreader::RarFile],
                ) {
                    let vname = String::from_utf8_lossy(member_name).into_owned();
                    let fname = files
                        .first()
                        .map(|f| String::from_utf8_lossy(&f.filename).into_owned())
                        .unwrap_or_default();
                    self.logged_vols
                        .borrow_mut()
                        .push((vname, vol.version, files.len(), fname));
                }
            }
            let diag_sources = RarDiagSourceSet {
                vol: MemoryContentSource(rar_vol),
                logged_vols: std::cell::RefCell::new(Vec::new()),
            };
            let rar_members = [SetMember::new(b"show.rar", diag_sources.vol.size())];
            let rar_set = MemberSet {
                format: MemberFormat::Rar,
                members: vec![0],
            };
            assert!(
                ContentMapper::build_map_with_tls(
                    &rar_members,
                    &rar_set,
                    &diag_sources,
                    None,
                    true,
                )
                .is_ok()
            );
            assert_eq!(
                diag_sources.logged_vols.into_inner(),
                vec![("show.rar".to_string(), 5, 1, "movie.mkv".to_string())]
            );

            // Verify FFI CrossPackSession emits RAR volume and file debug diagnostics
            // for both target initialization and donor mapping.
            let inner_payload: Vec<u8> = (0..1024).map(|i| ((i * 37 + 11) & 0xff) as u8).collect();
            let donor_rar_vol = build_rar5_store_volume(
                &inner_payload,
                1024,
                false,
                false,
                "movie.mkv",
                0,
            );
            let mut target_rar_vol = donor_rar_vol.clone();
            let hole_off = (donor_rar_vol.len() - 512) as i64;
            target_rar_vol[hole_off as usize..hole_off as usize + 128].fill(0);

            let tmp_dir = std::env::temp_dir().join(format!(
                "rustyget_rar_diag_ffi_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let _ = std::fs::remove_dir_all(&tmp_dir);
            std::fs::create_dir_all(&tmp_dir).unwrap();
            std::fs::write(tmp_dir.join("target.rar"), &target_rar_vol).unwrap();

            let dest_c = std::ffi::CString::new(tmp_dir.to_string_lossy().as_bytes()).unwrap();
            let target_name_c = c"target.rar";
            let member_ptrs = [target_name_c.as_ptr()];
            let target_ptrs = [target_name_c.as_ptr()];
            let target_sizes = [target_rar_vol.len() as i64];
            let target_eligible = [1u8];
            let target_hole_counts = [1usize];
            let target_holes_flat = [NzbgetRsByteRange {
                offset: hole_off,
                size: 128,
            }];

            unsafe extern "C" fn capture_debug_line(
                ctx: *mut std::ffi::c_void,
                line: *const std::ffi::c_char,
            ) {
                let out = unsafe { &mut *(ctx as *mut Vec<String>) };
                let s = unsafe { std::ffi::CStr::from_ptr(line) }
                    .to_string_lossy()
                    .into_owned();
                out.push(s);
            }

            let mut target_debug_lines: Vec<String> = Vec::new();
            let session = unsafe {
                crate::ffi::nzbget_rs_cross_pack_session_new(
                    dest_c.as_ptr(),
                    member_ptrs.as_ptr(),
                    1,
                    target_ptrs.as_ptr(),
                    target_sizes.as_ptr(),
                    target_eligible.as_ptr(),
                    target_hole_counts.as_ptr(),
                    target_holes_flat.as_ptr(),
                    1,
                    std::ptr::null(),
                    (&mut target_debug_lines as *mut Vec<String>).cast(),
                    None,
                    None,
                    Some(capture_debug_line),
                )
            };
            assert!(!session.is_null());
            assert_eq!(
                target_debug_lines,
                vec![
                    "Volume: version:5, multi:1, vol-no:0, new-naming:1, has-next:0, encrypted:0, file-count:1, [target.rar]".to_string(),
                    "  time:0, size:1024, attr:0, split-before:0, split-after:0, [movie.mkv]".to_string(),
                ]
            );

            struct FfiDonorDiagCtx {
                donor_bytes: Vec<u8>,
                debug_lines: Vec<String>,
            }
            unsafe extern "C" fn fetch_donor_rar_part(
                ctx: *mut std::ffi::c_void,
                member_index: i32,
                part_index: i32,
                out_success: *mut std::ffi::c_int,
                out_offset: *mut i64,
                out_file_size: *mut i64,
                out_data: *mut *const u8,
                out_data_len: *mut usize,
            ) -> std::ffi::c_int {
                if member_index != 0 || part_index != 0 {
                    return 0;
                }
                let dctx = unsafe { &*(ctx as *const FfiDonorDiagCtx) };
                unsafe {
                    *out_success = 1;
                    *out_offset = 0;
                    *out_file_size = dctx.donor_bytes.len() as i64;
                    *out_data = dctx.donor_bytes.as_ptr();
                    *out_data_len = dctx.donor_bytes.len();
                }
                1
            }
            unsafe extern "C" fn capture_donor_debug_line(
                ctx: *mut std::ffi::c_void,
                line: *const std::ffi::c_char,
            ) {
                let dctx = unsafe { &mut *(ctx as *mut FfiDonorDiagCtx) };
                let s = unsafe { std::ffi::CStr::from_ptr(line) }
                    .to_string_lossy()
                    .into_owned();
                dctx.debug_lines.push(s);
            }

            let mut donor_ctx = FfiDonorDiagCtx {
                donor_bytes: donor_rar_vol.clone(),
                debug_lines: Vec::new(),
            };
            let donor_name_c = c"donor.rar";
            let donor_file_ptrs = [donor_name_c.as_ptr()];
            let donor_file_sizes = [donor_rar_vol.len() as i64];
            let donor_art_counts = [1usize];
            let donor_art_sizes = [donor_rar_vol.len() as i32];

            unsafe {
                crate::ffi::nzbget_rs_cross_pack_session_process_donor(
                    session,
                    c"".as_ptr(),
                    c"Donor.RAR".as_ptr(),
                    c"".as_ptr(),
                    donor_file_ptrs.as_ptr(),
                    donor_file_sizes.as_ptr(),
                    donor_art_counts.as_ptr(),
                    donor_art_sizes.as_ptr(),
                    1,
                    0,
                    (&mut donor_ctx as *mut FfiDonorDiagCtx).cast(),
                    None,
                    Some(fetch_donor_rar_part),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(capture_donor_debug_line),
                );
            }
            assert_eq!(
                donor_ctx.debug_lines,
                vec![
                    "Volume: version:5, multi:1, vol-no:0, new-naming:1, has-next:0, encrypted:0, file-count:1, [donor.rar]".to_string(),
                    "  time:0, size:1024, attr:0, split-before:0, split-after:0, [movie.mkv]".to_string(),
                ]
            );
            assert_eq!(
                unsafe { crate::ffi::nzbget_rs_cross_pack_session_has_inner_holes(session) },
                0
            );
            unsafe { crate::ffi::nzbget_rs_cross_pack_session_free(session) };
            let _ = std::fs::remove_dir_all(&tmp_dir);
        }
    }

    struct MemTargetFileSet {
        files: Vec<Vec<u8>>,
        messages: Vec<(TargetMessageKind, String)>,
        stopped: bool,
    }

    impl MemTargetFileSet {
        fn new(files: Vec<Vec<u8>>) -> Self {
            Self {
                files,
                messages: Vec::new(),
                stopped: false,
            }
        }
    }

    impl TargetFileSet for MemTargetFileSet {
        fn is_stopped(&self) -> bool {
            self.stopped
        }

        fn has_file(&mut self, member_index: i32) -> bool {
            member_index >= 0 && (member_index as usize) < self.files.len()
        }

        fn read_file(&mut self, member_index: i32, offset: i64, buf: &mut [u8]) -> bool {
            let Some(file) = self.files.get(member_index as usize) else {
                return false;
            };
            if offset < 0 || (offset as usize).saturating_add(buf.len()) > file.len() {
                return false;
            }
            let start = offset as usize;
            buf.copy_from_slice(&file[start..start + buf.len()]);
            true
        }

        fn write_file(&mut self, member_index: i32, offset: i64, data: &[u8]) -> bool {
            let Some(file) = self.files.get_mut(member_index as usize) else {
                return false;
            };
            if offset < 0 || (offset as usize).saturating_add(data.len()) > file.len() {
                return false;
            }
            let start = offset as usize;
            file[start..start + data.len()].copy_from_slice(data);
            true
        }

        fn on_message(&mut self, kind: TargetMessageKind, text: &str) {
            self.messages.push((kind, text.to_string()));
        }
    }

    #[test]
    fn inner_space_plain_verify_and_patch_with_containment_guards() {
        let canonical: Vec<u8> = (0..512u32).map(|i| ((i * 37 + 11) & 0xff) as u8).collect();
        let mut target_disk = canonical.clone();
        // Zero out hole [128, 256) on target disk
        target_disk[128..256].fill(0);

        let members = vec![SetMember::new(b"movie.mkv", 512)];
        let holes = vec![vec![NzbgetRsByteRange {
            offset: 128,
            size: 128,
        }]];
        let mut target_src = MemorySourceSet::default();
        target_src.push(target_disk.clone());
        let mut repair_sets =
            ContentMapper::build_repair_sets(&members, &holes, &target_src, None);
        assert_eq!(repair_sets.len(), 1);

        let donor_members = vec![
            SetMember::new(b"movie.mkv.001", 256),
            SetMember::new(b"movie.mkv.002", 256),
        ];
        let mut donor_src = MemorySourceSet::default();
        donor_src.push(canonical[..256].to_vec());
        donor_src.push(canonical[256..].to_vec());
        let donor_set = MemberSet {
            format: MemberFormat::Split,
            members: vec![0, 1],
        };
        let donor_map = ContentMapper::build_map(&donor_members, &donor_set, &donor_src, None)
            .expect("split donor should map");

        let mut target_files = MemTargetFileSet::new(vec![target_disk.clone()]);
        assert!(repair_sets[0].verify_donor_set(&donor_map, &donor_src, &mut target_files));

        // Corrupt donor bytes in the probe window and verify rejection
        let mut bad_first = canonical[..256].to_vec();
        bad_first[0] ^= 0xff;
        let mut bad_donor_src = MemorySourceSet::default();
        bad_donor_src.push(bad_first);
        bad_donor_src.push(canonical[256..].to_vec());
        assert!(!repair_sets[0].verify_donor_set(&donor_map, &bad_donor_src, &mut target_files));

        // Containment guard: when target_holes does not cover the hole, write is blocked
        let mut empty_target_holes = vec![Vec::new()];
        let (wrote_blocked, credit_blocked) = repair_sets[0].patch_from_donor_set(
            &donor_map,
            &donor_src,
            &mut target_files,
            &mut empty_target_holes,
            &[0],
            &members,
            "Donor.Split",
        );
        assert_eq!(wrote_blocked, 0);
        assert!(!credit_blocked);
        assert!(target_files
            .messages
            .iter()
            .any(|(k, m)| *k == TargetMessageKind::Warning && m.contains("captured hole blocked")));

        // Valid patch fills the hole and updates both inner_holes and target_holes
        let mut live_target_holes = holes.clone();
        let (written, credit) = repair_sets[0].patch_from_donor_set(
            &donor_map,
            &donor_src,
            &mut target_files,
            &mut live_target_holes,
            &[0],
            &members,
            "Donor.Split",
        );
        assert_eq!(written, 128);
        assert!(credit);
        assert!(repair_sets[0].inner_holes.is_empty());
        assert!(live_target_holes[0].is_empty());
        assert_eq!(target_files.files[0], canonical);
    }

    #[test]
    fn inner_space_encrypted_m3_and_m4_verify_and_patch() {
        let key = [0x42u8; 16];
        let iv = [0x11u8; 16];
        let crypto = std::sync::Arc::new(
            RarCryptoContext::from_key_iv(&key, iv).expect("valid AES-128 context"),
        );

        let plain: Vec<u8> = (0..256u32).map(|i| ((i * 53 + 7) & 0xff) as u8).collect();
        let mut cipher = vec![0u8; 256];
        assert!(crypto.encrypt_range(None, &plain, &mut cipher, 16));

        let mut damaged_cipher = cipher.clone();
        // Hole at [64, 112) (3 AES blocks)
        damaged_cipher[64..112].fill(0);

        let mut target_map = ContentMap::default();
        target_map.set_inner_name("secret.bin");
        target_map.set_inner_size(256);
        target_map.runs_mut().push(NzbgetRsContentRun {
            inner_offset: 0,
            member_index: 0,
            member_offset: 0,
            size: 256,
        });
        target_map.run_cryptos_mut().push(RunCrypto {
            crypto: Some(crypto),
            cipher_data_offset: 0,
            cipher_size: 256,
        });

        let hole = NzbgetRsByteRange {
            offset: 64,
            size: 48,
        };
        let mut repair_set = RepairSetData {
            set: MemberSet {
                format: MemberFormat::Rar,
                members: vec![0],
            },
            map: Some(target_map),
            inner_holes: vec![hole],
            original_inner_holes: vec![hole],
            skip_reason: String::new(),
        };

        let mut donor_map = ContentMap::default();
        donor_map.set_inner_name("secret.bin");
        donor_map.set_inner_size(256);
        donor_map.runs_mut().push(NzbgetRsContentRun {
            inner_offset: 0,
            member_index: 0,
            member_offset: 0,
            size: 256,
        });
        let mut donor_src = MemorySourceSet::default();
        donor_src.push(plain.clone());

        let mut target_files = MemTargetFileSet::new(vec![damaged_cipher.clone()]);
        assert!(repair_set.verify_donor_set(&donor_map, &donor_src, &mut target_files));

        let members = vec![SetMember::new(b"secret.rar", 256)];
        let mut target_holes = vec![vec![hole]];
        let (written, credit) = repair_set.patch_from_donor_set(
            &donor_map,
            &donor_src,
            &mut target_files,
            &mut target_holes,
            &[0],
            &members,
            "Donor.Plain",
        );
        assert_eq!(written, 48);
        assert!(credit);
        assert!(repair_set.inner_holes.is_empty());
        assert!(target_holes[0].is_empty());
        assert_eq!(target_files.files[0], cipher);

        // Also test M4 (verify_donor_inner / patch_from_donor_inner) into an encrypted target
        repair_set.inner_holes = vec![hole];
        let mut target_files_m4 = MemTargetFileSet::new(vec![damaged_cipher]);
        let mut target_holes_m4 = vec![vec![hole]];
        let inner_source = MemoryContentSource(plain);
        assert!(repair_set.verify_donor_inner(&inner_source, 256, &mut target_files_m4));
        let (written_m4, holes_filled_m4, credit_m4) = repair_set.patch_from_donor_inner(
            &inner_source,
            &mut target_files_m4,
            &mut target_holes_m4,
            &[0],
            &members,
            "Donor.Decompressed",
        );
        assert_eq!(written_m4, 48);
        assert_eq!(holes_filled_m4, 1);
        assert!(credit_m4);
        assert_eq!(target_files_m4.files[0], cipher);
    }

    struct MockDonorTransport {
        parts: Vec<FetchedDonorArticle>,
        budget: Option<i32>,
        allow_prefetch: bool,
        prefetch_queue: std::collections::VecDeque<FetchedDonorArticle>,
        single_fetches: usize,
        batch_begins: usize,
    }

    impl DonorArticleTransport for MockDonorTransport {
        fn has_prefetch_batch(&self) -> bool {
            self.allow_prefetch && self.budget.is_none()
        }

        fn fetch_part(&mut self, part_index: i32) -> Option<FetchedDonorArticle> {
            if let Some(budget) = &mut self.budget {
                if *budget <= 0 {
                    return None;
                }
                *budget -= 1;
            }
            self.single_fetches += 1;
            self.parts.get(part_index as usize).cloned()
        }

        fn begin_prefetch_batch(&mut self, part_indices: &[i32]) {
            self.batch_begins += 1;
            self.prefetch_queue.clear();
            for &idx in part_indices {
                if let Some(article) = self.parts.get(idx as usize).cloned() {
                    self.prefetch_queue.push_back(article);
                }
            }
        }

        fn next_prefetch_part(&mut self) -> Option<FetchedDonorArticle> {
            self.prefetch_queue.pop_front()
        }

        fn cancel_prefetch_batch(&mut self) {
            self.prefetch_queue.clear();
        }
    }

    #[test]
    fn donor_member_source_caches_prefetches_and_resolves_drift() {
        // 20 parts of 100 bytes each (total 2000 bytes), but actual offsets are shifted
        // by +50 bytes on parts 1..19 (part 0 is 150 bytes, part 19 is 50 bytes) to test drift resolution.
        let mut parts = Vec::new();
        let mut full_data = Vec::new();
        for i in 0..20i32 {
            let (offset, len) = if i == 0 {
                (0i64, 150usize)
            } else if i == 19 {
                (1950i64, 50usize)
            } else {
                (150 + ((i - 1) as i64) * 100, 100usize)
            };
            let data: Vec<u8> = (0..len)
                .map(|b| (((offset as usize) + b) * 37 + 11) as u8)
                .collect();
            full_data.extend_from_slice(&data);
            parts.push(FetchedDonorArticle {
                success: true,
                offset,
                file_size: 2000,
                data,
            });
        }
        let article_sizes = vec![110i32; 20];
        let mut src = DonorMemberSource::new(article_sizes);
        let mut transport = MockDonorTransport {
            parts,
            budget: Some(1),
            allow_prefetch: true,
            prefetch_queue: std::collections::VecDeque::new(),
            single_fetches: 0,
            batch_begins: 0,
        };

        // Size consumes the 1-article budget for part 0
        assert_eq!(src.size_with_transport(&mut transport), 2000);
        assert_eq!(transport.single_fetches, 1);

        // Reading part 5 with exhausted budget fails without poisoning cache
        let mut buf = vec![0u8; 250];
        assert!(!src.read_with_transport(&mut transport, 500, &mut buf));

        // Removing budget allows prefetch + drift-corrected read across parts
        transport.budget = None;
        assert!(src.read_with_transport(&mut transport, 500, &mut buf));
        assert_eq!(&buf[..], &full_data[500..750]);
        assert!(transport.batch_begins >= 1);
        let served = src.take_served_parts();
        assert!(served >= 3);
        assert_eq!(src.take_served_parts(), 0);
    }

    struct MockBatchClient {
        articles: Vec<FetchedDonorArticle>,
        queue: std::collections::VecDeque<FetchedDonorArticle>,
        stopped: bool,
    }

    impl DonorBatchClient for MockBatchClient {
        fn is_stopped(&self) -> bool {
            self.stopped
        }

        fn begin_batch(&mut self, part_indices: &[i32]) {
            self.queue.clear();
            for &idx in part_indices {
                if let Some(a) = self.articles.get(idx as usize).cloned() {
                    self.queue.push_back(a);
                }
            }
        }

        fn next_article(&mut self) -> Option<FetchedDonorArticle> {
            self.queue.pop_front()
        }

        fn cancel_remaining(&mut self) {
            self.queue.clear();
        }
    }

    struct MockFileTarget {
        bytes: Vec<u8>,
        progress: i64,
        messages: Vec<(TargetMessageKind, Vec<u8>)>,
    }

    impl TargetFileReader for MockFileTarget {
        fn read_range(&mut self, offset: i64, buf: &mut [u8]) -> bool {
            if offset < 0 {
                return false;
            }
            let start = offset as usize;
            let end = start.saturating_add(buf.len());
            if end > self.bytes.len() {
                return false;
            }
            buf.copy_from_slice(&self.bytes[start..end]);
            true
        }
    }

    impl TargetFilePatcher for MockFileTarget {
        fn write_range(&mut self, offset: i64, data: &[u8]) -> bool {
            if offset < 0 {
                return false;
            }
            let start = offset as usize;
            let end = start.saturating_add(data.len());
            if end > self.bytes.len() {
                return false;
            }
            self.bytes[start..end].copy_from_slice(data);
            true
        }

        fn add_progress(&mut self, bytes: i64) {
            self.progress += bytes;
        }

        fn emit_message_bytes(&mut self, kind: TargetMessageKind, text: &[u8]) {
            self.messages.push((kind, text.to_vec()));
        }
    }

    #[test]
    fn m1_verify_and_patch_donor_file_handles_missing_probes_and_drift() {
        let file_size = 100_000usize;
        let canonical: Vec<u8> = (0..file_size).map(|i| ((i * 73 + 19) & 0xff) as u8).collect();
        let mut damaged = canonical.clone();
        // Hole at [40_000, 50_000)
        damaged[40_000..50_000].fill(0);
        let mut holes = vec![NzbgetRsByteRange {
            offset: 40_000,
            size: 10_000,
        }];

        // 10 donor parts of 10_000 bytes each; mark first probe part missing so replacement probe is exercised
        let mut donor_ranges = Vec::new();
        let mut articles = Vec::new();
        for i in 0..10usize {
            let offset = (i * 10_000) as i64;
            donor_ranges.push(NzbgetRsByteRange {
                offset,
                size: 10_000,
            });
            articles.push(FetchedDonorArticle {
                success: i != 1, // force a missing probe to test replacement selection
                offset,
                file_size: file_size as i64,
                data: if i == 1 {
                    Vec::new()
                } else {
                    canonical[i * 10_000..(i + 1) * 10_000].to_vec()
                },
            });
        }

        let mut batch_client = MockBatchClient {
            articles,
            queue: std::collections::VecDeque::new(),
            stopped: false,
        };
        let mut target = MockFileTarget {
            bytes: damaged,
            progress: 0,
            messages: Vec::new(),
        };

        assert!(verify_donor_file(
            file_size as i64,
            &holes,
            &donor_ranges,
            &mut batch_client,
            &mut target,
        ));

        let res = patch_from_donor_file(
            b"movie-\x80\xff.mkv",
            file_size as i64,
            &mut holes,
            &donor_ranges,
            b"Donor-\xfe.1080p",
            &mut batch_client,
            &mut target,
        );
        assert_eq!(res.recovered_parts, 1);
        assert_eq!(res.recovered_bytes, 10_000);
        assert!(res.credit_recovery);
        assert!(holes.is_empty());
        assert_eq!(target.bytes, canonical);
        assert_eq!(target.progress, 10_000);
        assert_eq!(target.messages.len(), 1);
        assert_eq!(
            target.messages[0].1,
            b"Recovered 0.0 MB (1 donor article(s)) of movie-\x80\xff.mkv from duplicate Donor-\xfe.1080p"
        );
    }

    fn reference_probe_parts(
        donor_ranges: &[NzbgetRsByteRange],
        holes: &[NzbgetRsByteRange],
        probe_count: i32,
    ) -> Vec<i32> {
        let mut excluded = vec![false; donor_ranges.len()];
        for (i, &dr) in donor_ranges.iter().enumerate() {
            for &hole in holes {
                if dr.offset < range_end(hole) && hole.offset < range_end(dr) {
                    excluded[i] = true;
                    if i > 0 {
                        excluded[i - 1] = true;
                    }
                    if i + 1 < donor_ranges.len() {
                        excluded[i + 1] = true;
                    }
                    break;
                }
            }
        }
        let candidates: Vec<i32> = excluded
            .iter()
            .enumerate()
            .filter_map(|(i, &ex)| (!ex).then_some(i as i32))
            .collect();
        if probe_count <= 0 {
            return Vec::new();
        }
        let wanted = probe_count as usize;
        if candidates.len() <= wanted {
            return candidates;
        }
        let mut probes = Vec::with_capacity(wanted);
        for k in 0..wanted {
            probes.push(candidates[candidates.len() * (2 * k + 1) / (2 * wanted)]);
        }
        probes
    }

    /// Full pure-Rust port of `tests/queue/DupeStreamRepair.cpp` (all `BOOST_AUTO_TEST_CASE` tests).
    #[test]
    fn dupe_stream_repair_full_suite_port_from_cpp() {
        let br = |offset: i64, size: i64| NzbgetRsByteRange { offset, size };

        // 1. StreamRepairEligibilityTest
        assert!(is_stream_eligible(b"movie.mkv"));
        assert!(is_stream_eligible(b"MOVIE.MKV"));
        assert!(is_stream_eligible(b"clip.mp4"));
        assert!(is_stream_eligible(b"show.ts"));
        assert!(!is_stream_eligible(b"release.r01"));
        assert!(!is_stream_eligible(b"archive.rar"));
        assert!(!is_stream_eligible(b"movie.par2"));
        assert!(!is_stream_eligible(b"noextension"));
        assert!(!is_stream_eligible(b""));

        // 2. StreamRepairComputeHolesTest
        let holes = compute_holes(&[br(0, 300), br(500, 300)], 1000);
        assert_eq!(holes, vec![br(300, 200), br(800, 200)]);
        assert_eq!(total_size(&holes), 400);

        // 3. StreamRepairComputeHolesLeadingHoleTest
        let leading = compute_holes(&[br(200, 400)], 600);
        assert_eq!(leading, vec![br(0, 200)]);

        // 4. StreamRepairComputeHolesCompleteAndUnknownTest
        assert!(compute_holes(&[br(0, 300), br(300, 400)], 700).is_empty());
        assert!(compute_holes(&[br(0, 300)], 0).is_empty());

        // 5. StreamRepairComputeHolesOverlapTest
        let overlap = compute_holes(&[br(500, 300), br(0, 400), br(300, 300)], 1000);
        assert_eq!(overlap, vec![br(800, 200)]);

        // 6. StreamRepairEstimateDonorRangesTest
        let uniform = estimate_donor_ranges(&[250, 250, 250, 250], 1000);
        assert_eq!(
            uniform,
            vec![br(0, 250), br(250, 250), br(500, 250), br(750, 250)]
        );
        let uneven = estimate_donor_ranges(&[300, 100, 100], 1000);
        assert_eq!(uneven, vec![br(0, 600), br(600, 200), br(800, 200)]);
        assert!(estimate_donor_ranges(&[], 1000).is_empty());
        assert!(estimate_donor_ranges(&[250, 250, 250, 250], 0).is_empty());

        // 7. StreamRepairSelectPatchPartsTest
        let pr = |offset: i64, size: i64| crate::streampatchparts::Range { offset, size };
        let donor_patch_ranges: Vec<_> = (0..10).map(|i| pr(i * 100, 100)).collect();
        let mut patch_out = vec![0i32; 10];
        let n_picks =
            crate::streampatchparts::select(&donor_patch_ranges, &[pr(450, 100)], 1, &mut patch_out);
        assert_eq!(&patch_out[..n_picks], &[3, 4, 5, 6]);
        let n_edge =
            crate::streampatchparts::select(&donor_patch_ranges, &[pr(0, 50)], 2, &mut patch_out);
        assert_eq!(&patch_out[..n_edge], &[0, 1, 2]);
        assert_eq!(
            crate::streampatchparts::select(&donor_patch_ranges, &[], 2, &mut patch_out),
            0
        );

        // 8. StreamRepairSelectProbePartsTest
        let donor_ranges: Vec<_> = (0..10).map(|i| br(i * 100, 100)).collect();
        assert_eq!(
            select_probe_parts(&donor_ranges, &[br(450, 100)], 2),
            vec![1, 8]
        );
        assert!(select_probe_parts(&donor_ranges, &[br(0, 1000)], 2).is_empty());
        assert!(select_probe_parts(&donor_ranges, &[br(0, 850)], 2).is_empty());

        // 9. StreamRepairSelectProbePartsBoundaryAndSamplingTest
        let donors5 = vec![
            br(0, 100),
            br(100, 100),
            br(200, 100),
            br(300, 100),
            br(400, 100),
        ];
        let boundary_cases: Vec<(Vec<NzbgetRsByteRange>, Vec<NzbgetRsByteRange>, i32, Vec<i32>)> =
            vec![
                (vec![], vec![br(0, 100)], 2, vec![]),
                (vec![br(0, 100)], vec![], 2, vec![0]),
                (vec![br(0, 100)], vec![br(100, 100)], 2, vec![0]),
                (vec![br(0, 100)], vec![br(0, 100)], 2, vec![]),
                (donors5.clone(), vec![], 0, vec![]),
                (donors5.clone(), vec![], -2, vec![]),
                (donors5.clone(), vec![], 1, vec![2]),
                (donors5.clone(), vec![], 2, vec![1, 3]),
                (donors5.clone(), vec![], 5, vec![0, 1, 2, 3, 4]),
                (donors5.clone(), vec![], 8, vec![0, 1, 2, 3, 4]),
                (
                    donors5.clone(),
                    vec![br(-100, 100), br(500, 100)],
                    5,
                    vec![0, 1, 2, 3, 4],
                ),
                (
                    donors5.clone(),
                    vec![br(-100, 100), br(0, 50)],
                    5,
                    vec![2, 3, 4],
                ),
                (donors5.clone(), vec![br(100, 100)], 5, vec![3, 4]),
                (donors5.clone(), vec![br(0, 150)], 3, vec![3, 4]),
                (donors5.clone(), vec![br(400, 100)], 5, vec![0, 1, 2]),
                (donors5.clone(), vec![br(100, 0)], 5, vec![0, 1, 2, 3, 4]),
                (donors5, vec![br(150, 0)], 5, vec![3, 4]),
            ];
        for (donors, holes, count, expected) in boundary_cases {
            assert_eq!(select_probe_parts(&donors, &holes, count), expected);
        }

        // 10. StreamRepairSelectProbePartsIrregularGeometryTest
        let end = i64::MAX;
        let start = i64::MIN;
        let irregular_cases: Vec<(Vec<NzbgetRsByteRange>, Vec<NzbgetRsByteRange>, Vec<i32>)> = vec![
            (
                vec![
                    br(300, 100),
                    br(0, 100),
                    br(100, 100),
                    br(400, 100),
                    br(200, 100),
                ],
                vec![br(150, 10)],
                vec![0, 4],
            ),
            (
                vec![
                    br(0, 300),
                    br(100, 20),
                    br(500, 100),
                    br(600, 100),
                    br(700, 100),
                ],
                vec![br(250, 10)],
                vec![2, 3, 4],
            ),
            (
                vec![
                    br(0, 100),
                    br(100, 100),
                    br(200, 100),
                    br(300, 100),
                    br(400, 100),
                ],
                vec![br(140, 60), br(170, 60)],
                vec![4],
            ),
            (
                vec![
                    br(0, 100),
                    br(100, 100),
                    br(200, 100),
                    br(300, 100),
                    br(400, 100),
                    br(500, 100),
                    br(600, 100),
                    br(700, 100),
                ],
                vec![br(650, 10), br(150, 10)],
                vec![3, 4],
            ),
            (
                vec![
                    br(0, 100),
                    br(150, 0),
                    br(200, 100),
                    br(300, 100),
                    br(400, 100),
                ],
                vec![br(100, 100)],
                vec![3, 4],
            ),
            (
                vec![
                    br(0, 100),
                    br(100, 0),
                    br(200, 100),
                    br(300, 100),
                    br(400, 100),
                ],
                vec![br(100, 100)],
                vec![0, 1, 2, 3, 4],
            ),
            (
                vec![br(0, 300), br(300, 100), br(400, 100), br(500, 100)],
                vec![br(200, -100)],
                vec![2, 3],
            ),
            (
                vec![
                    br(end - 500, 100),
                    br(end - 400, 100),
                    br(end - 300, 100),
                    br(end - 200, 100),
                    br(end - 100, 100),
                ],
                vec![br(end - 300, 100)],
                vec![0, 4],
            ),
            (vec![br(end, 1), br(end, 1)], vec![], vec![0, 1]),
            (vec![br(end, 1), br(end, 1)], vec![br(0, 1)], vec![0, 1]),
            (vec![br(0, 1), br(end - 10, 20)], vec![br(0, end)], vec![]),
            (
                vec![br(start, -1), br(0, 100)],
                vec![br(start, 0)],
                vec![0, 1],
            ),
            (
                vec![br(300, 1), br(0, 1), br(end - 10, 20)],
                vec![br(0, end)],
                vec![],
            ),
            (
                vec![br(0, 100), br(50, 100), br(end - 10, 20)],
                vec![br(0, end)],
                vec![],
            ),
            (
                vec![br(0, 0), br(100, 1), br(end - 10, 20)],
                vec![br(0, end)],
                vec![],
            ),
            (
                vec![br(end - 1, 1), br(end, 0)],
                vec![br(end - 2, 1)],
                vec![0, 1],
            ),
            (
                vec![br(start + 1, -1), br(0, 100)],
                vec![br(start, 0)],
                vec![0, 1],
            ),
            (vec![br(start, 1), br(start, -1)], vec![br(start, 1)], vec![]),
        ];
        for (donors, holes, expected) in irregular_cases {
            assert_eq!(
                select_probe_parts(&donors, &holes, donors.len() as i32),
                expected
            );
        }

        // 11. StreamRepairSelectProbePartsDifferentialTest
        let mut rng_state: u64 = 0x51ec7;
        let mut next_u32 = || -> u32 {
            rng_state = rng_state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (rng_state >> 32) as u32
        };
        for iteration in 0..2000 {
            let donor_count = (next_u32() % 80) as usize;
            let hole_count = (next_u32() % 25) as usize;
            let mut donors = Vec::with_capacity(donor_count);
            let mut holes = Vec::with_capacity(hole_count);
            for i in 0..donor_count {
                donors.push(if iteration % 2 == 0 {
                    br((i as i64) * 100, 100)
                } else {
                    br(
                        (next_u32() % 1000) as i64 - 50,
                        (next_u32() % 120) as i64 - 10,
                    )
                });
            }
            for i in 0..hole_count {
                holes.push(if iteration % 3 == 0 {
                    br((i as i64) * 250 + 50, 50)
                } else {
                    br(
                        (next_u32() % 1000) as i64 - 50,
                        (next_u32() % 120) as i64 - 10,
                    )
                });
            }
            for probe_count in [
                0,
                -1,
                1,
                2,
                donor_count as i32,
                (donor_count as i32) + 1,
            ] {
                let expected = reference_probe_parts(&donors, &holes, probe_count);
                let actual = select_probe_parts(&donors, &holes, probe_count);
                assert_eq!(actual, expected);
            }
        }

        // 12. StreamRepairSubtractCoveredTest
        let mut sub_holes = vec![br(300, 200), br(800, 200)];
        sub_holes = subtract_covered(&sub_holes, br(350, 100));
        assert_eq!(sub_holes, vec![br(300, 50), br(450, 50), br(800, 200)]);
        sub_holes = subtract_covered(&sub_holes, br(800, 200));
        assert_eq!(sub_holes, vec![br(300, 50), br(450, 50)]);
        sub_holes = subtract_covered(&sub_holes, br(600, 100));
        assert_eq!(sub_holes, vec![br(300, 50), br(450, 50)]);

        // 13. StreamRepairBuildRepairJobTest & 14. StreamRepairWholeFileJobTest & 20. StreamRepairDoesNotCaptureParFilesTest
        assert_eq!(
            eval_build_repair_job(
                2, 2, false, false, b"movie.mkv", true, 0, 0, 3, false, false, false, false, 2,
                1000, 1000
            ),
            2
        );
        assert_eq!(
            eval_build_repair_job(
                2, 2, false, false, b"release.r01", true, 0, 0, 3, false, false, false, false, 1,
                1000, 1000
            ),
            2
        );
        // Post-process active -> 0
        assert_eq!(
            eval_build_repair_job(
                2, 2, false, false, b"late.par2", true, 0, 0, 3, false, false, true, false, 1,
                1000, 1000
            ),
            0
        );
        // Below "stream" mode -> 0
        assert_eq!(
            eval_build_repair_job(
                0, 2, false, false, b"movie.mkv", true, 0, 0, 3, false, false, false, false, 1,
                1000, 1000
            ),
            0
        );
        // Whole-file missing -> 1
        assert_eq!(
            eval_build_repair_job(
                2, 2, false, false, b"ZGWRiqtR4jt2nswwN.part03.rar", true, 0, 0, 3, false, false,
                false, false, 0, 2_400_000, 0
            ),
            1
        );
        // Sizeless whole file -> 0
        assert_eq!(
            eval_build_repair_job(
                2, 2, false, false, b"x.r00", true, 0, 0, 3, false, false, false, false, 0, 0, 0
            ),
            0
        );
        // PAR files (by flag or by confirmed/disk .par2 name) -> 0
        assert_eq!(
            eval_build_repair_job(
                2, 2, false, true, b"obfuscated.bin", true, 0, 0, 3, false, false, false, false, 1,
                1000, 1000
            ),
            0
        );
        assert_eq!(
            eval_build_repair_job(
                2, 2, false, false, b"release.PaR2", true, 0, 0, 3, false, false, false, false, 1,
                1000, 1000
            ),
            0
        );
        assert_eq!(
            eval_build_repair_job(
                2, 2, false, false, b"release.vol01+02.PAR2", true, 0, 0, 3, false, false, false,
                false, 1, 1000, 1000
            ),
            0
        );

        // 15. StreamRepairDecodedSizePlausibleTest
        assert!(decoded_size_plausible(1_000_000, 1_020_000));
        assert!(decoded_size_plausible(1_020_000, 1_020_000));
        assert!(!decoded_size_plausible(1_020_001, 1_020_000));
        assert!(!decoded_size_plausible(800_000, 1_020_000));
        assert!(!decoded_size_plausible(0, 1_020_000));
        assert!(!decoded_size_plausible(1000, 0));

        // 16. StreamRepairSelectWholeFileDonorTest
        let flat = vec![768_000i32; 12];
        let twin = vec![
            770_318, 768_951, 771_402, 769_037, 770_660, 769_845, 768_590, 771_113, 769_722,
            771_000, 769_500, 398_760,
        ];
        let other = vec![
            769_211, 770_840, 768_392, 771_005, 769_930, 768_777, 770_123, 769_504, 771_288,
            770_100, 769_400, 398_760,
        ];
        let shifted = |sizes: &[i32], shift: i32| -> Vec<i32> {
            sizes.iter().map(|&s| s + shift).collect()
        };
        let sum_i64 = |sizes: &[i32]| -> i64 { sizes.iter().map(|&s| s as i64).sum() };

        {
            let target_hash = crate::dupefallback::article_size_steps_hash(&twin);
            assert_ne!(target_hash, 0);
            let other_shifted = shifted(&other, 27);
            let twin_shifted = shifted(&twin, 27);
            let donor_names: &[&[u8]] = &[b"aaa.bin", b"ccc.bin"];
            let donor_sizes = [sum_i64(&other_shifted), sum_i64(&twin_shifted)];
            let donor_hashes = [
                crate::dupefallback::article_size_steps_hash(&other_shifted),
                crate::dupefallback::article_size_steps_hash(&twin_shifted),
            ];
            let idx = select_whole_file_donor(
                b"Q8aZ1kLmN0pR",
                sum_i64(&twin),
                12,
                target_hash,
                b"",
                donor_names,
                &donor_sizes,
                &[0, 0],
                &[1, 1],
                &[0, 0],
                &donor_hashes,
            );
            assert_eq!(idx, 1);
            // Claimed member is skipped
            assert_eq!(
                select_whole_file_donor(
                    b"Q8aZ1kLmN0pR",
                    sum_i64(&twin),
                    12,
                    target_hash,
                    b"",
                    donor_names,
                    &donor_sizes,
                    &[0, 0],
                    &[1, 1],
                    &[0, 1],
                    &donor_hashes,
                ),
                -1
            );
        }
        {
            let flat_sum = sum_i64(&flat);
            let donor_names: &[&[u8]] = &[
                b"LN2ttWTMLC4N1DkTm.part02.rar",
                b"LN2ttWTMLC4N1DkTm.part03.rar",
                b"LN2ttWTMLC4N1DkTm.part04.rar",
                b"rel.vol00+08.par2",
            ];
            let donor_sizes = [flat_sum; 4];
            let donor_is_par = [0, 0, 0, 1];
            let donor_has_art = [1, 1, 1, 1];
            let donor_claimed = [0, 0, 0, 0];
            let donor_hashes = [0u64; 4];

            assert_eq!(
                select_whole_file_donor(
                    b"x8Tdn3e3iFf1MaWLu.part03.rar",
                    flat_sum,
                    12,
                    0,
                    b"",
                    donor_names,
                    &donor_sizes,
                    &donor_is_par,
                    &donor_has_art,
                    &donor_claimed,
                    &donor_hashes,
                ),
                1
            );
            assert_eq!(
                select_whole_file_donor(
                    b"ln2ttwtmlc4n1dktm.PART04.RAR",
                    flat_sum,
                    12,
                    0,
                    b"",
                    donor_names,
                    &donor_sizes,
                    &donor_is_par,
                    &donor_has_art,
                    &donor_claimed,
                    &donor_hashes,
                ),
                2
            );
            assert_eq!(
                select_whole_file_donor(
                    b"x8Tdn3e3iFf1MaWLu",
                    flat_sum,
                    12,
                    0,
                    b"",
                    donor_names,
                    &donor_sizes,
                    &donor_is_par,
                    &donor_has_art,
                    &donor_claimed,
                    &donor_hashes,
                ),
                -1
            );
            assert_eq!(
                select_whole_file_donor(
                    b"x8Tdn3e3iFf1MaWLu.part03.rar",
                    flat_sum,
                    20,
                    0,
                    b"",
                    donor_names,
                    &donor_sizes,
                    &donor_is_par,
                    &donor_has_art,
                    &donor_claimed,
                    &donor_hashes,
                ),
                1
            );
            assert_eq!(
                select_whole_file_donor(
                    b"x8Tdn3e3iFf1MaWLu.part03.rar",
                    flat_sum * 2,
                    12,
                    0,
                    b"",
                    donor_names,
                    &donor_sizes,
                    &donor_is_par,
                    &donor_has_art,
                    &donor_claimed,
                    &donor_hashes,
                ),
                -1
            );
            assert_eq!(
                select_whole_file_donor(
                    b"rel.vol00+08.par2",
                    flat_sum,
                    12,
                    0,
                    b"",
                    donor_names,
                    &donor_sizes,
                    &donor_is_par,
                    &donor_has_art,
                    &donor_claimed,
                    &donor_hashes,
                ),
                -1
            );
        }

        // 17. StreamRepairVolumeSetKeyTest
        assert_eq!(volume_set_key(b"Rel.part03.rar"), b"rel.part#.rar");
        assert_eq!(volume_set_key(b"REL.part1.RAR"), b"rel.part#.rar");
        assert_eq!(volume_set_key(b"Rel.r07"), b"rel.r#");
        assert_eq!(volume_set_key(b"Rel.rar"), b"rel.r#");
        assert_eq!(volume_set_key(b"Rel.s00"), b"rel.r#");
        assert_eq!(volume_set_key(b"Rel.t12"), b"rel.r#");
        assert_eq!(volume_set_key(b"Rel.7z.002"), b"rel.7z.#");
        assert_eq!(volume_set_key(b"movie.mkv.001"), b"movie.mkv.#");
        assert_eq!(volume_set_key(b"Rel.z01"), b"rel.z#");
        assert_eq!(volume_set_key(b"Rel.zip"), b"rel.z#");
        assert_eq!(
            volume_set_key(b"dir/x8Tdn3e3iFf1MaWLu.part12.rar"),
            b"x8tdn3e3iff1mawlu.part#.rar"
        );
        assert!(volume_set_key(b"Rel.nfo").is_empty());
        assert!(volume_set_key(b"Rel.sample.mkv").is_empty());
        assert!(volume_set_key(b"movie.mkv").is_empty());
        assert!(volume_set_key(b"Rel.srr").is_empty());
        assert!(volume_set_key(b"track.mp3").is_empty());
        assert!(volume_set_key(b"Rel.s001").is_empty());
        assert!(volume_set_key(b"Rel.part2").is_empty());
        assert!(volume_set_key(b"").is_empty());
        assert_ne!(
            volume_set_key(b"Rel.nfo"),
            volume_set_key(b"Rel.part01.rar")
        );

        // 18. StreamRepairVolumeKeyTest
        assert_eq!(volume_key(b"Rel.part003.rar"), b"part3.rar");
        assert_eq!(volume_key(b"Other.part03.rar"), b"part3.rar");
        assert_eq!(volume_key(b"X.r007"), b"r7");
        assert_eq!(volume_key(b"Y.R07"), b"r7");
        assert_eq!(volume_key(b"x.7z.001"), b"7z.1");
        assert_eq!(volume_key(b"x.r00"), b"r0");
        assert_eq!(volume_key(b"Rel.part10.rar"), b"part10.rar");
        assert_eq!(volume_key(b"Movie.x264.mkv"), b"x264.mkv");
        assert_ne!(
            volume_key(b"Rel.part10.rar"),
            volume_key(b"Rel.part01.rar")
        );
        assert!(volume_key(b"").is_empty());

        // 19. StreamRepairSuffixKeyTest
        assert_eq!(suffix_key(b"Rel.part03.rar"), b"part03.rar");
        assert_eq!(suffix_key(b"REL.R00"), b"r00");
        assert_eq!(suffix_key(b"movie.mkv"), b"mkv");
        assert_eq!(suffix_key(b"a.vol07+08.par2"), b"vol07+08.par2");
        assert_eq!(suffix_key(b"archive.7z.001"), b"7z.001");
        assert!(suffix_key(b"noextension").is_empty());
        assert!(suffix_key(b"").is_empty());

        // 20. StreamRepairRefusesParTargetCandidatesTest & StreamRepairExcludesParDonorCandidatesTest
        assert!(select_donor_candidates(
            b"release.PAR2",
            980,
            0,
            2,
            3,
            0,
            &[b"release.PAR2", b"renamed.bin"],
            &[1000, 1000],
            &[0, 0],
            &[1, 1],
            &[0, 0],
            &[0, 0],
        )
        .is_empty());
        assert_eq!(
            select_donor_candidates(
                b"release.r01",
                980,
                0,
                3,
                3,
                0,
                &[b"release.r01", b"release.PaR2", b"renamed.bin"],
                &[1000, 1000, 1000],
                &[1, 0, 0],
                &[1, 1, 1],
                &[0, 0, 0],
                &[0, 0, 0],
            ),
            vec![2]
        );

        // 21. StreamRepairStepFingerprintCandidateTest, StreamRepairSelectDonorCandidatesTest, StreamRepairObfuscatedSiblingCandidatesTest
        {
            let twin10 = [
                770_318, 768_951, 771_402, 769_037, 770_660, 769_845, 768_590, 771_113, 769_722,
                398_760,
            ];
            let other10 = [
                769_211, 770_840, 768_392, 771_005, 769_930, 768_777, 770_123, 769_504, 771_288,
                398_760,
            ];
            let third10 = [
                768_845, 771_230, 769_918, 770_402, 768_611, 771_077, 769_283, 770_936, 768_458,
                398_760,
            ];
            let target_hash = crate::dupefallback::article_size_steps_hash(&twin10);
            assert_ne!(target_hash, 0);
            let s_other = shifted(&other10, 27);
            let s_twin = shifted(&twin10, 27);
            let s_third = shifted(&third10, 27);
            let cands = select_donor_candidates(
                b"Q8aZ1kLmN0pR",
                7_300_000,
                -1,
                0,
                3,
                target_hash,
                &[b"aaa.bin", b"ccc.bin", b"bbb.bin"],
                &[sum_i64(&s_other), sum_i64(&s_twin), sum_i64(&s_third)],
                &[0, 0, 0],
                &[1, 1, 1],
                &[0, 0, 0],
                &[
                    crate::dupefallback::article_size_steps_hash(&s_other),
                    crate::dupefallback::article_size_steps_hash(&s_twin),
                    crate::dupefallback::article_size_steps_hash(&s_third),
                ],
            );
            assert_eq!(cands[0], 1);
            assert_eq!(
                crate::dupefallback::article_size_steps_hash(&vec![750_000i32; 10]),
                0
            );
        }
        {
            let repost_names: &[&[u8]] = &[
                b"rel.part01.rar",
                b"rel.part02.rar",
                b"rel.part03.rar",
                b"info.nfo",
            ];
            let repost_sizes = [1000i64, 1000, 1000, 50];
            let by_name = select_donor_candidates(
                b"rel.part02.rar",
                980,
                -1,
                0,
                3,
                0,
                repost_names,
                &repost_sizes,
                &[0; 4],
                &[1; 4],
                &[0; 4],
                &[0; 4],
            );
            assert_eq!(by_name[0], 1);
            assert!(!by_name.contains(&3));
            assert!(by_name.len() <= 3);

            let by_suffix = select_donor_candidates(
                b"other.part03.rar",
                980,
                -1,
                0,
                3,
                0,
                repost_names,
                &repost_sizes,
                &[0; 4],
                &[1; 4],
                &[0; 4],
                &[0; 4],
            );
            assert_eq!(by_suffix[0], 2);

            let obf_names: &[&[u8]] = &[b"ccc.bin", b"aaa.bin", b"bbb.bin"];
            let obf_sizes = [1000i64; 3];
            let by_rank = select_donor_candidates(
                b"zz.dat",
                980,
                1,
                3,
                3,
                0,
                obf_names,
                &obf_sizes,
                &[0; 3],
                &[1; 3],
                &[0; 3],
                &[0; 3],
            );
            assert_eq!(by_rank[0], 2); // "bbb.bin" is rank 1 in sorted ["aaa.bin", "bbb.bin", "ccc.bin"]

            let bad_window = select_donor_candidates(
                b"zz.dat",
                980,
                1,
                4,
                3,
                0,
                obf_names,
                &obf_sizes,
                &[0; 3],
                &[1; 3],
                &[0; 3],
                &[0; 3],
            );
            assert_eq!(bad_window.len(), 3);

            let bad_rank = select_donor_candidates(
                b"zz.dat",
                980,
                99,
                3,
                3,
                0,
                obf_names,
                &obf_sizes,
                &[0; 3],
                &[1; 3],
                &[0; 3],
                &[0; 3],
            );
            assert_eq!(bad_rank.len(), 3);

            let bare_ext = select_donor_candidates(
                b"zzz.mkv",
                980,
                -1,
                0,
                3,
                0,
                &[b"aaa.mkv", b"bbb.mkv"],
                &[1000, 980],
                &[0, 0],
                &[1, 1],
                &[0, 0],
                &[0, 0],
            );
            assert_eq!(bare_ext, vec![1, 0]);

            let digit_ext = select_donor_candidates(
                b"zzz.mp4",
                980,
                -1,
                0,
                3,
                0,
                &[b"ep1.mp4", b"ep2.mp4"],
                &[1000, 980],
                &[0, 0],
                &[1, 1],
                &[0, 0],
                &[0, 0],
            );
            assert_eq!(digit_ext, vec![1, 0]);
        }
        {
            let obf16_owned: Vec<Vec<u8>> = (0..16u8)
                .map(|i| format!("obf{}{}", 100 + i, (b'q' - i) as char).into_bytes())
                .collect();
            let obf16_refs: Vec<&[u8]> = obf16_owned.iter().map(Vec::as_slice).collect();
            let sizes16 = [1000i64; 16];
            let all16 = select_donor_candidates(
                b"zz.dat",
                980,
                -1,
                0,
                64,
                0,
                &obf16_refs,
                &sizes16,
                &[0; 16],
                &[1; 16],
                &[0; 16],
                &[0; 16],
            );
            assert_eq!(all16.len(), 16);

            let mut claimed16 = [0u8; 16];
            claimed16[all16[0] as usize] = 1;
            claimed16[all16[5] as usize] = 1;
            let rest14 = select_donor_candidates(
                b"zz.dat",
                980,
                -1,
                0,
                64,
                0,
                &obf16_refs,
                &sizes16,
                &[0; 16],
                &[1; 16],
                &claimed16,
                &[0; 16],
            );
            assert_eq!(rest14.len(), 14);
            assert!(!rest14.contains(&all16[0]));
            assert!(!rest14.contains(&all16[5]));

            let named_idx = all16[3] as usize;
            let mut claimed_named = [0u8; 16];
            claimed_named[named_idx] = 1;
            let by_name_unclaimed = select_donor_candidates(
                obf16_refs[named_idx],
                980,
                -1,
                0,
                64,
                0,
                &obf16_refs,
                &sizes16,
                &[0; 16],
                &[1; 16],
                &claimed_named,
                &[0; 16],
            );
            assert!(!by_name_unclaimed.is_empty());
            assert_ne!(by_name_unclaimed[0], named_idx as i32);
        }

        // 22. StreamRepairRequiredCompareFloorTest
        let cr = |offset: i64, size: i64| crate::streamcompare::Range { offset, size };
        assert_eq!(
            crate::streamcompare::required_compare_floor(
                1_500_000,
                &[cr(0, 1000)],
                &[cr(0, 500_000), cr(500_000, 500_000), cr(1_000_000, 500_000)],
                &[1, 2]
            ),
            16 * 1024
        );
        assert_eq!(
            crate::streamcompare::required_compare_floor(
                80_000,
                &[cr(0, 70_000)],
                &[cr(0, 80_000)],
                &[0]
            ),
            10_000
        );
        assert_eq!(
            crate::streamcompare::required_compare_floor(
                30_000,
                &[cr(2000, 8000), cr(12_000, 8000), cr(22_000, 8000)],
                &[cr(0, 10_000), cr(10_000, 10_000), cr(20_000, 10_000)],
                &[0]
            ),
            2000
        );
        assert_eq!(
            crate::streamcompare::required_compare_floor(
                1_000_000,
                &[cr(60, 999_940)],
                &[cr(0, 500_000), cr(500_000, 500_000)],
                &[0]
            ),
            64
        );

        // 23. SelectExtractedInnerTest
        {
            use std::os::unix::ffi::OsStrExt;
            let unique_id = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let temp_dir =
                std::env::temp_dir().join(format!("nzbget_select_extracted_{unique_id}"));
            let outside_dir =
                std::env::temp_dir().join(format!("nzbget_select_outside_{unique_id}"));
            let _ = std::fs::remove_dir_all(&temp_dir);
            let _ = std::fs::remove_dir_all(&outside_dir);
            std::fs::create_dir_all(&temp_dir).unwrap();
            std::fs::create_dir_all(&outside_dir).unwrap();

            let name_match = temp_dir.join("movie.mkv");
            let other_same_size = temp_dir.join("extracted.bin");
            let wrong_size = temp_dir.join("readme.txt");
            std::fs::write(&name_match, vec![b'x'; 1000]).unwrap();
            std::fs::write(&other_same_size, vec![b'x'; 1000]).unwrap();
            std::fs::write(&wrong_size, vec![b'x'; 50]).unwrap();

            let dir_bytes = temp_dir.as_os_str().as_bytes();
            let name_match_bytes = name_match.as_os_str().as_bytes().to_vec();
            let other_same_bytes = other_same_size.as_os_str().as_bytes().to_vec();

            assert_eq!(
                select_extracted_inner(dir_bytes, 1000, b"MOVIE.MKV"),
                (name_match_bytes.clone(), false)
            );
            let expected_first = name_match_bytes.min(other_same_bytes);
            assert_eq!(
                select_extracted_inner(dir_bytes, 1000, b"nomatch.mkv"),
                (expected_first, false)
            );
            assert_eq!(
                select_extracted_inner(dir_bytes, 999_999, b"movie.mkv"),
                (Vec::new(), false)
            );

            let outside_target = outside_dir.join("secret.mkv");
            std::fs::write(&outside_target, vec![b'x'; 1000]).unwrap();
            if std::os::unix::fs::symlink(&outside_target, temp_dir.join("escape.mkv")).is_ok() {
                let (selected, found_symlink) =
                    select_extracted_inner(dir_bytes, 1000, b"escape.mkv");
                assert!(selected.is_empty());
                assert!(found_symlink);
            }

            let _ = std::fs::remove_dir_all(&outside_dir);
            let _ = std::fs::remove_dir_all(&temp_dir);
        }

        // 24. DonorPostingIdentityPreservesRepairAlternativesTest
        {
            let original = build_donor_key(
                b"",
                &[b"movie.7z"],
                &[1000],
                &[0],
                &[0],
                &[],
                &[2],
                &[1, 2],
                &[500, 500],
                &[b"donor-1@example.com", b"donor-2@example.com"],
            );
            let copy = build_donor_key(
                b"",
                &[b"movie.7z"],
                &[1000],
                &[0],
                &[0],
                &[],
                &[2],
                &[1, 2],
                &[500, 500],
                &[b"donor-1@example.com", b"donor-2@example.com"],
            );
            assert_eq!(original, copy);

            let with_pw = build_donor_key(
                b"corrected-password",
                &[b"movie.7z"],
                &[1000],
                &[0],
                &[0],
                &[],
                &[2],
                &[1, 2],
                &[500, 500],
                &[b"donor-1@example.com", b"donor-2@example.com"],
            );
            assert_ne!(original, with_pw);

            let with_grp = build_donor_key(
                b"",
                &[b"movie.7z"],
                &[1000],
                &[0],
                &[1],
                &[b"alt.binaries.other"],
                &[2],
                &[1, 2],
                &[500, 500],
                &[b"donor-1@example.com", b"donor-2@example.com"],
            );
            assert_ne!(original, with_grp);

            let with_new_msgid = build_donor_key(
                b"",
                &[b"movie.7z"],
                &[1000],
                &[0],
                &[0],
                &[],
                &[2],
                &[1, 2],
                &[500, 500],
                &[b"replacement@example.com", b"donor-2@example.com"],
            );
            assert_ne!(original, with_new_msgid);
        }

        // 25. ExceedsDecompressCapTest
        assert!(!exceeds_decompress_cap(
            MAX_DECOMPRESS_BYTES - 100,
            100,
            0,
            0
        ));
        assert!(exceeds_decompress_cap(MAX_DECOMPRESS_BYTES - 99, 100, 0, 0));
        assert!(!exceeds_decompress_cap(0, 0, MAX_DECOMPRESS_BYTES - 50, 50));
        assert!(exceeds_decompress_cap(0, 0, MAX_DECOMPRESS_BYTES - 49, 50));
        assert!(exceeds_decompress_cap(MAX_DECOMPRESS_BYTES, 1, 0, 0));
        assert!(exceeds_decompress_cap(0, 0, MAX_DECOMPRESS_BYTES, 1));
        assert!(!exceeds_decompress_cap(0, 1024, 0, 1024));

        // 26. ParDamageBlocksTouchedTest
        let block = 1000u64;
        assert_eq!(crate::dupefallback::blocks_touched(&[br(100, 200)], block), 1);
        assert_eq!(crate::dupefallback::blocks_touched(&[br(900, 200)], block), 2);
        assert_eq!(
            crate::dupefallback::blocks_touched(
                &[br(5300, 10), br(100, 10), br(150, 10), br(5900, 200)],
                block
            ),
            3
        );
        assert_eq!(crate::dupefallback::blocks_touched(&[br(0, 1000)], block), 1);
        assert_eq!(crate::dupefallback::blocks_touched(&[br(0, 1001)], block), 2);
        assert_eq!(
            crate::dupefallback::blocks_touched(&[br(0, 10_000), br(3500, 10)], block),
            10
        );
        assert_eq!(crate::dupefallback::blocks_touched(&[], block), 0);
        assert_eq!(crate::dupefallback::blocks_touched(&[br(0, 10)], 0), 0);

        // 27. StreamRepairRustBoundaryTest
        {
            assert!(is_stream_eligible(b"movie.mkv"));
            assert!(!is_stream_eligible(b"movie.par2"));
            assert!(decoded_size_plausible(980, 1000));
            assert!(!exceeds_decompress_cap(0, 1024, 0, 1024));

            assert_eq!(volume_set_key(b"Show.part01.rar"), b"show.part#.rar");
            assert_eq!(volume_key(b"Show.part01.rar"), b"part1.rar");
            assert_eq!(suffix_key(b"Show.part01.rar"), b"part01.rar");

            let r = [br(0, 100), br(200, 100)];
            assert_eq!(total_size(&r), 200);
            assert!(intersects_any(br(50, 20), &r));
            assert!(!intersects_any(br(110, 50), &r));

            let out_holes = compute_holes(&r, 300);
            assert_eq!(out_holes, vec![br(100, 100)]);

            let adv_sizes = [150i32, 150];
            assert_eq!(estimate_donor_ranges(&adv_sizes, 300).len(), 2);

            assert!(!select_probe_parts(&r, &out_holes, 2).is_empty());
            assert_eq!(subtract_covered(&r, br(0, 100)).len(), 1);
            assert!(!build_probe_windows(300, &out_holes, &out_holes, 2, 16).is_empty());

            let five_holes = [br(0, 100), br(100_000, 100), br(200_000, 100)];
            assert_eq!(
                build_probe_windows(300_000, &five_holes, &five_holes, 2, 16).len(),
                5
            );

            let runs = [NzbgetRsContentRun {
                inner_offset: 0,
                member_index: 0,
                member_offset: 0,
                size: 1_000_000,
            }];
            assert_eq!(plain_compare_floor(&runs, &out_holes, &r[..1]), 100);
            assert_eq!(encrypted_chain_start(32, 64, 16, &out_holes), 32);

            let varied = [1u8, 2, 3, 4];
            assert!(buffer_has_varied_data(&varied));

            let names: [&[u8]; 2] = [b"b.rar", b"a.rar"];
            let sizes = [1000i64, 1000];
            let (rank, window) = compute_positional_rank(&names, &sizes, b"b.rar", 980);
            assert_eq!(rank, 1);
            assert_eq!(window, 2);
        }

        // 22. probe_whole_file_donor_size and materialize_donor_set
        {
            struct MockProbeClient {
                articles: Vec<FetchedDonorArticle>,
                current: Option<FetchedDonorArticle>,
                requested: Vec<i32>,
            }
            impl DonorBatchClient for MockProbeClient {
                fn is_stopped(&self) -> bool {
                    false
                }
                fn begin_batch(&mut self, part_indices: &[i32]) {
                    self.requested.extend_from_slice(part_indices);
                    self.current = part_indices
                        .first()
                        .and_then(|&idx| self.articles.get(idx as usize).cloned());
                }
                fn next_article(&mut self) -> Option<FetchedDonorArticle> {
                    self.current.take()
                }
                fn cancel_remaining(&mut self) {}
            }

            let mut probe_client = MockProbeClient {
                articles: vec![
                    FetchedDonorArticle {
                        success: false,
                        offset: 0,
                        file_size: 0,
                        data: Vec::new(),
                    },
                    FetchedDonorArticle {
                        success: true,
                        offset: 500,
                        file_size: 970,
                        data: vec![1; 500],
                    },
                ],
                current: None,
                requested: Vec::new(),
            };
            assert_eq!(
                probe_whole_file_donor_size(2, 1000, &mut probe_client),
                Some(970)
            );
            assert_eq!(probe_client.requested, vec![0, 1]);

            struct MockMatClient {
                member_articles: Vec<Vec<FetchedDonorArticle>>,
                queue: std::collections::VecDeque<FetchedDonorArticle>,
                written: Vec<(i32, i64, Vec<u8>)>,
                cur_member: i32,
                messages: Vec<String>,
                progress: Vec<(String, i32)>,
            }
            impl DonorMaterializeClient for MockMatClient {
                fn is_stopped(&mut self) -> bool {
                    false
                }
                fn open_member(&mut self, member_index: i32) -> bool {
                    self.cur_member = member_index;
                    true
                }
                fn write_member(&mut self, offset: i64, data: &[u8]) -> bool {
                    self.written.push((self.cur_member, offset, data.to_vec()));
                    true
                }
                fn close_member(&mut self) {}
                fn begin_member_batch(&mut self, member_index: i32) {
                    self.queue = self.member_articles[member_index as usize]
                        .iter()
                        .cloned()
                        .collect();
                }
                fn next_article(&mut self) -> Option<FetchedDonorArticle> {
                    self.queue.pop_front()
                }
                fn cancel_remaining(&mut self) {
                    self.queue.clear();
                }
                fn update_progress(&mut self, label: &[u8], stage_progress: i32) {
                    self.progress.push((
                        String::from_utf8_lossy(label).into_owned(),
                        stage_progress,
                    ));
                }
                fn emit_message_bytes(&mut self, _kind: TargetMessageKind, text: &[u8]) {
                    self.messages
                        .push(String::from_utf8_lossy(text).into_owned());
                }
            }

            let mut mat_client = MockMatClient {
                member_articles: vec![vec![
                    FetchedDonorArticle {
                        success: true,
                        offset: 0,
                        file_size: 8,
                        data: vec![1, 2, 3, 4],
                    },
                    FetchedDonorArticle {
                        success: true,
                        offset: 4,
                        file_size: 8,
                        data: vec![5, 6, 7, 8],
                    },
                ]],
                queue: std::collections::VecDeque::new(),
                written: Vec::new(),
                cur_member: -1,
                messages: Vec::new(),
                progress: Vec::new(),
            };
            let mut total_bytes = 0i64;
            assert!(materialize_donor_set(
                &[0],
                &[b"Show.part01.rar"],
                &[2],
                b"Dupe.Show",
                &mut total_bytes,
                &mut mat_client,
            ));
            assert_eq!(total_bytes, 8);
            assert_eq!(mat_client.written.len(), 2);
            assert_eq!(mat_client.progress.last().unwrap().1, 1000);

            // Incomplete hole after all articles fails materialization and emits info message
            let mut incomplete_client = MockMatClient {
                member_articles: vec![vec![FetchedDonorArticle {
                    success: true,
                    offset: 0,
                    file_size: 8,
                    data: vec![1, 2, 3, 4],
                }]],
                queue: std::collections::VecDeque::new(),
                written: Vec::new(),
                cur_member: -1,
                messages: Vec::new(),
                progress: Vec::new(),
            };
            let mut inc_bytes = 0i64;
            assert!(!materialize_donor_set(
                &[0],
                &[b"Show.part01.rar"],
                &[1],
                b"Dupe.Show",
                &mut inc_bytes,
                &mut incomplete_client,
            ));
            assert!(incomplete_client.messages[0]
                .contains("Show.part01.rar has missing ranges after all listed articles"));
        }
    }

    #[test]
    fn cross_pack_repair_session_store_and_decompress() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!(
            "rustyget_cross_pack_test_{}_{}",
            std::process::id(),
            unique
        ));
        let _ = std::fs::remove_dir_all(&temp_root);
        std::fs::create_dir_all(&temp_root).unwrap();

        // Create a 32 KiB inner payload with varied bytes
        let inner: Vec<u8> = (0..32768)
            .map(|i| ((i * 73 + 19) & 0xFF) as u8)
            .collect();

        // Write target split archive Show.mkv.001 (16 KiB with a 1 KiB hole at 4096..5120)
        // and Show.mkv.002 (16 KiB intact)
        let mut part1 = inner[..16384].to_vec();
        part1[4096..5120].fill(0);
        let part2 = inner[16384..].to_vec();
        std::fs::write(temp_root.join("Show.mkv.001"), &part1).unwrap();
        std::fs::write(temp_root.join("Show.mkv.002"), &part2).unwrap();

        let dest_bytes = crate::fsops::path_to_bytes(&temp_root).to_vec();
        let member_names: Vec<&[u8]> = vec![b"Show.mkv.001", b"Show.mkv.002"];
        let targets = vec![CrossPackTargetInput {
            filename: b"Show.mkv.001".to_vec(),
            decoded_file_size: 16384,
            patch_eligible: true,
            holes: vec![NzbgetRsByteRange {
                offset: 4096,
                size: 1024,
            }],
        }];

        let mut init_msgs = Vec::new();
        let mut emit_init = |kind: TargetMessageKind, msg: &[u8]| {
            init_msgs.push((kind, String::from_utf8_lossy(msg).into_owned()));
        };
        let mut session = CrossPackRepairSession::new(
            &dest_bytes,
            &member_names,
            &targets,
            None,
            true,
            None,
            &mut emit_init,
            None,
        )
        .expect("session should map split archive");
        assert!(session.has_inner_holes());

        struct MockDonorEnv {
            donor_bytes: Vec<u8>,
            recovered_bytes: i64,
            recovered_holes: i32,
            synced_holes: Vec<Vec<NzbgetRsByteRange>>,
            messages: Vec<String>,
        }
        impl CrossPackDonorEnv for MockDonorEnv {
            fn is_stopped(&mut self) -> bool {
                false
            }
            fn has_batch_fetcher(&self) -> bool {
                false
            }
            fn fetch_donor_part(
                &mut self,
                member_index: i32,
                part_index: i32,
            ) -> Option<FetchedDonorArticle> {
                if member_index != 0 || part_index != 0 {
                    return None;
                }
                Some(FetchedDonorArticle {
                    success: true,
                    offset: 0,
                    file_size: self.donor_bytes.len() as i64,
                    data: self.donor_bytes.clone(),
                })
            }
            fn begin_donor_prefetch(&mut self, _member_index: i32, _part_indices: &[i32]) {}
            fn next_donor_prefetch(&mut self) -> Option<FetchedDonorArticle> {
                None
            }
            fn cancel_donor_prefetch(&mut self) {}
            fn begin_materialize_member(&mut self, _member_index: i32) {}
            fn next_materialize_article(&mut self) -> Option<FetchedDonorArticle> {
                None
            }
            fn cancel_materialize_member(&mut self) {}
            fn set_progress(&mut self, _label: &[u8], _stage_progress: i32) {}
            fn set_materializing(&mut self, _delta: i32) {}
            fn add_recovered(&mut self, bytes: i64, holes: i32) {
                self.recovered_bytes += bytes;
                self.recovered_holes += holes;
            }
            fn sync_target_holes(&mut self, target_holes: &[Vec<NzbgetRsByteRange>]) {
                self.synced_holes = target_holes.to_vec();
            }
            fn emit_message_bytes(&mut self, _kind: TargetMessageKind, text: &[u8]) {
                self.messages
                    .push(String::from_utf8_lossy(text).into_owned());
            }
            fn emit_extractor_log(&mut self, _kind: i32, _text: &str) {}
        }

        // Donor is a bare file Show.mkv (different packaging from split target!)
        let mut env = MockDonorEnv {
            donor_bytes: inner.clone(),
            recovered_bytes: 0,
            recovered_holes: 0,
            synced_holes: Vec::new(),
            messages: Vec::new(),
        };
        let donor_files: Vec<&[u8]> = vec![b"Show.mkv"];
        let donor_sizes: Vec<i64> = vec![32768];
        let donor_articles: Vec<Vec<i32>> = vec![vec![32768]];

        session.process_donor(
            b"",
            b"Dupe.Bare",
            b"",
            &donor_files,
            &donor_sizes,
            &donor_articles,
            false,
            &mut env,
        );

        assert!(!session.has_inner_holes());
        assert_eq!(env.recovered_bytes, 1024);
        assert_eq!(env.synced_holes, vec![Vec::new()]);
        let repaired_part1 = std::fs::read(temp_root.join("Show.mkv.001")).unwrap();
        assert_eq!(repaired_part1, &inner[..16384]);

        let _ = std::fs::remove_dir_all(&temp_root);
    }
}
