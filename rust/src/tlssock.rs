//! OpenSSL TLS for news server and web connections, the socket deadline used by
//! TLS cipher probes, and the OpenSSL side of the TLS cipher measurement.
//!
//! Ported from the C++ `TlsSocket`, `SocketDeadline`, `TlsHostCb` and
//! `TlsCipherBackend` that lived in `daemon/util/Util.h`. The connection code in
//! `net.rs` reaches all of it through [`conn_host`], which keeps the host
//! callback operations documented on [`crate::net::NzbgetRsConnHostFn`]. The only
//! part still answered by C++ is whether the current C++ `Thread` was stopped.

use std::ffi::{c_char, c_int, c_long, c_ulong, c_void, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::time::{Duration, Instant};

use openssl_sys as ossl;

mod sys {
    use super::{c_char, c_int, c_ulong};
    use openssl_sys::{SSL, SSL_CIPHER, SSL_CTX, X509_STORE};

    #[allow(non_camel_case_types)]
    pub enum stack_st_SSL_CIPHER {}

    // Functions openssl-sys does not declare. All exist in OpenSSL 1.1.1 and 3.x.
    extern "C" {
        pub fn SSL_set_fd(ssl: *mut SSL, fd: c_int) -> c_int;
        pub fn ERR_peek_error() -> c_ulong;
        pub fn ERR_error_string_n(e: c_ulong, buf: *mut c_char, len: usize);
        pub fn X509_STORE_load_locations(
            store: *mut X509_STORE,
            file: *const c_char,
            dir: *const c_char,
        ) -> c_int;
        pub fn SSL_CTX_set1_cert_store(ctx: *mut SSL_CTX, store: *mut X509_STORE);
        pub fn SSL_CIPHER_get_auth_nid(c: *const SSL_CIPHER) -> c_int;
        pub fn SSL_CIPHER_get_kx_nid(c: *const SSL_CIPHER) -> c_int;
        pub fn SSL_CIPHER_is_aead(c: *const SSL_CIPHER) -> c_int;
        pub fn SSL_get_ciphers(s: *const SSL) -> *mut stack_st_SSL_CIPHER;
        pub fn SSL_CTX_get_ciphers(c: *const SSL_CTX) -> *mut stack_st_SSL_CIPHER;
        pub fn OPENSSL_cleanse(ptr: *mut std::ffi::c_void, len: usize);
    }
}

const NID_KX_ECDHE: c_int = 1038;
const NID_KX_DHE: c_int = 1039;
const NID_AUTH_RSA: c_int = 1046;
const NID_AUTH_ECDSA: c_int = 1047;
const EVP_CIPH_GCM_MODE: c_ulong = 0x6;
const EVP_CIPH_CCM_MODE: c_ulong = 0x7;
const EVP_CTRL_AEAD_SET_IVLEN: c_int = 0x9;
const EVP_CTRL_AEAD_GET_TAG: c_int = 0x10;
const EVP_CTRL_AEAD_SET_TAG: c_int = 0x11;
const EVP_MAX_BLOCK_LENGTH: usize = 32;
const ERR_LIB_SSL: c_int = 20;
const SSL_R_NO_SHARED_CIPHER: c_int = 193;
const SSL_R_UNSUPPORTED_PROTOCOL: c_int = 258;
const SSL_R_SSLV3_ALERT_HANDSHAKE_FAILURE: c_int = 1040;
const SSL_R_TLSV1_ALERT_PROTOCOL_VERSION: c_int = 1070;
const CERT_VERIF_NONE: c_int = 0;
const CERT_VERIF_MINIMAL: c_int = 1;

fn log_error(msg: &[u8]) {
    crate::log::log_active_bytes(2, msg);
}

fn log_debug(msg: &[u8]) {
    if crate::log::active_debug_build() {
        crate::log::log_active_bytes(3, msg);
    }
}

unsafe fn opt_cstr<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        unsafe { CStr::from_ptr(p).to_bytes() }
    }
}

fn to_cstring(raw: &[u8]) -> CString {
    let nul = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    CString::new(&raw[..nul]).unwrap_or_default()
}

unsafe fn cstr_lossy(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

/// Bound on a TLS cipher probe: a fixed end time plus an optional cancellation
/// callback. The connection code carries a pointer to it as `deadline_ptr`.
pub struct Deadline {
    end: Instant,
    cancel: Option<(crate::ffi::tls_cipher_native::Cancel, usize)>,
}

impl Deadline {
    pub fn new(
        timeout: Duration,
        cancel: Option<(crate::ffi::tls_cipher_native::Cancel, *mut c_void)>,
    ) -> Self {
        Self {
            end: Instant::now() + timeout,
            cancel: cancel.map(|(f, c)| (f, c as usize)),
        }
    }

    pub fn stopped(&self) -> bool {
        Instant::now() >= self.end
            || self
                .cancel
                .is_some_and(|(f, c)| unsafe { f(c as *mut c_void) } != 0)
    }

    pub fn expired(&self) -> bool {
        Instant::now() >= self.end
    }

    fn remaining_ms(&self) -> i64 {
        if self.stopped() {
            -1
        } else {
            i64::try_from(
                self.end
                    .saturating_duration_since(Instant::now())
                    .as_millis(),
            )
            .unwrap_or(i64::MAX)
        }
    }

    pub fn wait(&self, fd: c_int, read: bool, write: bool) -> bool {
        crate::lifecycle::socket_deadline_wait(fd, read, write, || self.remaining_ms())
    }
}

unsafe fn deadline_ref<'a>(ptr: usize) -> Option<&'a Deadline> {
    if ptr == 0 {
        None
    } else {
        Some(unsafe { &*(ptr as *const Deadline) })
    }
}

static CERT_STORE: AtomicPtr<ossl::X509_STORE> = AtomicPtr::new(ptr::null_mut());

/// Initialises the OpenSSL library (was `TlsSocket::Init`).
pub fn init() {
    log_debug(b"Initializing TLS library");
    if unsafe { ossl::OPENSSL_init_ssl(0, ptr::null_mut()) } == 0 {
        log_error(b"Could not initialize the OpenSSL library");
    }
}

/// Loads the certificate store used to verify servers (was `TlsSocket::InitOptions`).
pub fn init_options(cert_store: &[u8]) {
    if cert_store.is_empty() {
        return;
    }
    let store = unsafe { ossl::X509_STORE_new() };
    if store.is_null() {
        log_error(b"Could not create certificate store");
        return;
    }
    if let Some(old) = ptr::NonNull::new(CERT_STORE.swap(store, Ordering::AcqRel)) {
        unsafe { ossl::X509_STORE_free(old.as_ptr()) };
    }
    let path = to_cstring(cert_store);
    if unsafe { sys::X509_STORE_load_locations(store, path.as_ptr(), ptr::null()) } == 0 {
        let mut msg = b"Could not load certificate store location from ".to_vec();
        msg.extend_from_slice(path.as_bytes());
        log_error(&msg);
    }
}

/// Drops the certificate store (was `TlsSocket::Final`). OpenSSL cleans up its
/// own global state at process exit.
pub fn final_cleanup() {
    if let Some(old) = ptr::NonNull::new(CERT_STORE.swap(ptr::null_mut(), Ordering::AcqRel)) {
        unsafe { ossl::X509_STORE_free(old.as_ptr()) };
    }
}

pub fn has_certificate_store() -> bool {
    !CERT_STORE.load(Ordering::Acquire).is_null()
}

/// Frees this thread's OpenSSL state (was `OpenSSL::StopSSLThread`). Looked up at
/// run time because LibreSSL has no `OPENSSL_thread_stop`.
pub fn stop_ssl_thread() {
    type StopFn = unsafe extern "C" fn();
    static STOP: std::sync::OnceLock<Option<StopFn>> = std::sync::OnceLock::new();
    let stop = STOP.get_or_init(|| {
        let sym = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"OPENSSL_thread_stop".as_ptr()) };
        if sym.is_null() {
            None
        } else {
            Some(unsafe { std::mem::transmute::<*mut c_void, StopFn>(sym) })
        }
    });
    if let Some(f) = stop {
        unsafe { f() };
    }
}

unsafe fn cipher_stack_len(stack: *mut sys::stack_st_SSL_CIPHER) -> c_int {
    if stack.is_null() {
        0
    } else {
        unsafe { ossl::OPENSSL_sk_num(stack.cast()) }
    }
}

unsafe fn cipher_stack_at(
    stack: *mut sys::stack_st_SSL_CIPHER,
    i: c_int,
) -> *const ossl::SSL_CIPHER {
    unsafe { ossl::OPENSSL_sk_value(stack.cast(), i) as *const ossl::SSL_CIPHER }
}

/// The eligibility flags the cipher measurement uses for a suite: AEAD (1),
/// certificate authentication (2) and ephemeral key exchange (4).
unsafe fn suite_flags(suite: *const ossl::SSL_CIPHER, tls13: bool) -> u32 {
    let auth = unsafe { sys::SSL_CIPHER_get_auth_nid(suite) };
    let kx = unsafe { sys::SSL_CIPHER_get_kx_nid(suite) };
    (if unsafe { sys::SSL_CIPHER_is_aead(suite) } != 0 {
        1
    } else {
        0
    }) | (if tls13 || auth == NID_AUTH_RSA || auth == NID_AUTH_ECDSA {
        2
    } else {
        0
    }) | (if tls13 || kx == NID_KX_ECDHE || kx == NID_KX_DHE {
        4
    } else {
        0
    })
}

unsafe fn suite_is_tls13(suite: *const ossl::SSL_CIPHER) -> bool {
    (unsafe { opt_cstr(ossl::SSL_CIPHER_get_version(suite)) }) == b"TLSv1.3"
}

unsafe fn suite_bits(suite: *const ossl::SSL_CIPHER, name: &[u8]) -> u32 {
    if name.windows(4).any(|w| w == b"CCM8") {
        64
    } else {
        unsafe { ossl::SSL_CIPHER_get_bits(suite, ptr::null_mut()) as u32 }
    }
}

/// One OpenSSL session over a socket the connection code owns.
pub struct TlsSocket {
    fd: c_int,
    host: CString,
    cert_file: CString,
    key_file: CString,
    cipher: CString,
    is_client: bool,
    cert_verif_level: c_int,
    suppress_slot: usize,
    connected: bool,
    probe_failure: c_int,
    ret_code: c_int,
    ctx: *mut ossl::SSL_CTX,
    ssl: *mut ossl::SSL,
    deadline: usize,
}

impl Drop for TlsSocket {
    fn drop(&mut self) {
        self.close();
        unsafe { ossl::ERR_clear_error() };
    }
}

impl TlsSocket {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fd: c_int,
        is_client: bool,
        host: &[u8],
        cert_file: &[u8],
        key_file: &[u8],
        cipher: &[u8],
        cert_verif_level: c_int,
    ) -> Self {
        Self {
            fd,
            host: to_cstring(host),
            cert_file: to_cstring(cert_file),
            key_file: to_cstring(key_file),
            cipher: to_cstring(cipher),
            is_client,
            cert_verif_level,
            suppress_slot: 0,
            connected: false,
            probe_failure: -1,
            ret_code: 0,
            ctx: ptr::null_mut(),
            ssl: ptr::null_mut(),
            deadline: 0,
        }
    }

    pub fn bind_suppress_errors(&mut self, slot: usize) {
        self.suppress_slot = slot;
    }

    pub fn set_deadline(&mut self, deadline: usize) {
        self.deadline = deadline;
    }

    pub fn probe_failure(&self) -> c_int {
        self.probe_failure
    }

    fn deadline(&self) -> Option<&Deadline> {
        unsafe { deadline_ref(self.deadline) }
    }

    fn deadline_stopped(&self) -> bool {
        self.deadline().is_some_and(Deadline::stopped)
    }

    pub fn start(&mut self) -> bool {
        self.ctx = unsafe { ossl::SSL_CTX_new(ossl::TLS_method()) };
        if self.ctx.is_null() {
            self.report_error(b"Could not create TLS context", false);
            return false;
        }

        if !self.cert_file.as_bytes().is_empty() && !self.key_file.as_bytes().is_empty() {
            if unsafe {
                ossl::SSL_CTX_use_certificate_chain_file(self.ctx, self.cert_file.as_ptr())
            } == 0
            {
                self.report_error(b"Could not load certificate file", false);
                self.close();
                return false;
            }
            if unsafe {
                ossl::SSL_CTX_use_PrivateKey_file(
                    self.ctx,
                    self.key_file.as_ptr(),
                    ossl::SSL_FILETYPE_PEM,
                )
            } == 0
            {
                self.report_error(b"Could not load key file", false);
                self.close();
                return false;
            }
        }

        let store = CERT_STORE.load(Ordering::Acquire);
        if self.is_client && !store.is_null() {
            unsafe {
                sys::SSL_CTX_set1_cert_store(self.ctx, store);
                ossl::SSL_CTX_set_verify(
                    self.ctx,
                    if self.cert_verif_level > CERT_VERIF_NONE {
                        ossl::SSL_VERIFY_PEER
                    } else {
                        ossl::SSL_VERIFY_NONE
                    },
                    None,
                );
            }
        }

        self.ssl = unsafe { ossl::SSL_new(self.ctx) };
        if self.ssl.is_null() {
            self.report_error(b"Could not create TLS session", false);
            self.close();
            return false;
        }

        if !self.set_cipher_suite() {
            self.report_error(b"Could not set cipher suite for SSL/TLS connection", false);
            self.close();
            return false;
        }

        if self.is_client
            && !self.host.as_bytes().is_empty()
            && unsafe { ossl::SSL_set_tlsext_host_name(self.ssl, self.host.as_ptr().cast_mut()) }
                == 0
        {
            self.report_error(b"Could not set host name for TLS", true);
            self.close();
            return false;
        }

        if unsafe { sys::SSL_set_fd(self.ssl, self.fd) } == 0 {
            self.report_error(b"Could not set the file descriptor for TLS", true);
            self.close();
            return false;
        }

        let mut ec;
        loop {
            if self.deadline_stopped() {
                self.close();
                return false;
            }
            ec = unsafe {
                if self.is_client {
                    ossl::SSL_connect(self.ssl)
                } else {
                    ossl::SSL_accept(self.ssl)
                }
            };
            if !(ec < 1 && self.retry_io(ec)) {
                break;
            }
        }
        let should_verify = !store.is_null() && self.cert_verif_level > CERT_VERIF_NONE;
        if ec < 1 {
            if self.deadline != 0 {
                let code = unsafe { ossl::ERR_peek_last_error() };
                let reason = ossl::ERR_GET_REASON(code);
                if ossl::ERR_GET_LIB(code) == ERR_LIB_SSL
                    && matches!(
                        reason,
                        SSL_R_SSLV3_ALERT_HANDSHAKE_FAILURE
                            | SSL_R_TLSV1_ALERT_PROTOCOL_VERSION
                            | SSL_R_NO_SHARED_CIPHER
                            | SSL_R_UNSUPPORTED_PROTOCOL
                    )
                {
                    // Explicit rejection of this single-suite handshake.
                    self.probe_failure = 0;
                }
            }
            if self.is_client && should_verify {
                let verify_res = unsafe { ossl::SSL_get_verify_result(self.ssl) };
                if verify_res != c_long::from(ossl::X509_V_OK) {
                    self.probe_failure = -4;
                    let reason =
                        unsafe { cstr_lossy(ossl::X509_verify_cert_error_string(verify_res)) };
                    let msg = format!(
                        "TLS certificate verification failed for {}: {}. For more info visit https://github.com/xbmc4lyfe/RustyGet",
                        self.host.to_string_lossy(),
                        reason
                    );
                    log_error(msg.as_bytes());
                    self.close();
                    return false;
                }
            }
            let msg = format!("TLS handshake failed for {}", self.host.to_string_lossy());
            self.report_error(msg.as_bytes(), true);
            self.close();
            return false;
        }

        if self.is_client && should_verify && !self.validate_cert() {
            self.probe_failure = -4;
            self.close();
            return false;
        }

        let current = unsafe { ossl::SSL_get_current_cipher(self.ssl) };
        if !current.is_null() && crate::log::active_debug_build() {
            let msg = format!(
                "TLS connection established: {} using {}",
                unsafe { cstr_lossy(ossl::SSL_get_version(self.ssl)) },
                unsafe { cstr_lossy(ossl::SSL_CIPHER_get_name(current)) }
            );
            log_debug(msg.as_bytes());
        }

        self.connected = true;
        true
    }

    pub fn close(&mut self) {
        if !self.ssl.is_null() && self.connected && self.deadline == 0 {
            // A zero result means the bidirectional shutdown is not complete yet.
            if unsafe { ossl::SSL_shutdown(self.ssl) } == 0 {
                unsafe { ossl::SSL_shutdown(self.ssl) };
            }
        }
        if !self.ctx.is_null() {
            unsafe { ossl::SSL_CTX_free(self.ctx) };
            self.ctx = ptr::null_mut();
        }
        if !self.ssl.is_null() {
            unsafe { ossl::SSL_free(self.ssl) };
            self.ssl = ptr::null_mut();
        }
    }

    pub fn send(&mut self, buf: *const c_char, size: c_int) -> c_int {
        loop {
            if self.deadline_stopped() {
                return -1;
            }
            self.ret_code = unsafe { ossl::SSL_write(self.ssl, buf.cast(), size) };
            if !(self.ret_code <= 0 && self.retry_io(self.ret_code)) {
                break;
            }
        }
        if self.ret_code < 0 {
            let msg: &[u8] = if unsafe { sys::ERR_peek_error() } == 0 {
                b"Could not write to TLS-Socket: Connection closed by remote host"
            } else {
                b"Could not write to TLS-Socket"
            };
            self.report_error(msg, true);
            return -1;
        }
        self.ret_code
    }

    pub fn recv(&mut self, buf: *mut c_char, size: c_int) -> c_int {
        loop {
            if self.deadline_stopped() {
                return -1;
            }
            self.ret_code = unsafe { ossl::SSL_read(self.ssl, buf.cast(), size) };
            if !(self.ret_code <= 0 && self.retry_io(self.ret_code)) {
                break;
            }
        }
        if self.ret_code < 0 {
            let msg: &[u8] = if unsafe { sys::ERR_peek_error() } == 0 {
                b"Could not read from TLS-Socket: Connection closed by remote host"
            } else {
                b"Could not read from TLS-Socket"
            };
            self.report_error(msg, true);
            return -1;
        }
        self.ret_code
    }

    pub fn negotiated_cipher(&self) -> *const c_char {
        if self.ssl.is_null() {
            return c"".as_ptr();
        }
        let current = unsafe { ossl::SSL_get_current_cipher(self.ssl) };
        if current.is_null() {
            c"(NONE)".as_ptr()
        } else {
            unsafe { ossl::SSL_CIPHER_get_name(current) }
        }
    }

    pub fn negotiated_version(&self) -> c_int {
        if self.ssl.is_null() {
            0
        } else {
            unsafe { ossl::SSL_version(self.ssl) }
        }
    }

    fn retry_io(&self, result: c_int) -> bool {
        let Some(deadline) = self.deadline() else {
            return false;
        };
        let error = unsafe { ossl::SSL_get_error(self.ssl, result) };
        if error != ossl::SSL_ERROR_WANT_READ && error != ossl::SSL_ERROR_WANT_WRITE {
            return false;
        }
        deadline.wait(
            self.fd,
            error == ossl::SSL_ERROR_WANT_READ,
            error == ossl::SSL_ERROR_WANT_WRITE,
        )
    }

    fn report_error(&self, err_msg: &[u8], suppressable: bool) {
        let mut errcode = unsafe { ossl::ERR_get_error() };
        loop {
            let mut errstr = [0 as c_char; 1024];
            unsafe { sys::ERR_error_string_n(errcode, errstr.as_mut_ptr(), errstr.len()) };
            let detail = unsafe { CStr::from_ptr(errstr.as_ptr()) }.to_bytes();
            let mut msg = err_msg.to_vec();
            if suppressable
                && self.suppress_slot != 0
                && unsafe { crate::net::nzbget_rs_tls_suppressed(self.suppress_slot) } != 0
            {
                msg.extend_from_slice(b": ");
                msg.extend_from_slice(detail);
                log_debug(&msg);
            } else if errcode != 0 {
                msg.extend_from_slice(b": ");
                msg.extend_from_slice(detail);
                log_error(&msg);
            } else {
                log_error(&msg);
            }
            errcode = unsafe { ossl::ERR_get_error() };
            if errcode == 0 {
                break;
            }
        }
    }

    /// Configures the allowed suites for TLS 1.3 and older protocols from one
    /// combined string. A single modern suite is an exact pin: without a
    /// protocol bound, setting only a TLS 1.2 list leaves the default TLS 1.3
    /// suites usable (and the other way round). Empty values and custom lists
    /// keep their historical negotiation behaviour.
    fn set_cipher_suite(&mut self) -> bool {
        let cipher = self.cipher.as_bytes();
        if cipher.is_empty() {
            return true;
        }
        if !cipher.contains(&b':') {
            let ciphers = unsafe { sys::SSL_get_ciphers(self.ssl) };
            for i in 0..unsafe { cipher_stack_len(ciphers) } {
                let candidate = unsafe { cipher_stack_at(ciphers, i) };
                let name = unsafe { opt_cstr(ossl::SSL_CIPHER_get_name(candidate)) };
                if cipher != name {
                    continue;
                }
                let tls13 = unsafe { suite_is_tls13(candidate) };
                let flags = unsafe { suite_flags(candidate, tls13) };
                let bits = unsafe { suite_bits(candidate, name) };
                if crate::ffi::nzbget_rs_tls_cipher_pin(if tls13 { 13 } else { 12 }, bits, flags)
                    == 0
                {
                    break;
                }
                let version = if tls13 {
                    ossl::TLS1_3_VERSION
                } else {
                    ossl::TLS1_2_VERSION
                };
                let minimum = unsafe { ossl::SSL_get_min_proto_version(self.ssl) };
                let maximum = unsafe { ossl::SSL_get_max_proto_version(self.ssl) };
                if (minimum != 0 && version < minimum) || (maximum != 0 && version > maximum) {
                    return false;
                }
                if unsafe { ossl::SSL_set_min_proto_version(self.ssl, version as _) } == 0
                    || unsafe { ossl::SSL_set_max_proto_version(self.ssl, version as _) } == 0
                {
                    return false;
                }
                return unsafe {
                    if tls13 {
                        ossl::SSL_set_ciphersuites(self.ssl, self.cipher.as_ptr()) == 1
                    } else {
                        ossl::SSL_set_cipher_list(self.ssl, self.cipher.as_ptr()) == 1
                    }
                };
            }
        }
        let tls13 = unsafe { ossl::SSL_set_ciphersuites(self.ssl, self.cipher.as_ptr()) } != 0;
        let legacy = unsafe { ossl::SSL_set_cipher_list(self.ssl, self.cipher.as_ptr()) } != 0;
        if !tls13 {
            unsafe { ossl::ERR_clear_error() };
        }
        tls13 || legacy
    }

    fn validate_cert(&self) -> bool {
        // A server certificate must have been presented during the negotiation.
        let cert = unsafe { ossl::SSL_get1_peer_certificate(self.ssl) };
        if cert.is_null() {
            let msg = format!(
                "TLS certificate verification failed for {}: no certificate provided by server. For more info visit https://github.com/xbmc4lyfe/RustyGet",
                self.host.to_string_lossy()
            );
            log_error(msg.as_bytes());
            return false;
        }
        struct Owned(*mut ossl::X509);
        impl Drop for Owned {
            fn drop(&mut self) {
                unsafe { ossl::X509_free(self.0) };
            }
        }
        let cert = Owned(cert);

        let host = self.host.as_bytes();
        if self.cert_verif_level > CERT_VERIF_MINIMAL
            && !host.is_empty()
            && unsafe {
                ossl::X509_check_host(cert.0, self.host.as_ptr(), host.len(), 0, ptr::null_mut())
            } == 0
        {
            let mut cert_host = String::from("(null)");
            unsafe {
                let subject = ossl::X509_get_subject_name(cert.0);
                let loc = ossl::X509_NAME_get_index_by_NID(subject, ossl::NID_commonName, -1);
                if loc >= 0 {
                    let entry = ossl::X509_NAME_get_entry(subject, loc);
                    if !entry.is_null() {
                        let data = ossl::X509_NAME_ENTRY_get_data(entry);
                        if !data.is_null() {
                            let raw = ossl::ASN1_STRING_get0_data(data);
                            if !raw.is_null() {
                                cert_host = cstr_lossy(raw.cast());
                            }
                        }
                    }
                }
            }
            let msg = format!(
                "TLS certificate verification failed for {}: certificate hostname mismatch ({}). For more info visit https://github.com/xbmc4lyfe/RustyGet",
                self.host.to_string_lossy(),
                cert_host
            );
            log_error(msg.as_bytes());
            return false;
        }
        true
    }
}

fn cpp_thread_stopped() -> bool {
    crate::ffi::cpp_thread_host_cb().is_some_and(|cb| unsafe {
        cb(
            ptr::null_mut(),
            2,
            0,
            0,
            0,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
        ) != 0
    })
}

/// Host callback for connections. Operations are documented on
/// [`crate::net::NzbgetRsConnHostFn`].
///
/// # Safety
/// Arguments follow that documentation: `p1` is a deadline pointer, a
/// `[deadline, suppress_slot]` pair or a socket returned by operation 4,
/// strings are null or NUL-terminated, and `buf` / `out_int` are valid for the
/// operation.
pub unsafe extern "C" fn conn_host(
    _ctx: *mut c_void,
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
    match op {
        0 => {
            log_debug(unsafe { opt_cstr(s1) });
            0
        }
        1 => {
            let msg = unsafe { opt_cstr(s1) };
            if a1 != 0 {
                log_debug(msg);
            } else {
                log_error(msg);
            }
            0
        }
        2 => {
            let deadline_stopped = unsafe { deadline_ref(p1) }.is_some_and(Deadline::stopped);
            isize::from(
                crate::ffi::is_rust_thread_stopped() || cpp_thread_stopped() || deadline_stopped,
            )
        }
        3 => {
            let write = !out_int.is_null() && unsafe { *out_int } != 0;
            isize::from(unsafe { deadline_ref(p1) }.is_some_and(|d| d.wait(a1, a2 != 0, write)))
        }
        4 if !crate::ffi::build_tls_disabled() => {
            let ptrs = p1 as *const usize;
            let mut sock = Box::new(TlsSocket::new(
                a1,
                (a2 & 1) != 0,
                unsafe { opt_cstr(s1) },
                unsafe { opt_cstr(s2) },
                unsafe { opt_cstr(s3) },
                unsafe { opt_cstr(buf) },
                (a2 >> 2) & 0xff,
            ));
            if !ptrs.is_null() {
                sock.bind_suppress_errors(unsafe { *ptrs.add(1) });
                sock.set_deadline(unsafe { *ptrs });
            }
            let started = sock.start();
            if !out_int.is_null() {
                unsafe {
                    *out_int = c_int::from(started);
                    *out_int.add(1) = sock.probe_failure();
                }
            }
            Box::into_raw(sock) as isize
        }
        5 if p1 != 0 => unsafe { (*(p1 as *mut TlsSocket)).send(s1, a1) as isize },
        6 if p1 != 0 => unsafe { (*(p1 as *mut TlsSocket)).recv(buf, a1) as isize },
        7 if p1 != 0 => {
            let mut sock = unsafe { Box::from_raw(p1 as *mut TlsSocket) };
            if a1 != 0 {
                sock.close();
            }
            0
        }
        9 if p1 != 0 => {
            let sock = unsafe { &*(p1 as *const TlsSocket) };
            if !out_int.is_null() {
                unsafe { *out_int = sock.negotiated_version() };
            }
            sock.negotiated_cipher() as isize
        }
        10 => {
            if !crate::ffi::build_tls_disabled() {
                stop_ssl_thread();
            }
            0
        }
        _ => 0,
    }
}

/// OpenSSL side of the TLS cipher measurement: the callback table that
/// `tlscipher/native.rs` drives (was C++ `TlsCipherBackend`).
pub(crate) mod cipher {
    use super::*;
    use crate::ffi::tls_cipher_native::{Cancel, Emit, NativeServer};

    const RECORD_SIZE: usize = 16384;

    struct CipherCtx(*mut ossl::EVP_CIPHER_CTX);
    impl Drop for CipherCtx {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { ossl::EVP_CIPHER_CTX_free(self.0) };
            }
        }
    }

    pub(super) struct Crypto {
        cipher: *const ossl::EVP_CIPHER,
        encrypt: CipherCtx,
        decrypt: CipherCtx,
        key: [u8; 64],
        prefix: [u8; 4],
        plaintext: Box<[u8; RECORD_SIZE]>,
        output: Box<[u8; RECORD_SIZE + EVP_MAX_BLOCK_LENGTH]>,
        ciphertext: Vec<Box<[u8; RECORD_SIZE + EVP_MAX_BLOCK_LENGTH]>>,
        tags: Vec<[u8; 16]>,
        nonces: Vec<[u8; 12]>,
        first: u64,
        tag_size: c_int,
        ccm: bool,
        encrypt_keyed: bool,
        decrypt_keyed: bool,
    }

    impl Drop for Crypto {
        fn drop(&mut self) {
            unsafe { sys::OPENSSL_cleanse(self.key.as_mut_ptr().cast(), self.key.len()) };
        }
    }

    fn aad(tag_size: c_int) -> [u8; 5] {
        [23, 3, 3, 0x40, tag_size as u8]
    }

    impl Crypto {
        pub(super) fn new() -> Self {
            Self {
                cipher: ptr::null(),
                encrypt: CipherCtx(unsafe { ossl::EVP_CIPHER_CTX_new() }),
                decrypt: CipherCtx(unsafe { ossl::EVP_CIPHER_CTX_new() }),
                key: [0; 64],
                prefix: [0; 4],
                plaintext: Box::new([0; RECORD_SIZE]),
                output: Box::new([0; RECORD_SIZE + EVP_MAX_BLOCK_LENGTH]),
                ciphertext: Vec::new(),
                tags: Vec::new(),
                nonces: Vec::new(),
                first: 0,
                tag_size: 16,
                ccm: false,
                encrypt_keyed: false,
                decrypt_keyed: false,
            }
        }

        pub(super) fn init(&mut self, name: &[u8]) -> bool {
            let mut name = name.to_vec();
            if name.ends_with(b"@8") {
                self.tag_size = 8;
                name.truncate(name.len() - 2);
            }
            let cname = to_cstring(&name);
            self.cipher = unsafe { ossl::EVP_get_cipherbyname(cname.as_ptr()) };
            if self.cipher.is_null() || self.encrypt.0.is_null() || self.decrypt.0.is_null() {
                return false;
            }
            let mode =
                unsafe { ossl::EVP_CIPHER_flags(self.cipher) } & (ossl::EVP_CIPH_MODE as c_ulong);
            let key_size = unsafe { ossl::EVP_CIPHER_key_length(self.cipher) };
            self.ccm = mode == EVP_CIPH_CCM_MODE;
            if mode != EVP_CIPH_GCM_MODE
                && !self.ccm
                && unsafe { ossl::EVP_CIPHER_nid(self.cipher) } != ossl::NID_chacha20_poly1305
            {
                return false;
            }
            if key_size <= 0
                || key_size as usize > self.key.len()
                || unsafe { ossl::RAND_bytes(self.key.as_mut_ptr(), key_size) } != 1
                || unsafe { ossl::RAND_bytes(self.prefix.as_mut_ptr(), self.prefix.len() as c_int) }
                    != 1
            {
                return false;
            }
            for (i, b) in self.plaintext.iter_mut().enumerate() {
                *b = ((i * 31 + i / 251) & 255) as u8;
            }
            unsafe {
                ossl::EVP_EncryptInit_ex(
                    self.encrypt.0,
                    self.cipher,
                    ptr::null_mut(),
                    ptr::null(),
                    ptr::null(),
                ) == 1
                    && ossl::EVP_DecryptInit_ex(
                        self.decrypt.0,
                        self.cipher,
                        ptr::null_mut(),
                        ptr::null(),
                        ptr::null(),
                    ) == 1
                    && ossl::EVP_CIPHER_CTX_ctrl(
                        self.encrypt.0,
                        EVP_CTRL_AEAD_SET_IVLEN,
                        12,
                        ptr::null_mut(),
                    ) == 1
                    && ossl::EVP_CIPHER_CTX_ctrl(
                        self.decrypt.0,
                        EVP_CTRL_AEAD_SET_IVLEN,
                        12,
                        ptr::null_mut(),
                    ) == 1
            }
        }

        pub(super) fn prepare(&mut self, sequence: u64, count: u32) -> bool {
            if count == 0 || count > 64 || sequence > u64::MAX - u64::from(count) {
                return false;
            }
            let count = count as usize;
            self.first = sequence;
            self.ciphertext
                .resize_with(count, || Box::new([0; RECORD_SIZE + EVP_MAX_BLOCK_LENGTH]));
            self.tags.resize(count, [0; 16]);
            self.nonces.resize(count, [0; 12]);
            let enc = self.encrypt.0;
            for i in 0..count {
                let nonce = &mut self.nonces[i];
                nonce[..4].copy_from_slice(&self.prefix);
                nonce[4..].copy_from_slice(&(sequence + i as u64).to_be_bytes());
                let mut size: c_int = 0;
                let mut tail: c_int = 0;
                let aad = aad(self.tag_size);
                unsafe {
                    if self.ccm
                        && ossl::EVP_CIPHER_CTX_ctrl(
                            enc,
                            EVP_CTRL_AEAD_SET_TAG,
                            self.tag_size,
                            ptr::null_mut(),
                        ) != 1
                    {
                        return false;
                    }
                    let key = if self.encrypt_keyed {
                        ptr::null()
                    } else {
                        self.key.as_ptr()
                    };
                    if ossl::EVP_EncryptInit_ex(
                        enc,
                        ptr::null(),
                        ptr::null_mut(),
                        key,
                        self.nonces[i].as_ptr(),
                    ) != 1
                    {
                        return false;
                    }
                    self.encrypt_keyed = true;
                    if self.ccm
                        && ossl::EVP_EncryptUpdate(
                            enc,
                            ptr::null_mut(),
                            &mut size,
                            ptr::null(),
                            RECORD_SIZE as c_int,
                        ) != 1
                    {
                        return false;
                    }
                    let out = self.ciphertext[i].as_mut_ptr();
                    if ossl::EVP_EncryptUpdate(
                        enc,
                        ptr::null_mut(),
                        &mut size,
                        aad.as_ptr(),
                        aad.len() as c_int,
                    ) != 1
                        || ossl::EVP_EncryptUpdate(
                            enc,
                            out,
                            &mut size,
                            self.plaintext.as_ptr(),
                            RECORD_SIZE as c_int,
                        ) != 1
                        || size as usize != RECORD_SIZE
                        || ossl::EVP_EncryptFinal_ex(enc, out.add(size as usize), &mut tail) != 1
                        || tail != 0
                        || ossl::EVP_CIPHER_CTX_ctrl(
                            enc,
                            EVP_CTRL_AEAD_GET_TAG,
                            self.tag_size,
                            self.tags[i].as_mut_ptr().cast(),
                        ) != 1
                    {
                        return false;
                    }
                }
            }
            if self.record(sequence) != RECORD_SIZE as c_int {
                return false;
            }
            self.tags[0][0] ^= 1;
            let rejected = self.record(sequence) == -1;
            self.tags[0][0] ^= 1;
            unsafe { ossl::ERR_clear_error() };
            rejected && self.record(sequence) == RECORD_SIZE as c_int
        }

        pub(super) fn record(&mut self, sequence: u64) -> c_int {
            if sequence < self.first || sequence - self.first >= self.ciphertext.len() as u64 {
                return -1;
            }
            let i = (sequence - self.first) as usize;
            let aad = aad(self.tag_size);
            let mut size: c_int = 0;
            let mut tail: c_int = 0;
            let dec = self.decrypt.0;
            unsafe {
                if self.ccm
                    && ossl::EVP_CIPHER_CTX_ctrl(
                        dec,
                        EVP_CTRL_AEAD_SET_TAG,
                        self.tag_size,
                        self.tags[i].as_mut_ptr().cast(),
                    ) != 1
                {
                    return -1;
                }
                let key = if self.decrypt_keyed {
                    ptr::null()
                } else {
                    self.key.as_ptr()
                };
                if ossl::EVP_DecryptInit_ex(
                    dec,
                    ptr::null(),
                    ptr::null_mut(),
                    key,
                    self.nonces[i].as_ptr(),
                ) != 1
                {
                    return -1;
                }
                self.decrypt_keyed = true;
                if self.ccm
                    && ossl::EVP_DecryptUpdate(
                        dec,
                        ptr::null_mut(),
                        &mut size,
                        ptr::null(),
                        RECORD_SIZE as c_int,
                    ) != 1
                {
                    return -1;
                }
                let out = self.output.as_mut_ptr();
                if ossl::EVP_DecryptUpdate(
                    dec,
                    ptr::null_mut(),
                    &mut size,
                    aad.as_ptr(),
                    aad.len() as c_int,
                ) != 1
                    || ossl::EVP_DecryptUpdate(
                        dec,
                        out,
                        &mut size,
                        self.ciphertext[i].as_ptr(),
                        RECORD_SIZE as c_int,
                    ) != 1
                    || size as usize != RECORD_SIZE
                {
                    return -1;
                }
                if !self.ccm
                    && (ossl::EVP_CIPHER_CTX_ctrl(
                        dec,
                        EVP_CTRL_AEAD_SET_TAG,
                        self.tag_size,
                        self.tags[i].as_mut_ptr().cast(),
                    ) != 1
                        || ossl::EVP_DecryptFinal_ex(dec, out.add(size as usize), &mut tail) != 1
                        || tail != 0)
                {
                    return -1;
                }
            }
            if self.output[..RECORD_SIZE] == self.plaintext[..] {
                RECORD_SIZE as c_int
            } else {
                -1
            }
        }
    }

    /// Context owned by the callback table: the last configuration snapshot,
    /// kept alive until the next snapshot call.
    struct Context {
        snapshot: Vec<u8>,
    }

    pub(crate) unsafe extern "C" fn snapshot(
        context: *mut c_void,
        data: *mut *const u8,
        length: *mut usize,
    ) -> c_int {
        let mut buf = crate::ffi::RsBuf {
            data: ptr::null_mut(),
            len: 0,
            cap: 0,
        };
        let ok = unsafe {
            crate::ffi::nzbget_rs_script_config_exec(
                1,
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null_mut(),
                None,
                &mut buf,
            )
        };
        if ok == 0 {
            return 0;
        }
        let ctx = unsafe { &mut *context.cast::<Context>() };
        ctx.snapshot = if buf.data.is_null() {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(buf.data.cast::<u8>(), buf.len) }.to_vec()
        };
        unsafe { crate::ffi::nzbget_rs_free(buf) };
        unsafe {
            *data = ctx.snapshot.as_ptr();
            *length = ctx.snapshot.len();
        }
        1
    }

    pub(crate) unsafe extern "C" fn suites(
        _: *mut c_void,
        emit: Emit,
        context: *mut c_void,
    ) -> c_int {
        let tls = unsafe { ossl::SSL_CTX_new(ossl::TLS_method()) };
        if tls.is_null() {
            return 0;
        }
        struct Owned(*mut ossl::SSL_CTX);
        impl Drop for Owned {
            fn drop(&mut self) {
                unsafe { ossl::SSL_CTX_free(self.0) };
            }
        }
        let tls = Owned(tls);
        let minimum = unsafe { ossl::SSL_CTX_get_min_proto_version(tls.0) };
        let maximum = unsafe { ossl::SSL_CTX_get_max_proto_version(tls.0) };
        let list = unsafe { sys::SSL_CTX_get_ciphers(tls.0) };
        for i in 0..unsafe { cipher_stack_len(list) } {
            let suite = unsafe { cipher_stack_at(list, i) };
            let name_ptr = unsafe { ossl::SSL_CIPHER_get_name(suite) };
            let name = unsafe { opt_cstr(name_ptr) };
            let tls13 = unsafe { suite_is_tls13(suite) };
            let version = if tls13 {
                ossl::TLS1_3_VERSION
            } else {
                ossl::TLS1_2_VERSION
            };
            if (minimum != 0 && version < minimum) || (maximum != 0 && version > maximum) {
                continue;
            }
            let flags = unsafe { suite_flags(suite, tls13) };
            let bits = unsafe { suite_bits(suite, name) };
            // EVP_get_cipherbynid(nid) is EVP_get_cipherbyname(OBJ_nid2sn(nid)), and
            // the name of such a cipher is OBJ_nid2sn of its nid.
            let short = unsafe { ossl::OBJ_nid2sn(ossl::SSL_CIPHER_get_cipher_nid(suite)) };
            let evp = if short.is_null() {
                ptr::null()
            } else {
                unsafe { ossl::EVP_get_cipherbyname(short) }
            };
            let mut bulk = if evp.is_null() {
                b"unavailable".to_vec()
            } else {
                unsafe { opt_cstr(ossl::OBJ_nid2sn(ossl::EVP_CIPHER_nid(evp))) }.to_vec()
            };
            if name.windows(4).any(|w| w == b"CCM8") {
                bulk.extend_from_slice(b"@8");
            }
            let bulk = to_cstring(&bulk);
            if unsafe {
                emit(
                    context,
                    name_ptr,
                    bulk.as_ptr(),
                    if tls13 { 13 } else { 12 },
                    bits,
                    flags,
                )
            } == 0
            {
                return 0;
            }
        }
        1
    }

    pub(crate) unsafe extern "C" fn prepare(
        _: *mut c_void,
        algorithm: *const c_char,
    ) -> *mut c_void {
        let mut crypto = Box::new(Crypto::new());
        if crypto.init(unsafe { opt_cstr(algorithm) }) {
            Box::into_raw(crypto).cast()
        } else {
            ptr::null_mut()
        }
    }

    pub(crate) unsafe extern "C" fn prepare_batch(
        context: *mut c_void,
        first: u64,
        count: u32,
    ) -> c_int {
        c_int::from(unsafe { &mut *context.cast::<Crypto>() }.prepare(first, count))
    }

    pub(crate) unsafe extern "C" fn record(context: *mut c_void, sequence: u64) -> c_int {
        unsafe { &mut *context.cast::<Crypto>() }.record(sequence)
    }

    pub(crate) unsafe extern "C" fn free_crypto(context: *mut c_void) {
        if !context.is_null() {
            drop(unsafe { Box::from_raw(context.cast::<Crypto>()) });
        }
        unsafe { ossl::ERR_clear_error() };
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) unsafe extern "C" fn probe(
        _: *mut c_void,
        server: *const NativeServer,
        cipher: *const c_char,
        protocol: c_int,
        numeric: *const c_char,
        milliseconds: u64,
        cancel: Cancel,
        cancel_context: *mut c_void,
    ) -> c_int {
        if unsafe { cancel(cancel_context) } != 0 {
            return -6;
        }
        if milliseconds == 0 {
            return -3;
        }
        if milliseconds > 10000 {
            return -1;
        }
        let Some(srv) = (unsafe { server.cast::<crate::net::NzbgetTlsServer>().as_ref() }) else {
            return -1;
        };
        if srv.verification != 0 && !has_certificate_store() {
            return -4;
        }
        let deadline = Deadline::new(
            Duration::from_millis(milliseconds),
            Some((cancel, cancel_context)),
        );
        let expected = if protocol == 13 {
            ossl::TLS1_3_VERSION
        } else {
            ossl::TLS1_2_VERSION
        };
        let mut result = unsafe {
            crate::net::nzbget_rs_serverpool_probe_tls(
                (server as *const NativeServer).cast(),
                if cipher.is_null() {
                    c"".as_ptr()
                } else {
                    cipher
                },
                expected,
                if numeric.is_null() {
                    c"".as_ptr()
                } else {
                    numeric
                },
                &deadline as *const Deadline as usize,
            )
        };
        if result == -2 {
            return -2;
        }
        if unsafe { cancel(cancel_context) } != 0 {
            result = -6;
        } else if deadline.expired() {
            result = -3;
        }
        result
    }

    pub(crate) unsafe extern "C" fn commit(
        _: *mut c_void,
        original: *const u8,
        original_length: usize,
        replacement: *const u8,
        replacement_length: usize,
        cancel: Cancel,
        cancel_context: *mut c_void,
    ) -> c_int {
        unsafe {
            crate::ffi::nzbget_rs_script_config_exec(
                2,
                original.cast(),
                original_length,
                replacement.cast(),
                replacement_length,
                cancel_context,
                Some(cancel),
                ptr::null_mut(),
            )
        }
    }

    pub(crate) unsafe extern "C" fn destroy(context: *mut c_void) {
        if !context.is_null() {
            drop(unsafe { Box::from_raw(context.cast::<Context>()) });
        }
    }

    /// A fresh callback table; its context transfers to the cipher job at start.
    pub(crate) fn backend() -> crate::ffi::tls_cipher_native::Io {
        crate::ffi::tls_cipher_native::Io::new(
            Box::into_raw(Box::new(Context {
                snapshot: Vec::new(),
            }))
            .cast(),
            snapshot,
            suites,
            prepare,
            prepare_batch,
            record,
            free_crypto,
            probe,
            commit,
            destroy,
        )
    }
}

/// Ported from the C++ unit test `tests/connect/TlsCipherTest.h`: the OpenSSL
/// cipher backend measures every eligible suite, and probes against an owned
/// loopback TLS server honour pins, deadlines, certificates and cancellation.
#[cfg(test)]
mod tests {
    use super::cipher as backend;
    use super::*;
    use openssl::asn1::Asn1Time;
    use openssl::bn::BigNum;
    use openssl::hash::MessageDigest;
    use openssl::pkey::PKey;
    use openssl::rsa::Rsa;
    use openssl::ssl::{SslAcceptor, SslMethod, SslVersion};
    use openssl::x509::{X509NameBuilder, X509};
    use std::collections::BTreeSet;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize};
    use std::sync::{Arc, Mutex};

    // Probes share process-wide OpenSSL and signal state; run them one at a time.
    static SERIAL: Mutex<()> = Mutex::new(());

    struct IgnoreSigpipe(libc::sighandler_t);
    impl IgnoreSigpipe {
        fn new() -> Self {
            Self(unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) })
        }
    }
    impl Drop for IgnoreSigpipe {
        fn drop(&mut self) {
            if self.0 != libc::SIG_ERR {
                unsafe { libc::signal(libc::SIGPIPE, self.0) };
            }
        }
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Normal,
        BadLogin,
        StallGreeting,
    }

    struct Loopback {
        port: u16,
        negotiated: Arc<AtomicI32>,
        auth_commands: Arc<AtomicUsize>,
        article_commands: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
    }

    fn version(v: c_int) -> SslVersion {
        if v == ossl::TLS1_3_VERSION {
            SslVersion::TLS1_3
        } else {
            SslVersion::TLS1_2
        }
    }

    impl Loopback {
        fn new(minimum: c_int, maximum: c_int, legacy: &str, modern: &str, mode: Mode) -> Self {
            let rsa = Rsa::generate(2048).unwrap();
            let key = PKey::from_rsa(rsa).unwrap();
            let mut name = X509NameBuilder::new().unwrap();
            name.append_entry_by_text("CN", "localhost").unwrap();
            let name = name.build();
            let mut cert = X509::builder().unwrap();
            cert.set_version(2).unwrap();
            cert.set_serial_number(&BigNum::from_u32(1).unwrap().to_asn1_integer().unwrap())
                .unwrap();
            cert.set_not_before(&Asn1Time::from_unix(now_unix() - 60).unwrap())
                .unwrap();
            cert.set_not_after(&Asn1Time::from_unix(now_unix() + 3600).unwrap())
                .unwrap();
            cert.set_subject_name(&name).unwrap();
            cert.set_issuer_name(&name).unwrap();
            cert.set_pubkey(&key).unwrap();
            cert.sign(&key, MessageDigest::sha256()).unwrap();
            let cert = cert.build();

            let mut acceptor =
                SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server()).unwrap();
            acceptor
                .set_min_proto_version(Some(version(minimum)))
                .unwrap();
            acceptor
                .set_max_proto_version(Some(version(maximum)))
                .unwrap();
            acceptor.set_cipher_list(legacy).unwrap();
            acceptor.set_ciphersuites(modern).unwrap();
            acceptor.set_certificate(&cert).unwrap();
            acceptor.set_private_key(&key).unwrap();
            let acceptor = acceptor.build();

            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let port = listener.local_addr().unwrap().port();
            let negotiated = Arc::new(AtomicI32::new(0));
            let auth_commands = Arc::new(AtomicUsize::new(0));
            let article_commands = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let (n, a, r, st) = (
                negotiated.clone(),
                auth_commands.clone(),
                article_commands.clone(),
                stop.clone(),
            );
            let worker = std::thread::spawn(move || {
                while !st.load(Ordering::SeqCst) {
                    let socket = match listener.accept() {
                        Ok((s, _)) => s,
                        Err(_) => {
                            std::thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                    };
                    socket.set_nonblocking(false).unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    socket
                        .set_write_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    if let Ok(mut tls) = acceptor.accept(socket) {
                        let v = match tls.ssl().version2() {
                            Some(SslVersion::TLS1_3) => ossl::TLS1_3_VERSION,
                            Some(SslVersion::TLS1_2) => ossl::TLS1_2_VERSION,
                            _ => -1,
                        };
                        n.store(v, Ordering::SeqCst);
                        if mode == Mode::StallGreeting {
                            while !st.load(Ordering::SeqCst) {
                                std::thread::sleep(Duration::from_millis(10));
                            }
                        } else {
                            let _ = tls.write_all(b"200 example fixture ready\r\n");
                            let mut line = Vec::new();
                            while !st.load(Ordering::SeqCst) {
                                let mut byte = [0u8; 1];
                                if !matches!(tls.read(&mut byte), Ok(1)) {
                                    break;
                                }
                                line.push(byte[0]);
                                if line.len() > 4096 {
                                    break;
                                }
                                if byte[0] != b'\n' {
                                    continue;
                                }
                                let reply: &[u8] = if line.starts_with(b"AUTHINFO USER") {
                                    a.fetch_add(1, Ordering::SeqCst);
                                    b"381 password required\r\n"
                                } else if line.starts_with(b"AUTHINFO PASS") {
                                    a.fetch_add(1, Ordering::SeqCst);
                                    if mode == Mode::BadLogin {
                                        b"502 rejected\r\n"
                                    } else {
                                        b"281 accepted\r\n"
                                    }
                                } else {
                                    r.fetch_add(1, Ordering::SeqCst);
                                    b"500 no article commands permitted\r\n"
                                };
                                let _ = tls.write_all(reply);
                                line.clear();
                            }
                        }
                        let _ = tls.get_mut().shutdown(std::net::Shutdown::Both);
                    }
                    break;
                }
            });
            Self {
                port,
                negotiated,
                auth_commands,
                article_commands,
                stop,
                worker: Some(worker),
            }
        }
    }

    impl Drop for Loopback {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(w) = self.worker.take() {
                let _ = w.join();
            }
        }
    }

    fn now_unix() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    unsafe extern "C" fn never(_: *mut c_void) -> c_int {
        0
    }

    fn probe_fixture(
        server: &Loopback,
        suite: &str,
        protocol: c_int,
        milliseconds: u64,
        verification: u8,
        cancel: crate::ffi::tls_cipher_native::Cancel,
        cancel_context: *mut c_void,
        addresses: &str,
    ) -> c_int {
        let host = CString::new("localhost").unwrap();
        let user = CString::new("example").unwrap();
        let input = crate::net::NzbgetTlsServer {
            id: 1,
            host: host.as_ptr(),
            port: server.port,
            ip_version: 4,
            verification,
            username: user.as_ptr(),
            password: user.as_ptr(),
        };
        let suite = CString::new(suite).unwrap();
        let addresses = CString::new(addresses).unwrap();
        unsafe {
            backend::probe(
                ptr::null_mut(),
                (&input as *const crate::net::NzbgetTlsServer).cast(),
                suite.as_ptr(),
                protocol,
                addresses.as_ptr(),
                milliseconds,
                cancel,
                cancel_context,
            )
        }
    }

    fn probe_default(server: &Loopback, suite: &str, protocol: c_int) -> c_int {
        probe_fixture(
            server,
            suite,
            protocol,
            1000,
            0,
            never,
            ptr::null_mut(),
            "127.0.0.1",
        )
    }

    const LEGACY: &str = "ECDHE-RSA-AES128-GCM-SHA256";
    const T13_128: &str = "TLS_AES_128_GCM_SHA256";

    #[test]
    fn crypto_authenticates_every_advertised_eligible_algorithm() {
        unsafe extern "C" fn collect(
            context: *mut c_void,
            _: *const c_char,
            algorithm: *const c_char,
            protocol: c_int,
            bits: u32,
            flags: u32,
        ) -> c_int {
            if crate::ffi::nzbget_rs_tls_cipher_pin(protocol, bits, flags) != 0 {
                let set = unsafe { &mut *context.cast::<BTreeSet<CString>>() };
                set.insert(unsafe { CStr::from_ptr(algorithm) }.to_owned());
            }
            1
        }
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut algorithms: BTreeSet<CString> = BTreeSet::new();
        assert_eq!(
            unsafe {
                backend::suites(
                    ptr::null_mut(),
                    collect,
                    (&mut algorithms as *mut BTreeSet<CString>).cast(),
                )
            },
            1
        );
        assert!(!algorithms.is_empty());
        for algorithm in &algorithms {
            let crypto = unsafe { backend::prepare(ptr::null_mut(), algorithm.as_ptr()) };
            assert!(
                !crypto.is_null(),
                "Unmeasured advertised algorithm: {algorithm:?}"
            );
            unsafe {
                assert!(backend::record(crypto, 0) < 0);
                assert_eq!(backend::prepare_batch(crypto, 0, 2), 1);
                assert_eq!(backend::record(crypto, 0), 16384);
                assert_eq!(backend::record(crypto, 1), 16384);
                assert!(backend::record(crypto, 2) < 0);
                assert_eq!(backend::prepare_batch(crypto, 0, 65), 0);
                assert_eq!(backend::prepare_batch(crypto, u64::MAX, 1), 0);
                assert_eq!(backend::prepare_batch(crypto, 2, 2), 1);
                assert_eq!(backend::record(crypto, 2), 16384);
                backend::free_crypto(crypto);
            }
        }
    }

    #[test]
    fn tls12_pin_cannot_negotiate_tls13_defaults() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _s = IgnoreSigpipe::new();
        let f = Loopback::new(
            ossl::TLS1_2_VERSION,
            ossl::TLS1_3_VERSION,
            LEGACY,
            "TLS_AES_256_GCM_SHA384",
            Mode::Normal,
        );
        assert_eq!(probe_default(&f, LEGACY, 12), 1);
        assert_eq!(f.negotiated.load(Ordering::SeqCst), ossl::TLS1_2_VERSION);
        assert_eq!(f.auth_commands.load(Ordering::SeqCst), 2);
        assert_eq!(f.article_commands.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn tls13_pin_cannot_fall_back_to_tls12() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _s = IgnoreSigpipe::new();
        let f = Loopback::new(
            ossl::TLS1_2_VERSION,
            ossl::TLS1_2_VERSION,
            LEGACY,
            T13_128,
            Mode::Normal,
        );
        assert_eq!(probe_default(&f, T13_128, 13), 0);
        assert_eq!(f.auth_commands.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn authentication_failure_is_unknown_not_unsupported() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _s = IgnoreSigpipe::new();
        let f = Loopback::new(
            ossl::TLS1_3_VERSION,
            ossl::TLS1_3_VERSION,
            LEGACY,
            T13_128,
            Mode::BadLogin,
        );
        assert_eq!(probe_default(&f, T13_128, 13), -5);
    }

    #[test]
    fn greeting_deadline_bounds_the_complete_probe() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _s = IgnoreSigpipe::new();
        let f = Loopback::new(
            ossl::TLS1_3_VERSION,
            ossl::TLS1_3_VERSION,
            LEGACY,
            T13_128,
            Mode::StallGreeting,
        );
        assert_eq!(
            probe_fixture(&f, T13_128, 13, 100, 0, never, ptr::null_mut(), "127.0.0.1"),
            -3
        );
    }

    #[test]
    fn untrusted_certificate_is_unknown() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _s = IgnoreSigpipe::new();
        // The daemon only verifies against a loaded store; load the system one.
        let store = openssl_probe_store();
        init_options(store.as_bytes());
        let f = Loopback::new(
            ossl::TLS1_3_VERSION,
            ossl::TLS1_3_VERSION,
            LEGACY,
            T13_128,
            Mode::Normal,
        );
        let result = probe_fixture(
            &f,
            T13_128,
            13,
            1000,
            2,
            never,
            ptr::null_mut(),
            "127.0.0.1",
        );
        final_cleanup();
        assert_eq!(result, -4);
        assert_eq!(f.auth_commands.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn verification_without_a_store_is_a_certificate_failure() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        final_cleanup();
        let f = Loopback::new(
            ossl::TLS1_3_VERSION,
            ossl::TLS1_3_VERSION,
            LEGACY,
            T13_128,
            Mode::Normal,
        );
        assert_eq!(
            probe_fixture(
                &f,
                T13_128,
                13,
                1000,
                2,
                never,
                ptr::null_mut(),
                "127.0.0.1"
            ),
            -4
        );
    }

    #[test]
    fn cancellation_interrupts_greeting() {
        unsafe extern "C" fn negotiated(context: *mut c_void) -> c_int {
            c_int::from(unsafe { &*context.cast::<AtomicI32>() }.load(Ordering::SeqCst) != 0)
        }
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _s = IgnoreSigpipe::new();
        let f = Loopback::new(
            ossl::TLS1_3_VERSION,
            ossl::TLS1_3_VERSION,
            LEGACY,
            T13_128,
            Mode::StallGreeting,
        );
        // Cancellation becomes true once the fixture completed its handshake.
        let ctx = Arc::as_ptr(&f.negotiated) as *mut c_void;
        assert_eq!(
            probe_fixture(&f, T13_128, 13, 1000, 0, negotiated, ctx, "127.0.0.1"),
            -6
        );
        assert_eq!(f.auth_commands.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn probe_tries_second_resolved_address_of_same_family() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _s = IgnoreSigpipe::new();
        let f = Loopback::new(
            ossl::TLS1_3_VERSION,
            ossl::TLS1_3_VERSION,
            LEGACY,
            T13_128,
            Mode::Normal,
        );
        // The first address must fail at once on every platform. macOS drops packets
        // for 127.0.0.2 rather than refusing them, which would spend the whole deadline.
        assert_eq!(
            probe_fixture(
                &f,
                T13_128,
                13,
                1000,
                0,
                never,
                ptr::null_mut(),
                "255.255.255.255\n127.0.0.1"
            ),
            1
        );
        assert_eq!(f.negotiated.load(Ordering::SeqCst), ossl::TLS1_3_VERSION);
    }

    #[test]
    fn empty_resolved_addresses_cannot_fall_back_to_blocking_dns() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let f = Loopback::new(
            ossl::TLS1_3_VERSION,
            ossl::TLS1_3_VERSION,
            LEGACY,
            T13_128,
            Mode::Normal,
        );
        assert_eq!(
            probe_fixture(&f, T13_128, 13, 1000, 0, never, ptr::null_mut(), ""),
            -2
        );
        assert_eq!(f.negotiated.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn deadline_reports_stop_and_cancellation() {
        unsafe extern "C" fn yes(_: *mut c_void) -> c_int {
            1
        }
        assert!(!Deadline::new(Duration::from_secs(5), None).stopped());
        assert!(Deadline::new(Duration::ZERO, None).stopped());
        assert!(Deadline::new(Duration::from_secs(5), Some((yes, ptr::null_mut()))).stopped());
    }

    /// A CA file that does not contain the fixture's self-signed certificate.
    fn openssl_probe_store() -> String {
        for path in [
            "/etc/ssl/certs/ca-certificates.crt",
            "/etc/ssl/cert.pem",
            "/opt/homebrew/etc/openssl@3/cert.pem",
            "/usr/local/etc/openssl@3/cert.pem",
        ] {
            if std::path::Path::new(path).exists() {
                return path.to_owned();
            }
        }
        // An empty PEM file still creates a store that trusts nothing.
        let path = std::env::temp_dir().join("rustyget-empty-ca.pem");
        std::fs::write(&path, b"").unwrap();
        path.to_string_lossy().into_owned()
    }
}
