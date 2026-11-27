//! Full Rust runtime backing `MemoryMonitor`, `ArticleCache`,
//! `ArticleWriter`, `RenameContentAnalyzer`, and `ArticleDownloader`.

use crate::queuesubsys_host::NzbRef;
use crate::cstr;
use crate::options::OptStr;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::{Seek, SeekFrom, Write};
use std::os::fd::{FromRawFd, IntoRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

use crate::artwritercore::{
    article_cache_should_flush, article_dl_format_connection_name, article_dl_format_fetch_command,
    article_writer_advance_and_check_range, article_writer_build_output_filenames,
    article_writer_classify_completion_status, article_writer_create_output_file_guarded,
    article_writer_execute_cleanup_old_data, article_writer_execute_setup_output_file,
    article_writer_finish_disk_ops, article_writer_flush_cache_guarded,
    article_writer_format_dest_file_path, article_writer_format_info_path,
    article_writer_move_completed_files, article_writer_post_process_rename_and_hardlink,
    article_writer_process_articles_counted, article_writer_resolve_direct_write_ready,
    calc_write_buffer_size, join_c_slices, memory_monitor_step, ArticleWriterFlushItem,
    ArticleWriterProcessItem, PRESSURE_CONSTRAINED, PRESSURE_CRITICAL, PRESSURE_NORMAL,
};
use crate::decoder::{Decoder, Format};
use crate::dupefallback::{
    article_downloader_check_response, article_downloader_validate_yenc, YencRangeError,
};
use crate::nntp::{
    article_dl_all_servers_on_level_failed, article_dl_classify_attempt_stat,
    article_dl_classify_decode_status, article_dl_classify_write_failure,
    article_dl_eval_level_exhaustion, article_dl_handle_content_rejected,
    article_dl_is_join_group_ok, article_dl_is_retention_failure,
    article_dl_normalize_connect_and_decrement_retries, article_dl_resolve_final_status,
    article_dl_should_advance_server_list, BackboneMiss, LevelCandidate, TransientRetries,
};
use crate::queuesubsys_host::{
    FileRef,
    ActiveDownloadSnap, OpArgs, QueueSubsysHost, AD_FAILED, AD_FATAL_ERROR, AD_FINISHED, AD_RETRY,
    AD_RUNNING, MK_DEBUG, MK_DETAIL, MK_ERROR, MK_INFO, MK_WARNING,
};
use crate::renameanalyzer::Analyzer;
use crate::status::DupeMode;

const AD_WAITING: i32 = 2;
const AD_CRC_ERROR: i32 = 6;
const AD_NOT_FOUND: i32 = 7;
const AD_CONNECT_ERROR: i32 = 8;
const AD_TRANSIENT_ERROR: i32 = 9;

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

fn nul_vec(bytes: &[u8]) -> Vec<u8> {
    let clean = cstr::until_nul(bytes);
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

struct DlHost<'a> {
    qh: &'a QueueSubsysHost,
    file_ptr: FileRef,
    article_ptr: usize,
    stopped: &'a AtomicBool,
}

impl DlHost<'_> {
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn op45(
        &self,
        a1: c_int,
        a2: c_int,
        p2: usize,
        s1: *const c_char,
        s2: *const c_char,
        ints: *mut c_int,
        i64s: *mut i64,
        strs: *mut *const c_char,
    ) -> usize {
        let x = OpArgs::NONE.a2(a2).p2(p2).s1(s1).s2(s2);
        let x = x.ints(ints).i64s(i64s).strs(strs);
        self.qh.output_op(self.file_ptr, a1, x)
    }

    fn with_output_guard<R, F: FnOnce() -> R>(&self, always_lock: bool, f: F) -> R {
        struct GuardCall<F, R> {
            f: Option<F>,
            out: Option<R>,
        }
        unsafe extern "C" fn tramp<F: FnOnce() -> R, R>(raw: *mut c_void) {
            let c = &mut *raw.cast::<GuardCall<F, R>>();
            if let Some(func) = c.f.take() {
                c.out = Some(func());
            }
        }
        let mut call = GuardCall {
            f: Some(f),
            out: None,
        };
        let mut i64s = [(&mut call as *mut GuardCall<F, R>) as usize as i64];
        self.op45(
            0,
            c_int::from(always_lock),
            tramp::<F, R> as *const () as usize,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
        call.out.expect("output_guard callback did not run")
    }

    #[inline]
    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    #[inline]
    fn log(&self, kind: i32, msg: &[u8]) {
        crate::log::log_active_bytes(kind, msg);
    }

    fn log_nzb(&self, nzb_ptr: NzbRef, kind: i32, msg: &[u8]) {
        if nzb_ptr.is_none() {
            crate::log::log_active_bytes(kind, msg);
        } else {
            self.qh.nzb_print_message(nzb_ptr, kind, msg);
        }
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
static CACHE_CONTENT_LOCK: Mutex<()> = Mutex::new(());

#[inline]
pub(crate) fn with_cache_content_lock<R, F: FnOnce() -> R>(f: F) -> R {
    let _g = CACHE_CONTENT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    f()
}

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

pub fn memory_monitor_tier_name_c() -> &'static CStr {
    match memory_monitor_tier() {
        PRESSURE_CRITICAL => c"CRITICAL",
        PRESSURE_CONSTRAINED => c"CONSTRAINED",
        _ => c"NORMAL",
    }
}

pub fn memory_monitor_effective_cache_mb() -> i32 {
    MEM_EFFECTIVE_CACHE_MB.load(Ordering::Acquire) as i32
}

pub(crate) fn cache_file_busy(file_ptr: usize) -> bool {
    file_ptr != 0 && file_ptr == CACHE_FILE_INFO.load(Ordering::SeqCst)
}

pub(crate) fn cache_free_raw(ptr: *mut u8, cap: i32) {
    cache_free(ptr, cap);
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
    // Every waiter: one woken by `notify_one` might be a cancelled live repair helper that
    // leaves without the lock, and the waiter it took the wakeup from would sleep on.
    CACHE_FLUSH_LOCK.1.notify_all();
}

/// Takes the flush lock unless `cancelled` turns true while waiting for it (checked before
/// and every 50 ms while waiting). False when cancelled: the lock is not held then.
fn cache_lock_flush_unless(cancelled: &dyn Fn() -> bool) -> bool {
    let mut locked = CACHE_FLUSH_LOCK.0.lock().unwrap_or_else(|e| e.into_inner());
    while *locked {
        if cancelled() {
            // Pass on a wakeup this waiter may have taken.
            CACHE_FLUSH_LOCK.1.notify_all();
            return false;
        }
        locked = CACHE_FLUSH_LOCK
            .1
            .wait_timeout(locked, std::time::Duration::from_millis(50))
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
    if cancelled() {
        CACHE_FLUSH_LOCK.1.notify_all();
        return false;
    }
    *locked = true;
    CACHE_FLUSHING.store(true, Ordering::Release);
    true
}

/// The flush lock as a fence for a live repair helper that may be cancelled while it waits:
/// a cancelled wait gives up (`None`) instead of blocking, so a caller that cancels the
/// helper under the queue lock never waits on a flusher that waits for that lock.
pub(crate) fn with_live_cache_fence_unless<T>(
    cancelled: impl Fn() -> bool,
    action: impl FnOnce() -> T,
) -> Option<T> {
    struct Fence;
    impl Drop for Fence {
        fn drop(&mut self) {
            cache_unlock_flush();
        }
    }
    if !cache_lock_flush_unless(&cancelled) {
        return None;
    }
    let _fence = Fence;
    Some(action())
}

/// Takes the flush lock the plain way (as the flusher and the downloaders do) and releases it.
#[cfg(test)]
pub(crate) fn lock_and_unlock_cache_flush_for_test() {
    cache_lock_flush();
    cache_unlock_flush();
}

#[cfg(test)]
pub(crate) fn hold_cache_flush_lock_for_test() -> impl Drop {
    struct Held;
    impl Drop for Held {
        fn drop(&mut self) {
            cache_unlock_flush();
        }
    }
    cache_lock_flush();
    Held
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
    if let Ok(flag_c) = CString::new(cstr::until_nul(&flag_p)) {
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
    crate::diskstate_format::delete_cache_flag(qdir, crate::paths::PATH_SEPARATOR);
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
                        write_acache_flag(opts.str_bytes(OptStr::QueueDir));
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
                    delete_acache_flag(opts.str_bytes(OptStr::QueueDir));
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

pub(crate) fn memory_monitor_init_options(reset_state: bool) {
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
            .str_cstr(OptStr::MemoryPressurePath)
            .and_then(|c| c.to_str().ok())
            .filter(|s| !s.is_empty());
        crate::memory_pressure::set_mock_root(path_opt);
    }
    let _ = memory_monitor_update_state();
}

static DAEMON_MEM_RUNNING: AtomicBool = AtomicBool::new(false);
static DAEMON_MEM_STOPPED: AtomicBool = AtomicBool::new(false);
static DAEMON_MEM_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

pub(crate) fn create_daemon_memory_monitor() {
    DAEMON_MEM_RUNNING.store(false, Ordering::SeqCst);
    DAEMON_MEM_STOPPED.store(false, Ordering::SeqCst);
    memory_monitor_init_options(true);
}

pub(crate) fn start_daemon_memory_monitor() {
    let mut g = DAEMON_MEM_HANDLE.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_some() {
        return;
    }
    DAEMON_MEM_STOPPED.store(false, Ordering::SeqCst);
    DAEMON_MEM_RUNNING.store(true, Ordering::SeqCst);
    if let Ok(h) = std::thread::Builder::new()
        .name("memory-monitor".into())
        .spawn(|| {
            struct RunningReset;
            impl Drop for RunningReset {
                fn drop(&mut self) {
                    DAEMON_MEM_RUNNING.store(false, Ordering::SeqCst);
                }
            }
            let _reset = RunningReset;
            let _tg = crate::ffi::RustThreadGuard::enter(&DAEMON_MEM_STOPPED);
            crate::log::log_active_bytes(MK_DEBUG, b"MemoryMonitor thread started");
            while !DAEMON_MEM_STOPPED.load(Ordering::SeqCst) {
                let interval = memory_monitor_update_state();
                let mut slept = 0;
                while !DAEMON_MEM_STOPPED.load(Ordering::SeqCst) && slept < interval {
                    sleep_ms(50);
                    slept += 50;
                }
            }
            crate::log::log_active_bytes(MK_DEBUG, b"MemoryMonitor thread stopped");
        })
    {
        *g = Some(h);
    } else {
        DAEMON_MEM_RUNNING.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn stop_daemon_memory_monitor() {
    DAEMON_MEM_STOPPED.store(true, Ordering::SeqCst);
}

pub(crate) fn is_daemon_memory_monitor_running() -> bool {
    DAEMON_MEM_RUNNING.load(Ordering::SeqCst)
}

pub(crate) fn shutdown_daemon_memory_monitor() {
    DAEMON_MEM_STOPPED.store(true, Ordering::SeqCst);
    let handle = DAEMON_MEM_HANDLE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(h) = handle {
        let _ = h.join();
    }
    DAEMON_MEM_RUNNING.store(false, Ordering::SeqCst);
    MEM_ACTIVE.store(false, Ordering::Release);
}

// ============================================================================
// ArticleCache Run + CheckFlush + FlushCache
// ============================================================================

static DAEMON_CACHE_RUNNING: AtomicBool = AtomicBool::new(false);
static DAEMON_CACHE_STOPPED: AtomicBool = AtomicBool::new(false);
static DAEMON_CACHE_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

pub(crate) fn create_daemon_article_cache() {
    DAEMON_CACHE_RUNNING.store(false, Ordering::SeqCst);
    DAEMON_CACHE_STOPPED.store(false, Ordering::SeqCst);
}

pub(crate) fn start_daemon_article_cache() {
    if !crate::options::active_options().is_some_and(|o| o.scalars().article_cache > 0) {
        return;
    }
    let mut g = DAEMON_CACHE_HANDLE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if g.is_some() {
        return;
    }
    DAEMON_CACHE_STOPPED.store(false, Ordering::SeqCst);
    DAEMON_CACHE_RUNNING.store(true, Ordering::SeqCst);
    if let Ok(h) = std::thread::Builder::new()
        .name("article-cache".into())
        .spawn(|| {
            struct RunningReset;
            impl Drop for RunningReset {
                fn drop(&mut self) {
                    DAEMON_CACHE_RUNNING.store(false, Ordering::SeqCst);
                }
            }
            let _reset = RunningReset;
            let _tg = crate::ffi::RustThreadGuard::enter(&DAEMON_CACHE_STOPPED);
            daemon_article_cache_run();
        })
    {
        *g = Some(h);
    } else {
        DAEMON_CACHE_RUNNING.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn stop_daemon_article_cache() {
    DAEMON_CACHE_STOPPED.store(true, Ordering::SeqCst);
    cache_wakeup_flush();
}

pub(crate) fn is_daemon_article_cache_running() -> bool {
    DAEMON_CACHE_RUNNING.load(Ordering::SeqCst)
}

pub(crate) fn shutdown_daemon_article_cache() {
    stop_daemon_article_cache();
    let handle = DAEMON_CACHE_HANDLE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(h) = handle {
        let _ = h.join();
    }
    DAEMON_CACHE_RUNNING.store(false, Ordering::SeqCst);
}

/// Test hook (functional harness only): `RUSTYGET_TEST_FLUSH_HOLD_MS` keeps each cache flush
/// holding its file's flush lock that long before it writes, as a slow disk at line speed
/// does, so articles of that file finish and are captured while it is flushed. Logged once
/// when set; unset (production) it costs one load.
fn flush_test_hold() {
    static HOLD_MS: std::sync::LazyLock<u64> = std::sync::LazyLock::new(|| {
        let ms = std::env::var("RUSTYGET_TEST_FLUSH_HOLD_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map_or(0, |ms| ms.min(1000));
        if ms > 0 {
            crate::log::log_active_bytes(
                MK_WARNING,
                format!("Test hook RUSTYGET_TEST_FLUSH_HOLD_MS: each cache flush waits {ms} ms").as_bytes(),
            );
        }
        ms
    });
    if *HOLD_MS > 0 {
        std::thread::sleep(std::time::Duration::from_millis(*HOLD_MS));
    }
}

/// Articles live PAR copied from the article cache while a flush of their file ran (all jobs;
/// before, those were refused and read back from disk). For the live PAR I/O line.
pub(crate) static SNAPSHOTS_DURING_FLUSH: AtomicU64 = AtomicU64::new(0);

/// Whether a cache flush of a file hands its written bytes to live PAR: only while a session
/// runs, and never for a par2 file. A par2 file is no source of the live set, and resolving
/// it against the live namespace invalidates the namespace, which stops live PAR for the job
/// ("writer identity differs from its captured row").
fn live_flush_reports(par_file: bool, has_session: bool) -> bool {
    has_session && !par_file
}

fn daemon_article_cache_check_flush(allocated: usize, flush_everything: bool) -> bool {
    let Some(qh) = crate::ffi::queue_host() else {
        return false;
    };
    crate::log::log_active_bytes(
        MK_DEBUG,
        format!(
            "Checking cache, Allocated: {}, FlushEverything: {}",
            allocated as i32,
            i32::from(flush_everything)
        )
        .as_bytes(),
    );

    // Take the flush lock before choosing a file. Selection marks the file
    // busy (CACHE_FILE_INFO) under the queue lock, and `delete_file_info`
    // waits for that mark while holding the queue lock; downloaders take the
    // flush lock first and the queue lock second. Waiting for the flush lock
    // after marking the file deadlocked all three (seen with many connections
    // and frequent DirectWrite flushes). Always flush lock, then queue lock.
    cache_lock_flush();
    let selected = qh.with_locked_queue(|dq_ptr| {
        let qlen = qh.queue_len(dq_ptr);
        for ni in 0..qlen {
            let nzb_ptr = qh.queue_at(dq_ptr, ni);
            let nzb = qh.nzb_snap(nzb_ptr, false);
            if nzb.skip_disk_write && !flush_everything {
                continue;
            }
            let flen = qh.nzb_file_len(nzb_ptr);
            for fi in 0..flen {
                let fp = qh.nzb_file_at(nzb_ptr, fi);
                let f = qh.file_snap(fp);
                // A file a live extraction waits on is written out at once (see `livewrite`).
                if f.cached_articles > 0
                    && (f.active_downloads == 0
                        || flush_everything
                        || crate::livewrite::wanted(nzb.id, f.id))
                {
                    let Ok(write_context) = crate::parlive::source_write_context(
                        nzb.id,
                        f.id,
                        &crate::fsops::path(&nzb.dest_dir),
                        &f.filename,
                    ) else {
                        continue;
                    };
                    CACHE_FILE_INFO.store(fp.addr(), Ordering::SeqCst);
                    let info_name = article_writer_format_info_path(
                        &nzb.name,
                        crate::paths::PATH_SEPARATOR,
                        &f.filename,
                    );
                    // Identity for live PAR's flush report, decided once here: copied only
                    // when a session runs now (one starting mid-flush catches up from the
                    // completion replay).
                    let session = crate::parchecker::streaming_session_has_session(nzb.id);
                    let live = live_flush_reports(f.par_file, session)
                        .then(|| (nzb.id, f.id, f.filename.clone(), f.hash16k.clone()));
                    // A recovery volume's written bytes are kept for its absorption at
                    // completion (`on_recovery_volume_bytes`), never read back.
                    let recovery = (f.par_file
                        && f.decoded_file_size > 0
                        && crate::parchecker::streaming_session_wants_recovery_bytes(nzb.id))
                    .then(|| (nzb.id, f.id, f.decoded_file_size as u64));
                    // Written before live PAR can take them (no session yet, or a recovery
                    // volume of unknown size): kept for it (`presession_keep`).
                    let early = (live.is_none()
                        && recovery.is_none()
                        && (!session || f.par_file)
                        && memory_monitor_tier() != PRESSURE_CRITICAL
                        && crate::parchecker::presession_wanted(nzb.par_size))
                    .then(|| (nzb.id, f.id, f.par_file, f.filename.clone(), f.hash16k.clone()));
                    return Some((fp, info_name, write_context, live, recovery, early, (nzb.id, f.id)));
                }
            }
        }
        None
    });

    let Some((file_ptr, info_name, write_context, live, recovery, early, (nzb_id, file_id))) =
        selected
    else {
        CACHE_FILE_INFO.store(0, Ordering::SeqCst);
        cache_unlock_flush();
        crate::log::log_active_bytes(MK_DEBUG, b"Checking cache... nothing to flush");
        return false;
    };

    struct ClearBusyGuard;
    impl Drop for ClearBusyGuard {
        fn drop(&mut self) {
            CACHE_FILE_INFO.store(0, Ordering::SeqCst);
        }
    }
    debug_assert_eq!(CACHE_FILE_INFO.load(Ordering::SeqCst), file_ptr.addr());
    let _busy_guard = ClearBusyGuard;
    crate::log::log_active_bytes(
        MK_DETAIL,
        &join_c_slices(&[b"Flushing cache for ", &info_name]),
    );

    let (direct_write, skip_disk_write, nzb_ptr, output_filename) =
        qh.file_cache_flush_prepare(file_ptr);
    let Some(metas) = qh.file_cache_flush_lock_collect(file_ptr) else {
        cache_unlock_flush();
        return true;
    };
    flush_test_hold();

    let items: Vec<ArticleWriterFlushItem<'_>> = metas
        .iter()
        .map(|m| ArticleWriterFlushItem {
            segment_offset: m.1,
            segment_size: m.2,
            segment_content: unsafe {
                if m.3.is_null() || m.2 <= 0 {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(m.3, m.2 as usize)
                }
            },
            result_filename: m.4.as_deref(),
        })
        .collect();

    let mut shapes = flush_shapes(&items);
    // Items counted without being written (live PAR certified their target): not reported to
    // live extraction as written; the finished file is read for them.
    let dropped = std::cell::RefCell::new(Vec::new());
    // Items whose write failed: their bytes are gone, the articles are downloaded again.
    let lost = std::cell::RefCell::new(Vec::new());
    let hand_off = (live.is_some() || recovery.is_some() || early.is_some()) && !skip_disk_write;
    let (flushed_articles, flushed_size) = flush_cache_then_hand_off(
        &items,
        hand_off,
        |discard, not_written| {
            article_writer_flush_cache_guarded(
                &items,
                direct_write,
                skip_disk_write,
                &output_filename,
                &write_context,
                || qh.file_cache_is_deleted_unparked(file_ptr),
                discard,
                |index| {
                    dropped.borrow_mut().push(index);
                    not_written(index);
                },
                |index| lost.borrow_mut().push(index),
                |msg| {
                    if nzb_ptr.is_some() {
                        qh.nzb_print_message(nzb_ptr, MK_ERROR, msg);
                    } else {
                        crate::log::log_active_bytes(MK_ERROR, msg);
                    }
                },
            )
        },
        |idx| qh.file_cache_discard_article_segment(file_ptr, metas[idx].0),
        |written_items, complete| {
            if let Some((nzb_id, file_id, par_file, name, hash16k)) = early.as_ref() {
                // A session that started since the file was chosen takes them as a write
                // that ran before its capture (its catch-up may have captured them already).
                if !par_file && crate::parchecker::streaming_session_has_session(*nzb_id) {
                    crate::parchecker::streaming_session_articles_written(
                        *nzb_id,
                        (Some(*file_id), name, hash16k),
                        direct_write,
                        written_items,
                    );
                } else {
                    crate::parchecker::presession_keep(*nzb_id, *file_id, written_items);
                }
            }
            if let Some((nzb_id, file_id, name, hash16k)) = live.as_ref() {
                crate::parchecker::streaming_session_articles_written(
                    *nzb_id,
                    (Some(*file_id), name, hash16k),
                    direct_write,
                    written_items,
                );
            }
            if let Some((nzb_id, file_id, size)) = recovery {
                if complete {
                    crate::parchecker::streaming_session_recovery_bytes_written(
                        nzb_id,
                        file_id,
                        size,
                        written_items,
                    );
                } else {
                    crate::parchecker::streaming_session_recovery_void(nzb_id, file_id);
                }
            }
        },
    );
    if direct_write && !skip_disk_write {
        // In their places in the volume: live extraction may read them now (see `livewrite`).
        for &index in dropped.borrow().iter() {
            if let Some(shape) = shapes.get_mut(index) {
                shape.2 = false;
            }
        }
        for (offset, len) in flushed_ranges(&shapes, flushed_articles, flushed_size) {
            crate::livewrite::written(nzb_id, file_id, offset, len);
        }
    }
    let lost: Vec<usize> = lost.borrow().iter().filter_map(|&i| metas.get(i).map(|m| m.0)).collect();
    // The lost articles stay in the file's cached count until they are handed back: a
    // completion of the file that starts meanwhile then waits for the flush lock instead of
    // reading them as finished (`articles_uncommitted`).
    let lost_n = i32::try_from(lost.len()).unwrap_or(i32::MAX);
    let skip_after = qh.file_cache_flush_finish(file_ptr, flushed_articles.saturating_sub(lost_n))
        || crate::options::active_options().is_some_and(|o| o.scalars().skip_write != 0);
    // The busy mark goes first: `delete_file_info` waits for it holding the queue lock,
    // which handing the lost articles back takes. The flush lock stays (flush lock, then
    // queue lock, as everywhere).
    drop(_busy_guard);
    if !lost.is_empty() {
        crate::queuecoord::articles_uncommitted(&qh, file_id, &lost);
    }
    cache_unlock_flush();
    let flushed_mb = (flushed_size as f64) / 1024.0 / 1024.0;
    let prefix = if skip_after {
        format!(
            "Discarded {flushed_articles} articles ({flushed_mb:.2} MB) from cache (SkipWrite) for "
        )
    } else {
        format!(
            "Saved {} articles ({flushed_mb:.2} MB) from cache into disk for ",
            flushed_articles.saturating_sub(lost_n)
        )
    };
    crate::log::log_active_bytes(MK_DETAIL, &join_c_slices(&[prefix.as_bytes(), &info_name]));
    true
}

/// Each flush item's (offset, size, whether its bytes are all there), taken before the flush
/// frees the items' contents.
fn flush_shapes(items: &[ArticleWriterFlushItem<'_>]) -> Vec<(i64, i32, bool)> {
    items
        .iter()
        .map(|item| {
            let whole = item.segment_content.len() as i64 == i64::from(item.segment_size);
            (item.segment_offset, item.segment_size, whole)
        })
        .collect()
}

/// How many of the counted flush items (their `sizes`) were written: the flush writes a prefix
/// and counts the failed write and every item after it without their sizes
/// (`article_writer_flush_cache_guarded`), so the prefix is the longest one whose sizes add up
/// to `flushed_size`.
fn written_prefix(sizes: impl Iterator<Item = i32>, flushed_size: i64) -> usize {
    let mut sum = 0i64;
    let mut good = 0;
    for (index, size) in sizes.enumerate() {
        sum += i64::from(size);
        if sum > flushed_size {
            break;
        }
        good = index + 1;
    }
    good
}

/// The (offset, length) of the flushed items (`shapes`, see `flush_shapes`) written whole, up
/// to the first failed write (see `written_prefix`).
fn flushed_ranges(shapes: &[(i64, i32, bool)], flushed_articles: i32, flushed_size: i64) -> Vec<(u64, u64)> {
    let written = usize::try_from(flushed_articles).unwrap_or(0).min(shapes.len());
    if written == 0 {
        return Vec::new();
    }
    let good = written_prefix(shapes[..written].iter().map(|s| s.1), flushed_size);
    shapes[..good]
        .iter()
        .filter(|s| s.2)
        .filter_map(|&(offset, size, _)| {
            Some((u64::try_from(offset).ok()?, u64::try_from(size).ok().filter(|&n| n > 0)?))
        })
        .collect()
}

/// The flushed items live PAR receives: those written, up to the first failed write, whose
/// bytes are all present, less those `not_written` (dropped because their target was
/// certified: counted, never written).
fn live_written_items<'a>(
    items: &[ArticleWriterFlushItem<'a>],
    flushed_articles: i32,
    flushed_size: i64,
    not_written: &[usize],
) -> Vec<(u64, &'a [u8])> {
    let written = usize::try_from(flushed_articles).unwrap_or(0).min(items.len());
    if written == 0 {
        return Vec::new();
    }
    let good = written_prefix(items[..written].iter().map(|item| item.segment_size), flushed_size);
    items[..good]
        .iter()
        .enumerate()
        .filter(|(index, _)| !not_written.contains(index))
        .map(|(_, item)| item)
        .filter(|item| item.segment_content.len() as i64 == i64::from(item.segment_size))
        .filter_map(|item| Some((u64::try_from(item.segment_offset).ok()?, item.segment_content)))
        .collect()
}

/// Commits a cache flush (`flush`, which calls the `discard` it is given for each item it
/// is done with, and `not_written` for each it counted without writing) and, when
/// `hand_off`, gives live PAR the bytes just written (`live`), so par-rs never reads them
/// back. `live`'s second argument says whether every item was written as it is (no failed
/// write, none dropped, all bytes present): a recovery volume's kept bytes need that.
///
/// `discard` frees an item's cache segment, the memory its `segment_content` points into.
/// With a hand-off the discards wait until `live` has copied the written bytes: freeing
/// them first left live PAR copying freed (for large articles unmapped) cache memory.
fn flush_cache_then_hand_off<'a>(
    items: &[ArticleWriterFlushItem<'a>],
    hand_off: bool,
    flush: impl FnOnce(&mut dyn FnMut(usize), &mut dyn FnMut(usize)) -> (i32, i64),
    mut discard: impl FnMut(usize),
    live: impl FnOnce(&[(u64, &'a [u8])], bool),
) -> (i32, i64) {
    if !hand_off {
        return flush(&mut discard, &mut |_| {});
    }
    let mut done: Vec<usize> = Vec::with_capacity(items.len());
    let mut not_written: Vec<usize> = Vec::new();
    let (flushed_articles, flushed_size) =
        flush(&mut |index| done.push(index), &mut |index| not_written.push(index));
    if flushed_articles > 0 {
        let written = live_written_items(items, flushed_articles, flushed_size, &not_written);
        let counted = usize::try_from(flushed_articles).unwrap_or(0).min(items.len());
        let complete = not_written.is_empty() && written.len() == counted;
        live(&written, complete);
    }
    for index in done {
        discard(index);
    }
    (flushed_articles, flushed_size)
}

fn daemon_article_cache_run() {
    let is_stopped_cb = || DAEMON_CACHE_STOPPED.load(Ordering::SeqCst);
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
    // Articles are malloc'd one by one; after a flush hand the freed heap
    // back to the OS once the cache is empty, so RSS follows what is held.
    // At most one trim per TRIM_EVERY: malloc_trim walks every arena.
    const TRIM_EVERY: std::time::Duration = std::time::Duration::from_secs(2);
    let mut trim_pending = false;
    let mut last_trim: Option<std::time::Instant> = None;
    loop {
        let (allocated, effective_limit, is_stopped, direct_write, tier) = snap();
        if is_stopped && allocated == 0 {
            break;
        }
        let (mut should_flush, flush_everything, need_emergency_flush) = article_cache_should_flush(
            allocated,
            effective_limit,
            tier,
            just_flushed,
            reset_counter,
            is_stopped,
            direct_write,
        );
        // A live extraction waits for articles held here: write them out now, not at the next
        // periodic flush (see `livewrite`).
        should_flush |= direct_write && allocated > 0 && crate::livewrite::any_wanted();
        if should_flush {
            just_flushed = daemon_article_cache_check_flush(allocated, flush_everything);
            trim_pending |= just_flushed;
            reset_counter = 0;
            let (alloc_now, _, stopped_now, _, _) = snap();
            if need_emergency_flush && alloc_now == 0 {
                crate::memory_pressure::trim_heap();
                trim_pending = false;
                last_trim = Some(std::time::Instant::now());
            } else if !just_flushed && !stopped_now && !need_emergency_flush {
                // Nothing could be written yet: cache bytes are reserved when
                // an article starts, so many connections' in-flight articles
                // alone can pass the 32 MiB DirectWrite hold. Back off rather
                // than scanning the queue every few milliseconds.
                sleep_ms(50);
            } else if alloc_now >= allocated || !just_flushed {
                // A "flush" that freed nothing (file busy, write blocked)
                // must not spin, also while stopping.
                sleep_ms(5);
            }
        } else if allocated == 0 {
            if trim_pending {
                let wait = last_trim.map_or(std::time::Duration::ZERO, |t| {
                    TRIM_EVERY.saturating_sub(t.elapsed())
                });
                if wait.is_zero() {
                    crate::memory_pressure::trim_heap();
                    trim_pending = false;
                    last_trim = Some(std::time::Instant::now());
                } else {
                    // Too soon after the last trim: come back when it is due
                    // (or earlier, when articles arrive) instead of sleeping
                    // until the next download and keeping the freed heap.
                    let guard = CACHE_ALLOC_LOCK.0.lock().unwrap_or_else(|e| e.into_inner());
                    if CACHE_ALLOCATED.load(Ordering::SeqCst) == 0 && !is_stopped_cb() {
                        drop(
                            CACHE_ALLOC_LOCK
                                .1
                                .wait_timeout(guard, wait)
                                .unwrap_or_else(|e| e.into_inner()),
                        );
                    }
                    reset_counter = 0;
                    continue;
                }
            }
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

#[cfg(test)]
struct MockDlHost {
    cb: crate::queuesubsys_host::NzbgetRsQueueSubsysHostFn,
    ctx: *mut c_void,
}

#[cfg(test)]
impl MockDlHost {
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

    fn log(&self, kind: i32, msg: &[u8]) {
        crate::log::log_active_bytes(kind, msg);
    }

    fn log_nzb(&self, _nzb_ptr: NzbRef, kind: i32, msg: &[u8]) {
        crate::log::log_active_bytes(kind, msg);
    }
}

#[cfg(test)]
fn article_cache_check_flush(host: &MockDlHost, allocated: usize, flush_everything: bool) -> bool {
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
    let file_ptr = FileRef::from_addr(host.raw(
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
    ));
    if file_ptr.is_none() {
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
    debug_assert_eq!(CACHE_FILE_INFO.load(Ordering::SeqCst), file_ptr.addr());
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
        file_ptr.addr(),
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
        let nzb_ptr = NzbRef::from_addr(i64s[1] as usize);
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
                file_ptr.addr(),
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

        let (flushed_articles, flushed_size) = crate::artwritercore::article_writer_flush_cache(
            &items,
            direct_write,
            skip_disk_write,
            &output_filename,
            || {
                host.raw(
                    2,
                    9,
                    -1,
                    file_ptr.addr(),
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
                    file_ptr.addr(),
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
            file_ptr.addr(),
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

// ============================================================================
// BufferedDiskFile (userspace write buffering over O_CLOEXEC fd)
// ============================================================================

struct BufferedDiskFile {
    write_context: Option<crate::parlive::SourceWriteContext>,
    file: Option<std::fs::File>,
    buf: Vec<u8>,
    cap: usize,
    pos: i64,
}

fn prepare_article_write_context(
    context: crate::parlive::SourceWriteContext,
    direct_write: bool,
    scratch_root: &std::path::Path,
    article_number: i32,
) -> std::io::Result<crate::parlive::SourceWriteContext> {
    if direct_write {
        Ok(context)
    } else {
        context.with_article_temporary(scratch_root, article_number)
    }
}

impl BufferedDiskFile {
    #[cfg(test)]
    fn open(
        path: &[u8],
        read_write: bool,
        write_buffer_kb: i32,
        rec_size: i32,
    ) -> Result<Self, i32> {
        Self::open_inner(path, read_write, write_buffer_kb, rec_size, None)
    }

    fn open_guarded(
        path: &[u8],
        read_write: bool,
        write_buffer_kb: i32,
        rec_size: i32,
        write_context: crate::parlive::SourceWriteContext,
    ) -> Result<Self, i32> {
        Self::open_inner(
            path,
            read_write,
            write_buffer_kb,
            rec_size,
            Some(write_context),
        )
    }

    fn open_inner(
        path: &[u8],
        read_write: bool,
        write_buffer_kb: i32,
        rec_size: i32,
        write_context: Option<crate::parlive::SourceWriteContext>,
    ) -> Result<Self, i32> {
        let (file, write_context) = if let Some(context) = write_context {
            let (file, context, _permit) = context
                .open_attested(crate::fsops::path(cstr::until_nul(path)), !read_write)
                .map_err(|error| error.raw_os_error().unwrap_or(libc::ECANCELED))?;
            if !read_write {
                file.set_len(0).map_err(|error| {
                    context.invalidate();
                    error.raw_os_error().unwrap_or(libc::EIO)
                })?;
            }
            (file, Some(context))
        } else {
            let clean = cstr::until_nul(path);
            let Ok(c_path) = CString::new(clean) else {
                return Err(libc::EINVAL);
            };
            let mode = if read_write { 1 } else { 2 };
            let fd = crate::fsops::open_cloexec_fd(&c_path, mode, 0, 1, 2);
            if fd < 0 {
                if let Some(context) = &write_context {
                    context.invalidate();
                }
                return Err(crate::fsops::last_errno());
            }
            let file = unsafe { std::fs::File::from_raw_fd(fd) };
            (file, None)
        };
        let calc = calc_write_buffer_size(write_buffer_kb, rec_size);
        let cap = if calc > 0 { calc as usize } else { 8192 };
        Ok(Self {
            write_context,
            file: Some(file),
            buf: Vec::with_capacity(cap.min(64 * 1024)),
            cap,
            pos: 0,
        })
    }

    fn position(&self) -> i64 {
        self.pos
    }

    fn write_all_fd(
        file: &mut std::fs::File,
        mut data: &[u8],
        context: Option<&crate::parlive::SourceWriteContext>,
    ) -> (usize, bool) {
        let _permit = match context
            .map(|context| context.descriptor_permit(file))
            .transpose()
        {
            Ok(permit) => permit,
            Err(_) => return (0, false),
        };
        let mut written = 0usize;
        while !data.is_empty() {
            match file.write(data) {
                Ok(0) => {
                    if let Some(context) = context {
                        context.invalidate();
                    }
                    return (written, false);
                }
                Ok(n) => {
                    written += n;
                    data = &data[n..];
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    if let Some(context) = context {
                        context.invalidate();
                    }
                    return (written, false);
                }
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
        let (_, ok) = Self::write_all_fd(file, &self.buf, self.write_context.as_ref());
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
            Err(_) => {
                if let Some(context) = &self.write_context {
                    context.invalidate();
                }
                false
            }
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
            let (written, _) = Self::write_all_fd(file, data, self.write_context.as_ref());
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
        if !closed {
            if let Some(context) = &self.write_context {
                context.invalidate();
            }
        }
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
    write_context: Option<crate::parlive::SourceWriteContext>,
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
    /// Direct write: the (offset, length) of every finished article of the file being
    /// completed, all in place once its output is flushed (reported to `livewrite`).
    proc_written: Vec<(u64, u64)>,
    post_crc: u32,
    post_final_path: Vec<u8>,
    post_orig_filename: Vec<u8>,
    post_nzb_dest_dir: Vec<u8>,
    /// Decoded bytes of an article written straight to disk, kept for live PAR's capture
    /// (charged to its ParBuffer cap) so par-rs never reads them back from the file.
    live_tee: Option<crate::parchecker::LiveTee>,
    /// The same for an article of a recovery volume (job, queue file, volume size, bytes):
    /// the completed volume is absorbed from RAM (`on_recovery_volume_bytes`).
    recovery_tee: Option<(i32, i32, u64, Vec<u8>)>,
    /// The same for an article live PAR cannot take yet (job, queue file, bytes): no
    /// session yet, or a recovery volume of unknown size (`presession_keep`).
    presession_tee: Option<(i32, i32, Vec<u8>)>,
    /// The file being completed, for live PAR (`complete_file_parts`).
    proc_live: Option<ProcLive>,
}

/// What live PAR needs to know of a file whose parts are being completed.
#[derive(Clone)]
struct ProcLive {
    nzb_id: i32,
    file_id: i32,
    par_file: bool,
    decoded_size: u64,
    filename: Vec<u8>,
    hash16k: Vec<u8>,
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
            write_context: None,
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
            proc_written: Vec::new(),
            post_crc: 0,
            post_final_path: Vec::new(),
            post_orig_filename: Vec::new(),
            post_nzb_dest_dir: Vec::new(),
            live_tee: None,
            recovery_tee: None,
            presession_tee: None,
            proc_live: None,
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

    fn build_filenames_locked(&mut self, host: &DlHost<'_>) {
        let mut ints = [0i32; 4];
        let mut strs = [std::ptr::null::<c_char>(); 3];
        host.op45(
            1,
            0,
            host.article_ptr,
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
            Some(opts) => (opts.scalars().direct_write != 0, opts.str_bytes(OptStr::TempDir).to_vec()),
            None => (false, Vec::new()),
        };
        let dest_dir = unsafe { cstr_or_empty(strs[0]) };
        let existing_out = unsafe { cstr_or_empty(strs[1]) };
        let file_filename = unsafe { cstr_or_empty(strs[2]) };

        let mut res_c_opt: Option<Vec<u8>> = None;
        let mut ofn_c_opt: Option<Vec<u8>> = None;
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
            res_c_opt = Some(nul_vec(&built.result_filename));
            self.temp_filename = built.temp_filename;
            if let Some(ofn) = built.output_filename {
                if built.update_file_info_output_filename {
                    ofn_c_opt = Some(nul_vec(&ofn));
                }
                self.output_filename = ofn;
            }
        }

        let mut rstrs = [std::ptr::null::<c_char>(); 1];
        host.op45(
            2,
            0,
            host.article_ptr,
            res_c_opt
                .as_ref()
                .map_or(std::ptr::null(), |v| v.as_ptr().cast()),
            ofn_c_opt
                .as_ref()
                .map_or(std::ptr::null(), |v| v.as_ptr().cast()),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            rstrs.as_mut_ptr(),
        );
        self.result_filename = unsafe { cstr_or_empty(rstrs[0]) };
    }

    #[allow(clippy::too_many_arguments)]
    fn start(
        &mut self,
        host: &DlHost<'_>,
        info_name: &[u8],
        format: Format,
        filename: Option<&[u8]>,
        file_size: i64,
        article_offset: i64,
        article_size: i32,
    ) -> bool {
        // Fence source identity before an old buffered-file drop can flush.
        // Snapshots copy strings out of the queue; hold its lock so a concurrent
        // edit (for example a new dupe key) cannot free them mid-copy.
        let (fsnap, nzb, art, write_context) =
            host.qh.with_locked_queue(|_| {
                let fsnap = host.qh.file_snap(host.file_ptr);
                let nzb = host.qh.nzb_snap(fsnap.nzb_ptr, false);
                let context = if fsnap.deleted || nzb.deleting {
                    None
                } else {
                    crate::parlive::source_write_context(
                        nzb.id,
                        fsnap.id,
                        crate::fsops::path(&nzb.dest_dir),
                        &fsnap.filename,
                    )
                    .ok()
                };
                if let Some(context) = &context {
                    let end = article_offset.checked_add(i64::from(article_size));
                    let unambiguous = format == Format::Yenc
                        && article_size > 0
                        && end.is_some_and(|end| end <= file_size)
                        && !nzb.many_dupe_files
                        && host.qh.claim_source_header(
                            host.file_ptr,
                            host.article_ptr,
                            article_offset,
                            end.unwrap_or(-1),
                        );
                    if !unambiguous && context.invalidate_identity() {
                        host.log_nzb(fsnap.nzb_ptr, MK_DETAIL,
                        b"Live PAR source identity changed before write; ordinary repair retained");
                    }
                }
                (fsnap, nzb, host.qh.article_snap(host.article_ptr), context)
            });
        self.out_file = None;
        self.drop_cached();
        self.live_tee = None;
        self.recovery_tee = None;
        self.presession_tee = None;
        self.write_context = write_context;
        if self.write_context.is_none() {
            return false;
        }
        let nzb_ptr = fsnap.nzb_ptr;
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
        let dupe_mode = nzb.dupe_mode;
        let many_dupe_files = nzb.many_dupe_files;
        let force_direct_write = fsnap.force_direct_write;
        let skip_disk_write = nzb.skip_disk_write || skip_write_opt;
        let art_info_size = art.size;
        let is_dupe_fallback = art.dupe_fallback_round > 0;
        let dest_dir = nzb.dest_dir;

        self.format = format;
        self.article_offset = article_offset;
        self.article_size = if article_size > 0 {
            article_size
        } else {
            art_info_size
        };
        self.article_ptr = 0;

        if self.format == Format::Yenc {
            if dupe_check && dupe_mode != DupeMode::Force && !many_dupe_files {
                let dest_file_path = filename.map(|fn_bytes| {
                    article_writer_format_dest_file_path(
                        &dest_dir,
                        crate::paths::PATH_SEPARATOR,
                        fn_bytes,
                    )
                });
                let dest_c = dest_file_path.as_deref().map(nul_vec);
                let mut d_ints = [0i32; 2];
                host.op45(
                    5,
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
                let out_fn_ref = &self.output_filename;
                let created = std::cell::Cell::new(false);
                let ok = host.with_output_guard(true, || {
                    let mut cints = [0i32; 4];
                    let mut cstrs = [std::ptr::null::<c_char>(); 3];
                    host.op45(
                        1,
                        0,
                        host.article_ptr,
                        std::ptr::null(),
                        std::ptr::null(),
                        cints.as_mut_ptr(),
                        std::ptr::null_mut(),
                        cstrs.as_mut_ptr(),
                    );
                    let cur_init = cints[3] != 0;
                    if !cur_init
                        || (!skip_disk_write
                            && !crate::fsops::file_exists(out_fn_ref, &crate::fsops::posix_stat))
                    {
                        if !skip_disk_write {
                            if let Err(err) = article_writer_create_output_file_guarded(
                                out_fn_ref,
                                file_size,
                                self.write_context.as_ref().unwrap(),
                            ) {
                                if !err.is_empty() {
                                    host.log_nzb(nzb_ptr, MK_ERROR, &err);
                                }
                                return false;
                            }
                            created.set(true);
                        }
                        host.op45(
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
                    true
                });
                if !ok {
                    return false;
                }
                // Live extraction reads this volume's articles as they are written (see
                // `livewrite`): the output exists at its decoded length from here on.
                if !skip_disk_write && !fsnap.par_file {
                    if let Ok(size) = u64::try_from(file_size) {
                        let final_path = article_writer_format_dest_file_path(
                            &dest_dir,
                            crate::paths::PATH_SEPARATOR,
                            cstr::until_nul(&fsnap.filename),
                        );
                        crate::livewrite::register(
                            nzb.id,
                            fsnap.id,
                            &final_path,
                            cstr::until_nul(&self.output_filename),
                            size,
                            created.get(),
                        );
                    }
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
            // No cache buffer: keep a copy for live PAR within its ParBuffer cap, so the
            // article's capture need not have par-rs read it back from disk.
            if !raw_article
                && self.format == Format::Yenc
                && !is_dupe_fallback
                && !fsnap.par_file
                && memory_monitor_tier() != PRESSURE_CRITICAL
            {
                self.live_tee = crate::parchecker::streaming_session_reserve_tee(
                    nzb.id,
                    u64::try_from(self.article_size).unwrap_or(0),
                );
                if self.live_tee.is_none()
                    && !crate::parchecker::streaming_session_has_session(nzb.id)
                    && crate::parchecker::presession_wanted(nzb.par_size)
                {
                    self.presession_tee = Some((
                        nzb.id,
                        fsnap.id,
                        Vec::with_capacity(usize::try_from(self.article_size).unwrap_or(0)),
                    ));
                }
            } else if !raw_article
                && self.format == Format::Yenc
                && !is_dupe_fallback
                && fsnap.par_file
                && fsnap.decoded_file_size > 0
                && crate::parchecker::streaming_session_wants_recovery_bytes(nzb.id)
            {
                self.recovery_tee = Some((
                    nzb.id,
                    fsnap.id,
                    fsnap.decoded_file_size as u64,
                    Vec::with_capacity(usize::try_from(self.article_size).unwrap_or(0)),
                ));
            } else if !raw_article
                && self.format == Format::Yenc
                && !is_dupe_fallback
                && fsnap.par_file
                && memory_monitor_tier() != PRESSURE_CRITICAL
                && crate::parchecker::presession_wanted(nzb.par_size)
            {
                // A recovery volume article before the session or the volume's size: kept
                // until the volume's tee takes it.
                self.presession_tee = Some((
                    nzb.id,
                    fsnap.id,
                    Vec::with_capacity(usize::try_from(self.article_size).unwrap_or(0)),
                ));
            }
            let direct_write = (direct_write_opt || force_direct_write)
                && self.format == Format::Yenc
                && !is_dupe_fallback;
            let out_filename = if direct_write {
                &self.output_filename
            } else {
                &self.temp_filename
            };
            let scratch_dir = crate::options::active_options()
                .map(|opts| opts.str_bytes(OptStr::TempDir).to_vec())
                .unwrap_or_default();
            if !direct_write {
                // An article temporary is attested against the job's source
                // root, which direct write would have created with the output
                // file. Without direct write nothing has created it yet.
                let mut errmsg = Vec::new();
                if !crate::fsops::force_directories(&dest_dir, &mut errmsg, &crate::fsops::posix_stat) {
                    host.log_nzb(
                        nzb_ptr,
                        MK_ERROR,
                        &join_c_slices(&[b"Could not create directory ", &dest_dir, b": ", &errmsg]),
                    );
                    return false;
                }
            }
            let context = match prepare_article_write_context(
                self.write_context.as_ref().unwrap().clone(),
                direct_write,
                crate::fsops::path(&scratch_dir),
                art.part_number,
            ) {
                Ok(context) => context,
                Err(_) => return false,
            };
            match BufferedDiskFile::open_guarded(
                out_filename,
                direct_write,
                write_buffer_kb,
                art_info_size,
                context,
            ) {
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
        let ok = f.write_bytes(buf) > 0;
        if let Some(tee) = self.live_tee.as_mut() {
            if ok && !raw_article {
                tee.push(buf);
            } else {
                self.live_tee = None;
            }
        }
        if let Some((nzb_id, file_id, _, bytes)) = self.recovery_tee.as_mut() {
            if ok && !raw_article {
                bytes.extend_from_slice(buf);
            } else {
                crate::parchecker::streaming_session_recovery_void(*nzb_id, *file_id);
                self.recovery_tee = None;
            }
        }
        if let Some((_, _, bytes)) = self.presession_tee.as_mut() {
            let room = usize::try_from(self.article_size).unwrap_or(0);
            if ok && !raw_article && bytes.len() + buf.len() <= room {
                bytes.extend_from_slice(buf);
            } else {
                self.presession_tee = None;
            }
        }
        ok
    }

    fn finish(&mut self, host: &DlHost<'_>, success: bool) {
        // Flushed here (not on drop) so a failed last write is known: such an article is never
        // reported to live extraction as written.
        let flushed = self.out_file.take().is_none_or(|mut f| f.flush_buf());

        // Snapshots copy strings out of the queue; hold its lock so a concurrent
        // edit (for example a new dupe key) cannot free them mid-copy.
        let (fsnap, nzb, art) = host.qh.with_locked_queue(|_| {
            let fsnap = host.qh.file_snap(host.file_ptr);
            let nzb = host.qh.nzb_snap(fsnap.nzb_ptr, false);
            (fsnap, nzb, host.qh.article_snap(host.article_ptr))
        });
        let nzb_ptr = fsnap.nzb_ptr;
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
        let force_direct_write = fsnap.force_direct_write;
        let skip_disk_write = nzb.skip_disk_write || skip_write_opt;
        let is_dupe_fallback = art.dupe_fallback_round > 0;
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

        let live_tee = self.live_tee.take();
        let recovery_tee = self.recovery_tee.take();
        let presession_tee = self.presession_tee.take();
        if !success || raw_article {
            // A partial direct write may have changed the file: a kept copy is stale.
            if let Ok(offset) = u64::try_from(self.article_offset) {
                crate::parchecker::presession_drop(nzb.id, fsnap.id, &[offset]);
            }
            self.drop_cached();
            return;
        }
        if let Some((nzb_id, file_id, size, bytes)) =
            recovery_tee.filter(|_| !has_cached && !skip_disk_write)
        {
            // Written whole (the file is closed above): the volume's absorption takes these.
            match u64::try_from(self.article_offset) {
                Ok(offset) => crate::parchecker::streaming_session_recovery_bytes_written(
                    nzb_id,
                    file_id,
                    size,
                    &[(offset, &bytes)],
                ),
                Err(_) => crate::parchecker::streaming_session_recovery_void(nzb_id, file_id),
            }
        }
        if !has_cached && !skip_disk_write {
            if let Ok(offset) = u64::try_from(self.article_offset) {
                match presession_tee {
                    // Written whole: kept for the session's capture or the volume's tee.
                    Some((nzb_id, file_id, bytes)) => {
                        crate::parchecker::presession_keep(nzb_id, file_id, &[(offset, &bytes)]);
                    }
                    // Written again without a copy kept: an older kept copy is stale.
                    None => crate::parchecker::presession_drop(nzb.id, fsnap.id, &[offset]),
                }
            }
        }
        if let Some(tee) = live_tee.filter(|_| !has_cached && !skip_disk_write) {
            // Written whole (the file is closed above): the capture takes these bytes.
            if let Ok(offset) = u64::try_from(self.article_offset) {
                tee.stash((Some(fsnap.id), &fsnap.filename, &fsnap.hash16k), offset);
            }
        }
        if direct_write && !has_cached && !skip_disk_write && flushed {
            // In its place in the volume: live extraction may read it now (see `livewrite`).
            if let (Ok(offset), Ok(len)) =
                (u64::try_from(self.article_offset), u64::try_from(self.article_ptr.min(self.article_size)))
            {
                crate::livewrite::written(nzb.id, fsnap.id, offset, len);
            }
        }

        if has_cached {
            let Some(context) = self.write_context.as_ref() else {
                self.drop_cached();
                return;
            };
            let Ok(_permit) = context.permit() else {
                self.drop_cached();
                return;
            };
            if self.article_ptr > 0 && self.article_ptr != self.cached_cap {
                let (np, nc) = cache_realloc(self.cached_ptr, self.cached_cap, self.article_ptr);
                self.cached_ptr = np;
                self.cached_cap = nc;
            }
            let ptr = self.cached_ptr;
            let mut ints_arr = [self.cached_cap, self.article_ptr];
            let mut i64s_arr = [self.article_offset, ptr as usize as i64];
            self.cached_ptr = std::ptr::null_mut();
            self.cached_cap = 0;
            with_cache_content_lock(|| {
                host.op45(
                    7,
                    0,
                    host.article_ptr,
                    std::ptr::null(),
                    std::ptr::null(),
                    ints_arr.as_mut_ptr(),
                    i64s_arr.as_mut_ptr(),
                    std::ptr::null_mut(),
                );
            });
        } else {
            let mut ints_arr = [self.article_ptr];
            let mut off = [self.article_offset];
            host.op45(
                7,
                1,
                host.article_ptr,
                std::ptr::null(),
                std::ptr::null(),
                ints_arr.as_mut_ptr(),
                off.as_mut_ptr(),
                std::ptr::null_mut(),
            );
        }
    }

    fn report_completion_status(
        host: &DlHost<'_>,
        nzb_ptr: NzbRef,
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
        host: &DlHost<'_>,
        nzb_ptr: NzbRef,
        articles_len: usize,
        is_hard_linked: bool,
        inter_dir_empty: bool,
        direct_write: bool,
        nzb_dest_dir: &[u8],
        final_output_path: &[u8],
    ) {
        let Some(context) = self.write_context.as_ref() else {
            return;
        };
        let mut raw_articles = Vec::with_capacity(articles_len);
        for idx in 0..articles_len {
            let mut ints = [0i32; 5];
            let mut i64s = [0i64; 1];
            let mut strs = [std::ptr::null::<c_char>(); 2];
            host.op45(
                9,
                4,
                idx,
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
            |a, b| cstr::until_nul(a) == cstr::until_nul(b),
            |p| crate::fsops::file_exists(p, &crate::fsops::posix_stat),
            |p| {
                context
                    .perform_io(|| std::fs::remove_file(crate::fsops::path(cstr::until_nul(p))))
                    .is_ok()
            },
            |from_b, to_b| {
                let (Ok(cf), Ok(ct)) = (CString::new(cstr::until_nul(from_b)), CString::new(cstr::until_nul(to_b)))
                else {
                    return false;
                };
                if context
                    .perform_io(|| {
                        if crate::fsops::move_file(&cf, &ct) {
                            Ok(())
                        } else {
                            Err(std::io::Error::last_os_error())
                        }
                    })
                    .is_ok()
                {
                    true
                } else {
                    move_error = crate::fsops::error_message(crate::fsops::last_errno());
                    false
                }
            },
            |dir_b| {
                let c_dir = CString::new(cstr::until_nul(dir_b)).ok();
                let mut browser = crate::fsops::PosixDirBrowser::new(c_dir.as_deref(), true);
                browser.next_ptr().is_null()
            },
            |dir_b| {
                context
                    .perform_io(|| std::fs::remove_dir(crate::fsops::path(cstr::until_nul(dir_b))))
                    .is_ok()
            },
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
            host.op45(
                8,
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

    fn process_articles_locked(&mut self, host: &DlHost<'_>) {
        let mut meta_ints = [0i32; 16];
        let mut meta_strs = [std::ptr::null::<c_char>(); 4];
        let nzb_ptr = NzbRef::from_addr(host.op45(
            9,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            meta_ints.as_mut_ptr(),
            std::ptr::null_mut(),
            meta_strs.as_mut_ptr(),
        ));
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
            let is_fin = host.op45(
                9,
                4,
                idx,
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
        // Cached articles written here go to live PAR before their segments are freed: a
        // recovery volume's into its tee, a source file's into recovery slots added after
        // its slices verified (`backfill_from_bytes`).
        let live = self
            .proc_live
            .clone()
            .filter(|_| !self.proc_raw_article && !self.proc_skip_disk_write)
            .filter(|live| crate::parchecker::streaming_session_wants_recovery_bytes(live.nzb_id));
        // A data file's cached articles written here are their last time in RAM: live PAR
        // keeps a copy for each one it has not captured yet (`articles_written`), so a
        // capture that comes after this write never has par-rs read it back.
        let live_source = self
            .proc_live
            .clone()
            .filter(|live| !live.par_file && !self.proc_raw_article && !self.proc_skip_disk_write)
            .filter(|live| crate::parchecker::streaming_session_has_session(live.nzb_id));
        let held = std::cell::RefCell::new(Vec::<(u64, Vec<u8>)>::new());
        let held_bytes = std::cell::Cell::new(0u64);
        let (crc, commit_err, failed_delta, short_copies) = article_writer_process_articles_counted(
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
                if let (Some(live), Some(content)) =
                    (live_source.as_ref(), items[item_idx].segment_content)
                {
                    if let Ok(offset) = u64::try_from(items[item_idx].segment_offset) {
                        // Not "in the file" yet: the write may still sit in the output
                        // buffer until the file closes, so nothing is handed over as readable.
                        crate::parchecker::streaming_session_articles_completed(
                            live.nzb_id,
                            (Some(live.file_id), &live.filename, &live.hash16k),
                            false,
                            &[(offset, content)],
                        );
                    }
                }
                if let (Some(live), Some(content)) = (live.as_ref(), items[item_idx].segment_content) {
                    if let Ok(offset) = u64::try_from(items[item_idx].segment_offset) {
                        if live.par_file && live.decoded_size > 0 {
                            crate::parchecker::streaming_session_recovery_bytes_written(
                                live.nzb_id,
                                live.file_id,
                                live.decoded_size,
                                &[(offset, content)],
                            );
                        } else if !live.par_file
                            && held_bytes.get() + content.len() as u64
                                <= crate::parchecker::HELD_BACKFILL_LIMIT
                            && crate::parchecker::streaming_session_needs_backfill(live.nzb_id)
                        {
                            // Copied before the segment is freed, at most what the back-fill
                            // may keep.
                            held_bytes.set(held_bytes.get() + content.len() as u64);
                            held.borrow_mut().push((offset, content.to_vec()));
                        }
                    }
                }
                let orig_idx = finished[item_idx].idx;
                host.op45(
                    9,
                    5,
                    orig_idx,
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
        if let Some(live) = live.as_ref() {
            if live.par_file && commit_err {
                crate::parchecker::streaming_session_recovery_void(live.nzb_id, live.file_id);
            }
            if !live.par_file && !commit_err {
                let held = held.into_inner();
                if !held.is_empty() {
                    let items: Vec<(u64, &[u8])> =
                        held.iter().map(|(o, b)| (*o, b.as_slice())).collect();
                    crate::parchecker::streaming_session_backfill_held(
                        live.nzb_id,
                        (Some(live.file_id), &live.filename, &live.hash16k),
                        &items,
                    );
                }
            }
        }
        // An article whose bytes could not be committed (a missing article temporary, whose
        // range assembly leaves as zeros, a temporary copied short, or a failed staged copy)
        // may have been verified by live PAR from RAM: its slices are no proof of what is on
        // disk any more.
        if commit_err || failed_delta > 0 || short_copies > 0 {
            if let Some(live) = self.proc_live.as_ref() {
                crate::parchecker::streaming_session_distrust(
                    live.nzb_id,
                    "an article of a completed file could not be committed",
                );
            }
        }
        if commit_err {
            self.commit_error = true;
        }
        self.proc_written = if self.proc_direct_write && !self.proc_skip_disk_write && !commit_err {
            finished
                .iter()
                .filter_map(|m| Some((u64::try_from(m.offset).ok()?, u64::try_from(m.size).ok()?)))
                .collect()
        } else {
            Vec::new()
        };
        if failed_delta > 0 {
            host.op45(
                9,
                6,
                failed_delta as usize,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
    }

    fn post_process_locked(
        &mut self,
        host: &DlHost<'_>,
    ) -> Result<(), crate::parlive::WriteBlocked> {
        let context = self
            .write_context
            .as_ref()
            .ok_or(crate::parlive::WriteBlocked::Closed)?;
        let _permit = context.try_permit()?;
        let cur_path_c = nul_vec(&self.post_final_path);
        let mut ints = [0i32; 2];
        let mut strs = [std::ptr::null::<c_char>(); 3];
        let nzb_ptr = NzbRef::from_addr(host.op45(
            9,
            8,
            self.post_crc as usize,
            cur_path_c.as_ptr().cast(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        ));
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
            host.op45(
                8,
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
            host.op45(
                8,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            return Ok(());
        }
        if cstr::until_nul(&self.post_nzb_dest_dir) != cstr::until_nul(&cur_nzb_dest_dir) {
            let _ = move_completed_files_on_qh(host.qh, nzb_ptr, &self.post_nzb_dest_dir);
        }
        Ok(())
    }

    fn complete_file_parts(&mut self, host: &DlHost<'_>) -> (bool, bool) {
        host.log(MK_DEBUG, b"Completing file parts");
        self.commit_error = false;
        let (write_context, proc_live) = host.qh.with_locked_queue(|_| {
            let file = host.qh.file_snap(host.file_ptr);
            let nzb = host.qh.nzb_snap(file.nzb_ptr, false);
            // A parked download keeps its partly downloaded files: assemble them too.
            if (file.deleted || nzb.deleting) && !nzb.parking {
                return (None, None);
            }
            let live = ProcLive {
                nzb_id: nzb.id,
                file_id: file.id,
                par_file: file.par_file,
                decoded_size: u64::try_from(file.decoded_file_size).unwrap_or(0),
                filename: file.filename.clone(),
                hash16k: file.hash16k.clone(),
            };
            let context = crate::parlive::source_write_context(
                nzb.id,
                file.id,
                crate::fsops::path(&nzb.dest_dir),
                &file.filename,
            )
            .ok();
            (context, Some(live))
        });
        self.write_context = write_context;
        self.proc_live = proc_live;
        if self.write_context.is_none() {
            return (false, false);
        }

        let mut ints = [0i32; 16];
        let mut strs = [std::ptr::null::<c_char>(); 4];
        let nzb_ptr = NzbRef::from_addr(host.op45(
            9,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            strs.as_mut_ptr(),
        ));
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
        // Hold a damaged RAR volume before it appears under its final name, so a
        // running direct unpack never reads it as healthy in the window before
        // the file-downloaded callback records its holes.
        if failed_articles > 0
            && crate::filetypes::may_hold_for_direct_unpack(&filename, success_articles)
            && host.qh.opts().direct_unpack
        {
            let nzb_id = host.qh.with_locked_queue(|_| {
                host.qh.nzb_snap(host.qh.file_snap(host.file_ptr).nzb_ptr, false).id
            });
            let full = [
                nzb_dest_dir.as_slice(),
                &[crate::paths::PATH_SEPARATOR],
                filename.as_slice(),
            ]
            .concat();
            crate::postproc::direct_unpack_hold(nzb_id, &full);
        }

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
            host.op45(
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
            host.op45(
                9,
                2,
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
            |a, b| cstr::until_nul(a) == cstr::until_nul(b),
            |dir_b, fn_b| {
                let cd = CString::new(cstr::until_nul(dir_b)).unwrap_or_default();
                let cf = CString::new(cstr::until_nul(fn_b)).unwrap_or_default();
                crate::fsops::make_unique_filename(&cd, &cf, &crate::fsops::posix_stat)
            },
            |p| crate::fsops::file_exists(p, &crate::fsops::posix_stat),
            |p| crate::fsops::force_directories(p, &mut force_dirs_err, &crate::fsops::posix_stat),
            |p| {
                self.write_context
                    .as_ref()
                    .unwrap()
                    .perform_io(|| std::fs::remove_file(crate::fsops::path(cstr::until_nul(p))))
                    .is_ok()
            },
            |p, rw| match BufferedDiskFile::open_guarded(
                p,
                rw,
                write_buffer_kb,
                0,
                self.write_context.as_ref().unwrap().clone(),
            ) {
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
                if let Ok(cp) = CString::new(cstr::until_nul(p)) {
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
            let written = std::mem::take(&mut self.proc_written);
            if direct_write && !written.is_empty() {
                // Every finished article is in place now, the ones held in the cache included:
                // live extraction reads them before the rename (see `livewrite`).
                let (nzb_id, file_id) = host.qh.with_locked_queue(|_| {
                    let file = host.qh.file_snap(host.file_ptr);
                    (host.qh.nzb_snap(file.nzb_ptr, false).id, file.id)
                });
                for (offset, len) in written {
                    crate::livewrite::written(nzb_id, file_id, offset, len);
                }
            }
            if !direct_write {
                let moved = self
                    .write_context
                    .as_ref()
                    .unwrap()
                    .perform_io(|| {
                        match (
                            CString::new(cstr::until_nul(&temp_dest_path)),
                            CString::new(cstr::until_nul(&final_output_path)),
                        ) {
                            (Ok(source), Ok(target))
                                if crate::fsops::move_file(&source, &target) =>
                            {
                                Ok(())
                            }
                            _ => Err(std::io::Error::last_os_error()),
                        }
                    })
                    .is_ok();
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
        host.op45(
            9,
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

pub(crate) fn move_completed_files_on_qh(
    qh: &QueueSubsysHost,
    nzb_ptr: NzbRef,
    old_dest_dir: &[u8],
) -> bool {
    if nzb_ptr.is_none() {
        return true;
    }
    let nzb = qh.nzb_snap(nzb_ptr, false);
    if nzb.skip_disk_write {
        return true;
    }
    if cstr::until_nul(old_dest_dir) != cstr::until_nul(&nzb.dest_dir) {
        crate::parlive::forget_download_writes(nzb.id);
        crate::livewrite::forget(nzb.id);
        crate::postproc::direct_unpack_join_live_par_repair(nzb.id);
        crate::parchecker::streaming_session_forget(nzb.id);
        release_unwritten_outputs(qh, nzb_ptr, cstr::until_nul(old_dest_dir));
    }
    let cfs = qh.nzb_completed_files(nzb_ptr);
    if cfs.is_empty() {
        return true;
    }
    let refs: Vec<&[u8]> = cfs.iter().map(|c| c.filename.as_slice()).collect();
    article_writer_move_completed_files(
        &nzb.dest_dir,
        cstr::until_nul(old_dest_dir),
        &refs,
        |msg| qh.nzb_print_message(nzb_ptr, MK_ERROR, msg),
        || qh.nzb_has_busy_output_file(nzb_ptr),
    )
}

/// After a download's folder changed (category or name edit, or an InterDir
/// relocation), a file whose direct-write output is still in the old folder
/// would keep writing there. The write guard refuses that path, because the
/// download's writes now belong to the new folder, so every later article of
/// the file failed and the download ended FAILURE with health 0 (6790: 2877,
/// 2880, 2887). A file that is not being written follows the download: one
/// holding no article forgets its output (it is created afresh in the new
/// folder; an empty preallocated `.out.tmp` goes), one holding articles has
/// its partial output moved into the new folder.
fn release_unwritten_outputs(qh: &QueueSubsysHost, nzb_ptr: NzbRef, old_dest_dir: &[u8]) {
    let new_dest_dir = qh.nzb_snap(nzb_ptr, false).dest_dir;
    for i in 0..qh.nzb_file_len(nzb_ptr) {
        let f = qh.file_snap(qh.nzb_file_at(nzb_ptr, i));
        let Some(follow) = output_follow(&f.output_filename, old_dest_dir, &f) else {
            continue;
        };
        let out = cstr::until_nul(&f.output_filename).to_vec();
        let new_out = match follow {
            OutputFollow::Forget => {
                if out.ends_with(b".out.tmp") {
                    let _ = std::fs::remove_file(crate::fsops::path(&out));
                }
                None
            }
            OutputFollow::Move => {
                let moved = crate::artwritercore::article_writer_format_dest_file_path(
                    cstr::until_nul(&new_dest_dir),
                    b'/',
                    &out[crate::paths::base_file_name(&out).min(out.len())..],
                );
                if crate::fsops::file_exists(&out, &crate::fsops::posix_stat) {
                    let _ = std::fs::create_dir_all(crate::fsops::path(cstr::until_nul(&new_dest_dir)));
                    let res = match (CString::new(out.clone()), CString::new(moved.clone())) {
                        (Ok(src), Ok(dst)) => crate::fsops::move_file_no_overwrite(&src, &dst),
                        _ => Err(libc::EINVAL),
                    };
                    if let Err(code) = res {
                        let mut msg = b"Could not move file ".to_vec();
                        msg.extend_from_slice(&out);
                        msg.extend_from_slice(b" to ");
                        msg.extend_from_slice(&moved);
                        msg.extend_from_slice(b": ");
                        msg.extend_from_slice(&crate::fsops::error_message(code));
                        qh.nzb_print_message(nzb_ptr, MK_ERROR, &msg);
                        continue;
                    }
                }
                Some(moved)
            }
        };
        qh.file_set_str(f.ptr, crate::queuesubsys_host::FILE_STR_OUTPUT_FILENAME, new_out.as_deref());
        if new_out.is_none() {
            qh.file_set_int(f.ptr, crate::queuesubsys_host::FILE_INT_OUTPUT_INITIALIZED, 0);
        }
        // The saved record holds the articles too: one written while they are
        // not loaded yet (after a restart) would hold none.
        if f.articles_len == 0 {
            qh.disk_load_articles(f.ptr);
        }
        if qh.file_snap(f.ptr).articles_len > 0 {
            qh.disk_save_file(f.ptr);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum OutputFollow {
    /// nothing written yet: forget the output, it is created in the new folder
    Forget,
    /// articles written: move the partial output into the new folder
    Move,
}

/// What a file whose `output` lies in `old_dest_dir` does when its download
/// moves to another folder; `None` when the output is elsewhere or the file is
/// being written right now (an article downloading or cached).
fn output_follow(
    output: &[u8],
    old_dest_dir: &[u8],
    f: &crate::queuesubsys_host::FileSnap,
) -> Option<OutputFollow> {
    let output = cstr::until_nul(output);
    let dir = cstr::until_nul(old_dest_dir);
    let dir = dir.strip_suffix(b"/").unwrap_or(dir);
    let inside = !dir.is_empty()
        && output.len() > dir.len() + 1
        && output.starts_with(dir)
        && output[dir.len()] == b'/';
    if !inside || f.cached_articles != 0 || f.active_downloads != 0 {
        None
    } else if f.success_articles == 0 {
        Some(OutputFollow::Forget)
    } else {
        Some(OutputFollow::Move)
    }
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
    /// the last fetch was answered `430 no such article` (not 411/420/451)
    last_430: bool,
    /// the message-id the last fetch asked for
    message_id: Vec<u8>,
    downloaded_size: i32,
    decoded_file_size: i64,
    /// A pooled download ran out of retries on temporary server errors.
    pooled_transient_exhausted: bool,
}

const AD_UNDEFINED: i32 = 0;

/// What [`NzbgetRsArticleDownloader::download_pooled`] needs to know about
/// the article's job.
#[cfg(unix)]
struct PooledJob {
    force: bool,
    extra_priority: bool,
    has_desired_server: bool,
    desired_server_id: i32,
    dupe_fallback_round: i32,
    file_time: i64,
    retries: i32,
    interval_sec: i32,
    server_config_gen: i32,
}

/// Paces a pooled download as the read loop did: every read counts on the
/// speed meter and keeps the download from looking stalled; while over the
/// speed limit (or yielding to repair fetches) the next read waits.
#[cfg(unix)]
struct DownloadPacer {
    dl: std::sync::Arc<NzbgetRsArticleDownloader>,
}

#[cfg(unix)]
impl nntp_rs::Pacer for DownloadPacer {
    fn received(&self, _server: usize, bytes: usize) -> std::time::Duration {
        if let Some(sm) = crate::statmeter::active_statmeter() {
            sm.add_speed_reading_locked(i32::try_from(bytes).unwrap_or(i32::MAX));
        }
        // the speed limit as one bucket for all connections; while repair
        // fetches wait, the download takes a quarter of the line
        let wait = crate::articlefetcher::paced_delay(bytes, crate::articlefetcher::PaceClass::Download);
        // running, and fresh until the wait ends (the hanging-download check
        // must not take a paced download for a stalled one)
        let until = now_sec() + i64::try_from(wait.as_secs()).unwrap_or(i64::MAX / 2) + 1;
        self.dl.last_update_time.store(until, Ordering::SeqCst);
        self.dl.status.store(AD_RUNNING, Ordering::SeqCst);
        wait
    }
}

pub struct NzbgetRsArticleDownloader {
    pub(crate) file_ptr: FileRef,
    pub(crate) article_ptr: usize,
    pub(crate) stopped: AtomicBool,
    info_name: Vec<u8>,
    status: AtomicI32,
    last_update_time: AtomicI64,
    /// When the current request started (`liveorder::now_ms`, 0: none yet): a request far
    /// slower than the others is asked again for a live extraction (see `liveorder`).
    running_since_ms: std::sync::atomic::AtomicU64,
    /// Who decides the current request's end: `REQ_READING` while its body is read, then either
    /// the reader (`REQ_BODY_DONE`: decoded and written from here on) or a restart for a live
    /// extraction (`REQ_RESTART`, see `restart_for_live`), whichever swaps first.
    request_state: std::sync::atomic::AtomicU8,
    conn_state: Mutex<(usize, Vec<u8>)>,
    worker: Mutex<DlWorker>,
    /// Cancels a fetch in flight through the shared nntp-rs pool ([`Self::stop`]).
    cancel: nntp_rs::CancelHandle,
}

/// `NzbgetRsArticleDownloader::request_state` values.
const REQ_READING: u8 = 0;
const REQ_BODY_DONE: u8 = 1;
const REQ_RESTART: u8 = 2;

impl Drop for NzbgetRsArticleDownloader {
    fn drop(&mut self) {
        crate::log::log_active_bytes(MK_DEBUG, b"Destroying ArticleDownloader");
    }
}

impl NzbgetRsArticleDownloader {
    pub(crate) fn new_for_queue(
        file_ptr: FileRef,
        article_ptr: usize,
        has_analyzer: bool,
        parcheck_enabled: bool,
        initial_conn: usize,
        info_name: Vec<u8>,
    ) -> Self {
        crate::log::log_active_bytes(MK_DEBUG, b"Creating ArticleDownloader");
        Self {
            file_ptr,
            article_ptr,
            stopped: AtomicBool::new(false),
            info_name,
            status: AtomicI32::new(AD_UNDEFINED),
            last_update_time: AtomicI64::new(now_sec()),
            running_since_ms: std::sync::atomic::AtomicU64::new(0),
            request_state: std::sync::atomic::AtomicU8::new(REQ_READING),
            conn_state: Mutex::new((initial_conn, Vec::new())),
            cancel: nntp_rs::CancelHandle::new(),
            worker: Mutex::new(DlWorker {
                decoder: Decoder::new(
                    crate::decoder::yenc_decode_piece,
                    crate::decoder::yenc_crc,
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
                last_430: false,
                message_id: Vec::new(),
                downloaded_size: 0,
                decoded_file_size: 0,
                pooled_transient_exhausted: false,
            }),
        }
    }

    pub(crate) fn status(&self) -> i32 {
        self.status.load(Ordering::SeqCst)
    }

    pub(crate) fn last_update_time(&self) -> i64 {
        self.last_update_time.load(Ordering::SeqCst)
    }

    /// When the request running now started (`liveorder::now_ms`); `None` before the first.
    pub(crate) fn running_since_ms(&self) -> Option<u64> {
        Some(self.running_since_ms.load(Ordering::SeqCst)).filter(|&ms| ms > 0)
    }

    /// Stops the request running now so its article is handed out again, for a live extraction
    /// (`queuecoord::restart_live_stragglers`); false when its body was read already (it is
    /// decoded and written, and a stop would fetch a committed article again). One swap decides
    /// it against the reader (`request_state`): a reader that loses stops as if stopped.
    pub(crate) fn restart_for_live(&self) -> bool {
        if self
            .request_state
            .compare_exchange(REQ_READING, REQ_RESTART, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return false;
        }
        self.stop();
        true
    }

    /// The text this download logs itself under (`nzb/file [part/count]`).
    pub(crate) fn info_name(&self) -> &[u8] {
        &self.info_name
    }

    pub(crate) fn stop(&self) {
        crate::log::log_active_bytes(MK_DEBUG, b"Trying to stop ArticleDownloader");
        self.stopped.store(true, Ordering::SeqCst);
        self.cancel.cancel();
        let cs = self.conn_state.lock().unwrap_or_else(|e| e.into_inner());
        if cs.0 != 0 {
            let conn = unsafe { &*(cs.0 as *const crate::net::PooledNntpConn) };
            conn.set_suppress_errors(true);
            conn.cancel();
        }
        drop(cs);
        crate::log::log_active_bytes(MK_DEBUG, b"ArticleDownloader stopped successfully");
    }

    pub(crate) fn snap_for_coord(&self) -> ActiveDownloadSnap {
        let w = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        let (has_afn, afn) = match w.article_filename_c.as_ref() {
            Some(v) => (true, cstr::until_nul(v).to_vec()),
            None => (false, Vec::new()),
        };
        ActiveDownloadSnap {
            file_ptr: self.file_ptr,
            article_ptr: self.article_ptr,
            status: self.status.load(Ordering::SeqCst),
            has_content_analyzer: w.has_analyzer,
            analyzed_par_file: w.analyzer.as_ref().is_some_and(|ca| ca.par_file()),
            has_article_filename: has_afn,
            decoded_file_size: w.decoded_file_size,
            downloaded_size: w.downloaded_size as i64,
            article_filename: afn,
        }
    }

    pub(crate) fn finish_analyzer(&self) -> Option<(bool, Vec<u8>, Vec<u8>)> {
        let mut w = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        if !w.has_analyzer {
            return None;
        }
        let mut par_out = false;
        if w.parcheck_enabled {
            if let Some(ref mut ca) = w.analyzer {
                let h = ca.finish();
                par_out = ca.par_file();
                let set_id = ca.par_set_id();
                w.hash16k_c = h;
                if let Some(id) = set_id {
                    w.par_set_id_c = id;
                }
            }
        }
        Some((
            par_out,
            cstr::until_nul(&w.hash16k_c).to_vec(),
            cstr::until_nul(&w.par_set_id_c).to_vec(),
        ))
    }

    pub(crate) fn log_debug(&self) {
        let st = self.status.load(Ordering::SeqCst);
        let lut = self.last_update_time.load(Ordering::SeqCst);
        let mut tbuf = [0u8; 64];
        crate::util::util_format_time(lut, &mut tbuf);
        let tslice = cstr::until_nul(&tbuf);
        let prefix = format!("      Download: status={st}, LastUpdateTime=");
        crate::log::log_active_bytes(
            MK_INFO,
            &join_c_slices(&[prefix.as_bytes(), tslice, b", InfoName=", &self.info_name]),
        );
    }

    pub(crate) fn merge_server_stats(&self, qh: &QueueSubsysHost) {
        let w = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        for &(sid, s, f) in &w.server_stats {
            let mut arr = [sid, s, f];
            qh.output_op(self.file_ptr, 10, OpArgs::NONE.ints(arr.as_mut_ptr()));
        }
    }

    pub(crate) fn complete_file_parts(&self, qh: &QueueSubsysHost) -> bool {
        let host = DlHost {
            qh,
            file_ptr: self.file_ptr,
            article_ptr: self.article_ptr,
            stopped: &self.stopped,
        };
        let (ok, need_post) = {
            let mut w = self.worker.lock().unwrap_or_else(|e| e.into_inner());
            w.writer.complete_file_parts(&host)
        };
        if need_post {
            loop {
                let result = qh.with_locked_queue(|_| {
                    let mut w = self.worker.lock().unwrap_or_else(|e| e.into_inner());
                    w.writer.post_process_locked(&host)
                });
                if !matches!(result, Err(crate::parlive::WriteBlocked::Busy)) {
                    break;
                }
                // Wait outside the queue, then take fresh postprocessing snapshots.
                let context = self
                    .worker
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .writer
                    .write_context
                    .clone();
                let Some(context) = context else {
                    break;
                };
                if context.permit().is_err() {
                    break;
                }
            }
        }
        ok
    }

    pub(crate) fn cancel_hanging(&self) {
        let cname = self.copy_conn_name();
        crate::log::log_active_bytes(
            MK_ERROR,
            &join_c_slices(&[
                b"Cancelling hanging download ",
                &self.info_name,
                b" @ ",
                &cname,
            ]),
        );
        self.stop();
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

    /// A pooled fetch's attempt has its connection: running from now, unless a
    /// restart for a live extraction already claimed this request (it stays
    /// claimed; the cancel ends the fetch).
    fn mark_attempt_running(&self) {
        if self.request_state.load(Ordering::SeqCst) == REQ_RESTART {
            return;
        }
        self.last_update_time.store(now_sec(), Ordering::SeqCst);
        self.running_since_ms
            .store(crate::liveorder::now_ms().max(1), Ordering::SeqCst);
        self.status.store(AD_RUNNING, Ordering::SeqCst);
    }

    fn set_status(&self, st: i32, touch_now: bool) {
        if touch_now {
            self.last_update_time.store(now_sec(), Ordering::SeqCst);
        }
        if st == AD_RUNNING && touch_now {
            self.request_state.store(REQ_READING, Ordering::SeqCst);
            self.running_since_ms
                .store(crate::liveorder::now_ms().max(1), Ordering::SeqCst);
        }
        self.status.store(st, Ordering::SeqCst);
    }

    fn free_connection(&self, host: &DlHost<'_>, downloaded_size: &mut i32, keep_connected: bool) {
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
            crate::net::return_connection(cptr, true);
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
                art_filename_opt = Some(cstr::until_nul(&w.decoder.article_filename).to_vec());
                if w.decoder.format == Format::Yenc {
                    let begin_pos = w.decoder.begin_pos;
                    let end_pos = w.decoder.end_pos;
                    article_file_size = w.decoder.size;

                    let mut vints = [0i32; 1];
                    let mut vi64s = [0i64; 3];
                    host.op45(
                        4,
                        0,
                        host.article_ptr,
                        std::ptr::null(),
                        std::ptr::null(),
                        vints.as_mut_ptr(),
                        vi64s.as_mut_ptr(),
                        std::ptr::null_mut(),
                    );
                    let dupe_round = vints[0];
                    let expected_file_size = vi64s[0];
                    // an unknown bound is stored as -1
                    let dupe_expected_offset = Some(vi64s[1]).filter(|&v| v >= 0);
                    let dupe_expected_end = Some(vi64s[2]).filter(|&v| v >= 0);

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
                        Err(YencRangeError::MissingBounds) => return false,
                        Err(YencRangeError::Malformed) => {
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
                        Err(
                            mismatch @ (YencRangeError::FileSizeMismatch
                            | YencRangeError::OffsetMismatch
                            | YencRangeError::EndMismatch),
                        ) => {
                            w.decoded_file_size = article_file_size;
                            let tail = match mismatch {
                                YencRangeError::FileSizeMismatch => format!(
                                    " from duplicate: file size mismatch ({article_file_size} vs {expected_file_size})"
                                ),
                                YencRangeError::OffsetMismatch => format!(
                                    " from duplicate: decoded offset mismatch ({} vs {})",
                                    begin_pos - 1,
                                    dupe_expected_offset.unwrap_or(-1)
                                ),
                                // EndMismatch; the other kinds are handled above
                                _ => format!(
                                    " from duplicate: decoded end mismatch ({end_pos} vs {})",
                                    dupe_expected_end.unwrap_or(-1)
                                ),
                            };
                            host.log(
                                MK_DETAIL,
                                &join_c_slices(&[
                                    b"Discarding article ",
                                    &self.info_name,
                                    tail.as_bytes(),
                                ]),
                            );
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
                    host.op45(
                        3,
                        crc as c_int,
                        host.article_ptr,
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
        w.last_430 = false;
        if let Some(ref mut ca) = w.analyzer {
            ca.reset();
        }

        host.op45(
            3,
            0,
            host.article_ptr,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        let (nzb_skip, art) = host.qh.with_locked_queue(|_| {
            let fsnap = host.qh.file_snap(host.file_ptr);
            let nzb_skip =
                fsnap.nzb_ptr.is_some() && host.qh.nzb_snap(fsnap.nzb_ptr, false).skip_disk_write;
            (nzb_skip, host.qh.article_snap(host.article_ptr))
        });
        let groups = if join_group {
            host.qh.file_groups(host.file_ptr)
        } else {
            Vec::new()
        };
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
        let skip_disk_write = nzb_skip || opt_skip_write;
        let message_id = art.message_id;
        w.message_id.clone_from(&message_id);

        if join_group && !groups.is_empty() {
            let mut last_resp: Option<Vec<u8>> = None;
            let mut auth_err = false;
            for grp in &groups {
                let grp_c = nul_vec(grp);
                let resp_ptr = unsafe { conn.join_group(grp_c.as_ptr().cast()) };
                auth_err = unsafe { conn.auth_error() };
                last_resp = unsafe { opt_cstr(resp_ptr) };
                if article_dl_is_join_group_ok(last_resp.as_deref()) {
                    break;
                }
            }
            let st = self.check_response(
                host,
                &conn_name,
                last_resp.as_deref(),
                auth_err,
                host.is_stopped(),
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
        w.last_430 = !req_auth_err && fetch_resp.as_deref().is_some_and(|r| r.starts_with(b"430"));
        let st = self.check_response(
            host,
            &conn_name,
            fetch_resp.as_deref(),
            req_auth_err,
            host.is_stopped(),
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

        while !host.is_stopped() && !w.decoder.eof {
            if !host.is_stopped()
                && !w.writer.has_cached_buffer()
                && memory_monitor_tier() == PRESSURE_CRITICAL
                && CACHE_FLUSHING.load(Ordering::Acquire)
            {
                sleep_ms(10);
            }
            loop {
                if host.is_stopped() {
                    break;
                }
                let limit = crate::ffi::active_workstate().map_or(0, |ws| ws.speed_limit());
                if limit <= 0 {
                    break;
                }
                // Repair fetches waiting for the line get most of it (see `download_must_wait`).
                let over = crate::statmeter::active_statmeter().is_some_and(|sm| {
                    crate::articlefetcher::download_must_wait(
                        i64::from(limit),
                        sm.current_download_speed(),
                        sm.momentary_download_speed(),
                    )
                });
                if !over {
                    break;
                }
                self.last_update_time.store(now_sec(), Ordering::SeqCst);
                sleep_ms(10);
            }

            let (mut buf, avail) = unsafe { conn.drain_read_buffer() };
            let mut raw_len = avail as isize;
            if raw_len == 0 {
                let n = unsafe { conn.try_recv(&mut line_buf[..read_chunk as usize]) };
                raw_len = n as isize;
                buf = &mut line_buf[..];
            }
            if raw_len <= 0 {
                if !host.is_stopped() {
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

            let decoded_len = w.decoder.decode_buffer(buf, raw_len);
            if decoded_len > 0 {
                if !self.write_decoded(w, host, &buf[..decoded_len], raw_article, skip_disk_write) {
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

        if self
            .request_state
            .compare_exchange(REQ_READING, REQ_BODY_DONE, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            // A restart for a live extraction won: this request ends as a stopped one.
            self.stopped.store(true, Ordering::SeqCst);
        }
        if host.is_stopped() {
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

    /// One article through the shared nntp-rs pool: nntp-rs picks the server
    /// (levels, blocked and paused servers, a 430 shared within a backend),
    /// and concurrent or recent requests for the same article share one body.
    /// Every server attempt is logged and counted as the per-connection
    /// download did. A copy the writer rejects (a dupe-fallback round's wrong
    /// bytes) is fetched again from another server; temporary failures are
    /// retried `ArticleRetries` times, `ArticleInterval` apart.
    #[cfg(unix)]
    fn download_pooled(
        self: &std::sync::Arc<Self>,
        w: &mut DlWorker,
        host: &DlHost,
        shared: &crate::nntprs_pool::Shared,
        job: PooledJob,
    ) -> i32 {
        let opts = crate::options::active_options();
        let (raw_article, crc_check, opt_skip_write) = match opts.as_deref() {
            Some(o) => {
                let s = o.scalars();
                (s.raw_article != 0, s.crc_check != 0, s.skip_write != 0)
            }
            None => (false, true, false),
        };
        drop(opts);
        let (nzb_skip, art) = host.qh.with_locked_queue(|_| {
            let fsnap = host.qh.file_snap(host.file_ptr);
            let nzb_skip =
                fsnap.nzb_ptr.is_some() && host.qh.nzb_snap(fsnap.nzb_ptr, false).skip_disk_write;
            (nzb_skip, host.qh.article_snap(host.article_ptr))
        });
        let skip_disk_write = nzb_skip || opt_skip_write;
        let message_id = art.message_id;
        w.message_id.clone_from(&message_id);
        let id = String::from_utf8_lossy(cstr::until_nul(&message_id)).into_owned();
        let only_server = if job.has_desired_server {
            match shared.index_of(job.desired_server_id).filter(|&i| {
                shared
                    .server(i)
                    .is_some_and(|s| s.active.load(Ordering::SeqCst) && s.max_connections > 0)
            }) {
                Some(i) => Some(i),
                None => {
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[
                            b"Article ",
                            &self.info_name,
                            format!(
                                " failed: desired server {} is not configured, switched off or has no connections",
                                job.desired_server_id
                            )
                                .as_bytes(),
                        ]),
                    );
                    return AD_FAILED;
                }
            }
        } else {
            None
        };
        // servers whose retention the file is older than are not asked (nntp-rs
        // leaves them out); logged as the per-server download did
        if only_server.is_none() {
            let now_t = now_sec();
            for idx in 0..shared.server_count() {
                let Some(srv) = shared.server(idx) else { continue };
                if !srv.active.load(Ordering::SeqCst) {
                    continue;
                }
                let (retention_failure, file_age_days) =
                    article_dl_is_retention_failure(srv.retention, job.file_time, now_t);
                if retention_failure {
                    let tail = format!(
                        " failed: out of server retention (file age: {file_age_days}, configured retention: {})",
                        srv.retention
                    );
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[b"Article ", &self.info_name, b" @ ", &shared.conn_name(idx), tail.as_bytes()]),
                    );
                }
            }
        }
        let posted = (job.file_time > 0)
            .then(|| std::time::UNIX_EPOCH + std::time::Duration::from_secs(job.file_time as u64));
        // servers whose copy was rejected: the next request skips them
        let mut rejected: Vec<usize> = Vec::new();
        // backbones whose 430 was logged
        let mut noted_backbones: Vec<i32> = Vec::new();
        // temporary errors so far: ArticleRetries requests in all, the waits
        // between them doubling from ArticleInterval up to a minute (as the
        // per-connection download's TransientRetries)
        let mut transient_errors = 0i32;
        let transient_exhausted;
        loop {
            let stopped = host.is_stopped();
            let (paused_or_quota, dl_held) = match crate::ffi::active_workstate().as_deref() {
                Some(ws) => (ws.pause_download() || ws.quota_reached(), ws.download_held()),
                None => (false, false),
            };
            let generation = crate::net::active_server_pool().map_or(0, |p| p.generation());
            if stopped
                || (paused_or_quota && !job.force)
                || (dl_held && !job.extra_priority)
                || generation != job.server_config_gen
            {
                return AD_RETRY;
            }
            // waiting (for a connection, or on servers that do not answer)
            // until bytes arrive: the pacer marks it running then, so the
            // hanging-download check sees only a stalled body
            self.set_status(AD_WAITING, true);
            // a new fetch of this article (after a rejected copy) is read again
            let _ = self.request_state.compare_exchange(
                REQ_BODY_DONE,
                REQ_READING,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
            w.writing_started = false;
            w.local_write_error = false;
            w.content_rejected = false;
            w.last_430 = false;
            if let Some(ref mut ca) = w.analyzer {
                ca.reset();
            }
            host.op45(
                3,
                0,
                host.article_ptr,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            let missing_epoch = crate::articlefetcher::missing_epoch();
            let attempts = crate::nntprs_pool::AttemptLog::default();
            let fetch_opts = nntp_rs::FetchOptions {
                cancel: Some(self.cancel.clone()),
                posted,
                only_server,
                skip_servers: rejected.clone(),
                priority: if job.extra_priority || job.force {
                    nntp_rs::Priority::Next
                } else {
                    nntp_rs::Priority::Bulk
                },
                skip_crc_check: !crc_check,
                // RawArticle=yes wrote the ARTICLE reply exactly as it arrived
                // (still dot-stuffed, with its terminator)
                body: if raw_article {
                    nntp_rs::BodyMode::Wire
                } else {
                    nntp_rs::BodyMode::Decode
                },
                observer: Some(attempts.observer_with(
                    shared,
                    Some({
                        let dl = std::sync::Arc::clone(self);
                        std::sync::Arc::new(move || dl.set_status(AD_WAITING, true))
                    }),
                    // a connection in hand: running, as the per-connection
                    // download was once connected (the hanging-download check
                    // and a live extraction's straggler re-ask go by this)
                    Some({
                        let dl = std::sync::Arc::clone(self);
                        std::sync::Arc::new(move || dl.mark_attempt_running())
                    }),
                    None,
                )),
                pacer: Some(nntp_rs::Pace::new(DownloadPacer {
                    dl: std::sync::Arc::clone(self),
                })),
                ..Default::default()
            };
            let got = shared.pool.fetch_shared(&id, &fetch_opts);
            let recs = attempts.take();
            self.log_attempts(w, host, shared, &recs, &mut noted_backbones);
            let d = match got {
                Ok(d) => {
                    // the body is read: from here on it is decoded and written,
                    // unless a restart for a live extraction swapped first (then
                    // this request ends as a stopped one, as on a cancel)
                    if self
                        .request_state
                        .compare_exchange(REQ_READING, REQ_BODY_DONE, Ordering::SeqCst, Ordering::SeqCst)
                        .is_err()
                    {
                        self.stopped.store(true, Ordering::SeqCst);
                        return AD_RETRY;
                    }
                    d
                }
                Err(nntp_rs::Error::Cancelled) => return AD_RETRY,
                Err(nntp_rs::Error::NotFound) => {
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[b"Article ", &self.info_name, b" @ all servers failed"]),
                    );
                    if !host.is_stopped()
                        && rejected.is_empty()
                        && only_server.is_none()
                        && crate::nntprs_pool::shared().is_some_and(|now| std::ptr::eq(&*now, &*shared))
                        && crate::nntprs_pool::missing_everywhere(shared, &recs)
                    {
                        crate::articlefetcher::note_missing_everywhere_since(&w.message_id, missing_epoch);
                    }
                    return AD_FAILED;
                }
                Err(nntp_rs::Error::Unavailable) if crate::nntprs_pool::missing_on_usable(shared, &recs) => {
                    // every server switched on said 430 or 451; the switched-off ones
                    // were never asked (as before), so the article is not found here
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[b"Article ", &self.info_name, b" @ all servers failed"]),
                    );
                    return AD_FAILED;
                }
                Err(e)
                    if e.is_retryable()
                        && crate::nntprs_pool::wait_for_required(
                            shared,
                            &recs,
                            matches!(
                                e,
                                nntp_rs::Error::Unavailable
                                    | nntp_rs::Error::Io { .. }
                                    | nntp_rs::Error::ConnectionLimit { .. }
                            ),
                            &rejected,
                        ) =>
                {
                    // the required news server could not be connected to (down,
                    // refusing, or blocked for RetryInterval after that): no
                    // article retry is spent, as the per-connection download
                    // spent none on a connection it could not establish and
                    // waited for the blocked server; the loop's top still ends
                    // the wait on a stop, a pause or a server change
                    let mut waited = 0;
                    while waited < 10 && !host.is_stopped() {
                        sleep_ms(100);
                        waited += 1;
                    }
                    continue;
                }
                Err(e) if e.is_retryable() && !matches!(e, nntp_rs::Error::Decode(_)) => {
                    transient_errors += 1;
                    if transient_errors >= job.retries.max(1) {
                        transient_exhausted = true;
                        break;
                    }
                    let mut interval = job.interval_sec.max(0);
                    for _ in 1..transient_errors {
                        if interval >= crate::nntp::TransientRetries::MAX_INTERVAL_SEC {
                            break;
                        }
                        interval = interval.saturating_mul(2);
                    }
                    let interval = interval.min(crate::nntp::TransientRetries::MAX_INTERVAL_SEC);
                    let mut waited = 0;
                    while waited < interval * 10 && !host.is_stopped() {
                        sleep_ms(100);
                        waited += 1;
                    }
                    continue;
                }
                Err(_) => {
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[b"Article ", &self.info_name, b" @ all servers failed"]),
                    );
                    return AD_FAILED;
                }
            };
            let conn_name = shared.conn_name(d.server);
            self.set_conn_name(conn_name.clone());
            if recs.is_empty() {
                // a body another request fetched (shared, or kept from just now)
                host.log(
                    MK_DETAIL,
                    &join_c_slices(&[b"Downloading ", &self.info_name, b" @ ", &conn_name, b" (shared)"]),
                );
            }
            let written = self.write_decoded_from(w, host, &d, raw_article, skip_disk_write);
            let status = if written {
                self.decode_check_from(w, host, &d, raw_article, crc_check)
            } else {
                let (wst, crej) = article_dl_classify_write_failure(
                    i32::from(w.local_write_error),
                    AD_FATAL_ERROR,
                    AD_FAILED,
                );
                w.content_rejected = crej;
                wst
            };
            if status != AD_FATAL_ERROR {
                if let Some(srv) = shared.server(d.server) {
                    let ok = status == AD_FINISHED;
                    self.count_server_article(w, srv.id, i32::from(ok), i32::from(!ok));
                }
            }
            if w.writing_started {
                w.writer.finish(host, status == AD_FINISHED);
            }
            if status == AD_FINISHED {
                host.log(
                    MK_DETAIL,
                    &join_c_slices(&[b"Successfully downloaded ", &self.info_name]),
                );
                return AD_FINISHED;
            }
            if status == AD_FATAL_ERROR {
                return status;
            }
            if w.content_rejected {
                if job.dupe_fallback_round > 0 || only_server.is_some() {
                    return AD_FAILED;
                }
                // another copy, from another server (skip_servers also skips the cached body)
                rejected.push(d.server);
                if rejected.len() >= crate::nntprs_pool::usable_count(shared) {
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[b"Article ", &self.info_name, b" @ all servers failed"]),
                    );
                    return AD_FAILED;
                }
                continue;
            }
            // a decode failure after the pool accepted the body (range exceeded)
            rejected.push(d.server);
            if rejected.len() >= crate::nntprs_pool::usable_count(shared) {
                return AD_FAILED;
            }
        }
        w.pooled_transient_exhausted = transient_exhausted;
        AD_FAILED
    }

    /// Logs a pooled fetch's server attempts as the per-connection download
    /// did, and counts each on the server's article statistics.
    #[cfg(unix)]
    fn log_attempts(
        &self,
        w: &mut DlWorker,
        host: &DlHost,
        shared: &crate::nntprs_pool::Shared,
        attempts: &[crate::nntprs_pool::Rec],
        noted_backbones: &mut Vec<i32>,
    ) {
        use crate::nntp::article_dl_classify_attempt_stat;
        use crate::nntprs_pool::{FailKind, Seen};
        for a in attempts {
            let conn_name = shared.conn_name(a.server);
            let Some(srv) = shared.server(a.server) else {
                continue;
            };
            host.log(
                MK_DETAIL,
                &join_c_slices(&[b"Downloading ", &self.info_name, b" @ ", &conn_name]),
            );
            let status = match (&a.outcome, a.fail) {
                (Seen::Have, _) => AD_FINISHED,
                (Seen::Missing | Seen::Removed, _) => {
                    // a 451 is not found here too (as the per-server download took
                    // it) but no 430: it leaves the last-430 mark alone. nntp-rs
                    // v0.4.4 still passes over the server's backbone after it, so
                    // the backbone line below stays true (OPEN ITEM G21 at nntp-rs)
                    let removed = a.outcome == Seen::Removed;
                    if !removed {
                        w.last_430 = true;
                    }
                    let reply = a
                        .reply
                        .as_deref()
                        .unwrap_or(if removed { "451 Article removed" } else { "430 No Such Article" });
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[
                            b"Article ",
                            &self.info_name,
                            b" @ ",
                            &conn_name,
                            b" failed, could not fetch article: ",
                            reply.as_bytes(),
                        ]),
                    );
                    let backbone = srv.group();
                    if backbone > 0 && !noted_backbones.contains(&backbone) {
                        noted_backbones.push(backbone);
                        let name = crate::backbones::name_of(backbone).unwrap_or_default();
                        host.log(
                            MK_DETAIL,
                            &join_c_slices(&[
                                b"Article ",
                                &self.info_name,
                                b" @ ",
                                &conn_name,
                                b" not found; not asking other servers on backbone ",
                                name.as_bytes(),
                            ]),
                        );
                    }
                    AD_NOT_FOUND
                }
                (Seen::Unreachable(text), _) => {
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[
                            b"Article ",
                            &self.info_name,
                            b" @ ",
                            &conn_name,
                            b" failed, could not fetch article: ",
                            text.as_bytes(),
                        ]),
                    );
                    AD_FAILED
                }
                (Seen::Failed(_), Some(FailKind::Decode(st))) => {
                    let what: &[u8] = match st {
                        nntp_rs::decoder::Status::CrcError => b" failed: CRC-Error",
                        nntp_rs::decoder::Status::ArticleIncomplete => b" failed: article incomplete",
                        nntp_rs::decoder::Status::InvalidSize => b" failed: size mismatch",
                        nntp_rs::decoder::Status::NoBinaryData => b" failed: no binary data found",
                        _ => b" failed",
                    };
                    host.log(MK_DETAIL, &join_c_slices(&[b"Decoding ", &self.info_name, what]));
                    if st == nntp_rs::decoder::Status::CrcError {
                        AD_CRC_ERROR
                    } else {
                        AD_FAILED
                    }
                }
                (Seen::Failed(text), Some(FailKind::Login | FailKind::Auth)) => {
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[
                            b"Article ",
                            &self.info_name,
                            b" @ ",
                            &conn_name,
                            b" failed: could not establish connection (",
                            text.as_bytes(),
                            b")",
                        ]),
                    );
                    AD_CONNECT_ERROR
                }
                (Seen::Failed(text), _) => {
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[
                            b"Article ",
                            &self.info_name,
                            b" @ ",
                            &conn_name,
                            b" failed: ",
                            text.as_bytes(),
                        ]),
                    );
                    AD_FAILED
                }
            };
            // a connection that never got an answer (connect, TLS) says nothing
            // about the article, as before
            let no_answer = matches!(a.fail, Some(FailKind::Other | FailKind::Timeout))
                && a.reply.is_none()
                && a.wire_bytes == 0;
            let (rec, succ, fail) =
                article_dl_classify_attempt_stat(status, AD_FINISHED, AD_FAILED, AD_NOT_FOUND, AD_CRC_ERROR);
            // a copy that arrived is counted once the writer has taken or rejected it
            if rec && status != AD_FINISHED && !no_answer {
                self.count_server_article(w, srv.id, succ, fail);
            }
            w.downloaded_size = w
                .downloaded_size
                .wrapping_add(i32::try_from(a.wire_bytes).unwrap_or(i32::MAX));
        }
    }

    /// One article success or failure on server `server_id`, for the file's
    /// per-server counts and the statistics.
    #[cfg(unix)]
    fn count_server_article(&self, w: &mut DlWorker, server_id: i32, succ: i32, fail: i32) {
        if let Some(ent) = w.server_stats.iter_mut().find(|(sid, _, _)| *sid == server_id) {
            let (ns, nf) = crate::downloadinfo::server_stat_apply_op(ent.1, ent.2, succ, fail, 0, 0, 1, 2);
            ent.1 = ns;
            ent.2 = nf;
        } else {
            let (ns, nf) = crate::downloadinfo::server_stat_apply_op(0, 0, succ, fail, 0, 0, 1, 2);
            w.server_stats.push((server_id, ns, nf));
        }
        if let Some(sm) = crate::statmeter::active_statmeter() {
            let off = crate::ffi::active_workstate().map_or(0, |ws| ws.local_time_offset());
            sm.add_server_stats(0, fail as u32, succ as u32, server_id, off);
        }
    }

    /// [`Self::write_decoded`] for a whole article the pool decoded.
    #[cfg(unix)]
    fn write_decoded_from(
        &self,
        w: &mut DlWorker,
        host: &DlHost,
        d: &nntp_rs::Decoded,
        raw_article: bool,
        skip_disk_write: bool,
    ) -> bool {
        let mut art_filename_opt: Option<Vec<u8>> = None;
        let mut article_file_size = 0i64;
        let mut article_offset = 0i64;
        let mut article_size = 0i32;
        let format = if raw_article { Format::Unknown } else { d.format };
        if !raw_article {
            art_filename_opt = Some(d.filename.as_bytes().to_vec());
            if d.format == Format::Yenc {
                // nntp-rs gives a single-part article as begin 0 and end the
                // file size; RustyGet's Decoder said 1..=size
                let begin_pos = i64::try_from(d.part_begin.max(1)).unwrap_or(0);
                let end_pos = i64::try_from(d.part_end).unwrap_or(0);
                // the whole file's size (=ybegin size=), not the part's
                article_file_size = i64::try_from(d.file_size).unwrap_or(0);
                let mut vints = [0i32; 1];
                let mut vi64s = [0i64; 3];
                host.op45(
                    4,
                    0,
                    host.article_ptr,
                    std::ptr::null(),
                    std::ptr::null(),
                    vints.as_mut_ptr(),
                    vi64s.as_mut_ptr(),
                    std::ptr::null_mut(),
                );
                let dupe_round = vints[0];
                let expected_file_size = vi64s[0];
                let dupe_expected_offset = Some(vi64s[1]).filter(|&v| v >= 0);
                let dupe_expected_end = Some(vi64s[2]).filter(|&v| v >= 0);
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
                    Err(YencRangeError::MissingBounds) => return false,
                    Err(YencRangeError::Malformed) => {
                        let tail = format!(
                            ": range {begin_pos}-{end_pos} out of range (file size {article_file_size})"
                        );
                        host.log(
                            MK_WARNING,
                            &join_c_slices(&[b"Malformed article ", &self.info_name, tail.as_bytes()]),
                        );
                        return false;
                    }
                    Err(
                        mismatch @ (YencRangeError::FileSizeMismatch
                        | YencRangeError::OffsetMismatch
                        | YencRangeError::EndMismatch),
                    ) => {
                        w.decoded_file_size = article_file_size;
                        let tail = match mismatch {
                            YencRangeError::FileSizeMismatch => format!(
                                " from duplicate: file size mismatch ({article_file_size} vs {expected_file_size})"
                            ),
                            YencRangeError::OffsetMismatch => format!(
                                " from duplicate: decoded offset mismatch ({} vs {})",
                                begin_pos - 1,
                                dupe_expected_offset.unwrap_or(-1)
                            ),
                            _ => format!(
                                " from duplicate: decoded end mismatch ({end_pos} vs {})",
                                dupe_expected_end.unwrap_or(-1)
                            ),
                        };
                        host.log(
                            MK_DETAIL,
                            &join_c_slices(&[b"Discarding article ", &self.info_name, tail.as_bytes()]),
                        );
                        return false;
                    }
                }
            }
        }
        if !w.writer.start(
            host,
            &self.info_name,
            format,
            art_filename_opt.as_deref(),
            article_file_size,
            article_offset,
            article_size,
        ) {
            w.local_write_error = true;
            return false;
        }
        w.writing_started = true;
        let ok = w.writer.write(&d.data, raw_article, skip_disk_write);
        if !ok {
            w.local_write_error = true;
        }
        if let Some(ref mut ca) = w.analyzer {
            let _ = ca.append(&d.data);
        }
        ok
    }

    /// [`Self::decode_check`] for a whole article the pool decoded (it
    /// already refused bodies that did not decode).
    #[cfg(unix)]
    fn decode_check_from(
        &self,
        w: &mut DlWorker,
        host: &DlHost,
        d: &nntp_rs::Decoded,
        raw_article: bool,
        crc_check: bool,
    ) -> i32 {
        if raw_article {
            return AD_FINISHED;
        }
        let is_yenc = d.format == Format::Yenc;
        match article_dl_classify_decode_status(false, 1, is_yenc, w.writer.range_exceeded()) {
            1 => {
                w.article_filename_c = Some(nul_vec(d.filename.as_bytes()));
                if is_yenc {
                    let crc = if crc_check { d.calculated_crc } else { d.expected_crc };
                    host.op45(
                        3,
                        crc as c_int,
                        host.article_ptr,
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
                    &join_c_slices(&[b"Decoding ", &self.info_name, b" failed: more data than its range"]),
                );
                AD_FAILED
            }
            _ => {
                host.log(MK_DETAIL, &join_c_slices(&[b"Decoding ", &self.info_name, b" failed"]));
                AD_FAILED
            }
        }
    }

    pub(crate) fn run_with_coord(
        self: &std::sync::Arc<Self>,
        qh: &QueueSubsysHost,
        coord: &crate::queuecoord::NzbgetRsQueueCoordinator,
    ) {
        let host = DlHost {
            qh,
            file_ptr: self.file_ptr,
            article_ptr: self.article_ptr,
            stopped: &self.stopped,
        };
        host.log(MK_DEBUG, b"Entering ArticleDownloader-loop");

        self.set_status(AD_RUNNING, false);

        if self.file_ptr.is_some() && self.article_ptr != 0 {
            host.with_output_guard(false, || {
                let mut w = self.worker.lock().unwrap_or_else(|e| e.into_inner());
                w.writer.build_filenames_locked(&host);
            });
        }

        let final_status = {
            let mut w = self.worker.lock().unwrap_or_else(|e| e.into_inner());

            // Copy the queue state under the queue lock (see Writer::start).
            let (fsnap, nzb, art) = qh.with_locked_queue(|_| {
                let fsnap = qh.file_snap(self.file_ptr);
                let nzb = qh.nzb_snap(fsnap.nzb_ptr, false);
                (fsnap, nzb, qh.article_snap(self.article_ptr))
            });
            let force = nzb.force_priority;
            let extra_priority = fsnap.extra_priority;
            let has_desired_server = nzb.has_desired_server;
            let dupe_fallback_round = art.dupe_fallback_round;
            // A weak primary cut at its frontier (`frontiercut`): its data articles not fetched
            // yet are lent by its alt, so none goes to a server.
            let cut_at_frontier = !fsnap.par_file && crate::frontiercut::cuts_file(nzb.id, fsnap.id);
            let file_time = fsnap.time;

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
            // backbones that answered 430 for this article (see `BackboneMiss`)
            let mut missing: Vec<BackboneMiss> = Vec::new();
            // a server or backbone change meanwhile voids those 430s
            let missing_epoch = crate::articlefetcher::missing_epoch();
            let mut missing_epoch_seen = missing_epoch;
            // set when the epoch moved: the next server-list advance starts over at
            // the first level (a retry of the same server keeps its level)
            let mut restart_levels = false;
            // every server asked answered 430 (none failed another way)
            let mut asked_430 = 0u32;
            let mut only_430 = true;
            let mut want_server: usize = 0;
            let mut level = 0i32;
            let mut status = AD_FAILED;

            let runtime_flags = || {
                let stopped = host.is_stopped();
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

            let try_acquire_conn = |lvl: i32,
                                    want_ptr: usize,
                                    ignore_ptrs: &[usize],
                                    missing: &[BackboneMiss]|
             -> usize {
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
                pool.get_connection_missing(lvl, want_srv, &ign_refs, missing) as usize
            };

            // with the shared nntp-rs pool (always, in the daemon) the article
            // goes through it; one of another server generation means the
            // servers changed: try again under the new one. The per-connection
            // loop below serves only test setups without one.
            #[cfg(unix)]
            let pooled = crate::nntprs_pool::shared();
            #[cfg(not(unix))]
            let pooled: Option<std::sync::Arc<()>> = None;
            if cut_at_frontier && pooled.is_some() {
                // (the per-connection loop below checks it at its top)
                host.log(
                    MK_DETAIL,
                    &join_c_slices(&[b"Not fetching ", &self.info_name, b": its download is cut at its frontier, the alt lends the rest"]),
                );
                status = AD_NOT_FOUND;
            } else if pooled.as_ref().is_some_and(|s| s.generation() != server_config_gen) {
                status = AD_RETRY;
            } else if let Some(shared) = pooled {
                status = self.download_pooled(
                    &mut w,
                    &host,
                    &shared,
                    PooledJob {
                        force,
                        extra_priority,
                        has_desired_server,
                        desired_server_id: qh.with_locked_queue(|_| {
                            let f = qh.file_snap(self.file_ptr);
                            qh.nzb_snap(f.nzb_ptr, false).desired_server_id
                        }),
                        dupe_fallback_round,
                        file_time,
                        retries,
                        interval_sec,
                        server_config_gen,
                    },
                );
            } else {
            loop {
                if cut_at_frontier {
                    host.log(
                        MK_DETAIL,
                        &join_c_slices(&[b"Not fetching ", &self.info_name, b": its download is cut at its frontier, the alt lends the rest"]),
                    );
                    status = AD_NOT_FOUND;
                    break;
                }
                if crate::articlefetcher::missing_epoch() != missing_epoch_seen {
                    missing_epoch_seen = crate::articlefetcher::missing_epoch();
                    missing.clear();
                    restart_levels = true;
                }
                let (stopped, _, _, _, _, _, _) = runtime_flags();
                if stopped {
                    break;
                }
                status = AD_FAILED;
                self.set_status(AD_WAITING, false);

                loop {
                    let (st_now, _, _, cur_gen, _, _, now_t) = runtime_flags();
                    if self.conn_ptr() != 0 || st_now || cur_gen != server_config_gen {
                        break;
                    }
                    if transient_retries.is_empty() {
                        let got = try_acquire_conn(level, want_server, &failed_servers, &missing);
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
                            let got = try_acquire_conn(level, want_server, &ign_all, &missing);
                            if got != 0 {
                                self.set_conn_ptr(got);
                            }
                        }
                    }
                    if self.conn_ptr() == 0 {
                        sleep_ms(5);
                    }
                }

                self.set_status(AD_RUNNING, true);

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
                let backbone = srv.group();
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
                    self.free_connection(&host, &mut w.downloaded_size, true);
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
                if !connected {
                    only_430 = false;
                }
                if connected && !st_after_conn {
                    status = self.download_article(&mut w, &host, join_group);
                    if status == AD_NOT_FOUND && w.last_430 {
                        asked_430 += 1;
                    } else {
                        only_430 = false;
                    }
                    if status == AD_TRANSIENT_ERROR {
                        if transient_retries.register(last_server, now_sec()) {
                            status = AD_CONNECT_ERROR;
                            transient_retry_scheduled = true;
                        } else {
                            status = AD_FAILED;
                            remained_retries = 0;
                        }
                    }

                    if status == AD_NOT_FOUND && w.last_430 && backbone > 0 {
                        // the same article store answers for every server on this backbone
                        let miss = BackboneMiss {
                            group: backbone,
                            retention,
                        };
                        if !missing.contains(&miss) {
                            missing.push(miss);
                            let name = crate::backbones::name_of(backbone).unwrap_or_default();
                            host.log(
                                MK_DETAIL,
                                &join_c_slices(&[
                                    b"Article ",
                                    &self.info_name,
                                    b" @ ",
                                    &conn_name,
                                    b" not found; not asking other servers on backbone ",
                                    name.as_bytes(),
                                ]),
                            );
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
                    self.free_connection(&host, &mut w.downloaded_size, false);
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
                        &host,
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
                    // a server or backbone change since this article began voids the
                    // backbone 430s: decide from the first level again, minus the
                    // servers really tried (the level check below passes over the
                    // levels that are all tried)
                    if crate::articlefetcher::missing_epoch() != missing_epoch_seen {
                        missing_epoch_seen = crate::articlefetcher::missing_epoch();
                        missing.clear();
                        restart_levels = true;
                    }
                    if restart_levels {
                        restart_levels = false;
                        level = 0;
                    }

                    let (cand_ptrs, cands): (Vec<usize>, Vec<LevelCandidate>) =
                        crate::net::active_server_pool()
                            .map(|pool| {
                                pool.servers_snapshot()
                                    .iter()
                                    .map(|s| {
                                        let available = s.active.load(Ordering::SeqCst)
                                            && s.max_connections != 0
                                            && !(s.optional && pool.is_server_blocked(s));
                                        (
                                            std::sync::Arc::as_ptr(s) as usize,
                                            LevelCandidate {
                                                id: s.id,
                                                level: s.norm_level.load(Ordering::SeqCst),
                                                group: s.group(),
                                                retention: s.retention,
                                                available,
                                            },
                                        )
                                    })
                                    .unzip()
                            })
                            .unwrap_or_default();

                    let f_ids: Vec<i32> = failed_servers
                        .iter()
                        .filter_map(|fp| cand_ptrs.iter().position(|p| p == fp))
                        .map(|pos| cands[pos].id)
                        .collect();

                    let all_failed =
                        article_dl_all_servers_on_level_failed(level, &cands, &f_ids, &missing);
                    let (adv_lvl, mut next_lvl, mut break_all) =
                        article_dl_eval_level_exhaustion(all_failed, level, max_norm_lvl);
                    // pass over the next levels too when nothing on them can have the
                    // article (all on backbones that answered 430), so the article
                    // never waits on a level it can't use
                    while adv_lvl
                        && !break_all
                        && article_dl_all_servers_on_level_failed(next_lvl, &cands, &f_ids, &missing)
                    {
                        if next_lvl >= max_norm_lvl {
                            break_all = true;
                        } else {
                            next_lvl += 1;
                        }
                    }
                    let adv_lvl = adv_lvl && !break_all;
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
                        // missing everywhere only when every server could be asked
                        // (none inactive, without connections or blocked) and each
                        // answered 430, itself or through its backbone
                        if only_430
                            && asked_430 > 0
                            && cands.iter().all(|c| c.available)
                            && cands.iter().all(|c| {
                                article_dl_all_servers_on_level_failed(
                                    c.level, &cands, &f_ids, &missing,
                                )
                            })
                        {
                            crate::articlefetcher::note_missing_everywhere_since(&w.message_id, missing_epoch);
                        }
                        status = AD_FAILED;
                        break;
                    }
                }
            }

            }

            self.free_connection(&host, &mut w.downloaded_size, status == AD_FINISHED);

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
            if status == AD_FAILED && (transient_retries.is_exhausted() || w.pooled_transient_exhausted) {
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

        self.set_status(final_status, false);
        coord.on_article_finished(self, qh);
        host.log(MK_DEBUG, b"Exiting ArticleDownloader-loop");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn only_whole_flushed_articles_are_reported_to_live_extraction() {
        let a = vec![1u8; 100];
        let short = vec![2u8; 40];
        let item = |offset: i64, size: i32, content: &'static [u8]| ArticleWriterFlushItem {
            segment_offset: offset,
            segment_size: size,
            segment_content: content,
            result_filename: None,
        };
        let a: &'static [u8] = Box::leak(a.into_boxed_slice());
        let short: &'static [u8] = Box::leak(short.into_boxed_slice());
        let items = vec![item(0, 100, a), item(100, 100, short), item(200, 100, a), item(300, 100, a)];
        // All four flushed, the second without its bytes: it is not reported.
        assert_eq!(
            super::flushed_ranges(&super::flush_shapes(&items), 4, 340),
            vec![(0, 100), (200, 100)],
            "a flush short of the sizes drops the last item counted"
        );
        assert_eq!(super::flushed_ranges(&super::flush_shapes(&items), 4, 400), vec![(0, 100), (200, 100), (300, 100)]);
        // A write that failed at the third item: the first two counted, nothing after.
        assert_eq!(super::flushed_ranges(&super::flush_shapes(&items), 3, 250), vec![(0, 100)]);
        assert_eq!(super::flushed_ranges(&super::flush_shapes(&items), 0, 0), Vec::<(u64, u64)>::new());
    }

    #[test]
    fn a_par2_file_flush_is_never_handed_to_live_par() {
        assert!(super::live_flush_reports(false, true));
        assert!(!super::live_flush_reports(true, true), "a par2 file would stop live PAR");
        assert!(!super::live_flush_reports(false, false));
    }

    /// The cache flush frees each article's cache segment as it is written. Live PAR copies
    /// the written bytes after the flush, so the segments it reads must still be allocated
    /// (server 6790, cf007ba0: the article-cache thread segfaulted in that copy).
    #[test]
    fn live_par_copies_flushed_articles_before_their_cache_segments_are_freed() {
        use std::cell::RefCell;
        const N: usize = 4;
        const SIZE: usize = 4096;
        // Cache segments as the article cache holds them: separate heap blocks, freed by
        // the discard callback, referenced by the flush items through raw pointers.
        let raw: Vec<*mut u8> = (0..N)
            .map(|i| Box::into_raw(vec![i as u8 + 1; SIZE].into_boxed_slice()).cast::<u8>())
            .collect();
        let freed = RefCell::new([false; N]);
        let items: Vec<ArticleWriterFlushItem<'_>> = raw
            .iter()
            .enumerate()
            .map(|(i, &p)| ArticleWriterFlushItem {
                segment_offset: (i * SIZE) as i64,
                segment_size: SIZE as i32,
                segment_content: unsafe { std::slice::from_raw_parts(p, SIZE) },
                result_filename: None,
            })
            .collect();
        let handed = RefCell::new(Vec::new());
        let result = super::flush_cache_then_hand_off(
            &items,
            true,
            |discard, _not_written| {
                // As `article_writer_flush_cache_guarded`: write, then discard, per item.
                for i in 0..N {
                    discard(i);
                }
                (N as i32, (N * SIZE) as i64)
            },
            |i| {
                assert!(!freed.borrow()[i], "segment {i} freed twice");
                freed.borrow_mut()[i] = true;
                drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(raw[i], SIZE)) });
            },
            |written, complete| {
                assert!(complete);
                for &(offset, bytes) in written {
                    let i = offset as usize / SIZE;
                    assert!(!freed.borrow()[i], "live PAR read article {i} after its segment was freed");
                    assert_eq!(bytes, &vec![i as u8 + 1; SIZE][..]);
                    handed.borrow_mut().push(i);
                }
            },
        );
        assert_eq!(result, (N as i32, (N * SIZE) as i64));
        assert_eq!(*handed.borrow(), (0..N).collect::<Vec<_>>());
        assert_eq!(*freed.borrow(), [true; N], "every written segment is freed");
    }

    /// Queue item 11: an article dropped because its target was certified is counted as
    /// flushed but never written. Live PAR must not receive its stale cache bytes as written
    /// (with trusted slices it would vouch for bytes that are not on disk).
    #[test]
    fn live_par_never_receives_a_certified_drop_as_written() {
        let bufs: Vec<Vec<u8>> = (0..3).map(|i| vec![i as u8 + 1; 100]).collect();
        let items: Vec<ArticleWriterFlushItem<'_>> = bufs
            .iter()
            .enumerate()
            .map(|(i, b)| ArticleWriterFlushItem {
                segment_offset: (i * 100) as i64,
                segment_size: 100,
                segment_content: b,
                result_filename: None,
            })
            .collect();
        let mut handed = Vec::new();
        let mut whole = None;
        let result = super::flush_cache_then_hand_off(
            &items,
            true,
            |discard, not_written| {
                // Item 1's target is certified: counted and discarded, not written.
                for i in 0..3 {
                    if i == 1 {
                        not_written(i);
                    }
                    discard(i);
                }
                (3, 300)
            },
            |_| {},
            |written, complete| {
                handed.extend(written.iter().map(|w| w.0));
                whole = Some(complete);
            },
        );
        assert_eq!(result, (3, 300));
        assert_eq!(handed, [0, 200], "the certified drop is not handed over");
        assert_eq!(whole, Some(false), "a recovery tee must not count it either");
        assert_eq!(
            super::live_written_items(&items, 3, 300, &[1]).len(),
            2,
            "filtered by index"
        );
    }

    #[test]
    fn live_par_receives_only_articles_written_before_a_failed_write() {
        let bufs: Vec<Vec<u8>> = (0..3).map(|i| vec![i as u8; 100]).collect();
        let items: Vec<ArticleWriterFlushItem<'_>> = bufs
            .iter()
            .enumerate()
            .map(|(i, b)| ArticleWriterFlushItem {
                segment_offset: (i * 100) as i64,
                segment_size: 100,
                segment_content: b,
                result_filename: None,
            })
            .collect();
        // Three counted, the third failed (its size not counted).
        let written = super::live_written_items(&items, 3, 200, &[]);
        assert_eq!(written.iter().map(|w| w.0).collect::<Vec<_>>(), [0, 100]);
        assert!(super::live_written_items(&items, 0, 0, &[]).is_empty());
        assert_eq!(super::live_written_items(&items, 3, 300, &[]).len(), 3);
    }

    #[test]
    fn exhausted_live_par_budget_does_not_block_an_ordinary_article_write() {
        let dir = std::env::temp_dir().join(format!("rustyget-no-live-budget-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("part40.rar");
        std::fs::write(&path, b"original").unwrap();
        let context = crate::parlive::source_write_context(-1_961_502, 40, &dir, b"part40.rar").unwrap();
        let limit = crate::parchecker::effective_live_pool_bytes();
        assert!(limit > 0);
        // Fill a pool of this test's own (the shared one is process-global, and holding all of
        // it starved every other live test running in parallel).
        let pool = std::sync::Arc::new(crate::parlive::BudgetPool::with_size(limit));
        crate::parchecker::TEST_BUDGET_POOL.with(|p| *p.borrow_mut() = Some(pool.clone()));
        let mut reservation = Vec::new();
        let mut chunk = limit;
        while chunk != 0 {
            while let Some(lease) = pool.acquire(chunk, limit) {
                reservation.push(lease);
            }
            chunk /= 2;
        }
        let mut file = BufferedDiskFile::open_guarded(path.as_os_str().as_bytes(), true, 64, 0, context).unwrap();
        assert_eq!(file.write_bytes(b"ordinary"), 8);
        assert!(file.flush_and_close());
        assert_eq!(std::fs::read(path).unwrap(), b"ordinary");
        drop(reservation);
        crate::parchecker::TEST_BUDGET_POOL.with(|p| *p.borrow_mut() = None);
        crate::parlive::forget_download_writes(-1_961_502);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn jobs_sharing_a_dest_dir_do_not_refuse_each_others_writes() {
        let root = std::env::temp_dir().join(format!("rustyget-shared-dest-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let (a, b) = (-1_961_504, -1_961_505);
        let first = crate::parlive::source_write_context(a, 40, &root, b"one.rar").unwrap();
        let second = crate::parlive::source_write_context(b, 41, &root, b"two.rar").unwrap();
        let (_file, _context, held) = first.open_attested(&root.join("one.rar"), true).unwrap();
        let (_other, _context, _permit) = second.open_attested(&root.join("two.rar"), true).unwrap();
        drop(held);
        crate::parlive::forget_download_writes(a);
        crate::parlive::forget_download_writes(b);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn direct_write_off_assembly_temp_is_this_files_output() {
        let root = std::env::temp_dir().join(format!("rustyget-assembly-tmp-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let job = -1_961_506;
        let context = crate::parlive::source_write_context(job, 40, &root, b"part40.rar").unwrap();
        let path = root.join("part40.rar.tmp");
        let mut file = BufferedDiskFile::open_guarded(path.as_os_str().as_bytes(), false, 64, 0, context).unwrap();
        assert_eq!(file.write_bytes(b"assembled"), 9);
        assert!(file.flush_and_close());
        assert_eq!(std::fs::read(path).unwrap(), b"assembled");
        crate::parlive::forget_download_writes(job);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_article_temporaries_of_one_job_do_not_conflict() {
        let root = std::env::temp_dir().join(format!("rustyget-article-concurrent-src-{}", std::process::id()));
        let scratch = std::env::temp_dir().join(format!("rustyget-article-concurrent-tmp-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&scratch).unwrap();
        let job = -1_961_503;
        let source = crate::parlive::source_write_context(job, 40, &root, b"part40.rar").unwrap();
        let first = prepare_article_write_context(source.clone(), false, &scratch, 1).unwrap();
        let second = prepare_article_write_context(source, false, &scratch, 2).unwrap();
        // Another connection still holds its write permit for article 1.
        let (_file, _context, held) = first.open_attested(&scratch.join("40.001.tmp"), true).unwrap();
        let (_other, _context, _permit) = second.open_attested(&scratch.join("40.002.tmp"), true).unwrap();
        drop(held);
        crate::parlive::forget_download_writes(job);
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn live_buffered_article_temporary_flush_uses_owned_scratch_root() {
        let root = std::env::temp_dir().join(format!("rustyget-article-source-{}", std::process::id()));
        let scratch = std::env::temp_dir().join(format!("rustyget-article-scratch-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&scratch).unwrap();
        let context = crate::parlive::source_write_context(-1_961_501, 40, &root, b"part40.rar").unwrap();
        let context = prepare_article_write_context(context, false, &scratch, 7).unwrap();
        let path = scratch.join("40.007.tmp");
        let mut file = BufferedDiskFile::open_guarded(path.as_os_str().as_bytes(), false, 64, 0, context).unwrap();
        assert_eq!(file.write_bytes(b"one article"), 11);
        assert!(file.flush_and_close());
        assert_eq!(std::fs::read(path).unwrap(), b"one article");
        crate::parlive::forget_download_writes(-1_961_501);
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(scratch).unwrap();
    }
    #[test]
    fn live_buffered_actual_path_mismatch_never_truncates_other_file() {
        let dir = std::env::temp_dir().join(format!("rustyget-buffer-path-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let other = dir.join("other.rar");
        std::fs::write(&other, b"protected").unwrap();
        let context =
            crate::parlive::source_write_context(-1_961_500, 40, &dir, b"part40.rar").unwrap();
        let result =
            BufferedDiskFile::open_guarded(other.as_os_str().as_bytes(), false, 64, 0, context);
        assert_eq!(std::fs::read(&other).unwrap(), b"protected");
        assert!(result.is_err());
        crate::parlive::forget_download_writes(-1_961_500);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn live_buffered_file_rejects_a_late_flush_after_verified_repair() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rustyget-live-buffer-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("part40.rar");
        std::fs::write(&path, b"original").unwrap();
        let context =
            crate::parlive::source_write_context(-1900220, 40, &dir, b"part40.rar").unwrap();
        let mut file =
            BufferedDiskFile::open_guarded(path.as_os_str().as_bytes(), true, 64, 0, context)
                .unwrap();
        assert_eq!(file.write_bytes(b"late-old"), 8);
        let gate = crate::parlive::download_writes(-1900220, &dir).unwrap();
        gate.advance_generation(crate::parlive::SessionGeneration(1))
            .unwrap();
        let lease = gate
            .begin_repair(
                crate::parlive::SessionGeneration(1),
                vec![crate::parlive::WriterTarget {
                    file_id: 40,
                    path: path.clone(),
                }],
                crate::parlive::reserve_control(-1900220, 4096, 500 << 20).unwrap(),
            )
            .unwrap();
        std::fs::write(&path, b"repaired").unwrap();
        lease.publish(&[(40, 0)], || ()).unwrap();
        assert!(!file.flush_and_close());
        assert_eq!(std::fs::read(&path).unwrap(), b"repaired");
        crate::parlive::forget_download_writes(-1900220);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn live_buffered_file_io_failure_invalidates_its_cohort() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "rustyget-live-buffer-error-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("part40.rar");
        std::fs::write(&path, b"original").unwrap();
        let context =
            crate::parlive::source_write_context(-1900221, 40, &dir, b"part40.rar").unwrap();
        let mut file =
            BufferedDiskFile::open_guarded(path.as_os_str().as_bytes(), true, 64, 0, context)
                .unwrap();
        assert_eq!(file.write_bytes(b"new-data"), 8);
        file.file = Some(std::fs::File::open(&path).unwrap()); // a real read-only descriptor
        let gate = crate::parlive::download_writes(-1900221, &dir).unwrap();
        gate.advance_generation(crate::parlive::SessionGeneration(2))
            .unwrap();
        assert!(!file.flush_and_close());
        assert!(!gate.is_current_generation(crate::parlive::SessionGeneration(2)));
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        crate::parlive::forget_download_writes(-1900221);
        std::fs::remove_dir_all(dir).unwrap();
    }

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
                CACHE_FILE_INFO.store(fake_file, Ordering::SeqCst);
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
        let host = MockDlHost {
            cb: mock_host,
            ctx: std::ptr::null_mut(),
        };
        assert!(article_cache_check_flush(&host, 4096, false));
        assert!(OBSERVED_UNDER_LOCK.load(Ordering::SeqCst));
        assert!(!cache_file_busy(0xdead_beefusize));
    }

    #[test]
    fn an_output_in_the_old_folder_follows_the_download() {
        use crate::queuesubsys_host::FileSnap;
        let old = b"/nvme/inter/Movie.#7";
        let out = b"/nvme/inter/Movie.#7/41.out.tmp";
        let fresh = FileSnap::default();
        assert_eq!(output_follow(out, old, &fresh), Some(OutputFollow::Forget));
        assert_eq!(output_follow(out, b"/nvme/inter/Movie.#7/", &fresh), Some(OutputFollow::Forget));
        let written = FileSnap { success_articles: 3, ..Default::default() };
        assert_eq!(output_follow(out, old, &written), Some(OutputFollow::Move));
        // another folder, a sibling with the same prefix, or no output at all
        assert_eq!(output_follow(b"/hdd/inter/Movie.#7/41.out.tmp", old, &fresh), None);
        assert_eq!(output_follow(b"/nvme/inter/Movie.#77/41.out.tmp", old, &fresh), None);
        assert_eq!(output_follow(b"", old, &fresh), None);
        assert_eq!(output_follow(out, b"", &fresh), None);
        // a file being written right now is left alone
        for f in [
            FileSnap { cached_articles: 1, ..Default::default() },
            FileSnap { active_downloads: 1, success_articles: 2, ..Default::default() },
        ] {
            assert_eq!(output_follow(out, old, &f), None);
        }
    }
}
