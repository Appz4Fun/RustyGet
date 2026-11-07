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

use crate::postrepair::{strncasecmp_prefix, NzbgetRsParSegment};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

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
    load_par2_bytes, load_par2_file, FileDescPacket, FileId, IfscPacket, MainPacket, PacketSet,
    RecoverySliceRef, ScanOptions, SetId, SliceChecksum,
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
    FileLayout, SliceState as StreamSliceState, StreamEvent, StreamStatus,
    StreamingSliceAssembler, SyndromeBank,
};
pub use par_rs::streaming::{
    ArticleOutcome, EarlyRepairReport, LiveStatus, QueuedVolume, SessionOptions, SliceEvent,
    SliceState, StreamingParSession, UnpauseAction, VolumeStatus,
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

pub const PAR_RESULT_SUCCESS: i32 = 0;
pub const PAR_RESULT_REPAIR_POSSIBLE: i32 = 1;
pub const PAR_RESULT_REPAIR_NOT_POSSIBLE: i32 = 2;
pub const PAR_RESULT_INVALID_ARGS: i32 = 3;
pub const PAR_RESULT_INSUFFICIENT_CRITICAL_DATA: i32 = 4;
pub const PAR_RESULT_REPAIR_FAILED: i32 = 5;
pub const PAR_RESULT_FILE_IO_ERROR: i32 = 6;
pub const PAR_RESULT_LOGIC_ERROR: i32 = 7;
pub const PAR_RESULT_MEMORY_ERROR: i32 = 8;

/// Loads a `.par2` index file and any sibling volumes of the same collection in its directory
/// into a [`PacketSet`].
pub fn load_par2_set(main_path: &Path) -> Result<PacketSet, RepairError> {
    let base_dir = main_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut repairer = Par2Repairer::new(base_dir, RepairOptions::default());
    repairer.load_par2_set(main_path)?;
    Ok(repairer.packets().clone())
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
    pending_unpause_ids: Vec<u64>,
}

static STREAMING_SESSIONS: LazyLock<Mutex<HashMap<i32, ActiveStreamingSession>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Starts or replaces a live [`StreamingParSession`] for `nzb_id` from a `.par2` file on disk.
pub fn streaming_session_start(
    nzb_id: i32,
    par2_path: &Path,
    base_dir: &Path,
    options: SessionOptions,
) -> Result<(), RepairError> {
    let set = load_par2_set(par2_path)?;
    let session = StreamingParSession::new(&set, base_dir, options)?;
    let mut map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    map.insert(
        nzb_id,
        ActiveStreamingSession {
            session,
            pending_unpause_ids: Vec::new(),
        },
    );
    Ok(())
}

/// Starts or replaces a live [`StreamingParSession`] for `nzb_id` from in-memory `.par2` bytes.
pub fn streaming_session_start_bytes(
    nzb_id: i32,
    par2_bytes: &[u8],
    base_dir: &Path,
    options: SessionOptions,
) -> Result<(), RepairError> {
    let set = load_par2_set_bytes(par2_bytes).map_err(|source| RepairError::Io {
        context: "loading in-memory par2 bytes".to_string(),
        source,
    })?;
    let session = StreamingParSession::new(&set, base_dir, options)?;
    let mut map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    map.insert(
        nzb_id,
        ActiveStreamingSession {
            session,
            pending_unpause_ids: Vec::new(),
        },
    );
    Ok(())
}

/// Registers a queued `.vol*.par2` volume with the active [`StreamingParSession`] for `nzb_id`.
pub fn streaming_session_register_queued_volume(
    nzb_id: i32,
    volume_id: u64,
    blocks: i32,
    paused: bool,
) -> bool {
    let map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = map.get(&nzb_id) else {
        return false;
    };
    entry.session.register_queued_volume(QueuedVolume {
        id: volume_id,
        blocks,
        paused,
    });
    true
}

/// Resolves a PAR2 [`FileId`] by local filename in the active [`StreamingParSession`].
pub fn streaming_session_file_id_by_name(nzb_id: i32, name: &[u8]) -> Option<FileId> {
    let map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    map.get(&nzb_id)?.session.file_id_by_name(name)
}

/// Resolves a PAR2 [`FileId`] by 16 KiB MD5 hash in the active [`StreamingParSession`].
pub fn streaming_session_file_id_by_hash16k(nzb_id: i32, hash_16k: &[u8; 16]) -> Option<FileId> {
    let map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    map.get(&nzb_id)?.session.file_id_by_hash16k(hash_16k)
}

/// Feeds a completed article by [`FileId`] into the active [`StreamingParSession`].
pub fn streaming_session_on_article_finished(
    nzb_id: i32,
    file_id: &FileId,
    offset: u64,
    size: u64,
    crc32: u32,
    bytes: Option<&[u8]>,
) -> Option<Result<ArticleOutcome, RepairError>> {
    let mut map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let entry = map.get_mut(&nzb_id)?;
    let res = entry
        .session
        .on_article_finished(file_id, offset, size, crc32, bytes);
    if let Ok(ref outcome) = res {
        if let Some(ref act) = outcome.unpause {
            for &vid in &act.volume_ids_to_unpause {
                if !entry.pending_unpause_ids.contains(&vid) {
                    entry.pending_unpause_ids.push(vid);
                }
            }
        }
    }
    Some(res)
}

/// Feeds a completed article by filename into the active [`StreamingParSession`].
pub fn streaming_session_on_article_finished_by_name(
    nzb_id: i32,
    name: &[u8],
    offset: u64,
    size: u64,
    crc32: u32,
    bytes: Option<&[u8]>,
) -> Option<Result<ArticleOutcome, RepairError>> {
    let mut map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let entry = map.get_mut(&nzb_id)?;
    let res = entry
        .session
        .on_article_finished_by_name(name, offset, size, crc32, bytes);
    if let Ok(ref outcome) = res {
        if let Some(ref act) = outcome.unpause {
            for &vid in &act.volume_ids_to_unpause {
                if !entry.pending_unpause_ids.contains(&vid) {
                    entry.pending_unpause_ids.push(vid);
                }
            }
        }
    }
    Some(res)
}

/// Marks an article permanently failed by [`FileId`] and returns `(events, unpause_action)`.
pub fn streaming_session_on_article_failed_permanently(
    nzb_id: i32,
    file_id: &FileId,
    offset: u64,
    size: u64,
) -> Option<Result<(Vec<SliceEvent>, Option<UnpauseAction>), RepairError>> {
    let mut map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let entry = map.get_mut(&nzb_id)?;
    let res = entry
        .session
        .on_article_failed_permanently(file_id, offset, size);
    if let Ok((_, Some(ref act))) = res {
        for &vid in &act.volume_ids_to_unpause {
            if !entry.pending_unpause_ids.contains(&vid) {
                entry.pending_unpause_ids.push(vid);
            }
        }
    }
    Some(res)
}

/// Marks an article permanently failed by filename and returns `(events, unpause_action)`.
pub fn streaming_session_on_article_failed_permanently_by_name(
    nzb_id: i32,
    name: &[u8],
    offset: u64,
    size: u64,
) -> Option<Result<(Vec<SliceEvent>, Option<UnpauseAction>), RepairError>> {
    let mut map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let entry = map.get_mut(&nzb_id)?;
    let res = entry
        .session
        .on_article_failed_permanently_by_name(name, offset, size);
    if let Ok((_, Some(ref act))) = res {
        for &vid in &act.volume_ids_to_unpause {
            if !entry.pending_unpause_ids.contains(&vid) {
                entry.pending_unpause_ids.push(vid);
            }
        }
    }
    Some(res)
}

/// Registers a newly downloaded `.vol*.par2` recovery volume with the active [`StreamingParSession`].
pub fn streaming_session_on_recovery_volume_arrived(
    nzb_id: i32,
    path: &Path,
    queued_id: Option<u64>,
) -> Option<Result<usize, RepairError>> {
    let mut map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let entry = map.get_mut(&nzb_id)?;
    if let Some(id) = queued_id {
        entry.pending_unpause_ids.retain(|&x| x != id);
    }
    Some(entry.session.on_recovery_volume_arrived(path, queued_id))
}

/// Polls the live status of the [`StreamingParSession`] for `nzb_id`.
pub fn streaming_session_status(nzb_id: i32) -> Option<LiveStatus> {
    let map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    Some(map.get(&nzb_id)?.session.status())
}

/// Returns `blocks_needed_now` (`status().needed_recovery_blocks`) for the active [`StreamingParSession`].
pub fn streaming_session_blocks_needed_now(nzb_id: i32) -> Option<i64> {
    let map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    Some(map.get(&nzb_id)?.session.status().needed_recovery_blocks)
}

/// Returns the [`VolumeStatus`] of `file_id` in the active [`StreamingParSession`].
pub fn streaming_session_volume_status(
    nzb_id: i32,
    file_id: &FileId,
) -> Option<Result<VolumeStatus, RepairError>> {
    let map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    Some(map.get(&nzb_id)?.session.volume_status(file_id))
}

/// Repairs the specified target files immediately using streamed syndromes (`early_cutover_possible`).
pub fn streaming_session_repair_available_targets_now(
    nzb_id: i32,
    target_file_ids: &[FileId],
) -> Option<Result<EarlyRepairReport, RepairError>> {
    let map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let entry = map.get(&nzb_id)?;
    Some(entry.session.repair_available_targets_now(target_file_ids))
}

/// Splices candidate bytes into `hole_range` of `disk_file` for `DirectUnpack` and returns verified slice indices.
pub fn streaming_session_try_splice_live_slice(
    nzb_id: i32,
    file_id: &FileId,
    hole_range: ByteRange,
    candidate_bytes: &[u8],
    disk_file: &Path,
) -> Option<Result<Vec<usize>, RepairError>> {
    let map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let entry = map.get(&nzb_id)?;
    Some(entry.session.try_splice_live_slice(
        file_id,
        hole_range,
        candidate_bytes,
        disk_file,
    ))
}

/// Takes any pending volume IDs recommended for unpausing by `on_article_failed_permanently`.
pub fn streaming_session_take_unpause_ids(nzb_id: i32) -> Vec<u64> {
    let mut map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = map.get_mut(&nzb_id) else {
        return Vec::new();
    };
    std::mem::take(&mut entry.pending_unpause_ids)
}

/// Returns `true` if `nzb_id` currently has an active [`StreamingParSession`].
pub fn streaming_session_has_session(nzb_id: i32) -> bool {
    let map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    map.contains_key(&nzb_id)
}

/// Drops the [`StreamingParSession`] for `nzb_id`.
pub fn streaming_session_forget(nzb_id: i32) {
    let mut map = STREAMING_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    map.remove(&nzb_id);
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
            let fd = set.file_descs.get(id).ok_or_else(|| {
                RepairError::IncompleteMetadata {
                    missing_descriptions: vec![*id],
                }
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

/// C callback table passed from `ParChecker.cpp` to `RustParRepairer`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NzbgetRsParCallbacks {
    pub ctx: *mut c_void,
    pub on_filename: Option< unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char)>,
    pub on_progress: Option<unsafe extern "C" fn(ctx: *mut c_void, progress: c_int)>,
    pub on_done: Option<
        unsafe extern "C" fn(
            ctx: *mut c_void,
            name: *const c_char,
            available: c_int,
            total: c_int,
            target_exists: c_int,
            is_complete: c_int,
        ),
    >,
    pub on_quick_verify: Option<
        unsafe extern "C" fn(
            ctx: *mut c_void,
            name: *const c_char,
            out_crc: *mut u32,
            out_segs: *mut *const NzbgetRsParSegment,
            out_seg_len: *mut usize,
        ) -> c_int,
    >,
    pub on_quick_verified_inc: Option<unsafe extern "C" fn(ctx: *mut c_void)>,
}

struct CallbackObserver<'a> {
    cbs: &'a NzbgetRsParCallbacks,
    emit_file_signals: bool,
}

impl RepairObserver for CallbackObserver<'_> {
    fn progress(&mut self, permille: u32, _label: &str) {
        if let Some(cb) = self.cbs.on_progress {
            unsafe {
                cb(self.cbs.ctx, permille as c_int);
            }
        }
    }

    fn file_verified(&mut self, file: &SourceFileState) {
        if !self.emit_file_signals {
            return;
        }
        let Ok(c_name) = std::ffi::CString::new(file.target_filename.as_bytes()) else {
            return;
        };
        if file.target_exists {
            if let Some(cb) = self.cbs.on_filename {
                unsafe {
                    cb(self.cbs.ctx, c_name.as_ptr());
                }
            }
        }
        if file.quick_verified {
            if let Some(cb) = self.cbs.on_quick_verified_inc {
                unsafe {
                    cb(self.cbs.ctx);
                }
            }
        }
        if let Some(cb) = self.cbs.on_done {
            unsafe {
                cb(
                    self.cbs.ctx,
                    c_name.as_ptr(),
                    file.available_blocks as c_int,
                    file.block_count as c_int,
                    i32::from(file.target_exists),
                    i32::from(file.status == FileStatus::Complete),
                );
            }
        }
        if let Some(cb) = self.cbs.on_progress {
            unsafe {
                cb(self.cbs.ctx, 1000);
            }
        }
    }
}

struct CallbackQuickVerifier<'a> {
    cbs: &'a NzbgetRsParCallbacks,
}

impl QuickVerifier for CallbackQuickVerifier<'_> {
    fn quick_info(&mut self, target_filename: &str) -> Option<QuickInfo> {
        let cb = self.cbs.on_quick_verify?;
        let c_name = std::ffi::CString::new(target_filename.as_bytes()).ok()?;
        let mut crc = 0u32;
        let mut segs_ptr: *const NzbgetRsParSegment = std::ptr::null();
        let mut segs_len = 0usize;
        let status = unsafe {
            cb(
                self.cbs.ctx,
                c_name.as_ptr(),
                &mut crc,
                &mut segs_ptr,
                &mut segs_len,
            )
        };
        match status {
            // fsSuccess = 1
            1 => Some(QuickInfo::Success { crc }),
            // fsPartial = 2
            2 => {
                let raw_segs = if !segs_ptr.is_null() && segs_len > 0 {
                    unsafe { std::slice::from_raw_parts(segs_ptr, segs_len) }
                } else {
                    &[]
                };
                let segments = raw_segs
                    .iter()
                    .filter(|s| s.offset >= 0 && s.size >= 0)
                    .map(|s| ParSegment {
                        success: s.success != 0,
                        offset: s.offset,
                        size: s.size as i64,
                        crc: s.crc,
                    })
                    .collect();
                Some(QuickInfo::Partial { segments })
            }
            // fsFailure = 3
            3 => Some(QuickInfo::Failure),
            _ => None,
        }
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
        let repairer = Par2Repairer::new(&base_dir, options);
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

    pub fn preprocess(&mut self, par_filename: &Path, cbs: Option<&NzbgetRsParCallbacks>) -> i32 {
        if !par_filename.is_file() {
            return PAR_RESULT_INSUFFICIENT_CRITICAL_DATA;
        }
        if self.repairer.load_par2_set(par_filename).is_err() {
            return PAR_RESULT_INSUFFICIENT_CRITICAL_DATA;
        }
        if let Some(cbs) = cbs {
            if let Some(cb) = cbs.on_filename {
                let main_name = par_filename
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if let Ok(c_name) = std::ffi::CString::new(main_name.as_bytes()) {
                    unsafe {
                        cb(cbs.ctx, c_name.as_ptr());
                    }
                }
                if let Some(parent) = par_filename.parent() {
                    if let Ok(entries) = std::fs::read_dir(parent) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p == par_filename || !p.is_file() {
                                continue;
                            }
                            let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                                continue;
                            };
                            if name.to_ascii_lowercase().ends_with(".par2")
                                && same_par_collection(name.as_bytes(), main_name.as_bytes(), false)
                            {
                                if let Ok(c_name) = std::ffi::CString::new(name.as_bytes()) {
                                    unsafe {
                                        cb(cbs.ctx, c_name.as_ptr());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        // If `ParIgnoreExt` contains wildcard patterns (e.g. `*`), match every FileDesc
        // filename with `crate::util::match_file_ext` and ensure matched names are in
        // `ignore_extensions` before finalizing.
        if !self.par_ignore_ext.is_empty() {
            let mut extra_ignores = Vec::new();
            let lower_fn = |b: u8| i32::from(b.to_ascii_lowercase());
            let lower = crate::wildmask::Lower::Fold(&lower_fn);
            for fd in self.repairer.packets().file_descs.values() {
                let local = fd.local_name();
                if crate::util::match_file_ext(
                    local.as_bytes(),
                    &self.par_ignore_ext,
                    b",;",
                    &lower,
                    &lower,
                ) {
                    extra_ignores.push(local);
                }
            }
            if !extra_ignores.is_empty() {
                let mut ignore_extensions = parse_ignore_ext_tokens(&self.par_ignore_ext);
                for name in extra_ignores {
                    if !ignore_extensions
                        .iter()
                        .any(|e| e.eq_ignore_ascii_case(&name))
                    {
                        ignore_extensions.push(name);
                    }
                }
                let options = RepairOptions {
                    num_threads: self.threads_to_use.max(1) as usize,
                    mem_limit_bytes: (self.mem_to_use_mb.max(16) as u64) << 20,
                    backup: BackupPolicy::RenameNumbered,
                    verify_repaired: !self.par_quick,
                    sync_repaired: false,
                    rename_misnamed: true,
                    full_file_md5: false,
                    ignore_extensions,
                    ..RepairOptions::default()
                };
                let mut rebuilt = Par2Repairer::new(&self.base_dir, options);
                if rebuilt.load_par2_set(par_filename).is_err() {
                    return PAR_RESULT_INSUFFICIENT_CRITICAL_DATA;
                }
                self.repairer = rebuilt;
            }
        }
        match self.repairer.finalize() {
            Ok(()) => PAR_RESULT_SUCCESS,
            Err(_) => PAR_RESULT_INSUFFICIENT_CRITICAL_DATA,
        }
    }

    pub fn process(&mut self, do_repair: bool, cbs: &NzbgetRsParCallbacks) -> i32 {
        if !do_repair {
            if !self.has_verified {
                let mut obs = CallbackObserver {
                    cbs,
                    emit_file_signals: true,
                };
                let res = if self.par_quick {
                    let mut qv = CallbackQuickVerifier { cbs };
                    self.repairer.verify(Some(&mut qv), &mut obs)
                } else {
                    self.repairer.verify(None, &mut obs)
                };
                if let Err(err) = res {
                    return map_repair_error(err);
                }
                if self.par_scan_full {
                    let mut extra_obs = CallbackObserver {
                        cbs,
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
                PAR_RESULT_SUCCESS
            } else if v.repair_possible {
                PAR_RESULT_REPAIR_POSSIBLE
            } else {
                PAR_RESULT_REPAIR_NOT_POSSIBLE
            }
        } else {
            let mut obs = CallbackObserver {
                cbs,
                emit_file_signals: false,
            };
            let repaired_filenames: Vec<std::ffi::CString> = self
                .repairer
                .source_files()
                .iter()
                .filter(|sf| sf.status != FileStatus::Complete)
                .filter_map(|sf| std::ffi::CString::new(sf.target_filename.as_bytes()).ok())
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
            match self.repairer.repair(&mut obs) {
                Ok(RepairResult::NotNeeded) => PAR_RESULT_SUCCESS,
                Ok(RepairResult::Repaired { mut leftovers, .. }) => {
                    if let Some(cb) = cbs.on_filename {
                        for c_name in &repaired_filenames {
                            unsafe {
                                cb(cbs.ctx, c_name.as_ptr());
                            }
                        }
                    }
                    for frag in used_split_fragments {
                        if frag.is_file() && !leftovers.contains(&frag) {
                            leftovers.push(frag);
                        }
                    }
                    self.leftovers = leftovers;
                    PAR_RESULT_SUCCESS
                }
                Err(err) => map_repair_error(err),
            }
        }
    }

    pub fn load_extra_par2_file(&mut self, path: &Path, cbs: Option<&NzbgetRsParCallbacks>) -> bool {
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
            if let Some(cbs) = cbs {
                if let Some(cb) = cbs.on_filename {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if let Ok(c_name) = std::ffi::CString::new(name.as_bytes()) {
                            unsafe {
                                cb(cbs.ctx, c_name.as_ptr());
                            }
                        }
                    }
                }
            }
        }
        ok
    }

    pub fn verify_extra_file(
        &mut self,
        path: &Path,
        is_split_fragment: bool,
        cbs: &NzbgetRsParCallbacks,
    ) -> bool {
        if !path.is_file() {
            return false;
        }
        let base_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        if let Ok(c_name) = std::ffi::CString::new(base_name.as_bytes()) {
            if let Some(cb) = cbs.on_filename {
                unsafe {
                    cb(cbs.ctx, c_name.as_ptr());
                }
            }
        }
        let was_missing_files = self.missing_file_count();
        let mut obs = CallbackObserver {
            cbs,
            emit_file_signals: false,
        };
        let gained = self
            .repairer
            .add_extra_file(path, &mut obs)
            .unwrap_or(0);
        if is_split_fragment && gained > 0 && !self.split_candidates.iter().any(|p| p == path) {
            self.split_candidates.push(path.to_path_buf());
        }
        if let Ok(c_name) = std::ffi::CString::new(base_name.as_bytes()) {
            if let Some(cb) = cbs.on_done {
                unsafe {
                    cb(cbs.ctx, c_name.as_ptr(), gained as c_int, gained as c_int, 1, 0);
                }
            }
        }
        gained > 0 || self.missing_file_count() < was_missing_files
    }

    pub fn cancel(&self) {
        self.repairer.cancel();
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
}

fn parse_ignore_ext_tokens(raw: &[u8]) -> Vec<String> {
    let Ok(s) = std::str::from_utf8(raw) else {
        return Vec::new();
    };
    s.split([',', ';', ' '])
        .map(str::trim)
        .filter(|t| !t.is_empty() && !t.contains('*') && !t.contains('?'))
        .map(|t| {
            if t.starts_with('.') {
                t.to_string()
            } else {
                format!(".{t}")
            }
        })
        .collect()
}

fn map_repair_error(err: RepairError) -> i32 {
    match err {
        RepairError::NoMainPacket | RepairError::IncompleteMetadata { .. } => {
            PAR_RESULT_INSUFFICIENT_CRITICAL_DATA
        }
        RepairError::NotEnoughRecovery { .. } => PAR_RESULT_REPAIR_NOT_POSSIBLE,
        RepairError::Io { .. } => PAR_RESULT_FILE_IO_ERROR,
        RepairError::Cancelled | RepairError::VerificationFailed { .. } | RepairError::Singular => {
            PAR_RESULT_REPAIR_FAILED
        }
        RepairError::NotFinalized | RepairError::WrongSet { .. } | RepairError::UnknownFile(_) => {
            PAR_RESULT_LOGIC_ERROR
        }
    }
}

/// Computes `(threads_to_use, mem_to_use_mb)` for `Repairer::Repairer`.
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

/// Formats `-m<mem>`, `-t<threads>`, `-T<threads>`, and optional `<dir>/*` wildcard parameter
/// (preserving `BString<1024>` 1023-byte truncation before `BaseFileName`) for `Repairer::PreProcess`.
pub fn build_preprocess_cli_args(
    threads_to_use: i32,
    mem_to_use: i32,
    par_filename: &[u8],
    par_scan_full: bool,
) -> (Vec<u8>, Vec<u8>, Vec<u8>, Option<Vec<u8>>) {
    let threads_str = threads_to_use.to_string();
    let mem_param = format!("-m{mem_to_use}").into_bytes();
    let threads_param = format!("-t{threads_str}").into_bytes();
    let file_threads_param = format!("-T{threads_str}").into_bytes();
    let wildcard_param = if par_scan_full {
        let truncated = &par_filename[..par_filename.len().min(1023)];
        let base_idx = crate::paths::base_file_name(truncated);
        if base_idx > 0 && base_idx < truncated.len() {
            let mut out = truncated[..base_idx].to_vec();
            out.push(b'*');
            Some(out)
        } else {
            None
        }
    } else {
        None
    };
    (mem_param, threads_param, file_threads_param, wildcard_param)
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

/// Returns the NUL-terminated error message for `Par2CmdLineErrStr[res]`.
pub fn cmdline_err_str(res: i32) -> &'static [u8] {
    match res {
        0 => b"OK\0",
        1 => b"data files are damaged and there is enough recovery data available to repair them\0",
        2 => b"data files are damaged and there is insufficient recovery data available to be able to repair them\0",
        3 => b"there was something wrong with the command line arguments\0",
        4 => b"the PAR2 files did not contain sufficient information about the data files to be able to verify them\0",
        5 => b"repair completed but the data files still appear to be damaged\0",
        6 => b"an error occurred when accessing files\0",
        7 => b"internal error occurred\0",
        8 => b"out of memory\0",
        _ => b"internal error occurred\0",
    }
}

/// Returns the NUL-terminated stage message for `Par2StageMessage[stage]`.
pub fn stage_message(stage: i32) -> &'static [u8] {
    match stage {
        0 => b"Loading file\0",
        1 => b"Verifying file\0",
        2 => b"Repairing file\0",
        3 => b"Verifying repaired file\0",
        _ => b"Verifying file\0",
    }
}

/// Formats `"<verb> <info_name>"` (e.g. `"Verifying <info_name>"`, `"Repairing <info_name>"`).
pub fn format_action_label(verb: &[u8], info_name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(verb.len() + 1 + info_name.len());
    out.extend_from_slice(verb);
    out.push(b' ');
    out.extend_from_slice(info_name);
    out
}

/// Evaluates whether `ParChecker::RunParCheck` should attempt an additional recovery step
/// (`AddSplittedFragments`, `ProcessMorePars`, `AddDupeFiles`, or `ProcessTwinSet`).
pub fn should_try_more_recovery(
    has_damaged_files: bool,
    is_stopped: bool,
    res: i32,
    e_repair_not_possible: i32,
    require_dupe_scan: bool,
    par_scan: i32,
    ps_dupe: i32,
) -> bool {
    has_damaged_files
        && !is_stopped
        && res == e_repair_not_possible
        && (!require_dupe_scan || par_scan == ps_dupe)
}

/// Decides whether `ParChecker::RunParCheck` should scan for missing files (`AddMissingFiles`).
#[allow(clippy::too_many_arguments)]
pub fn should_add_missing_files(
    has_damaged: bool,
    is_stopped: bool,
    missing_file_count: u32,
    added_split: bool,
    res: i32,
    e_repair_possible: i32,
    par_scan: i32,
    ps_extended: i32,
    ps_dupe: i32,
) -> bool {
    has_damaged
        && !is_stopped
        && missing_file_count > 0
        && !(added_split && res == e_repair_possible)
        && (par_scan == ps_extended || par_scan == ps_dupe)
}

/// Evaluates the post-verify status and repair action in `ParChecker::RunParCheck`.
/// Returns `(status, action_code)`:
/// - `0`: repair not needed
/// - `1`: repair possible and `ParRepair` enabled (caller runs repair pass)
/// - `2`: repair possible and `ParRepair` disabled
/// - `3`: repair failed
#[allow(clippy::too_many_arguments)]
pub fn eval_post_verify_action(
    res: i32,
    e_success: i32,
    e_repair_possible: i32,
    has_damaged_files: bool,
    par_repair: bool,
    ps_failed: i32,
    ps_repair_not_needed: i32,
    ps_repair_possible: i32,
) -> (i32, i32) {
    if res == e_success || !has_damaged_files {
        (ps_repair_not_needed, 0)
    } else if res == e_repair_possible {
        if par_repair {
            (ps_repair_possible, 1)
        } else {
            (ps_repair_possible, 2)
        }
    } else {
        (ps_failed, 3)
    }
}

/// Evaluates cancellation or failure outcome at the end of `ParChecker::RunParCheck`.
/// Returns `(final_status, outcome_kind, use_cmdline_err)`:
/// - `0`: normal completion
/// - `1`: repair cancelled (`stage >= pt_repairing`)
/// - `2`: par-check cancelled (`stage < pt_repairing`)
/// - `3`: repair failed (`cur_status == ps_failed`)
#[allow(clippy::too_many_arguments)]
pub fn eval_cancel_or_fail_outcome(
    is_stopped: bool,
    stage: i32,
    pt_repairing: i32,
    cur_status: i32,
    ps_failed: i32,
    ps_repair_possible: i32,
    err_msg_empty: bool,
    res: i32,
) -> (i32, i32, bool) {
    if is_stopped {
        if stage >= pt_repairing {
            (ps_repair_possible, 1, false)
        } else {
            (ps_failed, 2, false)
        }
    } else if cur_status == ps_failed {
        let use_cmd_err = err_msg_empty && (0..=8).contains(&res);
        (ps_failed, 3, use_cmd_err)
    } else {
        (cur_status, 0, false)
    }
}

/// Classifies the step outcome after `Repairer::PreProcess` in `ParChecker::PreProcessPar`:
/// `1` = stopped, `2` = invalid command line arguments, `3` = failed (request more pars), `0` = success.
pub fn classify_preprocess_step(
    is_stopped: bool,
    res: i32,
    e_success: i32,
    e_invalid_args: i32,
) -> i32 {
    if is_stopped {
        1
    } else if res == e_invalid_args {
        2
    } else if res != e_success {
        3
    } else {
        0
    }
}

/// Formats `"not enough par-blocks, {missing_blocks} block(s) needed, but {block_found} block(s) available"`.
pub fn format_not_enough_blocks_errmsg(missing_blocks: i32, block_found: i32) -> Vec<u8> {
    format!(
        "not enough par-blocks, {missing_blocks} block(s) needed, but {block_found} block(s) available"
    )
    .into_bytes()
}

/// Formats the twin-set PAR error message when `res == e_repair_not_possible` in `ParChecker::ProcessTwinSet`.
pub fn format_twin_err_msg(
    res: i32,
    e_repair_not_possible: i32,
    own_err_msg: &[u8],
) -> Option<Vec<u8>> {
    if res != e_repair_not_possible {
        None
    } else if own_err_msg.is_empty() {
        Some(b"not enough par-blocks, the twin's included".to_vec())
    } else {
        let suffix = b"; the twin's didn't do either";
        let mut out = Vec::with_capacity(own_err_msg.len() + suffix.len());
        out.extend_from_slice(own_err_msg);
        out.extend_from_slice(suffix);
        Some(out)
    }
}

/// `ParChecker::MaybeSplittedFragement`
pub fn maybe_splitted_fragment(filename1: &[u8], filename2: &[u8]) -> bool {
    let len = filename2.len();
    if strncasecmp_prefix(filename1, filename2) {
        if filename1.get(len) == Some(&b'.') {
            if filename1[len + 1..].iter().all(|b| b.is_ascii_digit()) {
                return true;
            }
        }
    }

    let rfind_dot = |s: &[u8]| s.iter().rposition(|&b| b == b'.');
    if let (Some(pos1), Some(pos2)) = (rfind_dot(filename1), rfind_dot(filename2)) {
        let ext1 = &filename1[pos1..];
        let ext2 = &filename2[pos2..];
        if ext1.len() == 4
            && ext2.len() == 4
            && filename2.len() >= pos1
            && strncasecmp_prefix(filename1, &filename2[..pos1])
        {
            if ext1[1..].iter().all(|b| b.is_ascii_digit())
                && ext2[1..].iter().all(|b| b.is_ascii_digit())
            {
                return true;
            }
        }
    }

    false
}

/// Evaluates `ParChecker::AddDupeFiles` block deltas after scanning a dupe directory.
/// Returns `(new_used_blocks, total_extra_blocks_found)`.
pub fn calc_dupe_blocks_delta(
    prev_used_blocks: i32,
    was_blocks_missing2: u32,
    blocks_missing2: u32,
    was_blocks_missing_total: u32,
) -> (i32, u32) {
    let delta2 = was_blocks_missing2.wrapping_sub(blocks_missing2) as i32;
    let new_used = prev_used_blocks.wrapping_add(delta2);
    let total_found = if blocks_missing2 < was_blocks_missing_total {
        was_blocks_missing_total - blocks_missing2
    } else {
        0
    };
    (new_used, total_found)
}

/// Extracts the base filename of `par_filename` with its last `.ext` stripped (`ParChecker::SortExtraFiles`).
pub fn extract_base_par_stem(par_filename: &[u8]) -> Vec<u8> {
    let base = &par_filename[crate::paths::base_file_name(par_filename)..];
    match base.iter().rposition(|&b| b == b'.') {
        Some(dot) => base[..dot].to_vec(),
        None => base.to_vec(),
    }
}

/// Comparator in `ParChecker::SortExtraFiles`:
/// `strcmp(name1, baseParFilename) == 0 && strcmp(name1, name2) != 0` after stripping `.ext`.
pub fn compare_extra_file_stems(base_name1: &[u8], base_name2: &[u8], par_stem: &[u8]) -> bool {
    let stem1 = match base_name1.iter().rposition(|&b| b == b'.') {
        Some(pos) => &base_name1[..pos],
        None => base_name1,
    };
    let stem2 = match base_name2.iter().rposition(|&b| b == b'.') {
        Some(pos) => &base_name2[..pos],
        None => base_name2,
    };
    stem1 == par_stem && stem1 != stem2
}

/// Evaluates `ParChecker::AddExtraFiles` step outcome and early-abort condition for one extra file.
/// Returns `(file_added, block_added, new_files_added, missing_blocks_delta, register_parred, abort_reason)`:
/// `abort_reason`:
/// - `1`: all missing files found (`only_missing && now_files_missing == 0`)
/// - `2`: all missing blocks found (`!only_missing && now_blocks_missing == 0`)
/// - `0`: continue scanning
pub fn eval_extra_file_step(
    was_files_missing: u32,
    now_files_missing: u32,
    was_blocks_missing: u32,
    now_blocks_missing: u32,
    external_dir: bool,
    prev_files_added: bool,
    only_missing: bool,
) -> (bool, bool, bool, u32, bool, i32) {
    let file_added = was_files_missing > now_files_missing;
    let block_added = was_blocks_missing > now_blocks_missing;
    let new_files_added = prev_files_added || file_added || block_added;
    let blocks_delta = if block_added {
        was_blocks_missing - now_blocks_missing
    } else {
        0
    };
    let register_parred = file_added && !external_dir;
    let abort_reason = if only_missing && now_files_missing == 0 {
        1
    } else if !only_missing && now_blocks_missing == 0 {
        2
    } else {
        0
    };
    (
        file_added,
        block_added,
        new_files_added,
        blocks_delta,
        register_parred,
        abort_reason,
    )
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

/// Evaluates `ParChecker::SignalDone` damaged-file state when `stage == pt_verifying_sources`.
/// Returns `(should_report, new_has_damaged_files, bad_blocks)`.
pub fn eval_signal_done(
    stage: i32,
    pt_verifying_sources: i32,
    available: i32,
    total: i32,
    verifying_extra_files: bool,
    ignore: bool,
    prev_has_damaged_files: bool,
) -> (bool, bool, i32) {
    if stage == pt_verifying_sources && available < total && !verifying_extra_files {
        (
            true,
            prev_has_damaged_files || !ignore,
            total.wrapping_sub(available),
        )
    } else {
        (false, prev_has_damaged_files, 0)
    }
}

/// Evaluates initial preconditions in `ParChecker::VerifyDataFile`:
/// - `0`: proceed with CRC lookup
/// - `1`: return `fsSuccess` (`stage != pt_verifying_sources`)
/// - `2`: return `fsUnknown` (`!target_exists || !has_verification_packet`)
/// - `3`: return `fsFailure` and set `availableBlocks = 0` (`file_size == 0 && block_count > 0`)
pub fn check_verify_data_file_preconditions(
    stage: i32,
    pt_verifying_sources: i32,
    target_exists: bool,
    has_verification_packet: bool,
    file_size: i64,
    block_count: u64,
) -> i32 {
    if stage != pt_verifying_sources {
        return 1;
    }
    if !target_exists || !has_verification_packet {
        return 2;
    }
    if file_size == 0 && block_count > 0 {
        return 3;
    }
    0
}

/// Combines per-block CRCs into a PAR2 block-range or file-level CRC.
pub fn combine_block_crcs(block_crcs: &[u32], block_size: u64) -> u32 {
    let mut par_crc = 0u32;
    let len2 = block_size as u32;
    for (i, &block_crc) in block_crcs.iter().enumerate() {
        par_crc = if i == 0 {
            block_crc
        } else {
            crate::crc::combine(par_crc, block_crc, len2)
        };
    }
    par_crc
}

/// `ParChecker::VerifyPartialDataFile` valid-block determination.
pub fn compute_valid_blocks(
    block_count: usize,
    block_size: i64,
    file_size: i64,
    segments: &[NzbgetRsParSegment],
    out_valid: &mut [u8],
) {
    let n = block_count.min(out_valid.len());
    for i in 0..n {
        let block_start = (i as i64) * block_size;
        let block_end = if block_start + block_size < file_size - 1 {
            block_start + block_size
        } else {
            file_size - 1
        };
        let mut block_ok = false;
        let mut block_end_found = false;
        let mut cur_offset = 0i64;
        for seg in segments {
            let seg_end = seg.offset + (seg.size as i64);
            if !block_ok
                && seg.success != 0
                && seg.offset <= block_start
                && seg_end >= block_start
            {
                block_ok = true;
                cur_offset = seg.offset;
            }
            if block_ok {
                if !(seg.success != 0 && seg.offset == cur_offset) {
                    block_ok = false;
                    break;
                }
                if seg_end >= block_end {
                    block_end_found = true;
                    break;
                }
                cur_offset = seg_end;
            }
        }
        out_valid[i] = u8::from(block_ok && block_end_found);
    }
}

/// Finds the next contiguous `[block_start, block_end]` inclusive run of valid blocks
/// starting at `start_idx` in `ParChecker::VerifyPartialDataFile`, and computes
/// `(block_end, par_crc, bytes_start, clamped_end, pad_len)`.
pub fn next_valid_block_run(
    valid_blocks: &[u8],
    block_crcs: &[u32],
    start_idx: usize,
    block_size: i64,
    file_size: i64,
) -> Option<(usize, u32, i64, i64, usize)> {
    let len = valid_blocks.len().min(block_crcs.len());
    if start_idx >= len {
        return None;
    }
    let rel_start = valid_blocks[start_idx..len].iter().position(|&v| v != 0)?;
    let block_start = start_idx + rel_start;
    let rel_len = valid_blocks[block_start..len]
        .iter()
        .position(|&v| v == 0)
        .unwrap_or(len - block_start);
    let block_end = block_start + rel_len - 1;
    let par_crc = combine_block_crcs(
        &block_crcs[block_start..=block_end],
        block_size as u64,
    );
    let bytes_start = (block_start as i64).wrapping_mul(block_size);
    let bytes_end = (block_end as i64)
        .wrapping_mul(block_size)
        .wrapping_add(block_size)
        .wrapping_sub(1);
    let file_last = file_size.wrapping_sub(1);
    let clamped_end = if bytes_end < file_last {
        bytes_end
    } else {
        file_last
    };
    let pad_len = if bytes_end > file_last {
        bytes_end.wrapping_sub(file_last) as usize
    } else {
        0
    };
    Some((block_end, par_crc, bytes_start, clamped_end, pad_len))
}

/// `ParChecker::SmartCalcFileRangeCrc`
pub fn smart_calc_range_crc<F>(
    start: i64,
    end: i64,
    segments: &[NzbgetRsParSegment],
    mut dumb_calc_cb: F,
) -> Option<u32>
where
    F: FnMut(i64, i64) -> Option<u32>,
{
    let mut download_crc = 0u32;
    let mut started = false;
    for seg in segments {
        let seg_end = seg.offset + (seg.size as i64);
        if !started && seg.offset > start {
            download_crc = dumb_calc_cb(start, seg.offset - 1)?;
            if seg_end >= end {
                break;
            }
            started = true;
        }

        if seg.offset >= start && seg_end <= end {
            download_crc = if !started {
                seg.crc
            } else {
                crate::crc::combine(download_crc, seg.crc, seg.size as u32)
            };
            started = true;
        }

        if seg_end == end {
            break;
        }

        if seg_end > end {
            let partial_crc = dumb_calc_cb(seg.offset, end)?;
            download_crc = crate::crc::combine(
                download_crc,
                partial_crc,
                (end - seg.offset + 1) as u32,
            );
            break;
        }
    }

    Some(download_crc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repairer_resources_and_cli_args() {
        assert_eq!(calc_repairer_resources(0, 8, false, 0, 250), (8, 250));
        assert_eq!(calc_repairer_resources(4, 8, false, 0, 0), (4, 16));
        assert_eq!(calc_repairer_resources(0, 0, true, 8, 250), (1, 16));

        let (m, t, ft, wc) = build_preprocess_cli_args(4, 250, b"/tmp/dir/show.par2", true);
        assert_eq!(m, b"-m250");
        assert_eq!(t, b"-t4");
        assert_eq!(ft, b"-T4");
        assert_eq!(wc.as_deref(), Some(&b"/tmp/dir/*"[..]));
    }

    #[test]
    fn test_post_verify_and_extra_steps() {
        assert_eq!(
            eval_post_verify_action(0, 0, 1, false, true, 0, 2, 1),
            (2, 0)
        );
        assert_eq!(
            eval_post_verify_action(1, 0, 1, true, true, 0, 2, 1),
            (1, 1)
        );
        assert_eq!(
            eval_extra_file_step(2, 1, 10, 5, false, false, false),
            (true, true, true, 5, true, 0)
        );
        assert_eq!(
            eval_extra_file_step(1, 0, 10, 5, false, false, true).5,
            1
        );
    }

    #[test]
    fn test_next_valid_block_run() {
        let valid = [0u8, 1, 1, 0, 1];
        let crcs = [0x11u32, 0x22, 0x33, 0x44, 0x55];
        let (end, _crc, bs, ce, pad) =
            next_valid_block_run(&valid, &crcs, 0, 1000, 4500).unwrap();
        assert_eq!(end, 2);
        assert_eq!(bs, 1000);
        assert_eq!(ce, 2999);
        assert_eq!(pad, 0);

        let (end2, _crc2, bs2, ce2, pad2) =
            next_valid_block_run(&valid, &crcs, 3, 1000, 4500).unwrap();
        assert_eq!(end2, 4);
        assert_eq!(bs2, 4000);
        assert_eq!(ce2, 4499);
        assert_eq!(pad2, 500);
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

        let empty_cbs = NzbgetRsParCallbacks {
            ctx: std::ptr::null_mut(),
            on_filename: None,
            on_progress: None,
            on_done: None,
            on_quick_verify: None,
            on_quick_verified_inc: None,
        };

        let mut r = RustParRepairer::new(dir.clone(), 2, 64, false, false, Vec::new());
        assert_eq!(r.preprocess(&index_path, Some(&empty_cbs)), PAR_RESULT_SUCCESS);
        assert_eq!(r.creator(), "rustyget-escalation-test");
        assert_eq!(r.block_size(), 4096);

        // Initial verify: both files are missing on target path (7 blocks missing, 0 recovery)
        assert_eq!(r.process(false, &empty_cbs), PAR_RESULT_REPAIR_NOT_POSSIBLE);
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
        let (chosen, block_found) = select_more_pars(&vol_counts, r.repairer.blocks_needed() as i32);
        assert_eq!(block_found, 7);
        assert_eq!(chosen, vec![1]);
        for idx in chosen {
            let (vname, vbytes) = &built.volumes[idx];
            let vpath = dir.join(vname);
            std::fs::write(&vpath, vbytes).unwrap();
            assert!(r.load_extra_par2_file(&vpath, Some(&empty_cbs)));
        }

        // Now repair is possible and succeeds!
        assert_eq!(r.process(false, &empty_cbs), PAR_RESULT_REPAIR_POSSIBLE);
        assert_eq!(r.process(true, &empty_cbs), PAR_RESULT_SUCCESS);
        assert_eq!(std::fs::read(dir.join("movie.mkv")).unwrap(), data_a);
        assert_eq!(std::fs::read(dir.join("sample.mkv")).unwrap(), data_b);
        assert!(r.leftovers().contains(&dir.join("movie.mkv.001")));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
