//! Native Rust implementation of `ArticleFetcher`, `ArticleBatchFetcher` pool,
//! `TwinCheck::FetchEntry`, and in-memory donor NZB parsing (`ParseDonorNzb`).

use crate::cstr;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{c_char, c_int, CStr};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::decoder::{rust_yenc_crc, rust_yenc_decode_incremental, Decoder, Format, Status};
use crate::dupefallback::{
    article_fetch_add_decoded_bytes, article_fetch_add_raw_bytes,
    article_fetch_header_within_limit, article_size_steps_hash,
};
use crate::nntp::{ArticleBatchFetcher, BatchFetchedArticle, BatchRequest};
use crate::nzbfile::{Ctype, NzbFileParser};
use crate::twin::NzbEntry;

/// Global download limits and state, read before and during each fetch.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostStatus {
    pub max_norm_level: i32,
    pub quota_reached: bool,
    pub read_chunk: usize,
    pub server_count: usize,
    pub speed_limit: i64,
    /// Current and momentary speed; only filled in when `speed_limit > 0`.
    pub cur_speed: i64,
    pub mom_speed: i64,
}

/// A pooled connection. Outside this module it can only be made with the unsafe
/// [`ConnHandle::from_pooled`], so safe code cannot hand a host a made-up connection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConnHandle(usize);

impl ConnHandle {
    /// # Safety
    /// `conn` must be a connection the active server pool handed out and has not freed
    /// yet, and must stay so while the handle is in use.
    pub unsafe fn from_pooled(conn: *mut crate::net::PooledNntpConn) -> Self {
        Self(conn as usize)
    }

    fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// A news server, as named in [`NntpHost::get_connection`]'s exclusion list. Only a
/// host makes these, from servers it reported.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServerHandle(usize);

/// One configured news server.
#[derive(Clone, Debug, Default)]
pub struct ServerInfo {
    /// Identifies the server in [`NntpHost::get_connection`]'s exclusion list.
    pub handle: ServerHandle,
    pub active: bool,
    pub norm_level: i32,
    pub group: i32,
    pub id: i32,
    pub max_connections: i32,
    pub join_group: bool,
    pub retention: i32,
    pub name: Vec<u8>,
}

/// A pooled connection handed out by [`NntpHost::get_connection`].
#[derive(Clone, Debug, Default)]
pub struct ConnInfo {
    pub conn: ConnHandle,
    /// The connection was already open (it may have gone stale).
    pub reused: bool,
    pub server: ServerInfo,
}

/// The server pool, connections, speed meter and log an [`ArticleFetcher`]
/// works through. Connections and servers are passed around as opaque handles.
pub trait NntpHost: Send + Sync {
    fn status(&self) -> HostStatus;
    /// The server at `index` (`0..status().server_count`).
    fn server(&self, index: usize) -> Option<ServerInfo>;
    /// A free connection at `level`, skipping the servers in `excluded`.
    fn get_connection(&self, level: i32, excluded: &[ServerHandle]) -> Option<ConnInfo>;
    /// Books the connection's traffic and returns it to the pool, disconnecting
    /// it unless `keep_connected` (or when it was cancelled).
    fn release(&self, conn: ConnHandle, keep_connected: bool);
    /// The server `conn` belongs to.
    fn conn_server(&self, conn: ConnHandle) -> Option<ServerInfo>;
    /// Connects `conn` if it is not connected yet.
    fn connect(&self, conn: ConnHandle) -> bool;
    /// `GROUP` reply, or `None` when the connection failed.
    fn join_group(&self, conn: ConnHandle, group: &[u8]) -> Option<Vec<u8>>;
    /// Reply to a request line (`cmd` ends in CRLF), or `None` when the connection failed.
    fn request(&self, conn: ConnHandle, cmd: &[u8]) -> Option<Vec<u8>>;
    /// Reads the next chunk of the article body into `buf`, data the connection
    /// already buffered first. Returns its length; `None` when the connection is lost.
    fn read(&self, conn: ConnHandle, buf: &mut [u8]) -> Option<usize>;
    fn disconnect(&self, conn: ConnHandle);
    /// Stops a fetch in flight on `conn`, without logging its errors.
    fn cancel(&self, conn: ConnHandle);
    /// Read/write timeout for `conn`; `secs <= 0` restores the connection's own.
    fn set_io_timeout(&self, conn: ConnHandle, secs: i32);
    fn detail(&self, msg: &[u8]);
}

/// The daemon's [`NntpHost`]: the active server pool, stat meter, work state,
/// options and log.
pub struct NativeNntpHost;

impl NativeNntpHost {
    /// # Safety
    /// `conn` must be a live connection handle from [`NntpHost::get_connection`].
    unsafe fn conn<'a>(conn: ConnHandle) -> Option<&'a crate::net::PooledNntpConn> {
        unsafe { (conn.0 as *const crate::net::PooledNntpConn).as_ref() }
    }
}

fn server_info(s: &Arc<crate::net::RsNewsServer>) -> ServerInfo {
    ServerInfo {
        handle: ServerHandle(Arc::as_ptr(s) as usize),
        active: s.active.load(Ordering::SeqCst),
        norm_level: s.norm_level.load(Ordering::SeqCst),
        group: s.group,
        id: s.id,
        max_connections: s.max_connections,
        join_group: s.join_group,
        retention: s.retention,
        name: s.name.to_bytes().to_vec(),
    }
}

impl NntpHost for NativeNntpHost {
    fn status(&self) -> HostStatus {
        let pool = crate::net::active_server_pool();
        let ws = crate::ffi::active_workstate();
        let opts = crate::options::active_options();
        let sm = crate::statmeter::active_statmeter();
        let speed_limit = ws.as_ref().map_or(0, |w| i64::from(w.speed_limit()));
        let (cur_speed, mom_speed) = match sm.as_ref() {
            Some(s) if speed_limit > 0 => {
                (s.current_download_speed(), s.momentary_download_speed())
            }
            _ => (0, 0),
        };
        HostStatus {
            max_norm_level: pool.as_ref().map_or(0, |p| p.max_norm_level()),
            quota_reached: ws.as_ref().is_some_and(|w| w.quota_reached()),
            read_chunk: opts
                .as_ref()
                .map_or(4, |o| o.scalars().article_read_chunk_size)
                .max(1) as usize,
            server_count: pool.as_ref().map_or(0, |p| p.server_count()),
            speed_limit,
            cur_speed,
            mom_speed,
        }
    }

    fn server(&self, index: usize) -> Option<ServerInfo> {
        let pool = crate::net::active_server_pool()?;
        let servers = pool.servers_snapshot();
        servers.get(index).map(server_info)
    }

    fn get_connection(&self, level: i32, excluded: &[ServerHandle]) -> Option<ConnInfo> {
        let pool = crate::net::active_server_pool()?;
        // SAFETY: server handles come from `server`/`get_connection` and the
        // pool keeps its servers alive.
        let failed: Vec<&crate::net::RsNewsServer> = excluded
            .iter()
            .filter(|h| h.0 != 0)
            .map(|h| unsafe { &*(h.0 as *const crate::net::RsNewsServer) })
            .collect();
        let conn_ptr = pool.get_connection(level, None, &failed);
        // SAFETY: a non-null connection from the pool stays valid until freed.
        let conn = unsafe { conn_ptr.as_ref() }?;
        Some(ConnInfo {
            conn: ConnHandle(conn_ptr as usize),
            reused: conn.is_connected(),
            server: server_info(&conn.server),
        })
    }

    fn release(&self, conn: ConnHandle, keep_connected: bool) {
        let conn_ptr = conn.0 as *mut crate::net::PooledNntpConn;
        let Some(c) = (unsafe { Self::conn(conn) }) else {
            return;
        };
        let bytes_read = c.fetch_total_bytes_read();
        if bytes_read > 0 {
            if let Some(sm) = crate::statmeter::active_statmeter() {
                let off = crate::ffi::active_workstate().map_or(0, |w| w.local_time_offset());
                sm.add_server_stats(bytes_read, 0, 0, c.server.id, off);
            }
        }
        if !keep_connected || c.is_cancelled() {
            unsafe { c.disconnect() };
        }
        if let Some(pool) = crate::net::active_server_pool() {
            pool.free_connection(conn_ptr, true);
        }
    }

    fn conn_server(&self, conn: ConnHandle) -> Option<ServerInfo> {
        unsafe { Self::conn(conn) }.map(|c| server_info(&c.server))
    }

    fn connect(&self, conn: ConnHandle) -> bool {
        unsafe { Self::conn(conn) }.is_some_and(|c| unsafe { c.connect() })
    }

    fn join_group(&self, conn: ConnHandle, group: &[u8]) -> Option<Vec<u8>> {
        let c = unsafe { Self::conn(conn) }?;
        let group = cstr::to_cstring(group);
        let resp = unsafe { c.join_group(group.as_ptr()) };
        (!resp.is_null()).then(|| unsafe { cstr::owned(resp) })
    }

    fn request(&self, conn: ConnHandle, cmd: &[u8]) -> Option<Vec<u8>> {
        let c = unsafe { Self::conn(conn) }?;
        let cmd = cstr::to_cstring(cmd);
        let resp = unsafe { c.request(cmd.as_ptr()) };
        (!resp.is_null()).then(|| unsafe { cstr::owned(resp) })
    }

    fn read(&self, conn: ConnHandle, buf: &mut [u8]) -> Option<usize> {
        let c = unsafe { Self::conn(conn) }?;
        let mut len = unsafe { c.read_buffered(buf) };
        if len == 0 {
            let got = unsafe { c.try_recv(buf) };
            if got <= 0 {
                return None;
            }
            len = got as usize;
        }
        if let Some(sm) = crate::statmeter::active_statmeter() {
            sm.add_speed_reading_locked(len as c_int);
        }
        Some(len)
    }

    fn disconnect(&self, conn: ConnHandle) {
        if let Some(c) = unsafe { Self::conn(conn) } {
            unsafe { c.disconnect() };
        }
    }

    fn cancel(&self, conn: ConnHandle) {
        if let Some(c) = unsafe { Self::conn(conn) } {
            c.set_suppress_errors(true);
            c.cancel();
        }
    }

    fn set_io_timeout(&self, conn: ConnHandle, secs: i32) {
        if let Some(c) = unsafe { Self::conn(conn) } {
            c.set_io_timeout(secs);
        }
    }

    fn detail(&self, msg: &[u8]) {
        crate::log::log_active_bytes(crate::log::MK_DETAIL, msg);
    }
}

pub const MAX_TRANSIENT_ATTEMPTS: i32 = 4;

/// The longest a stream-repair fetch waits for the server to say anything. A borrow pass has
/// `DupeStreamTimeout` in all, so one article that never answers must not use it up at
/// `ArticleTimeout` per try: past this the try counts as timed out and goes to another connection.
pub const STREAM_REPLY_TIMEOUT_SECS: i32 = 60;

/// A read that failed after at least this long timed out rather than failed fast.
#[cfg(not(test))]
const TIMED_OUT_AFTER: Duration = Duration::from_secs(5);
#[cfg(test)]
const TIMED_OUT_AFTER: Duration = Duration::from_millis(50);

/// How many tries that time out one server gets for one article before the fetch moves on.
pub const MAX_TIMED_OUT_TRIES: i32 = 2;

/// Waiting longer than this for a free connection is logged.
const SLOW_CONNECTION_WAIT: Duration = Duration::from_secs(10);

/// Whether a read that failed after `elapsed` timed out. The bar sits below the reply timeout,
/// so a very short `ArticleTimeout` still counts as a timeout.
fn read_timed_out(elapsed: Duration) -> bool {
    let reply = Duration::from_secs(stream_reply_timeout_secs() as u64);
    elapsed >= TIMED_OUT_AFTER.min(reply * 4 / 5)
}

/// `STREAM_REPLY_TIMEOUT_SECS`, or `ArticleTimeout` when that is shorter.
fn stream_reply_timeout_secs() -> i32 {
    let article = crate::options::active_options().map_or(0, |o| o.scalars().article_timeout);
    if article > 0 {
        article.min(STREAM_REPLY_TIMEOUT_SECS)
    } else {
        STREAM_REPLY_TIMEOUT_SECS
    }
}

/// How many message-ids [`note_missing_everywhere`] keeps before it starts over.
const KNOWN_MISSING_CAP: usize = 100_000;

/// How long an article stays recorded as missing everywhere: a fresh post
/// can be missing on a server for a while until it propagates there.
const KNOWN_MISSING_TTL: Duration = Duration::from_secs(15 * 60);

type KnownMissing = std::collections::HashMap<Vec<u8>, Instant>;

fn known_missing() -> &'static Mutex<KnownMissing> {
    static SET: OnceLock<Mutex<KnownMissing>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(KnownMissing::new()))
}

fn bare_message_id(id: &[u8]) -> &[u8] {
    let id = id.strip_prefix(b"<").unwrap_or(id);
    id.strip_suffix(b">").unwrap_or(id)
}

fn note_missing_at(set: &mut KnownMissing, id: &[u8], now: Instant) {
    if set.len() >= KNOWN_MISSING_CAP {
        set.retain(|_, at| now.saturating_duration_since(*at) < KNOWN_MISSING_TTL);
        // Free a tenth at least, so a full set isn't rescanned on every insert.
        if set.len() >= KNOWN_MISSING_CAP - KNOWN_MISSING_CAP / 10 {
            set.clear();
        }
    }
    set.insert(id.to_vec(), now);
}

fn known_missing_at(set: &mut KnownMissing, id: &[u8], now: Instant) -> bool {
    match set.get(id) {
        Some(at) if now.saturating_duration_since(*at) < KNOWN_MISSING_TTL => true,
        Some(_) => {
            set.remove(id);
            false
        }
        None => false,
    }
}

/// Records an article that every server was asked for and answered `430 no
/// such article`: a background check (TwinCheck sampling) that would only
/// ask again need not hold a connection for it. Kept for
/// [`KNOWN_MISSING_TTL`], up to [`KNOWN_MISSING_CAP`] ids.
pub(crate) fn note_missing_everywhere(message_id: &[u8]) {
    let id = bare_message_id(message_id);
    if id.is_empty() {
        return;
    }
    let mut set = known_missing().lock().unwrap_or_else(|e| e.into_inner());
    note_missing_at(&mut set, id, Instant::now());
}

/// Forgets every recorded article, for when the servers change (a server
/// activated or the config reloaded) and a missing article may exist after all.
pub(crate) fn forget_missing_everywhere() {
    known_missing()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

/// Whether [`note_missing_everywhere`] recorded `message_id` (with or without
/// angle brackets) less than [`KNOWN_MISSING_TTL`] ago.
pub(crate) fn known_missing_everywhere(message_id: &[u8]) -> bool {
    let mut set = known_missing().lock().unwrap_or_else(|e| e.into_inner());
    known_missing_at(&mut set, bare_message_id(message_id), Instant::now())
}

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
    /// the server answered `430 no such article` (see `nntp::BackboneMiss`)
    pub not_found_430: bool,
    /// the connection was open but the server stopped answering (a read timed out)
    pub timed_out: bool,
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

pub struct ArticleFetcher {
    host: &'static dyn NntpHost,
    wait_counter: Mutex<Option<Arc<AtomicI32>>>,
    stopped: AtomicBool,
    conn_ptr: Mutex<ConnHandle>,
}

impl ArticleFetcher {
    pub fn new(host: &'static dyn NntpHost) -> Self {
        Self {
            host,
            wait_counter: Mutex::new(None),
            stopped: AtomicBool::new(false),
            conn_ptr: Mutex::new(ConnHandle::default()),
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
            if !conn.is_none() {
                self.host.cancel(conn);
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

    fn release_connection(&self, conn: ConnHandle, keep_connected: bool) {
        let mut guard = self.conn_ptr.lock().unwrap();
        self.host.release(conn, keep_connected);
        if *guard == conn {
            *guard = ConnHandle::default();
        }
    }

    pub fn fetch(&self, message_id: &[u8], groups: &[Vec<u8>]) -> FetchedArticle {
        let mut result = FetchedArticle::default();
        // servers this article failed on (only those: a login error or "too many
        // connections" says nothing about a server's backbone siblings) ...
        let mut failed_ptrs: Vec<ServerHandle> = Vec::new();
        // ... and backbones that answered 430 for it
        let mut missing: Vec<crate::nntp::BackboneMiss> = Vec::new();
        let mut transient_attempts: BTreeMap<ServerHandle, i32> = BTreeMap::new();
        let mut stale_connections: BTreeMap<ServerHandle, i32> = BTreeMap::new();
        let mut timed_out_tries: BTreeMap<ServerHandle, i32> = BTreeMap::new();
        // every server given up on answered 430
        let mut gave_up = 0u32;
        let mut only_430 = true;

        let mut level: i32 = 0;
        while !self.is_stopped() {
            let mut st = self.host.status();
            if level > st.max_norm_level {
                break;
            }
            while st.quota_reached && !self.is_stopped() {
                std::thread::sleep(Duration::from_millis(100));
                st = self.host.status();
            }
            if self.is_stopped() {
                break;
            }

            st = self.host.status();
            let mut eligible = 0i32;
            // the failed servers plus every server a backbone miss rules out, at any level
            let mut excluded_ptrs = failed_ptrs.clone();
            for idx in 0..st.server_count {
                let srv = self.host.server(idx).unwrap_or_default();
                let s_ptr = srv.handle;
                let ruled_out = crate::nntp::backbone_missing(&missing, srv.group, srv.retention);
                if ruled_out && !excluded_ptrs.contains(&s_ptr) {
                    excluded_ptrs.push(s_ptr);
                }
                if !srv.active && !ruled_out {
                    // an inactive server was never asked: the article may be there
                    only_430 = false;
                }
                if !srv.active || srv.norm_level != level {
                    continue;
                }
                if !ruled_out && !failed_ptrs.contains(&s_ptr) {
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

            let mut conn = ConnHandle::default();
            let mut reused = false;
            let mut server_id: i32 = 0;
            let mut max_connections: i32 = 0;
            let mut join_group = false;
            let mut server_ptr = ServerHandle::default();
            let mut server_group_of_conn: i32 = 0;
            let mut server_retention: i32 = 0;
            let mut server_name: Vec<u8> = Vec::new();

            let wait_started = Instant::now();
            while conn.is_none() && !self.is_stopped() {
                let cur_st = self.host.status();
                if cur_st.quota_reached {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                if let Some(got) = self.host.get_connection(level, &excluded_ptrs) {
                    conn = got.conn;
                    reused = got.reused;
                    server_id = got.server.id;
                    max_connections = got.server.max_connections;
                    join_group = got.server.join_group;
                    server_group_of_conn = got.server.group;
                    server_ptr = got.server.handle;
                    server_retention = got.server.retention;
                    server_name = got.server.name;
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            drop(wait_guard);

            if conn.is_none() {
                level += 1;
                continue;
            }
            let waited = wait_started.elapsed();
            if waited >= SLOW_CONNECTION_WAIT {
                let msg_end = message_id
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(message_id.len());
                let mut log = format!(
                    "Stream repair: waited {}s for a free connection to ",
                    waited.as_secs()
                )
                .into_bytes();
                log.extend_from_slice(&server_name);
                log.extend_from_slice(b" for ");
                log.extend_from_slice(&message_id[..msg_end]);
                self.host.detail(&log);
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
            let timed_out = result.timed_out;
            self.release_connection(conn, true);

            if result.success || !result.transient {
                set_server_unreachable(server_id, false);
            }
            if result.success {
                return result;
            }
            if result.retry {
                while self.host.status().quota_reached && !self.is_stopped() {
                    std::thread::sleep(Duration::from_millis(100));
                }
                continue;
            }
            if timed_out && !self.is_stopped() {
                // a server that sits on one article is not unreachable: retry once on
                // another connection (the timed-out one was disconnected), then move on
                let tries = timed_out_tries.entry(server_ptr).or_insert(0);
                *tries += 1;
                if *tries < MAX_TIMED_OUT_TRIES {
                    continue;
                }
            } else if result.transient && !server_unreachable(server_id) {
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

            if result.not_found_430 && server_group_of_conn > 0 {
                let miss = crate::nntp::BackboneMiss {
                    group: server_group_of_conn,
                    retention: server_retention,
                };
                if !missing.contains(&miss) {
                    missing.push(miss);
                }
            }
            gave_up += 1;
            only_430 &= result.not_found_430 && !result.transient && !result.timed_out;
            failed_ptrs.push(server_ptr);
        }

        if gave_up > 0 && only_430 && !self.is_stopped() {
            note_missing_everywhere(message_id);
        }
        result
    }

    pub fn fetch_from_connection(
        &self,
        conn: ConnHandle,
        message_id: &[u8],
        groups: &[Vec<u8>],
    ) -> FetchedArticle {
        self.fetch_from_connection_with_meta(conn, None, message_id, groups)
    }

    fn fetch_from_connection_with_meta(
        &self,
        conn: ConnHandle,
        known_meta: Option<(&[u8], bool)>,
        message_id: &[u8],
        groups: &[Vec<u8>],
    ) -> FetchedArticle {
        let mut result = FetchedArticle::default();
        if self.is_stopped() || conn.is_none() {
            return result;
        }

        let owned_name;
        let (server_name, join_group): (&[u8], bool) = match known_meta {
            Some((name, jg)) => (name, jg),
            None => {
                let srv = self.host.conn_server(conn).unwrap_or_default();
                owned_name = srv.name;
                (&owned_name, srv.join_group)
            }
        };
        let connected = self.host.connect(conn);

        let msg_clean_end = message_id
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(message_id.len());
        let msg_slice = &message_id[..msg_clean_end];

        if !connected {
            let mut log = b"Stream repair: could not connect to ".to_vec();
            log.extend_from_slice(server_name);
            self.host.detail(&log);
            result.transient = true;
            return result;
        }

        // the connection goes back to the pool with its own timeout
        struct IoTimeout(&'static dyn NntpHost, ConnHandle);
        impl Drop for IoTimeout {
            fn drop(&mut self) {
                self.0.set_io_timeout(self.1, 0);
            }
        }
        self.host.set_io_timeout(conn, stream_reply_timeout_secs());
        let _io_timeout = IoTimeout(self.host, conn);

        let disconnect = || self.host.disconnect(conn);

        if join_group {
            let join_started = Instant::now();
            let mut joined_resp: Option<Vec<u8>> = None;
            for group in groups {
                if let Some(resp) = self.host.join_group(conn, group) {
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
                self.host.detail(&log);
                result.transient = joined_resp.is_none();
                result.timed_out = result.transient && read_timed_out(join_started.elapsed());
                if result.transient {
                    disconnect();
                }
                return result;
            }
        }

        let mut body_cmd = Vec::with_capacity(msg_slice.len() + 8);
        body_cmd.extend_from_slice(b"BODY ");
        body_cmd.extend_from_slice(msg_slice);
        body_cmd.extend_from_slice(b"\r\n");
        let requested = Instant::now();
        let Some(resp) = self.host.request(conn, &body_cmd) else {
            let mut log = b"Stream repair: no response from ".to_vec();
            log.extend_from_slice(server_name);
            log.extend_from_slice(b" for ");
            log.extend_from_slice(msg_slice);
            log.extend_from_slice(format!(" after {}s", requested.elapsed().as_secs()).as_bytes());
            self.host.detail(&log);
            disconnect();
            result.transient = true;
            result.timed_out = read_timed_out(requested.elapsed());
            return result;
        };
        if resp.first() != Some(&b'2') {
            result.not_found_430 = resp.starts_with(b"430");
            let mut log = b"Stream repair: article ".to_vec();
            log.extend_from_slice(msg_slice);
            log.extend_from_slice(b" not available on ");
            log.extend_from_slice(server_name);
            self.host.detail(&log);
            return result;
        }

        let mut decoder = Decoder::new(rust_yenc_decode_incremental, rust_yenc_crc);
        decoder.clear();
        decoder.crc_check = true;
        decoder.raw_mode = false;

        let read_chunk = self.host.status().read_chunk;
        let mut recv_buf = vec![0u8; read_chunk + 128];
        let mut raw_bytes: i64 = 0;
        let mut decoded_bytes: i64 = 0;

        while !self.is_stopped() && !decoder.eof {
            let st0 = self.host.status();
            if st0.quota_reached {
                disconnect();
                result.retry = true;
                return result;
            }
            while !self.is_stopped() {
                let st_lim = self.host.status();
                if st_lim.quota_reached
                    || st_lim.speed_limit <= 0
                    || (st_lim.cur_speed <= st_lim.speed_limit
                        && st_lim.mom_speed <= st_lim.speed_limit)
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            if self.host.status().quota_reached {
                disconnect();
                result.retry = true;
                return result;
            }
            if self.is_stopped() {
                break;
            }

            let read_started = Instant::now();
            // A length past the buffer would make the decoder read out of bounds: treat it
            // like a lost connection.
            let read = self.host.read(conn, &mut recv_buf[..read_chunk]);
            let Some(len) = read.filter(|&n| n <= read_chunk) else {
                let mut log = b"Stream repair: connection to ".to_vec();
                log.extend_from_slice(server_name);
                log.extend_from_slice(b" lost while fetching ");
                log.extend_from_slice(msg_slice);
                self.host.detail(&log);
                disconnect();
                return FetchedArticle {
                    transient: true,
                    timed_out: read_timed_out(read_started.elapsed()),
                    ..Default::default()
                };
            };

            if !article_fetch_add_raw_bytes(&mut raw_bytes, len as i64) {
                let mut log = b"Stream repair: article ".to_vec();
                log.extend_from_slice(msg_slice);
                log.extend_from_slice(b" from ");
                log.extend_from_slice(server_name);
                log.extend_from_slice(b" exceeded the wire-size limit");
                self.host.detail(&log);
                disconnect();
                return result;
            }

            let buffer_ptr = recv_buf.as_mut_ptr();
            let decoded_len = unsafe { decoder.decode_buffer(buffer_ptr, len) };
            let declared_range_known =
                decoder.begin_pos > 0 && decoder.end_pos >= decoder.begin_pos && decoder.size > 0;
            if !article_fetch_header_within_limit(raw_bytes, declared_range_known) {
                let mut log = b"Stream repair: article ".to_vec();
                log.extend_from_slice(msg_slice);
                log.extend_from_slice(b" from ");
                log.extend_from_slice(server_name);
                log.extend_from_slice(b" exceeded the yEnc-header limit");
                self.host.detail(&log);
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
                self.host.detail(&log);
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
            self.host.detail(&log);
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
    host: &'static dyn NntpHost,
    state: Arc<Mutex<BatchPoolState>>,
    batch: ArticleBatchFetcher,
}

impl BatchFetcherPool {
    pub fn new(host: &'static dyn NntpHost) -> Self {
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
        char_signed: (c_char::MIN as i32) != 0,
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

    #[test]
    fn articles_missing_everywhere_are_remembered_with_or_without_brackets() {
        assert!(!known_missing_everywhere(b"<km-test-1@x>"));
        note_missing_everywhere(b"<km-test-1@x>");
        assert!(known_missing_everywhere(b"km-test-1@x"));
        assert!(known_missing_everywhere(b"<km-test-1@x>"));
        note_missing_everywhere(b"km-test-2@x");
        assert!(known_missing_everywhere(b"<km-test-2@x>"));
        assert!(!known_missing_everywhere(b"km-test-3@x"));
        note_missing_everywhere(b"<>");
        assert!(!known_missing_everywhere(b""));
    }

    #[test]
    fn articles_missing_everywhere_expire() {
        let mut set = KnownMissing::new();
        let t0 = Instant::now();
        note_missing_at(&mut set, b"a@x", t0);
        assert!(known_missing_at(
            &mut set,
            b"a@x",
            t0 + KNOWN_MISSING_TTL / 2
        ));
        assert!(!known_missing_at(&mut set, b"a@x", t0 + KNOWN_MISSING_TTL));
        assert!(set.is_empty());
    }

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
            format!(
                "\r\n=yend size={} part=1 pcrc32={crc:08x}\r\n.\r\n",
                payload.len()
            )
            .as_bytes(),
        );
        out
    }

    static TEST_WIRE_PAYLOAD: OnceLock<Vec<u8>> = OnceLock::new();
    static TEST_READ_POS: AtomicI32 = AtomicI32::new(0);

    fn mock_server() -> ServerInfo {
        ServerInfo {
            handle: ServerHandle(0x1000),
            active: true,
            id: 7,
            max_connections: 4,
            name: b"mock.nntp".to_vec(),
            ..Default::default()
        }
    }

    /// One server whose single article streams `TEST_WIRE_PAYLOAD`.
    struct MockHost;

    impl NntpHost for MockHost {
        fn status(&self) -> HostStatus {
            HostStatus {
                read_chunk: 64,
                server_count: 1,
                ..Default::default()
            }
        }
        fn server(&self, _index: usize) -> Option<ServerInfo> {
            Some(mock_server())
        }
        fn get_connection(&self, _level: i32, _excluded: &[ServerHandle]) -> Option<ConnInfo> {
            Some(ConnInfo {
                conn: ConnHandle(0x2000),
                reused: false,
                server: mock_server(),
            })
        }
        fn release(&self, _conn: ConnHandle, _keep_connected: bool) {}
        fn conn_server(&self, _conn: ConnHandle) -> Option<ServerInfo> {
            Some(mock_server())
        }
        fn connect(&self, _conn: ConnHandle) -> bool {
            true
        }
        fn join_group(&self, _conn: ConnHandle, _group: &[u8]) -> Option<Vec<u8>> {
            None
        }
        fn request(&self, _conn: ConnHandle, _cmd: &[u8]) -> Option<Vec<u8>> {
            Some(b"222 0 <seg1@test> body".to_vec())
        }
        fn read(&self, _conn: ConnHandle, buf: &mut [u8]) -> Option<usize> {
            let wire = TEST_WIRE_PAYLOAD.get().unwrap();
            let pos = TEST_READ_POS.load(Ordering::SeqCst) as usize;
            if pos >= wire.len() {
                return None;
            }
            let take = (wire.len() - pos).min(buf.len());
            buf[..take].copy_from_slice(&wire[pos..pos + take]);
            TEST_READ_POS.store((pos + take) as i32, Ordering::SeqCst);
            Some(take)
        }
        fn disconnect(&self, _conn: ConnHandle) {}
        fn cancel(&self, _conn: ConnHandle) {}
        fn set_io_timeout(&self, _conn: ConnHandle, _secs: i32) {}
        fn detail(&self, _msg: &[u8]) {}
    }

    #[test]
    fn article_fetcher_decodes_yenc_stream_and_reassembles_entry() {
        let expected = b"Hello from Rust ArticleFetcher yEnc test payload!";
        let _ = TEST_WIRE_PAYLOAD.set(yenc_encode_single_part(expected));
        TEST_READ_POS.store(0, Ordering::SeqCst);

        let fetcher = ArticleFetcher::new(&MockHost);
        let entry = NzbEntry {
            subject: "\"test.bin\" yEnc (1/1)".to_string(),
            filename: "test.bin".to_string(),
            groups: vec!["alt.binaries.test".to_string()],
            segments: vec![(expected.len() as u64, "<seg1@test>".to_string())],
        };
        let got = fetch_entry(&fetcher, &entry, 1024 * 1024, || false).expect("fetched entry");
        assert_eq!(got, expected);
    }

    static SILENT_REQUESTS: AtomicI32 = AtomicI32::new(0);
    static SILENT_TIMEOUTS: Mutex<Vec<c_int>> = Mutex::new(Vec::new());

    /// One server whose connections are reused and never answer `BODY`.
    struct SilentHost;

    impl NntpHost for SilentHost {
        fn status(&self) -> HostStatus {
            MockHost.status()
        }
        fn server(&self, _index: usize) -> Option<ServerInfo> {
            Some(ServerInfo {
                id: 9,
                ..mock_server()
            })
        }
        fn get_connection(&self, level: i32, excluded: &[ServerHandle]) -> Option<ConnInfo> {
            let mut got = MockHost.get_connection(level, excluded)?;
            got.reused = true;
            got.server.id = 9;
            Some(got)
        }
        fn release(&self, _conn: ConnHandle, _keep_connected: bool) {}
        fn conn_server(&self, conn: ConnHandle) -> Option<ServerInfo> {
            MockHost.conn_server(conn)
        }
        fn connect(&self, _conn: ConnHandle) -> bool {
            true
        }
        fn join_group(&self, _conn: ConnHandle, _group: &[u8]) -> Option<Vec<u8>> {
            None
        }
        fn request(&self, _conn: ConnHandle, _cmd: &[u8]) -> Option<Vec<u8>> {
            SILENT_REQUESTS.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(TIMED_OUT_AFTER + Duration::from_millis(30));
            None
        }
        fn read(&self, conn: ConnHandle, buf: &mut [u8]) -> Option<usize> {
            MockHost.read(conn, buf)
        }
        fn disconnect(&self, _conn: ConnHandle) {}
        fn cancel(&self, _conn: ConnHandle) {}
        fn set_io_timeout(&self, _conn: ConnHandle, secs: i32) {
            SILENT_TIMEOUTS.lock().unwrap().push(secs);
        }
        fn detail(&self, _msg: &[u8]) {}
    }

    #[test]
    fn an_article_that_never_answers_gets_two_tries_not_a_stale_retry_per_connection() {
        let fetcher = ArticleFetcher::new(&SilentHost);
        let started = Instant::now();
        let got = fetcher.fetch(b"<silent@test>", &[b"alt.binaries.test".to_vec()]);

        assert!(!got.success);
        assert!(got.timed_out);
        // one retry on another connection, then the server is out for this article: no
        // max_connections stale retries and no growing retry delays
        assert_eq!(SILENT_REQUESTS.load(Ordering::SeqCst), MAX_TIMED_OUT_TRIES);
        assert!(started.elapsed() < Duration::from_secs(1));
        // sitting on one article does not make the server unreachable for every other fetch
        assert!(!server_unreachable(9));
        // each try shortens the wait and gives the connection back with its own timeout
        let timeouts = SILENT_TIMEOUTS.lock().unwrap().clone();
        let short = stream_reply_timeout_secs();
        assert!(short > 0 && short <= STREAM_REPLY_TIMEOUT_SECS);
        assert_eq!(timeouts, vec![short, 0, short, 0]);
    }
}
