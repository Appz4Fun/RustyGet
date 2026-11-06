//! Rust daemon entrypoint and pre-flight CLI routing (Milestone M5 — Requirement R2 & R4).

use crate::lifecycle::{get_hooks, resolve_hooks, LifecycleManager};
use std::ffi::{c_char, c_int, CStr};
use std::ptr;

#[cfg(unix)]
extern "C" {
    fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
    fn time(tloc: *mut i64) -> i64;
    fn srand(seed: std::ffi::c_uint);
}

#[cfg(target_os = "macos")]
const LC_CTYPE: c_int = 2;

#[cfg(all(unix, not(target_os = "macos")))]
const LC_CTYPE: c_int = 0;

#[derive(Debug, PartialEq, Eq)]
pub enum EntrypointRoute {
    TlsCipherResolve,
    NServ,
    DaemonLifecycle,
}

/// Inspects argv to determine pre-flight routing.
pub unsafe fn inspect_preflight(argc: c_int, argv: *const *mut c_char) -> EntrypointRoute {
    if argc > 1 && !argv.is_null() {
        let arg1_ptr = *argv.add(1);
        if !arg1_ptr.is_null() {
            let first_arg = CStr::from_ptr(arg1_ptr);
            if first_arg.to_bytes() == b"--internal-tls-cipher-resolve" {
                return EntrypointRoute::TlsCipherResolve;
            }
            if first_arg.to_bytes() == b"--nserv" {
                return EntrypointRoute::NServ;
            }
        }
    }
    EntrypointRoute::DaemonLifecycle
}

/// Initializes platform, locale, RNG, rapidyenc, and argument globals.
pub unsafe fn init_platform(argc: c_int, argv: *mut *mut c_char, argp: *mut *mut c_char) {
    #[cfg(windows)]
    {
        extern "system" {
            fn SetConsoleOutputCP(wCodePageID: u32) -> c_int;
        }
        SetConsoleOutputCP(65001); // CP_UTF8
    }

    #[cfg(unix)]
    {
        setlocale(LC_CTYPE, c"".as_ptr());
    }

    let hooks = get_hooks().or_else(resolve_hooks);
    if let Some(hooks) = hooks {
        if let Some(util_init) = hooks.util_init {
            util_init();
        }
        if let Some(set_args) = hooks.set_process_args {
            set_args(argc, argv, argp);
        }
    }

    #[cfg(unix)]
    {
        let seed = time(ptr::null_mut()) as std::ffi::c_uint;
        srand(seed);
    }
}

/// Master C ABI daemon entrypoint exported from `nzbget_rs` staticlib.
/// Called by main() in `daemon/main/nzbget.cpp`.
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_main(
    argc: c_int,
    argv: *mut *mut c_char,
    argp: *mut *mut c_char,
) -> c_int {
    // 1. Pre-flight check
    let route = inspect_preflight(argc, argv as *const *mut c_char);
    if route == EntrypointRoute::TlsCipherResolve {
        return crate::ffi::nzbget_rs_tls_cipher_resolver(argc, argv as *const *const c_char);
    }

    // 2. Platform initialization
    init_platform(argc, argv, argp);

    if route == EntrypointRoute::NServ {
        let hooks = get_hooks().or_else(resolve_hooks);
        if let Some(hooks) = hooks {
            if let Some(nserv) = hooks.nserv_main {
                return nserv(argc, argv);
            }
        }
        eprintln!("ERROR: Could not start NServ, the program was compiled without NServ");
        return 1;
    }

    #[cfg(windows)]
    {
        let hooks = get_hooks().or_else(resolve_hooks);
        if let Some(hooks) = hooks {
            if let Some(win_svc) = hooks.win_service_check {
                let win_svc_rc = win_svc(argc, argv);
                if win_svc_rc != 0 {
                    return win_svc_rc;
                }
            }
        }
    }

    // 3. Lifecycle coordinator loop (4-Phase Lifecycle)
    LifecycleManager::global().run_daemon_loop(argc, argv, argp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_inspect_preflight() {
        unsafe {
            // Null or argc <= 1
            assert_eq!(
                inspect_preflight(1, std::ptr::null()),
                EntrypointRoute::DaemonLifecycle
            );

            // Regular args
            let arg0 = CString::new("nzbget").unwrap();
            let arg1_s = CString::new("-s").unwrap();
            let argv_s = [
                arg0.as_ptr().cast_mut(),
                arg1_s.as_ptr().cast_mut(),
                std::ptr::null_mut(),
            ];
            assert_eq!(
                inspect_preflight(2, argv_s.as_ptr()),
                EntrypointRoute::DaemonLifecycle
            );

            // Cipher resolve
            let arg1_tls = CString::new("--internal-tls-cipher-resolve").unwrap();
            let argv_tls = [
                arg0.as_ptr().cast_mut(),
                arg1_tls.as_ptr().cast_mut(),
                std::ptr::null_mut(),
            ];
            assert_eq!(
                inspect_preflight(2, argv_tls.as_ptr()),
                EntrypointRoute::TlsCipherResolve
            );

            // NServ
            let arg1_nserv = CString::new("--nserv").unwrap();
            let argv_nserv = [
                arg0.as_ptr().cast_mut(),
                arg1_nserv.as_ptr().cast_mut(),
                std::ptr::null_mut(),
            ];
            assert_eq!(
                inspect_preflight(2, argv_nserv.as_ptr()),
                EntrypointRoute::NServ
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_lc_ctype_platform_mapping() {
        #[cfg(target_os = "macos")]
        assert_eq!(
            LC_CTYPE, 2,
            "Darwin libc locale.h defines LC_CTYPE as 2 (LC_ALL is 0)"
        );

        #[cfg(target_os = "linux")]
        assert_eq!(LC_CTYPE, 0, "Linux glibc locale.h defines LC_CTYPE as 0");
    }
}
