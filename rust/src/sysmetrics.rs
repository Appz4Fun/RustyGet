//! Live machine metrics for the Status and Statistics pages.
//!
//! `snapshot()` reads the load averages, CPU count and usage, memory, swap and
//! memory compression (zswap / zram) of the machine RustyGet runs on, plus the
//! CPU time and resident memory of the RustyGet process itself.
//!
//! A sampler thread records one `Sample` every `FINE_INTERVAL_SEC` seconds into
//! a one hour ring, and folds every minute into a 24 hour ring of averages, so
//! the Statistics page can draw speed, connections, load, CPU and RAM over time
//! even for the time the page was closed. The `systemstats` RPC serves both.
//!
//! Linux reads `/proc`; macOS reads `sysctl` and the Mach host statistics.
//! Other systems report what POSIX gives (load averages, CPU count, process
//! CPU time) and zeros for the rest.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Seconds between two samples of the fine (one hour) history.
pub const FINE_INTERVAL_SEC: i64 = 5;
/// Samples kept in the fine history: one hour.
pub const FINE_CAPACITY: usize = 720;
/// Seconds covered by one sample of the coarse (24 hour) history.
pub const COARSE_INTERVAL_SEC: i64 = 60;
/// Samples kept in the coarse history: 24 hours.
pub const COARSE_CAPACITY: usize = 1440;

// ---------------------------------------------------------------------------
// Parsers (pure, unit tested on every platform)
// ---------------------------------------------------------------------------

/// Aggregate machine CPU time from the first `cpu ` line of `/proc/stat`,
/// as `(total, idle)` in clock ticks. Idle counts `idle` and `iowait`;
/// `guest` time is already included in `user` and is not added twice.
pub fn parse_proc_stat(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().find(|l| l.starts_with("cpu "))?;
    let vals: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(|v| v.parse().unwrap_or(0))
        .collect();
    if vals.len() < 4 {
        return None;
    }
    let total: u64 = vals.iter().sum();
    let idle = vals[3] + vals.get(4).copied().unwrap_or(0);
    Some((total, idle))
}

/// Memory figures in bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemInfo {
    pub total: u64,
    pub free: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
}

impl MemInfo {
    /// Memory in use by everything on the machine: total minus available.
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }
}

/// Parses `/proc/meminfo` (values in kB). Without `MemAvailable` (kernels
/// before 3.14) available is free plus buffers and page cache.
pub fn parse_meminfo(text: &str) -> MemInfo {
    let mut m = MemInfo::default();
    let mut buffers = 0;
    let mut cached = 0;
    let mut have_available = false;
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(key), Some(val)) = (it.next(), it.next()) else {
            continue;
        };
        let bytes = val.parse::<u64>().unwrap_or(0).saturating_mul(1024);
        match key {
            "MemTotal:" => m.total = bytes,
            "MemFree:" => m.free = bytes,
            "MemAvailable:" => {
                m.available = bytes;
                have_available = true;
            }
            "Buffers:" => buffers = bytes,
            "Cached:" => cached = bytes,
            "SwapTotal:" => m.swap_total = bytes,
            "SwapFree:" => m.swap_free = bytes,
            _ => {}
        }
    }
    if !have_available {
        m.available = m.free + buffers + cached;
    }
    m.available = m.available.min(m.total);
    m
}

/// Resident set size in bytes from `/proc/self/statm` (second field, pages).
pub fn parse_statm_rss(text: &str, page_size: u64) -> u64 {
    text.split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(0, |pages| pages.saturating_mul(page_size))
}

/// True when `/proc/swaps` lists a zram device in use as swap.
pub fn proc_swaps_has_zram(text: &str) -> bool {
    text.lines()
        .skip(1)
        .any(|l| l.split_whitespace().next().is_some_and(|d| d.contains("zram")))
}

/// True when `/sys/module/zswap/parameters/enabled` says so (`Y` or `1`).
pub fn zswap_flag_enabled(text: &str) -> bool {
    matches!(text.trim(), "Y" | "y" | "1")
}

/// What the CPU is: maker, model and, where the model name tells, generation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CpuIdentity {
    pub vendor: String,
    pub model: String,
    pub generation: String,
}

/// Reads maker and model from `/proc/cpuinfo`. x86 has `vendor_id` and
/// `model name`; ARM boards have `CPU implementer` and often `Model` or
/// `Hardware` instead.
pub fn parse_cpuinfo(text: &str) -> CpuIdentity {
    let mut vendor_id = "";
    let mut model = "";
    let mut implementer = "";
    let mut board = "";
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "vendor_id" if vendor_id.is_empty() => vendor_id = v,
            "model name" if model.is_empty() => model = v,
            "CPU implementer" if implementer.is_empty() => implementer = v,
            "Model" | "Hardware" if board.is_empty() => board = v,
            _ => {}
        }
    }
    let model = if model.is_empty() { board } else { model };
    let vendor = match vendor_id {
        "GenuineIntel" => "Intel".to_string(),
        "AuthenticAMD" => "AMD".to_string(),
        "" => arm_implementer_name(implementer)
            .map(str::to_string)
            .unwrap_or_else(|| vendor_from_model(model)),
        other => other.to_string(),
    };
    CpuIdentity {
        vendor,
        model: model.to_string(),
        generation: cpu_generation(model),
    }
}

fn arm_implementer_name(code: &str) -> Option<&'static str> {
    let code = code.trim_start_matches("0x").to_ascii_lowercase();
    Some(match code.as_str() {
        "41" => "ARM",
        "42" => "Broadcom",
        "43" => "Cavium",
        "46" => "Fujitsu",
        "48" => "HiSilicon",
        "4e" => "NVIDIA",
        "51" => "Qualcomm",
        "61" => "Apple",
        "c0" => "Ampere",
        _ => return None,
    })
}

/// Maker guessed from a model string, for systems without `vendor_id`.
pub fn vendor_from_model(model: &str) -> String {
    let lower = model.to_ascii_lowercase();
    for (needle, name) in [
        ("intel", "Intel"),
        ("amd", "AMD"),
        ("ryzen", "AMD"),
        ("epyc", "AMD"),
        ("apple", "Apple"),
        ("ampere", "Ampere"),
        ("qualcomm", "Qualcomm"),
        ("snapdragon", "Qualcomm"),
        ("raspberry", "Broadcom"),
        ("cortex", "ARM"),
    ] {
        if lower.contains(needle) {
            return name.to_string();
        }
    }
    String::new()
}

/// Digits that start at byte `at` of `s`.
fn digits_at(s: &str, at: usize) -> &str {
    let rest = &s[at..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    &rest[..end]
}

/// The generation a CPU model name tells, best effort, or an empty string.
///
/// Intel Core `i7-12700K` is "12th gen", `i5-8250U` "8th gen", Core Ultra
/// `155H` "Core Ultra Series 1"; AMD Ryzen `5950X` is "Ryzen 5000 (Zen 3)",
/// EPYC `7763` "EPYC Milan (Zen 3)"; Apple `M2 Pro` is "M2"; Xeon `v4` is "v4".
pub fn cpu_generation(model: &str) -> String {
    let lower = model.to_ascii_lowercase();

    if let Some(pos) = lower.find("apple m") {
        let d = digits_at(&lower, pos + "apple m".len());
        if !d.is_empty() {
            return format!("M{d}");
        }
    }

    if let Some(pos) = lower.find("core(tm) ultra").or_else(|| lower.find("core ultra")) {
        // "Core Ultra 7 155H": the series is the first digit of the model number.
        let tail = &lower[pos..];
        if let Some(num) = tail
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| w.len() >= 3 && w.as_bytes()[0].is_ascii_digit())
            .find(|w| digits_at(w, 0).len() == 3)
        {
            return format!("Core Ultra Series {}", &num[..1]);
        }
    }

    for tier in ["i3-", "i5-", "i7-", "i9-"] {
        if let Some(pos) = lower.find(tier) {
            let d = digits_at(&lower, pos + tier.len());
            let gen = match d.len() {
                5 => d[..2].parse::<u32>().ok(),
                4 => d[..1].parse::<u32>().ok(),
                _ => None,
            };
            if let Some(g) = gen.filter(|g| *g > 0) {
                return format!("{}{} gen", g, ordinal_suffix(g));
            }
        }
    }

    if lower.contains("xeon") {
        if let Some(pos) = lower.rfind(" v") {
            let d = digits_at(&lower, pos + 2);
            if !d.is_empty() {
                return format!("v{d}");
            }
        }
    }

    if let Some(pos) = lower.find("ryzen") {
        // "Ryzen 9 5950X" / "Ryzen 7 PRO 4750U": the model number's first digit.
        let tail = &lower[pos + "ryzen".len()..];
        if let Some(num) = tail
            .split(|c: char| !c.is_ascii_alphanumeric())
            .find(|w| digits_at(w, 0).len() == 4)
        {
            let series = &num[..1];
            let mobile = num[4..].starts_with(['u', 'h', 'c'])
                || tail.contains(" hs")
                || tail.contains(" hx");
            let zen = if mobile {
                // Mobile parts: from the 7000 series on the third digit names the
                // architecture (7520U is Zen 2, 7840HS Zen 4); 4000 is Zen 2 and
                // 6000 Zen 3+; 5000 mixes Zen 2 and Zen 3, so it gets no suffix.
                match (series, &num[2..3]) {
                    ("4", _) => "Zen 2",
                    ("6", _) => "Zen 3+",
                    ("7" | "8" | "9", "1") => "Zen+",
                    ("7" | "8" | "9", "2") => "Zen 2",
                    ("7" | "8" | "9", "3") => "Zen 3",
                    ("7" | "8" | "9", "4") => "Zen 4",
                    ("7" | "8" | "9", "5") => "Zen 5",
                    _ => "",
                }
            } else {
                match series {
                    "1" => "Zen",
                    "2" => "Zen+",
                    "3" => "Zen 2",
                    "4" => "Zen 2",
                    "5" => "Zen 3",
                    "7" => "Zen 4",
                    "8" => "Zen 4",
                    "9" => "Zen 5",
                    _ => "",
                }
            };
            return if zen.is_empty() {
                format!("Ryzen {series}000")
            } else {
                format!("Ryzen {series}000 ({zen})")
            };
        }
    }

    if let Some(pos) = lower.find("epyc") {
        // EPYC 7xx1..7xx3 and 9xx4/9xx5: the last digit names the family.
        let tail = &lower[pos + "epyc".len()..];
        if let Some(num) = tail
            .split(|c: char| !c.is_ascii_alphanumeric())
            .find(|w| digits_at(w, 0).len() == 4)
        {
            let name = match &num[3..4] {
                "1" => "Naples (Zen)",
                "2" => "Rome (Zen 2)",
                "3" => "Milan (Zen 3)",
                "4" => "Genoa (Zen 4)",
                "5" => "Turin (Zen 5)",
                _ => "",
            };
            if !name.is_empty() {
                return format!("EPYC {name}");
            }
        }
    }

    String::new()
}

fn ordinal_suffix(n: u32) -> &'static str {
    match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    }
}

/// Percent of `delta_busy` in `delta_total`, clamped to 0..=100.
pub fn percent(delta_busy: f64, delta_total: f64) -> f32 {
    if delta_total <= 0.0 {
        return 0.0;
    }
    ((delta_busy / delta_total) * 100.0).clamp(0.0, 100.0) as f32
}

// ---------------------------------------------------------------------------
// Platform readers
// ---------------------------------------------------------------------------

/// Total and available memory of the machine now.
pub fn memory_info() -> MemInfo {
    platform::memory()
}

/// Number of CPUs the system has online.
pub fn cpu_count() -> usize {
    let n = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    if n > 0 {
        n as usize
    } else {
        std::thread::available_parallelism().map_or(1, usize::from)
    }
}

/// Number of physical CPU cores (hyper-threads of one core count once), never more than
/// [`cpu_count`] and at least 1. Falls back to the logical count when the topology is unknown.
pub fn physical_cpu_count() -> usize {
    let logical = cpu_count().max(1);
    platform::physical_cores().map_or(logical, |n| n.clamp(1, logical))
}

/// Distinct (`physical id`, `core id`) pairs of a Linux `/proc/cpuinfo`; `None` when it lists
/// no core ids (many ARM kernels), where every logical CPU is its own core.
pub fn parse_cpuinfo_physical_cores(text: &str) -> Option<usize> {
    let mut cores = std::collections::HashSet::new();
    let (mut package, mut core) = (None::<String>, None::<String>);
    let field = |line: &str| line.split_once(':').map(|(_, v)| v.trim().to_owned());
    for line in text.lines().chain(std::iter::once("")) {
        if line.trim().is_empty() {
            if let Some(core) = core.take() {
                cores.insert((package.take().unwrap_or_default(), core));
            }
            package = None;
        } else if line.starts_with("physical id") {
            package = field(line);
        } else if line.starts_with("core id") {
            core = field(line);
        }
    }
    (!cores.is_empty()).then_some(cores.len())
}

/// The 1, 5 and 15 minute load averages.
pub fn load_average() -> [f64; 3] {
    let mut l = [0.0f64; 3];
    let n = unsafe { libc::getloadavg(l.as_mut_ptr(), 3) };
    if n < 3 {
        for v in l.iter_mut().skip(n.max(0) as usize) {
            *v = 0.0;
        }
    }
    l
}

/// CPU seconds (user + system) the RustyGet process has used.
pub fn process_cpu_seconds() -> f64 {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) } != 0 {
        return 0.0;
    }
    let tv = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1_000_000.0;
    tv(ru.ru_utime) + tv(ru.ru_stime)
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;

    fn read(path: &str) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    pub fn machine_cpu_ticks() -> Option<(u64, u64)> {
        parse_proc_stat(&read("/proc/stat"))
    }

    pub fn memory() -> MemInfo {
        parse_meminfo(&read("/proc/meminfo"))
    }

    pub fn process_rss() -> u64 {
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        parse_statm_rss(&read("/proc/self/statm"), page.max(4096) as u64)
    }

    pub fn zswap() -> bool {
        zswap_flag_enabled(&read("/sys/module/zswap/parameters/enabled"))
    }

    pub fn zram() -> bool {
        proc_swaps_has_zram(&read("/proc/swaps"))
    }

    pub fn compressor() -> bool {
        false
    }

    pub fn cpu_identity() -> CpuIdentity {
        parse_cpuinfo(&read("/proc/cpuinfo"))
    }

    pub fn physical_cores() -> Option<usize> {
        parse_cpuinfo_physical_cores(&read("/proc/cpuinfo"))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::ffi::CStr;
    use std::os::raw::{c_int, c_void};

    const HOST_CPU_LOAD_INFO: c_int = 3;
    const HOST_VM_INFO64: c_int = 4;

    /// `vm_statistics64` from `<mach/vm_statistics.h>`.
    #[repr(C)]
    #[derive(Default)]
    struct VmStatistics64 {
        free_count: u32,
        active_count: u32,
        inactive_count: u32,
        wire_count: u32,
        zero_fill_count: u64,
        reactivations: u64,
        pageins: u64,
        pageouts: u64,
        faults: u64,
        cow_faults: u64,
        lookups: u64,
        hits: u64,
        purges: u64,
        purgeable_count: u32,
        speculative_count: u32,
        decompressions: u64,
        compressions: u64,
        swapins: u64,
        swapouts: u64,
        compressor_page_count: u32,
        throttled_count: u32,
        external_page_count: u32,
        internal_page_count: u32,
        total_uncompressed_pages_in_compressor: u64,
    }

    /// `struct xsw_usage` from `<sys/sysctl.h>`.
    #[repr(C)]
    #[derive(Default)]
    struct XswUsage {
        total: u64,
        avail: u64,
        used: u64,
        pagesize: u32,
        encrypted: u32,
    }

    extern "C" {
        fn mach_host_self() -> u32;
        fn host_statistics(host: u32, flavor: c_int, info: *mut c_int, count: *mut u32) -> c_int;
        fn host_statistics64(host: u32, flavor: c_int, info: *mut c_int, count: *mut u32)
            -> c_int;
    }

    fn sysctl_raw<T: Default>(name: &CStr) -> Option<T> {
        let mut val = T::default();
        let mut len = std::mem::size_of::<T>();
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                (&mut val as *mut T).cast::<c_void>(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        (rc == 0).then_some(val)
    }

    pub fn physical_cores() -> Option<usize> {
        sysctl_raw::<c_int>(c"hw.physicalcpu")
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n > 0)
    }

    fn page_size() -> u64 {
        let p = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if p > 0 {
            p as u64
        } else {
            16384
        }
    }

    pub fn machine_cpu_ticks() -> Option<(u64, u64)> {
        // cpu_ticks[USER, SYSTEM, IDLE, NICE]
        let mut ticks = [0u32; 4];
        let mut count: u32 = 4;
        let rc = unsafe {
            host_statistics(
                mach_host_self(),
                HOST_CPU_LOAD_INFO,
                ticks.as_mut_ptr().cast::<c_int>(),
                &mut count,
            )
        };
        if rc != 0 {
            return None;
        }
        let total = ticks.iter().map(|&t| u64::from(t)).sum();
        Some((total, u64::from(ticks[2])))
    }

    pub fn memory() -> MemInfo {
        let total = sysctl_raw::<u64>(c"hw.memsize").unwrap_or(0);
        let mut vm = VmStatistics64::default();
        let mut count = (std::mem::size_of::<VmStatistics64>() / 4) as u32;
        let rc = unsafe {
            host_statistics64(
                mach_host_self(),
                HOST_VM_INFO64,
                (&mut vm as *mut VmStatistics64).cast::<c_int>(),
                &mut count,
            )
        };
        let page = page_size();
        let (free, available) = if rc == 0 {
            let free = u64::from(vm.free_count.saturating_sub(vm.speculative_count)) * page;
            // What Activity Monitor calls cached files can be reclaimed at once.
            let avail = u64::from(vm.free_count)
                + u64::from(vm.external_page_count)
                + u64::from(vm.purgeable_count);
            (free, (avail * page).min(total))
        } else {
            (0, 0)
        };
        let swap = sysctl_raw::<XswUsage>(c"vm.swapusage").unwrap_or_default();
        MemInfo {
            total,
            free,
            available,
            swap_total: swap.total,
            swap_free: swap.avail,
        }
    }

    pub fn process_rss() -> u64 {
        let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_taskinfo>() as c_int;
        let n = unsafe {
            libc::proc_pidinfo(
                libc::getpid(),
                libc::PROC_PIDTASKINFO,
                0,
                (&mut info as *mut libc::proc_taskinfo).cast::<c_void>(),
                size,
            )
        };
        if n == size {
            info.pti_resident_size
        } else {
            0
        }
    }

    pub fn zswap() -> bool {
        false
    }

    pub fn zram() -> bool {
        false
    }

    /// macOS compresses memory with its own compressor, always on.
    pub fn compressor() -> bool {
        true
    }

    pub fn cpu_identity() -> CpuIdentity {
        let (model, _) = crate::systemhealth::cpu_detect_info(b"");
        let model = String::from_utf8_lossy(&model).trim().to_string();
        CpuIdentity {
            vendor: vendor_from_model(&model),
            generation: cpu_generation(&model),
            model,
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod platform {
    use super::*;

    pub fn machine_cpu_ticks() -> Option<(u64, u64)> {
        None
    }
    pub fn memory() -> MemInfo {
        MemInfo::default()
    }
    pub fn process_rss() -> u64 {
        0
    }
    pub fn zswap() -> bool {
        false
    }
    pub fn zram() -> bool {
        false
    }
    pub fn compressor() -> bool {
        false
    }
    pub fn cpu_identity() -> CpuIdentity {
        CpuIdentity::default()
    }
    pub fn physical_cores() -> Option<usize> {
        None
    }
}

/// Memory compression in use, for display: `zswap`, `zram`, both, the macOS
/// compressor, or `none`.
pub fn memory_compression_label(zswap: bool, zram: bool, compressor: bool) -> &'static str {
    match (zswap, zram, compressor) {
        (true, true, _) => "zswap + zram",
        (true, false, _) => "zswap",
        (false, true, _) => "zram",
        (false, false, true) => "macOS compressor",
        _ => "none",
    }
}

// ---------------------------------------------------------------------------
// CPU usage meter and history
// ---------------------------------------------------------------------------

/// One point of the history.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sample {
    /// Unix time, seconds.
    pub time: i64,
    /// Download speed of all news servers together, bytes per second.
    pub speed: i64,
    /// News server connections in use.
    pub connections: i32,
    pub load1: f32,
    pub load5: f32,
    pub load15: f32,
    /// Busy CPU of the whole machine, percent of all CPUs.
    pub cpu: f32,
    /// CPU used by RustyGet, percent of all CPUs (so it compares with `cpu`).
    pub cpu_process: f32,
    /// Memory in use on the machine, bytes.
    pub mem_used: u64,
    /// Resident memory of RustyGet, bytes.
    pub mem_process: u64,
}

/// Names of the fields of a history row, in order.
pub const SAMPLE_FIELDS: [&str; 10] = [
    "Time",
    "Speed",
    "Connections",
    "Load1",
    "Load5",
    "Load15",
    "Cpu",
    "ProcessCpu",
    "MemUsed",
    "ProcessMem",
];

/// Turns successive CPU counter readings into percentages.
#[derive(Debug, Default)]
pub struct CpuMeter {
    last: Option<(Instant, Option<(u64, u64)>, f64)>,
    cpus: usize,
}

impl CpuMeter {
    pub fn new(cpus: usize) -> Self {
        Self {
            last: None,
            cpus: cpus.max(1),
        }
    }

    /// Feeds one reading; returns `(machine %, process %)` since the last one.
    pub fn feed(
        &mut self,
        now: Instant,
        machine: Option<(u64, u64)>,
        process_sec: f64,
    ) -> Option<(f32, f32)> {
        let prev = self.last.replace((now, machine, process_sec));
        let (t0, m0, p0) = prev?;
        let wall = now.duration_since(t0).as_secs_f64();
        if wall <= 0.0 {
            return None;
        }
        let machine_pct = match (m0, machine) {
            (Some((tot0, idle0)), Some((tot1, idle1))) if tot1 > tot0 => {
                let dt = (tot1 - tot0) as f64;
                let di = idle1.saturating_sub(idle0) as f64;
                percent(dt - di, dt)
            }
            _ => 0.0,
        };
        let proc_pct = percent(process_sec - p0, wall * self.cpus as f64);
        Some((machine_pct, proc_pct))
    }
}

/// The fine (5 s) and coarse (1 min average) rings.
#[derive(Debug, Default)]
pub struct History {
    fine: VecDeque<Sample>,
    coarse: VecDeque<Sample>,
    pending: Vec<Sample>,
}

impl History {
    pub fn push(&mut self, s: Sample) {
        if self.fine.len() == FINE_CAPACITY {
            self.fine.pop_front();
        }
        self.fine.push_back(s);

        let minute = s.time.div_euclid(COARSE_INTERVAL_SEC);
        if let Some(first) = self.pending.first() {
            if first.time.div_euclid(COARSE_INTERVAL_SEC) != minute {
                let avg = average(&self.pending, first.time.div_euclid(COARSE_INTERVAL_SEC));
                if self.coarse.len() == COARSE_CAPACITY {
                    self.coarse.pop_front();
                }
                self.coarse.push_back(avg);
                self.pending.clear();
            }
        }
        self.pending.push(s);
    }

    /// Samples covering the last `range_sec` seconds: the 5 second ring up to an
    /// hour, the one minute averages beyond. Returns `(interval, samples)`.
    pub fn window(&self, range_sec: i64, now: i64) -> (i64, Vec<Sample>) {
        let from = now - range_sec;
        if range_sec <= FINE_INTERVAL_SEC * FINE_CAPACITY as i64 {
            let v = self.fine.iter().filter(|s| s.time > from).copied().collect();
            (FINE_INTERVAL_SEC, v)
        } else {
            let v = self.coarse.iter().filter(|s| s.time > from).copied().collect();
            (COARSE_INTERVAL_SEC, v)
        }
    }

    pub fn latest(&self) -> Option<Sample> {
        self.fine.back().copied()
    }
}

fn average(samples: &[Sample], minute: i64) -> Sample {
    let n = samples.len().max(1);
    let nf = n as f32;
    let mut a = Sample {
        time: minute * COARSE_INTERVAL_SEC,
        ..Sample::default()
    };
    let (mut speed, mut conns, mut mem, mut pmem) = (0i64, 0i64, 0u64, 0u64);
    for s in samples {
        speed += s.speed;
        conns += i64::from(s.connections);
        a.load1 += s.load1;
        a.load5 += s.load5;
        a.load15 += s.load15;
        a.cpu += s.cpu;
        a.cpu_process += s.cpu_process;
        mem += s.mem_used;
        pmem += s.mem_process;
    }
    a.speed = speed / n as i64;
    a.connections = (conns as f64 / n as f64).round() as i32;
    a.load1 /= nf;
    a.load5 /= nf;
    a.load15 /= nf;
    a.cpu /= nf;
    a.cpu_process /= nf;
    a.mem_used = mem / n as u64;
    a.mem_process = pmem / n as u64;
    a
}

struct Sampler {
    meter: CpuMeter,
    history: History,
}

fn sampler() -> &'static Mutex<Sampler> {
    static S: OnceLock<Mutex<Sampler>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(Sampler {
            meter: CpuMeter::new(cpu_count()),
            history: History::default(),
        })
    })
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn download_speed() -> i64 {
    crate::statmeter::active_statmeter().map_or(0, |s| s.current_download_speed())
}

fn connections_in_use() -> i32 {
    crate::net::active_server_pool().map_or(0, |p| p.connections_in_use() as i32)
}

/// Takes one reading and records it. Returns the new sample, or `None` for the
/// very first reading (CPU usage needs two).
pub fn tick() -> Option<Sample> {
    let machine = platform::machine_cpu_ticks();
    let proc_sec = process_cpu_seconds();
    let mem = platform::memory();
    let rss = platform::process_rss();
    let load = load_average();
    let time = now_unix();
    let speed = download_speed();
    let connections = connections_in_use();

    let mut g = sampler().lock().ok()?;
    let (cpu, cpu_process) = g.meter.feed(Instant::now(), machine, proc_sec)?;
    let s = Sample {
        time,
        speed,
        connections,
        load1: load[0] as f32,
        load5: load[1] as f32,
        load15: load[2] as f32,
        cpu,
        cpu_process,
        mem_used: mem.used(),
        mem_process: rss,
    };
    g.history.push(s);
    Some(s)
}

/// Starts the background sampler once per process; later calls do nothing.
///
/// The daemon calls this after it forks (a thread started before `fork` does
/// not exist in the child), and every `systemstats` request calls it too. The
/// process id check restarts the sampler in a forked child that inherited the
/// "started" flag without the thread.
pub fn start_sampler() {
    use std::sync::atomic::{AtomicI32, Ordering};
    static SAMPLER_PID: AtomicI32 = AtomicI32::new(0);
    let pid = unsafe { libc::getpid() } as i32;
    if SAMPLER_PID.swap(pid, Ordering::SeqCst) == pid {
        return;
    }
    let _ = tick();
    let _ = std::thread::Builder::new()
        .name("sysmetrics".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(FINE_INTERVAL_SEC as u64));
            if unsafe { libc::getpid() } as i32 != pid {
                return;
            }
            let _ = tick();
        });
}

// ---------------------------------------------------------------------------
// The `systemstats` RPC
// ---------------------------------------------------------------------------

/// Everything the Status page shows about the machine, right now.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub time: i64,
    pub load: [f64; 3],
    pub cpu_count: usize,
    pub cpu: f32,
    pub cpu_process: f32,
    pub cpu_identity: CpuIdentity,
    pub arch: String,
    pub mem: MemInfo,
    pub mem_process: u64,
    pub zswap: bool,
    pub zram: bool,
    pub compressor: bool,
    pub speed: i64,
    pub connections: i32,
}

fn cpu_identity_cached() -> CpuIdentity {
    static ID: OnceLock<CpuIdentity> = OnceLock::new();
    ID.get_or_init(|| {
        let mut id = platform::cpu_identity();
        if id.model.is_empty() {
            let (model, _) = crate::systemhealth::cpu_detect_info(b"");
            id.model = String::from_utf8_lossy(&model).trim().to_string();
            id.generation = cpu_generation(&id.model);
            if id.vendor.is_empty() {
                id.vendor = vendor_from_model(&id.model);
            }
        }
        id
    })
    .clone()
}

fn arch() -> String {
    std::env::consts::ARCH.to_string()
}

/// Reads the machine now. CPU usage comes from the sampler's latest reading
/// when it is fresh; otherwise two readings a quarter second apart.
pub fn snapshot() -> Snapshot {
    start_sampler();
    let now = now_unix();
    let latest = sampler().lock().ok().and_then(|g| g.history.latest());
    let (cpu, cpu_process) = match latest {
        Some(s) if now - s.time <= FINE_INTERVAL_SEC * 2 => (s.cpu, s.cpu_process),
        _ => {
            let mut m = CpuMeter::new(cpu_count());
            m.feed(Instant::now(), platform::machine_cpu_ticks(), process_cpu_seconds());
            std::thread::sleep(Duration::from_millis(250));
            m.feed(Instant::now(), platform::machine_cpu_ticks(), process_cpu_seconds())
                .unwrap_or((0.0, 0.0))
        }
    };
    Snapshot {
        time: now,
        load: load_average(),
        cpu_count: cpu_count(),
        cpu,
        cpu_process,
        cpu_identity: cpu_identity_cached(),
        arch: arch(),
        mem: platform::memory(),
        mem_process: platform::process_rss(),
        zswap: platform::zswap(),
        zram: platform::zram(),
        compressor: platform::compressor(),
        speed: download_speed(),
        connections: connections_in_use(),
    }
}

/// The history window for `range_sec` (clamped to 1 minute .. 24 hours).
pub fn history(range_sec: i64) -> (i64, Vec<Sample>) {
    start_sampler();
    let range = range_sec.clamp(60, COARSE_INTERVAL_SEC * COARSE_CAPACITY as i64);
    sampler()
        .lock()
        .map_or((FINE_INTERVAL_SEC, Vec::new()), |g| g.history.window(range, now_unix()))
}

pub(crate) fn json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn num(v: f64) -> String {
    if v.is_finite() {
        let r = (v * 100.0).round() / 100.0;
        format!("{r}")
    } else {
        "0".to_string()
    }
}

/// The `systemstats` reply as JSON.
pub fn to_json(snap: &Snapshot, interval: i64, samples: &[Sample]) -> String {
    let mut o = String::with_capacity(256 + samples.len() * 64);
    o.push_str(&format!("{{\"Time\":{},", snap.time));
    o.push_str(&format!(
        "\"Load\":[{},{},{}],",
        num(snap.load[0]),
        num(snap.load[1]),
        num(snap.load[2])
    ));
    o.push_str(&format!("\"CpuCount\":{},", snap.cpu_count));
    o.push_str(&format!("\"CpuPercent\":{},", num(f64::from(snap.cpu))));
    o.push_str(&format!(
        "\"ProcessCpuPercent\":{},",
        num(f64::from(snap.cpu_process))
    ));
    o.push_str("\"Cpu\":{\"Vendor\":");
    json_str(&mut o, &snap.cpu_identity.vendor);
    o.push_str(",\"Model\":");
    json_str(&mut o, &snap.cpu_identity.model);
    o.push_str(",\"Generation\":");
    json_str(&mut o, &snap.cpu_identity.generation);
    o.push_str(",\"Arch\":");
    json_str(&mut o, &snap.arch);
    o.push_str("},");
    let m = &snap.mem;
    o.push_str(&format!(
        "\"Memory\":{{\"Total\":{},\"Free\":{},\"Available\":{},\"Used\":{},\"Process\":{}}},",
        m.total,
        m.free,
        m.available,
        m.used(),
        snap.mem_process
    ));
    o.push_str(&format!(
        "\"Swap\":{{\"Total\":{},\"Free\":{}}},",
        m.swap_total, m.swap_free
    ));
    o.push_str(&format!(
        "\"Zswap\":{},\"Zram\":{},\"MemoryCompression\":",
        snap.zswap, snap.zram
    ));
    json_str(
        &mut o,
        memory_compression_label(snap.zswap, snap.zram, snap.compressor),
    );
    o.push_str(&format!(
        ",\"Speed\":{},\"Connections\":{},",
        snap.speed, snap.connections
    ));
    o.push_str(&format!("\"History\":{{\"Interval\":{interval},\"Fields\":["));
    for (i, f) in SAMPLE_FIELDS.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        json_str(&mut o, f);
    }
    o.push_str("],\"Samples\":[");
    for (i, s) in samples.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!(
            "[{},{},{},{},{},{},{},{},{},{}]",
            s.time,
            s.speed,
            s.connections,
            num(f64::from(s.load1)),
            num(f64::from(s.load5)),
            num(f64::from(s.load15)),
            num(f64::from(s.cpu)),
            num(f64::from(s.cpu_process)),
            s.mem_used,
            s.mem_process
        ));
    }
    o.push_str("]}}");
    o
}

/// XML-RPC clients get the same JSON document as one string value.
pub fn to_xml(json: &str) -> String {
    let mut o = String::with_capacity(json.len() + 32);
    o.push_str("<string>");
    for c in json.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            c => o.push(c),
        }
    }
    o.push_str("</string>");
    o
}

/// Handles the `systemstats` RPC: `range_sec` of history (default one hour).
pub fn rpc_reply(is_json: bool, range_sec: Option<i32>) -> Vec<u8> {
    let range = i64::from(range_sec.filter(|r| *r > 0).unwrap_or(3600));
    let snap = snapshot();
    let (interval, samples) = history(range);
    let json = to_json(&snap, interval, &samples);
    if is_json {
        json.into_bytes()
    } else {
        to_xml(&json).into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_cores_count_hyperthreads_once() {
        // i7-8700K shape: 6 cores, 2 threads each, listed in kernel order.
        let mut text = String::new();
        for cpu in 0..12 {
            text += &format!(
                "processor\t: {cpu}\nphysical id\t: 0\ncore id\t\t: {}\n\n",
                cpu % 6
            );
        }
        assert_eq!(parse_cpuinfo_physical_cores(&text), Some(6));
        // Two sockets reuse core ids.
        let two = "physical id : 0\ncore id : 0\n\nphysical id : 1\ncore id : 0\n";
        assert_eq!(parse_cpuinfo_physical_cores(two), Some(2));
        // No core ids (ARM): unknown.
        assert_eq!(parse_cpuinfo_physical_cores("processor : 0\nBogoMIPS : 50\n"), None);
        let n = physical_cpu_count();
        assert!(n >= 1 && n <= cpu_count());
    }

    #[test]
    fn proc_stat_counts_idle_and_iowait() {
        let t = "cpu  100 5 50 800 20 1 2 0 0 0\ncpu0 1 2 3 4\n";
        assert_eq!(parse_proc_stat(t), Some((978, 820)));
        assert_eq!(parse_proc_stat("intr 1 2"), None);
    }

    #[test]
    fn meminfo_reads_available_and_swap() {
        let t = "MemTotal:       16000000 kB\nMemFree:         2000000 kB\nMemAvailable:    9000000 kB\nBuffers: 1 kB\nCached: 2 kB\nSwapTotal:       4000000 kB\nSwapFree:        3000000 kB\n";
        let m = parse_meminfo(t);
        assert_eq!(m.total, 16_000_000 * 1024);
        assert_eq!(m.available, 9_000_000 * 1024);
        assert_eq!(m.used(), 7_000_000 * 1024);
        assert_eq!(m.swap_total, 4_000_000 * 1024);
        assert_eq!(m.swap_free, 3_000_000 * 1024);
    }

    #[test]
    fn meminfo_without_available_uses_free_buffers_cache() {
        let t = "MemTotal: 1000 kB\nMemFree: 100 kB\nBuffers: 50 kB\nCached: 200 kB\n";
        assert_eq!(parse_meminfo(t).available, 350 * 1024);
    }

    #[test]
    fn statm_rss_in_bytes() {
        assert_eq!(parse_statm_rss("5000 1200 300 1 0 900 0", 4096), 1200 * 4096);
        assert_eq!(parse_statm_rss("", 4096), 0);
    }

    #[test]
    fn zram_and_zswap_flags() {
        let swaps = "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n/dev/zram0  partition 4194300 0 100\n";
        assert!(proc_swaps_has_zram(swaps));
        assert!(!proc_swaps_has_zram("Filename Type\n/swapfile file 1 0 -2\n"));
        assert!(zswap_flag_enabled("Y\n"));
        assert!(!zswap_flag_enabled("N\n"));
        assert!(!zswap_flag_enabled(""));
        assert_eq!(memory_compression_label(true, true, false), "zswap + zram");
        assert_eq!(memory_compression_label(false, false, true), "macOS compressor");
        assert_eq!(memory_compression_label(false, false, false), "none");
    }

    #[test]
    fn cpuinfo_x86_and_arm() {
        let x86 = "processor\t: 0\nvendor_id\t: AuthenticAMD\ncpu family\t: 25\nmodel name\t: AMD Ryzen 9 5950X 16-Core Processor\n";
        let id = parse_cpuinfo(x86);
        assert_eq!(id.vendor, "AMD");
        assert_eq!(id.model, "AMD Ryzen 9 5950X 16-Core Processor");
        assert_eq!(id.generation, "Ryzen 5000 (Zen 3)");

        let arm = "processor\t: 0\nCPU implementer\t: 0x41\nCPU part\t: 0xd08\n\nModel\t\t: Raspberry Pi 4 Model B Rev 1.4\n";
        let id = parse_cpuinfo(arm);
        assert_eq!(id.vendor, "ARM");
        assert_eq!(id.model, "Raspberry Pi 4 Model B Rev 1.4");
    }

    #[test]
    fn generations_from_model_names() {
        let cases = [
            ("12th Gen Intel(R) Core(TM) i7-12700K", "12th gen"),
            ("Intel(R) Core(TM) i5-8250U CPU @ 1.60GHz", "8th gen"),
            ("Intel(R) Core(TM) i9-14900K", "14th gen"),
            ("Intel(R) Core(TM) i3-10100", "10th gen"),
            ("Intel(R) Core(TM) i7-11800H", "11th gen"),
            ("Intel(R) Core(TM) i3-2120", "2nd gen"),
            ("Intel(R) Core(TM) i5-3570K", "3rd gen"),
            ("Intel(R) Core(TM) Ultra 7 155H", "Core Ultra Series 1"),
            ("Intel(R) Core(TM) Ultra 9 285K", "Core Ultra Series 2"),
            ("Intel(R) Xeon(R) CPU E5-2680 v4 @ 2.40GHz", "v4"),
            ("AMD Ryzen 7 PRO 4750U with Radeon Graphics", "Ryzen 4000 (Zen 2)"),
            ("AMD Ryzen 9 7950X 16-Core Processor", "Ryzen 7000 (Zen 4)"),
            ("AMD Ryzen 5 9600X", "Ryzen 9000 (Zen 5)"),
            ("AMD Ryzen 5 7520U with Radeon Graphics", "Ryzen 7000 (Zen 2)"),
            ("AMD Ryzen 7 7840HS w/ Radeon 780M Graphics", "Ryzen 7000 (Zen 4)"),
            ("AMD Ryzen 7 7735HS with Radeon Graphics", "Ryzen 7000 (Zen 3)"),
            ("AMD Ryzen 7 5700U with Radeon Graphics", "Ryzen 5000"),
            ("AMD Ryzen 7 6800H with Radeon Graphics", "Ryzen 6000 (Zen 3+)"),
            ("AMD EPYC 7763 64-Core Processor", "EPYC Milan (Zen 3)"),
            ("AMD EPYC 9554 64-Core Processor", "EPYC Genoa (Zen 4)"),
            ("Apple M2 Pro", "M2"),
            ("Apple M4", "M4"),
            ("Intel(R) Celeron(R) N5105", ""),
            ("", ""),
        ];
        for (model, want) in cases {
            assert_eq!(cpu_generation(model), want, "{model}");
        }
        assert_eq!(vendor_from_model("Apple M1"), "Apple");
        assert_eq!(vendor_from_model("Intel(R) Xeon(R)"), "Intel");
    }

    #[test]
    fn cpu_meter_percentages() {
        let mut m = CpuMeter::new(4);
        let t0 = Instant::now();
        assert_eq!(m.feed(t0, Some((1000, 800)), 10.0), None);
        let t1 = t0 + Duration::from_secs(2);
        // 100 ticks pass, 25 idle -> 75% busy; 2 CPU seconds over 2s * 4 CPUs -> 25%.
        let (cpu, own) = m.feed(t1, Some((1100, 825)), 12.0).unwrap();
        assert!((cpu - 75.0).abs() < 0.01, "{cpu}");
        assert!((own - 25.0).abs() < 0.01, "{own}");
    }

    fn sample(time: i64, speed: i64) -> Sample {
        Sample {
            time,
            speed,
            connections: 10,
            cpu: 50.0,
            ..Sample::default()
        }
    }

    #[test]
    fn history_rings_and_minute_averages() {
        let mut h = History::default();
        let base = 1_700_000_040; // a whole minute
        for i in 0..(FINE_CAPACITY as i64 + 30) {
            h.push(sample(base + i * FINE_INTERVAL_SEC, i));
        }
        assert_eq!(h.fine.len(), FINE_CAPACITY);
        let last = base + (FINE_CAPACITY as i64 + 29) * FINE_INTERVAL_SEC;
        assert_eq!(h.latest().unwrap().time, last);

        // 12 samples per minute, every finished minute is averaged.
        let first = h.coarse.front().unwrap();
        assert_eq!(first.time, base);
        assert_eq!(first.speed, (0..12).sum::<i64>() / 12);
        assert_eq!(first.connections, 10);
        assert!((first.cpu - 50.0).abs() < 0.01);

        let (iv, v) = h.window(600, last);
        assert_eq!(iv, FINE_INTERVAL_SEC);
        assert_eq!(v.len(), 120);
        let (iv, v) = h.window(6 * 3600, last);
        assert_eq!(iv, COARSE_INTERVAL_SEC);
        assert!(!v.is_empty());
    }

    #[test]
    fn coarse_ring_is_bounded() {
        let mut h = History::default();
        for i in 0..(COARSE_CAPACITY as i64 + 10) {
            h.push(sample(i * COARSE_INTERVAL_SEC, 1));
        }
        assert_eq!(h.coarse.len(), COARSE_CAPACITY);
    }

    #[test]
    fn json_reply_is_valid_shape() {
        let snap = Snapshot {
            time: 5,
            load: [0.5, 1.25, 2.0],
            cpu_count: 8,
            cpu: 12.345,
            cpu_identity: CpuIdentity {
                vendor: "AMD".into(),
                model: "AMD \"Ryzen\"".into(),
                generation: "Ryzen 5000 (Zen 3)".into(),
            },
            arch: "x86_64".into(),
            mem: MemInfo {
                total: 100,
                free: 10,
                available: 40,
                swap_total: 8,
                swap_free: 6,
            },
            mem_process: 7,
            zram: true,
            ..Snapshot::default()
        };
        let j = to_json(&snap, 5, &[sample(1, 2)]);
        assert!(j.starts_with("{\"Time\":5,\"Load\":[0.5,1.25,2],\"CpuCount\":8,\"CpuPercent\":12.35,"));
        assert!(j.contains("\"Model\":\"AMD \\\"Ryzen\\\"\""));
        assert!(j.contains("\"Memory\":{\"Total\":100,\"Free\":10,\"Available\":40,\"Used\":60,\"Process\":7}"));
        assert!(j.contains("\"Zswap\":false,\"Zram\":true,\"MemoryCompression\":\"zram\""));
        assert!(j.ends_with("\"Samples\":[[1,2,10,0,0,0,50,0,0,0]]}}"));
        assert_eq!(to_xml("a<b&c"), "<string>a&lt;b&amp;c</string>");
    }

    #[test]
    fn live_readings_are_sane() {
        assert!(cpu_count() >= 1);
        let l = load_average();
        assert!(l.iter().all(|v| *v >= 0.0));
        assert!(process_cpu_seconds() >= 0.0);
        let m = platform::memory();
        assert!(m.available <= m.total || m.total == 0);
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            assert!(m.total > 0);
            assert!(platform::process_rss() > 0);
            assert!(platform::machine_cpu_ticks().is_some());
        }
    }
}
