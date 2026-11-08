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
    pub set_process_args:
        Option<unsafe extern "C" fn(c_int, *mut *mut c_char, *mut *mut c_char)>,
    pub util_init: Option<unsafe extern "C" fn()>,
    pub nserv_main: Option<unsafe extern "C" fn(c_int, *mut *mut c_char) -> c_int>,
    pub win_service_check: Option<unsafe extern "C" fn(c_int, *mut *mut c_char) -> c_int>,
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
        let win_service_check = None;
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
                win_service_check,
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

/// Enables core dumps on Linux (`setrlimit(RLIMIT_CORE, RLIM_INFINITY)` + `prctl(PR_SET_DUMPABLE, 1)`).
pub fn enable_core_dump() {
    #[cfg(target_os = "linux")]
    unsafe {
        let rlim = libc::rlimit {
            rlim_cur: libc::RLIM_INFINITY,
            rlim_max: libc::RLIM_INFINITY,
        };
        let _ = libc::setrlimit(libc::RLIMIT_CORE, &rlim);
        let _ = libc::prctl(libc::PR_SET_DUMPABLE, 1, 0, 0, 0);
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
            let events = (if read { libc::POLLIN } else { 0 })
                | (if write { libc::POLLOUT } else { 0 });
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

        if !backward_compat_root && (libc::getuid() == 0 || libc::geteuid() == 0) {
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
            win_service_check: None,
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
            win_service_check: None,
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
            win_service_check: None,
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

        let tmp_dir = std::env::temp_dir().join(format!(
            "rustyget_batch154_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let existing_sub = tmp_dir.join("existing");
        std::fs::create_dir_all(&existing_sub).unwrap();
        let missing_sub = tmp_dir.join("missing");

        let req_spec = format!(
            "{};{}",
            existing_sub.display(),
            missing_sub.display()
        );
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
}
