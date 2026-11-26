//! End-to-end Rust orchestration for post-processing controllers:
//! `ParChecker`, `UnpackController`, `DirectUnpack`, `MoveController`,
//! `CleanupController`, `RenameController`, and `PostRenamerController`.

use crate::queuesubsys_host::NzbRef;
use crate::queuesubsys_host::FileRef;
use crate::cstr;
use crate::parchecker::{
    is_processed_file, ParRepairEvents, ParResult, ParSegment, QuickInfo, RustParRepairer,
};
use crate::options::OptStr;
use crate::postrepair::NzbgetRsParSegment;
use crate::queuesubsys_host::QueueSubsysHost;
use crate::status::{
    DeleteStatus, DirectUnpackStatus, NzbParStatus, ParCheckerStatus, PostStage, UnpackStatus,
};
use std::collections::VecDeque;
use std::ffi::c_int;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use crate::log::{MK_INFO, MK_WARNING, MK_ERROR, MK_DEBUG, MK_DETAIL};

pub const PAR_STAGE_LOADING_PARS: i32 = 0;
pub const PAR_STAGE_VERIFYING_SOURCES: i32 = 1;
pub const PAR_STAGE_REPAIRING: i32 = 2;
pub const PAR_STAGE_VERIFYING_REPAIRED: i32 = 3;

pub const PAR_SCAN_EXTENDED: i32 = 1;
pub const PAR_SCAN_FULL: i32 = 2;
pub const PAR_SCAN_DUPE: i32 = 3;

static UNPACK_MUTEX: Mutex<()> = Mutex::new(());

/// An extraction (unrar, 7-Zip, direct unpack) holds the unpack lock right now.
pub(crate) fn extraction_running() -> bool {
    matches!(UNPACK_MUTEX.try_lock(), Err(std::sync::TryLockError::WouldBlock))
}

#[inline]
pub(crate) fn current_time_sec() -> i64 {
    // SAFETY: `libc::time(null)` is async-signal-safe and reads the wall clock.
    unsafe { libc::time(ptr::null_mut()) as i64 }
}

#[inline]
fn current_thread_key() -> usize {
    // SAFETY: `pthread_self` returns the calling POSIX thread identifier.
    unsafe { libc::pthread_self() as usize }
}

#[inline]
fn trunc_1023(mut v: Vec<u8>) -> Vec<u8> {
    if v.len() > 1023 {
        v.truncate(1023);
    }
    v
}

// ============================================================================
// 1. ParChecker (`daemon/postprocess/ParChecker.cpp`)
// ============================================================================

#[derive(Clone, Default)]
pub struct ParDupeSource {
    pub id: i32,
    pub used_blocks: i32,
    pub directory: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct ParCheckerOptions {
    pub par_buffer_mb: i32,
    pub has_memory_monitor: bool,
    pub par_threads: i32,
    pub par_scan: i32,
    pub par_repair: bool,
    pub par_ignore_ext: Vec<u8>,
}

pub trait ParCheckerHooks {
    fn read_options(&self) -> ParCheckerOptions;
    fn is_stopped(&self) -> bool {
        false
    }
    fn request_more_pars(&self, block_needed: i32) -> (bool, i32);
    fn update_progress(&self) {}
    fn completed(&self) {}
    fn print_message(&self, _kind: i32, _msg: &[u8]) {}
    fn global_log(&self, _kind: i32, _msg: &[u8]) {}
    fn register_parred_file(&self, _filename: &[u8]) {}
    fn register_verified_file(&self, _filename: &[u8]) {}
    fn is_parred_file(&self, _filename: &[u8]) -> bool {
        false
    }
    fn find_file_quick_info(&self, _filename: &[u8]) -> Option<QuickInfo> {
        None
    }
    fn find_file_origname(&self, _filename: &[u8]) -> Option<Vec<u8>> {
        None
    }
    fn request_dupe_sources(&self) -> Vec<ParDupeSource> {
        Vec::new()
    }
    fn stat_dupe_sources(&self, _sources: &[ParDupeSource]) {}
    fn request_twin_index(&self) -> Vec<u8> {
        Vec::new()
    }
    fn request_twin_volumes(&self, _block_needed: i32) -> Vec<Vec<u8>> {
        Vec::new()
    }
}

#[derive(Default)]
struct ParCheckerMeta {
    info_name: Vec<u8>,
    dest_dir: Vec<u8>,
    nzb_name: Vec<u8>,
    par_filename: Vec<u8>,
    progress_label: Vec<u8>,
}

#[derive(Default)]
struct ParQueueState {
    queued_par_files: VecDeque<Vec<u8>>,
    changed: bool,
}

#[derive(Default)]
struct ParSignalState {
    processed_count: i32,
    files_to_repair: i32,
    extra_files: i32,
    verifying_extra_files: bool,
    has_damaged_files: bool,
    processed_files: Vec<Vec<u8>>,
    par_ignore_ext: Vec<u8>,
}

pub struct NzbgetRsParChecker {
    meta: Mutex<ParCheckerMeta>,
    queue: Mutex<ParQueueState>,
    signal: Mutex<ParSignalState>,
    cancel_token: Mutex<Option<Arc<AtomicBool>>>,
    verified_files: Mutex<Vec<Vec<u8>>>,
    err_msg: Mutex<Vec<u8>>,
    status: AtomicI32,
    stage: AtomicI32,
    file_progress: AtomicI32,
    stage_progress: AtomicI32,
    quick_files: AtomicI32,
    par_quick: AtomicBool,
    par_full: AtomicBool,
    force_repair: AtomicBool,
    block_size: AtomicI64,
    source_file_count: AtomicUsize,
}

impl Default for NzbgetRsParChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl NzbgetRsParChecker {
    pub fn new() -> Self {
        Self {
            meta: Mutex::new(ParCheckerMeta::default()),
            queue: Mutex::new(ParQueueState::default()),
            signal: Mutex::new(ParSignalState::default()),
            cancel_token: Mutex::new(None),
            verified_files: Mutex::new(Vec::new()),
            err_msg: Mutex::new(Vec::new()),
            status: AtomicI32::new(ParCheckerStatus::Failed.into()),
            stage: AtomicI32::new(PAR_STAGE_LOADING_PARS),
            file_progress: AtomicI32::new(0),
            stage_progress: AtomicI32::new(0),
            quick_files: AtomicI32::new(0),
            par_quick: AtomicBool::new(false),
            par_full: AtomicBool::new(false),
            force_repair: AtomicBool::new(false),
            block_size: AtomicI64::new(0),
            source_file_count: AtomicUsize::new(0),
        }
    }

    fn lock_meta(&self) -> std::sync::MutexGuard<'_, ParCheckerMeta> {
        self.meta.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_queue(&self) -> std::sync::MutexGuard<'_, ParQueueState> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_signal(&self) -> std::sync::MutexGuard<'_, ParSignalState> {
        self.signal.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_dest_dir(&self, v: Vec<u8>) {
        self.lock_meta().dest_dir = v;
    }
    pub fn dest_dir(&self) -> Vec<u8> {
        self.lock_meta().dest_dir.clone()
    }
    pub fn set_info_name(&self, v: Vec<u8>) {
        self.lock_meta().info_name = v;
    }
    pub fn info_name(&self) -> Vec<u8> {
        self.lock_meta().info_name.clone()
    }
    pub fn set_nzb_name(&self, v: Vec<u8>) {
        self.lock_meta().nzb_name = v;
    }
    pub fn nzb_name(&self) -> Vec<u8> {
        self.lock_meta().nzb_name.clone()
    }
    pub fn par_filename(&self) -> Vec<u8> {
        self.lock_meta().par_filename.clone()
    }
    pub fn progress_label(&self) -> Vec<u8> {
        self.lock_meta().progress_label.clone()
    }
    pub fn set_par_quick(&self, v: bool) {
        self.par_quick.store(v, Ordering::Release);
    }
    pub fn par_quick(&self) -> bool {
        self.par_quick.load(Ordering::Acquire)
    }
    pub fn set_force_repair(&self, v: bool) {
        self.force_repair.store(v, Ordering::Release);
    }
    pub fn set_par_full(&self, v: bool) {
        self.par_full.store(v, Ordering::Release);
    }
    pub fn par_full(&self) -> bool {
        self.par_full.load(Ordering::Acquire)
    }
    pub fn status(&self) -> ParCheckerStatus {
        ParCheckerStatus::from(self.status.load(Ordering::Acquire))
    }
    pub fn stage(&self) -> i32 {
        self.stage.load(Ordering::Acquire)
    }
    pub fn file_progress(&self) -> i32 {
        self.file_progress.load(Ordering::Acquire)
    }
    pub fn stage_progress(&self) -> i32 {
        self.stage_progress.load(Ordering::Acquire)
    }
    pub fn block_size(&self) -> i64 {
        self.block_size.load(Ordering::Acquire)
    }

    pub fn add_par_file(&self, par_filename: Vec<u8>) {
        let mut q = self.lock_queue();
        q.queued_par_files.push_back(par_filename);
        q.changed = true;
    }

    pub fn queue_changed(&self) {
        self.lock_queue().changed = true;
    }

    pub fn cancel(&self) {
        {
            let guard = self.cancel_token.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref tok) = *guard {
                tok.store(true, Ordering::Relaxed);
            }
        }
        self.queue_changed();
    }

    fn set_err_msg(&self, msg: Vec<u8>) {
        *self.err_msg.lock().unwrap_or_else(|e| e.into_inner()) = msg;
    }

    fn get_err_msg(&self) -> Vec<u8> {
        self.err_msg
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn cleanup(
        &self,
        repairer: &mut Option<RustParRepairer>,
        dupe_sources: &mut Vec<ParDupeSource>,
    ) {
        *self.cancel_token.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *repairer = None;
        self.block_size.store(0, Ordering::Release);
        self.source_file_count.store(0, Ordering::Release);
        self.lock_queue().queued_par_files.clear();
        self.lock_signal().processed_files.clear();
        dupe_sources.clear();
        self.set_err_msg(Vec::new());
        crate::memory_pressure::trim_heap();
    }

    pub fn execute(&self, hooks: &dyn ParCheckerHooks) {
        self.verified_files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        let mut st = self.run_par_check_all(hooks);
        self.status.store(st.into(), Ordering::Release);

        if st == ParCheckerStatus::RepairNotNeeded
            && self.par_quick()
            && self.force_repair.load(Ordering::Acquire)
            && !hooks.is_stopped()
        {
            let nzb_name = self.nzb_name();
            hooks.print_message(
                MK_INFO,
                &[b"Performing full par-check for ", nzb_name.as_slice()].concat(),
            );
            self.set_par_quick(false);
            self.verified_files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            st = self.run_par_check_all(hooks);
            self.status.store(st.into(), Ordering::Release);
        }

        let vf = self
            .verified_files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        for filename in &vf {
            hooks.register_verified_file(filename);
        }
        hooks.completed();
    }

    fn run_par_check_all(&self, hooks: &dyn ParCheckerHooks) -> ParCheckerStatus {
        let (dest_dir, nzb_name) = {
            let m = self.lock_meta();
            (m.dest_dir.clone(), m.nzb_name.clone())
        };
        let mut file_list: Vec<Vec<u8>> = Vec::new();
        if !crate::parparser::find_main_pars(crate::fsops::path(&dest_dir), true, |name| {
            file_list.push(name.to_vec());
        }) {
            hooks.print_message(
                MK_ERROR,
                &[
                    b"Could not start par-check for ",
                    nzb_name.as_slice(),
                    b". Could not find any par-files",
                ]
                .concat(),
            );
            return ParCheckerStatus::Failed;
        }

        let mut all_status = ParCheckerStatus::RepairNotNeeded;
        self.set_par_full(true);
        let sep = crate::paths::PATH_SEPARATOR;

        for par_filename in file_list {
            hooks.global_log(
                MK_DEBUG,
                &[b"Found par: ", par_filename.as_slice()].concat(),
            );

            if !hooks.is_stopped() {
                let path = [dest_dir.as_slice(), &[sep], par_filename.as_slice()].concat();
                let full_par_filename = crate::fsops::get_real_path(&path).unwrap_or(path);

                let probe = &par_filename[..par_filename.len().min(1023)];
                let base_len = crate::parparser::parse_par_filename(probe, true)
                    .map_or(0, |(l, _)| l.min(1023).min(par_filename.len()));
                let par_info_name =
                    trunc_1023([nzb_name.as_slice(), &[sep], &par_filename[..base_len]].concat());
                self.set_info_name(par_info_name);

                let status = self.run_par_check(full_par_filename, hooks);
                if all_status > status {
                    all_status = status;
                }
            }
        }

        all_status
    }

    fn run_par_check(
        &self,
        par_filename: Vec<u8>,
        hooks: &dyn ParCheckerHooks,
    ) -> ParCheckerStatus {
        let mut repairer: Option<RustParRepairer> = None;
        let mut dupe_sources: Vec<ParDupeSource> = Vec::new();
        let mut threads_to_use = 1i32;
        let mut mem_to_use = 16i32;

        self.cleanup(&mut repairer, &mut dupe_sources);

        let info_name = {
            let mut m = self.lock_meta();
            m.par_filename = par_filename.clone();
            m.progress_label = [b"Verifying ", m.info_name.as_slice()].concat();
            m.info_name.clone()
        };

        self.stage.store(PAR_STAGE_LOADING_PARS, Ordering::Release);
        {
            let mut sig = self.lock_signal();
            sig.processed_count = 0;
            sig.extra_files = 0;
            sig.verifying_extra_files = false;
            sig.has_damaged_files = false;
            sig.par_ignore_ext = hooks.read_options().par_ignore_ext;
        }
        self.quick_files.store(0, Ordering::Release);
        self.file_progress.store(0, Ordering::Release);
        self.stage_progress.store(0, Ordering::Release);

        hooks.print_message(MK_INFO, &[b"Verifying ", info_name.as_slice()].concat());
        hooks.global_log(MK_DEBUG, &[b"par: ", par_filename.as_slice()].concat());
        hooks.update_progress();

        let mut res = self.pre_process_par(
            &mut repairer,
            &mut dupe_sources,
            &mut threads_to_use,
            &mut mem_to_use,
            hooks,
        );
        if hooks.is_stopped() || res != ParResult::Success {
            self.cleanup(&mut repairer, &mut dupe_sources);
            return ParCheckerStatus::Failed;
        }

        let creator = repairer
            .as_ref()
            .map(RustParRepairer::creator)
            .unwrap_or_default();
        let creator_disp = if creator.is_empty() {
            "unknown"
        } else {
            creator.as_str()
        };
        hooks.global_log(
            MK_INFO,
            format!("Recovery files created by: {creator_disp}").as_bytes(),
        );

        self.stage
            .store(PAR_STAGE_VERIFYING_SOURCES, Ordering::Release);
        res = self.process_repairer(
            repairer.as_mut().expect("repairer initialized"),
            false,
            threads_to_use,
            mem_to_use,
            hooks,
        );

        if !self.par_quick() {
            let processed_snapshot = self.lock_signal().processed_files.clone();
            let refs: Vec<&[u8]> = processed_snapshot.iter().map(Vec::as_slice).collect();
            if repairer
                .as_ref()
                .expect("repairer initialized")
                .check_empty_files(&refs, &mut |msg| hooks.print_message(MK_WARNING, msg))
            {
                self.lock_signal().has_damaged_files = true;
            }
        }

        let opts = hooks.read_options();
        let mut added_splitted_fragments = false;
        if self.lock_signal().has_damaged_files
            && !hooks.is_stopped()
            && res == ParResult::RepairNotPossible
        {
            added_splitted_fragments = self
                .add_splitted_fragments(repairer.as_mut().expect("repairer initialized"), hooks);
            if added_splitted_fragments {
                res = self.process_repairer(
                    repairer.as_mut().expect("repairer initialized"),
                    false,
                    threads_to_use,
                    mem_to_use,
                    hooks,
                );
            }
        }

        if self.lock_signal().has_damaged_files
            && !hooks.is_stopped()
            && repairer
                .as_ref()
                .expect("repairer initialized")
                .missing_file_count()
                > 0
            && !(added_splitted_fragments && res == ParResult::RepairPossible)
            && (opts.par_scan == PAR_SCAN_EXTENDED || opts.par_scan == PAR_SCAN_DUPE)
        {
            let dest_dir = self.dest_dir();
            if self.add_extra_files(
                repairer.as_mut().expect("repairer initialized"),
                true,
                false,
                &dest_dir,
                hooks,
            ) {
                res = self.process_repairer(
                    repairer.as_mut().expect("repairer initialized"),
                    false,
                    threads_to_use,
                    mem_to_use,
                    hooks,
                );
            }
        }

        if self.lock_signal().has_damaged_files
            && !hooks.is_stopped()
            && res == ParResult::RepairNotPossible
        {
            res = self.process_more_pars(
                repairer.as_mut().expect("repairer initialized"),
                threads_to_use,
                mem_to_use,
                hooks,
            );
        }

        if self.lock_signal().has_damaged_files
            && !hooks.is_stopped()
            && res == ParResult::RepairNotPossible
            && opts.par_scan == PAR_SCAN_DUPE
        {
            if self.add_dupe_files(
                repairer.as_mut().expect("repairer initialized"),
                &mut dupe_sources,
                hooks,
            ) {
                res = self.process_repairer(
                    repairer.as_mut().expect("repairer initialized"),
                    false,
                    threads_to_use,
                    mem_to_use,
                    hooks,
                );
                if res == ParResult::RepairNotPossible && !hooks.is_stopped() {
                    res = self.process_more_pars(
                        repairer.as_mut().expect("repairer initialized"),
                        threads_to_use,
                        mem_to_use,
                        hooks,
                    );
                }
            }
        }

        if self.lock_signal().has_damaged_files
            && !hooks.is_stopped()
            && res == ParResult::RepairNotPossible
        {
            res = self.process_twin_set(
                &mut repairer,
                &mut dupe_sources,
                &mut threads_to_use,
                &mut mem_to_use,
                hooks,
            );
        }

        if hooks.is_stopped() {
            self.cleanup(&mut repairer, &mut dupe_sources);
            return ParCheckerStatus::Failed;
        }

        let mut status = ParCheckerStatus::Failed;
        let has_damaged = self.lock_signal().has_damaged_files;

        if res == ParResult::Success || !has_damaged {
            hooks.print_message(
                MK_INFO,
                &[b"Repair not needed for ", info_name.as_slice()].concat(),
            );
            status = ParCheckerStatus::RepairNotNeeded;
        } else if res == ParResult::RepairPossible {
            status = ParCheckerStatus::RepairPossible;
            if hooks.read_options().par_repair {
                hooks.print_message(MK_INFO, &[b"Repairing ", info_name.as_slice()].concat());
                self.lock_meta().progress_label = [b"Repairing ", info_name.as_slice()].concat();
                self.file_progress.store(0, Ordering::Release);
                self.stage_progress.store(0, Ordering::Release);
                {
                    let r = repairer.as_ref().expect("repairer initialized");
                    let mut sig = self.lock_signal();
                    sig.processed_count = 0;
                    sig.files_to_repair = (r.damaged_file_count() + r.missing_file_count()) as i32;
                }
                self.stage.store(PAR_STAGE_REPAIRING, Ordering::Release);
                hooks.update_progress();

                res = self.process_repairer(
                    repairer.as_mut().expect("repairer initialized"),
                    true,
                    threads_to_use,
                    mem_to_use,
                    hooks,
                );
                if res == ParResult::Success {
                    hooks.print_message(
                        MK_INFO,
                        &[b"Successfully repaired ", info_name.as_slice()].concat(),
                    );
                    status = ParCheckerStatus::Repaired;
                    hooks.stat_dupe_sources(&dupe_sources);
                    repairer
                        .as_ref()
                        .expect("repairer initialized")
                        .delete_leftovers(&mut |msg| hooks.print_message(MK_INFO, msg));
                } else {
                    status = ParCheckerStatus::Failed;
                }
            } else {
                hooks.print_message(
                    MK_INFO,
                    &[b"Repair possible for ", info_name.as_slice()].concat(),
                );
            }
        }

        if hooks.is_stopped() {
            if self.stage() >= PAR_STAGE_REPAIRING {
                hooks.print_message(
                    MK_WARNING,
                    &[b"Repair cancelled for ", info_name.as_slice()].concat(),
                );
                self.set_err_msg(b"repair cancelled".to_vec());
                status = ParCheckerStatus::RepairPossible;
            } else {
                hooks.print_message(
                    MK_WARNING,
                    &[b"Par-check cancelled for ", info_name.as_slice()].concat(),
                );
                self.set_err_msg(b"par-check cancelled".to_vec());
                status = ParCheckerStatus::Failed;
            }
        } else if status == ParCheckerStatus::Failed {
            let mut err = self.get_err_msg();
            if err.is_empty() {
                err = res.message().as_bytes().to_vec();
                self.set_err_msg(err.clone());
            }
            let msg = [
                b"Repair failed for ",
                info_name.as_slice(),
                b": ",
                err.as_slice(),
                b". Recovery files created by: ",
                creator_disp.as_bytes(),
            ]
            .concat();
            hooks.print_message(MK_ERROR, &msg);
        }

        if let Some(ref r) = repairer {
            let set_ok =
                status == ParCheckerStatus::Repaired || status == ParCheckerStatus::RepairNotNeeded;
            let mut vf = self
                .verified_files
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            r.for_each_target_verification(set_ok, &mut |target_path, keep_verified| {
                vf.retain(|prev| prev.as_slice() != target_path);
                if keep_verified {
                    vf.push(target_path.to_vec());
                }
            });
        }

        self.cleanup(&mut repairer, &mut dupe_sources);
        status
    }

    fn pre_process_par(
        &self,
        repairer: &mut Option<RustParRepairer>,
        dupe_sources: &mut Vec<ParDupeSource>,
        threads_to_use: &mut i32,
        mem_to_use: &mut i32,
        hooks: &dyn ParCheckerHooks,
    ) -> ParResult {
        let mut res = ParResult::RepairFailed;
        let info_name = self.info_name();

        while !hooks.is_stopped() && res != ParResult::Success {
            self.cleanup(repairer, dupe_sources);

            let opts = hooks.read_options();
            self.lock_signal().par_ignore_ext = opts.par_ignore_ext.clone();
            let cpu_cores = crate::parchecker::batch_repair_threads(
                opts.par_threads,
                crate::sysmetrics::usable_cpu_count() as i32,
                crate::sysmetrics::physical_cpu_count() as i32,
                crate::parchecker::repair_shares_cpu(),
            );
            let monitor_mb = if opts.has_memory_monitor {
                crate::articledl::memory_monitor_effective_par_buffer_mb(opts.par_buffer_mb)
            } else {
                0
            };
            let (threads, mem) = crate::parchecker::calc_repairer_resources(
                0,
                cpu_cores,
                opts.has_memory_monitor,
                monitor_mb,
                opts.par_buffer_mb,
            );
            *threads_to_use = threads;
            *mem_to_use = mem;

            let dest_dir = self.dest_dir();
            // Temporary live-repair files an earlier daemon left (killed mid-admission).
            #[cfg(unix)]
            crate::parlive::remove_stale_repair_temps(crate::fsops::path(&dest_dir));
            let mut r = RustParRepairer::new(
                crate::fsops::path(&dest_dir).to_path_buf(),
                threads,
                mem,
                self.par_quick(),
                opts.par_scan == PAR_SCAN_FULL,
                opts.par_ignore_ext,
            );
            *self.cancel_token.lock().unwrap_or_else(|e| e.into_inner()) = Some(r.cancel_token());

            let par_fn = self.par_filename();
            let ev = CheckerEvents {
                checker: self,
                hooks,
            };
            res = r.preprocess(crate::fsops::path(&par_fn), Some(&ev));
            self.block_size.store(r.block_size(), Ordering::Release);
            self.source_file_count
                .store(r.source_files().len(), Ordering::Release);
            *repairer = Some(r);

            hooks.global_log(
                MK_DEBUG,
                format!("ParChecker: PreProcess-result={}", res.code()).as_bytes(),
            );

            if hooks.is_stopped() {
                hooks.print_message(
                    MK_ERROR,
                    &[b"Could not verify ", info_name.as_slice(), b": stopping"].concat(),
                );
                self.set_err_msg(b"par-check was stopped".to_vec());
                return ParResult::RepairFailed;
            }

            if res == ParResult::InvalidArgs {
                hooks.print_message(
                    MK_ERROR,
                    &[
                        b"Could not start par-check for ",
                        info_name.as_slice(),
                        b". Par-file: ",
                        par_fn.as_slice(),
                    ]
                    .concat(),
                );
                self.set_err_msg(b"Command line could not be parsed".to_vec());
                return res;
            }

            if res != ParResult::Success {
                hooks.print_message(
                    MK_WARNING,
                    &[
                        b"Could not verify ",
                        info_name.as_slice(),
                        b": par2-file could not be processed",
                    ]
                    .concat(),
                );
                hooks.print_message(
                    MK_INFO,
                    &[b"Requesting more par2-files for ", info_name.as_slice()].concat(),
                );
                if !self.load_main_par_bak(hooks) {
                    hooks.print_message(MK_WARNING, b"No more par2-files found");
                    break;
                }
            }
        }

        if res != ParResult::Success {
            hooks.print_message(
                MK_ERROR,
                &[
                    b"Could not verify ",
                    info_name.as_slice(),
                    b": par2-file could not be processed",
                ]
                .concat(),
            );
            self.set_err_msg(b"par2-file could not be processed".to_vec());
        }

        res
    }

    fn load_main_par_bak(&self, hooks: &dyn ParCheckerHooks) -> bool {
        while !hooks.is_stopped() {
            let has_more = {
                let mut q = self.lock_queue();
                let non_empty = !q.queued_par_files.is_empty();
                q.queued_par_files.clear();
                non_empty
            };
            if has_more {
                return true;
            }

            let (requested, _) = hooks.request_more_pars(1);
            if requested {
                self.lock_meta().progress_label = b"Awaiting additional par-files".to_vec();
                self.file_progress.store(0, Ordering::Release);
                hooks.update_progress();
            }

            let has_more = {
                let mut q = self.lock_queue();
                q.changed = false;
                !q.queued_par_files.is_empty()
            };

            if !requested && !has_more {
                return false;
            }

            if !has_more {
                let mut changed = false;
                while !changed && !hooks.is_stopped() {
                    changed = self.lock_queue().changed;
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
        false
    }

    fn process_repairer(
        &self,
        repairer: &mut RustParRepairer,
        do_repair: bool,
        threads_to_use: i32,
        mem_to_use: i32,
        hooks: &dyn ParCheckerHooks,
    ) -> ParResult {
        if do_repair {
            let nzb_name = self.nzb_name();
            let prefix = format!(
                "Using {threads_to_use} thread(s) and {mem_to_use} MB to repair {} block(s) for ",
                repairer.missing_block_count()
            );
            hooks.print_message(MK_INFO, &[prefix.as_bytes(), nzb_name.as_slice()].concat());
        }
        self.source_file_count
            .store(repairer.source_files().len(), Ordering::Release);
        let ev = CheckerEvents {
            checker: self,
            hooks,
        };
        let res = repairer.process(do_repair, &ev);
        if !do_repair {
            let files = repairer.source_files();
            let quick = files.iter().filter(|file| file.quick_verified).count();
            hooks.print_message(MK_DETAIL, format!(
                "PAR verification metrics {{\"source_files\":{},\"quick_verified_files\":{},\"repair_needed\":{}}}",
                files.len(), quick, repairer.repairer.verify_result().repair_needed,
            ).as_bytes());
        }
        self.source_file_count
            .store(repairer.source_files().len(), Ordering::Release);
        hooks.global_log(
            MK_DEBUG,
            format!("ParChecker: Process-result={}", res.code()).as_bytes(),
        );
        res
    }

    fn verify_extra_files(
        &self,
        repairer: &mut RustParRepairer,
        extrafiles: &[Vec<u8>],
        is_split_fragment: bool,
        hooks: &dyn ParCheckerHooks,
    ) -> bool {
        let ev = CheckerEvents {
            checker: self,
            hooks,
        };
        let mut any_added = false;
        for extra in extrafiles {
            if repairer.verify_extra_file(crate::fsops::path(extra), is_split_fragment, &ev) {
                any_added = true;
            }
        }
        any_added
    }

    fn process_more_pars(
        &self,
        repairer: &mut RustParRepairer,
        threads_to_use: i32,
        mem_to_use: i32,
        hooks: &dyn ParCheckerHooks,
    ) -> ParResult {
        let mut res = ParResult::RepairNotPossible;
        let mut more_files_loaded = true;
        let info_name = self.info_name();

        while !hooks.is_stopped() && res == ParResult::RepairNotPossible {
            let missing_block_count =
                repairer.missing_block_count() as i32 - repairer.recovery_block_count() as i32;
            if missing_block_count <= 0 {
                return ParResult::RepairPossible;
            }

            if more_files_loaded {
                hooks.print_message(
                    MK_INFO,
                    &[
                        format!("Need more {missing_block_count} par-block(s) for ").as_bytes(),
                        info_name.as_slice(),
                    ]
                    .concat(),
                );
            }

            let mut has_more_pars = !self.lock_queue().queued_par_files.is_empty();
            if !has_more_pars {
                let (requested, block_found) = hooks.request_more_pars(missing_block_count);
                if requested {
                    self.lock_meta().progress_label = b"Awaiting additional par-files".to_vec();
                    self.file_progress.store(0, Ordering::Release);
                    hooks.update_progress();
                }

                has_more_pars = {
                    let mut q = self.lock_queue();
                    q.changed = false;
                    !q.queued_par_files.is_empty()
                };

                if !requested && !has_more_pars {
                    self.set_err_msg(
                        format!(
                            "not enough par-blocks, {missing_block_count} block(s) needed, but {block_found} block(s) available"
                        )
                        .into_bytes(),
                    );
                    break;
                }

                if !has_more_pars {
                    let mut changed = false;
                    while !changed && !hooks.is_stopped() {
                        changed = self.lock_queue().changed;
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }

            if hooks.is_stopped() {
                break;
            }

            let more_files: Vec<Vec<u8>> = {
                let mut q = self.lock_queue();
                q.queued_par_files.drain(..).collect()
            };

            let ev = CheckerEvents {
                checker: self,
                hooks,
            };
            for par_fn in &more_files {
                let loaded_ok =
                    repairer.load_extra_par2_file(crate::fsops::path(par_fn), Some(&ev));
                let base = &par_fn[crate::paths::base_file_name(par_fn)..];
                if loaded_ok {
                    hooks.print_message(
                        MK_INFO,
                        &[b"File ", base, b" successfully loaded for par-check"].concat(),
                    );
                } else {
                    hooks.print_message(
                        MK_INFO,
                        &[b"Could not load file ", base, b" for par-check"].concat(),
                    );
                }
            }

            more_files_loaded = !more_files.is_empty();
            if more_files_loaded {
                res = self.process_repairer(repairer, false, threads_to_use, mem_to_use, hooks);
            }
        }

        res
    }

    fn process_twin_set(
        &self,
        repairer: &mut Option<RustParRepairer>,
        dupe_sources: &mut Vec<ParDupeSource>,
        threads_to_use: &mut i32,
        mem_to_use: &mut i32,
        hooks: &dyn ParCheckerHooks,
    ) -> ParResult {
        let index = hooks.request_twin_index();
        if index.is_empty() || hooks.is_stopped() {
            return ParResult::RepairNotPossible;
        }

        let own_par_filename = self.par_filename();
        let own_err_msg = self.get_err_msg();
        self.lock_meta().par_filename = index;
        self.set_err_msg(Vec::new());

        let mut res =
            self.pre_process_par(repairer, dupe_sources, threads_to_use, mem_to_use, hooks);
        if res != ParResult::Success || hooks.is_stopped() {
            self.lock_meta().par_filename = own_par_filename;
            self.set_err_msg(own_err_msg);
            return ParResult::RepairNotPossible;
        }

        res = self.process_repairer(
            repairer.as_mut().expect("repairer initialized"),
            false,
            *threads_to_use,
            *mem_to_use,
            hooks,
        );
        if res == ParResult::RepairNotPossible && !hooks.is_stopped() {
            let r = repairer.as_ref().expect("repairer initialized");
            let block_needed =
                (r.missing_block_count() as i32 - r.recovery_block_count() as i32).max(1);
            let info_name = self.info_name();
            hooks.print_message(
                MK_INFO,
                &[
                    format!("Need {block_needed} par-block(s) of the twin's set for ").as_bytes(),
                    info_name.as_slice(),
                ]
                .concat(),
            );
            let volumes = hooks.request_twin_volumes(block_needed);
            let ev = CheckerEvents {
                checker: self,
                hooks,
            };
            let r_mut = repairer.as_mut().expect("repairer initialized");
            for volume in &volumes {
                if r_mut.load_extra_par2_file(crate::fsops::path(volume), Some(&ev)) {
                    let base = &volume[crate::paths::base_file_name(volume)..];
                    hooks.print_message(
                        MK_INFO,
                        &[b"File ", base, b" of the twin loaded for par-check"].concat(),
                    );
                }
            }
            if !volumes.is_empty() && !hooks.is_stopped() {
                res = self.process_repairer(
                    repairer.as_mut().expect("repairer initialized"),
                    false,
                    *threads_to_use,
                    *mem_to_use,
                    hooks,
                );
            }
        }

        if res == ParResult::RepairNotPossible {
            let msg = if own_err_msg.is_empty() {
                b"not enough par-blocks, the twin's included".to_vec()
            } else {
                [own_err_msg.as_slice(), b"; the twin's didn't do either"].concat()
            };
            self.set_err_msg(msg);
        }
        res
    }

    fn add_splitted_fragments(
        &self,
        repairer: &mut RustParRepairer,
        hooks: &dyn ParCheckerHooks,
    ) -> bool {
        let dest_dir = self.dest_dir();
        let processed_snapshot = self.lock_signal().processed_files.clone();
        let refs: Vec<&[u8]> = processed_snapshot.iter().map(Vec::as_slice).collect();

        let extrafiles = repairer.discover_splitted_fragments(
            &dest_dir,
            &refs,
            &mut |fname| hooks.is_parred_file(fname),
            &mut |curr| hooks.find_file_origname(curr),
            &mut |msg| hooks.global_log(MK_DETAIL, msg),
        );

        let mut fragments_added = false;
        if !extrafiles.is_empty() {
            {
                let mut sig = self.lock_signal();
                sig.extra_files += extrafiles.len() as i32;
                sig.verifying_extra_files = true;
            }
            let info_name = self.info_name();
            hooks.print_message(
                MK_INFO,
                &[
                    format!("Found {} splitted fragments for ", extrafiles.len()).as_bytes(),
                    info_name.as_slice(),
                ]
                .concat(),
            );
            fragments_added = self.verify_extra_files(repairer, &extrafiles, true, hooks);
            self.lock_signal().verifying_extra_files = false;
        }
        fragments_added
    }

    fn add_dupe_files(
        &self,
        repairer: &mut RustParRepairer,
        dupe_sources: &mut Vec<ParDupeSource>,
        hooks: &dyn ParCheckerHooks,
    ) -> bool {
        let par_fn = self.par_filename();
        let sep = crate::paths::PATH_SEPARATOR;
        let dir_len = par_fn
            .iter()
            .rposition(|&b| b == sep)
            .unwrap_or(par_fn.len())
            .min(1023);
        let directory = par_fn[..dir_len].to_vec();

        let mut added = self.add_extra_files(repairer, false, false, &directory, hooks);

        if repairer.missing_block_count() > 0 {
            *dupe_sources = hooks.request_dupe_sources();
            if !dupe_sources.is_empty() {
                let was_blocks_missing = repairer.missing_block_count();
                let mut total_extra_blocks_found = 0u32;
                for ds in dupe_sources.iter_mut() {
                    if repairer.missing_block_count() > 0
                        && crate::fsops::directory_exists(&ds.directory, &crate::fsops::posix_stat)
                    {
                        let was_missing2 = repairer.missing_block_count();
                        let one_added =
                            self.add_extra_files(repairer, false, true, &ds.directory, hooks);
                        added |= one_added;
                        let now_missing2 = repairer.missing_block_count();
                        ds.used_blocks += (was_missing2.wrapping_sub(now_missing2)) as i32;
                        total_extra_blocks_found = if now_missing2 < was_blocks_missing {
                            was_blocks_missing - now_missing2
                        } else {
                            0
                        };
                    }
                }

                if total_extra_blocks_found > 0 {
                    hooks.print_message(
                        MK_INFO,
                        format!("Found extra {total_extra_blocks_found} blocks in dupe sources")
                            .as_bytes(),
                    );
                } else {
                    hooks.print_message(MK_INFO, b"No extra blocks found in dupe sources");
                }
            }
        }

        added
    }

    fn add_extra_files(
        &self,
        repairer: &mut RustParRepairer,
        only_missing: bool,
        external_dir: bool,
        directory: &[u8],
        hooks: &dyn ParCheckerHooks,
    ) -> bool {
        let info_name = self.info_name();
        if external_dir {
            let base = &directory[crate::paths::base_file_name(directory)..];
            hooks.print_message(
                MK_INFO,
                &[
                    b"Performing dupe par-scan for ",
                    info_name.as_slice(),
                    b" in ",
                    base,
                ]
                .concat(),
            );
        } else {
            hooks.print_message(
                MK_INFO,
                &[b"Performing extra par-scan for ", info_name.as_slice()].concat(),
            );
        }

        let par_fn = self.par_filename();
        let processed_snapshot = self.lock_signal().processed_files.clone();
        let refs: Vec<&[u8]> = processed_snapshot.iter().map(Vec::as_slice).collect();
        let extrafiles = crate::parchecker::find_and_sort_extra_files(
            directory,
            external_dir,
            &par_fn,
            &refs,
            &mut |fname| hooks.is_parred_file(fname),
        );

        let mut files_added = false;
        if !extrafiles.is_empty() {
            {
                let mut sig = self.lock_signal();
                sig.extra_files += extrafiles.len() as i32;
                sig.verifying_extra_files = true;
            }

            let mut idx = 0usize;
            while !hooks.is_stopped() && idx < extrafiles.len() {
                let extra_file = &extrafiles[idx];
                idx += 1;

                let was_files_missing = repairer.missing_file_count();
                let was_blocks_missing = repairer.missing_block_count();

                self.verify_extra_files(repairer, std::slice::from_ref(extra_file), false, hooks);

                let now_files_missing = repairer.missing_file_count();
                let now_blocks_missing = repairer.missing_block_count();
                let file_added = was_files_missing > now_files_missing;
                let block_added = was_blocks_missing > now_blocks_missing;
                files_added = files_added || file_added || block_added;

                if file_added && !external_dir {
                    let base = &extra_file[crate::paths::base_file_name(extra_file)..];
                    hooks.print_message(MK_INFO, &[b"Found missing file ", base].concat());
                    hooks.register_parred_file(base);
                } else if block_added {
                    hooks.print_message(
                        MK_INFO,
                        format!(
                            "Found {} missing blocks",
                            was_blocks_missing - now_blocks_missing
                        )
                        .as_bytes(),
                    );
                }

                if only_missing && now_files_missing == 0 {
                    hooks.print_message(MK_INFO, b"All missing files found, aborting par-scan");
                    break;
                }
                if !only_missing && now_blocks_missing == 0 {
                    hooks.print_message(MK_INFO, b"All missing blocks found, aborting par-scan");
                    break;
                }
            }

            self.lock_signal().verifying_extra_files = false;
        }

        files_added
    }

    fn signal_filename(&self, name: &[u8], hooks: &dyn ParCheckerHooks) {
        let mut sig = self.lock_signal();
        let mut stage = self.stage();
        if stage == PAR_STAGE_REPAIRING {
            stage = PAR_STAGE_VERIFYING_REPAIRED;
            self.stage.store(stage, Ordering::Release);
        }

        let stage_msg: &[u8] = match stage {
            PAR_STAGE_LOADING_PARS => b"Loading file",
            PAR_STAGE_VERIFYING_SOURCES => b"Verifying file",
            PAR_STAGE_REPAIRING => b"Repairing file",
            PAR_STAGE_VERIFYING_REPAIRED => b"Verifying repaired file",
            _ => b"Verifying file",
        };
        if !(stage == PAR_STAGE_VERIFYING_REPAIRED && self.par_quick()) {
            hooks.print_message(MK_INFO, &[stage_msg, b" ", name].concat());
        }

        self.lock_meta().progress_label = [stage_msg, b" ", name].concat();

        if stage == PAR_STAGE_LOADING_PARS || stage == PAR_STAGE_VERIFYING_SOURCES {
            sig.processed_files.push(name.to_vec());
        }
        self.file_progress.store(0, Ordering::Release);
        drop(sig);
        hooks.update_progress();
    }

    fn signal_progress(&self, progress: i32, hooks: &dyn ParCheckerHooks) {
        let sig = self.lock_signal();
        self.file_progress.store(progress, Ordering::Release);
        let stage = self.stage();

        let raw_stage_progress = if stage == PAR_STAGE_REPAIRING {
            progress
        } else {
            let mut processed_files = sig.processed_count;
            let mut total_files = if stage == PAR_STAGE_VERIFYING_REPAIRED {
                sig.files_to_repair
            } else {
                let mut tf =
                    self.source_file_count.load(Ordering::Acquire) as i32 + sig.extra_files;
                if sig.extra_files > 0 {
                    let qf = self.quick_files.load(Ordering::Acquire);
                    tf -= qf;
                    processed_files -= qf;
                }
                tf
            };
            if total_files > 0 {
                let numer = if progress < 1000 {
                    processed_files * 1000 + progress
                } else {
                    processed_files * 1000
                };
                numer / total_files
            } else {
                total_files = 0;
                total_files
            }
        };
        let clamped = raw_stage_progress.clamp(0, 1000);
        self.stage_progress.store(clamped, Ordering::Release);
        drop(sig);
        hooks.global_log(
            MK_DEBUG,
            format!("Current-progress: {progress}, Total-progress: {clamped}").as_bytes(),
        );
        hooks.update_progress();
    }

    fn signal_done(
        &self,
        file_name: &[u8],
        available: i32,
        total: i32,
        file_exists: bool,
        hooks: &dyn ParCheckerHooks,
    ) {
        let mut sig = self.lock_signal();
        sig.processed_count += 1;

        if self.stage() == PAR_STAGE_VERIFYING_SOURCES
            && available < total
            && !sig.verifying_extra_files
        {
            let ignore = crate::util::match_file_ext_locale(file_name, &sig.par_ignore_ext, b",;");
            sig.has_damaged_files = sig.has_damaged_files || !ignore;
            let suffix: &[u8] = if ignore { b" (ignored)" } else { b"" };
            let msg = if file_exists {
                [
                    b"File ",
                    file_name,
                    format!(
                        " has {} bad block(s) of total {total} block(s)",
                        total - available
                    )
                    .as_bytes(),
                    suffix,
                ]
                .concat()
            } else {
                [
                    b"File ",
                    file_name,
                    format!(" with {total} block(s) is missing").as_bytes(),
                    suffix,
                ]
                .concat()
            };
            hooks.print_message(MK_WARNING, &msg);

            let refs: Vec<&[u8]> = sig.processed_files.iter().map(Vec::as_slice).collect();
            if !is_processed_file(file_name, &refs) {
                sig.processed_files.push(file_name.to_vec());
            }
        }
    }
}

struct CheckerEvents<'a> {
    checker: &'a NzbgetRsParChecker,
    hooks: &'a dyn ParCheckerHooks,
}

impl ParRepairEvents for CheckerEvents<'_> {
    fn on_filename(&self, name: &[u8]) {
        self.checker.signal_filename(name, self.hooks);
    }

    fn on_progress(&self, progress: i32) {
        self.checker.signal_progress(progress, self.hooks);
    }

    fn on_done(&self, name: &[u8], available: i32, total: i32, target_exists: bool) {
        self.checker
            .signal_done(name, available, total, target_exists, self.hooks);
    }

    fn on_quick_verify(&self, name: &[u8]) -> Option<QuickInfo> {
        self.hooks.find_file_quick_info(name)
    }

    fn on_quick_verified_inc(&self) {
        self.checker.quick_files.fetch_add(1, Ordering::AcqRel);
        self.checker.set_par_full(false);
    }
}

pub(crate) fn quick_info_from_status(
    status: i32,
    crc: u32,
    raw_segs: &[NzbgetRsParSegment],
) -> Option<QuickInfo> {
    match status {
        1 => Some(QuickInfo::Success { crc }),
        2 => {
            let segments = raw_segs
                .iter()
                .filter(|s| s.offset >= 0 && s.size >= 0)
                .map(|s| ParSegment {
                    success: s.success != 0,
                    offset: s.offset,
                    size: s.size as i64,
                    crc: s.crc,
                })
                .collect();
            Some(QuickInfo::Partial { segments })
        }
        3 => Some(QuickInfo::Failure),
        _ => None,
    }
}

// ============================================================================
// 2. UnpackController (`daemon/postprocess/UnpackController.cpp`)
// ============================================================================

#[derive(Default)]
struct UnpackRunState {
    name: Vec<u8>,
    info_name: Vec<u8>,
    info_name_up: Vec<u8>,
    dest_dir: Vec<u8>,
    final_dir: Vec<u8>,
    unpack_dir: Vec<u8>,
    password: Vec<u8>,
    unpack_pass_file: Vec<u8>,
    unpack_ignore_ext: Vec<u8>,
    inter_dir_opt: Vec<u8>,
    downloaded_files: Vec<Vec<u8>>,
    extracted_archives: Vec<Vec<u8>>,
    joined_files: Vec<Vec<u8>>,
    log_prefix: Option<&'static [u8]>,
    is_seven_zip: bool,
    inter_dir: bool,
    all_ok_received: bool,
    has_par_files: bool,
    has_rar_files: bool,
    has_not_unpacked_rar_files: bool,
    has_renamed_archive_files: bool,
    has_seven_zip_files: bool,
    has_seven_zip_multi_files: bool,
    has_splitted_files: bool,
    unpack_ok: bool,
    unpack_start_error: bool,
    unpack_space_error: bool,
    /// An unpack limit was breached (`unpacklimits`): a par repair cannot help.
    unpack_limit_error: bool,
    /// The job's unpack limits, shared by every unpack pass.
    unpack_guard: Option<crate::unpacklimits::SharedGuard>,
    unpack_decrypt_error: bool,
    unpack_password_error: bool,
    cleaned_up_disk: bool,
    auto_terminated: bool,
    final_dir_created: bool,
    unpack_dir_created: bool,
    pass_list_tried: bool,
    unpack_tried: bool,
    par_repaired: bool,
    prev_pass_list_tried: bool,
    last_unpack_status: UnpackStatus,
    direct_unpack_status: DirectUnpackStatus,
    /// Volumes of a 7z, ZIP or split-file set direct unpack extracted live are in the folder.
    has_live_extracted: bool,
    par_status: NzbParStatus,
    par_rename_status: i32,
    use_temp_unpack_dir: bool,
    unpack_cleanup_disk: bool,
    write_buffer_kb: i32,
    par_rename_opt: bool,
    umask: u32,
    parcheck_enabled: bool,
}

/// A post job together with the queue host it reports to.
#[cfg(unix)]
#[derive(Clone, Copy)]
pub(crate) struct JobQueue<'a> {
    job: &'a NzbgetRsPostJob,
    pub(crate) host: &'a crate::queuesubsys_host::QueueSubsysHost,
}

/// The address of a post job, for stopping it from a helper thread.
#[cfg(unix)]
pub(crate) struct JobStopHandle(usize);

#[cfg(unix)]
impl JobStopHandle {
    /// Stops the job, as the job's own stop path does.
    ///
    /// # Safety
    /// The job must still be alive.
    pub(crate) unsafe fn stop(&self) {
        let Some(host) = crate::ffi::queue_host() else {
            return;
        };
        let job = unsafe { &*(self.0 as *const NzbgetRsPostJob) };
        job.stop_from_rust(&host);
    }
}

#[cfg(unix)]
impl JobQueue<'_> {
    pub(crate) fn post_ptr(self) -> crate::queuesubsys_host::PostRef {
        self.job.post_ptr
    }

    /// Adds a message to the job's nzb.
    pub(crate) fn job_message(self, kind: i32, msg: &[u8]) {
        self.job.add_message(self.host, kind, msg);
    }

    /// Stops the job's thread and its controller.
    pub(crate) fn stop(self) {
        self.job.stop_from_rust(self.host);
    }

    fn nzb_ptr(self) -> NzbRef {
        self.job.nzb_ptr(self.host)
    }

    pub(crate) fn nzb_id(self) -> i32 {
        self.job.nzb_id
    }

    pub(crate) fn thread_ptr(self) -> usize {
        self.job.thread_ptr
    }

    /// A handle another thread can use to stop this job. The caller must not use it
    /// after the job is gone (the stream-repair watchdog is joined before its run returns).
    pub(crate) fn stop_handle(self) -> JobStopHandle {
        JobStopHandle(self.job as *const NzbgetRsPostJob as usize)
    }

    pub(crate) fn is_stopped(self) -> bool {
        self.host.thread_is_stopped(self.job.thread_ptr)
    }

    fn is_deleted(self) -> bool {
        self.job.post_ptr.is_some() && self.host.post_snap(self.job.post_ptr).deleted
    }

    fn add_message(self, kind: i32, msg: &[u8]) {
        if self.job.post_ptr.is_some() {
            self.job.add_message(self.host, kind, cstr::until_nul(msg));
        }
    }

    /// Writes to the global log only (not the job's message list).
    fn log(self, kind: i32, msg: &[u8]) {
        self.host.log(kind, cstr::until_nul(msg));
    }

    fn set_progress_label(self, label: &[u8]) {
        self.host.with_locked_queue(|_| {
            self.host
                .post_set_progress_label(self.job.post_ptr, cstr::until_nul(label));
        });
    }

    fn set_stage_progress(self, stage_progress: i32) {
        use crate::queuesubsys_host::POST_INT_STAGE_PROGRESS;
        if self.job.post_ptr.is_some() {
            self.host
                .post_set_int(self.job.post_ptr, POST_INT_STAGE_PROGRESS, stage_progress);
        }
    }

    fn reset_stage_time(self) {
        if self.job.post_ptr.is_some() {
            self.host
                .post_set_stage_time(self.job.post_ptr, current_time_sec());
        }
    }

    /// Fills `st` from the job and the options; returns whether unpacking is enabled.
    /// The caller holds the queue lock.
    fn read_unpack_settings(self, st: &mut UnpackRunState) -> bool {
        let host = self.host;
        let post_ptr = self.job.post_ptr;
        let ps = host.post_snap(post_ptr);
        let n = ps.nzb_ptr;
        let ns = host.nzb_snap(n, false);
        let unpack_param = host.nzb_get_param(n, b"*Unpack:");
        let password = host.nzb_get_param(n, b"*Unpack:Password");
        let opts = crate::options::active_options();
        let opt = |id| {
            cstr::until_nul(opts.as_ref().map_or(&b""[..], |o| o.str_bytes(id))).to_vec()
        };
        let sc = opts.as_ref().map(|o| o.scalars());

        st.dest_dir = ns.dest_dir;
        st.final_dir = ns.final_dir;
        st.name = ns.name;
        st.password = password
            .as_deref()
            .map_or_else(Vec::new, |p| cstr::until_nul(p).to_vec());
        st.unpack_pass_file = opt(OptStr::UnpackPassFile);
        st.unpack_ignore_ext = opt(OptStr::UnpackIgnoreExt);
        st.inter_dir_opt = opt(OptStr::InterDir);
        st.downloaded_files = (0..host.nzb_completed_files_len(n))
            .map(|i| cstr::until_nul(&host.nzb_completed_file_at(n, i).filename).to_vec())
            .collect();
        st.extracted_archives = host.post_extracted_archives(post_ptr);
        st.unpack_tried = ps.unpack_tried;
        st.par_repaired = ps.par_repaired;
        st.prev_pass_list_tried = ps.pass_list_tried;
        st.last_unpack_status = ps.last_unpack_status;
        st.direct_unpack_status = ns.direct_unpack_status;
        st.par_status = ns.par_status;
        st.par_rename_status = ns.par_rename_status;
        st.use_temp_unpack_dir = sc.as_ref().is_some_and(|s| s.use_temp_unpack_dir != 0);
        st.unpack_cleanup_disk = sc.as_ref().is_some_and(|s| s.unpack_cleanup_disk != 0);
        st.write_buffer_kb = sc.as_ref().map_or(0, |s| s.write_buffer);
        st.par_rename_opt = sc.as_ref().is_some_and(|s| s.par_rename != 0);
        st.umask = crate::lifecycle::active_umask() as u32;
        st.parcheck_enabled = crate::ffi::build_parcheck_enabled();

        !unpack_param
            .as_deref()
            .is_some_and(|v| v.eq_ignore_ascii_case(b"no"))
    }

    fn set_unpack_status(self, status: UnpackStatus) {
        self.host.nzb_set_unpack_status(self.nzb_ptr(), status);
    }

    fn unpack_succeeded(self, cleaned_up_disk: bool, par_rename: bool) {
        use crate::queuesubsys_host::{
            NZB_INT_PAR_RENAME_STATUS, NZB_INT_UNPACK_CLEANED_UP_DISK, RS_NONE,
        };
        let n = self.nzb_ptr();
        self.host.nzb_set_unpack_status(n, UnpackStatus::Success);
        self.host
            .nzb_set_int(n, NZB_INT_UNPACK_CLEANED_UP_DISK, i32::from(cleaned_up_disk));
        if par_rename {
            self.host.nzb_set_int(n, NZB_INT_PAR_RENAME_STATUS, RS_NONE);
        }
    }

    fn request_par_check(
        self,
        force_repair: bool,
        pass_list_tried: bool,
        last_status: UnpackStatus,
    ) {
        use crate::queuesubsys_host::{
            POST_INT_FORCE_REPAIR, POST_INT_LAST_UNPACK_STATUS, POST_INT_PASS_LIST_TRIED,
            POST_INT_REQUEST_PAR_CHECK, POST_INT_UNPACK_TRIED,
        };
        let pi = self.job.post_ptr;
        self.host.post_set_int(pi, POST_INT_REQUEST_PAR_CHECK, 1);
        self.host
            .post_set_int(pi, POST_INT_FORCE_REPAIR, i32::from(force_repair));
        self.host.post_set_int(pi, POST_INT_UNPACK_TRIED, 1);
        self.host
            .post_set_int(pi, POST_INT_PASS_LIST_TRIED, i32::from(pass_list_tried));
        self.host
            .post_set_int(pi, POST_INT_LAST_UNPACK_STATUS, last_status.into());
    }

    /// Adds `elapsed_sec` to the job's unpack time and ends the stage.
    fn finish(self, elapsed_sec: i32) {
        use crate::queuesubsys_host::{NZB_INT_UNPACK_SEC, POST_INT_WORKING};
        let n = self.nzb_ptr();
        let ns = self.host.nzb_snap(n, false);
        self.host
            .nzb_set_int(n, NZB_INT_UNPACK_SEC, ns.unpack_sec + elapsed_sec);
        self.host.post_set_int(self.job.post_ptr, POST_INT_WORKING, 0);
    }

    /// The job's par status and whether par was a full check.
    fn par_info(self) -> (NzbParStatus, bool) {
        let ns = self.host.nzb_snap(self.nzb_ptr(), false);
        (ns.par_status, ns.par_full)
    }

    fn build_final_dir_name(self) -> Vec<u8> {
        cstr::until_nul(&self.host.nzb_build_final_dir_name(self.nzb_ptr())).to_vec()
    }

    fn set_final_dir(self, final_dir: &[u8]) {
        use crate::queuesubsys_host::NZB_STR_FINAL_DIR;
        self.host
            .nzb_set_str(self.nzb_ptr(), NZB_STR_FINAL_DIR, cstr::until_nul(final_dir));
    }
}

#[cfg(unix)]
pub struct NzbgetRsUnpackController {
    terminated: AtomicBool,
    unpacking: AtomicBool,
    exec_thread_key: AtomicUsize,
}

#[cfg(unix)]
impl Default for NzbgetRsUnpackController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(unix)]
impl NzbgetRsUnpackController {
    pub fn new() -> Self {
        Self {
            terminated: AtomicBool::new(false),
            unpacking: AtomicBool::new(false),
            exec_thread_key: AtomicUsize::new(0),
        }
    }

    pub fn stop(&self) {
        self.terminated.store(true, Ordering::Release);
    }

    fn is_stopped_or_terminated(&self, q: JobQueue<'_>) -> bool {
        q.is_stopped() || self.terminated.load(Ordering::Acquire)
    }

    fn print_message(
        &self,
        q: JobQueue<'_>,
        st: &mut UnpackRunState,
        kind: i32,
        msg: &[u8],
    ) {
        if let Some(prefix) = st.log_prefix {
            let full = [prefix, b": ", msg].concat();
            self.add_message(q, st, kind, &full);
        } else {
            self.add_message(q, st, kind, msg);
        }
    }

    fn add_message(
        &self,
        q: JobQueue<'_>,
        st: &mut UnpackRunState,
        kind: i32,
        text: &[u8],
    ) {
        let info = crate::unpack::parse_unpack_message(text, &st.unpack_dir, st.is_seven_zip);
        let msg_text: Vec<u8> = if !st.is_seven_zip && info.strip_extracting_dir != 0 {
            trunc_1023([b"Unrar: Extracting ", &text[info.stripped_suffix_offset..]].concat())
        } else {
            trunc_1023(text.to_vec())
        };

        q.add_message(kind, &msg_text);

        if !st.is_seven_zip && info.reset_stage_time != 0 {
            q.reset_stage_time();
        }

        if !st.is_seven_zip && info.has_extracting_progress != 0 && msg_text.len() >= 7 {
            q.set_progress_label(&msg_text[7..]);
        }

        if !st.is_seven_zip && info.has_extracting_from != 0 && text.len() >= 7 {
            if text.len() >= 23 {
                q.log(
                    MK_DEBUG,
                    &[b"Filename: ", &text[23..]].concat(),
                );
            }
            q.set_progress_label(&text[7..]);
        }

        if info.decrypt_error != 0 {
            st.unpack_decrypt_error = true;
        }
        if info.password_error != 0 {
            st.unpack_password_error = true;
        }
        if crate::unpack::is_unpack_space_error(kind == MK_ERROR, text) {
            st.unpack_space_error = true;
        }
        // Also behind a log prefix ("Join: Unpack limit: ...").
        if kind == MK_ERROR
            && text
                .windows(crate::unpacklimits::LIMIT_PREFIX.len())
                .any(|w| w == crate::unpacklimits::LIMIT_PREFIX.as_bytes())
        {
            st.unpack_limit_error = true;
        }

        if !q.is_stopped()
            && (st.unpack_decrypt_error
                || st.unpack_password_error
                || info.crc_or_volume_error != 0)
        {
            if !((st.unpack_decrypt_error || st.unpack_password_error)
                && st.password.is_empty()
                && !st.unpack_pass_file.is_empty())
            {
                let warn_msg = trunc_1023(
                    [b"Cancelling ", st.info_name.as_slice(), b" due to errors"].concat(),
                );
                q.add_message(MK_WARNING, &warn_msg);
            }
            st.auto_terminated = true;
            q.job.stop_from_rust(q.host);
        }

        if info.all_ok != 0 {
            st.all_ok_received = true;
        }
    }

    fn run(&self, q: JobQueue<'_>) {
        let start = current_time_sec();
        let mut st = UnpackRunState {
            unpack_ok: true,
            ..UnpackRunState::default()
        };

        let unpack = q.host.with_locked_queue(|_| q.read_unpack_settings(&mut st));

        st.info_name = [b"unpack for ", st.name.as_slice()].concat();
        st.info_name_up = [b"Unpack for ", st.name.as_slice()].concat();
        st.has_par_files =
            crate::parparser::find_main_pars(crate::fsops::path(&st.dest_dir), false, |_| {});

        if unpack {
            let dl_refs: Vec<&[u8]> = st.downloaded_files.iter().map(Vec::as_slice).collect();
            let ex_refs: Vec<&[u8]> = st.extracted_archives.iter().map(Vec::as_slice).collect();
            let chk = crate::unpack::check_archive_files(
                &st.dest_dir,
                st.use_temp_unpack_dir,
                &dl_refs,
                &ex_refs,
                &st.unpack_ignore_ext,
            );
            st.has_rar_files = chk.has_rar_files;
            st.has_not_unpacked_rar_files = chk.has_not_unpacked_rar_files;
            st.has_renamed_archive_files = chk.has_renamed_archive_files;
            st.has_seven_zip_files = chk.has_seven_zip_files;
            st.has_seven_zip_multi_files = chk.has_seven_zip_multi_files;
            st.has_splitted_files = chk.has_splitted_files;
            st.has_live_extracted = chk.has_live_extracted;
        }

        let has_files = st.has_rar_files
            || st.has_seven_zip_files
            || st.has_seven_zip_multi_files
            || st.has_splitted_files
            || st.has_live_extracted;

        if st.unpack_tried
            && !st.par_repaired
            && (!st.password.is_empty()
                || st.unpack_pass_file.is_empty()
                || st.prev_pass_list_tried)
        {
            let m1 = [
                b"Second unpack attempt skipped for ",
                st.name.as_slice(),
                b" due to par-check not repaired anything",
            ]
            .concat();
            self.print_message(q, &mut st, MK_INFO, &m1);
            let m2 = if st.last_unpack_status == UnpackStatus::Password {
                [
                    st.info_name_up.as_slice(),
                    b" failed: checksum error in the encrypted file. Corrupt file or wrong password.",
                ]
                .concat()
            } else {
                [st.info_name_up.as_slice(), b" failed."].concat()
            };
            self.print_message(q, &mut st, MK_ERROR, &m2);
            q.set_unpack_status(st.last_unpack_status);
        } else if unpack && has_files {
            let m = [b"Unpacking ", st.name.as_slice()].concat();
            self.print_message(q, &mut st, MK_INFO, &m);
            self.create_unpack_dir(q, &mut st);
            // One guard for the job's unpack limits, continuing the count of the job's direct
            // unpack. Without one (after a restart), a temporary unpack folder that was there
            // already holds what direct unpack wrote for this job: it counts too.
            let guard = crate::unpacklimits::take_handed_over(crate::fsops::path(&st.unpack_dir))
                .unwrap_or_else(|| {
                    let mut guard = crate::unpacklimits::Guard::new(
                        crate::unpacklimits::Limits::from_options(),
                        crate::fsops::path(&st.unpack_dir),
                        &[
                            crate::fsops::path(&st.dest_dir),
                            crate::fsops::path(&st.final_dir),
                        ],
                    );
                    if st.use_temp_unpack_dir && !st.unpack_dir_created {
                        guard.seed_existing();
                    }
                    std::sync::Arc::new(std::sync::Mutex::new(guard))
                });
            st.unpack_guard = Some(guard);

            if st.has_live_extracted {
                // What direct unpack extracted live is gone, or par2 repaired volumes since: a
                // split file has no checksum of its own, so its join is redone from the repaired
                // pieces (archive members were verified against their checksums).
                let gone = st.unpack_dir_created;
                let redo = |a: &Vec<u8>| {
                    crate::livefeed::is_live_first(a)
                        && (gone || crate::livefeed::is_split_first(a))
                };
                if gone || (st.par_repaired && st.extracted_archives.iter().any(redo)) {
                    let txt: &[u8] = if gone {
                        b"Could not find files unpacked by direct unpack, unpacking them again"
                    } else {
                        b"Par-repair changed the pieces of a file joined by direct unpack, joining it again"
                    };
                    self.print_message(q, &mut st, if gone { MK_WARNING } else { MK_INFO }, txt);
                    st.extracted_archives.retain(|a| !redo(a));
                    let dl_refs: Vec<&[u8]> =
                        st.downloaded_files.iter().map(Vec::as_slice).collect();
                    let ex_refs: Vec<&[u8]> =
                        st.extracted_archives.iter().map(Vec::as_slice).collect();
                    let chk = crate::unpack::check_archive_files(
                        &st.dest_dir,
                        st.use_temp_unpack_dir,
                        &dl_refs,
                        &ex_refs,
                        &st.unpack_ignore_ext,
                    );
                    st.has_seven_zip_files |= chk.has_seven_zip_files;
                    st.has_seven_zip_multi_files |= chk.has_seven_zip_multi_files;
                    st.has_splitted_files |= chk.has_splitted_files;
                    st.has_live_extracted = chk.has_live_extracted;
                }
                if st.has_live_extracted && !st.has_rar_files {
                    self.print_message(q, &mut st, MK_INFO, b"Using directly unpacked files");
                }
            }

            if st.has_rar_files {
                if st.has_not_unpacked_rar_files || st.unpack_dir_created {
                    if st.direct_unpack_status == DirectUnpackStatus::Success {
                        let (kind, txt): (i32, &[u8]) = if st.unpack_dir_created {
                            (
                                MK_WARNING,
                                b"Could not find files unpacked by direct unpack, unpacking all files again",
                            )
                        } else {
                            (
                                MK_INFO,
                                b"Found archive files not processed by direct unpack, unpacking all files again",
                            )
                        };
                        self.print_message(q, &mut st, kind, txt);
                    }
                    q.host.post_clear_extracted_archives(q.job.post_ptr);
                    self.unpack_archives(q, &mut st, false, false);
                } else {
                    self.print_message(
                        q,
                        &mut st,
                        MK_INFO,
                        b"Using directly unpacked files",
                    );
                }
            }

            if st.has_seven_zip_files && st.unpack_ok {
                self.unpack_archives(q, &mut st, true, false);
            }
            if st.has_seven_zip_multi_files && st.unpack_ok {
                self.unpack_archives(q, &mut st, true, true);
            }
            if st.has_splitted_files && st.unpack_ok {
                self.join_splitted_files(q, &mut st);
            }

            self.completed(q, &mut st);
            st.joined_files.clear();
        } else if unpack {
            if st.parcheck_enabled
                && st.par_status <= NzbParStatus::Skipped
                && st.par_rename_status <= 1
                && st.has_par_files
            {
                let m = [b"Nothing to unpack for ", st.name.as_slice()].concat();
                self.print_message(q, &mut st, MK_INFO, &m);
                self.request_par_check(q, &mut st, false);
            } else if st.has_renamed_archive_files {
                let m = [
                    b"Could not unpack ",
                    st.name.as_slice(),
                    b" due to renamed archive files",
                ]
                .concat();
                self.print_message(q, &mut st, MK_ERROR, &m);
                q.set_unpack_status(UnpackStatus::Failure);
            } else {
                let m = [b"Nothing to unpack for ", st.name.as_slice()].concat();
                self.print_message(q, &mut st, MK_INFO, &m);
                q.set_unpack_status(UnpackStatus::Skipped);
            }
        } else {
            let m = [b"Unpack for ", st.name.as_slice(), b" skipped"].concat();
            self.print_message(q, &mut st, MK_INFO, &m);
            q.set_unpack_status(UnpackStatus::Skipped);
        }

        let elapsed = (current_time_sec() - start) as c_int;
        q.finish(elapsed);
    }

    fn create_unpack_dir(
        &self,
        q: JobQueue<'_>,
        st: &mut UnpackRunState,
    ) {
        let sep = crate::paths::PATH_SEPARATOR;
        st.inter_dir = crate::unpack::use_inter_dir(&st.dest_dir, &st.inter_dir_opt, sep);

        if st.inter_dir && st.final_dir.is_empty() {
            st.final_dir = q.build_final_dir_name();
            q.host.with_locked_queue(|_| q.set_final_dir(&st.final_dir));
            st.final_dir_created =
                !crate::fsops::directory_exists(&st.final_dir, &crate::fsops::posix_stat);
        }

        let base_dest = if !st.final_dir.is_empty() {
            st.final_dir.as_slice()
        } else {
            st.dest_dir.as_slice()
        };
        st.unpack_dir = if st.use_temp_unpack_dir {
            [base_dest, &[sep], b"_unpack"].concat()
        } else {
            base_dest.to_vec()
        };
        st.unpack_dir_created =
            !crate::fsops::directory_exists(&st.unpack_dir, &crate::fsops::posix_stat);

        q.log(
            MK_DETAIL,
            &[b"Unpacking into ", st.unpack_dir.as_slice()].concat(),
        );

        let mut errmsg = Vec::new();
        if !crate::fsops::force_directories(&st.unpack_dir, &mut errmsg, &crate::fsops::posix_stat)
        {
            let m = [
                b"Could not create directory ",
                st.unpack_dir.as_slice(),
                b": ",
                errmsg.as_slice(),
            ]
            .concat();
            self.print_message(q, st, MK_ERROR, &m);
        }
    }

    fn unpack_archives(
        &self,
        q: JobQueue<'_>,
        st: &mut UnpackRunState,
        is_seven_zip: bool,
        multi_volumes: bool,
    ) {
        if !st.unpack_tried || st.par_repaired {
            let pw = st.password.clone();
            self.execute_unpack(q, st, is_seven_zip, &pw, multi_volumes);
            if !st.unpack_ok
                && st.has_par_files
                && !st.unpack_password_error
                && st.par_status <= NzbParStatus::Skipped
            {
                q.log(
                    MK_DEBUG,
                    b"For rar4- or 7z-archives try par-check first, before trying password file",
                );
                return;
            }
        } else {
            st.unpack_ok = false;
            st.unpack_decrypt_error = st.last_unpack_status == UnpackStatus::Password;
        }

        if !st.unpack_ok
            && !st.unpack_start_error
            && !st.unpack_space_error
            && (st.unpack_decrypt_error || st.unpack_password_error)
            && (!self.terminated.load(Ordering::Acquire) || st.auto_terminated)
            && st.password.is_empty()
            && !st.unpack_pass_file.is_empty()
        {
            let pass_file = st.unpack_pass_file.clone();
            let state_cell = std::cell::RefCell::new(&mut *st);
            let opened = crate::unpack::try_passwords_from_file(
                crate::fsops::path(&pass_file),
                || {
                    let s = state_cell.borrow();
                    !s.unpack_ok
                        && !s.unpack_start_error
                        && !s.unpack_space_error
                        && (s.unpack_decrypt_error || s.unpack_password_error)
                },
                |pw| {
                    let mut s = state_cell.borrow_mut();
                    q.log(MK_DEBUG, &[b"Password line: ", pw].concat());
                    if q.is_stopped() && s.auto_terminated {
                        self.terminated.store(false, Ordering::Release);
                        q.host.thread_resume(q.job.thread_ptr);
                    }
                    s.unpack_decrypt_error = false;
                    s.unpack_password_error = false;
                    s.auto_terminated = false;
                    let m = [b"Trying password ", pw, b" for ", s.name.as_slice()].concat();
                    self.print_message(q, &mut s, MK_INFO, &m);
                    self.execute_unpack(q, &mut s, is_seven_zip, pw, multi_volumes);
                },
            );

            if !opened {
                let m = [b"Could not open file ", pass_file.as_slice()].concat();
                self.print_message(q, st, MK_ERROR, &m);
                return;
            }

            st.pass_list_tried = !q.is_stopped() || st.auto_terminated;
        }
    }

    fn execute_unpack(
        &self,
        q: JobQueue<'_>,
        st: &mut UnpackRunState,
        is_seven_zip: bool,
        password: &[u8],
        multi_volumes: bool,
    ) {
        st.all_ok_received = false;
        st.unpack_space_error = false;
        st.unpack_limit_error = false;
        st.is_seven_zip = is_seven_zip;

        if is_seven_zip {
            self.print_message(q, st, MK_INFO, b"Executing 7-Zip");
            st.log_prefix = Some(b"7-Zip");
        }
        q.set_progress_label(b"");

        let _unpack_guard = UNPACK_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        self.exec_thread_key
            .store(current_thread_key(), Ordering::Release);
        self.unpacking.store(true, Ordering::Release);

        let dest_dir = st.dest_dir.clone();
        let unpack_dir = st.unpack_dir.clone();
        let dl_snapshot = st.downloaded_files.clone();
        let dl_refs: Vec<&[u8]> = dl_snapshot.iter().map(Vec::as_slice).collect();
        let ex_snapshot = st.extracted_archives.clone();
        let ex_refs: Vec<&[u8]> = ex_snapshot.iter().map(Vec::as_slice).collect();
        let use_temp = st.use_temp_unpack_dir;
        let cleanup_disk = st.unpack_cleanup_disk;
        let pw_str = std::str::from_utf8(password).unwrap_or("");

        // Unpacked files may end up in the download or the final folder.
        let guard = st
            .unpack_guard
            .get_or_insert_with(|| {
                crate::unpacklimits::Guard::shared(
                    crate::fsops::path(&unpack_dir),
                    &[
                        crate::fsops::path(&st.dest_dir),
                        crate::fsops::path(&st.final_dir),
                    ],
                )
            })
            .clone();
        let st_cell = std::cell::RefCell::new(&mut *st);
        let all_ok = crate::unpack::extract_discovered_archives(
            &dest_dir,
            crate::fsops::path(&unpack_dir),
            &guard,
            pw_str,
            is_seven_zip,
            multi_volumes,
            use_temp,
            cleanup_disk,
            &dl_refs,
            &ex_refs,
            &mut || st_cell.borrow_mut().all_ok_received = false,
            &mut || st_cell.borrow().all_ok_received,
            &mut |kind, msg| {
                let msg_kind = match kind {
                    0 | 4 => MK_INFO,
                    1 => MK_ERROR,
                    2 => MK_WARNING,
                    _ => MK_DETAIL,
                };
                self.add_message(
                    q,
                    &mut st_cell.borrow_mut(),
                    msg_kind,
                    msg.as_bytes(),
                );
            },
            &mut || self.is_stopped_or_terminated(q),
        );

        self.unpacking.store(false, Ordering::Release);
        self.exec_thread_key.store(0, Ordering::Release);
        drop(_unpack_guard);

        st.log_prefix = None;
        q.set_progress_label(b"");

        st.unpack_ok = all_ok
            && st.all_ok_received
            && !self.terminated.load(Ordering::Acquire)
            && !q.is_stopped();
        st.unpack_start_error = false;
    }

    fn join_splitted_files(
        &self,
        q: JobQueue<'_>,
        st: &mut UnpackRunState,
    ) {
        st.log_prefix = Some(b"Join");
        q.set_progress_label(b"");
        q.set_stage_progress(0);

        let dest_dir = st.dest_dir.clone();
        let unpack_dir = st.unpack_dir.clone();
        let par_success = st.par_status == NzbParStatus::Success;
        let write_buf = st.write_buffer_kb;
        let mut joined = std::mem::take(&mut st.joined_files);
        let guard = st.unpack_guard.clone();
        let _scope = guard.as_ref().map(crate::unpacklimits::Scope::enter);
        let ex_snapshot = st.extracted_archives.clone();
        let ex_refs: Vec<&[u8]> = ex_snapshot.iter().map(Vec::as_slice).collect();

        let ok = crate::unpack::join_splitted_files(
            &dest_dir,
            &unpack_dir,
            par_success,
            write_buf,
            &ex_refs,
            &mut joined,
            &mut |kind, msg| {
                let msg_kind = match kind {
                    0 => MK_INFO,
                    1 => MK_ERROR,
                    2 => MK_WARNING,
                    _ => MK_DETAIL,
                };
                self.print_message(q, st, msg_kind, msg);
            },
            &mut |label, stage_progress| {
                if let Some(lbl) = label {
                    q.set_progress_label(lbl);
                }
                if stage_progress >= 0 {
                    q.set_stage_progress(stage_progress);
                }
            },
        );
        st.joined_files = joined;
        // A joined file named like an archive volume is output: count it now.
        if ok {
            if let Some(g) = &guard {
                if let Err(why) = crate::unpacklimits::settle(g) {
                    self.print_message(q, st, MK_ERROR, why.as_bytes());
                }
            }
        }
        let ok = ok && guard.as_ref().is_none_or(|g| crate::unpacklimits::breach_of(g).is_none());
        if !ok {
            st.unpack_ok = false;
            if guard.as_ref().is_some_and(|g| crate::unpacklimits::breach_of(g).is_some()) {
                st.unpack_limit_error = true;
            }
        }
        st.log_prefix = None;
        q.set_progress_label(b"");
    }

    fn cleanup(
        &self,
        q: JobQueue<'_>,
        st: &mut UnpackRunState,
    ) -> bool {
        let dest_dir = st.dest_dir.clone();
        let final_dir = st.final_dir.clone();
        let unpack_dir = st.unpack_dir.clone();
        let dl_snapshot = st.downloaded_files.clone();
        let jn_snapshot = st.joined_files.clone();
        let dl_refs: Vec<&[u8]> = dl_snapshot.iter().map(Vec::as_slice).collect();
        let jn_refs: Vec<&[u8]> = jn_snapshot.iter().map(Vec::as_slice).collect();
        let ex_snapshot = st.extracted_archives.clone();
        let ex_refs: Vec<&[u8]> = ex_snapshot.iter().map(Vec::as_slice).collect();
        let unpack_ok = st.unpack_ok;
        let use_temp = st.use_temp_unpack_dir;
        let fin_created = st.final_dir_created;
        let cleanup_disk = st.unpack_cleanup_disk;
        let inter_dir = st.inter_dir;
        let has_split = st.has_splitted_files;
        let umask = st.umask;

        let (ok, cleaned) = crate::unpack::unpack_controller_cleanup(
            &dest_dir,
            &final_dir,
            &unpack_dir,
            unpack_ok,
            use_temp,
            fin_created,
            cleanup_disk,
            inter_dir,
            has_split,
            umask,
            &dl_refs,
            &jn_refs,
            &ex_refs,
            &mut |kind, msg| {
                let msg_kind = match kind {
                    0 => MK_INFO,
                    1 => MK_ERROR,
                    2 => MK_WARNING,
                    _ => MK_DETAIL,
                };
                self.print_message(q, st, msg_kind, msg);
            },
        );
        st.cleaned_up_disk = cleaned;
        ok
    }

    fn completed(
        &self,
        q: JobQueue<'_>,
        st: &mut UnpackRunState,
    ) {
        let deleted = q.is_deleted();
        if !st.auto_terminated && !deleted && self.is_stopped_or_terminated(q) {
            q.log(
                MK_DEBUG,
                b"Unpack stopped due to reload/shutdown, preserving _unpack directory",
            );
            return;
        }

        // Every file the job unpacked is counted (archive volumes held back included), then
        // names are made ASCII: once every unpack pass of the job is done.
        if st.unpack_ok {
            if let Some(guard) = st.unpack_guard.clone() {
                if let Err(why) = crate::unpacklimits::settle_job(&guard) {
                    self.print_message(q, st, MK_ERROR, why.as_bytes());
                    st.unpack_ok = false;
                    st.unpack_limit_error = true;
                }
            }
        }
        if st.unpack_ok {
            if let Some(guard) = st.unpack_guard.clone() {
                let mut lines: Vec<(i32, String)> = Vec::new();
                let named = crate::unpacklimits::finish_names(&guard, &mut |k, m| {
                    lines.push((k, m.to_string()));
                });
                for (k, m) in lines {
                    let kind = if k == 1 { MK_ERROR } else { MK_INFO };
                    self.print_message(q, st, kind, m.as_bytes());
                }
                if named.is_err() {
                    // A file system error: a par repair cannot help.
                    st.unpack_ok = false;
                    st.unpack_start_error = true;
                }
            }
        }

        let cleanup_success = self.cleanup(q, st);
        if st.unpack_ok && cleanup_success {
            let m = [st.info_name_up.as_slice(), b" successful"].concat();
            self.print_message(q, st, MK_INFO, &m);
            q.unpack_succeeded(st.cleaned_up_disk, st.par_rename_opt);
        } else {
            let (par_status, par_full) = q.par_info();
            let (req_par, force_rep, unpack_status) = crate::unpack::eval_unpack_completed_failure(
                st.parcheck_enabled,
                st.unpack_ok,
                par_status,
                par_full,
                st.unpack_start_error || st.unpack_limit_error,
                st.unpack_space_error,
                st.unpack_password_error,
                st.unpack_decrypt_error,
                self.terminated.load(Ordering::Acquire),
                q.is_stopped(),
                st.auto_terminated,
                st.has_par_files,
                !st.password.is_empty(),
                !st.unpack_pass_file.is_empty(),
                st.pass_list_tried,
            );
            if st.parcheck_enabled && req_par {
                self.request_par_check(q, st, force_rep);
            } else {
                let m = [st.info_name_up.as_slice(), b" failed"].concat();
                self.print_message(q, st, MK_ERROR, &m);
                q.set_unpack_status(unpack_status);
            }
        }
    }

    fn request_par_check(
        &self,
        q: JobQueue<'_>,
        st: &mut UnpackRunState,
        force_repair: bool,
    ) {
        let suffix: &[u8] = if force_repair {
            b" requested par-check with forced repair"
        } else {
            b" requested par-check/repair"
        };
        let m = [st.info_name_up.as_slice(), suffix].concat();
        self.print_message(q, st, MK_INFO, &m);
        let last_status = if st.unpack_space_error {
            UnpackStatus::Space
        } else if st.unpack_password_error || st.unpack_decrypt_error {
            UnpackStatus::Password
        } else {
            UnpackStatus::Failure
        };
        q.request_par_check(force_repair, st.pass_list_tried, last_status);
    }
}

// ============================================================================
// 3. DirectUnpack (`daemon/postprocess/DirectUnpack.cpp`)
// ============================================================================

// ---------------------------------------------------------------------------
// Direct-unpack holds: a damaged archive volume pauses extraction instead of
// cancelling it. The volume is released once it has been repaired (borrowed
// articles, stream repair from a duplicate, or par-repair), and extraction
// continues from where it stopped.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct DirectUnpackHoldState {
    /// Full paths of damaged volumes extraction must not read yet.
    held: Vec<Vec<u8>>,
    /// File names released since the unpack thread last looked.
    released: Vec<Vec<u8>>,
    /// The download finished with volumes still held: post-processing repairs first.
    awaiting_repair: bool,
    /// The repair failed: the unpack thread gives up.
    abandoned: bool,
    /// A live par2 repair of the held volumes is running on its helper thread.
    live_repair_running: bool,
    live_repair_generation: Option<crate::parlive::SessionGeneration>,
    /// Lost byte ranges of held volumes, by full path. A held volume with known ranges is
    /// handed to the extraction session with those ranges as holes instead of pausing it.
    holes: std::collections::HashMap<Vec<u8>, Vec<(u64, u64)>>,
    /// The download finished: a volume that is not on disk now never will be.
    download_finished: bool,
    /// Dead volumes rebuilt from a duplicate (see `deadvolume`), by full path: the extraction
    /// reads these instead of the missing files.
    virtuals: std::collections::HashMap<Vec<u8>, crate::deadvolume::VirtualVolume>,
    /// Dead volumes handed over before their borrow finished, by full path: rebuilt like
    /// `virtuals`, with the bytes not borrowed yet as holes. They stay held until the borrow
    /// completes and `direct_unpack_set_virtual` replaces them.
    offered: std::collections::HashMap<Vec<u8>, crate::deadvolume::VirtualVolume>,
}

/// What direct unpack's extraction session sees of a download in progress: a volume is ready
/// once it is complete on disk, a damaged one comes with its lost ranges as holes, one that has
/// not arrived parks the session until it does.
struct DirectUnpackFeed {
    nzb_id: i32,
    /// RAR files the download will contain (queued plus completed).
    expected_volumes: u64,
}

impl crate::progressive_unpack::VolumeFeed for DirectUnpackFeed {
    fn volume(&mut self, path: &std::path::Path) -> crate::progressive_unpack::VolumeView {
        use crate::progressive_unpack::VolumeView;
        #[cfg(unix)]
        let full = {
            use std::os::unix::ffi::OsStrExt;
            path.as_os_str().as_bytes().to_vec()
        };
        #[cfg(not(unix))]
        let full = path.to_string_lossy().as_bytes().to_vec();
        if let Some(v) = direct_unpack_virtual(self.nzb_id, &full) {
            return VolumeView::Virtual(v);
        }
        match direct_unpack_hole_view(&full) {
            Some(holes) if !holes.is_empty() => VolumeView::Ready { holes },
            Some(_) => VolumeView::NotYet,
            None if path.is_file() => VolumeView::Ready { holes: Vec::new() },
            // With dead files still on hold this volume may be one of them, borrowed later.
            None if direct_unpack_abandoned(self.nzb_id)
                || (direct_unpack_download_finished(self.nzb_id)
                    && !direct_unpack_has_holds(self.nzb_id)) =>
            {
                VolumeView::Missing
            }
            None => VolumeView::NotYet,
        }
    }

    fn expected_volumes(&self) -> u64 {
        self.expected_volumes
    }

    fn has_borrowed(&self) -> bool {
        direct_unpack_holds()
            .get(&self.nzb_id)
            .is_some_and(|st| !st.virtuals.is_empty() || !st.offered.is_empty())
    }

    fn download_finished(&self) -> bool {
        direct_unpack_download_finished(self.nzb_id)
    }

    fn set_became_final(&self) -> bool {
        direct_unpack_download_finished(self.nzb_id)
            && !direct_unpack_has_holds(self.nzb_id)
            && !direct_unpack_abandoned(self.nzb_id)
    }

    fn remember_damaged(&mut self, path: &std::path::Path, holes: Vec<(u64, u64)>) {
        #[cfg(unix)]
        let full = {
            use std::os::unix::ffi::OsStrExt;
            path.as_os_str().as_bytes().to_vec()
        };
        #[cfg(not(unix))]
        let full = path.to_string_lossy().as_bytes().to_vec();
        direct_unpack_rehold(self.nzb_id, &full, holes);
    }

    fn still_held(&self, path: &std::path::Path) -> bool {
        #[cfg(unix)]
        let full = {
            use std::os::unix::ffi::OsStrExt;
            path.as_os_str().as_bytes().to_vec()
        };
        #[cfg(not(unix))]
        let full = path.to_string_lossy().as_bytes().to_vec();
        direct_unpack_holds()
            .get(&self.nzb_id)
            .is_some_and(|st| st.held.iter().any(|p| p == &full))
    }

    fn repaired(&self, path: &std::path::Path) -> bool {
        #[cfg(unix)]
        let full = {
            use std::os::unix::ffi::OsStrExt;
            path.as_os_str().as_bytes().to_vec()
        };
        #[cfg(not(unix))]
        let full = path.to_string_lossy().as_bytes().to_vec();
        // Asked only of volumes handed out with holes, which were held then. Released by its
        // repair: no longer held, and the holds were not given up (an abandon clears every
        // hold without repairing anything).
        direct_unpack_holds()
            .get(&self.nzb_id)
            .is_some_and(|st| !st.abandoned && !st.held.iter().any(|p| p == &full))
    }

    fn wait_repaired(&mut self, is_cancelled: &mut dyn FnMut() -> bool) -> bool {
        loop {
            if direct_unpack_abandoned(self.nzb_id) || is_cancelled() {
                return false;
            }
            if !direct_unpack_has_holds(self.nzb_id) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

static DIRECT_UNPACK_HOLDS: std::sync::LazyLock<
    Mutex<std::collections::HashMap<i32, DirectUnpackHoldState>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

fn direct_unpack_holds(
) -> std::sync::MutexGuard<'static, std::collections::HashMap<i32, DirectUnpackHoldState>> {
    DIRECT_UNPACK_HOLDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn hold_path_filename(path: &[u8]) -> &[u8] {
    match path
        .iter()
        .rposition(|&b| b == crate::paths::PATH_SEPARATOR)
    {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// Marks `full_path` (a damaged volume of download `nzb_id`) as not readable by direct unpack.
pub(crate) fn direct_unpack_hold(nzb_id: i32, full_path: &[u8]) {
    let mut map = direct_unpack_holds();
    let st = map.entry(nzb_id).or_default();
    if !st.held.iter().any(|p| p == full_path) {
        st.held.push(full_path.to_vec());
    }
}

/// Holds `full_path` and records its lost byte ranges (`offset`, `len`), so extraction can run
/// past them while the volume is repaired.
pub(crate) fn direct_unpack_hold_with_holes(nzb_id: i32, full_path: &[u8], holes: Vec<(u64, u64)>) {
    let mut map = direct_unpack_holds();
    let st = map.entry(nzb_id).or_default();
    if !st.held.iter().any(|p| p == full_path) {
        st.held.push(full_path.to_vec());
    }
    if holes.is_empty() {
        st.holes.remove(full_path);
    } else {
        st.holes.insert(full_path.to_vec(), holes);
    }
}

/// Replaces the lost ranges of a volume still on hold with `holes`; false when the hold is gone
/// (a repair released it, or the download ended), so nothing is held again.
pub(crate) fn direct_unpack_widen_hold(nzb_id: i32, full_path: &[u8], holes: Vec<(u64, u64)>) -> bool {
    let mut map = direct_unpack_holds();
    let Some(st) = map.get_mut(&nzb_id) else {
        return false;
    };
    if !st.held.iter().any(|p| p == full_path) {
        return false;
    }
    if holes.is_empty() {
        st.holes.remove(full_path);
    } else {
        st.holes.insert(full_path.to_vec(), holes);
        // The unpack thread picks this up like a release: a wait on it ends, a first volume
        // is queued for extraction.
        st.released.push(hold_path_filename(full_path).to_vec());
    }
    true
}

/// Holds `full_path` again with its lost ranges after a restart, unless the download is over or
/// abandoned: nothing would release a new hold then (post-processing only waits for holds the
/// download left), so the refill reads the volume as it is. One lock, so `NzbDownloaded` sees
/// either no hold or this one.
fn direct_unpack_rehold(nzb_id: i32, full_path: &[u8], holes: Vec<(u64, u64)>) -> bool {
    let mut map = direct_unpack_holds();
    let st = map.entry(nzb_id).or_default();
    if st.download_finished || st.abandoned || holes.is_empty() {
        return false;
    }
    if !st.held.iter().any(|p| p == full_path) {
        st.held.push(full_path.to_vec());
    }
    st.holes.insert(full_path.to_vec(), holes);
    true
}

/// `None` when `full_path` is not on hold; otherwise its recorded lost ranges (empty when they
/// are not known, in which case extraction waits for the repair).
pub(crate) fn direct_unpack_hole_view(full_path: &[u8]) -> Option<Vec<(u64, u64)>> {
    let map = direct_unpack_holds();
    map.values().find_map(|st| {
        st.held
            .iter()
            .any(|p| p == full_path)
            .then(|| st.holes.get(full_path).cloned().unwrap_or_default())
    })
}

fn direct_unpack_mark_downloaded(nzb_id: i32) {
    direct_unpack_holds()
        .entry(nzb_id)
        .or_default()
        .download_finished = true;
}

fn direct_unpack_download_finished(nzb_id: i32) -> bool {
    direct_unpack_holds()
        .get(&nzb_id)
        .is_some_and(|st| st.download_finished)
}

fn direct_unpack_abandoned(nzb_id: i32) -> bool {
    direct_unpack_holds()
        .get(&nzb_id)
        .is_some_and(|st| st.abandoned)
}

/// True while `full_path` is a damaged volume waiting for its repair.
pub(crate) fn direct_unpack_is_held_path(full_path: &[u8]) -> bool {
    direct_unpack_holds()
        .values()
        .any(|st| st.held.iter().any(|p| p == full_path))
}

/// True when download `nzb_id` has volumes on hold.
pub(crate) fn direct_unpack_has_holds(nzb_id: i32) -> bool {
    direct_unpack_holds()
        .get(&nzb_id)
        .is_some_and(|st| !st.held.is_empty())
}

/// The rebuilt stand-in for dead volume `full_path` of download `nzb_id`, if one was handed over.
pub(crate) fn direct_unpack_virtual(nzb_id: i32, full_path: &[u8]) -> Option<crate::deadvolume::VirtualVolume> {
    direct_unpack_holds()
        .get(&nzb_id)
        .and_then(|st| {
            // Names may differ in case between the extractor and the dead job (see set_virtual).
            st.virtuals.get(full_path).or_else(|| {
                st.virtuals.iter().find(|(k, _)| k.eq_ignore_ascii_case(full_path)).map(|(_, v)| v)
            })
            // One handed over before its borrow finished, with holes.
            .or_else(|| case_blind(&st.offered, full_path))
            .cloned()
        })
}

/// True when every file direct unpack of `nzb_id` still holds is a file that never reached the
/// disk under a name that is not a RAR volume's (an obfuscated stand-in held in case it was a
/// volume). Releasing those is safe: a volume the extraction really needs then shows as missing
/// and the extraction fails as it would have, while one it never needed lets it finish.
pub(crate) fn direct_unpack_holds_only_absent_stand_ins(nzb_id: i32) -> bool {
    direct_unpack_holds().get(&nzb_id).is_some_and(|st| {
        !st.held.is_empty()
            && st.held.iter().all(|p| {
                !crate::filetypes::is_rar_file(hold_path_filename(p))
                    && !crate::fsops::path(p).exists()
            })
    })
}

/// Downloads that had dead volumes rebuilt from a duplicate, with how many. Kept past the end
/// of direct unpack (which forgets its holds) so post-processing can tell why par-check failed.
fn dead_volumes_borrowed(
) -> std::sync::MutexGuard<'static, std::collections::HashMap<i32, usize>> {
    static MAP: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<i32, usize>>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Downloads whose dead volumes were placed in a duplicate's video by a repair still running.
fn dead_volume_borrows() -> std::sync::MutexGuard<'static, std::collections::HashSet<i32>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<i32>>> =
        std::sync::OnceLock::new();
    SET.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// What live repair runs found of each donor's video, by (download, donor's queued nzb file),
/// while dead volumes waited for it: `Err(n)`, the runs in which it did not match;
/// `Ok(())` once it matched (see [`note_dead_volume_donor_identity`]).
fn dead_volume_donor_identities() -> std::sync::MutexGuard<
    'static,
    std::collections::HashMap<(i32, Vec<u8>), Result<(), u32>>,
> {
    static MAP: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<(i32, Vec<u8>), Result<(), u32>>>,
    > = std::sync::OnceLock::new();
    MAP.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Runs in which a donor must fail the identity check before it counts as unable to lend. Only
/// a check that compared in full counts (`streamrepair::DonorCheck::Differs`: donor bytes that
/// arrived whole against target bytes on disk, or a payload of another size); one that could not
/// run in full counts for nothing and is retried by a later run.
pub(crate) const DEAD_VOLUME_DONOR_MISMATCH_RUNS: u32 = 2;

/// Records what a live repair run of download `nzb_id` found of the video in the donor whose
/// queued nzb file is `donor` while dead volumes waited for it: `confirmed` is a check that
/// matched, otherwise one that compared in full and differed (inconclusive checks are not
/// recorded at all). A match is kept for good. Before any match, mismatches are counted.
pub(crate) fn note_dead_volume_donor_identity(nzb_id: i32, donor: &[u8], confirmed: bool) {
    let mut map = dead_volume_donor_identities();
    let entry = map.entry((nzb_id, donor.to_vec())).or_insert(Err(0));
    if confirmed {
        *entry = Ok(());
    } else if let Err(n) = entry {
        *n += 1;
    }
}

/// True when the donor whose queued nzb file is `donor` failed the identity check of
/// download `nzb_id`'s dead volumes in [`DEAD_VOLUME_DONOR_MISMATCH_RUNS`] runs and never
/// passed it: it holds another video and cannot lend them.
pub(crate) fn dead_volume_donor_mismatched(nzb_id: i32, donor: &[u8]) -> bool {
    !donor.is_empty()
        && dead_volume_donor_identities()
            .get(&(nzb_id, donor.to_vec()))
            .is_some_and(|r| matches!(r, Err(n) if *n >= DEAD_VOLUME_DONOR_MISMATCH_RUNS))
}

/// (download, donor NZBID) pairs whose video a live repair run checked against the download's
/// own bytes and found the same, with no check of that run finding it different (see
/// [`dead_volume_donor_confirmed`]).
fn dead_volume_donor_confirmations(
) -> std::sync::MutexGuard<'static, std::collections::HashMap<(i32, i32), i64>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<(i32, i32), i64>>> =
        std::sync::OnceLock::new();
    SET.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Records that donor `donor_id`'s video matched download `nzb_id`'s video of `payload_size`
/// bytes (see [`dead_volume_donor_confirmed`]).
pub(crate) fn note_dead_volume_donor_confirmed(nzb_id: i32, donor_id: i32, payload_size: i64) {
    if donor_id > 0 {
        dead_volume_donor_confirmations().insert((nzb_id, donor_id), payload_size);
    }
}

/// Forgets that donor `donor_id`'s video matched download `nzb_id`'s: a later check compared it
/// in full and found it different.
pub(crate) fn forget_dead_volume_donor_confirmed(nzb_id: i32, donor_id: i32) {
    dead_volume_donor_confirmations().remove(&(nzb_id, donor_id));
}

/// True when the video of donor `donor_id` matched the one of download `nzb_id` in an earlier
/// live repair run, by a check that compared in full, and no check of that run found it
/// different. A later dead volume then has none of the download's own bytes around it to
/// compare (the primary died for good), and the donor lends it still (failover-continue).
/// Keyed by NZBID, which no later copy reuses.
/// Returns the payload size the match was for.
pub(crate) fn dead_volume_donor_confirmed(nzb_id: i32, donor_id: i32) -> Option<i64> {
    (donor_id > 0)
        .then(|| dead_volume_donor_confirmations().get(&(nzb_id, donor_id)).copied())
        .flatten()
}

/// True when some donor's video matched the one of download `nzb_id` while dead volumes waited
/// for it: that donor lends them.
pub(crate) fn dead_volume_any_donor_confirmed(nzb_id: i32) -> bool {
    dead_volume_donor_identities()
        .iter()
        .any(|((id, _), r)| *id == nzb_id && r.is_ok())
}

/// True when the donor whose queued nzb file is `donor` cannot lend the dead volumes of
/// download `nzb_id` (see [`dead_volume_donor_mismatched`]) and no other donor lends them
/// either: none matched, and direct unpack holds no volume rebuilt from one. (A repair run
/// under way says nothing of that: every run marks a borrow under way, also one that only
/// checks the mismatched donor again, and runs follow each other closely.)
pub(crate) fn dead_volume_donor_cannot_lend(nzb_id: i32, donor: &[u8]) -> bool {
    dead_volume_donor_mismatched(nzb_id, donor)
        && !dead_volume_rebuilt(nzb_id)
        && !dead_volume_any_donor_confirmed(nzb_id)
}

/// True once direct unpack of download `nzb_id` holds a dead volume rebuilt from a duplicate.
fn dead_volume_rebuilt(nzb_id: i32) -> bool {
    // A volume handed over before its borrow finished proves nothing of the donor: it is
    // offered before any donor is read.
    direct_unpack_holds()
        .get(&nzb_id)
        .is_some_and(|st| !st.virtuals.is_empty())
}

/// Forgets what was found of the donors of download `nzb_id` and whether the queue kept it for
/// them: it left the queue, or came back to it from history to start over.
pub(crate) fn forget_dead_volume_verdicts(nzb_id: i32) {
    crate::restartlend::forget(nzb_id);
    crate::frontiercut::forget(nzb_id);
    dead_volume_donor_identities().retain(|(id, _), _| *id != nzb_id);
    dead_volume_donor_confirmations().retain(|(id, _), _| *id != nzb_id);
    crate::queuecoord::forget_kept_for_borrowing(nzb_id);
    crate::streamrepair_coord::expire_batched();
}

/// Forgets what was put back into the volumes of download `nzb_id` (dead volumes read
/// borrowed, volume ends rebuilt): it left the queue, or starts over, and a fresh attempt must
/// not skip par-check over an earlier one's repairs.
pub(crate) fn forget_put_back(nzb_id: i32) {
    let _ = take_dead_volumes_borrowed(nzb_id);
    let _ = take_volume_end_rebuilt(nzb_id);
}

/// Marks (or clears) a borrow of dead volumes of download `nzb_id` in progress.
pub(crate) fn note_dead_volume_borrow(nzb_id: i32, underway: bool) {
    if underway {
        dead_volume_borrows().insert(nzb_id);
    } else {
        dead_volume_borrows().remove(&nzb_id);
    }
}

fn locked_archives() -> std::sync::MutexGuard<'static, std::collections::HashSet<i32>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<i32>>> =
        std::sync::OnceLock::new();
    SET.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Whether direct unpack of download `nzb_id` stopped because its archive needs a password
/// that neither its nzb nor `UnpackPassFile` can give (encrypted headers, say): the download
/// can never be extracted, so the queue fails it over now instead of downloading it whole.
/// Answered once: the mark is cleared as it is read.
pub(crate) fn take_direct_unpack_locked(nzb_id: i32) -> bool {
    locked_archives().remove(&nzb_id)
}

/// Whether a direct unpack failure means the download can never be extracted.
fn failure_locks_archive(
    failure: &crate::progressive_unpack::Failure,
    password: &[u8],
    pass_file_set: bool,
) -> bool {
    *failure == crate::progressive_unpack::Failure::PasswordRequired
        && password.is_empty()
        && !pass_file_set
}

/// True while dead volumes of download `nzb_id` are being borrowed from a duplicate, or once
/// direct unpack reads one rebuilt: replacing the download would throw that away.
pub(crate) fn dead_volume_borrow_underway(nzb_id: i32) -> bool {
    dead_volume_borrows().contains(&nzb_id)
        || direct_unpack_holds()
            .get(&nzb_id)
            .is_some_and(|st| !st.virtuals.is_empty() || !st.offered.is_empty())
}

/// How many dead volumes of download `nzb_id` were rebuilt from a duplicate (0 if none), once:
/// the count is forgotten as it is read.
pub(crate) fn take_dead_volumes_borrowed(nzb_id: i32) -> usize {
    dead_volumes_borrowed().remove(&nzb_id).unwrap_or(0)
}

/// Downloads with a RAR volume end rebuilt from a sibling volume (see
/// `deadvolume::restore_tail_from_sibling`): par2 sees the rebuilt bytes as damage when the
/// sibling's quick-open record differs, though the extraction read them fine.
fn volume_ends_rebuilt() -> std::sync::MutexGuard<'static, std::collections::HashSet<i32>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<i32>>> =
        std::sync::OnceLock::new();
    SET.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Notes that a volume end of download `nzb_id` was rebuilt from a sibling volume.
pub(crate) fn note_volume_end_rebuilt(nzb_id: i32) {
    volume_ends_rebuilt().insert(nzb_id);
}

/// True when a volume end of download `nzb_id` was rebuilt from a sibling volume, once: it is
/// forgotten as it is read.
pub(crate) fn take_volume_end_rebuilt(nzb_id: i32) -> bool {
    volume_ends_rebuilt().remove(&nzb_id)
}

/// True when a volume end of download `nzb_id` was rebuilt from a sibling volume, without
/// forgetting it (see [`take_volume_end_rebuilt`]).
pub(crate) fn volume_end_was_rebuilt(nzb_id: i32) -> bool {
    volume_ends_rebuilt().contains(&nzb_id)
}

/// How many dead volumes of download `nzb_id` direct unpack read borrowed, without forgetting
/// it (see [`take_dead_volumes_borrowed`]).
pub(crate) fn dead_volumes_borrowed_count(nzb_id: i32) -> usize {
    dead_volumes_borrowed().get(&nzb_id).copied().unwrap_or(0)
}

/// True while direct unpack of download `nzb_id` waits on a volume of which nothing is on disk
/// and that has no rebuilt stand-in yet: a later live repair may be able to place and borrow it
/// once the volumes before it have arrived.
pub(crate) fn direct_unpack_waits_on_dead_volume(nzb_id: i32) -> bool {
    let candidates: Vec<Vec<u8>> = direct_unpack_holds().get(&nzb_id).map_or_else(Vec::new, |st| {
        st.held
            .iter()
            // A volume handed over with holes still waits for the rest of its borrow.
            .filter(|p| !st.virtuals.contains_key(*p))
            .cloned()
            .collect()
    });
    // The disk is looked at after the holds lock is released.
    candidates
        .iter()
        .any(|p| !std::fs::metadata(crate::fsops::path(p)).is_ok_and(|m| m.len() > 0))
}

fn dead_volume_reruns() -> std::sync::MutexGuard<'static, std::collections::HashSet<i32>> {
    static RERUNS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<i32>>> =
        std::sync::OnceLock::new();
    RERUNS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Asks the running live repair of download `nzb_id` for one more pass: a volume arrived while
/// it ran, which may let a dead volume be placed.
pub(crate) fn request_dead_volume_rerun(nzb_id: i32) {
    dead_volume_reruns().insert(nzb_id);
}

/// True (once) when a pass was asked for since the last call.
pub(crate) fn take_dead_volume_rerun(nzb_id: i32) -> bool {
    dead_volume_reruns().remove(&nzb_id)
}

/// True when dead volume `full_path` already has a rebuilt stand-in.
pub(crate) fn direct_unpack_has_virtual(full_path: &[u8]) -> bool {
    direct_unpack_holds()
        .values()
        .any(|st| st.virtuals.keys().any(|k| k.eq_ignore_ascii_case(full_path)))
}

fn case_blind<'a>(
    map: &'a std::collections::HashMap<Vec<u8>, crate::deadvolume::VirtualVolume>,
    full_path: &[u8],
) -> Option<&'a crate::deadvolume::VirtualVolume> {
    map.get(full_path).or_else(|| {
        map.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(full_path))
            .map(|(_, v)| v)
    })
}

/// The stand-in for dead volume `full_path` of download `nzb_id` handed over before its borrow
/// finished (see [`direct_unpack_offer_virtual`]), if any.
#[cfg(test)]
pub(crate) fn direct_unpack_offered(
    nzb_id: i32,
    full_path: &[u8],
) -> Option<crate::deadvolume::VirtualVolume> {
    direct_unpack_holds()
        .get(&nzb_id)
        .and_then(|st| case_blind(&st.offered, full_path).cloned())
}

/// Hands direct unpack of download `nzb_id` a rebuilt stand-in for dead volume `full_path`
/// whose borrow has not finished: its `holes` are read as holes, so extraction goes on past
/// them instead of waiting at the volume. The volume stays held: the holes are patched into
/// the output once [`direct_unpack_set_virtual`] hands over the finished volume. Ignored once
/// a finished stand-in is there.
pub(crate) fn direct_unpack_offer_virtual(
    nzb_id: i32,
    full_path: &[u8],
    volume: crate::deadvolume::VirtualVolume,
) {
    let mut map = direct_unpack_holds();
    let st = map.entry(nzb_id).or_default();
    if case_blind(&st.virtuals, full_path).is_some() {
        return;
    }
    st.offered
        .retain(|k, _| !k.eq_ignore_ascii_case(full_path));
    st.offered.insert(full_path.to_vec(), volume);
}

/// Hands direct unpack of download `nzb_id` a rebuilt stand-in for dead volume `full_path`,
/// releasing the volume from hold when one was set for it (a dead file with an obfuscated name
/// is held under that name instead, see below).
pub(crate) fn direct_unpack_set_virtual(
    nzb_id: i32,
    full_path: &[u8],
    volume: crate::deadvolume::VirtualVolume,
) {
    let mut map = direct_unpack_holds();
    let id = nzb_id;
    let st = map.entry(id).or_default();
    st.offered
        .retain(|k, _| !k.eq_ignore_ascii_case(full_path));
    st.virtuals.insert(full_path.to_vec(), volume);
    // The extractor derives volume names from part 1's spelling, a dead job keeps its posted
    // spelling: match the hold case-blind.
    if let Some(i) = st.held.iter().position(|p| p.eq_ignore_ascii_case(full_path)) {
        let held = st.held.remove(i);
        st.holes.remove(&held);
    } else if let Some(i) = st.held.iter().position(|p| {
        // A held part-numbered volume stands only for itself.
        !crate::fsops::path(p).exists()
            && crate::streamrepair::rar_part_number(hold_path_filename(p)).is_none()
    }) {
        // Under obfuscated names a dead file is held under its posted name, which tells nothing
        // of its place in the set: drop one held file of this download that never reached the
        // disk for each stand-in, so the holds still count the volumes that wait for a repair.
        // Its own name is never reported released: it is no archive to extract.
        let stand_in = st.held.remove(i);
        st.holes.remove(&stand_in);
    } else {
        // The volume arrived intact under a name not renamed yet: nothing was lost for it.
        return;
    }
    *dead_volumes_borrowed().entry(nzb_id).or_default() += 1;
    st.released.push(hold_path_filename(full_path).to_vec());
    if st.held.is_empty() {
        st.awaiting_repair = false;
    }
}

/// Releases one repaired volume by path. Returns true when it was on hold.
pub(crate) fn direct_unpack_release_path(full_path: &[u8]) -> bool {
    let settled = settle_offered(full_path);
    let mut map = direct_unpack_holds();
    for st in map.values_mut() {
        if let Some(i) = st.held.iter().position(|p| p == full_path) {
            let path = st.held.remove(i);
            drop_offered(st, &path, settled.as_ref());
            st.holes.remove(&path);
            st.released.push(hold_path_filename(&path).to_vec());
            if st.held.is_empty() {
                st.awaiting_repair = false;
            }
            return true;
        }
    }
    false
}

/// The stand-in for `full_path` handed over with holes, completed from the real volume now on
/// disk (recreated whole from a twin, or repaired by par2), if both are there. The extraction
/// may have read the stand-in already and recorded its holes against the stand-in's layout,
/// which for RAR5 has no part checksum and so starts its data at another offset than the real
/// volume: the real volume's data is copied into the stand-in's scratch file instead of
/// switching to the real file, so the recorded holes still name the right bytes. The real
/// volume must end like the stand-in and be long enough to hold its data; anything it gets
/// wrong is caught when the extracted file is verified.
fn settle_offered(full_path: &[u8]) -> Option<crate::deadvolume::VirtualVolume> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let offered = direct_unpack_holds()
        .values()
        .find_map(|st| case_blind(&st.offered, full_path).cloned())?;
    let real = crate::fsops::path(full_path);
    let tail_len = offered.tail.len() as u64;
    let complete = (|| -> std::io::Result<Option<u32>> {
        let mut src = std::fs::File::open(real)?;
        let len = src.metadata()?.len();
        let Some(head_len) = len.checked_sub(offered.data_len + tail_len) else {
            return Ok(None);
        };
        let mut tail = vec![0u8; tail_len as usize];
        src.seek(SeekFrom::Start(len - tail_len))?;
        src.read_exact(&mut tail)?;
        if tail != offered.tail {
            return Ok(None);
        }
        src.seek(SeekFrom::Start(head_len))?;
        let mut dst = std::fs::OpenOptions::new().write(true).open(&offered.data)?;
        let mut hasher = crc32fast::Hasher::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut left = offered.data_len;
        while left > 0 {
            let want = left.min(buf.len() as u64) as usize;
            let n = src.read(&mut buf[..want])?;
            if n == 0 {
                return Ok(None);
            }
            hasher.update(&buf[..n]);
            dst.write_all(&buf[..n])?;
            left -= n as u64;
        }
        dst.flush()?;
        Ok(Some(hasher.finalize()))
    })()
    .ok()
    .flatten();
    // A RAR 1.5-4 stand-in was handed over with part checksum 0 (not known yet); the finished
    // one carries the real one, as a reader that checks it (the copy path) needs.
    complete.map(|crc| crate::deadvolume::VirtualVolume {
        head: crate::deadvolume::with_part_crc(&offered.head, crc),
        holes: Vec::new(),
        ..offered
    })
}

/// Drops the stand-in for `path` handed over with holes, as its real volume is released: it
/// becomes a finished stand-in when `settled` completed it (see [`settle_offered`]), otherwise
/// it goes with its scratch file and the real volume answers.
fn drop_offered(
    st: &mut DirectUnpackHoldState,
    path: &[u8],
    settled: Option<&crate::deadvolume::VirtualVolume>,
) {
    let mut stale = Vec::new();
    st.offered.retain(|k, v| {
        let keep = !k.eq_ignore_ascii_case(path);
        if !keep {
            stale.push((k.clone(), v.data.clone()));
        }
        keep
    });
    for (key, data) in stale {
        match settled {
            Some(v) if v.data == data => {
                st.virtuals.insert(key, v.clone());
            }
            _ => {
                if crate::deadvolume::is_scratch(&data) {
                    let _ = std::fs::remove_file(&data);
                }
            }
        }
    }
}

/// Releases the held volumes of download `nzb_id` if every one of them is in `complete` (full
/// paths of volumes a repair completed); otherwise releases none. A partial release would let
/// direct unpack read on while post-processing goes on to a par2 repair, which may rewrite a
/// released volume it sees as damaged (a volume end rebuilt from a sibling) under the reader.
pub(crate) fn direct_unpack_release_if_all(nzb_id: i32, complete: &[Vec<u8>]) -> bool {
    let held: Vec<Vec<u8>> = match direct_unpack_holds().get(&nzb_id) {
        Some(st) if !st.held.is_empty() && st.held.iter().all(|p| complete.contains(p)) => {
            st.held.clone()
        }
        _ => return false,
    };
    for path in held {
        direct_unpack_release_path(&path);
    }
    true
}

/// Releases every held volume of download `nzb_id` (its repair succeeded).
pub(crate) fn direct_unpack_release_all(nzb_id: i32) {
    let mut map = direct_unpack_holds();
    if let Some(st) = map.get_mut(&nzb_id) {
        let held = std::mem::take(&mut st.held);
        st.holes.clear();
        for path in held {
            st.released.push(hold_path_filename(&path).to_vec());
        }
        st.awaiting_repair = false;
    }
}

struct LiveHoldRun {
    nzb_id: i32,
    generation: crate::parlive::SessionGeneration,
    active: bool,
}
impl LiveHoldRun {
    fn finish(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        if let Some(state) = direct_unpack_holds().get_mut(&self.nzb_id) {
            if state.live_repair_generation == Some(self.generation) {
                state.live_repair_running = false;
                state.live_repair_generation = None;
            }
        }
    }
}
impl Drop for LiveHoldRun {
    fn drop(&mut self) {
        self.finish();
    }
}
struct LivePublication {
    targets: Vec<(i32, Vec<u8>)>,
    _admission: std::sync::Arc<crate::parlive::ControlLease>,
}
fn begin_live_hold_run(
    nzb_id: i32,
    generation: crate::parlive::SessionGeneration,
) -> Option<LiveHoldRun> {
    let mut map = direct_unpack_holds();
    let state = map.get_mut(&nzb_id)?;
    // Never alongside the download-end repair of the same download (checked under the
    // holds lock, which that repair also takes before it starts).
    if state.held.is_empty()
        || state.abandoned
        || state.live_repair_running
        || live_end_repair_running(nzb_id)
    {
        return None;
    }
    state.live_repair_running = true;
    state.live_repair_generation = Some(generation);
    Some(LiveHoldRun {
        nzb_id,
        generation,
        active: true,
    })
}
fn publish_live_holds(
    nzb_id: i32,
    generation: crate::parlive::SessionGeneration,
    targets: &[(i32, Vec<u8>)],
    verified: &[i32],
) -> usize {
    let settled: Vec<(&[u8], Option<crate::deadvolume::VirtualVolume>)> = targets
        .iter()
        .filter(|(id, _)| verified.contains(id))
        .map(|(_, path)| (path.as_slice(), settle_offered(path)))
        .collect();
    let mut map = direct_unpack_holds();
    let Some(state) = map.get_mut(&nzb_id) else {
        return 0;
    };
    if state.abandoned
        || !state.live_repair_running
        || state.live_repair_generation != Some(generation)
    {
        return 0;
    }
    let mut released = 0;
    for (id, path) in targets {
        if !verified.contains(id) {
            continue;
        }
        if let Some(index) = state.held.iter().position(|held| held == path) {
            let path = state.held.remove(index);
            let done = settled.iter().find(|(p, _)| *p == path.as_slice()).and_then(|(_, v)| v.as_ref());
            drop_offered(state, &path, done);
            state.released.push(hold_path_filename(&path).to_vec());
            released += 1;
        }
    }
    if state.held.is_empty() {
        state.awaiting_repair = false;
    }
    released
}

/// Capture only completed targets while the caller owns the queue. The helper never takes it.
#[cfg(unix)]
fn live_repair_may_start(nzb: &crate::queuesubsys_host::NzbSnap) -> bool {
    // Ordinary postprocessing owns all mutation once its queued stage is taken.
    !nzb.has_post_info
        || (!nzb.post_working && nzb.post_stage == PostStage::Queued)
}

/// Post-processing waits while live work for `nzb_id` is still pending: completed files
/// not yet folded into the live session, or a live repair helper that is running. It waits
/// as long as the session makes progress (`streaming_session_progress_token`), so a
/// completion drain that is still reading is never cut off for a batch repair that would
/// read the whole download again; only 10 s without any progress hands the job over.
#[cfg(unix)]
pub(crate) fn live_repair_should_defer_post(nzb_id: i32) -> bool {
    type Since = std::collections::HashMap<i32, (std::time::Instant, Option<u64>)>;
    static SINCE: std::sync::LazyLock<Mutex<Since>> =
        std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));
    const LIMIT: Duration = Duration::from_secs(10);
    // A running download-end repair is waited for while it progresses: never cut off
    // mid-decode, and its own wait does not start the clock below.
    match live_end_repair_defers_post(nzb_id) {
        Some(true) => return true,
        Some(false) => {}
        None => {}
    }
    let running = direct_unpack_holds()
        .get(&nzb_id)
        .is_some_and(|state| state.live_repair_running);
    // A download-end live repair that can start now finishes the job from the live
    // syndromes: wait (boundedly) for it to start rather than starting a batch repair. Not
    // when it failed or its targets cannot be resolved, nor when automatic repair is off.
    let end_repair_due = live_end_repair_defers_post(nzb_id).is_none()
        && crate::options::active_options().is_some_and(|options| {
            options.scalars().par_repair != 0
                && options.scalars().par_check != crate::queuesubsys_host::PC_MANUAL
        })
        && crate::parchecker::streaming_session_download_end_targets(nzb_id).is_some();
    let pending = running
        || crate::parchecker::streaming_session_pending_completed_jobs().contains(&nzb_id)
        || crate::parchecker::streaming_session_completion_drain_active(nzb_id)
        || end_repair_due;
    let mut since = SINCE.lock().unwrap_or_else(|e| e.into_inner());
    if !pending {
        since.remove(&nzb_id);
        return false;
    }
    let progress = crate::parchecker::streaming_session_progress_token(nzb_id);
    let now = std::time::Instant::now();
    let clock = since.entry(nzb_id).or_insert((now, progress));
    if progress.is_some() && clock.1 != progress {
        // Still working: the stall clock starts again.
        *clock = (now, progress);
    }
    if live_defer_within(clock.0, now, LIMIT) {
        return true;
    }
    // Past the cap ordinary repair takes over, but never alongside a live
    // repair: cancel it and wait for its helper to leave the files.
    if running {
        crate::parchecker::streaming_session_cancel(nzb_id);
        return true;
    }
    false
}

/// Within `limit` of the last progress at `since`: post-processing still waits.
#[cfg(unix)]
fn live_defer_within(since: std::time::Instant, now: std::time::Instant, limit: Duration) -> bool {
    now.saturating_duration_since(since) < limit
}

#[cfg(unix)]
pub(crate) fn direct_unpack_try_live_par_repair(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
) {
    let nzb_id = nzb.id;
    if host.self_is_stopped()
        || nzb.deleting
        || nzb.delete_status != DeleteStatus::None
        || !live_repair_may_start(nzb)
        || host.opts().par_check == crate::queuesubsys_host::PC_MANUAL
        || !crate::options::active_options()
            .is_some_and(|options| options.scalars().par_repair != 0)
        || !crate::parchecker::streaming_session_can_schedule_repair(nzb_id)
    {
        return;
    }
    let bytes = {
        let map = direct_unpack_holds();
        let holding = map.get(&nzb_id).is_some_and(|state| !state.held.is_empty());
        if !holding {
            drop(map);
            live_par_repair_at_download_end(host, nzb);
            return;
        }
        let Some(state) = map.get(&nzb_id) else {
            return;
        };
        if state.held.is_empty()
            || state.held.len() > 512
            || state.live_repair_running
            || state.abandoned
        {
            return;
        }
        state.held.iter().try_fold(4096u64, |total, path| {
            total.checked_add((path.len() as u64).checked_mul(4)?.checked_add(512)?)
        })
    };
    let Some(bytes) = bytes else {
        return;
    };
    let Ok(charge) = crate::parlive::reserve_control(
        nzb_id,
        bytes,
        crate::parchecker::effective_live_pool_bytes(),
    ) else {
        return;
    };
    let charge = std::sync::Arc::new(charge);
    let held = {
        let map = direct_unpack_holds();
        let Some(state) = map.get(&nzb_id) else {
            return;
        };
        let actual = state.held.iter().try_fold(4096u64, |total, path| {
            total.checked_add((path.len() as u64).checked_mul(4)?.checked_add(512)?)
        });
        if actual.is_none_or(|actual| actual > bytes)
            || state.live_repair_running
            || state.held.len() > 512
        {
            return;
        }
        state.held.clone()
    };
    let mut targets = Vec::with_capacity(held.len());
    let mut publication = Vec::with_capacity(held.len());
    for path in held {
        let name = hold_path_filename(&path);
        let mut queue_id = None;
        for index in 0..host.nzb_completed_files_len(nzb.ptr) {
            let file = host.nzb_completed_file_at(nzb.ptr, index);
            if file.filename == name && !file.par_file {
                queue_id = Some(file.id);
                break;
            }
        }
        let Some(file_id) = queue_id else {
            return;
        };
        let expected = crate::fsops::path(&nzb.dest_dir).join(crate::fsops::path(name));
        if expected != crate::fsops::path(&path) {
            return;
        }
        targets.push(crate::parlive::WriterTarget {
            file_id,
            path: expected,
        });
        publication.push((file_id, path));
    }
    refresh_live_target_paths(host, nzb);
    let Ok(request) = crate::parchecker::streaming_session_prepare_repair(
        nzb_id,
        targets,
        std::sync::Arc::clone(&charge),
    ) else {
        return;
    };
    let publication = LivePublication {
        targets: publication,
        _admission: request.admission_hold(),
    };
    let generation = request.generation();
    let Some(run) = begin_live_hold_run(nzb_id, generation) else {
        return;
    };
    let nzb_name = nzb.name.clone();
    let spawned = std::thread::Builder::new().name("live-par-repair".to_string()).spawn(move || {
        let mut run = run; // Every exit, including unwind, returns to ordinary repair.
        let started = std::time::Instant::now();
        let outcome = match request.acquire() {
            Ok(lease) => lease.repair_and_publish(
                |verified| publish_live_holds(nzb_id, generation, &publication.targets, verified),
                || run.finish(),
            ),
            Err(error) => { run.finish(); Err(error) }
        };
        let mut message = b"Live par-repair for ".to_vec();
        message.extend_from_slice(&nzb_name);
        let kind = match outcome {
            Ok((report, released)) => {
                message.extend_from_slice(format!(": repaired {} block(s) with {} recovery block(s) in {:.1} s while downloading, {} paused volume(s) released", report.repaired_slices.len(), report.recovery_used, started.elapsed().as_secs_f64(), released).as_bytes());
                MK_INFO
            }
            Err(error) => {
                message.extend_from_slice(format!(" not possible yet ({error}); left to par-check").as_bytes());
                MK_DETAIL
            }
        };
        crate::log::log_active_bytes(kind, &message);
    });
    if let Ok(handle) = spawned {
        keep_live_helper(&LIVE_PAR_HELPERS, nzb_id, handle);
    }
}

/// The download-end live repair of a download, while it runs or once it cannot.
#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
enum EndRepair {
    /// Running; see [`EndRepairClock`].
    Running(EndRepairClock),
    /// It failed: par-check repairs the download; no retry.
    Failed,
    /// Its targets are not under their par2 names (or not complete) yet: par-check goes
    /// ahead without waiting; tried again after `retry_at`.
    Unresolvable { retry_at: std::time::Instant },
}

#[cfg(unix)]
static LIVE_END_REPAIRS: std::sync::LazyLock<Mutex<std::collections::HashMap<i32, EndRepair>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Post-processing waits for a running download-end repair while it makes progress: while
/// par-rs's work counter rises, or the repair is idle (not in a repair call yet) or waiting
/// for another one. It is given up after this long in a step without the counter rising.
#[cfg(unix)]
const END_REPAIR_STALL: Duration = Duration::from_secs(300);
/// The sync step syncs one file at a time, and one large target's sync can stay flat for
/// a long while: it gets this long instead.
#[cfg(unix)]
const END_REPAIR_SYNC_STALL: Duration = Duration::from_secs(1800);

/// A running helper outside any repair call (still admitting its targets, behind the cache
/// fence or the write barrier) is given up after this long.
#[cfg(unix)]
const END_REPAIR_IDLE: Duration = Duration::from_secs(120);

/// A running end repair's progress: par-rs's work counter, when it last rose (or the repair
/// waited for another one), the helper's admission counter, and since when the helper has
/// been outside a repair call with neither rising.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EndRepairClock {
    work: u64,
    progressed_at: std::time::Instant,
    admission: u64,
    idle_since: Option<std::time::Instant>,
}

#[cfg(unix)]
impl EndRepairClock {
    fn start(work: u64, now: std::time::Instant) -> Self {
        Self {
            work,
            progressed_at: now,
            admission: 0,
            idle_since: None,
        }
    }

    /// Advances the clock with a fresh progress sample and the helper's admission counter;
    /// true when the repair has stalled: no work in a working step for longer than that step
    /// allows, or idle (outside a repair call: passing the cache fence, draining writers,
    /// opening targets, folding queued articles) with nothing advancing for
    /// [`END_REPAIR_IDLE`]. Waiting for another repair never stalls.
    fn update(
        &mut self,
        progress: Option<crate::parchecker::LiveRepairProgress>,
        admission: Option<u64>,
        now: std::time::Instant,
    ) -> bool {
        use crate::parchecker::LiveRepairStep;
        let Some(progress) = progress else {
            return false;
        };
        if progress.step == LiveRepairStep::Idle {
            let admission = admission.unwrap_or(self.admission);
            if admission != self.admission || progress.work != self.work {
                self.idle_since = None;
            }
            self.admission = admission;
            let since = *self.idle_since.get_or_insert(now);
            self.work = progress.work;
            self.progressed_at = now;
            return now.duration_since(since) >= END_REPAIR_IDLE;
        }
        self.idle_since = None;
        if progress.work != self.work || progress.step == LiveRepairStep::PayloadWait {
            self.work = progress.work;
            self.progressed_at = now;
            return false;
        }
        let limit = if progress.step == LiveRepairStep::Sync {
            END_REPAIR_SYNC_STALL
        } else {
            END_REPAIR_STALL
        };
        now.duration_since(self.progressed_at) >= limit
    }
}

#[cfg(unix)]
const END_REPAIR_RETRY: Duration = Duration::from_secs(10);

#[cfg(unix)]
fn live_end_repair_state(nzb_id: i32) -> Option<EndRepair> {
    LIVE_END_REPAIRS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&nzb_id)
        .copied()
}

#[cfg(unix)]
fn live_end_repair_running(nzb_id: i32) -> bool {
    matches!(live_end_repair_state(nzb_id), Some(EndRepair::Running(_)))
}

/// Whether post-processing should wait for the download-end repair of `nzb_id`: one that
/// runs and made progress within [`END_REPAIR_STALL`] (a stalled one is cancelled, and
/// post-processing waits for its helper to leave the files), or one that can start right
/// now. `None` when no end repair concerns post-processing.
#[cfg(unix)]
fn live_end_repair_defers_post(nzb_id: i32) -> Option<bool> {
    let now = std::time::Instant::now();
    let mut map = LIVE_END_REPAIRS.lock().unwrap_or_else(|e| e.into_inner());
    match map.get_mut(&nzb_id) {
        Some(EndRepair::Running(clock)) => {
            let stalled = clock.update(
                crate::parchecker::streaming_session_repair_progress(nzb_id),
                crate::parchecker::streaming_session_admission_progress(nzb_id),
                now,
            );
            if stalled {
                drop(map);
                crate::parchecker::streaming_session_cancel(nzb_id);
            }
            Some(true)
        }
        Some(EndRepair::Failed) => Some(false),
        Some(EndRepair::Unresolvable { retry_at }) if now < *retry_at => Some(false),
        _ => None,
    }
}

/// Keeps a live repair helper's handle for joining before cleanup. Handles of helpers that
/// already ended are joined here (they return at once); a running one is never waited for,
/// since callers may hold the queue lock.
#[cfg(unix)]
fn keep_live_helper(
    helpers: &LiveHelpers,
    nzb_id: i32,
    handle: std::thread::JoinHandle<()>,
) {
    let finished = {
        let mut map = helpers.lock().unwrap_or_else(|e| e.into_inner());
        let list = map.entry(nzb_id).or_default();
        let (done, running): (Vec<_>, Vec<_>) =
            std::mem::take(list).into_iter().partition(|h| h.is_finished());
        *list = running;
        list.push(handle);
        done
    };
    for handle in finished {
        let _ = handle.join();
    }
}

/// The live repair at the end of a download without held direct-unpack volumes: once every
/// slice arrived or failed and the recovery on hand covers the damage, rebuild the damaged
/// files from the live syndromes (no batch verify or repair read) and certify their CRCs,
/// so post-processing's par-check finds them repaired and no batch repair runs. A file still
/// under another name (obfuscated, not renamed yet) is repaired where it is: its live binding
/// names its par2 file, and the certified CRC follows it through par-rename. The caller
/// holds the queue lock; the repair runs on its own thread.
#[cfg(unix)]
fn live_par_repair_at_download_end(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
) {
    let nzb_id = nzb.id;
    let now = std::time::Instant::now();
    match live_end_repair_state(nzb_id) {
        Some(EndRepair::Running(_) | EndRepair::Failed) => return,
        Some(EndRepair::Unresolvable { retry_at }) if now < retry_at => return,
        _ => {}
    }
    // Never alongside a live repair of held direct-unpack volumes.
    if direct_unpack_holds()
        .get(&nzb_id)
        .is_some_and(|state| state.live_repair_running)
    {
        return;
    }
    let Some(names) = crate::parchecker::streaming_session_download_end_targets(nzb_id) else {
        return;
    };
    let unresolvable = || {
        LIVE_END_REPAIRS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(nzb_id, EndRepair::Unresolvable { retry_at: now + END_REPAIR_RETRY });
    };
    if names.len() > 512 {
        unresolvable();
        return;
    }
    let mut targets = Vec::with_capacity(names.len());
    for name in &names {
        // Under its par2 name, or still under another (obfuscated) one: the live binding
        // of the queue file names its par2 file, and it is repaired where it is.
        let found = (0..host.nzb_completed_files_len(nzb.ptr))
            .map(|index| host.nzb_completed_file_at(nzb.ptr, index))
            .filter(|file| !file.par_file)
            .find(|file| file.filename == *name)
            .or_else(|| {
                (0..host.nzb_completed_files_len(nzb.ptr))
                    .map(|index| host.nzb_completed_file_at(nzb.ptr, index))
                    .filter(|file| !file.par_file)
                    .find(|file| {
                        crate::parchecker::streaming_session_par_name_of(
                            nzb_id,
                            (Some(file.id), &file.filename, &file.hash16k),
                        )
                        .is_some_and(|par_name| par_name == *name)
                    })
            });
        let Some(file) = found else {
            unresolvable();
            return;
        };
        let path =
            crate::fsops::path(&nzb.dest_dir).join(crate::fsops::path(&file.filename));
        if !path.is_file() {
            unresolvable();
            return;
        }
        targets.push(crate::parlive::WriterTarget {
            file_id: file.id,
            path,
        });
    }
    refresh_live_target_paths(host, nzb);
    let bytes = targets.iter().fold(4096u64, |total, target| {
        total.saturating_add((target.path.as_os_str().len() as u64).saturating_mul(4) + 512)
    });
    let Ok(charge) = crate::parlive::reserve_control(
        nzb_id,
        bytes,
        crate::parchecker::effective_live_pool_bytes(),
    ) else {
        return;
    };
    let Ok(request) = crate::parchecker::streaming_session_prepare_repair(
        nzb_id,
        targets,
        std::sync::Arc::new(charge),
    ) else {
        return;
    };
    {
        // The holds lock first, as the held path takes it: one of the two starts.
        let holds = direct_unpack_holds();
        if holds.get(&nzb_id).is_some_and(|state| state.live_repair_running) {
            return;
        }
        let mut map = LIVE_END_REPAIRS.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(map.get(&nzb_id), Some(EndRepair::Running(_))) {
            return;
        }
        let work = crate::parchecker::streaming_session_repair_progress(nzb_id)
            .map_or(0, |progress| progress.work);
        let mut clock = EndRepairClock::start(work, now);
        clock.admission =
            crate::parchecker::streaming_session_admission_progress(nzb_id).unwrap_or(0);
        map.insert(nzb_id, EndRepair::Running(clock));
        drop(map);
        drop(holds);
    }
    /// Ends the running state: removed after success, `Failed` otherwise (also on unwind).
    struct Running {
        nzb_id: i32,
        ok: bool,
    }
    impl Drop for Running {
        fn drop(&mut self) {
            let mut map = LIVE_END_REPAIRS.lock().unwrap_or_else(|e| e.into_inner());
            if self.ok {
                map.remove(&self.nzb_id);
            } else {
                map.insert(self.nzb_id, EndRepair::Failed);
            }
        }
    }
    let running = Running { nzb_id, ok: false };
    let nzb_name = nzb.name.clone();
    let spawned = std::thread::Builder::new()
        .name("live-par-end-repair".to_string())
        .spawn(move || {
            let mut running = running; // Every exit, including unwind, lets par-check run.
            let started = std::time::Instant::now();
            let outcome = request
                .acquire()
                .and_then(|lease| lease.repair_and_publish(|verified| verified.len(), || {}));
            let mut message = b"Live par-repair for ".to_vec();
            message.extend_from_slice(&nzb_name);
            let kind = match outcome {
                Ok((report, files)) => {
                    running.ok = true;
                    message.extend_from_slice(format!(": repaired {} block(s) with {} recovery block(s) in {:.1} s as the download ended, {} file(s) verified; no batch repair needed", report.repaired_slices.len(), report.recovery_used, started.elapsed().as_secs_f64(), files).as_bytes());
                    MK_INFO
                }
                Err(error) => {
                    message.extend_from_slice(format!(" at download end not possible ({error}); left to par-check").as_bytes());
                    MK_DETAIL
                }
            };
            crate::log::log_active_bytes(kind, &message);
            drop(running);
        });
    if let Ok(handle) = spawned {
        keep_live_helper(&LIVE_END_HELPERS, nzb_id, handle);
    }
}

/// Tells the live session where every completed file of the download is now, so a rename
/// since its completion (the direct renamer) cannot make a repair's back-fill read a stale
/// path. The caller holds the queue lock; par-rs learns the paths before the repair.
#[cfg(unix)]
fn refresh_live_target_paths(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
) {
    for index in 0..host.nzb_completed_files_len(nzb.ptr).min(4096) {
        let file = host.nzb_completed_file_at(nzb.ptr, index);
        if file.par_file {
            continue;
        }
        let path = crate::fsops::path(&nzb.dest_dir).join(crate::fsops::path(&file.filename));
        crate::parchecker::streaming_session_note_file_path(
            nzb.id,
            (Some(file.id), &file.filename, &[]),
            &path,
        );
    }
}

#[cfg(unix)]
type LiveHelpers =
    std::sync::LazyLock<Mutex<std::collections::HashMap<i32, Vec<std::thread::JoinHandle<()>>>>>;

/// Helper threads of live par2 repairs of held direct-unpack volumes, per download.
#[cfg(unix)]
static LIVE_PAR_HELPERS: LiveHelpers =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Helper threads of download-end live par2 repairs, per download.
#[cfg(unix)]
static LIVE_END_HELPERS: LiveHelpers =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Stops the live par2 repairs of download `nzb_id` before its files are deleted, moved,
/// renamed, merged or split (deletion, shutdown, the direct renamer's renames and its
/// merge or split of output files, a dupe hand-off): cancels them, and joins their helper
/// threads on a reaper thread, never here. Callers hold the queue lock, and a helper may be waiting for the
/// article cache's flush lock while the flusher holding it waits for the queue lock; joining
/// here would hang the queue. Cancelled, a helper stops waiting for that lock within 50 ms,
/// par-rs stops between slices, and its target writes go through descriptors it owns, so
/// nothing it writes lands in a file another job uses. Post-processing still waits for a
/// helper until it has ended (its running state clears on exit).
#[track_caller]
pub(crate) fn direct_unpack_join_live_par_repair(nzb_id: i32) {
    // Whether this call cancels a running end repair, decided together with the cancel
    // under the end-repair lock (no repair can start or end in between): only then is its
    // Failed state (set by the cancel) cleared afterwards; a genuine failure stays Failed.
    #[cfg(unix)]
    let cancels_end_repair = {
        let map = LIVE_END_REPAIRS.lock().unwrap_or_else(|e| e.into_inner());
        let running = matches!(map.get(&nzb_id), Some(EndRepair::Running(_)));
        crate::parchecker::streaming_session_cancel(nzb_id);
        running
    };
    #[cfg(not(unix))]
    crate::parchecker::streaming_session_cancel(nzb_id);
    #[cfg(unix)]
    {
        let mut handles = Vec::new();
        for helpers in [&LIVE_PAR_HELPERS, &LIVE_END_HELPERS] {
            if let Some(list) = helpers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&nzb_id)
            {
                handles.extend(list);
            }
        }
        reap_live_helpers(nzb_id, handles, cancels_end_repair);
    }
    #[cfg(not(unix))]
    let _ = nzb_id;
}

/// Reaper threads still joining cancelled helpers, for the bounded wait at shutdown.
#[cfg(unix)]
static LIVE_REAPERS: std::sync::LazyLock<Mutex<Vec<std::thread::JoinHandle<()>>>> =
    std::sync::LazyLock::new(|| Mutex::new(Vec::new()));

/// At shutdown, after the live sessions are cancelled: waits up to `limit` for every live
/// repair helper and reaper to end, so no repair write outlives the daemon's own cleanup
/// when it can be helped. Helpers still running then are left to end on their own. Never
/// call it with the queue lock held.
pub(crate) fn live_par_helpers_wait(limit: Duration) {
    #[cfg(unix)]
    {
        let deadline = std::time::Instant::now() + limit;
        let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::new();
        for helpers in [&LIVE_PAR_HELPERS, &LIVE_END_HELPERS] {
            let mut map = helpers.lock().unwrap_or_else(|e| e.into_inner());
            handles.extend(map.drain().flat_map(|(_, list)| list));
        }
        handles.extend(LIVE_REAPERS.lock().unwrap_or_else(|e| e.into_inner()).drain(..));
        while !handles.iter().all(std::thread::JoinHandle::is_finished)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        for handle in handles {
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
    #[cfg(not(unix))]
    let _ = limit;
}

/// Joins ended or cancelled live repair helpers on a thread of its own (finished ones at once).
/// Once they ended, when `cancelled_end_repair` (this stop cancelled a running end repair),
/// its `Failed` state is cleared, so a later live session of the same download (after a
/// rename) may run one again.
#[cfg(unix)]
fn reap_live_helpers(
    nzb_id: i32,
    handles: Vec<std::thread::JoinHandle<()>>,
    cancelled_end_repair: bool,
) {
    let clear_failed = move || {
        if !cancelled_end_repair {
            return;
        }
        let mut map = LIVE_END_REPAIRS.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(map.get(&nzb_id), Some(EndRepair::Failed)) {
            map.remove(&nzb_id);
        }
    };
    let (finished, running): (Vec<_>, Vec<_>) =
        handles.into_iter().partition(|handle| handle.is_finished());
    for handle in finished {
        let _ = handle.join();
    }
    if running.is_empty() {
        clear_failed();
        return;
    }
    let reaper = std::thread::Builder::new()
        .name("live-par-reaper".into())
        .spawn(move || {
            for handle in running {
                let _ = handle.join();
            }
            clear_failed();
        });
    // Without a reaper the handles are dropped: the threads still end on their own.
    if let Ok(reaper) = reaper {
        let mut reapers = LIVE_REAPERS.lock().unwrap_or_else(|e| e.into_inner());
        reapers.retain(|handle| !handle.is_finished());
        reapers.push(reaper);
    }
}

/// Gives up on the held volumes of download `nzb_id` (its repair failed).
pub(crate) fn direct_unpack_abandon(nzb_id: i32) {
    let mut map = direct_unpack_holds();
    if let Some(st) = map.get_mut(&nzb_id) {
        st.held.clear();
        st.holes.clear();
        st.awaiting_repair = false;
        st.abandoned = true;
    }
}

/// True when the unpack thread of `nzb_id` is parked until post-processing repaired the download.
/// Not while a live par2 repair of the held volumes is running: post-processing must not
/// repair the same files at the same time.
pub(crate) fn direct_unpack_awaiting_repair(nzb_id: i32) -> bool {
    direct_unpack_holds()
        .get(&nzb_id)
        .is_some_and(|st| st.awaiting_repair && !st.live_repair_running)
}

fn direct_unpack_set_awaiting_repair(nzb_id: i32) -> bool {
    let mut map = direct_unpack_holds();
    match map.get_mut(&nzb_id) {
        Some(st) if !st.held.is_empty() => {
            st.awaiting_repair = true;
            true
        }
        _ => false,
    }
}

fn direct_unpack_take_released(nzb_id: i32) -> (Vec<Vec<u8>>, bool) {
    let mut map = direct_unpack_holds();
    match map.get_mut(&nzb_id) {
        Some(st) => (std::mem::take(&mut st.released), st.abandoned),
        None => (Vec::new(), false),
    }
}

/// New `(ParSec, live part of ParSec, RepairSec)` once live PAR's `(par, repair)`
/// seconds are charged; `None` when nothing changes. A deleted job's time is dropped.
fn live_par_time_charge(
    deleted: bool,
    (par_sec, live_par_sec, repair_sec): (i32, i32, i32),
    (live_par, live_repair): (i32, i32),
) -> Option<(i32, i32, i32)> {
    (!deleted && (live_par, live_repair) != (0, 0)).then_some((
        par_sec.saturating_add(live_par),
        live_par_sec.saturating_add(live_par),
        repair_sec.saturating_add(live_repair),
    ))
}

/// The job reaches history (or leaves the queue without it): live PAR's verification
/// and repair while downloading is charged to its times, once, whether or not a
/// par-check ran. A deleted job's, or one leaving without history, is dropped.
pub(crate) fn live_par_time_settle(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: NzbRef,
    keep: bool,
) {
    use crate::queuesubsys_host::{NZB_INT_LIVE_PAR_SEC, NZB_INT_PAR_SEC, NZB_INT_REPAIR_SEC};
    let nzb = host.nzb_snap(nzb_ptr, false);
    let taken = crate::parchecker::streaming_session_take_par_secs(nzb.id);
    let current = (nzb.par_sec, nzb.live_par_sec, nzb.repair_sec);
    let deleted = !keep || nzb.delete_status != DeleteStatus::None;
    if let Some((par, live, repair)) = live_par_time_charge(deleted, current, taken) {
        host.nzb_set_int(nzb_ptr, NZB_INT_PAR_SEC, par);
        host.nzb_set_int(nzb_ptr, NZB_INT_LIVE_PAR_SEC, live);
        host.nzb_set_int(nzb_ptr, NZB_INT_REPAIR_SEC, repair);
    }
}

/// Queue checkpoint: moves the whole seconds of a queued job's live PAR time into its
/// saved times now, so a restart keeps them; the fraction stays for later, and the
/// rest is settled at history. Nothing is counted twice.
pub(crate) fn live_par_time_checkpoint(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb_ptr: NzbRef,
) {
    use crate::queuesubsys_host::{NZB_INT_LIVE_PAR_SEC, NZB_INT_PAR_SEC, NZB_INT_REPAIR_SEC};
    let nzb = host.nzb_snap(nzb_ptr, false);
    if nzb.delete_status != DeleteStatus::None {
        return; // dropped at history
    }
    let taken = crate::parchecker::streaming_session_take_whole_par_secs(nzb.id);
    let current = (nzb.par_sec, nzb.live_par_sec, nzb.repair_sec);
    if let Some((par, live, repair)) = live_par_time_charge(false, current, taken) {
        host.nzb_set_int(nzb_ptr, NZB_INT_PAR_SEC, par);
        host.nzb_set_int(nzb_ptr, NZB_INT_LIVE_PAR_SEC, live);
        host.nzb_set_int(nzb_ptr, NZB_INT_REPAIR_SEC, repair);
    }
}

/// Drops job `nzb_id`'s unsettled live PAR time: live work that ended after the job
/// reached history, or a job that merge/split removed, belongs to no job's times.
pub(crate) fn live_par_time_discard(nzb_id: i32) {
    let _ = crate::parchecker::streaming_session_take_par_secs(nzb_id);
}

/// Borrowed dead volumes of downloads whose direct unpack ended without success, by full
/// path: post-processing's stream repair resumes them ([`direct_unpack_borrowed`]) and
/// writes them as real volumes, instead of fetching their bytes from the duplicate again.
/// Their scratch files go when the download leaves post-processing
/// ([`direct_unpack_forget_holds`]).
static KEPT_BORROWS: std::sync::LazyLock<
    Mutex<std::collections::HashMap<i32, std::collections::HashMap<Vec<u8>, crate::deadvolume::VirtualVolume>>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

fn kept_borrows() -> std::sync::MutexGuard<
    'static,
    std::collections::HashMap<i32, std::collections::HashMap<Vec<u8>, crate::deadvolume::VirtualVolume>>,
> {
    KEPT_BORROWS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Forgets download `nzb_id`'s holds and deletes the scratch files of its borrowed dead
/// volumes: the download is done with them (it left post-processing, was deleted, or
/// direct unpack extracted them).
pub(crate) fn direct_unpack_forget_holds(nzb_id: i32) {
    note_dead_volume_borrow(nzb_id, false);
    forget_dead_volume_verdicts(nzb_id);
    let gone = direct_unpack_holds().remove(&nzb_id);
    let kept = kept_borrows().remove(&nzb_id);
    for v in gone
        .iter()
        .flat_map(|st| st.virtuals.values().chain(st.offered.values()))
        .chain(kept.iter().flat_map(|k| k.values()))
    {
        if crate::deadvolume::is_scratch(&v.data) {
            let _ = std::fs::remove_file(&v.data);
        }
    }
}

/// Forgets download `nzb_id`'s holds while post-processing still needs its borrowed dead
/// volumes: direct unpack ended without extracting them (or never paused for them), so the
/// stages below repair and unpack the download. The borrowed bytes are kept for them, so no
/// dead volume is ever fetched twice.
pub(crate) fn direct_unpack_forget_holds_keep_borrows(nzb_id: i32) {
    note_dead_volume_borrow(nzb_id, false);
    forget_dead_volume_verdicts(nzb_id);
    let Some(gone) = direct_unpack_holds().remove(&nzb_id) else {
        return;
    };
    let DirectUnpackHoldState {
        virtuals, offered, ..
    } = gone;
    if virtuals.is_empty() && offered.is_empty() {
        return;
    }
    let mut kept = kept_borrows();
    let entry = kept.entry(nzb_id).or_default();
    // A finished stand-in replaces one handed over with holes.
    for (path, volume) in offered.into_iter().chain(virtuals) {
        if let Some(old) = entry.insert(path, volume) {
            if crate::deadvolume::is_scratch(&old.data)
                && !entry.values().any(|v| v.data == old.data)
            {
                let _ = std::fs::remove_file(&old.data);
            }
        }
    }
}

/// The borrowed stand-in for dead volume `full_path` of download `nzb_id`, finished or
/// handed over with holes, whether direct unpack still holds it or post-processing kept it.
pub(crate) fn direct_unpack_borrowed(
    nzb_id: i32,
    full_path: &[u8],
) -> Option<crate::deadvolume::VirtualVolume> {
    direct_unpack_virtual(nzb_id, full_path).or_else(|| {
        kept_borrows()
            .get(&nzb_id)
            .and_then(|kept| case_blind(kept, full_path).cloned())
    })
}

/// Drops the kept borrow of dead volume `full_path` of download `nzb_id` once it was
/// written as a real volume (its scratch file is gone).
pub(crate) fn direct_unpack_forget_borrowed(nzb_id: i32, full_path: &[u8]) {
    let mut kept = kept_borrows();
    if let Some(entry) = kept.get_mut(&nzb_id) {
        entry.retain(|k, _| !k.eq_ignore_ascii_case(full_path));
        if entry.is_empty() {
            kept.remove(&nzb_id);
        }
    }
    drop(kept);
    let mut holds = direct_unpack_holds();
    if let Some(st) = holds.get_mut(&nzb_id) {
        st.virtuals.retain(|k, _| !k.eq_ignore_ascii_case(full_path));
        st.offered.retain(|k, _| !k.eq_ignore_ascii_case(full_path));
    }
}

#[derive(Default)]
struct DirectUnpackVolumeState {
    archives: VecDeque<Vec<u8>>,
    waiting_file: Option<Vec<u8>>,
    nzb_completed: bool,
    progress_label: Vec<u8>,
    info_name: Vec<u8>,
}

struct DirectUnpackRunLocal {
    name: Vec<u8>,
    info_name: Vec<u8>,
    info_name_up: Vec<u8>,
    dest_dir: Vec<u8>,
    final_dir: Vec<u8>,
    unpack_dir: Vec<u8>,
    password: Vec<u8>,
    inter_dir_opt: Vec<u8>,
    extracted_archives: Vec<Vec<u8>>,
    all_ok_received: bool,
    unpack_ok: bool,
    final_dir_created: bool,
    use_temp_unpack_dir: bool,
    unpack_cleanup_disk: bool,
    expected_volumes: u64,
    /// The job's unpack limits, shared by every archive set this run unpacks.
    unpack_guard: Option<crate::unpacklimits::SharedGuard>,
}

/// Wall time a job spends in one activity, which starts and stops as it goes.
#[derive(Default)]
struct BusyTime {
    since: Option<std::time::Instant>,
    /// Not yet reported.
    total: Duration,
    /// The part of `total` from before the download completed.
    live: Duration,
    download_done: bool,
}

impl BusyTime {
    fn set_active(&mut self, active: bool, now: std::time::Instant) {
        match (active, self.since) {
            (true, None) => self.since = Some(now),
            (false, Some(since)) => {
                self.total += now.saturating_duration_since(since);
                self.since = None;
            }
            _ => {}
        }
    }

    /// Folds an open period into `total` up to `now`, and keeps it open.
    fn fold_open(&mut self, now: std::time::Instant) {
        if let Some(since) = self.since.as_mut() {
            self.total += now.saturating_duration_since(*since);
            *since = now;
        }
    }

    /// The download completed: what was busy so far ran while downloading. Idle time
    /// after this (waiting for a volume or the unpack lock) is not busy, so it never
    /// shifts the split.
    fn mark_download_done(&mut self, now: std::time::Instant) {
        if !self.download_done {
            self.fold_open(now);
            self.live = self.total;
            self.download_done = true;
        }
    }

    /// Whole seconds so far (an open period included) that no earlier take reported,
    /// and the part of them from before the download completed.
    fn take_secs(&mut self, now: std::time::Instant) -> (i32, i32) {
        self.fold_open(now);
        if !self.download_done {
            self.live = self.total;
        }
        let secs = |d: Duration| d.as_secs_f64().round().min(i32::MAX as f64);
        let total = secs(self.total);
        let live = secs(self.live).min(total);
        self.total = self.total.saturating_sub(Duration::from_secs_f64(total));
        self.live = self.live.saturating_sub(Duration::from_secs_f64(live));
        (total as i32, live as i32)
    }
}

/// What direct unpack reads from its download in the queue.
#[cfg(unix)]
struct DirectUnpackNzb {
    ptr: NzbRef,
    name: Vec<u8>,
    dest_dir: Vec<u8>,
    password: Option<Vec<u8>>,
    inter_dir_opt: Vec<u8>,
    use_temp_unpack_dir: bool,
    unpack_cleanup_disk: bool,
    /// The download's unpack thread is this direct unpack's.
    owned: bool,
    has_post_info: bool,
}

pub struct NzbgetRsDirectUnpack {
    nzb_id: i32,
    thread_ptr: usize,
    vol: Mutex<DirectUnpackVolumeState>,
    extra_start_time: AtomicI64,
    /// Time spent unpacking (not waiting for a volume), for the job's UnpackSec.
    busy: Mutex<BusyTime>,
    processed: AtomicBool,
    cancelled: AtomicBool,
    waiting_volume: AtomicBool,
    unpacking: AtomicBool,
    terminated: AtomicBool,
    exec_thread_key: AtomicUsize,
}

impl NzbgetRsDirectUnpack {
    pub fn new(nzb_id: i32, thread_ptr: usize) -> Self {
        Self {
            nzb_id,
            thread_ptr,
            vol: Mutex::new(DirectUnpackVolumeState::default()),
            extra_start_time: AtomicI64::new(0),
            busy: Mutex::new(BusyTime::default()),
            processed: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            waiting_volume: AtomicBool::new(false),
            unpacking: AtomicBool::new(false),
            terminated: AtomicBool::new(false),
            exec_thread_key: AtomicUsize::new(0),
        }
    }

    fn lock_vol(&self) -> std::sync::MutexGuard<'_, DirectUnpackVolumeState> {
        self.vol.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn is_unpacking(&self) -> bool {
        self.unpacking.load(Ordering::Acquire)
    }

    pub fn is_waiting_volume(&self) -> bool {
        self.waiting_volume.load(Ordering::Acquire)
    }

    fn set_waiting(&self, waiting: bool) {
        self.account_busy(&self.waiting_volume, waiting);
    }

    fn set_unpacking(&self, unpacking: bool) {
        self.account_busy(&self.unpacking, unpacking);
    }

    /// Sets `flag`; unpacking counts while an archive is being extracted and no volume
    /// is awaited. The store and the reading of both flags happen under the busy lock,
    /// so transitions from the downloader and the extractor apply in order.
    fn account_busy(&self, flag: &AtomicBool, value: bool) {
        let mut busy = self.busy.lock().unwrap_or_else(|e| e.into_inner());
        flag.store(value, Ordering::Release);
        let active =
            self.unpacking.load(Ordering::Acquire) && !self.waiting_volume.load(Ordering::Acquire);
        busy.set_active(active, std::time::Instant::now());
    }

    fn is_stopped(&self, host: &QueueSubsysHost) -> bool {
        host.thread_is_stopped(self.thread_ptr)
    }

    fn is_stopped_or_terminated(&self, host: &QueueSubsysHost) -> bool {
        self.is_stopped(host) || self.terminated.load(Ordering::Acquire)
    }

    /// Writes to the global log only.
    fn global_log(kind: i32, msg: &[u8]) {
        crate::log::log_active_bytes(kind, cstr::until_nul(msg));
    }

    /// Adds a message to the download's message list.
    fn nzb_message(host: &QueueSubsysHost, nzb_ptr: NzbRef, kind: i32, msg: &[u8]) {
        host.nzb_add_message(nzb_ptr, kind, cstr::until_nul(msg));
    }

    /// Shows `label` as the post-processing progress, when the download has post info.
    fn post_progress_label(host: &QueueSubsysHost, nzb_ptr: NzbRef, label: &[u8]) {
        let ns = host.nzb_snap(nzb_ptr, false);
        if ns.post_info_ptr.is_some() {
            host.post_set_progress_label(ns.post_info_ptr, cstr::until_nul(label));
        }
    }

    /// The direct unpack's download in the locked queue `dq_ptr`.
    fn find_nzb(&self, host: &QueueSubsysHost, dq_ptr: usize) -> Option<DirectUnpackNzb> {
        let n = host.queue_find_by_id(dq_ptr, self.nzb_id);
        if n.is_none() {
            return None;
        }
        let ns = host.nzb_snap(n, false);
        let password = host.nzb_get_param(n, b"*Unpack:Password");
        let opts = crate::options::active_options();
        let sc = opts.as_ref().map(|o| o.scalars());
        Some(DirectUnpackNzb {
            ptr: n,
            name: cstr::until_nul(&ns.name).to_vec(),
            dest_dir: cstr::until_nul(&ns.dest_dir).to_vec(),
            password: password.map(|p| cstr::until_nul(&p).to_vec()),
            inter_dir_opt: cstr::until_nul(
                opts.as_ref()
                    .map_or(&b""[..], |o| o.str_bytes(OptStr::InterDir)),
            )
            .to_vec(),
            use_temp_unpack_dir: sc.as_ref().is_some_and(|s| s.use_temp_unpack_dir != 0),
            unpack_cleanup_disk: sc.as_ref().is_some_and(|s| s.unpack_cleanup_disk != 0),
            owned: host.nzb_unpack_thread_ptr(n) == self.thread_ptr,
            has_post_info: ns.has_post_info,
        })
    }

    fn final_dir_name(host: &QueueSubsysHost, nzb_ptr: NzbRef) -> Vec<u8> {
        cstr::until_nul(&host.nzb_build_final_dir_name(nzb_ptr)).to_vec()
    }

    /// Clears the download's unpack thread and sets its direct unpack status.
    fn finish_nzb(host: &QueueSubsysHost, nzb_ptr: NzbRef, status: DirectUnpackStatus) {
        use crate::queuesubsys_host::NZB_ACT_CLEAR_UNPACK_THREAD;
        host.nzb_action(nzb_ptr, NZB_ACT_CLEAR_UNPACK_THREAD);
        host.nzb_set_direct_unpack_status(nzb_ptr, status);
    }

    fn add_extra_time(&self, host: &QueueSubsysHost, nzb_ptr: NzbRef, clear_working: bool) {
        let start = self.extra_start_time.swap(0, Ordering::AcqRel);
        let extra_sec = if start != 0 {
            (current_time_sec() - start) as c_int
        } else {
            0
        };
        // The unpacking done so far, while downloading and after, and the part while
        // downloading (split when the download completed); reported once.
        let (busy_sec, live_sec) = self
            .busy
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take_secs(std::time::Instant::now());
        if start != 0 || clear_working || busy_sec != 0 {
            use crate::queuesubsys_host::{
                NZB_INT_LIVE_UNPACK_SEC, NZB_INT_POST_TOTAL_SEC, NZB_INT_UNPACK_SEC,
                POST_INT_WORKING,
            };
            // busy_sec: seconds spent unpacking (while downloading too), live_sec: the part
            // of it while downloading; extra_sec: the wait after the download, which
            // post-processing spent on direct unpack.
            let ns = host.nzb_snap(nzb_ptr, false);
            if busy_sec != 0 {
                host.nzb_set_int(nzb_ptr, NZB_INT_UNPACK_SEC, ns.unpack_sec + busy_sec);
            }
            if live_sec != 0 {
                host.nzb_set_int(
                    nzb_ptr,
                    NZB_INT_LIVE_UNPACK_SEC,
                    ns.live_unpack_sec + live_sec,
                );
            }
            if start != 0 {
                host.nzb_set_int(
                    nzb_ptr,
                    NZB_INT_POST_TOTAL_SEC,
                    ns.post_total_sec + extra_sec,
                );
            }
            if clear_working && ns.post_info_ptr.is_some() && !ns.has_live_repair_thread {
                host.post_set_int(ns.post_info_ptr, POST_INT_WORKING, 0);
            }
        }
    }

    pub(crate) fn stop_with_nzb(&self, host: &QueueSubsysHost, nzb_ptr: NzbRef) {
        let info_name = self.lock_vol().info_name.clone();
        Self::global_log(MK_DEBUG, &[b"Stopping ", info_name.as_slice()].concat());

        let pp_stopped =
            crate::ffi::active_prepost().is_some_and(|pp| host.prepost_is_stopped(pp.owner_ptr()));
        if !pp_stopped {
            self.cancelled.store(true, Ordering::Release);
        }

        if self.processed.load(Ordering::Acquire) {
            let cancel_msg = trunc_1023([b"Cancelling ", info_name.as_slice()].concat());
            if nzb_ptr.is_some() {
                let c_msg = cstr::nul_terminated(&cancel_msg);
                Self::nzb_message(host, nzb_ptr, MK_WARNING, &c_msg);
            } else {
                Self::global_log(MK_WARNING, &cancel_msg);
            }
        }

        if nzb_ptr.is_some() {
            self.add_extra_time(host, nzb_ptr, true);
        }

        host.thread_stop(self.thread_ptr);

        if self.unpacking.load(Ordering::Acquire) {
            self.terminated.store(true, Ordering::Release);
        }
    }

    /// Picks up volumes released by a repair: a released volume the extractor is waiting for
    /// ends the wait, a released first volume is queued for extraction. A failed repair stops
    /// the thread.
    fn apply_releases(&self, host: &QueueSubsysHost) {
        let (released, abandoned) = direct_unpack_take_released(self.nzb_id);
        if abandoned && !self.cancelled.swap(true, Ordering::AcqRel) {
            host.thread_stop(self.thread_ptr);
            if self.unpacking.load(Ordering::Acquire) {
                self.terminated.store(true, Ordering::Release);
            }
            return;
        }
        if released.is_empty() {
            return;
        }
        let mut v = self.lock_vol();
        for filename in released {
            Self::global_log(
                MK_DEBUG,
                &[b"Released repaired volume ".as_slice(), filename.as_slice()].concat(),
            );
            if v.waiting_file
                .as_ref()
                .is_some_and(|wf| wf.eq_ignore_ascii_case(&filename))
            {
                v.waiting_file = None;
                self.set_waiting(false);
            }
            if crate::unpack::direct_unpack_is_main_archive(&filename) {
                v.archives.push_back(filename);
            }
        }
    }

    pub(crate) fn file_downloaded(&self, host: &QueueSubsysHost, fi_ptr: FileRef) {
        let fs = host.file_snap(fi_ptr);
        let nzb_ptr = fs.nzb_ptr;
        let nzb_name = if nzb_ptr.is_some() {
            cstr::until_nul(&host.nzb_snap(nzb_ptr, false).name).to_vec()
        } else {
            Vec::new()
        };
        let filename = cstr::until_nul(&fs.filename).to_vec();
        let failed_articles = fs.failed_articles;

        Self::global_log(
            MK_DEBUG,
            &[
                b"FileDownloaded for ",
                nzb_name.as_slice(),
                b"/",
                filename.as_slice(),
            ]
            .concat(),
        );

        if crate::filetypes::is_rar_file(&filename) && failed_articles > 0 {
            // A damaged volume pauses extraction until it is repaired; nothing is cancelled.
            let dest_dir = (nzb_ptr.is_some()).then(|| host.nzb_snap(nzb_ptr, false).dest_dir);
            let Some(dest_dir) = dest_dir else {
                self.stop_with_nzb(host, nzb_ptr);
                return;
            };
            let full = [
                dest_dir.as_slice(),
                &[crate::paths::PATH_SEPARATOR],
                filename.as_slice(),
            ]
            .concat();
            let skippable = direct_unpack_hole_view(&full).is_some_and(|h| !h.is_empty());
            if skippable {
                // Its lost ranges are known: extraction reads past them and the output is
                // patched once the volume is repaired.
                let mut v = self.lock_vol();
                if v.waiting_file
                    .as_ref()
                    .is_some_and(|wf| wf.eq_ignore_ascii_case(&filename))
                {
                    v.waiting_file = None;
                    self.set_waiting(false);
                }
                if crate::unpack::direct_unpack_is_main_archive(&filename) {
                    v.archives.push_back(filename);
                }
                return;
            }
            direct_unpack_hold(self.nzb_id, &full);
            let msg = trunc_1023(
                [
                    b"Pausing direct unpack at ".as_slice(),
                    filename.as_slice(),
                    b" until it is repaired",
                ]
                .concat(),
            );
            let c_msg = cstr::nul_terminated(&msg);
            Self::nzb_message(host, nzb_ptr, MK_INFO, &c_msg);
            return;
        }

        let mut v = self.lock_vol();
        if v.waiting_file
            .as_ref()
            .is_some_and(|wf| wf.eq_ignore_ascii_case(&filename))
        {
            v.waiting_file = None;
            self.set_waiting(false);
        }

        if crate::unpack::direct_unpack_is_main_archive(&filename) {
            v.archives.push_back(filename);
        }
    }

    pub(crate) fn nzb_downloaded(&self, host: &QueueSubsysHost, nzb_ptr: NzbRef) {
        // Extraction from here on is post-download work, also when it resumes after a
        // repair releases volumes parked below.
        self.busy
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .mark_download_done(std::time::Instant::now());
        let ns = host.nzb_snap(nzb_ptr, false);
        let nzb_name = cstr::until_nul(&ns.name).to_vec();
        let has_post_info = ns.has_post_info;
        Self::global_log(
            MK_DEBUG,
            &[b"NzbDownloaded for ", nzb_name.as_slice()].concat(),
        );

        let (waiting_missing, progress_label) = {
            let mut v = self.lock_vol();
            v.nzb_completed = true;
            direct_unpack_mark_downloaded(self.nzb_id);
            if direct_unpack_set_awaiting_repair(self.nzb_id) {
                drop(v);
                // Volumes are still on hold: post-processing repairs the download first, then
                // releases them and extraction continues where it paused.
                let msg = trunc_1023(
                    [
                        b"Direct unpack for ".as_slice(),
                        nzb_name.as_slice(),
                        b" waits for the repair of its damaged volumes",
                    ]
                    .concat(),
                );
                let c_msg = cstr::nul_terminated(&msg);
                Self::nzb_message(host, nzb_ptr, MK_INFO, &c_msg);
                self.add_extra_time(host, nzb_ptr, true);
                return;
            }
            if let Some(ref wf) = v.waiting_file {
                (Some(wf.clone()), Vec::new())
            } else {
                (None, v.progress_label.clone())
            }
        };

        if let Some(wf) = waiting_missing {
            let msg = trunc_1023([b"Could not find volume ", wf.as_slice()].concat());
            let c_msg = cstr::nul_terminated(&msg);
            Self::nzb_message(host, nzb_ptr, MK_WARNING, &c_msg);
            self.stop_with_nzb(host, nzb_ptr);
            return;
        }

        self.extra_start_time
            .store(current_time_sec(), Ordering::Release);
        if has_post_info {
            let c_lbl = cstr::nul_terminated(&progress_label);
            Self::post_progress_label(host, nzb_ptr, &c_lbl);
        }
    }

    pub(crate) fn nzb_deleted(&self, host: &QueueSubsysHost, nzb_ptr: NzbRef) {
        let ns = host.nzb_snap(nzb_ptr, false);
        let nzb_name = cstr::until_nul(&ns.name).to_vec();
        Self::global_log(
            MK_DEBUG,
            &[b"NzbDeleted for ", nzb_name.as_slice()].concat(),
        );
        Self::finish_nzb(host, nzb_ptr, DirectUnpackStatus::Failure);
        self.stop_with_nzb(host, nzb_ptr);
    }

    fn add_message(
        &self,
        host: &QueueSubsysHost,
        kind: i32,
        text: &[u8],
        local: &mut DirectUnpackRunLocal,
    ) {
        Self::global_log(MK_DEBUG, text);

        let info = crate::unpack::parse_unpack_message(text, &local.unpack_dir, false);
        let msg_text: Vec<u8> = if info.strip_extracting_dir != 0 {
            trunc_1023([b"Unrar: Extracting ", &text[info.stripped_suffix_offset..]].concat())
        } else {
            trunc_1023(text.to_vec())
        };

        if info.is_insert_disk != 0 {
            let filename = if info.insert_disk_name_len > 0
                && info.insert_disk_name_offset + info.insert_disk_name_len <= text.len()
            {
                trunc_1023(
                    text[info.insert_disk_name_offset
                        ..info.insert_disk_name_offset + info.insert_disk_name_len]
                        .to_vec(),
                )
            } else {
                Vec::new()
            };
            self.wait_next_volume(host, &filename, local);
            return;
        }

        host.with_locked_queue(|dq_ptr| {
            let nzb = self.find_nzb(host, dq_ptr);
            let nzb_ptr = nzb.as_ref().map_or(NzbRef::NONE, |n| n.ptr);
            let has_post_info = nzb.as_ref().is_some_and(|n| n.has_post_info);

            let c_msg = cstr::nul_terminated(&msg_text);
            if nzb_ptr.is_some() {
                Self::nzb_message(host, nzb_ptr, kind, &c_msg);
            } else {
                Self::global_log(kind, &msg_text);
            }

            if info.has_extracting_progress != 0 && nzb_ptr.is_some() && msg_text.len() >= 7 {
                let lbl = msg_text[7..].to_vec();
                self.lock_vol().progress_label = lbl.clone();
                if has_post_info {
                    let c_lbl = cstr::nul_terminated(&lbl);
                    Self::post_progress_label(host, nzb_ptr, &c_lbl);
                }
            }

            if info.has_extracting_from != 0 && nzb_ptr.is_some() && text.len() >= 7 {
                let lbl = text[7..].to_vec();
                self.lock_vol().progress_label = lbl.clone();
                if has_post_info {
                    let c_lbl = cstr::nul_terminated(&lbl);
                    Self::post_progress_label(host, nzb_ptr, &c_lbl);
                }
                if text.len() >= 23 {
                    local.extracted_archives.push(text[23..].to_vec());
                }
            }

            if !self.is_stopped(host)
                && (info.decrypt_error != 0
                    || info.direct_password_error != 0
                    || info.crc_or_volume_error != 0)
            {
                self.stop_with_nzb(host, nzb_ptr);
            }
        });

        if info.all_ok != 0 {
            local.all_ok_received = true;
        }
    }

    fn wait_next_volume(
        &self,
        host: &QueueSubsysHost,
        filename: &[u8],
        local: &mut DirectUnpackRunLocal,
    ) {
        Self::global_log(MK_DEBUG, &[b"WaitNextVolume for ", filename].concat());

        host.with_locked_queue(|dq_ptr| {
            if let Some(nzb) = self.find_nzb(host, dq_ptr) {
                let nzb_ptr = nzb.ptr;
                let cur_final = Self::final_dir_name(host, nzb_ptr);
                if nzb.dest_dir != local.dest_dir || cur_final != local.final_dir {
                    let cur_name = nzb.name;
                    let msg = trunc_1023(
                        [
                            b"Stopping direct unpack for ",
                            cur_name.as_slice(),
                            b" due to changed destination directory",
                        ]
                        .concat(),
                    );
                    let c_msg = cstr::nul_terminated(&msg);
                    Self::nzb_message(host, nzb_ptr, MK_WARNING, &c_msg);
                    self.stop_with_nzb(host, nzb_ptr);
                }
            }
        });

        let sep = crate::paths::PATH_SEPARATOR;
        let full_filename = [local.dest_dir.as_slice(), &[sep], filename].concat();
        let on_hold = direct_unpack_is_held_path(&full_filename);
        if !on_hold && crate::fsops::file_exists(&full_filename, &crate::fsops::posix_stat) {
            self.set_waiting(false);
        } else {
            let nzb_completed = {
                let mut v = self.lock_vol();
                v.waiting_file = Some(filename.to_vec());
                self.set_waiting(true);
                v.nzb_completed
            };
            if nzb_completed && !on_hold {
                let msg = [b"Could not find volume ", filename].concat();
                self.add_message(host, MK_WARNING, &msg, local);
                host.with_locked_queue(|dq_ptr| {
                    let nzb_ptr = self.find_nzb(host, dq_ptr).map_or(NzbRef::NONE, |n| n.ptr);
                    self.stop_with_nzb(host, nzb_ptr);
                });
            }
        }
    }

    /// Ends direct unpack after a typed extraction failure (wrong or missing password, damage
    /// that could not be repaired); post-processing takes over as before.
    fn stop_for_failure(
        &self,
        host: &QueueSubsysHost,
        failure: &crate::progressive_unpack::Failure,
    ) {
        Self::global_log(
            MK_DEBUG,
            format!("Direct unpack stopped: {failure}").as_bytes(),
        );
        host.with_locked_queue(|dq_ptr| {
            let nzb_ptr = self.find_nzb(host, dq_ptr).map_or(NzbRef::NONE, |n| n.ptr);
            if !self.is_stopped(host) {
                self.stop_with_nzb(host, nzb_ptr);
            }
        });
    }

    fn create_unpack_dir(&self, host: &QueueSubsysHost, local: &mut DirectUnpackRunLocal) {
        let sep = crate::paths::PATH_SEPARATOR;
        let use_inter_dir =
            crate::unpack::use_inter_dir(&local.dest_dir, &local.inter_dir_opt, sep);
        local.final_dir_created = use_inter_dir
            && !crate::fsops::directory_exists(&local.final_dir, &crate::fsops::posix_stat);

        let dest_dir = if use_inter_dir && !local.final_dir.is_empty() {
            local.final_dir.as_slice()
        } else {
            local.dest_dir.as_slice()
        };
        local.unpack_dir = if local.use_temp_unpack_dir {
            [dest_dir, &[sep], b"_unpack"].concat()
        } else {
            dest_dir.to_vec()
        };

        Self::global_log(
            MK_DETAIL,
            &[b"Unpacking into ", local.unpack_dir.as_slice()].concat(),
        );

        let mut errmsg = Vec::new();
        if !crate::fsops::force_directories(
            &local.unpack_dir,
            &mut errmsg,
            &crate::fsops::posix_stat,
        ) {
            let m = [
                b"Could not create directory ",
                local.unpack_dir.as_slice(),
                b": ",
                errmsg.as_slice(),
            ]
            .concat();
            self.add_message(host, MK_ERROR, &m, local);
        }
    }

    fn cleanup(&self, host: &QueueSubsysHost, local: &mut DirectUnpackRunLocal) {
        Self::global_log(
            MK_DEBUG,
            &[b"Cleanup for ", local.info_name.as_slice()].concat(),
        );
        let use_temp = local.use_temp_unpack_dir;
        let unpack_dir = local.unpack_dir.clone();
        let fin_created = local.final_dir_created;
        let final_dir = local.final_dir.clone();

        crate::unpack::direct_unpack_cleanup(
            use_temp,
            &unpack_dir,
            fin_created,
            &final_dir,
            &mut |kind, msg| {
                let mk = if kind == 1 { MK_ERROR } else { MK_INFO };
                self.add_message(host, mk, msg, local);
            },
        );
    }

    /// Extracts a 7z, ZIP or split-file set while it downloads (see `livefeed`). The set's
    /// last volume, which holds its directory, is moved up the download queue first. A set
    /// that cannot be extracted live is left to post-processing, which extracts it as before.
    fn execute_live(
        &self,
        host: &QueueSubsysHost,
        archive_name: &[u8],
        local: &mut DirectUnpackRunLocal,
    ) {
        use crate::livefeed::{Outcome, SetKind};
        local.all_ok_received = false;
        let names = host.with_locked_queue(|dq_ptr| {
            let Some(nzb) = self.find_nzb(host, dq_ptr) else {
                return Vec::new();
            };
            let mut names: Vec<Vec<u8>> = Vec::new();
            let mut queued = Vec::new();
            for i in 0..host.nzb_file_len(nzb.ptr) {
                let f = host.file_snap(host.nzb_file_at(nzb.ptr, i));
                if !f.deleted {
                    names.push(cstr::until_nul(&f.filename).to_vec());
                    queued.push(f);
                }
            }
            for i in 0..host.nzb_completed_files_len(nzb.ptr) {
                let f = host.nzb_completed_file_at(nzb.ptr, i);
                names.push(cstr::until_nul(&f.filename).to_vec());
            }
            if let Some(set) = crate::livefeed::live_set(archive_name, &names) {
                if set.kind != SetKind::Split {
                    for f in queued.iter().filter(|f| !f.extra_priority) {
                        if cstr::until_nul(&f.filename).eq_ignore_ascii_case(set.last()) {
                            host.file_set_int(
                                f.ptr,
                                crate::queuesubsys_host::FILE_INT_EXTRA_PRIORITY,
                                1,
                            );
                        }
                    }
                }
            }
            names
        });
        let Some(set) = crate::livefeed::live_set(archive_name, &names) else {
            // Not a set this download carries whole: post-processing looks at it.
            local.unpack_ok = true;
            return;
        };

        let _unpack_guard = UNPACK_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_stopped(host) {
            return;
        }
        self.set_waiting(false);
        self.exec_thread_key
            .store(current_thread_key(), Ordering::Release);
        self.set_unpacking(true);

        let dir = crate::fsops::path(&local.dest_dir).to_path_buf();
        let password = String::from_utf8_lossy(&local.password).into_owned();
        let mut feed = DirectUnpackFeed {
            nzb_id: self.nzb_id,
            expected_volumes: set.volumes.len() as u64,
        };
        let req = crate::livefeed::Request {
            dir: &dir,
            set: &set,
            password: &password,
        };
        let cell = std::cell::RefCell::new(&mut *local);
        let outcome = crate::livefeed::extract(
            &req,
            &mut feed,
            &mut || {
                let mut guard_local = cell.borrow_mut();
                let local = &mut **guard_local;
                if !self.processed.load(Ordering::Acquire) {
                    let m = [b"Directly unpacking ", local.name.as_slice()].concat();
                    self.add_message(host, MK_INFO, &m, local);
                    self.cleanup(host, local);
                    self.create_unpack_dir(host, local);
                    self.processed.store(true, Ordering::Release);
                }
                if local.unpack_dir.is_empty() {
                    return None;
                }
                let unpack_dir = local.unpack_dir.clone();
                let guard = local
                    .unpack_guard
                    .get_or_insert_with(|| {
                        let _ = crate::unpacklimits::take_handed_over(crate::fsops::path(
                            &unpack_dir,
                        ));
                        crate::unpacklimits::Guard::shared(
                            crate::fsops::path(&unpack_dir),
                            &[
                                crate::fsops::path(&local.dest_dir),
                                crate::fsops::path(&local.final_dir),
                            ],
                        )
                    })
                    .clone();
                Some((crate::fsops::path(&unpack_dir).to_path_buf(), Some(guard)))
            },
            &mut |kind, msg| {
                let msg_kind = match kind {
                    0 | 4 => MK_INFO,
                    1 => MK_ERROR,
                    2 => MK_WARNING,
                    _ => MK_DETAIL,
                };
                self.add_message(host, msg_kind, msg.as_bytes(), &mut cell.borrow_mut());
            },
            &mut || {
                self.apply_releases(host);
                self.is_stopped_or_terminated(host)
            },
            &|waiting| self.set_waiting(waiting),
        );
        drop(cell);

        match outcome {
            Outcome::Done { files, bytes, .. } => {
                Self::global_log(
                    MK_DETAIL,
                    format!(
                        "Direct unpack extracted {} live: {files} file(s), {bytes} bytes",
                        String::from_utf8_lossy(archive_name)
                    )
                    .as_bytes(),
                );
                self.add_message(host, MK_INFO, b"Unrar: All OK", local);
                local.unpack_ok = local.all_ok_received
                    && !self.terminated.load(Ordering::Acquire)
                    && !self.is_stopped(host);
            }
            Outcome::Declined(why) => {
                let msg = trunc_1023(
                    [
                        b"Direct unpack leaves ".as_slice(),
                        archive_name,
                        b" to post-processing: ",
                        why.as_bytes(),
                    ]
                    .concat(),
                );
                host.with_locked_queue(|dq_ptr| {
                    let nzb_ptr = self.find_nzb(host, dq_ptr).map_or(NzbRef::NONE, |n| n.ptr);
                    if nzb_ptr.is_some() {
                        Self::nzb_message(host, nzb_ptr, MK_INFO, &cstr::nul_terminated(&msg));
                    }
                });
                local.unpack_ok =
                    !self.terminated.load(Ordering::Acquire) && !self.is_stopped(host);
            }
            Outcome::Failed(why) => {
                Self::global_log(
                    MK_DEBUG,
                    format!("Direct unpack stopped: {why}").as_bytes(),
                );
                local.unpack_ok = false;
            }
        }

        self.set_unpacking(false);
        self.exec_thread_key.store(0, Ordering::Release);
        self.set_waiting(false);
    }

    fn execute_unrar(
        &self,
        host: &QueueSubsysHost,
        archive_name: &[u8],
        local: &mut DirectUnpackRunLocal,
    ) {
        local.all_ok_received = false;

        let _unpack_guard = UNPACK_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_stopped(host) {
            return;
        }
        self.set_waiting(false);
        self.exec_thread_key
            .store(current_thread_key(), Ordering::Release);
        self.set_unpacking(true);

        let sep = crate::paths::PATH_SEPARATOR;
        let full_archive_path =
            trunc_1023([local.dest_dir.as_slice(), &[sep], archive_name].concat());
        let unpack_dir = local.unpack_dir.clone();
        let pw_bytes = local.password.clone();
        let pw_str = std::str::from_utf8(&pw_bytes).unwrap_or("");
        let cleanup_disk = local.unpack_cleanup_disk;

        let mut failure: Option<crate::progressive_unpack::Failure> = None;
        // Set while the progressive session itself is parked on a volume: it logs from inside
        // its wait (a volume patched in line, say), and holding it there would never end.
        let session_parked = AtomicBool::new(false);
        let guard = local
            .unpack_guard
            .get_or_insert_with(|| {
                // A guard an earlier run left for this folder is stale.
                let _ = crate::unpacklimits::take_handed_over(crate::fsops::path(&unpack_dir));
                crate::unpacklimits::Guard::shared(
                    crate::fsops::path(&unpack_dir),
                    &[
                        crate::fsops::path(&local.dest_dir),
                        crate::fsops::path(&local.final_dir),
                    ],
                )
            })
            .clone();
        let feed = Box::new(DirectUnpackFeed {
            nzb_id: self.nzb_id,
            expected_volumes: local.expected_volumes,
        });
        let res = crate::unpack::extract_archive_with_feed(
            crate::fsops::path(&full_archive_path),
            crate::fsops::path(&unpack_dir),
            pw_str,
            1,
            cleanup_disk,
            |kind, msg| {
                let msg_kind = match kind {
                    0 | 4 => MK_INFO,
                    1 => MK_ERROR,
                    2 => MK_WARNING,
                    _ => MK_DETAIL,
                };
                self.add_message(host, msg_kind, msg.as_bytes(), local);
                // A volume wait signalled through a log line holds the extractor here; RAR sets
                // read by the progressive session wait in the session and never block here.
                while self.waiting_volume.load(Ordering::Acquire)
                    && !session_parked.load(Ordering::Acquire)
                    && !self.is_stopped_or_terminated(host)
                {
                    std::thread::sleep(Duration::from_millis(20));
                    self.apply_releases(host);
                }
            },
            &mut || {
                self.apply_releases(host);
                self.is_stopped_or_terminated(host)
            },
            crate::unpack::RarFeed {
                feed: Some(feed),
                failure: Some(&mut failure),
                // A parked session counts as waiting, as the volume handshake did, so reloads
                // are not deferred behind it.
                on_wait: Some(&|waiting| {
                    session_parked.store(waiting, Ordering::Release);
                    self.set_waiting(waiting);
                }),
                guard: Some(&guard),
            },
        );
        if let Some(f) = &failure {
            if failure_locks_archive(f, &local.password, crate::options::unpack_pass_file_set()) {
                locked_archives().insert(self.nzb_id);
            }
            if f.is_password() || f.is_damage() {
                self.stop_for_failure(host, f);
            }
        }

        self.set_unpacking(false);
        self.exec_thread_key.store(0, Ordering::Release);
        self.set_waiting(false);
        drop(_unpack_guard);

        local.unpack_ok = res.is_ok()
            && local.all_ok_received
            && !self.terminated.load(Ordering::Acquire)
            && !self.is_stopped(host);
    }

    pub(crate) fn run(&self, host: &QueueSubsysHost) {
        Self::global_log(
            MK_DEBUG,
            format!("Entering DirectUnpack-loop for {}", self.nzb_id).as_bytes(),
        );
        // a mark from an earlier run (before a password was set) is stale
        locked_archives().remove(&self.nzb_id);

        let mut local = DirectUnpackRunLocal {
            name: Vec::new(),
            info_name: Vec::new(),
            info_name_up: Vec::new(),
            dest_dir: Vec::new(),
            final_dir: Vec::new(),
            unpack_dir: Vec::new(),
            password: Vec::new(),
            inter_dir_opt: Vec::new(),
            extracted_archives: Vec::new(),
            all_ok_received: false,
            unpack_ok: false,
            final_dir_created: false,
            use_temp_unpack_dir: false,
            unpack_cleanup_disk: false,
            expected_volumes: 0,
            unpack_guard: None,
        };

        let found = host.with_locked_queue(|dq_ptr| {
            let Some(nzb) = self.find_nzb(host, dq_ptr) else {
                return false;
            };
            let nzb_ptr = nzb.ptr;
            local.name = nzb.name;
            local.dest_dir = nzb.dest_dir;
            local.final_dir = Self::final_dir_name(host, nzb_ptr);
            if let Some(pw) = nzb.password {
                local.password = pw;
            }
            local.inter_dir_opt = nzb.inter_dir_opt;
            local.use_temp_unpack_dir = nzb.use_temp_unpack_dir;
            local.unpack_cleanup_disk = nzb.unpack_cleanup_disk;
            // The set is still downloading: count every RAR file the queue
            // knows of, so the session's volume budget covers the whole set.
            {
                let queued = (0..host.nzb_file_len(nzb_ptr))
                    .map(|i| host.file_snap(host.nzb_file_at(nzb_ptr, i)))
                    .filter(|f| !f.deleted && crate::filetypes::is_rar_file(&f.filename))
                    .count();
                let completed = (0..host.nzb_completed_files_len(nzb_ptr))
                    .map(|i| host.nzb_completed_file_at(nzb_ptr, i))
                    .filter(|f| crate::filetypes::is_rar_file(&f.filename))
                    .count();
                local.expected_volumes = (queued + completed) as u64;
            }
            true
        });

        if !found {
            Self::global_log(
                MK_DEBUG,
                format!("Could not find NzbInfo for {}", self.nzb_id).as_bytes(),
            );
            self.set_unpacking(false);
            return;
        }

        local.info_name = [b"direct unpack for ", local.name.as_slice()].concat();
        local.info_name_up = [b"Direct unpack for ", local.name.as_slice()].concat();

        {
            let mut v = self.lock_vol();
            v.info_name = local.info_name.clone();
            for arch in crate::unpack::direct_unpack_find_archive_files(&local.dest_dir) {
                let full = [
                    local.dest_dir.as_slice(),
                    &[crate::paths::PATH_SEPARATOR],
                    arch.as_slice(),
                ]
                .concat();
                if direct_unpack_hole_view(&full).is_none_or(|h| !h.is_empty()) {
                    v.archives.push_back(arch);
                }
            }
        }

        let mut started: Vec<Vec<u8>> = Vec::new();
        while !self.is_stopped(host) {
            let next_archive = self.lock_vol().archives.pop_front();
            if let Some(archive) = next_archive {
                // A main volume extracted past its damage is queued again when its repair
                // releases it; the session already handled it.
                if started.iter().any(|a| a.eq_ignore_ascii_case(&archive)) {
                    continue;
                }
                started.push(archive.clone());
                self.set_waiting(false);
                // A 7z, ZIP or split-file set prepares its unpack folder only once it is known
                // to be extracted live.
                let live = crate::livefeed::is_live_first(&archive);
                if !live && !self.processed.load(Ordering::Acquire) {
                    let m = [b"Directly unpacking ", local.name.as_slice()].concat();
                    self.add_message(host, MK_INFO, &m, &mut local);
                    self.cleanup(host, &mut local);
                    self.create_unpack_dir(host, &mut local);
                    self.processed.store(true, Ordering::Release);
                }

                let committed = local.extracted_archives.len();
                if live {
                    self.execute_live(host, &archive, &mut local);
                } else {
                    self.execute_unrar(host, &archive, &mut local);
                }
                if !local.unpack_ok {
                    local.extracted_archives.truncate(committed);
                    break;
                }
            } else {
                if self.lock_vol().nzb_completed && !direct_unpack_has_holds(self.nzb_id) {
                    // Nothing queued, nothing on hold: one last look for a volume released
                    // right now, then done.
                    self.apply_releases(host);
                    if self.lock_vol().archives.is_empty() {
                        break;
                    }
                    continue;
                }
                self.set_waiting(true);
                std::thread::sleep(Duration::from_millis(100));
                self.apply_releases(host);
            }
        }

        self.set_unpacking(false);
        self.set_waiting(false);

        let cancelled = self.cancelled.load(Ordering::Acquire);
        let interrupted = !cancelled && self.is_stopped_or_terminated(host);
        // Names are ASCII only. A temporary unpack folder is renamed by post-processing, which
        // counts what is in it; files unpacked next to the download are renamed now.
        if local.unpack_ok && !interrupted && !cancelled {
            // Post-processing continues this count.
            if let Some(guard) = &local.unpack_guard {
                crate::unpacklimits::hand_over(guard);
            }
        }
        if local.unpack_ok && !interrupted && !cancelled && !local.use_temp_unpack_dir {
            if let Some(guard) = local.unpack_guard.clone() {
                let mut lines: Vec<(i32, String)> = Vec::new();
                let named = crate::unpacklimits::finish_names(&guard, &mut |k, m| {
                    lines.push((k, m.to_string()));
                });
                for (k, m) in lines {
                    let kind = if k == 1 { MK_ERROR } else { MK_INFO };
                    self.add_message(host, kind, m.as_bytes(), &mut local);
                }
                if named.is_err() {
                    local.unpack_ok = false;
                }
            }
        }
        if (!local.unpack_ok && !interrupted) || cancelled {
            self.cleanup(host, &mut local);
        }

        let mut extracted = false;
        let valid_owner = host.with_locked_queue(|dq_ptr| {
            let Some(nzb) = self.find_nzb(host, dq_ptr).filter(|n| n.owned) else {
                return false;
            };
            let nzb_ptr = nzb.ptr;

            let processed = self.processed.load(Ordering::Acquire);
            let status = if !processed || interrupted {
                DirectUnpackStatus::None
            } else if local.unpack_ok && !cancelled {
                DirectUnpackStatus::Success
            } else {
                DirectUnpackStatus::Failure
            };

            if interrupted {
                Self::global_log(
                    MK_DEBUG,
                    &[
                        local.info_name.as_slice(),
                        b" stopped due to reload/shutdown, preserving unpack state",
                    ]
                    .concat(),
                );
            } else if status != DirectUnpackStatus::None {
                let ok = status == DirectUnpackStatus::Success;
                let suffix: &[u8] = if ok { b" successful" } else { b" failed" };
                let msg = trunc_1023([local.info_name_up.as_slice(), suffix].concat());
                let c_msg = cstr::nul_terminated(&msg);
                Self::nzb_message(host, nzb_ptr, if ok { MK_INFO } else { MK_WARNING }, &c_msg);
            }

            if status == DirectUnpackStatus::Success || interrupted {
                for arch in &local.extracted_archives {
                    let c_arch = cstr::nul_terminated(arch);
                    host.nzb_push_extracted_archive(nzb_ptr, cstr::until_nul(&c_arch));
                }
            }

            self.add_extra_time(host, nzb_ptr, true);

            extracted = status == DirectUnpackStatus::Success;
            Self::finish_nzb(host, nzb_ptr, status);
            true
        });

        if !valid_owner {
            Self::global_log(
                MK_DEBUG,
                &[b"Could not find NzbInfo for ", local.info_name.as_slice()].concat(),
            );
            return;
        }

        if extracted || cancelled {
            // Direct unpack read the borrowed dead volumes, or the download is going away:
            // nothing else needs them.
            direct_unpack_forget_holds(self.nzb_id);
        } else {
            // Post-processing repairs and unpacks the download: its borrowed dead volumes
            // are written as real volumes then, never fetched again.
            direct_unpack_forget_holds_keep_borrows(self.nzb_id);
        }
        Self::global_log(
            MK_DEBUG,
            format!("Exiting DirectUnpack-loop for {}", self.nzb_id).as_bytes(),
        );
    }

    /// Runs `f` with the active queue host; does nothing without one.
    #[cfg(unix)]
    fn with_active_host(&self, f: impl FnOnce(&QueueSubsysHost)) {
        if let Some(host) = crate::ffi::queue_host() {
            f(&host);
        }
    }

    /// Runs the direct unpack on its thread.
    pub(crate) fn run_active(&self) {
        #[cfg(unix)]
        self.with_active_host(|host| self.run(host));
    }

    /// Stops the direct unpack of the collection `nzb`.
    pub(crate) fn stop_active(&self, nzb: NzbRef) {
        #[cfg(unix)]
        self.with_active_host(|host| self.stop_with_nzb(host, nzb));
        #[cfg(not(unix))]
        let _ = nzb;
    }

    /// Tells the direct unpack that `file` finished downloading.
    pub(crate) fn file_downloaded_active(&self, file: FileRef) {
        #[cfg(unix)]
        self.with_active_host(|host| self.file_downloaded(host, file));
        #[cfg(not(unix))]
        let _ = file;
    }

    /// Tells the direct unpack that the collection `nzb` finished downloading.
    pub(crate) fn nzb_downloaded_active(&self, nzb: NzbRef) {
        #[cfg(unix)]
        self.with_active_host(|host| self.nzb_downloaded(host, nzb));
        #[cfg(not(unix))]
        let _ = nzb;
    }

    /// Tells the direct unpack that the collection `nzb` was deleted.
    pub(crate) fn nzb_deleted_active(&self, nzb: NzbRef) {
        #[cfg(unix)]
        self.with_active_host(|host| self.nzb_deleted(host, nzb));
        #[cfg(not(unix))]
        let _ = nzb;
    }
}

// ============================================================================
// 4. Move, Cleanup, Rename & PostRenamer Controllers
// ============================================================================

fn map_cleanup_kind(kind: crate::cleanup::CleanupLogKind) -> i32 {
    match kind {
        crate::cleanup::CleanupLogKind::Info => MK_INFO,
        crate::cleanup::CleanupLogKind::Warning => MK_WARNING,
        crate::cleanup::CleanupLogKind::Error => MK_ERROR,
        crate::cleanup::CleanupLogKind::Detail => MK_DETAIL,
    }
}

#[cfg(unix)]
impl NzbgetRsPostJob {
    fn nzb_ptr(&self, host: &crate::queuesubsys_host::QueueSubsysHost) -> NzbRef {
        host.post_snap(self.post_ptr).nzb_ptr
    }

    fn add_message(&self, host: &crate::queuesubsys_host::QueueSubsysHost, kind: i32, msg: &[u8]) {
        host.nzb_add_message(self.nzb_ptr(host), kind, msg);
    }

    fn run_move(&self) {
        use crate::queuesubsys_host::{NZB_STR_DEST_DIR, NZB_STR_FINAL_DIR, POST_INT_WORKING};
        let Some(host) = crate::ffi::queue_host() else {
            return;
        };
        let host = &host;
        let (nzb_name, inter_dir, dest_dir) = host.with_locked_queue(|_| {
            let n = self.nzb_ptr(host);
            let ns = host.nzb_snap(n, false);
            let dest = if ns.final_dir.is_empty() {
                host.nzb_build_final_dir_name(n)
            } else {
                ns.final_dir
            };
            (trunc_1023(ns.name), ns.dest_dir, dest)
        });

        self.add_message(
            host,
            MK_INFO,
            &[b"Moving completed files for ", nzb_name.as_slice()].concat(),
        );

        let inter_path = crate::cleanup::bytes_to_path_buf(&inter_dir);
        let dest_path = crate::cleanup::bytes_to_path_buf(&dest_dir);
        let is_stopped = || host.thread_is_stopped(self.thread_ptr);
        let ok = crate::cleanup::move_files(
            &inter_path,
            &dest_path,
            is_stopped,
            |kind, msg| self.add_message(host, map_cleanup_kind(kind), msg),
            |old_rel, new_rel| {
                host.with_locked_queue(|_| {
                    if self.post_ptr.is_none() {
                        return;
                    }
                    let n = self.nzb_ptr(host);
                    if n.is_some() {
                        host.nzb_rename_completed_file(
                            n,
                            cstr::until_nul(old_rel),
                            cstr::until_nul(new_rel),
                        );
                    }
                });
            },
        );

        let up_info_name = trunc_1023([b"Move for ", nzb_name.as_slice()].concat());

        if ok {
            let hard_links = host.nzb_hard_link_path(self.nzb_ptr(host));
            if !hard_links.is_empty() {
                let hl_path = crate::cleanup::bytes_to_path_buf(&hard_links);
                crate::cleanup::remove_stale_hardlinks(&hl_path, &dest_path, |kind, msg| {
                    self.add_message(host, map_cleanup_kind(kind), msg);
                });
            }
        }

        host.with_locked_queue(|_| {
            if self.post_ptr.is_none() {
                return;
            }
            let n = self.nzb_ptr(host);
            if n.is_none() {
                return;
            }
            let (outcome, move_status) =
                crate::cleanup::decide_move_outcome(ok, is_stopped());
            if outcome == 1 {
                self.add_message(
                    host,
                    MK_INFO,
                    &[up_info_name.as_slice(), b" successful"].concat(),
                );
                host.nzb_set_str(n, NZB_STR_DEST_DIR, cstr::until_nul(&dest_dir));
                host.nzb_set_str(n, NZB_STR_FINAL_DIR, b"");
                host.nzb_set_move_status(n, move_status);
            } else if outcome == 2 {
                self.add_message(
                    host,
                    MK_INFO,
                    &[
                        up_info_name.as_slice(),
                        b" interrupted, resumes after the restart",
                    ]
                    .concat(),
                );
            } else {
                self.add_message(
                    host,
                    MK_ERROR,
                    &[up_info_name.as_slice(), b" failed"].concat(),
                );
                host.nzb_set_move_status(n, move_status);
            }
            host.post_set_int(self.post_ptr, POST_INT_WORKING, 0);
        });
    }

    fn run_cleanup(&self) {
        use crate::queuesubsys_host::POST_INT_WORKING;
        let Some(host) = crate::ffi::queue_host() else {
            return;
        };
        let host = &host;
        let (nzb_name, dest_dir, final_dir, ext_cleanup_disk) = host.with_locked_queue(|_| {
            let ns = host.nzb_snap(self.nzb_ptr(host), false);
            let ext = crate::options::active_options()
                .map(|o| o.str_bytes(OptStr::ExtCleanupDisk).to_vec())
                .unwrap_or_default();
            (trunc_1023(ns.name), ns.dest_dir, ns.final_dir, ext)
        });

        self.add_message(host, MK_INFO, &[b"Cleaning up ", nzb_name.as_slice()].concat());

        let dst_path = crate::cleanup::bytes_to_path_buf(&dest_dir);
        let fin_path = crate::cleanup::bytes_to_path_buf(&final_dir);
        let (outcome, cleanup_status) =
            crate::cleanup::run_cleanup(&dst_path, &fin_path, &ext_cleanup_disk, |kind, msg| {
                self.add_message(host, map_cleanup_kind(kind), msg);
            });

        let up_info_name = trunc_1023([b"Cleanup for ", nzb_name.as_slice()].concat());
        if outcome == 1 {
            self.add_message(
                host,
                MK_INFO,
                &[up_info_name.as_slice(), b" successful"].concat(),
            );
        } else if outcome == 2 {
            self.add_message(
                host,
                MK_INFO,
                &[b"Nothing to cleanup for ", nzb_name.as_slice()].concat(),
            );
        } else {
            self.add_message(
                host,
                MK_ERROR,
                &[up_info_name.as_slice(), b" failed"].concat(),
            );
        }

        host.nzb_set_cleanup_status(self.nzb_ptr(host), cleanup_status);
        host.post_set_int(self.post_ptr, POST_INT_WORKING, 0);
    }
}

#[cfg(unix)]
struct RenameRarCallbacks<'a> {
    job: &'a NzbgetRsPostJob,
    host: &'a crate::queuesubsys_host::QueueSubsysHost,
    renamed_count: i32,
}

#[cfg(unix)]
impl crate::rarrenamer::RunCallbacks for RenameRarCallbacks<'_> {
    fn stopped(&mut self) -> Result<bool, ()> {
        Ok(self.host.thread_is_stopped(self.job.thread_ptr))
    }

    fn progress(&mut self, label: &[u8], stage_progress: i32) -> Result<(), ()> {
        self.job.set_progress(self.host, label, stage_progress);
        Ok(())
    }

    fn message(&mut self, kind: i32, text: &[u8]) -> Result<(), ()> {
        self.job.add_message(self.host, renamer_msg_kind(kind), text);
        Ok(())
    }

    fn register_renamed(&mut self, old_base: &[u8], new_base: &[u8]) -> Result<(), ()> {
        self.job
            .register_renamed(self.host, old_base, new_base, &mut self.renamed_count);
        Ok(())
    }
}

/// Maps a renamer message level (0 info, 1 warning, else error) to a message kind.
fn renamer_msg_kind(kind: i32) -> i32 {
    match kind {
        0 => MK_INFO,
        1 => MK_WARNING,
        _ => MK_ERROR,
    }
}

#[cfg(unix)]
impl NzbgetRsPostJob {
    fn is_stopped(&self, host: &crate::queuesubsys_host::QueueSubsysHost) -> bool {
        host.thread_is_stopped(self.thread_ptr)
    }

    fn set_progress(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        label: &[u8],
        stage_progress: i32,
    ) {
        use crate::queuesubsys_host::POST_INT_STAGE_PROGRESS;
        host.with_locked_queue(|_| {
            host.post_set_progress_label(self.post_ptr, cstr::until_nul(label));
            host.post_set_int(self.post_ptr, POST_INT_STAGE_PROGRESS, stage_progress);
        });
    }

    /// Records a renamed completed file, warning when the queue has no entry for it.
    fn register_renamed(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        old_fn: &[u8],
        new_fn: &[u8],
        renamed_count: &mut i32,
    ) {
        let found = host.with_locked_queue(|_| {
            host.nzb_rename_completed_file(
                self.nzb_ptr(host),
                cstr::until_nul(old_fn),
                cstr::until_nul(new_fn),
            )
        });
        if !found {
            self.add_message(
                host,
                MK_WARNING,
                &[
                    b"Could not find completed-file entry for ",
                    old_fn,
                    b" while renaming to ",
                    new_fn,
                ]
                .concat(),
            );
        }
        *renamed_count += 1;
    }

    /// Par-rename (`kind` 0) or rar-rename (`kind` 1) of the downloaded files.
    fn run_rename(&self, kind: i32) {
        use crate::queuesubsys_host::{
            EA_GROUP_PAUSE_EXTRA_PARS, EA_GROUP_RESUME, NZB_INT_PAR_RENAME_STATUS,
            NZB_INT_RAR_RENAME_STATUS, POST_INT_REQUEST_PAR_CHECK, POST_INT_WORKING,
        };
        let Some(host) = crate::ffi::queue_host() else {
            return;
        };
        let host = &host;
        let parcheck_enabled = crate::ffi::build_parcheck_enabled();
        let tls_enabled = !crate::ffi::build_tls_disabled();

        let (
            (nzb_name, dest_dir, final_dir, password, par_ignore_ext, unpack_ignore_ext, unpack_status),
            known_hash16k,
        ) = host.with_locked_queue(|_| {
                let n = self.nzb_ptr(host);
                let ns = host.nzb_snap(n, false);
                // The first-16-KiB MD5 of every file downloaded whole, as its first article
                // gave it: par-rename identifies those without reading them.
                // (The analyzer sets hash16k only for a first article whose NZB size is at
                // least 16 KiB, or a file's only article: the MD5 of min(16 KiB, file size),
                // except for a first article that decodes to just under 16 KiB, where the
                // hash is short and par-rename misses the file as DirectRename already does.)
                // A name two completed files share is left to the renamer's own read.
                let mut known: Vec<(Vec<u8>, [u8; 16])> = (0..host.nzb_completed_files_len(n))
                    .map(|i| host.nzb_completed_file_at(n, i))
                    .filter(|f| f.status == crate::queuesubsys_host::CF_SUCCESS && !f.par_file)
                    .filter_map(|f| Some((f.filename, crate::parrenamer::parse_hash16k(&f.hash16k)?)))
                    .collect();
                let names: Vec<Vec<u8>> = known.iter().map(|(name, _)| name.clone()).collect();
                known.retain(|(name, _)| names.iter().filter(|other| *other == name).count() == 1);
                let password = host.nzb_get_param(n, b"*Unpack:Password").unwrap_or_default();
                let opts = crate::options::active_options();
                let opt = |id| opts.as_ref().map_or(&b""[..], |o| o.str_bytes(id)).to_vec();
                (
                    (
                        trunc_1023(ns.name),
                        ns.dest_dir,
                        ns.final_dir,
                        cstr::until_nul(&password).to_vec(),
                        opt(OptStr::ParIgnoreExt),
                        opt(OptStr::UnpackIgnoreExt),
                        ns.unpack_status,
                    ),
                    known,
                )
            });

        let archive_prefix: &[u8] = if kind == 1 { b"archive " } else { b"" };
        self.add_message(
            host,
            MK_INFO,
            &[
                b"Checking renamed ",
                archive_prefix,
                b"files for ",
                nzb_name.as_slice(),
            ]
            .concat(),
        );

        let mut renamed_count = 0i32;
        let mut has_missed_files = false;
        let mut has_damaged_par_files = false;

        if kind == 0 {
            if parcheck_enabled {
                let (use_final, detect_missing) =
                    crate::cleanup::rename_eval_par_config(unpack_status, final_dir.is_empty());
                let target_dir = if use_final { &final_dir } else { &dest_dir };
                let mut renamed_to: Vec<Vec<u8>> = Vec::new();
                // The hashes name files in the download folder; the final folder after an
                // unpack holds other files.
                let known: &[(Vec<u8>, [u8; 16])] =
                    if use_final { &[] } else { &known_hash16k };
                let state = crate::parrenamer::execute_all_known(
                    &nzb_name,
                    target_dir,
                    &par_ignore_ext,
                    detect_missing,
                    known,
                    &mut || self.is_stopped(host),
                    &mut |label: &[u8], stage: i32| self.set_progress(host, label, stage),
                    &mut |mk: crate::parrenamer::Kind, text: &[u8]| {
                        self.add_message(host, renamer_msg_kind(mk as i32), text);
                    },
                    &mut |name: &[u8]| {
                        host.post_push_parred_file(self.post_ptr, cstr::until_nul(name));
                    },
                    &mut |old_rel: &[u8], new_rel: &[u8]| {
                        self.register_renamed(host, old_rel, new_rel, &mut renamed_count);
                        renamed_to.push(new_rel.to_vec());
                    },
                );
                has_missed_files = state.has_missed_files;
                has_damaged_par_files = state.has_damaged_par_files;
                // After an unpack, names are ASCII only (`unpacklimits`): a par rename back to a
                // non-ASCII name is renamed again.
                if unpack_status == UnpackStatus::Success {
                    for rel in renamed_to {
                        self.ascii_rename(host, target_dir, &rel, &mut renamed_count);
                    }
                }
            }
        } else if kind == 1 {
            let mut cbs = RenameRarCallbacks {
                job: self,
                host,
                renamed_count: 0,
            };
            let mut ignored_out = 0;
            let _ = crate::rarrenamer::execute_all(
                &dest_dir,
                &nzb_name,
                &password,
                &unpack_ignore_ext,
                tls_enabled,
                &mut ignored_out,
                &mut cbs,
            );
            renamed_count = cbs.renamed_count;
        }

        let run_outcome =
            crate::cleanup::rename_classify_run_outcome(self.is_stopped(host), renamed_count);
        let summary = match run_outcome {
            1 => [b"Renaming cancelled for ", nzb_name.as_slice()].concat(),
            2 => [
                format!("Successfully renamed {renamed_count} ").as_bytes(),
                archive_prefix,
                b"file(s) for ",
                nzb_name.as_slice(),
            ]
            .concat(),
            _ => [
                b"No renamed ",
                archive_prefix,
                b"files found for ",
                nzb_name.as_slice(),
            ]
            .concat(),
        };
        self.add_message(host, if run_outcome == 1 { MK_WARNING } else { MK_INFO }, &summary);

        host.with_locked_queue(|dq_ptr| {
            let n = self.nzb_ptr(host);
            let ns = host.nzb_snap(n, false);
            let (initial_status, req_extra, req_par_check) = crate::cleanup::rename_complete_action(
                kind,
                renamed_count,
                has_damaged_par_files,
                ns.remaining_par_count,
                has_missed_files,
                ns.par_status,
            );

            let mut final_status = initial_status;
            if kind == 0 && parcheck_enabled && req_extra {
                self.add_message(
                    host,
                    MK_INFO,
                    &[
                        b"Requesting extra par2-files for ",
                        nzb_name.as_slice(),
                        b" to perform par-rename",
                    ]
                    .concat(),
                );
                host.nzb_set_int(n, NZB_INT_PAR_RENAME_STATUS, initial_status);
                host.dq_edit_entry(dq_ptr, ns.id, EA_GROUP_RESUME, None);
                host.dq_edit_entry(dq_ptr, ns.id, EA_GROUP_PAUSE_EXTRA_PARS, None);
                let rem_sz = host.nzb_snap(n, false).remaining_size;
                if crate::cleanup::rename_should_reset_par_status(rem_sz) {
                    final_status = 0; // rsNone
                }
            }

            let do_req_par = parcheck_enabled && req_par_check;
            if do_req_par {
                self.add_message(
                    host,
                    MK_INFO,
                    &[
                        b"Requesting par-check/repair for ",
                        nzb_name.as_slice(),
                        b" to restore missing files ",
                    ]
                    .concat(),
                );
            }

            match kind {
                0 => host.nzb_set_int(n, NZB_INT_PAR_RENAME_STATUS, final_status),
                1 => host.nzb_set_int(n, NZB_INT_RAR_RENAME_STATUS, final_status),
                _ => {}
            }
            if do_req_par {
                host.post_set_int(self.post_ptr, POST_INT_REQUEST_PAR_CHECK, 1);
            }
            host.post_set_int(self.post_ptr, POST_INT_WORKING, 0);
        });
    }

    /// Renames a par-renamed file under `target_dir` again when its name is not ASCII.
    fn ascii_rename(
        &self,
        host: &crate::queuesubsys_host::QueueSubsysHost,
        target_dir: &[u8],
        rel: &[u8],
        renamed_count: &mut i32,
    ) {
        let sep = crate::paths::PATH_SEPARATOR;
        let full = [target_dir, &[sep], rel].concat();
        match crate::unpacklimits::ascii_rename(crate::fsops::path(&full)) {
            Ok(Some(new)) => {
                let new_rel = new
                    .strip_prefix(crate::fsops::path(target_dir))
                    .map(|p| crate::fsops::path_to_bytes(p).to_vec())
                    .unwrap_or_default();
                if !new_rel.is_empty() {
                    self.register_renamed(host, rel, &new_rel, renamed_count);
                }
                let m = format!(
                    "Renamed {} to {} (names are ASCII only)",
                    String::from_utf8_lossy(&full),
                    new.display()
                );
                self.add_message(host, MK_INFO, m.as_bytes());
            }
            Ok(None) => {}
            Err(e) => {
                let m = format!(
                    "Could not rename {} to an ASCII name: {e}",
                    String::from_utf8_lossy(&full)
                );
                self.add_message(host, MK_ERROR, m.as_bytes());
            }
        }
    }

    /// Post-download (`is_post_unpack` false) or post-unpack renaming of obfuscated files.
    fn run_post_renamer(&self, is_post_unpack: bool) {
        use crate::queuesubsys_host::{
            NZB_INT_POST_DOWNLOAD_RENAMING_STATUS, NZB_INT_POST_UNPACK_RENAMING_STATUS,
            POST_INT_WORKING,
        };
        let Some(host) = crate::ffi::queue_host() else {
            return;
        };
        let host = &host;
        let (meta_name, nzb_name, dst_dir, ignore_ext) = host.with_locked_queue(|_| {
            let n = self.nzb_ptr(host);
            let ns = host.nzb_snap(n, false);
            let opts = crate::options::active_options();
            (
                host.nzb_meta_name(n),
                ns.name,
                ns.dest_dir,
                opts.as_ref()
                    .map_or(&b""[..], |o| o.str_bytes(OptStr::RenameIgnoreExt))
                    .to_vec(),
            )
        });

        let dst_path = crate::cleanup::bytes_to_path_buf(&dst_dir);
        let status = crate::cleanup::run_post_renamer(
            is_post_unpack,
            &meta_name,
            &nzb_name,
            &dst_path,
            &ignore_ext,
            |_| {},
            || self.is_stopped(host),
            |kind, msg| self.add_message(host, map_cleanup_kind(kind), msg),
            |old_name, new_name| {
                host.with_locked_queue(|_| {
                    if self.post_ptr.is_none() {
                        return;
                    }
                    let n = self.nzb_ptr(host);
                    if n.is_some() {
                        host.nzb_rename_completed_file(
                            n,
                            cstr::until_nul(old_name),
                            cstr::until_nul(new_name),
                        );
                    }
                });
            },
        );

        host.with_locked_queue(|_| {
            let n = self.nzb_ptr(host);
            let field = if is_post_unpack {
                NZB_INT_POST_UNPACK_RENAMING_STATUS
            } else {
                NZB_INT_POST_DOWNLOAD_RENAMING_STATUS
            };
            host.nzb_set_int(n, field, status);
            host.post_set_int(self.post_ptr, POST_INT_WORKING, 0);
        });
    }
}

/// Marks the post-processing of `post_ptr` as finished (`PostInfo::ptFinished`).
#[cfg(unix)]
fn finish_post_job(post_ptr: crate::queuesubsys_host::PostRef) {
    use crate::queuesubsys_host::POST_INT_WORKING;
    let Some(host) = crate::ffi::queue_host() else {
        return;
    };
    host.post_set_stage(post_ptr, PostStage::Finished);
    host.post_set_int(post_ptr, POST_INT_WORKING, 0);
}

#[cfg(unix)]
enum PostJobInner {
    Rename(c_int),
    StreamRepair(crate::streamrepair_coord::NzbgetRsStreamRepairCtrl),
    Repair(Option<crate::repair::NzbgetRsRepairController>),
    Unpack(NzbgetRsUnpackController),
    Cleanup,
    Move,
    PostRenamer(bool),
    /// Last stage: marks post-processing as finished.
    Finish,
}

#[cfg(unix)]
pub struct NzbgetRsPostJob {
    thread_ptr: usize,
    post_ptr: crate::queuesubsys_host::PostRef,
    nzb_id: i32,
    inner: PostJobInner,
}

#[cfg(unix)]
impl NzbgetRsPostJob {
    pub(crate) fn new(
        kind: c_int,
        thread_ptr: usize,
        post_ptr: crate::queuesubsys_host::PostRef,
        nzb_id: c_int,
    ) -> Self {
        let inner = match kind {
            0 | 1 => {
                crate::log::log_active_bytes(MK_DEBUG, b"Creating RenameController");
                PostJobInner::Rename(kind)
            }
            2 | 10 => PostJobInner::StreamRepair(
                crate::streamrepair_coord::NzbgetRsStreamRepairCtrl::new(
                    kind == 10,
                    nzb_id,
                    &crate::articlefetcher::NativeNntpHost,
                    !crate::ffi::build_tls_disabled(),
                    crate::ffi::build_debug_enabled(),
                ),
            ),
            3 => {
                crate::log::log_active_bytes(MK_DEBUG, b"Creating RepairController");
                let ctrl = if crate::ffi::build_parcheck_enabled() {
                    Some(crate::repair::NzbgetRsRepairController::new(
                        &crate::articlefetcher::NativeNntpHost,
                    ))
                } else {
                    None
                };
                PostJobInner::Repair(ctrl)
            }
            4 => PostJobInner::Unpack(NzbgetRsUnpackController::new()),
            5 => PostJobInner::Cleanup,
            6 => PostJobInner::Move,
            7 | 8 => PostJobInner::PostRenamer(kind == 8),
            9 => PostJobInner::Finish,
            _ => PostJobInner::Cleanup,
        };
        Self {
            thread_ptr,
            post_ptr,
            nzb_id,
            inner,
        }
    }

    /// Asks the job's running controller to stop.
    pub(crate) fn request_stop(&self) {
        match &self.inner {
            PostJobInner::Repair(ctrl_opt) => {
                crate::log::log_active_bytes(MK_DEBUG, b"Stopping RepairController");
                if let Some(ctrl) = ctrl_opt {
                    ctrl.stop();
                }
            }
            PostJobInner::StreamRepair(ctrl) => ctrl.stop(),
            PostJobInner::Unpack(ctrl) => {
                crate::log::log_active_bytes(MK_DEBUG, b"Stopping unpack");
                ctrl.stop();
            }
            _ => {}
        }
    }

    fn stop_from_rust(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        if matches!(self.inner, PostJobInner::StreamRepair(_)) {
            self.request_stop();
            host.thread_stop(self.thread_ptr);
        } else {
            match &self.inner {
                PostJobInner::Repair(_) => {
                    crate::log::log_active_bytes(MK_DEBUG, b"Stopping RepairController");
                }
                PostJobInner::Unpack(_) => {
                    crate::log::log_active_bytes(MK_DEBUG, b"Stopping unpack");
                }
                _ => {}
            }
            host.thread_stop(self.thread_ptr);
            match &self.inner {
                PostJobInner::Repair(Some(ctrl)) => ctrl.stop(),
                PostJobInner::Unpack(ctrl) => ctrl.stop(),
                _ => {}
            }
        }
    }

    /// Runs the job's work on its thread.
    pub(crate) fn run(&self) {
        match &self.inner {
            PostJobInner::Rename(rk) => {
                self.run_rename(*rk);
            }
            PostJobInner::StreamRepair(ctrl) => {
                if let Some(host) = crate::ffi::queue_host() {
                    ctrl.run(JobQueue { job: self, host: &host });
                }
            }
            PostJobInner::Repair(Some(ctrl)) => {
                if let Some(host) = crate::ffi::queue_host() {
                    ctrl.run(JobQueue { job: self, host: &host });
                }
            }
            PostJobInner::Repair(None) => {}
            PostJobInner::Unpack(ctrl) => {
                if let Some(host) = crate::ffi::queue_host() {
                    ctrl.run(JobQueue { job: self, host: &host });
                }
            }
            PostJobInner::Cleanup => {
                self.run_cleanup();
            }
            PostJobInner::Move => {
                self.run_move();
            }
            PostJobInner::PostRenamer(post_unpack) => {
                self.run_post_renamer(*post_unpack);
            }
            PostJobInner::Finish => {
                finish_post_job(self.post_ptr);
            }
        }
    }

    /// Adds the par file `file` (`deleted` when it was deleted) to a running
    /// repair. False for other jobs or when the repair did not take it.
    pub(crate) fn add_par(&self, file: FileRef, deleted: bool) -> bool {
        if let PostJobInner::Repair(Some(ctrl)) = &self.inner {
            crate::ffi::queue_host()
                .is_some_and(|host| ctrl.add_par(JobQueue { job: self, host: &host }, file, deleted))
        } else {
            false
        }
    }
}

#[cfg(all(test, unix))]
mod live_hold_tests {
    #[test]
    fn borrowed_volume_is_released_under_its_own_name_not_the_stand_in() {
        let dir = std::env::temp_dir().join(format!("rg-standin-rel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = |n: &str| dir.join(n).to_string_lossy().as_bytes().to_vec();
        let id = 917_302;
        super::direct_unpack_hold(id, &p("x3obf.rar"));
        let v = crate::deadvolume::VirtualVolume {
            head: Vec::new(),
            data: dir.join("none"),
            data_len: 0,
            tail: Vec::new(),
            holes: Vec::new(),
        };
        super::direct_unpack_set_virtual(id, &p("movie.part04.rar"), v.clone());
        let (released, _) = super::direct_unpack_take_released(id);
        assert_eq!(released, vec![b"movie.part04.rar".to_vec()]);
        assert!(!super::direct_unpack_has_holds(id));
        // An intact volume not renamed yet: nothing held for it, nothing counted.
        std::fs::write(dir.join("y1obf.rar"), b"intact").unwrap();
        super::direct_unpack_hold(id, &p("y1obf.rar"));
        super::direct_unpack_set_virtual(id, &p("movie.part05.rar"), v);
        assert!(super::direct_unpack_has_holds(id));
        // Another dead volume held under its own name is no stand-in for this one.
        super::direct_unpack_hold(id, &p("movie.part07.rar"));
        let v2 = crate::deadvolume::VirtualVolume {
            head: Vec::new(),
            data: dir.join("none"),
            data_len: 0,
            tail: Vec::new(),
            holes: Vec::new(),
        };
        super::direct_unpack_set_virtual(id, &p("movie.part06.rar"), v2);
        assert!(super::direct_unpack_is_held_path(&p("movie.part07.rar")));
        assert_eq!(super::take_dead_volumes_borrowed(id), 1);
        super::direct_unpack_forget_holds(id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_volume_recreated_whole_drops_its_stand_in_handed_over_with_holes() {
        use crate::progressive_unpack::{VolumeFeed, VolumeView};
        let dir = std::env::temp_dir().join(format!("rg-offer-rel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let vol = dir.join("movie.part04.rar");
        let full = vol.to_string_lossy().as_bytes().to_vec();
        let scratch = dir.join(String::from_utf8(crate::deadvolume::scratch_name(b"movie.part04.rar")).unwrap());
        std::fs::write(&scratch, vec![0u8; 8]).unwrap();
        let id = 917_303;
        super::direct_unpack_hold(id, &full);
        super::direct_unpack_offer_virtual(
            id,
            &full,
            crate::deadvolume::VirtualVolume {
                head: vec![1, 2],
                data: scratch.clone(),
                data_len: 8,
                tail: Vec::new(),
                holes: vec![(2, 8)],
            },
        );
        let mut feed = super::DirectUnpackFeed { nzb_id: id, expected_volumes: 6 };
        assert!(matches!(feed.volume(&vol), VolumeView::Virtual(_)));
        // Recreated whole from a twin: its data completes the stand-in, whose layout the
        // extraction may have recorded holes against.
        std::fs::write(&vol, b"HEADwhole-v!TL").unwrap();
        super::direct_unpack_offer_virtual(
            id,
            &full,
            crate::deadvolume::VirtualVolume {
                head: vec![1, 2],
                data: scratch.clone(),
                data_len: 8,
                tail: b"TL".to_vec(),
                holes: vec![(2, 8)],
            },
        );
        assert!(super::direct_unpack_release_path(&full));
        assert!(super::direct_unpack_offered(id, &full).is_none());
        assert_eq!(std::fs::read(&scratch).unwrap(), b"whole-v!");
        assert!(matches!(feed.volume(&vol), VolumeView::Virtual(ref v) if v.holes.is_empty()));
        super::direct_unpack_forget_holds(id);
        assert!(!scratch.exists());
        // A real volume that does not end like the stand-in: the stand-in goes, the file answers.
        std::fs::write(&scratch, vec![0u8; 8]).unwrap();
        super::direct_unpack_hold(id, &full);
        super::direct_unpack_offer_virtual(
            id,
            &full,
            crate::deadvolume::VirtualVolume {
                head: vec![1, 2],
                data: scratch.clone(),
                data_len: 8,
                tail: b"XX".to_vec(),
                holes: vec![(2, 8)],
            },
        );
        assert!(super::direct_unpack_release_path(&full));
        assert!(super::direct_unpack_offered(id, &full).is_none());
        assert!(!scratch.exists());
        assert!(!super::dead_volume_borrow_underway(id));
        assert!(matches!(feed.volume(&vol), VolumeView::Ready { ref holes } if holes.is_empty()));
        super::direct_unpack_forget_holds(id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_volume_remembered_after_a_restart_is_held_until_repaired() {
        use crate::progressive_unpack::{VolumeFeed, VolumeView};
        let dir = std::env::temp_dir().join(format!("rg-remember-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let vol = dir.join("movie.part02.rar");
        std::fs::write(&vol, b"damaged").unwrap();
        let id = 917_311;
        let mut feed = super::DirectUnpackFeed { nzb_id: id, expected_volumes: 4 };
        assert!(matches!(feed.volume(&vol), VolumeView::Ready { ref holes } if holes.is_empty()));
        feed.remember_damaged(&vol, vec![(2, 3)]);
        assert!(super::direct_unpack_has_holds(id));
        assert!(matches!(feed.volume(&vol), VolumeView::Ready { ref holes } if holes == &[(2, 3)]));
        assert!(!feed.wait_repaired(&mut || true), "still held: waits (here cancelled)");
        super::direct_unpack_forget_holds(id);
        // Once the download is over nothing would release a new hold.
        super::direct_unpack_mark_downloaded(id);
        feed.remember_damaged(&vol, vec![(2, 3)]);
        assert!(!super::direct_unpack_has_holds(id));
        super::direct_unpack_forget_holds(id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_absent_obfuscated_stand_ins_can_be_released_without_repair() {
        let dir = std::env::temp_dir().join(format!("rg-standins-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = |n: &str| dir.join(n).to_string_lossy().as_bytes().to_vec();
        let id = 917_301;
        super::direct_unpack_hold(id, &p("a8f3k2b9"));
        assert!(super::direct_unpack_holds_only_absent_stand_ins(id));
        std::fs::write(dir.join("x1y2z3"), b"damaged").unwrap();
        super::direct_unpack_hold(id, &p("x1y2z3"));
        assert!(!super::direct_unpack_holds_only_absent_stand_ins(id), "a damaged file on disk");
        super::direct_unpack_forget_holds(id);
        super::direct_unpack_hold(id, &p("movie.part04.rar"));
        assert!(!super::direct_unpack_holds_only_absent_stand_ins(id), "a missing RAR volume");
        super::direct_unpack_forget_holds(id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;
    #[test]
    fn a_widened_hold_reaches_the_unpack_thread_and_a_released_one_stays_released() {
        let id = -1900299;
        let path = b"/fixture/widen.part02.rar".as_slice();
        assert!(!direct_unpack_widen_hold(id, path, vec![(70_000, 10)]));
        direct_unpack_hold_with_holes(id, path, Vec::new());
        assert_eq!(direct_unpack_hole_view(path), Some(Vec::new()));
        assert!(direct_unpack_widen_hold(id, path, vec![(70_000, 10)]));
        assert_eq!(direct_unpack_hole_view(path), Some(vec![(70_000, 10)]));
        let (released, _) = direct_unpack_take_released(id);
        assert_eq!(released, vec![b"widen.part02.rar".to_vec()]);
        direct_unpack_holds().remove(&id);
        assert!(!direct_unpack_widen_hold(id, path, vec![(70_000, 10)]));
        assert_eq!(direct_unpack_hole_view(path), None);
    }

    #[test]
    fn live_hold_publication_is_scoped_to_its_generation_and_download() {
        let id = -1900240;
        let other = -1900241;
        direct_unpack_hold(id, b"/fixture/part40.rar");
        direct_unpack_hold(other, b"/fixture/part40.rar");
        direct_unpack_set_awaiting_repair(id);
        let old = begin_live_hold_run(id, crate::parlive::SessionGeneration(1)).unwrap();
        assert_eq!(
            publish_live_holds(
                id,
                crate::parlive::SessionGeneration(2),
                &[(40, b"/fixture/part40.rar".to_vec())],
                &[40]
            ),
            0
        );
        assert!(direct_unpack_has_holds(id));
        assert_eq!(
            publish_live_holds(
                id,
                crate::parlive::SessionGeneration(1),
                &[(40, b"/fixture/part40.rar".to_vec())],
                &[40]
            ),
            1
        );
        assert!(!direct_unpack_has_holds(id));
        assert!(direct_unpack_has_holds(other));
        drop(old);
        direct_unpack_forget_holds(id);
        direct_unpack_forget_holds(other);
    }
    #[test]
    fn finished_run_cannot_clear_a_new_run_in_the_same_generation() {
        let id = -1900243;
        direct_unpack_hold(id, b"/fixture/part40.rar");
        direct_unpack_set_awaiting_repair(id);
        let generation = crate::parlive::SessionGeneration(4);
        let mut first = begin_live_hold_run(id, generation).unwrap();
        first.finish();
        let second = begin_live_hold_run(id, generation).unwrap();
        drop(first);
        assert!(
            !direct_unpack_awaiting_repair(id),
            "old finished guard reset the next helper"
        );
        drop(second);
        assert!(direct_unpack_awaiting_repair(id));
        direct_unpack_forget_holds(id);
    }

    #[test]
    fn normal_postprocessing_takeover_prevents_a_late_live_retry() {
        use crate::queuesubsys_host::NzbSnap;
        let mut nzb = NzbSnap::default();
        assert!(live_repair_may_start(&nzb));
        nzb.has_post_info = true;
        nzb.post_stage = PostStage::Queued;
        assert!(live_repair_may_start(&nzb));
        nzb.post_working = true;
        assert!(!live_repair_may_start(&nzb));
        nzb.post_working = false;
        nzb.post_stage = PostStage::LoadingPars;
        assert!(!live_repair_may_start(&nzb));
        nzb.post_stage = PostStage::Unpacking;
        assert!(!live_repair_may_start(&nzb));
    }

    #[test]
    fn failed_live_helper_keeps_normal_repair_available() {
        let id = -1900242;
        direct_unpack_hold(id, b"/fixture/part40.rar");
        direct_unpack_set_awaiting_repair(id);
        let run = begin_live_hold_run(id, crate::parlive::SessionGeneration(3)).unwrap();
        assert!(!direct_unpack_awaiting_repair(id));
        drop(run); // failure, spawn refusal or unwind
        assert!(direct_unpack_has_holds(id));
        assert!(direct_unpack_awaiting_repair(id));
        direct_unpack_forget_holds(id);
    }
}

#[cfg(test)]
mod live_par_time_tests {
    use super::live_par_time_charge;

    #[test]
    fn live_par_time_is_charged_at_history_unless_deleted() {
        // A job whose par-check was skipped (ParSec 0) still gets its live time.
        assert_eq!(live_par_time_charge(false, (0, 0, 0), (7, 3)), Some((7, 7, 3)));
        // Added to an ordinary par-check's times.
        assert_eq!(live_par_time_charge(false, (4, 0, 2), (7, 3)), Some((11, 7, 5)));
        // Deleted, or leaving the queue without history: dropped.
        assert_eq!(live_par_time_charge(true, (4, 0, 2), (7, 3)), None);
        // Nothing to charge.
        assert_eq!(live_par_time_charge(false, (4, 1, 2), (0, 0)), None);
    }

    /// Time from live work that ended after the job reached history (or of a job
    /// merge/split removed) is dropped, so a return from history never adds it.
    #[test]
    fn orphaned_live_par_time_is_discarded() {
        let nzb_id = 0x6a0f_0022;
        let _ = crate::parchecker::streaming_session_take_par_secs(nzb_id);
        crate::parchecker::live_par_time_add_for_test(nzb_id, 5_000, 2_000);
        super::live_par_time_discard(nzb_id);
        let after_return = crate::parchecker::streaming_session_take_par_secs(nzb_id);
        assert_eq!(live_par_time_charge(false, (4, 0, 1), after_return), None);
    }

    /// The job's live time is taken once: a second settle (a job returned from
    /// history and completed again) never adds the first download's time again.
    #[test]
    fn live_par_time_is_taken_once() {
        let nzb_id = 0x6a0f_0020;
        assert_eq!(crate::parchecker::streaming_session_take_par_secs(nzb_id), (0, 0));
        crate::parchecker::live_par_time_add_for_test(nzb_id, 2_400, 900);
        let first = crate::parchecker::streaming_session_take_par_secs(nzb_id);
        assert_eq!(first, (3, 1));
        assert_eq!(live_par_time_charge(false, (0, 0, 0), first), Some((3, 3, 1)));
        let again = crate::parchecker::streaming_session_take_par_secs(nzb_id);
        assert_eq!(live_par_time_charge(false, (0, 0, 0), again), None);
    }
}

#[cfg(test)]
mod busy_time_tests {
    use super::BusyTime;
    use std::time::{Duration, Instant};

    #[test]
    fn counts_only_active_periods_and_reports_each_second_once() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut busy = BusyTime::default();
        busy.set_active(true, at(0));
        busy.set_active(true, at(1)); // already active: no restart
        busy.set_active(false, at(3));
        busy.set_active(false, at(10)); // idle time is not counted
        busy.set_active(true, at(20));
        // Still downloading: all of it is live.
        assert_eq!(busy.take_secs(at(22)), (5, 5)); // 3 closed + 2 open
        assert_eq!(busy.take_secs(at(22)), (0, 0));
        busy.set_active(false, at(25));
        assert_eq!(busy.take_secs(at(30)), (3, 3));
        assert_eq!(BusyTime::default().take_secs(at(5)), (0, 0));
    }

    /// 60 s unpacking while downloading, 90 s waiting after the download (for the
    /// unpack lock), then 10 s unpacking: 70 s in all, 60 s of it live.
    #[test]
    fn splits_at_download_completion_not_by_the_wait_after_it() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut busy = BusyTime::default();
        busy.set_active(true, at(0));
        busy.set_active(false, at(60));
        busy.mark_download_done(at(60));
        busy.set_active(true, at(150));
        busy.set_active(false, at(160));
        assert_eq!(busy.take_secs(at(160)), (70, 60));
        // Download completing mid-extraction splits the open period there.
        let mut busy = BusyTime::default();
        busy.set_active(true, at(0));
        busy.mark_download_done(at(40));
        busy.mark_download_done(at(45)); // once
        assert_eq!(busy.take_secs(at(50)), (50, 40));
        assert_eq!(busy.take_secs(at(55)), (5, 0));
    }
}

#[cfg(test)]
mod dead_volume_case_tests {
    use super::*;

    #[test]
    fn a_donor_counts_as_another_video_only_after_repeated_mismatches() {
        let (id, donor) = (-4_242_522, b"/q/backup.nzb".as_slice());
        assert!(!dead_volume_donor_mismatched(id, donor));
        note_dead_volume_donor_identity(id, donor, false);
        // one failed check may be an article that could not be read just then
        assert!(!dead_volume_donor_mismatched(id, donor));
        note_dead_volume_donor_identity(id, donor, false);
        assert!(dead_volume_donor_mismatched(id, donor));
        assert!(!dead_volume_donor_mismatched(id, b"/q/other.nzb"));
        // a confirmed check clears it, for good: later failed checks are probes landing on
        // volumes still downloading
        note_dead_volume_donor_identity(id, donor, true);
        assert!(!dead_volume_donor_mismatched(id, donor));
        for _ in 0..5 {
            note_dead_volume_donor_identity(id, donor, false);
        }
        assert!(!dead_volume_donor_mismatched(id, donor));
    }

    #[test]
    fn a_donor_confirmed_for_a_download_stays_confirmed_until_it_leaves() {
        let (id, donor) = (-4_242_529, 7_711);
        assert_eq!(dead_volume_donor_confirmed(id, donor), None);
        note_dead_volume_donor_confirmed(id, donor, 15_000_000);
        assert_eq!(dead_volume_donor_confirmed(id, donor), Some(15_000_000));
        assert_eq!(dead_volume_donor_confirmed(id, donor + 1), None);
        assert_eq!(dead_volume_donor_confirmed(id + 1, donor), None);
        note_dead_volume_donor_confirmed(id, 0, 1);
        assert_eq!(dead_volume_donor_confirmed(id, 0), None);
        // A later check that differs takes it back.
        forget_dead_volume_donor_confirmed(id, donor);
        assert_eq!(dead_volume_donor_confirmed(id, donor), None);
        note_dead_volume_donor_confirmed(id, donor, 15_000_000);
        forget_dead_volume_verdicts(id);
        assert_eq!(dead_volume_donor_confirmed(id, donor), None);
    }

    #[test]
    fn a_mismatched_donor_holds_nothing_back_only_while_no_other_lends() {
        let (id, b1, b2) = (-4_242_523, b"/q/b1.nzb".as_slice(), b"/q/b2.nzb".as_slice());
        for _ in 0..DEAD_VOLUME_DONOR_MISMATCH_RUNS {
            note_dead_volume_donor_identity(id, b1, false);
        }
        assert!(dead_volume_donor_cannot_lend(id, b1));
        // a repair run under way is no lender: every run marks one, also one checking b1 again
        note_dead_volume_borrow(id, true);
        assert!(dead_volume_donor_cannot_lend(id, b1));
        note_dead_volume_borrow(id, false);
        // another donor's volume was rebuilt and handed to direct unpack ...
        direct_unpack_hold(id, b"/dl/Set.part07.rar");
        direct_unpack_set_virtual(
            id,
            b"/dl/Set.part07.rar",
            crate::deadvolume::VirtualVolume {
                head: vec![1],
                data: std::path::PathBuf::from("/dl/x"),
                data_len: 0,
                tail: Vec::new(),
                holes: Vec::new(),
            },
        );
        assert!(!dead_volume_donor_cannot_lend(id, b1));
        direct_unpack_forget_holds(id);
        for _ in 0..DEAD_VOLUME_DONOR_MISMATCH_RUNS {
            note_dead_volume_donor_identity(id, b1, false);
        }
        assert!(dead_volume_donor_cannot_lend(id, b1));
        // ... or another donor's video matched
        note_dead_volume_donor_identity(id, b2, true);
        assert!(dead_volume_any_donor_confirmed(id));
        assert!(!dead_volume_donor_cannot_lend(id, b1));
        assert!(dead_volume_donor_mismatched(id, b1));
        forget_dead_volume_verdicts(id);
    }

    #[test]
    fn verdicts_on_donors_are_forgotten_when_the_download_leaves_the_queue() {
        let (id, donor) = (-4_242_534, b"/q/b1.nzb".as_slice());
        for _ in 0..DEAD_VOLUME_DONOR_MISMATCH_RUNS {
            note_dead_volume_donor_identity(id, donor, false);
        }
        note_dead_volume_donor_identity(id + 1, donor, true);
        assert!(dead_volume_donor_mismatched(id, donor));
        // the post job ends (or the download is deleted, or returned from history)
        direct_unpack_forget_holds(id);
        assert!(!dead_volume_donor_mismatched(id, donor));
        assert!(dead_volume_any_donor_confirmed(id + 1));
        forget_dead_volume_verdicts(id + 1);
        assert!(!dead_volume_any_donor_confirmed(id + 1));
    }

    #[test]
    fn a_borrowed_volume_is_found_whatever_case_the_dead_job_used() {
        let id = -4_242_471;
        let held = b"/dl/SET.part03.rar";
        let extractor = b"/dl/set.part03.rar";
        direct_unpack_hold(id, held);
        let v = crate::deadvolume::VirtualVolume {
            head: vec![1],
            data: std::path::PathBuf::from("/dl/x"),
            data_len: 0,
            tail: Vec::new(),
            holes: Vec::new(),
        };
        direct_unpack_set_virtual(id, held, v.clone());
        assert!(!direct_unpack_has_holds(id), "the hold is released");
        assert_eq!(direct_unpack_virtual(id, extractor), Some(v));
        assert!(direct_unpack_has_virtual(extractor));
        direct_unpack_holds().remove(&id);
    }
}

#[cfg(test)]
mod locked_archive_tests {
    use super::*;
    use crate::progressive_unpack::Failure;

    #[test]
    fn a_password_the_nzb_lacks_locks_the_archive() {
        let locks =
            |f: Failure, password: &[u8], pass_file| failure_locks_archive(&f, password, pass_file);
        assert!(locks(Failure::PasswordRequired, b"", false));
        // a password from the nzb or the pass file may still open it
        assert!(!locks(Failure::PasswordRequired, b"pw", false));
        assert!(!locks(Failure::PasswordRequired, b"", true));
        // damage is the dupe fallback's business, not this
        assert!(!locks(Failure::WrongPassword, b"pw", false));
        assert!(!locks(Failure::Damaged(String::new()), b"", false));
    }

    #[test]
    fn the_locked_mark_is_answered_once() {
        let id = -4_242_472;
        locked_archives().insert(id);
        assert!(take_direct_unpack_locked(id));
        assert!(!take_direct_unpack_locked(id));
    }
}

#[cfg(all(test, unix))]
mod live_end_repair_tests {
    use super::*;

    impl EndRepairClock {
        /// `update` with an unchanged admission counter.
        fn update_test(
            &mut self,
            progress: Option<crate::parchecker::LiveRepairProgress>,
            now: std::time::Instant,
        ) -> bool {
            let admission = self.admission;
            self.update(progress, Some(admission), now)
        }
    }

    #[test]
    fn an_idle_helper_that_admits_targets_or_folds_articles_is_not_stuck() {
        use crate::parchecker::{LiveRepairProgress, LiveRepairStep};
        let mut idle = LiveRepairProgress::default();
        idle.step = LiveRepairStep::Idle;
        let start = std::time::Instant::now();
        let later = |secs| start + Duration::from_secs(secs);
        let mut c = EndRepairClock::start(0, start);
        // Idle for 10 minutes, but the drain folds an article every minute: not stuck.
        for minute in 1..=10u64 {
            assert!(!c.update(Some(idle), Some(minute), later(minute * 60)));
        }
        // Then nothing advances: stuck 120 s after the last step.
        assert!(!c.update(Some(idle), Some(10), later(600 + 119)));
        assert!(c.update(Some(idle), Some(10), later(600 + 120)));
    }

    #[test]
    fn an_end_repair_stalls_only_without_work_in_a_working_step() {
        use crate::parchecker::{LiveRepairProgress, LiveRepairStep};
        let progress = |step, work| {
            let mut p = LiveRepairProgress::default();
            p.step = step;
            p.work = work;
            Some(p)
        };
        let start = std::time::Instant::now();
        let later = |secs| start + Duration::from_secs(secs);
        let clock = || EndRepairClock::start(10, start);
        // The work counter rising is progress, in any working step, however long it runs.
        let mut c = clock();
        assert!(!c.update_test(progress(LiveRepairStep::Matrix, 11), later(400)));
        assert_eq!((c.work, c.progressed_at), (11, later(400)));
        // Flat in the matrix step: stalled after 300 s, not before.
        assert!(!clock().update_test(progress(LiveRepairStep::Matrix, 10), later(299)));
        assert!(clock().update_test(progress(LiveRepairStep::Matrix, 10), later(300)));
        // Waiting for another repair never stalls.
        assert!(!clock().update_test(progress(LiveRepairStep::PayloadWait, 10), later(5000)));
        // Idle (outside a repair call, still admitting targets): given up after 120 s.
        let mut c = clock();
        assert!(!c.update_test(progress(LiveRepairStep::Idle, 10), later(10)));
        assert!(!c.update_test(progress(LiveRepairStep::Idle, 10), later(129)));
        assert!(c.update_test(progress(LiveRepairStep::Idle, 10), later(130)));
        // Leaving idle resets that clock.
        let mut c = clock();
        assert!(!c.update_test(progress(LiveRepairStep::Idle, 10), later(10)));
        assert!(!c.update_test(progress(LiveRepairStep::Decode, 11), later(100)));
        assert!(!c.update_test(progress(LiveRepairStep::Idle, 11), later(200)));
        assert!(!c.update_test(progress(LiveRepairStep::Idle, 11), later(319)));
        // Syncing one large file may stay flat much longer.
        assert!(!clock().update_test(progress(LiveRepairStep::Sync, 10), later(1799)));
        assert!(clock().update_test(progress(LiveRepairStep::Sync, 10), later(1800)));
        // No session to ask: no verdict.
        assert!(!clock().update_test(None, later(100_000)));
    }

    #[test]
    fn stopping_a_download_never_waits_for_its_repair_helpers() {
        // A helper stuck (as one waiting for the cache fence would be) must not hold up
        // the caller, who may hold the queue lock; the reaper joins it once it ends.
        let id = -1_975_002;
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let stuck = std::thread::spawn(move || {
            let _ = wait.recv();
        });
        keep_live_helper(&LIVE_END_HELPERS, id, stuck);
        let (release2, wait2) = std::sync::mpsc::channel::<()>();
        keep_live_helper(
            &LIVE_PAR_HELPERS,
            id,
            std::thread::spawn(move || {
                let _ = wait2.recv();
            }),
        );
        let started = std::time::Instant::now();
        direct_unpack_join_live_par_repair(id);
        assert!(started.elapsed() < Duration::from_secs(1), "stopping must not join");
        assert!(LIVE_END_HELPERS.lock().unwrap().get(&id).is_none());
        assert!(LIVE_PAR_HELPERS.lock().unwrap().get(&id).is_none());
        release.send(()).unwrap();
        release2.send(()).unwrap();
    }

    #[test]
    fn a_cancelled_helper_stops_waiting_for_the_cache_fence() {
        // The cache flusher holds the fence (and would wait for the queue lock); the job is
        // deleted meanwhile. The helper's fence wait must end on the cancel, not hang.
        let held = crate::articledl::hold_cache_flush_lock_for_test();
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&cancelled);
        let waiter = std::thread::spawn(move || {
            crate::articledl::with_live_cache_fence_unless(
                || flag.load(std::sync::atomic::Ordering::Relaxed),
                || (),
            )
        });
        std::thread::sleep(Duration::from_millis(120));
        cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
        let started = std::time::Instant::now();
        while !waiter.is_finished() {
            assert!(started.elapsed() < Duration::from_secs(2), "fence wait hung");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(waiter.join().unwrap().is_none());
        drop(held);
        // Not cancelled: it takes the fence once free.
        assert_eq!(crate::articledl::with_live_cache_fence_unless(|| false, || 7), Some(7));
    }

    #[test]
    fn a_cancelled_fence_waiter_never_swallows_a_plain_waiters_wakeup() {
        // The fence is held; a live repair helper and the cache flusher both wait for it.
        // The helper is cancelled and leaves without the lock: the flusher must still get it.
        for _ in 0..20 {
            let held = crate::articledl::hold_cache_flush_lock_for_test();
            let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let flag = std::sync::Arc::clone(&cancelled);
            let helper = std::thread::spawn(move || {
                crate::articledl::with_live_cache_fence_unless(
                    || flag.load(std::sync::atomic::Ordering::Relaxed),
                    || (),
                )
            });
            let flusher = std::thread::spawn(crate::articledl::lock_and_unlock_cache_flush_for_test);
            std::thread::sleep(Duration::from_millis(5));
            cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
            drop(held);
            let started = std::time::Instant::now();
            while !flusher.is_finished() {
                assert!(started.elapsed() < Duration::from_secs(2), "lost wakeup: flusher hangs");
                std::thread::sleep(Duration::from_millis(2));
            }
            flusher.join().unwrap();
            let _ = helper.join().unwrap();
        }
    }

    #[test]
    fn a_genuinely_failed_end_repair_stays_failed_after_a_stop() {
        let id = -1_975_004;
        LIVE_END_REPAIRS.lock().unwrap().insert(id, EndRepair::Failed);
        keep_live_helper(&LIVE_END_HELPERS, id, std::thread::spawn(|| {}));
        direct_unpack_join_live_par_repair(id);
        std::thread::sleep(Duration::from_millis(50));
        assert!(matches!(live_end_repair_state(id), Some(EndRepair::Failed)));
        LIVE_END_REPAIRS.lock().unwrap().remove(&id);
    }

    #[test]
    fn a_cancelled_end_repair_can_run_again_once_its_helper_ended() {
        let id = -1_975_003;
        // Running when the stop comes; the helper's exit marks it Failed.
        LIVE_END_REPAIRS
            .lock()
            .unwrap()
            .insert(id, EndRepair::Running(EndRepairClock::start(0, std::time::Instant::now())));
        let (release, wait) = std::sync::mpsc::channel::<()>();
        keep_live_helper(
            &LIVE_END_HELPERS,
            id,
            std::thread::spawn(move || {
                let _ = wait.recv();
            }),
        );
        direct_unpack_join_live_par_repair(id);
        // The helper's exit (cancelled) marks it Failed, as the end repair's guard does.
        LIVE_END_REPAIRS.lock().unwrap().insert(id, EndRepair::Failed);
        release.send(()).unwrap();
        let started = std::time::Instant::now();
        while live_end_repair_state(id).is_some() {
            assert!(started.elapsed() < Duration::from_secs(2), "Failed never cleared");
            std::thread::sleep(Duration::from_millis(5));
        }
        // Shutdown's bounded wait returns at once with nothing left running.
        let started = std::time::Instant::now();
        live_par_helpers_wait(Duration::from_secs(5));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn keeping_a_helper_never_waits_for_a_running_one() {
        static HELPERS: LiveHelpers =
            std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let running = std::thread::spawn(move || {
            let _ = wait.recv();
        });
        keep_live_helper(&HELPERS, 7, running);
        // A second helper while the first still runs: returns at once, keeps both.
        let started = std::time::Instant::now();
        keep_live_helper(&HELPERS, 7, std::thread::spawn(|| {}));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(HELPERS.lock().unwrap()[&7].len(), 2);
        release.send(()).unwrap();
        while !HELPERS.lock().unwrap()[&7].iter().all(|h| h.is_finished()) {
            std::thread::sleep(Duration::from_millis(5));
        }
        // Ended helpers are joined when the next one is kept.
        keep_live_helper(&HELPERS, 7, std::thread::spawn(|| {}));
        assert_eq!(HELPERS.lock().unwrap()[&7].len(), 1);
    }

    #[test]
    fn post_processing_waits_only_for_a_progressing_or_startable_end_repair() {
        let id = -1_975_001;
        let set = |state: Option<EndRepair>| {
            let mut map = LIVE_END_REPAIRS.lock().unwrap();
            match state {
                Some(state) => map.insert(id, state),
                None => map.remove(&id),
            };
        };
        set(None);
        assert_eq!(live_end_repair_defers_post(id), None);
        // A failed end repair or unresolvable targets: par-check goes ahead at once.
        set(Some(EndRepair::Failed));
        assert_eq!(live_end_repair_defers_post(id), Some(false));
        let later = std::time::Instant::now() + Duration::from_secs(60);
        set(Some(EndRepair::Unresolvable { retry_at: later }));
        assert_eq!(live_end_repair_defers_post(id), Some(false));
        let past = std::time::Instant::now() - Duration::from_millis(1);
        set(Some(EndRepair::Unresolvable { retry_at: past }));
        assert_eq!(live_end_repair_defers_post(id), None);
        // A running one is waited for, however long it has run, while it progresses.
        set(Some(EndRepair::Running(EndRepairClock::start(0, std::time::Instant::now()))));
        assert_eq!(live_end_repair_defers_post(id), Some(true));
        assert!(live_end_repair_running(id));
        // The held-volume repair does not start beside it.
        direct_unpack_holds().entry(id).or_default().held.push(b"/x/a.rar".to_vec());
        assert!(begin_live_hold_run(id, crate::parlive::SessionGeneration(1)).is_none());
        direct_unpack_holds().remove(&id);
        set(None);
    }
}
