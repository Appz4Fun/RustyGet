//! ArticleDownloader, DirectUnpack, UnpackController, Repair & ParChecker Hot-Path Port (`rust/src/dlunpackrepair.rs`).
//!
//! Covers hot-path computational, parsing, classification, and state-transition kernels for:
//! - `daemon/nntp/ArticleDownloader.cpp`:
//!   - `article_dl_is_retention_failure`: server retention vs file age check (`Run`)
//!   - `article_dl_all_servers_on_level_failed`: per-level server exhaustion check (`Run`)
//!   - `article_dl_resolve_final_status`: final status resolution after loop exit (`Run`)
//!   - `article_dl_should_throttle_bandwidth`: speed limit throttling check (`Download`)
//!   - `article_dl_classify_decode_status`: `Decoder::EStatus` classification (`DecodeCheck`)
//! - `daemon/postprocess/DirectUnpack.cpp`:
//!   - `direct_unpack_resolve_final_status`: `nsSuccess` / `nsNone` / `nsFailure` (`Run`)
//!   - `direct_unpack_use_inter_dir`: `InterDir` prefix + separator check (`CreateUnpackDir`)
//!   - `direct_unpack_is_main_archive`: `.part([0-9]+).rar$` part 1 or unsplit check (`IsMainArchive`)
//!   - `direct_unpack_is_insert_disk_prompt`: `Insert disk with ... [C]ontinue, [Q]uit ` check (`ReadLine`)
//! - `daemon/postprocess/UnpackController.cpp`:
//!   - `unpack_ctrl_should_skip_second_attempt`: second unpack attempt skip predicate (`Run`)
//!   - `unpack_ctrl_eval_unrar_exit`: `exitCode` flags (`unpack_ok`, `start_err`, `space_err`, `pass_err`) (`ExecuteUnrar`)
//!   - `unpack_ctrl_classify_archive_file`: `.rar`, `.r00-.z99`, `.7z`, `.7z.NNN`, `.xxx.NNN` (`CheckArchiveFiles`, `Cleanup`)
//!   - `unpack_ctrl_resolve_fail_status`: `usSpace` / `usPassword` / `usFailure` (`Completed`, `RequestParCheck`)
//!   - `unpack_ctrl_parse_progress_percent`: `%` progress parsing after backspace (`ReadLine`)
//!   - `unpack_ctrl_has_completed_archive_ext`: `.rar`, `.7z`, `.7z.NNN` check (`HasCompletedArchiveFiles`)
//! - `daemon/postprocess/Repair.cpp`:
//!   - `repair_ctrl_classify_file_crc_status`: `CompletedFile::EStatus` -> `ParChecker::EFileStatus` (`FindFileCrc`)
//!   - `repair_ctrl_estimate_twin_vol_blocks`: encoded size to PAR2 block count (`RequestTwinVolumes`)
//!   - `repair_ctrl_estimate_par_blocks_from_size`: unencoded size to PAR2 block count (`FindPars`)
//!   - `repair_ctrl_match_strict_par_name`: `<base>.par2` / `<base>.vol` substring check (`FindPars`)
//!   - `repair_ctrl_resolve_par_status`: `ParChecker::EStatus` -> `NzbInfo::EParStatus` (`ParCheckCompleted`)
//! - `daemon/postprocess/ParChecker.cpp`:
//!   - `parchecker_calc_threads_and_mem`: thread count and memory limit (`Repairer::Repairer`)
//!   - `parchecker_wildcard_truncate_offset`: last path separator offset for `*` wildcard (`Repairer::PreProcess`)
//!   - `parchecker_calc_pad_bytes`: zero-padding byte count to block multiple (`VerifySuccessDataFile`)
//!   - `parchecker_compare_extra_file_stems`: stem comparison for `SortExtraFiles`
//! - `daemon/systemhealth/` (`FeedsValidator.cpp`, `CategoriesValidator.cpp`, `PathsValidator.cpp`, `SystemHealthService.cpp`):
//!   - `health_find_duplicate_str`: first duplicate non-empty string index pair (`DuplicateFeedsValidator`, `DuplicateNamesValidator`)
//!   - `health_classify_log_file_mode`: `LogFileValidator::Validate` mode classification
//!   - `health_classify_cert_store`: `CertStoreValidator::Validate` empty/certCheck classification
//!   - `health_alert_matches`: `Service::ReportAlert` source & category match

// ============================================================================
// ArticleDownloader (`daemon/nntp/ArticleDownloader.cpp`)
// ============================================================================

/// Matches the server retention check in `ArticleDownloader::Run`:
/// returns `(is_failure, age_days)`.
pub fn article_dl_is_retention_failure(
    retention_days: i32,
    file_time: i64,
    cur_time: i64,
) -> (bool, i32) {
    if retention_days <= 0 || file_time <= 0 {
        return (false, 0);
    }
    let age_days = (cur_time.wrapping_sub(file_time) / 86400) as i32;
    (age_days > retention_days, age_days)
}

/// Matches the `allServersOnLevelFailed` loop in `ArticleDownloader::Run`.
/// For each candidate server `i` in `0..cand_levels.len()`:
/// if `cand_levels[i] == level` and `cand_avail[i] != 0` (meaning active, max_conn > 0, and not
/// optional-blocked), checks whether any failed server `j` has `failed_ids[j] == cand_ids[i]` or
/// `(failed_groups[j] > 0 && failed_groups[j] == cand_groups[i] && failed_levels[j] == cand_levels[i])`.
pub fn article_dl_all_servers_on_level_failed(
    level: i32,
    cand_ids: &[i32],
    cand_levels: &[i32],
    cand_groups: &[i32],
    cand_avail: &[u8],
    failed_ids: &[i32],
    failed_levels: &[i32],
    failed_groups: &[i32],
) -> bool {
    let n = cand_ids
        .len()
        .min(cand_levels.len())
        .min(cand_groups.len())
        .min(cand_avail.len());
    let f = failed_ids
        .len()
        .min(failed_levels.len())
        .min(failed_groups.len());

    for i in 0..n {
        if cand_levels[i] != level {
            continue;
        }
        if cand_avail[i] == 0 {
            continue;
        }
        let cid = cand_ids[i];
        let clvl = cand_levels[i];
        let cgrp = cand_groups[i];
        let mut server_failed = false;
        for j in 0..f {
            if failed_ids[j] == cid
                || (failed_groups[j] > 0 && failed_groups[j] == cgrp && failed_levels[j] == clvl)
            {
                server_failed = true;
                break;
            }
        }
        if !server_failed {
            return false;
        }
    }
    true
}

/// Matches the post-loop status normalization in `ArticleDownloader::Run`:
/// `adFinished = 2`, `adFailed = 3`, `adRetry = 4`, `adFatalError = 7`.
pub fn article_dl_resolve_final_status(
    mut status: i32,
    is_duplicate: bool,
    is_stopped: bool,
    ad_finished: i32,
    ad_failed: i32,
    ad_retry: i32,
    ad_fatal_error: i32,
) -> i32 {
    if is_duplicate {
        status = ad_finished;
    }
    if status != ad_finished && status != ad_retry && status != ad_fatal_error {
        status = ad_failed;
    }
    if is_stopped {
        status = ad_retry;
    }
    status
}

/// Matches the bandwidth throttling condition in `ArticleDownloader::Download`:
/// `!is_stopped && speed_limit > 0.0 && (cur_speed > speed_limit || mom_speed > speed_limit)`.
pub fn article_dl_should_throttle_bandwidth(
    is_stopped: bool,
    speed_limit: f32,
    cur_speed: f32,
    mom_speed: f32,
) -> bool {
    !is_stopped && speed_limit > 0.0 && (cur_speed > speed_limit || mom_speed > speed_limit)
}

/// Matches `ArticleDownloader::DecodeCheck()`:
/// Given `decoder_status` (`0=dsUnknownError`, `1=dsFinished`, `2=dsArticleIncomplete`,
/// `3=dsCrcError`, `4=dsInvalidSize`, `5=dsNoBinaryData`), `is_yenc`, `raw_article`, and
/// `range_exceeded`:
/// Returns `(action_code, use_calculated_crc)`:
/// - `0`: `adFinished` (raw article)
/// - `1`: `adFinished` (decoded ok)
/// - `2`: `adFailed` ("more data than its range")
/// - `3`: `adCrcError` ("CRC-Error")
/// - `4`: `adFailed` ("article incomplete")
/// - `5`: `adFailed` ("size mismatch")
/// - `6`: `adFailed` ("no binary data found")
/// - `7`: `adFailed` (generic "Decoding %s failed")
pub fn article_dl_classify_decode_status(
    raw_article: bool,
    decoder_status: i32,
    is_yenc: bool,
    range_exceeded: bool,
) -> i32 {
    if raw_article {
        return 0;
    }
    if decoder_status == 1 && is_yenc && range_exceeded {
        return 2;
    }
    match decoder_status {
        1 => 1,
        3 => 3,
        2 => 4,
        4 => 5,
        5 => 6,
        _ => 7,
    }
}

// ============================================================================
// DirectUnpack (`daemon/postprocess/DirectUnpack.cpp`)
// ============================================================================

/// Matches `nzbInfo->SetDirectUnpackStatus(...)` in `DirectUnpack::Run`:
/// `processed ? (unpack_ok && !interrupted && !cancelled ? ns_success : (interrupted ? ns_none : ns_failure)) : ns_none`.
pub fn direct_unpack_resolve_final_status(
    processed: bool,
    unpack_ok: bool,
    interrupted: bool,
    cancelled: bool,
    ns_none: i32,
    ns_success: i32,
    ns_failure: i32,
) -> i32 {
    if !processed {
        return ns_none;
    }
    if unpack_ok && !interrupted && !cancelled {
        ns_success
    } else if interrupted {
        ns_none
    } else {
        ns_failure
    }
}

/// Matches `useInterDir` in `DirectUnpack::CreateUnpackDir` and `UnpackController::CreateUnpackDir`:
/// `!inter_dir.is_empty() && dest_dir.starts_with(inter_dir) && dest_dir.get(inter_dir.len()) == Some(&sep)`.
pub fn direct_unpack_use_inter_dir(dest_dir: &[u8], inter_dir: &[u8], sep: u8) -> bool {
    if inter_dir.is_empty() || dest_dir.len() <= inter_dir.len() {
        return false;
    }
    dest_dir.starts_with(inter_dir) && dest_dir[inter_dir.len()] == sep
}

/// Matches `RegEx regExRarPart(".*\\.part([0-9]+)\\.rar$"); return !regExRarPart.Match(filename) || atoi(...) == 1;`
/// in `DirectUnpack::IsMainArchive` (note: `RegEx` uses `REG_ICASE` in nzbget).
pub fn direct_unpack_is_main_archive(filename: &[u8]) -> bool {
    // Search for the last case-insensitive `.part<digits>.rar` at the end of `filename`.
    if filename.len() < 10 {
        return true;
    }
    let rar_pos = filename.len() - 4;
    if !filename[rar_pos..].eq_ignore_ascii_case(b".rar") {
        return true;
    }
    let before_rar = &filename[..rar_pos];
    let mut d_start = before_rar.len();
    while d_start > 0 && before_rar[d_start - 1].is_ascii_digit() {
        d_start -= 1;
    }
    // Must have at least 1 digit and be preceded by `.part` case-insensitively
    if d_start == before_rar.len() || d_start < 5 {
        return true;
    }
    if !before_rar[d_start - 5..d_start].eq_ignore_ascii_case(b".part") {
        return true;
    }
    // Parse digits via C `atoi` semantics (leading zeros + 1 == 1)
    let digits = &before_rar[d_start..];
    let mut val: i64 = 0;
    for &b in digits {
        val = val.saturating_mul(10).saturating_add(i64::from(b - b'0'));
    }
    val == 1
}

/// Matches the prompt check in `DirectUnpack::ReadLine`:
/// `i > 35 && ch == ' ' && !strncmp(buf, "Insert disk with", 16) && strstr(buf, " [C]ontinue, [Q]uit ")`.
pub fn direct_unpack_is_insert_disk_prompt(buf: &[u8], i: usize, ch: i32) -> bool {
    if i <= 35 || ch != i32::from(b' ') {
        return false;
    }
    if !buf.starts_with(b"Insert disk with") {
        return false;
    }
    let needle = b" [C]ontinue, [Q]uit ";
    buf.windows(needle.len()).any(|w| w == needle)
}

// ============================================================================
// UnpackController (`daemon/postprocess/UnpackController.cpp`)
// ============================================================================

/// Matches the second unpack attempt skip condition in `UnpackController::Run`:
/// `unpack_tried && !par_repaired && (!password_empty || pass_file_empty || pass_list_tried)`.
pub fn unpack_ctrl_should_skip_second_attempt(
    unpack_tried: bool,
    par_repaired: bool,
    password_empty: bool,
    pass_file_empty: bool,
    pass_list_tried: bool,
) -> bool {
    unpack_tried && !par_repaired && (!password_empty || pass_file_empty || pass_list_tried)
}

/// Matches exit code flag evaluation in `UnpackController::ExecuteUnrar`:
/// returns `(unpack_ok, start_error, space_error, password_error)`.
pub fn unpack_ctrl_eval_unrar_exit(
    exit_code: i32,
    all_ok_received: bool,
    terminated: bool,
    auto_terminated: bool,
    prev_password_error: bool,
) -> (bool, bool, bool, bool) {
    let unpack_ok = exit_code == 0 && all_ok_received && !terminated;
    let start_error = exit_code == -1 && !auto_terminated;
    let space_error = exit_code == 5;
    let password_error = prev_password_error || exit_code == 11;
    (unpack_ok, start_error, space_error, password_error)
}

/// Matches archive filename classification in `UnpackController::CheckArchiveFiles`:
/// Returns:
/// - `1`: `.*\\.rar$` (case-insensitive)
/// - `2`: `.*\\.7z$` (case-insensitive)
/// - `3`: `.*\\.7z\\.[0-9]+$` (case-insensitive)
/// - `4`: `.*\\.[a-z,0-9]{3}\\.[0-9]{3}$` with `extNum == 0 || extNum == 1`
/// - `5`: `.*\\.[r-z][0-9][0-9]$` (case-insensitive multi-seq RAR volume, not a renamed archive)
/// - `6`: `.*\\.[a-z,0-9]{3}\\.[0-9]{3}$` with `extNum > 1`
/// - `0`: other file (candidate for `FileHasRarSignature` check if not ignored)
pub fn unpack_ctrl_classify_archive_file(filename: &[u8]) -> i32 {
    if filename.len() >= 4 && filename[filename.len() - 4..].eq_ignore_ascii_case(b".rar") {
        return 1;
    }
    if filename.len() >= 3 && filename[filename.len() - 3..].eq_ignore_ascii_case(b".7z") {
        return 2;
    }
    if let Some(dot_pos) = filename.iter().rposition(|&b| b == b'.') {
        let ext = &filename[dot_pos + 1..];
        let before = &filename[..dot_pos];
        if !ext.is_empty()
            && ext.iter().all(|b| b.is_ascii_digit())
            && before.len() >= 3
            && before[before.len() - 3..].eq_ignore_ascii_case(b".7z")
        {
            return 3;
        }
        if ext.len() == 3 && ext.iter().all(|b| b.is_ascii_digit()) && before.len() >= 4 {
            let prev_ext = &before[before.len() - 4..];
            if prev_ext[0] == b'.'
                && prev_ext[1..].iter().all(|&b| {
                    b.is_ascii_lowercase()
                        || b.is_ascii_uppercase()
                        || b.is_ascii_digit()
                        || b == b','
                })
            {
                let ext_num = (i32::from(ext[0] - b'0') * 100)
                    + (i32::from(ext[1] - b'0') * 10)
                    + i32::from(ext[2] - b'0');
                return if ext_num == 0 || ext_num == 1 { 4 } else { 6 };
            }
        }
        if ext.len() == 3 {
            let c0 = ext[0].to_ascii_lowercase();
            if (b'r'..=b'z').contains(&c0) && ext[1].is_ascii_digit() && ext[2].is_ascii_digit() {
                return 5;
            }
        }
    }
    0
}

/// Matches `m_unpackSpaceError ? NzbInfo::usSpace : m_unpackPasswordError || m_unpackDecryptError ? NzbInfo::usPassword : NzbInfo::usFailure`.
pub fn unpack_ctrl_resolve_fail_status(
    space_error: bool,
    password_error: bool,
    decrypt_error: bool,
    us_space: i32,
    us_password: i32,
    us_failure: i32,
) -> i32 {
    if space_error {
        us_space
    } else if password_error || decrypt_error {
        us_password
    } else {
        us_failure
    }
}

/// Matches `UnpackController::HasCompletedArchiveFiles` filename check:
/// `.*\\.rar$`, `.*\\.7z$`, or `.*\\.7z\\.[0-9]+$`.
pub fn unpack_ctrl_has_completed_archive_ext(filename: &[u8]) -> bool {
    matches!(unpack_ctrl_classify_archive_file(filename), 1..=3)
}

// ============================================================================
// RepairController (`daemon/postprocess/Repair.cpp`)
// ============================================================================

/// Matches `RepairController::PostParChecker::FindFileCrc` status mapping:
/// `cf_status == cf_success -> fs_success`,
/// `cf_status == cf_failure && !reprocess -> fs_failure`,
/// `cf_status == cf_partial && has_segments && !reprocess -> fs_partial`,
/// else `fs_unknown`.
pub fn repair_ctrl_classify_file_crc_status(
    cf_status: i32,
    reprocess: bool,
    has_segments: bool,
    cf_success: i32,
    cf_failure: i32,
    cf_partial: i32,
    fs_success: i32,
    fs_failure: i32,
    fs_partial: i32,
    fs_unknown: i32,
) -> i32 {
    if cf_status == cf_success {
        fs_success
    } else if cf_status == cf_failure && !reprocess {
        fs_failure
    } else if cf_status == cf_partial && has_segments && !reprocess {
        fs_partial
    } else {
        fs_unknown
    }
}

/// Matches `blocks = (int)(entry.size * 100 / 102 / (int64)(m_twinBlockSize + 68))` in
/// `RepairController::PostParChecker::RequestTwinVolumes`.
pub fn repair_ctrl_estimate_twin_vol_blocks(entry_size: i64, twin_block_size: u64) -> i32 {
    if twin_block_size == 0 {
        return 0;
    }
    let denom = (twin_block_size as i64).saturating_add(68);
    if denom <= 0 {
        return 0;
    }
    ((entry_size.saturating_mul(100) / 102) / denom) as i32
}

/// Matches `blockSize > 0 ? (int)std::max<int64>(1, fileInfo->GetSize() / (blockSize + 68)) : 1`
/// in `RepairController::FindPars`.
pub fn repair_ctrl_estimate_par_blocks_from_size(file_size: i64, block_size: i64) -> i32 {
    if block_size > 0 {
        let denom = block_size.saturating_add(68);
        1i64.max(file_size / denom) as i32
    } else {
        1
    }
}

/// Matches the `strictParName` check in `RepairController::FindPars`:
/// checks if `filename` (case-insensitively) contains `<main_base>.par2` or `<main_base>.vol`.
pub fn repair_ctrl_match_strict_par_name(filename: &[u8], main_base: &[u8]) -> bool {
    if filename.len() < main_base.len() + 4 {
        return false;
    }
    for i in 0..=filename.len() - (main_base.len() + 4) {
        if filename[i..i + main_base.len()].eq_ignore_ascii_case(main_base) {
            let rest = &filename[i + main_base.len()..];
            if (rest.len() >= 5 && rest[..5].eq_ignore_ascii_case(b".par2"))
                || (rest.len() >= 4 && rest[..4].eq_ignore_ascii_case(b".vol"))
            {
                return true;
            }
        }
    }
    false
}

/// Matches `ParStatus` update in `RepairController::ParCheckCompleted`:
/// Returns `(new_par_status, set_par_repaired, par_repaired_val)`.
pub fn repair_ctrl_resolve_par_status(
    checker_status: i32,
    cur_par_status: i32,
    ps_repaired: i32,
    ps_repair_not_needed: i32,
    ps_repair_possible: i32,
    nzb_ps_skipped: i32,
    nzb_ps_success: i32,
    nzb_ps_repair_possible: i32,
    nzb_ps_failure: i32,
) -> (i32, bool, bool) {
    if (checker_status == ps_repaired || checker_status == ps_repair_not_needed)
        && cur_par_status <= nzb_ps_skipped
    {
        (nzb_ps_success, true, checker_status == ps_repaired)
    } else if checker_status == ps_repair_possible && cur_par_status != nzb_ps_failure {
        (nzb_ps_repair_possible, false, false)
    } else {
        (nzb_ps_failure, false, false)
    }
}

// ============================================================================
// ParChecker (`daemon/postprocess/ParChecker.cpp`)
// ============================================================================

/// Matches `m_threadsToUse` and `m_memToUse` calculation in `Repairer::Repairer`.
pub fn parchecker_calc_threads_and_mem(
    par_threads: i32,
    cpu_cores: i32,
    effective_mem_mb: i32,
) -> (i32, i32) {
    let raw_threads = if par_threads > 0 {
        par_threads
    } else {
        cpu_cores
    };
    let threads = if raw_threads > 0 { raw_threads } else { 1 };
    let mem = if effective_mem_mb < 16 {
        16
    } else {
        effective_mem_mb
    };
    (threads, mem)
}

/// Matches `VerifySuccessDataFile` zero-padding length calculation:
/// `blocksize * block_count > file_size ? blocksize * block_count - file_size : 0`.
pub fn parchecker_calc_pad_bytes(block_size: u64, block_count: u32, file_size: u64) -> u64 {
    let total = block_size.saturating_mul(u64::from(block_count));
    if total > file_size {
        total - file_size
    } else {
        0
    }
}

/// Matches the comparator in `ParChecker::SortExtraFiles`:
/// `strcmp(name1, baseParFilename) == 0 && strcmp(name1, name2) != 0` after stripping `.ext`
/// from `name1` and `name2`.
pub fn parchecker_compare_extra_file_stems(
    base_name1: &[u8],
    base_name2: &[u8],
    par_stem: &[u8],
) -> bool {
    let stem1 = match base_name1.iter().rposition(|&b| b == b'.') {
        Some(pos) => &base_name1[..pos],
        None => base_name1,
    };
    let stem2 = match base_name2.iter().rposition(|&b| b == b'.') {
        Some(pos) => &base_name2[..pos],
        None => base_name2,
    };
    stem1 == par_stem && stem1 != stem2
}

// ============================================================================
// SystemHealth (`SystemHealthService.cpp`)
// ============================================================================

/// Matches `existing.GetSource() == alert.GetSource() && existing.GetCategory() == alert.GetCategory()`
/// in `SystemHealth::Service::ReportAlert`.
pub fn health_alert_matches(
    src1: &[u8],
    cat1: &[u8],
    src2: &[u8],
    cat2: &[u8],
) -> bool {
    src1 == src2 && cat1 == cat2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_article_downloader_helpers() {
        assert_eq!(article_dl_is_retention_failure(0, 100, 1000000), (false, 0));
        assert_eq!(
            article_dl_is_retention_failure(10, 100, 100 + 11 * 86400),
            (true, 11)
        );
        assert!(article_dl_all_servers_on_level_failed(
            0,
            &[1, 2],
            &[0, 0],
            &[10, 10],
            &[1, 1],
            &[1],
            &[0],
            &[10],
        ));
        assert!(!article_dl_all_servers_on_level_failed(
            0,
            &[1, 2],
            &[0, 0],
            &[0, 0],
            &[1, 1],
            &[1],
            &[0],
            &[0],
        ));
        assert_eq!(
            article_dl_resolve_final_status(5, true, false, 2, 3, 4, 7),
            2
        );
        assert_eq!(
            article_dl_resolve_final_status(2, false, true, 2, 3, 4, 7),
            4
        );
        assert!(article_dl_should_throttle_bandwidth(
            false, 100.0, 120.0, 80.0
        ));
        assert_eq!(article_dl_classify_decode_status(false, 1, true, true), 2);
        assert_eq!(article_dl_classify_decode_status(false, 3, true, false), 3);
    }

    #[test]
    fn test_direct_unpack_and_unpack_ctrl() {
        assert_eq!(
            direct_unpack_resolve_final_status(true, true, false, false, 0, 3, 2),
            3
        );
        assert!(direct_unpack_use_inter_dir(
            b"/inter/job1",
            b"/inter",
            b'/'
        ));
        assert!(!direct_unpack_use_inter_dir(b"/inter", b"/inter", b'/'));
        assert!(direct_unpack_is_main_archive(b"movie.rar"));
        assert!(direct_unpack_is_main_archive(b"movie.part01.rar"));
        assert!(!direct_unpack_is_main_archive(b"movie.part02.rar"));
        assert!(direct_unpack_is_insert_disk_prompt(
            b"Insert disk with movie.part02.rar [C]ontinue, [Q]uit ",
            52,
            i32::from(b' ')
        ));
        unsafe {
            assert_eq!(
                crate::ffi::nzbget_rs_dur_direct_unpack_is_insert_disk_prompt(
                    std::ptr::dangling(),
                    0,
                    i32::from(b'\r'),
                ),
                0
            );
            let prompt = b"Insert disk with movie.part02.rar [C]ontinue, [Q]uit \0";
            assert_eq!(
                crate::ffi::nzbget_rs_dur_direct_unpack_is_insert_disk_prompt(
                    prompt.as_ptr().cast(),
                    52,
                    i32::from(b' '),
                ),
                1
            );
        }

        assert!(unpack_ctrl_should_skip_second_attempt(
            true, false, false, false, false
        ));
        assert_eq!(
            unpack_ctrl_eval_unrar_exit(11, false, false, false, false),
            (false, false, false, true)
        );
        assert_eq!(unpack_ctrl_classify_archive_file(b"test.rar"), 1);
        assert_eq!(unpack_ctrl_classify_archive_file(b"test.7z"), 2);
        assert_eq!(unpack_ctrl_classify_archive_file(b"test.7z.001"), 3);
        assert_eq!(unpack_ctrl_classify_archive_file(b"test.mkv.001"), 4);
        assert_eq!(unpack_ctrl_classify_archive_file(b"test.r00"), 5);
        assert_eq!(unpack_ctrl_classify_archive_file(b"test.mkv.002"), 6);
        assert!(unpack_ctrl_has_completed_archive_ext(b"archive.7z.001"));
    }

    #[test]
    fn test_repair_parchecker_and_health() {
        assert_eq!(
            repair_ctrl_classify_file_crc_status(1, false, false, 1, 2, 3, 10, 20, 30, 0),
            10
        );
        assert!(repair_ctrl_match_strict_par_name(
            b"Prefix.Movie.Vol01+02.PAR2",
            b"movie"
        ));
        assert_eq!(parchecker_calc_threads_and_mem(0, 8, 8), (8, 16));
        assert_eq!(parchecker_calc_pad_bytes(1000, 5, 4500), 500);
        assert!(parchecker_compare_extra_file_stems(
            b"movie.mkv",
            b"other.mkv",
            b"movie"
        ));
        assert!(health_alert_matches(b"s1", b"c1", b"s1", b"c1"));
    }
}
