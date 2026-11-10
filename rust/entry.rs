//! Rust process entry point for the `nzbget` daemon binary.
//!
//! Owns the OS `main` symbol and binds the legacy C++ subsystem hooks
//! (`nzbget_c_*`) into the Rust lifecycle orchestrator (`nzbget_rs_main`).

use std::ffi::{c_char, c_int};

#[repr(C)]
pub struct LifecycleHooks {
    pub set_process_args: unsafe extern "C" fn(c_int, *mut *mut c_char, *mut *mut c_char),
    pub util_init: unsafe extern "C" fn(),
    pub nserv_main: unsafe extern "C" fn(c_int, *mut *mut c_char) -> c_int,
    pub phase1_init: unsafe extern "C" fn(bool) -> c_int,
    pub phase2_subsystems_create_and_recover: unsafe extern "C" fn(bool) -> c_int,
    pub phase3_server_activate: unsafe extern "C" fn() -> c_int,
    pub phase4_shutdown: unsafe extern "C" fn(bool),
    pub has_active_unpack: unsafe extern "C" fn() -> bool,
    pub is_reloading: unsafe extern "C" fn() -> bool,
    pub stop_server: unsafe extern "C" fn(bool),
    pub reload: unsafe extern "C" fn(),
    pub exit_proc: unsafe extern "C" fn(),
}

extern "C" {
    fn nzbget_c_set_process_args(argc: c_int, argv: *mut *mut c_char, argp: *mut *mut c_char);
    fn nzbget_c_util_init();
    fn nzbget_c_nserv_main(argc: c_int, argv: *mut *mut c_char) -> c_int;
    fn nzbget_c_phase1_init(reloading: bool) -> c_int;
    fn nzbget_c_phase2_subsystems_create_and_recover(reloading: bool) -> c_int;
    fn nzbget_c_phase3_server_activate() -> c_int;
    fn nzbget_c_phase4_shutdown(reloading: bool);
    fn nzbget_c_has_active_unpack() -> bool;
    fn nzbget_c_is_reloading() -> bool;
    fn nzbget_c_stop_server(reloading: bool);
    fn nzbget_c_reload();
    fn nzbget_c_exit_proc();

    fn nzbget_rs_register_lifecycle_hooks(hooks: *const LifecycleHooks);
    fn nzbget_rs_main(argc: c_int, argv: *mut *mut c_char, argp: *mut *mut c_char) -> c_int;
}

static HOOKS: LifecycleHooks = LifecycleHooks {
    set_process_args: nzbget_c_set_process_args,
    util_init: nzbget_c_util_init,
    nserv_main: nzbget_c_nserv_main,
    phase1_init: nzbget_c_phase1_init,
    phase2_subsystems_create_and_recover: nzbget_c_phase2_subsystems_create_and_recover,
    phase3_server_activate: nzbget_c_phase3_server_activate,
    phase4_shutdown: nzbget_c_phase4_shutdown,
    has_active_unpack: nzbget_c_has_active_unpack,
    is_reloading: nzbget_c_is_reloading,
    stop_server: nzbget_c_stop_server,
    reload: nzbget_c_reload,
    exit_proc: nzbget_c_exit_proc,
};

#[no_mangle]
pub unsafe extern "C" fn main(
    argc: c_int,
    argv: *mut *mut c_char,
    argp: *mut *mut c_char,
) -> c_int {
    nzbget_rs_register_lifecycle_hooks(&HOOKS);
    nzbget_rs_main(argc, argv, argp)
}
