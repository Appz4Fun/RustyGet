//! Subsystem lifecycle coordination and phase state machine (Milestone M5 — Requirement R2).

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

#[cfg(unix)]
extern "C" {
    fn dlsym(handle: *mut std::ffi::c_void, symbol: *const c_char) -> *mut std::ffi::c_void;
}

#[cfg(target_os = "macos")]
const RTLD_DEFAULT: *mut std::ffi::c_void = -2isize as *mut std::ffi::c_void;
#[cfg(all(unix, not(target_os = "macos")))]
const RTLD_DEFAULT: *mut std::ffi::c_void = std::ptr::null_mut();

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

/// Function table of C++ subsystem lifecycle bridge hooks.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LifecycleHooks {
    pub set_process_args: Option<unsafe extern "C" fn(c_int, *mut *mut c_char, *mut *mut c_char)>,
    pub util_init: Option<unsafe extern "C" fn()>,
    pub nserv_main: Option<unsafe extern "C" fn(c_int, *mut *mut c_char) -> c_int>,
    pub phase1_init: Option<unsafe extern "C" fn(bool) -> c_int>,
    pub phase2_subsystems_create_and_recover: Option<unsafe extern "C" fn(bool) -> c_int>,
    pub phase3_server_activate: Option<unsafe extern "C" fn() -> c_int>,
    pub phase4_shutdown: Option<unsafe extern "C" fn(bool)>,
    pub has_active_unpack: Option<unsafe extern "C" fn() -> bool>,
    pub is_reloading: Option<unsafe extern "C" fn() -> bool>,
    pub stop_server: Option<unsafe extern "C" fn(bool)>,
    pub reload: Option<unsafe extern "C" fn()>,
    pub exit_proc: Option<unsafe extern "C" fn()>,
}

static REGISTERED_HOOKS: RwLock<Option<LifecycleHooks>> = RwLock::new(None);

pub fn register_hooks(hooks: LifecycleHooks) {
    if let Ok(mut lock) = REGISTERED_HOOKS.write() {
        *lock = Some(hooks);
    }
}

pub fn get_hooks() -> Option<LifecycleHooks> {
    REGISTERED_HOOKS.read().ok().and_then(|guard| *guard)
}

#[cfg(unix)]
unsafe fn resolve_sym<T: Copy>(name: &[u8]) -> Option<T> {
    let sym = dlsym(RTLD_DEFAULT, name.as_ptr() as *const c_char);
    if sym.is_null() {
        None
    } else {
        Some(std::mem::transmute_copy(&sym))
    }
}

pub fn resolve_hooks() -> Option<LifecycleHooks> {
    #[cfg(unix)]
    unsafe {
        let set_process_args = resolve_sym(b"nzbget_c_set_process_args\0");
        let util_init = resolve_sym(b"nzbget_c_util_init\0");
        let nserv_main = resolve_sym(b"nzbget_c_nserv_main\0");
        let phase1_init = resolve_sym(b"nzbget_c_phase1_init\0");
        let phase2_subsystems_create_and_recover =
            resolve_sym(b"nzbget_c_phase2_subsystems_create_and_recover\0");
        let phase3_server_activate = resolve_sym(b"nzbget_c_phase3_server_activate\0");
        let phase4_shutdown = resolve_sym(b"nzbget_c_phase4_shutdown\0");
        let has_active_unpack = resolve_sym(b"nzbget_c_has_active_unpack\0");
        let is_reloading = resolve_sym(b"nzbget_c_is_reloading\0");
        let stop_server = resolve_sym(b"nzbget_c_stop_server\0");
        let reload = resolve_sym(b"nzbget_c_reload\0");
        let exit_proc = resolve_sym(b"nzbget_c_exit_proc\0");

        if phase1_init.is_some() && phase3_server_activate.is_some() {
            Some(LifecycleHooks {
                set_process_args,
                util_init,
                nserv_main,
                phase1_init,
                phase2_subsystems_create_and_recover,
                phase3_server_activate,
                phase4_shutdown,
                has_active_unpack,
                is_reloading,
                stop_server,
                reload,
                exit_proc,
            })
        } else {
            None
        }
    }
    #[cfg(not(unix))]
    {
        None
    }
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
        if let Some(hooks) = get_hooks().or_else(resolve_hooks) {
            if let Some(reload_fn) = hooks.reload {
                unsafe { reload_fn() };
            }
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
        if let Some(hooks) = get_hooks().or_else(resolve_hooks) {
            if let Some(exit_fn) = hooks.exit_proc {
                unsafe { exit_fn() };
            }
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
        let hooks = get_hooks()
            .or_else(resolve_hooks)
            .expect("Lifecycle hooks must be registered");

        let mut reloading = false;
        let mut exit_code = 0;

        while !self.is_stopping() {
            // Phase 1: Platform & Preflight, Options & Logging
            self.set_phase(LifecyclePhase::OptionsLogging);
            let p1_fn = hooks.phase1_init.expect("phase1_init hook required");
            let p1_rc = unsafe { p1_fn(reloading) };
            if p1_rc != 0 {
                // Non-zero return indicates early exit or client operation complete
                exit_code = if p1_rc > 0 { p1_rc } else { 0 };
                break;
            }

            // Phase 2: Core Subsystems Creation & State Recovery
            self.set_phase(LifecyclePhase::SubsystemsRecovery);
            if let Some(p2_fn) = hooks.phase2_subsystems_create_and_recover {
                let p2_rc = unsafe { p2_fn(reloading) };
                if p2_rc != 0 {
                    exit_code = p2_rc;
                    break;
                }
            }

            // Phase 3: Server Activation & Wait
            self.set_phase(LifecyclePhase::ServersActive);
            let p3_fn = hooks.phase3_server_activate.expect("phase3 hook required");
            let p3_rc = unsafe { p3_fn() };
            if p3_rc != 0 {
                exit_code = p3_rc;
            }

            // Phase 4: Orderly Teardown & In-Process Reload Looping
            self.set_phase(LifecyclePhase::ShutdownReload);

            // Active unpack deferral check (Requirement R3):
            // If in-process reload is requested, ensure active unpack finishes before tearing down
            let c_reloading = hooks.is_reloading.map(|f| unsafe { f() }).unwrap_or(false);
            let mut is_reload = !self.is_stopping()
                && if hooks.reload.is_some() && hooks.is_reloading.is_some() {
                    c_reloading
                } else {
                    self.is_reloading() || c_reloading
                };

            if is_reload {
                if let Some(unpack_fn) = hooks.has_active_unpack {
                    while !self.is_stopping() && unsafe { unpack_fn() } {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
                if self.is_stopping()
                    || (hooks.reload.is_some()
                        && hooks.is_reloading.is_some()
                        && !hooks.is_reloading.map(|f| unsafe { f() }).unwrap_or(false))
                {
                    is_reload = false;
                }
            }

            if let Some(p4_fn) = hooks.phase4_shutdown {
                unsafe { p4_fn(is_reload) };
            }

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
/// Returns `Ok((lock_file_owned, truncate_failed))` on success, or `Err(error_code)` where:
/// - `1`: could not create lock-file
/// - `2`: lock-file is not a regular file
/// - `3`: could not acquire lock on lock-file
/// - `4`: invalid `DaemonUsername`
/// - `5`: could not drop privileges
pub fn daemonize_setup(
    lock_file: &std::ffi::CStr,
    daemon_username: &std::ffi::CStr,
) -> Result<(bool, bool), i32> {
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

        if !lock_file.to_bytes().is_empty() {
            let flags = libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC;
            lfp = libc::open(lock_file.as_ptr(), flags, 0o640 as libc::c_uint);
            if lfp < 0 {
                return Err(1);
            }

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

        Ok((lock_file_owned, truncate_failed))
    }
    #[cfg(not(unix))]
    {
        let _ = (lock_file, daemon_username);
        Ok((false, false))
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

#[cfg(test)]
pub(crate) static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

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

    unsafe extern "C" fn mock_phase1(reload: bool) -> c_int {
        MOCK_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
        MOCK_P1_RELOAD.store(reload, Ordering::SeqCst);
        0
    }

    unsafe extern "C" fn mock_phase2(_reload: bool) -> c_int {
        MOCK_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
        0
    }

    unsafe extern "C" fn mock_phase3() -> c_int {
        MOCK_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
        0
    }

    unsafe extern "C" fn mock_phase4(reload: bool) {
        MOCK_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
        MOCK_P4_RELOAD.store(reload, Ordering::SeqCst);
    }

    unsafe extern "C" fn mock_has_active_unpack() -> bool {
        if MOCK_UNPACK_COUNT.load(Ordering::SeqCst) > 0 {
            MOCK_UNPACK_COUNT.fetch_sub(1, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    unsafe extern "C" fn mock_is_reloading() -> bool {
        false
    }

    #[test]
    fn test_daemon_loop_full_clean_cycle() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        MOCK_CALL_COUNT.store(0, Ordering::SeqCst);
        MOCK_P1_RELOAD.store(false, Ordering::SeqCst);
        MOCK_P4_RELOAD.store(false, Ordering::SeqCst);
        MOCK_UNPACK_COUNT.store(0, Ordering::SeqCst);

        let hooks = LifecycleHooks {
            set_process_args: None,
            util_init: None,
            nserv_main: None,
            phase1_init: Some(mock_phase1),
            phase2_subsystems_create_and_recover: Some(mock_phase2),
            phase3_server_activate: Some(mock_phase3),
            phase4_shutdown: Some(mock_phase4),
            has_active_unpack: Some(mock_has_active_unpack),
            is_reloading: Some(mock_is_reloading),
            stop_server: None,
            reload: None,
            exit_proc: None,
        };

        register_hooks(hooks);

        let mgr = LifecycleManager {
            phase: AtomicU32::new(LifecyclePhase::Uninitialized as u32),
            reloading: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
        };

        let rc = mgr.run_daemon_loop(1, std::ptr::null_mut(), std::ptr::null_mut());
        assert_eq!(rc, 0);
        assert_eq!(mgr.get_phase(), LifecyclePhase::Terminated);
        assert_eq!(MOCK_CALL_COUNT.load(Ordering::SeqCst), 4);
        assert!(!MOCK_P1_RELOAD.load(Ordering::SeqCst));
        assert!(!MOCK_P4_RELOAD.load(Ordering::SeqCst));
    }

    unsafe extern "C" fn mock_phase1_client_exit(_reload: bool) -> c_int {
        10
    }

    #[test]
    fn test_daemon_loop_early_exit_client_mode() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let hooks = LifecycleHooks {
            set_process_args: None,
            util_init: None,
            nserv_main: None,
            phase1_init: Some(mock_phase1_client_exit),
            phase2_subsystems_create_and_recover: Some(mock_phase2),
            phase3_server_activate: Some(mock_phase3),
            phase4_shutdown: Some(mock_phase4),
            has_active_unpack: Some(mock_has_active_unpack),
            is_reloading: Some(mock_is_reloading),
            stop_server: None,
            reload: None,
            exit_proc: None,
        };

        register_hooks(hooks);

        let mgr = LifecycleManager {
            phase: AtomicU32::new(LifecyclePhase::Uninitialized as u32),
            reloading: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
        };

        let rc = mgr.run_daemon_loop(1, std::ptr::null_mut(), std::ptr::null_mut());
        assert_eq!(rc, 10);
        assert_eq!(mgr.get_phase(), LifecyclePhase::Terminated);
    }

    static RELOAD_SIM_STEP: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn mock_is_reloading_once() -> bool {
        if RELOAD_SIM_STEP.load(Ordering::SeqCst) == 0 {
            RELOAD_SIM_STEP.store(1, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    #[test]
    fn test_daemon_loop_in_process_reload_and_unpack_deferral() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        RELOAD_SIM_STEP.store(0, Ordering::SeqCst);
        MOCK_UNPACK_COUNT.store(2, Ordering::SeqCst); // 2 active unpack polls
        MOCK_CALL_COUNT.store(0, Ordering::SeqCst);
        MOCK_P1_RELOAD.store(false, Ordering::SeqCst);
        MOCK_P4_RELOAD.store(false, Ordering::SeqCst);

        let hooks = LifecycleHooks {
            set_process_args: None,
            util_init: None,
            nserv_main: None,
            phase1_init: Some(mock_phase1),
            phase2_subsystems_create_and_recover: Some(mock_phase2),
            phase3_server_activate: Some(mock_phase3),
            phase4_shutdown: Some(mock_phase4),
            has_active_unpack: Some(mock_has_active_unpack),
            is_reloading: Some(mock_is_reloading_once),
            stop_server: None,
            reload: None,
            exit_proc: None,
        };

        register_hooks(hooks);

        let mgr = LifecycleManager {
            phase: AtomicU32::new(LifecyclePhase::Uninitialized as u32),
            reloading: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
        };

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
        for _ in 0..40 {
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
}
