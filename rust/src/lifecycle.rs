//! Subsystem lifecycle coordination and phase state machine (Milestone M5 — Requirement R2).

use crate::options::OptStr;
use std::ffi::{c_char, c_int};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::RwLock;
use std::time::Duration;

#[cfg(unix)]
const SIGHUP: c_int = 1;
#[cfg(unix)]
const SIGINT: c_int = 2;
#[cfg(unix)]
const SIGTERM: c_int = 15;

static ACTIVE_UMASK: AtomicU32 = AtomicU32::new(0);

pub(crate) fn active_umask() -> u32 {
    ACTIVE_UMASK.load(Ordering::Relaxed)
}

/// Daemon lifecycle phases managed by Rust orchestrator.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecyclePhase {
    Uninitialized = 0,
    OptionsLogging = 1,
    SubsystemsRecovery = 2,
    ServersActive = 3,
    ShutdownReload = 4,
    Terminated = 5,
}

impl From<u32> for LifecyclePhase {
    fn from(v: u32) -> Self {
        match v {
            1 => LifecyclePhase::OptionsLogging,
            2 => LifecyclePhase::SubsystemsRecovery,
            3 => LifecyclePhase::ServersActive,
            4 => LifecyclePhase::ShutdownReload,
            5 => LifecyclePhase::Terminated,
            _ => LifecyclePhase::Uninitialized,
        }
    }
}

/// The daemon's lifecycle hooks, which `LifecycleManager::run_daemon_loop`
/// drives (`crate::daemon` implements it; tests register mocks). A default
/// method is what the loop does when the host has nothing to add.
pub trait LifecycleHooks: Sync {
    /// Process start-up, before the command line is looked at.
    fn util_init(&self) {}

    /// `--nserv`: runs the test NNTP server and returns its exit code, or
    /// `None` when the binary has no NServ.
    ///
    /// # Safety
    /// `argv` must point to `argc` valid C strings.
    unsafe fn nserv_main(&self, _argc: c_int, _argv: *mut *mut c_char) -> Option<c_int> {
        None
    }

    /// Phase 1. Non-zero ends the loop: a positive value is the exit code,
    /// a negative one exits with 0.
    fn phase1_init(&self, reload: bool) -> c_int;

    /// Phase 2. `None` lets the loop create the server connections and the
    /// stat meter itself; `Some(non-zero)` ends the loop with that code.
    fn phase2_subsystems_create_and_recover(&self, _reload: bool) -> Option<c_int> {
        None
    }

    /// Phase 3. Non-zero becomes the exit code.
    fn phase3_server_activate(&self) -> c_int;

    /// Phase 4: tear down, for a reload or for good.
    fn phase4_shutdown(&self, _reload: bool) {}

    /// Whether an unpack is running, which defers an in-process reload.
    fn has_active_unpack(&self) -> bool {
        false
    }

    /// The host's own reload flag.
    fn is_reloading(&self) -> bool {
        false
    }

    /// Whether the host runs reloads itself ([`Self::reload`] and
    /// [`Self::is_reloading`]). If not, the manager's own reload flag counts too.
    fn drives_reload(&self) -> bool {
        false
    }

    /// Starts a reload the manager accepted.
    fn reload(&self) {}

    /// Starts a shutdown the manager accepted.
    fn exit_proc(&self) {}
}

static REGISTERED_HOOKS: RwLock<Option<&'static dyn LifecycleHooks>> = RwLock::new(None);

pub fn register_hooks(hooks: &'static dyn LifecycleHooks) {
    if let Ok(mut lock) = REGISTERED_HOOKS.write() {
        *lock = Some(hooks);
    }
}

pub fn get_hooks() -> Option<&'static dyn LifecycleHooks> {
    REGISTERED_HOOKS.read().ok().and_then(|guard| *guard)
}

pub struct LifecycleManager {
    phase: AtomicU32,
    reloading: AtomicBool,
    stopping: AtomicBool,
}

static LIFECYCLE_MANAGER: LifecycleManager = LifecycleManager {
    phase: AtomicU32::new(LifecyclePhase::Uninitialized as u32),
    reloading: AtomicBool::new(false),
    stopping: AtomicBool::new(false),
};

impl LifecycleManager {
    pub fn global() -> &'static Self {
        &LIFECYCLE_MANAGER
    }

    pub fn get_phase(&self) -> LifecyclePhase {
        self.phase.load(Ordering::SeqCst).into()
    }

    pub fn set_phase(&self, phase: LifecyclePhase) {
        self.phase.store(phase as u32, Ordering::SeqCst);
    }

    pub fn is_reloading(&self) -> bool {
        self.reloading.load(Ordering::SeqCst)
    }

    pub fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    pub fn set_reloading(&self, val: bool) {
        self.reloading.store(val, Ordering::SeqCst);
    }

    pub fn set_stopping(&self, val: bool) {
        self.stopping.store(val, Ordering::SeqCst);
    }

    pub fn trigger_reload(&self) {
        if self.is_stopping() {
            return;
        }
        if self
            .reloading
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        if let Some(hooks) = get_hooks() {
            hooks.reload();
        }
    }

    pub fn trigger_shutdown(&self) {
        if self
            .stopping
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        if let Some(hooks) = get_hooks() {
            hooks.exit_proc();
        }
    }

    pub fn notify_signal(&self, signum: c_int) {
        #[cfg(unix)]
        {
            if signum == SIGHUP {
                if !self.is_stopping() {
                    self.reloading.store(true, Ordering::SeqCst);
                }
            } else if signum == SIGINT || signum == SIGTERM {
                self.stopping.store(true, Ordering::SeqCst);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = signum;
        }
    }

    pub fn run_daemon_loop(
        &self,
        _argc: c_int,
        _argv: *mut *mut c_char,
        _argp: *mut *mut c_char,
    ) -> c_int {
        let hooks = get_hooks().expect("Lifecycle hooks must be registered");

        let mut reloading = false;
        let mut exit_code = 0;

        while !self.is_stopping() {
            // Phase 1: Platform & Preflight, Options & Logging
            self.set_phase(LifecyclePhase::OptionsLogging);
            let p1_rc = hooks.phase1_init(reloading);
            if p1_rc != 0 {
                // Non-zero return indicates early exit or client operation complete
                exit_code = if p1_rc > 0 { p1_rc } else { 0 };
                break;
            }

            // Phase 2: Core Subsystems Creation & State Recovery
            self.set_phase(LifecyclePhase::SubsystemsRecovery);
            if let Some(p2_rc) = hooks.phase2_subsystems_create_and_recover(reloading) {
                if p2_rc != 0 {
                    exit_code = p2_rc;
                    break;
                }
            } else {
                #[cfg(unix)]
                if crate::options::active_options().is_some_and(|o| !o.remote_client_mode()) {
                    if let Some(sp) = crate::net::active_server_pool() {
                        sp.init_connections();
                        crate::nntprs_pool::rebuild(&sp);
                    }
                    crate::statmeter::init_daemon_statmeter();
                }
            }

            // Phase 3: Server Activation & Wait
            self.set_phase(LifecyclePhase::ServersActive);
            let p3_rc = hooks.phase3_server_activate();
            if p3_rc != 0 {
                exit_code = p3_rc;
            }

            // Phase 4: Orderly Teardown & In-Process Reload Looping
            self.set_phase(LifecyclePhase::ShutdownReload);

            // Active unpack deferral check (Requirement R3):
            // If in-process reload is requested, ensure active unpack finishes before tearing down
            let host_reloading = hooks.is_reloading();
            let mut is_reload = !self.is_stopping()
                && if hooks.drives_reload() {
                    host_reloading
                } else {
                    self.is_reloading() || host_reloading
                };

            if is_reload {
                while !self.is_stopping() && hooks.has_active_unpack() {
                    std::thread::sleep(Duration::from_millis(100));
                }
                if self.is_stopping() || (hooks.drives_reload() && !hooks.is_reloading()) {
                    is_reload = false;
                }
            }

            hooks.phase4_shutdown(is_reload);

            if !is_reload {
                break;
            }

            reloading = true;
            self.set_reloading(false);
        }

        self.set_phase(LifecyclePhase::Terminated);
        exit_code
    }
}

static SIG_HUP_PENDING: AtomicBool = AtomicBool::new(false);
static SIG_EXIT_PENDING: AtomicBool = AtomicBool::new(false);
static SHUTDOWN_INITIATED: AtomicBool = AtomicBool::new(false);
static SIGSEGV_LOG_CB: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(unix)]
fn record_signal_flags(signum: c_int) -> c_int {
    match signum {
        libc::SIGHUP => {
            if SHUTDOWN_INITIATED.load(Ordering::Relaxed) {
                return 0;
            }
            SIG_HUP_PENDING.store(true, Ordering::SeqCst);
            LifecycleManager::global().notify_signal(signum);
            1
        }
        libc::SIGINT | libc::SIGTERM => {
            if SHUTDOWN_INITIATED.swap(true, Ordering::SeqCst) {
                return 0;
            }
            SIG_EXIT_PENDING.store(true, Ordering::SeqCst);
            LifecycleManager::global().notify_signal(signum);
            2
        }
        _ => 0,
    }
}

#[cfg(unix)]
extern "C" fn posix_signal_proc(signum: c_int) {
    unsafe {
        match signum {
            libc::SIGHUP => {
                let _ = record_signal_flags(signum);
            }
            libc::SIGINT | libc::SIGTERM => {
                if record_signal_flags(signum) == 2 {
                    let _ = libc::signal(libc::SIGINT, libc::SIG_IGN);
                    let _ = libc::signal(libc::SIGTERM, libc::SIG_IGN);
                    let _ = libc::signal(libc::SIGHUP, libc::SIG_IGN);
                }
            }
            libc::SIGCHLD => {}
            libc::SIGSEGV => {
                let _ = libc::signal(libc::SIGSEGV, libc::SIG_DFL);
                let addr = SIGSEGV_LOG_CB.load(Ordering::Relaxed);
                if addr != 0 {
                    let cb: unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char) =
                        std::mem::transmute(addr);
                    cb(std::ptr::null_mut(), c"Segmentation fault".as_ptr());
                }
            }
            _ => {}
        }
    }
}

/// Installs daemon POSIX signal handlers (`SIGINT`, `SIGTERM`, `SIGHUP`, `SIGPIPE`, `SIGCHLD`,
/// and debug `SIGSEGV`) and optionally enables core dumps (`CrashDump`).
pub fn install_error_handler(
    crash_dump: bool,
    crash_trace: bool,
    segv_log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
) {
    if crash_dump {
        enable_core_dump();
    }
    let cb_addr = segv_log_cb.map_or(0usize, |f| f as usize);
    SIGSEGV_LOG_CB.store(cb_addr, Ordering::SeqCst);

    let exit_pending = SIG_EXIT_PENDING.load(Ordering::SeqCst);
    SHUTDOWN_INITIATED.store(exit_pending, Ordering::SeqCst);
    SIG_HUP_PENDING.store(false, Ordering::SeqCst);

    #[cfg(unix)]
    unsafe {
        let handler = posix_signal_proc as *const () as libc::sighandler_t;
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = handler;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaddset(&mut sa.sa_mask, libc::SIGINT);
        libc::sigaddset(&mut sa.sa_mask, libc::SIGTERM);
        libc::sigaddset(&mut sa.sa_mask, libc::SIGHUP);
        sa.sa_flags = libc::SA_RESTART;

        let _ = libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
        let _ = libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
        let _ = libc::sigaction(libc::SIGHUP, &sa, std::ptr::null_mut());
        let _ = libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        if crash_trace {
            let _ = libc::signal(libc::SIGSEGV, handler);
        }
        let _ = libc::signal(libc::SIGCHLD, handler);
    }
    #[cfg(not(unix))]
    {
        let _ = crash_trace;
    }
}

/// Queries or updates pending daemon signal state:
/// - `0`: load `SIG_EXIT_PENDING`
/// - `1`: swap `SIG_EXIT_PENDING` with `false`
/// - `2`: swap `SIG_HUP_PENDING` with `false`
/// - `3`: clear `SIG_HUP_PENDING`
/// - `4`: load `SIG_EXIT_PENDING || SIG_HUP_PENDING`
/// - `5`: notify `LifecycleManager` of `SIGTERM` shutdown
pub fn signal_state_op(sub_op: c_int) -> c_int {
    match sub_op {
        0 => c_int::from(SIG_EXIT_PENDING.load(Ordering::SeqCst)),
        1 => c_int::from(SIG_EXIT_PENDING.swap(false, Ordering::SeqCst)),
        2 => c_int::from(SIG_HUP_PENDING.swap(false, Ordering::SeqCst)),
        3 => {
            SIG_HUP_PENDING.store(false, Ordering::SeqCst);
            0
        }
        4 => c_int::from(
            SIG_EXIT_PENDING.load(Ordering::SeqCst) || SIG_HUP_PENDING.load(Ordering::SeqCst),
        ),
        5 => {
            #[cfg(unix)]
            LifecycleManager::global().notify_signal(SIGTERM);
            0
        }
        _ => 0,
    }
}

/// Enables core dumps (`setrlimit(RLIMIT_CORE, RLIM_INFINITY)` on Unix + `prctl(PR_SET_DUMPABLE, 1)` on Linux).
pub fn enable_core_dump() {
    #[cfg(unix)]
    unsafe {
        let rlim = libc::rlimit {
            rlim_cur: libc::RLIM_INFINITY,
            rlim_max: libc::RLIM_INFINITY,
        };
        let _ = libc::setrlimit(libc::RLIMIT_CORE, &rlim);
        #[cfg(target_os = "linux")]
        {
            let _ = libc::prctl(libc::PR_SET_DUMPABLE, 1, 0, 0, 0);
        }
    }
}

/// Polls `fd` in bounded 1..=50 ms intervals while `remaining_ms_or_stopped() >= 0`.
/// Returns `true` when the socket is ready and the deadline has not elapsed.
pub fn socket_deadline_wait(
    fd: c_int,
    read: bool,
    write: bool,
    mut remaining_ms_or_stopped: impl FnMut() -> i64,
) -> bool {
    #[cfg(unix)]
    {
        loop {
            let remaining = remaining_ms_or_stopped();
            if remaining < 0 {
                return false;
            }
            let milliseconds = remaining.clamp(1, 50) as c_int;
            let events =
                (if read { libc::POLLIN } else { 0 }) | (if write { libc::POLLOUT } else { 0 });
            let mut descriptor = libc::pollfd {
                fd,
                events,
                revents: 0,
            };
            let result = unsafe { libc::poll(&mut descriptor, 1, milliseconds) };
            if result < 0 {
                let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                if err != libc::EINTR {
                    return false;
                }
            } else if result > 0 {
                return remaining_ms_or_stopped() >= 0;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, read, write, &mut remaining_ms_or_stopped);
        false
    }
}

/// Performs post-`fork` / post-`setsid` POSIX daemonization setup (`RustyGet::Daemonize`):
/// redirects stdio to `/dev/null`, creates/validates/locks/truncates `lock_file` (keeping the
/// locked descriptor open for the lifetime of the daemon), drops privileges to `daemon_username`
/// when running as root (unless backward-compat `"root"`), writes `<pid>\n` to the lock-file,
/// and ignores `SIGCHLD`, `SIGTSTP`, `SIGTTOU`, and `SIGTTIN`.
///
/// Returns `Ok((lock_file_owned, truncate_failed, skipped_errno))` on success, or
/// `Err(error_code)` where:
/// - `1`: could not create lock-file (unless its folder does not exist: then it runs
///   without one and `skipped_errno` says why)
/// - `2`: lock-file is not a regular file
/// - `3`: could not acquire lock on lock-file
/// - `4`: invalid `DaemonUsername`
/// - `5`: could not drop privileges
pub fn daemonize_setup(
    lock_file: &std::ffi::CStr,
    daemon_username: &std::ffi::CStr,
) -> Result<(bool, bool, Option<i32>), i32> {
    #[cfg(unix)]
    unsafe {
        let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if devnull >= 0 {
            let _ = libc::dup2(devnull, 0);
            let _ = libc::dup2(devnull, 1);
            let _ = libc::dup2(devnull, 2);
            if devnull > 2 {
                let _ = libc::close(devnull);
            }
        }

        let mut lfp: c_int = -1;
        let mut lock_file_owned = false;
        let mut truncate_failed = false;
        let mut skipped_errno = None;

        if !lock_file.to_bytes().is_empty() {
            let flags = libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC;
            lfp = libc::open(lock_file.as_ptr(), flags, 0o640 as libc::c_uint);
            if lfp < 0 {
                let errno = crate::fsops::last_errno();
                // only a folder that is not there is skipped: a permission or
                // read-only error, or a lock file that may be another instance's,
                // still refuses to start
                if !lock_file_skippable_errno(errno) || !lock_file_folder_missing(lock_file.to_bytes()) {
                    return Err(1);
                }
                // a path that does not work yet: run without the lock file
                skipped_errno = Some(errno);
            }
        }

        if lfp >= 0 {
            let mut st: libc::stat = std::mem::zeroed();
            if libc::fstat(lfp, &mut st) != 0 || (st.st_mode & libc::S_IFMT) != libc::S_IFREG {
                let _ = libc::close(lfp);
                return Err(2);
            }

            if libc::lockf(lfp, libc::F_TLOCK, 0) < 0 {
                let _ = libc::close(lfp);
                return Err(3);
            }
            lock_file_owned = true;

            if libc::ftruncate(lfp, 0) != 0 {
                truncate_failed = true;
            }
        }

        let backward_compat_root =
            crate::systemhealth::nzbget_is_backward_compat_root_user(daemon_username.to_bytes());

        if !daemon_username.to_bytes().is_empty()
            && !backward_compat_root
            && (libc::getuid() == 0 || libc::geteuid() == 0)
        {
            let pw = libc::getpwnam(daemon_username.as_ptr());
            if pw.is_null() {
                return Err(4);
            }
            let uid = (*pw).pw_uid;
            let gid = (*pw).pw_gid;

            if lfp > -1 {
                let _ = libc::fchown(lfp, uid, gid);
            }

            #[cfg(target_os = "macos")]
            let initgroups_gid = gid as c_int;
            #[cfg(not(target_os = "macos"))]
            let initgroups_gid = gid;

            if libc::setgroups(0, std::ptr::null()) != 0
                || libc::setgid(gid) != 0
                || libc::initgroups(daemon_username.as_ptr(), initgroups_gid) != 0
                || libc::setuid(uid) != 0
            {
                return Err(5);
            }
        }

        if lfp > -1 {
            let pid_line = format!("{}\n", libc::getpid());
            let _ = libc::write(lfp, pid_line.as_ptr().cast(), pid_line.len());
        }

        let _ = libc::signal(libc::SIGCHLD, libc::SIG_IGN);
        let _ = libc::signal(libc::SIGTSTP, libc::SIG_IGN);
        let _ = libc::signal(libc::SIGTTOU, libc::SIG_IGN);
        let _ = libc::signal(libc::SIGTTIN, libc::SIG_IGN);

        Ok((lock_file_owned, truncate_failed, skipped_errno))
    }
    #[cfg(not(unix))]
    {
        let _ = (lock_file, daemon_username);
        Ok((false, false, None))
    }
}

/// True when `lock_file` is set, does not exist and its folder is missing.
/// True when `lock_file` is set and neither it nor its folder exists (or a
/// regular file stands where the folder should be). A lookup that fails for
/// another reason, such as a permission error, does not count as missing.
pub(crate) fn lock_file_folder_missing(lock_file: &[u8]) -> bool {
    if lock_file.is_empty() {
        return false;
    }
    let gone = |p: &std::path::Path| match std::fs::symlink_metadata(p) {
        Err(e) => lock_file_skippable_errno(e.raw_os_error().unwrap_or(0)),
        Ok(_) => false,
    };
    let path = crate::cleanup::bytes_to_path_buf(lock_file);
    let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) else {
        return false;
    };
    gone(&path)
        && (gone(dir) || std::fs::metadata(dir).is_ok_and(|m| !m.is_dir()))
}

/// A lock file that cannot be created because its folder does not exist is a
/// path to fix, not a reason to refuse to start. A permission or read-only error
/// still refuses, as before.
pub(crate) fn lock_file_skippable_errno(errno: i32) -> bool {
    #[cfg(unix)]
    {
        matches!(errno, libc::ENOENT | libc::ENOTDIR)
    }
    #[cfg(not(unix))]
    {
        let _ = errno;
        false
    }
}

struct ReloadState {
    pending: bool,
    worker_running: bool,
    first_request_time: Option<std::time::Instant>,
    target_time: Option<std::time::Instant>,
    worker_thread: Option<std::thread::JoinHandle<()>>,
}

struct ReloadCoordinator {
    mutex: std::sync::Mutex<ReloadState>,
    cond: std::sync::Condvar,
}

impl ReloadCoordinator {
    const fn new() -> Self {
        Self {
            mutex: std::sync::Mutex::new(ReloadState {
                pending: false,
                worker_running: false,
                first_request_time: None,
                target_time: None,
                worker_thread: None,
            }),
            cond: std::sync::Condvar::new(),
        }
    }

    fn cancel_pending(&self) {
        let mut state = self.mutex.lock().unwrap_or_else(|e| e.into_inner());
        state.pending = false;
        self.cond.notify_all();
    }

    fn cancel_and_join(&self) {
        let mut state = self.mutex.lock().unwrap_or_else(|e| e.into_inner());
        state.pending = false;
        self.cond.notify_all();
        let can_join = state
            .worker_thread
            .as_ref()
            .is_some_and(|h| h.thread().id() != std::thread::current().id());
        if can_join {
            while state.worker_running {
                state = self.cond.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            if let Some(handle) = state.worker_thread.take() {
                drop(state);
                let _ = handle.join();
            }
        }
    }

    fn request_reload<F>(&'static self, state_cb: F)
    where
        F: Fn(c_int) -> c_int + Send + Sync + Copy + 'static,
    {
        use std::time::{Duration, Instant};

        if state_cb(0) != 0 {
            state_cb(1);
            return;
        }

        let now = Instant::now();
        let mut state = self.mutex.lock().unwrap_or_else(|e| e.into_inner());

        if !state.pending {
            while state.worker_running {
                state = self.cond.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            if state_cb(0) != 0 {
                drop(state);
                state_cb(1);
                return;
            }
            if let Some(finished) = state.worker_thread.take() {
                drop(state);
                let _ = finished.join();
                state = self.mutex.lock().unwrap_or_else(|e| e.into_inner());
                if state.pending || state.worker_running || state_cb(0) != 0 {
                    return;
                }
            }

            state.pending = true;
            state.worker_running = true;
            state.first_request_time = Some(now);
            state.target_time = Some(now + Duration::from_millis(100));

            let handle = std::thread::spawn(move || {
                let mut wstate = self.mutex.lock().unwrap_or_else(|e| e.into_inner());

                while wstate.pending && state_cb(3) == 0 {
                    let cur = Instant::now();
                    let first = wstate.first_request_time.unwrap_or(cur);
                    let target = wstate.target_time.unwrap_or(cur);
                    if cur >= target
                        || cur.saturating_duration_since(first) >= Duration::from_millis(1500)
                    {
                        break;
                    }
                    let wait_dur = target.saturating_duration_since(cur);
                    wstate = self
                        .cond
                        .wait_timeout(wstate, wait_dur)
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }

                if !wstate.pending && state_cb(3) == 0 {
                    wstate.worker_running = false;
                    self.cond.notify_all();
                    return;
                }

                let mut logged_deferral = false;
                while wstate.pending && state_cb(3) == 0 {
                    drop(wstate);
                    let flags = state_cb(4);
                    let active_unpack = (flags & 1) != 0;
                    let already_reloading = (flags & 2) != 0;
                    if active_unpack && !logged_deferral {
                        state_cb(5);
                        logged_deferral = true;
                    }
                    wstate = self.mutex.lock().unwrap_or_else(|e| e.into_inner());
                    if !wstate.pending || state_cb(3) != 0 || !active_unpack || already_reloading {
                        break;
                    }
                    wstate = self
                        .cond
                        .wait_timeout(wstate, Duration::from_millis(100))
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }

                if state_cb(6) != 0 {
                    wstate.pending = false;
                    drop(wstate);
                    state_cb(7);
                    let mut done = self.mutex.lock().unwrap_or_else(|e| e.into_inner());
                    done.worker_running = false;
                    self.cond.notify_all();
                    return;
                }

                if !wstate.pending {
                    wstate.worker_running = false;
                    self.cond.notify_all();
                    return;
                }

                wstate.pending = false;
                drop(wstate);

                state_cb(8);

                let mut done = self.mutex.lock().unwrap_or_else(|e| e.into_inner());
                done.worker_running = false;
                self.cond.notify_all();
            });
            state.worker_thread = Some(handle);
        } else {
            let first = state.first_request_time.unwrap_or(now);
            let elapsed_ms = now
                .saturating_duration_since(first)
                .as_millis()
                .min(i64::MAX as u128) as i64;
            let clamped = crate::options::roc_nzbget_reload_clamp_debounce_ms(elapsed_ms, 100, 1500)
                .max(0) as u64;
            state.target_time = Some(first + Duration::from_millis(clamped));
            self.cond.notify_one();
            drop(state);
            state_cb(2);
        }
    }
}

static RELOAD_COORDINATOR: ReloadCoordinator = ReloadCoordinator::new();

/// Daemon reload debounce and unpack-deferral coordinator:
/// - `op = 0`: `Reload()` (debounce 100ms..1500ms, defer while `HasActiveUnpack`, handle `SigExitPending`).
/// - `op = 1`: Cancel pending reload (`Stop(false)` / `ExitProc()`).
/// - `op = 2`: `CancelAndJoinReloadThread()`.
pub fn reload_coordinator_op(op: c_int, state_cb: Option<unsafe extern "C" fn(c_int) -> c_int>) {
    match op {
        0 => {
            if let Some(cb) = state_cb {
                RELOAD_COORDINATOR.request_reload(move |action| unsafe { cb(action) });
            }
        }
        1 => RELOAD_COORDINATOR.cancel_pending(),
        2 => RELOAD_COORDINATOR.cancel_and_join(),
        _ => {}
    }
}

// ============================================================================
// End-to-end `DiskService` & `RustyGet` daemon state/loop coordinator
// ============================================================================

/// What the lifecycle needs from the running daemon instance
/// (implemented by `crate::daemon`).
pub(crate) trait DaemonHost: Sync {
    /// Whether an instance exists.
    fn has_instance(&self) -> bool;
    /// Whether the pre/post-processor has an unpack running.
    fn has_active_unpack(&self) -> bool;
    /// Stops the instance for a reload or for good; does nothing without one.
    fn stop(&self, reload: bool);
    /// Starts the pre/post-processor.
    fn start_prepost(&self);
    /// Whether the pre/post-processor thread is running.
    fn prepost_running(&self) -> bool;
    /// Whether the pre/post-processor has jobs.
    fn prepost_has_jobs(&self) -> bool;
    /// Stops the pre/post-processor unless it is busy (standalone mode end).
    fn stop_prepost_when_idle(&self);
    /// Stops the pre/post-processor as the daemon stops (not in remote mode).
    fn stop_prepost(&self, remote_mode: bool);
}

struct DiskServiceState {
    waiting_required_dir: bool,
    waiting_reported: bool,
}

struct DaemonWaitState {
    reloading: bool,
    stopped: bool,
}

struct DaemonInstanceState {
    disk: std::sync::Mutex<DiskServiceState>,
    wait_mutex: std::sync::Mutex<DaemonWaitState>,
    wait_cond: std::sync::Condvar,
    lock_file_owned: AtomicBool,
    cmdline: std::sync::Mutex<Option<(Vec<u8>, crate::cmdline::CmdlineHandle)>>,
}

static DAEMON_INSTANCE: DaemonInstanceState = DaemonInstanceState {
    disk: std::sync::Mutex::new(DiskServiceState {
        waiting_required_dir: true,
        waiting_reported: false,
    }),
    wait_mutex: std::sync::Mutex::new(DaemonWaitState {
        reloading: false,
        stopped: false,
    }),
    wait_cond: std::sync::Condvar::new(),
    lock_file_owned: AtomicBool::new(false),
    cmdline: std::sync::Mutex::new(None),
};

#[cfg(unix)]
pub(crate) fn daemon_cmdline_options_inputs() -> Option<(
    Vec<u8>,
    Option<std::ffi::CString>,
    bool,
    Vec<std::ffi::CString>,
)> {
    let g = DAEMON_INSTANCE.cmdline.lock().ok()?;
    let (exe, cmd) = g.as_ref()?;
    Some((
        exe.clone(),
        cmd.config_filename.clone(),
        cmd.no_config,
        cmd.option_list.clone(),
    ))
}

/// Server or daemon mode on the parsed command line.
pub(crate) fn daemon_cmdline_server_mode() -> bool {
    DAEMON_INSTANCE
        .cmdline
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|(_, cmd)| cmd.server_mode || cmd.daemon_mode))
        .unwrap_or(false)
}

#[cfg(unix)]
static DAEMON_FRONTEND: std::sync::Mutex<Option<std::sync::Arc<crate::ffi::FrontendHandle>>> =
    std::sync::Mutex::new(None);

#[cfg(unix)]
fn daemon_frontend_clone() -> Option<std::sync::Arc<crate::ffi::FrontendHandle>> {
    DAEMON_FRONTEND.lock().ok().and_then(|g| g.clone())
}

#[cfg(unix)]
fn daemon_cmdline_clone() -> Option<crate::cmdline::CmdlineHandle> {
    DAEMON_INSTANCE
        .cmdline
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|(_, c)| c.clone()))
}

#[cfg(unix)]
unsafe fn daemon_process_direct(cmd: &crate::cmdline::CmdlineHandle) {
    if crate::ffi::build_debug_enabled() && cmd.test_backtrace {
        libc::raise(libc::SIGSEGV);
        libc::abort();
    }

    if cmd.web_get {
        let url = cmd
            .last_arg
            .as_deref()
            .map_or(&[][..], std::ffi::CStr::to_bytes);
        let out_fn = cmd
            .web_get_filename
            .as_deref()
            .map_or(&[][..], std::ffi::CStr::to_bytes);
        let dl = crate::net::NzbgetRsWebDl::new_configured(
            url, b"WebGet", out_fn, true, false, 2, None, None,
        );
        let st = dl.download_sync(5);
        libc::exit(
            if st == crate::webdownload::WebDlStatus::AdFinished as c_int {
                0
            } else {
                1
            },
        );
    }

    if cmd.sig_verify {
        if crate::ffi::build_tls_disabled() {
            libc::printf(
                c"ERROR: Could not verify signature, the program was compiled without OpenSSL support\n"
                    .as_ptr(),
            );
            libc::exit(1);
        }
        let in_fn = cmd.last_arg.as_deref().map_or(c"", |c| c);
        let sig_fn = cmd.sig_filename.as_deref().map_or(c"", |c| c);
        let pub_fn = cmd.pub_key_filename.as_deref().map_or(c"", |c| c);
        let ok = crate::maintenance::verify_signature_active(
            in_fn.to_bytes(),
            sig_fn.to_bytes(),
            pub_fn.to_bytes(),
        );
        libc::exit(if ok { 93 } else { 1 });
    }

    if cmd.client_operation != 0 {
        let opts = crate::options::active_options();
        let ctrl_ip = opts.as_deref().map_or(&[][..], |o| o.str_bytes(OptStr::ControlIp));
        let ctrl_port = opts.as_deref().map_or(0, |o| o.scalars().control_port);
        let ctrl_user = opts.as_deref().map_or(&[][..], |o| o.str_bytes(OptStr::ControlUsername));
        let ctrl_pass = opts.as_deref().map_or(&[][..], |o| o.str_bytes(OptStr::ControlPassword));
        let ok = cmd.execute_client(
            ctrl_ip,
            ctrl_port,
            ctrl_user,
            ctrl_pass,
            |bytes| crate::ffi::write_stdout_bytes(bytes, false),
            |cmsg| libc::perror(cmsg.as_ptr()),
        );
        libc::exit(if ok { 0 } else { 1 });
    }

    if cmd.print_options {
        if let Some(opts) = crate::options::active_options() {
            opts.for_each_option(|n, v, _| {
                libc::printf(c"%s = \"%s\"\n".as_ptr(), n.as_ptr(), v.as_ptr());
            });
        }
        libc::exit(0);
    }
}

fn dm_cbuf(bytes: &[u8]) -> Vec<u8> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let mut v = Vec::with_capacity(end + 1);
    v.extend_from_slice(&bytes[..end]);
    v.push(0);
    v
}

fn dm_emit_active_log(level: c_int, msg: &[u8]) {
    #[cfg(unix)]
    {
        let kind = match level {
            0 => 3,
            1 => 4,
            2 => 0,
            3 => 1,
            4 => 2,
            _ => return,
        };
        crate::log::log_active_bytes(kind, msg);
    }
    #[cfg(not(unix))]
    {
        let _ = (level, msg);
    }
}

fn dm_log(level: c_int, msg: &[u8]) {
    dm_emit_active_log(level, msg);
}

fn dm_is_stopped() -> bool {
    DAEMON_INSTANCE
        .wait_mutex
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .stopped
}

fn dm_is_reloading() -> bool {
    DAEMON_INSTANCE
        .wait_mutex
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .reloading
}

fn trigger_exit(host: &dyn DaemonHost) {
    RELOAD_COORDINATOR.cancel_pending();
    signal_state_op(5);
    host.stop(false);
}

fn trigger_reload(host: &'static dyn DaemonHost) {
    RELOAD_COORDINATOR.request_reload(move |action| match action {
        0 => c_int::from(
            !host.has_instance() || dm_is_reloading() || dm_is_stopped() || signal_state_op(0) != 0,
        ),
        1 => {
            dm_emit_active_log(0, b"Daemon is already reloading or stopping");
            0
        }
        2 => {
            dm_emit_active_log(0, b"Reload request debounced, target extended");
            0
        }
        3 => signal_state_op(0),
        4 => {
            c_int::from(host.has_active_unpack())
                | (if !host.has_instance() || dm_is_reloading() { 2 } else { 0 })
        }
        5 => {
            dm_emit_active_log(
                2,
                b"Unpack is actively executing; deferring reload until unpack completes...",
            );
            0
        }
        6 => signal_state_op(1),
        7 => {
            RELOAD_COORDINATOR.cancel_pending();
            signal_state_op(5);
            host.stop(false);
            0
        }
        8 => {
            if host.has_instance() && !dm_is_reloading() {
                dm_emit_active_log(2, b"Reloading...");
                host.stop(true);
            }
            0
        }
        _ => 0,
    });
}

#[cfg(unix)]
pub(crate) fn disk_service_interval_active() -> c_int {
    let waiting = DAEMON_INSTANCE
        .disk
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .waiting_required_dir;
    let disk_space = crate::options::active_options().map_or(0, |o| o.scalars().disk_space);
    let downloading = crate::ffi::active_workstate().is_some_and(|w| w.downloading());
    if waiting || (disk_space > 0 && downloading) {
        1
    } else {
        -1
    }
}

#[cfg(unix)]
pub(crate) fn disk_service_work_active() {
    dm_emit_active_log(0, b"Disk service work");
    let opts = crate::options::active_options();
    let ws = crate::ffi::active_workstate();
    let disk_space = opts.as_ref().map_or(0, |o| o.scalars().disk_space);
    let downloading = ws.as_ref().is_some_and(|w| w.downloading());
    if disk_space > 0 && downloading {
        dm_emit_active_log(0, b"Disk service work: check disk space");
        let dest_c = opts.as_ref().and_then(|o| o.str_cstr(OptStr::DestDir)).unwrap_or(c"");
        let dest_b = opts.as_ref().map_or(&[][..], |o| o.str_bytes(OptStr::DestDir));
        let inter_c = opts.as_ref().and_then(|o| o.str_cstr(OptStr::InterDir));
        let inter_b = opts.as_ref().map_or(&[][..], |o| o.str_bytes(OptStr::InterDir));
        let (dest_low, inter_low) =
            crate::fsops::disk_service_check_disk_space(dest_c, inter_c, disk_space);
        if dest_low {
            let mut wm = b"Low disk space on ".to_vec();
            wm.extend_from_slice(dest_b);
            wm.extend_from_slice(b". Pausing download");
            dm_emit_active_log(3, &wm);
            if let Some(w) = ws.as_ref() {
                w.set_pause_download(true);
            }
        }
        if inter_low {
            let mut wm = b"Low disk space on ".to_vec();
            wm.extend_from_slice(inter_b);
            wm.extend_from_slice(b". Pausing download");
            dm_emit_active_log(3, &wm);
            if let Some(w) = ws.as_ref() {
                w.set_pause_download(true);
            }
        }
    }

    let (waiting_req, prev_reported) = {
        let dg = DAEMON_INSTANCE
            .disk
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        (dg.waiting_required_dir, dg.waiting_reported)
    };
    if waiting_req {
        dm_emit_active_log(0, b"Disk service work: check required dir");
        let req_b = opts.as_ref().map_or(&[][..], |o| o.str_bytes(OptStr::RequiredDir));
        let (all_exist, next_reported) =
            crate::fsops::disk_service_check_required_dirs(req_b, prev_reported, |dir_cstr| {
                let mut im = b"Waiting for required directory ".to_vec();
                im.extend_from_slice(dir_cstr.to_bytes());
                dm_emit_active_log(2, &im);
            });
        if !all_exist {
            DAEMON_INSTANCE
                .disk
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .waiting_reported = next_reported;
            return;
        }
        if prev_reported {
            dm_emit_active_log(2, b"All required directories available");
        }
        if let Some(w) = ws.as_ref() {
            w.set_temp_pause_download(false);
            w.set_temp_pause_postprocess(false);
        }
        let mut dg = DAEMON_INSTANCE
            .disk
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        dg.waiting_required_dir = false;
        dg.waiting_reported = next_reported;
    }
}


/// Phase 1's end: runs the command line's direct modes (they exit), forks
/// into the background in daemon mode, logs the start banner and installs
/// the signal handlers. `segv_cb` logs a crash from the SIGSEGV handler.
pub(crate) unsafe fn activate_daemon(
    segv_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
) {
    let (daemon_mode, remote_mode) = if let Some(cmd) = daemon_cmdline_clone() {
        daemon_process_direct(&cmd);
        (cmd.daemon_mode, cmd.remote_client_mode)
    } else {
        (false, false)
    };

    let was_reloading = dm_is_reloading();
    let active_opts = crate::options::active_options();
    if let Some(opts) = active_opts.as_ref() {
        if opts.server_mode() && opts.scalars().system_health_check != 0 {
            // (before the lock file is taken: the check opens it, and closing any
            // descriptor of a file drops the process's lock on it)
            if daemon_mode && !was_reloading && lock_file_folder_missing(opts.str_bytes(OptStr::LockFile)) {
                // daemonize_setup warns about it once
                crate::options::note_path_warning(b"LockFile", opts.str_bytes(OptStr::LockFile));
            }
            let _ = crate::ffi::system_health_diagnose_bytes(0);
        }
    }
    let (ver, lock_file, daemon_user, server_mode, cfg_fn, ctrl_ip, ctrl_port, crash_dump, crash_trace) =
        if let Some(opts) = active_opts.as_ref() {
            let sc = opts.scalars();
            (
                crate::ffi::build_version_bytes(),
                opts.str_cstr(OptStr::LockFile).unwrap_or(c""),
                opts.str_cstr(OptStr::DaemonUsername).unwrap_or(c""),
                opts.server_mode(),
                opts.str_bytes(OptStr::ConfigFile),
                opts.str_bytes(OptStr::ControlIp),
                sc.control_port,
                sc.crash_dump != 0,
                crate::ffi::build_debug_enabled() && sc.crash_trace != 0,
            )
        } else {
            (&[][..], c"", c"", false, &[][..], &[][..], 0, false, false)
        };

    if daemon_mode {
        if !was_reloading {
            let f = libc::fork();
            if f < 0 {
                libc::exit(1);
            }
            if f > 0 {
                libc::exit(0);
            }
            libc::setsid();
            match daemonize_setup(lock_file, daemon_user) {
                Ok((owned, trunc_failed, skipped_errno)) => {
                    DAEMON_INSTANCE
                        .lock_file_owned
                        .store(owned, Ordering::SeqCst);
                    if let Some(errno) = skipped_errno {
                        let why = crate::fsops::error_message(errno);
                        let mut wm = crate::options::path_problem_message(
                            b"LockFile",
                            lock_file.to_bytes(),
                            &why,
                        );
                        wm.extend_from_slice(
                            b" Running without a lock file: after fixing LockFile, restart RustyGet \
(a reload is not enough) so it takes the lock.",
                        );
                        crate::options::note_path_warning(b"LockFile", lock_file.to_bytes());
                        dm_log(3, &wm);
                    }
                    if trunc_failed {
                        let mut wm = b"Could not truncate lock-file ".to_vec();
                        wm.extend_from_slice(lock_file.to_bytes());
                        dm_log(3, &wm);
                    }
                }
                Err(code) => {
                    let mut em = b"Starting daemon failed: ".to_vec();
                    match code {
                        1 => {
                            em.extend_from_slice(b"could not create lock-file ");
                            em.extend_from_slice(lock_file.to_bytes());
                        }
                        2 => {
                            em.extend_from_slice(b"lock-file ");
                            em.extend_from_slice(lock_file.to_bytes());
                            em.extend_from_slice(b" is not a regular file");
                        }
                        3 => {
                            em.extend_from_slice(b"could not acquire lock on lock-file ");
                            em.extend_from_slice(lock_file.to_bytes());
                        }
                        4 => em.extend_from_slice(b"invalid DaemonUsername"),
                        _ => em.extend_from_slice(b"could not drop privileges"),
                    }
                    dm_log(4, &em);
                    libc::exit(1);
                }
            }
        }
        let mut im = b"RustyGet ".to_vec();
        im.extend_from_slice(ver);
        im.extend_from_slice(b" daemon-mode");
        dm_log(2, &im);
        // after the fork: a thread started in the parent would not survive it
        crate::log::allow_background_threads();
        crate::sysmetrics::start_sampler();
    } else if server_mode {
        let mut im = b"RustyGet ".to_vec();
        im.extend_from_slice(ver);
        im.extend_from_slice(b" server-mode");
        dm_log(2, &im);
        crate::log::allow_background_threads();
        crate::sysmetrics::start_sampler();
    } else if remote_mode {
        let mut im = b"RustyGet ".to_vec();
        im.extend_from_slice(ver);
        im.extend_from_slice(b" remote-mode");
        dm_log(2, &im);
    }

    if server_mode {
        let mut u = b"using ".to_vec();
        u.extend_from_slice(cfg_fn);
        dm_log(2, &u);

        let mut r = b"RustyGet runs on ".to_vec();
        r.extend_from_slice(ctrl_ip);
        r.extend_from_slice(format!(":{ctrl_port}").as_bytes());
        dm_log(2, &r);
    }

    if crate::ffi::build_debug_enabled() {
        let sys_bytes = crate::ffi::format_system_info_bytes(0, false);
        let mut lines: Vec<&[u8]> = sys_bytes.split(|&b| b == b'\n').collect();
        if sys_bytes.is_empty() || sys_bytes.ends_with(b"\n") {
            lines.pop();
        }
        for line in lines {
            dm_emit_active_log(1, line);
        }
    }

    DAEMON_INSTANCE
        .wait_mutex
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .reloading = false;

    install_error_handler(crash_dump, crash_trace, segv_cb);
}

fn start_daemon_services() {
    crate::articledl::start_daemon_article_cache();
    crate::scheduler::start_daemon_service_coord();
    crate::dupesearch_coord::start_daemon_dupesearch();
    crate::urlcoord::start_daemon_url_coord();
    crate::feed::start_daemon_feed_coord();
    crate::articledl::start_daemon_memory_monitor();
}

fn stop_daemon_services() {
    crate::articledl::stop_daemon_article_cache();
    crate::scheduler::stop_daemon_service_coord();
    crate::dupesearch_coord::stop_daemon_dupesearch();
    crate::urlcoord::stop_daemon_url_coord();
    crate::feed::stop_daemon_feed_coord();
    crate::articledl::stop_daemon_memory_monitor();
}

fn daemon_services_running() -> bool {
    crate::queuecoord::is_daemon_queue_coord_running()
        || crate::articledl::is_daemon_article_cache_running()
        || crate::scheduler::is_daemon_service_coord_running()
        || crate::dupesearch_coord::is_daemon_dupesearch_running()
        || crate::urlcoord::is_daemon_url_coord_running()
        || crate::feed::is_daemon_feed_coord_running()
        || crate::articledl::is_daemon_memory_monitor_running()
}

/// Handles a pending exit or reload signal.
fn handle_pending_signals(host: &'static dyn DaemonHost) {
    if signal_state_op(1) != 0 {
        trigger_exit(host);
    } else if signal_state_op(2) != 0 {
        trigger_reload(host);
    }
}

/// Phase 3: starts the frontend and the services, then runs the main loop
/// (standalone or server mode) or the remote-mode loop until stop or reload.
pub(crate) fn run_daemon(host: &'static dyn DaemonHost) {
    crate::net::remote_srv_ctl(0);
    if let Some(opts) = crate::options::active_options() {
        if !opts.daemon_mode() {
            let fe_mode = match opts.scalars().output_mode {
                2 if !crate::ffi::build_curses_disabled() => Some(2),
                2 | 1 => Some(1),
                0 => Some(0),
                _ => None,
            };
            if let Some(m) = fe_mode {
                let exit_fn: std::sync::Arc<dyn Fn() + Send + Sync> =
                    std::sync::Arc::new(move || trigger_exit(host));
                let fe = std::sync::Arc::new(crate::ffi::FrontendHandle::new(m, Some(exit_fn)));
                fe.start();
                if let Ok(mut g) = DAEMON_FRONTEND.lock() {
                    *g = Some(fe);
                }
            }
        }
    }
    let cmd_opt = daemon_cmdline_clone();
    // without a command line there is nothing to serve: wait as in remote mode
    let (remote_mode, server_mode, standalone_add, cat_ptr, nzb_ptr) =
        if let Some(ref cmd) = cmd_opt {
            let srv_mode = crate::options::active_options().is_some_and(|o| o.server_mode());
            (
                cmd.remote_client_mode,
                srv_mode,
                !cmd.server_mode,
                cmd.add_category
                    .as_deref()
                    .map_or(std::ptr::null(), std::ffi::CStr::as_ptr),
                cmd.arg_filename
                    .as_deref()
                    .map_or(std::ptr::null(), std::ffi::CStr::as_ptr),
            )
        } else {
            (true, false, false, std::ptr::null(), std::ptr::null())
        };

    if !remote_mode {
        if standalone_add && signal_state_op(0) == 0 {
            // SAFETY: both are null or point into `cmd_opt`, alive until the end.
            let _ = unsafe { crate::scanner::add_cli_nzb_file_active(cat_ptr, nzb_ptr) };
        }

        dm_log(0, b"Entering main program loop");
        crate::queuecoord::start_daemon_queue_coord();
        host.start_prepost();
        start_daemon_services();

        loop {
            if !host.prepost_running() && !daemon_services_running() {
                break;
            }
            handle_pending_signals(host);

            let post_jobs = host.prepost_has_jobs();
            let queue_jobs = crate::queuecoord::daemon_queue_coord_has_more_jobs();
            let url_jobs = crate::urlcoord::daemon_url_coord_has_more_jobs();
            if crate::options::nzbget_should_stop_standalone(
                server_mode,
                queue_jobs,
                url_jobs,
                post_jobs,
            ) {
                crate::queuecoord::stop_daemon_queue_coord(false);
                host.stop_prepost_when_idle();
                stop_daemon_services();
            }

            std::thread::sleep(Duration::from_millis(100));

            if server_mode && !dm_is_stopped() {
                while !dm_is_stopped() {
                    {
                        let wg = DAEMON_INSTANCE
                            .wait_mutex
                            .lock()
                            .unwrap_or_else(|err| err.into_inner());
                        if !wg.stopped && signal_state_op(4) == 0 {
                            let _ = DAEMON_INSTANCE
                                .wait_cond
                                .wait_timeout(wg, Duration::from_millis(50))
                                .unwrap_or_else(|err| err.into_inner());
                        }
                    }
                    handle_pending_signals(host);
                }
            }
        }

        dm_log(0, b"Main program loop terminated");
    } else {
        if signal_state_op(1) != 0 {
            trigger_exit(host);
        }
        while !dm_is_stopped()
            && !dm_is_reloading()
            && daemon_frontend_clone().is_some_and(|fe| fe.is_running())
        {
            handle_pending_signals(host);
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    drop(cmd_opt);
}

/// Stops the daemon (`reload`: for an in-process reload): waits for an active
/// unpack before a reload, stops the frontend or the queue, the
/// pre/post-processor and the services, then marks the instance stopped.
pub(crate) fn stop_daemon(reload: bool, host: &dyn DaemonHost) {
    let remote_mode = daemon_cmdline_clone().is_some_and(|c| c.remote_client_mode);
    if !reload {
        RELOAD_COORDINATOR.cancel_pending();
        signal_state_op(5);
    }

    {
        let mut wg = DAEMON_INSTANCE
            .wait_mutex
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if wg.stopped && !wg.reloading && reload {
            drop(wg);
            dm_log(0, b"Ignoring reload request because shutdown is already in progress");
            return;
        }
        wg.reloading = reload;
    }

    if !dm_is_reloading() {
        signal_state_op(3);
        dm_log(2, b"Stopping, please wait...");
    }

    if dm_is_reloading() && host.has_active_unpack() {
        dm_log(2, b"Waiting for active unpack to complete before in-process reload...");
        while host.has_active_unpack() && signal_state_op(0) == 0 {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    if signal_state_op(1) != 0 {
        DAEMON_INSTANCE
            .wait_mutex
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .reloading = false;
        signal_state_op(5);
    }

    if remote_mode {
        if let Some(fe) = daemon_frontend_clone() {
            dm_log(0, b"Stopping Frontend");
            fe.stop();
        }
    } else {
        crate::queuecoord::stop_daemon_queue_coord(true);
    }
    host.stop_prepost(remote_mode);
    if !remote_mode {
        stop_daemon_services();
    }

    let mut wg = DAEMON_INSTANCE
        .wait_mutex
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    wg.stopped = true;
    DAEMON_INSTANCE.wait_cond.notify_all();
}

/// Asks for an in-process reload (debounced, deferred while unpacking).
pub(crate) fn request_reload(host: &'static dyn DaemonHost) {
    trigger_reload(host);
}

/// Asks the daemon to exit.
pub(crate) fn request_exit(host: &dyn DaemonHost) {
    trigger_exit(host);
}

/// Phase 4: stops scripts, the remote server and the frontend.
pub(crate) fn shutdown_daemon(reload: bool, host: &dyn DaemonHost) {
    let remote_mode = daemon_cmdline_clone().is_some_and(|c| c.remote_client_mode);
    DAEMON_INSTANCE
        .wait_mutex
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .reloading = reload;
    RELOAD_COORDINATOR.cancel_and_join();
    crate::procrun::terminate_all_active();
    crate::net::remote_srv_ctl(1);
    if let Some(fe) = daemon_frontend_clone() {
        if !remote_mode || dm_is_stopped() {
            dm_log(0, b"Stopping Frontend");
            fe.stop();
        }
        while fe.is_running() {
            if signal_state_op(1) != 0 {
                trigger_exit(host);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        dm_log(0, b"Frontend stopped");
    }
    RELOAD_COORDINATOR.cancel_and_join();
}

/// Resets the instance's wait and disk-service state at the start of phase 1.
pub(crate) fn reset_instance(reloading: bool) {
    crate::memory_pressure::limit_malloc_arenas();
    #[cfg(unix)]
    {
        crate::diskstate_format::reset_active_legacy_queue_read();
    }
    {
        let mut dg = DAEMON_INSTANCE
            .disk
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        dg.waiting_required_dir = true;
        dg.waiting_reported = false;
    }
    let mut wg = DAEMON_INSTANCE
        .wait_mutex
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    wg.reloading = reloading;
    wg.stopped = false;
}

/// Whether the daemon is reloading rather than stopping.
pub(crate) fn daemon_is_reloading() -> bool {
    dm_is_reloading()
}

/// Whether this instance is going away: a stop or a config reload has begun.
/// Work that would outlive the instance (a detached job) is not started then.
pub(crate) fn daemon_is_winding_down() -> bool {
    let g = DAEMON_INSTANCE
        .wait_mutex
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    g.stopped || g.reloading
}

/// Shuts down the daemon's global subsystems.
#[cfg(unix)]
fn shutdown_daemon_globals() {
    if let Ok(mut g) = DAEMON_INSTANCE.cmdline.lock() {
        *g = None;
    }
    if let Ok(mut g) = DAEMON_FRONTEND.lock() {
        let _ = g.take();
    }
    crate::scheduler::shutdown_daemon_service_coord();
    crate::scanner::shutdown_daemon_scanner();
    crate::articledl::shutdown_daemon_article_cache();
    crate::articledl::shutdown_daemon_memory_monitor();
    crate::feed::shutdown_daemon_feed_coord();
    crate::urlcoord::shutdown_daemon_url_coord();
    crate::twin::shutdown_daemon_twincheck();
    crate::dupesearch_coord::shutdown_daemon_dupesearch();
    crate::maintenance::shutdown_daemon_maintenance();
    crate::statmeter::shutdown_daemon_statmeter();
    crate::scheduler::shutdown_daemon_scheduler();
    crate::queuecoord::shutdown_daemon_queue_coord();
}

/// Whether this instance created its lock file and may delete it now.
/// True while this daemon holds the lock on its LockFile.
pub(crate) fn daemon_holds_lock_file() -> bool {
    DAEMON_INSTANCE.lock_file_owned.load(Ordering::SeqCst)
}

fn may_delete_lock_file() -> bool {
    !dm_is_reloading() && DAEMON_INSTANCE.lock_file_owned.load(Ordering::SeqCst)
}

/// Frees the daemon's global objects and, with `delete_lock_file`, removes
/// the options' lock file if this instance owns it.
pub(crate) fn cleanup_global_objects(delete_lock_file: bool) {
    dm_emit_active_log(0, b"Cleaning up global objects");
    #[cfg(unix)]
    shutdown_daemon_globals();
    if delete_lock_file && may_delete_lock_file() {
        dm_emit_active_log(2, b"Deleting lock file");
        #[cfg(unix)]
        if let Some(lf) = crate::options::active_options()
            .as_ref()
            .and_then(|opts| opts.str_cstr(OptStr::LockFile))
        {
            crate::fsops::delete_file(lf);
        }
    }
}

/// Why [`parse_command_line`] failed.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandLineError {
    /// The process has no arguments, not even its own name.
    NoArguments,
    /// The arguments did not parse.
    Invalid,
}

/// Creates the early services, then parses the process's command line into
/// the instance. `--version`, `--help` and bad arguments print and exit.
#[cfg(unix)]
pub(crate) fn parse_command_line() -> Result<(), CommandLineError> {
    crate::scheduler::create_daemon_service_coord();
    crate::articledl::create_daemon_article_cache();
    crate::queuecoord::create_daemon_queue_coord();
    dm_emit_active_log(0, b"Parsing command line");
    let args = crate::entrypoint::process_argv();
    let argc = args.len();
    if argc < 1 {
        return Err(CommandLineError::NoArguments);
    }
    let Some(cmd) = crate::cmdline::CmdlineHandle::parse_args(&args, |bytes| {
        crate::ffi::write_stdout_bytes(bytes, false);
    }) else {
        return Err(CommandLineError::Invalid);
    };
    let arg0 = crate::cleanup::os_str_to_bytes(&args[0]).to_vec();
    if cmd.print_version {
        unsafe {
            libc::printf(
                c"RustyGet version: %s\n".as_ptr(),
                crate::ffi::build_version_cstr().as_ptr(),
            );
            libc::exit(0);
        }
    }
    if cmd.print_usage || cmd.errors || argc <= 1 {
        let base_idx = crate::paths::base_file_name(&arg0);
        let usage = crate::cmdline::format_usage(
            &arg0[base_idx..],
            crate::paths::PATH_SEPARATOR as u8,
            true,
        );
        let usage_c = dm_cbuf(&usage);
        unsafe {
            libc::printf(c"%s".as_ptr(), usage_c.as_ptr().cast::<c_char>());
            libc::exit(if cmd.print_usage { 0 } else { 1 });
        }
    }
    if let Ok(mut g) = DAEMON_INSTANCE.cmdline.lock() {
        *g = Some((arg0, cmd));
    }
    dm_emit_active_log(0, b"Reading options");
    Ok(())
}

/// Applies the parsed command line to the loaded options and to the
/// subsystems that read options at start-up.
#[cfg(unix)]
pub(crate) fn apply_command_line() {
    let Some(cmd) = daemon_cmdline_clone() else {
        return;
    };
    let Some(opts) = crate::options::active_options() else {
        return;
    };
    opts.set_mode_flag(2, cmd.remote_client_mode);
    opts.set_mode_flag(0, cmd.server_mode);
    opts.set_mode_flag(1, cmd.daemon_mode);
    let test_bt = crate::ffi::build_debug_enabled() && cmd.test_backtrace;
    let direct_mode = test_bt
        || cmd.web_get
        || cmd.sig_verify
        || cmd.client_operation != 0
        || cmd.print_options;
    opts.set_mode_flag(3, direct_mode);

    if let Some(ws) = crate::ffi::active_workstate() {
        ws.set_pause_download(cmd.pause_download);
        ws.set_speed_limit(opts.scalars().download_rate);
    }
    if let Some(lg) = crate::log::active_log() {
        lg.init_options_active();
    }
    if opts.has_fatal_error() {
        unsafe { libc::exit(1) };
    }
    if opts.has_config_errors() && cmd.client_operation == 0 {
        // both notices when there are path errors and other configuration errors
        if crate::options::has_pausing_path_errors() {
            dm_emit_active_log(
                3,
                b"Downloading, post-processing and scanning are paused until the bad paths are fixed. \
Fix them in Settings > Paths (or in the config file), then save and reload.",
            );
        }
        if opts.has_other_config_errors() || !crate::options::has_pausing_path_errors() {
            dm_emit_active_log(2, b"Pausing all activities due to errors in configuration");
        }
        if let Some(ws) = crate::ffi::active_workstate() {
            ws.set_pause_download(true);
            ws.set_pause_post_process(true);
            ws.set_pause_scan(true);
        }
    }
    if cmd.client_operation == 0 && (cmd.server_mode || cmd.daemon_mode) && crate::firstrun::no_usable_server() {
        // nothing can download without a news server: say what to do
        dm_emit_active_log(
            3,
            b"Downloading is paused: no news server is set up. Add one in the web interface \
(the setup guide, or Settings > News servers), test it, then save.",
        );
        if let Some(ws) = crate::ffi::active_workstate() {
            ws.set_pause_download(true);
        }
    }
    if let Some(sp) = crate::net::active_server_pool() {
        sp.set_timeout(opts.scalars().article_timeout);
        sp.set_retry_interval(opts.scalars().article_interval);
    }
    crate::ffi::reload_script_templates();
    if crate::articledl::memory_monitor_active() {
        crate::articledl::memory_monitor_init_options(false);
    }
    let umask_raw = opts.scalars().umask as u32;
    // SAFETY: umask only swaps the process file-mode mask.
    let eff_umask = unsafe {
        if crate::options::nzbget_is_valid_umask(umask_raw) {
            let m = umask_raw as libc::mode_t;
            libc::umask(m);
            m
        } else {
            let m = libc::umask(0);
            libc::umask(m);
            m
        }
    } as c_int;
    ACTIVE_UMASK.store(eff_umask as u32, Ordering::Relaxed);
    if crate::ffi::build_debug_enabled() {
        dm_emit_active_log(0, format!("Using {:o} umask", eff_umask as u32).as_bytes());
    }
    if let Some(sc) = crate::scanner::active_scanner() {
        sc.init_options_active();
    }
}

#[cfg(test)]
pub(crate) static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn a_lock_file_in_a_missing_or_closed_folder_is_skipped_not_fatal() {
        for errno in [libc::ENOENT, libc::ENOTDIR] {
            assert!(lock_file_skippable_errno(errno), "{errno}");
        }
        // a permission or read-only error still refuses to start
        for errno in [libc::EACCES, libc::EROFS, libc::ELOOP, libc::EMFILE, libc::ENOSPC, 0] {
            assert!(!lock_file_skippable_errno(errno), "{errno}");
        }
    }

    #[test]
    fn only_a_lock_file_folder_that_is_not_there_counts_as_missing() {
        let base = std::env::temp_dir().join(format!("rustyget_lockdir_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let b = |p: std::path::PathBuf| p.as_os_str().as_encoded_bytes().to_vec();
        assert!(lock_file_folder_missing(&b(base.join("gone").join("rustyget.lock"))));
        // a regular file where the folder should be
        std::fs::write(base.join("file"), b"").unwrap();
        assert!(lock_file_folder_missing(&b(base.join("file").join("rustyget.lock"))));
        // the folder is there (the lock file can simply be created) or the file exists
        assert!(!lock_file_folder_missing(&b(base.join("rustyget.lock"))));
        std::fs::write(base.join("held.lock"), b"1\n").unwrap();
        assert!(!lock_file_folder_missing(&b(base.join("held.lock"))));
        assert!(!lock_file_folder_missing(b""));
        // a folder that cannot be searched is a permission problem, not a missing one
        if unsafe { libc::geteuid() } != 0 {
            use std::os::unix::fs::PermissionsExt as _;
            let closed = base.join("closed");
            std::fs::create_dir_all(closed.join("inner")).unwrap();
            std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).unwrap();
            let missing = lock_file_folder_missing(&b(closed.join("inner").join("rustyget.lock")));
            std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(!missing);
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_phase_conversions() {
        assert_eq!(LifecyclePhase::from(0), LifecyclePhase::Uninitialized);
        assert_eq!(LifecyclePhase::from(1), LifecyclePhase::OptionsLogging);
        assert_eq!(LifecyclePhase::from(2), LifecyclePhase::SubsystemsRecovery);
        assert_eq!(LifecyclePhase::from(3), LifecyclePhase::ServersActive);
        assert_eq!(LifecyclePhase::from(4), LifecyclePhase::ShutdownReload);
        assert_eq!(LifecyclePhase::from(5), LifecyclePhase::Terminated);
        assert_eq!(LifecyclePhase::from(99), LifecyclePhase::Uninitialized);
    }

    #[test]
    fn test_lifecycle_manager_state() {
        let mgr = LifecycleManager {
            phase: AtomicU32::new(LifecyclePhase::Uninitialized as u32),
            reloading: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
        };

        assert_eq!(mgr.get_phase(), LifecyclePhase::Uninitialized);
        assert!(!mgr.is_reloading());
        assert!(!mgr.is_stopping());

        mgr.set_phase(LifecyclePhase::OptionsLogging);
        assert_eq!(mgr.get_phase(), LifecyclePhase::OptionsLogging);

        mgr.set_reloading(true);
        assert!(mgr.is_reloading());
        mgr.set_reloading(false);
        assert!(!mgr.is_reloading());

        mgr.set_stopping(true);
        assert!(mgr.is_stopping());
        mgr.set_stopping(false);
        assert!(!mgr.is_stopping());
    }

    #[test]
    fn test_trigger_reload_stops_during_shutdown() {
        let mgr = LifecycleManager {
            phase: AtomicU32::new(LifecyclePhase::ShutdownReload as u32),
            reloading: AtomicBool::new(false),
            stopping: AtomicBool::new(true),
        };
        mgr.trigger_reload();
        assert!(!mgr.is_reloading());
    }

    static MOCK_CALL_COUNT: AtomicUsize = AtomicUsize::new(0);
    static MOCK_P1_RELOAD: AtomicBool = AtomicBool::new(false);
    static MOCK_P4_RELOAD: AtomicBool = AtomicBool::new(false);
    static MOCK_UNPACK_COUNT: AtomicUsize = AtomicUsize::new(0);
    static RELOAD_SIM_STEP: AtomicUsize = AtomicUsize::new(0);

    /// Hooks that count phase calls. `phase1_rc` is what phase 1 returns;
    /// `reload_once` reports one host reload, then none.
    struct MockHooks {
        phase1_rc: c_int,
        reload_once: bool,
    }

    impl LifecycleHooks for MockHooks {
        fn phase1_init(&self, reload: bool) -> c_int {
            if self.phase1_rc != 0 {
                return self.phase1_rc;
            }
            MOCK_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
            MOCK_P1_RELOAD.store(reload, Ordering::SeqCst);
            0
        }

        fn phase2_subsystems_create_and_recover(&self, _reload: bool) -> Option<c_int> {
            MOCK_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
            Some(0)
        }

        fn phase3_server_activate(&self) -> c_int {
            MOCK_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
            0
        }

        fn phase4_shutdown(&self, reload: bool) {
            MOCK_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
            MOCK_P4_RELOAD.store(reload, Ordering::SeqCst);
        }

        fn has_active_unpack(&self) -> bool {
            if MOCK_UNPACK_COUNT.load(Ordering::SeqCst) > 0 {
                MOCK_UNPACK_COUNT.fetch_sub(1, Ordering::SeqCst);
                true
            } else {
                false
            }
        }

        fn is_reloading(&self) -> bool {
            if self.reload_once && RELOAD_SIM_STEP.load(Ordering::SeqCst) == 0 {
                RELOAD_SIM_STEP.store(1, Ordering::SeqCst);
                true
            } else {
                false
            }
        }
    }

    static MOCK_CLEAN: MockHooks = MockHooks {
        phase1_rc: 0,
        reload_once: false,
    };
    static MOCK_CLIENT_EXIT: MockHooks = MockHooks {
        phase1_rc: 10,
        reload_once: false,
    };
    static MOCK_RELOAD_ONCE: MockHooks = MockHooks {
        phase1_rc: 0,
        reload_once: true,
    };

    fn fresh_manager() -> LifecycleManager {
        LifecycleManager {
            phase: AtomicU32::new(LifecyclePhase::Uninitialized as u32),
            reloading: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
        }
    }

    #[test]
    fn test_daemon_loop_full_clean_cycle() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        MOCK_CALL_COUNT.store(0, Ordering::SeqCst);
        MOCK_P1_RELOAD.store(false, Ordering::SeqCst);
        MOCK_P4_RELOAD.store(false, Ordering::SeqCst);
        MOCK_UNPACK_COUNT.store(0, Ordering::SeqCst);

        register_hooks(&MOCK_CLEAN);
        let mgr = fresh_manager();

        let rc = mgr.run_daemon_loop(1, std::ptr::null_mut(), std::ptr::null_mut());
        assert_eq!(rc, 0);
        assert_eq!(mgr.get_phase(), LifecyclePhase::Terminated);
        assert_eq!(MOCK_CALL_COUNT.load(Ordering::SeqCst), 4);
        assert!(!MOCK_P1_RELOAD.load(Ordering::SeqCst));
        assert!(!MOCK_P4_RELOAD.load(Ordering::SeqCst));
    }

    #[test]
    fn test_daemon_loop_early_exit_client_mode() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        register_hooks(&MOCK_CLIENT_EXIT);
        let mgr = fresh_manager();

        let rc = mgr.run_daemon_loop(1, std::ptr::null_mut(), std::ptr::null_mut());
        assert_eq!(rc, 10);
        assert_eq!(mgr.get_phase(), LifecyclePhase::Terminated);
    }

    #[test]
    fn test_daemon_loop_in_process_reload_and_unpack_deferral() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        RELOAD_SIM_STEP.store(0, Ordering::SeqCst);
        MOCK_UNPACK_COUNT.store(2, Ordering::SeqCst); // 2 active unpack polls
        MOCK_CALL_COUNT.store(0, Ordering::SeqCst);
        MOCK_P1_RELOAD.store(false, Ordering::SeqCst);
        MOCK_P4_RELOAD.store(false, Ordering::SeqCst);

        register_hooks(&MOCK_RELOAD_ONCE);
        let mgr = fresh_manager();

        let rc = mgr.run_daemon_loop(1, std::ptr::null_mut(), std::ptr::null_mut());
        assert_eq!(rc, 0);
        assert_eq!(mgr.get_phase(), LifecyclePhase::Terminated);
        // Ran 2 full cycles (4 calls each = 8 calls)
        assert_eq!(MOCK_CALL_COUNT.load(Ordering::SeqCst), 8);
        assert_eq!(MOCK_UNPACK_COUNT.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_socket_deadline_wait_and_disk_service_and_http_get_helpers() {
        let mut fds = [0i32; 2];
        unsafe {
            assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
        }
        // Write end is immediately writable
        assert!(socket_deadline_wait(fds[1], false, true, || 50));
        // Stopped callback returns false immediately
        assert!(!socket_deadline_wait(fds[0], true, false, || -1));
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }

        let tmp_dir =
            std::env::temp_dir().join(format!("rustyget_batch154_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let existing_sub = tmp_dir.join("existing");
        std::fs::create_dir_all(&existing_sub).unwrap();
        let missing_sub = tmp_dir.join("missing");

        let req_spec = format!("{};{}", existing_sub.display(), missing_sub.display());
        let mut logged = Vec::new();
        let (all_exist, next_reported) = crate::fsops::disk_service_check_required_dirs(
            req_spec.as_bytes(),
            false,
            |dir_cstr| logged.push(dir_cstr.to_string_lossy().into_owned()),
        );
        assert!(!all_exist);
        assert!(next_reported);
        assert_eq!(logged, vec![missing_sub.display().to_string()]);

        std::fs::create_dir_all(&missing_sub).unwrap();
        let (all_exist2, _) = crate::fsops::disk_service_check_required_dirs(
            req_spec.as_bytes(),
            true,
            |_| unreachable!(),
        );
        assert!(all_exist2);

        let tmp_file = tmp_dir.join("http_body.tmp");
        std::fs::write(&tmp_file, b"hello world").unwrap();
        use std::os::unix::ffi::OsStrExt;
        let body = crate::dupesearch_nzb::http_get_read_and_delete(
            tmp_file.as_os_str().as_bytes(),
            1024,
            true,
        );
        assert_eq!(body.as_deref(), Some(&b"hello world"[..]));
        assert!(!tmp_file.exists());

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_reload_coordinator_debounce_unpack_deferral_and_cancel() {
        // RELOAD_COORDINATOR is a process-wide static that the daemon-loop
        // tests also cancel and join, so hold TEST_MUTEX like they do.
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        static TEST_EVENTS: std::sync::Mutex<Vec<c_int>> = std::sync::Mutex::new(Vec::new());
        static TEST_UNPACK_POLLS: AtomicUsize = AtomicUsize::new(0);

        unsafe extern "C" fn test_reload_cb(action: c_int) -> c_int {
            TEST_EVENTS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(action);
            match action {
                0 => 0,
                4 => {
                    if TEST_UNPACK_POLLS.load(Ordering::SeqCst) > 0 {
                        TEST_UNPACK_POLLS.fetch_sub(1, Ordering::SeqCst);
                        1
                    } else {
                        0
                    }
                }
                _ => 0,
            }
        }

        TEST_EVENTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        TEST_UNPACK_POLLS.store(1, Ordering::SeqCst);

        reload_coordinator_op(0, Some(test_reload_cb));
        reload_coordinator_op(0, Some(test_reload_cb));
        reload_coordinator_op(2, None);
        assert!(TEST_EVENTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&2));

        TEST_EVENTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        TEST_UNPACK_POLLS.store(1, Ordering::SeqCst);
        reload_coordinator_op(0, Some(test_reload_cb));
        // Debounce (100 ms) plus one deferral poll (100 ms) normally finish in
        // well under a second; allow 5 s for a loaded CI runner.
        for _ in 0..200 {
            if TEST_EVENTS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(&8)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        reload_coordinator_op(2, None);
        let recorded = TEST_EVENTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        assert!(
            !recorded.contains(&1),
            "unexpected already-stopping log: {recorded:?}"
        );
        assert!(
            recorded.contains(&5),
            "expected unpack deferral log: {recorded:?}"
        );
        assert!(
            recorded.contains(&8),
            "expected Stop(true) trigger: {recorded:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_signal_state_ops_and_shutdown_gating() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        SIG_HUP_PENDING.store(false, Ordering::SeqCst);
        SIG_EXIT_PENDING.store(false, Ordering::SeqCst);
        SHUTDOWN_INITIATED.store(false, Ordering::SeqCst);
        LifecycleManager::global().set_reloading(false);
        LifecycleManager::global().set_stopping(false);

        assert_eq!(signal_state_op(0), 0);
        assert_eq!(signal_state_op(4), 0);

        assert_eq!(record_signal_flags(libc::SIGHUP), 1);
        assert_eq!(signal_state_op(4), 1);
        assert_eq!(signal_state_op(2), 1);
        assert_eq!(signal_state_op(2), 0);
        LifecycleManager::global().set_reloading(false);

        assert_eq!(record_signal_flags(libc::SIGTERM), 2);
        assert_eq!(record_signal_flags(libc::SIGINT), 0);
        assert_eq!(record_signal_flags(libc::SIGHUP), 0);
        assert_eq!(signal_state_op(0), 1);
        assert_eq!(signal_state_op(2), 0);
        assert_eq!(signal_state_op(1), 1);
        assert_eq!(signal_state_op(0), 0);

        SIG_HUP_PENDING.store(false, Ordering::SeqCst);
        SIG_EXIT_PENDING.store(false, Ordering::SeqCst);
        SHUTDOWN_INITIATED.store(false, Ordering::SeqCst);
        LifecycleManager::global().set_reloading(false);
        LifecycleManager::global().set_stopping(false);
    }

    #[test]
    fn test_daemon_reset_stop_and_cleanup_lockfile() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        struct MockHost(std::sync::Mutex<Vec<bool>>);
        impl DaemonHost for MockHost {
            fn has_instance(&self) -> bool {
                true
            }
            fn has_active_unpack(&self) -> bool {
                true
            }
            fn stop(&self, _reload: bool) {}
            fn start_prepost(&self) {}
            fn prepost_running(&self) -> bool {
                false
            }
            fn prepost_has_jobs(&self) -> bool {
                false
            }
            fn stop_prepost_when_idle(&self) {}
            fn stop_prepost(&self, remote_mode: bool) {
                self.0.lock().unwrap().push(remote_mode);
            }
        }

        let lock_path =
            std::env::temp_dir().join(format!("nzbget_rs_dm_lock_{}.lock", std::process::id()));
        std::fs::write(&lock_path, b"123\n").unwrap();
        use std::os::unix::ffi::OsStrExt;
        let mut lock_c = lock_path.as_os_str().as_bytes().to_vec();
        lock_c.push(0);

        {
            let host = MockHost(std::sync::Mutex::new(Vec::new()));
            reset_instance(false);
            assert!(!dm_is_stopped());

            stop_daemon(false, &host);
            assert_eq!(*host.0.lock().unwrap(), vec![false]);
            assert!(dm_is_stopped());

            DAEMON_INSTANCE
                .lock_file_owned
                .store(true, Ordering::SeqCst);
            assert!(may_delete_lock_file());
            crate::fsops::delete_file(std::ffi::CStr::from_bytes_with_nul(&lock_c).unwrap());
            assert!(!lock_path.exists());

            reset_instance(false);
        }
    }
}
