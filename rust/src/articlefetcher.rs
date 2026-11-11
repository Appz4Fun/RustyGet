//! Native Rust implementation of `ArticleFetcher`, `ArticleBatchFetcher` pool,
//! `TwinCheck::FetchEntry`, and in-memory donor NZB parsing (`ParseDonorNzb`).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{c_char, c_int, CStr};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::decoder::{rust_yenc_crc, rust_yenc_decode_incremental, Decoder, Format, Status};
use crate::dupefallback::{
    article_fetch_add_decoded_bytes, article_fetch_add_raw_bytes, article_fetch_header_within_limit,
    article_size_steps_hash,
};
use crate::nntp::{ArticleBatchFetcher, BatchFetchedArticle, BatchRequest};
use crate::nzbfile::{Ctype, NzbFileParser};
use crate::twin::NzbEntry;

/// Shared C++ NNTP host callback (`NzbgetCArticleFetchHost` in `Util.h`):
/// - `op == 0`: global limits/status -> `ints[0]=max_norm_level`, `ints[1]=quota_reached`,
///   `ints[2]=read_chunk`, `ints[3]=server_count`; `i64s[0]=speed_limit`,
///   `i64s[1]=cur_speed` (if `speed_limit > 0`), `i64s[2]=mom_speed` (if `speed_limit > 0`)
/// - `op == 1`: server at index `a1` -> `ints[0]=active`, `ints[1]=norm_level`, `ints[2]=group`,
///   `ints[3]=id`, `ints[4]=max_connections`, `ints[5]=join_group`, `strs[0]=name`; returns `server_ptr`
/// - `op == 2`: `GetConnection(level=a1, nullptr, failed_ptrs=buf[..a2])` -> returns `conn_ptr` (0 if none),
///   `ints[0]=reused`, `ints[1]=server_id`, `ints[2]=max_connections`, `ints[3]=join_group`,
///   `i64s[0]=server_ptr`, `strs[0]=server_name`
/// - `op == 3`: release connection `p1` (`a1=keep_connected`) -> `AddServerStats`, `Disconnect` if `!a1`
///   or `csCancelled`, `FreeConnection(conn, true)`
/// - `op == 4`: connect `p1` -> fills `strs[0]=server_name`, `ints[0]=join_group`, returns `1` on `Connect()`
/// - `op == 5`: `JoinGroup(s1)` on `p1` -> `strs[0]=response`, returns `1` if non-null
/// - `op == 6`: `Request(s1)` on `p1` -> `strs[0]=response`, returns `1` if non-null
/// - `op == 7`: `ReadBuffer` / `TryRecv(buf, buf_len)` on `p1` + `AddSpeedReading` ->
///   `ints[0]=len`, `strs[0]=buffer_ptr`, returns `1` if `len > 0`
/// - `op == 8`: `Disconnect()` on `p1`
/// - `op == 9`: `SetSuppressErrors(true); Cancel();` on `p1`
/// - `op == 10`: `detail("%s", s1)`
pub type NzbgetRsNntpFetchHostFn = unsafe extern "C" fn(
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    s1: *const c_char,
    ints: *mut c_int,
    i64s: *mut i64,
    strs: *mut *const c_char,
    buf: *mut u8,
    buf_len: usize,
) -> usize;

pub const MAX_TRANSIENT_ATTEMPTS: i32 = 4;

#[inline]
pub fn transient_retry_delay(attempt: i32) -> i32 {
    attempt
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FetchedArticle {
    pub data: Vec<u8>,
    pub offset: i64,
    pub file_size: i64,
    pub success: bool,
    pub retry: bool,
    pub transient: bool,
}

static UNREACHABLE_SERVERS: OnceLock<Mutex<BTreeSet<i32>>> = OnceLock::new();

fn unreachable_set() -> &'static Mutex<BTreeSet<i32>> {
    UNREACHABLE_SERVERS.get_or_init(|| Mutex::new(BTreeSet::new()))
}

pub fn server_unreachable(server_id: i32) -> bool {
    unreachable_set()
        .lock()
        .map(|s| s.contains(&server_id))
        .unwrap_or(false)
}

pub fn set_server_unreachable(server_id: i32, unreachable: bool) {
    if let Ok(mut s) = unreachable_set().lock() {
        if unreachable {
            s.insert(server_id);
        } else {
            s.remove(&server_id);
        }
    }
}

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

#[derive(Clone, Copy)]
struct HostStatus {
    max_norm_level: i32,
    quota_reached: bool,
    read_chunk: usize,
    server_count: usize,
    speed_limit: i64,
    cur_speed: i64,
    mom_speed: i64,
}

fn query_host_status(host: NzbgetRsNntpFetchHostFn) -> HostStatus {
    let mut ints = [0 as c_int; 4];
    let mut i64s = [0i64; 3];
    unsafe {
        host(
            0,
            0,
            0,
            0,
            ptr::null(),
            ints.as_mut_ptr(),
            i64s.as_mut_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        );
    }
    HostStatus {
        max_norm_level: ints[0],
        quota_reached: ints[1] != 0,
        read_chunk: ints[2].max(1) as usize,
        server_count: ints[3].max(0) as usize,
        speed_limit: i64s[0],
        cur_speed: i64s[1],
        mom_speed: i64s[2],
    }
}

pub fn emit_detail(host: NzbgetRsNntpFetchHostFn, msg: &[u8]) {
    let c = to_c_bytes(msg);
    unsafe {
        host(
            10,
            0,
            0,
            0,
            c.as_ptr() as *const c_char,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        );
    }
}

pub struct ArticleFetcher {
    host: NzbgetRsNntpFetchHostFn,
    wait_counter: Mutex<Option<Arc<AtomicI32>>>,
    stopped: AtomicBool,
    conn_ptr: Mutex<usize>,
}

impl ArticleFetcher {
    pub fn new(host: NzbgetRsNntpFetchHostFn) -> Self {
        Self {
            host,
            wait_counter: Mutex::new(None),
            stopped: AtomicBool::new(false),
            conn_ptr: Mutex::new(0),
        }
    }

    pub fn set_wait_counter(&self, counter: Option<Arc<AtomicI32>>) {
        if let Ok(mut g) = self.wait_counter.lock() {
            *g = counter;
        }
    }

    #[inline]
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Ok(guard) = self.conn_ptr.lock() {
            let conn = *guard;
            if conn != 0 {
                unsafe {
                    (self.host)(
                        9,
                        0,
                        0,
                        conn,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        0,
                    );
                }
            }
        }
    }

    fn wait_before_retry(&self, seconds: i32) -> bool {
        let ticks = seconds.saturating_mul(10);
        let mut waited = 0;
        while waited < ticks && !self.is_stopped() {
            std::thread::sleep(Duration::from_millis(100));
            waited += 1;
        }
        !self.is_stopped()
    }

    fn release_connection(&self, conn: usize, keep_connected: bool) {
        let mut guard = self.conn_ptr.lock().unwrap();
        unsafe {
            (self.host)(
                3,
                c_int::from(keep_connected),
                0,
                conn,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
            );
        }
        if *guard == conn {
            *guard = 0;
        }
    }

    pub fn fetch(&self, message_id: &[u8], groups: &[Vec<u8>]) -> FetchedArticle {
        let mut result = FetchedArticle::default();
        let mut failed_servers: Vec<(usize, i32, i32)> = Vec::new();
        let mut failed_ptrs: Vec<usize> = Vec::new();
        let mut transient_attempts: BTreeMap<usize, i32> = BTreeMap::new();
        let mut stale_connections: BTreeMap<usize, i32> = BTreeMap::new();

        let mut level: i32 = 0;
        while !self.is_stopped() {
            let mut st = query_host_status(self.host);
            if level > st.max_norm_level {
                break;
            }
            while st.quota_reached && !self.is_stopped() {
                std::thread::sleep(Duration::from_millis(100));
                st = query_host_status(self.host);
            }
            if self.is_stopped() {
                break;
            }

            st = query_host_status(self.host);
            let mut eligible = 0i32;
            for idx in 0..st.server_count {
                let mut s_ints = [0 as c_int; 6];
                let mut s_strs = [ptr::null::<c_char>(); 1];
                let s_ptr = unsafe {
                    (self.host)(
                        1,
                        idx as c_int,
                        0,
                        0,
                        ptr::null(),
                        s_ints.as_mut_ptr(),
                        ptr::null_mut(),
                        s_strs.as_mut_ptr(),
                        ptr::null_mut(),
                        0,
                    )
                };
                if s_ints[0] == 0 || s_ints[1] != level {
                    continue;
                }
                let s_group = s_ints[2];
                let server_failed = failed_servers.iter().any(|&(fp, fg, fl)| {
                    fp == s_ptr || (fg > 0 && fg == s_group && fl == level)
                });
                if !server_failed {
                    eligible += 1;
                }
            }
            if eligible == 0 {
                level += 1;
                continue;
            }

            let wc = self.wait_counter.lock().ok().and_then(|g| g.clone());
            struct WaitGuard(Option<Arc<AtomicI32>>);
            impl Drop for WaitGuard {
                fn drop(&mut self) {
                    if let Some(ref c) = self.0 {
                        c.fetch_sub(1, Ordering::SeqCst);
                    }
                }
            }
            if let Some(ref c) = wc {
                c.fetch_add(1, Ordering::SeqCst);
            }
            let wait_guard = WaitGuard(wc);

            let mut conn: usize = 0;
            let mut reused = false;
            let mut server_id: i32 = 0;
            let mut max_connections: i32 = 0;
            let mut join_group = false;
            let mut server_ptr: usize = 0;
            let mut server_group_of_conn: i32 = 0;
            let mut server_name: Vec<u8> = Vec::new();

            while conn == 0 && !self.is_stopped() {
                let cur_st = query_host_status(self.host);
                if cur_st.quota_reached {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                let mut c_ints = [0 as c_int; 5];
                let mut c_i64s = [0i64; 1];
                let mut c_strs = [ptr::null::<c_char>(); 1];
                conn = unsafe {
                    (self.host)(
                        2,
                        level,
                        failed_ptrs.len() as c_int,
                        0,
                        ptr::null(),
                        c_ints.as_mut_ptr(),
                        c_i64s.as_mut_ptr(),
                        c_strs.as_mut_ptr(),
                        if failed_ptrs.is_empty() {
                            ptr::null_mut()
                        } else {
                            failed_ptrs.as_mut_ptr() as *mut u8
                        },
                        failed_ptrs.len(),
                    )
                };
                if conn != 0 {
                    reused = c_ints[0] != 0;
                    server_id = c_ints[1];
                    max_connections = c_ints[2];
                    join_group = c_ints[3] != 0;
                    server_group_of_conn = c_ints[4];
                    server_ptr = c_i64s[0] as usize;
                    server_name = unsafe { ptr_to_bytes(c_strs[0]) };
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            drop(wait_guard);

            if conn == 0 {
                level += 1;
                continue;
            }

            let stopped_now = {
                let mut guard = self.conn_ptr.lock().unwrap();
                *guard = conn;
                self.is_stopped()
            };
            if stopped_now {
                self.release_connection(conn, true);
                break;
            }

            result = self.fetch_from_connection_with_meta(
                conn,
                Some((&server_name, join_group)),
                message_id,
                groups,
            );
            self.release_connection(conn, true);

            if result.success || !result.transient {
                set_server_unreachable(server_id, false);
            }
            if result.success {
                return result;
            }
            if result.retry {
                while query_host_status(self.host).quota_reached && !self.is_stopped() {
                    std::thread::sleep(Duration::from_millis(100));
                }
                continue;
            }
            if result.transient && !server_unreachable(server_id) {
                if reused {
                    let stale = stale_connections.entry(server_ptr).or_insert(0);
                    *stale += 1;
                    if *stale <= max_connections {
                        continue;
                    }
                }
                let attempt = {
                    let entry = transient_attempts.entry(server_ptr).or_insert(0);
                    *entry += 1;
                    *entry
                };
                if attempt < MAX_TRANSIENT_ATTEMPTS {
                    if self.wait_before_retry(transient_retry_delay(attempt)) {
                        continue;
                    }
                    break;
                }
                set_server_unreachable(server_id, true);
            }

            failed_servers.push((server_ptr, server_group_of_conn, level));
            failed_ptrs.push(server_ptr);
        }

        result
    }

    pub fn fetch_from_connection(
        &self,
        conn: usize,
        message_id: &[u8],
        groups: &[Vec<u8>],
    ) -> FetchedArticle {
        self.fetch_from_connection_with_meta(conn, None, message_id, groups)
    }

    fn fetch_from_connection_with_meta(
        &self,
        conn: usize,
        known_meta: Option<(&[u8], bool)>,
        message_id: &[u8],
        groups: &[Vec<u8>],
    ) -> FetchedArticle {
        let mut result = FetchedArticle::default();
        if self.is_stopped() || conn == 0 {
            return result;
        }

        let mut c_ints = [0 as c_int; 1];
        let mut c_strs = [ptr::null::<c_char>(); 1];
        let connected = unsafe {
            (self.host)(
                4,
                0,
                0,
                conn,
                ptr::null(),
                c_ints.as_mut_ptr(),
                ptr::null_mut(),
                c_strs.as_mut_ptr(),
                ptr::null_mut(),
                0,
            )
        };
        let owned_name;
        let (server_name, join_group): (&[u8], bool) = match known_meta {
            Some((name, jg)) => (name, jg),
            None => {
                owned_name = unsafe { ptr_to_bytes(c_strs[0]) };
                (&owned_name, c_ints[0] != 0)
            }
        };

        let msg_clean_end = message_id
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(message_id.len());
        let msg_slice = &message_id[..msg_clean_end];

        if connected == 0 {
            let mut log = b"Stream repair: could not connect to ".to_vec();
            log.extend_from_slice(server_name);
            emit_detail(self.host, &log);
            result.transient = true;
            return result;
        }

        let disconnect = || unsafe {
            (self.host)(
                8,
                0,
                0,
                conn,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
            );
        };

        if join_group {
            let mut joined_resp: Option<Vec<u8>> = None;
            for group in groups {
                let grp_c = to_c_bytes(group);
                let mut g_strs = [ptr::null::<c_char>(); 1];
                let ok = unsafe {
                    (self.host)(
                        5,
                        0,
                        0,
                        conn,
                        grp_c.as_ptr() as *const c_char,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        g_strs.as_mut_ptr(),
                        ptr::null_mut(),
                        0,
                    )
                };
                if ok != 0 && !g_strs[0].is_null() {
                    let resp = unsafe { ptr_to_bytes(g_strs[0]) };
                    let is_ok = resp.first() == Some(&b'2');
                    joined_resp = Some(resp);
                    if is_ok {
                        break;
                    }
                } else {
                    joined_resp = None;
                }
            }
            let joined_ok = joined_resp
                .as_ref()
                .is_some_and(|r| r.first() == Some(&b'2'));
            if !joined_ok {
                let mut log = b"Stream repair: could not join groups on ".to_vec();
                log.extend_from_slice(server_name);
                log.extend_from_slice(b" for ");
                log.extend_from_slice(msg_slice);
                emit_detail(self.host, &log);
                result.transient = joined_resp.is_none();
                if result.transient {
                    disconnect();
                }
                return result;
            }
        }

        let mut body_cmd = Vec::with_capacity(msg_slice.len() + 8);
        body_cmd.extend_from_slice(b"BODY ");
        body_cmd.extend_from_slice(msg_slice);
        body_cmd.extend_from_slice(b"\r\n\0");
        let mut r_strs = [ptr::null::<c_char>(); 1];
        let req_ok = unsafe {
            (self.host)(
                6,
                0,
                0,
                conn,
                body_cmd.as_ptr() as *const c_char,
                ptr::null_mut(),
                ptr::null_mut(),
                r_strs.as_mut_ptr(),
                ptr::null_mut(),
                0,
            )
        };
        if req_ok == 0 || r_strs[0].is_null() {
            let mut log = b"Stream repair: no response from ".to_vec();
            log.extend_from_slice(server_name);
            log.extend_from_slice(b" for ");
            log.extend_from_slice(msg_slice);
            emit_detail(self.host, &log);
            disconnect();
            result.transient = true;
            return result;
        }
        let resp_first = unsafe { *r_strs[0] as u8 };
        if resp_first != b'2' {
            let mut log = b"Stream repair: article ".to_vec();
            log.extend_from_slice(msg_slice);
            log.extend_from_slice(b" not available on ");
            log.extend_from_slice(server_name);
            emit_detail(self.host, &log);
            return result;
        }

        let mut decoder = Decoder::new(rust_yenc_decode_incremental, rust_yenc_crc);
        decoder.clear();
        decoder.crc_check = true;
        decoder.raw_mode = false;

        let read_chunk = query_host_status(self.host).read_chunk;
        let mut recv_buf = vec![0u8; read_chunk + 128];
        let mut raw_bytes: i64 = 0;
        let mut decoded_bytes: i64 = 0;

        while !self.is_stopped() && !decoder.eof {
            let st0 = query_host_status(self.host);
            if st0.quota_reached {
                disconnect();
                result.retry = true;
                return result;
            }
            while !self.is_stopped() {
                let st_lim = query_host_status(self.host);
                if st_lim.quota_reached
                    || st_lim.speed_limit <= 0
                    || (st_lim.cur_speed <= st_lim.speed_limit
                        && st_lim.mom_speed <= st_lim.speed_limit)
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            if query_host_status(self.host).quota_reached {
                disconnect();
                result.retry = true;
                return result;
            }
            if self.is_stopped() {
                break;
            }

            let mut rd_ints = [0 as c_int; 1];
            let mut rd_strs = [ptr::null::<c_char>(); 1];
            let rd_ok = unsafe {
                (self.host)(
                    7,
                    0,
                    0,
                    conn,
                    ptr::null(),
                    rd_ints.as_mut_ptr(),
                    ptr::null_mut(),
                    rd_strs.as_mut_ptr(),
                    recv_buf.as_mut_ptr(),
                    read_chunk,
                )
            };
            let len = rd_ints[0];
            if rd_ok == 0 || len <= 0 || rd_strs[0].is_null() {
                let mut log = b"Stream repair: connection to ".to_vec();
                log.extend_from_slice(server_name);
                log.extend_from_slice(b" lost while fetching ");
                log.extend_from_slice(msg_slice);
                emit_detail(self.host, &log);
                disconnect();
                return FetchedArticle {
                    transient: true,
                    ..Default::default()
                };
            }

            if !article_fetch_add_raw_bytes(&mut raw_bytes, len as i64) {
                let mut log = b"Stream repair: article ".to_vec();
                log.extend_from_slice(msg_slice);
                log.extend_from_slice(b" from ");
                log.extend_from_slice(server_name);
                log.extend_from_slice(b" exceeded the wire-size limit");
                emit_detail(self.host, &log);
                disconnect();
                return result;
            }

            let buffer_ptr = rd_strs[0] as *mut u8;
            let decoded_len = unsafe { decoder.decode_buffer(buffer_ptr, len as usize) };
            let declared_range_known = decoder.begin_pos > 0
                && decoder.end_pos >= decoder.begin_pos
                && decoder.size > 0;
            if !article_fetch_header_within_limit(raw_bytes, declared_range_known) {
                let mut log = b"Stream repair: article ".to_vec();
                log.extend_from_slice(msg_slice);
                log.extend_from_slice(b" from ");
                log.extend_from_slice(server_name);
                log.extend_from_slice(b" exceeded the yEnc-header limit");
                emit_detail(self.host, &log);
                disconnect();
                return result;
            }
            if declared_range_known
                && !article_fetch_add_decoded_bytes(
                    &mut decoded_bytes,
                    decoded_len as i64,
                    decoder.begin_pos,
                    decoder.end_pos,
                    decoder.size,
                )
            {
                let mut log = b"Stream repair: article ".to_vec();
                log.extend_from_slice(msg_slice);
                log.extend_from_slice(b" from ");
                log.extend_from_slice(server_name);
                log.extend_from_slice(b" exceeded its declared decoded range");
                emit_detail(self.host, &log);
                disconnect();
                return result;
            }
            if decoded_len > 0 {
                let slice = unsafe { std::slice::from_raw_parts(buffer_ptr, decoded_len) };
                result.data.extend_from_slice(slice);
            }
        }

        if self.is_stopped() {
            disconnect();
            return result;
        }

        if decoder.check() != Status::Finished
            || decoder.format != Format::Yenc
            || decoder.begin_pos <= 0
            || decoder.end_pos <= 0
            || result.data.is_empty()
        {
            let mut log = b"Stream repair: article ".to_vec();
            log.extend_from_slice(msg_slice);
            log.extend_from_slice(b" from ");
            log.extend_from_slice(server_name);
            log.extend_from_slice(b" failed decoding");
            emit_detail(self.host, &log);
            return result;
        }

        result.offset = decoder.begin_pos - 1;
        result.file_size = decoder.size;
        result.success = (result.data.len() as i64) == decoder.end_pos - decoder.begin_pos + 1;
        result
    }
}

struct BatchPoolState {
    worker_count: i32,
    wait_counter: Option<Arc<AtomicI32>>,
    inline_fetcher: Arc<ArticleFetcher>,
    worker_fetchers: Vec<Arc<ArticleFetcher>>,
}

/// Native Rust equivalent of `ArticleBatchFetcher` in `Util.h`, pairing
/// [`crate::nntp::ArticleBatchFetcher`] with a pool of [`ArticleFetcher`]s.
pub struct BatchFetcherPool {
    host: NzbgetRsNntpFetchHostFn,
    state: Arc<Mutex<BatchPoolState>>,
    batch: ArticleBatchFetcher,
}

impl BatchFetcherPool {
    pub fn new(host: NzbgetRsNntpFetchHostFn) -> Self {
        let state = Arc::new(Mutex::new(BatchPoolState {
            worker_count: 0,
            wait_counter: None,
            inline_fetcher: Arc::new(ArticleFetcher::new(host)),
            worker_fetchers: Vec::new(),
        }));
        let state_fetch = Arc::clone(&state);
        let state_stop = Arc::clone(&state);
        let stop_hook: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if let Ok(guard) = state_stop.lock() {
                guard.inline_fetcher.stop();
                for wf in &guard.worker_fetchers {
                    wf.stop();
                }
            }
        });
        let batch = ArticleBatchFetcher::new(
            move |worker_idx, req: &BatchRequest| {
                let fetcher = {
                    let guard = state_fetch.lock().unwrap();
                    if worker_idx != usize::MAX && worker_idx < guard.worker_fetchers.len() {
                        Arc::clone(&guard.worker_fetchers[worker_idx])
                    } else {
                        Arc::clone(&guard.inline_fetcher)
                    }
                };
                let fetched = fetcher.fetch(&req.message_id, &req.groups);
                Ok(BatchFetchedArticle {
                    data: fetched.data,
                    offset: fetched.offset,
                    file_size: fetched.file_size,
                    success: fetched.success,
                    retry: fetched.retry,
                    transient: fetched.transient,
                })
            },
            Some(stop_hook),
        );
        Self { host, state, batch }
    }

    pub fn set_worker_count(&self, worker_count: i32) {
        if let Ok(mut guard) = self.state.lock() {
            guard.worker_count = worker_count;
        }
        self.batch.set_worker_count(worker_count);
    }

    pub fn set_wait_counter(&self, counter: Option<Arc<AtomicI32>>) {
        if let Ok(mut guard) = self.state.lock() {
            guard.wait_counter = counter.clone();
            guard.inline_fetcher.set_wait_counter(counter.clone());
            for wf in &guard.worker_fetchers {
                wf.set_wait_counter(counter.clone());
            }
        }
    }

    fn ensure_worker_fetchers(&self) {
        if let Ok(mut guard) = self.state.lock() {
            let target = guard.worker_count.max(0) as usize;
            let wc = guard.wait_counter.clone();
            while guard.worker_fetchers.len() < target {
                let wf = Arc::new(ArticleFetcher::new(self.host));
                wf.set_wait_counter(wc.clone());
                guard.worker_fetchers.push(wf);
            }
        }
    }

    pub fn begin(&self, requests: Vec<BatchRequest>) {
        self.ensure_worker_fetchers();
        self.batch.begin(requests);
    }

    pub fn next(&self) -> Option<BatchFetchedArticle> {
        self.batch.next()
    }

    pub fn cancel_remaining(&self) {
        self.batch.cancel_remaining();
    }

    pub fn stop(&self) {
        self.batch.stop();
    }
}

/// Port of `TwinCheck::FetchEntry`: fetches and reassembles `entry` segments up to `max_size`.
pub fn fetch_entry(
    fetcher: &ArticleFetcher,
    entry: &NzbEntry,
    max_size: i64,
    is_stopped: impl Fn() -> bool,
) -> Option<Vec<u8>> {
    let groups: Vec<Vec<u8>> = entry.groups.iter().map(|g| g.as_bytes().to_vec()).collect();
    let mut data = Vec::new();
    for (_, seg_id) in &entry.segments {
        if is_stopped() {
            return None;
        }
        let part = fetcher.fetch(seg_id.as_bytes(), &groups);
        if !part.success
            || part.offset < 0
            || part.offset.saturating_add(part.data.len() as i64) > max_size
        {
            continue;
        }
        let off = part.offset as usize;
        let end_len = off + part.data.len();
        if end_len > data.len() {
            data.resize(end_len, 0);
        }
        data[off..end_len].copy_from_slice(&part.data);
    }
    (!data.is_empty()).then_some(data)
}

#[derive(Clone, Debug)]
pub struct DonorParsedFile {
    pub filename: Vec<u8>,
    pub subject: Vec<u8>,
    pub size: i64,
    pub par_file: bool,
    pub total_articles: i32,
    pub groups: Arc<Vec<Vec<u8>>>,
    pub article_parts: Vec<i32>,
    pub article_sizes: Vec<i32>,
    pub article_msg_ids: Vec<Vec<u8>>,
    pub steps_hash: u64,
}

#[derive(Clone, Debug)]
pub struct DonorParsedNzb {
    pub files: Vec<DonorParsedFile>,
}

/// In-memory port of `DupeArticleFallback::ParseDonorNzb` using [`NzbFileParser::parse_file`]
/// directly without writing or discarding temporary `<QueueDir>/f<id>` disk-state files.
#[cfg(unix)]
pub fn parse_donor_nzb_file(
    queued_filename: &[u8],
    par_ignore_ext: &[u8],
    mut detail_log: impl FnMut(&[u8]),
) -> Option<DonorParsedNzb> {
    if queued_filename.is_empty()
        || !crate::fsops::file_exists(queued_filename, &crate::fsops::posix_stat)
    {
        return None;
    }

    let mut classes = [0u16; 256];
    let mut lower = [0i32; 384];
    for c in 0..256i32 {
        let sp = if unsafe { libc::isspace(c) } != 0 {
            1u16
        } else {
            0
        };
        let dg = if unsafe { libc::isdigit(c) } != 0 {
            2u16
        } else {
            0
        };
        classes[c as usize] = sp | dg;
    }
    for c in -128..256i32 {
        lower[(c + 128) as usize] = if c < 0 {
            c
        } else {
            unsafe { libc::tolower(c) }
        };
    }
    let ct = Ctype {
        classes: &classes,
        space: 1,
        digit: 2,
        lower: &lower,
        char_signed: c_char::MIN < 0,
    };

    let mut parser = NzbFileParser::new(queued_filename);
    let mut next_temp_id = 0i32;
    let ok = parser.parse_file(
        par_ignore_ext,
        &ct,
        || {
            next_temp_id = next_temp_id.wrapping_add(1);
            next_temp_id
        },
        |subj, fname| {
            let mut m1 = b"Extracting a filename from Subject ".to_vec();
            m1.extend_from_slice(subj.to_bytes());
            detail_log(&m1);
            let mut m2 = b"Extracted Filename: ".to_vec();
            m2.extend_from_slice(fname.to_bytes());
            detail_log(&m2);
        },
        Some(|filenames: &[*const c_char], indices: &mut [usize]| {
            indices.sort_by(|&a, &b| {
                let sa = if filenames[a].is_null() {
                    &[][..]
                } else {
                    unsafe { CStr::from_ptr(filenames[a]).to_bytes() }
                };
                let sb = if filenames[b].is_null() {
                    &[][..]
                } else {
                    unsafe { CStr::from_ptr(filenames[b]).to_bytes() }
                };
                sb.cmp(sa)
            });
        }),
    );

    if !ok {
        let mut msg = b"Could not parse duplicate nzb-file ".to_vec();
        msg.extend_from_slice(queued_filename);
        detail_log(&msg);
        return None;
    }

    let mut files = Vec::with_capacity(parser.files.len());
    for f in parser.files {
        let article_sizes: Vec<i32> = f.article_sizes.iter().map(|&s| s as i32).collect();
        let sh = article_size_steps_hash(&article_sizes);
        let groups = Arc::new(
            f.groups
                .into_iter()
                .map(|g| g.into_bytes())
                .collect::<Vec<Vec<u8>>>(),
        );
        let article_msg_ids = f
            .article_msg_ids
            .into_iter()
            .map(|m| m.into_bytes())
            .collect();
        files.push(DonorParsedFile {
            filename: f.filename.into_bytes(),
            subject: f.subject.into_bytes(),
            size: f.size,
            par_file: f.par_file,
            total_articles: f.total_articles,
            groups,
            article_parts: f.article_parts,
            article_sizes,
            article_msg_ids,
            steps_hash: sh,
        });
    }
    Some(DonorParsedNzb { files })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yenc_encode_single_part(payload: &[u8]) -> Vec<u8> {
        let crc = unsafe { rust_yenc_crc(payload.as_ptr().cast(), payload.len(), 0) };
        let mut out = format!(
            "=ybegin part=1 total=1 line=128 size={} name=test.bin\r\n=ypart begin=1 end={}\r\n",
            payload.len(),
            payload.len()
        )
        .into_bytes();
        for &b in payload {
            let enc = b.wrapping_add(42);
            if matches!(enc, 0 | b'\n' | b'\r' | b'=') {
                out.push(b'=');
                out.push(enc.wrapping_add(64));
            } else {
                out.push(enc);
            }
        }
        out.extend_from_slice(
            format!("\r\n=yend size={} part=1 pcrc32={crc:08x}\r\n.\r\n", payload.len()).as_bytes(),
        );
        out
    }

    static TEST_WIRE_PAYLOAD: OnceLock<Vec<u8>> = OnceLock::new();
    static TEST_READ_POS: AtomicI32 = AtomicI32::new(0);

    unsafe extern "C" fn mock_nntp_host(
        op: c_int,
        _a1: c_int,
        _a2: c_int,
        _p1: usize,
        _s1: *const c_char,
        ints: *mut c_int,
        i64s: *mut i64,
        strs: *mut *const c_char,
        buf: *mut u8,
        buf_len: usize,
    ) -> usize {
        match op {
            0 => {
                let s = unsafe { std::slice::from_raw_parts_mut(ints, 4) };
                s[0] = 0;
                s[1] = 0;
                s[2] = 64;
                s[3] = 1;
                let l = unsafe { std::slice::from_raw_parts_mut(i64s, 3) };
                l[0] = 0;
                l[1] = 0;
                l[2] = 0;
                0
            }
            1 => {
                let s = unsafe { std::slice::from_raw_parts_mut(ints, 6) };
                s[0] = 1;
                s[1] = 0;
                s[2] = 0;
                s[3] = 7;
                s[4] = 4;
                s[5] = 0;
                unsafe { *strs = c"mock.nntp".as_ptr() };
                0x1000
            }
            2 => {
                let s = unsafe { std::slice::from_raw_parts_mut(ints, 5) };
                s[0] = 0;
                s[1] = 7;
                s[2] = 4;
                s[3] = 0;
                s[4] = 0;
                unsafe { *i64s = 0x1000 };
                unsafe { *strs = c"mock.nntp".as_ptr() };
                0x2000
            }
            3 | 8 | 9 | 10 => 0,
            4 => {
                unsafe { *ints = 0 };
                unsafe { *strs = c"mock.nntp".as_ptr() };
                1
            }
            6 => {
                unsafe { *strs = c"222 0 <seg1@test> body".as_ptr() };
                1
            }
            7 => {
                let wire = TEST_WIRE_PAYLOAD.get().unwrap();
                let pos = TEST_READ_POS.load(Ordering::SeqCst) as usize;
                if pos >= wire.len() {
                    unsafe { *ints = 0 };
                    return 0;
                }
                let take = (wire.len() - pos).min(buf_len);
                unsafe {
                    ptr::copy_nonoverlapping(wire.as_ptr().add(pos), buf, take);
                    *ints = take as c_int;
                    *strs = buf as *const c_char;
                }
                TEST_READ_POS.store((pos + take) as i32, Ordering::SeqCst);
                1
            }
            _ => 0,
        }
    }

    #[test]
    fn article_fetcher_decodes_yenc_stream_and_reassembles_entry() {
        let expected = b"Hello from Rust ArticleFetcher yEnc test payload!";
        let _ = TEST_WIRE_PAYLOAD.set(yenc_encode_single_part(expected));
        TEST_READ_POS.store(0, Ordering::SeqCst);

        let fetcher = ArticleFetcher::new(mock_nntp_host);
        let entry = NzbEntry {
            subject: "\"test.bin\" yEnc (1/1)".to_string(),
            filename: "test.bin".to_string(),
            groups: vec!["alt.binaries.test".to_string()],
            segments: vec![(expected.len() as u64, "<seg1@test>".to_string())],
        };
        let got = fetch_entry(&fetcher, &entry, 1024 * 1024, || false).expect("fetched entry");
        assert_eq!(got, expected);
    }
}
