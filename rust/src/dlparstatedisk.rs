// DownloadInfo, ParChecker & DiskState Hot-Path Kernels
// (`daemon/queue/DownloadInfo.cpp`, `daemon/postprocess/ParChecker.cpp`, `daemon/queue/DiskState.cpp`)

use std::os::raw::c_int;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NzbgetRsDpsCurrentStatsAcc {
    pub remaining_size: i64,
    pub current_success_size: i64,
    pub current_failed_size: i64,
    pub paused_size: i64,
    pub par_current_success_size: i64,
    pub par_current_failed_size: i64,
    pub current_success_articles: i32,
    pub current_failed_articles: i32,
    pub extra_priority: i32,
    pub paused_file_count: i32,
    pub remaining_par_count: i32,
    pub _pad: i32,
}

/// `NzbInfo::SetId` / `FileInfo::SetId` / `CompletedFile::CompletedFile` / `DupInfo::SetId`
/// (`DownloadInfo.cpp`:182, 1045, 1150, 1180):
/// Returns the updated `m_idMax` (`id > id_max ? id : id_max`).
pub fn dl_info_update_max_id(id: c_int, id_max: c_int) -> c_int {
    if id_max < id {
        id
    } else {
        id_max
    }
}

/// `NzbInfo::ResetGenId` / `FileInfo::ResetGenId` (`DownloadInfo.cpp`:188-199, 1051-1062):
/// Computes the new `(m_idGen, m_idMax)` pair given `max` flag and current `id_max`.
pub fn dl_info_reset_gen_id(max: c_int, id_max: c_int) -> (c_int, c_int) {
    if max != 0 {
        (id_max, id_max)
    } else {
        (0, 0)
    }
}

/// `NzbInfo::UpdateCurrentStats` per-file accumulation step (`DownloadInfo.cpp`:872-890):
/// Updates all 11 running counters in `acc` for one `FileInfo`.
#[allow(clippy::too_many_arguments)]
pub fn dl_info_update_current_stats_step(
    acc: &mut NzbgetRsDpsCurrentStatsAcc,
    file_remaining_size: i64,
    file_success_articles: i32,
    file_failed_articles: i32,
    file_success_size: i64,
    file_failed_size: i64,
    file_extra_priority: c_int,
    file_paused: c_int,
    file_par_file: c_int,
) {
    acc.remaining_size = acc.remaining_size.wrapping_add(file_remaining_size);
    acc.current_success_articles = acc
        .current_success_articles
        .wrapping_add(file_success_articles);
    acc.current_failed_articles = acc
        .current_failed_articles
        .wrapping_add(file_failed_articles);
    acc.current_success_size = acc.current_success_size.wrapping_add(file_success_size);
    acc.current_failed_size = acc.current_failed_size.wrapping_add(file_failed_size);
    acc.extra_priority = acc
        .extra_priority
        .wrapping_add(if file_extra_priority != 0 { 1 } else { 0 });

    if file_paused != 0 {
        acc.paused_file_count = acc.paused_file_count.wrapping_add(1);
        acc.paused_size = acc.paused_size.wrapping_add(file_remaining_size);
    }
    if file_par_file != 0 {
        acc.remaining_par_count = acc.remaining_par_count.wrapping_add(1);
        acc.par_current_success_size = acc
            .par_current_success_size
            .wrapping_add(file_success_size);
        acc.par_current_failed_size = acc
            .par_current_failed_size
            .wrapping_add(file_failed_size);
    }
}

/// `DownloadQueue::CalcRemainingSize` per-file accumulation step (`DownloadInfo.cpp`:1237-1244):
/// Accumulates `remaining_size` and `remaining_forced_size` when `!paused && !deleted`.
pub fn dl_queue_calc_remaining_size_step(
    paused: c_int,
    deleted: c_int,
    force_priority: c_int,
    file_remaining_size: i64,
    remaining_size: i64,
    remaining_forced_size: i64,
) -> (i64, i64) {
    if paused == 0 && deleted == 0 {
        let new_rem = remaining_size.wrapping_add(file_remaining_size);
        let new_forced = if force_priority != 0 {
            remaining_forced_size.wrapping_add(file_remaining_size)
        } else {
            remaining_forced_size
        };
        (new_rem, new_forced)
    } else {
        (remaining_size, remaining_forced_size)
    }
}

/// `Repairer::ScanDataFile` (`ParChecker.cpp`:200):
/// Returns `1` when quick verification is allowed (`!(stage == pt_verifying_repaired && par_full != 0)`).
pub fn parchecker_should_quick_verify(
    stage: c_int,
    pt_verifying_repaired: c_int,
    par_full: c_int,
) -> c_int {
    if stage == pt_verifying_repaired && par_full != 0 {
        0
    } else {
        1
    }
}

/// `ParChecker::RunParCheck` (`ParChecker.cpp`:536):
/// Computes `static_cast<int>(damagedfilecount + missingfilecount)`.
pub fn parchecker_calc_files_to_repair(damaged_count: u32, missing_count: u32) -> c_int {
    damaged_count.wrapping_add(missing_count) as c_int
}

/// `ParChecker::ProcessTwinSet` (`ParChecker.cpp`:927-930):
/// Returns `1` when `res == e_repair_not_possible` and `own_err_empty != 0` (replace with
/// `"not enough par-blocks, the twin's included"`), `2` when `res == e_repair_not_possible`
/// and `own_err_empty == 0` (append `"; the twin's didn't do either"`), or `0` otherwise.
pub fn parchecker_classify_twin_repair_err(
    res: c_int,
    e_repair_not_possible: c_int,
    own_err_empty: c_int,
) -> c_int {
    if res != e_repair_not_possible {
        0
    } else if own_err_empty != 0 {
        1
    } else {
        2
    }
}

/// `ParChecker::CheckEmptyFiles` (`ParChecker.cpp`:1526):
/// Returns updated `m_hasDamagedFiles` (`has_damaged | !ignore`).
pub fn parchecker_update_damaged_flag(has_damaged: c_int, ignore: c_int) -> c_int {
    if has_damaged != 0 || ignore == 0 {
        1
    } else {
        0
    }
}

/// `ParChecker::VerifyDataFile` (`ParChecker.cpp`:1709):
/// Returns `1` if the hash entry is invalid/mismatched/already-set
/// (`!has_hash_entry || !same_source_file || is_already_set`), meaning quick verification
/// must undo and fall back to full verification.
pub fn parchecker_is_verify_hash_mismatch(
    has_hash_entry: c_int,
    same_source_file: c_int,
    is_already_set: c_int,
) -> c_int {
    if has_hash_entry == 0 || same_source_file == 0 || is_already_set != 0 {
        1
    } else {
        0
    }
}

/// `ParChecker::VerifyDataFile` (`ParChecker.cpp`:1720):
/// Computes `i * blocksize` as `u64`.
pub fn parchecker_calc_block_offset(block_idx: u32, block_size: u64) -> u64 {
    (block_idx as u64).wrapping_mul(block_size)
}

/// `StateDiskFile::ReadLine` (`DiskState.cpp`:106-115):
/// Given a NUL-terminated line buffer of length `len`, returns `(needs_scroll, new_len)`
/// where `needs_scroll` is `true` if `len > 0 && buf[len - 1] != b'\n'`, and `new_len`
/// is `len - 1` when `len > 0` (since `buffer[strlen(buffer) - 1] = 0` is always executed
/// when `*buffer != 0`).
pub fn diskstate_inspect_read_line(buf: &[u8]) -> (bool, usize) {
    if buf.is_empty() {
        (false, 0)
    } else {
        let last = buf[buf.len() - 1];
        (last != b'\n', buf.len() - 1)
    }
}

/// `DiskState::LoadDownloadQueue` (`DiskState.cpp`:410-429):
/// Evaluates `format_version` against `queue_version` (64).
/// Returns status code:
/// - `1`: corrupted (`format_version <= 0`)
/// - `2`: too old (`format_version < 47`)
/// - `0`: valid
/// Also sets `out_is_legacy` (`format_version != queue_version`) and
/// `out_load_inline_history` (`format_version < 57`).
pub fn diskstate_check_queue_format_version(
    format_version: c_int,
    queue_version: c_int,
) -> (c_int, bool, bool) {
    let is_legacy = format_version != queue_version;
    if format_version <= 0 {
        (1, is_legacy, false)
    } else if format_version < 47 {
        (2, is_legacy, false)
    } else {
        (0, is_legacy, format_version < 57)
    }
}

/// `DiskState::LoadDupeExtras` (`DiskState.cpp`:896, 927):
/// Validates `scan_cnt == 1 && count >= 0 && count <= max_count`.
pub fn diskstate_validate_bounded_count(
    scan_cnt: c_int,
    count: c_int,
    max_count: c_int,
) -> c_int {
    if scan_cnt == 1 && count >= 0 && count <= max_count {
        1
    } else {
        0
    }
}

/// `DiskState::LoadDupeTrailer` / `DiskState::LoadNzbInfo` (`DiskState.cpp`:1095-1098, 1663-1666):
/// Returns `18` if `s` starts with `"ExtractedArchives="`, else `0`.
pub fn diskstate_extracted_archives_prefix_len(s: &[u8]) -> usize {
    if s.starts_with(b"ExtractedArchives=") {
        18
    } else {
        0
    }
}

/// `DiskState::SaveNzbInfo` (`DiskState.cpp`:1134):
/// Computes persisted postStage (`has_post_info != 0 ? stage + 1 : 0`).
pub fn diskstate_calc_persisted_post_stage(has_post_info: c_int, stage: c_int) -> c_int {
    if has_post_info != 0 {
        stage.wrapping_add(1)
    } else {
        0
    }
}

/// `DiskState::LoadNzbInfo` (`DiskState.cpp`:1540-1546):
/// Checks `num_items` returned by `ScanLine("%i,%i,%i,%i", ...)`.
/// Returns `0` on error (`num_items < 1`), `1` if valid without extra flags (`1..=3`),
/// or `2` if all 4 fields were read (`num_items == 4`).
pub fn diskstate_classify_param_header_items(num_items: c_int) -> c_int {
    if num_items < 1 {
        0
    } else if num_items == 4 {
        2
    } else {
        1
    }
}

/// `DiskState::LoadFileState` (`DiskState.cpp`:2266-2279):
/// Parses a `< 8` format trailer line:
/// - `#decoded <u32>,<u32>` -> returns `(1, JoinInt64(hi, lo), 0)`
/// - `OutputInitialized=<i32>` -> returns `(2, 0, val != 0)`
/// - anything else -> returns `(0, 0, 0)`
pub fn diskstate_parse_legacy_file_state_trailer(line: &[u8]) -> (c_int, i64, c_int) {
    if let Some(rest) = line.strip_prefix(b"#decoded ") {
        if let Some((hi, after_hi)) = parse_c_u32_prefix(rest) {
            if let Some(after_comma) = after_hi.strip_prefix(b",") {
                if let Some((lo, _)) = parse_c_u32_prefix(after_comma) {
                    let joined = ((hi as i64) << 32) | (lo as i64);
                    return (1, joined, 0);
                }
            }
        }
    }
    if let Some(rest) = line.strip_prefix(b"OutputInitialized=") {
        if let Some((val, _)) = parse_c_i32_prefix(rest) {
            return (2, 0, if val != 0 { 1 } else { 0 });
        }
    }
    (0, 0, 0)
}

fn parse_c_u32_prefix(mut s: &[u8]) -> Option<(u32, &[u8])> {
    while let Some(&b) = s.first() {
        if b.is_ascii_whitespace() {
            s = &s[1..];
        } else {
            break;
        }
    }
    let mut neg = false;
    if let Some(&b) = s.first() {
        if b == b'+' {
            s = &s[1..];
        } else if b == b'-' {
            neg = true;
            s = &s[1..];
        }
    }
    let mut digits = 0usize;
    let mut acc: u64 = 0;
    while let Some(&b) = s.get(digits) {
        if b.is_ascii_digit() {
            acc = acc.saturating_mul(10).saturating_add((b - b'0') as u64);
            digits += 1;
        } else {
            break;
        }
    }
    if digits == 0 {
        return None;
    }
    let val = if neg {
        (acc as u32).wrapping_neg()
    } else {
        acc.min(u32::MAX as u64) as u32
    };
    Some((val, &s[digits..]))
}

fn parse_c_i32_prefix(mut s: &[u8]) -> Option<(i32, &[u8])> {
    while let Some(&b) = s.first() {
        if b.is_ascii_whitespace() {
            s = &s[1..];
        } else {
            break;
        }
    }
    let mut neg = false;
    let mut base: u64 = 10;
    if let Some(&b) = s.first() {
        if b == b'+' {
            s = &s[1..];
        } else if b == b'-' {
            neg = true;
            s = &s[1..];
        }
    }
    if s.starts_with(b"0x") || s.starts_with(b"0X") {
        base = 16;
        s = &s[2..];
    } else if s.starts_with(b"0") && s.len() > 1 && s[1].is_ascii_digit() {
        base = 8;
    }
    let mut digits = 0usize;
    let mut acc: i64 = 0;
    while let Some(&b) = s.get(digits) {
        let d = match b {
            b'0'..=b'9' if (b - b'0') < base as u8 => (b - b'0') as i64,
            b'a'..=b'f' if base == 16 => (b - b'a' + 10) as i64,
            b'A'..=b'F' if base == 16 => (b - b'A' + 10) as i64,
            _ => break,
        };
        acc = acc.saturating_mul(base as i64).saturating_add(d);
        digits += 1;
    }
    if digits == 0 {
        return None;
    }
    let signed = if neg { -acc } else { acc };
    let clamped = signed.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    Some((clamped, &s[digits..]))
}

/// `DiskState::LoadFeedStatus` (`DiskState.cpp`:2841-2845):
/// Returns `1` if `feed_url == url` and the format-version-specific filter check matches:
/// - `format_version == 1`
/// - `format_version == 2 && feed_filter == filter`
/// - `format_version >= 3 && feed_filter_hash == filter_hash`
pub fn diskstate_feed_status_matches(
    feed_url: &[u8],
    url: &[u8],
    format_version: c_int,
    feed_filter: &[u8],
    filter: &[u8],
    feed_filter_hash: u32,
    filter_hash: u32,
) -> c_int {
    if feed_url != url {
        return 0;
    }
    let ok = format_version == 1
        || (format_version == 2 && feed_filter == filter)
        || (format_version >= 3 && feed_filter_hash == filter_hash);
    if ok {
        1
    } else {
        0
    }
}

/// `DiskState::LoadAllFileInfos` (`DiskState.cpp`:2970):
/// Returns `1` when `scan_cnt == 1 && scanned_size == expected_file_count`.
pub fn diskstate_hibernate_header_matches(
    scan_cnt: c_int,
    scanned_size: c_int,
    expected_file_count: c_int,
) -> c_int {
    if scan_cnt == 1 && scanned_size == expected_file_count {
        1
    } else {
        0
    }
}

/// `DiskState::LoadVolumeStat` (`DiskState.cpp`:3525):
/// Computes `static_cast<time_t>(firstDay * 86400)` matching C++ 32-bit signed `int`
/// multiplication `firstDay * 86400` widened to `i64`.
pub fn diskstate_calc_first_day_reset_time(first_day: c_int) -> i64 {
    first_day.wrapping_mul(86400) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dl_info_ids_and_stats() {
        assert_eq!(dl_info_update_max_id(10, 5), 10);
        assert_eq!(dl_info_update_max_id(3, 5), 5);
        assert_eq!(dl_info_reset_gen_id(1, 42), (42, 42));
        assert_eq!(dl_info_reset_gen_id(0, 42), (0, 0));

        let mut acc = NzbgetRsDpsCurrentStatsAcc::default();
        dl_info_update_current_stats_step(&mut acc, 1000, 5, 1, 5000, 500, 1, 1, 1);
        assert_eq!(acc.remaining_size, 1000);
        assert_eq!(acc.current_success_articles, 5);
        assert_eq!(acc.current_failed_articles, 1);
        assert_eq!(acc.current_success_size, 5000);
        assert_eq!(acc.current_failed_size, 500);
        assert_eq!(acc.extra_priority, 1);
        assert_eq!(acc.paused_file_count, 1);
        assert_eq!(acc.paused_size, 1000);
        assert_eq!(acc.remaining_par_count, 1);
        assert_eq!(acc.par_current_success_size, 5000);
        assert_eq!(acc.par_current_failed_size, 500);

        assert_eq!(
            dl_queue_calc_remaining_size_step(0, 0, 1, 500, 1000, 200),
            (1500, 700)
        );
        assert_eq!(
            dl_queue_calc_remaining_size_step(1, 0, 1, 500, 1000, 200),
            (1000, 200)
        );
    }

    #[test]
    fn test_parchecker_helpers() {
        assert_eq!(parchecker_should_quick_verify(2, 2, 1), 0);
        assert_eq!(parchecker_should_quick_verify(2, 2, 0), 1);
        assert_eq!(parchecker_should_quick_verify(1, 2, 1), 1);
        assert_eq!(parchecker_calc_files_to_repair(3, 4), 7);
        assert_eq!(parchecker_classify_twin_repair_err(5, 5, 1), 1);
        assert_eq!(parchecker_classify_twin_repair_err(5, 5, 0), 2);
        assert_eq!(parchecker_classify_twin_repair_err(0, 5, 1), 0);
        assert_eq!(parchecker_update_damaged_flag(0, 0), 1);
        assert_eq!(parchecker_update_damaged_flag(0, 1), 0);
        assert_eq!(parchecker_update_damaged_flag(1, 1), 1);
        assert_eq!(parchecker_is_verify_hash_mismatch(1, 1, 0), 0);
        assert_eq!(parchecker_is_verify_hash_mismatch(0, 1, 0), 1);
        assert_eq!(parchecker_is_verify_hash_mismatch(1, 0, 0), 1);
        assert_eq!(parchecker_is_verify_hash_mismatch(1, 1, 1), 1);
        assert_eq!(parchecker_calc_block_offset(10, 524288), 5_242_880);
    }

    #[test]
    fn test_diskstate_helpers() {
        assert_eq!(diskstate_inspect_read_line(b"hello\n"), (false, 5));
        assert_eq!(diskstate_inspect_read_line(b"hello"), (true, 4));
        assert_eq!(diskstate_inspect_read_line(b""), (false, 0));

        assert_eq!(diskstate_check_queue_format_version(0, 64), (1, true, false));
        assert_eq!(diskstate_check_queue_format_version(46, 64), (2, true, false));
        assert_eq!(diskstate_check_queue_format_version(56, 64), (0, true, true));
        assert_eq!(diskstate_check_queue_format_version(64, 64), (0, false, false));

        assert_eq!(diskstate_validate_bounded_count(1, 50, 100), 1);
        assert_eq!(diskstate_validate_bounded_count(1, -1, 100), 0);
        assert_eq!(diskstate_validate_bounded_count(1, 101, 100), 0);
        assert_eq!(diskstate_validate_bounded_count(0, 50, 100), 0);

        assert_eq!(
            diskstate_extracted_archives_prefix_len(b"ExtractedArchives=a.rar,b.rar"),
            18
        );
        assert_eq!(diskstate_extracted_archives_prefix_len(b"a.rar"), 0);

        assert_eq!(diskstate_calc_persisted_post_stage(1, 3), 4);
        assert_eq!(diskstate_calc_persisted_post_stage(0, 3), 0);

        assert_eq!(diskstate_classify_param_header_items(0), 0);
        assert_eq!(diskstate_classify_param_header_items(1), 1);
        assert_eq!(diskstate_classify_param_header_items(4), 2);

        assert_eq!(
            diskstate_parse_legacy_file_state_trailer(b"#decoded 1,100"),
            (1, (1i64 << 32) | 100, 0)
        );
        assert_eq!(
            diskstate_parse_legacy_file_state_trailer(b"OutputInitialized=1"),
            (2, 0, 1)
        );
        assert_eq!(
            diskstate_parse_legacy_file_state_trailer(b"OutputInitialized=0"),
            (2, 0, 0)
        );
        assert_eq!(
            diskstate_parse_legacy_file_state_trailer(b"unknown"),
            (0, 0, 0)
        );

        assert_eq!(
            diskstate_feed_status_matches(b"http://a", b"http://a", 3, b"", b"", 123, 123),
            1
        );
        assert_eq!(
            diskstate_feed_status_matches(b"http://a", b"http://a", 3, b"", b"", 123, 456),
            0
        );
        assert_eq!(diskstate_hibernate_header_matches(1, 10, 10), 1);
        assert_eq!(diskstate_hibernate_header_matches(1, 9, 10), 0);
        assert_eq!(diskstate_calc_first_day_reset_time(20000), 20000 * 86400);
    }
}
