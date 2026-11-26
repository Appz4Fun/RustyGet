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

/// Selects and sorts active servers for `DupeProbe::Measure`: one entry per
/// backbone (servers with the same positive `groups[i]` read from one article
/// store) and one per independent server, sorted by `(norm_level, id)` of the
/// entry's first server. Each entry lists the servers to try in order: the
/// longest retention first (0 = not set counts as the longest), then by
/// `(norm_level, id)`. A probe moves on to the next server of an entry only
/// when the one before could not be reached, so a sibling with a full
/// connection limit or a refused login never makes the backbone look empty.
pub fn select_server_order(
    active: &[u8],
    groups: &[i32],
    norm_levels: &[i32],
    retentions: &[i32],
    ids: &[i32],
) -> Vec<Vec<i32>> {
    let n = active
        .len()
        .min(groups.len())
        .min(norm_levels.len())
        .min(retentions.len())
        .min(ids.len());
    let mut entries: Vec<(i32, Vec<usize>)> = Vec::new();
    for i in 0..n {
        if active[i] == 0 {
            continue;
        }
        let g = groups[i];
        match entries.iter_mut().find(|(eg, _)| g > 0 && *eg == g) {
            Some((_, members)) => members.push(i),
            None => entries.push((g, vec![i])),
        }
    }
    let keep = |r: i32| if r <= 0 { i32::MAX } else { r };
    let mut out: Vec<((i32, i32), Vec<i32>)> = entries
        .into_iter()
        .map(|(_, mut members)| {
            let first = members
                .iter()
                .map(|&i| (norm_levels[i], ids[i]))
                .min()
                .unwrap_or_default();
            members.sort_unstable_by_key(|&i| {
                (std::cmp::Reverse(keep(retentions[i])), norm_levels[i], ids[i])
            });
            (first, members.into_iter().map(|i| ids[i]).collect())
        })
        .collect();
    out.sort_unstable_by_key(|(first, _)| *first);
    out.into_iter().map(|(_, members)| members).collect()
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
        "; retrying them"
    } else {
        "; not retrying"
    };
    let out = format!("{existing} of {sampled} failed articles exist on the servers{suffix}")
        .into_bytes();
    (if retry { 3 } else { 4 }, out)
}

use std::collections::{BTreeMap, BTreeSet};
#[cfg(unix)]
use std::ffi::{c_char, CStr};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const CONNECTION_WAIT_SEC: i64 = 10;
pub const PROBE_LIMIT_SEC: i32 = 90;
pub const RECHECK_LIMIT_SEC: i32 = 60;

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
    conn_ptr: usize,
    /// Set once the probe got its first connection (see [`probe_waiting_for_connection`]).
    connected: bool,
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

#[cfg(unix)]
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

/// Most STATs a probe sends before it reads their answers (pipelined).
pub const STAT_PIPELINE_DEPTH: usize = 16;

/// The samples a probe asks at once from `start` (pipelined): the ones not
/// found yet, at most [`STAT_PIPELINE_DEPTH`]; on a server that needs its
/// group joined, only those of the group joined for `start`'s sample.
pub fn stat_batch(samples: &[ProbeSample], found: &[bool], start: usize, join_group: bool) -> Vec<usize> {
    let mut out = Vec::new();
    for i in start..samples.len() {
        if out.len() >= STAT_PIPELINE_DEPTH {
            break;
        }
        if found.get(i).copied().unwrap_or(false) {
            continue;
        }
        if (join_group && i != start && samples[i].groups != samples[start].groups)
            || !pipelinable(&samples[i].message_id)
        {
            break;
        }
        out.push(i);
    }
    out
}

/// Whether a message-id can be sent in a pipelined STAT: no byte that could
/// end the command or start another (CR, LF, NUL, space, tab), at most 250
/// bytes as RFC 3977 allows plus its brackets.
pub fn pipelinable(message_id: &[u8]) -> bool {
    !message_id.is_empty()
        && message_id.len() <= 252
        && !message_id.iter().any(|&b| matches!(b, b'\r' | b'\n' | 0 | b' ' | b'\t'))
}

/// `STAT <id>` with its line end, never past 1023 bytes nor past a NUL in
/// the id (as the single request was sent).
pub fn stat_command(message_id: &[u8]) -> Vec<u8> {
    let id = message_id.split(|&b| b == 0).next().unwrap_or(&[]);
    let mut cmd = Vec::with_capacity(id.len() + 7);
    cmd.extend_from_slice(b"STAT ");
    cmd.extend_from_slice(id);
    cmd.truncate(1021);
    cmd.extend_from_slice(b"\r\n");
    cmd
}

#[cfg(unix)]
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

/// True while the dead-pick probe of download `nzb_id` waits for its first connection, for at
/// most [`CONNECTION_WAIT_SEC`]: the queue then picks no article of that download (other
/// downloads go on), so a connection the download frees goes to the probe. Without that, a
/// download using every connection of a small server takes each freed one back at once, and
/// the probe, polling for one, may only get it after the download failed over on its health
/// anyway (functional `deadpickstray`, two connections).
pub fn probe_waiting_for_connection(nzb_id: i32) -> bool {
    if PROBE_COUNT.load(Ordering::SeqCst) == 0 {
        return false;
    }
    let Ok(reg) = registry().lock() else {
        return false;
    };
    let now = now_sec();
    reg.entries.values().any(|entry| {
        entry.nzb_id == nzb_id
            && !entry.count_all
            && !entry.connected
            && !entry.stopped.load(Ordering::SeqCst)
            && now - entry.started <= CONNECTION_WAIT_SEC
    })
}

pub fn probe_stop_all() {
    let Ok(mut reg) = registry().lock() else {
        return;
    };
    reg.stopping = true;
    #[cfg(unix)]
    let has_pool = crate::net::active_server_pool().is_some();
    for entry in reg.entries.values() {
        entry.stopped.store(true, Ordering::SeqCst);
        #[cfg(unix)]
        if has_pool && entry.conn_ptr != 0 {
            unsafe {
                let c = &*(entry.conn_ptr as *const crate::net::PooledNntpConn);
                c.set_suppress_errors(true);
                c.cancel();
            }
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
            registered: false,
        }
    }

    pub fn add_sample(&mut self, message_id: Vec<u8>, groups: Vec<Vec<u8>>) {
        self.samples.push(ProbeSample { message_id, groups });
        self.found.push(false);
    }

    pub fn register(&mut self) -> bool {
        let Ok(mut reg) = registry().lock() else {
            return false;
        };
        if reg.stopping {
            return false;
        }
        reg.entries.insert(
            self.id,
            ActiveProbeEntry {
                nzb_id: self.nzb_id,
                count_all: self.count_all,
                started: now_sec(),
                stopped: Arc::clone(&self.stopped),
                conn_ptr: 0,
                connected: false,
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

    #[cfg(unix)]
    fn set_conn(&self, conn_ptr: usize) {
        if let Ok(mut reg) = registry().lock() {
            if let Some(entry) = reg.entries.get_mut(&self.id) {
                entry.conn_ptr = conn_ptr;
                entry.connected |= conn_ptr != 0;
            }
        }
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    #[cfg(unix)]
    fn probe_server_native(
        &mut self,
        pool: &crate::net::NzbgetRsServerPool,
        server_id: i32,
        probed: &mut BTreeSet<i32>,
    ) -> (bool, ServerResult) {
        let mut result = ServerResult::default();
        if pool.server_by_id(server_id).is_none() {
            return (false, result);
        }

        let mut conn_ptr: usize = 0;
        let mut actual_server_id: i32 = 0;
        let mut join_group = false;
        let mut server_name: Vec<u8> = Vec::new();
        let wait_start = now_sec();
        while conn_ptr == 0 && !self.is_stopped() && now_sec() - wait_start <= CONNECTION_WAIT_SEC {
            let raw = crate::light_pool::check_connection_by_server_id(pool, server_id);
            if !raw.is_null() {
                let c = unsafe { &*raw };
                conn_ptr = raw as usize;
                actual_server_id = c.server.id;
                join_group = c.server.join_group;
                server_name = c.server.name.to_bytes().to_vec();
            } else {
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        if conn_ptr == 0 {
            result.unreachable = match pool.server_by_id(server_id) {
                None => false,
                Some(wanted) => {
                    let bt = wanted.block_time.load(Ordering::SeqCst);
                    pool.is_server_blocked(&wanted) || (bt > 0 && now_sec() - bt < 60)
                }
            };
            return (false, result);
        }

        // goes back to the pool however this probe ends
        let checkout =
            crate::light_pool::Checkout::new(pool, conn_ptr as *mut crate::net::PooledNntpConn);
        let fresh = probed.insert(actual_server_id);
        let mut probed_server = false;
        // a server that could not even connect (login refused, TLS, network) before
        // answering anything does not count as reached: a sibling is asked instead
        let mut never_answered = false;
        if fresh {
            self.set_conn(conn_ptr);
            let conn = unsafe { &*(conn_ptr as *const crate::net::PooledNntpConn) };
            unsafe {
                conn.prepare_for_use();
            }

            let n = self.samples.len();
            let mut index = 0usize;
            'walk: while index < n {
                if self.is_stopped() {
                    break;
                }
                if self.found[index] {
                    index += 1;
                    continue;
                }
                if !pipelinable(&self.samples[index].message_id) {
                    // an id that could carry a second command (CR, LF, a
                    // space) or is longer than a message-id may be is never
                    // sent: unknown, never evidence
                    result.unknown += 1;
                    index += 1;
                    continue;
                }

                if !unsafe { conn.connect() } {
                    result.unknown += (n - index) as i32;
                    never_answered = result.exists + result.missing == 0;
                    // a light connection that could not connect proves nothing
                    // about the server: unknown, never fewer servers to a dead verdict
                    result.unreachable = never_answered && !crate::light_pool::is_light(conn);
                    break;
                }

                if join_group {
                    let mut joined: Option<Vec<u8>> = None;
                    for group in &self.samples[index].groups {
                        let g_c = c_buf(group);
                        let ans = unsafe { conn.join_group(g_c.as_ptr() as *const c_char) };
                        if !ans.is_null() {
                            let resp = unsafe { bytes_from_ptr(ans) };
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
                        result.unknown += (n - index) as i32;
                        if joined.is_none() {
                            unsafe {
                                conn.disconnect();
                            }
                        }
                        break;
                    }
                }

                // the samples left from here, asked at once (pipelined): one
                // round trip for the batch instead of one per article
                let batch = stat_batch(&self.samples, &self.found, index, join_group);
                let mut cmds = Vec::with_capacity(batch.len() * 64);
                for &i in &batch {
                    cmds.extend_from_slice(&stat_command(&self.samples[i].message_id));
                }
                if !unsafe { conn.send_bytes(&cmds) } {
                    conn.note_io(false);
                    unsafe {
                        conn.disconnect();
                    }
                    result.unknown += (n - index) as i32;
                    break;
                }
                let mut answers: Vec<i32> = Vec::with_capacity(batch.len());
                let mut line = [0u8; 1024];
                for _ in &batch {
                    match unsafe { conn.read_line(&mut line) } {
                        // only a whole line is an answer: one that fills the
                        // buffer has more behind it, one without its end was
                        // cut by a close; either way stop here
                        Some(len) if len > 0 && len < line.len() - 1 && line[len - 1] == b'\n' => answers.push(classify(Some(&line[..len]))),
                        _ => break,
                    }
                }
                let read_all = answers.len() == batch.len();
                // the light reserve's flakiness check counts what the server
                // did: one outcome per answer, as one request each did
                for _ in &answers {
                    conn.note_io(true);
                }
                let mut broken = !read_all;
                if broken {
                    conn.note_io(false);
                    // out of step or closed: never handed to the next borrower
                    unsafe {
                        conn.disconnect();
                    }
                }
                for (k, &i) in batch.iter().enumerate() {
                    let Some(&read) = answers.get(k) else {
                        // never answered (the connection broke in the batch):
                        // this and every sample after it unknown
                        result.unknown += (n - i) as i32;
                        break 'walk;
                    };
                    let mut cls = read;
                    if cls == DUPE_PROBE_UNKNOWN && !broken {
                        // an answer that is neither (a login asked for again,
                        // a busy server): asked alone, the way that logs in
                        // again and resends
                        let cmd_c = c_buf(&stat_command(&self.samples[i].message_id));
                        let ans = unsafe { conn.request(cmd_c.as_ptr() as *const c_char) };
                        if ans.is_null() {
                            unsafe {
                                conn.disconnect();
                            }
                            // the answers already read still count
                            broken = true;
                        } else {
                            cls = classify(Some(&unsafe { bytes_from_ptr(ans) }));
                        }
                    }
                    match cls {
                        DUPE_PROBE_EXISTS => {
                            result.exists += 1;
                            self.found[i] = true;
                            if self.found_on.is_empty() {
                                self.found_on.extend_from_slice(&server_name);
                                self.found_on.extend_from_slice(b" (");
                                self.found_on.extend_from_slice(&self.samples[i].message_id);
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
                        break 'walk;
                    }
                }
                let next = batch.last().map_or(index + 1, |&last| last + 1);
                if broken {
                    // the connection is gone: the samples after the batch unknown
                    result.unknown += (n - next) as i32;
                    break;
                }
                index = next;
            }
            probed_server = !never_answered;
        }

        self.set_conn(0);
        let stopped_flag = self.is_stopped();
        let conn = unsafe { &*(conn_ptr as *const crate::net::PooledNntpConn) };
        if stopped_flag || conn.is_cancelled() {
            unsafe {
                conn.disconnect();
            }
        }
        checkout.release(true);
        (probed_server, result)
    }

    fn probe_server(&mut self, server_id: i32, probed: &mut BTreeSet<i32>) -> (bool, ServerResult) {
        #[cfg(unix)]
        if let Some(pool) = crate::net::active_server_pool() {
            return self.probe_server_native(&pool, server_id, probed);
        }
        #[cfg(not(unix))]
        let _ = (server_id, probed);
        (false, ServerResult::default())
    }

    fn measure(&mut self, limit_sec: i32) -> Verdict {
        let mut verdict = Verdict::new();
        let mut active = Vec::new();
        let mut groups = Vec::new();
        let mut norm_levels = Vec::new();
        let mut retentions = Vec::new();
        let mut ids = Vec::new();
        #[cfg(unix)]
        if let Some(pool) = crate::net::active_server_pool() {
            let srvs = pool.servers_snapshot();
            active.reserve(srvs.len());
            groups.reserve(srvs.len());
            norm_levels.reserve(srvs.len());
            retentions.reserve(srvs.len());
            ids.reserve(srvs.len());
            for s in &srvs {
                active.push(u8::from(s.active.load(Ordering::SeqCst)));
                groups.push(s.group);
                norm_levels.push(s.norm_level.load(Ordering::SeqCst));
                retentions.push(s.retention);
                ids.push(s.id);
            }
        }
        let order = select_server_order(&active, &groups, &norm_levels, &retentions, &ids);
        verdict.active_servers = order.len() as i32;

        let mut probed = BTreeSet::new();
        let start = now_sec();
        'entries: for entry in order {
            let mut reached = false;
            let mut result = ServerResult::default();
            for &server_id in &entry {
                if self.is_stopped() || now_sec() - start > i64::from(limit_sec) {
                    verdict.finished = false;
                    break 'entries;
                }
                (reached, result) = self.probe_server(server_id, &mut probed);
                if reached {
                    break;
                }
                // could not use this one (full, blocked, login refused): try a sibling
            }
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
        if self.is_stopped() {
            verdict.finished = false;
        }
        verdict
    }

    pub fn run(&mut self) {
        if self.count_all {
            let verdict = self.measure(RECHECK_LIMIT_SEC);
            if !self.is_stopped() && verdict.finished {
                #[cfg(unix)]
                if let Some(qh) = crate::ffi::queue_host() {
                    let sampled = self.samples.len() as i32;
                    let nzb_id = self.nzb_id;
                    qh.with_locked_queue(|dq_ptr| {
                        probe_recheck_host(
                            &qh,
                            dq_ptr,
                            nzb_id,
                            sampled,
                            verdict.existing,
                            verdict.reached_servers,
                        );
                    });
                }
            }
            self.unregister();
            return;
        }

        let verdict = self.measure(PROBE_LIMIT_SEC);
        if self.is_stopped() {
            self.unregister();
            return;
        }

        let sampled = self.samples.len() as i32;
        let (outcome, text) = eval_run_outcome(
            verdict.existing,
            sampled,
            verdict.missing_servers,
            verdict.active_servers,
            verdict.unreachable_servers,
            verdict.reached_servers,
            verdict.finished,
            &verdict.found_on,
        );
        #[cfg(unix)]
        if outcome != 0 {
            if let Some((qc, qh)) = crate::ffi::active_queue_coord_and_host() {
                let nzb_id = self.nzb_id;
                if outcome == 3 {
                    qh.with_locked_queue(|dq_ptr| {
                        let nzb_ptr = qh.queue_find_by_id(dq_ptr, nzb_id);
                        if nzb_ptr.is_some() {
                            qh.nzb_set_int(
                                nzb_ptr,
                                crate::queuesubsys_host::NZB_INT_DEAD_PICK_PROBED,
                                0,
                            );
                            qh.nzb_print_message(
                                nzb_ptr,
                                crate::queuesubsys_host::MK_DETAIL,
                                &text,
                            );
                        }
                    });
                } else {
                    qh.with_locked_queue(|dq_ptr| {
                        let nzb_ptr = qh.queue_find_by_id(dq_ptr, nzb_id);
                        if nzb_ptr.is_some() {
                            qh.nzb_print_message(nzb_ptr, crate::queuesubsys_host::MK_INFO, &text);
                        } else {
                            let mut d = format!("Dupe probe of download {nzb_id}: ").into_bytes();
                            d.extend_from_slice(&text);
                            qh.log(crate::queuesubsys_host::MK_DETAIL, &d);
                        }
                    });
                    if outcome == 1 {
                        let rec = self.recovered_at_start;
                        qh.with_locked_queue(|dq_ptr| {
                            qc.fail_over_dead_pick(
                                &qh,
                                dq_ptr,
                                nzb_id,
                                sampled,
                                verdict.missing_servers,
                                verdict.active_servers,
                                rec,
                            );
                        });
                    }
                }
            }
        }
        #[cfg(not(unix))]
        let _ = (outcome, text);
        self.unregister();
    }

    pub fn check(&mut self, limit_sec: i32) -> bool {
        if !self.register() {
            return false;
        }
        let verdict = self.measure(limit_sec);
        self.unregister();
        verdict.dead()
    }
}

#[cfg(unix)]
pub(crate) fn launch_probe_rs(mut probe: Box<NzbgetRsDupeProbe>) {
    if !probe.register() {
        return;
    }
    let stopped = Arc::clone(&probe.stopped);
    let _ = std::thread::Builder::new()
        .name("dupe-probe".into())
        .spawn(move || {
            let _tg = crate::ffi::RustThreadGuard::enter(&stopped);
            probe.run();
        });
}

#[cfg(unix)]
pub(crate) fn start_recheck_rs(nzb_id: i32, samples: Vec<ProbeSample>) {
    let mut probe = Box::new(NzbgetRsDupeProbe::new(nzb_id, true, 0));
    for s in samples {
        probe.add_sample(s.message_id, s.groups);
    }
    launch_probe_rs(probe);
}

#[cfg(unix)]
fn probe_recheck_host(
    qh: &crate::queuesubsys_host::QueueSubsysHost,
    dq_ptr: usize,
    nzb_id: i32,
    sampled: i32,
    existing: i32,
    reached_servers: i32,
) {
    let hist_len = qh.history_len(dq_ptr);
    let mut found_hi = None;
    let mut found_nzb = None;
    for i in 0..hist_len {
        let hi = qh.history_at(dq_ptr, i);
        if hi.kind == crate::queuesubsys_host::HK_NZB && hi.nzb().is_some() {
            let snap = qh.nzb_snap(hi.nzb(), false);
            if snap.id == nzb_id {
                found_hi = Some(hi);
                found_nzb = Some(snap);
                break;
            }
        }
    }
    let (Some(hi), Some(nzb)) = (found_hi, found_nzb) else {
        return;
    };
    let mut dup_name = Vec::new();
    if reached_servers > 0 && existing.saturating_mul(2) >= sampled {
        let q_len = qh.queue_len(dq_ptr);
        for idx in 0..q_len {
            let q_ptr = qh.queue_at(dq_ptr, idx);
            let q = qh.nzb_snap(q_ptr, false);
            if crate::dupecoord::same_name_or_key(&q.name, &q.dupe_key, &nzb.name, &nzb.dupe_key) {
                dup_name = q.name;
                break;
            }
        }
    }
    let (action, text) = eval_recheck_outcome(existing, sampled, reached_servers, &dup_name);
    qh.nzb_print_message(hi.nzb(), crate::queuesubsys_host::MK_INFO, &text);
    if action == 1 {
        qh.nzb_set_param(hi.nzb(), b"*DupeRecheck", None);
    } else if action == 3 {
        qh.dq_edit_list_single_id(
            dq_ptr,
            hi.id,
            crate::queuesubsys_host::EA_HISTORY_RETRY_FAILED,
            None,
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

    #[cfg(unix)]
    #[test]
    fn a_dead_pick_probe_waits_for_its_first_connection_only() {
        // ids no other test registers: the registry is process-wide
        let nzb_id = -1_907_001;
        let mut probe = NzbgetRsDupeProbe::new(nzb_id, false, 0);
        assert!(!probe_waiting_for_connection(nzb_id));
        assert!(probe.register());
        assert!(probe_waiting_for_connection(nzb_id));
        assert!(!probe_waiting_for_connection(nzb_id - 1));
        // connected once: the downloads take connections again, also between its servers
        probe.set_conn(0x1000);
        assert!(!probe_waiting_for_connection(nzb_id));
        probe.set_conn(0);
        assert!(!probe_waiting_for_connection(nzb_id));
        probe.unregister();
        // a recheck probe (count_all) never holds the downloads back
        let mut recheck = NzbgetRsDupeProbe::new(nzb_id, true, 0);
        assert!(recheck.register());
        assert!(!probe_waiting_for_connection(nzb_id));
        recheck.unregister();
    }

    /// A probe of a server that refuses connections (port 1), on 40 connections
    /// with `LightConnections=cap`.
    #[cfg(unix)]
    fn probe_refusing_server(cap: i32) -> (bool, ServerResult) {
        use std::sync::Arc;
        let pool = crate::net::NzbgetRsServerPool::new(crate::ffi::tls_host());
        let srv = Arc::new(crate::net::RsNewsServer::new(
            7,
            true,
            b"s7",
            b"127.0.0.1",
            1,
            0,
            b"",
            b"",
            false,
            false,
            b"",
            40,
            0,
            0,
            0,
            false,
            0,
        ));
        pool.add_server(Arc::clone(&srv));
        pool.init_connections();
        pool.set_light_cap(cap);
        let mut probe = NzbgetRsDupeProbe::new(1, false, 0);
        probe.add_sample(b"<a@b>".to_vec(), Vec::new());
        let mut probed = BTreeSet::new();
        let out = probe.probe_server_native(&pool, 7, &mut probed);
        let size = if cap > 0 { 5 } else { 0 };
        assert_eq!(pool.light_status(), vec![(7, size, 0)]);
        assert_eq!(pool.connections_in_use(), 0);
        out
    }

    /// Probe speed: a probe sends its STATs at once and reads the answers
    /// after (pipelined). The fake server waits 60 ms before answering each
    /// chunk it reads: ten STATs one by one took ten waits, pipelined they take
    /// one or two. An answer that is neither present nor missing ("503") is
    /// asked again alone. The batches skip found samples and stop at a group
    /// change on a server that joins groups.
    #[cfg(unix)]
    #[test]
    fn a_probe_pipelines_its_stats() {
        use std::sync::Arc;
        let chunks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let port = fake_stat_server(Arc::clone(&chunks));
        let pool = crate::net::NzbgetRsServerPool::new(crate::ffi::tls_host());
        let srv = Arc::new(crate::net::RsNewsServer::new(
            9, true, b"s9", b"127.0.0.1", port as i32, 0, b"", b"", false, false, b"", 4, 0, 0, 0, false, 0,
        ));
        pool.add_server(Arc::clone(&srv));
        pool.init_connections();
        let mut probe = NzbgetRsDupeProbe::new(1, true, 0);
        for i in 0..8 {
            probe.add_sample(format!("<gone{i}@x>").into_bytes(), Vec::new());
        }
        probe.add_sample(b"<alive1@x>".to_vec(), Vec::new());
        probe.add_sample(b"<busy@x>".to_vec(), Vec::new());
        let mut probed = BTreeSet::new();
        let start = std::time::Instant::now();
        let (reached, result) = probe.probe_server_native(&pool, 9, &mut probed);
        let took = start.elapsed();
        assert!(reached);
        assert_eq!((result.exists, result.missing, result.unknown), (2, 8, 0), "{result:?}");
        // one chunk for the batch, one for the busy sample asked alone
        let sent = chunks.load(std::sync::atomic::Ordering::SeqCst);
        assert!(sent <= 3, "{sent} chunks: not pipelined");
        // the chunk count above is the proof; the time only catches a stall
        assert!(took < std::time::Duration::from_secs(3), "{took:?}");

        // review: without count_all the probe stops once enough are found (it
        // reads the whole batch first, so the connection stays in step, and
        // counts nothing past the find that was enough, as before), and an id
        // that could carry a second command is never sent: unknown
        let chunks2 = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let port2 = fake_stat_server(Arc::clone(&chunks2));
        let pool2 = crate::net::NzbgetRsServerPool::new(crate::ffi::tls_host());
        let srv2 = Arc::new(crate::net::RsNewsServer::new(
            10, true, b"s10", b"127.0.0.1", port2 as i32, 0, b"", b"", false, false, b"", 4, 0, 0, 0, false, 0,
        ));
        pool2.add_server(Arc::clone(&srv2));
        pool2.init_connections();
        let mut early = NzbgetRsDupeProbe::new(2, false, 0);
        early.add_sample(b"<x@y>\r\nQUIT".to_vec(), Vec::new());
        early.add_sample(b"<alive1@x>".to_vec(), Vec::new());
        early.add_sample(b"<alive2@x>".to_vec(), Vec::new());
        early.add_sample(b"<gone1@x>".to_vec(), Vec::new());
        let mut probed2 = BTreeSet::new();
        let (reached2, r2) = early.probe_server_native(&pool2, 10, &mut probed2);
        assert!(reached2);
        assert_eq!((r2.exists, r2.missing, r2.unknown), (2, 0, 1), "{r2:?}");
        assert!(early.found[1] && early.found[2] && !early.found[0]);
        assert_eq!(pool2.connections_in_use(), 0);
        assert!(!pipelinable(b"<a b>") && !pipelinable(b"<a\nb>") && !pipelinable(&[b'x'; 300]));
        assert!(pipelinable(b"<part1of9.abc@news.example>"));
        pipeline_batch_rules();
    }

    /// A fake news server for [`a_probe_pipelines_its_stats`]: answers STAT
    /// for `<alive…>` and (the second time) `<busy@x>` with 223, else 430,
    /// waiting 60 ms before each chunk of lines it reads (counted in `chunks`).
    #[cfg(unix)]
    fn fake_stat_server(chunks: std::sync::Arc<std::sync::atomic::AtomicUsize>) -> u16 {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let chunks_srv = chunks;
        std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let mut out = sock.try_clone().unwrap();
            out.write_all(b"200 fake ready\r\n").unwrap();
            let mut reader = BufReader::new(sock);
            let mut asked_busy = false;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                // one wait per chunk the client sent: a pipelined batch waits once
                let mut lines = vec![line];
                while !reader.buffer().is_empty() {
                    let mut more = String::new();
                    if reader.read_line(&mut more).unwrap_or(0) == 0 {
                        break;
                    }
                    lines.push(more);
                }
                chunks_srv.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(60));
                for l in lines {
                    let reply: &[u8] = if l.starts_with("QUIT") {
                        b"205 bye\r\n"
                    } else if l.contains("<busy@x>") && !asked_busy {
                        asked_busy = true;
                        b"503 try again\r\n"
                    } else if l.contains("<busy@x>") || l.contains("<alive") {
                        b"223 0 <x>\r\n"
                    } else {
                        b"430 no such article\r\n"
                    };
                    let _ = out.write_all(reply);
                }
            }
        });
        port
    }

    /// Review: a server that answers part of a batch and closes (here two 223
    /// lines for four STATs) is never handed on in that state, even when the
    /// answers it gave were enough to stop: the connection is closed, the rest
    /// unknown.
    #[cfg(unix)]
    #[test]
    fn a_short_answer_closes_the_connection() {
        use std::io::{BufRead, BufReader, Write};
        use std::sync::Arc;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let mut out = sock.try_clone().unwrap();
            out.write_all(b"200 fake ready\r\n").unwrap();
            let mut reader = BufReader::new(sock);
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = out.write_all(b"223 0 <a>\r\n223 0 <b>\r\n");
            let _ = out.shutdown(std::net::Shutdown::Both);
        });
        let pool = crate::net::NzbgetRsServerPool::new(crate::ffi::tls_host());
        let srv = Arc::new(crate::net::RsNewsServer::new(
            11, true, b"s11", b"127.0.0.1", port as i32, 0, b"", b"", false, false, b"", 4, 0, 0, 0, false, 0,
        ));
        pool.add_server(Arc::clone(&srv));
        pool.init_connections();
        let mut probe = NzbgetRsDupeProbe::new(3, false, 0);
        for id in ["<alive1@x>", "<alive2@x>", "<c@x>", "<d@x>"] {
            probe.add_sample(id.as_bytes().to_vec(), Vec::new());
        }
        let mut probed = BTreeSet::new();
        let (reached, result) = probe.probe_server_native(&pool, 11, &mut probed);
        assert!(reached);
        assert_eq!(result.exists, 2, "{result:?}");
        assert_eq!(result.missing, 0, "{result:?}");
        assert_eq!(pool.connections_in_use(), 0);
        let conn = pool.get_check_connection_by_server_id(11);
        assert!(!conn.is_null());
        // SAFETY: just handed out by the pool, given back below
        assert!(!unsafe { &*conn }.is_connected(), "a short batch closes its connection");
        pool.free_connection(conn, false);
    }

    /// Review: when asking an unclear answer again alone fails (the server
    /// closes), the answers of the batch already read still count.
    #[cfg(unix)]
    #[test]
    fn a_failed_retry_keeps_the_answers_read() {
        use std::io::{BufRead, BufReader, Write};
        use std::sync::Arc;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let mut out = sock.try_clone().unwrap();
            out.write_all(b"200 fake ready\r\n").unwrap();
            let mut reader = BufReader::new(sock);
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            std::thread::sleep(std::time::Duration::from_millis(50));
            while !reader.buffer().is_empty() {
                let mut more = String::new();
                if reader.read_line(&mut more).unwrap_or(0) == 0 {
                    break;
                }
            }
            let _ = out.write_all(b"503 busy\r\n223 0 <b>\r\n430 no\r\n");
            // the retry of the first: closed instead of answered
            let mut retry = String::new();
            let _ = reader.read_line(&mut retry);
            let _ = out.shutdown(std::net::Shutdown::Both);
        });
        let pool = crate::net::NzbgetRsServerPool::new(crate::ffi::tls_host());
        let srv = Arc::new(crate::net::RsNewsServer::new(
            12, true, b"s12", b"127.0.0.1", port as i32, 0, b"", b"", false, false, b"", 4, 0, 0, 0, false, 0,
        ));
        pool.add_server(Arc::clone(&srv));
        pool.init_connections();
        let mut probe = NzbgetRsDupeProbe::new(4, true, 0);
        for id in ["<a@x>", "<b@x>", "<c@x>"] {
            probe.add_sample(id.as_bytes().to_vec(), Vec::new());
        }
        let mut probed = BTreeSet::new();
        let (_, result) = probe.probe_server_native(&pool, 12, &mut probed);
        assert_eq!((result.exists, result.missing, result.unknown), (1, 1, 1), "{result:?}");
        assert_eq!(pool.connections_in_use(), 0);
    }

    /// The batch and command rules [`a_probe_pipelines_its_stats`] relies on.
    #[cfg(unix)]
    fn pipeline_batch_rules() {
        let s = |id: &str, g: &[&str]| ProbeSample {
            message_id: id.as_bytes().to_vec(),
            groups: g.iter().map(|x| x.as_bytes().to_vec()).collect(),
        };
        let samples = vec![s("<1>", &["a"]), s("<2>", &["a"]), s("<3>", &["b"]), s("<4>", &["b"])];
        assert_eq!(stat_batch(&samples, &[false, true, false, false], 0, false), vec![0, 2, 3]);
        assert_eq!(stat_batch(&samples, &[false; 4], 0, true), vec![0, 1]);
        assert_eq!(stat_batch(&samples, &[false; 4], 2, true), vec![2, 3]);
        let odd = vec![s("<1>", &[]), s("<2 x>", &[]), s("<3>", &[])];
        assert_eq!(stat_batch(&odd, &[false; 3], 0, false), vec![0], "a batch ends before an id never sent");
        assert_eq!(stat_command(b"<a@b>"), b"STAT <a@b>\r\n");
        assert_eq!(stat_command(b"<a\0b>"), b"STAT <a\r\n");
        let long = vec![b'x'; 2000];
        let cmd = stat_command(&long);
        assert_eq!(cmd.len(), 1023);
        assert!(cmd.ends_with(b"\r\n"));
    }

    #[cfg(unix)]
    #[test]
    fn a_light_connection_that_cannot_connect_leaves_the_server_unknown() {
        // on a light connection: unknown, the server still counts toward a verdict
        let (reached, result) = probe_refusing_server(crate::light_pool::LIGHT_MAX);
        assert!(!reached);
        assert!(!result.unreachable);
        assert_eq!(result.unknown, 1);
        // on a main-pool check connection: unreachable as before
        let (reached, result) = probe_refusing_server(0);
        assert!(!reached);
        assert!(result.unreachable);
    }

    #[test]
    fn probe_order_is_one_entry_per_backbone_with_siblings_as_fallbacks() {
        // ids 1..=5; 1 and 3 on backbone 7, 4 on backbone 8, 2 and 5 independent, 5 inactive
        let active = [1, 1, 1, 1, 0];
        let groups = [7, 0, 7, 8, 0];
        let levels = [1, 0, 0, 2, 0];
        let retentions = [3000, 0, 1000, 0, 0];
        let ids = [1, 2, 3, 4, 5];
        let order = select_server_order(&active, &groups, &levels, &retentions, &ids);
        // sorted by the lowest (level, id) of each entry; inside a backbone the
        // longest retention comes first
        assert_eq!(order, vec![vec![2], vec![1, 3], vec![4]]);
    }

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
            b"6 of 10 failed articles exist on the servers; retrying them"
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
        assert_eq!(
            classify(Some(b"223 0 <id> article exists")),
            DUPE_PROBE_EXISTS
        );
        assert_eq!(
            classify(Some(b"430 No Such Article Found")),
            DUPE_PROBE_MISSING
        );
        assert_eq!(
            classify(Some(b"423 No such article number")),
            DUPE_PROBE_MISSING
        );
        assert_eq!(
            classify(Some(b"412 no newsgroup selected")),
            DUPE_PROBE_MISSING
        );
        assert_eq!(classify(Some(b"451 Article not found")), DUPE_PROBE_MISSING);

        assert_eq!(classify(None), DUPE_PROBE_UNKNOWN);
        assert_eq!(
            classify(Some(b"400 service temporarily unavailable")),
            DUPE_PROBE_UNKNOWN
        );
        assert_eq!(classify(Some(b"499 connection limit")), DUPE_PROBE_UNKNOWN);
        assert_eq!(
            classify(Some(b"481 authentication failed")),
            DUPE_PROBE_UNKNOWN
        );
        assert_eq!(
            classify(Some(b"502 too many connections")),
            DUPE_PROBE_UNKNOWN
        );
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
