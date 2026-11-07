//! End-to-end Rust implementation of `RepairController` (`daemon/postprocess/Repair.cpp`).
//!
//! Replaces piecemeal FFI predicates and micro-helpers with cohesive domain functions
//! for PAR2 recovery volume selection, twin PAR2 index/volume evaluation, file CRC
//! status resolution, progress/stage transitions, and pause timestamp adjustment.

use std::ffi::CStr;

pub use crate::postrepair::{check_par_time_limit, pick_twin_volume, select_more_pars};

/// Case-insensitive filename equality check (`!strcasecmp(a, b)`),
/// used by `RepairController::PostParChecker::IsParredFile`, `FindFileCrc`, and `FindFileOrigname`.
pub fn filename_eq_ignore_case(a: &CStr, b: &CStr) -> bool {
    a.to_bytes().eq_ignore_ascii_case(b.to_bytes())
}

/// Evaluates whether `FindFileCrc` should load partial article segment state from disk
/// (`status == cf_partial && id > 0 && !reprocess`).
pub fn should_load_partial_segments(
    status: i32,
    cf_partial: i32,
    id: i32,
    reprocess: bool,
) -> bool {
    status == cf_partial && id > 0 && !reprocess
}

/// Resolves the final `ParChecker::EFileStatus` returned by `RepairController::PostParChecker::FindFileCrc`.
#[allow(clippy::too_many_arguments)]
pub fn classify_file_crc_status(
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

/// Formats the `_twin.` filename for `RepairController::PostParChecker::FetchTwinFile`.
pub fn build_twin_filename(entry_filename: &[u8], twin_files_count: usize) -> Vec<u8> {
    let mut out = b"_twin.".to_vec();
    if entry_filename.is_empty() {
        out.extend_from_slice(format!("{twin_files_count}.par2").as_bytes());
    } else {
        out.extend_from_slice(entry_filename);
    }
    out
}

/// Checks in-memory candidate eligibility in `RepairController::PostParChecker::RequestTwinIndex`
/// before checking on-disk file existence.
pub fn should_consider_twin_candidate(
    is_self: bool,
    kind: i32,
    nk_nzb: i32,
    same_name_or_key: bool,
    queued_filename: &[u8],
    has_print: bool,
    own_print_matches: bool,
) -> bool {
    if is_self || kind != nk_nzb || !same_name_or_key || queued_filename.contains(&b'|') {
        return false;
    }
    if has_print && !own_print_matches {
        return false;
    }
    true
}

/// Selects the smallest `.par2` entry index within `max_index_size` for
/// `RepairController::PostParChecker::RequestTwinIndex` and `TwinCheckJob::IndexData`.
pub fn select_best_twin_par2_index(
    is_par2: &[u8],
    sizes: &[i64],
    max_index_size: i64,
) -> Option<usize> {
    let n = is_par2.len().min(sizes.len());
    let mut best_idx: Option<usize> = None;
    let mut best_size: i64 = 0;
    for i in 0..n {
        if is_par2[i] != 0
            && sizes[i] <= max_index_size
            && (best_idx.is_none() || sizes[i] < best_size)
        {
            best_idx = Some(i);
            best_size = sizes[i];
        }
    }
    best_idx
}

/// Returns the NUL-terminated rejection reason (`"describes other files"` vs
/// `"names the files otherwise"`) for a twin PAR2 set in `RequestTwinIndex`.
pub fn twin_par2_reject_reason(cmp: i32) -> &'static [u8] {
    if cmp == 1 {
        b"describes other files\0"
    } else {
        b"names the files otherwise\0"
    }
}

/// Returns whether a twin NZB entry should be skipped while collecting recovery volumes
/// in `RepairController::PostParChecker::RequestTwinVolumes`.
pub fn should_skip_twin_volume_entry(
    is_par2: bool,
    entry_filename: &[u8],
    twin_index: &[u8],
) -> bool {
    !is_par2 || entry_filename == twin_index
}

/// Estimates recovery block count from encoded NZB entry size and twin block size in
/// `RepairController::PostParChecker::RequestTwinVolumes`.
pub fn estimate_twin_vol_blocks(entry_size: i64, twin_block_size: u64) -> i32 {
    if twin_block_size == 0 {
        return 0;
    }
    let denom = (twin_block_size as i64).saturating_add(68);
    if denom <= 0 {
        return 0;
    }
    ((entry_size.saturating_mul(100) / 102) / denom) as i32
}

/// Sums available twin volume blocks and checks whether they cover `block_needed`
/// in `RepairController::PostParChecker::RequestTwinVolumes`.
pub fn sum_and_check_twin_blocks(vol_blocks: &[i32], block_needed: i32) -> (i32, bool) {
    let available: i32 = vol_blocks.iter().copied().sum();
    (available, available >= block_needed)
}

/// Determines whether `ParQuick` should be used in `RepairController::Run`.
pub fn should_use_par_quick(
    par_quick_opt: bool,
    force_par_full: bool,
    dupe_repaired: bool,
) -> bool {
    par_quick_opt && !force_par_full && !dupe_repaired
}

/// Evaluates `ParCheckCompleted` status and elapsed time updates in one call:
/// returns `(new_par_status, set_par_repaired, par_repaired_val, wait_time, par_sec)`.
#[allow(clippy::too_many_arguments)]
pub fn complete_par_check(
    checker_status: i32,
    cur_par_status: i32,
    ps_repaired: i32,
    ps_repair_not_needed: i32,
    ps_repair_possible: i32,
    nzb_ps_skipped: i32,
    nzb_ps_success: i32,
    nzb_ps_repair_possible: i32,
    nzb_ps_failure: i32,
    cur_dl_sec: i32,
    start_dl_sec: i32,
    cur_time: i64,
    par_time: i64,
) -> (i32, bool, bool, i32, i32) {
    let (new_par_status, set_par_repaired, par_repaired_val) = if (checker_status == ps_repaired
        || checker_status == ps_repair_not_needed)
        && cur_par_status <= nzb_ps_skipped
    {
        (nzb_ps_success, true, checker_status == ps_repaired)
    } else if checker_status == ps_repair_possible && cur_par_status != nzb_ps_failure {
        (nzb_ps_repair_possible, false, false)
    } else {
        (nzb_ps_failure, false, false)
    };
    let wait_time = cur_dl_sec.wrapping_sub(start_dl_sec);
    let par_sec = (cur_time.wrapping_sub(par_time) as i32).wrapping_sub(wait_time);
    (
        new_par_status,
        set_par_repaired,
        par_repaired_val,
        wait_time,
        par_sec,
    )
}

/// Evaluates a candidate `FileInfo` in `RepairController::FindPars`:
/// estimates blocks from `file_size` and `block_size` when `parsed && block_count < 0`,
/// checks whether the candidate is a usable PAR block file (`parsed && effective_blocks > 0`),
/// and checks `strict_par_name` when `!exact_par_name && strict_par_name`.
/// Returns `(is_usable, effective_blocks, matches_strict_name)`.
pub fn eval_find_pars_candidate(
    parsed: bool,
    raw_block_count: i32,
    file_size: i64,
    block_size: i64,
    exact_par_name: bool,
    strict_par_name: bool,
    filename: &[u8],
    main_base: &[u8],
) -> (bool, i32, bool) {
    let effective_blocks = if parsed && raw_block_count < 0 {
        if block_size > 0 {
            let denom = block_size.saturating_add(68);
            1i64.max(file_size / denom) as i32
        } else {
            1
        }
    } else {
        raw_block_count
    };
    let is_usable = parsed && effective_blocks > 0;
    let matches_strict = if !is_usable || exact_par_name || !strict_par_name {
        true
    } else {
        match_strict_par_name(filename, main_base)
    };
    (is_usable, effective_blocks, matches_strict)
}

/// Checks if `filename` (case-insensitively) contains `<main_base>.par2` or `<main_base>.vol`.
pub fn match_strict_par_name(filename: &[u8], main_base: &[u8]) -> bool {
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

/// Evaluates `UpdateParCheckProgress` stage mapping, stage transition, repair time accumulation,
/// and repair time-limit cancellation in one call:
/// returns `(new_post_stage, stage_changed, set_repair_time, new_repair_sec, cancel_repair, estimated_repair_sec, estimated_repair_min)`.
#[allow(clippy::too_many_arguments)]
pub fn eval_par_progress_update(
    par_stage: i32,
    cur_post_stage: i32,
    pt_loading_pars: i32,
    pt_verifying_sources: i32,
    pt_repairing: i32,
    pt_verifying_repaired: i32,
    current_time: i64,
    stage_time: i64,
    repair_time: i64,
    cur_repair_sec: i32,
    is_stopped: bool,
    par_time_limit_min: i32,
    is_par_repairing_stage: bool,
    stage_progress: i32,
) -> (i32, bool, bool, i32, bool, i32, i32) {
    let new_post_stage = match par_stage {
        0 => pt_loading_pars,
        1 => pt_verifying_sources,
        2 => pt_repairing,
        3 => pt_verifying_repaired,
        _ => pt_loading_pars,
    };
    let stage_changed = cur_post_stage != new_post_stage;
    let set_repair_time = stage_changed && new_post_stage == pt_repairing;
    let new_repair_sec = if stage_changed && new_post_stage == pt_verifying_repaired {
        cur_repair_sec + (current_time - repair_time) as i32
    } else {
        cur_repair_sec
    };
    let effective_stage_time = if stage_changed {
        current_time
    } else {
        stage_time
    };
    let (cancel_repair, estimated_sec, estimated_min) = if !is_stopped {
        if let Some(est_sec) = check_par_time_limit(
            par_time_limit_min,
            is_par_repairing_stage,
            current_time - effective_stage_time,
            stage_progress,
        ) {
            (true, est_sec, est_sec / 60)
        } else {
            (false, 0, 0)
        }
    } else {
        (false, 0, 0)
    };
    (
        new_post_stage,
        stage_changed,
        set_repair_time,
        new_repair_sec,
        cancel_repair,
        estimated_sec,
        estimated_min,
    )
}

/// Computes shifted timestamps `(stageTime + delta, startTime + delta, parTime + delta, repairTime + delta)`
/// for positive timestamps during `RepairController::CheckPauseState`.
pub fn calc_paused_timestamps(
    cur_time: i64,
    wait_time: i64,
    stage_time: i64,
    start_time: i64,
    par_time: i64,
    repair_time: i64,
) -> (i64, i64, i64, i64) {
    let delta = cur_time.wrapping_sub(wait_time);
    (
        if stage_time > 0 {
            stage_time.wrapping_add(delta)
        } else {
            0
        },
        if start_time > 0 {
            start_time.wrapping_add(delta)
        } else {
            0
        },
        if par_time > 0 {
            par_time.wrapping_add(delta)
        } else {
            0
        },
        if repair_time > 0 {
            repair_time.wrapping_add(delta)
        } else {
            0
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repair_twin_and_find_pars() {
        assert_eq!(
            build_twin_filename(b"show.vol01+02.par2", 2),
            b"_twin.show.vol01+02.par2"
        );
        assert_eq!(build_twin_filename(b"", 4), b"_twin.4.par2");

        let is_par2 = [0u8, 1, 1, 1];
        let sizes = [100i64, 50_000, 12_000, 2_000_000];
        assert_eq!(
            select_best_twin_par2_index(&is_par2, &sizes, 1_000_000),
            Some(2)
        );

        assert!(match_strict_par_name(b"rel.vol00+01.PAR2", b"rel"));
        assert!(!match_strict_par_name(b"other.par2", b"rel"));

        let (usable, blocks, strict_ok) = eval_find_pars_candidate(
            true,
            -1,
            500_000,
            100_000,
            false,
            true,
            b"movie.vol01-04.par2",
            b"movie",
        );
        assert!(usable);
        assert_eq!(blocks, 4);
        assert!(strict_ok);
    }

    #[test]
    fn test_complete_par_check_and_progress() {
        let (status, set_rep, rep_val, wait, par_sec) =
            complete_par_check(1, 0, 1, 2, 3, 1, 2, 3, 4, 30, 10, 1100, 1000);
        assert_eq!(status, 2);
        assert!(set_rep);
        assert!(rep_val);
        assert_eq!(wait, 20);
        assert_eq!(par_sec, 80);

        let (new_stage, changed, set_rt, new_rsec, cancel, est_sec, est_min) =
            eval_par_progress_update(2, 1, 10, 20, 30, 40, 2000, 1000, 0, 5, false, 10, false, 500);
        assert_eq!(new_stage, 30);
        assert!(changed);
        assert!(set_rt);
        assert_eq!(new_rsec, 5);
        assert!(!cancel);
        assert_eq!((est_sec, est_min), (0, 0));
    }
}
