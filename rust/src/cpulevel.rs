//! Which CPU tier this binary was built for, and a startup check that the
//! host has it. Release builds come in tiers (x86-64-v3 is the floor for
//! x86_64, x86-64-v4 adds AVX-512; apple-m4 is the floor for macOS, apple-m5
//! the newer tier). A binary built for a tier the CPU lacks would die on an
//! illegal instruction somewhere random, so the daemon says which build to use
//! instead. Runtime-dispatched kernels inside every tier still pick the best
//! code path for the actual CPU.

/// The tier named by the features this binary was compiled with.
pub fn build_level() -> &'static str {
    if cfg!(all(target_arch = "x86_64", target_feature = "avx512bw", target_feature = "avx512vl")) {
        "x86-64-v4"
    } else if cfg!(all(target_arch = "x86_64", target_feature = "avx2", target_feature = "bmi2", target_feature = "fma")) {
        "x86-64-v3"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64", target_feature = "i8mm", target_feature = "bf16")) {
        // apple-m2 and newer; release builds use apple-m4 or apple-m5
        "apple-m4"
    } else {
        "generic"
    }
}

/// Features this binary was compiled to assume that the running CPU lacks.
pub fn missing_features() -> Vec<&'static str> {
    #[allow(unused_mut)]
    let mut missing = Vec::new();
    #[cfg(target_arch = "x86_64")]
    {
        macro_rules! need {
            ($($f:tt),*) => {$(
                if cfg!(target_feature = $f) && !std::arch::is_x86_feature_detected!($f) {
                    missing.push($f);
                }
            )*};
        }
        need!(
            "avx2", "bmi1", "bmi2", "fma", "lzcnt", "movbe", "f16c", "avx512f", "avx512bw",
            "avx512cd", "avx512dq", "avx512vl"
        );
    }
    #[cfg(target_arch = "aarch64")]
    {
        macro_rules! need {
            ($($f:tt),*) => {$(
                if cfg!(target_feature = $f) && !std::arch::is_aarch64_feature_detected!($f) {
                    missing.push($f);
                }
            )*};
        }
        need!("i8mm", "bf16", "dotprod", "fp16", "sha3", "sve", "sve2");
    }
    missing
}

/// The message for a host that lacks this build's tier, or None when it fits.
pub fn mismatch_message() -> Option<String> {
    let missing = missing_features();
    if missing.is_empty() {
        return None;
    }
    let use_instead = if cfg!(target_arch = "x86_64") { "x86-64-v3" } else { "apple-m4 or generic" };
    Some(format!(
        "This RustyGet binary was built for {} CPUs, but this CPU lacks {}. Install or run the {} build instead.",
        build_level(),
        missing.join(", "),
        use_instead
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_test_host_runs_this_build() {
        // The test binary itself would not get here otherwise, so the check
        // must agree with reality.
        assert_eq!(super::mismatch_message(), None, "{}", super::build_level());
    }
}
