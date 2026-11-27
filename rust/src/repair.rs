//! End-to-end Rust implementation of `RepairController` (`daemon/postprocess/Repair.cpp`).
//!
//! Replaces piecemeal FFI predicates and micro-helpers with cohesive domain functions
//! for PAR2 recovery volume selection, twin PAR2 index/volume evaluation, file CRC
//! status resolution, progress/stage transitions, and pause timestamp adjustment.

use crate::queuesubsys_host::NzbRef;
use crate::queuesubsys_host::FileRef;
use crate::cstr;
use crate::status::{DupeMode, NzbParStatus, ParCheckerStatus, PostStage};
use std::ffi::CStr;

pub use crate::postrepair::{check_par_time_limit, pick_twin_volume, select_more_pars};

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
    checker_status: ParCheckerStatus,
    cur_par_status: NzbParStatus,
    cur_dl_sec: i32,
    start_dl_sec: i32,
    cur_time: i64,
    par_time: i64,
) -> (NzbParStatus, bool, bool, i32, i32) {
    use ParCheckerStatus::{RepairNotNeeded, RepairPossible, Repaired};
    let (new_par_status, set_par_repaired, par_repaired_val) =
        if matches!(checker_status, Repaired | RepairNotNeeded)
            && cur_par_status <= NzbParStatus::Skipped
        {
            (NzbParStatus::Success, true, checker_status == Repaired)
        } else if checker_status == RepairPossible && cur_par_status != NzbParStatus::Failure {
            (NzbParStatus::RepairPossible, false, false)
        } else {
            (NzbParStatus::Failure, false, false)
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
    cur_post_stage: PostStage,
    current_time: i64,
    stage_time: i64,
    repair_time: i64,
    cur_repair_sec: i32,
    is_stopped: bool,
    par_time_limit_min: i32,
    is_par_repairing_stage: bool,
    stage_progress: i32,
) -> (PostStage, bool, bool, i32, bool, i32, i32) {
    let new_post_stage = match par_stage {
        1 => PostStage::VerifyingSources,
        2 => PostStage::Repairing,
        3 => PostStage::VerifyingRepaired,
        _ => PostStage::LoadingPars,
    };
    let stage_changed = cur_post_stage != new_post_stage;
    let set_repair_time = stage_changed && new_post_stage == PostStage::Repairing;
    let new_repair_sec = if stage_changed && new_post_stage == PostStage::VerifyingRepaired {
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

use std::ffi::c_int;
use std::path::Path;
use std::ptr;
use std::time::Duration;

use crate::articlefetcher::{fetch_entry, ArticleFetcher, NntpHost};
use crate::twin::{
    block_size as par2_block_size, fingerprint as twin_fingerprint, par2_sigs, read_nzb, NzbEntry,
    MAX_INDEX_SIZE,
};

use crate::log::{MK_DEBUG, MK_DETAIL, MK_ERROR, MK_INFO, MK_WARNING};
use crate::options::OptStr;
#[cfg(unix)]
use crate::postproc::JobQueue;
use crate::queuesubsys_host::{
    QueueSubsysHost, FILE_INT_EXTRA_PRIORITY, FILE_INT_PAUSED, HK_NZB, NK_NZB,
    NZB_INT_EXTRA_PAR_BLOCKS, NZB_INT_PAR_FULL, NZB_INT_PAR_SEC, NZB_INT_REPAIR_SEC,
    POST_INT_FILE_PROGRESS, POST_INT_PAR_REPAIRED, POST_INT_STAGE_PROGRESS,
    POST_INT_WORKING,
};

const MAX_TWIN_FETCH_SIZE: i64 = 1024 * 1024 * 1024;

/// Message text as it used to reach the host: cut at the first NUL and at 1023 bytes.
#[inline]
fn msg_1023(bytes: &[u8]) -> &[u8] {
    cstr::until_nul(&bytes[..bytes.len().min(1023)])
}

struct CtrlSnapshot {
    nzb_ptr: NzbRef,
    checker_status: ParCheckerStatus,
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
    post_stage: PostStage,
    par_buffer: i32,
    has_memory_monitor: bool,
    par_threads: i32,
    par_scan: i32,
    par_repair: bool,
    cur_time: i64,
    block_size: i64,
    post_stage_time: i64,
    post_start_time: i64,
    progress_label: Vec<u8>,
    info_name: Vec<u8>,
    par_filename: Vec<u8>,
    checker_dest_dir: Vec<u8>,
    unrar_cmd: Vec<u8>,
    par_ignore_ext: Vec<u8>,
}

struct NzbSnapshot {
    id: i32,
    kind: i32,
    nk_nzb: i32,
    download_sec: i32,
    repair_sec: i32,
    par_status: NzbParStatus,
    reprocess: bool,
    force_priority: bool,
    extra_par_blocks: i32,
    file_list_len: usize,
    completed_len: usize,
    jobs_len: usize,
    dupe_mode: DupeMode,
    dupe_recovered_bytes: i64,
    expected_data_size: i64,
    name: Vec<u8>,
    dest_dir: Vec<u8>,
    dupe_key: Vec<u8>,
    queued_filename: Vec<u8>,
    has_twin_files_param: bool,
    twin_files_param: Vec<u8>,
}

/// One file of an nzb's file list, as `find_pars_pass` sees it.
struct FileEntry {
    ptr: FileRef,
    confirmed: bool,
    paused: bool,
    par_file: bool,
    size: i64,
    filename: Vec<u8>,
}

/// One completed file of an nzb.
struct CompletedEntry {
    id: i32,
    status: i32,
    crc: u32,
    filename: Vec<u8>,
    origname: Vec<u8>,
}

#[cfg(unix)]
fn inspect_ctrl(par_checker: &crate::postproc::NzbgetRsParChecker, q: JobQueue<'_>) -> CtrlSnapshot {
    let ps = q.host.post_snap(q.post_ptr());
    let opts = crate::options::active_options();
    let sc = opts.as_ref().map(|o| o.scalars());
    let opt = |id| cstr::until_nul(opts.as_ref().map_or(&b""[..], |o| o.str_bytes(id))).to_vec();
    CtrlSnapshot {
        nzb_ptr: ps.nzb_ptr,
        checker_status: par_checker.status(),
        checker_stage: par_checker.stage(),
        file_progress: par_checker.file_progress(),
        stage_progress: par_checker.stage_progress(),
        par_full: par_checker.par_full(),
        is_stopped: q.is_stopped(),
        par_quick_opt: sc.as_ref().is_some_and(|s| s.par_quick != 0),
        par_time_limit: sc.as_ref().map_or(0, |s| s.par_time_limit),
        pause_post_process: crate::ffi::active_workstate()
            .as_ref()
            .is_some_and(|w| w.pause_post_process()),
        force_par_full: ps.force_par_full,
        force_repair: ps.force_repair,
        post_stage: ps.stage,
        par_buffer: sc.as_ref().map_or(0, |s| s.par_buffer),
        has_memory_monitor: crate::articledl::memory_monitor_active(),
        par_threads: sc.as_ref().map_or(0, |s| s.par_threads),
        par_scan: sc.as_ref().map_or(0, |s| s.par_scan),
        par_repair: sc.as_ref().is_some_and(|s| s.par_repair != 0),
        cur_time: crate::postproc::current_time_sec(),
        block_size: par_checker.block_size(),
        post_stage_time: ps.stage_time,
        post_start_time: ps.start_time,
        progress_label: par_checker.progress_label(),
        info_name: par_checker.info_name(),
        par_filename: par_checker.par_filename(),
        checker_dest_dir: par_checker.dest_dir(),
        unrar_cmd: opt(OptStr::UnrarCmd),
        par_ignore_ext: opt(OptStr::ParIgnoreExt),
    }
}

#[cfg(unix)]
fn inspect_nzb(host: &QueueSubsysHost, nzb_ptr: NzbRef) -> NzbSnapshot {
    let ns = host.nzb_snap(nzb_ptr, false);
    let twin_files = host.nzb_get_param(nzb_ptr, b"DupeFiles");
    NzbSnapshot {
        id: ns.id,
        kind: ns.kind,
        nk_nzb: NK_NZB,
        download_sec: ns.download_sec,
        repair_sec: ns.repair_sec,
        par_status: ns.par_status,
        reprocess: ns.reprocess,
        force_priority: ns.force_priority,
        extra_par_blocks: ns.extra_par_blocks,
        file_list_len: host.nzb_file_len(nzb_ptr),
        completed_len: host.nzb_completed_files_len(nzb_ptr),
        jobs_len: host.nzb_stream_repair_jobs_len(nzb_ptr),
        dupe_mode: ns.dupe_mode,
        dupe_recovered_bytes: ns.dupe_recovered_bytes,
        expected_data_size: ns.size - ns.par_size,
        name: cstr::until_nul(&ns.name).to_vec(),
        dest_dir: cstr::until_nul(&ns.dest_dir).to_vec(),
        dupe_key: cstr::until_nul(&ns.dupe_key).to_vec(),
        queued_filename: cstr::until_nul(&ns.queued_filename).to_vec(),
        has_twin_files_param: twin_files.is_some(),
        twin_files_param: twin_files.as_deref().map_or_else(Vec::new, |t| cstr::until_nul(t).to_vec()),
    }
}

#[cfg(unix)]
fn file_entry(host: &QueueSubsysHost, nzb_ptr: NzbRef, idx: usize) -> FileEntry {
    let ptr = host.nzb_file_at(nzb_ptr, idx);
    let fs = host.file_snap(ptr);
    FileEntry {
        ptr,
        confirmed: fs.filename_confirmed,
        paused: fs.paused,
        par_file: fs.par_file,
        size: fs.size,
        filename: cstr::until_nul(&fs.filename).to_vec(),
    }
}

/// Unpauses the file at `idx` of the nzb and gives it extra priority.
#[cfg(unix)]
fn unpause_file(host: &QueueSubsysHost, nzb_ptr: NzbRef, idx: usize) {
    let ptr = host.nzb_file_at(nzb_ptr, idx);
    host.file_set_int(ptr, FILE_INT_PAUSED, 0);
    host.file_set_int(ptr, FILE_INT_EXTRA_PRIORITY, 1);
}

#[cfg(unix)]
fn completed_entry(host: &QueueSubsysHost, nzb_ptr: NzbRef, idx: usize) -> CompletedEntry {
    let cf = host.nzb_completed_file_at(nzb_ptr, idx);
    CompletedEntry {
        id: cf.id,
        status: cf.status,
        crc: cf.crc,
        filename: cstr::until_nul(&cf.filename).to_vec(),
        origname: cstr::until_nul(&cf.origname).to_vec(),
    }
}

/// The nzb at history index `idx`, or [`NzbRef::NONE`] when that history entry is not an nzb.
#[cfg(unix)]
fn history_nzb(host: &QueueSubsysHost, dq_ptr: usize, idx: usize) -> NzbRef {
    let h = host.history_at(dq_ptr, idx);
    if h.kind == HK_NZB {
        h.nzb()
    } else {
        NzbRef::NONE
    }
}

/// A message for the job's nzb (`kind` 0..=4), or the old host's other message codes:
/// 5 debug log, 6 stop the job, 7 info log, 8 detail log.
#[cfg(unix)]
fn emit_msg(q: JobQueue<'_>, kind: c_int, text: &[u8]) {
    let msg = msg_1023(text);
    match kind {
        ..=4 => q.job_message(kind, msg),
        5 => q.host.log(MK_DEBUG, msg),
        6 => q.stop(),
        7 => q.host.log(MK_INFO, msg),
        8 => q.host.log(MK_DETAIL, msg),
        _ => {}
    }
}

#[cfg(unix)]
fn emit_debug(q: JobQueue<'_>, text: &[u8]) {
    q.host.log(MK_DEBUG, cstr::until_nul(text));
}

/// Global log only; message kinds other than info, debug and detail are dropped.
#[cfg(unix)]
fn emit_global_log(q: JobQueue<'_>, kind: c_int, text: &[u8]) {
    let kind = match kind {
        0 => MK_INFO,
        3 => MK_DEBUG,
        4 => MK_DETAIL,
        _ => return,
    };
    q.host.log(kind, cstr::until_nul(text));
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
}

pub struct NzbgetRsRepairController {
    twin_fetcher: ArticleFetcher,
    par_checker: crate::postproc::NzbgetRsParChecker,
    state: std::sync::Mutex<RepairCtrlState>,
}

#[cfg(unix)]
impl NzbgetRsRepairController {
    pub fn new(nntp_host: &'static dyn NntpHost) -> Self {
        Self {
            twin_fetcher: ArticleFetcher::new(nntp_host),
            par_checker: crate::postproc::NzbgetRsParChecker::new(),
            state: std::sync::Mutex::new(RepairCtrlState::default()),
        }
    }

    #[inline]
    fn lock_state(&self) -> std::sync::MutexGuard<'_, RepairCtrlState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[inline]
    fn snap(&self, q: JobQueue<'_>) -> CtrlSnapshot {
        inspect_ctrl(&self.par_checker, q)
    }

    pub fn stop(&self) {
        self.twin_fetcher.stop();
        self.par_checker.cancel();
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
            let c_path = cstr::nul_terminated(path);
            if let Ok(cs) = CStr::from_bytes_with_nul(&c_path) {
                crate::fsops::delete_file(cs);
            }
        }
    }

    fn is_stopped(&self, q: JobQueue<'_>) -> bool {
        q.is_stopped()
    }

    pub(crate) fn run(&self, q: JobQueue<'_>) {
        let (
            nzb_name,
            dest_dir,
            dupe_repaired,
            dl_sec,
            par_quick_opt,
            force_par_full,
            force_repair,
            cur_time,
        ) = q.host.with_locked_queue(|_dq| {
            let ctrl = self.snap(q);
            let nzb = inspect_nzb(q.host, ctrl.nzb_ptr);
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
        self.par_checker.set_dest_dir(dest_dir);
        self.par_checker.set_nzb_name(nzb_name.clone());
        self.par_checker.set_par_quick(par_quick);
        self.par_checker.set_force_repair(force_repair);
        let mut msg = b"Checking pars for ".to_vec();
        msg.extend_from_slice(&nzb_name);
        emit_msg(q, MK_INFO, &msg);
        let hooks = RepairParHooks { ctrl: self, q };
        self.par_checker.execute(&hooks);
    }

    /// A par file of the queue was downloaded (or deleted). Returns whether it belongs to
    /// this job's nzb.
    pub(crate) fn add_par(&self, q: JobQueue<'_>, file_ptr: FileRef, deleted: bool) -> bool {
        let fs = q.host.file_snap(file_ptr);
        let ps = q.host.post_snap(q.post_ptr());
        let same_collection = fs.nzb_ptr == ps.nzb_ptr;
        if same_collection && !deleted {
            let ns = q.host.nzb_snap(fs.nzb_ptr, false);
            let full = crate::artwritercore::article_writer_format_dest_file_path(
                cstr::until_nul(&ns.dest_dir),
                crate::paths::PATH_SEPARATOR,
                cstr::until_nul(&fs.filename),
            );
            self.par_checker.add_par_file(full);
        } else {
            self.par_checker.queue_changed();
        }
        same_collection
    }

    fn par_check_completed(&self, q: JobQueue<'_>) {
        self.delete_twin_files();
        let (start_dl_sec, par_time) = {
            let st = self.lock_state();
            (st.download_sec, st.par_time)
        };
        q.host.with_locked_queue(|_dq| {
            let ctrl = self.snap(q);
            let nzb = inspect_nzb(q.host, ctrl.nzb_ptr);
            let (new_status, set_rep, rep_val, wait_time, par_sec) = complete_par_check(
                ctrl.checker_status,
                nzb.par_status,
                nzb.download_sec,
                start_dl_sec,
                ctrl.cur_time,
                par_time,
            );
            let host = q.host;
            let n = ctrl.nzb_ptr;
            let pi = q.post_ptr();
            let ps = host.post_snap(pi);
            let ns = host.nzb_snap(n, false);
            host.nzb_set_par_status(n, new_status);
            if set_rep {
                host.post_set_int(pi, POST_INT_PAR_REPAIRED, i32::from(rep_val));
            }
            host.post_set_start_time(pi, ps.start_time + i64::from(wait_time));
            host.nzb_set_int(n, NZB_INT_PAR_SEC, ns.par_sec + par_sec);
            host.nzb_set_int(n, NZB_INT_PAR_FULL, i32::from(ctrl.par_full));
            host.post_set_int(pi, POST_INT_WORKING, 0);
        });
    }

    fn find_pars_pass(
        q: JobQueue<'_>,
        nzb_ptr: NzbRef,
        nzb_name: &[u8],
        file_list_len: usize,
        block_size: i64,
        par_filename: &[u8],
        strict_par_name: bool,
        exact_par_name: bool,
        blocks: &mut Vec<(usize, FileRef, i32, bool, Vec<u8>)>,
    ) -> i32 {
        let base_off = crate::paths::base_file_name(par_filename);
        let base_par = &par_filename[base_off..];
        let Some((main_base_len, _)) = crate::parparser::parse_par_filename(base_par, true) else {
            let mut msg = b"Internal error: could not parse filename ".to_vec();
            msg.extend_from_slice(base_par);
            emit_msg(q, MK_ERROR, &msg);
            let _ = nzb_name;
            return 0;
        };
        let main_base = &base_par[..main_base_len.min(base_par.len())];
        let mut found = 0i32;

        for idx in 0..file_list_len {
            let FileEntry {
                ptr: file_ptr,
                confirmed,
                paused,
                size: file_size,
                filename,
                ..
            } = file_entry(q.host, nzb_ptr, idx);

            let (parsed, raw_blocks) =
                match crate::parparser::parse_par_filename(&filename, confirmed) {
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

    fn request_more_pars(&self, q: JobQueue<'_>, block_needed: i32) -> (bool, i32) {
        q.host.with_locked_queue(|_dq| {
            let ctrl = self.snap(q);
            let nzb = inspect_nzb(q.host, ctrl.nzb_ptr);
            let mut available: Vec<(usize, FileRef, i32, bool, Vec<u8>)> = Vec::new();
            let mut block_found = 0;
            for (strict, exact) in [(true, true), (true, false), (false, false)] {
                if strict && exact || block_found < block_needed {
                    block_found += Self::find_pars_pass(
                        q,
                        ctrl.nzb_ptr,
                        &nzb.name,
                        nzb.file_list_len,
                        ctrl.block_size,
                        &ctrl.par_filename,
                        strict,
                        exact,
                        &mut available,
                    );
                }
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
                        emit_msg(q, MK_INFO, &msg);
                        unpause_file(q.host, ctrl.nzb_ptr, file_idx);
                    }
                }
            }

            let has_unpaused_par = (0..nzb.file_list_len).any(|idx| {
                let f = file_entry(q.host, ctrl.nzb_ptr, idx);
                f.par_file && !f.paused
            });

            (rem_needed <= 0 || has_unpaused_par, block_found)
        })
    }

    fn update_par_check_progress(&self, q: JobQueue<'_>) {
        let cur_repair_time = self.lock_state().repair_time;
        let mut updated_repair_time = cur_repair_time;
        q.host.with_locked_queue(|_dq| {
            let ctrl = self.snap(q);
            let nzb = inspect_nzb(q.host, ctrl.nzb_ptr);
            let (new_stage, changed, set_rt, new_rsec, cancel, est_sec, est_min) =
                eval_par_progress_update(
                    ctrl.checker_stage,
                    ctrl.post_stage,
                    ctrl.cur_time,
                    ctrl.post_stage_time,
                    cur_repair_time,
                    nzb.repair_sec,
                    ctrl.is_stopped,
                    ctrl.par_time_limit,
                    ctrl.checker_stage == 2,
                    ctrl.stage_progress,
                );
            let mut set_repair_sec = false;
            if changed {
                if set_rt {
                    updated_repair_time = ctrl.cur_time;
                } else if new_stage == PostStage::VerifyingRepaired {
                    set_repair_sec = true;
                }
            }

            let host = q.host;
            let pi = q.post_ptr();
            if ctrl.file_progress == 0 {
                host.post_set_progress_label(pi, cstr::until_nul(&ctrl.progress_label));
            }
            host.post_set_int(pi, POST_INT_FILE_PROGRESS, ctrl.file_progress);
            host.post_set_int(pi, POST_INT_STAGE_PROGRESS, ctrl.stage_progress);
            if changed {
                host.post_set_stage(pi, new_stage);
                host.post_set_stage_time(pi, ctrl.cur_time);
            }
            if set_repair_sec {
                let n = host.post_snap(pi).nzb_ptr;
                host.nzb_set_int(n, NZB_INT_REPAIR_SEC, new_rsec);
            }

            if cancel {
                emit_debug(q, format!("Estimated repair time {est_sec} seconds").as_bytes());
                let mut msg = b"Cancelling par-repair for ".to_vec();
                msg.extend_from_slice(&ctrl.info_name);
                msg.extend_from_slice(
                    format!(
                        ", estimated repair time ({est_min} minutes) exceeds allowed repair time"
                    )
                    .as_bytes(),
                );
                emit_msg(q, MK_WARNING, &msg);
                q.stop();
            }
        });
        if updated_repair_time != cur_repair_time {
            self.lock_state().repair_time = updated_repair_time;
        }
        self.check_pause_state(q);
    }

    fn check_pause_state(&self, q: JobQueue<'_>) {
        let ctrl0 = self.snap(q);
        let nzb0 = inspect_nzb(q.host, ctrl0.nzb_ptr);
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
            let c_check = self.snap(q);
            let n_check = inspect_nzb(q.host, c_check.nzb_ptr);
            if !c_check.pause_post_process || n_check.force_priority || c_check.is_stopped {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            let c_after = self.snap(q);
            let (s_stage, s_start, s_par, s_rep) = calc_paused_timestamps(
                c_after.cur_time,
                wait_time,
                stage_time,
                start_time,
                par_time,
                repair_time,
            );
            let pi = q.post_ptr();
            if s_stage > 0 {
                q.host.post_set_stage_time(pi, s_stage);
            }
            if s_start > 0 {
                q.host.post_set_start_time(pi, s_start);
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

    fn register_verified_file(&self, q: JobQueue<'_>, filename: &[u8]) {
        let verified_path =
            crate::fsops::get_real_path(filename).unwrap_or_else(|| filename.to_vec());
        q.host.with_locked_queue(|_dq| {
            let ctrl = self.snap(q);
            let nzb = inspect_nzb(q.host, ctrl.nzb_ptr);
            for job_idx in (0..nzb.jobs_len).rev() {
                let job = q.host.nzb_stream_repair_job_detail(ctrl.nzb_ptr, job_idx);
                let job_file_id = job.file_id;
                let mut current_name = cstr::until_nul(&job.filename).to_vec();
                for cf_idx in 0..nzb.completed_len {
                    let cf = completed_entry(q.host, ctrl.nzb_ptr, cf_idx);
                    if cf.id == job_file_id {
                        current_name = cf.filename;
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
                    q.host.nzb_stream_repair_erase_job(ctrl.nzb_ptr, job_idx, true);
                }
            }
        });
    }

    fn is_parred_file(&self, q: JobQueue<'_>, filename: &[u8]) -> bool {
        let post_ptr = q.post_ptr();
        q.host
            .post_parred_files(post_ptr)
            .iter()
            .any(|pf| pf.eq_ignore_ascii_case(filename))
    }

    fn find_file_quick_info(
        &self,
        q: JobQueue<'_>,
        filename: &[u8],
    ) -> Option<crate::parchecker::QuickInfo> {
        let found = q.host.with_locked_queue(|_dq| {
            let ctrl = self.snap(q);
            let nzb = inspect_nzb(q.host, ctrl.nzb_ptr);
            (0..nzb.completed_len)
                .map(|cf_idx| completed_entry(q.host, ctrl.nzb_ptr, cf_idx))
                .find(|cf| cf.filename.eq_ignore_ascii_case(filename))
                .map(|cf| (cf.id, cf.status, cf.crc, nzb.reprocess, nzb.id, nzb.dest_dir.clone()))
        });

        let (id, status, crc, reprocess, nzb_id, dest_dir) = found?;

        if !reprocess {
            if let Some(crc) = crate::parchecker::streaming_session_verified_target_crc(
                nzb_id,
                crate::fsops::path(&dest_dir),
                id,
                filename,
            ) {
                return Some(crate::parchecker::QuickInfo::Success { crc });
            }
        }

        let mut dbg = b"Found completed file: ".to_vec();
        dbg.extend_from_slice(filename);
        dbg.extend_from_slice(format!(", CRC: {crc:08x}, Status: {status}").as_bytes());
        emit_debug(q, &dbg);

        let mut raw_segs: Vec<crate::postrepair::NzbgetRsParSegment> = Vec::new();
        extern "C" fn push_seg(vec_ptr: usize, s: c_int, off: i64, sz: c_int, c: u32) {
            let v = unsafe { &mut *(vec_ptr as *mut Vec<crate::postrepair::NzbgetRsParSegment>) };
            v.push(crate::postrepair::NzbgetRsParSegment {
                success: s,
                offset: off,
                size: sz,
                crc: c,
            });
        }

        let mut has_segments = false;
        if should_load_partial_segments(status, 2, id, reprocess) {
            let loaded = q.host.disk_load_completed_segments(
                id,
                (&mut raw_segs as *mut Vec<crate::postrepair::NzbgetRsParSegment>) as usize,
                push_seg as *const () as usize,
            );
            if loaded == 0 {
                return None;
            }
            has_segments = loaded >= 2;
        }

        let st = classify_file_crc_status(status, reprocess, has_segments, 1, 3, 2, 1, 3, 2, 0);
        crate::postproc::quick_info_from_status(st, crc, &raw_segs)
    }

    fn find_file_origname(&self, q: JobQueue<'_>, filename: &[u8]) -> Option<Vec<u8>> {
        q.host.with_locked_queue(|_dq| {
            let ctrl = self.snap(q);
            let nzb = inspect_nzb(q.host, ctrl.nzb_ptr);
            (0..nzb.completed_len)
                .map(|cf_idx| completed_entry(q.host, ctrl.nzb_ptr, cf_idx))
                .find(|cf| cf.filename.eq_ignore_ascii_case(filename))
                .map(|cf| cf.origname)
        })
    }

    fn request_dupe_sources(&self, q: JobQueue<'_>) -> Vec<crate::postproc::ParDupeSource> {
        let mut out = Vec::new();
        q.host.with_locked_queue(|dq_ptr| {
            let ctrl = self.snap(q);
            let nzb = inspect_nzb(q.host, ctrl.nzb_ptr);
            if nzb.dupe_mode == DupeMode::Force {
                return;
            }
            let hist_len = q.host.history_len(dq_ptr);
            let mut dupes: Vec<(i32, Vec<u8>)> = Vec::new();
            for h_idx in 0..hist_len {
                let item_ptr = history_nzb(q.host, dq_ptr, h_idx);
                if item_ptr.is_none() {
                    continue;
                }
                let item = inspect_nzb(q.host, item_ptr);
                if item.dupe_mode != DupeMode::Force
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
                    emit_msg(q, if kind == 3 { MK_DETAIL } else { MK_INFO }, msg);
                },
            );
            for u_idx in usable {
                let (dupe_id, ref dupe_dir) = dupes[u_idx];
                out.push(crate::postproc::ParDupeSource {
                    id: dupe_id,
                    used_blocks: 0,
                    directory: dupe_dir.clone(),
                });
            }
        });
        out
    }

    fn stat_dupe_sources(&self, q: JobQueue<'_>, sources: &[crate::postproc::ParDupeSource]) {
        q.host.with_locked_queue(|dq_ptr| {
            let ctrl = self.snap(q);
            let nzb = inspect_nzb(q.host, ctrl.nzb_ptr);
            let hist_len = q.host.history_len(dq_ptr);

            let mut total_extra = 0i32;
            for src in sources {
                if src.used_blocks > 0 {
                    for h_idx in 0..hist_len {
                        let item_ptr = history_nzb(q.host, dq_ptr, h_idx);
                        if item_ptr.is_some() {
                            let item = inspect_nzb(q.host, item_ptr);
                            if item.id == src.id {
                                q.host.nzb_set_int(
                                    item_ptr,
                                    NZB_INT_EXTRA_PAR_BLOCKS,
                                    item.extra_par_blocks - src.used_blocks,
                                );
                            }
                        }
                    }
                }
                total_extra += src.used_blocks;
            }
            q.host.nzb_set_int(
                ctrl.nzb_ptr,
                NZB_INT_EXTRA_PAR_BLOCKS,
                nzb.extra_par_blocks + total_extra,
            );
        });
    }

    fn fetch_twin_file(&self, q: JobQueue<'_>, entry: &NzbEntry) -> Option<(Vec<u8>, Vec<u8>)> {
        let data = fetch_entry(&self.twin_fetcher, entry, MAX_TWIN_FETCH_SIZE, || {
            self.is_stopped(q)
        })?;
        let ctrl = self.snap(q);
        let count = self.lock_state().twin_files.len();
        let name = build_twin_filename(entry.filename.as_bytes(), count);
        let path = crate::artwritercore::article_writer_format_dest_file_path(
            &ctrl.checker_dest_dir,
            crate::paths::PATH_SEPARATOR,
            &name,
        );
        let c_path = cstr::nul_terminated(&path);
        let cs_path = CStr::from_bytes_with_nul(&c_path).ok()?;
        if !crate::fsops::save_buffer_into_file(cs_path, &data) {
            return None;
        }
        self.lock_state().twin_files.push(path.clone());
        Some((path, data))
    }

    fn request_twin_index(&self, q: JobQueue<'_>) -> Vec<u8> {
        let ctrl0 = self.snap(q);
        let c_par_fn = cstr::nul_terminated(&ctrl0.par_filename);
        let mut par_buf = Vec::new();
        if let Ok(cs) = CStr::from_bytes_with_nul(&c_par_fn) {
            crate::fsops::load_file_into_buffer(cs, false, &crate::fsops::posix_stat, |buf_size| {
                if buf_size <= 0 {
                    ptr::null_mut()
                } else {
                    par_buf.resize(buf_size as usize, 0);
                    par_buf.as_mut_ptr()
                }
            });
        }
        let own = if par_buf.is_empty() {
            Vec::new()
        } else {
            par2_sigs(&par_buf)
        };
        let own_print = twin_fingerprint(&own);
        if own_print.is_empty() {
            return Vec::new();
        }

        let mut candidates: Vec<(Vec<u8>, Vec<u8>, bool)> = q.host.with_locked_queue(|dq_ptr| {
            let ctrl = self.snap(q);
            let nzb = inspect_nzb(q.host, ctrl.nzb_ptr);
            let q_len = q.host.queue_len(dq_ptr);
            let h_len = q.host.history_len(dq_ptr);
            let mut out = Vec::new();

            let mut consider = |other_ptr: NzbRef| {
                if other_ptr.is_none() {
                    return;
                }
                let other = inspect_nzb(q.host, other_ptr);
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
                ) || !crate::fsops::file_exists(
                    &other.queued_filename,
                    &crate::fsops::posix_stat,
                ) {
                    return;
                }
                out.push((other.name, other.queued_filename, has_print));
            };

            for idx in 0..q_len {
                consider(q.host.queue_at(dq_ptr, idx));
            }
            for idx in 0..h_len {
                consider(history_nzb(q.host, dq_ptr, idx));
            }
            out
        });

        candidates.sort_by_key(|c| std::cmp::Reverse(c.2));

        let own_names: Vec<&[u8]> = own.iter().map(|s| s.name.as_slice()).collect();
        let own_md5s: Vec<[u8; 16]> = own.iter().map(|s| s.md5).collect();

        for (cand_name, cand_nzb_fn, _) in candidates {
            if self.is_stopped(q) {
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
            let Some((path, data)) = self.fetch_twin_file(q, &index_entry) else {
                continue;
            };
            let cmp =
                par_rs::twin::compare_twin_par2_data(&own_names, &own_md5s, &own_print, &data)
                    .code();
            if cmp != 0 {
                let reason = twin_par2_reject_reason(cmp);
                let reason_end = reason.iter().position(|&b| b == 0).unwrap_or(reason.len());
                let mut msg = cand_name.clone();
                msg.extend_from_slice(b": its par2 set ");
                msg.extend_from_slice(&reason[..reason_end]);
                emit_msg(q, MK_DETAIL, &msg);
                let c_del = cstr::nul_terminated(&path);
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
            emit_msg(q, MK_INFO, &msg);

            let mut st = self.lock_state();
            st.twin_name = cand_name;
            st.twin_entries = entries;
            st.twin_index = index_entry.filename.into_bytes();
            st.twin_block_size = blk_sz;
            return path;
        }

        Vec::new()
    }

    fn request_twin_volumes(&self, q: JobQueue<'_>, block_needed: i32) -> Vec<Vec<u8>> {
        let (twin_entries, twin_index, twin_block_size, twin_name) = {
            let st = self.lock_state();
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
            let mut blocks = par_rs::filenames::volume_blocks_bytes(entry.filename.as_bytes())
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
            emit_msg(q, MK_INFO, &msg);
            return Vec::new();
        }

        let mut fetched_vols: Vec<Vec<u8>> = Vec::new();
        let mut left = block_needed;
        while left > 0 && !volumes.is_empty() && !self.is_stopped(q) {
            let vol_blocks: Vec<i32> = volumes.iter().map(|v| v.1).collect();
            let pick_idx = pick_twin_volume(&vol_blocks, left).unwrap_or(volumes.len() - 1);
            let (entry, blocks) = volumes.remove(pick_idx);
            let mut msg = b"Fetching ".to_vec();
            msg.extend_from_slice(entry.filename.as_bytes());
            msg.extend_from_slice(b" of twin ");
            msg.extend_from_slice(&twin_name);
            msg.extend_from_slice(format!(" for par-repair ({blocks} block(s))").as_bytes());
            emit_msg(q, MK_INFO, &msg);
            if let Some((path, _)) = self.fetch_twin_file(q, &entry) {
                fetched_vols.push(path);
                left -= blocks;
            }
        }
        fetched_vols
    }
}

#[cfg(unix)]
struct RepairParHooks<'a> {
    ctrl: &'a NzbgetRsRepairController,
    q: JobQueue<'a>,
}

#[cfg(unix)]
impl crate::postproc::ParCheckerHooks for RepairParHooks<'_> {
    fn read_options(&self) -> crate::postproc::ParCheckerOptions {
        let ctrl = self.ctrl.snap(self.q);
        crate::postproc::ParCheckerOptions {
            par_buffer_mb: ctrl.par_buffer,
            has_memory_monitor: ctrl.has_memory_monitor,
            par_threads: ctrl.par_threads,
            par_scan: ctrl.par_scan,
            par_repair: ctrl.par_repair,
            par_ignore_ext: ctrl.par_ignore_ext,
        }
    }

    fn is_stopped(&self) -> bool {
        self.ctrl.is_stopped(self.q)
    }

    fn known_damaged_bytes(&self) -> u64 {
        self.q.host.with_locked_queue(|_dq| {
            let ctrl = self.ctrl.snap(self.q);
            let ns = self.q.host.nzb_snap(ctrl.nzb_ptr, false);
            let lost = ns.failed_size - ns.par_failed_size - ns.dupe_recovered_bytes;
            u64::try_from(lost).unwrap_or(0)
        })
    }

    fn request_more_pars(&self, block_needed: i32) -> (bool, i32) {
        self.ctrl.request_more_pars(self.q, block_needed)
    }

    fn update_progress(&self) {
        self.ctrl.update_par_check_progress(self.q);
    }

    fn completed(&self) {
        self.ctrl.par_check_completed(self.q);
    }

    fn print_message(&self, kind: i32, msg: &[u8]) {
        emit_msg(self.q, kind, msg);
    }

    fn global_log(&self, kind: i32, msg: &[u8]) {
        emit_global_log(self.q, kind, msg);
    }

    fn register_parred_file(&self, filename: &[u8]) {
        self.q
            .host
            .post_push_parred_file(self.q.post_ptr(), cstr::until_nul(filename));
    }

    fn register_verified_file(&self, filename: &[u8]) {
        self.ctrl.register_verified_file(self.q, filename);
    }

    fn is_parred_file(&self, filename: &[u8]) -> bool {
        self.ctrl.is_parred_file(self.q, filename)
    }

    fn find_file_quick_info(&self, filename: &[u8]) -> Option<crate::parchecker::QuickInfo> {
        self.ctrl.find_file_quick_info(self.q, filename)
    }

    fn find_file_origname(&self, filename: &[u8]) -> Option<Vec<u8>> {
        self.ctrl.find_file_origname(self.q, filename)
    }

    fn request_dupe_sources(&self) -> Vec<crate::postproc::ParDupeSource> {
        self.ctrl.request_dupe_sources(self.q)
    }

    fn stat_dupe_sources(&self, sources: &[crate::postproc::ParDupeSource]) {
        self.ctrl.stat_dupe_sources(self.q, sources);
    }

    fn note_repaired(&self, repaired: u64, available: u64) {
        crate::repairsummary::note_post(self.q.nzb_id(), repaired, available);
    }

    fn request_twin_index(&self) -> Vec<u8> {
        self.ctrl.request_twin_index(self.q)
    }

    fn request_twin_volumes(&self, block_needed: i32) -> Vec<Vec<u8>> {
        self.ctrl.request_twin_volumes(self.q, block_needed)
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
        let (status, set_rep, rep_val, wait, par_sec) = complete_par_check(
            ParCheckerStatus::Repaired,
            NzbParStatus::None,
            30,
            10,
            1100,
            1000,
        );
        assert_eq!(status, NzbParStatus::Success);
        assert!(set_rep);
        assert!(rep_val);
        assert_eq!(wait, 20);
        assert_eq!(par_sec, 80);
        let outcome = |checker, cur| {
            let (status, set_rep, rep_val, _, _) = complete_par_check(checker, cur, 0, 0, 0, 0);
            (status, set_rep, rep_val)
        };
        use ParCheckerStatus as C;
        assert_eq!(
            outcome(C::RepairNotNeeded, NzbParStatus::Skipped),
            (NzbParStatus::Success, true, false)
        );
        assert_eq!(
            outcome(C::Repaired, NzbParStatus::Failure),
            (NzbParStatus::Failure, false, false)
        );
        assert_eq!(
            outcome(C::RepairPossible, NzbParStatus::None),
            (NzbParStatus::RepairPossible, false, false)
        );
        assert_eq!(
            outcome(C::RepairPossible, NzbParStatus::Failure),
            (NzbParStatus::Failure, false, false)
        );
        assert_eq!(
            outcome(C::Failed, NzbParStatus::None),
            (NzbParStatus::Failure, false, false)
        );

        let (new_stage, changed, set_rt, new_rsec, cancel, est_sec, est_min) =
            eval_par_progress_update(
                2,
                PostStage::LoadingPars,
                2000,
                1000,
                0,
                5,
                false,
                10,
                false,
                500,
            );
        assert_eq!(new_stage, PostStage::Repairing);
        assert!(changed);
        assert!(set_rt);
        assert_eq!(new_rsec, 5);
        assert!(!cancel);
        assert_eq!((est_sec, est_min), (0, 0));
    }
}
