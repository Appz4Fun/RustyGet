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

static ACTIVE_UMASK: AtomicU32 = AtomicU32::new(0);

pub(crate) fn active_umask() -> u32 {
    ACTIVE_UMASK.load(Ordering::Relaxed)
}

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
            } else {
                #[cfg(unix)]
                if crate::options::active_options().is_some_and(|o| !o.remote_client_mode()) {
                    if let Some(sp) = crate::net::active_server_pool() {
                        sp.init_connections();
                    }
                    crate::statmeter::init_daemon_statmeter();
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

// ============================================================================
// End-to-end `DiskService` & `RustyGet` daemon state/loop coordinator
// ============================================================================

#[repr(C)]
pub struct NzbgetRsDaemonEnv {
    pub ctx: *mut std::ffi::c_void,
    pub a1: c_int,
    pub a2: c_int,
    pub a3: c_int,
    pub a4: c_int,
    pub a5: c_int,
    pub a6: c_int,
    pub s1: *const c_char,
    pub s2: *const c_char,
    pub s3: *const c_char,
    pub s4: *const c_char,
    pub s5: *const c_char,
    pub segv_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, *const c_char)>,
    pub global_log_cb: Option<unsafe extern "C" fn(c_int, *const c_char)>,
    pub reload_cb: Option<unsafe extern "C" fn(c_int) -> c_int>,
    pub log_cb: Option<unsafe extern "C" fn(*mut std::ffi::c_void, c_int, *const c_char)>,
    pub action_cb: Option<
        unsafe extern "C" fn(
            *mut std::ffi::c_void,
            c_int,
            c_int,
            *const c_char,
            *const c_char,
        ) -> c_int,
    >,
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
        let ctrl_ip = opts.as_deref().map_or(&[][..], |o| o.str_bytes(14));
        let ctrl_port = opts.as_deref().map_or(0, |o| o.scalars().control_port);
        let ctrl_user = opts.as_deref().map_or(&[][..], |o| o.str_bytes(15));
        let ctrl_pass = opts.as_deref().map_or(&[][..], |o| o.str_bytes(16));
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

unsafe fn dm_cstr_bytes<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        std::ffi::CStr::from_ptr(p).to_bytes()
    }
}

unsafe fn dm_opt_cstr<'a>(p: *const c_char) -> &'a std::ffi::CStr {
    if p.is_null() {
        c""
    } else {
        std::ffi::CStr::from_ptr(p)
    }
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

unsafe fn dm_log(env: &NzbgetRsDaemonEnv, level: c_int, msg: &[u8]) {
    if let Some(cb) = env.log_cb {
        let c = dm_cbuf(msg);
        cb(env.ctx, level, c.as_ptr() as *const c_char);
    } else if let Some(gcb) = env.global_log_cb {
        let c = dm_cbuf(msg);
        gcb(level, c.as_ptr() as *const c_char);
    } else {
        dm_emit_active_log(level, msg);
    }
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

unsafe fn dm_trigger_exit(env: &NzbgetRsDaemonEnv) {
    RELOAD_COORDINATOR.cancel_pending();
    signal_state_op(5);
    if let Some(rcb) = env.reload_cb {
        rcb(3);
    } else if let Some(acb) = env.action_cb {
        acb(env.ctx, 10, 0, std::ptr::null(), std::ptr::null());
    }
}

unsafe fn dm_trigger_reload(
    reload_cb: Option<unsafe extern "C" fn(c_int) -> c_int>,
    global_log_cb: Option<unsafe extern "C" fn(c_int, *const c_char)>,
) {
    let Some(rcb) = reload_cb else {
        return;
    };
    RELOAD_COORDINATOR.request_reload(move |action| match action {
        0 => {
            let flags = rcb(0);
            let has_inst = (flags & 1) != 0;
            c_int::from(
                !has_inst || dm_is_reloading() || dm_is_stopped() || signal_state_op(0) != 0,
            )
        }
        1 => {
            if let Some(lcb) = global_log_cb {
                lcb(0, c"Daemon is already reloading or stopping".as_ptr());
            } else {
                dm_emit_active_log(0, b"Daemon is already reloading or stopping");
            }
            0
        }
        2 => {
            if let Some(lcb) = global_log_cb {
                lcb(0, c"Reload request debounced, target extended".as_ptr());
            } else {
                dm_emit_active_log(0, b"Reload request debounced, target extended");
            }
            0
        }
        3 => signal_state_op(0),
        4 => {
            let flags = rcb(0);
            let has_inst = (flags & 1) != 0;
            let active_unpack = (flags & 2) != 0;
            c_int::from(active_unpack) | (if !has_inst || dm_is_reloading() { 2 } else { 0 })
        }
        5 => {
            if let Some(lcb) = global_log_cb {
                lcb(
                    2,
                    c"Unpack is actively executing; deferring reload until unpack completes..."
                        .as_ptr(),
                );
            } else {
                dm_emit_active_log(
                    2,
                    b"Unpack is actively executing; deferring reload until unpack completes...",
                );
            }
            0
        }
        6 => signal_state_op(1),
        7 => {
            RELOAD_COORDINATOR.cancel_pending();
            signal_state_op(5);
            rcb(3);
            0
        }
        8 => {
            let flags = rcb(0);
            if (flags & 1) != 0 && !dm_is_reloading() {
                if let Some(lcb) = global_log_cb {
                    lcb(2, c"Reloading...".as_ptr());
                } else {
                    dm_emit_active_log(2, b"Reloading...");
                }
                rcb(1);
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
        let dest_c = opts.as_ref().and_then(|o| o.str_cstr(3)).unwrap_or(c"");
        let dest_b = opts.as_ref().map_or(&[][..], |o| o.str_bytes(3));
        let inter_c = opts.as_ref().and_then(|o| o.str_cstr(4));
        let inter_b = opts.as_ref().map_or(&[][..], |o| o.str_bytes(4));
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
        let req_b = opts.as_ref().map_or(&[][..], |o| o.str_bytes(11));
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

#[cfg(unix)]
pub(crate) fn should_start_secure_server_active() -> bool {
    let Some(opts) = crate::options::active_options() else {
        return false;
    };
    crate::options::nzbget_should_start_secure_server(
        opts.scalars().secure_control != 0,
        true,
        opts.str_bytes(14),
    )
}

/// Executes `DiskService` and `RustyGet` daemon lifecycle operations (`daemon/main/rustyget.cpp`):
/// - `0`: `DiskService::ServiceInterval`
/// - `1`: `DiskService::ServiceWork`
/// - `2`: `ResetInstance(a1 = reload)`
/// - `3`: `QueryState` (`0`: `reloading`, `1`: `stopped`, `2`: `signal_state_op(a2)`)
/// - `4`: `Phase1ApplyUmask(a1 = opt_umask, a2 = is_debug)`
/// - `5`: `Phase1DaemonizeAndActivate`
/// - `6`: `ShouldStartSecureServer`
/// - `7`: `Phase3Run` (standalone/server `DoMainLoop` + `remote-mode` loop)
/// - `8`: `Stop(a1 = reload, a2 = remote_mode)`
/// - `9`: `Reload()`
/// - `10`: `ExitProc()`
/// - `11`: `Phase4Shutdown`
/// - `12`: `CleanupLockFile`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nzbget_rs_daemon_exec(op: c_int, env: *const NzbgetRsDaemonEnv) -> c_int {
    let Some(e) = env.as_ref() else {
        return 0;
    };
    match op {
        0 => {
            let waiting = DAEMON_INSTANCE
                .disk
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .waiting_required_dir;
            if waiting || (e.a1 > 0 && e.a2 != 0) {
                1
            } else {
                -1
            }
        }
        1 => {
            dm_log(e, 0, b"Disk service work");
            if e.a1 > 0 && e.a2 != 0 {
                dm_log(e, 0, b"Disk service work: check disk space");
                let inter_opt = if e.s2.is_null() {
                    None
                } else {
                    Some(std::ffi::CStr::from_ptr(e.s2))
                };
                let (dest_low, inter_low) =
                    crate::fsops::disk_service_check_disk_space(dm_opt_cstr(e.s1), inter_opt, e.a1);
                if dest_low {
                    let mut wm = b"Low disk space on ".to_vec();
                    wm.extend_from_slice(dm_cstr_bytes(e.s1));
                    wm.extend_from_slice(b". Pausing download");
                    dm_log(e, 3, &wm);
                    if let Some(cb) = e.action_cb {
                        cb(e.ctx, 0, 0, std::ptr::null(), std::ptr::null());
                    }
                }
                if inter_low {
                    let mut wm = b"Low disk space on ".to_vec();
                    wm.extend_from_slice(dm_cstr_bytes(e.s2));
                    wm.extend_from_slice(b". Pausing download");
                    dm_log(e, 3, &wm);
                    if let Some(cb) = e.action_cb {
                        cb(e.ctx, 0, 0, std::ptr::null(), std::ptr::null());
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
                dm_log(e, 0, b"Disk service work: check required dir");
                let (all_exist, next_reported) = crate::fsops::disk_service_check_required_dirs(
                    dm_cstr_bytes(e.s3),
                    prev_reported,
                    |dir_cstr| {
                        let mut im = b"Waiting for required directory ".to_vec();
                        im.extend_from_slice(dir_cstr.to_bytes());
                        dm_log(e, 2, &im);
                    },
                );
                if !all_exist {
                    DAEMON_INSTANCE
                        .disk
                        .lock()
                        .unwrap_or_else(|err| err.into_inner())
                        .waiting_reported = next_reported;
                    return 0;
                }
                if prev_reported {
                    dm_log(e, 2, b"All required directories available");
                }
                if let Some(cb) = e.action_cb {
                    cb(e.ctx, 1, 0, std::ptr::null(), std::ptr::null());
                }
                let mut dg = DAEMON_INSTANCE
                    .disk
                    .lock()
                    .unwrap_or_else(|err| err.into_inner());
                dg.waiting_required_dir = false;
                dg.waiting_reported = next_reported;
            }
            1
        }
        2 => {
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
            wg.reloading = e.a1 != 0;
            wg.stopped = false;
            1
        }
        3 => match e.a1 {
            0 => c_int::from(dm_is_reloading()),
            1 => c_int::from(dm_is_stopped()),
            2 => signal_state_op(e.a2),
            _ => 0,
        },
        4 => {
            #[cfg(unix)]
            let (umask_raw, is_debug) = if e.a1 < 0 {
                (
                    crate::options::active_options().map_or(0o1000, |o| o.scalars().umask),
                    crate::ffi::build_debug_enabled(),
                )
            } else {
                (e.a1, e.a2 != 0)
            };
            #[cfg(unix)]
            let eff = {
                let umask_val = umask_raw as u32;
                let res = if crate::options::nzbget_is_valid_umask(umask_val) {
                    let m = umask_val as libc::mode_t;
                    libc::umask(m);
                    m
                } else {
                    let m = libc::umask(0);
                    libc::umask(m);
                    m
                };
                res as c_int
            };
            #[cfg(not(unix))]
            let (eff, is_debug) = (0, e.a2 != 0);

            ACTIVE_UMASK.store(eff as u32, Ordering::Relaxed);
            if is_debug {
                let msg = format!("Using {:o} umask", eff as u32);
                dm_log(e, 0, msg.as_bytes());
            }
            eff
        }
        5 => {
            #[cfg(unix)]
            let (daemon_mode, remote_mode) = if e.s1.is_null() && e.a1 < 0 {
                if let Some(cmd) = daemon_cmdline_clone() {
                    daemon_process_direct(&cmd);
                    (cmd.daemon_mode, cmd.remote_client_mode)
                } else {
                    (false, false)
                }
            } else {
                (e.a1 != 0, e.a3 != 0)
            };
            #[cfg(not(unix))]
            let (daemon_mode, remote_mode) = (e.a1 != 0, e.a3 != 0);

            let was_reloading = dm_is_reloading();
            #[cfg(unix)]
            let active_opts = if e.s1.is_null() {
                crate::options::active_options()
            } else {
                None
            };
            #[cfg(unix)]
            if let Some(opts) = active_opts.as_ref() {
                if opts.server_mode() && opts.scalars().system_health_check != 0 {
                    let _ = crate::ffi::system_health_diagnose_bytes(0);
                }
            }
            #[cfg(unix)]
            let (
                ver,
                lock_file,
                daemon_user,
                server_mode,
                cfg_fn,
                ctrl_ip,
                ctrl_port,
                crash_dump,
                crash_trace,
            ) = if let Some(opts) = active_opts.as_ref() {
                let sc = opts.scalars();
                (
                    crate::ffi::build_version_bytes(),
                    opts.str_cstr(25).unwrap_or(c""),
                    opts.str_cstr(26).unwrap_or(c""),
                    opts.server_mode(),
                    opts.str_bytes(0),
                    opts.str_bytes(14),
                    sc.control_port,
                    sc.crash_dump != 0,
                    crate::ffi::build_debug_enabled() && sc.crash_trace != 0,
                )
            } else {
                (
                    dm_cstr_bytes(e.s1),
                    dm_opt_cstr(e.s2),
                    dm_opt_cstr(e.s3),
                    e.a2 != 0,
                    dm_cstr_bytes(e.s4),
                    dm_cstr_bytes(e.s5),
                    e.a6,
                    e.a4 != 0,
                    e.a5 != 0,
                )
            };
            #[cfg(not(unix))]
            let (
                ver,
                lock_file,
                daemon_user,
                server_mode,
                cfg_fn,
                ctrl_ip,
                ctrl_port,
                crash_dump,
                crash_trace,
            ) = (
                dm_cstr_bytes(e.s1),
                dm_opt_cstr(e.s2),
                dm_opt_cstr(e.s3),
                e.a2 != 0,
                dm_cstr_bytes(e.s4),
                dm_cstr_bytes(e.s5),
                e.a6,
                e.a4 != 0,
                e.a5 != 0,
            );

            if daemon_mode {
                if !was_reloading {
                    #[cfg(unix)]
                    {
                        let f = libc::fork();
                        if f < 0 {
                            libc::exit(1);
                        }
                        if f > 0 {
                            libc::exit(0);
                        }
                        libc::setsid();
                    }
                    match daemonize_setup(lock_file, daemon_user) {
                        Ok((owned, trunc_failed)) => {
                            DAEMON_INSTANCE
                                .lock_file_owned
                                .store(owned, Ordering::SeqCst);
                            if trunc_failed {
                                let mut wm = b"Could not truncate lock-file ".to_vec();
                                wm.extend_from_slice(lock_file.to_bytes());
                                dm_log(e, 3, &wm);
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
                            dm_log(e, 4, &em);
                            libc::exit(1);
                        }
                    }
                }
                let mut im = b"RustyGet ".to_vec();
                im.extend_from_slice(ver);
                im.extend_from_slice(b" daemon-mode");
                dm_log(e, 2, &im);
            } else if server_mode {
                let mut im = b"RustyGet ".to_vec();
                im.extend_from_slice(ver);
                im.extend_from_slice(b" server-mode");
                dm_log(e, 2, &im);
            } else if remote_mode {
                let mut im = b"RustyGet ".to_vec();
                im.extend_from_slice(ver);
                im.extend_from_slice(b" remote-mode");
                dm_log(e, 2, &im);
            }

            if server_mode {
                let mut u = b"using ".to_vec();
                u.extend_from_slice(cfg_fn);
                dm_log(e, 2, &u);

                let mut r = b"RustyGet runs on ".to_vec();
                r.extend_from_slice(ctrl_ip);
                r.extend_from_slice(format!(":{ctrl_port}").as_bytes());
                dm_log(e, 2, &r);
            }

            #[cfg(unix)]
            if e.s1.is_null() && crate::ffi::build_debug_enabled() {
                let sys_bytes = crate::ffi::format_system_info_bytes(0, false);
                let mut pos = 0;
                while pos < sys_bytes.len() {
                    let end = sys_bytes[pos..]
                        .iter()
                        .position(|&b| b == b'\n')
                        .map_or(sys_bytes.len(), |d| pos + d);
                    dm_emit_active_log(1, &sys_bytes[pos..end]);
                    if end == sys_bytes.len() {
                        break;
                    }
                    pos = end + 1;
                }
            }
            if let Some(cb) = e.action_cb {
                cb(e.ctx, 2, 0, std::ptr::null(), std::ptr::null());
            }

            DAEMON_INSTANCE
                .wait_mutex
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .reloading = false;

            install_error_handler(crash_dump, crash_trace, e.segv_cb);
            0
        }
        6 => c_int::from(crate::options::nzbget_should_start_secure_server(
            e.a1 != 0,
            cfg!(unix),
            dm_cstr_bytes(e.s1),
        )),
        7 => {
            let Some(cb) = e.action_cb else {
                return 0;
            };
            #[cfg(unix)]
            if e.a1 < 0 {
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
                            let rcb = e.reload_cb;
                            let acb = e.action_cb;
                            let ctx_u = e.ctx as usize;
                            let exit_fn: std::sync::Arc<dyn Fn() + Send + Sync> =
                                std::sync::Arc::new(move || unsafe {
                                    RELOAD_COORDINATOR.cancel_pending();
                                    signal_state_op(5);
                                    if let Some(r) = rcb {
                                        r(3);
                                    } else if let Some(a) = acb {
                                        a(
                                            ctx_u as *mut std::ffi::c_void,
                                            10,
                                            0,
                                            std::ptr::null(),
                                            std::ptr::null(),
                                        );
                                    }
                                });
                            let fe = std::sync::Arc::new(crate::ffi::FrontendHandle::new(
                                m,
                                Some(exit_fn),
                            ));
                            fe.start();
                            if let Ok(mut g) = DAEMON_FRONTEND.lock() {
                                *g = Some(fe);
                            }
                        }
                    }
                }
            }
            #[cfg(unix)]
            let cmd_opt = if e.a1 < 0 {
                daemon_cmdline_clone()
            } else {
                None
            };
            #[cfg(unix)]
            let (remote_mode, server_mode, standalone_add, cat_ptr, nzb_ptr) =
                if let Some(ref cmd) = cmd_opt {
                    let srv_mode =
                        crate::options::active_options().is_some_and(|o| o.server_mode());
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
                    (e.a1 != 0, e.a2 != 0, e.a3 != 0, e.s1, e.s2)
                };
            #[cfg(not(unix))]
            let (remote_mode, server_mode, standalone_add, cat_ptr, nzb_ptr) =
                (e.a1 != 0, e.a2 != 0, e.a3 != 0, e.s1, e.s2);

            if !remote_mode {
                if standalone_add && signal_state_op(0) == 0 {
                    #[cfg(unix)]
                    if e.a1 < 0 {
                        let _ = crate::scanner::add_cli_nzb_file_active(cat_ptr, nzb_ptr);
                    } else {
                        cb(e.ctx, 11, 0, cat_ptr, nzb_ptr);
                    }
                    #[cfg(not(unix))]
                    cb(e.ctx, 11, 0, cat_ptr, nzb_ptr);
                }

                dm_log(e, 0, b"Entering main program loop");
                cb(e.ctx, 4, 0, std::ptr::null(), std::ptr::null());
                #[cfg(unix)]
                if e.a1 < 0 {
                    crate::articledl::start_daemon_article_cache();
                    crate::scheduler::start_daemon_service_coord();
                    crate::dupesearch_coord::start_daemon_dupesearch();
                    crate::urlcoord::start_daemon_url_coord();
                    crate::feed::start_daemon_feed_coord();
                    crate::articledl::start_daemon_memory_monitor();
                }

                loop {
                    let mut st_mask = cb(e.ctx, 5, 0, std::ptr::null(), std::ptr::null());
                    #[cfg(unix)]
                    if e.a1 < 0
                        && (crate::articledl::is_daemon_article_cache_running()
                            || crate::scheduler::is_daemon_service_coord_running()
                            || crate::dupesearch_coord::is_daemon_dupesearch_running()
                            || crate::urlcoord::is_daemon_url_coord_running()
                            || crate::feed::is_daemon_feed_coord_running()
                            || crate::articledl::is_daemon_memory_monitor_running())
                    {
                        st_mask |= 1;
                    }
                    if (st_mask & 1) == 0 {
                        break;
                    }
                    if signal_state_op(1) != 0 {
                        dm_trigger_exit(e);
                    } else if signal_state_op(2) != 0 {
                        dm_trigger_reload(e.reload_cb, e.global_log_cb);
                    }

                    let mut st_jobs = cb(e.ctx, 5, 1, std::ptr::null(), std::ptr::null());
                    #[cfg(unix)]
                    if e.a1 < 0 && crate::urlcoord::daemon_url_coord_has_more_jobs() {
                        st_jobs |= 2;
                    }
                    if crate::options::nzbget_should_stop_standalone(
                        server_mode,
                        (st_jobs & 1) != 0,
                        (st_jobs & 2) != 0,
                        (st_jobs & 4) != 0,
                    ) {
                        cb(e.ctx, 6, 0, std::ptr::null(), std::ptr::null());
                        #[cfg(unix)]
                        if e.a1 < 0 {
                            crate::articledl::stop_daemon_article_cache();
                            crate::scheduler::stop_daemon_service_coord();
                            crate::dupesearch_coord::stop_daemon_dupesearch();
                            crate::urlcoord::stop_daemon_url_coord();
                            crate::feed::stop_daemon_feed_coord();
                            crate::articledl::stop_daemon_memory_monitor();
                        }
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
                            if signal_state_op(1) != 0 {
                                dm_trigger_exit(e);
                            } else if signal_state_op(2) != 0 {
                                dm_trigger_reload(e.reload_cb, e.global_log_cb);
                            }
                        }
                    }
                }

                dm_log(e, 0, b"Main program loop terminated");
            } else {
                if signal_state_op(1) != 0 {
                    dm_trigger_exit(e);
                }
                while !dm_is_stopped() && !dm_is_reloading() && {
                    #[cfg(unix)]
                    let running = if e.a1 < 0 {
                        daemon_frontend_clone().is_some_and(|fe| fe.is_running())
                    } else {
                        cb(e.ctx, 3, 0, std::ptr::null(), std::ptr::null()) != 0
                    };
                    #[cfg(not(unix))]
                    let running = cb(e.ctx, 3, 0, std::ptr::null(), std::ptr::null()) != 0;
                    running
                } {
                    if signal_state_op(1) != 0 {
                        dm_trigger_exit(e);
                    } else if signal_state_op(2) != 0 {
                        dm_trigger_reload(e.reload_cb, e.global_log_cb);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            0
        }
        8 => {
            #[cfg(unix)]
            let eff_remote_mode = if e.a2 < 0 {
                c_int::from(daemon_cmdline_clone().is_some_and(|c| c.remote_client_mode))
            } else {
                e.a2
            };
            #[cfg(not(unix))]
            let eff_remote_mode = e.a2;
            let reload = e.a1 != 0;
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
                    dm_log(
                        e,
                        0,
                        b"Ignoring reload request because shutdown is already in progress",
                    );
                    return 0;
                }
                wg.reloading = reload;
            }

            if !dm_is_reloading() {
                signal_state_op(3);
                dm_log(e, 2, b"Stopping, please wait...");
            }

            if let Some(cb) = e.action_cb {
                if dm_is_reloading() && cb(e.ctx, 7, 0, std::ptr::null(), std::ptr::null()) != 0 {
                    dm_log(
                        e,
                        2,
                        b"Waiting for active unpack to complete before in-process reload...",
                    );
                    while cb(e.ctx, 7, 0, std::ptr::null(), std::ptr::null()) != 0
                        && signal_state_op(0) == 0
                    {
                        std::thread::sleep(Duration::from_millis(100));
                    }
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

            #[cfg(unix)]
            if e.a2 < 0 && eff_remote_mode != 0 {
                if let Some(fe) = daemon_frontend_clone() {
                    dm_log(e, 0, b"Stopping Frontend");
                    fe.stop();
                }
            }
            if let Some(cb) = e.action_cb {
                cb(
                    e.ctx,
                    8,
                    eff_remote_mode,
                    std::ptr::null(),
                    std::ptr::null(),
                );
            }
            #[cfg(unix)]
            if e.a2 < 0 && eff_remote_mode == 0 {
                crate::articledl::stop_daemon_article_cache();
                crate::scheduler::stop_daemon_service_coord();
                crate::dupesearch_coord::stop_daemon_dupesearch();
                crate::urlcoord::stop_daemon_url_coord();
                crate::feed::stop_daemon_feed_coord();
                crate::articledl::stop_daemon_memory_monitor();
            }

            let mut wg = DAEMON_INSTANCE
                .wait_mutex
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            wg.stopped = true;
            DAEMON_INSTANCE.wait_cond.notify_all();
            1
        }
        9 => {
            dm_trigger_reload(e.reload_cb, e.global_log_cb);
            1
        }
        10 => {
            dm_trigger_exit(e);
            1
        }
        11 => {
            #[cfg(unix)]
            let eff_remote_mode = if e.a2 < 0 {
                c_int::from(daemon_cmdline_clone().is_some_and(|c| c.remote_client_mode))
            } else {
                e.a2
            };
            #[cfg(not(unix))]
            let eff_remote_mode = e.a2;
            DAEMON_INSTANCE
                .wait_mutex
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .reloading = e.a1 != 0;
            RELOAD_COORDINATOR.cancel_and_join();
            #[cfg(unix)]
            if e.a2 < 0 {
                crate::procrun::terminate_all_active();
                crate::net::remote_srv_ctl(1);
                if let Some(fe) = daemon_frontend_clone() {
                    if eff_remote_mode == 0 || dm_is_stopped() {
                        dm_log(e, 0, b"Stopping Frontend");
                        fe.stop();
                    }
                    while fe.is_running() {
                        if signal_state_op(1) != 0 {
                            dm_trigger_exit(e);
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    dm_log(e, 0, b"Frontend stopped");
                }
            } else if let Some(cb) = e.action_cb {
                cb(
                    e.ctx,
                    12,
                    c_int::from(eff_remote_mode == 0 || dm_is_stopped()),
                    std::ptr::null(),
                    std::ptr::null(),
                );
            }
            #[cfg(not(unix))]
            if let Some(cb) = e.action_cb {
                cb(
                    e.ctx,
                    12,
                    c_int::from(eff_remote_mode == 0 || dm_is_stopped()),
                    std::ptr::null(),
                    std::ptr::null(),
                );
            }
            RELOAD_COORDINATOR.cancel_and_join();
            1
        }
        12 => {
            dm_log(e, 0, b"Cleaning up global objects");
            #[cfg(unix)]
            if e.s1.is_null() {
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
            }
            if e.a1 != 0
                && !dm_is_reloading()
                && DAEMON_INSTANCE.lock_file_owned.load(Ordering::SeqCst)
            {
                dm_log(e, 2, b"Deleting lock file");
                if !e.s1.is_null() {
                    crate::fsops::delete_file(dm_opt_cstr(e.s1));
                } else {
                    #[cfg(unix)]
                    if let Some(opts) = crate::options::active_options() {
                        if let Some(lf) = opts.str_cstr(25) {
                            crate::fsops::delete_file(lf);
                        }
                    }
                }
            }
            1
        }
        #[cfg(unix)]
        13 => {
            crate::scheduler::create_daemon_service_coord();
            crate::articledl::create_daemon_article_cache();
            dm_emit_active_log(0, b"Parsing command line");
            let (argc, argv) = if e.ctx.is_null() {
                let (c, v, _) = crate::entrypoint::process_args();
                (c, v.cast::<*const c_char>())
            } else {
                (e.a1, e.ctx.cast::<*const c_char>())
            };
            if argc < 1 || argv.is_null() {
                return -1;
            }
            let Some(cmd) = crate::cmdline::CmdlineHandle::parse_argv(argc, argv, |bytes| {
                crate::ffi::write_stdout_bytes(bytes, false);
            }) else {
                return -2;
            };
            let arg0 = if (*argv).is_null() {
                Vec::new()
            } else {
                std::ffi::CStr::from_ptr(*argv).to_bytes().to_vec()
            };
            if cmd.print_version {
                libc::printf(
                    c"RustyGet version: %s\n".as_ptr(),
                    crate::ffi::build_version_cstr().as_ptr(),
                );
                libc::exit(0);
            }
            if cmd.print_usage || cmd.errors || argc <= 1 {
                let base_idx = crate::paths::base_file_name(&arg0);
                let usage = crate::cmdline::format_usage(
                    &arg0[base_idx..],
                    crate::paths::PATH_SEPARATOR as u8,
                    true,
                );
                let usage_c = dm_cbuf(&usage);
                libc::printf(c"%s".as_ptr(), usage_c.as_ptr().cast::<c_char>());
                libc::exit(if cmd.print_usage { 0 } else { 1 });
            }
            if let Ok(mut g) = DAEMON_INSTANCE.cmdline.lock() {
                *g = Some((arg0, cmd));
            }
            dm_emit_active_log(0, b"Reading options");
            1
        }
        #[cfg(unix)]
        14 => {
            let Some(cmd) = daemon_cmdline_clone() else {
                return 0;
            };
            let Some(opts) = crate::options::active_options() else {
                return 0;
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
                libc::exit(1);
            }
            if opts.has_config_errors() && cmd.client_operation == 0 {
                dm_emit_active_log(2, b"Pausing all activities due to errors in configuration");
                if let Some(ws) = crate::ffi::active_workstate() {
                    ws.set_pause_download(true);
                    ws.set_pause_post_process(true);
                    ws.set_pause_scan(true);
                }
            }
            if let Some(sp) = crate::net::active_server_pool() {
                sp.set_timeout(opts.scalars().article_timeout);
                sp.set_retry_interval(opts.scalars().article_interval);
            }
            crate::ffi::nzbget_rs_script_config_exec(
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                None,
                std::ptr::null_mut(),
            );
            if crate::articledl::memory_monitor_active() {
                crate::articledl::memory_monitor_init_options(false);
            }
            let umask_raw = opts.scalars().umask as u32;
            let eff_umask = if crate::options::nzbget_is_valid_umask(umask_raw) {
                let m = umask_raw as libc::mode_t;
                libc::umask(m);
                m
            } else {
                let m = libc::umask(0);
                libc::umask(m);
                m
            } as c_int;
            ACTIVE_UMASK.store(eff_umask as u32, Ordering::Relaxed);
            if crate::ffi::build_debug_enabled() {
                dm_emit_active_log(0, format!("Using {:o} umask", eff_umask as u32).as_bytes());
            }
            if let Some(sc) = crate::scanner::active_scanner() {
                sc.init_options_active();
            }
            eff_umask
        }
        _ => 0,
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

    #[test]
    fn test_daemon_exec_reset_stop_add_task_and_cleanup_lockfile() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        unsafe extern "C" fn mock_dm_action(
            raw: *mut std::ffi::c_void,
            sub: c_int,
            a1: c_int,
            _s1: *const c_char,
            _s2: *const c_char,
        ) -> c_int {
            let vec = &mut *(raw as *mut Vec<(c_int, c_int)>);
            vec.push((sub, a1));
            1
        }

        let lock_path =
            std::env::temp_dir().join(format!("nzbget_rs_dm_lock_{}.lock", std::process::id()));
        std::fs::write(&lock_path, b"123\n").unwrap();
        use std::os::unix::ffi::OsStrExt;
        let mut lock_c = lock_path.as_os_str().as_bytes().to_vec();
        lock_c.push(0);

        unsafe {
            let mut calls: Vec<(c_int, c_int)> = Vec::new();
            let mut env = NzbgetRsDaemonEnv {
                ctx: (&mut calls as *mut Vec<(c_int, c_int)>).cast(),
                a1: 0,
                a2: 0,
                a3: 0,
                a4: 0,
                a5: 0,
                a6: 0,
                s1: std::ptr::null(),
                s2: std::ptr::null(),
                s3: std::ptr::null(),
                s4: std::ptr::null(),
                s5: std::ptr::null(),
                segv_cb: None,
                global_log_cb: None,
                reload_cb: None,
                log_cb: None,
                action_cb: Some(mock_dm_action),
            };
            assert_eq!(nzbget_rs_daemon_exec(2, &env), 1);
            env.a1 = 1;
            assert_eq!(nzbget_rs_daemon_exec(3, &env), 0);

            env.a1 = 1;
            env.s1 = c"".as_ptr();
            assert_eq!(nzbget_rs_daemon_exec(6, &env), 1);

            env.a1 = 0;
            env.a2 = 1;
            assert_eq!(nzbget_rs_daemon_exec(8, &env), 1);
            assert_eq!(calls, vec![(8, 1)]);
            env.a1 = 1;
            assert_eq!(nzbget_rs_daemon_exec(3, &env), 1);

            DAEMON_INSTANCE
                .lock_file_owned
                .store(true, Ordering::SeqCst);
            env.a1 = 1;
            env.s1 = lock_c.as_ptr() as *const c_char;
            assert_eq!(nzbget_rs_daemon_exec(12, &env), 1);
            assert!(!lock_path.exists());

            env.a1 = 0;
            env.s1 = std::ptr::null();
            let _ = nzbget_rs_daemon_exec(2, &env);
        }
    }
}
