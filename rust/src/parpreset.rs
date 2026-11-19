//! `ParPreset`: one-click resource levels for the Check and repair settings.
//!
//! A preset sets the par options that decide how much CPU, RAM and disk
//! bandwidth par-check and par-repair may take: `ParBuffer`, `ParThreads` and
//! `ParPauseQueue`. "custom" keeps those options as written. "detect" looks at
//! the machine (logical CPUs, available RAM, the intermediate folder's disk)
//! and picks values for a downloader that should be a heavier app on the PC
//! without taking everything the way "uber" does.
//!
//! The presets never touch `LiveParRepair` (live par2 syndromes during the
//! download): measurements showed it costs RAM and disk reads without making
//! the final repair faster, so it is its own switch.

use crate::options::OptStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Option names in the order of [`PRESET_VALUES`].
pub const PRESET_NAMES: [&std::ffi::CStr; 6] =
    [c"low", c"medium", c"large", c"uber", c"detect", c"custom"];
pub const PRESET_VALUES: [i32; 6] = [0, 1, 2, 3, 4, 5];

pub const LOW: i32 = 0;
pub const MEDIUM: i32 = 1;
pub const LARGE: i32 = 2;
pub const UBER: i32 = 3;
pub const DETECT: i32 = 4;
pub const CUSTOM: i32 = 5;

/// Largest ParBuffer a preset sets (the Settings slider's top). A 32-bit
/// build can't address that much: ArticleCache plus ParBuffer must stay
/// within 1900 MB there (see `options::clamp_32bit_memory_limits`).
#[cfg(target_pointer_width = "64")]
pub const MAX_PAR_BUFFER_MB: i32 = 4096;
#[cfg(not(target_pointer_width = "64"))]
pub const MAX_PAR_BUFFER_MB: i32 = 1024;

/// What a preset sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParResources {
    /// `ParBuffer` in MB.
    pub par_buffer_mb: i32,
    /// `ParThreads`; 0 = all logical CPUs.
    pub par_threads: i32,
    /// `ParPauseQueue`.
    pub par_pause_queue: bool,
}

/// What "detect" looked at.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Probe {
    pub cpus: usize,
    pub mem_total: u64,
    pub mem_available: u64,
    /// Free bytes in the intermediate folder, when known.
    pub inter_free: Option<u64>,
    /// Sequential write speed of the intermediate folder (MB/s, synced to disk).
    pub disk_write_mbps: Option<f64>,
    /// Sequential read speed of the intermediate folder (MB/s, not from cache where the OS allows).
    pub disk_read_mbps: Option<f64>,
}

const MB: u64 = 1024 * 1024;

/// Below this write speed the download and a repair fight over the disk, so
/// "detect" pauses the queue while repairing (a single hard disk, a NAS).
const SLOW_DISK_MBPS: f64 = 150.0;

/// The values a preset sets on a machine with `cpus` logical CPUs; `None`
/// for "custom". "detect" uses `probe`, or "large" until a probe exists.
pub fn resources(preset: i32, cpus: usize, probe: Option<&Probe>) -> Option<ParResources> {
    let cpus = cpus.max(1);
    match preset {
        // least CPU and RAM: one thread, a small buffer, downloads pause while repairing
        LOW => Some(ParResources { par_buffer_mb: 64, par_threads: 1, par_pause_queue: true }),
        // a trade-off: half the CPUs
        MEDIUM => Some(ParResources {
            par_buffer_mb: 256,
            par_threads: (cpus / 2).max(1) as i32,
            par_pause_queue: false,
        }),
        // aggressive: every CPU and 1 GB
        LARGE => Some(ParResources { par_buffer_mb: 1024, par_threads: 0, par_pause_queue: false }),
        // everything maxed out: the largest buffer and two threads more than CPUs
        // to cover waits for I/O
        UBER => Some(ParResources {
            par_buffer_mb: MAX_PAR_BUFFER_MB,
            par_threads: (cpus + 2) as i32,
            par_pause_queue: false,
        }),
        DETECT => Some(match probe {
            Some(p) => detected(p),
            None => resources(LARGE, cpus, None).unwrap(),
        }),
        _ => None,
    }
}

/// "detect": a heavier app, not one that takes everything.
/// - threads: every logical CPU, except one left free on 2 to 4 CPU machines;
/// - buffer: 15% of the RAM available now, in 64 MB steps, 128 MB to 4 GB;
/// - pause the queue while repairing when the intermediate disk writes
///   slower than 150 MB/s.
pub fn detected(p: &Probe) -> ParResources {
    let cpus = p.cpus.max(1);
    let par_threads = match cpus {
        1 => 1,
        2..=4 => (cpus - 1) as i32,
        _ => 0,
    };
    let share = p.mem_available / 100 * 15 / MB;
    let par_buffer_mb = (share / 64 * 64).clamp(128, MAX_PAR_BUFFER_MB as u64) as i32;
    let par_pause_queue = p.disk_write_mbps.is_some_and(|w| w < SLOW_DISK_MBPS);
    ParResources { par_buffer_mb, par_threads, par_pause_queue }
}

/// `ParPauseQueue` as post-processing should apply it: with "detect" the
/// disk test, once finished, decides; otherwise the parsed value.
pub fn pause_queue(parsed: bool) -> bool {
    if crate::options::par_preset() != DETECT {
        return parsed;
    }
    last_probe().map_or(parsed, |p| detected(&p).par_pause_queue)
}

// ---------------------------------------------------------------------------
// The probe
// ---------------------------------------------------------------------------

struct ProbeState {
    probe: Option<Probe>,
    running: bool,
    at: Option<Instant>,
    /// When the running probe started.
    started: Option<Instant>,
}

static STATE: Mutex<ProbeState> =
    Mutex::new(ProbeState { probe: None, running: false, at: None, started: None });

/// A probe still running after this long (a disk that hangs in sync) is
/// given up on: it no longer blocks a new probe or keeps Settings polling.
const PROBE_GIVE_UP: Duration = Duration::from_secs(300);

impl ProbeState {
    fn running_now(&self) -> bool {
        self.running && self.started.is_some_and(|t| t.elapsed() < PROBE_GIVE_UP)
    }
}

/// The last probe, if one has finished.
pub fn last_probe() -> Option<Probe> {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).probe
}

/// CPU count and memory, read now (no disk test).
pub fn quick_probe() -> Probe {
    let mem = crate::sysmetrics::memory_info();
    Probe {
        cpus: crate::sysmetrics::cpu_count(),
        mem_total: mem.total,
        mem_available: mem.available,
        ..Probe::default()
    }
}

/// Starts a probe of the machine and `inter_dir` on a background thread
/// unless one is running or finished less than `min_age` ago. The disk test
/// writes and reads back up to 512 MB (about 2-5 s on a fast disk) and is
/// skipped when the folder has less than 2 GB free.
pub fn start_probe(inter_dir: Vec<u8>, min_age: Duration) {
    {
        let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if s.running_now() || s.at.is_some_and(|at| at.elapsed() < min_age) {
            return;
        }
        s.running = true;
        s.started = Some(Instant::now());
    }
    let spawned = std::thread::Builder::new()
        .name("parpreset-probe".into())
        .spawn(move || {
            let probe = full_probe(&inter_dir);
            let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
            s.probe = Some(probe);
            s.running = false;
            s.at = Some(Instant::now());
            drop(s);
            let r = detected(&probe);
            // ParBuffer and ParThreads were set when the options were read;
            // a newer RAM figure only takes effect on the next reload
            let applied = crate::options::active_options().map(|o| o.scalars().par_buffer);
            let later = if crate::options::par_preset() == DETECT
                && applied.is_some_and(|b| b != r.par_buffer_mb)
            {
                " (ParBuffer applies after the next reload)"
            } else {
                ""
            };
            let msg = format!(
                "Par preset detect: {} CPUs, {} MB RAM available of {}, intermediate disk {}; ParBuffer {} MB, ParThreads {}, ParPauseQueue {}{later}",
                probe.cpus,
                probe.mem_available / MB,
                probe.mem_total / MB,
                describe_disk(&probe),
                r.par_buffer_mb,
                if r.par_threads == 0 { "all".to_string() } else { r.par_threads.to_string() },
                if r.par_pause_queue { "yes" } else { "no" },
            );
            crate::log::log_active_bytes(crate::queuesubsys_host::MK_INFO, msg.as_bytes());
        });
    if spawned.is_err() {
        STATE.lock().unwrap_or_else(|e| e.into_inner()).running = false;
    }
}

fn describe_disk(p: &Probe) -> String {
    match (p.disk_write_mbps, p.disk_read_mbps) {
        (Some(w), Some(r)) => format!("write {w:.0} MB/s, read {r:.0} MB/s"),
        (Some(w), None) => format!("write {w:.0} MB/s"),
        _ => "not tested".to_string(),
    }
}

fn full_probe(inter_dir: &[u8]) -> Probe {
    let mut p = quick_probe();
    p.inter_free = crate::fsops::statvfs_available_bytes(inter_dir).ok();
    if p.inter_free.is_some_and(|free| free >= 2048 * MB) {
        if let Some((w, r)) = disk_test(inter_dir, 512 * MB, Duration::from_secs(5)) {
            p.disk_write_mbps = Some(w);
            p.disk_read_mbps = r;
        }
    }
    p
}

/// Writes up to `max` bytes (synced to disk) then reads them back, past the
/// page cache where the OS allows it. Returns MB/s for write and read.
fn disk_test(dir: &[u8], max: u64, limit: Duration) -> Option<(f64, Option<f64>)> {
    use std::io::{Read, Write};
    #[cfg(unix)]
    let dir = {
        use std::os::unix::ffi::OsStrExt;
        std::path::Path::new(std::ffi::OsStr::from_bytes(dir)).to_path_buf()
    };
    #[cfg(not(unix))]
    let dir = std::path::PathBuf::from(String::from_utf8_lossy(dir).into_owned());
    // a unique name per test, so a given-up probe that wakes later can't
    // touch a newer probe's file
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!(".rustyget-parpreset-{}-{seq}.tmp", std::process::id()));
    struct Remove(std::path::PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _remove = Remove(path.clone());

    // pseudo-random bytes, so a compressing or deduplicating filesystem
    // (ZFS, btrfs) still has to write them all
    let mut block = vec![0u8; 4 * MB as usize];
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15 ^ u64::from(std::process::id());
    let mut file = std::fs::File::create(&path).ok()?;
    // macOS: keep the written pages out of the cache too, or the read test
    // would measure memory (Linux drops them after the sync instead)
    #[cfg(target_os = "macos")]
    uncache(&file);
    let start = Instant::now();
    let mut written = 0u64;
    while written < max && start.elapsed() < limit {
        // new bytes for every block, so a deduplicating filesystem can't
        // store the test once
        for chunk in block.chunks_exact_mut(8) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            chunk.copy_from_slice(&x.to_le_bytes());
        }
        file.write_all(&block).ok()?;
        written += block.len() as u64;
    }
    file.sync_data().ok()?;
    let write_secs = start.elapsed().as_secs_f64();
    drop(file);
    let write = (written as f64 / MB as f64) / write_secs.max(1e-6);

    let mut file = std::fs::File::open(&path).ok()?;
    if !uncache(&file) {
        return Some((write, None));
    }
    let mut buf = vec![0u8; block.len()];
    let start = Instant::now();
    let mut read = 0u64;
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => read += n as u64,
            Err(_) => return Some((write, None)),
        }
    }
    let read_secs = start.elapsed().as_secs_f64();
    Some((write, Some((read as f64 / MB as f64) / read_secs.max(1e-6))))
}

/// Drops the file's pages from the cache (Linux) or turns caching off for
/// this descriptor (macOS), so the read test measures the disk.
#[cfg(target_os = "linux")]
fn uncache(file: &std::fs::File) -> bool {
    use std::os::unix::io::AsRawFd;
    unsafe { libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) == 0 }
}

#[cfg(target_os = "macos")]
fn uncache(file: &std::fs::File) -> bool {
    use std::os::unix::io::AsRawFd;
    unsafe { libc::fcntl(file.as_raw_fd(), libc::F_NOCACHE, 1) != -1 }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn uncache(_file: &std::fs::File) -> bool {
    false
}

// ---------------------------------------------------------------------------
// The `parpresetdetect` RPC
// ---------------------------------------------------------------------------

/// JSON for Settings: what was detected and what each preset sets here.
pub fn to_json(probe: Option<&Probe>, running: bool, cpus: usize) -> String {
    let mut o = String::from("{");
    o.push_str(&format!("\"Running\":{running},\"CPUs\":{cpus}"));
    // the ParBuffer in use now (a new RAM reading applies on the next reload)
    if let Some(opts) = crate::options::active_options() {
        o.push_str(&format!(",\"AppliedParBuffer\":{}", opts.scalars().par_buffer));
    }
    if let Some(p) = probe {
        o.push_str(&format!(
            ",\"MemTotalMB\":{},\"MemAvailableMB\":{}",
            p.mem_total / MB,
            p.mem_available / MB
        ));
        if let Some(free) = p.inter_free {
            o.push_str(&format!(",\"InterFreeMB\":{}", free / MB));
        }
        if let Some(w) = p.disk_write_mbps {
            o.push_str(&format!(",\"DiskWriteMBps\":{w:.0}"));
        }
        if let Some(r) = p.disk_read_mbps {
            o.push_str(&format!(",\"DiskReadMBps\":{r:.0}"));
        }
    }
    o.push_str(",\"Presets\":{");
    for (i, (name, value)) in PRESET_NAMES.iter().zip(PRESET_VALUES).enumerate() {
        let Some(r) = resources(value, cpus, probe) else { continue };
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!(
            "\"{}\":{{\"ParBuffer\":{},\"ParThreads\":{},\"ParPauseQueue\":{}}}",
            name.to_str().unwrap_or_default(),
            r.par_buffer_mb,
            r.par_threads,
            r.par_pause_queue
        ));
    }
    o.push_str("}}");
    o
}

/// Handles `parpresetdetect(start)`: `start` true runs a new probe of the
/// intermediate folder (at most one every 30 s).
/// The folder whose disk "detect" tests: InterDir, or DestDir when InterDir
/// is empty (downloads then go straight to DestDir).
pub fn probe_dir(o: &crate::options::NzbgetRsOptions) -> Vec<u8> {
    let inter = o.str_bytes(OptStr::InterDir);
    if inter.is_empty() {
        o.str_bytes(OptStr::DestDir).to_vec()
    } else {
        inter.to_vec()
    }
}

pub fn rpc_reply(is_json: bool, start: bool) -> Vec<u8> {
    if start {
        let inter = crate::options::active_options()
            .filter(|o| !o.no_disk_access())
            .map(|o| probe_dir(&o))
            .unwrap_or_default();
        if !inter.is_empty() {
            start_probe(inter, Duration::from_secs(30));
        }
    }
    let (probe, running) = {
        let s = STATE.lock().unwrap_or_else(|e| e.into_inner());
        (s.probe, s.running_now())
    };
    // before a disk test finishes, size "detect" from RAM now, as the parser does
    let probe = probe.or_else(|| Some(quick_probe()));
    let json = to_json(probe.as_ref(), running, crate::sysmetrics::cpu_count());
    if is_json {
        json.into_bytes()
    } else {
        crate::sysmetrics::to_xml(&json).into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(cpus: usize, avail_gb: u64, write: Option<f64>) -> Probe {
        Probe {
            cpus,
            mem_total: 64 * 1024 * MB,
            mem_available: avail_gb * 1024 * MB,
            inter_free: Some(100 * 1024 * MB),
            disk_write_mbps: write,
            disk_read_mbps: write,
        }
    }

    #[test]
    fn fixed_presets_scale_from_low_to_uber() {
        let low = resources(LOW, 12, None).unwrap();
        let medium = resources(MEDIUM, 12, None).unwrap();
        let large = resources(LARGE, 12, None).unwrap();
        let uber = resources(UBER, 12, None).unwrap();
        assert_eq!(low, ParResources { par_buffer_mb: 64, par_threads: 1, par_pause_queue: true });
        assert_eq!(medium, ParResources { par_buffer_mb: 256, par_threads: 6, par_pause_queue: false });
        assert_eq!(large, ParResources { par_buffer_mb: 1024, par_threads: 0, par_pause_queue: false });
        assert_eq!(uber, ParResources { par_buffer_mb: MAX_PAR_BUFFER_MB, par_threads: 14, par_pause_queue: false });
        assert!(low.par_buffer_mb < medium.par_buffer_mb && medium.par_buffer_mb < large.par_buffer_mb);
        assert!(large.par_buffer_mb < uber.par_buffer_mb);
        // a single CPU still gets one thread on medium
        assert_eq!(resources(MEDIUM, 1, None).unwrap().par_threads, 1);
        assert_eq!(resources(CUSTOM, 12, None), None);
    }

    #[test]
    fn detect_waits_for_a_probe_with_large() {
        assert_eq!(resources(DETECT, 8, None), resources(LARGE, 8, None));
    }

    #[test]
    fn detect_takes_a_share_of_available_ram_and_cpus() {
        // the prod server: 12 CPUs, ~18 GB available, NVMe at 1 GB/s
        let r = detected(&probe(12, 18, Some(1000.0)));
        assert_eq!(r.par_threads, 0);
        assert_eq!(r.par_buffer_mb, 2752); // 15% of 18 GB in 64 MB steps
        assert!(!r.par_pause_queue);
        // a small NAS: 4 CPUs, 1 GB free, a single hard disk at 120 MB/s
        let r = detected(&probe(4, 1, Some(120.0)));
        assert_eq!(r.par_threads, 3);
        assert_eq!(r.par_buffer_mb, 128);
        assert!(r.par_pause_queue);
        // lots of RAM caps at 4 GB; an untested disk never pauses
        let r = detected(&probe(32, 64, None));
        assert_eq!(r.par_buffer_mb, MAX_PAR_BUFFER_MB);
        assert!(!r.par_pause_queue);
    }

    #[test]
    fn json_lists_every_preset_but_custom() {
        let p = probe(12, 18, Some(1000.0));
        let json = to_json(Some(&p), false, 12);
        for name in ["low", "medium", "large", "uber", "detect"] {
            assert!(json.contains(&format!("\"{name}\":{{")), "{json}");
        }
        assert!(!json.contains("\"custom\""));
        assert!(json.contains("\"DiskWriteMBps\":1000"));
    }

    #[test]
    fn disk_test_measures_a_temp_folder() {
        let dir = std::env::temp_dir().join(format!("rg-parpreset-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        #[cfg(unix)]
        let bytes = {
            use std::os::unix::ffi::OsStrExt;
            dir.as_os_str().as_bytes().to_vec()
        };
        #[cfg(not(unix))]
        let bytes = dir.to_string_lossy().as_bytes().to_vec();
        let (w, _r) = disk_test(&bytes, 8 * MB, Duration::from_secs(2)).unwrap();
        assert!(w > 0.0);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "test file removed");
        let _ = std::fs::remove_dir(&dir);
    }
}
