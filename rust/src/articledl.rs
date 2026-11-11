//! Full Rust runtime backing `MemoryMonitor`, `ArticleCache`, `ServerPool`,
//! `ArticleWriter`, `RenameContentAnalyzer`, and `ArticleDownloader`.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::{Seek, SeekFrom, Write};
use std::os::fd::{FromRawFd, IntoRawFd, RawFd};
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::Mutex;

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
use crate::dupefallback::{
    article_downloader_check_response, article_downloader_validate_yenc, server_pool_is_blocked,
    server_pool_level_should_close, server_pool_normalize_levels, NzbgetRsServerNormInput,
};
use crate::nntp::{
    article_dl_all_servers_on_level_failed, article_dl_classify_attempt_stat,
    article_dl_classify_decode_status, article_dl_classify_write_failure,
    article_dl_eval_level_exhaustion, article_dl_handle_content_rejected,
    article_dl_is_join_group_ok, article_dl_is_retention_failure,
    article_dl_normalize_connect_and_decrement_retries, article_dl_resolve_final_status,
    article_dl_should_advance_server_list, server_pool_eval_block_server,
    server_pool_select_connection, ServerPoolConnView, ServerPoolServerView, TransientRetries,
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
const CONNECTION_HOLD_SECONDS: i32 = 5;

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

    fn log(&self, kind: i32, msg: &[u8]) {
        let c = nul_vec(msg);
        self.raw(
            1,
            kind,
            0,
            0,
            0,
            c.as_ptr().cast(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    fn log_nzb(&self, nzb_ptr: usize, kind: i32, msg: &[u8]) {
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

    fn now(&self) -> i64 {
        let mut t = [0i64; 1];
        self.raw(
            1,
            10,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            t.as_mut_ptr(),
            std::ptr::null_mut(),
        );
        t[0]
    }

    fn sleep_ms(&self, ms: i32) {
        self.raw(
            1,
            11,
            ms,
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

// ============================================================================
// MemoryMonitor global runtime state
// ============================================================================

static MEM_TIER: AtomicI32 = AtomicI32::new(PRESSURE_NORMAL);
static MEM_EFFECTIVE_CACHE_MB: AtomicU32 = AtomicU32::new(0);
static MEM_HYST: Mutex<(i32, i32)> = Mutex::new((PRESSURE_NORMAL, 0));

pub fn memory_monitor_tier() -> i32 {
    MEM_TIER.load(Ordering::Acquire)
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

fn memory_monitor_update_state(host: &DlHost) -> i32 {
    let mut ints = [0i32; 2];
    let mut strs = [std::ptr::null::<c_char>(); 1];
    let _has_opts = host.raw(
        2,
        0,
        0,
        0,
        0,
        std::ptr::null(),
        std::ptr::null(),
        ints.as_mut_ptr(),
        std::ptr::null_mut(),
        strs.as_mut_ptr(),
    );
    let cfg_cache = ints[0].max(0) as u32;
    let interval = ints[1].max(100);

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
            host.log(
                MK_WARNING,
                b"System memory pressure CRITICAL (>=90%): shedding article cache and throttling downloads",
            );
            host.raw(
                2,
                2,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            crate::memory_pressure::trim_heap();
        } else if next_tier == PRESSURE_CONSTRAINED {
            host.log(
                MK_DETAIL,
                b"System memory pressure CONSTRAINED: shrinking article cache ceiling",
            );
            host.raw(
                2,
                2,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        } else if next_tier == PRESSURE_NORMAL && current_tier != PRESSURE_NORMAL {
            host.log(
                MK_DETAIL,
                b"System memory pressure returned to NORMAL: restoring cache and download capacity",
            );
            crate::memory_pressure::trim_heap();
        }
    }

    interval
}

fn memory_monitor_init_options(host: &DlHost, reset_state: bool) {
    if reset_state {
        MEM_TIER.store(PRESSURE_NORMAL, Ordering::Release);
        MEM_EFFECTIVE_CACHE_MB.store(0, Ordering::Release);
        if let Ok(mut hyst) = MEM_HYST.lock() {
            *hyst = (PRESSURE_NORMAL, 0);
        }
    }
    let mut ints = [0i32; 2];
    let mut strs = [std::ptr::null::<c_char>(); 1];
    let has_opts = host.raw(
        2,
        0,
        0,
        0,
        0,
        std::ptr::null(),
        std::ptr::null(),
        ints.as_mut_ptr(),
        std::ptr::null_mut(),
        strs.as_mut_ptr(),
    ) != 0;
    if has_opts {
        let path_opt = unsafe {
            if strs[0].is_null() {
                None
            } else {
                CStr::from_ptr(strs[0])
                    .to_str()
                    .ok()
                    .filter(|s| !s.is_empty())
            }
        };
        crate::memory_pressure::set_mock_root(path_opt);
    }
    let _ = memory_monitor_update_state(host);
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
        let interval = memory_monitor_update_state(host);
        let mut slept = 0;
        while !is_stopped() && slept < interval {
            host.sleep_ms(50);
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
        host.log(MK_DEBUG, b"Checking cache... nothing to flush");
        return false;
    }

    let info_name = unsafe { cstr_or_empty(strs[0]) };
    host.log(
        MK_DETAIL,
        &join_c_slices(&[b"Flushing cache for ", &info_name]),
    );

    let mut ints = [0i32; 4];
    let mut i64s = [0i64; 2];
    let mut fstrs = [std::ptr::null::<c_char>(); 2];
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
        let skip_after = post_ints[0] != 0;
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

    host.raw(
        2,
        6,
        0,
        0,
        0,
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    true
}

fn article_cache_run(host: &DlHost) {
    let snap = || {
        let mut ints = [0i32; 3];
        let mut i64s = [0i64; 2];
        host.raw(
            2,
            3,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            std::ptr::null_mut(),
        );
        (
            i64s[0].max(0) as usize,
            i64s[1].max(0) as usize,
            ints[0] != 0,
            ints[1] != 0,
            ints[2],
        )
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
                host.sleep_ms(5);
            }
        } else if allocated == 0 {
            host.raw(
                2,
                4,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            reset_counter = 0;
        } else {
            host.sleep_ms(5);
            reset_counter += 5;
        }
    }
}

// ============================================================================
// ServerPool Rust runtime operations
// ============================================================================

fn server_pool_init_connections(host: &DlHost) {
    let mut meta = [0i32; 7];
    let mut now_arr = [0i64; 1];
    host.raw(
        3,
        0,
        0,
        0,
        0,
        std::ptr::null(),
        std::ptr::null(),
        meta.as_mut_ptr(),
        now_arr.as_mut_ptr(),
        std::ptr::null_mut(),
    );
    let srv_count = meta[0].max(0) as usize;
    let conn_count = meta[1].max(0) as usize;

    if srv_count > 0 {
        let mut inputs = Vec::with_capacity(srv_count);
        for i in 0..srv_count {
            let mut ints = [0i32; 7];
            let mut i64s = [0i64; 1];
            let mut strs = [std::ptr::null::<c_char>(); 2];
            host.raw(
                3,
                1,
                i as i32,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                i64s.as_mut_ptr(),
                strs.as_mut_ptr(),
            );
            inputs.push(NzbgetRsServerNormInput {
                level: ints[1],
                active: ints[3],
                max_connections: ints[6],
            });
        }
        let mut sorted_idx = vec![0usize; srv_count];
        let mut norm_levels = vec![0i32; srv_count];
        let max_norm = server_pool_normalize_levels(&inputs, &mut sorted_idx, &mut norm_levels);
        for (i, &nl) in norm_levels.iter().enumerate() {
            host.raw(
                3,
                2,
                0,
                i,
                nl as usize,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
        let mut order_i32: Vec<c_int> = sorted_idx.iter().map(|&idx| idx as c_int).collect();
        host.raw(
            3,
            2,
            1,
            srv_count,
            max_norm as usize,
            std::ptr::null(),
            std::ptr::null(),
            order_i32.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    } else {
        host.raw(
            3,
            2,
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

    let mut conn_srv_ptrs = Vec::with_capacity(conn_count);
    for i in 0..conn_count {
        let mut ints = [0i32; 5];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null::<c_char>(); 2];
        let sptr = host.raw(
            3,
            3,
            i as i32,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        conn_srv_ptrs.push(sptr);
    }

    for i in 0..srv_count {
        let mut ints = [0i32; 7];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null::<c_char>(); 2];
        let sptr = host.raw(
            3,
            1,
            i as i32,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        let norm_level = ints[2];
        let active = ints[3] != 0;
        let max_conns = ints[6];
        let mut existing = 0i32;
        let mut to_add = 0usize;
        if norm_level > -1 && active {
            existing = conn_srv_ptrs.iter().filter(|&&p| p == sptr).count() as i32;
            if max_conns > existing {
                to_add = (max_conns - existing) as usize;
                existing = max_conns;
            }
        }
        let mut args = [norm_level, existing, i32::from(active)];
        host.raw(
            3,
            2,
            2,
            i,
            to_add,
            std::ptr::null(),
            std::ptr::null(),
            args.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    host.raw(
        3,
        2,
        3,
        0,
        0,
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
}

fn server_pool_get_connection(
    host: &DlHost,
    level: i32,
    want_srv_ptr: usize,
    ignore_list_ptr: usize,
) -> usize {
    if level < 0 {
        return 0;
    }
    let mut meta = [0i32; 7];
    let mut now_arr = [0i64; 1];
    host.raw(
        3,
        0,
        0,
        0,
        0,
        std::ptr::null(),
        std::ptr::null(),
        meta.as_mut_ptr(),
        now_arr.as_mut_ptr(),
        std::ptr::null_mut(),
    );
    let srv_count = meta[0].max(0) as usize;
    let conn_count = meta[1].max(0) as usize;
    let levels_len = meta[2].max(0) as usize;
    let retry_interval = meta[4];
    let cur_time = now_arr[0];
    if srv_count == 0 || conn_count == 0 {
        return 0;
    }

    let mut levels = Vec::with_capacity(levels_len);
    for i in 0..levels_len {
        let v = host.raw(
            3,
            5,
            6,
            i,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) as i32;
        levels.push(v);
    }

    let mut srv_ptrs = Vec::with_capacity(srv_count);
    let mut srv_views = Vec::with_capacity(srv_count);
    for i in 0..srv_count {
        let mut ints = [0i32; 7];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null::<c_char>(); 2];
        let sptr = host.raw(
            3,
            1,
            i as i32,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        srv_ptrs.push(sptr);
        srv_views.push(ServerPoolServerView {
            id: ints[0],
            group: ints[5],
            norm_level: ints[2],
            active: ints[3] != 0,
            optional: ints[4] != 0,
            blocked: server_pool_is_blocked(i64s[0], cur_time, retry_interval),
        });
    }

    let mut conn_views = Vec::with_capacity(conn_count);
    for i in 0..conn_count {
        let mut ints = [0i32; 5];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null::<c_char>(); 2];
        let sptr = host.raw(
            3,
            3,
            i as i32,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        let server_index = srv_ptrs
            .iter()
            .position(|&p| p == sptr)
            .unwrap_or(srv_count);
        conn_views.push(ServerPoolConnView {
            server_index,
            in_use: ints[0] != 0,
            connected: ints[1] != 0,
        });
    }

    let want_tuple = if want_srv_ptr != 0 {
        let mut ints = [0i32; 7];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null::<c_char>(); 2];
        host.raw(
            3,
            1,
            -1,
            want_srv_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        Some((ints[0], ints[5]))
    } else {
        None
    };

    let mut ignore_tuples = Vec::new();
    if ignore_list_ptr != 0 && want_srv_ptr == 0 {
        let ign_len = host.raw(
            3,
            4,
            -1,
            ignore_list_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        ignore_tuples.reserve(ign_len);
        for i in 0..ign_len {
            let mut ints = [0i32; 3];
            host.raw(
                3,
                4,
                i as i32,
                ignore_list_ptr,
                0,
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            ignore_tuples.push((ints[0], ints[1], ints[2]));
        }
    }

    let rand_val = unsafe { libc::rand() };
    let mut unblock = Vec::new();
    let found = server_pool_select_connection(
        level,
        &levels,
        &srv_views,
        &conn_views,
        want_tuple,
        &ignore_tuples,
        rand_val,
        &mut unblock,
    );

    let mut zero = [0i64; 1];
    for idx in unblock {
        host.raw(
            3,
            5,
            0,
            idx,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            zero.as_mut_ptr(),
            std::ptr::null_mut(),
        );
    }

    if let Some((chosen_conn, chosen_level)) = found {
        host.raw(
            3,
            5,
            1,
            chosen_conn,
            chosen_level as usize,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } else {
        0
    }
}

fn server_pool_close_unused(host: &DlHost) {
    let cur_time = host.now();
    host.raw(
        3,
        5,
        3,
        0,
        0,
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );

    let mut meta = [0i32; 7];
    let mut now_arr = [0i64; 1];
    host.raw(
        3,
        0,
        0,
        0,
        0,
        std::ptr::null(),
        std::ptr::null(),
        meta.as_mut_ptr(),
        now_arr.as_mut_ptr(),
        std::ptr::null_mut(),
    );
    let conn_count = meta[1].max(0) as usize;
    let max_norm_level = meta[3];

    let mut snaps = Vec::with_capacity(conn_count);
    for i in 0..conn_count {
        let mut ints = [0i32; 5];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null::<c_char>(); 2];
        host.raw(
            3,
            3,
            i as i32,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        snaps.push((ints[4], ints[0] as u8, ints[1] as u8, ints[2], i64s[0]));
    }

    for lvl in 0..=max_norm_level {
        let mut in_use = Vec::new();
        let mut connected = Vec::new();
        let mut free_times = Vec::new();
        for &(nl, iu, conn, _, ft) in &snaps {
            if nl == lvl {
                in_use.push(iu);
                connected.push(conn);
                free_times.push(ft);
            }
        }
        if server_pool_level_should_close(
            &in_use,
            &connected,
            &free_times,
            cur_time,
            CONNECTION_HOLD_SECONDS,
        ) {
            for (idx, &(nl, _, conn, srv_id, _)) in snaps.iter().enumerate() {
                if nl == lvl && conn != 0 {
                    host.log(
                        MK_DEBUG,
                        format!("Closing (and keeping) unused connection to server{srv_id}")
                            .as_bytes(),
                    );
                    host.raw(
                        3,
                        5,
                        4,
                        idx,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                }
            }
        }
    }
}

fn server_pool_block_server(host: &DlHost, srv_ptr: usize) {
    let mut ints = [0i32; 1];
    let mut i64s = [0i64; 2];
    let mut strs = [std::ptr::null::<c_char>(); 2];
    host.raw(
        3,
        5,
        5,
        srv_ptr,
        0,
        std::ptr::null(),
        std::ptr::null(),
        ints.as_mut_ptr(),
        i64s.as_mut_ptr(),
        strs.as_mut_ptr(),
    );
    let retry_interval = ints[0];
    let (_new_block, should_warn) = server_pool_eval_block_server(i64s[0], i64s[1], retry_interval);
    if should_warn {
        let name = unsafe { cstr_or_empty(strs[0]) };
        let shost = unsafe { cstr_or_empty(strs[1]) };
        let tail = format!(") for {retry_interval} sec");
        host.log(
            MK_WARNING,
            &join_c_slices(&[b"Blocking ", &name, b" (", &shost, tail.as_bytes()]),
        );
    }
}

fn server_pool_log_debug(host: &DlHost) {
    let mut meta = [0i32; 7];
    let mut now_arr = [0i64; 1];
    host.raw(
        3,
        0,
        0,
        0,
        0,
        std::ptr::null(),
        std::ptr::null(),
        meta.as_mut_ptr(),
        now_arr.as_mut_ptr(),
        std::ptr::null_mut(),
    );
    let conn_count = meta[1].max(0) as usize;
    let levels_len = meta[2].max(0) as usize;
    let max_norm_level = meta[3];
    let retry_interval = meta[4];
    let raw_servers_len = meta[6].max(0) as usize;
    let cur_time = now_arr[0];

    host.log(MK_INFO, b"   ---------- ServerPool");
    host.log(
        MK_INFO,
        format!("    Max-Level: {max_norm_level}").as_bytes(),
    );
    host.log(
        MK_INFO,
        format!("    Servers: {raw_servers_len}").as_bytes(),
    );

    for i in 0..raw_servers_len {
        let mut ints = [0i32; 7];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null::<c_char>(); 2];
        host.raw(
            3,
            1,
            i as i32,
            0,
            1,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        let id = ints[0];
        let lvl = ints[1];
        let nlvl = ints[2];
        let block_time = i64s[0];
        let mut block_sec = 0i32;
        if block_time != 0 {
            let end = block_time.saturating_add(retry_interval as i64);
            if end > cur_time {
                let diff = (end as u64).wrapping_sub(cur_time as u64);
                block_sec = if diff > (i32::MAX as u64) {
                    i32::MAX
                } else {
                    diff as i32
                };
            }
        }
        let name = unsafe { cstr_or_empty(strs[0]) };
        let shost = unsafe { cstr_or_empty(strs[1]) };
        let p1 = format!("      {id}) ");
        let p2 = format!(": Level={lvl}, NormLevel={nlvl}, BlockSec={block_sec}");
        host.log(
            MK_INFO,
            &join_c_slices(&[p1.as_bytes(), &name, b" (", &shost, b")", p2.as_bytes()]),
        );
    }

    host.log(MK_INFO, format!("    Levels: {levels_len}").as_bytes());
    for idx in 0..levels_len {
        let sz = host.raw(
            3,
            5,
            6,
            idx,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) as i32;
        host.log(
            MK_INFO,
            format!("      {idx}: Free connections={sz}").as_bytes(),
        );
    }

    host.log(MK_INFO, format!("    Connections: {conn_count}").as_bytes());
    for i in 0..conn_count {
        let mut ints = [0i32; 5];
        let mut i64s = [0i64; 1];
        let mut strs = [std::ptr::null::<c_char>(); 2];
        host.raw(
            3,
            3,
            i as i32,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        let in_use = ints[0];
        let id = ints[2];
        let lvl = ints[3];
        let nlvl = ints[4];
        let name = unsafe { cstr_or_empty(strs[0]) };
        let shost = unsafe { cstr_or_empty(strs[1]) };
        let p1 = format!("      {id}) ");
        let p2 = format!(": Level={lvl}, NormLevel={nlvl}, InUse:{in_use}");
        host.log(
            MK_INFO,
            &join_c_slices(&[p1.as_bytes(), &name, b" (", &shost, b")", p2.as_bytes()]),
        );
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

impl Default for RsArticleWriter {
    fn default() -> Self {
        Self {
            temp_filename: Vec::new(),
            output_filename: Vec::new(),
            result_filename: Vec::new(),
            format: Format::Unknown,
            cached_ptr: std::ptr::null_mut(),
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
    fn range_exceeded(&self) -> bool {
        self.article_ptr > self.article_size
    }

    fn has_cached_buffer(&self) -> bool {
        !self.cached_ptr.is_null()
    }

    fn build_filenames_locked(&mut self, host: &DlHost) {
        let mut ints = [0i32; 6];
        let mut strs = [std::ptr::null::<c_char>(); 5];
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
        let direct_write = ints[2] != 0;
        let force_direct_write = ints[3] != 0;
        let output_initialized = ints[4] != 0;
        let tmp_dir = unsafe { cstr_or_empty(strs[0]) };
        let dest_dir = unsafe { cstr_or_empty(strs[1]) };
        let existing_out = unsafe { cstr_or_empty(strs[2]) };
        let file_filename = unsafe { cstr_or_empty(strs[3]) };

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
        self.cached_ptr = std::ptr::null_mut();
        host.raw(
            4,
            18,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );

        let mut ints = [0i32; 11];
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
        let dupe_check = ints[0] != 0;
        let dupe_mode = ints[1];
        let many_dupe_files = ints[2] != 0;
        let direct_write_opt = ints[3] != 0;
        let force_direct_write = ints[4] != 0;
        let skip_disk_write = ints[5] != 0;
        let article_cache_mb = ints[6];
        let raw_article = ints[7] != 0;
        let write_buffer_kb = ints[8];
        let art_info_size = ints[9];
        let is_dupe_fallback = ints[10] > 0;
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
            let mut fl_ints = [0i32; 1];
            let mut p = host.raw(
                4,
                18,
                1,
                self.article_size as usize,
                0,
                std::ptr::null(),
                std::ptr::null(),
                fl_ints.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            while p == 0 && fl_ints[0] != 0 && memory_monitor_tier() != PRESSURE_CRITICAL {
                host.sleep_ms(5);
                p = host.raw(
                    4,
                    18,
                    1,
                    self.article_size as usize,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    fl_ints.as_mut_ptr(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
            self.cached_ptr = p as *mut u8;
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

        let mut ints = [0i32; 11];
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
        let direct_write_opt = ints[3] != 0;
        let force_direct_write = ints[4] != 0;
        let skip_disk_write = ints[5] != 0;
        let raw_article = ints[7] != 0;
        let is_dupe_fallback = ints[10] > 0;
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
            if has_cached {
                self.cached_ptr = std::ptr::null_mut();
                host.raw(
                    4,
                    18,
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
            return;
        }

        let mut off = [self.article_offset];
        if has_cached {
            self.cached_ptr = std::ptr::null_mut();
            host.raw(
                4,
                18,
                2,
                self.article_ptr as usize,
                self.article_size as usize,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
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

        struct FlushLockGuard<'a>(&'a DlHost);
        impl Drop for FlushLockGuard<'_> {
            fn drop(&mut self) {
                self.0.raw(
                    4,
                    19,
                    3,
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
        let _flush_guard = if cached {
            host.raw(
                4,
                19,
                3,
                1,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            Some(FlushLockGuard(host))
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

pub struct NzbgetRsArticleDownloader {
    info_name: Vec<u8>,
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

    fn free_connection(&self, host: &DlHost, downloaded_size: &mut i32, keep_connected: bool) {
        if self.conn_ptr() == 0 {
            return;
        }
        host.log(MK_DEBUG, b"Releasing connection");
        let mut cs = self.conn_state.lock().unwrap_or_else(|e| e.into_inner());
        if cs.0 != 0 {
            let mut ints = [0i32; 1];
            host.raw(
                4,
                8,
                i32::from(keep_connected),
                cs.0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                ints.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            *downloaded_size = downloaded_size.wrapping_add(ints[0]);
            cs.0 = 0;
        }
    }

    fn add_conn_bytes(&self, host: &DlHost, downloaded_size: &mut i32) {
        let cptr = self.conn_ptr();
        if cptr == 0 {
            return;
        }
        let mut ints = [0i32; 2];
        host.raw(
            4,
            7,
            0,
            cptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            ints.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        *downloaded_size = downloaded_size.wrapping_add(ints[0]);
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
        let conn_name = self.copy_conn_name();
        w.writing_started = false;
        w.local_write_error = false;
        w.content_rejected = false;
        if let Some(ref mut ca) = w.analyzer {
            ca.reset();
        }

        let mut dints = [0i32; 5];
        let mut dstrs = [std::ptr::null::<c_char>(); 1];
        host.raw(
            4,
            12,
            0,
            cptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            dints.as_mut_ptr(),
            std::ptr::null_mut(),
            dstrs.as_mut_ptr(),
        );
        let raw_article = dints[0] != 0;
        let crc_check = dints[1] != 0;
        let read_chunk = dints[2].max(1);
        let groups_len = dints[3].max(0) as usize;
        let skip_disk_write = dints[4] != 0;
        let message_id = unsafe { cstr_or_empty(dstrs[0]) };

        let is_stopped = || {
            let mut fl = [0i32; 6];
            let mut tm = [0i64; 1];
            host.raw(
                4,
                2,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                fl.as_mut_ptr(),
                tm.as_mut_ptr(),
                std::ptr::null_mut(),
            );
            fl[0] != 0
        };

        if join_group && groups_len > 0 {
            let mut last_resp: Option<Vec<u8>> = None;
            let mut auth_err = false;
            for gidx in 0..groups_len {
                let mut gints = [0i32; 1];
                let mut gstrs = [std::ptr::null::<c_char>(); 1];
                host.raw(
                    4,
                    12,
                    1,
                    cptr,
                    gidx,
                    std::ptr::null(),
                    std::ptr::null(),
                    gints.as_mut_ptr(),
                    std::ptr::null_mut(),
                    gstrs.as_mut_ptr(),
                );
                auth_err = gints[0] != 0;
                last_resp = unsafe { opt_cstr(gstrs[0]) };
                if article_dl_is_join_group_ok(last_resp.as_deref()) {
                    break;
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
        let mut rints = [0i32; 1];
        let mut rstrs = [std::ptr::null::<c_char>(); 1];
        host.raw(
            4,
            12,
            2,
            cptr,
            0,
            cmd_c.as_ptr().cast(),
            std::ptr::null(),
            rints.as_mut_ptr(),
            std::ptr::null_mut(),
            rstrs.as_mut_ptr(),
        );
        let fetch_resp = unsafe { opt_cstr(rstrs[0]) };
        let st = self.check_response(
            host,
            &conn_name,
            fetch_resp.as_deref(),
            rints[0] != 0,
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
            let mut cints = [0i32; 2];
            let mut cstrs = [std::ptr::null::<c_char>(); 1];
            let ok = host.raw(
                4,
                13,
                i32::from(w.writer.has_cached_buffer()),
                cptr,
                read_chunk as usize,
                std::ptr::null(),
                line_buf.as_mut_ptr().cast(),
                cints.as_mut_ptr(),
                std::ptr::null_mut(),
                cstrs.as_mut_ptr(),
            ) != 0;

            if !ok {
                if cints[1] != 0 {
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
                status = AD_FAILED;
                break;
            }

            let raw_len = cints[0] as usize;
            if cints[1] != 0 {
                self.add_conn_bytes(host, &mut w.downloaded_size);
            }
            let buf_ptr = cstrs[0] as *mut u8;
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

        let set_status = |st: i32, touch_now: bool, notify: bool| {
            let mut tm = [if touch_now { host.now() } else { 0 }];
            let flags = usize::from(touch_now) | (usize::from(notify) << 1);
            host.raw(
                4,
                3,
                st,
                flags,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                tm.as_mut_ptr(),
                std::ptr::null_mut(),
            );
        };

        set_status(AD_RUNNING, false, false);

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

            let mut init_ints = [0i32; 14];
            let mut init_i64s = [0i64; 1];
            let mut init_strs = [std::ptr::null::<c_char>(); 4];
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
                init_strs.as_mut_ptr(),
            );
            let retries = init_ints[0];
            let interval_sec = init_ints[1];
            let server_config_gen = init_ints[2];
            let servers_cap = init_ints[3].max(0) as usize;
            let force = init_ints[4] != 0;
            let extra_priority = init_ints[5] != 0;
            let has_desired_server = init_ints[6] != 0;
            let dupe_fallback_round = init_ints[7];
            let file_time = init_i64s[0];

            let mut transient_retries = TransientRetries::new(retries, interval_sec);
            let mut failed_servers: Vec<usize> = Vec::with_capacity(servers_cap);
            let mut want_server: usize = 0;
            let mut level = 0i32;
            let mut status = AD_FAILED;

            let runtime_flags = || {
                let mut fl = [0i32; 6];
                let mut tm = [0i64; 1];
                host.raw(
                    4,
                    2,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    fl.as_mut_ptr(),
                    tm.as_mut_ptr(),
                    std::ptr::null_mut(),
                );
                (
                    fl[0] != 0,
                    fl[1] != 0,
                    fl[2] != 0,
                    fl[3],
                    fl[4],
                    fl[5],
                    tm[0],
                )
            };

            loop {
                let (stopped, _, _, _, _, _, _) = runtime_flags();
                if stopped {
                    break;
                }
                status = AD_FAILED;
                set_status(AD_WAITING, false, false);

                loop {
                    let (st_now, _, _, cur_gen, _, _, now_t) = runtime_flags();
                    if self.conn_ptr() != 0 || st_now || cur_gen != server_config_gen {
                        break;
                    }
                    if transient_retries.is_empty() {
                        let mut ign_i64: Vec<i64> =
                            failed_servers.iter().map(|&p| p as i64).collect();
                        let got = host.raw(
                            4,
                            4,
                            level,
                            want_server,
                            ign_i64.len(),
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null_mut(),
                            if ign_i64.is_empty() {
                                std::ptr::null_mut()
                            } else {
                                ign_i64.as_mut_ptr()
                            },
                            std::ptr::null_mut(),
                        );
                        if got != 0 {
                            self.set_conn_ptr(got);
                        }
                    } else {
                        let waiting = transient_retries.waiting_servers(now_t);
                        if !waiting.contains(&want_server) {
                            let mut ign_i64: Vec<i64> = failed_servers
                                .iter()
                                .chain(waiting.iter())
                                .map(|&p| p as i64)
                                .collect();
                            let got = host.raw(
                                4,
                                4,
                                level,
                                want_server,
                                ign_i64.len(),
                                std::ptr::null(),
                                std::ptr::null(),
                                std::ptr::null_mut(),
                                if ign_i64.is_empty() {
                                    std::ptr::null_mut()
                                } else {
                                    ign_i64.as_mut_ptr()
                                },
                                std::ptr::null_mut(),
                            );
                            if got != 0 {
                                self.set_conn_ptr(got);
                            }
                        }
                    }
                    host.sleep_ms(5);
                }

                set_status(AD_RUNNING, true, false);

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
                let mut sints = [0i32; 5];
                let mut sstrs = [std::ptr::null::<c_char>(); 2];
                let last_server = host.raw(
                    4,
                    5,
                    0,
                    cptr,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    sints.as_mut_ptr(),
                    std::ptr::null_mut(),
                    sstrs.as_mut_ptr(),
                );
                level = sints[0];
                let retention = sints[1];
                let server_id = sints[2];
                let join_group = sints[4] != 0;
                let mut remained_retries = transient_retries.remained_retries(last_server);

                let srv_name = unsafe { cstr_or_empty(sstrs[0]) };
                let conn_host = unsafe { cstr_or_empty(sstrs[1]) };
                let conn_name = article_dl_format_connection_name(&srv_name, &conn_host);
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

                let mut auth_rej_int = [0i32; 1];
                let mut connected = if self.conn_ptr() != 0 {
                    host.raw(
                        4,
                        6,
                        0,
                        self.conn_ptr(),
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        auth_rej_int.as_mut_ptr(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    ) != 0
                } else {
                    false
                };
                let mut transient_retry_scheduled = false;
                let mut auth_rejected = false;
                if !connected && self.conn_ptr() != 0 && auth_rej_int[0] != 0 {
                    if transient_retries.register(last_server, host.now()) {
                        transient_retry_scheduled = true;
                    } else {
                        auth_rejected = true;
                    }
                }

                let (st_after_conn, _, _, _, _, _, _) = runtime_flags();
                if connected && !st_after_conn {
                    status = self.download_article(&mut w, host, join_group);
                    if status == AD_TRANSIENT_ERROR {
                        if transient_retries.register(last_server, host.now()) {
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
                        let mut st_arr = [succ, fail];
                        host.raw(
                            4,
                            7,
                            1,
                            server_id as usize,
                            0,
                            std::ptr::null(),
                            std::ptr::null(),
                            st_arr.as_mut_ptr(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        );
                    }
                }

                if self.conn_ptr() != 0 {
                    self.add_conn_bytes(host, &mut w.downloaded_size);
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

                let (st_end_iter, paused_end, held_end, gen_end, max_norm_lvl, srv_len, _) =
                    runtime_flags();
                let mut optional_blocked = false;
                if !connected && self.conn_ptr() != 0 && !st_end_iter {
                    optional_blocked = host.raw(
                        4,
                        10,
                        0,
                        last_server,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    ) != 0;
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

                    let cnt = srv_len.max(0) as usize;
                    let mut cand_ids = Vec::with_capacity(cnt);
                    let mut cand_lvls = Vec::with_capacity(cnt);
                    let mut cand_grps = Vec::with_capacity(cnt);
                    let mut cand_avail = Vec::with_capacity(cnt);
                    let mut cand_ptrs = Vec::with_capacity(cnt);
                    for si in 0..cnt {
                        let mut cints = [0i32; 4];
                        let sp = host.raw(
                            4,
                            11,
                            si as i32,
                            0,
                            0,
                            std::ptr::null(),
                            std::ptr::null(),
                            cints.as_mut_ptr(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        );
                        cand_ptrs.push(sp);
                        cand_ids.push(cints[0]);
                        cand_lvls.push(cints[1]);
                        cand_grps.push(cints[2]);
                        cand_avail.push(cints[3] as u8);
                    }

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

        set_status(final_status, false, true);
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
    // Pure static getters that require no host callback
    match op {
        102 => return memory_monitor_tier() as usize,
        103 => return memory_monitor_tier_name_c() as usize,
        104 => return memory_monitor_effective_cache_mb() as usize,
        105 => return memory_monitor_effective_par_buffer_mb(a1) as usize,
        106 => return memory_monitor_effective_downloads_limit(a1) as usize,
        125 => return usize::from(server_pool_is_blocked(p1 as i64, p2 as i64, a1)),
        _ => {}
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
            100 => {
                memory_monitor_init_options(&host, a1 != 0);
                0
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
            120 => {
                server_pool_init_connections(&host);
                0
            }
            121 => server_pool_get_connection(&host, a1, p1, p2),
            122 => {
                host.raw(
                    3,
                    5,
                    2,
                    p1,
                    p2,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                0
            }
            123 => {
                server_pool_close_unused(&host);
                0
            }
            124 => {
                server_pool_block_server(&host, p1);
                0
            }
            126 => {
                server_pool_log_debug(&host);
                0
            }
            _ => 0,
        };
    }

    let dl = unsafe { &*ptr };
    match op {
        0 => {
            dl.run(&host);
            0
        }
        10 => {
            // Snapshot for QueueCoordinator (`case 51, a1 == 0`)
            let w = dl.worker.lock().unwrap_or_else(|e| e.into_inner());
            let ints_out = unsafe { std::slice::from_raw_parts_mut(p1 as *mut c_int, 4) };
            let i64s_out = unsafe { std::slice::from_raw_parts_mut(p2 as *mut i64, 4) };
            let strs_out = unsafe { std::slice::from_raw_parts_mut(s1 as *mut *const c_char, 1) };
            ints_out[0] = a1; // current atomic status passed by C++
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
            1
        }
        11 => {
            // Finish content analyzer (`case 51, a1 == 4`)
            let mut w = dl.worker.lock().unwrap_or_else(|e| e.into_inner());
            if !w.has_analyzer {
                return 0;
            }
            let ints_out = unsafe { std::slice::from_raw_parts_mut(p1 as *mut c_int, 1) };
            let strs_out = unsafe { std::slice::from_raw_parts_mut(s1 as *mut *const c_char, 2) };
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
            1
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
        14 => {
            // LogDebugInfo (`a1` = status, `p1` = last_update_time)
            let info_c = nul_vec(&dl.info_name);
            let mut t = [p1 as i64];
            host.raw(
                1,
                12,
                a1,
                0,
                0,
                info_c.as_ptr().cast(),
                std::ptr::null(),
                std::ptr::null_mut(),
                t.as_mut_ptr(),
                std::ptr::null_mut(),
            );
            0
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
        16 => {
            // Stop connection cancel (`ArticleDownloader::Stop`)
            let cs = dl.conn_state.lock().unwrap_or_else(|e| e.into_inner());
            if cs.0 != 0 {
                host.raw(
                    4,
                    9,
                    0,
                    cs.0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
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
}
