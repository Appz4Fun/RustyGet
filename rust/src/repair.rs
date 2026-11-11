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

// ============================================================================
// Stateful RepairController Coordinator (`NzbgetRsRepairController`)
// ============================================================================

use std::ffi::{c_char, c_int, c_void};
use std::path::Path;
use std::ptr;
use std::time::Duration;

use crate::articlefetcher::{fetch_entry, ArticleFetcher, NzbgetRsNntpFetchHostFn};
use crate::twin::{
    block_size as par2_block_size, compare_twin_par2_data, fingerprint as twin_fingerprint,
    par2_sigs, read_nzb, volume_blocks_bytes, NzbEntry, MAX_INDEX_SIZE,
};

const MK_INFO: c_int = 0;
const MK_WARNING: c_int = 1;
const MK_ERROR: c_int = 2;
const MK_DETAIL: c_int = 4;

const MAX_TWIN_FETCH_SIZE: i64 = 1024 * 1024 * 1024;

/// Host callback contract between [`NzbgetRsRepairController`] and `RepairController` in `Util.h`:
/// - `op == 0`: acquire `DownloadQueue::Guard()` and invoke `reinterpret_cast<void(*)(uintptr_t, uintptr_t)>(p1)(p2, dq_ptr)`
/// - `op == 1`: inspect controller / `m_parChecker` / `m_postInfo` / `g_Options` / `g_WorkState`:
///   - `ints[0..12]`: `[checker_status, checker_stage, file_progress, stage_progress, par_full, is_stopped, par_quick_opt, par_time_limit, pause_post_process, force_par_full, force_repair, post_stage]`
///   - `i64s[0..4]`: `[cur_time, block_size, post_stage_time, post_start_time]`
///   - `strs[0..5]`: `[progress_label, info_name, par_filename, checker_dest_dir, unrar_cmd]`
///   - returns `reinterpret_cast<uintptr_t>(m_postInfo->GetNzbInfo())`
/// - `op == 2`: inspect `NzbInfo*` at `p1`:
///   - `ints[0..14]`: `[id, kind, nk_nzb, download_sec, par_sec, repair_sec, par_status, reprocess, force_priority, extra_par_blocks, file_list_len, completed_len, jobs_len, dupe_mode]`
///   - `i64s[0..2]`: `[dupe_recovered_bytes, expected_data_size]`
///   - `strs[0..5]`: `[name, dest_dir, dupe_key, queued_filename, twin_files_param]`
/// - `op == 3`: configure and run `m_parChecker` (`s1=dest_dir`, `s2=nzb_name`, `a1=par_quick`, `a2=force_repair`)
/// - `op == 4`: `AddPar` helper:
///   - `a1 == 0`: inspect `FileInfo*` `p1` -> `strs[0]=dest_dir`, `strs[1]=filename`, returns `1` if `sameCollection`
///   - `a1 == 1`: `m_parChecker.AddParFile(s1)`
///   - `a1 == 2`: `m_parChecker.QueueChanged()`
/// - `op == 5`: apply `ParCheckCompleted` on `NzbInfo* p1`:
///   - `ints[0..6]`: `[new_par_status, set_repaired, repaired_val, wait_time, par_sec, par_full]`
/// - `op == 6`: `FileInfo` in `NzbInfo* p1` at index `a1`:
///   - `a2 == 0`: inspect -> `ints[0..3]=[confirmed, paused, is_par_file]`, `i64s[0]=size`, `strs[0]=filename`, returns `file_ptr`
///   - `a2 == 1`: unpause -> `SetPaused(false); SetExtraPriority(true);`
/// - `op == 7`: message / debug / stop:
///   - `a1 == 0..=4`: `m_postInfo->GetNzbInfo()->AddMessage((Message::EKind)a1, s1)`
///   - `a1 == 5`: `debug("%s", s1)`
///   - `a1 == 6`: `c->Stop()`
/// - `op == 8`: apply progress / pause updates to `m_postInfo`:
///   - `a1 == 0`: `if (s1) SetProgressLabel(s1)`, `SetFileProgress(ints[0])`, `SetStageProgress(ints[1])`,
///     `if (ints[2] & 1) { SetStage(ints[3]); SetStageTime(i64s[0]); }`,
///     `if (ints[2] & 8) { GetNzbInfo()->SetRepairSec(ints[4]); }`
///   - `a1 == 1`: `if (i64s[0] > 0) SetStageTime(i64s[0]); if (i64s[1] > 0) SetStartTime(i64s[1]);`
/// - `op == 9`: `ParredFiles`, `CompletedFiles`, `StreamRepairJobs`, and `DiskState::LoadFileState`:
///   - `a1 == 1`: returns `m_postInfo->GetParredFiles()->size()`
///   - `a1 == 2`: `strs[0] = (*m_postInfo->GetParredFiles())[a2]`
///   - `a1 == 3`: inspect `CompletedFile` at `a2` on `NzbInfo* p1` -> `ints[0..3]=[id, status, crc_i32]`, `strs[0]=filename`, `strs[1]=origname`
///   - `a1 == 4`: inspect `StreamRepairJob` at `a2` on `NzbInfo* p1` -> `ints[0]=file_id`, `strs[0]=filename`
///   - `a1 == 5`: erase `StreamRepairJob` at `a2` on `NzbInfo* p1` and `nzb->SetChanged(true)`
///   - `a1 == 6`: `LoadFileState` for `file_id = a2` into `SegmentList* p1` -> returns `0` on load failure, `1` if loaded empty, `2` if loaded non-empty
/// - `op == 10`: `DownloadQueue*` / `DupeSourceList*` helpers:
///   - `a1 == 0`: `DownloadQueue* p1` -> `ints[0]=queue_len`, `ints[1]=history_len`
///   - `a1 == 1`: returns `NzbInfo*` at queue index `a2` on `DownloadQueue* p1`
///   - `a1 == 2`: returns `NzbInfo*` at history index `a2` on `DownloadQueue* p1` if `hkNzb`, else `0`
///   - `a1 == 5`: `DupeSourceList* p1` -> `emplace_back(a2, s1)`
///   - `a1 == 6`: `DupeSourceList* p1` -> if `a2 < 0` returns `size()`, else `ints[0]=m_id`, `ints[1]=m_usedBlocks`
///   - `a1 == 7`: `NzbInfo* p1` -> `SetExtraParBlocks(a2)`
pub type NzbgetRsRepairCtrlHostFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    s2: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize;

#[inline]
fn to_c_buf(bytes: &[u8]) -> Vec<u8> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let mut out = Vec::with_capacity(end + 1);
    out.extend_from_slice(&bytes[..end]);
    out.push(0);
    out
}

#[inline]
fn to_c_msg_1024(bytes: &[u8]) -> Vec<u8> {
    let cap = bytes.len().min(1023);
    let end = bytes[..cap].iter().position(|&b| b == 0).unwrap_or(cap);
    let mut out = Vec::with_capacity(end + 1);
    out.extend_from_slice(&bytes[..end]);
    out.push(0);
    out
}

#[inline]
unsafe fn ptr_bytes(p: *const c_char) -> Vec<u8> {
    if p.is_null() {
        Vec::new()
    } else {
        unsafe { CStr::from_ptr(p).to_bytes().to_vec() }
    }
}

fn with_dq_guard<R, F: FnOnce(usize) -> R>(
    ctx: *mut c_void,
    host_cb: NzbgetRsRepairCtrlHostFn,
    f: F,
) -> R {
    unsafe extern "C" fn trampoline<F: FnOnce(usize) -> R, R>(slot_ptr: usize, dq_ptr: usize) {
        let slot = unsafe { &mut *(slot_ptr as *mut (Option<F>, Option<R>)) };
        if let Some(cb) = slot.0.take() {
            slot.1 = Some(cb(dq_ptr));
        }
    }
    let mut slot: (Option<F>, Option<R>) = (Some(f), None);
    unsafe {
        host_cb(
            ctx,
            0,
            0,
            0,
            trampoline::<F, R> as *const () as usize,
            (&mut slot as *mut (Option<F>, Option<R>)) as usize,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
    }
    slot.1.expect("DownloadQueue::Guard callback must execute")
}

struct CtrlSnapshot {
    nzb_ptr: usize,
    checker_status: i32,
    checker_stage: i32,
    file_progress: i32,
    stage_progress: i32,
    par_full: bool,
    is_stopped: bool,
    par_quick_opt: bool,
    par_time_limit: i32,
    pause_post_process: bool,
    force_par_full: bool,
    force_repair: bool,
    post_stage: i32,
    cur_time: i64,
    block_size: i64,
    post_stage_time: i64,
    post_start_time: i64,
    progress_label: Vec<u8>,
    info_name: Vec<u8>,
    par_filename: Vec<u8>,
    checker_dest_dir: Vec<u8>,
    unrar_cmd: Vec<u8>,
}

struct NzbSnapshot {
    id: i32,
    kind: i32,
    nk_nzb: i32,
    download_sec: i32,
    repair_sec: i32,
    par_status: i32,
    reprocess: bool,
    force_priority: bool,
    extra_par_blocks: i32,
    file_list_len: usize,
    completed_len: usize,
    jobs_len: usize,
    dupe_mode: i32,
    dupe_recovered_bytes: i64,
    expected_data_size: i64,
    name: Vec<u8>,
    dest_dir: Vec<u8>,
    dupe_key: Vec<u8>,
    queued_filename: Vec<u8>,
    has_twin_files_param: bool,
    twin_files_param: Vec<u8>,
}

fn inspect_ctrl(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn) -> CtrlSnapshot {
    let mut ints = [0 as c_int; 12];
    let mut i64s = [0i64; 4];
    let mut strs = [ptr::null::<c_char>(); 5];
    let nzb_ptr = unsafe {
        host_cb(
            ctx,
            1,
            0,
            0,
            0,
            0,
            ptr::null(),
            ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        )
    };
    CtrlSnapshot {
        nzb_ptr,
        checker_status: ints[0],
        checker_stage: ints[1],
        file_progress: ints[2],
        stage_progress: ints[3],
        par_full: ints[4] != 0,
        is_stopped: ints[5] != 0,
        par_quick_opt: ints[6] != 0,
        par_time_limit: ints[7],
        pause_post_process: ints[8] != 0,
        force_par_full: ints[9] != 0,
        force_repair: ints[10] != 0,
        post_stage: ints[11],
        cur_time: i64s[0],
        block_size: i64s[1],
        post_stage_time: i64s[2],
        post_start_time: i64s[3],
        progress_label: unsafe { ptr_bytes(strs[0]) },
        info_name: unsafe { ptr_bytes(strs[1]) },
        par_filename: unsafe { ptr_bytes(strs[2]) },
        checker_dest_dir: unsafe { ptr_bytes(strs[3]) },
        unrar_cmd: unsafe { ptr_bytes(strs[4]) },
    }
}

fn inspect_nzb(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn, nzb_ptr: usize) -> NzbSnapshot {
    let mut ints = [0 as c_int; 14];
    let mut i64s = [0i64; 2];
    let mut strs = [ptr::null::<c_char>(); 5];
    unsafe {
        host_cb(
            ctx,
            2,
            0,
            0,
            nzb_ptr,
            0,
            ptr::null(),
            ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
    }
    NzbSnapshot {
        id: ints[0],
        kind: ints[1],
        nk_nzb: ints[2],
        download_sec: ints[3],
        repair_sec: ints[5],
        par_status: ints[6],
        reprocess: ints[7] != 0,
        force_priority: ints[8] != 0,
        extra_par_blocks: ints[9],
        file_list_len: ints[10].max(0) as usize,
        completed_len: ints[11].max(0) as usize,
        jobs_len: ints[12].max(0) as usize,
        dupe_mode: ints[13],
        dupe_recovered_bytes: i64s[0],
        expected_data_size: i64s[1],
        name: unsafe { ptr_bytes(strs[0]) },
        dest_dir: unsafe { ptr_bytes(strs[1]) },
        dupe_key: unsafe { ptr_bytes(strs[2]) },
        queued_filename: unsafe { ptr_bytes(strs[3]) },
        has_twin_files_param: !strs[4].is_null(),
        twin_files_param: unsafe { ptr_bytes(strs[4]) },
    }
}

fn emit_msg(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn, kind: c_int, text: &[u8]) {
    let c_text = to_c_msg_1024(text);
    unsafe {
        host_cb(
            ctx,
            7,
            kind,
            0,
            0,
            0,
            c_text.as_ptr() as *const c_char,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
    }
}

fn emit_debug(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn, text: &[u8]) {
    let c_text = to_c_buf(text);
    unsafe {
        host_cb(
            ctx,
            7,
            5,
            0,
            0,
            0,
            c_text.as_ptr() as *const c_char,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
    }
}

#[derive(Default)]
struct RepairCtrlState {
    par_time: i64,
    repair_time: i64,
    download_sec: i32,
    twin_entries: Vec<NzbEntry>,
    twin_name: Vec<u8>,
    twin_index: Vec<u8>,
    twin_block_size: u64,
    twin_files: Vec<Vec<u8>>,
    last_origname: Option<Box<[u8]>>,
    last_twin_path: Option<Box<[u8]>>,
    last_twin_vols: Vec<Box<[u8]>>,
}

pub struct NzbgetRsRepairController {
    twin_fetcher: ArticleFetcher,
    state: std::sync::Mutex<RepairCtrlState>,
}

impl NzbgetRsRepairController {
    pub fn new(nntp_host: NzbgetRsNntpFetchHostFn) -> Self {
        Self {
            twin_fetcher: ArticleFetcher::new(nntp_host),
            state: std::sync::Mutex::new(RepairCtrlState::default()),
        }
    }

    #[inline]
    fn lock_state(&self) -> std::sync::MutexGuard<'_, RepairCtrlState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn stop(&self) {
        self.twin_fetcher.stop();
    }

    fn delete_twin_files(&self) {
        let paths = {
            let mut st = self.lock_state();
            st.twin_entries.clear();
            st.twin_name.clear();
            st.twin_index.clear();
            st.twin_block_size = 0;
            std::mem::take(&mut st.twin_files)
        };
        for path in &paths {
            let c_path = to_c_buf(path);
            if let Ok(cs) = CStr::from_bytes_with_nul(&c_path) {
                crate::fsops::delete_file(cs);
            }
        }
    }

    fn is_stopped(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn) -> bool {
        inspect_ctrl(ctx, host_cb).is_stopped
    }

    pub fn run(&self, ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn) {
        let (nzb_name, dest_dir, dupe_repaired, dl_sec, par_quick_opt, force_par_full, force_repair, cur_time) =
            with_dq_guard(ctx, host_cb, |_dq| {
                let ctrl = inspect_ctrl(ctx, host_cb);
                let nzb = inspect_nzb(ctx, host_cb, ctrl.nzb_ptr);
                (
                    nzb.name,
                    nzb.dest_dir,
                    nzb.dupe_recovered_bytes > 0,
                    nzb.download_sec,
                    ctrl.par_quick_opt,
                    ctrl.force_par_full,
                    ctrl.force_repair,
                    ctrl.cur_time,
                )
            });

        {
            let mut st = self.lock_state();
            st.par_time = cur_time;
            st.download_sec = dl_sec;
        }
        let par_quick = should_use_par_quick(par_quick_opt, force_par_full, dupe_repaired);
        let c_dest = to_c_buf(&dest_dir);
        let c_name = to_c_buf(&nzb_name);
        unsafe {
            host_cb(
                ctx,
                3,
                c_int::from(par_quick),
                c_int::from(force_repair),
                0,
                0,
                c_dest.as_ptr() as *const c_char,
                c_name.as_ptr() as *const c_char,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    pub fn add_par(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        file_ptr: usize,
        deleted: bool,
    ) -> bool {
        let mut strs = [ptr::null::<c_char>(); 2];
        let same_collection = unsafe {
            host_cb(
                ctx,
                4,
                0,
                0,
                file_ptr,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                strs.as_mut_ptr(),
            ) != 0
        };
        if same_collection && !deleted {
            let dest_dir = unsafe { ptr_bytes(strs[0]) };
            let filename = unsafe { ptr_bytes(strs[1]) };
            let full = crate::artwritercore::article_writer_format_dest_file_path(
                &dest_dir,
                crate::paths::PATH_SEPARATOR,
                &filename,
            );
            let c_full = to_c_buf(&full);
            unsafe {
                host_cb(
                    ctx,
                    4,
                    1,
                    0,
                    0,
                    0,
                    c_full.as_ptr() as *const c_char,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
        } else {
            unsafe {
                host_cb(
                    ctx,
                    4,
                    2,
                    0,
                    0,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
        }
        same_collection
    }

    pub fn par_check_completed(&self, ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn) {
        self.delete_twin_files();
        let (start_dl_sec, par_time) = {
            let st = self.lock_state();
            (st.download_sec, st.par_time)
        };
        with_dq_guard(ctx, host_cb, |_dq| {
            let ctrl = inspect_ctrl(ctx, host_cb);
            let nzb = inspect_nzb(ctx, host_cb, ctrl.nzb_ptr);
            let (new_status, set_rep, rep_val, wait_time, par_sec) = complete_par_check(
                ctrl.checker_status,
                nzb.par_status,
                2,
                3,
                1,
                1,
                3,
                4,
                2,
                nzb.download_sec,
                start_dl_sec,
                ctrl.cur_time,
                par_time,
            );
            let mut ints = [
                new_status,
                c_int::from(set_rep),
                c_int::from(rep_val),
                wait_time,
                par_sec,
                c_int::from(ctrl.par_full),
            ];
            unsafe {
                host_cb(
                    ctx,
                    5,
                    0,
                    0,
                    ctrl.nzb_ptr,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ints.as_mut_ptr(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
        });
    }

    fn find_pars_pass(
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        nzb_ptr: usize,
        nzb_name: &[u8],
        file_list_len: usize,
        block_size: i64,
        par_filename: &[u8],
        strict_par_name: bool,
        exact_par_name: bool,
        blocks: &mut Vec<(usize, usize, i32, bool, Vec<u8>)>,
    ) -> i32 {
        let base_off = crate::paths::base_file_name(par_filename);
        let base_par = &par_filename[base_off..];
        let Some((main_base_len, _)) = crate::parparser::parse_par_filename(base_par, true) else {
            let mut msg = b"Internal error: could not parse filename ".to_vec();
            msg.extend_from_slice(base_par);
            emit_msg(ctx, host_cb, MK_ERROR, &msg);
            let _ = nzb_name;
            return 0;
        };
        let main_base = &base_par[..main_base_len.min(base_par.len())];
        let mut found = 0i32;

        for idx in 0..file_list_len {
            let mut f_ints = [0 as c_int; 3];
            let mut f_i64s = [0i64; 1];
            let mut f_strs = [ptr::null::<c_char>(); 1];
            let file_ptr = unsafe {
                host_cb(
                    ctx,
                    6,
                    idx as c_int,
                    0,
                    nzb_ptr,
                    0,
                    ptr::null(),
                    ptr::null(),
                    f_ints.as_mut_ptr(),
                    f_i64s.as_mut_ptr(),
                    f_strs.as_mut_ptr(),
                )
            };
            let confirmed = f_ints[0] != 0;
            let paused = f_ints[1] != 0;
            let file_size = f_i64s[0];
            let filename = unsafe { ptr_bytes(f_strs[0]) };

            let (parsed, raw_blocks) = match crate::parparser::parse_par_filename(&filename, confirmed) {
                Some((_, bc)) => (true, bc),
                None => (false, 0),
            };
            let (usable, eff_blocks, strict_ok) = eval_find_pars_candidate(
                parsed,
                raw_blocks,
                file_size,
                block_size,
                exact_par_name,
                strict_par_name,
                &filename,
                main_base,
            );
            let same_coll = !exact_par_name
                || crate::parparser::same_par_collection(&filename, base_par, confirmed);
            if same_coll && usable && strict_ok && !blocks.iter().any(|b| b.1 == file_ptr) {
                blocks.push((idx, file_ptr, eff_blocks, paused, filename));
                found += eff_blocks;
            }
        }
        found
    }

    pub fn request_more_pars(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        block_needed: i32,
    ) -> (bool, i32) {
        with_dq_guard(ctx, host_cb, |_dq| {
            let ctrl = inspect_ctrl(ctx, host_cb);
            let nzb = inspect_nzb(ctx, host_cb, ctrl.nzb_ptr);
            let mut available: Vec<(usize, usize, i32, bool, Vec<u8>)> = Vec::new();
            let mut block_found = Self::find_pars_pass(
                ctx,
                host_cb,
                ctrl.nzb_ptr,
                &nzb.name,
                nzb.file_list_len,
                ctrl.block_size,
                &ctrl.par_filename,
                true,
                true,
                &mut available,
            );
            if block_found < block_needed {
                block_found += Self::find_pars_pass(
                    ctx,
                    host_cb,
                    ctrl.nzb_ptr,
                    &nzb.name,
                    nzb.file_list_len,
                    ctrl.block_size,
                    &ctrl.par_filename,
                    true,
                    false,
                    &mut available,
                );
            }
            if block_found < block_needed {
                block_found += Self::find_pars_pass(
                    ctx,
                    host_cb,
                    ctrl.nzb_ptr,
                    &nzb.name,
                    nzb.file_list_len,
                    ctrl.block_size,
                    &ctrl.par_filename,
                    false,
                    false,
                    &mut available,
                );
            }

            available.sort_by_key(|b| b.2);

            let mut rem_needed = block_needed;
            if block_found >= block_needed {
                let counts: Vec<i32> = available.iter().map(|b| b.2).collect();
                let (selected, rem) = select_more_pars(&counts, block_needed);
                rem_needed = rem;
                for sel_idx in selected {
                    let (file_idx, _, _, paused, ref fname) = available[sel_idx];
                    if paused {
                        let mut msg = b"Unpausing ".to_vec();
                        msg.extend_from_slice(&nzb.name);
                        msg.push(crate::paths::PATH_SEPARATOR);
                        msg.extend_from_slice(fname);
                        msg.extend_from_slice(b" for par-recovery");
                        emit_msg(ctx, host_cb, MK_INFO, &msg);
                        unsafe {
                            host_cb(
                                ctx,
                                6,
                                file_idx as c_int,
                                1,
                                ctrl.nzb_ptr,
                                0,
                                ptr::null(),
                                ptr::null(),
                                ptr::null_mut(),
                                ptr::null_mut(),
                                ptr::null_mut(),
                            );
                        }
                    }
                }
            }

            let mut has_unpaused_par = false;
            for idx in 0..nzb.file_list_len {
                let mut f_ints = [0 as c_int; 3];
                let mut f_i64s = [0i64; 1];
                let mut f_strs = [ptr::null::<c_char>(); 1];
                unsafe {
                    host_cb(
                        ctx,
                        6,
                        idx as c_int,
                        0,
                        ctrl.nzb_ptr,
                        0,
                        ptr::null(),
                        ptr::null(),
                        f_ints.as_mut_ptr(),
                        f_i64s.as_mut_ptr(),
                        f_strs.as_mut_ptr(),
                    );
                }
                if f_ints[2] != 0 && f_ints[1] == 0 {
                    has_unpaused_par = true;
                    break;
                }
            }

            (rem_needed <= 0 || has_unpaused_par, block_found)
        })
    }

    pub fn update_par_check_progress(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
    ) {
        let cur_repair_time = self.lock_state().repair_time;
        let mut updated_repair_time = cur_repair_time;
        with_dq_guard(ctx, host_cb, |_dq| {
            let ctrl = inspect_ctrl(ctx, host_cb);
            let nzb = inspect_nzb(ctx, host_cb, ctrl.nzb_ptr);
            let label_c = if ctrl.file_progress == 0 {
                Some(to_c_buf(&ctrl.progress_label))
            } else {
                None
            };
            let (new_stage, changed, set_rt, new_rsec, cancel, est_sec, est_min) =
                eval_par_progress_update(
                    ctrl.checker_stage,
                    ctrl.post_stage,
                    1,
                    2,
                    3,
                    4,
                    ctrl.cur_time,
                    ctrl.post_stage_time,
                    cur_repair_time,
                    nzb.repair_sec,
                    ctrl.is_stopped,
                    ctrl.par_time_limit,
                    ctrl.checker_stage == 2,
                    ctrl.stage_progress,
                );
            let mut flags = c_int::from(changed);
            if changed {
                if set_rt {
                    updated_repair_time = ctrl.cur_time;
                } else if new_stage == 4 {
                    flags |= 8;
                }
            }
            let mut ints = [
                ctrl.file_progress,
                ctrl.stage_progress,
                flags,
                new_stage,
                new_rsec,
            ];
            let mut i64s = [ctrl.cur_time];
            unsafe {
                host_cb(
                    ctx,
                    8,
                    0,
                    0,
                    0,
                    0,
                    label_c
                        .as_ref()
                        .map_or(ptr::null(), |b| b.as_ptr() as *const c_char),
                    ptr::null(),
                    ints.as_mut_ptr(),
                    i64s.as_mut_ptr(),
                    ptr::null_mut(),
                );
            }
            if cancel {
                emit_debug(
                    ctx,
                    host_cb,
                    format!("Estimated repair time {est_sec} seconds").as_bytes(),
                );
                let mut msg = b"Cancelling par-repair for ".to_vec();
                msg.extend_from_slice(&ctrl.info_name);
                msg.extend_from_slice(
                    format!(
                        ", estimated repair time ({est_min} minutes) exceeds allowed repair time"
                    )
                    .as_bytes(),
                );
                emit_msg(ctx, host_cb, MK_WARNING, &msg);
                unsafe {
                    host_cb(
                        ctx,
                        7,
                        6,
                        0,
                        0,
                        0,
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            }
        });
        if updated_repair_time != cur_repair_time {
            self.lock_state().repair_time = updated_repair_time;
        }
        self.check_pause_state(ctx, host_cb);
    }

    fn check_pause_state(&self, ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn) {
        let ctrl0 = inspect_ctrl(ctx, host_cb);
        let nzb0 = inspect_nzb(ctx, host_cb, ctrl0.nzb_ptr);
        if !ctrl0.pause_post_process || nzb0.force_priority {
            return;
        }
        let stage_time = ctrl0.post_stage_time;
        let start_time = ctrl0.post_start_time;
        let (par_time, repair_time) = {
            let st = self.lock_state();
            (st.par_time, st.repair_time)
        };
        let wait_time = ctrl0.cur_time;

        loop {
            let c_check = inspect_ctrl(ctx, host_cb);
            let n_check = inspect_nzb(ctx, host_cb, c_check.nzb_ptr);
            if !c_check.pause_post_process || n_check.force_priority || c_check.is_stopped {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            let c_after = inspect_ctrl(ctx, host_cb);
            let (s_stage, s_start, s_par, s_rep) = calc_paused_timestamps(
                c_after.cur_time,
                wait_time,
                stage_time,
                start_time,
                par_time,
                repair_time,
            );
            let mut i64s = [s_stage, s_start];
            unsafe {
                host_cb(
                    ctx,
                    8,
                    1,
                    0,
                    0,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    i64s.as_mut_ptr(),
                    ptr::null_mut(),
                );
            }
            if s_par > 0 || s_rep > 0 {
                let mut st = self.lock_state();
                if s_par > 0 {
                    st.par_time = s_par;
                }
                if s_rep > 0 {
                    st.repair_time = s_rep;
                }
            }
        }
    }

    pub fn register_verified_file(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        filename: &[u8],
    ) {
        let verified_path =
            crate::fsops::get_real_path(filename).unwrap_or_else(|| filename.to_vec());
        with_dq_guard(ctx, host_cb, |_dq| {
            let ctrl = inspect_ctrl(ctx, host_cb);
            let nzb = inspect_nzb(ctx, host_cb, ctrl.nzb_ptr);
            for job_idx in (0..nzb.jobs_len).rev() {
                let mut j_ints = [0 as c_int; 1];
                let mut j_strs = [ptr::null::<c_char>(); 1];
                unsafe {
                    host_cb(
                        ctx,
                        9,
                        4,
                        job_idx as c_int,
                        ctrl.nzb_ptr,
                        0,
                        ptr::null(),
                        ptr::null(),
                        j_ints.as_mut_ptr(),
                        ptr::null_mut(),
                        j_strs.as_mut_ptr(),
                    );
                }
                let job_file_id = j_ints[0];
                let mut current_name = unsafe { ptr_bytes(j_strs[0]) };
                for cf_idx in 0..nzb.completed_len {
                    let mut c_ints = [0 as c_int; 3];
                    let mut c_strs = [ptr::null::<c_char>(); 2];
                    unsafe {
                        host_cb(
                            ctx,
                            9,
                            3,
                            cf_idx as c_int,
                            ctrl.nzb_ptr,
                            0,
                            ptr::null(),
                            ptr::null(),
                            c_ints.as_mut_ptr(),
                            ptr::null_mut(),
                            c_strs.as_mut_ptr(),
                        );
                    }
                    if c_ints[0] == job_file_id {
                        current_name = unsafe { ptr_bytes(c_strs[0]) };
                        break;
                    }
                }
                let raw_path = crate::artwritercore::article_writer_format_dest_file_path(
                    &nzb.dest_dir,
                    crate::paths::PATH_SEPARATOR,
                    &current_name,
                );
                let real_path = crate::fsops::get_real_path(&raw_path).unwrap_or(raw_path);
                if real_path == verified_path {
                    unsafe {
                        host_cb(
                            ctx,
                            9,
                            5,
                            job_idx as c_int,
                            ctrl.nzb_ptr,
                            0,
                            ptr::null(),
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                    }
                }
            }
        });
    }

    pub fn is_parred_file(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        filename: &[u8],
    ) -> bool {
        let count = unsafe {
            host_cb(
                ctx,
                9,
                1,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        for idx in 0..count {
            let mut strs = [ptr::null::<c_char>(); 1];
            unsafe {
                host_cb(
                    ctx,
                    9,
                    2,
                    idx as c_int,
                    0,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    strs.as_mut_ptr(),
                );
            }
            if !strs[0].is_null() {
                let pf = unsafe { CStr::from_ptr(strs[0]).to_bytes() };
                if pf.eq_ignore_ascii_case(filename) {
                    return true;
                }
            }
        }
        false
    }

    pub fn find_file_crc(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        filename: &[u8],
        out_crc: *mut u32,
        segments_ptr: usize,
    ) -> i32 {
        let found = with_dq_guard(ctx, host_cb, |_dq| {
            let ctrl = inspect_ctrl(ctx, host_cb);
            let nzb = inspect_nzb(ctx, host_cb, ctrl.nzb_ptr);
            for cf_idx in 0..nzb.completed_len {
                let mut c_ints = [0 as c_int; 3];
                let mut c_strs = [ptr::null::<c_char>(); 2];
                unsafe {
                    host_cb(
                        ctx,
                        9,
                        3,
                        cf_idx as c_int,
                        ctrl.nzb_ptr,
                        0,
                        ptr::null(),
                        ptr::null(),
                        c_ints.as_mut_ptr(),
                        ptr::null_mut(),
                        c_strs.as_mut_ptr(),
                    );
                }
                let cf_name = unsafe { ptr_bytes(c_strs[0]) };
                if cf_name.eq_ignore_ascii_case(filename) {
                    return Some((c_ints[0], c_ints[1], c_ints[2] as u32, nzb.reprocess));
                }
            }
            None
        });

        let Some((id, status, crc, reprocess)) = found else {
            return 0; // ParChecker::fsUnknown
        };
        if !out_crc.is_null() {
            unsafe { *out_crc = crc };
        }

        let mut dbg = b"Found completed file: ".to_vec();
        dbg.extend_from_slice(filename);
        dbg.extend_from_slice(format!(", CRC: {crc:08x}, Status: {status}").as_bytes());
        emit_debug(ctx, host_cb, &dbg);

        let mut has_segments = false;
        if should_load_partial_segments(status, 2, id, reprocess) {
            let loaded = unsafe {
                host_cb(
                    ctx,
                    9,
                    6,
                    id,
                    segments_ptr,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
            if loaded == 0 {
                return 0; // ParChecker::fsUnknown
            }
            has_segments = loaded >= 2;
        }

        classify_file_crc_status(status, reprocess, has_segments, 1, 3, 2, 1, 3, 2, 0)
    }

    pub fn find_file_origname(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        filename: &[u8],
    ) -> *const c_char {
        let orig = with_dq_guard(ctx, host_cb, |_dq| {
            let ctrl = inspect_ctrl(ctx, host_cb);
            let nzb = inspect_nzb(ctx, host_cb, ctrl.nzb_ptr);
            for cf_idx in 0..nzb.completed_len {
                let mut c_ints = [0 as c_int; 3];
                let mut c_strs = [ptr::null::<c_char>(); 2];
                unsafe {
                    host_cb(
                        ctx,
                        9,
                        3,
                        cf_idx as c_int,
                        ctrl.nzb_ptr,
                        0,
                        ptr::null(),
                        ptr::null(),
                        c_ints.as_mut_ptr(),
                        ptr::null_mut(),
                        c_strs.as_mut_ptr(),
                    );
                }
                let cf_name = unsafe { ptr_bytes(c_strs[0]) };
                if cf_name.eq_ignore_ascii_case(filename) {
                    if c_strs[1].is_null() {
                        return None;
                    }
                    return Some(unsafe { ptr_bytes(c_strs[1]) });
                }
            }
            None
        });
        match orig {
            Some(bytes) => {
                let boxed = to_c_buf(&bytes).into_boxed_slice();
                let p = boxed.as_ptr() as *const c_char;
                self.lock_state().last_origname = Some(boxed);
                p
            }
            None => ptr::null(),
        }
    }

    pub fn request_dupe_sources(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        dupe_source_list_ptr: usize,
    ) {
        with_dq_guard(ctx, host_cb, |dq_ptr| {
            let ctrl = inspect_ctrl(ctx, host_cb);
            let nzb = inspect_nzb(ctx, host_cb, ctrl.nzb_ptr);
            if nzb.dupe_mode == crate::dupecoord::DUPE_MODE_FORCE {
                return;
            }
            let mut q_ints = [0 as c_int; 2];
            unsafe {
                host_cb(
                    ctx,
                    10,
                    0,
                    0,
                    dq_ptr,
                    0,
                    ptr::null(),
                    ptr::null(),
                    q_ints.as_mut_ptr(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
            let hist_len = q_ints[1].max(0) as usize;
            let mut dupes: Vec<(i32, Vec<u8>)> = Vec::new();
            for h_idx in 0..hist_len {
                let item_ptr = unsafe {
                    host_cb(
                        ctx,
                        10,
                        2,
                        h_idx as c_int,
                        dq_ptr,
                        0,
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    )
                };
                if item_ptr == 0 {
                    continue;
                }
                let item = inspect_nzb(ctx, host_cb, item_ptr);
                if item.dupe_mode != crate::dupecoord::DUPE_MODE_FORCE
                    && crate::dupecoord::same_name_or_key(
                        &item.name,
                        &item.dupe_key,
                        &nzb.name,
                        &nzb.dupe_key,
                    )
                {
                    dupes.push((item.id, item.dest_dir));
                }
            }
            if dupes.is_empty() {
                return;
            }
            let dir_refs: Vec<&[u8]> = dupes.iter().map(|(_, d)| d.as_slice()).collect();
            let dest_path: &Path = crate::fsops::path(&nzb.dest_dir);
            let usable = crate::pardamage::dupe_matcher_filter_dupe_sources(
                &nzb.name,
                dest_path,
                nzb.expected_data_size,
                &ctrl.unrar_cmd,
                &dir_refs,
                |kind, msg| {
                    emit_msg(
                        ctx,
                        host_cb,
                        if kind == 3 { MK_DETAIL } else { MK_INFO },
                        msg,
                    );
                },
            );
            for u_idx in usable {
                let (dupe_id, ref dupe_dir) = dupes[u_idx];
                let c_dir = to_c_buf(dupe_dir);
                unsafe {
                    host_cb(
                        ctx,
                        10,
                        5,
                        dupe_id,
                        dupe_source_list_ptr,
                        0,
                        c_dir.as_ptr() as *const c_char,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            }
        });
    }

    pub fn stat_dupe_sources(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        dupe_source_list_ptr: usize,
    ) {
        with_dq_guard(ctx, host_cb, |dq_ptr| {
            let ctrl = inspect_ctrl(ctx, host_cb);
            let nzb = inspect_nzb(ctx, host_cb, ctrl.nzb_ptr);
            let ds_len = unsafe {
                host_cb(
                    ctx,
                    10,
                    6,
                    -1,
                    dupe_source_list_ptr,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
            let mut q_ints = [0 as c_int; 2];
            unsafe {
                host_cb(
                    ctx,
                    10,
                    0,
                    0,
                    dq_ptr,
                    0,
                    ptr::null(),
                    ptr::null(),
                    q_ints.as_mut_ptr(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
            let hist_len = q_ints[1].max(0) as usize;

            let mut total_extra = 0i32;
            for idx in 0..ds_len {
                let mut ds_ints = [0 as c_int; 2];
                unsafe {
                    host_cb(
                        ctx,
                        10,
                        6,
                        idx as c_int,
                        dupe_source_list_ptr,
                        0,
                        ptr::null(),
                        ptr::null(),
                        ds_ints.as_mut_ptr(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
                let ds_id = ds_ints[0];
                let used_blocks = ds_ints[1];
                if used_blocks > 0 {
                    for h_idx in 0..hist_len {
                        let item_ptr = unsafe {
                            host_cb(
                                ctx,
                                10,
                                2,
                                h_idx as c_int,
                                dq_ptr,
                                0,
                                ptr::null(),
                                ptr::null(),
                                ptr::null_mut(),
                                ptr::null_mut(),
                                ptr::null_mut(),
                            )
                        };
                        if item_ptr != 0 {
                            let item = inspect_nzb(ctx, host_cb, item_ptr);
                            if item.id == ds_id {
                                unsafe {
                                    host_cb(
                                        ctx,
                                        10,
                                        7,
                                        item.extra_par_blocks - used_blocks,
                                        item_ptr,
                                        0,
                                        ptr::null(),
                                        ptr::null(),
                                        ptr::null_mut(),
                                        ptr::null_mut(),
                                        ptr::null_mut(),
                                    );
                                }
                            }
                        }
                    }
                }
                total_extra += used_blocks;
            }
            unsafe {
                host_cb(
                    ctx,
                    10,
                    7,
                    nzb.extra_par_blocks + total_extra,
                    ctrl.nzb_ptr,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
        });
    }

    fn fetch_twin_file(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        entry: &NzbEntry,
    ) -> Option<(Vec<u8>, Vec<u8>)> {
        let data = fetch_entry(&self.twin_fetcher, entry, MAX_TWIN_FETCH_SIZE, || {
            Self::is_stopped(ctx, host_cb)
        })?;
        let ctrl = inspect_ctrl(ctx, host_cb);
        let count = self.lock_state().twin_files.len();
        let name = build_twin_filename(entry.filename.as_bytes(), count);
        let path = crate::artwritercore::article_writer_format_dest_file_path(
            &ctrl.checker_dest_dir,
            crate::paths::PATH_SEPARATOR,
            &name,
        );
        let c_path = to_c_buf(&path);
        let cs_path = CStr::from_bytes_with_nul(&c_path).ok()?;
        if !crate::fsops::save_buffer_into_file(cs_path, &data) {
            return None;
        }
        self.lock_state().twin_files.push(path.clone());
        Some((path, data))
    }

    pub fn request_twin_index(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
    ) -> *const c_char {
        let empty_cstr = || -> *const c_char {
            let boxed = vec![0u8].into_boxed_slice();
            let p = boxed.as_ptr() as *const c_char;
            self.lock_state().last_twin_path = Some(boxed);
            p
        };

        let ctrl0 = inspect_ctrl(ctx, host_cb);
        let c_par_fn = to_c_buf(&ctrl0.par_filename);
        let mut par_buf = Vec::new();
        if let Ok(cs) = CStr::from_bytes_with_nul(&c_par_fn) {
            crate::fsops::load_file_into_buffer(
                cs,
                false,
                &crate::fsops::posix_stat,
                |buf_size| {
                    if buf_size <= 0 {
                        ptr::null_mut()
                    } else {
                        par_buf.resize(buf_size as usize, 0);
                        par_buf.as_mut_ptr()
                    }
                },
            );
        }
        let own = if par_buf.is_empty() {
            Vec::new()
        } else {
            par2_sigs(&par_buf)
        };
        let own_print = twin_fingerprint(&own);
        if own_print.is_empty() {
            return empty_cstr();
        }

        let mut candidates: Vec<(Vec<u8>, Vec<u8>, bool)> = with_dq_guard(ctx, host_cb, |dq_ptr| {
            let ctrl = inspect_ctrl(ctx, host_cb);
            let nzb = inspect_nzb(ctx, host_cb, ctrl.nzb_ptr);
            let mut q_ints = [0 as c_int; 2];
            unsafe {
                host_cb(
                    ctx,
                    10,
                    0,
                    0,
                    dq_ptr,
                    0,
                    ptr::null(),
                    ptr::null(),
                    q_ints.as_mut_ptr(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
            let q_len = q_ints[0].max(0) as usize;
            let h_len = q_ints[1].max(0) as usize;
            let mut out = Vec::new();

            let mut consider = |other_ptr: usize| {
                if other_ptr == 0 {
                    return;
                }
                let other = inspect_nzb(ctx, host_cb, other_ptr);
                let is_self = other_ptr == ctrl.nzb_ptr;
                let same = !is_self
                    && other.kind == other.nk_nzb
                    && crate::dupecoord::same_name_or_key(
                        &other.name,
                        &other.dupe_key,
                        &nzb.name,
                        &nzb.dupe_key,
                    );
                let has_print = same && other.has_twin_files_param;
                let print_matches = has_print && other.twin_files_param == own_print.as_bytes();
                if !should_consider_twin_candidate(
                    is_self,
                    other.kind,
                    other.nk_nzb,
                    same,
                    &other.queued_filename,
                    has_print,
                    print_matches,
                ) || !crate::fsops::file_exists(&other.queued_filename, &crate::fsops::posix_stat)
                {
                    return;
                }
                out.push((other.name, other.queued_filename, has_print));
            };

            for idx in 0..q_len {
                let p = unsafe {
                    host_cb(
                        ctx,
                        10,
                        1,
                        idx as c_int,
                        dq_ptr,
                        0,
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    )
                };
                consider(p);
            }
            for idx in 0..h_len {
                let p = unsafe {
                    host_cb(
                        ctx,
                        10,
                        2,
                        idx as c_int,
                        dq_ptr,
                        0,
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    )
                };
                consider(p);
            }
            out
        });

        candidates.sort_by_key(|c| std::cmp::Reverse(c.2));

        let own_names: Vec<&[u8]> = own.iter().map(|s| s.name.as_bytes()).collect();
        let own_md5s: Vec<&[u8]> = own.iter().map(|s| s.md5.as_bytes()).collect();

        for (cand_name, cand_nzb_fn, _) in candidates {
            if Self::is_stopped(ctx, host_cb) {
                break;
            }
            let Ok(xml) = std::fs::read(crate::fsops::path(&cand_nzb_fn)) else {
                continue;
            };
            let entries = read_nzb(&xml);
            let is_par2: Vec<u8> = entries.iter().map(|e| u8::from(e.is_par2())).collect();
            let sizes: Vec<i64> = entries.iter().map(|e| e.size() as i64).collect();
            let Some(best_idx) = select_best_twin_par2_index(&is_par2, &sizes, MAX_INDEX_SIZE)
            else {
                continue;
            };
            let index_entry = entries[best_idx].clone();
            let Some((path, data)) = self.fetch_twin_file(ctx, host_cb, &index_entry) else {
                continue;
            };
            let cmp = compare_twin_par2_data(&own_names, &own_md5s, own_print.as_bytes(), &data);
            if cmp != 0 {
                let reason = twin_par2_reject_reason(cmp);
                let reason_end = reason.iter().position(|&b| b == 0).unwrap_or(reason.len());
                let mut msg = cand_name.clone();
                msg.extend_from_slice(b": its par2 set ");
                msg.extend_from_slice(&reason[..reason_end]);
                emit_msg(ctx, host_cb, MK_DETAIL, &msg);
                let c_del = to_c_buf(&path);
                if let Ok(cs) = CStr::from_bytes_with_nul(&c_del) {
                    crate::fsops::delete_file(cs);
                }
                self.lock_state().twin_files.pop();
                continue;
            }

            let blk_sz = par2_block_size(&data);
            let mb = (blk_sz as f64) / (1024.0 * 1024.0);
            let mut msg = b"Trying the par2 set of twin ".to_vec();
            msg.extend_from_slice(&cand_name);
            msg.extend_from_slice(format!(" (blocks of {mb:.1} MB)").as_bytes());
            emit_msg(ctx, host_cb, MK_INFO, &msg);

            let boxed = to_c_buf(&path).into_boxed_slice();
            let p = boxed.as_ptr() as *const c_char;
            let mut st = self.lock_state();
            st.twin_name = cand_name;
            st.twin_entries = entries;
            st.twin_index = index_entry.filename.into_bytes();
            st.twin_block_size = blk_sz;
            st.last_twin_path = Some(boxed);
            return p;
        }

        empty_cstr()
    }

    pub fn request_twin_volumes(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        block_needed: i32,
    ) -> usize {
        let (twin_entries, twin_index, twin_block_size, twin_name) = {
            let mut st = self.lock_state();
            st.last_twin_vols.clear();
            (
                st.twin_entries.clone(),
                st.twin_index.clone(),
                st.twin_block_size,
                st.twin_name.clone(),
            )
        };
        let mut volumes: Vec<(NzbEntry, i32)> = Vec::new();
        for entry in &twin_entries {
            if should_skip_twin_volume_entry(
                entry.is_par2(),
                entry.filename.as_bytes(),
                &twin_index,
            ) {
                continue;
            }
            let mut blocks = volume_blocks_bytes(entry.filename.as_bytes())
                .map_or(-1, |b| b as i32);
            if blocks < 0 && twin_block_size > 0 {
                blocks = estimate_twin_vol_blocks(entry.size() as i64, twin_block_size);
            }
            if blocks > 0 {
                volumes.push((entry.clone(), blocks));
            }
        }
        volumes.sort_by_key(|v| v.1);

        let init_blocks: Vec<i32> = volumes.iter().map(|v| v.1).collect();
        let (available, covers) = sum_and_check_twin_blocks(&init_blocks, block_needed);
        if !covers {
            let mut msg = b"Twin ".to_vec();
            msg.extend_from_slice(&twin_name);
            msg.extend_from_slice(
                format!(" holds {available} par-block(s), {block_needed} needed: not fetched")
                    .as_bytes(),
            );
            emit_msg(ctx, host_cb, MK_INFO, &msg);
            return 0;
        }

        let mut fetched_vols: Vec<Box<[u8]>> = Vec::new();
        let mut left = block_needed;
        while left > 0 && !volumes.is_empty() && !Self::is_stopped(ctx, host_cb) {
            let vol_blocks: Vec<i32> = volumes.iter().map(|v| v.1).collect();
            let pick_idx = pick_twin_volume(&vol_blocks, left).unwrap_or(volumes.len() - 1);
            let (entry, blocks) = volumes.remove(pick_idx);
            let mut msg = b"Fetching ".to_vec();
            msg.extend_from_slice(entry.filename.as_bytes());
            msg.extend_from_slice(b" of twin ");
            msg.extend_from_slice(&twin_name);
            msg.extend_from_slice(format!(" for par-repair ({blocks} block(s))").as_bytes());
            emit_msg(ctx, host_cb, MK_INFO, &msg);
            if let Some((path, _)) = self.fetch_twin_file(ctx, host_cb, &entry) {
                fetched_vols.push(to_c_buf(&path).into_boxed_slice());
                left -= blocks;
            }
        }
        let len = fetched_vols.len();
        self.lock_state().last_twin_vols = fetched_vols;
        len
    }
}

/// Unified C-ABI dispatch entry point for [`NzbgetRsRepairController`]:
/// - `op == 0`: `Stop()`
/// - `op == 1`: `Run()`
/// - `op == 2`: `AddPar(file_ptr=p1, deleted=a1 != 0)` -> returns `same_collection` (`0`/`1`)
/// - `op == 3`: `ParCheckCompleted()`
/// - `op == 4`: `RequestMorePars(block_needed=a1)` -> writes `*p1` (`int* blockFoundOut`), returns `0`/`1`
/// - `op == 5`: `UpdateParCheckProgress()` (including `CheckPauseState`)
/// - `op == 6`: `FindFileCrc(filename=s1, crc=p1 as *mut u32, segments=p2)` -> returns `ParChecker::EFileStatus`
/// - `op == 7`: `FindFileOrigname(filename=s1)` -> writes `*out_str` (`nullptr` if none)
/// - `op == 8`: `RequestDupeSources(dupeSourceList=p1)`
/// - `op == 9`: `RequestTwinIndex()` -> writes `*out_str`
/// - `op == 10`: `RequestTwinVolumes(block_needed=a1)` -> returns fetched volume count
/// - `op == 11`: get fetched twin volume path at `a1` -> writes `*out_str`
/// - `op == 12`: `StatDupeSources(dupeSourceList=p1)`
/// - `op == 13`: `RegisterVerifiedFile(filename=s1)`
/// - `op == 14`: `IsParredFile(filename=s1)` -> returns `0`/`1`
#[allow(clippy::too_many_arguments)]
pub unsafe fn repair_ctrl_exec(
    ctrl: *const NzbgetRsRepairController,
    op: c_int,
    a1: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    out_str: *mut *const c_char,
    host_cb: Option<NzbgetRsRepairCtrlHostFn>,
    ctx: *mut c_void,
) -> c_int {
    let Some(c) = (unsafe { ctrl.as_ref() }) else {
        return 0;
    };
    match op {
        0 => {
            c.stop();
            0
        }
        1 => {
            if let Some(cb) = host_cb {
                c.run(ctx, cb);
            }
            0
        }
        2 => {
            if let Some(cb) = host_cb {
                c_int::from(c.add_par(ctx, cb, p1, a1 != 0))
            } else {
                0
            }
        }
        3 => {
            if let Some(cb) = host_cb {
                c.par_check_completed(ctx, cb);
            }
            0
        }
        4 => {
            let Some(cb) = host_cb else {
                return 0;
            };
            let (ok, found) = c.request_more_pars(ctx, cb, a1);
            if p1 != 0 {
                unsafe { *(p1 as *mut c_int) = found };
            }
            c_int::from(ok)
        }
        5 => {
            if let Some(cb) = host_cb {
                c.update_par_check_progress(ctx, cb);
            }
            0
        }
        6 => {
            let Some(cb) = host_cb else {
                return 0;
            };
            let fname = unsafe { ptr_bytes(s1) };
            c.find_file_crc(ctx, cb, &fname, p1 as *mut u32, p2)
        }
        7 => {
            let Some(cb) = host_cb else {
                return 0;
            };
            let fname = unsafe { ptr_bytes(s1) };
            let ptr_val = c.find_file_origname(ctx, cb, &fname);
            if !out_str.is_null() {
                unsafe { *out_str = ptr_val };
            }
            c_int::from(!ptr_val.is_null())
        }
        8 => {
            if let Some(cb) = host_cb {
                c.request_dupe_sources(ctx, cb, p1);
            }
            0
        }
        9 => {
            let Some(cb) = host_cb else {
                return 0;
            };
            let ptr_val = c.request_twin_index(ctx, cb);
            if !out_str.is_null() {
                unsafe { *out_str = ptr_val };
            }
            0
        }
        10 => {
            let Some(cb) = host_cb else {
                return 0;
            };
            c.request_twin_volumes(ctx, cb, a1) as c_int
        }
        11 => {
            let idx = a1.max(0) as usize;
            if !out_str.is_null() {
                let st = c.lock_state();
                unsafe {
                    *out_str = st
                        .last_twin_vols
                        .get(idx)
                        .map_or(ptr::null(), |v| v.as_ptr() as *const c_char);
                }
            }
            0
        }
        12 => {
            if let Some(cb) = host_cb {
                c.stat_dupe_sources(ctx, cb, p1);
            }
            0
        }
        13 => {
            if let Some(cb) = host_cb {
                let fname = unsafe { ptr_bytes(s1) };
                c.register_verified_file(ctx, cb, &fname);
            }
            0
        }
        14 => {
            let Some(cb) = host_cb else {
                return 0;
            };
            let fname = unsafe { ptr_bytes(s1) };
            c_int::from(c.is_parred_file(ctx, cb, &fname))
        }
        _ => 0,
    }
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
