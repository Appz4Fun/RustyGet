//! End-to-end Rust implementation of `ParChecker` (`daemon/postprocess/ParChecker.cpp`).
//!
//! Consolidates PAR2 verification, block-range CRC computation, extra/split/dupe/twin
//! recovery orchestration, CLI flag construction, and progress/status state machines
//! into a single idiomatic module.

use crate::postrepair::{strncasecmp_prefix, NzbgetRsParSegment};

pub const PAR_STAGE_LOADING_PARS: i32 = 0;
pub const PAR_STAGE_VERIFYING_SOURCES: i32 = 1;
pub const PAR_STAGE_REPAIRING: i32 = 2;
pub const PAR_STAGE_VERIFYING_REPAIRED: i32 = 3;

/// Computes `(threads_to_use, mem_to_use_mb)` for `Repairer::Repairer`.
pub fn calc_repairer_resources(
    par_threads: i32,
    cpu_cores: i32,
    has_memory_monitor: bool,
    monitor_mem_mb: i32,
    configured_par_buffer_mb: i32,
) -> (i32, i32) {
    let effective_mem_mb = if has_memory_monitor {
        monitor_mem_mb
    } else if configured_par_buffer_mb > 0 {
        configured_par_buffer_mb
    } else {
        16
    };
    let raw_threads = if par_threads > 0 {
        par_threads
    } else {
        cpu_cores
    };
    let threads = if raw_threads > 0 { raw_threads } else { 1 };
    let mem = effective_mem_mb.max(16);
    (threads, mem)
}

/// Formats `-m<mem>`, `-t<threads>`, `-T<threads>`, and optional `<dir>/*` wildcard parameter
/// (preserving `BString<1024>` 1023-byte truncation before `BaseFileName`) for `Repairer::PreProcess`.
pub fn build_preprocess_cli_args(
    threads_to_use: i32,
    mem_to_use: i32,
    par_filename: &[u8],
    par_scan_full: bool,
) -> (Vec<u8>, Vec<u8>, Vec<u8>, Option<Vec<u8>>) {
    let threads_str = threads_to_use.to_string();
    let mem_param = format!("-m{mem_to_use}").into_bytes();
    let threads_param = format!("-t{threads_str}").into_bytes();
    let file_threads_param = format!("-T{threads_str}").into_bytes();
    let wildcard_param = if par_scan_full {
        let truncated = &par_filename[..par_filename.len().min(1023)];
        let base_idx = crate::paths::base_file_name(truncated);
        if base_idx > 0 && base_idx < truncated.len() {
            let mut out = truncated[..base_idx].to_vec();
            out.push(b'*');
            Some(out)
        } else {
            None
        }
    } else {
        None
    };
    (mem_param, threads_param, file_threads_param, wildcard_param)
}

/// `ParChecker::StreamBuf::overflow` line classifier:
/// Returns `(extra_debug, msg_offset)`.
pub fn classify_streambuf_line(msg: &[u8]) -> (bool, usize) {
    let extra_debug = msg.contains(&b'%')
        || msg.starts_with(b"Loading")
        || (msg.starts_with(b"Target: ") && !msg.ends_with(b"found"));
    let offset = if msg.starts_with(b"You have ") { 9 } else { 0 };
    (extra_debug, offset)
}

/// Returns the NUL-terminated error message for `Par2CmdLineErrStr[res]`.
pub fn cmdline_err_str(res: i32) -> &'static [u8] {
    match res {
        0 => b"OK\0",
        1 => b"data files are damaged and there is enough recovery data available to repair them\0",
        2 => b"data files are damaged and there is insufficient recovery data available to be able to repair them\0",
        3 => b"there was something wrong with the command line arguments\0",
        4 => b"the PAR2 files did not contain sufficient information about the data files to be able to verify them\0",
        5 => b"repair completed but the data files still appear to be damaged\0",
        6 => b"an error occurred when accessing files\0",
        7 => b"internal error occurred\0",
        8 => b"out of memory\0",
        _ => b"internal error occurred\0",
    }
}

/// Returns the NUL-terminated stage message for `Par2StageMessage[stage]`.
pub fn stage_message(stage: i32) -> &'static [u8] {
    match stage {
        0 => b"Loading file\0",
        1 => b"Verifying file\0",
        2 => b"Repairing file\0",
        3 => b"Verifying repaired file\0",
        _ => b"Verifying file\0",
    }
}

/// Formats `"<verb> <info_name>"` (e.g. `"Verifying <info_name>"`, `"Repairing <info_name>"`).
pub fn format_action_label(verb: &[u8], info_name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(verb.len() + 1 + info_name.len());
    out.extend_from_slice(verb);
    out.push(b' ');
    out.extend_from_slice(info_name);
    out
}

/// Evaluates whether `ParChecker::RunParCheck` should attempt an additional recovery step
/// (`AddSplittedFragments`, `ProcessMorePars`, `AddDupeFiles`, or `ProcessTwinSet`).
pub fn should_try_more_recovery(
    has_damaged_files: bool,
    is_stopped: bool,
    res: i32,
    e_repair_not_possible: i32,
    require_dupe_scan: bool,
    par_scan: i32,
    ps_dupe: i32,
) -> bool {
    has_damaged_files
        && !is_stopped
        && res == e_repair_not_possible
        && (!require_dupe_scan || par_scan == ps_dupe)
}

/// Decides whether `ParChecker::RunParCheck` should scan for missing files (`AddMissingFiles`).
#[allow(clippy::too_many_arguments)]
pub fn should_add_missing_files(
    has_damaged: bool,
    is_stopped: bool,
    missing_file_count: u32,
    added_split: bool,
    res: i32,
    e_repair_possible: i32,
    par_scan: i32,
    ps_extended: i32,
    ps_dupe: i32,
) -> bool {
    has_damaged
        && !is_stopped
        && missing_file_count > 0
        && !(added_split && res == e_repair_possible)
        && (par_scan == ps_extended || par_scan == ps_dupe)
}

/// Evaluates the post-verify status and repair action in `ParChecker::RunParCheck`.
/// Returns `(status, action_code)`:
/// - `0`: repair not needed
/// - `1`: repair possible and `ParRepair` enabled (caller runs repair pass)
/// - `2`: repair possible and `ParRepair` disabled
/// - `3`: repair failed
#[allow(clippy::too_many_arguments)]
pub fn eval_post_verify_action(
    res: i32,
    e_success: i32,
    e_repair_possible: i32,
    has_damaged_files: bool,
    par_repair: bool,
    ps_failed: i32,
    ps_repair_not_needed: i32,
    ps_repair_possible: i32,
) -> (i32, i32) {
    if res == e_success || !has_damaged_files {
        (ps_repair_not_needed, 0)
    } else if res == e_repair_possible {
        if par_repair {
            (ps_repair_possible, 1)
        } else {
            (ps_repair_possible, 2)
        }
    } else {
        (ps_failed, 3)
    }
}

/// Evaluates cancellation or failure outcome at the end of `ParChecker::RunParCheck`.
/// Returns `(final_status, outcome_kind, use_cmdline_err)`:
/// - `0`: normal completion
/// - `1`: repair cancelled (`stage >= pt_repairing`)
/// - `2`: par-check cancelled (`stage < pt_repairing`)
/// - `3`: repair failed (`cur_status == ps_failed`)
#[allow(clippy::too_many_arguments)]
pub fn eval_cancel_or_fail_outcome(
    is_stopped: bool,
    stage: i32,
    pt_repairing: i32,
    cur_status: i32,
    ps_failed: i32,
    ps_repair_possible: i32,
    err_msg_empty: bool,
    res: i32,
) -> (i32, i32, bool) {
    if is_stopped {
        if stage >= pt_repairing {
            (ps_repair_possible, 1, false)
        } else {
            (ps_failed, 2, false)
        }
    } else if cur_status == ps_failed {
        let use_cmd_err = err_msg_empty && (0..=8).contains(&res);
        (ps_failed, 3, use_cmd_err)
    } else {
        (cur_status, 0, false)
    }
}

/// Classifies the step outcome after `Repairer::PreProcess` in `ParChecker::PreProcessPar`:
/// `1` = stopped, `2` = invalid command line arguments, `3` = failed (request more pars), `0` = success.
pub fn classify_preprocess_step(
    is_stopped: bool,
    res: i32,
    e_success: i32,
    e_invalid_args: i32,
) -> i32 {
    if is_stopped {
        1
    } else if res == e_invalid_args {
        2
    } else if res != e_success {
        3
    } else {
        0
    }
}

/// Formats `"not enough par-blocks, {missing_blocks} block(s) needed, but {block_found} block(s) available"`.
pub fn format_not_enough_blocks_errmsg(missing_blocks: i32, block_found: i32) -> Vec<u8> {
    format!(
        "not enough par-blocks, {missing_blocks} block(s) needed, but {block_found} block(s) available"
    )
    .into_bytes()
}

/// Formats the twin-set PAR error message when `res == e_repair_not_possible` in `ParChecker::ProcessTwinSet`.
pub fn format_twin_err_msg(
    res: i32,
    e_repair_not_possible: i32,
    own_err_msg: &[u8],
) -> Option<Vec<u8>> {
    if res != e_repair_not_possible {
        None
    } else if own_err_msg.is_empty() {
        Some(b"not enough par-blocks, the twin's included".to_vec())
    } else {
        let suffix = b"; the twin's didn't do either";
        let mut out = Vec::with_capacity(own_err_msg.len() + suffix.len());
        out.extend_from_slice(own_err_msg);
        out.extend_from_slice(suffix);
        Some(out)
    }
}

/// `ParChecker::MaybeSplittedFragement`
pub fn maybe_splitted_fragment(filename1: &[u8], filename2: &[u8]) -> bool {
    let len = filename2.len();
    if strncasecmp_prefix(filename1, filename2) {
        if filename1.get(len) == Some(&b'.') {
            if filename1[len + 1..].iter().all(|b| b.is_ascii_digit()) {
                return true;
            }
        }
    }

    let rfind_dot = |s: &[u8]| s.iter().rposition(|&b| b == b'.');
    if let (Some(pos1), Some(pos2)) = (rfind_dot(filename1), rfind_dot(filename2)) {
        let ext1 = &filename1[pos1..];
        let ext2 = &filename2[pos2..];
        if ext1.len() == 4
            && ext2.len() == 4
            && filename2.len() >= pos1
            && strncasecmp_prefix(filename1, &filename2[..pos1])
        {
            if ext1[1..].iter().all(|b| b.is_ascii_digit())
                && ext2[1..].iter().all(|b| b.is_ascii_digit())
            {
                return true;
            }
        }
    }

    false
}

/// Evaluates `ParChecker::AddDupeFiles` block deltas after scanning a dupe directory.
/// Returns `(new_used_blocks, total_extra_blocks_found)`.
pub fn calc_dupe_blocks_delta(
    prev_used_blocks: i32,
    was_blocks_missing2: u32,
    blocks_missing2: u32,
    was_blocks_missing_total: u32,
) -> (i32, u32) {
    let delta2 = was_blocks_missing2.wrapping_sub(blocks_missing2) as i32;
    let new_used = prev_used_blocks.wrapping_add(delta2);
    let total_found = if blocks_missing2 < was_blocks_missing_total {
        was_blocks_missing_total - blocks_missing2
    } else {
        0
    };
    (new_used, total_found)
}

/// Extracts the base filename of `par_filename` with its last `.ext` stripped (`ParChecker::SortExtraFiles`).
pub fn extract_base_par_stem(par_filename: &[u8]) -> Vec<u8> {
    let base = &par_filename[crate::paths::base_file_name(par_filename)..];
    match base.iter().rposition(|&b| b == b'.') {
        Some(dot) => base[..dot].to_vec(),
        None => base.to_vec(),
    }
}

/// Comparator in `ParChecker::SortExtraFiles`:
/// `strcmp(name1, baseParFilename) == 0 && strcmp(name1, name2) != 0` after stripping `.ext`.
pub fn compare_extra_file_stems(base_name1: &[u8], base_name2: &[u8], par_stem: &[u8]) -> bool {
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

/// Evaluates `ParChecker::AddExtraFiles` step outcome and early-abort condition for one extra file.
/// Returns `(file_added, block_added, new_files_added, missing_blocks_delta, register_parred, abort_reason)`:
/// `abort_reason`:
/// - `1`: all missing files found (`only_missing && now_files_missing == 0`)
/// - `2`: all missing blocks found (`!only_missing && now_blocks_missing == 0`)
/// - `0`: continue scanning
pub fn eval_extra_file_step(
    was_files_missing: u32,
    now_files_missing: u32,
    was_blocks_missing: u32,
    now_blocks_missing: u32,
    external_dir: bool,
    prev_files_added: bool,
    only_missing: bool,
) -> (bool, bool, bool, u32, bool, i32) {
    let file_added = was_files_missing > now_files_missing;
    let block_added = was_blocks_missing > now_blocks_missing;
    let new_files_added = prev_files_added || file_added || block_added;
    let blocks_delta = if block_added {
        was_blocks_missing - now_blocks_missing
    } else {
        0
    };
    let register_parred = file_added && !external_dir;
    let abort_reason = if only_missing && now_files_missing == 0 {
        1
    } else if !only_missing && now_blocks_missing == 0 {
        2
    } else {
        0
    };
    (
        file_added,
        block_added,
        new_files_added,
        blocks_delta,
        register_parred,
        abort_reason,
    )
}

/// `ParChecker::SignalProgress` stage-progress calculation.
pub fn calc_stage_progress(
    stage: i32,
    file_progress: i32,
    processed_count: i32,
    files_to_repair: i32,
    source_files_count: i32,
    extra_files: i32,
    quick_files: i32,
) -> i32 {
    let stage_progress = if stage == PAR_STAGE_REPAIRING {
        file_progress
    } else {
        let mut total_files;
        let mut processed_files = processed_count;
        if stage == PAR_STAGE_VERIFYING_REPAIRED {
            total_files = files_to_repair;
        } else {
            total_files = source_files_count + extra_files;
            if extra_files > 0 {
                total_files -= quick_files;
                processed_files -= quick_files;
            }
        }

        if total_files > 0 {
            if file_progress < 1000 {
                (processed_files * 1000 + file_progress) / total_files
            } else {
                processed_files * 1000 / total_files
            }
        } else {
            0
        }
    };

    stage_progress.clamp(0, 1000)
}

/// Evaluates `ParChecker::SignalDone` damaged-file state when `stage == pt_verifying_sources`.
/// Returns `(should_report, new_has_damaged_files, bad_blocks)`.
pub fn eval_signal_done(
    stage: i32,
    pt_verifying_sources: i32,
    available: i32,
    total: i32,
    verifying_extra_files: bool,
    ignore: bool,
    prev_has_damaged_files: bool,
) -> (bool, bool, i32) {
    if stage == pt_verifying_sources && available < total && !verifying_extra_files {
        (
            true,
            prev_has_damaged_files || !ignore,
            total.wrapping_sub(available),
        )
    } else {
        (false, prev_has_damaged_files, 0)
    }
}

/// Evaluates initial preconditions in `ParChecker::VerifyDataFile`:
/// - `0`: proceed with CRC lookup
/// - `1`: return `fsSuccess` (`stage != pt_verifying_sources`)
/// - `2`: return `fsUnknown` (`!target_exists || !has_verification_packet`)
/// - `3`: return `fsFailure` and set `availableBlocks = 0` (`file_size == 0 && block_count > 0`)
pub fn check_verify_data_file_preconditions(
    stage: i32,
    pt_verifying_sources: i32,
    target_exists: bool,
    has_verification_packet: bool,
    file_size: i64,
    block_count: u64,
) -> i32 {
    if stage != pt_verifying_sources {
        return 1;
    }
    if !target_exists || !has_verification_packet {
        return 2;
    }
    if file_size == 0 && block_count > 0 {
        return 3;
    }
    0
}

/// Combines per-block CRCs into a PAR2 block-range or file-level CRC.
pub fn combine_block_crcs(block_crcs: &[u32], block_size: u64) -> u32 {
    let mut par_crc = 0u32;
    let len2 = block_size as u32;
    for (i, &block_crc) in block_crcs.iter().enumerate() {
        par_crc = if i == 0 {
            block_crc
        } else {
            crate::crc::combine(par_crc, block_crc, len2)
        };
    }
    par_crc
}

/// `ParChecker::VerifyPartialDataFile` valid-block determination.
pub fn compute_valid_blocks(
    block_count: usize,
    block_size: i64,
    file_size: i64,
    segments: &[NzbgetRsParSegment],
    out_valid: &mut [u8],
) {
    let n = block_count.min(out_valid.len());
    for i in 0..n {
        let block_start = (i as i64) * block_size;
        let block_end = if block_start + block_size < file_size - 1 {
            block_start + block_size
        } else {
            file_size - 1
        };
        let mut block_ok = false;
        let mut block_end_found = false;
        let mut cur_offset = 0i64;
        for seg in segments {
            let seg_end = seg.offset + (seg.size as i64);
            if !block_ok
                && seg.success != 0
                && seg.offset <= block_start
                && seg_end >= block_start
            {
                block_ok = true;
                cur_offset = seg.offset;
            }
            if block_ok {
                if !(seg.success != 0 && seg.offset == cur_offset) {
                    block_ok = false;
                    break;
                }
                if seg_end >= block_end {
                    block_end_found = true;
                    break;
                }
                cur_offset = seg_end;
            }
        }
        out_valid[i] = u8::from(block_ok && block_end_found);
    }
}

/// Finds the next contiguous `[block_start, block_end]` inclusive run of valid blocks
/// starting at `start_idx` in `ParChecker::VerifyPartialDataFile`, and computes
/// `(block_end, par_crc, bytes_start, clamped_end, pad_len)`.
pub fn next_valid_block_run(
    valid_blocks: &[u8],
    block_crcs: &[u32],
    start_idx: usize,
    block_size: i64,
    file_size: i64,
) -> Option<(usize, u32, i64, i64, usize)> {
    let len = valid_blocks.len().min(block_crcs.len());
    if start_idx >= len {
        return None;
    }
    let rel_start = valid_blocks[start_idx..len].iter().position(|&v| v != 0)?;
    let block_start = start_idx + rel_start;
    let rel_len = valid_blocks[block_start..len]
        .iter()
        .position(|&v| v == 0)
        .unwrap_or(len - block_start);
    let block_end = block_start + rel_len - 1;
    let par_crc = combine_block_crcs(
        &block_crcs[block_start..=block_end],
        block_size as u64,
    );
    let bytes_start = (block_start as i64).wrapping_mul(block_size);
    let bytes_end = (block_end as i64)
        .wrapping_mul(block_size)
        .wrapping_add(block_size)
        .wrapping_sub(1);
    let file_last = file_size.wrapping_sub(1);
    let clamped_end = if bytes_end < file_last {
        bytes_end
    } else {
        file_last
    };
    let pad_len = if bytes_end > file_last {
        bytes_end.wrapping_sub(file_last) as usize
    } else {
        0
    };
    Some((block_end, par_crc, bytes_start, clamped_end, pad_len))
}

/// `ParChecker::SmartCalcFileRangeCrc`
pub fn smart_calc_range_crc<F>(
    start: i64,
    end: i64,
    segments: &[NzbgetRsParSegment],
    mut dumb_calc_cb: F,
) -> Option<u32>
where
    F: FnMut(i64, i64) -> Option<u32>,
{
    let mut download_crc = 0u32;
    let mut started = false;
    for seg in segments {
        let seg_end = seg.offset + (seg.size as i64);
        if !started && seg.offset > start {
            download_crc = dumb_calc_cb(start, seg.offset - 1)?;
            if seg_end >= end {
                break;
            }
            started = true;
        }

        if seg.offset >= start && seg_end <= end {
            download_crc = if !started {
                seg.crc
            } else {
                crate::crc::combine(download_crc, seg.crc, seg.size as u32)
            };
            started = true;
        }

        if seg_end == end {
            break;
        }

        if seg_end > end {
            let partial_crc = dumb_calc_cb(seg.offset, end)?;
            download_crc = crate::crc::combine(
                download_crc,
                partial_crc,
                (end - seg.offset + 1) as u32,
            );
            break;
        }
    }

    Some(download_crc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repairer_resources_and_cli_args() {
        assert_eq!(calc_repairer_resources(0, 8, false, 0, 250), (8, 250));
        assert_eq!(calc_repairer_resources(4, 8, false, 0, 0), (4, 16));
        assert_eq!(calc_repairer_resources(0, 0, true, 8, 250), (1, 16));

        let (m, t, ft, wc) = build_preprocess_cli_args(4, 250, b"/tmp/dir/show.par2", true);
        assert_eq!(m, b"-m250");
        assert_eq!(t, b"-t4");
        assert_eq!(ft, b"-T4");
        assert_eq!(wc.as_deref(), Some(&b"/tmp/dir/*"[..]));
    }

    #[test]
    fn test_post_verify_and_extra_steps() {
        assert_eq!(
            eval_post_verify_action(0, 0, 1, false, true, 0, 2, 1),
            (2, 0)
        );
        assert_eq!(
            eval_post_verify_action(1, 0, 1, true, true, 0, 2, 1),
            (1, 1)
        );
        assert_eq!(
            eval_extra_file_step(2, 1, 10, 5, false, false, false),
            (true, true, true, 5, true, 0)
        );
        assert_eq!(
            eval_extra_file_step(1, 0, 10, 5, false, false, true).5,
            1
        );
    }

    #[test]
    fn test_next_valid_block_run() {
        let valid = [0u8, 1, 1, 0, 1];
        let crcs = [0x11u32, 0x22, 0x33, 0x44, 0x55];
        let (end, _crc, bs, ce, pad) =
            next_valid_block_run(&valid, &crcs, 0, 1000, 4500).unwrap();
        assert_eq!(end, 2);
        assert_eq!(bs, 1000);
        assert_eq!(ce, 2999);
        assert_eq!(pad, 0);

        let (end2, _crc2, bs2, ce2, pad2) =
            next_valid_block_run(&valid, &crcs, 3, 1000, 4500).unwrap();
        assert_eq!(end2, 4);
        assert_eq!(bs2, 4000);
        assert_eq!(ce2, 4499);
        assert_eq!(pad2, 500);
    }
}
