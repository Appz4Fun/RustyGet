//! End-to-end Rust implementation of `ParChecker` (`daemon/postprocess/ParChecker.cpp`)
//! and live repair-while-downloading powered by `par-rs`.
//!
//! Consolidates:
//! 1. Live streaming repair (`par_rs::streaming::StreamingParSession`) during NNTP download,
//!    with IFSC slice verification, zero-reread syndrome folding, selective `.vol` unpausing,
//!    early cutover (`repair_available_targets_now`), and live `DirectUnpack` splicing
//!    (`try_splice_live_slice`).
//! 2. Pure-memory streaming slice assembly (`par_rs::stream::{StreamingSliceAssembler, SyndromeBank}`).
//! 3. Post-download verification, escalation (more volumes, split fragments, extra/dupe files,
//!    twin sets), and Reed-Solomon repair (`par_rs::repairer::Par2Repairer`).

use crate::parlive::{
    download_writes, reserve_control, resources, ControlLease, SessionBudgetLease,
    SessionGeneration, WriteCoordinator, LIVE_BUDGET,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

#[cfg(test)]
mod live_capture_tests;
#[cfg(test)]
mod live_lifecycle_tests;
mod live_metrics;
pub(crate) mod live_namespace;
#[cfg(test)]
use live_metrics::streaming_session_metrics;

pub use par_rs::damage::{
    blocks_touched, compute_holes, normalize_ranges, par_cannot_cover, presumed_valid_blocks,
    project_par_damage, quick_verify, range_covered, verify_partial_file, verify_success_file_crc,
    ByteRange, DownloadStatus, FileDamage, NoDiskAccess, QuickVerdict, RangeReader,
    Segment as ParSegment,
};
pub use par_rs::filenames::{
    ascii_lower, calc_completed_volume_blocks, calc_queued_volume_blocks, find_main_pars,
    find_main_pars_in_dir, parse_par_filename, pick_twin_volume, same_par_collection,
    select_more_pars, volume_blocks_bytes, volume_filename, volume_number_width, volume_range,
    ParName,
};
pub use par_rs::packet::{
    load_par2_bytes, load_par2_collection, load_par2_file, FileDescPacket, FileId, IfscPacket,
    MainPacket, PacketSet, RecoverySliceRef, ScanOptions, SetId, SliceChecksum,
};
pub use par_rs::renamer::{
    direct_par_name, is_split_fragment, maybe_split_fragment, ParRenamer, RenamerEvent,
    RenamerOptions, RenamerReport,
};
pub use par_rs::repairer::{
    BackupPolicy, FileStatus, Level as ParLogLevel, NullObserver, Par2Repairer, QuickInfo,
    QuickVerifier, RepairError, RepairObserver, RepairOptions, RepairResult, SourceFileState,
    Stage as ParStage, VerifyResult,
};
pub use par_rs::stream::{
    FileLayout, SliceState as StreamSliceState, StreamEvent, StreamStatus, StreamingSliceAssembler,
    SyndromeBank,
};
pub use par_rs::streaming::{
    ArticleOutcome, EarlyRepairReport, LiveIoStats, LiveRepairProgress, LiveRepairStep,
    LiveStatus, QueuedVolume, SessionOptions,
    SliceEvent, SliceState, StreamingParSession, UnpauseAction, VolumeStatus,
};
pub use par_rs::twin::{
    block_size as twin_block_size, block_size_of_set, compare_twin_par2_data, compare_twin_sets,
    fingerprint as twin_fingerprint, name_mapping as twin_name_mapping, par2_set as twin_par2_set,
    sigs_of_set, FileSig, TwinRelation,
};
pub use par_rs::Kernel;

pub const PAR_STAGE_LOADING_PARS: i32 = 0;
pub const PAR_STAGE_VERIFYING_SOURCES: i32 = 1;
pub const PAR_STAGE_REPAIRING: i32 = 2;
pub const PAR_STAGE_VERIFYING_REPAIRED: i32 = 3;

/// The outcome of a par2 verify or repair, with par2cmdline's exit-code meanings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParResult {
    Success,
    RepairPossible,
    RepairNotPossible,
    InvalidArgs,
    InsufficientCriticalData,
    RepairFailed,
    FileIoError,
    LogicError,
    MemoryError,
}

impl ParResult {
    /// par2cmdline's exit code for this outcome, as the debug log prints it.
    pub const fn code(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::RepairPossible => 1,
            Self::RepairNotPossible => 2,
            Self::InvalidArgs => 3,
            Self::InsufficientCriticalData => 4,
            Self::RepairFailed => 5,
            Self::FileIoError => 6,
            Self::LogicError => 7,
            Self::MemoryError => 8,
        }
    }

    /// par2cmdline's description of this outcome.
    pub const fn message(self) -> &'static str {
        match self {
            Self::Success => "OK",
            Self::RepairPossible => {
                "data files are damaged and there is enough recovery data available to repair them"
            }
            Self::RepairNotPossible => "data files are damaged and there is insufficient recovery data available to be able to repair them",
            Self::InvalidArgs => "there was something wrong with the command line arguments",
            Self::InsufficientCriticalData => "the PAR2 files did not contain sufficient information about the data files to be able to verify them",
            Self::RepairFailed => "repair completed but the data files still appear to be damaged",
            Self::FileIoError => "an error occurred when accessing files",
            Self::LogicError => "internal error occurred",
            Self::MemoryError => "out of memory",
        }
    }
}

impl std::fmt::Display for ParResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// Loads a `.par2` index file and any sibling volumes of the same collection in its directory
/// into a [`PacketSet`].
pub fn load_par2_set(main_path: &Path) -> Result<PacketSet, RepairError> {
    let (set, _files) =
        load_par2_collection(main_path, ScanOptions::METADATA).map_err(|source| {
            RepairError::Io {
                context: format!("loading par2 collection of {}", main_path.display()),
                source,
            }
        })?;
    Ok(set)
}

/// Loads a `.par2` packet set directly from in-memory bytes.
pub fn load_par2_set_bytes(bytes: &[u8]) -> Result<PacketSet, std::io::Error> {
    load_par2_bytes(bytes, ScanOptions::METADATA)
}

// =============================================================================================
// 1. Live Streaming Repair Session Registry (`StreamingParSession`)
// =============================================================================================

struct ActiveStreamingSession {
    session: StreamingParSession,
    generation: SessionGeneration,
    writers: Arc<WriteCoordinator>,
    operation: Mutex<()>,
    articles: Mutex<DeferredArticles>,
    /// Grows when free ParBuffer is lent to queued snapshots (`lend_memory`).
    article_byte_limit: AtomicU64,
    /// ParBuffer lent to RAM staging and queued snapshots, at most half of
    /// the pool; returned with the session's budget.
    lent_memory: AtomicU64,
    pending_unpause_ids: Mutex<Vec<u64>>,
    /// A back-fill thread is re-reading verified slices for new slots.
    backfill_running: AtomicBool,
    /// After a back-fill that left slices out (verified from RAM, not on
    /// disk yet), no new one starts before this time.
    backfill_retry_at: Mutex<Option<std::time::Instant>>,
    /// Recovery blocks of registered volumes that have not arrived yet: the
    /// most recovery a damaged block can still get, so growth never charges
    /// ParBuffer for slots no block could fill.
    queued_blocks: Mutex<HashMap<u64, u64>>,
    /// Slot cap last written to the log, so growth is not logged per block.
    logged_slots: AtomicUsize,
    file_lengths: HashMap<FileId, u64>,
    target_names: HashMap<FileId, PathBuf>,
    base_dir: PathBuf,
    par_name: Mutex<Vec<u8>>,
    catchup_source_bytes: AtomicU64,
    metrics: live_metrics::LiveMetricCounters,
    last_error: Mutex<Option<String>>,
    warning_reported: AtomicBool,
    /// Distinct refusal reasons with counts (bounded), so the log says why
    /// captures failed, not only the first failure.
    refusals: Mutex<Vec<(String, u64)>>,
    stop_logged: AtomicBool,
    /// Where each file's bytes are written and the articles that wait for theirs to get
    /// there; see [`CommittedSources`].
    committed: Mutex<CommittedSources>,
    /// Rises with every step a live repair helper takes before par-rs's repair call (the
    /// cache fence passed, a target opened, an article folded by the drain), so a helper
    /// that is busy admitting its repair is not taken for a stuck one.
    admission_progress: AtomicU64,
    // Drop last, after the session, queue and all owned metadata. Grows with
    // the syndrome slots as damage appears (see `grow_for_damage`).
    budget: Mutex<SessionBudgetLease>,
}

/// Live repair's None path: an article whose bytes RAM cannot supply is handed to par-rs
/// without them (`on_article_finished(.., None)`) once they are in the file, and par-rs
/// reads the slice from that file when the slice is complete. The file is the job's
/// DirectWrite output (`<dest>/<id>.out.tmp`) or the completed file under its current
/// (possibly obfuscated) name, given to par-rs with `set_target_path`.
///
/// The None path is the last resort. An article refused while still in the cache (RAM
/// staging full, or the snapshot queue full) waits here until the article cache writes it
/// out, and that write hands it over with its bytes (`streaming_session_articles_written`);
/// an article written before its capture (a flush that ran first, or no cache buffer) is
/// taken from bytes kept at the write. Only past the snapshot byte limit is an article in
/// its file handed over without bytes; one never written before its file completes is
/// covered by the completion replay.
#[derive(Default)]
struct CommittedSources {
    /// The file each queue file's bytes are written into, and whether par-rs knows it.
    paths: HashMap<FileId, (PathBuf, bool)>,
    /// Articles (offset, size, CRC32) waiting for the cache to write them out.
    awaiting: HashMap<FileId, Vec<(u64, u64, u32)>>,
}

struct DeferredArticle {
    file_id: FileId,
    offset: u64,
    crc32: u32,
    size: u64,
    bytes: Option<Vec<u8>>,
    /// Without `bytes`: the article is in its file (par-rs reads it there), not lost.
    committed: bool,
}

// Keep both payload ownership and its reservation through the actual fold.
struct LiveArticleLease {
    article: Option<DeferredArticle>,
    source_slices: u64,
    entry: Arc<ActiveStreamingSession>,
}

fn take_live_article(entry: &Arc<ActiveStreamingSession>) -> Option<LiveArticleLease> {
    let mut queue = entry.articles.lock().unwrap_or_else(|e| e.into_inner());
    let article = queue.pending.pop_front()?;
    let size = entry.session.slice_size();
    let source_slices = if article.bytes.is_some() {
        (article.offset % size)
            .saturating_add(article.size)
            .div_ceil(size)
    } else {
        0
    };
    // Geometry and serialized consumption bound this by the source slice count.
    queue.active_upper_bound += source_slices;
    drop(queue);
    Some(LiveArticleLease {
        article: Some(article),
        source_slices,
        entry: Arc::clone(entry),
    })
}

impl LiveArticleLease {
    fn fold(&self) -> Result<ArticleOutcome, RepairError> {
        let article = self.article.as_ref().unwrap();
        match &article.bytes {
            // In the file as well: par-rs takes the bytes, and may read a slice's gaps there.
            Some(data) if article.committed => self.entry.session.on_article_finished(
                &article.file_id,
                article.offset,
                article.size,
                article.crc32,
                Some(data),
            ),
            Some(data) => self.entry.session.on_article_finished_memory_only(
                &article.file_id,
                article.offset,
                data,
                article.crc32,
            ),
            None if article.committed => self.entry.session.on_article_finished(
                &article.file_id,
                article.offset,
                article.size,
                article.crc32,
                None,
            ),
            None => self
                .entry
                .session
                .on_article_failed_permanently(&article.file_id, article.offset, article.size)
                .map(|(events, unpause)| ArticleOutcome { events, unpause }),
        }
    }
}

impl Drop for LiveArticleLease {
    fn drop(&mut self) {
        let article = self.article.take().unwrap();
        let bytes = article.bytes.as_ref().map_or(0, |data| data.len() as u64);
        drop(article);
        let mut queue = self
            .entry
            .articles
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        queue.bytes -= bytes;
        queue.active_upper_bound -= self.source_slices;
    }
}

#[derive(Default)]
struct DeferredArticles {
    pending: VecDeque<DeferredArticle>,
    // Includes the item currently being folded, until its owned bytes drop, and the
    // stashed bytes below.
    bytes: u64,
    active_upper_bound: u64,
    /// Articles captured from RAM whose cache write is still to come: the write needs to
    /// hand nothing over for them.
    captured: HashSet<(FileId, u64)>,
    /// Bytes of articles written to disk before live PAR captured them (a cache flush that
    /// ran first, or an article written straight to disk with the cache full or off). The
    /// capture takes them here instead of par-rs reading them back from the file. Charged
    /// to the snapshot byte limit like queued snapshots; dropped with the file's completion.
    stash: HashMap<(FileId, u64), Vec<u8>>,
}

/// At most this many captured-article marks and stashed articles per session.
const LIVE_MARK_LIMIT: usize = 65536;
const LIVE_STASH_LIMIT: usize = 4096;

static STREAMING_SESSIONS: LazyLock<Mutex<HashMap<i32, Arc<ActiveStreamingSession>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_LIVE_GENERATION: AtomicU64 = AtomicU64::new(1);

fn live_error(context: &str, message: &str) -> RepairError {
    RepairError::Io {
        context: context.to_owned(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    }
}

pub(crate) fn effective_live_pool_bytes() -> u64 {
    let configured = crate::options::active_options().map_or(500, |o| o.scalars().par_buffer);
    crate::articledl::memory_monitor_effective_par_buffer_mb(configured).max(0) as u64 * 1024 * 1024
}

/// The size of the pool a session's lease draws from: the effective ParBuffer for the
/// daemon's shared pool.
fn lease_pool_bytes(lease: &SessionBudgetLease) -> u64 {
    lease.pool().size().unwrap_or_else(effective_live_pool_bytes)
}

fn acquire_session_budget(bytes: u64, limit: u64) -> Result<SessionBudgetLease, RepairError> {
    LIVE_BUDGET
        .acquire(bytes, limit)
        .ok_or(RepairError::StreamingMemoryLimit {
            context: "shared live PAR reservation",
            required_bytes: bytes,
            limit_bytes: limit,
        })
}

fn build_streaming_session(
    set: &PacketSet,
    base_dir: &Path,
    options: SessionOptions,
    budget: SessionBudgetLease,
    writers: Arc<WriteCoordinator>,
) -> Result<Arc<ActiveStreamingSession>, RepairError> {
    build_streaming_session_with(set, base_dir, options, budget, writers, None)
}

/// What the queue knows when live PAR starts, to size the recovery slots up front
/// ([`live_recovery_plan`]).
#[derive(Clone, Debug, Default)]
pub(crate) struct LivePlanHint {
    /// The set's recovery volumes in the queue.
    pub volumes: Vec<LiveQueuedVolume>,
    /// Source bytes and articles that failed so far.
    pub failed_bytes: u64,
    pub failed_articles: u64,
    /// Source bytes downloaded (good or failed) so far, and in all.
    pub done_bytes: u64,
    pub total_bytes: u64,
}

/// A `.volXX+YY.par2` file in the queue.
#[derive(Clone, Debug)]
pub(crate) struct LiveQueuedVolume {
    pub id: u64,
    pub first_exponent: u32,
    pub count: u32,
    pub size: u64,
    pub articles: u32,
    pub paused: bool,
}

/// Damaged slices expected by the end of the download: the failures so far, projected
/// over the rest of it. Each failed article damages the slices its bytes cover plus at most
/// one more where it straddles a slice boundary.
pub(crate) fn expected_damaged_slices(hint: &LivePlanHint, slice_size: u64) -> u64 {
    if slice_size == 0 {
        return 0;
    }
    let done = hint.done_bytes.max(1);
    let total = hint.total_bytes.max(done);
    let project = |value: u64| {
        u64::try_from(u128::from(value) * u128::from(total) / u128::from(done)).unwrap_or(u64::MAX)
    };
    project(hint.failed_bytes)
        .div_ceil(slice_size)
        .saturating_add(project(hint.failed_articles))
}

/// Recovery slots to plan for `expected` damaged slices: 20% more, at least 2 more. Live
/// folding costs memory bandwidth per slot (slots x slice bytes for every slice that
/// arrives), so the plan follows the expected damage, not the free memory; more damage
/// grows the slots later (`grow_for_damage`).
pub(crate) fn planned_recovery_slots(expected: u64) -> u64 {
    expected
        .saturating_add(expected.div_ceil(5).max(2))
        .min(65535)
}

/// The recovery exponents to give syndrome slots before any source slice arrives, so none
/// is back-filled: those of the volumes the live session's selector unpauses first (paused,
/// fewest blocks first, `select_more_pars`), planned with par-rs's
/// `plan_recovery_articles`, for the expected damage plus margin. RustyGet fetches whole
/// volumes, so each volume is planned whole. Without known volumes, the first exponents
/// (volumes count up from exponent 0).
pub(crate) fn live_recovery_plan(
    hint: &LivePlanHint,
    slice_size: u64,
    loaded: &HashSet<u32>,
) -> par_rs::recovery_plan::RecoveryArticlePlan {
    use par_rs::recovery_plan::{plan_recovery_articles, RecoveryVolume};
    let needed = planned_recovery_slots(expected_damaged_slices(hint, slice_size)) as u32;
    let mut paused: Vec<&LiveQueuedVolume> = hint
        .volumes
        .iter()
        .filter(|v| v.paused && v.count > 0)
        .collect();
    paused.sort_by_key(|v| (v.count, v.id));
    let counts: Vec<i32> = paused
        .iter()
        .map(|v| i32::try_from(v.count).unwrap_or(i32::MAX))
        .collect();
    let (selected, _) = select_more_pars(&counts, i32::try_from(needed).unwrap_or(i32::MAX));
    let volumes: Vec<RecoveryVolume> = selected
        .into_iter()
        .filter_map(|i| paused.get(i))
        .map(|v| {
            let size = v.size.max(1);
            let articles = u64::from(v.articles.max(1)).min(size);
            RecoveryVolume {
                id: v.id,
                first_exponent: v.first_exponent,
                count: v.count,
                layout: None,
                article_ends: (1..=articles).map(|k| size * k / articles).collect(),
                fetched: Vec::new(),
            }
        })
        .collect();
    let mut plan = plan_recovery_articles(&volumes, loaded, needed);
    let mut next = 0u32;
    while plan.exponents.len() < needed as usize && next < 65535 {
        if !loaded.contains(&next) && !plan.exponents.contains(&next) {
            plan.exponents.push(next);
        }
        next += 1;
    }
    plan.exponents.sort_unstable();
    plan.exponents.truncate(needed as usize);
    plan
}

/// Creates the engine sized by a recovery plan (`StreamingParSession::with_recovery_plan`):
/// syndrome slots for the loaded and planned exponents, no more, and the lease shrunk to
/// match; `grow_for_damage` adds slots when damage outgrows the plan.
fn build_planned_streaming_session(
    set: &PacketSet,
    base_dir: &Path,
    options: SessionOptions,
    mut budget: SessionBudgetLease,
    writers: Arc<WriteCoordinator>,
    hint: &LivePlanHint,
) -> Result<Arc<ActiveStreamingSession>, RepairError> {
    let main = set.main.as_ref().ok_or(RepairError::NoMainPacket)?;
    let slice_size = main.slice_size.max(1);
    let loaded: HashSet<u32> = set.recovery.keys().copied().collect();
    let plan = live_recovery_plan(hint, slice_size, &loaded);
    let slots = (loaded.len() + plan.exponents.len()).max(1) as u64;
    let syndrome_ram = slots
        .saturating_mul(slice_size)
        .min(options.max_syndrome_bytes)
        .max(slice_size);
    // The reservation paid two slices (syndrome and output) for every slot the options
    // allowed; give back what the plan does not use, so growth pays for itself.
    let unused = options.max_syndrome_bytes.saturating_sub(syndrome_ram);
    budget.shrink_by(unused.saturating_mul(2));
    build_streaming_session_with(
        set,
        base_dir,
        options,
        budget,
        writers,
        Some((&plan, syndrome_ram)),
    )
}

fn build_streaming_session_with(
    set: &PacketSet,
    base_dir: &Path,
    options: SessionOptions,
    budget: SessionBudgetLease,
    writers: Arc<WriteCoordinator>,
    plan: Option<(&par_rs::recovery_plan::RecoveryArticlePlan, u64)>,
) -> Result<Arc<ActiveStreamingSession>, RepairError> {
    let main = set.main.as_ref().ok_or(RepairError::NoMainPacket)?;
    let mut lengths = HashMap::new();
    let mut target_names = HashMap::new();
    for id in &main.recoverable_file_ids {
        let desc = set
            .file_descs
            .get(id)
            .ok_or_else(|| RepairError::IncompleteMetadata {
                missing_descriptions: vec![*id],
            })?;
        if !set.ifscs.contains_key(id) {
            return Err(live_error("live PAR metadata", "missing slice checksums"));
        }
        lengths.insert(*id, desc.file_length);
        target_names.insert(*id, PathBuf::from(desc.local_name()));
    }
    let generation = NEXT_LIVE_GENERATION
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| live_error("live PAR generation", "exhausted"))?;
    let article_byte_limit = options.max_working_bytes;
    let session = match plan {
        Some((plan, syndrome_ram)) => {
            let slots = usize::try_from(syndrome_ram / main.slice_size.max(1)).unwrap_or(usize::MAX);
            StreamingParSession::with_recovery_plan_limits(
                set,
                base_dir,
                options,
                plan,
                syndrome_ram,
                slots,
            )?
        }
        None => StreamingParSession::new(set, base_dir, options)?,
    };
    Ok(Arc::new(ActiveStreamingSession {
        session,
        generation: SessionGeneration(generation),
        writers,
        budget: Mutex::new(budget),
        operation: Mutex::new(()),
        articles: Mutex::new(DeferredArticles::default()),
        article_byte_limit: AtomicU64::new(article_byte_limit),
        lent_memory: AtomicU64::new(0),
        pending_unpause_ids: Mutex::new(Vec::new()),
        backfill_running: AtomicBool::new(false),
        backfill_retry_at: Mutex::new(None),
        queued_blocks: Mutex::new(HashMap::new()),
        logged_slots: AtomicUsize::new(0),
        file_lengths: lengths,
        target_names,
        base_dir: base_dir.to_path_buf(),
        par_name: Mutex::new(Vec::new()),
        catchup_source_bytes: AtomicU64::new(0),
        metrics: live_metrics::LiveMetricCounters::default(),
        last_error: Mutex::new(None),
        warning_reported: AtomicBool::new(false),
        refusals: Mutex::new(Vec::new()),
        stop_logged: AtomicBool::new(false),
        committed: Mutex::new(CommittedSources::default()),
        admission_progress: AtomicU64::new(0),
    }))
}

fn active_streaming_session(nzb_id: i32) -> Option<Arc<ActiveStreamingSession>> {
    STREAMING_SESSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&nzb_id)
        .cloned()
}

fn install_streaming_session(
    nzb_id: i32,
    entry: Arc<ActiveStreamingSession>,
) -> Result<(), RepairError> {
    let old = entry
        .writers
        .bind_generation(entry.generation, || {
            STREAMING_SESSIONS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(nzb_id, Arc::clone(&entry))
        })
        .map_err(|_| RepairError::Cancelled)?;
    if let Some(old) = old {
        old.session.cancel();
        old.writers.cancel_generation(old.generation);
    }
    entry.observe_count_readiness();
    entry.emit_live_metrics(nzb_id, "started");
    Ok(())
}

fn ensure_current_generation(
    nzb_id: i32,
    entry: &ActiveStreamingSession,
) -> Result<(), RepairError> {
    if !entry.writers.is_current_generation(entry.generation) {
        return Err(RepairError::Cancelled);
    }
    ensure_registry_generation(nzb_id, entry)
}

fn ensure_registry_generation(
    nzb_id: i32,
    entry: &ActiveStreamingSession,
) -> Result<(), RepairError> {
    if entry.session.is_cancelled()
        || active_streaming_session(nzb_id)
            .is_none_or(|current| current.generation != entry.generation)
    {
        return Err(RepairError::Cancelled);
    }
    Ok(())
}

impl ActiveStreamingSession {
    fn note_admission_progress(&self) {
        self.admission_progress.fetch_add(1, Ordering::Relaxed);
    }

    /// File `id`'s bytes are written into `path` now. Only noted (callers may hold the
    /// queue lock); [`Self::apply_target_paths`] tells par-rs.
    fn note_written(&self, id: FileId, path: &Path) {
        let mut sources = self.committed.lock().unwrap_or_else(|e| e.into_inner());
        if sources.paths.get(&id).is_none_or(|(known, _)| known != path) {
            sources.paths.insert(id, (path.to_path_buf(), false));
        }
    }

    /// Tells par-rs where the files noted since last time are (`set_target_path`). It waits
    /// for a repair under way, so callers hold the operation lock (no RustyGet repair runs)
    /// and never call it from a repair's slice callback.
    fn apply_target_paths(&self) -> Result<(), RepairError> {
        let pending: Vec<(FileId, PathBuf)> = self
            .committed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .paths
            .iter()
            .filter(|(_, (_, applied))| !applied)
            .map(|(id, (path, _))| (*id, path.clone()))
            .collect();
        for (id, path) in pending {
            self.session.set_target_path(&id, &path)?;
            let mut sources = self.committed.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((current, applied)) = sources.paths.get_mut(&id) {
                if *current == path {
                    *applied = true;
                }
            }
        }
        Ok(())
    }

    /// The written file of `id` is known.
    fn has_written_file(&self, id: &FileId) -> bool {
        self.committed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .paths
            .contains_key(id)
    }

    /// An article RAM cannot take while it is still in the cache: the cache write hands it
    /// over with the bytes it writes ([`Self::articles_written`]). False when too many wait
    /// (the completion replay covers it then).
    fn await_commit(&self, id: FileId, offset: u64, size: u64, crc32: u32) -> bool {
        let mut sources = self.committed.lock().unwrap_or_else(|e| e.into_inner());
        let awaiting = sources.awaiting.entry(id).or_default();
        if awaiting.len() >= 4096 {
            return false;
        }
        awaiting.push((offset, size, crc32));
        drop(sources);
        self.metrics
            .awaited_commits
            .fetch_add(1, Ordering::Relaxed);
        true
    }

    /// The article cache wrote `items` (offset, bytes) of file `id`, into its DirectWrite file
    /// when `in_file`. Every article live PAR still waits for is handed over with the bytes
    /// written, so par-rs never reads them back: one that waited for the write (RAM could not
    /// take it) is queued with them, one not captured yet is stashed for its capture. A copy
    /// is taken only within the snapshot byte limit (ParBuffer); past it, an article in its
    /// file is handed over without bytes (par-rs reads it there), any other is left to the
    /// completion replay. Copies only, no I/O: the cache thread calls it after its write.
    /// This is the one place a cache write meets live PAR.
    fn articles_written(&self, id: FileId, in_file: bool, items: &[(u64, &[u8])]) -> usize {
        let in_file = in_file && self.has_written_file(&id);
        let mut ready: Vec<(u64, u64, u32)> = Vec::new();
        {
            let mut sources = self.committed.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(awaiting) = sources.awaiting.get_mut(&id) {
                awaiting.retain(|&(o, n, crc)| {
                    let written = items
                        .iter()
                        .any(|&(offset, bytes)| offset == o && bytes.len() as u64 == n);
                    if written {
                        ready.push((o, n, crc));
                    }
                    !written
                });
            }
        }
        let mut queue = self.articles.lock().unwrap_or_else(|e| e.into_inner());
        let limit = self.article_byte_limit.load(Ordering::Acquire);
        let mut handed = 0;
        for &(offset, bytes) in items {
            let size = bytes.len() as u64;
            if size == 0 || queue.captured.remove(&(id, offset)) {
                continue;
            }
            let room = queue.bytes.checked_add(size).is_some_and(|n| n <= limit);
            if let Some(&(_, _, crc32)) = ready.iter().find(|&&(o, _, _)| o == offset) {
                if queue.pending.len() >= 4096 || (!room && !in_file) {
                    continue; // the completion replay covers it
                }
                let copy = room.then(|| bytes.to_vec());
                if copy.is_some() {
                    queue.bytes += size;
                    self.metrics.captured_bytes.fetch_add(size, Ordering::Relaxed);
                    self.metrics.written_handoffs.fetch_add(1, Ordering::Relaxed);
                }
                queue.pending.push_back(DeferredArticle {
                    file_id: id,
                    offset,
                    crc32,
                    size,
                    bytes: copy,
                    committed: in_file,
                });
                handed += 1;
            } else if room && queue.stash.len() < LIVE_STASH_LIMIT {
                if let Some(old) = queue.stash.insert((id, offset), bytes.to_vec()) {
                    queue.bytes -= old.len() as u64;
                }
                queue.bytes += size;
            }
        }
        handed
    }

    /// Takes the stashed bytes of the article of `id` at `offset` of `size` bytes, if any.
    fn take_stashed(
        queue: &mut DeferredArticles,
        id: FileId,
        offset: u64,
        size: u64,
    ) -> Option<Vec<u8>> {
        let bytes = queue.stash.remove(&(id, offset))?;
        if bytes.len() as u64 == size {
            return Some(bytes); // stays charged: it moves into the queue
        }
        queue.bytes -= bytes.len() as u64;
        None
    }

    /// Stashes `bytes` of the article of `id` at `offset`, already charged to the queue.
    fn stash_charged(&self, id: FileId, offset: u64, bytes: Vec<u8>, charged: u64) {
        let mut queue = self.articles.lock().unwrap_or_else(|e| e.into_inner());
        queue.bytes -= charged;
        let size = bytes.len() as u64;
        if size == 0
            || queue.captured.contains(&(id, offset))
            || queue.stash.len() >= LIVE_STASH_LIMIT
            || queue.bytes.checked_add(size).is_none()
        {
            return;
        }
        if let Some(old) = queue.stash.insert((id, offset), bytes) {
            queue.bytes -= old.len() as u64;
        }
        queue.bytes += size;
    }

    /// File `id` completed: its completion replay covers what waits, and par-rs reads it at
    /// `path` from now on.
    fn file_completed(&self, id: FileId, path: &Path) {
        let mut sources = self.committed.lock().unwrap_or_else(|e| e.into_inner());
        sources.awaiting.remove(&id);
        sources.paths.insert(id, (path.to_path_buf(), false));
        drop(sources);
        let mut queue = self.articles.lock().unwrap_or_else(|e| e.into_inner());
        queue.captured.retain(|(file, _)| *file != id);
        let mut freed = 0;
        queue.stash.retain(|(file, _), bytes| {
            let keep = *file != id;
            if !keep {
                freed += bytes.len() as u64;
            }
            keep
        });
        queue.bytes -= freed;
    }

    /// Articles still waiting for the cache, for the log.
    fn awaiting_commits(&self) -> usize {
        self.committed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .awaiting
            .values()
            .map(Vec::len)
            .sum()
    }

    /// A back-fill is pending, none runs, and no retry delay is in force.    /// A back-fill is pending, none runs, and no retry delay is in force.
    fn backfill_due(&self) -> bool {
        self.session.needs_backfill()
            && !self.backfill_running.load(Ordering::Acquire)
            && self
                .backfill_retry_at
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_none_or(|at| std::time::Instant::now() >= at)
    }

    fn record<T>(&self, result: Result<T, RepairError>) -> Result<T, RepairError> {
        if let Err(error) = &result {
            *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.to_string());
            self.note_refusal(error);
            self.lend_memory(error);
        } else {
            self.observe_count_readiness();
        }
        result
    }
    /// Count a refusal by reason; log each new reason once as it first appears.
    fn note_refusal(&self, error: &RepairError) {
        const MAX_REASONS: usize = 8;
        let mut reason = error.to_string();
        if matches!(error, RepairError::UnknownFile(FileId(id)) if *id == [0; 16]) {
            // live_source_id's "no binding yet": an obfuscated queue file
            // whose real name and 16k hash are not known yet.
            reason = "queue file not matched to a par2 file yet".to_owned();
        }
        if matches!(error, RepairError::Cancelled) {
            if let Some(why) = self.writers.stop_reason() {
                reason = format!("{reason} ({why})");
            }
        }
        // Key without numbers, so byte counts and ids in one kind of error
        // do not each take a slot; the first full message is logged.
        let mut key = String::with_capacity(reason.len());
        for c in reason.chars() {
            if !c.is_ascii_digit() {
                key.push(c);
            } else if !key.ends_with('#') {
                key.push('#');
            }
        }
        let mut refusals = self.refusals.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, count)) = refusals.iter_mut().find(|(known, _)| *known == key) {
            *count += 1;
            return;
        }
        if refusals.len() >= MAX_REASONS {
            let other = "other reasons".to_owned();
            match refusals.iter_mut().find(|(known, _)| *known == other) {
                Some((_, count)) => *count += 1,
                None => refusals.push((other, 1)),
            }
            return;
        }
        let message = format!(
            "Live PAR (job {}) refused: {reason}",
            self.metrics.report_nzb_id()
        );
        refusals.push((key, 1));
        drop(refusals);
        crate::log::log_active_bytes(crate::queuesubsys_host::MK_DETAIL, message.as_bytes());
    }

    /// RAM staging or the snapshot queue ran out while many files arrive at
    /// once: lend it free ParBuffer, up to half of the pool, so later
    /// articles are verified from RAM instead of being replayed
    /// from disk. A refused staging article is folded again at once when the
    /// loan makes room for it (`drain_live_articles_locked`); otherwise it is
    /// replayed when its file completes. The memory returns with the
    /// session's budget. Returns whether the limit now holds `required_bytes`.
    fn lend_memory(&self, error: &RepairError) -> bool {
        let RepairError::StreamingMemoryLimit {
            context,
            required_bytes,
            limit_bytes,
        } = error
        else {
            return false;
        };
        let staging = *context == "RAM-only partial staging";
        // Only a byte shortfall: the snapshot queue's article-count limit
        // reports the same context, and memory cannot relieve it. A staging
        // cap of 0 means staging was turned off on purpose. Replay payloads
        // share the snapshot queue's byte limit.
        if (!staging && *context != "live article snapshots" && *context != "live replay payload")
            || required_bytes <= limit_bytes
            || (staging && self.session.max_staging_bytes() == 0)
            || self.session.is_cancelled()
        {
            return false;
        }
        let unit = self.session.slice_size();
        // Callers may hold the daemon queue lock; growth of repair slots holds
        // the budget across volume absorbs, so never wait for it here.
        let Ok(mut lease) = self.budget.try_lock() else {
            return false;
        };
        let pool = lease_pool_bytes(&lease);
        let before = self.lent_memory.load(Ordering::Relaxed);
        if unit == 0 {
            return false;
        }
        // Cover the whole shortfall at once: every refusal drops data. With
        // many files downloading at once staging holds the received part of
        // one partial slice per file (par-rs charges bytes held, not whole
        // slices), so loans may take up to half of ParBuffer. They never take
        // the last quarter, which stays free for repair slots; when the pool
        // has room for two ordinary jobs, it is at least one ordinary job's
        // start reservation, so another job's live PAR can still start (a job
        // with very large slices may reserve more than that).
        let wanted = (required_bytes - limit_bytes).div_ceil(unit);
        let room = (pool / 2).saturating_sub(before) / unit;
        let ceiling = crate::parlive::JOB_CEILING;
        let keep_free = if pool >= 2 * ceiling {
            (pool / 4).max(ceiling)
        } else {
            pool / 4
        };
        let units = lease.grow_by_units(unit, wanted.min(room), pool.saturating_sub(keep_free));
        if units == 0 {
            return false;
        }
        let grant = units * unit;
        let lent = before + grant;
        self.lent_memory.store(lent, Ordering::Relaxed);
        let limit = if staging {
            let limit = self.session.max_staging_bytes().saturating_add(grant);
            self.session.set_max_staging_bytes(limit);
            limit
        } else {
            self.article_byte_limit.fetch_add(grant, Ordering::AcqRel) + grant
        };
        drop(lease);
        // Log the first loan, then each doubling.
        if before / unit == 0 || (lent / unit).ilog2() > (before / unit).ilog2() {
            let name = self.par_name.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let msg = format!(
                "Live PAR for {}: staging memory grew by {} MB ({} MB of {} MB ParBuffer lent) for many files arriving at once",
                String::from_utf8_lossy(&name),
                grant.div_ceil(1024 * 1024),
                lent.div_ceil(1024 * 1024),
                pool / (1024 * 1024),
            );
            crate::log::log_active_bytes(crate::queuesubsys_host::MK_DETAIL, msg.as_bytes());
        }
        limit >= *required_bytes
    }

    /// Log once why this session stopped taking work.
    fn note_stop(&self, nzb_id: i32, why: &str) {
        if self.session.is_cancelled() || self.stop_logged.swap(true, Ordering::Relaxed) {
            return;
        }
        let message = format!("Live PAR (job {nzb_id}) stopped: {why}");
        crate::log::log_active_bytes(crate::queuesubsys_host::MK_DETAIL, message.as_bytes());
    }

    /// The refusal counts, for the disposal footer.
    fn refusal_summary(&self) -> Option<String> {
        let refusals = self.refusals.lock().unwrap_or_else(|e| e.into_inner());
        (!refusals.is_empty()).then(|| {
            // Each reason above shows its first message, from before any loan;
            // the final limits tell whether loans kept up.
            let mb = |bytes: u64| bytes.div_ceil(1024 * 1024);
            format!(
                "{} (final limits: staging {} MB, article snapshots {} MB, {} MB of ParBuffer lent)",
                refusals
                    .iter()
                    .map(|(reason, count)| format!("{count} x {reason}"))
                    .collect::<Vec<_>>()
                    .join("; "),
                mb(self.session.max_staging_bytes()),
                mb(self.article_byte_limit.load(Ordering::Relaxed)),
                mb(self.lent_memory.load(Ordering::Relaxed)),
            )
        })
    }

    /// Grow the syndrome slots so every damaged block can take a recovery
    /// block, paid from free ParBuffer: each slot costs two slices (its
    /// syndrome and its reconstructed output). The memory goes back to the
    /// pool when the session ends.
    fn grow_for_damage(&self) {
        let status = self.session.status();
        let coming: u64 = self
            .queued_blocks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .sum();
        let turned_away = self.session.turned_away_recovery_slices();
        // Never more slots than recovery blocks that are here, turned away by
        // the cap (absorbed again on a raise) or still coming.
        let usable = status
            .ready_recovery
            .saturating_add(turned_away)
            .saturating_add(usize::try_from(coming).unwrap_or(usize::MAX));
        let mut want = status.damaged.min(usable);
        // Every slot is taken but blocks are still short: a preallocated slot
        // held an exponent that did not come. Grow into what was turned away.
        let cap = self.session.max_syndrome_slots();
        if status.ready_recovery < status.damaged && status.syndrome_slots >= cap {
            want = want.max(cap + (status.damaged - status.ready_recovery).min(turned_away));
        }
        self.grow_slots_to(want, &status);
    }

    fn grow_slots_to(&self, want: usize, status: &LiveStatus) {
        if self.session.is_cancelled() || want <= self.session.max_syndrome_slots() {
            return;
        }
        let unit = self.session.slice_size().saturating_mul(2);
        let mut lease = self.budget.lock().unwrap_or_else(|e| e.into_inner());
        let cap = self.session.max_syndrome_slots();
        if want <= cap {
            return;
        }
        let pool = lease_pool_bytes(&lease);
        let granted = lease.grow_by_units(unit, (want - cap) as u64, pool);
        if granted == 0 {
            return;
        }
        let asked = cap + granted as usize;
        let result = self.session.set_max_syndrome_slots(asked);
        let effective = self.session.max_syndrome_slots().max(cap);
        if effective < asked {
            lease.shrink_by(unit * (asked - effective) as u64);
        }
        let logged = self.logged_slots.load(Ordering::Relaxed);
        // Log the first growth, then each further quarter (at least 4 blocks).
        if effective > cap && (logged == 0 || effective >= logged + (logged / 4).max(4)) {
            self.logged_slots.store(effective, Ordering::Relaxed);
            let name = self
                .par_name
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let msg = format!(
                "Live PAR for {}: repair memory grew to {} blocks ({} MB of {} MB ParBuffer) for {} damaged blocks",
                String::from_utf8_lossy(&name),
                effective,
                (effective as u64 * unit).div_ceil(1024 * 1024),
                pool / (1024 * 1024),
                status.damaged,
            );
            crate::log::log_active_bytes(crate::queuesubsys_host::MK_INFO, msg.as_bytes());
        }
        if let Err(error) = result {
            *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.to_string());
        }
    }

    fn unpause(&self, action: Option<&UnpauseAction>) {
        // Every outcome may carry new damage, also when volumes already in
        // flight cover it and no unpause is due.
        self.grow_for_damage();
        if let Some(action) = action {
            let mut ids = self
                .pending_unpause_ids
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            for id in &action.volume_ids_to_unpause {
                if !ids.contains(id) {
                    ids.push(*id);
                }
            }
        }
    }
}

/// Wall time live PAR spent on each job's work, verifying and repairing, kept past the
/// session (a job may have several) until the job reaches history
/// (`postproc::live_par_time_settle`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LiveParTime {
    verify: std::time::Duration,
    repair: std::time::Duration,
    /// Whole seconds of par time (verify + repair) and of repair already moved into the
    /// job's saved times by queue checkpoints.
    taken_par: u64,
    taken_repair: u64,
}

static LIVE_PAR_TIME: LazyLock<Mutex<HashMap<i32, LiveParTime>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

thread_local! {
    static LIVE_PAR_TIMING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl ActiveStreamingSession {
    /// Runs live PAR work for `nzb_id` and charges its wall time to the job: the part
    /// par-rs spent in target repair calls is repair, the rest verification. Nested
    /// calls are charged by the outermost one only.
    fn timed<T>(&self, nzb_id: i32, f: impl FnOnce() -> T) -> T {
        if LIVE_PAR_TIMING.with(|timing| timing.replace(true)) {
            return f();
        }
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                LIVE_PAR_TIMING.with(|timing| timing.set(false));
            }
        }
        let _reset = Reset;
        let started = std::time::Instant::now();
        let before = self.session.repair_phase_stats();
        let result = f();
        let elapsed = started.elapsed();
        let after = self.session.repair_phase_stats();
        let (verify, repair) = split_live_time(
            elapsed,
            after.total_ns.saturating_sub(before.total_ns),
            after.payload_wait_ns.saturating_sub(before.payload_wait_ns),
        );
        let mut times = LIVE_PAR_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let time = times.entry(nzb_id).or_default();
        time.verify += verify;
        time.repair += repair;
        result
    }
}

/// Splits an operation's wall time into verification and repair: par-rs's target
/// repair calls (`repair_ns`) are repair, except their wait for exclusive payload
/// admission (`wait_ns`), which is neither; the rest is verification.
fn split_live_time(
    elapsed: std::time::Duration,
    repair_ns: u64,
    wait_ns: u64,
) -> (std::time::Duration, std::time::Duration) {
    let in_repair = std::time::Duration::from_nanos(repair_ns).min(elapsed);
    let wait = std::time::Duration::from_nanos(wait_ns).min(in_repair);
    (elapsed - in_repair, in_repair - wait)
}

/// Takes the seconds live PAR spent on job `nzb_id` since the last take: verifying and
/// repairing (par time, which includes the repair) and repairing alone.
pub(crate) fn streaming_session_take_par_secs(nzb_id: i32) -> (i32, i32) {
    let time = LIVE_PAR_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&nzb_id)
        .unwrap_or_default();
    let secs = |d: std::time::Duration, taken: u64| {
        (d.as_secs_f64().round() as u64)
            .saturating_sub(taken)
            .min(i32::MAX as u64) as i32
    };
    (
        secs(time.verify + time.repair, time.taken_par),
        secs(time.repair, time.taken_repair),
    )
}

/// Whether job `nzb_id` has live PAR time not yet in its times.
pub(crate) fn live_par_time_pending(nzb_id: i32) -> bool {
    LIVE_PAR_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&nzb_id)
        .is_some_and(|time| (time.verify + time.repair).as_secs() > time.taken_par)
}

/// Takes only the whole seconds of job `nzb_id`'s live PAR time (par time, which
/// includes the repair, and repair alone) and keeps the fraction for later, so
/// repeated checkpoints neither lose nor add time.
pub(crate) fn streaming_session_take_whole_par_secs(nzb_id: i32) -> (i32, i32) {
    let mut times = LIVE_PAR_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let Some(time) = times.get_mut(&nzb_id) else {
        return (0, 0);
    };
    // Measured time is never reduced; only the whole seconds not yet taken move, so the
    // repair seconds can never pass the par seconds and no fraction is lost.
    let par = (time.verify + time.repair).as_secs().saturating_sub(time.taken_par);
    let repair = time.repair.as_secs().saturating_sub(time.taken_repair);
    time.taken_par += par;
    time.taken_repair += repair;
    (par.min(i32::MAX as u64) as i32, repair.min(i32::MAX as u64) as i32)
}

/// Charges live PAR time to job `nzb_id` as if it had been measured.
#[cfg(test)]
pub(crate) fn live_par_time_add_for_test(nzb_id: i32, verify_ms: u64, repair_ms: u64) {
    let mut times = LIVE_PAR_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let time = times.entry(nzb_id).or_default();
    time.verify += std::time::Duration::from_millis(verify_ms);
    time.repair += std::time::Duration::from_millis(repair_ms);
}

fn with_live_session<T>(
    nzb_id: i32,
    f: impl FnOnce(&ActiveStreamingSession) -> Result<T, RepairError>,
) -> Option<Result<T, RepairError>> {
    let entry = active_streaming_session(nzb_id)?;
    Some(with_live_entry(nzb_id, &entry, f))
}

fn with_live_entry<T>(
    nzb_id: i32,
    entry: &Arc<ActiveStreamingSession>,
    f: impl FnOnce(&ActiveStreamingSession) -> Result<T, RepairError>,
) -> Result<T, RepairError> {
    let _operation = entry.operation.lock().unwrap_or_else(|e| e.into_inner());
    entry.timed(nzb_id, || {
        entry.record((|| {
            ensure_current_generation(nzb_id, entry)?;
            let result = f(entry)?;
            ensure_current_generation(nzb_id, entry)?;
            Ok(result)
        })())
    })
}

/// Reserve before taking an owned snapshot. This capture path performs no source I/O,
/// hashing or GF work and may be called while the daemon queue is locked.
#[cfg(test)]
pub(crate) fn streaming_session_defer_article(
    nzb_id: i32,
    identity: (&[u8], &[u8]),
    offset: u64,
    size: u64,
    crc32: u32,
    snapshot: impl FnOnce() -> Option<Vec<u8>>,
) -> Option<Result<(), RepairError>> {
    streaming_session_defer_article_for_queue(
        nzb_id,
        (None, identity.0, identity.1),
        offset,
        size,
        crc32,
        snapshot,
    )
}

#[cfg(test)]
pub(crate) fn streaming_session_defer_article_for_queue(
    nzb_id: i32,
    identity: (Option<i32>, &[u8], &[u8]),
    offset: u64,
    size: u64,
    crc32: u32,
    snapshot: impl FnOnce() -> Option<Vec<u8>>,
) -> Option<Result<(), RepairError>> {
    streaming_session_defer_committed_article(
        nzb_id, identity, offset, size, crc32, None, snapshot,
    )
}

/// Captures a finished article for live PAR, with `written`: the file the article's
/// bytes were written into (the job's DirectWrite file). When RAM cannot supply the bytes
/// (the article cache dropped them, or the snapshot queue is full) the article's slices are
/// parked and read back whole from that file once every byte of them is on disk, instead of
/// refusing the article. Does no I/O: the caller may hold the daemon queue lock.
#[allow(clippy::too_many_arguments)]
pub(crate) fn streaming_session_defer_committed_article(
    nzb_id: i32,
    identity: (Option<i32>, &[u8], &[u8]),
    offset: u64,
    size: u64,
    crc32: u32,
    written: Option<&Path>,
    snapshot: impl FnOnce() -> Option<Vec<u8>>,
) -> Option<Result<(), RepairError>> {
    let entry = active_streaming_session(nzb_id)?;
    let result = (|| {
        ensure_current_generation(nzb_id, &entry)?;
        let id = live_source_id(&entry, identity.0, identity.1, identity.2)?;
        let length = entry.file_lengths[&id];
        if size == 0 || offset.checked_add(size).is_none_or(|end| end > length) {
            return Err(live_error(
                "live article snapshot",
                "invalid accepted range",
            ));
        }
        if let Some(path) = written {
            entry.note_written(id, path);
        }
        let mut queue = entry.articles.lock().unwrap_or_else(|e| e.into_inner());
        // Written before this capture: its bytes were kept aside then, already charged.
        if queue.pending.len() < 4096 {
            if let Some(bytes) = ActiveStreamingSession::take_stashed(&mut queue, id, offset, size) {
                queue.pending.push_back(DeferredArticle {
                    file_id: id,
                    offset,
                    crc32,
                    size,
                    bytes: Some(bytes),
                    committed: entry.has_written_file(&id),
                });
                // Not in the article cache: still a cache miss, but nothing is read back.
                entry.metrics.cache_misses.fetch_add(1, Ordering::Relaxed);
                entry
                    .metrics
                    .stashed_articles
                    .fetch_add(1, Ordering::Relaxed);
                entry
                    .metrics
                    .captured_bytes
                    .fetch_add(size, Ordering::Relaxed);
                return Ok(());
            }
        }
        let next = queue.bytes.checked_add(size);
        let mut limit = entry.article_byte_limit.load(Ordering::Acquire);
        // Many files arriving at once fill the first byte limit; a loan of free
        // ParBuffer that covers the shortfall takes the snapshot, not a refusal.
        if let Some(required_bytes) = next.filter(|&n| n > limit && queue.pending.len() < 4096) {
            let shortfall = RepairError::StreamingMemoryLimit {
                context: "live article snapshots",
                required_bytes,
                limit_bytes: limit,
            };
            // A short loan still raises the limit, so the refusal reports the new one.
            entry.lend_memory(&shortfall);
            limit = entry.article_byte_limit.load(Ordering::Acquire);
        }
        if queue.pending.len() >= 4096 || next.is_none_or(|n| n > limit) {
            drop(queue);
            if entry.await_commit(id, offset, size, crc32) {
                return Ok(());
            }
            return Err(RepairError::StreamingMemoryLimit {
                context: "live article snapshots",
                required_bytes: next.unwrap_or(u64::MAX),
                limit_bytes: limit,
            });
        }
        let Some(bytes) = snapshot() else {
            entry.metrics.cache_misses.fetch_add(1, Ordering::Relaxed);
            // Not in the cache any more: the article reached its file. par-rs reads it
            // there once its slice is complete.
            if entry.has_written_file(&id) && queue.pending.len() < 4096 {
                queue.pending.push_back(DeferredArticle {
                    file_id: id,
                    offset,
                    crc32,
                    size,
                    bytes: None,
                    committed: true,
                });
                entry
                    .metrics
                    .committed_articles
                    .fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
            drop(queue);
            return Err(live_error(
                "live article cache",
                "cache bytes unavailable; committed replay required",
            ));
        };
        if bytes.len() as u64 != size || bytes.capacity() as u64 > size {
            return Err(live_error(
                "live article snapshot",
                "snapshot size differs from admitted payload",
            ));
        }
        ensure_current_generation(nzb_id, &entry)?;
        queue.bytes = next.unwrap();
        // Bytes kept aside for it (a write that ran first) are not needed any more; else
        // its coming cache write has nothing to hand over.
        if let Some(old) = queue.stash.remove(&(id, offset)) {
            queue.bytes -= old.len() as u64;
        } else if queue.captured.len() < LIVE_MARK_LIMIT {
            queue.captured.insert((id, offset));
        }
        queue.pending.push_back(DeferredArticle {
            file_id: id,
            offset,
            crc32,
            size,
            bytes: Some(bytes),
            committed: false,
        });
        entry
            .metrics
            .captured_articles
            .fetch_add(1, Ordering::Relaxed);
        entry
            .metrics
            .captured_bytes
            .fetch_add(size, Ordering::Relaxed);
        Ok(())
    })();
    if result.is_err() {
        entry
            .metrics
            .capture_refusals
            .fetch_add(1, Ordering::Relaxed);
    }
    Some(entry.record(result))
}

/// Capture a permanent failure without waiting for a hash/fold operation.
#[cfg(test)]
pub(crate) fn streaming_session_defer_failure(
    nzb_id: i32,
    identity: (&[u8], &[u8]),
    offset: u64,
    size: u64,
) -> Option<Result<(), RepairError>> {
    streaming_session_defer_failure_for_queue(nzb_id, (None, identity.0, identity.1), offset, size)
}

pub(crate) fn streaming_session_defer_failure_for_queue(
    nzb_id: i32,
    identity: (Option<i32>, &[u8], &[u8]),
    offset: u64,
    size: u64,
) -> Option<Result<(), RepairError>> {
    let entry = active_streaming_session(nzb_id)?;
    Some(entry.record((|| {
        ensure_current_generation(nzb_id, &entry)?;
        let id = live_source_id(&entry, identity.0, identity.1, identity.2)?;
        if size == 0
            || offset
                .checked_add(size)
                .is_none_or(|end| end > entry.file_lengths[&id])
        {
            return Err(live_error(
                "live failure snapshot",
                "invalid accepted range",
            ));
        }
        let mut queue = entry.articles.lock().unwrap_or_else(|e| e.into_inner());
        if queue.pending.len() >= 4096 {
            return Err(live_error(
                "live failure snapshot",
                "event limit reached; committed replay required",
            ));
        }
        queue.pending.push_back(DeferredArticle {
            file_id: id,
            offset,
            size,
            crc32: 0,
            bytes: None,
            committed: false,
        });
        Ok(())
    })()))
}

/// Surface one fallback warning per generation rather than flooding the log for
/// each cache miss. The detailed last error remains available separately.
pub(crate) fn streaming_session_take_warning(nzb_id: i32) -> Option<String> {
    let entry = active_streaming_session(nzb_id)?;
    if entry.session.is_cancelled() {
        return None;
    }
    let warning = entry
        .last_error
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()?;
    entry
        .warning_reported
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .ok()
        .map(|_| warning)
}

/// Consume owned snapshots outside the daemon queue lock. Another active drain
/// retains ownership; the coordinator will retry on its next tick.
pub(crate) fn streaming_session_drain_articles(nzb_id: i32) -> Option<Result<usize, RepairError>> {
    let entry = active_streaming_session(nzb_id)?;
    let _operation = match entry.operation.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return Some(Ok(0)),
    };
    Some(drain_live_articles_locked(nzb_id, &entry))
}

fn drain_live_articles_locked(
    nzb_id: i32,
    entry: &Arc<ActiveStreamingSession>,
) -> Result<usize, RepairError> {
    entry.record((|| {
        let mut count = 0;
        entry.apply_target_paths()?;
        loop {
            ensure_current_generation(nzb_id, entry)?;
            let Some(article) = take_live_article(entry) else {
                return Ok(count);
            };
            let mut result = entry.timed(nzb_id, || article.fold());
            // Staging that a loan of free ParBuffer enlarges enough takes the article after
            // all: not a refusal. A shortfall no loan covers parks the article's slices,
            // read back whole from its file once complete; without a known file it is
            // refused and reported.
            if matches!(&result, Err(error) if entry.lend_memory(error)) {
                result = entry.timed(nzb_id, || article.fold());
            }
            let parked = matches!(&result, Err(RepairError::StreamingMemoryLimit { .. }))
                && article.article.as_ref().is_some_and(|a| {
                    a.bytes.is_some() && entry.await_commit(a.file_id, a.offset, a.size, a.crc32)
                });
            // The file was renamed (completed) before par-rs learned its new path: an article
            // read from its file, or a slice par-rs completes from the file, finds nothing
            // there. Its completion replay covers it: no warning, and the drain goes on.
            let renamed = article
                .article
                .as_ref()
                .is_some_and(|a| renamed_away(&result, a.bytes.is_none() && a.committed));
            let parked = parked || renamed;
            drop(article);
            if parked {
                count += 1;
                continue;
            }
            let outcome = result?;
            ensure_current_generation(nzb_id, entry)?;
            entry.unpause(outcome.unpause.as_ref());
            entry.note_admission_progress();
            count += 1;
        }
    })())
}

/// An article's fold failed only because its file was renamed away: any I/O error for one
/// par-rs reads from its file (`committed`), a missing file for one from RAM (par-rs may
/// complete its slice from the file).
fn renamed_away<T>(result: &Result<T, RepairError>, committed: bool) -> bool {
    match result {
        Err(RepairError::Io { source, .. }) => {
            committed || source.kind() == std::io::ErrorKind::NotFound
        }
        _ => false,
    }
}

/// Queue file `identity` of job `nzb_id` is at `path` now (completed, possibly renamed since):
/// par-rs reads and back-fills it there (`set_target_path`, applied before the next fold or
/// repair). Metadata only; callers may hold the queue lock.
pub(crate) fn streaming_session_note_file_path(
    nzb_id: i32,
    identity: (Option<i32>, &[u8], &[u8]),
    path: &Path,
) {
    let Some(entry) = active_streaming_session(nzb_id) else {
        return;
    };
    if let Ok(id) = live_source_id(&entry, identity.0, identity.1, identity.2) {
        entry.note_written(id, path);
    }
}

/// The article cache wrote `items` (offset, bytes) of a queue file, into its DirectWrite
/// file when `in_file` (else into article temporaries). Live PAR takes what it still needs
/// of them from these bytes, never from the file (see `articles_written`). No I/O; the
/// cache thread calls it after its write. Returns how many waiting articles were queued.
pub(crate) fn streaming_session_articles_written(
    nzb_id: i32,
    identity: (Option<i32>, &[u8], &[u8]),
    in_file: bool,
    items: &[(u64, &[u8])],
) -> usize {
    let Some(entry) = active_streaming_session(nzb_id) else {
        return 0;
    };
    if entry.session.is_cancelled() || items.is_empty() {
        return 0;
    }
    let Ok(id) = live_source_id(&entry, identity.0, identity.1, identity.2) else {
        return 0;
    };
    entry.articles_written(id, in_file, items)
}

/// Room for an article of `size` bytes written straight to disk (no cache buffer): its
/// decoded bytes are kept aside for live PAR's capture, charged to the session's snapshot
/// byte limit (ParBuffer, a cap) while held. `None` without a session or room.
pub(crate) fn streaming_session_reserve_tee(nzb_id: i32, size: u64) -> Option<LiveTee> {
    let entry = active_streaming_session(nzb_id)?;
    if entry.session.is_cancelled() || size == 0 {
        return None;
    }
    let mut queue = entry.articles.lock().unwrap_or_else(|e| e.into_inner());
    let next = queue.bytes.checked_add(size)?;
    if next > entry.article_byte_limit.load(Ordering::Acquire)
        || queue.stash.len() >= LIVE_STASH_LIMIT
    {
        return None;
    }
    queue.bytes = next;
    drop(queue);
    Some(LiveTee {
        entry: Some(entry),
        nzb_id,
        charged: size,
        bytes: Vec::with_capacity(usize::try_from(size).ok()?),
        void: false,
    })
}

/// The decoded bytes of one article written straight to disk, kept for live PAR's capture
/// so par-rs need not read them back (see [`streaming_session_reserve_tee`]). Its charge
/// returns when it drops, or moves to the kept bytes with [`LiveTee::stash`].
pub(crate) struct LiveTee {
    entry: Option<Arc<ActiveStreamingSession>>,
    nzb_id: i32,
    charged: u64,
    bytes: Vec<u8>,
    void: bool,
}

impl LiveTee {
    /// Appends decoded bytes. More than reserved voids the tee (nothing is kept).
    pub(crate) fn push(&mut self, data: &[u8]) {
        if self.void {
            return;
        }
        if self.bytes.len() as u64 + data.len() as u64 > self.charged {
            self.void = true;
            self.bytes = Vec::new();
            return;
        }
        self.bytes.extend_from_slice(data);
    }

    /// The article is written whole at `offset` of queue file `identity`: keep its bytes for
    /// the capture, when the session that reserved them is still the job's.
    pub(crate) fn stash(mut self, identity: (Option<i32>, &[u8], &[u8]), offset: u64) {
        let Some(entry) = self.entry.take() else {
            return;
        };
        let bytes = std::mem::take(&mut self.bytes);
        let current = !self.void
            && !entry.session.is_cancelled()
            && active_streaming_session(self.nzb_id)
                .is_some_and(|now| now.generation == entry.generation);
        let id = current
            .then(|| live_source_id(&entry, identity.0, identity.1, identity.2).ok())
            .flatten();
        match id {
            Some(id) => entry.stash_charged(id, offset, bytes, self.charged),
            None => {
                entry.articles.lock().unwrap_or_else(|e| e.into_inner()).bytes -= self.charged;
            }
        }
    }
}

impl Drop for LiveTee {
    fn drop(&mut self) {
        if let Some(entry) = self.entry.take() {
            entry.articles.lock().unwrap_or_else(|e| e.into_inner()).bytes -= self.charged;
        }
    }
}

/// Jobs with queued work, copied under the registry lock without doing repair work.
pub(crate) fn streaming_session_pending_article_jobs() -> Vec<i32> {
    let entries: Vec<_> = STREAMING_SESSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|(&id, entry)| (id, Arc::clone(entry)))
        .collect();
    entries
        .into_iter()
        .filter_map(|(id, entry)| {
            if entry.session.is_cancelled() {
                return None;
            }
            let queued = !entry
                .articles
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pending
                .is_empty();
            let warning = !entry.warning_reported.load(Ordering::Relaxed)
                && entry
                    .last_error
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_some();
            // Damage found by a completed-file replay or a recovery volume
            // also asks for volumes; they must be unpaused even when no
            // article waits, or recovery fetching stalls.
            let unpause = !entry
                .pending_unpause_ids
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty();
            let backfill = entry.backfill_due();
            let poll = live_poll_due(id);
            (queued || warning || unpause || backfill || poll).then_some(id)
        })
        .collect()
}

/// Owned completion facts, captured only after the downloader's assembly/flush
/// barrier. No queue pointers are retained beyond that lock.
pub(crate) struct LiveCompletedFile {
    pub nzb_id: i32,
    pub base_dir: PathBuf,
    pub path: PathBuf,
    pub name: Vec<u8>,
    pub hash16k: Vec<u8>,
    pub par_file: bool,
    pub successful: bool,
    pub accepted: Vec<ByteRange>,
    pub queued_id: Option<u64>,
}

static COMPLETED_LIVE_FILES: LazyLock<Mutex<VecDeque<PendingCompletedFile>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));
static COMPLETION_DRAINERS: LazyLock<Mutex<HashSet<i32>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
pub(crate) struct CompletionDrainLease {
    nzb_id: i32,
}
impl Drop for CompletionDrainLease {
    fn drop(&mut self) {
        COMPLETION_DRAINERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.nzb_id);
    }
}
/// A completion drain for `nzb_id` is folding files and has not yet made its
/// follow-up live repair decision.
pub(crate) fn streaming_session_completion_drain_active(nzb_id: i32) -> bool {
    COMPLETION_DRAINERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&nzb_id)
}
pub(crate) fn streaming_session_claim_completion_drain(
    nzb_id: i32,
) -> Option<CompletionDrainLease> {
    let admitted = COMPLETION_DRAINERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(nzb_id);
    admitted.then(|| CompletionDrainLease { nzb_id })
}

struct PendingCompletedFile {
    file: LiveCompletedFile,
    generation: Option<SessionGeneration>,
    writers: Arc<WriteCoordinator>,
    retry_ready: bool,
    _charge: ControlLease,
}

pub(crate) fn streaming_session_queue_completed_file(
    nzb_id: i32,
    bytes: u64,
    capture: impl FnOnce() -> Result<LiveCompletedFile, RepairError>,
) -> Result<(), RepairError> {
    let charge = reserve_control(nzb_id, bytes, effective_live_pool_bytes())?;
    let file = capture()?;
    let actual = file.name.capacity()
        + file.hash16k.capacity()
        + file.base_dir.capacity()
        + file.path.capacity()
        + file.accepted.capacity() * std::mem::size_of::<ByteRange>()
        + 512;
    if file.nzb_id != nzb_id || actual as u64 > bytes || file.accepted.len() > 4096 {
        return Err(live_error(
            "live completion capture",
            "completion facts exceed admission",
        ));
    }
    let writers = download_writes(nzb_id, &file.base_dir).map_err(|_| RepairError::Cancelled)?;
    let generation = active_streaming_session(nzb_id).map(|entry| entry.generation);
    COMPLETED_LIVE_FILES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push_back(PendingCompletedFile {
            file,
            generation,
            writers,
            retry_ready: true,
            _charge: charge,
        });
    Ok(())
}

pub(crate) fn streaming_session_pending_completed_jobs() -> Vec<i32> {
    let pending = COMPLETED_LIVE_FILES
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut ids = Vec::new();
    for item in pending.iter() {
        if !ids.contains(&item.file.nzb_id) {
            ids.push(item.file.nzb_id);
        }
    }
    ids
}

/// Only a later coordinator loop may re-admit an event deferred by capture.
pub(crate) fn streaming_session_retry_completion_tick() {
    for item in COMPLETED_LIVE_FILES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter_mut()
    {
        item.retry_ready = true;
    }
}

pub(crate) fn streaming_session_capture_busy(error: &RepairError) -> bool {
    matches!(error, RepairError::Io { context, source }
        if context == "live namespace busy" && source.kind() == std::io::ErrorKind::WouldBlock)
}

fn live_source_id(
    entry: &ActiveStreamingSession,
    queue_id: Option<i32>,
    name: &[u8],
    hash16k: &[u8],
) -> Result<FileId, RepairError> {
    match entry
        .writers
        .namespace_source_id(entry.generation, queue_id, name, hash16k)
    {
        Ok(Some(Some(id))) => return Ok(id),
        Ok(Some(None)) => return Err(RepairError::UnknownFile(FileId([0; 16]))),
        Err(_) => {
            let why = entry.writers.stop_reason().unwrap_or_else(|| "live namespace refused".to_owned());
            entry.note_stop(entry.metrics.report_nzb_id(), &why);
            entry.session.cancel();
            return Err(RepairError::Cancelled);
        }
        Ok(None) => {}
    }
    entry
        .session
        .file_id_by_name(name)
        .or_else(|| live_hash16k(hash16k).and_then(|hash| entry.session.file_id_by_hash16k(&hash)))
        .ok_or(RepairError::UnknownFile(FileId([0; 16])))
}

fn live_hash16k(bytes: &[u8]) -> Option<[u8; 16]> {
    fn digit(n: u8) -> Option<u8> {
        match n {
            b'0'..=b'9' => Some(n - b'0'),
            b'a'..=b'f' => Some(n - b'a' + 10),
            b'A'..=b'F' => Some(n - b'A' + 10),
            _ => None,
        }
    }
    if bytes.len() != 32 {
        return None;
    }
    let mut hash = [0; 16];
    for (i, pair) in bytes.chunks_exact(2).enumerate() {
        hash[i] = digit(pair[0])? * 16 + digit(pair[1])?;
    }
    Some(hash)
}

#[cfg(test)]
pub(crate) fn streaming_session_process_completed_file(
    nzb_id: i32,
    resolve: impl FnOnce(&mut LiveCompletedFile) -> Result<(), RepairError>,
) -> Option<Result<bool, RepairError>> {
    streaming_session_process_completed_file_with_start(nzb_id, resolve, |file, writers| {
        streaming_session_start_bounded_with_context(
            nzb_id,
            &file.path,
            &file.base_dir,
            effective_live_pool_bytes(),
            writers,
        )
    })
}

pub(crate) fn streaming_session_process_completed_file_with_start(
    nzb_id: i32,
    resolve: impl FnOnce(&mut LiveCompletedFile) -> Result<(), RepairError>,
    start: impl FnOnce(&LiveCompletedFile, Arc<WriteCoordinator>) -> Result<(), RepairError>,
) -> Option<Result<bool, RepairError>> {
    let mut item = {
        let mut pending = COMPLETED_LIVE_FILES
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let index = pending
            .iter()
            .position(|item| item.file.nzb_id == nzb_id && item.retry_ready)?;
        pending.remove(index)?
    };
    let result = (|| {
        resolve(&mut item.file)?;
        let file = &item.file;
        let actual = file.name.capacity()
            + file.hash16k.capacity()
            + file.base_dir.capacity()
            + file.path.capacity()
            + file.accepted.capacity() * std::mem::size_of::<ByteRange>()
            + 512;
        if actual as u64 > item._charge.bytes() {
            return Err(live_error(
                "live completion refresh",
                "renamed context exceeds admission",
            ));
        }
        if let Some(generation) = item.generation {
            if active_streaming_session(nzb_id).is_none_or(|entry| entry.generation != generation) {
                return Err(RepairError::Cancelled);
            }
        }
        let file = &item.file;
        if file.par_file {
            // The production per-job CompletionDrainLease owns initialization;
            // shutdown must never wait on a lock held through recovery work.
            if active_streaming_session(nzb_id).is_none() {
                if !file.successful {
                    return Ok(false);
                }
                start(file, Arc::clone(&item.writers))?;
                return Ok(true);
            }
            // A volume that failed outright may never have been written: fetch
            // a replacement for the blocks it was expected to bring.
            if !file.successful && !file.path.exists() {
                if let Some(id) = file.queued_id {
                    streaming_session_on_recovery_volume_lost(nzb_id, id);
                }
                return Ok(false);
            }
            // A recovery volume that lost articles still carries good blocks:
            // every packet is checked by MD5 and the reader resyncs past holes,
            // so only the damaged blocks are lost. Dropping the whole volume made
            // the selector fetch the next, larger one for blocks already here.
            streaming_session_on_recovery_volume_arrived(nzb_id, &file.path, file.queued_id)
                .ok_or(RepairError::Cancelled)??;
            return Ok(false);
        }
        let Some(entry) = active_streaming_session(nzb_id) else {
            return Ok(false);
        };
        let queue_id = file.queued_id.and_then(|id| i32::try_from(id).ok());
        let id = live_source_id(&entry, queue_id, &file.name, &file.hash16k)?;
        entry.file_completed(id, &file.path);
        let length = entry.file_lengths[&id];
        if length > i64::MAX as u64 {
            return Err(live_error(
                "live completed source",
                "file length exceeds range geometry",
            ));
        }
        let accepted = if file.successful {
            vec![ByteRange::new(0, length as i64)]
        } else {
            file.accepted.clone()
        };
        for range in &accepted {
            if range.offset < 0
                || range.size < 0
                || range
                    .offset
                    .checked_add(range.size)
                    .is_none_or(|end| end as u64 > length)
            {
                return Err(live_error(
                    "live completed source",
                    "invalid accepted range",
                ));
            }
        }
        let accepted = normalize_ranges(&accepted);
        with_live_entry(nzb_id, &entry, |entry| {
            let mut end = 0;
            for range in &accepted {
                if range.offset > end {
                    let (_, action) = entry.session.on_article_failed_permanently(
                        &id,
                        end as u64,
                        (range.offset - end) as u64,
                    )?;
                    entry.unpause(action.as_ref());
                }
                end = range.end();
            }
            if end < length as i64 {
                let (_, action) = entry.session.on_article_failed_permanently(
                    &id,
                    end as u64,
                    length - end as u64,
                )?;
                entry.unpause(action.as_ref());
            }
            Ok(())
        })?;
        // Replay one slice at a time and prioritize incoming snapshots between
        // slices, so a late index's prefix cannot monopolize the operation gate.
        for range in accepted {
            let size = entry.session.slice_size();
            let mut offset = (range.offset as u64).div_ceil(size) * size;
            while offset < range.end() as u64 {
                let end = (offset + size).min(length);
                // Permanently missing bytes make this whole PAR slice an
                // erasure. Reading its good prefix cannot contribute a row.
                if end > range.end() as u64 {
                    break;
                }
                if let Some(Err(error)) = streaming_session_drain_articles(nzb_id) {
                    if matches!(error, RepairError::Cancelled) {
                        return Err(error);
                    }
                }
                with_live_entry(nzb_id, &entry, |entry| {
                    reconcile_live_file(
                        nzb_id,
                        entry,
                        id,
                        &file.path,
                        &[ByteRange::new(offset as i64, (end - offset) as i64)],
                    )
                })?;
                offset = end;
            }
        }
        Ok(false)
    })();
    if result
        .as_ref()
        .is_err_and(|error| streaming_session_capture_busy(error))
    {
        item.retry_ready = false;
        COMPLETED_LIVE_FILES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(item);
    }
    Some(result)
}

const LIVE_METADATA_BYTES: u64 = 8 << 20;

fn live_metadata_charge(
    total: &mut u64,
    raw: &par_rs::packet::RawPacket,
) -> Result<(), RepairError> {
    use par_rs::packet::PacketType;
    // Charge owned strings/maps/checksum vectors conservatively, including
    // duplicate framing. Recovery payloads remain on disk, but their refs count.
    let payload = match raw.header.kind() {
        PacketType::RecoverySlice | PacketType::Unknown => 0,
        _ => raw.header.length,
    };
    let next = payload
        .checked_mul(4)
        .and_then(|n| n.checked_add(256))
        .and_then(|n| total.checked_add(n));
    if next.is_none_or(|bytes| bytes > LIVE_METADATA_BYTES) {
        return Err(RepairError::StreamingMemoryLimit {
            context: "live PAR metadata",
            required_bytes: next.unwrap_or(u64::MAX),
            limit_bytes: LIVE_METADATA_BYTES,
        });
    }
    *total = next.unwrap();
    Ok(())
}

fn load_bounded_live_metadata(path: &Path) -> Result<PacketSet, RepairError> {
    use par_rs::packet::{parse_packet, FileSource, Packet, PacketIter};
    let io = |source| RepairError::Io {
        context: format!("loading bounded PAR metadata {}", path.display()),
        source,
    };
    let source = FileSource::open(path).map_err(io)?;
    let mut charge = 0;
    // Inspect framing before authenticating or allocating metadata. Conservative
    // rejection of malformed framing is safe: ordinary repair remains available.
    for raw in PacketIter::new(&source, ScanOptions::UNVERIFIED) {
        live_metadata_charge(&mut charge, &raw.map_err(io)?)?;
    }
    let mut set = PacketSet::new();
    charge = 0;
    for raw in PacketIter::new(&source, ScanOptions::METADATA) {
        let raw = raw.map_err(io)?;
        live_metadata_charge(&mut charge, &raw)?;
        if raw.header.kind() == par_rs::packet::PacketType::Main {
            if let Ok(Packet::Main(main)) = parse_packet(&source, &raw, 0).map_err(io)? {
                set.add(raw.header.set_id, Packet::Main(main));
                break;
            }
        }
    }
    if set.main.is_none() {
        return Err(RepairError::NoMainPacket);
    }
    charge = 0;
    for raw in PacketIter::new(&source, ScanOptions::METADATA) {
        let raw = raw.map_err(io)?;
        live_metadata_charge(&mut charge, &raw)?;
        if let Ok(packet) = parse_packet(&source, &raw, 0).map_err(io)? {
            set.add(raw.header.set_id, packet);
        }
    }
    Ok(set)
}

/// Starts bounded download-time accumulation. Insufficient admission leaves normal repair available.
pub fn streaming_session_start_bounded(
    nzb_id: i32,
    par2_path: &Path,
    base_dir: &Path,
    effective_pool_bytes: u64,
) -> Result<(), RepairError> {
    let writers = download_writes(nzb_id, base_dir).map_err(|_| RepairError::Cancelled)?;
    streaming_session_start_bounded_with_context(
        nzb_id,
        par2_path,
        base_dir,
        effective_pool_bytes,
        writers,
    )
}

fn streaming_session_start_bounded_with_context(
    nzb_id: i32,
    par2_path: &Path,
    base_dir: &Path,
    effective_pool_bytes: u64,
    writers: Arc<WriteCoordinator>,
) -> Result<(), RepairError> {
    let mut budget = acquire_session_budget(LIVE_METADATA_BYTES, effective_pool_bytes)?;
    let set = load_bounded_live_metadata(par2_path)?;
    let slice_size = set
        .main
        .as_ref()
        .ok_or(RepairError::NoMainPacket)?
        .slice_size;
    let limits =
        resources(slice_size, effective_pool_bytes).ok_or(RepairError::StreamingMemoryLimit {
            context: "bounded live PAR startup",
            required_bytes: crate::parlive::min_reservation(slice_size),
            limit_bytes: effective_pool_bytes,
        })?;
    if !budget.resize(limits.reservation_bytes, effective_pool_bytes) {
        return Err(RepairError::StreamingMemoryLimit {
            context: "shared live PAR reservation",
            required_bytes: limits.reservation_bytes,
            limit_bytes: effective_pool_bytes,
        });
    }
    let entry = build_planned_streaming_session(
        &set,
        base_dir,
        limits.options,
        budget,
        writers,
        &LivePlanHint::default(),
    )?;
    entry.session.on_recovery_volume_arrived(par2_path, None)?;
    *entry.par_name.lock().unwrap_or_else(|e| e.into_inner()) = par2_path
        .file_name()
        .map(par_rs::filenames::os_str_bytes)
        .unwrap_or_default();
    install_streaming_session(nzb_id, entry)
}

pub(crate) struct PreparedLiveMetadata {
    set: PacketSet,
    base_dir: PathBuf,
    options: SessionOptions,
    writers: Arc<WriteCoordinator>,
    budget: SessionBudgetLease,
    hint: LivePlanHint,
}

pub(crate) struct PreparedLiveSession {
    entry: Arc<ActiveStreamingSession>,
    proof: Option<live_namespace::NamespaceProof>,
    startup: Option<Arc<LiveStartupLease>>,
}

pub(crate) struct InstalledLiveSession {
    entry: Arc<ActiveStreamingSession>,
    old: Option<Arc<ActiveStreamingSession>>,
    startup: Option<Arc<LiveStartupLease>>,
}

/// One coordinator owns both unpublished cancellation and install admission.
#[derive(Clone, Default)]
pub(crate) struct LiveStartupFence(Arc<Mutex<LiveStartupState>>);
pub(crate) struct LiveStartupAttempt {
    fence: LiveStartupFence,
    epoch: u64,
}
#[derive(Default)]
struct LiveStartupState {
    stopped: bool,
    epoch: u64,
    entries: Vec<(SessionGeneration, std::sync::Weak<ActiveStreamingSession>)>,
}
struct LiveStartupLease {
    fence: LiveStartupFence,
    epoch: u64,
    // Keeps engine metadata and its charge alive until index removal on Drop.
    entry: Arc<ActiveStreamingSession>,
}
impl LiveStartupFence {
    /// Capture the epoch before parsing, not only once an engine exists.
    pub fn begin(&self) -> Result<LiveStartupAttempt, RepairError> {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped {
            return Err(RepairError::Cancelled);
        }
        Ok(LiveStartupAttempt {
            fence: self.clone(),
            epoch: state.epoch,
        })
    }
    fn track(
        &self,
        entry: &Arc<ActiveStreamingSession>,
        epoch: u64,
    ) -> Result<Arc<LiveStartupLease>, RepairError> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped || state.epoch != epoch {
            drop(state);
            entry.session.cancel();
            return Err(RepairError::Cancelled);
        }
        state
            .entries
            .try_reserve(1)
            .map_err(|_| live_error("live startup cancellation", "allocation refused"))?;
        state
            .entries
            .push((entry.generation, Arc::downgrade(entry)));
        Ok(Arc::new(LiveStartupLease {
            fence: self.clone(),
            epoch: state.epoch,
            entry: Arc::clone(entry),
        }))
    }
    /// No queue acquisition or joining while this admission mutex is held.
    pub fn stop<T>(&self, cancel_installed: impl FnOnce() -> T) -> T {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if !state.stopped {
            state.stopped = true;
            state.epoch = state.epoch.saturating_add(1);
        }
        for (_, entry) in &state.entries {
            if let Some(entry) = entry.upgrade() {
                entry.session.cancel();
            }
        }
        cancel_installed()
    }
    pub fn reset(&self) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.epoch == u64::MAX {
            return false;
        }
        state.stopped = false;
        true
    }
}
impl LiveStartupAttempt {
    pub fn ensure_current(&self) -> Result<(), RepairError> {
        let state = self.fence.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped || state.epoch != self.epoch {
            return Err(RepairError::Cancelled);
        }
        Ok(())
    }
}
impl LiveStartupLease {
    fn admitted<T>(
        &self,
        action: impl FnOnce() -> Result<T, RepairError>,
    ) -> Result<T, RepairError> {
        let state = self.fence.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped || state.epoch != self.epoch || self.entry.session.is_cancelled() {
            return Err(RepairError::Cancelled);
        }
        action()
    }
}
impl Drop for LiveStartupLease {
    fn drop(&mut self) {
        self.fence
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries
            .retain(|(generation, _)| *generation != self.entry.generation);
    }
}

pub(crate) fn prepare_live_start(
    par2_path: &Path,
    base_dir: &Path,
    pool: u64,
    writers: Arc<WriteCoordinator>,
) -> Result<PreparedLiveMetadata, RepairError> {
    let mut budget = acquire_session_budget(LIVE_METADATA_BYTES, pool)?;
    let set = load_bounded_live_metadata(par2_path)?;
    let size = set
        .main
        .as_ref()
        .ok_or(RepairError::NoMainPacket)?
        .slice_size;
    let limits = resources(size, pool).ok_or(RepairError::StreamingMemoryLimit {
        context: "bounded live PAR startup",
        required_bytes: crate::parlive::min_reservation(size),
        limit_bytes: pool,
    })?;
    if !budget.resize(limits.reservation_bytes, pool) {
        return Err(RepairError::StreamingMemoryLimit {
            context: "shared live PAR reservation",
            required_bytes: limits.reservation_bytes,
            limit_bytes: pool,
        });
    }
    Ok(PreparedLiveMetadata {
        set,
        base_dir: base_dir.to_path_buf(),
        options: limits.options,
        writers,
        budget,
        hint: LivePlanHint::default(),
    })
}

impl PreparedLiveMetadata {
    /// The queue's recovery volumes and failures so far, to plan the recovery slots.
    pub fn set_plan_hint(&mut self, hint: LivePlanHint) {
        self.hint = hint;
    }
    pub fn catalog_required_bytes(&self) -> Result<usize, RepairError> {
        live_namespace::NamespaceProof::catalog_required_bytes(&self.set)
    }
    /// Parsing, initial recovery work and engine construction happen off queue.
    #[cfg(test)]
    pub fn finish(
        self,
        path: &Path,
        facts: live_namespace::NamespaceFacts,
    ) -> Result<PreparedLiveSession, RepairError> {
        self.finish_inner(path, facts, None)
    }
    pub fn finish_tracked(
        self,
        path: &Path,
        facts: live_namespace::NamespaceFacts,
        attempt: &LiveStartupAttempt,
    ) -> Result<PreparedLiveSession, RepairError> {
        self.finish_inner(path, facts, Some(attempt))
    }
    fn finish_inner(
        self,
        path: &Path,
        facts: live_namespace::NamespaceFacts,
        attempt: Option<&LiveStartupAttempt>,
    ) -> Result<PreparedLiveSession, RepairError> {
        if let Some(attempt) = attempt {
            attempt.ensure_current()?;
        }
        if facts.root != self.base_dir {
            return Err(RepairError::Cancelled);
        }
        let proof = live_namespace::NamespaceProof::prepare(&self.set, facts)?;
        let entry = build_planned_streaming_session(
            &self.set,
            &self.base_dir,
            self.options,
            self.budget,
            self.writers,
            &self.hint,
        )?;
        // Register before the potentially long recovery arrival scan/fold.
        let startup = attempt
            .map(|attempt| attempt.fence.track(&entry, attempt.epoch))
            .transpose()?;
        entry.session.on_recovery_volume_arrived(path, None)?;
        *entry.par_name.lock().unwrap_or_else(|e| e.into_inner()) = path
            .file_name()
            .map(par_rs::filenames::os_str_bytes)
            .unwrap_or_default();
        Ok(PreparedLiveSession {
            entry,
            proof: Some(proof),
            startup,
        })
    }
}

impl PreparedLiveSession {
    /// The caller owns the queue throughout this final borrowed recheck/install.
    /// Retain self until after unlocking, including every failed/busy attempt.
    pub fn check_and_install(
        &mut self,
        nzb_id: i32,
        check: impl FnOnce(&mut live_namespace::NamespaceProof) -> Result<(), RepairError>,
    ) -> Result<InstalledLiveSession, RepairError> {
        let proof = self.proof.as_mut().ok_or(RepairError::Cancelled)?;
        proof.begin_recheck();
        check(proof)?;
        proof.finish_recheck()?;
        let startup = self.startup.clone();
        let mut install = || {
            let namespace = self.proof.take().ok_or(RepairError::Cancelled)?;
            self.entry
                .writers
                .bind_namespace_generation(self.entry.generation, namespace, || {
                    STREAMING_SESSIONS
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(nzb_id, Arc::clone(&self.entry))
                })
                .map_err(|_| RepairError::Cancelled)
        };
        let old = match &startup {
            Some(startup) => startup.admitted(install)?,
            None => install()?,
        };
        Ok(InstalledLiveSession {
            entry: Arc::clone(&self.entry),
            old,
            startup: self.startup.clone(),
        })
    }
}

impl InstalledLiveSession {
    /// Run after the global queue guard has dropped.
    pub fn finish(self, nzb_id: i32) {
        // Retired engines may join their workers on last drop; keep that work
        // outside startup admission, just as it stays outside the queue.
        if let Some(old) = self.old {
            old.session.cancel();
            old.writers.cancel_generation(old.generation);
        }
        let finish = || {
            if ensure_current_generation(nzb_id, &self.entry).is_ok() {
                self.entry.observe_count_readiness();
                self.entry.emit_live_metrics(nzb_id, "started");
            }
            Ok(())
        };
        match &self.startup {
            Some(startup) => {
                let _ = startup.admitted(finish);
            }
            None => {
                let _ = finish();
            }
        }
    }
}

fn options_reservation(options: &SessionOptions) -> Result<u64, RepairError> {
    options
        .max_syndrome_bytes
        .checked_mul(2)
        .and_then(|n| options.max_working_bytes.checked_mul(5)?.checked_add(n))
        .and_then(|n| n.checked_add(options.max_staging_bytes))
        .and_then(|n| n.checked_add(options.solve_mem_limit_bytes.max(16 << 20).checked_mul(2)?))
        .and_then(|n| n.checked_add(8 << 20))
        .ok_or_else(|| live_error("live PAR reservation", "byte count overflow"))
}

/// Starts or replaces a session with explicit limits, charged to the shared PAR pool.
pub fn streaming_session_start(
    nzb_id: i32,
    par2_path: &Path,
    base_dir: &Path,
    options: SessionOptions,
) -> Result<(), RepairError> {
    let budget =
        acquire_session_budget(options_reservation(&options)?, effective_live_pool_bytes())?;
    start_streaming_session_with(nzb_id, par2_path, base_dir, options, budget)
}

/// Starts a session charged to a pool of its own (fixed size), so what it lends does
/// not depend on what other tests' sessions hold in the shared one.
#[cfg(test)]
pub(crate) fn streaming_session_start_in_pool(
    pool: &Arc<crate::parlive::BudgetPool>,
    nzb_id: i32,
    par2_path: &Path,
    base_dir: &Path,
    options: SessionOptions,
) -> Result<(), RepairError> {
    let bytes = options_reservation(&options)?;
    let budget = pool.acquire(bytes, pool.size().unwrap_or(u64::MAX)).ok_or(
        RepairError::StreamingMemoryLimit {
            context: "test live PAR reservation",
            required_bytes: bytes,
            limit_bytes: pool.size().unwrap_or(u64::MAX),
        },
    )?;
    start_streaming_session_with(nzb_id, par2_path, base_dir, options, budget)
}

fn start_streaming_session_with(
    nzb_id: i32,
    par2_path: &Path,
    base_dir: &Path,
    options: SessionOptions,
    budget: SessionBudgetLease,
) -> Result<(), RepairError> {
    let writers = download_writes(nzb_id, base_dir).map_err(|_| RepairError::Cancelled)?;
    let set = load_par2_set(par2_path)?;
    let entry = build_streaming_session(&set, base_dir, options, budget, writers)?;
    entry.session.on_recovery_volume_arrived(par2_path, None)?;
    *entry.par_name.lock().unwrap_or_else(|e| e.into_inner()) = par2_path
        .file_name()
        .map(par_rs::filenames::os_str_bytes)
        .unwrap_or_default();
    install_streaming_session(nzb_id, entry)
}

/// Tells the live session of `nzb_id` that `size` bytes at `offset` of file `name` were
/// rewritten on disk (a stream-repair patch, a rebuilt volume head or end): it reads the range
/// back and re-verifies the slices it touches instead of being cancelled. `Some(true)` when
/// every slice was reconciled; `Some(false)` when one could not be (the caller falls back to
/// cancelling); `None` when no live session runs for the download.
pub fn streaming_session_on_range_replaced_by_name(
    nzb_id: i32,
    name: &[u8],
    offset: u64,
    size: u64,
) -> Option<bool> {
    let outcome = with_live_session(nzb_id, |entry| {
        let id = entry
            .session
            .file_id_by_name(name)
            .ok_or(RepairError::UnknownFile(FileId([0; 16])))?;
        let outcome = entry.session.on_range_replaced(&id, offset, size, None)?;
        entry.unpause(outcome.unpause.as_ref());
        Ok(outcome.is_reconciled() && entry.session.diverged_slices().is_empty())
    })?;
    Some(outcome.unwrap_or(false))
}

/// Cancels the current generation permanently; in-flight owners retain their reservation.
#[track_caller]
pub fn streaming_session_cancel(nzb_id: i32) {
    if let Some(entry) = active_streaming_session(nzb_id) {
        let at = std::panic::Location::caller();
        entry.note_stop(nzb_id, &format!("cancelled from {}:{}", at.file(), at.line()));
        entry
            .writers
            .cancel_generation_with(entry.generation, || entry.session.cancel());
    }
}

pub(crate) struct CancelledLiveOwners {
    _entries: Vec<Arc<ActiveStreamingSession>>,
}
/// Retain the returned owners until after the startup admission guard drops.
pub(crate) fn streaming_sessions_cancel_all() -> CancelledLiveOwners {
    let entries: Vec<_> = STREAMING_SESSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .cloned()
        .collect();
    for entry in &entries {
        entry
            .writers
            .cancel_generation_with(entry.generation, || entry.session.cancel());
    }
    CancelledLiveOwners { _entries: entries }
}
pub(crate) fn streaming_sessions_forget_all() {
    let entries: Vec<_> = STREAMING_SESSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .copied()
        .collect();
    for id in entries {
        streaming_session_forget(id);
    }
    COMPLETED_LIVE_FILES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

/// Last operation failure for the current generation.
pub fn streaming_session_last_error(nzb_id: i32) -> Option<String> {
    active_streaming_session(nzb_id)?
        .last_error
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub(crate) fn streaming_session_par_filename(nzb_id: i32) -> Option<Vec<u8>> {
    let entry = active_streaming_session(nzb_id)?;
    let name = entry
        .par_name
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    Some(name)
}

pub(crate) fn streaming_session_source_length(
    nzb_id: i32,
    name: &[u8],
    hash16k: &[u8],
) -> Option<u64> {
    let entry = active_streaming_session(nzb_id)?;
    let id = entry.session.file_id_by_name(name).or_else(|| {
        live_hash16k(hash16k).and_then(|hash| entry.session.file_id_by_hash16k(&hash))
    })?;
    entry.file_lengths.get(&id).copied()
}

/// Logical successful I/O, including bounded source reads made by caller catch-up.
pub fn streaming_session_io_stats(nzb_id: i32) -> Option<LiveIoStats> {
    let entry = active_streaming_session(nzb_id)?;
    let mut stats = entry.session.io_stats();
    stats.source_bytes_read = stats
        .source_bytes_read
        .saturating_add(entry.catchup_source_bytes.load(Ordering::Relaxed));
    Some(stats)
}

/// Replays only accepted, committed ranges. The caller must first establish its flush barrier.
/// Already verified slices are skipped; neither sparse holes nor preallocated bytes are accepted.
pub fn streaming_session_reconcile_file(
    nzb_id: i32,
    file_id: FileId,
    source_path: &Path,
    accepted: &[ByteRange],
) -> Option<Result<ArticleOutcome, RepairError>> {
    with_live_session(nzb_id, |entry| {
        reconcile_live_file(nzb_id, entry, file_id, source_path, accepted)
    })
}

struct CallerPayloadCharge<'a> {
    entry: &'a ActiveStreamingSession,
    bytes: u64,
}
impl<'a> CallerPayloadCharge<'a> {
    /// Replays run one chunk at a time under the session's operation lock, so
    /// a chunk that fits the limit on its own is admitted even while article
    /// snapshots fill the queue: otherwise a busy download defers committed
    /// replay, the fallback for every refused snapshot, until it ends. Memory
    /// stays bounded by the limit plus one chunk.
    fn acquire(entry: &'a ActiveStreamingSession, bytes: u64) -> Result<Self, RepairError> {
        let mut queue = entry.articles.lock().unwrap_or_else(|e| e.into_inner());
        let next = queue.bytes.checked_add(bytes);
        let limit = entry.article_byte_limit.load(Ordering::Acquire);
        if next.is_none() || bytes > limit {
            return Err(RepairError::StreamingMemoryLimit {
                context: "live replay payload",
                required_bytes: if next.is_none() { u64::MAX } else { bytes },
                limit_bytes: limit,
            });
        }
        queue.bytes = next.unwrap();
        Ok(Self { entry, bytes })
    }
}
impl Drop for CallerPayloadCharge<'_> {
    fn drop(&mut self) {
        self.entry
            .articles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .bytes -= self.bytes;
    }
}

fn reconcile_live_file(
    nzb_id: i32,
    entry: &ActiveStreamingSession,
    file_id: FileId,
    source_path: &Path,
    accepted: &[ByteRange],
) -> Result<ArticleOutcome, RepairError> {
    let length = *entry
        .file_lengths
        .get(&file_id)
        .ok_or(RepairError::UnknownFile(file_id))?;
    for range in accepted {
        if range.offset < 0
            || range.size < 0
            || range
                .offset
                .checked_add(range.size)
                .is_none_or(|end| end as u64 > length)
        {
            return Err(live_error("live PAR catch-up", "invalid accepted range"));
        }
    }
    let mut source = std::fs::File::open(source_path).map_err(|source| RepairError::Io {
        context: format!("opening live catch-up {}", source_path.display()),
        source,
    })?;
    let mut outcome = ArticleOutcome::default();
    let slice_size = entry.session.slice_size();
    for range in accepted {
        let mut offset = range.offset as u64;
        let end = offset + range.size as u64;
        while offset < end {
            ensure_current_generation(nzb_id, entry)?;
            let slice = offset / slice_size;
            let slice_end = ((slice + 1) * slice_size).min(length);
            let chunk_end = slice_end.min(end);
            if matches!(
                entry.session.slice_state(&file_id, slice as usize)?,
                SliceState::VerifiedGood
            ) {
                offset = chunk_end;
                continue;
            }
            let count = (chunk_end - offset) as usize;
            let _charge = CallerPayloadCharge::acquire(entry, count as u64)?;
            let mut buffer = vec![0u8; count];
            source
                .seek(SeekFrom::Start(offset))
                .map_err(|source| RepairError::Io {
                    context: "seeking live catch-up".to_owned(),
                    source,
                })?;
            let mut read = 0;
            while read < count {
                ensure_current_generation(nzb_id, entry)?;
                let n = source
                    .read(&mut buffer[read..count.min(read + 64 * 1024)])
                    .map_err(|source| RepairError::Io {
                        context: "reading live catch-up".to_owned(),
                        source,
                    })?;
                if n == 0 {
                    return Err(live_error(
                        "live PAR catch-up",
                        "unexpected end of committed source",
                    ));
                }
                entry
                    .catchup_source_bytes
                    .fetch_add(n as u64, Ordering::Relaxed);
                read += n;
            }
            let next = entry.session.on_article_finished_memory_only(
                &file_id,
                offset,
                &buffer,
                crc32fast::hash(&buffer),
            )?;
            entry.unpause(next.unpause.as_ref());
            outcome.events.extend(next.events);
            if next.unpause.is_some() {
                outcome.unpause = next.unpause;
            }
            offset += count as u64;
        }
    }
    Ok(outcome)
}

/// Registers a queued `.vol*.par2` volume with the active [`StreamingParSession`] for `nzb_id`.
pub fn streaming_session_register_queued_volume(
    nzb_id: i32,
    volume_id: u64,
    blocks: i32,
    paused: bool,
) -> bool {
    let Some(entry) = active_streaming_session(nzb_id) else {
        return false;
    };
    entry.session.register_queued_volume(QueuedVolume {
        id: volume_id,
        blocks,
        paused,
    });
    entry
        .queued_blocks
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(volume_id, u64::try_from(blocks).unwrap_or(0));
    true
}

/// Feeds a completed article by filename.
pub fn streaming_session_on_article_finished_by_name(
    nzb_id: i32,
    name: &[u8],
    offset: u64,
    size: u64,
    crc32: u32,
    bytes: Option<&[u8]>,
) -> Option<Result<ArticleOutcome, RepairError>> {
    with_live_session(nzb_id, |entry| {
        let outcome = entry
            .session
            .on_article_finished_by_name(name, offset, size, crc32, bytes)?;
        entry.unpause(outcome.unpause.as_ref());
        Ok(outcome)
    })
}

/// Marks a permanently failed article by file ID.
pub fn streaming_session_on_article_failed_permanently(
    nzb_id: i32,
    file_id: &FileId,
    offset: u64,
    size: u64,
) -> Option<Result<(Vec<SliceEvent>, Option<UnpauseAction>), RepairError>> {
    with_live_session(nzb_id, |entry| {
        let outcome = entry
            .session
            .on_article_failed_permanently(file_id, offset, size)?;
        entry.unpause(outcome.1.as_ref());
        Ok(outcome)
    })
}

/// A recovery volume that was in flight failed outright and left nothing on
/// disk: stop counting its blocks as coming and fetch a replacement.
pub(crate) fn streaming_session_on_recovery_volume_lost(nzb_id: i32, queued_id: u64) {
    let _ = with_live_session(nzb_id, |entry| {
        entry
            .queued_blocks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&queued_id);
        entry.unpause(entry.session.on_recovery_volume_lost(queued_id).as_ref());
        Ok(())
    });
}

/// Registers a newly downloaded `.vol*.par2` recovery volume with the active [`StreamingParSession`].
pub fn streaming_session_on_recovery_volume_arrived(
    nzb_id: i32,
    path: &Path,
    queued_id: Option<u64>,
) -> Option<Result<usize, RepairError>> {
    with_live_session(nzb_id, |entry| {
        // Growth that found ParBuffer full earlier may fit now.
        entry.grow_for_damage();
        if let Some(id) = queued_id {
            entry
                .queued_blocks
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
        }
        let count = entry.session.on_recovery_volume_arrived(path, queued_id)?;
        entry.grow_for_damage();
        // The volume may have brought fewer usable blocks than it promised
        // while in flight; fetch the rest now, not at the next damage.
        entry.unpause(entry.session.request_unpause().as_ref());
        if let Some(id) = queued_id {
            entry
                .pending_unpause_ids
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|&x| x != id);
        }
        Ok(count)
    })
    .inspect(|_| streaming_session_kick_backfill(nzb_id))
}

/// A back-fill that could not re-read every slice (slices verified from RAM
/// reach the disk when the cache flushes them) is retried after this delay,
/// not on every coordinator tick.
const BACKFILL_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

/// Starts re-reading verified slices for syndrome slots allocated since the
/// last back-fill, on a thread of its own: it can take seconds per GB, and
/// live articles keep folding meanwhile. At most one runs per session; after
/// an error the back-fill is tried again once [`BACKFILL_RETRY_DELAY`] passes.
pub(crate) fn streaming_session_kick_backfill(nzb_id: i32) {
    let Some(entry) = active_streaming_session(nzb_id) else {
        return;
    };
    if entry.session.is_cancelled()
        || !entry.backfill_due()
        || entry
            .backfill_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return;
    }
    let worker = Arc::clone(&entry);
    let spawned = std::thread::Builder::new()
        .name(format!("live-par-backfill-{nzb_id}"))
        .spawn(move || {
            // Clears the running flag even if the back-fill panics.
            struct Running<'a>(&'a AtomicBool);
            impl Drop for Running<'_> {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::Release);
                }
            }
            let _running = Running(&worker.backfill_running);
            let retry_at = || {
                worker
                    .backfill_retry_at
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
            };
            // Back off up front, so a panic below waits out the delay too.
            *retry_at() = Some(std::time::Instant::now() + BACKFILL_RETRY_DELAY);
            // A shared back-fill error is expected while verified slices still
            // sit in the article cache, so it is retried, never reported: the
            // strict back-fill inside repair surfaces any error that persists.
            // Back-fill rereads and folds verified data: verification time for the job.
            // Charged directly, not through `timed`, whose repair-stat delta could pick up a
            // foreground repair running at the same time.
            let started = std::time::Instant::now();
            let reread = worker.session.io_stats().backfill_bytes_read;
            let backfilled = worker.session.backfill_pending().is_ok();
            let reread = worker.session.io_stats().backfill_bytes_read.saturating_sub(reread);
            if reread > 0 {
                let message = format!(
                    "Live PAR (job {nzb_id}): back-fill re-read {:.1} MB for recovery slots added after the plan",
                    reread as f64 / (1024.0 * 1024.0)
                );
                crate::log::log_active_bytes(crate::queuesubsys_host::MK_DETAIL, message.as_bytes());
            }
            LIVE_PAR_TIME
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(nzb_id)
                .or_default()
                .verify += started.elapsed();
            if backfilled {
                *retry_at() = None;
            }
        });
    if spawned.is_err() {
        entry.backfill_running.store(false, Ordering::Release);
    }
}

/// How often each job's live status is polled ([`streaming_session_poll`]).
pub(crate) const LIVE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);

/// The last polled status of each job's live session, with its time.
static LIVE_POLLS: LazyLock<
    Mutex<HashMap<i32, (std::time::Instant, LiveStatus, SessionGeneration)>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Drops job `nzb_id`'s last poll when it belongs to the session `generation` that ended.
fn forget_live_poll(nzb_id: i32, generation: SessionGeneration) {
    let mut polls = LIVE_POLLS.lock().unwrap_or_else(|e| e.into_inner());
    if polls
        .get(&nzb_id)
        .is_some_and(|(_, _, polled)| *polled == generation)
    {
        polls.remove(&nzb_id);
    }
}

/// A poll of job `nzb_id`'s live status is due.
fn live_poll_due(nzb_id: i32) -> bool {
    LIVE_POLLS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&nzb_id)
        .is_none_or(|(at, _, _)| at.elapsed() >= LIVE_POLL_INTERVAL)
}

/// Polls `status()` of job `nzb_id`'s live session, at most every [`LIVE_POLL_INTERVAL`]
/// unless `force`: keeps it for the RPC progress field
/// ([`streaming_session_progress_permille`]) and decides between fetching and repairing.
/// While recovery blocks are short (`needed_recovery_blocks`), it grows the recovery slots
/// and asks the session's selector for more volumes (queued for unpausing). Once
/// `early_cutover_possible`, the caller repairs now
/// (`postproc::direct_unpack_try_live_par_repair`). Returns the status when polled.
pub(crate) fn streaming_session_poll(nzb_id: i32, force: bool) -> Option<LiveStatus> {
    let entry = active_streaming_session(nzb_id)?;
    if entry.session.is_cancelled() || (!force && !live_poll_due(nzb_id)) {
        return None;
    }
    let mut status = entry.session.status();
    if status.needed_recovery_blocks > 0 {
        let action = entry.session.request_unpause();
        entry.unpause(action.as_ref());
        status = entry.session.status();
    }
    LIVE_POLLS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(nzb_id, (std::time::Instant::now(), status.clone(), entry.generation));
    Some(status)
}

/// Live repair progress of job `nzb_id` in permille: slices verified (or already rebuilt)
/// of all slices, from the last poll; -1 without a live session.
pub(crate) fn streaming_session_progress_permille(nzb_id: i32) -> i32 {
    let polled = LIVE_POLLS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&nzb_id)
        .map(|(_, status, _)| status.clone());
    let status = match polled {
        Some(status) if active_streaming_session(nzb_id).is_some() => status,
        _ => return -1,
    };
    live_progress_permille(&status)
}

fn live_progress_permille(status: &LiveStatus) -> i32 {
    if status.total == 0 {
        return 0;
    }
    (status.verified.min(status.total) as u64 * 1000 / status.total as u64) as i32
}

/// The par2 names of job `nzb_id`'s files that still need a repair, when the download has
/// ended for the set (every slice arrived or failed) and the ready recovery covers every
/// damaged slice: the live repair can finish the job right now. `None` otherwise.
pub(crate) fn streaming_session_download_end_targets(nzb_id: i32) -> Option<Vec<Vec<u8>>> {
    let entry = active_streaming_session(nzb_id)?;
    if ensure_current_generation(nzb_id, &entry).is_err() {
        return None;
    }
    let status = entry.session.status();
    // Folds still queued do not matter: the repair applies them before its solve.
    if !status.early_cutover_possible
        || status.damaged == 0
        || status.unarrived + status.partially_arrived > 0
    {
        return None;
    }
    let mut names: Vec<Vec<u8>> = entry
        .target_names
        .iter()
        .filter(|(id, _)| {
            !matches!(
                entry.session.volume_status(id),
                Ok(VolumeStatus::CompleteAndVerified)
            )
        })
        .map(|(_, name)| par_rs::filenames::os_str_bytes(name.as_os_str()).to_vec())
        .collect();
    names.sort_unstable();
    (!names.is_empty()).then_some(names)
}

/// The par2 name of queue file `identity` of job `nzb_id`: the file's live binding, which
/// also holds for a file still under another (obfuscated) name. `None` without a session or
/// when the file is not matched to a par2 file. Metadata only; callers may hold the queue lock.
pub(crate) fn streaming_session_par_name_of(
    nzb_id: i32,
    identity: (Option<i32>, &[u8], &[u8]),
) -> Option<Vec<u8>> {
    let entry = active_streaming_session(nzb_id)?;
    let id = live_source_id(&entry, identity.0, identity.1, identity.2).ok()?;
    let name = entry.target_names.get(&id)?;
    Some(par_rs::filenames::os_str_bytes(name.as_os_str()).to_vec())
}

/// Live progress of the repair call running on job `nzb_id`'s session (par-rs #104): its
/// step, the step's done and total units, and the session's work counter, which rises while
/// a repair makes progress. `None` without a session.
pub(crate) fn streaming_session_repair_progress(nzb_id: i32) -> Option<LiveRepairProgress> {
    Some(active_streaming_session(nzb_id)?.session.repair_progress())
}

/// A counter that rises whenever job `nzb_id`'s live session makes progress: slices
/// folded, source or recovery bytes read, articles captured, repair work, admission steps.
/// Post-processing waits for live work while it rises and hands over to batch repair only
/// when it stalls. `None` without a session.
pub(crate) fn streaming_session_progress_token(nzb_id: i32) -> Option<u64> {
    let entry = active_streaming_session(nzb_id)?;
    let io = entry.session.io_stats();
    let (folds, _) = entry.session.fold_stats();
    Some(
        [
            folds,
            io.source_bytes_read,
            io.backfill_bytes_read,
            io.recovery_bytes_read,
            io.target_verification_bytes_read,
            entry.catchup_source_bytes.load(Ordering::Relaxed),
            entry.session.repair_progress().work,
            entry.admission_progress.load(Ordering::Relaxed),
            entry.metrics.captured_articles.load(Ordering::Relaxed),
            entry.metrics.committed_articles.load(Ordering::Relaxed),
        ]
        .iter()
        .fold(0u64, |total, n| total.wrapping_add(*n)),
    )
}

/// Job `nzb_id`'s live repair admission counter (see `admission_progress`).
pub(crate) fn streaming_session_admission_progress(nzb_id: i32) -> Option<u64> {
    Some(
        active_streaming_session(nzb_id)?
            .admission_progress
            .load(Ordering::Relaxed),
    )
}

/// The name of a live repair step, for the RPC and the log; empty when idle.
pub(crate) fn live_repair_step_name(step: LiveRepairStep) -> &'static str {
    match step {
        LiveRepairStep::Idle => "",
        LiveRepairStep::PayloadWait => "waiting",
        LiveRepairStep::Backfill => "backfill",
        LiveRepairStep::Matrix => "matrix",
        LiveRepairStep::Decode => "decode",
        LiveRepairStep::Sync => "sync",
        LiveRepairStep::Refold => "refold",
        LiveRepairStep::Verify => "verify",
        _ => "repair",
    }
}

/// Polls the live status of the [`StreamingParSession`] for `nzb_id`.
pub fn streaming_session_status(nzb_id: i32) -> Option<LiveStatus> {
    Some(active_streaming_session(nzb_id)?.session.status())
}

/// A target CRC is reusable only while its original publication remains fenced.
/// Used by the ordinary quick verifier; full/reprocess checks bypass this path.
pub(crate) fn streaming_session_verified_target_crc(
    nzb_id: i32,
    base_dir: &Path,
    queue_id: i32,
    filename: &[u8],
) -> Option<u32> {
    let entry = active_streaming_session(nzb_id)?;
    if entry.base_dir != base_dir || entry.session.is_cancelled() {
        return None;
    }
    entry.writers.certified_crc(
        entry.generation,
        queue_id,
        &base_dir.join(crate::fsops::path(filename)),
    )
}

/// A bounded count upper bound only: the helper must authenticate owned RAM and
/// validate actual recovery rank before patching. This function performs no GF or I/O.
pub(crate) fn streaming_session_can_schedule_repair(nzb_id: i32) -> bool {
    let Some(entry) = active_streaming_session(nzb_id) else {
        return false;
    };
    if ensure_current_generation(nzb_id, &entry).is_err() {
        return false;
    }
    // Hold ownership stable while sampling the engine counters. Otherwise a fold
    // could finish and release its active bound after a stale status read.
    // The fold never holds the article mutex while calling the engine.
    let queue = entry.articles.lock().unwrap_or_else(|e| e.into_inner());
    let status = entry.session.status();
    if status.early_cutover_possible {
        return true;
    }
    let size = entry.session.slice_size();
    let queued_upper_bound = queue
        .pending
        .iter()
        .filter(|article| article.bytes.is_some())
        .fold(queue.active_upper_bound, |sum, article| {
            sum.saturating_add(
                (article.offset % size)
                    .saturating_add(article.size)
                    .div_ceil(size),
            )
        });
    let unknown = status.unarrived + status.partially_arrived + status.damaged;
    unknown as u64 <= (status.ready_recovery as u64).saturating_add(queued_upper_bound)
}

/// Missing repair targets one admission created: each under a temporary name until the
/// admission succeeds, then under its target name. On failure the temporary names are
/// removed, and the target names already given only while each still points at the file
/// this admission created (checked right before the unlink), so a file the direct renamer
/// moves to a target's name meanwhile is never removed.
struct CreatedTargets<'a> {
    writers: &'a WriteCoordinator,
    /// (temporary name, target path, index into the admitted files)
    temps: Vec<(std::ffi::CString, PathBuf, usize)>,
    /// Target names given, with the file each one was given to.
    named: Vec<(PathBuf, std::fs::File)>,
}

impl CreatedTargets<'_> {
    /// Gives every created file its target name, never replacing a file that has the name
    /// (see `parlive::name_repair_temp`), updating `files` where the name had to be created
    /// directly. Every name is checked free first.
    fn name_all(
        &mut self,
        files: &mut [par_rs::RepairTargetFile],
    ) -> Result<(), RepairError> {
        let unproven = || live_error("live target admission", "target ownership unproven");
        for (_, path, _) in &self.temps {
            if !crate::parlive::repair_target_name_is_free(self.writers, path)
                .map_err(|_| unproven())?
            {
                return Err(unproven());
            }
        }
        let mut modes = [0usize; 3];
        while let Some((temp, path, index)) = self.temps.first().cloned() {
            let file = &mut files[index].file;
            // The handle that records a given name, taken before anything changes.
            let Ok(handle) = file.try_clone() else {
                return Err(unproven());
            };
            match crate::parlive::name_repair_temp(self.writers, &temp, &path) {
                Ok(naming) => {
                    self.temps.remove(0);
                    let named = match naming {
                        crate::parlive::TargetNaming::Linked => {
                            modes[0] += 1;
                            handle
                        }
                        crate::parlive::TargetNaming::Renamed => {
                            modes[1] += 1;
                            handle
                        }
                        crate::parlive::TargetNaming::Created(created) => {
                            modes[2] += 1;
                            let Ok(named) = created.try_clone() else {
                                // The name exists and is this file: record it to roll back.
                                self.named.push((path, created));
                                return Err(unproven());
                            };
                            *file = created;
                            named
                        }
                    };
                    self.named.push((path, named));
                }
                Err((_, left)) => {
                    match left {
                        crate::parlive::NamingLeft::Temp => {}
                        crate::parlive::NamingLeft::Named => {
                            // Linked, but the temporary name stayed: both names are this file.
                            self.named.push((path.clone(), handle));
                        }
                        crate::parlive::NamingLeft::Nothing => {
                            self.temps.remove(0);
                        }
                    }
                    return Err(unproven());
                }
            }
        }
        if modes.iter().sum::<usize>() > 0 {
            let message = format!(
                "Live PAR: created {} missing repair target(s): {} by hard link, {} by rename, {} directly (no hard links on this filesystem)",
                modes.iter().sum::<usize>(),
                modes[0],
                modes[1],
                modes[2],
            );
            crate::log::log_active_bytes(crate::queuesubsys_host::MK_DETAIL, message.as_bytes());
        }
        Ok(())
    }

    fn keep(mut self) {
        self.temps.clear();
        self.named.clear();
    }
}

impl Drop for CreatedTargets<'_> {
    fn drop(&mut self) {
        for (temp, path, _) in self.temps.drain(..) {
            let _ = crate::parlive::unlink_repair_temp(self.writers, &temp, &path);
        }
        for (path, file) in self.named.drain(..) {
            let _ = crate::parlive::remove_named_repair_target(self.writers, &path, &file);
        }
    }
}

/// Opens the repair targets under the write barrier. An existing file is opened as it is;
/// a missing one is created under a temporary name, and named as the target only once every
/// target is open, the session is still current and the download folder is still the one
/// bound (before the targets are checked or published). On any error, cancellation
/// included, the temporary names are removed and the target names already given are
/// removed only while they still point at the files created here, so a cancelled or refused
/// repair leaves no empty file behind and never removes another file.
fn open_live_repair_targets(
    nzb_id: i32,
    entry: &ActiveStreamingSession,
    targets: &[(PathBuf, FileId)],
    cancelled: impl Fn() -> bool,
) -> Result<Vec<par_rs::RepairTargetFile>, RepairError> {
    let unproven = || live_error("live target admission", "target ownership unproven");
    // Declared before `created`, so a rollback (its drop) still runs under the lock.
    let naming = crate::parlive::target_naming_lock(nzb_id);
    let mut naming_guard = None;
    let mut created = CreatedTargets {
        writers: &entry.writers,
        temps: Vec::new(),
        named: Vec::new(),
    };
    let mut files = Vec::with_capacity(targets.len());
    for (path, file_id) in targets {
        if cancelled() {
            return Err(RepairError::Cancelled);
        }
        let file = match crate::parlive::open_repair_target(&entry.writers, path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let (file, temp) = crate::parlive::create_repair_temp(&entry.writers, path)
                    .map_err(|_| unproven())?;
                created.temps.push((temp, path.clone(), files.len()));
                file
            }
            Err(_) => return Err(unproven()),
        };
        entry.note_admission_progress();
        files.push(par_rs::RepairTargetFile {
            file_id: *file_id,
            file,
        });
    }
    if cancelled() {
        return Err(RepairError::Cancelled);
    }
    if let Some((_, path, _)) = created.temps.first() {
        // From the free-name check through naming and any rollback, no rename, link or
        // removal in the folder (the direct renamer, file completion) can interleave.
        // Nothing below takes the queue lock or the cache flush lock.
        naming_guard = Some(naming.lock().unwrap_or_else(|e| e.into_inner()));
        crate::parlive::repair_root_is_current(&entry.writers, path).map_err(|_| unproven())?;
    }
    created.name_all(&mut files)?;
    if cancelled() {
        return Err(RepairError::Cancelled);
    }
    created.keep();
    drop(naming_guard);
    Ok(files)
}

/// Captured under the queue lock; it owns the original session and admitted metadata.
pub(crate) struct LiveRepairRequest {
    nzb_id: i32,
    entry: Arc<ActiveStreamingSession>,
    ids: Vec<(i32, FileId)>,
    targets: Vec<crate::parlive::WriterTarget>,
    charge: Arc<ControlLease>,
}
struct RepairTargetAdmission {
    targets: Vec<crate::parlive::WriterTarget>,
    charge: Arc<ControlLease>,
}
struct RepairFinished<F: FnOnce()> {
    finished: Option<F>,
}
impl<F: FnOnce()> Drop for RepairFinished<F> {
    fn drop(&mut self) {
        if let Some(finished) = self.finished.take() {
            finished();
        }
    }
}
pub(crate) struct TargetWriteLease {
    nzb_id: i32,
    ids: Vec<(i32, FileId)>,
    ownership: crate::parlive::RepairWriteLease,
    // Descriptors admitted after writers drained; repair and verification use
    // only these, and publication requires each path to still name them.
    paths: Vec<PathBuf>,
    files: Vec<par_rs::RepairTargetFile>,
    writer_wait_ns: u64,
    // Retain all live session resources until actual I/O and publication finish.
    entry: Arc<ActiveStreamingSession>,
}

pub(crate) fn streaming_session_prepare_repair(
    nzb_id: i32,
    targets: Vec<crate::parlive::WriterTarget>,
    charge: impl Into<Arc<ControlLease>>,
) -> Result<LiveRepairRequest, RepairError> {
    let admission = RepairTargetAdmission {
        targets,
        charge: charge.into(),
    };
    let targets = &admission.targets;
    let entry = active_streaming_session(nzb_id).ok_or(RepairError::Cancelled)?;
    ensure_current_generation(nzb_id, &entry)?;
    let bytes = targets
        .capacity()
        .checked_mul(std::mem::size_of::<crate::parlive::WriterTarget>())
        .and_then(|bytes| {
            targets.iter().try_fold(bytes, |total, target| {
                total.checked_add(target.path.capacity())
            })
        })
        .and_then(|bytes| bytes.checked_add(targets.len().checked_mul(128)?))
        .and_then(|bytes| bytes.checked_add(1024));
    if targets.is_empty()
        || targets.len() > 512
        || bytes.is_none_or(|bytes| bytes as u64 > admission.charge.bytes())
    {
        return Err(live_error(
            "live target admission",
            "target metadata exceeds reservation",
        ));
    }
    let mut ids: Vec<(i32, FileId)> = Vec::with_capacity(targets.len());
    for target in targets {
        let name = target
            .path
            .file_name()
            .ok_or_else(|| live_error("live target admission", "target has no filename"))?;
        let leaf = par_rs::filenames::os_str_bytes(name);
        // The par2 file under its own name in the repair destination.
        let by_name = entry
            .session
            .file_id_by_name(&leaf)
            .filter(|id| entry.base_dir.join(&entry.target_names[id]) == target.path);
        // The queue file's live binding: a file still under another (obfuscated) name is
        // repaired where it is, never written to under a par2 name it does not have yet.
        let bound = live_source_id(&entry, Some(target.file_id), &leaf, &[]).ok();
        let id = match (by_name, bound) {
            (Some(named), Some(bound)) if named != bound => {
                return Err(live_error(
                    "live target admission",
                    "target name and queue binding disagree",
                ));
            }
            (Some(id), _) => id,
            (None, Some(id)) if target.path.parent() == Some(entry.base_dir.as_path()) => id,
            (None, Some(_)) => {
                return Err(live_error(
                    "live target admission",
                    "target path differs from repair destination",
                ));
            }
            (None, None) => {
                return Err(live_error(
                    "live target admission",
                    "target is not in this PAR set",
                ));
            }
        };
        if ids.iter().any(|(_, known)| *known == id) {
            return Err(live_error(
                "live target admission",
                "two targets name one PAR file",
            ));
        }
        ids.push((target.file_id, id));
    }
    Ok(LiveRepairRequest {
        nzb_id,
        entry,
        ids,
        targets: admission.targets,
        charge: admission.charge,
    })
}
impl LiveRepairRequest {
    pub(crate) fn admission_hold(&self) -> Arc<ControlLease> {
        Arc::clone(&self.charge)
    }
    pub(crate) fn generation(&self) -> SessionGeneration {
        self.entry.generation
    }
    pub(crate) fn acquire(self) -> Result<TargetWriteLease, RepairError> {
        // Fence native cache flushing before installing a pending barrier. No queue lock here.
        let waited = std::time::Instant::now();
        let paths: Vec<PathBuf> = self.targets.iter().map(|t| t.path.clone()).collect();
        // Bind the physical root through the ordinary registry (alias and
        // cross-job checks) only when no source writer attested it yet, and
        // before the repair barrier, so the check never cancels this repair.
        if !self.entry.writers.has_physical_root() {
            for (path, (queue_id, _)) in paths.iter().zip(&self.ids) {
                if let Some(name) = path.file_name() {
                    if let Ok(context) = crate::parlive::source_write_context(
                        self.nzb_id,
                        *queue_id,
                        &self.entry.base_dir,
                        name.as_encoded_bytes(),
                    ) {
                        let _ = context.attest_existing_source();
                    }
                }
            }
        }
        // Cancelled while waiting for the cache fence (the job was deleted or stopped): give
        // up rather than block, since the canceller may hold the queue lock that the cache
        // flusher holding the fence waits for.
        let cancelled = || {
            self.entry.session.is_cancelled()
                || !self.entry.writers.is_current_generation(self.entry.generation)
        };
        let ownership = crate::articledl::with_live_cache_fence_unless(cancelled, || {
            self.entry
                .writers
                .begin_repair(self.entry.generation, self.targets, self.charge)
        })
        .ok_or(RepairError::Cancelled)?
        .map_err(|_| RepairError::Cancelled)?;
        self.entry.note_admission_progress();
        // Writers are drained: bind the actual target inodes under the barrier.
        let targets: Vec<(PathBuf, FileId)> = paths
            .iter()
            .zip(&self.ids)
            .map(|(path, (_, file_id))| (path.clone(), *file_id))
            .collect();
        let files = open_live_repair_targets(self.nzb_id, &self.entry, &targets, cancelled)?;
        Ok(TargetWriteLease {
            nzb_id: self.nzb_id,
            ids: self.ids,
            ownership,
            paths,
            files,
            writer_wait_ns: live_metrics::nanos(waited.elapsed()),
            entry: self.entry,
        })
    }
}
impl TargetWriteLease {
    pub(crate) fn repair_and_publish(
        self,
        publish: impl FnOnce(&[i32]) -> usize,
        finished: impl FnOnce(),
    ) -> Result<(EarlyRepairReport, usize), RepairError> {
        let Self {
            nzb_id,
            ids,
            ownership,
            paths,
            files,
            writer_wait_ns,
            entry,
        } = self;
        let targets_current = || {
            paths.iter().zip(&files).try_for_each(|(path, target)| {
                crate::parlive::repair_target_is_current(&entry.writers, path, &target.file)
            })
            .map_err(|_| live_error("live target ownership", "target replaced after admission"))
        };
        entry
            .metrics
            .writer_wait_ns
            .fetch_add(writer_wait_ns, Ordering::Relaxed);
        let waited = std::time::Instant::now();
        let _operation = entry.operation.lock().unwrap_or_else(|e| e.into_inner());
        // Reset helper state before an article drainer can acquire this operation.
        // The callback is metadata-only and must never reacquire the daemon queue.
        let _finished = RepairFinished {
            finished: Some(finished),
        };
        entry
            .metrics
            .operation_wait_ns
            .fetch_add(live_metrics::nanos(waited.elapsed()), Ordering::Relaxed);
        ensure_current_generation(nzb_id, &entry)?;
        // The final queue callback can precede its deferred RAM fold. Consume the
        // admitted snapshots here before ordinary postprocessing may take over.
        drain_live_articles_locked(nzb_id, &entry)?;
        let par_ids: Vec<_> = ids.iter().map(|(_, id)| *id).collect();
        targets_current()?;
        let (report, hashes) = entry.record(entry.timed(nzb_id, || {
            entry
                .session
                .repair_available_targets_with_files_and_hashes_now(&par_ids, &files)
        }))?;
        ensure_current_generation(nzb_id, &entry)?;
        targets_current()?;
        // The library checks both the whole-file MD5 and actual file length under our lease.
        let verified: Vec<_> = ids
            .iter()
            .filter(|(_, id)| {
                report.files.iter().any(|(fid, status, ok)| {
                    fid == id && *ok && *status == VolumeStatus::CompleteAndVerified
                })
            })
            .map(|(queue_id, _)| *queue_id)
            .collect();
        let crcs: Vec<_> = ids
            .iter()
            .filter_map(|(queue_id, par_id)| {
                verified
                    .contains(queue_id)
                    .then(|| {
                        hashes
                            .iter()
                            .find(|(file_id, _)| file_id == par_id)
                            .map(|(_, hash)| (*queue_id, hash.crc32))
                    })
                    .flatten()
            })
            .collect();
        if crcs.len() != verified.len() {
            return Err(live_error(
                "live verification",
                "verified target hash missing",
            ));
        }
        let released = ownership
            .publish(&crcs, || {
                ensure_registry_generation(nzb_id, &entry)?;
                let released = publish(&verified);
                if released > 0 {
                    entry.record_live_handoff();
                }
                Ok(released)
            })
            .map_err(|_| RepairError::Cancelled)??;
        entry.emit_live_metrics(nzb_id, "repair_complete");
        Ok((report, released))
    }
}

/// Repairs the specified target files immediately using streamed syndromes (`early_cutover_possible`).
pub fn streaming_session_repair_available_targets_now(
    nzb_id: i32,
    target_file_ids: &[FileId],
) -> Option<Result<EarlyRepairReport, RepairError>> {
    with_live_session(nzb_id, |entry| {
        entry.session.repair_available_targets_now(target_file_ids)
    })
}

/// Takes up to `max_ids` pending volume IDs recommended for unpausing by
/// `on_article_failed_permanently`, preserving any remaining IDs for subsequent calls.
pub fn streaming_session_take_unpause_ids_bounded(nzb_id: i32, max_ids: usize) -> Vec<u64> {
    if max_ids == 0 {
        return Vec::new();
    }
    let Some(entry) = active_streaming_session(nzb_id) else {
        return Vec::new();
    };
    let mut pending = entry
        .pending_unpause_ids
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let take_count = pending.len().min(max_ids);
    pending.drain(..take_count).collect()
}

/// Takes any pending volume IDs recommended for unpausing by `on_article_failed_permanently`.
pub fn streaming_session_take_unpause_ids(nzb_id: i32) -> Vec<u64> {
    streaming_session_take_unpause_ids_bounded(nzb_id, usize::MAX)
}

/// Returns `true` if `nzb_id` currently has an active [`StreamingParSession`].
pub fn streaming_session_has_session(nzb_id: i32) -> bool {
    let map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    map.contains_key(&nzb_id)
}

/// Drops the [`StreamingParSession`] for `nzb_id`.
#[track_caller]
pub fn streaming_session_forget(nzb_id: i32) {
    LIVE_POLLS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&nzb_id);
    if let Some(old) = active_streaming_session(nzb_id) {
        let at = std::panic::Location::caller();
        old.note_stop(nzb_id, &format!("forgotten from {}:{}", at.file(), at.line()));
        old.writers.cancel_generation_with(old.generation, || {
            old.session.cancel();
            let mut sessions = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
            if sessions
                .get(&nzb_id)
                .is_some_and(|entry| entry.generation == old.generation)
            {
                sessions.remove(&nzb_id);
            }
        });
        // Its slots and staging go with the last reference; give the freed
        // heap back to the OS. Off this thread: callers may hold the queue lock.
        drop(old);
        let _ = std::thread::Builder::new()
            .name("live-par-trim".into())
            .spawn(crate::memory_pressure::trim_heap);
    }
    COMPLETED_LIVE_FILES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|item| item.file.nzb_id != nzb_id);
}

// =============================================================================================
// 2. Lower-Level Pure-Memory Streaming Variant (`StreamingSliceAssembler` + `SyndromeBank`)
// =============================================================================================

/// Pure-memory streaming slice assembler + GF(2^16) syndrome bank (`par_rs::stream`).
/// Operates entirely in RAM with zero disk I/O.
pub struct MemoryStreamingSession {
    pub assembler: StreamingSliceAssembler,
    pub file_ids: Vec<FileId>,
}

impl MemoryStreamingSession {
    /// Creates a pure-memory streaming session from explicit [`FileLayout`]s and recovery exponents.
    pub fn new(
        slice_size: usize,
        files: Vec<FileLayout>,
        exponents: &[u32],
        kernel: Kernel,
        max_staging_bytes: usize,
    ) -> Result<Self, par_rs::repair::RepairError> {
        let file_ids = files.iter().map(|f| FileId(f.file_id)).collect();
        let assembler =
            StreamingSliceAssembler::new(slice_size, files, exponents, kernel, max_staging_bytes)?;
        Ok(Self {
            assembler,
            file_ids,
        })
    }

    /// Creates a pure-memory streaming session from a [`PacketSet`] and recovery exponents.
    pub fn from_packet_set(
        set: &PacketSet,
        exponents: &[u32],
        max_staging_bytes: usize,
    ) -> Result<Self, RepairError> {
        let main = set.main.as_ref().ok_or(RepairError::NoMainPacket)?;
        let mut files = Vec::with_capacity(main.recoverable_file_ids.len());
        let mut file_ids = Vec::with_capacity(main.recoverable_file_ids.len());
        for id in &main.recoverable_file_ids {
            let fd = set
                .file_descs
                .get(id)
                .ok_or_else(|| RepairError::IncompleteMetadata {
                    missing_descriptions: vec![*id],
                })?;
            let block_count = main.slice_count_for(fd.file_length) as usize;
            let slices = match set.ifscs.get(id) {
                Some(ifsc) if ifsc.slices.len() == block_count => ifsc.slices.clone(),
                _ => vec![
                    SliceChecksum {
                        md5: [0u8; 16],
                        crc32: 0
                    };
                    block_count
                ],
            };
            files.push(FileLayout {
                file_id: id.0,
                size: fd.file_length,
                slices,
            });
            file_ids.push(*id);
        }
        let assembler = StreamingSliceAssembler::new(
            main.slice_size as usize,
            files,
            exponents,
            Kernel::detect(),
            max_staging_bytes,
        )
        .map_err(|_| RepairError::Singular)?;
        Ok(Self {
            assembler,
            file_ids,
        })
    }

    /// Feeds an arbitrary chunk (e.g. a yEnc article) at `byte_offset` for `file_id`, verifying
    /// completed slices against IFSC CRC32/MD5 and folding them into the internal [`SyndromeBank`].
    pub fn push_chunk(
        &mut self,
        file_id: &[u8; 16],
        byte_offset: u64,
        data: &[u8],
    ) -> Result<Vec<StreamEvent>, par_rs::repair::RepairError> {
        self.assembler.push_chunk(file_id, byte_offset, data)
    }

    /// Marks `[byte_offset, byte_offset + len)` in `file_id` as permanently damaged.
    pub fn mark_range_damaged(
        &mut self,
        file_id: &[u8; 16],
        byte_offset: u64,
        len: u64,
    ) -> Vec<StreamEvent> {
        self.assembler.mark_range_damaged(file_id, byte_offset, len)
    }

    /// Supplies a recovery block for `exponent`.
    pub fn push_recovery(
        &mut self,
        exponent: u32,
        data: &[u8],
    ) -> Result<Vec<StreamEvent>, par_rs::repair::RepairError> {
        self.assembler.push_recovery(exponent, data)
    }

    /// Snapshot of current progress and whether early repair is possible.
    pub fn status(&self) -> StreamStatus {
        self.assembler.status()
    }

    /// Whether early solve is currently possible.
    pub fn can_solve_now(&self) -> bool {
        self.assembler.can_solve_now()
    }

    /// Reconstructs all non-verified slices once `can_solve_now()` is true.
    pub fn solve_now(&mut self) -> Result<Vec<(usize, Vec<u8>)>, par_rs::repair::RepairError> {
        self.assembler.solve_now()
    }

    /// Reconstructs only the requested global slice indices (`wanted_globals`).
    pub fn solve_selected_slices(
        &self,
        wanted_globals: &[usize],
    ) -> Result<Vec<(usize, Vec<u8>)>, par_rs::repair::RepairError> {
        self.assembler.solve_selected_slices(wanted_globals)
    }
}

// =============================================================================================
// 3. Post-Download `Par2Repairer` Engine (`RustParRepairer`)
// =============================================================================================

#[inline]
fn os_str_bytes(s: &std::ffi::OsStr) -> &[u8] {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        s.as_bytes()
    }
    #[cfg(not(unix))]
    {
        s.to_str().unwrap_or_default().as_bytes()
    }
}

/// Safe Rust event sink for [`RustParRepairer`].
pub trait ParRepairEvents {
    fn on_filename(&self, _name: &[u8]) {}
    fn on_progress(&self, _progress: i32) {}
    fn on_done(&self, _name: &[u8], _available: i32, _total: i32, _target_exists: bool) {}
    fn on_quick_verify(&self, _name: &[u8]) -> Option<QuickInfo> {
        None
    }
    fn on_quick_verified_inc(&self) {}
}

pub struct NoopParEvents;
impl ParRepairEvents for NoopParEvents {}

struct CallbackObserver<'a> {
    events: &'a dyn ParRepairEvents,
    emit_file_signals: bool,
}

impl RepairObserver for CallbackObserver<'_> {
    fn progress(&mut self, permille: u32, _label: &str) {
        self.events.on_progress(permille as i32);
    }

    fn file_verified(&mut self, file: &SourceFileState) {
        if !self.emit_file_signals {
            return;
        }
        let name = file.target_filename.as_bytes();
        if file.target_exists {
            self.events.on_filename(name);
        }
        if file.quick_verified {
            self.events.on_quick_verified_inc();
        }
        let file_exists = file.target_exists || crate::paths::base_file_name(name) > 0;
        self.events.on_done(
            name,
            file.available_blocks as i32,
            file.block_count as i32,
            file_exists,
        );
        self.events.on_progress(1000);
    }
}

struct CallbackQuickVerifier<'a> {
    events: &'a dyn ParRepairEvents,
}

impl QuickVerifier for CallbackQuickVerifier<'_> {
    fn quick_info(&mut self, target_filename: &str) -> Option<QuickInfo> {
        self.events.on_quick_verify(target_filename.as_bytes())
    }
}

/// The batch fallback, for when live repair did not finish the job during the download:
/// write only the missing blocks into the damaged files (no full-size backup copies), keep
/// recovery volumes out of the page cache so the files extracted next stay cached, and when
/// a post-repair check runs, read back only the slices the repair wrote.
fn configure_batch_repairer(repairer: &mut Par2Repairer) {
    repairer.set_in_place_repair(true);
    repairer.set_uncached_recovery_reads(true);
    repairer.set_verify_rebuilt_slices_only(true);
}

/// The one-line phase split of a batch repair (`repair_timings`, par-rs #79), in seconds,
/// and the files it repaired in place.
fn batch_repair_timing_line(
    timings: &par_rs::repairer::RepairTimings,
    in_place: &[par_rs::repairer::InPlaceRepair],
    name: &str,
    ok: bool,
) -> String {
    let secs = |d: std::time::Duration| format!("{:.2}", d.as_secs_f64());
    let blocks: u64 = in_place.iter().map(|f| u64::from(f.blocks_written)).sum();
    format!(
        "PAR repair {} for {name}: verify {} s, backups {} s, plan {} s, reconstruct {} s, sync {} s, total {} s; {} file(s) repaired in place ({blocks} block(s) written)",
        if ok { "timings" } else { "timings (failed)" },
        secs(timings.verify),
        secs(timings.backups),
        secs(timings.plan),
        secs(timings.reconstruct),
        secs(timings.sync),
        secs(timings.total),
        in_place.len(),
    )
}

fn log_batch_repair(repairer: &Par2Repairer, base_dir: &Path, ok: bool) {
    let name = base_dir
        .file_name()
        .map_or_else(|| base_dir.display().to_string(), |n| n.to_string_lossy().into_owned());
    let in_place = repairer.in_place_repairs();
    let line = batch_repair_timing_line(&repairer.repair_timings(), in_place, &name, ok);
    crate::log::log_active_bytes(crate::queuesubsys_host::MK_INFO, line.as_bytes());
    for file in in_place {
        let msg = format!(
            "Rewrote {} of {} blocks of {} in place",
            file.blocks_written, file.block_count, file.target_filename
        );
        crate::log::log_active_bytes(crate::queuesubsys_host::MK_DETAIL, msg.as_bytes());
    }
}

/// Stateful post-download repairer backing `ParChecker.cpp`.
pub struct RustParRepairer {
    pub repairer: Par2Repairer,
    pub base_dir: PathBuf,
    pub threads_to_use: i32,
    pub mem_to_use_mb: i32,
    pub par_quick: bool,
    pub par_scan_full: bool,
    pub par_ignore_ext: Vec<u8>,
    pub has_verified: bool,
    pub split_candidates: Vec<PathBuf>,
    pub leftovers: Vec<PathBuf>,
}

impl RustParRepairer {
    pub fn new(
        base_dir: PathBuf,
        threads_to_use: i32,
        mem_to_use_mb: i32,
        par_quick: bool,
        par_scan_full: bool,
        par_ignore_ext: Vec<u8>,
    ) -> Self {
        let ignore_extensions = parse_ignore_ext_tokens(&par_ignore_ext);
        let options = RepairOptions {
            num_threads: threads_to_use.max(1) as usize,
            mem_limit_bytes: (mem_to_use_mb.max(16) as u64) << 20,
            backup: BackupPolicy::RenameNumbered,
            verify_repaired: !par_quick,
            sync_repaired: false,
            rename_misnamed: true,
            full_file_md5: false,
            ignore_extensions,
            ..RepairOptions::default()
        };
        let mut repairer = Par2Repairer::new(&base_dir, options);
        configure_batch_repairer(&mut repairer);
        Self {
            repairer,
            base_dir,
            threads_to_use,
            mem_to_use_mb,
            par_quick,
            par_scan_full,
            par_ignore_ext,
            has_verified: false,
            split_candidates: Vec::new(),
            leftovers: Vec::new(),
        }
    }

    pub fn preprocess(&mut self, par_filename: &Path, events: Option<&dyn ParRepairEvents>) -> ParResult {
        if !par_filename.is_file() {
            return ParResult::InsufficientCriticalData;
        }
        if self.repairer.load_par2_set(par_filename).is_err() {
            return ParResult::InsufficientCriticalData;
        }
        if let Some(events) = events {
            let main_name = par_filename
                .file_name()
                .map(os_str_bytes)
                .unwrap_or_default();
            events.on_filename(main_name);
            if let Some(parent) = par_filename.parent() {
                if let Ok(entries) = std::fs::read_dir(parent) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p == par_filename || !p.is_file() {
                            continue;
                        }
                        let Some(os_name) = p.file_name() else {
                            continue;
                        };
                        let name = os_str_bytes(os_name);
                        if name.len() >= 5
                            && name[name.len() - 5..].eq_ignore_ascii_case(b".par2")
                            && same_par_collection(name, main_name, false)
                        {
                            events.on_filename(name);
                        }
                    }
                }
            }
        }
        match self.repairer.finalize() {
            Ok(()) => ParResult::Success,
            Err(_) => ParResult::InsufficientCriticalData,
        }
    }

    pub fn process(&mut self, do_repair: bool, events: &dyn ParRepairEvents) -> ParResult {
        if !do_repair {
            if !self.has_verified {
                let mut obs = CallbackObserver {
                    events,
                    emit_file_signals: true,
                };
                let res = if self.par_quick {
                    let mut qv = CallbackQuickVerifier { events };
                    self.repairer.verify(Some(&mut qv), &mut obs)
                } else {
                    self.repairer.verify(None, &mut obs)
                };
                if let Err(err) = res {
                    return map_repair_error(err);
                }
                if self.par_scan_full {
                    let mut extra_obs = CallbackObserver {
                        events,
                        emit_file_signals: false,
                    };
                    let _ = self
                        .repairer
                        .add_extra_files_from_dir(&self.base_dir, &mut extra_obs);
                }
                self.has_verified = true;
            }
            let v = self.repairer.verify_result();
            if !v.repair_needed {
                ParResult::Success
            } else if v.repair_possible {
                ParResult::RepairPossible
            } else {
                ParResult::RepairNotPossible
            }
        } else {
            let mut obs = CallbackObserver {
                events,
                emit_file_signals: false,
            };
            let repaired_filenames: Vec<Vec<u8>> = self
                .repairer
                .source_files()
                .iter()
                .filter(|sf| sf.status != FileStatus::Complete)
                .map(|sf| sf.target_filename.as_bytes().to_vec())
                .collect();
            let used_split_fragments: Vec<PathBuf> = self
                .split_candidates
                .iter()
                .filter(|frag| {
                    let used = self
                        .repairer
                        .disk_files()
                        .iter()
                        .position(|p| p == *frag)
                        .is_some_and(|di| {
                            self.repairer
                                .block_locations()
                                .iter()
                                .flatten()
                                .any(|loc| loc.file == di)
                        });
                    let is_target = self
                        .repairer
                        .source_files()
                        .iter()
                        .any(|f| &f.target_path == *frag);
                    used && !is_target
                })
                .cloned()
                .collect();
            let result = self.repairer.repair(&mut obs);
            log_batch_repair(&self.repairer, &self.base_dir, result.is_ok());
            match result {
                Ok(RepairResult::NotNeeded) => ParResult::Success,
                Ok(RepairResult::Repaired { mut leftovers, .. }) => {
                    for name in &repaired_filenames {
                        events.on_filename(name);
                    }
                    for frag in used_split_fragments {
                        if frag.is_file() && !leftovers.contains(&frag) {
                            leftovers.push(frag);
                        }
                    }
                    self.leftovers = leftovers;
                    ParResult::Success
                }
                Err(err) => map_repair_error(err),
            }
        }
    }

    pub fn load_extra_par2_file(
        &mut self,
        path: &Path,
        events: Option<&dyn ParRepairEvents>,
    ) -> bool {
        if !path.is_file() {
            return false;
        }
        let before_files = self.repairer.packets().recovery.len()
            + self.repairer.packets().file_descs.len()
            + usize::from(self.repairer.packets().main.is_some());
        let ok = match self.repairer.load_par2_file(path) {
            Ok(added_recovery) => {
                if added_recovery > 0 {
                    true
                } else if let Ok(set) = load_par2_file(path, ScanOptions::METADATA) {
                    set.set_id.is_some() && set.set_id == self.repairer.set_id()
                } else {
                    before_files > 0
                }
            }
            Err(_) => false,
        };
        if ok {
            if let Some(events) = events {
                if let Some(name) = path.file_name() {
                    events.on_filename(os_str_bytes(name));
                }
            }
        }
        ok
    }

    pub fn verify_extra_file(
        &mut self,
        path: &Path,
        is_split_fragment: bool,
        events: &dyn ParRepairEvents,
    ) -> bool {
        if !path.is_file() {
            return false;
        }
        let base_name = path
            .file_name()
            .map(os_str_bytes)
            .unwrap_or_default()
            .to_vec();
        events.on_filename(&base_name);
        let was_missing_files = self.missing_file_count();
        let mut obs = CallbackObserver {
            events,
            emit_file_signals: false,
        };
        let gained = self.repairer.add_extra_file(path, &mut obs).unwrap_or(0);
        if is_split_fragment && gained > 0 && !self.split_candidates.iter().any(|p| p == path) {
            self.split_candidates.push(path.to_path_buf());
        }
        events.on_done(&base_name, gained as i32, gained as i32, true);
        gained > 0 || self.missing_file_count() < was_missing_files
    }

    pub fn cancel(&self) {
        self.repairer.cancel();
    }

    pub fn cancel_token(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.repairer.cancel_token()
    }

    pub fn creator(&self) -> String {
        self.repairer
            .packets()
            .creator
            .as_ref()
            .map(|c| c.client())
            .unwrap_or_default()
    }

    pub fn block_size(&self) -> i64 {
        self.repairer.block_size() as i64
    }

    pub fn source_files(&self) -> &[SourceFileState] {
        self.repairer.source_files()
    }

    pub fn leftovers(&self) -> &[PathBuf] {
        &self.leftovers
    }

    pub fn missing_file_count(&self) -> u32 {
        self.repairer
            .source_files()
            .iter()
            .filter(|f| !f.ignored && f.status == FileStatus::Missing)
            .count() as u32
    }

    pub fn damaged_file_count(&self) -> u32 {
        self.repairer
            .source_files()
            .iter()
            .filter(|f| {
                !f.ignored && matches!(f.status, FileStatus::Damaged | FileStatus::Misnamed)
            })
            .count() as u32
    }

    pub fn missing_block_count(&self) -> u32 {
        self.repairer.missing_blocks() as u32
    }

    pub fn recovery_block_count(&self) -> u32 {
        self.repairer.recovery_blocks() as u32
    }

    /// `ParChecker::DeleteLeftovers`: logs and deletes every leftover file recorded by `repair()`.
    pub fn delete_leftovers(&self, log_cb: &mut dyn FnMut(&[u8])) {
        for path in &self.leftovers {
            let path_bytes = crate::fsops::path_to_bytes(path);
            if !path_bytes.is_empty() {
                let base = &path_bytes[crate::paths::base_file_name(path_bytes)..];
                let msg = [b"Deleting file ", base].concat();
                log_cb(&msg);
                if let Ok(cpath) = std::ffi::CString::new(path_bytes) {
                    let _ = crate::fsops::delete_file(&cpath);
                }
            }
        }
    }

    /// `ParChecker::CheckEmptyFiles`: when `ParQuick` is disabled, checks any source file that was
    /// not yet added to `processed_files` (e.g., empty damaged files), emits its warning message,
    /// and returns `true` if any unignored damaged file was found.
    pub fn check_empty_files(
        &self,
        processed_files: &[&[u8]],
        log_cb: &mut dyn FnMut(&[u8]),
    ) -> bool {
        let mut found_unignored_damaged = false;
        for sf in self.source_files() {
            let filename = sf.target_filename.as_bytes();
            if !filename.is_empty() && !is_processed_file(filename, processed_files) {
                let ignore =
                    crate::util::match_file_ext_locale(filename, &self.par_ignore_ext, b",;");
                found_unignored_damaged |= !ignore;
                let suffix: &[u8] = if ignore { b" (ignored)" } else { b"" };
                let msg = [
                    b"File ",
                    filename,
                    format!(
                        " has {} bad block(s) of total {} block(s)",
                        sf.block_count, sf.block_count
                    )
                    .as_bytes(),
                    suffix,
                ]
                .concat();
                log_cb(&msg);
            }
        }
        found_unignored_damaged
    }

    /// Used by `ParChecker::SignalDone`: returns `false` if any missing (`!target_exists`) source
    /// file in this set has a base filename equal to `file_name`.
    pub fn target_file_exists_by_basename(&self, file_name: &[u8]) -> bool {
        for sf in self.source_files() {
            let target_bytes = sf.target_filename.as_bytes();
            let base = &target_bytes[crate::paths::base_file_name(target_bytes)..];
            if !sf.target_exists && base == file_name {
                return false;
            }
        }
        true
    }

    /// Used at the end of `ParChecker::RunParCheck` to update `m_verifiedFiles`: invokes
    /// `cb(target_path_bytes, keep_verified)` for each source file in the set.
    pub fn for_each_target_verification(
        &self,
        set_succeeded: bool,
        cb: &mut dyn FnMut(&[u8], bool),
    ) {
        for sf in self.source_files() {
            let target_path = crate::fsops::path_to_bytes(&sf.target_path);
            let is_complete_target = sf.status == FileStatus::Complete
                && sf.complete_file.as_ref() == Some(&sf.target_path);
            cb(target_path, set_succeeded && is_complete_target);
        }
    }

    /// `ParChecker::AddSplittedFragments` candidate discovery: scans `dest_dir` for split fragments
    /// matching any source file in the PAR2 set (or its pre-rename original name).
    pub fn discover_splitted_fragments(
        &self,
        dest_dir: &[u8],
        processed_files: &[&[u8]],
        is_parred_cb: &mut dyn FnMut(&[u8]) -> bool,
        find_origname_cb: &mut dyn FnMut(&[u8]) -> Option<Vec<u8>>,
        detail_cb: &mut dyn FnMut(&[u8]),
    ) -> Vec<Vec<u8>> {
        let mut extrafiles = Vec::new();
        let sep = crate::paths::PATH_SEPARATOR;
        for filename in crate::fsops::list_dir_entries(dest_dir) {
            if !is_parred_cb(&filename) && !is_processed_file(&filename, processed_files) {
                for sf in self.source_files() {
                    let target = sf.target_filename.as_bytes();
                    let current = &target[crate::paths::base_file_name(target)..];
                    let orig = find_origname_cb(current);
                    let matched = maybe_splitted_fragment(&filename, current)
                        || orig.as_deref().is_some_and(|o| {
                            !o.eq_ignore_ascii_case(current)
                                && maybe_splitted_fragment(&filename, o)
                        });
                    if matched {
                        let msg = [b"Found splitted fragment ", filename.as_slice()].concat();
                        detail_cb(&msg);
                        extrafiles.push([dest_dir, &[sep], filename.as_slice()].concat());
                        break;
                    }
                }
            }
        }
        extrafiles
    }
}

/// `ParChecker::IsProcessedFile`: returns `true` if `filename` matches the base filename of any
/// entry in `processed_files` (case-insensitively).
pub fn is_processed_file(filename: &[u8], processed_files: &[&[u8]]) -> bool {
    processed_files.iter().any(|p| {
        let base = &p[crate::paths::base_file_name(p)..];
        base.eq_ignore_ascii_case(filename)
    })
}

fn collect_extra_files_rec(
    directory: &[u8],
    external_dir: bool,
    processed_files: &[&[u8]],
    is_parred_cb: &mut dyn FnMut(&[u8]) -> bool,
    out: &mut Vec<Vec<u8>>,
) {
    let sep = crate::paths::PATH_SEPARATOR;
    for filename in crate::fsops::list_dir_entries(directory) {
        let full = [directory, &[sep], &filename].concat();
        if crate::fsops::directory_exists(&full, &crate::fsops::posix_stat) {
            collect_extra_files_rec(&full, external_dir, processed_files, is_parred_cb, out);
        } else {
            let is_parred = !external_dir && is_parred_cb(&filename);
            let is_processed =
                !external_dir && !is_parred && is_processed_file(&filename, processed_files);
            if external_dir || (!is_parred && !is_processed) {
                out.push(full);
            }
        }
    }
}

/// `ParChecker::FindExtraFiles` + `ParChecker::SortExtraFiles`:
/// Recursively collects candidate extra files under `directory` and sorts files whose stem matches
/// the `.par2` stem ahead of other files.
pub fn find_and_sort_extra_files(
    directory: &[u8],
    external_dir: bool,
    par_filename: &[u8],
    processed_files: &[&[u8]],
    is_parred_cb: &mut dyn FnMut(&[u8]) -> bool,
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    collect_extra_files_rec(
        directory,
        external_dir,
        processed_files,
        is_parred_cb,
        &mut out,
    );
    let base_par_stem = extract_base_par_stem(par_filename);
    out.sort_by_key(|a| {
        let base = &a[crate::paths::base_file_name(a)..];
        let stem = match base.iter().rposition(|&b| b == b'.') {
            Some(pos) => &base[..pos],
            None => base,
        };
        stem != base_par_stem.as_slice()
    });
    out
}

/// `ParIgnoreExt` as `RepairOptions::ignore_extensions`: the raw option bytes split on `,` /
/// `;` and trimmed, as `Util::MatchFileExt` does, each token passed through verbatim. A token
/// that is not UTF-8 is dropped on its own; the others stay. par-rs applies nzbget's rules to
/// each: a case-insensitive suffix match on the local name (`.nfo`, `nfo`, a literal
/// `readme.nfo`), and for a token with `*` or `?` a `WildMask` match over the whole local name
/// (`*` any run, `?` one byte, `#` one digit).
fn parse_ignore_ext_tokens(raw: &[u8]) -> Vec<String> {
    raw.split(|b| *b == b',' || *b == b';')
        .map(|t| t.trim_ascii())
        .filter(|t| !t.is_empty())
        .filter_map(|t| std::str::from_utf8(t).ok())
        .map(str::to_string)
        .collect()
}

fn map_repair_error(err: RepairError) -> ParResult {
    match err {
        RepairError::NoMainPacket | RepairError::IncompleteMetadata { .. } => {
            ParResult::InsufficientCriticalData
        }
        RepairError::NotEnoughRecovery { .. } => ParResult::RepairNotPossible,
        RepairError::Io { .. } => ParResult::FileIoError,
        RepairError::Cancelled
        | RepairError::VerificationFailed { .. }
        | RepairError::Singular
        | RepairError::StreamingMemoryLimit { .. }
        | RepairError::RecoveryMemoryLimit { .. } => ParResult::RepairFailed,
        RepairError::NotFinalized | RepairError::WrongSet { .. } | RepairError::UnknownFile(_) => {
            ParResult::LogicError
        }
    }
}

/// Computes `(threads_to_use, mem_to_use_mb)` for `Repairer::Repairer`.
/// Threads for a batch repair. `ParThreads` from 1 to the logical CPU count is kept. With
/// `ParThreads=0`, or more threads than logical CPUs (the "uber" preset asks for two more;
/// a 6C/12T i7-8700K repaired no faster with 14 than with 12): the physical cores while
/// downloads or extraction share the CPU (6 threads cost 5-14% wall time there and save
/// 36% CPU), every logical CPU when the repair runs alone.
pub fn batch_repair_threads(par_threads: i32, logical: i32, physical: i32, busy: bool) -> i32 {
    let logical = logical.max(1);
    if par_threads > 0 && par_threads <= logical {
        return par_threads;
    }
    if busy {
        physical.clamp(1, logical)
    } else {
        logical
    }
}

/// Whether downloads or extraction share the CPU with a repair starting now.
pub(crate) fn repair_shares_cpu() -> bool {
    let downloading = crate::statmeter::active_statmeter()
        .is_some_and(|sm| sm.current_download_speed() > 0);
    downloading || crate::postproc::extraction_running()
}

pub fn calc_repairer_resources(
    par_threads: i32,
    cpu_cores: i32,
    has_memory_monitor: bool,
    monitor_mem_mb: i32,
    configured_par_buffer_mb: i32,
) -> (i32, i32) {
    let effective_mem_mb = if has_memory_monitor {
        monitor_mem_mb
    } else if configured_par_buffer_mb > 0 {
        configured_par_buffer_mb
    } else {
        16
    };
    let raw_threads = if par_threads > 0 {
        par_threads
    } else {
        cpu_cores
    };
    let threads = if raw_threads > 0 { raw_threads } else { 1 };
    let mem = effective_mem_mb.max(16);
    (threads, mem)
}

/// `ParChecker::StreamBuf::overflow` line classifier:
/// Returns `(extra_debug, msg_offset)`.
pub fn classify_streambuf_line(msg: &[u8]) -> (bool, usize) {
    let extra_debug = msg.contains(&b'%')
        || msg.starts_with(b"Loading")
        || (msg.starts_with(b"Target: ") && !msg.ends_with(b"found"));
    let offset = if msg.starts_with(b"You have ") { 9 } else { 0 };
    (extra_debug, offset)
}

/// `ParChecker::MaybeSplittedFragement` (par-rs's `renamer::maybe_split_fragment`).
pub use par_rs::renamer::maybe_split_fragment as maybe_splitted_fragment;

/// Extracts the base filename of `par_filename` with its last `.ext` stripped (`ParChecker::SortExtraFiles`).
pub fn extract_base_par_stem(par_filename: &[u8]) -> Vec<u8> {
    let base = &par_filename[crate::paths::base_file_name(par_filename)..];
    match base.iter().rposition(|&b| b == b'.') {
        Some(dot) => base[..dot].to_vec(),
        None => base.to_vec(),
    }
}

/// `ParChecker::SignalProgress` stage-progress calculation.
pub fn calc_stage_progress(
    stage: i32,
    file_progress: i32,
    processed_count: i32,
    files_to_repair: i32,
    source_files_count: i32,
    extra_files: i32,
    quick_files: i32,
) -> i32 {
    let stage_progress = if stage == PAR_STAGE_REPAIRING {
        file_progress
    } else {
        let mut total_files;
        let mut processed_files = processed_count;
        if stage == PAR_STAGE_VERIFYING_REPAIRED {
            total_files = files_to_repair;
        } else {
            total_files = source_files_count + extra_files;
            if extra_files > 0 {
                total_files -= quick_files;
                processed_files -= quick_files;
            }
        }

        if total_files > 0 {
            if file_progress < 1000 {
                (processed_files * 1000 + file_progress) / total_files
            } else {
                processed_files * 1000 / total_files
            }
        } else {
            0
        }
    };

    stage_progress.clamp(0, 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ignore_ext_tokens_keeps_wildcard_masks() {
        // Tokens are nzbget's: split on `,` / `;`, trimmed, otherwise verbatim (a space inside a
        // token is part of it; nothing is prefixed or dropped).
        assert_eq!(
            parse_ignore_ext_tokens(b".sfv, nfo;.NZB"),
            vec![".sfv", "nfo", ".NZB"]
        );
        assert_eq!(
            parse_ignore_ext_tokens(b"*, *.nfo ; sample##.mkv,my file.txt, readme.nfo"),
            vec!["*", "*.nfo", "sample##.mkv", "my file.txt", "readme.nfo"]
        );
        assert!(parse_ignore_ext_tokens(b"").is_empty());
        assert!(parse_ignore_ext_tokens(b" ,; ").is_empty());
        assert!(parse_ignore_ext_tokens(&[0xFF, 0xFE]).is_empty());
        // One bad token does not take the good ones with it.
        assert_eq!(
            parse_ignore_ext_tokens(b"*.nfo,\xff,.sfv"),
            vec!["*.nfo", ".sfv"]
        );
        // par-rs applies MatchFileExt + WildMask semantics to each token.
        let m = par_rs::repairer::ignore_name_matches;
        for token in parse_ignore_ext_tokens(b"*, *.nfo, nfo, .NFO") {
            assert!(m("release.nfo", &token), "{token}");
        }
        assert!(m("readme.nfo", "readme.nfo"), "literal file name");
        assert!(
            !m("extras/clip.mkv", "clip*"),
            "masks see the whole local name, as nzbget's do"
        );
        assert!(m("sample01.mkv", "sample##.*"), "# is one digit");
        assert!(!m("sample1.mkv", "sample##.*"));
        assert!(!m("release.rar", "*.nfo"));
    }

    #[test]
    fn live_plan_follows_expected_damage() {
        let hint = |failed_bytes, failed_articles, done_bytes, total_bytes| LivePlanHint {
            volumes: Vec::new(),
            failed_bytes,
            failed_articles,
            done_bytes,
            total_bytes,
        };
        // Nothing failed: no damage expected, a margin of 2 slots.
        assert_eq!(expected_damaged_slices(&hint(0, 0, 0, 1 << 30), 1 << 20), 0);
        assert_eq!(planned_recovery_slots(0), 2);
        // 2 articles (1.5 MB) failed in the first quarter: 4x over the whole download,
        // each article's slices plus a boundary slice.
        let h = hint(3 << 19, 2, 1 << 28, 1 << 30);
        assert_eq!(expected_damaged_slices(&h, 1 << 20), 6 + 8);
        assert_eq!(planned_recovery_slots(14), 14 + 3);
        assert_eq!(planned_recovery_slots(100), 120);
        assert_eq!(planned_recovery_slots(u64::MAX), 65535);
        assert_eq!(expected_damaged_slices(&h, 0), 0);
    }

    #[test]
    fn live_plan_takes_the_volumes_the_selector_unpauses_first() {
        let volume = |id, first_exponent, count, paused| LiveQueuedVolume {
            id,
            first_exponent,
            count,
            size: u64::from(count) * 1000,
            articles: count * 2,
            paused,
        };
        let mut hint = LivePlanHint {
            volumes: vec![
                volume(10, 0, 1, true),
                volume(11, 1, 2, true),
                volume(12, 3, 4, true),
                volume(13, 7, 8, true),
            ],
            ..LivePlanHint::default()
        };
        // 2 slots for no damage yet: the smallest volume that covers them (vol01+2), as
        // the session's selector unpauses it.
        let plan = live_recovery_plan(&hint, 1000, &HashSet::new());
        assert_eq!(plan.exponents, vec![1, 2]);
        // 5 damaged slices so far, half way: 10 expected, 12 planned.
        hint.failed_bytes = 5000;
        hint.done_bytes = 50_000;
        hint.total_bytes = 100_000;
        let plan = live_recovery_plan(&hint, 1000, &HashSet::new());
        assert_eq!(plan.exponents.len(), 12);
        assert!(plan.exponents.windows(2).all(|w| w[0] < w[1]));
        // Exponents already loaded get their slots anyway and are not planned twice.
        let loaded: HashSet<u32> = [0, 1, 2].into_iter().collect();
        hint.failed_bytes = 0;
        let plan = live_recovery_plan(&hint, 1000, &loaded);
        assert_eq!(plan.exponents.len(), 2);
        assert!(plan.exponents.iter().all(|e| !loaded.contains(e)));
        // No volume known: the first exponents.
        let plan = live_recovery_plan(&LivePlanHint::default(), 1000, &HashSet::new());
        assert_eq!(plan.exponents, vec![0, 1]);
    }

    #[test]
    fn live_repair_steps_have_rpc_names() {
        assert_eq!(live_repair_step_name(LiveRepairStep::Idle), "");
        assert_eq!(live_repair_step_name(LiveRepairStep::Matrix), "matrix");
        assert_eq!(live_repair_step_name(LiveRepairStep::Decode), "decode");
        assert_eq!(live_repair_step_name(LiveRepairStep::Sync), "sync");
        assert_eq!(live_repair_step_name(LiveRepairStep::PayloadWait), "waiting");
    }

    #[test]
    fn renamed_away_tolerates_only_the_rename_window() {
        let io = |kind| -> Result<(), RepairError> {
            Err(RepairError::Io {
                context: "reading".into(),
                source: std::io::Error::from(kind),
            })
        };
        assert!(renamed_away(&io(std::io::ErrorKind::NotFound), false));
        assert!(renamed_away(&io(std::io::ErrorKind::UnexpectedEof), true));
        assert!(!renamed_away(&io(std::io::ErrorKind::UnexpectedEof), false));
        assert!(!renamed_away(&Err::<(), _>(RepairError::Cancelled), true));
        assert!(!renamed_away(&Ok::<(), RepairError>(()), true));
    }

    #[test]
    fn batch_repair_timing_line_has_every_phase() {
        let timings = par_rs::repairer::RepairTimings::default();
        let line = batch_repair_timing_line(&timings, &[], "job", true);
        for phase in ["verify", "backups", "plan", "reconstruct", "sync", "total"] {
            assert!(line.contains(&format!("{phase} 0.00 s")), "{line}");
        }
        assert!(line.contains("0 file(s) repaired in place"));
        assert!(batch_repair_timing_line(&timings, &[], "job", false).contains("(failed)"));
    }

    #[test]
    fn batch_repairer_is_configured_for_in_place_uncached_repair() {
        let dir = std::env::temp_dir();
        let repairer = RustParRepairer::new(dir, 1, 16, true, false, Vec::new());
        // The options themselves are private to par-rs; the timings start empty and the
        // in-place list is empty before any repair.
        assert!(repairer.repairer.in_place_repairs().is_empty());
        assert_eq!(repairer.repairer.repair_timings().total, std::time::Duration::ZERO);
    }

    #[test]
    fn test_repairer_resources() {
        assert_eq!(batch_repair_threads(0, 12, 6, true), 6);
        assert_eq!(batch_repair_threads(0, 12, 6, false), 12);
        assert_eq!(batch_repair_threads(14, 12, 6, false), 12);
        assert_eq!(batch_repair_threads(14, 12, 6, true), 6);
        assert_eq!(batch_repair_threads(4, 12, 6, true), 4);
        assert_eq!(batch_repair_threads(0, 0, 0, true), 1);
        assert_eq!(calc_repairer_resources(0, 8, false, 0, 250), (8, 250));
        assert_eq!(calc_repairer_resources(4, 8, false, 0, 0), (4, 16));
        assert_eq!(calc_repairer_resources(0, 0, true, 8, 250), (1, 16));

    }

    fn make_deterministic_bytes(len: usize, seed: u64) -> Vec<u8> {
        let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        (0..len)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s >> 37) as u8
            })
            .collect()
    }

    #[test]
    fn live_session_grows_syndrome_slots_from_free_par_buffer_as_damage_appears() {
        grow_live_slots_case(0x6a0f_0001, 0, 6, 3);
    }

    #[test]
    fn live_session_never_grows_past_the_recovery_blocks_that_can_come() {
        // Three damaged blocks but only two recovery blocks exist: ParBuffer
        // is charged for two slots, not three.
        grow_live_slots_case(0x6a0f_0003, 0, 2, 2);
    }

    #[test]
    fn live_session_grows_past_a_preallocated_slot_whose_exponent_never_comes() {
        // The preallocated slot holds exponent 0, but the volumes carry 3..8:
        // after arrival one more slot is needed and the turned-away block is
        // absorbed again.
        grow_live_slots_case(0x6a0f_0002, 3, 6, 4);
    }

    #[test]
    fn live_session_lends_free_par_buffer_to_staging_and_snapshots_up_to_half() {
        use par_rs::create::{build_set, SetOptions, VolumeScheme};

        let nzb_id = 0x6a0f_0010;
        let dir = std::env::temp_dir().join(format!(
            "rustyget-parchecker-lend-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f0 = make_deterministic_bytes(8 * 4096, 405);
        let built = build_set(
            "lend",
            &[("part01.rar", &f0)],
            &SetOptions {
                slice_size: 4096,
                recovery_blocks: 2,
                first_exponent: 0,
                volumes: VolumeScheme::Single,
                creator: "rustyget-test".to_string(),
                unicode_names: false,
                kernel: Kernel::detect(),
            },
        )
        .unwrap();
        let index_path = dir.join("lend.par2");
        std::fs::write(&index_path, &built.index).unwrap();
        // A pool of its own, so other tests' sessions cannot take the room it lends.
        let pool = Arc::new(crate::parlive::BudgetPool::with_size(500 << 20));
        streaming_session_start_in_pool(
            &pool,
            nzb_id,
            &index_path,
            &dir,
            SessionOptions {
                preallocated_syndrome_slots: 1,
                max_syndrome_slots: 1,
                max_syndrome_bytes: 4096,
                max_working_bytes: 4 << 20,
                max_staging_bytes: 4096,
                solve_mem_limit_bytes: 16 << 20,
                ..SessionOptions::default()
            },
        )
        .unwrap();
        let entry = active_streaming_session(nzb_id).unwrap();
        let start = entry.budget.lock().unwrap().reserved_bytes();
        let staging_full = RepairError::StreamingMemoryLimit {
            context: "RAM-only partial staging",
            required_bytes: 8192,
            limit_bytes: 4096,
        };
        let queue_full = RepairError::StreamingMemoryLimit {
            context: "live article snapshots",
            required_bytes: 8192,
            limit_bytes: 4096,
        };
        let queue_start = entry.article_byte_limit.load(Ordering::Relaxed);
        let _ = entry.record::<()>(Err(staging_full));
        assert_eq!(entry.session.max_staging_bytes(), 8192);
        let _ = entry.record::<()>(Err(queue_full));
        assert_eq!(entry.article_byte_limit.load(Ordering::Relaxed), queue_start + 4096);
        assert_eq!(entry.budget.lock().unwrap().reserved_bytes(), start + 8192);
        // The article-count limit (bytes fit) lends nothing.
        let _ = entry.record::<()>(Err(RepairError::StreamingMemoryLimit {
            context: "live article snapshots",
            required_bytes: 4096,
            limit_bytes: 1 << 20,
        }));
        assert_eq!(entry.budget.lock().unwrap().reserved_bytes(), start + 8192);
        // Other limits lend nothing.
        let _ = entry.record::<()>(Err(RepairError::StreamingMemoryLimit {
            context: "article metadata",
            required_bytes: 1,
            limit_bytes: 0,
        }));
        assert_eq!(entry.budget.lock().unwrap().reserved_bytes(), start + 8192);
        // A shortfall of several slices is covered in one loan.
        let _ = entry.record::<()>(Err(RepairError::StreamingMemoryLimit {
            context: "RAM-only partial staging",
            required_bytes: 8192 + 3 * 4096 - 100,
            limit_bytes: 8192,
        }));
        assert_eq!(entry.session.max_staging_bytes(), 8192 + 3 * 4096);
        // Replay payloads share, and grow, the snapshot byte limit.
        let _ = entry.record::<()>(Err(RepairError::StreamingMemoryLimit {
            context: "live replay payload",
            required_bytes: queue_start + 4096 + 2 * 4096,
            limit_bytes: queue_start + 4096,
        }));
        assert_eq!(entry.article_byte_limit.load(Ordering::Relaxed), queue_start + 3 * 4096);
        assert_eq!(entry.budget.lock().unwrap().reserved_bytes(), start + 7 * 4096);
        assert_eq!(entry.lent_memory.load(Ordering::Relaxed), 7 * 4096);
        // A replay chunk that fits the limit on its own is admitted even
        // while snapshots fill the queue; a larger one is refused.
        let limit = entry.article_byte_limit.load(Ordering::Relaxed);
        entry.articles.lock().unwrap().bytes += limit - 10;
        {
            let charge = CallerPayloadCharge::acquire(&entry, limit).unwrap();
            assert_eq!(entry.articles.lock().unwrap().bytes, 2 * limit - 10);
            drop(charge);
        }
        assert_eq!(entry.articles.lock().unwrap().bytes, limit - 10);
        assert!(matches!(
            CallerPayloadCharge::acquire(&entry, limit + 1),
            Err(RepairError::StreamingMemoryLimit { context: "live replay payload", required_bytes, .. })
                if required_bytes == limit + 1
        ));
        entry.articles.lock().unwrap().bytes -= limit - 10;
        // Pending volume unpauses are drained even with no article queued.
        let _ = streaming_session_take_warning(nzb_id);
        // A fresh status poll is not due again for a few seconds.
        assert!(streaming_session_poll(nzb_id, true).is_some());
        assert!(!streaming_session_pending_article_jobs().contains(&nzb_id));
        entry.pending_unpause_ids.lock().unwrap().push(77);
        assert!(streaming_session_pending_article_jobs().contains(&nzb_id));
        assert_eq!(streaming_session_take_unpause_ids(nzb_id), vec![77]);
        assert!(!streaming_session_pending_article_jobs().contains(&nzb_id));
        // Never more than half of the pool.
        entry
            .lent_memory
            .store(pool.size().unwrap() / 2, Ordering::Relaxed);
        let _ = entry.record::<()>(Err(RepairError::StreamingMemoryLimit {
            context: "RAM-only partial staging",
            required_bytes: 1,
            limit_bytes: 0,
        }));
        assert_eq!(entry.session.max_staging_bytes(), 8192 + 3 * 4096);
        streaming_session_cancel(nzb_id);
        drop(entry);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Many files arriving at once overrun the first staging cap: the loan of free
    /// ParBuffer that follows takes the refused article at once, so the drain neither
    /// drops it nor leaves an error to report.
    #[test]
    fn live_staging_overrun_that_a_loan_covers_is_not_a_refusal() {
        use par_rs::create::{build_set, SetOptions, VolumeScheme};

        let nzb_id = 0x6a0f_0011;
        let dir = std::env::temp_dir().join(format!(
            "rustyget-parchecker-lend-retry-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f0 = make_deterministic_bytes(8 * 4096, 406);
        let built = build_set(
            "retry",
            &[("part01.rar", &f0)],
            &SetOptions {
                slice_size: 4096,
                recovery_blocks: 2,
                first_exponent: 0,
                volumes: VolumeScheme::Single,
                creator: "rustyget-test".to_string(),
                unicode_names: false,
                kernel: Kernel::detect(),
            },
        )
        .unwrap();
        let index_path = dir.join("retry.par2");
        std::fs::write(&index_path, &built.index).unwrap();
        // A pool of its own: what other tests' sessions hold in the shared one must
        // not decide whether the loan is made.
        let pool = Arc::new(crate::parlive::BudgetPool::with_size(500 << 20));
        streaming_session_start_in_pool(
            &pool,
            nzb_id,
            &index_path,
            &dir,
            SessionOptions {
                preallocated_syndrome_slots: 1,
                max_syndrome_slots: 1,
                max_syndrome_bytes: 4096,
                max_working_bytes: 4 << 20,
                max_staging_bytes: 4096,
                solve_mem_limit_bytes: 16 << 20,
                ..SessionOptions::default()
            },
        )
        .unwrap();
        // The first 3000 bytes of three slices: each piece stays partial, 9000 bytes
        // staged against a 4096-byte cap.
        for slice in 0..3u64 {
            let offset = slice * 4096;
            let piece = f0[offset as usize..offset as usize + 3000].to_vec();
            let crc = crc32fast::hash(&piece);
            streaming_session_defer_article(nzb_id, (b"part01.rar", &[]), offset, 3000, crc, || {
                Some(piece)
            })
            .unwrap()
            .unwrap();
        }
        assert_eq!(streaming_session_drain_articles(nzb_id).unwrap().unwrap(), 3);
        let entry = active_streaming_session(nzb_id).unwrap();
        assert!(entry.session.max_staging_bytes() >= 9000);
        assert!(entry.refusals.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
        assert!(streaming_session_take_warning(nzb_id).is_none());
        streaming_session_forget(nzb_id);
        drop(entry);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The snapshot queue's first byte limit, overrun by many files arriving at once,
    /// takes the snapshot when a loan of free ParBuffer covers it: no refusal to report.
    #[test]
    fn live_snapshot_overrun_that_a_loan_covers_is_not_a_refusal() {
        use par_rs::create::{build_set, SetOptions, VolumeScheme};

        let nzb_id = 0x6a0f_0013;
        let dir = std::env::temp_dir().join(format!(
            "rustyget-parchecker-lend-queue-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f0 = make_deterministic_bytes(8 * 4096, 408);
        let built = build_set(
            "queue",
            &[("part01.rar", &f0)],
            &SetOptions {
                slice_size: 4096,
                recovery_blocks: 2,
                first_exponent: 0,
                volumes: VolumeScheme::Single,
                creator: "rustyget-test".to_string(),
                unicode_names: false,
                kernel: Kernel::detect(),
            },
        )
        .unwrap();
        let index_path = dir.join("queue.par2");
        std::fs::write(&index_path, &built.index).unwrap();
        // A pool of its own: what other tests' sessions hold in the shared one must
        // not decide whether the loan is made.
        let pool = Arc::new(crate::parlive::BudgetPool::with_size(500 << 20));
        streaming_session_start_in_pool(
            &pool,
            nzb_id,
            &index_path,
            &dir,
            SessionOptions {
                preallocated_syndrome_slots: 1,
                max_syndrome_slots: 1,
                max_syndrome_bytes: 4096,
                max_working_bytes: 4 << 20,
                max_staging_bytes: 4096,
                solve_mem_limit_bytes: 16 << 20,
                ..SessionOptions::default()
            },
        )
        .unwrap();
        let entry = active_streaming_session(nzb_id).unwrap();
        entry.article_byte_limit.store(4096, Ordering::Release);
        let results: Vec<_> = (0..2u64)
            .map(|slice| {
                let offset = slice * 4096;
                let piece = f0[offset as usize..offset as usize + 4096].to_vec();
                let crc = crc32fast::hash(&piece);
                streaming_session_defer_article(
                    nzb_id,
                    (b"part01.rar", &[]),
                    offset,
                    4096,
                    crc,
                    || Some(piece),
                )
                .unwrap()
            })
            .collect();
        assert!(results.iter().all(Result::is_ok), "{results:?}");
        assert_eq!(entry.article_byte_limit.load(Ordering::Relaxed), 2 * 4096);
        assert!(entry.refusals.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
        assert!(streaming_session_take_warning(nzb_id).is_none());
        assert_eq!(streaming_session_drain_articles(nzb_id).unwrap().unwrap(), 2);
        // An overrun no loan can cover is still refused: the pool is full.
        let full = *pool.reserved_for_test();
        let filler = pool.acquire(pool.size().unwrap() - full, u64::MAX).unwrap();
        entry.article_byte_limit.store(4096, Ordering::Release);
        let piece = f0[8192..12288].to_vec();
        let crc = crc32fast::hash(&piece);
        streaming_session_defer_article(nzb_id, (b"part01.rar", &[]), 8192, 4096, crc, || {
            Some(piece.clone())
        })
        .unwrap()
        .unwrap();
        let piece2 = f0[12288..16384].to_vec();
        let crc2 = crc32fast::hash(&piece2);
        assert!(matches!(
            streaming_session_defer_article(nzb_id, (b"part01.rar", &[]), 12288, 4096, crc2, || {
                Some(piece2)
            })
            .unwrap(),
            Err(RepairError::StreamingMemoryLimit {
                context: "live article snapshots",
                ..
            })
        ));
        drop(filler);
        streaming_session_forget(nzb_id);
        drop(entry);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn live_time_split_leaves_payload_wait_out_of_repair() {
        let ms = std::time::Duration::from_millis;
        // 10 ms in all, 6 ms in repair calls of which 1 ms waiting for admission.
        assert_eq!(split_live_time(ms(10), 6_000_000, 1_000_000), (ms(4), ms(5)));
        // Library timers can overshoot the caller's: never more than the wall time.
        assert_eq!(split_live_time(ms(10), 12_000_000, 20_000_000), (ms(0), ms(0)));
        assert_eq!(split_live_time(ms(3), 0, 0), (ms(3), ms(0)));
    }

    /// Checkpoints take whole seconds and keep the fraction: however often the queue
    /// is saved, the saved and the settled time add up to what was measured.
    #[test]
    fn live_par_time_checkpoints_take_whole_seconds_once() {
        let nzb_id = 0x6a0f_0021;
        let _ = streaming_session_take_par_secs(nzb_id);
        live_par_time_add_for_test(nzb_id, 1_700, 600); // 2.3 s, 0.6 s repair
        assert!(live_par_time_pending(nzb_id));
        assert_eq!(streaming_session_take_whole_par_secs(nzb_id), (2, 0));
        assert_eq!(streaming_session_take_whole_par_secs(nzb_id), (0, 0));
        assert!(!live_par_time_pending(nzb_id));
        // 0.3 s kept; 2.3 s more of which 0.9 s repair: 2.6 s, 1.2 s repair.
        live_par_time_add_for_test(nzb_id, 1_400, 900);
        assert_eq!(streaming_session_take_whole_par_secs(nzb_id), (2, 1));
        // The rest settles at history, rounded: 5 s par for 4.6 s, 2 s repair for 1.5 s.
        assert_eq!(streaming_session_take_par_secs(nzb_id), (1, 1));
        assert_eq!(streaming_session_take_whole_par_secs(nzb_id), (0, 0));
        // A repair fraction larger than the par fraction is kept, not folded into verify:
        // 3.7 s repair + 0.5 s verify checkpoints 4 s / 3 s and settles 0 s / 1 s.
        live_par_time_add_for_test(nzb_id, 500, 3_700);
        assert_eq!(streaming_session_take_whole_par_secs(nzb_id), (4, 3));
        assert_eq!(streaming_session_take_par_secs(nzb_id), (0, 1));
    }

    /// Live PAR work is charged to the job's par time, once, nested calls included once,
    /// and survives the session until the job reaches history and takes it.
    #[test]
    fn live_par_time_is_charged_once_and_taken_once() {
        use par_rs::create::{build_set, SetOptions, VolumeScheme};

        let nzb_id = 0x6a0f_0012;
        let dir = std::env::temp_dir().join(format!(
            "rustyget-parchecker-time-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f0 = make_deterministic_bytes(8 * 4096, 407);
        let built = build_set(
            "time",
            &[("part01.rar", &f0)],
            &SetOptions {
                slice_size: 4096,
                recovery_blocks: 2,
                first_exponent: 0,
                volumes: VolumeScheme::Single,
                creator: "rustyget-test".to_string(),
                unicode_names: false,
                kernel: Kernel::detect(),
            },
        )
        .unwrap();
        let index_path = dir.join("time.par2");
        std::fs::write(&index_path, &built.index).unwrap();
        let pool = Arc::new(crate::parlive::BudgetPool::with_size(500 << 20));
        streaming_session_start_in_pool(
            &pool,
            nzb_id,
            &index_path,
            &dir,
            SessionOptions {
                preallocated_syndrome_slots: 1,
                max_syndrome_slots: 1,
                max_syndrome_bytes: 4096,
                max_working_bytes: 4 << 20,
                max_staging_bytes: 4096,
                solve_mem_limit_bytes: 16 << 20,
                ..SessionOptions::default()
            },
        )
        .unwrap();
        assert_eq!(streaming_session_take_par_secs(nzb_id), (0, 0));
        let entry = active_streaming_session(nzb_id).unwrap();
        let times = || LIVE_PAR_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let value = entry.timed(nzb_id, || {
            let inner = entry.timed(nzb_id, || {
                std::thread::sleep(std::time::Duration::from_millis(100));
                7
            });
            // The nested call charged nothing of its own: the outer one charges it.
            assert!(!times().contains_key(&nzb_id));
            inner
        });
        assert_eq!(value, 7);
        let time = times()[&nzb_id];
        assert!(time.verify >= std::time::Duration::from_millis(100));
        assert_eq!(time.repair, std::time::Duration::ZERO);
        // Ordinary article work goes through the same accounting.
        let piece = f0[..4096].to_vec();
        let crc = crc32fast::hash(&piece);
        streaming_session_on_article_finished_by_name(
            nzb_id,
            b"part01.rar",
            0,
            4096,
            crc,
            Some(&piece),
        )
        .unwrap()
        .unwrap();
        assert!(times()[&nzb_id].verify > time.verify);
        streaming_session_forget(nzb_id);
        drop(entry);
        // Kept past the session; par time includes the repair; whole seconds.
        times().insert(
            nzb_id,
            LiveParTime {
                verify: std::time::Duration::from_millis(1400),
                repair: std::time::Duration::from_millis(600),
                ..Default::default()
            },
        );
        assert_eq!(streaming_session_take_par_secs(nzb_id), (2, 1));
        assert_eq!(streaming_session_take_par_secs(nzb_id), (0, 0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn live_session_grows_into_blocks_turned_away_before_new_damage() {
        use par_rs::create::{build_set, SetOptions, VolumeScheme};

        let nzb_id = 0x6a0f_0004;
        let dir = std::env::temp_dir().join(format!(
            "rustyget-parchecker-grow-late-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f0 = make_deterministic_bytes(8 * 4096, 404);
        let built = build_set(
            "late",
            &[("part01.rar", &f0)],
            &SetOptions {
                slice_size: 4096,
                recovery_blocks: 6,
                first_exponent: 0,
                volumes: VolumeScheme::Single,
                creator: "rustyget-test".to_string(),
                unicode_names: false,
                kernel: Kernel::detect(),
            },
        )
        .unwrap();
        let index_path = dir.join("late.par2");
        std::fs::write(&index_path, &built.index).unwrap();
        streaming_session_start(
            nzb_id,
            &index_path,
            &dir,
            SessionOptions {
                preallocated_syndrome_slots: 1,
                max_syndrome_slots: 1,
                max_syndrome_bytes: 4096,
                max_working_bytes: 4 << 20,
                max_staging_bytes: 0,
                solve_mem_limit_bytes: 16 << 20,
                ..SessionOptions::default()
            },
        )
        .unwrap();
        let entry = active_streaming_session(nzb_id).unwrap();
        let (vname, vbytes) = &built.volumes[0];
        assert!(streaming_session_register_queued_volume(nzb_id, 7, 6, true));
        let id0 = entry.session.file_id_by_name(b"part01.rar").unwrap();
        let mut disk = f0.clone();
        for s in [1usize, 4, 6] {
            disk[s * 4096..(s + 1) * 4096].fill(0);
        }
        std::fs::write(dir.join("part01.rar"), &disk).unwrap();
        let feed = |s: usize| {
            let off = (s * 4096) as u64;
            if matches!(s, 1 | 4 | 6) {
                streaming_session_on_article_failed_permanently(nzb_id, &id0, off, 4096)
                    .unwrap()
                    .unwrap();
            } else {
                let chunk = &f0[s * 4096..(s + 1) * 4096];
                entry
                    .session
                    .on_article_finished(&id0, off, 4096, par_rs::crc::crc32(chunk), Some(chunk))
                    .unwrap();
            }
        };
        for s in 0..6 {
            feed(s);
        }
        assert_eq!(entry.session.max_syndrome_slots(), 2);
        // The whole volume arrives while only two blocks are damaged.
        let path = dir.join(vname);
        std::fs::write(&path, vbytes).unwrap();
        streaming_session_on_recovery_volume_arrived(nzb_id, &path, Some(7))
            .unwrap()
            .unwrap();
        assert_eq!(entry.session.status().ready_recovery, 2);
        assert_eq!(entry.session.turned_away_recovery_slices(), 4);
        // A third damaged block grows into a block the volume turned away.
        feed(6);
        feed(7);
        assert_eq!(entry.session.max_syndrome_slots(), 3);
        assert_eq!(entry.session.status().ready_recovery, 3);
        let report = streaming_session_repair_available_targets_now(nzb_id, &[id0])
            .unwrap()
            .unwrap();
        assert_eq!(report.repaired_slices.len(), 3);
        assert_eq!(std::fs::read(dir.join("part01.rar")).unwrap(), f0);
        // The repair is charged as repair time, the feeding as verification.
        let time = LIVE_PAR_TIME.lock().unwrap_or_else(|e| e.into_inner())[&nzb_id];
        assert!(time.repair > std::time::Duration::ZERO);
        assert!(time.verify > std::time::Duration::ZERO);
        let stats = entry.session.repair_phase_stats();
        assert_eq!(
            time.repair.as_nanos() as u64,
            stats.total_ns - stats.payload_wait_ns
        );
        let _ = streaming_session_take_par_secs(nzb_id);
        streaming_session_cancel(nzb_id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn grow_live_slots_case(
        nzb_id: i32,
        first_exponent: u32,
        recovery_blocks: u32,
        final_cap: usize,
    ) {
        use par_rs::create::{build_set, SetOptions, VolumeScheme};

        let dir = std::env::temp_dir().join(format!(
            "rustyget-parchecker-grow-{}-{nzb_id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f0 = make_deterministic_bytes(8 * 4096, 303);
        let built = build_set(
            "grow",
            &[("part01.rar", &f0)],
            &SetOptions {
                slice_size: 4096,
                recovery_blocks,
                first_exponent,
                volumes: VolumeScheme::Single,
                creator: "rustyget-test".to_string(),
                unicode_names: false,
                kernel: Kernel::detect(),
            },
        )
        .unwrap();
        let index_path = dir.join("grow.par2");
        std::fs::write(&index_path, &built.index).unwrap();
        // Start with room for one recovery block only.
        streaming_session_start(
            nzb_id,
            &index_path,
            &dir,
            SessionOptions {
                preallocated_syndrome_slots: 1,
                max_syndrome_slots: 1,
                max_syndrome_bytes: 4096,
                max_working_bytes: 4 << 20,
                max_staging_bytes: 0,
                solve_mem_limit_bytes: 16 << 20,
                defer_backfill: true,
                ..SessionOptions::default()
            },
        )
        .unwrap();
        let entry = active_streaming_session(nzb_id).unwrap();
        let reserved =
            |entry: &ActiveStreamingSession| entry.budget.lock().unwrap().reserved_bytes();
        let start_bytes = reserved(&entry);
        assert_eq!(entry.session.max_syndrome_slots(), 1);
        for (idx, (vname, _)) in built.volumes.iter().enumerate() {
            let blocks = volume_blocks_bytes(vname.as_bytes()).unwrap_or(1) as i32;
            assert!(streaming_session_register_queued_volume(
                nzb_id,
                2000 + idx as u64,
                blocks,
                true
            ));
        }
        let id0 = entry.session.file_id_by_name(b"part01.rar").unwrap();
        let mut disk = f0.clone();
        for s in [1usize, 4, 6] {
            disk[s * 4096..(s + 1) * 4096].fill(0);
        }
        std::fs::write(dir.join("part01.rar"), &disk).unwrap();
        for s in 0..8usize {
            let off = (s * 4096) as u64;
            if matches!(s, 1 | 4 | 6) {
                streaming_session_on_article_failed_permanently(nzb_id, &id0, off, 4096)
                    .unwrap()
                    .unwrap();
                continue;
            }
            let chunk = &f0[s * 4096..(s + 1) * 4096];
            entry
                .session
                .on_article_finished(&id0, off, 4096, par_rs::crc::crc32(chunk), Some(chunk))
                .unwrap();
        }
        // Three damaged blocks: the cap grew to one slot per block that can
        // come, two slices each.
        assert_eq!(entry.session.status().damaged, 3);
        let grown = 3.min(recovery_blocks as usize);
        assert_eq!(entry.session.max_syndrome_slots(), grown);
        assert_eq!(reserved(&entry), start_bytes + (grown as u64 - 1) * 2 * 4096);
        for (idx, (vname, bytes)) in built.volumes.iter().enumerate() {
            let path = dir.join(vname);
            std::fs::write(&path, bytes).unwrap();
            streaming_session_on_recovery_volume_arrived(nzb_id, &path, Some(2000 + idx as u64))
                .unwrap()
                .unwrap();
        }
        assert_eq!(entry.session.max_syndrome_slots(), final_cap);
        // Slots that arrived after slices were verified are back-filled on a
        // thread of their own, not inside the arrival.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while entry.session.needs_backfill() || entry.backfill_running.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline, "back-fill never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            reserved(&entry),
            start_bytes + (final_cap as u64 - 1) * 2 * 4096
        );
        if recovery_blocks < 3 {
            assert_eq!(entry.session.status().ready_recovery, recovery_blocks as usize);
            streaming_session_cancel(nzb_id);
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        assert_eq!(entry.session.status().ready_recovery, 3);
        let report = streaming_session_repair_available_targets_now(nzb_id, &[id0])
            .unwrap()
            .unwrap();
        assert_eq!(report.repaired_slices.len(), 3);
        assert_eq!(std::fs::read(dir.join("part01.rar")).unwrap(), f0);
        streaming_session_cancel(nzb_id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_streaming_par_session_end_to_end() {
        use par_rs::create::{build_set, SetOptions, VolumeScheme};

        let dir = std::env::temp_dir().join(format!(
            "rustyget-parchecker-stream-{}-{}",
            std::process::id(),
            1
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let f0 = make_deterministic_bytes(16_384, 101);
        let f1 = make_deterministic_bytes(9_500, 202);
        let built = build_set(
            "release",
            &[("part01.rar", &f0), ("part02.rar", &f1)],
            &SetOptions {
                slice_size: 4096,
                recovery_blocks: 6,
                first_exponent: 0,
                volumes: VolumeScheme::Exponential,
                creator: "rustyget-test".to_string(),
                unicode_names: false,
                kernel: Kernel::detect(),
            },
        )
        .unwrap();

        // 1. Write the .par2 index and load it via `load_par2_set`, then start StreamingParSession.
        let index_path = dir.join("release.par2");
        std::fs::write(&index_path, &built.index).unwrap();
        let set = load_par2_set(&index_path).unwrap();
        let session = StreamingParSession::new(&set, &dir, SessionOptions::default()).unwrap();

        // Register every queued .vol*.par2 with `register_queued_volume`.
        for (idx, (vname, _)) in built.volumes.iter().enumerate() {
            let blocks = volume_blocks_bytes(vname.as_bytes()).unwrap_or(1) as i32;
            session.register_queued_volume(QueuedVolume {
                id: 1000 + idx as u64,
                blocks,
                paused: true,
            });
        }

        // 2. Resolve file IDs via `file_id_by_name` and `file_id_by_hash16k`.
        let id0 = session.file_id_by_name(b"part01.rar").unwrap();
        let id1 = session.file_id_by_name(b"part02.rar").unwrap();
        let fd0_hash16k = set.file_descs[&id0].hash_16k;
        assert_eq!(session.file_id_by_hash16k(&fd0_hash16k), Some(id0));

        // Simulate downloader writing part01.rar (with slice 1 missing) and part02.rar (with slice 2 corrupted)
        let mut f0_disk = f0.clone();
        f0_disk[4096..8192].fill(0);
        std::fs::write(dir.join("part01.rar"), &f0_disk).unwrap();

        let mut f1_disk = f1.clone();
        f1_disk[8192] ^= 0xFF;
        std::fs::write(dir.join("part02.rar"), &f1_disk).unwrap();

        // Feed completed articles out of order for part01.rar (slices 0, 2, 3) using `on_article_finished`
        for &s in &[3usize, 0, 2] {
            let off = (s * 4096) as u64;
            let chunk = &f0[s * 4096..(s + 1) * 4096];
            let crc = par_rs::crc::crc32(chunk);
            let outcome = session
                .on_article_finished(&id0, off, 4096, crc, Some(chunk))
                .unwrap();
            assert_eq!(outcome.events.len(), 1);
            assert_eq!(outcome.events[0].slice_idx, s);
            assert_eq!(outcome.events[0].state, SliceState::VerifiedGood);
        }

        // Feed completed articles for part02.rar (slices 0, 1 intact; slice 2 corrupted) using `on_article_finished_by_name`
        for s in 0..2usize {
            let off = (s * 4096) as u64;
            let chunk = &f1[s * 4096..(s + 1) * 4096];
            let crc = par_rs::crc::crc32(chunk);
            let outcome = session
                .on_article_finished_by_name(b"part02.rar", off, 4096, crc, Some(chunk))
                .unwrap();
            assert_eq!(outcome.events.len(), 1);
            assert_eq!(outcome.events[0].slice_idx, s);
            assert_eq!(outcome.events[0].state, SliceState::VerifiedGood);
        }
        // Corrupted tail slice of part02.rar
        let tail_chunk = &f1_disk[8192..];
        let bad_crc = par_rs::crc::crc32(tail_chunk);
        let bad_outcome = session
            .on_article_finished_by_name(
                b"part02.rar",
                8192,
                tail_chunk.len() as u64,
                bad_crc,
                Some(tail_chunk),
            )
            .unwrap();
        assert!(bad_outcome
            .events
            .iter()
            .any(|e| e.slice_idx == 2 && e.state == SliceState::Damaged));

        // 3. Permanently fail slice 1 of part01.rar and check `UnpauseAction`
        let (ev_fail, unpause_action) = session
            .on_article_failed_permanently(&id0, 4096, 4096)
            .unwrap();
        assert_eq!(ev_fail.len(), 1);
        assert_eq!(ev_fail[0].slice_idx, 1);
        assert_eq!(ev_fail[0].state, SliceState::Damaged);

        let mut to_unpause = Vec::new();
        if let Some(act) = bad_outcome.unpause {
            to_unpause.extend(act.volume_ids_to_unpause);
        }
        if let Some(act) = unpause_action {
            to_unpause.extend(act.volume_ids_to_unpause);
        }
        assert!(!to_unpause.is_empty());

        // 4. Deliver the unpaused recovery volumes via `on_recovery_volume_arrived`
        for vid in to_unpause {
            let v_idx = (vid - 1000) as usize;
            let (vname, vbytes) = &built.volumes[v_idx];
            let vol_path = dir.join(vname);
            std::fs::write(&vol_path, vbytes).unwrap();
            let added = session
                .on_recovery_volume_arrived(&vol_path, Some(vid))
                .unwrap();
            assert!(added > 0);
        }

        // 5. Poll `status()` -> `LiveStatus`
        let st = session.status();
        assert_eq!(st.damaged, 2);
        assert_eq!(st.unarrived, 0);
        assert!(st.early_cutover_possible);

        // Test `try_splice_live_slice` for DirectUnpack holes (splicing slice 1 of part01.rar)
        let spliced_indices = session
            .try_splice_live_slice(
                &id0,
                ByteRange::new(4096, 4096),
                &f0[4096..8192],
                &dir.join("part01.rar"),
            )
            .unwrap();
        assert_eq!(spliced_indices, vec![1]);
        assert_eq!(std::fs::read(dir.join("part01.rar")).unwrap(), f0);
        assert_eq!(
            session.volume_status(&id0).unwrap(),
            VolumeStatus::CompleteAndVerified
        );

        // Repair remaining incomplete targets via `repair_available_targets_now`
        let report = session.repair_available_targets_now(&[id1]).unwrap();
        assert_eq!(report.erasures, 1);
        assert_eq!(std::fs::read(dir.join("part01.rar")).unwrap(), f0);
        assert_eq!(std::fs::read(dir.join("part02.rar")).unwrap(), f1);
        assert_eq!(
            session.volume_status(&id1).unwrap(),
            VolumeStatus::CompleteAndVerified
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_pure_memory_streaming_assembler_and_syndrome_bank() {
        use par_rs::create::{build_set, SetOptions, VolumeScheme};

        let f0 = make_deterministic_bytes(12_288, 303);
        let f1 = make_deterministic_bytes(6_000, 404);
        let built = build_set(
            "memset",
            &[("mem0.bin", &f0), ("mem1.bin", &f1)],
            &SetOptions {
                slice_size: 4096,
                recovery_blocks: 3,
                first_exponent: 0,
                volumes: VolumeScheme::Single,
                creator: "rustyget-mem-test".to_string(),
                unicode_names: false,
                kernel: Kernel::detect(),
            },
        )
        .unwrap();

        let set = load_par2_set_bytes(&built.index).unwrap();
        let vol_bytes = &built.volumes[0].1;
        let vol_set = load_par2_set_bytes(vol_bytes).unwrap();
        let exponents: Vec<u32> = vol_set.recovery.keys().copied().collect();
        let mut mem_session =
            MemoryStreamingSession::from_packet_set(&set, &exponents, 1 << 20).unwrap();
        let id0 = set
            .file_descs
            .iter()
            .find(|(_, fd)| fd.name == b"mem0.bin")
            .map(|(id, _)| id.0)
            .unwrap();
        let id1 = set
            .file_descs
            .iter()
            .find(|(_, fd)| fd.name == b"mem1.bin")
            .map(|(id, _)| id.0)
            .unwrap();

        // Feed mem0.bin in irregular article chunks (skipping slice 1 = bytes 4096..8192)
        mem_session.push_chunk(&id0, 0, &f0[0..2500]).unwrap();
        mem_session.push_chunk(&id0, 2500, &f0[2500..4096]).unwrap();
        mem_session
            .push_chunk(&id0, 8192, &f0[8192..12_288])
            .unwrap();
        let damaged0 = mem_session.mark_range_damaged(&id0, 4500, 500);
        assert!(!damaged0.is_empty());

        // Feed mem1.bin slice 0 intact, corrupt slice 1
        mem_session.push_chunk(&id1, 0, &f1[0..4096]).unwrap();
        let mut bad_tail = f1[4096..].to_vec();
        bad_tail[0] ^= 0xAA;
        let evs = mem_session.push_chunk(&id1, 4096, &bad_tail).unwrap();
        assert!(evs
            .iter()
            .any(|e| matches!(e, StreamEvent::SliceMismatch { .. })));

        // Push recovery slices from the volume bytes into the syndrome bank
        for (&exp, rref) in &vol_set.recovery {
            let start = rref.data_offset as usize;
            let end = start + rref.slice_size as usize;
            mem_session
                .push_recovery(exp, &vol_bytes[start..end])
                .unwrap();
        }

        assert!(mem_session.can_solve_now());
        assert!(matches!(
            mem_session.status(),
            StreamStatus::ReadyToSolve {
                unknown: 2,
                ready: 3
            }
        ));

        // Find the global indices of the 2 damaged slices and verify `solve_selected_slices` + `solve_now`
        let all_recovered = mem_session.solve_now().unwrap();
        assert_eq!(all_recovered.len(), 2);
        let g_first = all_recovered[0].0;
        let single = mem_session.solve_selected_slices(&[g_first]).unwrap();
        assert_eq!(single.len(), 1);
        assert_eq!(single[0], all_recovered[0]);
    }

    #[test]
    fn test_post_download_par2_repairer_and_par_renamer_fallback() {
        use par_rs::create::{build_set, SetOptions, VolumeScheme};

        let dir = std::env::temp_dir().join(format!(
            "rustyget-parchecker-fallback-{}-{}",
            std::process::id(),
            2
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let data_a = make_deterministic_bytes(20_000, 505);
        let data_b = make_deterministic_bytes(8_192, 606);
        let data_nfo = b"release info".to_vec();
        let built = build_set(
            "fallback",
            &[
                ("archive.r00", &data_a),
                ("archive.r01", &data_b),
                ("readme.nfo", &data_nfo),
            ],
            &SetOptions {
                slice_size: 2048,
                recovery_blocks: 6,
                first_exponent: 0,
                volumes: VolumeScheme::Exponential,
                creator: "rustyget-fallback-test".to_string(),
                unicode_names: false,
                kernel: Kernel::detect(),
            },
        )
        .unwrap();

        let index_path = dir.join("fallback.par2");
        std::fs::write(&index_path, &built.index).unwrap();
        for (vname, vbytes) in &built.volumes {
            std::fs::write(dir.join(vname), vbytes).unwrap();
        }

        // Write `archive.r01` under an obfuscated name and verify `direct_par_name` + `ParRenamer`
        let direct_name = direct_par_name(&built.set_id.to_string(), b"fallback.par2", &|_| false);
        assert!(direct_name.to_ascii_lowercase().ends_with(".par2"));
        let direct_vol = direct_par_name(
            &built.set_id.to_string(),
            b"fallback.vol01+02.par2",
            &|_| false,
        );
        assert!(direct_vol.ends_with(".vol01+02.par2"));
        std::fs::write(dir.join("0bfu5c4t3d_file"), &data_b).unwrap();

        let renamer = ParRenamer::new(&dir, RenamerOptions::default());
        let rename_report = renamer.run(None, &mut |_| {}).unwrap();
        assert_eq!(rename_report.renamed_count(), 1);
        assert!(dir.join("archive.r01").is_file());

        // Write `archive.r00` with 2 corrupted blocks, and leave `readme.nfo` missing (ignored via ParIgnoreExt)
        let mut corrupt_a = data_a.clone();
        corrupt_a[100] ^= 0xFF;
        corrupt_a[5000] ^= 0x55;
        std::fs::write(dir.join("archive.r00"), &corrupt_a).unwrap();

        let opts = RepairOptions {
            full_file_md5: false,
            sync_repaired: false,
            ignore_extensions: vec![".nfo".to_string()],
            backup: BackupPolicy::RenameNumbered,
            ..RepairOptions::default()
        };
        let mut repairer = Par2Repairer::new(&dir, opts);
        repairer.load_par2_set(&index_path).unwrap();
        repairer.finalize().unwrap();

        // Quick-verify `archive.r01` by whole-file CRC
        let crc_b = par_rs::crc::crc32(&data_b);
        let mut qv = |name: &str| -> Option<QuickInfo> {
            if name == "archive.r01" {
                Some(QuickInfo::Success { crc: crc_b })
            } else {
                None
            }
        };
        let vres = repairer.verify(Some(&mut qv), &mut NullObserver).unwrap();
        assert_eq!(vres.quick_verified_files, 1);
        assert_eq!(vres.ignored_damaged, vec!["readme.nfo".to_string()]);
        assert!(vres.repair_needed);
        assert!(vres.repair_possible);

        // Recreate readme.nfo so repair covers all files cleanly, then run repair and verify `.1` backup in `leftovers`
        std::fs::write(dir.join("readme.nfo"), &data_nfo).unwrap();
        repairer.verify(None, &mut NullObserver).unwrap();
        let rep = repairer.repair(&mut NullObserver).unwrap();
        match rep {
            RepairResult::Repaired {
                files,
                blocks_recovered,
                leftovers,
            } => {
                assert!(files.contains(&"archive.r00".to_string()));
                assert_eq!(blocks_recovered, 2);
                assert_eq!(leftovers, vec![dir.join("archive.r00.1")]);
                for b in leftovers {
                    std::fs::remove_file(b).unwrap();
                }
            }
            RepairResult::NotNeeded => panic!("expected repair"),
        }
        assert_eq!(std::fs::read(dir.join("archive.r00")).unwrap(), data_a);
        assert_eq!(std::fs::read(dir.join("archive.r01")).unwrap(), data_b);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_escalation_more_pars_split_extra_and_twin() {
        use par_rs::create::{build_set, SetOptions, VolumeScheme};

        let dir = std::env::temp_dir().join(format!(
            "rustyget-parchecker-escalation-{}-{}",
            std::process::id(),
            3
        ));
        let dupe_dir = dir.join("dupe");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dupe_dir).unwrap();

        let data_a = make_deterministic_bytes(16_384, 707);
        let data_b = make_deterministic_bytes(12_288, 808);
        let built = build_set(
            "esc",
            &[("movie.mkv", &data_a), ("sample.mkv", &data_b)],
            &SetOptions {
                slice_size: 4096,
                recovery_blocks: 7,
                first_exponent: 0,
                volumes: VolumeScheme::Exponential, // 1, 2, 4
                creator: "rustyget-escalation-test".to_string(),
                unicode_names: false,
                kernel: Kernel::detect(),
            },
        )
        .unwrap();

        // Build a twin set over the same content with different filenames and block size
        let twin_built = build_set(
            "twin",
            &[("twin_movie.mkv", &data_a), ("twin_sample.mkv", &data_b)],
            &SetOptions {
                slice_size: 2048,
                recovery_blocks: 4,
                first_exponent: 0,
                volumes: VolumeScheme::Exponential,
                creator: "rustyget-twin-test".to_string(),
                unicode_names: false,
                kernel: Kernel::detect(),
            },
        )
        .unwrap();

        // Verify twin::compare_twin_sets and twin_name_mapping
        let (set_own, _) = twin_par2_set(&built.index);
        let (set_twin, _) = twin_par2_set(&twin_built.index);
        assert_eq!(
            compare_twin_sets(&set_own, &twin_built.index),
            TwinRelation::NamesFilesOtherwise
        );
        let mapping = twin_name_mapping(&set_own, &set_twin);
        assert_eq!(mapping.len(), 2);

        // Write only index initially; keep volumes off-dir so `load_par2_set` doesn't auto-load them
        let index_path = dir.join("esc.par2");
        std::fs::write(&index_path, &built.index).unwrap();

        // Write `movie.mkv` split into `movie.mkv.001` (first 8192 bytes) and `movie.mkv.002` (last 8192 bytes, with 2 damaged slices)
        std::fs::write(dir.join("movie.mkv.001"), &data_a[0..8192]).unwrap();
        let mut split2 = data_a[8192..16_384].to_vec();
        split2[10] ^= 0xFF;
        split2[4200] ^= 0xFF;
        std::fs::write(dir.join("movie.mkv.002"), &split2).unwrap();

        // Put `sample.mkv` in `dupe_dir` (missing from main `dir`)
        std::fs::write(dupe_dir.join("dupe_sample.mkv"), &data_b).unwrap();

        let empty_cbs = NoopParEvents;

        let mut r = RustParRepairer::new(dir.clone(), 2, 64, false, false, Vec::new());
        assert_eq!(
            r.preprocess(&index_path, Some(&empty_cbs)),
            ParResult::Success
        );
        assert_eq!(r.creator(), "rustyget-escalation-test");
        assert_eq!(r.block_size(), 4096);

        // Initial verify: both files are missing on target path (7 blocks missing, 0 recovery)
        assert_eq!(r.process(false, &empty_cbs), ParResult::RepairNotPossible);
        assert_eq!(r.missing_file_count(), 2);
        assert_eq!(r.missing_block_count(), 7);

        // Step 1: Add split fragments (`movie.mkv.001` and `movie.mkv.002`) -> recovers 2 of 4 blocks of `movie.mkv`
        assert!(r.verify_extra_file(&dir.join("movie.mkv.001"), true, &empty_cbs));
        let _ = r.verify_extra_file(&dir.join("movie.mkv.002"), true, &empty_cbs);
        assert_eq!(r.missing_block_count(), 5);

        // Step 2: Scan `dupe_dir` via `add_extra_files_from_dir` -> recovers all 3 blocks of `sample.mkv`
        let dupe_gained = r
            .repairer
            .add_extra_files_from_dir(&dupe_dir, &mut NullObserver)
            .unwrap();
        assert_eq!(dupe_gained, 3);
        assert_eq!(r.missing_block_count(), 2);
        assert_eq!(r.repairer.blocks_needed(), 2);

        // Step 3: Select volume(s) for 2 needed blocks with `filenames::select_more_pars`
        let vol_counts: Vec<i32> = built
            .volumes
            .iter()
            .map(|(vname, _)| volume_blocks_bytes(vname.as_bytes()).unwrap() as i32)
            .collect();
        let (chosen, block_found) =
            select_more_pars(&vol_counts, r.repairer.blocks_needed() as i32);
        assert_eq!(block_found, 7);
        assert_eq!(chosen, vec![1]);
        for idx in chosen {
            let (vname, vbytes) = &built.volumes[idx];
            let vpath = dir.join(vname);
            std::fs::write(&vpath, vbytes).unwrap();
            assert!(r.load_extra_par2_file(&vpath, Some(&empty_cbs)));
        }

        // Now repair is possible and succeeds!
        assert_eq!(r.process(false, &empty_cbs), ParResult::RepairPossible);
        assert_eq!(r.process(true, &empty_cbs), ParResult::Success);
        assert_eq!(std::fs::read(dir.join("movie.mkv")).unwrap(), data_a);
        assert_eq!(std::fs::read(dir.join("sample.mkv")).unwrap(), data_b);
        assert!(r.leftovers().contains(&dir.join("movie.mkv.001")));

        // Verify target_file_exists_by_basename, for_each_target_verification, and delete_leftovers
        assert!(r.target_file_exists_by_basename(b"movie.mkv"));
        let mut verified = Vec::new();
        r.for_each_target_verification(true, &mut |path, keep| {
            assert!(keep);
            verified.push(path.to_vec());
        });
        assert_eq!(verified.len(), 2);

        let mut deleted_logs = Vec::new();
        r.delete_leftovers(&mut |msg| deleted_logs.push(msg.to_vec()));
        assert!(!deleted_logs.is_empty());
        assert!(!dir.join("movie.mkv.001").exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parchecker_extra_files_fragments_and_empty_check() {
        use par_rs::create::{build_set, SetOptions, VolumeScheme};

        let dir = std::env::temp_dir().join(format!(
            "rustyget-parchecker-helpers-{}-{}",
            std::process::id(),
            4
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert!(is_processed_file(
            b"Archive.R00",
            &[b"/tmp/sub/archive.r00", b"/tmp/sub/other.r01"]
        ));
        assert!(!is_processed_file(
            b"missing.r02",
            &[b"/tmp/sub/archive.r00"]
        ));

        let data_a = make_deterministic_bytes(8192, 909);
        let built = build_set(
            "hlp",
            &[("archive.bin", &data_a)],
            &SetOptions {
                slice_size: 4096,
                recovery_blocks: 2,
                first_exponent: 0,
                volumes: VolumeScheme::Single,
                creator: "rustyget-helper-test".to_string(),
                unicode_names: false,
                kernel: Kernel::detect(),
            },
        )
        .unwrap();

        let index_path = dir.join("hlp.par2");
        std::fs::write(&index_path, &built.index).unwrap();
        std::fs::write(dir.join("archive.bin.001"), &data_a[0..4096]).unwrap();
        std::fs::write(dir.join("hlp.nfo"), b"info").unwrap();
        std::fs::write(dir.join("zzz.txt"), b"other").unwrap();

        let empty_cbs = NoopParEvents;

        let mut r = RustParRepairer::new(dir.clone(), 1, 16, false, false, Vec::new());
        assert_eq!(
            r.preprocess(&index_path, Some(&empty_cbs)),
            ParResult::Success
        );
        let _ = r.process(false, &empty_cbs);
        assert!(!r.target_file_exists_by_basename(b"archive.bin"));

        let mut empty_logs = Vec::new();
        let has_damaged = r.check_empty_files(&[], &mut |msg| {
            empty_logs.push(String::from_utf8_lossy(msg).into_owned())
        });
        assert!(has_damaged);
        assert_eq!(empty_logs.len(), 1);
        assert!(empty_logs[0].contains("File archive.bin has 2 bad block(s) of total 2 block(s)"));

        let dir_bytes = crate::fsops::path_to_bytes(&dir).to_vec();
        let frags = r.discover_splitted_fragments(
            &dir_bytes,
            &[],
            &mut |name| name.ends_with(b".par2"),
            &mut |_| None,
            &mut |_| {},
        );
        assert_eq!(frags.len(), 1);
        assert!(frags[0].ends_with(b"archive.bin.001"));

        let extras = find_and_sort_extra_files(
            &dir_bytes,
            false,
            b"hlp.par2",
            &[b"archive.bin.001"],
            &mut |name| name.ends_with(b".par2"),
        );
        assert_eq!(extras.len(), 2);
        assert!(extras[0].ends_with(b"hlp.nfo"));
        assert!(extras[1].ends_with(b"zzz.txt"));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
