//! Rust process entry point for the `nzbget` daemon binary.
//!
//! Owns the OS `main` symbol, registers the daemon's lifecycle hooks
//! (rust/src/daemon.rs) with the C++ start-up shim, and runs `nzbget_rs_main`.

use std::ffi::{c_char, c_int};

extern "C" {
    /// The C++ start-up shim (`daemon/main/rustyget.cpp`): build flags and the
    /// queue model's host callback.
    fn nzbget_c_util_init();

    fn nzbget_rs_register_daemon_hooks(util_init: Option<unsafe extern "C" fn()>);
    fn nzbget_rs_main(argc: c_int, argv: *mut *mut c_char, argp: *mut *mut c_char) -> c_int;
}

/// CPU features the Rust library was compiled to assume (its release tier,
/// passed in by the build as `--cfg rustyget_tier="..."`) that this CPU lacks.
/// This object is always compiled for the compiler's default CPU, so the
/// detection below really runs on the CPU (with target-cpu set, std's
/// detection macros would answer "yes" at compile time).
fn missing_tier_features() -> Vec<&'static str> {
    #[allow(unused_mut)]
    let mut need: Vec<(&'static str, bool)> = Vec::new();
    #[cfg(all(
        target_arch = "x86_64",
        any(rustyget_tier = "x86-64-v3", rustyget_tier = "x86-64-v4")
    ))]
    {
        use std::arch::is_x86_feature_detected as has;
        need.extend([
            ("avx2", has!("avx2")),
            ("bmi1", has!("bmi1")),
            ("bmi2", has!("bmi2")),
            ("fma", has!("fma")),
            ("lzcnt", has!("lzcnt")),
            ("movbe", has!("movbe")),
            ("f16c", has!("f16c")),
        ]);
    }
    #[cfg(all(target_arch = "x86_64", rustyget_tier = "x86-64-v4"))]
    {
        use std::arch::is_x86_feature_detected as has;
        need.extend([
            ("avx512f", has!("avx512f")),
            ("avx512bw", has!("avx512bw")),
            ("avx512cd", has!("avx512cd")),
            ("avx512dq", has!("avx512dq")),
            ("avx512vl", has!("avx512vl")),
        ]);
    }
    #[cfg(all(target_arch = "aarch64", rustyget_tier = "apple-m4"))]
    {
        use std::arch::is_aarch64_feature_detected as has;
        need.extend([("i8mm", has!("i8mm")), ("bf16", has!("bf16"))]);
    }
    need.into_iter()
        .filter(|&(_, ok)| !ok)
        .map(|(f, _)| f)
        .collect()
}

#[no_mangle]
pub unsafe extern "C" fn main(
    argc: c_int,
    argv: *mut *mut c_char,
    argp: *mut *mut c_char,
) -> c_int {
    // A tier build on a CPU below its tier: say which build to use before any
    // tier-specific code runs, instead of dying on an illegal instruction.
    let missing = missing_tier_features();
    if !missing.is_empty() {
        let advice = if cfg!(rustyget_tier = "x86-64-v4") {
            "Install or run the default (x86-64-v3) build instead."
        } else {
            "This CPU is below RustyGet's supported floor (Intel 8th gen / AVX2, Apple M4)."
        };
        eprintln!(
            "This RustyGet binary was built for newer CPUs: this CPU lacks {}. {advice}",
            missing.join(", ")
        );
        return 1;
    }
    nzbget_rs_register_daemon_hooks(Some(nzbget_c_util_init));
    nzbget_rs_main(argc, argv, argp)
}
