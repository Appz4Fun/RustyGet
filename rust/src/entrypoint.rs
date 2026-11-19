//! Rust daemon entrypoint and pre-flight CLI routing (Milestone M5 — Requirement R2 & R4).

use crate::lifecycle::{get_hooks, LifecycleManager};
use std::ffi::{c_char, c_int, CStr, OsString};
use std::ptr;

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

/// The daemon's command line and the environment block passed to `main`,
/// copied once at startup.
struct ProcessArgs {
    argv: Vec<OsString>,
    envp: Option<Vec<OsString>>,
}

static PROCESS_ARGS: std::sync::RwLock<ProcessArgs> = std::sync::RwLock::new(ProcessArgs {
    argv: Vec::new(),
    envp: None,
});

/// Copies a NULL-terminated array of C strings (at most `limit` entries).
unsafe fn os_strings(arr: *const *mut c_char, limit: usize) -> Vec<OsString> {
    let mut out = Vec::new();
    if arr.is_null() {
        return out;
    }
    for i in 0..limit {
        let p = *arr.add(i);
        if p.is_null() {
            break;
        }
        out.push(os_string_from_bytes(CStr::from_ptr(p).to_bytes()));
    }
    out
}

#[cfg(unix)]
pub(crate) fn os_string_from_bytes(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes.to_vec())
}

#[cfg(not(unix))]
pub(crate) fn os_string_from_bytes(bytes: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(bytes).into_owned())
}

/// The daemon's own command line (`argv`), empty before [`init_platform`].
pub(crate) fn process_argv() -> Vec<OsString> {
    PROCESS_ARGS
        .read()
        .map_or_else(|_| Vec::new(), |g| g.argv.clone())
}

/// The environment block `main` received (`envp`), if the platform passed one.
pub(crate) fn process_envp() -> Option<Vec<OsString>> {
    PROCESS_ARGS.read().ok().and_then(|g| g.envp.clone())
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
        libc::setlocale(libc::LC_CTYPE, c"".as_ptr());
    }

    if let Ok(mut g) = PROCESS_ARGS.write() {
        g.argv = os_strings(argv, usize::try_from(argc).unwrap_or(0));
        g.envp = (!argp.is_null()).then(|| os_strings(argp, usize::MAX));
    }

    let hooks = get_hooks();
    if let Some(hooks) = hooks {
        if let Some(util_init) = hooks.util_init {
            util_init();
        }
    }

    #[cfg(unix)]
    {
        let seed = libc::time(ptr::null_mut()) as std::ffi::c_uint;
        libc::srand(seed);
    }
}

/// Master C ABI daemon entrypoint of the `nzbget_rs` library.
/// Called by `main` in `rust/entry.rs`.
pub unsafe extern "C" fn nzbget_rs_main(
    argc: c_int,
    argv: *mut *mut c_char,
    argp: *mut *mut c_char,
) -> c_int {
    // 1. Pre-flight check
    let route = inspect_preflight(argc, argv as *const *mut c_char);
    if route == EntrypointRoute::TlsCipherResolve {
        if argc != 5 || argv.is_null() {
            return 2;
        }
        let pointers = std::slice::from_raw_parts(argv as *const *const c_char, argc as usize);
        let mut args = Vec::new();
        for &pointer in &pointers[1..] {
            if pointer.is_null() {
                return 2;
            }
            let Ok(value) = CStr::from_ptr(pointer).to_str() else {
                return 2;
            };
            args.push(value.to_owned());
        }
        return crate::ffi::tls_cipher_native::resolver(&args);
    }

    // 2. Platform initialization
    init_platform(argc, argv, argp);

    if route == EntrypointRoute::NServ {
        let hooks = get_hooks();
        if let Some(hooks) = hooks {
            if let Some(nserv) = hooks.nserv_main {
                return nserv(argc, argv);
            }
        }
        eprintln!("ERROR: Could not start NServ, the program was compiled without NServ");
        return 1;
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
}
