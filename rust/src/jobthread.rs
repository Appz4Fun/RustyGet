//! Threads for post-processing jobs, direct unpack, live repair and the
//! pre/post-processor loop.
//!
//! These were C++ `Thread` subclasses (`PostJobThread`, `DirectUnpack`,
//! `PrePostProcessor`) whose bodies already called into Rust. The queue model
//! (still C++) keeps an opaque pointer to a [`JobThread`] where it used to keep
//! a `Thread*`, and the host operations that started, stopped or queried those
//! threads now call the C ABI below.

use std::ffi::c_int;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::postproc::{NzbgetRsDirectUnpack, NzbgetRsPostJob};

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
    fn alloc(kind: c_int, auto_destroy: bool) -> *mut JobThread {
        Box::into_raw(Box::new(JobThread {
            stopped: AtomicBool::new(false),
            running: AtomicBool::new(false),
            auto_destroy,
            kind,
            pthread: AtomicUsize::new(0),
            work: None,
        }))
    }

    fn run(&self) {
        let _cancel_guard = unsafe { crate::ffi::PosixCancelGuard::disable() };
        match &self.work {
            Some(Work::Post(job)) => {
                unsafe { job.exec(1, 0, 0) };
            }
            Some(Work::Unpack(du)) => {
                du.exec_active(0, 0, 0);
            }
            None => {}
        }
    }

    /// The stop request a job's own code sees (was `PostJobThread::Stop`,
    /// which also told the running job to stop).
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(Work::Post(job)) = &self.work {
            let _cancel_guard = unsafe { crate::ffi::PosixCancelGuard::disable() };
            unsafe { job.exec(0, 0, 0) };
        }
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
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

/// Starts the thread. It frees the [`JobThread`] itself when it finishes if
/// it was created with `auto_destroy`; otherwise the owner frees it. Returns
/// false when no thread could be created; the caller then still owns `t` and
/// must clear any handle it stored before freeing it.
unsafe fn start(t: *mut JobThread) -> bool {
    let Some(thread) = (unsafe { t.as_ref() }) else {
        return false;
    };
    thread.running.store(true, Ordering::SeqCst);
    let addr = t as usize;
    // Held until the pthread handle is published: a worker that finishes at
    // once and frees itself must not do so before then (C++ Thread::Start's
    // m_threadMutex did the same).
    let start_guard = START_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let spawned = std::thread::Builder::new()
        .name(if matches!(thread.work, Some(Work::Unpack(_))) {
            "direct-unpack".into()
        } else {
            "post-job".into()
        })
        .spawn(move || {
            drop(START_LOCK.lock().unwrap_or_else(|e| e.into_inner()));
            let t = addr as *mut JobThread;
            let thread = unsafe { &*t };
            {
                let _tg = crate::ffi::RustThreadGuard::enter(&thread.stopped);
                thread.run();
            }
            // read before running goes false: an owner waiting for that may free it
            let auto_destroy = thread.auto_destroy;
            thread.running.store(false, Ordering::SeqCst);
            if auto_destroy {
                drop(unsafe { Box::from_raw(t) });
            }
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

static START_LOCK: Mutex<()> = Mutex::new(());

/// A post-processing job thread (live repair when `kind` is 10).
///
/// # Safety
/// `post_ptr` is null or a live `PostInfo`; ownership of the returned thread is
/// the caller's until it is started with auto-destroy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_job_new_post(
    kind: c_int,
    post_ptr: usize,
    nzb_id: c_int,
    auto_destroy: c_int,
) -> *mut JobThread {
    let t = JobThread::alloc(kind, auto_destroy != 0);
    let job = NzbgetRsPostJob::new(kind, t as usize, post_ptr, nzb_id);
    unsafe { (*t).work = Some(Work::Post(Box::new(job))) };
    t
}

/// A direct-unpack thread for the collection `nzb_id`; frees itself when done.
#[unsafe(no_mangle)]
pub extern "C" fn nzbget_rs_job_new_unpack(nzb_id: c_int) -> *mut JobThread {
    let t = JobThread::alloc(-1, true);
    let du = NzbgetRsDirectUnpack::new(nzb_id, t as usize);
    unsafe { (*t).work = Some(Work::Unpack(Box::new(du))) };
    t
}

/// Returns 1 when the thread runs, 0 when it could not be created (the caller
/// then clears its handle and frees `t` with `nzbget_rs_job_free`).
///
/// # Safety
/// `t` comes from `nzbget_rs_job_new_*` and has not been started.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_job_start(t: *mut JobThread) -> c_int {
    c_int::from(unsafe { start(t) })
}

/// Thread operations on a live job thread:
/// 0 stop (the job's own stop), 1 is stopped, 2 set the stop flag only,
/// 3 clear the stop flag, 4 is running, 5 add a par file (`p1` file, `a1`
/// deleted), 10.. direct-unpack operations (`op - 10` as in
/// `NzbgetRsDirectUnpack::exec_active`, with `p1`/`p2`).
///
/// # Safety
/// `t` is null or a live job thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_job_op(
    t: *const JobThread,
    op: c_int,
    a1: c_int,
    p1: usize,
    p2: usize,
) -> c_int {
    let Some(thread) = (unsafe { t.as_ref() }) else {
        return 0;
    };
    match op {
        0 => {
            thread.stop();
            0
        }
        1 => c_int::from(thread.is_stopped()),
        2 => {
            thread.stopped.store(true, Ordering::SeqCst);
            0
        }
        3 => {
            thread.stopped.store(false, Ordering::SeqCst);
            0
        }
        4 => c_int::from(thread.running.load(Ordering::SeqCst)),
        5 => match &thread.work {
            Some(Work::Post(job)) => {
                let _cancel_guard = unsafe { crate::ffi::PosixCancelGuard::disable() };
                unsafe { job.exec(2, a1, p1) }
            }
            _ => 0,
        },
        n if n >= 10 => match &thread.work {
            Some(Work::Unpack(du)) => {
                let _cancel_guard = unsafe { crate::ffi::PosixCancelGuard::disable() };
                du.exec_active(n - 10, p1, p2)
            }
            _ => 0,
        },
        _ => 0,
    }
}

/// Frees a job thread that is not auto-destroying, or one whose start failed.
/// With `kill`, the thread is cancelled first (was `Thread::Kill`).
///
/// # Safety
/// `t` is null or a job thread no one else frees and no running thread uses.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_job_free(t: *mut JobThread, kill: c_int) {
    if t.is_null() {
        return;
    }
    if kill != 0 {
        let thread = unsafe { &*t };
        thread.stopped.store(true, Ordering::SeqCst);
        let pt = thread.pthread.load(Ordering::SeqCst);
        if pt != 0 && thread.running.load(Ordering::SeqCst) {
            let cancelled = unsafe { libc::pthread_cancel(pt as libc::pthread_t) } == 0;
            if cancelled {
                crate::ffi::nzbget_rs_thread_count_op(-1);
            }
        }
    }
    drop(unsafe { Box::from_raw(t) });
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
        let (ctx, cb) = (qh.ctx, qh.cb);
        let _cancel_guard = unsafe { crate::ffi::PosixCancelGuard::disable() };
        unsafe {
            self.pp
                .exec(op, a1, 0, p1, p2, p3, std::ptr::null(), ctx, cb)
        }
    }
}

/// Creates the daemon's pre/post-processor (stopped, not started) and makes it
/// the active one. Returns an opaque handle for the other runner calls.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
