//! Rust parts of nzbget, called from C++ through a C ABI (see nzbget_rs.h).
//! Each module replaces one C++ routine and must match its output byte for byte.

pub mod binrpc;
pub mod bytes;
pub mod cmdline;
pub mod collection;
pub mod crc;
pub mod decode;
pub mod decoder;
pub mod deobfuscation;
pub mod directrenamer;
pub mod donorscore;
pub mod dupesearch_nzb;
pub mod escape;
pub mod extload;
pub mod feedfilter;
pub mod ffi;
pub mod filetypes;
pub mod fsdelete;
#[cfg(unix)]
pub mod fsops;
pub mod json;
pub mod manifest;
pub mod nntp;
pub mod nzbfile;
pub mod options;
pub mod parparser;
pub mod parrenamer;
pub mod paths;
pub mod posting;
pub mod rarreader;
pub mod rarrenamer;
pub mod releasename;
pub mod renameanalyzer;
pub mod rpcparams;
pub mod rpcroute;
pub mod scheduler;
pub mod statmeter;
pub mod text;
pub mod tlscipher;
pub mod twin;
pub mod url;
pub mod urlcoord;
pub mod util;
pub mod webdownload;
pub mod webserver;
pub mod webutil;
pub mod wildmask;

pub mod dupeidentity;
pub mod dupefallback;
pub mod streamrepair;
pub mod postrepair;
pub mod queuehotpath;
pub mod diskstatepp;
pub mod dupefeed;
pub mod scripturlstat;
pub mod rpcconnopt;
pub mod frontexthealth;
pub mod sysnservhealth;
pub mod extutilhealth;
pub mod dlunpackrepair;
pub mod artwritercore;
pub mod connclientmain;
pub mod nntptlsweb;
pub mod feedurlqueue;
pub mod parunpackpost;
pub mod contentdupenntp;
pub mod diskartdlpost;
pub mod queuecoordinfo;
pub mod pardupenzb;
pub mod histscanrepair;
pub mod streamtwinqueue;
pub mod unpackcleanurlfeed;
pub mod renamerarrepdl;
pub mod poolstatprepost;
pub mod scriptfeedcfg;
pub mod schedfrontweb;
pub mod rpcoptlogutil;
pub mod utilfsscript;
pub mod dlparstatedisk;
pub mod coorddlwriterpool;
pub mod dupecoordfallbackstream;
pub mod twinproberenhist;
pub mod urlqueuescanfeedpost;
pub mod rpcoptclient;
pub mod streamarticleparrepair;
pub mod dupedlstateconn;

pub mod deadpostings;
pub mod dupewatch;
pub mod streamcompare;
pub mod streampatchparts;

pub mod dupepurge;

#[cfg(test)]
mod allocation_probe;
pub mod purgejournal;

pub mod memory_pressure {
    //! Low-Memory & OOM Resilience Monitor (Requirement R8).
    //! Zero-allocation metrics sampling and 3-tier memory pressure evaluation.

    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
    use std::sync::Mutex;

    #[repr(C)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PressureTier {
        Normal = 0,
        Constrained = 1,
        Critical = 2,
    }

    impl PressureTier {
        pub fn from_i32(val: i32) -> Self {
            match val {
                1 => PressureTier::Constrained,
                2 => PressureTier::Critical,
                _ => PressureTier::Normal,
            }
        }
    }

    static OVERRIDE_TIER: AtomicI32 = AtomicI32::new(-1);
    static GLOBAL_MOCK_ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);

    static LAST_TIER: AtomicI32 = AtomicI32::new(PressureTier::Normal as i32);
    static LAST_HEADROOM_MB: AtomicU32 = AtomicU32::new(u32::MAX);

    #[derive(Debug, Default, Clone, PartialEq)]
    pub struct MemoryMetrics {
        pub psi_some_avg10: Option<f64>,
        pub psi_full_avg10: Option<f64>,
        pub cgroup_utilization_pct: Option<f64>,
        pub cgroup_headroom_mb: Option<u32>,
        pub host_available_pct: Option<f64>,
        pub host_available_mb: Option<u32>,
        pub darwin_pressure_level: Option<i32>,
    }

    pub fn set_mock_root(path: Option<&str>) {
        let mut guard = GLOBAL_MOCK_ROOT.lock().unwrap();
        *guard = path.filter(|s| !s.is_empty()).map(PathBuf::from);
    }

    pub fn set_override_tier(tier: i32) {
        OVERRIDE_TIER.store(tier, Ordering::Release);
    }

    pub fn get_override_tier() -> i32 {
        OVERRIDE_TIER.load(Ordering::Acquire)
    }

    pub fn get_last_tier() -> PressureTier {
        PressureTier::from_i32(LAST_TIER.load(Ordering::Acquire))
    }

    pub fn get_last_headroom_mb() -> Option<u32> {
        let val = LAST_HEADROOM_MB.load(Ordering::Acquire);
        if val == u32::MAX {
            None
        } else {
            Some(val)
        }
    }

    pub fn evaluate_state(m: &MemoryMetrics) -> PressureTier {
        let override_val = OVERRIDE_TIER.load(Ordering::Acquire);
        if override_val >= 0 {
            return PressureTier::from_i32(override_val);
        }

        // Critical tier criteria (Tier 2)
        if let (Some(some), full) = (m.psi_some_avg10, m.psi_full_avg10) {
            if some >= 30.0 || full.unwrap_or(0.0) > 5.0 {
                return PressureTier::Critical;
            }
        }
        if let Some(cgroup_pct) = m.cgroup_utilization_pct {
            if cgroup_pct >= 90.0 {
                return PressureTier::Critical;
            }
        }
        if let Some(headroom) = m.cgroup_headroom_mb {
            if headroom < 16 && m.cgroup_utilization_pct.unwrap_or(0.0) > 0.0 {
                return PressureTier::Critical;
            }
        }
        if let Some(avail_pct) = m.host_available_pct {
            if avail_pct < 10.0 {
                return PressureTier::Critical;
            }
        }
        if let Some(level) = m.darwin_pressure_level {
            if level >= 4 {
                return PressureTier::Critical;
            }
        }

        // Constrained tier criteria (Tier 1)
        if let Some(some) = m.psi_some_avg10 {
            if some >= 10.0 {
                return PressureTier::Constrained;
            }
        }
        if let Some(cgroup_pct) = m.cgroup_utilization_pct {
            if cgroup_pct >= 75.0 {
                return PressureTier::Constrained;
            }
        }
        if let Some(avail_pct) = m.host_available_pct {
            if avail_pct < 25.0 {
                return PressureTier::Constrained;
            }
        }
        if let Some(level) = m.darwin_pressure_level {
            if level == 2 {
                return PressureTier::Constrained;
            }
        }

        PressureTier::Normal
    }

    pub fn calculate_effective_cache_limit(
        configured_mb: u32,
        tier: PressureTier,
        headroom_mb: Option<u32>,
    ) -> u32 {
        match tier {
            PressureTier::Normal => configured_mb,
            PressureTier::Constrained => {
                let shrunk = configured_mb / 2;
                if let Some(headroom) = headroom_mb {
                    let headroom_cap = (headroom as f64 * 0.6) as u32;
                    shrunk.min(headroom_cap)
                } else {
                    shrunk
                }
            }
            PressureTier::Critical => 0, // Emergency bypass to disk
        }
    }

    pub fn calculate_effective_par_buffer(
        configured_mb: u32,
        tier: PressureTier,
        headroom_mb: Option<u32>,
    ) -> u32 {
        const MIN_PAR_BUFFER_MB: u32 = 16;
        if configured_mb == 0 {
            return 0;
        }
        match tier {
            PressureTier::Normal => configured_mb,
            PressureTier::Constrained => {
                let shrunk = (configured_mb / 2).max(MIN_PAR_BUFFER_MB);
                if let Some(headroom) = headroom_mb {
                    let headroom_cap = ((headroom as f64 * 0.5) as u32).max(MIN_PAR_BUFFER_MB);
                    shrunk.min(headroom_cap)
                } else {
                    shrunk
                }
            }
            PressureTier::Critical => {
                // libpar2 rejects -m0; enforce safe 16 MB floor (or configured if smaller than 16)
                configured_mb.min(MIN_PAR_BUFFER_MB).max(8)
            }
        }
    }

    pub fn calculate_effective_downloads_limit(configured_limit: u32, tier: PressureTier) -> u32 {
        match tier {
            PressureTier::Normal => configured_limit,
            PressureTier::Constrained => (configured_limit / 2).max(2),
            PressureTier::Critical => (configured_limit / 4).max(1),
        }
    }

    pub fn sample_system_metrics(mock_root: Option<&Path>) -> MemoryMetrics {
        let mut metrics = MemoryMetrics::default();

        let root_path_buf = if mock_root.is_some() {
            None
        } else {
            let guard = GLOBAL_MOCK_ROOT.lock().unwrap();
            guard.clone()
        };

        let effective_mock_root = mock_root.or(root_path_buf.as_deref());

        let env_mock_root = if effective_mock_root.is_none() {
            std::env::var("NZBGET_MEMORY_PRESSURE_MOCK_ROOT")
                .ok()
                .or_else(|| std::env::var("NZBGET_PRESSURE_ROOT").ok())
                .map(PathBuf::from)
        } else {
            None
        };

        let final_mock_root = effective_mock_root.or(env_mock_root.as_deref());

        // 1. Linux PSI (/proc/pressure/memory)
        let psi_path = if let Ok(custom_path) = std::env::var("NZBGET_PSI_PATH") {
            if !custom_path.is_empty() {
                PathBuf::from(custom_path)
            } else {
                resolve_subpath(final_mock_root, "proc/pressure/memory")
            }
        } else {
            resolve_subpath(final_mock_root, "proc/pressure/memory")
        };

        if let Ok(content) = std::fs::read_to_string(&psi_path) {
            parse_psi(&content, &mut metrics);
        }

        // 2. Linux Cgroups v2 (hierarchy-aware via /proc/self/cgroup & /proc/self/mountinfo)
        sample_cgroup_v2_hierarchy(final_mock_root, &mut metrics);

        // 3. Linux Cgroups v1 (hierarchy-aware with inactive_file subtraction)
        sample_cgroup_v1_hierarchy(final_mock_root, &mut metrics);

        // 4. Host meminfo (/proc/meminfo)
        let meminfo_path = resolve_subpath(final_mock_root, "proc/meminfo");
        if let Ok(content) = std::fs::read_to_string(&meminfo_path) {
            parse_meminfo(&content, &mut metrics);
        }

        // 5. Darwin Fallback (when not mocking Linux and running on macOS)
        #[cfg(target_os = "macos")]
        if final_mock_root.is_none() && psi_path.as_os_str() == "/proc/pressure/memory" {
            sample_darwin(&mut metrics);
        }

        // Update atomic cache
        let tier = evaluate_state(&metrics);
        LAST_TIER.store(tier as i32, Ordering::Release);
        let headroom = metrics
            .cgroup_headroom_mb
            .or(metrics.host_available_mb)
            .unwrap_or(u32::MAX);
        LAST_HEADROOM_MB.store(headroom, Ordering::Release);

        metrics
    }

    fn resolve_subpath(root: Option<&Path>, subpath: &str) -> PathBuf {
        let trimmed = subpath.trim_start_matches('/');
        if let Some(r) = root {
            r.join(trimmed)
        } else {
            PathBuf::from("/").join(trimmed)
        }
    }

    fn unescape_mountinfo_path(raw: &str) -> String {
        raw.replace("\\040", " ")
            .replace("\\011", "\t")
            .replace("\\012", "\n")
            .replace("\\134", "\\")
    }

    fn parse_proc_self_cgroup_paths(content: &str) -> (Option<String>, Option<String>) {
        let mut v2_path: Option<String> = None;
        let mut v1_mem_path: Option<String> = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut parts = trimmed.splitn(3, ':');
            let (Some(hier_id), Some(controllers), Some(cg_path)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            if hier_id == "0" && controllers.is_empty() {
                v2_path = Some(cg_path.to_string());
            } else if controllers.split(',').any(|c| c == "memory") {
                v1_mem_path = Some(cg_path.to_string());
            }
        }
        (v2_path, v1_mem_path)
    }

    fn parse_mountinfo_cgroup_mounts(
        content: &str,
    ) -> (Option<(String, String)>, Option<(String, String)>) {
        let mut v2_mount: Option<(String, String)> = None;
        let mut v1_mem_mount: Option<(String, String)> = None;

        for line in content.lines() {
            let Some((pre, post)) = line.split_once(" - ") else {
                continue;
            };
            let pre_fields: Vec<&str> = pre.split_whitespace().collect();
            let post_fields: Vec<&str> = post.split_whitespace().collect();
            if pre_fields.len() < 5 || post_fields.is_empty() {
                continue;
            }
            let mount_root = unescape_mountinfo_path(pre_fields[3]);
            let mount_point = unescape_mountinfo_path(pre_fields[4]);
            let fs_type = post_fields[0];
            let super_opts = post_fields.get(2).copied().unwrap_or("");

            if fs_type == "cgroup2" && v2_mount.is_none() {
                v2_mount = Some((mount_root, mount_point));
            } else if fs_type == "cgroup"
                && (super_opts.split(',').any(|o| o == "memory")
                    || mount_point.ends_with("/memory"))
                && v1_mem_mount.is_none()
            {
                v1_mem_mount = Some((mount_root, mount_point));
            }
        }

        (v2_mount, v1_mem_mount)
    }

    fn build_cgroup_hierarchy_dirs(
        mock_root: Option<&Path>,
        default_mount_subpath: &str,
        mount_info: Option<(String, String)>,
        proc_cgroup_path: Option<&str>,
    ) -> Vec<PathBuf> {
        let (mount_root_in_cg, mount_point_subpath) = match mount_info {
            Some((m_root, m_point)) => (m_root, m_point),
            None => ("/".to_string(), default_mount_subpath.to_string()),
        };

        let mount_base = resolve_subpath(mock_root, &mount_point_subpath);
        let fallback_base = resolve_subpath(mock_root, default_mount_subpath);
        let base_dir = if mount_base.exists() || !fallback_base.exists() {
            mount_base
        } else {
            fallback_base
        };

        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut push_hierarchy = |rel_str: &str| {
            let clean_rel = rel_str.trim_matches('/');
            let mut cur = if clean_rel.is_empty() {
                base_dir.clone()
            } else {
                base_dir.join(clean_rel)
            };
            loop {
                if !dirs.contains(&cur) {
                    dirs.push(cur.clone());
                }
                if cur == base_dir {
                    break;
                }
                match cur.parent() {
                    Some(parent) if parent.starts_with(&base_dir) => cur = parent.to_path_buf(),
                    _ => break,
                }
            }
        };

        if let Some(cg_path) = proc_cgroup_path {
            let norm_cg = format!("/{}", cg_path.trim_start_matches('/'));
            let norm_mount_root = format!("/{}", mount_root_in_cg.trim_matches('/'));
            if norm_mount_root != "/" {
                if let Some(stripped) = norm_cg.strip_prefix(&norm_mount_root) {
                    push_hierarchy(stripped);
                }
            }
            push_hierarchy(&norm_cg);
        }

        if !dirs.contains(&base_dir) {
            dirs.push(base_dir);
        }

        dirs
    }

    fn sample_cgroup_v2_hierarchy(mock_root: Option<&Path>, m: &mut MemoryMetrics) {
        let proc_cg = std::fs::read_to_string(resolve_subpath(mock_root, "proc/self/cgroup")).ok();
        let mountinfo =
            std::fs::read_to_string(resolve_subpath(mock_root, "proc/self/mountinfo")).ok();

        let (v2_cg_path, _) = proc_cg
            .as_deref()
            .map(parse_proc_self_cgroup_paths)
            .unwrap_or((None, None));
        let (v2_mount, _) = mountinfo
            .as_deref()
            .map(parse_mountinfo_cgroup_mounts)
            .unwrap_or((None, None));

        let dirs = build_cgroup_hierarchy_dirs(
            mock_root,
            "sys/fs/cgroup",
            v2_mount,
            v2_cg_path.as_deref(),
        );

        let mut leaf_cur: Option<u64> = None;
        let mut max_pct: Option<f64> = None;
        let mut min_headroom: Option<u32> = None;
        let mut saw_unbounded = false;

        for dir in dirs {
            let cur_val = std::fs::read_to_string(dir.join("memory.current"))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok());
            if leaf_cur.is_none() {
                leaf_cur = cur_val;
            }
            let effective_cur = match (cur_val, leaf_cur) {
                (Some(c), Some(l)) => Some(c.max(l)),
                (Some(c), None) => Some(c),
                (None, Some(l)) => Some(l),
                (None, None) => None,
            };

            let Ok(max_str) = std::fs::read_to_string(dir.join("memory.max")) else {
                continue;
            };
            let Some(cur) = effective_cur else {
                continue;
            };

            let mut candidate = MemoryMetrics::default();
            parse_cgroup_v2(&cur.to_string(), &max_str, &mut candidate);
            if let (Some(pct), Some(headroom)) = (
                candidate.cgroup_utilization_pct,
                candidate.cgroup_headroom_mb,
            ) {
                max_pct = Some(max_pct.map_or(pct, |prev| prev.max(pct)));
                min_headroom = Some(min_headroom.map_or(headroom, |prev| prev.min(headroom)));
            } else if candidate.cgroup_utilization_pct.is_some() {
                saw_unbounded = true;
            }
        }

        if let (Some(pct), Some(headroom)) = (max_pct, min_headroom) {
            m.cgroup_utilization_pct = Some(pct);
            m.cgroup_headroom_mb = Some(headroom);
        } else if saw_unbounded {
            m.cgroup_utilization_pct = Some(0.0);
        }
    }

    fn sample_cgroup_v1_hierarchy(mock_root: Option<&Path>, m: &mut MemoryMetrics) {
        let proc_cg = std::fs::read_to_string(resolve_subpath(mock_root, "proc/self/cgroup")).ok();
        let mountinfo =
            std::fs::read_to_string(resolve_subpath(mock_root, "proc/self/mountinfo")).ok();

        let (_, v1_cg_path) = proc_cg
            .as_deref()
            .map(parse_proc_self_cgroup_paths)
            .unwrap_or((None, None));
        let (_, v1_mount) = mountinfo
            .as_deref()
            .map(parse_mountinfo_cgroup_mounts)
            .unwrap_or((None, None));

        let dirs = build_cgroup_hierarchy_dirs(
            mock_root,
            "sys/fs/cgroup/memory",
            v1_mount,
            v1_cg_path.as_deref(),
        );

        let mut leaf_usage: Option<u64> = None;
        let mut leaf_stat: Option<String> = None;

        for dir in dirs {
            let usage_val = std::fs::read_to_string(dir.join("memory.usage_in_bytes"))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok());
            let stat_str = std::fs::read_to_string(dir.join("memory.stat")).ok();
            if leaf_usage.is_none() {
                leaf_usage = usage_val;
            }
            if leaf_stat.is_none() {
                leaf_stat = stat_str.clone();
            }

            let Ok(limit_str) = std::fs::read_to_string(dir.join("memory.limit_in_bytes")) else {
                continue;
            };
            let effective_usage = match (usage_val, leaf_usage) {
                (Some(u), Some(l)) => Some(u.max(l)),
                (Some(u), None) => Some(u),
                (None, Some(l)) => Some(l),
                (None, None) => None,
            };
            let Some(usage) = effective_usage else {
                continue;
            };
            let effective_stat = stat_str.as_deref().or(leaf_stat.as_deref());
            parse_cgroup_v1(&usage.to_string(), &limit_str, effective_stat, m);
        }
    }

    pub fn parse_psi(content: &str, m: &mut MemoryMetrics) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("some ") {
                if let Some(avg10) = extract_key_val(trimmed, "avg10=") {
                    m.psi_some_avg10 = avg10.parse().ok();
                }
            } else if trimmed.starts_with("full ") {
                if let Some(avg10) = extract_key_val(trimmed, "avg10=") {
                    m.psi_full_avg10 = avg10.parse().ok();
                }
            }
        }
    }

    pub fn parse_cgroup_v2(cur_str: &str, max_str: &str, m: &mut MemoryMetrics) {
        let cur: u64 = cur_str.trim().parse().unwrap_or(0);
        let max_trimmed = max_str.trim();
        if max_trimmed == "max" {
            m.cgroup_utilization_pct = Some(0.0);
            return;
        }
        if let Ok(max) = max_trimmed.parse::<u64>() {
            if max >= (1u64 << 62) || max == 0 {
                m.cgroup_utilization_pct = Some(0.0);
            } else {
                let pct = (cur as f64 / max as f64) * 100.0;
                let headroom = (max.saturating_sub(cur) / (1024 * 1024)) as u32;
                m.cgroup_utilization_pct = Some(pct);
                m.cgroup_headroom_mb = Some(headroom);
            }
        }
    }

    pub fn parse_cgroup_v1(
        usage_str: &str,
        limit_str: &str,
        stat_str: Option<&str>,
        m: &mut MemoryMetrics,
    ) {
        let usage: u64 = usage_str.trim().parse().unwrap_or(0);
        let limit: u64 = limit_str.trim().parse().unwrap_or(0);

        // Unbounded in v1 is >= 0x7FFFFFFFFFFFF000 (~9 exabytes)
        if limit == 0 || limit >= (1u64 << 62) {
            if m.cgroup_utilization_pct.is_none() {
                m.cgroup_utilization_pct = Some(0.0);
            }
            return;
        }

        let mut inactive_file: u64 = 0;
        if let Some(stats) = stat_str {
            for line in stats.lines() {
                let mut parts = line.split_whitespace();
                if let (Some(key), Some(val)) = (parts.next(), parts.next()) {
                    if key == "total_inactive_file" || key == "inactive_file" {
                        if let Ok(parsed) = val.parse::<u64>() {
                            inactive_file = parsed;
                        }
                    }
                }
            }
        }

        let working_set = usage.saturating_sub(inactive_file);
        let pct = (working_set as f64 / limit as f64) * 100.0;
        let headroom = (limit.saturating_sub(working_set) / (1024 * 1024)) as u32;

        // Adopt worst case across hierarchy/v2: max utilization pct and min headroom MB independently
        m.cgroup_utilization_pct = Some(
            m.cgroup_utilization_pct
                .map_or(pct, |existing_pct| existing_pct.max(pct)),
        );
        m.cgroup_headroom_mb = Some(
            m.cgroup_headroom_mb
                .map_or(headroom, |existing_hr| existing_hr.min(headroom)),
        );
    }

    pub fn parse_meminfo(content: &str, m: &mut MemoryMetrics) {
        let mut total_kb: Option<u64> = None;
        let mut avail_kb: Option<u64> = None;
        let mut free_kb: Option<u64> = None;
        let mut buffers_kb: Option<u64> = None;
        let mut cached_kb: Option<u64> = None;

        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                total_kb = extract_meminfo_kb(line);
            } else if line.starts_with("MemAvailable:") {
                avail_kb = extract_meminfo_kb(line);
            } else if line.starts_with("MemFree:") {
                free_kb = extract_meminfo_kb(line);
            } else if line.starts_with("Buffers:") {
                buffers_kb = extract_meminfo_kb(line);
            } else if line.starts_with("Cached:") {
                cached_kb = extract_meminfo_kb(line);
            }
        }

        let effective_avail_kb = avail_kb.or_else(|| {
            let free = free_kb.unwrap_or(0);
            let buf = buffers_kb.unwrap_or(0);
            let cache = cached_kb.unwrap_or(0);
            if free + buf + cache > 0 {
                Some(free + buf + cache)
            } else {
                None
            }
        });

        if let (Some(total), Some(avail)) = (total_kb, effective_avail_kb) {
            if total > 0 {
                m.host_available_pct = Some((avail as f64 / total as f64) * 100.0);
                m.host_available_mb = Some((avail / 1024) as u32);
            }
        }
    }

    fn extract_key_val<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
        line.split_whitespace().find_map(|s| s.strip_prefix(prefix))
    }

    fn extract_meminfo_kb(line: &str) -> Option<u64> {
        line.split_whitespace().nth(1)?.parse().ok()
    }

    #[cfg(target_os = "macos")]
    fn sample_darwin(m: &mut MemoryMetrics) {
        use std::ffi::{c_char, c_int, c_long, c_void};
        use std::mem::MaybeUninit;

        extern "C" {
            fn sysctlbyname(
                name: *const c_char,
                oldp: *mut c_void,
                oldlenp: *mut usize,
                newp: *const c_void,
                newlen: usize,
            ) -> c_int;
            fn mach_host_self() -> u32;
            fn host_statistics64(
                host_priv: u32,
                flavor: i32,
                host_info_out: *mut i32,
                host_info_outCnt: *mut u32,
            ) -> i32;
            fn sysconf(name: c_int) -> c_long;
        }

        const HOST_VM_INFO64: i32 = 4;
        const _SC_PAGESIZE: c_int = 29;

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

        unsafe {
            let mut level: i32 = 0;
            let mut size = std::mem::size_of::<i32>();
            let name_level = c"kern.memorystatus_vm_pressure_level".as_ptr();
            if sysctlbyname(
                name_level,
                &mut level as *mut _ as *mut _,
                &mut size,
                std::ptr::null(),
                0,
            ) == 0
            {
                m.darwin_pressure_level = Some(level);
            }

            let mut memsize: u64 = 0;
            let mut memsize_size = std::mem::size_of::<u64>();
            let name_mem = c"hw.memsize".as_ptr();
            if sysctlbyname(
                name_mem,
                &mut memsize as *mut _ as *mut _,
                &mut memsize_size,
                std::ptr::null(),
                0,
            ) == 0
                && memsize > 0
            {
                let mut vm_stat: MaybeUninit<VmStatistics64> = MaybeUninit::uninit();
                let mut count =
                    (std::mem::size_of::<VmStatistics64>() / std::mem::size_of::<i32>()) as u32;
                let host_port = mach_host_self();
                if host_statistics64(
                    host_port,
                    HOST_VM_INFO64,
                    vm_stat.as_mut_ptr() as *mut i32,
                    &mut count,
                ) == 0
                {
                    let stat = vm_stat.assume_init();
                    let page_size_raw = sysconf(_SC_PAGESIZE);
                    let page_size = if page_size_raw > 0 {
                        page_size_raw as u64
                    } else {
                        4096
                    };
                    let avail_bytes = (stat.free_count as u64
                        + stat.inactive_count as u64
                        + stat.speculative_count as u64)
                        * page_size;
                    m.host_available_pct = Some((avail_bytes as f64 / memsize as f64) * 100.0);
                    m.host_available_mb = Some((avail_bytes / (1024 * 1024)) as u32);
                }
            }
        }
    }

    pub fn trim_heap() {
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        unsafe {
            extern "C" {
                fn malloc_trim(pad: usize) -> std::ffi::c_int;
            }
            malloc_trim(0);
        }
        #[cfg(target_os = "macos")]
        unsafe {
            extern "C" {
                fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
            }
            malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::Mutex;

        static MP_TEST_MUTEX: Mutex<()> = Mutex::new(());

        #[test]
        fn test_psi_parsing_and_evaluation() {
            let _guard = MP_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let psi_normal = "some avg10=1.50 avg60=0.00 avg300=0.00 total=10\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
            let mut m = MemoryMetrics::default();
            parse_psi(psi_normal, &mut m);
            assert_eq!(m.psi_some_avg10, Some(1.5));
            assert_eq!(m.psi_full_avg10, Some(0.0));
            assert_eq!(evaluate_state(&m), PressureTier::Normal);

            let psi_constrained = "some avg10=15.20 avg60=5.00 avg300=1.00 total=100\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
            let mut m2 = MemoryMetrics::default();
            parse_psi(psi_constrained, &mut m2);
            assert_eq!(m2.psi_some_avg10, Some(15.2));
            assert_eq!(evaluate_state(&m2), PressureTier::Constrained);

            let psi_critical = "some avg10=35.00 avg60=20.00 avg300=10.00 total=500\nfull avg10=8.50 avg60=4.00 avg300=2.00 total=50\n";
            let mut m3 = MemoryMetrics::default();
            parse_psi(psi_critical, &mut m3);
            assert_eq!(m3.psi_some_avg10, Some(35.0));
            assert_eq!(m3.psi_full_avg10, Some(8.5));
            assert_eq!(evaluate_state(&m3), PressureTier::Critical);
        }

        #[test]
        fn test_cgroup_v2_parsing_and_extremes() {
            let _guard = MP_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let mut m = MemoryMetrics::default();
            parse_cgroup_v2("400000000\n", "500000000\n", &mut m);
            assert_eq!(m.cgroup_utilization_pct, Some(80.0));
            assert_eq!(m.cgroup_headroom_mb, Some(95));
            assert_eq!(evaluate_state(&m), PressureTier::Constrained);

            let mut m_max = MemoryMetrics::default();
            parse_cgroup_v2("100000000\n", "max\n", &mut m_max);
            assert_eq!(m_max.cgroup_utilization_pct, Some(0.0));
            assert_eq!(evaluate_state(&m_max), PressureTier::Normal);

            let mut m_crit = MemoryMetrics::default();
            parse_cgroup_v2("475000000\n", "500000000\n", &mut m_crit);
            assert_eq!(m_crit.cgroup_utilization_pct, Some(95.0));
            assert_eq!(evaluate_state(&m_crit), PressureTier::Critical);
        }

        #[test]
        fn test_cgroup_v1_working_set_accounting() {
            let _guard = MP_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let usage = "450000000\n";
            let limit = "500000000\n";
            let stat = "cache 100000000\nrss 300000000\ninactive_file 100000000\n";
            let mut m = MemoryMetrics::default();
            parse_cgroup_v1(usage, limit, Some(stat), &mut m);
            // working_set = 450MB - 100MB = 350MB. 350 / 500 = 70.0%
            assert_eq!(m.cgroup_utilization_pct, Some(70.0));
            assert_eq!(evaluate_state(&m), PressureTier::Normal);
        }

        #[test]
        fn test_meminfo_parsing() {
            let _guard = MP_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let meminfo = "MemTotal:       16384000 kB\nMemFree:         1000000 kB\nMemAvailable:    3276800 kB\nBuffers:          500000 kB\nCached:          2000000 kB\n";
            let mut m = MemoryMetrics::default();
            parse_meminfo(meminfo, &mut m);
            assert_eq!(m.host_available_pct, Some(20.0));
            assert_eq!(m.host_available_mb, Some(3200));
            assert_eq!(evaluate_state(&m), PressureTier::Constrained);
        }

        #[test]
        fn test_effective_calculations() {
            // Normal
            assert_eq!(
                calculate_effective_cache_limit(500, PressureTier::Normal, None),
                500
            );
            assert_eq!(
                calculate_effective_par_buffer(128, PressureTier::Normal, None),
                128
            );
            assert_eq!(
                calculate_effective_downloads_limit(20, PressureTier::Normal),
                20
            );

            // Constrained
            assert_eq!(
                calculate_effective_cache_limit(500, PressureTier::Constrained, Some(100)),
                60
            );
            assert_eq!(
                calculate_effective_cache_limit(500, PressureTier::Constrained, None),
                250
            );
            assert_eq!(
                calculate_effective_par_buffer(128, PressureTier::Constrained, Some(100)),
                50
            );
            assert_eq!(
                calculate_effective_par_buffer(128, PressureTier::Constrained, None),
                64
            );
            assert_eq!(
                calculate_effective_downloads_limit(20, PressureTier::Constrained),
                10
            );

            // Critical
            assert_eq!(
                calculate_effective_cache_limit(500, PressureTier::Critical, None),
                0
            );
            assert_eq!(
                calculate_effective_par_buffer(128, PressureTier::Critical, None),
                16
            );
            assert_eq!(
                calculate_effective_par_buffer(0, PressureTier::Critical, None),
                0
            );
            assert_eq!(
                calculate_effective_downloads_limit(20, PressureTier::Critical),
                5
            );
        }

        #[test]
        fn test_override_tier() {
            let _guard = MP_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let m = MemoryMetrics {
                psi_some_avg10: Some(50.0), // Critical
                ..Default::default()
            };
            assert_eq!(evaluate_state(&m), PressureTier::Critical);

            set_override_tier(0); // Force Normal
            assert_eq!(evaluate_state(&m), PressureTier::Normal);

            set_override_tier(1); // Force Constrained
            assert_eq!(evaluate_state(&m), PressureTier::Constrained);

            set_override_tier(-1); // Reset
            assert_eq!(evaluate_state(&m), PressureTier::Critical);
        }

        #[test]
        fn test_cgroup_hierarchy_sampling_v2_and_v1() {
            let _guard = MP_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let tmp =
                std::env::temp_dir().join(format!("nzbget_cg_hier_test_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&tmp);

            // Root cgroup is unbounded ("max"), child service cgroup is unbounded ("max"),
            // parent slice ("system.slice") has a 500 MB finite limit, and child has 400 MB usage.
            let proc_self = tmp.join("proc/self");
            let cg_root = tmp.join("sys/fs/cgroup");
            let cg_slice = cg_root.join("system.slice");
            let cg_service = cg_slice.join("nzbget.service");
            std::fs::create_dir_all(&proc_self).unwrap();
            std::fs::create_dir_all(&cg_service).unwrap();

            std::fs::write(
                proc_self.join("cgroup"),
                "0::/system.slice/nzbget.service\n",
            )
            .unwrap();
            std::fs::write(cg_root.join("memory.current"), "900000000\n").unwrap();
            std::fs::write(cg_root.join("memory.max"), "max\n").unwrap();
            std::fs::write(cg_slice.join("memory.max"), "500000000\n").unwrap();
            std::fs::write(cg_service.join("memory.current"), "400000000\n").unwrap();
            std::fs::write(cg_service.join("memory.max"), "max\n").unwrap();

            let m = sample_system_metrics(Some(&tmp));
            assert_eq!(m.cgroup_utilization_pct, Some(80.0));
            assert_eq!(m.cgroup_headroom_mb, Some(95));
            assert_eq!(evaluate_state(&m), PressureTier::Constrained);

            // Child has 15 MiB headroom (113 MiB / 128 MiB = 88.28%) while parent slice is at 89% (890 MiB / 1000 MiB):
            // min headroom (15 MiB) and max utilization (89.0%) must be aggregated independently -> Critical.
            std::fs::write(cg_service.join("memory.current"), (113u64 * 1024 * 1024).to_string()).unwrap();
            std::fs::write(cg_service.join("memory.max"), (128u64 * 1024 * 1024).to_string()).unwrap();
            std::fs::write(cg_slice.join("memory.current"), (890u64 * 1024 * 1024).to_string()).unwrap();
            std::fs::write(cg_slice.join("memory.max"), (1000u64 * 1024 * 1024).to_string()).unwrap();

            let m_nested = sample_system_metrics(Some(&tmp));
            assert_eq!(m_nested.cgroup_utilization_pct, Some(89.0));
            assert_eq!(m_nested.cgroup_headroom_mb, Some(15));
            assert_eq!(evaluate_state(&m_nested), PressureTier::Critical);

            // Now test cgroup v1 child hierarchy
            let _ = std::fs::remove_dir_all(&tmp);
            let proc_self_v1 = tmp.join("proc/self");
            let cg_v1_root = tmp.join("sys/fs/cgroup/memory");
            let cg_v1_leaf = cg_v1_root.join("docker/container1");
            std::fs::create_dir_all(&proc_self_v1).unwrap();
            std::fs::create_dir_all(&cg_v1_leaf).unwrap();

            std::fs::write(
                proc_self_v1.join("cgroup"),
                "6:cpu,cpuacct:/docker/container1\n5:memory:/docker/container1\n",
            )
            .unwrap();
            std::fs::write(cg_v1_root.join("memory.usage_in_bytes"), "100000000\n").unwrap();
            std::fs::write(
                cg_v1_root.join("memory.limit_in_bytes"),
                "9223372036854771712\n",
            )
            .unwrap();
            std::fs::write(cg_v1_leaf.join("memory.usage_in_bytes"), "475000000\n").unwrap();
            std::fs::write(cg_v1_leaf.join("memory.limit_in_bytes"), "500000000\n").unwrap();

            let m_v1 = sample_system_metrics(Some(&tmp));
            assert_eq!(m_v1.cgroup_utilization_pct, Some(95.0));
            assert_eq!(evaluate_state(&m_v1), PressureTier::Critical);

            let _ = std::fs::remove_dir_all(&tmp);
        }
    }
}

pub mod lifecycle {
    //! Subsystem lifecycle coordination and phase state machine (Milestone M5 — Requirement R2).

    use std::ffi::{c_char, c_int};
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::RwLock;
    use std::time::Duration;

    #[cfg(unix)]
    const SIGHUP: c_int = 1;
    #[cfg(unix)]
    const SIGINT: c_int = 2;
    #[cfg(unix)]
    const SIGTERM: c_int = 15;

    #[cfg(unix)]
    extern "C" {
        fn dlsym(handle: *mut std::ffi::c_void, symbol: *const c_char) -> *mut std::ffi::c_void;
    }

    #[cfg(target_os = "macos")]
    const RTLD_DEFAULT: *mut std::ffi::c_void = -2isize as *mut std::ffi::c_void;
    #[cfg(all(unix, not(target_os = "macos")))]
    const RTLD_DEFAULT: *mut std::ffi::c_void = std::ptr::null_mut();

    /// Daemon lifecycle phases managed by Rust orchestrator.
    #[repr(u32)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum LifecyclePhase {
        Uninitialized = 0,
        OptionsLogging = 1,
        SubsystemsRecovery = 2,
        ServersActive = 3,
        ShutdownReload = 4,
        Terminated = 5,
    }

    impl From<u32> for LifecyclePhase {
        fn from(v: u32) -> Self {
            match v {
                1 => LifecyclePhase::OptionsLogging,
                2 => LifecyclePhase::SubsystemsRecovery,
                3 => LifecyclePhase::ServersActive,
                4 => LifecyclePhase::ShutdownReload,
                5 => LifecyclePhase::Terminated,
                _ => LifecyclePhase::Uninitialized,
            }
        }
    }

    /// Function table of C++ subsystem lifecycle bridge hooks.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct LifecycleHooks {
        pub set_process_args:
            Option<unsafe extern "C" fn(c_int, *mut *mut c_char, *mut *mut c_char)>,
        pub util_init: Option<unsafe extern "C" fn()>,
        pub nserv_main: Option<unsafe extern "C" fn(c_int, *mut *mut c_char) -> c_int>,
        pub win_service_check: Option<unsafe extern "C" fn(c_int, *mut *mut c_char) -> c_int>,
        pub phase1_init: Option<unsafe extern "C" fn(bool) -> c_int>,
        pub phase2_subsystems_create_and_recover: Option<unsafe extern "C" fn(bool) -> c_int>,
        pub phase3_server_activate: Option<unsafe extern "C" fn() -> c_int>,
        pub phase4_shutdown: Option<unsafe extern "C" fn(bool)>,
        pub has_active_unpack: Option<unsafe extern "C" fn() -> bool>,
        pub is_reloading: Option<unsafe extern "C" fn() -> bool>,
        pub stop_server: Option<unsafe extern "C" fn(bool)>,
        pub reload: Option<unsafe extern "C" fn()>,
        pub exit_proc: Option<unsafe extern "C" fn()>,
    }

    static REGISTERED_HOOKS: RwLock<Option<LifecycleHooks>> = RwLock::new(None);

    pub fn register_hooks(hooks: LifecycleHooks) {
        if let Ok(mut lock) = REGISTERED_HOOKS.write() {
            *lock = Some(hooks);
        }
    }

    pub fn get_hooks() -> Option<LifecycleHooks> {
        REGISTERED_HOOKS.read().ok().and_then(|guard| *guard)
    }

    #[cfg(unix)]
    unsafe fn resolve_sym<T: Copy>(name: &[u8]) -> Option<T> {
        let sym = dlsym(RTLD_DEFAULT, name.as_ptr() as *const c_char);
        if sym.is_null() {
            None
        } else {
            Some(std::mem::transmute_copy(&sym))
        }
    }

    pub fn resolve_hooks() -> Option<LifecycleHooks> {
        #[cfg(unix)]
        unsafe {
            let set_process_args = resolve_sym(b"nzbget_c_set_process_args\0");
            let util_init = resolve_sym(b"nzbget_c_util_init\0");
            let nserv_main = resolve_sym(b"nzbget_c_nserv_main\0");
            let win_service_check = None;
            let phase1_init = resolve_sym(b"nzbget_c_phase1_init\0");
            let phase2_subsystems_create_and_recover =
                resolve_sym(b"nzbget_c_phase2_subsystems_create_and_recover\0");
            let phase3_server_activate = resolve_sym(b"nzbget_c_phase3_server_activate\0");
            let phase4_shutdown = resolve_sym(b"nzbget_c_phase4_shutdown\0");
            let has_active_unpack = resolve_sym(b"nzbget_c_has_active_unpack\0");
            let is_reloading = resolve_sym(b"nzbget_c_is_reloading\0");
            let stop_server = resolve_sym(b"nzbget_c_stop_server\0");
            let reload = resolve_sym(b"nzbget_c_reload\0");
            let exit_proc = resolve_sym(b"nzbget_c_exit_proc\0");

            if phase1_init.is_some() && phase3_server_activate.is_some() {
                Some(LifecycleHooks {
                    set_process_args,
                    util_init,
                    nserv_main,
                    win_service_check,
                    phase1_init,
                    phase2_subsystems_create_and_recover,
                    phase3_server_activate,
                    phase4_shutdown,
                    has_active_unpack,
                    is_reloading,
                    stop_server,
                    reload,
                    exit_proc,
                })
            } else {
                None
            }
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    pub struct LifecycleManager {
        phase: AtomicU32,
        reloading: AtomicBool,
        stopping: AtomicBool,
    }

    static LIFECYCLE_MANAGER: LifecycleManager = LifecycleManager {
        phase: AtomicU32::new(LifecyclePhase::Uninitialized as u32),
        reloading: AtomicBool::new(false),
        stopping: AtomicBool::new(false),
    };

    impl LifecycleManager {
        pub fn global() -> &'static Self {
            &LIFECYCLE_MANAGER
        }

        pub fn get_phase(&self) -> LifecyclePhase {
            self.phase.load(Ordering::SeqCst).into()
        }

        pub fn set_phase(&self, phase: LifecyclePhase) {
            self.phase.store(phase as u32, Ordering::SeqCst);
        }

        pub fn is_reloading(&self) -> bool {
            self.reloading.load(Ordering::SeqCst)
        }

        pub fn is_stopping(&self) -> bool {
            self.stopping.load(Ordering::SeqCst)
        }

        pub fn set_reloading(&self, val: bool) {
            self.reloading.store(val, Ordering::SeqCst);
        }

        pub fn set_stopping(&self, val: bool) {
            self.stopping.store(val, Ordering::SeqCst);
        }

        pub fn trigger_reload(&self) {
            if self.is_stopping() {
                return;
            }
            if self
                .reloading
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return;
            }
            if let Some(hooks) = get_hooks().or_else(resolve_hooks) {
                if let Some(reload_fn) = hooks.reload {
                    unsafe { reload_fn() };
                }
            }
        }

        pub fn trigger_shutdown(&self) {
            if self
                .stopping
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return;
            }
            if let Some(hooks) = get_hooks().or_else(resolve_hooks) {
                if let Some(exit_fn) = hooks.exit_proc {
                    unsafe { exit_fn() };
                }
            }
        }

        pub fn notify_signal(&self, signum: c_int) {
            #[cfg(unix)]
            {
                if signum == SIGHUP {
                    if !self.is_stopping() {
                        self.reloading.store(true, Ordering::SeqCst);
                    }
                } else if signum == SIGINT || signum == SIGTERM {
                    self.stopping.store(true, Ordering::SeqCst);
                }
            }
            #[cfg(not(unix))]
            {
                let _ = signum;
            }
        }

        pub fn run_daemon_loop(
            &self,
            _argc: c_int,
            _argv: *mut *mut c_char,
            _argp: *mut *mut c_char,
        ) -> c_int {
            let hooks = get_hooks()
                .or_else(resolve_hooks)
                .expect("Lifecycle hooks must be registered");

            let mut reloading = false;
            let mut exit_code = 0;

            while !self.is_stopping() {
                // Phase 1: Platform & Preflight, Options & Logging
                self.set_phase(LifecyclePhase::OptionsLogging);
                let p1_fn = hooks.phase1_init.expect("phase1_init hook required");
                let p1_rc = unsafe { p1_fn(reloading) };
                if p1_rc != 0 {
                    // Non-zero return indicates early exit or client operation complete
                    exit_code = if p1_rc > 0 { p1_rc } else { 0 };
                    break;
                }

                // Phase 2: Core Subsystems Creation & State Recovery
                self.set_phase(LifecyclePhase::SubsystemsRecovery);
                if let Some(p2_fn) = hooks.phase2_subsystems_create_and_recover {
                    let p2_rc = unsafe { p2_fn(reloading) };
                    if p2_rc != 0 {
                        exit_code = p2_rc;
                        break;
                    }
                }

                // Phase 3: Server Activation & Wait
                self.set_phase(LifecyclePhase::ServersActive);
                let p3_fn = hooks.phase3_server_activate.expect("phase3 hook required");
                let p3_rc = unsafe { p3_fn() };
                if p3_rc != 0 {
                    exit_code = p3_rc;
                }

                // Phase 4: Orderly Teardown & In-Process Reload Looping
                self.set_phase(LifecyclePhase::ShutdownReload);

                // Active unpack deferral check (Requirement R3):
                // If in-process reload is requested, ensure active unpack finishes before tearing down
                let c_reloading = hooks.is_reloading.map(|f| unsafe { f() }).unwrap_or(false);
                let mut is_reload = !self.is_stopping()
                    && if hooks.reload.is_some() && hooks.is_reloading.is_some() {
                        c_reloading
                    } else {
                        self.is_reloading() || c_reloading
                    };

                if is_reload {
                    if let Some(unpack_fn) = hooks.has_active_unpack {
                        while !self.is_stopping() && unsafe { unpack_fn() } {
                            std::thread::sleep(Duration::from_millis(100));
                        }
                    }
                    if self.is_stopping()
                        || (hooks.reload.is_some()
                            && hooks.is_reloading.is_some()
                            && !hooks.is_reloading.map(|f| unsafe { f() }).unwrap_or(false))
                    {
                        is_reload = false;
                    }
                }

                if let Some(p4_fn) = hooks.phase4_shutdown {
                    unsafe { p4_fn(is_reload) };
                }

                if !is_reload {
                    break;
                }

                reloading = true;
                self.set_reloading(false);
            }

            self.set_phase(LifecyclePhase::Terminated);
            exit_code
        }
    }

    #[cfg(test)]
    pub(crate) static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::atomic::AtomicUsize;

        #[test]
        fn test_phase_conversions() {
            assert_eq!(LifecyclePhase::from(0), LifecyclePhase::Uninitialized);
            assert_eq!(LifecyclePhase::from(1), LifecyclePhase::OptionsLogging);
            assert_eq!(LifecyclePhase::from(2), LifecyclePhase::SubsystemsRecovery);
            assert_eq!(LifecyclePhase::from(3), LifecyclePhase::ServersActive);
            assert_eq!(LifecyclePhase::from(4), LifecyclePhase::ShutdownReload);
            assert_eq!(LifecyclePhase::from(5), LifecyclePhase::Terminated);
            assert_eq!(LifecyclePhase::from(99), LifecyclePhase::Uninitialized);
        }

        #[test]
        fn test_lifecycle_manager_state() {
            let mgr = LifecycleManager {
                phase: AtomicU32::new(LifecyclePhase::Uninitialized as u32),
                reloading: AtomicBool::new(false),
                stopping: AtomicBool::new(false),
            };

            assert_eq!(mgr.get_phase(), LifecyclePhase::Uninitialized);
            assert!(!mgr.is_reloading());
            assert!(!mgr.is_stopping());

            mgr.set_phase(LifecyclePhase::OptionsLogging);
            assert_eq!(mgr.get_phase(), LifecyclePhase::OptionsLogging);

            mgr.set_reloading(true);
            assert!(mgr.is_reloading());
            mgr.set_reloading(false);
            assert!(!mgr.is_reloading());

            mgr.set_stopping(true);
            assert!(mgr.is_stopping());
            mgr.set_stopping(false);
            assert!(!mgr.is_stopping());
        }

        #[test]
        fn test_trigger_reload_stops_during_shutdown() {
            let mgr = LifecycleManager {
                phase: AtomicU32::new(LifecyclePhase::ShutdownReload as u32),
                reloading: AtomicBool::new(false),
                stopping: AtomicBool::new(true),
            };
            mgr.trigger_reload();
            assert!(!mgr.is_reloading());
        }

        static MOCK_CALL_COUNT: AtomicUsize = AtomicUsize::new(0);
        static MOCK_P1_RELOAD: AtomicBool = AtomicBool::new(false);
        static MOCK_P4_RELOAD: AtomicBool = AtomicBool::new(false);
        static MOCK_UNPACK_COUNT: AtomicUsize = AtomicUsize::new(0);

        unsafe extern "C" fn mock_phase1(reload: bool) -> c_int {
            MOCK_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
            MOCK_P1_RELOAD.store(reload, Ordering::SeqCst);
            0
        }

        unsafe extern "C" fn mock_phase2(_reload: bool) -> c_int {
            MOCK_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
            0
        }

        unsafe extern "C" fn mock_phase3() -> c_int {
            MOCK_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
            0
        }

        unsafe extern "C" fn mock_phase4(reload: bool) {
            MOCK_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
            MOCK_P4_RELOAD.store(reload, Ordering::SeqCst);
        }

        unsafe extern "C" fn mock_has_active_unpack() -> bool {
            if MOCK_UNPACK_COUNT.load(Ordering::SeqCst) > 0 {
                MOCK_UNPACK_COUNT.fetch_sub(1, Ordering::SeqCst);
                true
            } else {
                false
            }
        }

        unsafe extern "C" fn mock_is_reloading() -> bool {
            false
        }

        #[test]
        fn test_daemon_loop_full_clean_cycle() {
            let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            MOCK_CALL_COUNT.store(0, Ordering::SeqCst);
            MOCK_P1_RELOAD.store(false, Ordering::SeqCst);
            MOCK_P4_RELOAD.store(false, Ordering::SeqCst);
            MOCK_UNPACK_COUNT.store(0, Ordering::SeqCst);

            let hooks = LifecycleHooks {
                set_process_args: None,
                util_init: None,
                nserv_main: None,
                win_service_check: None,
                phase1_init: Some(mock_phase1),
                phase2_subsystems_create_and_recover: Some(mock_phase2),
                phase3_server_activate: Some(mock_phase3),
                phase4_shutdown: Some(mock_phase4),
                has_active_unpack: Some(mock_has_active_unpack),
                is_reloading: Some(mock_is_reloading),
                stop_server: None,
                reload: None,
                exit_proc: None,
            };

            register_hooks(hooks);

            let mgr = LifecycleManager {
                phase: AtomicU32::new(LifecyclePhase::Uninitialized as u32),
                reloading: AtomicBool::new(false),
                stopping: AtomicBool::new(false),
            };

            let rc = mgr.run_daemon_loop(1, std::ptr::null_mut(), std::ptr::null_mut());
            assert_eq!(rc, 0);
            assert_eq!(mgr.get_phase(), LifecyclePhase::Terminated);
            assert_eq!(MOCK_CALL_COUNT.load(Ordering::SeqCst), 4);
            assert!(!MOCK_P1_RELOAD.load(Ordering::SeqCst));
            assert!(!MOCK_P4_RELOAD.load(Ordering::SeqCst));
        }

        unsafe extern "C" fn mock_phase1_client_exit(_reload: bool) -> c_int {
            10
        }

        #[test]
        fn test_daemon_loop_early_exit_client_mode() {
            let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let hooks = LifecycleHooks {
                set_process_args: None,
                util_init: None,
                nserv_main: None,
                win_service_check: None,
                phase1_init: Some(mock_phase1_client_exit),
                phase2_subsystems_create_and_recover: Some(mock_phase2),
                phase3_server_activate: Some(mock_phase3),
                phase4_shutdown: Some(mock_phase4),
                has_active_unpack: Some(mock_has_active_unpack),
                is_reloading: Some(mock_is_reloading),
                stop_server: None,
                reload: None,
                exit_proc: None,
            };

            register_hooks(hooks);

            let mgr = LifecycleManager {
                phase: AtomicU32::new(LifecyclePhase::Uninitialized as u32),
                reloading: AtomicBool::new(false),
                stopping: AtomicBool::new(false),
            };

            let rc = mgr.run_daemon_loop(1, std::ptr::null_mut(), std::ptr::null_mut());
            assert_eq!(rc, 10);
            assert_eq!(mgr.get_phase(), LifecyclePhase::Terminated);
        }

        static RELOAD_SIM_STEP: AtomicUsize = AtomicUsize::new(0);

        unsafe extern "C" fn mock_is_reloading_once() -> bool {
            if RELOAD_SIM_STEP.load(Ordering::SeqCst) == 0 {
                RELOAD_SIM_STEP.store(1, Ordering::SeqCst);
                true
            } else {
                false
            }
        }

        #[test]
        fn test_daemon_loop_in_process_reload_and_unpack_deferral() {
            let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            RELOAD_SIM_STEP.store(0, Ordering::SeqCst);
            MOCK_UNPACK_COUNT.store(2, Ordering::SeqCst); // 2 active unpack polls
            MOCK_CALL_COUNT.store(0, Ordering::SeqCst);
            MOCK_P1_RELOAD.store(false, Ordering::SeqCst);
            MOCK_P4_RELOAD.store(false, Ordering::SeqCst);

            let hooks = LifecycleHooks {
                set_process_args: None,
                util_init: None,
                nserv_main: None,
                win_service_check: None,
                phase1_init: Some(mock_phase1),
                phase2_subsystems_create_and_recover: Some(mock_phase2),
                phase3_server_activate: Some(mock_phase3),
                phase4_shutdown: Some(mock_phase4),
                has_active_unpack: Some(mock_has_active_unpack),
                is_reloading: Some(mock_is_reloading_once),
                stop_server: None,
                reload: None,
                exit_proc: None,
            };

            register_hooks(hooks);

            let mgr = LifecycleManager {
                phase: AtomicU32::new(LifecyclePhase::Uninitialized as u32),
                reloading: AtomicBool::new(false),
                stopping: AtomicBool::new(false),
            };

            let rc = mgr.run_daemon_loop(1, std::ptr::null_mut(), std::ptr::null_mut());
            assert_eq!(rc, 0);
            assert_eq!(mgr.get_phase(), LifecyclePhase::Terminated);
            // Ran 2 full cycles (4 calls each = 8 calls)
            assert_eq!(MOCK_CALL_COUNT.load(Ordering::SeqCst), 8);
            assert_eq!(MOCK_UNPACK_COUNT.load(Ordering::SeqCst), 0);
        }
    }
}

pub mod entrypoint {
    //! Rust daemon entrypoint and pre-flight CLI routing (Milestone M5 — Requirement R2 & R4).

    use crate::lifecycle::{get_hooks, resolve_hooks, LifecycleManager};
    use std::ffi::{c_char, c_int, CStr};
    use std::ptr;

    #[cfg(unix)]
    extern "C" {
        fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
        fn time(tloc: *mut i64) -> i64;
        fn srand(seed: std::ffi::c_uint);
        fn rapidyenc_decode_init();
        fn rapidyenc_crc_init();
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
            rapidyenc_decode_init();
            rapidyenc_crc_init();
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
}
