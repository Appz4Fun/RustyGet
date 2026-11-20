//! Threads for post-processing jobs, direct unpack, live repair and the
//! pre/post-processor loop.
//!
//! These were C++ `Thread` subclasses (`PostJobThread`, `DirectUnpack`,
//! `PrePostProcessor`) whose bodies already called into Rust. The queue model
//! keeps a [`JobHandle`] (stored as its address) where it used to keep a
//! `Thread*`, and starts, stops and queries those threads through it.

use std::ffi::c_int;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::postproc::{NzbgetRsDirectUnpack, NzbgetRsPostJob};
use crate::queuesubsys_host::{FileRef, NzbRef, PostRef};

/// Post-processing kind whose thread must release OpenSSL state on exit.
const POST_KIND_TLS: c_int = 4;

enum Work {
    Post(Box<NzbgetRsPostJob>),
    Unpack(Box<NzbgetRsDirectUnpack>),
}

/// One job thread. Its address is the "thread pointer" the Rust jobs and the
/// queue model refer to.
pub struct JobThread {
    stopped: AtomicBool,
    running: AtomicBool,
    auto_destroy: bool,
    kind: c_int,
    pthread: AtomicUsize,
    work: Option<Work>,
}

impl JobThread {
    fn run(&self) {
        let _cancel_guard = unsafe { crate::ffi::PosixCancelGuard::disable() };
        match &self.work {
            Some(Work::Post(job)) => job.run(),
            Some(Work::Unpack(du)) => du.run_active(),
            None => {}
        }
    }

    /// The stop request a job's own code sees (was `PostJobThread::Stop`,
    /// which also told the running job to stop).
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(Work::Post(job)) = &self.work {
            let _cancel_guard = unsafe { crate::ffi::PosixCancelGuard::disable() };
            job.request_stop();
        }
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    fn unpack(&self) -> Option<&NzbgetRsDirectUnpack> {
        match &self.work {
            Some(Work::Unpack(du)) => Some(du),
            _ => None,
        }
    }
}

impl Drop for JobThread {
    fn drop(&mut self) {
        self.work = None;
        if self.kind == POST_KIND_TLS {
            crate::ffi::stop_ssl_thread();
        }
    }
}

static START_LOCK: Mutex<()> = Mutex::new(());

/// A handle to a heap-allocated [`JobThread`].
///
/// The handle is `Copy`, like the `Thread*` it replaces: the queue model
/// stores its address, and whoever created it frees it with [`JobHandle::free`]
/// unless it was started with auto-destroy (then the thread frees itself when
/// it finishes). Every method other than the constructors needs the thread to
/// still be alive; [`JobHandle::from_addr`] is where that is promised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct JobHandle(NonNull<JobThread>);

// SAFETY: a JobThread's shared state is atomics plus jobs that are themselves
// used from the queue thread and the job thread (as the C++ Thread was).
unsafe impl Send for JobHandle {}
unsafe impl Sync for JobHandle {}

impl JobHandle {
    fn alloc(kind: c_int, auto_destroy: bool, make: impl FnOnce(usize) -> Work) -> Self {
        let t = Box::into_raw(Box::new(JobThread {
            stopped: AtomicBool::new(false),
            running: AtomicBool::new(false),
            auto_destroy,
            kind,
            pthread: AtomicUsize::new(0),
            work: None,
        }));
        // The job keeps the thread's address to report back to it.
        let work = make(t as usize);
        // SAFETY: `t` was just allocated and nothing else refers to it yet.
        unsafe { (*t).work = Some(work) };
        // SAFETY: Box::into_raw never returns null.
        JobHandle(unsafe { NonNull::new_unchecked(t) })
    }

    /// A post-processing job thread (live repair when `kind` is 10) for the
    /// post-processing entry `post` (or none) of the collection `nzb_id`.
    pub(crate) fn new_post(kind: c_int, post: PostRef, nzb_id: c_int, auto_destroy: bool) -> Self {
        Self::alloc(kind, auto_destroy, |addr| {
            Work::Post(Box::new(NzbgetRsPostJob::new(kind, addr, post, nzb_id)))
        })
    }

    /// A direct-unpack thread for the collection `nzb_id`; frees itself when
    /// done.
    pub(crate) fn new_unpack(nzb_id: c_int) -> Self {
        Self::alloc(-1, true, |addr| {
            Work::Unpack(Box::new(NzbgetRsDirectUnpack::new(nzb_id, addr)))
        })
    }

    /// The handle stored at `addr`, or `None` for 0.
    ///
    /// # Safety
    /// A nonzero `addr` is the address of a live job thread (from
    /// [`JobHandle::addr`]), and it stays alive while the handle is used.
    pub(crate) unsafe fn from_addr(addr: usize) -> Option<Self> {
        NonNull::new(addr as *mut JobThread).map(JobHandle)
    }

    /// The address the queue model stores.
    pub(crate) fn addr(self) -> usize {
        self.0.as_ptr() as usize
    }

    fn thread(&self) -> &JobThread {
        // SAFETY: handles exist only for live threads (see `from_addr`).
        unsafe { self.0.as_ref() }
    }

    /// Starts the thread. It frees itself when it finishes if it was created
    /// with auto-destroy; otherwise the owner frees it. Returns false when no
    /// thread could be created; the caller then still owns the handle and
    /// must clear any copy it stored before freeing it.
    pub(crate) fn start(self) -> bool {
        let thread = self.thread();
        thread.running.store(true, Ordering::SeqCst);
        // Held until the pthread handle is published: a worker that finishes at
        // once and frees itself must not do so before then (C++ Thread::Start's
        // m_threadMutex did the same).
        let start_guard = START_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let name = if thread.unpack().is_some() { "direct-unpack" } else { "post-job" };
        let spawned = std::thread::Builder::new().name(name.into()).spawn(move || {
            drop(START_LOCK.lock().unwrap_or_else(|e| e.into_inner()));
            let t = self.0.as_ptr();
            {
                let thread = self.thread();
                {
                    let _tg = crate::ffi::RustThreadGuard::enter(&thread.stopped);
                    thread.run();
                }
                // read before running goes false: an owner waiting for that may free it
                let auto_destroy = thread.auto_destroy;
                thread.running.store(false, Ordering::SeqCst);
                if !auto_destroy {
                    return;
                }
            }
            // SAFETY: an auto-destroying thread is owned by its worker alone.
            drop(unsafe { Box::from_raw(t) });
        });
        let started = match spawned {
            Ok(handle) => {
                use std::os::unix::thread::JoinHandleExt;
                thread
                    .pthread
                    .store(handle.as_pthread_t() as usize, Ordering::SeqCst);
                drop(handle);
                true
            }
            Err(_) => {
                thread.running.store(false, Ordering::SeqCst);
                false
            }
        };
        drop(start_guard);
        started
    }

    /// Frees a thread that is not auto-destroying, or one whose start failed.
    /// With `kill`, the thread is cancelled first (was `Thread::Kill`).
    ///
    /// # Safety
    /// No one else frees this thread, no running thread uses it, and no copy
    /// of the handle is used afterwards.
    pub(crate) unsafe fn free(self, kill: bool) {
        if kill {
            let thread = self.thread();
            thread.stopped.store(true, Ordering::SeqCst);
            let pt = thread.pthread.load(Ordering::SeqCst);
            if pt != 0 && thread.running.load(Ordering::SeqCst) {
                let cancelled = unsafe { libc::pthread_cancel(pt as libc::pthread_t) } == 0;
                if cancelled {
                    crate::ffi::note_thread_cancelled();
                }
            }
        }
        drop(unsafe { Box::from_raw(self.0.as_ptr()) });
    }

    /// Asks the thread and its job to stop.
    pub(crate) fn stop(self) {
        self.thread().stop();
    }

    /// Whether a stop was requested.
    pub(crate) fn is_stopped(self) -> bool {
        self.thread().is_stopped()
    }

    /// Sets or clears only the stop flag, without telling the job.
    pub(crate) fn set_stopped(self, stopped: bool) {
        self.thread().stopped.store(stopped, Ordering::SeqCst);
    }

    /// Whether the thread is running.
    pub(crate) fn is_running(self) -> bool {
        self.thread().running.load(Ordering::SeqCst)
    }

    /// Hands the par file `file` (`deleted` when it was deleted) to a running
    /// repair job. False for other jobs or when the repair did not take it.
    pub(crate) fn add_par_file(self, file: FileRef, deleted: bool) -> bool {
        match &self.thread().work {
            Some(Work::Post(job)) => {
                let _cancel_guard = unsafe { crate::ffi::PosixCancelGuard::disable() };
                job.add_par(file, deleted)
            }
            _ => false,
        }
    }

    fn with_unpack(self, f: impl FnOnce(&NzbgetRsDirectUnpack)) {
        if let Some(du) = self.thread().unpack() {
            let _cancel_guard = unsafe { crate::ffi::PosixCancelGuard::disable() };
            f(du);
        }
    }

    /// Stops a direct unpack of the collection `nzb`.
    pub(crate) fn unpack_stop(self, nzb: NzbRef) {
        self.with_unpack(|du| du.stop_active(nzb));
    }

    /// Tells a direct unpack that `file` finished downloading.
    pub(crate) fn unpack_file_downloaded(self, file: FileRef) {
        self.with_unpack(|du| du.file_downloaded_active(file));
    }

    /// Tells a direct unpack that the collection `nzb` finished downloading.
    pub(crate) fn unpack_nzb_downloaded(self, nzb: NzbRef) {
        self.with_unpack(|du| du.nzb_downloaded_active(nzb));
    }

    /// Tells a direct unpack that the collection `nzb` was deleted.
    pub(crate) fn unpack_nzb_deleted(self, nzb: NzbRef) {
        self.with_unpack(|du| du.nzb_deleted_active(nzb));
    }

    /// Whether a direct unpack is unpacking.
    pub(crate) fn unpack_is_unpacking(self) -> bool {
        self.thread().unpack().is_some_and(|du| du.is_unpacking())
    }

    /// Whether a direct unpack waits for its next volume.
    pub(crate) fn unpack_is_waiting_volume(self) -> bool {
        self.thread().unpack().is_some_and(|du| du.is_waiting_volume())
    }
}

// Old raw-pointer entry points, kept as thin wrappers over [`JobHandle`]
// until the queue model's call sites switch to it.

fn handle(t: *const JobThread) -> Option<JobHandle> {
    // SAFETY: the callers' contracts require `t` to be null or live.
    unsafe { JobHandle::from_addr(t as usize) }
}

/// # Safety
/// `post_ptr` is null or a live `PostInfo`; ownership of the returned thread is
/// the caller's until it is started with auto-destroy.
pub unsafe fn nzbget_rs_job_new_post(
    kind: c_int,
    post_ptr: usize,
    nzb_id: c_int,
    auto_destroy: c_int,
) -> *mut JobThread {
    JobHandle::new_post(kind, PostRef::from_addr(post_ptr), nzb_id, auto_destroy != 0).addr()
        as *mut JobThread
}

pub fn nzbget_rs_job_new_unpack(nzb_id: c_int) -> *mut JobThread {
    JobHandle::new_unpack(nzb_id).addr() as *mut JobThread
}

/// # Safety
/// `t` comes from `nzbget_rs_job_new_*` and has not been started.
pub unsafe fn nzbget_rs_job_start(t: *mut JobThread) -> c_int {
    c_int::from(handle(t).is_some_and(JobHandle::start))
}

/// Ops: 0 stop, 1 is stopped, 2/3 set/clear the stop flag, 4 is running,
/// 5 add par (`p1` file, `a1` deleted), 11-14 direct unpack with `p2`
/// (stop, file downloaded, nzb downloaded, nzb deleted), 15 is unpacking,
/// 16 is waiting volume.
///
/// # Safety
/// `t` is null or a live job thread.
pub unsafe fn nzbget_rs_job_op(
    t: *const JobThread,
    op: c_int,
    a1: c_int,
    p1: usize,
    p2: usize,
) -> c_int {
    let Some(h) = handle(t) else {
        return 0;
    };
    match op {
        0 => h.stop(),
        1 => return c_int::from(h.is_stopped()),
        2 => h.set_stopped(true),
        3 => h.set_stopped(false),
        4 => return c_int::from(h.is_running()),
        5 => return c_int::from(h.add_par_file(FileRef::from_addr(p1), a1 != 0)),
        11 => h.unpack_stop(NzbRef::from_addr(p2)),
        12 => h.unpack_file_downloaded(FileRef::from_addr(p2)),
        13 => h.unpack_nzb_downloaded(NzbRef::from_addr(p2)),
        14 => h.unpack_nzb_deleted(NzbRef::from_addr(p2)),
        15 => return c_int::from(h.unpack_is_unpacking()),
        16 => return c_int::from(h.unpack_is_waiting_volume()),
        _ => {}
    }
    0
}

/// # Safety
/// `t` is null or a job thread no one else frees and no running thread uses.
pub unsafe fn nzbget_rs_job_free(t: *mut JobThread, kill: c_int) {
    if let Some(h) = handle(t) {
        unsafe { h.free(kill != 0) };
    }
}

/// The daemon's pre/post-processor and the thread running its loop
/// (was the C++ `PrePostProcessor` thread).
pub struct PrePostRunner {
    // boxed so its address, the processor's owner, stays put
    stopped: Box<AtomicBool>,
    running: AtomicBool,
    pp: Arc<crate::prepost::NzbgetRsPrePostProcessor>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

static ACTIVE_RUNNER: Mutex<Option<Arc<PrePostRunner>>> = Mutex::new(None);

pub(crate) fn active_runner() -> Option<Arc<PrePostRunner>> {
    ACTIVE_RUNNER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Whether the runner owning `owner` (a [`PrePostRunner`] stop flag) stopped.
pub(crate) fn owner_stopped(owner: usize) -> bool {
    owner != 0 && unsafe { &*(owner as *const AtomicBool) }.load(Ordering::SeqCst)
}

impl PrePostRunner {
    fn exec(&self, op: c_int, a1: c_int, p1: usize, p2: usize, p3: usize) -> usize {
        let Some(qh) = crate::ffi::queue_host() else {
            return 0;
        };
        let _cancel_guard = unsafe { crate::ffi::PosixCancelGuard::disable() };
        unsafe { self.pp.exec(op, a1, 0, p1, p2, p3, std::ptr::null(), qh) }
    }
}

/// Creates the daemon's pre/post-processor (stopped, not started) and makes it
/// the active one. Returns an opaque handle for the other runner calls.
pub extern "C" fn nzbget_rs_prepost_runner_new() -> *const PrePostRunner {
    let stopped = Box::new(AtomicBool::new(false));
    let owner = (&*stopped as *const AtomicBool) as usize;
    let runner = Arc::new(PrePostRunner {
        stopped,
        running: AtomicBool::new(false),
        pp: Arc::new(crate::prepost::NzbgetRsPrePostProcessor::new(owner)),
        handle: Mutex::new(None),
    });
    crate::ffi::set_active_prepost(&runner.pp);
    *ACTIVE_RUNNER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(&runner));
    Arc::into_raw(runner)
}

/// Runner operations: 0 start, 1 stop, 2 is running, 3 is stopped, 4 has more
/// jobs, 5 has active unpack (`a1` includes waiting volumes), 6 collection
/// downloaded (`p1` queue, `p2` collection), 8 queue change (`a1` action,
/// `p1` queue, `p2` collection, `p3` file).
///
/// # Safety
/// `r` is null or a handle from `nzbget_rs_prepost_runner_new` not yet freed.
pub unsafe extern "C" fn nzbget_rs_prepost_runner_op(
    r: *const PrePostRunner,
    op: c_int,
    a1: c_int,
    p1: usize,
    p2: usize,
    p3: usize,
) -> c_int {
    let Some(runner) = (unsafe { r.as_ref() }) else {
        return 0;
    };
    match op {
        0 => {
            runner.stopped.store(false, Ordering::SeqCst);
            runner.running.store(true, Ordering::SeqCst);
            let addr = r as usize;
            let spawned = std::thread::Builder::new()
                .name("prepost".into())
                .spawn(move || {
                    let runner = unsafe { &*(addr as *const PrePostRunner) };
                    {
                        let _tg = crate::ffi::RustThreadGuard::enter(&runner.stopped);
                        runner.exec(0, 0, 0, 0, 0);
                    }
                    runner.running.store(false, Ordering::SeqCst);
                });
            match spawned {
                Ok(h) => *runner.handle.lock().unwrap_or_else(|e| e.into_inner()) = Some(h),
                Err(_) => runner.running.store(false, Ordering::SeqCst),
            }
            0
        }
        1 => {
            runner.stopped.store(true, Ordering::SeqCst);
            runner.exec(1, 0, 0, 0, 0);
            0
        }
        2 => c_int::from(runner.running.load(Ordering::SeqCst)),
        3 => c_int::from(runner.stopped.load(Ordering::SeqCst)),
        4 => c_int::from(runner.exec(2, 0, 0, 0, 0) != 0),
        5 => c_int::from(runner.exec(3, a1, 0, 0, 0) != 0),
        6 => {
            runner.exec(6, 0, p1, p2, 0);
            0
        }
        8 => {
            runner.exec(8, a1, p1, p2, p3);
            0
        }
        _ => 0,
    }
}

/// Releases the runner. Its thread must have finished (see op 2).
///
/// # Safety
/// `r` is null or a handle from `nzbget_rs_prepost_runner_new`, freed once.
pub unsafe extern "C" fn nzbget_rs_prepost_runner_free(r: *const PrePostRunner) {
    if r.is_null() {
        return;
    }
    let runner = unsafe { Arc::from_raw(r) };
    if runner.running.load(Ordering::SeqCst) && !runner.stopped.load(Ordering::SeqCst) {
        runner.stopped.store(true, Ordering::SeqCst);
        runner.exec(1, 0, 0, 0, 0);
    }
    if let Some(h) = runner
        .handle
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    {
        let _ = h.join();
    }
    let mut active = ACTIVE_RUNNER.lock().unwrap_or_else(|e| e.into_inner());
    if active.as_ref().is_some_and(|a| Arc::ptr_eq(a, &runner)) {
        *active = None;
    }
    drop(active);
    crate::ffi::clear_active_prepost(&runner.pp);
}

/// Forwards a download-queue change to the active pre/post-processor
/// (was its `Observer::Update` on the C++ `DownloadQueue`).
pub extern "C" fn nzbget_rs_prepost_queue_changed(
    action: c_int,
    dq: usize,
    nzb: usize,
    file: usize,
) {
    if let Some(runner) = active_runner() {
        runner.exec(8, action, dq, nzb, file);
    }
}
