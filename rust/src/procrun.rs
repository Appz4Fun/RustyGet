//! Child process execution (`ScriptController`) for the update scripts and the
//! scheduler `Process` command.

use crate::options::OptStr;
use std::ffi::{c_char, c_int, CStr};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::Duration;

pub use crate::log::{MK_INFO, MK_WARNING, MK_ERROR, MK_DEBUG, MK_DETAIL};

pub const FORK_ERROR_EXIT_CODE: c_int = 254;
const MAX_LINE_CHUNK: usize = 1024 * 10 - 1;
const WATCHDOG_WAIT_SECS: u64 = 60;

/// Process-wide services a script run needs: logging, the inherited
/// environment, options, the daemon's own command line, and shutdown.
///
/// [`NativeScriptHost`] is the production implementation; tests supply their own.
pub(crate) trait ScriptHost: Send + Sync {
    /// Writes a message to the global log (`kind` is a `MK_*` value).
    fn log(&self, kind: c_int, msg: &[u8]);
    /// The environment scripts inherit, as `NAME=value` entries.
    fn environment(&self) -> Vec<Vec<u8>>;
    /// The `ShellOverride` option.
    fn shell_override(&self) -> Vec<u8>;
    /// All options as `(name, value)` pairs, in definition order.
    fn options(&self) -> Vec<(Vec<u8>, Vec<u8>)>;
    /// The daemon's own command line (`argv`).
    fn args(&self) -> Vec<Vec<u8>>;
    /// Asks the daemon to shut down (`ExitProc`).
    fn exit_proc(&self);
}

/// The production [`ScriptHost`]: the active log, options and process state.
pub(crate) struct NativeScriptHost;

/// Shared production host instance.
pub(crate) static NATIVE_SCRIPT_HOST: NativeScriptHost = NativeScriptHost;

/// Collects a NULL-terminated C string array (`environ`-style).
unsafe fn c_str_array(arr: *const *const c_char) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if arr.is_null() {
        return out;
    }
    let mut idx = 0usize;
    loop {
        let p = *arr.add(idx);
        if p.is_null() {
            break;
        }
        out.push(CStr::from_ptr(p).to_bytes().to_vec());
        idx += 1;
    }
    out
}

impl ScriptHost for NativeScriptHost {
    fn log(&self, kind: c_int, msg: &[u8]) {
        crate::log::log_active_bytes(kind, msg);
    }

    fn environment(&self) -> Vec<Vec<u8>> {
        let (_, _, argp) = crate::entrypoint::process_args();
        if !argp.is_null() {
            return unsafe { c_str_array(argp.cast()) };
        }
        #[cfg(target_os = "macos")]
        unsafe {
            extern "C" {
                fn _NSGetEnviron() -> *mut *mut *mut c_char;
            }
            let p = _NSGetEnviron();
            if p.is_null() {
                Vec::new()
            } else {
                c_str_array((*p).cast())
            }
        }
        #[cfg(not(target_os = "macos"))]
        unsafe {
            extern "C" {
                static environ: *mut *mut c_char;
            }
            c_str_array(environ.cast())
        }
    }

    fn shell_override(&self) -> Vec<u8> {
        crate::options::active_options()
            .map_or_else(Vec::new, |o| o.str_bytes(OptStr::ShellOverride).to_vec())
    }

    fn options(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut out = Vec::new();
        if let Some(opts) = crate::options::active_options() {
            opts.for_each_option(|n, v, _| {
                out.push((n.to_bytes().to_vec(), v.to_bytes().to_vec()));
            });
        }
        out
    }

    fn args(&self) -> Vec<Vec<u8>> {
        let (argc, argv, _) = crate::entrypoint::process_args();
        if argv.is_null() || argc <= 0 {
            return Vec::new();
        }
        (0..argc as usize)
            .map(|i| unsafe { opt_bytes((*argv.add(i)).cast_const()) })
            .collect()
    }

    fn exit_proc(&self) {
        if let Some(exit_fn) = crate::lifecycle::get_hooks().and_then(|h| h.exit_proc) {
            // SAFETY: the lifecycle hook takes no arguments and is safe to call from any thread.
            unsafe { exit_fn() };
        }
    }
}

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
        host: &'static dyn ScriptHost,
    ) -> Self {
        let state = Arc::new((Mutex::new(false), Condvar::new()));
        let state_bg = Arc::clone(&state);
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
                host.log(MK_INFO, &msg);
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
    pub(crate) fn new(host: &dyn ScriptHost) -> Self {
        let mut s = Self::default();
        s.reset_env(host);
        s
    }

    pub(crate) fn reset_env(&mut self, host: &dyn ScriptHost) {
        self.env_strings = host
            .environment()
            .into_iter()
            .filter(|var| !is_nzbget_internal_env_var(var))
            .collect();
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
        let norm = normalize_special_env_var_name(&varname);
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

    pub(crate) fn prepare_env_options(
        &mut self,
        strip_prefix: Option<&[u8]>,
        host: &dyn ScriptHost,
    ) {
        let raw_opts = host.options();
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

    pub(crate) fn terminate(&self, host: &dyn ScriptHost) {
        let info = self.get_info_name();
        host.log(
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
            host.log(
                MK_DEBUG,
                &[b"Terminated ", info.as_slice()].concat(),
            );
        } else {
            host.log(
                MK_ERROR,
                &[b"Could not terminate ", info.as_slice()].concat(),
            );
        }
        host.log(
            MK_DEBUG,
            &[b"Stopped ", info.as_slice()].concat(),
        );
    }

    pub(crate) fn send_break(&self, host: &dyn ScriptHost) -> bool {
        let info = self.get_info_name();
        host.log(
            MK_DEBUG,
            &[b"Sending break signal to ", info.as_slice()].concat(),
        );
        let pid = self.process_id.load(Ordering::Acquire);
        let ok = pid > 0
            && !self.completed.load(Ordering::Acquire)
            && unsafe { libc::kill(pid, libc::SIGINT) } == 0;
        if ok {
            host.log(
                MK_DEBUG,
                &[b"Sent break signal to ", info.as_slice()].concat(),
            );
        } else {
            host.log(
                MK_WARNING,
                &[b"Could not send break signal to ", info.as_slice()].concat(),
            );
        }
        ok
    }

    pub(crate) fn detach(&self, host: &dyn ScriptHost) {
        let info = self.get_info_name();
        host.log(
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

    pub(crate) fn terminate_all(host: &dyn ScriptHost) {
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
                if script.send_break(host) {
                    let cur = current_time_sec();
                    while !script.completed.load(Ordering::Acquire)
                        && (cur - current_time_sec()).abs() <= 10
                    {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
                script.terminate(host);
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

    pub(crate) fn execute(
        &self,
        cfg: &mut ScriptExecEnv,
        host: &'static dyn ScriptHost,
        add_msg: &mut dyn FnMut(c_int, &[u8]),
    ) -> c_int {
        cfg.prepare_env_options(None, host);

        // PrepareArgs
        let shell_override = host.shell_override();

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
            host.log(
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

            host.log(MK_DEBUG, b"forking");
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

            host.log(MK_DEBUG, b"forked");
            host.log(
                MK_DEBUG,
                format!("Child Process-ID: {pid}").as_bytes(),
            );

            self.process_id.store(pid, Ordering::Release);
            unsafe {
                let _ = libc::close(pin[1]);
            }
            self.read_fd.store(pin[0], Ordering::Release);

            host.log(MK_DEBUG, b"Creating child watchdog");
            let mut watchdog = ChildWatchDogGuard::start(pid, self.get_info_name(), host);

            let mut reader = PipeLineReader::new();
            let mut line_buf = Vec::with_capacity(512);

            host.log(MK_DEBUG, b"Entering pipe-loop");
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
                    host.log(MK_DEBUG, b"Child confirmed");
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
                self.process_output_line(line, cfg.log_prefix.as_deref(), host, add_msg);
                first_line = false;
            }
            host.log(MK_DEBUG, b"Exited pipe-loop");

            host.log(MK_DEBUG, b"Destroying WatchDog");
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
                    host.log(
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

        host.log(
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
        host: &dyn ScriptHost,
        add_msg: &mut dyn FnMut(c_int, &[u8]),
    ) {
        host.log(
            MK_DEBUG,
            b"Processing output received from script",
        );
        let (trimmed_len, mk, text_off) = classify_script_output_line(line);
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
        host.log(
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

/// Background job dispatched through C++ `ScriptJobThread` so `Thread::GetThreadCount()`
/// tracks active background scripts accurately through daemon shutdown.
pub enum BackgroundScriptJob {
    /// Scheduler `Process` command: runs an external program.
    SchedulerProcess { task_id: c_int, words: Vec<Vec<u8>> },
    MaintenanceUpdate {
        maint: Arc<crate::maintenance::NzbgetRsMaintenance>,
        runner: Arc<ScriptRunner>,
        script_path: Vec<u8>,
        branch: c_int,
        daemon_mode: bool,
    },
}

pub(crate) fn spawn_background_job(
    job: BackgroundScriptJob,
    host: &'static dyn ScriptHost,
) {
    std::thread::spawn(move || run_background_script_job(job, host));
}
fn run_scheduler_process_job(
    task_id: c_int,
    words: Vec<Vec<u8>>,
    host: &'static dyn ScriptHost,
) {
    let runner = ScriptRunner::new(true);
    let mut cfg = ScriptExecEnv::new(host);
    let mut add_msg = |kind: c_int, text: &[u8]| host.log(kind, text);
    cfg.args = words;
    let script0 = cfg.args.first().map_or(&[][..], Vec::as_slice);
    let (exec_msg, info_name, log_prefix) =
        crate::scheduler::format_scheduler_script_labels(true, task_id, script0, b"");
    host.log(MK_INFO, &exec_msg);
    runner.set_info_name(&info_name);
    cfg.log_prefix = Some(log_prefix);
    runner.execute(&mut cfg, host, &mut add_msg);
}

fn run_maintenance_update_job(
    maint: Arc<crate::maintenance::NzbgetRsMaintenance>,
    runner: Arc<ScriptRunner>,
    script_path: Vec<u8>,
    branch: c_int,
    daemon_mode: bool,
    host: &'static dyn ScriptHost,
) {
    runner.unregister_running();
    let maint = maint.as_ref();

    let mut cfg = ScriptExecEnv::new(host);
    cfg.args = vec![script_path.clone()];

    let base = &script_path[crate::paths::base_file_name(&script_path)..];
    let exec_msg = [b"Executing update-script ", script_path.as_slice()].concat();
    let (info_name, log_prefix, prefix_len_i32) =
        crate::maintenance::format_maint_script_prefix(false, base, 99);
    let mut active_prefix_len: usize = 0;

    let process_maint_msg = |kind: c_int, full_text: &[u8], pref_len: usize| {
        let text = &full_text[pref_len.min(full_text.len())..];
        if text.starts_with(b"[NZB] ") {
            host.log(
                MK_DEBUG,
                &[b"Command ", &text[6..], b" detected"].concat(),
            );
            if &text[6..] == b"QUIT" {
                runner.detach(host);
                host.exit_proc();
            } else {
                host.log(
                    MK_ERROR,
                    &[b"Invalid command \"", text, b"\" received"].concat(),
                );
            }
            return;
        }
        maint.add_message_pub(kind, 0, text);
        host.log(kind, text);
    };

    process_maint_msg(MK_INFO, &exec_msg, active_prefix_len);
    runner.set_info_name(&info_name);
    cfg.log_prefix = Some(log_prefix);
    active_prefix_len = prefix_len_i32.max(0) as usize;

    for (i, arg) in host.args().iter().enumerate() {
        let key = format!("NZBUP_CMDLINE{i}");
        cfg.set_env_var(key.as_bytes(), arg);
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
        runner.execute(&mut cfg, host, &mut add_msg);
    }

    maint.clear_update_controller();
}

pub(crate) fn run_maint_info_script(
    info_script: &[u8],
    info_name: &[u8],
    log_prefix: &[u8],
    prefix_len: usize,
    host: &'static dyn ScriptHost,
) -> Vec<u8> {
    let runner = ScriptRunner::new(true);
    runner.set_info_name(info_name);
    let mut cfg = ScriptExecEnv::new(host);
    cfg.args = vec![info_script.to_vec()];
    cfg.log_prefix = Some(log_prefix.to_vec());

    let mut acc = Vec::new();
    let mut add_msg = |kind: c_int, full_text: &[u8]| {
        let text = &full_text[prefix_len.min(full_text.len())..];
        if text.starts_with(b"[NZB] ") {
            host.log(
                MK_DEBUG,
                &[b"Command ", &text[6..], b" detected"].concat(),
            );
            if text[6..].starts_with(b"[UPDATEINFO]") {
                acc.extend_from_slice(&text[18..]);
            } else {
                host.log(
                    MK_ERROR,
                    &[b"Invalid command \"", text, b"\" received from ", info_name].concat(),
                );
            }
            return;
        }
        host.log(kind, text);
    };
    runner.execute(&mut cfg, host, &mut add_msg);
    acc
}
pub(crate) fn terminate_all_active() {
    ScriptRunner::terminate_all(&NATIVE_SCRIPT_HOST);
}

fn run_background_script_job(job: BackgroundScriptJob, host: &'static dyn ScriptHost) {
    let stopped = AtomicBool::new(false);
    let _thread_guard = crate::ffi::RustThreadGuard::enter(&stopped);
    // SAFETY: only changes this thread's pthread cancel state; restored on drop.
    let _cancel_guard = unsafe { crate::ffi::PosixCancelGuard::disable() };
    match job {
        BackgroundScriptJob::SchedulerProcess { task_id, words } => {
            run_scheduler_process_job(task_id, words, host)
        }
        BackgroundScriptJob::MaintenanceUpdate {
            maint,
            runner,
            script_path,
            branch,
            daemon_mode,
        } => run_maintenance_update_job(maint, runner, script_path, branch, daemon_mode, host),
    }
    crate::ffi::stop_ssl_thread();
}
/// Starts the scheduler `Process` command (`words` = program and its arguments).
pub(crate) unsafe fn scheduler_process_start_active(
    task_id: c_int,
    words: *const *const c_char,
    word_count: usize,
) {
    let mut argv = Vec::with_capacity(word_count);
    if !words.is_null() {
        for i in 0..word_count {
            argv.push(opt_bytes(*words.add(i)));
        }
    }
    spawn_background_job(
        BackgroundScriptJob::SchedulerProcess {
            task_id,
            words: argv,
        },
        &NATIVE_SCRIPT_HOST,
    );
}

// ============================================================================
// 10. ScriptController (`daemon/util/ScriptController.cpp`)
// ============================================================================

/// Checks whether an environment variable string `var` matches the `NZBXX_...` pattern
/// filtered out in `EnvironmentStrings::InitFromCurrentProcess`.
pub fn is_nzbget_internal_env_var(var: &[u8]) -> bool {
    var.len() > 5 && var.starts_with(b"NZB") && var[5] == b'_'
}

/// Normalizes an environment variable name in `ScriptController::SetEnvVarSpecial`:
/// replaces `.:*!"$%&/()=`+~#'{}[]@- ` with `_` and converts ASCII letters to uppercase.
pub fn normalize_special_env_var_name(varname: &[u8]) -> Vec<u8> {
    const SPECIAL: &[u8] = b".:*!\"$%&/()=`+~#'{}[]@- ";
    varname
        .iter()
        .map(|&b| {
            let replaced = if SPECIAL.contains(&b) { b'_' } else { b };
            replaced.to_ascii_uppercase()
        })
        .collect()
}

/// Strips trailing `\n`, `\r`, and `' '` in-place and classifies the log prefix
/// in `ScriptController::ProcessOutput`.
/// Returns `(is_empty, message_kind, text_offset)` where `message_kind`:
/// - `0`: `mkInfo`
/// - `1`: `mkWarning`
/// - `2`: `mkError`
/// - `3`: `mkDetail`
/// - `4`: `mkDebug`
pub fn classify_script_output_line(line: &[u8]) -> (usize, i32, usize) {
    let mut trimmed_len = line.len();
    while trimmed_len > 0 {
        let last = line[trimmed_len - 1];
        if last == b'\n' || last == b'\r' || last == b' ' {
            trimmed_len -= 1;
        } else {
            break;
        }
    }
    if trimmed_len == 0 {
        return (0, 0, 0);
    }
    let trimmed = &line[..trimmed_len];
    if trimmed.starts_with(b"[INFO] ") {
        (trimmed_len, 0, 7)
    } else if trimmed.starts_with(b"[WARNING] ") {
        (trimmed_len, 1, 10)
    } else if trimmed.starts_with(b"[ERROR] ") {
        (trimmed_len, 2, 8)
    } else if trimmed.starts_with(b"[DETAIL] ") {
        (trimmed_len, 3, 9)
    } else if trimmed.starts_with(b"[DEBUG] ") {
        (trimmed_len, 4, 8)
    } else {
        (trimmed_len, 0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct MockHost {
        logs: Mutex<Vec<(c_int, Vec<u8>)>>,
        opts: Vec<(&'static [u8], &'static [u8])>,
    }

    impl ScriptHost for MockHost {
        fn log(&self, kind: c_int, msg: &[u8]) {
            self.logs.lock().unwrap().push((kind, msg.to_vec()));
        }
        fn environment(&self) -> Vec<Vec<u8>> {
            Vec::new()
        }
        fn shell_override(&self) -> Vec<u8> {
            Vec::new()
        }
        fn options(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
            self.opts.iter().map(|(k, v)| (k.to_vec(), v.to_vec())).collect()
        }
        fn args(&self) -> Vec<Vec<u8>> {
            Vec::new()
        }
        fn exit_proc(&self) {}
    }

    fn mock_host(opts: Vec<(&'static [u8], &'static [u8])>) -> &'static MockHost {
        Box::leak(Box::new(MockHost {
            logs: Mutex::new(Vec::new()),
            opts,
        }))
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
            b"#!/bin/sh\necho \"[INFO] opt=$NZBOP_TEMPDIR arg=$1\"\necho \"[NZB] NZBPR_OUT=ok\"\nexit 93\n",
        )
        .unwrap();
        // Deliberately strip execute permissions (0o644): with 2 args (like UpdateScript),
        // `execvp` executes `script_path` directly, triggering EACCES -> chmod +x -> retry!
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let host = mock_host(vec![(b"TempDir", b"/tmp/nzb")]);

        let runner = ScriptRunner::new(true);
        runner.set_info_name(b"test_ext");
        let mut cfg = ScriptExecEnv::new(host);
        let sp_bytes = crate::cleanup::os_str_to_bytes(script_path.as_os_str()).to_vec();
        cfg.args = vec![sp_bytes, b"hello".to_vec()];
        cfg.log_prefix = Some(b"TestExt".to_vec());
        cfg.prepare_env_options(None, host);

        let mut captured = Vec::new();
        let rc = runner.execute(&mut cfg, host, &mut |kind, line| {
            captured.push((kind, line.to_vec()));
        });
        assert_eq!(rc, 93);
        assert!(captured
            .iter()
            .any(|(k, m)| *k == MK_WARNING && m.starts_with(b"TestExt: Fixing permissions for")));
        assert!(captured
            .iter()
            .any(|(k, m)| *k == MK_INFO && m == b"TestExt: opt=/tmp/nzb arg=hello"));
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

        let host = mock_host(Vec::new());
        let runner = ScriptRunner::new(true);
        runner.set_info_name(b"crash_ext");
        let mut cfg = ScriptExecEnv::new(host);
        cfg.args = vec![crate::cleanup::os_str_to_bytes(script_path.as_os_str()).to_vec()];

        let mut captured = Vec::new();
        let rc = runner.execute(&mut cfg, host, &mut |kind, line| {
            captured.push((kind, line.to_vec()));
        });
        assert_eq!(rc, 128 + libc::SIGKILL);
        assert!(captured
            .iter()
            .any(|(k, m)| *k == MK_WARNING && m == b"crash_ext was killed by signal 9"));
    }

    #[test]
    fn prepare_env_options_matches_modified_options_case_insensitively() {
        let host = mock_host(vec![
                (b"TempDir", b"/tmp/saved"),
                (b"Email:Server", b"smtp.saved.example"),
            ]);
        let mut cfg = ScriptExecEnv {
            modified_options: vec![
                (b"tempdir".to_vec(), b"/tmp/override".to_vec()),
                (b"email:server".to_vec(), b"smtp.override.example".to_vec()),
            ],
            ..ScriptExecEnv::default()
        };
        cfg.prepare_env_options(None, host);
        assert!(cfg
            .env_strings
            .contains(&b"NZBOP_TEMPDIR=/tmp/override".to_vec()));
        assert!(cfg
            .env_strings
            .contains(&b"NZBOP_EMAIL_SERVER=smtp.override.example".to_vec()));
    }
}
