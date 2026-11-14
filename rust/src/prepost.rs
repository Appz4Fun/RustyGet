//! End-to-end Rust implementation of `PrePostProcessor` (`daemon/postprocess/PrePostProcessor.cpp`).
//!
//! Consolidates post-queue sanitisation, queue/file event routing, NZB completion & deletion
//! workflows, PAR/stream-repair coverage decisions, job scheduling strategies, stage
//! transitions, pause-state management, and direct-unpack triggers into a single module.

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RsPostJobCandidate {
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
    stage: i32,
    pt_executing_script: i32,
    pt_finished: i32,
    pt_queued: i32,
    dest_dir_exists: bool,
    unpack_tried: bool,
    par_status: i32,
    ps_skipped: i32,
) -> (i32, bool) {
    if stage == pt_executing_script || !dest_dir_exists {
        (pt_finished, false)
    } else {
        let request_par = unpack_tried && par_status <= ps_skipped;
        (pt_queued, request_par)
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
    delete_status: i32,
    ds_none: i32,
    ds_health: i32,
    has_post_info: bool,
    download_completed_ignore_paused: bool,
    download_completed_all: bool,
) -> i32 {
    if (is_file_completed || dupe_deleted)
        && delete_status != ds_health
        && !has_post_info
        && download_completed_ignore_paused
    {
        1
    } else if (is_file_deleted || (is_file_completed && delete_status > ds_none))
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
    direct_rename_status: i32,
    ts_success: i32,
    has_post_info: bool,
    delete_status: i32,
    ds_none: i32,
    completed_not_empty: bool,
    download_completed: bool,
    raw_article: bool,
    skip_write: bool,
    skip_disk_write: bool,
) -> bool {
    !is_stopped
        && direct_rename_status == ts_success
        && !has_post_info
        && delete_status == ds_none
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
    delete_status: i32,
    ds_health: i32,
    ds_bad: i32,
    has_post_info: bool,
    opt_raw_article: bool,
    par_status: i32,
    ps_none: i32,
    par_check_opt: i32,
    pc_always: i32,
    pc_force: i32,
) -> (i32, bool, bool) {
    if skip_script_processing && (skip_disk_write || opt_skip_write) {
        return (1, false, false);
    }
    let enqueue_deleted = delete_status == ds_health || delete_status == ds_bad;
    if !has_post_info && !opt_raw_article && !opt_skip_write && !skip_disk_write {
        let set_par_skipped =
            par_status == ps_none && par_check_opt != pc_always && par_check_opt != pc_force;
        (2, enqueue_deleted, set_par_skipped)
    } else {
        (3, enqueue_deleted, false)
    }
}

/// Resolve delete status and route in `PrePostProcessor::NzbDeleted`:
/// Returns `(new_delete_status, call_nzb_downloaded)`.
pub fn resolve_nzb_deleted_route(
    delete_status: i32,
    ds_none: i32,
    ds_manual: i32,
    ds_health: i32,
    ds_bad: i32,
) -> (i32, bool) {
    let new_status = if delete_status == ds_none {
        ds_manual
    } else {
        delete_status
    };
    let call_downloaded = new_status == ds_health || new_status == ds_bad;
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
    dupe_mode: i32,
    dm_force: i32,
    fails_over: bool,
    delete_status: i32,
    ds_none: i32,
    ds_health: i32,
    ds_bad: i32,
    ds_scan: i32,
    ds_copy: i32,
) -> (bool, bool, bool, bool) {
    let download_dupe = dupe_hint == dh_redownload_auto;
    let add_to_history = keep_history > 0 && !avoid_history;
    let call_dupe_completed = dupe_check
        && (dupe_mode != dm_force || fails_over)
        && (delete_status == ds_none
            || delete_status == ds_health
            || delete_status == ds_bad
            || delete_status == ds_scan
            || (delete_status == ds_copy && download_dupe));
    let enqueue_deleted_script =
        delete_status > ds_none && delete_status != ds_health && delete_status != ds_bad;
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
    delete_status: i32,
    ds_dupe: i32,
    parking: bool,
    skip_disk_write: bool,
    opt_skip_write: bool,
) -> i32 {
    if cleanup_disk || (delete_status == ds_dupe && !parking) {
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
    par_status: i32,
    ps_skipped: i32,
    force_repair: bool,
    par_full: bool,
    par_check_opt: i32,
    pc_manual: i32,
    cur_force_par_full: bool,
    file_list_empty: bool,
) -> (i32, bool) {
    if request_par_check
        && (par_status <= ps_skipped || (force_repair && !par_full))
        && par_check_opt != pc_manual
    {
        let new_force_par_full = cur_force_par_full || par_status > ps_skipped;
        (1, new_force_par_full)
    } else if request_par_check && par_status <= ps_skipped && par_check_opt == pc_manual {
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
    stage: i32,
    pt_finished: i32,
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
    let is_completed = stage == pt_finished || deleted;
    (true, update_total_sec, is_completed, new_total_sec)
}

/// `PrePostProcessor::CanRunMoreJobs` scheduler evaluator.
/// `post_strategy`: `0` = `ppSequential`, `1` = `ppBalanced`, `2` = `ppAggressive`, `3` = `ppRocket`.
/// Returns `(can_run, allow_par)`.
pub fn can_run_more_jobs(
    job_stages: &[i32],
    pt_loading_pars: i32,
    pt_verifying_repaired: i32,
    pt_repairing: i32,
    post_strategy: i32,
) -> (bool, bool) {
    let total_jobs = job_stages.len() as i32;
    let mut par_jobs = 0;
    let mut other_jobs = 0;
    let mut repair_jobs = false;

    for &stage in job_stages {
        let par_job = stage >= pt_loading_pars && stage <= pt_verifying_repaired;
        repair_jobs |= stage == pt_repairing;
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
        {
            best_idx = idx as i32;
            best_priority = c.priority;
        }
    }

    best_idx
}

/// Classify skipped par-check health action in `PrePostProcessor::StartJob`:
/// - `0`: no action
/// - `1`: skip par-check due to health 0%
/// - `2`: skip par-check due to health below critical
/// - `3`: request par-check due to failed non-par bytes
#[allow(clippy::too_many_arguments)]
pub fn classify_skipped_par_health(
    par_status: i32,
    ps_skipped: i32,
    par_scan_opt: i32,
    ps_dupe: i32,
    health: i32,
    critical_health: i32,
    failed_size: i64,
    par_failed_size: i64,
) -> i32 {
    if par_status != ps_skipped {
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
    unpack_status: i32,
    us_none: i32,
    delete_status: i32,
    ds_none: i32,
    rar_rename_status: i32,
    rs_none: i32,
    opt_rar_rename: bool,
) -> (bool, bool, bool) {
    let want_unpack = !(has_unpack_param && unpack_param_val.eq_ignore_ascii_case(b"no"));
    let unpack = want_unpack && unpack_status == us_none && delete_status == ds_none;
    let start_rar_rename = rar_rename_status == rs_none && unpack && opt_rar_rename;
    (want_unpack, unpack, start_rar_rename)
}

/// `PrePostProcessor::StartJob` stage eligibility calculator.
#[allow(clippy::too_many_arguments)]
pub fn eval_post_stages(
    want_unpack: bool,
    unpack_status: i32,
    delete_status: i32,
    par_status: i32,
    cleanup_status: i32,
    has_ext_cleanup_disk: bool,
    health: i32,
    move_status: i32,
    dest_dir: &[u8],
    inter_dir: &[u8],
    sep: u8,
    direct_unpack_status: i32,
    opt_post_download_rename: bool,
    post_download_renaming_status: i32,
    has_dest_dir: bool,
    has_name: bool,
    opt_rename_after_unpack: bool,
    post_unpack_renaming_status: i32,
) -> RsPostStageDecision {
    let in_inter_dir = !inter_dir.is_empty()
        && dest_dir.len() > inter_dir.len()
        && dest_dir.starts_with(inter_dir)
        && dest_dir[inter_dir.len()] == sep;
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
    unpack_status: i32,
    delete_status: i32,
    par_status: i32,
    cleanup_status: i32,
    has_ext_cleanup_disk: bool,
    health: i32,
    move_status: i32,
    in_inter_dir: bool,
    direct_unpack_status: i32,
    opt_post_download_rename: bool,
    post_download_renaming_status: i32,
    has_dest_dir: bool,
    has_name: bool,
    opt_rename_after_unpack: bool,
    post_unpack_renaming_status: i32,
) -> RsPostStageDecision {
    const DS_NONE: i32 = 0;
    const PS_NONE: i32 = 0;
    const PS_SKIPPED: i32 = 1;
    const PS_FAILURE: i32 = 2;
    const PS_SUCCESS: i32 = 3;
    const PS_REPAIR_POSSIBLE: i32 = 4;
    const PS_MANUAL: i32 = 5;
    const US_NONE: i32 = 0;
    const US_SKIPPED: i32 = 1;
    const US_FAILURE: i32 = 2;
    const US_SUCCESS: i32 = 3;
    const US_SPACE: i32 = 4;
    const US_PASSWORD: i32 = 5;
    const CS_NONE: i32 = 0;
    const MS_NONE: i32 = 0;
    const MS_SUCCESS: i32 = 2;
    const NS_SUCCESS: i32 = 3;

    let mut unpack = want_unpack && unpack_status == US_NONE && delete_status == DS_NONE;
    let par_failed =
        par_status == PS_FAILURE || par_status == PS_REPAIR_POSSIBLE || par_status == PS_MANUAL;

    let cleanup = !unpack
        && want_unpack
        && cleanup_status == CS_NONE
        && has_ext_cleanup_disk
        && ((par_status == PS_SUCCESS
            && unpack_status != US_FAILURE
            && unpack_status != US_SPACE
            && unpack_status != US_PASSWORD)
            || (unpack_status == US_SUCCESS && par_status != PS_FAILURE)
            || ((unpack_status == US_NONE || unpack_status == US_SKIPPED)
                && (par_status == PS_NONE || par_status == PS_SKIPPED)
                && health == 1000));

    let move_inter = !unpack
        && move_status == MS_NONE
        && unpack_status != US_FAILURE
        && unpack_status != US_SPACE
        && unpack_status != US_PASSWORD
        && par_status != PS_FAILURE
        && par_status != PS_MANUAL
        && delete_status == DS_NONE
        && !((unpack_status == US_NONE || unpack_status == US_SKIPPED)
            && (par_status == PS_NONE || par_status == PS_SKIPPED)
            && health < 1000)
        && in_inter_dir;

    let mut skip_unpack_due_to_par = false;
    let mut effective_unpack_status = unpack_status;
    if unpack && par_failed && direct_unpack_status != NS_SUCCESS {
        skip_unpack_due_to_par = true;
        effective_unpack_status = US_SKIPPED;
        unpack = false;
    }

    let post_download_renaming = opt_post_download_rename
        && post_download_renaming_status == 0
        && has_dest_dir
        && has_name
        && (effective_unpack_status == US_NONE || effective_unpack_status == US_SKIPPED)
        && effective_unpack_status != US_FAILURE
        && effective_unpack_status != US_SPACE
        && effective_unpack_status != US_PASSWORD
        && par_status != PS_FAILURE
        && par_status != PS_MANUAL
        && !move_inter
        && (move_status == MS_SUCCESS || move_status == MS_NONE);

    let post_unpack_renaming = opt_rename_after_unpack
        && post_unpack_renaming_status == 0
        && has_dest_dir
        && has_name
        && effective_unpack_status == US_SUCCESS
        && par_status != PS_FAILURE
        && par_status != PS_MANUAL
        && !move_inter
        && (move_status == MS_SUCCESS || move_status == MS_NONE);

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
    job_stages: &[i32],
    par_pause_queue: bool,
    unpack_pause_queue: bool,
    script_pause_queue: bool,
) -> bool {
    let mut need_pause = false;
    for &stage in job_stages {
        match stage {
            1..=5 | 14 => need_pause |= par_pause_queue,
            6..=10 | 13 => need_pause |= unpack_pause_queue,
            11 => need_pause |= script_pause_queue,
            _ => {}
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
    direct_unpack_status: i32,
    ns_none: i32,
    ns_running: i32,
    direct_rename_status: i32,
    ts_running: i32,
    has_unpack_thread: bool,
) -> i32 {
    if !opt_direct_unpack || opt_raw_article || opt_skip_write || skip_disk_write {
        return 0;
    }
    if direct_unpack_status == ns_none && direct_rename_status != ts_running {
        1
    } else if direct_unpack_status == ns_running && has_unpack_thread {
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

#[cfg(unix)]
fn has_completed_archive_files(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: usize,
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
    nzb_ptr: usize,
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
    use crate::queuesubsys_host::{c_bytes, DS_DUPE, MK_DETAIL, MK_ERROR};
    use std::ffi::{CStr, CString};

    let mode = classify_delete_cleanup_mode(
        nzb.cleanup_disk,
        nzb.delete_status,
        DS_DUPE,
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
    active_jobs: std::sync::Mutex<Vec<usize>>,
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

    #[allow(clippy::too_many_arguments)]
    pub unsafe fn exec(
        &self,
        op: std::ffi::c_int,
        a1: std::ffi::c_int,
        a2: std::ffi::c_int,
        p1: usize,
        p2: usize,
        p3: usize,
        s1: *const std::ffi::c_char,
        host_ctx: *mut std::ffi::c_void,
        host_cb: crate::queuesubsys_host::NzbgetRsQueueSubsysHostFn,
    ) -> usize {
        use std::sync::atomic::Ordering;
        let _ = (a2, s1);
        let host = crate::queuesubsys_host::QueueSubsysHost::new(host_ctx, host_cb);
        match op {
            0 => {
                self.run(&host);
                0
            }
            1 => {
                self.stop(&host);
                0
            }
            2 => usize::from(self.queued_jobs.load(Ordering::Acquire) > 0),
            3 => {
                let inc = a1 != 0;
                if p1 == 0 {
                    let mut res = false;
                    host.with_locked_queue(|dq_ptr| {
                        res = self.has_active_unpack(&host, dq_ptr, inc);
                    });
                    usize::from(res)
                } else {
                    usize::from(self.has_active_unpack(&host, p1, inc))
                }
            }
            6 => {
                self.nzb_downloaded(&host, p1, p2);
                0
            }
            8 => {
                self.download_queue_update(&host, a1, p1, p2, p3);
                0
            }
            _ => 0,
        }
    }

    pub(crate) fn owner_ptr(&self) -> usize {
        self.owner
    }

    fn run(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
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

    fn stop(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        host.with_locked_queue(|dq_ptr| {
            let jobs = self.active_jobs.lock().unwrap().clone();
            for job_ptr in jobs {
                let nzb = host.nzb_snap(job_ptr, false);
                if nzb.post_info_ptr != 0 {
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
                if nzb.post_info_ptr != 0 {
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
                host.nzb_action(nzb_ptr, NZB_ACT_DISCONNECT_THREADS, 0);
            }
        });

        host.log(MK_DEBUG, b"PrePostProcessor: Jobs are completed");
    }

    fn sanitise_post_queue(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        use crate::queuesubsys_host::{
            DS_NONE, MK_INFO, NK_NZB, NS_NONE, NS_RUNNING, NZB_INT_DIRECT_UNPACK_STATUS,
            PAR_STATUS_SKIPPED, POST_INT_REQUEST_PAR_CHECK, POST_INT_STAGE, POST_INT_WORKING,
            PT_EXECUTING_SCRIPT, PT_FINISHED, PT_QUEUED,
        };
        use std::sync::atomic::Ordering;

        host.with_locked_queue(|dq_ptr| {
            let qlen = host.queue_len(dq_ptr);
            let entries: Vec<(i32, usize)> = (0..qlen)
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
                if nzb.direct_unpack_status == NS_RUNNING && !nzb.has_unpack_thread {
                    host.nzb_set_int(nzb_ptr, NZB_INT_DIRECT_UNPACK_STATUS, NS_NONE);
                }
                if nzb.post_info_ptr == 0
                    && nzb.kind == NK_NZB
                    && !host.nzb_completed_files(nzb_ptr).is_empty()
                    && nzb.delete_status == DS_NONE
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
                    if nzb.post_info_ptr != 0 {
                        self.queued_jobs.fetch_sub(1, Ordering::AcqRel);
                    }
                }
                if nzb.post_info_ptr != 0 {
                    self.queued_jobs.fetch_add(1, Ordering::AcqRel);
                    let ps = host.post_snap(nzb.post_info_ptr);
                    let dest_exists =
                        crate::fsops::directory_exists(&nzb.dest_dir, &crate::fsops::posix_stat);
                    let (new_stage, req_par) = sanitise_post_stage(
                        ps.stage,
                        PT_EXECUTING_SCRIPT,
                        PT_FINISHED,
                        PT_QUEUED,
                        dest_exists,
                        nzb.unpack_tried,
                        nzb.par_status,
                        PAR_STATUS_SKIPPED,
                    );
                    host.post_set_int(nzb.post_info_ptr, POST_INT_STAGE, new_stage);
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
        use crate::queuesubsys_host::PT_UNPACKING;

        let jobs = self.active_jobs.lock().unwrap().clone();
        for job_ptr in jobs {
            let nzb = host.nzb_snap(job_ptr, false);
            if nzb.has_post_info && nzb.post_stage == PT_UNPACKING {
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
        nzb_ptr: usize,
    ) {
        use crate::queuesubsys_host::{
            DS_COPY, DS_DUPE, DS_GOOD, DS_NONE, DS_SCAN, EA_GROUP_PAUSE_EXTRA_PARS,
            EA_GROUP_SORT_FILES, MK_INFO, PC_FORCE,
        };

        let opts = host.opts();
        let nzb = host.nzb_snap(nzb_ptr, false);

        if opts.par_check != PC_FORCE {
            host.dq_edit_entry(dq_ptr, nzb.id, EA_GROUP_PAUSE_EXTRA_PARS, None);
        }

        if opts.reorder_files && nzb.delete_status == DS_NONE {
            let mut msg = b"Reordering files for ".to_vec();
            msg.extend_from_slice(&nzb.name);
            host.nzb_print_message(nzb_ptr, MK_INFO, &msg);
            host.dq_edit_entry(dq_ptr, nzb.id, EA_GROUP_SORT_FILES, None);
        }
        #[cfg(unix)]
        if nzb.delete_status == DS_NONE {
            crate::dupefallback::prioritize_par_index_host(host, nzb_ptr);
        }

        let nzb = host.nzb_snap(nzb_ptr, false);
        if matches!(nzb.delete_status, DS_DUPE | DS_COPY | DS_GOOD | DS_SCAN) {
            self.nzb_completed(host, dq_ptr, nzb_ptr, false);
        }
    }

    pub(crate) fn check_download_completed(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb_ptr: usize,
    ) {
        use crate::queuesubsys_host::{DS_NONE, MK_INFO, TS_SUCCESS};

        let opts = host.opts();
        let nzb = host.nzb_snap(nzb_ptr, false);
        let completed_not_empty = !host.nzb_completed_files(nzb_ptr).is_empty();
        if should_complete_after_direct_rename(
            host.prepost_is_stopped(self.owner),
            nzb.direct_rename_status,
            TS_SUCCESS,
            nzb.has_post_info,
            nzb.delete_status,
            DS_NONE,
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
        nzb_ptr: usize,
    ) {
        use crate::queuesubsys_host::{
            DS_HEALTH, MK_INFO, NZB_ACT_ENTER_POST_PROCESS, NZB_INT_CHANGED, NZB_INT_CLEANUP_DISK,
            NZB_INT_PAR_STATUS, PAR_STATUS_NONE, PAR_STATUS_SKIPPED, PC_FORCE, POST_INT_WORKING,
        };
        use std::sync::atomic::Ordering;

        if nzb_ptr == 0 {
            return;
        }

        let opts = host.opts();
        let nzb = host.nzb_snap(nzb_ptr, false);
        const DS_BAD: i32 = 4;
        const PC_ALWAYS: i32 = 1;

        let (route, _, set_par_skipped) = classify_nzb_downloaded_entry(
            nzb.skip_script_processing,
            nzb.skip_disk_write,
            opts.skip_write,
            nzb.delete_status,
            DS_HEALTH,
            DS_BAD,
            nzb.has_post_info,
            opts.raw_article,
            nzb.par_status,
            PAR_STATUS_NONE,
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
            delete_cleanup_host(host, &opts, &nzb2);
            self.nzb_completed(host, dq_ptr, nzb_ptr, true);
            return;
        }

        if route == 2 {
            let mut msg = b"Queueing ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b" for post-processing");
            host.nzb_print_message(nzb_ptr, MK_INFO, &msg);
            host.nzb_action(nzb_ptr, NZB_ACT_ENTER_POST_PROCESS, 0);
            if set_par_skipped {
                host.nzb_set_int(nzb_ptr, NZB_INT_PAR_STATUS, PAR_STATUS_SKIPPED);
            }

            let nzb2 = host.nzb_snap(nzb_ptr, false);
            if nzb2.has_unpack_thread || nzb2.has_live_repair_thread {
                if nzb2.post_info_ptr != 0 {
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

    fn download_queue_update(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        action: i32,
        dq_ptr: usize,
        nzb_ptr: usize,
        file_ptr: usize,
    ) {
        use crate::queuesubsys_host::{
            ASPECT_FILE_COMPLETED, ASPECT_FILE_DELETED, ASPECT_NZB_ADDED, ASPECT_NZB_DELETED,
            ASPECT_NZB_FOUND, ASPECT_NZB_NAMED, ASPECT_URL_ADDED, ASPECT_URL_DELETED,
            ASPECT_URL_FAILED, ASPECT_URL_FOUND, DM_FORCE, DS_HEALTH, DS_NONE, MK_INFO,
            PT_LOADING_PARS, PT_VERIFYING_REPAIRED,
        };

        if host.prepost_is_stopped(self.owner) {
            return;
        }

        if action == ASPECT_NZB_FOUND || action == ASPECT_URL_FOUND {
            let opts = host.opts();
            let nzb = host.nzb_snap(nzb_ptr, false);
            if opts.dupe_check && nzb.dupe_mode != DM_FORCE {
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
        } else if action == ASPECT_FILE_COMPLETED || action == ASPECT_FILE_DELETED {
            if action == ASPECT_FILE_COMPLETED {
                self.file_downloaded(host, dq_ptr, nzb_ptr, file_ptr);
            }

            let opts = host.opts();
            let file = host.file_snap(file_ptr);
            if opts.parcheck_enabled {
                let jobs = self.active_jobs.lock().unwrap().clone();
                for job_ptr in jobs {
                    if job_ptr != 0 && file.nzb_ptr == job_ptr {
                        let job_nzb = host.nzb_snap(job_ptr, false);
                        if job_nzb.post_info_ptr != 0 {
                            let ps = host.post_snap(job_nzb.post_info_ptr);
                            if ps.has_post_thread
                                && ps.stage >= PT_LOADING_PARS
                                && ps.stage <= PT_VERIFYING_REPAIRED
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
                DS_NONE,
                DS_HEALTH,
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
        nzb_ptr: usize,
    ) {
        use crate::queuesubsys_host::{
            DS_HEALTH, DS_MANUAL, DS_NONE, NZB_INT_DELETE_STATUS, NZB_INT_DELETING,
        };
        const DS_BAD: i32 = 4;

        let nzb = host.nzb_snap(nzb_ptr, false);
        if nzb.has_unpack_thread {
            host.direct_unpack_nzb_deleted(dq_ptr, nzb_ptr);
        }
        if nzb.has_live_repair_thread {
            host.live_repair_detach_and_stop(nzb_ptr);
        }

        let nzb = host.nzb_snap(nzb_ptr, false);
        let (new_status, call_downloaded) =
            resolve_nzb_deleted_route(nzb.delete_status, DS_NONE, DS_MANUAL, DS_HEALTH, DS_BAD);
        host.nzb_set_int(nzb_ptr, NZB_INT_DELETE_STATUS, new_status);
        host.nzb_set_int(nzb_ptr, NZB_INT_DELETING, 0);

        let opts = host.opts();
        let nzb2 = host.nzb_snap(nzb_ptr, false);
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
        nzb_ptr: usize,
        save_queue: bool,
    ) {
        use crate::queuesubsys_host::{
            DH_REDOWNLOAD_AUTO, DM_FORCE, DS_COPY, DS_HEALTH, DS_NONE, DS_SCAN,
            NZB_INT_AVOID_HISTORY,
        };
        const DS_BAD: i32 = 4;

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
            DM_FORCE,
            fails_over,
            nzb.delete_status,
            DS_NONE,
            DS_HEALTH,
            DS_BAD,
            DS_SCAN,
            DS_COPY,
        );

        if add_to_history {
            host.history_add_to_history(dq_ptr, nzb_ptr);
        }
        host.nzb_set_int(nzb_ptr, NZB_INT_AVOID_HISTORY, 0);
        if call_dupe {
            crate::dupecoord::nzb_completed_host(host, dq_ptr, nzb_ptr);
        }
        crate::dupecoord::rescue_on_health_failure_host(host, dq_ptr, nzb_ptr);
        if !add_to_history {
            host.history_delete_disk_files(nzb_ptr);
            crate::dupefallback::par_damage_forget_all(nzb.id);
            host.queue_remove(dq_ptr, nzb_ptr);
        }
        if save_queue && need_save {
            host.dq_save(dq_ptr);
        }
    }

    fn check_request_par(&self, host: &crate::queuesubsys_host::QueueSubsysHost, dq_ptr: usize) {
        use crate::queuesubsys_host::{
            EA_GROUP_RESUME, MK_INFO, NZB_ACT_CLEAR_SCRIPT_STATUSES, NZB_INT_PAR_STATUS,
            PAR_STATUS_MANUAL, PAR_STATUS_NONE, PAR_STATUS_SKIPPED, PC_MANUAL,
            POST_INT_FORCE_PAR_FULL, POST_INT_REQUEST_PAR_CHECK, POST_INT_STAGE, POST_INT_WORKING,
            PT_FINISHED, PT_QUEUED,
        };

        let opts = host.opts();
        if !opts.parcheck_enabled {
            return;
        }

        let jobs = self.active_jobs.lock().unwrap().clone();
        for job_ptr in jobs {
            let nzb = host.nzb_snap(job_ptr, false);
            if nzb.post_info_ptr == 0 {
                continue;
            }
            let ps = host.post_snap(nzb.post_info_ptr);
            let (act, new_force_full) = classify_request_par_action(
                ps.request_par_check,
                nzb.par_status,
                PAR_STATUS_SKIPPED,
                ps.force_repair,
                nzb.par_full,
                opts.par_check,
                PC_MANUAL,
                ps.force_par_full,
                nzb.file_count == 0,
            );
            if act == 1 {
                host.post_set_int(ps.ptr, POST_INT_FORCE_PAR_FULL, i32::from(new_force_full));
                host.nzb_set_int(nzb.ptr, NZB_INT_PAR_STATUS, PAR_STATUS_NONE);
                host.post_set_int(ps.ptr, POST_INT_REQUEST_PAR_CHECK, 0);
                host.nzb_action(nzb.ptr, NZB_ACT_CLEAR_SCRIPT_STATUSES, 0);
                host.post_set_int(ps.ptr, POST_INT_STAGE, PT_QUEUED);
                host.post_set_int(ps.ptr, POST_INT_WORKING, 0);
            } else if act == 2 || act == 3 {
                host.post_set_int(ps.ptr, POST_INT_REQUEST_PAR_CHECK, 0);
                host.nzb_set_int(nzb.ptr, NZB_INT_PAR_STATUS, PAR_STATUS_MANUAL);
                if act == 2 {
                    let mut msg =
                        b"Downloading all remaining files for manual par-check for ".to_vec();
                    msg.extend_from_slice(&nzb.name);
                    host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
                    host.dq_edit_entry(dq_ptr, nzb.id, EA_GROUP_RESUME, None);
                    host.post_set_int(ps.ptr, POST_INT_STAGE, PT_FINISHED);
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
        use crate::queuesubsys_host::{
            NZB_INT_POST_TOTAL_SEC, POST_INT_STAGE, PT_FINISHED, PT_QUEUED,
        };

        let jobs = self.active_jobs.lock().unwrap().clone();
        let cur_time = host.opts().cur_time;
        for job_ptr in jobs {
            let nzb = host.nzb_snap(job_ptr, false);
            if nzb.post_info_ptr == 0 {
                self.active_jobs.lock().unwrap().retain(|&p| p != job_ptr);
                continue;
            }
            let ps = host.post_snap(nzb.post_info_ptr);
            let (ready, upd_sec, is_comp, new_sec) = eval_cleanup_job_state(
                ps.working,
                ps.has_post_thread && ps.post_thread_running,
                ps.stage,
                PT_FINISHED,
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
                    host.post_set_int(ps.ptr, POST_INT_STAGE, PT_QUEUED);
                }
            }
        }
    }

    fn active_job_stages(&self, host: &crate::queuesubsys_host::QueueSubsysHost) -> Vec<i32> {
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

    fn can_run_more_jobs(&self, host: &crate::queuesubsys_host::QueueSubsysHost) -> (bool, bool) {
        use crate::queuesubsys_host::{PT_LOADING_PARS, PT_VERIFYING_REPAIRED};
        const PT_REPAIRING: i32 = 3;
        let stages = self.active_job_stages(host);
        let opts = host.opts();
        can_run_more_jobs(
            &stages,
            PT_LOADING_PARS,
            PT_VERIFYING_REPAIRED,
            PT_REPAIRING,
            opts.post_strategy,
        )
    }

    fn pick_next_job(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        allow_par: bool,
    ) -> usize {
        use crate::queuesubsys_host::NS_RUNNING;

        let active = self.active_jobs.lock().unwrap().clone();
        let qlen = host.queue_len(dq_ptr);
        let mut ptrs = Vec::with_capacity(qlen);
        let mut cands = Vec::with_capacity(qlen);
        for qi in 0..qlen {
            let nzb_ptr = host.queue_at(dq_ptr, qi);
            let nzb = host.nzb_snap(nzb_ptr, false);
            let (working, need_par, need_unpack) = if nzb.post_info_ptr != 0 {
                let ps = host.post_snap(nzb.post_info_ptr);
                (ps.working, ps.need_par_check, ps.need_unpack)
            } else {
                (false, false, false)
            };
            ptrs.push(nzb_ptr);
            cands.push(RsPostJobCandidate {
                has_post_info: u8::from(nzb.post_info_ptr != 0),
                working: u8::from(working),
                has_queue_script_job: 0,
                direct_unpack_running: u8::from(
                    nzb.direct_unpack_status == NS_RUNNING
                        && !crate::postproc::direct_unpack_awaiting_repair(nzb.id),
                ),
                has_live_repair_thread: u8::from(nzb.has_live_repair_thread),
                priority: nzb.priority,
                force_priority: u8::from(nzb.force_priority),
                need_par_check: u8::from(need_par),
                need_unpack: u8::from(need_unpack),
                in_active_jobs: u8::from(active.contains(&nzb_ptr)),
                download_completed: u8::from(nzb.is_download_completed),
            });
        }

        let has_active_unpack = self.has_active_unpack(host, dq_ptr, true);
        let pause_post = host.opts().pause_post_process;
        let idx = pick_next_post_job(&cands, allow_par, has_active_unpack, pause_post);
        if idx >= 0 {
            ptrs[idx as usize]
        } else {
            0
        }
    }

    fn check_post_queue(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        use crate::queuesubsys_host::PT_QUEUED;

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
                if post_job == 0 {
                    break;
                }

                self.active_jobs.lock().unwrap().push(post_job);

                let nzb = host.nzb_snap(post_job, false);
                if nzb.post_info_ptr != 0 {
                    let ps = host.post_snap(nzb.post_info_ptr);
                    let pause_post = host.opts().pause_post_process;
                    if ps.stage == PT_QUEUED && (!pause_post || nzb.force_priority) {
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
        post_ptr: usize,
        stage: i32,
    ) {
        use crate::queuesubsys_host::{POST_INT_STAGE, POST_INT_WORKING};
        host.post_set_int(post_ptr, POST_INT_WORKING, 1);
        host.post_set_int(post_ptr, POST_INT_STAGE, stage);
    }

    fn start_job(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        post_ptr: usize,
        allow_par: bool,
    ) {
        use crate::queuesubsys_host::{
            CTRL_CLEANUP, CTRL_MOVE, CTRL_POST_RENAME_DL, CTRL_POST_RENAME_UNPACK,
            CTRL_POST_SCRIPT, CTRL_RENAME_PAR, CTRL_RENAME_RAR, CTRL_REPAIR, CTRL_STREAM_REPAIR,
            CTRL_UNPACK, DAF_STREAM, DS_NONE, FILE_INT_EXTRA_PRIORITY, FILE_INT_PAUSED, HC_DUPE,
            MK_INFO, MK_WARNING, NZB_INT_CHANGED, NZB_INT_PAR_STATUS, NZB_INT_UNPACK_STATUS,
            PAR_STATUS_FAILURE, PAR_STATUS_MANUAL, PAR_STATUS_NONE, PAR_STATUS_REPAIR_POSSIBLE,
            PAR_STATUS_SKIPPED, PC_AUTO, POST_INT_NEED_PAR_CHECK, POST_INT_NEED_UNPACK,
            POST_INT_PAR_CANNOT_COVER, POST_INT_REQUEST_PAR_CHECK, PS_DUPE, PT_CLEANING_UP,
            PT_EXECUTING_SCRIPT, PT_LOADING_PARS, PT_MOVING, PT_PAR_RENAMING,
            PT_POST_DOWNLOAD_RENAMING, PT_POST_UNPACK_RENAMING, PT_RAR_RENAMING,
            PT_STREAM_REPAIRING, PT_UNPACKING, RS_NONE, US_SKIPPED,
        };

        let opts = host.opts();
        let ps = host.post_snap(post_ptr);
        let nzb = host.nzb_snap(ps.nzb_ptr, false);

        if !nzb.has_unpack_thread {
            // No direct unpack is paused on this download: its damaged volumes are repaired
            // and unpacked by the stages below, nothing may stay on hold.
            crate::postproc::direct_unpack_forget_holds(nzb.id);
        }

        if ps.start_time == 0 {
            host.post_set_start_time(post_ptr, opts.cur_time);
        }
        host.post_reset_stage_progress(post_ptr, opts.cur_time);

        if nzb.par_rename_status == RS_NONE && nzb.delete_status == DS_NONE && opts.par_rename {
            self.enter_stage(host, post_ptr, PT_PAR_RENAMING);
            host.post_controller_start_job(post_ptr, CTRL_RENAME_PAR);
            return;
        }

        let mut stream_repair = opts.dupe_article_fallback >= DAF_STREAM
            && nzb.has_stream_repair_jobs
            && nzb.delete_status == DS_NONE;

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
            .is_some()
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
                && nzb.par_status == PAR_STATUS_SKIPPED
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
                self.enter_stage(host, post_ptr, PT_STREAM_REPAIRING);
                host.post_controller_start_job(post_ptr, CTRL_STREAM_REPAIR);
                return;
            }

            if ps.par_cannot_cover
                && ps.stream_repair_done
                && !ps.stream_repair_recovered
                && nzb.dupe_recovered_bytes <= 0
                && opts.par_check == PC_AUTO
                && nzb.par_status <= PAR_STATUS_NONE
                && nzb.delete_status == DS_NONE
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
                host.nzb_set_int(nzb.ptr, NZB_INT_PAR_STATUS, PAR_STATUS_FAILURE);
                return;
            }

            if stream_repair && nzb.par_status == PAR_STATUS_SKIPPED && has_main_pars(&nzb.dest_dir)
            {
                host.post_set_int(post_ptr, POST_INT_REQUEST_PAR_CHECK, 1);
                return;
            }

            if (nzb.par_status == PAR_STATUS_NONE || nzb.par_status == PAR_STATUS_SKIPPED)
                && nzb.delete_status == DS_NONE
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

            if nzb.par_status == PAR_STATUS_NONE && nzb.delete_status == DS_NONE {
                if has_main_pars(&nzb.dest_dir) {
                    if !allow_par {
                        host.post_set_int(post_ptr, POST_INT_NEED_PAR_CHECK, 1);
                        return;
                    }
                    self.enter_stage(host, post_ptr, PT_LOADING_PARS);
                    host.post_set_int(post_ptr, POST_INT_NEED_PAR_CHECK, 0);
                    host.post_controller_start_job(post_ptr, CTRL_REPAIR);
                } else {
                    let mut msg = b"Nothing to par-check for ".to_vec();
                    msg.extend_from_slice(&nzb.name);
                    host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
                    host.nzb_set_int(nzb.ptr, NZB_INT_PAR_STATUS, PAR_STATUS_SKIPPED);
                }
                return;
            }

            if nzb.par_status == PAR_STATUS_SKIPPED {
                let h_act = classify_skipped_par_health(
                    nzb.par_status,
                    PAR_STATUS_SKIPPED,
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
                        host.nzb_set_int(nzb.ptr, NZB_INT_PAR_STATUS, PAR_STATUS_FAILURE);
                    } else {
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
            && nzb.par_status != PAR_STATUS_REPAIR_POSSIBLE
            && nzb.par_status != PAR_STATUS_MANUAL
        {
            self.enter_stage(host, post_ptr, PT_STREAM_REPAIRING);
            host.post_controller_start_job(post_ptr, CTRL_STREAM_REPAIR);
            return;
        }

        if crate::postproc::direct_unpack_awaiting_repair(nzb.id) {
            // Direct unpack paused at a damaged volume and the repair stages are over: let it
            // continue from where it stopped (or give up when the repair failed), then come back.
            const PAR_STATUS_SUCCESS: i32 = 3;
            let repaired = nzb.par_status == PAR_STATUS_SUCCESS
                || (nzb.health >= 1000
                    && nzb.par_status != PAR_STATUS_FAILURE
                    && nzb.par_status != PAR_STATUS_REPAIR_POSSIBLE
                    && nzb.par_status != PAR_STATUS_MANUAL);
            let mut msg = if repaired {
                b"Resuming direct unpack for ".to_vec()
            } else {
                b"Not resuming direct unpack for ".to_vec()
            };
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(if repaired {
                b" after repair"
            } else {
                b": its damaged volumes were not repaired"
            });
            host.nzb_print_message(nzb.ptr, MK_INFO, &msg);
            host.post_set_int(post_ptr, crate::queuesubsys_host::POST_INT_WORKING, 1);
            if repaired {
                crate::postproc::direct_unpack_release_all(nzb.id);
            } else {
                crate::postproc::direct_unpack_abandon(nzb.id);
            }
            return;
        }

        const US_NONE: i32 = 0;
        let unpack_param = host.nzb_get_param(nzb.ptr, b"*Unpack:");
        let (want_unpack, _unpack, start_rar_rename) = eval_want_unpack_and_rar_rename(
            unpack_param.is_some(),
            unpack_param.as_deref().unwrap_or(b""),
            nzb.unpack_status,
            US_NONE,
            nzb.delete_status,
            DS_NONE,
            nzb.rar_rename_status,
            RS_NONE,
            opts.rar_rename,
        );
        if start_rar_rename {
            self.enter_stage(host, post_ptr, PT_RAR_RENAMING);
            host.post_controller_start_job(post_ptr, CTRL_RENAME_RAR);
            return;
        }

        if opts.parcheck_enabled
            && nzb.par_status == PAR_STATUS_SKIPPED
            && nzb.delete_status == DS_NONE
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
            nzb.direct_unpack_status,
            opts.post_download_rename,
            nzb.post_download_renaming_status,
            !nzb.dest_dir.is_empty(),
            !nzb.name.is_empty(),
            opts.rename_after_unpack,
            nzb.post_unpack_renaming_status,
        );
        if dec.skip_unpack_due_to_par != 0 {
            let reason: &[u8] = if nzb.par_status == PAR_STATUS_MANUAL {
                b"required par-repair"
            } else {
                b"par-failure"
            };
            let mut msg = b"Skipping unpack for ".to_vec();
            msg.extend_from_slice(&nzb.name);
            msg.extend_from_slice(b" due to ");
            msg.extend_from_slice(reason);
            host.nzb_print_message(nzb.ptr, MK_WARNING, &msg);
            host.nzb_set_int(nzb.ptr, NZB_INT_UNPACK_STATUS, US_SKIPPED);
        }

        if dec.unpack != 0 {
            if self.has_active_unpack(host, dq_ptr, true) {
                host.post_set_int(post_ptr, POST_INT_NEED_UNPACK, 1);
                return;
            }
            host.post_set_int(post_ptr, POST_INT_NEED_UNPACK, 0);
            self.enter_stage(host, post_ptr, PT_UNPACKING);
            host.post_controller_start_job(post_ptr, CTRL_UNPACK);
        } else if dec.cleanup != 0 {
            self.enter_stage(host, post_ptr, PT_CLEANING_UP);
            host.post_controller_start_job(post_ptr, CTRL_CLEANUP);
        } else if dec.move_inter != 0 {
            self.enter_stage(host, post_ptr, PT_MOVING);
            host.post_controller_start_job(post_ptr, CTRL_MOVE);
        } else if dec.post_download_renaming != 0 {
            self.enter_stage(host, post_ptr, PT_POST_DOWNLOAD_RENAMING);
            host.post_controller_start_job(post_ptr, CTRL_POST_RENAME_DL);
        } else if dec.post_unpack_renaming != 0 {
            self.enter_stage(host, post_ptr, PT_POST_UNPACK_RENAMING);
            host.post_controller_start_job(post_ptr, CTRL_POST_RENAME_UNPACK);
        } else {
            self.enter_stage(host, post_ptr, PT_EXECUTING_SCRIPT);
            host.post_controller_start_job(post_ptr, CTRL_POST_SCRIPT);
        }
    }

    fn job_completed(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb_ptr: usize,
    ) {
        use crate::queuesubsys_host::NZB_ACT_LEAVE_POST_PROCESS;
        use std::sync::atomic::Ordering;

        host.nzb_action(nzb_ptr, NZB_ACT_LEAVE_POST_PROCESS, 0);
        let nzb = host.nzb_snap(nzb_ptr, false);
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
            opts.par_pause_queue,
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
            if nzb_ptr == 0 {
                continue;
            }
            let nzb = host.nzb_snap(nzb_ptr, false);
            if nzb.post_info_ptr == 0 {
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

    fn file_downloaded(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        dq_ptr: usize,
        nzb_ptr: usize,
        file_ptr: usize,
    ) {
        use crate::queuesubsys_host::{NS_NONE, NS_RUNNING, TS_RUNNING};

        let opts = host.opts();
        let nzb = host.nzb_snap(nzb_ptr, false);
        if opts.direct_unpack {
            // A damaged archive volume is put on hold: direct unpack pauses in front of it
            // until a repair releases it, instead of being cancelled.
            let file = host.file_snap(file_ptr);
            if file.failed_articles > 0 && crate::filetypes::is_rar_file(&file.filename) {
                let full = [
                    nzb.dest_dir.as_slice(),
                    &[crate::paths::PATH_SEPARATOR],
                    file.filename.as_slice(),
                ]
                .concat();
                crate::postproc::direct_unpack_hold(nzb.id, &full);
            }
        }

        let act = classify_file_downloaded_direct_unpack(
            opts.direct_unpack,
            opts.raw_article,
            opts.skip_write,
            nzb.skip_disk_write,
            nzb.direct_unpack_status,
            NS_NONE,
            NS_RUNNING,
            nzb.direct_rename_status,
            TS_RUNNING,
            nzb.has_unpack_thread,
        );
        if act == 1 {
            let file = host.file_snap(file_ptr);
            if crate::filetypes::is_rar_file(&file.filename)
                && self.can_run_more_jobs(host).0
                && !self.has_active_unpack(host, dq_ptr, true)
            {
                let unpack_param = host.nzb_get_param(nzb_ptr, b"*Unpack:");
                let want_unpack = unpack_param
                    .as_ref()
                    .map_or(true, |v| v.eq_ignore_ascii_case(b"yes"));
                if want_unpack {
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
    use super::*;

    #[test]
    fn test_prepost_stage_and_pause() {
        assert_eq!(
            sanitise_post_stage(11, 11, 12, 0, true, false, 0, 1),
            (12, false)
        );
        assert_eq!(
            sanitise_post_stage(2, 11, 12, 0, true, true, 1, 1),
            (0, true)
        );
        assert!(calc_post_pause_state(&[2], true, false, false));
        assert!(!calc_post_pause_state(&[2], false, true, true));
    }

    #[test]
    fn test_eval_post_stages_inter_dir() {
        let dec = eval_post_stages(
            false,
            0,
            0,
            3,
            0,
            false,
            1000,
            0,
            b"/inter/job1",
            b"/inter",
            b'/',
            0,
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
    fn prepost_processor_full_suite_port_from_cpp() {
        // Port of tests/postprocess/PrePostProcessor.cpp:
        // DirectRenameSchedulesCompletedDownload, DirectRenameWaitsForRemainingData,
        // DirectRenameWaitsForActiveDownloads, DirectRenameWaitsForRenamerToFinish,
        // DirectRenameDoesNotCompleteHealthDeletion, DirectRenameSchedulesAfterPausedRequestsDrain
        const TS_RUNNING: i32 = 1;
        const TS_SUCCESS: i32 = 3;
        const DS_NONE: i32 = 0;
        const DS_HEALTH: i32 = 2;

        struct SimCompletionFixture {
            direct_rename_status: i32,
            has_post_info: bool,
            delete_status: i32,
            completed_not_empty: bool,
            par_file_only_remaining: bool,
            active_downloads: i32,
            job_count: usize,
        }

        impl SimCompletionFixture {
            fn new() -> Self {
                Self {
                    direct_rename_status: TS_SUCCESS,
                    has_post_info: false,
                    delete_status: DS_NONE,
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
                    TS_SUCCESS,
                    self.has_post_info,
                    self.delete_status,
                    DS_NONE,
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
                        DS_HEALTH,
                        4,
                        self.has_post_info,
                        false,
                        0,
                        0,
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
        f.direct_rename_status = TS_RUNNING;
        f.check_download_completed();
        assert!(!f.has_post_info);
        assert_eq!(f.job_count, 0);

        // 5. DirectRenameDoesNotCompleteHealthDeletion
        let mut f = SimCompletionFixture::new();
        f.delete_status = DS_HEALTH;
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
