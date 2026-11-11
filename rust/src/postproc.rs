//! End-to-end Rust orchestration for post-processing controllers:
//! `ParChecker`, `UnpackController`, `DirectUnpack`, `MoveController`,
//! `CleanupController`, `RenameController`, `PostRenamerController`, and `PostScriptController`.

use crate::parchecker::{
    is_processed_file, ParRepairEvents, ParSegment, QuickInfo, RustParRepairer,
    PAR_RESULT_INVALID_ARGS, PAR_RESULT_REPAIR_FAILED, PAR_RESULT_REPAIR_NOT_POSSIBLE,
    PAR_RESULT_REPAIR_POSSIBLE, PAR_RESULT_SUCCESS,
};
use crate::postrepair::NzbgetRsParSegment;
use crate::repair::NzbgetRsRepairCtrlHostFn;
use std::collections::VecDeque;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const MK_INFO: i32 = 0;
pub const MK_WARNING: i32 = 1;
pub const MK_ERROR: i32 = 2;
pub const MK_DEBUG: i32 = 3;
pub const MK_DETAIL: i32 = 4;

pub const PAR_STATUS_FAILED: i32 = 0;
pub const PAR_STATUS_REPAIR_POSSIBLE: i32 = 1;
pub const PAR_STATUS_REPAIRED: i32 = 2;
pub const PAR_STATUS_REPAIR_NOT_NEEDED: i32 = 3;

pub const PAR_STAGE_LOADING_PARS: i32 = 0;
pub const PAR_STAGE_VERIFYING_SOURCES: i32 = 1;
pub const PAR_STAGE_REPAIRING: i32 = 2;
pub const PAR_STAGE_VERIFYING_REPAIRED: i32 = 3;

pub const PAR_SCAN_EXTENDED: i32 = 1;
pub const PAR_SCAN_FULL: i32 = 2;
pub const PAR_SCAN_DUPE: i32 = 3;

static UNPACK_MUTEX: Mutex<()> = Mutex::new(());

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
pub(crate) fn to_c_buf(bytes: &[u8]) -> Vec<u8> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let mut out = Vec::with_capacity(end + 1);
    out.extend_from_slice(&bytes[..end]);
    out.push(0);
    out
}

#[inline]
pub(crate) unsafe fn ptr_bytes(p: *const c_char) -> Vec<u8> {
    if p.is_null() {
        Vec::new()
    } else {
        unsafe { CStr::from_ptr(p).to_bytes().to_vec() }
    }
}

#[inline]
fn trunc_1023(mut v: Vec<u8>) -> Vec<u8> {
    if v.len() > 1023 {
        v.truncate(1023);
    }
    v
}

#[inline]
fn with_dq_guard<R, F: FnOnce(usize) -> R>(
    ctx: *mut c_void,
    host_cb: NzbgetRsRepairCtrlHostFn,
    f: F,
) -> R {
    struct Call<F, R> {
        f: Option<F>,
        out: Option<R>,
    }
    extern "C" fn tramp<F: FnOnce(usize) -> R, R>(state_ptr: usize, dq_ptr: usize) {
        let st = unsafe { &mut *(state_ptr as *mut Call<F, R>) };
        if let Some(func) = st.f.take() {
            st.out = Some(func(dq_ptr));
        }
    }
    let mut call = Call {
        f: Some(f),
        out: None,
    };
    let fn_ptr = tramp::<F, R> as extern "C" fn(usize, usize) as usize;
    let state_ptr = (&mut call as *mut Call<F, R>) as usize;
    unsafe {
        host_cb(
            ctx,
            0,
            0,
            0,
            fn_ptr,
            state_ptr,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
    }
    call.out
        .expect("DownloadQueue::Guard callback must execute")
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
            status: AtomicI32::new(PAR_STATUS_FAILED),
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
    pub fn status(&self) -> i32 {
        self.status.load(Ordering::Acquire)
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
        self.status.store(st, Ordering::Release);

        if st == PAR_STATUS_REPAIR_NOT_NEEDED
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
            self.status.store(st, Ordering::Release);
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

    fn run_par_check_all(&self, hooks: &dyn ParCheckerHooks) -> i32 {
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
            return PAR_STATUS_FAILED;
        }

        let mut all_status = PAR_STATUS_REPAIR_NOT_NEEDED;
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

    fn run_par_check(&self, par_filename: Vec<u8>, hooks: &dyn ParCheckerHooks) -> i32 {
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
        if hooks.is_stopped() || res != PAR_RESULT_SUCCESS {
            self.cleanup(&mut repairer, &mut dupe_sources);
            return PAR_STATUS_FAILED;
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
            && res == PAR_RESULT_REPAIR_NOT_POSSIBLE
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
            && !(added_splitted_fragments && res == PAR_RESULT_REPAIR_POSSIBLE)
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
            && res == PAR_RESULT_REPAIR_NOT_POSSIBLE
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
            && res == PAR_RESULT_REPAIR_NOT_POSSIBLE
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
                if res == PAR_RESULT_REPAIR_NOT_POSSIBLE && !hooks.is_stopped() {
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
            && res == PAR_RESULT_REPAIR_NOT_POSSIBLE
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
            return PAR_STATUS_FAILED;
        }

        let mut status = PAR_STATUS_FAILED;
        let has_damaged = self.lock_signal().has_damaged_files;

        if res == PAR_RESULT_SUCCESS || !has_damaged {
            hooks.print_message(
                MK_INFO,
                &[b"Repair not needed for ", info_name.as_slice()].concat(),
            );
            status = PAR_STATUS_REPAIR_NOT_NEEDED;
        } else if res == PAR_RESULT_REPAIR_POSSIBLE {
            status = PAR_STATUS_REPAIR_POSSIBLE;
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
                if res == PAR_RESULT_SUCCESS {
                    hooks.print_message(
                        MK_INFO,
                        &[b"Successfully repaired ", info_name.as_slice()].concat(),
                    );
                    status = PAR_STATUS_REPAIRED;
                    hooks.stat_dupe_sources(&dupe_sources);
                    repairer
                        .as_ref()
                        .expect("repairer initialized")
                        .delete_leftovers(&mut |msg| hooks.print_message(MK_INFO, msg));
                } else {
                    status = PAR_STATUS_FAILED;
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
                status = PAR_STATUS_REPAIR_POSSIBLE;
            } else {
                hooks.print_message(
                    MK_WARNING,
                    &[b"Par-check cancelled for ", info_name.as_slice()].concat(),
                );
                self.set_err_msg(b"par-check cancelled".to_vec());
                status = PAR_STATUS_FAILED;
            }
        } else if status == PAR_STATUS_FAILED {
            let mut err = self.get_err_msg();
            if err.is_empty() && (0..=8).contains(&res) {
                let raw = crate::parchecker::cmdline_err_str(res);
                let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                err = raw[..end].to_vec();
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
            let set_ok = status == PAR_STATUS_REPAIRED || status == PAR_STATUS_REPAIR_NOT_NEEDED;
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
    ) -> i32 {
        let mut res = PAR_RESULT_REPAIR_FAILED;
        let info_name = self.info_name();

        while !hooks.is_stopped() && res != PAR_RESULT_SUCCESS {
            self.cleanup(repairer, dupe_sources);

            let opts = hooks.read_options();
            self.lock_signal().par_ignore_ext = opts.par_ignore_ext.clone();
            let cpu_cores = std::thread::available_parallelism()
                .map(|n| n.get() as i32)
                .unwrap_or(1);
            let monitor_mb = if opts.has_memory_monitor {
                crate::articledl::memory_monitor_effective_par_buffer_mb(opts.par_buffer_mb)
            } else {
                0
            };
            let (threads, mem) = crate::parchecker::calc_repairer_resources(
                opts.par_threads,
                cpu_cores,
                opts.has_memory_monitor,
                monitor_mb,
                opts.par_buffer_mb,
            );
            *threads_to_use = threads;
            *mem_to_use = mem;

            let dest_dir = self.dest_dir();
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
                format!("ParChecker: PreProcess-result={res}").as_bytes(),
            );

            if hooks.is_stopped() {
                hooks.print_message(
                    MK_ERROR,
                    &[b"Could not verify ", info_name.as_slice(), b": stopping"].concat(),
                );
                self.set_err_msg(b"par-check was stopped".to_vec());
                return PAR_RESULT_REPAIR_FAILED;
            }

            if res == PAR_RESULT_INVALID_ARGS {
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

            if res != PAR_RESULT_SUCCESS {
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

        if res != PAR_RESULT_SUCCESS {
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
    ) -> i32 {
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
        self.source_file_count
            .store(repairer.source_files().len(), Ordering::Release);
        hooks.global_log(
            MK_DEBUG,
            format!("ParChecker: Process-result={res}").as_bytes(),
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
    ) -> i32 {
        let mut res = PAR_RESULT_REPAIR_NOT_POSSIBLE;
        let mut more_files_loaded = true;
        let info_name = self.info_name();

        while !hooks.is_stopped() && res == PAR_RESULT_REPAIR_NOT_POSSIBLE {
            let missing_block_count =
                repairer.missing_block_count() as i32 - repairer.recovery_block_count() as i32;
            if missing_block_count <= 0 {
                return PAR_RESULT_REPAIR_POSSIBLE;
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
    ) -> i32 {
        let index = hooks.request_twin_index();
        if index.is_empty() || hooks.is_stopped() {
            return PAR_RESULT_REPAIR_NOT_POSSIBLE;
        }

        let own_par_filename = self.par_filename();
        let own_err_msg = self.get_err_msg();
        self.lock_meta().par_filename = index;
        self.set_err_msg(Vec::new());

        let mut res =
            self.pre_process_par(repairer, dupe_sources, threads_to_use, mem_to_use, hooks);
        if res != PAR_RESULT_SUCCESS || hooks.is_stopped() {
            self.lock_meta().par_filename = own_par_filename;
            self.set_err_msg(own_err_msg);
            return PAR_RESULT_REPAIR_NOT_POSSIBLE;
        }

        res = self.process_repairer(
            repairer.as_mut().expect("repairer initialized"),
            false,
            *threads_to_use,
            *mem_to_use,
            hooks,
        );
        if res == PAR_RESULT_REPAIR_NOT_POSSIBLE && !hooks.is_stopped() {
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

        if res == PAR_RESULT_REPAIR_NOT_POSSIBLE {
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

/// Host bridge for standalone `class ParChecker` (`ParCheckerTest.h`).
struct StandaloneParHooks {
    ctx: *mut c_void,
    host_cb: NzbgetRsRepairCtrlHostFn,
}

impl ParCheckerHooks for StandaloneParHooks {
    fn read_options(&self) -> ParCheckerOptions {
        let mut ints = [0 as c_int; 5];
        let mut strs = [ptr::null() as *const c_char; 1];
        unsafe {
            (self.host_cb)(
                self.ctx,
                0,
                0,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ints.as_mut_ptr(),
                ptr::null_mut(),
                strs.as_mut_ptr(),
            );
        }
        ParCheckerOptions {
            par_buffer_mb: ints[0],
            has_memory_monitor: ints[1] != 0,
            par_threads: ints[2],
            par_scan: ints[3],
            par_repair: ints[4] != 0,
            par_ignore_ext: unsafe { ptr_bytes(strs[0]) },
        }
    }

    fn request_more_pars(&self, block_needed: i32) -> (bool, i32) {
        let mut found = 0 as c_int;
        let ok = unsafe {
            (self.host_cb)(
                self.ctx,
                1,
                block_needed,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                &mut found,
                ptr::null_mut(),
                ptr::null_mut(),
            ) != 0
        };
        (ok, found)
    }

    fn register_verified_file(&self, filename: &[u8]) {
        let c_fn = to_c_buf(filename);
        unsafe {
            (self.host_cb)(
                self.ctx,
                2,
                0,
                0,
                0,
                0,
                c_fn.as_ptr() as *const c_char,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    fn find_file_quick_info(&self, filename: &[u8]) -> Option<QuickInfo> {
        let c_fn = to_c_buf(filename);
        let mut crc: u32 = 0;
        let mut raw_segs: Vec<NzbgetRsParSegment> = Vec::new();
        extern "C" fn push_seg(vec_ptr: usize, s: c_int, off: i64, sz: c_int, c: u32) {
            let v = unsafe { &mut *(vec_ptr as *mut Vec<NzbgetRsParSegment>) };
            v.push(NzbgetRsParSegment {
                success: s,
                offset: off,
                size: sz,
                crc: c,
            });
        }
        let status = unsafe {
            (self.host_cb)(
                self.ctx,
                3,
                0,
                0,
                (&mut crc as *mut u32) as usize,
                (&mut raw_segs as *mut Vec<NzbgetRsParSegment>) as usize,
                c_fn.as_ptr() as *const c_char,
                push_seg as *const () as *const c_char,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            ) as i32
        };
        quick_info_from_status(status, crc, &raw_segs)
    }

    fn global_log(&self, kind: i32, msg: &[u8]) {
        let c_msg = to_c_buf(msg);
        unsafe {
            (self.host_cb)(
                self.ctx,
                4,
                kind,
                0,
                0,
                0,
                c_msg.as_ptr() as *const c_char,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
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

pub unsafe fn par_checker_exec(
    checker: *const NzbgetRsParChecker,
    op: c_int,
    a1: c_int,
    s1: *const c_char,
    host_cb: Option<NzbgetRsRepairCtrlHostFn>,
    ctx: *mut c_void,
) -> c_int {
    let Some(c) = (unsafe { checker.as_ref() }) else {
        return 0;
    };
    match op {
        0 => {
            if let Some(cb) = host_cb {
                c.execute(&StandaloneParHooks { ctx, host_cb: cb });
            }
            0
        }
        1 => {
            c.set_dest_dir(unsafe { ptr_bytes(s1) });
            0
        }
        2 => {
            c.set_par_quick(a1 != 0);
            0
        }
        3 => c_int::from(c.par_full()),
        4 => c.status(),
        _ => 0,
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
    last_unpack_status: i32,
    direct_unpack_status: i32,
    par_status: i32,
    par_rename_status: i32,
    use_temp_unpack_dir: bool,
    unpack_cleanup_disk: bool,
    write_buffer_kb: i32,
    par_rename_opt: bool,
    umask: u32,
    parcheck_enabled: bool,
}

pub struct NzbgetRsUnpackController {
    terminated: AtomicBool,
    unpacking: AtomicBool,
    exec_thread_key: AtomicUsize,
}

impl Default for NzbgetRsUnpackController {
    fn default() -> Self {
        Self::new()
    }
}

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

    fn is_stopped(&self, ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn) -> bool {
        unsafe {
            host_cb(
                ctx,
                2,
                0,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            ) != 0
        }
    }

    fn is_stopped_or_terminated(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
    ) -> bool {
        self.is_stopped(ctx, host_cb) || self.terminated.load(Ordering::Acquire)
    }

    fn set_progress_label(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn, label: &[u8]) {
        let c_lbl = to_c_buf(label);
        unsafe {
            host_cb(
                ctx,
                2,
                4,
                0,
                0,
                0,
                c_lbl.as_ptr() as *const c_char,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    fn global_log(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn, kind: i32, msg: &[u8]) {
        let c_msg = to_c_buf(msg);
        unsafe {
            host_cb(
                ctx,
                2,
                7,
                kind,
                0,
                0,
                c_msg.as_ptr() as *const c_char,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    fn print_message(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        st: &mut UnpackRunState,
        kind: i32,
        msg: &[u8],
    ) {
        if let Some(prefix) = st.log_prefix {
            let full = [prefix, b": ", msg].concat();
            self.add_message(ctx, host_cb, st, kind, &full);
        } else {
            self.add_message(ctx, host_cb, st, kind, msg);
        }
    }

    fn add_message(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
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

        let c_msg = to_c_buf(&msg_text);
        unsafe {
            host_cb(
                ctx,
                2,
                3,
                kind,
                0,
                0,
                c_msg.as_ptr() as *const c_char,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }

        if !st.is_seven_zip && info.reset_stage_time != 0 {
            let mut now = current_time_sec();
            unsafe {
                host_cb(
                    ctx,
                    2,
                    6,
                    0,
                    0,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    &mut now,
                    ptr::null_mut(),
                );
            }
        }

        if !st.is_seven_zip && info.has_extracting_progress != 0 && msg_text.len() >= 7 {
            Self::set_progress_label(ctx, host_cb, &msg_text[7..]);
        }

        if !st.is_seven_zip && info.has_extracting_from != 0 && text.len() >= 7 {
            if text.len() >= 23 {
                Self::global_log(
                    ctx,
                    host_cb,
                    MK_DEBUG,
                    &[b"Filename: ", &text[23..]].concat(),
                );
            }
            Self::set_progress_label(ctx, host_cb, &text[7..]);
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

        if !self.is_stopped(ctx, host_cb)
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
                let c_warn = to_c_buf(&warn_msg);
                unsafe {
                    host_cb(
                        ctx,
                        2,
                        3,
                        MK_WARNING,
                        0,
                        0,
                        c_warn.as_ptr() as *const c_char,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            }
            st.auto_terminated = true;
            unsafe {
                host_cb(
                    ctx,
                    2,
                    1,
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

        if info.all_ok != 0 {
            st.all_ok_received = true;
        }
    }

    pub fn run(&self, ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn) {
        let start = current_time_sec();
        let mut st = UnpackRunState {
            unpack_ok: true,
            ..UnpackRunState::default()
        };

        let unpack = with_dq_guard(ctx, host_cb, |_| {
            let mut ints = [0 as c_int; 18];
            let mut strs = [ptr::null() as *const c_char; 7];
            unsafe {
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
                    ptr::null_mut(),
                    strs.as_mut_ptr(),
                );
            }
            st.dest_dir = unsafe { ptr_bytes(strs[0]) };
            st.final_dir = unsafe { ptr_bytes(strs[1]) };
            st.name = unsafe { ptr_bytes(strs[2]) };
            if ints[1] != 0 {
                st.password = unsafe { ptr_bytes(strs[3]) };
            }
            st.unpack_pass_file = unsafe { ptr_bytes(strs[4]) };
            st.unpack_ignore_ext = unsafe { ptr_bytes(strs[5]) };
            st.inter_dir_opt = unsafe { ptr_bytes(strs[6]) };

            let dl_count = ints[2].max(0) as usize;
            let ex_count = ints[3].max(0) as usize;
            st.downloaded_files.reserve(dl_count);
            for i in 0..dl_count {
                let mut s = [ptr::null() as *const c_char; 1];
                unsafe {
                    host_cb(
                        ctx,
                        1,
                        1,
                        i as c_int,
                        0,
                        0,
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        s.as_mut_ptr(),
                    );
                }
                st.downloaded_files.push(unsafe { ptr_bytes(s[0]) });
            }
            st.extracted_archives.reserve(ex_count);
            for i in 0..ex_count {
                let mut s = [ptr::null() as *const c_char; 1];
                unsafe {
                    host_cb(
                        ctx,
                        1,
                        2,
                        i as c_int,
                        0,
                        0,
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        s.as_mut_ptr(),
                    );
                }
                st.extracted_archives.push(unsafe { ptr_bytes(s[0]) });
            }

            st.unpack_tried = ints[4] != 0;
            st.par_repaired = ints[5] != 0;
            st.prev_pass_list_tried = ints[6] != 0;
            st.last_unpack_status = ints[7];
            st.direct_unpack_status = ints[8];
            st.par_status = ints[9];
            st.par_rename_status = ints[10];
            st.use_temp_unpack_dir = ints[12] != 0;
            st.unpack_cleanup_disk = ints[13] != 0;
            st.write_buffer_kb = ints[14];
            st.par_rename_opt = ints[15] != 0;
            st.umask = ints[16] as u32;
            st.parcheck_enabled = ints[17] != 0;

            ints[0] != 0
        });

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
        }

        let has_files = st.has_rar_files
            || st.has_seven_zip_files
            || st.has_seven_zip_multi_files
            || st.has_splitted_files;

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
            self.print_message(ctx, host_cb, &mut st, MK_INFO, &m1);
            let m2 = if st.last_unpack_status == 5 {
                [
                    st.info_name_up.as_slice(),
                    b" failed: checksum error in the encrypted file. Corrupt file or wrong password.",
                ]
                .concat()
            } else {
                [st.info_name_up.as_slice(), b" failed."].concat()
            };
            self.print_message(ctx, host_cb, &mut st, MK_ERROR, &m2);
            unsafe {
                host_cb(
                    ctx,
                    3,
                    0,
                    st.last_unpack_status,
                    0,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
        } else if unpack && has_files {
            let m = [b"Unpacking ", st.name.as_slice()].concat();
            self.print_message(ctx, host_cb, &mut st, MK_INFO, &m);
            self.create_unpack_dir(ctx, host_cb, &mut st);

            if st.has_rar_files {
                if st.has_not_unpacked_rar_files || st.unpack_dir_created {
                    if st.direct_unpack_status == 3 {
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
                        self.print_message(ctx, host_cb, &mut st, kind, txt);
                    }
                    unsafe {
                        host_cb(
                            ctx,
                            1,
                            3,
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
                    self.unpack_archives(ctx, host_cb, &mut st, false, false);
                } else {
                    self.print_message(
                        ctx,
                        host_cb,
                        &mut st,
                        MK_INFO,
                        b"Using directly unpacked files",
                    );
                }
            }

            if st.has_seven_zip_files && st.unpack_ok {
                self.unpack_archives(ctx, host_cb, &mut st, true, false);
            }
            if st.has_seven_zip_multi_files && st.unpack_ok {
                self.unpack_archives(ctx, host_cb, &mut st, true, true);
            }
            if st.has_splitted_files && st.unpack_ok {
                self.join_splitted_files(ctx, host_cb, &mut st);
            }

            self.completed(ctx, host_cb, &mut st);
            st.joined_files.clear();
        } else if unpack {
            if st.parcheck_enabled
                && st.par_status <= 1
                && st.par_rename_status <= 1
                && st.has_par_files
            {
                let m = [b"Nothing to unpack for ", st.name.as_slice()].concat();
                self.print_message(ctx, host_cb, &mut st, MK_INFO, &m);
                self.request_par_check(ctx, host_cb, &mut st, false);
            } else if st.has_renamed_archive_files {
                let m = [
                    b"Could not unpack ",
                    st.name.as_slice(),
                    b" due to renamed archive files",
                ]
                .concat();
                self.print_message(ctx, host_cb, &mut st, MK_ERROR, &m);
                unsafe {
                    host_cb(
                        ctx,
                        3,
                        0,
                        2,
                        0,
                        0,
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            } else {
                let m = [b"Nothing to unpack for ", st.name.as_slice()].concat();
                self.print_message(ctx, host_cb, &mut st, MK_INFO, &m);
                unsafe {
                    host_cb(
                        ctx,
                        3,
                        0,
                        1,
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
        } else {
            let m = [b"Unpack for ", st.name.as_slice(), b" skipped"].concat();
            self.print_message(ctx, host_cb, &mut st, MK_INFO, &m);
            unsafe {
                host_cb(
                    ctx,
                    3,
                    0,
                    1,
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

        let elapsed = (current_time_sec() - start) as c_int;
        unsafe {
            host_cb(
                ctx,
                3,
                3,
                elapsed,
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

    fn create_unpack_dir(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        st: &mut UnpackRunState,
    ) {
        let sep = crate::paths::PATH_SEPARATOR;
        st.inter_dir = crate::unpack::use_inter_dir(&st.dest_dir, &st.inter_dir_opt, sep);

        if st.inter_dir && st.final_dir.is_empty() {
            let mut built = Vec::new();
            extern "C" fn set_vec(vec_ptr: usize, s: *const c_char) {
                let v = unsafe { &mut *(vec_ptr as *mut Vec<u8>) };
                *v = unsafe { ptr_bytes(s) };
            }
            unsafe {
                host_cb(
                    ctx,
                    3,
                    5,
                    0,
                    (&mut built as *mut Vec<u8>) as usize,
                    set_vec as *const () as usize,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
            st.final_dir = built;
            let c_fin = to_c_buf(&st.final_dir);
            with_dq_guard(ctx, host_cb, |_| unsafe {
                host_cb(
                    ctx,
                    3,
                    6,
                    0,
                    0,
                    0,
                    c_fin.as_ptr() as *const c_char,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            });
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

        Self::global_log(
            ctx,
            host_cb,
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
            self.print_message(ctx, host_cb, st, MK_ERROR, &m);
        }
    }

    fn unpack_archives(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        st: &mut UnpackRunState,
        is_seven_zip: bool,
        multi_volumes: bool,
    ) {
        if !st.unpack_tried || st.par_repaired {
            let pw = st.password.clone();
            self.execute_unpack(ctx, host_cb, st, is_seven_zip, &pw, multi_volumes);
            if !st.unpack_ok && st.has_par_files && !st.unpack_password_error && st.par_status <= 1
            {
                Self::global_log(
                    ctx,
                    host_cb,
                    MK_DEBUG,
                    b"For rar4- or 7z-archives try par-check first, before trying password file",
                );
                return;
            }
        } else {
            st.unpack_ok = false;
            st.unpack_decrypt_error = st.last_unpack_status == 5;
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
                    Self::global_log(ctx, host_cb, MK_DEBUG, &[b"Password line: ", pw].concat());
                    if self.is_stopped(ctx, host_cb) && s.auto_terminated {
                        self.terminated.store(false, Ordering::Release);
                        unsafe {
                            host_cb(
                                ctx,
                                2,
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
                    s.unpack_decrypt_error = false;
                    s.unpack_password_error = false;
                    s.auto_terminated = false;
                    let m = [b"Trying password ", pw, b" for ", s.name.as_slice()].concat();
                    self.print_message(ctx, host_cb, &mut s, MK_INFO, &m);
                    self.execute_unpack(ctx, host_cb, &mut s, is_seven_zip, pw, multi_volumes);
                },
            );

            if !opened {
                let m = [b"Could not open file ", pass_file.as_slice()].concat();
                self.print_message(ctx, host_cb, st, MK_ERROR, &m);
                return;
            }

            st.pass_list_tried = !self.is_stopped(ctx, host_cb) || st.auto_terminated;
        }
    }

    fn execute_unpack(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        st: &mut UnpackRunState,
        is_seven_zip: bool,
        password: &[u8],
        multi_volumes: bool,
    ) {
        st.all_ok_received = false;
        st.unpack_space_error = false;
        st.is_seven_zip = is_seven_zip;

        if is_seven_zip {
            self.print_message(ctx, host_cb, st, MK_INFO, b"Executing 7-Zip");
            st.log_prefix = Some(b"7-Zip");
        }
        Self::set_progress_label(ctx, host_cb, b"");

        let _unpack_guard = UNPACK_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        self.exec_thread_key
            .store(current_thread_key(), Ordering::Release);
        self.unpacking.store(true, Ordering::Release);

        let dest_dir = st.dest_dir.clone();
        let unpack_dir = st.unpack_dir.clone();
        let dl_snapshot = st.downloaded_files.clone();
        let dl_refs: Vec<&[u8]> = dl_snapshot.iter().map(Vec::as_slice).collect();
        let use_temp = st.use_temp_unpack_dir;
        let cleanup_disk = st.unpack_cleanup_disk;
        let pw_str = std::str::from_utf8(password).unwrap_or("");

        let st_cell = std::cell::RefCell::new(&mut *st);
        let all_ok = crate::unpack::extract_discovered_archives(
            &dest_dir,
            crate::fsops::path(&unpack_dir),
            pw_str,
            is_seven_zip,
            multi_volumes,
            use_temp,
            cleanup_disk,
            &dl_refs,
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
                    ctx,
                    host_cb,
                    &mut st_cell.borrow_mut(),
                    msg_kind,
                    msg.as_bytes(),
                );
            },
            &mut || self.is_stopped_or_terminated(ctx, host_cb),
        );

        self.unpacking.store(false, Ordering::Release);
        self.exec_thread_key.store(0, Ordering::Release);
        drop(_unpack_guard);

        st.log_prefix = None;
        Self::set_progress_label(ctx, host_cb, b"");

        st.unpack_ok = all_ok
            && st.all_ok_received
            && !self.terminated.load(Ordering::Acquire)
            && !self.is_stopped(ctx, host_cb);
        st.unpack_start_error = false;
    }

    fn join_splitted_files(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        st: &mut UnpackRunState,
    ) {
        st.log_prefix = Some(b"Join");
        Self::set_progress_label(ctx, host_cb, b"");
        unsafe {
            host_cb(
                ctx,
                2,
                5,
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

        let dest_dir = st.dest_dir.clone();
        let unpack_dir = st.unpack_dir.clone();
        let par_success = st.par_status == 3;
        let write_buf = st.write_buffer_kb;
        let mut joined = std::mem::take(&mut st.joined_files);

        let ok = crate::unpack::join_splitted_files(
            &dest_dir,
            &unpack_dir,
            par_success,
            write_buf,
            &mut joined,
            &mut |kind, msg| {
                let msg_kind = match kind {
                    0 => MK_INFO,
                    1 => MK_ERROR,
                    2 => MK_WARNING,
                    _ => MK_DETAIL,
                };
                self.print_message(ctx, host_cb, st, msg_kind, msg);
            },
            &mut |label, stage_progress| {
                if let Some(lbl) = label {
                    Self::set_progress_label(ctx, host_cb, lbl);
                }
                if stage_progress >= 0 {
                    unsafe {
                        host_cb(
                            ctx,
                            2,
                            5,
                            stage_progress,
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
            },
        );
        st.joined_files = joined;
        if !ok {
            st.unpack_ok = false;
        }
        st.log_prefix = None;
        Self::set_progress_label(ctx, host_cb, b"");
    }

    fn cleanup(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        st: &mut UnpackRunState,
    ) -> bool {
        let dest_dir = st.dest_dir.clone();
        let final_dir = st.final_dir.clone();
        let unpack_dir = st.unpack_dir.clone();
        let dl_snapshot = st.downloaded_files.clone();
        let jn_snapshot = st.joined_files.clone();
        let dl_refs: Vec<&[u8]> = dl_snapshot.iter().map(Vec::as_slice).collect();
        let jn_refs: Vec<&[u8]> = jn_snapshot.iter().map(Vec::as_slice).collect();
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
            &mut |kind, msg| {
                let msg_kind = match kind {
                    0 => MK_INFO,
                    1 => MK_ERROR,
                    2 => MK_WARNING,
                    _ => MK_DETAIL,
                };
                self.print_message(ctx, host_cb, st, msg_kind, msg);
            },
        );
        st.cleaned_up_disk = cleaned;
        ok
    }

    fn completed(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        st: &mut UnpackRunState,
    ) {
        let deleted = unsafe {
            host_cb(
                ctx,
                2,
                8,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            ) != 0
        };
        if !st.auto_terminated && !deleted && self.is_stopped_or_terminated(ctx, host_cb) {
            Self::global_log(
                ctx,
                host_cb,
                MK_DEBUG,
                b"Unpack stopped due to reload/shutdown, preserving _unpack directory",
            );
            return;
        }

        let cleanup_success = self.cleanup(ctx, host_cb, st);
        if st.unpack_ok && cleanup_success {
            let m = [st.info_name_up.as_slice(), b" successful"].concat();
            self.print_message(ctx, host_cb, st, MK_INFO, &m);
            unsafe {
                host_cb(
                    ctx,
                    3,
                    1,
                    c_int::from(st.cleaned_up_disk),
                    usize::from(st.par_rename_opt),
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
        } else {
            let mut par_info = [0 as c_int; 2];
            unsafe {
                host_cb(
                    ctx,
                    3,
                    4,
                    0,
                    0,
                    0,
                    ptr::null(),
                    ptr::null(),
                    par_info.as_mut_ptr(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
            let (req_par, force_rep, unpack_status) = crate::unpack::eval_unpack_completed_failure(
                st.parcheck_enabled,
                st.unpack_ok,
                par_info[0],
                1,
                par_info[1] != 0,
                st.unpack_start_error,
                st.unpack_space_error,
                st.unpack_password_error,
                st.unpack_decrypt_error,
                self.terminated.load(Ordering::Acquire),
                self.is_stopped(ctx, host_cb),
                st.auto_terminated,
                st.has_par_files,
                !st.password.is_empty(),
                !st.unpack_pass_file.is_empty(),
                st.pass_list_tried,
            );
            if st.parcheck_enabled && req_par {
                self.request_par_check(ctx, host_cb, st, force_rep);
            } else {
                let m = [st.info_name_up.as_slice(), b" failed"].concat();
                self.print_message(ctx, host_cb, st, MK_ERROR, &m);
                unsafe {
                    host_cb(
                        ctx,
                        3,
                        0,
                        unpack_status,
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
        }
    }

    fn request_par_check(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        st: &mut UnpackRunState,
        force_repair: bool,
    ) {
        let suffix: &[u8] = if force_repair {
            b" requested par-check with forced repair"
        } else {
            b" requested par-check/repair"
        };
        let m = [st.info_name_up.as_slice(), suffix].concat();
        self.print_message(ctx, host_cb, st, MK_INFO, &m);
        let last_status = if st.unpack_space_error {
            4
        } else if st.unpack_password_error || st.unpack_decrypt_error {
            5
        } else {
            2
        };
        let mut ints = [
            c_int::from(force_repair),
            c_int::from(st.pass_list_tried),
            last_status,
        ];
        unsafe {
            host_cb(
                ctx,
                3,
                2,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ints.as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }
}

pub unsafe fn unpack_ctrl_exec(
    ctrl: *const NzbgetRsUnpackController,
    op: c_int,
    host_cb: Option<NzbgetRsRepairCtrlHostFn>,
    ctx: *mut c_void,
) -> c_int {
    let Some(c) = (unsafe { ctrl.as_ref() }) else {
        return 0;
    };
    match op {
        0 => {
            if let Some(cb) = host_cb {
                c.run(ctx, cb);
            }
            0
        }
        1 => {
            c.stop();
            0
        }
        _ => 0,
    }
}

// ============================================================================
// 3. DirectUnpack (`daemon/postprocess/DirectUnpack.cpp`)
// ============================================================================

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
}

pub struct NzbgetRsDirectUnpack {
    nzb_id: i32,
    vol: Mutex<DirectUnpackVolumeState>,
    extra_start_time: AtomicI64,
    processed: AtomicBool,
    cancelled: AtomicBool,
    waiting_volume: AtomicBool,
    unpacking: AtomicBool,
    terminated: AtomicBool,
    exec_thread_key: AtomicUsize,
}

impl NzbgetRsDirectUnpack {
    pub fn new(nzb_id: i32) -> Self {
        Self {
            nzb_id,
            vol: Mutex::new(DirectUnpackVolumeState::default()),
            extra_start_time: AtomicI64::new(0),
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

    fn is_stopped(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn) -> bool {
        unsafe {
            host_cb(
                ctx,
                2,
                0,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            ) != 0
        }
    }

    fn is_stopped_or_terminated(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
    ) -> bool {
        Self::is_stopped(ctx, host_cb) || self.terminated.load(Ordering::Acquire)
    }

    fn global_log(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn, kind: i32, msg: &[u8]) {
        let c_msg = to_c_buf(msg);
        unsafe {
            host_cb(
                ctx,
                2,
                3,
                kind,
                0,
                0,
                c_msg.as_ptr() as *const c_char,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    fn add_extra_time(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        nzb_ptr: usize,
        clear_working: bool,
    ) {
        let start = self.extra_start_time.swap(0, Ordering::AcqRel);
        let extra_sec = if start != 0 {
            (current_time_sec() - start) as c_int
        } else {
            0
        };
        if start != 0 || clear_working {
            unsafe {
                host_cb(
                    ctx,
                    1,
                    5,
                    extra_sec,
                    nzb_ptr,
                    usize::from(clear_working) | (if start != 0 { 2 } else { 0 }),
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
        }
    }

    pub fn stop_with_nzb(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        nzb_ptr: usize,
    ) {
        let info_name = self.lock_vol().info_name.clone();
        Self::global_log(
            ctx,
            host_cb,
            MK_DEBUG,
            &[b"Stopping ", info_name.as_slice()].concat(),
        );

        let pp_stopped = unsafe {
            host_cb(
                ctx,
                2,
                2,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            ) != 0
        };
        if !pp_stopped {
            self.cancelled.store(true, Ordering::Release);
        }

        if self.processed.load(Ordering::Acquire) {
            let cancel_msg = trunc_1023([b"Cancelling ", info_name.as_slice()].concat());
            if nzb_ptr != 0 {
                let c_msg = to_c_buf(&cancel_msg);
                unsafe {
                    host_cb(
                        ctx,
                        1,
                        3,
                        MK_WARNING,
                        nzb_ptr,
                        0,
                        c_msg.as_ptr() as *const c_char,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            } else {
                Self::global_log(ctx, host_cb, MK_WARNING, &cancel_msg);
            }
        }

        if nzb_ptr != 0 {
            self.add_extra_time(ctx, host_cb, nzb_ptr, true);
        }

        unsafe {
            host_cb(
                ctx,
                2,
                1,
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

        if self.unpacking.load(Ordering::Acquire) {
            self.terminated.store(true, Ordering::Release);
        }
    }

    pub fn file_downloaded(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        fi_ptr: usize,
    ) {
        let mut ints = [0 as c_int; 1];
        let mut strs = [ptr::null() as *const c_char; 2];
        let nzb_ptr = unsafe {
            host_cb(
                ctx,
                1,
                1,
                0,
                fi_ptr,
                0,
                ptr::null(),
                ptr::null(),
                ints.as_mut_ptr(),
                ptr::null_mut(),
                strs.as_mut_ptr(),
            )
        };
        let nzb_name = unsafe { ptr_bytes(strs[0]) };
        let filename = unsafe { ptr_bytes(strs[1]) };
        let failed_articles = ints[0];

        Self::global_log(
            ctx,
            host_cb,
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
            self.stop_with_nzb(ctx, host_cb, nzb_ptr);
            return;
        }

        let mut v = self.lock_vol();
        if v.waiting_file
            .as_ref()
            .is_some_and(|wf| wf.eq_ignore_ascii_case(&filename))
        {
            v.waiting_file = None;
            self.waiting_volume.store(false, Ordering::Release);
        }

        if crate::unpack::direct_unpack_is_main_archive(&filename) {
            v.archives.push_back(filename);
        }
    }

    pub fn nzb_downloaded(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        nzb_ptr: usize,
    ) {
        let mut ints = [0 as c_int; 2];
        let mut strs = [ptr::null() as *const c_char; 2];
        unsafe {
            host_cb(
                ctx,
                1,
                2,
                0,
                nzb_ptr,
                0,
                ptr::null(),
                ptr::null(),
                ints.as_mut_ptr(),
                ptr::null_mut(),
                strs.as_mut_ptr(),
            );
        }
        let nzb_name = unsafe { ptr_bytes(strs[0]) };
        Self::global_log(
            ctx,
            host_cb,
            MK_DEBUG,
            &[b"NzbDownloaded for ", nzb_name.as_slice()].concat(),
        );

        let (waiting_missing, progress_label) = {
            let mut v = self.lock_vol();
            v.nzb_completed = true;
            if let Some(ref wf) = v.waiting_file {
                (Some(wf.clone()), Vec::new())
            } else {
                (None, v.progress_label.clone())
            }
        };

        if let Some(wf) = waiting_missing {
            let msg = trunc_1023([b"Could not find volume ", wf.as_slice()].concat());
            let c_msg = to_c_buf(&msg);
            unsafe {
                host_cb(
                    ctx,
                    1,
                    3,
                    MK_WARNING,
                    nzb_ptr,
                    0,
                    c_msg.as_ptr() as *const c_char,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
            self.stop_with_nzb(ctx, host_cb, nzb_ptr);
            return;
        }

        self.extra_start_time
            .store(current_time_sec(), Ordering::Release);
        if ints[0] != 0 {
            let c_lbl = to_c_buf(&progress_label);
            unsafe {
                host_cb(
                    ctx,
                    1,
                    4,
                    0,
                    nzb_ptr,
                    0,
                    c_lbl.as_ptr() as *const c_char,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
        }
    }

    pub fn nzb_deleted(&self, ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn, nzb_ptr: usize) {
        let mut ints = [0 as c_int; 2];
        let mut strs = [ptr::null() as *const c_char; 2];
        unsafe {
            host_cb(
                ctx,
                1,
                2,
                0,
                nzb_ptr,
                0,
                ptr::null(),
                ptr::null(),
                ints.as_mut_ptr(),
                ptr::null_mut(),
                strs.as_mut_ptr(),
            );
        }
        let nzb_name = unsafe { ptr_bytes(strs[0]) };
        Self::global_log(
            ctx,
            host_cb,
            MK_DEBUG,
            &[b"NzbDeleted for ", nzb_name.as_slice()].concat(),
        );
        unsafe {
            host_cb(
                ctx,
                1,
                6,
                0,
                nzb_ptr,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
        self.stop_with_nzb(ctx, host_cb, nzb_ptr);
    }

    fn add_message(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        kind: i32,
        text: &[u8],
        local: &mut DirectUnpackRunLocal,
    ) {
        Self::global_log(ctx, host_cb, MK_DEBUG, text);

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
            self.wait_next_volume(ctx, host_cb, &filename, local);
            return;
        }

        with_dq_guard(ctx, host_cb, |dq_ptr| {
            let mut ints = [0 as c_int; 6];
            let mut strs = [ptr::null() as *const c_char; 4];
            let nzb_ptr = unsafe {
                host_cb(
                    ctx,
                    1,
                    0,
                    self.nzb_id,
                    dq_ptr,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ints.as_mut_ptr(),
                    ptr::null_mut(),
                    strs.as_mut_ptr(),
                )
            };

            let c_msg = to_c_buf(&msg_text);
            if nzb_ptr != 0 {
                unsafe {
                    host_cb(
                        ctx,
                        1,
                        3,
                        kind,
                        nzb_ptr,
                        0,
                        c_msg.as_ptr() as *const c_char,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            } else {
                Self::global_log(ctx, host_cb, kind, &msg_text);
            }

            if info.has_extracting_progress != 0 && nzb_ptr != 0 && msg_text.len() >= 7 {
                let lbl = msg_text[7..].to_vec();
                self.lock_vol().progress_label = lbl.clone();
                if ints[4] != 0 {
                    let c_lbl = to_c_buf(&lbl);
                    unsafe {
                        host_cb(
                            ctx,
                            1,
                            4,
                            0,
                            nzb_ptr,
                            0,
                            c_lbl.as_ptr() as *const c_char,
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                    }
                }
            }

            if info.has_extracting_from != 0 && nzb_ptr != 0 && text.len() >= 7 {
                let lbl = text[7..].to_vec();
                self.lock_vol().progress_label = lbl.clone();
                if ints[4] != 0 {
                    let c_lbl = to_c_buf(&lbl);
                    unsafe {
                        host_cb(
                            ctx,
                            1,
                            4,
                            0,
                            nzb_ptr,
                            0,
                            c_lbl.as_ptr() as *const c_char,
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                    }
                }
                if text.len() >= 23 {
                    local.extracted_archives.push(text[23..].to_vec());
                }
            }

            if !Self::is_stopped(ctx, host_cb)
                && (info.decrypt_error != 0
                    || info.direct_password_error != 0
                    || info.crc_or_volume_error != 0)
            {
                self.stop_with_nzb(ctx, host_cb, nzb_ptr);
            }
        });

        if info.all_ok != 0 {
            local.all_ok_received = true;
        }
    }

    fn wait_next_volume(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        filename: &[u8],
        local: &mut DirectUnpackRunLocal,
    ) {
        Self::global_log(
            ctx,
            host_cb,
            MK_DEBUG,
            &[b"WaitNextVolume for ", filename].concat(),
        );

        with_dq_guard(ctx, host_cb, |dq_ptr| {
            let mut ints = [0 as c_int; 6];
            let mut strs = [ptr::null() as *const c_char; 4];
            let mut cur_final = Vec::new();
            extern "C" fn set_vec(vec_ptr: usize, s: *const c_char) {
                let v = unsafe { &mut *(vec_ptr as *mut Vec<u8>) };
                *v = unsafe { ptr_bytes(s) };
            }
            let nzb_ptr = unsafe {
                host_cb(
                    ctx,
                    1,
                    0,
                    self.nzb_id,
                    dq_ptr,
                    (&mut cur_final as *mut Vec<u8>) as usize,
                    ptr::null(),
                    set_vec as *const () as *const c_char,
                    ints.as_mut_ptr(),
                    ptr::null_mut(),
                    strs.as_mut_ptr(),
                )
            };
            if nzb_ptr != 0 {
                let cur_dest = unsafe { ptr_bytes(strs[1]) };
                if cur_dest != local.dest_dir || cur_final != local.final_dir {
                    let cur_name = unsafe { ptr_bytes(strs[0]) };
                    let msg = trunc_1023(
                        [
                            b"Stopping direct unpack for ",
                            cur_name.as_slice(),
                            b" due to changed destination directory",
                        ]
                        .concat(),
                    );
                    let c_msg = to_c_buf(&msg);
                    unsafe {
                        host_cb(
                            ctx,
                            1,
                            3,
                            MK_WARNING,
                            nzb_ptr,
                            0,
                            c_msg.as_ptr() as *const c_char,
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                    }
                    self.stop_with_nzb(ctx, host_cb, nzb_ptr);
                }
            }
        });

        let sep = crate::paths::PATH_SEPARATOR;
        let full_filename = [local.dest_dir.as_slice(), &[sep], filename].concat();
        if crate::fsops::file_exists(&full_filename, &crate::fsops::posix_stat) {
            self.waiting_volume.store(false, Ordering::Release);
        } else {
            let nzb_completed = {
                let mut v = self.lock_vol();
                v.waiting_file = Some(filename.to_vec());
                self.waiting_volume.store(true, Ordering::Release);
                v.nzb_completed
            };
            if nzb_completed {
                let msg = [b"Could not find volume ", filename].concat();
                self.add_message(ctx, host_cb, MK_WARNING, &msg, local);
                with_dq_guard(ctx, host_cb, |dq_ptr| {
                    let mut ints = [0 as c_int; 6];
                    let mut strs = [ptr::null() as *const c_char; 4];
                    let nzb_ptr = unsafe {
                        host_cb(
                            ctx,
                            1,
                            0,
                            self.nzb_id,
                            dq_ptr,
                            0,
                            ptr::null(),
                            ptr::null(),
                            ints.as_mut_ptr(),
                            ptr::null_mut(),
                            strs.as_mut_ptr(),
                        )
                    };
                    self.stop_with_nzb(ctx, host_cb, nzb_ptr);
                });
            }
        }
    }

    fn create_unpack_dir(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        local: &mut DirectUnpackRunLocal,
    ) {
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
            ctx,
            host_cb,
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
            self.add_message(ctx, host_cb, MK_ERROR, &m, local);
        }
    }

    fn cleanup(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        local: &mut DirectUnpackRunLocal,
    ) {
        Self::global_log(
            ctx,
            host_cb,
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
                self.add_message(ctx, host_cb, mk, msg, local);
            },
        );
    }

    fn execute_unrar(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        archive_name: &[u8],
        local: &mut DirectUnpackRunLocal,
    ) {
        local.all_ok_received = false;

        let _unpack_guard = UNPACK_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        if Self::is_stopped(ctx, host_cb) {
            return;
        }
        self.waiting_volume.store(false, Ordering::Release);
        self.exec_thread_key
            .store(current_thread_key(), Ordering::Release);
        self.unpacking.store(true, Ordering::Release);

        let sep = crate::paths::PATH_SEPARATOR;
        let full_archive_path =
            trunc_1023([local.dest_dir.as_slice(), &[sep], archive_name].concat());
        let unpack_dir = local.unpack_dir.clone();
        let pw_bytes = local.password.clone();
        let pw_str = std::str::from_utf8(&pw_bytes).unwrap_or("");
        let cleanup_disk = local.unpack_cleanup_disk;

        let res = crate::unpack::extract_archive_with_zipfast_cancellable(
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
                self.add_message(ctx, host_cb, msg_kind, msg.as_bytes(), local);
                while self.waiting_volume.load(Ordering::Acquire)
                    && !self.is_stopped_or_terminated(ctx, host_cb)
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
            },
            &mut || self.is_stopped_or_terminated(ctx, host_cb),
        );

        self.unpacking.store(false, Ordering::Release);
        self.exec_thread_key.store(0, Ordering::Release);
        self.waiting_volume.store(false, Ordering::Release);
        drop(_unpack_guard);

        local.unpack_ok = res.is_ok()
            && local.all_ok_received
            && !self.terminated.load(Ordering::Acquire)
            && !Self::is_stopped(ctx, host_cb);
    }

    pub fn run(&self, ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn) {
        Self::global_log(
            ctx,
            host_cb,
            MK_DEBUG,
            format!("Entering DirectUnpack-loop for {}", self.nzb_id).as_bytes(),
        );

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
        };

        let found = with_dq_guard(ctx, host_cb, |dq_ptr| {
            let mut ints = [0 as c_int; 6];
            let mut strs = [ptr::null() as *const c_char; 4];
            let mut built_final = Vec::new();
            extern "C" fn set_vec(vec_ptr: usize, s: *const c_char) {
                let v = unsafe { &mut *(vec_ptr as *mut Vec<u8>) };
                *v = unsafe { ptr_bytes(s) };
            }
            let nzb_ptr = unsafe {
                host_cb(
                    ctx,
                    1,
                    0,
                    self.nzb_id,
                    dq_ptr,
                    (&mut built_final as *mut Vec<u8>) as usize,
                    ptr::null(),
                    set_vec as *const () as *const c_char,
                    ints.as_mut_ptr(),
                    ptr::null_mut(),
                    strs.as_mut_ptr(),
                )
            };
            if nzb_ptr == 0 {
                return false;
            }
            local.name = unsafe { ptr_bytes(strs[0]) };
            local.dest_dir = unsafe { ptr_bytes(strs[1]) };
            local.final_dir = built_final;
            if ints[0] != 0 {
                local.password = unsafe { ptr_bytes(strs[2]) };
            }
            local.inter_dir_opt = unsafe { ptr_bytes(strs[3]) };
            local.use_temp_unpack_dir = ints[1] != 0;
            local.unpack_cleanup_disk = ints[2] != 0;
            true
        });

        if !found {
            Self::global_log(
                ctx,
                host_cb,
                MK_DEBUG,
                format!("Could not find NzbInfo for {}", self.nzb_id).as_bytes(),
            );
            self.unpacking.store(false, Ordering::Release);
            return;
        }

        local.info_name = [b"direct unpack for ", local.name.as_slice()].concat();
        local.info_name_up = [b"Direct unpack for ", local.name.as_slice()].concat();

        {
            let mut v = self.lock_vol();
            v.info_name = local.info_name.clone();
            for arch in crate::unpack::direct_unpack_find_archive_files(&local.dest_dir) {
                v.archives.push_back(arch);
            }
        }

        while !Self::is_stopped(ctx, host_cb) {
            let next_archive = self.lock_vol().archives.pop_front();
            if let Some(archive) = next_archive {
                self.waiting_volume.store(false, Ordering::Release);
                if !self.processed.load(Ordering::Acquire) {
                    let m = [b"Directly unpacking ", local.name.as_slice()].concat();
                    self.add_message(ctx, host_cb, MK_INFO, &m, &mut local);
                    self.cleanup(ctx, host_cb, &mut local);
                    self.create_unpack_dir(ctx, host_cb, &mut local);
                    self.processed.store(true, Ordering::Release);
                }

                let committed = local.extracted_archives.len();
                self.execute_unrar(ctx, host_cb, &archive, &mut local);
                if !local.unpack_ok {
                    local.extracted_archives.truncate(committed);
                    break;
                }
            } else {
                if self.lock_vol().nzb_completed {
                    break;
                }
                self.waiting_volume.store(true, Ordering::Release);
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        self.unpacking.store(false, Ordering::Release);
        self.waiting_volume.store(false, Ordering::Release);

        let cancelled = self.cancelled.load(Ordering::Acquire);
        let interrupted = !cancelled && self.is_stopped_or_terminated(ctx, host_cb);
        if (!local.unpack_ok && !interrupted) || cancelled {
            self.cleanup(ctx, host_cb, &mut local);
        }

        let valid_owner = with_dq_guard(ctx, host_cb, |dq_ptr| {
            let mut ints = [0 as c_int; 6];
            let mut strs = [ptr::null() as *const c_char; 4];
            let nzb_ptr = unsafe {
                host_cb(
                    ctx,
                    1,
                    0,
                    self.nzb_id,
                    dq_ptr,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ints.as_mut_ptr(),
                    ptr::null_mut(),
                    strs.as_mut_ptr(),
                )
            };
            if nzb_ptr == 0 || ints[3] == 0 {
                return false;
            }

            let processed = self.processed.load(Ordering::Acquire);
            let status = if !processed {
                0 // nsNone
            } else if local.unpack_ok && !interrupted && !cancelled {
                3 // nsSuccess
            } else if interrupted {
                0 // nsNone
            } else {
                2 // nsFailure
            };

            if interrupted {
                Self::global_log(
                    ctx,
                    host_cb,
                    MK_DEBUG,
                    &[
                        local.info_name.as_slice(),
                        b" stopped due to reload/shutdown, preserving unpack state",
                    ]
                    .concat(),
                );
            } else if status == 3 || status == 2 {
                let ok = status == 3;
                let suffix: &[u8] = if ok { b" successful" } else { b" failed" };
                let msg = trunc_1023([local.info_name_up.as_slice(), suffix].concat());
                let c_msg = to_c_buf(&msg);
                unsafe {
                    host_cb(
                        ctx,
                        1,
                        3,
                        if ok { MK_INFO } else { MK_WARNING },
                        nzb_ptr,
                        0,
                        c_msg.as_ptr() as *const c_char,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            }

            if status == 3 || interrupted {
                for arch in &local.extracted_archives {
                    let c_arch = to_c_buf(arch);
                    unsafe {
                        host_cb(
                            ctx,
                            1,
                            8,
                            0,
                            nzb_ptr,
                            0,
                            c_arch.as_ptr() as *const c_char,
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                    }
                }
            }

            self.add_extra_time(ctx, host_cb, nzb_ptr, true);

            unsafe {
                host_cb(
                    ctx,
                    1,
                    7,
                    status,
                    nzb_ptr,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
            true
        });

        if !valid_owner {
            Self::global_log(
                ctx,
                host_cb,
                MK_DEBUG,
                &[b"Could not find NzbInfo for ", local.info_name.as_slice()].concat(),
            );
            return;
        }

        Self::global_log(
            ctx,
            host_cb,
            MK_DEBUG,
            format!("Exiting DirectUnpack-loop for {}", self.nzb_id).as_bytes(),
        );
    }
}

pub unsafe fn direct_unpack_exec(
    ctrl: *const NzbgetRsDirectUnpack,
    op: c_int,
    p1: usize,
    p2: usize,
    host_cb: Option<NzbgetRsRepairCtrlHostFn>,
    ctx: *mut c_void,
) -> c_int {
    let Some(c) = (unsafe { ctrl.as_ref() }) else {
        return 0;
    };
    match op {
        0 => {
            if let Some(cb) = host_cb {
                c.run(ctx, cb);
            }
            0
        }
        1 => {
            if let Some(cb) = host_cb {
                let _ = p1;
                c.stop_with_nzb(ctx, cb, p2);
            }
            0
        }
        2 => {
            if let Some(cb) = host_cb {
                let _ = p1;
                c.file_downloaded(ctx, cb, p2);
            }
            0
        }
        3 => {
            if let Some(cb) = host_cb {
                let _ = p1;
                c.nzb_downloaded(ctx, cb, p2);
            }
            0
        }
        4 => {
            if let Some(cb) = host_cb {
                let _ = p1;
                c.nzb_deleted(ctx, cb, p2);
            }
            0
        }
        5 => c_int::from(c.is_unpacking()),
        6 => c_int::from(c.is_waiting_volume()),
        _ => 0,
    }
}

// ============================================================================
// 4. Move, Cleanup, Rename, PostRenamer & PostScript Controllers
// ============================================================================

fn map_cleanup_kind(kind: crate::cleanup::CleanupLogKind) -> i32 {
    match kind {
        crate::cleanup::CleanupLogKind::Info => MK_INFO,
        crate::cleanup::CleanupLogKind::Warning => MK_WARNING,
        crate::cleanup::CleanupLogKind::Error => MK_ERROR,
        crate::cleanup::CleanupLogKind::Detail => MK_DETAIL,
    }
}

fn emit_job_msg(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn, kind: i32, msg: &[u8]) {
    let c_msg = to_c_buf(msg);
    unsafe {
        host_cb(
            ctx,
            2,
            1,
            kind,
            0,
            0,
            c_msg.as_ptr() as *const c_char,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
    }
}

fn job_is_stopped(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn) -> bool {
    unsafe {
        host_cb(
            ctx,
            2,
            0,
            0,
            0,
            0,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        ) != 0
    }
}

fn run_move_job(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn) {
    let (nzb_name, inter_dir, dest_dir) = with_dq_guard(ctx, host_cb, |_| {
        let mut strs = [ptr::null() as *const c_char; 3];
        let mut built_final = Vec::new();
        extern "C" fn set_vec(vec_ptr: usize, s: *const c_char) {
            let v = unsafe { &mut *(vec_ptr as *mut Vec<u8>) };
            *v = unsafe { ptr_bytes(s) };
        }
        unsafe {
            host_cb(
                ctx,
                1,
                0,
                0,
                (&mut built_final as *mut Vec<u8>) as usize,
                set_vec as *const () as usize,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                strs.as_mut_ptr(),
            );
        }
        let name = trunc_1023(unsafe { ptr_bytes(strs[0]) });
        let inter = unsafe { ptr_bytes(strs[1]) };
        let fin = unsafe { ptr_bytes(strs[2]) };
        let dst = if fin.is_empty() { built_final } else { fin };
        (name, inter, dst)
    });

    emit_job_msg(
        ctx,
        host_cb,
        MK_INFO,
        &[b"Moving completed files for ", nzb_name.as_slice()].concat(),
    );

    let inter_path = crate::cleanup::bytes_to_path_buf(&inter_dir);
    let dest_path = crate::cleanup::bytes_to_path_buf(&dest_dir);
    let ok = crate::cleanup::move_files(
        &inter_path,
        &dest_path,
        || job_is_stopped(ctx, host_cb),
        |kind, msg| emit_job_msg(ctx, host_cb, map_cleanup_kind(kind), msg),
        |old_rel, new_rel| {
            let c_old = to_c_buf(old_rel);
            let c_new = to_c_buf(new_rel);
            with_dq_guard(ctx, host_cb, |_| unsafe {
                host_cb(
                    ctx,
                    2,
                    2,
                    0,
                    0,
                    0,
                    c_old.as_ptr() as *const c_char,
                    c_new.as_ptr() as *const c_char,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            });
        },
    );

    let up_info_name = trunc_1023([b"Move for ", nzb_name.as_slice()].concat());

    if ok {
        let mut s = [ptr::null() as *const c_char; 1];
        unsafe {
            host_cb(
                ctx,
                1,
                1,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                s.as_mut_ptr(),
            );
        }
        let hl_bytes = unsafe { ptr_bytes(s[0]) };
        if !hl_bytes.is_empty() {
            let hl_path = crate::cleanup::bytes_to_path_buf(&hl_bytes);
            crate::cleanup::remove_stale_hardlinks(&hl_path, &dest_path, |kind, msg| {
                emit_job_msg(ctx, host_cb, map_cleanup_kind(kind), msg);
            });
        }
    }

    with_dq_guard(ctx, host_cb, |_| {
        let has_nzb = unsafe {
            host_cb(
                ctx,
                1,
                2,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            ) != 0
        };
        if has_nzb {
            let (outcome, move_status) =
                crate::cleanup::decide_move_outcome(ok, job_is_stopped(ctx, host_cb));
            if outcome == 1 {
                emit_job_msg(
                    ctx,
                    host_cb,
                    MK_INFO,
                    &[up_info_name.as_slice(), b" successful"].concat(),
                );
                let c_dst = to_c_buf(&dest_dir);
                unsafe {
                    host_cb(
                        ctx,
                        3,
                        1,
                        move_status,
                        0,
                        0,
                        c_dst.as_ptr() as *const c_char,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            } else if outcome == 2 {
                emit_job_msg(
                    ctx,
                    host_cb,
                    MK_INFO,
                    &[
                        up_info_name.as_slice(),
                        b" interrupted, resumes after the restart",
                    ]
                    .concat(),
                );
            } else {
                emit_job_msg(
                    ctx,
                    host_cb,
                    MK_ERROR,
                    &[up_info_name.as_slice(), b" failed"].concat(),
                );
                unsafe {
                    host_cb(
                        ctx,
                        3,
                        0,
                        move_status,
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
            unsafe {
                host_cb(
                    ctx,
                    3,
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
    });
}

fn run_cleanup_job(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn) {
    let (nzb_name, dest_dir, final_dir, ext_cleanup_disk) = with_dq_guard(ctx, host_cb, |_| {
        let mut strs = [ptr::null() as *const c_char; 4];
        unsafe {
            host_cb(
                ctx,
                1,
                0,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                strs.as_mut_ptr(),
            );
        }
        (
            trunc_1023(unsafe { ptr_bytes(strs[0]) }),
            unsafe { ptr_bytes(strs[1]) },
            unsafe { ptr_bytes(strs[2]) },
            unsafe { ptr_bytes(strs[3]) },
        )
    });

    emit_job_msg(
        ctx,
        host_cb,
        MK_INFO,
        &[b"Cleaning up ", nzb_name.as_slice()].concat(),
    );

    let dst_path = crate::cleanup::bytes_to_path_buf(&dest_dir);
    let fin_path = crate::cleanup::bytes_to_path_buf(&final_dir);
    let (outcome, cleanup_status) =
        crate::cleanup::run_cleanup(&dst_path, &fin_path, &ext_cleanup_disk, |kind, msg| {
            emit_job_msg(ctx, host_cb, map_cleanup_kind(kind), msg);
        });

    let up_info_name = trunc_1023([b"Cleanup for ", nzb_name.as_slice()].concat());
    if outcome == 1 {
        emit_job_msg(
            ctx,
            host_cb,
            MK_INFO,
            &[up_info_name.as_slice(), b" successful"].concat(),
        );
    } else if outcome == 2 {
        emit_job_msg(
            ctx,
            host_cb,
            MK_INFO,
            &[b"Nothing to cleanup for ", nzb_name.as_slice()].concat(),
        );
    } else {
        emit_job_msg(
            ctx,
            host_cb,
            MK_ERROR,
            &[up_info_name.as_slice(), b" failed"].concat(),
        );
    }

    unsafe {
        host_cb(
            ctx,
            3,
            0,
            cleanup_status,
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

struct RenameRarCallbacks {
    ctx: *mut c_void,
    host_cb: NzbgetRsRepairCtrlHostFn,
    renamed_count: i32,
}

impl crate::rarrenamer::RunCallbacks for RenameRarCallbacks {
    fn stopped(&mut self) -> Result<bool, ()> {
        Ok(job_is_stopped(self.ctx, self.host_cb))
    }

    fn progress(&mut self, label: &[u8], stage_progress: i32) -> Result<(), ()> {
        let c_lbl = to_c_buf(label);
        with_dq_guard(self.ctx, self.host_cb, |_| unsafe {
            (self.host_cb)(
                self.ctx,
                2,
                3,
                stage_progress,
                0,
                0,
                c_lbl.as_ptr() as *const c_char,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
        Ok(())
    }

    fn message(&mut self, kind: i32, text: &[u8]) -> Result<(), ()> {
        let mk = match kind {
            0 => MK_INFO,
            1 => MK_WARNING,
            _ => MK_ERROR,
        };
        emit_job_msg(self.ctx, self.host_cb, mk, text);
        Ok(())
    }

    fn register_renamed(&mut self, old_base: &[u8], new_base: &[u8]) -> Result<(), ()> {
        register_renamed_helper(
            self.ctx,
            self.host_cb,
            old_base,
            new_base,
            &mut self.renamed_count,
        );
        Ok(())
    }
}

fn register_renamed_helper(
    ctx: *mut c_void,
    host_cb: NzbgetRsRepairCtrlHostFn,
    old_fn: &[u8],
    new_fn: &[u8],
    renamed_count: &mut i32,
) {
    let c_old = to_c_buf(old_fn);
    let c_new = to_c_buf(new_fn);
    let found = with_dq_guard(ctx, host_cb, |_| unsafe {
        host_cb(
            ctx,
            2,
            2,
            0,
            0,
            0,
            c_old.as_ptr() as *const c_char,
            c_new.as_ptr() as *const c_char,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        ) != 0
    });
    if !found {
        emit_job_msg(
            ctx,
            host_cb,
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

fn run_rename_job(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn, kind: i32, flags: i32) {
    let parcheck_enabled = (flags & 1) != 0;
    let tls_enabled = (flags & 2) != 0;

    let (nzb_name, dest_dir, final_dir, password, par_ignore_ext, unpack_ignore_ext, unpack_status) =
        with_dq_guard(ctx, host_cb, |_| {
            let mut ints = [0 as c_int; 2];
            let mut strs = [ptr::null() as *const c_char; 6];
            unsafe {
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
                    ptr::null_mut(),
                    strs.as_mut_ptr(),
                );
            }
            (
                trunc_1023(unsafe { ptr_bytes(strs[0]) }),
                unsafe { ptr_bytes(strs[1]) },
                unsafe { ptr_bytes(strs[2]) },
                if ints[0] != 0 {
                    unsafe { ptr_bytes(strs[3]) }
                } else {
                    Vec::new()
                },
                unsafe { ptr_bytes(strs[4]) },
                unsafe { ptr_bytes(strs[5]) },
                ints[1],
            )
        });

    let archive_prefix: &[u8] = if kind == 1 { b"archive " } else { b"" };
    emit_job_msg(
        ctx,
        host_cb,
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
                crate::cleanup::rename_eval_par_config(unpack_status, 3, 0, final_dir.is_empty());
            let target_dir = if use_final { &final_dir } else { &dest_dir };
            let mut stop_fn = || job_is_stopped(ctx, host_cb);
            let mut prog_fn = |label: &[u8], stage: i32| {
                let c_lbl = to_c_buf(label);
                with_dq_guard(ctx, host_cb, |_| unsafe {
                    host_cb(
                        ctx,
                        2,
                        3,
                        stage,
                        0,
                        0,
                        c_lbl.as_ptr() as *const c_char,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                });
            };
            let mut msg_fn = |mk: crate::parrenamer::Kind, text: &[u8]| {
                let m = match mk as i32 {
                    0 => MK_INFO,
                    1 => MK_WARNING,
                    _ => MK_ERROR,
                };
                emit_job_msg(ctx, host_cb, m, text);
            };
            let mut parred_fn = |name: &[u8]| {
                let c_name = to_c_buf(name);
                unsafe {
                    host_cb(
                        ctx,
                        2,
                        4,
                        0,
                        0,
                        0,
                        c_name.as_ptr() as *const c_char,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            };
            let mut ren_fn = |old_rel: &[u8], new_rel: &[u8]| {
                register_renamed_helper(ctx, host_cb, old_rel, new_rel, &mut renamed_count);
            };

            let state = crate::parrenamer::execute_all(
                &nzb_name,
                target_dir,
                &par_ignore_ext,
                detect_missing,
                crate::paths::PATH_SEPARATOR,
                &mut stop_fn,
                &mut prog_fn,
                &mut msg_fn,
                &mut parred_fn,
                &mut ren_fn,
            );
            has_missed_files = state.has_missed_files;
            has_damaged_par_files = state.has_damaged_par_files;
        }
    } else if kind == 1 {
        let mut cbs = RenameRarCallbacks {
            ctx,
            host_cb,
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
        crate::cleanup::rename_classify_run_outcome(job_is_stopped(ctx, host_cb), renamed_count);
    if run_outcome == 1 {
        emit_job_msg(
            ctx,
            host_cb,
            MK_WARNING,
            &[b"Renaming cancelled for ", nzb_name.as_slice()].concat(),
        );
    } else if run_outcome == 2 {
        emit_job_msg(
            ctx,
            host_cb,
            MK_INFO,
            &[
                format!("Successfully renamed {renamed_count} ").as_bytes(),
                archive_prefix,
                b"file(s) for ",
                nzb_name.as_slice(),
            ]
            .concat(),
        );
    } else {
        emit_job_msg(
            ctx,
            host_cb,
            MK_INFO,
            &[
                b"No renamed ",
                archive_prefix,
                b"files found for ",
                nzb_name.as_slice(),
            ]
            .concat(),
        );
    }

    with_dq_guard(ctx, host_cb, |dq_ptr| {
        let mut ints = [0 as c_int; 2];
        unsafe {
            host_cb(
                ctx,
                1,
                1,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ints.as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
        let rem_par_count = ints[0];
        let par_status = ints[1];
        let (initial_status, req_extra, req_par_check) = crate::cleanup::rename_complete_action(
            kind,
            renamed_count,
            has_damaged_par_files,
            rem_par_count,
            has_missed_files,
            par_status,
        );

        let mut final_status = initial_status;
        if kind == 0 && parcheck_enabled && req_extra {
            emit_job_msg(
                ctx,
                host_cb,
                MK_INFO,
                &[
                    b"Requesting extra par2-files for ",
                    nzb_name.as_slice(),
                    b" to perform par-rename",
                ]
                .concat(),
            );
            let mut rem_sz = 0i64;
            unsafe {
                host_cb(
                    ctx,
                    3,
                    1,
                    initial_status,
                    dq_ptr,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    &mut rem_sz,
                    ptr::null_mut(),
                );
            }
            if crate::cleanup::rename_should_reset_par_status(rem_sz) {
                final_status = 0; // rsNone
            }
        }

        let do_req_par = parcheck_enabled && req_par_check;
        if do_req_par {
            emit_job_msg(
                ctx,
                host_cb,
                MK_INFO,
                &[
                    b"Requesting par-check/repair for ",
                    nzb_name.as_slice(),
                    b" to restore missing files ",
                ]
                .concat(),
            );
        }

        unsafe {
            host_cb(
                ctx,
                3,
                0,
                kind,
                final_status as usize,
                usize::from(do_req_par),
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    });
}

fn run_post_renamer_job(ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn, is_post_unpack: bool) {
    let (meta_name, nzb_name, dst_dir, ignore_ext) = with_dq_guard(ctx, host_cb, |_| {
        let mut strs = [ptr::null() as *const c_char; 4];
        unsafe {
            host_cb(
                ctx,
                1,
                0,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                strs.as_mut_ptr(),
            );
        }
        (
            unsafe { ptr_bytes(strs[0]) },
            unsafe { ptr_bytes(strs[1]) },
            unsafe { ptr_bytes(strs[2]) },
            unsafe { ptr_bytes(strs[3]) },
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
        || job_is_stopped(ctx, host_cb),
        |kind, msg| emit_job_msg(ctx, host_cb, map_cleanup_kind(kind), msg),
        |old_name, new_name| {
            let c_old = to_c_buf(old_name);
            let c_new = to_c_buf(new_name);
            with_dq_guard(ctx, host_cb, |_| unsafe {
                host_cb(
                    ctx,
                    2,
                    2,
                    0,
                    0,
                    0,
                    c_old.as_ptr() as *const c_char,
                    c_new.as_ptr() as *const c_char,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            });
        },
    );

    with_dq_guard(ctx, host_cb, |_| unsafe {
        host_cb(
            ctx,
            3,
            c_int::from(is_post_unpack),
            status,
            0,
            0,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
    });
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn post_script_add_message_inner(
    ctx: *mut c_void,
    host_cb: NzbgetRsRepairCtrlHostFn,
    script_host_cb: crate::script::NzbgetRsScriptHostFn,
    prefix_len: usize,
    info_name: &[u8],
    nzb_name: &[u8],
    kind: i32,
    text: &[u8],
) {
    let msg_text = &text[prefix_len.min(text.len())..];
    let cmd = crate::extension::parse_post_script_cmd(msg_text);
    if cmd.kind != 0 {
        if msg_text.len() >= 6 {
            let dbg = [b"Command ", &msg_text[6..], b" detected"].concat();
            crate::script::host_log(ptr::null_mut(), script_host_cb, MK_DEBUG, &dbg);
        }
        match cmd.kind {
            1 => {
                let c_val = to_c_buf(&msg_text[cmd.val_offset.min(msg_text.len())..]);
                with_dq_guard(ctx, host_cb, |_| unsafe {
                    host_cb(
                        ctx,
                        5,
                        2,
                        1,
                        0,
                        0,
                        c_val.as_ptr() as *const c_char,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                });
            }
            2 => {
                let c_val = to_c_buf(&msg_text[cmd.val_offset.min(msg_text.len())..]);
                with_dq_guard(ctx, host_cb, |_| unsafe {
                    host_cb(
                        ctx,
                        5,
                        2,
                        2,
                        0,
                        0,
                        c_val.as_ptr() as *const c_char,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                });
            }
            3 => {
                let k_end = (cmd.key_offset + cmd.key_len).min(msg_text.len());
                let c_key = to_c_buf(&msg_text[cmd.key_offset.min(k_end)..k_end]);
                let c_val = to_c_buf(&msg_text[cmd.val_offset.min(msg_text.len())..]);
                with_dq_guard(ctx, host_cb, |_| unsafe {
                    host_cb(
                        ctx,
                        5,
                        2,
                        3,
                        0,
                        0,
                        c_key.as_ptr() as *const c_char,
                        c_val.as_ptr() as *const c_char,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                });
            }
            4 => {
                let mark_msg = [b"Marking ", nzb_name, b" as bad"].concat();
                post_script_add_message_inner(
                    ctx,
                    host_cb,
                    script_host_cb,
                    0,
                    info_name,
                    nzb_name,
                    MK_WARNING,
                    &mark_msg,
                );
                with_dq_guard(ctx, host_cb, |_| unsafe {
                    host_cb(
                        ctx,
                        5,
                        4,
                        1,
                        0,
                        0,
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                });
            }
            _ => {
                let err_msg = [
                    b"Invalid command \"",
                    msg_text,
                    b"\" received from ",
                    info_name,
                ]
                .concat();
                let c_err = to_c_buf(&err_msg);
                unsafe {
                    host_cb(
                        ctx,
                        5,
                        3,
                        MK_ERROR,
                        0,
                        0,
                        c_err.as_ptr() as *const c_char,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            }
        }
    } else {
        let c_txt = to_c_buf(text);
        unsafe {
            host_cb(
                ctx,
                5,
                3,
                kind,
                0,
                0,
                c_txt.as_ptr() as *const c_char,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
        with_dq_guard(ctx, host_cb, |_| unsafe {
            host_cb(
                ctx,
                5,
                4,
                0,
                0,
                0,
                c_txt.as_ptr() as *const c_char,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
    }

    let mut pause_i64s = [0i64; 2];
    let is_paused = unsafe {
        host_cb(
            ctx,
            5,
            5,
            0,
            0,
            0,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            pause_i64s.as_mut_ptr(),
            ptr::null_mut(),
        ) != 0
    };
    if is_paused {
        let stage_time = pause_i64s[0];
        let start_time = pause_i64s[1];
        let wait_time = current_time_sec();

        loop {
            let still_paused = unsafe {
                host_cb(
                    ctx,
                    5,
                    5,
                    0,
                    0,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                ) != 0
            };
            if !still_paused || job_is_stopped(ctx, host_cb) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            let delta = current_time_sec() - wait_time;
            let mut upd = [
                if stage_time > 0 {
                    stage_time + delta
                } else {
                    0
                },
                if start_time > 0 {
                    start_time + delta
                } else {
                    0
                },
            ];
            unsafe {
                host_cb(
                    ctx,
                    5,
                    6,
                    0,
                    0,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    upd.as_mut_ptr(),
                    ptr::null_mut(),
                );
            }
        }
    }
}

#[cfg(unix)]
pub unsafe fn post_script_run(
    runner_ptr: *mut crate::script::NzbgetRsScriptRunner,
    ext_mgr_ptr: *const crate::extension::RsExtensionManager,
    parcheck_enabled: bool,
    ctx: *mut c_void,
    host_cb: Option<NzbgetRsRepairCtrlHostFn>,
    script_host_cb: Option<crate::script::NzbgetRsScriptHostFn>,
) {
    let (Some(cb), Some(shcb)) = (host_cb, script_host_cb) else {
        return;
    };

    let script_comma_list = with_dq_guard(ctx, cb, |_| {
        let param_count = unsafe {
            cb(
                ctx,
                1,
                0,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            ) as usize
        };
        let mut list = Vec::new();
        for idx in 0..param_count {
            let mut strs = [ptr::null() as *const c_char; 2];
            unsafe {
                cb(
                    ctx,
                    1,
                    1,
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
            let pname = unsafe { ptr_bytes(strs[0]) };
            let pval = unsafe { ptr_bytes(strs[1]) };
            if !pname.is_empty()
                && pname[0] != b'*'
                && pname[pname.len() - 1] == b':'
                && (pval.eq_ignore_ascii_case(b"yes")
                    || pval.eq_ignore_ascii_case(b"on")
                    || pval == b"1")
            {
                list.extend_from_slice(&pname[..pname.len() - 1]);
                list.push(b',');
            }
        }
        list
    });

    if let (Some(runner), Some(ext_mgr)) = (runner_ptr.as_ref(), ext_mgr_ptr.as_ref()) {
        let mut cfg = crate::script::ScriptExecEnv::new(ptr::null_mut(), shcb);

        ext_mgr.execute_script_list(&script_comma_list, |ext| {
            if !ext.post_script {
                return;
            }
            let mut ints = [0 as c_int; 1];
            let mut strs = [ptr::null() as *const c_char; 1];
            unsafe {
                cb(
                    ctx,
                    4,
                    0,
                    0,
                    0,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ints.as_mut_ptr(),
                    ptr::null_mut(),
                    strs.as_mut_ptr(),
                );
            }
            if ints[0] != 0 {
                return;
            }
            let nzb_name = unsafe { ptr_bytes(strs[0]) };
            let info_name = trunc_1023(
                [
                    b"post-process-script ",
                    ext.name.as_slice(),
                    b" for ",
                    nzb_name.as_slice(),
                ]
                .concat(),
            );

            let exec_msg = [
                b"Executing post-process-script ",
                ext.name.as_slice(),
                b" for ",
                nzb_name.as_slice(),
            ]
            .concat();
            post_script_add_message_inner(
                ctx, cb, shcb, 0, &info_name, &nzb_name, MK_INFO, &exec_msg,
            );

            let prog_lbl =
                trunc_1023([b"Executing post-process-script ", ext.name.as_slice()].concat());
            let c_prog = to_c_buf(&prog_lbl);
            with_dq_guard(ctx, cb, |_| unsafe {
                cb(
                    ctx,
                    4,
                    1,
                    0,
                    0,
                    0,
                    c_prog.as_ptr() as *const c_char,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            });

            runner.inner.set_info_name(&info_name);
            cfg.args = vec![ext.entry.clone()];
            cfg.log_prefix = Some(ext.display_name.clone());
            let prefix_len = ext.display_name.len() + 2;

            with_dq_guard(ctx, cb, |_| {
                cfg.reset_env(ptr::null_mut(), shcb);
                let mut env_ints = [0 as c_int; 14];
                let mut env_strs = [ptr::null() as *const c_char; 8];
                let mut status_buf = Vec::new();
                extern "C" fn set_vec(vec_ptr: usize, s: *const c_char) {
                    let v = unsafe { &mut *(vec_ptr as *mut Vec<u8>) };
                    *v = unsafe { ptr_bytes(s) };
                }
                unsafe {
                    cb(
                        ctx,
                        4,
                        3,
                        0,
                        (&mut status_buf as *mut Vec<u8>) as usize,
                        set_vec as *const () as usize,
                        ptr::null(),
                        ptr::null(),
                        env_ints.as_mut_ptr(),
                        ptr::null_mut(),
                        env_strs.as_mut_ptr(),
                    );
                }
                let srv_count = env_ints[12].max(0) as usize;
                let mut srv_ids = Vec::with_capacity(srv_count);
                let mut srv_succ = Vec::with_capacity(srv_count);
                let mut srv_fail = Vec::with_capacity(srv_count);
                for i in 0..srv_count {
                    let mut si = [0 as c_int; 3];
                    unsafe {
                        cb(
                            ctx,
                            4,
                            4,
                            i as c_int,
                            0,
                            0,
                            ptr::null(),
                            ptr::null(),
                            si.as_mut_ptr(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                    }
                    srv_ids.push(si[0]);
                    srv_succ.push(si[1]);
                    srv_fail.push(si[2]);
                }

                let param_count = env_ints[13].max(0) as usize;
                let mut params = Vec::with_capacity(param_count);
                for idx in 0..param_count {
                    let mut pstrs = [ptr::null() as *const c_char; 2];
                    unsafe {
                        cb(
                            ctx,
                            1,
                            1,
                            idx as c_int,
                            0,
                            0,
                            ptr::null(),
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            pstrs.as_mut_ptr(),
                        );
                    }
                    params.push((unsafe { ptr_bytes(pstrs[0]) }, unsafe {
                        ptr_bytes(pstrs[1])
                    }));
                }

                let c_status = to_c_buf(&status_buf);
                let cstr_or_empty = |p: *const c_char| -> &CStr {
                    if p.is_null() {
                        c""
                    } else {
                        unsafe { CStr::from_ptr(p) }
                    }
                };
                let status_cstr = CStr::from_bytes_with_nul(&c_status).unwrap_or(c"");
                let inp = crate::extension::PostScriptEnvInput {
                    nzb_id: env_ints[0],
                    nzb_name: cstr_or_empty(env_strs[0]),
                    dest_dir: cstr_or_empty(env_strs[1]),
                    filename: cstr_or_empty(env_strs[2]),
                    queued_filename: cstr_or_empty(env_strs[3]),
                    url: cstr_or_empty(env_strs[4]),
                    final_dir: cstr_or_empty(env_strs[5]),
                    category: cstr_or_empty(env_strs[6]),
                    health: env_ints[1],
                    critical_health: env_ints[2],
                    dupe_key: cstr_or_empty(env_strs[7]),
                    dupe_score: env_ints[3],
                    dupe_mode: env_ints[4],
                    status: status_cstr,
                    script_status: env_ints[5],
                    par_status: env_ints[6],
                    unpack_status: env_ints[7],
                    delete_status: env_ints[8],
                    mark_status: env_ints[9],
                    total_articles: env_ints[10],
                    success_articles: env_ints[11],
                    failed_articles: unsafe {
                        cb(
                            ctx,
                            4,
                            5,
                            0,
                            0,
                            0,
                            ptr::null(),
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        ) as c_int
                    },
                    server_ids: &srv_ids,
                    server_success_articles: &srv_succ,
                    server_failed_articles: &srv_fail,
                };

                let cfg_cell = std::cell::RefCell::new(&mut cfg);
                crate::extension::post_script_prepare_env(
                    &inp,
                    |name, val| {
                        cfg_cell
                            .borrow_mut()
                            .set_env_var(name.to_bytes(), val.to_bytes());
                    },
                    |name, val| {
                        cfg_cell.borrow_mut().set_int_env_var(name.to_bytes(), val);
                    },
                );
                cfg.prepare_env_script(Some(&params), &ext.name, ptr::null_mut(), shcb);
            });

            let exit_code =
                runner
                    .inner
                    .execute(&mut cfg, ptr::null_mut(), shcb, &mut |kind, text| {
                        let cur_info = runner.inner.get_info_name();
                        post_script_add_message_inner(
                            ctx, cb, shcb, prefix_len, &cur_info, &nzb_name, kind, text,
                        );
                    });
            cfg.log_prefix = None;

            let par_status = unsafe {
                cb(
                    ctx,
                    4,
                    9,
                    0,
                    0,
                    0,
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                ) as c_int
            };

            let mut up_info_name = info_name.clone();
            if let Some(first) = up_info_name.first_mut() {
                *first = b'P';
            }

            let (st, action, req_par) = crate::extension::analyse_post_script_exit_code(
                exit_code,
                par_status,
                1,
                parcheck_enabled,
                0,
                1,
                2,
            );
            let (res_kind, res_msg) = match action {
                1 => (MK_INFO, [up_info_name.as_slice(), b" successful"].concat()),
                2 => (MK_ERROR, [up_info_name.as_slice(), b" failed"].concat()),
                3 => (MK_INFO, [up_info_name.as_slice(), b" skipped"].concat()),
                4 => (
                    MK_ERROR,
                    [
                        up_info_name.as_slice(),
                        b" requested par-check/repair, but the collection was already checked",
                    ]
                    .concat(),
                ),
                5 => (
                    MK_INFO,
                    [up_info_name.as_slice(), b" requested par-check/repair"].concat(),
                ),
                _ => (
                    MK_ERROR,
                    [
                        up_info_name.as_slice(),
                        b" failed (terminated with unknown status)",
                    ]
                    .concat(),
                ),
            };
            post_script_add_message_inner(
                ctx, cb, shcb, 0, &info_name, &nzb_name, res_kind, &res_msg,
            );

            let c_sname = to_c_buf(&ext.name);
            with_dq_guard(ctx, cb, |_| unsafe {
                cb(
                    ctx,
                    4,
                    10,
                    st,
                    usize::from(req_par),
                    0,
                    c_sname.as_ptr() as *const c_char,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            });
        });
    }

    unsafe {
        cb(
            ctx,
            1,
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

pub unsafe fn post_job_exec(
    job: c_int,
    a1: c_int,
    a2: c_int,
    host_cb: Option<NzbgetRsRepairCtrlHostFn>,
    ctx: *mut c_void,
) -> c_int {
    let Some(cb) = host_cb else {
        return 0;
    };
    match job {
        0 => run_move_job(ctx, cb),
        1 => run_cleanup_job(ctx, cb),
        2 => run_rename_job(ctx, cb, a1, a2),
        3 => run_post_renamer_job(ctx, cb, a1 != 0),
        _ => {}
    }
    0
}
