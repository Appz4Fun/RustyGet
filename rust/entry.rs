//! Rust process entry point for the `rustyget` daemon binary.
//!
//! Owns the OS `main` symbol: checks the CPU against the build's tier,
//! registers the daemon's lifecycle hooks (rust/src/daemon.rs) and runs
//! `nzbget_rs_main`.

#![no_main]

use std::ffi::{c_char, c_int};

extern "C" {
    /// The CPU tier check (rust/tiercheck.rs), which build.rs compiles for the
    /// compiler's default CPU so it runs on any CPU: prints which build to use
    /// and returns nonzero when this CPU lacks a feature the tier assumes.
    fn rustyget_tier_check() -> c_int;
}

#[no_mangle]
pub unsafe extern "C" fn main(
    argc: c_int,
    argv: *mut *mut c_char,
    argp: *mut *mut c_char,
) -> c_int {
    // A tier build on a CPU below its tier: say which build to use before any
    // tier-specific code runs, instead of dying on an illegal instruction.
    if rustyget_tier_check() != 0 {
        return 1;
    }
    nzbget_rs::daemon::register_daemon_hooks();
    nzbget_rs::entrypoint::nzbget_rs_main(argc, argv, argp)
}
