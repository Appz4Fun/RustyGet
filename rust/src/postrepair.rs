//! Rust hot-path port for `DupeCoordinator`, `DupeMatcher`, `StreamCrypto`,
//! `RepairController`, `ParChecker`, `DirectUnpack`, and `UnpackController`.

use std::collections::{BTreeSet, VecDeque};
use std::ffi::{c_char, c_int, CStr};

// ============================================================================
// Case-insensitive comparison helpers
// ============================================================================

#[inline]
pub fn strcasecmp_bytes(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[inline]
pub fn strncasecmp_prefix(haystack: &[u8], prefix: &[u8]) -> bool {
    haystack.len() >= prefix.len() && haystack[..prefix.len()].eq_ignore_ascii_case(prefix)
}

#[inline]
pub fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ============================================================================
// 1. DupeCoordinator (`daemon/queue/DupeCoordinator.cpp`)
// ============================================================================

pub const DUPE_MODE_SCORE: i32 = 0;
pub const DUPE_MODE_FORCE: i32 = 2;

pub const HEALTH_CHECK_PARK: i32 = 2;
pub const HEALTH_CHECK_NONE: i32 = 3;
pub const HEALTH_CHECK_DUPE: i32 = 4;

pub const DELETE_STATUS_MANUAL: i32 = 1;
pub const DELETE_STATUS_HEALTH: i32 = 2;
pub const DELETE_STATUS_DUPE: i32 = 3;
pub const DELETE_STATUS_COPY: i32 = 6;

pub const MARK_STATUS_NONE: i32 = 0;
pub const MARK_STATUS_BAD: i32 = 1;
pub const MARK_STATUS_GOOD: i32 = 2;

pub const DUP_STATUS_SUCCESS: i32 = 1;
pub const DUP_STATUS_GOOD: i32 = 6;

pub const HISTORY_KIND_NZB: i32 = 1;
pub const HISTORY_KIND_URL: i32 = 2;
pub const HISTORY_KIND_DUP: i32 = 3;

/// `DupeCoordinator::SameNameOrKey`
pub fn same_name_or_key(name1: &[u8], dupe_key1: &[u8], name2: &[u8], dupe_key2: &[u8]) -> bool {
    let has_dupe_keys = !dupe_key1.is_empty() && !dupe_key2.is_empty();
    (has_dupe_keys && strcasecmp_bytes(dupe_key1, dupe_key2))
        || (!has_dupe_keys && strcasecmp_bytes(name1, name2))
}

/// `DupeCoordinator::DupeFailoverWarranted`
pub fn failover_warranted(item_score: i32, health: i32, backup_score: i32) -> bool {
    let clamped_health = health.clamp(0, 1000);
    let warranted = if item_score > 0 {
        ((item_score as i64) * (clamped_health as i64) / 1000) as i32
    } else {
        0
    };
    warranted == 0 || backup_score >= warranted
}

/// `DupeCoordinator::AlivePermille` string value parser
pub unsafe fn parse_alive_permille_cstr(value: *const c_char) -> i32 {
    crate::dupecoord::parse_alive_permille_cstr(value)
}

/// `DupeCoordinator::FailsOver`
pub fn fails_over(dupe_check: bool, dupe_mode: i32, health_check: i32) -> bool {
    dupe_check
        && (dupe_mode == DUPE_MODE_SCORE
            || (dupe_mode == DUPE_MODE_FORCE && health_check == HEALTH_CHECK_DUPE))
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NzbgetRsDupeHistoryCandidate {
    pub kind: i32,
    pub dupe_mode: i32,
    pub dupe_score: i32,
    pub is_dupe_success: i32,
    pub mark_status: i32,
    pub dup_status: i32,
    pub delete_status: i32,
    pub full_content_hash: u32,
    pub alive_permille: i32,
    pub health: i32,
    pub critical_health: i32,
    pub same_name_or_key: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NzbgetRsDupeQueueCandidate {
    pub is_self: i32,
    pub is_nzb_kind: i32,
    pub dupe_mode: i32,
    pub dupe_score: i32,
    pub same_name_or_key: i32,
}

/// `DupeCoordinator::FindDupeBackup` full decision algorithm.
/// Returns the winning `0..history.len()` index, or `-1` if no backup qualifies.
/// `file_exists_cb(ctx, idx)` is invoked lazily on the exact same candidates as C++.
pub fn select_dupe_backup<F>(
    has_self_nzb: bool,
    self_full_content_hash: u32,
    dupe_health: bool,
    history: &[NzbgetRsDupeHistoryCandidate],
    queue: &[NzbgetRsDupeQueueCandidate],
    mut file_exists_cb: F,
) -> i32
where
    F: FnMut(usize) -> bool,
{
    let mut dupe_found = false;
    let mut history_score = 0i32;

    for h in history {
        let mut good_dupe = false;

        if h.kind == HISTORY_KIND_NZB
            && h.dupe_mode != DUPE_MODE_FORCE
            && h.is_dupe_success != 0
            && h.same_name_or_key != 0
        {
            if !dupe_found || h.dupe_score > history_score {
                history_score = h.dupe_score;
            }
            dupe_found = true;
            good_dupe = h.mark_status == MARK_STATUS_GOOD;
        }

        if h.kind == HISTORY_KIND_DUP
            && h.dupe_mode != DUPE_MODE_FORCE
            && (h.dup_status == DUP_STATUS_SUCCESS || h.dup_status == DUP_STATUS_GOOD)
            && h.same_name_or_key != 0
        {
            if !dupe_found || h.dupe_score > history_score {
                history_score = h.dupe_score;
            }
            dupe_found = true;
            good_dupe = h.dup_status == DUP_STATUS_GOOD;
        }

        if good_dupe {
            return -1;
        }
    }

    let mut queue_dupe = false;
    let mut queue_score = 0i32;
    for q in queue {
        if q.is_self == 0
            && q.is_nzb_kind != 0
            && q.dupe_mode != DUPE_MODE_FORCE
            && q.same_name_or_key != 0
            && (!queue_dupe || q.dupe_score > queue_score)
        {
            queue_score = q.dupe_score;
            queue_dupe = true;
        }
    }

    let mut failed_content: BTreeSet<u32> = BTreeSet::new();
    if dupe_health {
        if has_self_nzb {
            failed_content.insert(self_full_content_hash);
        }
        for h in history {
            if h.kind == HISTORY_KIND_NZB
                && h.full_content_hash > 0
                && h.is_dupe_success == 0
                && h.delete_status != DELETE_STATUS_DUPE
                && h.delete_status != DELETE_STATUS_COPY
                && h.delete_status != DELETE_STATUS_MANUAL
            {
                failed_content.insert(h.full_content_hash);
            }
        }
    }

    let mut best_idx: Option<usize> = None;
    for (idx, h) in history.iter().enumerate() {
        let has_candidate = h.kind == HISTORY_KIND_NZB || h.kind == HISTORY_KIND_URL;
        let backup_status = has_candidate
            && (h.delete_status == DELETE_STATUS_DUPE
                || (dupe_health
                    && h.kind == HISTORY_KIND_NZB
                    && h.delete_status == DELETE_STATUS_COPY))
            && !(dupe_health
                && h.full_content_hash > 0
                && failed_content.contains(&h.full_content_hash));

        let candidate_score = if has_candidate { h.dupe_score } else { 0 };
        let better = has_candidate
            && match best_idx {
                None => true,
                Some(prev) => {
                    let prev_h = &history[prev];
                    candidate_score > prev_h.dupe_score
                        || (candidate_score == prev_h.dupe_score
                            && h.alive_permille > prev_h.alive_permille)
                }
            };

        if has_candidate
            && h.dupe_mode != DUPE_MODE_FORCE
            && backup_status
            && better
            && !(dupe_health && h.alive_permille == 0)
            && (h.kind != HISTORY_KIND_NZB || file_exists_cb(idx))
            && h.health >= h.critical_health
            && h.mark_status != MARK_STATUS_BAD
            && (!dupe_found || h.dupe_score > history_score)
            && (!queue_dupe || h.dupe_score > queue_score)
            && h.same_name_or_key != 0
        {
            best_idx = Some(idx);
        }
    }

    best_idx.map(|i| i as i32).unwrap_or(-1)
}

// ============================================================================
// 2. DupeMatcher (`daemon/postprocess/DupeMatcher.cpp`)
// ============================================================================

/// `DupeMatcher::SizeDiffOK`
pub fn size_diff_ok(size1: i64, size2: i64, max_diff_percent: i32) -> bool {
    if size1 == 0 || size2 == 0 {
        return false;
    }
    let diff = size1.wrapping_sub(size2);
    let diff = if diff > 0 { diff } else { diff.wrapping_neg() };
    let max = if size1 > size2 { size1 } else { size2 };
    let diff_percent = (diff.wrapping_mul(100) / max) as i32;
    diff_percent < max_diff_percent
}

pub const RAR_LISTER_NONE: i32 = 0;
pub const RAR_LISTER_ARCHIVE: i32 = 1;
pub const RAR_LISTER_NAME: i32 = 2;
pub const RAR_LISTER_SIZE_NEW_MAX: i32 = 3;
pub const RAR_LISTER_SIZE_NOT_MAX: i32 = 4;
pub const RAR_LISTER_COMPRESSION_TERMINATE: i32 = 5;

/// `RarLister::AddMessage` line parser.
pub unsafe fn parse_rar_lister_line_cstr(
    text: *const c_char,
    expected_size: i64,
    max_size: *mut i64,
    compressed: *mut c_int,
    last_size_max: *mut c_int,
) -> i32 {
    if text.is_null() || max_size.is_null() || compressed.is_null() || last_size_max.is_null() {
        return RAR_LISTER_NONE;
    }
    let bytes = CStr::from_ptr(text).to_bytes();
    let mut action = RAR_LISTER_NONE;

    if strncasecmp_prefix(bytes, b"Archive: ") {
        action = RAR_LISTER_ARCHIVE;
    } else if strncasecmp_prefix(bytes, b"        Name: ") {
        action = RAR_LISTER_NAME;
    } else if strncasecmp_prefix(bytes, b"        Size: ") {
        *last_size_max = 0;
        let size = crate::feed::c_atoll_bytes(&bytes[14..]);
        if size > *max_size {
            *max_size = size;
            *last_size_max = 1;
            return RAR_LISTER_SIZE_NEW_MAX;
        }
        return RAR_LISTER_SIZE_NOT_MAX;
    }

    if *last_size_max != 0 && strncasecmp_prefix(bytes, b" Compression: ") {
        *compressed = if find_subslice(bytes, b" -m0").is_none() {
            1
        } else {
            0
        };
        if *max_size > expected_size || size_diff_ok(*max_size, expected_size, 20) {
            return RAR_LISTER_COMPRESSION_TERMINATE;
        }
    }

    action
}

// ============================================================================
// 3. StreamCrypto (`daemon/postprocess/StreamCrypto.cpp`)
// ============================================================================

/// `StreamCrypto::DeriveRar3` 4-byte-group digest-to-key byte swizzle.
pub fn rar3_swizzle_key(digest: &[u8; 20], key_out: &mut [u8; 16]) {
    for i in 0..4 {
        for j in 0..4 {
            key_out[i * 4 + j] = digest[i * 4 + 3 - j];
        }
    }
}

/// `StreamCrypto::DeriveRar5PswCheck` 32-byte XOR fold into 8-byte password check.
pub fn rar5_fold_psw_check(value: &[u8; 32], check_out: &mut [u8; 8]) {
    let mut check = [0u8; 8];
    for i in 0..32 {
        check[i % 8] ^= value[i];
    }
    *check_out = check;
}

// ============================================================================
// 4. RepairController (`daemon/postprocess/Repair.cpp`)
// ============================================================================

/// `RepairController::RequestMorePars` two-stage greedy selection + superfluous pruning.
/// Expects `sorted_block_counts` sorted in ascending order of `m_blockCount`.
/// Returns `(selected_indices, remaining_block_needed)`.
pub fn select_more_pars(sorted_block_counts: &[i32], mut block_needed: i32) -> (Vec<usize>, i32) {
    let mut selected: VecDeque<(usize, i32)> = VecDeque::new();
    for (idx, &count) in sorted_block_counts.iter().enumerate() {
        if block_needed <= 0 {
            break;
        }
        selected.push_front((idx, count));
        block_needed -= count;
    }

    let mut i = 0;
    while i < selected.len() {
        let count = selected[i].1;
        if block_needed + count <= 0 {
            block_needed += count;
            selected.remove(i);
        } else {
            i += 1;
        }
    }

    let indices = selected.into_iter().map(|(idx, _)| idx).collect();
    (indices, block_needed)
}

/// `RepairController::PostParChecker::RequestTwinVolumes` single-step volume selector:
/// Given ascending-sorted `volume_blocks` and `left > 0`, returns the index of the first
/// volume with `blocks >= left`, or the last volume (`len - 1`) if none covers `left`.
pub fn pick_twin_volume(sorted_volume_blocks: &[i32], left: i32) -> Option<usize> {
    if sorted_volume_blocks.is_empty() || left <= 0 {
        return None;
    }
    if let Some(pos) = sorted_volume_blocks.iter().position(|&b| b >= left) {
        Some(pos)
    } else {
        Some(sorted_volume_blocks.len() - 1)
    }
}

/// `RepairController::UpdateParCheckProgress` repair time-limit check.
/// Returns `Some(estimated_repair_time_sec)` when the repair should be cancelled.
pub fn check_par_time_limit(
    par_time_limit_min: i32,
    is_repairing_stage: bool,
    elapsed_sec: i64,
    stage_progress: i32,
) -> Option<i32> {
    if par_time_limit_min <= 0 || !is_repairing_stage {
        return None;
    }
    let warmup_passed = (par_time_limit_min > 5 && elapsed_sec > 5 * 60)
        || (par_time_limit_min <= 5 && elapsed_sec > 60);
    if !warmup_passed {
        return None;
    }
    let denom = if stage_progress > 0 {
        stage_progress as i64
    } else {
        1
    };
    let estimated_repair_time = (elapsed_sec * 1000 / denom) as i32;
    if estimated_repair_time > par_time_limit_min * 60 {
        Some(estimated_repair_time)
    } else {
        None
    }
}

// ============================================================================
// 5. ParChecker (`daemon/postprocess/ParChecker.cpp`)
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NzbgetRsParSegment {
    pub success: i32,
    pub offset: i64,
    pub size: i32,
    pub crc: u32,
}

pub use crate::parchecker::{
    calc_stage_progress, classify_streambuf_line, compute_valid_blocks, maybe_splitted_fragment,
    smart_calc_range_crc, PAR_STAGE_LOADING_PARS, PAR_STAGE_REPAIRING,
    PAR_STAGE_VERIFYING_REPAIRED, PAR_STAGE_VERIFYING_SOURCES,
};

// ============================================================================
// 6. DirectUnpack, UnpackController & RarVolume (`DirectUnpack.cpp`, `UnpackController.cpp`, `RarReader.cpp`)
// ============================================================================

/// `RarVolume::DetectRarVersion` and `UnpackController::FileHasRarSignature`:
/// Given the 8-byte file signature read from disk, returns `3` (RAR3/4), `5` (RAR5), or `0` (not RAR).
pub fn detect_rar_signature(sig: &[u8]) -> i32 {
    if sig.len() != 8 {
        return 0;
    }
    const RAR3: &[u8] = b"Rar!\x1a\x07\x00";
    const RAR5: &[u8] = b"Rar!\x1a\x07\x01\x00";
    let strcmp_eq = |s: &[u8]| s.iter().zip(sig.iter()).all(|(a, b)| a == b);
    let rar5 = strcmp_eq(RAR5);
    let rar3 = !rar5 && strcmp_eq(RAR3);
    if rar3 {
        3
    } else if rar5 {
        5
    } else {
        0
    }
}

/// `UnpackController::JoinFile` fragment continuity and size validation:
/// Returns `true` when the split fragments can be joined, or `false` if missing fragments are detected.
pub fn check_joinable_fragments(
    count: i32,
    min_seg: i32,
    max_seg: i32,
    dif_size_min: i32,
    par_success: bool,
) -> bool {
    let corrected_count = count - if min_seg == 0 { 1 } else { 0 };
    if min_seg > 1
        || corrected_count != max_seg
        || ((dif_size_min != corrected_count || dif_size_min > max_seg) && !par_success)
    {
        return false;
    }
    true
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NzbgetRsUnpackMessageInfo {
    /// Non-zero if `"Unrar: Extracting  <unpack_extended_dir>..."` prefix was stripped;
    /// `stripped_suffix_offset` is the byte offset into `text` for `"Unrar: Extracting %s"`.
    pub strip_extracting_dir: i32,
    pub stripped_suffix_offset: usize,
    /// Non-zero if DirectUnpack saw `"Unrar: Insert disk with ... [C]ontinue, [Q]uit"`.
    pub is_insert_disk: i32,
    pub insert_disk_name_offset: usize,
    pub insert_disk_name_len: usize,
    /// Non-zero if `"Unrar: UNRAR ... Copyright ... Alexander Roshal"` banner was seen.
    pub reset_stage_time: i32,
    /// Non-zero if `"Unrar: Extracting "` or `"Unrar: Extracting from "` was seen.
    pub has_extracting_progress: i32,
    /// Non-zero if `"Unrar: Extracting from "` was seen (archive name at `text + 23`).
    pub has_extracting_from: i32,
    /// Non-zero if a decrypt error line was matched.
    pub decrypt_error: i32,
    /// Non-zero if a password error line was matched.
    pub password_error: i32,
    /// Non-zero if a DirectUnpack password error (`"Unrar: The specified password is incorrect."`) was matched.
    pub direct_password_error: i32,
    /// Non-zero if a packed data CRC/checksum or previous-volume error was matched.
    pub crc_or_volume_error: i32,
    /// Non-zero if `"Unrar: All OK"` or `"7-Zip: Everything is Ok"` was matched.
    pub all_ok: i32,
}

/// Parses an unpacker output line for both `DirectUnpack::AddMessage` (`is_seven_zip == false`)
/// and `UnpackController::AddMessage` (`is_seven_zip == false` for Unrar, `true` for 7-Zip).
pub fn parse_unpack_message(
    text: &[u8],
    unpack_extended_dir: &[u8],
    is_seven_zip: bool,
) -> NzbgetRsUnpackMessageInfo {
    let mut info = NzbgetRsUnpackMessageInfo::default();
    let len = text.len();

    if !is_seven_zip
        && text.starts_with(b"Unrar: Extracting  ")
        && text[19..].starts_with(unpack_extended_dir)
    {
        info.strip_extracting_dir = 1;
        info.stripped_suffix_offset = (19 + unpack_extended_dir.len() + 1).min(len);
    }

    if !is_seven_zip && text.starts_with(b"Unrar: Insert disk with") {
        if let Some(pos) = find_subslice(text, b" [C]ontinue, [Q]uit") {
            info.is_insert_disk = 1;
            if pos >= 24 {
                info.insert_disk_name_offset = 24;
                info.insert_disk_name_len = pos - 24;
            }
        }
    }

    // Banner check runs on msgText in C++ (which is identical to text unless text started with "Unrar: Extracting  ")
    if !is_seven_zip
        && info.strip_extracting_dir == 0
        && text.starts_with(b"Unrar: UNRAR ")
        && find_subslice(text, b" Copyright ").is_some()
        && find_subslice(text, b" Alexander Roshal").is_some()
    {
        info.reset_stage_time = 1;
    }

    if !is_seven_zip && (info.strip_extracting_dir != 0 || text.starts_with(b"Unrar: Extracting "))
    {
        info.has_extracting_progress = 1;
    }

    if !is_seven_zip && text.starts_with(b"Unrar: Extracting from ") {
        info.has_extracting_from = 1;
    }

    if !is_seven_zip {
        // Note: C++ uses strncmp(text, "Unrar: Checksum error in the encrypted file", 42)
        if text.starts_with(&b"Unrar: Checksum error in the encrypted file"[..42])
            || text.starts_with(b"Unrar: CRC failed in the encrypted file")
        {
            info.decrypt_error = 1;
        }
        if text.starts_with(b"Unrar: The specified password is incorrect.") {
            info.password_error = 1;
            info.direct_password_error = 1;
        } else if text.starts_with(b"Unrar: Incorrect password for") {
            info.password_error = 1;
        }
    } else if find_subslice(text, b"Cannot open encrypted archive. Wrong password?").is_some()
        || find_subslice(text, b"Data Error in encrypted file. Wrong password?").is_some()
        || find_subslice(text, b"CRC Failed in encrypted file. Wrong password?").is_some()
    {
        info.decrypt_error = 1;
    }

    if find_subslice(text, b" : packed data CRC failed in volume").is_some()
        || find_subslice(text, b" : packed data checksum error in volume").is_some()
        || (len > 13 && text.ends_with(b" - CRC failed"))
        || (len > 18 && text.ends_with(b" - checksum failed"))
        || text.starts_with(b"Unrar: WARNING: You need to start extraction from a previous volume")
    {
        info.crc_or_volume_error = 1;
    }

    if (!is_seven_zip && text.starts_with(b"Unrar: All OK"))
        || (is_seven_zip && text.starts_with(b"7-Zip: Everything is Ok"))
    {
        info.all_ok = 1;
    }

    info
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_same_name_or_key_and_failover() {
        assert!(same_name_or_key(b"Movie.A", b"key1", b"Movie.B", b"KEY1"));
        assert!(!same_name_or_key(b"Movie.A", b"key1", b"Movie.A", b"key2"));
        assert!(same_name_or_key(b"Movie.A", b"", b"movie.a", b""));
        assert!(same_name_or_key(b"Movie.A", b"key1", b"movie.a", b""));

        assert!(failover_warranted(0, 900, -100));
        assert!(failover_warranted(1000, 300, 300));
        assert!(!failover_warranted(1000, 300, 299));

        assert!(fails_over(true, DUPE_MODE_SCORE, HEALTH_CHECK_NONE));
        assert!(fails_over(true, DUPE_MODE_FORCE, HEALTH_CHECK_DUPE));
        assert!(!fails_over(true, DUPE_MODE_FORCE, HEALTH_CHECK_PARK));
        assert!(!fails_over(false, DUPE_MODE_SCORE, HEALTH_CHECK_DUPE));
    }

    #[test]
    fn test_parse_alive_permille() {
        let s1 = CString::new("97.4").unwrap();
        let s2 = CString::new("150.0").unwrap();
        let s3 = CString::new("invalid").unwrap();
        unsafe {
            assert_eq!(parse_alive_permille_cstr(s1.as_ptr()), 974);
            assert_eq!(parse_alive_permille_cstr(s2.as_ptr()), 1000);
            assert_eq!(parse_alive_permille_cstr(s3.as_ptr()), -1);
            assert_eq!(parse_alive_permille_cstr(std::ptr::null()), -1);
        }
    }

    #[test]
    fn test_select_dupe_backup_prefers_alive_and_skips_failed_hash() {
        let history = vec![
            // Index 0: failed copy with hash 42
            NzbgetRsDupeHistoryCandidate {
                kind: HISTORY_KIND_NZB,
                dupe_mode: DUPE_MODE_SCORE,
                dupe_score: 500,
                is_dupe_success: 0,
                mark_status: MARK_STATUS_NONE,
                dup_status: 0,
                delete_status: DELETE_STATUS_HEALTH,
                full_content_hash: 42,
                alive_permille: 900,
                health: 500,
                critical_health: 900,
                same_name_or_key: 1,
            },
            // Index 1: dsCopy of hash 42 -> should be skipped because hash 42 failed
            NzbgetRsDupeHistoryCandidate {
                kind: HISTORY_KIND_NZB,
                dupe_mode: DUPE_MODE_SCORE,
                dupe_score: 500,
                is_dupe_success: 0,
                mark_status: MARK_STATUS_NONE,
                dup_status: 0,
                delete_status: DELETE_STATUS_COPY,
                full_content_hash: 42,
                alive_permille: 1000,
                health: 1000,
                critical_health: 900,
                same_name_or_key: 1,
            },
            // Index 2: dsDupe with score 100, alive 950
            NzbgetRsDupeHistoryCandidate {
                kind: HISTORY_KIND_NZB,
                dupe_mode: DUPE_MODE_SCORE,
                dupe_score: 100,
                is_dupe_success: 0,
                mark_status: MARK_STATUS_NONE,
                dup_status: 0,
                delete_status: DELETE_STATUS_DUPE,
                full_content_hash: 77,
                alive_permille: 950,
                health: 1000,
                critical_health: 900,
                same_name_or_key: 1,
            },
            // Index 3: dsDupe with equal score 100, higher alive 980 -> wins
            NzbgetRsDupeHistoryCandidate {
                kind: HISTORY_KIND_NZB,
                dupe_mode: DUPE_MODE_SCORE,
                dupe_score: 100,
                is_dupe_success: 0,
                mark_status: MARK_STATUS_NONE,
                dup_status: 0,
                delete_status: DELETE_STATUS_DUPE,
                full_content_hash: 88,
                alive_permille: 980,
                health: 1000,
                critical_health: 900,
                same_name_or_key: 1,
            },
        ];
        let winner = select_dupe_backup(true, 99, true, &history, &[], |_| true);
        assert_eq!(winner, 3);
    }

    #[test]
    fn test_size_diff_ok_and_rar_lister() {
        assert!(size_diff_ok(100, 85, 20));
        assert!(!size_diff_ok(100, 80, 20));
        assert!(!size_diff_ok(0, 100, 20));

        let mut max_size = -1i64;
        let mut compressed = 0;
        let mut last_size_max = 0;
        let l1 = CString::new("        Name: movie.mkv").unwrap();
        let l2 = CString::new("        Size: 1000").unwrap();
        let l3 = CString::new(" Compression: RAR 5.0(v50) -m3").unwrap();
        unsafe {
            assert_eq!(
                parse_rar_lister_line_cstr(
                    l1.as_ptr(),
                    950,
                    &mut max_size,
                    &mut compressed,
                    &mut last_size_max
                ),
                RAR_LISTER_NAME
            );
            assert_eq!(
                parse_rar_lister_line_cstr(
                    l2.as_ptr(),
                    950,
                    &mut max_size,
                    &mut compressed,
                    &mut last_size_max
                ),
                RAR_LISTER_SIZE_NEW_MAX
            );
            assert_eq!(max_size, 1000);
            assert_eq!(last_size_max, 1);
            assert_eq!(
                parse_rar_lister_line_cstr(
                    l3.as_ptr(),
                    950,
                    &mut max_size,
                    &mut compressed,
                    &mut last_size_max
                ),
                RAR_LISTER_COMPRESSION_TERMINATE
            );
            assert_eq!(compressed, 1);
        }
    }

    #[test]
    fn test_stream_crypto_helpers() {
        let mut digest = [0u8; 20];
        for (i, b) in digest.iter_mut().enumerate() {
            *b = i as u8;
        }
        let mut key = [0u8; 16];
        rar3_swizzle_key(&digest, &mut key);
        assert_eq!(key, [3, 2, 1, 0, 7, 6, 5, 4, 11, 10, 9, 8, 15, 14, 13, 12]);

        let mut val = [0u8; 32];
        val[0] = 0xAA;
        val[8] = 0x55;
        let mut check = [0u8; 8];
        rar5_fold_psw_check(&val, &mut check);
        assert_eq!(check[0], 0xFF);
    }

    #[test]
    fn test_repair_select_more_pars_and_twin_volume() {
        // Volumes of 1, 2, 4, 8 blocks; need 5 blocks -> picks 1, 2, 4 (total 7, excess 2), then prunes 2 -> keeps [4, 1] (indices [2, 0])
        let counts = [1, 2, 4, 8];
        let (indices, rem) = select_more_pars(&counts, 5);
        assert_eq!(indices, vec![2, 0]);
        assert_eq!(rem, 0);

        assert_eq!(pick_twin_volume(&counts, 3), Some(2));
        assert_eq!(pick_twin_volume(&counts, 10), Some(3));
        assert_eq!(check_par_time_limit(10, true, 360, 100), Some(3600));
        assert_eq!(
            crate::parparser::parse_par_filename(b"movie.vol07+08.par2", true),
            Some((5, 8))
        );
        assert_eq!(
            crate::parparser::parse_par_filename(b"movie.vol-06.par2", true),
            Some((5, -1))
        );
        assert_eq!(
            crate::parparser::parse_par_filename(b"movie.par2", true),
            Some((5, 0))
        );
    }

    #[test]
    fn test_parchecker_helpers() {
        assert!(maybe_splitted_fragment(b"movie.mkv.001", b"movie.mkv"));
        assert!(maybe_splitted_fragment(b"movie.001", b"movie.002"));
        assert!(!maybe_splitted_fragment(b"movie.rar", b"movie.r00"));

        assert_eq!(
            calc_stage_progress(PAR_STAGE_VERIFYING_SOURCES, 500, 2, 0, 4, 1, 1),
            375
        );

        let (extra_dbg, off) = classify_streambuf_line(b"You have 5 recovery blocks");
        assert!(!extra_dbg);
        assert_eq!(off, 9);

        let (extra_dbg2, _) = classify_streambuf_line(b"Target: \"a.rar\" - missing");
        assert!(extra_dbg2);
        let (extra_dbg3, _) = classify_streambuf_line(b"Target: \"a.rar\" - found");
        assert!(!extra_dbg3);
    }

    #[test]
    fn test_unpack_helpers() {
        assert_eq!(detect_rar_signature(b"Rar!\x1a\x07\x00\x99"), 3);
        assert_eq!(detect_rar_signature(b"Rar!\x1a\x07\x01\x00"), 5);
        assert_eq!(detect_rar_signature(b"PK\x03\x04\x00\x00\x00\x00"), 0);

        assert!(check_joinable_fragments(3, 1, 3, 3, false));
        assert!(!check_joinable_fragments(3, 1, 3, 999999, false));
        assert!(check_joinable_fragments(3, 1, 3, 999999, true));

        let info = parse_unpack_message(
            b"Unrar: Extracting  /dst/_unpack/video.mkv",
            b"/dst/_unpack",
            false,
        );
        assert_eq!(info.strip_extracting_dir, 1);
        assert_eq!(
            &b"Unrar: Extracting  /dst/_unpack/video.mkv"[info.stripped_suffix_offset..],
            b"video.mkv"
        );
        assert_eq!(info.has_extracting_progress, 1);
    }
}
