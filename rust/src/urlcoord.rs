//! End-to-end Rust implementation of `UrlCoordinator` and `UrlDownloader`
//! (`daemon/queue/UrlCoordinator.cpp`): X-DNZB header parsing (`dnzb_header`),
//! next-URL priority selection (`select_next_url`), hanging-download detection
//! (`is_hanging_url_download`), temporary output path formatting
//! (`format_temp_url_path`), and completion status transitions (`completed`).

/// What `UrlDownloader::ProcessHeader` takes from a header line.
#[derive(Debug, PartialEq, Eq)]
pub enum Dnzb {
    /// Not an X-DNZB header, or one without a colon.
    None,
    /// `X-DNZB-Category`'s value, trimmed of CR, LF, space and tab: its start
    /// and end in the line.
    Category(usize, usize),
    /// Another `X-DNZB-` header: its parameter name (`"*DNZB:"` and the
    /// header's name, cut to 99 bytes as `BString<100>` cut it) and its value
    /// (from after the colon's spaces, right-trimmed, Latin-1 to UTF-8).
    Param(Vec<u8>, Vec<u8>),
}

pub fn dnzb_header(line: &[u8]) -> Dnzb {
    if let Some(value) = line.strip_prefix(b"X-DNZB-Category:") {
        let (start, end) = crate::util::trim_line(value);
        let prefix = line.len() - value.len();
        return Dnzb::Category(prefix + start, prefix + end);
    }
    if !line.starts_with(b"X-DNZB-") {
        return Dnzb::None;
    }
    let Some(colon) = crate::bytes::find_byte(line, b':') else {
        return Dnzb::None;
    };
    let mut name = Vec::with_capacity(6 + colon - 7);
    name.extend_from_slice(b"*DNZB:");
    name.extend_from_slice(&line[7..colon]);
    name.truncate(99);
    let mut value = &line[colon + 1..];
    while let [b' ', rest @ ..] = value {
        value = rest;
    }
    let value = &value[..crate::util::trim_right_line(value)];
    let mut utf8 = Vec::new();
    crate::text::latin1_to_utf8(value, &mut utf8);
    Dnzb::Param(name, utf8)
}

/// `WebDownloader::EStatus` values `UrlCompleted` tests.
const AD_FINISHED: i32 = 2;
const AD_FAILED: i32 = 3;
const AD_RETRY: i32 = 4;
/// `NzbInfo::EUrlStatus` and `EDeleteStatus` values it sets.
const LS_NONE: i32 = 0;
const LS_FINISHED: i32 = 2;
const LS_FAILED: i32 = 3;
const DS_NONE: i32 = 0;
const DS_MANUAL: i32 = 1;

/// The statuses `UrlCoordinator::UrlCompleted` leaves on the NZB for the
/// downloader's status: `(url_status, delete_status, retry)`.
pub fn completed(
    downloader: i32,
    deleting: bool,
    url_status: i32,
    delete_status: i32,
) -> (i32, i32, bool) {
    let retry = downloader == AD_RETRY && !deleting;
    if deleting {
        return (
            LS_NONE,
            if delete_status == DS_NONE {
                DS_MANUAL
            } else {
                delete_status
            },
            retry,
        );
    }
    let url_status = match downloader {
        AD_FINISHED => LS_FINISHED,
        AD_FAILED => LS_FAILED,
        AD_RETRY => LS_NONE,
        _ => url_status,
    };
    (url_status, delete_status, retry)
}

/// Selects the next URL index to download in `UrlCoordinator::GetNextUrl`.
/// Returns `-1` if no eligible URL entry exists.
pub fn select_next_url(
    kinds: &[i32],
    url_statuses: &[i32],
    delete_statuses: &[i32],
    priorities: &[i32],
    nk_url: i32,
    ls_none: i32,
    ds_none: i32,
) -> i32 {
    let count = kinds
        .len()
        .min(url_statuses.len())
        .min(delete_statuses.len())
        .min(priorities.len());
    let mut best_idx: i32 = -1;
    let mut best_prio: i32 = i32::MIN;
    for i in 0..count {
        if kinds[i] == nk_url
            && url_statuses[i] == ls_none
            && delete_statuses[i] == ds_none
            && (best_idx < 0 || priorities[i] > best_prio)
        {
            best_idx = i as i32;
            best_prio = priorities[i];
        }
    }
    best_idx
}

/// Checks if a URL download is hanging in `UrlCoordinator::ResetHangingDownloads`.
pub fn is_hanging_url_download(
    now: i64,
    last_update_time: i64,
    url_timeout: i32,
    status: i32,
    ad_running: i32,
) -> bool {
    url_timeout > 0 && now - last_update_time > i64::from(url_timeout) + 10 && status == ad_running
}

/// Formats the temporary URL download path (`"%s%curl-%i.tmp"`) for `UrlCoordinator::StartUrlDownload`.
pub fn format_temp_url_path(temp_dir: &[u8], sep: u8, id: i32) -> Vec<u8> {
    let suffix = format!("url-{id}.tmp");
    let mut out = Vec::with_capacity(temp_dir.len() + 1 + suffix.len());
    out.extend_from_slice(temp_dir);
    out.push(sep);
    out.extend_from_slice(suffix.as_bytes());
    out
}

// ============================================================================
// End-to-End UrlCoordinator & UrlDownloader Engine (`NzbgetRsUrlCoord`)
// ============================================================================

use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

const AD_RUNNING: i32 = 1;
const LS_RUNNING: i32 = 1;
const LS_SCAN_SKIPPED: i32 = 5;
const LS_SCAN_FAILED: i32 = 6;
const NK_URL: i32 = 1;
const AS_SUCCESS: i32 = 1;
const AS_FAILED: i32 = 2;
const EA_URL_FOUND: i32 = 7;
const EA_URL_ADDED: i32 = 8;
const EA_URL_DELETED: i32 = 9;
const EA_URL_COMPLETED: i32 = 10;
const EA_URL_FAILED: i32 = 11;
const EA_URL_RETURNED: i32 = 12;

struct ActiveUrlSlot {
    #[cfg(unix)]
    dl: Arc<crate::net::NzbgetRsWebDl>,
    #[cfg(not(unix))]
    dl_ptr: usize,
    nzb_ptr: usize,
    category: Vec<u8>,
}

/// Stateful Rust engine backing `UrlCoordinator` and `UrlDownloader`.
pub struct NzbgetRsUrlCoord {
    active: Mutex<Vec<ActiveUrlSlot>>,
    has_more_jobs: AtomicBool,
    wait_mu: Mutex<()>,
    wait_cv: Condvar,
    host_env: Mutex<Option<NzbgetRsUrlCoordEnv>>,
    self_weak: Mutex<std::sync::Weak<NzbgetRsUrlCoord>>,
}

impl Default for NzbgetRsUrlCoord {
    fn default() -> Self {
        Self::new()
    }
}

/// Host C++ callback environment for `nzbget_rs_url_coord_exec`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NzbgetRsUrlCoordEnv {
    pub ctx: *mut c_void,
    pub p1: usize,
    pub p2: usize,
    pub a1: c_int,
    pub a2: c_int,
    pub s1: *const c_char,
    pub with_queue_guard_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            Option<unsafe extern "C" fn(*mut c_void, usize)>,
        ),
    >,
    pub log_cb: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char)>,
    pub query_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            c_int,
            usize,
            *mut c_int,
            *mut i64,
            *mut *const c_char,
        ) -> usize,
    >,
    pub action_cb: Option<
        unsafe extern "C" fn(
            *mut c_void,
            c_int,
            usize,
            usize,
            c_int,
            c_int,
            *const c_char,
            *const c_char,
            *const c_char,
        ) -> usize,
    >,
}

unsafe impl Send for NzbgetRsUrlCoordEnv {}
unsafe impl Sync for NzbgetRsUrlCoordEnv {}

fn url_bytes_to_cstring(raw: &[u8]) -> std::ffi::CString {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    unsafe { std::ffi::CString::from_vec_unchecked(raw[..end].to_vec()) }
}

fn url_cstr_bytes<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        b""
    } else {
        unsafe { CStr::from_ptr(p) }.to_bytes()
    }
}

fn url_now_sec() -> i64 {
    #[cfg(unix)]
    unsafe {
        libc::time(std::ptr::null_mut()) as i64
    }
    #[cfg(not(unix))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

fn url_emit_log(env: &NzbgetRsUrlCoordEnv, level: i32, parts: &[&[u8]]) {
    if let Some(cb) = env.log_cb {
        let joined: Vec<u8> = parts.concat();
        let cstr = url_bytes_to_cstring(&joined);
        unsafe { cb(env.ctx, level as c_int, cstr.as_ptr()) };
        return;
    }
    #[cfg(unix)]
    {
        if level == 0 && !crate::log::active_debug_build() {
            return;
        }
        let kind = match level {
            0 => 3,
            1 => 4,
            2 => 0,
            3 => 1,
            _ => 2,
        };
        let joined: Vec<u8> = parts.concat();
        crate::log::log_active_bytes(kind, &joined);
    }
}

fn url_runtime_opts(env: &NzbgetRsUrlCoordEnv) -> (i32, bool, bool, i32, u8, Vec<u8>) {
    if let Some(opts) = crate::options::active_options() {
        let sc = opts.scalars();
        let pause_download = crate::ffi::active_workstate().is_some_and(|w| w.pause_download());
        return (
            sc.url_connections,
            pause_download,
            sc.url_force != 0,
            sc.url_timeout,
            crate::paths::PATH_SEPARATOR as u8,
            opts.str_bytes(5).to_vec(),
        );
    }
    let mut opts_ints = [0 as c_int; 5];
    let mut opts_strs = [std::ptr::null::<c_char>(); 4];
    if let Some(q) = env.query_cb {
        unsafe {
            q(
                env.ctx,
                2,
                0,
                opts_ints.as_mut_ptr(),
                std::ptr::null_mut(),
                opts_strs.as_mut_ptr(),
            );
        }
    }
    (
        opts_ints[0],
        opts_ints[1] != 0,
        opts_ints[2] != 0,
        opts_ints[3],
        opts_ints[4] as u8,
        url_cstr_bytes(opts_strs[0]).to_vec(),
    )
}

fn with_dq_guard<F: FnOnce(usize)>(env: &NzbgetRsUrlCoordEnv, f: F) {
    struct GuardTrampoline<F> {
        f: Option<F>,
    }
    unsafe extern "C" fn tramp<F: FnOnce(usize)>(raw: *mut c_void, dq_ptr: usize) {
        let state = unsafe { &mut *raw.cast::<GuardTrampoline<F>>() };
        if let Some(f) = state.f.take() {
            f(dq_ptr);
        }
    }
    let Some(cb) = env.with_queue_guard_cb else {
        return;
    };
    let mut state = GuardTrampoline { f: Some(f) };
    unsafe {
        cb(
            env.ctx,
            (&mut state as *mut GuardTrampoline<F>).cast(),
            Some(tramp::<F>),
        );
    }
}

impl NzbgetRsUrlCoord {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(Vec::new()),
            has_more_jobs: AtomicBool::new(true),
            wait_mu: Mutex::new(()),
            wait_cv: Condvar::new(),
            host_env: Mutex::new(None),
            self_weak: Mutex::new(std::sync::Weak::new()),
        }
    }

    fn effective_env(&self, caller: Option<&NzbgetRsUrlCoordEnv>) -> NzbgetRsUrlCoordEnv {
        let mut base = self
            .host_env
            .lock()
            .ok()
            .and_then(|g| *g)
            .unwrap_or_else(|| unsafe { std::mem::zeroed() });
        if let Some(c) = caller {
            base.p1 = c.p1;
            base.p2 = c.p2;
            base.a1 = c.a1;
            base.a2 = c.a2;
            base.s1 = c.s1;
            if c.with_queue_guard_cb.is_some() {
                base.with_queue_guard_cb = c.with_queue_guard_cb;
            }
            if c.log_cb.is_some() {
                base.log_cb = c.log_cb;
            }
            if c.query_cb.is_some() {
                base.query_cb = c.query_cb;
                if !c.ctx.is_null() {
                    base.ctx = c.ctx;
                }
            }
            if c.action_cb.is_some() {
                base.action_cb = c.action_cb;
                if !c.ctx.is_null() {
                    base.ctx = c.ctx;
                }
            }
        }
        base
    }

    fn is_stopped(&self, env: &NzbgetRsUrlCoordEnv) -> bool {
        let Some(q) = env.query_cb else {
            return true;
        };
        unsafe {
            q(
                env.ctx,
                1,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) != 0
        }
    }

    fn reset_hanging_downloads(&self, env: &NzbgetRsUrlCoordEnv) {
        let (_, _, _, url_timeout, _, _) = url_runtime_opts(env);
        if url_timeout == 0 {
            return;
        }
        with_dq_guard(env, |_dq_ptr| {
            let tm = url_now_sec();
            #[cfg(unix)]
            {
                let dls: Vec<Arc<crate::net::NzbgetRsWebDl>> = self
                    .active
                    .lock()
                    .map(|g| g.iter().map(|s| Arc::clone(&s.dl)).collect())
                    .unwrap_or_default();
                for dl in dls {
                    if is_hanging_url_download(
                        tm,
                        dl.last_update_time(),
                        url_timeout,
                        dl.status_code(),
                        AD_RUNNING,
                    ) {
                        let info_name = dl.info_name_bytes();
                        url_emit_log(env, 4, &[b"Cancelling hanging url download ", &info_name]);
                        dl.stop();
                    }
                }
            }
            #[cfg(not(unix))]
            let _ = tm;
        });
    }

    fn wait_jobs(&self, env: &NzbgetRsUrlCoordEnv) {
        url_emit_log(
            env,
            0,
            &[b"UrlCoordinator: waiting for Downloads to complete"],
        );
        loop {
            let mut is_empty = true;
            with_dq_guard(env, |_dq_ptr| {
                is_empty = self.active.lock().map(|g| g.is_empty()).unwrap_or(true);
            });
            if is_empty {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            self.reset_hanging_downloads(env);
        }
        url_emit_log(env, 0, &[b"UrlCoordinator: Downloads are completed"]);
    }

    fn run_loop(&self, env: &NzbgetRsUrlCoordEnv) {
        let (Some(q), Some(act)) = (env.query_cb, env.action_cb) else {
            return;
        };
        url_emit_log(env, 0, &[b"Entering UrlCoordinator-loop"]);

        while !crate::ffi::is_queue_loaded() {
            if unsafe {
                q(
                    env.ctx,
                    0,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            } != 0
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        let mut last_reset: i64 = 0;
        while !self.is_stopped(env) {
            let mut download_started = false;

            with_dq_guard(env, |dq_ptr| {
                let (url_connections, pause_download, url_force, _, sep, temp_dir) =
                    url_runtime_opts(env);

                let active_len = self.active.lock().map(|g| g.len() as i32).unwrap_or(0);
                let mut chosen_nzb: usize = 0;
                let mut chosen_id: i32 = 0;

                if active_len < url_connections {
                    let count = unsafe {
                        q(
                            env.ctx,
                            3,
                            dq_ptr,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        )
                    };
                    if count > 0 {
                        let mut kinds = Vec::with_capacity(count);
                        let mut url_st = Vec::with_capacity(count);
                        let mut del_st = Vec::with_capacity(count);
                        let mut prios = Vec::with_capacity(count);
                        let mut ptrs = Vec::with_capacity(count);
                        let mut ids = Vec::with_capacity(count);
                        for idx in 0..count {
                            let mut item_ints = [idx as c_int, 0, 0, 0, 0];
                            let nzb_ptr = unsafe {
                                q(
                                    env.ctx,
                                    4,
                                    dq_ptr,
                                    item_ints.as_mut_ptr(),
                                    std::ptr::null_mut(),
                                    std::ptr::null_mut(),
                                )
                            };
                            kinds.push(item_ints[0]);
                            url_st.push(item_ints[1]);
                            del_st.push(item_ints[2]);
                            prios.push(item_ints[3]);
                            ids.push(item_ints[4]);
                            ptrs.push(nzb_ptr);
                        }
                        let sel = select_next_url(
                            &kinds, &url_st, &del_st, &prios, NK_URL, LS_NONE, DS_NONE,
                        );
                        if sel >= 0 && (sel as usize) < count {
                            chosen_nzb = ptrs[sel as usize];
                            chosen_id = ids[sel as usize];
                        }
                    }

                    if chosen_nzb != 0 && (!pause_download || url_force) {
                        url_emit_log(env, 0, &[b"Starting new UrlDownloader"]);
                        let tmp_path = format_temp_url_path(&temp_dir, sep, chosen_id);
                        let mut nzb_ints = [0 as c_int; 4];
                        let mut nzb_strs = [std::ptr::null::<c_char>(); 4];
                        unsafe {
                            q(
                                env.ctx,
                                6,
                                chosen_nzb,
                                nzb_ints.as_mut_ptr(),
                                std::ptr::null_mut(),
                                nzb_strs.as_mut_ptr(),
                            );
                        }
                        let url_bytes = url_cstr_bytes(nzb_strs[0]).to_vec();
                        let fn_bytes = url_cstr_bytes(nzb_strs[1]).to_vec();
                        let url_c = url_bytes_to_cstring(&url_bytes);
                        let info_name =
                            crate::downloadinfo::make_nice_url_name(Some(&url_c), &fn_bytes);
                        unsafe {
                            act(
                                env.ctx,
                                3,
                                chosen_nzb,
                                0,
                                0,
                                1,
                                std::ptr::null(),
                                std::ptr::null(),
                                std::ptr::null(),
                            );
                            act(
                                env.ctx,
                                3,
                                chosen_nzb,
                                0,
                                1,
                                LS_RUNNING,
                                std::ptr::null(),
                                std::ptr::null(),
                                std::ptr::null(),
                            );
                        }
                        #[cfg(unix)]
                        {
                            let weak_s =
                                self.self_weak.lock().map(|g| g.clone()).unwrap_or_default();
                            let weak_h = weak_s.clone();
                            let dl = Arc::new(crate::net::NzbgetRsWebDl::new_configured(
                                &url_bytes,
                                &info_name,
                                &tmp_path,
                                url_force,
                                true,
                                2,
                                Some(Box::new(move |dl_ref, st_enum| {
                                    if let Some(uc) = weak_s.upgrade() {
                                        uc.on_webdl_status(dl_ref, chosen_nzb, st_enum as i32);
                                    }
                                })),
                                Some(Box::new(move |dl_ref, hdr_line| {
                                    if let Some(uc) = weak_h.upgrade() {
                                        uc.on_webdl_header(dl_ref, chosen_nzb, hdr_line);
                                    }
                                })),
                            ));
                            if let Ok(mut g) = self.active.lock() {
                                g.push(ActiveUrlSlot {
                                    dl: Arc::clone(&dl),
                                    nzb_ptr: chosen_nzb,
                                    category: Vec::new(),
                                });
                            }
                            crate::net::NzbgetRsWebDl::spawn(dl);
                            download_started = true;
                        }
                        #[cfg(not(unix))]
                        let _ = (info_name, tmp_path);
                    }
                }

                let active_nonempty = self.active.lock().map(|g| !g.is_empty()).unwrap_or(false);
                self.has_more_jobs
                    .store(active_nonempty || chosen_nzb != 0, Ordering::SeqCst);
            });

            let now = url_now_sec();
            if last_reset != now {
                self.reset_hanging_downloads(env);
                last_reset = url_now_sec();
            }

            if !self.has_more_jobs.load(Ordering::SeqCst) && !self.is_stopped(env) {
                if let Ok(mut lk) = self.wait_mu.lock() {
                    while !self.has_more_jobs.load(Ordering::SeqCst) && !self.is_stopped(env) {
                        lk = match self.wait_cv.wait(lk) {
                            Ok(g) => g,
                            Err(p) => p.into_inner(),
                        };
                    }
                }
            } else if download_started {
                std::thread::yield_now();
            } else {
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        self.wait_jobs(env);
        url_emit_log(env, 0, &[b"Exiting UrlCoordinator-loop"]);
    }

    fn stop_downloads(&self, env: &NzbgetRsUrlCoordEnv) {
        url_emit_log(env, 0, &[b"Stopping UrlDownloads"]);
        with_dq_guard(env, |_dq_ptr| {
            #[cfg(unix)]
            {
                let dls: Vec<Arc<crate::net::NzbgetRsWebDl>> = self
                    .active
                    .lock()
                    .map(|g| g.iter().map(|s| Arc::clone(&s.dl)).collect())
                    .unwrap_or_default();
                for dl in dls {
                    dl.stop();
                }
            }
        });
        url_emit_log(env, 0, &[b"UrlDownloads are notified"]);
        let _lk = self.wait_mu.lock();
        self.wait_cv.notify_all();
    }

    fn on_queue_update(&self, env: &NzbgetRsUrlCoordEnv) {
        url_emit_log(env, 0, &[b"Notification from download queue received"]);
        if env.a1 == EA_URL_ADDED || env.a1 == EA_URL_RETURNED {
            let _lk = self.wait_mu.lock();
            self.has_more_jobs.store(true, Ordering::SeqCst);
            self.wait_cv.notify_all();
        }
    }

    #[cfg(unix)]
    fn on_webdl_header(&self, dl: &crate::net::NzbgetRsWebDl, nzb_ptr: usize, line: &[u8]) {
        let env = self.effective_env(None);
        let dl_key = dl as *const crate::net::NzbgetRsWebDl;
        match dnzb_header(line) {
            Dnzb::None => {}
            Dnzb::Category(from, to) => {
                let cat = line[from..to].to_vec();
                if let Ok(mut g) = self.active.lock() {
                    if let Some(slot) = g.iter_mut().find(|s| Arc::as_ptr(&s.dl) == dl_key) {
                        slot.category = cat.clone();
                    }
                }
                url_emit_log(&env, 0, &[b"Category: ", &cat]);
            }
            Dnzb::Param(name, val) => {
                url_emit_log(&env, 0, &[b"X-DNZB: ", &name]);
                url_emit_log(&env, 0, &[b"Value: ", &val]);
                if let Some(act) = env.action_cb {
                    let c_name = url_bytes_to_cstring(&name);
                    let c_val = url_bytes_to_cstring(&val);
                    unsafe {
                        act(
                            env.ctx,
                            2,
                            nzb_ptr,
                            0,
                            0,
                            0,
                            c_name.as_ptr(),
                            c_val.as_ptr(),
                            std::ptr::null(),
                        );
                    }
                }
            }
        }
    }

    #[cfg(unix)]
    fn on_webdl_status(&self, dl: &crate::net::NzbgetRsWebDl, nzb_ptr: usize, status: i32) {
        let env = self.effective_env(None);
        url_emit_log(&env, 0, &[b"Notification from UrlDownloader received"]);
        let (Some(q), Some(act)) = (env.query_cb, env.action_cb) else {
            return;
        };
        if status != AD_FINISHED && status != AD_FAILED && status != AD_RETRY {
            return;
        }

        url_emit_log(&env, 0, &[b"URL downloaded"]);

        let orig_fn_opt = dl.original_filename_bytes();
        let out_fn_bytes = dl.output_filename_bytes();
        let dl_key = dl as *const crate::net::NzbgetRsWebDl;

        let mut nzb_ints = [0 as c_int; 4];
        let mut nzb_strs = [std::ptr::null::<c_char>(); 4];
        unsafe {
            q(
                env.ctx,
                6,
                nzb_ptr,
                nzb_ints.as_mut_ptr(),
                std::ptr::null_mut(),
                nzb_strs.as_mut_ptr(),
            );
        }
        let nzb_url = url_cstr_bytes(nzb_strs[0]).to_vec();
        let origname: Vec<u8> =
            orig_fn_opt.unwrap_or_else(|| crate::nserv::base_file_name(&nzb_url).to_vec());
        let valid_filename = crate::paths::make_valid_filename(&origname, false);
        url_emit_log(&env, 0, &[b"Filename: [", &valid_filename, b"]"]);

        let mut retry = false;
        let mut dnzb_cat = Vec::new();
        let mut final_url_status = LS_NONE;

        with_dq_guard(&env, |dq_ptr| {
            if let Ok(mut g) = self.active.lock() {
                if let Some(pos) = g.iter().position(|s| Arc::as_ptr(&s.dl) == dl_key) {
                    let removed = g.remove(pos);
                    dnzb_cat = removed.category;
                }
            }

            let mut cur_ints = [0 as c_int; 4];
            let mut cur_strs = [std::ptr::null::<c_char>(); 4];
            unsafe {
                q(
                    env.ctx,
                    6,
                    nzb_ptr,
                    cur_ints.as_mut_ptr(),
                    std::ptr::null_mut(),
                    cur_strs.as_mut_ptr(),
                );
            }
            let cur_url = cur_ints[0];
            let cur_del = cur_ints[1];
            let deleting = cur_ints[2] != 0;

            let (new_url, new_del, r) = completed(status, deleting, cur_url, cur_del);
            retry = r;
            final_url_status = new_url;

            unsafe {
                act(
                    env.ctx,
                    3,
                    nzb_ptr,
                    0,
                    2,
                    new_del,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                );
                if deleting {
                    act(
                        env.ctx,
                        3,
                        nzb_ptr,
                        0,
                        3,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                    );
                }
                act(
                    env.ctx,
                    3,
                    nzb_ptr,
                    0,
                    1,
                    new_url,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                );
                if !retry {
                    let completed_url = url_cstr_bytes(cur_strs[0]).to_vec();
                    act(
                        env.ctx,
                        4,
                        dq_ptr,
                        nzb_ptr,
                        EA_URL_COMPLETED,
                        0,
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                    );
                    #[cfg(unix)]
                    crate::feed::notify_active_feed_queue_aspect(
                        EA_URL_COMPLETED,
                        Some(&completed_url),
                    );
                }
            }
        });

        if retry {
            unsafe {
                act(
                    env.ctx,
                    3,
                    nzb_ptr,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                );
            }
            return;
        }

        if final_url_status == LS_FINISHED {
            let mut cur_ints = [0 as c_int; 4];
            let mut cur_strs = [std::ptr::null::<c_char>(); 4];
            unsafe {
                q(
                    env.ctx,
                    6,
                    nzb_ptr,
                    cur_ints.as_mut_ptr(),
                    std::ptr::null_mut(),
                    cur_strs.as_mut_ptr(),
                );
            }
            let nzb_fn = url_cstr_bytes(cur_strs[1]);
            let nzb_cat = url_cstr_bytes(cur_strs[2]);
            let eff_name = if !nzb_fn.is_empty() {
                nzb_fn.to_vec()
            } else {
                valid_filename
            };
            let eff_cat = if !nzb_cat.is_empty() {
                nzb_cat.to_vec()
            } else {
                dnzb_cat
            };

            let c_name = url_bytes_to_cstring(&eff_name);
            let c_cat = url_bytes_to_cstring(&eff_cat);
            let c_out = url_bytes_to_cstring(&out_fn_bytes);
            let is_archive = if c_name.as_bytes().is_empty() {
                false
            } else {
                use std::os::unix::ffi::OsStrExt;
                crate::unpack::can_extract_with_zipfast(std::path::Path::new(
                    std::ffi::OsStr::from_bytes(c_name.as_bytes()),
                ))
            };

            let add_status = unsafe {
                act(
                    env.ctx,
                    5,
                    nzb_ptr,
                    0,
                    c_int::from(is_archive),
                    0,
                    c_name.as_ptr(),
                    c_cat.as_ptr(),
                    c_out.as_ptr(),
                )
            } as i32;

            if add_status == AS_SUCCESS {
                return;
            }

            let scan_st = if add_status == AS_FAILED {
                LS_SCAN_FAILED
            } else {
                LS_SCAN_SKIPPED
            };
            unsafe {
                act(
                    env.ctx,
                    3,
                    nzb_ptr,
                    0,
                    1,
                    scan_st,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                );
            }
        }

        unsafe {
            act(
                env.ctx,
                6,
                nzb_ptr,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            );
        }

        with_dq_guard(&env, |dq_ptr| unsafe {
            act(
                env.ctx,
                3,
                nzb_ptr,
                0,
                0,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            );
            act(
                env.ctx,
                4,
                dq_ptr,
                nzb_ptr,
                EA_URL_FAILED,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            );
            #[cfg(unix)]
            crate::feed::notify_active_feed_queue_aspect(EA_URL_FAILED, None);
        });
    }

    fn delete_queue_entry(&self, env: &NzbgetRsUrlCoordEnv) -> c_int {
        let (Some(q), Some(act)) = (env.query_cb, env.action_cb) else {
            return 0;
        };
        let dq_ptr = env.p1;
        let nzb_ptr = env.p2;
        let avoid_history = env.a1;

        let mut nzb_ints = [0 as c_int; 4];
        let mut nzb_strs = [std::ptr::null::<c_char>(); 4];
        unsafe {
            q(
                env.ctx,
                6,
                nzb_ptr,
                nzb_ints.as_mut_ptr(),
                std::ptr::null_mut(),
                nzb_strs.as_mut_ptr(),
            );
        }
        let nzb_name = url_cstr_bytes(nzb_strs[3]).to_vec();

        if nzb_ints[3] > 0 {
            url_emit_log(env, 2, &[b"Deleting active URL ", &nzb_name]);
            unsafe {
                act(
                    env.ctx,
                    3,
                    nzb_ptr,
                    0,
                    3,
                    1,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                );
                act(
                    env.ctx,
                    3,
                    nzb_ptr,
                    0,
                    4,
                    avoid_history,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                );
            }
            #[cfg(unix)]
            {
                let found_dl = self.active.lock().ok().and_then(|g| {
                    g.iter()
                        .find(|s| s.nzb_ptr == nzb_ptr)
                        .map(|s| Arc::clone(&s.dl))
                });
                if let Some(dl) = found_dl {
                    dl.stop();
                    return 1;
                }
            }
            return 0;
        }

        url_emit_log(env, 2, &[b"Deleting URL ", &nzb_name]);
        unsafe {
            if nzb_ints[1] == DS_NONE {
                act(
                    env.ctx,
                    3,
                    nzb_ptr,
                    0,
                    2,
                    DS_MANUAL,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                );
            }
            act(
                env.ctx,
                3,
                nzb_ptr,
                0,
                1,
                LS_NONE,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            );
            act(
                env.ctx,
                4,
                dq_ptr,
                nzb_ptr,
                EA_URL_DELETED,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            );
            #[cfg(unix)]
            crate::feed::notify_active_feed_queue_aspect(EA_URL_DELETED, None);
        }
        1
    }

    fn add_url_to_queue(&self, env: &NzbgetRsUrlCoordEnv) {
        let (Some(q), Some(act)) = (env.query_cb, env.action_cb) else {
            return;
        };
        url_emit_log(env, 0, &[b"Adding URL to queue"]);
        let nzb_ptr = env.p1;
        let holder_ptr = env.p2;
        let add_first = env.a1;

        with_dq_guard(env, |dq_ptr| unsafe {
            act(
                env.ctx,
                4,
                dq_ptr,
                nzb_ptr,
                EA_URL_FOUND,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            );
            #[cfg(unix)]
            crate::feed::notify_active_feed_queue_aspect(EA_URL_FOUND, None);
            let mut nzb_ints = [0 as c_int; 4];
            let mut nzb_strs = [std::ptr::null::<c_char>(); 4];
            q(
                env.ctx,
                6,
                nzb_ptr,
                nzb_ints.as_mut_ptr(),
                std::ptr::null_mut(),
                nzb_strs.as_mut_ptr(),
            );
            let do_add = c_int::from(nzb_ints[1] != DS_MANUAL);
            act(
                env.ctx,
                7,
                dq_ptr,
                holder_ptr,
                do_add,
                add_first,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            );
            if do_add != 0 {
                act(
                    env.ctx,
                    4,
                    dq_ptr,
                    nzb_ptr,
                    EA_URL_ADDED,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                );
                #[cfg(unix)]
                crate::feed::notify_active_feed_queue_aspect(EA_URL_ADDED, None);
            }
            act(
                env.ctx,
                7,
                dq_ptr,
                0,
                2,
                0,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            );
        });
    }

    fn log_debug_info(&self, env: &NzbgetRsUrlCoordEnv) {
        url_emit_log(env, 2, &[b"   ---------- UrlCoordinator"]);
        with_dq_guard(env, |_dq_ptr| {
            #[cfg(unix)]
            {
                let dls: Vec<Arc<crate::net::NzbgetRsWebDl>> = self
                    .active
                    .lock()
                    .map(|g| g.iter().map(|s| Arc::clone(&s.dl)).collect())
                    .unwrap_or_default();
                url_emit_log(
                    env,
                    2,
                    &[format!("    Active Downloads: {}", dls.len()).as_bytes()],
                );
                for dl in dls {
                    dl.log_debug_info();
                }
            }
        });
    }

    fn destroy_remaining(&self, env: &NzbgetRsUrlCoordEnv) {
        url_emit_log(env, 0, &[b"Destroying UrlCoordinator"]);
        if let Ok(mut g) = self.active.lock() {
            g.clear();
        }
        url_emit_log(env, 0, &[b"UrlCoordinator destroyed"]);
    }
}

static ACTIVE_URL_COORD: std::sync::RwLock<Option<std::sync::Weak<NzbgetRsUrlCoord>>> =
    std::sync::RwLock::new(None);

pub(crate) fn active_url_coord() -> Option<Arc<NzbgetRsUrlCoord>> {
    ACTIVE_URL_COORD
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
}

impl NzbgetRsUrlCoord {
    pub(crate) fn delete_queue_entry_active(
        &self,
        dq_ptr: usize,
        nzb_ptr: usize,
        avoid_history: bool,
    ) -> bool {
        let mut env = self.effective_env(None);
        env.p1 = dq_ptr;
        env.p2 = nzb_ptr;
        env.a1 = c_int::from(avoid_history);
        self.delete_queue_entry(&env) != 0
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_url_coord_new() -> *mut NzbgetRsUrlCoord {
    let arc = Arc::new(NzbgetRsUrlCoord::new());
    if let Ok(mut w) = arc.self_weak.lock() {
        *w = Arc::downgrade(&arc);
    }
    if let Ok(mut g) = ACTIVE_URL_COORD.write() {
        *g = Some(Arc::downgrade(&arc));
    }
    Arc::into_raw(arc).cast_mut()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_url_coord_free(coord: *mut NzbgetRsUrlCoord) {
    if !coord.is_null() {
        let arc = Arc::from_raw(coord.cast_const());
        let wk = Arc::downgrade(&arc);
        if let Ok(mut g) = ACTIVE_URL_COORD.write() {
            if g.as_ref()
                .is_some_and(|cur| std::sync::Weak::ptr_eq(cur, &wk))
            {
                *g = None;
            }
        }
        drop(arc);
    }
}

/// Dispatches `UrlCoordinator` and `UrlDownloader` operations:
/// - `0`: `Run()`
/// - `1`: `Stop()`
/// - `2`: `DownloadQueueUpdate(action)`
/// - `5`: `DeleteQueueEntry(dq_ptr, nzb_ptr, avoid_history) -> 1/0`
/// - `6`: `AddUrlToQueue(nzb_ptr, holder_ptr, add_first)`
/// - `7`: `HasMoreJobs() -> 1/0`
/// - `8`: `LogDebugInfo()`
/// - `9`: `~UrlCoordinator()` cleanup
/// - `10`: bind host environment (`ctx`, `with_queue_guard_cb`, `query_cb`, `action_cb`)
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_url_coord_exec(
    coord: *const NzbgetRsUrlCoord,
    op: c_int,
    env: *const NzbgetRsUrlCoordEnv,
) -> c_int {
    let Some(coord) = coord.as_ref() else {
        return 0;
    };
    if op == 7 {
        return c_int::from(coord.has_more_jobs.load(Ordering::SeqCst));
    }
    if op == 10 {
        if let Some(e) = env.as_ref() {
            if let Ok(mut g) = coord.host_env.lock() {
                *g = Some(*e);
            }
        }
        return 1;
    }
    let eff = coord.effective_env(env.as_ref());
    match op {
        0 => {
            coord.run_loop(&eff);
            1
        }
        1 => {
            coord.stop_downloads(&eff);
            1
        }
        2 => {
            coord.on_queue_update(&eff);
            1
        }
        5 => coord.delete_queue_entry(&eff),
        6 => {
            coord.add_url_to_queue(&eff);
            1
        }
        8 => {
            coord.log_debug_info(&eff);
            1
        }
        9 => {
            coord.destroy_remaining(&eff);
            1
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param(name: &[u8], value: &[u8]) -> Dnzb {
        Dnzb::Param(name.to_vec(), value.to_vec())
    }

    #[test]
    fn headers() {
        assert_eq!(
            dnzb_header(b"X-DNZB-Category: \tTV \r\n"),
            Dnzb::Category(18, 20)
        );
        assert_eq!(dnzb_header(b"X-DNZB-Category:"), Dnzb::Category(16, 16));
        assert_eq!(
            dnzb_header(b"X-DNZB-Category: \r\n"),
            Dnzb::Category(16, 16)
        );
        assert_eq!(
            dnzb_header(b"X-DNZB-Name:  Some Show \r\n"),
            param(b"*DNZB:Name", b"Some Show")
        );
        assert_eq!(
            dnzb_header(b"X-DNZB-Name: \tx"),
            param(b"*DNZB:Name", b"\tx")
        );
        assert_eq!(
            dnzb_header(b"X-DNZB-MoreInfo:http://a:b"),
            param(b"*DNZB:MoreInfo", b"http://a:b")
        );
        assert_eq!(dnzb_header(b"X-DNZB-:v"), param(b"*DNZB:", b"v"));
        assert_eq!(
            dnzb_header(b"X-DNZB-Name:caf\xe9"),
            param(b"*DNZB:Name", "café".as_bytes())
        );
        assert_eq!(dnzb_header(b"X-DNZB-Name"), Dnzb::None);
        assert_eq!(dnzb_header(b"x-dnzb-name: v"), Dnzb::None);
        assert_eq!(dnzb_header(b"Content-Type: x"), Dnzb::None);
        assert_eq!(
            dnzb_header(b"X-DNZB-category: v"),
            param(b"*DNZB:category", b"v")
        );
        let long = [b"X-DNZB-".as_slice(), &[b'n'; 200], b": v"].concat();
        let Dnzb::Param(name, _) = dnzb_header(&long) else {
            panic!()
        };
        assert_eq!(name.len(), 99);
    }

    #[test]
    fn statuses() {
        assert_eq!(completed(AD_FINISHED, false, 1, 0), (LS_FINISHED, 0, false));
        assert_eq!(completed(AD_FAILED, false, 1, 0), (LS_FAILED, 0, false));
        assert_eq!(completed(AD_RETRY, false, 1, 0), (LS_NONE, 0, true));
        assert_eq!(completed(8, false, 1, 0), (1, 0, false));
        assert_eq!(completed(AD_RETRY, true, 1, 0), (LS_NONE, DS_MANUAL, false));
        assert_eq!(completed(AD_FINISHED, true, 1, 3), (LS_NONE, 3, false));
        assert_eq!(
            select_next_url(&[1, 1], &[0, 0], &[0, 0], &[10, 50], 1, 0, 0),
            1
        );
        assert!(is_hanging_url_download(100, 50, 30, 1, 1));
        assert_eq!(format_temp_url_path(b"/tmp", b'/', 42), b"/tmp/url-42.tmp");
    }
}
