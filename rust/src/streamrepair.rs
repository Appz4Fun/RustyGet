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
            let mut field_pos = 0usize;
            while field_pos + 4 <= extra_len {
                let field_id = get_le16(&extra[field_pos..field_pos + 2]);
                let field_size = get_le16(&extra[field_pos + 2..field_pos + 4]) as usize;
                if field_id == 0x0001 {
                    let mut val_pos = field_pos + 4;
                    let field_end = (field_pos + 4 + field_size).min(extra_len);
                    if uncomp_size == 0xffff_ffff && val_pos + 8 <= field_end {
                        uncomp_size = get_le64(&extra[val_pos..val_pos + 8]);
                        val_pos += 8;
                    }
                    if comp_size == 0xffff_ffff && val_pos + 8 <= field_end {
                        comp_size = get_le64(&extra[val_pos..val_pos + 8]);
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

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
