//! Stateful Rust log engine (`NzbgetRsLog` and `NzbgetRsCmdScriptLog`)
//! backing `Log` (`daemon/util/Log.h` / `Log.cpp`) and `CommandScriptLog`.

use std::collections::VecDeque;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;
use std::sync::Mutex;

use crate::options::NzbgetRsOptions;
use crate::xmlrpc_format::{NzbgetRsRpcLogEntry, NzbgetRsXmlRpcEmitFn};

type BinRpcLogBeginFn = unsafe extern "C" fn(*mut c_void, c_int, u32, *mut c_int);
type BinRpcLogPushFn = unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const c_char);

const MESSAGE_PREFIXES: [&[u8]; 5] = [b"INFO", b"WARNING", b"ERROR", b"DEBUG", b"DETAIL"];

#[inline]
fn current_unix_time() -> i64 {
    unsafe { libc::time(ptr::null_mut()) as i64 }
}

fn cstring_lossless(bytes: &[u8]) -> CString {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let mut v = Vec::with_capacity(end + 1);
    v.extend_from_slice(&bytes[..end]);
    v.push(0);
    unsafe { CString::from_vec_with_nul_unchecked(v) }
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
}

impl LogInner {
    fn add_message(&mut self, kind: i32, text: &[u8], log_buffer: Option<i32>) {
        self.id_gen = self.id_gen.wrapping_add(1);
        self.messages.push_back(RsLogMessage {
            id: self.id_gen,
            kind,
            time: current_unix_time(),
            text: cstring_lossless(text),
        });
        if self.opt_init {
            if let Some(cap) = log_buffer {
                let max_len = (cap as u32) as usize;
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
    pub fn new(debug_build: bool) -> Self {
        Self::with_debug_cb(debug_build, None)
    }

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
        if let Ok(dbg) = self.debuggables.lock() {
            for &item in dbg.iter() {
                if item == 1 {
                    crate::statmeter::log_active_statmeter_debug_info();
                } else if item == 2 {
                    crate::feed::log_active_feed_debug_info();
                } else if item == 3 {
                    crate::urlcoord::log_active_url_debug_info();
                } else if let Some(cb) = self.debug_cb {
                    unsafe { cb(item) };
                }
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
        let cstr = cstring_lossless(text);
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
    if let Some(opts) = options {
        let sc = opts.scalars();
        match kind {
            0 => sc.info_target,
            1 => sc.warning_target,
            2 => sc.error_target,
            3 => sc.debug_target,
            4 => sc.detail_target,
            _ => 0,
        }
    } else if kind == 2 {
        3 // mtBoth
    } else {
        1 // mtScreen
    }
}

fn prefix_for_kind(kind: i32) -> &'static [u8] {
    MESSAGE_PREFIXES
        .get(usize::try_from(kind).unwrap_or(0))
        .copied()
        .unwrap_or(b"INFO")
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

fn filelog_locked(
    inner: &mut LogInner,
    path_mtx: &Mutex<CString>,
    prefix: &[u8],
    text: &[u8],
    debug_build: bool,
    options: Option<&NzbgetRsOptions>,
) {
    let mut log_filename = match path_mtx.lock() {
        Ok(g) => g.clone(),
        Err(_) => return,
    };
    if log_filename.to_bytes().is_empty() {
        return;
    }

    let tmp2 = format_truncated_1023(prefix, text);
    let time_corr = options.map_or(0, |o| o.scalars().time_correction) as i64;
    let write_log = options.map_or(0, |o| o.scalars().write_log);
    let rotate_log = options.map_or(0, |o| o.scalars().rotate_log);
    let log_buffer = options.map(|o| o.scalars().log_buffer);

    let rawtime = current_unix_time() + time_corr;
    let mut time_buf = [0u8; 50];
    crate::util::util_format_time(rawtime, &mut time_buf);
    let time_len = time_buf
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(time_buf.len());

    if crate::util::log_should_rotate(rawtime, inner.last_written, write_log) {
        inner.log_file = None;
        let rot_time = current_unix_time() + time_corr;
        let opt_log = options.map_or(&[][..], |o| o.str_bytes(27));
        let rotated = crate::systemhealth::log_rotate_log_file(
            opt_log,
            crate::paths::PATH_SEPARATOR as u8,
            rot_time,
            rotate_log,
            |info_msg| {
                inner.add_message(0, info_msg, log_buffer);
            },
        );
        let new_cstr = cstring_lossless(&rotated);
        if let Ok(mut pg) = path_mtx.lock() {
            *pg = new_cstr.clone();
        }
        log_filename = new_cstr;
    }

    inner.last_written = rawtime;

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
                        if let Some(user) = options.and_then(|o| o.str_cstr(26)) {
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
    unsafe {
        let _ = libc::fseek(fp, 0, libc::SEEK_END);
    }

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

    unsafe {
        let _ = libc::fwrite(line.as_ptr().cast(), 1, line.len(), fp);
        let _ = libc::fflush(fp);
    }
}

fn emit_message(
    log: &NzbgetRsLog,
    kind: i32,
    debug_build: bool,
    options: Option<&NzbgetRsOptions>,
    text_cstr: &CStr,
) {
    if kind == 3 && !debug_build {
        return;
    }
    let Ok(mut inner) = log.inner.lock() else {
        return;
    };
    if kind == 3 && options.is_none() && inner.extra_debug {
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
    emit_ctx: *mut c_void,
    emit_cb: NzbgetRsXmlRpcEmitFn,
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
    let entries: Vec<NzbgetRsRpcLogEntry> = if (start as u32) < (messages.len() as u32) {
        messages
            .iter()
            .skip(start as usize)
            .map(|m| NzbgetRsRpcLogEntry {
                id: m.id as c_int,
                kind: m.kind,
                time: m.time as c_int,
                text: m.text.as_ptr(),
            })
            .collect()
    } else {
        Vec::new()
    };
    unsafe {
        emit_cb(
            emit_ctx,
            2,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            entries.as_ptr().cast(),
            entries.len(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
        );
    }
}

struct CmdScriptLogInner {
    messages: VecDeque<RsLogMessage>,
    id_message_gen: i32,
    id_script_gen: i32,
}

pub struct NzbgetRsCmdScriptLog {
    inner: Mutex<CmdScriptLogInner>,
}

impl Default for NzbgetRsCmdScriptLog {
    fn default() -> Self {
        Self::new()
    }
}

impl NzbgetRsCmdScriptLog {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(CmdScriptLogInner {
                messages: VecDeque::new(),
                id_message_gen: 0,
                id_script_gen: 0,
            }),
        }
    }
}

static ACTIVE_LOG: std::sync::RwLock<Option<std::sync::Weak<NzbgetRsLog>>> =
    std::sync::RwLock::new(None);

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
    let cs = cstring_lossless(bytes);
    emit_message(&lg, kind, lg.debug_build, opts.as_deref(), &cs);
}

#[no_mangle]
pub extern "C" fn nzbget_rs_log_new(
    debug_build: c_int,
    debug_cb: NzbgetRsDebuggableFn,
) -> *mut NzbgetRsLog {
    let arc = std::sync::Arc::new(NzbgetRsLog::with_debug_cb(debug_build != 0, debug_cb));
    if let Ok(mut g) = ACTIVE_LOG.write() {
        *g = Some(std::sync::Arc::downgrade(&arc));
    }
    std::sync::Arc::into_raw(arc).cast_mut()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_log_free(log: *mut NzbgetRsLog) {
    if !log.is_null() {
        let arc = std::sync::Arc::from_raw(log.cast_const());
        let wk = std::sync::Arc::downgrade(&arc);
        if let Ok(mut g) = ACTIVE_LOG.write() {
            if g.as_ref()
                .is_some_and(|cur| std::sync::Weak::ptr_eq(cur, &wk))
            {
                *g = None;
            }
        }
        drop(arc);
    }
}

#[no_mangle]
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

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_log_ctl(
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
                    if let Some(lf) = o.str_cstr(27) {
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
            if let Some(lf) = opts.and_then(|o| o.str_cstr(27)) {
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

pub(crate) fn active_cmd_script_log() -> &'static NzbgetRsCmdScriptLog {
    static LOG: std::sync::OnceLock<NzbgetRsCmdScriptLog> = std::sync::OnceLock::new();
    LOG.get_or_init(NzbgetRsCmdScriptLog::new)
}

impl NzbgetRsCmdScriptLog {
    pub(crate) fn reset_all(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.messages.clear();
            inner.id_script_gen = 0;
            inner.id_message_gen = 0;
        }
    }

    pub(crate) fn reset_script(&self) -> c_int {
        let Ok(mut inner) = self.inner.lock() else {
            return 0;
        };
        inner.messages.clear();
        inner.id_script_gen = inner.id_script_gen.wrapping_add(1);
        inner.id_script_gen
    }

    pub(crate) fn add_message_bytes(&self, script_id: c_int, kind: c_int, bytes: &[u8]) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if script_id == inner.id_script_gen {
            inner.id_message_gen = inner.id_message_gen.wrapping_add(1);
            let msg_id = inner.id_message_gen as u32;
            inner.messages.push_back(RsLogMessage {
                id: msg_id,
                kind,
                time: current_unix_time(),
                text: cstring_lossless(bytes),
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn nzbget_rs_log_collect(
    mode: c_int,
    log: *const NzbgetRsLog,
    cmd_log: *const NzbgetRsCmdScriptLog,
    a1: c_int,
    a2: c_int,
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
        1 => {
            // XML/JSON-RPC log / commandscriptlog
            if fn1 == 0 {
                return;
            }
            let emit_cb: NzbgetRsXmlRpcEmitFn = std::mem::transmute(fn1);
            if let Some(clg) = cmd_log.as_ref() {
                if let Ok(inner) = clg.inner.lock() {
                    collect_messages_rpc(&inner.messages, a1, a2, ctx, emit_cb);
                }
            } else if let Some(lg) = log.as_ref() {
                if let Ok(inner) = lg.inner.lock() {
                    collect_messages_rpc(&inner.messages, a1, a2, ctx, emit_cb);
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
pub(crate) unsafe fn nzbget_rs_cmd_script_log_op(
    op: c_int,
    handle: *mut NzbgetRsCmdScriptLog,
    script_id: c_int,
    kind: c_int,
    text: *const c_char,
) -> usize {
    match op {
        0 => Box::into_raw(Box::new(NzbgetRsCmdScriptLog::new())) as usize,
        1 => {
            if !handle.is_null() {
                drop(Box::from_raw(handle));
            }
            0
        }
        2 => handle.as_ref().map_or(0, |h| h.reset_script() as usize),
        3 => {
            if let Some(h) = handle.as_ref() {
                let bytes = if text.is_null() {
                    &[][..]
                } else {
                    CStr::from_ptr(text).to_bytes()
                };
                h.add_message_bytes(script_id, kind, bytes);
            }
            0
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

        let log = nzbget_rs_log_new(0, None);
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
            c"WriteLog=reset".as_ptr(),
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

        let disk_text = std::fs::read_to_string(&log_path).unwrap();
        assert!(!disk_text.contains("stale line"));
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
                ptr::null(),
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
                ptr::null(),
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
            nzbget_rs_log_free(log);
            nzbget_rs_options_free(opts);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn command_script_log_filters_by_active_script_generation() {
        unsafe {
            let h = nzbget_rs_cmd_script_log_op(0, ptr::null_mut(), 0, 0, ptr::null())
                as *mut NzbgetRsCmdScriptLog;
            let gen1 = nzbget_rs_cmd_script_log_op(2, h, 0, 0, ptr::null()) as c_int;
            assert_eq!(gen1, 1);
            nzbget_rs_cmd_script_log_op(3, h, gen1, 0, c"line1".as_ptr());
            let gen2 = nzbget_rs_cmd_script_log_op(2, h, 0, 0, ptr::null()) as c_int;
            assert_eq!(gen2, 2);
            // Stale script_id message is ignored
            nzbget_rs_cmd_script_log_op(3, h, gen1, 2, c"stale".as_ptr());
            nzbget_rs_cmd_script_log_op(3, h, gen2, 0, c"fresh".as_ptr());

            let inner = (*h).inner.lock().unwrap();
            assert_eq!(inner.messages.len(), 1);
            assert_eq!(inner.messages[0].text.as_c_str(), c"fresh");
            drop(inner);
            nzbget_rs_cmd_script_log_op(1, h, 0, 0, ptr::null());
        }
    }
}
