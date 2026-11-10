//! POSIX socket and stream connection operations (`daemon/connect/Connection.cpp`).
//!
//! Replaces `SetCloseOnExec`, `closesocket_gracefully`, and the non-TLS socket
//! lifecycle and buffered I/O methods of `Connection` (`Bind`, `DoConnect`,
//! `ConnectWithTimeout`, `InitSocketOpts`, `DoDisconnect`, `Accept`,
//! `GetRemoteAddr`, `Cancel`, `WriteLine`, `Send`, `ReadLine`, `Recv`, and
//! `ReportError`).

use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::artwritercore::join_c_slices;
use crate::fsops::{error_message, last_errno, set_last_errno};

pub const INVALID_SOCKET: c_int = -1;

pub const CS_CONNECTED: c_int = 0;
pub const CS_DISCONNECTED: c_int = 1;
pub const CS_LISTENING: c_int = 2;
pub const CS_CANCELLED: c_int = 3;
pub const CS_BROKEN: c_int = 4;

pub const CONNECTION_READBUFFER_SIZE: usize = 1024;
const BSTRING_MAX: usize = 1023;
const REMOTE_ADDR_CAP: usize = 99; // BString<100>::Capacity()

/// Sets `FD_CLOEXEC` on `fd` if valid (`SetCloseOnExec`).
pub fn set_close_on_exec(fd: c_int) {
    if fd != INVALID_SOCKET {
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFD, 0);
            if flags >= 0 {
                libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
            }
        }
    }
}

unsafe fn socket_cloexec(domain: c_int, ty: c_int, protocol: c_int) -> c_int {
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    {
        libc::socket(domain, ty | libc::SOCK_CLOEXEC, protocol)
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
    )))]
    {
        let fd = libc::socket(domain, ty, protocol);
        set_close_on_exec(fd);
        fd
    }
}

/// Gracefully shuts down, drains, and closes `fd` (`closesocket_gracefully`).
pub fn closesocket_gracefully(fd: c_int) {
    if fd == INVALID_SOCKET {
        return;
    }
    unsafe {
        let linger = libc::linger {
            l_onoff: 1,
            l_linger: 1,
        };
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_LINGER,
            (&linger as *const libc::linger).cast(),
            std::mem::size_of::<libc::linger>() as libc::socklen_t,
        );
        libc::shutdown(fd, libc::SHUT_WR);
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
        let mut buf = [0u8; 1024];
        loop {
            let n = libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0);
            if n <= 0 {
                break;
            }
        }
        libc::close(fd);
    }
}

fn truncate_bstring(mut v: Vec<u8>) -> Vec<u8> {
    let nul = v.iter().position(|&b| b == 0).unwrap_or(v.len());
    v.truncate(nul.min(BSTRING_MAX));
    v
}

/// Formats `Connection::ReportError`, preserving raw non-UTF8 bytes and
/// `BString<1024>` truncation semantics. Returns `None` when `tls_error`
/// suppressed duplicate emission (clearing `*tls_error`).
pub fn format_report_error(
    tls_error: &mut bool,
    msg_prefix: &[u8],
    msg_arg: Option<&[u8]>,
    print_err_code: bool,
    err_code: c_int,
    err_msg: Option<&[u8]>,
) -> Option<Vec<u8>> {
    if *tls_error {
        *tls_error = false;
        return None;
    }

    let arg = msg_arg.unwrap_or(b"(null)");
    let prefix_raw = if let Some(pos) = msg_prefix.windows(2).position(|w| w == b"%s") {
        join_c_slices(&[&msg_prefix[..pos], arg, &msg_prefix[pos + 2..]])
    } else {
        msg_prefix.to_vec()
    };
    let err_prefix = truncate_bstring(prefix_raw);

    if !print_err_code {
        return Some(err_prefix);
    }

    let code = if err_code == 0 {
        last_errno()
    } else {
        err_code
    };
    let print_err_msg = match err_msg {
        Some(m) => truncate_bstring(m.to_vec()),
        None => error_message(code),
    };
    let code_str = code.to_string();
    let full = join_c_slices(&[
        &err_prefix,
        b": Error ",
        code_str.as_bytes(),
        b" - ",
        &print_err_msg,
    ]);
    Some(truncate_bstring(full))
}

fn emit_error<F>(
    on_error: &mut F,
    tls_error: &mut bool,
    msg_prefix: &[u8],
    msg_arg: Option<&[u8]>,
    print_err_code: bool,
    err_code: c_int,
    err_msg: Option<&[u8]>,
) where
    F: FnMut(&[u8]),
{
    if let Some(formatted) = format_report_error(
        tls_error,
        msg_prefix,
        msg_arg,
        print_err_code,
        err_code,
        err_msg,
    ) {
        on_error(&formatted);
    }
}

/// Applies `SO_RCVTIMEO` and `SO_SNDTIMEO` (`Connection::InitSocketOpts`).
pub fn init_socket_opts(fd: c_int, timeout_sec: c_int) -> Result<(), c_int> {
    let tv = libc::timeval {
        tv_sec: timeout_sec as _,
        tv_usec: 0,
    };
    unsafe {
        let optptr: *const libc::c_void = (&tv as *const libc::timeval).cast();
        let optlen = std::mem::size_of::<libc::timeval>() as libc::socklen_t;
        if libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_RCVTIMEO, optptr, optlen) != 0 {
            return Err(last_errno());
        }
        if libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_SNDTIMEO, optptr, optlen) != 0 {
            return Err(last_errno());
        }
    }
    Ok(())
}

fn ip_version_family(ip_version: c_int) -> c_int {
    match ip_version {
        4 => libc::AF_INET,
        6 => libc::AF_INET6,
        _ => libc::AF_UNSPEC,
    }
}

fn to_c_bytes(bytes: &[u8]) -> CString {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    // Safety: truncated at first NUL byte.
    unsafe { CString::from_vec_unchecked(bytes[..end].to_vec()) }
}

/// Binds and listens on a Unix domain path or TCP `(host, port)` (`Connection::Bind`).
#[allow(clippy::too_many_arguments)]
pub fn bind_listener<E>(
    host: &[u8],
    port: c_int,
    ip_version: c_int,
    socket_slot: &AtomicI32,
    status_slot: &AtomicI32,
    is_listener: &mut bool,
    tls_error: &mut bool,
    mut on_error: E,
) -> bool
where
    E: FnMut(&[u8]),
{
    if status_slot.load(Ordering::SeqCst) == CS_LISTENING {
        return true;
    }

    let mut errcode: c_int = 0;

    if !host.is_empty() && host[0] == b'/' {
        let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        if host.len() >= addr.sun_path.len() {
            emit_error(
                &mut on_error,
                tls_error,
                b"Binding socket failed for %s: name too long\n",
                Some(host),
                false,
                0,
                None,
            );
            return false;
        }
        addr.sun_family = libc::AF_UNIX as _;
        unsafe {
            ptr::copy_nonoverlapping(
                host.as_ptr().cast::<c_char>(),
                addr.sun_path.as_mut_ptr(),
                host.len(),
            );
            let sock = socket_cloexec(libc::PF_UNIX, libc::SOCK_STREAM, 0);
            socket_slot.store(sock, Ordering::SeqCst);
            if sock == INVALID_SOCKET {
                let err = last_errno();
                emit_error(
                    &mut on_error,
                    tls_error,
                    b"Socket creation failed for %s",
                    Some(host),
                    true,
                    err,
                    None,
                );
                return false;
            }
            let host_c = to_c_bytes(host);
            libc::unlink(host_c.as_ptr());
            if libc::bind(
                sock,
                (&addr as *const libc::sockaddr_un).cast(),
                std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t,
            ) == -1
            {
                errcode = last_errno();
                libc::close(sock);
                socket_slot.store(INVALID_SOCKET, Ordering::SeqCst);
            }
        }
    } else {
        unsafe {
            let mut hints: libc::addrinfo = std::mem::zeroed();
            hints.ai_family = ip_version_family(ip_version);
            hints.ai_socktype = libc::SOCK_STREAM;
            hints.ai_flags = libc::AI_PASSIVE;

            let host_c = to_c_bytes(host);
            let port_c = CString::new(port.to_string()).unwrap();
            let mut addr_list: *mut libc::addrinfo = ptr::null_mut();
            let res = libc::getaddrinfo(host_c.as_ptr(), port_c.as_ptr(), &hints, &mut addr_list);
            if res != 0 {
                let sys_err = last_errno();
                let gai_err = if res != libc::EAI_SYSTEM { res } else { 0 };
                let gai_msg = if gai_err != 0 {
                    let s = libc::gai_strerror(res);
                    if s.is_null() {
                        None
                    } else {
                        Some(CStr::from_ptr(s).to_bytes())
                    }
                } else {
                    None
                };
                emit_error(
                    &mut on_error,
                    tls_error,
                    b"Could not resolve hostname %s",
                    Some(host),
                    true,
                    if gai_err != 0 { gai_err } else { sys_err },
                    gai_msg,
                );
                return false;
            }

            socket_slot.store(INVALID_SOCKET, Ordering::SeqCst);
            let mut cur = addr_list;
            while !cur.is_null() {
                let ai = &*cur;
                let sock = socket_cloexec(ai.ai_family, ai.ai_socktype, ai.ai_protocol);
                socket_slot.store(sock, Ordering::SeqCst);
                if sock != INVALID_SOCKET {
                    let opt: c_int = 1;
                    libc::setsockopt(
                        sock,
                        libc::SOL_SOCKET,
                        libc::SO_REUSEADDR,
                        (&opt as *const c_int).cast(),
                        std::mem::size_of::<c_int>() as libc::socklen_t,
                    );
                    if libc::bind(sock, ai.ai_addr, ai.ai_addrlen) != -1 {
                        break;
                    }
                    errcode = last_errno();
                    libc::close(sock);
                    socket_slot.store(INVALID_SOCKET, Ordering::SeqCst);
                }
                cur = ai.ai_next;
            }
            if !addr_list.is_null() {
                libc::freeaddrinfo(addr_list);
            }
        }
    }

    let sock = socket_slot.load(Ordering::SeqCst);
    if sock == INVALID_SOCKET {
        emit_error(
            &mut on_error,
            tls_error,
            b"Binding socket failed for %s",
            Some(host),
            true,
            errcode,
            None,
        );
        return false;
    }

    unsafe {
        if libc::listen(sock, 100) < 0 {
            let err = last_errno();
            emit_error(
                &mut on_error,
                tls_error,
                b"Listen on socket failed for %s",
                Some(host),
                true,
                err,
                None,
            );
            libc::close(sock);
            socket_slot.store(INVALID_SOCKET, Ordering::SeqCst);
            return false;
        }
    }

    *is_listener = true;
    status_slot.store(CS_LISTENING, Ordering::SeqCst);
    true
}

struct ResolvedAddr {
    family: c_int,
    socktype: c_int,
    protocol: c_int,
    storage: libc::sockaddr_storage,
    addrlen: libc::socklen_t,
}

unsafe fn collect_addrinfo(list: *mut libc::addrinfo, out: &mut Vec<ResolvedAddr>) {
    let mut cur = list;
    while !cur.is_null() {
        let ai = &*cur;
        if !ai.ai_addr.is_null()
            && (ai.ai_addrlen as usize) <= std::mem::size_of::<libc::sockaddr_storage>()
        {
            let mut storage: libc::sockaddr_storage = std::mem::zeroed();
            ptr::copy_nonoverlapping(
                ai.ai_addr.cast::<u8>(),
                (&mut storage as *mut libc::sockaddr_storage).cast::<u8>(),
                ai.ai_addrlen as usize,
            );
            out.push(ResolvedAddr {
                family: ai.ai_family,
                socktype: ai.ai_socktype,
                protocol: ai.ai_protocol,
                storage,
                addrlen: ai.ai_addrlen,
            });
        }
        cur = ai.ai_next;
    }
}

unsafe fn connect_with_timeout<W, S>(
    sock: c_int,
    addr: *const libc::sockaddr,
    addr_len: libc::socklen_t,
    timeout_sec: c_int,
    has_deadline: bool,
    status_slot: &AtomicI32,
    deadline_wait: &mut W,
    deadline_stopped: &mut S,
) -> bool
where
    W: FnMut(c_int, bool, bool) -> bool,
    S: FnMut() -> bool,
{
    let flags = libc::fcntl(sock, libc::F_GETFL, 0);
    if flags < 0 {
        return false;
    }
    if libc::fcntl(sock, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
        return false;
    }

    let mut ret = libc::connect(sock, addr, addr_len);
    if ret < 0 {
        let err = last_errno();
        if err != libc::EINPROGRESS {
            return false;
        }
    }

    if ret != 0 {
        let mut pfd = libc::pollfd {
            fd: sock,
            events: libc::POLLIN | libc::POLLOUT,
            revents: 0,
        };
        if has_deadline {
            ret = if deadline_wait(sock, true, true) {
                1
            } else {
                0
            };
            pfd.revents = libc::POLLIN | libc::POLLOUT;
        } else {
            let start = std::time::Instant::now();
            let limit_ms: Option<i64> = if timeout_sec > 0 {
                Some(i64::from(timeout_sec).saturating_mul(1000))
            } else {
                None
            };
            ret = loop {
                if status_slot.load(Ordering::SeqCst) == CS_CANCELLED || deadline_stopped() {
                    set_last_errno(libc::ECANCELED);
                    return false;
                }
                let wait_ms = match limit_ms {
                    Some(total) => {
                        let elapsed = start.elapsed().as_millis() as i64;
                        let rem = total - elapsed;
                        if rem <= 0 {
                            break 0;
                        }
                        rem.clamp(1, 50) as c_int
                    }
                    None => 50,
                };
                pfd.revents = 0;
                let r = libc::poll(&mut pfd, 1, wait_ms);
                if r < 0 {
                    if last_errno() == libc::EINTR {
                        continue;
                    }
                    break r;
                }
                if r > 0 {
                    break r;
                }
            };
        }

        if ret < 0 {
            return false;
        }
        if ret == 0 {
            set_last_errno(libc::ETIMEDOUT);
            return false;
        }
        if (pfd.revents & (libc::POLLIN | libc::POLLOUT | libc::POLLERR | libc::POLLHUP)) == 0 {
            return false;
        }

        let mut error: c_int = 0;
        let mut len = std::mem::size_of::<c_int>() as libc::socklen_t;
        if libc::getsockopt(
            sock,
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            (&mut error as *mut c_int).cast(),
            &mut len,
        ) < 0
        {
            return false;
        }
        if error != 0 {
            set_last_errno(error);
            return false;
        }
    }

    if status_slot.load(Ordering::SeqCst) == CS_CANCELLED || deadline_stopped() {
        set_last_errno(libc::ECANCELED);
        return false;
    }
    if has_deadline {
        return true;
    }
    if libc::fcntl(sock, libc::F_SETFL, flags) < 0 {
        return false;
    }
    true
}

/// Connects a Unix domain or TCP client socket and applies socket timeouts
/// (`Connection::DoConnect` prior to optional `StartTls`).
#[allow(clippy::too_many_arguments)]
pub fn connect_stream<W, S, E, D>(
    host: &[u8],
    port: c_int,
    ip_version: c_int,
    timeout_sec: c_int,
    resolved_addresses: &[&[u8]],
    has_deadline: bool,
    mut deadline_wait: W,
    mut deadline_stopped: S,
    socket_slot: &AtomicI32,
    status_slot: &AtomicI32,
    tls_error: &mut bool,
    mut on_error: E,
    mut on_debug: D,
) -> bool
where
    W: FnMut(c_int, bool, bool) -> bool,
    S: FnMut() -> bool,
    E: FnMut(&[u8]),
    D: FnMut(&[u8]),
{
    if status_slot.load(Ordering::SeqCst) == CS_CANCELLED || deadline_stopped() {
        set_last_errno(libc::ECANCELED);
        return false;
    }

    socket_slot.store(INVALID_SOCKET, Ordering::SeqCst);

    if !host.is_empty() && host[0] == b'/' {
        let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        if host.len() >= addr.sun_path.len() {
            emit_error(
                &mut on_error,
                tls_error,
                b"Connection to %s failed: name too long\n",
                Some(host),
                false,
                0,
                None,
            );
            return false;
        }
        addr.sun_family = libc::AF_UNIX as _;
        unsafe {
            ptr::copy_nonoverlapping(
                host.as_ptr().cast::<c_char>(),
                addr.sun_path.as_mut_ptr(),
                host.len(),
            );
            let sock = socket_cloexec(libc::PF_UNIX, libc::SOCK_STREAM, 0);
            socket_slot.store(sock, Ordering::SeqCst);
            if sock == INVALID_SOCKET {
                let err = last_errno();
                emit_error(
                    &mut on_error,
                    tls_error,
                    b"Socket creation failed for %s",
                    Some(host),
                    true,
                    err,
                    None,
                );
                return false;
            }
            if !connect_with_timeout(
                sock,
                (&addr as *const libc::sockaddr_un).cast(),
                std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t,
                timeout_sec,
                has_deadline,
                status_slot,
                &mut deadline_wait,
                &mut deadline_stopped,
            ) {
                let err = last_errno();
                emit_error(
                    &mut on_error,
                    tls_error,
                    b"Connection to %s failed",
                    Some(host),
                    true,
                    err,
                    None,
                );
                let cur = socket_slot.swap(INVALID_SOCKET, Ordering::SeqCst);
                if cur != INVALID_SOCKET {
                    libc::close(cur);
                }
                return false;
            }
        }
    } else {
        unsafe {
            let mut hints: libc::addrinfo = std::mem::zeroed();
            hints.ai_family = ip_version_family(ip_version);
            hints.ai_socktype = libc::SOCK_STREAM;

            let port_c = CString::new(port.to_string()).unwrap();
            let mut items = Vec::new();
            let res: c_int;
            let mut sys_err: c_int = 0;

            if !resolved_addresses.is_empty() {
                hints.ai_flags |= libc::AI_NUMERICHOST;
                for &numeric in resolved_addresses {
                    let num_c = to_c_bytes(numeric);
                    let mut list: *mut libc::addrinfo = ptr::null_mut();
                    if libc::getaddrinfo(num_c.as_ptr(), port_c.as_ptr(), &hints, &mut list) == 0
                        && !list.is_null()
                    {
                        collect_addrinfo(list, &mut items);
                        libc::freeaddrinfo(list);
                    }
                }
                res = if items.is_empty() {
                    libc::EAI_NONAME
                } else {
                    0
                };
            } else if has_deadline {
                // Probe DNS must never fall back to blocking resolution.
                return false;
            } else {
                let host_c = to_c_bytes(host);
                let mut list: *mut libc::addrinfo = ptr::null_mut();
                res = libc::getaddrinfo(host_c.as_ptr(), port_c.as_ptr(), &hints, &mut list);
                if res != 0 {
                    sys_err = last_errno();
                } else if !list.is_null() {
                    collect_addrinfo(list, &mut items);
                    libc::freeaddrinfo(list);
                }
            }

            let res_str = res.to_string();
            on_debug(&join_c_slices(&[
                b"getaddrinfo for ",
                host,
                b": ",
                res_str.as_bytes(),
            ]));

            if res != 0 {
                let gai_err = if res != libc::EAI_SYSTEM { res } else { 0 };
                let gai_msg = if gai_err != 0 {
                    let s = libc::gai_strerror(res);
                    if s.is_null() {
                        None
                    } else {
                        Some(CStr::from_ptr(s).to_bytes())
                    }
                } else {
                    None
                };
                emit_error(
                    &mut on_error,
                    tls_error,
                    b"Could not resolve hostname %s",
                    Some(host),
                    true,
                    if gai_err != 0 { gai_err } else { sys_err },
                    gai_msg,
                );
                return false;
            }

            let mut tried: Vec<(c_int, c_int, c_int)> = Vec::new();
            let mut connected = false;
            let mut last_err: c_int = 0;

            for item in &items {
                if status_slot.load(Ordering::SeqCst) == CS_CANCELLED || deadline_stopped() {
                    set_last_errno(libc::ECANCELED);
                    last_err = libc::ECANCELED;
                    break;
                }
                let key = (item.family, item.socktype, item.protocol);
                if !has_deadline && tried.contains(&key) {
                    continue;
                }
                tried.push(key);

                let prev = socket_slot.swap(INVALID_SOCKET, Ordering::SeqCst);
                if prev != INVALID_SOCKET {
                    libc::close(prev);
                }

                let sock = socket_cloexec(item.family, item.socktype, item.protocol);
                socket_slot.store(sock, Ordering::SeqCst);
                if sock == INVALID_SOCKET {
                    last_err = last_errno();
                    continue;
                }

                if connect_with_timeout(
                    sock,
                    (&item.storage as *const libc::sockaddr_storage).cast(),
                    item.addrlen,
                    timeout_sec,
                    has_deadline,
                    status_slot,
                    &mut deadline_wait,
                    &mut deadline_stopped,
                ) {
                    connected = true;
                    break;
                }
                last_err = last_errno();
            }

            let final_sock = socket_slot.load(Ordering::SeqCst);
            if final_sock == INVALID_SOCKET && !items.is_empty() {
                emit_error(
                    &mut on_error,
                    tls_error,
                    b"Socket creation failed for %s",
                    Some(host),
                    true,
                    last_err,
                    None,
                );
            }

            if !connected && final_sock != INVALID_SOCKET {
                emit_error(
                    &mut on_error,
                    tls_error,
                    b"Connection to %s failed",
                    Some(host),
                    true,
                    last_err,
                    None,
                );
                let cur = socket_slot.swap(INVALID_SOCKET, Ordering::SeqCst);
                if cur != INVALID_SOCKET {
                    libc::close(cur);
                }
            }

            if socket_slot.load(Ordering::SeqCst) == INVALID_SOCKET {
                return false;
            }
        }
    }

    let sock = socket_slot.load(Ordering::SeqCst);
    if let Err(err) = init_socket_opts(sock, timeout_sec) {
        emit_error(
            &mut on_error,
            tls_error,
            b"Socket initialization failed for %s",
            Some(host),
            true,
            err,
            None,
        );
        return false;
    }

    true
}

/// Closes the underlying socket and unlinks Unix listener paths (`Connection::DoDisconnect`
/// after `CloseTls()`).
pub fn disconnect_socket(
    host: &[u8],
    graceful: bool,
    is_listener: &mut bool,
    socket_slot: &AtomicI32,
    status_slot: &AtomicI32,
) {
    let was_listener = *is_listener;
    *is_listener = false;
    let sock = socket_slot.swap(INVALID_SOCKET, Ordering::SeqCst);
    if sock != INVALID_SOCKET {
        unsafe {
            if !host.is_empty() && host[0] == b'/' {
                let mut is_listening: c_int = 0;
                let mut len = std::mem::size_of::<c_int>() as libc::socklen_t;
                let sock_listening = libc::getsockopt(
                    sock,
                    libc::SOL_SOCKET,
                    libc::SO_ACCEPTCONN,
                    (&mut is_listening as *mut c_int).cast(),
                    &mut len,
                ) == 0
                    && is_listening != 0;
                if was_listener || sock_listening {
                    let host_c = to_c_bytes(host);
                    libc::unlink(host_c.as_ptr());
                }
            }
            if graceful {
                closesocket_gracefully(sock);
            } else {
                libc::close(sock);
            }
        }
    }
    status_slot.store(CS_DISCONNECTED, Ordering::SeqCst);
}

fn is_transient_accept_err(err: c_int) -> bool {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if err == libc::ENONET {
        return true;
    }
    matches!(
        err,
        libc::EINTR
            | libc::ECONNABORTED
            | libc::EPROTO
            | libc::ENETDOWN
            | libc::ENOPROTOOPT
            | libc::EHOSTDOWN
            | libc::EHOSTUNREACH
            | libc::EOPNOTSUPP
            | libc::ENETUNREACH
    )
}

/// Accepts an incoming client socket from a listening socket (`Connection::Accept`).
/// Returns `INVALID_SOCKET` (`-1`) if not listening, cancelled, stopped, or failed.
pub fn accept_connection<S, E>(
    host: &[u8],
    timeout_sec: c_int,
    socket_slot: &AtomicI32,
    status_slot: &AtomicI32,
    accept_busy: &mut bool,
    tls_error: &mut bool,
    mut should_stop: S,
    mut on_error: E,
) -> c_int
where
    S: FnMut() -> bool,
    E: FnMut(&[u8]),
{
    if status_slot.load(Ordering::SeqCst) != CS_LISTENING || should_stop() {
        return INVALID_SOCKET;
    }

    let initial_fd = socket_slot.load(Ordering::SeqCst);
    if initial_fd != INVALID_SOCKET {
        unsafe {
            let fl = libc::fcntl(initial_fd, libc::F_GETFL, 0);
            if fl >= 0 && (fl & libc::O_NONBLOCK) == 0 {
                libc::fcntl(initial_fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
            }
        }
    }

    *accept_busy = false;
    let mut err: c_int = 0;
    let mut stopped_cleanly = false;
    let accepted = loop {
        if status_slot.load(Ordering::SeqCst) != CS_LISTENING || should_stop() {
            stopped_cleanly = true;
            break INVALID_SOCKET;
        }
        let listen_fd = socket_slot.load(Ordering::SeqCst);
        if listen_fd == INVALID_SOCKET {
            stopped_cleanly = status_slot.load(Ordering::SeqCst) != CS_LISTENING || should_stop();
            err = libc::EBADF;
            break INVALID_SOCKET;
        }

        let mut pfd = libc::pollfd {
            fd: listen_fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let poll_res = unsafe { libc::poll(&mut pfd, 1, 50) };
        if poll_res == 0 {
            continue;
        }
        if poll_res < 0 {
            if status_slot.load(Ordering::SeqCst) != CS_LISTENING || should_stop() {
                stopped_cleanly = true;
                break INVALID_SOCKET;
            }
            err = last_errno();
            if is_transient_accept_err(err) {
                continue;
            }
            break INVALID_SOCKET;
        }

        let fd = unsafe {
            #[cfg(target_os = "linux")]
            {
                libc::accept4(
                    listen_fd,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    libc::SOCK_CLOEXEC,
                )
            }
            #[cfg(not(target_os = "linux"))]
            {
                let s = libc::accept(listen_fd, ptr::null_mut(), ptr::null_mut());
                set_close_on_exec(s);
                s
            }
        };
        if fd >= 0 {
            unsafe {
                let fl = libc::fcntl(fd, libc::F_GETFL, 0);
                if fl >= 0 && (fl & libc::O_NONBLOCK) != 0 {
                    libc::fcntl(fd, libc::F_SETFL, fl & !libc::O_NONBLOCK);
                }
            }
            break fd;
        }
        if status_slot.load(Ordering::SeqCst) != CS_LISTENING || should_stop() {
            stopped_cleanly = true;
            break INVALID_SOCKET;
        }
        err = last_errno();
        if err == libc::EAGAIN || err == libc::EWOULDBLOCK || is_transient_accept_err(err) {
            continue;
        }
        *accept_busy = matches!(
            err,
            libc::EMFILE | libc::ENFILE | libc::ENOBUFS | libc::ENOMEM
        );
        break INVALID_SOCKET;
    };

    if accepted == INVALID_SOCKET
        && !stopped_cleanly
        && status_slot.load(Ordering::SeqCst) != CS_CANCELLED
    {
        emit_error(
            &mut on_error,
            tls_error,
            b"Could not accept connection for %s",
            Some(host),
            true,
            err,
            None,
        );
    }
    if accepted == INVALID_SOCKET {
        return INVALID_SOCKET;
    }

    if let Err(opt_err) = init_socket_opts(accepted, timeout_sec) {
        emit_error(
            &mut on_error,
            tls_error,
            b"Socket initialization failed for %s",
            Some(host),
            true,
            opt_err,
            None,
        );
    }

    accepted
}

/// Resolves the peer IP string for `fd` (`Connection::GetRemoteAddr`).
pub fn remote_addr(host: &[u8], fd: c_int) -> Vec<u8> {
    if !host.is_empty() && host[0] == b'/' {
        return b"-".to_vec();
    }
    if fd == INVALID_SOCKET {
        return Vec::new();
    }
    extern "C" {
        fn inet_ntop(
            af: c_int,
            src: *const libc::c_void,
            dst: *mut c_char,
            size: libc::socklen_t,
        ) -> *const c_char;
    }
    unsafe {
        let mut peer: libc::sockaddr_storage = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if libc::getpeername(
            fd,
            (&mut peer as *mut libc::sockaddr_storage).cast(),
            &mut len,
        ) < 0
        {
            return Vec::new();
        }
        let family = peer.ss_family as c_int;
        let mut buf = [0u8; REMOTE_ADDR_CAP + 1];
        let src: *const libc::c_void = if family == libc::AF_INET6 {
            let sin6 = &*(&peer as *const libc::sockaddr_storage).cast::<libc::sockaddr_in6>();
            (&sin6.sin6_addr as *const libc::in6_addr).cast()
        } else {
            let sin = &*(&peer as *const libc::sockaddr_storage).cast::<libc::sockaddr_in>();
            (&sin.sin_addr as *const libc::in_addr).cast()
        };
        let res = inet_ntop(
            family,
            src,
            buf.as_mut_ptr().cast(),
            REMOTE_ADDR_CAP as libc::socklen_t,
        );
        if res.is_null() {
            return Vec::new();
        }
        buf[REMOTE_ADDR_CAP] = 0;
        CStr::from_bytes_until_nul(&buf)
            .map(|c| c.to_bytes().to_vec())
            .unwrap_or_default()
    }
}

/// Cancels an active or connecting socket (`Connection::Cancel`).
pub fn cancel_socket(force_close: bool, socket_slot: &AtomicI32, status_slot: &AtomicI32) {
    let cur = socket_slot.load(Ordering::SeqCst);
    if cur != INVALID_SOCKET {
        status_slot.store(CS_CANCELLED, Ordering::SeqCst);
        unsafe {
            if force_close {
                let s = socket_slot.swap(INVALID_SOCKET, Ordering::SeqCst);
                if s != INVALID_SOCKET {
                    libc::shutdown(s, libc::SHUT_RDWR);
                    libc::close(s);
                }
            } else {
                libc::shutdown(cur, libc::SHUT_RDWR);
            }
        }
    }
}

/// Sends a single line buffer via `send_chunk` (`Connection::WriteLine`).
pub fn write_line<F>(status_slot: &AtomicI32, line: &[u8], mut send_chunk: F) -> c_int
where
    F: FnMut(&[u8]) -> c_int,
{
    if status_slot.load(Ordering::SeqCst) != CS_CONNECTED {
        return -1;
    }
    let res = send_chunk(line);
    if res <= 0 {
        status_slot.store(CS_BROKEN, Ordering::SeqCst);
    }
    res
}

/// Sends the entire slice via `send_chunk` (`Connection::Send`).
pub fn send_all<F>(status_slot: &AtomicI32, data: &[u8], mut send_chunk: F) -> bool
where
    F: FnMut(&[u8]) -> c_int,
{
    if status_slot.load(Ordering::SeqCst) != CS_CONNECTED {
        return false;
    }
    let mut sent = 0usize;
    while sent < data.len() {
        let res = send_chunk(&data[sent..]);
        if res <= 0 {
            status_slot.store(CS_BROKEN, Ordering::SeqCst);
            return false;
        }
        sent += res as usize;
    }
    true
}

/// Buffered line reader up to `\n` or `dst.len() - 1` (`Connection::ReadLine`).
/// Returns `Some(bytes_read)` when at least one byte was placed in `dst`
/// (with trailing NUL written at `dst[bytes_read]`), or `None` on EOF/error/disconnected.
#[allow(clippy::too_many_arguments)]
pub fn read_line<R, E>(
    host: &[u8],
    status_slot: &AtomicI32,
    read_buf: &mut [u8],
    buf_offset: &mut usize,
    buf_avail: &mut usize,
    total_bytes_read: &mut i32,
    dst: &mut [u8],
    tls_error: &mut bool,
    mut recv_chunk: R,
    mut on_error: E,
) -> Option<usize>
where
    R: FnMut(&mut [u8], &mut bool) -> c_int,
    E: FnMut(&[u8]),
{
    if status_slot.load(Ordering::SeqCst) != CS_CONNECTED || dst.is_empty() {
        return None;
    }

    let max_copy = dst.len() - 1;
    let mut bytes_read = 0usize;
    let recv_cap = read_buf
        .len()
        .saturating_sub(1)
        .min(CONNECTION_READBUFFER_SIZE);

    while bytes_read < max_copy {
        if *buf_avail == 0 {
            let n = recv_chunk(&mut read_buf[..recv_cap], tls_error);
            if n < 0 {
                let err = last_errno();
                emit_error(
                    &mut on_error,
                    tls_error,
                    b"Could not receive data on socket from %s",
                    Some(host),
                    true,
                    err,
                    None,
                );
                status_slot.store(CS_BROKEN, Ordering::SeqCst);
                *buf_avail = 0;
                break;
            } else if n == 0 {
                break;
            } else {
                *total_bytes_read = total_bytes_read.wrapping_add(n);
                *buf_offset = 0;
                *buf_avail = n as usize;
                read_buf[*buf_avail] = 0;
            }
        }

        let avail_slice = &read_buf[*buf_offset..*buf_offset + *buf_avail];
        let nl = avail_slice.iter().position(|&b| b == b'\n');
        let chunk_len = nl
            .map_or(*buf_avail, |idx| idx + 1)
            .min(max_copy - bytes_read);

        dst[bytes_read..bytes_read + chunk_len].copy_from_slice(&avail_slice[..chunk_len]);
        bytes_read += chunk_len;
        *buf_offset += chunk_len;
        *buf_avail -= chunk_len;

        if nl.is_some() {
            break;
        }
    }

    dst[bytes_read] = 0;
    if bytes_read == 0 {
        None
    } else {
        Some(bytes_read)
    }
}

/// Reads an exact buffer, draining buffered `ReadLine` bytes first (`Connection::Recv`).
#[allow(clippy::too_many_arguments)]
pub fn recv_exact<R, E>(
    host: &[u8],
    read_buf: &[u8],
    buf_offset: &mut usize,
    buf_avail: &mut usize,
    total_bytes_read: &mut i32,
    dst: &mut [u8],
    tls_error: &mut bool,
    mut recv_chunk: R,
    mut on_error: E,
) -> bool
where
    R: FnMut(&mut [u8], &mut bool) -> c_int,
    E: FnMut(&[u8]),
{
    dst.fill(0);
    let mut pos = 0usize;

    if *buf_avail > 0 {
        let take = dst.len().min(*buf_avail);
        dst[..take].copy_from_slice(&read_buf[*buf_offset..*buf_offset + take]);
        pos += take;
        *buf_offset += take;
        *buf_avail -= take;
    }

    while pos < dst.len() {
        let received = recv_chunk(&mut dst[pos..], tls_error);
        if received <= 0 {
            let err = last_errno();
            emit_error(
                &mut on_error,
                tls_error,
                b"Could not receive data on socket from %s",
                Some(host),
                true,
                err,
                None,
            );
            return false;
        }
        pos += received as usize;
        *total_bytes_read = total_bytes_read.wrapping_add(received);
    }

    true
}

/// Receives up to `dst.len()` bytes once (`Connection::TryRecv`).
pub fn try_recv<R, E>(
    host: &[u8],
    total_bytes_read: &mut i32,
    dst: &mut [u8],
    tls_error: &mut bool,
    mut recv_chunk: R,
    mut on_error: E,
) -> c_int
where
    R: FnMut(&mut [u8], &mut bool) -> c_int,
    E: FnMut(&[u8]),
{
    dst.fill(0);
    let received = recv_chunk(dst, tls_error);
    if received < 0 {
        let err = last_errno();
        emit_error(
            &mut on_error,
            tls_error,
            b"Could not receive data on socket from %s",
            Some(host),
            true,
            err,
            None,
        );
    } else {
        *total_bytes_read = total_bytes_read.wrapping_add(received);
    }
    received
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_loopback_bind_connect_accept_readline_recv() {
        let srv_sock = AtomicI32::new(INVALID_SOCKET);
        let srv_status = AtomicI32::new(CS_DISCONNECTED);
        let mut srv_listener = false;
        let mut tls_err = false;
        let mut errs = Vec::new();

        assert!(bind_listener(
            b"127.0.0.1",
            0,
            4,
            &srv_sock,
            &srv_status,
            &mut srv_listener,
            &mut tls_err,
            |e| errs.push(e.to_vec()),
        ));
        assert!(errs.is_empty());
        assert_eq!(srv_status.load(Ordering::SeqCst), CS_LISTENING);

        let port = unsafe {
            let mut sin: libc::sockaddr_in = std::mem::zeroed();
            let mut len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
            assert_eq!(
                libc::getsockname(
                    srv_sock.load(Ordering::SeqCst),
                    (&mut sin as *mut libc::sockaddr_in).cast(),
                    &mut len,
                ),
                0
            );
            u16::from_be(sin.sin_port) as c_int
        };

        let cli_sock = AtomicI32::new(INVALID_SOCKET);
        let cli_status = AtomicI32::new(CS_DISCONNECTED);
        let mut debugs = Vec::new();
        assert!(connect_stream(
            b"127.0.0.1",
            port,
            4,
            5,
            &[],
            false,
            |_, _, _| true,
            || false,
            &cli_sock,
            &cli_status,
            &mut tls_err,
            |e| errs.push(e.to_vec()),
            |d| debugs.push(d.to_vec()),
        ));
        cli_status.store(CS_CONNECTED, Ordering::SeqCst);
        assert!(errs.is_empty());
        assert!(debugs
            .iter()
            .any(|d| d.starts_with(b"getaddrinfo for 127.0.0.1: 0")));

        let mut busy = false;
        let accepted_fd = accept_connection(
            b"127.0.0.1",
            5,
            &srv_sock,
            &srv_status,
            &mut busy,
            &mut tls_err,
            || false,
            |e| errs.push(e.to_vec()),
        );
        assert!(accepted_fd >= 0);
        assert!(!busy);
        assert_eq!(remote_addr(b"127.0.0.1", accepted_fd), b"127.0.0.1");

        let payload = b"200 ready\r\nBINARY1234";
        assert!(send_all(&cli_status, payload, |chunk| unsafe {
            libc::send(
                cli_sock.load(Ordering::SeqCst),
                chunk.as_ptr().cast(),
                chunk.len(),
                0,
            ) as c_int
        }));

        let acc_status = AtomicI32::new(CS_CONNECTED);
        let mut rbuf = [0u8; CONNECTION_READBUFFER_SIZE + 129];
        let mut off = 0usize;
        let mut avail = 0usize;
        let mut total = 0i32;
        let mut line = [0u8; 64];

        let n = read_line(
            b"127.0.0.1",
            &acc_status,
            &mut rbuf,
            &mut off,
            &mut avail,
            &mut total,
            &mut line,
            &mut tls_err,
            |dst, _| unsafe {
                libc::recv(accepted_fd, dst.as_mut_ptr().cast(), dst.len(), 0) as c_int
            },
            |e| errs.push(e.to_vec()),
        )
        .expect("line");
        assert_eq!(&line[..n], b"200 ready\r\n");

        let mut body = [0u8; 10];
        assert!(recv_exact(
            b"127.0.0.1",
            &rbuf,
            &mut off,
            &mut avail,
            &mut total,
            &mut body,
            &mut tls_err,
            |dst, _| unsafe {
                libc::recv(accepted_fd, dst.as_mut_ptr().cast(), dst.len(), 0) as c_int
            },
            |e| errs.push(e.to_vec()),
        ));
        assert_eq!(&body, b"BINARY1234");
        assert_eq!(total as usize, payload.len());

        let acc_sock = AtomicI32::new(accepted_fd);
        let mut false_listener = false;
        disconnect_socket(
            b"127.0.0.1",
            true,
            &mut false_listener,
            &acc_sock,
            &acc_status,
        );
        disconnect_socket(
            b"127.0.0.1",
            false,
            &mut false_listener,
            &cli_sock,
            &cli_status,
        );
        disconnect_socket(
            b"127.0.0.1",
            false,
            &mut srv_listener,
            &srv_sock,
            &srv_status,
        );
    }

    #[test]
    fn unix_socket_bind_connect_and_unlink_on_disconnect() {
        let sock_path = format!("/tmp/rg_sock_{}_{}.sock", std::process::id(), 219);
        let path_bytes = sock_path.as_bytes();

        let srv_sock = AtomicI32::new(INVALID_SOCKET);
        let srv_status = AtomicI32::new(CS_DISCONNECTED);
        let mut srv_listener = false;
        let mut tls_err = false;
        assert!(bind_listener(
            path_bytes,
            0,
            0,
            &srv_sock,
            &srv_status,
            &mut srv_listener,
            &mut tls_err,
            |_| {},
        ));
        assert!(std::path::Path::new(&sock_path).exists());

        let cli_sock = AtomicI32::new(INVALID_SOCKET);
        let cli_status = AtomicI32::new(CS_DISCONNECTED);
        let mut cli_listener = false;
        assert!(connect_stream(
            path_bytes,
            0,
            0,
            5,
            &[],
            false,
            |_, _, _| true,
            || false,
            &cli_sock,
            &cli_status,
            &mut tls_err,
            |_| {},
            |_| {},
        ));
        assert_eq!(
            remote_addr(path_bytes, cli_sock.load(Ordering::SeqCst)),
            b"-"
        );

        // Disconnecting client must NOT unlink listener socket
        disconnect_socket(path_bytes, false, &mut cli_listener, &cli_sock, &cli_status);
        assert!(std::path::Path::new(&sock_path).exists());

        // Disconnecting listener MUST unlink listener socket
        disconnect_socket(path_bytes, false, &mut srv_listener, &srv_sock, &srv_status);
        assert!(!std::path::Path::new(&sock_path).exists());
    }

    #[test]
    fn deadline_empty_resolved_never_blocks_and_multi_addr_falls_back() {
        let srv_sock = AtomicI32::new(INVALID_SOCKET);
        let srv_status = AtomicI32::new(CS_DISCONNECTED);
        let mut srv_listener = false;
        let mut tls_err = false;
        assert!(bind_listener(
            b"127.0.0.1",
            0,
            4,
            &srv_sock,
            &srv_status,
            &mut srv_listener,
            &mut tls_err,
            |_| {},
        ));
        let port = unsafe {
            let mut sin: libc::sockaddr_in = std::mem::zeroed();
            let mut len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
            libc::getsockname(
                srv_sock.load(Ordering::SeqCst),
                (&mut sin as *mut libc::sockaddr_in).cast(),
                &mut len,
            );
            u16::from_be(sin.sin_port) as c_int
        };

        let cli_sock = AtomicI32::new(INVALID_SOCKET);
        let cli_status = AtomicI32::new(CS_DISCONNECTED);
        // Empty resolved_addresses with has_deadline must fail immediately without DNS
        assert!(!connect_stream(
            b"localhost",
            port,
            4,
            5,
            &[],
            true,
            |_, _, _| true,
            || false,
            &cli_sock,
            &cli_status,
            &mut tls_err,
            |_| {},
            |_| {},
        ));

        // Second address of same family succeeds under deadline mode
        let addrs: [&[u8]; 2] = [b"255.255.255.255", b"127.0.0.1"];
        assert!(connect_stream(
            b"localhost",
            port,
            4,
            5,
            &addrs,
            true,
            |fd, _, _| unsafe {
                let mut pfd = libc::pollfd {
                    fd,
                    events: libc::POLLIN | libc::POLLOUT,
                    revents: 0,
                };
                libc::poll(&mut pfd, 1, 1000) > 0
            },
            || false,
            &cli_sock,
            &cli_status,
            &mut tls_err,
            |_| {},
            |_| {},
        ));

        cli_status.store(CS_CONNECTED, Ordering::SeqCst);
        let mut cli_listener = false;
        disconnect_socket(
            b"localhost",
            false,
            &mut cli_listener,
            &cli_sock,
            &cli_status,
        );
        disconnect_socket(
            b"127.0.0.1",
            false,
            &mut srv_listener,
            &srv_sock,
            &srv_status,
        );
    }

    #[test]
    fn cooperative_stop_wakes_accept_and_cancelled_recv_without_pthread_cancel() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let srv_sock = Arc::new(AtomicI32::new(INVALID_SOCKET));
        let srv_status = Arc::new(AtomicI32::new(CS_DISCONNECTED));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let mut srv_listener = false;
        let mut tls_err = false;
        assert!(bind_listener(
            b"127.0.0.1",
            0,
            4,
            &srv_sock,
            &srv_status,
            &mut srv_listener,
            &mut tls_err,
            |_| {},
        ));

        let srv_sock_t = Arc::clone(&srv_sock);
        let srv_status_t = Arc::clone(&srv_status);
        let stop_t = Arc::clone(&stop_flag);
        let accept_handle = std::thread::spawn(move || {
            let mut busy = false;
            let mut te = false;
            let mut errs = Vec::new();
            let fd = accept_connection(
                b"127.0.0.1",
                60,
                &srv_sock_t,
                &srv_status_t,
                &mut busy,
                &mut te,
                || stop_t.load(Ordering::SeqCst),
                |e| errs.push(e.to_vec()),
            );
            (fd, errs)
        });

        std::thread::sleep(std::time::Duration::from_millis(20));
        stop_flag.store(true, Ordering::SeqCst);
        let (acc_fd, errs) = accept_handle.join().expect("accept thread");
        assert_eq!(acc_fd, INVALID_SOCKET);
        assert!(errs.is_empty());

        disconnect_socket(
            b"127.0.0.1",
            false,
            &mut srv_listener,
            &srv_sock,
            &srv_status,
        );

        // Verify cancel_socket wakes a blocked recv_exact promptly via shutdown(SHUT_RDWR).
        let mut pair = [INVALID_SOCKET; 2];
        assert_eq!(
            unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, pair.as_mut_ptr()) },
            0
        );
        init_socket_opts(pair[0], 60).expect("opts");
        let r_sock = Arc::new(AtomicI32::new(pair[0]));
        let r_status = Arc::new(AtomicI32::new(CS_CONNECTED));
        let r_sock_t = Arc::clone(&r_sock);
        let recv_handle = std::thread::spawn(move || {
            let rbuf = [0u8; CONNECTION_READBUFFER_SIZE];
            let mut off = 0usize;
            let mut avail = 0usize;
            let mut total = 0i32;
            let mut dst = [0u8; 4];
            let mut te = false;
            recv_exact(
                b"peer",
                &rbuf,
                &mut off,
                &mut avail,
                &mut total,
                &mut dst,
                &mut te,
                |chunk, _| unsafe {
                    libc::recv(
                        r_sock_t.load(Ordering::SeqCst),
                        chunk.as_mut_ptr().cast(),
                        chunk.len(),
                        0,
                    ) as c_int
                },
                |_| {},
            )
        });

        std::thread::sleep(std::time::Duration::from_millis(20));
        cancel_socket(false, &r_sock, &r_status);
        assert!(!recv_handle.join().expect("recv thread"));

        let mut dummy_listener = false;
        disconnect_socket(b"peer", false, &mut dummy_listener, &r_sock, &r_status);
        unsafe {
            libc::close(pair[1]);
        }

        // Verify connect_stream aborts immediately when stop callback is true.
        let cli_sock = AtomicI32::new(INVALID_SOCKET);
        let cli_status = AtomicI32::new(CS_DISCONNECTED);
        assert!(!connect_stream(
            b"127.0.0.1",
            9,
            4,
            60,
            &[b"127.0.0.1"],
            false,
            |_, _, _| true,
            || true,
            &cli_sock,
            &cli_status,
            &mut tls_err,
            |_| {},
            |_| {},
        ));
    }

    #[test]
    fn report_error_preserves_non_utf8_and_honors_tls_suppression() {
        let mut tls_err = true;
        assert!(format_report_error(
            &mut tls_err,
            b"Connection to %s failed",
            Some(b"\xff\xfe"),
            false,
            0,
            None
        )
        .is_none());
        assert!(!tls_err);

        let out = format_report_error(
            &mut tls_err,
            b"Connection to %s failed",
            Some(b"host\xffname"),
            true,
            111,
            Some(b"custom\xfeerr"),
        )
        .expect("formatted");
        assert_eq!(
            out,
            b"Connection to host\xffname failed: Error 111 - custom\xfeerr"
        );
    }
}
