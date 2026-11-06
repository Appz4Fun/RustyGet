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
}
