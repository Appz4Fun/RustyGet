//! End-to-end Rust implementation of `DupeProbe` (`daemon/queue/DupeProbe.cpp`):
//! even-spread sample indexing (`sample_indexes`), NNTP `STAT` reply classification
//! (`classify`), dead-posting verdict (`is_dead`), server group/level ordering
//! (`select_server_order`), and probe/recheck outcome evaluation and log formatting
//! (`eval_run_outcome`, `eval_recheck_outcome`).

pub const DUPE_PROBE_EXISTS: i32 = 0;
pub const DUPE_PROBE_MISSING: i32 = 1;
pub const DUPE_PROBE_UNKNOWN: i32 = 2;

pub const SAMPLE_COUNT: i32 = 10;
pub const MIN_ARTICLES: i32 = 4;
pub const MIN_ALIVE_SAMPLES: i32 = 2;
pub const RECENT_BLOCK_SEC: i64 = 60;

/// Computes `DupeProbe::SampleIndexes(total, count)`.
pub fn sample_indexes(total: usize, count: i32) -> Vec<usize> {
    if total == 0 || count <= 0 {
        return Vec::new();
    }
    let wanted = total.min(count as usize);
    let mut indexes = Vec::with_capacity(wanted);
    for i in 0..wanted {
        let num = (2 * i + 1) as u128 * (total as u128);
        let den = (2 * wanted) as u128;
        indexes.push((num / den) as usize);
    }
    indexes
}

/// Classifies an NNTP `STAT` response for `DupeProbe::Classify`.
pub fn classify(response: Option<&[u8]>) -> i32 {
    let Some(resp) = response else {
        return DUPE_PROBE_UNKNOWN;
    };
    if resp.starts_with(b"2") {
        return DUPE_PROBE_EXISTS;
    }
    if resp.starts_with(b"400") || resp.starts_with(b"499") {
        return DUPE_PROBE_UNKNOWN;
    }
    if resp.starts_with(b"41")
        || resp.starts_with(b"42")
        || resp.starts_with(b"43")
        || resp.starts_with(b"451")
    {
        return DUPE_PROBE_MISSING;
    }
    DUPE_PROBE_UNKNOWN
}

/// Computes `DupeProbe::IsDead(existing, definitive_servers, active_servers)`.
pub fn is_dead(existing: i32, definitive_servers: i32, active_servers: i32) -> bool {
    existing < MIN_ALIVE_SAMPLES && definitive_servers > 0 && definitive_servers >= active_servers
}

/// Selects and sorts active servers for `DupeProbe::Measure`:
/// filters to `active[i] != 0`, deduplicates positive `groups[i]` in iteration order,
/// sorts by `(norm_levels[i], ids[i])`, and returns the sorted server `ids`.
pub fn select_server_order(
    active: &[u8],
    groups: &[i32],
    norm_levels: &[i32],
    ids: &[i32],
) -> Vec<i32> {
    let n = active
        .len()
        .min(groups.len())
        .min(norm_levels.len())
        .min(ids.len());
    let mut seen_groups: Vec<i32> = Vec::new();
    let mut order: Vec<(i32, i32)> = Vec::new();
    for i in 0..n {
        if active[i] == 0 {
            continue;
        }
        let g = groups[i];
        if g > 0 {
            if seen_groups.contains(&g) {
                continue;
            }
            seen_groups.push(g);
        }
        order.push((norm_levels[i], ids[i]));
    }
    order.sort_unstable();
    order.into_iter().map(|(_, id)| id).collect()
}

/// Evaluates the outcome and formats the log message for `DupeProbe::Run`.
/// Returns `(outcome, message)`:
/// - `1`: dead posting (`Note(mkInfo, msg)` + `Abandon`)
/// - `2`: alive posting (`Note(mkInfo, msg)`)
/// - `3`: no server reached (`SetDeadPickProbed(false)` + `PrintMessage(mkDetail, msg)`)
/// - `4`: inconclusive verdict (`Note(mkInfo, msg)`)
/// - `0`: unfinished / cancelled
#[allow(clippy::too_many_arguments)]
pub fn eval_run_outcome(
    existing: i32,
    sampled: i32,
    missing_servers: i32,
    active_servers: i32,
    unreachable_servers: i32,
    reached_servers: i32,
    finished: bool,
    found_on: &[u8],
) -> (i32, Vec<u8>) {
    let dead = finished && is_dead(existing, missing_servers, active_servers);
    let unreachable_suffix = if unreachable_servers > 0 {
        format!(", {unreachable_servers} unreachable left out")
    } else {
        String::new()
    };

    if dead {
        let mut out = format!(
            "Dupe probe: {existing} of {sampled} sampled articles exist ({missing_servers} of {active_servers} servers answered definitively{unreachable_suffix})"
        )
        .into_bytes();
        if !found_on.is_empty() {
            out.extend_from_slice(b", on ");
            out.extend_from_slice(found_on);
        }
        out.extend_from_slice(b": the posting is dead");
        (1, out)
    } else if existing >= MIN_ALIVE_SAMPLES {
        let mut out = format!(
            "Dupe probe: {existing} of {sampled} sampled articles found (it stops at {MIN_ALIVE_SAMPLES}), the first on "
        )
        .into_bytes();
        out.extend_from_slice(found_on);
        out.extend_from_slice(b": not abandoning it");
        (2, out)
    } else if finished && reached_servers == 0 {
        (
            3,
            b"Dupe probe: no server could be asked, will probe again".to_vec(),
        )
    } else if finished {
        let out = format!(
            "Dupe probe: no verdict ({existing} of {sampled} sampled articles exist, {missing_servers} of {active_servers} servers answered definitively{unreachable_suffix})"
        )
        .into_bytes();
        (4, out)
    } else {
        (0, Vec::new())
    }
}

/// Evaluates the outcome and formats the log message for `DupeProbe::Recheck`.
/// Returns `(outcome, message)`:
/// - `1`: no server reached (`PrintMessage(mkInfo, msg)` + clear `*DupeRecheck`)
/// - `2`: duplicate queued in place (`PrintMessage(mkInfo, msg)`)
/// - `3`: retry failed articles (`PrintMessage(mkInfo, msg)` + `eaHistoryRetryFailed`)
/// - `4`: do not retry (`PrintMessage(mkInfo, msg)`)
pub fn eval_recheck_outcome(
    existing: i32,
    sampled: i32,
    reached_servers: i32,
    duplicate_queued_name: &[u8],
) -> (i32, Vec<u8>) {
    if reached_servers == 0 {
        return (
            1,
            b"Failed articles not rechecked: no server connection was free".to_vec(),
        );
    }
    let retry = existing.saturating_mul(2) >= sampled;
    if retry && !duplicate_queued_name.is_empty() {
        let mut out = b"Not retrying failed articles: duplicate ".to_vec();
        out.extend_from_slice(duplicate_queued_name);
        out.extend_from_slice(b" downloads in its place");
        return (2, out);
    }
    let suffix = if retry {
        ": retrying failed articles"
    } else {
        "; not retrying"
    };
    let out = format!("{existing} of {sampled} failed articles exist on the servers{suffix}")
        .into_bytes();
    (if retry { 3 } else { 4 }, out)
}

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const CONNECTION_WAIT_SEC: i64 = 10;
pub const PROBE_LIMIT_SEC: i32 = 90;
pub const RECHECK_LIMIT_SEC: i32 = 60;

pub type DupeProbeHostFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    a3: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    ints: *mut c_int,
    strs: *mut *const c_char,
) -> usize;

#[derive(Clone, Debug)]
pub struct ProbeSample {
    pub message_id: Vec<u8>,
    pub groups: Vec<Vec<u8>>,
}

#[derive(Default, Clone, Debug)]
struct ServerResult {
    exists: i32,
    missing: i32,
    unknown: i32,
    unreachable: bool,
}

#[derive(Clone, Debug)]
struct Verdict {
    existing: i32,
    missing_servers: i32,
    found_on: Vec<u8>,
    active_servers: i32,
    unreachable_servers: i32,
    reached_servers: i32,
    finished: bool,
}

impl Verdict {
    fn new() -> Self {
        Self {
            existing: 0,
            missing_servers: 0,
            found_on: Vec::new(),
            active_servers: 0,
            unreachable_servers: 0,
            reached_servers: 0,
            finished: true,
        }
    }

    fn dead(&self) -> bool {
        self.finished && is_dead(self.existing, self.missing_servers, self.active_servers)
    }
}

struct ActiveProbeEntry {
    nzb_id: i32,
    count_all: bool,
    started: i64,
    stopped: Arc<AtomicBool>,
    thread_ptr: usize,
    conn_ptr: usize,
    host_cb: DupeProbeHostFn,
}

#[derive(Default)]
struct ProbeRegistry {
    stopping: bool,
    entries: BTreeMap<u64, ActiveProbeEntry>,
}

static NEXT_PROBE_ID: AtomicU64 = AtomicU64::new(1);
static PROBE_COUNT: AtomicI32 = AtomicI32::new(0);
static PROBE_REGISTRY: OnceLock<Mutex<ProbeRegistry>> = OnceLock::new();

fn registry() -> &'static Mutex<ProbeRegistry> {
    PROBE_REGISTRY.get_or_init(|| Mutex::new(ProbeRegistry::default()))
}

fn now_sec() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn c_buf(bytes: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(bytes.len() + 1);
    for &b in bytes {
        if b == 0 {
            break;
        }
        v.push(b);
    }
    v.push(0);
    v
}

unsafe fn bytes_from_ptr(p: *const c_char) -> Vec<u8> {
    if p.is_null() {
        Vec::new()
    } else {
        unsafe { CStr::from_ptr(p).to_bytes().to_vec() }
    }
}

pub fn probe_reset() {
    if let Ok(mut reg) = registry().lock() {
        reg.stopping = false;
    }
}

pub fn probe_probing_for(nzb_id: i32) -> i32 {
    let Ok(reg) = registry().lock() else {
        return -1;
    };
    let now = now_sec();
    for entry in reg.entries.values() {
        if entry.nzb_id == nzb_id && !entry.count_all {
            return (now - entry.started).max(0) as i32;
        }
    }
    -1
}

pub fn probe_stop_all() {
    let Ok(mut reg) = registry().lock() else {
        return;
    };
    reg.stopping = true;
    for entry in reg.entries.values() {
        entry.stopped.store(true, Ordering::SeqCst);
        unsafe {
            (entry.host_cb)(
                std::ptr::null_mut(),
                10,
                0,
                0,
                0,
                entry.thread_ptr,
                entry.conn_ptr,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
    }
}

pub fn probe_wait_all() {
    while PROBE_COUNT.load(Ordering::SeqCst) > 0 {
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub struct NzbgetRsDupeProbe {
    id: u64,
    nzb_id: i32,
    count_all: bool,
    recovered_at_start: i32,
    samples: Vec<ProbeSample>,
    found: Vec<bool>,
    found_on: Vec<u8>,
    stopped: Arc<AtomicBool>,
    thread_ptr: usize,
    registered: bool,
}

impl NzbgetRsDupeProbe {
    pub fn new(nzb_id: i32, count_all: bool, recovered_at_start: i32) -> Self {
        Self {
            id: NEXT_PROBE_ID.fetch_add(1, Ordering::Relaxed),
            nzb_id,
            count_all,
            recovered_at_start,
            samples: Vec::new(),
            found: Vec::new(),
            found_on: Vec::new(),
            stopped: Arc::new(AtomicBool::new(false)),
            thread_ptr: 0,
            registered: false,
        }
    }

    pub fn add_sample(&mut self, message_id: Vec<u8>, groups: Vec<Vec<u8>>) {
        self.samples.push(ProbeSample { message_id, groups });
        self.found.push(false);
    }

    pub fn register(&mut self, thread_ptr: usize, host_cb: DupeProbeHostFn) -> bool {
        let Ok(mut reg) = registry().lock() else {
            return false;
        };
        if reg.stopping {
            return false;
        }
        self.thread_ptr = thread_ptr;
        reg.entries.insert(
            self.id,
            ActiveProbeEntry {
                nzb_id: self.nzb_id,
                count_all: self.count_all,
                started: now_sec(),
                stopped: Arc::clone(&self.stopped),
                thread_ptr,
                conn_ptr: 0,
                host_cb,
            },
        );
        PROBE_COUNT.fetch_add(1, Ordering::SeqCst);
        self.registered = true;
        true
    }

    pub fn unregister(&mut self) {
        if !self.registered {
            return;
        }
        if let Ok(mut reg) = registry().lock() {
            reg.entries.remove(&self.id);
        }
        self.registered = false;
        PROBE_COUNT.fetch_sub(1, Ordering::SeqCst);
    }

    fn set_conn(&self, conn_ptr: usize) {
        if let Ok(mut reg) = registry().lock() {
            if let Some(entry) = reg.entries.get_mut(&self.id) {
                entry.conn_ptr = conn_ptr;
            }
        }
    }

    fn is_stopped(&self, ctx: *mut c_void, host_cb: DupeProbeHostFn) -> bool {
        if self.stopped.load(Ordering::SeqCst) {
            return true;
        }
        if self.thread_ptr != 0 {
            let s = unsafe {
                host_cb(
                    ctx,
                    11,
                    0,
                    0,
                    0,
                    self.thread_ptr,
                    0,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if s != 0 {
                return true;
            }
        }
        false
    }

    fn probe_server(
        &mut self,
        ctx: *mut c_void,
        host_cb: DupeProbeHostFn,
        server_id: i32,
        probed: &mut BTreeSet<i32>,
    ) -> (bool, ServerResult) {
        let mut result = ServerResult::default();
        let exists = unsafe {
            host_cb(
                ctx,
                2,
                server_id,
                0,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if exists == 0 {
            return (false, result);
        }

        let mut conn_ptr: usize = 0;
        let mut actual_server_id: i32 = 0;
        let mut join_group = false;
        let mut server_name: Vec<u8> = Vec::new();
        let wait_start = now_sec();
        while conn_ptr == 0
            && !self.is_stopped(ctx, host_cb)
            && now_sec() - wait_start <= CONNECTION_WAIT_SEC
        {
            let mut ints = [0i32; 4];
            let mut strs: [*const c_char; 2] = [std::ptr::null(); 2];
            conn_ptr = unsafe {
                host_cb(
                    ctx,
                    3,
                    server_id,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    ints.as_mut_ptr(),
                    strs.as_mut_ptr(),
                )
            };
            if conn_ptr != 0 {
                actual_server_id = ints[0];
                join_group = ints[1] != 0;
                server_name = unsafe { bytes_from_ptr(strs[0]) };
            } else {
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        if conn_ptr == 0 {
            let unreach = unsafe {
                host_cb(
                    ctx,
                    4,
                    server_id,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            result.unreachable = unreach != 0;
            return (false, result);
        }

        let fresh = probed.insert(actual_server_id);
        let mut probed_server = false;
        if fresh {
            self.set_conn(conn_ptr);

            let mut remaining = self.samples.len() as i32;
            for index in 0..self.samples.len() {
                if self.is_stopped(ctx, host_cb) {
                    break;
                }
                remaining -= 1;
                if self.found[index] {
                    continue;
                }

                let connected = unsafe {
                    host_cb(
                        ctx,
                        5,
                        0,
                        0,
                        0,
                        conn_ptr,
                        0,
                        std::ptr::null(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                };
                if connected == 0 {
                    result.unknown += remaining + 1;
                    result.unreachable = result.exists + result.missing == 0;
                    break;
                }

                if join_group {
                    let mut joined: Option<Vec<u8>> = None;
                    for group in &self.samples[index].groups {
                        let g_c = c_buf(group);
                        let mut strs: [*const c_char; 2] = [std::ptr::null(); 2];
                        let ok = unsafe {
                            host_cb(
                                ctx,
                                6,
                                0,
                                0,
                                0,
                                conn_ptr,
                                0,
                                g_c.as_ptr() as *const c_char,
                                std::ptr::null_mut(),
                                strs.as_mut_ptr(),
                            )
                        };
                        if ok != 0 && !strs[0].is_null() {
                            let resp = unsafe { bytes_from_ptr(strs[0]) };
                            let is_2xx = resp.starts_with(b"2");
                            joined = Some(resp);
                            if is_2xx {
                                break;
                            }
                        } else {
                            joined = None;
                        }
                    }
                    let joined_ok = match joined {
                        Some(ref r) => r.starts_with(b"2"),
                        None => false,
                    };
                    if !joined_ok {
                        result.unknown += remaining + 1;
                        if joined.is_none() {
                            unsafe {
                                host_cb(
                                    ctx,
                                    8,
                                    0,
                                    0,
                                    0,
                                    conn_ptr,
                                    0,
                                    std::ptr::null(),
                                    std::ptr::null_mut(),
                                    std::ptr::null_mut(),
                                );
                            }
                        }
                        break;
                    }
                }

                let mut cmd = Vec::with_capacity(self.samples[index].message_id.len() + 8);
                cmd.extend_from_slice(b"STAT ");
                cmd.extend_from_slice(&self.samples[index].message_id);
                cmd.extend_from_slice(b"\r\n");
                if cmd.len() >= 1024 {
                    cmd.truncate(1023);
                }
                let cmd_c = c_buf(&cmd);
                let mut strs: [*const c_char; 2] = [std::ptr::null(); 2];
                let ok = unsafe {
                    host_cb(
                        ctx,
                        7,
                        0,
                        0,
                        0,
                        conn_ptr,
                        0,
                        cmd_c.as_ptr() as *const c_char,
                        std::ptr::null_mut(),
                        strs.as_mut_ptr(),
                    )
                };
                if ok == 0 || strs[0].is_null() {
                    unsafe {
                        host_cb(
                            ctx,
                            8,
                            0,
                            0,
                            0,
                            conn_ptr,
                            0,
                            std::ptr::null(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        );
                    }
                    result.unknown += remaining + 1;
                    break;
                }

                let resp = unsafe { bytes_from_ptr(strs[0]) };
                match classify(Some(&resp)) {
                    DUPE_PROBE_EXISTS => {
                        result.exists += 1;
                        self.found[index] = true;
                        if self.found_on.is_empty() {
                            self.found_on.extend_from_slice(&server_name);
                            self.found_on.extend_from_slice(b" (");
                            self.found_on.extend_from_slice(&self.samples[index].message_id);
                            self.found_on.push(b')');
                        }
                    }
                    DUPE_PROBE_MISSING => {
                        result.missing += 1;
                    }
                    _ => {
                        result.unknown += 1;
                    }
                }

                let found_count = self.found.iter().filter(|&&b| b).count() as i32;
                if !self.count_all && found_count >= MIN_ALIVE_SAMPLES {
                    break;
                }
            }
            probed_server = true;
        }

        self.set_conn(0);
        let stopped_flag = i32::from(self.is_stopped(ctx, host_cb));
        unsafe {
            host_cb(
                ctx,
                9,
                stopped_flag,
                0,
                0,
                conn_ptr,
                0,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
        (probed_server, result)
    }

    fn measure(&mut self, ctx: *mut c_void, host_cb: DupeProbeHostFn, limit_sec: i32) -> Verdict {
        let mut verdict = Verdict::new();
        let n_servers = unsafe {
            host_cb(
                ctx,
                0,
                0,
                0,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        let mut active = Vec::with_capacity(n_servers);
        let mut groups = Vec::with_capacity(n_servers);
        let mut norm_levels = Vec::with_capacity(n_servers);
        let mut ids = Vec::with_capacity(n_servers);
        for i in 0..n_servers {
            let mut ints = [0i32; 4];
            unsafe {
                host_cb(
                    ctx,
                    1,
                    i as c_int,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    ints.as_mut_ptr(),
                    std::ptr::null_mut(),
                );
            }
            active.push(ints[0] as u8);
            groups.push(ints[1]);
            norm_levels.push(ints[2]);
            ids.push(ints[3]);
        }
        let order = select_server_order(&active, &groups, &norm_levels, &ids);
        verdict.active_servers = order.len() as i32;

        let mut probed = BTreeSet::new();
        let start = now_sec();
        for server_id in order {
            if self.is_stopped(ctx, host_cb) || now_sec() - start > i64::from(limit_sec) {
                verdict.finished = false;
                break;
            }

            let (reached, result) = self.probe_server(ctx, host_cb, server_id, &mut probed);
            if result.unreachable {
                verdict.active_servers -= 1;
                verdict.unreachable_servers += 1;
            }
            if !reached {
                continue;
            }
            verdict.reached_servers += 1;
            verdict.existing = self.found.iter().filter(|&&b| b).count() as i32;
            verdict.found_on = self.found_on.clone();
            if self.count_all {
                if verdict.existing == self.samples.len() as i32 {
                    break;
                }
                continue;
            }
            if verdict.existing >= MIN_ALIVE_SAMPLES {
                break;
            }
            if result.unknown == 0 && result.exists + result.missing > 0 {
                verdict.missing_servers += 1;
            }
        }
        if self.is_stopped(ctx, host_cb) {
            verdict.finished = false;
        }
        verdict
    }

    pub fn run(&mut self, ctx: *mut c_void, host_cb: DupeProbeHostFn) {
        if self.count_all {
            let verdict = self.measure(ctx, host_cb, RECHECK_LIMIT_SEC);
            if !self.is_stopped(ctx, host_cb) && verdict.finished {
                let mut ints = [verdict.existing, verdict.reached_servers, 0, 0];
                unsafe {
                    host_cb(
                        ctx,
                        13,
                        self.nzb_id,
                        self.samples.len() as c_int,
                        0,
                        0,
                        0,
                        std::ptr::null(),
                        ints.as_mut_ptr(),
                        std::ptr::null_mut(),
                    );
                }
            }
            self.unregister();
            return;
        }

        let verdict = self.measure(ctx, host_cb, PROBE_LIMIT_SEC);
        if self.is_stopped(ctx, host_cb) {
            self.unregister();
            return;
        }

        let (outcome, text) = eval_run_outcome(
            verdict.existing,
            self.samples.len() as i32,
            verdict.missing_servers,
            verdict.active_servers,
            verdict.unreachable_servers,
            verdict.reached_servers,
            verdict.finished,
            &verdict.found_on,
        );
        if outcome != 0 {
            let text_c = c_buf(&text);
            let mut ints = [
                verdict.missing_servers,
                verdict.active_servers,
                self.recovered_at_start,
                0,
            ];
            unsafe {
                host_cb(
                    ctx,
                    12,
                    outcome,
                    self.nzb_id,
                    self.samples.len() as c_int,
                    0,
                    0,
                    text_c.as_ptr() as *const c_char,
                    ints.as_mut_ptr(),
                    std::ptr::null_mut(),
                );
            }
        }
        self.unregister();
    }

    pub fn check(&mut self, limit_sec: i32, ctx: *mut c_void, host_cb: DupeProbeHostFn) -> bool {
        if !self.register(0, host_cb) {
            return false;
        }
        let verdict = self.measure(ctx, host_cb, limit_sec);
        self.unregister();
        verdict.dead()
    }
}

pub fn probe_recheck_locked(
    dq_ptr: usize,
    nzb_id: i32,
    sampled: i32,
    existing: i32,
    reached_servers: i32,
    ctx: *mut c_void,
    host_cb: DupeProbeHostFn,
) {
    let mut ints = [0i32; 4];
    let mut strs: [*const c_char; 2] = [std::ptr::null(); 2];
    let hist_ptr = unsafe {
        host_cb(
            ctx,
            14,
            nzb_id,
            0,
            0,
            dq_ptr,
            0,
            std::ptr::null(),
            ints.as_mut_ptr(),
            strs.as_mut_ptr(),
        )
    };
    if hist_ptr == 0 {
        return;
    }
    let queue_len = ints[0].max(0) as usize;
    let nzb_name = unsafe { bytes_from_ptr(strs[0]) };
    let nzb_key = unsafe { bytes_from_ptr(strs[1]) };
    let mut dup_name = Vec::new();
    if reached_servers > 0 && existing.saturating_mul(2) >= sampled {
        for idx in 0..queue_len {
            let mut q_strs: [*const c_char; 2] = [std::ptr::null(); 2];
            unsafe {
                host_cb(
                    ctx,
                    15,
                    idx as c_int,
                    0,
                    0,
                    dq_ptr,
                    0,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    q_strs.as_mut_ptr(),
                );
            }
            let q_name = unsafe { bytes_from_ptr(q_strs[0]) };
            let q_key = unsafe { bytes_from_ptr(q_strs[1]) };
            if crate::dupecoord::same_name_or_key(&q_name, &q_key, &nzb_name, &nzb_key) {
                dup_name = q_name;
                break;
            }
        }
    }
    let (action, text) = eval_recheck_outcome(existing, sampled, reached_servers, &dup_name);
    let text_c = c_buf(&text);
    unsafe {
        host_cb(
            ctx,
            16,
            action,
            0,
            0,
            dq_ptr,
            hist_ptr,
            text_c.as_ptr() as *const c_char,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
}

impl Drop for NzbgetRsDupeProbe {
    fn drop(&mut self) {
        self.unregister();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_indexes_and_classify() {
        assert_eq!(sample_indexes(100, 4), vec![12, 37, 62, 87]);
        assert_eq!(classify(Some(b"223 0 <msg@id>")), DUPE_PROBE_EXISTS);
        assert_eq!(classify(Some(b"430 No such article")), DUPE_PROBE_MISSING);
        assert_eq!(classify(Some(b"400 Service busy")), DUPE_PROBE_UNKNOWN);
        assert!(is_dead(1, 3, 3));
        assert!(!is_dead(2, 3, 3));
        assert!(!is_dead(0, 2, 3));
    }

    #[test]
    fn run_and_recheck_outcomes() {
        let (code, msg) = eval_run_outcome(0, 10, 3, 3, 1, 3, true, b"");
        assert_eq!(code, 1);
        assert_eq!(
            msg,
            b"Dupe probe: 0 of 10 sampled articles exist (3 of 3 servers answered definitively, 1 unreachable left out): the posting is dead"
        );

        let (code, msg) = eval_recheck_outcome(6, 10, 2, b"");
        assert_eq!(code, 3);
        assert_eq!(
            msg,
            b"6 of 10 failed articles exist on the servers: retrying failed articles"
        );
    }

    #[test]
    fn dupe_probe_full_suite_port_from_cpp() {
        // 1. DupeProbeSampleIndexesTest
        let indexes = sample_indexes(100, 10);
        assert_eq!(indexes.len(), 10);
        assert_eq!(*indexes.first().unwrap(), 5);
        assert_eq!(*indexes.last().unwrap(), 95);
        for i in 1..indexes.len() {
            assert_eq!(indexes[i] - indexes[i - 1], 10);
        }

        let indexes4 = sample_indexes(4, 10);
        assert_eq!(indexes4.len(), 4);
        assert_eq!(indexes4[0], 0);
        assert_eq!(indexes4[3], 3);

        for total in 1..300 {
            let idx = sample_indexes(total, 10);
            for i in 0..idx.len() {
                assert!(idx[i] < total);
                assert!(i == 0 || idx[i] > idx[i - 1]);
            }
        }
        assert!(sample_indexes(0, 10).is_empty());
        assert!(sample_indexes(10, 0).is_empty());

        // 2. DupeProbeClassifyTest
        assert_eq!(classify(Some(b"223 0 <id> article exists")), DUPE_PROBE_EXISTS);
        assert_eq!(classify(Some(b"430 No Such Article Found")), DUPE_PROBE_MISSING);
        assert_eq!(classify(Some(b"423 No such article number")), DUPE_PROBE_MISSING);
        assert_eq!(classify(Some(b"412 no newsgroup selected")), DUPE_PROBE_MISSING);
        assert_eq!(classify(Some(b"451 Article not found")), DUPE_PROBE_MISSING);

        assert_eq!(classify(None), DUPE_PROBE_UNKNOWN);
        assert_eq!(
            classify(Some(b"400 service temporarily unavailable")),
            DUPE_PROBE_UNKNOWN
        );
        assert_eq!(classify(Some(b"499 connection limit")), DUPE_PROBE_UNKNOWN);
        assert_eq!(classify(Some(b"481 authentication failed")), DUPE_PROBE_UNKNOWN);
        assert_eq!(classify(Some(b"502 too many connections")), DUPE_PROBE_UNKNOWN);
        assert_eq!(classify(Some(b"500 unknown command")), DUPE_PROBE_UNKNOWN);

        // 3. DupeProbeIsDeadTest
        assert!(is_dead(0, 8, 8));
        assert!(!is_dead(0, 7, 8));
        assert!(!is_dead(0, 5, 8));

        assert!(is_dead(0, 1, 1));
        assert!(is_dead(0, 3, 3));
        assert!(!is_dead(0, 2, 3));

        assert!(is_dead(1, 8, 8));
        assert!(!is_dead(2, 8, 8));
        assert!(!is_dead(3, 5, 5));
        assert!(!is_dead(1, 4, 8));

        assert!(!is_dead(0, 0, 0));
        assert!(!is_dead(0, 0, 6));
    }
}
