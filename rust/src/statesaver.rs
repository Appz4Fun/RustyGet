//! Writes the queue's state files (queue, progress, partial states `<id>s`, ...) on a
//! background thread, `queue-saver`, so the queue coordinator never waits for a disk
//! sync. On 6790 a post-processing move kept the queue disk busy and each fdatasync
//! of the once-a-second saves took 0.4-3.5 s; the coordinator held the queue lock and
//! handed out no articles meanwhile (108 MB/s fell to 4-20 MB/s).
//!
//! The content is still serialized by the caller, under the queue lock, so each file is
//! a consistent snapshot; only the write, the sync, the rename and the directory sync
//! move here. While the coordinator runs (its [`AsyncSaves`] scope) the coordinator's
//! saves are queued and it never waits for them. Another thread's save made under the
//! queue lock is queued too, and that thread waits for it once it releases the lock
//! (`queue_lock_released`): the coordinator never waits for that lock during a sync, and
//! the save is still on disk when the thread's caller (an API edit, an added NZB) gets
//! its answer. Other saves, and all saves before and after the coordinator runs, run
//! inline as before, after the jobs queued for the same file.
//! Queued jobs run first in, first out; a newer write of a file still queued
//! replaces its content in place (latest content, first position: partial states, then
//! progress, then queue, as they were saved). A delete goes to the back, behind the
//! writes it follows. A reader gets the content of a queued write from memory
//! ([`Saver::pending_content`]) and otherwise reads the file once no job touches it
//! ([`settled`]), so it sees the latest content without waiting for a sync.
//!
//! Across files there is no crash-time ordering beyond that: after a crash the disk
//! can hold a newer `progress` next to an older `queue`. The loader never relied on
//! more (a partial state could already be newer than `queue`, `files` is checked
//! against the queued file count, `ids` is a floor written inline). A delete that
//! must follow a write of another file (`progress` after `queue`) waits for it
//! ([`JobKind::Delete`] `after`) and is dropped if that write never succeeds.
//!
//! A failed save (6790: the queue disk full, ENOSPC) keeps the last good file (the
//! writer never renames a write that failed) and is retried with backoff
//! ([`Retry`]): it stays queued, a newer save of the file takes its place and its
//! backoff, and nobody waits for it meanwhile (a full disk must not stall the API or
//! the coordinator). At shutdown [`finish`] tries each failing save once more and
//! names the ones that still fail: the stop is then not clean.

use crate::diskstate_format::DiskLogLevel;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// The first retry of a failed save comes after this; each further one doubles it, up
/// to [`RETRY_MAX`].
const RETRY_BASE: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(30);

/// How often a save of this file failed in a row, and when it is tried again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Retry {
    pub attempts: u32,
    pub not_before: Option<Instant>,
}

impl Retry {
    /// The state after one more failed attempt at `now`.
    fn failed(&self, now: Instant) -> Self {
        let attempts = self.attempts.saturating_add(1);
        let shift = (attempts - 1).min(16);
        let wait = RETRY_BASE
            .checked_mul(1u32 << shift)
            .map_or(RETRY_MAX, |d| d.min(RETRY_MAX));
        Self {
            attempts,
            not_before: Some(now + wait),
        }
    }
}

/// What a job does to its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JobKind {
    /// Write `bytes`; `transactional` writes `temp` and renames it over the file,
    /// `sync` (FlushQueue) syncs the data before the rename and the directory after.
    Write {
        bytes: Vec<u8>,
        transactional: bool,
        sync: bool,
    },
    /// Delete the file, and `temp` too when `temp_too`. With `after`, only once no
    /// job for that file is queued or running: `progress` is deleted after the `queue`
    /// save that makes it obsolete has reached the disk, never while it fails.
    Delete {
        temp_too: bool,
        after: Option<Vec<u8>>,
    },
}

/// One state file to write or delete. `path` identifies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Job {
    /// Order of submission, set by the saver; a write coalesced into a queued one keeps
    /// the older number, so a thread waiting for the older save waits for the newer.
    pub seq: u64,
    pub path: Vec<u8>,
    pub temp: Vec<u8>,
    pub sep: u8,
    pub kind: JobKind,
    /// Failed attempts so far; set by the saver.
    pub retry: Retry,
}

pub(crate) type Log<'a> = &'a mut dyn FnMut(DiskLogLevel, &[u8]);
type Exec = dyn Fn(&Job, Log<'_>) -> bool + Send + Sync;

#[derive(Default)]
struct State {
    queue: VecDeque<Job>,
    /// Files whose job runs right now, on the saver thread or inline in a caller.
    in_flight: Vec<(Vec<u8>, u64)>,
    thread_started: bool,
    next_seq: u64,
    /// Saves are queued (the coordinator runs), else run inline.
    queue_saves: bool,
    /// Files whose save was given up ([`Saver::drain`] after its last attempt failed).
    unsaved: Vec<Vec<u8>>,
}

impl State {
    /// A queued job nobody waits for: it failed before (it is retried later), or it
    /// is a delete waiting for a write of another file.
    fn deferred(&self, job: &Job) -> bool {
        job.retry.attempts > 0 || self.blocked(job)
    }

    /// A delete whose `after` file still has a job queued or running.
    fn blocked(&self, job: &Job) -> bool {
        match &job.kind {
            JobKind::Delete {
                after: Some(after), ..
            } => {
                self.queue.iter().any(|j| j.path == *after)
                    || self.in_flight.iter().any(|(p, _)| p == after)
                    || self.unsaved.contains(after)
            }
            _ => false,
        }
    }
}

pub(crate) struct Saver {
    state: Mutex<State>,
    cv: Condvar,
    exec: Box<Exec>,
}

thread_local! {
    /// This thread is the coordinator: its saves are queued and never waited for.
    static COORDINATOR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// How deep this thread holds the queue lock (`qqueue::CurrentQueueLock`).
    static QUEUE_LOCK_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Saves this thread queued under the queue lock: (file, seq), waited for on release.
    static TICKETS: std::cell::RefCell<Vec<(Vec<u8>, u64)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// While alive (the coordinator's loop), saves are queued for the saver thread; on drop,
/// saves run inline again and the queued ones are drained.
pub(crate) struct AsyncSaves(&'static Arc<Saver>);

impl AsyncSaves {
    pub(crate) fn enter() -> Self {
        let saver: &'static Arc<Saver> = global();
        saver.lock().queue_saves = true;
        COORDINATOR.with(|c| c.set(true));
        Self(saver)
    }
}

impl Drop for AsyncSaves {
    fn drop(&mut self) {
        COORDINATOR.with(|c| c.set(false));
        self.0.lock().queue_saves = false;
        self.0.drain();
    }
}

/// `qqueue::CurrentQueueLock` took the queue lock.
pub(crate) fn queue_lock_acquired() {
    QUEUE_LOCK_DEPTH.with(|d| d.set(d.get() + 1));
}

/// `qqueue::CurrentQueueLock` released the queue lock: once this thread holds it no more,
/// it waits for the saves it queued meanwhile, so they are on disk when it goes on.
pub(crate) fn queue_lock_released() {
    let depth = QUEUE_LOCK_DEPTH.with(|d| {
        let v = d.get().saturating_sub(1);
        d.set(v);
        v
    });
    if depth == 0 {
        let tickets = TICKETS.with(|t| std::mem::take(&mut *t.borrow_mut()));
        if !tickets.is_empty() {
            global().wait_for(&tickets);
        }
    }
}

fn active_log(level: DiskLogLevel, msg: &[u8]) {
    crate::log::log_active_bytes(level.kind() as i32, msg);
}

fn saved_again_message(job: &Job) -> Vec<u8> {
    [
        &b"Saved diskstate file "[..],
        &job.path,
        format!(" after {} failed attempts", job.retry.attempts).as_bytes(),
    ]
    .concat()
}

impl Saver {
    pub(crate) fn new(exec: Box<Exec>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            cv: Condvar::new(),
            exec,
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wait<'a>(&self, st: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        self.cv.wait(st).unwrap_or_else(|e| e.into_inner())
    }

    /// Queues `job` while saves are queued and this thread is the coordinator (true),
    /// or holds the queue lock (true; waited for on release, `queue_lock_released`);
    /// else runs it now and returns its result.
    pub(crate) fn submit(self: &Arc<Self>, mut job: Job, log: Log<'_>) -> bool {
        let mut st = self.lock();
        let coordinator = COORDINATOR.with(|c| c.get());
        let under_lock = QUEUE_LOCK_DEPTH.with(|d| d.get() > 0);
        // A delete that waits for another file's write is queued behind it.
        if (st.queue_saves && (coordinator || under_lock)) || st.blocked(&job) {
            let path = job.path.clone();
            let seq = self.enqueue_locked(st, job);
            if !coordinator {
                TICKETS.with(|t| t.borrow_mut().push((path, seq)));
            }
            true
        } else {
            // Taken off the queue and marked running in one hold (see `idle`).
            st = self.idle(st, &job.path);
            st.next_seq += 1;
            job.seq = st.next_seq;
            // This newer content (or delete) replaces a queued one, and whoever waits
            // for that one waits for this; a failing one's count goes on.
            if let Some(i) = st.queue.iter().position(|j| j.path == job.path) {
                let old = st.queue.remove(i).expect("position is in range");
                job.seq = old.seq;
                job.retry.attempts = old.retry.attempts;
            }
            self.run_marked(st, job, log)
        }
    }

    /// Queues `job` for the saver thread, coalescing with a queued job for its file.
    #[cfg(test)]
    fn enqueue(self: &Arc<Self>, job: Job) -> u64 {
        let st = self.lock();
        self.enqueue_locked(st, job)
    }

    /// Queues `job`; returns the seq a waiter for it waits for.
    fn enqueue_locked(self: &Arc<Self>, mut st: MutexGuard<'_, State>, mut job: Job) -> u64 {
        st.next_seq += 1;
        job.seq = st.next_seq;
        let seq = match st.queue.iter().position(|j| j.path == job.path) {
            Some(i)
                if matches!(st.queue[i].kind, JobKind::Write { .. })
                    && matches!(job.kind, JobKind::Write { .. }) =>
            {
                // Latest content, first position, and a failing save's backoff.
                job.seq = st.queue[i].seq;
                job.retry = st.queue[i].retry.clone();
                st.queue[i] = job;
                st.queue[i].seq
            }
            Some(i) => {
                st.queue.remove(i);
                st.queue.push_back(job);
                st.next_seq
            }
            None => {
                st.queue.push_back(job);
                st.next_seq
            }
        };
        self.start_worker(st);
        seq
    }

    /// Starts the saver thread if it is not running yet, and wakes it.
    fn start_worker(self: &Arc<Self>, mut st: MutexGuard<'_, State>) {
        if !st.thread_started {
            let me = Arc::clone(self);
            if std::thread::Builder::new()
                .name("queue-saver".into())
                .spawn(move || me.worker())
                .is_ok()
            {
                st.thread_started = true;
            } else {
                // No thread: run the queue here, as before.
                drop(st);
                self.run_queue_inline();
                return;
            }
        }
        self.cv.notify_all();
    }

    /// Runs `job`, marked running by the caller. A failed save is queued again for a
    /// retry with backoff (the saver thread runs it), unless a newer save of the file
    /// is queued already: that one takes over the backoff.
    fn exec_job(self: &Arc<Self>, job: Job, log: Log<'_>) -> bool {
        let ok = (self.exec)(&job, log);
        if ok {
            if job.retry.attempts > 0 {
                log(DiskLogLevel::Info, &saved_again_message(&job));
            }
            let mut st = self.lock();
            st.unsaved.retain(|p| *p != job.path);
            return true;
        }
        let retry = job.retry.failed(Instant::now());
        let mut st = self.lock();
        if let Some(newer) = st.queue.iter_mut().find(|j| j.path == job.path) {
            if newer.retry.attempts < retry.attempts {
                newer.retry = retry;
            }
        } else {
            st.queue.push_back(Job { retry, ..job });
        }
        self.start_worker(st);
        false
    }

    /// Waits until none of `tickets` (file, seq) is queued or running, but for a save
    /// that failed (retried later) or a delete waiting for another file's write.
    fn wait_for(&self, tickets: &[(Vec<u8>, u64)]) {
        let mut st = self.lock();
        loop {
            let busy = tickets.iter().any(|(p, s)| {
                st.queue
                    .iter()
                    .any(|j| j.path == *p && j.seq <= *s && !st.deferred(j))
                    || st.in_flight.iter().any(|(fp, fs)| fp == p && fs <= s)
            });
            if !busy {
                return;
            }
            st = self.wait(st);
        }
    }

    /// Waits until no job runs for `path`. A caller that takes a job of `path` off the
    /// queue does so after this, in the same hold as `mark`, so the job is never out of
    /// both the queue and the running list (where `wait_for` could miss it).
    fn idle<'a>(&self, mut st: MutexGuard<'a, State>, path: &[u8]) -> MutexGuard<'a, State> {
        while st.in_flight.iter().any(|(p, _)| p == path) {
            st = self.wait(st);
        }
        st
    }

    /// Waits for a job running for the same file, then runs `job` in this thread.
    fn run_marked(self: &Arc<Self>, st: MutexGuard<'_, State>, job: Job, log: Log<'_>) -> bool {
        let path = job.path.clone();
        let _mark = self.mark(st, &path, job.seq);
        self.exec_job(job, log)
    }

    /// Waits until no job runs for `path`, then marks it running (as save `seq`; a
    /// reader marks with `u64::MAX`, which no waiter waits for) until the guard drops.
    fn mark<'a>(&'a self, mut st: MutexGuard<'_, State>, path: &'a [u8], seq: u64) -> Mark<'a> {
        while st.in_flight.iter().any(|(p, _)| p == path) {
            st = self.wait(st);
        }
        st.in_flight.push((path.to_vec(), seq));
        Mark { saver: self, path }
    }

    /// The first queued job that may run now: its file is not running, its retry is
    /// due, and a delete's `after` write is done.
    fn next_job(&self, st: &mut State, now: Instant) -> Option<Job> {
        let i = st.queue.iter().position(|j| {
            !st.in_flight.iter().any(|(p, _)| *p == j.path)
                && j.retry.not_before.is_none_or(|t| t <= now)
                && !st.blocked(j)
        })?;
        st.queue.remove(i)
    }

    fn worker(self: Arc<Self>) {
        loop {
            let mut st = self.lock();
            let job = loop {
                if let Some(job) = self.next_job(&mut st, Instant::now()) {
                    break job;
                }
                // Asleep until a job comes, or the next retry is due.
                st = match st.queue.iter().filter_map(|j| j.retry.not_before).min() {
                    Some(due) => {
                        let wait = due.saturating_duration_since(Instant::now());
                        self.cv
                            .wait_timeout(st, wait.max(Duration::from_millis(1)))
                            .unwrap_or_else(|e| e.into_inner())
                            .0
                    }
                    None => self.wait(st),
                };
            };
            // A panicking job must not kill the thread: later jobs and `drain` wait for it.
            let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.run_marked(st, job, &mut active_log)
            }));
            if ran.is_err() {
                active_log(
                    DiskLogLevel::Error,
                    b"Error saving diskstate: a save panicked",
                );
            }
        }
    }

    /// Runs the queued jobs due now in this thread (no saver thread could start). A
    /// failed one stays queued for the next call.
    fn run_queue_inline(self: &Arc<Self>) {
        loop {
            let mut st = self.lock();
            let Some(job) = self.next_job(&mut st, Instant::now()) else {
                return;
            };
            let path = job.path.clone();
            let _mark = self.mark(st, &path, job.seq);
            if !(self.exec)(&job, &mut active_log) {
                let retry = job.retry.failed(Instant::now());
                self.lock().queue.push_back(Job { retry, ..job });
                return;
            }
        }
    }

    /// Makes the latest content of `path` reach the disk before a caller reads,
    /// stats or deletes it: runs its queued job now, in this thread, or waits for
    /// the one running.
    pub(crate) fn settle(self: &Arc<Self>, path: &[u8]) {
        self.settled(path, || ());
    }

    /// [`Saver::settle`], then runs `f` while no job can touch `path`: a reader that
    /// checks the file and its `.new` never sees the moment of a rename. A delete that
    /// still waits for another file's write stays queued (the file stays meanwhile).
    pub(crate) fn settled<R>(self: &Arc<Self>, path: &[u8], f: impl FnOnce() -> R) -> R {
        let mut st = self.idle(self.lock(), path);
        let pending = st
            .queue
            .iter()
            .position(|j| j.path == path && !st.blocked(j))
            .and_then(|i| st.queue.remove(i));
        let seq = pending.as_ref().map_or(u64::MAX, |j| j.seq);
        let _mark = self.mark(st, path, seq);
        if let Some(job) = pending {
            self.exec_job(job, &mut active_log);
        }
        f()
    }

    /// The content of a queued write of `path`, for a reader: the file on disk is older.
    /// A queued delete runs now (no sync), so the reader finds the file gone, unless it
    /// waits for another file's write.
    pub(crate) fn pending_content(self: &Arc<Self>, path: &[u8]) -> Option<Vec<u8>> {
        let mut st = self.lock();
        let i = st.queue.iter().position(|j| j.path == path)?;
        if let JobKind::Write { bytes, .. } = &st.queue[i].kind {
            return Some(bytes.clone());
        }
        st = self.idle(st, path);
        let i = st.queue.iter().position(|j| j.path == path)?;
        if let JobKind::Write { bytes, .. } = &st.queue[i].kind {
            return Some(bytes.clone());
        }
        if st.blocked(&st.queue[i]) {
            return None;
        }
        let job = st.queue.remove(i).expect("position is in range");
        self.run_marked(st, job, &mut active_log);
        None
    }

    /// Waits until every queued job has run (the coordinator's exit, the stop). A save
    /// that failed and waits for its retry gets a last attempt now; one that fails
    /// again is given up, the last good file staying ([`Saver::unsaved`] names it), and
    /// a delete waiting for its write is dropped. Returns how many files are unsaved.
    pub(crate) fn drain(self: &Arc<Self>) -> usize {
        let mut st = self.lock();
        loop {
            while !st.in_flight.is_empty()
                || st
                    .queue
                    .iter()
                    .any(|j| j.retry.attempts == 0 && !st.blocked(j))
            {
                st = self.wait(st);
            }
            let mut failed = Vec::new();
            let mut i = 0;
            while i < st.queue.len() {
                if st.queue[i].retry.attempts > 0 {
                    failed.extend(st.queue.remove(i));
                } else {
                    i += 1;
                }
            }
            if failed.is_empty() {
                // Left: deletes waiting for a write that was given up.
                st.queue.clear();
                self.cv.notify_all();
                return st.unsaved.len();
            }
            for job in failed {
                let ok = {
                    let _mark = self.mark(st, &job.path, job.seq);
                    (self.exec)(&job, &mut active_log)
                };
                st = self.lock();
                if ok {
                    active_log(DiskLogLevel::Info, &saved_again_message(&job));
                    st.unsaved.retain(|p| *p != job.path);
                } else if !st.unsaved.contains(&job.path) {
                    active_log(
                        DiskLogLevel::Error,
                        &[
                            &b"Error saving diskstate: giving up on "[..],
                            &job.path,
                            b"; the last good copy on disk is kept",
                        ]
                        .concat(),
                    );
                    st.unsaved.push(job.path);
                }
            }
        }
    }

    /// The files whose save [`Saver::drain`] gave up and no later save wrote.
    pub(crate) fn unsaved(&self) -> Vec<Vec<u8>> {
        self.lock().unsaved.clone()
    }

    #[cfg(test)]
    fn queued_paths(&self) -> Vec<Vec<u8>> {
        self.lock().queue.iter().map(|j| j.path.clone()).collect()
    }
}

/// A file marked running; unmarks it and wakes the waiters on drop.
struct Mark<'a> {
    saver: &'a Saver,
    path: &'a [u8],
}

impl Drop for Mark<'_> {
    fn drop(&mut self) {
        let mut st = self.saver.lock();
        if let Some(i) = st.in_flight.iter().position(|(p, _)| p == self.path) {
            st.in_flight.swap_remove(i);
        }
        self.saver.cv.notify_all();
    }
}

/// The daemon's saver, writing with `diskstate_format::run_state_job`.
pub(crate) fn global() -> &'static Arc<Saver> {
    static SAVER: OnceLock<Arc<Saver>> = OnceLock::new();
    SAVER.get_or_init(|| Saver::new(Box::new(crate::diskstate_format::run_state_job)))
}

/// [`Saver::submit`] on the daemon's saver.
pub(crate) fn submit(job: Job, log: Log<'_>) -> bool {
    global().submit(job, log)
}

/// [`Saver::settle`] on the daemon's saver.
pub(crate) fn settle(path: &[u8]) {
    global().settle(path)
}

/// [`Saver::settled`] on the daemon's saver.
pub(crate) fn settled<R>(path: &[u8], f: impl FnOnce() -> R) -> R {
    global().settled(path, f)
}

/// [`Saver::pending_content`] on the daemon's saver.
pub(crate) fn pending_content(path: &[u8]) -> Option<Vec<u8>> {
    global().pending_content(path)
}

/// [`Saver::drain`] on the daemon's saver.
pub(crate) fn drain() -> usize {
    global().drain()
}

/// The daemon stops: every queued save runs (a failing one gets its last attempt), and
/// the files whose latest state could not be saved are returned. Not empty, the stop
/// is not clean: their last good copies on disk are older than the queue in memory.
pub(crate) fn finish() -> Vec<Vec<u8>> {
    let saver = global();
    saver.drain();
    saver.unsaved()
}

/// Set once a stop (or reload) left state files unsaved; the process exit status says so.
static STOPPED_UNCLEAN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// At the instance's teardown: [`finish`], and an error naming the unsaved files, if any.
pub(crate) fn stop_report() {
    let unsaved = finish();
    if unsaved.is_empty() {
        return;
    }
    STOPPED_UNCLEAN.store(true, std::sync::atomic::Ordering::SeqCst);
    let mut msg = format!(
        "Stopping is not clean: {} state file(s) could not be saved, their last good copies \
         are older than the queue in memory:",
        unsaved.len()
    )
    .into_bytes();
    for p in unsaved.iter().take(10) {
        msg.push(b' ');
        msg.extend_from_slice(crate::nserv::base_file_name(p));
    }
    active_log(DiskLogLevel::Error, &msg);
}

/// True once a teardown left state files unsaved ([`stop_report`]).
pub(crate) fn stopped_unclean() -> bool {
    STOPPED_UNCLEAN.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A saver that records the jobs it runs and holds the job of file `hold` while `gate`
    /// is set.
    fn recording() -> (Arc<Saver>, Arc<Mutex<Vec<Job>>>, Arc<AtomicBool>) {
        let done = Arc::new(Mutex::new(Vec::new()));
        let gate = Arc::new(AtomicBool::new(false));
        let (d, g) = (Arc::clone(&done), Arc::clone(&gate));
        let saver = Saver::new(Box::new(move |job: &Job, _log: Log<'_>| {
            while job.path == b"hold" && g.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            d.lock().unwrap().push(job.clone());
            true
        }));
        (saver, done, gate)
    }

    fn write(path: &str, content: &str) -> Job {
        Job {
            seq: 0,
            path: path.as_bytes().to_vec(),
            temp: format!("{path}.new").into_bytes(),
            sep: b'/',
            kind: JobKind::Write {
                bytes: content.as_bytes().to_vec(),
                transactional: true,
                sync: true,
            },
            retry: Retry::default(),
        }
    }

    fn delete(path: &str) -> Job {
        Job {
            kind: JobKind::Delete {
                temp_too: true,
                after: None,
            },
            ..write(path, "")
        }
    }

    fn summary(done: &Mutex<Vec<Job>>) -> Vec<String> {
        done.lock()
            .unwrap()
            .iter()
            .map(|j| match &j.kind {
                JobKind::Write { bytes, .. } => format!(
                    "{}={}",
                    String::from_utf8_lossy(&j.path),
                    String::from_utf8_lossy(bytes)
                ),
                JobKind::Delete { .. } => format!("-{}", String::from_utf8_lossy(&j.path)),
            })
            .collect()
    }

    /// Holds the worker on a first job so the next ones stay queued.
    fn hold(saver: &Arc<Saver>, gate: &AtomicBool) {
        gate.store(true, Ordering::SeqCst);
        saver.enqueue(write("hold", "0"));
        while !saver.queued_paths().is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn runs_in_order_and_coalesces_writes_in_first_position() {
        let (saver, done, gate) = recording();
        hold(&saver, &gate);
        saver.enqueue(write("1s", "a"));
        saver.enqueue(write("progress", "p1"));
        saver.enqueue(write("queue", "q1"));
        saver.enqueue(write("1s", "b"));
        saver.enqueue(write("progress", "p2"));
        assert_eq!(saver.queued_paths().len(), 3);
        gate.store(false, Ordering::SeqCst);
        saver.drain();
        assert_eq!(
            summary(&done),
            ["hold=0", "1s=b", "progress=p2", "queue=q1"]
        );
    }

    #[test]
    fn a_delete_goes_behind_the_writes_it_follows() {
        let (saver, done, gate) = recording();
        hold(&saver, &gate);
        saver.enqueue(write("progress", "p1"));
        saver.enqueue(write("queue", "q1"));
        saver.enqueue(delete("progress"));
        saver.enqueue(write("2s", "x"));
        gate.store(false, Ordering::SeqCst);
        saver.drain();
        assert_eq!(summary(&done), ["hold=0", "queue=q1", "-progress", "2s=x"]);
    }

    #[test]
    fn drain_waits_for_every_queued_job() {
        let (saver, done, gate) = recording();
        hold(&saver, &gate);
        for i in 0..20 {
            saver.enqueue(write(&format!("{i}s"), "c"));
        }
        let releaser = {
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(50));
                gate.store(false, Ordering::SeqCst);
            })
        };
        saver.drain();
        assert_eq!(done.lock().unwrap().len(), 21);
        assert!(saver.queued_paths().is_empty());
        releaser.join().unwrap();
    }

    #[test]
    fn settle_runs_a_queued_write_now_and_waits_for_a_running_one() {
        let (saver, done, gate) = recording();
        hold(&saver, &gate);
        saver.enqueue(write("3s", "pending"));
        saver.enqueue(write("queue", "q"));
        // The worker is held on "hold"; the reader of 3s runs its job itself.
        saver.settle(b"3s");
        assert_eq!(summary(&done), ["3s=pending"]);
        assert_eq!(saver.queued_paths(), [b"queue".to_vec()]);
        // "hold" is running: settling it waits until it is done.
        let releaser = {
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(30));
                gate.store(false, Ordering::SeqCst);
            })
        };
        saver.settle(b"hold");
        assert!(summary(&done).contains(&"hold=0".to_string()));
        releaser.join().unwrap();
        saver.drain();
        assert_eq!(summary(&done), ["3s=pending", "hold=0", "queue=q"]);
    }

    #[test]
    fn saves_queue_only_in_a_scope_and_an_inline_write_supersedes_a_queued_one() {
        let (saver, done, gate) = recording();
        hold(&saver, &gate);
        saver.lock().queue_saves = true;
        // Neither the coordinator nor under the queue lock: inline.
        assert!(saver.submit(write("7s", "inline"), &mut |_, _| {}));
        assert_eq!(summary(&done), ["7s=inline"]);
        done.lock().unwrap().clear();
        COORDINATOR.with(|c| c.set(true));
        assert!(saver.submit(write("4s", "old"), &mut |_, _| {}));
        COORDINATOR.with(|c| c.set(false));
        assert_eq!(saver.queued_paths(), [b"4s".to_vec()]);
        assert_eq!(saver.pending_content(b"4s"), Some(b"old".to_vec()));
        saver.lock().queue_saves = false;
        // Outside the scope: runs now, and the queued older content is dropped.
        assert!(saver.submit(write("4s", "new"), &mut |_, _| {}));
        assert!(saver.queued_paths().is_empty());
        assert_eq!(summary(&done), ["4s=new"]);
        gate.store(false, Ordering::SeqCst);
        saver.drain();
        assert_eq!(summary(&done), ["4s=new", "hold=0"]);
    }

    #[test]
    fn a_reader_of_a_queued_delete_runs_it_now() {
        let (saver, done, gate) = recording();
        hold(&saver, &gate);
        saver.enqueue(delete("6s"));
        assert_eq!(saver.pending_content(b"6s"), None);
        assert_eq!(summary(&done), ["-6s"]);
        assert!(saver.queued_paths().is_empty());
        gate.store(false, Ordering::SeqCst);
        saver.drain();
    }

    #[test]
    fn a_save_queued_under_the_queue_lock_is_waited_for_after_the_lock() {
        let (saver, done, gate) = recording();
        hold(&saver, &gate);
        let first = saver.enqueue(write("queue", "q1"));
        let second = saver.enqueue(write("queue", "q2"));
        assert_eq!(
            first, second,
            "a coalesced write keeps its place and number"
        );
        let releaser = {
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(30));
                gate.store(false, Ordering::SeqCst);
            })
        };
        saver.wait_for(&[(b"queue".to_vec(), first)]);
        assert_eq!(summary(&done), ["hold=0", "queue=q2"]);
        releaser.join().unwrap();
    }

    /// A saver whose writes fail while `full` is set (a full disk); it records the
    /// jobs that succeeded and counts every attempt.
    fn disk_full() -> (
        Arc<Saver>,
        Arc<Mutex<Vec<Job>>>,
        Arc<AtomicBool>,
        Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let done = Arc::new(Mutex::new(Vec::new()));
        let full = Arc::new(AtomicBool::new(true));
        let tries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (d, f, n) = (Arc::clone(&done), Arc::clone(&full), Arc::clone(&tries));
        let saver = Saver::new(Box::new(move |job: &Job, _log: Log<'_>| {
            n.fetch_add(1, Ordering::SeqCst);
            if matches!(job.kind, JobKind::Write { .. }) && f.load(Ordering::SeqCst) {
                return false;
            }
            d.lock().unwrap().push(job.clone());
            true
        }));
        (saver, done, full, tries)
    }

    fn wait_until(what: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !what() {
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_failed_save_is_kept_and_retried_with_backoff_until_it_succeeds() {
        let (saver, done, full, tries) = disk_full();
        assert!(!saver.submit(write("9s", "a"), &mut |_, _| {}));
        // Kept, with its backoff; nobody waits for it.
        let st = saver.lock();
        assert_eq!(st.queue.len(), 1);
        assert_eq!(st.queue[0].retry.attempts, 1);
        assert!(st.queue[0].retry.not_before.is_some());
        let seq = st.queue[0].seq;
        drop(st);
        saver.wait_for(&[(b"9s".to_vec(), seq)]);
        // The first retry comes after about a second, not at once.
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(tries.load(Ordering::SeqCst), 1);
        wait_until(|| tries.load(Ordering::SeqCst) >= 2);
        assert_eq!(saver.lock().queue[0].retry.attempts, 2);
        full.store(false, Ordering::SeqCst);
        wait_until(|| !done.lock().unwrap().is_empty());
        assert_eq!(summary(&done), ["9s=a"]);
        assert_eq!(saver.drain(), 0);
    }

    #[test]
    fn a_newer_save_of_a_failing_file_takes_its_place_and_backoff() {
        let (saver, done, full, _tries) = disk_full();
        assert!(!saver.submit(write("8s", "old"), &mut |_, _| {}));
        saver.lock().queue_saves = true;
        COORDINATOR.with(|c| c.set(true));
        assert!(saver.submit(write("8s", "new"), &mut |_, _| {}));
        COORDINATOR.with(|c| c.set(false));
        {
            let st = saver.lock();
            assert_eq!(st.queue.len(), 1);
            assert_eq!(st.queue[0].retry.attempts, 1, "the backoff goes on");
        }
        assert_eq!(saver.pending_content(b"8s"), Some(b"new".to_vec()));
        full.store(false, Ordering::SeqCst);
        wait_until(|| !done.lock().unwrap().is_empty());
        assert_eq!(summary(&done), ["8s=new"]);
        saver.lock().queue_saves = false;
        saver.drain();
    }

    #[test]
    fn a_delete_after_a_failing_write_waits_for_it_and_is_dropped_if_it_is_given_up() {
        let (saver, done, full, _tries) = disk_full();
        let after_queue = |path: &str| Job {
            kind: JobKind::Delete {
                temp_too: true,
                after: Some(b"queue".to_vec()),
            },
            ..write(path, "")
        };
        // The queue save fails: progress stays.
        assert!(!saver.submit(write("queue", "q1"), &mut |_, _| {}));
        assert!(saver.submit(after_queue("progress"), &mut |_, _| {}));
        assert_eq!(
            saver.queued_paths(),
            [b"queue".to_vec(), b"progress".to_vec()]
        );
        assert_eq!(
            saver.pending_content(b"progress"),
            None,
            "a reader finds the file still there"
        );
        assert_eq!(saver.queued_paths().len(), 2);
        // Given up at the stop: the delete is dropped, progress kept.
        assert_eq!(saver.drain(), 1);
        assert_eq!(saver.unsaved(), [b"queue".to_vec()]);
        assert!(saver.queued_paths().is_empty());
        assert!(summary(&done).is_empty());
        // The next queue save succeeds: then progress goes, after it.
        full.store(false, Ordering::SeqCst);
        assert!(saver.submit(write("queue", "q2"), &mut |_, _| {}));
        assert!(saver.unsaved().is_empty());
        assert!(saver.submit(after_queue("progress"), &mut |_, _| {}));
        assert_eq!(summary(&done), ["queue=q2", "-progress"]);
    }

    #[test]
    fn drain_gives_a_failing_save_a_last_attempt_and_names_it_when_that_fails() {
        let (saver, done, full, tries) = disk_full();
        assert!(!saver.submit(write("5s", "x"), &mut |_, _| {}));
        assert!(!saver.submit(write("6s", "y"), &mut |_, _| {}));
        assert_eq!(saver.drain(), 2);
        assert!(tries.load(Ordering::SeqCst) >= 4);
        assert!(saver.queued_paths().is_empty());
        let mut unsaved = saver.unsaved();
        unsaved.sort();
        assert_eq!(unsaved, [b"5s".to_vec(), b"6s".to_vec()]);
        // Space again: the last attempt succeeds.
        assert!(!saver.submit(write("7s", "z"), &mut |_, _| {}));
        full.store(false, Ordering::SeqCst);
        assert_eq!(saver.drain(), 2);
        assert_eq!(summary(&done), ["7s=z"]);
    }

    /// The real writer on a full disk: the last good file stays whole, no `.new` is left.
    #[test]
    fn a_failed_write_never_replaces_the_last_good_file() {
        use std::os::unix::ffi::OsStrExt;
        let dir = std::env::temp_dir().join(format!(
            "rg-statesaver-full-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("4242s");
        let temp = dir.join("4242s.new");
        std::fs::write(&path, b"last good").unwrap();
        let job = Job {
            seq: 0,
            path: path.as_os_str().as_bytes().to_vec(),
            temp: temp.as_os_str().as_bytes().to_vec(),
            sep: b'/',
            kind: JobKind::Write {
                bytes: b"newer".to_vec(),
                transactional: true,
                sync: true,
            },
            retry: Retry::default(),
        };
        crate::diskstate_format::FAIL_WRITES
            .lock()
            .unwrap()
            .push(b"/4242s".to_vec());
        let mut errors = Vec::new();
        let ok = crate::diskstate_format::run_state_job(&job, &mut |level, msg| {
            if level == DiskLogLevel::Error {
                errors.push(String::from_utf8_lossy(msg).into_owned());
            }
        });
        crate::diskstate_format::FAIL_WRITES
            .lock()
            .unwrap()
            .retain(|n| n != b"/4242s");
        assert!(!ok);
        assert_eq!(std::fs::read(&path).unwrap(), b"last good");
        assert!(!temp.exists());
        assert!(
            errors
                .iter()
                .any(|e| e.contains("Could not write file") && e.contains("4242s.new")),
            "{errors:?}"
        );
        // Space again: the save replaces it.
        assert!(crate::diskstate_format::run_state_job(&job, &mut |_, _| {}));
        assert_eq!(std::fs::read(&path).unwrap(), b"newer");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real job writer: a queued save is on disk, readable, after `settle`,
    /// and its temporary file is gone.
    #[test]
    fn read_after_pending_write_sees_the_latest_content() {
        use std::os::unix::ffi::OsStrExt;
        let dir = std::env::temp_dir().join(format!(
            "rg-statesaver-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let saver = Saver::new(Box::new(crate::diskstate_format::run_state_job));
        let path = dir.join("5s");
        let job = |content: &str| Job {
            seq: 0,
            path: path.as_os_str().as_bytes().to_vec(),
            temp: dir.join("5s.new").as_os_str().as_bytes().to_vec(),
            sep: b'/',
            kind: JobKind::Write {
                bytes: content.as_bytes().to_vec(),
                transactional: true,
                sync: true,
            },
            retry: Retry::default(),
        };
        saver.enqueue(job("first"));
        saver.enqueue(job("second"));
        saver.settle(path.as_os_str().as_bytes());
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert!(!dir.join("5s.new").exists());
        saver.drain();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
