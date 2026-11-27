//! Writes the queue's state files (queue, progress, partial states `<id>s`, ...) on a
//! background thread, `queue-saver`, so the queue coordinator never waits for a disk
//! sync. On 6790 a post-processing move kept the queue disk busy and each fdatasync
//! of the once-a-second saves took 0.4-3.5 s; the coordinator held the queue lock and
//! handed out no articles meanwhile (108 MB/s fell to 4-20 MB/s).
//!
//! The content is still serialized by the caller, under the queue lock, so each file is
//! a consistent snapshot; only the write, the sync, the rename and the directory sync
//! move here. While the coordinator runs (its [`AsyncSaves`] scope) every thread's saves
//! are queued: a save made by another thread under the queue lock would otherwise keep
//! the coordinator waiting for that lock. Before and after (startup, the final saves of
//! a shutdown) saves run inline as before, after the jobs queued for the same file.
//! Queued jobs run first in, first out; a newer write of a file still queued
//! replaces its content in place (latest content, first position: partial states, then
//! progress, then queue, as they were saved). A delete goes to the back, behind the
//! writes it follows. A reader gets the content of a queued write from memory
//! ([`Saver::pending_content`]) and otherwise reads the file once no job touches it
//! ([`settled`]), so it sees the latest content without waiting for a sync.
//!
//! Across files there is no crash-time ordering beyond that: after a crash the disk
//! can hold a newer `progress` next to an older `queue`, and a queue save whose write
//! fails on this thread has already queued the delete of `progress` behind it. The
//! loader never relied on more (a partial state could already be newer than `queue`,
//! `files` is checked against the queued file count, `ids` is a floor written inline).

use crate::diskstate_format::DiskLogLevel;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};

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
    /// Delete the file, and `temp` too when `temp_too`.
    Delete { temp_too: bool },
}

/// One state file to write or delete. `path` identifies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Job {
    pub path: Vec<u8>,
    pub temp: Vec<u8>,
    pub sep: u8,
    pub kind: JobKind,
}

pub(crate) type Log<'a> = &'a mut dyn FnMut(DiskLogLevel, &[u8]);
type Exec = dyn Fn(&Job, Log<'_>) -> bool + Send + Sync;

#[derive(Default)]
struct State {
    queue: VecDeque<Job>,
    /// Files whose job runs right now, on the saver thread or inline in a caller.
    in_flight: Vec<Vec<u8>>,
    thread_started: bool,
    /// Saves are queued (the coordinator runs), else run inline.
    queue_saves: bool,
}

pub(crate) struct Saver {
    state: Mutex<State>,
    cv: Condvar,
    exec: Box<Exec>,
}

/// While alive, every thread's state saves are queued for the saver thread; on drop,
/// saves run inline again and the queued ones are drained.
pub(crate) struct AsyncSaves(&'static Saver);

impl AsyncSaves {
    pub(crate) fn enter() -> Self {
        let saver: &'static Saver = global();
        saver.lock().queue_saves = true;
        Self(saver)
    }
}

impl Drop for AsyncSaves {
    fn drop(&mut self) {
        self.0.lock().queue_saves = false;
        self.0.drain();
    }
}

fn active_log(level: DiskLogLevel, msg: &[u8]) {
    crate::log::log_active_bytes(level.kind() as i32, msg);
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

    /// Queues `job` while saves are queued (true), else runs it now and returns its
    /// result.
    pub(crate) fn submit(self: &Arc<Self>, job: Job, log: Log<'_>) -> bool {
        let mut st = self.lock();
        if st.queue_saves {
            self.enqueue_locked(st, job);
            true
        } else {
            // This newer content (or delete) replaces a queued one.
            st.queue.retain(|j| j.path != job.path);
            self.run_marked(st, job, log)
        }
    }

    /// Queues `job` for the saver thread, coalescing with a queued job for its file.
    #[cfg(test)]
    fn enqueue(self: &Arc<Self>, job: Job) {
        let st = self.lock();
        self.enqueue_locked(st, job);
    }

    fn enqueue_locked(self: &Arc<Self>, mut st: MutexGuard<'_, State>, job: Job) {
        match st.queue.iter().position(|j| j.path == job.path) {
            Some(i)
                if matches!(st.queue[i].kind, JobKind::Write { .. })
                    && matches!(job.kind, JobKind::Write { .. }) =>
            {
                st.queue[i] = job;
            }
            Some(i) => {
                st.queue.remove(i);
                st.queue.push_back(job);
            }
            None => st.queue.push_back(job),
        }
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

    /// Waits for a job running for the same file, then runs `job` in this thread.
    fn run_marked(&self, st: MutexGuard<'_, State>, job: Job, log: Log<'_>) -> bool {
        let _mark = self.mark(st, &job.path);
        (self.exec)(&job, log)
    }

    /// Waits until no job runs for `path`, then marks it running until the guard drops.
    fn mark<'a>(&'a self, mut st: MutexGuard<'_, State>, path: &'a [u8]) -> Mark<'a> {
        while st.in_flight.iter().any(|p| p == path) {
            st = self.wait(st);
        }
        st.in_flight.push(path.to_vec());
        Mark { saver: self, path }
    }

    fn next_job(&self, st: &mut State) -> Option<Job> {
        let i = st
            .queue
            .iter()
            .position(|j| !st.in_flight.contains(&j.path))?;
        st.queue.remove(i)
    }

    fn worker(&self) {
        loop {
            let mut st = self.lock();
            let job = loop {
                if let Some(job) = self.next_job(&mut st) {
                    break job;
                }
                st = self.wait(st);
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

    fn run_queue_inline(&self) {
        loop {
            let mut st = self.lock();
            let Some(job) = self.next_job(&mut st) else {
                return;
            };
            self.run_marked(st, job, &mut active_log);
        }
    }

    /// Makes the latest content of `path` reach the disk before a caller reads,
    /// stats or deletes it: runs its queued job now, in this thread, or waits for
    /// the one running.
    pub(crate) fn settle(&self, path: &[u8]) {
        self.settled(path, || ());
    }

    /// [`Saver::settle`], then runs `f` while no job can touch `path`: a reader that
    /// checks the file and its `.new` never sees the moment of a rename.
    pub(crate) fn settled<R>(&self, path: &[u8], f: impl FnOnce() -> R) -> R {
        let mut st = self.lock();
        let pending = st
            .queue
            .iter()
            .position(|j| j.path == path)
            .and_then(|i| st.queue.remove(i));
        let _mark = self.mark(st, path);
        if let Some(job) = pending {
            (self.exec)(&job, &mut active_log);
        }
        f()
    }

    /// The content of a queued write of `path`, for a reader: the file on disk is older.
    /// A queued delete runs now (no sync), so the reader finds the file gone.
    pub(crate) fn pending_content(&self, path: &[u8]) -> Option<Vec<u8>> {
        let mut st = self.lock();
        let i = st.queue.iter().position(|j| j.path == path)?;
        if let JobKind::Write { bytes, .. } = &st.queue[i].kind {
            return Some(bytes.clone());
        }
        let job = st.queue.remove(i).expect("position is in range");
        self.run_marked(st, job, &mut active_log);
        None
    }

    /// Waits until every queued job has run (the coordinator's exit).
    pub(crate) fn drain(&self) {
        let mut st = self.lock();
        while !st.queue.is_empty() || !st.in_flight.is_empty() {
            st = self.wait(st);
        }
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
        if let Some(i) = st.in_flight.iter().position(|p| p == self.path) {
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
pub(crate) fn drain() {
    global().drain()
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
            path: path.as_bytes().to_vec(),
            temp: format!("{path}.new").into_bytes(),
            sep: b'/',
            kind: JobKind::Write {
                bytes: content.as_bytes().to_vec(),
                transactional: true,
                sync: true,
            },
        }
    }

    fn delete(path: &str) -> Job {
        Job {
            kind: JobKind::Delete { temp_too: true },
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
        assert!(saver.submit(write("4s", "old"), &mut |_, _| {}));
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
            path: path.as_os_str().as_bytes().to_vec(),
            temp: dir.join("5s.new").as_os_str().as_bytes().to_vec(),
            sep: b'/',
            kind: JobKind::Write {
                bytes: content.as_bytes().to_vec(),
                transactional: true,
                sync: true,
            },
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
