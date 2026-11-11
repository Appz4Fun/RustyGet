//! Process execution, `ScriptController`, `ExtensionManager` runtime orchestration,
//! and background/foreground script controllers (`CommandScriptController`,
//! `QueueScriptController`, `PostScriptController`, `ScanScriptController`,
//! `FeedScriptController`, `SchedulerScriptController`, `UpdateScriptController`,
//! and `Maintenance::CheckUpdates` `InfoController`).

use std::ffi::{c_char, c_int, c_void, CStr};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::Duration;

use crate::extension::{ExtensionScriptView, RsExtensionManager};
use crate::ffi::{into_buf, RsBuf};

pub const MK_INFO: c_int = 0;
pub const MK_WARNING: c_int = 1;
pub const MK_ERROR: c_int = 2;
pub const MK_DEBUG: c_int = 3;
pub const MK_DETAIL: c_int = 4;

pub const FORK_ERROR_EXIT_CODE: c_int = 254;
const MAX_LINE_CHUNK: usize = 1024 * 10 - 1;
const WATCHDOG_WAIT_SECS: u64 = 60;

/// Shared C++ host callback for script controllers (`ScriptHostCb` in `Util.h`):
/// - `op = 0`: global log (`a1 = Message::EKind`, `s1 = text`)
/// - `op = 1`: returns `*const *const c_char` for `g_EnvironmentVariables` (or `0`)
/// - `op = 2`: `g_Options` access:
///   - `a1 = 0`: sets `strs[0] = g_Options->GetShellOverride().c_str()`
///   - `a1 = 1`: iterates `g_Options->GuardOptEntries()`, calling `p2(p1, name, val)`
/// - `op = 3`: spawns `ScriptJobThread` for boxed job `p1` (`SetAutoDestroy(true); Start();`)
/// - `op = 4`: `g_CommandScriptLog->AddMessage(a1 = log_id, a2 = kind, s1 = text)`
/// - `op = 5`: `QueueScript` `DownloadQueue` actions:
///   - `a1 = 0`: `nzbInfo->AddMessage(a2 = kind, s1 = text)` for `nzb_id = p1` (returns `1` if found)
///   - `a1 = 1`: apply `[NZB]` command (`p1 = nzb_id`, `a2 = cmd_kind`, `s1 = key`, `s2 = val`)
///   - `a1 = 2`: mark bad & `eaGroupDelete` (`p1 = nzb_id`, `s1 = nzb_name`)
///   - `a1 = 3`: `g_QueueScriptCoordinator->CheckQueue()`
/// - `op = 6`: query `g_ArgumentCount` / `(*g_Arguments)[a1]` into `strs[0]`
/// - `op = 7`: `ExitProc()`
pub type NzbgetRsScriptHostFn = unsafe extern "C" fn(
    ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    p2: usize,
    s1: *const c_char,
    s2: *const c_char,
    ints: *mut c_int,
    strs: *mut *const c_char,
) -> usize;

#[inline]
pub(crate) fn c_buf(bytes: &[u8]) -> Vec<u8> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let mut v = Vec::with_capacity(end + 1);
    v.extend_from_slice(&bytes[..end]);
    v.push(0);
    v
}

#[inline]
pub(crate) unsafe fn opt_bytes(p: *const c_char) -> Vec<u8> {
    if p.is_null() {
        Vec::new()
    } else {
        CStr::from_ptr(p).to_bytes().to_vec()
    }
}

#[inline]
pub(crate) unsafe fn opt_slice<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        CStr::from_ptr(p).to_bytes()
    }
}

#[inline]
fn trunc_bytes(mut v: Vec<u8>, max_len: usize) -> Vec<u8> {
    if v.len() > max_len {
        v.truncate(max_len);
    }
    v
}

#[inline]
fn current_time_sec() -> i64 {
    unsafe { libc::time(ptr::null_mut()) as i64 }
}

#[inline]
pub(crate) fn host_log(ctx: *mut c_void, host_cb: NzbgetRsScriptHostFn, kind: c_int, msg: &[u8]) {
    let c = c_buf(msg);
    unsafe {
        host_cb(
            ctx,
            0,
            kind,
            0,
            0,
            0,
            c.as_ptr().cast(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
    }
}

#[inline]
pub(crate) fn format_prefixed(prefix: Option<&[u8]>, msg: &[u8]) -> Vec<u8> {
    match prefix {
        Some(p) => {
            let mut out = Vec::with_capacity(p.len() + 2 + msg.len());
            out.extend_from_slice(p);
            out.extend_from_slice(b": ");
            out.extend_from_slice(msg);
            out
        }
        None => msg.to_vec(),
    }
}

static RUNNING_SCRIPTS: OnceLock<Mutex<Vec<Weak<ScriptRunner>>>> = OnceLock::new();

fn running_registry() -> &'static Mutex<Vec<Weak<ScriptRunner>>> {
    RUNNING_SCRIPTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn args_limit_size() -> isize {
    static LIMIT: OnceLock<isize> = OnceLock::new();
    *LIMIT.get_or_init(|| unsafe { libc::sysconf(libc::_SC_ARG_MAX) as isize })
}

#[inline]
unsafe fn raw_errno() -> c_int {
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))]
    {
        *libc::__error()
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        *libc::__errno_location()
    }
    #[cfg(target_os = "netbsd")]
    {
        *libc::__errno()
    }
}

#[inline]
unsafe fn set_child_environ(env_ptrs: *mut *mut c_char) {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn _NSGetEnviron() -> *mut *mut *mut c_char;
        }
        *_NSGetEnviron() = env_ptrs;
    }
    #[cfg(not(target_os = "macos"))]
    {
        extern "C" {
            static mut environ: *mut *mut c_char;
        }
        environ = env_ptrs;
    }
}

#[inline]
unsafe fn write_fd1(bytes: &[u8]) {
    if !bytes.is_empty() {
        let _ = libc::write(1, bytes.as_ptr().cast(), bytes.len());
    }
}

unsafe fn make_cloexec_pipe(fds: &mut [c_int; 2]) -> Result<(), c_int> {
    #[cfg(target_os = "linux")]
    {
        if libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) != 0 {
            return Err(raw_errno());
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return Err(raw_errno());
        }
        let _ = libc::fcntl(fds[0], libc::F_SETFD, libc::FD_CLOEXEC);
        let _ = libc::fcntl(fds[1], libc::F_SETFD, libc::FD_CLOEXEC);
        Ok(())
    }
}

struct PipeLineReader {
    buf: [u8; 4096],
    pos: usize,
    cap: usize,
    eof: bool,
}

impl PipeLineReader {
    fn new() -> Self {
        Self {
            buf: [0u8; 4096],
            pos: 0,
            cap: 0,
            eof: false,
        }
    }

    fn read_line(&mut self, read_fd_atom: &AtomicI32, out: &mut Vec<u8>) -> bool {
        out.clear();
        while out.len() < MAX_LINE_CHUNK {
            if self.pos >= self.cap {
                if self.eof {
                    break;
                }
                let fd = read_fd_atom.load(Ordering::Acquire);
                if fd < 0 {
                    return false;
                }
                let n = loop {
                    let cur_fd = read_fd_atom.load(Ordering::Acquire);
                    if cur_fd < 0 {
                        return false;
                    }
                    let r =
                        unsafe { libc::read(cur_fd, self.buf.as_mut_ptr().cast(), self.buf.len()) };
                    if r == -1 && unsafe { raw_errno() } == libc::EINTR {
                        continue;
                    }
                    break r;
                };
                if n <= 0 {
                    self.eof = true;
                    break;
                }
                self.pos = 0;
                self.cap = n as usize;
            }
            let avail = &self.buf[self.pos..self.cap];
            let rem = MAX_LINE_CHUNK - out.len();
            let window = &avail[..avail.len().min(rem)];
            if let Some(nl) = window.iter().position(|&b| b == b'\n') {
                out.extend_from_slice(&window[..=nl]);
                self.pos += nl + 1;
                return true;
            }
            out.extend_from_slice(window);
            self.pos += window.len();
        }
        !out.is_empty()
    }
}

struct ChildWatchDogGuard {
    state: Arc<(Mutex<bool>, Condvar)>,
    join_handle: Option<std::thread::JoinHandle<()>>,
}

impl ChildWatchDogGuard {
    fn start(
        pid: c_int,
        info_name: Vec<u8>,
        ctx: *mut c_void,
        host_cb: NzbgetRsScriptHostFn,
    ) -> Self {
        let state = Arc::new((Mutex::new(false), Condvar::new()));
        let state_bg = Arc::clone(&state);
        let ctx_addr = ctx as usize;
        let handle = std::thread::spawn(move || {
            let (lock, cvar) = &*state_bg;
            let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            let (guard, _) = cvar
                .wait_timeout_while(guard, Duration::from_secs(WATCHDOG_WAIT_SECS), |stopped| {
                    !*stopped
                })
                .unwrap_or_else(|e| e.into_inner());
            if !*guard {
                let mut msg = b"Restarting hanging child process for ".to_vec();
                msg.extend_from_slice(&info_name);
                host_log(ctx_addr as *mut c_void, host_cb, MK_INFO, &msg);
                unsafe {
                    let _ = libc::kill(pid, libc::SIGKILL);
                }
            }
        });
        Self {
            state,
            join_handle: Some(handle),
        }
    }

    fn stop(&self) {
        let (lock, cvar) = &*self.state;
        let mut g = lock.lock().unwrap_or_else(|e| e.into_inner());
        *g = true;
        cvar.notify_all();
    }

    fn stop_and_join(&mut self) {
        self.stop();
        if let Some(h) = self.join_handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for ChildWatchDogGuard {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

/// Environment and argument builder matching `EnvironmentStrings` + `NzbScriptController`.
#[derive(Default)]
pub struct ScriptExecEnv {
    pub args: Vec<Vec<u8>>,
    pub log_prefix: Option<Vec<u8>>,
    pub env_strings: Vec<Vec<u8>>,
    pub modified_options: Vec<(Vec<u8>, Vec<u8>)>,
}

impl ScriptExecEnv {
    pub fn new(ctx: *mut c_void, host_cb: NzbgetRsScriptHostFn) -> Self {
        let mut s = Self::default();
        s.reset_env(ctx, host_cb);
        s
    }

    pub fn reset_env(&mut self, ctx: *mut c_void, host_cb: NzbgetRsScriptHostFn) {
        self.env_strings.clear();
        let env_arr = unsafe {
            host_cb(
                ctx,
                1,
                0,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
            ) as *const *const c_char
        };
        if env_arr.is_null() {
            return;
        }
        let mut idx = 0usize;
        unsafe {
            loop {
                let p = *env_arr.add(idx);
                if p.is_null() {
                    break;
                }
                let bytes = CStr::from_ptr(p).to_bytes();
                if !crate::extension::is_nzbget_internal_env_var(bytes) {
                    self.env_strings.push(bytes.to_vec());
                }
                idx += 1;
            }
        }
    }

    pub fn set_env_var(&mut self, name: &[u8], value: &[u8]) {
        let mut entry = Vec::with_capacity(name.len() + 1 + value.len());
        entry.extend_from_slice(name);
        entry.push(b'=');
        entry.extend_from_slice(value);
        self.env_strings.push(entry);
    }

    pub fn set_int_env_var(&mut self, name: &[u8], value: i32) {
        let s = value.to_string();
        self.set_env_var(name, s.as_bytes());
    }

    pub fn set_env_var_special(&mut self, prefix: &[u8], name: &[u8], value: &[u8]) {
        let mut varname = Vec::with_capacity(prefix.len() + 1 + name.len());
        varname.extend_from_slice(prefix);
        varname.push(b'_');
        varname.extend_from_slice(name);
        let varname = trunc_bytes(varname, 1023);
        self.set_env_var(&varname, value);
        let norm = crate::extension::normalize_special_env_var_name(&varname);
        if norm.as_slice() != varname.as_slice() {
            self.set_env_var(&norm, value);
        }
    }

    fn resolve_opt_value<'a>(&'a self, name: &[u8], default_val: &'a [u8]) -> &'a [u8] {
        for (k, v) in &self.modified_options {
            if k.eq_ignore_ascii_case(name) {
                return v.as_slice();
            }
        }
        default_val
    }

    pub fn prepare_env_options(
        &mut self,
        strip_prefix: Option<&[u8]>,
        ctx: *mut c_void,
        host_cb: NzbgetRsScriptHostFn,
    ) {
        let mut raw_opts: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        extern "C" fn push_opt(vec_ptr: usize, name: *const c_char, val: *const c_char) {
            let v = unsafe { &mut *(vec_ptr as *mut Vec<(Vec<u8>, Vec<u8>)>) };
            v.push(unsafe { (opt_bytes(name), opt_bytes(val)) });
        }
        unsafe {
            host_cb(
                ctx,
                2,
                1,
                0,
                (&mut raw_opts as *mut Vec<(Vec<u8>, Vec<u8>)>) as usize,
                push_opt as *const () as usize,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
        for (name, def_val) in &raw_opts {
            let val = self.resolve_opt_value(name, def_val).to_vec();
            if let Some(sp) = strip_prefix {
                if name.len() > sp.len() && name.starts_with(sp) {
                    self.set_env_var_special(b"NZBPO", &name[sp.len()..], &val);
                }
            } else {
                self.set_env_var_special(b"NZBOP", name, &val);
            }
        }
    }

    pub fn prepare_env_script(
        &mut self,
        parameters: Option<&[(Vec<u8>, Vec<u8>)]>,
        script_name: &[u8],
        ctx: *mut c_void,
        host_cb: NzbgetRsScriptHostFn,
    ) {
        let has_params = parameters.is_some();
        let mut name_bufs: Vec<Vec<u8>> = Vec::new();
        let mut val_bufs: Vec<Vec<u8>> = Vec::new();
        if let Some(params) = parameters {
            name_bufs.reserve(params.len());
            val_bufs.reserve(params.len());
            for (k, v) in params {
                name_bufs.push(c_buf(k));
                val_bufs.push(c_buf(v));
            }
        }
        let name_cstrs: Vec<&CStr> = name_bufs
            .iter()
            .map(|b| CStr::from_bytes_with_nul(b).unwrap_or(c""))
            .collect();
        let val_cstrs: Vec<&CStr> = val_bufs
            .iter()
            .map(|b| CStr::from_bytes_with_nul(b).unwrap_or(c""))
            .collect();

        let cell = std::cell::RefCell::new(&mut *self);
        crate::extension::nzb_script_prepare_env(
            script_name,
            has_params,
            &name_cstrs,
            &val_cstrs,
            |prefix, name, val| {
                cell.borrow_mut().set_env_var_special(
                    prefix.to_bytes(),
                    name.to_bytes(),
                    val.to_bytes(),
                );
            },
            |strip| {
                cell.borrow_mut()
                    .prepare_env_options(Some(strip.to_bytes()), ctx, host_cb);
            },
        );
    }
}

/// Thread-safe POSIX child process controller (`ScriptController` in `ScriptController.cpp`).
pub struct ScriptRunner {
    terminated: AtomicBool,
    completed: AtomicBool,
    detached: AtomicBool,
    process_id: AtomicI32,
    read_fd: AtomicI32,
    registered: AtomicBool,
    info_name: Mutex<Vec<u8>>,
}

impl ScriptRunner {
    pub fn new(register: bool) -> Arc<Self> {
        let runner = Arc::new(Self {
            terminated: AtomicBool::new(false),
            completed: AtomicBool::new(false),
            detached: AtomicBool::new(false),
            process_id: AtomicI32::new(0),
            read_fd: AtomicI32::new(-1),
            registered: AtomicBool::new(register),
            info_name: Mutex::new(Vec::new()),
        });
        if register {
            let mut reg = running_registry().lock().unwrap_or_else(|e| e.into_inner());
            reg.retain(|w| w.strong_count() > 0);
            reg.push(Arc::downgrade(&runner));
        }
        runner
    }

    pub fn unregister_running(&self) {
        if self.registered.swap(false, Ordering::AcqRel) {
            let self_ptr = self as *const Self;
            let mut reg = running_registry().lock().unwrap_or_else(|e| e.into_inner());
            reg.retain(|w| w.strong_count() > 0 && !ptr::eq(w.as_ptr(), self_ptr));
        }
    }

    pub fn set_info_name(&self, name: &[u8]) {
        let mut g = self.info_name.lock().unwrap_or_else(|e| e.into_inner());
        *g = name.to_vec();
    }

    pub fn get_info_name(&self) -> Vec<u8> {
        self.info_name
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn has_active_process(&self) -> bool {
        self.process_id.load(Ordering::Acquire) > 0 && !self.completed.load(Ordering::Acquire)
    }

    pub fn terminate(&self, ctx: *mut c_void, host_cb: NzbgetRsScriptHostFn) {
        let info = self.get_info_name();
        host_log(
            ctx,
            host_cb,
            MK_DEBUG,
            &[b"Stopping ", info.as_slice()].concat(),
        );
        self.terminated.store(true, Ordering::Release);

        let child_pid = self.process_id.load(Ordering::Acquire);
        let kill_id = if child_pid > 0 && unsafe { libc::getpgid(child_pid) } == child_pid {
            -child_pid
        } else {
            child_pid
        };
        let kill_res = if child_pid > 0 {
            unsafe { libc::kill(kill_id, libc::SIGKILL) }
        } else {
            0
        };
        let ok = child_pid <= 0 || kill_res == 0 || self.completed.load(Ordering::Acquire);
        if ok {
            host_log(
                ctx,
                host_cb,
                MK_DEBUG,
                &[b"Terminated ", info.as_slice()].concat(),
            );
        } else {
            host_log(
                ctx,
                host_cb,
                MK_ERROR,
                &[b"Could not terminate ", info.as_slice()].concat(),
            );
        }
        host_log(
            ctx,
            host_cb,
            MK_DEBUG,
            &[b"Stopped ", info.as_slice()].concat(),
        );
    }

    pub fn send_break(&self, ctx: *mut c_void, host_cb: NzbgetRsScriptHostFn) -> bool {
        let info = self.get_info_name();
        host_log(
            ctx,
            host_cb,
            MK_DEBUG,
            &[b"Sending break signal to ", info.as_slice()].concat(),
        );
        let pid = self.process_id.load(Ordering::Acquire);
        let ok = pid > 0
            && !self.completed.load(Ordering::Acquire)
            && unsafe { libc::kill(pid, libc::SIGINT) } == 0;
        if ok {
            host_log(
                ctx,
                host_cb,
                MK_DEBUG,
                &[b"Sent break signal to ", info.as_slice()].concat(),
            );
        } else {
            host_log(
                ctx,
                host_cb,
                MK_WARNING,
                &[b"Could not send break signal to ", info.as_slice()].concat(),
            );
        }
        ok
    }

    pub fn detach(&self, ctx: *mut c_void, host_cb: NzbgetRsScriptHostFn) {
        let info = self.get_info_name();
        host_log(
            ctx,
            host_cb,
            MK_DEBUG,
            &[b"Detaching ", info.as_slice()].concat(),
        );
        self.detached.store(true, Ordering::Release);
        let fd = self.read_fd.swap(-1, Ordering::AcqRel);
        if fd >= 0 {
            unsafe {
                let _ = libc::close(fd);
            }
        }
    }

    pub fn terminate_all(ctx: *mut c_void, host_cb: NzbgetRsScriptHostFn) {
        let active: Vec<Arc<ScriptRunner>> = {
            let mut reg = running_registry().lock().unwrap_or_else(|e| e.into_inner());
            let mut out = Vec::new();
            reg.retain(|w| match w.upgrade() {
                Some(arc) => {
                    out.push(arc);
                    true
                }
                None => false,
            });
            out
        };
        for script in active {
            if script.has_active_process() && !script.detached.load(Ordering::Acquire) {
                if script.send_break(ctx, host_cb) {
                    let cur = current_time_sec();
                    while !script.completed.load(Ordering::Acquire)
                        && (cur - current_time_sec()).abs() <= 10
                    {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
                script.terminate(ctx, host_cb);
            }
        }
    }

    fn check_env_size(
        env_c_bufs: &[Vec<u8>],
        log_prefix: Option<&[u8]>,
        add_msg: &mut dyn FnMut(c_int, &[u8]),
    ) {
        let limit_raw = args_limit_size();
        if limit_raw <= 0 {
            return;
        }
        let limit = limit_raw as usize;
        let ptr_sz = std::mem::size_of::<*mut c_char>();
        let mut total = 0usize;
        for buf in env_c_bufs {
            // `buf` includes the trailing NUL byte (`strlen(var) + 1`)
            total += buf.len() + ptr_sz;
            if total > limit {
                let msg = format!(
                    "Total environment size ({total} bytes) exceeds OS limit ({limit} bytes): This may cause process creation to fail"
                );
                let formatted = format_prefixed(log_prefix, msg.as_bytes());
                add_msg(MK_WARNING, &formatted);
                break;
            }
        }
    }

    fn wait_process(
        &self,
        log_prefix: Option<&[u8]>,
        add_msg: &mut dyn FnMut(c_int, &[u8]),
    ) -> c_int {
        let pid = self.process_id.load(Ordering::Acquire);
        if pid <= 0 {
            return -1;
        }
        let mut status: c_int = 0;
        let res = loop {
            let r = unsafe { libc::waitpid(pid, &mut status, 0) };
            if r == -1 && unsafe { raw_errno() } == libc::EINTR {
                continue;
            }
            break r;
        };
        if res == -1 {
            return -1;
        }
        if libc::WIFEXITED(status) {
            return libc::WEXITSTATUS(status);
        }
        if libc::WIFSIGNALED(status) {
            let sig = libc::WTERMSIG(status);
            if !self.terminated.load(Ordering::Acquire) {
                let info = self.get_info_name();
                let mut m = info;
                m.extend_from_slice(format!(" was killed by signal {sig}").as_bytes());
                let formatted = format_prefixed(log_prefix, &m);
                add_msg(MK_WARNING, &formatted);
            }
            return 128 + sig;
        }
        -1
    }

    pub fn execute(
        &self,
        cfg: &mut ScriptExecEnv,
        ctx: *mut c_void,
        host_cb: NzbgetRsScriptHostFn,
        add_msg: &mut dyn FnMut(c_int, &[u8]),
    ) -> c_int {
        cfg.prepare_env_options(None, ctx, host_cb);

        // PrepareArgs
        let mut strs = [ptr::null() as *const c_char; 1];
        unsafe {
            host_cb(
                ctx,
                2,
                0,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                strs.as_mut_ptr(),
            );
        }
        let shell_override = unsafe { opt_bytes(strs[0]) };

        let mut cmd_args_prefix: Vec<Vec<u8>> = Vec::new();
        let cmd_line: Vec<u8> = if cfg.args.len() == 1 {
            let arg0 = &cfg.args[0];
            let found = crate::util::util_find_executor_program(arg0, &shell_override);
            let chosen = match &found {
                Some(f) => f.as_slice(),
                None => arg0.as_slice(),
            };
            let cl = trunc_bytes(chosen.to_vec(), 2047);
            if let Some(f) = found {
                if f.as_slice() != arg0.as_slice() {
                    cmd_args_prefix.push(f);
                }
            }
            host_log(
                ctx,
                host_cb,
                MK_DEBUG,
                &[b"CmdLine: ", cl.as_slice()].concat(),
            );
            cl
        } else {
            trunc_bytes(cfg.args.first().map_or_else(Vec::new, Clone::clone), 2047)
        };

        self.completed.store(false, Ordering::Release);
        self.process_id.store(0, Ordering::Release);
        if self.terminated.load(Ordering::Acquire) {
            self.completed.store(true, Ordering::Release);
            return -1;
        }

        let mut exit_code: c_int = 0;
        let mut child_confirmed = false;

        while !child_confirmed && !self.terminated.load(Ordering::Acquire) {
            let working_dir = crate::fsops::get_current_directory();
            let script_arg0 = cfg.args.first().map_or(&[][..], Vec::as_slice);

            let mut pin = [-1, -1];
            if let Err(e) = unsafe { make_cloexec_pipe(&mut pin) } {
                let msg = format!("Could not open read pipe: errno {e}");
                let formatted = format_prefixed(cfg.log_prefix.as_deref(), msg.as_bytes());
                add_msg(MK_ERROR, &formatted);
                self.completed.store(true, Ordering::Release);
                return -1;
            }

            let mut env_c_bufs: Vec<Vec<u8>> = cfg.env_strings.iter().map(|s| c_buf(s)).collect();
            Self::check_env_size(&env_c_bufs, cfg.log_prefix.as_deref(), add_msg);
            let mut env_ptrs: Vec<*mut c_char> = env_c_bufs
                .iter_mut()
                .map(|b| b.as_mut_ptr().cast::<c_char>())
                .collect();
            env_ptrs.push(ptr::null_mut());

            let mut arg_c_bufs: Vec<Vec<u8>> =
                Vec::with_capacity(cmd_args_prefix.len() + cfg.args.len());
            for a in &cmd_args_prefix {
                arg_c_bufs.push(c_buf(a));
            }
            for a in &cfg.args {
                arg_c_bufs.push(c_buf(a));
            }
            let mut arg_ptrs: Vec<*mut c_char> = arg_c_bufs
                .iter_mut()
                .map(|b| b.as_mut_ptr().cast::<c_char>())
                .collect();
            arg_ptrs.push(ptr::null_mut());

            let c_working_dir = c_buf(&working_dir);
            let c_cmd_line = c_buf(&cmd_line);
            let c_script = c_buf(script_arg0);
            let script_no_nul = &c_script[..c_script.len().saturating_sub(1)];

            host_log(ctx, host_cb, MK_DEBUG, b"forking");
            let pid = unsafe { libc::fork() };
            if pid == -1 {
                let err = unsafe { raw_errno() };
                let info = self.get_info_name();
                let mut msg = b"Could not start ".to_vec();
                msg.extend_from_slice(&info);
                msg.extend_from_slice(format!(": errno {err}").as_bytes());
                let formatted = format_prefixed(cfg.log_prefix.as_deref(), &msg);
                add_msg(MK_ERROR, &formatted);
                unsafe {
                    let _ = libc::close(pin[0]);
                    let _ = libc::close(pin[1]);
                }
                self.completed.store(true, Ordering::Release);
                return -1;
            } else if pid == 0 {
                // Child process: strictly async-signal-safe syscalls only (zero allocations/locks).
                unsafe {
                    let _ = libc::setsid();
                    let _ = libc::dup2(pin[1], 1);
                    let _ = libc::dup2(pin[1], 2);
                    let _ = libc::close(pin[0]);
                    let _ = libc::close(pin[1]);

                    write_fd1(b"\n");
                    let _ = libc::fsync(1);

                    if c_working_dir.len() > 1 {
                        let _ = libc::chdir(c_working_dir.as_ptr().cast());
                    }
                    set_child_environ(env_ptrs.as_mut_ptr());

                    libc::execvp(c_cmd_line.as_ptr().cast(), arg_ptrs.as_ptr().cast());
                    let mut err_no = raw_errno();

                    if err_no == libc::EACCES {
                        write_fd1(b"[WARNING] Fixing permissions for");
                        write_fd1(script_no_nul);
                        write_fd1(b"\n");
                        let _ = libc::fsync(1);
                        let mut st: libc::stat = std::mem::zeroed();
                        if libc::stat(c_script.as_ptr().cast(), &mut st) == 0 {
                            let new_mode =
                                st.st_mode | (libc::S_IXUSR | libc::S_IXGRP | libc::S_IXOTH);
                            let _ = libc::chmod(c_script.as_ptr().cast(), new_mode);
                        }
                        libc::execvp(c_cmd_line.as_ptr().cast(), arg_ptrs.as_ptr().cast());
                        err_no = raw_errno();
                    }

                    write_fd1(b"[ERROR] Could not start ");
                    write_fd1(script_no_nul);
                    write_fd1(b": ");
                    let mut err_buf = [0 as c_char; 256];
                    if libc::strerror_r(err_no, err_buf.as_mut_ptr(), err_buf.len()) == 0 {
                        let len = libc::strlen(err_buf.as_ptr());
                        let _ = libc::write(1, err_buf.as_ptr().cast(), len);
                    } else {
                        let p = libc::strerror(err_no);
                        if !p.is_null() {
                            let len = libc::strlen(p);
                            let _ = libc::write(1, p.cast(), len);
                        }
                    }
                    write_fd1(b"\n");
                    let _ = libc::fsync(1);
                    libc::_exit(FORK_ERROR_EXIT_CODE);
                }
            }

            host_log(ctx, host_cb, MK_DEBUG, b"forked");
            host_log(
                ctx,
                host_cb,
                MK_DEBUG,
                format!("Child Process-ID: {pid}").as_bytes(),
            );

            self.process_id.store(pid, Ordering::Release);
            unsafe {
                let _ = libc::close(pin[1]);
            }
            self.read_fd.store(pin[0], Ordering::Release);

            host_log(ctx, host_cb, MK_DEBUG, b"Creating child watchdog");
            let mut watchdog = ChildWatchDogGuard::start(pid, self.get_info_name(), ctx, host_cb);

            let mut reader = PipeLineReader::new();
            let mut line_buf = Vec::with_capacity(512);

            host_log(ctx, host_cb, MK_DEBUG, b"Entering pipe-loop");
            let mut first_line = true;
            let mut start_error = false;
            while !self.terminated.load(Ordering::Acquire) && !self.detached.load(Ordering::Acquire)
            {
                if self.read_fd.load(Ordering::Acquire) < 0 {
                    break;
                }
                if !reader.read_line(&self.read_fd, &mut line_buf) {
                    break;
                }
                if self.read_fd.load(Ordering::Acquire) < 0 {
                    break;
                }
                if !child_confirmed {
                    child_confirmed = true;
                    watchdog.stop();
                    host_log(ctx, host_cb, MK_DEBUG, b"Child confirmed");
                    continue;
                }
                let nul_end = line_buf
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(line_buf.len());
                let line = &line_buf[..nul_end];
                if first_line && line.starts_with(b"[ERROR] Could not start ") {
                    start_error = true;
                }
                self.process_output_line(line, cfg.log_prefix.as_deref(), ctx, host_cb, add_msg);
                first_line = false;
            }
            host_log(ctx, host_cb, MK_DEBUG, b"Exited pipe-loop");

            host_log(ctx, host_cb, MK_DEBUG, b"Destroying WatchDog");
            watchdog.stop_and_join();

            let fd = self.read_fd.swap(-1, Ordering::AcqRel);
            if fd >= 0 {
                unsafe {
                    let _ = libc::close(fd);
                }
            }

            if self.terminated.load(Ordering::Acquire) {
                let info = self.get_info_name();
                if !info.is_empty() {
                    host_log(
                        ctx,
                        host_cb,
                        MK_WARNING,
                        &[b"Interrupted ", info.as_slice()].concat(),
                    );
                }
            }

            exit_code = 0;
            if !self.detached.load(Ordering::Acquire) {
                exit_code = self.wait_process(cfg.log_prefix.as_deref(), add_msg);
                if exit_code == FORK_ERROR_EXIT_CODE && start_error {
                    exit_code = -1;
                }
            }
        }

        host_log(
            ctx,
            host_cb,
            MK_DEBUG,
            format!("Exit code {exit_code}").as_bytes(),
        );
        self.completed.store(true, Ordering::Release);
        self.process_id.store(0, Ordering::Release);
        exit_code
    }

    fn process_output_line(
        &self,
        line: &[u8],
        log_prefix: Option<&[u8]>,
        ctx: *mut c_void,
        host_cb: NzbgetRsScriptHostFn,
        add_msg: &mut dyn FnMut(c_int, &[u8]),
    ) {
        host_log(
            ctx,
            host_cb,
            MK_DEBUG,
            b"Processing output received from script",
        );
        let (trimmed_len, mk, text_off) = crate::extension::classify_script_output_line(line);
        if trimmed_len == 0 {
            return;
        }
        let kind = match mk {
            0 => MK_INFO,
            1 => MK_WARNING,
            2 => MK_ERROR,
            3 => MK_DETAIL,
            _ => MK_DEBUG,
        };
        let body = &line[text_off.min(trimmed_len)..trimmed_len.min(line.len())];
        let formatted = format_prefixed(log_prefix, body);
        add_msg(kind, &formatted);
        host_log(
            ctx,
            host_cb,
            MK_DEBUG,
            b"Processing output received from script - completed",
        );
    }
}

impl Drop for ScriptRunner {
    fn drop(&mut self) {
        self.unregister_running();
        let fd = self.read_fd.swap(-1, Ordering::AcqRel);
        if fd >= 0 {
            unsafe {
                let _ = libc::close(fd);
            }
        }
    }
}

/// Opaque C handle wrapping `Arc<ScriptRunner>` (used by `PostScriptController`).
pub struct NzbgetRsScriptRunner {
    pub(crate) inner: Arc<ScriptRunner>,
}

/// Background job dispatched through C++ `ScriptJobThread` so `Thread::GetThreadCount()`
/// tracks active background scripts accurately through daemon shutdown.
pub enum BackgroundScriptJob {
    Command {
        ext_mgr: *const RsExtensionManager,
        script_name: Vec<u8>,
        command: Vec<u8>,
        log_id: c_int,
        modified_options: Vec<(Vec<u8>, Vec<u8>)>,
    },
    Queue {
        script: Arc<ExtensionScriptView>,
        event: c_int,
        nzb_id: c_int,
        nzb_name: Vec<u8>,
        nzb_filename: Vec<u8>,
        url: Vec<u8>,
        category: Vec<u8>,
        dest_dir: Vec<u8>,
        queued_filename: Vec<u8>,
        priority: c_int,
        dupe_key: Vec<u8>,
        dupe_mode: c_int,
        dupe_score: c_int,
        delete_status: c_int,
        url_status: c_int,
        mark_status: c_int,
        parameters: Vec<(Vec<u8>, Vec<u8>)>,
    },
    Scheduler {
        ext_mgr: *const RsExtensionManager,
        external_process: bool,
        task_id: c_int,
        script_param: Vec<u8>,
        words: Vec<Vec<u8>>,
    },
    MaintenanceUpdate {
        maint: *const crate::extension::NzbgetRsMaintenance,
        runner: Arc<ScriptRunner>,
        script_path: Vec<u8>,
        branch: c_int,
        daemon_mode: bool,
    },
}

pub(crate) fn spawn_background_job(
    job: BackgroundScriptJob,
    ctx: *mut c_void,
    host_cb: NzbgetRsScriptHostFn,
) {
    let job_ptr = Box::into_raw(Box::new(job)) as usize;
    unsafe {
        host_cb(
            ctx,
            3,
            0,
            0,
            job_ptr,
            0,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
    }
}

fn run_command_job(
    ext_mgr_ptr: *const RsExtensionManager,
    script_name: Vec<u8>,
    command: Vec<u8>,
    log_id: c_int,
    modified_options: Vec<(Vec<u8>, Vec<u8>)>,
    ctx: *mut c_void,
    host_cb: NzbgetRsScriptHostFn,
) {
    let Some(ext_mgr) = (unsafe { ext_mgr_ptr.as_ref() }) else {
        return;
    };
    let runner = ScriptRunner::new(true);
    let mut cfg = ScriptExecEnv::new(ctx, host_cb);
    cfg.modified_options = modified_options;

    let mut add_msg = |kind: c_int, text: &[u8]| {
        host_log(ctx, host_cb, kind, text);
        let c_txt = c_buf(text);
        unsafe {
            host_cb(
                ctx,
                4,
                log_id,
                kind,
                0,
                0,
                c_txt.as_ptr().cast(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    };

    ext_mgr.execute_script_list(&script_name, |ext| {
        let banner = [
            b"Executing script ",
            ext.name.as_slice(),
            b" with command ",
            command.as_slice(),
        ]
        .concat();
        let formatted_banner = format_prefixed(cfg.log_prefix.as_deref(), &banner);
        add_msg(MK_INFO, &formatted_banner);

        cfg.args = vec![ext.entry.clone()];
        let mut info_name = trunc_bytes(
            [
                b"script ",
                ext.name.as_slice(),
                b" with command ",
                command.as_slice(),
            ]
            .concat(),
            1023,
        );
        runner.set_info_name(&info_name);
        cfg.log_prefix = Some(ext.display_name.clone());

        cfg.reset_env(ctx, host_cb);
        cfg.set_env_var(b"NZBCP_COMMAND", &command);
        cfg.prepare_env_script(None, &ext.name, ctx, host_cb);

        let exit_code = runner.execute(&mut cfg, ctx, host_cb, &mut add_msg);

        if let Some(first) = info_name.first_mut() {
            *first = b'S';
        }
        cfg.log_prefix = None;

        match exit_code {
            93 => add_msg(MK_INFO, &[info_name.as_slice(), b" successful"].concat()),
            94 | -1 => add_msg(MK_ERROR, &[info_name.as_slice(), b" failed"].concat()),
            _ => add_msg(
                MK_ERROR,
                &[
                    info_name.as_slice(),
                    b" failed (terminated with unknown status)",
                ]
                .concat(),
            ),
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn run_queue_job(
    script: Arc<ExtensionScriptView>,
    event: c_int,
    nzb_id: c_int,
    nzb_name: Vec<u8>,
    nzb_filename: Vec<u8>,
    url: Vec<u8>,
    category: Vec<u8>,
    dest_dir: Vec<u8>,
    queued_filename: Vec<u8>,
    priority: c_int,
    dupe_key: Vec<u8>,
    dupe_mode: c_int,
    dupe_score: c_int,
    delete_status: c_int,
    url_status: c_int,
    mark_status: c_int,
    parameters: Vec<(Vec<u8>, Vec<u8>)>,
    ctx: *mut c_void,
    host_cb: NzbgetRsScriptHostFn,
) {
    let runner = ScriptRunner::new(true);
    let mut cfg = ScriptExecEnv::new(ctx, host_cb);
    let mut mark_bad = false;

    let base_nzb = &nzb_name[crate::paths::base_file_name(&nzb_name)..];
    let exec_msg = [
        b"Executing queue-script ",
        script.name.as_slice(),
        b" for ",
        base_nzb,
    ]
    .concat();
    let info_name = trunc_bytes(
        [b"queue-script ", script.name.as_slice(), b" for ", base_nzb].concat(),
        1023,
    );

    let send_nzb_or_global = |kind: c_int, text: &[u8]| {
        let c_txt = c_buf(text);
        let handled = unsafe {
            host_cb(
                ctx,
                5,
                0,
                kind,
                nzb_id as usize,
                0,
                c_txt.as_ptr().cast(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if handled == 0 {
            host_log(ctx, host_cb, kind, text);
        }
    };

    send_nzb_or_global(if event == 0 { MK_DETAIL } else { MK_INFO }, &exec_msg);
    runner.set_info_name(&info_name);

    let c_nzb_name = c_buf(&nzb_name);
    let c_filename = c_buf(&nzb_filename);
    let c_dest = c_buf(&dest_dir);
    let c_queued = c_buf(&queued_filename);
    let c_url = c_buf(&url);
    let c_cat = c_buf(&category);
    let c_dkey = c_buf(&dupe_key);

    let inp = crate::extension::QueueScriptEnvInput {
        nzb_name: CStr::from_bytes_with_nul(&c_nzb_name).unwrap_or(c""),
        nzb_id,
        filename: CStr::from_bytes_with_nul(&c_filename).unwrap_or(c""),
        dest_dir: CStr::from_bytes_with_nul(&c_dest).unwrap_or(c""),
        queued_filename: CStr::from_bytes_with_nul(&c_queued).unwrap_or(c""),
        url: CStr::from_bytes_with_nul(&c_url).unwrap_or(c""),
        category: CStr::from_bytes_with_nul(&c_cat).unwrap_or(c""),
        priority,
        dupe_key: CStr::from_bytes_with_nul(&c_dkey).unwrap_or(c""),
        dupe_score,
        dupe_mode,
        event,
        delete_status,
        url_status,
        mark_status,
    };
    let cfg_cell = std::cell::RefCell::new(&mut cfg);
    crate::extension::queue_script_prepare_env(
        &inp,
        |k, v| {
            cfg_cell
                .borrow_mut()
                .set_env_var(k.to_bytes(), v.to_bytes())
        },
        |k, iv| cfg_cell.borrow_mut().set_int_env_var(k.to_bytes(), iv),
    );

    cfg.args = vec![script.entry.clone()];
    cfg.log_prefix = Some(script.display_name.clone());
    let prefix_len = script.display_name.len() + 2;
    cfg.prepare_env_script(Some(&parameters), &script.name, ctx, host_cb);

    {
        let mut add_msg = |kind: c_int, text: &[u8]| {
            let msg_text = &text[prefix_len.min(text.len())..];
            let cmd = crate::extension::parse_queue_script_cmd(msg_text, event, 5);
            if cmd.kind != 0 {
                if msg_text.len() >= 6 {
                    host_log(
                        ctx,
                        host_cb,
                        MK_DEBUG,
                        &[b"Command ", &msg_text[6..], b" detected"].concat(),
                    );
                }
                if cmd.kind > 0 {
                    if cmd.kind == 3 {
                        mark_bad = true;
                    }
                    let key_c = if cmd.kind == 1 {
                        let end = (cmd.key_offset + cmd.key_len).min(msg_text.len());
                        c_buf(&msg_text[cmd.key_offset.min(end)..end])
                    } else {
                        vec![0]
                    };
                    let val_c = if cmd.kind == 1 || cmd.kind == 2 {
                        c_buf(&msg_text[cmd.val_offset.min(msg_text.len())..])
                    } else {
                        c_buf(&[b"Marking ", nzb_name.as_slice(), b" as bad"].concat())
                    };
                    unsafe {
                        host_cb(
                            ctx,
                            5,
                            1,
                            cmd.kind,
                            nzb_id as usize,
                            0,
                            key_c.as_ptr().cast(),
                            val_c.as_ptr().cast(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                    }
                } else {
                    let cur_info = runner.get_info_name();
                    host_log(
                        ctx,
                        host_cb,
                        MK_ERROR,
                        &[
                            b"Invalid command \"",
                            msg_text,
                            b"\" received from ",
                            cur_info.as_slice(),
                        ]
                        .concat(),
                    );
                }
                return;
            }
            send_nzb_or_global(kind, text);
        };
        runner.execute(&mut cfg, ctx, host_cb, &mut add_msg);
    }

    cfg.log_prefix = None;
    if mark_bad {
        let c_name = c_buf(&nzb_name);
        unsafe {
            host_cb(
                ctx,
                5,
                2,
                0,
                nzb_id as usize,
                0,
                c_name.as_ptr().cast(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        }
    }
    unsafe {
        host_cb(
            ctx,
            5,
            3,
            0,
            0,
            0,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
    }
}

fn run_scheduler_job(
    ext_mgr_ptr: *const RsExtensionManager,
    external_process: bool,
    task_id: c_int,
    script_param: Vec<u8>,
    words: Vec<Vec<u8>>,
    ctx: *mut c_void,
    host_cb: NzbgetRsScriptHostFn,
) {
    let runner = ScriptRunner::new(true);
    let mut cfg = ScriptExecEnv::new(ctx, host_cb);
    let mut add_msg = |kind: c_int, text: &[u8]| host_log(ctx, host_cb, kind, text);

    if external_process {
        cfg.args = words;
        let script0 = cfg.args.first().map_or(&[][..], Vec::as_slice);
        let (exec_msg, info_name, log_prefix) =
            crate::scheduler::format_scheduler_script_labels(true, task_id, script0, b"");
        host_log(ctx, host_cb, MK_INFO, &exec_msg);
        runner.set_info_name(&info_name);
        cfg.log_prefix = Some(log_prefix);
        runner.execute(&mut cfg, ctx, host_cb, &mut add_msg);
    } else if let Some(ext_mgr) = unsafe { ext_mgr_ptr.as_ref() } {
        ext_mgr.execute_script_list(&script_param, |ext| {
            if !ext.scheduler_script {
                return;
            }
            let (exec_msg, info_name, log_prefix) =
                crate::scheduler::format_scheduler_script_labels(
                    false,
                    task_id,
                    &ext.name,
                    &ext.display_name,
                );
            add_msg(MK_INFO, &exec_msg);
            cfg.args = vec![ext.entry.clone()];
            runner.set_info_name(&info_name);
            cfg.log_prefix = Some(log_prefix);
            cfg.reset_env(ctx, host_cb);
            cfg.set_int_env_var(b"NZBSP_TASKID", task_id);
            cfg.prepare_env_script(None, &ext.name, ctx, host_cb);
            runner.execute(&mut cfg, ctx, host_cb, &mut add_msg);
            cfg.log_prefix = None;
        });
    }
}

fn run_maintenance_update_job(
    maint_ptr: *const crate::extension::NzbgetRsMaintenance,
    runner: Arc<ScriptRunner>,
    script_path: Vec<u8>,
    branch: c_int,
    daemon_mode: bool,
    ctx: *mut c_void,
    host_cb: NzbgetRsScriptHostFn,
) {
    runner.unregister_running();
    let Some(maint) = (unsafe { maint_ptr.as_ref() }) else {
        return;
    };

    let mut cfg = ScriptExecEnv::new(ctx, host_cb);
    cfg.args = vec![script_path.clone()];

    let base = &script_path[crate::paths::base_file_name(&script_path)..];
    let exec_msg = [b"Executing update-script ", script_path.as_slice()].concat();
    let (info_name, log_prefix, prefix_len_i32) =
        crate::extension::format_maint_script_prefix(false, base, 99);
    let mut active_prefix_len: usize = 0;

    let process_maint_msg = |kind: c_int, full_text: &[u8], pref_len: usize| {
        let text = &full_text[pref_len.min(full_text.len())..];
        if text.starts_with(b"[NZB] ") {
            host_log(
                ctx,
                host_cb,
                MK_DEBUG,
                &[b"Command ", &text[6..], b" detected"].concat(),
            );
            if &text[6..] == b"QUIT" {
                runner.detach(ctx, host_cb);
                unsafe {
                    host_cb(
                        ctx,
                        7,
                        0,
                        0,
                        0,
                        0,
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            } else {
                host_log(
                    ctx,
                    host_cb,
                    MK_ERROR,
                    &[b"Invalid command \"", text, b"\" received"].concat(),
                );
            }
            return;
        }
        maint.add_message_pub(kind, 0, text);
        host_log(ctx, host_cb, kind, text);
    };

    process_maint_msg(MK_INFO, &exec_msg, active_prefix_len);
    runner.set_info_name(&info_name);
    cfg.log_prefix = Some(log_prefix);
    active_prefix_len = prefix_len_i32.max(0) as usize;

    let arg_count = unsafe {
        host_cb(
            ctx,
            6,
            -1,
            0,
            0,
            0,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
        ) as c_int
    };
    for i in 0..arg_count {
        let mut strs = [ptr::null() as *const c_char; 1];
        unsafe {
            host_cb(
                ctx,
                6,
                i,
                0,
                0,
                0,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                strs.as_mut_ptr(),
            );
        }
        let key = format!("NZBUP_CMDLINE{i}");
        let val = unsafe { opt_bytes(strs[0]) };
        cfg.set_env_var(key.as_bytes(), &val);
    }

    let branch_s: &[u8] = match branch {
        1 => b"TESTING",
        2 => b"DEVEL",
        _ => b"STABLE",
    };
    let runmode_s: &[u8] = if daemon_mode { b"DAEMON" } else { b"SERVER" };
    cfg.set_env_var(b"NZBUP_BRANCH", branch_s);
    cfg.set_env_var(b"NZBUP_RUNMODE", runmode_s);
    let pid = unsafe { libc::getpid() };
    cfg.set_int_env_var(b"NZBUP_PROCESSID", pid);

    {
        let mut add_msg = |kind: c_int, text: &[u8]| {
            process_maint_msg(kind, text, active_prefix_len);
        };
        runner.execute(&mut cfg, ctx, host_cb, &mut add_msg);
    }

    maint.clear_update_controller();
}

pub(crate) fn run_maint_info_script(
    info_script: &[u8],
    info_name: &[u8],
    log_prefix: &[u8],
    prefix_len: usize,
    ctx: *mut c_void,
    host_cb: NzbgetRsScriptHostFn,
) -> Vec<u8> {
    let runner = ScriptRunner::new(true);
    runner.set_info_name(info_name);
    let mut cfg = ScriptExecEnv::new(ctx, host_cb);
    cfg.args = vec![info_script.to_vec()];
    cfg.log_prefix = Some(log_prefix.to_vec());

    let mut acc = Vec::new();
    let mut add_msg = |kind: c_int, full_text: &[u8]| {
        let text = &full_text[prefix_len.min(full_text.len())..];
        if text.starts_with(b"[NZB] ") {
            host_log(
                ctx,
                host_cb,
                MK_DEBUG,
                &[b"Command ", &text[6..], b" detected"].concat(),
            );
            if text[6..].starts_with(b"[UPDATEINFO]") {
                acc.extend_from_slice(&text[18..]);
            } else {
                host_log(
                    ctx,
                    host_cb,
                    MK_ERROR,
                    &[b"Invalid command \"", text, b"\" received from ", info_name].concat(),
                );
            }
            return;
        }
        host_log(ctx, host_cb, kind, text);
    };
    runner.execute(&mut cfg, ctx, host_cb, &mut add_msg);
    acc
}

// ============================================================================
// C-ABI entry points
// ============================================================================

#[no_mangle]
pub extern "C" fn nzbget_rs_ext_mgr_new() -> *mut RsExtensionManager {
    Box::into_raw(Box::new(RsExtensionManager::new()))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_mgr_free(mgr: *mut RsExtensionManager) {
    if !mgr.is_null() {
        drop(Box::from_raw(mgr));
    }
}

/// Unified `ExtensionManager::Manager` FFI dispatcher:
/// - `op = 0`: `LoadExtensions` (`s1 = script_dir`, `s2 = script_order`, `s3 = extensions_opt`,
///   `a1 = path_sep`, `p1 = create_sched_task_cb(ctx, task_time, name)`, `host_cb` for `error()`,
///   `out_buf` = error text on `0`, returns `1` on success)
/// - `op = 1`: `SerializeList` (`a1 = is_json`, writes JSON/XML array into `out_buf`, returns `1`)
/// - `op = 2`: `DeleteExtension` (`s1 = ext_name`, `out_buf` = error on `0`, returns `1` on success)
/// - `op = 3`: `InstallOrUpdateExtension` (`a1 = 0` install into `s2 = dest_dir`, `a1 = 1` update `s2 = ext_name`,
///   from `s1 = archive_path`, `host_cb` for logging, `out_buf` = error on `0`, returns `1` on success)
/// - `op = 4`: `MakeDownloadTempPath` (`s1 = temp_dir`, `a1 = path_sep`, writes path to `out_buf`, returns `1`)
/// - `op = 5`: `Scanner::InitPPParameters` (`s1 = extensions`, `a1 = unpack`, `a2 = reset`,
///   `ctx` = `NzbParameterList*`, `p1 = param_cb(ctx, find_or_set, name, val) -> int`)
/// - `op = 6`: `ScanScriptController::HasScripts` (`s1 = global_ext`, `strs` + `a1` = category extensions)
/// - `op = 7`: `HasQueueScripts` (returns `1` if any loaded extension has `queue_script`)
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_ext_mgr_op(
    mgr: *mut RsExtensionManager,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    strs: *const *const c_char,
    ctx: *mut c_void,
    host_cb: Option<NzbgetRsScriptHostFn>,
    out_buf: *mut RsBuf,
) -> c_int {
    static DL_COUNTER: AtomicI32 = AtomicI32::new(0);
    if op == 4 {
        let seq = DL_COUNTER.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        let ts = current_time_sec();
        let dir = opt_slice(s1);
        let sep = a1 as u8;
        let mut path = Vec::with_capacity(dir.len() + 40);
        path.extend_from_slice(dir);
        if !dir.is_empty() && dir.last() != Some(&sep) {
            path.push(sep);
        }
        path.extend_from_slice(format!("ext-{ts}-{seq}.zip").as_bytes());
        if !out_buf.is_null() {
            *out_buf = into_buf(path);
        }
        return 1;
    }

    let Some(m) = mgr.as_ref() else {
        return 0;
    };

    match op {
        0 => {
            let script_dir = opt_slice(s1);
            if script_dir.is_empty() {
                if !out_buf.is_null() {
                    *out_buf = into_buf(b"\"ScriptDir\" is not specified".to_vec());
                }
                return 0;
            }
            let script_order = opt_slice(s2);
            let enabled_exts = opt_slice(s3);
            let res = m.load_with_cb(script_dir, script_order, a1 as u8, |err_msg| {
                if let Some(cb) = host_cb {
                    host_log(ctx, cb, MK_ERROR, err_msg);
                }
            });
            match res {
                Ok(loaded) => {
                    if p1 != 0 {
                        let create_task: unsafe extern "C" fn(
                            *mut c_void,
                            *const c_char,
                            *const c_char,
                        ) = std::mem::transmute(p1);
                        for ext in &loaded {
                            if ext.scheduler_script
                                && !ext.task_time.is_empty()
                                && crate::extension::ext_is_in_list(enabled_exts, &ext.name)
                            {
                                let c_tt = c_buf(&ext.task_time);
                                let c_nm = c_buf(&ext.name);
                                create_task(ctx, c_tt.as_ptr().cast(), c_nm.as_ptr().cast());
                            }
                        }
                    }
                    1
                }
                Err(err_bytes) => {
                    if !out_buf.is_null() {
                        *out_buf = into_buf(err_bytes);
                    }
                    0
                }
            }
        }
        1 => {
            let serialized = m.serialize_list(a1 != 0);
            if !out_buf.is_null() {
                *out_buf = into_buf(serialized);
            }
            1
        }
        2 => match m.delete_bytes(opt_slice(s1)) {
            Ok(()) => 1,
            Err(err) => {
                if !out_buf.is_null() {
                    *out_buf = into_buf(err);
                }
                0
            }
        },
        3 => {
            let log_unpack = |kind: i32, msg: &str| {
                if let Some(cb) = host_cb {
                    let mk = match kind {
                        1 => MK_ERROR,
                        2 => MK_WARNING,
                        0 => MK_INFO,
                        _ => MK_DETAIL,
                    };
                    host_log(ctx, cb, mk, msg.as_bytes());
                }
            };
            let res = if a1 == 0 {
                match crate::extension::ext_mgr_install_extension(
                    opt_slice(s1),
                    opt_slice(s2),
                    log_unpack,
                ) {
                    None => Ok(()),
                    Some(err) => Err(err),
                }
            } else {
                m.update_bytes(opt_slice(s1), opt_slice(s2), log_unpack)
            };
            match res {
                Ok(()) => 1,
                Err(err) => {
                    if !out_buf.is_null() {
                        *out_buf = into_buf(err);
                    }
                    0
                }
            }
        }
        5 => {
            if p1 == 0 {
                return 0;
            }
            let param_cb: unsafe extern "C" fn(
                *mut c_void,
                c_int,
                *const c_char,
                *const c_char,
            ) -> c_int = std::mem::transmute(p1);
            let extensions = opt_slice(s1);
            let unpack = a1 != 0;
            let reset = a2 != 0;
            let snap = m.snapshot();
            if reset {
                for ext in &snap {
                    let mut pn = trunc_bytes([ext.name.as_slice(), b":"].concat(), 1023);
                    pn.push(0);
                    param_cb(ctx, 1, pn.as_ptr().cast(), ptr::null());
                }
            }
            if param_cb(ctx, 0, c"*Unpack:".as_ptr(), ptr::null()) == 0 {
                let v = if unpack { c"yes" } else { c"no" };
                param_cb(ctx, 1, c"*Unpack:".as_ptr(), v.as_ptr());
            }
            if !extensions.is_empty() {
                for ext in &snap {
                    if ext.post_script || ext.queue_script {
                        let mut pn = trunc_bytes([ext.name.as_slice(), b":"].concat(), 1023);
                        pn.push(0);
                        if param_cb(ctx, 0, pn.as_ptr().cast(), ptr::null()) == 0
                            && crate::extension::ext_is_in_list(extensions, &ext.name)
                        {
                            param_cb(ctx, 1, pn.as_ptr().cast(), c"yes".as_ptr());
                        }
                    }
                }
            }
            1
        }
        6 => {
            let mut has = false;
            m.execute_script_list(opt_slice(s1), |ext| has |= ext.scan_script);
            if !has && !strs.is_null() && a1 > 0 {
                for i in 0..(a1 as usize) {
                    m.execute_script_list(opt_slice(*strs.add(i)), |ext| has |= ext.scan_script);
                    if has {
                        break;
                    }
                }
            }
            c_int::from(has)
        }
        7 => c_int::from(m.find_if(|e| e.queue_script).is_some()),
        _ => 0,
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_script_runner_new() -> *mut NzbgetRsScriptRunner {
    Box::into_raw(Box::new(NzbgetRsScriptRunner {
        inner: ScriptRunner::new(true),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_script_runner_free(r: *mut NzbgetRsScriptRunner) {
    if !r.is_null() {
        drop(Box::from_raw(r));
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_script_runner_terminate(
    r: *mut NzbgetRsScriptRunner,
    host_cb: NzbgetRsScriptHostFn,
) {
    if let Some(r_ref) = r.as_ref() {
        r_ref.inner.terminate(ptr::null_mut(), host_cb);
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_script_terminate_all(host_cb: NzbgetRsScriptHostFn) {
    ScriptRunner::terminate_all(ptr::null_mut(), host_cb);
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_script_run_job(job_ptr: usize, host_cb: NzbgetRsScriptHostFn) {
    if job_ptr == 0 {
        return;
    }
    let _cancel_guard = crate::ffi::PosixCancelGuard::disable();
    let job = *Box::from_raw(job_ptr as *mut BackgroundScriptJob);
    match job {
        BackgroundScriptJob::Command {
            ext_mgr,
            script_name,
            command,
            log_id,
            modified_options,
        } => run_command_job(
            ext_mgr,
            script_name,
            command,
            log_id,
            modified_options,
            ptr::null_mut(),
            host_cb,
        ),
        BackgroundScriptJob::Queue {
            script,
            event,
            nzb_id,
            nzb_name,
            nzb_filename,
            url,
            category,
            dest_dir,
            queued_filename,
            priority,
            dupe_key,
            dupe_mode,
            dupe_score,
            delete_status,
            url_status,
            mark_status,
            parameters,
        } => run_queue_job(
            script,
            event,
            nzb_id,
            nzb_name,
            nzb_filename,
            url,
            category,
            dest_dir,
            queued_filename,
            priority,
            dupe_key,
            dupe_mode,
            dupe_score,
            delete_status,
            url_status,
            mark_status,
            parameters,
            ptr::null_mut(),
            host_cb,
        ),
        BackgroundScriptJob::Scheduler {
            ext_mgr,
            external_process,
            task_id,
            script_param,
            words,
        } => run_scheduler_job(
            ext_mgr,
            external_process,
            task_id,
            script_param,
            words,
            ptr::null_mut(),
            host_cb,
        ),
        BackgroundScriptJob::MaintenanceUpdate {
            maint,
            runner,
            script_path,
            branch,
            daemon_mode,
        } => run_maintenance_update_job(
            maint,
            runner,
            script_path,
            branch,
            daemon_mode,
            ptr::null_mut(),
            host_cb,
        ),
    }
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_command_script_start(
    ext_mgr: *const RsExtensionManager,
    script_name: *const c_char,
    command: *const c_char,
    log_id: c_int,
    params: *const crate::xmlrpc_format::NzbgetRsRpcParamItem,
    params_len: usize,
    host_cb: NzbgetRsScriptHostFn,
) -> c_int {
    let sname = opt_bytes(script_name);
    let cmd = opt_bytes(command);
    let mut mod_opts = Vec::with_capacity(params_len);
    if !params.is_null() {
        for i in 0..params_len {
            let item = &*params.add(i);
            mod_opts.push((opt_bytes(item.name), opt_bytes(item.value)));
        }
    }
    let found = ext_mgr
        .as_ref()
        .is_some_and(|m| m.find_by_name(&sname).is_some());
    spawn_background_job(
        BackgroundScriptJob::Command {
            ext_mgr,
            script_name: sname,
            command: cmd,
            log_id,
            modified_options: mod_opts,
        },
        ptr::null_mut(),
        host_cb,
    );
    c_int::from(found)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_feed_scripts_exec(
    ext_mgr: *const RsExtensionManager,
    feed_script: *const c_char,
    feed_file: *const c_char,
    feed_id: c_int,
    host_cb: NzbgetRsScriptHostFn,
) -> c_int {
    let Some(mgr) = ext_mgr.as_ref() else {
        return 1;
    };
    let ffile = opt_bytes(feed_file);
    let runner = ScriptRunner::new(true);
    let mut cfg = ScriptExecEnv::new(ptr::null_mut(), host_cb);
    let mut success = true;
    let mut add_msg = |kind: c_int, text: &[u8]| host_log(ptr::null_mut(), host_cb, kind, text);

    mgr.execute_script_list(opt_slice(feed_script), |ext| {
        if !ext.feed_script {
            return;
        }
        let banner = format!("Executing feed-script ",);
        let mut banner_bytes = banner.into_bytes();
        banner_bytes.extend_from_slice(&ext.name);
        banner_bytes.extend_from_slice(format!(" for Feed{feed_id}").as_bytes());
        add_msg(
            MK_INFO,
            &format_prefixed(cfg.log_prefix.as_deref(), &banner_bytes),
        );

        cfg.args = vec![ext.entry.clone()];
        let mut info_name = trunc_bytes(
            [
                b"feed-script ",
                ext.name.as_slice(),
                format!(" for Feed{feed_id}").as_bytes(),
            ]
            .concat(),
            1023,
        );
        runner.set_info_name(&info_name);
        cfg.log_prefix = Some(ext.display_name.clone());

        cfg.reset_env(ptr::null_mut(), host_cb);
        cfg.set_env_var(b"NZBFP_FILENAME", &ffile);
        cfg.set_int_env_var(b"NZBFP_FEEDID", feed_id);
        cfg.prepare_env_script(None, &ext.name, ptr::null_mut(), host_cb);

        let exit_code = runner.execute(&mut cfg, ptr::null_mut(), host_cb, &mut add_msg);
        if exit_code != 93 {
            if let Some(first) = info_name.first_mut() {
                *first = b'F';
            }
            let fail_msg = [info_name.as_slice(), b" failed"].concat();
            add_msg(
                MK_ERROR,
                &format_prefixed(cfg.log_prefix.as_deref(), &fail_msg),
            );
            success = false;
        }
        cfg.log_prefix = None;
    });
    c_int::from(success)
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_scheduler_script_start(
    ext_mgr: *const RsExtensionManager,
    param: *const c_char,
    external_process: c_int,
    task_id: c_int,
    words: *const *const c_char,
    word_count: usize,
    host_cb: NzbgetRsScriptHostFn,
) {
    let mut argv = Vec::new();
    if external_process != 0 && !words.is_null() {
        argv.reserve(word_count);
        for i in 0..word_count {
            argv.push(opt_bytes(*words.add(i)));
        }
    }
    spawn_background_job(
        BackgroundScriptJob::Scheduler {
            ext_mgr,
            external_process: external_process != 0,
            task_id,
            script_param: opt_bytes(param),
            words: argv,
        },
        ptr::null_mut(),
        host_cb,
    );
}

/// Executes `Scanner::ScanScriptController::ExecuteScripts` synchronously.
/// `scan_cb(scan_ctx, op, a1, s1, s2, ints, strs)`:
/// - `op = 0`: read current state (`strs[0]=nzbName, strs[1]=category, strs[2]=dupeKey`,
///   `ints[0]=priority, ints[1]=addTop, ints[2]=addPaused, ints[3]=dupeScore, ints[4]=dupeMode, ints[5]=paramCount`)
/// - `op = 1`: read parameter `a1` into `strs[0]=name, strs[1]=val`
/// - `op = 2`: apply `[NZB]` command (`a1 = cmd_kind`, `s1 = key_or_val`, `s2 = val`, `ints[0] = int_val`)
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_scan_scripts_exec(
    ext_mgr: *const RsExtensionManager,
    extensions: *const c_char,
    nzb_filename: *const c_char,
    url: *const c_char,
    directory: *const c_char,
    scan_ctx: *mut c_void,
    scan_cb: unsafe extern "C" fn(
        *mut c_void,
        c_int,
        c_int,
        *const c_char,
        *const c_char,
        *mut c_int,
        *mut *const c_char,
    ),
    host_cb: NzbgetRsScriptHostFn,
) {
    let Some(mgr) = ext_mgr.as_ref() else {
        return;
    };
    let fn_bytes = opt_bytes(nzb_filename);
    let base_fn = fn_bytes[crate::paths::base_file_name(&fn_bytes)..].to_vec();
    let url_bytes = opt_bytes(url);
    let dir_bytes = opt_bytes(directory);
    let c_fn = c_buf(&fn_bytes);
    let c_base_fn = c_buf(&base_fn);
    let c_url = c_buf(&url_bytes);

    let runner = ScriptRunner::new(true);
    let mut cfg = ScriptExecEnv::new(ptr::null_mut(), host_cb);

    mgr.execute_script_list(opt_slice(extensions), |ext| {
        if !ext.scan_script || !crate::fsops::file_exists(&fn_bytes, &crate::fsops::posix_stat) {
            return;
        }
        let banner = [
            b"Executing scan-script ",
            ext.name.as_slice(),
            b" for ",
            base_fn.as_slice(),
        ]
        .concat();
        host_log(ptr::null_mut(), host_cb, MK_INFO, &banner);

        cfg.args = vec![ext.entry.clone()];
        let info_name = trunc_bytes(
            [
                b"scan-script ",
                ext.name.as_slice(),
                b" for ",
                base_fn.as_slice(),
            ]
            .concat(),
            1023,
        );
        runner.set_info_name(&info_name);
        cfg.log_prefix = Some(ext.display_name.clone());
        let prefix_len = ext.display_name.len() + 2;

        // Read current mutable scan state + parameters
        let mut ints = [0 as c_int; 6];
        let mut strs = [ptr::null() as *const c_char; 3];
        scan_cb(
            scan_ctx,
            0,
            0,
            ptr::null(),
            ptr::null(),
            ints.as_mut_ptr(),
            strs.as_mut_ptr(),
        );
        let c_nzb_name = c_buf(opt_slice(strs[0]));
        let c_cat = c_buf(opt_slice(strs[1]));
        let c_dkey = c_buf(opt_slice(strs[2]));
        let has_params = ints[5] >= 0;
        let param_count = ints[5].max(0) as usize;
        let mut params = Vec::with_capacity(param_count);
        for i in 0..param_count {
            let mut pstrs = [ptr::null() as *const c_char; 2];
            scan_cb(
                scan_ctx,
                1,
                i as c_int,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                pstrs.as_mut_ptr(),
            );
            params.push((opt_bytes(pstrs[0]), opt_bytes(pstrs[1])));
        }

        cfg.reset_env(ptr::null_mut(), host_cb);
        let cfg_cell = std::cell::RefCell::new(&mut cfg);
        crate::extension::scan_script_prepare_env(
            CStr::from_bytes_with_nul(&c_fn).unwrap_or(c""),
            CStr::from_bytes_with_nul(&c_base_fn).unwrap_or(c""),
            CStr::from_bytes_with_nul(&c_url).unwrap_or(c""),
            CStr::from_bytes_with_nul(&c_nzb_name).unwrap_or(c""),
            CStr::from_bytes_with_nul(&c_cat).unwrap_or(c""),
            ints[0],
            ints[1] != 0,
            ints[2] != 0,
            CStr::from_bytes_with_nul(&c_dkey).unwrap_or(c""),
            ints[3],
            ints[4],
            &dir_bytes,
            crate::paths::PATH_SEPARATOR,
            |k, v| {
                cfg_cell
                    .borrow_mut()
                    .set_env_var(k.to_bytes(), v.to_bytes())
            },
            |k, iv| cfg_cell.borrow_mut().set_int_env_var(k.to_bytes(), iv),
        );
        cfg.prepare_env_script(
            if has_params { Some(&params) } else { None },
            &ext.name,
            ptr::null_mut(),
            host_cb,
        );

        let mut add_msg = |kind: c_int, text: &[u8]| {
            let msg_text = &text[prefix_len.min(text.len())..];
            let cmd = crate::extension::parse_scan_script_cmd(msg_text);
            if cmd.kind != 0 {
                if msg_text.len() >= 6 {
                    host_log(
                        ptr::null_mut(),
                        host_cb,
                        MK_DEBUG,
                        &[b"Command ", &msg_text[6..], b" detected"].concat(),
                    );
                }
                let val_c = c_buf(&msg_text[cmd.val_offset.min(msg_text.len())..]);
                if cmd.kind > 0 {
                    let key_c = if cmd.kind == 3 {
                        let end = (cmd.key_offset + cmd.key_len).min(msg_text.len());
                        c_buf(&msg_text[cmd.key_offset.min(end)..end])
                    } else {
                        vec![0]
                    };
                    let mut iv = [cmd.int_val];
                    scan_cb(
                        scan_ctx,
                        2,
                        cmd.kind,
                        key_c.as_ptr().cast(),
                        val_c.as_ptr().cast(),
                        iv.as_mut_ptr(),
                        ptr::null_mut(),
                    );
                } else if cmd.kind == -2 {
                    let cur_info = runner.get_info_name();
                    let val_slice = &val_c[..val_c.len().saturating_sub(1)];
                    host_log(
                        ptr::null_mut(),
                        host_cb,
                        MK_ERROR,
                        &[
                            b"Invalid value \"",
                            val_slice,
                            b"\" for command \"DUPEMODE\" received from ",
                            cur_info.as_slice(),
                        ]
                        .concat(),
                    );
                } else {
                    let cur_info = runner.get_info_name();
                    host_log(
                        ptr::null_mut(),
                        host_cb,
                        MK_ERROR,
                        &[
                            b"Invalid command \"",
                            msg_text,
                            b"\" received from ",
                            cur_info.as_slice(),
                        ]
                        .concat(),
                    );
                }
                return;
            }
            host_log(ptr::null_mut(), host_cb, kind, text);
        };
        runner.execute(&mut cfg, ptr::null_mut(), host_cb, &mut add_msg);
        cfg.log_prefix = None;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct MockHost {
        logs: Mutex<Vec<(c_int, Vec<u8>)>>,
        opts: Vec<(&'static [u8], &'static [u8])>,
    }

    unsafe extern "C" fn test_host_cb(
        ctx: *mut c_void,
        op: c_int,
        a1: c_int,
        _a2: c_int,
        p1: usize,
        p2: usize,
        s1: *const c_char,
        _s2: *const c_char,
        _ints: *mut c_int,
        strs: *mut *const c_char,
    ) -> usize {
        let host = &*(ctx as *const MockHost);
        match op {
            0 => {
                host.logs.lock().unwrap().push((a1, opt_bytes(s1)));
                0
            }
            1 => 0,
            2 => {
                if a1 == 0 {
                    *strs = c"".as_ptr();
                } else if a1 == 1 {
                    let f: extern "C" fn(usize, *const c_char, *const c_char) =
                        std::mem::transmute(p2);
                    for (k, v) in &host.opts {
                        let ck = c_buf(k);
                        let cv = c_buf(v);
                        f(p1, ck.as_ptr().cast(), cv.as_ptr().cast());
                    }
                }
                0
            }
            _ => 0,
        }
    }

    struct TempDirGuard {
        path: std::path::PathBuf,
    }

    impl TempDirGuard {
        fn new(tag: &str) -> Self {
            static SEQ: AtomicI32 = AtomicI32::new(0);
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "nzbget_rs_script_{}_{}_{}",
                tag,
                std::process::id(),
                n
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn script_runner_spawns_captures_env_output_and_fixes_eacces() {
        let dir = TempDirGuard::new("eacces");
        let script_path = dir.path.join("test_ext.sh");
        std::fs::write(
            &script_path,
            b"#!/bin/sh\necho \"[INFO] opt=$NZBOP_TEMPDIR po=$NZBPO_MYOPT arg=$1\"\necho \"[NZB] NZBPR_OUT=ok\"\nexit 93\n",
        )
        .unwrap();
        // Deliberately strip execute permissions (0o644): with 2 args (like UpdateScript),
        // `execvp` executes `script_path` directly, triggering EACCES -> chmod +x -> retry!
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let host = MockHost {
            logs: Mutex::new(Vec::new()),
            opts: vec![(b"TempDir", b"/tmp/nzb"), (b"test_ext:MyOpt", b"val42")],
        };
        let ctx = (&host as *const MockHost) as *mut c_void;

        let runner = ScriptRunner::new(true);
        runner.set_info_name(b"test_ext");
        let mut cfg = ScriptExecEnv::new(ctx, test_host_cb);
        let sp_bytes = crate::cleanup::os_str_to_bytes(script_path.as_os_str()).to_vec();
        cfg.args = vec![sp_bytes, b"hello".to_vec()];
        cfg.log_prefix = Some(b"TestExt".to_vec());
        cfg.prepare_env_script(None, b"test_ext", ctx, test_host_cb);

        let mut captured = Vec::new();
        let rc = runner.execute(&mut cfg, ctx, test_host_cb, &mut |kind, line| {
            captured.push((kind, line.to_vec()));
        });
        assert_eq!(rc, 93);
        assert!(captured
            .iter()
            .any(|(k, m)| *k == MK_WARNING && m.starts_with(b"TestExt: Fixing permissions for")));
        assert!(captured
            .iter()
            .any(|(k, m)| *k == MK_INFO && m == b"TestExt: opt=/tmp/nzb po=val42 arg=hello"));
        assert!(captured
            .iter()
            .any(|(k, m)| *k == MK_INFO && m == b"TestExt: [NZB] NZBPR_OUT=ok"));
    }

    #[test]
    fn script_runner_signal_exit_returns_128_plus_sig() {
        let dir = TempDirGuard::new("sigkill");
        let script_path = dir.path.join("crash.sh");
        std::fs::write(&script_path, b"#!/bin/sh\nkill -9 $$\n").unwrap();
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let host = MockHost {
            logs: Mutex::new(Vec::new()),
            opts: Vec::new(),
        };
        let ctx = (&host as *const MockHost) as *mut c_void;
        let runner = ScriptRunner::new(true);
        runner.set_info_name(b"crash_ext");
        let mut cfg = ScriptExecEnv::new(ctx, test_host_cb);
        cfg.args = vec![crate::cleanup::os_str_to_bytes(script_path.as_os_str()).to_vec()];

        let mut captured = Vec::new();
        let rc = runner.execute(&mut cfg, ctx, test_host_cb, &mut |kind, line| {
            captured.push((kind, line.to_vec()));
        });
        assert_eq!(rc, 128 + libc::SIGKILL);
        assert!(captured
            .iter()
            .any(|(k, m)| *k == MK_WARNING && m == b"crash_ext was killed by signal 9"));
    }

    #[test]
    fn prepare_env_options_matches_modified_options_case_insensitively() {
        let host = MockHost {
            logs: Mutex::new(Vec::new()),
            opts: vec![
                (b"TempDir", b"/tmp/saved"),
                (b"Email:Server", b"smtp.saved.example"),
            ],
        };
        let ctx = (&host as *const MockHost) as *mut c_void;
        let mut cfg = ScriptExecEnv {
            modified_options: vec![
                (b"tempdir".to_vec(), b"/tmp/override".to_vec()),
                (b"email:server".to_vec(), b"smtp.override.example".to_vec()),
            ],
            ..ScriptExecEnv::default()
        };
        cfg.prepare_env_script(None, b"Email", ctx, test_host_cb);
        cfg.prepare_env_options(None, ctx, test_host_cb);
        assert!(cfg
            .env_strings
            .contains(&b"NZBOP_TEMPDIR=/tmp/override".to_vec()));
        assert!(cfg
            .env_strings
            .contains(&b"NZBOP_EMAIL_SERVER=smtp.override.example".to_vec()));
        assert!(cfg
            .env_strings
            .contains(&b"NZBPO_SERVER=smtp.override.example".to_vec()));
    }
}
