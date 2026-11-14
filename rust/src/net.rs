//! Stateful networking, NNTP session, HTTP downloader, and RPC listener/processor
//! runtime (`Connection`, `CancelRegistry`, `NntpConnection`, `WebDownloader`,
//! `RemoteServer`, and `RequestProcessor::Execute`).

use std::cell::{RefCell, UnsafeCell};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::artwritercore::join_c_slices;
use crate::socket::{
    accept_connection, bind_listener, cancel_socket, connect_stream, disconnect_socket,
    format_report_error, read_line, recv_exact, remote_addr, send_all, try_recv, write_line,
    CONNECTION_READBUFFER_SIZE, CS_BROKEN, CS_CANCELLED, CS_CONNECTED, CS_DISCONNECTED,
    INVALID_SOCKET,
};
use crate::webdownload::{
    webdl_download_with_redirects, webdl_run_loop, WebDlConfig, WebDlIo, WebDlStatus,
};

const DECODE_SLACK: usize = 128;
const NNTP_LINEBUF_SIZE: usize = 1024 * 10;
const NZBMESSAGE_SIGNATURE: u32 = 0x6E7A_6231;
const MAX_PROCESSORS: usize = 256;

static NEXT_CONN_ID: AtomicUsize = AtomicUsize::new(1);

/// Host callback bridging OpenSSL `TlsSocket`, `SocketDeadline`, thread-stop checks,
/// and C++ `Connection::PrintError` / logging.
///
/// Operations:
/// - `0`: `debug("%s", s1)`
/// - `1`: `a1 != 0` (`suppress_errors`) -> `debug("%s", s1)` else `PrintError(s1)` / `error`
/// - `2`: `is_stopped(deadline=p1)` -> `1` if stopped else `0`
/// - `3`: `deadline_wait(deadline=p1, fd=a1, r=a2!=0, w=*out_int!=0)` -> `1` / `0`
/// - `4`: `tls_start(fd=a1, flags_verif=a2, ptrs=p1 ([deadline, suppress_slot]), host=s1, cert=s2, key=s3, cipher=buf, out_int=[ok, probe_failure]) -> tls_sock`
/// - `5`: `tls_send(tls_sock=p1, data=s1, len=a1) -> sent`
/// - `6`: `tls_recv(tls_sock=p1, data=buf, len=a1) -> recvd`
/// - `7`: `tls_close_free(tls_sock=p1, do_close=a1!=0)`
/// - `9`: `tls_get_info(tls_sock=p1, out_ver=out_int) -> cipher_cstr as isize`
/// - `10`: `OpenSSL::StopSSLThread()`
pub type NzbgetRsConnHostFn = Option<
    unsafe extern "C" fn(
        ctx: *mut c_void,
        op: c_int,
        a1: c_int,
        a2: c_int,
        p1: usize,
        s1: *const c_char,
        s2: *const c_char,
        s3: *const c_char,
        buf: *mut c_char,
        out_int: *mut c_int,
    ) -> isize,
>;

fn bytes_to_cstring(raw: &[u8]) -> CString {
    let nul = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    // SAFETY: slice up to first NUL contains no NUL bytes.
    unsafe { CString::from_vec_unchecked(raw[..nul].to_vec()) }
}

unsafe fn opt_bytes<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        unsafe { CStr::from_ptr(p).to_bytes() }
    }
}

pub struct ConnCancelSlot {
    pub socket: AtomicI32,
    pub status: AtomicI32,
    pub suppress_errors: AtomicBool,
    pub owner_ctx: AtomicUsize,
    pub total_bytes_read: AtomicI32,
    pub accept_busy: AtomicBool,
    pub tls_probe_failure: AtomicI32,
    pub debug_on: bool,
    pub host_cb: NzbgetRsConnHostFn,
}

impl ConnCancelSlot {
    fn new(
        fd: c_int,
        status: c_int,
        debug_on: bool,
        owner_ctx: usize,
        host_cb: NzbgetRsConnHostFn,
    ) -> Self {
        Self {
            socket: AtomicI32::new(fd),
            status: AtomicI32::new(status),
            suppress_errors: AtomicBool::new(true),
            owner_ctx: AtomicUsize::new(owner_ctx),
            total_bytes_read: AtomicI32::new(0),
            accept_busy: AtomicBool::new(false),
            tls_probe_failure: AtomicI32::new(-1),
            debug_on,
            host_cb,
        }
    }

    fn emit_debug(&self, msg: &[u8]) {
        if !self.debug_on {
            return;
        }
        if let Some(cb) = self.host_cb {
            let cs = bytes_to_cstring(msg);
            let ctx = self.owner_ctx.load(Ordering::SeqCst);
            unsafe {
                cb(
                    ctx as *mut c_void,
                    0,
                    0,
                    0,
                    0,
                    cs.as_ptr(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
        }
    }

    pub fn set_suppress_errors(&self, sup: bool) {
        self.suppress_errors.store(sup, Ordering::SeqCst);
    }

    pub fn cancel(&self, suppress: bool, _host_cb: NzbgetRsConnHostFn) {
        if suppress {
            self.set_suppress_errors(true);
        }
        self.emit_debug(b"Cancelling connection");
        cancel_socket(false, &self.socket, &self.status);
    }
}

pub struct NzbgetRsCancelReg {
    inner: Mutex<(bool, Vec<(usize, Arc<ConnCancelSlot>)>)>,
}

impl Default for NzbgetRsCancelReg {
    fn default() -> Self {
        Self::new()
    }
}

impl NzbgetRsCancelReg {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new((false, Vec::new())),
        }
    }

    pub fn stop_all(&self) {
        let slots: Vec<Arc<ConnCancelSlot>> = match self.inner.lock() {
            Ok(mut g) => {
                g.0 = true;
                g.1.iter().map(|(_, s)| Arc::clone(s)).collect()
            }
            Err(p) => {
                let mut g = p.into_inner();
                g.0 = true;
                g.1.iter().map(|(_, s)| Arc::clone(s)).collect()
            }
        };
        for slot in slots {
            slot.status.store(CS_CANCELLED, Ordering::SeqCst);
            cancel_socket(false, &slot.socket, &slot.status);
        }
    }

    fn detach(&self, id: usize) {
        if let Ok(mut g) = self.inner.lock() {
            g.1.retain(|(eid, _)| *eid != id);
        }
    }
}

thread_local! {
    static TL_CANCEL_REG: RefCell<Option<Arc<NzbgetRsCancelReg>>> = const { RefCell::new(None) };
}

struct CancelRegScope {
    prev: Option<Arc<NzbgetRsCancelReg>>,
}

impl CancelRegScope {
    fn enter(reg: Arc<NzbgetRsCancelReg>) -> Self {
        let prev = TL_CANCEL_REG.with(|cell| cell.borrow_mut().replace(reg));
        Self { prev }
    }
}

impl Drop for CancelRegScope {
    fn drop(&mut self) {
        let prev = self.prev.take();
        TL_CANCEL_REG.with(|cell| {
            *cell.borrow_mut() = prev;
        });
    }
}

struct ConnCore {
    id: usize,
    slot: Arc<ConnCancelSlot>,
    cancel_reg: Option<Arc<NzbgetRsCancelReg>>,
    host: CString,
    cipher: CString,
    resolved_addrs: Vec<Vec<u8>>,
    deadline_ptr: usize,
    tls_box: usize,
    port: c_int,
    tls: bool,
    ip_version: c_int,
    read_buf: Vec<u8>,
    buf_offset: usize,
    buf_avail: usize,
    timeout: c_int,
    is_listener: bool,
    tls_error: bool,
    remote_addr_cstr: CString,
    graceful: bool,
    cert_verif_level: u32,
}

impl ConnCore {
    fn owner_ctx(&self) -> usize {
        self.slot.owner_ctx.load(Ordering::SeqCst)
    }

    fn emit_debug(&self, msg: &[u8]) {
        self.slot.emit_debug(msg);
    }

    fn emit_error(&self, msg: &[u8]) {
        if let Some(cb) = self.slot.host_cb {
            let cs = bytes_to_cstring(msg);
            let sup = c_int::from(self.slot.suppress_errors.load(Ordering::SeqCst));
            unsafe {
                cb(
                    self.owner_ctx() as *mut c_void,
                    1,
                    sup,
                    0,
                    0,
                    cs.as_ptr(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
        }
    }

    fn is_stopped(&self) -> bool {
        let Some(cb) = self.slot.host_cb else {
            return false;
        };
        unsafe {
            cb(
                self.owner_ctx() as *mut c_void,
                2,
                0,
                0,
                self.deadline_ptr,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
            ) != 0
        }
    }

    fn start_tls(&mut self, is_client: bool, cert_file: &CStr, key_file: &CStr) -> bool {
        self.emit_debug(b"Starting TLS");
        let Some(cb) = self.slot.host_cb else {
            return false;
        };
        let ctx = self.owner_ctx();
        let old_tb = std::mem::replace(&mut self.tls_box, 0);
        if old_tb != 0 {
            unsafe {
                cb(
                    ctx as *mut c_void,
                    7,
                    0,
                    0,
                    old_tb,
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
        }
        let fd = self.slot.socket.load(Ordering::SeqCst);
        let flags_verif = c_int::from(is_client) | ((self.cert_verif_level as c_int) << 2);
        let ptrs: [usize; 2] = [
            self.deadline_ptr,
            ptr::from_ref(&self.slot.suppress_errors) as usize,
        ];
        let mut out = [0 as c_int, -1 as c_int];
        let new_tb = unsafe {
            cb(
                ctx as *mut c_void,
                4,
                fd,
                flags_verif,
                ptrs.as_ptr() as usize,
                self.host.as_ptr(),
                cert_file.as_ptr(),
                key_file.as_ptr(),
                self.cipher.as_ptr().cast_mut(),
                out.as_mut_ptr(),
            ) as usize
        };
        self.tls_box = new_tb;
        self.slot.tls_probe_failure.store(out[1], Ordering::SeqCst);
        out[0] != 0
    }

    fn close_tls_box(&mut self, do_close: bool) {
        let tb = std::mem::replace(&mut self.tls_box, 0);
        if tb != 0 {
            if let Some(cb) = self.slot.host_cb {
                unsafe {
                    cb(
                        self.owner_ctx() as *mut c_void,
                        7,
                        c_int::from(do_close),
                        0,
                        tb,
                        ptr::null(),
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            }
        }
    }

    fn do_disconnect(&mut self) {
        self.emit_debug(b"Do disconnecting");
        let fd = self.slot.socket.load(Ordering::SeqCst);
        self.close_tls_box(fd != INVALID_SOCKET);
        let graceful = self.graceful
            && self.slot.status.load(Ordering::SeqCst) != CS_CANCELLED
            && !self.is_stopped();
        disconnect_socket(
            self.host.to_bytes(),
            graceful,
            &mut self.is_listener,
            &self.slot.socket,
            &self.slot.status,
        );
    }

    fn disconnect_base(&mut self) -> bool {
        self.emit_debug(b"Disconnecting");
        if self.slot.status.load(Ordering::SeqCst) == CS_DISCONNECTED
            && self.slot.socket.load(Ordering::SeqCst) == INVALID_SOCKET
        {
            self.close_tls_box(false);
            return true;
        }
        self.do_disconnect();
        self.buf_avail = 0;
        self.buf_offset = 0;
        true
    }

    fn connect_base(&mut self) -> bool {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        self.emit_debug(b"Connecting");
        if self.slot.status.load(Ordering::SeqCst) == CS_CONNECTED {
            return true;
        }
        let resolved_slices: Vec<&[u8]> = self.resolved_addrs.iter().map(Vec::as_slice).collect();
        let owner_ctx = self.owner_ctx();
        let host_cb = self.slot.host_cb;
        let debug_on = self.slot.debug_on;
        let sup_ref = &self.slot.suppress_errors;
        let deadline_ptr = self.deadline_ptr;
        let on_err = |msg: &[u8]| {
            if let Some(cb) = host_cb {
                let cs = bytes_to_cstring(msg);
                let sup = c_int::from(sup_ref.load(Ordering::SeqCst));
                unsafe {
                    cb(
                        owner_ctx as *mut c_void,
                        1,
                        sup,
                        0,
                        0,
                        cs.as_ptr(),
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            }
        };
        let on_dbg = |msg: &[u8]| {
            if !debug_on {
                return;
            }
            if let Some(cb) = host_cb {
                let cs = bytes_to_cstring(msg);
                unsafe {
                    cb(
                        owner_ctx as *mut c_void,
                        0,
                        0,
                        0,
                        0,
                        cs.as_ptr(),
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            }
        };
        let mut res = connect_stream(
            self.host.to_bytes(),
            self.port,
            self.ip_version,
            self.timeout,
            &resolved_slices,
            deadline_ptr != 0,
            |fd, r, w| {
                let Some(cb) = host_cb else {
                    return false;
                };
                let mut wf = c_int::from(w);
                unsafe {
                    cb(
                        owner_ctx as *mut c_void,
                        3,
                        fd,
                        c_int::from(r),
                        deadline_ptr,
                        ptr::null(),
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        &mut wf,
                    ) != 0
                }
            },
            || {
                host_cb.map_or(false, |cb| unsafe {
                    cb(
                        owner_ctx as *mut c_void,
                        2,
                        0,
                        0,
                        deadline_ptr,
                        ptr::null(),
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    ) != 0
                })
            },
            &self.slot.socket,
            &self.slot.status,
            &mut self.tls_error,
            on_err,
            on_dbg,
        );
        if self.slot.status.load(Ordering::SeqCst) == CS_CANCELLED || self.is_stopped() {
            res = false;
        }
        if res && self.tls && !self.start_tls(true, c"", c"") {
            res = false;
        }
        if res && self.slot.status.load(Ordering::SeqCst) != CS_CANCELLED && !self.is_stopped() {
            self.slot.status.store(CS_CONNECTED, Ordering::SeqCst);
        } else {
            res = false;
            self.do_disconnect();
        }
        res
    }

    fn bind(&mut self) -> bool {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        self.emit_debug(b"Binding");
        let owner_ctx = self.owner_ctx();
        let host_cb = self.slot.host_cb;
        let sup_ref = &self.slot.suppress_errors;
        bind_listener(
            self.host.to_bytes(),
            self.port,
            self.ip_version,
            &self.slot.socket,
            &self.slot.status,
            &mut self.is_listener,
            &mut self.tls_error,
            |msg| {
                if let Some(cb) = host_cb {
                    let cs = bytes_to_cstring(msg);
                    let sup = c_int::from(sup_ref.load(Ordering::SeqCst));
                    unsafe {
                        cb(
                            owner_ctx as *mut c_void,
                            1,
                            sup,
                            0,
                            0,
                            cs.as_ptr(),
                            ptr::null(),
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                    }
                }
            },
        )
    }

    fn accept_fd(&mut self) -> c_int {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        self.emit_debug(b"Accepting connection");
        let owner_ctx = self.owner_ctx();
        let host_cb = self.slot.host_cb;
        let sup_ref = &self.slot.suppress_errors;
        let deadline_ptr = self.deadline_ptr;
        let mut busy = self.slot.accept_busy.load(Ordering::SeqCst);
        let fd = accept_connection(
            self.host.to_bytes(),
            self.timeout,
            &self.slot.socket,
            &self.slot.status,
            &mut busy,
            &mut self.tls_error,
            || {
                host_cb.map_or(false, |cb| unsafe {
                    cb(
                        owner_ctx as *mut c_void,
                        2,
                        0,
                        0,
                        deadline_ptr,
                        ptr::null(),
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    ) != 0
                })
            },
            |msg| {
                if let Some(cb) = host_cb {
                    let cs = bytes_to_cstring(msg);
                    let sup = c_int::from(sup_ref.load(Ordering::SeqCst));
                    unsafe {
                        cb(
                            owner_ctx as *mut c_void,
                            1,
                            sup,
                            0,
                            0,
                            cs.as_ptr(),
                            ptr::null(),
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                    }
                }
            },
        );
        self.slot.accept_busy.store(busy, Ordering::SeqCst);
        fd
    }

    fn raw_send_chunk(
        slot: &ConnCancelSlot,
        tb: usize,
        owner_ctx: usize,
        host_cb: NzbgetRsConnHostFn,
        tls_error: &mut bool,
        chunk: &[u8],
    ) -> c_int {
        if tb != 0 {
            *tls_error = false;
            let sent = host_cb.map_or(-1, |cb| unsafe {
                cb(
                    owner_ctx as *mut c_void,
                    5,
                    chunk.len() as c_int,
                    0,
                    tb,
                    chunk.as_ptr().cast::<c_char>(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                ) as c_int
            });
            if sent < 0 {
                *tls_error = true;
                return -1;
            }
            return sent;
        }
        unsafe {
            libc::send(
                slot.socket.load(Ordering::SeqCst),
                chunk.as_ptr().cast(),
                chunk.len(),
                0,
            ) as c_int
        }
    }

    fn raw_recv_chunk(
        slot: &ConnCancelSlot,
        tb: usize,
        owner_ctx: usize,
        host_cb: NzbgetRsConnHostFn,
        chunk: &mut [u8],
        te: &mut bool,
    ) -> c_int {
        if tb != 0 {
            *te = false;
            let recvd = host_cb.map_or(-1, |cb| unsafe {
                cb(
                    owner_ctx as *mut c_void,
                    6,
                    chunk.len() as c_int,
                    0,
                    tb,
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    chunk.as_mut_ptr().cast::<c_char>(),
                    ptr::null_mut(),
                ) as c_int
            });
            if recvd < 0 {
                *te = true;
                return -1;
            }
            return recvd;
        }
        *te = false;
        unsafe {
            libc::recv(
                slot.socket.load(Ordering::SeqCst),
                chunk.as_mut_ptr().cast(),
                chunk.len(),
                0,
            ) as c_int
        }
    }

    fn write_line_bytes(&mut self, line: &[u8]) -> c_int {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        let owner_ctx = self.owner_ctx();
        let tb = self.tls_box;
        let slot = &self.slot;
        let host_cb = slot.host_cb;
        let tls_err = &mut self.tls_error;
        write_line(&slot.status, line, |chunk| {
            Self::raw_send_chunk(slot, tb, owner_ctx, host_cb, tls_err, chunk)
        })
    }

    fn send_bytes(&mut self, data: &[u8]) -> bool {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        self.emit_debug(b"Sending data");
        let owner_ctx = self.owner_ctx();
        let tb = self.tls_box;
        let slot = &self.slot;
        let host_cb = slot.host_cb;
        let tls_err = &mut self.tls_error;
        send_all(&slot.status, data, |chunk| {
            Self::raw_send_chunk(slot, tb, owner_ctx, host_cb, tls_err, chunk)
        })
    }

    fn read_line_slice(&mut self, dst: &mut [u8]) -> Option<usize> {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        if self.slot.status.load(Ordering::SeqCst) != CS_CONNECTED || dst.is_empty() {
            return None;
        }
        let owner_ctx = self.owner_ctx();
        let tb = self.tls_box;
        let slot = &self.slot;
        let host_cb = slot.host_cb;
        let mut delta = 0i32;
        let res = read_line(
            self.host.to_bytes(),
            &slot.status,
            &mut self.read_buf,
            &mut self.buf_offset,
            &mut self.buf_avail,
            &mut delta,
            dst,
            &mut self.tls_error,
            |chunk, te| Self::raw_recv_chunk(slot, tb, owner_ctx, host_cb, chunk, te),
            |msg| {
                if let Some(cb) = host_cb {
                    let cs = bytes_to_cstring(msg);
                    let sup = c_int::from(slot.suppress_errors.load(Ordering::SeqCst));
                    unsafe {
                        cb(
                            owner_ctx as *mut c_void,
                            1,
                            sup,
                            0,
                            0,
                            cs.as_ptr(),
                            ptr::null(),
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                    }
                }
            },
        );
        if delta != 0 {
            slot.total_bytes_read.fetch_add(delta, Ordering::SeqCst);
        }
        res
    }

    fn recv_exact_slice(&mut self, dst: &mut [u8]) -> bool {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        if dst.is_empty() {
            return true;
        }
        let owner_ctx = self.owner_ctx();
        let tb = self.tls_box;
        let slot = &self.slot;
        let host_cb = slot.host_cb;
        let mut delta = 0i32;
        let ok = recv_exact(
            self.host.to_bytes(),
            &self.read_buf,
            &mut self.buf_offset,
            &mut self.buf_avail,
            &mut delta,
            dst,
            &mut self.tls_error,
            |chunk, te| Self::raw_recv_chunk(slot, tb, owner_ctx, host_cb, chunk, te),
            |msg| {
                if let Some(cb) = host_cb {
                    let cs = bytes_to_cstring(msg);
                    let sup = c_int::from(slot.suppress_errors.load(Ordering::SeqCst));
                    unsafe {
                        cb(
                            owner_ctx as *mut c_void,
                            1,
                            sup,
                            0,
                            0,
                            cs.as_ptr(),
                            ptr::null(),
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                    }
                }
            },
        );
        if delta != 0 {
            slot.total_bytes_read.fetch_add(delta, Ordering::SeqCst);
        }
        ok
    }

    fn try_recv_slice(&mut self, dst: &mut [u8]) -> c_int {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        let owner_ctx = self.owner_ctx();
        let tb = self.tls_box;
        let slot = &self.slot;
        let host_cb = slot.host_cb;
        let mut delta = 0i32;
        let n = try_recv(
            self.host.to_bytes(),
            &mut delta,
            dst,
            &mut self.tls_error,
            |chunk, te| Self::raw_recv_chunk(slot, tb, owner_ctx, host_cb, chunk, te),
            |msg| {
                if let Some(cb) = host_cb {
                    let cs = bytes_to_cstring(msg);
                    let sup = c_int::from(slot.suppress_errors.load(Ordering::SeqCst));
                    unsafe {
                        cb(
                            owner_ctx as *mut c_void,
                            1,
                            sup,
                            0,
                            0,
                            cs.as_ptr(),
                            ptr::null(),
                            ptr::null(),
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                    }
                }
            },
        );
        if delta != 0 {
            slot.total_bytes_read.fetch_add(delta, Ordering::SeqCst);
        }
        n
    }

    fn report_error_msg(
        &mut self,
        prefix: &[u8],
        arg: Option<&[u8]>,
        print_code: bool,
        code: c_int,
    ) {
        if let Some(formatted) =
            format_report_error(&mut self.tls_error, prefix, arg, print_code, code, None)
        {
            self.emit_error(&formatted);
        }
    }

    fn get_remote_addr(&mut self) -> &CStr {
        let h = self.host.to_bytes();
        if !h.is_empty() && h[0] == b'/' {
            return c"-";
        }
        let ra = remote_addr(h, self.slot.socket.load(Ordering::SeqCst));
        let cap = ra.len().min(99);
        self.remote_addr_cstr = bytes_to_cstring(&ra[..cap]);
        &self.remote_addr_cstr
    }
}

struct ConnSession {
    core: ConnCore,
    nntp_active_group: Option<CString>,
    nntp_line_buf: Option<Box<[u8; NNTP_LINEBUF_SIZE]>>,
    nntp_auth: crate::nntp::Auth,
}

impl ConnSession {
    fn nntp_connect(&mut self, user: &CStr, pass: &CStr, srv_name: &CStr, srv_host: &CStr) -> bool {
        if self.core.slot.debug_on {
            let msg = join_c_slices(&[b"Opening connection to ", self.core.host.to_bytes()]);
            self.core.emit_debug(&msg);
        }
        if self.core.slot.status.load(Ordering::SeqCst) == CS_CONNECTED {
            return true;
        }
        self.nntp_auth.rejected = false;
        self.nntp_active_group = None;
        if !self.core.connect_base() {
            return false;
        }
        let line_buf = self
            .nntp_line_buf
            .get_or_insert_with(|| Box::new([0u8; NNTP_LINEBUF_SIZE]));
        let conn_host = self.core.host.clone();
        let mut io = DirectNntpIo {
            core: &mut self.core,
            line_buf,
            user,
            pass,
            srv_name,
            srv_host,
            conn_host: &conn_host,
        };
        let result = crate::nntp::handshake(&mut io, &mut self.nntp_auth).unwrap_or(1);
        if result != 0 {
            if result == 1 {
                self.nntp_disconnect(user, pass, srv_name, srv_host);
            }
            return false;
        }
        true
    }

    fn nntp_disconnect(
        &mut self,
        user: &CStr,
        pass: &CStr,
        srv_name: &CStr,
        srv_host: &CStr,
    ) -> bool {
        if self.core.slot.status.load(Ordering::SeqCst) == CS_CONNECTED {
            let _ = self.nntp_request(c"quit\r\n".as_ptr(), user, pass, srv_name, srv_host);
        }
        self.nntp_active_group = None;
        self.core.disconnect_base()
    }

    fn nntp_request(
        &mut self,
        req: *const c_char,
        user: &CStr,
        pass: &CStr,
        srv_name: &CStr,
        srv_host: &CStr,
    ) -> *const c_char {
        if req.is_null() {
            return ptr::null();
        }
        let line_buf = self
            .nntp_line_buf
            .get_or_insert_with(|| Box::new([0u8; NNTP_LINEBUF_SIZE]));
        let conn_host = self.core.host.clone();
        let mut io = DirectNntpIo {
            core: &mut self.core,
            line_buf,
            user,
            pass,
            srv_name,
            srv_host,
            conn_host: &conn_host,
        };
        match unsafe { crate::nntp::request(&mut io, &mut self.nntp_auth, req) } {
            Ok(ans) => ans.cast_const(),
            Err(_) => ptr::null(),
        }
    }

    fn nntp_join_group(
        &mut self,
        grp: *const c_char,
        user: &CStr,
        pass: &CStr,
        srv_name: &CStr,
        srv_host: &CStr,
    ) -> *const c_char {
        if !grp.is_null() {
            let grp_cstr = unsafe { CStr::from_ptr(grp) };
            if let Some(active) = self.nntp_active_group.as_deref() {
                if !active.to_bytes().is_empty() && active == grp_cstr {
                    let buf = self
                        .nntp_line_buf
                        .get_or_insert_with(|| Box::new([0u8; NNTP_LINEBUF_SIZE]));
                    buf[..5].copy_from_slice(b"211 \0");
                    return buf.as_ptr().cast::<c_char>();
                }
            }
        } else {
            return ptr::null();
        }
        let line_buf = self
            .nntp_line_buf
            .get_or_insert_with(|| Box::new([0u8; NNTP_LINEBUF_SIZE]));
        let conn_host = self.core.host.clone();
        let mut io = DirectNntpIo {
            core: &mut self.core,
            line_buf,
            user,
            pass,
            srv_name,
            srv_host,
            conn_host: &conn_host,
        };
        match unsafe { crate::nntp::join_group(&mut io, &mut self.nntp_auth, grp) } {
            Ok((ans, joined)) => {
                if joined {
                    let grp_cstr = unsafe { CStr::from_ptr(grp) };
                    self.nntp_active_group = Some(grp_cstr.to_owned());
                }
                ans.cast_const()
            }
            Err(_) => ptr::null(),
        }
    }
}

pub struct NzbgetRsConn {
    slot: Arc<ConnCancelSlot>,
    host: CString,
    session: UnsafeCell<ConnSession>,
}

// SAFETY: Cross-thread calls (`Cancel`, `SetSuppressErrors`, status/TLS/byte queries) borrow
// `NzbgetRsConn` by shared `&self` and access only `slot` atomics and immutable `host`, never
// `session`. Sequential owner-thread I/O borrows `session` inside `UnsafeCell`.
unsafe impl Sync for NzbgetRsConn {}

impl Drop for NzbgetRsConn {
    fn drop(&mut self) {
        let sess = self.session.get_mut();
        sess.core.emit_debug(b"Destroying Connection");
        if let Some(reg) = sess.core.cancel_reg.take() {
            reg.detach(sess.core.id);
        }
        sess.core.disconnect_base();
    }
}

impl NzbgetRsConn {
    pub fn new_client(
        host: &[u8],
        port: c_int,
        tls: bool,
        register_cancel: bool,
        debug_on: bool,
        owner_ctx: usize,
        host_cb: NzbgetRsConnHostFn,
    ) -> Self {
        let id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
        let slot = Arc::new(ConnCancelSlot::new(
            INVALID_SOCKET,
            CS_DISCONNECTED,
            debug_on,
            owner_ctx,
            host_cb,
        ));
        let mut cancel_reg = None;
        if register_cancel {
            TL_CANCEL_REG.with(|cell| {
                if let Some(reg) = cell.borrow().as_ref() {
                    let reg_arc = Arc::clone(reg);
                    let pre_stopped = match reg_arc.inner.lock() {
                        Ok(mut g) => {
                            g.1.push((id, Arc::clone(&slot)));
                            g.0
                        }
                        Err(p) => {
                            let mut g = p.into_inner();
                            g.1.push((id, Arc::clone(&slot)));
                            g.0
                        }
                    };
                    if pre_stopped {
                        slot.status.store(CS_CANCELLED, Ordering::SeqCst);
                        cancel_socket(false, &slot.socket, &slot.status);
                    }
                    cancel_reg = Some(reg_arc);
                }
            });
        }
        let host_cstr = bytes_to_cstring(host);
        let core = ConnCore {
            id,
            slot: Arc::clone(&slot),
            cancel_reg,
            host: host_cstr.clone(),
            cipher: CString::default(),
            resolved_addrs: Vec::new(),
            deadline_ptr: 0,
            tls_box: 0,
            port,
            tls,
            ip_version: 0,
            read_buf: vec![0u8; CONNECTION_READBUFFER_SIZE + 1 + DECODE_SLACK],
            buf_offset: 0,
            buf_avail: 0,
            timeout: 60,
            is_listener: false,
            tls_error: false,
            remote_addr_cstr: CString::default(),
            graceful: false,
            cert_verif_level: 2,
        };
        core.emit_debug(b"Creating Connection");
        Self {
            slot,
            host: host_cstr,
            session: UnsafeCell::new(ConnSession {
                core,
                nntp_active_group: None,
                nntp_line_buf: None,
                nntp_auth: crate::nntp::Auth::default(),
            }),
        }
    }

    pub fn from_accepted(
        fd: c_int,
        tls: bool,
        timeout: c_int,
        debug_on: bool,
        owner_ctx: usize,
        host_cb: NzbgetRsConnHostFn,
    ) -> Self {
        let id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
        let slot = Arc::new(ConnCancelSlot::new(
            fd,
            CS_CONNECTED,
            debug_on,
            owner_ctx,
            host_cb,
        ));
        let mut core = ConnCore {
            id,
            slot: Arc::clone(&slot),
            cancel_reg: None,
            host: CString::default(),
            cipher: CString::default(),
            resolved_addrs: Vec::new(),
            deadline_ptr: 0,
            tls_box: 0,
            port: 0,
            tls,
            ip_version: 0,
            read_buf: vec![0u8; CONNECTION_READBUFFER_SIZE + 1 + DECODE_SLACK],
            buf_offset: 0,
            buf_avail: 0,
            timeout,
            is_listener: false,
            tls_error: false,
            remote_addr_cstr: CString::default(),
            graceful: false,
            cert_verif_level: 2,
        };
        core.emit_debug(b"Creating Connection");
        let host_cstr = core.get_remote_addr().to_owned();
        core.host = host_cstr.clone();
        Self {
            slot,
            host: host_cstr,
            session: UnsafeCell::new(ConnSession {
                core,
                nntp_active_group: None,
                nntp_line_buf: None,
                nntp_auth: crate::nntp::Auth::default(),
            }),
        }
    }

    pub(crate) unsafe fn remote_addr_bytes(&self) -> Vec<u8> {
        (*self.session.get())
            .core
            .get_remote_addr()
            .to_bytes()
            .to_vec()
    }

    pub(crate) unsafe fn read_line_web(&self, dst: &mut [u8]) -> bool {
        (*self.session.get()).core.read_line_slice(dst).is_some()
    }

    pub(crate) unsafe fn recv_exact_web(&self, dst: &mut [u8]) -> bool {
        if dst.is_empty() {
            true
        } else {
            (*self.session.get()).core.recv_exact_slice(dst)
        }
    }

    pub(crate) unsafe fn send_web(&self, src: &[u8]) -> bool {
        (*self.session.get()).core.send_bytes(src)
    }
}

struct DirectNntpIo<'a> {
    core: &'a mut ConnCore,
    line_buf: &'a mut [u8; NNTP_LINEBUF_SIZE],
    user: &'a CStr,
    pass: &'a CStr,
    srv_name: &'a CStr,
    srv_host: &'a CStr,
    conn_host: &'a CStr,
}

unsafe impl crate::nntp::Io for DirectNntpIo<'_> {
    fn write_line(&mut self, line: &CStr) -> Result<(), crate::nntp::Abort> {
        self.core.write_line_bytes(line.to_bytes());
        Ok(())
    }

    fn read_line(&mut self) -> Result<*mut c_char, crate::nntp::Abort> {
        if self.core.read_line_slice(&mut self.line_buf[..]).is_some() {
            Ok(self.line_buf.as_mut_ptr().cast::<c_char>())
        } else {
            Ok(ptr::null_mut())
        }
    }

    fn report_error(&mut self, prefix: &CStr, arg: &CStr) -> Result<(), crate::nntp::Abort> {
        self.core
            .report_error_msg(prefix.to_bytes(), Some(arg.to_bytes()), false, 0);
        Ok(())
    }

    fn debug(&mut self, msg: &CStr) -> Result<(), crate::nntp::Abort> {
        self.core.emit_debug(msg.to_bytes());
        Ok(())
    }

    fn debug_enabled(&self) -> bool {
        self.core.slot.debug_on
    }

    fn cancelled(&mut self) -> Result<bool, crate::nntp::Abort> {
        Ok(self.core.slot.status.load(Ordering::SeqCst) == CS_CANCELLED)
    }

    fn user(&self) -> &CStr {
        self.user
    }

    fn password(&self) -> &CStr {
        self.pass
    }

    fn name(&self) -> &CStr {
        self.srv_name
    }

    fn host(&self) -> &CStr {
        self.srv_host
    }

    fn conn_host(&self) -> &CStr {
        self.conn_host
    }
}

pub(crate) type WebDlStatusHook = Box<dyn Fn(&NzbgetRsWebDl, WebDlStatus) + Send + Sync>;
pub(crate) type WebDlHeaderHook = Box<dyn Fn(&NzbgetRsWebDl, &[u8]) + Send + Sync>;

fn now_unix_sec() -> i64 {
    #[cfg(unix)]
    unsafe {
        libc::time(ptr::null_mut()) as i64
    }
    #[cfg(not(unix))]
    {
        0
    }
}

pub struct NzbgetRsWebDl {
    url: Mutex<CString>,
    output_filename: Mutex<CString>,
    info_name: Mutex<CString>,
    original_filename: Mutex<Option<CString>>,
    status: AtomicI32,
    last_update_time: AtomicI64,
    http_status: AtomicI32,
    force: AtomicBool,
    retry: AtomicBool,
    cert_verif_level: AtomicI32,
    stopped: AtomicBool,
    active_slot: Mutex<Option<Arc<ConnCancelSlot>>>,
    debug_on: bool,
    conn_host_cb: NzbgetRsConnHostFn,
    on_status: Option<WebDlStatusHook>,
    on_header: Option<WebDlHeaderHook>,
}

impl NzbgetRsWebDl {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_configured(
        url: &[u8],
        info_name: &[u8],
        output_filename: &[u8],
        force: bool,
        retry: bool,
        cert_verif_level: c_int,
        on_status: Option<WebDlStatusHook>,
        on_header: Option<WebDlHeaderHook>,
    ) -> Self {
        let dl = Self {
            url: Mutex::new(bytes_to_cstring(url)),
            output_filename: Mutex::new(bytes_to_cstring(output_filename)),
            info_name: Mutex::new(bytes_to_cstring(info_name)),
            original_filename: Mutex::new(None),
            status: AtomicI32::new(WebDlStatus::AdUndefined as i32),
            last_update_time: AtomicI64::new(now_unix_sec()),
            http_status: AtomicI32::new(0),
            force: AtomicBool::new(force),
            retry: AtomicBool::new(retry),
            cert_verif_level: AtomicI32::new(cert_verif_level),
            stopped: AtomicBool::new(false),
            active_slot: Mutex::new(None),
            debug_on: crate::ffi::build_debug_enabled(),
            conn_host_cb: crate::ffi::tls_host_cb(),
            on_status,
            on_header,
        };
        dl.emit_log(0, b"Creating WebDownloader");
        dl
    }

    pub(crate) fn spawn(dl: Arc<Self>) {
        std::thread::spawn(move || {
            let _tg = crate::ffi::RustThreadGuard::enter(&dl.stopped);
            dl.execute(1, 5);
            crate::ffi::stop_ssl_thread();
        });
    }

    pub(crate) fn download_sync(&self, max_redirects: c_int) -> c_int {
        self.execute(0, max_redirects)
    }

    pub(crate) fn status_code(&self) -> c_int {
        self.status.load(Ordering::SeqCst)
    }

    pub(crate) fn http_status(&self) -> c_int {
        self.http_status.load(Ordering::SeqCst)
    }

    pub(crate) fn last_update_time(&self) -> i64 {
        self.last_update_time.load(Ordering::SeqCst)
    }

    pub(crate) fn info_name_bytes(&self) -> Vec<u8> {
        self.info_name
            .lock()
            .map(|g| g.to_bytes().to_vec())
            .unwrap_or_default()
    }

    pub(crate) fn output_filename_bytes(&self) -> Vec<u8> {
        self.output_filename
            .lock()
            .map(|g| g.to_bytes().to_vec())
            .unwrap_or_default()
    }

    pub(crate) fn original_filename_bytes(&self) -> Option<Vec<u8>> {
        self.original_filename
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|c| c.to_bytes().to_vec()))
    }

    fn emit_log(&self, level: c_int, msg: &[u8]) {
        if level == 0 && !self.debug_on {
            return;
        }
        let kind = match level {
            0 => 3,
            1 => 4,
            2 => 1,
            3 => 2,
            _ => 0,
        };
        crate::log::log_active_bytes(kind, msg);
    }

    fn execute(&self, op: c_int, max_redirects: c_int) -> c_int {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        if op == 1 {
            self.emit_log(0, b"Entering WebDownloader-loop");
        }
        let opts = crate::options::active_options();
        let (write_buffer_kb, url_retries, url_interval_sec, url_timeout) =
            opts.as_deref().map_or((0, 1, 0, 60), |o| {
                let sc = o.scalars();
                (
                    sc.write_buffer,
                    sc.url_retries,
                    sc.url_interval,
                    sc.url_timeout,
                )
            });
        let tls_disabled = crate::ffi::build_tls_disabled();
        let gzip_disabled = crate::ffi::build_gzip_disabled();
        let initial_url = self
            .url
            .lock()
            .map(|g| g.to_bytes().to_vec())
            .unwrap_or_default();
        let out_fn = self.output_filename_bytes();
        let info_nm = self.info_name_bytes();
        let cfg = WebDlConfig {
            initial_url: &initial_url,
            output_filename: &out_fn,
            info_name: &info_nm,
            write_buffer_kb,
            url_retries,
            url_interval_sec,
            retry: self.retry.load(Ordering::SeqCst),
            force: self.force.load(Ordering::SeqCst),
            tls_disabled,
            gzip_disabled,
        };
        let mut io = DirectWebDlIo {
            dl: self,
            conn: None,
            url_timeout,
        };
        let st = if op == 1 {
            webdl_run_loop(&cfg, &mut io)
        } else {
            webdl_download_with_redirects(max_redirects, &cfg, &mut io)
        };
        if op == 1 {
            self.emit_log(0, b"Exiting WebDownloader-loop");
        }
        st as c_int
    }

    pub(crate) fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.emit_log(0, b"Trying to stop WebDownloader");
        if let Ok(g) = self.active_slot.lock() {
            if let Some(slot) = g.as_ref() {
                slot.cancel(true, self.conn_host_cb);
            }
        }
        self.emit_log(0, b"WebDownloader stopped successfully");
    }

    pub(crate) fn log_debug_info(&self) {
        let mut tbuf = [0u8; 50];
        crate::util::util_format_time(self.last_update_time.load(Ordering::SeqCst), &mut tbuf);
        let tlen = tbuf.iter().position(|&b| b == 0).unwrap_or(0);
        let out_b = self.output_filename_bytes();
        let base = &out_b[crate::paths::base_file_name(&out_b)..];
        let st_s = self.status.load(Ordering::SeqCst).to_string();
        let msg = join_c_slices(&[
            b"      Web-Download: status=",
            st_s.as_bytes(),
            b", LastUpdateTime=",
            &tbuf[..tlen],
            b", filename=",
            base,
        ]);
        self.emit_log(4, &msg);
    }
}

struct DirectWebDlIo<'a> {
    dl: &'a NzbgetRsWebDl,
    conn: Option<NzbgetRsConn>,
    url_timeout: c_int,
}

impl Drop for DirectWebDlIo<'_> {
    fn drop(&mut self) {
        self.free_conn();
    }
}

impl WebDlIo for DirectWebDlIo<'_> {
    fn connect(&mut self, host: &CStr, port: i32, tls: bool) -> bool {
        self.free_conn();
        let mut conn = NzbgetRsConn::new_client(
            host.to_bytes(),
            port,
            tls,
            true,
            self.dl.debug_on,
            0,
            self.dl.conn_host_cb,
        );
        let sess = conn.session.get_mut();
        sess.core.cert_verif_level = self.dl.cert_verif_level.load(Ordering::SeqCst) as u32;
        sess.core.timeout = self.url_timeout;
        conn.slot.set_suppress_errors(false);
        if let Ok(mut g) = self.dl.active_slot.lock() {
            *g = Some(Arc::clone(&conn.slot));
        }
        let ok = conn.session.get_mut().core.connect_base();
        self.conn = Some(conn);
        ok
    }

    fn free_conn(&mut self) {
        if let Some(mut conn) = self.conn.take() {
            self.dl.emit_log(0, b"Releasing connection");
            if let Ok(mut g) = self.dl.active_slot.lock() {
                *g = None;
            }
            if conn.slot.status.load(Ordering::SeqCst) == CS_CANCELLED {
                conn.session.get_mut().core.disconnect_base();
            }
        }
    }

    fn write_line(&mut self, line: &CStr) {
        if let Some(conn) = self.conn.as_mut() {
            conn.session
                .get_mut()
                .core
                .write_line_bytes(line.to_bytes());
        }
    }

    fn read_line(&mut self, buf: &mut [u8]) -> Option<usize> {
        self.conn
            .as_mut()?
            .session
            .get_mut()
            .core
            .read_line_slice(buf)
    }

    fn read_body_chunk(&mut self, buf: &mut [u8]) -> i32 {
        let Some(conn) = self.conn.as_mut() else {
            return -1;
        };
        let core = &mut conn.session.get_mut().core;
        if core.buf_avail > 0 {
            let avail = core.buf_avail;
            let off = core.buf_offset;
            core.buf_avail = 0;
            let copy_len = avail.min(buf.len());
            buf[..copy_len].copy_from_slice(&core.read_buf[off..off + copy_len]);
            return copy_len as i32;
        }
        core.try_recv_slice(buf)
    }

    fn is_stopped(&mut self) -> bool {
        crate::ffi::is_rust_thread_stopped() || self.dl.stopped.load(Ordering::SeqCst)
    }

    fn is_paused(&mut self) -> bool {
        #[cfg(unix)]
        {
            crate::ffi::active_workstate().is_some_and(|w| w.pause_download())
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    fn touch_time(&mut self) {
        self.dl
            .last_update_time
            .store(now_unix_sec(), Ordering::SeqCst);
    }

    fn sleep_100ms(&mut self) {
        std::thread::sleep(Duration::from_millis(100));
    }

    fn set_status(&mut self, status: WebDlStatus) {
        let s = status as c_int;
        self.dl.status.store(s, Ordering::SeqCst);
        if let Some(ref cb) = self.dl.on_status {
            cb(self.dl, status);
        }
    }

    fn set_http_status(&mut self, http_status: i32) {
        self.dl.http_status.store(http_status, Ordering::SeqCst);
    }

    fn set_original_filename(&mut self, filename: &[u8]) {
        if let Ok(mut g) = self.dl.original_filename.lock() {
            *g = Some(bytes_to_cstring(filename));
        }
    }

    fn set_url(&mut self, url: &[u8]) {
        if let Ok(mut g) = self.dl.url.lock() {
            *g = bytes_to_cstring(url);
        }
    }

    fn on_header(&mut self, line: &CStr) {
        if let Some(ref cb) = self.dl.on_header {
            cb(self.dl, line.to_bytes());
        }
    }

    fn on_log(&mut self, level: i32, msg: &[u8]) {
        self.dl.emit_log(level, msg);
    }
}

struct ActiveProc {
    id: u64,
    stopped: Arc<AtomicBool>,
    conn_slot: Arc<ConnCancelSlot>,
    cancel_reg: Arc<NzbgetRsCancelReg>,
}

struct RemoteSrvState {
    listener_slot: Option<Arc<ConnCancelSlot>>,
    active_processors: Vec<ActiveProc>,
    next_proc_id: u64,
    last_cap_warning: i64,
}

struct RemoteSrvInstance {
    tls: bool,
    tls_disabled: bool,
    debug_on: bool,
    conn_host_cb: NzbgetRsConnHostFn,
    stopped: AtomicBool,
    state: Mutex<RemoteSrvState>,
}

impl RemoteSrvInstance {
    fn new(tls: bool) -> Arc<Self> {
        Arc::new(Self {
            tls,
            tls_disabled: crate::ffi::build_tls_disabled(),
            debug_on: crate::ffi::build_debug_enabled(),
            conn_host_cb: crate::ffi::tls_host_cb(),
            stopped: AtomicBool::new(false),
            state: Mutex::new(RemoteSrvState {
                listener_slot: None,
                active_processors: Vec::new(),
                next_proc_id: 1,
                last_cap_warning: 0,
            }),
        })
    }

    fn emit_log(&self, level: c_int, msg: &[u8]) {
        if level == 0 && !self.debug_on {
            return;
        }
        let kind = match level {
            0 => 3,
            1 => 1,
            _ => 2,
        };
        crate::log::log_active_bytes(kind, msg);
    }

    #[inline]
    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    fn run_loop(self: &Arc<Self>) {
        self.emit_log(0, b"Entering RemoteServer-loop");
        let opts = crate::options::active_options();
        let (port, remote_timeout, control_ip, secure_cert, secure_key) =
            opts.as_deref().map_or_else(
                || (0, 60, Vec::new(), Vec::new(), Vec::new()),
                |o| {
                    let sc = o.scalars();
                    (
                        if self.tls {
                            sc.secure_port
                        } else {
                            sc.control_port
                        },
                        sc.remote_timeout,
                        o.str_bytes(14).to_vec(),
                        o.str_bytes(21).to_vec(),
                        o.str_bytes(22).to_vec(),
                    )
                },
            );

        if self.tls && !self.tls_disabled {
            if secure_cert.is_empty()
                || !crate::fsops::file_exists(&secure_cert, &crate::fsops::posix_stat)
            {
                self.emit_log(
                    2,
                    b"Could not initialize TLS, secure certificate is not configured or the cert-file was not found. Check option <SecureCert>",
                );
                return;
            }
            if secure_key.is_empty()
                || !crate::fsops::file_exists(&secure_key, &crate::fsops::posix_stat)
            {
                self.emit_log(
                    2,
                    b"Could not initialize TLS, secure key is not configured or the key-file was not found. Check option <SecureKey>",
                );
                return;
            }
        }

        let mut listener: Option<NzbgetRsConn> = None;

        while !self.is_stopped() {
            let mut need_bind = false;
            {
                let Ok(mut g) = self.state.lock() else {
                    break;
                };
                if self.is_stopped() {
                    break;
                }
                if listener.is_none() {
                    let mut c = NzbgetRsConn::new_client(
                        &control_ip,
                        port,
                        self.tls,
                        true,
                        self.debug_on,
                        0,
                        self.conn_host_cb,
                    );
                    c.session.get_mut().core.timeout = remote_timeout;
                    c.slot.set_suppress_errors(false);
                    g.listener_slot = Some(Arc::clone(&c.slot));
                    listener = Some(c);
                    need_bind = true;
                }
            }

            let bound = match listener.as_mut() {
                Some(c) => !need_bind || c.session.get_mut().core.bind(),
                None => false,
            };
            if self.is_stopped() {
                break;
            }

            let mut accepted_conn: Option<Box<NzbgetRsConn>> = None;
            let mut accept_busy = false;
            if bound {
                if let Some(c) = listener.as_mut() {
                    let fd = c.session.get_mut().core.accept_fd();
                    accept_busy = c.slot.accept_busy.load(Ordering::SeqCst);
                    if fd != INVALID_SOCKET {
                        accepted_conn = Some(Box::new(NzbgetRsConn::from_accepted(
                            fd,
                            self.tls,
                            remote_timeout,
                            self.debug_on,
                            0,
                            self.conn_host_cb,
                        )));
                    }
                }
            }
            if bound && accepted_conn.is_none() && accept_busy && !self.is_stopped() {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            let Some(acc) = accepted_conn else {
                if self.is_stopped() {
                    break;
                }
                if let Ok(mut g) = self.state.lock() {
                    g.listener_slot = None;
                }
                listener = None;
                std::thread::sleep(Duration::from_millis(500));
                continue;
            };

            let mut spawn_failed = false;
            {
                let Ok(mut g) = self.state.lock() else {
                    break;
                };
                if self.is_stopped() {
                    drop(acc);
                    break;
                }
                if g.active_processors.len() >= MAX_PROCESSORS {
                    let now = now_unix_sec();
                    if now - g.last_cap_warning >= 60 {
                        g.last_cap_warning = now;
                        let msg = format!(
                            "Too many connections to the web interface/API ({MAX_PROCESSORS}); refusing new ones"
                        );
                        self.emit_log(1, msg.as_bytes());
                    }
                    drop(acc);
                    continue;
                }
                let proc_id = g.next_proc_id;
                g.next_proc_id = g.next_proc_id.wrapping_add(1);
                let proc_stopped = Arc::new(AtomicBool::new(false));
                let proc_reg = Arc::new(NzbgetRsCancelReg::new());
                let proc_slot = Arc::clone(&acc.slot);
                g.active_processors.push(ActiveProc {
                    id: proc_id,
                    stopped: Arc::clone(&proc_stopped),
                    conn_slot: proc_slot,
                    cancel_reg: Arc::clone(&proc_reg),
                });
                let srv_clone = Arc::clone(self);
                let spawn_res = std::thread::Builder::new().spawn(move || {
                    #[cfg(unix)]
                    let _tg = crate::ffi::RustThreadGuard::enter(&proc_stopped);
                    unsafe {
                        run_request_processor(*acc, &proc_reg, srv_clone.tls);
                    }
                    drop(proc_reg);
                    crate::ffi::stop_ssl_thread();
                    srv_clone.on_processor_done(proc_id);
                });
                if let Err(e) = spawn_res {
                    g.active_processors.retain(|p| p.id != proc_id);
                    let msg =
                        format!("Could not start a thread for a web interface/API connection: {e}");
                    self.emit_log(1, msg.as_bytes());
                    spawn_failed = true;
                }
            }
            if spawn_failed {
                std::thread::sleep(Duration::from_millis(200));
            }
        }

        if let Ok(mut g) = self.state.lock() {
            g.listener_slot = None;
        }
        if let Some(mut c) = listener.take() {
            c.session.get_mut().core.disconnect_base();
        }

        self.emit_log(
            0,
            b"RemoteServer: waiting for request processor to complete",
        );
        loop {
            let done = self
                .state
                .lock()
                .map(|g| g.active_processors.is_empty())
                .unwrap_or(true);
            if done {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        self.emit_log(0, b"RemoteServer: request processor are completed");
        self.emit_log(0, b"Exiting RemoteServer-loop");
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.emit_log(0, b"Stopping RequestProcessors");
        if let Ok(g) = self.state.lock() {
            if let Some(slot) = g.listener_slot.as_ref() {
                slot.cancel(true, self.conn_host_cb);
            }
            for p in &g.active_processors {
                p.stopped.store(true, Ordering::SeqCst);
                p.conn_slot.cancel(false, self.conn_host_cb);
                p.cancel_reg.stop_all();
            }
        }
        self.emit_log(0, b"RequestProcessors are notified");
        self.emit_log(0, b"RemoteServer stop end");
    }

    fn on_processor_done(&self, proc_id: u64) {
        self.emit_log(0, b"Notification from RequestProcessor received");
        if let Ok(mut g) = self.state.lock() {
            if let Some(pos) = g.active_processors.iter().position(|p| p.id == proc_id) {
                g.active_processors.remove(pos);
            }
        }
    }
}

unsafe fn exec_request_processor(conn: &mut NzbgetRsConn, tls: bool) {
    conn.slot.set_suppress_errors(true);
    let debug_on = conn.slot.debug_on;
    let emit = |lvl: c_int, msg: &[u8]| {
        if lvl == 0 && !debug_on {
            return;
        }
        let kind = if lvl == 0 { 3 } else { 1 };
        crate::log::log_active_bytes(kind, msg);
    };

    let opts = crate::options::active_options();
    if tls && !crate::ffi::build_tls_disabled() {
        let cert = opts.as_deref().and_then(|o| o.str_cstr(21)).unwrap_or(c"");
        let key = opts.as_deref().and_then(|o| o.str_cstr(22)).unwrap_or(c"");
        let started = conn.session.get_mut().core.start_tls(false, cert, key);
        if !started {
            emit(
                0,
                b"Could not establish secure connection to web-client: Start TLS failed",
            );
            return;
        }
    }

    let mut sig = [0u8; 4];
    if !conn.session.get_mut().core.recv_exact_slice(&mut sig) {
        emit(0, b"Could not read request signature");
        return;
    }

    let mut ok = false;
    if u32::from_be_bytes(sig) == NZBMESSAGE_SIGNATURE {
        ok = true;
        #[cfg(unix)]
        crate::ffi::serve_binrpc_conn(conn);
    } else if &sig == b"POST" || &sig == b"GET " || &sig == b"OPTI" {
        ok = true;
        #[cfg(unix)]
        loop {
            if !crate::ffi::serve_web_conn(conn, &sig) {
                break;
            }
            if !conn.session.get_mut().core.recv_exact_slice(&mut sig) {
                emit(0, b"Could not read request signature");
                break;
            }
        }
        let core = &mut conn.session.get_mut().core;
        core.graceful = true;
        core.disconnect_base();
    }

    if !ok {
        let port = opts.as_deref().map_or(0, |o| {
            let sc = o.scalars();
            if tls {
                sc.secure_port
            } else {
                sc.control_port
            }
        });
        let ra = conn
            .session
            .get_mut()
            .core
            .get_remote_addr()
            .to_bytes()
            .to_vec();
        let ps = port.to_string();
        let msg = join_c_slices(&[
            b"Non-RustyGet request received on port ",
            ps.as_bytes(),
            b" from ",
            &ra,
        ]);
        emit(1, &msg);
    }
}

unsafe fn run_request_processor(
    mut conn: NzbgetRsConn,
    cancel_reg: &Arc<NzbgetRsCancelReg>,
    tls: bool,
) {
    let _scope = CancelRegScope::enter(Arc::clone(cancel_reg));
    exec_request_processor(&mut conn, tls);
    conn.session.get_mut().core.disconnect_base();
}

struct ActiveRemoteServers {
    plain: Option<(Arc<RemoteSrvInstance>, std::thread::JoinHandle<()>)>,
    secure: Option<(Arc<RemoteSrvInstance>, std::thread::JoinHandle<()>)>,
}

static ACTIVE_REMOTE_SERVERS: Mutex<ActiveRemoteServers> = Mutex::new(ActiveRemoteServers {
    plain: None,
    secure: None,
});

fn spawn_remote_server_listener(
    tls: bool,
) -> Option<(Arc<RemoteSrvInstance>, std::thread::JoinHandle<()>)> {
    let srv = RemoteSrvInstance::new(tls);
    let srv_thread = Arc::clone(&srv);
    let handle = std::thread::Builder::new()
        .spawn(move || {
            #[cfg(unix)]
            let _tg = crate::ffi::RustThreadGuard::enter(&srv_thread.stopped);
            srv_thread.run_loop();
            crate::ffi::stop_ssl_thread();
        })
        .ok()?;
    Some((srv, handle))
}

pub(crate) fn remote_srv_ctl(op: c_int) {
    match op {
        0 => {
            if !crate::options::active_options().is_some_and(|o| o.server_mode()) {
                return;
            }
            crate::webserver::init_server_tokens(|m| crate::log::log_active_bytes(2, m));
            let plain = spawn_remote_server_listener(false);
            let secure = if crate::lifecycle::should_start_secure_server_active() {
                spawn_remote_server_listener(true)
            } else {
                None
            };
            if let Ok(mut g) = ACTIVE_REMOTE_SERVERS.lock() {
                g.plain = plain;
                g.secure = secure;
            }
        }
        1 => {
            let (plain, secure) = match ACTIVE_REMOTE_SERVERS.lock() {
                Ok(mut g) => (g.plain.take(), g.secure.take()),
                Err(_) => return,
            };
            if let Some((ref srv, _)) = plain {
                if crate::ffi::build_debug_enabled() {
                    crate::log::log_active_bytes(3, b"stopping RemoteServer");
                }
                srv.stop();
            }
            if let Some((ref srv, _)) = secure {
                if crate::ffi::build_debug_enabled() {
                    crate::log::log_active_bytes(3, b"stopping RemoteSecureServer");
                }
                srv.stop();
            }
            if let Some((_, h)) = plain {
                let _ = h.join();
            }
            if let Some((_, h)) = secure {
                let _ = h.join();
            }
            if crate::ffi::build_debug_enabled() {
                crate::log::log_active_bytes(3, b"RemoteServer stopped");
            }
        }
        _ => {}
    }
}

/// Default Rust-backed `conn_cb` implementation for `NServMain` (`nserv.rs`).
pub unsafe extern "C" fn nserv_default_conn_cb(
    tls_host_cb: NzbgetRsConnHostFn,
    debug_on: bool,
    raw_conn: usize,
    op: c_int,
    a1: c_int,
    a2: c_int,
    s1: *const c_char,
    s2: *const c_char,
    out_buf: *mut c_char,
) -> usize {
    if op == 0 {
        let mut conn = Box::new(NzbgetRsConn::new_client(
            opt_bytes(s1),
            a1,
            !s2.is_null(),
            true,
            debug_on,
            0,
            tls_host_cb,
        ));
        conn.session.get_mut().core.timeout = 10;
        conn.slot.set_suppress_errors(false);
        return Box::into_raw(conn) as usize;
    }
    if raw_conn == 0 {
        return 0;
    }
    let conn = &*(raw_conn as *const NzbgetRsConn);
    match op {
        1 => usize::from((*conn.session.get()).core.bind()),
        2 => {
            let (fd, tls, timeout) = {
                let core = &mut (*conn.session.get()).core;
                (core.accept_fd(), core.tls, core.timeout)
            };
            if fd == INVALID_SOCKET {
                0
            } else {
                let acc = Box::new(NzbgetRsConn::from_accepted(
                    fd,
                    tls,
                    timeout,
                    debug_on,
                    0,
                    tls_host_cb,
                ));
                Box::into_raw(acc) as usize
            }
        }
        3 => {
            conn.slot.cancel(true, tls_host_cb);
            1
        }
        4 => {
            {
                let core = &mut (*conn.session.get()).core;
                if a1 == 1 {
                    core.graceful = true;
                }
                if a1 == 1 || a1 == 2 || a2 == 1 {
                    core.disconnect_base();
                }
            }
            drop(Box::from_raw(raw_conn as *mut NzbgetRsConn));
            if a2 == 1 {
                if let Some(cb) = tls_host_cb {
                    cb(
                        ptr::null_mut(),
                        10,
                        0,
                        0,
                        0,
                        ptr::null(),
                        ptr::null(),
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            }
            0
        }
        5 => {
            conn.slot.set_suppress_errors(false);
            if !s1.is_null() {
                let cert = CStr::from_ptr(s1);
                let key = if s2.is_null() {
                    c""
                } else {
                    CStr::from_ptr(s2)
                };
                if !(*conn.session.get()).core.start_tls(false, cert, key) {
                    return 0;
                }
            }
            if !out_buf.is_null() && a1 > 0 {
                let hb = conn.host.to_bytes();
                let copy_len = hb.len().min((a1 as usize).saturating_sub(1));
                if copy_len > 0 {
                    ptr::copy_nonoverlapping(hb.as_ptr(), out_buf.cast::<u8>(), copy_len);
                }
                *out_buf.add(copy_len) = 0;
            }
            1
        }
        6 => {
            if out_buf.is_null() || a1 <= 0 {
                0
            } else {
                let dst = std::slice::from_raw_parts_mut(out_buf.cast::<u8>(), a1 as usize);
                usize::from((*conn.session.get()).core.read_line_slice(dst).is_some())
            }
        }
        7 => {
            let core = &mut (*conn.session.get()).core;
            if a2 != 0 {
                core.write_line_bytes(opt_bytes(s1));
            } else if !s1.is_null() && a1 > 0 {
                let data = std::slice::from_raw_parts(s1.cast::<u8>(), a1 as usize);
                core.send_bytes(data);
            }
            1
        }
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// FFI entry points
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_tls_suppressed(slot: usize) -> c_int {
    if slot == 0 {
        0
    } else {
        c_int::from((&*(slot as *const AtomicBool)).load(Ordering::SeqCst))
    }
}

#[cfg(test)]
pub(crate) unsafe fn nzbget_rs_conn_new(
    host: *const c_char,
    port: c_int,
    tls: c_int,
    reg_cancel: c_int,
    debug_on: c_int,
    owner_ctx: *mut c_void,
    host_cb: NzbgetRsConnHostFn,
) -> *mut NzbgetRsConn {
    Box::into_raw(Box::new(NzbgetRsConn::new_client(
        opt_bytes(host),
        port,
        tls != 0,
        reg_cancel != 0,
        debug_on != 0,
        owner_ctx as usize,
        host_cb,
    )))
}

#[cfg(test)]
pub(crate) unsafe fn nzbget_rs_conn_from_fd(
    fd: c_int,
    tls: c_int,
    timeout: c_int,
    debug_on: c_int,
    owner_ctx: *mut c_void,
    host_cb: NzbgetRsConnHostFn,
) -> *mut NzbgetRsConn {
    Box::into_raw(Box::new(NzbgetRsConn::from_accepted(
        fd,
        tls != 0,
        timeout,
        debug_on != 0,
        owner_ctx as usize,
        host_cb,
    )))
}

#[cfg(test)]
pub(crate) unsafe fn nzbget_rs_conn_free(conn: *mut NzbgetRsConn) {
    if !conn.is_null() {
        drop(Box::from_raw(conn));
    }
}

#[cfg(test)]
pub(crate) unsafe fn nzbget_rs_conn_ctl(
    conn: *mut NzbgetRsConn,
    op: c_int,
    a1: c_int,
    p1: usize,
    s1: *const c_char,
    s2: *const c_char,
    addrs: *const *const c_char,
    addrs_len: usize,
) -> isize {
    if op == 23 {
        if p1 == 0 {
            return 0;
        }
        return isize::from((&*(p1 as *const AtomicBool)).load(Ordering::SeqCst));
    }
    if conn.is_null() {
        return 0;
    }
    let c = &*(conn as *const NzbgetRsConn);
    match op {
        3 => {
            c.slot.cancel(false, c.slot.host_cb);
            return 1;
        }
        5 => {
            c.slot.set_suppress_errors(a1 != 0);
            return 1;
        }
        11 => {
            c.slot.status.store(CS_BROKEN, Ordering::SeqCst);
            return 1;
        }
        14 => return c.slot.status.load(Ordering::SeqCst) as isize,
        15 => return isize::from(c.slot.accept_busy.load(Ordering::SeqCst)),
        16 => return c.slot.total_bytes_read.swap(0, Ordering::SeqCst) as isize,
        17 => return c.slot.tls_probe_failure.load(Ordering::SeqCst) as isize,
        19 => return c.host.as_ptr() as isize,
        22 => {
            c.slot.owner_ctx.store(p1, Ordering::SeqCst);
            return 1;
        }
        _ => {}
    }

    let core = &mut (*c.session.get()).core;
    match op {
        0 => isize::from(core.connect_base()),
        1 => isize::from(core.disconnect_base()),
        2 => isize::from(core.bind()),
        4 => {
            let cert = if s1.is_null() {
                c""
            } else {
                CStr::from_ptr(s1)
            };
            let key = if s2.is_null() {
                c""
            } else {
                CStr::from_ptr(s2)
            };
            isize::from(core.start_tls(a1 != 0, cert, key))
        }
        6 => {
            core.timeout = a1;
            1
        }
        7 => {
            core.cipher = bytes_to_cstring(opt_bytes(s1));
            1
        }
        8 => {
            core.ip_version = a1;
            1
        }
        9 => {
            core.cert_verif_level = a1 as u32;
            1
        }
        10 => {
            core.graceful = a1 != 0;
            1
        }
        12 => {
            core.deadline_ptr = p1;
            1
        }
        13 => {
            core.resolved_addrs.clear();
            if !addrs.is_null() {
                core.resolved_addrs.reserve(addrs_len);
                for i in 0..addrs_len {
                    let p = *addrs.add(i);
                    core.resolved_addrs.push(opt_bytes(p).to_vec());
                }
            }
            1
        }
        18 => {
            if core.tls_box == 0 {
                return 0;
            }
            let mut ver: c_int = 0;
            if let Some(cb) = core.slot.host_cb {
                cb(
                    core.owner_ctx() as *mut c_void,
                    9,
                    0,
                    0,
                    core.tls_box,
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    &mut ver,
                );
            }
            ver as isize
        }
        20 => core.get_remote_addr().as_ptr() as isize,
        21 => {
            if core.tls_box == 0 {
                return c"".as_ptr() as isize;
            }
            let mut ver: c_int = 0;
            core.slot.host_cb.map_or(c"".as_ptr() as isize, |cb| {
                let p = cb(
                    core.owner_ctx() as *mut c_void,
                    9,
                    0,
                    0,
                    core.tls_box,
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    &mut ver,
                );
                if p == 0 {
                    c"".as_ptr() as isize
                } else {
                    p
                }
            })
        }
        _ => 0,
    }
}

#[cfg(test)]
pub(crate) unsafe fn nzbget_rs_conn_io(
    conn: *mut NzbgetRsConn,
    op: c_int,
    buf: *mut c_char,
    size: c_int,
    out_int: *mut c_int,
) -> c_int {
    if conn.is_null() {
        return if op == 4 { -1 } else { 0 };
    }
    let c = &*(conn as *const NzbgetRsConn);
    let core = &mut (*c.session.get()).core;
    match op {
        0 => core.write_line_bytes(opt_bytes(buf.cast_const())),
        1 => {
            let slice = if buf.is_null() || size <= 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(buf.cast::<u8>(), size as usize)
            };
            c_int::from(core.send_bytes(slice))
        }
        2 => {
            if buf.is_null() || size <= 0 {
                if !out_int.is_null() {
                    *out_int = 0;
                }
                return 0;
            }
            let dst = std::slice::from_raw_parts_mut(buf.cast::<u8>(), size as usize);
            match core.read_line_slice(dst) {
                Some(n) => {
                    if !out_int.is_null() {
                        *out_int = n as c_int;
                    }
                    1
                }
                None => {
                    if !out_int.is_null() {
                        *out_int = 0;
                    }
                    0
                }
            }
        }
        3 => {
            if size <= 0 {
                return 1;
            }
            if buf.is_null() {
                return 0;
            }
            let dst = std::slice::from_raw_parts_mut(buf.cast::<u8>(), size as usize);
            c_int::from(core.recv_exact_slice(dst))
        }
        4 => {
            if buf.is_null() || size < 0 {
                return -1;
            }
            let dst = if size == 0 {
                &mut [][..]
            } else {
                std::slice::from_raw_parts_mut(buf.cast::<u8>(), size as usize)
            };
            core.try_recv_slice(dst)
        }
        5 => {
            if !out_int.is_null() {
                *out_int = core.buf_avail as c_int;
            }
            if !buf.is_null() {
                let p = core
                    .read_buf
                    .as_mut_ptr()
                    .add(core.buf_offset)
                    .cast::<c_char>();
                *(buf as *mut *mut c_char) = p;
            }
            core.buf_avail = 0;
            1
        }
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// NewsServer, PooledNntpConn, and NzbgetRsServerPool
// ---------------------------------------------------------------------------

const POOL_HOLD_SECONDS: i32 = 5;

pub struct RsNewsServer {
    pub id: i32,
    pub state_id: AtomicI32,
    pub active: AtomicBool,
    pub name: CString,
    pub host: CString,
    pub port: i32,
    pub ip_version: i32,
    pub user: CString,
    pub password: CString,
    pub join_group: bool,
    pub tls: bool,
    pub cipher: CString,
    pub max_connections: i32,
    pub retention: i32,
    pub level: i32,
    pub norm_level: AtomicI32,
    pub group: i32,
    pub optional: bool,
    pub block_time: AtomicI64,
    pub cert_verif_level: u32,
}

impl RsNewsServer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: i32,
        active: bool,
        name: &[u8],
        host: &[u8],
        port: i32,
        ip_version: i32,
        user: &[u8],
        password: &[u8],
        join_group: bool,
        tls: bool,
        cipher: &[u8],
        max_connections: i32,
        retention: i32,
        level: i32,
        group: i32,
        optional: bool,
        cert_verif_level: u32,
    ) -> Self {
        let name_cs = if name.is_empty() || name[0] == 0 {
            bytes_to_cstring(format!("Server{id}").as_bytes())
        } else {
            bytes_to_cstring(name)
        };
        Self {
            id,
            state_id: AtomicI32::new(0),
            active: AtomicBool::new(active),
            name: name_cs,
            host: bytes_to_cstring(host),
            port,
            ip_version,
            user: bytes_to_cstring(user),
            password: bytes_to_cstring(password),
            join_group,
            tls,
            cipher: bytes_to_cstring(cipher),
            max_connections,
            retention,
            level,
            norm_level: AtomicI32::new(level),
            group,
            optional,
            block_time: AtomicI64::new(0),
            cert_verif_level,
        }
    }
}

pub struct HealthServerSnapshot {
    pub id: i32,
    pub active: bool,
    pub name: Vec<u8>,
    pub level: i32,
    pub optional: bool,
    pub group: i32,
    pub host: Vec<u8>,
    pub port: i32,
    pub username: Vec<u8>,
    pub password: Vec<u8>,
    pub tls: bool,
    pub cipher: Vec<u8>,
    pub max_connections: i32,
    pub retention: i32,
    pub cert_level: u32,
    pub ip_version: i32,
}

pub struct PooledNntpConn {
    pub server: Arc<RsNewsServer>,
    pub conn: NzbgetRsConn,
    pub in_use: AtomicBool,
    pub free_time: AtomicI64,
}

impl PooledNntpConn {
    pub fn new(
        server: Arc<RsNewsServer>,
        timeout: i32,
        debug_on: bool,
        conn_host_cb: NzbgetRsConnHostFn,
    ) -> Self {
        let mut conn = NzbgetRsConn::new_client(
            server.host.to_bytes(),
            server.port,
            server.tls,
            false,
            debug_on,
            0,
            conn_host_cb,
        );
        {
            let core = &mut conn.session.get_mut().core;
            core.cipher = server.cipher.clone();
            core.ip_version = match server.ip_version {
                4 => 4,
                6 => 6,
                _ => 0,
            };
            core.cert_verif_level = server.cert_verif_level;
            core.timeout = timeout;
        }
        Self {
            server,
            conn,
            in_use: AtomicBool::new(false),
            free_time: AtomicI64::new(0),
        }
    }

    #[inline]
    pub fn is_connected(&self) -> bool {
        self.conn.slot.status.load(Ordering::SeqCst) == CS_CONNECTED
    }

    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.conn.slot.status.load(Ordering::SeqCst) == CS_CANCELLED
    }

    #[inline]
    pub fn set_suppress_errors(&self, suppress: bool) {
        self.conn.slot.set_suppress_errors(suppress);
    }

    #[inline]
    pub fn cancel(&self) {
        self.conn.slot.cancel(false, self.conn.slot.host_cb);
    }

    #[inline]
    pub fn set_broken(&self) {
        self.conn.slot.status.store(CS_BROKEN, Ordering::SeqCst);
    }

    #[inline]
    pub fn fetch_total_bytes_read(&self) -> i32 {
        self.conn.slot.total_bytes_read.swap(0, Ordering::SeqCst)
    }

    #[inline]
    pub fn host_cstr(&self) -> &CStr {
        &self.conn.host
    }

    /// Owner-thread: prepare cert verification level & clear error suppression before connect/request.
    pub unsafe fn prepare_for_use(&self) {
        self.set_suppress_errors(false);
        (*self.conn.session.get()).core.cert_verif_level = self.server.cert_verif_level;
    }

    pub unsafe fn connect(&self) -> bool {
        (*self.conn.session.get()).nntp_connect(
            &self.server.user,
            &self.server.password,
            &self.server.name,
            &self.server.host,
        )
    }

    pub unsafe fn disconnect(&self) -> bool {
        (*self.conn.session.get()).nntp_disconnect(
            &self.server.user,
            &self.server.password,
            &self.server.name,
            &self.server.host,
        )
    }

    pub unsafe fn request(&self, req: *const c_char) -> *const c_char {
        (*self.conn.session.get()).nntp_request(
            req,
            &self.server.user,
            &self.server.password,
            &self.server.name,
            &self.server.host,
        )
    }

    pub unsafe fn join_group(&self, grp: *const c_char) -> *const c_char {
        (*self.conn.session.get()).nntp_join_group(
            grp,
            &self.server.user,
            &self.server.password,
            &self.server.name,
            &self.server.host,
        )
    }

    pub unsafe fn auth_error(&self) -> bool {
        (*self.conn.session.get()).nntp_auth.error
    }

    pub unsafe fn auth_rejected(&self) -> bool {
        (*self.conn.session.get()).nntp_auth.rejected
    }

    pub unsafe fn read_line(&self, dst: &mut [u8]) -> Option<usize> {
        (*self.conn.session.get()).core.read_line_slice(dst)
    }

    pub unsafe fn drain_read_buffer(&self) -> (*mut u8, usize) {
        let core = &mut (*self.conn.session.get()).core;
        let avail = core.buf_avail;
        let p = core.read_buf.as_mut_ptr().add(core.buf_offset);
        core.buf_avail = 0;
        (p, avail)
    }

    pub unsafe fn try_recv(&self, dst: &mut [u8]) -> c_int {
        (*self.conn.session.get()).core.try_recv_slice(dst)
    }

    pub unsafe fn send_bytes(&self, bytes: &[u8]) -> bool {
        (*self.conn.session.get()).core.send_bytes(bytes)
    }
}

struct ServerPoolInner {
    sorted_servers: Vec<Arc<RsNewsServer>>,
    connections: Vec<Box<PooledNntpConn>>,
    levels: Vec<i32>,
}

pub struct NzbgetRsServerPool {
    servers: std::sync::RwLock<Vec<Arc<RsNewsServer>>>,
    inner: Mutex<ServerPoolInner>,
    max_norm_level: AtomicI32,
    generation: AtomicI32,
    timeout: AtomicI32,
    retry_interval: AtomicI32,
    conn_host_cb: NzbgetRsConnHostFn,
}

static ACTIVE_SERVER_POOL: std::sync::RwLock<Option<std::sync::Weak<NzbgetRsServerPool>>> =
    std::sync::RwLock::new(None);

pub(crate) fn active_server_pool() -> Option<Arc<NzbgetRsServerPool>> {
    ACTIVE_SERVER_POOL
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(std::sync::Weak::upgrade))
}

impl NzbgetRsServerPool {
    pub fn new(conn_host_cb: NzbgetRsConnHostFn) -> Self {
        Self {
            servers: std::sync::RwLock::new(Vec::new()),
            inner: Mutex::new(ServerPoolInner {
                sorted_servers: Vec::new(),
                connections: Vec::new(),
                levels: Vec::new(),
            }),
            max_norm_level: AtomicI32::new(0),
            generation: AtomicI32::new(0),
            timeout: AtomicI32::new(60),
            retry_interval: AtomicI32::new(0),
            conn_host_cb,
        }
    }

    pub fn add_server(&self, server: Arc<RsNewsServer>) {
        crate::log::log_active_bytes(3, b"Adding server to ServerPool");
        if let Ok(mut srvs) = self.servers.write() {
            srvs.push(Arc::clone(&server));
        }
        if let Ok(mut g) = self.inner.lock() {
            g.sorted_servers.push(server);
        }
    }

    #[inline]
    pub fn max_norm_level(&self) -> i32 {
        self.max_norm_level.load(Ordering::SeqCst)
    }

    #[inline]
    pub fn generation(&self) -> i32 {
        self.generation.load(Ordering::SeqCst)
    }

    #[inline]
    pub fn retry_interval(&self) -> i32 {
        self.retry_interval.load(Ordering::SeqCst)
    }

    #[inline]
    pub(crate) fn set_timeout(&self, timeout: i32) {
        self.timeout.store(timeout, Ordering::SeqCst);
    }

    #[inline]
    pub(crate) fn set_retry_interval(&self, retry_interval: i32) {
        self.retry_interval.store(retry_interval, Ordering::SeqCst);
    }

    pub fn server_count(&self) -> usize {
        self.servers.read().map_or(0, |g| g.len())
    }

    pub fn servers_snapshot(&self) -> Vec<Arc<RsNewsServer>> {
        self.servers
            .read()
            .map_or_else(|_| Vec::new(), |g| g.clone())
    }

    pub fn server_by_id(&self, id: i32) -> Option<Arc<RsNewsServer>> {
        let g = self.servers.read().ok()?;
        g.iter().find(|s| s.id == id).cloned()
    }

    pub fn is_server_blocked(&self, srv: &RsNewsServer) -> bool {
        crate::dupefallback::server_pool_is_blocked(
            srv.block_time.load(Ordering::SeqCst),
            now_unix_sec(),
            self.retry_interval(),
        )
    }

    pub fn block_server(&self, srv: &RsNewsServer) {
        let retry_interval = self.retry_interval();
        let prev_block = {
            let _guard = self.inner.lock();
            let cur_time = now_unix_sec();
            let prev = srv.block_time.swap(cur_time, Ordering::SeqCst);
            (prev, cur_time)
        };
        let (_new_block, should_warn) =
            crate::nntp::server_pool_eval_block_server(prev_block.0, prev_block.1, retry_interval);
        if should_warn {
            let tail = format!(") for {retry_interval} sec");
            crate::log::log_active_bytes(
                1,
                &join_c_slices(&[
                    b"Blocking ",
                    srv.name.to_bytes(),
                    b" (",
                    srv.host.to_bytes(),
                    tail.as_bytes(),
                ]),
            );
        }
    }

    pub fn init_connections(&self) {
        crate::log::log_active_bytes(3, b"Initializing connections in ServerPool");
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        let srv_count = g.sorted_servers.len();
        if srv_count > 0 {
            let inputs: Vec<crate::dupefallback::NzbgetRsServerNormInput> = g
                .sorted_servers
                .iter()
                .map(|s| crate::dupefallback::NzbgetRsServerNormInput {
                    level: s.level,
                    active: i32::from(s.active.load(Ordering::SeqCst)),
                    max_connections: s.max_connections,
                })
                .collect();
            let mut sorted_idx = vec![0usize; srv_count];
            let mut norm_levels = vec![0i32; srv_count];
            let max_norm = crate::dupefallback::server_pool_normalize_levels(
                &inputs,
                &mut sorted_idx,
                &mut norm_levels,
            );
            for (i, &nl) in norm_levels.iter().enumerate() {
                g.sorted_servers[i].norm_level.store(nl, Ordering::SeqCst);
            }
            let new_sorted: Vec<Arc<RsNewsServer>> = sorted_idx
                .into_iter()
                .map(|idx| Arc::clone(&g.sorted_servers[idx]))
                .collect();
            g.sorted_servers = new_sorted;
            self.max_norm_level.store(max_norm, Ordering::SeqCst);
        }
        g.levels.clear();

        let timeout = self.timeout.load(Ordering::SeqCst);
        let debug_on = crate::log::active_debug_build();
        let sorted_clone = g.sorted_servers.clone();
        for ns in &sorted_clone {
            ns.block_time.store(0, Ordering::SeqCst);
            let nl = ns.norm_level.load(Ordering::SeqCst);
            let active = ns.active.load(Ordering::SeqCst);
            if nl > -1 {
                if (g.levels.len() as i32) <= nl {
                    g.levels.push(0);
                }
                if active {
                    let mut existing = 0i32;
                    let mut free_existing = 0i32;
                    for c in &g.connections {
                        if Arc::ptr_eq(&c.server, ns) {
                            existing += 1;
                            if !c.in_use.load(Ordering::SeqCst) {
                                free_existing += 1;
                            }
                        }
                    }
                    let mut to_add = 0usize;
                    if ns.max_connections > existing {
                        to_add = (ns.max_connections - existing) as usize;
                    }
                    for _ in 0..to_add {
                        g.connections.push(Box::new(PooledNntpConn::new(
                            Arc::clone(ns),
                            timeout,
                            debug_on,
                            self.conn_host_cb,
                        )));
                    }
                    g.levels[nl as usize] += free_existing + (to_add as i32);
                }
            }
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn get_connection(
        &self,
        level: i32,
        want_srv: Option<&RsNewsServer>,
        ignore_srvs: &[&RsNewsServer],
    ) -> *mut PooledNntpConn {
        if level < 0 {
            return ptr::null_mut();
        }
        let Ok(mut g) = self.inner.lock() else {
            return ptr::null_mut();
        };
        let srv_count = g.sorted_servers.len();
        let conn_count = g.connections.len();
        if srv_count == 0 || conn_count == 0 {
            return ptr::null_mut();
        }
        let cur_time = now_unix_sec();
        let retry_interval = self.retry_interval();

        let srv_views: Vec<crate::nntp::ServerPoolServerView> = g
            .sorted_servers
            .iter()
            .map(|s| crate::nntp::ServerPoolServerView {
                id: s.id,
                group: s.group,
                norm_level: s.norm_level.load(Ordering::SeqCst),
                active: s.active.load(Ordering::SeqCst),
                optional: s.optional,
                blocked: crate::dupefallback::server_pool_is_blocked(
                    s.block_time.load(Ordering::SeqCst),
                    cur_time,
                    retry_interval,
                ),
            })
            .collect();

        let conn_views: Vec<crate::nntp::ServerPoolConnView> = g
            .connections
            .iter()
            .map(|c| {
                let server_index = g
                    .sorted_servers
                    .iter()
                    .position(|s| Arc::ptr_eq(s, &c.server))
                    .unwrap_or(srv_count);
                crate::nntp::ServerPoolConnView {
                    server_index,
                    in_use: c.in_use.load(Ordering::SeqCst),
                    connected: c.is_connected(),
                }
            })
            .collect();

        let want_tuple = want_srv.map(|s| (s.id, s.group));
        let ignore_tuples: Vec<(i32, i32, i32)> = if want_srv.is_none() {
            ignore_srvs
                .iter()
                .map(|s| (s.id, s.group, s.norm_level.load(Ordering::SeqCst)))
                .collect()
        } else {
            Vec::new()
        };

        let rand_val = unsafe { libc::rand() };
        let mut unblock = Vec::new();
        let found = crate::nntp::server_pool_select_connection(
            level,
            &g.levels,
            &srv_views,
            &conn_views,
            want_tuple,
            &ignore_tuples,
            rand_val,
            &mut unblock,
        );
        for idx in unblock {
            g.sorted_servers[idx].block_time.store(0, Ordering::SeqCst);
        }
        if let Some((chosen_conn, chosen_level)) = found {
            g.levels[chosen_level as usize] -= 1;
            let c = &mut g.connections[chosen_conn];
            c.in_use.store(true, Ordering::SeqCst);
            (&mut **c) as *mut PooledNntpConn
        } else {
            ptr::null_mut()
        }
    }

    pub fn get_connection_by_server_id(&self, server_id: i32) -> *mut PooledNntpConn {
        let Some(srv) = self.server_by_id(server_id) else {
            return ptr::null_mut();
        };
        let nl = srv.norm_level.load(Ordering::SeqCst);
        self.get_connection(nl, Some(&srv), &[])
    }

    pub fn free_connection(&self, conn_ptr: *mut PooledNntpConn, used: bool) {
        if conn_ptr.is_null() {
            return;
        }
        if used {
            crate::log::log_active_bytes(3, b"Freeing used connection");
        }
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        let c = unsafe { &*conn_ptr };
        c.in_use.store(false, Ordering::SeqCst);
        if used {
            c.free_time.store(now_unix_sec(), Ordering::SeqCst);
        }
        let nl = c.server.norm_level.load(Ordering::SeqCst);
        if nl > -1 && c.server.active.load(Ordering::SeqCst) {
            if let Some(slot) = g.levels.get_mut(nl as usize) {
                *slot += 1;
            }
        }
    }

    pub fn close_unused_connections(&self) {
        let cur_time = now_unix_sec();
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        g.connections.retain(|c| {
            let in_use = c.in_use.load(Ordering::SeqCst);
            let nl = c.server.norm_level.load(Ordering::SeqCst);
            let active = c.server.active.load(Ordering::SeqCst);
            if !in_use && (nl == -1 || !active) {
                let msg = format!(
                    "Closing (and deleting) unused connection to server{}",
                    c.server.id
                );
                crate::log::log_active_bytes(3, msg.as_bytes());
                if c.is_connected() {
                    unsafe {
                        c.disconnect();
                    }
                }
                false
            } else {
                true
            }
        });

        let max_norm = self.max_norm_level.load(Ordering::SeqCst);
        for lvl in 0..=max_norm {
            let mut in_use = Vec::new();
            let mut connected = Vec::new();
            let mut free_times = Vec::new();
            for c in &g.connections {
                if c.server.norm_level.load(Ordering::SeqCst) == lvl {
                    in_use.push(u8::from(c.in_use.load(Ordering::SeqCst)));
                    connected.push(u8::from(c.is_connected()));
                    free_times.push(c.free_time.load(Ordering::SeqCst));
                }
            }
            if crate::dupefallback::server_pool_level_should_close(
                &in_use,
                &connected,
                &free_times,
                cur_time,
                POOL_HOLD_SECONDS,
            ) {
                for c in &g.connections {
                    if c.server.norm_level.load(Ordering::SeqCst) == lvl && c.is_connected() {
                        let msg = format!(
                            "Closing (and keeping) unused connection to server{}",
                            c.server.id
                        );
                        crate::log::log_active_bytes(3, msg.as_bytes());
                        unsafe {
                            c.disconnect();
                        }
                    }
                }
            }
        }
    }

    pub fn changed(&self) {
        crate::log::log_active_bytes(3, b"Server config has been changed");
        self.init_connections();
        self.close_unused_connections();
    }

    pub fn primary_connections(&self) -> i32 {
        let Ok(srvs) = self.servers.read() else {
            return 0;
        };
        srvs.iter()
            .filter(|s| s.active.load(Ordering::SeqCst) && s.norm_level.load(Ordering::SeqCst) == 0)
            .map(|s| s.max_connections)
            .sum()
    }

    pub fn collect_health_servers(&self) -> Vec<HealthServerSnapshot> {
        let Ok(srvs) = self.servers.read() else {
            return Vec::new();
        };
        srvs.iter()
            .map(|s| HealthServerSnapshot {
                id: s.id,
                active: s.active.load(Ordering::SeqCst),
                name: s.name.to_bytes().to_vec(),
                level: s.level,
                optional: s.optional,
                group: s.group,
                host: s.host.to_bytes().to_vec(),
                port: s.port,
                username: s.user.to_bytes().to_vec(),
                password: s.password.to_bytes().to_vec(),
                tls: s.tls,
                cipher: s.cipher.to_bytes().to_vec(),
                max_connections: s.max_connections,
                retention: s.retention,
                cert_level: s.cert_verif_level,
                ip_version: s.ip_version,
            })
            .collect()
    }

    pub fn log_debug_info(&self) {
        let srvs = self.servers_snapshot();
        let Ok(g) = self.inner.lock() else {
            return;
        };
        let cur_time = now_unix_sec();
        let max_norm_level = self.max_norm_level.load(Ordering::SeqCst);
        let retry_interval = self.retry_interval();
        let raw_servers_len = srvs.len();
        let levels_len = g.levels.len();
        let conn_count = g.connections.len();

        crate::log::log_active_bytes(0, b"   ---------- ServerPool");
        crate::log::log_active_bytes(0, format!("    Max-Level: {max_norm_level}").as_bytes());
        crate::log::log_active_bytes(0, format!("    Servers: {raw_servers_len}").as_bytes());

        for s in &srvs {
            let id = s.id;
            let lvl = s.level;
            let nlvl = s.norm_level.load(Ordering::SeqCst);
            let block_time = s.block_time.load(Ordering::SeqCst);
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
            let p1 = format!("      {id}) ");
            let p2 = format!(": Level={lvl}, NormLevel={nlvl}, BlockSec={block_sec}");
            crate::log::log_active_bytes(
                0,
                &join_c_slices(&[
                    p1.as_bytes(),
                    s.name.to_bytes(),
                    b" (",
                    s.host.to_bytes(),
                    b")",
                    p2.as_bytes(),
                ]),
            );
        }

        crate::log::log_active_bytes(0, format!("    Levels: {levels_len}").as_bytes());
        for (idx, &sz) in g.levels.iter().enumerate() {
            crate::log::log_active_bytes(
                0,
                format!("      {idx}: Free connections={sz}").as_bytes(),
            );
        }

        crate::log::log_active_bytes(0, format!("    Connections: {conn_count}").as_bytes());
        for c in &g.connections {
            let in_use = i32::from(c.in_use.load(Ordering::SeqCst));
            let id = c.server.id;
            let lvl = c.server.level;
            let nlvl = c.server.norm_level.load(Ordering::SeqCst);
            let p1 = format!("      {id}) ");
            let p2 = format!(": Level={lvl}, NormLevel={nlvl}, InUse:{in_use}");
            crate::log::log_active_bytes(
                0,
                &join_c_slices(&[
                    p1.as_bytes(),
                    c.server.name.to_bytes(),
                    b" (",
                    c.server.host.to_bytes(),
                    b")",
                    p2.as_bytes(),
                ]),
            );
        }
    }
    pub(crate) fn for_each_server_with_state_id(&self, state_id: i32, mut f: impl FnMut(i32)) {
        let Ok(srvs) = self.servers.read() else {
            return;
        };
        for s in srvs.iter() {
            if s.state_id.load(Ordering::SeqCst) == state_id {
                f(s.id);
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn nzbget_rs_serverpool_new() -> *mut NzbgetRsServerPool {
    let arc = Arc::new(NzbgetRsServerPool::new(crate::ffi::tls_host_cb()));
    if let Ok(mut g) = ACTIVE_SERVER_POOL.write() {
        *g = Some(Arc::downgrade(&arc));
    }
    if let Some(lg) = crate::log::active_log() {
        lg.register_debuggable(4);
    }
    Arc::into_raw(arc).cast_mut()
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_serverpool_free(pool: *mut NzbgetRsServerPool) {
    if !pool.is_null() {
        if let Some(lg) = crate::log::active_log() {
            lg.unregister_debuggable(4);
        }
        let arc = Arc::from_raw(pool.cast_const());
        let wk = Arc::downgrade(&arc);
        if let Ok(mut g) = ACTIVE_SERVER_POOL.write() {
            if g.as_ref()
                .is_some_and(|cur| std::sync::Weak::ptr_eq(cur, &wk))
            {
                *g = None;
            }
        }
        drop(arc);
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn nzbget_rs_serverpool_add(
    pool: *const NzbgetRsServerPool,
    id: c_int,
    active: c_int,
    name: *const c_char,
    host: *const c_char,
    port: c_int,
    ip_version: c_int,
    user: *const c_char,
    pass: *const c_char,
    join_group: c_int,
    tls: c_int,
    cipher: *const c_char,
    max_connections: c_int,
    retention: c_int,
    level: c_int,
    group: c_int,
    optional: c_int,
    cert_verif_level: u32,
) {
    let Some(p) = pool.as_ref() else {
        return;
    };
    p.add_server(Arc::new(RsNewsServer::new(
        id,
        active != 0,
        opt_bytes(name),
        opt_bytes(host),
        port,
        ip_version,
        opt_bytes(user),
        opt_bytes(pass),
        join_group != 0,
        tls != 0,
        opt_bytes(cipher),
        max_connections,
        retention,
        level,
        group,
        optional != 0,
        cert_verif_level,
    )));
}

pub(crate) unsafe fn nzbget_rs_serverpool_ctl(
    pool: *const NzbgetRsServerPool,
    op: c_int,
    a1: c_int,
    _a2: c_int,
    p1: usize,
    p2: usize,
) -> isize {
    let Some(p) = pool.as_ref() else {
        return 0;
    };
    match op {
        0 => {
            p.timeout.store(a1, Ordering::SeqCst);
            0
        }
        1 => {
            p.retry_interval.store(a1, Ordering::SeqCst);
            0
        }
        2 => {
            p.init_connections();
            0
        }
        3 => {
            p.close_unused_connections();
            0
        }
        4 => {
            p.changed();
            0
        }
        5 => {
            p.log_debug_info();
            0
        }
        6 => p.primary_connections() as isize,
        7 => {
            // RPC serveractivate: p1 = *const c_int pairs [id, active, ...], p2 = len
            if p1 == 0 || p2 < 2 {
                return 0;
            }
            let ids = std::slice::from_raw_parts(p1 as *const c_int, p2);
            let srvs = p.servers_snapshot();
            let mut ok = false;
            let mut i = 0;
            while i + 1 < ids.len() {
                for s in &srvs {
                    if s.id == ids[i] {
                        s.active.store(ids[i + 1] != 0, Ordering::SeqCst);
                        ok = true;
                    }
                }
                i += 2;
            }
            if ok {
                p.changed();
                1
            } else {
                0
            }
        }
        8 => {
            // ApplyServerStats iterator: a1 = state_id, p1 = *mut usize cursor -> returns matching server id or 0
            if p1 == 0 {
                return 0;
            }
            let cursor = &mut *(p1 as *mut usize);
            let Ok(srvs) = p.servers.read() else {
                return 0;
            };
            while *cursor < srvs.len() {
                let s = &srvs[*cursor];
                *cursor += 1;
                if s.state_id.load(Ordering::SeqCst) == a1 {
                    return s.id as isize;
                }
            }
            0
        }
        9 => {
            // RPC status server list: if p1 == 0 return count; else fill p1 as *mut NzbgetRsRpcServerStatus up to p2
            let Ok(srvs) = p.servers.read() else {
                return 0;
            };
            if p1 != 0 && p2 > 0 {
                let out = std::slice::from_raw_parts_mut(
                    p1 as *mut crate::xmlrpc_format::NzbgetRsRpcServerStatus,
                    p2.min(srvs.len()),
                );
                for (dst, s) in out.iter_mut().zip(srvs.iter()) {
                    dst.id = s.id;
                    dst.active = c_int::from(s.active.load(Ordering::SeqCst));
                }
            }
            srvs.len() as isize
        }
        10 => {
            // SpeedTest server validation: a1 = server_id, p1 = *mut *const c_char error_out
            let err_msg: Option<&CStr> = match p.server_by_id(a1) {
                None => Some(c"Server not found"),
                Some(s) if !s.active.load(Ordering::SeqCst) => Some(c"Server is not active"),
                Some(s) if s.max_connections == 0 => Some(c"Server has no connections configured"),
                Some(_) => None,
            };
            if let Some(m) = err_msg {
                if p1 != 0 {
                    *(p1 as *mut *const c_char) = m.as_ptr();
                }
                0
            } else {
                1
            }
        }
        11 => p.max_norm_level() as isize,
        12 => p.generation() as isize,
        _ => 0,
    }
}

struct TestConnTrap {
    real_cb: NzbgetRsConnHostFn,
    first_err: Option<Vec<u8>>,
}

unsafe extern "C" fn test_conn_host_cb(
    ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    buf: *mut c_char,
    out_int: *mut c_int,
) -> isize {
    let trap = &mut *(ctx as *mut TestConnTrap);
    if op == 1 && a1 == 0 {
        if trap.first_err.is_none() && !s1.is_null() {
            trap.first_err = Some(CStr::from_ptr(s1).to_bytes().to_vec());
        }
        return 0;
    }
    if let Some(cb) = trap.real_cb {
        cb(ptr::null_mut(), op, a1, a2, p1, s1, s2, s3, buf, out_int)
    } else {
        0
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn nzbget_rs_serverpool_test(
    host: *const c_char,
    port: c_int,
    user: *const c_char,
    pass: *const c_char,
    tls: c_int,
    cipher: *const c_char,
    timeout: c_int,
    cert_verif_level: u32,
    out_err: *mut crate::ffi::RsBuf,
) -> c_int {
    let conn_host_cb = crate::ffi::tls_host_cb();
    let srv = RsNewsServer::new(
        0,
        true,
        b"test server",
        opt_bytes(host),
        port,
        0,
        opt_bytes(user),
        opt_bytes(pass),
        false,
        tls != 0,
        opt_bytes(cipher),
        1,
        0,
        0,
        0,
        false,
        cert_verif_level,
    );
    let mut trap = TestConnTrap {
        real_cb: conn_host_cb,
        first_err: None,
    };
    let debug_on = crate::log::active_debug_build();
    let mut conn = NzbgetRsConn::new_client(
        srv.host.to_bytes(),
        srv.port,
        srv.tls,
        true,
        debug_on,
        (&mut trap as *mut TestConnTrap) as usize,
        Some(test_conn_host_cb),
    );
    let eff_timeout = if timeout > 0 {
        timeout
    } else {
        crate::options::active_options().map_or(60, |o| o.scalars().article_timeout)
    };
    {
        let core = &mut conn.session.get_mut().core;
        core.cipher = srv.cipher.clone();
        core.cert_verif_level = srv.cert_verif_level;
        core.timeout = eff_timeout;
    }
    conn.slot.set_suppress_errors(false);
    let mut ok =
        conn.session
            .get_mut()
            .nntp_connect(&srv.user, &srv.password, &srv.name, &srv.host);
    if ok {
        let mut id = String::with_capacity(48);
        while id.len() < 30 {
            use std::fmt::Write as _;
            let _ = write!(id, "{}", libc::rand());
        }
        let req = bytes_to_cstring(format!("ARTICLE <{id}@nzbget.com>\r\n").as_bytes());
        let resp = conn.session.get_mut().nntp_request(
            req.as_ptr(),
            &srv.user,
            &srv.password,
            &srv.name,
            &srv.host,
        );
        ok = !resp.is_null() && matches!(*resp as u8, b'4' | b'2');
    }
    if !ok {
        if let Some(err) = trap.first_err.take() {
            if !err.is_empty() && !out_err.is_null() {
                *out_err = crate::ffi::into_buf(err);
            }
        }
    }
    c_int::from(ok)
}

unsafe extern "C" fn probe_conn_host_cb(
    ctx: *mut c_void,
    op: c_int,
    a1: c_int,
    a2: c_int,
    p1: usize,
    s1: *const c_char,
    s2: *const c_char,
    s3: *const c_char,
    buf: *mut c_char,
    out_int: *mut c_int,
) -> isize {
    if op == 1 && a1 == 0 {
        return 0;
    }
    let real_cb: NzbgetRsConnHostFn = std::mem::transmute(ctx);
    if let Some(cb) = real_cb {
        cb(ptr::null_mut(), op, a1, a2, p1, s1, s2, s3, buf, out_int)
    } else {
        0
    }
}

#[repr(C)]
pub struct NzbgetTlsServer {
    pub id: u32,
    pub host: *const c_char,
    pub port: u16,
    pub ip_version: u8,
    pub verification: u8,
    pub username: *const c_char,
    pub password: *const c_char,
}

#[no_mangle]
pub unsafe extern "C" fn nzbget_rs_serverpool_probe_tls(
    server: *const NzbgetTlsServer,
    cipher: *const c_char,
    expected_ver: c_int,
    numeric: *const c_char,
    deadline_ptr: usize,
) -> c_int {
    let conn_host_cb = crate::ffi::tls_host_cb();
    let Some(srv_in) = server.as_ref() else {
        return -1;
    };
    let num_bytes = opt_bytes(numeric);
    let mut addrs: Vec<Vec<u8>> = Vec::new();
    let mut pos = 0;
    while pos < num_bytes.len() {
        let end = num_bytes[pos..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(num_bytes.len(), |d| pos + d);
        addrs.push(num_bytes[pos..end].to_vec());
        if end == num_bytes.len() {
            break;
        }
        pos = end + 1;
    }
    if addrs.is_empty() || addrs.len() > 16 {
        return -2;
    }

    let srv = RsNewsServer::new(
        srv_in.id as i32,
        true,
        b"TLS cipher probe",
        opt_bytes(srv_in.host),
        i32::from(srv_in.port),
        i32::from(srv_in.ip_version),
        opt_bytes(srv_in.username),
        opt_bytes(srv_in.password),
        false,
        true,
        opt_bytes(cipher),
        1,
        0,
        0,
        0,
        false,
        srv_in.verification as u32,
    );
    let debug_on = crate::log::active_debug_build();
    let real_cb_raw: usize = std::mem::transmute(conn_host_cb);
    let mut conn = NzbgetRsConn::new_client(
        srv.host.to_bytes(),
        srv.port,
        true,
        true,
        debug_on,
        real_cb_raw,
        Some(probe_conn_host_cb),
    );
    {
        let core = &mut conn.session.get_mut().core;
        core.cipher = srv.cipher.clone();
        core.ip_version = match srv.ip_version {
            4 => 4,
            6 => 6,
            _ => 0,
        };
        core.cert_verif_level = srv.cert_verif_level;
        core.resolved_addrs = addrs;
        core.deadline_ptr = deadline_ptr;
        core.timeout = 10;
    }
    conn.slot.set_suppress_errors(true);
    let connected =
        conn.session
            .get_mut()
            .nntp_connect(&srv.user, &srv.password, &srv.name, &srv.host);

    let mut neg_ver: c_int = 0;
    let mut neg_cipher: *const c_char = c"".as_ptr();
    let tb = conn.session.get_mut().core.tls_box;
    if tb != 0 {
        if let Some(cb) = conn_host_cb {
            let p = cb(
                ptr::null_mut(),
                9,
                0,
                0,
                tb,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                &mut neg_ver,
            );
            if p != 0 {
                neg_cipher = p as *const c_char;
            }
        }
    }
    let want_cipher = opt_bytes(cipher);
    let got_cipher = opt_bytes(neg_cipher);
    let result = if connected && got_cipher == want_cipher && neg_ver == expected_ver {
        1
    } else if conn.session.get_mut().nntp_auth.error {
        -5
    } else {
        conn.slot.tls_probe_failure.load(Ordering::SeqCst)
    };
    conn.slot.status.store(CS_BROKEN, Ordering::SeqCst);
    conn.session
        .get_mut()
        .nntp_disconnect(&srv.user, &srv.password, &srv.name, &srv.host);
    result
}

/// Executes RPC `readurl` (creating a unique `readurl-N.tmp` file in `temp_dir`,
/// downloading with up to 5 redirects, loading up to the first NUL into `out_buf`,
/// and removing the temporary file).
pub(crate) unsafe fn nzbget_rs_webdl_read_temp_url(
    url: *const c_char,
    info_name: *const c_char,
    temp_dir: *const c_char,
    out_buf: *mut crate::ffi::RsBuf,
) -> c_int {
    let tdir = opt_bytes(temp_dir);
    let sep = crate::paths::PATH_SEPARATOR as u8;
    let mut num = 1i32;
    let tmp_path = loop {
        let ns = num.to_string();
        let cand = join_c_slices(&[tdir, &[sep], b"readurl-", ns.as_bytes(), b".tmp"]);
        if !crate::fsops::file_exists(&cand, &crate::fsops::posix_stat) {
            break cand;
        }
        num += 1;
    };
    let dl = NzbgetRsWebDl::new_configured(
        opt_bytes(url),
        opt_bytes(info_name),
        &tmp_path,
        true,
        false,
        2,
        None,
        None,
    );
    let ok = dl.download_sync(5) == (WebDlStatus::AdFinished as c_int);

    if ok && !out_buf.is_null() {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let os_p = std::path::Path::new(std::ffi::OsStr::from_bytes(&tmp_path));
            if let Ok(bytes) = std::fs::read(os_p) {
                let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                out_buf.write(crate::ffi::into_buf(bytes[..end].to_vec()));
            } else {
                out_buf.write(crate::ffi::into_buf(Vec::new()));
            }
        }
    }
    let tmp_c = bytes_to_cstring(&tmp_path);
    let _ = libc::unlink(tmp_c.as_ptr());
    c_int::from(ok)
}

/// Executes one RPC speed-test HTTP iteration against `speed.cloudflare.com`.
pub(crate) unsafe fn nzbget_rs_net_speedtest_iter(
    body: *const c_char,
    body_len: usize,
    duration_sec: c_int,
    out_bytes: *mut u32,
    out_secs: *mut f64,
) -> c_int {
    let (port, use_tls) = if crate::ffi::build_tls_disabled() {
        (80, false)
    } else {
        (443, true)
    };
    let mut conn = NzbgetRsConn::new_client(
        b"speed.cloudflare.com",
        port,
        use_tls,
        true,
        crate::ffi::build_debug_enabled(),
        0,
        crate::ffi::tls_host_cb(),
    );
    let core = &mut conn.session.get_mut().core;
    core.timeout = 1;
    let payload = if body.is_null() || body_len == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(body.cast::<u8>(), body_len)
    };
    if !core.connect_base() || !core.send_bytes(payload) {
        if !out_bytes.is_null() {
            *out_bytes = 0;
        }
        if !out_secs.is_null() {
            *out_secs = 0.0;
        }
        return 0;
    }
    let mut total: u32 = 0;
    let mut buf = [0u8; 8 * 1024];
    let start = Instant::now();
    let limit = duration_sec as f64;
    loop {
        let received = core.try_recv_slice(&mut buf);
        if received <= 0 || start.elapsed().as_secs_f64() >= limit {
            break;
        }
        total = total.wrapping_add(received as u32);
    }
    if !out_bytes.is_null() {
        *out_bytes = total;
    }
    if !out_secs.is_null() {
        *out_secs = start.elapsed().as_secs_f64();
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_registry_tracks_and_cancels_connections() {
        let reg = Arc::new(NzbgetRsCancelReg::new());
        {
            let _scope = CancelRegScope::enter(Arc::clone(&reg));
            let c1 = NzbgetRsConn::new_client(b"127.0.0.1", 12345, false, true, false, 0, None);
            assert_eq!(c1.slot.status.load(Ordering::SeqCst), CS_DISCONNECTED);
            reg.stop_all();
            assert_eq!(c1.slot.status.load(Ordering::SeqCst), CS_CANCELLED);

            let c2 = NzbgetRsConn::new_client(b"127.0.0.1", 12345, false, true, false, 0, None);
            assert_eq!(c2.slot.status.load(Ordering::SeqCst), CS_CANCELLED);
        }
        assert!(reg.inner.lock().unwrap().1.is_empty());
    }

    #[test]
    fn loopback_bind_connect_io_and_nntp_handshake() {
        let mut listener = NzbgetRsConn::new_client(b"127.0.0.1", 0, false, false, false, 0, None);
        assert!(listener.session.get_mut().core.bind());
        let lfd = listener.slot.socket.load(Ordering::SeqCst);
        let mut addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        let mut alen = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        unsafe {
            assert_eq!(
                libc::getsockname(lfd, (&mut addr as *mut libc::sockaddr_in).cast(), &mut alen),
                0
            );
        }
        let port = u16::from_be(addr.sin_port) as c_int;

        let handle = std::thread::spawn(move || {
            let afd = listener.session.get_mut().core.accept_fd();
            assert_ne!(afd, INVALID_SOCKET);
            let mut srv = NzbgetRsConn::from_accepted(afd, false, 5, false, 0, None);
            let core = &mut srv.session.get_mut().core;
            core.write_line_bytes(b"200 Welcome\r\n");
            let mut lbuf = [0u8; 256];
            let n = core.read_line_slice(&mut lbuf).unwrap();
            assert_eq!(&lbuf[..n], b"GROUP alt.binaries.test\r\n");
            core.write_line_bytes(b"211 1 1 1 alt.binaries.test\r\n");
            let n2 = core.read_line_slice(&mut lbuf).unwrap();
            assert_eq!(&lbuf[..n2], b"quit\r\n");
            core.write_line_bytes(b"205 Bye\r\n");
        });

        let mut client = NzbgetRsConn::new_client(b"127.0.0.1", port, false, false, false, 0, None);
        let cs = client.session.get_mut();
        assert!(cs.nntp_connect(c"", c"", c"test", c"127.0.0.1"));
        let g1 = cs.nntp_join_group(
            c"alt.binaries.test".as_ptr(),
            c"",
            c"",
            c"test",
            c"127.0.0.1",
        );
        assert!(!g1.is_null());
        let g2 = cs.nntp_join_group(
            c"alt.binaries.test".as_ptr(),
            c"",
            c"",
            c"test",
            c"127.0.0.1",
        );
        assert_eq!(unsafe { CStr::from_ptr(g2).to_bytes() }, b"211 ");
        assert!(cs.nntp_disconnect(c"", c"", c"test", c"127.0.0.1"));
        handle.join().unwrap();
    }

    #[test]
    fn concurrent_cancel_wakes_blocking_recv_and_readline_via_shared_slot() {
        for io_op in [2, 3] {
            let mut fds = [-1 as c_int; 2];
            unsafe {
                assert_eq!(
                    libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()),
                    0
                );
            }
            let raw_conn =
                unsafe { nzbget_rs_conn_from_fd(fds[0], 0, 30, 0, ptr::null_mut(), None) };
            let raw_addr = raw_conn as usize;

            let (entered_tx, entered_rx) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || unsafe {
                let c = raw_addr as *mut NzbgetRsConn;
                entered_tx.send(()).unwrap();
                let mut buf = [0 as c_char; 64];
                let mut n = 0 as c_int;
                nzbget_rs_conn_io(c, io_op, buf.as_mut_ptr(), 16, &mut n)
            });

            entered_rx.recv().unwrap();
            std::thread::sleep(Duration::from_millis(25));
            unsafe {
                assert_eq!(
                    nzbget_rs_conn_ctl(raw_conn, 5, 1, 0, ptr::null(), ptr::null(), ptr::null(), 0),
                    1
                );
                assert_eq!(
                    nzbget_rs_conn_ctl(raw_conn, 3, 0, 0, ptr::null(), ptr::null(), ptr::null(), 0),
                    1
                );
                assert_eq!(
                    nzbget_rs_conn_ctl(
                        raw_conn,
                        14,
                        0,
                        0,
                        ptr::null(),
                        ptr::null(),
                        ptr::null(),
                        0
                    ),
                    CS_CANCELLED as isize
                );
            }
            let res = reader.join().unwrap();
            assert_eq!(res, 0);
            unsafe {
                nzbget_rs_conn_free(raw_conn);
                libc::close(fds[1]);
            }
        }
    }

    #[test]
    fn concurrent_suppress_updates_during_tls_start_and_close_are_lifetime_safe() {
        struct MockTlsObj {
            alive_magic: AtomicUsize,
            suppress_slot: usize,
        }

        unsafe extern "C" fn mock_tls_host_cb(
            _ctx: *mut c_void,
            op: c_int,
            _a1: c_int,
            _a2: c_int,
            p1: usize,
            _s1: *const c_char,
            _s2: *const c_char,
            _s3: *const c_char,
            _buf: *mut c_char,
            out_int: *mut c_int,
        ) -> isize {
            match op {
                4 => {
                    let ptrs = &*(p1 as *const [usize; 2]);
                    let obj = Box::new(MockTlsObj {
                        alive_magic: AtomicUsize::new(0xA11CE_5AFE),
                        suppress_slot: ptrs[1],
                    });
                    // Simulate `TlsSocket::ReportError` reading suppression mid-handshake.
                    let sup = nzbget_rs_conn_ctl(
                        ptr::null_mut(),
                        23,
                        0,
                        obj.suppress_slot,
                        ptr::null(),
                        ptr::null(),
                        ptr::null(),
                        0,
                    );
                    assert!(sup == 0 || sup == 1);
                    if !out_int.is_null() {
                        *out_int = 0; // failing handshake triggers cleanup
                        *out_int.add(1) = -1;
                    }
                    Box::into_raw(obj) as isize
                }
                7 => {
                    let obj = Box::from_raw(p1 as *mut MockTlsObj);
                    assert_eq!(obj.alive_magic.swap(0xDEAD, Ordering::SeqCst), 0xA11CE_5AFE);
                    let sup = nzbget_rs_conn_ctl(
                        ptr::null_mut(),
                        23,
                        0,
                        obj.suppress_slot,
                        ptr::null(),
                        ptr::null(),
                        ptr::null(),
                        0,
                    );
                    assert!(sup == 0 || sup == 1);
                    drop(obj);
                    0
                }
                8 => panic!("TLS suppression must not dereference tls_box via op 8"),
                _ => 0,
            }
        }

        let raw_conn = unsafe {
            nzbget_rs_conn_new(
                c"127.0.0.1".as_ptr(),
                563,
                1,
                0,
                0,
                ptr::null_mut(),
                Some(mock_tls_host_cb),
            )
        };
        let raw_addr = raw_conn as usize;
        let done = Arc::new(AtomicBool::new(false));
        let done_stopper = Arc::clone(&done);

        let stopper = std::thread::spawn(move || unsafe {
            let c = raw_addr as *mut NzbgetRsConn;
            let mut flip = 0 as c_int;
            while !done_stopper.load(Ordering::Relaxed) {
                flip ^= 1;
                assert_eq!(
                    nzbget_rs_conn_ctl(c, 5, flip, 0, ptr::null(), ptr::null(), ptr::null(), 0),
                    1
                );
                std::thread::yield_now();
            }
        });

        unsafe {
            for _ in 0..500 {
                assert_eq!(
                    nzbget_rs_conn_ctl(raw_conn, 4, 1, 0, ptr::null(), ptr::null(), ptr::null(), 0),
                    0
                );
                assert_eq!(
                    nzbget_rs_conn_ctl(raw_conn, 1, 0, 0, ptr::null(), ptr::null(), ptr::null(), 0),
                    1
                );
            }
        }
        done.store(true, Ordering::SeqCst);
        stopper.join().unwrap();

        unsafe {
            assert_eq!(
                nzbget_rs_conn_ctl(raw_conn, 5, 1, 0, ptr::null(), ptr::null(), ptr::null(), 0),
                1
            );
            let c = &*(raw_conn as *const NzbgetRsConn);
            let sup_slot = ptr::from_ref(&c.slot.suppress_errors) as usize;
            assert_eq!(
                nzbget_rs_conn_ctl(
                    ptr::null_mut(),
                    23,
                    0,
                    sup_slot,
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    0
                ),
                1
            );
            nzbget_rs_conn_free(raw_conn);
        }
    }
}
