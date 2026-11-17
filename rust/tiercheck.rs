//! The CPU tier check behind `rust/entry.rs`.
//!
//! A release build assumes its CPU tier (`-C target-cpu`), so code compiled
//! with it may use instructions an older CPU lacks. build.rs compiles this
//! file on its own for the compiler's default CPU, with the tier passed as
//! `--cfg rustyget_tier="..."`, so the check really runs on the CPU (with
//! target-cpu set, std's detection macros would answer "yes" at compile
//! time). It is `no_std` and cannot panic, so the object needs nothing but
//! the C library.

#![no_std]

use core::ffi::c_int;

#[allow(dead_code)]
extern "C" {
    fn write(fd: c_int, buf: *const u8, count: usize) -> isize;
}

/// A fixed buffer for the message (no allocation, no panics).
#[allow(dead_code)]
struct Msg {
    buf: [u8; 512],
    len: usize,
}

#[allow(dead_code)]
impl Msg {
    fn push(&mut self, s: &[u8]) {
        for &b in s {
            if let Some(slot) = self.buf.get_mut(self.len) {
                *slot = b;
                self.len += 1;
            }
        }
    }
}

/// Prints the message for the features in `need` that are missing and
/// returns 1, or returns 0 when none is.
#[allow(dead_code)]
fn report(need: &[(&[u8], bool)]) -> c_int {
    let mut msg = Msg {
        buf: [0; 512],
        len: 0,
    };
    msg.push(b"This RustyGet binary was built for newer CPUs: this CPU lacks ");
    let mut missing = 0;
    for &(name, ok) in need {
        if !ok {
            if missing > 0 {
                msg.push(b", ");
            }
            msg.push(name);
            missing += 1;
        }
    }
    if missing == 0 {
        return 0;
    }
    msg.push(b". ");
    if cfg!(rustyget_tier = "x86-64-v4") {
        msg.push(b"Install or run the default (x86-64-v3) build instead.");
    } else {
        msg.push(b"This CPU is below RustyGet's supported floor (Intel 8th gen / AVX2, Apple M4).");
    }
    msg.push(b"\n");
    let len = if msg.len <= msg.buf.len() {
        msg.len
    } else {
        msg.buf.len()
    };
    unsafe { write(2, msg.buf.as_ptr(), len) };
    1
}

/// CPU features as std's runtime detection reports them (with the operating
/// system's AVX and AVX-512 state support, as std checks it).
#[cfg(target_arch = "x86_64")]
#[allow(dead_code)]
struct X86 {
    leaf1_ecx: u32,
    leaf7_ebx: u32,
    ext1_ecx: u32,
    os_avx: bool,
    os_avx512: bool,
}

#[cfg(target_arch = "x86_64")]
#[allow(dead_code, unused_unsafe)]
fn x86() -> X86 {
    use core::arch::x86_64::{__cpuid_count, __get_cpuid_max};
    let (max, _) = unsafe { __get_cpuid_max(0) };
    let leaf1_ecx = if max >= 1 {
        unsafe { __cpuid_count(1, 0) }.ecx
    } else {
        0
    };
    let leaf7_ebx = if max >= 7 {
        unsafe { __cpuid_count(7, 0) }.ebx
    } else {
        0
    };
    let (ext_max, _) = unsafe { __get_cpuid_max(0x8000_0000) };
    let ext1_ecx = if ext_max >= 0x8000_0001 {
        unsafe { __cpuid_count(0x8000_0001, 0) }.ecx
    } else {
        0
    };
    let osxsave = leaf1_ecx & (1 << 27) != 0;
    let xcr0 = if osxsave {
        let (eax, edx): (u32, u32);
        unsafe {
            core::arch::asm!(
                "xgetbv",
                in("ecx") 0u32,
                out("eax") eax,
                out("edx") edx,
                options(nomem, nostack, preserves_flags)
            )
        };
        (u64::from(edx) << 32) | u64::from(eax)
    } else {
        0
    };
    let os_avx = xcr0 & 0x6 == 0x6;
    X86 {
        leaf1_ecx,
        leaf7_ebx,
        ext1_ecx,
        os_avx,
        os_avx512: os_avx && xcr0 & 0xe0 == 0xe0,
    }
}

#[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
#[allow(dead_code)]
fn arm_feature(name: &core::ffi::CStr) -> bool {
    extern "C" {
        fn sysctlbyname(
            name: *const core::ffi::c_char,
            oldp: *mut core::ffi::c_void,
            oldlenp: *mut usize,
            newp: *mut core::ffi::c_void,
            newlen: usize,
        ) -> c_int;
    }
    let mut v: c_int = 0;
    let mut len = core::mem::size_of::<c_int>();
    let r = unsafe {
        sysctlbyname(
            name.as_ptr(),
            (&mut v as *mut c_int).cast(),
            &mut len,
            core::ptr::null_mut(),
            0,
        )
    };
    r == 0 && v != 0
}

#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
#[allow(dead_code)]
fn arm_hwcap2(bit: u32) -> bool {
    extern "C" {
        fn getauxval(t: core::ffi::c_ulong) -> core::ffi::c_ulong;
    }
    const AT_HWCAP2: core::ffi::c_ulong = 26;
    unsafe { getauxval(AT_HWCAP2) }
    &(1 << bit) != 0
}

#[unsafe(no_mangle)]
pub extern "C" fn rustyget_tier_check() -> c_int {
    #[cfg(all(
        target_arch = "x86_64",
        any(rustyget_tier = "x86-64-v3", rustyget_tier = "x86-64-v4")
    ))]
    {
        let c = x86();
        let bit = |r: u32, b: u32| r & (1 << b) != 0;
        let v3: [(&[u8], bool); 7] = [
            (b"avx2", c.os_avx && bit(c.leaf7_ebx, 5)),
            (b"bmi1", bit(c.leaf7_ebx, 3)),
            (b"bmi2", bit(c.leaf7_ebx, 8)),
            (b"fma", c.os_avx && bit(c.leaf1_ecx, 12)),
            (b"lzcnt", bit(c.ext1_ecx, 5)),
            (b"movbe", bit(c.leaf1_ecx, 22)),
            (b"f16c", c.os_avx && bit(c.leaf1_ecx, 29)),
        ];
        #[cfg(rustyget_tier = "x86-64-v3")]
        return report(&v3);
        #[cfg(rustyget_tier = "x86-64-v4")]
        {
            let e = c.os_avx512;
            let [a, b, cc, d, f, g, h] = v3;
            return report(&[
                a,
                b,
                cc,
                d,
                f,
                g,
                h,
                (b"avx512f", e && bit(c.leaf7_ebx, 16)),
                (b"avx512bw", e && bit(c.leaf7_ebx, 30)),
                (b"avx512cd", e && bit(c.leaf7_ebx, 28)),
                (b"avx512dq", e && bit(c.leaf7_ebx, 17)),
                (b"avx512vl", e && bit(c.leaf7_ebx, 31)),
            ]);
        }
    }
    #[cfg(all(
        target_arch = "aarch64",
        target_vendor = "apple",
        rustyget_tier = "apple-m4"
    ))]
    {
        return report(&[
            (b"i8mm", arm_feature(c"hw.optional.arm.FEAT_I8MM")),
            (b"bf16", arm_feature(c"hw.optional.arm.FEAT_BF16")),
        ]);
    }
    #[cfg(all(
        target_arch = "aarch64",
        target_os = "linux",
        rustyget_tier = "apple-m4"
    ))]
    {
        // HWCAP2_I8MM and HWCAP2_BF16
        return report(&[(b"i8mm", arm_hwcap2(13)), (b"bf16", arm_hwcap2(14))]);
    }
    #[allow(unreachable_code)]
    0
}
