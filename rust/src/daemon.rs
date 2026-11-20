//! The daemon's start-up, reload and shutdown wiring, formerly the `RustyGet`
//! class and the `nzbget_c_*` entry points in `daemon/main/rustyget.cpp`.
//!
//! [`Daemon`] implements the `LifecycleHooks` that
//! `LifecycleManager::run_daemon_loop` drives. The process entry
//! (`rust/entry.rs`) registers it through [`register_daemon_hooks`]. Its first
//! hook, `util_init`, sets the build environment (formerly the C++ `Util::Init`).
//!
//! One daemon instance lives from phase 1 to phase 4. It owns the log, the
//! options, the work state, the server pool and the pre/post-processor runner,
//! and frees them in the order the C++ class did: runner, global cleanup,
//! server pool, work state, options, log.

use crate::options::OptStr;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once, PoisonError};
use std::time::Duration;

use crate::ffi::NzbgetRsWorkState;
use crate::jobthread::PrePostRunner;
use crate::lifecycle::{DaemonHost, LifecycleHooks};
use crate::log::NzbgetRsLog;
use crate::net::NzbgetRsServerPool;
use crate::nserv::NzbgetRsNservEnv;
use crate::options::NzbgetRsOptions;

/// `Message::EKind` values.
const KIND_ERROR: i32 = 2;
const KIND_DEBUG: i32 = 3;

/// Whether a daemon instance exists (phase 1 until phase 4 or exit).
static INSTANCE: AtomicBool = AtomicBool::new(false);
static LOG: Mutex<Option<Arc<NzbgetRsLog>>> = Mutex::new(None);
static OPTIONS: Mutex<Option<Arc<NzbgetRsOptions>>> = Mutex::new(None);
static WORK_STATE: Mutex<Option<Arc<NzbgetRsWorkState>>> = Mutex::new(None);
static SERVER_POOL: Mutex<Option<Arc<NzbgetRsServerPool>>> = Mutex::new(None);
/// The daemon's pre/post-processor thread (rust/src/jobthread.rs).
static PRE_POST: Mutex<Option<Arc<PrePostRunner>>> = Mutex::new(None);
static ATEXIT: Once = Once::new();

/// Registers the daemon's lifecycle hooks for `nzbget_rs_main`. Called by
/// the process entry.
pub fn register_daemon_hooks() {
    crate::lifecycle::register_hooks(&DAEMON);
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

fn new_log() -> Arc<NzbgetRsLog> {
    crate::log::install_log(crate::ffi::build_debug_enabled())
}

/// A start-up failure the C++ daemon reported by throwing out of phase 1,
/// which ended the process with an abort.
fn fatal(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::abort();
}

/// The instance's pre/post-processor runner, if any.
fn prepost() -> Option<Arc<PrePostRunner>> {
    PRE_POST.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn prepost_has_active_unpack() -> bool {
    prepost().is_some_and(|r| r.has_active_unpack(false))
}

fn get_reloading() -> bool {
    crate::lifecycle::daemon_is_reloading()
}

fn stop(reload: bool) {
    crate::lifecycle::stop_daemon(reload, &DAEMON);
}

/// The instance as the lifecycle sees it.
struct Daemon;
static DAEMON: Daemon = Daemon;

impl DaemonHost for Daemon {
    fn has_instance(&self) -> bool {
        INSTANCE.load(Ordering::SeqCst)
    }

    fn has_active_unpack(&self) -> bool {
        prepost_has_active_unpack()
    }

    fn stop(&self, reload: bool) {
        if self.has_instance() {
            stop(reload);
        }
    }

    fn start_prepost(&self) {
        if let Some(r) = prepost() {
            r.start();
        }
    }

    fn prepost_running(&self) -> bool {
        prepost().is_some_and(|r| r.is_running())
    }

    fn prepost_has_jobs(&self) -> bool {
        prepost().is_some_and(|r| r.has_more_jobs())
    }

    fn stop_prepost_when_idle(&self) {
        if let Some(r) = prepost() {
            if !r.is_stopped() {
                r.stop();
            }
        }
    }

    fn stop_prepost(&self, remote_mode: bool) {
        if !remote_mode {
            if let Some(r) = prepost() {
                r.stop();
            }
        }
    }
}

impl LifecycleHooks for Daemon {
    fn util_init(&self) {
        crate::ffi::init_build_env();
    }

    unsafe fn nserv_main(&self, argc: c_int, argv: *mut *mut c_char) -> Option<c_int> {
        Some(nserv_main(argc, argv))
    }

    fn phase1_init(&self, reload: bool) -> c_int {
        unsafe { phase1_init(reload) }
    }

    fn phase3_server_activate(&self) -> c_int {
        phase3_server_activate()
    }

    fn phase4_shutdown(&self, reload: bool) {
        phase4_shutdown(reload);
    }

    fn has_active_unpack(&self) -> bool {
        prepost_has_active_unpack()
    }

    fn is_reloading(&self) -> bool {
        INSTANCE.load(Ordering::SeqCst) && get_reloading()
    }

    fn drives_reload(&self) -> bool {
        true
    }

    fn reload(&self) {
        crate::lifecycle::request_reload(&DAEMON);
    }

    fn exit_proc(&self) {
        crate::lifecycle::request_exit(&DAEMON);
    }
}

unsafe extern "C" fn segv_log(_ctx: *mut c_void, msg: *const c_char) {
    if !msg.is_null() {
        crate::log::log_active_bytes(KIND_ERROR, CStr::from_ptr(msg).to_bytes());
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
    let runner = PRE_POST.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(r) = runner {
        r.shutdown();
    }
    crate::lifecycle::cleanup_global_objects(
        OPTIONS.lock().unwrap_or_else(PoisonError::into_inner).is_some(),
    );
    if let Some(pool) = SERVER_POOL.lock().unwrap_or_else(PoisonError::into_inner).take() {
        crate::net::release_server_pool(pool);
    }
    if let Some(ws) = WORK_STATE.lock().unwrap_or_else(PoisonError::into_inner).take() {
        crate::ffi::uninstall_workstate(&ws);
    }
    if let Some(opts) = OPTIONS.lock().unwrap_or_else(PoisonError::into_inner).take() {
        crate::options::release_options(opts);
    }
    if let Some(log) = LOG.lock().unwrap_or_else(PoisonError::into_inner).take() {
        crate::log::uninstall_log(log);
    }
}

/// Phase 1: log, globals, command line, options, TLS, then daemonize and
/// activate. Returns 0 to continue (the command line's direct modes exit).
unsafe fn phase1_init(reload: bool) -> c_int {
    ATEXIT.call_once(|| {
        libc::atexit(atexit_cleanup);
    });
    // a previous instance phase 4 did not end goes first, as the C++ unique_ptr did
    destroy_instance();
    INSTANCE.store(true, Ordering::SeqCst);

    crate::lifecycle::reset_instance(reload);
    *LOG.lock().unwrap_or_else(PoisonError::into_inner) = Some(new_log());

    let mut version = b"RustyGet ".to_vec();
    version.extend_from_slice(crate::ffi::build_version_bytes());
    log_debug(&version);

    if !reload {
        log_debug(b"Initializing global connection data");
        tls_init();
    }

    *WORK_STATE.lock().unwrap_or_else(PoisonError::into_inner) = Some(crate::ffi::install_workstate());
    *SERVER_POOL.lock().unwrap_or_else(PoisonError::into_inner) = Some(crate::net::install_server_pool());

    match crate::lifecycle::parse_command_line() {
        Ok(()) => {}
        Err(crate::lifecycle::CommandLineError::NoArguments) => fatal("Invalid command line"),
        Err(crate::lifecycle::CommandLineError::Invalid) => fatal("Command line parsing failed"),
    }

    *PRE_POST.lock().unwrap_or_else(PoisonError::into_inner) = Some(PrePostRunner::new());

    let options = crate::options::new_options(
        crate::options::OptionsSource::DaemonCommandLine,
        false,
        options_flags(),
        crate::ffi::build_version_cstr(),
        None,
    );
    *OPTIONS.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&options));
    crate::lifecycle::apply_command_line();

    if !crate::ffi::build_tls_disabled() {
        let opts = &*options;
        let cert_store = if opts.scalars().cert_check != 0 {
            opts.str_bytes(OptStr::CertStore)
        } else {
            &[]
        };
        crate::tlssock::init_options(cert_store);
    }

    crate::lifecycle::activate_daemon(Some(segv_log));
    0
}

/// Phase 3: the main program loop until stop or reload.
fn phase3_server_activate() -> c_int {
    if !INSTANCE.load(Ordering::SeqCst) {
        return 0;
    }
    crate::lifecycle::run_daemon(&DAEMON);
    0
}

/// Phase 4: stop the subsystems, shut TLS down, and free the instance.
fn phase4_shutdown(reload: bool) {
    if INSTANCE.load(Ordering::SeqCst) {
        crate::lifecycle::shutdown_daemon(reload, &DAEMON);

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
            let slot = ctx.cast::<Option<Arc<NzbgetRsOptions>>>();
            let cmd_opts: Vec<&CStr> = if opts.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(opts, opt_len)
                    .iter()
                    .filter(|p| !p.is_null())
                    .map(|&p| CStr::from_ptr(p))
                    .collect()
            };
            let created = crate::options::new_options(
                crate::options::OptionsSource::Given {
                    exe_name: b"rustyget/rustyget",
                    config_file: None,
                    no_config: true,
                    cmd_opts: &cmd_opts,
                },
                true,
                options_flags(),
                crate::ffi::build_version_cstr(),
                None,
            );
            if let Some(old) = (*slot).replace(created) {
                crate::options::release_options(old);
            }
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
unsafe fn nserv_main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    let log = new_log();
    let mut options: Option<Arc<NzbgetRsOptions>> = None;
    let env = NzbgetRsNservEnv {
        ctx: (&raw mut options).cast::<c_void>(),
        version: ptr::null(),
        write_stdout_cb: None,
        log_cb: None,
        runtime_cb: Some(nserv_runtime),
        transport: None,
        tls_host: None,
        debug_on: 0,
    };
    let rc =
        crate::nserv::nzbget_rs_nserv_run(argc, argv.cast::<*const c_char>().cast_const(), &env);
    if let Some(opts) = options {
        crate::options::release_options(opts);
    }
    crate::log::uninstall_log(log);
    rc
}
