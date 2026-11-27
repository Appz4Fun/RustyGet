//! End-to-end Rust implementation of `PrePostProcessor` (`daemon/postprocess/PrePostProcessor.cpp`).
//!
//! Consolidates post-queue sanitisation, queue/file event routing, NZB completion & deletion
//! workflows, PAR/stream-repair coverage decisions, job scheduling strategies, stage
//! transitions, pause-state management, and direct-unpack triggers into a single module.

use crate::queuesubsys_host::NzbRef;
use crate::queuesubsys_host::FileRef;
use crate::status::{
    CleanupStatus, DeleteStatus, DirectRenameStatus, DirectUnpackStatus, DupeMode, MoveStatus,
    NzbParStatus, PostStage, UnpackStatus,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RsPostJobCandidate {
    /// Held at the post gate of its duplicate key (see [`dupe_post_gate`]).
    pub dupe_gate_held: u8,
    pub has_post_info: u8,
    pub working: u8,
    pub has_queue_script_job: u8,
    pub direct_unpack_running: u8,
    pub has_live_repair_thread: u8,
    pub priority: i32,
    pub force_priority: u8,
    pub need_par_check: u8,
    pub need_unpack: u8,
    pub in_active_jobs: u8,
    pub download_completed: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsPostStageDecision {
    pub unpack: i32,
    pub skip_unpack_due_to_par: i32,
    pub cleanup: i32,
    pub move_inter: i32,
    pub post_download_renaming: i32,
    pub post_unpack_renaming: i32,
}

/// `PrePostProcessor::SanitisePostQueue` stage & `RequestParCheck` transition.
pub fn sanitise_post_stage(
    stage: PostStage,
    dest_dir_exists: bool,
    unpack_tried: bool,
    par_status: NzbParStatus,
) -> (PostStage, bool) {
    if stage == PostStage::ExecutingScript || !dest_dir_exists {
        (PostStage::Finished, false)
    } else {
        let request_par = unpack_tried && par_status <= NzbParStatus::Skipped;
        (PostStage::Queued, request_par)
    }
}

/// `PrePostProcessor::DownloadQueueUpdate` `eaFileCompleted` / `eaFileDeleted` classifier.
/// Returns:
/// - `1`: collection completely downloaded (`qeNzbDownloaded` + `NzbDownloaded`)
/// - `2`: collection deleted from queue (`NzbDeleted`)
/// - `0`: no collection transition
#[allow(clippy::too_many_arguments)]
pub fn classify_file_event(
    is_file_completed: bool,
    is_file_deleted: bool,
    dupe_deleted: bool,
    delete_status: DeleteStatus,
    has_post_info: bool,
    download_completed_ignore_paused: bool,
    download_completed_all: bool,
) -> i32 {
    if (is_file_completed || dupe_deleted)
        && delete_status != DeleteStatus::Health
        && !has_post_info
        && download_completed_ignore_paused
    {
        1
    } else if (is_file_deleted || (is_file_completed && delete_status > DeleteStatus::None))
        && !has_post_info
        && download_completed_all
    {
        2
    } else {
        0
    }
}

/// `PrePostProcessor::CheckDownloadCompleted`: whether completion should trigger after `DirectRename`.
#[allow(clippy::too_many_arguments)]
pub fn should_complete_after_direct_rename(
    is_stopped: bool,
    direct_rename_status: DirectRenameStatus,
    has_post_info: bool,
    delete_status: DeleteStatus,
    completed_not_empty: bool,
    download_completed: bool,
    raw_article: bool,
    skip_write: bool,
    skip_disk_write: bool,
) -> bool {
    !is_stopped
        && direct_rename_status == DirectRenameStatus::Success
        && !has_post_info
        && delete_status == DeleteStatus::None
        && completed_not_empty
        && download_completed
        && !raw_article
        && !skip_write
        && !skip_disk_write
}

/// Classify entry routing in `PrePostProcessor::NzbDownloaded`:
/// Returns `(route, enqueue_deleted_script, set_par_skipped)` where `route`:
/// - `1`: skip-script + skip-write immediate cleanup and `NzbCompleted`
/// - `2`: queue for post-processing
/// - `3`: direct `NzbCompleted`
#[allow(clippy::too_many_arguments)]
pub fn classify_nzb_downloaded_entry(
    skip_script_processing: bool,
    skip_disk_write: bool,
    opt_skip_write: bool,
    delete_status: DeleteStatus,
    has_post_info: bool,
    opt_raw_article: bool,
    par_status: NzbParStatus,
    par_check_opt: i32,
    pc_always: i32,
    pc_force: i32,
) -> (i32, bool, bool) {
    if skip_script_processing && (skip_disk_write || opt_skip_write) {
        return (1, false, false);
    }
    let enqueue_deleted =
        delete_status == DeleteStatus::Health || delete_status == DeleteStatus::Bad;
    if !has_post_info && !opt_raw_article && !opt_skip_write && !skip_disk_write {
        let set_par_skipped = par_status == NzbParStatus::None
            && par_check_opt != pc_always
            && par_check_opt != pc_force;
        (2, enqueue_deleted, set_par_skipped)
    } else {
        (3, enqueue_deleted, false)
    }
}

/// Resolve delete status and route in `PrePostProcessor::NzbDeleted`:
/// Returns `(new_delete_status, call_nzb_downloaded)`.
pub fn resolve_nzb_deleted_route(delete_status: DeleteStatus) -> (DeleteStatus, bool) {
    let new_status = if delete_status == DeleteStatus::None {
        DeleteStatus::Manual
    } else {
        delete_status
    };
    let call_downloaded = new_status == DeleteStatus::Health || new_status == DeleteStatus::Bad;
    (new_status, call_downloaded)
}

/// `PrePostProcessor::NzbCompleted` action flags evaluator.
/// Returns `(add_to_history, call_dupe_completed, enqueue_deleted_script, need_save)`.
#[allow(clippy::too_many_arguments)]
pub fn eval_nzb_completed(
    dupe_hint: i32,
    dh_redownload_auto: i32,
    keep_history: i32,
    avoid_history: bool,
    dupe_check: bool,
    dupe_mode: DupeMode,
    fails_over: bool,
    delete_status: DeleteStatus,
) -> (bool, bool, bool, bool) {
    let download_dupe = dupe_hint == dh_redownload_auto;
    let add_to_history = keep_history > 0 && !avoid_history;
    let call_dupe_completed = dupe_check
        && (dupe_mode != DupeMode::Force || fails_over)
        && (delete_status == DeleteStatus::None
            || delete_status == DeleteStatus::Health
            || delete_status == DeleteStatus::Bad
            || delete_status == DeleteStatus::Scan
            || (delete_status == DeleteStatus::Copy && download_dupe));
    let enqueue_deleted_script = delete_status > DeleteStatus::None
        && delete_status != DeleteStatus::Health
        && delete_status != DeleteStatus::Bad;
    let need_save = true;
    (
        add_to_history,
        call_dupe_completed,
        enqueue_deleted_script,
        need_save,
    )
}

/// Classify disk cleanup mode in `PrePostProcessor::DeleteCleanup`:
/// - `0`: no cleanup
/// - `1`: skip-write mode (`DeleteDirectoryWithContent` on `destDir`)
/// - `2`: standard cleanup (delete completed files, `.out.tmp` files, and empty dirs)
pub fn classify_delete_cleanup_mode(
    cleanup_disk: bool,
    delete_status: DeleteStatus,
    parking: bool,
    skip_disk_write: bool,
    opt_skip_write: bool,
) -> i32 {
    if cleanup_disk || (delete_status == DeleteStatus::Dupe && !parking) {
        if skip_disk_write || opt_skip_write {
            1
        } else {
            2
        }
    } else {
        0
    }
}

/// Classify action for `PrePostProcessor::CheckRequestPar`:
/// Returns `(action, new_force_par_full)` where `action`:
/// - `0`: no action
/// - `1`: restart job (`psNone`, clear scripts, `ptQueued`, `working = false`)
/// - `2`: manual par-check with remaining files (`psManual`, `eaGroupResume`, `ptFinished`, `working = false`)
/// - `3`: manual par-check with no remaining files (`psManual`, `working = false`)
#[allow(clippy::too_many_arguments)]
pub fn classify_request_par_action(
    request_par_check: bool,
    par_status: NzbParStatus,
    force_repair: bool,
    par_full: bool,
    par_check_opt: i32,
    pc_manual: i32,
    cur_force_par_full: bool,
    file_list_empty: bool,
) -> (i32, bool) {
    if request_par_check
        && (par_status <= NzbParStatus::Skipped || (force_repair && !par_full))
        && par_check_opt != pc_manual
    {
        let new_force_par_full = cur_force_par_full || par_status > NzbParStatus::Skipped;
        (1, new_force_par_full)
    } else if request_par_check && par_status <= NzbParStatus::Skipped && par_check_opt == pc_manual
    {
        if !file_list_empty {
            (2, cur_force_par_full)
        } else {
            (3, cur_force_par_full)
        }
    } else {
        (0, cur_force_par_full)
    }
}

/// `PrePostProcessor::CleanupJobs`:
/// Evaluates whether an active post-job is ready for cleanup, whether its `PostTotalSec`
/// should be updated, whether `JobCompleted` should be invoked vs resetting stage to `ptQueued`,
/// and the updated `PostTotalSec`.
#[allow(clippy::too_many_arguments)]
pub fn eval_cleanup_job_state(
    working: bool,
    has_running_thread: bool,
    stage: PostStage,
    deleted: bool,
    start_time: i64,
    cur_time: i64,
    prev_post_total_sec: i32,
) -> (bool, bool, bool, i32) {
    if working || has_running_thread {
        return (false, false, false, prev_post_total_sec);
    }
    let update_total_sec = start_time > 0;
    let new_total_sec = if update_total_sec {
        let elapsed = cur_time.wrapping_sub(start_time) as i32;
        prev_post_total_sec.wrapping_add(elapsed)
    } else {
        prev_post_total_sec
    };
    let is_completed = stage == PostStage::Finished || deleted;
    (true, update_total_sec, is_completed, new_total_sec)
}

/// `PrePostProcessor::CanRunMoreJobs` scheduler evaluator.
/// `post_strategy`: `0` = `ppSequential`, `1` = `ppBalanced`, `2` = `ppAggressive`, `3` = `ppRocket`.
/// Returns `(can_run, allow_par)`.
pub fn can_run_more_jobs(
    job_stages: &[PostStage],
    post_strategy: i32,
) -> (bool, bool) {
    let total_jobs = job_stages.len() as i32;
    let mut par_jobs = 0;
    let mut other_jobs = 0;
    let mut repair_jobs = false;

    for &stage in job_stages {
        let par_job = stage >= PostStage::LoadingPars && stage <= PostStage::VerifyingRepaired;
        repair_jobs |= stage == PostStage::Repairing;
        if par_job {
            par_jobs += 1;
        } else {
            other_jobs += 1;
        }
    }

    match post_strategy {
        0 => (total_jobs == 0, true), // ppSequential
        1 => (
            other_jobs == 0 && (par_jobs == 0 || repair_jobs),
            par_jobs == 0,
        ), // ppBalanced
        2 => (total_jobs < 3, par_jobs < 1), // ppAggressive
        3 => (total_jobs < 6, par_jobs < 2), // ppRocket
        _ => (false, false),
    }
}

/// `PrePostProcessor::PickNextJob` candidate selector.
/// Returns the selected index or `-1`.
pub fn pick_next_post_job(
    candidates: &[RsPostJobCandidate],
    allow_par: bool,
    has_active_unpack: bool,
    pause_post_process: bool,
) -> i32 {
    let mut best_idx: i32 = -1;
    let mut best_priority: i32 = i32::MIN;

    for (idx, c) in candidates.iter().enumerate() {
        if c.has_post_info != 0
            && c.working == 0
            && c.has_queue_script_job == 0
            && c.direct_unpack_running == 0
            && c.has_live_repair_thread == 0
            && (best_idx < 0 || c.priority > best_priority)
            && (!pause_post_process || c.force_priority != 0)
            && (allow_par || c.need_par_check == 0)
            && (!has_active_unpack || c.need_unpack == 0)
            && c.in_active_jobs == 0
            && c.download_completed != 0
            && c.dupe_gate_held == 0
        {
            best_idx = idx as i32;
            best_priority = c.priority;
        }
    }

    best_idx
}

/// A copy of a release in the queue, as the post gate of its duplicate key sees it (see
/// [`dupe_post_gate`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateCopy {
    /// It went past the gate: it is unpacking, moving or finishing.
    pub passed: bool,
    /// Its direct unpack made progress (it extracted part of the release, or waits on
    /// volumes being repaired to carry on).
    pub extracting: bool,
}

/// What the post gate of a duplicate key tells a copy about to go past its repair stages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateVerdict {
    Pass,
    /// Another copy (its index in `others`) went past the gate first; whichever ends on disk
    /// first stops the other.
    WaitPassed(usize),
    /// Another copy's extraction is under way and this one's is not: it carries on, as
    /// starting over from a whole copy throws that extraction away.
    WaitExtracting(usize),
}

/// The post gate of a duplicate key (HealthCheck=dupe or park, where a copy may download while
/// another is stream-repairing): only one copy goes past its repair stages into unpack and
/// move, so two copies never both land files in the destination. The first copy to reach the
/// gate goes, unless another copy's extraction made progress and its own did not.
pub fn dupe_post_gate(own: GateCopy, others: &[GateCopy]) -> GateVerdict {
    if own.passed {
        return GateVerdict::Pass;
    }
    if let Some(i) = others.iter().position(|o| o.passed) {
        return GateVerdict::WaitPassed(i);
    }
    if !own.extracting {
        if let Some(i) = others.iter().position(|o| o.extracting) {
            return GateVerdict::WaitExtracting(i);
        }
    }
    GateVerdict::Pass
}

/// Downloads that went past the post gate of their duplicate key, and those held at it (told
/// once), by id (see [`dupe_post_gate`]).
static DUPE_GATE_PASSED: std::sync::Mutex<Option<std::collections::HashSet<i32>>> =
    std::sync::Mutex::new(None);
static DUPE_GATE_HELD: std::sync::Mutex<Option<std::collections::HashSet<i32>>> =
    std::sync::Mutex::new(None);

fn dupe_gate_set(
    set: &std::sync::Mutex<Option<std::collections::HashSet<i32>>>,
    id: i32,
    on: bool,
) -> bool {
    let mut g = set.lock().unwrap_or_else(|e| e.into_inner());
    let g = g.get_or_insert_with(Default::default);
    if on {
        g.insert(id)
    } else {
        g.remove(&id)
    }
}

fn dupe_gate_has(set: &std::sync::Mutex<Option<std::collections::HashSet<i32>>>, id: i32) -> bool {
    set.lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|g| g.contains(&id))
}

/// Downloads whose post job was stopped because another copy of the release is on disk.
static DUPE_STOPPED_IN_POST: std::sync::Mutex<Option<std::collections::HashSet<i32>>> =
    std::sync::Mutex::new(None);

/// Marks download `id`, in post-processing, as stopped because another copy of its release is
/// downloaded and on disk: when its job ends its files are removed (unless HealthCheck=park
/// parks it), as the delete route removes a copy stopped in the queue (see
/// [`classify_delete_cleanup_mode`]: DeleteStatus DUPE, not parking).
pub(crate) fn note_stopped_for_on_disk(id: i32) {
    dupe_gate_set(&DUPE_STOPPED_IN_POST, id, true);
}

/// Forgets the post gate's record of download `id`: its job ended or it left the queue.
fn dupe_gate_forget(id: i32) {
    dupe_gate_set(&DUPE_GATE_PASSED, id, false);
    dupe_gate_set(&DUPE_GATE_HELD, id, false);
    dupe_gate_set(&DUPE_STOPPED_IN_POST, id, false);
}

/// The post gate's verdict for queued download `nzb`, with the name of the copy it waits for.
#[cfg(unix)]
fn dupe_post_gate_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb: &crate::queuesubsys_host::NzbSnap,
) -> (GateVerdict, Vec<u8>) {
    use crate::queuesubsys_host::{HC_DUPE, HC_PARK, NK_NZB};
    let health_check = host.opts().health_check;
    if nzb.dupe_key.is_empty()
        || nzb.dupe_mode == DupeMode::Force
        || nzb.delete_status != DeleteStatus::None
        || (health_check != HC_DUPE && health_check != HC_PARK)
    {
        return (GateVerdict::Pass, Vec::new());
    }
    let extracting = |n: &crate::queuesubsys_host::NzbSnap| {
        n.direct_unpack_status == DirectUnpackStatus::Running
            || n.direct_unpack_status == DirectUnpackStatus::Success
            || crate::postproc::direct_unpack_has_holds(n.id)
            || crate::postproc::direct_unpack_awaiting_repair(n.id)
    };
    // Past the gate is also what a copy's state on disk says (its unpack or move already
    // started), as the gate's record does not survive a restart.
    let passed = |n: &crate::queuesubsys_host::NzbSnap| {
        dupe_gate_has(&DUPE_GATE_PASSED, n.id)
            || n.unpack_status != UnpackStatus::None
            || n.move_status != MoveStatus::None
    };
    let own = GateCopy { passed: passed(nzb), extracting: extracting(nzb) };
    let mut others = Vec::new();
    let mut names = Vec::new();
    for i in 0..host.queue_len(dq_ptr) {
        let qp = host.queue_at(dq_ptr, i);
        if qp.is_none() || qp == nzb.ptr {
            continue;
        }
        let q = host.nzb_snap(qp, false);
        if q.kind == NK_NZB
            && !q.deleting
            && q.delete_status == DeleteStatus::None
            && q.dupe_mode != DupeMode::Force
            && q.dupe_key.eq_ignore_ascii_case(&nzb.dupe_key)
        {
            others.push(GateCopy {
                passed: passed(&q),
                extracting: extracting(&q),
            });
            names.push(q.name);
        }
    }
    let verdict = dupe_post_gate(own, &others);
    let name = match verdict {
        GateVerdict::WaitPassed(i) | GateVerdict::WaitExtracting(i) => names.swap_remove(i),
        GateVerdict::Pass => Vec::new(),
    };
    (verdict, name)
}

/// True when a backup of kind `backup_kind` can lend a download of kind `own_kind`, whose
/// direct unpack holds volumes, its lost volumes (see `queuecoord::alt_covers_lost_articles`:
/// with an extraction running, a backup of unknown kind lends too when it was labelled the
/// same video in another container, `labelled_alt`, such as a nested alt).
fn backup_can_lend(
    own_kind: Option<&[u8]>,
    backup_kind: Option<&[u8]>,
    labelled_alt: bool,
) -> bool {
    use crate::donorkind::DonorKind;
    DonorKind::from_param(own_kind) != DonorKind::Unborrowable
        && match DonorKind::from_param(backup_kind) {
            DonorKind::Twin | DonorKind::Borrowable | DonorKind::EncryptedBorrowable => true,
            DonorKind::Unknown => labelled_alt,
            DonorKind::Unborrowable => false,
        }
}

/// True when a download that direct unpack extracted and verified has no damage outside its
/// archive: every damaged or missing file is a par2 file, a RAR volume (its content was checked
/// by the extraction), or one of at most `borrowed` files that never arrived under a name telling
/// nothing (obfuscated dead volumes, each rebuilt from a duplicate). Only when something was
/// borrowed: `borrowed` dead volumes, or bytes put back otherwise (`recovered`: lent by a
/// duplicate, or a volume end rebuilt from a sibling volume) that par2 cannot tell from damage
/// or could not check (its own par2 files damaged).
fn damage_is_borrowed_volumes(
    files: &[crate::queuesubsys_host::CompletedFileSnap],
    extracted: &[Vec<u8>],
    dest_dir: &[u8],
    borrowed: usize,
    recovered: bool,
) -> bool {
    const CF_SUCCESS: i32 = 1;
    if borrowed == 0 && !recovered {
        return false;
    }
    let mut absent_unnamed = 0usize;
    // A lost `.nfo` or `.sfv` holds no payload: it never decides a verified extraction.
    for f in files
        .iter()
        .filter(|f| f.status != CF_SUCCESS && !f.par_file && !is_informational_file(&f.filename))
    {
        if crate::filetypes::is_rar_file(&f.filename) {
            // Only a volume direct unpack read was checked by the extraction.
            if extracted.iter().any(|e| e.eq_ignore_ascii_case(&f.filename)) {
                continue;
            }
            return false;
        }
        let ext = crate::filetypes::file_extension(&f.filename);
        let mut path = dest_dir.to_vec();
        path.push(crate::paths::PATH_SEPARATOR);
        path.extend_from_slice(&f.filename);
        if crate::filetypes::is_known_collection_ext(ext) || crate::fsops::path(&path).exists() {
            return false;
        }
        absent_unnamed += 1;
    }
    absent_unnamed <= borrowed
}

/// True when par-check would only rebuild volumes that direct unpack already read borrowed,
/// or only flag bytes put back into them: every data file that never reached the disk is one
/// of the `borrowed` dead volumes, every damaged file on disk is an archive volume direct
/// unpack extracted from (its outputs passed the archive's own checksums, with any hole
/// patched from a repair), and something is missing or was put back. A damaged file direct
/// unpack never read still needs par-check.
fn direct_unpack_read_all_damage(
    files: &[crate::queuesubsys_host::CompletedFileSnap],
    extracted: &[Vec<u8>],
    dest_dir: &[u8],
    borrowed: usize,
    put_back: bool,
) -> bool {
    const CF_SUCCESS: i32 = 1;
    // Files that never reached the disk: each one must be a dead volume read borrowed.
    let mut absent = 0usize;
    // A lost `.nfo` or `.sfv` holds no payload: it never decides a verified extraction.
    for f in files
        .iter()
        .filter(|f| f.status != CF_SUCCESS && !f.par_file && !is_informational_file(&f.filename))
    {
        let read = extracted.iter().any(|e| e.eq_ignore_ascii_case(&f.filename));
        let mut path = dest_dir.to_vec();
        path.push(crate::paths::PATH_SEPARATOR);
        path.extend_from_slice(&f.filename);
        if crate::fsops::path(&path).exists() {
            if read {
                continue;
            }
            return false;
        }
        // A borrowed volume is read under its own name, so it is in `extracted` too: it still
        // takes its place in `borrowed`. One under an obfuscated name has no known extension;
        // a known file that never arrived (a sample, subtitles) was never borrowed.
        if !read
            && !crate::filetypes::is_rar_file(&f.filename)
            && crate::filetypes::is_known_collection_ext(crate::filetypes::file_extension(
                &f.filename,
            ))
        {
            return false;
        }
        absent += 1;
    }
    // Nothing else checks a file direct unpack did not extract: bytes put back into it (a
    // split file, a sample) are verified by par-check alone, so it is kept for such a job
    // unless every file it did not read is a small informational one.
    if put_back && absent == 0 {
        let unread = files.iter().filter(|f| !f.par_file).any(|f| {
            !extracted.iter().any(|e| e.eq_ignore_ascii_case(&f.filename))
                && !is_informational_file(&f.filename)
        });
        if unread {
            return false;
        }
    }
    // With every file on disk par-check only verifies (quickly, from the article checksums
    // where it can): it is kept, unless bytes were put back into the volumes that par2 can
    // count as damage although the extraction read them fine (`put_back`: lent by a duplicate,
    // or a volume end rebuilt from a sibling, whose quick-open record par2 sees as a bad
    // block): its failure would then fail a download direct unpack verified. It is skipped
    // when it would rebuild borrowed volumes.
    absent <= borrowed && (absent > 0 || put_back)
}

/// Release side files that hold no payload (`.nfo`, `.sfv`, ...): par-check is not kept for them.
fn is_informational_file(name: &[u8]) -> bool {
    const EXTS: [&[u8]; 12] = [
        b".nfo", b".sfv", b".srr", b".srs", b".txt", b".jpg", b".jpeg", b".png", b".url",
        b".nzb", b".md5", b".sha1",
    ];
    let ext = crate::filetypes::file_extension(name);
    EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext))
}

/// Classify skipped par-check health action in `PrePostProcessor::StartJob`:
/// - `0`: no action
/// - `1`: skip par-check due to health 0%
/// - `2`: skip par-check due to health below critical
/// - `3`: request par-check due to failed non-par bytes
#[allow(clippy::too_many_arguments)]
pub fn classify_skipped_par_health(
    par_status: NzbParStatus,
    par_scan_opt: i32,
    ps_dupe: i32,
    health: i32,
    critical_health: i32,
    failed_size: i64,
    par_failed_size: i64,
) -> i32 {
    if par_status != NzbParStatus::Skipped {
        return 0;
    }
    if (par_scan_opt != ps_dupe && health < critical_health && critical_health < 1000)
        || health == 0
    {
        if health == 0 {
            1
        } else {
            2
        }
    } else if failed_size - par_failed_size > 0 {
        3
    } else {
        0
    }
}

/// Evaluate `wantUnpack`, `unpack`, and whether to start `RenameController::jkRar` in `PrePostProcessor::StartJob`.
#[allow(clippy::too_many_arguments)]
pub fn eval_want_unpack_and_rar_rename(
    has_unpack_param: bool,
    unpack_param_val: &[u8],
    unpack_status: UnpackStatus,
    delete_status: DeleteStatus,
    rar_rename_status: i32,
    rs_none: i32,
    opt_rar_rename: bool,
) -> (bool, bool, bool) {
    let want_unpack = !(has_unpack_param && unpack_param_val.eq_ignore_ascii_case(b"no"));
    let unpack =
        want_unpack && unpack_status == UnpackStatus::None && delete_status == DeleteStatus::None;
    let start_rar_rename = rar_rename_status == rs_none && unpack && opt_rar_rename;
    (want_unpack, unpack, start_rar_rename)
}

/// `PrePostProcessor::StartJob` stage eligibility calculator.
#[allow(clippy::too_many_arguments)]
pub fn eval_post_stages(
    want_unpack: bool,
    unpack_status: UnpackStatus,
    delete_status: DeleteStatus,
    par_status: NzbParStatus,
    cleanup_status: CleanupStatus,
    has_ext_cleanup_disk: bool,
    health: i32,
    move_status: MoveStatus,
    dest_dir: &[u8],
    inter_dir: &[u8],
    sep: u8,
    nzb_id: i32,
    direct_unpack_status: DirectUnpackStatus,
    opt_post_download_rename: bool,
    post_download_renaming_status: i32,
    has_dest_dir: bool,
    has_name: bool,
    opt_rename_after_unpack: bool,
    post_unpack_renaming_status: i32,
) -> RsPostStageDecision {
    // also a folder an earlier InterDir gave it, which then moves like any other
    let in_inter_dir = crate::unpack::job_dir_is_intermediate(dest_dir, inter_dir, sep, Some(nzb_id));
    eval_post_stages_in_inter(
        want_unpack,
        unpack_status,
        delete_status,
        par_status,
        cleanup_status,
        has_ext_cleanup_disk,
        health,
        move_status,
        in_inter_dir,
        direct_unpack_status,
        opt_post_download_rename,
        post_download_renaming_status,
        has_dest_dir,
        has_name,
        opt_rename_after_unpack,
        post_unpack_renaming_status,
    )
}

/// `PrePostProcessor::StartJob` stage eligibility calculator with precomputed `in_inter_dir`.
#[allow(clippy::too_many_arguments)]
pub fn eval_post_stages_in_inter(
    want_unpack: bool,
    unpack_status: UnpackStatus,
    delete_status: DeleteStatus,
    par_status: NzbParStatus,
    cleanup_status: CleanupStatus,
    has_ext_cleanup_disk: bool,
    health: i32,
    move_status: MoveStatus,
    in_inter_dir: bool,
    direct_unpack_status: DirectUnpackStatus,
    opt_post_download_rename: bool,
    post_download_renaming_status: i32,
    has_dest_dir: bool,
    has_name: bool,
    opt_rename_after_unpack: bool,
    post_unpack_renaming_status: i32,
) -> RsPostStageDecision {
    let mut unpack =
        want_unpack && unpack_status == UnpackStatus::None && delete_status == DeleteStatus::None;
    let par_failed = par_status == NzbParStatus::Failure
        || par_status == NzbParStatus::RepairPossible
        || par_status == NzbParStatus::Manual;

    let cleanup = !unpack
        && want_unpack
        && cleanup_status == CleanupStatus::None
        && has_ext_cleanup_disk
        && ((par_status == NzbParStatus::Success
            && unpack_status != UnpackStatus::Failure
            && unpack_status != UnpackStatus::Space
            && unpack_status != UnpackStatus::Password)
            || (unpack_status == UnpackStatus::Success && par_status != NzbParStatus::Failure)
            || ((unpack_status == UnpackStatus::None || unpack_status == UnpackStatus::Skipped)
                && (par_status == NzbParStatus::None || par_status == NzbParStatus::Skipped)
                && health == 1000));

    let move_inter = !unpack
        && move_status == MoveStatus::None
        && unpack_status != UnpackStatus::Failure
        && unpack_status != UnpackStatus::Space
        && unpack_status != UnpackStatus::Password
        && par_status != NzbParStatus::Failure
        && par_status != NzbParStatus::Manual
        && delete_status == DeleteStatus::None
        && !((unpack_status == UnpackStatus::None || unpack_status == UnpackStatus::Skipped)
            && (par_status == NzbParStatus::None || par_status == NzbParStatus::Skipped)
            && health < 1000)
        && in_inter_dir;

    let mut skip_unpack_due_to_par = false;
    let mut effective_unpack_status = unpack_status;
    if unpack && par_failed && direct_unpack_status != DirectUnpackStatus::Success {
        skip_unpack_due_to_par = true;
        effective_unpack_status = UnpackStatus::Skipped;
        unpack = false;
    }

    let post_download_renaming = opt_post_download_rename
        && post_download_renaming_status == 0
        && has_dest_dir
        && has_name
        && (effective_unpack_status == UnpackStatus::None
            || effective_unpack_status == UnpackStatus::Skipped)
        && effective_unpack_status != UnpackStatus::Failure
        && effective_unpack_status != UnpackStatus::Space
        && effective_unpack_status != UnpackStatus::Password
        && par_status != NzbParStatus::Failure
        && par_status != NzbParStatus::Manual
        && !move_inter
        && (move_status == MoveStatus::Success || move_status == MoveStatus::None);

    let post_unpack_renaming = opt_rename_after_unpack
        && post_unpack_renaming_status == 0
        && has_dest_dir
        && has_name
        && effective_unpack_status == UnpackStatus::Success
        && par_status != NzbParStatus::Failure
        && par_status != NzbParStatus::Manual
        && !move_inter
        && (move_status == MoveStatus::Success || move_status == MoveStatus::None);

    RsPostStageDecision {
        unpack: if unpack { 1 } else { 0 },
        skip_unpack_due_to_par: if skip_unpack_due_to_par { 1 } else { 0 },
        cleanup: if cleanup { 1 } else { 0 },
        move_inter: if move_inter { 1 } else { 0 },
        post_download_renaming: if post_download_renaming { 1 } else { 0 },
        post_unpack_renaming: if post_unpack_renaming { 1 } else { 0 },
    }
}

/// `PrePostProcessor::UpdatePauseState` pause-download evaluator.
pub fn calc_post_pause_state(
    job_stages: &[PostStage],
    par_pause_queue: bool,
    unpack_pause_queue: bool,
    script_pause_queue: bool,
) -> bool {
    let mut need_pause = false;
    for &stage in job_stages {
        match stage {
            PostStage::LoadingPars
            | PostStage::VerifyingSources
            | PostStage::Repairing
            | PostStage::VerifyingRepaired
            | PostStage::ParRenaming
            | PostStage::StreamRepairing => need_pause |= par_pause_queue,
            PostStage::RarRenaming
            | PostStage::Unpacking
            | PostStage::CleaningUp
            | PostStage::Moving
            | PostStage::PostUnpackRenaming
            | PostStage::PostDownloadRenaming => need_pause |= unpack_pause_queue,
            PostStage::ExecutingScript => need_pause |= script_pause_queue,
            PostStage::Queued | PostStage::Finished | PostStage::Other(_) => {}
        }
    }
    need_pause
}

/// Classify direct-unpack action in `PrePostProcessor::FileDownloaded`:
/// - `0`: do nothing
/// - `1`: candidate for starting `DirectUnpack::StartJob`
/// - `2`: notify running `DirectUnpack::FileDownloaded`
#[allow(clippy::too_many_arguments)]
pub fn classify_file_downloaded_direct_unpack(
    opt_direct_unpack: bool,
    opt_raw_article: bool,
    opt_skip_write: bool,
    skip_disk_write: bool,
    direct_unpack_status: DirectUnpackStatus,
    direct_rename_status: DirectRenameStatus,
    has_unpack_thread: bool,
) -> i32 {
    if !opt_direct_unpack || opt_raw_article || opt_skip_write || skip_disk_write {
        return 0;
    }
    if direct_unpack_status == DirectUnpackStatus::None
        && direct_rename_status != DirectRenameStatus::Running
    {
        1
    } else if direct_unpack_status == DirectUnpackStatus::Running && has_unpack_thread {
        2
    } else {
        0
    }
}

#[cfg(unix)]
const MAX_INDEX_PAR_SIZE: i64 = 16 * 1024 * 1024;

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> &std::path::Path {
    use std::os::unix::ffi::OsStrExt;
    std::path::Path::new(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(unix)]
fn has_main_pars(dest_dir: &[u8]) -> bool {
    crate::parparser::find_main_pars(bytes_to_path(dest_dir), false, |_| {})
}

/// Live PAR's verified cover of the download, when post-processing may take it instead of a
/// par-check (`parchecker::streaming_session_verified_cover`: every slice of every file
/// verified or rebuilt and verified, every file under its par2 name at its par2 length and
/// unchanged since). Only with ParCheck=auto (always and force ask for a par-check of every
/// download), not for a post-process-again, not for a par-check asked for after an unpack
/// (it failed: the files on disk may not be the bytes live PAR verified) or a forced full
/// check or repair, not while live PAR still works on the download, and only when the folder
/// holds that one par2 set: a second set was never verified live. `Err` says why not.
#[cfg(unix)]
fn live_par_cover(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
    ps: &crate::queuesubsys_host::PostSnap,
) -> Result<crate::parchecker::LiveCover, String> {
    if host.opts().par_check != crate::queuesubsys_host::PC_AUTO {
        return Err("ParCheck asks for a par-check of every download".into());
    }
    if nzb.reprocess {
        return Err("post-processing again".into());
    }
    if nzb.unpack_tried || ps.force_par_full || ps.force_repair {
        return Err("a full par-check was asked for".into());
    }
    if !crate::postproc::live_par_work_idle(nzb.id) {
        return Err("live PAR is still working".into());
    }
    if crate::postproc::direct_unpack_declared_damage(nzb.id) {
        // The extraction declared damage live PAR has not verified away: only a repair
        // (or the release of that hold) may settle it, never a cover taken as it stands.
        return Err("direct unpack holds declared damage".into());
    }
    let par_name = crate::parchecker::streaming_session_par_filename(nzb.id)
        .ok_or("no live PAR session")?;
    let dir = bytes_to_path(&nzb.dest_dir);
    let mut mains: Vec<Vec<u8>> = Vec::new();
    crate::parparser::find_main_pars(dir, true, |name| mains.push(name.to_vec()));
    match mains.as_slice() {
        [main] if crate::parparser::same_par_collection(main, &par_name, true) => {}
        [_] => return Err("the folder's par2 set is not the one live PAR verified".into()),
        _ => return Err(format!("{} par2 sets in the folder", mains.len())),
    }
    let count = host.nzb_completed_files_len(nzb.ptr);
    if count > 4096 {
        return Err("too many files".into());
    }
    let files: Vec<(i32, Vec<u8>)> = (0..count)
        .map(|index| host.nzb_completed_file_at(nzb.ptr, index))
        .filter(|file| !file.par_file)
        .map(|file| (file.id, file.filename))
        .collect();
    crate::parchecker::streaming_session_verified_cover(nzb.id, dir, &files)
}

/// Takes live PAR's verified cover instead of the par-check post-processing was about to run
/// for `nzb` (see [`live_par_cover`]): the job gets what nzbget stores after a quick
/// par-check that found nothing to repair (ParStatus SUCCESS, not a full check), the
/// stream-repair jobs of the verified files go as a verified file's do, and no byte of the
/// set is read again. False, with the reason in the detail log, when the cover is not
/// complete: the normal par-check runs.
#[cfg(unix)]
fn trust_live_par(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
    ps: &crate::queuesubsys_host::PostSnap,
) -> bool {
    use crate::queuesubsys_host::{MK_DETAIL, MK_INFO, NZB_INT_CHANGED, NZB_INT_PAR_FULL};
    let cover = match live_par_cover(host, nzb, ps) {
        Ok(cover) => cover,
        Err(why) => {
            if crate::parchecker::streaming_session_has_session(nzb.id) {
                let mut msg = b"Live PAR does not cover ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(format!(" ({why}): par-check runs").as_bytes());
                host.nzb_print_message(nzb.ptr, MK_DETAIL, &msg);
            }
            return false;
        }
    };
    let mut msg = b"Skipping par-check for ".to_vec();
    msg.extend_from_slice(&nzb.name);
    msg.extend_from_slice(b": live PAR verified every file of ");
    msg.extend_from_slice(&cover.par_name);
    msg.extend_from_slice(
        format!(" ({} file(s), every slice checked or repaired as it downloaded)", cover.files)
            .as_bytes(),
    );
    host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
    host.nzb_set_par_status(nzb.ptr, NzbParStatus::Success);
    // A par-check that quick-verified any file stores par_full 0 (`ParChecker`); a failed
    // unpack then still asks for a full par-check.
    host.nzb_set_int(nzb.ptr, NZB_INT_PAR_FULL, 0);
    // As `RepairController::register_verified_file` does for each verified file: nothing
    // is left for stream repair to fetch for them.
    for index in (0..host.nzb_stream_repair_jobs_len(nzb.ptr)).rev() {
        let job = host.nzb_stream_repair_job_detail(nzb.ptr, index);
        if cover.queue_ids.contains(&job.file_id) {
            host.nzb_stream_repair_erase_job(nzb.ptr, index, true);
        }
    }
    host.nzb_set_int(nzb.ptr, NZB_INT_CHANGED, 1);
    true
}

#[cfg(unix)]
fn has_completed_archive_files(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: NzbRef,
) -> bool {
    host.nzb_completed_files(nzb_ptr)
        .iter()
        .any(|cf| crate::unpack::has_completed_archive_ext(&cf.filename))
}

#[cfg(unix)]
pub(crate) fn par_cannot_cover_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
) -> bool {
    use std::os::unix::ffi::OsStrExt;

    let health = nzb.health;
    let critical_health = nzb.critical_health_exact;
    if health == 0 || health >= critical_health {
        return false;
    }

    let dest_path = bytes_to_path(&nzb.dest_dir);
    let mut main_pars: Vec<Vec<u8>> = Vec::new();
    let found_main = crate::parparser::find_main_pars(dest_path, true, |name| {
        main_pars.push(name.to_vec());
    });
    if !found_main || main_pars.len() != 1 {
        return false;
    }

    let mut smallest_path: Option<std::path::PathBuf> = None;
    let mut smallest_size: i64 = -1;
    if let Ok(entries) = std::fs::read_dir(dest_path) {
        for entry in entries.flatten() {
            let fname_os = entry.file_name();
            let fname_bytes = fname_os.as_os_str().as_bytes();
            if fname_bytes.contains(&0) {
                continue;
            }
            if crate::parparser::same_par_collection(fname_bytes, &main_pars[0], true) {
                let candidate_path = entry.path();
                if let Ok(meta) = std::fs::metadata(&candidate_path) {
                    if meta.is_file() {
                        let sz = meta.len() as i64;
                        if sz >= 0 && (smallest_size < 0 || sz < smallest_size) {
                            smallest_size = sz;
                            smallest_path = Some(candidate_path);
                        }
                    }
                }
            }
        }
    }

    let Some(par_path) = smallest_path else {
        return false;
    };
    if smallest_size < 0 {
        return false;
    }
    let Some(parred_files) = crate::parrenamer::list_parred_files(&par_path, MAX_INDEX_PAR_SIZE)
    else {
        return false;
    };

    let completed = host.nzb_completed_files(nzb.ptr);
    let jobs = host.nzb_stream_repair_jobs(nzb.ptr);
    let mut protected_damage: i64 = 0;
    for job in &jobs {
        let mut filename: &[u8] = &job.filename;
        for cf in &completed {
            if cf.id == job.file_id {
                filename = &cf.filename;
                break;
            }
        }
        let base_name = crate::queuecoord::base_file_name(filename);
        if parred_files
            .iter()
            .any(|pf| pf.eq_ignore_ascii_case(base_name))
        {
            if job.decoded_file_size > 0 {
                protected_damage += job.holes_total_size;
            } else {
                protected_damage += job.failed_plus_missed_size;
            }
        }
    }

    let intact_par = (nzb.par_size - nzb.par_current_failed_size).max(0);
    protected_damage > intact_par
}

#[cfg(unix)]
pub(crate) fn has_dupe_scan_sources_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    dq_ptr: usize,
    nzb_ptr: NzbRef,
) -> bool {
    if opts.par_scan != crate::queuesubsys_host::PS_DUPE {
        return false;
    }
    for dupe_ptr in crate::dupecoord::list_history_dupes_host(host, dq_ptr, nzb_ptr) {
        let dupe = host.nzb_snap(dupe_ptr, false);
        if !dupe.dest_dir.is_empty()
            && crate::fsops::directory_exists(&dupe.dest_dir, &crate::fsops::posix_stat)
        {
            return true;
        }
    }
    false
}

#[cfg(unix)]
pub(crate) fn delete_cleanup_host(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    opts: &crate::queuesubsys_host::GlobalOptsSnap,
    nzb: &crate::queuesubsys_host::NzbSnap,
) {
    use crate::queuesubsys_host::{c_bytes, MK_DETAIL, MK_ERROR};
    use std::ffi::{CStr, CString};

    // F2c: a fleet primary deleted (by hand, HealthCheck=Delete) leaves with
    // its own priority, not its fleet's raise
    crate::fleethold::unboost_host(host, nzb.ptr);

    // a fleet copy that shares the primary's folder, or is kept as a donor or
    // a dead copy with its files: never cleaned up (fleethold::KEEP_FILES_PARAM)
    if host
        .nzb_get_param(nzb.ptr, crate::fleethold::KEEP_FILES_PARAM)
        .is_some_and(|v| v.trim_ascii().eq_ignore_ascii_case(b"yes"))
    {
        return;
    }
    let mode = classify_delete_cleanup_mode(
        nzb.cleanup_disk,
        nzb.delete_status,
        nzb.parking,
        nzb.skip_disk_write,
        opts.skip_write,
    );
    if mode == 0 || nzb.dest_dir.is_empty() {
        return;
    }

    let dest_bytes = nzb.dest_dir.as_slice();
    let sep = crate::paths::PATH_SEPARATOR;

    if mode == 1 {
        if crate::fsops::directory_exists(dest_bytes, &crate::fsops::posix_stat) {
            let mut detail_msg = b"Deleting dir ".to_vec();
            detail_msg.extend_from_slice(dest_bytes);
            if detail_msg.len() > 1023 {
                detail_msg.truncate(1023);
            }
            host.log(MK_DETAIL, &detail_msg);

            let mut errmsg = Vec::new();
            if !crate::fsdelete::delete_directory_with_content(
                &mut crate::fsops::PosixFsOps,
                dest_bytes,
                sep,
                &mut errmsg,
            )
            .unwrap_or(false)
            {
                let err_end = errmsg.iter().position(|&b| b == 0).unwrap_or(errmsg.len());
                let mut err = b"Could not delete directory ".to_vec();
                err.extend_from_slice(dest_bytes);
                err.extend_from_slice(b": ");
                err.extend_from_slice(&errmsg[..err_end]);
                host.log(MK_ERROR, &err);
            }
        }
        return;
    }

    for cf in host.nzb_completed_files(nzb.ptr) {
        if cf.filename.is_empty() {
            continue;
        }
        let mut full = Vec::with_capacity(dest_bytes.len() + 1 + cf.filename.len());
        full.extend_from_slice(dest_bytes);
        full.push(sep);
        full.extend_from_slice(&cf.filename);
        if crate::fsops::file_exists(&full, &crate::fsops::posix_stat) {
            let mut detail_msg = b"Deleting file ".to_vec();
            detail_msg.extend_from_slice(&cf.filename);
            if detail_msg.len() > 1023 {
                detail_msg.truncate(1023);
            }
            host.log(MK_DETAIL, &detail_msg);
            if let Ok(full_c) = CString::new(full) {
                let _ = crate::fsops::delete_file(&full_c);
            }
        }
    }

    let dest_c_vec = c_bytes(dest_bytes);
    let dest_cstr = unsafe { CStr::from_bytes_with_nul_unchecked(&dest_c_vec) };
    let mut dir = crate::fsops::PosixDirBrowser::new(Some(dest_cstr), false);
    loop {
        let fn_ptr = dir.next_ptr();
        if fn_ptr.is_null() {
            break;
        }
        let fn_bytes = unsafe { CStr::from_ptr(fn_ptr).to_bytes() };
        if fn_bytes.ends_with(b".out.tmp") {
            let mut full = Vec::with_capacity(dest_bytes.len() + 1 + fn_bytes.len());
            full.extend_from_slice(dest_bytes);
            full.push(sep);
            full.extend_from_slice(fn_bytes);
            let mut detail_msg = b"Deleting file ".to_vec();
            detail_msg.extend_from_slice(fn_bytes);
            if detail_msg.len() > 1023 {
                detail_msg.truncate(1023);
            }
            host.log(MK_DETAIL, &detail_msg);
            if let Ok(full_c) = CString::new(full) {
                let _ = crate::fsops::delete_file(&full_c);
            }
        }
    }

    let _ = crate::fsdelete::delete_directory(&mut crate::fsops::PosixFsOps, dest_bytes, sep);
    let final_dir = host.nzb_build_final_dir_name(nzb.ptr);
    if !final_dir.is_empty()
        && final_dir.as_slice() != dest_bytes
        && crate::fsops::directory_exists(&final_dir, &crate::fsops::posix_stat)
    {
        let _ = crate::fsdelete::delete_directory(&mut crate::fsops::PosixFsOps, &final_dir, sep);
    }
}

#[cfg(unix)]
fn fmt_tenths(val: i32) -> String {
    format!("{:.1}", f64::from(val) / 10.0)
}

#[cfg(unix)]
pub struct NzbgetRsPrePostProcessor {
    owner: usize,
    queued_jobs: std::sync::atomic::AtomicI32,
    active_jobs: std::sync::Mutex<Vec<NzbRef>>,
    wait_mutex: std::sync::Mutex<()>,
    wait_cond: std::sync::Condvar,
}

#[cfg(unix)]
impl NzbgetRsPrePostProcessor {
    pub fn new(owner: usize) -> Self {
        Self {
            owner,
            queued_jobs: std::sync::atomic::AtomicI32::new(0),
            active_jobs: std::sync::Mutex::new(Vec::new()),
            wait_mutex: std::sync::Mutex::new(()),
            wait_cond: std::sync::Condvar::new(),
        }
    }

    /// Whether jobs are queued for post-processing.
    pub(crate) fn has_queued_jobs(&self) -> bool {
        self.queued_jobs.load(std::sync::atomic::Ordering::Acquire) > 0
    }

    /// Whether a job is unpacking (or, with `include_waiting_volume`, waiting
    /// for a volume to unpack), checked under the queue lock.
    pub(crate) fn has_active_unpack_locked(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        include_waiting_volume: bool,
    ) -> bool {
        let mut res = false;
        host.with_locked_queue(|dq_ptr| {
            res = self.has_active_unpack(host, dq_ptr, include_waiting_volume);
        });
        res
    }

    pub(crate) fn owner_ptr(&self) -> usize {
        self.owner
    }

    pub(crate) fn run(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        use crate::queuesubsys_host::MK_DEBUG;
        use std::sync::atomic::Ordering;

        host.log(MK_DEBUG, b"Entering PrePostProcessor-loop");

        while !host.dq_is_loaded() {
            host.sleep_ms(20);
        }

        if host.opts().server_mode {
            self.sanitise_post_queue(host);
        }

        while !host.prepost_is_stopped(self.owner) {
            if host.opts().temp_pause_postprocess {
                host.sleep_ms(200);
                continue;
            }

            if self.queued_jobs.load(Ordering::Acquire) != 0 {
                self.check_post_queue(host);
                host.sleep_ms(200);
            } else {
                let mut lk = self.wait_mutex.lock().unwrap();
                while self.queued_jobs.load(Ordering::Acquire) == 0
                    && !host.prepost_is_stopped(self.owner)
                {
                    lk = self.wait_cond.wait(lk).unwrap();
                }
            }
        }

        self.wait_jobs(host);

        host.log(MK_DEBUG, b"Exiting PrePostProcessor-loop");
    }

    pub(crate) fn stop(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        host.with_locked_queue(|dq_ptr| {
            let jobs = self.active_jobs.lock().unwrap().clone();
            for job_ptr in jobs {
                let nzb = host.nzb_snap(job_ptr, false);
                if nzb.post_info_ptr.is_some() {
                    host.post_stop_thread(nzb.post_info_ptr);
                }
            }

            let qlen = host.queue_len(dq_ptr);
            for qi in 0..qlen {
                let nzb_ptr = host.queue_at(dq_ptr, qi);
                let nzb = host.nzb_snap(nzb_ptr, false);
                if nzb.has_unpack_thread {
                    host.direct_unpack_stop(dq_ptr, nzb_ptr);
                }
                if nzb.has_live_repair_thread {
                    host.live_repair_stop(nzb_ptr);
                }
                crate::postproc::direct_unpack_join_live_par_repair(nzb.id);
            }
        });

        let _wg = self.wait_mutex.lock().unwrap();
        self.wait_cond.notify_all();
    }

    fn wait_jobs(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        use crate::queuesubsys_host::{MK_DEBUG, MK_WARNING, NZB_ACT_DISCONNECT_THREADS};

        host.log(MK_DEBUG, b"PrePostProcessor: waiting for jobs to complete");

        let mut wait_start = host.opts().cur_time;
        while host.opts().cur_time - wait_start < 5 {
            let mut empty = false;
            host.with_locked_queue(|_| {
                empty = self.active_jobs.lock().unwrap().is_empty();
            });
            if empty {
                break;
            }
            self.check_post_queue(host);
            host.sleep_ms(200);
        }

        host.with_locked_queue(|_| {
            let jobs = self.active_jobs.lock().unwrap().clone();
            for job_ptr in jobs {
                let nzb = host.nzb_snap(job_ptr, false);
                if nzb.post_info_ptr.is_some() {
                    let ps = host.post_snap(nzb.post_info_ptr);
                    if ps.has_post_thread {
                        let mut msg = b"Terminating active post-process job for ".to_vec();
                        msg.extend_from_slice(&nzb.name);
                        host.log(MK_WARNING, &msg);
                        host.post_kill_and_delete_thread(nzb.post_info_ptr);
                    }
                }
            }
        });

        wait_start = host.opts().cur_time;
        while host.opts().cur_time - wait_start < 5 {
            let mut any_running = false;
            host.with_locked_queue(|dq_ptr| {
                let qlen = host.queue_len(dq_ptr);
                for qi in 0..qlen {
                    let nzb = host.nzb_snap(host.queue_at(dq_ptr, qi), false);
                    if nzb.has_unpack_thread || nzb.has_live_repair_thread {
                        any_running = true;
                        break;
                    }
                }
            });
            if !any_running {
                break;
            }
            host.sleep_ms(200);
        }

        host.with_locked_queue(|dq_ptr| {
            let qlen = host.queue_len(dq_ptr);
            for qi in 0..qlen {
                let nzb_ptr = host.queue_at(dq_ptr, qi);
                host.nzb_action(nzb_ptr, NZB_ACT_DISCONNECT_THREADS);
            }
        });

        host.log(MK_DEBUG, b"PrePostProcessor: Jobs are completed");
    }

    fn sanitise_post_queue(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        use crate::queuesubsys_host::{
            MK_INFO, NK_NZB, POST_INT_REQUEST_PAR_CHECK, POST_INT_WORKING,
        };
        use std::sync::atomic::Ordering;

        host.with_locked_queue(|dq_ptr| {
            let qlen = host.queue_len(dq_ptr);
            let entries: Vec<(i32, NzbRef)> = (0..qlen)
                .map(|i| {
                    let p = host.queue_at(dq_ptr, i);
                    (host.nzb_snap(p, false).id, p)
                })
                .collect();

            for (nzb_id, nzb_ptr) in entries {
                if host.queue_find_by_id(dq_ptr, nzb_id) != nzb_ptr {
                    continue;
                }
                let mut nzb = host.nzb_snap(nzb_ptr, false);
                if nzb.direct_unpack_status == DirectUnpackStatus::Running
                    && !nzb.has_unpack_thread
                {
                    host.nzb_set_direct_unpack_status(nzb_ptr, DirectUnpackStatus::None);
                }
                if nzb.post_info_ptr.is_none()
                    && nzb.kind == NK_NZB
                    && !host.nzb_completed_files(nzb_ptr).is_empty()
                    && nzb.delete_status == DeleteStatus::None
                    && nzb.is_download_completed
                {
                    let mut msg = b"Collection ".to_vec();
                    msg.extend_from_slice(&nzb.name);
                    msg.extend_from_slice(b" completely downloaded");
                    host.nzb_print_message(nzb_ptr, MK_INFO, &msg);
                    self.nzb_downloaded(host, dq_ptr, nzb_ptr);
                    if host.queue_find_by_id(dq_ptr, nzb_id) != nzb_ptr {
                        continue;
                    }
                    nzb = host.nzb_snap(nzb_ptr, false);
                    if nzb.post_info_ptr.is_some() {
                        self.queued_jobs.fetch_sub(1, Ordering::AcqRel);
                    }
                }
                if nzb.post_info_ptr.is_some() {
                    self.queued_jobs.fetch_add(1, Ordering::AcqRel);
                    let ps = host.post_snap(nzb.post_info_ptr);
                    let dest_exists =
                        crate::fsops::directory_exists(&nzb.dest_dir, &crate::fsops::posix_stat);
                    let (new_stage, req_par) = sanitise_post_stage(
                        ps.stage,
                        dest_exists,
                        nzb.unpack_tried,
                        nzb.par_status,
                    );
                    host.post_set_stage(nzb.post_info_ptr, new_stage);
                    if req_par {
                        host.post_set_int(nzb.post_info_ptr, POST_INT_REQUEST_PAR_CHECK, 1);
                    }
                    host.post_set_int(nzb.post_info_ptr, POST_INT_WORKING, 0);
                }
            }
        });
    }

    fn has_active_unpack(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        include_waiting_volume: bool,
    ) -> bool {

        let jobs = self.active_jobs.lock().unwrap().clone();
        for job_ptr in jobs {
            let nzb = host.nzb_snap(job_ptr, false);
            if nzb.has_post_info && nzb.post_stage == PostStage::Unpacking {
                return true;
            }
        }

        let qlen = host.queue_len(dq_ptr);
        for qi in 0..qlen {
            let nzb_ptr = host.queue_at(dq_ptr, qi);
            let nzb = host.nzb_snap(nzb_ptr, false);
            if nzb.has_unpack_thread
                && host.direct_unpack_is_active(nzb_ptr, include_waiting_volume)
            {
                return true;
            }
        }

        false
    }

    pub(crate) fn nzb_added(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb_ptr: NzbRef,
    ) {
        use crate::queuesubsys_host::{
            EA_GROUP_PAUSE_EXTRA_PARS, EA_GROUP_SORT_FILES, MK_INFO, PC_FORCE,
        };

        let opts = host.opts();
        let nzb = host.nzb_snap(nzb_ptr, false);

        if opts.par_check != PC_FORCE {
            host.dq_edit_entry(dq_ptr, nzb.id, EA_GROUP_PAUSE_EXTRA_PARS, None);
        }

        // F16: the copy is in the queue now, so its fleet decides its primary
        // with it counted (exactly one copy downloads)
        if crate::fleethold::holds_in_queue(host, &nzb) {
            crate::fleethold::refresh_key_host(host, dq_ptr, &nzb.dupe_key);
        }

        if opts.reorder_files && nzb.delete_status == DeleteStatus::None {
            let mut msg = b"Reordering files for ".to_vec();
            msg.extend_from_slice(&nzb.name);
            host.nzb_print_message(nzb_ptr, MK_INFO, &msg);
            host.dq_edit_entry(dq_ptr, nzb.id, EA_GROUP_SORT_FILES, None);
        }
        #[cfg(unix)]
        if nzb.delete_status == DeleteStatus::None {
            crate::dupefallback::prioritize_par_index_host(host, nzb_ptr);
        }

        let nzb = host.nzb_snap(nzb_ptr, false);
        if matches!(
            nzb.delete_status,
            DeleteStatus::Dupe | DeleteStatus::Copy | DeleteStatus::Good | DeleteStatus::Scan
        ) {
            self.nzb_completed(host, dq_ptr, nzb_ptr, false);
        }
    }

    pub(crate) fn check_download_completed(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb_ptr: NzbRef,
    ) {
        use crate::queuesubsys_host::MK_INFO;

        let opts = host.opts();
        let nzb = host.nzb_snap(nzb_ptr, false);
        let completed_not_empty = !host.nzb_completed_files(nzb_ptr).is_empty();
        if should_complete_after_direct_rename(
            host.prepost_is_stopped(self.owner),
            nzb.direct_rename_status,
            nzb.has_post_info,
            nzb.delete_status,
            completed_not_empty,
            nzb.is_download_completed,
            opts.raw_article,
            opts.skip_write,
            nzb.skip_disk_write,
        ) {
            let mut msg = b"Collection ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b" completely downloaded");
            host.nzb_print_message(nzb_ptr, MK_INFO, &msg);
            self.nzb_downloaded(host, dq_ptr, nzb_ptr);
        }
    }

    pub(crate) fn nzb_downloaded(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb_ptr: NzbRef,
    ) {
        use crate::queuesubsys_host::{
            MK_INFO, NZB_ACT_ENTER_POST_PROCESS, NZB_INT_CHANGED, NZB_INT_CLEANUP_DISK, PC_FORCE,
            POST_INT_WORKING,
        };
        use std::sync::atomic::Ordering;

        if nzb_ptr.is_some() {
            // Every file is written: nothing more is reported for live extraction.
            crate::livewrite::forget(host.nzb_snap(nzb_ptr, false).id);
            #[cfg(unix)]
            crate::queuecoord::discard_par_probes_at_download_end(host, nzb_ptr);
        }
        if nzb_ptr.is_none() {
            return;
        }

        let opts = host.opts();
        let nzb = host.nzb_snap(nzb_ptr, false);
        const PC_ALWAYS: i32 = 1;

        let (route, _, set_par_skipped) = classify_nzb_downloaded_entry(
            nzb.skip_script_processing,
            nzb.skip_disk_write,
            opts.skip_write,
            nzb.delete_status,
            nzb.has_post_info,
            opts.raw_article,
            nzb.par_status,
            opts.par_check,
            PC_ALWAYS,
            PC_FORCE,
        );

        if route == 1 {
            if nzb.has_live_repair_thread {
                host.live_repair_detach_and_stop(nzb_ptr);
            }
            host.nzb_set_int(nzb_ptr, NZB_INT_CLEANUP_DISK, 1);
            let nzb2 = host.nzb_snap(nzb_ptr, false);
            crate::parlive::forget_download_writes(nzb2.id);
            crate::livewrite::forget(nzb2.id);
            crate::postproc::direct_unpack_join_live_par_repair(nzb2.id);
            delete_cleanup_host(host, &opts, &nzb2);
            self.nzb_completed(host, dq_ptr, nzb_ptr, true);
            return;
        }

        if route == 2 {
            let mut msg = b"Queueing ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b" for post-processing");
            host.nzb_print_message(nzb_ptr, MK_INFO, &msg);
            host.nzb_action(nzb_ptr, NZB_ACT_ENTER_POST_PROCESS);
            if set_par_skipped {
                host.nzb_set_par_status(nzb_ptr, NzbParStatus::Skipped);
            }

            let nzb2 = host.nzb_snap(nzb_ptr, false);
            if nzb2.has_unpack_thread || nzb2.has_live_repair_thread {
                if nzb2.post_info_ptr.is_some() {
                    host.post_set_int(nzb2.post_info_ptr, POST_INT_WORKING, 1);
                }
                self.active_jobs.lock().unwrap().push(nzb_ptr);
                if nzb2.has_unpack_thread {
                    host.direct_unpack_nzb_downloaded(dq_ptr, nzb_ptr);
                }
            }

            host.nzb_set_int(nzb_ptr, NZB_INT_CHANGED, 1);
            host.dq_save_changed(dq_ptr);

            let _wg = self.wait_mutex.lock().unwrap();
            self.queued_jobs.fetch_add(1, Ordering::AcqRel);
            self.wait_cond.notify_all();
        } else {
            if nzb.has_unpack_thread {
                host.direct_unpack_nzb_downloaded(dq_ptr, nzb_ptr);
            }
            if nzb.has_live_repair_thread {
                host.live_repair_detach_and_stop(nzb_ptr);
            }
            self.nzb_completed(host, dq_ptr, nzb_ptr, true);
        }
    }

    pub(crate) fn edit_list(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        ids: &[i32],
        action: i32,
        _args: Option<&[u8]>,
    ) -> bool {
        use crate::queuesubsys_host::{EA_POST_DELETE, MK_DEBUG};
        host.log(MK_DEBUG, b"Edit-command for post-processor received");
        if action == EA_POST_DELETE {
            self.post_queue_delete(host, dq_ptr, ids)
        } else {
            false
        }
    }

    pub(crate) fn download_queue_update(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        action: i32,
        dq_ptr: usize,
        nzb_ptr: NzbRef,
        file_ptr: FileRef,
    ) {
        use crate::queuesubsys_host::{
            ASPECT_FILE_COMPLETED, ASPECT_FILE_DELETED, ASPECT_NZB_ADDED, ASPECT_NZB_DELETED,
            ASPECT_NZB_FOUND, ASPECT_NZB_NAMED, ASPECT_URL_ADDED, ASPECT_URL_DELETED,
            ASPECT_URL_FAILED, ASPECT_URL_FOUND, MK_INFO,
        };

        if host.prepost_is_stopped(self.owner) {
            return;
        }

        if action == ASPECT_NZB_FOUND || action == ASPECT_URL_FOUND {
            let opts = host.opts();
            let nzb = host.nzb_snap(nzb_ptr, false);
            if opts.dupe_check && nzb.dupe_mode != DupeMode::Force {
                crate::dupecoord::nzb_found_host(host, dq_ptr, nzb_ptr);
            }
        } else if action == ASPECT_NZB_ADDED || action == ASPECT_URL_ADDED {
            self.nzb_added(host, dq_ptr, nzb_ptr);
        } else if action == ASPECT_NZB_NAMED {
            self.check_download_completed(host, dq_ptr, nzb_ptr);
        } else if action == ASPECT_NZB_DELETED {
            let nzb = host.nzb_snap(nzb_ptr, false);
            if nzb.deleting && !nzb.has_post_info && nzb.file_count == 0 {
                let mut msg = b"Collection ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(b" deleted from queue");
                host.nzb_print_message(nzb_ptr, MK_INFO, &msg);
                self.nzb_deleted(host, dq_ptr, nzb_ptr);
            }
        } else if action == ASPECT_URL_DELETED {
            self.nzb_deleted(host, dq_ptr, nzb_ptr);
        } else if action == ASPECT_URL_FAILED {
            self.nzb_completed(host, dq_ptr, nzb_ptr, true);
        } else if action == crate::queuesubsys_host::ASPECT_LIVE_HEAD {
            self.live_head_written(host, dq_ptr, nzb_ptr);
        } else if action == ASPECT_FILE_COMPLETED || action == ASPECT_FILE_DELETED {
            if action == ASPECT_FILE_COMPLETED {
                self.file_downloaded(host, dq_ptr, nzb_ptr, file_ptr);
            }

            let opts = host.opts();
            let file = host.file_snap(file_ptr);
            if opts.parcheck_enabled {
                let jobs = self.active_jobs.lock().unwrap().clone();
                for job_ptr in jobs {
                    if job_ptr.is_some() && file.nzb_ptr == job_ptr {
                        let job_nzb = host.nzb_snap(job_ptr, false);
                        if job_nzb.post_info_ptr.is_some() {
                            let ps = host.post_snap(job_nzb.post_info_ptr);
                            if ps.has_post_thread
                                && ps.stage >= PostStage::LoadingPars
                                && ps.stage <= PostStage::VerifyingRepaired
                                && host.repair_add_par(
                                    job_nzb.post_info_ptr,
                                    file_ptr,
                                    action == ASPECT_FILE_DELETED,
                                )
                            {
                                return;
                            }
                        }
                    }
                }
            }

            let file_nzb = host.nzb_snap(file.nzb_ptr, false);
            let nzb = host.nzb_snap(nzb_ptr, false);
            let file_act = classify_file_event(
                action == ASPECT_FILE_COMPLETED,
                action == ASPECT_FILE_DELETED,
                file.dupe_deleted,
                file_nzb.delete_status,
                nzb.has_post_info,
                nzb.is_download_completed,
                nzb.is_download_completed_strict,
            );
            if file_act == 1 {
                let mut msg = b"Collection ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(b" completely downloaded");
                host.nzb_print_message(nzb_ptr, MK_INFO, &msg);
                self.nzb_downloaded(host, dq_ptr, nzb_ptr);
            } else if file_act == 2 {
                let mut msg = b"Collection ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(b" deleted from queue");
                host.nzb_print_message(nzb_ptr, MK_INFO, &msg);
                self.nzb_deleted(host, dq_ptr, nzb_ptr);
            }
        }
    }

    fn nzb_deleted(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb_ptr: NzbRef,
    ) {
        use crate::queuesubsys_host::NZB_INT_DELETING;

        let nzb = host.nzb_snap(nzb_ptr, false);
        if nzb.has_unpack_thread {
            host.direct_unpack_nzb_deleted(dq_ptr, nzb_ptr);
        }
        if nzb.has_live_repair_thread {
            host.live_repair_detach_and_stop(nzb_ptr);
        }

        let nzb = host.nzb_snap(nzb_ptr, false);
        let (new_status, call_downloaded) = resolve_nzb_deleted_route(nzb.delete_status);
        host.nzb_set_delete_status(nzb_ptr, new_status.into());
        host.nzb_set_int(nzb_ptr, NZB_INT_DELETING, 0);

        let opts = host.opts();
        let nzb2 = host.nzb_snap(nzb_ptr, false);
        crate::parlive::forget_download_writes(nzb2.id);
        crate::livewrite::forget(nzb2.id);
        crate::postproc::forget_dead_volume_verdicts(nzb2.id);
        crate::donorstore::forget(nzb2.id);
        crate::postproc::forget_put_back(nzb2.id);
        dupe_gate_forget(nzb2.id);
        crate::postproc::direct_unpack_join_live_par_repair(nzb2.id);
        delete_cleanup_host(host, &opts, &nzb2);

        if call_downloaded {
            self.nzb_downloaded(host, dq_ptr, nzb_ptr);
        } else {
            self.nzb_completed(host, dq_ptr, nzb_ptr, true);
        }
    }

    fn nzb_completed(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb_ptr: NzbRef,
        save_queue: bool,
    ) {
        use crate::queuesubsys_host::{DH_REDOWNLOAD_AUTO, NZB_INT_AVOID_HISTORY};

        let opts = host.opts();
        let nzb = host.nzb_snap(nzb_ptr, false);
        if !nzb.has_unpack_thread {
            crate::postproc::direct_unpack_forget_holds(nzb.id);
        }
        let fails_over = crate::dupecoord::fails_over_snap(&opts, &nzb);
        let (add_to_history, call_dupe, _, need_save) = eval_nzb_completed(
            nzb.dupe_hint,
            DH_REDOWNLOAD_AUTO,
            opts.keep_history,
            nzb.avoid_history,
            opts.dupe_check,
            nzb.dupe_mode,
            fails_over,
            nzb.delete_status,
        );

        if add_to_history {
            host.history_add_to_history(dq_ptr, nzb_ptr);
        }
        host.nzb_set_int(nzb_ptr, NZB_INT_AVOID_HISTORY, 0);
        if call_dupe {
            crate::dupecoord::nzb_completed_host(host, dq_ptr, nzb_ptr);
        }
        crate::dupecoord::rescue_on_health_failure_host(host, dq_ptr, nzb_ptr);
        if add_to_history {
            crate::dupecoord::keep_as_donor_host(host, dq_ptr, nzb_ptr);
        }
        if !add_to_history {
            crate::postproc::live_par_time_settle(host, nzb_ptr, false);
            // a fleet copy kept with its files (fleethold::KEEP_FILES_PARAM)
            // may share the primary's folder: its disk is left alone
            if !host
                .nzb_get_param(nzb_ptr, crate::fleethold::KEEP_FILES_PARAM)
                .is_some_and(|v| v.trim_ascii().eq_ignore_ascii_case(b"yes"))
            {
                host.history_delete_disk_files(nzb_ptr);
            }
            crate::dupefallback::par_damage_forget_all(nzb.id);
            host.queue_remove(dq_ptr, nzb_ptr);
        }
        if save_queue && need_save {
            host.dq_save(dq_ptr);
        }
    }

    fn check_request_par(&self, host: &crate::queuesubsys_host::QueueSubsysHost, dq_ptr: usize) {
        use crate::queuesubsys_host::{
            EA_GROUP_RESUME, MK_INFO, NZB_ACT_CLEAR_SCRIPT_STATUSES, PC_MANUAL,
            POST_INT_FORCE_PAR_FULL, POST_INT_REQUEST_PAR_CHECK, POST_INT_WORKING,
        };

        let opts = host.opts();
        if !opts.parcheck_enabled {
            return;
        }

        let jobs = self.active_jobs.lock().unwrap().clone();
        for job_ptr in jobs {
            let nzb = host.nzb_snap(job_ptr, false);
            if nzb.post_info_ptr.is_none() {
                continue;
            }
            let ps = host.post_snap(nzb.post_info_ptr);
            let (act, new_force_full) = classify_request_par_action(
                ps.request_par_check,
                nzb.par_status,
                ps.force_repair,
                nzb.par_full,
                opts.par_check,
                PC_MANUAL,
                ps.force_par_full,
                nzb.file_count == 0,
            );
            if act == 1 {
                host.post_set_int(ps.ptr, POST_INT_FORCE_PAR_FULL, i32::from(new_force_full));
                host.nzb_set_par_status(nzb.ptr, NzbParStatus::None);
                host.post_set_int(ps.ptr, POST_INT_REQUEST_PAR_CHECK, 0);
                host.nzb_action(nzb.ptr, NZB_ACT_CLEAR_SCRIPT_STATUSES);
                host.post_set_stage(ps.ptr, PostStage::Queued);
                host.post_set_int(ps.ptr, POST_INT_WORKING, 0);
            } else if act == 2 || act == 3 {
                host.post_set_int(ps.ptr, POST_INT_REQUEST_PAR_CHECK, 0);
                host.nzb_set_par_status(nzb.ptr, NzbParStatus::Manual);
                if act == 2 {
                    let mut msg =
                        b"Downloading all remaining files for manual par-check for ".to_vec();
                    msg.extend_from_slice(&nzb.name);
                    host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
                    host.dq_edit_entry(dq_ptr, nzb.id, EA_GROUP_RESUME, None);
                    host.post_set_stage(ps.ptr, PostStage::Finished);
                } else {
                    let mut msg = b"There are no par-files remain for download for ".to_vec();
                    msg.extend_from_slice(&nzb.name);
                    host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
                }
                host.post_set_int(ps.ptr, POST_INT_WORKING, 0);
            }
        }
    }

    fn cleanup_jobs(&self, host: &crate::queuesubsys_host::QueueSubsysHost, dq_ptr: usize) {
        use crate::queuesubsys_host::NZB_INT_POST_TOTAL_SEC;

        let jobs = self.active_jobs.lock().unwrap().clone();
        let cur_time = host.opts().cur_time;
        for job_ptr in jobs {
            let nzb = host.nzb_snap(job_ptr, false);
            if nzb.post_info_ptr.is_none() {
                self.active_jobs.lock().unwrap().retain(|&p| p != job_ptr);
                continue;
            }
            let ps = host.post_snap(nzb.post_info_ptr);
            let (ready, upd_sec, is_comp, new_sec) = eval_cleanup_job_state(
                ps.working,
                ps.has_post_thread && ps.post_thread_running,
                ps.stage,
                ps.deleted,
                ps.start_time,
                cur_time,
                nzb.post_total_sec,
            );
            if ready {
                self.active_jobs.lock().unwrap().retain(|&p| p != job_ptr);
                host.post_delete_thread(ps.ptr);
                host.post_reset_stage_progress(ps.ptr, 0);
                if upd_sec {
                    host.nzb_set_int(job_ptr, NZB_INT_POST_TOTAL_SEC, new_sec);
                    host.post_set_start_time(ps.ptr, 0);
                }
                if is_comp {
                    self.job_completed(host, dq_ptr, job_ptr);
                } else {
                    host.post_set_stage(ps.ptr, PostStage::Queued);
                }
            }
        }
    }

    fn active_job_stages(&self, host: &crate::queuesubsys_host::QueueSubsysHost) -> Vec<PostStage> {
        let jobs = self.active_jobs.lock().unwrap().clone();
        let mut stages = Vec::with_capacity(jobs.len());
        for job_ptr in jobs {
            let nzb = host.nzb_snap(job_ptr, false);
            if nzb.has_post_info {
                stages.push(nzb.post_stage);
            }
        }
        stages
    }

    /// Whether direct unpack of download `nzb_ptr` may start now: not while another copy of its
    /// release (its duplicate key) extracts or went past the post gate (`dupe_post_gate`), so
    /// only one copy extracts into the destination. (Under the one global unpack lock a second
    /// copy's direct unpack never started while the first one's ran; it reached the post gate
    /// instead, and still does.)
    ///
    /// Direct unpack sessions waiting for volumes hold no slot but keep their decoder state
    /// and open volumes; past `unpackslots::session_limit` sessions this is logged (once per
    /// limit crossing) but the new one still starts: the download that downloads now goes
    /// first, and stopping another session (a cancel) would delete its borrowed volumes.
    fn room_for_direct_unpack(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb_ptr: NzbRef,
    ) -> bool {
        if dupe_post_gate_host(host, dq_ptr, &host.nzb_snap(nzb_ptr, false)).0 != GateVerdict::Pass {
            return false;
        }
        static OVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let limit = crate::unpackslots::session_limit();
        let sessions = (0..host.queue_len(dq_ptr))
            .filter(|&qi| host.nzb_snap(host.queue_at(dq_ptr, qi), false).has_unpack_thread)
            .count();
        let over = sessions >= limit;
        if over && !OVER.swap(true, std::sync::atomic::Ordering::AcqRel) {
            let mut msg = b"Starting the direct unpack of ".to_vec();
            msg.extend_from_slice(crate::cstr::until_nul(&host.nzb_snap(nzb_ptr, false).name));
            msg.extend_from_slice(
                format!(
                    " although {sessions} direct unpacks exist (UnpackSlots suggests at most {limit}); paused ones waiting for volumes keep their state"
                )
                .as_bytes(),
            );
            host.nzb_print_message(nzb_ptr, crate::queuesubsys_host::MK_WARNING, &msg);
        } else if !over {
            OVER.store(false, std::sync::atomic::Ordering::Release);
        }
        true
    }

    /// Whether no more finished jobs may enter their unpack stage now
    /// (`unpackslots::room_for_finished`): other finished jobs' unpacks holding or waiting for
    /// a slot, and jobs already in their unpack stage, fill the slots. Direct unpacks holding
    /// or waiting keep no job out: its unpack waits for a slot in line (and ages there), so a
    /// live extraction that takes the slot again for each article cannot hold it back for ever.
    fn unpack_slot_lacking(&self, host: &crate::queuesubsys_host::QueueSubsysHost) -> bool {
        let unpacking = self
            .active_job_stages(host)
            .iter()
            .filter(|&&s| s == PostStage::Unpacking)
            .count();
        let starting = unpacking.saturating_sub(crate::unpackslots::post_users());
        !crate::unpackslots::room_for_finished(starting)
    }

    fn can_run_more_jobs(&self, host: &crate::queuesubsys_host::QueueSubsysHost) -> (bool, bool) {
                let stages = self.active_job_stages(host);
        let opts = host.opts();
        can_run_more_jobs(&stages, opts.post_strategy)
    }

    fn pick_next_job(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        allow_par: bool,
    ) -> NzbRef {
        let active = self.active_jobs.lock().unwrap().clone();
        let qlen = host.queue_len(dq_ptr);
        let mut ptrs = Vec::with_capacity(qlen);
        let mut cands = Vec::with_capacity(qlen);
        for qi in 0..qlen {
            let nzb_ptr = host.queue_at(dq_ptr, qi);
            let nzb = host.nzb_snap(nzb_ptr, false);
            let (working, need_par, need_unpack) = if nzb.post_info_ptr.is_some() {
                let ps = host.post_snap(nzb.post_info_ptr);
                (ps.working, ps.need_par_check, ps.need_unpack)
            } else {
                (false, false, false)
            };
            ptrs.push(nzb_ptr);
            // A copy held at the post gate is picked again once the gate lets it through.
            let dupe_gate_held = nzb.post_info_ptr.is_some()
                && !working
                && dupe_gate_has(&DUPE_GATE_HELD, nzb.id)
                && dupe_post_gate_host(host, dq_ptr, &nzb).0 != GateVerdict::Pass;
            cands.push(RsPostJobCandidate {
                dupe_gate_held: u8::from(dupe_gate_held),
                has_post_info: u8::from(nzb.post_info_ptr.is_some()),
                working: u8::from(working),
                has_queue_script_job: 0,
                direct_unpack_running: u8::from(
                    nzb.direct_unpack_status == DirectUnpackStatus::Running
                        && !crate::postproc::direct_unpack_awaiting_repair(nzb.id),
                ),
                // Pending live work (a recovery volume still being folded, or a
                // running live repair) defers only this job, never the queue.
                has_live_repair_thread: u8::from(
                    nzb.has_live_repair_thread || {
                        #[cfg(unix)]
                        let defer = nzb.is_download_completed
                            && crate::postproc::live_repair_should_defer_post(nzb.id);
                        #[cfg(not(unix))]
                        let defer = false;
                        defer
                    },
                ),
                priority: nzb.priority,
                force_priority: u8::from(nzb.force_priority),
                need_par_check: u8::from(need_par),
                need_unpack: u8::from(need_unpack),
                in_active_jobs: u8::from(active.contains(&nzb_ptr)),
                download_completed: u8::from(nzb.is_download_completed),
            });
        }

        let has_active_unpack = self.unpack_slot_lacking(host);
        let pause_post = host.opts().pause_post_process;
        let idx = pick_next_post_job(&cands, allow_par, has_active_unpack, pause_post);
        if idx >= 0 {
            ptrs[idx as usize]
        } else {
            NzbRef::NONE
        }
    }

    fn check_post_queue(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {

        host.with_locked_queue(|dq_ptr| {
            let count_before = self.active_jobs.lock().unwrap().len();
            self.check_request_par(host, dq_ptr);
            self.cleanup_jobs(host, dq_ptr);
            let mut changed = self.active_jobs.lock().unwrap().len() != count_before;

            loop {
                let (can_run, allow_par) = self.can_run_more_jobs(host);
                if !can_run || host.prepost_is_stopped(self.owner) {
                    break;
                }
                let post_job = self.pick_next_job(host, dq_ptr, allow_par);
                if post_job.is_none() {
                    break;
                }
                self.active_jobs.lock().unwrap().push(post_job);

                let nzb = host.nzb_snap(post_job, false);
                if nzb.post_info_ptr.is_some() {
                    let ps = host.post_snap(nzb.post_info_ptr);
                    let pause_post = host.opts().pause_post_process;
                    if ps.stage == PostStage::Queued && (!pause_post || nzb.force_priority) {
                        self.start_job(host, dq_ptr, nzb.post_info_ptr, allow_par);
                        self.check_request_par(host, dq_ptr);
                        self.cleanup_jobs(host, dq_ptr);
                        changed = true;
                    }
                }
            }

            if changed {
                host.dq_save(dq_ptr);
                self.update_pause_state(host);
            }

            let empty = self.active_jobs.lock().unwrap().is_empty();
            host.set_standby_mode(empty);
        });
    }

    fn enter_stage(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        post_ptr: crate::queuesubsys_host::PostRef,
        stage: PostStage,
    ) {
        use crate::queuesubsys_host::POST_INT_WORKING;
        host.post_set_int(post_ptr, POST_INT_WORKING, 1);
        host.post_set_stage(post_ptr, stage);
    }

    fn start_job(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        post_ptr: crate::queuesubsys_host::PostRef,
        allow_par: bool,
    ) {
        use crate::queuesubsys_host::{
            CTRL_CLEANUP, CTRL_MOVE, CTRL_POST_RENAME_DL, CTRL_POST_RENAME_UNPACK,
            CTRL_POST_SCRIPT, CTRL_RENAME_PAR, CTRL_RENAME_RAR, CTRL_REPAIR, CTRL_STREAM_REPAIR,
            CTRL_UNPACK, DAF_STREAM, FILE_INT_EXTRA_PRIORITY, FILE_INT_PAUSED, HC_DUPE, MK_INFO,
            MK_WARNING, NZB_INT_CHANGED, PC_AUTO, POST_INT_NEED_PAR_CHECK, POST_INT_NEED_UNPACK,
            POST_INT_PAR_CANNOT_COVER, POST_INT_REQUEST_PAR_CHECK, PS_DUPE, RS_NONE,
        };

        let opts = host.opts();
        let ps = host.post_snap(post_ptr);
        let nzb = host.nzb_snap(ps.nzb_ptr, false);

        if !nzb.has_unpack_thread {
            // No direct unpack is paused on this download: its damaged volumes are repaired
            // and unpacked by the stages below, nothing may stay on hold. Dead volumes
            // borrowed already are kept for them (written as real volumes, not fetched again).
            crate::postproc::direct_unpack_forget_holds_keep_borrows(nzb.id);
        }

        if ps.start_time == 0 {
            host.post_set_start_time(post_ptr, opts.cur_time);
        }
        host.post_reset_stage_progress(post_ptr, opts.cur_time);

        if nzb.par_status <= NzbParStatus::Skipped
            && nzb.delete_status == DeleteStatus::None
            && crate::parchecker::streaming_session_has_session(nzb.id)
            && !crate::parchecker::streaming_session_cover_sealed(nzb.id)
        {
            // Seals live PAR's cover (its files as they are now) before any stage may touch
            // them, so the par decision below sees a change made in between.
            let _ = live_par_cover(host, &nzb, &ps);
        }

        if nzb.par_rename_status == RS_NONE
            && nzb.delete_status == DeleteStatus::None
            && opts.par_rename
        {
            self.enter_stage(host, post_ptr, PostStage::ParRenaming);
            host.post_controller_start_job(post_ptr, CTRL_RENAME_PAR);
            return;
        }

        let mut stream_repair = opts.dupe_article_fallback >= DAF_STREAM
            && nzb.has_stream_repair_jobs
            && nzb.delete_status == DeleteStatus::None;

        if stream_repair
            && !ps.stream_repair_done
            && !crate::options::dupe_borrow_first()
            && opts.health_check == HC_DUPE
            && crate::dupecoord::fails_over_snap(&opts, &nzb)
            && crate::dupecoord::find_dupe_backup_host(
                host,
                &opts,
                dq_ptr,
                nzb.ptr,
                &nzb.name,
                &nzb.dupe_key,
            )
            .is_some_and(|(_, backup)| {
                // Direct unpack paused at damaged volumes keeps going only if they are borrowed:
                // switching to a backup that could lend them would start the extraction over.
                !(crate::postproc::direct_unpack_has_holds(nzb.id)
                    && !crate::postproc::dead_volume_donor_cannot_lend(
                        nzb.id,
                        &backup.queued_filename,
                    )
                    && backup_can_lend(
                        host.nzb_get_param(nzb.ptr, crate::donorkind::KIND_PARAM)
                            .as_deref(),
                        host.nzb_get_param(backup.ptr, crate::donorkind::KIND_PARAM)
                            .as_deref(),
                        crate::queuecoord::backup_may_lend_while_extracting(host, backup.ptr),
                    ))
            })
        {
            let mut msg = b"Not repairing ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b" from duplicates: a backup in history is tried first");
            host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
            stream_repair = false;
        }

        if opts.parcheck_enabled {
            if stream_repair
                && !ps.stream_repair_done
                && nzb.par_status == NzbParStatus::Skipped
                && opts.par_check == PC_AUTO
                && par_cannot_cover_host(host, &nzb)
                && has_main_pars(&nzb.dest_dir)
            {
                let h_s = fmt_tenths(nzb.health);
                let c_s = fmt_tenths(nzb.critical_health_exact);
                let mut msg = b"Par2 files of ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(b" cannot cover its damage (health ");
                msg.extend_from_slice(h_s.as_bytes());
                msg.extend_from_slice(b"%, critical ");
                msg.extend_from_slice(c_s.as_bytes());
                msg.extend_from_slice(b"%), recovering from duplicates before par-check");
                host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
                host.post_set_int(post_ptr, POST_INT_PAR_CANNOT_COVER, 1);
                self.enter_stage(host, post_ptr, PostStage::StreamRepairing);
                host.post_controller_start_job(post_ptr, CTRL_STREAM_REPAIR);
                return;
            }

            if ps.par_cannot_cover
                && ps.stream_repair_done
                && !ps.stream_repair_recovered
                && nzb.dupe_recovered_bytes <= 0
                && opts.par_check == PC_AUTO
                && nzb.par_status <= NzbParStatus::None
                && nzb.delete_status == DeleteStatus::None
                && !has_dupe_scan_sources_host(host, &opts, dq_ptr, nzb.ptr)
                && has_main_pars(&nzb.dest_dir)
            {
                let h_s = fmt_tenths(nzb.health);
                let c_s = fmt_tenths(nzb.critical_health_exact);
                let mut msg = b"Skipping par-check for ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(b": its par2 files cannot cover the damage (health ");
                msg.extend_from_slice(h_s.as_bytes());
                msg.extend_from_slice(b"%, critical ");
                msg.extend_from_slice(c_s.as_bytes());
                msg.extend_from_slice(b"%) and duplicates recovered nothing");
                host.nzb_print_message(nzb.ptr, MK_WARNING, &msg);
                host.nzb_set_par_status(nzb.ptr, NzbParStatus::Failure);
                return;
            }

            if stream_repair
                && nzb.par_status == NzbParStatus::Skipped
                && has_main_pars(&nzb.dest_dir)
            {
                host.post_set_int(post_ptr, POST_INT_REQUEST_PAR_CHECK, 1);
                return;
            }

            if (nzb.par_status == NzbParStatus::None || nzb.par_status == NzbParStatus::Skipped)
                && nzb.delete_status == DeleteStatus::None
                && nzb.health < 1000
                && !has_main_pars(&nzb.dest_dir)
            {
                let flen = host.nzb_file_len(nzb.ptr);
                let mut smallest: Option<crate::queuesubsys_host::FileSnap> = None;
                for fi in 0..flen {
                    let f = host.file_snap(host.nzb_file_at(nzb.ptr, fi));
                    if f.par_file
                        && f.paused
                        && !f.deleted
                        && smallest.as_ref().map_or(true, |s| f.size < s.size)
                    {
                        smallest = Some(f);
                    }
                }
                if let Some(sm) = smallest {
                    let mut msg = b"No par2-file of ".to_vec();
                    msg.extend_from_slice(&nzb.name);
                    msg.extend_from_slice(b" on disk to check it with: unpausing ");
                    msg.extend_from_slice(&sm.filename);
                    host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
                    host.file_set_int(sm.ptr, FILE_INT_PAUSED, 0);
                    host.file_set_int(sm.ptr, FILE_INT_EXTRA_PRIORITY, 1);
                    host.nzb_set_int(nzb.ptr, NZB_INT_CHANGED, 1);
                    host.dq_save_changed(dq_ptr);
                    return;
                }
            }

            if nzb.par_status == NzbParStatus::None
                && nzb.delete_status == DeleteStatus::None
                && opts.par_check == PC_AUTO
                && nzb.direct_unpack_status == DirectUnpackStatus::Success
                && direct_unpack_read_all_damage(
                    &host.nzb_completed_files(nzb.ptr),
                    &host.nzb_extracted_archives(nzb.ptr),
                    &nzb.dest_dir,
                    crate::postproc::dead_volumes_borrowed_count(nzb.id),
                    crate::postproc::volume_end_was_rebuilt(nzb.id) || nzb.dupe_recovered_bytes > 0,
                )
            {
                // Every extracted file already passed the archive checksums: par2 would only
                // rebuild volumes nothing reads again.
                let mut msg = b"Skipping par-check for ".to_vec();
                msg.extend_from_slice(&nzb.name);
                msg.extend_from_slice(
                    b": direct unpack verified every file against the archive checksums, \
                      reading what duplicates lent and any volume rebuilt",
                );
                host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
                host.nzb_set_par_status(nzb.ptr, NzbParStatus::Success);
                host.nzb_set_int(nzb.ptr, NZB_INT_CHANGED, 1);
                let _ = crate::postproc::take_dead_volumes_borrowed(nzb.id);
                let _ = crate::postproc::take_volume_end_rebuilt(nzb.id);
                return;
            }

            if nzb.par_status == NzbParStatus::None && nzb.delete_status == DeleteStatus::None {
                if has_main_pars(&nzb.dest_dir) {
                    if trust_live_par(host, &nzb, &ps) {
                        return;
                    }
                    if !allow_par {
                        host.post_set_int(post_ptr, POST_INT_NEED_PAR_CHECK, 1);
                        return;
                    }
                    self.enter_stage(host, post_ptr, PostStage::LoadingPars);
                    host.post_set_int(post_ptr, POST_INT_NEED_PAR_CHECK, 0);
                    host.post_controller_start_job(post_ptr, CTRL_REPAIR);
                } else {
                    let mut msg = b"Nothing to par-check for ".to_vec();
                    msg.extend_from_slice(&nzb.name);
                    host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
                    host.nzb_set_par_status(nzb.ptr, NzbParStatus::Skipped);
                }
                return;
            }

            if nzb.par_status == NzbParStatus::Skipped {
                let h_act = classify_skipped_par_health(
                    nzb.par_status,
                    opts.par_scan,
                    PS_DUPE,
                    nzb.health,
                    nzb.critical_health_exact,
                    nzb.failed_size,
                    nzb.par_failed_size,
                );
                if h_act > 0 && has_main_pars(&nzb.dest_dir) {
                    if h_act == 1 || h_act == 2 {
                        if h_act == 1 {
                            let mut msg = b"Skipping par-check for ".to_vec();
                            msg.extend_from_slice(&nzb.name);
                            msg.extend_from_slice(b" due to health 0%");
                            host.nzb_print_message(nzb.ptr, MK_WARNING, &msg);
                        } else {
                            let h_s = fmt_tenths(nzb.health);
                            let c_s = fmt_tenths(nzb.critical_health_exact);
                            let mut msg = b"Skipping par-check for ".to_vec();
                            msg.extend_from_slice(&nzb.name);
                            msg.extend_from_slice(b" due to health ");
                            msg.extend_from_slice(h_s.as_bytes());
                            msg.extend_from_slice(b"% below critical ");
                            msg.extend_from_slice(c_s.as_bytes());
                            msg.push(b'%');
                            host.nzb_print_message(nzb.ptr, MK_WARNING, &msg);
                        }
                        host.nzb_set_par_status(nzb.ptr, NzbParStatus::Failure);
                    } else {
                        if trust_live_par(host, &nzb, &ps) {
                            return;
                        }
                        let h_s = fmt_tenths(nzb.health);
                        let mut msg = b"Collection ".to_vec();
                        msg.extend_from_slice(&nzb.name);
                        msg.extend_from_slice(b" with health ");
                        msg.extend_from_slice(h_s.as_bytes());
                        msg.extend_from_slice(b"% needs par-check");
                        host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
                        host.post_set_int(post_ptr, POST_INT_REQUEST_PAR_CHECK, 1);
                    }
                    return;
                }
            }
        }

        if stream_repair
            && nzb.par_status != NzbParStatus::RepairPossible
            && nzb.par_status != NzbParStatus::Manual
        {
            self.enter_stage(host, post_ptr, PostStage::StreamRepairing);
            host.post_controller_start_job(post_ptr, CTRL_STREAM_REPAIR);
            return;
        }

        // Past the repair stages only one copy of a release goes on to unpack and move.
        match dupe_post_gate_host(host, dq_ptr, &nzb) {
            (GateVerdict::Pass, _) => {
                if !nzb.dupe_key.is_empty() {
                    dupe_gate_set(&DUPE_GATE_PASSED, nzb.id, true);
                }
            }
            (verdict, other) => {
                if dupe_gate_set(&DUPE_GATE_HELD, nzb.id, true) {
                    let mut msg = b"Holding ".to_vec();
                    msg.extend_from_slice(&nzb.name);
                    msg.extend_from_slice(b" before unpacking: ");
                    msg.extend_from_slice(&other);
                    msg.extend_from_slice(if matches!(verdict, GateVerdict::WaitPassed(_)) {
                        b" is already unpacking or moving the release" as &[u8]
                    } else {
                        b" is extracting it already"
                    });
                    msg.extend_from_slice(b"; it goes on only if that copy fails");
                    host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
                }
                return;
            }
        }

        if crate::postproc::direct_unpack_awaiting_repair(nzb.id) {
            // Direct unpack paused at a damaged volume and the repair stages are over: let it
            // continue from where it stopped (or give up when the repair failed), then come back.
            // Damage the extraction found itself (a part that failed its own check on an
            // article whose transport check passed) leaves the health at 100%: only a par2
            // repair (or a twin) fixes it, never a download that merely looks whole.
            let repaired = nzb.par_status == NzbParStatus::Success
                || (nzb.health >= 1000
                    && !crate::postproc::direct_unpack_declared_damage(nzb.id)
                    && nzb.par_status != NzbParStatus::Failure
                    && nzb.par_status != NzbParStatus::RepairPossible
                    && nzb.par_status != NzbParStatus::Manual);
            // Only obfuscated files that never arrived are held: they may not be volumes at all.
            let stand_ins_only =
                !repaired && crate::postproc::direct_unpack_holds_only_absent_stand_ins(nzb.id);
            let mut msg = if repaired || stand_ins_only {
                b"Resuming direct unpack for ".to_vec()
            } else {
                b"Not resuming direct unpack for ".to_vec()
            };
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(if repaired {
                b" after repair"
            } else if stand_ins_only {
                b": the files still missing may not be volumes"
            } else {
                b": its damaged volumes were not repaired"
            });
            host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
            host.post_set_int(post_ptr, crate::queuesubsys_host::POST_INT_WORKING, 1);
            if repaired || stand_ins_only {
                crate::postproc::direct_unpack_release_all(nzb.id);
            } else {
                crate::postproc::direct_unpack_abandon(nzb.id);
            }
            return;
        }

        if nzb.direct_unpack_status == DirectUnpackStatus::Success
            && nzb.par_status == NzbParStatus::Failure
            && damage_is_borrowed_volumes(
                &host.nzb_completed_files(nzb.ptr),
                &host.nzb_extracted_archives(nzb.ptr),
                &nzb.dest_dir,
                crate::postproc::take_dead_volumes_borrowed(nzb.id),
                // Not short-circuited: both marks are read once and forgotten.
                crate::postproc::take_volume_end_rebuilt(nzb.id) | (nzb.dupe_recovered_bytes > 0),
            )
        {
            // Par-check could not restore the dead volumes, but direct unpack read them rebuilt
            // from a duplicate and the archive's own checksums verified every extracted file.
            let mut msg = b"Direct unpack of ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(
                b" verified the volumes borrowed from a duplicate: par-check result set to success",
            );
            host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
            host.nzb_set_par_status(nzb.ptr, NzbParStatus::Success);
            host.nzb_set_int(nzb.ptr, NZB_INT_CHANGED, 1);
            return;
        }

        let unpack_param = host.nzb_get_param(nzb.ptr, b"*Unpack:");
        let (want_unpack, _unpack, start_rar_rename) = eval_want_unpack_and_rar_rename(
            unpack_param.is_some(),
            unpack_param.as_deref().unwrap_or(b""),
            nzb.unpack_status,
            nzb.delete_status,
            nzb.rar_rename_status,
            RS_NONE,
            opts.rar_rename,
        );
        if start_rar_rename {
            self.enter_stage(host, post_ptr, PostStage::RarRenaming);
            host.post_controller_start_job(post_ptr, CTRL_RENAME_RAR);
            return;
        }

        if opts.parcheck_enabled
            && nzb.par_status == NzbParStatus::Skipped
            && nzb.delete_status == DeleteStatus::None
            && opts.par_check == PC_AUTO
            && !has_completed_archive_files(host, nzb.ptr)
            && has_main_pars(&nzb.dest_dir)
        {
            let mut msg = b"Requesting par-check for collection ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b" without archive files");
            host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
            host.post_set_int(post_ptr, POST_INT_REQUEST_PAR_CHECK, 1);
            return;
        }

        let dec = eval_post_stages(
            want_unpack,
            nzb.unpack_status,
            nzb.delete_status,
            nzb.par_status,
            nzb.cleanup_status,
            !opts.ext_cleanup_disk.is_empty(),
            nzb.health,
            nzb.move_status,
            &nzb.dest_dir,
            &opts.inter_dir,
            crate::paths::PATH_SEPARATOR,
            nzb.id,
            nzb.direct_unpack_status,
            opts.post_download_rename,
            nzb.post_download_renaming_status,
            !nzb.dest_dir.is_empty(),
            !nzb.name.is_empty(),
            opts.rename_after_unpack,
            nzb.post_unpack_renaming_status,
        );
        if dec.skip_unpack_due_to_par != 0 {
            let reason: &[u8] = if nzb.par_status == NzbParStatus::Manual {
                b"required par-repair"
            } else {
                b"par-failure"
            };
            let mut msg = b"Skipping unpack for ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b" due to ");
            msg.extend_from_slice(reason);
            host.nzb_print_message(nzb.ptr, MK_WARNING, &msg);
            host.nzb_set_unpack_status(nzb.ptr, UnpackStatus::Skipped);
        }

        if dec.unpack != 0 {
            if self.unpack_slot_lacking(host) {
                host.post_set_int(post_ptr, POST_INT_NEED_UNPACK, 1);
                return;
            }
            host.post_set_int(post_ptr, POST_INT_NEED_UNPACK, 0);
            self.enter_stage(host, post_ptr, PostStage::Unpacking);
            host.post_controller_start_job(post_ptr, CTRL_UNPACK);
        } else if dec.cleanup != 0 {
            self.enter_stage(host, post_ptr, PostStage::CleaningUp);
            host.post_controller_start_job(post_ptr, CTRL_CLEANUP);
        } else if dec.move_inter != 0 {
            self.enter_stage(host, post_ptr, PostStage::Moving);
            host.post_controller_start_job(post_ptr, CTRL_MOVE);
        } else if dec.post_download_renaming != 0 {
            self.enter_stage(host, post_ptr, PostStage::PostDownloadRenaming);
            host.post_controller_start_job(post_ptr, CTRL_POST_RENAME_DL);
        } else if dec.post_unpack_renaming != 0 {
            self.enter_stage(host, post_ptr, PostStage::PostUnpackRenaming);
            host.post_controller_start_job(post_ptr, CTRL_POST_RENAME_UNPACK);
        } else {
            self.enter_stage(host, post_ptr, PostStage::ExecutingScript);
            host.post_controller_start_job(post_ptr, CTRL_POST_SCRIPT);
        }
    }

    fn job_completed(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb_ptr: NzbRef,
    ) {
        use crate::queuesubsys_host::NZB_ACT_LEAVE_POST_PROCESS;
        use std::sync::atomic::Ordering;

        host.nzb_action(nzb_ptr, NZB_ACT_LEAVE_POST_PROCESS);
        let nzb = host.nzb_snap(nzb_ptr, false);
        if dupe_gate_has(&DUPE_STOPPED_IN_POST, nzb.id) && nzb.delete_status == DeleteStatus::Dupe {
            let opts = host.opts();
            if opts.health_check != crate::queuesubsys_host::HC_PARK {
                delete_cleanup_host(host, &opts, &nzb);
            }
        }
        dupe_gate_forget(nzb.id);
        if nzb.is_download_completed {
            self.nzb_completed(host, dq_ptr, nzb_ptr, false);
        }

        let _wg = self.wait_mutex.lock().unwrap();
        self.queued_jobs.fetch_sub(1, Ordering::AcqRel);
    }

    fn update_pause_state(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        use crate::queuesubsys_host::MK_INFO;

        let stages = self.active_job_stages(host);
        let opts = host.opts();
        let need_pause = calc_post_pause_state(
            &stages,
            crate::parpreset::pause_queue(opts.par_pause_queue),
            opts.unpack_pause_queue,
            opts.script_pause_queue,
        );

        if need_pause && !opts.temp_pause_download {
            host.log(MK_INFO, b"Pausing download before post-processing");
        } else if !need_pause && opts.temp_pause_download {
            host.log(MK_INFO, b"Unpausing download after post-processing");
        }

        host.work_state_set_temp_pause_download(need_pause);
    }

    fn post_queue_delete(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        ids: &[i32],
    ) -> bool {
        use crate::queuesubsys_host::{
            MK_DEBUG, MK_ERROR, MK_INFO, POST_INT_DELETED, POST_INT_WORKING,
        };

        let mut ok = false;

        for &id in ids {
            let nzb_ptr = host.queue_find_by_id(dq_ptr, id);
            if nzb_ptr.is_none() {
                continue;
            }
            let nzb = host.nzb_snap(nzb_ptr, false);
            if nzb.post_info_ptr.is_none() {
                continue;
            }
            let ps = host.post_snap(nzb.post_info_ptr);
            if ps.working {
                let mut msg = b"Deleting active post-job ".to_vec();
                msg.extend_from_slice(&nzb.name);
                host.nzb_print_message(nzb_ptr, MK_INFO, &msg);
                host.post_set_int(ps.ptr, POST_INT_DELETED, 1);
                if ps.has_post_thread {
                    let mut dbg = b"Terminating post-process thread for ".to_vec();
                    dbg.extend_from_slice(&nzb.name);
                    host.log(MK_DEBUG, &dbg);
                    host.post_stop_thread(ps.ptr);
                    ok = true;
                } else if nzb.has_unpack_thread {
                    host.direct_unpack_nzb_deleted(dq_ptr, nzb_ptr);
                    ok = true;
                } else if nzb.has_live_repair_thread {
                    host.live_repair_detach_and_stop(nzb_ptr);
                    host.post_set_int(ps.ptr, POST_INT_WORKING, 0);
                    ok = true;
                } else {
                    host.log(MK_ERROR, b"Internal error in PrePostProcessor::QueueDelete");
                }
            } else {
                let mut msg = b"Deleting queued post-job ".to_vec();
                msg.extend_from_slice(&nzb.name);
                host.nzb_print_message(nzb_ptr, MK_INFO, &msg);

                self.active_jobs.lock().unwrap().retain(|&p| p != nzb_ptr);
                self.job_completed(host, dq_ptr, nzb_ptr);
                ok = true;
            }
        }

        if ok {
            host.dq_save(dq_ptr);
        }

        ok
    }

    /// The headers of a set's first volume in download `nzb_ptr` are written (see
    /// `livewrite::take_new_heads`): direct unpack starts on the same terms as when a volume
    /// completes, and reads the set as it downloads (a running one picks the volume up itself).
    fn live_head_written(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb_ptr: NzbRef,
    ) {
        let opts = host.opts();
        let nzb = host.nzb_snap(nzb_ptr, false);
        let act = classify_file_downloaded_direct_unpack(
            opts.direct_unpack,
            opts.raw_article,
            opts.skip_write,
            nzb.skip_disk_write,
            nzb.direct_unpack_status,
            nzb.direct_rename_status,
            nzb.has_unpack_thread,
        );
        // (direct unpack waits for an unpack slot itself, see `unpackslots`)
        if act != 1 {
            return;
        }
        let want_unpack = host
            .nzb_get_param(nzb_ptr, b"*Unpack:")
            .as_ref()
            .map_or(true, |v| v.eq_ignore_ascii_case(b"yes"));
        if want_unpack && self.room_for_direct_unpack(host, dq_ptr, nzb_ptr) {
            host.direct_unpack_start_job(nzb_ptr);
        }
    }

    fn file_downloaded(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb_ptr: NzbRef,
        file_ptr: FileRef,
    ) {
        let opts = host.opts();
        let nzb = host.nzb_snap(nzb_ptr, false);
        if opts.direct_unpack {
            // A damaged archive volume is put on hold: direct unpack pauses in front of it
            // until a repair releases it, instead of being cancelled.
            let file = host.file_snap(file_ptr);
            // The .zip that ends an Info-ZIP split set is a volume of that set (`livefeed`).
            let infozip_last = file.failed_articles > 0
                && crate::livefeed::is_infozip_last(&file.filename, &nzb_file_names(host, nzb_ptr));
            if file.failed_articles > 0
                && (infozip_last
                    || crate::filetypes::may_hold_for_direct_unpack(
                        &file.filename,
                        file.success_articles,
                    ))
            {
                let full = [
                    nzb.dest_dir.as_slice(),
                    &[crate::paths::PATH_SEPARATOR],
                    file.filename.as_slice(),
                ]
                .concat();
                let articles: Vec<_> = (0..file.total_articles.max(0) as usize)
                    .map(|i| host.article_at(file_ptr, i))
                    .collect();
                let ranges =
                    crate::dupefallback::compute_holes_slice(&articles, file.decoded_file_size);
                // Where the holes fall against the volume's own headers: a hole inside a
                // member piece's data is read past wherever it lies in the volume. Placing them
                // reads only the volume's header chain (a few small reads, no key derivation:
                // encrypted headers are not opened here), so it stays on this thread; writing a
                // rebuilt head or tail goes to the thread below.
                // A volume of a 7z, ZIP or split-file set: its feed decoders read past any
                // lost range they can, and its headers wait for the repair on their own.
                let live = infozip_last || crate::livefeed::is_live_volume(&file.filename);
                let placement = if live {
                    crate::progressive_unpack::HolePlacement::Data(
                        crate::progressive_unpack::merge_holes(&byte_spans(&ranges)),
                    )
                } else if crate::filetypes::is_rar_file(&file.filename) {
                    crate::progressive_unpack::place_holes(
                        crate::fsops::path(&full),
                        &byte_spans(&ranges),
                    )
                } else {
                    crate::progressive_unpack::HolePlacement::Unknown
                };
                let holes = crate::progressive_unpack::skippable_placed(
                    &placement,
                    &ranges,
                    file.decoded_file_size,
                );
                crate::postproc::direct_unpack_hold_with_holes(nzb.id, &full, holes);
                // A lost first article took the volume's headers, so extraction would wait at
                // it for the whole repair. Its quick-open record, at the end, rebuilds them as
                // the volume finishes, and extraction reads on past the lost data. A lost
                // tail (the headers behind the last piece's data) is rebuilt from the
                // volume's own headers and length. The file work and the live PAR reconcile
                // run off this thread: the queue lock is held.
                let writes_disk = !opts.skip_write && !nzb.skip_disk_write && !opts.raw_article;
                let head = ranges.iter().any(|r| r.offset == 0 && r.size > 0);
                let tail = matches!(
                    placement,
                    crate::progressive_unpack::HolePlacement::Tail { .. }
                );
                if writes_disk && !live && (head || tail) {
                    spawn_head_rebuild(nzb.id, full, file.filename, ranges, file.decoded_file_size);
                }
            }
        }

        let act = classify_file_downloaded_direct_unpack(
            opts.direct_unpack,
            opts.raw_article,
            opts.skip_write,
            nzb.skip_disk_write,
            nzb.direct_unpack_status,
            nzb.direct_rename_status,
            nzb.has_unpack_thread,
        );
        if act == 1 {
            let file = host.file_snap(file_ptr);
            if crate::filetypes::is_rar_file(&file.filename)
                || crate::livefeed::is_live_volume(&file.filename)
            {
                // It starts whatever else extracts or post-processes: the extraction waits for
                // an unpack slot itself (`unpackslots`), an active download's first. A paused
                // download's extraction waiting for a volume holds none, so it never keeps a
                // new download from extracting while it downloads.
                let unpack_param = host.nzb_get_param(nzb_ptr, b"*Unpack:");
                let want_unpack = unpack_param
                    .as_ref()
                    .map_or(true, |v| v.eq_ignore_ascii_case(b"yes"));
                if want_unpack && self.room_for_direct_unpack(host, dq_ptr, nzb_ptr) {
                    host.direct_unpack_start_job(nzb_ptr);
                }
            }
        } else if act == 2 {
            host.direct_unpack_file_downloaded(dq_ptr, nzb_ptr, file_ptr);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_one_copy_of_a_release_goes_past_its_repair() {
        use super::{dupe_post_gate as gate, GateCopy, GateVerdict::*};
        let copy = |passed, extracting| GateCopy { passed, extracting };
        // the first copy at the gate goes; one arriving after it waits
        assert_eq!(gate(copy(false, false), &[copy(false, false)]), Pass);
        assert_eq!(gate(copy(false, false), &[copy(false, false), copy(true, false)]), WaitPassed(1));
        // even a copy whose extraction is under way waits for one already unpacking or moving
        assert_eq!(gate(copy(false, true), &[copy(true, false)]), WaitPassed(0));
        // a whole copy starting from scratch waits for a copy whose extraction carries on
        assert_eq!(gate(copy(false, false), &[copy(false, true)]), WaitExtracting(0));
        assert_eq!(gate(copy(false, true), &[copy(false, true)]), Pass);
        // once through, it stays through
        assert_eq!(gate(copy(true, false), &[copy(true, true)]), Pass);
        assert_eq!(gate(copy(false, false), &[]), Pass);
    }

    #[test]
    fn par_failure_is_overridden_only_for_borrowed_volume_damage() {
        use super::damage_is_borrowed_volumes as ok;
        use crate::queuesubsys_host::CompletedFileSnap;
        let dir = std::env::temp_dir().join(format!("rg-borrowed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_string_lossy().as_bytes().to_vec();
        let f = |name: &str, status: i32, par: bool| CompletedFileSnap {
            status,
            par_file: par,
            filename: name.as_bytes().to_vec(),
            ..Default::default()
        };
        let ex: Vec<Vec<u8>> = vec![b"a.part01.rar".to_vec(), b"a.part02.rar".to_vec()];
        let base = vec![
            f("a.part01.rar", 1, false),
            f("a.part02.rar", 2, false),
            f("a.vol00+01.par2", 3, true),
        ];
        assert!(!ok(&base, &ex, &d, 0, false), "nothing borrowed");
        assert!(
            !ok(&base, &[], &d, 0, true),
            "a damaged volume direct unpack did not read (another set) is real damage"
        );
        assert!(
            ok(&base, &ex, &d, 0, true),
            "bytes a duplicate lent into the volumes, checked by the extraction"
        );
        assert!(
            ok(&base, &ex, &d, 1, false),
            "damage only inside the archive and its par2 files"
        );
        let mut obf = base.clone();
        obf.push(f("a8f3k2", 3, false));
        obf.push(f("q9z1m0", 3, false));
        assert!(ok(&obf, &ex, &d, 2, false));
        assert!(
            !ok(&obf, &ex, &d, 1, false),
            "more dead obfuscated files than borrowed volumes"
        );
        assert!(!ok(&obf, &ex, &d, 0, true), "lent bytes rebuild no missing file");
        let mut sample = base.clone();
        sample.push(f("a.sample.mkv", 2, false));
        assert!(!ok(&sample, &ex, &d, 3, false), "a damaged loose file is real damage");
        std::fs::write(dir.join("x1y2z3"), b"partial").unwrap();
        let mut present = base;
        present.push(f("x1y2z3", 2, false));
        assert!(
            !ok(&present, &ex, &d, 3, false),
            "a damaged file on disk is not a borrowed volume"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lost_nfo_does_not_send_a_verified_extraction_to_par_check() {
        use crate::queuesubsys_host::CompletedFileSnap;
        let dir = std::env::temp_dir().join(format!("rg-lostnfo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_string_lossy().as_bytes().to_vec();
        let f = |name: &str, status: i32, par: bool| CompletedFileSnap {
            status,
            par_file: par,
            filename: name.as_bytes().to_vec(),
            ..Default::default()
        };
        std::fs::write(dir.join("a.part01.rar"), b"x").unwrap();
        std::fs::write(dir.join("a.part02.rar"), b"x").unwrap();
        let extracted: Vec<Vec<u8>> = vec![b"a.part01.rar".to_vec(), b"a.part02.rar".to_vec()];
        // The A2 shape: volumes put back (end rebuilt), the release's nfo never arrived.
        let files = vec![
            f("a.part01.rar", 2, false),
            f("a.part02.rar", 2, false),
            f("Release.nfo", 3, false),
            f("a.par2", 1, true),
        ];
        assert!(super::direct_unpack_read_all_damage(&files, &extracted, &d, 0, true));
        assert!(super::damage_is_borrowed_volumes(&files, &extracted, &d, 0, true));
        // A lost sample still counts.
        let mut sample = files.clone();
        sample.push(f("a.sample.mkv", 3, false));
        assert!(!super::direct_unpack_read_all_damage(&sample, &extracted, &d, 0, true));
        assert!(!super::damage_is_borrowed_volumes(&sample, &extracted, &d, 0, true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn par_check_is_skipped_only_when_direct_unpack_read_every_damaged_file() {
        use super::direct_unpack_read_all_damage as ok;
        use crate::queuesubsys_host::CompletedFileSnap;
        let dir = std::env::temp_dir().join(format!("rg-dureadall-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_string_lossy().as_bytes().to_vec();
        let f = |name: &str, status: i32, par: bool| CompletedFileSnap {
            status,
            par_file: par,
            filename: name.as_bytes().to_vec(),
            ..Default::default()
        };
        let extracted: Vec<Vec<u8>> = ["a.part01.rar", "a.part02.rar", "a.part03.rar"]
            .iter()
            .map(|n| n.as_bytes().to_vec())
            .collect();
        std::fs::write(dir.join("A.PART02.rar"), b"holed").unwrap();
        // part02 lost articles (repaired and read), part04 never arrived and was borrowed
        let files = vec![
            f("a.part01.rar", 1, false),
            f("A.PART02.rar", 2, false),
            f("a.part03.rar", 1, false),
            f("a.part04.rar", 3, false),
            f("a.vol00+01.par2", 3, true),
        ];
        assert!(ok(&files, &extracted, &d, 1, false));
        assert!(!ok(&files, &extracted, &d, 0, false), "a missing volume nobody borrowed");
        let mut sample = files.clone();
        std::fs::write(dir.join("a.sample.mkv"), b"partial").unwrap();
        sample.push(f("a.sample.mkv", 2, false));
        assert!(!ok(&sample, &extracted, &d, 5, false), "a damaged file direct unpack never read");
        let mut other_set = files;
        std::fs::write(dir.join("subs.rar"), b"partial").unwrap();
        other_set.push(f("subs.rar", 2, false));
        assert!(!ok(&other_set, &extracted, &d, 5, false), "an archive direct unpack did not extract");
        assert!(!ok(&[f("a.part01.rar", 1, false)], &extracted, &d, 0, false), "nothing to rebuild");
        assert!(
            !ok(&[f("A.PART02.rar", 2, false)], &extracted, &d, 0, false),
            "holes repaired in place: par-check only verifies"
        );
        assert!(
            ok(&[f("A.PART02.rar", 2, false)], &extracted, &d, 0, true),
            "bytes put back (lent, or an end rebuilt) that par2 would count as damage"
        );
        assert!(
            !ok(&[f("a.part01.rar", 1, false)], &extracted, &d, 0, false),
            "nothing damaged, nothing put back"
        );
        let patched_split = [f("A.PART02.rar", 2, false), f("Show.mkv.001", 1, false)];
        assert!(
            !ok(&patched_split, &extracted, &d, 0, true),
            "a file direct unpack never read may hold lent bytes only par-check verifies"
        );
        let with_nfo = [f("A.PART02.rar", 2, false), f("a.nfo", 1, false), f("a.SFV", 1, false)];
        assert!(ok(&with_nfo, &extracted, &d, 0, true), "side files hold no payload");
        // part03 never arrived and was read borrowed (so it is in `extracted`): it takes the
        // one borrowed slot, and a missing obfuscated file has none left.
        let borrowed_read = vec![f("a.part03.rar", 3, false)];
        assert!(ok(&borrowed_read, &extracted, &d, 1, false));
        let mut plus_obf = borrowed_read.clone();
        plus_obf.push(f("q9z1m0", 3, false));
        assert!(!ok(&plus_obf, &extracted, &d, 1, false), "one borrowed volume covers one file");
        assert!(ok(&plus_obf, &extracted, &d, 2, false));
        let mut plus_sample = borrowed_read;
        plus_sample.push(f("a.sample.mkv", 3, false));
        assert!(!ok(&plus_sample, &extracted, &d, 5, false), "a sample that never arrived");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn borrowing_beats_a_backup_only_when_it_can_lend() {
        use super::backup_can_lend as can;
        assert!(can(None, Some(b"borrowable"), false));
        assert!(can(Some(b"twin"), Some(b"twin"), false));
        // unknown: only when labelled an alt (nested in another container), never unlabelled
        assert!(!can(None, None, false));
        assert!(!can(Some(b"borrowable"), Some(b"unknown"), false));
        assert!(can(None, None, true));
        assert!(can(Some(b"borrowable"), Some(b"unknown"), true));
        assert!(!can(Some(b"borrowable"), Some(b"unborrowable"), true));
        assert!(!can(Some(b"unborrowable"), Some(b"twin"), true));
        // an encrypted store copy whose nzb password opens it lends without a label
        assert!(can(Some(b"borrowable"), Some(b"borrowable-encrypted"), false));
        assert!(can(Some(b"borrowable-encrypted"), Some(b"borrowable"), false));
    }

    use super::*;

    #[test]
    fn test_prepost_stage_and_pause() {
        assert_eq!(
            sanitise_post_stage(PostStage::ExecutingScript, true, false, NzbParStatus::None),
            (PostStage::Finished, false)
        );
        assert_eq!(
            sanitise_post_stage(PostStage::VerifyingSources, true, true, NzbParStatus::Skipped),
            (PostStage::Queued, true)
        );
        assert!(calc_post_pause_state(&[PostStage::VerifyingSources], true, false, false));
        assert!(!calc_post_pause_state(&[PostStage::VerifyingSources], false, true, true));
    }

    #[test]
    fn test_eval_post_stages_inter_dir() {
        let dec = eval_post_stages(
            false,
            UnpackStatus::None,
            DeleteStatus::None,
            NzbParStatus::Success,
            CleanupStatus::None,
            false,
            1000,
            MoveStatus::None,
            b"/inter/job1",
            b"/inter",
            b'/',
            1,
            DirectUnpackStatus::None,
            false,
            0,
            true,
            true,
            false,
            0,
        );
        assert_eq!(dec.move_inter, 1);
    }

    #[test]
    fn a_download_from_an_earlier_inter_dir_still_moves() {
        let eval = |dest: &[u8], id: i32| {
            eval_post_stages(
                false,
                UnpackStatus::None,
                DeleteStatus::None,
                NzbParStatus::Success,
                CleanupStatus::None,
                false,
                1000,
                MoveStatus::None,
                dest,
                b"/hdd/inter",
                b'/',
                id,
                DirectUnpackStatus::None,
                false,
                0,
                true,
                true,
                false,
                0,
            )
            .move_inter
        };
        assert_eq!(eval(b"/nvme/inter/Movie.#42", 42), 1);
        assert_eq!(eval(b"/dst/Movies/Movie", 42), 0);
    }

    #[test]
    fn prepost_processor_full_suite_port_from_cpp() {
        // Port of tests/postprocess/PrePostProcessor.cpp:
        // DirectRenameSchedulesCompletedDownload, DirectRenameWaitsForRemainingData,
        // DirectRenameWaitsForActiveDownloads, DirectRenameWaitsForRenamerToFinish,
        // DirectRenameDoesNotCompleteHealthDeletion, DirectRenameSchedulesAfterPausedRequestsDrain
        struct SimCompletionFixture {
            direct_rename_status: DirectRenameStatus,
            has_post_info: bool,
            delete_status: DeleteStatus,
            completed_not_empty: bool,
            par_file_only_remaining: bool,
            active_downloads: i32,
            job_count: usize,
        }

        impl SimCompletionFixture {
            fn new() -> Self {
                Self {
                    direct_rename_status: DirectRenameStatus::Success,
                    has_post_info: false,
                    delete_status: DeleteStatus::None,
                    completed_not_empty: true,
                    par_file_only_remaining: true,
                    active_downloads: 0,
                    job_count: 0,
                }
            }

            fn is_download_completed(&self, ignore_paused_pars: bool) -> bool {
                let files = if self.par_file_only_remaining {
                    [(true, true, false)]
                } else {
                    [(false, false, false)]
                };
                crate::downloadinfo::is_download_completed(
                    self.active_downloads,
                    ignore_paused_pars,
                    &files,
                )
            }

            fn check_download_completed(&mut self) {
                if should_complete_after_direct_rename(
                    false,
                    self.direct_rename_status,
                    self.has_post_info,
                    self.delete_status,
                    self.completed_not_empty,
                    self.is_download_completed(true),
                    false,
                    false,
                    false,
                ) {
                    let (route, _, _) = classify_nzb_downloaded_entry(
                        false,
                        false,
                        false,
                        self.delete_status,
                        self.has_post_info,
                        false,
                        NzbParStatus::None,
                        0,
                        2,
                        3,
                    );
                    if route == 2 {
                        self.has_post_info = true;
                        self.job_count += 1;
                    }
                }
            }
        }

        // 1. DirectRenameSchedulesCompletedDownload
        let mut f = SimCompletionFixture::new();
        assert!(f.is_download_completed(true));
        assert!(!f.has_post_info);
        f.check_download_completed();
        assert!(f.has_post_info);
        assert_eq!(f.job_count, 1);
        f.check_download_completed();
        assert_eq!(f.job_count, 1);

        // 2. DirectRenameWaitsForRemainingData
        let mut f = SimCompletionFixture::new();
        f.par_file_only_remaining = false;
        assert!(!f.is_download_completed(true));
        f.check_download_completed();
        assert!(!f.has_post_info);
        assert_eq!(f.job_count, 0);

        // 3. DirectRenameWaitsForActiveDownloads
        let mut f = SimCompletionFixture::new();
        f.active_downloads = 1;
        f.check_download_completed();
        assert!(!f.has_post_info);
        assert_eq!(f.job_count, 0);

        // 4. DirectRenameWaitsForRenamerToFinish
        let mut f = SimCompletionFixture::new();
        f.direct_rename_status = DirectRenameStatus::Running;
        f.check_download_completed();
        assert!(!f.has_post_info);
        assert_eq!(f.job_count, 0);

        // 5. DirectRenameDoesNotCompleteHealthDeletion
        let mut f = SimCompletionFixture::new();
        f.delete_status = DeleteStatus::Health;
        f.check_download_completed();
        assert!(!f.has_post_info);
        assert_eq!(f.job_count, 0);

        // 6. DirectRenameSchedulesAfterPausedRequestsDrain
        let mut f = SimCompletionFixture::new();
        f.active_downloads = 1;
        f.check_download_completed();
        assert!(!f.has_post_info);
        f.active_downloads = 0;
        f.check_download_completed();
        assert!(f.has_post_info);
        assert_eq!(f.job_count, 1);
        f.check_download_completed();
        assert_eq!(f.job_count, 1);
    }
}

/// Writes back the headers of RAR5 volume `full` (`filename` names its number) when its first
/// lost range starts the file and its quick-open record arrived, and drops them from `ranges`.
/// Returns where the headers end and where the first file data behind them ends.
/// Rebuilds a finished volume's lost headers off the queue lock (its head from its quick-open
/// record, its tail from its own headers and length), then widens its direct-unpack hold so
/// extraction reads on past the lost data, or releases it when nothing is left lost. The hold
/// set before this keeps extraction waiting at the headers meanwhile.
fn spawn_head_rebuild(
    nzb_id: i32,
    full: Vec<u8>,
    filename: Vec<u8>,
    mut ranges: Vec<crate::dupefallback::NzbgetRsByteRange>,
    file_size: i64,
) {
    use crate::progressive_unpack::HolePlacement;
    let _ = std::thread::Builder::new()
        .name("rebuild-head".to_string())
        .spawn(move || {
            let mut lines: Vec<Vec<u8>> = Vec::new();
            let mut behind_head = None;
            if ranges.iter().any(|r| r.offset == 0 && r.size > 0) {
                let Some((head_len, data_end)) = rebuild_lost_head(&full, &filename, &mut ranges)
                else {
                    return;
                };
                behind_head = Some((head_len, data_end));
                let mut msg = b"Rebuilt the headers of ".to_vec();
                msg.extend_from_slice(&filename);
                msg.extend_from_slice(
                    format!(
                        " ({head_len} bytes) from its own quick-open record as it finished; \
                         direct unpack reads on past its lost data"
                    )
                    .as_bytes(),
                );
                lines.push(msg);
            }
            let path = crate::fsops::path(&full);
            let mut spans = byte_spans(&ranges);
            let mut placement = crate::progressive_unpack::place_holes(path, &spans);
            if let HolePlacement::Tail { data_end } = placement {
                let Some(left) = crate::deadvolume::restore_tail_guarded(&full, &spans, data_end)
                else {
                    return;
                };
                let rebuilt = u64::try_from(file_size).unwrap_or(0).saturating_sub(data_end);
                let mut msg = b"Rebuilt the end of ".to_vec();
                msg.extend_from_slice(&filename);
                msg.extend_from_slice(
                    format!(
                        " ({rebuilt} bytes behind its data) from its own headers as it finished; \
                         direct unpack reads on past its lost data"
                    )
                    .as_bytes(),
                );
                lines.push(msg);
                spans = left;
                placement = crate::progressive_unpack::place_holes(path, &spans);
            }
            let holes = match placement {
                HolePlacement::Data(h) => h,
                HolePlacement::Unknown => match behind_head {
                    Some((head_len, data_end)) => {
                        crate::progressive_unpack::skippable_holes_behind_head(
                            &ranges, file_size, head_len, data_end,
                        )
                    }
                    None => return,
                },
                HolePlacement::Tail { .. } | HolePlacement::Header => return,
            };
            let moved = if holes.is_empty() {
                // Every lost byte was header, now written back: the volume is whole.
                crate::postproc::direct_unpack_release_path(&full)
            } else {
                crate::postproc::direct_unpack_widen_hold(nzb_id, &full, holes)
            };
            if !moved {
                return;
            }
            for msg in lines {
                crate::log::log_active_bytes(crate::queuesubsys_host::MK_INFO, &msg);
            }
        });
}

/// The names of every file of download `nzb_ptr`, queued and completed.
fn nzb_file_names(host: &crate::queuesubsys_host::QueueSubsysHost, nzb_ptr: NzbRef) -> Vec<Vec<u8>> {
    let mut names: Vec<Vec<u8>> = (0..host.nzb_file_len(nzb_ptr))
        .map(|i| crate::cstr::until_nul(&host.file_snap(host.nzb_file_at(nzb_ptr, i)).filename).to_vec())
        .collect();
    names.extend(
        (0..host.nzb_completed_files_len(nzb_ptr)).map(|i| crate::cstr::until_nul(&host.nzb_completed_file_at(nzb_ptr, i).filename).to_vec()),
    );
    names
}

/// `ranges` as (`offset`, `len`) pairs, dropping empty and negative ones.
fn byte_spans(ranges: &[crate::dupefallback::NzbgetRsByteRange]) -> Vec<(u64, u64)> {
    ranges
        .iter()
        .filter_map(|r| {
            let start = u64::try_from(r.offset).ok()?;
            let len = u64::try_from(r.size).ok().filter(|&l| l > 0)?;
            Some((start, len))
        })
        .collect()
}

fn rebuild_lost_head(
    full: &[u8],
    filename: &[u8],
    ranges: &mut Vec<crate::dupefallback::NzbgetRsByteRange>,
) -> Option<(i64, i64)> {
    ranges.retain(|r| r.size > 0);
    ranges.sort_by_key(|r| r.offset);
    if ranges.first()?.offset != 0 {
        return None;
    }
    let (_, number, _, _) = crate::streamrepair::rar_part_number(filename)?;
    let spans: Vec<(u64, u64)> = ranges
        .iter()
        .filter_map(|r| {
            let start = u64::try_from(r.offset).ok()?;
            Some((start, start.checked_add(u64::try_from(r.size).ok()?)?))
        })
        .collect();
    let index = usize::try_from(number.checked_sub(1)?).ok()?;
    let (head_len, data_end) = crate::deadvolume::restore_head_guarded(full, &spans, index)?;
    let (head_len, data_end) = (i64::try_from(head_len).ok()?, i64::try_from(data_end).ok()?);
    let first = &mut ranges[0];
    let end = first.offset.saturating_add(first.size);
    if end <= head_len {
        ranges.remove(0);
    } else {
        first.offset = head_len;
        first.size = end - head_len;
    }
    Some((head_len, data_end))
}

#[cfg(test)]
mod lost_head_tests {
    use crate::dupefallback::NzbgetRsByteRange as R;

    #[test]
    fn a_finished_volume_gets_its_headers_back_and_extraction_reads_past_the_rest() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/testdata/deadvolume/out.part3.rar");
        let good = std::fs::read(src).unwrap();
        let dir = std::env::temp_dir().join(format!("rg-lost-head-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.part3.rar");
        let mut lost = good.clone();
        lost[..4000].fill(0);
        std::fs::write(&path, &lost).unwrap();
        let full = path.to_str().unwrap().as_bytes().to_vec();
        let size = good.len() as i64;
        let mut ranges = vec![R { offset: 0, size: 4000 }];
        // Without the headers, extraction must wait for the repair.
        assert!(crate::progressive_unpack::skippable_holes(&ranges, size).is_empty());
        let (head, data_end) =
            super::rebuild_lost_head(&full, b"out.part3.rar", &mut ranges).unwrap();
        assert!(data_end > 4000);
        let now = std::fs::read(&path).unwrap();
        assert_eq!(now[..head as usize], good[..head as usize]);
        assert_eq!(ranges.len(), 1);
        assert_eq!((ranges[0].offset, ranges[0].size), (head, 4000 - head));
        assert_eq!(
            crate::progressive_unpack::skippable_holes_behind_head(&ranges, size, head, data_end),
            vec![(head as u64, (4000 - head) as u64)]
        );
        // A hole past the first file data, inside the usual header guard, still waits.
        let behind = [R { offset: data_end, size: 10 }];
        if data_end + 10 < 64 * 1024 {
            assert!(crate::progressive_unpack::skippable_holes_behind_head(
                &behind, size, head, data_end
            )
            .is_empty());
        }
        // A volume whose first article arrived is left alone.
        let mut later = vec![R { offset: 8000, size: 100 }];
        assert_eq!(super::rebuild_lost_head(&full, b"out.part3.rar", &mut later), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
