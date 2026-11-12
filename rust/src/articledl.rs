//! Full Rust runtime backing `MemoryMonitor`, `ArticleCache`,
//! `ArticleWriter`, `RenameContentAnalyzer`, and `ArticleDownloader`.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::{Seek, SeekFrom, Write};
use std::os::fd::{FromRawFd, IntoRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

use crate::artwritercore::{
    article_cache_should_flush, article_dl_format_connection_name, article_dl_format_fetch_command,
    article_writer_advance_and_check_range, article_writer_build_output_filenames,
    article_writer_classify_completion_status, article_writer_create_output_file,
    article_writer_execute_cleanup_old_data, article_writer_execute_setup_output_file,
    article_writer_finish_disk_ops, article_writer_flush_cache,
    article_writer_format_dest_file_path, article_writer_format_info_path,
    article_writer_move_completed_files, article_writer_post_process_rename_and_hardlink,
    article_writer_process_articles, article_writer_resolve_direct_write_ready,
    calc_write_buffer_size, join_c_slices, memory_monitor_step, ArticleWriterFlushItem,
    ArticleWriterProcessItem, PRESSURE_CONSTRAINED, PRESSURE_CRITICAL, PRESSURE_NORMAL,
};
use crate::decoder::{Decoder, Format};
use crate::dupefallback::{article_downloader_check_response, article_downloader_validate_yenc};
use crate::nntp::{
    article_dl_all_servers_on_level_failed, article_dl_classify_attempt_stat,
    article_dl_classify_decode_status, article_dl_classify_write_failure,
    article_dl_eval_level_exhaustion, article_dl_handle_content_rejected,
    article_dl_is_join_group_ok, article_dl_is_retention_failure,
    article_dl_normalize_connect_and_decrement_retries, article_dl_resolve_final_status,
    article_dl_should_advance_server_list, TransientRetries,
};
use crate::queuesubsys_host::{
    NzbgetRsQueueSubsysHostFn, AD_FAILED, AD_FATAL_ERROR, AD_FINISHED, AD_RETRY, AD_RUNNING,
    MK_DEBUG, MK_DETAIL, MK_ERROR, MK_INFO, MK_WARNING,
};
use crate::renameanalyzer::Analyzer;

const AD_WAITING: i32 = 2;
const AD_CRC_ERROR: i32 = 6;
const AD_NOT_FOUND: i32 = 7;
const AD_CONNECT_ERROR: i32 = 8;
const AD_TRANSIENT_ERROR: i32 = 9;
const DM_FORCE: i32 = 2;

#[inline]
fn now_sec() -> i64 {
    #[cfg(unix)]
    unsafe {
        libc::time(std::ptr::null_mut()) as i64
    }
    #[cfg(not(unix))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64)
    }
}

#[inline]
fn sleep_ms(ms: i32) {
    if ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }
}

fn c_bytes(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    &bytes[..end]
}

fn nul_vec(bytes: &[u8]) -> Vec<u8> {
    let clean = c_bytes(bytes);
    let mut v = Vec::with_capacity(clean.len() + 1);
    v.extend_from_slice(clean);
    v.push(0);
    v
}

unsafe fn opt_cstr(p: *const c_char) -> Option<Vec<u8>> {
    if p.is_null() {
        None
    } else {
        Some(CStr::from_ptr(p).to_bytes().to_vec())
    }
}

unsafe fn cstr_or_empty(p: *const c_char) -> Vec<u8> {
    if p.is_null() {
        Vec::new()
    } else {
        CStr::from_ptr(p).to_bytes().to_vec()
    }
}

struct DlHost {
    cb: NzbgetRsQueueSubsysHostFn,
    ctx: *mut c_void,
}

impl DlHost {
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn raw(
        &self,
        op: c_int,
        a1: c_int,
        a2: c_int,
        p1: usize,
        p2: usize,
        s1: *const c_char,
        s2: *const c_char,
        ints: *mut c_int,
        i64s: *mut i64,
        strs: *mut *const c_char,
    ) -> usize {
        unsafe { (self.cb)(self.ctx, op, a1, a2, p1, p2, s1, s2, ints, i64s, strs) }
    }

    #[inline]
    fn log(&self, kind: i32, msg: &[u8]) {
        crate::log::log_active_bytes(kind, msg);
    }

    fn log_nzb(&self, nzb_ptr: usize, kind: i32, msg: &[u8]) {
        if nzb_ptr == 0 {
            crate::log::log_active_bytes(kind, msg);
            return;
        }
        let c = nul_vec(msg);
        self.raw(
            1,
            kind,
            0,
            nzb_ptr,
            0,
            c.as_ptr().cast(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
}

// ============================================================================
// MemoryMonitor + ArticleCache global runtime state
// ============================================================================

static MEM_ACTIVE: AtomicBool = AtomicBool::new(false);
static MEM_TIER: AtomicI32 = AtomicI32::new(PRESSURE_NORMAL);
static MEM_EFFECTIVE_CACHE_MB: AtomicU32 = AtomicU32::new(0);
static MEM_HYST: Mutex<(i32, i32)> = Mutex::new((PRESSURE_NORMAL, 0));

static CACHE_ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static CACHE_FILE_INFO: AtomicUsize = AtomicUsize::new(0);
static CACHE_FLUSHING: AtomicBool = AtomicBool::new(false);
static CACHE_ALLOC_LOCK: (Mutex<()>, Condvar) = (Mutex::new(()), Condvar::new());
static CACHE_FLUSH_LOCK: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

pub(crate) fn article_cache_allocated() -> usize {
    CACHE_ALLOCATED.load(Ordering::Acquire)
}

pub(crate) fn memory_monitor_active() -> bool {
    MEM_ACTIVE.load(Ordering::Acquire)
}

pub fn memory_monitor_tier() -> i32 {
    if MEM_ACTIVE.load(Ordering::Acquire) {
        MEM_TIER.load(Ordering::Acquire)
    } else {
        PRESSURE_NORMAL
    }
}

pub fn memory_monitor_tier_name_c() -> *const c_char {
    match memory_monitor_tier() {
        PRESSURE_CRITICAL => c"CRITICAL".as_ptr(),
        PRESSURE_CONSTRAINED => c"CONSTRAINED".as_ptr(),
        _ => c"NORMAL".as_ptr(),
    }
}

pub fn memory_monitor_effective_cache_mb() -> i32 {
    MEM_EFFECTIVE_CACHE_MB.load(Ordering::Acquire) as i32
}

pub(crate) fn cache_file_busy(file_ptr: usize) -> bool {
    file_ptr != 0 && file_ptr == CACHE_FILE_INFO.load(Ordering::SeqCst)
}

pub fn memory_monitor_effective_par_buffer_mb(configured_mb: i32) -> i32 {
    let tier = memory_monitor_tier();
    if tier == PRESSURE_CRITICAL {
        if configured_mb == 0 {
            0
        } else {
            configured_mb.min(16).max(8)
        }
    } else if tier == PRESSURE_NORMAL {
        configured_mb
    } else {
        let last_tier = crate::memory_pressure::get_last_tier();
        let headroom = crate::memory_pressure::get_last_headroom_mb();
        crate::memory_pressure::calculate_effective_par_buffer(
            configured_mb.max(0) as u32,
            last_tier,
            headroom,
        ) as i32
    }
}

pub fn memory_monitor_effective_downloads_limit(configured_limit: i32) -> i32 {
    match memory_monitor_tier() {
        PRESSURE_CRITICAL => (configured_limit / 4).max(1),
        PRESSURE_CONSTRAINED => (configured_limit / 2).max(2),
        _ => configured_limit,
    }
}

fn cache_wakeup_flush() {
    let _guard = CACHE_ALLOC_LOCK.0.lock().unwrap_or_else(|e| e.into_inner());
    CACHE_ALLOC_LOCK.1.notify_all();
}

fn cache_lock_flush() {
    let mut locked = CACHE_FLUSH_LOCK.0.lock().unwrap_or_else(|e| e.into_inner());
    while *locked {
        locked = CACHE_FLUSH_LOCK
            .1
            .wait(locked)
            .unwrap_or_else(|e| e.into_inner());
    }
    *locked = true;
    CACHE_FLUSHING.store(true, Ordering::Release);
}

fn cache_unlock_flush() {
    CACHE_FLUSHING.store(false, Ordering::Release);
    let mut locked = CACHE_FLUSH_LOCK.0.lock().unwrap_or_else(|e| e.into_inner());
    *locked = false;
    CACHE_FLUSH_LOCK.1.notify_one();
}

fn cache_effective_limit() -> usize {
    if MEM_ACTIVE.load(Ordering::Acquire) {
        (memory_monitor_effective_cache_mb().max(0) as usize) * 1024 * 1024
    } else {
        crate::options::active_options().map_or(0, |o| {
            (o.scalars().article_cache.max(0) as usize) * 1024 * 1024
        })
    }
}

fn write_acache_flag(qdir: &[u8]) {
    let flag_p =
        article_writer_format_dest_file_path(qdir, crate::paths::PATH_SEPARATOR, b"acache");
    if let Ok(flag_c) = CString::new(c_bytes(&flag_p)) {
        let fd = crate::fsops::open_cloexec_fd(&flag_c, 2, 0, 1, 2);
        if fd == -1 {
            crate::log::log_active_bytes(
                MK_ERROR,
                &join_c_slices(&[b"Error saving diskstate: Could not create file ", &flag_p]),
            );
        } else {
            unsafe {
                libc::close(fd);
            }
        }
    }
}

fn delete_acache_flag(qdir: &[u8]) {
    let mut o1 = 0;
    let mut o2 = 0;
    let _ = crate::diskstate_format::diskstate_fs_op(
        3,
        qdir,
        crate::paths::PATH_SEPARATOR,
        0,
        0,
        0,
        0,
        &mut o1,
        &mut o2,
    );
}

fn cache_alloc(size: i32) -> *mut u8 {
    if size <= 0 {
        return std::ptr::null_mut();
    }
    let _guard = CACHE_ALLOC_LOCK.0.lock().unwrap_or_else(|e| e.into_inner());
    let effective_limit = cache_effective_limit();
    let cur_alloc = CACHE_ALLOCATED.load(Ordering::SeqCst);
    let was_empty = cur_alloc == 0;
    let avail = effective_limit.saturating_sub(cur_alloc);
    let mut p: *mut u8 = std::ptr::null_mut();
    if effective_limit > 0 && (size as usize) <= avail {
        p = unsafe { libc::malloc(size as usize).cast::<u8>() };
        if !p.is_null() {
            if was_empty {
                if let Some(opts) = crate::options::active_options() {
                    if opts.server_mode()
                        && opts.scalars().continue_partial != 0
                        && opts.scalars().skip_write == 0
                    {
                        write_acache_flag(opts.str_bytes(6));
                    }
                }
                CACHE_ALLOC_LOCK.1.notify_all();
            }
            CACHE_ALLOCATED.fetch_add(size as usize, Ordering::SeqCst);
        }
    }
    p
}

fn cache_realloc(ptr: *mut u8, old_cap: i32, new_size: i32) -> (*mut u8, i32) {
    if ptr.is_null() || new_size <= 0 || new_size == old_cap {
        return (ptr, old_cap);
    }
    let _guard = CACHE_ALLOC_LOCK.0.lock().unwrap_or_else(|e| e.into_inner());
    let new_p = unsafe { libc::realloc(ptr.cast::<c_void>(), new_size as usize).cast::<u8>() };
    if !new_p.is_null() {
        let cur = CACHE_ALLOCATED.load(Ordering::SeqCst);
        let next = if new_size >= old_cap {
            cur.saturating_add((new_size - old_cap) as usize)
        } else {
            cur.saturating_sub((old_cap - new_size) as usize)
        };
        CACHE_ALLOCATED.store(next, Ordering::SeqCst);
        (new_p, new_size)
    } else {
        (ptr, old_cap)
    }
}

fn cache_free(ptr: *mut u8, cap: i32) {
    if cap > 0 && !ptr.is_null() {
        unsafe {
            libc::free(ptr.cast::<c_void>());
        }
        let _guard = CACHE_ALLOC_LOCK.0.lock().unwrap_or_else(|e| e.into_inner());
        let cur = CACHE_ALLOCATED.load(Ordering::SeqCst);
        let next = cur.saturating_sub(cap as usize);
        CACHE_ALLOCATED.store(next, Ordering::SeqCst);
        if next == 0 {
            if let Some(opts) = crate::options::active_options() {
                if opts.server_mode() && opts.scalars().continue_partial != 0 {
                    delete_acache_flag(opts.str_bytes(6));
                }
            }
        }
    }
}

fn memory_monitor_update_state() -> i32 {
    let (cfg_cache, interval) = match crate::options::active_options() {
        Some(opts) => (
            opts.scalars().article_cache.max(0) as u32,
            opts.scalars().memory_pressure_interval.max(100),
        ),
        None => (0u32, 500i32),
    };

    let metrics = crate::memory_pressure::sample_system_metrics(None);
    let raw_sample = crate::memory_pressure::evaluate_state(&metrics) as i32;
    let current_tier = MEM_TIER.load(Ordering::Relaxed);

    let (next_tier, transitioned) = {
        let mut hyst = MEM_HYST.lock().unwrap_or_else(|e| e.into_inner());
        let (mut cand, mut consec) = *hyst;
        let out = memory_monitor_step(raw_sample, current_tier, &mut cand, &mut consec);
        *hyst = (cand, consec);
        out
    };

    let eff_cache = if next_tier == PRESSURE_CRITICAL {
        0
    } else if next_tier == PRESSURE_NORMAL {
        cfg_cache
    } else if raw_sample == next_tier {
        let last_tier = crate::memory_pressure::get_last_tier();
        let headroom = crate::memory_pressure::get_last_headroom_mb();
        crate::memory_pressure::calculate_effective_cache_limit(cfg_cache, last_tier, headroom)
    } else {
        cfg_cache / 2
    };
    MEM_EFFECTIVE_CACHE_MB.store(eff_cache, Ordering::Release);

    if transitioned {
        MEM_TIER.store(next_tier, Ordering::Release);
        if next_tier == PRESSURE_CRITICAL {
            crate::log::log_active_bytes(
                MK_WARNING,
                b"System memory pressure CRITICAL (>=90%): shedding article cache and throttling downloads",
            );
            cache_wakeup_flush();
            crate::memory_pressure::trim_heap();
        } else if next_tier == PRESSURE_CONSTRAINED {
            crate::log::log_active_bytes(
                MK_DETAIL,
                b"System memory pressure CONSTRAINED: shrinking article cache ceiling",
            );
            cache_wakeup_flush();
        } else if next_tier == PRESSURE_NORMAL && current_tier != PRESSURE_NORMAL {
            crate::log::log_active_bytes(
                MK_DETAIL,
                b"System memory pressure returned to NORMAL: restoring cache and download capacity",
            );
            crate::memory_pressure::trim_heap();
        }
    }

    interval
}

fn memory_monitor_init_options(reset_state: bool) {
    MEM_ACTIVE.store(true, Ordering::Release);
    if reset_state {
        MEM_TIER.store(PRESSURE_NORMAL, Ordering::Release);
        MEM_EFFECTIVE_CACHE_MB.store(0, Ordering::Release);
        if let Ok(mut hyst) = MEM_HYST.lock() {
            *hyst = (PRESSURE_NORMAL, 0);
        }
    }
    if let Some(opts) = crate::options::active_options() {
        let path_opt = opts
            .str_cstr(36)
            .and_then(|c| c.to_str().ok())
            .filter(|s| !s.is_empty());
        crate::memory_pressure::set_mock_root(path_opt);
    }
    let _ = memory_monitor_update_state();
}

fn memory_monitor_run(host: &DlHost) {
    host.log(MK_DEBUG, b"MemoryMonitor thread started");
    let is_stopped = || {
        host.raw(
            2,
            1,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    };
    while !is_stopped() {
        let interval = memory_monitor_update_state();
        let mut slept = 0;
        while !is_stopped() && slept < interval {
            sleep_ms(50);
            slept += 50;
        }
    }
    host.log(MK_DEBUG, b"MemoryMonitor thread stopped");
}

// ============================================================================
// ArticleCache Run + CheckFlush + FlushCache
// ============================================================================

fn article_cache_check_flush(host: &DlHost, allocated: usize, flush_everything: bool) -> bool {
    host.log(
        MK_DEBUG,
        format!(
            "Checking cache, Allocated: {}, FlushEverything: {}",
            allocated as i32,
            i32::from(flush_everything)
        )
        .as_bytes(),
    );

    let mut strs = [std::ptr::null::<c_char>(); 2];
    let file_ptr = host.raw(
        2,
        5,
        i32::from(flush_everything),
        0,
        0,
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        strs.as_mut_ptr(),
    );
    if file_ptr == 0 {
        CACHE_FILE_INFO.store(0, Ordering::SeqCst);
        host.log(MK_DEBUG, b"Checking cache... nothing to flush");
        return false;
    }

    struct ClearBusyGuard;
    impl Drop for ClearBusyGuard {
        fn drop(&mut self) {
            CACHE_FILE_INFO.store(0, Ordering::SeqCst);
        }
    }
    // Selection callback (op 2, 5) publishes CACHE_FILE_INFO while holding DownloadQueue::Guard().
    debug_assert_eq!(CACHE_FILE_INFO.load(Ordering::SeqCst), file_ptr);
    let _busy_guard = ClearBusyGuard;
    let info_name = unsafe { cstr_or_empty(strs[0]) };
    host.log(
        MK_DETAIL,
        &join_c_slices(&[b"Flushing cache for ", &info_name]),
    );

    let mut ints = [0i32; 4];
    let mut i64s = [0i64; 2];
    let mut fstrs = [std::ptr::null::<c_char>(); 2];
    cache_lock_flush();
    let locked = host.raw(
        2,
        7,
        0,
        file_ptr,
        0,
        std::ptr::null(),
        std::ptr::null(),
        ints.as_mut_ptr(),
        i64s.as_mut_ptr(),
        fstrs.as_mut_ptr(),
    ) != 0;
    if !locked {
        cache_unlock_flush();
    }

    if locked {
        let direct_write = ints[0] != 0;
        let skip_disk_write = ints[1] != 0;
        let count = ints[2].max(0) as usize;
        let nzb_ptr = i64s[1] as usize;
        let output_filename = unsafe {
            if fstrs[0].is_null() || i64s[0] <= 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(fstrs[0].cast::<u8>(), i64s[0] as usize).to_vec()
            }
        };

        struct RawCachedMeta {
            offset: i64,
            size: i32,
            content_ptr: *const u8,
            result_filename: Option<Vec<u8>>,
        }
        let mut metas = Vec::with_capacity(count);
        for idx in 0..count {
            let mut aints = [0i32; 2];
            let mut ai64s = [0i64; 1];
            let mut astrs = [std::ptr::null::<c_char>(); 2];
            host.raw(
                2,
                8,
                idx as i32,
                file_ptr,
                0,
                std::ptr::null(),
                std::ptr::null(),
                aints.as_mut_ptr(),
                ai64s.as_mut_ptr(),
                astrs.as_mut_ptr(),
            );
            metas.push(RawCachedMeta {
                offset: ai64s[0],
                size: aints[0],
                content_ptr: astrs[0].cast::<u8>(),
                result_filename: unsafe { opt_cstr(astrs[1]) },
            });
        }

        let items: Vec<ArticleWriterFlushItem<'_>> = metas
            .iter()
            .map(|m| ArticleWriterFlushItem {
                segment_offset: m.offset,
                segment_size: m.size,
                segment_content: unsafe {
                    if m.content_ptr.is_null() || m.size <= 0 {
                        &[][..]
                    } else {
                        std::slice::from_raw_parts(m.content_ptr, m.size as usize)
                    }
                },
                result_filename: m.result_filename.as_deref(),
            })
            .collect();

        let (flushed_articles, flushed_size) = article_writer_flush_cache(
            &items,
            direct_write,
            skip_disk_write,
            &output_filename,
            || {
                host.raw(
                    2,
                    9,
                    -1,
                    file_ptr,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ) != 0
            },
            |idx| {
                host.raw(
                    2,
                    9,
                    idx as i32,
                    file_ptr,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            },
            |msg| host.log_nzb(nzb_ptr, MK_ERROR, msg),
        );

        let mut post_ints = [0i32; 1];
        host.raw(
            2,
            10,
            flushed_articles,
            file_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            post_ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        cache_unlock_flush();
        let skip_after = post_ints[0] != 0
            || crate::options::active_options().is_some_and(|o| o.scalars().skip_write != 0);
        let flushed_mb = (flushed_size as f64) / 1024.0 / 1024.0;
        let prefix = if skip_after {
            format!("Discarded {flushed_articles} articles ({flushed_mb:.2} MB) from cache (SkipWrite) for ")
        } else {
            format!(
                "Saved {flushed_articles} articles ({flushed_mb:.2} MB) from cache into disk for "
            )
        };
        host.log(MK_DETAIL, &join_c_slices(&[prefix.as_bytes(), &info_name]));
    }

    true
}

fn article_cache_run(host: &DlHost) {
    let is_stopped_cb = || {
        host.raw(
            2,
            3,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    };
    let snap = || {
        let allocated = CACHE_ALLOCATED.load(Ordering::SeqCst);
        let effective_limit = cache_effective_limit();
        let is_stopped = is_stopped_cb();
        let direct_write =
            crate::options::active_options().is_some_and(|o| o.scalars().direct_write != 0);
        let tier = memory_monitor_tier();
        (allocated, effective_limit, is_stopped, direct_write, tier)
    };

    let mut reset_counter = 0i32;
    let mut just_flushed = false;
    loop {
        let (allocated, effective_limit, is_stopped, direct_write, tier) = snap();
        if is_stopped && allocated == 0 {
            break;
        }
        let (should_flush, flush_everything, need_emergency_flush) = article_cache_should_flush(
            allocated,
            effective_limit,
            tier,
            just_flushed,
            reset_counter,
            is_stopped,
            direct_write,
        );
        if should_flush {
            just_flushed = article_cache_check_flush(host, allocated, flush_everything);
            reset_counter = 0;
            let (alloc_now, _, stopped_now, _, _) = snap();
            if need_emergency_flush && alloc_now == 0 {
                crate::memory_pressure::trim_heap();
            } else if !just_flushed && !stopped_now {
                sleep_ms(5);
            }
        } else if allocated == 0 {
            let mut guard = CACHE_ALLOC_LOCK.0.lock().unwrap_or_else(|e| e.into_inner());
            while CACHE_ALLOCATED.load(Ordering::SeqCst) == 0 && !is_stopped_cb() {
                guard = CACHE_ALLOC_LOCK
                    .1
                    .wait(guard)
                    .unwrap_or_else(|e| e.into_inner());
            }
            drop(guard);
            reset_counter = 0;
        } else {
            sleep_ms(5);
            reset_counter += 5;
        }
    }
}

// ============================================================================
// BufferedDiskFile (userspace write buffering over O_CLOEXEC fd)
// ============================================================================

struct BufferedDiskFile {
    file: Option<std::fs::File>,
    buf: Vec<u8>,
    cap: usize,
    pos: i64,
}

impl BufferedDiskFile {
    fn open(
        path: &[u8],
        read_write: bool,
        write_buffer_kb: i32,
        rec_size: i32,
    ) -> Result<Self, i32> {
        let clean = c_bytes(path);
        let Ok(c_path) = CString::new(clean) else {
            return Err(libc::EINVAL);
        };
        let mode = if read_write { 1 } else { 2 };
        let fd = crate::fsops::open_cloexec_fd(&c_path, mode, 0, 1, 2);
        if fd < 0 {
            return Err(crate::fsops::last_errno());
        }
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        let calc = calc_write_buffer_size(write_buffer_kb, rec_size);
        let cap = if calc > 0 { calc as usize } else { 8192 };
        Ok(Self {
            file: Some(file),
            buf: Vec::with_capacity(cap.min(64 * 1024)),
            cap,
            pos: 0,
        })
    }

    fn position(&self) -> i64 {
        self.pos
    }

    fn write_all_fd(file: &mut std::fs::File, mut data: &[u8]) -> (usize, bool) {
        let mut written = 0usize;
        while !data.is_empty() {
            match file.write(data) {
                Ok(0) => return (written, false),
                Ok(n) => {
                    written += n;
                    data = &data[n..];
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return (written, false),
            }
        }
        (written, true)
    }

    fn flush_buf(&mut self) -> bool {
        if self.buf.is_empty() {
            return true;
        }
        let Some(file) = self.file.as_mut() else {
            self.buf.clear();
            return false;
        };
        let (_, ok) = Self::write_all_fd(file, &self.buf);
        self.buf.clear();
        ok
    }

    fn seek(&mut self, offset: i64) -> bool {
        let Ok(uoff) = u64::try_from(offset) else {
            return false;
        };
        if !self.flush_buf() {
            return false;
        }
        let Some(file) = self.file.as_mut() else {
            return false;
        };
        match file.seek(SeekFrom::Start(uoff)) {
            Ok(_) => {
                self.pos = offset;
                true
            }
            Err(_) => false,
        }
    }

    fn write_bytes(&mut self, data: &[u8]) -> i64 {
        if data.is_empty() {
            return 0;
        }
        if self.file.is_none() {
            return 0;
        }
        if self.buf.len().saturating_add(data.len()) <= self.cap {
            self.buf.extend_from_slice(data);
            self.pos = self.pos.saturating_add(data.len() as i64);
            return data.len() as i64;
        }
        if !self.flush_buf() {
            return 0;
        }
        let Some(file) = self.file.as_mut() else {
            return 0;
        };
        if data.len() >= self.cap {
            let (written, _) = Self::write_all_fd(file, data);
            self.pos = self.pos.saturating_add(written as i64);
            written as i64
        } else {
            self.buf.extend_from_slice(data);
            self.pos = self.pos.saturating_add(data.len() as i64);
            data.len() as i64
        }
    }

    fn flush_and_close_with(mut self, close_fn: impl FnOnce(RawFd) -> c_int) -> bool {
        let flushed = self.flush_buf();
        let closed = match self.file.take() {
            Some(file) => close_fn(file.into_raw_fd()) == 0,
            None => true,
        };
        flushed && closed
    }

    fn flush_and_close(self) -> bool {
        self.flush_and_close_with(|fd| unsafe { libc::close(fd) })
    }
}

impl Drop for BufferedDiskFile {
    fn drop(&mut self) {
        let _ = self.flush_buf();
    }
}

// ============================================================================
// ArticleWriter + ArticleDownloader Rust runtime
// ============================================================================

struct RsArticleWriter {
    temp_filename: Vec<u8>,
    output_filename: Vec<u8>,
    result_filename: Vec<u8>,
    format: Format,
    cached_ptr: *mut u8,
    cached_cap: i32,
    article_offset: i64,
    article_size: i32,
    article_ptr: i32,
    duplicate: bool,
    commit_error: bool,
    out_file: Option<BufferedDiskFile>,
    proc_info_path: Vec<u8>,
    proc_final_path: Vec<u8>,
    proc_raw_article: bool,
    proc_direct_write: bool,
    proc_skip_disk_write: bool,
    proc_outfile: Option<BufferedDiskFile>,
    proc_crc: u32,
    post_crc: u32,
    post_final_path: Vec<u8>,
    post_orig_filename: Vec<u8>,
    post_nzb_dest_dir: Vec<u8>,
}

// Safety: `cached_ptr` points to an `ArticleCache`-owned `malloc` buffer exclusively owned
// by this `ArticleDownloader` instance until `Finish(true)` attaches it or `Start`/destructor frees it.
unsafe impl Send for RsArticleWriter {}

impl Drop for RsArticleWriter {
    fn drop(&mut self) {
        if !self.cached_ptr.is_null() {
            cache_free(self.cached_ptr, self.cached_cap);
            self.cached_ptr = std::ptr::null_mut();
            self.cached_cap = 0;
        }
    }
}

impl Default for RsArticleWriter {
    fn default() -> Self {
        Self {
            temp_filename: Vec::new(),
            output_filename: Vec::new(),
            result_filename: Vec::new(),
            format: Format::Unknown,
            cached_ptr: std::ptr::null_mut(),
            cached_cap: 0,
            article_offset: 0,
            article_size: 0,
            article_ptr: 0,
            duplicate: false,
            commit_error: false,
            out_file: None,
            proc_info_path: Vec::new(),
            proc_final_path: Vec::new(),
            proc_raw_article: false,
            proc_direct_write: false,
            proc_skip_disk_write: false,
            proc_outfile: None,
            proc_crc: 0,
            post_crc: 0,
            post_final_path: Vec::new(),
            post_orig_filename: Vec::new(),
            post_nzb_dest_dir: Vec::new(),
        }
    }
}

impl RsArticleWriter {
    fn drop_cached(&mut self) {
        if !self.cached_ptr.is_null() {
            cache_free(self.cached_ptr, self.cached_cap);
            self.cached_ptr = std::ptr::null_mut();
            self.cached_cap = 0;
        }
    }

    fn range_exceeded(&self) -> bool {
        self.article_ptr > self.article_size
    }

    fn has_cached_buffer(&self) -> bool {
        !self.cached_ptr.is_null()
    }

    fn build_filenames_locked(&mut self, host: &DlHost) {
        let mut ints = [0i32; 4];
        let mut strs = [std::ptr::null::<c_char>(); 3];
        host.raw(
            4,
            1,
            1,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        let file_id = ints[0];
        let part_num = ints[1];
        let force_direct_write = ints[2] != 0;
        let output_initialized = ints[3] != 0;
        let (direct_write, tmp_dir) = match crate::options::active_options() {
            Some(opts) => (opts.scalars().direct_write != 0, opts.str_bytes(5).to_vec()),
            None => (false, Vec::new()),
        };
        let dest_dir = unsafe { cstr_or_empty(strs[0]) };
        let existing_out = unsafe { cstr_or_empty(strs[1]) };
        let file_filename = unsafe { cstr_or_empty(strs[2]) };

        if let Some(built) = article_writer_build_output_filenames(
            &tmp_dir,
            &dest_dir,
            crate::paths::PATH_SEPARATOR,
            file_id,
            part_num,
            direct_write,
            force_direct_write,
            &existing_out,
            output_initialized,
            &file_filename,
            |p| crate::fsops::file_exists(p, &crate::fsops::posix_stat),
        ) {
            let res_c = nul_vec(&built.result_filename);
            host.raw(
                4,
                18,
                5,
                0,
                0,
                res_c.as_ptr().cast(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            self.temp_filename = built.temp_filename;
            if let Some(ofn) = built.output_filename {
                if built.update_file_info_output_filename {
                    let ofn_c = nul_vec(&ofn);
                    host.raw(
                        4,
                        18,
                        6,
                        0,
                        0,
                        ofn_c.as_ptr().cast(),
                        std::ptr::null(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                }
                self.output_filename = ofn;
            }
        }

        let mut rstrs = [std::ptr::null::<c_char>(); 1];
        host.raw(
            4,
            1,
            2,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            rstrs.as_mut_ptr(),
        );
        self.result_filename = unsafe { cstr_or_empty(rstrs[0]) };
    }

    #[allow(clippy::too_many_arguments)]
    fn start(
        &mut self,
        host: &DlHost,
        info_name: &[u8],
        format: Format,
        filename: Option<&[u8]>,
        file_size: i64,
        article_offset: i64,
        article_size: i32,
    ) -> bool {
        self.out_file = None;
        self.drop_cached();

        let mut ints = [0i32; 6];
        let mut strs = [std::ptr::null::<c_char>(); 1];
        let nzb_ptr = host.raw(
            4,
            15,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        let (
            dupe_check,
            direct_write_opt,
            skip_write_opt,
            article_cache_mb,
            raw_article,
            write_buffer_kb,
        ) = match crate::options::active_options() {
            Some(opts) => {
                let sc = opts.scalars();
                (
                    sc.dupe_check != 0,
                    sc.direct_write != 0,
                    sc.skip_write != 0,
                    sc.article_cache,
                    sc.raw_article != 0,
                    sc.write_buffer,
                )
            }
            None => (false, false, false, 0, false, 0),
        };
        let dupe_mode = ints[0];
        let many_dupe_files = ints[1] != 0;
        let force_direct_write = ints[2] != 0;
        let skip_disk_write = ints[3] != 0 || skip_write_opt;
        let art_info_size = ints[4];
        let is_dupe_fallback = ints[5] > 0;
        let dest_dir = unsafe { cstr_or_empty(strs[0]) };

        self.format = format;
        self.article_offset = article_offset;
        self.article_size = if article_size > 0 {
            article_size
        } else {
            art_info_size
        };
        self.article_ptr = 0;

        if self.format == Format::Yenc {
            if dupe_check && dupe_mode != DM_FORCE && !many_dupe_files {
                let dest_file_path = filename.map(|fn_bytes| {
                    article_writer_format_dest_file_path(
                        &dest_dir,
                        crate::paths::PATH_SEPARATOR,
                        fn_bytes,
                    )
                });
                let dest_c = dest_file_path.as_deref().map(nul_vec);
                let mut d_ints = [0i32; 2];
                host.raw(
                    4,
                    16,
                    0,
                    0,
                    0,
                    dest_c
                        .as_ref()
                        .map_or(std::ptr::null(), |v| v.as_ptr().cast()),
                    std::ptr::null(),
                    d_ints.as_mut_ptr(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                let output_initialized = d_ints[0] != 0;
                let is_own_output_file = d_ints[1] != 0;
                if !output_initialized && !is_own_output_file {
                    if let Some(ref dfp) = dest_file_path {
                        if crate::fsops::file_exists(dfp, &crate::fsops::posix_stat) {
                            self.duplicate = true;
                            return false;
                        }
                    }
                }
            }

            if direct_write_opt && !is_dupe_fallback {
                let out_c = nul_vec(&self.output_filename);
                let mut fsz = [file_size];
                let ok = host.raw(
                    4,
                    17,
                    i32::from(skip_disk_write),
                    0,
                    0,
                    out_c.as_ptr().cast(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    fsz.as_mut_ptr(),
                    std::ptr::null_mut(),
                ) != 0;
                if !ok {
                    return false;
                }
            }
        }

        let is_critical = memory_monitor_tier() == PRESSURE_CRITICAL;
        if article_cache_mb > 0
            && !raw_article
            && self.format == Format::Yenc
            && !is_dupe_fallback
            && !is_critical
        {
            let mut p = cache_alloc(self.article_size);
            while p.is_null()
                && CACHE_FLUSHING.load(Ordering::Acquire)
                && memory_monitor_tier() != PRESSURE_CRITICAL
            {
                sleep_ms(5);
                p = cache_alloc(self.article_size);
            }
            self.cached_ptr = p;
            self.cached_cap = if p.is_null() { 0 } else { self.article_size };
            if self.cached_ptr.is_null() {
                host.log(
                    MK_DETAIL,
                    &join_c_slices(&[b"Article cache is full, using disk for ", info_name]),
                );
            }
        }

        if self.cached_ptr.is_null() {
            if skip_disk_write {
                return true;
            }
            let direct_write = (direct_write_opt || force_direct_write)
                && self.format == Format::Yenc
                && !is_dupe_fallback;
            let out_filename = if direct_write {
                &self.output_filename
            } else {
                &self.temp_filename
            };
            match BufferedDiskFile::open(out_filename, direct_write, write_buffer_kb, art_info_size)
            {
                Ok(mut f) => {
                    if direct_write {
                        let _ = f.seek(self.article_offset);
                    }
                    self.out_file = Some(f);
                }
                Err(err_no) => {
                    let err_msg = crate::fsops::error_message(err_no);
                    let verb: &[u8] = if direct_write { b"open" } else { b"create" };
                    let msg = join_c_slices(&[
                        b"Could not ",
                        verb,
                        b" file ",
                        out_filename,
                        b": ",
                        &err_msg,
                    ]);
                    host.log_nzb(nzb_ptr, MK_ERROR, &msg);
                    return false;
                }
            }
        }

        true
    }

    fn write(&mut self, buf: &[u8], raw_article: bool, skip_disk_write: bool) -> bool {
        let (new_ptr, exceeded) = article_writer_advance_and_check_range(
            raw_article,
            self.article_ptr,
            buf.len() as i32,
            self.article_size,
        );
        self.article_ptr = new_ptr;
        if !raw_article && exceeded {
            return true;
        }
        if !raw_article && !self.cached_ptr.is_null() {
            let start = (self.article_ptr - buf.len() as i32).max(0) as usize;
            unsafe {
                std::ptr::copy_nonoverlapping(buf.as_ptr(), self.cached_ptr.add(start), buf.len());
            }
            return true;
        }
        if skip_disk_write {
            return true;
        }
        let Some(ref mut f) = self.out_file else {
            return false;
        };
        f.write_bytes(buf) > 0
    }

    fn finish(&mut self, host: &DlHost, success: bool) {
        self.out_file = None;

        let mut ints = [0i32; 6];
        let mut strs = [std::ptr::null::<c_char>(); 1];
        let nzb_ptr = host.raw(
            4,
            15,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        let (direct_write_opt, skip_write_opt, raw_article) = match crate::options::active_options()
        {
            Some(opts) => {
                let sc = opts.scalars();
                (
                    sc.direct_write != 0,
                    sc.skip_write != 0,
                    sc.raw_article != 0,
                )
            }
            None => (false, false, false),
        };
        let force_direct_write = ints[2] != 0;
        let skip_disk_write = ints[3] != 0 || skip_write_opt;
        let is_dupe_fallback = ints[5] > 0;
        let direct_write = (direct_write_opt || force_direct_write)
            && self.format == Format::Yenc
            && !is_dupe_fallback;
        let has_cached = !self.cached_ptr.is_null();

        article_writer_finish_disk_ops(
            success,
            skip_disk_write,
            raw_article,
            direct_write,
            has_cached,
            &self.temp_filename,
            &self.result_filename,
            |msg| host.log_nzb(nzb_ptr, MK_ERROR, msg),
        );

        if !success || raw_article {
            self.drop_cached();
            return;
        }

        let mut off = [self.article_offset];
        if has_cached {
            if self.article_ptr > 0 && self.article_ptr != self.cached_cap {
                let (np, nc) = cache_realloc(self.cached_ptr, self.cached_cap, self.article_ptr);
                self.cached_ptr = np;
                self.cached_cap = nc;
            }
            let ptr = self.cached_ptr;
            let mut cap_arr = [self.cached_cap];
            self.cached_ptr = std::ptr::null_mut();
            self.cached_cap = 0;
            host.raw(
                4,
                18,
                2,
                ptr as usize,
                self.article_ptr as usize,
                std::ptr::null(),
                std::ptr::null(),
                cap_arr.as_mut_ptr(),
                off.as_mut_ptr(),
                std::ptr::null_mut(),
            );
        } else {
            host.raw(
                4,
                18,
                3,
                self.article_ptr as usize,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                off.as_mut_ptr(),
                std::ptr::null_mut(),
            );
        }
    }

    fn report_completion_status(
        host: &DlHost,
        nzb_ptr: usize,
        total_articles: i32,
        success_articles: i32,
        missed_articles: i32,
        failed_articles: i32,
        info_filename: &[u8],
    ) {
        match article_writer_classify_completion_status(
            total_articles,
            success_articles,
            missed_articles,
            failed_articles,
        ) {
            1 => host.log_nzb(
                nzb_ptr,
                MK_INFO,
                &join_c_slices(&[b"Successfully downloaded ", info_filename]),
            ),
            2 => {
                let p = format!(
                    "{} of {} article downloads failed for \"",
                    missed_articles.wrapping_add(failed_articles),
                    total_articles
                );
                host.log_nzb(
                    nzb_ptr,
                    MK_WARNING,
                    &join_c_slices(&[p.as_bytes(), info_filename, b"\""]),
                );
            }
            _ => host.log_nzb(
                nzb_ptr,
                MK_INFO,
                &join_c_slices(&[b"Partially downloaded ", info_filename]),
            ),
        }
    }

    fn cleanup_old_data(
        &self,
        host: &DlHost,
        nzb_ptr: usize,
        articles_len: usize,
        is_hard_linked: bool,
        inter_dir_empty: bool,
        direct_write: bool,
        nzb_dest_dir: &[u8],
        final_output_path: &[u8],
    ) {
        let mut raw_articles = Vec::with_capacity(articles_len);
        for idx in 0..articles_len {
            let mut ints = [0i32; 5];
            let mut i64s = [0i64; 1];
            let mut strs = [std::ptr::null::<c_char>(); 2];
            host.raw(
                4,
                19,
                4,
                idx,
                0,
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                strs.as_mut_ptr(),
            );
            raw_articles.push((ints[4], unsafe { opt_cstr(strs[1]) }));
        }
        let refs: Vec<(i32, Option<&[u8]>)> = raw_articles
            .iter()
            .map(|(r, rf)| (*r, rf.as_deref()))
            .collect();

        let mut move_error = Vec::new();
        let res = article_writer_execute_cleanup_old_data(
            direct_write,
            nzb_dest_dir,
            final_output_path,
            &self.output_filename,
            is_hard_linked,
            inter_dir_empty,
            crate::paths::PATH_SEPARATOR,
            crate::paths::ALT_PATH_SEPARATOR,
            &refs,
            |a, b| c_bytes(a) == c_bytes(b),
            |p| crate::fsops::file_exists(p, &crate::fsops::posix_stat),
            |p| CString::new(c_bytes(p)).is_ok_and(|c| unsafe { libc::unlink(c.as_ptr()) == 0 }),
            |from_b, to_b| {
                let (Ok(cf), Ok(ct)) = (CString::new(c_bytes(from_b)), CString::new(c_bytes(to_b)))
                else {
                    return false;
                };
                if crate::fsops::move_file(&cf, &ct) {
                    true
                } else {
                    move_error = crate::fsops::error_message(crate::fsops::last_errno());
                    false
                }
            },
            |dir_b| {
                let c_dir = CString::new(c_bytes(dir_b)).ok();
                let mut browser = crate::fsops::PosixDirBrowser::new(c_dir.as_deref(), true);
                browser.next_ptr().is_null()
            },
            |dir_b| CString::new(c_bytes(dir_b)).is_ok_and(|c| crate::fsops::remove_directory(&c)),
        );

        if res.move_failed {
            let msg = join_c_slices(&[
                b"Could not move file ",
                &self.output_filename,
                b" to ",
                final_output_path,
                b": ",
                &move_error,
            ]);
            host.log_nzb(nzb_ptr, MK_ERROR, &msg);
        }
        if res.checked_old_dest_dir.is_some() {
            host.log(
                MK_DEBUG,
                &join_c_slices(&[b"Checking old dir for: ", &self.output_filename]),
            );
        }
        if let Some(del_dir) = res.deleted_old_dest_dir {
            host.log(MK_DEBUG, &join_c_slices(&[b"Deleting old dir: ", &del_dir]));
        }
        if res.clear_output_filename {
            host.raw(
                4,
                18,
                6,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
    }

    fn process_articles_locked(&mut self, host: &DlHost) {
        let mut meta_ints = [0i32; 16];
        let mut meta_strs = [std::ptr::null::<c_char>(); 4];
        let nzb_ptr = host.raw(
            4,
            19,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            meta_ints.as_mut_ptr(),
            std::ptr::null_mut(),
            meta_strs.as_mut_ptr(),
        );
        let total_articles = meta_ints[15].max(0) as usize;

        struct FinArtMeta {
            idx: usize,
            offset: i64,
            size: i32,
            crc: u32,
            part_num: i32,
            dupe_round: i32,
            content_ptr: *const u8,
            result_filename: Option<Vec<u8>>,
        }
        let mut finished = Vec::with_capacity(total_articles);
        for idx in 0..total_articles {
            let mut ints = [0i32; 5];
            let mut i64s = [0i64; 1];
            let mut strs = [std::ptr::null::<c_char>(); 2];
            let is_fin = host.raw(
                4,
                19,
                4,
                idx,
                0,
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                strs.as_mut_ptr(),
            ) != 0;
            if !is_fin {
                continue;
            }
            finished.push(FinArtMeta {
                idx,
                offset: i64s[0],
                size: ints[1],
                crc: ints[2] as u32,
                part_num: ints[3],
                dupe_round: ints[4],
                content_ptr: strs[0].cast::<u8>(),
                result_filename: unsafe { opt_cstr(strs[1]) },
            });
        }

        let items: Vec<ArticleWriterProcessItem<'_>> = finished
            .iter()
            .map(|m| ArticleWriterProcessItem {
                segment_offset: m.offset,
                segment_size: m.size,
                crc: m.crc,
                part_number: m.part_num,
                dupe_fallback_round: m.dupe_round,
                segment_content: unsafe {
                    if m.content_ptr.is_null() {
                        None
                    } else {
                        Some(std::slice::from_raw_parts(
                            m.content_ptr,
                            m.size.max(0) as usize,
                        ))
                    }
                },
                result_filename: m.result_filename.as_deref(),
            })
            .collect();

        let outfile_cell = std::cell::RefCell::new(self.proc_outfile.take());
        let (crc, commit_err, failed_delta) = article_writer_process_articles(
            &items,
            total_articles,
            &self.proc_info_path,
            &self.proc_final_path,
            self.proc_raw_article,
            self.proc_direct_write,
            self.proc_skip_disk_write,
            self.format == Format::Yenc,
            || {
                outfile_cell
                    .borrow()
                    .as_ref()
                    .map_or(-1, BufferedDiskFile::position)
            },
            |off| {
                outfile_cell
                    .borrow_mut()
                    .as_mut()
                    .is_some_and(|f| f.seek(off))
            },
            |buf| {
                outfile_cell
                    .borrow_mut()
                    .as_mut()
                    .map_or(0, |f| f.write_bytes(buf))
            },
            |item_idx| {
                let orig_idx = finished[item_idx].idx;
                host.raw(
                    4,
                    19,
                    5,
                    orig_idx,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            },
            |msg| host.log_nzb(nzb_ptr, MK_ERROR, msg),
        );
        self.proc_outfile = outfile_cell.into_inner();
        self.proc_crc = crc;
        if commit_err {
            self.commit_error = true;
        }
        if failed_delta > 0 {
            host.raw(
                4,
                19,
                6,
                failed_delta as usize,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
    }

    fn post_process_locked(&mut self, host: &DlHost) {
        let cur_path_c = nul_vec(&self.post_final_path);
        let mut ints = [0i32; 2];
        let mut strs = [std::ptr::null::<c_char>(); 3];
        let nzb_ptr = host.raw(
            4,
            19,
            8,
            self.post_crc as usize,
            0,
            cur_path_c.as_ptr().cast(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        let is_hard_linked = ints[0] != 0;
        let inter_dir_empty = ints[1] != 0;
        let cur_filename = unsafe { cstr_or_empty(strs[0]) };
        let file_output_fn = unsafe { cstr_or_empty(strs[1]) };
        let cur_nzb_dest_dir = unsafe { cstr_or_empty(strs[2]) };

        let (updated_ofn, clear_and_return) = article_writer_post_process_rename_and_hardlink(
            &self.post_orig_filename,
            &cur_filename,
            &self.post_nzb_dest_dir,
            &file_output_fn,
            &self.output_filename,
            is_hard_linked,
            inter_dir_empty,
            |msg| host.log_nzb(nzb_ptr, MK_ERROR, msg),
        );
        if let Some(new_ofn) = updated_ofn {
            let new_c = nul_vec(&new_ofn);
            host.raw(
                4,
                18,
                6,
                0,
                0,
                new_c.as_ptr().cast(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
        if clear_and_return {
            host.raw(
                4,
                18,
                6,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            return;
        }
        if c_bytes(&self.post_nzb_dest_dir) != c_bytes(&cur_nzb_dest_dir) {
            let _ = move_completed_files_on_host(host, nzb_ptr, &self.post_nzb_dest_dir);
        }
    }

    fn complete_file_parts(&mut self, host: &DlHost) -> (bool, bool) {
        host.log(MK_DEBUG, b"Completing file parts");
        self.commit_error = false;

        let mut ints = [0i32; 16];
        let mut strs = [std::ptr::null::<c_char>(); 4];
        let nzb_ptr = host.raw(
            4,
            19,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        );
        let nzb_name = unsafe { cstr_or_empty(strs[0]) };
        let nzb_dest_dir = unsafe { cstr_or_empty(strs[1]) };
        let filename = unsafe { cstr_or_empty(strs[2]) };
        let file_out_fn = unsafe { cstr_or_empty(strs[3]) };

        host.log(MK_DEBUG, &join_c_slices(&[b"ArticleFilename: ", &filename]));

        if self.output_filename.is_empty() && !file_out_fn.is_empty() {
            self.output_filename = file_out_fn;
        }

        let direct_write_opt = ints[0] != 0;
        let force_direct_write = ints[1] != 0;
        let cur_output_init = ints[2] != 0;
        let cached = ints[3] > 0;
        let success_articles = ints[4];
        let failed_articles = ints[5];
        let missed_articles = ints[6];
        let total_articles = ints[7];
        let is_hard_linked = ints[8] != 0;
        let inter_dir_empty = ints[9] != 0;
        let raw_article = ints[10] != 0;
        let skip_disk_write = ints[11] != 0;
        let write_buffer_kb = ints[12];
        let articles_len = ints[15].max(0) as usize;

        let mut has_own_out = false;
        let mut has_force_dest = false;
        if (direct_write_opt || force_direct_write) && !cur_output_init {
            let dest_path = article_writer_format_dest_file_path(
                &nzb_dest_dir,
                crate::paths::PATH_SEPARATOR,
                &filename,
            );
            has_own_out = !self.output_filename.is_empty()
                && crate::fsops::file_exists(&self.output_filename, &crate::fsops::posix_stat);
            has_force_dest = !has_own_out
                && force_direct_write
                && crate::fsops::file_exists(&dest_path, &crate::fsops::posix_stat);
        }
        let (new_init, direct_write) = article_writer_resolve_direct_write_ready(
            direct_write_opt,
            force_direct_write,
            cur_output_init,
            has_own_out,
            has_force_dest,
        );
        if new_init && !cur_output_init {
            host.raw(
                4,
                19,
                1,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }

        let full_info_path =
            article_writer_format_info_path(&nzb_name, crate::paths::PATH_SEPARATOR, &filename);
        let start_prefix: &[u8] = if raw_article {
            b"Moving articles for "
        } else if direct_write && cached {
            b"Writing articles for "
        } else if direct_write {
            b"Checking articles for "
        } else {
            b"Joining articles for "
        };
        host.log(MK_DETAIL, &join_c_slices(&[start_prefix, &full_info_path]));

        if success_articles == 0 {
            host.log(
                MK_DETAIL,
                &join_c_slices(&[
                    b"Skipping file parts completion for ",
                    &full_info_path,
                    b": no successful articles",
                ]),
            );
            if !skip_disk_write {
                self.cleanup_old_data(
                    host,
                    nzb_ptr,
                    articles_len,
                    is_hard_linked,
                    inter_dir_empty,
                    direct_write,
                    &nzb_dest_dir,
                    b"",
                );
            }
            Self::report_completion_status(
                host,
                nzb_ptr,
                total_articles,
                success_articles,
                missed_articles,
                failed_articles,
                &full_info_path,
            );
            return (true, false);
        }

        if skip_disk_write {
            Self::report_completion_status(
                host,
                nzb_ptr,
                total_articles,
                success_articles,
                missed_articles,
                failed_articles,
                &full_info_path,
            );
            host.raw(
                4,
                19,
                2,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            return (true, false);
        }

        let mut dir_err = Vec::new();
        if !crate::fsops::force_directories(&nzb_dest_dir, &mut dir_err, &crate::fsops::posix_stat)
        {
            let msg = join_c_slices(&[
                b"Could not create directory ",
                &nzb_dest_dir,
                b": ",
                &dir_err,
            ]);
            host.log_nzb(nzb_ptr, MK_ERROR, &msg);
            return (false, false);
        }

        let mut opened_outfile: Option<BufferedDiskFile> = None;
        let mut open_errno = 0i32;
        let mut force_dirs_err = Vec::new();
        let setup = article_writer_execute_setup_output_file(
            &nzb_dest_dir,
            &filename,
            crate::paths::PATH_SEPARATOR,
            raw_article,
            direct_write,
            force_direct_write,
            &self.output_filename,
            |a, b| c_bytes(a) == c_bytes(b),
            |dir_b, fn_b| {
                let cd = CString::new(c_bytes(dir_b)).unwrap_or_default();
                let cf = CString::new(c_bytes(fn_b)).unwrap_or_default();
                crate::fsops::make_unique_filename(&cd, &cf, &crate::fsops::posix_stat)
            },
            |p| crate::fsops::file_exists(p, &crate::fsops::posix_stat),
            |p| crate::fsops::force_directories(p, &mut force_dirs_err, &crate::fsops::posix_stat),
            |p| CString::new(c_bytes(p)).is_ok_and(|c| unsafe { libc::unlink(c.as_ptr()) == 0 }),
            |p, rw| match BufferedDiskFile::open(p, rw, write_buffer_kb, 0) {
                Ok(f) => {
                    opened_outfile = Some(f);
                    true
                }
                Err(e) => {
                    open_errno = e;
                    false
                }
            },
            |p| {
                if let Ok(cp) = CString::new(c_bytes(p)) {
                    let _ = unsafe {
                        libc::mkdir(cp.as_ptr(), libc::S_IRWXU | libc::S_IRWXG | libc::S_IRWXO)
                    };
                }
                crate::fsops::directory_exists(p, &crate::fsops::posix_stat)
            },
        );

        self.output_filename = setup.updated_output_filename;
        if let Some(pdir) = setup.parent_dir_failed {
            let msg = join_c_slices(&[
                b"Could not create directory ",
                &pdir,
                b": ",
                &force_dirs_err,
            ]);
            host.log_nzb(nzb_ptr, MK_ERROR, &msg);
        }
        if !setup.ok {
            if setup.error_kind != 0 {
                let errno_val = if open_errno != 0 {
                    open_errno
                } else {
                    crate::fsops::last_errno()
                };
                let err_msg = crate::fsops::error_message(errno_val);
                let verb: &[u8] = match setup.error_kind {
                    1 => b"create file",
                    2 => b"open file",
                    _ => b"create directory",
                };
                let msg = join_c_slices(&[
                    b"Could not ",
                    verb,
                    b" ",
                    &setup.error_path,
                    b": ",
                    &err_msg,
                ]);
                host.log_nzb(nzb_ptr, MK_ERROR, &msg);
            }
            return (false, false);
        }

        let final_output_path = setup.final_path;
        let temp_dest_path = setup.temp_path;

        self.proc_info_path = full_info_path.clone();
        self.proc_final_path = final_output_path.clone();
        self.proc_raw_article = raw_article;
        self.proc_direct_write = direct_write;
        self.proc_skip_disk_write = skip_disk_write;
        self.proc_outfile = opened_outfile;
        self.proc_crc = 0;

        struct FlushLockGuard;
        impl Drop for FlushLockGuard {
            fn drop(&mut self) {
                cache_unlock_flush();
            }
        }
        let _flush_guard = if cached {
            cache_lock_flush();
            Some(FlushLockGuard)
        } else {
            None
        };
        self.process_articles_locked(host);
        drop(_flush_guard);

        let crc = self.proc_crc;
        let outfile_opt = self.proc_outfile.take();

        if self.commit_error {
            drop(outfile_opt);
            host.log_nzb(
                nzb_ptr,
                MK_ERROR,
                &join_c_slices(&[b"Could not commit all article data for ", &full_info_path]),
            );
            return (false, false);
        }

        if let Some(outfile) = outfile_opt {
            if !outfile.flush_and_close() {
                host.log_nzb(
                    nzb_ptr,
                    MK_ERROR,
                    &join_c_slices(&[
                        b"Could not flush completed output file ",
                        &final_output_path,
                    ]),
                );
                return (false, false);
            }
            if !direct_write {
                let moved = match (
                    CString::new(c_bytes(&temp_dest_path)),
                    CString::new(c_bytes(&final_output_path)),
                ) {
                    (Ok(cs), Ok(cd)) => crate::fsops::move_file(&cs, &cd),
                    _ => false,
                };
                if !moved {
                    let err_msg = crate::fsops::error_message(crate::fsops::last_errno());
                    let msg = join_c_slices(&[
                        b"Could not move file ",
                        &temp_dest_path,
                        b" to ",
                        &final_output_path,
                        b": ",
                        &err_msg,
                    ]);
                    host.log_nzb(nzb_ptr, MK_ERROR, &msg);
                }
            }
        }

        self.cleanup_old_data(
            host,
            nzb_ptr,
            articles_len,
            is_hard_linked,
            inter_dir_empty,
            direct_write,
            &nzb_dest_dir,
            &final_output_path,
        );

        let mut post_meta = [0i32; 16];
        let mut post_strs = [std::ptr::null::<c_char>(); 4];
        host.raw(
            4,
            19,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            post_meta.as_mut_ptr(),
            std::ptr::null_mut(),
            post_strs.as_mut_ptr(),
        );
        Self::report_completion_status(
            host,
            nzb_ptr,
            post_meta[7],
            post_meta[4],
            post_meta[6],
            post_meta[5],
            &full_info_path,
        );

        self.post_crc = crc;
        self.post_final_path = final_output_path;
        self.post_orig_filename = filename;
        self.post_nzb_dest_dir = nzb_dest_dir;
        (true, true)
    }
}

fn move_completed_files_on_host(host: &DlHost, nzb_ptr: usize, old_dest_dir: &[u8]) -> bool {
    let mut ints = [0i32; 2];
    let mut strs = [std::ptr::null::<c_char>(); 1];
    host.raw(
        5,
        0,
        0,
        nzb_ptr,
        0,
        std::ptr::null(),
        std::ptr::null(),
        ints.as_mut_ptr(),
        std::ptr::null_mut(),
        strs.as_mut_ptr(),
    );
    let skip_disk_write = ints[0] != 0;
    let count = ints[1].max(0) as usize;
    if skip_disk_write || count == 0 {
        return true;
    }
    let dest_dir = unsafe { cstr_or_empty(strs[0]) };
    let mut owned_names = Vec::with_capacity(count);
    for idx in 0..count {
        let mut fstrs = [std::ptr::null::<c_char>(); 1];
        host.raw(
            5,
            1,
            idx as i32,
            nzb_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            fstrs.as_mut_ptr(),
        );
        owned_names.push(unsafe { cstr_or_empty(fstrs[0]) });
    }
    let refs: Vec<&[u8]> = owned_names.iter().map(Vec::as_slice).collect();
    article_writer_move_completed_files(
        &dest_dir,
        c_bytes(old_dest_dir),
        &refs,
        |msg| host.log_nzb(nzb_ptr, MK_ERROR, msg),
        || {
            host.raw(
                5,
                2,
                0,
                nzb_ptr,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) != 0
        },
    )
}

struct DlWorker {
    decoder: Decoder,
    writer: RsArticleWriter,
    analyzer: Option<Analyzer>,
    has_analyzer: bool,
    parcheck_enabled: bool,
    hash16k_c: [u8; 33],
    par_set_id_c: [u8; 33],
    article_filename_c: Option<Vec<u8>>,
    server_stats: Vec<(i32, i32, i32)>,
    writing_started: bool,
    local_write_error: bool,
    content_rejected: bool,
    downloaded_size: i32,
    decoded_file_size: i64,
}

const AD_UNDEFINED: i32 = 0;

pub struct NzbgetRsArticleDownloader {
    info_name: Vec<u8>,
    status: AtomicI32,
    last_update_time: AtomicI64,
    conn_state: Mutex<(usize, Vec<u8>)>,
    worker: Mutex<DlWorker>,
}

impl NzbgetRsArticleDownloader {
    pub fn new(
        has_analyzer: bool,
        parcheck_enabled: bool,
        initial_conn: usize,
        info_name: Vec<u8>,
    ) -> Self {
        Self {
            info_name,
            status: AtomicI32::new(AD_UNDEFINED),
            last_update_time: AtomicI64::new(now_sec()),
            conn_state: Mutex::new((initial_conn, Vec::new())),
            worker: Mutex::new(DlWorker {
                decoder: Decoder::new(
                    crate::decoder::rust_yenc_decode_incremental,
                    crate::decoder::rust_yenc_crc,
                ),
                writer: RsArticleWriter::default(),
                analyzer: if has_analyzer && parcheck_enabled {
                    Some(Analyzer::default())
                } else {
                    None
                },
                has_analyzer,
                parcheck_enabled,
                hash16k_c: [0u8; 33],
                par_set_id_c: [0u8; 33],
                article_filename_c: None,
                server_stats: Vec::new(),
                writing_started: false,
                local_write_error: false,
                content_rejected: false,
                downloaded_size: 0,
                decoded_file_size: 0,
            }),
        }
    }

    fn conn_ptr(&self) -> usize {
        self.conn_state.lock().unwrap_or_else(|e| e.into_inner()).0
    }

    fn set_conn_ptr(&self, ptr: usize) {
        self.conn_state.lock().unwrap_or_else(|e| e.into_inner()).0 = ptr;
    }

    fn set_conn_name(&self, name: Vec<u8>) {
        self.conn_state.lock().unwrap_or_else(|e| e.into_inner()).1 = name;
    }

    fn copy_conn_name(&self) -> Vec<u8> {
        self.conn_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .clone()
    }

    fn set_status(&self, host: &DlHost, st: i32, touch_now: bool, notify: bool) {
        if touch_now {
            self.last_update_time.store(now_sec(), Ordering::SeqCst);
        }
        self.status.store(st, Ordering::SeqCst);
        if notify {
            host.raw(
                4,
                3,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
    }

    fn free_connection(&self, host: &DlHost, downloaded_size: &mut i32, keep_connected: bool) {
        if self.conn_ptr() == 0 {
            return;
        }
        host.log(MK_DEBUG, b"Releasing connection");
        let mut cs = self.conn_state.lock().unwrap_or_else(|e| e.into_inner());
        if cs.0 != 0 {
            let cptr = cs.0 as *mut crate::net::PooledNntpConn;
            let conn = unsafe { &*cptr };
            if !keep_connected || conn.is_cancelled() {
                unsafe {
                    conn.disconnect();
                }
            }
            let bytes_read = conn.fetch_total_bytes_read();
            if let Some(sm) = crate::statmeter::active_statmeter() {
                let off = crate::ffi::active_workstate().map_or(0, |ws| ws.local_time_offset());
                sm.add_server_stats(bytes_read, 0, 0, conn.server.id, off);
            }
            if let Some(pool) = crate::net::active_server_pool() {
                pool.free_connection(cptr, true);
            }
            *downloaded_size = downloaded_size.wrapping_add(bytes_read);
            cs.0 = 0;
        }
    }

    fn add_conn_bytes(&self, downloaded_size: &mut i32) {
        let cptr = self.conn_ptr();
        if cptr == 0 {
            return;
        }
        let conn = unsafe { &*(cptr as *const crate::net::PooledNntpConn) };
        let bytes_read = conn.fetch_total_bytes_read();
        if let Some(sm) = crate::statmeter::active_statmeter() {
            let off = crate::ffi::active_workstate().map_or(0, |ws| ws.local_time_offset());
            sm.add_server_stats(bytes_read, 0, 0, conn.server.id, off);
        }
        *downloaded_size = downloaded_size.wrapping_add(bytes_read);
    }

    fn check_response(
        &self,
        host: &DlHost,
        conn_name: &[u8],
        response: Option<&[u8]>,
        auth_error: bool,
        is_stopped: bool,
        comment: &[u8],
    ) -> i32 {
        let status = article_downloader_check_response(response, auth_error);
        if status != AD_FINISHED {
            if let Some(resp) = response {
                let msg = join_c_slices(&[
                    b"Article ",
                    &self.info_name,
                    b" @ ",
                    conn_name,
                    b" failed, ",
                    comment,
                    b": ",
                    resp,
                ]);
                host.log(MK_DETAIL, &msg);
            } else if !is_stopped {
                let msg = join_c_slices(&[
                    b"Article ",
                    &self.info_name,
                    b" @ ",
                    conn_name,
                    b" failed, ",
                    comment,
                    b": Connection closed by remote host",
                ]);
                host.log(MK_DETAIL, &msg);
            }
        }
        status
    }

    fn write_decoded(
        &self,
        w: &mut DlWorker,
        host: &DlHost,
        decoded: &[u8],
        raw_article: bool,
        skip_disk_write: bool,
    ) -> bool {
        if !w.writing_started {
            let mut art_filename_opt: Option<Vec<u8>> = None;
            let mut article_file_size = 0i64;
            let mut article_offset = 0i64;
            let mut article_size = 0i32;

            if !raw_article {
                art_filename_opt = Some(c_bytes(&w.decoder.article_filename).to_vec());
                if w.decoder.format == Format::Yenc {
                    let begin_pos = w.decoder.begin_pos;
                    let end_pos = w.decoder.end_pos;
                    article_file_size = w.decoder.size;

                    let mut vints = [0i32; 1];
                    let mut vi64s = [0i64; 3];
                    host.raw(
                        4,
                        14,
                        0,
                        0,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        vints.as_mut_ptr(),
                        vi64s.as_mut_ptr(),
                        std::ptr::null_mut(),
                    );
                    let dupe_round = vints[0];
                    let expected_file_size = vi64s[0];
                    let dupe_expected_offset = vi64s[1];
                    let dupe_expected_end = vi64s[2];

                    match article_downloader_validate_yenc(
                        begin_pos,
                        end_pos,
                        article_file_size,
                        expected_file_size,
                        dupe_round,
                        dupe_expected_offset,
                        dupe_expected_end,
                    ) {
                        Ok((off, sz)) => {
                            w.decoded_file_size = article_file_size;
                            article_offset = off;
                            article_size = sz;
                        }
                        Err(1) => return false,
                        Err(2) => {
                            let tail = format!(
                                ": range {begin_pos}-{end_pos} out of range (file size {article_file_size})"
                            );
                            host.log(
                                MK_WARNING,
                                &join_c_slices(&[
                                    b"Malformed article ",
                                    &self.info_name,
                                    tail.as_bytes(),
                                ]),
                            );
                            return false;
                        }
                        Err(code) => {
                            w.decoded_file_size = article_file_size;
                            if code == 3 {
                                let tail = format!(
                                    " from duplicate: file size mismatch ({article_file_size} vs {expected_file_size})"
                                );
                                host.log(
                                    MK_DETAIL,
                                    &join_c_slices(&[
                                        b"Discarding article ",
                                        &self.info_name,
                                        tail.as_bytes(),
                                    ]),
                                );
                            } else if code == 4 {
                                let actual_off = begin_pos - 1;
                                let tail = format!(
                                    " from duplicate: decoded offset mismatch ({actual_off} vs {dupe_expected_offset})"
                                );
                                host.log(
                                    MK_DETAIL,
                                    &join_c_slices(&[
                                        b"Discarding article ",
                                        &self.info_name,
                                        tail.as_bytes(),
                                    ]),
                                );
                            } else if code == 5 {
                                let tail = format!(
                                    " from duplicate: decoded end mismatch ({end_pos} vs {dupe_expected_end})"
                                );
                                host.log(
                                    MK_DETAIL,
                                    &join_c_slices(&[
                                        b"Discarding article ",
                                        &self.info_name,
                                        tail.as_bytes(),
                                    ]),
                                );
                            }
                            return false;
                        }
                    }
                }
            }

            if !w.writer.start(
                host,
                &self.info_name,
                w.decoder.format,
                art_filename_opt.as_deref(),
                article_file_size,
                article_offset,
                article_size,
            ) {
                w.local_write_error = true;
                return false;
            }
            w.writing_started = true;
        }

        let ok = w.writer.write(decoded, raw_article, skip_disk_write);
        if !ok {
            w.local_write_error = true;
        }
        if let Some(ref mut ca) = w.analyzer {
            let _ = ca.append(decoded);
        }
        ok
    }

    fn decode_check(
        &self,
        w: &mut DlWorker,
        host: &DlHost,
        raw_article: bool,
        crc_check: bool,
    ) -> i32 {
        if raw_article {
            return AD_FINISHED;
        }
        let dec_st = w.decoder.check() as i32;
        let is_yenc = w.decoder.format == Format::Yenc;
        match article_dl_classify_decode_status(
            raw_article,
            dec_st,
            is_yenc,
            w.writer.range_exceeded(),
        ) {
            1 => {
                w.article_filename_c = Some(nul_vec(&w.decoder.article_filename));
                if is_yenc {
                    let crc = if crc_check {
                        w.decoder.calculated_crc
                    } else {
                        w.decoder.expected_crc
                    };
                    host.raw(
                        4,
                        18,
                        4,
                        crc as usize,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                }
                AD_FINISHED
            }
            2 => {
                host.log(
                    MK_DETAIL,
                    &join_c_slices(&[
                        b"Decoding ",
                        &self.info_name,
                        b" failed: more data than its range",
                    ]),
                );
                AD_FAILED
            }
            3 => {
                host.log(
                    MK_DETAIL,
                    &join_c_slices(&[b"Decoding ", &self.info_name, b" failed: CRC-Error"]),
                );
                AD_CRC_ERROR
            }
            4 => {
                host.log(
                    MK_DETAIL,
                    &join_c_slices(&[
                        b"Decoding ",
                        &self.info_name,
                        b" failed: article incomplete",
                    ]),
                );
                AD_FAILED
            }
            5 => {
                host.log(
                    MK_DETAIL,
                    &join_c_slices(&[b"Decoding ", &self.info_name, b" failed: size mismatch"]),
                );
                AD_FAILED
            }
            6 => {
                host.log(
                    MK_DETAIL,
                    &join_c_slices(&[
                        b"Decoding ",
                        &self.info_name,
                        b" failed: no binary data found",
                    ]),
                );
                AD_FAILED
            }
            _ => {
                host.log(
                    MK_DETAIL,
                    &join_c_slices(&[b"Decoding ", &self.info_name, b" failed"]),
                );
                AD_FAILED
            }
        }
    }

    fn download_article(&self, w: &mut DlWorker, host: &DlHost, join_group: bool) -> i32 {
        let cptr = self.conn_ptr();
        if cptr == 0 {
            return AD_FAILED;
        }
        let conn = unsafe { &*(cptr as *const crate::net::PooledNntpConn) };
        let conn_name = self.copy_conn_name();
        w.writing_started = false;
        w.local_write_error = false;
        w.content_rejected = false;
        if let Some(ref mut ca) = w.analyzer {
            ca.reset();
        }

        let mut dints = [0i32; 2];
        let mut dstrs = [std::ptr::null::<c_char>(); 1];
        host.raw(
            4,
            12,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            dints.as_mut_ptr(),
            std::ptr::null_mut(),
            dstrs.as_mut_ptr(),
        );
        let opts = crate::options::active_options();
        let (raw_article, crc_check, read_chunk, opt_skip_write) = match opts.as_deref() {
            Some(o) => {
                let s = o.scalars();
                (
                    s.raw_article != 0,
                    s.crc_check != 0,
                    s.article_read_chunk_size.max(1),
                    s.skip_write != 0,
                )
            }
            None => (false, true, 4096, false),
        };
        let groups_len = dints[0].max(0) as usize;
        let skip_disk_write = dints[1] != 0 || opt_skip_write;
        let message_id = unsafe { cstr_or_empty(dstrs[0]) };

        let is_stopped = || {
            host.raw(
                4,
                2,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) != 0
        };

        if join_group && groups_len > 0 {
            let mut last_resp: Option<Vec<u8>> = None;
            let mut auth_err = false;
            for gidx in 0..groups_len {
                let mut gstrs = [std::ptr::null::<c_char>(); 1];
                host.raw(
                    4,
                    12,
                    1,
                    0,
                    gidx,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    gstrs.as_mut_ptr(),
                );
                if !gstrs[0].is_null() {
                    let resp_ptr = unsafe { conn.join_group(gstrs[0]) };
                    auth_err = unsafe { conn.auth_error() };
                    last_resp = unsafe { opt_cstr(resp_ptr) };
                    if article_dl_is_join_group_ok(last_resp.as_deref()) {
                        break;
                    }
                }
            }
            let st = self.check_response(
                host,
                &conn_name,
                last_resp.as_deref(),
                auth_err,
                is_stopped(),
                b"could not join group",
            );
            if st != AD_FINISHED {
                return st;
            }
        }

        let cmd = article_dl_format_fetch_command(raw_article, &message_id);
        let cmd_c = nul_vec(&cmd);
        let resp_ptr = unsafe { conn.request(cmd_c.as_ptr().cast()) };
        let req_auth_err = unsafe { conn.auth_error() };
        let fetch_resp = unsafe { opt_cstr(resp_ptr) };
        let st = self.check_response(
            host,
            &conn_name,
            fetch_resp.as_deref(),
            req_auth_err,
            is_stopped(),
            b"could not fetch article",
        );
        if st != AD_FINISHED {
            return st;
        }

        w.decoder.clear();
        w.decoder.crc_check = crc_check;
        w.decoder.raw_mode = raw_article;

        let mut status = AD_RUNNING;
        let mut line_buf = vec![0u8; (read_chunk as usize) + 128];

        while !is_stopped() && !w.decoder.eof {
            if !is_stopped()
                && !w.writer.has_cached_buffer()
                && memory_monitor_tier() == PRESSURE_CRITICAL
                && CACHE_FLUSHING.load(Ordering::Acquire)
            {
                sleep_ms(10);
            }
            loop {
                if is_stopped() {
                    break;
                }
                let limit = crate::ffi::active_workstate().map_or(0, |ws| ws.speed_limit());
                if limit <= 0 {
                    break;
                }
                let over = crate::statmeter::active_statmeter().is_some_and(|sm| {
                    sm.current_download_speed() > i64::from(limit)
                        || sm.momentary_download_speed() > i64::from(limit)
                });
                if !over {
                    break;
                }
                self.last_update_time.store(now_sec(), Ordering::SeqCst);
                sleep_ms(10);
            }

            let (mut buf_ptr, avail) = unsafe { conn.drain_read_buffer() };
            let mut raw_len = avail as isize;
            if raw_len == 0 {
                let n = unsafe { conn.try_recv(&mut line_buf[..read_chunk as usize]) };
                raw_len = n as isize;
                buf_ptr = line_buf.as_mut_ptr();
            }
            if raw_len <= 0 {
                if !is_stopped() {
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[
                            b"Article ",
                            &self.info_name,
                            b" @ ",
                            &conn_name,
                            b" failed: Unexpected end of article",
                        ]),
                    );
                }
                conn.set_broken();
                unsafe {
                    conn.disconnect();
                }
                status = AD_FAILED;
                break;
            }

            let raw_len = raw_len as usize;
            if let Some(sm) = crate::statmeter::active_statmeter() {
                sm.add_speed_reading_locked(raw_len as i32);
            }
            let now_t = now_sec();
            let old_t = self.last_update_time.swap(now_t, Ordering::SeqCst);
            if old_t != now_t {
                self.add_conn_bytes(&mut w.downloaded_size);
            }

            let decoded_len = unsafe { w.decoder.decode_buffer(buf_ptr, raw_len) };
            if decoded_len > 0 {
                let decoded_slice = unsafe { std::slice::from_raw_parts(buf_ptr, decoded_len) };
                if !self.write_decoded(w, host, decoded_slice, raw_article, skip_disk_write) {
                    let (wst, crej) = article_dl_classify_write_failure(
                        i32::from(w.local_write_error),
                        AD_FATAL_ERROR,
                        AD_FAILED,
                    );
                    status = wst;
                    w.content_rejected = crej;
                    break;
                }
            }
        }

        if is_stopped() {
            status = AD_FAILED;
        }
        if status == AD_RUNNING {
            self.free_connection(host, &mut w.downloaded_size, true);
            status = self.decode_check(w, host, raw_article, crc_check);
        }
        if w.writing_started {
            w.writer.finish(host, status == AD_FINISHED);
        }
        if status == AD_FINISHED {
            host.log(
                MK_DETAIL,
                &join_c_slices(&[b"Successfully downloaded ", &self.info_name]),
            );
        }
        status
    }

    fn run(&self, host: &DlHost) {
        host.log(MK_DEBUG, b"Entering ArticleDownloader-loop");

        self.set_status(host, AD_RUNNING, false, false);

        host.raw(
            4,
            1,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );

        let final_status = {
            let mut w = self.worker.lock().unwrap_or_else(|e| e.into_inner());

            let mut init_ints = [0i32; 4];
            let mut init_i64s = [0i64; 1];
            host.raw(
                4,
                0,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                init_ints.as_mut_ptr(),
                init_i64s.as_mut_ptr(),
                std::ptr::null_mut(),
            );
            let force = init_ints[0] != 0;
            let extra_priority = init_ints[1] != 0;
            let has_desired_server = init_ints[2] != 0;
            let dupe_fallback_round = init_ints[3];
            let file_time = init_i64s[0];

            let (retries, interval_sec) = match crate::options::active_options().as_deref() {
                Some(o) => {
                    let s = o.scalars();
                    (
                        if s.article_retries > 0 {
                            s.article_retries
                        } else {
                            1
                        },
                        if s.article_interval > 0 {
                            s.article_interval
                        } else {
                            0
                        },
                    )
                }
                None => (1, 0),
            };
            let pool_init = crate::net::active_server_pool();
            let server_config_gen = pool_init.as_ref().map_or(0, |p| p.generation());
            let servers_cap = pool_init.as_ref().map_or(0, |p| p.server_count());
            drop(pool_init);

            let mut transient_retries = TransientRetries::new(retries, interval_sec);
            let mut failed_servers: Vec<usize> = Vec::with_capacity(servers_cap);
            let mut want_server: usize = 0;
            let mut level = 0i32;
            let mut status = AD_FAILED;

            let runtime_flags = || {
                let stopped = host.raw(
                    4,
                    2,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ) != 0;
                let (paused_or_quota, dl_held) = match crate::ffi::active_workstate().as_deref() {
                    Some(ws) => (
                        ws.pause_download() || ws.quota_reached(),
                        ws.download_held(),
                    ),
                    None => (false, false),
                };
                let (cur_gen, max_norm_lvl, srv_len) =
                    match crate::net::active_server_pool().as_deref() {
                        Some(p) => (p.generation(), p.max_norm_level(), p.server_count() as i32),
                        None => (0, 0, 0),
                    };
                (
                    stopped,
                    paused_or_quota,
                    dl_held,
                    cur_gen,
                    max_norm_lvl,
                    srv_len,
                    now_sec(),
                )
            };

            let try_acquire_conn = |lvl: i32, want_ptr: usize, ignore_ptrs: &[usize]| -> usize {
                let Some(pool) = crate::net::active_server_pool() else {
                    return 0;
                };
                let srvs = pool.servers_snapshot();
                let want_srv = if want_ptr != 0 {
                    srvs.iter()
                        .find(|s| (std::sync::Arc::as_ptr(s) as usize) == want_ptr)
                        .map(|s| s.as_ref())
                } else {
                    None
                };
                if want_ptr != 0 && want_srv.is_none() {
                    return 0;
                }
                let mut ign_refs = Vec::with_capacity(ignore_ptrs.len());
                if want_srv.is_none() {
                    for &ip in ignore_ptrs {
                        if let Some(s) = srvs
                            .iter()
                            .find(|s| (std::sync::Arc::as_ptr(s) as usize) == ip)
                        {
                            ign_refs.push(s.as_ref());
                        }
                    }
                }
                pool.get_connection(lvl, want_srv, &ign_refs) as usize
            };

            loop {
                let (stopped, _, _, _, _, _, _) = runtime_flags();
                if stopped {
                    break;
                }
                status = AD_FAILED;
                self.set_status(host, AD_WAITING, false, false);

                loop {
                    let (st_now, _, _, cur_gen, _, _, now_t) = runtime_flags();
                    if self.conn_ptr() != 0 || st_now || cur_gen != server_config_gen {
                        break;
                    }
                    if transient_retries.is_empty() {
                        let got = try_acquire_conn(level, want_server, &failed_servers);
                        if got != 0 {
                            self.set_conn_ptr(got);
                        }
                    } else {
                        let waiting = transient_retries.waiting_servers(now_t);
                        if !waiting.contains(&want_server) {
                            let mut ign_all =
                                Vec::with_capacity(failed_servers.len() + waiting.len());
                            ign_all.extend_from_slice(&failed_servers);
                            ign_all.extend_from_slice(&waiting);
                            let got = try_acquire_conn(level, want_server, &ign_all);
                            if got != 0 {
                                self.set_conn_ptr(got);
                            }
                        }
                    }
                    if self.conn_ptr() == 0 {
                        sleep_ms(5);
                    }
                }

                self.set_status(host, AD_RUNNING, true, false);

                let (st_now, paused_or_quota, dl_held, cur_gen, _, _, now_t) = runtime_flags();
                if st_now
                    || (paused_or_quota && !force)
                    || (dl_held && !extra_priority)
                    || cur_gen != server_config_gen
                {
                    status = AD_RETRY;
                    break;
                }

                let cptr = self.conn_ptr();
                if cptr == 0 {
                    status = AD_RETRY;
                    break;
                }
                let conn_ref = unsafe { &*(cptr as *const crate::net::PooledNntpConn) };
                unsafe {
                    conn_ref.prepare_for_use();
                }
                let srv = &conn_ref.server;
                let last_server = std::sync::Arc::as_ptr(srv) as usize;
                level = srv.norm_level.load(Ordering::SeqCst);
                let retention = srv.retention;
                let server_id = srv.id;
                let join_group = srv.join_group;
                let mut remained_retries = transient_retries.remained_retries(last_server);

                let conn_name = article_dl_format_connection_name(
                    srv.name.to_bytes(),
                    conn_ref.host_cstr().to_bytes(),
                );
                self.set_conn_name(conn_name.clone());

                let (retention_failure, file_age_days) =
                    article_dl_is_retention_failure(retention, file_time, now_t);
                if retention_failure {
                    let tail = format!(
                        " failed: out of server retention (file age: {file_age_days}, configured retention: {retention})"
                    );
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[
                            b"Article ",
                            &self.info_name,
                            b" @ ",
                            &conn_name,
                            tail.as_bytes(),
                        ]),
                    );
                    status = AD_FAILED;
                    self.free_connection(host, &mut w.downloaded_size, true);
                }

                let (st_before_conn, _, _, _, _, _, _) = runtime_flags();
                if self.conn_ptr() != 0 && !st_before_conn {
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[b"Downloading ", &self.info_name, b" @ ", &conn_name]),
                    );
                }

                let mut auth_rej = false;
                let mut connected = if self.conn_ptr() != 0 {
                    let c = unsafe { &*(self.conn_ptr() as *const crate::net::PooledNntpConn) };
                    let ok = unsafe { c.connect() };
                    auth_rej = unsafe { c.auth_rejected() };
                    ok
                } else {
                    false
                };
                let mut transient_retry_scheduled = false;
                let mut auth_rejected = false;
                if !connected && self.conn_ptr() != 0 && auth_rej {
                    if transient_retries.register(last_server, now_sec()) {
                        transient_retry_scheduled = true;
                    } else {
                        auth_rejected = true;
                    }
                }

                let (st_after_conn, _, _, _, _, _, _) = runtime_flags();
                if connected && !st_after_conn {
                    status = self.download_article(&mut w, host, join_group);
                    if status == AD_TRANSIENT_ERROR {
                        if transient_retries.register(last_server, now_sec()) {
                            status = AD_CONNECT_ERROR;
                            transient_retry_scheduled = true;
                        } else {
                            status = AD_FAILED;
                            remained_retries = 0;
                        }
                    }

                    let (rec, succ, fail) = article_dl_classify_attempt_stat(
                        status,
                        AD_FINISHED,
                        AD_FAILED,
                        AD_NOT_FOUND,
                        AD_CRC_ERROR,
                    );
                    if rec {
                        if let Some(ent) = w
                            .server_stats
                            .iter_mut()
                            .find(|(sid, _, _)| *sid == server_id)
                        {
                            let (ns, nf) = crate::downloadinfo::server_stat_apply_op(
                                ent.1, ent.2, succ, fail, 0, 0, 1, 2,
                            );
                            ent.1 = ns;
                            ent.2 = nf;
                        } else {
                            let (ns, nf) = crate::downloadinfo::server_stat_apply_op(
                                0, 0, succ, fail, 0, 0, 1, 2,
                            );
                            w.server_stats.push((server_id, ns, nf));
                        }
                        if let Some(sm) = crate::statmeter::active_statmeter() {
                            let off = crate::ffi::active_workstate()
                                .map_or(0, |ws| ws.local_time_offset());
                            sm.add_server_stats(0, fail as u32, succ as u32, server_id, off);
                        }
                    }
                }

                if self.conn_ptr() != 0 {
                    self.add_conn_bytes(&mut w.downloaded_size);
                }

                let (handled_rej, break_rej, next_st, next_crej, next_rem) =
                    article_dl_handle_content_rejected(
                        i32::from(w.content_rejected),
                        dupe_fallback_round,
                        status,
                        AD_FAILED,
                        remained_retries,
                    );
                if handled_rej {
                    self.free_connection(host, &mut w.downloaded_size, false);
                    status = next_st;
                    if break_rej {
                        break;
                    }
                    w.content_rejected = next_crej;
                    remained_retries = next_rem;
                }

                if !connected && self.conn_ptr() != 0 {
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[
                            b"Article ",
                            &self.info_name,
                            b" @ ",
                            &conn_name,
                            b" failed: could not establish connection",
                        ]),
                    );
                }

                let (new_conn, new_st, new_rem) =
                    article_dl_normalize_connect_and_decrement_retries(
                        connected,
                        status,
                        AD_CONNECT_ERROR,
                        AD_FAILED,
                        remained_retries,
                    );
                connected = new_conn;
                status = new_st;
                remained_retries = new_rem;
                transient_retries.set_remained_retries(last_server, remained_retries);

                let (st_end_iter, paused_end, held_end, gen_end, max_norm_lvl, _, _) =
                    runtime_flags();
                let mut optional_blocked = false;
                if !connected && self.conn_ptr() != 0 && !st_end_iter {
                    if let Some(pool) = crate::net::active_server_pool() {
                        let srvs = pool.servers_snapshot();
                        if let Some(s) = srvs
                            .iter()
                            .find(|s| (std::sync::Arc::as_ptr(s) as usize) == last_server)
                        {
                            pool.block_server(s);
                            optional_blocked = s.optional;
                        }
                    }
                }

                want_server = 0;
                if connected && status == AD_FAILED && remained_retries > 0 && !retention_failure {
                    want_server = last_server;
                } else {
                    self.free_connection(
                        host,
                        &mut w.downloaded_size,
                        status == AD_FINISHED || status == AD_NOT_FOUND,
                    );
                    if transient_retry_scheduled && has_desired_server {
                        want_server = last_server;
                    }
                }

                if status == AD_FINISHED || status == AD_FATAL_ERROR {
                    break;
                }

                if st_end_iter
                    || (paused_end && !force)
                    || (held_end && !extra_priority)
                    || gen_end != server_config_gen
                {
                    status = AD_RETRY;
                    break;
                }

                if want_server == 0 && has_desired_server {
                    status = AD_FAILED;
                    break;
                }

                let (advance_list, push_failed) = article_dl_should_advance_server_list(
                    want_server != 0,
                    connected,
                    retention_failure,
                    optional_blocked,
                    auth_rejected,
                );
                if advance_list {
                    if push_failed {
                        failed_servers.push(last_server);
                    }

                    let (cand_ptrs, cand_ids, cand_lvls, cand_grps, cand_avail) =
                        crate::net::active_server_pool()
                            .map(|pool| {
                                let list = pool.servers_snapshot();
                                let mut c_ptrs = Vec::with_capacity(list.len());
                                let mut c_ids = Vec::with_capacity(list.len());
                                let mut c_lvls = Vec::with_capacity(list.len());
                                let mut c_grps = Vec::with_capacity(list.len());
                                let mut c_avail = Vec::with_capacity(list.len());
                                for s in &list {
                                    c_ptrs.push(std::sync::Arc::as_ptr(s) as usize);
                                    c_ids.push(s.id);
                                    c_lvls.push(s.norm_level.load(Ordering::SeqCst));
                                    c_grps.push(s.group);
                                    let avail = s.active.load(Ordering::SeqCst)
                                        && s.max_connections != 0
                                        && !(s.optional && pool.is_server_blocked(s));
                                    c_avail.push(u8::from(avail));
                                }
                                (c_ptrs, c_ids, c_lvls, c_grps, c_avail)
                            })
                            .unwrap_or_default();

                    let mut f_ids = Vec::with_capacity(failed_servers.len());
                    let mut f_lvls = Vec::with_capacity(failed_servers.len());
                    let mut f_grps = Vec::with_capacity(failed_servers.len());
                    for &fp in &failed_servers {
                        if let Some(pos) = cand_ptrs.iter().position(|&p| p == fp) {
                            f_ids.push(cand_ids[pos]);
                            f_lvls.push(cand_lvls[pos]);
                            f_grps.push(cand_grps[pos]);
                        }
                    }

                    let all_failed = article_dl_all_servers_on_level_failed(
                        level,
                        &cand_ids,
                        &cand_lvls,
                        &cand_grps,
                        &cand_avail,
                        &f_ids,
                        &f_lvls,
                        &f_grps,
                    );
                    let (adv_lvl, next_lvl, break_all) =
                        article_dl_eval_level_exhaustion(all_failed, level, max_norm_lvl);
                    if adv_lvl {
                        let tail = format!(" @ all level {level} servers failed, increasing level");
                        host.log(
                            MK_DETAIL,
                            &join_c_slices(&[b"Article ", &self.info_name, tail.as_bytes()]),
                        );
                        level = next_lvl;
                    } else if break_all {
                        host.log(
                            MK_DETAIL,
                            &join_c_slices(&[
                                b"Article ",
                                &self.info_name,
                                b" @ all servers failed",
                            ]),
                        );
                        status = AD_FAILED;
                        break;
                    }
                }
            }

            self.free_connection(host, &mut w.downloaded_size, status == AD_FINISHED);

            let (st_final, _, _, _, _, _, _) = runtime_flags();
            status = article_dl_resolve_final_status(
                status,
                w.writer.duplicate,
                st_final,
                AD_FINISHED,
                AD_FAILED,
                AD_RETRY,
                AD_FATAL_ERROR,
            );
            if st_final {
                host.log(
                    MK_DETAIL,
                    &join_c_slices(&[b"Download ", &self.info_name, b" cancelled"]),
                );
            }
            if status == AD_FAILED && transient_retries.is_exhausted() {
                host.log(
                    MK_DETAIL,
                    &join_c_slices(&[
                        b"Download ",
                        &self.info_name,
                        b" failed: temporary server errors outlasted all attempts, the article may still exist",
                    ]),
                );
            } else if status == AD_FAILED {
                host.log(
                    MK_DETAIL,
                    &join_c_slices(&[b"Download ", &self.info_name, b" failed"]),
                );
            }
            status
        };

        self.set_status(host, final_status, false, true);
        host.log(MK_DEBUG, b"Exiting ArticleDownloader-loop");
    }
}

#[allow(clippy::too_many_arguments)]
pub fn article_dl_exec(
    ptr: *mut NzbgetRsArticleDownloader,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    host_cb: Option<NzbgetRsQueueSubsysHostFn>,
    ctx: *mut c_void,
) -> usize {
    let _ = a2;
    // Static ops that require no host callback
    match op {
        100 => {
            memory_monitor_init_options(a1 != 0);
            return 0;
        }
        102 => return memory_monitor_tier() as usize,
        103 => return memory_monitor_tier_name_c() as usize,
        104 => return memory_monitor_effective_cache_mb() as usize,
        105 => return memory_monitor_effective_par_buffer_mb(a1) as usize,
        106 => return memory_monitor_effective_downloads_limit(a1) as usize,
        107 => {
            MEM_ACTIVE.store(false, Ordering::Release);
            return 0;
        }
        112 => {
            cache_free(p1 as *mut u8, a1);
            return 0;
        }
        113 => {
            cache_wakeup_flush();
            return 0;
        }
        114 => return CACHE_ALLOCATED.load(Ordering::SeqCst),
        115 => {
            CACHE_FILE_INFO.store(p1, Ordering::SeqCst);
            return 0;
        }
        _ => {}
    }

    // Instance ops that require no host callback
    if !ptr.is_null() {
        let dl = unsafe { &*ptr };
        match op {
            10 => {
                let w = dl.worker.lock().unwrap_or_else(|e| e.into_inner());
                let ints_out = unsafe { std::slice::from_raw_parts_mut(p1 as *mut c_int, 4) };
                let i64s_out = unsafe { std::slice::from_raw_parts_mut(p2 as *mut i64, 4) };
                let strs_out =
                    unsafe { std::slice::from_raw_parts_mut(s1 as *mut *const c_char, 1) };
                ints_out[0] = dl.status.load(Ordering::SeqCst);
                ints_out[1] = i32::from(w.has_analyzer);
                ints_out[2] = i32::from(w.analyzer.as_ref().is_some_and(|ca| ca.par_file));
                if let Some(ref afn) = w.article_filename_c {
                    ints_out[3] = 1;
                    strs_out[0] = afn.as_ptr().cast();
                } else {
                    ints_out[3] = 0;
                    strs_out[0] = std::ptr::null();
                }
                i64s_out[2] = w.decoded_file_size;
                i64s_out[3] = w.downloaded_size as i64;
                return 1;
            }
            11 => {
                let mut w = dl.worker.lock().unwrap_or_else(|e| e.into_inner());
                if !w.has_analyzer {
                    return 0;
                }
                let ints_out = unsafe { std::slice::from_raw_parts_mut(p1 as *mut c_int, 1) };
                let strs_out =
                    unsafe { std::slice::from_raw_parts_mut(s1 as *mut *const c_char, 2) };
                let mut par_out = 0;
                if w.parcheck_enabled {
                    if let Some(ref mut ca) = w.analyzer {
                        let h = ca.finish();
                        let par_file = ca.par_file;
                        let set_id = ca.par_set_id;
                        w.hash16k_c = h;
                        if let Some(id) = set_id {
                            w.par_set_id_c = id;
                        }
                        par_out = i32::from(par_file);
                    }
                }
                ints_out[0] = par_out;
                strs_out[0] = w.hash16k_c.as_ptr().cast();
                strs_out[1] = w.par_set_id_c.as_ptr().cast();
                return 1;
            }
            14 => {
                let st = dl.status.load(Ordering::SeqCst);
                let lut = dl.last_update_time.load(Ordering::SeqCst);
                let mut tbuf = [0u8; 64];
                crate::util::util_format_time(lut, &mut tbuf);
                let tslice = c_bytes(&tbuf);
                let prefix = format!("      Download: status={st}, LastUpdateTime=");
                crate::log::log_active_bytes(
                    MK_INFO,
                    &join_c_slices(&[prefix.as_bytes(), tslice, b", InfoName=", &dl.info_name]),
                );
                return 0;
            }
            16 => {
                let cs = dl.conn_state.lock().unwrap_or_else(|e| e.into_inner());
                if cs.0 != 0 {
                    let conn = unsafe { &*(cs.0 as *const crate::net::PooledNntpConn) };
                    conn.set_suppress_errors(true);
                    conn.cancel();
                }
                return 0;
            }
            18 => return dl.status.load(Ordering::SeqCst) as usize,
            19 => return dl.last_update_time.load(Ordering::SeqCst) as usize,
            _ => {}
        }
    }

    let Some(cb) = host_cb else {
        return 0;
    };
    let host = DlHost { cb, ctx };

    // Static subsystem ops (`ptr` is null)
    if ptr.is_null() {
        return match op {
            20 => {
                let out_fn = unsafe { cstr_or_empty(s1) };
                match article_writer_create_output_file(&out_fn, p2 as i64) {
                    Ok(()) => 1,
                    Err(err) => {
                        if !err.is_empty() {
                            host.log_nzb(p1, MK_ERROR, &err);
                        }
                        0
                    }
                }
            }
            23 => {
                let old_dir = unsafe { cstr_or_empty(s1) };
                usize::from(move_completed_files_on_host(&host, p1, &old_dir))
            }
            101 => {
                memory_monitor_run(&host);
                0
            }
            110 => {
                article_cache_run(&host);
                0
            }
            111 => usize::from(article_cache_check_flush(&host, p1, a1 != 0)),
            _ => 0,
        };
    }

    let dl = unsafe { &*ptr };
    match op {
        0 => {
            dl.run(&host);
            0
        }
        12 => {
            // Merge per-article server_stats into FileInfo & NzbInfo (`case 51, a1 == 3`)
            let w = dl.worker.lock().unwrap_or_else(|e| e.into_inner());
            for &(sid, s, f) in &w.server_stats {
                let mut arr = [sid, s, f];
                host.raw(
                    4,
                    20,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    arr.as_mut_ptr(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
            0
        }
        13 => {
            let (ok, need_post) = {
                let mut w = dl.worker.lock().unwrap_or_else(|e| e.into_inner());
                w.writer.complete_file_parts(&host)
            };
            if need_post {
                host.raw(
                    4,
                    19,
                    7,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
            usize::from(ok)
        }
        15 => {
            // CancelHanging (`case 51, a1 == 6`)
            let cname = dl.copy_conn_name();
            host.log(
                MK_ERROR,
                &join_c_slices(&[
                    b"Cancelling hanging download ",
                    &dl.info_name,
                    b" @ ",
                    &cname,
                ]),
            );
            host.raw(
                4,
                21,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            0
        }
        17 => {
            // Re-entry while FileInfo::GuardOutputFile is held on C++ stack
            let mut w = dl.worker.lock().unwrap_or_else(|e| e.into_inner());
            w.writer.build_filenames_locked(&host);
            0
        }
        22 => {
            // Re-entry while DownloadQueue::Guard is held on C++ stack
            let mut w = dl.worker.lock().unwrap_or_else(|e| e.into_inner());
            w.writer.post_process_locked(&host);
            0
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn buffered_disk_file_flush_and_close_propagates_close_error() {
        let dir = std::env::temp_dir().join(format!(
            "rustyget-buffered-close-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let ok_path = dir.join("ok.bin");
        let fail_path = dir.join("fail.bin");

        let mut ok_file = BufferedDiskFile::open(ok_path.as_os_str().as_bytes(), false, 64, 0)
            .expect("open ok.bin");
        assert_eq!(ok_file.write_bytes(b"hello world"), 11);
        assert!(ok_file.flush_and_close());
        assert_eq!(
            std::fs::read(&ok_path).expect("read ok.bin"),
            b"hello world"
        );

        let mut fail_file = BufferedDiskFile::open(fail_path.as_os_str().as_bytes(), false, 64, 0)
            .expect("open fail.bin");
        assert_eq!(fail_file.write_bytes(b"delayed error"), 13);
        let committed = fail_file.flush_and_close_with(|fd| {
            unsafe {
                libc::close(fd);
            }
            -1
        });
        assert!(!committed);
        assert_eq!(
            std::fs::read(&fail_path).expect("read fail.bin"),
            b"delayed error"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_busy_marker_published_under_selection_lock_and_cleared_after_flush() {
        static OBSERVED_UNDER_LOCK: AtomicBool = AtomicBool::new(false);
        unsafe extern "C" fn mock_host(
            _ctx: *mut c_void,
            op: c_int,
            a1: c_int,
            _a2: c_int,
            _p1: usize,
            _p2: usize,
            _s1: *const c_char,
            _s2: *const c_char,
            _ints: *mut c_int,
            _i64s: *mut i64,
            strs: *mut *const c_char,
        ) -> usize {
            if op == 2 && a1 == 5 {
                let fake_file = 0xdead_beefusize;
                crate::ffi::nzbget_rs_article_dl_exec(
                    std::ptr::null_mut(),
                    115,
                    0,
                    0,
                    fake_file,
                    0,
                    std::ptr::null(),
                    None,
                    std::ptr::null_mut(),
                );
                // While simulated DownloadQueue::Guard() is still held inside op 2, 5:
                OBSERVED_UNDER_LOCK.store(cache_file_busy(fake_file), Ordering::SeqCst);
                if !strs.is_null() {
                    *strs = c"nzb/file.bin".as_ptr();
                }
                return fake_file;
            }
            0
        }

        OBSERVED_UNDER_LOCK.store(false, Ordering::SeqCst);
        CACHE_FILE_INFO.store(0, Ordering::SeqCst);
        let host = DlHost {
            cb: mock_host,
            ctx: std::ptr::null_mut(),
        };
        assert!(article_cache_check_flush(&host, 4096, false));
        assert!(OBSERVED_UNDER_LOCK.load(Ordering::SeqCst));
        assert!(!cache_file_busy(0xdead_beefusize));
    }
}
