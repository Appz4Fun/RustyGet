//! Stateful Rust log engine (`NzbgetRsLog`)
//! backing `Log` (`daemon/util/Log.h` / `Log.cpp`) and `CommandScriptLog`.

use crate::cstr;
use crate::options::OptStr;
use std::collections::VecDeque;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;
use std::sync::Mutex;

use crate::options::NzbgetRsOptions;
use crate::xmlrpc_format::{NzbgetRsRpcLogEntry, RpcEmit};

type BinRpcLogBeginFn = unsafe extern "C" fn(*mut c_void, c_int, u32, *mut c_int);
type BinRpcLogPushFn = unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const c_char);

/// Severity of a log message. The discriminants are nzbget's
/// `Message::EKind` values, which disk state, RPC and the remaining
/// C-shaped callers still pass around as `i32`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum MessageKind {
    Info = 0,
    Warning = 1,
    Error = 2,
    Debug = 3,
    Detail = 4,
}

impl MessageKind {
    pub fn from_i32(kind: i32) -> Option<Self> {
        Some(match kind {
            0 => Self::Info,
            1 => Self::Warning,
            2 => Self::Error,
            3 => Self::Debug,
            4 => Self::Detail,
            _ => return None,
        })
    }

    /// The word written before the message in the log file.
    pub fn prefix(self) -> &'static [u8] {
        match self {
            Self::Info => b"INFO",
            Self::Warning => b"WARNING",
            Self::Error => b"ERROR",
            Self::Debug => b"DEBUG",
            Self::Detail => b"DETAIL",
        }
    }
}

pub const MK_INFO: i32 = MessageKind::Info as i32;
pub const MK_WARNING: i32 = MessageKind::Warning as i32;
pub const MK_ERROR: i32 = MessageKind::Error as i32;
pub const MK_DEBUG: i32 = MessageKind::Debug as i32;
pub const MK_DETAIL: i32 = MessageKind::Detail as i32;

#[inline]
fn current_unix_time() -> i64 {
    unsafe { libc::time(ptr::null_mut()) as i64 }
}

struct LogFileGuard(*mut libc::FILE);

unsafe impl Send for LogFileGuard {}

impl LogFileGuard {
    fn open_append(path: &CStr) -> Option<Self> {
        let fd = crate::fsops::open_cloexec_fd(path, 3, 0, 1, 2);
        if fd == -1 {
            return None;
        }
        let fp = unsafe { libc::fdopen(fd, c"ab".as_ptr()) };
        if fp.is_null() {
            let err = crate::fsops::last_errno();
            unsafe {
                let _ = libc::close(fd);
            }
            crate::fsops::set_last_errno(err);
            return None;
        }
        Some(Self(fp))
    }
}

impl Drop for LogFileGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = libc::fclose(self.0);
            }
            self.0 = ptr::null_mut();
        }
    }
}

#[derive(Clone)]
struct RsLogMessage {
    id: u32,
    kind: i32,
    time: i64,
    text: CString,
}

struct LogInner {
    messages: VecDeque<RsLogMessage>,
    log_file: Option<LogFileGuard>,
    id_gen: u32,
    last_written: i64,
    opt_init: bool,
    extra_debug: bool,
    /// the day (in days since the epoch) and size of what LogFile holds, learned on the first
    /// write; a LogFile that is a device (/dev/stdout) is never renamed
    file_day: Option<i64>,
    file_size: u64,
    device: bool,
}

/// LogFile is moved aside to `<stem>-YYYY-MM-DD.<n><ext>` (then gzipped) once it
/// reaches this size, and a new LogFile is started.
#[cfg(not(test))]
pub const LOG_PART_SIZE: u64 = 10 * 1024 * 1024;
#[cfg(test)]
pub const LOG_PART_SIZE: u64 = 4096;

impl LogInner {
    fn add_message(&mut self, kind: i32, text: &[u8], log_buffer: Option<i32>) {
        self.id_gen = self.id_gen.wrapping_add(1);
        self.messages.push_back(RsLogMessage {
            id: self.id_gen,
            kind,
            time: current_unix_time(),
            text: cstr::to_cstring(text),
        });
        if self.opt_init {
            // LogBuffer 0 keeps every message
            if let Some(cap) = log_buffer.filter(|&cap| cap > 0) {
                let max_len = cap as usize;
                while self.messages.len() > max_len {
                    self.messages.pop_front();
                }
            }
        }
    }
}

pub type NzbgetRsDebuggableFn = Option<unsafe extern "C" fn(usize)>;

pub struct NzbgetRsLog {
    inner: Mutex<LogInner>,
    path: Mutex<CString>,
    debuggables: Mutex<Vec<usize>>,
    debug_cb: NzbgetRsDebuggableFn,
    debug_build: bool,
}

impl NzbgetRsLog {
    pub fn with_debug_cb(debug_build: bool, debug_cb: NzbgetRsDebuggableFn) -> Self {
        let extra_debug = if debug_build {
            crate::fsops::file_exists(b"extradebug", &crate::fsops::posix_stat)
        } else {
            false
        };
        Self {
            inner: Mutex::new(LogInner {
                messages: VecDeque::new(),
                log_file: None,
                id_gen: 0,
                last_written: 0,
                opt_init: false,
                extra_debug,
                file_day: None,
                file_size: 0,
                device: false,
            }),
            path: Mutex::new(c"".to_owned()),
            debuggables: Mutex::new(Vec::new()),
            debug_cb,
            debug_build,
        }
    }

    pub(crate) fn register_debuggable(&self, ptr: usize) {
        if let Ok(mut dbg) = self.debuggables.lock() {
            dbg.push(ptr);
        }
    }

    pub(crate) fn unregister_debuggable(&self, ptr: usize) {
        if let Ok(mut dbg) = self.debuggables.lock() {
            dbg.retain(|&x| x != ptr);
        }
    }

    pub(crate) fn interval_check_active(&self) {
        let opts = crate::options::active_options();
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if inner.log_file.is_some() {
            let corr = opts.as_deref().map_or(0, |o| o.scalars().time_correction) as i64;
            let cur_time = current_unix_time() + corr;
            if crate::util::log_should_close_idle(cur_time, inner.last_written) {
                inner.log_file = None;
            }
        }
    }

    pub(crate) fn log_debug_info_active(&self) {
        let opts = crate::options::active_options();
        let opt_ref = opts.as_deref();
        let sep = c"--------------------------------------------";
        emit_message(self, 0, self.debug_build, opt_ref, sep);
        emit_message(
            self,
            0,
            self.debug_build,
            opt_ref,
            c"Dumping debug info to log",
        );
        emit_message(self, 0, self.debug_build, opt_ref, sep);
        let items = self
            .debuggables
            .lock()
            .map(|dbg| dbg.clone())
            .unwrap_or_default();
        for item in items {
            if item == 1 {
                crate::statmeter::log_active_statmeter_debug_info();
            } else if item == 2 {
                crate::feed::log_active_feed_debug_info();
            } else if item == 3 {
                crate::urlcoord::log_active_url_debug_info();
            } else if item == 4 {
                #[cfg(unix)]
                if let Some(sp) = crate::net::active_server_pool() {
                    sp.log_debug_info();
                }
            } else if item == 5 {
                #[cfg(unix)]
                crate::ffi::log_active_queue_coord_debug_info();
            } else if let Some(cb) = self.debug_cb {
                unsafe { cb(item) };
            }
        }
        emit_message(self, 0, self.debug_build, opt_ref, sep);
    }

    pub(crate) fn log_filename_bytes(&self) -> Vec<u8> {
        self.path
            .lock()
            .map_or_else(|_| Vec::new(), |p| p.to_bytes().to_vec())
    }

    pub(crate) fn emit_bytes(&self, kind: i32, options: Option<&NzbgetRsOptions>, text: &[u8]) {
        let cstr = cstr::to_cstring(text);
        emit_message(self, kind, self.debug_build, options, &cstr);
    }

    pub(crate) fn init_options_active(&self) {
        let opts = crate::options::active_options();
        let flags = opts.as_deref().map_or(0, |o| {
            c_int::from(o.server_mode())
                | (c_int::from(o.remote_client_mode()) << 1)
                | (c_int::from(o.direct_client_mode()) << 2)
        });
        unsafe {
            nzbget_rs_log_ctl(
                self,
                1,
                flags,
                0,
                opts.as_deref().map_or(ptr::null(), |o| o as *const _),
                ptr::null(),
                None,
            );
        }
    }
}

fn target_for_kind(kind: i32, options: Option<&NzbgetRsOptions>) -> i32 {
    let kind = MessageKind::from_i32(kind);
    if let Some(opts) = options {
        let sc = opts.scalars();
        match kind {
            Some(MessageKind::Info) => sc.info_target,
            Some(MessageKind::Warning) => sc.warning_target,
            Some(MessageKind::Error) => sc.error_target,
            Some(MessageKind::Debug) => sc.debug_target,
            Some(MessageKind::Detail) => sc.detail_target,
            None => 0,
        }
    } else if kind == Some(MessageKind::Error) {
        3 // mtBoth
    } else {
        1 // mtScreen
    }
}

fn prefix_for_kind(kind: i32) -> &'static [u8] {
    MessageKind::from_i32(kind).map_or(b"INFO", MessageKind::prefix)
}

fn format_truncated_1023(prefix: &[u8], text: &[u8]) -> Vec<u8> {
    let text_end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    let mut out = Vec::with_capacity((prefix.len() + 1 + text_end).min(1023));
    let p_take = prefix.len().min(1023);
    out.extend_from_slice(&prefix[..p_take]);
    if out.len() < 1023 {
        out.push(b'\t');
    }
    if out.len() < 1023 {
        let rem = 1023 - out.len();
        out.extend_from_slice(&text[..text_end.min(rem)]);
    }
    out
}

#[cfg(test)]
pub(crate) fn native_detail_record_for_test(text: &[u8]) -> Vec<u8> {
    format_truncated_1023(MessageKind::Detail.prefix(), text)
}

fn filelog_locked(
    inner: &mut LogInner,
    path_mtx: &Mutex<CString>,
    prefix: &[u8],
    text: &[u8],
    debug_build: bool,
    options: Option<&NzbgetRsOptions>,
) {
    let log_filename = match path_mtx.lock() {
        Ok(g) => g.clone(),
        Err(_) => return,
    };
    if log_filename.to_bytes().is_empty() {
        return;
    }

    let tmp2 = format_truncated_1023(prefix, text);
    let time_corr = options.map_or(0, |o| o.scalars().time_correction) as i64;
    let write_log = options.map_or(0, |o| o.scalars().write_log);
    let rotate_log = options.map_or(crate::options::LOG_ROTATIONS_DEFAULT, |o| o.scalars().rotate_log);
    let log_buffer = options.map(|o| o.scalars().log_buffer);

    let rawtime = current_unix_time() + time_corr;
    let mut time_buf = [0u8; 50];
    crate::util::util_format_time(rawtime, &mut time_buf);
    let time_len = time_buf
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(time_buf.len());

    inner.last_written = rawtime;

    let mut line = Vec::with_capacity(time_len + tmp2.len() + 48);
    line.extend_from_slice(&time_buf[..time_len]);
    line.push(b'\t');
    if debug_build {
        let pid = unsafe { libc::getpid() as u64 };
        let tid = unsafe { libc::pthread_self() as usize as u64 };
        use std::io::Write as _;
        let _ = write!(line, "{pid}\t{tid}\t");
    }
    line.extend_from_slice(&tmp2);
    line.push(b'\n');

    if write_log == 3 {
        rotate_if_due(inner, &log_filename, rawtime, time_corr, line.len() as u64, rotate_log, log_buffer);
    }

    if inner.log_file.is_none() {
        match LogFileGuard::open_append(&log_filename) {
            None => {
                unsafe {
                    libc::perror(log_filename.as_ptr());
                }
                return;
            }
            Some(guard) => {
                unsafe {
                    if libc::getuid() == 0 || libc::geteuid() == 0 {
                        if let Some(user) = options.and_then(|o| o.str_cstr(OptStr::DaemonUsername)) {
                            let pwd = libc::getpwnam(user.as_ptr());
                            let fd = libc::fileno(guard.0);
                            if !pwd.is_null() && fd >= 0 {
                                let _ = libc::fchown(fd, (*pwd).pw_uid, (*pwd).pw_gid);
                            }
                        }
                    }
                }
                inner.log_file = Some(guard);
            }
        }
    }

    let Some(ref guard) = inner.log_file else {
        return;
    };
    let fp = guard.0;
    let size = unsafe {
        let _ = libc::fseek(fp, 0, libc::SEEK_END);
        libc::ftell(fp)
    };

    let written = unsafe {
        let n = libc::fwrite(line.as_ptr().cast(), 1, line.len(), fp);
        let _ = libc::fflush(fp);
        n
    };
    inner.file_size = size.max(0) as u64 + written as u64;
}

/// Moves LogFile aside to `<stem>-<day>.<n><ext>` when the next line would take it
/// past `LOG_PART_SIZE`, then gzips the moved file (`.gz`) and keeps only the newest
/// `keep` rolled files (0: all of them) on a background thread.
fn rotate_if_due(
    inner: &mut LogInner,
    log_filename: &CStr,
    rawtime: i64,
    time_corr: i64,
    line_len: u64,
    keep: i32,
    log_buffer: Option<i32>,
) {
    use std::os::unix::ffi::OsStrExt as _;
    let path = log_filename.to_bytes();
    let os_path = std::path::Path::new(std::ffi::OsStr::from_bytes(path));
    let today = rawtime.div_euclid(86_400);
    let sep = crate::paths::PATH_SEPARATOR as u8;
    if inner.file_day.is_none() {
        // the first write: learn what an existing LogFile holds, and compress and
        // prune what earlier runs rolled over
        match std::fs::metadata(os_path) {
            Ok(m) if !m.is_file() => inner.device = true,
            Ok(m) => inner.file_size = m.len(),
            _ => inner.file_size = 0,
        }
        inner.file_day = Some(today);
        if !inner.device {
            roll_up_in_background(path.to_vec(), sep, keep);
        }
    }
    let _ = (time_corr, log_buffer);
    if inner.device || inner.file_size == 0 || inner.file_size + line_len <= LOG_PART_SIZE {
        return;
    }
    inner.log_file = None;
    let day_path = crate::systemhealth::log_rotated_day_path(path, sep, today * 86_400);
    // the next part is one past the highest of today, gzipped or not: never a
    // gap that pruning left, or the newest file would sort as the oldest
    let part = crate::systemhealth::log_highest_part(&day_path) + 1;
    let target = crate::systemhealth::log_part_path(&day_path, part);
    if std::fs::rename(os_path, std::ffi::OsStr::from_bytes(&target)).is_err() {
        // could not move it aside: keep writing where we are rather than losing lines
        return;
    }
    inner.file_size = 0;
    inner.file_day = Some(today);
    roll_up_in_background(path.to_vec(), sep, keep);
}

/// Gzips every rolled-over log next to `log_file` and deletes the oldest beyond
/// `keep` (0 keeps all), off the logging path. One roll-up runs at a time.
fn roll_up_in_background(log_file: Vec<u8>, sep: u8, keep: i32) {
    static ROLL_UP: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let work = move || {
        let _one = ROLL_UP.lock().unwrap_or_else(|e| e.into_inner());
        crate::systemhealth::log_roll_up(&log_file, sep, keep);
    };
    if cfg!(test) {
        work();
    } else if std::thread::Builder::new()
        .name("log-rollup".into())
        .spawn(work.clone())
        .is_err()
    {
        work();
    }
}

fn emit_message(
    log: &NzbgetRsLog,
    kind: i32,
    debug_build: bool,
    options: Option<&NzbgetRsOptions>,
    text_cstr: &CStr,
) {
    if kind == MK_DEBUG && !debug_build {
        return;
    }
    let Ok(mut inner) = log.inner.lock() else {
        return;
    };
    if kind == MK_DEBUG && options.is_none() && inner.extra_debug {
        unsafe {
            libc::printf(c"%s\n".as_ptr(), text_cstr.as_ptr());
        }
    }
    let target = target_for_kind(kind, options);
    let text_bytes = text_cstr.to_bytes();
    if target == 1 || target == 3 {
        inner.add_message(kind, text_bytes, options.map(|o| o.scalars().log_buffer));
    }
    if target == 2 || target == 3 {
        filelog_locked(
            &mut inner,
            &log.path,
            prefix_for_kind(kind),
            text_bytes,
            debug_build,
            options,
        );
    }
}

fn collect_messages_rpc(
    messages: &VecDeque<RsLogMessage>,
    id_from: c_int,
    max_entries: c_int,
    out: &mut dyn RpcEmit,
) {
    let len = messages.len() as i32;
    let mut start = len;
    if max_entries > 0 {
        start = len - max_entries.min(len);
    }
    if id_from > 0 {
        if let Some(front) = messages.front() {
            start = (id_from.wrapping_sub(front.id as i32)).max(0);
        }
    }
    let entries: Vec<NzbgetRsRpcLogEntry<'_>> = if (start as u32) < (messages.len() as u32) {
        messages
            .iter()
            .skip(start as usize)
            .map(|m| NzbgetRsRpcLogEntry {
                id: m.id as c_int,
                kind: m.kind,
                time: m.time as c_int,
                text: &m.text,
            })
            .collect()
    } else {
        Vec::new()
    };
    out.log(&entries);
}

static ACTIVE_LOG: std::sync::RwLock<Option<std::sync::Weak<NzbgetRsLog>>> =
    std::sync::RwLock::new(None);

impl NzbgetRsLog {
    /// The XML/JSON-RPC `log` result: messages from `id_from`, or the last
    /// `max_entries` (both 0 = all).
    pub(crate) fn collect_rpc(&self, id_from: c_int, max_entries: c_int, out: &mut dyn RpcEmit) {
        if let Ok(inner) = self.inner.lock() {
            collect_messages_rpc(&inner.messages, id_from, max_entries, out);
        }
    }
}

pub(crate) fn active_log() -> Option<std::sync::Arc<NzbgetRsLog>> {
    ACTIVE_LOG
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
}

pub(crate) fn active_debug_build() -> bool {
    active_log().is_some_and(|l| l.debug_build)
}

pub(crate) fn log_active_bytes(kind: i32, bytes: &[u8]) {
    let Some(lg) = active_log() else {
        return;
    };
    let opts = crate::options::active_options();
    let cs = cstr::to_cstring(bytes);
    emit_message(&lg, kind, lg.debug_build, opts.as_deref(), &cs);
}

/// Creates a log and makes it the active one.
pub(crate) fn install_log(debug_build: bool) -> std::sync::Arc<NzbgetRsLog> {
    let arc = std::sync::Arc::new(NzbgetRsLog::with_debug_cb(debug_build, None));
    if let Ok(mut g) = ACTIVE_LOG.write() {
        *g = Some(std::sync::Arc::downgrade(&arc));
    }
    arc
}

/// Drops `log`, first clearing the active log if it is this one.
pub(crate) fn uninstall_log(log: std::sync::Arc<NzbgetRsLog>) {
    let wk = std::sync::Arc::downgrade(&log);
    if let Ok(mut g) = ACTIVE_LOG.write() {
        if g.as_ref()
            .is_some_and(|cur| std::sync::Weak::ptr_eq(cur, &wk))
        {
            *g = None;
        }
    }
    drop(log);
}

pub unsafe extern "C" fn nzbget_rs_log_emit(
    log: *const NzbgetRsLog,
    kind: c_int,
    options: *const NzbgetRsOptions,
    text: *const c_char,
) {
    let Some(lg) = log.as_ref() else {
        return;
    };
    let cstr = if text.is_null() {
        c""
    } else {
        CStr::from_ptr(text)
    };
    emit_message(lg, kind, lg.debug_build, options.as_ref(), cstr);
}

/// Emits `text` (a `Message::EKind`) to the active log with the active options,
/// as the C++ `error`/`warn`/`info`/`detail`/`debug` functions do. Without an
/// active log it does nothing.
///
/// # Safety
/// `text` is null or NUL-terminated.
pub unsafe extern "C" fn nzbget_rs_log_active_emit(kind: c_int, text: *const c_char) {
    let Some(lg) = active_log() else {
        return;
    };
    let cstr = if text.is_null() {
        c""
    } else {
        CStr::from_ptr(text)
    };
    let opts = crate::options::active_options();
    emit_message(&lg, kind, lg.debug_build, opts.as_deref(), cstr);
}

pub(crate) unsafe fn nzbget_rs_log_ctl(
    log: *const NzbgetRsLog,
    op: c_int,
    a1: c_int,
    p1: usize,
    options: *const NzbgetRsOptions,
    s1: *const c_char,
    debug_cb: Option<unsafe extern "C" fn(usize)>,
) {
    let Some(lg) = log.as_ref() else {
        return;
    };
    let opts = options.as_ref();
    match op {
        0 => {
            if let Ok(mut inner) = lg.inner.lock() {
                inner.messages.clear();
            }
        }
        1 => {
            let server_mode = (a1 & 1) != 0;
            let remote_mode = (a1 & 2) != 0;
            let direct_mode = (a1 & 4) != 0;
            let debug_build = lg.debug_build;
            if let Some(o) = opts {
                if !remote_mode && !direct_mode && o.scalars().write_log != 0 {
                    if let Some(lf) = o.str_cstr(OptStr::LogFile) {
                        if let Ok(mut pg) = lg.path.lock() {
                            *pg = lf.to_owned();
                        }
                        if server_mode && o.scalars().write_log == 2 {
                            let _ = libc::unlink(lf.as_ptr());
                        }
                    }
                }
            }
            let Ok(mut inner) = lg.inner.lock() else {
                return;
            };
            inner.id_gen = 0;
            let mut i = 0usize;
            while i < inner.messages.len() {
                let kind = inner.messages[i].kind;
                let target = target_for_kind(kind, opts);
                let (write_to_file, keep_in_screen) =
                    crate::util::log_init_message_disposition(target);
                if write_to_file {
                    let msg_bytes = inner.messages[i].text.to_bytes().to_vec();
                    filelog_locked(
                        &mut inner,
                        &lg.path,
                        prefix_for_kind(kind),
                        &msg_bytes,
                        debug_build,
                        opts,
                    );
                }
                if !keep_in_screen {
                    inner.messages.remove(i);
                } else {
                    inner.id_gen = inner.id_gen.wrapping_add(1);
                    inner.messages[i].id = inner.id_gen;
                    i += 1;
                }
            }
            inner.opt_init = true;
        }
        2 => {
            let Ok(mut inner) = lg.inner.lock() else {
                return;
            };
            if inner.log_file.is_some() {
                let corr = opts.map_or(0, |o| o.scalars().time_correction) as i64;
                let cur_time = current_unix_time() + corr;
                if crate::util::log_should_close_idle(cur_time, inner.last_written) {
                    inner.log_file = None;
                }
            }
        }
        3 => {
            if let Some(lf) = opts.and_then(|o| o.str_cstr(OptStr::LogFile)) {
                let _ = libc::unlink(lf.as_ptr());
            }
        }
        4 => {
            let new_c = if s1.is_null() {
                c"".to_owned()
            } else {
                CStr::from_ptr(s1).to_owned()
            };
            if let Ok(mut pg) = lg.path.lock() {
                *pg = new_c;
            }
        }
        5 => {
            if let Ok(mut dbg) = lg.debuggables.lock() {
                dbg.push(p1);
            }
        }
        6 => {
            if let Ok(mut dbg) = lg.debuggables.lock() {
                dbg.retain(|&x| x != p1);
            }
        }
        7 => {
            let debug_build = lg.debug_build;
            let sep = c"--------------------------------------------";
            emit_message(lg, 0, debug_build, opts, sep);
            emit_message(lg, 0, debug_build, opts, c"Dumping debug info to log");
            emit_message(lg, 0, debug_build, opts, sep);
            if let Ok(dbg) = lg.debuggables.lock() {
                if let Some(cb) = debug_cb {
                    for &item in dbg.iter() {
                        cb(item);
                    }
                }
            }
            emit_message(lg, 0, debug_build, opts, sep);
        }
        _ => {}
    }
}

pub(crate) fn clear_active_log() {
    if let Some(lg) = active_log() {
        if let Ok(mut inner) = lg.inner.lock() {
            inner.messages.clear();
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn nzbget_rs_log_collect(
    mode: c_int,
    log: *const NzbgetRsLog,
    a1: c_int,
    _a2: c_int,
    u1: u32,
    out_int: *mut c_int,
    ctx: *mut c_void,
    fn1: usize,
    fn2: usize,
) {
    match mode {
        0 => {
            // Frontend::local_messages_cb
            let Some(lg) = log.as_ref() else {
                return;
            };
            let Ok(inner) = lg.inner.lock() else {
                return;
            };
            if inner.messages.is_empty() || fn1 == 0 {
                return;
            }
            let emit: unsafe extern "C" fn(*mut c_void, u32, c_int, i64, *const c_char) =
                std::mem::transmute(fn1);
            if a1 == 0 {
                let front_id = inner.messages.front().map_or(0, |m| m.id as i32);
                let mut start = (u1 as i32).wrapping_sub(front_id).wrapping_add(1);
                if start < 0 {
                    if !out_int.is_null() {
                        *out_int = 1;
                    }
                    start = 0;
                }
                for m in inner.messages.iter().skip(start as usize) {
                    emit(ctx, m.id, m.kind, m.time, m.text.as_ptr());
                }
            } else {
                for m in &inner.messages {
                    emit(ctx, m.id, m.kind, m.time, m.text.as_ptr());
                }
            }
        }
        2 => {
            // BinRPC collect_log_cb
            let Some(lg) = log.as_ref() else {
                return;
            };
            if fn1 == 0 || fn2 == 0 {
                return;
            }
            let begin_log: BinRpcLogBeginFn = std::mem::transmute(fn1);
            let push_log: BinRpcLogPushFn = std::mem::transmute(fn2);
            let Ok(inner) = lg.inner.lock() else {
                return;
            };
            let mut start: c_int = 0;
            let first_id = inner.messages.front().map_or(0, |m| m.id);
            begin_log(ctx, inner.messages.len() as c_int, first_id, &mut start);
            if (start as u32) < (inner.messages.len() as u32) {
                for m in inner.messages.iter().skip(start as usize) {
                    push_log(
                        ctx,
                        m.id,
                        m.kind as u32,
                        (m.time as i32) as u32,
                        m.text.as_ptr(),
                    );
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::FileTypeExt as _;
    use crate::options::{nzbget_rs_options_free, nzbget_rs_options_new, NzbgetRsParsedOptions};

    #[test]
    fn startup_buffering_init_options_file_flush_and_ring_trim() {
        let dir = std::env::temp_dir().join(format!(
            "rustyget_log_test_{}_{}",
            std::process::id(),
            current_unix_time()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let log_path = dir.join("nzbget.log");
        let _ = std::fs::write(&log_path, b"stale line\n");

        let log_arc = install_log(false);
        let log = std::sync::Arc::as_ptr(&log_arc);
        unsafe {
            // Startup messages before Options exist:
            // info (kind 0) -> mtScreen, detail (kind 4) -> mtScreen, error (kind 2) -> mtBoth
            nzbget_rs_log_emit(log, 0, ptr::null(), c"startup info".as_ptr());
            nzbget_rs_log_emit(log, 4, ptr::null(), c"startup detail".as_ptr());
            nzbget_rs_log_emit(log, 2, ptr::null(), c"startup err".as_ptr());
        }

        let log_opt = CString::new(format!("LogFile={}", log_path.to_str().unwrap())).unwrap();
        let cmd_opts = [
            log_opt.as_ptr(),
            c"LogToFile=yes".as_ptr(),
            c"InfoTarget=both".as_ptr(),
            c"DetailTarget=log".as_ptr(),
            c"ErrorTarget=both".as_ptr(),
            c"LogBuffer=2".as_ptr(),
        ];
        let mut scalars = NzbgetRsParsedOptions::default();
        let mut flags = 0;
        let opts = unsafe {
            nzbget_rs_options_new(
                c"rustyget".as_ptr(),
                ptr::null(),
                1,
                1,
                cmd_opts.as_ptr(),
                cmd_opts.len(),
                0,
                c"1.0".as_ptr(),
                ptr::null_mut(),
                None,
                &mut scalars,
                &mut flags,
            )
        };

        unsafe {
            // server_mode = 1 -> resets stale log file and replays buffered messages
            nzbget_rs_log_ctl(log, 1, 1, 0, opts, ptr::null(), None);
        }

        // LogFile itself is written: the old line is kept and new ones follow it
        let disk_text = std::fs::read_to_string(&log_path).unwrap();
        assert!(disk_text.starts_with("stale line\n"));
        assert!(disk_text.contains("INFO\tstartup info\n"));
        assert!(disk_text.contains("DETAIL\tstartup detail\n"));
        assert!(disk_text.contains("ERROR\tstartup err\n"));

        // Screen messages should contain only startup info (id 1) and startup err (id 2); detail was log-only!
        unsafe extern "C" fn collect_local(
            ctx: *mut c_void,
            id: u32,
            kind: c_int,
            _time: i64,
            text: *const c_char,
        ) {
            let v = &mut *(ctx as *mut Vec<(u32, i32, String)>);
            v.push((id, kind, CStr::from_ptr(text).to_str().unwrap().to_owned()));
        }
        let mut screen: Vec<(u32, i32, String)> = Vec::new();
        unsafe {
            nzbget_rs_log_collect(
                0,
                log,
                1,
                0,
                0,
                ptr::null_mut(),
                (&mut screen as *mut Vec<_>).cast(),
                collect_local as *const () as usize,
                0,
            );
        }
        assert_eq!(
            screen,
            vec![
                (1, 0, "startup info".to_owned()),
                (2, 2, "startup err".to_owned()),
            ]
        );

        // Emit one more message -> LogBuffer=2 evicts id 1, and local tailMode=0 from id 0 sets skipped=1!
        unsafe {
            nzbget_rs_log_emit(log, 1, opts, c"post warn".as_ptr());
        }
        screen.clear();
        let mut skipped = 0;
        unsafe {
            nzbget_rs_log_collect(
                0,
                log,
                0,
                0,
                0,
                &mut skipped,
                (&mut screen as *mut Vec<_>).cast(),
                collect_local as *const () as usize,
                0,
            );
        }
        assert_eq!(skipped, 1);
        assert_eq!(screen.len(), 2);
        assert_eq!(screen[0].0, 2);
        assert_eq!(screen[1].0, 3);

        unsafe {
            uninstall_log(log_arc);
            nzbget_rs_options_free(opts);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_full_log_continues_in_numbered_parts_and_log_buffer_zero_keeps_all() {
        let dir = std::env::temp_dir().join(format!(
            "rustyget_log_parts_{}_{}",
            std::process::id(),
            current_unix_time()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let log_path = dir.join("rustyget.log");
        let log_opt = CString::new(format!("LogFile={}", log_path.to_str().unwrap())).unwrap();
        let cmd_opts = [
            log_opt.as_ptr(),
            c"LogToFile=yes".as_ptr(),
            c"InfoTarget=both".as_ptr(),
            c"LogBuffer=0".as_ptr(),
        ];
        let mut scalars = NzbgetRsParsedOptions::default();
        let mut flags = 0;
        let log_arc = install_log(false);
        let log = std::sync::Arc::as_ptr(&log_arc);
        let opts = unsafe {
            nzbget_rs_options_new(
                c"rustyget".as_ptr(),
                ptr::null(),
                1,
                1,
                cmd_opts.as_ptr(),
                cmd_opts.len(),
                0,
                c"1.0".as_ptr(),
                ptr::null_mut(),
                None,
                &mut scalars,
                &mut flags,
            )
        };
        let line = CString::new("x".repeat(200)).unwrap();
        unsafe {
            nzbget_rs_log_ctl(log, 1, 1, 0, opts, ptr::null(), None);
            for _ in 0..60 {
                nzbget_rs_log_emit(log, 0, opts, line.as_ptr());
            }
        }
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().to_str().unwrap().to_owned()))
            .collect();
        names.sort();
        // 60 lines of ~230 bytes is ~14KB: LogFile plus parts .1 to .3 of today, 4KB each,
        // each gzipped once it is moved aside
        assert_eq!(names.len(), 4, "{names:?}");
        assert!(names.contains(&"rustyget.log".to_owned()), "LogFile is still the one written: {names:?}");
        let part1 = names.iter().find(|n| n.ends_with(".1.log.gz")).expect("a first part").clone();
        assert!(part1.starts_with("rustyget-") && part1.len() == "rustyget-2026-11-15.1.log.gz".len(), "{part1}");
        let stem = &part1[..part1.len() - 9];
        for part in 2..=3 {
            assert!(names.contains(&format!("{stem}.{part}.log.gz")), "{names:?}");
        }
        let read = |name: &str| -> String {
            let mut text = String::new();
            if name.ends_with(".gz") {
                let f = std::fs::File::open(dir.join(name)).unwrap();
                std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(f), &mut text).unwrap();
            } else {
                text = std::fs::read_to_string(dir.join(name)).unwrap();
            }
            text
        };
        // the newest lines are in LogFile, the oldest in part 1
        assert!(read(&part1).lines().count() > 0);
        let mut total = 0;
        for name in &names {
            let text = read(name);
            assert!(text.len() as u64 <= LOG_PART_SIZE, "{name} is {} bytes", text.len());
            total += text.lines().count();
        }
        assert_eq!(total, 60, "every line is written once");

        let mut screen: Vec<(u32, i32, String)> = Vec::new();
        unsafe extern "C" fn collect(ctx: *mut c_void, id: u32, kind: c_int, _time: i64, text: *const c_char) {
            let v = &mut *(ctx as *mut Vec<(u32, i32, String)>);
            v.push((id, kind, CStr::from_ptr(text).to_str().unwrap().to_owned()));
        }
        unsafe {
            nzbget_rs_log_collect(
                0,
                log,
                1,
                0,
                0,
                ptr::null_mut(),
                (&mut screen as *mut Vec<_>).cast(),
                collect as *const () as usize,
                0,
            );
            uninstall_log(log_arc);
            nzbget_rs_options_free(opts);
        }
        assert_eq!(screen.len(), 60, "LogBuffer 0 keeps every message");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_rotations_keeps_only_the_newest_rolled_over_files() {
        let dir = std::env::temp_dir().join(format!(
            "rustyget_log_keep_{}_{}",
            std::process::id(),
            current_unix_time()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let log_path = dir.join("rustyget.log");
        let log_opt = CString::new(format!("LogFile={}", log_path.to_str().unwrap())).unwrap();
        let cmd_opts = [
            log_opt.as_ptr(),
            c"LogToFile=yes".as_ptr(),
            c"InfoTarget=log".as_ptr(),
            c"LogRotations=2".as_ptr(),
        ];
        let mut scalars = NzbgetRsParsedOptions::default();
        let mut flags = 0;
        let log_arc = install_log(false);
        let log = std::sync::Arc::as_ptr(&log_arc);
        let opts = unsafe {
            nzbget_rs_options_new(
                c"rustyget".as_ptr(),
                ptr::null(),
                1,
                1,
                cmd_opts.as_ptr(),
                cmd_opts.len(),
                0,
                c"1.0".as_ptr(),
                ptr::null_mut(),
                None,
                &mut scalars,
                &mut flags,
            )
        };
        unsafe {
            nzbget_rs_log_ctl(log, 1, 1, 0, opts, ptr::null(), None);
            for i in 0..120 {
                let line = CString::new(format!("line {i:03} {}", "y".repeat(190))).unwrap();
                nzbget_rs_log_emit(log, 0, opts, line.as_ptr());
            }
            uninstall_log(log_arc);
            nzbget_rs_options_free(opts);
        }
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().to_str().unwrap().to_owned()))
            .collect();
        names.sort();
        // ~28KB is LogFile plus 6 or 7 parts of 4KB; only the newest 2 are kept, gzipped
        let parts: Vec<&String> = names.iter().filter(|n| n.ends_with(".log.gz")).collect();
        assert_eq!(parts.len(), 2, "{names:?}");
        assert_eq!(names.len(), 3, "LogFile and two rolled-over files: {names:?}");
        assert!(!names.iter().any(|n| n.ends_with(".1.log.gz")), "the oldest went first: {names:?}");
        // the two kept archives hold the lines just before LogFile's: nothing newer was lost
        let mut kept = String::new();
        for name in &parts {
            let f = std::fs::File::open(dir.join(name)).unwrap();
            std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(f), &mut kept).unwrap();
        }
        let current = std::fs::read_to_string(dir.join("rustyget.log")).unwrap();
        let first_current: usize = current.lines().next().unwrap().split("line ").nth(1).unwrap()[..3].parse().unwrap();
        let last_kept = kept
            .lines()
            .map(|l| l.split("line ").nth(1).unwrap()[..3].parse::<usize>().unwrap())
            .max()
            .unwrap();
        assert_eq!(last_kept + 1, first_current, "the newest rolled lines are kept");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_device_log_file_is_written_as_is_and_never_renamed() {
        let mut inner = LogInner {
            messages: VecDeque::new(),
            log_file: None,
            id_gen: 0,
            last_written: 0,
            opt_init: false,
            extra_debug: false,
            file_day: None,
            file_size: 0,
            device: false,
        };
        let now = current_unix_time();
        rotate_if_due(&mut inner, c"/dev/null", now, 0, 10, 7, None);
        assert!(inner.device, "/dev/null is a device");
        inner.file_size = LOG_PART_SIZE * 2;
        inner.file_day = Some(now / 86_400 - 3);
        rotate_if_due(&mut inner, c"/dev/null", now, 0, 10, 7, None);
        assert!(std::fs::metadata("/dev/null").unwrap().file_type().is_char_device());
    }
}
