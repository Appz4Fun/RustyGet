//! The daemon's start-up, reload and shutdown wiring, formerly the `RustyGet`
//! class and the `nzbget_c_*` entry points in `daemon/main/rustyget.cpp`.
//!
//! [`daemon_hooks`] is the table `LifecycleManager::run_daemon_loop` drives.
//! The process entry (`rust/entry.rs`) registers it through
//! [`register_daemon_hooks`]. Its first hook, [`util_init`], sets the build
//! environment (formerly the C++ `Util::Init`).
//!
//! One daemon instance lives from phase 1 to phase 4. It owns the log, the
//! options, the work state, the server pool and the pre/post-processor runner,
//! and frees them in the order the C++ class did: runner, global cleanup,
//! server pool, work state, options, log.

use crate::options::OptStr;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex, Once, PoisonError};
use std::time::Duration;

use crate::ffi::NzbgetRsWorkState;
use crate::jobthread::PrePostRunner;
use crate::lifecycle::{nzbget_rs_daemon_exec, LifecycleHooks, NzbgetRsDaemonEnv};
use crate::log::NzbgetRsLog;
use crate::net::NzbgetRsServerPool;
use crate::nserv::NzbgetRsNservEnv;
use crate::options::NzbgetRsOptions;

/// `Message::EKind` values.
const KIND_ERROR: i32 = 2;
const KIND_DEBUG: i32 = 3;

/// Whether a daemon instance exists (phase 1 until phase 4 or exit).
static INSTANCE: AtomicBool = AtomicBool::new(false);
static LOG: AtomicPtr<NzbgetRsLog> = AtomicPtr::new(ptr::null_mut());
static OPTIONS: AtomicPtr<NzbgetRsOptions> = AtomicPtr::new(ptr::null_mut());
static WORK_STATE: Mutex<Option<Arc<NzbgetRsWorkState>>> = Mutex::new(None);
static SERVER_POOL: AtomicPtr<NzbgetRsServerPool> = AtomicPtr::new(ptr::null_mut());
/// The daemon's pre/post-processor thread (rust/src/jobthread.rs).
static PRE_POST: AtomicPtr<PrePostRunner> = AtomicPtr::new(ptr::null_mut());
static ATEXIT: Once = Once::new();

/// Sets the build flags, the version and the TLS library's version text,
/// before anything else runs (the former C++ `Util::Init`).
unsafe extern "C" fn util_init() {
    crate::ffi::init_build_env();
}

/// The lifecycle table for the daemon binary.
pub fn daemon_hooks() -> LifecycleHooks {
    LifecycleHooks {
        util_init: Some(util_init),
        nserv_main: Some(nserv_main),
        phase1_init: Some(phase1_init),
        phase2_subsystems_create_and_recover: None,
        phase3_server_activate: Some(phase3_server_activate),
        phase4_shutdown: Some(phase4_shutdown),
        has_active_unpack: Some(has_active_unpack_hook),
        is_reloading: Some(is_reloading_hook),
        stop_server: Some(stop_server_hook),
        reload: Some(reload),
        exit_proc: Some(exit_proc),
    }
}

/// Registers [`daemon_hooks`] for `nzbget_rs_main`. Called by the process entry.
pub fn register_daemon_hooks() {
    crate::lifecycle::register_hooks(daemon_hooks());
}

fn empty_env() -> NzbgetRsDaemonEnv {
    NzbgetRsDaemonEnv {
        ctx: ptr::null_mut(),
        a1: 0,
        a2: 0,
        a3: 0,
        a4: 0,
        a5: 0,
        a6: 0,
        s1: ptr::null(),
        s2: ptr::null(),
        s3: ptr::null(),
        s4: ptr::null(),
        s5: ptr::null(),
        segv_cb: None,
        global_log_cb: None,
        reload_cb: None,
        log_cb: None,
        action_cb: None,
    }
}

fn exec(op: c_int, env: &NzbgetRsDaemonEnv) -> c_int {
    unsafe { nzbget_rs_daemon_exec(op, env) }
}

fn log_debug(text: &[u8]) {
    crate::log::log_active_bytes(KIND_DEBUG, text);
}

/// The flags the C++ `Options` class passed to `nzbget_rs_options_new`:
/// 32 default diagnostics sink, 8 `HAVE_X509_CHECK_HOST`, and 1/2/4 for a
/// build without curses, PAR checking or TLS.
fn options_flags() -> c_int {
    let mut flags = 32 | 8;
    if crate::ffi::build_curses_disabled() {
        flags |= 1;
    }
    if !crate::ffi::build_parcheck_enabled() {
        flags |= 2;
    }
    if crate::ffi::build_tls_disabled() {
        flags |= 4;
    }
    flags
}

fn tls_init() {
    if !crate::ffi::build_tls_disabled() {
        crate::tlssock::init();
    }
}

fn new_log() -> *mut NzbgetRsLog {
    crate::log::nzbget_rs_log_new(c_int::from(crate::ffi::build_debug_enabled()))
}

/// A start-up failure the C++ daemon reported by throwing out of phase 1,
/// which ended the process with an abort.
fn fatal(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::abort();
}

fn prepost_op(op: c_int) -> c_int {
    unsafe {
        crate::jobthread::nzbget_rs_prepost_runner_op(
            PRE_POST.load(Ordering::SeqCst),
            op,
            0,
            0,
            0,
            0,
        )
    }
}

fn prepost_has_active_unpack() -> bool {
    !PRE_POST.load(Ordering::SeqCst).is_null() && prepost_op(5) != 0
}

fn get_reloading() -> bool {
    let env = empty_env();
    exec(3, &env) != 0
}

fn stop(reload: bool) {
    let env = NzbgetRsDaemonEnv {
        a1: c_int::from(reload),
        a2: -1,
        action_cb: Some(action_cb),
        ..empty_env()
    };
    exec(8, &env);
}

unsafe extern "C" fn segv_log(_ctx: *mut c_void, msg: *const c_char) {
    if !msg.is_null() {
        crate::log::log_active_bytes(KIND_ERROR, CStr::from_ptr(msg).to_bytes());
    }
}

/// Answers the lifecycle's requests to the daemon instance (`action_cb`).
unsafe extern "C" fn action_cb(
    _ctx: *mut c_void,
    sub: c_int,
    a2: c_int,
    _s1: *const c_char,
    _s2: *const c_char,
) -> c_int {
    match sub {
        4 => {
            prepost_op(0);
            0
        }
        5 => {
            if a2 == 0 {
                c_int::from(prepost_op(2) != 0)
            } else if prepost_op(4) != 0 {
                4
            } else {
                0
            }
        }
        6 => {
            if prepost_op(3) == 0 {
                prepost_op(1);
            }
            0
        }
        7 => c_int::from(prepost_has_active_unpack()),
        8 => {
            if a2 == 0 && !PRE_POST.load(Ordering::SeqCst).is_null() {
                prepost_op(1);
            }
            0
        }
        10 => {
            exit_proc();
            0
        }
        _ => 0,
    }
}

/// The reload coordinator's view of the instance (`reload_cb`): 0 asks for
/// flags (1 instance exists, 2 unpack active), 1 stops for reload, 3 stops.
unsafe extern "C" fn reload_cb(sub: c_int) -> c_int {
    let has_instance = INSTANCE.load(Ordering::SeqCst);
    match sub {
        0 => c_int::from(has_instance) | (c_int::from(prepost_has_active_unpack()) << 1),
        1 if has_instance => {
            stop(true);
            0
        }
        3 if has_instance => {
            stop(false);
            0
        }
        _ => 0,
    }
}

extern "C" fn atexit_cleanup() {
    destroy_instance();
}

/// Frees the instance: the runner, then the global objects (and the lock file
/// unless reloading), then the server pool, work state, options and log.
fn destroy_instance() {
    if !INSTANCE.swap(false, Ordering::SeqCst) {
        return;
    }
    unsafe {
        crate::jobthread::nzbget_rs_prepost_runner_free(
            PRE_POST.swap(ptr::null_mut(), Ordering::SeqCst),
        );
    }
    let env = NzbgetRsDaemonEnv {
        a1: c_int::from(!OPTIONS.load(Ordering::SeqCst).is_null()),
        ..empty_env()
    };
    exec(12, &env);
    unsafe {
        crate::net::nzbget_rs_serverpool_free(SERVER_POOL.swap(ptr::null_mut(), Ordering::SeqCst));
    }
    if let Some(ws) = WORK_STATE.lock().unwrap_or_else(PoisonError::into_inner).take() {
        crate::ffi::uninstall_workstate(&ws);
    }
    unsafe {
        crate::options::nzbget_rs_options_free(OPTIONS.swap(ptr::null_mut(), Ordering::SeqCst));
        crate::log::nzbget_rs_log_free(LOG.swap(ptr::null_mut(), Ordering::SeqCst));
    }
}

/// Phase 1: log, globals, command line, options, TLS, then daemonize and
/// activate. Returns 0 to continue (the command line's direct modes exit).
unsafe extern "C" fn phase1_init(reload: bool) -> c_int {
    ATEXIT.call_once(|| {
        libc::atexit(atexit_cleanup);
    });
    // a previous instance phase 4 did not end goes first, as the C++ unique_ptr did
    destroy_instance();
    INSTANCE.store(true, Ordering::SeqCst);

    let reset = NzbgetRsDaemonEnv {
        a1: c_int::from(reload),
        ..empty_env()
    };
    exec(2, &reset);
    LOG.store(new_log(), Ordering::SeqCst);

    let mut version = b"RustyGet ".to_vec();
    version.extend_from_slice(crate::ffi::build_version_bytes());
    log_debug(&version);

    if !reload {
        log_debug(b"Initializing global connection data");
        tls_init();
    }

    *WORK_STATE.lock().unwrap_or_else(PoisonError::into_inner) = Some(crate::ffi::install_workstate());
    SERVER_POOL.store(crate::net::nzbget_rs_serverpool_new(), Ordering::SeqCst);

    let cmdline = empty_env();
    let rc = exec(13, &cmdline);
    if rc == -1 {
        fatal("Invalid command line");
    }
    if rc < 0 {
        fatal("Command line parsing failed");
    }

    PRE_POST.store(
        crate::jobthread::nzbget_rs_prepost_runner_new().cast_mut(),
        Ordering::SeqCst,
    );

    // no_config < 0: the options come from the command line phase 13 parsed
    let mut state_flags: c_int = 0;
    let options = crate::options::nzbget_rs_options_new(
        ptr::null(),
        ptr::null(),
        -1,
        0,
        ptr::null(),
        0,
        options_flags(),
        crate::ffi::build_version_cstr().as_ptr(),
        ptr::null_mut(),
        None,
        ptr::null_mut(),
        &mut state_flags,
    );
    OPTIONS.store(options, Ordering::SeqCst);
    exec(14, &cmdline);

    if !crate::ffi::build_tls_disabled() {
        let opts = &*options;
        let cert_store = if opts.scalars().cert_check != 0 {
            opts.str_bytes(OptStr::CertStore)
        } else {
            &[]
        };
        crate::tlssock::init_options(cert_store);
    }

    let activate = NzbgetRsDaemonEnv {
        a1: -1,
        segv_cb: Some(segv_log),
        action_cb: Some(action_cb),
        ..empty_env()
    };
    exec(5, &activate)
}

/// Phase 3: the main program loop until stop or reload.
unsafe extern "C" fn phase3_server_activate() -> c_int {
    if !INSTANCE.load(Ordering::SeqCst) {
        return 0;
    }
    let env = NzbgetRsDaemonEnv {
        a1: -1,
        reload_cb: Some(reload_cb),
        action_cb: Some(action_cb),
        ..empty_env()
    };
    exec(7, &env)
}

/// Phase 4: stop the subsystems, shut TLS down, and free the instance.
unsafe extern "C" fn phase4_shutdown(reload: bool) {
    if INSTANCE.load(Ordering::SeqCst) {
        let env = NzbgetRsDaemonEnv {
            a1: c_int::from(reload),
            a2: -1,
            action_cb: Some(action_cb),
            ..empty_env()
        };
        exec(11, &env);

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        if !crate::ffi::build_tls_disabled() {
            crate::ffi::tls_cipher_native::service().shutdown();
        }
        if !get_reloading() && !crate::ffi::build_tls_disabled() {
            crate::tlssock::final_cleanup();
        }
    }
    destroy_instance();
}

unsafe extern "C" fn has_active_unpack_hook() -> bool {
    prepost_has_active_unpack()
}

unsafe extern "C" fn is_reloading_hook() -> bool {
    INSTANCE.load(Ordering::SeqCst) && get_reloading()
}

unsafe extern "C" fn stop_server_hook(reload: bool) {
    if INSTANCE.load(Ordering::SeqCst) {
        stop(reload);
    }
}

unsafe extern "C" fn reload() {
    let env = NzbgetRsDaemonEnv {
        reload_cb: Some(reload_cb),
        ..empty_env()
    };
    exec(9, &env);
}

unsafe extern "C" fn exit_proc() {
    let env = NzbgetRsDaemonEnv {
        reload_cb: Some(reload_cb),
        ..empty_env()
    };
    exec(10, &env);
}

/// NServ's runtime requests: 0 creates the options from its command line and
/// sets up TLS; 3 waits on standard input until it reads a NUL byte.
unsafe extern "C" fn nserv_runtime(
    ctx: *mut c_void,
    op: c_int,
    _arg: c_int,
    opts: *const *const c_char,
    opt_len: usize,
) {
    match op {
        0 => {
            let slot = ctx.cast::<*mut NzbgetRsOptions>();
            let mut state_flags: c_int = 0;
            let created = crate::options::nzbget_rs_options_new(
                c"rustyget/rustyget".as_ptr(),
                ptr::null(),
                1,
                1,
                if opt_len > 0 { opts } else { ptr::null() },
                opt_len,
                options_flags(),
                crate::ffi::build_version_cstr().as_ptr(),
                ptr::null_mut(),
                None,
                ptr::null_mut(),
                &mut state_flags,
            );
            crate::options::nzbget_rs_options_free(std::mem::replace(&mut *slot, created));
            log_debug(b"Initializing global connection data");
            tls_init();
            libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        }
        3 => {
            while libc::getchar() != 0 {
                std::thread::sleep(Duration::from_millis(200));
            }
        }
        _ => {}
    }
}

/// `--nserv`: the test NNTP server, with its own log and options.
unsafe extern "C" fn nserv_main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    let log = new_log();
    let mut options: *mut NzbgetRsOptions = ptr::null_mut();
    let env = NzbgetRsNservEnv {
        ctx: (&raw mut options).cast::<c_void>(),
        version: ptr::null(),
        write_stdout_cb: None,
        log_cb: None,
        runtime_cb: Some(nserv_runtime),
        conn_cb: None,
        tls_host_cb: None,
        debug_on: 0,
    };
    let rc =
        crate::nserv::nzbget_rs_nserv_run(argc, argv.cast::<*const c_char>().cast_const(), &env);
    crate::options::nzbget_rs_options_free(options);
    crate::log::nzbget_rs_log_free(log);
    rc
}
