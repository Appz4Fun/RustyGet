//! End-to-end Rust implementation of `StreamRepairController`
//! (`daemon/postprocess/StreamRepair.cpp` / `daemon/util/Util.h`), covering both
//! post-processing stream repair (`Run`) and download-concurrent live stream repair (`RunLive`).

use crate::queuesubsys_host::NzbRef;
use crate::cstr;
use crate::options::OptStr;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CStr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::articlefetcher::{
    parse_donor_nzb_file, ArticleFetcher, BatchFetcherPool, DonorParsedFile, DonorParsedNzb,
    NntpHost,
};
use crate::dupecoord::same_name_or_key;
use crate::dupefallback::NzbgetRsByteRange;
use crate::dupeprobe::{sample_indexes, NzbgetRsDupeProbe, MIN_ARTICLES, SAMPLE_COUNT};
use crate::nntp::BatchRequest;
use crate::streamrepair::{
    build_donor_key, classify_remaining_hole_target, compute_positional_rank,
    format_rar_volume_debug_lines, format_whole_files_suffix,
    is_incomplete_nzb_hole, is_par2_job_or_file, process_m1_donor, resolve_completion_par_action,
    stream_repair_bytes_to_mb, total_size, watchdog_eval_tick, CrossPackDonorEnv,
    CrossPackRepairSession, CrossPackTargetInput, FetchedDonorArticle, M1DonorEnv, M1RepairTarget,
    NzbSizes, SizePair, TargetMessageKind,
};

use crate::log::MessageKind;
use crate::postproc::JobQueue;
use crate::status::DupeMode;

const RS_NONE: i32 = 0;

const SLOW_REPAIR_FACTOR: i32 = 3;
const MAX_RUN_FACTOR: i32 = 5;
const MAX_CONNECTION_WAIT_SEC: i32 = 1800;
const DONOR_CHECK_SEC: i32 = 20;

#[inline]
const fn map_target_msg_kind(kind: TargetMessageKind) -> MessageKind {
    match kind {
        TargetMessageKind::Info => MessageKind::Info,
        TargetMessageKind::Warning => MessageKind::Warning,
        TargetMessageKind::Error => MessageKind::Error,
        TargetMessageKind::Detail => MessageKind::Detail,
    }
}

/// Fetch workers of a repair that borrows in earnest: half the connections, up to 30. With 8
/// of the 60 (about 1 MB/s each through the news proxy) A2's repair on 6790 fetched 7-8 MB/s
/// while the download, on all 60, ran at 20-30 MB/s, and extraction waited on the repair.
fn stream_fetch_worker_count(primary_connections: i32) -> i32 {
    (primary_connections / 2).clamp(4, 30)
}

/// Fetch workers of a live repair that borrows a dead volume direct unpack waits on, while the
/// download still runs. The workers take their connections from the same pool as the download,
/// so 8 of them against its 60 got an eighth of the pipe: a 264 MB borrow ran at 1.5 MB/s and
/// finished 40 s after the download. Extraction needs the borrowed bytes before anything the
/// download fetches later, so the borrow takes up to half the connections.
fn live_borrow_fetch_worker_count(primary_connections: i32) -> i32 {
    (primary_connections / 2).clamp(stream_fetch_worker_count(primary_connections), 30)
}

/// Fetch workers of a live repair that borrows no dead volume: fewer than after the download,
/// so the download keeps most connections, but more than one. Direct unpack patches each hole
/// as soon as it is borrowed, so one article at a time left holes to be filled at the end.
fn live_hole_fetch_worker_count(primary_connections: i32) -> i32 {
    (primary_connections / 4).clamp(2, 16)
}

/// The target download as read under the queue lock.
struct TargetSnap {
    nzb_ptr: NzbRef,
    has_par_files: bool,
    completed_count: usize,
    job_count: usize,
    queue_count: usize,
    history_count: usize,
    full_hash: i64,
    name: Vec<u8>,
    dest_dir: Vec<u8>,
    dupe_key: Vec<u8>,
    password: Option<Vec<u8>>,
}

/// A possible donor download as read under the queue lock.
struct CandSnap {
    id: i32,
    dupe_score: i32,
    dupe_mode: DupeMode,
    full_hash: i64,
    name: Vec<u8>,
    dupe_key: Vec<u8>,
    queued_filename: Vec<u8>,
    password: Vec<u8>,
}

#[derive(Clone, Debug)]
struct RepairTarget {
    file_id: i32,
    filename: Vec<u8>,
    decoded_file_size: i64,
    holes: Vec<NzbgetRsByteRange>,
    encoded_size: i64,
    article_count: i32,
    recreated: bool,
    failed_size: i64,
    missed_size: i64,
    failed_articles: i32,
    is_par_file: bool,
    patch_eligible: bool,
    positional_rank: i32,
    positional_window: i32,
    steps_hash: u64,
    /// An earlier live repair already took this file on (it is not `patch_eligible` then).
    tried_before: bool,
}

/// True for a dead-volume status line (or the live repair's start line) already logged in the
/// last ten minutes: the live repair reruns as each volume arrives, and the same lines would
/// otherwise repeat every few seconds.
fn repeats_recent_dead_volume_line(key: i32, text: &[u8]) -> bool {
    const PREFIXES: [&[u8]; 6] = [
        b"Starting live stream repair",
        b"Leaving dead volume",
        b"Dead volumes in",
        b"Could not borrow dead volume",
        b"No byte-identical duplicate holds",
        b"Not borrowing dead volume",
    ];
    if !PREFIXES.iter().any(|p| text.starts_with(p)) {
        return false;
    }
    static SEEN: std::sync::Mutex<Vec<(i32, Vec<u8>, std::time::Instant)>> =
        std::sync::Mutex::new(Vec::new());
    let now = std::time::Instant::now();
    let window = std::time::Duration::from_secs(600);
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    seen.retain(|(_, _, at)| now.duration_since(*at) < window);
    if seen.iter().any(|(k, t, _)| *k == key && t == text) {
        return true;
    }
    if seen.len() < 512 {
        seen.push((key, text.to_vec(), now));
    }
    false
}

/// True when DirectUnpack is on: only direct unpack reads a rebuilt dead volume.
fn direct_unpack_on() -> bool {
    crate::options::active_options().is_some_and(|o| o.scalars().direct_unpack != 0)
}

/// Whether the live repair leaves dead volume `t` to the dead-volume borrow: `None` when `t`
/// is not a volume of which nothing arrived (par2 files included), else `Ok(())` to borrow it or
/// `Err(reason)` saying why not. The borrow plans `.partNN.rar` sets from a readable first
/// volume, and files direct unpack holds under a name that is not a RAR name (obfuscated ones).
/// The archive's own headers say whether it is stored and unencrypted, so a password set on the
/// download does not rule it out here.
fn dead_volume_decision(dest_dir: &[u8], t: &RepairTarget) -> Option<Result<(), &'static str>> {
    dead_volume_decision_with(dest_dir, t, direct_unpack_on())
}

fn dead_volume_decision_with(
    dest_dir: &[u8],
    t: &RepairTarget,
    direct_unpack: bool,
) -> Option<Result<(), &'static str>> {
    if t.is_par_file || !crate::streamrepair::nothing_arrived(t.decoded_file_size, &t.holes) {
        return None;
    }
    if !direct_unpack {
        return Some(Err("DirectUnpack is off"));
    }
    if let Some((_, n, _, _)) = crate::streamrepair::rar_part_number(&t.filename) {
        return Some(if n > 1 {
            Ok(())
        } else {
            Err("the first volume cannot be rebuilt")
        });
    }
    if crate::filetypes::is_rar_file(&t.filename) {
        return Some(Err("only .partNN.rar volumes can be rebuilt"));
    }
    let full = [
        dest_dir,
        &[crate::paths::PATH_SEPARATOR],
        t.filename.as_slice(),
    ]
    .concat();
    Some(if crate::postproc::direct_unpack_is_held_path(&full) {
        Ok(())
    } else {
        Err("direct unpack is not waiting on it")
    })
}

/// True when the live repair leaves `t` to the dead-volume borrow, see [`dead_volume_decision`].
fn borrowable_dead_volume(dest_dir: &[u8], t: &RepairTarget) -> bool {
    matches!(dead_volume_decision(dest_dir, t), Some(Ok(())))
}

/// Which targets the live repair leaves to the dead-volume borrow instead of recreating them
/// whole first: recreating fetches every article one by one (minutes, long enough for the queue
/// to fail over) and only works from a twin.
fn dead_volumes_to_borrow(dest_dir: &[u8], targets: &[RepairTarget]) -> Vec<bool> {
    targets
        .iter()
        .map(|t| borrowable_dead_volume(dest_dir, t))
        .collect()
}

/// Downloads whose stream repair after the download runs now, by NZB id (a
/// count: a job runs once, but a mark must never outlive its repair).
fn running_repairs() -> &'static Mutex<std::collections::BTreeMap<i32, u32>> {
    static RUNNING: std::sync::OnceLock<Mutex<std::collections::BTreeMap<i32, u32>>> =
        std::sync::OnceLock::new();
    RUNNING.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

/// Whether the stream repair after the download of `nzb_id` runs now:
/// TwinCheck holds its sampling against that copy meanwhile.
pub(crate) fn stream_repair_running(nzb_id: i32) -> bool {
    running_repairs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(&nzb_id)
}

/// Marks the stream repair of a download running for as long as it lives
/// (also on unwind).
struct RunningRepair(i32);

impl RunningRepair {
    fn mark(nzb_id: i32) -> Self {
        *running_repairs()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(nzb_id)
            .or_insert(0) += 1;
        Self(nzb_id)
    }
}

impl Drop for RunningRepair {
    fn drop(&mut self) {
        let mut running = running_repairs().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = running.get_mut(&self.0) {
            *n -= 1;
            if *n == 0 {
                running.remove(&self.0);
            }
        }
    }
}

/// Marks a borrow of dead volumes of a download underway for as long as it lives, so the
/// queue does not fail over meanwhile; dropping it (also on unwind) clears the mark.
struct BorrowNote(Option<i32>);

impl BorrowNote {
    fn mark(nzb_id: Option<i32>) -> Self {
        if let Some(id) = nzb_id {
            crate::postproc::note_dead_volume_borrow(id, true);
        }
        Self(nzb_id)
    }
}

impl Drop for BorrowNote {
    fn drop(&mut self) {
        // A rebuilt volume handed to direct unpack keeps the borrow marked on its own.
        if let Some(id) = self.0.take() {
            crate::postproc::note_dead_volume_borrow(id, false);
        }
    }
}

impl RepairTarget {
    #[inline]
    fn needs_repair(&self) -> bool {
        !self.holes.is_empty() || self.decoded_file_size <= 0
    }
}

#[derive(Clone, Debug)]
struct DonorSource {
    queued_filename: Vec<u8>,
    info_name: Vec<u8>,
    password: Vec<u8>,
}

#[derive(Clone, Debug)]
struct HostOptions {
    timeout_sec: i32,
    allow_decompress_opt: bool,
    primary_connections: i32,
    par_check: i32,
    download_rate: i64,
    par_ignore_ext: Vec<u8>,
}

#[derive(Default)]
struct RunCounters {
    has_par_files: bool,
    target_password: Vec<u8>,
    recovered_articles: i32,
    recovered_bytes: i64,
    recovered_holes: i32,
    recovered_failed_size: i64,
    recovered_failed_par_size: i64,
    holes_remain: bool,
    donor_dead: BTreeMap<Vec<u8>, bool>,
}

/// Donor articles fetched one at a time, by message id: each repair wave builds its donor's
/// map again and reads every volume's first article for it (39 of them per wave in A2 on
/// 6790, about 27 MB a wave), so later waves take them from here. At most
/// `RECENT_SINGLES_MAX` articles and `RECENT_SINGLES_BYTES` in all; an article over a
/// sixteenth of that is not kept.
static RECENT_SINGLES: Mutex<std::collections::VecDeque<(Vec<u8>, FetchedDonorArticle)>> =
    Mutex::new(std::collections::VecDeque::new());
const RECENT_SINGLES_MAX: usize = 64;
const RECENT_SINGLES_BYTES: usize = 48 << 20;

fn recent_single(msg_id: &[u8]) -> Option<FetchedDonorArticle> {
    let g = RECENT_SINGLES.lock().unwrap_or_else(|e| e.into_inner());
    g.iter().find(|(id, _)| id == msg_id).map(|(_, a)| a.clone())
}

fn keep_single(msg_id: &[u8], article: FetchedDonorArticle) {
    if article.data.len() > RECENT_SINGLES_BYTES / 16 {
        return;
    }
    let mut g = RECENT_SINGLES.lock().unwrap_or_else(|e| e.into_inner());
    if g.iter().any(|(id, _)| id == msg_id) {
        return;
    }
    let mut held: usize = g.iter().map(|(_, a)| a.data.len()).sum();
    while !g.is_empty()
        && (g.len() >= RECENT_SINGLES_MAX || held + article.data.len() > RECENT_SINGLES_BYTES)
    {
        if let Some((_, old)) = g.pop_front() {
            held -= old.data.len();
        }
    }
    g.push_back((msg_id.to_vec(), article));
}

/// Fetches of one donor's borrow: how many went one at a time and how many in batches, and
/// how long the repair waited on them, so a slow borrow tells whether the network or the
/// repair itself held it up.
#[derive(Debug, Default)]
struct DonorFetchStats {
    singles: u32,
    batches: u32,
    batch_parts: u32,
    bytes: u64,
    waited: std::time::Duration,
}

impl DonorFetchStats {
    fn line(&self, donor: &[u8], elapsed: std::time::Duration, workers: i32) -> Vec<u8> {
        let mut msg = b"Fetch cost of borrowing from ".to_vec();
        msg.extend_from_slice(donor);
        msg.extend_from_slice(
            format!(
                ": {:.1} MB in {:.1} s, waiting on the network {:.1} s; {} single fetch(es), {} batch(es) of {} article(s), {} worker(s)",
                self.bytes as f64 / 1_000_000.0,
                elapsed.as_secs_f64(),
                self.waited.as_secs_f64(),
                self.singles,
                self.batches,
                self.batch_parts,
                workers,
            )
            .as_bytes(),
        );
        msg
    }
}

pub struct NzbgetRsStreamRepairCtrl {
    live_mode: bool,
    /// Keys the repeated-line filter, so one job's lines never hide another's.
    nzb_id: i32,
    /// Set once a live repair borrows a dead volume: its fetches then run in parallel.
    parallel_live_fetch: AtomicBool,
    /// The live pass that is borrowing: the dead volumes it took on, and when the queue was
    /// last asked for new ones (see `yield_for_new_dead`).
    pass_dead: Mutex<PassDead>,
    /// Raised when a volume went dead while a live pass borrows: the pass stops so the next
    /// one hands every dead volume to direct unpack at once.
    yielding: AtomicBool,
    /// Set while the cross-pack session borrows one dead volume: a yield waits until that
    /// volume is in, so its borrow is handed over whole instead of restarting from zero.
    finishing_volume: AtomicBool,
    /// Dead volumes whose whole-file recreation a yielded pass skipped: a later pass still owes
    /// it, though the queue now marks them tried.
    recreate_owed: Mutex<std::collections::BTreeSet<Vec<u8>>>,
    /// What borrowing from the current donor cost in fetches, logged after each donor.
    fetch_stats: Mutex<DonorFetchStats>,
    tls_enabled: bool,
    debug_enabled: bool,
    fetcher: ArticleFetcher,
    batch_fetcher: BatchFetcherPool,
    timed_out: Arc<AtomicBool>,
    progress_bytes: Arc<AtomicI64>,
    checking_donor: Arc<AtomicI32>,
    materializing: Arc<AtomicI32>,
    waiting_for_connection: Arc<AtomicI32>,
    slow_reason: Arc<Mutex<Vec<u8>>>,
    watchdog_pair: Arc<(Mutex<bool>, Condvar)>,
}

impl NzbgetRsStreamRepairCtrl {
    pub fn new(
        live_mode: bool,
        nzb_id: i32,
        nntp_host: &'static dyn NntpHost,
        tls_enabled: bool,
        debug_enabled: bool,
    ) -> Self {
        Self {
            live_mode,
            nzb_id,
            parallel_live_fetch: AtomicBool::new(false),
            pass_dead: Mutex::new(PassDead::default()),
            yielding: AtomicBool::new(false),
            finishing_volume: AtomicBool::new(false),
            recreate_owed: Mutex::new(Default::default()),
            fetch_stats: Mutex::new(DonorFetchStats::default()),
            tls_enabled,
            debug_enabled,
            fetcher: ArticleFetcher::new(nntp_host),
            batch_fetcher: BatchFetcherPool::new(nntp_host),
            timed_out: Arc::new(AtomicBool::new(false)),
            progress_bytes: Arc::new(AtomicI64::new(0)),
            checking_donor: Arc::new(AtomicI32::new(0)),
            materializing: Arc::new(AtomicI32::new(0)),
            waiting_for_connection: Arc::new(AtomicI32::new(0)),
            slow_reason: Arc::new(Mutex::new(Vec::new())),
            watchdog_pair: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    pub fn stop(&self) {
        self.batch_fetcher.stop();
        self.fetcher.stop();
    }

    fn host_is_stopped(&self, q: JobQueue<'_>) -> bool {
        q.is_stopped()
    }

    fn interrupted(&self, q: JobQueue<'_>) -> bool {
        self.host_is_stopped(q) && !self.timed_out.load(Ordering::SeqCst)
    }

    /// Adds a message to the job's download: the queued one with this job's id in live
    /// mode (under the queue lock), else the post job's.
    fn print_message(&self, q: JobQueue<'_>, kind: MessageKind, text: &[u8]) {
        if self.live_mode && repeats_recent_dead_volume_line(self.nzb_id, text) {
            return;
        }
        let text = cstr::until_nul(text);
        let host = q.host;
        if self.live_mode {
            host.with_locked_queue(|dq| {
                let nzb = host.queue_find_by_id(dq, q.nzb_id());
                if nzb.is_some() {
                    host.nzb_add_message(nzb, kind as i32, text);
                }
            });
        } else if q.post_ptr().is_some() {
            let nzb = host.post_snap(q.post_ptr()).nzb_ptr;
            if nzb.is_some() {
                host.nzb_add_message(nzb, kind as i32, text);
            }
        }
    }

    /// The id of the download this repair works on.
    fn target_nzb_id(&self, q: JobQueue<'_>) -> Option<i32> {
        if self.live_mode {
            return Some(q.nzb_id());
        }
        let post_ptr = q.post_ptr();
        if post_ptr.is_none() {
            return None;
        }
        q.host.with_locked_queue(|_| {
            let nzb = q.host.post_snap(post_ptr).nzb_ptr;
            (!nzb.is_none()).then(|| q.host.nzb_snap(nzb, false).id)
        })
    }

    /// Writes to the global log.
    fn global_log(&self, q: JobQueue<'_>, kind: MessageKind, text: &[u8]) {
        q.host.log(kind as i32, cstr::until_nul(text));
    }

    fn set_post_progress(&self, q: JobQueue<'_>, label: &[u8], stage_progress: i32) {
        use crate::queuesubsys_host::POST_INT_STAGE_PROGRESS;
        let post_ptr = q.post_ptr();
        if post_ptr.is_none() {
            return;
        }
        let label = cstr::until_nul(label);
        q.host.with_locked_queue(|_| {
            q.host.post_set_progress_label(post_ptr, label);
            q.host.post_set_int(post_ptr, POST_INT_STAGE_PROGRESS, stage_progress);
        });
    }

    /// The target download, read under the queue lock (`dq_ptr`); `None` when it is gone or,
    /// in live mode, this thread no longer holds it. `set_init_label` sets the post job's
    /// first progress label.
    fn inspect_target(
        &self,
        q: JobQueue<'_>,
        dq_ptr: usize,
        set_init_label: bool,
    ) -> Option<TargetSnap> {
        let host = q.host;
        let nzb = if self.live_mode {
            let n = host.queue_find_by_id(dq_ptr, q.nzb_id());
            if n.is_none() || host.nzb_snap(n, false).live_repair_thread_ptr != q.thread_ptr() {
                return None;
            }
            n
        } else {
            let post_ptr = q.post_ptr();
            if post_ptr.is_none() {
                return None;
            }
            let n = host.post_snap(post_ptr).nzb_ptr;
            if n.is_none() {
                return None;
            }
            if set_init_label {
                host.post_set_progress_label(post_ptr, b"Repairing files from duplicates");
            }
            n
        };
        let ns = host.nzb_snap(nzb, false);
        Some(TargetSnap {
            nzb_ptr: nzb,
            has_par_files: ns.par_size > 0,
            completed_count: host.nzb_completed_files_len(nzb),
            job_count: host.nzb_stream_repair_jobs_len(nzb),
            queue_count: host.queue_len(dq_ptr),
            history_count: host.history_len(dq_ptr),
            full_hash: i64::from(ns.full_content_hash),
            name: cstr::until_nul(&ns.name).to_vec(),
            dest_dir: cstr::until_nul(&ns.dest_dir).to_vec(),
            dupe_key: cstr::until_nul(&ns.dupe_key).to_vec(),
            password: host
                .nzb_get_param(nzb, b"*Unpack:Password")
                .map(|p| cstr::until_nul(&p).to_vec()),
        })
    }

    /// Id, par flag and name of the completed file at `idx` of `nzb`.
    fn completed_file(q: JobQueue<'_>, nzb: NzbRef, idx: usize) -> (i32, bool, Vec<u8>) {
        let cf = q.host.nzb_completed_file_at(nzb, idx);
        (cf.id, cf.par_file, cstr::until_nul(&cf.filename).to_vec())
    }

    /// The stream-repair job at `idx` of `nzb`.
    fn repair_job(
        q: JobQueue<'_>,
        nzb: NzbRef,
        idx: usize,
    ) -> crate::queuesubsys_host::StreamRepairJobDetailSnap {
        let mut jd = q.host.nzb_stream_repair_job_detail(nzb, idx);
        jd.filename = cstr::until_nul(&jd.filename).to_vec();
        jd
    }

    /// A queued (`from_history` false) or history download at `idx` that may donate, `None`
    /// when it is no plain download.
    fn candidate(
        q: JobQueue<'_>,
        dq_ptr: usize,
        from_history: bool,
        idx: usize,
    ) -> Option<(NzbRef, CandSnap)> {
        use crate::queuesubsys_host::{HK_NZB, NK_NZB};
        let host = q.host;
        let cand = if from_history {
            let h = host.history_at(dq_ptr, idx);
            if h.kind != HK_NZB || h.nzb().is_none() {
                return None;
            }
            h.nzb()
        } else {
            let c = host.queue_at(dq_ptr, idx);
            if c.is_none() || host.nzb_snap(c, false).kind != NK_NZB {
                return None;
            }
            c
        };
        let cs = host.nzb_snap(cand, false);
        Some((
            cand,
            CandSnap {
                id: cs.id,
                dupe_score: cs.dupe_score,
                dupe_mode: cs.dupe_mode,
                full_hash: i64::from(cs.full_content_hash),
                name: cstr::until_nul(&cs.name).to_vec(),
                dupe_key: cstr::until_nul(&cs.dupe_key).to_vec(),
                queued_filename: cstr::until_nul(&cs.queued_filename).to_vec(),
                password: host
                    .nzb_get_param(cand, b"*Unpack:Password")
                    .map_or_else(Vec::new, |p| cstr::until_nul(&p).to_vec()),
            },
        ))
    }

    /// Sets a job's hole list and, when `new_decoded_size >= 0`, its decoded size.
    fn update_job_holes(
        q: JobQueue<'_>,
        nzb: NzbRef,
        idx: usize,
        new_decoded_size: i64,
        holes: &[NzbgetRsByteRange],
    ) {
        q.host
            .nzb_stream_repair_update_job_holes(nzb, idx, new_decoded_size, holes);
    }

    fn query_host_options(
        &self,
        ) -> HostOptions {
        let opts = crate::options::active_options();
        let primary_connections = crate::net::active_server_pool().map_or(0, |p| unsafe {
            crate::net::nzbget_rs_serverpool_ctl(Arc::as_ptr(&p), 6, 0, 0, 0, 0) as i32
        });
        let download_rate = crate::statmeter::active_statmeter().map_or(0, |sm| {
            let (_up, dn_sec, all_bytes, _sb) = sm.total_stat_snapshot();
            if dn_sec > 0 {
                all_bytes / i64::from(dn_sec)
            } else {
                0
            }
        });
        HostOptions {
            timeout_sec: opts
                .as_deref()
                .map_or(0, |o| o.scalars().dupe_stream_timeout),
            allow_decompress_opt: opts
                .as_deref()
                .is_some_and(|o| o.scalars().dupe_stream_decompress != 0),
            primary_connections,
            par_check: opts.as_deref().map_or(0, |o| o.scalars().par_check),
            download_rate,
            par_ignore_ext: opts
                .as_deref()
                .map_or_else(Vec::new, |o| o.str_bytes(OptStr::ParIgnoreExt).to_vec()),
        }
    }

    fn collect_targets_and_donors(
        &self,
        q: JobQueue<'_>,
        dq_ptr: usize,
        set_init_label: bool,
        mark_live_attempted: bool,
        counters: &mut RunCounters,
    ) -> Option<(
        Vec<u8>,
        Vec<u8>,
        Vec<RepairTarget>,
        Vec<DonorSource>,
        Vec<Vec<u8>>,
    )> {
        let target = self.inspect_target(q, dq_ptr, set_init_label)?;
        let nzb_ptr = target.nzb_ptr;
        let own_nzb_ptr = nzb_ptr;
        counters.has_par_files = target.has_par_files;
        let completed_count = target.completed_count;
        let job_count = target.job_count;
        let queue_count = target.queue_count;
        let history_count = target.history_count;
        let own_full_hash = target.full_hash;
        let nzb_name = target.name;
        let dest_dir = target.dest_dir;
        let own_dupe_key = target.dupe_key;
        if let Some(password) = target.password {
            counters.target_password = password;
        }

        let mut completed_files: Vec<(i32, bool, Vec<u8>)> = Vec::with_capacity(completed_count);
        let mut member_names: Vec<Vec<u8>> = Vec::with_capacity(completed_count);
        for idx in 0..completed_count {
            let (cf_id, cf_par, fname) = Self::completed_file(q, nzb_ptr, idx);
            member_names.push(fname.clone());
            completed_files.push((cf_id, cf_par, fname));
        }

        let mut job_file_ids: Vec<i32> = Vec::with_capacity(job_count);
        let mut targets: Vec<RepairTarget> = Vec::new();
        for idx in 0..job_count {
            let jd = Self::repair_job(q, nzb_ptr, idx);
            let file_id = jd.file_id;
            job_file_ids.push(file_id);
            let failed_articles = jd.failed_articles;
            let mut is_par_file = jd.par_file;
            let decoded_file_size = jd.decoded_file_size;
            let failed_size = jd.failed_size;
            let missed_size = jd.missed_size;
            let steps_hash = jd.steps_hash;
            let mut filename = jd.filename;
            if is_par2_job_or_file(is_par_file, &filename) {
                continue;
            }
            let holes = jd.holes;
            for (cf_id, cf_par, cf_name) in &completed_files {
                if *cf_id == file_id {
                    filename = cf_name.clone();
                    is_par_file = is_par_file || *cf_par;
                    break;
                }
            }
            if is_par2_job_or_file(is_par_file, &filename) {
                continue;
            }
            targets.push(RepairTarget {
                file_id,
                filename,
                decoded_file_size,
                holes,
                encoded_size: failed_size.saturating_add(missed_size),
                article_count: failed_articles,
                recreated: false,
                failed_size,
                missed_size,
                failed_articles,
                is_par_file,
                patch_eligible: true,
                tried_before: false,
                positional_rank: -1,
                positional_window: 0,
                steps_hash,
            });
        }

        struct CandRow {
            dupe_score: i32,
            id: i32,
            full_hash: i64,
            queued_filename: Vec<u8>,
            info_name: Vec<u8>,
            password: Vec<u8>,
        }
        let mut cand_rows: Vec<CandRow> = Vec::new();
        for (from_history, count) in [(false, queue_count), (true, history_count)] {
            for idx in 0..count {
                let Some((cand_ptr, cand)) = Self::candidate(q, dq_ptr, from_history, idx) else {
                    continue;
                };
                if !from_history && cand_ptr == own_nzb_ptr {
                    continue;
                }
                if cand.dupe_mode == DupeMode::Force {
                    continue;
                }
                if !same_name_or_key(&cand.name, &cand.dupe_key, &nzb_name, &own_dupe_key) {
                    continue;
                }
                cand_rows.push(CandRow {
                    dupe_score: cand.dupe_score,
                    id: cand.id,
                    full_hash: cand.full_hash,
                    queued_filename: cand.queued_filename,
                    info_name: cand.name,
                    password: cand.password,
                });
            }
        }

        cand_rows.sort_by(|a, b| {
            if a.dupe_score != b.dupe_score {
                b.dupe_score.cmp(&a.dupe_score)
            } else {
                a.id.cmp(&b.id)
            }
        });

        let mut donors: Vec<DonorSource> = Vec::new();
        for row in cand_rows {
            if (own_full_hash > 0 && own_full_hash == row.full_hash)
                || row.queued_filename.is_empty()
            {
                continue;
            }
            donors.push(DonorSource {
                queued_filename: row.queued_filename,
                info_name: row.info_name,
                password: row.password,
            });
        }

        // A live pass with no duplicate to borrow from leaves its files
        // unattempted, so a backup added later (a fleet member, DupeWatch)
        // still gets them; only a pass that could borrow marks them.
        if mark_live_attempted && !donors.is_empty() {
            for target in &mut targets {
                for (j_idx, &jf_id) in job_file_ids.iter().enumerate() {
                    if jf_id == target.file_id {
                        let was_attempted =
                            q.host.nzb_stream_repair_mark_live_attempted(nzb_ptr, j_idx);
                        target.patch_eligible = !was_attempted;
                        target.tried_before = was_attempted;
                        break;
                    }
                }
            }
        }

        Some((nzb_name, dest_dir, targets, donors, member_names))
    }

    fn refresh_live_names(
        &self,
        q: JobQueue<'_>,
        targets: &mut [RepairTarget],
        member_names: &mut Vec<Vec<u8>>,
    ) {
        q.host.with_locked_queue(|dq_ptr| {
            let Some(t) = self.inspect_target(q, dq_ptr, false) else {
                return;
            };
            let completed_files: Vec<(i32, bool, Vec<u8>)> = (0..t.completed_count)
                .map(|idx| Self::completed_file(q, t.nzb_ptr, idx))
                .collect();
            for target in targets.iter_mut() {
                if let Some((_, cf_par, cf_name)) =
                    completed_files.iter().find(|(id, _, _)| *id == target.file_id)
                {
                    target.filename = cf_name.clone();
                    target.is_par_file = target.is_par_file || *cf_par;
                }
            }
            member_names.clear();
            member_names.extend(completed_files.into_iter().map(|(_, _, name)| name));
        });
    }

    fn refresh_live_target_name(
        &self,
        q: JobQueue<'_>,
        file_id: i32,
        cur_filename: &mut Vec<u8>,
    ) {
        q.host.with_locked_queue(|dq_ptr| {
            let Some(t) = self.inspect_target(q, dq_ptr, false) else {
                return;
            };
            for idx in 0..t.completed_count {
                let (id, _, name) = Self::completed_file(q, t.nzb_ptr, idx);
                if id == file_id {
                    *cur_filename = name;
                    break;
                }
            }
        });
    }

    fn compute_positional_ranks(
        &self,
        dest_dir: &[u8],
        targets: &mut [RepairTarget],
        member_names: &[Vec<u8>],
    ) {
        let sep = std::path::MAIN_SEPARATOR as u8;
        let mut sizes = Vec::with_capacity(member_names.len());
        let mut name_refs = Vec::with_capacity(member_names.len());
        for m in member_names {
            let full_path =
                crate::artwritercore::article_writer_format_dest_file_path(dest_dir, sep, m);
            let full_c = cstr::nul_terminated(&full_path);
            let sz =
                crate::fsops::file_size(unsafe { CStr::from_bytes_with_nul_unchecked(&full_c) });
            sizes.push(sz);
            name_refs.push(m.as_slice());
        }
        for target in targets.iter_mut() {
            let (rank, win) = compute_positional_rank(
                &name_refs,
                &sizes,
                &target.filename,
                target.decoded_file_size,
            );
            target.positional_rank = rank;
            target.positional_window = win;
        }
    }

    fn start_watchdog(
        &self,
        q: JobQueue<'_>,
        opts: &HostOptions,
        targets: &[RepairTarget],
    ) -> Option<JoinHandle<()>> {
        self.fetcher
            .set_wait_counter(Some(Arc::clone(&self.waiting_for_connection)));
        self.batch_fetcher
            .set_wait_counter(Some(Arc::clone(&self.waiting_for_connection)));

        let timeout = opts.timeout_sec;
        if timeout <= 0 {
            return None;
        }

        let mut missing_bytes: i64 = 0;
        for target in targets {
            missing_bytes = missing_bytes.saturating_add(if target.decoded_file_size > 0 {
                total_size(&target.holes)
            } else {
                target.encoded_size
            });
        }

        let download_rate = opts.download_rate;
        if let Ok(mut done) = self.watchdog_pair.0.lock() {
            *done = false;
        }

        let pair = Arc::clone(&self.watchdog_pair);
        let timed_out = Arc::clone(&self.timed_out);
        let progress_bytes = Arc::clone(&self.progress_bytes);
        let checking_donor = Arc::clone(&self.checking_donor);
        let materializing = Arc::clone(&self.materializing);
        let waiting_for_connection = Arc::clone(&self.waiting_for_connection);
        let slow_reason = Arc::clone(&self.slow_reason);
        let stop_job = q.stop_handle();

        Some(std::thread::spawn(move || {
            let (lock, cvar) = &*pair;
            let mut done_guard = lock.lock().unwrap();
            let start_progress = progress_bytes.load(Ordering::SeqCst);
            let mut progress = start_progress;
            let mut start = Instant::now();
            let begun = start;
            let mut last_progress = start;
            let mut last_tick = start;

            while !*done_guard {
                let (g, _) = cvar
                    .wait_timeout(done_guard, Duration::from_millis(500))
                    .unwrap();
                done_guard = g;
                if *done_guard {
                    break;
                }
                let now = Instant::now();
                let waiting = waiting_for_connection.load(Ordering::SeqCst) > 0
                    || checking_donor.load(Ordering::SeqCst) > 0;
                if waiting {
                    start += now.saturating_duration_since(last_tick);
                }
                last_tick = now;

                let since_last_progress_sec =
                    now.saturating_duration_since(last_progress).as_secs() as i64;
                let since_begun_sec = now.saturating_duration_since(begun).as_secs() as i64;
                let since_start_sec = now.saturating_duration_since(start).as_secs() as i64;
                let elapsed_sec = now.saturating_duration_since(start).as_secs_f64();
                let cur_progress = progress_bytes.load(Ordering::SeqCst);

                let mut new_progress = progress;
                let mut reset_last = false;
                let stop_reason = watchdog_eval_tick(
                    cur_progress,
                    progress,
                    waiting,
                    materializing.load(Ordering::SeqCst),
                    since_last_progress_sec,
                    since_begun_sec,
                    since_start_sec,
                    elapsed_sec,
                    timeout,
                    MAX_RUN_FACTOR,
                    MAX_CONNECTION_WAIT_SEC,
                    SLOW_REPAIR_FACTOR,
                    download_rate,
                    start_progress,
                    missing_bytes,
                    &mut new_progress,
                    &mut reset_last,
                );
                progress = new_progress;
                if reset_last {
                    last_progress = now;
                }
                if let Some(reason) = stop_reason {
                    if let Ok(mut sr) = slow_reason.lock() {
                        *sr = reason;
                    }
                    break;
                }
            }

            if !*done_guard {
                timed_out.store(true, Ordering::SeqCst);
                drop(done_guard);
                // SAFETY: stop_watchdog joins this thread before the run (and so the
                // job) can end.
                unsafe { stop_job.stop() };
            }
        }))
    }

    fn stop_watchdog(
        &self,
        q: JobQueue<'_>,
        handle: Option<JoinHandle<()>>,
        nzb_name: &[u8],
        targets: &[RepairTarget],
        recovered_bytes: i64,
    ) {
        let Some(join_handle) = handle else {
            return;
        };
        if let Ok(mut done) = self.watchdog_pair.0.lock() {
            *done = true;
        }
        self.watchdog_pair.1.notify_all();
        let _ = join_handle.join();

        if self.timed_out.load(Ordering::SeqCst) {
            let mut missing: i64 = 0;
            let mut unsized_cnt: i32 = 0;
            for target in targets {
                missing = missing.saturating_add(total_size(&target.holes));
                if target.decoded_file_size <= 0 {
                    unsized_cnt += 1;
                }
            }
            let suffix = format_whole_files_suffix(unsized_cnt);
            let reason = self
                .slow_reason
                .lock()
                .map(|r| r.clone())
                .unwrap_or_default();
            let rec_mb = stream_repair_bytes_to_mb(recovered_bytes);
            let miss_mb = stream_repair_bytes_to_mb(missing);
            let mut msg = b"Stream repair of ".to_vec();
            msg.extend_from_slice(nzb_name);
            msg.extend_from_slice(b" stopped: ");
            msg.extend_from_slice(&reason);
            msg.extend_from_slice(
                format!(" (option DupeStreamTimeout); {rec_mb:.1} MB recovered, {miss_mb:.1} MB")
                    .as_bytes(),
            );
            msg.extend_from_slice(&suffix);
            msg.extend_from_slice(b" still missing");
            self.print_message(q, MessageKind::Warning, &msg);
        }
    }

    fn build_parsed_donor_key(donor_nzb: &DonorParsedNzb, password: &[u8]) -> Vec<u8> {
        let file_count = donor_nzb.files.len();
        let mut file_names: Vec<&[u8]> = Vec::with_capacity(file_count);
        let mut file_sizes: Vec<i64> = Vec::with_capacity(file_count);
        let mut file_is_par: Vec<u8> = Vec::with_capacity(file_count);
        let mut file_group_counts: Vec<usize> = Vec::with_capacity(file_count);
        let mut groups: Vec<&[u8]> = Vec::new();
        let mut file_article_counts: Vec<usize> = Vec::with_capacity(file_count);
        let mut art_part_numbers: Vec<i32> = Vec::new();
        let mut art_sizes: Vec<i32> = Vec::new();
        let mut art_msg_ids: Vec<&[u8]> = Vec::new();

        for df in &donor_nzb.files {
            file_names.push(&df.filename);
            file_sizes.push(df.size);
            file_is_par.push(u8::from(df.par_file));
            file_group_counts.push(df.groups.len());
            for g in df.groups.iter() {
                groups.push(g.as_slice());
            }
            let art_cnt = df
                .article_parts
                .len()
                .min(df.article_sizes.len())
                .min(df.article_msg_ids.len());
            file_article_counts.push(art_cnt);
            for i in 0..art_cnt {
                art_part_numbers.push(df.article_parts[i]);
                art_sizes.push(df.article_sizes[i]);
                art_msg_ids.push(&df.article_msg_ids[i]);
            }
        }

        build_donor_key(
            password,
            &file_names,
            &file_sizes,
            &file_is_par,
            &file_group_counts,
            &groups,
            &file_article_counts,
            &art_part_numbers,
            &art_sizes,
            &art_msg_ids,
        )
    }

    fn donor_dead(
        &self,
        q: JobQueue<'_>,
        donor: &DonorSource,
        donor_nzb: &DonorParsedNzb,
        counters: &mut RunCounters,
    ) -> bool {
        if let Some(&cached) = counters.donor_dead.get(&donor.queued_filename) {
            return cached;
        }

        let mut selected: Vec<&DonorParsedFile> = Vec::new();
        for pass in 0..2 {
            if !selected.is_empty() {
                break;
            }
            for df in &donor_nzb.files {
                if df.total_articles > 0 && (pass == 1 || !df.par_file) {
                    selected.push(df);
                }
            }
        }

        let mut total_arts: usize = 0;
        for df in &selected {
            total_arts = total_arts.saturating_add(df.total_articles.max(0) as usize);
        }
        let indexes = sample_indexes(total_arts, SAMPLE_COUNT);
        let mut probe = NzbgetRsDupeProbe::new(0, false, 0);
        let mut sample_cnt: i32 = 0;
        for pos in indexes {
            let mut offset = 0usize;
            for df in &selected {
                let cnt = df.total_articles.max(0) as usize;
                if pos >= offset + cnt {
                    offset += cnt;
                    continue;
                }
                let idx = pos - offset;
                if idx < df.article_msg_ids.len() {
                    probe.add_sample(df.article_msg_ids[idx].clone(), (*df.groups).clone());
                    sample_cnt += 1;
                }
                break;
            }
        }

        let mut dead = false;
        if sample_cnt >= MIN_ARTICLES && !self.host_is_stopped(q) {
            self.checking_donor.fetch_add(1, Ordering::SeqCst);
            dead = probe.check(DONOR_CHECK_SEC);
            self.checking_donor.fetch_sub(1, Ordering::SeqCst);
            if dead {
                let mut msg = b"Skipping duplicate ".to_vec();
                msg.extend_from_slice(&donor.info_name);
                msg.extend_from_slice(
                    format!(
                        " for stream repair: none of {SAMPLE_COUNT} sampled articles exists on any server"
                    )
                    .as_bytes(),
                );
                self.print_message(q, MessageKind::Info, &msg);
            }
        }

        counters
            .donor_dead
            .insert(donor.queued_filename.clone(), dead);
        dead
    }

    fn parse_donor(
        &self,
        q: JobQueue<'_>,
        queued_filename: &[u8],
        par_ignore_ext: &[u8],
    ) -> Option<DonorParsedNzb> {
        parse_donor_nzb_file(queued_filename, par_ignore_ext, |msg| {
            self.global_log(q, MessageKind::Detail, msg);
        })
    }

    fn exec_repair(
        &self,
        q: JobQueue<'_>,
        opts: &HostOptions,
        dest_dir: &[u8],
        targets: &mut [RepairTarget],
        donors: &[DonorSource],
        member_names: &[Vec<u8>],
        counters: &mut RunCounters,
        left_to_borrow: &[bool],
    ) {
        self.batch_fetcher.set_worker_count(if self.live_mode {
            if self.parallel_live_fetch.load(Ordering::SeqCst) {
                // Half the connections only while a dead volume is left to borrow: a later
                // pass that patches a few holes keeps to the usual count.
                if left_to_borrow.iter().any(|b| *b) {
                    live_borrow_fetch_worker_count(opts.primary_connections)
                } else {
                    stream_fetch_worker_count(opts.primary_connections)
                }
            } else {
                live_hole_fetch_worker_count(opts.primary_connections)
            }
        } else {
            stream_fetch_worker_count(opts.primary_connections)
        });
        let mut tried_postings: BTreeSet<Vec<u8>> = BTreeSet::new();
        // Dead volumes left to the cross-packing repair to borrow (the mask from
        // `dead_volumes_to_borrow`, or empty for none) skip the whole-file recreation.
        let left_to_borrow: Vec<bool> = (0..targets.len())
            .map(|i| left_to_borrow.get(i).copied().unwrap_or(false))
            .collect();
        for (t, _) in targets.iter().zip(&left_to_borrow).filter(|(_, l)| **l) {
            let mut msg = b"Leaving dead volume ".to_vec();
            msg.extend_from_slice(&t.filename);
            msg.extend_from_slice(b" to be borrowed from a duplicate; direct unpack keeps going");
            self.print_message(q, MessageKind::Info, &msg);
        }

        for donor in donors {
            if self.host_is_stopped(q) || self.yielding.load(Ordering::SeqCst) {
                break;
            }
            let any_holes = targets
                .iter()
                .zip(&left_to_borrow)
                .any(|(t, left)| !left && t.patch_eligible && t.needs_repair());
            if !any_holes {
                break;
            }

            let Some(donor_nzb) = self.parse_donor(q, &donor.queued_filename, &opts.par_ignore_ext)
            else {
                continue;
            };
            let donor_key = Self::build_parsed_donor_key(&donor_nzb, &donor.password);
            if !tried_postings.insert(donor_key) {
                let mut msg = b"Skipping another copy of duplicate posting ".to_vec();
                msg.extend_from_slice(&donor.info_name);
                self.print_message(q, MessageKind::Info, &msg);
                continue;
            }
            if self.donor_dead(q, donor, &donor_nzb, counters) {
                continue;
            }

            if !self.live_mode {
                let mut label = b"Repairing from duplicate ".to_vec();
                label.extend_from_slice(&donor.info_name);
                self.set_post_progress(q, &label, 0);
            }

            let member_name_refs: Vec<&[u8]> = member_names.iter().map(|m| m.as_slice()).collect();
            let mut m1_targets: Vec<M1RepairTarget> = targets
                .iter()
                .zip(&left_to_borrow)
                .map(|(t, left)| M1RepairTarget {
                    filename: t.filename.clone(),
                    decoded_file_size: t.decoded_file_size,
                    holes: t.holes.clone(),
                    encoded_size: t.encoded_size,
                    article_count: t.article_count,
                    recreated: t.recreated,
                    donor_set: Vec::new(),
                    patch_eligible: t.patch_eligible && !left,
                    positional_rank: t.positional_rank,
                    positional_window: t.positional_window,
                    steps_hash: t.steps_hash,
                })
                .collect();
            let target_file_ids: Vec<i32> = targets.iter().map(|t| t.file_id).collect();
            let mut live_filenames: Vec<Vec<u8>> =
                targets.iter().map(|t| t.filename.clone()).collect();

            let donor_file_names: Vec<&[u8]> = donor_nzb
                .files
                .iter()
                .map(|f| f.filename.as_slice())
                .collect();
            let donor_file_sizes: Vec<i64> = donor_nzb.files.iter().map(|f| f.size).collect();
            let donor_file_is_par: Vec<u8> = donor_nzb
                .files
                .iter()
                .map(|f| u8::from(f.par_file))
                .collect();
            let donor_file_steps_hashes: Vec<u64> =
                donor_nzb.files.iter().map(|f| f.steps_hash).collect();
            let donor_article_sizes: Vec<Vec<i32>> = donor_nzb
                .files
                .iter()
                .map(|f| f.article_sizes.clone())
                .collect();

            struct NativeM1Env<'a> {
                ctrl: &'a NzbgetRsStreamRepairCtrl,
                q: JobQueue<'a>,
                donor_files: &'a [DonorParsedFile],
                target_file_ids: &'a [i32],
                live_filenames: &'a mut [Vec<u8>],
                recovered_bytes: &'a mut i64,
            }

            impl M1DonorEnv for NativeM1Env<'_> {
                fn is_stopped(&self) -> bool {
                    self.ctrl.host_is_stopped(self.q)
                }

                fn refresh_live_target_name(&mut self, target_index: usize) -> Option<Vec<u8>> {
                    if target_index >= self.target_file_ids.len() {
                        return None;
                    }
                    self.ctrl.refresh_live_target_name(
                        self.q,
                        self.target_file_ids[target_index],
                        &mut self.live_filenames[target_index],
                    );
                
                    Some(self.live_filenames[target_index].clone())
                }

                fn begin_member_batch(&mut self, donor_file_index: i32, part_indices: &[i32]) {
                    if donor_file_index < 0 {
                        return;
                    }
                    let Some(df) = self.donor_files.get(donor_file_index as usize) else {
                        return;
                    };
                    let mut requests = Vec::with_capacity(part_indices.len());
                    for &pi in part_indices {
                        if pi >= 0 {
                            if let Some(msg_id) = df.article_msg_ids.get(pi as usize) {
                                requests.push(BatchRequest {
                                    message_id: msg_id.clone(),
                                    groups: Arc::clone(&df.groups),
                                });
                            }
                        }
                    }
                    self.ctrl.batch_fetcher.begin(requests);
                }

                fn next_article(&mut self) -> Option<FetchedDonorArticle> {
                    let fetched = self.ctrl.batch_fetcher.next()?;
                    Some(FetchedDonorArticle {
                        success: fetched.success,
                        offset: fetched.offset,
                        file_size: fetched.file_size,
                        data: fetched.data,
                    })
                }

                fn cancel_remaining(&mut self) {
                    self.ctrl.batch_fetcher.cancel_remaining();
                }

                fn add_progress_bytes(&mut self, bytes: i64) {
                    self.ctrl.progress_bytes.fetch_add(bytes, Ordering::SeqCst);
                }

                fn add_recovered_bytes(&mut self, bytes: i64) {
                    *self.recovered_bytes = self.recovered_bytes.saturating_add(bytes);
                }

                fn emit_message_bytes(&mut self, kind: TargetMessageKind, text: &[u8]) {
                    self.ctrl
                        .print_message(self.q, map_target_msg_kind(kind), text);
                
                }
            }

            let mut env = NativeM1Env {
                ctrl: self,
                q,
                donor_files: &donor_nzb.files,
                target_file_ids: &target_file_ids,
                live_filenames: &mut live_filenames,
                recovered_bytes: &mut counters.recovered_bytes,
            };

            process_m1_donor(
                dest_dir,
                self.live_mode,
                &mut m1_targets,
                &member_name_refs,
                &donor.info_name,
                &donor_file_names,
                &donor_file_sizes,
                &donor_file_is_par,
                &donor_file_steps_hashes,
                &donor_article_sizes,
                &mut env,
            );

            for (target, m1) in targets.iter_mut().zip(m1_targets.into_iter()) {
                if !m1.filename.is_empty() {
                    target.filename = m1.filename;
                }
                target.decoded_file_size = m1.decoded_file_size;
                if m1.recreated {
                    target.recreated = true;
                }
                target.holes = m1.holes;
            }
        }
    }

    /// Runs the whole-file recreation for the dead volumes left to the borrow that it did not
    /// rebuild, and for nothing else.
    #[allow(clippy::too_many_arguments)]
    fn recreate_unborrowed(
        &self,
        q: JobQueue<'_>,
        opts: &HostOptions,
        dest_dir: &[u8],
        targets: &mut [RepairTarget],
        donors: &[DonorSource],
        member_names: &[Vec<u8>],
        counters: &mut RunCounters,
        left_to_borrow: &[bool],
    ) {
        let mut owed = self.recreate_owed.lock().unwrap_or_else(|e| e.into_inner());
        let retry: Vec<bool> = (0..targets.len())
            .map(|i| {
                let t = &targets[i];
                left_to_borrow.get(i).copied().unwrap_or(false)
                    && (!t.tried_before || owed.contains(&t.filename))
                    && t.needs_repair()
                    && !crate::postproc::direct_unpack_has_virtual(
                        &[
                            dest_dir,
                            &[crate::paths::PATH_SEPARATOR],
                            t.filename.as_slice(),
                        ]
                        .concat(),
                    )
            })
            .collect();
        for (t, r) in targets.iter().zip(&retry) {
            if *r {
                owed.remove(&t.filename);
            }
        }
        drop(owed);
        if !retry.iter().any(|r| *r) {
            return;
        }
        let saved: Vec<bool> = targets.iter().map(|t| t.patch_eligible).collect();
        for (t, r) in targets.iter_mut().zip(&retry) {
            if *r {
                let mut msg = b"Could not borrow dead volume ".to_vec();
                msg.extend_from_slice(&t.filename);
                msg.extend_from_slice(b"; recreating it from a byte-identical duplicate");
                self.print_message(q, MessageKind::Info, &msg);
            } else {
                t.patch_eligible = false;
            }
        }
        self.exec_repair(
            q,
            opts,
            dest_dir,
            targets,
            donors,
            member_names,
            counters,
            &[],
        );
    
        for ((t, s), r) in targets.iter_mut().zip(saved).zip(&retry) {
            t.patch_eligible = s;
            if *r && t.needs_repair() {
                let mut msg = b"No byte-identical duplicate holds ".to_vec();
                msg.extend_from_slice(&t.filename);
                msg.extend_from_slice(
                    b" either; a later run may still borrow it, else par2 rebuilds it",
                );
                self.print_message(q, MessageKind::Info, &msg);
            }
        }
    }

    /// Data files of the download still queued and the download's id (`op == 16`), each
    /// `None` when the host cannot tell.
    /// The number of data files still queued in the target download, and its id; both
    /// `None` when the download is gone.
    fn pending_data_files(&self, q: JobQueue<'_>) -> (Option<usize>, Option<i32>) {
        let host = q.host;
        host.with_locked_queue(|dq_ptr| {
            let nzb = if self.live_mode {
                host.queue_find_by_id(dq_ptr, q.nzb_id())
            } else if q.post_ptr().is_some() {
                host.post_snap(q.post_ptr()).nzb_ptr
            } else {
                NzbRef::NONE
            };
            if nzb.is_none() {
                return (None, None);
            }
            let pending = (0..host.nzb_file_len(nzb))
                .map(|i| host.nzb_file_at(nzb, i))
                .filter(|&f| f.is_some() && !host.file_snap(f).par_file)
                .count();
            (Some(pending), Some(host.nzb_snap(nzb, false).id))
        })
    }

    fn exec_cross_pack_repair(
        &self,
        q: JobQueue<'_>,
        opts: &HostOptions,
        dest_dir: &[u8],
        targets: &mut [RepairTarget],
        donors: &[DonorSource],
        member_names: &[Vec<u8>],
        counters: &mut RunCounters,
    ) {
        let member_name_refs: Vec<&[u8]> = member_names.iter().map(|m| m.as_slice()).collect();
        let target_inputs: Vec<CrossPackTargetInput> = targets
            .iter()
            .map(|t| CrossPackTargetInput {
                filename: t.filename.clone(),
                decoded_file_size: t.decoded_file_size,
                encoded_size: t.encoded_size,
                patch_eligible: t.patch_eligible,
                holes: t.holes.clone(),
                all_articles_failed: crate::streamrepair::nothing_arrived(
                    t.decoded_file_size,
                    &t.holes,
                ),
            })
            .collect();
        let pw =
            (!counters.target_password.is_empty()).then_some(counters.target_password.as_slice());

        let aborted = || self.host_is_stopped(q) || self.yield_stops_now();
        let mut emit_msg = |kind: TargetMessageKind, text: &[u8]| {
            self.print_message(q, map_target_msg_kind(kind), text);
        };
        let rar_vol_cb = |member_name: &[u8],
                          vol: &crate::rarreader::Volume,
                          files: &[crate::rarreader::RarFile]| {
            for line in format_rar_volume_debug_lines(member_name, vol, files) {
                self.global_log(q, MessageKind::Debug, &line);
            }
        };
        let rar_vol_ref: Option<
            &dyn Fn(&[u8], &crate::rarreader::Volume, &[crate::rarreader::RarFile]),
        > = self.debug_enabled.then_some(
            &rar_vol_cb as &dyn Fn(&[u8], &crate::rarreader::Volume, &[crate::rarreader::RarFile]),
        );

        let (pending_data_files, nzb_id) = self.pending_data_files(q);
        // Only direct unpack reads a rebuilt dead volume: without it nothing is borrowed.
        let nzb_id = nzb_id.filter(|_| direct_unpack_on());
        let dead_nzb_id = nzb_id;
        let dead_context = crate::streamrepair::DeadVolumeContext {
            pending_data_files,
            nzb_id,
        };
        let Some(mut session) = CrossPackRepairSession::new_with_dead(
            dest_dir,
            &member_name_refs,
            &target_inputs,
            pw,
            self.tls_enabled,
            Some(&aborted),
            &mut emit_msg,
            rar_vol_ref,
            dead_context,
        ) else {
            return;
        };
        // A volume that lost only its headers is rebuilt and written back as the session
        // starts, before any donor pass would report it: it is repaired already.
        for (t, holes) in targets.iter_mut().zip(session.target_holes()) {
            if holes.is_empty() {
                t.holes.clear();
            }
        }

        let mut tried_postings: BTreeSet<Vec<u8>> = BTreeSet::new();
        let allow_decompress = !self.live_mode && opts.allow_decompress_opt;

        for donor in donors {
            // A pass stopping for a newly dead volume probes no further donor.
            if self.host_is_stopped(q)
                || self.yielding.load(Ordering::SeqCst)
                || !session.has_inner_holes()
            {
                break;
            }

            let Some(donor_nzb) = self.parse_donor(q, &donor.queued_filename, &opts.par_ignore_ext)
            else {
                continue;
            };
            let donor_key = Self::build_parsed_donor_key(&donor_nzb, &donor.password);
            if !tried_postings.insert(donor_key) {
                let mut msg = b"Skipping another copy of duplicate posting ".to_vec();
                msg.extend_from_slice(&donor.info_name);
                self.print_message(q, MessageKind::Info, &msg);
                continue;
            }
            if self.donor_dead(q, donor, &donor_nzb, counters) {
                continue;
            }

            let donor_file_names: Vec<&[u8]> = donor_nzb
                .files
                .iter()
                .map(|f| f.filename.as_slice())
                .collect();
            let donor_file_sizes: Vec<i64> = donor_nzb.files.iter().map(|f| f.size).collect();
            let donor_article_sizes: Vec<Vec<i32>> = donor_nzb
                .files
                .iter()
                .map(|f| f.article_sizes.clone())
                .collect();

            struct NativeCrossPackEnv<'a> {
                ctrl: &'a NzbgetRsStreamRepairCtrl,
                q: JobQueue<'a>,
                donor_files: &'a [DonorParsedFile],
                targets: &'a mut [RepairTarget],
                recovered_bytes: &'a mut i64,
                recovered_holes: &'a mut i32,
            }

            impl CrossPackDonorEnv for NativeCrossPackEnv<'_> {
                fn is_stopped(&mut self) -> bool {
                    // Still asks the queue for new dead volumes while a volume finishes, so
                    // the yield is raised as soon as one turns up.
                    let new_dead = self.ctrl.yield_for_new_dead(self.q);
                    self.ctrl.host_is_stopped(self.q) || (new_dead && self.ctrl.yield_stops_now())
                }

                fn set_finishing_volume(&mut self, on: bool) {
                    self.ctrl.finishing_volume.store(on, Ordering::SeqCst);
                }

                fn fetch_donor_part(
                    &mut self,
                    member_index: i32,
                    part_index: i32,
                ) -> Option<FetchedDonorArticle> {
                    if member_index < 0 || part_index < 0 {
                        return None;
                    }
                    let df = self.donor_files.get(member_index as usize)?;
                    let msg_id = df.article_msg_ids.get(part_index as usize)?;
                    if let Some(hit) = recent_single(msg_id) {
                        return Some(hit);
                    }
                    let started = std::time::Instant::now();
                    let fetched = self.ctrl.fetcher.fetch(msg_id, &df.groups);
                    if fetched.success {
                        keep_single(
                            msg_id,
                            FetchedDonorArticle {
                                success: true,
                                offset: fetched.offset,
                                file_size: fetched.file_size,
                                data: fetched.data.clone(),
                            },
                        );
                    }
                    {
                        let mut st = self.ctrl.fetch_stats.lock().unwrap_or_else(|e| e.into_inner());
                        st.singles += 1;
                        st.bytes += fetched.data.len() as u64;
                        st.waited += started.elapsed();
                    }
                    Some(FetchedDonorArticle {
                        success: fetched.success,
                        offset: fetched.offset,
                        file_size: fetched.file_size,
                        data: fetched.data,
                    })
                }

                fn begin_donor_prefetch(&mut self, member_index: i32, part_indices: &[i32]) {
                    if member_index < 0 {
                        return;
                    }
                    let Some(df) = self.donor_files.get(member_index as usize) else {
                        return;
                    };
                    let mut requests = Vec::with_capacity(part_indices.len());
                    for &pi in part_indices {
                        if pi >= 0 {
                            if let Some(msg_id) = df.article_msg_ids.get(pi as usize) {
                                requests.push(BatchRequest {
                                    message_id: msg_id.clone(),
                                    groups: Arc::clone(&df.groups),
                                });
                            }
                        }
                    }
                    {
                        let mut st = self.ctrl.fetch_stats.lock().unwrap_or_else(|e| e.into_inner());
                        st.batches += 1;
                        st.batch_parts += requests.len() as u32;
                    }
                    self.ctrl.batch_fetcher.begin(requests);
                }

                fn begin_donor_prefetch_multi(&mut self, parts: &[(i32, i32)]) -> bool {
                    let mut requests = Vec::with_capacity(parts.len());
                    for &(member, part) in parts {
                        let msg = usize::try_from(member)
                            .ok()
                            .and_then(|m| self.donor_files.get(m))
                            .and_then(|df| {
                                let id = df.article_msg_ids.get(usize::try_from(part).ok()?)?;
                                Some(BatchRequest {
                                    message_id: id.clone(),
                                    groups: Arc::clone(&df.groups),
                                })
                            });
                        // Every part must map to a request, or the delivery order would shift.
                        let Some(request) = msg else {
                            return false;
                        };
                        requests.push(request);
                    }
                    {
                        let mut st = self.ctrl.fetch_stats.lock().unwrap_or_else(|e| e.into_inner());
                        st.batches += 1;
                        st.batch_parts += requests.len() as u32;
                    }
                    self.ctrl.batch_fetcher.begin(requests);
                    true
                }

                fn next_donor_prefetch(&mut self) -> Option<FetchedDonorArticle> {
                    let started = std::time::Instant::now();
                    let fetched = self.ctrl.batch_fetcher.next();
                    {
                        let mut st = self.ctrl.fetch_stats.lock().unwrap_or_else(|e| e.into_inner());
                        st.waited += started.elapsed();
                        st.bytes += fetched.as_ref().map_or(0, |f| f.data.len() as u64);
                    }
                    let fetched = fetched?;
                    Some(FetchedDonorArticle {
                        success: fetched.success,
                        offset: fetched.offset,
                        file_size: fetched.file_size,
                        data: fetched.data,
                    })
                }

                fn cancel_donor_prefetch(&mut self) {
                    self.ctrl.batch_fetcher.cancel_remaining();
                }

                fn begin_materialize_member(&mut self, member_index: i32) {
                    if member_index < 0 {
                        return;
                    }
                    let Some(df) = self.donor_files.get(member_index as usize) else {
                        return;
                    };
                    let requests: Vec<BatchRequest> = df
                        .article_msg_ids
                        .iter()
                        .map(|msg_id| BatchRequest {
                            message_id: msg_id.clone(),
                            groups: Arc::clone(&df.groups),
                        })
                        .collect();
                    {
                        let mut st = self.ctrl.fetch_stats.lock().unwrap_or_else(|e| e.into_inner());
                        st.batches += 1;
                        st.batch_parts += requests.len() as u32;
                    }
                    self.ctrl.batch_fetcher.begin(requests);
                }

                fn next_materialize_article(&mut self) -> Option<FetchedDonorArticle> {
                    self.next_donor_prefetch()
                }

                fn cancel_materialize_member(&mut self) {
                    self.cancel_donor_prefetch();
                }

                fn set_progress(&mut self, label: &[u8], stage_progress: i32) {
                    if !self.ctrl.live_mode {
                        self.ctrl
                            .set_post_progress(self.q, label, stage_progress);
                    
                    }
                }

                fn set_materializing(&mut self, delta: i32) {
                    self.ctrl.materializing.fetch_add(delta, Ordering::SeqCst);
                }

                fn add_recovered(&mut self, bytes: i64, holes: i32) {
                    *self.recovered_bytes = self.recovered_bytes.saturating_add(bytes);
                    self.ctrl.progress_bytes.fetch_add(bytes, Ordering::SeqCst);
                    *self.recovered_holes = self.recovered_holes.saturating_add(holes);
                }

                fn sync_target_holes(&mut self, target_holes: &[Vec<NzbgetRsByteRange>]) {
                    for (t, holes) in self.targets.iter_mut().zip(target_holes.iter()) {
                        t.holes = holes.clone();
                    }
                }

                fn emit_message_bytes(&mut self, kind: TargetMessageKind, text: &[u8]) {
                    self.ctrl
                        .print_message(self.q, map_target_msg_kind(kind), text);
                
                }

                fn emit_extractor_log(&mut self, kind: i32, msg: &str) {
                    // the extractor's codes: 0 info, 1 error, 2 warning, else detail
                    let log_kind = match kind {
                        0 => MessageKind::Info,
                        1 => MessageKind::Error,
                        2 => MessageKind::Warning,
                        _ => MessageKind::Detail,
                    };
                    self.ctrl
                        .global_log(self.q, log_kind, msg.as_bytes());
                
                }

                fn on_rar_volume_parsed(
                    &mut self,
                    member_name: &[u8],
                    vol: &crate::rarreader::Volume,
                    files: &[crate::rarreader::RarFile],
                ) {
                    if self.ctrl.debug_enabled {
                        for line in format_rar_volume_debug_lines(member_name, vol, files) {
                            self.ctrl.global_log(self.q, MessageKind::Debug, &line);
                        }
                    }
                }
            }

            let mut env = NativeCrossPackEnv {
                ctrl: self,
                q,
                donor_files: &donor_nzb.files,
                targets,
                recovered_bytes: &mut counters.recovered_bytes,
                recovered_holes: &mut counters.recovered_holes,
            };

            // Borrowing dead volumes: a yield waits for this donor's first dead volume (its
            // check included), so the next pass resumes after it instead of fetching it again.
            // The session clears this after each volume it borrows.
            self.finishing_volume
                .store(session.has_pending_dead_fills(), Ordering::SeqCst);
            *self.fetch_stats.lock().unwrap_or_else(|e| e.into_inner()) =
                DonorFetchStats::default();
            let donor_started = std::time::Instant::now();
            session.process_donor(
                &donor.queued_filename,
                &donor.info_name,
                &donor.password,
                &donor_file_names,
                &donor_file_sizes,
                &donor_article_sizes,
                allow_decompress,
                &mut env,
            );
            self.finishing_volume.store(false, Ordering::SeqCst);
            {
                let st = self.fetch_stats.lock().unwrap_or_else(|e| e.into_inner());
                if st.bytes > 0 {
                    let line = st.line(
                        &donor.info_name,
                        donor_started.elapsed(),
                        self.batch_fetcher.worker_count(),
                    );
                    drop(st);
                    self.print_message(q, MessageKind::Info, &line);
                }
            }
            // A donor whose video was checked while dead volumes waited for it: the queue
            // stops keeping the download for it once it fails the check in a few runs. Live
            // repair never decompresses, so a donor holding its video compressed cannot lend
            // live and counts as one that failed it.
            let identity = session
                .donor_identity()
                .or_else(|| (self.live_mode && session.donor_compressed()).then_some(false));
            if let (Some(id), Some(confirmed)) = (dead_nzb_id, identity) {
                if session.has_pending_dead_fills() || confirmed {
                    crate::postproc::note_dead_volume_donor_identity(
                        id,
                        &donor.queued_filename,
                        confirmed,
                    );
                }
            }
        }
    }

    fn report_remaining_holes(
        &self,
        q: JobQueue<'_>,
        targets: &[RepairTarget],
        counters: &mut RunCounters,
    ) {
        let interrupted = self.interrupted(q);
        if interrupted {
            let unfinished = targets
                .iter()
                .filter(|t| t.decoded_file_size <= 0 || !t.holes.is_empty())
                .count() as i32;
            counters.holes_remain |= unfinished > 0;
            let msg =
                format!("Stream repair interrupted with {unfinished} file(s) still incomplete");
            self.print_message(q, MessageKind::Info, msg.as_bytes());
        }

        let par_disposition: &[u8] = if counters.has_par_files {
            b"left to par-repair"
        } else {
            b"no par2 files to repair it"
        };

        for target in targets {
            let (outcome, new_articles, new_failed_size, new_failed_par_size) =
                classify_remaining_hole_target(
                    interrupted,
                    target.decoded_file_size,
                    target.holes.is_empty(),
                    target.is_par_file,
                    target.failed_articles,
                    target.failed_size,
                    target.missed_size,
                    counters.recovered_articles,
                    counters.recovered_failed_size,
                    counters.recovered_failed_par_size,
                );
            counters.recovered_articles = new_articles;
            counters.recovered_failed_size = new_failed_size;
            counters.recovered_failed_par_size = new_failed_par_size;
            if outcome == 0 || outcome == 1 {
                continue;
            }
            counters.holes_remain = true;
            if outcome == 2 {
                let mut msg = b"Stream repair of ".to_vec();
                msg.extend_from_slice(&target.filename);
                msg.extend_from_slice(b": no duplicate could supply the file (");
                msg.extend_from_slice(par_disposition);
                msg.push(b')');
                self.print_message(q, MessageKind::Info, &msg);
            } else {
                let missing = total_size(&target.holes);
                let miss_mb = stream_repair_bytes_to_mb(missing);
                let mut msg = b"Stream repair of ".to_vec();
                msg.extend_from_slice(&target.filename);
                msg.extend_from_slice(
                    format!(": {miss_mb:.1} MB still missing after stream repair (").as_bytes(),
                );
                msg.extend_from_slice(par_disposition);
                msg.push(b')');
                self.print_message(q, MessageKind::Info, &msg);

                if is_incomplete_nzb_hole(missing, target.encoded_size) {
                    let dec_mb = stream_repair_bytes_to_mb(target.decoded_file_size);
                    let cov_mb =
                        stream_repair_bytes_to_mb(target.decoded_file_size.saturating_sub(missing));
                    let mut warn_msg = target.filename.clone();
                    warn_msg.extend_from_slice(
                        format!(
                            " is {dec_mb:.1} MB according to its articles, but the nzb's articles cover only {cov_mb:.1} MB of it: the nzb is incomplete"
                        )
                        .as_bytes(),
                    );
                    self.print_message(q, MessageKind::Warning, &warn_msg);
                }
            }
        }
    }

    fn repair_completed_post(
        &self,
        q: JobQueue<'_>,
        opts: &HostOptions,
        dest_dir: &[u8],
        targets: &[RepairTarget],
        counters: &RunCounters,
    ) {
        use crate::queuesubsys_host::{
            NZB_I64_CUR_FAILED_SIZE, NZB_I64_CUR_SUCCESS_SIZE, NZB_I64_DUPE_RECOVERED_BYTES,
            NZB_I64_FAILED_SIZE, NZB_I64_PAR_CUR_FAILED_SIZE, NZB_I64_PAR_CUR_SUCCESS_SIZE,
            NZB_I64_PAR_FAILED_SIZE, NZB_I64_PAR_SUCCESS_SIZE, NZB_I64_SUCCESS_SIZE,
            NZB_INT_DUPE_RECOVERED_ARTICLES, NZB_INT_DUPE_RECOVERED_HOLES,
            NZB_INT_PAR_RENAME_STATUS, POST_INT_FORCE_PAR_FULL, POST_INT_REQUEST_PAR_CHECK,
            POST_INT_STREAM_REPAIR_DONE, POST_INT_STREAM_REPAIR_RECOVERED, POST_INT_WORKING,
        };
        let interrupted = self.interrupted(q);
        let host = q.host;
        host.with_locked_queue(|dq_ptr| {
            let Some(snap) = self.inspect_target(q, dq_ptr, false) else {
                return;
            };
            let nzb_ptr = snap.nzb_ptr;
            let mut job_count = snap.job_count;

            for target in targets {
                if target.decoded_file_size > 0 && target.holes.is_empty() {
                    host.nzb_stream_repair_update_completed_file(nzb_ptr, target.file_id, 1);
                } else if target.recreated {
                    host.nzb_stream_repair_update_completed_file(nzb_ptr, target.file_id, 2);
                }
            }

            let mut job_idx = 0usize;
            while job_idx < job_count {
                if !interrupted {
                    host.nzb_stream_repair_erase_job(nzb_ptr, job_idx, false);
                    job_count -= 1;
                    continue;
                }

                let jd = Self::repair_job(q, nzb_ptr, job_idx);
                let mut erased = false;
                if let Some(target) = targets.iter().find(|t| t.file_id == jd.file_id) {
                    if target.holes.is_empty() && target.decoded_file_size > 0 {
                        host.nzb_stream_repair_erase_job(nzb_ptr, job_idx, false);
                        job_count -= 1;
                        erased = true;
                    } else {
                        let new_dec = if jd.decoded_file_size <= 0 && target.decoded_file_size > 0 {
                            target.decoded_file_size
                        } else {
                            -1
                        };
                        Self::update_job_holes(q, nzb_ptr, job_idx, new_dec, &target.holes);
                    }
                }
                if !erased {
                    job_idx += 1;
                }
            }

            let ns = host.nzb_snap(nzb_ptr, false);
            let action = resolve_completion_par_action(
                counters.recovered_bytes,
                counters.holes_remain,
                ns.par_rename_status != RS_NONE,
                ns.par_status,
                opts.par_check == crate::queuesubsys_host::PC_MANUAL,
            );
            let request_par_check = action.check_main_pars
                && crate::parparser::find_main_pars(crate::fsops::path(dest_dir), false, |_| {});

            if counters.recovered_articles > 0
                || counters.recovered_bytes > 0
                || counters.recovered_holes > 0
            {
                host.nzb_set_int(
                    nzb_ptr,
                    NZB_INT_DUPE_RECOVERED_ARTICLES,
                    ns.dupe_recovered_articles + counters.recovered_articles,
                );
                host.nzb_set_i64(
                    nzb_ptr,
                    NZB_I64_DUPE_RECOVERED_BYTES,
                    ns.dupe_recovered_bytes + counters.recovered_bytes,
                );
                host.nzb_set_int(
                    nzb_ptr,
                    NZB_INT_DUPE_RECOVERED_HOLES,
                    ns.dupe_recovered_holes + counters.recovered_holes,
                );
            }
            if counters.recovered_failed_size > 0 {
                let pair = |failed, success| SizePair { failed, success };
                let mut sizes = NzbSizes {
                    total: pair(ns.failed_size, ns.success_size),
                    current: pair(ns.current_failed_size, ns.current_success_size),
                    par: pair(ns.par_failed_size, ns.par_success_size),
                    par_current: pair(ns.par_current_failed_size, ns.par_current_success_size),
                };
                sizes.credit_repaired(
                    counters.recovered_failed_size,
                    counters.recovered_failed_par_size,
                );
                host.nzb_set_i64(nzb_ptr, NZB_I64_FAILED_SIZE, sizes.total.failed);
                host.nzb_set_i64(nzb_ptr, NZB_I64_SUCCESS_SIZE, sizes.total.success);
                host.nzb_set_i64(nzb_ptr, NZB_I64_CUR_FAILED_SIZE, sizes.current.failed);
                host.nzb_set_i64(nzb_ptr, NZB_I64_CUR_SUCCESS_SIZE, sizes.current.success);
                host.nzb_set_i64(nzb_ptr, NZB_I64_PAR_FAILED_SIZE, sizes.par.failed);
                host.nzb_set_i64(nzb_ptr, NZB_I64_PAR_SUCCESS_SIZE, sizes.par.success);
                host.nzb_set_i64(nzb_ptr, NZB_I64_PAR_CUR_FAILED_SIZE, sizes.par_current.failed);
                host.nzb_set_i64(nzb_ptr, NZB_I64_PAR_CUR_SUCCESS_SIZE, sizes.par_current.success);
            }
            let post_ptr = q.post_ptr();
            host.post_set_int(post_ptr, POST_INT_STREAM_REPAIR_DONE, 1);
            host.post_set_int(
                post_ptr,
                POST_INT_STREAM_REPAIR_RECOVERED,
                i32::from(counters.recovered_bytes > 0),
            );
            if action.reset_par_rename {
                host.nzb_set_int(nzb_ptr, NZB_INT_PAR_RENAME_STATUS, RS_NONE);
            }
            if let Some(status) = action.new_par_status {
                host.nzb_set_par_status(nzb_ptr, status);
            }
            if action.force_par_full {
                host.post_set_int(post_ptr, POST_INT_FORCE_PAR_FULL, 1);
            }
            if action.clear_request_par_check {
                host.post_set_int(post_ptr, POST_INT_REQUEST_PAR_CHECK, 0);
            }
            if request_par_check {
                host.post_set_int(post_ptr, POST_INT_REQUEST_PAR_CHECK, 1);
            }
            host.dq_save(dq_ptr);
            host.post_set_int(post_ptr, POST_INT_WORKING, 0);
        });
    }

    fn repair_completed_live(
        &self,
        q: JobQueue<'_>,
        targets: &[RepairTarget],
        counters: &RunCounters,
    ) {
        q.host.with_locked_queue(|dq_ptr| {
            self.record_live_pass(q, dq_ptr, targets, counters);
        });
    }

    /// Writes one live pass's results to the job; returns the job, 0 when this thread no
    /// longer holds it.
    fn record_live_pass(
        &self,
        q: JobQueue<'_>,
        dq_ptr: usize,
        targets: &[RepairTarget],
        counters: &RunCounters,
    ) -> NzbRef {
        use crate::queuesubsys_host::{NZB_I64_DUPE_RECOVERED_BYTES, NZB_INT_DUPE_RECOVERED_HOLES};
        let host = q.host;
        let nzb_ptr = host.queue_find_by_id(dq_ptr, q.nzb_id());
        if nzb_ptr.is_none() {
            return NzbRef::NONE;
        }
        let ns = host.nzb_snap(nzb_ptr, false);
        let rec_bytes = counters.recovered_bytes;
        if ns.live_repair_thread_ptr != q.thread_ptr() {
            // Another live repair took the job over: keep only the byte count.
            host.nzb_set_i64(
                nzb_ptr,
                NZB_I64_DUPE_RECOVERED_BYTES,
                ns.dupe_recovered_bytes + rec_bytes,
            );
            return NzbRef::NONE;
        }
        let rec_holes = counters.recovered_holes;
        if rec_bytes > 0 || rec_holes > 0 {
            host.nzb_set_i64(
                nzb_ptr,
                NZB_I64_DUPE_RECOVERED_BYTES,
                ns.dupe_recovered_bytes + rec_bytes,
            );
            host.nzb_set_int(
                nzb_ptr,
                NZB_INT_DUPE_RECOVERED_HOLES,
                ns.dupe_recovered_holes + rec_holes,
            );
        }
        let job_ids: Vec<i32> = (0..host.nzb_stream_repair_jobs_len(nzb_ptr))
            .map(|j_idx| Self::repair_job(q, nzb_ptr, j_idx).file_id)
            .collect();

        for target in targets.iter().filter(|t| t.patch_eligible) {
            if let Some(j_idx) = job_ids.iter().position(|&id| id == target.file_id) {
                let new_dec = if target.recreated {
                    host.nzb_stream_repair_update_completed_file(nzb_ptr, target.file_id, 2);
                    target.decoded_file_size
                } else {
                    -1
                };
                Self::update_job_holes(q, nzb_ptr, j_idx, new_dec, &target.holes);
            }
        }
        nzb_ptr
    }

    /// Ends the live repair: gives the job back (clears the live-repair thread, lets a waiting
    /// post-processing go on). Called once, after the last pass.
    fn release_live_job(&self, q: JobQueue<'_>) {
        use crate::postproc::{
            direct_unpack_awaiting_repair, direct_unpack_waits_on_dead_volume,
            take_dead_volume_rerun,
        };
        use crate::queuesubsys_host::{NZB_ACT_CLEAR_LIVE_REPAIR_THREAD, POST_INT_WORKING};
        let host = q.host;
        host.with_locked_queue(|dq_ptr| {
            let nzb_ptr = self.record_live_pass(q, dq_ptr, &[], &RunCounters::default());
            if nzb_ptr.is_none() {
                return;
            }
            host.nzb_action(nzb_ptr, NZB_ACT_CLEAR_LIVE_REPAIR_THREAD);
            let ns = host.nzb_snap(nzb_ptr, false);
            // A volume arrived while this pass ran and direct unpack still waits on a dead
            // volume: one more pass may now place it for the borrow.
            if ns.post_info_ptr.is_none()
                && take_dead_volume_rerun(ns.id)
                && direct_unpack_waits_on_dead_volume(ns.id)
            {
                host.stream_repair_start_live(nzb_ptr);
            }
            // A direct unpack parked on damaged volumes waits for post-processing itself, so
            // it will not release the job: the ending live repair has to.
            if ns.post_info_ptr.is_some()
                && (!ns.has_unpack_thread || direct_unpack_awaiting_repair(ns.id))
            {
                host.post_set_int(ns.post_info_ptr, POST_INT_WORKING, 0);
            }
            host.dq_save(dq_ptr);
        });
    }

    pub(crate) fn run(&self, q: JobQueue<'_>) {
        if self.live_mode {
            self.run_live(q);
        } else {
            self.run_post(q);
        }
    }

    fn run_post(&self, q: JobQueue<'_>) {
        // (self.nzb_id is set for a live repair only)
        let _running = self.pending_data_files(q).1.map(RunningRepair::mark);
        // Direct unpack already extracted the whole archive (dead volumes read in place from
        // what the live repair borrowed): borrowing those volumes again into files nothing
        // reads only spends downloads.
        let already_unpacked = q.host.with_locked_queue(|_| {
            let post_ptr = q.post_ptr();
            if post_ptr.is_none() {
                return false;
            }
            let nzb = q.host.post_snap(post_ptr).nzb_ptr;
            !nzb.is_none()
                && q.host.nzb_snap(nzb, false).direct_unpack_status
                    == crate::status::DirectUnpackStatus::Success
        });
        let mut counters = RunCounters::default();
        let Some((nzb_name, dest_dir, mut targets, donors, member_names)) =
            q.host.with_locked_queue(|dq_ptr| {
                self.collect_targets_and_donors(q, dq_ptr, true, false, &mut counters)
            })
        else {
            return;
        };
        self.restore_lost_heads(q, &dest_dir, &mut targets);

        let opts = self.query_host_options();

        // Direct unpack reads RAR volumes only: a damaged file of another kind (a 7z, a
        // standalone file) is still repaired.
        let skipped = if already_unpacked {
            let before = targets.len();
            targets.retain(|t| !crate::filetypes::is_rar_file(&t.filename));
            before - targets.len()
        } else {
            0
        };
        if skipped > 0 {
            let mut msg = format!("Not stream-repairing {skipped} RAR volume(s) of ").into_bytes();
            msg.extend_from_slice(&nzb_name);
            msg.extend_from_slice(b": direct unpack already extracted them");
            self.print_message(q, MessageKind::Info, &msg);
        }
        if targets.is_empty() {
            if skipped == 0 {
                let mut msg = b"Nothing to stream-repair for ".to_vec();
                msg.extend_from_slice(&nzb_name);
                self.print_message(q, MessageKind::Info, &msg);
            }
        } else if donors.is_empty() {
            let mut msg = b"No duplicate collections found for stream repair of ".to_vec();
            msg.extend_from_slice(&nzb_name);
            self.print_message(q, MessageKind::Info, &msg);
        } else {
            let wd_handle = self.start_watchdog(q, &opts, &targets);
            self.compute_positional_ranks(&dest_dir, &mut targets, &member_names);
            self.exec_repair(
                q,
                &opts,
                &dest_dir,
                &mut targets,
                &donors,
                &member_names,
                &mut counters,
                &[],
            );
        
            let any_holes = targets.iter().any(|t| t.needs_repair());
            if any_holes && !self.host_is_stopped(q) {
                self.exec_cross_pack_repair(
                    q,
                    &opts,
                    &dest_dir,
                    &mut targets,
                    &donors,
                    &member_names,
                    &mut counters,
                );
            
            }
            self.restore_lost_tails(q, &dest_dir, &mut targets, &member_names);
            self.stop_watchdog(
                q,
                wd_handle,
                &nzb_name,
                &targets,
                counters.recovered_bytes,
            );
        
        }

        self.report_remaining_holes(q, &targets, &mut counters);
        self.repair_completed_post(q, &opts, &dest_dir, &targets, &counters);
        // Volumes this repair completed can be read by a paused direct unpack again, as after
        // a live pass: left held, they wait for a par2 repair that may fail over another file
        // (a dead volume direct unpack reads rebuilt in place), and the extraction is dropped.
        // Only all at once: with any still held, par2 runs next and may rewrite a released one.
        #[cfg(unix)]
        if let Some(id) = self.target_nzb_id(q) {
            let complete: Vec<Vec<u8>> = targets
                .iter()
                .filter(|t| !t.needs_repair())
                .map(|t| [dest_dir.as_slice(), &[crate::paths::PATH_SEPARATOR], &t.filename].concat())
                .collect();
            crate::postproc::direct_unpack_release_if_all(id, &complete);
        }
    
    }

    fn run_live(&self, q: JobQueue<'_>) {
        // Volumes can go dead while a pass borrows another one (a run of dead volumes arrives
        // over a minute or two): look again after each pass, while this thread still holds the
        // job, and borrow the new ones before post-processing takes over and par2 rebuilds them.
        // The job is given back only after the last pass: giving it back after each one would
        // end this thread's hold, and the next look would find nothing.
        // Holes also turn up while a pass runs, in files still arriving when it collected its
        // targets: the pass marks those files tried, so without another pass their holes go to
        // a par2 repair that may not exist, though the duplicate could lend them.
        // Such a file finishing while a pass runs gets its repair job then (a new file to this
        // repair), and the live repair already running keeps the queue from starting another.
        let mut tried: std::collections::BTreeSet<Vec<u8>> = Default::default();
        let mut seen: std::collections::BTreeMap<Vec<u8>, Vec<(i64, i64)>> = Default::default();
        let mut again: std::collections::BTreeSet<Vec<u8>> = Default::default();
        let mut rerun_for: std::collections::BTreeMap<Vec<u8>, Vec<(i64, i64)>> =
            Default::default();
        let mut ran = false;
        // Passes stopped for a volume that went dead meanwhile do not count against the bound:
        // a long run of dead volumes can stop several.
        let (mut passes, mut yields) = (0, 0);
        while passes < MAX_LIVE_BORROW_PASSES {
            passes += 1;
            let Some((left, attempted)) = self.run_live_pass(q, &again) else {
                break;
            };
            ran = true;
            tried.extend(left);
            if self.host_is_stopped(q) {
                break;
            }
            // The pass stopped for a volume that went dead meanwhile: start the next one now,
            // taking on again every file it had taken on (it was cut short: the files are marked
            // tried, and their holes are not counted as seen, so a later look retries them).
            if self.yielding.swap(false, Ordering::SeqCst) && yields < MAX_LIVE_YIELDS {
                yields += 1;
                passes -= 1;
                again = attempted.into_iter().map(|(name, _)| name).collect();
                continue;
            }
            for (name, holes) in attempted {
                seen.entry(name).or_default().extend(holes);
            }
            let (fresh, now) = self.look_again(q);
            let (newcomers, retry) = holes_to_rerun(&seen, &mut rerun_for, now);
            again = retry;
            if fresh.iter().all(|n| tried.contains(n)) && again.is_empty() && !newcomers {
                break;
            }
        }
        if ran {
            self.release_live_job(q);
        }
    }

    /// Restores the ends of RAR volumes whose only lost bytes lie behind their file's data (the
    /// end block and the quick-open record in front of it, lost with a volume's last article):
    /// no duplicate lends those, they belong to no file, and without them the extraction stops
    /// at the volume. Rebuilt from the nearest complete volume of the same set; see
    /// `deadvolume::restore_tail_from_sibling`.
    /// Writes back the first bytes of RAR5 volumes that lost them (their first article) from
    /// each volume's own quick-open record (see [`crate::deadvolume::restore_head_from_tail`]),
    /// and leaves only the rest of that hole, which holds archived data, to the borrow. When
    /// every copy and the par2 set lost every volume's first article, nothing else says where
    /// a volume's data belongs; with the headers back each volume reads, maps and lends again.
    fn restore_lost_heads(&self, q: JobQueue<'_>, dest_dir: &[u8], targets: &mut [RepairTarget]) {
        let path = |name: &[u8]| [dest_dir, &[crate::paths::PATH_SEPARATOR], name].concat();
        for t in targets.iter_mut().filter(|t| t.holes.first().is_some_and(|h| h.offset == 0)) {
            let Some((_, number, _, _)) = crate::streamrepair::rar_part_number(&t.filename) else {
                continue;
            };
            let holes: Vec<(u64, u64)> = hole_spans(t)
                .into_iter()
                .filter_map(|(s, e)| Some((u64::try_from(s).ok()?, u64::try_from(e).ok()?)))
                .collect();
            let Some(head_len) = number.checked_sub(1).and_then(|index| {
                crate::deadvolume::restore_head_from_tail(&path(&t.filename), &holes, index as usize)
            }) else {
                continue;
            };
            let head_len = head_len as i64;
            let first = &mut t.holes[0];
            let end = first.offset.saturating_add(first.size);
            if end <= head_len {
                t.holes.remove(0);
            } else {
                first.offset = head_len;
                first.size = end - head_len;
            }
            let mut msg = b"Rebuilt the headers of ".to_vec();
            msg.extend_from_slice(&t.filename);
            msg.extend_from_slice(
                format!(
                    " ({head_len} bytes) from its own quick-open record; borrowing the {:.1} MB \
                     of archived data its first article held",
                    crate::streamrepair::stream_repair_bytes_to_mb(end.saturating_sub(head_len))
                )
                .as_bytes(),
            );
            self.print_message(q, MessageKind::Info, &msg);
        }
    }

    fn restore_lost_tails(
        &self,
        q: JobQueue<'_>,
        dest_dir: &[u8],
        targets: &mut [RepairTarget],
        member_names: &[Vec<u8>],
    ) {
        let damaged: Vec<Vec<u8>> = targets
            .iter()
            .filter(|t| t.needs_repair())
            .map(|t| t.filename.to_ascii_lowercase())
            .collect();
        let path = |name: &[u8]| [dest_dir, &[crate::paths::PATH_SEPARATOR], name].concat();
        for t in targets
            .iter_mut()
            .filter(|t| t.patch_eligible && !t.holes.is_empty() && t.decoded_file_size > 0)
        {
            let Some((prefix, number, _, _)) = crate::streamrepair::rar_part_number(&t.filename)
            else {
                continue;
            };
            let holes: Vec<(u64, u64)> = hole_spans(t)
                .into_iter()
                .filter_map(|(s, e)| Some((u64::try_from(s).ok()?, u64::try_from(e).ok()?)))
                .collect();
            let mut siblings: Vec<(u32, &Vec<u8>)> = member_names
                .iter()
                .filter(|n| !damaged.contains(&n.to_ascii_lowercase()))
                .filter_map(|n| {
                    let (p, k, _, _) = crate::streamrepair::rar_part_number(n)?;
                    (p.eq_ignore_ascii_case(&prefix) && k != number)
                        .then_some((k.abs_diff(number), n))
                })
                .collect();
            siblings.sort();
            for (_, sibling) in siblings.into_iter().take(4) {
                if crate::deadvolume::restore_tail_from_sibling(
                    &path(&t.filename),
                    &holes,
                    &path(sibling),
                ) {
                    t.holes.clear();
                    if let Some(id) = self.target_nzb_id(q) {
                        crate::postproc::note_volume_end_rebuilt(id);
                    }
                    let mut msg = b"Rebuilt the end of ".to_vec();
                    msg.extend_from_slice(&t.filename);
                    msg.extend_from_slice(b" from ");
                    msg.extend_from_slice(sibling);
                    msg.extend_from_slice(b": its lost bytes lie behind the archived data");
                    self.print_message(q, MessageKind::Info, &msg);
                    break;
                }
            }
        }
    }

    /// Names of the dead volumes a pass would leave to the borrow now, and the holes of every
    /// file that still has some.
    /// True once a volume went dead that the borrowing live pass did not take on. Until the
    /// pass ends, a dead volume found after it started waits for the whole borrow before it is
    /// handed to direct unpack, so the extraction stands at it (minutes for a run of dead
    /// volumes). Stopping the pass instead lets the next one hand every dead volume over at
    /// once and resume the borrow where this one left it (its borrowed bytes are kept). The
    /// queue is asked at most every `YIELD_CHECK`.
    /// Whether a raised yield stops the pass now: not while a dead volume's borrow finishes.
    fn yield_stops_now(&self) -> bool {
        self.yielding.load(Ordering::SeqCst) && !self.finishing_volume.load(Ordering::SeqCst)
    }

    fn yield_for_new_dead(&self, q: JobQueue<'_>) -> bool {
        if !self.live_mode {
            return false;
        }
        if self.yielding.load(Ordering::SeqCst) {
            return true;
        }
        let taken = {
            let mut g = self.pass_dead.lock().unwrap_or_else(|e| e.into_inner());
            if !g.borrowing {
                return false;
            }
            let now = std::time::Instant::now();
            if g.checked.is_some_and(|t| now.duration_since(t) < YIELD_CHECK) {
                return false;
            }
            g.checked = Some(now);
            g.taken.clone()
        };
        let mut counters = RunCounters::default();
        let Some((_, dest_dir, targets, _, member_names)) = q.host.with_locked_queue(|dq_ptr| {
            self.collect_targets_and_donors(q, dq_ptr, false, false, &mut counters)
        }) else {
            return false;
        };
        // Every finished file of the download, damaged or not.
        let finished: Vec<&[u8]> = member_names.iter().map(Vec::as_slice).collect();
        let Some(new) = dead_volumes_to_borrow(&dest_dir, &targets)
            .into_iter()
            .zip(&targets)
            .filter_map(|(left, t)| left.then_some(&t.filename))
            .find(|n| !taken.contains(*n) && placeable_now(n, &finished))
            .cloned()
        else {
            return false;
        };
        let mut msg = b"Dead volume ".to_vec();
        msg.extend_from_slice(&new);
        msg.extend_from_slice(
            b" went dead while borrowing others: handing it to direct unpack now, the borrow goes on after",
        );
        self.print_message(q, MessageKind::Info, &msg);
        self.yielding.store(true, Ordering::SeqCst);
        true
    }

    fn look_again(&self, q: JobQueue<'_>) -> (Vec<Vec<u8>>, Vec<(Vec<u8>, Vec<(i64, i64)>)>) {
        let mut counters = RunCounters::default();
        // Only looks: the live-attempted marks belong to the pass that repairs the files.
        let Some((_, dest_dir, targets, _, _)) = q.host.with_locked_queue(|dq_ptr| {
            self.collect_targets_and_donors(q, dq_ptr, false, false, &mut counters)
        }) else {
            return (Vec::new(), Vec::new());
        };
        let holes = targets
            .iter()
            .filter(|t| !t.holes.is_empty())
            .map(|t| (t.filename.clone(), hole_spans(t)))
            .collect();
        let dead = dead_volumes_to_borrow(&dest_dir, &targets)
            .into_iter()
            .zip(targets)
            .filter_map(|(left, t)| left.then_some(t.filename))
            .collect();
        (dead, holes)
    }

    /// One live pass; returns the dead volumes it left to the borrow and the holes it took on
    /// (by file), None when there was nothing to look at. The files in `again` are taken on
    /// even though an earlier pass of this repair marked them tried: they have new holes.
    #[allow(clippy::type_complexity)]
    fn run_live_pass(
        &self,
        q: JobQueue<'_>,
        again: &std::collections::BTreeSet<Vec<u8>>,
    ) -> Option<(Vec<Vec<u8>>, Vec<(Vec<u8>, Vec<(i64, i64)>)>)> {
        let mut counters = RunCounters::default();
        let Some((nzb_name, dest_dir, mut targets, donors, mut member_names)) =
            q.host.with_locked_queue(|dq_ptr| {
                self.collect_targets_and_donors(q, dq_ptr, false, true, &mut counters)
            })
        else {
            return None;
        };
        self.restore_lost_heads(q, &dest_dir, &mut targets);

        // A dead volume is offered to the borrow on every run, also after an earlier one could
        // not place it yet: volumes arriving since may let it be placed now. Planning costs no
        // fetches; only the whole-file recreation is not repeated.
        let left_to_borrow = dead_volumes_to_borrow(&dest_dir, &targets);
        for (t, left) in targets.iter_mut().zip(&left_to_borrow) {
            if *left || again.contains(&t.filename) {
                t.patch_eligible = true;
            }
        }
        let attempted: Vec<(Vec<u8>, Vec<(i64, i64)>)> = if donors.is_empty() {
            Vec::new()
        } else {
            targets
                .iter()
                .filter(|t| t.patch_eligible && !t.holes.is_empty())
                .map(|t| (t.filename.clone(), hole_spans(t)))
                .collect()
        };
        let any_eligible = targets.iter().any(|t| t.patch_eligible && t.needs_repair());
        // Dead volumes this pass could place (the rest wait for a later pass, see `PassDead`).
        let mut taken: std::collections::BTreeSet<Vec<u8>> = Default::default();
        if any_eligible && !donors.is_empty() {
            let mut msg = b"Starting live stream repair for ".to_vec();
            msg.extend_from_slice(&nzb_name);
            self.print_message(q, MessageKind::Info, &msg);

            let opts = self.query_host_options();
            // Keep the queue from failing over while the repair works towards borrowing a dead
            // volume: the switch would throw away the extraction already done.
            for t in &targets {
                if let Some(Err(why)) = dead_volume_decision(&dest_dir, t) {
                    let mut msg = b"Not borrowing dead volume ".to_vec();
                    msg.extend_from_slice(&t.filename);
                    msg.extend_from_slice(b": ");
                    msg.extend_from_slice(why.as_bytes());
                    self.print_message(q, MessageKind::Info, &msg);
                }
            }
            let borrowing = left_to_borrow.iter().any(|l| *l);
            let borrow_note = BorrowNote::mark(if borrowing {
                self.pending_data_files(q).1
            } else {
                None
            });
            let wd_handle = self.start_watchdog(q, &opts, &targets);
            self.compute_positional_ranks(&dest_dir, &mut targets, &member_names);
            {
                // Only the dead volumes this pass can place count as taken: one it cannot place
                // yet (an earlier volume is still downloading) stops it once it can be.
                let finished: Vec<&[u8]> = member_names.iter().map(Vec::as_slice).collect();
                let mut g = self.pass_dead.lock().unwrap_or_else(|e| e.into_inner());
                g.taken = left_to_borrow
                    .iter()
                    .zip(&targets)
                    .filter(|(left, t)| **left && placeable_now(&t.filename, &finished))
                    .map(|(_, t)| t.filename.clone())
                    .collect();
                g.borrowing = borrowing;
                g.checked = Some(std::time::Instant::now());
                taken = g.taken.clone();
            }
            self.exec_repair(
                q,
                &opts,
                &dest_dir,
                &mut targets,
                &donors,
                &member_names,
                &mut counters,
                &left_to_borrow,
            );
        
            let any_holes = targets.iter().any(|t| t.patch_eligible && t.needs_repair());
            if any_holes && !self.host_is_stopped(q) {
                // Live repairs fetch with a few workers so they never crowd out the download.
                // Direct unpack stands still until a dead volume is borrowed, so from here on
                // this live repair (the borrow, the recreation fallback and any later pass)
                // fetches with as many workers as the repair after the download does. Once
                // raised it stays raised.
                if borrowing {
                    self.parallel_live_fetch.store(true, Ordering::SeqCst);
                    self.batch_fetcher
                        .set_worker_count(live_borrow_fetch_worker_count(opts.primary_connections));
                }
                self.refresh_live_names(q, &mut targets, &mut member_names);
                self.exec_cross_pack_repair(
                    q,
                    &opts,
                    &dest_dir,
                    &mut targets,
                    &donors,
                    &member_names,
                    &mut counters,
                );
            
            }
            drop(borrow_note);
            self.restore_lost_tails(q, &dest_dir, &mut targets, &member_names);
            // A pass that stopped for a new dead volume leaves the rest to the next one.
            let yielded = self.yielding.load(Ordering::SeqCst);
            *self.pass_dead.lock().unwrap_or_else(|e| e.into_inner()) = PassDead::default();
            // Dead volumes the borrow could not rebuild (a compressed set, a volume that cannot
            // be placed yet, the real last one...) still get the whole-file recreation from a
            // byte-identical duplicate, as before.
            if borrowing && yielded {
                let mut owed = self.recreate_owed.lock().unwrap_or_else(|e| e.into_inner());
                for (t, left) in targets.iter().zip(&left_to_borrow) {
                    if *left && t.needs_repair() {
                        owed.insert(t.filename.clone());
                    }
                }
            }
            if borrowing && !yielded && !self.host_is_stopped(q) {
                self.recreate_unborrowed(
                    q,
                    &opts,
                    &dest_dir,
                    &mut targets,
                    &donors,
                    &member_names,
                    &mut counters,
                    &left_to_borrow,
                );
            
            }
            self.stop_watchdog(
                q,
                wd_handle,
                &nzb_name,
                &targets,
                counters.recovered_bytes,
            );
        
        }

        self.repair_completed_live(q, &targets, &counters);
        // A volume the live repair completed can be read by a paused direct unpack again.
        release_completed_holds(&dest_dir, &targets);
        // A dead `partNN` volume still waiting for an earlier volume does not count as tried: it
        // gets another pass once that volume finishes. Any other one does.
        let waiting = |name: &[u8]| {
            !taken.contains(name) && crate::streamrepair::rar_part_number(name).is_some()
        };
        Some((
            left_to_borrow
                .iter()
                .zip(&targets)
                .filter_map(|(left, t)| (*left && !waiting(&t.filename)).then(|| t.filename.clone()))
                .collect(),
            attempted,
        ))
    }
}

/// A target's holes as (start, end) byte spans.
fn hole_spans(t: &RepairTarget) -> Vec<(i64, i64)> {
    t.holes
        .iter()
        .map(|h| (h.offset, h.offset.saturating_add(h.size)))
        .collect()
}

/// What another live pass is for: whether a file with holes that no pass of this repair took
/// on turned up (the next pass takes it on by itself), and the files a pass took on that have
/// a hole no pass tried (taken on again). Each file is rerun for once per state of its holes
/// (`rerun_for`), so a pass that cannot take them on is not repeated for nothing.
#[allow(clippy::type_complexity)]
fn holes_to_rerun(
    seen: &std::collections::BTreeMap<Vec<u8>, Vec<(i64, i64)>>,
    rerun_for: &mut std::collections::BTreeMap<Vec<u8>, Vec<(i64, i64)>>,
    now: Vec<(Vec<u8>, Vec<(i64, i64)>)>,
) -> (bool, std::collections::BTreeSet<Vec<u8>>) {
    let fresh: Vec<(Vec<u8>, Vec<(i64, i64)>)> = now
        .into_iter()
        .filter(|(name, holes)| rerun_for.get(name) != Some(holes))
        .collect();
    let newcomers: Vec<&(Vec<u8>, Vec<(i64, i64)>)> =
        fresh.iter().filter(|(name, _)| !seen.contains_key(name)).collect();
    let any_new = !newcomers.is_empty();
    let again = holes_not_tried(seen, fresh.clone());
    for (name, holes) in fresh {
        if !seen.contains_key(&name) || again.contains(&name) {
            rerun_for.insert(name, holes);
        }
    }
    (any_new, again)
}

/// Files with a hole that no earlier pass took on: a byte of it lies outside every span tried
/// for that file. What is left of a hole a pass tried and could not fill is not new.
fn holes_not_tried(
    seen: &std::collections::BTreeMap<Vec<u8>, Vec<(i64, i64)>>,
    now: Vec<(Vec<u8>, Vec<(i64, i64)>)>,
) -> std::collections::BTreeSet<Vec<u8>> {
    let covered = |tried: &[(i64, i64)], (start, end): (i64, i64)| {
        let mut spans: Vec<(i64, i64)> = tried.to_vec();
        spans.sort_unstable();
        let mut at = start;
        for (s, e) in spans {
            if s > at {
                break;
            }
            at = at.max(e);
            if at >= end {
                return true;
            }
        }
        at >= end
    };
    now.into_iter()
        .filter(|(name, holes)| {
            seen.get(name)
                .is_some_and(|tried| holes.iter().any(|&h| !covered(tried, h)))
        })
        .map(|(name, _)| name)
        .collect()
}

/// The live pass that is borrowing, as `yield_for_new_dead` sees it.
#[derive(Default)]
struct PassDead {
    /// The pass borrows a dead volume.
    borrowing: bool,
    /// The dead volumes it could place when it started.
    taken: std::collections::BTreeSet<Vec<u8>>,
    /// When the queue was last asked for new ones.
    checked: Option<std::time::Instant>,
}

/// Whether a dead volume can be placed now: every earlier volume of its set is finished (on
/// disk or dead too). A borrow plan walks the set from part 1, so a dead volume behind one
/// still downloading waits; stopping a pass for it would only start the borrow over.
fn placeable_now(name: &[u8], finished: &[&[u8]]) -> bool {
    let Some((prefix, n, width, suffix)) = crate::streamrepair::rar_part_number(name) else {
        return false;
    };
    let done: std::collections::BTreeSet<u32> = finished
        .iter()
        .filter_map(|f| crate::streamrepair::rar_part_number(f))
        .filter(|(p, _, w, x)| {
            p.eq_ignore_ascii_case(&prefix) && *w == width && x.eq_ignore_ascii_case(&suffix)
        })
        .map(|(_, k, _, _)| k)
        .collect();
    (1..n).all(|k| done.contains(&k))
}

/// How often a borrowing live pass asks the queue for volumes that went dead since it started.
const YIELD_CHECK: std::time::Duration = std::time::Duration::from_secs(2);

/// Bounds the passes one live repair stops early for volumes that went dead while it borrowed.
const MAX_LIVE_YIELDS: usize = 32;

/// Bounds the borrow passes one live repair makes for volumes that go dead while it runs.
const MAX_LIVE_BORROW_PASSES: usize = 8;

/// Releases direct unpack's hold on every target a repair left complete.
fn release_completed_holds(dest_dir: &[u8], targets: &[RepairTarget]) {
    #[cfg(unix)]
    for t in targets.iter().filter(|t| !t.needs_repair()) {
        let full = [
            dest_dir,
            &[crate::paths::PATH_SEPARATOR],
            t.filename.as_slice(),
        ]
        .concat();
        crate::postproc::direct_unpack_release_path(&full);
    }
    #[cfg(not(unix))]
    let _ = (dest_dir, targets);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_donor_article_fetched_once_is_kept_for_the_next_wave() {
        let id = b"<first-article-cache-test@x>".to_vec();
        assert!(recent_single(&id).is_none());
        let article = FetchedDonorArticle { success: true, offset: 0, file_size: 10, data: vec![1, 2, 3] };
        keep_single(&id, article.clone());
        assert_eq!(recent_single(&id).map(|a| a.data), Some(vec![1, 2, 3]));
        // Bounded: old ones make way.
        for i in 0..RECENT_SINGLES_MAX {
            keep_single(format!("<filler-{i}@x>").as_bytes(), article.clone());
        }
        assert!(recent_single(&id).is_none());
        assert!(RECENT_SINGLES.lock().unwrap().len() <= RECENT_SINGLES_MAX);
        // Bounded in bytes too, and a huge article is never kept.
        let big = FetchedDonorArticle { success: true, offset: 0, file_size: 1, data: vec![0; RECENT_SINGLES_BYTES / 16 + 1] };
        keep_single(b"<huge@x>", big);
        assert!(recent_single(b"<huge@x>").is_none());
        let chunk = FetchedDonorArticle { success: true, offset: 0, file_size: 1, data: vec![0; RECENT_SINGLES_BYTES / 16] };
        for i in 0..40 {
            keep_single(format!("<chunk-{i}@x>").as_bytes(), chunk.clone());
        }
        let held: usize = RECENT_SINGLES.lock().unwrap().iter().map(|(_, a)| a.data.len()).sum();
        assert!(held <= RECENT_SINGLES_BYTES);
    }

    #[test]
    fn the_fetch_cost_line_tells_network_from_repair_time() {
        let st = DonorFetchStats {
            singles: 3,
            batches: 2,
            batch_parts: 40,
            bytes: 30_000_000,
            waited: std::time::Duration::from_secs(9),
        };
        let line = String::from_utf8(st.line(b"Copy.05", std::time::Duration::from_secs(12), 8)).unwrap();
        assert_eq!(
            line,
            "Fetch cost of borrowing from Copy.05: 30.0 MB in 12.0 s, waiting on the network 9.0 s; 3 single fetch(es), 2 batch(es) of 40 article(s), 8 worker(s)"
        );
    }

    #[test]
    fn a_yield_waits_for_the_dead_volume_being_borrowed() {
        static HOST: crate::articlefetcher::NativeNntpHost = crate::articlefetcher::NativeNntpHost;
        let ctrl = NzbgetRsStreamRepairCtrl::new(true, 1, &HOST, false, false);
        assert!(!ctrl.yield_stops_now());
        ctrl.yielding.store(true, Ordering::SeqCst);
        assert!(ctrl.yield_stops_now());
        // mid-volume: the borrow finishes first, then the pass stops
        ctrl.finishing_volume.store(true, Ordering::SeqCst);
        assert!(!ctrl.yield_stops_now());
        ctrl.finishing_volume.store(false, Ordering::SeqCst);
        assert!(ctrl.yield_stops_now());
    }

    #[test]
    fn a_dead_volume_is_placeable_once_every_earlier_volume_finished() {
        let done: Vec<&[u8]> = vec![b"x.part01.rar", b"x.part02.rar", b"y.part01.rar"];
        assert!(placeable_now(b"x.part03.rar", &done));
        assert!(!placeable_now(b"x.part04.rar", &done));
        // Another set's volumes do not count.
        assert!(!placeable_now(b"y.part03.rar", &done));
        assert!(placeable_now(b"y.part02.rar", &done));
        assert!(!placeable_now(b"movie.mkv", &done));
    }

    #[test]
    fn holes_that_turned_up_during_a_pass_are_taken_on_again() {
        let mut seen = std::collections::BTreeMap::new();
        seen.insert(b"a.part20.rar".to_vec(), vec![(0, 100), (200, 300)]);
        seen.insert(b"a.part21.rar".to_vec(), vec![(0, 50)]);
        let now = vec![
            // What is left of holes already tried: not new.
            (b"a.part20.rar".to_vec(), vec![(10, 90), (200, 250)]),
            // A hole past what the pass saw.
            (b"a.part21.rar".to_vec(), vec![(0, 50), (60, 900)]),
            // Never taken on by this repair: the next pass collects it anyway.
            (b"a.part22.rar".to_vec(), vec![(0, 10)]),
        ];
        let again = holes_not_tried(&seen, now.clone());
        assert_eq!(again.into_iter().collect::<Vec<_>>(), vec![b"a.part21.rar".to_vec()]);
        // part22 finished during the pass: another pass runs for it, and for part21 again.
        let mut rerun_for = std::collections::BTreeMap::new();
        let (newcomers, again) = holes_to_rerun(&seen, &mut rerun_for, now.clone());
        assert!(newcomers);
        assert_eq!(again.into_iter().collect::<Vec<_>>(), vec![b"a.part21.rar".to_vec()]);
        // A pass that could not take them on changes nothing: not run again for them.
        let (newcomers, again) = holes_to_rerun(&seen, &mut rerun_for, now);
        assert!(!newcomers && again.is_empty());
        // Spans tried in two passes cover a hole together.
        let mut seen = std::collections::BTreeMap::new();
        seen.insert(b"b".to_vec(), vec![(50, 100), (0, 60)]);
        assert!(holes_not_tried(&seen, vec![(b"b".to_vec(), vec![(0, 100)])]).is_empty());
    }

    #[test]
    fn a_running_repair_is_marked_until_its_last_mark_drops() {
        let id = -7_301_101;
        assert!(!stream_repair_running(id));
        let a = RunningRepair::mark(id);
        let b = RunningRepair::mark(id);
        assert!(stream_repair_running(id));
        drop(a);
        assert!(stream_repair_running(id));
        drop(b);
        assert!(!stream_repair_running(id));
    }

    fn target(name: &str, decoded: i64, holes: Vec<NzbgetRsByteRange>) -> RepairTarget {
        RepairTarget {
            file_id: 1,
            filename: name.as_bytes().to_vec(),
            decoded_file_size: decoded,
            holes,
            encoded_size: 277_872_640,
            article_count: 388,
            recreated: false,
            failed_size: 0,
            missed_size: 0,
            failed_articles: 388,
            is_par_file: false,
            patch_eligible: true,
            tried_before: false,
            positional_rank: 0,
            positional_window: 0,
            steps_hash: 0,
        }
    }

    /// The decision with DirectUnpack on (tests run without active options).
    fn borrowable_dead_volume(dest_dir: &[u8], t: &RepairTarget) -> bool {
        matches!(dead_volume_decision_with(dest_dir, t, true), Some(Ok(())))
    }

    #[test]
    fn a_dead_volume_not_borrowed_says_why() {
        let dir = b"/dst/dead-volume-reasons-test";
        let dead = target("Movie.part36.rar", 0, vec![]);
        assert_eq!(dead_volume_decision_with(dir, &dead, true), Some(Ok(())));
        assert_eq!(
            dead_volume_decision_with(dir, &dead, false),
            Some(Err("DirectUnpack is off"))
        );
        // A volume an earlier run could not place is offered to the borrow again.
        let mut tried = dead.clone();
        tried.patch_eligible = false;
        tried.tried_before = true;
        assert_eq!(dead_volume_decision_with(dir, &tried, true), Some(Ok(())));
        assert_eq!(
            dead_volume_decision_with(dir, &target("Movie.part01.rar", 0, vec![]), true),
            Some(Err("the first volume cannot be rebuilt"))
        );
        assert_eq!(
            dead_volume_decision_with(dir, &target("Movie.r05", 0, vec![]), true),
            Some(Err("only .partNN.rar volumes can be rebuilt"))
        );
        // Damaged but partly arrived: not a dead volume, nothing to say.
        let hole = vec![NzbgetRsByteRange {
            offset: 0,
            size: 10,
        }];
        assert_eq!(
            dead_volume_decision_with(dir, &target("Movie.part36.rar", 1000, hole), true),
            None
        );
    }

    #[test]
    fn only_dead_volumes_the_borrow_can_rebuild_skip_the_whole_file_recreate() {
        let dir = b"/dst/borrowable-dead-volume-test";
        // The live case: part36 of a stored set, nothing arrived.
        assert!(borrowable_dead_volume(
            dir,
            &target("Movie.part36.rar", 0, vec![])
        ));
        // Holes covering the whole file count as nothing arrived too.
        let whole = vec![NzbgetRsByteRange {
            offset: 0,
            size: 1000,
        }];
        assert!(borrowable_dead_volume(
            dir,
            &target("Movie.part36.rar", 1000, whole)
        ));
        // A damaged volume with a small hole keeps the article-level repair.
        let hole = vec![NzbgetRsByteRange {
            offset: 0,
            size: 10,
        }];
        assert!(!borrowable_dead_volume(
            dir,
            &target("Movie.part36.rar", 1000, hole)
        ));
        // The first volume cannot be rebuilt, old naming and par2 files are not borrowed.
        assert!(!borrowable_dead_volume(
            dir,
            &target("Movie.part01.rar", 0, vec![])
        ));
        assert!(!borrowable_dead_volume(
            dir,
            &target("Movie.r05", 0, vec![])
        ));
        let mut par = target("Movie.vol07+08.par2", 0, vec![]);
        par.is_par_file = true;
        assert!(!borrowable_dead_volume(dir, &par));
        // Held RAR names that the borrow cannot plan stay with the recreation.
        for name in ["Movie.part01.rar", "Movie.r05", "Movie.rar"] {
            let full = [
                dir.as_slice(),
                &[crate::paths::PATH_SEPARATOR],
                name.as_bytes(),
            ]
            .concat();
            crate::postproc::direct_unpack_hold(987_653, &full);
            let held = borrowable_dead_volume(dir, &target(name, 0, vec![]));
            crate::postproc::direct_unpack_forget_holds(987_653);
            assert!(!held, "{name}");
        }
        // An obfuscated name counts once direct unpack holds it.
        let obf = target("a8f3c1d9e2", 0, vec![]);
        assert!(!borrowable_dead_volume(dir, &obf));
        let full = [
            dir.as_slice(),
            &[crate::paths::PATH_SEPARATOR],
            b"a8f3c1d9e2",
        ]
        .concat();
        crate::postproc::direct_unpack_hold(987_654, &full);
        let held = borrowable_dead_volume(dir, &obf);
        crate::postproc::direct_unpack_forget_holds(987_654);
        assert!(held);
        assert!(!borrowable_dead_volume(dir, &obf));
    }

    #[test]
    fn the_borrow_note_is_cleared_when_dropped_even_on_unwind() {
        let id = 987_655;
        {
            let _note = BorrowNote::mark(Some(id));
            assert!(crate::postproc::dead_volume_borrow_underway(id));
        }
        assert!(!crate::postproc::dead_volume_borrow_underway(id));
        let unwound = std::panic::catch_unwind(|| {
            let _note = BorrowNote::mark(Some(id));
            panic!("repair failed");
        });
        assert!(unwound.is_err());
        assert!(!crate::postproc::dead_volume_borrow_underway(id));
        drop(BorrowNote::mark(None));
    }
}
