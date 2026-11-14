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

use crate::queuesubsys_host::{
    QueueSubsysHost, ASPECT_URL_ADDED, ASPECT_URL_COMPLETED, ASPECT_URL_DELETED, ASPECT_URL_FAILED,
    ASPECT_URL_FOUND, ASPECT_URL_RETURNED, NZB_INT_ACTIVE_DOWNLOADS, NZB_INT_AVOID_HISTORY,
    NZB_INT_DELETE_STATUS, NZB_INT_DELETING, NZB_INT_URL_STATUS,
};
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
    daemon_stopped: AtomicBool,
    daemon_running: AtomicBool,
    wait_mu: Mutex<()>,
    wait_cv: Condvar,
    self_weak: Mutex<std::sync::Weak<NzbgetRsUrlCoord>>,
}

impl Default for NzbgetRsUrlCoord {
    fn default() -> Self {
        Self::new()
    }
}

fn url_bytes_to_cstring(raw: &[u8]) -> std::ffi::CString {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    unsafe { std::ffi::CString::from_vec_unchecked(raw[..end].to_vec()) }
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

fn url_emit_log(level: i32, parts: &[&[u8]]) {
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
    #[cfg(not(unix))]
    let _ = (level, parts);
}

fn url_runtime_opts() -> (i32, bool, bool, i32, u8, Vec<u8>) {
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
    (
        0,
        false,
        false,
        0,
        crate::paths::PATH_SEPARATOR as u8,
        Vec::new(),
    )
}

fn with_dq_guard<F: FnOnce(&QueueSubsysHost, usize)>(f: F) {
    let Some(host) = crate::ffi::queue_host() else {
        return;
    };
    host.with_locked_queue(|dq_ptr| f(&host, dq_ptr));
}

impl NzbgetRsUrlCoord {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(Vec::new()),
            has_more_jobs: AtomicBool::new(true),
            daemon_stopped: AtomicBool::new(false),
            daemon_running: AtomicBool::new(false),
            wait_mu: Mutex::new(()),
            wait_cv: Condvar::new(),
            self_weak: Mutex::new(std::sync::Weak::new()),
        }
    }

    fn is_stopped(&self) -> bool {
        self.daemon_stopped.load(Ordering::SeqCst)
    }

    fn reset_hanging_downloads(&self) {
        let (_, _, _, url_timeout, _, _) = url_runtime_opts();
        if url_timeout == 0 {
            return;
        }
        with_dq_guard(|_host, _dq_ptr| {
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
                        url_emit_log(4, &[b"Cancelling hanging url download ", &info_name]);
                        dl.stop();
                    }
                }
            }
            #[cfg(not(unix))]
            let _ = tm;
        });
    }

    fn wait_jobs(&self) {
        url_emit_log(0, &[b"UrlCoordinator: waiting for Downloads to complete"]);
        loop {
            let mut is_empty = true;
            with_dq_guard(|_host, _dq_ptr| {
                is_empty = self.active.lock().map(|g| g.is_empty()).unwrap_or(true);
            });
            if is_empty {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            self.reset_hanging_downloads();
        }
        url_emit_log(0, &[b"UrlCoordinator: Downloads are completed"]);
    }

    fn run_loop(&self) {
        if crate::ffi::queue_host().is_none() {
            return;
        }
        url_emit_log(0, &[b"Entering UrlCoordinator-loop"]);

        while !crate::ffi::is_queue_loaded() {
            if self.is_stopped() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        let mut last_reset: i64 = 0;
        while !self.is_stopped() {
            let mut download_started = false;

            with_dq_guard(|host, dq_ptr| {
                let (url_connections, pause_download, url_force, _, sep, temp_dir) =
                    url_runtime_opts();

                let active_len = self.active.lock().map(|g| g.len() as i32).unwrap_or(0);
                let mut chosen_nzb: usize = 0;
                let mut chosen_id: i32 = 0;

                if active_len < url_connections {
                    let count = host.queue_len(dq_ptr);
                    if count > 0 {
                        let mut kinds = Vec::with_capacity(count);
                        let mut url_st = Vec::with_capacity(count);
                        let mut del_st = Vec::with_capacity(count);
                        let mut prios = Vec::with_capacity(count);
                        let mut ptrs = Vec::with_capacity(count);
                        let mut ids = Vec::with_capacity(count);
                        for idx in 0..count {
                            let nzb_ptr = host.queue_at(dq_ptr, idx);
                            let snap = host.nzb_snap(nzb_ptr, false);
                            kinds.push(snap.kind);
                            url_st.push(snap.url_status);
                            del_st.push(snap.delete_status);
                            prios.push(snap.priority);
                            ids.push(snap.id);
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
                        url_emit_log(0, &[b"Starting new UrlDownloader"]);
                        let tmp_path = format_temp_url_path(&temp_dir, sep, chosen_id);
                        let snap = host.nzb_snap(chosen_nzb, false);
                        let url_c = url_bytes_to_cstring(&snap.url);
                        let info_name =
                            crate::downloadinfo::make_nice_url_name(Some(&url_c), &snap.filename);
                        host.nzb_set_int(chosen_nzb, NZB_INT_ACTIVE_DOWNLOADS, 1);
                        host.nzb_set_int(chosen_nzb, NZB_INT_URL_STATUS, LS_RUNNING);
                        #[cfg(unix)]
                        {
                            let weak_s =
                                self.self_weak.lock().map(|g| g.clone()).unwrap_or_default();
                            let weak_h = weak_s.clone();
                            let dl = Arc::new(crate::net::NzbgetRsWebDl::new_configured(
                                &snap.url,
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
                self.reset_hanging_downloads();
                last_reset = url_now_sec();
            }

            if !self.has_more_jobs.load(Ordering::SeqCst) && !self.is_stopped() {
                if let Ok(mut lk) = self.wait_mu.lock() {
                    while !self.has_more_jobs.load(Ordering::SeqCst) && !self.is_stopped() {
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

        self.wait_jobs();
        url_emit_log(0, &[b"Exiting UrlCoordinator-loop"]);
    }

    fn stop_downloads(&self) {
        url_emit_log(0, &[b"Stopping UrlDownloads"]);
        with_dq_guard(|_host, _dq_ptr| {
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
        url_emit_log(0, &[b"UrlDownloads are notified"]);
        let _lk = self.wait_mu.lock();
        self.wait_cv.notify_all();
    }

    fn on_queue_update(&self, action: i32) {
        url_emit_log(0, &[b"Notification from download queue received"]);
        if action == ASPECT_URL_ADDED || action == ASPECT_URL_RETURNED {
            let _lk = self.wait_mu.lock();
            self.has_more_jobs.store(true, Ordering::SeqCst);
            self.wait_cv.notify_all();
        }
    }

    #[cfg(unix)]
    fn on_webdl_header(&self, dl: &crate::net::NzbgetRsWebDl, nzb_ptr: usize, line: &[u8]) {
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
                url_emit_log(0, &[b"Category: ", &cat]);
            }
            Dnzb::Param(name, val) => {
                url_emit_log(0, &[b"X-DNZB: ", &name]);
                url_emit_log(0, &[b"Value: ", &val]);
                if let Some(host) = crate::ffi::queue_host() {
                    host.nzb_set_param(nzb_ptr, &name, Some(&val));
                }
            }
        }
    }

    #[cfg(unix)]
    fn on_webdl_status(&self, dl: &crate::net::NzbgetRsWebDl, nzb_ptr: usize, status: i32) {
        url_emit_log(0, &[b"Notification from UrlDownloader received"]);
        let Some(host) = crate::ffi::queue_host() else {
            return;
        };
        if status != AD_FINISHED && status != AD_FAILED && status != AD_RETRY {
            return;
        }

        url_emit_log(0, &[b"URL downloaded"]);

        let orig_fn_opt = dl.original_filename_bytes();
        let out_fn_bytes = dl.output_filename_bytes();
        let dl_key = dl as *const crate::net::NzbgetRsWebDl;

        let init_snap = host.nzb_snap(nzb_ptr, false);
        let origname: Vec<u8> =
            orig_fn_opt.unwrap_or_else(|| crate::nserv::base_file_name(&init_snap.url).to_vec());
        let valid_filename = crate::paths::make_valid_filename(&origname, false);
        url_emit_log(0, &[b"Filename: [", &valid_filename, b"]"]);

        let mut retry = false;
        let mut dnzb_cat = Vec::new();
        let mut final_url_status = LS_NONE;

        host.with_locked_queue(|dq_ptr| {
            if let Ok(mut g) = self.active.lock() {
                if let Some(pos) = g.iter().position(|s| Arc::as_ptr(&s.dl) == dl_key) {
                    let removed = g.remove(pos);
                    dnzb_cat = removed.category;
                }
            }

            let cur_snap = host.nzb_snap(nzb_ptr, false);
            let (new_url, new_del, r) = completed(
                status,
                cur_snap.deleting,
                cur_snap.url_status,
                cur_snap.delete_status,
            );
            retry = r;
            final_url_status = new_url;

            host.nzb_set_int(nzb_ptr, NZB_INT_DELETE_STATUS, new_del);
            if cur_snap.deleting {
                host.nzb_set_int(nzb_ptr, NZB_INT_DELETING, 0);
            }
            host.nzb_set_int(nzb_ptr, NZB_INT_URL_STATUS, new_url);
            if !retry {
                host.dq_notify(dq_ptr, ASPECT_URL_COMPLETED, nzb_ptr, 0);
            }
        });

        if retry {
            host.nzb_set_int(nzb_ptr, NZB_INT_ACTIVE_DOWNLOADS, 0);
            return;
        }

        if final_url_status == LS_FINISHED {
            let cur_snap = host.nzb_snap(nzb_ptr, false);
            let eff_name = if !cur_snap.filename.is_empty() {
                cur_snap.filename
            } else {
                valid_filename
            };
            let eff_cat = if !cur_snap.category.is_empty() {
                cur_snap.category
            } else {
                dnzb_cat
            };

            let c_name = url_bytes_to_cstring(&eff_name);
            let is_archive = if c_name.as_bytes().is_empty() {
                false
            } else {
                use std::os::unix::ffi::OsStrExt;
                crate::unpack::can_extract_with_zipfast(std::path::Path::new(
                    std::ffi::OsStr::from_bytes(c_name.as_bytes()),
                ))
            };

            let add_status =
                host.scanner_add_url_file(nzb_ptr, is_archive, &eff_name, &eff_cat, &out_fn_bytes);

            if add_status == AS_SUCCESS {
                return;
            }

            let scan_st = if add_status == AS_FAILED {
                LS_SCAN_FAILED
            } else {
                LS_SCAN_SKIPPED
            };
            host.nzb_set_int(nzb_ptr, NZB_INT_URL_STATUS, scan_st);
        }

        host.with_locked_queue(|dq_ptr| {
            host.nzb_set_int(nzb_ptr, NZB_INT_ACTIVE_DOWNLOADS, 0);
            host.dq_notify(dq_ptr, ASPECT_URL_FAILED, nzb_ptr, 0);
        });
    }

    fn delete_queue_entry(&self, dq_ptr: usize, nzb_ptr: usize, avoid_history: bool) -> bool {
        let Some(host) = crate::ffi::queue_host() else {
            return false;
        };
        let snap = host.nzb_snap(nzb_ptr, false);

        if snap.active_downloads > 0 {
            url_emit_log(2, &[b"Deleting active URL ", &snap.name]);
            host.nzb_set_int(nzb_ptr, NZB_INT_DELETING, 1);
            host.nzb_set_int(nzb_ptr, NZB_INT_AVOID_HISTORY, i32::from(avoid_history));
            #[cfg(unix)]
            {
                let found_dl = self.active.lock().ok().and_then(|g| {
                    g.iter()
                        .find(|s| s.nzb_ptr == nzb_ptr)
                        .map(|s| Arc::clone(&s.dl))
                });
                if let Some(dl) = found_dl {
                    dl.stop();
                    return true;
                }
            }
            return false;
        }

        url_emit_log(2, &[b"Deleting URL ", &snap.name]);
        if snap.delete_status == DS_NONE {
            host.nzb_set_int(nzb_ptr, NZB_INT_DELETE_STATUS, DS_MANUAL);
        }
        host.nzb_set_int(nzb_ptr, NZB_INT_URL_STATUS, LS_NONE);
        host.dq_notify(dq_ptr, ASPECT_URL_DELETED, nzb_ptr, 0);
        true
    }

    fn add_url_to_queue(&self, nzb_ptr: usize, holder_ptr: usize, add_first: bool) {
        url_emit_log(0, &[b"Adding URL to queue"]);
        with_dq_guard(|host, dq_ptr| {
            host.dq_notify(dq_ptr, ASPECT_URL_FOUND, nzb_ptr, 0);
            let snap = host.nzb_snap(nzb_ptr, false);
            let do_add = snap.delete_status != DS_MANUAL;
            if do_add && holder_ptr != 0 {
                host.queue_insert_owned(dq_ptr, holder_ptr, 0, add_first);
                host.dq_notify(dq_ptr, ASPECT_URL_ADDED, nzb_ptr, 0);
            }
            host.dq_save(dq_ptr);
        });
    }

    fn log_debug_info(&self) {
        url_emit_log(2, &[b"   ---------- UrlCoordinator"]);
        with_dq_guard(|_host, _dq_ptr| {
            #[cfg(unix)]
            {
                let dls: Vec<Arc<crate::net::NzbgetRsWebDl>> = self
                    .active
                    .lock()
                    .map(|g| g.iter().map(|s| Arc::clone(&s.dl)).collect())
                    .unwrap_or_default();
                url_emit_log(
                    2,
                    &[format!("    Active Downloads: {}", dls.len()).as_bytes()],
                );
                for dl in dls {
                    dl.log_debug_info();
                }
            }
        });
    }

    fn destroy_remaining(&self) {
        url_emit_log(0, &[b"Destroying UrlCoordinator"]);
        if let Ok(mut g) = self.active.lock() {
            g.clear();
        }
        url_emit_log(0, &[b"UrlCoordinator destroyed"]);
    }

    pub(crate) fn delete_queue_entry_active(
        &self,
        dq_ptr: usize,
        nzb_ptr: usize,
        avoid_history: bool,
    ) -> bool {
        self.delete_queue_entry(dq_ptr, nzb_ptr, avoid_history)
    }
}

static ACTIVE_URL_COORD: std::sync::RwLock<Option<std::sync::Weak<NzbgetRsUrlCoord>>> =
    std::sync::RwLock::new(None);
static DAEMON_URL_COORD: std::sync::RwLock<Option<Arc<NzbgetRsUrlCoord>>> =
    std::sync::RwLock::new(None);
static DAEMON_URL_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

pub(crate) fn active_url_coord() -> Option<Arc<NzbgetRsUrlCoord>> {
    ACTIVE_URL_COORD
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
}

#[cfg(unix)]
pub(crate) fn create_daemon_url_coord() {
    let arc = Arc::new(NzbgetRsUrlCoord::new());
    if let Ok(mut w) = arc.self_weak.lock() {
        *w = Arc::downgrade(&arc);
    }
    if let Some(lg) = crate::log::active_log() {
        lg.register_debuggable(3);
    }
    if let Ok(mut g) = ACTIVE_URL_COORD.write() {
        *g = Some(Arc::downgrade(&arc));
    }
    if let Ok(mut g) = DAEMON_URL_COORD.write() {
        *g = Some(arc);
    }
}

#[cfg(unix)]
pub(crate) fn start_daemon_url_coord() {
    let Some(uc) = active_url_coord() else {
        return;
    };
    let mut g = DAEMON_URL_HANDLE.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_some() {
        return;
    }
    uc.daemon_stopped.store(false, Ordering::SeqCst);
    uc.daemon_running.store(true, Ordering::SeqCst);
    let uc_run = Arc::clone(&uc);
    if let Ok(h) = std::thread::Builder::new()
        .name("url-coord".into())
        .spawn(move || {
            struct RunningReset(Arc<NzbgetRsUrlCoord>);
            impl Drop for RunningReset {
                fn drop(&mut self) {
                    self.0.daemon_running.store(false, Ordering::SeqCst);
                }
            }
            let _reset = RunningReset(Arc::clone(&uc_run));
            let _tg = crate::ffi::RustThreadGuard::enter(&uc_run.daemon_stopped);
            uc_run.run_loop();
        })
    {
        *g = Some(h);
    } else {
        uc.daemon_running.store(false, Ordering::SeqCst);
    }
}

#[cfg(unix)]
pub(crate) fn stop_daemon_url_coord() {
    if let Some(uc) = active_url_coord() {
        uc.daemon_stopped.store(true, Ordering::SeqCst);
        uc.stop_downloads();
    }
}

#[cfg(unix)]
pub(crate) fn is_daemon_url_coord_running() -> bool {
    active_url_coord().is_some_and(|uc| uc.daemon_running.load(Ordering::SeqCst))
}

#[cfg(unix)]
pub(crate) fn daemon_url_coord_has_more_jobs() -> bool {
    active_url_coord().is_some_and(|uc| uc.has_more_jobs.load(Ordering::SeqCst))
}

#[cfg(unix)]
pub(crate) fn shutdown_daemon_url_coord() {
    stop_daemon_url_coord();
    let handle = DAEMON_URL_HANDLE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(h) = handle {
        let _ = h.join();
    }
    if let Some(uc) = DAEMON_URL_COORD.write().ok().and_then(|mut g| g.take()) {
        uc.destroy_remaining();
    }
    if let Ok(mut g) = ACTIVE_URL_COORD.write() {
        *g = None;
    }
}

pub(crate) fn notify_active_urlcoord_queue_update(action: i32) {
    if let Some(uc) = active_url_coord() {
        uc.on_queue_update(action);
    }
}

pub(crate) fn log_active_url_debug_info() {
    if let Some(uc) = active_url_coord() {
        uc.log_debug_info();
    }
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_feed_url_active(
    feed_id: i32,
    url: &[u8],
    filename: &[u8],
    category: &[u8],
    priority: i32,
    add_paused: bool,
    dupe_key: &[u8],
    dupe_score: i32,
    dupe_mode: i32,
    size: i64,
    time: i64,
) {
    use crate::queuesubsys_host::{
        NK_URL, NZB_I64_MAX_TIME, NZB_I64_MIN_TIME, NZB_I64_SIZE, NZB_INT_ADD_URL_PAUSED,
        NZB_INT_DUPE_MODE, NZB_INT_DUPE_SCORE, NZB_INT_FEED_ID, NZB_INT_KIND, NZB_INT_PRIORITY,
        NZB_STR_CATEGORY, NZB_STR_DUPE_KEY, NZB_STR_FILENAME, NZB_STR_URL,
    };
    let (Some(host), Some(uc)) = (crate::ffi::queue_host(), active_url_coord()) else {
        return;
    };
    let (uptr_addr, nzb_ptr) = host.alloc_new_nzb_uptr();
    if uptr_addr == 0 || nzb_ptr == 0 {
        host.destroy_nzb_uptr(uptr_addr);
        return;
    }
    host.nzb_set_int(nzb_ptr, NZB_INT_KIND, NK_URL);
    host.nzb_set_int(nzb_ptr, NZB_INT_FEED_ID, feed_id);
    host.nzb_set_str(nzb_ptr, NZB_STR_URL, url);
    if !filename.is_empty() {
        host.nzb_set_str(nzb_ptr, NZB_STR_FILENAME, filename);
    }
    host.nzb_set_str(nzb_ptr, NZB_STR_CATEGORY, category);
    host.nzb_set_int(nzb_ptr, NZB_INT_PRIORITY, priority);
    host.nzb_set_int(nzb_ptr, NZB_INT_ADD_URL_PAUSED, i32::from(add_paused));
    host.nzb_set_str(nzb_ptr, NZB_STR_DUPE_KEY, dupe_key);
    host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_SCORE, dupe_score);
    host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_MODE, dupe_mode);
    host.nzb_set_i64(nzb_ptr, NZB_I64_SIZE, size);
    host.nzb_set_i64(nzb_ptr, NZB_I64_MIN_TIME, time);
    host.nzb_set_i64(nzb_ptr, NZB_I64_MAX_TIME, time);
    uc.add_url_to_queue(nzb_ptr, uptr_addr, false);
    host.destroy_nzb_uptr(uptr_addr);
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_rpc_url_active(
    url: &[u8],
    filename: &[u8],
    category: &[u8],
    auto_category: bool,
    priority: i32,
    add_paused: bool,
    add_top: bool,
    dupe_key: &[u8],
    dupe_score: i32,
    dupe_mode: i32,
    params: &[(&[u8], &[u8])],
) -> Option<u32> {
    use crate::queuesubsys_host::{
        MK_INFO, NK_URL, NZB_INT_ADD_URL_PAUSED, NZB_INT_AUTO_CATEGORY, NZB_INT_DUPE_MODE,
        NZB_INT_DUPE_SCORE, NZB_INT_KIND, NZB_INT_PRIORITY, NZB_STR_CATEGORY, NZB_STR_DUPE_KEY,
        NZB_STR_FILENAME, NZB_STR_URL,
    };
    let (Some(host), Some(uc)) = (crate::ffi::queue_host(), active_url_coord()) else {
        return None;
    };
    let (uptr_addr, nzb_ptr) = host.alloc_new_nzb_uptr();
    if uptr_addr == 0 || nzb_ptr == 0 {
        host.destroy_nzb_uptr(uptr_addr);
        return None;
    }
    host.nzb_set_int(nzb_ptr, NZB_INT_KIND, NK_URL);
    host.nzb_set_str(nzb_ptr, NZB_STR_URL, url);
    host.nzb_set_str(nzb_ptr, NZB_STR_FILENAME, filename);
    host.nzb_set_str(nzb_ptr, NZB_STR_CATEGORY, category);
    host.nzb_set_int(nzb_ptr, NZB_INT_AUTO_CATEGORY, i32::from(auto_category));
    host.nzb_set_int(nzb_ptr, NZB_INT_PRIORITY, priority);
    host.nzb_set_int(nzb_ptr, NZB_INT_ADD_URL_PAUSED, i32::from(add_paused));
    host.nzb_set_str(nzb_ptr, NZB_STR_DUPE_KEY, dupe_key);
    host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_SCORE, dupe_score);
    host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_MODE, dupe_mode);
    for &(k, v) in params {
        host.nzb_set_param(nzb_ptr, k, Some(v));
    }
    let nzb_id = host.nzb_snap(nzb_ptr, false).id;
    let nice = host.nzb_make_nice_url_name(nzb_ptr, url, filename);
    let mut msg = b"Queue ".to_vec();
    msg.extend_from_slice(&nice);
    crate::log::log_active_bytes(MK_INFO, &msg);
    uc.add_url_to_queue(nzb_ptr, uptr_addr, add_top);
    host.destroy_nzb_uptr(uptr_addr);
    Some(nzb_id as u32)
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_binrpc_url_active(
    url: &[u8],
    filename: &[u8],
    category: &[u8],
    auto_category: bool,
    priority: i32,
    add_paused: bool,
    add_top: bool,
    dupe_key: &[u8],
    dupe_score: i32,
    dupe_mode: i32,
) -> bool {
    use crate::queuesubsys_host::{
        NK_URL, NZB_INT_ADD_URL_PAUSED, NZB_INT_AUTO_CATEGORY, NZB_INT_DUPE_MODE,
        NZB_INT_DUPE_SCORE, NZB_INT_KIND, NZB_INT_PRIORITY, NZB_STR_CATEGORY, NZB_STR_DUPE_KEY,
        NZB_STR_FILENAME, NZB_STR_URL,
    };
    let (Some(host), Some(uc)) = (crate::ffi::queue_host(), active_url_coord()) else {
        return false;
    };
    let (uptr_addr, nzb_ptr) = host.alloc_new_nzb_uptr();
    if uptr_addr == 0 || nzb_ptr == 0 {
        host.destroy_nzb_uptr(uptr_addr);
        return false;
    }
    host.nzb_set_int(nzb_ptr, NZB_INT_KIND, NK_URL);
    host.nzb_set_str(nzb_ptr, NZB_STR_URL, url);
    host.nzb_set_str(nzb_ptr, NZB_STR_FILENAME, filename);
    host.nzb_set_str(nzb_ptr, NZB_STR_CATEGORY, category);
    host.nzb_set_int(nzb_ptr, NZB_INT_AUTO_CATEGORY, i32::from(auto_category));
    host.nzb_set_int(nzb_ptr, NZB_INT_PRIORITY, priority);
    host.nzb_set_int(nzb_ptr, NZB_INT_ADD_URL_PAUSED, i32::from(add_paused));
    host.nzb_set_str(nzb_ptr, NZB_STR_DUPE_KEY, dupe_key);
    host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_SCORE, dupe_score);
    host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_MODE, dupe_mode);
    uc.add_url_to_queue(nzb_ptr, uptr_addr, add_top);
    host.destroy_nzb_uptr(uptr_addr);
    true
}

#[cfg(unix)]
pub(crate) fn add_testserver_url_active(server_id: i32, url: &[u8]) -> bool {
    use crate::queuesubsys_host::{
        DM_FORCE, NK_URL, NZB_FORCE_PRIORITY, NZB_INT_ADD_URL_PAUSED, NZB_INT_DESIRED_SERVER_ID,
        NZB_INT_DUPE_MODE, NZB_INT_EXTRA_PRIORITY, NZB_INT_KIND, NZB_INT_PRIORITY,
        NZB_INT_SKIP_DISK_WRITE, NZB_INT_SKIP_SCRIPT_PROCESSING, NZB_STR_FILENAME, NZB_STR_URL,
    };
    let (Some(host), Some(uc)) = (crate::ffi::queue_host(), active_url_coord()) else {
        return false;
    };
    let (uptr_addr, nzb_ptr) = host.alloc_new_nzb_uptr();
    if uptr_addr == 0 || nzb_ptr == 0 {
        host.destroy_nzb_uptr(uptr_addr);
        return false;
    }
    host.nzb_set_int(nzb_ptr, NZB_INT_KIND, NK_URL);
    host.nzb_set_str(nzb_ptr, NZB_STR_URL, url);
    host.nzb_set_str(nzb_ptr, NZB_STR_FILENAME, url);
    host.nzb_set_int(nzb_ptr, NZB_INT_PRIORITY, NZB_FORCE_PRIORITY);
    host.nzb_set_int(nzb_ptr, NZB_INT_EXTRA_PRIORITY, NZB_FORCE_PRIORITY);
    host.nzb_set_int(nzb_ptr, NZB_INT_ADD_URL_PAUSED, 0);
    host.nzb_set_int(nzb_ptr, NZB_INT_DUPE_MODE, DM_FORCE);
    host.nzb_set_int(nzb_ptr, NZB_INT_DESIRED_SERVER_ID, server_id);
    host.nzb_set_int(nzb_ptr, NZB_INT_SKIP_SCRIPT_PROCESSING, 1);
    host.nzb_set_int(nzb_ptr, NZB_INT_SKIP_DISK_WRITE, 1);
    uc.add_url_to_queue(nzb_ptr, uptr_addr, true);
    host.destroy_nzb_uptr(uptr_addr);
    true
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
