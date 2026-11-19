//! End-to-end Rust orchestration for post-processing controllers:
//! `ParChecker`, `UnpackController`, `DirectUnpack`, `MoveController`,
//! `CleanupController`, `RenameController`, and `PostRenamerController`.

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
            has_memory_monitor: ints[1] != 0 || crate::articledl::memory_monitor_active(),
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
        // Also behind a log prefix ("Join: Unpack limit: ...").
        if kind == MK_ERROR
            && text
                .windows(crate::unpacklimits::LIMIT_PREFIX.len())
                .any(|w| w == crate::unpacklimits::LIMIT_PREFIX.as_bytes())
        {
            st.unpack_limit_error = true;
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
        st.unpack_limit_error = false;
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
        let guard = st.unpack_guard.clone();
        let _scope = guard.as_ref().map(crate::unpacklimits::Scope::enter);

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
        // A joined file named like an archive volume is output: count it now.
        if ok {
            if let Some(g) = &guard {
                if let Err(why) = crate::unpacklimits::settle(g) {
                    self.print_message(ctx, host_cb, st, MK_ERROR, why.as_bytes());
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

        // Every file the job unpacked is counted (archive volumes held back included), then
        // names are made ASCII: once every unpack pass of the job is done.
        if st.unpack_ok {
            if let Some(guard) = st.unpack_guard.clone() {
                if let Err(why) = crate::unpacklimits::settle_job(&guard) {
                    self.print_message(ctx, host_cb, st, MK_ERROR, why.as_bytes());
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
                    self.print_message(ctx, host_cb, st, kind, m.as_bytes());
                }
                if named.is_err() {
                    // A file system error: a par repair cannot help.
                    st.unpack_ok = false;
                    st.unpack_start_error = true;
                }
            }
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
                st.unpack_start_error || st.unpack_limit_error,
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
            .is_some_and(|st| !st.virtuals.is_empty())
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
    direct_unpack_holds()
        .get(&nzb_id)
        .is_some_and(|st| !st.virtuals.is_empty())
}

/// Forgets what was found of the donors of download `nzb_id` and whether the queue kept it for
/// them: it left the queue, or came back to it from history to start over.
pub(crate) fn forget_dead_volume_verdicts(nzb_id: i32) {
    dead_volume_donor_identities().retain(|(id, _), _| *id != nzb_id);
    crate::queuecoord::forget_kept_for_borrowing(nzb_id);
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
            .is_some_and(|st| !st.virtuals.is_empty())
}

/// How many dead volumes of download `nzb_id` were rebuilt from a duplicate (0 if none), once:
/// the count is forgotten as it is read.
pub(crate) fn take_dead_volumes_borrowed(nzb_id: i32) -> usize {
    dead_volumes_borrowed().remove(&nzb_id).unwrap_or(0)
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
    let mut map = direct_unpack_holds();
    for st in map.values_mut() {
        if let Some(i) = st.held.iter().position(|p| p == full_path) {
            let path = st.held.remove(i);
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
    if state.held.is_empty() || state.abandoned || state.live_repair_running {
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
        || (!nzb.post_working && nzb.post_stage == crate::queuesubsys_host::PT_QUEUED)
}

/// Post-processing waits (boundedly) while live work for `nzb_id` is still
/// pending: completed files not yet folded into the live session, or a live
/// repair helper that is running.
#[cfg(unix)]
pub(crate) fn live_repair_should_defer_post(nzb_id: i32) -> bool {
    static SINCE: std::sync::LazyLock<Mutex<std::collections::HashMap<i32, std::time::Instant>>> =
        std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));
    const LIMIT: Duration = Duration::from_secs(10);
    let running = direct_unpack_holds()
        .get(&nzb_id)
        .is_some_and(|state| state.live_repair_running);
    let pending = running
        || crate::parchecker::streaming_session_pending_completed_jobs().contains(&nzb_id)
        || crate::parchecker::streaming_session_completion_drain_active(nzb_id);
    let mut since = SINCE.lock().unwrap_or_else(|e| e.into_inner());
    if !pending {
        since.remove(&nzb_id);
        return false;
    }
    let first = *since.entry(nzb_id).or_insert_with(std::time::Instant::now);
    if first.elapsed() < LIMIT {
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

#[cfg(unix)]
pub(crate) fn direct_unpack_try_live_par_repair(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    nzb: &crate::queuesubsys_host::NzbSnap,
) {
    let nzb_id = nzb.id;
    if host.self_is_stopped()
        || nzb.deleting
        || nzb.delete_status != 0
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
        let previous = LIVE_PAR_HELPERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(nzb_id, handle);
        if let Some(previous) = previous {
            let _ = previous.join();
        }
    }
}

/// Helper threads of running live par2 repairs, per download.
#[cfg(unix)]
static LIVE_PAR_HELPERS: std::sync::LazyLock<
    Mutex<std::collections::HashMap<i32, std::thread::JoinHandle<()>>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Waits for the live par2 repair of download `nzb_id` to end. Called before the download's
/// files are cleaned up (deletion, shutdown) so no repair write outlives the download.
#[track_caller]
pub(crate) fn direct_unpack_join_live_par_repair(nzb_id: i32) {
    crate::parchecker::streaming_session_cancel(nzb_id);
    #[cfg(unix)]
    {
        let handle = LIVE_PAR_HELPERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&nzb_id);
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
    #[cfg(not(unix))]
    let _ = nzb_id;
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
    nzb_ptr: usize,
    keep: bool,
) {
    use crate::queuesubsys_host::{
        DS_NONE, NZB_INT_LIVE_PAR_SEC, NZB_INT_PAR_SEC, NZB_INT_REPAIR_SEC,
    };
    let nzb = host.nzb_snap(nzb_ptr, false);
    let taken = crate::parchecker::streaming_session_take_par_secs(nzb.id);
    let current = (nzb.par_sec, nzb.live_par_sec, nzb.repair_sec);
    let deleted = !keep || nzb.delete_status != DS_NONE;
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
    nzb_ptr: usize,
) {
    use crate::queuesubsys_host::{
        DS_NONE, NZB_INT_LIVE_PAR_SEC, NZB_INT_PAR_SEC, NZB_INT_REPAIR_SEC,
    };
    let nzb = host.nzb_snap(nzb_ptr, false);
    if nzb.delete_status != DS_NONE {
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

pub(crate) fn direct_unpack_forget_holds(nzb_id: i32) {
    note_dead_volume_borrow(nzb_id, false);
    forget_dead_volume_verdicts(nzb_id);
    let gone = direct_unpack_holds().remove(&nzb_id);
    // Borrowed data of rebuilt dead volumes is only read by direct unpack.
    for v in gone.iter().flat_map(|st| st.virtuals.values()) {
        if crate::deadvolume::is_scratch(&v.data) {
            let _ = std::fs::remove_file(&v.data);
        }
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
        let active = self.unpacking.load(Ordering::Acquire)
            && !self.waiting_volume.load(Ordering::Acquire);
        busy.set_active(active, std::time::Instant::now());
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
        // The unpacking done so far, while downloading and after, and the part while
        // downloading (split when the download completed); reported once.
        let (busy_sec, live_sec) = self
            .busy
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take_secs(std::time::Instant::now());
        let mut unpack_sec = [busy_sec, live_sec];
        if start != 0 || clear_working || unpack_sec[0] != 0 {
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
                    unpack_sec.as_mut_ptr(),
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

    /// Picks up volumes released by a repair: a released volume the extractor is waiting for
    /// ends the wait, a released first volume is queued for extraction. A failed repair stops
    /// the thread.
    fn apply_releases(&self, ctx: *mut c_void, host_cb: NzbgetRsRepairCtrlHostFn) {
        let (released, abandoned) = direct_unpack_take_released(self.nzb_id);
        if abandoned && !self.cancelled.swap(true, Ordering::AcqRel) {
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
            return;
        }
        if released.is_empty() {
            return;
        }
        let mut v = self.lock_vol();
        for filename in released {
            Self::global_log(
                ctx,
                host_cb,
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
            // A damaged volume pauses extraction until it is repaired; nothing is cancelled.
            let dest_dir = if nzb_ptr != 0 {
                crate::ffi::queue_host().map(|host| host.nzb_snap(nzb_ptr, false).dest_dir)
            } else {
                None
            };
            let Some(dest_dir) = dest_dir else {
                self.stop_with_nzb(ctx, host_cb, nzb_ptr);
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
            let c_msg = to_c_buf(&msg);
            unsafe {
                host_cb(
                    ctx,
                    1,
                    3,
                    MK_INFO,
                    nzb_ptr,
                    0,
                    c_msg.as_ptr() as *const c_char,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
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
                let c_msg = to_c_buf(&msg);
                unsafe {
                    host_cb(
                        ctx,
                        1,
                        3,
                        MK_INFO,
                        nzb_ptr,
                        0,
                        c_msg.as_ptr() as *const c_char,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
                self.add_extra_time(ctx, host_cb, nzb_ptr, true);
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
        self.busy
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .mark_download_done(std::time::Instant::now());
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

    /// Ends direct unpack after a typed extraction failure (wrong or missing password, damage
    /// that could not be repaired); post-processing takes over as before.
    fn stop_for_failure(
        &self,
        ctx: *mut c_void,
        host_cb: NzbgetRsRepairCtrlHostFn,
        failure: &crate::progressive_unpack::Failure,
    ) {
        Self::global_log(
            ctx,
            host_cb,
            MK_DEBUG,
            format!("Direct unpack stopped: {failure}").as_bytes(),
        );
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
            if !Self::is_stopped(ctx, host_cb) {
                self.stop_with_nzb(ctx, host_cb, nzb_ptr);
            }
        });
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
                self.add_message(ctx, host_cb, msg_kind, msg.as_bytes(), local);
                // A volume wait signalled through a log line holds the extractor here; RAR sets
                // read by the progressive session wait in the session and never block here.
                while self.waiting_volume.load(Ordering::Acquire)
                    && !self.is_stopped_or_terminated(ctx, host_cb)
                {
                    std::thread::sleep(Duration::from_millis(20));
                    self.apply_releases(ctx, host_cb);
                }
            },
            &mut || {
                self.apply_releases(ctx, host_cb);
                self.is_stopped_or_terminated(ctx, host_cb)
            },
            crate::unpack::RarFeed {
                feed: Some(feed),
                failure: Some(&mut failure),
                // A parked session counts as waiting, as the volume handshake did, so reloads
                // are not deferred behind it.
                on_wait: Some(&|waiting| self.set_waiting(waiting)),
                guard: Some(&guard),
            },
        );
        if let Some(f) = &failure {
            if failure_locks_archive(f, &local.password, crate::options::unpack_pass_file_set()) {
                locked_archives().insert(self.nzb_id);
            }
            if f.is_password() || f.is_damage() {
                self.stop_for_failure(ctx, host_cb, f);
            }
        }

        self.set_unpacking(false);
        self.exec_thread_key.store(0, Ordering::Release);
        self.set_waiting(false);
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
            // The set is still downloading: count every RAR file the queue
            // knows of, so the session's volume budget covers the whole set.
            if let Some(host) = crate::ffi::queue_host() {
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
                ctx,
                host_cb,
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
        while !Self::is_stopped(ctx, host_cb) {
            let next_archive = self.lock_vol().archives.pop_front();
            if let Some(archive) = next_archive {
                // A main volume extracted past its damage is queued again when its repair
                // releases it; the session already handled it.
                if started.iter().any(|a| a.eq_ignore_ascii_case(&archive)) {
                    continue;
                }
                started.push(archive.clone());
                self.set_waiting(false);
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
                if self.lock_vol().nzb_completed && !direct_unpack_has_holds(self.nzb_id) {
                    // Nothing queued, nothing on hold: one last look for a volume released
                    // right now, then done.
                    self.apply_releases(ctx, host_cb);
                    if self.lock_vol().archives.is_empty() {
                        break;
                    }
                    continue;
                }
                self.set_waiting(true);
                std::thread::sleep(Duration::from_millis(100));
                self.apply_releases(ctx, host_cb);
            }
        }

        self.set_unpacking(false);
        self.set_waiting(false);

        let cancelled = self.cancelled.load(Ordering::Acquire);
        let interrupted = !cancelled && self.is_stopped_or_terminated(ctx, host_cb);
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
                    self.add_message(ctx, host_cb, kind, m.as_bytes(), &mut local);
                }
                if named.is_err() {
                    local.unpack_ok = false;
                }
            }
        }
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

        direct_unpack_forget_holds(self.nzb_id);
        Self::global_log(
            ctx,
            host_cb,
            MK_DEBUG,
            format!("Exiting DirectUnpack-loop for {}", self.nzb_id).as_bytes(),
        );
    }

    pub fn exec_active(&self, op: c_int, p1: usize, p2: usize) -> c_int {
        match op {
            5 => return c_int::from(self.is_unpacking()),
            6 => return c_int::from(self.is_waiting_volume()),
            _ => {}
        }
        #[cfg(unix)]
        {
            let _arena = PostJobArenaGuard::enter();
            let ctx = (self as *const Self) as *mut c_void;
            match op {
                0 => {
                    self.run(ctx, direct_unpack_host_cb);
                    0
                }
                1 => {
                    let _ = p1;
                    self.stop_with_nzb(ctx, direct_unpack_host_cb, p2);
                    0
                }
                2 => {
                    let _ = p1;
                    self.file_downloaded(ctx, direct_unpack_host_cb, p2);
                    0
                }
                3 => {
                    let _ = p1;
                    self.nzb_downloaded(ctx, direct_unpack_host_cb, p2);
                    0
                }
                4 => {
                    let _ = p1;
                    self.nzb_deleted(ctx, direct_unpack_host_cb, p2);
                    0
                }
                _ => 0,
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (p1, p2);
            0
        }
    }
}

#[cfg(unix)]
unsafe extern "C" fn direct_unpack_host_cb(
    raw_ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    s2: *const c_char,
    ints: *mut c_int,
    _i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    use crate::queuesubsys_host::{
        NZB_ACT_CLEAR_UNPACK_THREAD, NZB_INT_DIRECT_UNPACK_STATUS, NZB_INT_POST_TOTAL_SEC,
        NZB_INT_LIVE_UNPACK_SEC, NZB_INT_UNPACK_SEC, POST_INT_WORKING,
    };
    if op != 0 {
        reset_call_arena();
    }
    let Some(host) = crate::ffi::queue_host() else {
        return 0;
    };
    let c = unsafe { &*(raw_ctx as *const NzbgetRsDirectUnpack) };
    match op {
        0 => {
            unsafe { invoke_dq_guard_cb(&host, p1, p2) };
            0
        }
        1 => {
            if a1 == 0 {
                let n = host.queue_find_by_id(p1, a2);
                if n == 0 {
                    return 0;
                }
                let ns = host.nzb_snap(n, false);
                let pw = host.nzb_get_param(n, b"*Unpack:Password");
                let opts = crate::options::active_options();
                let sc = opts.as_ref().map(|o| o.scalars());
                let ins = unsafe { std::slice::from_raw_parts_mut(ints, 6) };
                let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 4) };
                ins[0] = i32::from(pw.is_some());
                ins[1] = sc
                    .as_ref()
                    .map_or(0, |s| i32::from(s.use_temp_unpack_dir != 0));
                ins[2] = sc
                    .as_ref()
                    .map_or(0, |s| i32::from(s.unpack_cleanup_disk != 0));
                ins[3] = i32::from(host.nzb_unpack_thread_ptr(n) == c.thread_ptr);
                ins[4] = i32::from(ns.has_post_info);
                ins[5] = 0;
                str_out[0] = arena_cstr(&ns.name);
                str_out[1] = arena_cstr(&ns.dest_dir);
                str_out[2] = arena_opt_cstr(pw.as_deref());
                str_out[3] = arena_cstr(opts.as_ref().map_or(b"", |o| o.str_bytes(4)));
                if p2 != 0 && !s2.is_null() {
                    let fin = host.nzb_build_final_dir_name(n);
                    let c_fin = to_c_buf(&fin);
                    let cb_fn: extern "C" fn(usize, *const c_char) =
                        unsafe { std::mem::transmute(s2 as usize) };
                    cb_fn(p2, c_fin.as_ptr() as *const c_char);
                }
                return n;
            }
            if a1 == 1 {
                let fs = host.file_snap(p1);
                let nzb_name = if fs.nzb_ptr != 0 {
                    host.nzb_snap(fs.nzb_ptr, false).name
                } else {
                    Vec::new()
                };
                unsafe { *ints = fs.failed_articles };
                let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 2) };
                str_out[0] = arena_cstr(&nzb_name);
                str_out[1] = arena_cstr(&fs.filename);
                return fs.nzb_ptr;
            }
            if a1 == 2 {
                let ns = host.nzb_snap(p1, false);
                let ins = unsafe { std::slice::from_raw_parts_mut(ints, 2) };
                ins[0] = i32::from(ns.has_post_info);
                ins[1] = 0;
                let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 2) };
                str_out[0] = arena_cstr(&ns.name);
                str_out[1] = ptr::null();
                return 0;
            }
            if a1 == 3 {
                host.nzb_add_message(p1, a2, &unsafe { ptr_bytes(s1) });
                return 0;
            }
            if a1 == 4 {
                let ns = host.nzb_snap(p1, false);
                if ns.post_info_ptr != 0 {
                    host.post_set_progress_label(ns.post_info_ptr, &unsafe { ptr_bytes(s1) });
                }
                return 0;
            }
            if a1 == 5 {
                let ns = host.nzb_snap(p1, false);
                // ints[0]: seconds spent unpacking (while downloading too), ints[1]: the
                // part of it while downloading; a2: the wait after the download, which
                // post-processing spent on direct unpack.
                let (unpack_sec, live_unpack_sec) = if ints.is_null() {
                    (0, 0)
                } else {
                    unsafe { (*ints, *ints.add(1)) }
                };
                if unpack_sec != 0 {
                    host.nzb_set_int(p1, NZB_INT_UNPACK_SEC, ns.unpack_sec + unpack_sec);
                }
                if live_unpack_sec != 0 {
                    host.nzb_set_int(
                        p1,
                        NZB_INT_LIVE_UNPACK_SEC,
                        ns.live_unpack_sec + live_unpack_sec,
                    );
                }
                if (p2 & 2) != 0 {
                    host.nzb_set_int(p1, NZB_INT_POST_TOTAL_SEC, ns.post_total_sec + a2);
                }
                if (p2 & 1) != 0 && ns.post_info_ptr != 0 && !ns.has_live_repair_thread {
                    host.post_set_int(ns.post_info_ptr, POST_INT_WORKING, 0);
                }
                return 0;
            }
            if a1 == 6 {
                host.nzb_action(p1, NZB_ACT_CLEAR_UNPACK_THREAD, 0);
                host.nzb_set_int(p1, NZB_INT_DIRECT_UNPACK_STATUS, 2);
                return 0;
            }
            if a1 == 7 {
                host.nzb_action(p1, NZB_ACT_CLEAR_UNPACK_THREAD, 0);
                host.nzb_set_int(p1, NZB_INT_DIRECT_UNPACK_STATUS, a2);
                return 0;
            }
            if a1 == 8 {
                host.nzb_push_extracted_archive(p1, &unsafe { ptr_bytes(s1) });
                return 0;
            }
            0
        }
        2 => {
            if a1 == 0 {
                return usize::from(host.thread_is_stopped(c.thread_ptr));
            }
            if a1 == 1 {
                host.thread_stop(c.thread_ptr);
                return 0;
            }
            if a1 == 2 {
                return usize::from(
                    crate::ffi::active_prepost()
                        .is_some_and(|pp| host.prepost_is_stopped(pp.owner_ptr())),
                );
            }
            if a1 == 3 {
                crate::log::log_active_bytes(a2, &unsafe { ptr_bytes(s1) });
            }
            0
        }
        _ => 0,
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
            let mut renamed_to: Vec<Vec<u8>> = Vec::new();
            let mut ren_fn = |old_rel: &[u8], new_rel: &[u8]| {
                register_renamed_helper(ctx, host_cb, old_rel, new_rel, &mut renamed_count);
                renamed_to.push(new_rel.to_vec());
            };

            let state = crate::parrenamer::execute_all(
                &nzb_name,
                target_dir,
                &par_ignore_ext,
                detect_missing,
                &mut stop_fn,
                &mut prog_fn,
                &mut msg_fn,
                &mut parred_fn,
                &mut ren_fn,
            );
            has_missed_files = state.has_missed_files;
            has_damaged_par_files = state.has_damaged_par_files;
            // After an unpack, names are ASCII only (`unpacklimits`): a par rename back to a
            // non-ASCII name is renamed again.
            if unpack_status == 3 {
                for rel in renamed_to {
                    let sep = crate::paths::PATH_SEPARATOR;
                    let full = [target_dir.as_slice(), &[sep], rel.as_slice()].concat();
                    match crate::unpacklimits::ascii_rename(crate::fsops::path(&full)) {
                        Ok(Some(new)) => {
                            let new_rel = new
                                .strip_prefix(crate::fsops::path(target_dir))
                                .map(|p| crate::fsops::path_to_bytes(p).to_vec())
                                .unwrap_or_default();
                            if !new_rel.is_empty() {
                                register_renamed_helper(
                                    ctx,
                                    host_cb,
                                    &rel,
                                    &new_rel,
                                    &mut renamed_count,
                                );
                            }
                            let m = format!(
                                "Renamed {} to {} (names are ASCII only)",
                                String::from_utf8_lossy(&full),
                                new.display()
                            );
                            emit_job_msg(ctx, host_cb, MK_INFO, m.as_bytes());
                        }
                        Ok(None) => {}
                        Err(e) => {
                            let m = format!(
                                "Could not rename {} to an ASCII name: {e}",
                                String::from_utf8_lossy(&full)
                            );
                            emit_job_msg(ctx, host_cb, MK_ERROR, m.as_bytes());
                        }
                    }
                }
            }
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
thread_local! {
    static POST_JOB_ARENA_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static POST_JOB_STR_ARENA: std::cell::RefCell<Vec<std::ffi::CString>> = const { std::cell::RefCell::new(Vec::new()) };
    static POST_JOB_RANGE_ARENA: std::cell::RefCell<Vec<Vec<crate::dupefallback::NzbgetRsByteRange>>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(unix)]
struct PostJobArenaGuard;

#[cfg(unix)]
impl PostJobArenaGuard {
    fn enter() -> Self {
        POST_JOB_ARENA_DEPTH.with(|d| d.set(d.get() + 1));
        Self
    }
}

#[cfg(unix)]
impl Drop for PostJobArenaGuard {
    fn drop(&mut self) {
        POST_JOB_ARENA_DEPTH.with(|d| {
            let next = d.get().saturating_sub(1);
            d.set(next);
            if next == 0 {
                reset_call_arena();
            }
        });
    }
}

#[cfg(unix)]
#[inline]
fn reset_call_arena() {
    POST_JOB_STR_ARENA.with(|a| {
        let mut v = a.borrow_mut();
        v.clear();
        if v.capacity() > 16 {
            v.shrink_to(16);
        }
    });
    POST_JOB_RANGE_ARENA.with(|a| {
        let mut v = a.borrow_mut();
        v.clear();
        if v.capacity() > 4 {
            v.shrink_to(4);
        }
    });
}

#[cfg(unix)]
fn arena_cstr(bytes: &[u8]) -> *const c_char {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    // SAFETY: `bytes[..end]` contains no interior NUL bytes.
    let cs = unsafe { std::ffi::CString::from_vec_unchecked(bytes[..end].to_vec()) };
    let p = cs.as_ptr();
    POST_JOB_STR_ARENA.with(|a| a.borrow_mut().push(cs));
    p
}

#[cfg(unix)]
fn arena_opt_cstr(opt: Option<&[u8]>) -> *const c_char {
    match opt {
        Some(b) => arena_cstr(b),
        None => ptr::null(),
    }
}

#[cfg(unix)]
fn arena_ranges(ranges: Vec<crate::dupefallback::NzbgetRsByteRange>) -> *const c_char {
    if ranges.is_empty() {
        return ptr::null();
    }
    let p = ranges.as_ptr() as *const c_char;
    POST_JOB_RANGE_ARENA.with(|a| a.borrow_mut().push(ranges));
    p
}

/// Marks the post-processing of `post_ptr` as finished (`PostInfo::ptFinished`).
#[cfg(unix)]
fn finish_post_job(post_ptr: usize) {
    use crate::queuesubsys_host::{POST_INT_STAGE, POST_INT_WORKING, PT_FINISHED};
    let Some(host) = crate::ffi::queue_host() else {
        return;
    };
    host.post_set_int(post_ptr, POST_INT_STAGE, PT_FINISHED);
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
    kind: c_int,
    thread_ptr: usize,
    post_ptr: usize,
    nzb_id: i32,
    inner: PostJobInner,
}

#[cfg(unix)]
impl NzbgetRsPostJob {
    pub fn new(kind: c_int, thread_ptr: usize, post_ptr: usize, nzb_id: c_int) -> Self {
        let inner = match kind {
            0 | 1 => {
                crate::log::log_active_bytes(MK_DEBUG, b"Creating RenameController");
                PostJobInner::Rename(kind)
            }
            2 | 10 => PostJobInner::StreamRepair(
                crate::streamrepair_coord::NzbgetRsStreamRepairCtrl::new(
                    kind == 10,
                    nzb_id,
                    crate::articlefetcher::native_nntp_fetch_host,
                    !crate::ffi::build_tls_disabled(),
                    crate::ffi::build_debug_enabled(),
                ),
            ),
            3 => {
                crate::log::log_active_bytes(MK_DEBUG, b"Creating RepairController");
                let ctrl = if crate::ffi::build_parcheck_enabled() {
                    Some(crate::repair::NzbgetRsRepairController::new(
                        crate::articlefetcher::native_nntp_fetch_host,
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
            kind,
            thread_ptr,
            post_ptr,
            nzb_id,
            inner,
        }
    }

    fn stop_from_cpp(&self) {
        match &self.inner {
            PostJobInner::Repair(ctrl_opt) => {
                crate::log::log_active_bytes(MK_DEBUG, b"Stopping RepairController");
                if let Some(ctrl) = ctrl_opt {
                    ctrl.stop();
                }
            }
            PostJobInner::StreamRepair(ctrl) => unsafe {
                crate::streamrepair_coord::stream_repair_ctrl_exec(ctrl, 0, None, ptr::null_mut());
            },
            PostJobInner::Unpack(ctrl) => {
                crate::log::log_active_bytes(MK_DEBUG, b"Stopping unpack");
                ctrl.stop();
            }
            _ => {}
        }
    }

    fn stop_from_rust(&self, host: &crate::queuesubsys_host::QueueSubsysHost) {
        if matches!(self.inner, PostJobInner::StreamRepair(_)) {
            self.stop_from_cpp();
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

    pub unsafe fn exec(&self, op: c_int, a1: c_int, p1: usize) -> c_int {
        let ctx = (self as *const Self).cast_mut().cast::<c_void>();
        match op {
            0 => {
                self.stop_from_cpp();
                0
            }
            1 => {
                let _arena = PostJobArenaGuard::enter();
                match &self.inner {
                    PostJobInner::Rename(rk) => {
                        let mut flags = 0;
                        if crate::ffi::build_parcheck_enabled() {
                            flags |= 1;
                        }
                        if !crate::ffi::build_tls_disabled() {
                            flags |= 2;
                        }
                        run_rename_job(ctx, rename_job_host_cb, *rk, flags);
                        0
                    }
                    PostJobInner::StreamRepair(ctrl) => {
                        crate::streamrepair_coord::stream_repair_ctrl_exec(
                            ctrl,
                            1,
                            Some(stream_repair_job_host_cb),
                            ctx,
                        )
                    }
                    PostJobInner::Repair(Some(ctrl)) => crate::repair::repair_ctrl_exec(
                        ctrl,
                        1,
                        0,
                        0,
                        Some(repair_job_host_cb),
                        ctx,
                    ),
                    PostJobInner::Repair(None) => 0,
                    PostJobInner::Unpack(ctrl) => {
                        unpack_ctrl_exec(ctrl, 0, Some(unpack_job_host_cb), ctx)
                    }
                    PostJobInner::Cleanup => {
                        run_cleanup_job(ctx, cleanup_job_host_cb);
                        0
                    }
                    PostJobInner::Move => {
                        run_move_job(ctx, move_job_host_cb);
                        0
                    }
                    PostJobInner::PostRenamer(post_unpack) => {
                        run_post_renamer_job(ctx, post_renamer_job_host_cb, *post_unpack);
                        0
                    }
                    PostJobInner::Finish => {
                        finish_post_job(self.post_ptr);
                        0
                    }
                }
            }
            2 => {
                let _arena = PostJobArenaGuard::enter();
                if let PostJobInner::Repair(Some(ctrl)) = &self.inner {
                    crate::repair::repair_ctrl_exec(ctrl, 2, a1, p1, Some(repair_job_host_cb), ctx)
                } else {
                    0
                }
            }
            _ => 0,
        }
    }
}

#[cfg(unix)]
struct DqGuardArenaCleanup;

#[cfg(unix)]
impl Drop for DqGuardArenaCleanup {
    fn drop(&mut self) {
        reset_call_arena();
    }
}

#[cfg(unix)]
#[inline]
unsafe fn invoke_dq_guard_cb(
    host: &crate::queuesubsys_host::QueueSubsysHost,
    p1: usize,
    p2: usize,
) {
    let _cleanup = DqGuardArenaCleanup;
    host.with_locked_queue(|dq_ptr| unsafe {
        let cb_fn: extern "C" fn(usize, usize) = std::mem::transmute(p1);
        cb_fn(p2, dq_ptr);
    });
}

#[cfg(unix)]
unsafe extern "C" fn repair_job_host_cb(
    raw_ctx: *mut c_void,
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
) -> usize {
    use crate::queuesubsys_host::{
        FILE_INT_EXTRA_PRIORITY, FILE_INT_PAUSED, HK_NZB, NK_NZB, NZB_INT_EXTRA_PAR_BLOCKS,
        NZB_INT_PAR_FULL, NZB_INT_PAR_SEC, NZB_INT_PAR_STATUS, NZB_INT_REPAIR_SEC,
        POST_INT_FILE_PROGRESS, POST_INT_PAR_REPAIRED, POST_INT_STAGE, POST_INT_STAGE_PROGRESS,
        POST_INT_WORKING,
    };
    if op != 0 {
        reset_call_arena();
    }
    let Some(host) = crate::ffi::queue_host() else {
        return 0;
    };
    let job = unsafe { &*(raw_ctx as *const NzbgetRsPostJob) };
    match op {
        0 => {
            unsafe { invoke_dq_guard_cb(&host, p1, p2) };
            0
        }
        1 => {
            let ps = host.post_snap(job.post_ptr);
            let opts = crate::options::active_options();
            let pause_pp = crate::ffi::active_workstate()
                .as_ref()
                .is_some_and(|w| w.pause_post_process());
            let sc = opts.as_ref().map(|o| o.scalars());
            let ins = unsafe { std::slice::from_raw_parts_mut(ints, 12) };
            let i64_out = unsafe { std::slice::from_raw_parts_mut(i64s, 3) };
            let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 2) };
            ins[0] = i32::from(host.thread_is_stopped(job.thread_ptr));
            ins[1] = sc.as_ref().map_or(0, |s| i32::from(s.par_quick != 0));
            ins[2] = sc.as_ref().map_or(0, |s| s.par_time_limit);
            ins[3] = i32::from(pause_pp);
            ins[4] = i32::from(ps.force_par_full);
            ins[5] = i32::from(ps.force_repair);
            ins[6] = ps.stage;
            ins[7] = sc.as_ref().map_or(0, |s| s.par_buffer);
            ins[8] = 0;
            ins[9] = sc.as_ref().map_or(0, |s| s.par_threads);
            ins[10] = sc.as_ref().map_or(0, |s| s.par_scan);
            ins[11] = sc.as_ref().map_or(0, |s| i32::from(s.par_repair != 0));
            i64_out[0] = current_time_sec();
            i64_out[1] = ps.stage_time;
            i64_out[2] = ps.start_time;
            str_out[0] = arena_cstr(opts.as_ref().map_or(b"", |o| o.str_bytes(30)));
            str_out[1] = arena_cstr(opts.as_ref().map_or(b"", |o| o.str_bytes(33)));
            ps.nzb_ptr
        }
        2 => {
            let ns = host.nzb_snap(p1, false);
            let tf = host.nzb_get_param(p1, b"DupeFiles");
            let ins = unsafe { std::slice::from_raw_parts_mut(ints, 14) };
            let i64_out = unsafe { std::slice::from_raw_parts_mut(i64s, 2) };
            let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 5) };
            ins[0] = ns.id;
            ins[1] = ns.kind;
            ins[2] = NK_NZB;
            ins[3] = ns.download_sec;
            ins[4] = ns.par_sec;
            ins[5] = ns.repair_sec;
            ins[6] = ns.par_status;
            ins[7] = i32::from(ns.reprocess);
            ins[8] = i32::from(ns.force_priority);
            ins[9] = ns.extra_par_blocks;
            ins[10] = host.nzb_file_len(p1) as c_int;
            ins[11] = host.nzb_completed_files_len(p1) as c_int;
            ins[12] = host.nzb_stream_repair_jobs_len(p1) as c_int;
            ins[13] = ns.dupe_mode;
            i64_out[0] = ns.dupe_recovered_bytes;
            i64_out[1] = ns.size - ns.par_size;
            str_out[0] = arena_cstr(&ns.name);
            str_out[1] = arena_cstr(&ns.dest_dir);
            str_out[2] = arena_cstr(&ns.dupe_key);
            str_out[3] = arena_cstr(&ns.queued_filename);
            str_out[4] = arena_opt_cstr(tf.as_deref());
            0
        }
        4 => {
            let fs = host.file_snap(p1);
            let ps = host.post_snap(job.post_ptr);
            let ns = host.nzb_snap(fs.nzb_ptr, false);
            let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 2) };
            str_out[0] = arena_cstr(&ns.dest_dir);
            str_out[1] = arena_cstr(&fs.filename);
            usize::from(fs.nzb_ptr == ps.nzb_ptr)
        }
        5 => {
            let pi = job.post_ptr;
            let ps = host.post_snap(pi);
            let ns = host.nzb_snap(p1, false);
            let ins = unsafe { std::slice::from_raw_parts(ints, 6) };
            host.nzb_set_int(p1, NZB_INT_PAR_STATUS, ins[0]);
            if ins[1] != 0 {
                host.post_set_int(pi, POST_INT_PAR_REPAIRED, i32::from(ins[2] != 0));
            }
            host.post_set_start_time(pi, ps.start_time + i64::from(ins[3]));
            host.nzb_set_int(p1, NZB_INT_PAR_SEC, ns.par_sec + ins[4]);
            host.nzb_set_int(p1, NZB_INT_PAR_FULL, i32::from(ins[5] != 0));
            host.post_set_int(pi, POST_INT_WORKING, 0);
            0
        }
        6 => {
            let fi_ptr = host.nzb_file_at(p1, a1 as usize);
            if a2 == 0 {
                let fs = host.file_snap(fi_ptr);
                let ins = unsafe { std::slice::from_raw_parts_mut(ints, 3) };
                ins[0] = i32::from(fs.filename_confirmed);
                ins[1] = i32::from(fs.paused);
                ins[2] = i32::from(fs.par_file);
                unsafe { *i64s = fs.size };
                unsafe { *strs = arena_cstr(&fs.filename) };
                return fi_ptr;
            }
            host.file_set_int(fi_ptr, FILE_INT_PAUSED, 0);
            host.file_set_int(fi_ptr, FILE_INT_EXTRA_PRIORITY, 1);
            0
        }
        7 => {
            let msg = unsafe { ptr_bytes(s1) };
            if a1 <= 4 {
                let ps = host.post_snap(job.post_ptr);
                host.nzb_add_message(ps.nzb_ptr, a1, &msg);
            } else if a1 == 5 {
                host.log(MK_DEBUG, &msg);
            } else if a1 == 6 {
                job.stop_from_rust(&host);
            } else if a1 == 7 {
                host.log(MK_INFO, &msg);
            } else if a1 == 8 {
                host.log(MK_DETAIL, &msg);
            }
            0
        }
        8 => {
            let pi = job.post_ptr;
            if a1 == 0 {
                if !s1.is_null() {
                    host.post_set_progress_label(pi, &unsafe { ptr_bytes(s1) });
                }
                let ins = unsafe { std::slice::from_raw_parts(ints, 5) };
                host.post_set_int(pi, POST_INT_FILE_PROGRESS, ins[0]);
                host.post_set_int(pi, POST_INT_STAGE_PROGRESS, ins[1]);
                if (ins[2] & 1) != 0 {
                    host.post_set_int(pi, POST_INT_STAGE, ins[3]);
                    host.post_set_stage_time(pi, unsafe { *i64s });
                }
                if (ins[2] & 8) != 0 {
                    let ps = host.post_snap(pi);
                    host.nzb_set_int(ps.nzb_ptr, NZB_INT_REPAIR_SEC, ins[4]);
                }
            } else {
                let i0 = unsafe { *i64s };
                let i1 = unsafe { *i64s.add(1) };
                if i0 > 0 {
                    host.post_set_stage_time(pi, i0);
                }
                if i1 > 0 {
                    host.post_set_start_time(pi, i1);
                }
            }
            0
        }
        9 => {
            if a1 == 0 {
                host.post_push_parred_file(job.post_ptr, &unsafe { ptr_bytes(s1) });
                return 0;
            }
            if a1 == 1 {
                return host.post_snap(job.post_ptr).parred_files_len;
            }
            if a1 == 2 {
                let pf = host.post_parred_file_at(job.post_ptr, a2 as usize);
                unsafe { *strs = arena_cstr(&pf) };
                return 0;
            }
            if a1 == 3 {
                let cf = host.nzb_completed_file_at(p1, a2 as usize);
                let ins = unsafe { std::slice::from_raw_parts_mut(ints, 3) };
                let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 2) };
                ins[0] = cf.id;
                ins[1] = cf.status;
                ins[2] = cf.crc as c_int;
                str_out[0] = arena_cstr(&cf.filename);
                str_out[1] = arena_cstr(&cf.origname);
                return 0;
            }
            if a1 == 4 {
                let jd = host.nzb_stream_repair_job_detail(p1, a2 as usize);
                unsafe { *ints = jd.file_id };
                unsafe { *strs = arena_cstr(&jd.filename) };
                return 0;
            }
            if a1 == 5 {
                host.nzb_stream_repair_erase_job(p1, a2 as usize, true);
                return 0;
            }
            host.disk_load_completed_segments(a2, p1, s2 as usize)
        }
        10 => {
            if a1 == 0 {
                let ins = unsafe { std::slice::from_raw_parts_mut(ints, 2) };
                ins[0] = host.queue_len(p1) as c_int;
                ins[1] = host.history_len(p1) as c_int;
                return 0;
            }
            if a1 == 1 {
                return host.queue_at(p1, a2 as usize);
            }
            if a1 == 2 {
                let h = host.history_at(p1, a2 as usize);
                return if h.kind == HK_NZB { h.inner_ptr } else { 0 };
            }
            if a1 == 7 {
                host.nzb_set_int(p1, NZB_INT_EXTRA_PAR_BLOCKS, a2);
            }
            0
        }
        _ => 0,
    }
}

#[cfg(unix)]
unsafe extern "C" fn stream_repair_job_host_cb(
    raw_ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    use crate::queuesubsys_host::{
        HK_NZB, NK_NZB, NZB_ACT_CLEAR_LIVE_REPAIR_THREAD, NZB_I64_CUR_FAILED_SIZE,
        NZB_I64_CUR_SUCCESS_SIZE, NZB_I64_DUPE_RECOVERED_BYTES, NZB_I64_FAILED_SIZE,
        NZB_I64_PAR_CUR_FAILED_SIZE, NZB_I64_PAR_CUR_SUCCESS_SIZE, NZB_I64_PAR_FAILED_SIZE,
        NZB_I64_PAR_SUCCESS_SIZE, NZB_I64_SUCCESS_SIZE, NZB_INT_DUPE_RECOVERED_ARTICLES,
        NZB_INT_DUPE_RECOVERED_HOLES, NZB_INT_PAR_RENAME_STATUS, NZB_INT_PAR_STATUS,
        POST_INT_FORCE_PAR_FULL, POST_INT_REQUEST_PAR_CHECK, POST_INT_STAGE_PROGRESS,
        POST_INT_STREAM_REPAIR_DONE, POST_INT_STREAM_REPAIR_RECOVERED, POST_INT_WORKING, RS_NONE,
    };
    if op != 0 {
        reset_call_arena();
    }
    let Some(host) = crate::ffi::queue_host() else {
        return 0;
    };
    let job = unsafe { &*(raw_ctx as *const NzbgetRsPostJob) };
    let live_mode = job.kind == 10;
    match op {
        0 => {
            unsafe { invoke_dq_guard_cb(&host, p1, p2) };
            0
        }
        1 => usize::from(host.thread_is_stopped(job.thread_ptr)),
        2 => {
            job.stop_from_rust(&host);
            0
        }
        3 => 0,
        4 => {
            let msg = unsafe { ptr_bytes(s1) };
            if live_mode {
                host.with_locked_queue(|dq| {
                    let nzb = host.queue_find_by_id(dq, job.nzb_id);
                    if nzb != 0 {
                        host.nzb_add_message(nzb, a1, &msg);
                    }
                });
            } else if job.post_ptr != 0 {
                let ps = host.post_snap(job.post_ptr);
                if ps.nzb_ptr != 0 {
                    host.nzb_add_message(ps.nzb_ptr, a1, &msg);
                }
            }
            0
        }
        5 => {
            if s1.is_null() {
                return 0;
            }
            let kind = match a1 {
                0 => MK_INFO,
                1 => MK_ERROR,
                2 => MK_WARNING,
                3 => MK_DETAIL,
                4 => MK_DEBUG,
                _ => return 0,
            };
            host.log(kind, &unsafe { ptr_bytes(s1) });
            0
        }
        7 => {
            let nzb = if live_mode {
                let n = host.queue_find_by_id(p1, job.nzb_id);
                if n == 0 || host.nzb_snap(n, false).live_repair_thread_ptr != job.thread_ptr {
                    return 0;
                }
                n
            } else {
                if job.post_ptr == 0 {
                    return 0;
                }
                let ps = host.post_snap(job.post_ptr);
                if ps.nzb_ptr == 0 {
                    return 0;
                }
                if a1 != 0 {
                    host.post_set_progress_label(job.post_ptr, b"Repairing files from duplicates");
                }
                ps.nzb_ptr
            };
            let ns = host.nzb_snap(nzb, false);
            let pw = host.nzb_get_param(nzb, b"*Unpack:Password");
            let ins = unsafe { std::slice::from_raw_parts_mut(ints, 6) };
            let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 4) };
            ins[0] = ns.id;
            ins[1] = i32::from(ns.par_size > 0);
            ins[2] = host.nzb_completed_files_len(nzb) as c_int;
            ins[3] = host.nzb_stream_repair_jobs_len(nzb) as c_int;
            ins[4] = host.queue_len(p1) as c_int;
            ins[5] = host.history_len(p1) as c_int;
            unsafe { *i64s = i64::from(ns.full_content_hash) };
            str_out[0] = arena_cstr(&ns.name);
            str_out[1] = arena_cstr(&ns.dest_dir);
            str_out[2] = arena_cstr(&ns.dupe_key);
            str_out[3] = arena_opt_cstr(pw.as_deref());
            nzb
        }
        8 => {
            let cf = host.nzb_completed_file_at(p1, a1 as usize);
            let ins = unsafe { std::slice::from_raw_parts_mut(ints, 3) };
            ins[0] = cf.id;
            ins[1] = i32::from(cf.par_file);
            ins[2] = cf.status;
            unsafe { *strs = arena_cstr(&cf.filename) };
            0
        }
        9 => {
            if a2 == 1 {
                return usize::from(host.nzb_stream_repair_mark_live_attempted(p1, a1 as usize));
            }
            let jd = host.nzb_stream_repair_job_detail(p1, a1 as usize);
            let ins = unsafe { std::slice::from_raw_parts_mut(ints, 5) };
            let i64_out = unsafe { std::slice::from_raw_parts_mut(i64s, 4) };
            let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 2) };
            ins[0] = jd.file_id;
            ins[1] = jd.failed_articles;
            ins[2] = i32::from(jd.par_file);
            ins[3] = i32::from(jd.live_attempted);
            ins[4] = jd.holes.len() as c_int;
            i64_out[0] = jd.decoded_file_size;
            i64_out[1] = jd.failed_size;
            i64_out[2] = jd.missed_size;
            i64_out[3] = jd.steps_hash as i64;
            str_out[0] = arena_cstr(&jd.filename);
            str_out[1] = arena_ranges(jd.holes);
            0
        }
        10 => {
            let cand = if a1 == 0 {
                let c = host.queue_at(p1, a2 as usize);
                if c == 0 || host.nzb_snap(c, false).kind != NK_NZB {
                    return 0;
                }
                c
            } else {
                let h = host.history_at(p1, a2 as usize);
                if h.kind != HK_NZB || h.inner_ptr == 0 {
                    return 0;
                }
                h.inner_ptr
            };
            let cs = host.nzb_snap(cand, false);
            let pw = host.nzb_get_param(cand, b"*Unpack:Password");
            let ins = unsafe { std::slice::from_raw_parts_mut(ints, 3) };
            let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 4) };
            ins[0] = cs.id;
            ins[1] = cs.dupe_score;
            ins[2] = cs.dupe_mode;
            unsafe { *i64s = i64::from(cs.full_content_hash) };
            str_out[0] = arena_cstr(&cs.name);
            str_out[1] = arena_cstr(&cs.dupe_key);
            str_out[2] = arena_cstr(&cs.queued_filename);
            str_out[3] = arena_opt_cstr(pw.as_deref());
            cand
        }
        11 => {
            if job.post_ptr != 0 {
                let lbl = unsafe { ptr_bytes(s1) };
                host.with_locked_queue(|_| {
                    host.post_set_progress_label(job.post_ptr, &lbl);
                    host.post_set_int(job.post_ptr, POST_INT_STAGE_PROGRESS, a1);
                });
            }
            0
        }
        12 => {
            host.nzb_stream_repair_update_completed_file(p1, a1, a2);
            0
        }
        13 => {
            if a2 == 0 {
                host.nzb_stream_repair_erase_job(p1, a1 as usize, false);
                return 0;
            }
            let new_dec = unsafe { *i64s };
            let holes_len = unsafe { *ints };
            host.nzb_stream_repair_update_job_holes(p1, a1 as usize, new_dec, p2, holes_len);
            0
        }
        14 => {
            if a2 == 0 {
                let ns = host.nzb_snap(p2, false);
                let ins = unsafe { std::slice::from_raw_parts_mut(ints, 2) };
                let i64_out = unsafe { std::slice::from_raw_parts_mut(i64s, 8) };
                ins[0] = ns.par_rename_status;
                ins[1] = ns.par_status;
                i64_out[0] = ns.failed_size;
                i64_out[1] = ns.success_size;
                i64_out[2] = ns.current_failed_size;
                i64_out[3] = ns.current_success_size;
                i64_out[4] = ns.par_failed_size;
                i64_out[5] = ns.par_success_size;
                i64_out[6] = ns.par_current_failed_size;
                i64_out[7] = ns.par_current_success_size;
                return 0;
            }
            let ns = host.nzb_snap(p2, false);
            let ins = unsafe { std::slice::from_raw_parts(ints, 4) };
            let i64_in = unsafe { std::slice::from_raw_parts(i64s, 9) };
            if ins[0] > 0 || i64_in[8] > 0 || ins[1] > 0 {
                host.nzb_set_int(
                    p2,
                    NZB_INT_DUPE_RECOVERED_ARTICLES,
                    ns.dupe_recovered_articles + ins[0],
                );
                host.nzb_set_i64(
                    p2,
                    NZB_I64_DUPE_RECOVERED_BYTES,
                    ns.dupe_recovered_bytes + i64_in[8],
                );
                host.nzb_set_int(
                    p2,
                    NZB_INT_DUPE_RECOVERED_HOLES,
                    ns.dupe_recovered_holes + ins[1],
                );
            }
            if a1 != 0 {
                host.nzb_set_i64(p2, NZB_I64_FAILED_SIZE, i64_in[0]);
                host.nzb_set_i64(p2, NZB_I64_SUCCESS_SIZE, i64_in[1]);
                host.nzb_set_i64(p2, NZB_I64_CUR_FAILED_SIZE, i64_in[2]);
                host.nzb_set_i64(p2, NZB_I64_CUR_SUCCESS_SIZE, i64_in[3]);
                host.nzb_set_i64(p2, NZB_I64_PAR_FAILED_SIZE, i64_in[4]);
                host.nzb_set_i64(p2, NZB_I64_PAR_SUCCESS_SIZE, i64_in[5]);
                host.nzb_set_i64(p2, NZB_I64_PAR_CUR_FAILED_SIZE, i64_in[6]);
                host.nzb_set_i64(p2, NZB_I64_PAR_CUR_SUCCESS_SIZE, i64_in[7]);
            }
            let pi = job.post_ptr;
            host.post_set_int(pi, POST_INT_STREAM_REPAIR_DONE, 1);
            host.post_set_int(
                pi,
                POST_INT_STREAM_REPAIR_RECOVERED,
                i32::from(i64_in[8] > 0),
            );
            let pf = ins[2];
            if (pf & 1) != 0 {
                host.nzb_set_int(p2, NZB_INT_PAR_RENAME_STATUS, RS_NONE);
            }
            if (pf & 2) != 0 {
                host.nzb_set_int(p2, NZB_INT_PAR_STATUS, ins[3]);
            }
            if (pf & 4) != 0 {
                host.post_set_int(pi, POST_INT_FORCE_PAR_FULL, 1);
            }
            if (pf & 8) != 0 {
                host.post_set_int(pi, POST_INT_REQUEST_PAR_CHECK, 0);
            }
            if (pf & 16) != 0 {
                host.post_set_int(pi, POST_INT_REQUEST_PAR_CHECK, 1);
            }
            host.dq_save(p1);
            host.post_set_int(pi, POST_INT_WORKING, 0);
            0
        }
        16 => {
            let out = unsafe { std::slice::from_raw_parts_mut(ints, 2) };
            out[0] = -1;
            let nzb = if live_mode {
                host.queue_find_by_id(p1, job.nzb_id)
            } else if job.post_ptr != 0 {
                host.post_snap(job.post_ptr).nzb_ptr
            } else {
                0
            };
            if nzb != 0 {
                let pending = (0..host.nzb_file_len(nzb))
                    .map(|i| host.nzb_file_at(nzb, i))
                    .filter(|&f| f != 0 && !host.file_snap(f).par_file)
                    .count();
                out[0] = pending as c_int;
                out[1] = host.nzb_snap(nzb, false).id;
            }
            0
        }
        15 => {
            if a2 == 0 {
                let nzb = host.queue_find_by_id(p1, job.nzb_id);
                if nzb == 0 {
                    return 0;
                }
                let ns = host.nzb_snap(nzb, false);
                let rec_bytes = unsafe { *i64s };
                if ns.live_repair_thread_ptr != job.thread_ptr {
                    host.nzb_set_i64(
                        nzb,
                        NZB_I64_DUPE_RECOVERED_BYTES,
                        ns.dupe_recovered_bytes + rec_bytes,
                    );
                    return 0;
                }
                let rec_holes = unsafe { *ints.add(1) };
                if rec_bytes > 0 || rec_holes > 0 {
                    host.nzb_set_i64(
                        nzb,
                        NZB_I64_DUPE_RECOVERED_BYTES,
                        ns.dupe_recovered_bytes + rec_bytes,
                    );
                    host.nzb_set_int(
                        nzb,
                        NZB_INT_DUPE_RECOVERED_HOLES,
                        ns.dupe_recovered_holes + rec_holes,
                    );
                }
                unsafe { *ints = host.nzb_stream_repair_jobs_len(nzb) as c_int };
                return nzb;
            }
            host.nzb_action(p2, NZB_ACT_CLEAR_LIVE_REPAIR_THREAD, 0);
            let ns = host.nzb_snap(p2, false);
            // A volume arrived while this pass ran and direct unpack still waits on a dead
            // volume: one more pass may now place it for the borrow.
            if ns.post_info_ptr == 0
                && take_dead_volume_rerun(ns.id)
                && direct_unpack_waits_on_dead_volume(ns.id)
            {
                host.stream_repair_start_live(p2);
            }
            // A direct unpack parked on damaged volumes waits for post-processing itself, so
            // it will not release the job: the ending live repair has to.
            if ns.post_info_ptr != 0
                && (!ns.has_unpack_thread || direct_unpack_awaiting_repair(ns.id))
            {
                host.post_set_int(ns.post_info_ptr, POST_INT_WORKING, 0);
            }
            host.dq_save(p1);
            0
        }
        _ => 0,
    }
}

#[cfg(unix)]
unsafe extern "C" fn unpack_job_host_cb(
    raw_ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    _s2: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    use crate::queuesubsys_host::{
        NZB_INT_PAR_RENAME_STATUS, NZB_INT_UNPACK_CLEANED_UP_DISK, NZB_INT_UNPACK_SEC,
        NZB_INT_UNPACK_STATUS, NZB_STR_FINAL_DIR, POST_INT_FORCE_REPAIR,
        POST_INT_LAST_UNPACK_STATUS, POST_INT_PASS_LIST_TRIED, POST_INT_REQUEST_PAR_CHECK,
        POST_INT_STAGE_PROGRESS, POST_INT_UNPACK_TRIED, POST_INT_WORKING, RS_NONE, US_SUCCESS,
    };
    if op != 0 {
        reset_call_arena();
    }
    let Some(host) = crate::ffi::queue_host() else {
        return 0;
    };
    let job = unsafe { &*(raw_ctx as *const NzbgetRsPostJob) };
    match op {
        0 => {
            unsafe { invoke_dq_guard_cb(&host, p1, p2) };
            0
        }
        1 => {
            let pi = job.post_ptr;
            let ps = host.post_snap(pi);
            let n = ps.nzb_ptr;
            if a1 == 0 {
                let ns = host.nzb_snap(n, false);
                let up = host.nzb_get_param(n, b"*Unpack:");
                let pw = host.nzb_get_param(n, b"*Unpack:Password");
                let opts = crate::options::active_options();
                let sc = opts.as_ref().map(|o| o.scalars());
                let ins = unsafe { std::slice::from_raw_parts_mut(ints, 18) };
                let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 7) };
                ins[0] = i32::from(!up.as_deref().is_some_and(|v| v.eq_ignore_ascii_case(b"no")));
                ins[1] = i32::from(pw.is_some());
                ins[2] = host.nzb_completed_files_len(n) as c_int;
                ins[3] = ps.extracted_archives_len as c_int;
                ins[4] = i32::from(ps.unpack_tried);
                ins[5] = i32::from(ps.par_repaired);
                ins[6] = i32::from(ps.pass_list_tried);
                ins[7] = ps.last_unpack_status;
                ins[8] = ns.direct_unpack_status;
                ins[9] = ns.par_status;
                ins[10] = ns.par_rename_status;
                ins[11] = 0;
                ins[12] = sc
                    .as_ref()
                    .map_or(0, |s| i32::from(s.use_temp_unpack_dir != 0));
                ins[13] = sc
                    .as_ref()
                    .map_or(0, |s| i32::from(s.unpack_cleanup_disk != 0));
                ins[14] = sc.as_ref().map_or(0, |s| s.write_buffer);
                ins[15] = sc.as_ref().map_or(0, |s| i32::from(s.par_rename != 0));
                ins[16] = crate::lifecycle::active_umask() as c_int;
                ins[17] = i32::from(crate::ffi::build_parcheck_enabled());
                str_out[0] = arena_cstr(&ns.dest_dir);
                str_out[1] = arena_cstr(&ns.final_dir);
                str_out[2] = arena_cstr(&ns.name);
                str_out[3] = arena_opt_cstr(pw.as_deref());
                str_out[4] = arena_cstr(opts.as_ref().map_or(b"", |o| o.str_bytes(31)));
                str_out[5] = arena_cstr(opts.as_ref().map_or(b"", |o| o.str_bytes(35)));
                str_out[6] = arena_cstr(opts.as_ref().map_or(b"", |o| o.str_bytes(4)));
                return 0;
            }
            if a1 == 1 {
                let cf = host.nzb_completed_file_at(n, a2 as usize);
                unsafe { *strs = arena_cstr(&cf.filename) };
                return 0;
            }
            if a1 == 2 {
                let ea = host.post_extracted_archive_at(pi, a2 as usize);
                unsafe { *strs = arena_cstr(&ea) };
                return 0;
            }
            if a1 == 3 {
                host.post_clear_extracted_archives(pi);
            }
            0
        }
        2 => {
            if a1 == 0 {
                return usize::from(host.thread_is_stopped(job.thread_ptr));
            }
            if a1 == 1 {
                job.stop_from_rust(&host);
                return 0;
            }
            if a1 == 2 {
                host.thread_resume(job.thread_ptr);
                return 0;
            }
            if a1 == 3 {
                if job.post_ptr != 0 {
                    let ps = host.post_snap(job.post_ptr);
                    host.nzb_add_message(ps.nzb_ptr, a2, &unsafe { ptr_bytes(s1) });
                }
                return 0;
            }
            if a1 == 4 {
                let lbl = unsafe { ptr_bytes(s1) };
                host.with_locked_queue(|_| host.post_set_progress_label(job.post_ptr, &lbl));
                return 0;
            }
            if a1 == 5 {
                if job.post_ptr != 0 {
                    host.post_set_int(job.post_ptr, POST_INT_STAGE_PROGRESS, a2);
                }
                return 0;
            }
            if a1 == 6 {
                if job.post_ptr != 0 {
                    host.post_set_stage_time(job.post_ptr, unsafe { *i64s });
                }
                return 0;
            }
            if a1 == 7 {
                if a2 == 3 || a2 == 4 {
                    host.log(a2, &unsafe { ptr_bytes(s1) });
                }
                return 0;
            }
            if a1 == 8 {
                return usize::from(job.post_ptr != 0 && host.post_snap(job.post_ptr).deleted);
            }
            0
        }
        3 => {
            let pi = job.post_ptr;
            let ps = host.post_snap(pi);
            let n = ps.nzb_ptr;
            if a1 == 0 {
                host.nzb_set_int(n, NZB_INT_UNPACK_STATUS, a2);
                return 0;
            }
            if a1 == 1 {
                host.nzb_set_int(n, NZB_INT_UNPACK_STATUS, US_SUCCESS);
                host.nzb_set_int(n, NZB_INT_UNPACK_CLEANED_UP_DISK, i32::from(a2 != 0));
                if p1 != 0 {
                    host.nzb_set_int(n, NZB_INT_PAR_RENAME_STATUS, RS_NONE);
                }
                return 0;
            }
            if a1 == 2 {
                let ins = unsafe { std::slice::from_raw_parts(ints, 3) };
                host.post_set_int(pi, POST_INT_REQUEST_PAR_CHECK, 1);
                host.post_set_int(pi, POST_INT_FORCE_REPAIR, i32::from(ins[0] != 0));
                host.post_set_int(pi, POST_INT_UNPACK_TRIED, 1);
                host.post_set_int(pi, POST_INT_PASS_LIST_TRIED, i32::from(ins[1] != 0));
                host.post_set_int(pi, POST_INT_LAST_UNPACK_STATUS, ins[2]);
                return 0;
            }
            if a1 == 3 {
                let ns = host.nzb_snap(n, false);
                host.nzb_set_int(n, NZB_INT_UNPACK_SEC, ns.unpack_sec + a2);
                host.post_set_int(pi, POST_INT_WORKING, 0);
                return 0;
            }
            if a1 == 4 {
                let ns = host.nzb_snap(n, false);
                let ins = unsafe { std::slice::from_raw_parts_mut(ints, 2) };
                ins[0] = ns.par_status;
                ins[1] = i32::from(ns.par_full);
                return 0;
            }
            if a1 == 5 {
                let fin = host.nzb_build_final_dir_name(n);
                let c_fin = to_c_buf(&fin);
                let cb_fn: extern "C" fn(usize, *const c_char) = unsafe { std::mem::transmute(p2) };
                cb_fn(p1, c_fin.as_ptr() as *const c_char);
                return 0;
            }
            if a1 == 6 {
                host.nzb_set_str(n, NZB_STR_FINAL_DIR, &unsafe { ptr_bytes(s1) });
            }
            0
        }
        _ => 0,
    }
}

#[cfg(unix)]
unsafe extern "C" fn move_job_host_cb(
    raw_ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    s2: *const c_char,
    _ints: *mut c_int,
    _i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    use crate::queuesubsys_host::{
        NZB_INT_MOVE_STATUS, NZB_STR_DEST_DIR, NZB_STR_FINAL_DIR, POST_INT_WORKING,
    };
    if op != 0 {
        reset_call_arena();
    }
    let Some(host) = crate::ffi::queue_host() else {
        return 0;
    };
    let job = unsafe { &*(raw_ctx as *const NzbgetRsPostJob) };
    match op {
        0 => {
            unsafe { invoke_dq_guard_cb(&host, p1, p2) };
            0
        }
        1 => {
            if a1 == 2 {
                return usize::from(job.post_ptr != 0 && host.post_snap(job.post_ptr).nzb_ptr != 0);
            }
            let n = host.post_snap(job.post_ptr).nzb_ptr;
            if a1 == 1 {
                let hl = host.nzb_hard_link_path(n);
                unsafe { *strs = arena_cstr(&hl) };
                return 0;
            }
            let ns = host.nzb_snap(n, false);
            let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 3) };
            str_out[0] = arena_cstr(&ns.name);
            str_out[1] = arena_cstr(&ns.dest_dir);
            str_out[2] = arena_cstr(&ns.final_dir);
            if ns.final_dir.is_empty() {
                let fin = host.nzb_build_final_dir_name(n);
                let c_fin = to_c_buf(&fin);
                let cb_fn: extern "C" fn(usize, *const c_char) = unsafe { std::mem::transmute(p2) };
                cb_fn(p1, c_fin.as_ptr() as *const c_char);
            }
            0
        }
        2 => {
            if a1 == 0 {
                return usize::from(host.thread_is_stopped(job.thread_ptr));
            }
            if a1 == 1 {
                let n = host.post_snap(job.post_ptr).nzb_ptr;
                host.nzb_add_message(n, a2, &unsafe { ptr_bytes(s1) });
                return 0;
            }
            if a1 == 2 && job.post_ptr != 0 {
                let n = host.post_snap(job.post_ptr).nzb_ptr;
                if n != 0 {
                    host.nzb_rename_completed_file(n, &unsafe { ptr_bytes(s1) }, &unsafe {
                        ptr_bytes(s2)
                    });
                }
            }
            0
        }
        3 => {
            if a1 == 0 {
                let n = host.post_snap(job.post_ptr).nzb_ptr;
                host.nzb_set_int(n, NZB_INT_MOVE_STATUS, a2);
            } else if a1 == 1 {
                let n = host.post_snap(job.post_ptr).nzb_ptr;
                host.nzb_set_str(n, NZB_STR_DEST_DIR, &unsafe { ptr_bytes(s1) });
                host.nzb_set_str(n, NZB_STR_FINAL_DIR, b"");
                host.nzb_set_int(n, NZB_INT_MOVE_STATUS, a2);
            } else if a1 == 2 {
                host.post_set_int(job.post_ptr, POST_INT_WORKING, 0);
            }
            0
        }
        _ => 0,
    }
}

#[cfg(unix)]
unsafe extern "C" fn cleanup_job_host_cb(
    raw_ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    _s2: *const c_char,
    _ints: *mut c_int,
    _i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    use crate::queuesubsys_host::{NZB_INT_CLEANUP_STATUS, POST_INT_WORKING};
    if op != 0 {
        reset_call_arena();
    }
    let Some(host) = crate::ffi::queue_host() else {
        return 0;
    };
    let job = unsafe { &*(raw_ctx as *const NzbgetRsPostJob) };
    match op {
        0 => {
            unsafe { invoke_dq_guard_cb(&host, p1, p2) };
            0
        }
        1 => {
            let n = host.post_snap(job.post_ptr).nzb_ptr;
            let ns = host.nzb_snap(n, false);
            let opts = crate::options::active_options();
            let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 4) };
            str_out[0] = arena_cstr(&ns.name);
            str_out[1] = arena_cstr(&ns.dest_dir);
            str_out[2] = arena_cstr(&ns.final_dir);
            str_out[3] = arena_cstr(opts.as_ref().map_or(b"", |o| o.str_bytes(32)));
            0
        }
        2 => {
            if a1 == 1 {
                let n = host.post_snap(job.post_ptr).nzb_ptr;
                host.nzb_add_message(n, a2, &unsafe { ptr_bytes(s1) });
            }
            0
        }
        3 => {
            let n = host.post_snap(job.post_ptr).nzb_ptr;
            host.nzb_set_int(n, NZB_INT_CLEANUP_STATUS, a2);
            host.post_set_int(job.post_ptr, POST_INT_WORKING, 0);
            0
        }
        _ => 0,
    }
}

#[cfg(unix)]
unsafe extern "C" fn rename_job_host_cb(
    raw_ctx: *mut c_void,
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
) -> usize {
    use crate::queuesubsys_host::{
        EA_GROUP_PAUSE_EXTRA_PARS, EA_GROUP_RESUME, NZB_INT_PAR_RENAME_STATUS,
        NZB_INT_RAR_RENAME_STATUS, POST_INT_REQUEST_PAR_CHECK, POST_INT_STAGE_PROGRESS,
        POST_INT_WORKING,
    };
    if op != 0 {
        reset_call_arena();
    }
    let Some(host) = crate::ffi::queue_host() else {
        return 0;
    };
    let job = unsafe { &*(raw_ctx as *const NzbgetRsPostJob) };
    match op {
        0 => {
            unsafe { invoke_dq_guard_cb(&host, p1, p2) };
            0
        }
        1 => {
            let n = host.post_snap(job.post_ptr).nzb_ptr;
            let ns = host.nzb_snap(n, false);
            let ins = unsafe { std::slice::from_raw_parts_mut(ints, 2) };
            if a1 == 1 {
                ins[0] = ns.remaining_par_count;
                ins[1] = ns.par_status;
                return 0;
            }
            let pw = host.nzb_get_param(n, b"*Unpack:Password");
            let opts = crate::options::active_options();
            let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 6) };
            ins[0] = i32::from(pw.is_some());
            ins[1] = ns.unpack_status;
            str_out[0] = arena_cstr(&ns.name);
            str_out[1] = arena_cstr(&ns.dest_dir);
            str_out[2] = arena_cstr(&ns.final_dir);
            str_out[3] = arena_opt_cstr(pw.as_deref());
            str_out[4] = arena_cstr(opts.as_ref().map_or(b"", |o| o.str_bytes(33)));
            str_out[5] = arena_cstr(opts.as_ref().map_or(b"", |o| o.str_bytes(35)));
            0
        }
        2 => {
            if a1 == 0 {
                return usize::from(host.thread_is_stopped(job.thread_ptr));
            }
            if a1 == 1 {
                let n = host.post_snap(job.post_ptr).nzb_ptr;
                host.nzb_add_message(n, a2, &unsafe { ptr_bytes(s1) });
                return 0;
            }
            if a1 == 2 {
                let n = host.post_snap(job.post_ptr).nzb_ptr;
                return usize::from(host.nzb_rename_completed_file(
                    n,
                    &unsafe { ptr_bytes(s1) },
                    &unsafe { ptr_bytes(s2) },
                ));
            }
            if a1 == 3 {
                host.post_set_progress_label(job.post_ptr, &unsafe { ptr_bytes(s1) });
                host.post_set_int(job.post_ptr, POST_INT_STAGE_PROGRESS, a2);
                return 0;
            }
            if a1 == 4 {
                host.post_push_parred_file(job.post_ptr, &unsafe { ptr_bytes(s1) });
            }
            0
        }
        3 => {
            let n = host.post_snap(job.post_ptr).nzb_ptr;
            if a1 == 1 {
                let ns = host.nzb_snap(n, false);
                host.nzb_set_int(n, NZB_INT_PAR_RENAME_STATUS, a2);
                host.dq_edit_entry(p1, ns.id, EA_GROUP_RESUME, None);
                host.dq_edit_entry(p1, ns.id, EA_GROUP_PAUSE_EXTRA_PARS, None);
                unsafe { *i64s = host.nzb_snap(n, false).remaining_size };
                return 0;
            }
            if a2 == 0 {
                host.nzb_set_int(n, NZB_INT_PAR_RENAME_STATUS, p1 as c_int);
            } else if a2 == 1 {
                host.nzb_set_int(n, NZB_INT_RAR_RENAME_STATUS, p1 as c_int);
            }
            if crate::ffi::build_parcheck_enabled() && p2 != 0 {
                host.post_set_int(job.post_ptr, POST_INT_REQUEST_PAR_CHECK, 1);
            }
            host.post_set_int(job.post_ptr, POST_INT_WORKING, 0);
            0
        }
        _ => 0,
    }
}

#[cfg(unix)]
unsafe extern "C" fn post_renamer_job_host_cb(
    raw_ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    s2: *const c_char,
    _ints: *mut c_int,
    _i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize {
    use crate::queuesubsys_host::{
        NZB_INT_POST_DOWNLOAD_RENAMING_STATUS, NZB_INT_POST_UNPACK_RENAMING_STATUS,
        POST_INT_WORKING,
    };
    if op != 0 {
        reset_call_arena();
    }
    let Some(host) = crate::ffi::queue_host() else {
        return 0;
    };
    let job = unsafe { &*(raw_ctx as *const NzbgetRsPostJob) };
    match op {
        0 => {
            unsafe { invoke_dq_guard_cb(&host, p1, p2) };
            0
        }
        1 => {
            let n = host.post_snap(job.post_ptr).nzb_ptr;
            let ns = host.nzb_snap(n, false);
            let meta = host.nzb_meta_name(n);
            let opts = crate::options::active_options();
            let str_out = unsafe { std::slice::from_raw_parts_mut(strs, 4) };
            str_out[0] = arena_cstr(&meta);
            str_out[1] = arena_cstr(&ns.name);
            str_out[2] = arena_cstr(&ns.dest_dir);
            str_out[3] = arena_cstr(opts.as_ref().map_or(b"", |o| o.str_bytes(34)));
            0
        }
        2 => {
            if a1 == 0 {
                return usize::from(host.thread_is_stopped(job.thread_ptr));
            }
            if a1 == 1 {
                let n = host.post_snap(job.post_ptr).nzb_ptr;
                host.nzb_add_message(n, a2, &unsafe { ptr_bytes(s1) });
                return 0;
            }
            if a1 == 2 && job.post_ptr != 0 {
                let n = host.post_snap(job.post_ptr).nzb_ptr;
                if n != 0 {
                    host.nzb_rename_completed_file(n, &unsafe { ptr_bytes(s1) }, &unsafe {
                        ptr_bytes(s2)
                    });
                }
            }
            0
        }
        3 => {
            let n = host.post_snap(job.post_ptr).nzb_ptr;
            if a1 != 0 {
                host.nzb_set_int(n, NZB_INT_POST_UNPACK_RENAMING_STATUS, a2);
            } else {
                host.nzb_set_int(n, NZB_INT_POST_DOWNLOAD_RENAMING_STATUS, a2);
            }
            host.post_set_int(job.post_ptr, POST_INT_WORKING, 0);
            0
        }
        _ => 0,
    }
}

#[cfg(all(test, unix))]
mod post_job_arena_tests {
    use super::*;

    #[test]
    fn repeated_callback_snapshots_remain_bounded() {
        let guard = PostJobArenaGuard::enter();
        for tick in 0..512usize {
            let mut strs = [ptr::null::<c_char>(); 5];
            unsafe {
                let _ = repair_job_host_cb(
                    ptr::null_mut(),
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
            let p0 = arena_cstr(format!("name-{tick}").as_bytes());
            let p1 = arena_cstr(b"/tmp/dest");
            let p2 = arena_ranges(vec![crate::dupefallback::NzbgetRsByteRange {
                offset: tick as i64,
                size: 64,
            }]);
            assert!(!p0.is_null() && !p1.is_null() && !p2.is_null());
            let str_len = POST_JOB_STR_ARENA.with(|a| a.borrow().len());
            let range_len = POST_JOB_RANGE_ARENA.with(|a| a.borrow().len());
            assert_eq!(str_len, 2);
            assert_eq!(range_len, 1);
        }
        drop(guard);
        assert_eq!(POST_JOB_STR_ARENA.with(|a| a.borrow().len()), 0);
        assert_eq!(POST_JOB_RANGE_ARENA.with(|a| a.borrow().len()), 0);
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
        };
        super::direct_unpack_set_virtual(id, &p("movie.part06.rar"), v2);
        assert!(super::direct_unpack_is_held_path(&p("movie.part07.rar")));
        assert_eq!(super::take_dead_volumes_borrowed(id), 1);
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
        use crate::queuesubsys_host::{NzbSnap, PT_LOADING_PARS, PT_QUEUED, PT_UNPACKING};
        let mut nzb = NzbSnap::default();
        assert!(live_repair_may_start(&nzb));
        nzb.has_post_info = true;
        nzb.post_stage = PT_QUEUED;
        assert!(live_repair_may_start(&nzb));
        nzb.post_working = true;
        assert!(!live_repair_may_start(&nzb));
        nzb.post_working = false;
        nzb.post_stage = PT_LOADING_PARS;
        assert!(!live_repair_may_start(&nzb));
        nzb.post_stage = PT_UNPACKING;
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
