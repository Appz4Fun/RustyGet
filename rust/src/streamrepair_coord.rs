//! End-to-end Rust implementation of `StreamRepairController`
//! (`daemon/postprocess/StreamRepair.cpp` / `daemon/util/Util.h`), covering both
//! post-processing stream repair (`Run`) and download-concurrent live stream repair (`RunLive`).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{c_char, c_int, c_void, CStr};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::articlefetcher::{
    parse_donor_nzb_file, ArticleFetcher, BatchFetcherPool, DonorParsedFile, DonorParsedNzb,
    NzbgetRsNntpFetchHostFn,
};
use crate::dupecoord::same_name_or_key;
use crate::dupefallback::NzbgetRsByteRange;
use crate::dupeprobe::{sample_indexes, NzbgetRsDupeProbe, MIN_ARTICLES, SAMPLE_COUNT};
use crate::nntp::BatchRequest;
use crate::streamrepair::{
    build_donor_key, classify_remaining_hole_target, compute_positional_rank,
    credit_repaired_sizes, format_rar_volume_debug_lines, format_whole_files_suffix,
    is_incomplete_nzb_hole, is_par2_job_or_file, process_m1_donor, resolve_completion_par_action,
    stream_repair_bytes_to_mb, total_size, watchdog_eval_tick, CrossPackDonorEnv,
    CrossPackRepairSession, CrossPackTargetInput, FetchedDonorArticle, M1DonorEnv, M1RepairTarget,
    TargetMessageKind,
};

const MK_INFO: c_int = 0;
const MK_WARNING: c_int = 1;
const MK_ERROR: c_int = 2;
const MK_DETAIL: c_int = 4;

const DM_FORCE: i32 = 2;
const RS_NONE: i32 = 0;
const PS_NONE: i32 = 0;
const PS_SKIPPED: i32 = 1;
const PS_MANUAL: i32 = 5;

const SLOW_REPAIR_FACTOR: i32 = 3;
const MAX_RUN_FACTOR: i32 = 5;
const MAX_CONNECTION_WAIT_SEC: i32 = 1800;
const DONOR_CHECK_SEC: i32 = 20;

/// Host callback (`StreamRepairController::HostCb` in `Util.h`):
/// - `op == 0`: `DownloadQueue::Guard()` trampoline (`p1 = fn_ptr`, `p2 = slot_ptr`)
/// - `op == 1`: `IsStopped()` -> `0` / `1`
/// - `op == 2`: `Stop()` (invoked by watchdog on timeout after setting `timed_out = true`)
/// - `op == 3`: `SetInfoName(s1)`
/// - `op == 4`: `PrintMessage((Message::EKind)a1, "%s", s1)`
/// - `op == 5`: global log (`a1`: `0 => info`, `1 => error`, `2 => warn`, `3 => detail`, `4 => debug`)
/// - `op == 6`: query options/stats/servers ->
///   `ints[0]=dupe_stream_timeout`, `ints[1]=dupe_stream_decompress`,
///   `ints[2]=primary_connections`, `ints[3]=par_check`, `ints[4]=pc_manual`,
///   `i64s[0]=download_rate`, `strs[0]=par_ignore_ext`
/// - `op == 7`: inspect target `NzbInfo` under `dq` (`p1 = dq_ptr`, `a1 = set_init_label`) ->
///   returns `nzb_ptr` (`0` if missing or live ownership lost),
///   `ints[0]=id`, `ints[1]=has_par_files`, `ints[2]=completed_count`, `ints[3]=job_count`,
///   `ints[4]=queue_count`, `ints[5]=history_count`, `i64s[0]=full_content_hash`,
///   `strs[0]=name`, `strs[1]=dest_dir`, `strs[2]=dupe_key`, `strs[3]=unpack_password`
/// - `op == 8`: read `CompletedFile` at `a1` of `nzb` (`p1 = nzb_ptr`) ->
///   `ints[0]=id`, `ints[1]=par_file`, `ints[2]=status`, `strs[0]=filename`
/// - `op == 9`: read/mark `StreamRepairJob` at `a1` of `nzb` (`p1 = nzb_ptr`):
///   if `a2 == 1`, marks `SetLiveAttempted(true)` and returns previous `GetLiveAttempted()`;
///   if `a2 == 0`, fills `ints[0]=file_id`, `ints[1]=failed_articles`, `ints[2]=par_file`,
///   `ints[3]=live_attempted`, `ints[4]=hole_count`, `i64s[0]=decoded_file_size`,
///   `i64s[1]=failed_size`, `i64s[2]=missed_size`, `i64s[3]=steps_hash`,
///   `strs[0]=filename`, `strs[1]=holes_ptr`
/// - `op == 10`: read candidate donor `NzbInfo` under `dq` (`p1 = dq_ptr`, `a1 = 0` queue / `1` history, `a2 = idx`) ->
///   returns `cand_nzb_ptr` (`0` if not NZB), `ints[0]=id`, `ints[1]=dupe_score`, `ints[2]=dupe_mode`,
///   `i64s[0]=full_content_hash`, `strs[0]=name`, `strs[1]=dupe_key`, `strs[2]=queued_filename`,
///   `strs[3]=unpack_password`
/// - `op == 11`: set `PostInfo` progress (`s1 = label`, `a1 = stage_progress`)
/// - `op == 12`: mark `CompletedFile` on `nzb` (`p1 = nzb_ptr`, `a1 = file_id`, `a2 = 1` repaired / `2` recreated)
/// - `op == 13`: mutate `StreamRepairJob` at `a1` of `nzb` (`p1 = nzb_ptr`):
///   `a2 == 0` erases job; `a2 == 1` sets `DecodedFileSize` (if `i64s[0] >= 0`) and `Holes` (`p2`, `ints[0]`)
/// - `op == 14`: post-process `RepairCompleted` (`p1 = dq_ptr`, `p2 = nzb_ptr`):
///   `a2 == 0` reads `ints[0]=par_rename_status`, `ints[1]=par_status`, `i64s[0..8]=sizes`;
///   `a2 == 1` writes counters/flags (`a1 = write_sizes`), `dq->Save()`, `SetWorking(false)`
/// - `op == 15`: live-mode `RepairCompletedLive` (`p1 = dq_ptr`, `p2 = nzb_ptr`):
///   `a2 == 0` checks live ownership, credits recovered bytes/holes, fills `ints[0]=job_count`, returns `nzb_ptr`;
///   `a2 == 1` clears `LiveRepairThread`, clears `PostInfo::Working` if `!UnpackThread`, `dq->Save()`
pub type NzbgetRsStreamRepairHostFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
) -> usize;

#[inline]
fn to_c_bytes(bytes: &[u8]) -> Vec<u8> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let mut out = Vec::with_capacity(end + 1);
    out.extend_from_slice(&bytes[..end]);
    out.push(0);
    out
}

#[inline]
unsafe fn ptr_to_bytes(p: *const c_char) -> Vec<u8> {
    if p.is_null() {
        Vec::new()
    } else {
        unsafe { CStr::from_ptr(p).to_bytes().to_vec() }
    }
}

#[inline]
fn map_target_msg_kind(kind: TargetMessageKind) -> c_int {
    match kind {
        TargetMessageKind::Info => MK_INFO,
        TargetMessageKind::Warning => MK_WARNING,
        TargetMessageKind::Error => MK_ERROR,
        TargetMessageKind::Detail => MK_DETAIL,
    }
}

fn stream_fetch_worker_count(primary_connections: i32) -> i32 {
    (primary_connections / 4).clamp(4, 16)
}

unsafe fn with_dq_guard<R, F: FnOnce(usize) -> R>(
    ctx: *mut c_void,
    cb: NzbgetRsStreamRepairHostFn,
    f: F,
) -> R {
    struct Slot<F, R> {
        f: Option<F>,
        out: Option<R>,
    }
    unsafe extern "C" fn tramp<F: FnOnce(usize) -> R, R>(slot_ptr: usize, dq_ptr: usize) {
        let slot = unsafe { &mut *(slot_ptr as *mut Slot<F, R>) };
        if let Some(f) = slot.f.take() {
            slot.out = Some(f(dq_ptr));
        }
    }
    let mut slot = Slot {
        f: Some(f),
        out: None,
    };
    unsafe {
        cb(
            ctx,
            0,
            0,
            0,
            tramp::<F, R> as *const () as usize,
            (&mut slot as *mut Slot<F, R>) as usize,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
    }
    slot.out.expect("DownloadQueue::Guard callback must run")
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
    pc_manual: i32,
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

pub struct NzbgetRsStreamRepairCtrl {
    live_mode: bool,
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
        _nzb_id: i32,
        nntp_host: NzbgetRsNntpFetchHostFn,
        tls_enabled: bool,
        debug_enabled: bool,
    ) -> Self {
        Self {
            live_mode,
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

    #[inline]
    unsafe fn host_is_stopped(&self, ctx: *mut c_void, cb: NzbgetRsStreamRepairHostFn) -> bool {
        unsafe {
            cb(
                ctx,
                1,
                0,
                0,
                0,
                0,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            ) != 0
        }
    }

    #[inline]
    unsafe fn interrupted(&self, ctx: *mut c_void, cb: NzbgetRsStreamRepairHostFn) -> bool {
        unsafe { self.host_is_stopped(ctx, cb) && !self.timed_out.load(Ordering::SeqCst) }
    }

    unsafe fn set_info_name(&self, ctx: *mut c_void, cb: NzbgetRsStreamRepairHostFn, name: &[u8]) {
        let name_c = to_c_bytes(name);
        unsafe {
            cb(
                ctx,
                3,
                0,
                0,
                0,
                0,
                name_c.as_ptr() as *const c_char,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    unsafe fn print_message(
        &self,
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
        kind: c_int,
        text: &[u8],
    ) {
        let text_c = to_c_bytes(text);
        unsafe {
            cb(
                ctx,
                4,
                kind,
                0,
                0,
                0,
                text_c.as_ptr() as *const c_char,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    unsafe fn global_log(
        &self,
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
        kind: c_int,
        text: &[u8],
    ) {
        let text_c = to_c_bytes(text);
        unsafe {
            cb(
                ctx,
                5,
                kind,
                0,
                0,
                0,
                text_c.as_ptr() as *const c_char,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    unsafe fn set_post_progress(
        &self,
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
        label: &[u8],
        stage_progress: i32,
    ) {
        let label_c = to_c_bytes(label);
        unsafe {
            cb(
                ctx,
                11,
                stage_progress,
                0,
                0,
                0,
                label_c.as_ptr() as *const c_char,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }

    unsafe fn query_host_options(
        &self,
        _ctx: *mut c_void,
        _cb: NzbgetRsStreamRepairHostFn,
    ) -> HostOptions {
        let opts = crate::options::active_options();
        let primary_connections = crate::net::active_server_pool().map_or(0, |p| unsafe {
            crate::net::nzbget_rs_serverpool_ctl(Arc::as_ptr(&p), 6, 0, 0, 0, 0) as c_int
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
            pc_manual: 3,
            download_rate,
            par_ignore_ext: opts
                .as_deref()
                .map_or_else(Vec::new, |o| o.str_bytes(33).to_vec()),
        }
    }

    unsafe fn collect_targets_and_donors(
        &self,
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
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
        let mut ints = [0 as c_int; 6];
        let mut i64s = [0i64; 1];
        let mut strs = [ptr::null::<c_char>(); 4];
        let nzb_ptr = unsafe {
            cb(
                ctx,
                7,
                c_int::from(set_init_label),
                0,
                dq_ptr,
                0,
                ptr::null(),
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                strs.as_mut_ptr(),
            )
        };
        if nzb_ptr == 0 {
            return None;
        }
        let own_nzb_ptr = nzb_ptr;
        counters.has_par_files = ints[1] != 0;
        let completed_count = ints[2].max(0) as usize;
        let job_count = ints[3].max(0) as usize;
        let queue_count = ints[4].max(0) as usize;
        let history_count = ints[5].max(0) as usize;
        let own_full_hash = i64s[0];
        let nzb_name = unsafe { ptr_to_bytes(strs[0]) };
        let dest_dir = unsafe { ptr_to_bytes(strs[1]) };
        let own_dupe_key = unsafe { ptr_to_bytes(strs[2]) };
        if !strs[3].is_null() {
            counters.target_password = unsafe { ptr_to_bytes(strs[3]) };
        }

        let mut completed_files: Vec<(i32, bool, Vec<u8>)> = Vec::with_capacity(completed_count);
        let mut member_names: Vec<Vec<u8>> = Vec::with_capacity(completed_count);
        for idx in 0..completed_count {
            let mut cf_ints = [0 as c_int; 3];
            let mut cf_strs = [ptr::null::<c_char>(); 1];
            unsafe {
                cb(
                    ctx,
                    8,
                    idx as c_int,
                    0,
                    nzb_ptr,
                    0,
                    ptr::null(),
                    cf_ints.as_mut_ptr(),
                    ptr::null_mut(),
                    cf_strs.as_mut_ptr(),
                );
            }
            let fname = unsafe { ptr_to_bytes(cf_strs[0]) };
            member_names.push(fname.clone());
            completed_files.push((cf_ints[0], cf_ints[1] != 0, fname));
        }

        let mut job_file_ids: Vec<i32> = Vec::with_capacity(job_count);
        let mut targets: Vec<RepairTarget> = Vec::new();
        for idx in 0..job_count {
            let mut j_ints = [0 as c_int; 5];
            let mut j_i64s = [0i64; 4];
            let mut j_strs = [ptr::null::<c_char>(); 2];
            unsafe {
                cb(
                    ctx,
                    9,
                    idx as c_int,
                    0,
                    nzb_ptr,
                    0,
                    ptr::null(),
                    j_ints.as_mut_ptr(),
                    j_i64s.as_mut_ptr(),
                    j_strs.as_mut_ptr(),
                );
            }
            let file_id = j_ints[0];
            job_file_ids.push(file_id);
            let failed_articles = j_ints[1];
            let mut is_par_file = j_ints[2] != 0;
            let hole_count = j_ints[4].max(0) as usize;
            let decoded_file_size = j_i64s[0];
            let failed_size = j_i64s[1];
            let missed_size = j_i64s[2];
            let steps_hash = j_i64s[3] as u64;
            let mut filename = unsafe { ptr_to_bytes(j_strs[0]) };
            if is_par2_job_or_file(is_par_file, &filename) {
                continue;
            }
            let holes: Vec<NzbgetRsByteRange> = if hole_count > 0 && !j_strs[1].is_null() {
                unsafe {
                    std::slice::from_raw_parts(j_strs[1] as *const NzbgetRsByteRange, hole_count)
                        .to_vec()
                }
            } else {
                Vec::new()
            };
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
                positional_rank: -1,
                positional_window: 0,
                steps_hash,
            });
        }

        if mark_live_attempted {
            for target in &mut targets {
                for (j_idx, &jf_id) in job_file_ids.iter().enumerate() {
                    if jf_id == target.file_id {
                        let was_attempted = unsafe {
                            cb(
                                ctx,
                                9,
                                j_idx as c_int,
                                1,
                                nzb_ptr,
                                0,
                                ptr::null(),
                                ptr::null_mut(),
                                ptr::null_mut(),
                                ptr::null_mut(),
                            ) != 0
                        };
                        target.patch_eligible = !was_attempted;
                        break;
                    }
                }
            }
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
        for (src_kind, count) in [(0 as c_int, queue_count), (1 as c_int, history_count)] {
            for idx in 0..count {
                let mut c_ints = [0 as c_int; 3];
                let mut c_i64s = [0i64; 1];
                let mut c_strs = [ptr::null::<c_char>(); 4];
                let cand_ptr = unsafe {
                    cb(
                        ctx,
                        10,
                        src_kind,
                        idx as c_int,
                        dq_ptr,
                        0,
                        ptr::null(),
                        c_ints.as_mut_ptr(),
                        c_i64s.as_mut_ptr(),
                        c_strs.as_mut_ptr(),
                    )
                };
                if cand_ptr == 0 || (src_kind == 0 && cand_ptr == own_nzb_ptr) {
                    continue;
                }
                let dupe_mode = c_ints[2];
                if dupe_mode == DM_FORCE {
                    continue;
                }
                let cand_name = unsafe { ptr_to_bytes(c_strs[0]) };
                let cand_key = unsafe { ptr_to_bytes(c_strs[1]) };
                if !same_name_or_key(&cand_name, &cand_key, &nzb_name, &own_dupe_key) {
                    continue;
                }
                cand_rows.push(CandRow {
                    dupe_score: c_ints[1],
                    id: c_ints[0],
                    full_hash: c_i64s[0],
                    queued_filename: unsafe { ptr_to_bytes(c_strs[2]) },
                    info_name: cand_name,
                    password: unsafe { ptr_to_bytes(c_strs[3]) },
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

        Some((nzb_name, dest_dir, targets, donors, member_names))
    }

    unsafe fn refresh_live_names(
        &self,
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
        targets: &mut [RepairTarget],
        member_names: &mut Vec<Vec<u8>>,
    ) {
        unsafe {
            with_dq_guard(ctx, cb, |dq_ptr| {
                let mut ints = [0 as c_int; 6];
                let mut i64s = [0i64; 1];
                let mut strs = [ptr::null::<c_char>(); 4];
                let nzb_ptr = cb(
                    ctx,
                    7,
                    0,
                    0,
                    dq_ptr,
                    0,
                    ptr::null(),
                    ints.as_mut_ptr(),
                    i64s.as_mut_ptr(),
                    strs.as_mut_ptr(),
                );
                if nzb_ptr == 0 {
                    return;
                }
                let completed_count = ints[2].max(0) as usize;
                let mut completed_files: Vec<(i32, bool, Vec<u8>)> =
                    Vec::with_capacity(completed_count);
                for idx in 0..completed_count {
                    let mut cf_ints = [0 as c_int; 3];
                    let mut cf_strs = [ptr::null::<c_char>(); 1];
                    cb(
                        ctx,
                        8,
                        idx as c_int,
                        0,
                        nzb_ptr,
                        0,
                        ptr::null(),
                        cf_ints.as_mut_ptr(),
                        ptr::null_mut(),
                        cf_strs.as_mut_ptr(),
                    );
                    completed_files.push((cf_ints[0], cf_ints[1] != 0, ptr_to_bytes(cf_strs[0])));
                }
                for target in targets.iter_mut() {
                    for (cf_id, cf_par, cf_name) in &completed_files {
                        if *cf_id == target.file_id {
                            target.filename = cf_name.clone();
                            target.is_par_file = target.is_par_file || *cf_par;
                            break;
                        }
                    }
                }
                member_names.clear();
                for (_, _, cf_name) in completed_files {
                    member_names.push(cf_name);
                }
            });
        }
    }

    unsafe fn refresh_live_target_name(
        &self,
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
        file_id: i32,
        cur_filename: &mut Vec<u8>,
    ) {
        unsafe {
            with_dq_guard(ctx, cb, |dq_ptr| {
                let mut ints = [0 as c_int; 6];
                let mut i64s = [0i64; 1];
                let mut strs = [ptr::null::<c_char>(); 4];
                let nzb_ptr = cb(
                    ctx,
                    7,
                    0,
                    0,
                    dq_ptr,
                    0,
                    ptr::null(),
                    ints.as_mut_ptr(),
                    i64s.as_mut_ptr(),
                    strs.as_mut_ptr(),
                );
                if nzb_ptr == 0 {
                    return;
                }
                let completed_count = ints[2].max(0) as usize;
                for idx in 0..completed_count {
                    let mut cf_ints = [0 as c_int; 3];
                    let mut cf_strs = [ptr::null::<c_char>(); 1];
                    cb(
                        ctx,
                        8,
                        idx as c_int,
                        0,
                        nzb_ptr,
                        0,
                        ptr::null(),
                        cf_ints.as_mut_ptr(),
                        ptr::null_mut(),
                        cf_strs.as_mut_ptr(),
                    );
                    if cf_ints[0] == file_id {
                        *cur_filename = ptr_to_bytes(cf_strs[0]);
                        break;
                    }
                }
            });
        }
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
            let full_c = to_c_bytes(&full_path);
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
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
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
        let ctx_addr = ctx as usize;

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
                unsafe {
                    cb(
                        ctx_addr as *mut c_void,
                        2,
                        0,
                        0,
                        0,
                        0,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            }
        }))
    }

    unsafe fn stop_watchdog(
        &self,
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
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
            unsafe { self.print_message(ctx, cb, MK_WARNING, &msg) };
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

    unsafe fn donor_dead(
        &self,
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
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
        if sample_cnt >= MIN_ARTICLES && unsafe { !self.host_is_stopped(ctx, cb) } {
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
                unsafe { self.print_message(ctx, cb, MK_INFO, &msg) };
            }
        }

        counters
            .donor_dead
            .insert(donor.queued_filename.clone(), dead);
        dead
    }

    unsafe fn parse_donor(
        &self,
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
        queued_filename: &[u8],
        par_ignore_ext: &[u8],
    ) -> Option<DonorParsedNzb> {
        parse_donor_nzb_file(queued_filename, par_ignore_ext, |msg| unsafe {
            self.global_log(ctx, cb, 3, msg);
        })
    }

    unsafe fn exec_repair(
        &self,
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
        opts: &HostOptions,
        dest_dir: &[u8],
        targets: &mut [RepairTarget],
        donors: &[DonorSource],
        member_names: &[Vec<u8>],
        counters: &mut RunCounters,
    ) {
        self.batch_fetcher.set_worker_count(if self.live_mode {
            0
        } else {
            stream_fetch_worker_count(opts.primary_connections)
        });
        let mut tried_postings: BTreeSet<Vec<u8>> = BTreeSet::new();

        for donor in donors {
            if unsafe { self.host_is_stopped(ctx, cb) } {
                break;
            }
            let any_holes = targets.iter().any(|t| t.patch_eligible && t.needs_repair());
            if !any_holes {
                break;
            }

            let Some(donor_nzb) = (unsafe {
                self.parse_donor(ctx, cb, &donor.queued_filename, &opts.par_ignore_ext)
            }) else {
                continue;
            };
            let donor_key = Self::build_parsed_donor_key(&donor_nzb, &donor.password);
            if !tried_postings.insert(donor_key) {
                let mut msg = b"Skipping another copy of duplicate posting ".to_vec();
                msg.extend_from_slice(&donor.info_name);
                unsafe { self.print_message(ctx, cb, MK_INFO, &msg) };
                continue;
            }
            if unsafe { self.donor_dead(ctx, cb, donor, &donor_nzb, counters) } {
                continue;
            }

            if !self.live_mode {
                let mut label = b"Repairing from duplicate ".to_vec();
                label.extend_from_slice(&donor.info_name);
                unsafe { self.set_post_progress(ctx, cb, &label, 0) };
            }

            let member_name_refs: Vec<&[u8]> = member_names.iter().map(|m| m.as_slice()).collect();
            let mut m1_targets: Vec<M1RepairTarget> = targets
                .iter()
                .map(|t| M1RepairTarget {
                    filename: t.filename.clone(),
                    decoded_file_size: t.decoded_file_size,
                    holes: t.holes.clone(),
                    encoded_size: t.encoded_size,
                    article_count: t.article_count,
                    recreated: t.recreated,
                    donor_set: Vec::new(),
                    patch_eligible: t.patch_eligible,
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
                ctx: *mut c_void,
                cb: NzbgetRsStreamRepairHostFn,
                donor_files: &'a [DonorParsedFile],
                target_file_ids: &'a [i32],
                live_filenames: &'a mut [Vec<u8>],
                recovered_bytes: &'a mut i64,
            }

            impl M1DonorEnv for NativeM1Env<'_> {
                fn is_stopped(&self) -> bool {
                    unsafe { self.ctrl.host_is_stopped(self.ctx, self.cb) }
                }

                fn refresh_live_target_name(&mut self, target_index: usize) -> Option<Vec<u8>> {
                    if target_index >= self.target_file_ids.len() {
                        return None;
                    }
                    unsafe {
                        self.ctrl.refresh_live_target_name(
                            self.ctx,
                            self.cb,
                            self.target_file_ids[target_index],
                            &mut self.live_filenames[target_index],
                        );
                    }
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
                    unsafe {
                        self.ctrl
                            .print_message(self.ctx, self.cb, map_target_msg_kind(kind), text);
                    }
                }
            }

            let mut env = NativeM1Env {
                ctrl: self,
                ctx,
                cb,
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

    unsafe fn exec_cross_pack_repair(
        &self,
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
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
                patch_eligible: t.patch_eligible,
                holes: t.holes.clone(),
            })
            .collect();
        let pw =
            (!counters.target_password.is_empty()).then_some(counters.target_password.as_slice());

        let aborted = || unsafe { self.host_is_stopped(ctx, cb) };
        let mut emit_msg = |kind: TargetMessageKind, text: &[u8]| unsafe {
            self.print_message(ctx, cb, map_target_msg_kind(kind), text);
        };
        let rar_vol_cb = |member_name: &[u8],
                          vol: &crate::rarreader::Volume,
                          files: &[crate::rarreader::RarFile]| {
            for line in format_rar_volume_debug_lines(member_name, vol, files) {
                unsafe { self.global_log(ctx, cb, 4, &line) };
            }
        };
        let rar_vol_ref: Option<
            &dyn Fn(&[u8], &crate::rarreader::Volume, &[crate::rarreader::RarFile]),
        > = self.debug_enabled.then_some(
            &rar_vol_cb as &dyn Fn(&[u8], &crate::rarreader::Volume, &[crate::rarreader::RarFile]),
        );

        let Some(mut session) = CrossPackRepairSession::new(
            dest_dir,
            &member_name_refs,
            &target_inputs,
            pw,
            self.tls_enabled,
            Some(&aborted),
            &mut emit_msg,
            rar_vol_ref,
        ) else {
            return;
        };

        let mut tried_postings: BTreeSet<Vec<u8>> = BTreeSet::new();
        let allow_decompress = !self.live_mode && opts.allow_decompress_opt;

        for donor in donors {
            if unsafe { self.host_is_stopped(ctx, cb) } || !session.has_inner_holes() {
                break;
            }

            let Some(donor_nzb) = (unsafe {
                self.parse_donor(ctx, cb, &donor.queued_filename, &opts.par_ignore_ext)
            }) else {
                continue;
            };
            let donor_key = Self::build_parsed_donor_key(&donor_nzb, &donor.password);
            if !tried_postings.insert(donor_key) {
                let mut msg = b"Skipping another copy of duplicate posting ".to_vec();
                msg.extend_from_slice(&donor.info_name);
                unsafe { self.print_message(ctx, cb, MK_INFO, &msg) };
                continue;
            }
            if unsafe { self.donor_dead(ctx, cb, donor, &donor_nzb, counters) } {
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
                ctx: *mut c_void,
                cb: NzbgetRsStreamRepairHostFn,
                donor_files: &'a [DonorParsedFile],
                targets: &'a mut [RepairTarget],
                recovered_bytes: &'a mut i64,
                recovered_holes: &'a mut i32,
            }

            impl CrossPackDonorEnv for NativeCrossPackEnv<'_> {
                fn is_stopped(&mut self) -> bool {
                    unsafe { self.ctrl.host_is_stopped(self.ctx, self.cb) }
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
                    let fetched = self.ctrl.fetcher.fetch(msg_id, &df.groups);
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
                    self.ctrl.batch_fetcher.begin(requests);
                }

                fn next_donor_prefetch(&mut self) -> Option<FetchedDonorArticle> {
                    let fetched = self.ctrl.batch_fetcher.next()?;
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
                        unsafe {
                            self.ctrl
                                .set_post_progress(self.ctx, self.cb, label, stage_progress);
                        }
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
                    unsafe {
                        self.ctrl
                            .print_message(self.ctx, self.cb, map_target_msg_kind(kind), text);
                    }
                }

                fn emit_extractor_log(&mut self, kind: i32, msg: &str) {
                    let log_kind = match kind {
                        0 => 0,
                        1 => 1,
                        2 => 2,
                        _ => 3,
                    };
                    unsafe {
                        self.ctrl
                            .global_log(self.ctx, self.cb, log_kind, msg.as_bytes());
                    }
                }

                fn on_rar_volume_parsed(
                    &mut self,
                    member_name: &[u8],
                    vol: &crate::rarreader::Volume,
                    files: &[crate::rarreader::RarFile],
                ) {
                    if self.ctrl.debug_enabled {
                        for line in format_rar_volume_debug_lines(member_name, vol, files) {
                            unsafe { self.ctrl.global_log(self.ctx, self.cb, 4, &line) };
                        }
                    }
                }
            }

            let mut env = NativeCrossPackEnv {
                ctrl: self,
                ctx,
                cb,
                donor_files: &donor_nzb.files,
                targets,
                recovered_bytes: &mut counters.recovered_bytes,
                recovered_holes: &mut counters.recovered_holes,
            };

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
        }
    }

    unsafe fn report_remaining_holes(
        &self,
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
        targets: &[RepairTarget],
        counters: &mut RunCounters,
    ) {
        let interrupted = unsafe { self.interrupted(ctx, cb) };
        if interrupted {
            let unfinished = targets
                .iter()
                .filter(|t| t.decoded_file_size <= 0 || !t.holes.is_empty())
                .count() as i32;
            counters.holes_remain |= unfinished > 0;
            let msg =
                format!("Stream repair interrupted with {unfinished} file(s) still incomplete");
            unsafe { self.print_message(ctx, cb, MK_INFO, msg.as_bytes()) };
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
                unsafe { self.print_message(ctx, cb, MK_INFO, &msg) };
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
                unsafe { self.print_message(ctx, cb, MK_INFO, &msg) };

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
                    unsafe { self.print_message(ctx, cb, MK_WARNING, &warn_msg) };
                }
            }
        }
    }

    unsafe fn repair_completed_post(
        &self,
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
        opts: &HostOptions,
        dest_dir: &[u8],
        targets: &[RepairTarget],
        counters: &RunCounters,
    ) {
        let interrupted = unsafe { self.interrupted(ctx, cb) };
        unsafe {
            with_dq_guard(ctx, cb, |dq_ptr| {
                let mut ints = [0 as c_int; 6];
                let mut i64s = [0i64; 1];
                let mut strs = [ptr::null::<c_char>(); 4];
                let nzb_ptr = cb(
                    ctx,
                    7,
                    0,
                    0,
                    dq_ptr,
                    0,
                    ptr::null(),
                    ints.as_mut_ptr(),
                    i64s.as_mut_ptr(),
                    strs.as_mut_ptr(),
                );
                if nzb_ptr == 0 {
                    return;
                }
                let mut job_count = ints[3].max(0) as usize;

                for target in targets {
                    if target.decoded_file_size > 0 && target.holes.is_empty() {
                        cb(
                            ctx,
                            12,
                            target.file_id,
                            1,
                            nzb_ptr,
                            0,
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                    } else if target.recreated {
                        cb(
                            ctx,
                            12,
                            target.file_id,
                            2,
                            nzb_ptr,
                            0,
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                    }
                }

                let mut job_idx = 0usize;
                while job_idx < job_count {
                    if !interrupted {
                        cb(
                            ctx,
                            13,
                            job_idx as c_int,
                            0,
                            nzb_ptr,
                            0,
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                        job_count -= 1;
                        continue;
                    }

                    let mut j_ints = [0 as c_int; 5];
                    let mut j_i64s = [0i64; 4];
                    let mut j_strs = [ptr::null::<c_char>(); 2];
                    cb(
                        ctx,
                        9,
                        job_idx as c_int,
                        0,
                        nzb_ptr,
                        0,
                        ptr::null(),
                        j_ints.as_mut_ptr(),
                        j_i64s.as_mut_ptr(),
                        j_strs.as_mut_ptr(),
                    );
                    let jf_id = j_ints[0];
                    let j_dec_size = j_i64s[0];

                    let mut erased = false;
                    for target in targets {
                        if target.file_id == jf_id {
                            if target.holes.is_empty() && target.decoded_file_size > 0 {
                                cb(
                                    ctx,
                                    13,
                                    job_idx as c_int,
                                    0,
                                    nzb_ptr,
                                    0,
                                    ptr::null(),
                                    ptr::null_mut(),
                                    ptr::null_mut(),
                                    ptr::null_mut(),
                                );
                                job_count -= 1;
                                erased = true;
                            } else {
                                let new_dec = if j_dec_size <= 0 && target.decoded_file_size > 0 {
                                    target.decoded_file_size
                                } else {
                                    -1
                                };
                                let mut upd_ints = [target.holes.len() as c_int];
                                let mut upd_i64s = [new_dec];
                                cb(
                                    ctx,
                                    13,
                                    job_idx as c_int,
                                    1,
                                    nzb_ptr,
                                    if target.holes.is_empty() {
                                        0
                                    } else {
                                        target.holes.as_ptr() as usize
                                    },
                                    ptr::null(),
                                    upd_ints.as_mut_ptr(),
                                    upd_i64s.as_mut_ptr(),
                                    ptr::null_mut(),
                                );
                            }
                            break;
                        }
                    }
                    if !erased {
                        job_idx += 1;
                    }
                }

                let mut st_ints = [0 as c_int; 2];
                let mut sz_i64s = [0i64; 8];
                cb(
                    ctx,
                    14,
                    0,
                    0,
                    dq_ptr,
                    nzb_ptr,
                    ptr::null(),
                    st_ints.as_mut_ptr(),
                    sz_i64s.as_mut_ptr(),
                    ptr::null_mut(),
                );

                let write_sizes = counters.recovered_failed_size > 0;
                let mut out_i64s = [0i64; 9];
                if write_sizes {
                    let (f, s, cf, cs, pf, ps, pcf, pcs) = credit_repaired_sizes(
                        counters.recovered_failed_size,
                        counters.recovered_failed_par_size,
                        sz_i64s[0],
                        sz_i64s[1],
                        sz_i64s[2],
                        sz_i64s[3],
                        sz_i64s[4],
                        sz_i64s[5],
                        sz_i64s[6],
                        sz_i64s[7],
                    );
                    out_i64s[0] = f;
                    out_i64s[1] = s;
                    out_i64s[2] = cf;
                    out_i64s[3] = cs;
                    out_i64s[4] = pf;
                    out_i64s[5] = ps;
                    out_i64s[6] = pcf;
                    out_i64s[7] = pcs;
                }
                out_i64s[8] = counters.recovered_bytes;

                let (
                    reset_par_rename,
                    set_par_status,
                    new_par_status,
                    set_force_par_full,
                    clear_req_par_check,
                    check_main_pars,
                ) = resolve_completion_par_action(
                    counters.recovered_bytes,
                    counters.holes_remain,
                    st_ints[0],
                    RS_NONE,
                    st_ints[1],
                    PS_SKIPPED,
                    opts.par_check,
                    opts.pc_manual,
                    PS_MANUAL,
                    PS_NONE,
                );
                let mut par_flags: c_int = 0;
                if reset_par_rename {
                    par_flags |= 1;
                }
                if set_par_status {
                    par_flags |= 2;
                }
                if set_force_par_full {
                    par_flags |= 4;
                }
                if clear_req_par_check {
                    par_flags |= 8;
                }
                if check_main_pars
                    && crate::parparser::find_main_pars(crate::fsops::path(dest_dir), false, |_| {})
                {
                    par_flags |= 16;
                }

                let mut fin_ints = [
                    counters.recovered_articles,
                    counters.recovered_holes,
                    par_flags,
                    new_par_status,
                ];
                cb(
                    ctx,
                    14,
                    c_int::from(write_sizes),
                    1,
                    dq_ptr,
                    nzb_ptr,
                    ptr::null(),
                    fin_ints.as_mut_ptr(),
                    out_i64s.as_mut_ptr(),
                    ptr::null_mut(),
                );
            });
        }
    }

    unsafe fn repair_completed_live(
        &self,
        ctx: *mut c_void,
        cb: NzbgetRsStreamRepairHostFn,
        targets: &[RepairTarget],
        counters: &RunCounters,
    ) {
        unsafe {
            with_dq_guard(ctx, cb, |dq_ptr| {
                let mut ints = [0, counters.recovered_holes];
                let mut i64s = [counters.recovered_bytes];
                let nzb_ptr = cb(
                    ctx,
                    15,
                    0,
                    0,
                    dq_ptr,
                    0,
                    ptr::null(),
                    ints.as_mut_ptr(),
                    i64s.as_mut_ptr(),
                    ptr::null_mut(),
                );
                if nzb_ptr == 0 {
                    return;
                }
                let job_count = ints[0].max(0) as usize;
                let mut job_ids = Vec::with_capacity(job_count);
                for j_idx in 0..job_count {
                    let mut j_ints = [0 as c_int; 5];
                    let mut j_i64s = [0i64; 4];
                    let mut j_strs = [ptr::null::<c_char>(); 2];
                    cb(
                        ctx,
                        9,
                        j_idx as c_int,
                        0,
                        nzb_ptr,
                        0,
                        ptr::null(),
                        j_ints.as_mut_ptr(),
                        j_i64s.as_mut_ptr(),
                        j_strs.as_mut_ptr(),
                    );
                    job_ids.push(j_ints[0]);
                }

                for target in targets {
                    if !target.patch_eligible {
                        continue;
                    }
                    for (j_idx, &jf_id) in job_ids.iter().enumerate() {
                        if jf_id == target.file_id {
                            let new_dec = if target.recreated {
                                cb(
                                    ctx,
                                    12,
                                    target.file_id,
                                    2,
                                    nzb_ptr,
                                    0,
                                    ptr::null(),
                                    ptr::null_mut(),
                                    ptr::null_mut(),
                                    ptr::null_mut(),
                                );
                                target.decoded_file_size
                            } else {
                                -1
                            };
                            let mut upd_ints = [target.holes.len() as c_int];
                            let mut upd_i64s = [new_dec];
                            cb(
                                ctx,
                                13,
                                j_idx as c_int,
                                1,
                                nzb_ptr,
                                if target.holes.is_empty() {
                                    0
                                } else {
                                    target.holes.as_ptr() as usize
                                },
                                ptr::null(),
                                upd_ints.as_mut_ptr(),
                                upd_i64s.as_mut_ptr(),
                                ptr::null_mut(),
                            );
                            break;
                        }
                    }
                }

                cb(
                    ctx,
                    15,
                    0,
                    1,
                    dq_ptr,
                    nzb_ptr,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            });
        }
    }

    pub unsafe fn run(&self, ctx: *mut c_void, cb: NzbgetRsStreamRepairHostFn) {
        if self.live_mode {
            unsafe { self.run_live(ctx, cb) };
        } else {
            unsafe { self.run_post(ctx, cb) };
        }
    }

    unsafe fn run_post(&self, ctx: *mut c_void, cb: NzbgetRsStreamRepairHostFn) {
        let mut counters = RunCounters::default();
        let Some((nzb_name, dest_dir, mut targets, donors, member_names)) = (unsafe {
            with_dq_guard(ctx, cb, |dq_ptr| {
                self.collect_targets_and_donors(ctx, cb, dq_ptr, true, false, &mut counters)
            })
        }) else {
            return;
        };

        let mut info_name = b"stream repair for ".to_vec();
        info_name.extend_from_slice(&nzb_name);
        unsafe { self.set_info_name(ctx, cb, &info_name) };

        let opts = unsafe { self.query_host_options(ctx, cb) };

        if targets.is_empty() {
            let mut msg = b"Nothing to stream-repair for ".to_vec();
            msg.extend_from_slice(&nzb_name);
            unsafe { self.print_message(ctx, cb, MK_INFO, &msg) };
        } else if donors.is_empty() {
            let mut msg = b"No duplicate collections found for stream repair of ".to_vec();
            msg.extend_from_slice(&nzb_name);
            unsafe { self.print_message(ctx, cb, MK_INFO, &msg) };
        } else {
            let wd_handle = self.start_watchdog(ctx, cb, &opts, &targets);
            self.compute_positional_ranks(&dest_dir, &mut targets, &member_names);
            unsafe {
                self.exec_repair(
                    ctx,
                    cb,
                    &opts,
                    &dest_dir,
                    &mut targets,
                    &donors,
                    &member_names,
                    &mut counters,
                );
            }
            let any_holes = targets.iter().any(|t| t.needs_repair());
            if any_holes && unsafe { !self.host_is_stopped(ctx, cb) } {
                unsafe {
                    self.exec_cross_pack_repair(
                        ctx,
                        cb,
                        &opts,
                        &dest_dir,
                        &mut targets,
                        &donors,
                        &member_names,
                        &mut counters,
                    );
                }
            }
            unsafe {
                self.stop_watchdog(
                    ctx,
                    cb,
                    wd_handle,
                    &nzb_name,
                    &targets,
                    counters.recovered_bytes,
                );
            }
        }

        unsafe { self.report_remaining_holes(ctx, cb, &targets, &mut counters) };
        unsafe {
            self.repair_completed_post(ctx, cb, &opts, &dest_dir, &targets, &counters);
        }
    }

    unsafe fn run_live(&self, ctx: *mut c_void, cb: NzbgetRsStreamRepairHostFn) {
        let mut counters = RunCounters::default();
        let Some((nzb_name, dest_dir, mut targets, donors, mut member_names)) = (unsafe {
            with_dq_guard(ctx, cb, |dq_ptr| {
                self.collect_targets_and_donors(ctx, cb, dq_ptr, false, true, &mut counters)
            })
        }) else {
            return;
        };

        let mut info_name = b"live stream repair for ".to_vec();
        info_name.extend_from_slice(&nzb_name);
        unsafe { self.set_info_name(ctx, cb, &info_name) };

        let any_eligible = targets.iter().any(|t| t.patch_eligible && t.needs_repair());
        if any_eligible && !donors.is_empty() {
            let mut msg = b"Starting live stream repair for ".to_vec();
            msg.extend_from_slice(&nzb_name);
            unsafe { self.print_message(ctx, cb, MK_INFO, &msg) };

            let opts = unsafe { self.query_host_options(ctx, cb) };
            let wd_handle = self.start_watchdog(ctx, cb, &opts, &targets);
            self.compute_positional_ranks(&dest_dir, &mut targets, &member_names);
            unsafe {
                self.exec_repair(
                    ctx,
                    cb,
                    &opts,
                    &dest_dir,
                    &mut targets,
                    &donors,
                    &member_names,
                    &mut counters,
                );
            }
            let any_holes = targets.iter().any(|t| t.patch_eligible && t.needs_repair());
            if any_holes && unsafe { !self.host_is_stopped(ctx, cb) } {
                unsafe {
                    self.refresh_live_names(ctx, cb, &mut targets, &mut member_names);
                    self.exec_cross_pack_repair(
                        ctx,
                        cb,
                        &opts,
                        &dest_dir,
                        &mut targets,
                        &donors,
                        &member_names,
                        &mut counters,
                    );
                }
            }
            unsafe {
                self.stop_watchdog(
                    ctx,
                    cb,
                    wd_handle,
                    &nzb_name,
                    &targets,
                    counters.recovered_bytes,
                );
            }
        }

        unsafe { self.repair_completed_live(ctx, cb, &targets, &counters) };
    }
}

pub unsafe fn stream_repair_ctrl_exec(
    ctrl: *const NzbgetRsStreamRepairCtrl,
    op: c_int,
    host_cb: Option<NzbgetRsStreamRepairHostFn>,
    ctx: *mut c_void,
) -> c_int {
    let Some(c) = (unsafe { ctrl.as_ref() }) else {
        return 0;
    };
    match op {
        0 => {
            c.stop();
            0
        }
        1 => {
            if let Some(cb) = host_cb {
                unsafe { c.run(ctx, cb) };
            }
            0
        }
        _ => 0,
    }
}
