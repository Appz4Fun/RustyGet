//! Stateful networking, NNTP session, HTTP downloader, and RPC listener/processor
//! runtime (`Connection`, `CancelRegistry`, `NntpConnection`, `WebDownloader`,
//! `RemoteServer`, and `RequestProcessor::Execute`).

use crate::options::OptStr;
use std::cell::{RefCell, UnsafeCell};
use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::artwritercore::join_c_slices;
use crate::socket::{
    accept_connection, bind_listener, cancel_socket, connect_stream, disconnect_socket,
    format_report_error, read_line, recv_exact, remote_addr, send_all, try_recv, write_line,
    AcceptOutcome, ChunkError, ConnStatus, ConnStatusCell, ErrorGate, CONNECTION_READBUFFER_SIZE,
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

/// A TLS socket the host started for a connection. It is only made by
/// [`ConnHostOps::tls_start`] and freed by [`ConnHostOps::tls_close`], so
/// holding one means the host's socket is alive.
#[derive(Debug, PartialEq, Eq)]
pub struct TlsHandle(std::num::NonZeroUsize);

impl TlsHandle {
    /// Wraps a host's socket address.
    ///
    /// # Safety
    /// `raw` is a socket the host is handing out from its `tls_start`; only
    /// that host's `tls_close` may free it.
    pub unsafe fn from_raw(raw: std::num::NonZeroUsize) -> Self {
        Self(raw)
    }

    /// The host's socket address.
    pub fn raw(&self) -> usize {
        self.0.get()
    }
}

/// What [`ConnHostOps::tls_start`] reports besides the socket.
pub struct TlsStarted {
    pub handle: Option<TlsHandle>,
    pub ok: bool,
    pub probe_failure: c_int,
}

impl TlsStarted {
    /// TLS did not start and no socket was made.
    pub const NOT_STARTED: Self = Self {
        handle: None,
        ok: false,
        probe_failure: -1,
    };
}

/// What a connection needs from its host: logging, thread-stop and deadline
/// checks, and TLS (OpenSSL `TlsSocket`, `SocketDeadline`). The real host is
/// [`crate::tlssock::TlsHost`]; the defaults behave as "no host".
pub trait ConnHostOps: Send + Sync {
    /// A debug message.
    fn debug(&self, _msg: &CStr) {}

    /// An error, logged as debug when `suppressed`.
    fn error(&self, _msg: &CStr, _suppressed: bool) {}

    /// Whether the thread or the deadline (0 = none) says to stop.
    ///
    /// # Safety
    /// `deadline` is 0 or the address of a live `tlssock::Deadline`: a host
    /// may dereference any nonzero value.
    unsafe fn is_stopped(&self, _deadline: usize) -> bool {
        false
    }

    /// Waits on `fd` for readability/writability within the deadline.
    ///
    /// # Safety
    /// As for [`ConnHostOps::is_stopped`]: `deadline` is 0 or a live deadline.
    unsafe fn wait(&self, _deadline: usize, _fd: c_int, _read: bool, _write: bool) -> bool {
        false
    }

    /// Starts TLS on `fd`. `flags_verif` holds "is client" in bit 0 and the
    /// certificate verification level from bit 2.
    ///
    /// # Safety
    /// `deadline` (0 = none) and `suppress` must outlive the returned handle:
    /// the TLS socket keeps both addresses.
    #[allow(clippy::too_many_arguments)]
    unsafe fn tls_start(
        &self,
        _fd: c_int,
        _flags_verif: c_int,
        _deadline: usize,
        _suppress: &AtomicBool,
        _host: &CStr,
        _cert: &CStr,
        _key: &CStr,
        _cipher: &CStr,
    ) -> TlsStarted {
        TlsStarted::NOT_STARTED
    }

    /// Sends on the TLS socket; negative on error.
    ///
    /// # Safety
    /// `sock` came from this host's `tls_start` and was not closed.
    unsafe fn tls_send(&self, _sock: &TlsHandle, _data: &[u8]) -> c_int {
        -1
    }

    /// Receives into `buf` from the TLS socket; negative on error.
    ///
    /// # Safety
    /// `sock` came from this host's `tls_start` and was not closed.
    unsafe fn tls_recv(&self, _sock: &TlsHandle, _buf: &mut [u8]) -> c_int {
        -1
    }

    /// Frees the TLS socket, shutting TLS down first when `close`.
    ///
    /// # Safety
    /// `sock` came from this host's `tls_start` and was not closed.
    unsafe fn tls_close(&self, _sock: TlsHandle, _close: bool) {}

    /// The negotiated TLS version and cipher name (`""` when unknown).
    ///
    /// # Safety
    /// `sock` came from this host's `tls_start` and was not closed.
    unsafe fn tls_info(&self, _sock: &TlsHandle) -> (c_int, &'static CStr) {
        (0, c"")
    }

    /// Lets OpenSSL drop this thread's state.
    fn stop_ssl_thread(&self) {}
}

/// A connection's host, cheap to clone. Without one ([`ConnHost::none`])
/// nothing is logged, nothing is stopped, and TLS can't start.
#[derive(Clone, Default)]
pub struct ConnHost(Option<Arc<dyn ConnHostOps>>);

impl ConnHost {
    pub const fn none() -> Self {
        Self(None)
    }

    pub fn new(ops: Arc<dyn ConnHostOps>) -> Self {
        Self(Some(ops))
    }

    pub const fn is_some(&self) -> bool {
        self.0.is_some()
    }

    /// A debug message.
    pub fn debug(&self, msg: &[u8]) {
        if let Some(ops) = &self.0 {
            ops.debug(&bytes_to_cstring(msg));
        }
    }

    /// An error, logged as debug when `suppressed`.
    pub fn error(&self, msg: &[u8], suppressed: bool) {
        if let Some(ops) = &self.0 {
            ops.error(&bytes_to_cstring(msg), suppressed);
        }
    }

    /// Whether the thread or the deadline (0 = none) says to stop.
    ///
    /// # Safety
    /// `deadline` is 0 or the address of a live `tlssock::Deadline`: the host
    /// dereferences any nonzero value.
    pub unsafe fn is_stopped(&self, deadline: usize) -> bool {
        // SAFETY: the caller guarantees `deadline` is 0 or a live deadline.
        self.0
            .as_ref()
            .is_some_and(|ops| unsafe { ops.is_stopped(deadline) })
    }

    /// Waits on `fd` for readability/writability within the deadline.
    ///
    /// # Safety
    /// As for [`ConnHost::is_stopped`]: `deadline` is 0 or a live deadline.
    pub unsafe fn wait(&self, deadline: usize, fd: c_int, read: bool, write: bool) -> bool {
        // SAFETY: the caller guarantees `deadline`.
        self.0
            .as_ref()
            .is_some_and(|ops| unsafe { ops.wait(deadline, fd, read, write) })
    }

    /// Starts TLS on `fd`.
    ///
    /// # Safety
    /// `deadline` (0 = none) and `suppress` must outlive the returned handle:
    /// the TLS socket keeps both addresses.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn tls_start(
        &self,
        fd: c_int,
        flags_verif: c_int,
        deadline: usize,
        suppress: &AtomicBool,
        host: &CStr,
        cert: &CStr,
        key: &CStr,
        cipher: &CStr,
    ) -> TlsStarted {
        let Some(ops) = &self.0 else {
            return TlsStarted::NOT_STARTED;
        };
        // SAFETY: the caller keeps `deadline` and `suppress` alive for the socket.
        unsafe { ops.tls_start(fd, flags_verif, deadline, suppress, host, cert, key, cipher) }
    }

    /// Sends on the TLS socket; negative on error.
    ///
    /// `sock` must come from this host's [`ConnHost::tls_start`].
    pub fn tls_send(&self, sock: &TlsHandle, data: &[u8]) -> c_int {
        // SAFETY: a live socket this host started (see above).
        self.0
            .as_ref()
            .map_or(-1, |ops| unsafe { ops.tls_send(sock, data) })
    }

    /// Receives into `buf` from the TLS socket; negative on error.
    ///
    /// `sock` must come from this host's [`ConnHost::tls_start`].
    pub fn tls_recv(&self, sock: &TlsHandle, buf: &mut [u8]) -> c_int {
        // SAFETY: a live socket this host started (see above).
        self.0
            .as_ref()
            .map_or(-1, |ops| unsafe { ops.tls_recv(sock, buf) })
    }

    /// Frees the TLS socket, shutting TLS down first when `close`.
    ///
    /// `sock` must come from this host's [`ConnHost::tls_start`].
    pub fn tls_close(&self, sock: TlsHandle, close: bool) {
        if let Some(ops) = &self.0 {
            // SAFETY: a live socket this host started (see above), consumed here.
            unsafe { ops.tls_close(sock, close) };
        }
    }

    /// The negotiated TLS version and cipher name (`""` when unknown).
    ///
    /// `sock` must come from this host's [`ConnHost::tls_start`].
    pub fn tls_info(&self, sock: &TlsHandle) -> (c_int, &'static CStr) {
        // SAFETY: a live socket this host started (see above).
        self.0
            .as_ref()
            .map_or((0, c""), |ops| unsafe { ops.tls_info(sock) })
    }

    /// Lets OpenSSL drop this thread's state.
    pub fn stop_ssl_thread(&self) {
        if let Some(ops) = &self.0 {
            ops.stop_ssl_thread();
        }
    }
}

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
    pub status: ConnStatusCell,
    pub suppress_errors: AtomicBool,
    pub total_bytes_read: AtomicI32,
    pub accept_busy: AtomicBool,
    pub tls_probe_failure: AtomicI32,
    pub debug_on: bool,
    pub host: ConnHost,
}

impl ConnCancelSlot {
    fn new(fd: c_int, status: ConnStatus, debug_on: bool, host: ConnHost) -> Self {
        Self {
            socket: AtomicI32::new(fd),
            status: ConnStatusCell::new(status),
            suppress_errors: AtomicBool::new(true),
            total_bytes_read: AtomicI32::new(0),
            accept_busy: AtomicBool::new(false),
            tls_probe_failure: AtomicI32::new(-1),
            debug_on,
            host,
        }
    }

    fn emit_debug(&self, msg: &[u8]) {
        if self.debug_on {
            self.host.debug(msg);
        }
    }

    /// Reports an error, as debug output while errors are suppressed.
    fn report_error(&self, msg: &[u8]) {
        let suppressed = self.suppress_errors.load(Ordering::SeqCst);
        self.host.error(msg, suppressed);
    }

    /// Adds `n` freshly received bytes to the resettable read counter.
    fn count_bytes_read(&self, n: usize) {
        if n != 0 {
            let n = i32::try_from(n).unwrap_or(i32::MAX);
            self.total_bytes_read.fetch_add(n, Ordering::SeqCst);
        }
    }

    /// Whether the thread or `deadline` says to stop.
    ///
    /// Callers pass only their connection's own deadline field, which is 0
    /// or the deadline its owner set and keeps alive for the connection.
    fn is_stopped(&self, deadline: usize) -> bool {
        // SAFETY: see above; every caller in this module passes `core.deadline`.
        unsafe { self.host.is_stopped(deadline) }
    }

    pub fn set_suppress_errors(&self, sup: bool) {
        self.suppress_errors.store(sup, Ordering::SeqCst);
    }

    pub fn cancel(&self, suppress: bool) {
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
            slot.status.set(ConnStatus::Cancelled);
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
    tls_box: Option<TlsHandle>,
    port: c_int,
    tls: bool,
    ip_version: c_int,
    read_buf: Vec<u8>,
    buf_offset: usize,
    buf_avail: usize,
    timeout: c_int,
    is_listener: bool,
    errors: ErrorGate,
    remote_addr_cstr: CString,
    graceful: bool,
    cert_verif_level: u32,
    /// the last `connect_base` failed because the connection was cancelled or the thread stopped
    connect_cancelled: bool,
}

impl ConnCore {
    fn emit_debug(&self, msg: &[u8]) {
        self.slot.emit_debug(msg);
    }

    fn emit_error(&self, msg: &[u8]) {
        self.slot.report_error(msg);
    }

    fn is_stopped(&self) -> bool {
        self.slot.is_stopped(self.deadline_ptr)
    }

    fn start_tls(&mut self, is_client: bool, cert_file: &CStr, key_file: &CStr) -> bool {
        self.emit_debug(b"Starting TLS");
        let host = &self.slot.host;
        if !host.is_some() {
            return false;
        }
        if let Some(old) = self.tls_box.take() {
            host.tls_close(old, false);
        }
        let fd = self.slot.socket.load(Ordering::SeqCst);
        let flags_verif = c_int::from(is_client) | ((self.cert_verif_level as c_int) << 2);
        // SAFETY: the deadline is set by the connection's owner and outlives
        // it, and `suppress_errors` lives in `self.slot`, which this core keeps
        // alive at least until `close_tls_box` frees the socket.
        let started = unsafe {
            host.tls_start(
                fd,
                flags_verif,
                self.deadline_ptr,
                &self.slot.suppress_errors,
                &self.host,
                cert_file,
                key_file,
                &self.cipher,
            )
        };
        self.tls_box = started.handle;
        self.slot
            .tls_probe_failure
            .store(started.probe_failure, Ordering::SeqCst);
        started.ok
    }

    fn close_tls_box(&mut self, do_close: bool) {
        if let Some(sock) = self.tls_box.take() {
            self.slot.host.tls_close(sock, do_close);
        }
    }

    /// The negotiated TLS version and cipher, when TLS is up.
    fn tls_info(&self) -> Option<(c_int, &'static CStr)> {
        let sock = self.tls_box.as_ref()?;
        Some(self.slot.host.tls_info(sock))
    }

    fn do_disconnect(&mut self) {
        self.emit_debug(b"Do disconnecting");
        let fd = self.slot.socket.load(Ordering::SeqCst);
        self.close_tls_box(fd != INVALID_SOCKET);
        let graceful =
            self.graceful && !self.slot.status.is(ConnStatus::Cancelled) && !self.is_stopped();
        disconnect_socket(
            self.host.to_bytes(),
            graceful,
            std::mem::take(&mut self.is_listener),
            &self.slot.socket,
            &self.slot.status,
        );
    }

    fn disconnect_base(&mut self) -> bool {
        self.emit_debug(b"Disconnecting");
        if self.slot.status.is(ConnStatus::Disconnected)
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
        if self.slot.status.is(ConnStatus::Connected) {
            return true;
        }
        let resolved_slices: Vec<&[u8]> = self.resolved_addrs.iter().map(Vec::as_slice).collect();
        let slot = &self.slot;
        let deadline_ptr = self.deadline_ptr;
        let mut res = connect_stream(
            self.host.to_bytes(),
            self.port,
            self.ip_version,
            self.timeout,
            &resolved_slices,
            deadline_ptr != 0,
            // SAFETY: `deadline_ptr` is this connection's own deadline field.
            |fd, r, w| unsafe { slot.host.wait(deadline_ptr, fd, r, w) },
            || slot.is_stopped(deadline_ptr),
            &slot.socket,
            &slot.status,
            &mut self.errors,
            |msg| slot.report_error(msg),
            |msg| slot.emit_debug(msg),
        );
        if self.slot.status.is(ConnStatus::Cancelled) || self.is_stopped() {
            res = false;
        }
        if res && self.tls && !self.start_tls(true, c"", c"") {
            res = false;
        }
        self.connect_cancelled = false;
        if res && !self.slot.status.is(ConnStatus::Cancelled) && !self.is_stopped() {
            self.slot.status.set(ConnStatus::Connected);
        } else {
            res = false;
            // seen before do_disconnect, which leaves the status at disconnected
            self.connect_cancelled =
                self.slot.status.is(ConnStatus::Cancelled) || self.is_stopped();
            self.do_disconnect();
        }
        res
    }

    fn bind(&mut self) -> bool {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        self.emit_debug(b"Binding");
        let slot = &self.slot;
        let listening = bind_listener(
            self.host.to_bytes(),
            self.port,
            self.ip_version,
            &slot.socket,
            &slot.status,
            &mut self.errors,
            |msg| slot.report_error(msg),
        );
        if listening {
            self.is_listener = true;
        }
        listening
    }

    fn accept_fd(&mut self) -> c_int {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        self.emit_debug(b"Accepting connection");
        let slot = &self.slot;
        let deadline_ptr = self.deadline_ptr;
        let outcome = accept_connection(
            self.host.to_bytes(),
            self.timeout,
            &slot.socket,
            &slot.status,
            &mut self.errors,
            || slot.is_stopped(deadline_ptr),
            |msg| slot.report_error(msg),
        );
        match outcome {
            AcceptOutcome::Accepted(fd) => {
                slot.accept_busy.store(false, Ordering::SeqCst);
                fd
            }
            AcceptOutcome::NotListening => INVALID_SOCKET,
            AcceptOutcome::NoClient { busy } => {
                slot.accept_busy.store(busy, Ordering::SeqCst);
                INVALID_SOCKET
            }
        }
    }

    fn raw_send_chunk(
        slot: &ConnCancelSlot,
        tls: Option<&TlsHandle>,
        errors: &mut ErrorGate,
        chunk: &[u8],
    ) -> c_int {
        if let Some(sock) = tls {
            let sent = slot.host.tls_send(sock, chunk);
            errors.set_tls_error(sent < 0);
            return sent.max(-1);
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
        tls: Option<&TlsHandle>,
        chunk: &mut [u8],
    ) -> Result<usize, ChunkError> {
        if let Some(sock) = tls {
            let recvd = slot.host.tls_recv(sock, chunk);
            return usize::try_from(recvd).map_err(|_| ChunkError::Tls);
        }
        let recvd = unsafe {
            libc::recv(
                slot.socket.load(Ordering::SeqCst),
                chunk.as_mut_ptr().cast(),
                chunk.len(),
                0,
            )
        };
        usize::try_from(recvd).map_err(|_| ChunkError::Socket)
    }

    /// Receives one chunk, adding what arrived to the slot's read counter.
    fn counted_recv_chunk(
        slot: &ConnCancelSlot,
        tls: Option<&TlsHandle>,
        chunk: &mut [u8],
    ) -> Result<usize, ChunkError> {
        Self::raw_recv_chunk(slot, tls, chunk).inspect(|&n| slot.count_bytes_read(n))
    }

    fn write_line_bytes(&mut self, line: &[u8]) -> c_int {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        let tb = self.tls_box.as_ref();
        let slot = &self.slot;
        let errors = &mut self.errors;
        write_line(&slot.status, line, |chunk| {
            Self::raw_send_chunk(slot, tb, errors, chunk)
        })
    }

    fn send_bytes(&mut self, data: &[u8]) -> bool {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        self.emit_debug(b"Sending data");
        let tb = self.tls_box.as_ref();
        let slot = &self.slot;
        let errors = &mut self.errors;
        send_all(&slot.status, data, |chunk| {
            Self::raw_send_chunk(slot, tb, errors, chunk)
        })
    }

    fn read_line_slice(&mut self, dst: &mut [u8]) -> Option<usize> {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        if !self.slot.status.is(ConnStatus::Connected) || dst.is_empty() {
            return None;
        }
        let tb = self.tls_box.as_ref();
        let slot = &self.slot;
        read_line(
            self.host.to_bytes(),
            &slot.status,
            &mut self.read_buf,
            &mut self.buf_offset,
            &mut self.buf_avail,
            dst,
            &mut self.errors,
            |chunk| Self::counted_recv_chunk(slot, tb, chunk),
            |msg| slot.report_error(msg),
        )
    }

    fn recv_exact_slice(&mut self, dst: &mut [u8]) -> bool {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        if dst.is_empty() {
            return true;
        }
        let tb = self.tls_box.as_ref();
        let slot = &self.slot;
        recv_exact(
            self.host.to_bytes(),
            &self.read_buf,
            &mut self.buf_offset,
            &mut self.buf_avail,
            dst,
            &mut self.errors,
            |chunk| Self::counted_recv_chunk(slot, tb, chunk),
            |msg| slot.report_error(msg),
        )
    }

    fn try_recv_slice(&mut self, dst: &mut [u8]) -> c_int {
        #[cfg(unix)]
        let _cg = unsafe { crate::ffi::PosixCancelGuard::disable() };
        let tb = self.tls_box.as_ref();
        let slot = &self.slot;
        let received = try_recv(
            self.host.to_bytes(),
            dst,
            &mut self.errors,
            |chunk| Self::counted_recv_chunk(slot, tb, chunk),
            |msg| slot.report_error(msg),
        );
        received.map_or(-1, |n| c_int::try_from(n).unwrap_or(c_int::MAX))
    }

    fn report_error_msg(
        &mut self,
        prefix: &[u8],
        arg: Option<&[u8]>,
        print_code: bool,
        code: c_int,
    ) {
        if let Some(formatted) =
            format_report_error(&mut self.errors, prefix, arg, print_code, code, None)
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
    /// How the last `nntp_connect` ended: 0 connected, 1 socket or TLS failed, 2 the
    /// greeting failed (or the connection dropped during the login), 3 the server refused
    /// the login, 4 cancelled or stopped.
    nntp_connect_stage: u8,
}

impl ConnSession {
    fn nntp_connect(&mut self, user: &CStr, pass: &CStr, srv_name: &CStr, srv_host: &CStr) -> bool {
        if self.core.slot.debug_on {
            let msg = join_c_slices(&[b"Opening connection to ", self.core.host.to_bytes()]);
            self.core.emit_debug(&msg);
        }
        if self.core.slot.status.is(ConnStatus::Connected) {
            return true;
        }
        self.nntp_auth.rejected = false;
        self.nntp_auth.dropped = false;
        self.nntp_active_group = None;
        if !self.core.connect_base() {
            self.nntp_connect_stage = if self.core.connect_cancelled { 4 } else { 1 };
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
        let handshake = crate::nntp::handshake(&mut io, &mut self.nntp_auth);
        let cancelled = handshake.is_err()
            || self.core.slot.status.is(ConnStatus::Cancelled)
            || self.core.is_stopped();
        let result = handshake.unwrap_or(1);
        self.nntp_connect_stage = match result {
            0 => 0,
            _ if cancelled => 4,
            2 if !self.nntp_auth.dropped => 3,
            _ => 2,
        };
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
        if self.core.slot.status.is(ConnStatus::Connected) {
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
        conn_host: ConnHost,
    ) -> Self {
        let id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
        let slot = Arc::new(ConnCancelSlot::new(
            INVALID_SOCKET,
            ConnStatus::Disconnected,
            debug_on,
            conn_host,
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
                        slot.status.set(ConnStatus::Cancelled);
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
            tls_box: None,
            port,
            tls,
            ip_version: 0,
            read_buf: vec![0u8; CONNECTION_READBUFFER_SIZE + 1 + DECODE_SLACK],
            buf_offset: 0,
            buf_avail: 0,
            timeout: 60,
            is_listener: false,
            errors: ErrorGate::default(),
            remote_addr_cstr: CString::default(),
            graceful: false,
            cert_verif_level: 2,
            connect_cancelled: false,
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
                nntp_connect_stage: 0,
            }),
        }
    }

    pub fn from_accepted(
        fd: c_int,
        tls: bool,
        timeout: c_int,
        debug_on: bool,
        host: ConnHost,
    ) -> Self {
        let id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
        let slot = Arc::new(ConnCancelSlot::new(
            fd,
            ConnStatus::Connected,
            debug_on,
            host,
        ));
        let mut core = ConnCore {
            id,
            slot: Arc::clone(&slot),
            cancel_reg: None,
            host: CString::default(),
            cipher: CString::default(),
            resolved_addrs: Vec::new(),
            deadline_ptr: 0,
            tls_box: None,
            port: 0,
            tls,
            ip_version: 0,
            read_buf: vec![0u8; CONNECTION_READBUFFER_SIZE + 1 + DECODE_SLACK],
            buf_offset: 0,
            buf_avail: 0,
            timeout,
            is_listener: false,
            errors: ErrorGate::default(),
            remote_addr_cstr: CString::default(),
            graceful: false,
            cert_verif_level: 2,
            connect_cancelled: false,
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
                nntp_connect_stage: 0,
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
        Ok(self.core.slot.status.is(ConnStatus::Cancelled))
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
    conn_host: ConnHost,
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
            conn_host: crate::ffi::tls_host(),
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
                slot.cancel(true);
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
            self.dl.conn_host.clone(),
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
            if conn.slot.status.is(ConnStatus::Cancelled) {
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
    conn_host: ConnHost,
    stopped: AtomicBool,
    state: Mutex<RemoteSrvState>,
}

impl RemoteSrvInstance {
    fn new(tls: bool) -> Arc<Self> {
        Arc::new(Self {
            tls,
            tls_disabled: crate::ffi::build_tls_disabled(),
            debug_on: crate::ffi::build_debug_enabled(),
            conn_host: crate::ffi::tls_host(),
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
                        o.str_bytes(OptStr::ControlIp).to_vec(),
                        o.str_bytes(OptStr::SecureCert).to_vec(),
                        o.str_bytes(OptStr::SecureKey).to_vec(),
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

        let secure_control = opts
            .as_deref()
            .is_some_and(|o| o.scalars().secure_control != 0);
        if !self.tls && secure_control && !self.tls_disabled {
            let missing = |path: &[u8]| {
                path.is_empty() || !crate::fsops::file_exists(path, &crate::fsops::posix_stat)
            };
            if missing(&secure_cert) {
                self.emit_log(
                    2,
                    b"HTTPS is on but the certificate file was not found, so only plain HTTP will work. Check option <SecureCert>",
                );
            } else if missing(&secure_key) {
                self.emit_log(
                    2,
                    b"HTTPS is on but the key file was not found, so only plain HTTP will work. Check option <SecureKey>",
                );
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
                        self.conn_host.clone(),
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
                            self.conn_host.clone(),
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
                slot.cancel(true);
            }
            for p in &g.active_processors {
                p.stopped.store(true, Ordering::SeqCst);
                p.conn_slot.cancel(false);
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

const TLS_HANDSHAKE_RECORD: u8 = 0x16;

/// Looks at the first byte a client sent without taking it off the socket, waiting
/// up to the connection timeout. `None` if nothing arrived or the socket failed.
fn peek_first_byte(conn: &mut NzbgetRsConn) -> Option<u8> {
    let fd = conn.slot.socket.load(Ordering::SeqCst);
    if fd == INVALID_SOCKET {
        return None;
    }
    #[cfg(unix)]
    {
        let timeout = conn.session.get_mut().core.timeout.max(1);
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd for the duration of the call.
        let ready = unsafe { libc::poll(&mut pfd, 1, timeout.saturating_mul(1000)) };
        if ready <= 0 {
            return None;
        }
        let mut byte = 0u8;
        loop {
            // SAFETY: reads at most one byte into a local; MSG_PEEK leaves it queued.
            let n = unsafe { libc::recv(fd, (&raw mut byte).cast(), 1, libc::MSG_PEEK) };
            if n == 1 {
                return Some(byte);
            }
            if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return None;
        }
    }
    #[cfg(not(unix))]
    {
        None
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
    // HTTPS shares ControlPort: with SecureControl on, a connection that opens with a
    // TLS handshake record (0x16) is answered over TLS, anything else as plain HTTP.
    let tls = tls
        || (opts.as_deref().is_some_and(|o| o.scalars().secure_control != 0)
            && !crate::ffi::build_tls_disabled()
            && peek_first_byte(conn) == Some(TLS_HANDSHAKE_RECORD));
    if tls && !crate::ffi::build_tls_disabled() {
        let cert = opts.as_deref().and_then(|o| o.str_cstr(OptStr::SecureCert)).unwrap_or(c"");
        let key = opts.as_deref().and_then(|o| o.str_cstr(OptStr::SecureKey)).unwrap_or(c"");
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
            // HTTPS is answered on ControlPort itself (see exec_request_processor).
            let secure = None;
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

/// NServ's sockets over [`NzbgetRsConn`]: the transport `--nserv` uses unless a
/// test supplies its own.
pub struct NetNservTransport {
    pub tls_host: ConnHost,
    pub debug_on: bool,
}

impl crate::nserv::NservTransport for NetNservTransport {
    fn listener(
        &self,
        bind_address: &CStr,
        port: c_int,
        tls: bool,
    ) -> Option<Box<dyn crate::nserv::NservListener>> {
        let mut conn = Box::new(NzbgetRsConn::new_client(
            bind_address.to_bytes(),
            port,
            tls,
            true,
            self.debug_on,
            self.tls_host.clone(),
        ));
        conn.session.get_mut().core.timeout = 10;
        conn.slot.set_suppress_errors(false);
        Some(Box::new(NetNservSocket {
            conn,
            tls_host: self.tls_host.clone(),
            debug_on: self.debug_on,
        }))
    }
}

/// One NServ socket: a listener or an accepted client connection. It owns its
/// connection, so its I/O goes through `&mut` without `unsafe`; other threads
/// reach it only through [`crate::nserv::NservListener::canceller`].
struct NetNservSocket {
    conn: Box<NzbgetRsConn>,
    tls_host: ConnHost,
    debug_on: bool,
}

impl NetNservSocket {
    fn core(&mut self) -> &mut ConnCore {
        &mut self.conn.session.get_mut().core
    }
}

impl crate::nserv::NservListener for NetNservSocket {
    fn bind(&mut self) -> bool {
        self.core().bind()
    }

    fn accept(&mut self) -> Option<Box<dyn crate::nserv::NservConn>> {
        let (tls_host, debug_on) = (self.tls_host.clone(), self.debug_on);
        let core = self.core();
        let fd = core.accept_fd();
        if fd == INVALID_SOCKET {
            return None;
        }
        let conn = Box::new(NzbgetRsConn::from_accepted(
            fd,
            core.tls,
            core.timeout,
            debug_on,
            tls_host.clone(),
        ));
        Some(Box::new(Self {
            conn,
            tls_host,
            debug_on,
        }))
    }

    fn canceller(&self) -> crate::nserv::NservCanceller {
        let slot = Arc::clone(&self.conn.slot);
        Box::new(move || slot.cancel(true))
    }

    fn shutdown(mut self: Box<Self>) {
        self.core().disconnect_base();
    }
}

impl crate::nserv::NservConn for NetNservSocket {
    fn start_tls(&mut self, cert: Option<&CStr>, key: Option<&CStr>) -> Option<Vec<u8>> {
        self.conn.slot.set_suppress_errors(false);
        if let Some(cert) = cert {
            if !self.core().start_tls(false, cert, key.unwrap_or(c"")) {
                return None;
            }
        }
        Some(self.conn.host.to_bytes().to_vec())
    }

    fn read_line<'a>(&mut self, buf: &'a mut [u8]) -> Option<&'a [u8]> {
        self.core().read_line_slice(buf)?;
        Some(match CStr::from_bytes_until_nul(buf) {
            Ok(line) => line.to_bytes(),
            Err(_) => buf,
        })
    }

    fn write_line(&mut self, line: &[u8]) {
        self.core().write_line_bytes(line);
    }

    fn send(&mut self, data: &[u8]) {
        self.core().send_bytes(data);
    }

    fn close(mut self: Box<Self>, graceful: bool) {
        let core = self.core();
        if graceful {
            core.graceful = true;
        }
        core.disconnect_base();
        let host = self.tls_host.clone();
        drop(self);
        host.stop_ssl_thread();
    }
}

// ---------------------------------------------------------------------------
// FFI entry points
// ---------------------------------------------------------------------------

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
    conn_host: ConnHost,
) -> *mut NzbgetRsConn {
    Box::into_raw(Box::new(NzbgetRsConn::new_client(
        opt_bytes(host),
        port,
        tls != 0,
        reg_cancel != 0,
        debug_on != 0,
        conn_host,
    )))
}

#[cfg(test)]
pub(crate) unsafe fn nzbget_rs_conn_from_fd(
    fd: c_int,
    tls: c_int,
    timeout: c_int,
    debug_on: c_int,
    host: ConnHost,
) -> *mut NzbgetRsConn {
    Box::into_raw(Box::new(NzbgetRsConn::from_accepted(
        fd,
        tls != 0,
        timeout,
        debug_on != 0,
        host,
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
            c.slot.cancel(false);
            return 1;
        }
        5 => {
            c.slot.set_suppress_errors(a1 != 0);
            return 1;
        }
        11 => {
            c.slot.status.set(ConnStatus::Broken);
            return 1;
        }
        14 => return c.slot.status.get() as isize,
        15 => return isize::from(c.slot.accept_busy.load(Ordering::SeqCst)),
        16 => return c.slot.total_bytes_read.swap(0, Ordering::SeqCst) as isize,
        17 => return c.slot.tls_probe_failure.load(Ordering::SeqCst) as isize,
        19 => return c.host.as_ptr() as isize,
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
        18 => core.tls_info().map_or(0, |(version, _)| version as isize),
        20 => core.get_remote_addr().as_ptr() as isize,
        21 => core.tls_info().map_or(c"", |(_, cipher)| cipher).as_ptr() as isize,
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
    /// Unique for the life of the process, never reused: a server of one pool
    /// generation is never mistaken for a server of another (which may sit at
    /// the address a freed one had) when an exclusion list made before a
    /// config reload is used after it.
    pub uid: u64,
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
    /// The backbone id (`crate::backbones`; 0 = none): configured, or set by
    /// backend identification ([`Self::set_group`]). Read with [`Self::group`].
    group_id: AtomicI32,
    /// The backbone id from `ServerN.Backbone` (0 = none): what backend
    /// identification starts from each time, never one it made itself.
    pub configured_group: i32,
    pub optional: bool,
    pub block_time: AtomicI64,
    pub cert_verif_level: u32,
    /// Connections of this server currently held by duplicate health checks
    /// (`NzbgetRsServerPool::get_check_connection`).
    pub check_in_use: AtomicUsize,
    /// Connects and requests that got an answer, for the light reserve's
    /// flakiness test (`crate::light_pool`).
    pub io_ok: AtomicUsize,
    /// Connects and requests that failed or timed out (not cancelled ones).
    pub io_errors: AtomicUsize,
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
        static NEXT_UID: AtomicU64 = AtomicU64::new(1);
        Self {
            id,
            uid: NEXT_UID.fetch_add(1, Ordering::Relaxed),
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
            group_id: AtomicI32::new(group),
            configured_group: group,
            optional,
            block_time: AtomicI64::new(0),
            check_in_use: AtomicUsize::new(0),
            io_ok: AtomicUsize::new(0),
            io_errors: AtomicUsize::new(0),
            cert_verif_level,
        }
    }

    /// The backbone id: servers with the same id above 0 share one article
    /// store, so a 430 on one rules out the others (`crate::nntp::BackboneMiss`).
    pub fn group(&self) -> i32 {
        self.group_id.load(Ordering::SeqCst)
    }

    /// Puts the server on backbone `group` (backend identification).
    pub fn set_group(&self, group: i32) {
        self.group_id.store(group, Ordering::SeqCst);
    }

    /// Counts a connect or request outcome for the light reserve's flakiness test.
    pub fn note_io(&self, ok: bool) {
        if ok {
            self.io_ok.fetch_add(1, Ordering::Relaxed);
        } else {
            self.io_errors.fetch_add(1, Ordering::Relaxed);
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
    /// Checked out by a duplicate health check, not a download.
    pub check: AtomicBool,
    /// Checked out as a light connection (`crate::light_pool`).
    pub light: AtomicBool,
    /// The pool generation that handed the connection out, held while it is
    /// checked out (see [`NzbgetRsServerPool`]): a config reload retires that
    /// generation, and it is freed only once its last connection is back.
    lease: Mutex<Option<Arc<NzbgetRsServerPool>>>,
}

impl PooledNntpConn {
    pub fn new(
        server: Arc<RsNewsServer>,
        timeout: i32,
        debug_on: bool,
        conn_host: ConnHost,
    ) -> Self {
        let mut conn = NzbgetRsConn::new_client(
            server.host.to_bytes(),
            server.port,
            server.tls,
            false,
            debug_on,
            conn_host,
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
            check: AtomicBool::new(false),
            light: AtomicBool::new(false),
            lease: Mutex::new(None),
        }
    }

    #[inline]
    pub fn is_connected(&self) -> bool {
        self.conn.slot.status.is(ConnStatus::Connected)
    }

    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.conn.slot.status.is(ConnStatus::Cancelled)
    }

    #[inline]
    pub fn set_suppress_errors(&self, suppress: bool) {
        self.conn.slot.set_suppress_errors(suppress);
    }

    #[inline]
    pub fn cancel(&self) {
        self.conn.slot.cancel(false);
    }

    #[inline]
    pub fn set_broken(&self) {
        self.conn.slot.status.set(ConnStatus::Broken);
    }

    #[inline]
    pub fn fetch_total_bytes_read(&self) -> i32 {
        self.conn.slot.total_bytes_read.swap(0, Ordering::SeqCst)
    }

    /// Sets how long a read or write on the open socket may wait, in seconds; `0` or less
    /// restores the connection's own timeout (the pool's `ArticleTimeout`). Takes effect on the
    /// current socket only: a reconnect applies the connection's own timeout again. A light
    /// connection never waits longer than [`Self::light_io_timeout`].
    pub fn set_io_timeout(&self, secs: i32) {
        let fd = self.conn.slot.socket.load(Ordering::SeqCst);
        if fd < 0 || !self.is_connected() {
            return;
        }
        let _ = crate::socket::init_socket_opts(fd, self.io_timeout_for(secs));
    }

    /// The read/write timeout [`Self::set_io_timeout`] applies for `secs`.
    fn io_timeout_for(&self, secs: i32) -> i32 {
        // SAFETY: only the thread holding the checked-out connection calls this
        let own = unsafe { (*self.conn.session.get()).core.timeout };
        if self.light.load(Ordering::SeqCst) {
            let light = self.light_io_timeout();
            if secs <= 0 {
                light
            } else {
                secs.min(light)
            }
        } else if secs <= 0 {
            own
        } else {
            secs
        }
    }

    /// Read/write timeout of a light connection: [`crate::light_pool::LIGHT_IO_TIMEOUT_SECS`],
    /// or the connection's own when shorter. Connecting keeps the connection's own timeout.
    fn light_io_timeout(&self) -> i32 {
        // SAFETY: only the thread holding the checked-out connection calls this
        let own = unsafe { (*self.conn.session.get()).core.timeout };
        let light = crate::light_pool::LIGHT_IO_TIMEOUT_SECS;
        if own > 0 {
            own.min(light)
        } else {
            light
        }
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

    /// Connects and logs in. A host name with several addresses goes through the address
    /// plan (`crate::addrpick`): the pinned or fastest address first, then the next one after
    /// a network-level failure (socket, TLS, greeting), never after a login failure.
    pub unsafe fn connect(&self) -> bool {
        if self.conn.slot.status.is(ConnStatus::Connected) {
            return true;
        }
        let ok = self.connect_planned();
        self.note_io(ok);
        if ok && self.light.load(Ordering::SeqCst) {
            // connected with the connection's own timeout; reads take the light one
            self.set_io_timeout(0);
        }
        ok
    }

    /// Counts an outcome on the server, unless the connection was cancelled
    /// (a stop is not the server's fault).
    pub(crate) fn note_io(&self, ok: bool) {
        if ok || !self.is_cancelled() {
            self.server.note_io(ok);
        }
    }

    unsafe fn connect_planned(&self) -> bool {
        let session = &mut *self.conn.session.get();
        let host = String::from_utf8_lossy(self.server.host.to_bytes()).into_owned();
        let name = String::from_utf8_lossy(self.server.name.to_bytes()).into_owned();
        let plan = crate::addrpick::plan(&crate::addrpick::ServerFacts {
            id: self.server.id,
            name: &name,
            host: &host,
            port: self.server.port,
            ip_version: self.server.ip_version,
            tls: self.server.tls,
            cert_verif_level: self.server.cert_verif_level,
        });
        if plan.is_empty() {
            session.core.resolved_addrs.clear();
            return session.nntp_connect(
                &self.server.user,
                &self.server.password,
                &self.server.name,
                &self.server.host,
            );
        }
        let total = plan.len();
        for (i, addr) in plan.iter().enumerate() {
            session.core.resolved_addrs = vec![addr.as_bytes().to_vec()];
            let ok = session.nntp_connect(
                &self.server.user,
                &self.server.password,
                &self.server.name,
                &self.server.host,
            );
            if ok {
                crate::addrpick::report(self.server.id, addr, true);
                return true;
            }
            let cancelled = self.conn.slot.status.is(ConnStatus::Cancelled);
            // a refused login fails the same everywhere; a cancel or stop is not the address's fault
            if matches!(session.nntp_connect_stage, 3 | 4) || cancelled || session.core.is_stopped()
            {
                return false;
            }
            crate::addrpick::report(self.server.id, addr, false);
            if i + 1 < total {
                crate::log::log_active_bytes(
                    1,
                    format!(
                        "{name}: could not connect to {host} at {addr}, trying {}",
                        plan[i + 1]
                    )
                    .as_bytes(),
                );
                // a failed attempt may leave a socket or TLS state behind; start clean
                session.core.disconnect_base();
            }
        }
        false
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
        let answer = (*self.conn.session.get()).nntp_request(
            req,
            &self.server.user,
            &self.server.password,
            &self.server.name,
            &self.server.host,
        );
        self.note_io(!answer.is_null());
        answer
    }

    pub unsafe fn join_group(&self, grp: *const c_char) -> *const c_char {
        let answer = (*self.conn.session.get()).nntp_join_group(
            grp,
            &self.server.user,
            &self.server.password,
            &self.server.name,
            &self.server.host,
        );
        self.note_io(!answer.is_null());
        answer
    }

    /// Shortens the read/write timeout while the connection serves light work
    /// ([`Self::light_io_timeout`]); connecting keeps the connection's own timeout,
    /// so a slow connect is not taken for a dead server. Only while nobody else
    /// holds the connection (checkout under the pool lock).
    unsafe fn begin_light(&self) {
        self.light.store(true, Ordering::SeqCst);
        self.set_io_timeout(0);
    }

    /// Gives the connection its own read/write timeout back. Only by the
    /// thread that held it, as it returns it.
    unsafe fn end_light(&self) {
        self.light.store(false, Ordering::SeqCst);
        self.set_io_timeout(0);
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

    /// Takes the buffered bytes: the slice starts at them and runs to the end
    /// of the read buffer, so it includes the decode slack after them.
    pub unsafe fn drain_read_buffer(&self) -> (&mut [u8], usize) {
        let core = &mut (*self.conn.session.get()).core;
        let avail = core.buf_avail;
        core.buf_avail = 0;
        (&mut core.read_buf[core.buf_offset..], avail)
    }

    /// Moves up to `dst.len()` already-buffered bytes into `dst`; the rest
    /// stays buffered for the next call.
    pub unsafe fn read_buffered(&self, dst: &mut [u8]) -> usize {
        let core = &mut (*self.conn.session.get()).core;
        let n = core.buf_avail.min(dst.len());
        dst[..n].copy_from_slice(&core.read_buf[core.buf_offset..core.buf_offset + n]);
        core.buf_offset += n;
        core.buf_avail -= n;
        n
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
    /// The light reserve of each server (`crate::light_pool`).
    light: crate::light_pool::LightState,
}

/// Connections of one server held now: by downloads and checks, and as light connections.
fn server_busy(connections: &[Box<PooledNntpConn>], srv: &Arc<RsNewsServer>) -> (i32, i32) {
    let mut main = 0;
    let mut light = 0;
    for c in connections.iter().filter(|c| Arc::ptr_eq(&c.server, srv)) {
        if c.in_use.load(Ordering::SeqCst) {
            if c.light.load(Ordering::SeqCst) {
                light += 1;
            } else {
                main += 1;
            }
        }
    }
    (main, light)
}

/// What [`NzbgetRsServerPool::get_light_connection_level`] found.
pub enum LightPick {
    Got(*mut PooledNntpConn),
    /// A server has a reserve, but every light connection of it is busy.
    Busy,
    /// No matching server has a reserve: use the main pool.
    NoReserve,
}

/// Inputs of a recompute read outside the pool lock.
struct LightInputs {
    cap: i32,
    bandwidth_bound: bool,
}

/// One generation of the news server pool: the servers of one configuration
/// and their connections.
///
/// Ownership (one model for every user of a news server connection):
/// - The daemon holds the current generation (`daemon::SERVER_POOL`); the
///   active pointer ([`active_server_pool`]) is weak, so it never keeps an old
///   generation alive by itself.
/// - Every checked-out connection holds a strong lease on the generation that
///   handed it out (`PooledNntpConn::lease`), taken under the pool lock at
///   checkout and dropped when the connection comes back. A fetch, live stream
///   repair, borrow wave, probe, live PAR fetch or a held fleet connection
///   therefore keeps its connection, its server and the whole generation
///   alive however long it runs.
/// - A config reload retires the current generation ([`release_server_pool`]):
///   it hands out no more connections, closes its idle ones, and is freed when
///   the last lease ends. The reloaded daemon builds a new generation beside
///   it; new work only ever gets connections of the active one.
/// - A connection is returned with `free_connection` on any generation: the
///   pointer is matched by identity against the generations still alive, so
///   it reaches the one that owns it even when the caller only knows the
///   active pool.
///
/// While a retired generation still has connections out, the provider sees
/// them beside the new generation's (at most the connections in flight at the
/// reload; usually a few seconds, as each fetch takes a fresh connection from
/// the active pool). A provider that refuses the extra ones is handled like
/// any "too many connections" answer.
///
/// Any other connection backend (the coming nntp-rs pool) must follow the
/// same rule: its pool belongs to a generation, and a fetch holds an `Arc` of
/// the generation it fetches on.
pub struct NzbgetRsServerPool {
    servers: std::sync::RwLock<Vec<Arc<RsNewsServer>>>,
    inner: Mutex<ServerPoolInner>,
    max_norm_level: AtomicI32,
    generation: AtomicI32,
    timeout: AtomicI32,
    retry_interval: AtomicI32,
    conn_host: ConnHost,
    /// Last time (ms since an arbitrary epoch) a download asked for a connection
    /// and none was free. Duplicate health checks back off while this is recent.
    download_starved_at_ms: AtomicI64,
    /// When the light reserves were last recomputed (`pool_now_ms`).
    light_checked_ms: AtomicI64,
    /// `LightConnections` to use instead of the option's (tests); -1 = the option.
    light_cap_override: AtomicI32,
    /// This generation, for the leases of its checked-out connections. Unset
    /// for a pool not made by [`NzbgetRsServerPool::new_shared`] (tests that
    /// own the pool outright), whose connections then hold no lease.
    self_ref: std::sync::OnceLock<std::sync::Weak<NzbgetRsServerPool>>,
    /// Retired by a config reload: no more checkouts; freed with its last lease.
    /// Changed only under the pool lock.
    retired: AtomicBool,
}

/// Generations retired by a config reload whose connections may still be out.
static RETIRED_POOLS: Mutex<Vec<std::sync::Weak<NzbgetRsServerPool>>> = Mutex::new(Vec::new());

/// Retired generations still alive (some connection of theirs is checked out).
pub(crate) fn retired_pools_alive() -> usize {
    let mut g = RETIRED_POOLS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    g.retain(|w| w.strong_count() > 0);
    g.len()
}

/// Who asks [`NzbgetRsServerPool`] for a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConnUse {
    Download,
    /// A duplicate health check (`get_check_connection`).
    Check,
    /// A fleet probe request (`get_probe_connection`).
    Probe,
}

/// How long a download's failed connection request keeps duplicate health checks
/// from taking more connections.
pub const DOWNLOAD_STARVED_WINDOW_MS: i64 = 3000;

fn pool_now_ms() -> i64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    i64::try_from(start.elapsed().as_millis()).unwrap_or(i64::MAX) + 1
}

/// Whether a duplicate health check may take one more connection of a server:
/// always while checks hold fewer than `floor` (at least its first one, so
/// checks keep making progress; a fleet probe's floor is
/// `crate::fleetprobe::probe_check_floor`), any further one only while no
/// download has been refused a connection within the window. Only idle
/// connections are ever handed out: a connection a download holds stays its.
pub fn check_connection_allowed(
    check_in_use: usize,
    floor: usize,
    starved_at_ms: i64,
    now_ms: i64,
) -> bool {
    check_in_use < floor.max(1)
        || starved_at_ms == 0
        || now_ms.saturating_sub(starved_at_ms) > DOWNLOAD_STARVED_WINDOW_MS
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
    pub fn new(conn_host: ConnHost) -> Self {
        Self {
            servers: std::sync::RwLock::new(Vec::new()),
            inner: Mutex::new(ServerPoolInner {
                sorted_servers: Vec::new(),
                connections: Vec::new(),
                levels: Vec::new(),
                light: crate::light_pool::LightState::default(),
            }),
            max_norm_level: AtomicI32::new(0),
            generation: AtomicI32::new(0),
            timeout: AtomicI32::new(60),
            retry_interval: AtomicI32::new(0),
            conn_host,
            download_starved_at_ms: AtomicI64::new(0),
            light_checked_ms: AtomicI64::new(0),
            light_cap_override: AtomicI32::new(-1),
            self_ref: std::sync::OnceLock::new(),
            retired: AtomicBool::new(false),
        }
    }

    /// A pool whose checked-out connections lease it (see the type's docs).
    pub fn new_shared(conn_host: ConnHost) -> Arc<Self> {
        let arc = Arc::new(Self::new(conn_host));
        let _ = arc.self_ref.set(Arc::downgrade(&arc));
        arc
    }

    /// Whether a config reload retired this generation.
    pub fn is_retired(&self) -> bool {
        self.retired.load(Ordering::SeqCst)
    }

    /// Gives `conn`, just checked out under the pool lock, its lease on this
    /// generation.
    fn lease_out(&self, conn: &PooledNntpConn) {
        let lease = self.self_ref.get().and_then(std::sync::Weak::upgrade);
        let old = std::mem::replace(
            &mut *conn.lease.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
            lease,
        );
        // a free connection holds none; one left over would only be dropped here
        drop(old);
    }

    /// Uses `cap` instead of the `LightConnections` option (tests), and
    /// recomputes the reserves at the next checkout.
    pub fn set_light_cap(&self, cap: i32) {
        self.light_cap_override.store(cap, Ordering::SeqCst);
        self.light_checked_ms.store(0, Ordering::SeqCst);
        if let Ok(mut g) = self.inner.lock() {
            g.light.last_ms = 0;
        }
    }

    /// Reads what a due recompute needs from outside the pool (options,
    /// speed limit and meter) before the pool lock is taken; `None` when no
    /// recompute is due.
    fn light_inputs_if_due(&self) -> Option<LightInputs> {
        let now = pool_now_ms();
        let last = self.light_checked_ms.load(Ordering::SeqCst);
        if last != 0 && now - last < crate::light_pool::RECOMPUTE_MS {
            return None;
        }
        let over = self.light_cap_override.load(Ordering::SeqCst);
        let cap = if over >= 0 {
            over
        } else {
            crate::options::active_options().map_or(crate::light_pool::LIGHT_MAX, |o| {
                o.scalars().light_connections
            })
        };
        let limit = crate::ffi::active_workstate().map_or(0, |w| i64::from(w.speed_limit()));
        let speed = if limit > 0 {
            crate::statmeter::active_statmeter().map_or(0, |s| s.current_download_speed())
        } else {
            0
        };
        let bandwidth_bound = crate::light_pool::bandwidth_bound(limit, speed);
        #[cfg(unix)]
        if let Some(shared) = crate::nntprs_pool::shared() {
            shared.pool.pool().set_bandwidth_bound(bandwidth_bound);
        }
        Some(LightInputs {
            cap,
            bandwidth_bound,
        })
    }

    /// Recomputes every server's light reserve (`crate::light_pool::next_size`)
    /// under the pool lock; returns the log lines of the changes.
    fn recompute_light(&self, g: &mut ServerPoolInner, inputs: &LightInputs) -> Vec<Vec<u8>> {
        use crate::light_pool::Reason;
        let now = pool_now_ms();
        let mut msgs = Vec::new();
        if g.light.last_ms != 0 && now - g.light.last_ms < crate::light_pool::RECOMPUTE_MS {
            return msgs;
        }
        let window_ms = if g.light.last_ms == 0 {
            crate::light_pool::RECOMPUTE_MS
        } else {
            now - g.light.last_ms
        };
        g.light.last_ms = now;
        self.light_checked_ms.store(now, Ordering::SeqCst);
        let starved_at = self.download_starved_at_ms.load(Ordering::SeqCst);
        let download_starved = starved_at != 0 && now - starved_at <= window_ms;
        let servers = g.sorted_servers.clone();
        g.light
            .servers
            .retain(|l| servers.iter().any(|s| s.id == l.server_id));
        for srv in &servers {
            let usable =
                srv.active.load(Ordering::SeqCst) && srv.norm_level.load(Ordering::SeqCst) >= 0;
            let ok = srv.io_ok.load(Ordering::Relaxed);
            let err = srv.io_errors.load(Ordering::Relaxed);
            let blocked = self.is_server_blocked(srv);
            let (main_busy, _) = server_busy(&g.connections, srv);
            if g.light.get(srv.id).is_none() {
                g.light.servers.push(crate::light_pool::ServerLight {
                    server_id: srv.id,
                    size: 0,
                    min_spare: srv.max_connections - main_busy,
                    ok_mark: ok,
                    err_mark: err,
                    flaky_until_ms: 0,
                });
            }
            let Some(entry) = g.light.get_mut(srv.id) else {
                continue;
            };
            let d_ok = ok.wrapping_sub(entry.ok_mark) as u64;
            let d_err = err.wrapping_sub(entry.err_mark) as u64;
            if blocked || crate::light_pool::window_flaky(d_ok, d_err) {
                entry.flaky_until_ms = now + crate::light_pool::FLAKY_HOLD_MS;
            }
            let input = crate::light_pool::SizingInput {
                max_connections: if usable { srv.max_connections } else { 0 },
                cap: inputs.cap,
                flaky: now < entry.flaky_until_ms,
                current: entry.size,
                spare: entry.min_spare,
                download_starved,
                bandwidth_bound: inputs.bandwidth_bound,
            };
            let (size, reason) = crate::light_pool::next_size(&input);
            if size != entry.size {
                let detail = match reason {
                    Reason::Flaky if blocked => "blocked".to_string(),
                    Reason::Flaky => format!("{d_err} of {} requests failed", d_ok + d_err),
                    Reason::Spare => format!("{} of {} idle", input.spare, srv.max_connections),
                    Reason::FewConnections | Reason::Ceiling | Reason::DownloadSaturated => {
                        format!("{} connections", srv.max_connections)
                    }
                    _ => String::new(),
                };
                msgs.push(crate::light_pool::change_message(
                    srv.name.to_bytes(),
                    entry.size,
                    size,
                    reason,
                    &detail,
                ));
            }
            entry.size = size;
            entry.min_spare = srv.max_connections - size - main_busy;
            entry.ok_mark = ok;
            entry.err_mark = err;
        }
        msgs
    }

    /// Recomputes the reserves when due.
    fn maybe_recompute_light(&self) {
        let Some(inputs) = self.light_inputs_if_due() else {
            return;
        };
        let msgs = match self.inner.lock() {
            Ok(mut g) => self.recompute_light(&mut g, &inputs),
            Err(_) => return,
        };
        for msg in msgs {
            crate::log::log_active_bytes(crate::log::MK_DETAIL, &msg);
        }
    }

    /// A light connection of `srv` (see `crate::light_pool`), or null when
    /// the server has no free one.
    pub fn get_light_connection(&self, srv: &RsNewsServer) -> *mut PooledNntpConn {
        match self.select_light(&|s: &RsNewsServer| s.id == srv.id) {
            LightPick::Got(c) => c,
            _ => ptr::null_mut(),
        }
    }

    /// A light connection of any server on `level` but those in `ignore_srvs`.
    pub fn get_light_connection_level(
        &self,
        level: i32,
        ignore_srvs: &[&RsNewsServer],
    ) -> LightPick {
        if level < 0 {
            return LightPick::NoReserve;
        }
        self.select_light(&|s: &RsNewsServer| {
            s.norm_level.load(Ordering::SeqCst) == level
                && !ignore_srvs.iter().any(|i| i.id == s.id)
        })
    }

    /// Takes a free light connection of a matching server (active, not
    /// blocked, reserve not used up), an open one first.
    fn select_light(&self, matches: &dyn Fn(&RsNewsServer) -> bool) -> LightPick {
        self.maybe_recompute_light();
        let Ok(mut g) = self.inner.lock() else {
            return LightPick::NoReserve;
        };
        if self.retired.load(Ordering::SeqCst) {
            return LightPick::NoReserve;
        }
        let inner = &mut *g;
        let mut any_reserve = false;
        let mut picks: Vec<usize> = Vec::new();
        for srv in &inner.sorted_servers {
            if !srv.active.load(Ordering::SeqCst)
                || srv.norm_level.load(Ordering::SeqCst) < 0
                || !matches(srv)
                || self.is_server_blocked(srv)
            {
                continue;
            }
            let size = inner.light.size_of(srv.id);
            if size <= 0 {
                continue;
            }
            any_reserve = true;
            let (_, light_busy) = server_busy(&inner.connections, srv);
            if light_busy >= size {
                continue;
            }
            let mut best: Option<usize> = None;
            for (idx, c) in inner.connections.iter().enumerate() {
                if Arc::ptr_eq(&c.server, srv) && !c.in_use.load(Ordering::SeqCst) {
                    if c.is_connected() {
                        best = Some(idx);
                        break;
                    }
                    best.get_or_insert(idx);
                }
            }
            if let Some(idx) = best {
                picks.push(idx);
            }
        }
        if picks.is_empty() {
            return if any_reserve {
                LightPick::Busy
            } else {
                LightPick::NoReserve
            };
        }
        let rand_val = unsafe { libc::rand() };
        let idx = picks[crate::nntp::server_pool_pick_candidate_index(rand_val, picks.len())];
        let nl = {
            let c = &inner.connections[idx];
            c.in_use.store(true, Ordering::SeqCst);
            // SAFETY: free until now, and taken under the pool lock
            unsafe { c.begin_light() };
            self.lease_out(c);
            c.server.norm_level.load(Ordering::SeqCst)
        };
        if let Some(slot) = inner.levels.get_mut(nl as usize) {
            *slot -= 1;
        }
        LightPick::Got((&mut *inner.connections[idx]) as *mut PooledNntpConn)
    }

    /// `(server id, light reserve, light connections busy)` per server, for RPC status.
    pub fn light_status(&self) -> Vec<(i32, i32, i32)> {
        let Ok(g) = self.inner.lock() else {
            return Vec::new();
        };
        g.sorted_servers
            .iter()
            .map(|s| {
                let (_, busy) = server_busy(&g.connections, s);
                (s.id, g.light.size_of(s.id), busy)
            })
            .collect()
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
                            self.conn_host.clone(),
                        )));
                    }
                    g.levels[nl as usize] += free_existing + (to_add as i32);
                }
            }
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// A connection for a download. When none is free, duplicate health checks
    /// stop taking more connections for a while, so the ones they hold come
    /// back to downloads as their current probes finish.
    pub fn get_connection(
        &self,
        level: i32,
        want_srv: Option<&RsNewsServer>,
        ignore_srvs: &[&RsNewsServer],
    ) -> *mut PooledNntpConn {
        self.get_connection_missing(level, want_srv, ignore_srvs, &[])
    }

    /// [`Self::get_connection`] for an article some backbones answered 430 for.
    pub fn get_connection_missing(
        &self,
        level: i32,
        want_srv: Option<&RsNewsServer>,
        ignore_srvs: &[&RsNewsServer],
        missing: &[crate::nntp::BackboneMiss],
    ) -> *mut PooledNntpConn {
        let conn = self.select_connection(level, want_srv, ignore_srvs, missing, ConnUse::Download);
        if conn.is_null() && level >= 0 {
            self.download_starved_at_ms
                .store(pool_now_ms(), Ordering::SeqCst);
        }
        conn
    }

    /// A connection of `srv` for a duplicate health check. Checks may use every
    /// idle connection, but downloads come first: beyond one per server, none is
    /// handed out while a download was recently refused one.
    pub fn get_check_connection(&self, srv: &RsNewsServer) -> *mut PooledNntpConn {
        let nl = srv.norm_level.load(Ordering::SeqCst);
        self.select_connection(nl, Some(srv), &[], &[], ConnUse::Check)
    }

    /// [`Self::get_check_connection`] for a fleet probe request: up to
    /// `crate::fleetprobe::probe_check_floor` check connections of the server
    /// even while a download was refused one (idle connections only).
    pub fn get_probe_connection(&self, srv: &RsNewsServer) -> *mut PooledNntpConn {
        let nl = srv.norm_level.load(Ordering::SeqCst);
        self.select_connection(nl, Some(srv), &[], &[], ConnUse::Probe)
    }

    /// [`Self::get_check_connection`] by server id.
    pub fn get_check_connection_by_server_id(&self, server_id: i32) -> *mut PooledNntpConn {
        let Some(srv) = self.server_by_id(server_id) else {
            return ptr::null_mut();
        };
        self.get_check_connection(&srv)
    }

    /// Picks a free connection under the pool lock. For a check or probe
    /// (`use_`), the admission rule of [`check_connection_allowed`] is
    /// applied to the server that actually supplies the connection, and the
    /// check occupancy is reserved in the same critical section.
    fn select_connection(
        &self,
        level: i32,
        want_srv: Option<&RsNewsServer>,
        ignore_srvs: &[&RsNewsServer],
        missing: &[crate::nntp::BackboneMiss],
        use_: ConnUse,
    ) -> *mut PooledNntpConn {
        let for_check = use_ != ConnUse::Download;
        if level < 0 {
            return ptr::null_mut();
        }
        self.maybe_recompute_light();
        let Ok(mut g) = self.inner.lock() else {
            return ptr::null_mut();
        };
        if self.retired.load(Ordering::SeqCst) {
            return ptr::null_mut();
        }
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
                group: s.group(),
                norm_level: s.norm_level.load(Ordering::SeqCst),
                active: s.active.load(Ordering::SeqCst),
                optional: s.optional,
                blocked: crate::dupefallback::server_pool_is_blocked(
                    s.block_time.load(Ordering::SeqCst),
                    cur_time,
                    retry_interval,
                ),
                retention: s.retention,
            })
            .collect();

        let mut conn_views: Vec<crate::nntp::ServerPoolConnView> = g
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

        // downloads and checks may hold a server's connections but its light
        // reserve: once they hold that many, its free ones look busy to them
        let mut main_busy = vec![0i32; srv_count];
        for (view, c) in conn_views.iter().zip(g.connections.iter()) {
            if view.in_use && view.server_index < srv_count && !c.light.load(Ordering::SeqCst) {
                main_busy[view.server_index] += 1;
            }
        }
        let main_allowed: Vec<i32> = g
            .sorted_servers
            .iter()
            .map(|s| s.max_connections - g.light.size_of(s.id))
            .collect();
        for view in &mut conn_views {
            if !view.in_use
                && view.server_index < srv_count
                && main_busy[view.server_index] >= main_allowed[view.server_index]
            {
                view.in_use = true;
            }
        }

        let want_tuple = want_srv.map(|s| (s.id, s.group()));
        let ignore_ids: Vec<i32> = if want_srv.is_none() {
            ignore_srvs.iter().map(|s| s.id).collect()
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
            &ignore_ids,
            missing,
            rand_val,
            &mut unblock,
        );
        for idx in unblock {
            g.sorted_servers[idx].block_time.store(0, Ordering::SeqCst);
        }
        if let Some((chosen_conn, chosen_level)) = found {
            if for_check {
                let srv = &g.connections[chosen_conn].server;
                let now = pool_now_ms();
                let floor = if use_ == ConnUse::Probe {
                    crate::fleetprobe::probe_check_floor(
                        g.light.size_of(srv.id),
                        srv.max_connections,
                        g.light.flaky(srv.id, now),
                    )
                } else {
                    1
                };
                if !check_connection_allowed(
                    srv.check_in_use.load(Ordering::SeqCst),
                    floor,
                    self.download_starved_at_ms.load(Ordering::SeqCst),
                    now,
                ) {
                    return ptr::null_mut();
                }
            }
            g.levels[chosen_level as usize] -= 1;
            let srv_index = conn_views[chosen_conn].server_index;
            if srv_index < srv_count {
                let id = g.sorted_servers[srv_index].id;
                let spare = main_allowed[srv_index] - main_busy[srv_index] - 1;
                g.light.note_spare(id, spare);
            }
            let c = &mut g.connections[chosen_conn];
            c.in_use.store(true, Ordering::SeqCst);
            if for_check && !c.check.swap(true, Ordering::SeqCst) {
                c.server.check_in_use.fetch_add(1, Ordering::SeqCst);
            }
            self.lease_out(c);
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

    /// Return a connection handed out by `get_connection*` to the pool
    /// generation that handed it out: this one, or another still alive (a
    /// connection checked out before a config reload, returned through the
    /// pool active after it).
    ///
    /// The pointer is only used as an identity: it is looked up among each
    /// generation's own connections, so a null or foreign pointer is ignored
    /// instead of dereferenced.
    pub fn free_connection(&self, conn_ptr: *mut PooledNntpConn, used: bool) {
        if conn_ptr.is_null() {
            return;
        }
        if used {
            crate::log::log_active_bytes(3, b"Freeing used connection");
        }
        match self.free_own_connection(conn_ptr, used) {
            // the caller holds this generation (`&self`), so the lease is never
            // its last reference here
            Some(lease) => drop(lease),
            None => free_in_other_generation(Some(self), conn_ptr, used),
        }
    }

    /// Returns `conn_ptr` if it is one of this generation's connections, and
    /// hands back its lease for the caller to drop outside the pool lock.
    /// `None`: not a connection of this generation.
    fn free_own_connection(
        &self,
        conn_ptr: *mut PooledNntpConn,
        used: bool,
    ) -> Option<Option<Arc<NzbgetRsServerPool>>> {
        // a poisoned lock still frees: a connection never returned would keep
        // its generation alive for good
        let mut g = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let inner = &mut *g;
        let c = inner
            .connections
            .iter()
            .find(|c| ptr::eq(&***c, conn_ptr.cast_const()))?;
        if c.check.swap(false, Ordering::SeqCst) {
            // paired with the increment in get_check_connection
            c.server.check_in_use.fetch_sub(1, Ordering::SeqCst);
        }
        if c.light.swap(false, Ordering::SeqCst) {
            // SAFETY: the caller held the connection until this call
            unsafe { c.end_light() };
        }
        if self.retired.load(Ordering::SeqCst) && c.is_connected() {
            // nothing will use it again: don't hold a server slot until the
            // generation's last connection is back
            // SAFETY: the caller held the connection until this call
            unsafe { c.disconnect() };
        }
        let lease = c
            .lease
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        c.in_use.store(false, Ordering::SeqCst);
        if used {
            c.free_time.store(now_unix_sec(), Ordering::SeqCst);
        }
        let nl = c.server.norm_level.load(Ordering::SeqCst);
        if nl > -1 && c.server.active.load(Ordering::SeqCst) {
            if let Some(slot) = inner.levels.get_mut(nl as usize) {
                *slot += 1;
            }
        }
        drop(g);
        Some(lease)
    }

    /// Retires this generation (a config reload replaced it): no more
    /// checkouts, idle connections closed now, the ones still out closed as
    /// they come back. Returns how many are still out.
    fn retire(&self) -> usize {
        // a poisoned lock still retires: a generation never retired would hand
        // out connections of an old configuration
        let mut g = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.retired.store(true, Ordering::SeqCst);
        // an idle connection has no lease and nobody holds its pointer
        g.connections.retain(|c| c.in_use.load(Ordering::SeqCst));
        g.connections.len()
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
        // An article missing on the old set of servers may exist on the new one.
        crate::articlefetcher::forget_missing_everywhere();
        self.init_connections();
        self.close_unused_connections();
        #[cfg(unix)]
        if let Some(me) = active_server_pool().filter(|p| std::ptr::eq(&**p, self)) {
            crate::nntprs_pool::rebuild(&me);
        }
    }

    /// News server connections checked out right now (downloads and checks).
    pub fn connections_in_use(&self) -> usize {
        self.inner.lock().map_or(0, |g| {
            g.connections
                .iter()
                .filter(|c| c.in_use.load(Ordering::SeqCst))
                .count()
        })
    }

    /// Connections the pool has, checked out or not (tests).
    #[cfg(test)]
    fn connection_count(&self) -> usize {
        self.inner.lock().map_or(0, |g| g.connections.len())
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

    /// Light connections kept now on the servers [`Self::primary_connections`] counts.
    pub fn primary_light_connections(&self) -> i32 {
        self.maybe_recompute_light();
        let Ok(g) = self.inner.lock() else {
            return 0;
        };
        g.sorted_servers
            .iter()
            .filter(|s| s.active.load(Ordering::SeqCst) && s.norm_level.load(Ordering::SeqCst) == 0)
            .map(|s| g.light.size_of(s.id))
            .sum()
    }

    /// Whether a download was refused a connection within
    /// [`DOWNLOAD_STARVED_WINDOW_MS`] (checks past their first then wait).
    pub fn download_starved_now(&self) -> bool {
        let at = self.download_starved_at_ms.load(Ordering::SeqCst);
        !check_connection_allowed(1, 1, at, pool_now_ms())
    }

    /// Probe requests the servers [`Self::primary_connections`] counts may
    /// have at once (`crate::fleetprobe::probe_reads`): their light
    /// connections raised to the probe floor, 0 with no light reserve.
    pub fn primary_probe_reads(&self) -> i32 {
        self.maybe_recompute_light();
        let Ok(g) = self.inner.lock() else {
            return 0;
        };
        let now = pool_now_ms();
        g.sorted_servers
            .iter()
            .filter(|s| s.active.load(Ordering::SeqCst) && s.norm_level.load(Ordering::SeqCst) == 0)
            .map(|s| {
                let flaky = g.light.flaky(s.id, now);
                crate::fleetprobe::probe_reads(g.light.size_of(s.id), s.max_connections, flaky)
            })
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
                group: s.group(),
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

/// Creates a server pool and makes it the active one.
pub(crate) fn install_server_pool() -> Arc<NzbgetRsServerPool> {
    let arc = NzbgetRsServerPool::new_shared(crate::ffi::tls_host());
    // A new pool (start-up or a config reload) may hold servers that have
    // articles the old one lacked.
    crate::articlefetcher::forget_missing_everywhere();
    if let Ok(mut g) = ACTIVE_SERVER_POOL.write() {
        *g = Some(Arc::downgrade(&arc));
    }
    if let Some(lg) = crate::log::active_log() {
        lg.register_debuggable(4);
    }
    arc
}

/// Drops the daemon's hold on `pool`, first clearing the active pool if it is
/// this one. The generation is retired: connections still checked out (a
/// live stream repair, a probe, a held fleet connection) keep it alive and
/// finish on it; it is freed when the last one comes back.
pub(crate) fn release_server_pool(pool: Arc<NzbgetRsServerPool>) {
    if let Some(lg) = crate::log::active_log() {
        lg.unregister_debuggable(4);
    }
    let wk = Arc::downgrade(&pool);
    // findable as retired before it stops being the active one, so a
    // connection returned in between always reaches it
    RETIRED_POOLS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(wk.clone());
    if let Ok(mut g) = ACTIVE_SERVER_POOL.write() {
        if g.as_ref()
            .is_some_and(|cur| std::sync::Weak::ptr_eq(cur, &wk))
        {
            *g = None;
        }
    }
    let out = pool.retire();
    if out > 0 {
        let msg = format!(
            "News server connections still in use when the server pool was replaced: {out}; they finish on the old pool (the server briefly sees them beside the new pool's)"
        );
        crate::log::log_active_bytes(crate::log::MK_DETAIL, msg.as_bytes());
    }
    drop(pool);
    // a generation freed meanwhile leaves no entry behind
    retired_pools_alive();
}

impl Drop for NzbgetRsServerPool {
    fn drop(&mut self) {
        let left = self.inner.get_mut().map_or(0, |g| g.connections.len());
        if *self.retired.get_mut() && left > 0 {
            let msg = format!(
                "Old server pool freed after its last {left} connection(s) came back"
            );
            crate::log::log_active_bytes(crate::log::MK_DETAIL, msg.as_bytes());
        }
    }
}

/// Returns a checked-out connection to the generation that handed it out,
/// whether or not a pool is active right now (between the two halves of a
/// config reload there is none). Use this rather than freeing through
/// [`active_server_pool`] only when there is one: a connection skipped that way
/// stays checked out and keeps its generation alive for good.
pub(crate) fn return_connection(conn_ptr: *mut PooledNntpConn, used: bool) {
    if conn_ptr.is_null() {
        return;
    }
    match active_server_pool() {
        Some(pool) => pool.free_connection(conn_ptr, used),
        None => {
            if used {
                crate::log::log_active_bytes(3, b"Freeing used connection");
            }
            free_in_other_generation(None, conn_ptr, used);
        }
    }
}

/// Returns `conn_ptr` to whichever other live generation owns it: the active
/// one or a retired one. A pointer no generation knows is ignored.
fn free_in_other_generation(
    skip: Option<&NzbgetRsServerPool>,
    conn_ptr: *mut PooledNntpConn,
    used: bool,
) {
    let mut candidates: Vec<Arc<NzbgetRsServerPool>> = Vec::new();
    if let Some(active) = active_server_pool() {
        candidates.push(active);
    }
    {
        let mut g = RETIRED_POOLS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        g.retain(|w| w.strong_count() > 0);
        candidates.extend(g.iter().filter_map(std::sync::Weak::upgrade));
    }
    for pool in &candidates {
        if skip.is_some_and(|s| ptr::eq(&**pool, s)) {
            continue;
        }
        if let Some(lease) = pool.free_own_connection(conn_ptr, used) {
            drop(lease);
            break;
        }
    }
    // the last lease of a retired generation may have been the one above:
    // the generation is freed here, outside any of its methods
    drop(candidates);
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
            let light = if p1 != 0 && p2 > 0 {
                p.light_status()
            } else {
                Vec::new()
            };
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
                    let (size, busy) = light
                        .iter()
                        .find(|l| l.0 == s.id)
                        .map_or((0, 0), |l| (l.1, l.2));
                    dst.light_connections = size;
                    dst.light_busy = busy;
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

/// A host that keeps the first error that was not suppressed instead of
/// logging it, and forwards everything else to `inner`. Server checks and
/// TLS cipher probes report their own failures, so they don't log errors.
struct ErrorTrapHost {
    inner: ConnHost,
    first_err: Mutex<Option<Vec<u8>>>,
}

impl ErrorTrapHost {
    fn new(inner: ConnHost) -> Self {
        Self {
            inner,
            first_err: Mutex::new(None),
        }
    }

    fn take_first_err(&self) -> Option<Vec<u8>> {
        match self.first_err.lock() {
            Ok(mut g) => g.take(),
            Err(p) => p.into_inner().take(),
        }
    }
}

impl ConnHostOps for ErrorTrapHost {
    fn debug(&self, msg: &CStr) {
        self.inner.debug(msg.to_bytes());
    }

    fn error(&self, msg: &CStr, suppressed: bool) {
        if suppressed {
            self.inner.error(msg.to_bytes(), true);
            return;
        }
        let mut g = match self.first_err.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if g.is_none() {
            *g = Some(msg.to_bytes().to_vec());
        }
    }

    unsafe fn is_stopped(&self, deadline: usize) -> bool {
        // SAFETY: forwarded under the caller's contract.
        unsafe { self.inner.is_stopped(deadline) }
    }

    unsafe fn wait(&self, deadline: usize, fd: c_int, read: bool, write: bool) -> bool {
        // SAFETY: forwarded under the caller's contract.
        unsafe { self.inner.wait(deadline, fd, read, write) }
    }

    unsafe fn tls_start(
        &self,
        fd: c_int,
        flags_verif: c_int,
        deadline: usize,
        suppress: &AtomicBool,
        host: &CStr,
        cert: &CStr,
        key: &CStr,
        cipher: &CStr,
    ) -> TlsStarted {
        // SAFETY: forwarded under the caller's contract.
        unsafe {
            self.inner
                .tls_start(fd, flags_verif, deadline, suppress, host, cert, key, cipher)
        }
    }

    unsafe fn tls_send(&self, sock: &TlsHandle, data: &[u8]) -> c_int {
        self.inner.tls_send(sock, data)
    }

    unsafe fn tls_recv(&self, sock: &TlsHandle, buf: &mut [u8]) -> c_int {
        self.inner.tls_recv(sock, buf)
    }

    unsafe fn tls_close(&self, sock: TlsHandle, close: bool) {
        self.inner.tls_close(sock, close);
    }

    unsafe fn tls_info(&self, sock: &TlsHandle) -> (c_int, &'static CStr) {
        self.inner.tls_info(sock)
    }

    fn stop_ssl_thread(&self) {
        self.inner.stop_ssl_thread();
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
    address: &[u8],
    out_err: &mut Vec<u8>,
) -> c_int {
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
    let trap = Arc::new(ErrorTrapHost::new(crate::ffi::tls_host()));
    let debug_on = crate::log::active_debug_build();
    let mut conn = NzbgetRsConn::new_client(
        srv.host.to_bytes(),
        srv.port,
        srv.tls,
        true,
        debug_on,
        ConnHost::new(Arc::clone(&trap) as Arc<dyn ConnHostOps>),
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
    if !address.is_empty() {
        // a per-address check: connect there, keeping the host name for TLS
        let Some(addr) = crate::addrpick::normalize(&String::from_utf8_lossy(address)) else {
            *out_err = b"Not an IP address".to_vec();
            return 0;
        };
        conn.session.get_mut().core.resolved_addrs = vec![addr.into_bytes()];
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
        // QUIT, so the check doesn't hold one of the account's connections
        conn.session
            .get_mut()
            .nntp_disconnect(&srv.user, &srv.password, &srv.name, &srv.host);
    }
    if !ok {
        if let Some(err) = trap.take_first_err() {
            if !err.is_empty() {
                *out_err = err;
            }
        }
    }
    c_int::from(ok)
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

pub unsafe extern "C" fn nzbget_rs_serverpool_probe_tls(
    server: *const NzbgetTlsServer,
    cipher: *const c_char,
    expected_ver: c_int,
    numeric: *const c_char,
    deadline_ptr: usize,
) -> c_int {
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
    let mut conn = NzbgetRsConn::new_client(
        srv.host.to_bytes(),
        srv.port,
        true,
        true,
        debug_on,
        ConnHost::new(Arc::new(ErrorTrapHost::new(crate::ffi::tls_host()))),
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

    // The connection's host forwards `tls_info` to the real host.
    let (neg_ver, neg_cipher) = conn.session.get_mut().core.tls_info().unwrap_or((0, c""));
    let want_cipher = opt_bytes(cipher);
    let got_cipher = neg_cipher.to_bytes();
    let result = if connected && got_cipher == want_cipher && neg_ver == expected_ver {
        1
    } else if conn.session.get_mut().nntp_auth.error {
        -5
    } else {
        conn.slot.tls_probe_failure.load(Ordering::SeqCst)
    };
    conn.slot.status.set(ConnStatus::Broken);
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
    out_buf: &mut Vec<u8>,
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

    if ok {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let os_p = std::path::Path::new(std::ffi::OsStr::from_bytes(&tmp_path));
            if let Ok(bytes) = std::fs::read(os_p) {
                let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                *out_buf = bytes[..end].to_vec();
            } else {
                *out_buf = Vec::new();
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
        crate::ffi::tls_host(),
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
    fn check_test_pool(connections: i32) -> (NzbgetRsServerPool, Arc<RsNewsServer>) {
        let pool = NzbgetRsServerPool::new(crate::ffi::tls_host());
        let srv = Arc::new(RsNewsServer::new(
            1,
            true,
            b"s1",
            b"127.0.0.1",
            1,
            0,
            b"",
            b"",
            false,
            false,
            b"",
            connections,
            0,
            0,
            0,
            false,
            0,
        ));
        pool.add_server(Arc::clone(&srv));
        pool.init_connections();
        // these tests are about the main pool alone
        pool.set_light_cap(0);
        (pool, srv)
    }

    fn shared_test_pool(connections: i32, light_cap: i32) -> (Arc<NzbgetRsServerPool>, Arc<RsNewsServer>) {
        let pool = NzbgetRsServerPool::new_shared(crate::ffi::tls_host());
        let srv = Arc::new(RsNewsServer::new(
            1, true, b"s1", b"127.0.0.1", 1, 0, b"", b"", false, false, b"", connections, 0, 0, 0,
            false, 0,
        ));
        pool.add_server(Arc::clone(&srv));
        pool.init_connections();
        pool.set_light_cap(light_cap);
        (pool, srv)
    }

    /// 6790 crash of 2026-11-26 02:40:45 CST: a config reload freed the pool
    /// while a live stream repair still fetched on one of its connections.
    /// Every kind of checkout (download, check, probe, light) now leases its
    /// generation: retiring it keeps every connection, its server and the pool
    /// alive; it hands out nothing more; each connection can be returned
    /// through the next generation; the old one is freed with the last.
    #[test]
    fn checked_out_connections_keep_a_retired_generation_alive() {
        let (old, srv) = shared_test_pool(8, 2);
        let dl = old.get_connection(0, None, &[]);
        let check = old.get_check_connection(&srv);
        let probe = old.get_probe_connection(&srv);
        let light = old.get_light_connection(&srv);
        let held = [dl, check, probe, light];
        assert!(held.iter().all(|c| !c.is_null()));
        let srv_uid = srv.uid;
        drop(srv);
        let weak = Arc::downgrade(&old);
        // what the reload does: the daemon lets go of the generation
        release_server_pool(old);
        let gen = weak.upgrade().expect("the checkouts keep the old generation");
        assert!(gen.is_retired());
        assert!(gen.get_connection(0, None, &[]).is_null(), "a retired pool hands out nothing");
        assert!(gen.get_light_connection(&gen.servers_snapshot()[0]).is_null());
        // idle connections are closed at once; the four held ones stay
        assert_eq!(gen.connections_in_use(), 4);
        assert_eq!(gen.connection_count(), 4);
        drop(gen);
        // the connections and their server are still there for their users
        for &c in &held {
            let c = unsafe { &*c };
            c.note_io(true);
            assert_eq!(c.server.uid, srv_uid);
            assert!(c.in_use.load(Ordering::SeqCst));
        }
        // the next generation: each old connection returned through it
        let (new, _) = shared_test_pool(8, 2);
        assert_ne!(new.servers_snapshot()[0].uid, srv_uid, "a server of another generation");
        for (i, &c) in held.iter().enumerate() {
            assert!(weak.upgrade().is_some(), "freed before connection {i} came back");
            new.free_connection(c, true);
        }
        // (another test returning a connection may hold the old generation for
        // a moment while it looks it up)
        let deadline = Instant::now() + Duration::from_secs(2);
        while weak.upgrade().is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(weak.upgrade().is_none(), "the last connection back frees the old generation");
        // the new generation is untouched by the old one's returns
        assert_eq!(new.connections_in_use(), 0);
        let c = new.get_connection(0, None, &[]);
        assert!(!c.is_null());
        new.free_connection(c, true);
        assert_eq!(new.connections_in_use(), 0);
    }

    /// A pool owned outright (not shared) leases nothing, as before.
    #[test]
    fn an_unshared_pool_leases_nothing() {
        let (pool, _srv) = check_test_pool(2);
        let c = pool.get_connection(0, None, &[]);
        assert!(!c.is_null());
        assert!(unsafe { &*c }.lease.lock().unwrap().is_none());
        pool.free_connection(c, true);
    }

    fn light_test_pool(connections: i32, cap: i32) -> (NzbgetRsServerPool, Arc<RsNewsServer>) {
        let (pool, srv) = check_test_pool(connections);
        pool.set_light_cap(cap);
        (pool, srv)
    }

    #[test]
    fn light_reserve_is_kept_from_downloads() {
        // 40 connections: a reserve of 5, so downloads get 35
        let (pool, srv) = light_test_pool(40, crate::light_pool::LIGHT_MAX);
        let dl: Vec<_> = (0..40).map(|_| pool.get_connection(0, None, &[])).collect();
        let got = dl.iter().filter(|c| !c.is_null()).count();
        assert_eq!(got, 35);
        assert_eq!(pool.light_status(), vec![(1, 5, 0)]);
        // a STAT check during the full download is served at once
        let light: Vec<_> = (0..5)
            .map(|_| crate::light_pool::check_connection(&pool, &srv))
            .collect();
        assert!(light.iter().all(|c| !c.is_null()));
        assert!(light
            .iter()
            .all(|&c| unsafe { &*c }.light.load(Ordering::SeqCst)));
        assert_eq!(pool.light_status(), vec![(1, 5, 5)]);
        // the reserve is used up and downloads hold the rest: nothing more
        assert!(crate::light_pool::check_connection(&pool, &srv).is_null());
        // a freed light connection never goes to a download
        pool.free_connection(light[0], true);
        assert!(pool.get_connection(0, None, &[]).is_null());
        assert!(!unsafe { &*light[0] }.light.load(Ordering::SeqCst));
        for c in light.into_iter().skip(1).chain(dl) {
            pool.free_connection(c, true);
        }
        assert_eq!(pool.light_status(), vec![(1, 5, 0)]);
    }

    #[test]
    fn light_reserve_off_and_small_servers() {
        // turned off: downloads get every connection, checks the main pool
        let (pool, srv) = light_test_pool(40, 0);
        let dl: Vec<_> = (0..40).map(|_| pool.get_connection(0, None, &[])).collect();
        assert!(dl.iter().all(|c| !c.is_null()));
        assert!(pool.get_light_connection(&srv).is_null());
        for c in dl {
            pool.free_connection(c, true);
        }
        // four connections: one light connection
        let (pool, srv) = light_test_pool(4, crate::light_pool::LIGHT_MAX);
        let dl: Vec<_> = (0..4).map(|_| pool.get_connection(0, None, &[])).collect();
        assert_eq!(dl.iter().filter(|c| !c.is_null()).count(), 3);
        let light = pool.get_light_connection(&srv);
        assert!(!light.is_null());
        pool.free_connection(light, true);
        for c in dl {
            pool.free_connection(c, true);
        }
        // a single connection stays with downloads
        let (pool, srv) = light_test_pool(1, crate::light_pool::LIGHT_MAX);
        let dl = pool.get_connection(0, None, &[]);
        assert!(!dl.is_null());
        assert!(pool.get_light_connection(&srv).is_null());
        pool.free_connection(dl, true);
    }

    #[test]
    fn light_level_pick_reports_busy_and_no_reserve() {
        let (pool, srv) = light_test_pool(16, crate::light_pool::LIGHT_MAX);
        let a = pool.get_light_connection_level(0, &[]);
        let b = pool.get_light_connection_level(0, &[]);
        let (LightPick::Got(a), LightPick::Got(b)) = (a, b) else {
            panic!("16 connections keep 2 light ones");
        };
        assert!(matches!(
            pool.get_light_connection_level(0, &[]),
            LightPick::Busy
        ));
        // an excluded server has no reserve to offer
        assert!(matches!(
            pool.get_light_connection_level(0, &[&srv]),
            LightPick::NoReserve
        ));
        // no server on that level
        assert!(matches!(
            pool.get_light_connection_level(1, &[]),
            LightPick::NoReserve
        ));
        pool.free_connection(a, true);
        pool.free_connection(b, true);
    }

    #[test]
    fn flaky_server_keeps_one_light_connection() {
        let (pool, srv) = light_test_pool(40, crate::light_pool::LIGHT_MAX);
        let c = pool.get_light_connection(&srv);
        pool.free_connection(c, true);
        assert_eq!(pool.light_status(), vec![(1, 5, 0)]);
        for _ in 0..10 {
            srv.note_io(false);
        }
        // the next recompute sees the errors
        pool.light_checked_ms.store(0, Ordering::SeqCst);
        pool.inner.lock().unwrap().light.last_ms = 0;
        let c = pool.get_light_connection(&srv);
        assert!(!c.is_null());
        pool.free_connection(c, true);
        assert_eq!(pool.light_status(), vec![(1, 1, 0)]);
    }

    fn force_light_recompute(pool: &NzbgetRsServerPool) {
        pool.light_checked_ms.store(0, Ordering::SeqCst);
        pool.inner.lock().unwrap().light.last_ms = 0;
    }

    #[test]
    fn light_checkout_keeps_the_connect_timeout() {
        let (pool, srv) = light_test_pool(40, crate::light_pool::LIGHT_MAX);
        let c = pool.get_light_connection(&srv);
        let conn = unsafe { &*c };
        // the pool's ArticleTimeout (60 s) still governs connecting ...
        assert_eq!(unsafe { (*conn.conn.session.get()).core.timeout }, 60);
        // ... while reads and writes wait at most the light timeout
        let light = crate::light_pool::LIGHT_IO_TIMEOUT_SECS;
        assert_eq!(conn.io_timeout_for(0), light);
        assert_eq!(conn.io_timeout_for(60), light);
        assert_eq!(conn.io_timeout_for(5), 5);
        pool.free_connection(c, true);
        // back to the connection's own once returned
        assert!(!conn.light.load(Ordering::SeqCst));
        assert_eq!(conn.io_timeout_for(0), 60);
        assert_eq!(conn.io_timeout_for(90), 90);
        assert_eq!(unsafe { (*conn.conn.session.get()).core.timeout }, 60);
    }

    #[test]
    fn small_servers_keep_no_or_a_returnable_light_connection() {
        // three connections: all for downloads
        let (pool, srv) = light_test_pool(3, crate::light_pool::LIGHT_MAX);
        let dl: Vec<_> = (0..3).map(|_| pool.get_connection(0, None, &[])).collect();
        assert!(dl.iter().all(|c| !c.is_null()));
        assert!(pool.get_light_connection(&srv).is_null());
        assert_eq!(pool.light_status(), vec![(1, 0, 0)]);
        for c in dl {
            pool.free_connection(c, true);
        }
        // four: one light connection, handed to a download that needs every one
        let (pool, srv) = light_test_pool(4, crate::light_pool::LIGHT_MAX);
        let mut dl: Vec<_> = (0..4).map(|_| pool.get_connection(0, None, &[])).collect();
        assert_eq!(dl.iter().filter(|c| !c.is_null()).count(), 3);
        assert_eq!(pool.light_status(), vec![(1, 1, 0)]);
        force_light_recompute(&pool);
        let fourth = pool.get_connection(0, None, &[]);
        assert!(!fourth.is_null());
        assert_eq!(pool.light_status(), vec![(1, 0, 0)]);
        dl.push(fourth);
        for c in dl {
            pool.free_connection(c, true);
        }
        // and back after a window in which the download left connections idle
        force_light_recompute(&pool);
        assert!(pool.get_light_connection(&srv).is_null());
        force_light_recompute(&pool);
        let light = pool.get_light_connection(&srv);
        assert!(!light.is_null());
        assert_eq!(pool.light_status(), vec![(1, 1, 1)]);
        pool.free_connection(light, true);
    }

    #[test]
    fn checkout_returns_the_connection_on_panic() {
        let (pool, srv) = light_test_pool(40, crate::light_pool::LIGHT_MAX);
        let c = crate::light_pool::check_connection(&pool, &srv);
        assert!(!c.is_null());
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = crate::light_pool::Checkout::new(&pool, c);
            panic!("light job failed");
        }));
        assert!(res.is_err());
        assert_eq!(pool.light_status(), vec![(1, 5, 0)]);
        assert!(!unsafe { &*c }.in_use.load(Ordering::SeqCst));
        // a normal release
        let c = crate::light_pool::check_connection(&pool, &srv);
        crate::light_pool::Checkout::new(&pool, c).release(true);
        assert_eq!(pool.light_status(), vec![(1, 5, 0)]);
    }

    #[test]
    fn check_connection_allowed_rules() {
        // first check connection of a server is always allowed
        assert!(check_connection_allowed(0, 1, 1000, 1001));
        assert!(check_connection_allowed(0, 0, 1000, 1001));
        // further ones while no download was refused
        assert!(check_connection_allowed(5, 1, 0, 1001));
        // not while a download was refused recently
        assert!(!check_connection_allowed(
            5,
            1,
            1000,
            1000 + DOWNLOAD_STARVED_WINDOW_MS
        ));
        // again once the window has passed
        assert!(check_connection_allowed(
            5,
            1,
            1000,
            1001 + DOWNLOAD_STARVED_WINDOW_MS
        ));
        // a probe floor of 4: up to 4 even while a download was refused
        assert!(check_connection_allowed(3, 4, 1000, 1001));
        assert!(!check_connection_allowed(4, 4, 1000, 1001));
    }

    /// 6790 (#717 follow-up): downloads use every connection, so the light
    /// reserve shrinks to 1 and the fleet probes used to read one article at
    /// a time. Probe requests now keep 5 at once (1 light + 4 idle check
    /// connections of the probe share) while a download is refused
    /// connections; a plain check still gets only its one, and no connection
    /// a download holds is ever handed out.
    #[test]
    fn probes_keep_a_floor_while_downloads_saturate() {
        let (pool, srv) = light_test_pool(60, crate::light_pool::LIGHT_MAX);
        assert_eq!(pool.primary_probe_reads(), 5, "a full reserve");
        let mut dl: Vec<*mut PooledNntpConn> = Vec::new();
        let take_all = |dl: &mut Vec<*mut PooledNntpConn>| loop {
            let c = pool.get_connection(0, None, &[]);
            if c.is_null() {
                break;
            }
            dl.push(c);
        };
        take_all(&mut dl);
        assert_eq!(dl.len(), 55);
        // the download wants every connection: one less light per window
        for _ in 0..4 {
            force_light_recompute(&pool);
            take_all(&mut dl);
        }
        assert_eq!(pool.light_status(), vec![(1, 1, 0)]);
        assert_eq!(dl.len(), 59);
        // the probes keep 5 reads at once all the same
        assert_eq!(pool.primary_probe_reads(), crate::fleetprobe::PROBE_READ_FLOOR);
        // the downloads hold every main connection: nothing to take from them
        assert!(pool.get_probe_connection(&srv).is_null());
        let light = pool.get_light_connection(&srv);
        assert!(!light.is_null());
        // four downloads end an article (between two fetches) and a download
        // was refused a connection just now
        for c in dl.drain(..4) {
            pool.free_connection(c, true);
        }
        assert!(pool.get_connection(0, None, &[&srv]).is_null());
        let probes: Vec<_> = (0..4).map(|_| pool.get_probe_connection(&srv)).collect();
        assert!(probes.iter().all(|c| !c.is_null()));
        assert_eq!(srv.check_in_use.load(Ordering::SeqCst), 4);
        assert!(pool.download_starved_now());
        // no fifth, and a plain check gets none past the probes' either
        pool.free_connection(dl.pop().unwrap(), true);
        assert!(pool.get_probe_connection(&srv).is_null());
        assert!(pool.get_check_connection(&srv).is_null());
        // the idle connection is the download's
        let back = pool.get_connection(0, None, &[]);
        assert!(!back.is_null());
        dl.push(back);
        for c in probes.into_iter().chain(dl).chain([light]) {
            pool.free_connection(c, true);
        }
        assert_eq!(srv.check_in_use.load(Ordering::SeqCst), 0);
        // a flaky server: one probe read
        for _ in 0..10 {
            srv.note_io(false);
        }
        force_light_recompute(&pool);
        assert_eq!(pool.primary_probe_reads(), 1);
    }

    #[test]
    fn checks_use_every_idle_connection() {
        let (pool, srv) = check_test_pool(4);
        let held: Vec<_> = (0..4).map(|_| pool.get_check_connection(&srv)).collect();
        assert!(held.iter().all(|c| !c.is_null()));
        assert_eq!(srv.check_in_use.load(Ordering::SeqCst), 4);
        assert!(pool.get_check_connection(&srv).is_null());
        for c in held {
            pool.free_connection(c, true);
        }
        assert_eq!(srv.check_in_use.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn concurrent_checks_respect_backoff_atomically() {
        let (pool, srv) = check_test_pool(8);
        // downloads waiting before any check holds a connection
        let dl: Vec<_> = (0..8).map(|_| pool.get_connection(0, None, &[])).collect();
        assert!(dl.iter().all(|c| !c.is_null()));
        assert!(pool.get_connection(0, None, &[]).is_null());
        for c in dl {
            pool.free_connection(c, true);
        }
        let pool = Arc::new(pool);
        let got: Vec<usize> = std::thread::scope(|sc| {
            let hs: Vec<_> = (0..8)
                .map(|_| {
                    let pool = Arc::clone(&pool);
                    let srv = Arc::clone(&srv);
                    sc.spawn(move || pool.get_check_connection(&srv) as usize)
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let taken: Vec<usize> = got.into_iter().filter(|&c| c != 0).collect();
        // only the one-per-server floor while downloads are starved
        assert_eq!(taken.len(), 1);
        assert_eq!(srv.check_in_use.load(Ordering::SeqCst), 1);
        for c in taken {
            pool.free_connection(c as *mut PooledNntpConn, true);
        }
    }

    #[test]
    fn downloads_preempt_check_connections() {
        let (pool, srv) = check_test_pool(3);
        let held: Vec<_> = (0..3).map(|_| pool.get_check_connection(&srv)).collect();
        assert!(held.iter().all(|c| !c.is_null()));

        // a download finds nothing free: from now on checks yield
        assert!(pool.get_connection(0, None, &[]).is_null());

        // a check finishes its probe and frees a connection: the next check may
        // not take it, the waiting download does
        pool.free_connection(held[0], true);
        assert!(pool.get_check_connection(&srv).is_null());
        let dl = pool.get_connection(0, None, &[]);
        assert!(!dl.is_null());
        assert!(!unsafe { &*dl }.check.load(Ordering::SeqCst));
        assert_eq!(srv.check_in_use.load(Ordering::SeqCst), 2);

        // checks keep one connection per server even while downloads wait
        pool.free_connection(held[1], true);
        pool.free_connection(held[2], true);
        assert_eq!(srv.check_in_use.load(Ordering::SeqCst), 0);
        let first = pool.get_check_connection(&srv);
        assert!(!first.is_null());
        assert!(pool.get_check_connection(&srv).is_null());

        // a download connection freed never counts as a check connection
        pool.free_connection(dl, true);
        pool.free_connection(first, true);
        assert_eq!(srv.check_in_use.load(Ordering::SeqCst), 0);
    }

    use super::*;

    #[test]
    fn cancel_registry_tracks_and_cancels_connections() {
        let reg = Arc::new(NzbgetRsCancelReg::new());
        {
            let _scope = CancelRegScope::enter(Arc::clone(&reg));
            let c1 =
                NzbgetRsConn::new_client(b"127.0.0.1", 12345, false, true, false, ConnHost::none());
            assert_eq!(c1.slot.status.get(), ConnStatus::Disconnected);
            reg.stop_all();
            assert_eq!(c1.slot.status.get(), ConnStatus::Cancelled);

            let c2 =
                NzbgetRsConn::new_client(b"127.0.0.1", 12345, false, true, false, ConnHost::none());
            assert_eq!(c2.slot.status.get(), ConnStatus::Cancelled);
        }
        assert!(reg.inner.lock().unwrap().1.is_empty());
    }

    /// One NNTP session from a std listener that runs `serve` on the accepted stream.
    fn stage_after(serve: fn(std::net::TcpStream), cancel_after: Option<u64>) -> u8 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = c_int::from(listener.local_addr().unwrap().port());
        let server = std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                serve(stream);
            }
        });
        let mut client =
            NzbgetRsConn::new_client(b"127.0.0.1", port, false, false, false, ConnHost::none());
        client.session.get_mut().core.timeout = 5;
        let slot = Arc::clone(&client.slot);
        let canceller = cancel_after.map(|ms| {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(ms));
                slot.cancel(false);
            })
        });
        let cs = client.session.get_mut();
        assert!(!cs.nntp_connect(c"user", c"pass", c"test", c"127.0.0.1"));
        let stage = cs.nntp_connect_stage;
        if let Some(c) = canceller {
            c.join().unwrap();
        }
        drop(client);
        server.join().unwrap();
        stage
    }

    #[test]
    fn connect_stage_tells_cancel_refusal_and_drop_apart() {
        use std::io::{BufRead, BufReader, Write};
        // no greeting, then cancelled: a cancel, not the address's fault
        assert_eq!(
            stage_after(
                |s| {
                    std::thread::sleep(Duration::from_millis(900));
                    drop(s);
                },
                Some(300)
            ),
            4
        );
        // the server refuses the login
        assert_eq!(
            stage_after(
                |mut s| {
                    s.write_all(b"200 hi\r\n").unwrap();
                    let mut r = BufReader::new(s.try_clone().unwrap());
                    let mut line = String::new();
                    r.read_line(&mut line).unwrap();
                    s.write_all(b"481 Authentication failed\r\n").unwrap();
                    let _ = r.read_line(&mut line);
                },
                None
            ),
            3
        );
        // the connection drops during the login: a network failure
        assert_eq!(
            stage_after(
                |mut s| {
                    s.write_all(b"200 hi\r\n").unwrap();
                    let mut r = BufReader::new(s.try_clone().unwrap());
                    let mut line = String::new();
                    r.read_line(&mut line).unwrap();
                },
                None
            ),
            2
        );
        // a bad greeting
        assert_eq!(stage_after(|mut s| s.write_all(b"502 busy\r\n").unwrap(), None), 2);
    }

    #[test]
    fn loopback_bind_connect_io_and_nntp_handshake() {
        let mut listener =
            NzbgetRsConn::new_client(b"127.0.0.1", 0, false, false, false, ConnHost::none());
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
            let mut srv = NzbgetRsConn::from_accepted(afd, false, 5, false, ConnHost::none());
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

        let mut client =
            NzbgetRsConn::new_client(b"127.0.0.1", port, false, false, false, ConnHost::none());
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
            let raw_conn = unsafe { nzbget_rs_conn_from_fd(fds[0], 0, 30, 0, ConnHost::none()) };
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
                    ConnStatus::Cancelled as isize
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

        fn read_suppress_slot(slot: usize) {
            // Simulate `TlsSocket::ReportError` reading suppression mid-handshake.
            let sup = unsafe {
                nzbget_rs_conn_ctl(
                    ptr::null_mut(),
                    23,
                    0,
                    slot,
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    0,
                )
            };
            assert!(sup == 0 || sup == 1);
        }

        struct MockTlsHost;

        impl ConnHostOps for MockTlsHost {
            unsafe fn tls_start(
                &self,
                _fd: c_int,
                _flags_verif: c_int,
                _deadline: usize,
                suppress: &AtomicBool,
                _host: &CStr,
                _cert: &CStr,
                _key: &CStr,
                _cipher: &CStr,
            ) -> TlsStarted {
                let obj = Box::new(MockTlsObj {
                    alive_magic: AtomicUsize::new(0xA11CE_5AFE),
                    suppress_slot: ptr::from_ref(suppress) as usize,
                });
                read_suppress_slot(obj.suppress_slot);
                let raw = std::num::NonZeroUsize::new(Box::into_raw(obj) as usize).unwrap();
                TlsStarted {
                    // SAFETY: freed only by `tls_close` below.
                    handle: Some(unsafe { TlsHandle::from_raw(raw) }),
                    ok: false, // failing handshake triggers cleanup
                    probe_failure: -1,
                }
            }

            unsafe fn tls_close(&self, sock: TlsHandle, _close: bool) {
                let obj = unsafe { Box::from_raw(sock.raw() as *mut MockTlsObj) };
                assert_eq!(obj.alive_magic.swap(0xDEAD, Ordering::SeqCst), 0xA11CE_5AFE);
                read_suppress_slot(obj.suppress_slot);
                drop(obj);
            }
        }

        let raw_conn = unsafe {
            nzbget_rs_conn_new(
                c"127.0.0.1".as_ptr(),
                563,
                1,
                0,
                0,
                ConnHost::new(Arc::new(MockTlsHost)),
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
