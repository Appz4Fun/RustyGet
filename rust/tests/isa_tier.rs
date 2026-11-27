//! Which SIMD kernels the real callers reach on a CI runner of one
//! instruction-set tier (`scripts/ci/isa-tests.sh`; the tiers and runner
//! labels are in docs/CPU_TIERS.md, "CI tiers").
//!
//! The kernels fall back silently: on a CPU without AVX-512 every test still
//! passes, on the AVX2 kernel. This test turns that into a failure on a runner
//! that claims a tier: it names the payload kernel and break scan nntp-rs
//! chose for the Decoder and the GF(2^16) kernel par-rs chose for repair and
//! create, under the cap in `RUSTYGET_SIMD` (or `NNTP_RS_SIMD`) and
//! `PAR_RS_SIMD`, and checks them against the tier in `RUSTYGET_CI_TIER`.
//! Each library reads its cap once per process, so each cap needs a run of
//! its own. Ignored by default, since only a tagged runner knows its tier.

/// What the tier and caps should select: (nntp-rs payload kernel, nntp-rs
/// break scan, par-rs GF kernel).
fn expected(tier: &str, cap: &str, par_off: bool) -> (u32, u8, &'static str) {
    // nntp-rs: payload 0 scalar, 1 SSSE3, 2 AVX2, 3 AVX-512 (BW and VBMI2),
    // 4 NEON; break scan 1 scalar, 2 AVX2, 3 AVX-512BW, 4 NEON. `scalar` and
    // `sse2` both cap both to scalar, `avx2` caps x86 at AVX2.
    let (best_payload, best_scan, best_par) = match tier {
        "avx512" => (3, 3, "avx512+gfni"),
        "avx2" => (2, 2, "avx2"),
        "neon" | "apple-m" => (4, 4, "neon"),
        other => panic!("unknown RUSTYGET_CI_TIER {other:?} (avx512, avx2, neon, apple-m)"),
    };
    let (payload, scan) = match cap {
        "scalar" | "sse2" => (0, 1),
        "avx2" if best_payload == 3 => (2, 2),
        _ => (best_payload, best_scan),
    };
    // par-rs has one switch, all or nothing; no per-level cap.
    (payload, scan, if par_off { "scalar" } else { best_par })
}

#[test]
#[ignore = "run by scripts/ci/isa-tests.sh on a runner of the tier named in RUSTYGET_CI_TIER"]
fn reaches_the_tier_kernels() {
    let tier = std::env::var("RUSTYGET_CI_TIER").expect("RUSTYGET_CI_TIER names the runner's tier");
    match tier.as_str() {
        "avx512" | "avx2" => assert!(
            cfg!(target_arch = "x86_64"),
            "tier {tier} on a non-x86_64 build"
        ),
        _ => assert!(
            cfg!(target_arch = "aarch64"),
            "tier {tier} on a non-aarch64 build"
        ),
    }
    let cap = std::env::var("NNTP_RS_SIMD")
        .or_else(|_| std::env::var("RUSTYGET_SIMD"))
        .unwrap_or_default();
    let par_off = matches!(
        std::env::var("PAR_RS_SIMD")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "off" | "0" | "false" | "no" | "scalar"
    );
    let got = (
        nntp_rs::yenc_simd::kernel(),
        nntp_rs::decoder::scan_kernel(),
        par_rs::cpu::gf_kernel().to_string(),
    );
    let want = expected(&tier, &cap, par_off);
    eprintln!(
        "isa tier={tier} cap={cap:?} par_off={par_off}: payload={} scan={} par={}",
        got.0, got.1, got.2
    );
    assert_eq!(
        (got.0, got.1, got.2.as_str()),
        want,
        "tier {tier}, cap {cap:?}, par_off {par_off}"
    );
}

#[test]
fn expectations_cover_every_tier_and_cap() {
    assert_eq!(expected("avx512", "", false), (3, 3, "avx512+gfni"));
    assert_eq!(expected("avx512", "avx2", false), (2, 2, "avx512+gfni"));
    assert_eq!(expected("avx512", "scalar", true), (0, 1, "scalar"));
    assert_eq!(expected("avx2", "avx2", false), (2, 2, "avx2"));
    assert_eq!(expected("avx2", "sse2", false), (0, 1, "avx2"));
    assert_eq!(expected("neon", "avx2", false), (4, 4, "neon"));
    assert_eq!(expected("apple-m", "scalar", true), (0, 1, "scalar"));
}
